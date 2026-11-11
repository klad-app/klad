use crate::layers;
use anyhow::{Result, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_render::export::{Options, Target, detail_for, png8, png16, render};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Export {
    pub layer: Option<String>,
    pub frame: u32,
    pub scale: f32,
    pub depth: u8,
    pub background: bool,
    pub references: bool,
    pub out: Option<PathBuf>,
}

pub(crate) fn scale_tag(scale: f32) -> String {
    if (scale - 1.0).abs() < f32::EPSILON {
        return String::new();
    }
    let text = format!("{scale:.3}");
    format!("@{}x", text.trim_end_matches('0').trim_end_matches('.'))
}

pub fn default_path(bundle: &Bundle, layer: Option<&str>, frame: u32, scale: f32) -> PathBuf {
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
    root.parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem}{layer}{frame}{}.png", scale_tag(scale)))
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
    let detail = detail_for(opts.scale);
    let rasters = layers::load_at(&bundle, opts.frame, &BrushSet::for_bundle(&bundle)?, detail)?;
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
        .unwrap_or_else(|| default_path(&bundle, name, opts.frame, opts.scale));
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
        assert_eq!(default_path(&bundle, None, 0, 1.0), dir.join("art.png"));
        assert_eq!(default_path(&bundle, Some("ink"), 0, 2.0), dir.join("art-ink@2x.png"));
        assert_eq!(default_path(&bundle, None, 0, 0.5), dir.join("art@0.5x.png"));
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
        assert_eq!(detail_for(4.0), 4.0);
        assert_eq!(detail_for(0.5), 1.0);
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
            let detail = detail_for(scale);
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
}
