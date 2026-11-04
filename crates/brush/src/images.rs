use crate::brush::{TipMask, smoothstep};
use anyhow::{Result, bail};
use klad_core::bundle::Bundle;
use klad_core::rng::{FNV_OFFSET, fnv, hash2_unit};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const TIP_SIZE: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
    pub hash: u64,
}

pub type Pictures = BTreeMap<String, Arc<Picture>>;

pub fn decode(bytes: &[u8]) -> Result<Picture> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let Some(size) = reader.output_buffer_size() else {
        bail!("the picture is too big");
    };
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf)?;
    let (width, height) = (info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    let pixels: Vec<&[u8]> = buf[..width * height * channels].chunks_exact(channels).collect();
    let alpha = |px: &[u8]| -> Option<u8> {
        match channels {
            2 => Some(px[1]),
            4 => Some(px[3]),
            _ => None,
        }
    };
    let see_through = pixels.iter().any(|px| alpha(px).is_some_and(|a| a < 255));
    let luminance = |px: &[u8]| -> f32 {
        match channels {
            1 | 2 => px[0] as f32 / 255.0,
            _ => (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0,
        }
    };
    let data = pixels
        .iter()
        .map(|px| match see_through {
            true => alpha(px).unwrap_or(255) as f32 / 255.0,
            false => 1.0 - luminance(px),
        })
        .collect();
    Ok(Picture {
        width,
        height,
        data,
        hash: fnv(FNV_OFFSET, bytes),
    })
}

pub fn load(bundle: &Bundle) -> Result<Pictures> {
    let dir = bundle.root.join("brushes").join("images");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Pictures::new());
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let picture = decode(&std::fs::read(&path)?).map_err(|e| anyhow::anyhow!("{}: {e:#}", path.display()))?;
            Ok((name, Arc::new(picture)))
        })
        .collect()
}

pub fn tip_from(picture: &Picture) -> TipMask {
    let side = picture.width.max(picture.height) as f32;
    let (ox, oy) = (
        (side - picture.width as f32) / 2.0,
        (side - picture.height as f32) / 2.0,
    );
    let data = (0..TIP_SIZE * TIP_SIZE)
        .map(|i| {
            let (x, y) = (i % TIP_SIZE, i / TIP_SIZE);
            let sx = (x as f32 + 0.5) / TIP_SIZE as f32 * side - ox;
            let sy = (y as f32 + 0.5) / TIP_SIZE as f32 * side - oy;
            if sx < 0.0 || sy < 0.0 || sx >= picture.width as f32 || sy >= picture.height as f32 {
                return 0.0;
            }
            picture.data[sy as usize * picture.width + sx as usize]
        })
        .collect();
    TipMask { size: TIP_SIZE, data }
}

fn disc(u: f32, v: f32, soft: f32) -> f32 {
    1.0 - smoothstep(1.0 - soft, 1.0, libm::sqrtf(u * u + v * v))
}

fn bristles_flat(u: f32, v: f32) -> f32 {
    let envelope = 1.0 - smoothstep(0.22, 0.45, u.abs());
    let edge = 1.0 - smoothstep(0.88, 1.0, v.abs());
    let lines = (0..18).fold(0.0f32, |most, k| {
        let at = -0.9 + 1.8 * k as f32 / 17.0 + (hash2_unit(k, 1, 31) - 0.5) * 0.06;
        let width = 0.025 + hash2_unit(k, 2, 31) * 0.035;
        let strength = 0.55 + hash2_unit(k, 3, 31) * 0.45;
        let d = (v - at).abs() / width;
        most.max(strength * (1.0 - smoothstep(0.4, 1.0, d)))
    });
    envelope * edge * lines
}

fn bristles_round(u: f32, v: f32) -> f32 {
    let body = disc(u, v, 0.25);
    if body <= 0.0 {
        return 0.0;
    }
    let spots = (0..70).fold(0.0f32, |most, k| {
        let angle = hash2_unit(k, 1, 47) * std::f32::consts::TAU;
        let r = libm::sqrtf(hash2_unit(k, 2, 47)) * 0.92;
        let (cx, cy) = (r * libm::cosf(angle), r * libm::sinf(angle));
        let size = 0.07 + hash2_unit(k, 3, 47) * 0.08;
        let d = libm::sqrtf((u - cx) * (u - cx) + (v - cy) * (v - cy)) / size;
        most.max(1.0 - smoothstep(0.5, 1.0, d))
    });
    body * (0.25 + 0.75 * spots)
}

