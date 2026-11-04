use crate::{BrushSet, CelReplay, ENGINE_VERSION, Raster};
use anyhow::{Context, Result, bail};
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::rng::{FNV_OFFSET, fnv};
use klad_core::stroke::{Mark, decode_cel};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub const BUDGET: u64 = 2 << 30;
pub const SLOW: Duration = Duration::from_millis(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    Cached,
    Updated(usize),
    Replayed,
}

pub struct CelLoad {
    pub raster: Raster,
    pub replay: CelReplay,
    pub file_hash: u64,
    pub how: How,
    pub strokes: usize,
    pub dabs: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    Always,
    WhenSlow,
    Never,
}

#[derive(Clone, Debug)]
pub struct CelCache {
    dir: PathBuf,
    base: u64,
    size: (u32, u32),
    write: Write,
}

type Record = (Vec<(u32, u64)>, Vec<Vec<u32>>);

impl CelCache {
    pub fn new(bundle: &Bundle, brushes: &BrushSet, write: Write) -> Self {
        let size = (bundle.doc.canvas.width, bundle.doc.canvas.height);
        let base = fnv(
            fnv(
                FNV_OFFSET,
                &[ENGINE_VERSION, size.0, size.1].map(u32::to_le_bytes).concat(),
            ),
            &brushes.fingerprint().to_le_bytes(),
        );
        Self {
            dir: bundle.cache_dir().join("tiles"),
            base,
            size,
            write,
        }
    }

    pub fn key(&self, bytes: &[u8]) -> u64 {
        fnv(self.base, bytes)
    }

    pub fn holds_key(&self, cel: &str, key: u64) -> bool {
        let mut head = [0u8; 8];
        fs::File::open(self.paths(cel).0)
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
            .is_ok_and(|_| head == key.to_le_bytes())
    }

    pub fn holds(&self, bundle: &Bundle, layer: &str, cel: &str) -> bool {
        let bytes = fs::read(bundle.strokes_path(layer, cel)).unwrap_or_default();
        self.holds_key(cel, self.key(&bytes))
    }

    fn paths(&self, cel: &str) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{cel}.raster")),
            self.dir.join(format!("{cel}.marks")),
        )
    }

    fn raster(&self, cel: &str, key: u64) -> Result<Raster> {
        let path = self.paths(cel).0;
        let bytes = fs::read(&path)?;
        if bytes.len() < 8 || bytes[..8] != key.to_le_bytes() {
            bail!("stale cache");
        }
        let raster = Raster::decode(&zstd::decode_all(&bytes[8..])?)?;
        let _ = fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(SystemTime::now()));
        Ok(raster)
    }

    fn record(&self, cel: &str) -> Result<(u64, Record)> {
        let bytes = fs::read(self.paths(cel).1)?;
        if bytes.len() < 16 || bytes[8..16] != self.base.to_le_bytes() {
            bail!("the record was made with other brushes or another canvas");
        }
        let key = u64::from_le_bytes(bytes[..8].try_into()?);
        Ok((key, serde_json::from_slice(&zstd::decode_all(&bytes[16..])?)?))
    }

    pub fn load(&self, bundle: &Bundle, brushes: &BrushSet, layer: &str, cel: &str, want: bool) -> Result<CelLoad> {
        let path = bundle.strokes_path(layer, cel);
        let bytes = if path.exists() { fs::read(&path)? } else { Vec::new() };
        let key = self.key(&bytes);
        let file_hash = fnv(FNV_OFFSET, &bytes);
        let marks = || -> Result<Vec<Mark>> {
            match bytes.is_empty() {
                true => Ok(Vec::new()),
                false => decode_cel(&bytes).with_context(|| format!("cel {cel}")),
            }
        };
        let (w, h) = self.size;
        if let Ok(raster) = self.raster(cel, key) {
            let replay = match want {
                false => Some(CelReplay::new(w, h)),
                true => self
                    .record(cel)
                    .ok()
                    .filter(|(k, _)| *k == key)
                    .and_then(|(_, (keys, footprints))| {
                        CelReplay::from_cache(w, h, keys, footprints).with_marks(marks().ok()?)
                    }),
            };
            if let Some(replay) = replay {
                return Ok(CelLoad {
                    raster,
                    replay,
                    file_hash,
                    how: How::Cached,
                    strokes: 0,
                    dabs: 0,
                });
            }
        }
        let marks = marks()?;
        let start = Instant::now();
        let stale = self.record(cel).and_then(|(k, (keys, footprints))| {
            Ok((CelReplay::from_cache(w, h, keys, footprints), self.raster(cel, k)?))
        });
        let (replay, raster, how, strokes, dabs) = match stale {
            Ok((mut replay, mut raster)) => {
                let done = replay
                    .update(&mut raster, marks, brushes)
                    .with_context(|| format!("cel {cel}"))?;
                (replay, raster, How::Updated(done.replayed), done.replayed, 0)
            }
            Err(_) => {
                let count = marks.len();
                let (replay, raster, dabs) =
                    CelReplay::replay(w, h, marks, brushes).with_context(|| format!("cel {cel}"))?;
                (replay, raster, How::Replayed, count, dabs)
            }
        };
        let store = match self.write {
            Write::Always => true,
            Write::WhenSlow => start.elapsed() >= SLOW,
            Write::Never => false,
        };
        if store {
            self.store(cel, key, &raster, &replay)?;
        }
        Ok(CelLoad {
            raster,
            replay,
            file_hash,
            how,
            strokes,
            dabs,
        })
    }

    pub fn strokes(&self, bundle: &Bundle, layer: &str, cel: &str) -> Result<Option<(CelReplay, u64)>> {
        let path = bundle.strokes_path(layer, cel);
        let bytes = if path.exists() { fs::read(&path)? } else { Vec::new() };
        let Ok((key, (keys, footprints))) = self.record(cel) else {
            return Ok(None);
        };
        if key != self.key(&bytes) {
            return Ok(None);
        }
        let marks = match bytes.is_empty() {
            true => Vec::new(),
            false => decode_cel(&bytes).with_context(|| format!("cel {cel}"))?,
        };
        let (w, h) = self.size;
        Ok(CelReplay::from_cache(w, h, keys, footprints)
            .with_marks(marks)
            .map(|replay| (replay, fnv(FNV_OFFSET, &bytes))))
    }

    pub fn store(&self, cel: &str, key: u64, raster: &Raster, replay: &CelReplay) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let (pixels, record) = self.paths(cel);
        let mut bytes = key.to_le_bytes().to_vec();
        bytes.extend(compress(raster)?);
        write_atomic(&pixels, &bytes)?;
        let mut bytes = [key.to_le_bytes(), self.base.to_le_bytes()].concat();
        let kept: (&[(u32, u64)], &[Vec<u32>]) = (replay.keys(), replay.footprints());
        bytes.extend(zstd::encode_all(&serde_json::to_vec(&kept)?[..], 1)?);
        write_atomic(&record, &bytes)?;
        prune(&self.dir, BUDGET);
        Ok(())
    }
}

