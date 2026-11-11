use crate::layers;
use crate::sprites::{bundle_dir, bundle_stem, file_safe};
use anyhow::{Context, Result, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Blank, Bundle};
use klad_core::color::parse_hex;
use klad_core::doc::{Document, Swatch};
use klad_core::ops::{ClipEdit, Op, PaletteEdit, PixelSpec};
use klad_render::export::{Options, Target, detail_of, dither8, render};
use klad_render::pixelise::{MOST_COLOURS, MOST_SIDE, Picture, colours, counted, decode, fitted, mapped, shrunk};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const UNASKED: usize = 32;
const FROM_PAINT: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct Ask {
    pub size: Option<u32>,
    pub colors: Option<usize>,
}

pub fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["png", "jpg", "jpeg"].iter().any(|known| e.eq_ignore_ascii_case(known)))
}

fn read(file: &Path) -> Result<Picture> {
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    decode(&bytes).with_context(|| file.display().to_string())
}

fn sized(picture: &Picture, ask: &Ask) -> Result<Picture> {
    let longest = picture.width.max(picture.height);
    let (width, height) = match ask.size {
        Some(0) => bail!("--size is the longest side in pixels, from 1 to {MOST_SIDE}"),
        Some(size) if size > MOST_SIDE => {
            bail!("--size {size} is past {MOST_SIDE}: a pixel drawing that large is a painting")
        }
        Some(size) => fitted(picture.width, picture.height, size),
        None if longest <= MOST_SIDE => (picture.width, picture.height),
        None => bail!(
            "the picture is {}x{}: give --size, its longest side in pixels once it is pixel art (at most {MOST_SIDE})",
            picture.width,
            picture.height
        ),
    };
    Ok(shrunk(picture, width, height))
}

fn checked(ask: &Ask) -> Result<()> {
    match ask.colors {
        Some(n) if !(2..=MOST_COLOURS).contains(&n) => bail!("--colors takes 2 to {MOST_COLOURS}, not {n}"),
        _ => Ok(()),
    }
}

