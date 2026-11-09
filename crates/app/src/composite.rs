use crate::planes;
use anyhow::{Context, Result, anyhow};
use half::f16;
use klad_brush::TILE;
use klad_brush::brush::StrokeBlend;
use klad_brush::cache::{CelCache, Write};
use klad_brush::tiles::TILE_PX;
use klad_brush::{BrushSet, CelReplay, Raster, StrokeBuffer};
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::doc::{Blend, Document, Kind, Layer, Loc, MASK, Node};
use klad_core::fill::{Fill, FillRef};
use klad_core::history::record;
use klad_core::look::Effect;
use klad_core::ops::{Applied, Op, apply_all};
use klad_core::patch::{Patch, diff};
use klad_core::rng::{FNV_OFFSET, fnv};
use klad_core::stroke::{Mark, Stroke, decode_cel, encode_cel};
use klad_render::onion::{self, Ghost, Onion, Onions};
use klad_render::{
    Rasters, Swap, SwapTiles, TileItem, TileJob, compose_tile, fill_reference, flatten_with, rasterize_nodes,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const CACHE_BYTES: usize = 1 << 30;

pub type Tiles = Vec<(usize, Vec<f16>)>;

pub(crate) fn effect_reach(doc: &Document, layers: &[Layer]) -> i32 {
    layers
        .iter()
        .filter(|l| l.visible)
        .map(|l| klad_render::effects::margin(&doc.effects_of(l), 1.0) + effect_reach(doc, l.children()))
        .max()
        .unwrap_or(0)
}

pub(crate) fn dilate(tiles: &[usize], reach: i32, cols: usize, rows: usize) -> Vec<usize> {
    let n = (reach.max(0) as usize).div_ceil(TILE) as i64;
    if n == 0 {
        return tiles.to_vec();
    }
    tiles
        .iter()
        .flat_map(|&i| {
            let (x, y) = ((i % cols) as i64, (i / cols) as i64);
            (-n..=n).flat_map(move |dy| (-n..=n).map(move |dx| (x + dx, y + dy)))
        })
        .filter(|(x, y)| *x >= 0 && *y >= 0 && (*x as usize) < cols && (*y as usize) < rows)
        .map(|(x, y)| y as usize * cols + x as usize)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn blends_through(layer: &Layer) -> bool {
    layer.visible
        && (layer.blend != Blend::Normal
            || layer.reads_under()
            || layer
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Glow(g) if g.blend != Blend::Normal))
            || layer.children().iter().any(blends_through))
}

fn live_above(above: &[&Layer]) -> bool {
    above.iter().any(|l| blends_through(l))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    inode: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
        inode: meta.ino(),
    })
}

pub struct CelData {
    pub replay: CelReplay,
    file_hash: u64,
}

impl CelData {
    pub fn marks(&self) -> &[Mark] {
        self.replay.marks()
    }
}

pub struct Preview<'a> {
    pub buffer: &'a StrokeBuffer,
    pub twin: Option<&'a StrokeBuffer>,
    pub color: [f32; 4],
    pub opacity: f32,
    pub blend: StrokeBlend,
}

pub struct Scene {
    pub bundle: Bundle,
    pub active: Vec<usize>,
    pub frame: u32,
    rasters: Rasters,
    cels: BTreeMap<String, CelData>,
    vector_hash: BTreeMap<String, u64>,
    memo: std::collections::VecDeque<(String, u64, Raster)>,
    below: Raster,
    above: Raster,
    fast: bool,
    unsaved: Patch,
    pub tint_mask: bool,
    used: BTreeMap<String, u64>,
    tick: u64,
    budget: usize,
    pub stale: bool,
    pub onion: Onion,
    pub onion_on: bool,
    pub onion_marked: BTreeSet<String>,
    onions: Onions,
    onion_key: u64,
    hashes: BTreeMap<String, (Stamp, u64)>,
    pub keep_after: std::time::Duration,
    pub lean: bool,
}

pub type Plan = Vec<(String, Vec<Ghost>)>;

pub struct FrameImage {
    pub key: u64,
    pub width: u32,
    pub height: u32,
    pub mips: Vec<Vec<u8>>,
}

impl FrameImage {
    pub fn bytes(&self) -> usize {
        self.mips.iter().map(Vec::len).sum()
    }
}

pub struct FrameRequest {
    pub frame: u32,
    pub key: u64,
    level: u32,
    rev: u64,
    bundle: Bundle,
    rasters: Rasters,
    paint: Vec<(String, String)>,
    vector: Vec<(String, u64, Vec<Node>)>,
    plan: Option<Plan>,
    brushes: BrushSet,
    disk: Option<std::path::PathBuf>,
    keep_after: std::time::Duration,
    planes: Option<PlanesAsk>,
}

#[derive(Clone)]
pub struct PlanesAsk {
    pub base: u64,
    pub cels: Vec<(String, String)>,
    pub dir: std::path::PathBuf,
    pub active: Vec<usize>,
}

impl PlanesAsk {
    fn key(&self, bundle: &Bundle, brushes: &BrushSet) -> u64 {
        let cache = CelCache::new(bundle, brushes, Write::Never);
        let keys: Vec<u64> = self
            .cels
            .par_iter()
            .map(|(l, c)| cache.key(&fs::read(bundle.strokes_path(l, c)).unwrap_or_default()))
            .collect();
        planes::key(self.base, &keys)
    }
}

fn thumbs_of(doc: &Document, frame: u32, rasters: &Rasters) -> planes::Thumbs {
    let size = (doc.canvas.width, doc.canvas.height);
    paint_cels(doc, frame)
        .par_iter()
        .filter_map(|(_, c)| rasters.get(c).map(|r| (c.clone(), thumbnail(r, size, THUMB))))
        .collect()
}

pub struct FrameDone {
    pub image: FrameImage,
    rev: u64,
    paint: Vec<Loaded>,
    vector: Vec<(String, u64, Raster)>,
}

impl FrameDone {
    pub fn loaded_bytes(&self) -> usize {
        self.paint.iter().map(|(_, r, _)| r.bytes()).sum::<usize>()
            + self.vector.iter().map(|(_, _, r)| r.bytes()).sum::<usize>()
    }
}

fn image_at(raster: &Raster, level: u32) -> (u32, u32, Vec<u8>) {
    let step = 1usize << level;
    let (w, h) = (
        (raster.width as usize).div_ceil(step),
        (raster.height as usize).div_ceil(step),
    );
    let side = TILE / step;
    let mut out = vec![0u8; w * h * 4];
    let rows: Vec<(usize, Vec<u8>)> = (0..raster.rows)
        .into_par_iter()
        .map(|ty| {
            let mut band = vec![0u8; w * side * 4];
            (0..raster.cols).for_each(|tx| {
                let Some(tile) = raster.at(ty * raster.cols + tx) else {
                    return;
                };
                (0..side).for_each(|y| {
                    (0..side).for_each(|x| {
                        let ox = tx * side + x;
                        if ox >= w {
                            return;
                        }
                        let mut sum = [0.0f32; 4];
                        (0..step).for_each(|dy| {
                            (0..step).for_each(|dx| {
                                let at = ((y * step + dy) * TILE + x * step + dx) * 4;
                                (0..4).for_each(|c| sum[c] += tile[at + c].to_f32());
                            })
                        });
                        let n = (step * step) as f32;
                        let to = (y * w + ox) * 4;
                        (0..4).for_each(|c| band[to + c] = ((sum[c] / n).clamp(0.0, 1.0) * 255.0).round() as u8);
                    })
                });
            });
            (ty, band)
        })
        .collect();
    rows.into_iter().for_each(|(ty, band)| {
        let y0 = ty * side;
        let lines = side.min(h.saturating_sub(y0));
        out[y0 * w * 4..(y0 + lines) * w * 4].copy_from_slice(&band[..lines * w * 4]);
    });
    (w as u32, h as u32, out)
}

fn mip_chain(width: u32, height: u32, base: Vec<u8>) -> Vec<Vec<u8>> {
    let levels = crate::gpu::levels_for((width, height));
    let mut out = vec![base];
    let (mut w, mut h) = (width as usize, height as usize);
    while out.len() < levels as usize {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let last = out.last().expect("a base level");
        let next: Vec<u8> = (0..nw * nh * 4)
            .map(|i| {
                let (c, px) = (i % 4, i / 4);
                let (x, y) = ((px % nw) * 2, (px / nw) * 2);
                let at = |x: usize, y: usize| last[(y.min(h - 1) * w + x.min(w - 1)) * 4 + c] as u32;
                ((at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1) + 2) / 4) as u8
            })
            .collect();
        out.push(next);
        (w, h) = (nw, nh);
    }
    out
}

pub const THUMB: (u32, u32) = (40, 28);

pub fn thumbnail(raster: &Raster, (cw, ch): (u32, u32), (tw, th): (u32, u32)) -> egui::ColorImage {
    let scale = (cw as f32 / tw as f32).max(ch as f32 / th as f32).max(1e-3);
    let (ox, oy) = (
        (tw as f32 * scale - cw as f32) / 2.0,
        (th as f32 * scale - ch as f32) / 2.0,
    );
    let step = ((scale / 12.0).ceil() as usize).max(1);
    let texel = |px: usize, py: usize| -> [f32; 4] {
        raster.get(px / TILE, py / TILE).map_or([0.0; 4], |t| {
            let i = ((py % TILE) * TILE + px % TILE) * 4;
            [t[i].to_f32(), t[i + 1].to_f32(), t[i + 2].to_f32(), t[i + 3].to_f32()]
        })
    };
    let pixels = (0..th)
        .flat_map(|y| (0..tw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let checker = if (x / 4 + y / 4) % 2 == 0 { 0.82 } else { 0.66 };
            let span = |t: u32, o: f32, size: u32| {
                let a = ((t as f32 * scale - o).max(0.0) as usize).min(size as usize);
                let b = (((t + 1) as f32 * scale - o).max(0.0) as usize).min(size as usize);
                (a..b).step_by(step)
            };
            let (sum, n) = span(y, oy, ch)
                .flat_map(|py| span(x, ox, cw).map(move |px| (px, py)))
                .fold(([0.0f32; 4], 0usize), |(s, n), (px, py)| {
                    let t = texel(px, py);
                    ([s[0] + t[0], s[1] + t[1], s[2] + t[2], s[3] + t[3]], n + 1)
                });
            let mean = sum.map(|v| v / n.max(1) as f32);
            let alpha = mean[3].clamp(0.0, 1.0);
            let shown = alpha.sqrt();
            let [r, g, b] = [0, 1, 2].map(|k| {
                let straight = if alpha > 1e-4 { mean[k] / alpha } else { 0.0 };
                ((straight * shown + checker * (1.0 - shown)).clamp(0.0, 1.0) * 255.0) as u8
            });
            egui::Color32::from_rgb(r, g, b)
        })
        .collect();
    egui::ColorImage {
        size: [tw as usize, th as usize],
        source_size: egui::vec2(tw as f32, th as f32),
        pixels,
    }
}

const PICTURE: &[u8; 4] = b"DAP1";
pub const PICTURES: u64 = 1 << 30;

pub fn picture_path(dir: &Path, key: u64) -> std::path::PathBuf {
    dir.join(format!("{key:016x}.pic"))
}

