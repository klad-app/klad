use crate::{Ctx, Rasters, background, draw_clipped_alone, draw_layer, draw_layers, f16_info};
use anyhow::{Result, anyhow, bail};
use klad_core::doc::{Blend, Document, Layer};
use rayon::prelude::*;
use skia_safe::{
    AlphaType, ColorSpace, ColorType, CubicResampler, FilterMode, Image, ImageInfo, MipmapMode, SamplingOptions,
    image::CachingHint, surfaces,
};

pub const SNAP: f32 = 0.13;
pub const MAX_SCALE: f32 = 8.0;
pub const PIXEL_MAX_SCALE: f32 = 64.0;
pub const PIXEL_MAX_SIDE: f32 = 16384.0;

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
    pub detail: f32,
}

pub fn detail_for(scale: f32) -> f32 {
    match scale > 1.0 {
        true => scale,
        false => 1.0,
    }
}

pub fn detail_of(doc: &Document, scale: f32) -> f32 {
    match doc.pixel_art() {
        true => 1.0,
        false => detail_for(scale),
    }
}

pub fn pixel_most(doc: &Document) -> f32 {
    let longest = doc.canvas.width.max(doc.canvas.height) as f32;
    PIXEL_MAX_SCALE.min((PIXEL_MAX_SIDE / longest).floor()).max(1.0)
}

pub fn check_scale(doc: &Document, scale: f32) -> Result<()> {
    if !doc.pixel_art() {
        if !(1.0 / 16.0..=MAX_SCALE).contains(&scale) {
            bail!("export scale {scale} is outside 1/16..{MAX_SCALE}");
        }
        return Ok(());
    }
    let most = pixel_most(doc);
    if scale.fract() != 0.0 || scale < 1.0 {
        bail!(
            "a pixel drawing exports at whole-number scales (1, 2, 3 up to {most}), so every pixel stays a square: not {scale}"
        );
    }
    if scale > most {
        bail!(
            "export scale {scale} is past {most} for this {}x{} pixel drawing",
            doc.canvas.width,
            doc.canvas.height
        );
    }
    Ok(())
}

pub fn render(doc: &Document, frame: u32, rasters: &Rasters, target: &Target, options: Options) -> Result<Image> {
    let scale = options.scale;
    check_scale(doc, scale)?;
    if doc.pixel_art() && scale > 1.0 {
        let real = Options {
            scale: 1.0,
            detail: 1.0,
            ..options
        };
        return crate::resample(&render(doc, frame, rasters, target, real)?, scale);
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
        detail: options.detail,
        ..Ctx::full(frame, rasters)
    };
    match target {
        Target::Frame => draw_layers(canvas, &doc.layers, ctx)?,
        Target::Layer(layer) => {
            let mut alone = (*layer).clone();
            alone.visible = true;
            alone.blend = Blend::Normal;
            match clip_base(&doc.layers, &layer.id).filter(|_| layer.clip) {
                Some(base) => {
                    alone.clip = false;
                    draw_clipped_alone(canvas, base, &alone, ctx)?
                }
                None => draw_layer(canvas, &alone, ctx)?,
            }
        }
    }
    Ok(surface.image_snapshot())
}

fn clip_base<'a>(layers: &'a [Layer], id: &str) -> Option<&'a Layer> {
    match layers.iter().position(|l| l.id == id) {
        Some(at) => layers[..at].iter().rev().find(|l| !l.clip),
        None => layers.iter().find_map(|l| clip_base(l.children(), id)),
    }
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
    Ok(crate::pngwrite::write(w, h, 8, &dither8(image)?))
}

pub fn png16(image: &Image) -> Result<Vec<u8>> {
    let (w, h) = (image.width() as u32, image.height() as u32);
    Ok(png_of_f16(w, h, &f16_pixels(image, AlphaType::Unpremul)?))
}