fn hex(colour: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

fn own_palette(palette: &[Swatch]) -> Vec<(String, [u8; 3])> {
    palette
        .iter()
        .enumerate()
        .filter_map(|(index, swatch)| {
            let c = parse_hex(&swatch.color).ok()?;
            (c[3] >= 1.0).then(|| {
                let rgb = [c[0], c[1], c[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                (index.to_string(), rgb)
            })
        })
        .collect()
}

fn chosen(
    palette: &[Swatch],
    pictures: &[&Picture],
    ask: &Ask,
    unasked: usize,
    notes: &mut Vec<String>,
) -> Vec<(String, [u8; 3])> {
    let made = |count: usize| -> Vec<(String, [u8; 3])> {
        colours(pictures, count)
            .into_iter()
            .map(|colour| (hex(colour), colour))
            .collect()
    };
    let own = own_palette(palette);
    match (ask.colors, own.len() >= 2) {
        (Some(count), _) => made(count),
        (None, true) => {
            notes.push(format!(
                "every pixel took the nearest of the palette's {} colours (--colors N makes colours from the picture instead)",
                own.len()
            ));
            own
        }
        (None, false) => match counted(pictures).len() <= MOST_COLOURS {
            true => made(MOST_COLOURS),
            false => {
                notes.push(format!(
                    "the picture has more than {MOST_COLOURS} colours: it was reduced to {unasked} (--colors N for another number)"
                ));
                made(unasked)
            }
        },
    }
}

fn marks(
    layer: &str,
    frame: u32,
    at: [i64; 2],
    picture: &Picture,
    keys: &[(String, [u8; 3])],
    canvas: (u32, u32),
) -> Vec<Op> {
    let table: Vec<[u8; 3]> = keys.iter().map(|(_, colour)| *colour).collect();
    let mut by_colour: BTreeMap<usize, Vec<[i64; 2]>> = BTreeMap::new();
    mapped(picture, &table).into_iter().enumerate().for_each(|(i, index)| {
        let (x, y) = (
            at[0] + (i as u32 % picture.width) as i64,
            at[1] + (i as u32 / picture.width) as i64,
        );
        if let Some(index) = index
            && x >= 0
            && y >= 0
            && x < canvas.0 as i64
            && y < canvas.1 as i64
        {
            by_colour.entry(index).or_default().push([x, y]);
        }
    });
    by_colour
        .into_iter()
        .map(|(index, cells)| {
            Op::Pixel(PixelSpec {
                layer: layer.to_string(),
                frame,
                color: Some(keys[index].0.clone()),
                cells,
                ..PixelSpec::default()
            })
        })
        .collect()
}

pub fn placed(
    doc: &Document,
    file: &Path,
    layer: &str,
    frame: u32,
    at: [i64; 2],
    ask: &Ask,
) -> Result<(Vec<Op>, Vec<String>)> {
    checked(ask)?;
    let owner = doc.find_layer(layer)?;
    if !owner.pixel() {
        bail!(
            "{} is a {} layer: a picture's pixels go on a pixel layer (klad add <bundle> layer pixel)",
            owner.name,
            owner.kind_name()
        );
    }
    let picture = sized(&read(file)?, ask)?;
    let mut notes = Vec::new();
    let keys = chosen(&doc.palette, &[&picture], ask, UNASKED, &mut notes);
    let ops = marks(
        &owner.id,
        frame,
        at,
        &picture,
        &keys,
        (doc.canvas.width, doc.canvas.height),
    );
    if ops.is_empty() {
        bail!(
            "the picture has no solid pixels inside the canvas at {},{}",
            at[0],
            at[1]
        );
    }
    notes.insert(
        0,
        format!(
            "{}x{} pixels in {} on {}",
            picture.width,
            picture.height,
            match ops.len() {
                1 => "1 colour".to_string(),
                n => format!("{n} colours"),
            },
            owner.name
        ),
    );
    Ok((ops, notes))
}

fn fresh(target: &Path) -> Result<()> {
    match target.exists() {
        true => bail!(
            "{} is there already: give the drawing another name with -o",
            target.display()
        ),
        false => Ok(()),
    }
}

fn finished(mut bundle: Bundle, ops: &[Op], target: &Path, what: &str) -> Result<Bundle> {
    let applied = bundle.apply(ops).and_then(|_| bundle.stamp("cli"));
    match applied {
        Ok(_) => Ok(bundle),
        Err(error) => {
            let _ = fs::remove_dir_all(target);
            Err(error.context(what.to_string()))
        }
    }
}

pub fn import(file: &Path, ask: &Ask, out: Option<PathBuf>) -> Result<PathBuf> {
    checked(ask)?;
    let target = out.unwrap_or_else(|| file.with_extension("klad"));
    fresh(&target)?;
    let picture = sized(&read(file)?, ask)?;
    let mut notes = Vec::new();
    let keys = chosen(&[], &[&picture], ask, UNASKED, &mut notes);
    let bundle = Bundle::blank(
        &target,
        &Blank {
            width: picture.width,
            height: picture.height,
            pixel: true,
            ..Blank::default()
        },
        "cli",
    )?;
    let layer = bundle.doc.layers[0].id.clone();
    let ops = marks(&layer, 0, [0, 0], &picture, &keys, (picture.width, picture.height));
    let bundle = finished(bundle, &ops, &target, &format!("bringing in {}", file.display()))?;
    eprintln!(
        "{}x{}, {} palette colours",
        bundle.doc.canvas.width,
        bundle.doc.canvas.height,
        bundle.doc.palette.len()
    );
    notes.iter().for_each(|note| eprintln!("note: {note}"));
    Ok(target)
}

pub fn pixelize(path: &Path, layer: Option<&str>, ask: &Ask, out: Option<PathBuf>) -> Result<PathBuf> {
    checked(ask)?;
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let source = layer.map(|key| doc.find_layer(key)).transpose()?;
    let Some(size) = ask.size else {
        bail!("give --size: the longest side of the pixel drawing, in pixels (64 for a 64 px sprite)");
    };
    if size == 0 || size > MOST_SIDE {
        bail!("--size is the longest side in pixels, from 1 to {MOST_SIDE}");
    }
    let name = source.map(|l| match l.name.is_empty() {
        true => l.id.clone(),
        false => l.name.clone(),
    });
    let target = out.unwrap_or_else(|| {
        let part = name.as_ref().map_or(String::new(), |n| format!("-{}", file_safe(n)));
        bundle_dir(&bundle).join(format!("{}{part}-pixel.klad", bundle_stem(&bundle)))
    });
    fresh(&target)?;
    let (width, height) = (doc.canvas.width, doc.canvas.height);
    let (to_width, to_height) = fitted(width, height, size);
    let scale = match doc.pixel_art() {
        true => 1.0,
        false => (size as f32 / width.max(height) as f32).clamp(1.0 / 16.0, 1.0),
    };
    let detail = detail_of(doc, scale);
    let brushes = BrushSet::for_bundle(&bundle)?;
    let aim = source.map_or(Target::Frame, Target::Layer);
    let options = Options {
        scale,
        background: false,
        references: false,
        detail,
    };
    let frames: Vec<Picture> = (0..doc.timeline.frames)
        .map(|frame| {
            let rasters = layers::load_at(&bundle, frame, &brushes, detail)?;
            let image = render(doc, frame, &rasters, &aim, options)?;
            let shown = Picture {
                width: image.width() as u32,
                height: image.height() as u32,
                rgba: dither8(&image)?,
            };
            Ok(shrunk(&shown, to_width, to_height))
        })
        .collect::<Result<_>>()?;
    let all: Vec<&Picture> = frames.iter().collect();
    let mut notes = Vec::new();
    let keys = chosen(&doc.palette, &all, ask, FROM_PAINT, &mut notes);
    let made = Bundle::blank(
        &target,
        &Blank {
            width: to_width,
            height: to_height,
            fps: doc.timeline.fps,
            frames: doc.timeline.frames,
            pixel: true,
            ..Blank::default()
        },
        "cli",
    )?;
    let to = made.doc.layers[0].id.clone();
    let uses_own = ask.colors.is_none() && keys.iter().all(|(key, _)| !key.starts_with('#'));
    let palette = uses_own.then(|| {
        Op::Palette(PaletteEdit::Set {
            swatches: doc
                .palette
                .iter()
                .map(|s| Swatch {
                    id: String::new(),
                    ..s.clone()
                })
                .collect(),
        })
    });
    let named = Op::parse(json!({ "op": "set", "id": to, "name": name.clone().unwrap_or_else(|| "pixels".into()) }))?;
    let drawn: Vec<Op> = frames
        .iter()
        .enumerate()
        .filter(|(at, picture)| *at == 0 || frames[at - 1] != **picture)
        .map(|(at, picture)| {
            let cel = (at > 0)
                .then(|| Op::parse(json!({ "op": "add", "cel": { "frame": at }, "in": to })))
                .transpose()?;
            Ok(cel
                .into_iter()
                .chain(marks(&to, at as u32, [0, 0], picture, &keys, (to_width, to_height)))
                .collect::<Vec<_>>())
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let clips = doc
        .timeline
        .clips
        .iter()
        .map(|clip| Op::Clip(ClipEdit::Add(clip.clone())));
    let ops: Vec<Op> = palette.into_iter().chain([named]).chain(drawn).chain(clips).collect();
    let made = finished(made, &ops, &target, &format!("making pixels of {}", path.display()))?;
    let drawings = made.doc.layers[0].cels().len();
    eprintln!(
        "{to_width}x{to_height}, {} frames ({}), {} palette colours",
        made.doc.timeline.frames,
        match drawings {
            1 => "1 drawing".to_string(),
            n => format!("{n} drawings"),
        },
        made.doc.palette.len()
    );
    notes.iter().for_each(|note| eprintln!("note: {note}"));
    Ok(target)
}