pub fn compress(raster: &Raster) -> Result<Vec<u8>> {
    Ok(zstd::encode_all(&raster.encode()[..], 1)?)
}

pub fn prune(dir: &Path, budget: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= budget {
        return;
    }
    files.sort();
    files.into_iter().try_fold(total, |left, (_, len, path)| {
        if left <= budget {
            return None;
        }
        let _ = fs::remove_file(&path);
        Some(left - len)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Document, Layer};
    use klad_core::stroke::marks;
    use klad_core::synth::Synth;

    #[test]
    fn a_cached_drawing_loads_its_pixels_and_edit_record_and_an_edited_one_updates() {
        let dir = std::env::temp_dir().join(format!("draw-cel-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 400);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let bundle = Bundle::create(&dir, doc).unwrap();
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let strokes = Synth::canvas(600.0, 400.0).strokes(40, 7);
        bundle.write_marks("l1", "c1", &marks(strokes.clone())).unwrap();
        let cache = CelCache::new(&bundle, &brushes, Write::Always);
        let first = cache.load(&bundle, &brushes, "l1", "c1", true).unwrap();
        assert_eq!(first.how, How::Replayed);
        assert!(cache.holds(&bundle, "l1", "c1"));
        let again = cache.load(&bundle, &brushes, "l1", "c1", true).unwrap();
        assert_eq!(again.how, How::Cached);
        assert_eq!(again.raster.hash(), first.raster.hash());
        assert_eq!(again.replay.marks(), first.replay.marks());
        assert_eq!(again.replay.keys(), first.replay.keys());
        assert_eq!(again.replay.footprints(), first.replay.footprints());
        let more = Synth::canvas(600.0, 400.0).strokes(3, 99);
        let edited = marks(strokes.into_iter().chain(more));
        bundle.write_marks("l1", "c1", &edited).unwrap();
        assert!(!cache.holds(&bundle, "l1", "c1"), "a changed drawing is not held");
        let updated = cache.load(&bundle, &brushes, "l1", "c1", true).unwrap();
        assert!(matches!(updated.how, How::Updated(n) if n < 43), "{:?}", updated.how);
        let (_, fresh, _) = CelReplay::replay(600, 400, edited.clone(), &brushes).unwrap();
        assert_eq!(updated.raster.hash(), fresh.hash());
        assert_eq!(updated.replay.marks(), &edited[..]);
        assert!(
            CelReplay::from_cache(
                600,
                400,
                again.replay.keys().to_vec(),
                again.replay.footprints().to_vec()
            )
            .with_marks(edited)
            .is_none()
        );
        let lazy = CelCache::new(&bundle, &brushes, Write::Never);
        fs::remove_dir_all(dir.join(".cache")).unwrap();
        assert_eq!(
            lazy.load(&bundle, &brushes, "l1", "c1", true).unwrap().how,
            How::Replayed
        );
        assert!(!dir.join(".cache/tiles/c1.raster").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_cache_keeps_to_its_budget_dropping_the_oldest_files() {
        let dir = std::env::temp_dir().join(format!("draw-cel-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (0..5).for_each(|i| {
            fs::write(dir.join(format!("c{i}.raster")), vec![0u8; 1000]).unwrap();
            let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + i * 60);
            fs::File::options()
                .write(true)
                .open(dir.join(format!("c{i}.raster")))
                .unwrap()
                .set_modified(when)
                .unwrap();
        });
        prune(&dir, 2500);
        let left: Vec<bool> = (0..5).map(|i| dir.join(format!("c{i}.raster")).exists()).collect();
        assert_eq!(left, [false, false, false, true, true]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
