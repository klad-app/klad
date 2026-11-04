use anyhow::{Result, bail};
use egui::{Color32, ColorImage};
use klad_brush::Raster;
use klad_brush::cache::{compress, prune};
use klad_core::bundle::write_atomic;
use klad_core::rng::fnv;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const BUDGET: u64 = 1 << 30;
const VERSION: u64 = 1;

pub type Thumbs = Vec<(String, ColorImage)>;

pub struct Stored {
    pub below: Raster,
    pub above: Raster,
    pub thumbs: Thumbs,
}

pub fn key(base: u64, cels: &[u64]) -> u64 {
    cels.iter()
        .fold(fnv(base, &VERSION.to_le_bytes()), |h, k| fnv(h, &k.to_le_bytes()))
}

pub fn path(dir: &Path, key: u64) -> PathBuf {
    dir.join(format!("{key:016x}.planes"))
}

fn section(out: &mut Vec<u8>, bytes: Vec<u8>) {
    out.extend((bytes.len() as u64).to_le_bytes());
    out.extend(bytes);
}

fn thumbs_bytes(thumbs: &Thumbs) -> Vec<u8> {
    thumbs.iter().fold(Vec::new(), |mut out, (cel, image)| {
        out.extend((cel.len() as u32).to_le_bytes());
        out.extend(cel.as_bytes());
        out.extend((image.size[0] as u32).to_le_bytes());
        out.extend((image.size[1] as u32).to_le_bytes());
        out.extend(image.pixels.iter().flat_map(|c| c.to_array()));
        out
    })
}

fn read_thumbs(mut bytes: &[u8]) -> Result<Thumbs> {
    let mut out = Thumbs::new();
    let take = |n: usize, bytes: &mut &[u8]| -> Result<Vec<u8>> {
        if bytes.len() < n {
            bail!("the thumbnails are cut short");
        }
        let (head, rest) = bytes.split_at(n);
        *bytes = rest;
        Ok(head.to_vec())
    };
    let word = |b: Vec<u8>| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    while !bytes.is_empty() {
        let n = word(take(4, &mut bytes)?);
        let cel = String::from_utf8(take(n, &mut bytes)?)?;
        let (w, h) = (word(take(4, &mut bytes)?), word(take(4, &mut bytes)?));
        let pixels = take(w * h * 4, &mut bytes)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        out.push((cel, ColorImage::new([w, h], pixels)));
    }
    Ok(out)
}

pub fn write(dir: &Path, key: u64, below: &Raster, above: &Raster, thumbs: &Thumbs) -> Result<()> {
    let mut bytes = key.to_le_bytes().to_vec();
    section(&mut bytes, compress(below)?);
    section(&mut bytes, compress(above)?);
    section(&mut bytes, zstd::encode_all(&thumbs_bytes(thumbs)[..], 1)?);
    write_atomic(&path(dir, key), &bytes)?;
    prune(dir, BUDGET);
    Ok(())
}

pub fn read(dir: &Path, key: u64) -> Result<Stored> {
    let file = path(dir, key);
    let bytes = fs::read(&file)?;
    if bytes.len() < 8 || bytes[..8] != key.to_le_bytes() {
        bail!("stale planes");
    }
    let mut rest = &bytes[8..];
    let mut next = || -> Result<Vec<u8>> {
        if rest.len() < 8 {
            bail!("the planes are cut short");
        }
        let n = u64::from_le_bytes(rest[..8].try_into()?) as usize;
        if rest.len() < 8 + n {
            bail!("the planes are cut short");
        }
        let part = zstd::decode_all(&rest[8..8 + n])?;
        rest = &rest[8 + n..];
        Ok(part)
    };
    let below = Raster::decode(&next()?)?;
    let above = Raster::decode(&next()?)?;
    let thumbs = read_thumbs(&next()?)?;
    let _ = fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|f| f.set_modified(SystemTime::now()));
    Ok(Stored { below, above, thumbs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;
    use klad_brush::tiles::TILE_PX;

    #[test]
    fn planes_read_back_as_written() {
        let dir = std::env::temp_dir().join(format!("draw-planes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut below = Raster::new(600, 300);
        below.put(1, vec![f16::from_f32(0.25); TILE_PX * 4].into_boxed_slice());
        let above = Raster::new(600, 300);
        let thumbs = vec![(
            "c4".to_string(),
            ColorImage::new([2, 1], vec![Color32::from_rgb(10, 20, 30), Color32::TRANSPARENT]),
        )];
        write(&dir, 77, &below, &above, &thumbs).unwrap();
        let back = read(&dir, 77).unwrap();
        assert_eq!(back.below.at(1), below.at(1));
        assert_eq!(back.above.count(), 0);
        assert_eq!(back.thumbs[0].0, "c4");
        assert_eq!(back.thumbs[0].1.pixels, thumbs[0].1.pixels);
        assert!(read(&dir, 78).is_err());
        assert_ne!(key(1, &[2, 3]), key(1, &[3, 2]));
        let _ = fs::remove_dir_all(&dir);
    }
}
