use crate::{Ctx, Rasters, background, draw_layer, draw_layers, f16_info, rgba8_info};
use anyhow::{Result, anyhow, bail};
use klad_core::doc::{Blend, Document, Layer};
use rayon::prelude::*;
use skia_safe::{
    AlphaType, ColorSpace, ColorType, CubicResampler, FilterMode, Image, ImageInfo, MipmapMode, Pixmap,
    SamplingOptions, image::CachingHint, png_encoder, surfaces,
};

pub const SNAP: f32 = 0.13;
pub const MAX_SCALE: f32 = 8.0;

const BAYER: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60, 28, 52, 20, 62, 30,
    54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23,
    61, 29, 53, 21,
];

pub enum Target<'a> {
    Frame,
    Layer(&'a Layer),
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub scale: f32,
    pub background: bool,
    pub references: bool,
}

pub fn render(doc: &Document, frame: u32, rasters: &Rasters, target: &Target, options: Options) -> Result<Image> {
    let scale = options.scale;
    if !(1.0 / 16.0..=MAX_SCALE).contains(&scale) {
        bail!("export scale {scale} is outside 1/16..{MAX_SCALE}");
    }
    let (w, h) = (
        ((doc.canvas.width as f32 * scale).round() as u32).max(1),
        ((doc.canvas.height as f32 * scale).round() as u32).max(1),
    );
    let mut surface =
        surfaces::raster(&f16_info(w, h), None, None).ok_or_else(|| anyhow!("cannot allocate {w}x{h}"))?;
    let canvas = surface.canvas();
    let fill = match (options.background, target) {
        (true, Target::Frame) => background(doc.canvas.background.as_deref())?,
        _ => background(None)?,
    };
    canvas.clear(fill);
    canvas.scale((scale, scale));
    let sampling = if scale > 1.0 {
        Some(SamplingOptions::from(CubicResampler::mitchell()))
    } else if scale < 1.0 {
        Some(SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear))
    } else {
        None
    };
    let ctx = Ctx {
        sampling,
        doc: Some(doc),
        references: options.references,
        ..Ctx::full(frame, rasters)
    };
    match target {
        Target::Frame => draw_layers(canvas, &doc.layers, ctx)?,
        Target::Layer(layer) => {
            let mut alone = (*layer).clone();
            alone.visible = true;
            alone.blend = Blend::Normal;
            draw_layer(canvas, &alone, ctx)?;
        }
    }
    Ok(surface.image_snapshot())
}

pub(crate) fn f16_pixels(image: &Image, alpha: AlphaType) -> Result<Vec<half::f16>> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBAF16, alpha, ColorSpace::new_srgb());
    let mut pixels = vec![half::f16::ZERO; w * h * 4];
    if !image.read_pixels(
        &info,
        bytemuck::cast_slice_mut::<half::f16, u8>(&mut pixels),
        w * 8,
        (0, 0),
        CachingHint::Allow,
    ) {
        return Err(anyhow!("cannot read pixels"));
    }
    Ok(pixels)
}

pub fn quantize(value: f32, threshold: f32) -> u8 {
    let x = value.clamp(0.0, 1.0) * 255.0;
    let near = x.round();
    if (x - near).abs() <= SNAP {
        return near as u8;
    }
    (x + threshold).floor().clamp(0.0, 255.0) as u8
}

pub fn dither8(image: &Image) -> Result<Vec<u8>> {
    let w = image.width() as usize;
    let pixels = f16_pixels(image, AlphaType::Premul)?;
    let mut out = vec![0u8; pixels.len()];
    out.par_chunks_mut(w * 4)
        .zip(pixels.par_chunks(w * 4))
        .enumerate()
        .for_each(|(y, (row, source))| {
            row.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source.as_chunks::<4>().0)
                .enumerate()
                .for_each(|(x, (px, from))| {
                    let t = (BAYER[(y % 8) * 8 + x % 8] as f32 + 0.5) / 64.0;
                    let a = from[3].to_f32();
                    if a <= 0.0 {
                        return;
                    }
                    let alpha = quantize(a, t);
                    if alpha > 0 {
                        let [r, g, b] = [0, 1, 2].map(|c| quantize(from[c].to_f32() / a, t));
                        *px = [r, g, b, alpha];
                    }
                });
        });
    Ok(out)
}

pub fn png8(image: &Image) -> Result<Vec<u8>> {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let mut rgba = dither8(image)?;
    let pixmap = Pixmap::new(&rgba8_info(w, h), &mut rgba, w as usize * 4).ok_or_else(|| anyhow!("bad pixmap"))?;
    encode(&pixmap)
}

pub fn png16(image: &Image) -> Result<Vec<u8>> {
    let (w, h) = (image.width(), image.height());
    let mut pixels = f16_pixels(image, AlphaType::Unpremul)?;
    let info = ImageInfo::new((w, h), ColorType::RGBAF16, AlphaType::Unpremul, ColorSpace::new_srgb());
    let pixmap = Pixmap::new(
        &info,
        bytemuck::cast_slice_mut::<half::f16, u8>(&mut pixels),
        w as usize * 8,
    )
    .ok_or_else(|| anyhow!("bad pixmap"))?;
    encode(&pixmap)
}

