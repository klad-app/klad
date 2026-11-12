use anyhow::{Result, anyhow, bail};
use klad_core::color::to_oklab;
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo, image::CachingHint};
use std::collections::BTreeMap;

pub const SOLID: u8 = 128;
pub const MOST_SIDE: u32 = 512;
pub const MOST_COLOURS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode(bytes: &[u8]) -> Result<Picture> {
    let image = Image::from_encoded(Data::new_copy(bytes))
        .ok_or_else(|| anyhow!("this is not a picture klad can read: it reads PNG and JPEG files"))?;
    let (width, height) = (image.width() as u32, image.height() as u32);
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    if !image.read_pixels(&info, &mut rgba, width as usize * 4, (0, 0), CachingHint::Allow) {
        bail!("the picture's pixels cannot be read");
    }
    Ok(Picture { width, height, rgba })
}

pub fn fitted(width: u32, height: u32, longest: u32) -> (u32, u32) {
    let side = width.max(height).max(1) as f64;
    let scale = longest as f64 / side;
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

pub fn shrunk(picture: &Picture, width: u32, height: u32) -> Picture {
    if (width, height) == (picture.width, picture.height) {
        return picture.clone();
    }
    let (sw, sh) = (picture.width as usize, picture.height as usize);
    let edge = |at: u32, of: u32, source: usize| -> (usize, usize) {
        let from = at as usize * source / of as usize;
        let to = ((at as usize + 1) * source).div_ceil(of as usize);
        (from.min(source - 1), to.clamp(from + 1, source))
    };
    let rgba = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let ((x0, x1), (y0, y1)) = (edge(x, width, sw), edge(y, height, sh));
            let mut sums = [0u64; 4];
            (y0..y1).for_each(|sy| {
                (x0..x1).for_each(|sx| {
                    let px = &picture.rgba[(sy * sw + sx) * 4..][..4];
                    let alpha = px[3] as u64;
                    (0..3).for_each(|ch| sums[ch] += px[ch] as u64 * alpha);
                    sums[3] += alpha;
                });
            });
            let count = ((x1 - x0) * (y1 - y0)) as u64;
            match sums[3] {
                0 => [0, 0, 0, 0],
                alpha => [
                    ((sums[0] + alpha / 2) / alpha) as u8,
                    ((sums[1] + alpha / 2) / alpha) as u8,
                    ((sums[2] + alpha / 2) / alpha) as u8,
                    ((alpha + count / 2) / count) as u8,
                ],
            }
        })
        .collect();
    Picture { width, height, rgba }
}

pub fn counted(pictures: &[&Picture]) -> BTreeMap<[u8; 3], usize> {
    let mut seen: BTreeMap<[u8; 3], usize> = BTreeMap::new();
    pictures.iter().for_each(|picture| {
        picture
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] >= SOLID)
            .for_each(|px| *seen.entry([px[0], px[1], px[2]]).or_default() += 1);
    });
    seen
}

pub fn colours(pictures: &[&Picture], most: usize) -> Vec<[u8; 3]> {
    let counts = counted(pictures);
    match counts.len() <= most.max(1) {
        true => {
            let mut by_use: Vec<([u8; 3], usize)> = counts.into_iter().collect();
            by_use.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            by_use.into_iter().map(|(colour, _)| colour).collect()
        }
        false => {
            let mut cut = crate::gif::median_cut(&counts, most.max(1));
            cut.sort_by(|a, b| {
                let light =
                    |c: &[u8; 3]| to_oklab([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0])[0];
                light(a).total_cmp(&light(b)).then(a.cmp(b))
            });
            cut.dedup();
            cut
        }
    }
}

