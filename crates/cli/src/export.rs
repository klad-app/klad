use crate::layers;
use anyhow::{Result, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_render::export::{Options, Target, png8, png16, render};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Export {
    pub layer: Option<String>,
    pub frame: u32,
    pub scale: f32,
    pub depth: u8,
    pub background: bool,
    pub references: bool,
    pub variant: Option<String>,
    pub out: Option<PathBuf>,
}

pub(crate) fn scale_tag(scale: f32) -> String {
    if (scale - 1.0).abs() < f32::EPSILON {
        return String::new();
    }
    let text = format!("{scale:.3}");
    format!("@{}x", text.trim_end_matches('0').trim_end_matches('.'))
}

pub fn default_path(bundle: &Bundle, layer: Option<&str>, frame: u32, scale: f32, variant: Option<&str>) -> PathBuf {
    let root = bundle.root.components().collect::<PathBuf>();
    let stem = root
        .file_stem()
        .map_or("export".into(), |s| s.to_string_lossy().to_string());
    let layer = layer.map_or(String::new(), |l| format!("-{l}"));
    let frame = if bundle.doc.timeline.frames > 1 {
        format!("-f{frame}")
    } else {
        String::new()
    };
    root.parent().unwrap_or(Path::new(".")).join(format!(
        "{stem}{}{layer}{frame}{}.png",
        layers::variant_tag(variant),
        scale_tag(scale)
    ))
}

pub fn run(path: &Path, opts: Export) -> Result<()> {
    let start = Instant::now();
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    if opts.frame >= doc.timeline.frames {
        bail!(
            "frame {} is past the last frame {}",
            opts.frame,
            doc.timeline.frames - 1
        );
    }
    let layer = opts.layer.as_deref().map(|key| doc.find_layer(key)).transpose()?;
    klad_render::export::check_scale(doc, opts.scale)?;
    let detail = klad_render::export::detail_of(doc, opts.scale);
    let brushes = BrushSet::for_bundle(&bundle)?;
    let dress = layers::dress_of(doc, opts.variant.as_deref())?;
    let rasters = layers::dressed(
        &bundle,
        &brushes,
        layers::load_at(&bundle, opts.frame, &brushes, detail)?,
        &dress,
    )?;
    let target = match layer {
        Some(l) => Target::Layer(l),
        None => Target::Frame,
    };
    let options = Options {
        scale: opts.scale,
        background: opts.background,
        references: opts.references,
        detail,
    };
    let image = render(doc, opts.frame, &rasters, &target, options)?;
    let rendered = start.elapsed().as_secs_f64() * 1000.0;
    let bytes = match opts.depth {
        8 => png8(&image)?,
        16 => png16(&image)?,
        other => bail!("--depth takes 8 or 16, not {other}"),
    };
    let name = layer.map(|l| {
        if l.name.is_empty() {
            l.id.as_str()
        } else {
            l.name.as_str()
        }
    });
    let out = opts
        .out
        .unwrap_or_else(|| default_path(&bundle, name, opts.frame, opts.scale, opts.variant.as_deref()));
    write_atomic(&out, &bytes)?;
    println!("{}", out.display());
    eprintln!(
        "{}x{}  {}-bit{}  {} KB  rendered {rendered:.0} ms, {:.0} ms in all",
        image.width(),
        image.height(),
        opts.depth,
        if opts.depth == 8 { " dithered" } else { "" },
        bytes.len() / 1024,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Document, Layer};

    #[test]
    fn exports_land_next_to_the_bundle_with_layer_frame_and_scale_in_the_name() {
        let dir = std::env::temp_dir().join(format!("draw-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(64, 32);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let bundle = Bundle::create(dir.join("art.klad/"), doc).unwrap();
        assert_eq!(default_path(&bundle, None, 0, 1.0, None), dir.join("art.png"));
        assert_eq!(
            default_path(&bundle, Some("ink"), 0, 2.0, Some("Blue team")),
            dir.join("art-Blue_team-ink@2x.png")
        );
        assert_eq!(
            default_path(&bundle, None, 0, 0.5, Some("base")),
            dir.join("art@0.5x.png")
        );
        let out = dir.join("out/ink.png");
        run(
            &dir.join("art.klad"),
            Export {
                layer: Some("ink".into()),
                frame: 0,
                scale: 4.0,
                depth: 16,
                background: false,
                references: false,
                variant: None,
                out: Some(out.clone()),
            },
        )
        .unwrap();
        let png = std::fs::read(&out).unwrap();
        assert_eq!(
            (png[24], u32::from_be_bytes(png[16..20].try_into().unwrap())),
            (16, 256)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_export_above_full_size_draws_the_strokes_again_at_that_size() {
        let dir = std::env::temp_dir().join(format!("draw-export-sharp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(120, 60);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let mut bundle = Bundle::create(dir.join("art.klad/"), doc).unwrap();
        bundle
            .apply(
                &klad_core::ops::parse_lines(
                    r##"{"op":"stroke","layer":"l1","brush":"ink","size":6,"color":"#101010","path":"M10 20 L110 26","taper":[0,0]}"##,
                )
                .unwrap(),
            )
            .unwrap();
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let edge = |detail: f32| {
            let rasters = layers::load_at(&bundle, 0, &brushes, detail).unwrap();
            let options = Options {
                scale: 4.0,
                background: false,
                references: false,
                detail,
            };
            let image = render(&bundle.doc, 0, &rasters, &Target::Frame, options).unwrap();
            let frame = klad_render::read_rgba8(&image).unwrap();
            (40..140)
                .filter(|&y| (12..243).contains(&frame.pixel(240, y)[3]))
                .count()
        };
        let (stretched, drawn) = (edge(1.0), edge(4.0));
        assert!(
            drawn <= 4 && stretched >= drawn + 3,
            "soft pixels across the line: stretched {stretched}, drawn {drawn}"
        );
        assert_eq!(klad_render::export::detail_for(4.0), 4.0);
        assert_eq!(klad_render::export::detail_for(0.5), 1.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pixel_drawing_exports_its_palette_colours_exactly_and_stays_hard_when_larger() {
        use klad_core::bundle::Blank;
        let dir = std::env::temp_dir().join(format!("draw-export-pixel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut bundle = Bundle::blank(
            dir.join("art.klad"),
            &Blank {
                width: 16,
                height: 17,
                pixel: true,
                ..Blank::default()
            },
            "cli",
        )
        .unwrap();
        let layer = bundle.doc.layers[0].id.clone();
        let colour = |i: u32| [i as u8, (255 - i) as u8, ((i * 7) % 256) as u8];
        let lines: Vec<String> = (0..256u32)
            .map(|i| {
                let [r, g, b] = colour(i);
                format!(
                    r##"{{"op":"pixel","layer":"{layer}","color":"#{r:02x}{g:02x}{b:02x}","cells":[[{},{}]]}}"##,
                    i % 16,
                    i / 16
                )
            })
            .collect();
        bundle
            .apply(&klad_core::ops::parse_lines(&lines.join("\n")).unwrap())
            .unwrap();
        assert_eq!(bundle.doc.palette.len(), 256);
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let pixels = |scale: f32| {
            let detail = klad_render::export::detail_of(&bundle.doc, scale);
            assert_eq!(detail, 1.0, "a pixel drawing is drawn at its real size, then enlarged");
            let rasters = layers::load_at(&bundle, 0, &brushes, detail).unwrap();
            let options = Options {
                scale,
                background: false,
                references: false,
                detail,
            };
            let image = render(&bundle.doc, 0, &rasters, &Target::Frame, options).unwrap();
            (image.width() as usize, klad_render::export::dither8(&image).unwrap())
        };
        let (w, flat) = pixels(1.0);
        (0..256usize).for_each(|i| {
            let at = ((i / 16) * w + i % 16) * 4;
            let [r, g, b] = colour(i as u32);
            assert_eq!(flat[at..at + 4], [r, g, b, 255], "colour {i} at real size");
        });
        assert!(flat[16 * w * 4..].iter().all(|v| *v == 0), "the empty row stays empty");
        let (w, big) = pixels(3.0);
        (0..256usize).for_each(|i| {
            let [r, g, b] = colour(i as u32);
            (0..9).for_each(|k| {
                let at = (((i / 16) * 3 + k / 3) * w + (i % 16) * 3 + k % 3) * 4;
                assert_eq!(big[at..at + 4], [r, g, b, 255], "colour {i} at 3x, block pixel {k}");
            });
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pixel_drawing_is_enlarged_pixel_for_pixel() {
        use klad_core::bundle::Blank;
        let dir = std::env::temp_dir().join(format!("draw-export-pixel-sharp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut bundle = Bundle::blank(
            dir.join("art.klad"),
            &Blank {
                width: 6,
                height: 4,
                pixel: true,
                ..Blank::default()
            },
            "cli",
        )
        .unwrap();
        let layer = bundle.doc.layers[0].id.clone();
        bundle
            .apply(
                &klad_core::ops::parse_lines(&format!(
                    r##"{{"op":"pixel","layer":"{layer}","color":"#ef233c","cells":[[1,1],[3,2],[4,2]]}}"##
                ))
                .unwrap(),
            )
            .unwrap();
        let doc = &bundle.doc;
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let rasters = layers::load_at(&bundle, 0, &brushes, 1.0).unwrap();
        let shown = |scale: f32| {
            let options = Options {
                scale,
                background: false,
                references: false,
                detail: klad_render::export::detail_of(doc, scale),
            };
            let image = render(doc, 0, &rasters, &Target::Frame, options).unwrap();
            (image.width() as usize, klad_render::export::dither8(&image).unwrap())
        };
        let (_, real) = shown(1.0);
        let (wide, big) = shown(5.0);
        assert_eq!(wide, 30);
        (0..20usize).for_each(|y| {
            (0..30usize).for_each(|x| {
                let from = ((y / 5) * 6 + x / 5) * 4;
                let at = (y * 30 + x) * 4;
                assert_eq!(
                    big[at..at + 4],
                    real[from..from + 4],
                    "pixel {x},{y} of the enlarged picture"
                );
            });
        });
        assert_eq!(real[(6 + 1) * 4..(6 + 1) * 4 + 4], [0xef, 0x23, 0x3c, 255]);
        let whole = |scale: f32| klad_render::export::check_scale(doc, scale);
        assert!(whole(64.0).is_ok() && whole(1.0).is_ok());
        assert!(whole(1.5).is_err() && whole(0.5).is_err() && whole(65.0).is_err());
        let mut wide_doc = doc.clone();
        wide_doc.canvas.width = 1024;
        assert_eq!(
            klad_render::export::pixel_most(&wide_doc),
            16.0,
            "no side goes past 16384 px"
        );
        let mut painting = doc.clone();
        painting.layers.iter_mut().for_each(|l| {
            if let klad_core::doc::LayerContent::Paint { pixel, .. } = &mut l.content {
                *pixel = false;
            }
        });
        assert!(!painting.pixel_art());
        assert!(klad_render::export::check_scale(&painting, 2.5).is_ok());
        assert!(klad_render::export::check_scale(&painting, 9.0).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_keyed_pixel_layer_moves_by_whole_pixels_and_stays_hard() {
        use klad_core::bundle::Blank;
        let dir = std::env::temp_dir().join(format!("draw-export-pixel-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut bundle = Bundle::blank(
            dir.join("art.klad"),
            &Blank {
                width: 12,
                height: 4,
                pixel: true,
                frames: 5,
                ..Blank::default()
            },
            "cli",
        )
        .unwrap();
        let layer = bundle.doc.layers[0].id.clone();
        bundle
            .apply(
                &klad_core::ops::parse_lines(&format!(
                    "{}\n{}\n{}",
                    format_args!(r##"{{"op":"pixel","layer":"{layer}","color":"#ef233c","cells":[[1,1],[2,1]]}}"##),
                    format_args!(r##"{{"op":"key","id":"{layer}","frame":0,"x":0}}"##),
                    format_args!(r##"{{"op":"key","id":"{layer}","frame":4,"x":6}}"##),
                ))
                .unwrap(),
            )
            .unwrap();
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let reds = |frame: u32| -> Vec<usize> {
            let rasters = layers::load_at(&bundle, frame, &brushes, 1.0).unwrap();
            let options = Options {
                scale: 1.0,
                background: false,
                references: false,
                detail: 1.0,
            };
            let image = render(&bundle.doc, frame, &rasters, &Target::Frame, options).unwrap();
            let pixels = klad_render::export::dither8(&image).unwrap();
            (0..12)
                .filter(|x| {
                    let at = (12 + x) * 4;
                    let px = &pixels[at..at + 4];
                    assert!(
                        px == [0xef, 0x23, 0x3c, 255] || px == [0, 0, 0, 0],
                        "frame {frame}, pixel {x} is {px:?}: neither the colour nor empty"
                    );
                    px[3] == 255
                })
                .collect()
        };
        assert_eq!(reds(0), [1, 2]);
        assert_eq!(reds(1), [3, 4], "a quarter of the way is 1.5 pixels: it lands on 2");
        assert_eq!(reds(2), [4, 5]);
        assert_eq!(reds(4), [7, 8]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