fn read_picture(dir: &Path, key: u64) -> Option<FrameImage> {
    let path = picture_path(dir, key);
    let bytes = fs::read(&path).ok()?;
    let head = |i: usize| {
        bytes
            .get(4 + i * 4..8 + i * 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (engine, width, height) = (head(0)?, head(1)?, head(2)?);
    if bytes.get(..4)? != PICTURE || engine != klad_brush::ENGINE_VERSION {
        return None;
    }
    let base = zstd::decode_all(bytes.get(16..)?).ok()?;
    if base.len() != width as usize * height as usize * 4 {
        return None;
    }
    let _ = fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|f| f.set_modified(std::time::SystemTime::now()));
    Some(FrameImage {
        key,
        width,
        height,
        mips: mip_chain(width, height, base),
    })
}

fn write_picture(dir: &Path, image: &FrameImage) -> Result<()> {
    fs::create_dir_all(dir)?;
    let mut bytes = PICTURE.to_vec();
    [klad_brush::ENGINE_VERSION, image.width, image.height]
        .iter()
        .for_each(|v| bytes.extend(v.to_le_bytes()));
    bytes.extend(zstd::encode_all(&image.mips[0][..], 1)?);
    write_atomic(&picture_path(dir, image.key), &bytes)?;
    klad_brush::cache::prune(dir, PICTURES);
    Ok(())
}

fn picture(
    doc: &Document,
    frame: u32,
    key: u64,
    level: u32,
    rasters: &Rasters,
    onions: Option<&Onions>,
) -> Result<FrameImage> {
    let layers: Vec<&Layer> = doc.layers.iter().collect();
    let flat = flatten_with(
        doc,
        frame,
        rasters,
        &layers,
        doc.canvas.background.as_deref(),
        onions,
        true,
    )?;
    let (width, height, base) = image_at(&flat, level);
    Ok(FrameImage {
        key,
        width,
        height,
        mips: mip_chain(width, height, base),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared {
    Already,
    Drawings(usize),
    Frame(usize),
}

impl FrameRequest {
    pub fn prepare(self) -> Result<Prepared> {
        let dir = self
            .disk
            .clone()
            .ok_or_else(|| crate::report::bug("the frame pictures have no folder"))?;
        let cache = CelCache::new(&self.bundle, &self.brushes, Write::Always);
        let pictured = picture_path(&dir, self.key).exists();
        let planes_key = self.planes.as_ref().map(|p| p.key(&self.bundle, &self.brushes));
        let planned = self
            .planes
            .as_ref()
            .zip(planes_key)
            .is_none_or(|(p, key)| planes::path(&p.dir, key).exists());
        let todo: Vec<&(String, String)> = self
            .paint
            .iter()
            .filter(|(layer, cel)| !cache.holds(&self.bundle, layer, cel))
            .collect();
        if pictured && planned && todo.is_empty() {
            return Ok(Prepared::Already);
        }
        if pictured && planned {
            todo.par_iter()
                .try_for_each(|(layer, cel)| cache.load(&self.bundle, &self.brushes, layer, cel, false).map(|_| ()))?;
            return Ok(Prepared::Drawings(todo.len()));
        }
        let doc = &self.bundle.doc;
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let mut rasters: Rasters = self
            .paint
            .par_iter()
            .map(|(layer, cel)| {
                cache
                    .load(&self.bundle, &self.brushes, layer, cel, false)
                    .map(|l| (cel.clone(), l.raster))
            })
            .collect::<Result<_>>()?;
        let vectors = self
            .vector
            .par_iter()
            .map(|(cel, _, nodes)| rasterize_nodes(nodes, w, h).map(|r| (cel.clone(), r)))
            .collect::<Result<Vec<_>>>()?;
        rasters.extend(vectors);
        let onions = self.plan.as_ref().map(|plan| onion::build(doc, plan, &rasters));
        if !pictured {
            write_picture(
                &dir,
                &picture(doc, self.frame, self.key, self.level, &rasters, onions.as_ref())?,
            )?;
        }
        if let (Some(p), Some(key), false) = (&self.planes, planes_key, planned)
            && let Some((below, above)) = caches(doc, self.frame, &p.active, &rasters, &onions.unwrap_or_default())?
        {
            let thumbs = match !self.paint.is_empty() {
                true => thumbs_of(doc, self.frame, &rasters),
                false => Vec::new(),
            };
            planes::write(&p.dir, key, &below, &above, &thumbs)?;
        }
        Ok(match pictured {
            true => Prepared::Drawings(todo.len()),
            false => Prepared::Frame(todo.len()),
        })
    }

    pub fn run(self) -> Result<FrameDone> {
        if let Some(image) = self.disk.as_deref().and_then(|d| read_picture(d, self.key)) {
            return Ok(FrameDone {
                image,
                rev: self.rev,
                paint: Vec::new(),
                vector: Vec::new(),
            });
        }
        let start = std::time::Instant::now();
        let doc = &self.bundle.doc;
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let paint = self
            .paint
            .par_iter()
            .map(|(layer, cel)| {
                load_paint(&self.bundle, layer, cel, &self.brushes).with_context(|| format!("cel {cel}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let vector = self
            .vector
            .par_iter()
            .map(|(cel, hash, nodes)| rasterize_nodes(nodes, w, h).map(|r| (cel.clone(), *hash, r)))
            .collect::<Result<Vec<_>>>()?;
        let mut rasters = self.rasters;
        paint.iter().for_each(|(id, raster, _)| {
            rasters.insert(id.clone(), raster.clone());
        });
        vector.iter().for_each(|(id, _, raster)| {
            rasters.insert(id.clone(), raster.clone());
        });
        let onions = self.plan.as_ref().map(|plan| onion::build(doc, plan, &rasters));
        let image = picture(doc, self.frame, self.key, self.level, &rasters, onions.as_ref())?;
        if let Some(dir) = self.disk.as_deref().filter(|_| start.elapsed() >= self.keep_after) {
            write_picture(dir, &image)?;
        }
        let mut paint = paint;
        paint.iter_mut().for_each(|(_, _, data)| data.replay.drop_checkpoints());
        Ok(FrameDone {
            image,
            rev: self.rev,
            paint,
            vector,
        })
    }
}

pub struct FrameJob {
    bundle: Bundle,
    paint: Vec<(String, String)>,
    vector: Vec<(String, u64, Vec<Node>)>,
    brushes: BrushSet,
}

pub struct LoadedFrame {
    paint: Vec<Loaded>,
    vector: Vec<(String, u64, Raster)>,
}

pub struct CanvasRequest {
    frame: u32,
    active: Vec<usize>,
    rev: u64,
    bundle: Bundle,
    rasters: Rasters,
    paint: Vec<(String, String)>,
    vector: Vec<(String, u64, Vec<Node>)>,
    plan: Plan,
    onion: bool,
    lean: bool,
    keep: Option<String>,
    brushes: BrushSet,
    planes: Option<PlanesAsk>,
    picture: Option<(
        u64,
        u32,
        std::path::PathBuf,
        std::sync::Arc<std::sync::atomic::AtomicU64>,
    )>,
}

pub struct CanvasDone {
    pub frame: u32,
    pub loaded: usize,
    pub bytes: usize,
    active: Vec<usize>,
    rev: u64,
    paint: Vec<Loaded>,
    vector: Vec<(String, u64, Raster)>,
    onions: Onions,
    caches: Option<(Raster, Raster)>,
    pub thumbs: Vec<(String, egui::ColorImage)>,
    strokes: Vec<(String, CelData)>,
}

impl CanvasRequest {
    pub fn with_picture(
        self,
        key: u64,
        level: u32,
        dir: std::path::PathBuf,
        written: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            picture: Some((key, level, dir, written)),
            ..self
        }
    }

    pub fn run(self) -> Result<CanvasDone> {
        let doc = &self.bundle.doc;
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let planes_key = self.planes.as_ref().map(|p| p.key(&self.bundle, &self.brushes));
        let stored = self
            .planes
            .as_ref()
            .zip(planes_key)
            .and_then(|(p, key)| planes::read(&p.dir, key).ok());
        let keep: BTreeSet<&str> = self
            .keep
            .iter()
            .map(String::as_str)
            .chain(
                self.plan
                    .iter()
                    .flat_map(|(_, ghosts)| ghosts.iter().map(|g| g.cel.as_str())),
            )
            .collect();
        let lean_hit = stored.is_some() && self.lean;
        let records = CelCache::new(&self.bundle, &self.brushes, Write::Never);
        let (wanted, strokes): Wanted = self
            .paint
            .par_iter()
            .map(|pair| {
                let (layer, cel) = pair;
                match lean_hit && !keep.contains(cel.as_str()) {
                    true => match records.strokes(&self.bundle, layer, cel).ok().flatten() {
                        Some((replay, file_hash)) => (None, Some((cel.clone(), CelData { replay, file_hash }))),
                        None => (Some(pair), None),
                    },
                    false => (Some(pair), None),
                }
            })
            .fold(
                || (Vec::new(), Vec::new()),
                |(mut w, mut s), (a, b)| {
                    w.extend(a);
                    s.extend(b);
                    (w, s)
                },
            )
            .reduce(
                || (Vec::new(), Vec::new()),
                |(mut w, mut s), (a, b)| {
                    w.extend(a);
                    s.extend(b);
                    (w, s)
                },
            );
        let mut paint = wanted
            .par_iter()
            .map(|(layer, cel)| {
                load_paint(&self.bundle, layer, cel, &self.brushes).with_context(|| format!("cel {cel}"))
            })
            .collect::<Result<Vec<_>>>()?;
        paint
            .iter_mut()
            .filter(|(id, _, _)| Some(id) != self.keep.as_ref())
            .for_each(|(_, _, data)| data.replay.drop_checkpoints());
        let vector = self
            .vector
            .par_iter()
            .map(|(cel, hash, nodes)| rasterize_nodes(nodes, w, h).map(|r| (cel.clone(), *hash, r)))
            .collect::<Result<Vec<_>>>()?;
        let mut rasters = self.rasters;
        paint.iter().for_each(|(id, raster, _)| {
            rasters.insert(id.clone(), raster.clone());
        });
        vector.iter().for_each(|(id, _, raster)| {
            rasters.insert(id.clone(), raster.clone());
        });
        let onions = onion::build(doc, &self.plan, &rasters);
        let (caches, kept_thumbs) = match stored {
            Some(stored) => (Some((stored.below, stored.above)), Some(stored.thumbs)),
            None => (caches(doc, self.frame, &self.active, &rasters, &onions)?, None),
        };
        if let Some((key, level, dir, written)) = self
            .picture
            .filter(|(key, _, dir, _)| !picture_path(dir, *key).exists())
        {
            let (doc, frame, rasters) = (doc.clone(), self.frame, rasters.clone());
            let onions = self.onion.then(|| onions.clone());
            crate::background().spawn(move || {
                if let Ok(image) = picture(&doc, frame, key, level, &rasters, onions.as_ref())
                    && write_picture(&dir, &image).is_ok()
                {
                    written.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
        let bytes = paint.iter().map(|(_, r, _)| r.bytes()).sum::<usize>()
            + vector.iter().map(|(_, _, r)| r.bytes()).sum::<usize>();
        let thumbs = match (kept_thumbs, self.lean || bytes > crate::frames::HEAVY) {
            (Some(kept), _) => kept,
            (None, true) => thumbs_of(doc, self.frame, &rasters),
            (None, false) => Vec::new(),
        };
        if let (Some(p), Some(key), Some((below, above))) = (&self.planes, planes_key, &caches)
            && !planes::path(&p.dir, key).exists()
        {
            let (dir, below, above, thumbs) = (p.dir.clone(), below.clone(), above.clone(), thumbs.clone());
            crate::background().spawn(move || {
                let _ = planes::write(&dir, key, &below, &above, &thumbs);
            });
        }
        Ok(CanvasDone {
            frame: self.frame,
            loaded: paint.len() + vector.len(),
            bytes,
            thumbs,
            active: self.active,
            rev: self.rev,
            paint,
            vector,
            onions,
            caches,
            strokes,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compose_items(
    doc: &Document,
    frame: u32,
    rasters: &Rasters,
    onions: Option<&Onions>,
    items: &[&Layer],
    index: usize,
    background: Option<&str>,
    under: Option<&[f16]>,
) -> Result<Option<Vec<f16>>> {
    let cols = (doc.canvas.width as usize).div_ceil(TILE);
    if items.iter().all(|l| !l.visible) && background.is_none() && under.is_none() {
        return Ok(None);
    }
    let visible: Vec<TileItem> = items
        .iter()
        .map(|l| TileItem::Layer(l))
        .chain(under.map(|tile| TileItem::Pixels {
            tile,
            opacity: 1.0,
            blend: Blend::Normal,
        }))
        .collect();
    let job = TileJob {
        frame,
        rasters,
        background,
        tx: index % cols,
        ty: index / cols,
        swap: None,
        onions,
        doc,
        references: true,
    };
    compose_tile(&visible, &job).map(Some)
}

fn fast_split<'a>(doc: &'a Document, active: &[usize]) -> Option<Split<'a>> {
    let is_paint = doc
        .locate_path(active)
        .is_some_and(|l| l.kind() == Kind::Paint && doc.effects_of(l).is_empty());
    split(doc, &doc.layers, active)
        .filter(|_| is_paint)
        .filter(|(_, _, through)| *through)
}

type Caches = Vec<(usize, Option<Vec<f16>>, Option<Vec<f16>>)>;

fn cache_tiles(
    doc: &Document,
    frame: u32,
    active: &[usize],
    rasters: &Rasters,
    onions: &Onions,
    tiles: &[usize],
) -> Result<Option<Caches>> {
    let Some((below, above, _)) = fast_split(doc, active) else {
        return Ok(None);
    };
    let id = doc.locate_path(active).map(|l| l.id.clone()).unwrap_or_default();
    let background = doc.canvas.background.as_deref();
    let map = (!onions.is_empty()).then_some(onions);
    let live = live_above(&above);
    tiles
        .par_iter()
        .map(|&i| {
            let under = onions.get(&id).and_then(|r| r.at(i));
            let upper = match live {
                true => None,
                false => compose_items(doc, frame, rasters, map, &above, i, None, None)?,
            };
            Ok((
                i,
                compose_items(doc, frame, rasters, map, &below, i, background, under)?,
                upper,
            ))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

fn caches(
    doc: &Document,
    frame: u32,
    active: &[usize],
    rasters: &Rasters,
    onions: &Onions,
) -> Result<Option<(Raster, Raster)>> {
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let all: Vec<usize> = {
        let r = Raster::new(w, h);
        (0..r.cols * r.rows).collect()
    };
    let Some(fresh) = cache_tiles(doc, frame, active, rasters, onions, &all)? else {
        return Ok(None);
    };
    let (mut below, mut above) = (Raster::new(w, h), Raster::new(w, h));
    fresh.into_iter().for_each(|(i, b, a)| {
        if let Some(b) = b {
            below.put(i, b.into_boxed_slice());
        }
        if let Some(a) = a {
            above.put(i, a.into_boxed_slice());
        }
    });
    Ok(Some((below, above)))
}

impl FrameJob {
    pub fn run(self) -> Result<LoadedFrame> {
        let (w, h) = (self.bundle.doc.canvas.width, self.bundle.doc.canvas.height);
        let paint = self
            .paint
            .par_iter()
            .map(|(layer, cel)| {
                load_paint(&self.bundle, layer, cel, &self.brushes).with_context(|| format!("cel {cel}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let vector = self
            .vector
            .par_iter()
            .map(|(cel, hash, nodes)| rasterize_nodes(nodes, w, h).map(|r| (cel.clone(), *hash, r)))
            .collect::<Result<Vec<_>>>()?;
        Ok(LoadedFrame { paint, vector })
    }
}

const MASK_TINT: [f32; 3] = [0.92, 0.16, 0.2];
const MASK_TINT_STRENGTH: f32 = 0.45;

fn tint(tile: &mut [f16], mask: &[f16]) {
    tile.as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(mask.as_chunks::<4>().0)
        .for_each(|(px, m)| {
            let a = m[3].to_f32() * MASK_TINT_STRENGTH;
            if a > 0.0 {
                (0..3).for_each(|c| px[c] = f16::from_f32(MASK_TINT[c] * a + px[c].to_f32() * (1.0 - a)));
                px[3] = f16::from_f32(a + px[3].to_f32() * (1.0 - a));
            }
        });
}

const MEMO_BYTES: usize = 256 << 20;

type Loaded = (String, Raster, CelData);
type Wanted<'a> = (Vec<&'a (String, String)>, Vec<(String, CelData)>);

fn load_paint(bundle: &Bundle, layer: &str, cel: &str, brushes: &BrushSet) -> Result<Loaded> {
    let load = CelCache::new(bundle, brushes, Write::WhenSlow).load(bundle, brushes, layer, cel, true)?;
    Ok((
        cel.to_string(),
        load.raster,
        CelData {
            replay: load.replay,
            file_hash: load.file_hash,
        },
    ))
}

fn paint_cels(doc: &Document, frame: u32) -> Vec<(String, String)> {
    doc.paint_content_at(frame)
        .iter()
        .map(|(l, c)| (l.id.clone(), c.id.clone()))
        .collect()
}

fn redraw(nodes: &[Node], before: Option<(&[Node], &Raster)>, (w, h): (u32, u32)) -> Result<Raster> {
    if let Some((old, raster)) = before
        && let Some(tiles) = klad_render::changed_tiles(old, nodes, w, h)?
    {
        let mut patched = raster.clone();
        klad_render::rasterize_tiles(nodes, w, h, &tiles)?
            .into_iter()
            .for_each(|(i, tile)| match tile {
                Some(tile) => patched.put(i, tile.into_boxed_slice()),
                None => patched.clear(i),
            });
        return Ok(patched);
    }
    rasterize_nodes(nodes, w, h)
}

fn nodes_hash(nodes: &[Node]) -> u64 {
    fnv(FNV_OFFSET, &serde_json::to_vec(nodes).unwrap_or_default())
}

fn vector_cels(doc: &Document, frame: u32) -> Vec<(String, u64)> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Vector)
        .filter_map(|l| l.content_at(frame))
        .map(|c| (c.id.clone(), nodes_hash(&c.nodes)))
        .collect()
}

fn vector_ids(doc: &Document, frame: u32) -> Vec<String> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Vector)
        .filter_map(|l| l.content_at(frame))
        .map(|c| c.id.clone())
        .collect()
}

fn cel_nodes<'a>(doc: &'a Document, cel: &str) -> Option<&'a [Node]> {
    match doc.item(cel)? {
        klad_core::doc::Item::Cel(_, c) => Some(&c.nodes),
        _ => None,
    }
}

fn drawings(doc: &Document) -> BTreeMap<String, (String, Kind)> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() != Kind::Group)
        .flat_map(|l| {
            l.cels()
                .iter()
                .filter(|c| c.link.is_none())
                .map(move |c| (c.id.clone(), (l.id.clone(), l.kind())))
        })
        .collect()
}

fn nodes_of(doc: &Document, cel: &str) -> Vec<Node> {
    match doc.locate(cel) {
        Some(Loc::Cel(path, c)) => doc.layer_at(&path).cels()[c].nodes.clone(),
        _ => Vec::new(),
    }
}

fn structure(doc: &Document, frame: u32) -> u64 {
    fn walk(layers: &[Layer], frame: u32, out: &mut Vec<u8>) {
        layers.iter().for_each(|l| {
            out.extend(l.id.as_bytes());
            out.extend(l.kind().name().as_bytes());
            let (opacity, moved) = l.keyed(frame);
            out.extend(opacity.to_le_bytes());
            out.extend(serde_json::to_vec(&moved).unwrap_or_default());
            out.push(l.blend as u8);
            out.push(l.visible as u8);
            out.push(l.clip as u8);
            out.extend(serde_json::to_vec(&l.effects).unwrap_or_default());
            out.extend(l.depth.unwrap_or(-1.0).to_le_bytes());
            out.push(l.reference as u8);
            if let Some(adjust) = l.adjustment() {
                out.extend(serde_json::to_vec(adjust).unwrap_or_default());
            }
            out.extend(l.content_at(frame).map_or(&b"-"[..], |c| c.id.as_bytes()));
            out.push(b'[');
            walk(l.mask.as_deref().map_or(&[][..], std::slice::from_ref), frame, out);
            out.push(b']');
            out.push(b'(');
            walk(l.children(), frame, out);
            out.push(b')');
        });
    }
    let mut bytes = Vec::new();
    walk(&doc.layers, frame, &mut bytes);
    bytes.extend(doc.canvas.background.as_deref().unwrap_or("").as_bytes());
    bytes.extend(serde_json::to_vec(&doc.atmosphere).unwrap_or_default());
    fnv(FNV_OFFSET, &bytes)
}

type Split<'a> = (Vec<&'a Layer>, Vec<&'a Layer>, bool);

fn alone(layers: &[Layer], i: usize) -> bool {
    let layer = &layers[i];
    !layer.clip && layer.mask.is_none() && layers.get(i + 1).is_none_or(|next| !next.clip)
}

fn split<'a>(doc: &Document, layers: &'a [Layer], path: &[usize]) -> Option<Split<'a>> {
    let (&i, rest) = path.split_first()?;
    let target = layers.get(i)?;
    let mut below: Vec<&Layer> = layers[..i].iter().collect();
    let mut above: Vec<&Layer> = layers[i + 1..].iter().collect();
    if rest.is_empty() {
        return Some((below, above, alone(layers, i)));
    }
    let through = target.visible
        && target.opacity >= 1.0
        && target.blend == Blend::Normal
        && doc.effects_of(target).is_empty()
        && !target.children().iter().any(Layer::reads_under)
        && alone(layers, i);
    let (inner_below, inner_above, inner) = split(doc, target.children(), rest)?;
    let joins = inner_below.first().is_some_and(|l| l.clip);
    below.extend(inner_below);
    above = inner_above.into_iter().chain(above).collect();
    Some((below, above, through && inner && !joins))
}

fn occupied(raster: Option<&Raster>) -> impl Iterator<Item = usize> + '_ {
    raster.into_iter().flat_map(|r| r.occupied().map(|(i, _)| i))
}

impl Scene {
    #[cfg(test)]
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_at(path, 0)
    }

    pub fn open_lazy(path: &Path) -> Result<Self> {
        let bundle = Bundle::open(path)?;
        let (w, h) = (bundle.doc.canvas.width, bundle.doc.canvas.height);
        let mut scene = Self {
            keep_after: klad_brush::cache::SLOW,
            lean: false,
            bundle,
            active: Vec::new(),
            frame: 0,
            rasters: Rasters::new(),
            cels: BTreeMap::new(),
            vector_hash: BTreeMap::new(),
            memo: std::collections::VecDeque::new(),
            below: Raster::new(w, h),
            above: Raster::new(w, h),
            fast: false,
            unsaved: Patch::default(),
            tint_mask: true,
            used: BTreeMap::new(),
            tick: 0,
            budget: CACHE_BYTES,
            stale: true,
            onion: Onion::default(),
            onion_on: false,
            onion_marked: BTreeSet::new(),
            onions: Onions::new(),
            onion_key: FNV_OFFSET,
            hashes: BTreeMap::new(),
        };
        scene.active = scene.default_active();
        Ok(scene)
    }

    pub fn open_at(path: &Path, frame: u32) -> Result<Self> {
        let bundle = Bundle::open(path)?;
        let brushes = &BrushSet::for_bundle(&bundle)?;
        let (w, h) = (bundle.doc.canvas.width, bundle.doc.canvas.height);
        let frame = frame.min(bundle.doc.timeline.frames - 1);
        let paint = paint_cels(&bundle.doc, frame)
            .par_iter()
            .map(|(layer, cel)| load_paint(&bundle, layer, cel, brushes).with_context(|| format!("cel {cel}")))
            .collect::<Result<Vec<_>>>()?;
        let vectors = vector_cels(&bundle.doc, frame);
        let rasterized = vectors
            .par_iter()
            .map(|(cel, _)| rasterize_nodes(&nodes_of(&bundle.doc, cel), w, h).map(|r| (cel.clone(), r)))
            .collect::<Result<Vec<_>>>()?;
        let mut rasters = Rasters::new();
        let mut cels = BTreeMap::new();
        paint.into_iter().for_each(|(id, raster, data)| {
            rasters.insert(id.clone(), raster);
            cels.insert(id, data);
        });
        rasters.extend(rasterized);
        let mut scene = Self {
            keep_after: klad_brush::cache::SLOW,
            lean: false,
            bundle,
            active: Vec::new(),
            frame,
            rasters,
            cels,
            vector_hash: vectors.into_iter().collect(),
            memo: std::collections::VecDeque::new(),
            below: Raster::new(w, h),
            above: Raster::new(w, h),
            fast: false,
            unsaved: Patch::default(),
            tint_mask: true,
            used: BTreeMap::new(),
            tick: 0,
            budget: CACHE_BYTES,
            stale: false,
            onion: Onion::default(),
            onion_on: false,
            onion_marked: BTreeSet::new(),
            onions: Onions::new(),
            onion_key: FNV_OFFSET,
            hashes: BTreeMap::new(),
        };
        scene.active = scene.default_active();
        scene.trim();
        scene.rebuild_caches(None)?;
        Ok(scene)
    }

    fn default_active(&self) -> Vec<usize> {
        self.bundle
            .doc
            .all_layers()
            .into_iter()
            .rev()
            .filter(|l| l.kind() == Kind::Paint)
            .find_map(|l| match self.bundle.doc.locate(&l.id) {
                Some(Loc::Layer(path)) if path.last() != Some(&MASK) => Some(path),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub fn active_is_mask(&self) -> bool {
        self.active.last() == Some(&MASK)
    }

    pub fn active_locks_alpha(&self) -> bool {
        self.active_layer().is_some_and(|l| l.lock_alpha)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.bundle.doc.canvas.width, self.bundle.doc.canvas.height)
    }

    pub fn cols(&self) -> usize {
        self.below.cols
    }

    pub fn all_tiles(&self) -> Vec<usize> {
        (0..self.below.cols * self.below.rows).collect()
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.bundle.doc.locate_path(&self.active)
    }

    pub fn active_id(&self) -> Option<String> {
        self.active_layer().map(|l| l.id.clone())
    }

    pub fn can_paint(&self) -> bool {
        self.active_layer()
            .is_some_and(|l| l.kind() == Kind::Paint && !l.locked)
    }

    pub fn active_cel(&self) -> Option<String> {
        self.active_layer()
            .filter(|l| l.kind() == Kind::Paint)
            .and_then(|l| l.content_at(self.frame))
            .map(|c| c.id.clone())
    }

    pub fn active_marks(&self) -> &[Mark] {
        self.active_cel()
            .and_then(|c| self.cels.get(&c))
            .map_or(&[], |d| d.marks())
    }

    pub fn marks_of(&self, cel: &str) -> &[Mark] {
        self.cels.get(cel).map_or(&[], |d| d.marks())
    }

    pub fn active_raster(&self) -> Option<&Raster> {
        self.rasters.get(&self.active_cel()?)
    }

    fn seen_at(&self, path: &[usize], at: (f32, f32)) -> Option<f32> {
        let doc = &self.bundle.doc;
        let alpha_of = |layer: &Layer| -> Option<f32> {
            let cel = layer.content_at(self.frame)?;
            self.rasters.get(&cel.id).map(|r| r.alpha_at(at.0, at.1))
        };
        let layer = doc.locate_path(path)?;
        let own = alpha_of(layer)?;
        let opacity: f32 = (1..=path.len())
            .map(|n| doc.locate_path(&path[..n]).map_or(1.0, |l| l.opacity))
            .product();
        let unmasked = layer
            .mask
            .as_deref()
            .filter(|m| m.visible)
            .and_then(alpha_of)
            .map_or(1.0, |m| 1.0 - m);
        let based = match (layer.clip, path.split_last()) {
            (true, Some((&i, parent))) => {
                let list = match parent.is_empty() {
                    true => &doc.layers[..],
                    false => doc.locate_path(parent).map_or(&[][..], |p| p.children()),
                };
                list[..i]
                    .iter()
                    .rev()
                    .find(|l| !l.clip)
                    .map_or(Some(1.0), alpha_of)
                    .unwrap_or(1.0)
            }
            _ => 1.0,
        };
        Some(own * opacity * unmasked * based)
    }

    pub fn marks_at(
        &self,
        at: (f32, f32),
        slack: f32,
        all: bool,
        hidden: impl Fn(&[Mark], &Mark) -> bool + Copy,
    ) -> Option<(Vec<usize>, u32)> {
        const SEEN: f32 = 0.05;
        let doc = &self.bundle.doc;
        let shown = |path: &[usize]| (1..=path.len()).all(|n| doc.locate_path(&path[..n]).is_some_and(|l| l.visible));
        let paths: Vec<Vec<usize>> = match all {
            false => vec![self.active.clone()],
            true => doc
                .all_layers()
                .into_iter()
                .rev()
                .filter_map(|l| match doc.locate(&l.id) {
                    Some(Loc::Layer(path)) => Some(path),
                    _ => None,
                })
                .collect(),
        };
        let layers: Vec<(Vec<usize>, &[Mark], Option<f32>)> = paths
            .into_iter()
            .filter(|path| path.last() != Some(&MASK) && shown(path))
            .filter_map(|path| {
                let layer = doc.locate_path(&path)?;
                if layer.kind() != Kind::Paint || layer.locked {
                    return None;
                }
                let marks = self.cels.get(&layer.content_at(self.frame)?.id)?.marks();
                let seen = self.seen_at(&path, at);
                Some((path, marks, seen))
            })
            .collect();
        let pick = |marks: &[Mark], slack: f32| klad_core::markedit::pick(marks, at, slack, |m| hidden(marks, m));
        layers
            .iter()
            .find(|(_, marks, seen)| seen.map_or_else(|| pick(marks, 0.0).is_some(), |a| a > SEEN))
            .and_then(|(path, marks, _)| pick(marks, slack).map(|id| (path.clone(), id)))
            .or_else(|| {
                layers
                    .iter()
                    .find_map(|(path, marks, _)| pick(marks, slack).map(|id| (path.clone(), id)))
            })
    }

    pub fn total_marks(&self) -> usize {
        self.cels.values().map(|c| c.marks().len()).sum()
    }

    pub fn raster_of(&self, cel: &str) -> Option<&Raster> {
        self.rasters.get(cel)
    }

    #[cfg(test)]
    pub fn set_active(&mut self, path: Vec<usize>) -> Result<()> {
        self.active = path;
        self.rebuild_caches(None)
    }

    pub fn set_active_later(&mut self, path: Vec<usize>) {
        if path != self.active {
            self.active = path;
            self.stale = true;
            self.trim();
        }
    }

    pub fn canvas_request(&self, brushes: &BrushSet) -> CanvasRequest {
        let frame = self.frame;
        let plan = self.onion_plan(frame);
        let (paint, vector) = self.needs(frame, self.onion_on.then_some(&plan));
        let doc = &self.bundle.doc;
        let rasters: Rasters = paint
            .iter()
            .map(|(_, c)| c)
            .filter(|c| self.has_pixels(c))
            .chain(vector.iter().filter(|c| self.vector_loaded(c)))
            .filter_map(|c| self.rasters.get(c).map(|r| (c.clone(), r.clone())))
            .collect();
        CanvasRequest {
            frame,
            active: self.active.clone(),
            rev: doc.rev,
            bundle: Bundle {
                root: self.bundle.root.clone(),
                seen: self.bundle.seen,
                doc: doc.clone(),
            },
            rasters,
            paint: paint.into_iter().filter(|(_, c)| !self.has_pixels(c)).collect(),
            vector: vector
                .into_iter()
                .filter(|c| !self.vector_loaded(c))
                .map(|c| {
                    let nodes = nodes_of(doc, &c);
                    (c, nodes_hash(&nodes), nodes)
                })
                .collect(),
            plan,
            onion: self.onion_on,
            lean: self.lean,
            keep: self.active_cel(),
            brushes: brushes.clone(),
            planes: self.planes_ask(frame),
            picture: None,
        }
    }

    pub fn adopt(&mut self, done: CanvasDone, wanted: bool) -> bool {
        if done.rev != self.bundle.doc.rev {
            return false;
        }
        self.take_loaded(done.paint, done.vector);
        done.strokes.into_iter().for_each(|(id, data)| {
            if self.cels.get(&id).is_none_or(|d| d.file_hash != data.file_hash) {
                self.cels.insert(id, data);
            }
        });
        let fits = wanted && done.frame == self.frame && done.active == self.active;
        if fits {
            self.onions = done.onions;
            self.onion_key = onion::plan_hash(&self.onion_plan(self.frame), |cel| self.loaded_version(cel));
            let (w, h) = self.size();
            (self.below, self.above, self.fast) = match done.caches {
                Some((below, above)) => (below, above, true),
                None => (Raster::new(w, h), Raster::new(w, h), false),
            };
            self.stale = false;
        }
        self.trim();
        self.slim();
        fits
    }

    fn has_pixels(&self, cel: &str) -> bool {
        self.cels.contains_key(cel) && self.rasters.contains_key(cel)
    }

    fn slimmed(&self) -> bool {
        self.lean
            && paint_cels(&self.bundle.doc, self.frame)
                .iter()
                .any(|(_, c)| !self.rasters.contains_key(c))
    }

    fn slim(&mut self) {
        if !(self.lean && self.fast && !self.stale) {
            return;
        }
        let active = self.active_cel();
        let garbage: Vec<Raster> = paint_cels(&self.bundle.doc, self.frame)
            .into_iter()
            .filter(|(_, c)| Some(c) != active.as_ref())
            .filter_map(|(_, c)| self.rasters.remove(&c))
            .collect();
        if !garbage.is_empty() {
            std::thread::spawn(move || drop(garbage));
        }
    }

    fn onion_map(&self) -> Option<&Onions> {
        (!self.onions.is_empty()).then_some(&self.onions)
    }

    pub fn rebuild_caches(&mut self, tiles: Option<&[usize]>) -> Result<()> {
        if self.slimmed() {
            self.stale = true;
            return Ok(());
        }
        let (w, h) = self.size();
        let full = tiles.is_none() || !self.fast || self.stale;
        let wanted: Vec<usize> = match tiles {
            Some(t) if !full => dilate(
                t,
                effect_reach(&self.bundle.doc, &self.bundle.doc.layers),
                self.cols(),
                self.rows(),
            ),
            _ => self.all_tiles(),
        };
        let fresh = cache_tiles(
            &self.bundle.doc,
            self.frame,
            &self.active,
            &self.rasters,
            &self.onions,
            &wanted,
        )?;
        let Some(fresh) = fresh else {
            self.fast = false;
            self.stale = false;
            self.below = Raster::new(w, h);
            self.above = Raster::new(w, h);
            return Ok(());
        };
        if full {
            self.below = Raster::new(w, h);
            self.above = Raster::new(w, h);
        }
        fresh.into_iter().for_each(|(i, b, a)| {
            match b {
                Some(b) => self.below.put(i, b.into_boxed_slice()),
                None => self.below.clear(i),
            }
            match a {
                Some(a) => self.above.put(i, a.into_boxed_slice()),
                None => self.above.clear(i),
            }
        });
        self.fast = true;
        self.stale = false;
        Ok(())
    }

    pub fn compose(&self, tiles: &[usize], preview: Option<&Preview>) -> Result<Vec<(usize, Vec<f16>)>> {
        self.compose_painting(&[], tiles, preview)
    }

    fn active_reach(&self) -> i32 {
        (1..=self.active.len())
            .filter_map(|n| self.bundle.doc.locate_path(&self.active[..n]))
            .map(|l| klad_render::effects::margin(&self.bundle.doc.effects_of(l), 1.0))
            .sum()
    }

    pub fn compose_painting(
        &self,
        stroke: &[usize],
        other: &[usize],
        preview: Option<&Preview>,
    ) -> Result<Vec<(usize, Vec<f16>)>> {
        let (cols, rows) = (self.cols(), self.rows());
        let doc = &self.bundle.doc;
        let near = self.active_reach();
        let tiles: Vec<usize> = dilate(stroke, near, cols, rows)
            .into_iter()
            .chain(dilate(other, effect_reach(doc, &doc.layers), cols, rows))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let cel = self.active_cel();
        let owns: SwapTiles = match (preview, cel.as_deref()) {
            (Some(p), Some(c)) => dilate(&tiles, near, cols, rows)
                .into_par_iter()
                .filter(|i| p.buffer.coverage(*i).is_some() || p.twin.is_some_and(|t| t.coverage(*i).is_some()))
                .map(|i| {
                    let mut tile = self
                        .rasters
                        .get(c)
                        .and_then(|r| r.at(i))
                        .map_or_else(|| vec![f16::ZERO; TILE_PX * 4], <[f16]>::to_vec);
                    klad_brush::land_tile(&mut tile, p.buffer, i, p.color, p.opacity, p.blend);
                    if let Some(twin) = p.twin.filter(|t| t.coverage(i).is_some()) {
                        klad_brush::land_tile(&mut tile, twin, i, p.color, p.opacity, p.blend);
                    }
                    (i, Some(tile))
                })
                .collect(),
            _ => SwapTiles::new(),
        };
        let split = self.fast.then(|| fast_split(doc, &self.active)).flatten();
        let live: Option<Vec<&Layer>> = split
            .filter(|(_, above, _)| live_above(above))
            .map(|(_, above, _)| above);
        tiles
            .par_iter()
            .map(|&i| Ok((i, self.compose_one(i, &owns, live.as_deref())?)))
            .collect()
    }

    pub fn planes_job(&self) -> impl FnOnce() -> Result<Vec<(f32, egui::ColorImage)>> + Send + 'static {
        let doc = self.bundle.doc.clone();
        let rasters = self.rasters.clone();
        let frame = self.frame;
        move || {
            let level = (doc.canvas.width.max(doc.canvas.height) as f32 / 1536.0)
                .log2()
                .ceil()
                .max(0.0) as u32;
            let background = doc.canvas.background.clone();
            doc.layers
                .iter()
                .filter(|l| l.visible)
                .enumerate()
                .map(|(i, layer)| {
                    let back = if i == 0 { background.as_deref() } else { None };
                    let flat = klad_render::flatten_with(&doc, frame, &rasters, &[layer], back, None, true)?;
                    let (w, h, rgba) = image_at(&flat, level);
                    Ok((
                        layer.depth.unwrap_or(0.0),
                        egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &rgba),
                    ))
                })
                .collect()
        }
    }

    pub fn preview_layer(&mut self, layer: Layer) -> Result<()> {
        let Some(Loc::Layer(path)) = self.bundle.doc.locate(&layer.id) else {
            return Err(crate::report::bug(format!("layer {} is not in the scene", layer.id)));
        };
        let before: BTreeMap<String, Vec<Node>> = self
            .bundle
            .doc
            .layer_at(&path)
            .cels()
            .iter()
            .map(|c| (c.id.clone(), c.nodes.clone()))
            .collect();
        let size = (self.bundle.doc.canvas.width, self.bundle.doc.canvas.height);
        let changed: Vec<(String, Vec<Node>)> = layer
            .cels()
            .iter()
            .filter(|c| self.rasters.contains_key(&c.id))
            .filter(|c| {
                before
                    .get(&c.id)
                    .is_some_and(|old| nodes_hash(old) != nodes_hash(&c.nodes))
            })
            .map(|c| (c.id.clone(), c.nodes.clone()))
            .collect();
        *self.bundle.doc.layer_at_mut(&path) = layer;
        let mut dirty = BTreeSet::new();
        changed.into_iter().try_for_each(|(cel, nodes)| {
            let old = before.get(&cel).map(Vec::as_slice).unwrap_or_default();
            let drawn = self
                .rasters
                .get(&cel)
                .filter(|_| self.vector_hash.get(&cel) == Some(&nodes_hash(old)));
            let raster = redraw(&nodes, drawn.map(|r| (old, r)), size)?;
            self.swap_vector(cel.clone(), raster, &mut dirty);
            self.vector_hash.insert(cel, nodes_hash(&nodes));
            Ok::<_, anyhow::Error>(())
        })?;
        self.rebuild_caches(None)
    }

    pub fn rows(&self) -> usize {
        (self.bundle.doc.canvas.height as usize).div_ceil(TILE)
    }

    fn compose_one(&self, index: usize, owns: &SwapTiles, live: Option<&[&Layer]>) -> Result<Vec<f16>> {
        let (tx, ty) = (index % self.cols(), index / self.cols());
        let doc = &self.bundle.doc;
        let cel = self.active_cel();
        let own: Option<&[f16]> = match owns.get(&index) {
            Some(tile) => tile.as_deref(),
            None => cel.as_ref().and_then(|c| self.rasters.get(c)).and_then(|r| r.at(index)),
        };
        if self.fast
            && let Some(active) = self.active_layer()
        {
            let flat = |tile| TileItem::Pixels {
                tile,
                opacity: 1.0,
                blend: Blend::Normal,
            };
            let own_item = own.filter(|_| active.visible).map(|tile| TileItem::Pixels {
                tile,
                opacity: active.opacity,
                blend: active.blend,
            });
            let upper: Vec<TileItem> = match live {
                Some(layers) => layers.iter().map(|l| TileItem::Layer(l)).collect(),
                None => self.above.at(index).map(flat).into_iter().collect(),
            };
            let items: Vec<TileItem> = self
                .below
                .at(index)
                .map(flat)
                .into_iter()
                .chain(own_item)
                .chain(upper)
                .collect();
            let job = TileJob {
                frame: self.frame,
                rasters: &self.rasters,
                background: None,
                tx,
                ty,
                swap: None,
                onions: None,
                doc,
                references: true,
            };
            return compose_tile(&items, &job);
        }
        let items: Vec<TileItem> = doc.layers.iter().map(TileItem::Layer).collect();
        let swap = cel.as_deref().filter(|_| !owns.is_empty()).map(|c| Swap {
            cel: c,
            cols: self.cols(),
            rows: self.rows(),
            tiles: owns,
        });
        let job = TileJob {
            frame: self.frame,
            rasters: &self.rasters,
            background: doc.canvas.background.as_deref(),
            tx,
            ty,
            swap,
            onions: self.onion_map(),
            doc,
            references: true,
        };
        let mut tile = compose_tile(&items, &job)?;
        if self.tint_mask
            && self.active_is_mask()
            && let Some(mask) = own
        {
            tint(&mut tile, mask);
        }
        Ok(tile)
    }

    fn ensure_active_cel(&mut self) -> Result<String> {
        if let Some(cel) = self.active_cel() {
            return Ok(cel);
        }
        let layer = self.active_id().ok_or_else(|| anyhow!("pick a layer first"))?;
        let op = Op::AddCel {
            cel: serde_json::json!({ "frame": self.frame }),
            layer,
        };
        let (doc, applied) = apply_all(&self.bundle.doc, &[op])?;
        let mut restore = diff(&doc, &self.bundle.doc)?;
        restore.0.extend(std::mem::take(&mut self.unsaved).0);
        self.unsaved = restore;
        self.bundle.doc = doc;
        let cel = applied
            .added
            .first()
            .cloned()
            .ok_or_else(|| crate::report::bug("a new drawing was not added"))?;
        let (w, h) = self.size();
        self.rasters.insert(cel.clone(), Raster::new(w, h));
        self.cels.insert(
            cel.clone(),
            CelData {
                replay: CelReplay::new(w, h),
                file_hash: fnv(FNV_OFFSET, &[]),
            },
        );
        Ok(cel)
    }

    pub fn next_mark_id(&self) -> u32 {
        let cel = self.active_cel().unwrap_or_default();
        self.bundle.doc.mark_ids(&cel).next(self.active_marks())
    }

    pub fn commit_stroke(
        &mut self,
        stroke: Stroke,
        buffer: &mut StrokeBuffer,
        blend: StrokeBlend,
    ) -> Result<Vec<usize>> {
        let cel = self.ensure_active_cel()?;
        let raster = self
            .rasters
            .get_mut(&cel)
            .ok_or_else(|| crate::report::bug(format!("drawing {cel} has no pixels")))?;
        let data = self
            .cels
            .get_mut(&cel)
            .ok_or_else(|| crate::report::bug(format!("drawing {cel} has no strokes")))?;
        Ok(data.replay.commit_stroke(raster, stroke, buffer, blend))
    }

    pub fn commit_fill(&mut self, fill: Fill, brushes: &BrushSet) -> Result<Vec<usize>> {
        let cel = self.ensure_active_cel()?;
        let raster = self
            .rasters
            .get_mut(&cel)
            .ok_or_else(|| crate::report::bug(format!("drawing {cel} has no pixels")))?;
        let data = self
            .cels
            .get_mut(&cel)
            .ok_or_else(|| crate::report::bug(format!("drawing {cel} has no strokes")))?;
        data.replay.paint(raster, Mark::Fill(fill), brushes)
    }

    pub fn fill_reference(&self, reference: &FillRef, brushes: &BrushSet) -> Result<Raster> {
        let cel = self.active_cel().unwrap_or_default();
        if *reference == FillRef::This || !self.slimmed() {
            return fill_reference(&self.bundle.doc, self.frame, &self.rasters, &cel, reference);
        }
        let missing: Vec<(String, String)> = paint_cels(&self.bundle.doc, self.frame)
            .into_iter()
            .filter(|(_, c)| !self.rasters.contains_key(c))
            .collect();
        let loaded = missing
            .par_iter()
            .map(|(layer, cel)| load_paint(&self.bundle, layer, cel, brushes))
            .collect::<Result<Vec<_>>>()?;
        let mut rasters = self.rasters.clone();
        loaded.into_iter().for_each(|(id, raster, _)| {
            rasters.insert(id, raster);
        });
        fill_reference(&self.bundle.doc, self.frame, &rasters, &cel, reference)
    }

    pub fn sample(&self, (x, y): (f32, f32), layer_only: bool) -> Result<Option<[f32; 4]>> {
        let (w, h) = self.size();
        if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
            return Ok(None);
        }
        let (x, y) = (x as usize, y as usize);
        let index = (y / TILE) * self.cols() + x / TILE;
        let at = ((y % TILE) * TILE + x % TILE) * 4;
        let px: Option<[f32; 4]> = if layer_only {
            self.active_cel()
                .and_then(|c| self.rasters.get(&c))
                .and_then(|r| r.at(index))
                .map(|t| [t[at], t[at + 1], t[at + 2], t[at + 3]].map(f16::to_f32))
        } else if self.slimmed() {
            let composed = self.compose(&[index], None)?;
            let tile = composed
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, t)| t)
                .ok_or_else(|| crate::report::bug(format!("tile {index} was not composed")))?;
            Some([tile[at], tile[at + 1], tile[at + 2], tile[at + 3]].map(f16::to_f32))
        } else {
            let items: Vec<TileItem> = self.bundle.doc.layers.iter().map(TileItem::Layer).collect();
            let tile = self.compose_with(&items, index, self.bundle.doc.canvas.background.as_deref(), false)?;
            Some([tile[at], tile[at + 1], tile[at + 2], tile[at + 3]].map(f16::to_f32))
        };
        Ok(px.filter(|p| p[3] > 0.0).map(|[r, g, b, a]| {
            let un = |c: f32| (c / a).clamp(0.0, 1.0);
            [un(r), un(g), un(b), a.clamp(0.0, 1.0)]
        }))
    }

    pub fn step(
        &mut self,
        stack: klad_core::history::Stack,
        brushes: &BrushSet,
    ) -> Result<Option<(String, Vec<usize>)>> {
        let touched = klad_core::history::peek(&self.bundle, stack)?
            .and_then(|(_, patch)| patch)
            .and_then(|patch| patch.touched_cels(&self.bundle.doc));
        let old = match touched {
            Some(_) => None,
            None => Some(self.bundle.doc.clone()),
        };
        let Some(entry) = klad_core::history::step(&mut self.bundle, stack)? else {
            return Ok(None);
        };
        self.bundle.stamp("editor")?;
        let dirty = match (old, touched) {
            (Some(old), _) => self.sync(&old, brushes)?,
            (None, Some(cels)) => self.sync_nodes(&cels, brushes)?,
            (None, None) => unreachable!("one of the two is set"),
        };
        Ok(Some((entry.summary, dirty)))
    }

    pub fn onion_layers(&self) -> Vec<String> {
        if !self.onion_on {
            return Vec::new();
        }
        let active = self
            .active_layer()
            .filter(|l| matches!(l.kind(), Kind::Paint | Kind::Vector) && !self.active_is_mask())
            .map(|l| l.id.clone());
        active
            .into_iter()
            .chain(self.onion_marked.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn onion_plan(&self, frame: u32) -> Plan {
        onion::plan(&self.bundle.doc, frame, &self.onion_layers(), &self.onion)
    }

    fn needs(&self, frame: u32, plan: Option<&Plan>) -> (Vec<(String, String)>, Vec<String>) {
        let doc = &self.bundle.doc;
        let ghosts = plan.map_or(Vec::new(), |p| onion::needed(doc, p));
        let paint: BTreeSet<(String, String)> = paint_cels(doc, frame)
            .into_iter()
            .chain(
                ghosts
                    .iter()
                    .filter(|(_, _, k)| *k == Kind::Paint)
                    .map(|(l, c, _)| (l.clone(), c.clone())),
            )
            .collect();
        let vector: BTreeSet<String> = vector_ids(doc, frame)
            .into_iter()
            .chain(
                ghosts
                    .iter()
                    .filter(|(_, _, k)| *k == Kind::Vector)
                    .map(|(_, c, _)| c.clone()),
            )
            .collect();
        (paint.into_iter().collect(), vector.into_iter().collect())
    }

    fn vector_loaded(&self, cel: &str) -> bool {
        self.vector_hash.contains_key(cel) && self.rasters.contains_key(cel)
    }

    pub fn has_frame(&self, frame: u32) -> bool {
        let plan = self.onion_on.then(|| self.onion_plan(frame));
        let (paint, vector) = self.needs(frame, plan.as_ref());
        paint.iter().all(|(_, c)| self.has_pixels(c)) && vector.iter().all(|c| self.vector_loaded(c))
    }

    pub fn frame_job(&self, frame: u32, brushes: &BrushSet) -> Option<FrameJob> {
        let doc = &self.bundle.doc;
        let frame = frame.min(doc.timeline.frames - 1);
        if self.has_frame(frame) {
            return None;
        }
        let plan = self.onion_on.then(|| self.onion_plan(frame));
        let (paint, vector) = self.needs(frame, plan.as_ref());
        Some(FrameJob {
            bundle: Bundle {
                root: self.bundle.root.clone(),
                seen: self.bundle.seen,
                doc: doc.clone(),
            },
            paint: paint.into_iter().filter(|(_, c)| !self.cels.contains_key(c)).collect(),
            vector: vector
                .into_iter()
                .filter(|c| !self.vector_loaded(c))
                .map(|c| {
                    let nodes = nodes_of(doc, &c);
                    (c, nodes_hash(&nodes), nodes)
                })
                .collect(),
            brushes: brushes.clone(),
        })
    }

    fn take_loaded(&mut self, paint: Vec<Loaded>, vector: Vec<(String, u64, Raster)>) {
        paint.into_iter().for_each(|(id, raster, data)| {
            self.rasters.entry(id.clone()).or_insert(raster);
            if self.cels.get(&id).is_none_or(|d| d.file_hash != data.file_hash) {
                self.cels.insert(id, data);
            }
        });
        vector.into_iter().for_each(|(cel, hash, raster)| {
            if !self.vector_loaded(&cel) {
                self.rasters.insert(cel.clone(), raster);
                self.vector_hash.insert(cel, hash);
            }
        });
    }

    #[cfg(test)]
    pub fn set_frame(&mut self, frame: u32, brushes: &BrushSet) -> Result<Vec<usize>> {
        self.move_to(frame);
        self.refresh(brushes)?;
        Ok(self.all_tiles())
    }

    pub fn move_to(&mut self, frame: u32) {
        let frame = frame.min(self.bundle.doc.timeline.frames - 1);
        if frame != self.frame {
            self.frame = frame;
            self.stale = true;
        }
        self.trim();
    }

    fn load_missing(&mut self, brushes: &BrushSet) -> Result<()> {
        if let Some(job) = self.frame_job(self.frame, brushes) {
            let loaded = job.run()?;
            self.take_loaded(loaded.paint, loaded.vector);
        }
        Ok(())
    }

    pub fn refresh(&mut self, brushes: &BrushSet) -> Result<()> {
        self.load_missing(brushes)?;
        self.build_onions();
        self.rebuild_caches(None)
    }

    fn build_onions(&mut self) -> Vec<usize> {
        let plan = self.onion_plan(self.frame);
        let key = onion::plan_hash(&plan, |cel| self.loaded_version(cel));
        if key == self.onion_key {
            return Vec::new();
        }
        let fresh = onion::build(&self.bundle.doc, &plan, &self.rasters);
        let dirty: BTreeSet<usize> = self
            .onions
            .values()
            .chain(fresh.values())
            .flat_map(|r| r.occupied().map(|(i, _)| i))
            .collect();
        self.onions = fresh;
        self.onion_key = key;
        dirty.into_iter().collect()
    }

    #[cfg(test)]
    pub fn refresh_onions(&mut self, brushes: &BrushSet) -> Result<Vec<usize>> {
        self.load_missing(brushes)?;
        let dirty = self.build_onions();
        if !dirty.is_empty() {
            self.rebuild_caches(Some(&dirty))?;
        }
        Ok(dirty)
    }

    fn loaded_version(&self, cel: &str) -> u64 {
        match self.cels.get(cel) {
            Some(data) => data.file_hash,
            None => self.vector_hash.get(cel).copied().unwrap_or(0),
        }
    }

    fn paint_version(&mut self, layer: &str, cel: &str) -> u64 {
        if let Some(data) = self.cels.get(cel) {
            return data.file_hash;
        }
        let path = self.bundle.strokes_path(layer, cel);
        let now = stamp(&path);
        if let (Some((was, hash)), Some(now)) = (self.hashes.get(cel), now)
            && *was == now
        {
            return *hash;
        }
        let bytes = fs::read(&path).unwrap_or_default();
        let hash = fnv(FNV_OFFSET, &bytes);
        if let Some(now) = now {
            self.hashes.insert(cel.to_string(), (now, hash));
        }
        hash
    }

    fn vector_version(&self, cel: &str) -> u64 {
        match self.vector_loaded(cel) {
            true => self.vector_hash[cel],
            false => nodes_hash(&nodes_of(&self.bundle.doc, cel)),
        }
    }

    pub fn frame_key(&mut self, frame: u32, onion: bool, level: u32, fingerprint: u64) -> u64 {
        let plan = onion.then(|| self.onion_plan(frame));
        let (paint, vector) = self.needs(frame, plan.as_ref());
        let mut versions: BTreeMap<String, u64> = paint
            .iter()
            .map(|(layer, cel)| (cel.clone(), self.paint_version(layer, cel)))
            .collect();
        versions.extend(vector.iter().map(|cel| (cel.clone(), self.vector_version(cel))));
        let mut hash = fnv(FNV_OFFSET, &structure(&self.bundle.doc, frame).to_le_bytes());
        hash = versions
            .iter()
            .fold(hash, |h, (cel, v)| fnv(fnv(h, cel.as_bytes()), &v.to_le_bytes()));
        if let Some(plan) = &plan {
            let ghosts = onion::plan_hash(plan, |cel| versions.get(cel).copied().unwrap_or(0));
            hash = fnv(fnv(hash, b"onion"), &ghosts.to_le_bytes());
        }
        fnv(
            hash,
            &[level as u64, fingerprint, klad_render::RENDER_VERSION as u64]
                .map(u64::to_le_bytes)
                .concat(),
        )
    }

    pub fn frame_request(&mut self, frame: u32, onion: bool, level: u32, brushes: &BrushSet) -> FrameRequest {
        let key = self.frame_key(frame, onion, level, brushes.fingerprint());
        let plan = onion.then(|| self.onion_plan(frame));
        let (paint, vector) = self.needs(frame, plan.as_ref());
        let doc = &self.bundle.doc;
        let rasters: Rasters = paint
            .iter()
            .map(|(_, c)| c)
            .filter(|c| self.cels.contains_key(*c))
            .chain(vector.iter().filter(|c| self.vector_loaded(c)))
            .filter_map(|c| self.rasters.get(c).map(|r| (c.clone(), r.clone())))
            .collect();
        FrameRequest {
            frame,
            key,
            level,
            rev: doc.rev,
            bundle: Bundle {
                root: self.bundle.root.clone(),
                seen: self.bundle.seen,
                doc: doc.clone(),
            },
            rasters,
            paint: paint.into_iter().filter(|(_, c)| !self.cels.contains_key(c)).collect(),
            vector: vector
                .into_iter()
                .filter(|c| !self.vector_loaded(c))
                .map(|c| {
                    let nodes = nodes_of(doc, &c);
                    (c, nodes_hash(&nodes), nodes)
                })
                .collect(),
            plan,
            brushes: brushes.clone(),
            disk: Some(self.pictures()),
            keep_after: self.keep_after,
            planes: None,
        }
    }

    pub fn prepare_request(&mut self, frame: u32, onion: bool, level: u32, brushes: &BrushSet) -> FrameRequest {
        let key = self.frame_key(frame, onion, level, brushes.fingerprint());
        let plan = onion.then(|| self.onion_plan(frame));
        let (paint, vector) = self.needs(frame, plan.as_ref());
        let doc = &self.bundle.doc;
        FrameRequest {
            frame,
            key,
            level,
            rev: doc.rev,
            bundle: Bundle {
                root: self.bundle.root.clone(),
                seen: self.bundle.seen,
                doc: doc.clone(),
            },
            rasters: Rasters::new(),
            paint,
            vector: vector
                .into_iter()
                .map(|c| {
                    let nodes = nodes_of(doc, &c);
                    (c, nodes_hash(&nodes), nodes)
                })
                .collect(),
            plan,
            brushes: brushes.clone(),
            disk: Some(self.pictures()),
            keep_after: std::time::Duration::ZERO,
            planes: self.planes_ask(frame),
        }
    }

    pub fn planes_ask(&self, frame: u32) -> Option<PlanesAsk> {
        let doc = &self.bundle.doc;
        fast_split(doc, &self.active)?;
        let plan = self.onion_on.then(|| self.onion_plan(frame));
        let ghosts = plan
            .iter()
            .flatten()
            .flat_map(|(layer, ghosts)| ghosts.iter().map(move |g| (layer.clone(), g.cel.clone())));
        let cels: Vec<(String, String)> = paint_cels(doc, frame)
            .into_iter()
            .chain(ghosts)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let path: Vec<u8> = self.active.iter().flat_map(|i| (*i as u64).to_le_bytes()).collect();
        let base = vector_cels(doc, frame).iter().fold(
            fnv(fnv(FNV_OFFSET, &structure(doc, frame).to_le_bytes()), &path),
            |h, (cel, version)| fnv(fnv(h, cel.as_bytes()), &version.to_le_bytes()),
        );
        let base = match &plan {
            Some(plan) => fnv(fnv(base, b"onion"), &onion::plan_hash(plan, |_| 0).to_le_bytes()),
            None => base,
        };
        Some(PlanesAsk {
            base,
            cels,
            dir: self.bundle.cache_dir().join("planes"),
            active: self.active.clone(),
        })
    }

    pub fn holding(&self) -> BTreeSet<String> {
        self.cels
            .keys()
            .filter(|c| self.rasters.contains_key(*c))
            .cloned()
            .collect()
    }

    pub fn pictures(&self) -> std::path::PathBuf {
        self.bundle.cache_dir().join("frames")
    }

    pub fn absorb(&mut self, done: FrameDone) -> FrameImage {
        if done.rev == self.bundle.doc.rev {
            self.take_loaded(done.paint, done.vector);
            self.trim();
            self.slim();
        }
        done.image
    }

    fn current(&self) -> BTreeSet<String> {
        let doc = &self.bundle.doc;
        paint_cels(doc, self.frame)
            .into_iter()
            .map(|(_, c)| c)
            .chain(vector_ids(doc, self.frame))
            .collect()
    }

    fn evict(&mut self, cel: &str) -> (Option<Raster>, Option<CelData>) {
        let raster = match (self.rasters.remove(cel), self.vector_hash.remove(cel)) {
            (Some(raster), Some(hash)) => {
                self.retire(cel, hash, raster);
                None
            }
            (raster, _) => raster,
        };
        self.used.remove(cel);
        (raster, self.cels.remove(cel))
    }

    fn trim(&mut self) {
        let current = self.current();
        let ghosts: BTreeSet<String> = onion::needed(&self.bundle.doc, &self.onion_plan(self.frame))
            .into_iter()
            .map(|(_, c, _)| c)
            .collect();
        self.tick += 1;
        let tick = self.tick;
        current.iter().chain(&ghosts).for_each(|c| {
            self.used.insert(c.clone(), tick);
        });
        let active = self.active_cel();
        self.cels
            .iter_mut()
            .filter(|(id, _)| Some(*id) != active.as_ref())
            .for_each(|(_, data)| data.replay.drop_checkpoints());
        let mut others: Vec<(u64, String, usize)> = self
            .rasters
            .iter()
            .filter(|(id, _)| !current.contains(*id) && !ghosts.contains(*id))
            .map(|(id, r)| (self.used.get(id).copied().unwrap_or(0), id.clone(), r.bytes()))
            .collect();
        others.sort_by_key(|a| std::cmp::Reverse(a.0));
        let mut total = 0usize;
        let over: Vec<String> = others
            .into_iter()
            .filter(|(_, _, bytes)| {
                total += bytes;
                total > self.budget
            })
            .map(|(_, id, _)| id)
            .collect();
        let garbage: Vec<_> = over.iter().map(|id| self.evict(id)).collect();
        if !garbage.is_empty() {
            std::thread::spawn(move || drop(garbage));
        }
    }

    #[cfg(test)]
    fn paint_cel_count_at(&self, frame: u32) -> usize {
        paint_cels(&self.bundle.doc, frame).len()
    }

    pub fn cache_bytes(&self) -> usize {
        self.rasters.values().map(Raster::bytes).sum::<usize>()
            + self.cels.values().map(|d| d.replay.checkpoint_bytes()).sum::<usize>()
            + self.below.bytes()
            + self.above.bytes()
    }

    fn forget_stale(
        &mut self,
        wanted: &[(String, String)],
        old: Option<&Document>,
        touched: Option<&BTreeSet<String>>,
    ) {
        let doc = &self.bundle.doc;
        let drawings = drawings(doc);
        let current: BTreeSet<String> = vector_ids(doc, self.frame).into_iter().collect();
        let shown: BTreeSet<&String> = wanted.iter().map(|(_, c)| c).collect();
        let stale: Vec<String> = self
            .rasters
            .keys()
            .filter(|c| !shown.contains(c))
            .filter(|c| match drawings.get(*c) {
                None => true,
                Some((layer, Kind::Paint)) => {
                    let bytes = fs::read(self.bundle.strokes_path(layer, c)).unwrap_or_default();
                    self.cels.get(*c).is_none_or(|d| d.file_hash != fnv(FNV_OFFSET, &bytes))
                }
                Some(_) if current.contains(*c) => false,
                Some(_) => {
                    touched.is_some_and(|t| t.contains(*c))
                        || old.is_some_and(|old| cel_nodes(old, c) != cel_nodes(doc, c))
                }
            })
            .cloned()
            .collect();
        let garbage: Vec<_> = stale.iter().map(|c| self.evict(c)).collect();
        if !garbage.is_empty() {
            std::thread::spawn(move || drop(garbage));
        }
    }

    fn sync_nodes(&mut self, cels: &BTreeSet<String>, brushes: &BrushSet) -> Result<Vec<usize>> {
        let (w, h) = self.size();
        let mut dirty: BTreeSet<usize> = BTreeSet::new();
        let wanted = paint_cels(&self.bundle.doc, self.frame);
        wanted
            .iter()
            .try_for_each(|(layer, cel)| self.sync_paint(layer, cel, brushes, &mut dirty))?;
        let current = self.current();
        let doc = &self.bundle.doc;
        let changed: Vec<(String, u64)> = cels
            .iter()
            .filter(|cel| self.vector_hash.contains_key(*cel) && current.contains(*cel))
            .map(|cel| (cel.clone(), nodes_hash(&nodes_of(doc, cel))))
            .filter(|(cel, hash)| self.vector_hash.get(cel) != Some(hash))
            .collect();
        changed.into_iter().try_for_each(|(cel, hash)| {
            let raster = match self.recall(&cel, hash) {
                Some(raster) => raster,
                None => rasterize_nodes(&nodes_of(&self.bundle.doc, &cel), w, h)?,
            };
            self.swap_vector(cel.clone(), raster, &mut dirty);
            self.vector_hash.insert(cel, hash);
            Ok::<_, anyhow::Error>(())
        })?;
        self.forget_stale(&wanted, None, Some(cels));
        self.load_missing(brushes)?;
        dirty.extend(self.build_onions());
        let dirty: Vec<usize> = dirty.into_iter().collect();
        self.rebuild_caches(Some(&dirty))?;
        Ok(dirty)
    }

    pub fn save_active(&mut self, summary: &str) -> Result<usize> {
        let Some(cel) = self.active_cel() else {
            return Ok(0);
        };
        let layer = self.active_id().unwrap_or_default();
        record(
            &self.bundle,
            &std::mem::take(&mut self.unsaved),
            "editor",
            &format!("{summary} on {layer}"),
            &[self.bundle.strokes_path(&layer, &cel)],
        )?;
        let Some(data) = self.cels.get_mut(&cel) else {
            return Ok(0);
        };
        self.bundle.embed_brushes_of(data.marks())?;
        let bytes = encode_cel(data.marks())?;
        write_atomic(&self.bundle.strokes_path(&layer, &cel), &bytes)?;
        data.file_hash = fnv(FNV_OFFSET, &bytes);
        self.bundle.doc.note_marks(&cel, data.marks());
        self.bundle.save()?;
        self.bundle.stamp("editor")?;
        Ok(bytes.len())
    }

    pub fn reload(&mut self, brushes: &BrushSet) -> Result<Vec<usize>> {
        let old = self.bundle.doc.clone();
        self.bundle.reload()?;
        self.unsaved = Patch::default();
        self.sync(&old, brushes)
    }

    pub fn apply(&mut self, ops: &[Op], brushes: &BrushSet) -> Result<Vec<usize>> {
        self.apply_with(ops, brushes).map(|(dirty, _)| dirty)
    }

    pub fn apply_with(&mut self, ops: &[Op], brushes: &BrushSet) -> Result<(Vec<usize>, Applied)> {
        let old = self.bundle.doc.clone();
        let applied = self.bundle.apply_as(ops, "editor")?;
        self.bundle.stamp("editor")?;
        Ok((self.sync(&old, brushes)?, applied))
    }

    fn retire(&mut self, cel: &str, hash: u64, raster: Raster) {
        self.memo.retain(|(c, h, _)| !(c == cel && *h == hash));
        self.memo.push_front((cel.to_string(), hash, raster));
        let mut total = 0usize;
        let keep = self
            .memo
            .iter()
            .take_while(|(_, _, r)| {
                total += r.bytes();
                total <= MEMO_BYTES
            })
            .count()
            .max(1);
        self.memo.truncate(keep);
    }

    fn recall(&mut self, cel: &str, hash: u64) -> Option<Raster> {
        let at = self.memo.iter().position(|(c, h, _)| c == cel && *h == hash)?;
        self.memo.remove(at).map(|(_, _, r)| r)
    }

    fn swap_vector(&mut self, cel: String, raster: Raster, dirty: &mut BTreeSet<usize>) {
        match self.rasters.get(&cel) {
            Some(old) => dirty.extend(old.differs(&raster)),
            None => dirty.extend(occupied(Some(&raster))),
        }
        let previous = self.vector_hash.get(&cel).copied();
        if let (Some(hash), Some(old)) = (previous, self.rasters.insert(cel.clone(), raster)) {
            self.retire(&cel, hash, old);
        }
    }

    pub fn apply_rendered(
        &mut self,
        ops: &[Op],
        rendered: (String, u64, Raster),
        brushes: &BrushSet,
    ) -> Result<Vec<usize>> {
        let old = self.bundle.doc.clone();
        self.bundle.apply_as(ops, "editor")?;
        self.bundle.stamp("editor")?;
        let (cel, hash, raster) = rendered;
        let mut extra = BTreeSet::new();
        self.swap_vector(cel.clone(), raster, &mut extra);
        self.vector_hash.insert(cel, hash);
        let dirty = self.sync(&old, brushes)?;
        if dirty.len() == self.all_tiles().len() {
            return Ok(dirty);
        }
        let missing: Vec<usize> = extra.iter().copied().filter(|i| !dirty.contains(i)).collect();
        self.rebuild_caches(Some(&missing))?;
        extra.extend(dirty);
        Ok(extra.into_iter().collect())
    }

    fn compose_with(
        &self,
        items: &[TileItem],
        index: usize,
        background: Option<&str>,
        onions: bool,
    ) -> Result<Vec<f16>> {
        let job = TileJob {
            frame: self.frame,
            rasters: &self.rasters,
            background,
            tx: index % self.cols(),
            ty: index / self.cols(),
            swap: None,
            onions: self.onion_map().filter(|_| onions),
            doc: &self.bundle.doc,
            references: true,
        };
        compose_tile(items, &job)
    }

    pub fn uses_caches(&self) -> bool {
        self.fast
    }

    pub fn preview_snapshot(&self, cels: &[String], brushes: &BrushSet) -> Option<crate::preview::Snapshot> {
        if self.slimmed() || self.stale || (self.tint_mask && self.active_is_mask()) {
            return None;
        }
        let replays = cels
            .iter()
            .map(|c| self.cels.get(c).map(|d| (c.clone(), d.replay.clone())))
            .collect::<Option<BTreeMap<_, _>>>()?;
        Some(crate::preview::Snapshot {
            rev: self.bundle.doc.rev,
            bundle: Bundle {
                root: self.bundle.root.clone(),
                seen: self.bundle.seen,
                doc: self.bundle.doc.clone(),
            },
            frame: self.frame,
            rasters: self.rasters.clone(),
            cels: replays,
            onions: self.onion_map().cloned(),
            brushes: brushes.clone(),
            vectors: std::sync::OnceLock::new(),
        })
    }

    fn vector_cels_since(&self, old: &Document) -> Vec<(String, u64)> {
        let before: BTreeMap<&str, &Vec<Node>> = old
            .all_layers()
            .into_iter()
            .filter(|l| l.kind() == Kind::Vector)
            .filter_map(|l| l.content_at(self.frame))
            .map(|c| (c.id.as_str(), &c.nodes))
            .collect();
        self.bundle
            .doc
            .all_layers()
            .into_iter()
            .filter(|l| l.kind() == Kind::Vector)
            .filter_map(|l| l.content_at(self.frame))
            .map(|c| {
                let known = self
                    .vector_hash
                    .get(&c.id)
                    .filter(|_| before.get(c.id.as_str()) == Some(&&c.nodes));
                let hash = known.copied().unwrap_or_else(|| nodes_hash(&c.nodes));
                (c.id.clone(), hash)
            })
            .collect()
    }

    fn sync_paint(&mut self, layer: &str, cel: &str, brushes: &BrushSet, dirty: &mut BTreeSet<usize>) -> Result<()> {
        let (w, h) = self.size();
        let bytes = fs::read(self.bundle.strokes_path(layer, cel)).unwrap_or_default();
        let hash = fnv(FNV_OFFSET, &bytes);
        if self.cels.get(cel).is_some_and(|d| d.file_hash == hash) {
            return Ok(());
        }
        if self.lean
            && !self.rasters.contains_key(cel)
            && let Some(data) = self.cels.get_mut(cel)
        {
            data.file_hash = 0;
            self.stale = true;
            return Ok(());
        }
        let marks = if bytes.is_empty() {
            Vec::new()
        } else {
            decode_cel(&bytes)?
        };
        match (self.cels.get_mut(cel), self.rasters.get_mut(cel)) {
            (Some(data), Some(raster)) => {
                dirty.extend(data.replay.update(raster, marks, brushes)?.tiles);
                data.file_hash = hash;
            }
            _ => {
                let (replay, fresh, _) = CelReplay::replay(w, h, marks, brushes)?;
                dirty.extend(occupied(self.rasters.get(cel)).chain(occupied(Some(&fresh))));
                self.rasters.insert(cel.to_string(), fresh);
                self.cels.insert(
                    cel.to_string(),
                    CelData {
                        replay,
                        file_hash: hash,
                    },
                );
            }
        }
        Ok(())
    }

    fn sync(&mut self, old: &Document, brushes: &BrushSet) -> Result<Vec<usize>> {
        let (w, h) = self.size();
        let active_id = old.locate_path(&self.active).map(|l| l.id.clone());
        if (self.below.width, self.below.height) != (w, h) {
            let kept = (
                self.tint_mask,
                self.onion.clone(),
                self.onion_on,
                std::mem::take(&mut self.onion_marked),
            );
            *self = Self::open_at(&self.bundle.root.clone(), self.frame)?;
            (self.tint_mask, self.onion, self.onion_on, self.onion_marked) = kept;
            self.refresh(brushes)?;
            return Ok(self.all_tiles());
        }
        let last = self.bundle.doc.timeline.frames - 1;
        let moved = self.frame > last;
        self.frame = self.frame.min(last);
        let mut dirty: BTreeSet<usize> = BTreeSet::new();
        let wanted = paint_cels(&self.bundle.doc, self.frame);
        wanted
            .iter()
            .try_for_each(|(layer, cel)| self.sync_paint(layer, cel, brushes, &mut dirty))?;
        let vectors = self.vector_cels_since(old);
        let changed: Vec<(String, u64)> = vectors
            .iter()
            .filter(|(cel, hash)| self.vector_hash.get(cel) != Some(hash))
            .cloned()
            .collect();
        let vectors_changed = !changed.is_empty();
        let (recalled, missing): (Vec<_>, Vec<_>) = changed
            .iter()
            .map(|(cel, hash)| (cel.clone(), self.recall(cel, *hash)))
            .partition(|(_, raster)| raster.is_some());
        let doc = &self.bundle.doc;
        let fresh = missing
            .par_iter()
            .map(|(cel, _)| {
                let before = nodes_of(old, cel);
                let drawn = self
                    .rasters
                    .get(cel)
                    .filter(|_| self.vector_hash.get(cel) == Some(&nodes_hash(&before)));
                redraw(&nodes_of(doc, cel), drawn.map(|r| (&before[..], r)), (w, h)).map(|r| (cel.clone(), r))
            })
            .collect::<Result<Vec<_>>>()?;
        recalled
            .into_iter()
            .filter_map(|(cel, raster)| raster.map(|r| (cel, r)))
            .chain(fresh)
            .for_each(|(cel, raster)| self.swap_vector(cel, raster, &mut dirty));
        vectors.into_iter().for_each(|(cel, hash)| {
            self.vector_hash.insert(cel, hash);
        });
        self.forget_stale(&wanted, Some(old), None);
        self.trim();
        self.active = active_id
            .and_then(|id| match self.bundle.doc.locate(&id) {
                Some(Loc::Layer(path)) => Some(path),
                _ => None,
            })
            .unwrap_or_else(|| self.default_active());
        self.load_missing(brushes)?;
        let ghosts = self.build_onions();
        let others = vectors_changed || !ghosts.is_empty();
        dirty.extend(ghosts);
        if moved || structure(old, self.frame) != structure(&self.bundle.doc, self.frame) {
            self.rebuild_caches(None)?;
            return Ok(self.all_tiles());
        }
        if self.slimmed() {
            if self.stale || others {
                self.stale = true;
                return Ok(self.all_tiles());
            }
            return Ok(dirty.into_iter().collect());
        }
        let dirty: Vec<usize> = dirty.into_iter().collect();
        self.rebuild_caches(Some(&dirty))?;
        Ok(dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_brush::StrokePainter;
    use klad_core::doc::{Cel, Loc, Shape};
    use klad_core::fill::{FillSource, Seed};
    use klad_core::stroke::marks;
    use klad_core::synth::Synth;
    use klad_render::{View, compose};

    struct Row {
        id: String,
        depth: usize,
        strokes: usize,
        fills: usize,
        mask: bool,
        clip: bool,
    }

    fn fills(data: &CelData) -> usize {
        data.marks().iter().filter(|m| m.as_fill().is_some()).count()
    }

    fn rows(scene: &Scene) -> Vec<Row> {
        fn row(scene: &Scene, layer: &Layer, path: &[usize]) -> Row {
            let data = layer.content_at(scene.frame).and_then(|c| scene.cels.get(&c.id));
            Row {
                depth: path.iter().filter(|i| **i != MASK).count() - 1 + usize::from(path.last() == Some(&MASK)),
                id: layer.id.clone(),
                strokes: data.map_or(0, |d| d.marks().len() - fills(d)),
                fills: data.map_or(0, fills),
                mask: path.last() == Some(&MASK),
                clip: layer.clip,
            }
        }
        fn walk(scene: &Scene, layers: &[Layer], path: &mut Vec<usize>, out: &mut Vec<Row>) {
            layers.iter().enumerate().rev().for_each(|(i, layer)| {
                path.push(i);
                out.push(row(scene, layer, path));
                if let Some(mask) = layer.mask.as_deref() {
                    path.push(MASK);
                    out.push(row(scene, mask, path));
                    path.pop();
                }
                walk(scene, layer.children(), path, out);
                path.pop();
            });
        }
        let mut out = Vec::new();
        walk(scene, &scene.bundle.doc.layers, &mut Vec::new(), &mut out);
        out
    }

    fn bundle(name: &str, top_blend: Blend) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-scene-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 300);
        doc.canvas.background = Some("#ffffff".into());
        let rect = Node::new(
            "n1",
            Shape::Rect {
                x: 50.0,
                y: 50.0,
                w: 400.0,
                h: 150.0,
                radius: 0.0,
            },
        )
        .filled("#3366aa");
        let mut top = Layer::paint("b", "b", "cb");
        top.blend = top_blend;
        top.opacity = 0.6;
        doc.layers = vec![
            Layer::vector("v", "v", "cv", vec![rect]),
            Layer::group("g", "g", vec![Layer::paint("a", "a", "ca")]),
            top,
        ];
        let b = Bundle::create(&dir, doc).unwrap();
        b.write_marks("a", "ca", &marks(Synth::canvas(600.0, 300.0).strokes(30, 1)))
            .unwrap();
        b.write_marks("b", "cb", &marks(Synth::canvas(600.0, 300.0).strokes(30, 2)))
            .unwrap();
        dir
    }

    fn tile_rgb(scene: &Scene, x: usize, y: usize) -> [u8; 3] {
        let index = (y / 256) * scene.cols() + x / 256;
        let composed = scene.compose(&[index], None).unwrap();
        let tile = &composed.iter().find(|(i, _)| *i == index).unwrap().1;
        let at = ((y % 256) * 256 + x % 256) * 4;
        let a = tile[at + 3].to_f32();
        [0, 1, 2].map(|c| (tile[at + c].to_f32() / a * 255.0).round() as u8)
    }

    fn agrees(scene: &Scene) {
        let paint: Rasters = scene
            .rasters
            .iter()
            .filter(|(k, _)| scene.cels.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let full = compose(&scene.bundle.doc, scene.frame, &paint, View::scaled(1.0)).unwrap();
        [(60, 60), (300, 120), (520, 250), (10, 290)]
            .iter()
            .for_each(|&(x, y)| {
                let reference = full.pixel(x as u32, y as u32);
                let tiled = tile_rgb(scene, x, y);
                assert!(
                    tiled.iter().zip(&reference[..3]).all(|(a, b)| a.abs_diff(*b) <= 2),
                    "({x},{y}) tiled {tiled:?} full {reference:?}"
                );
            });
    }

    fn looks(name: &str) -> Scene {
        let brushes = BrushSet::presets();
        let mut scene = Scene::open(&bundle(name, Blend::Normal)).unwrap();
        let ops = klad_core::ops::parse_lines(
            r##"{"op":"set","id":"v","effects":[{"glow":{"radius":10,"color":"#ffcc66"}},{"outline":{"width":3,"color":"#202020"}}]}"##,
        )
        .unwrap()
        .into_iter()
        .chain([Op::AddLayer {
            layer: serde_json::json!({"kind": "adjust", "id": "k", "name": "dusk", "adjust": {"curves": {"points": [[0, 0], [0.5, 0.3], [1, 0.9]]}}}),
            place: klad_core::ops::Place::default(),
        }])
        .collect::<Vec<_>>();
        scene.apply(&ops, &brushes).unwrap();
        scene
    }

    #[test]
    fn effects_and_adjustments_compose_like_the_cli_on_every_path() {
        let brushes = BrushSet::presets();
        let mut scene = looks("looks");
        scene.set_active(vec![1, 0]).unwrap();
        assert!(scene.fast, "an adjustment above is composed live over the cached below");
        agrees(&scene);
        let edge = tile_rgb(&scene, 48, 125);
        assert!(
            edge[0] < 80 && edge[1] < 80,
            "the outline shows past the rect: {edge:?}"
        );
        scene.set_active(vec![0]).unwrap();
        assert!(!scene.fast);
        agrees(&scene);
        let glow = klad_core::ops::parse_lines(
            r##"{"op":"set","id":"a","effects":[{"glow":{"radius":16,"blend":"normal","color":"#ff0000","strength":2}}]}"##,
        )
        .unwrap();
        scene.apply(&glow, &brushes).unwrap();
        scene.set_active(vec![1, 0]).unwrap();
        assert!(!scene.fast, "a layer with effects is painted on the full path");
        let brush = brushes.get("ink").unwrap();
        let stroke = Stroke::new(scene.next_mark_id(), "ink", [0.1, 0.2, 0.9, 1.0], 16.0, 7)
            .with_points(&[(190.0, 240.0, 1.0), (236.0, 240.0, 1.0)], 4.0);
        let mut buffer = StrokeBuffer::new(600, 300);
        let mut painter = StrokePainter::live(brush, &stroke);
        stroke.samples.iter().for_each(|s| painter.push(s, &mut buffer));
        let dirty = buffer.take_dirty();
        assert_eq!(dirty, vec![0], "the stroke stays in the first tile");
        let pixel = |tiles: &[(usize, Vec<f16>)], x: usize, y: usize| {
            let index = (y / 256) * 3 + x / 256;
            let tile = &tiles.iter().find(|(i, _)| *i == index).unwrap().1;
            let at = ((y % 256) * 256 + x % 256) * 4;
            [tile[at].to_f32(), tile[at + 1].to_f32(), tile[at + 2].to_f32()]
        };
        let before = scene.compose(&[1], None).unwrap();
        let live = {
            let preview = Preview {
                twin: None,
                buffer: &buffer,
                color: stroke.color(),
                opacity: 1.0,
                blend: StrokeBlend::Normal,
            };
            scene.compose(&dirty, Some(&preview)).unwrap()
        };
        assert!(
            live.iter().any(|(i, _)| *i == 1),
            "the glow's reach brings in the next tile"
        );
        scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
        let after = scene.compose(&[1], None).unwrap();
        let (b, l, a) = (
            pixel(&before, 262, 240),
            pixel(&live, 262, 240),
            pixel(&after, 262, 240),
        );
        assert!(
            a[1] < b[1] - 0.05,
            "the committed glow spills over the tile edge: {b:?} {a:?}"
        );
        assert!(
            l.iter().zip(&a).all(|(x, y)| (x - y).abs() < 0.01),
            "live {l:?} committed {a:?}"
        );
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app look_dab_timing -- --ignored --nocapture"]
    fn look_dab_timing() {
        use std::time::Instant;
        let brushes = BrushSet::presets();
        let dir = stress("look-dab-timing");
        let mut scene = Scene::open(&dir).unwrap();
        let layers = scene.bundle.doc.layers.len();
        scene.set_active(vec![1]).unwrap();
        let ops = vec![
            Op::AddLayer {
                layer: serde_json::json!({"kind": "adjust", "name": "dusk", "adjust": {"curves": {"points": [[0, 0], [0.5, 0.3], [1, 0.9]]}}}),
                place: klad_core::ops::Place::default(),
            },
            Op::Set {
                id: "l2".into(),
                props: serde_json::from_str(r#"{"effects":[{"glow":{"radius":24}}]}"#).unwrap(),
            },
        ];
        let pick: Vec<Op> = match std::env::var("LOOK").as_deref() {
            Ok("curves") => ops[..1].to_vec(),
            Ok("glow") => ops[1..].to_vec(),
            _ => ops,
        };
        scene.apply(&pick, &brushes).unwrap();
        let points: Vec<(f32, f32, f32)> = (0..240)
            .map(|i| {
                let t = i as f32 / 239.0;
                (200.0 + 1600.0 * t, 1024.0 + 500.0 * (t * 9.0).sin(), 0.4 + 0.6 * t)
            })
            .collect();
        let brush = brushes.get("ink").unwrap();
        let stroke = Stroke::new(999, "ink", [0.1, 0.1, 0.1, 1.0], 24.0, 3).with_points(&points, 4.0);
        let mut buffer = StrokeBuffer::new(2048, 2048);
        let mut live = StrokePainter::live(brush, &stroke);
        let times: Vec<f64> = stroke
            .samples
            .iter()
            .map(|sample| {
                let begin = Instant::now();
                live.push(sample, &mut buffer);
                let tiles = buffer.take_dirty();
                let preview = Preview {
                    twin: None,
                    buffer: &buffer,
                    color: stroke.color(),
                    opacity: 1.0,
                    blend: StrokeBlend::Normal,
                };
                scene.compose_painting(&tiles, &[], Some(&preview)).unwrap();
                begin.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        println!(
            "{layers} layers, a curves layer on top and a glow above the active layer: dab to composed tiles median {:.2} ms, p90 {:.2} ms, max {:.2} ms (fast path {})",
            percentile(&times, 0.5),
            percentile(&times, 0.9),
            percentile(&times, 1.0),
            scene.fast
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tiles_agree_with_full_render_on_every_path() {
        let mut fast = Scene::open(&bundle("fast", Blend::Normal)).unwrap();
        assert!(fast.fast);
        assert_eq!(fast.active, vec![2]);
        agrees(&fast);
        fast.set_active(vec![1, 0]).unwrap();
        assert!(fast.fast);
        agrees(&fast);
        let mut slow = Scene::open(&bundle("slow", Blend::Multiply)).unwrap();
        slow.set_active(vec![1, 0]).unwrap();
        assert!(
            slow.fast,
            "a multiplied layer above is composed live over the cached below"
        );
        agrees(&slow);
        let translucent = klad_core::ops::parse_lines(r#"{"op":"set","id":"g","opacity":0.8}"#).unwrap();
        slow.apply(&translucent, &BrushSet::presets()).unwrap();
        assert!(!slow.fast);
        agrees(&slow);
        assert_eq!(
            rows(&slow).iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["b", "g", "a", "v"]
        );
    }

    fn animated(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-frames-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 300);
        doc.canvas.background = Some("#ffffff".into());
        doc.timeline.frames = 6;
        let rect = |x: f32| {
            Node::new(
                &format!("n{x}"),
                Shape::Rect {
                    x,
                    y: 40.0,
                    w: 200.0,
                    h: 120.0,
                    radius: 0.0,
                },
            )
            .filled("#3366aa")
        };
        let mut vector = Layer::vector("v", "v", "v0", vec![rect(20.0)]);
        let mut later = Cel::at("v2", 2);
        later.nodes = vec![rect(300.0)];
        vector.cels_mut().unwrap().push(later);
        let mut lines = Layer::paint("a", "a", "a0");
        lines.cels_mut().unwrap()[0].hold = Some(2);
        lines.cels_mut().unwrap().extend([
            Cel {
                hold: Some(1),
                ..Cel::at("a2", 2)
            },
            Cel {
                link: Some("a0".into()),
                ..Cel::at("a4", 4)
            },
        ]);
        let mut top = Layer::paint("b", "b", "b0");
        top.blend = Blend::Multiply;
        doc.layers = vec![vector, lines, top];
        doc.next_id = 100;
        let b = Bundle::create(&dir, doc).unwrap();
        let synth = Synth::canvas(600.0, 300.0);
        b.write_marks("a", "a0", &marks(synth.strokes(20, 1))).unwrap();
        b.write_marks("a", "a2", &marks(synth.strokes(20, 3))).unwrap();
        b.write_marks("b", "b0", &marks(synth.strokes(10, 5))).unwrap();
        dir
    }

    fn line_stroke(id: u32, from: (f32, f32), to: (f32, f32), brushes: &BrushSet) -> (Stroke, StrokeBuffer) {
        let stroke = Stroke::new(id, "ink", [0.8, 0.1, 0.1, 1.0], 16.0, 7)
            .with_points(&[(from.0, from.1, 1.0), (to.0, to.1, 1.0)], 5.0);
        let mut buffer = StrokeBuffer::new(600, 300);
        let mut live = StrokePainter::new(brushes.get("ink").unwrap(), &stroke);
        stroke.samples.iter().for_each(|s| live.push(s, &mut buffer));
        (stroke, buffer)
    }

    #[test]
    fn frames_show_their_cels_and_paint_where_they_should() {
        let brushes = BrushSet::presets();
        let dir = animated("show");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![1]).unwrap();
        agrees(&scene);
        (0..6).for_each(|f| {
            scene.set_frame(f, &brushes).unwrap();
            assert_eq!(scene.frame, f);
            agrees(&scene);
        });
        assert!(scene.has_frame(0) && scene.has_frame(2));
        scene.set_frame(4, &brushes).unwrap();
        assert_eq!(scene.active_cel().as_deref(), Some("a0"));
        let before = scene.rasters["a0"].hash();
        let (stroke, mut buffer) = line_stroke(50, (20.0, 280.0), (580.0, 20.0), &brushes);
        scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
        scene.save_active("stroke").unwrap();
        assert_ne!(scene.rasters["a0"].hash(), before);
        assert_eq!(scene.bundle.read_marks("a", "a0").unwrap().len(), 21);
        scene.set_frame(0, &brushes).unwrap();
        agrees(&scene);
        scene.set_frame(3, &brushes).unwrap();
        assert_eq!(scene.active_cel(), None);
        let (stroke, mut buffer) = line_stroke(1, (20.0, 20.0), (580.0, 280.0), &brushes);
        scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
        scene.save_active("stroke").unwrap();
        let layer = scene.bundle.doc.layer("a").unwrap();
        let fresh = layer.cel_at(3).unwrap();
        assert_eq!((fresh.frame, fresh.link.as_deref()), (3, None));
        assert_eq!(layer.content_at(4).unwrap().id, "a0");
        agrees(&scene);
        let reopened = Scene::open_at(&dir, 3).unwrap();
        assert_eq!(reopened.rasters[&fresh.id].hash(), scene.rasters[&fresh.id].hash());
    }

    #[test]
    fn a_blink_made_from_the_timeline_paints_on_every_new_drawing() {
        let brushes = BrushSet::presets();
        let dir = animated("blink");
        let mut scene = Scene::open(&dir).unwrap();
        let add =
            klad_core::ops::parse_lines(r#"{"op":"add","layer":{"kind":"paint","cels":[{"frame":0}]},"above":"b"}"#)
                .unwrap();
        let (_, applied) = scene.apply_with(&add, &brushes).unwrap();
        let layer = applied.added[0].clone();
        let Some(Loc::Layer(path)) = scene.bundle.doc.locate(&layer) else {
            panic!()
        };
        scene.set_active(path).unwrap();
        let paint = |scene: &mut Scene, y: f32| {
            let (stroke, mut buffer) = line_stroke(scene.next_mark_id(), (40.0, y), (560.0, y), &brushes);
            scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
            scene.save_active("stroke").unwrap();
        };
        paint(&mut scene, 60.0);
        let first = scene.active_cel().unwrap();
        let hold = klad_core::ops::parse_lines(&format!(r#"{{"op":"set","id":"{first}","hold":4}}"#)).unwrap();
        scene.apply_with(&hold, &brushes).unwrap();
        [(4u32, 120.0f32), (5, 180.0)].iter().for_each(|&(frame, y)| {
            scene.set_frame(frame, &brushes).unwrap();
            if frame == 4 {
                assert_eq!(scene.active_cel(), None, "frame 4 is blank on the new layer");
            }
            let new =
                klad_core::ops::parse_lines(&format!(r#"{{"op":"cel","new":"{layer}","frame":{frame}}}"#)).unwrap();
            let (_, applied) = scene.apply_with(&new, &brushes).unwrap();
            let cel = applied.added[0].clone();
            assert_eq!(scene.active_cel().as_deref(), Some(cel.as_str()));
            paint(&mut scene, y);
            assert_eq!(scene.bundle.read_marks(&layer, &cel).unwrap().len(), 1);
            assert!(scene.rasters[&cel].occupied().count() > 0);
            agrees(&scene);
        });
        let reopened = Scene::open_at(&dir, 5).unwrap();
        assert_eq!(reopened.cels.len(), scene.paint_cel_count_at(5));
    }

    #[test]
    fn a_frame_picture_seen_once_comes_back_from_disk_and_the_canvas_job_keeps_one_too() {
        let brushes = BrushSet::presets();
        let dir = animated("pictures");
        let mut scene = Scene::open(&dir).unwrap();
        scene.keep_after = std::time::Duration::ZERO;
        let rendered = scene.frame_request(2, false, 0, &brushes).run().unwrap();
        assert!(!rendered.paint.is_empty());
        assert!(picture_path(&scene.pictures(), rendered.image.key).exists());
        let mut fresh = Scene::open(&dir).unwrap();
        let again = fresh.frame_request(2, false, 0, &brushes).run().unwrap();
        assert!(again.paint.is_empty(), "read from disk, nothing loaded");
        assert_eq!(again.image.key, rendered.image.key);
        assert_eq!(again.image.mips, rendered.image.mips);
        let ops = klad_core::ops::parse_lines(r#"{"op":"set","id":"b","opacity":0.5}"#).unwrap();
        fresh.apply(&ops, &brushes).unwrap();
        let changed = fresh.frame_request(2, false, 0, &brushes).run().unwrap();
        assert_ne!(changed.image.key, rendered.image.key);
        let key = fresh.frame_key(0, false, 1, brushes.fingerprint());
        let path = picture_path(&fresh.pictures(), key);
        assert!(!path.exists());
        fresh.move_to(0);
        let done = fresh
            .canvas_request(&brushes)
            .with_picture(key, 1, fresh.pictures(), Default::default())
            .run()
            .unwrap();
        assert!(fresh.adopt(done, true));
        let start = std::time::Instant::now();
        while !path.exists() && start.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let kept = read_picture(&fresh.pictures(), key).expect("the canvas job wrote the frame's picture");
        let made = fresh.frame_request(0, false, 1, &brushes);
        assert_eq!(made.key, key);
        let reference = picture(
            &made.bundle.doc,
            0,
            key,
            1,
            &{
                let mut all = made.rasters.clone();
                made.paint.iter().for_each(|(l, c)| {
                    all.insert(c.clone(), load_paint(&made.bundle, l, c, &brushes).unwrap().1);
                });
                made.vector.iter().for_each(|(c, _, nodes)| {
                    all.insert(c.clone(), rasterize_nodes(nodes, 600, 300).unwrap());
                });
                all
            },
            None,
        )
        .unwrap();
        assert_eq!(kept.mips[0], reference.mips[0]);
    }

    fn layered(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-lean-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 400);
        doc.canvas.background = Some("#f4efe6".into());
        ["p0", "p1", "p2", "p3"].iter().enumerate().for_each(|(i, id)| {
            doc.layers.push(Layer::paint(id, id, &format!("c{i}")));
        });
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let lines: Vec<String> = (0..4)
            .flat_map(|i| {
                (0..6).map(move |k| {
                    format!(
                        r#"{{"op":"stroke","layer":"p{i}","path":"M{} {} L{} {}","size":{},"color":"{}"}}"#,
                        20 + k * 90 + i * 7,
                        30 + i * 80,
                        80 + k * 90,
                        120 + i * 70,
                        6 + i * 3,
                        ["#aa3322", "#2255aa", "#22aa55", "#555555"][i]
                    )
                })
            })
            .collect();
        bundle
            .apply(&klad_core::ops::parse_lines(&lines.join("\n")).unwrap())
            .unwrap();
        dir
    }

    fn shown(scene: &Scene) -> Vec<u64> {
        scene
            .compose(&scene.all_tiles(), None)
            .unwrap()
            .into_iter()
            .map(|(i, t)| fnv(i as u64, bytemuck::cast_slice(&t)))
            .collect()
    }

    fn catch_up(scene: &mut Scene, brushes: &BrushSet) {
        let done = scene.canvas_request(brushes).run().unwrap();
        assert!(scene.adopt(done, true));
    }

    #[test]
    fn planes_on_disk_bring_a_lean_frame_back_with_only_the_active_pixels() {
        let brushes = BrushSet::presets();
        let dir = layered("planes");
        let full = Scene::open(&dir).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let cache = CelCache::new(&bundle, &brushes, Write::Always);
        paint_cels(&bundle.doc, 0).iter().for_each(|(layer, cel)| {
            cache.load(&bundle, &brushes, layer, cel, true).unwrap();
        });
        let mut first = Scene::open_lazy(&dir).unwrap();
        first.lean = true;
        let ask = first.planes_ask(0).unwrap();
        let key = ask.key(&first.bundle, &brushes);
        let made = first.canvas_request(&brushes).run().unwrap();
        assert_eq!(made.paint.len(), 4, "the first visit loads every drawing");
        let start = std::time::Instant::now();
        while !planes::path(&ask.dir, key).exists() && start.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let stored = planes::read(&ask.dir, key).unwrap();
        let (below, above) = made.caches.clone().unwrap();
        let same = |a: &Raster, b: &Raster| (0..a.cols * a.rows).all(|i| a.at(i) == b.at(i));
        assert!(
            same(&below, &stored.below) && same(&above, &stored.above),
            "planes read back as built"
        );
        assert_eq!(stored.thumbs.len(), made.thumbs.len());
        let mut again = Scene::open_lazy(&dir).unwrap();
        again.lean = true;
        let back = again.canvas_request(&brushes).run().unwrap();
        assert_eq!(back.paint.len(), 1, "a revisit loads only the active drawing's pixels");
        assert_eq!(back.strokes.len(), 3, "and the others' strokes");
        assert!(again.adopt(back, true));
        assert!(again.fast && !again.stale);
        assert_eq!(again.cels.len(), 4);
        assert_eq!(shown(&again), shown(&full));
        let ops = klad_core::ops::parse_lines(r##"{"op":"set","id":"c0:s1","color":"#ffcc00"}"##).unwrap();
        let mut edited = Scene::open(&dir).unwrap();
        edited.apply(&ops, &brushes).unwrap();
        assert_ne!(
            again.planes_ask(0).unwrap().key(&again.bundle, &brushes),
            key,
            "an edit changes the key"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lean_scene_keeps_only_the_active_pixels_and_shows_and_edits_the_same() {
        let brushes = BrushSet::presets();
        let dir = layered("lean");
        let mut full = Scene::open(&dir).unwrap();
        let mut lean = Scene::open_lazy(&dir).unwrap();
        lean.lean = true;
        catch_up(&mut lean, &brushes);
        assert!(lean.fast && !lean.stale);
        assert_eq!(lean.rasters.len(), 1, "only the active drawing keeps its pixels");
        assert_eq!(lean.cels.len(), 4, "every drawing keeps its strokes");
        assert_eq!(shown(&lean), shown(&full));
        let ops = |line: &str| klad_core::ops::parse_lines(line).unwrap();
        let recolour = ops(r##"{"op":"set","id":"c3:s1","color":"#ffcc00"}"##);
        full.apply(&recolour, &brushes).unwrap();
        lean.apply(&recolour, &brushes).unwrap();
        assert!(!lean.stale, "an edit on the active drawing needs nothing reloaded");
        assert_eq!(shown(&lean), shown(&full));
        let at = (100.0, 70.0);
        assert_eq!(lean.sample(at, false).unwrap(), full.sample(at, false).unwrap());
        let all = klad_core::fill::FillRef::All;
        assert_eq!(
            lean.fill_reference(&all, &brushes).unwrap().hash(),
            full.fill_reference(&all, &brushes).unwrap().hash()
        );
        full.set_active(vec![1]).unwrap();
        lean.set_active_later(vec![1]);
        assert!(lean.stale);
        catch_up(&mut lean, &brushes);
        assert_eq!(lean.rasters.len(), 1);
        assert_eq!(shown(&lean), shown(&full));
        let mut outside = Bundle::open(&dir).unwrap();
        outside
            .apply(&ops(
                r#"{"op":"stroke","layer":"p3","path":"M10 380 L590 20","size":20}"#,
            ))
            .unwrap();
        full.reload(&brushes).unwrap();
        lean.reload(&brushes).unwrap();
        assert!(
            lean.stale,
            "a dropped drawing that changed outside is reloaded by the canvas job"
        );
        catch_up(&mut lean, &brushes);
        assert_eq!(shown(&lean), shown(&full));
        assert_eq!(lean.cels["c3"].marks().len(), full.cels["c3"].marks().len());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn preparing_a_frame_writes_its_picture_and_drawings_once() {
        let brushes = BrushSet::presets();
        let dir = animated("prepare");
        let mut scene = Scene::open_lazy(&dir).unwrap();
        let request = scene.prepare_request(2, false, 0, &brushes);
        let key = request.key;
        assert!(request.rasters.is_empty() && !request.paint.is_empty());
        assert!(matches!(request.prepare().unwrap(), Prepared::Frame(n) if n > 0));
        assert!(picture_path(&scene.pictures(), key).exists());
        let cache = CelCache::new(&scene.bundle, &brushes, Write::Never);
        assert!(
            paint_cels(&scene.bundle.doc, 2)
                .iter()
                .all(|(l, c)| cache.holds(&scene.bundle, l, c))
        );
        assert_eq!(
            scene.prepare_request(2, false, 0, &brushes).prepare().unwrap(),
            Prepared::Already
        );
        fs::remove_file(picture_path(&scene.pictures(), key)).unwrap();
        assert!(matches!(
            scene.prepare_request(2, false, 0, &brushes).prepare().unwrap(),
            Prepared::Frame(0)
        ));
        let shown = scene.frame_request(2, false, 0, &brushes).run().unwrap();
        assert!(shown.paint.is_empty(), "the prepared picture comes from disk");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn other_frames_reload_after_outside_edits_and_loads_go_stale() {
        let brushes = BrushSet::presets();
        let dir = animated("outside");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_frame(2, &brushes).unwrap();
        scene.set_frame(0, &brushes).unwrap();
        assert!(scene.cels.contains_key("a2"));
        let mut outside = Bundle::open(&dir).unwrap();
        let ops =
            klad_core::ops::parse_lines(r#"{"op":"stroke","layer":"a","frame":2,"path":"M10 10 L200 100","size":12}"#)
                .unwrap();
        outside.apply(&ops).unwrap();
        scene.reload(&brushes).unwrap();
        assert!(!scene.cels.contains_key("a2"));
        assert!(scene.frame_job(2, &brushes).is_some());
        scene.set_frame(2, &brushes).unwrap();
        assert_eq!(scene.cels["a2"].marks().len(), 21);
        agrees(&scene);
        let job = scene.frame_job(5, &brushes);
        assert!(job.is_none(), "frame 5 shows a0 and v2, both loaded");
        scene.set_frame(0, &brushes).unwrap();
        let mut outside = Bundle::open(&dir).unwrap();
        let ops = klad_core::ops::parse_lines(r##"{"op":"set","id":"n300","fill":"#aa3333"}"##).unwrap();
        outside.apply(&ops).unwrap();
        scene.reload(&brushes).unwrap();
        assert!(!scene.rasters.contains_key("v2") && scene.rasters.contains_key("v0"));
        scene.set_frame(2, &brushes).unwrap();
        agrees(&scene);
        scene.budget = 0;
        scene.set_frame(0, &brushes).unwrap();
        assert!(!scene.cels.contains_key("a2") && !scene.rasters.contains_key("v2"));
        let done = scene.frame_request(2, false, 0, &brushes).run().unwrap();
        let ops = klad_core::ops::parse_lines(r#"{"op":"set","id":"b","opacity":0.5}"#).unwrap();
        scene.apply(&ops, &brushes).unwrap();
        scene.absorb(done);
        assert!(!scene.has_frame(2), "drawings loaded before an edit are dropped");
        scene.budget = CACHE_BYTES;
        let done = scene.frame_request(2, false, 0, &brushes).run().unwrap();
        scene.absorb(done);
        assert!(scene.has_frame(2));
        scene.set_frame(2, &brushes).unwrap();
        agrees(&scene);
    }

    #[test]
    fn stroke_undo_save_and_incremental_reload() {
        let brushes = BrushSet::presets();
        let dir = bundle("edit", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        let before = scene.rasters["cb"].hash();
        let stroke = Stroke::new(99, "ink", [0.0, 0.0, 0.0, 1.0], 20.0, 3)
            .with_points(&[(10.0, 10.0, 1.0), (590.0, 290.0, 1.0)], 5.0);
        let mut buffer = StrokeBuffer::new(600, 300);
        let mut live = StrokePainter::new(brushes.get("ink").unwrap(), &stroke);
        stroke.samples.iter().for_each(|s| live.push(s, &mut buffer));
        assert!(
            !scene
                .commit_stroke(stroke, &mut buffer, StrokeBlend::Normal)
                .unwrap()
                .is_empty()
        );
        scene.save_active("stroke").unwrap();
        let other = Scene::open(&dir).unwrap();
        assert_eq!(other.rasters["cb"].hash(), scene.rasters["cb"].hash());
        assert!(scene.step(klad_core::history::Stack::Undo, &brushes).unwrap().is_some());
        assert_eq!(scene.rasters["cb"].hash(), before);
        let mut outside = Bundle::open(&dir).unwrap();
        let ops =
            klad_core::ops::parse_lines(r#"{"op":"stroke","layer":"a","path":"M10 10 L200 100","size":12}"#).unwrap();
        outside.apply(&ops).unwrap();
        assert!(scene.bundle.changed_on_disk());
        let dirty = scene.reload(&brushes).unwrap();
        assert!(!dirty.is_empty() && dirty.len() < scene.all_tiles().len(), "{dirty:?}");
        assert_eq!(scene.cels["ca"].marks().len(), 31);
        let replayed = klad_brush::replay(600, 300, scene.cels["ca"].marks(), &brushes)
            .unwrap()
            .raster;
        assert_eq!(replayed.hash(), scene.rasters["ca"].hash());
        agrees(&scene);
    }

    #[test]
    fn editor_ops_refresh_vector_tiles() {
        let brushes = BrushSet::presets();
        let dir = bundle("ops", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        let ops = klad_core::ops::parse_lines(r##"{"op":"set","id":"n1","fill":"#ff0000","transform.x":40}"##).unwrap();
        let dirty = scene.apply(&ops, &brushes).unwrap();
        assert!(!dirty.is_empty());
        agrees(&scene);
        let log = klad_core::history::entries(&scene.bundle, klad_core::history::Stack::Undo).unwrap();
        assert_eq!(log[0].author, "editor");
        let hide = klad_core::ops::parse_lines(r#"{"op":"set","id":"v","visible":false}"#).unwrap();
        assert_eq!(scene.apply(&hide, &brushes).unwrap().len(), scene.all_tiles().len());
        agrees(&scene);
    }

    #[test]
    #[ignore = "timing run: DRAW_BUNDLE=<bundle> cargo test --release -p klad-app timing -- --ignored --nocapture"]
    fn timing() {
        let path = std::env::var("DRAW_BUNDLE").expect("DRAW_BUNDLE");
        let brushes = BrushSet::presets();
        let start = std::time::Instant::now();
        let mut scene = Scene::open(Path::new(&path)).unwrap();
        println!(
            "open, rasterize vectors, build caches: {:.0} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = std::time::Instant::now();
        let tiles = scene.compose(&scene.all_tiles(), None).unwrap();
        println!(
            "compose all {} tiles: {:.0} ms",
            tiles.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        let target = scene.default_active();
        let layer = scene.bundle.doc.layer_at(&target).id.clone();
        let mut outside = Bundle::open(Path::new(&path)).unwrap();
        let line =
            format!(r#"{{"op":"stroke","layer":"{layer}","path":"M300 300 C 900 100 1400 900 2000 600","size":60}}"#);
        outside.apply(&klad_core::ops::parse_lines(&line).unwrap()).unwrap();
        let start = std::time::Instant::now();
        let dirty = scene.reload(&brushes).unwrap();
        let reload = start.elapsed().as_secs_f64() * 1000.0;
        let start = std::time::Instant::now();
        scene.compose(&dirty, None).unwrap();
        println!(
            "external stroke: reload {reload:.0} ms, {} tiles recomposed in {:.0} ms",
            dirty.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        klad_core::history::step(&mut outside, klad_core::history::Stack::Undo).unwrap();
        let scene = Scene::open(Path::new(&path)).unwrap();
        for node in std::env::var("DRAW_NODES")
            .unwrap_or_default()
            .split(',')
            .filter(|n| !n.is_empty())
        {
            let Some(Loc::Node(layer, c, at)) = scene.bundle.doc.locate(node) else {
                continue;
            };
            let nodes = scene.bundle.doc.layer_at(&layer).cels()[c].nodes.clone();
            let index = at[0];
            let (w, h) = scene.size();
            let op = klad_core::ops::parse_lines(&format!(
                r#"{{"op":"set","id":"{node}","transform.x":{}}}"#,
                nodes[index].transform.x + 25.0
            ))
            .unwrap();
            let mut editing = Scene::open(Path::new(&path)).unwrap();
            let start = std::time::Instant::now();
            let done = previewed(&editing, op.clone(), &[], &brushes);
            println!(
                "preview {node} ({layer:?}, {w}x{h}): {:.0} ms, {} tiles composed",
                start.elapsed().as_secs_f64() * 1000.0,
                done.tiles.len()
            );
            let start = std::time::Instant::now();
            let dirty = editing.apply(&op, &brushes).unwrap();
            println!(
                "commit {node}: apply {:.0} ms, {} tiles",
                start.elapsed().as_secs_f64() * 1000.0,
                dirty.len()
            );
            let start = std::time::Instant::now();
            let mut probe = Bundle::open(Path::new(&path)).unwrap();
            let opened = start.elapsed().as_secs_f64() * 1000.0;
            let start = std::time::Instant::now();
            klad_core::history::step(&mut probe, klad_core::history::Stack::Undo).unwrap();
            let io = start.elapsed().as_secs_f64() * 1000.0;
            klad_core::history::step(&mut probe, klad_core::history::Stack::Redo).unwrap();
            let start = std::time::Instant::now();
            let step = editing
                .step(klad_core::history::Stack::Undo, &brushes)
                .unwrap()
                .map(|(_, d)| d.len());
            println!(
                "undo {node}: open bundle {opened:.0} ms, history step io {io:.0} ms, editor undo total {:.0} ms ({step:?} tiles)",
                start.elapsed().as_secs_f64() * 1000.0
            );
            let start = std::time::Instant::now();
            let step = editing
                .step(klad_core::history::Stack::Redo, &brushes)
                .unwrap()
                .map(|(_, d)| d.len());
            println!(
                "redo {node}: editor redo total {:.0} ms ({step:?} tiles)",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    fn previewed(scene: &Scene, ops: Vec<Op>, cels: &[String], brushes: &BrushSet) -> crate::preview::Done {
        crate::preview::Job {
            ops,
            snap: std::sync::Arc::new(scene.preview_snapshot(cels, brushes).unwrap()),
            shown: BTreeSet::new(),
            visible: scene.all_tiles().into_iter().collect(),
            cancel: std::sync::Arc::default(),
        }
        .run()
        .unwrap()
    }

    fn preview_agrees(scene: &mut Scene, ops: Vec<Op>, cels: &[String], brushes: &BrushSet) -> usize {
        let done = previewed(scene, ops.clone(), cels, brushes);
        let dirty = scene.apply(&ops, brushes).unwrap();
        assert!(
            dirty.iter().all(|i| done.changed.contains(i)),
            "the commit changed {dirty:?}, the preview {:?}",
            done.changed
        );
        let indices: Vec<usize> = done.tiles.iter().map(|(i, _)| *i).collect();
        let committed = scene.compose(&indices, None).unwrap();
        done.tiles.iter().zip(&committed).for_each(|((i, shown), (j, after))| {
            assert_eq!(i, j);
            let worst = shown
                .iter()
                .zip(after)
                .map(|(a, b)| (a.to_f32() - b.to_f32()).abs())
                .fold(0.0f32, f32::max);
            assert!(worst <= 2.0 / 255.0, "tile {i} previewed {:.2}/255 off", worst * 255.0);
        });
        done.tiles.len()
    }

    #[test]
    fn previews_compose_what_the_edit_will_show_under_every_blend() {
        let brushes = BrushSet::presets();
        [Blend::Normal, Blend::Multiply, Blend::Screen, Blend::Overlay]
            .iter()
            .for_each(|&blend| {
                let dir = bundle(&format!("preview-{blend:?}"), blend);
                let mut scene = Scene::open(&dir).unwrap();
                let moved = klad_core::ops::parse_lines(r#"{"op":"set","id":"n1","transform.x":120,"transform.y":40}"#)
                    .unwrap();
                let tiles = preview_agrees(&mut scene, moved, &[], &brushes);
                assert!(tiles > 0 && tiles < scene.all_tiles().len(), "{blend:?}: {tiles} tiles");
            });
    }

    #[test]
    fn undo_and_redo_reuse_remembered_vector_rasters() {
        let brushes = BrushSet::presets();
        let dir = bundle("memo", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        let before = scene.rasters["cv"].hash();
        let ops = klad_core::ops::parse_lines(r#"{"op":"set","id":"n1","transform.x":60}"#).unwrap();
        scene.apply(&ops, &brushes).unwrap();
        let after = scene.rasters["cv"].hash();
        assert_ne!(before, after);
        assert!(scene.memo.iter().any(|(c, _, r)| c == "cv" && r.hash() == before));
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap().unwrap();
        assert_eq!(scene.rasters["cv"].hash(), before);
        assert!(scene.memo.iter().any(|(c, _, r)| c == "cv" && r.hash() == after));
        scene.step(klad_core::history::Stack::Redo, &brushes).unwrap().unwrap();
        assert_eq!(scene.rasters["cv"].hash(), after);
        let fresh = rasterize_nodes(&nodes_of(&scene.bundle.doc, "cv"), 600, 300).unwrap();
        assert_eq!(fresh.hash(), after);
        agrees(&scene);
    }

    #[test]
    fn editor_fills_use_the_scene_reference_save_and_reload_identically() {
        let brushes = BrushSet::presets();
        let dir = bundle("fill", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![1, 0]).unwrap();
        let reference = scene.fill_reference(&FillRef::Layer("v".into()), &brushes).unwrap();
        let seed = Seed {
            x: 100.0,
            y: 100.0,
            tolerance: 0.1,
            gap: 0.0,
            expand: 0.0,
        };
        let coverage = klad_brush::flood(&reference, &seed);
        assert_eq!(coverage.pixels(), 400 * 150);
        let fill = Fill::new(
            scene.next_mark_id(),
            [0.9, 0.7, 0.2, 1.0],
            FillSource::Seed {
                seed,
                reference: FillRef::Layer("v".into()),
            },
            coverage,
        );
        let touched = scene.commit_fill(fill, &brushes).unwrap();
        assert!(!touched.is_empty());
        scene.save_active("fill").unwrap();
        let other = Scene::open(&dir).unwrap();
        assert_eq!(other.rasters["ca"].hash(), scene.rasters["ca"].hash());
        assert_eq!(
            rows(&other).iter().find(|r| r.id == "a").map(|r| (r.strokes, r.fills)),
            Some((30, 1))
        );
        let image = scene.sample((300.0, 120.0), false).unwrap().unwrap();
        let layer = scene.sample((300.0, 120.0), true).unwrap().unwrap();
        assert!(
            (layer[0] - 0.9).abs() < 0.01 && (layer[2] - 0.2).abs() < 0.01,
            "{layer:?}"
        );
        assert!(image[3] > 0.99);
        assert!(scene.sample((-5.0, 3.0), false).unwrap().is_none());
        let undo = scene.step(klad_core::history::Stack::Undo, &brushes).unwrap().unwrap();
        assert!(undo.0.contains("fill"));
        assert_eq!(fills(&scene.cels["ca"]), 0);
        agrees(&scene);
    }

    fn onion_agrees(scene: &Scene) -> usize {
        let doc = &scene.bundle.doc;
        let paint: Rasters = scene
            .rasters
            .iter()
            .filter(|(k, _)| scene.cels.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let plan = onion::plan(doc, scene.frame, &scene.onion_layers(), &scene.onion);
        let onions = onion::build(doc, &plan, &scene.rasters);
        let full = klad_render::compose_view(doc, scene.frame, &paint, View::scaled(1.0), None, Some(&onions)).unwrap();
        let plain = compose(doc, scene.frame, &paint, View::scaled(1.0)).unwrap();
        let points: Vec<(usize, usize)> = (0..12)
            .flat_map(|i| (0..6).map(move |j| (i * 50 + 7, j * 50 + 9)))
            .collect();
        points.iter().for_each(|&(x, y)| {
            let reference = full.pixel(x as u32, y as u32);
            let tiled = tile_rgb(scene, x, y);
            assert!(
                tiled.iter().zip(&reference[..3]).all(|(a, b)| a.abs_diff(*b) <= 2),
                "({x},{y}) at frame {}: tiled {tiled:?} full {reference:?}",
                scene.frame
            );
        });
        points
            .iter()
            .filter(|&&(x, y)| full.pixel(x as u32, y as u32) != plain.pixel(x as u32, y as u32))
            .count()
    }

    #[test]
    fn the_editor_onion_matches_the_cli_on_every_path() {
        let brushes = BrushSet::presets();
        let dir = animated("onion");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![1]).unwrap();
        scene.onion_on = true;
        let clipped = klad_core::ops::parse_lines(r#"{"op":"set","id":"b","clip":true}"#).unwrap();
        scene.apply(&clipped, &brushes).unwrap();
        scene.set_frame(2, &brushes).unwrap();
        assert!(!scene.fast);
        assert_eq!(
            scene.onion_plan(2)[0]
                .1
                .iter()
                .map(|g| g.cel.as_str())
                .collect::<Vec<_>>(),
            ["a0"]
        );
        assert!(onion_agrees(&scene) > 3, "the onion shows");
        scene.onion_on = false;
        assert!(!scene.refresh_onions(&brushes).unwrap().is_empty());
        assert_eq!(onion_agrees(&scene), 0);
        scene.onion_on = true;
        scene.refresh_onions(&brushes).unwrap();
        let normal = klad_core::ops::parse_lines(r#"{"op":"set","id":"b","blend":"normal","clip":false}"#).unwrap();
        scene.apply(&normal, &brushes).unwrap();
        assert!(scene.fast, "the onion of the active layer rides in the cached below");
        assert!(onion_agrees(&scene) > 3);
        scene.set_active(vec![2]).unwrap();
        scene.onion_marked.insert("a".into());
        scene.refresh_onions(&brushes).unwrap();
        assert!(scene.fast);
        assert!(onion_agrees(&scene) > 3, "a marked layer keeps its onion");
        (0..6).for_each(|f| {
            scene.set_frame(f, &brushes).unwrap();
            onion_agrees(&scene);
        });
        let before = scene.frame_key(5, true, 0, 1);
        let stroke =
            klad_core::ops::parse_lines(r#"{"op":"stroke","layer":"a","frame":2,"path":"M10 10 L200 100","size":12}"#)
                .unwrap();
        scene.apply(&stroke, &brushes).unwrap();
        assert_ne!(scene.frame_key(5, true, 0, 1), before, "a2 is a ghost of frame 5");
        let changed = scene.frame_job(scene.frame, &brushes);
        assert!(changed.is_none(), "ghosts stay loaded after an edit");
    }

    #[test]
    fn frame_keys_share_holds_and_links_and_follow_edits() {
        let brushes = BrushSet::presets();
        let dir = animated("keys");
        let mut scene = Scene::open(&dir).unwrap();
        let fp = brushes.fingerprint();
        let keys: Vec<u64> = (0..6).map(|f| scene.frame_key(f, false, 0, fp)).collect();
        assert_eq!(keys[0], keys[1], "a hold");
        assert_ne!(keys[1], keys[2]);
        assert_ne!(keys[2], keys[3]);
        assert_eq!(keys[4], keys[5]);
        assert_ne!(keys[0], keys[4], "the vector layer changed at frame 2");
        assert_ne!(
            scene.frame_key(0, false, 1, fp),
            keys[0],
            "the level is part of the key"
        );
        let stroke =
            klad_core::ops::parse_lines(r#"{"op":"stroke","layer":"a","frame":2,"path":"M10 10 L200 100","size":12}"#)
                .unwrap();
        scene.apply(&stroke, &brushes).unwrap();
        let after: Vec<u64> = (0..6).map(|f| scene.frame_key(f, false, 0, fp)).collect();
        let moved: Vec<usize> = (0..6).filter(|&f| after[f] != keys[f]).collect();
        assert_eq!(moved, [2], "only the frame showing a2 changed");
        let clips = klad_core::ops::parse_lines(
            "{\"op\":\"clip\",\"add\":\"run\",\"from\":0,\"to\":3,\"loop\":true,\"fps\":24}\n{\"op\":\"export\",\"godot\":\"game\"}",
        )
        .unwrap();
        let (dirty, _) = scene.apply_with(&clips, &brushes).unwrap();
        assert!(dirty.is_empty(), "clips and the export folder change no pixels");
        let with_clip: Vec<u64> = (0..6).map(|f| scene.frame_key(f, false, 0, fp)).collect();
        assert_eq!(with_clip, after, "so every cached frame stays");
        assert_eq!(scene.bundle.doc.timeline.clip("run").map(|c| c.fps), Some(Some(24)));
    }

    #[test]
    fn a_frame_job_composes_what_the_canvas_shows() {
        let brushes = BrushSet::presets();
        let dir = animated("job");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![1]).unwrap();
        scene.onion_on = true;
        let request = scene.frame_request(2, true, 0, &brushes);
        assert_eq!(
            request.paint,
            [("a".to_string(), "a2".to_string())],
            "a2 is not loaded yet"
        );
        let key = request.key;
        let done = request.run().unwrap();
        let image = scene.absorb(done);
        assert_eq!(image.key, key);
        assert_eq!((image.width, image.height), (600, 300));
        assert_eq!(image.mips.len(), crate::gpu::levels_for((600, 300)) as usize);
        assert_eq!(
            image.mips.last().unwrap().len(),
            2 * 4,
            "600x300 ends at 2x1, as wgpu sizes the levels"
        );
        assert!(scene.has_frame(2), "the job's drawings were kept");
        scene.set_frame(2, &brushes).unwrap();
        [(60usize, 60usize), (300, 120), (520, 250), (10, 290), (157, 33)]
            .iter()
            .for_each(|&(x, y)| {
                let at = (y * 600 + x) * 4;
                let got = &image.mips[0][at..at + 3];
                let want = tile_rgb(&scene, x, y);
                assert!(
                    got.iter().zip(&want).all(|(a, b)| a.abs_diff(*b) <= 2),
                    "({x},{y}) {got:?} vs {want:?}"
                );
            });
        let half = scene.frame_request(2, true, 1, &brushes).run().unwrap().image;
        assert_eq!((half.width, half.height), (300, 150));
    }

    #[test]
    fn painting_into_a_layer_without_a_cel_creates_one() {
        let brushes = BrushSet::presets();
        let dir = std::env::temp_dir().join(format!("draw-scene-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(64, 64);
        let mut layer = Layer::paint("p", "p", "c1");
        layer.cels_mut().unwrap().clear();
        doc.layers.push(layer);
        Bundle::create(&dir, doc).unwrap();
        let mut scene = Scene::open(&dir).unwrap();
        assert!(scene.can_paint());
        let stroke = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 8.0, 1).with_points(&[(32.0, 32.0, 1.0)], 0.0);
        let mut buffer = StrokeBuffer::new(64, 64);
        let mut live = StrokePainter::new(brushes.get("ink").unwrap(), &stroke);
        stroke.samples.iter().for_each(|s| live.push(s, &mut buffer));
        scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
        scene.save_active("stroke").unwrap();
        let reopened = Bundle::open(&dir).unwrap();
        let cel = reopened.doc.layer("p").unwrap().cels()[0].id.clone();
        assert_eq!(reopened.read_strokes("p", &cel).unwrap().len(), 1);
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap().unwrap();
        assert!(scene.bundle.doc.layer("p").unwrap().cels().is_empty());
        assert!(!scene.bundle.strokes_path("p", &cel).exists());
        scene.step(klad_core::history::Stack::Redo, &brushes).unwrap().unwrap();
        assert_eq!(scene.active_marks().len(), 1);
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app stroke_edit_timing -- --ignored --nocapture (DENSE=1 packs the strokes into 640 px)"]
    fn stroke_edit_timing() {
        let brushes = BrushSet::presets();
        let dir = std::env::temp_dir().join(format!("draw-scene-edit-timing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(2048, 2048);
        doc.canvas.background = Some("#f4efe6".into());
        doc.layers = vec![Layer::paint("ink", "ink", "c1")];
        let b = Bundle::create(&dir, doc).unwrap();
        let side = if std::env::var("DENSE").is_ok() { 640.0 } else { 2048.0 };
        let mut synth = Synth::canvas(side, side);
        synth.brushes = vec!["ink".into(), "pencil".into(), "marker".into()];
        b.write_marks("ink", "c1", &marks(synth.strokes(1000, 3))).unwrap();
        let mut scene = Scene::open(&dir).unwrap();
        let ids: Vec<u32> = scene.active_marks().iter().map(Mark::id).collect();
        let timed = |scene: &mut Scene, lines: String, what: &str| {
            let start = std::time::Instant::now();
            let dirty = scene
                .apply(&klad_core::ops::parse_lines(&lines).unwrap(), &brushes)
                .unwrap();
            let applied = start.elapsed().as_secs_f64() * 1000.0;
            scene.compose(&dirty, None).unwrap();
            println!(
                "{what}: applied {applied:.0} ms, visible after {:.0} ms ({} tiles)",
                start.elapsed().as_secs_f64() * 1000.0,
                dirty.len()
            );
        };
        [0usize, 250, 500, 750, 990].iter().for_each(|&i| {
            let line = format!(r##"{{"op":"set","id":"c1:s{}","color":"#c03020"}}"##, ids[i]);
            timed(&mut scene, line, &format!("recolour stroke {}", i + 1));
        });
        let moved: Vec<String> = ids[940..990].iter().map(|id| format!("\"c1:s{id}\"")).collect();
        let line = format!(r#"{{"op":"transform","ids":[{}],"x":40,"y":25}}"#, moved.join(","));
        timed(&mut scene, line, "move 50 strokes");
        let start = std::time::Instant::now();
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap();
        println!("undo the move: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stroke_edits_from_either_side_replay_in_place_and_share_history() {
        let brushes = BrushSet::presets();
        let dir = bundle("strokes", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![1, 0]).unwrap();
        let ids: Vec<u32> = scene.active_marks().iter().map(Mark::id).collect();
        let cel = scene.active_cel().unwrap();
        let edit = format!(
            r##"{{"op":"set","id":"{cel}:s{}","color":"#ff0000","size":30}}
{{"op":"transform","ids":["{cel}:s{}","{cel}:s{}"],"x":40,"rotate":10}}
{{"op":"rm","id":"{cel}:s{}"}}"##,
            ids[3], ids[10], ids[11], ids[20]
        );
        let dirty = scene
            .apply(&klad_core::ops::parse_lines(&edit).unwrap(), &brushes)
            .unwrap();
        assert!(!dirty.is_empty());
        let expected = |scene: &Scene| {
            klad_brush::replay(600, 300, scene.active_marks(), &brushes)
                .unwrap()
                .raster
                .hash()
        };
        assert_eq!(scene.active_marks().len(), ids.len() - 1);
        assert_eq!(scene.rasters[&cel].hash(), expected(&scene));
        agrees(&scene);
        let mut outside = Bundle::open(&dir).unwrap();
        let line = format!(r#"{{"op":"set","id":"{cel}:s{}","opacity":0.4}}"#, ids[5]);
        outside.apply(&klad_core::ops::parse_lines(&line).unwrap()).unwrap();
        scene.reload(&brushes).unwrap();
        assert_eq!(scene.rasters[&cel].hash(), expected(&scene));
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap().unwrap();
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap().unwrap();
        assert_eq!(scene.active_marks().len(), ids.len());
        assert_eq!(scene.rasters[&cel].hash(), expected(&scene));
        agrees(&scene);
    }

    #[test]
    fn clips_and_masks_compose_right_from_every_active_layer_and_preview_too() {
        let brushes = BrushSet::presets();
        let dir = bundle("clipmask", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        scene.tint_mask = false;
        let ops = klad_core::ops::parse_lines(
            r#"{"op":"set","id":"b","clip":true,"blend":"multiply"}
{"op":"add","mask":{"id":"m"},"in":"a"}
{"op":"stroke","layer":"m","path":"M0 150 L600 150","size":60}"#,
        )
        .unwrap();
        scene.apply(&ops, &brushes).unwrap();
        assert!(
            scene
                .cels
                .contains_key(&scene.bundle.doc.layer("m").unwrap().cels()[0].id)
        );
        assert_eq!(
            rows(&scene)
                .iter()
                .map(|r| (r.id.as_str(), r.depth, r.mask, r.clip))
                .collect::<Vec<_>>(),
            [
                ("b", 0, false, true),
                ("g", 0, false, false),
                ("a", 1, false, false),
                ("m", 2, true, false),
                ("v", 0, false, false)
            ]
        );
        [vec![2], vec![1, 0], vec![1, 0, MASK], vec![0]]
            .into_iter()
            .for_each(|path| {
                scene.set_active(path.clone()).unwrap();
                assert!(!scene.fast, "{path:?}");
                agrees(&scene);
            });
        let unclip = klad_core::ops::parse_lines(r#"{"op":"set","id":"b","clip":false}"#).unwrap();
        scene.apply(&unclip, &brushes).unwrap();
        scene.set_active(vec![2]).unwrap();
        assert!(scene.fast);
        agrees(&scene);
        let reclip = klad_core::ops::parse_lines(r#"{"op":"set","id":"b","clip":true}"#).unwrap();
        scene.apply(&reclip, &brushes).unwrap();
        assert!(!scene.fast);
        agrees(&scene);
        let moved =
            klad_core::ops::parse_lines(r#"{"op":"set","id":"n1","transform.x":-60,"transform.y":70}"#).unwrap();
        preview_agrees(&mut scene, moved, &[], &brushes);
        let painted = scene.bundle.doc.layer("a").unwrap().cels()[0].id.clone();
        let ids: Vec<String> = scene.cels[&painted]
            .marks()
            .iter()
            .map(|m| format!("{painted}:s{}", m.id()))
            .collect();
        let shifted = vec![Op::Transform {
            ids,
            transform: klad_core::markedit::MarkTransform {
                x: 40.0,
                y: -25.0,
                ..Default::default()
            },
        }];
        scene.set_active(vec![1, 0]).unwrap();
        preview_agrees(&mut scene, shifted, &[painted], &brushes);
    }

    #[test]
    fn painting_on_a_mask_hides_the_layer_live_and_after_commit() {
        let brushes = BrushSet::presets();
        let dir = bundle("maskpaint", Blend::Normal);
        let mut scene = Scene::open(&dir).unwrap();
        scene.tint_mask = false;
        let add = klad_core::ops::parse_lines(
            r#"{"op":"add","mask":{"id":"m"},"in":"v"}
{"op":"set","id":"g","visible":false}
{"op":"set","id":"b","visible":false}"#,
        )
        .unwrap();
        scene.apply(&add, &brushes).unwrap();
        scene.set_active(vec![0, MASK]).unwrap();
        assert!(scene.can_paint());
        let before = tile_rgb(&scene, 250, 120);
        assert_eq!(before, [0x33, 0x66, 0xaa]);
        let stroke = Stroke::new(scene.next_mark_id(), "ink", [0.0, 0.0, 0.0, 1.0], 80.0, 3)
            .with_points(&[(60.0, 120.0, 1.0), (440.0, 120.0, 1.0)], 5.0);
        let mut buffer = StrokeBuffer::new(600, 300);
        let mut live = StrokePainter::new(brushes.get("ink").unwrap(), &stroke);
        stroke.samples.iter().for_each(|s| live.push(s, &mut buffer));
        let index = 0;
        let preview = Preview {
            twin: None,
            buffer: &buffer,
            color: stroke.color(),
            opacity: 1.0,
            blend: StrokeBlend::Normal,
        };
        let shown = &scene.compose(&[index], Some(&preview)).unwrap()[0].1;
        let at = (120 * 256 + 250) * 4;
        let live_rgb = [0, 1, 2].map(|c| (shown[at + c].to_f32() / shown[at + 3].to_f32() * 255.0).round() as u8);
        assert_ne!(live_rgb, before, "the live stroke on the mask changes nothing");
        scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
        scene.save_active("stroke").unwrap();
        let after = tile_rgb(&scene, 250, 120);
        assert_ne!(after, before, "the committed mask stroke changes nothing");
        assert_eq!(after, live_rgb);
        agrees(&scene);
        let mut reopened = Scene::open(&dir).unwrap();
        reopened.tint_mask = false;
        assert_eq!(tile_rgb(&reopened, 250, 120), after);
        reopened.set_active(vec![0, MASK]).unwrap();
        let red = tile_rgb(&reopened, 250, 120);
        reopened.tint_mask = true;
        let tinted = tile_rgb(&reopened, 250, 120);
        assert!(
            tinted[1] + 60 < red[1] && tinted[0] > tinted[1] + 60,
            "{tinted:?} vs {red:?}"
        );
        let clipped = klad_core::ops::parse_lines(
            r#"{"op":"set","id":"g","visible":true}
{"op":"set","id":"b","visible":true,"clip":true}
{"op":"add","mask":{"id":"mb"},"in":"b"}
{"op":"add","mask":{"id":"mg"},"in":"g"}"#,
        )
        .unwrap();
        scene.apply(&clipped, &brushes).unwrap();
        [vec![2, MASK], vec![1, MASK]].into_iter().for_each(|path| {
            scene.set_active(path.clone()).unwrap();
            let all = scene.all_tiles();
            let before = scene.compose(&all, None).unwrap();
            let stroke = Stroke::new(scene.next_mark_id(), "ink", [0.0, 0.0, 0.0, 1.0], 120.0, 5)
                .with_points(&[(20.0, 150.0, 1.0), (580.0, 150.0, 1.0)], 5.0);
            let mut buffer = StrokeBuffer::new(600, 300);
            let mut live = StrokePainter::new(brushes.get("ink").unwrap(), &stroke);
            stroke.samples.iter().for_each(|s| live.push(s, &mut buffer));
            scene.commit_stroke(stroke, &mut buffer, StrokeBlend::Normal).unwrap();
            scene.save_active("stroke").unwrap();
            let after = scene.compose(&all, None).unwrap();
            let changed = before
                .iter()
                .zip(&after)
                .filter(|((_, a), (_, b))| a.iter().zip(b.iter()).any(|(x, y)| x != y))
                .count();
            assert!(changed > 0, "painting on the mask at {path:?} changed no tile");
            agrees(&scene);
        });
    }

    fn stress(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-scene-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(2048, 2048);
        doc.canvas.background = Some("#f4efe6".into());
        doc.timeline.frames = 48;
        doc.timeline.fps = 24;
        let ids: Vec<String> = (0..4).map(|i| format!("l{i}")).collect();
        doc.layers = ids
            .iter()
            .map(|id| {
                let mut layer = Layer::paint(id, id, &format!("{id}c0"));
                *layer.cels_mut().unwrap() = (0..24).map(|k| Cel::at(&format!("{id}c{}", k * 2), k * 2)).collect();
                layer
            })
            .collect();
        let b = Bundle::create(&dir, doc).unwrap();
        let synth = Synth::canvas(2048.0, 2048.0);
        ids.iter().enumerate().for_each(|(i, id)| {
            (0..24).for_each(|k| {
                let strokes = synth.strokes(60, (i * 100 + k) as u64);
                b.write_marks(id, &format!("{id}c{}", k * 2), &marks(strokes)).unwrap();
            })
        });
        dir
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app frame_timing -- --ignored --nocapture"]
    fn frame_timing() {
        let brushes = BrushSet::presets();
        let dir = stress("frames-timing");
        let start = std::time::Instant::now();
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![3]).unwrap();
        println!("open: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        let all = scene.all_tiles();
        let ms = |s: std::time::Instant| s.elapsed().as_secs_f64() * 1000.0;
        let mut first = Vec::new();
        (1..48).for_each(|f| {
            let start = std::time::Instant::now();
            let job = scene.frame_job(f, &brushes);
            let loaded = job.map(|j| j.run().unwrap());
            let load = ms(start);
            let start = std::time::Instant::now();
            if let Some(l) = loaded {
                scene.take_loaded(l.paint, l.vector);
            }
            scene.set_frame(f, &brushes).unwrap();
            scene.compose(&all, None).unwrap();
            if f % 2 == 0 {
                first.push((load, ms(start)));
            }
        });
        let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let loads: Vec<f64> = first.iter().map(|(l, _)| *l).collect();
        let shows: Vec<f64> = first.iter().map(|(_, s)| *s).collect();
        println!(
            "first visit: load {:.0} ms avg (max {:.0}), then show {:.0} ms avg (max {:.0})",
            avg(&loads),
            loads.iter().copied().fold(0.0, f64::max),
            avg(&shows),
            shows.iter().copied().fold(0.0, f64::max)
        );
        println!(
            "memory after one pass: {} MB ({} drawings cached)",
            scene.cache_bytes() >> 20,
            scene.cels.len()
        );
        let steps: Vec<f64> = (0..3)
            .flat_map(|_| (0..48).rev().collect::<Vec<u32>>())
            .map(|f| {
                let start = std::time::Instant::now();
                scene.set_frame(f, &brushes).unwrap();
                scene.compose(&all, None).unwrap();
                ms(start)
            })
            .collect();
        let mut sorted = steps.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "step back and forth (set frame, caches, compose all {} tiles): median {:.0} ms, p90 {:.0} ms, max {:.0} ms",
            all.len(),
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 9 / 10],
            sorted[sorted.len() - 1]
        );
        println!(
            "memory: {} MB ({} drawings cached)",
            scene.cache_bytes() >> 20,
            scene.cels.len()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn percentile(values: &[f64], at: f64) -> f64 {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[((sorted.len() - 1) as f64 * at).round() as usize]
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app playback_timing -- --ignored --nocapture"]
    fn playback_timing() {
        use crate::frames::{FrameCache, Player};
        use std::time::{Duration, Instant};
        let brushes = BrushSet::presets();
        let dir = stress("playback-timing");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![3]).unwrap();
        let mut cache = FrameCache::new(std::sync::Arc::new(|| {}));
        let tick = Duration::from_micros(8333);
        let fps = scene.bundle.doc.timeline.fps;
        let loops = 4u32;
        let start = Instant::now();
        let mut player = Player::new(start, 0, (0, 47), fps, false);
        let mut ui = Vec::new();
        while start.elapsed() < Duration::from_secs_f64((48 * loops) as f64 / fps as f64) {
            let begin = Instant::now();
            cache.collect(&mut scene);
            let now = Instant::now();
            player.tick(now, |f| {
                let key = cache.key(&mut scene, f, false, &brushes);
                cache.has(key)
            });
            player
                .ahead(now, 48)
                .into_iter()
                .try_for_each(|f| cache.request(&mut scene, f, false, &brushes, false).then_some(()));
            cache.take();
            cache.prune(&BTreeSet::new());
            ui.push(begin.elapsed().as_secs_f64() * 1000.0);
            std::thread::sleep(tick.saturating_sub(begin.elapsed()));
        }
        println!(
            "playback at {fps} fps, 48 frames (24 distinct), {loops} loops: dropped per loop {:?}",
            player.drops
        );
        println!(
            "UI thread per tick: median {:.2} ms, p99 {:.2} ms, max {:.1} ms",
            percentile(&ui, 0.5),
            percentile(&ui, 0.99),
            percentile(&ui, 1.0)
        );
        println!(
            "memory: frame cache {} frames, {} MB; drawings {} MB ({} cached)",
            cache.len(),
            cache.bytes() >> 20,
            scene.cache_bytes() >> 20,
            scene.cels.len()
        );
        let steps: Vec<f64> = (0..3)
            .flat_map(|_| (0..48u32).rev())
            .map(|f| {
                let begin = Instant::now();
                scene.move_to(f);
                let key = cache.key(&mut scene, f, false, &brushes);
                assert!(cache.has(key));
                cache.touch(key);
                begin.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        println!(
            "step through the cache (UI work before the GPU shows it): median {:.3} ms, max {:.3} ms",
            percentile(&steps, 0.5),
            percentile(&steps, 1.0)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app held_step_timing -- --ignored --nocapture"]
    fn held_step_timing() {
        use crate::frames::FrameCache;
        use std::time::{Duration, Instant};
        let brushes = BrushSet::presets();
        let dir = stress("held-step-timing");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![3]).unwrap();
        let mut cache = FrameCache::new(std::sync::Arc::new(|| {}));
        let repeat = Duration::from_millis(33);
        let start = Instant::now();
        let mut next_repeat = start + repeat;
        let mut stepped = start;
        let mut shown: Vec<(u32, f64)> = vec![(0, 0.0)];
        let mut ignored = 0;
        while scene.frame < 47 || shown.last().map(|s| s.0) != Some(47) {
            cache.collect(&mut scene);
            let now = Instant::now();
            let frame = scene.frame;
            let key = cache.key(&mut scene, frame, false, &brushes);
            let on_screen = shown.last().map(|s| s.0) == Some(frame);
            if !on_screen && cache.has(key) {
                shown.push((frame, now.duration_since(stepped).as_secs_f64() * 1000.0));
            } else if !on_screen {
                cache.request(&mut scene, frame, false, &brushes, true);
            }
            (1..=4).filter(|k| frame + k < 48).for_each(|k| {
                cache.request(&mut scene, frame + k, false, &brushes, false);
            });
            if now >= next_repeat {
                next_repeat += repeat;
                match shown.last().map(|s| s.0) == Some(scene.frame) {
                    true => {
                        scene.move_to(scene.frame + 1);
                        stepped = now;
                    }
                    false => ignored += 1,
                }
            }
            cache.take();
            std::thread::sleep(Duration::from_micros(8333));
            assert!(start.elapsed() < Duration::from_secs(60));
        }
        let frames: Vec<u32> = shown.iter().map(|s| s.0).collect();
        assert_eq!(
            frames,
            (0..48).collect::<Vec<_>>(),
            "every frame shows once, none skipped"
        );
        let waits: Vec<f64> = shown.iter().skip(1).map(|s| s.1).collect();
        println!(
            "held . from a cold start, key repeat every 33 ms: all 48 frames shown in {:.2} s, step to screen median {:.0} ms, p90 {:.0} ms, max {:.0} ms, {ignored} repeats ignored while a frame was loading",
            start.elapsed().as_secs_f64(),
            percentile(&waits, 0.5),
            percentile(&waits, 0.9),
            percentile(&waits, 1.0)
        );
        println!(
            "drawings kept: {} ({} MB), frames cached: {} ({} MB)",
            scene.cels.len(),
            scene.cache_bytes() >> 20,
            cache.len(),
            cache.bytes() >> 20
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app onion_dab_timing -- --ignored --nocapture"]
    fn onion_dab_timing() {
        use std::time::Instant;
        let brushes = BrushSet::presets();
        let dir = stress("onion-dab-timing");
        let mut scene = Scene::open(&dir).unwrap();
        scene.set_active(vec![3]).unwrap();
        scene.set_frame(10, &brushes).unwrap();
        let points: Vec<(f32, f32, f32)> = (0..240)
            .map(|i| {
                let t = i as f32 / 239.0;
                (200.0 + 1600.0 * t, 1024.0 + 500.0 * (t * 9.0).sin(), 0.4 + 0.6 * t)
            })
            .collect();
        [false, true].iter().for_each(|&onion| {
            scene.onion_on = onion;
            let start = Instant::now();
            scene.refresh_onions(&brushes).unwrap();
            let loaded = start.elapsed().as_secs_f64() * 1000.0;
            scene.onion_on = false;
            scene.refresh_onions(&brushes).unwrap();
            scene.onion_on = onion;
            let start = Instant::now();
            scene.refresh_onions(&brushes).unwrap();
            let onions = start.elapsed().as_secs_f64() * 1000.0;
            let brush = brushes.get("ink").unwrap();
            let stroke = Stroke::new(999, "ink", [0.1, 0.1, 0.1, 1.0], 24.0, 3).with_points(&points, 4.0);
            let mut buffer = StrokeBuffer::new(2048, 2048);
            let mut live = StrokePainter::live(brush, &stroke);
            let times: Vec<f64> = stroke
                .samples
                .iter()
                .map(|sample| {
                    let begin = Instant::now();
                    live.push(sample, &mut buffer);
                    let tiles = buffer.take_dirty();
                    let preview = Preview {
                        twin: None,
                        buffer: &buffer,
                        color: stroke.color(),
                        opacity: 1.0,
                        blend: StrokeBlend::Normal,
                    };
                    scene.compose(&tiles, Some(&preview)).unwrap();
                    begin.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            println!(
                "onion {}: {} ghosts loaded and built in {loaded:.0} ms, built from loaded drawings in {onions:.0} ms; dab to composed tiles per sample: median {:.2} ms, p90 {:.2} ms, max {:.2} ms ({} samples, fast path {})",
                if onion { "on (2 before, 2 after)" } else { "off" },
                scene.onion_plan(10).iter().map(|(_, g)| g.len()).sum::<usize>(),
                percentile(&times, 0.5),
                percentile(&times, 0.9),
                percentile(&times, 1.0),
                times.len(),
                scene.fast
            );
        });
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-app batch_four_timing -- --ignored --nocapture"]
    fn batch_four_timing() {
        let brushes = BrushSet::presets();
        let dir = std::env::temp_dir().join(format!("draw-scene-b4-timing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = Document::new(2048, 2048);
        doc.canvas.background = Some("#f4efe6".into());
        let flats = Layer::vector(
            "flats",
            "flats",
            "cf",
            vec![
                Node::new(
                    "n1",
                    Shape::Ellipse {
                        cx: 1024.0,
                        cy: 1024.0,
                        rx: 700.0,
                        ry: 600.0,
                    },
                )
                .filled("#d0a060"),
            ],
        );
        let mut shade = Layer::paint("shade", "shade", "cs");
        shade.clip = true;
        shade.blend = Blend::Multiply;
        doc.layers = vec![flats, shade, Layer::paint("ink", "ink", "c1")];
        let b = Bundle::create(&dir, doc).unwrap();
        let mut synth = Synth::canvas(2048.0, 2048.0);
        synth.brushes = vec!["ink".into(), "pencil".into(), "marker".into()];
        b.write_marks("ink", "c1", &marks(synth.strokes(1000, 3))).unwrap();
        b.write_marks("shade", "cs", &marks(synth.strokes(200, 4))).unwrap();
        let mut scene = Scene::open(&dir).unwrap();
        let all = scene.all_tiles();
        [vec![2], vec![1]].into_iter().for_each(|path| {
            scene.set_active(path.clone()).unwrap();
            let start = std::time::Instant::now();
            scene.compose(&all[..16], None).unwrap();
            println!(
                "compose 16 tiles with {} active ({}): {:.1} ms",
                scene.active_id().unwrap(),
                if scene.fast {
                    "cached below and above"
                } else {
                    "whole stack"
                },
                start.elapsed().as_secs_f64() * 1000.0
            );
        });
        scene.set_active(vec![2]).unwrap();
        let line = r#"{"op":"lasso","layer":"ink","polygon":[[600,600],[1400,560],[1450,1300],[640,1380]],"cut":true,"x":40,"y":-25}"#;
        let start = std::time::Instant::now();
        let dirty = scene
            .apply(&klad_core::ops::parse_lines(line).unwrap(), &brushes)
            .unwrap();
        let applied = start.elapsed().as_secs_f64() * 1000.0;
        scene.compose(&dirty, None).unwrap();
        println!(
            "cut lasso and move in a 1,000-stroke cel: applied {applied:.0} ms, visible after {:.0} ms ({} tiles)",
            start.elapsed().as_secs_f64() * 1000.0,
            dirty.len()
        );
        let start = std::time::Instant::now();
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap();
        println!("undo the cut: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
        let small = r#"{"op":"lasso","layer":"ink","polygon":[[900,900],[1200,880],[1220,1180],[920,1200]],"cut":true,"x":40,"y":-25}"#;
        let start = std::time::Instant::now();
        let applied = scene
            .apply(&klad_core::ops::parse_lines(small).unwrap(), &brushes)
            .unwrap();
        println!(
            "a 300 px cut lasso: {:.0} ms ({} tiles)",
            start.elapsed().as_secs_f64() * 1000.0,
            applied.len()
        );
        scene.step(klad_core::history::Stack::Undo, &brushes).unwrap();
        let whole = line.replace(r#""cut":true,"#, "");
        let start = std::time::Instant::now();
        scene
            .apply(&klad_core::ops::parse_lines(&whole).unwrap(), &brushes)
            .unwrap();
        println!(
            "the same lasso without cut (whole strokes only): {:.0} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