pub fn mapped(picture: &Picture, palette: &[[u8; 3]]) -> Vec<Option<usize>> {
    let lab = |c: [u8; 3]| to_oklab([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]);
    let labs: Vec<[f32; 3]> = palette.iter().map(|c| lab(*c)).collect();
    let mut found: BTreeMap<[u8; 3], usize> = palette.iter().enumerate().rev().map(|(i, c)| (*c, i)).collect();
    picture
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            if px[3] < SOLID || palette.is_empty() {
                return None;
            }
            let colour = [px[0], px[1], px[2]];
            Some(*found.entry(colour).or_insert_with(|| {
                let at = lab(colour);
                labs.iter()
                    .enumerate()
                    .min_by(|a, b| {
                        let far = |l: &[f32; 3]| (0..3).map(|i| (l[i] - at[i]).powi(2)).sum::<f32>();
                        far(a.1).total_cmp(&far(b.1))
                    })
                    .map_or(0, |(i, _)| i)
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, at: impl Fn(u32, u32) -> [u8; 4]) -> Picture {
        Picture {
            width,
            height,
            rgba: (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .flat_map(|(x, y)| at(x, y))
                .collect(),
        }
    }

    #[test]
    fn a_picture_enlarged_by_whole_numbers_shrinks_back_to_its_pixels() {
        let small = picture(5, 3, |x, y| match (x + y) % 3 {
            0 => [0, 0, 0, 0],
            1 => [239, 35, 60, 255],
            _ => [34, 34, 59, 255],
        });
        let big = picture(40, 24, |x, y| {
            let at = ((y / 8) * 5 + x / 8) as usize * 4;
            small.rgba[at..at + 4].try_into().unwrap()
        });
        assert_eq!(shrunk(&big, 5, 3), small);
        assert_eq!(shrunk(&small, 5, 3), small);
        assert_eq!(fitted(40, 24, 5), (5, 3));
        assert_eq!(fitted(1000, 10, 64), (64, 1));
        let half = shrunk(&picture(2, 1, |x, _| [200, 100, 0, if x == 0 { 255 } else { 0 }]), 1, 1);
        assert_eq!(
            half.rgba,
            [200, 100, 0, 128],
            "a clear pixel takes no part in the colour, only in the alpha"
        );
    }

    #[test]
    fn colours_are_the_pictures_own_when_few_and_cut_down_when_many() {
        let few = picture(4, 1, |x, _| match x {
            0 => [10, 10, 10, 255],
            1 | 2 => [200, 30, 30, 255],
            _ => [1, 2, 3, 40],
        });
        assert_eq!(
            colours(&[&few], 16),
            [[200, 30, 30], [10, 10, 10]],
            "most used first, clear pixels left out"
        );
        let many = picture(64, 64, |x, y| [(x * 4) as u8, (y * 4) as u8, 128, 255]);
        let cut = colours(&[&many], 8);
        assert!(cut.len() <= 8 && cut.len() >= 6, "{}", cut.len());
        let lights: Vec<f32> = cut
            .iter()
            .map(|c| to_oklab([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0])[0])
            .collect();
        assert!(lights.windows(2).all(|w| w[0] <= w[1]), "dark to light: {lights:?}");
    }

    #[test]
    fn every_solid_pixel_takes_the_nearest_palette_colour() {
        let palette = [[0, 0, 0], [255, 255, 255], [220, 40, 40], [40, 60, 200]];
        let shown = picture(5, 1, |x, _| match x {
            0 => [250, 245, 240, 255],
            1 => [200, 60, 50, 255],
            2 => [30, 30, 90, 200],
            3 => [20, 20, 20, 100],
            _ => [40, 60, 200, 255],
        });
        assert_eq!(mapped(&shown, &palette), [Some(1), Some(2), Some(3), None, Some(3)]);
        assert!(mapped(&shown, &[]).iter().all(Option::is_none));
    }

    #[test]
    fn a_png_is_read_with_its_alpha() {
        let image = {
            let info = ImageInfo::new((2, 1), ColorType::RGBA8888, AlphaType::Unpremul, None);
            let pixels = [255u8, 0, 0, 255, 0, 0, 255, 0];
            skia_safe::images::raster_from_data(&info, Data::new_copy(&pixels), 8).unwrap()
        };
        let png = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, None)
            .expect("Skia writes a PNG");
        let read = decode(png.as_bytes()).unwrap();
        assert_eq!((read.width, read.height), (2, 1));
        assert_eq!(read.rgba[..4], [255, 0, 0, 255]);
        assert_eq!(read.rgba[7], 0);
        assert!(
            decode(b"not a picture")
                .unwrap_err()
                .to_string()
                .contains("PNG and JPEG")
        );
    }
}