pub(crate) fn png_of_f16(width: u32, height: u32, pixels: &[half::f16]) -> Vec<u8> {
    let wide: Vec<u8> = pixels
        .par_iter()
        .flat_map_iter(|v| ((v.to_f32().clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes())
        .collect();
    crate::pngwrite::write(width, height, 16, &wide)
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
            detail: 1.0,
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
            detail: 1.0,
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
                detail: 1.0,
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
                    detail: 1.0,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_clipped_layer_exported_alone_keeps_its_clip() {
        let rect = |id: &str, x: f32, w: f32, colour: &str| {
            Node::new(
                id,
                Shape::Rect {
                    x,
                    y: 0.0,
                    w,
                    h: 32.0,
                    radius: 0.0,
                },
            )
            .filled(colour)
        };
        let mut doc = Document::new(64, 32);
        doc.layers.push(Layer::vector(
            "base",
            "base",
            "c1",
            vec![rect("n1", 0.0, 32.0, "#ff0000")],
        ));
        let mut shade = Layer::vector("shade", "shade", "c2", vec![rect("n2", 0.0, 64.0, "#0000ff")]);
        shade.clip = true;
        doc.layers.push(shade);
        let options = Options {
            scale: 1.0,
            background: false,
            references: false,
            detail: 1.0,
        };
        let layer = doc.layers[1].clone();
        let image = render(&doc, 0, &Rasters::new(), &Target::Layer(&layer), options).unwrap();
        let frame = crate::read_rgba8(&image).unwrap();
        assert_eq!(
            frame.pixel(16, 16),
            [0, 0, 255, 255],
            "inside the base: the layer's own colour"
        );
        assert_eq!(frame.pixel(48, 16)[3], 0, "outside the base: clipped away");
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    #[ignore = "timing: PICTURE=<png> cargo test --release -p klad-render png_timing -- --ignored --nocapture"]
    fn png_timing() {
        let path = std::env::var("PICTURE").unwrap();
        let rgba = crate::assets::decode(&std::fs::read(&path).unwrap()).unwrap();
        let info = ImageInfo::new(
            (rgba.width as i32, rgba.height as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            ColorSpace::new_srgb(),
        );
        let image = skia_safe::images::raster_from_data(
            &info,
            skia_safe::Data::new_copy(&rgba.pixels),
            rgba.width as usize * 4,
        )
        .unwrap();
        let time = |f: &dyn Fn() -> usize| {
            let t = std::time::Instant::now();
            let n = f();
            (t.elapsed().as_secs_f64() * 1000.0, n)
        };
        let (dither, _) = time(&|| dither8(&image).unwrap().len());
        let skia = |pixmap: &skia_safe::Pixmap| {
            let mut out = Vec::new();
            skia_safe::png_encoder::encode(pixmap, &mut out, &skia_safe::png_encoder::Options::default());
            out.len()
        };
        let (before, before_bytes) = time(&|| {
            let mut rgba = dither8(&image).unwrap();
            let (w, h) = (image.width() as u32, image.height() as u32);
            let pixmap = skia_safe::Pixmap::new(&crate::rgba8_info(w, h), &mut rgba, w as usize * 4).unwrap();
            skia(&pixmap)
        });
        let same = {
            let ours = png8(&image).unwrap();
            let mut reader = png::Decoder::new(std::io::Cursor::new(&ours[..])).read_info().unwrap();
            let mut buf = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut buf).unwrap();
            buf == dither8(&image).unwrap()
        };
        println!(
            "skia png8 {before:.0} ms ({} KB); ours decodes to the same pixels: {same}",
            before_bytes / 1024
        );
        let (eight, bytes8) = time(&|| png8(&image).unwrap().len());
        let (sixteen, bytes16) = time(&|| png16(&image).unwrap().len());
        println!(
            "{}x{}: dither {dither:.0} ms, png8 {eight:.0} ms ({} KB), png16 {sixteen:.0} ms ({} KB)",
            image.width(),
            image.height(),
            bytes8 / 1024,
            bytes16 / 1024
        );
    }
}