pub(crate) fn encode(pixmap: &Pixmap) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if !png_encoder::encode(pixmap, &mut out, &png_encoder::Options::default()) {
        bail!("PNG encoding failed");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Linear, Node, Paint, Shape, Stop};

    fn ramp(width: u32, from: &str, to: &str) -> Document {
        let mut doc = Document::new(width, 64);
        let mut bar = Node::new(
            "n1",
            Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: width as f32,
                h: 64.0,
                radius: 0.0,
            },
        );
        bar.fill = Some(Paint::Linear {
            linear: Linear {
                from: [0.0, 0.0],
                to: [width as f32, 0.0],
                stops: vec![Stop(0.0, from.into()), Stop(1.0, to.into())],
            },
        });
        doc.layers.push(Layer::vector("v", "v", "c1", vec![bar]));
        doc
    }

    fn block_means(rgba: &[u8], w: usize, h: usize) -> Vec<f32> {
        (0..w / 8)
            .map(|b| {
                let sum: f32 = (0..h)
                    .flat_map(|y| (b * 8..b * 8 + 8).map(move |x| rgba[(y * w + x) * 4] as f32))
                    .sum();
                sum / (8 * h) as f32
            })
            .collect()
    }

    #[test]
    fn dithering_turns_bands_into_a_smooth_ramp() {
        let doc = ramp(1024, "#202020", "#282828");
        let options = Options {
            scale: 1.0,
            background: false,
            references: false,
        };
        let image = render(&doc, 0, &Rasters::new(), &Target::Frame, options).unwrap();
        let dithered = dither8(&image).unwrap();
        let ideal = |b: usize| 32.0 + 8.0 * (b as f32 * 8.0 + 4.0) / 1024.0;
        let drift = |means: &[f32]| {
            means
                .iter()
                .enumerate()
                .map(|(b, m)| (m - ideal(b)).abs())
                .fold(0.0f32, f32::max)
        };
        let worst = drift(&block_means(&dithered, 1024, 64));
        assert!(worst < 0.2, "dithered blocks drift {worst} levels from the ramp");
        let plain = crate::read_rgba8(&image).unwrap();
        let banded = block_means(&plain.rgba, 1024, 64);
        assert!(drift(&banded) > 0.4, "{}", drift(&banded));
        let png = png8(&image).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(png[24], 8);
    }

    #[test]
    fn flat_colours_stay_flat_and_transparency_stays_clean() {
        let mut doc = Document::new(64, 64);
        let square = Node::new(
            "n1",
            Shape::Rect {
                x: 8.0,
                y: 8.0,
                w: 32.0,
                h: 32.0,
                radius: 0.0,
            },
        )
        .filled("#f4efe6");
        doc.layers.push(Layer::vector("v", "v", "c1", vec![square]));
        doc.canvas.background = Some("#336699".into());
        let options = Options {
            scale: 1.0,
            background: false,
            references: false,
        };
        let image = render(&doc, 0, &Rasters::new(), &Target::Frame, options).unwrap();
        let rgba = dither8(&image).unwrap();
        let px = |x: usize, y: usize| [0, 1, 2, 3].map(|c| rgba[(y * 64 + x) * 4 + c]);
        assert!((8..40).all(|y| (8..40).all(|x| px(x, y) == [0xf4, 0xef, 0xe6, 255])));
        assert_eq!(px(50, 50), [0, 0, 0, 0]);
        let with_bg = render(
            &doc,
            0,
            &Rasters::new(),
            &Target::Frame,
            Options {
                background: true,
                ..options
            },
        )
        .unwrap();
        assert_eq!(
            &dither8(&with_bg).unwrap()[(50 * 64 + 50) * 4..(50 * 64 + 50) * 4 + 4],
            [0x33, 0x66, 0x99, 255]
        );
    }

    #[test]
    fn scaled_exports_keep_vectors_sharp_and_sixteen_bit_pngs_are_sixteen_bit() {
        let mut doc = Document::new(100, 50);
        let mut dot = Node::new(
            "n1",
            Shape::Ellipse {
                cx: 50.0,
                cy: 25.0,
                rx: 20.0,
                ry: 20.0,
            },
        )
        .filled("#000000");
        dot.opacity = 0.5;
        doc.layers.push(Layer::vector("v", "v", "c1", vec![dot]));
        let image = render(
            &doc,
            0,
            &Rasters::new(),
            &Target::Layer(&doc.layers[0]),
            Options {
                scale: 2.0,
                background: false,
                references: false,
            },
        )
        .unwrap();
        assert_eq!((image.width(), image.height()), (200, 100));
        let rgba = dither8(&image).unwrap();
        let alpha = |x: usize, y: usize| rgba[(y * 200 + x) * 4 + 3];
        assert!((127..=128).contains(&alpha(100, 50)));
        let edge: Vec<u8> = (136..146).map(|x| alpha(x, 50)).collect();
        assert!(edge.iter().filter(|a| (5..120).contains(*a)).count() <= 2, "{edge:?}");
        let png = png16(&image).unwrap();
        assert_eq!(png[24], 16, "bit depth");
        assert!(
            render(
                &doc,
                0,
                &Rasters::new(),
                &Target::Frame,
                Options {
                    scale: 20.0,
                    background: false,
                    references: false,
                }
            )
            .is_err()
        );
    }
}