fn sponge(u: f32, v: f32) -> f32 {
    let body = disc(u, v, 0.3);
    if body <= 0.0 {
        return 0.0;
    }
    let cell = |scale: f32, seed: u32| {
        let (x, y) = ((u + 1.0) * scale, (v + 1.0) * scale);
        let (gx, gy) = (x.floor(), y.floor());
        let (fx, fy) = (x - gx, y - gy);
        let at = |dx: f32, dy: f32| hash2_unit((gx + dx) as i32, (gy + dy) as i32, seed);
        let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * smoothstep(0.0, 1.0, fx);
        let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * smoothstep(0.0, 1.0, fx);
        top + (bottom - top) * smoothstep(0.0, 1.0, fy)
    };
    let noise = cell(9.0, 61) * 0.65 + cell(23.0, 62) * 0.35;
    body * smoothstep(0.38, 0.62, noise)
}

fn leaf(u: f32, v: f32) -> f32 {
    if u.abs() >= 1.0 {
        return 0.0;
    }
    let half = 0.42 * libm::powf(1.0 - u * u, 0.8);
    let body = 1.0 - smoothstep(half * 0.85, half, v.abs());
    let rib = match u > -0.75 && u < 0.85 {
        true => 1.0 - 0.6 * (1.0 - smoothstep(0.015, 0.035, v.abs())),
        false => 1.0,
    };
    body * rib
}

fn grass(u: f32, v: f32) -> f32 {
    let height = (1.0 - v) / 2.0;
    (0..5).fold(0.0f32, |most, k| {
        let foot = -0.6 + 0.3 * k as f32 + (hash2_unit(k, 1, 73) - 0.5) * 0.12;
        let lean = (hash2_unit(k, 2, 73) - 0.5) * 0.7;
        let tall = 0.7 + hash2_unit(k, 3, 73) * 0.3;
        if height > tall {
            return most;
        }
        let centre = foot + lean * height * height;
        let width = 0.09 * (1.0 - height / tall) + 0.005;
        most.max(1.0 - smoothstep(0.6, 1.0, (u - centre).abs() / width))
    })
}

fn splatter(u: f32, v: f32) -> f32 {
    (0..16).fold(0.0f32, |most, k| {
        let angle = hash2_unit(k, 1, 89) * std::f32::consts::TAU;
        let r = libm::sqrtf(hash2_unit(k, 2, 89)) * 0.85;
        let (cx, cy) = (r * libm::cosf(angle), r * libm::sinf(angle));
        let size = 0.03 + libm::powf(hash2_unit(k, 3, 89), 3.0) * 0.2;
        let d = libm::sqrtf((u - cx) * (u - cx) + (v - cy) * (v - cy)) / size;
        most.max(1.0 - smoothstep(0.7, 1.0, d))
    })
}

pub fn library_tip(name: &str) -> Option<TipMask> {
    let shape: fn(f32, f32) -> f32 = match name {
        "bristle-flat" => bristles_flat,
        "bristle-round" => bristles_round,
        "sponge" => sponge,
        "leaf" => leaf,
        "grass" => grass,
        "splatter" => splatter,
        _ => return None,
    };
    let data = (0..TIP_SIZE * TIP_SIZE)
        .map(|i| {
            let (x, y) = (i % TIP_SIZE, i / TIP_SIZE);
            let u = (x as f32 + 0.5) / TIP_SIZE as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / TIP_SIZE as f32 * 2.0 - 1.0;
            shape(u, v).clamp(0.0, 1.0)
        })
        .collect();
    Some(TipMask { size: TIP_SIZE, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        out
    }

    #[test]
    fn library_tips_have_body_and_air() {
        klad_core::brushes::TIP_IMAGES.iter().for_each(|name| {
            let tip = library_tip(name).unwrap();
            let filled = tip.data.iter().filter(|v| **v > 0.5).count();
            assert!(
                filled > tip.data.len() / 200 && filled < tip.data.len() * 3 / 4,
                "{name}: {filled} of {}",
                tip.data.len()
            );
            assert_eq!(
                tip.sample(0.999, 0.999),
                library_tip(name).unwrap().sample(0.999, 0.999)
            );
        });
        assert!(library_tip("nope").is_none());
    }

    #[test]
    fn pictures_read_dark_as_paint_or_their_alpha() {
        let grey = png(2, 1, png::ColorType::Grayscale, &[0, 255]);
        assert_eq!(decode(&grey).unwrap().data, vec![1.0, 0.0]);
        let alpha = png(2, 1, png::ColorType::Rgba, &[0, 0, 0, 255, 255, 255, 255, 51]);
        assert_eq!(decode(&alpha).unwrap().data, vec![1.0, 0.2]);
        let tip = tip_from(&decode(&png(2, 1, png::ColorType::Grayscale, &[0, 255])).unwrap());
        assert!(tip.sample(-0.6, 0.0) > 0.9 && tip.sample(0.6, 0.0) < 0.1 && tip.sample(0.0, -0.9) == 0.0);
    }
}
