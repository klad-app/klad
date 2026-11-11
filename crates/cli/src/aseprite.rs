use crate::layers::{dress_of, variant_tag};
use crate::sprites::{bundle_dir, bundle_stem};
use anyhow::{Context, Result, bail};
use klad_core::bundle::{Blank, Bundle, write_atomic};
use klad_core::color::parse_hex;
use klad_core::doc::{Blend, Clip, Document, Layer, Swatch};
use klad_core::ops::{ClipEdit, Op, PaletteEdit, PixelSpec};
use klad_render::aseprite::{Cel, Colour, Picture, Sprite, Tag};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const MOST_COLOURS: usize = 1024;
const MOST_FPS: u32 = 120;

const BLENDS: [(u16, Blend); 12] = [
    (0, Blend::Normal),
    (1, Blend::Multiply),
    (2, Blend::Screen),
    (3, Blend::Overlay),
    (4, Blend::Darken),
    (5, Blend::Lighten),
    (6, Blend::ColorDodge),
    (8, Blend::HardLight),
    (9, Blend::SoftLight),
    (14, Blend::Color),
    (15, Blend::Luminosity),
    (16, Blend::Add),
];

pub fn is_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("aseprite") || e.eq_ignore_ascii_case("ase"))
}

pub struct Timing {
    pub fps: u32,
    pub spans: Vec<u32>,
    pub exact: bool,
}

pub fn timing(durations: &[u16]) -> Timing {
    let spans_at = |fps: u32| -> Vec<u32> {
        durations
            .iter()
            .map(|d| ((*d as f64 * fps as f64 / 1000.0).round() as u32).max(1))
            .collect()
    };
    let off = |fps: u32| -> f64 {
        spans_at(fps)
            .iter()
            .zip(durations)
            .map(|(span, d)| (*span as f64 * 1000.0 / fps as f64 - *d as f64).abs())
            .sum()
    };
    let fps = (1..=MOST_FPS)
        .min_by(|a, b| off(*a).total_cmp(&off(*b)).then(a.cmp(b)))
        .unwrap_or(12);
    Timing {
        fps,
        spans: spans_at(fps),
        exact: off(fps) < 0.5,
    }
}

fn hex_of(rgba: [u8; 4]) -> String {
    match rgba[3] {
        255 => format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]),
        a => format!("#{:02x}{:02x}{:02x}{a:02x}", rgba[0], rgba[1], rgba[2]),
    }
}

fn swatches(palette: &[Colour]) -> Vec<Swatch> {
    let mut names = BTreeSet::new();
    palette
        .iter()
        .map(|colour| {
            let name = colour.name.trim().trim_start_matches('#').to_string();
            Swatch {
                color: hex_of(colour.rgba),
                name: match name.is_empty() || !names.insert(name.clone()) {
                    true => String::new(),
                    false => name,
                },
                id: String::new(),
                ramp: String::new(),
            }
        })
        .collect()
}

struct Placed {
    id: String,
    cels: BTreeMap<u16, String>,
}

fn real_cel(sprite: &Sprite, layer: u16, frame: u16) -> Option<&Cel> {
    let mut at = frame;
    (0..sprite.durations.len()).find_map(|_| {
        let cel = sprite.cels.iter().find(|c| c.layer == layer && c.frame == at)?;
        match &cel.picture {
            Picture::Link(to) if *to != at => {
                at = *to;
                None
            }
            Picture::Link(_) => None,
            Picture::Pixels { .. } => Some(cel),
        }
    })
}

fn layer_json(
    sprite: &Sprite,
    at: usize,
    starts: &[u32],
    spans: &[u32],
    ids: &mut impl FnMut(char) -> String,
    placed: &mut BTreeMap<u16, Placed>,
    notes: &mut BTreeSet<String>,
) -> (Value, usize) {
    let layer = &sprite.layers[at];
    let id = ids('l');
    let blend = BLENDS.iter().find(|(n, _)| *n == layer.blend).map(|(_, b)| *b);
    if blend.is_none() {
        notes.insert(format!(
            "layer {} has a blend mode klad does not have: it blends as normal",
            layer.name
        ));
    }
    let mut value = json!({
        "id": id,
        "name": layer.name,
        "visible": layer.visible,
        "opacity": layer.opacity as f32 / 255.0,
        "blend": blend.unwrap_or_default(),
    });
    if layer.reference {
        value["reference"] = Value::from(true);
    }
    let mut next = at + 1;
    if layer.group {
        let mut children = Vec::new();
        while next < sprite.layers.len() && sprite.layers[next].level > layer.level {
            let (child, after) = layer_json(sprite, next, starts, spans, ids, placed, notes);
            if !sprite.layers[next].tilemap {
                children.push(child);
            }
            next = after;
        }
        value["kind"] = Value::from("group");
        value["children"] = Value::Array(children);
        return (value, next);
    }
    let index = at as u16;
    let frames: Vec<u16> = (0..sprite.durations.len() as u16)
        .filter(|f| sprite.cels.iter().any(|c| c.layer == index && c.frame == *f))
        .collect();
    let mut made: BTreeMap<u16, String> = BTreeMap::new();
    let cels: Vec<Value> = frames
        .iter()
        .map(|&frame| {
            let cel_id = ids('c');
            let own = sprite
                .cels
                .iter()
                .find(|c| c.layer == index && c.frame == frame)
                .is_some_and(|c| matches!(c.picture, Picture::Pixels { .. }));
            let source = real_cel(sprite, index, frame).map(|c| c.frame);
            let mut cel = json!({ "id": cel_id, "frame": starts[frame as usize] });
            match (own, source.and_then(|s| made.get(&s))) {
                (false, Some(link)) => cel["link"] = Value::from(link.clone()),
                _ => {
                    made.insert(frame, cel_id.clone());
                }
            }
            if !frames.contains(&(frame + 1)) && (frame as usize + 1) < sprite.durations.len() {
                cel["hold"] = Value::from(spans[frame as usize]);
            }
            cel
        })
        .collect();
    value["kind"] = Value::from("pixel");
    if !cels.is_empty() {
        value["cels"] = Value::Array(cels);
    }
    placed.insert(index, Placed { id, cels: made });
    (value, next)
}

fn clip_names(tags: &[Tag]) -> Vec<String> {
    let mut taken = BTreeSet::new();
    tags.iter()
        .enumerate()
        .map(|(i, tag)| {
            let wanted = match tag.name.trim() {
                "" => format!("tag {}", i + 1),
                name => name.to_string(),
            };
            let name = std::iter::once(wanted.clone())
                .chain((2..).map(|n| format!("{wanted} {n}")))
                .find(|n| !taken.contains(n))
                .unwrap_or(wanted);
            taken.insert(name.clone());
            name
        })
        .collect()
}

pub fn import(file: &Path, out: Option<PathBuf>) -> Result<PathBuf> {
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let sprite = klad_render::aseprite::read(&bytes).with_context(|| file.display().to_string())?;
    let target = out.unwrap_or_else(|| file.with_extension("klad"));
    if target.exists() {
        bail!(
            "{} is there already: give the drawing another name with -o",
            target.display()
        );
    }
    if sprite.durations.is_empty() {
        bail!("{} holds no frames", file.display());
    }
    let mut notes: BTreeSet<String> = sprite
        .notes
        .iter()
        .map(|n| format!("{n} are not carried over"))
        .collect();
    let timed = timing(&sprite.durations);
    if !timed.exact {
        notes.insert(format!(
            "frame times were rounded to whole frames at {} a second",
            timed.fps
        ));
    }
    let starts: Vec<u32> = timed
        .spans
        .iter()
        .scan(0u32, |at, span| {
            let start = *at;
            *at += span;
            Some(start)
        })
        .collect();
    let total: u32 = timed.spans.iter().sum();
    let (width, height) = (sprite.width as u32, sprite.height as u32);
    let mut bundle = Bundle::blank(
        &target,
        &Blank {
            width,
            height,
            layers: 0,
            fps: timed.fps,
            frames: total,
            pixel: true,
            ..Blank::default()
        },
        "cli",
    )?;
    let tag = bundle.doc.here.tag.clone();
    let mut counter = bundle.doc.next_id;
    let mut ids = |prefix: char| {
        counter += 1;
        format!("{prefix}{counter}{tag}")
    };
    let mut placed: BTreeMap<u16, Placed> = BTreeMap::new();
    let mut tree = Vec::new();
    let mut at = 0;
    while at < sprite.layers.len() {
        let (value, next) = layer_json(&sprite, at, &starts, &timed.spans, &mut ids, &mut placed, &mut notes);
        if !sprite.layers[at].tilemap {
            tree.push(value);
        }
        at = next;
    }
    if placed.is_empty() {
        bail!("{} holds no image layers to bring in", file.display());
    }
    let palette = swatches(&sprite.palette);
    let known: BTreeMap<[u8; 4], usize> = sprite
        .palette
        .iter()
        .enumerate()
        .rev()
        .map(|(i, c)| (c.rgba, i))
        .collect();
    let mut strangers: BTreeSet<[u8; 4]> = BTreeSet::new();
    let pixels: Vec<Op> = sprite
        .cels
        .iter()
        .filter_map(|cel| match &cel.picture {
            Picture::Pixels { width, rgba, .. } => Some((cel, (*width as i64).max(1), rgba)),
            Picture::Link(_) => None,
        })
        .filter_map(|(cel, w, rgba)| {
            let owner = placed.get(&cel.layer)?;
            let frame = starts[cel.frame as usize];
            owner.cels.get(&cel.frame)?;
            let mut by_colour: BTreeMap<[u8; 4], Vec<[i64; 2]>> = BTreeMap::new();
            rgba.as_chunks::<4>().0.iter().enumerate().for_each(|(i, px)| {
                let alpha = (px[3] as u32 * cel.opacity as u32 + 127) / 255;
                let (x, y) = (cel.x as i64 + i as i64 % w, cel.y as i64 + i as i64 / w);
                let inside = x >= 0 && y >= 0 && x < width as i64 && y < height as i64;
                if alpha > 0 && inside {
                    by_colour
                        .entry([px[0], px[1], px[2], alpha as u8])
                        .or_default()
                        .push([x, y]);
                }
            });
            Some(
                by_colour
                    .into_iter()
                    .map(|(colour, cells)| {
                        let key = match known.get(&colour) {
                            Some(index) => index.to_string(),
                            None => {
                                strangers.insert(colour);
                                hex_of(colour)
                            }
                        };
                        Op::Pixel(PixelSpec {
                            layer: owner.id.clone(),
                            frame,
                            color: Some(key),
                            cells,
                            ..PixelSpec::default()
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect();
    if strangers.len() > MOST_COLOURS {
        let _ = fs::remove_dir_all(&target);
        bail!(
            "{} uses {} colours that are not in its palette: klad draws pixel art with palette colours, at most {MOST_COLOURS} new ones are added. Reduce its colours first (in Aseprite: Sprite > Color Mode > Indexed)",
            file.display(),
            strangers.len()
        );
    }
    if !strangers.is_empty() {
        notes.insert(format!(
            "{} colours that were not in the file's palette were added to it",
            strangers.len()
        ));
    }
    let names = clip_names(&sprite.tags);
    let last = sprite.durations.len() - 1;
    let clips = sprite.tags.iter().zip(&names).map(|(tag, name)| {
        let (from, to) = ((tag.from as usize).min(last), (tag.to as usize).min(last));
        if tag.direction != 0 {
            notes.insert(format!(
                "tag {name} plays backwards or back and forth in the file: the clip plays forwards"
            ));
        }
        Op::Clip(ClipEdit::Add(Clip::new(
            name,
            starts[from.min(to)],
            starts[to.max(from)] + timed.spans[to.max(from)] - 1,
            tag.repeat == 0,
        )))
    });
    let ops: Vec<Op> = (!palette.is_empty())
        .then_some(Op::Palette(PaletteEdit::Set { swatches: palette }))
        .into_iter()
        .map(Ok)
        .chain(
            tree.into_iter()
                .map(|layer| Op::parse(json!({ "op": "add", "layer": layer }))),
        )
        .chain(pixels.into_iter().map(Ok))
        .chain(clips.collect::<Vec<_>>().into_iter().map(Ok))
        .collect::<Result<_>>()?;
    let applied = bundle.apply(&ops).and_then(|_| bundle.stamp("cli"));
    if let Err(error) = applied {
        let _ = fs::remove_dir_all(&target);
        return Err(error.context(format!("bringing in {}", file.display())));
    }
    let doc = &bundle.doc;
    eprintln!(
        "{}x{}, {} layers, {} frames at {} fps, {} palette colours, {} clips",
        doc.canvas.width,
        doc.canvas.height,
        doc.all_layers().len(),
        doc.timeline.frames,
        doc.timeline.fps,
        doc.palette.len(),
        doc.timeline.clips.len()
    );
    notes.iter().for_each(|note| eprintln!("note: {note}"));
    Ok(target)
}

fn rgba8(color: [f32; 4]) -> [u8; 4] {
    color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn listed<'a>(layers: &'a [Layer], level: u16, out: &mut Vec<(&'a Layer, u16)>, skipped: &mut Vec<String>) {
    layers.iter().for_each(|layer| match layer.children().is_empty() {
        true if layer.pixel() => out.push((layer, level)),
        true => skipped.push(layer.name.clone()),
        false => {
            let before = out.len();
            out.push((layer, level));
            listed(layer.children(), level + 1, out, skipped);
            if out.len() == before + 1 {
                out.pop();
            }
        }
    });
}

fn ms(frame: u32, fps: u32) -> u32 {
    ((frame as u64 * 1000 + fps as u64 / 2) / fps as u64) as u32
}

pub fn sprite_of(bundle: &Bundle, variant: Option<&str>) -> Result<(Sprite, Vec<String>)> {
    let doc: &Document = &bundle.doc;
    let (width, height) = (doc.canvas.width, doc.canvas.height);
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        bail!("an Aseprite file is at most 65535 px a side");
    }
    let dress = dress_of(doc, variant)?;
    let colour_of = |key: &str| -> Result<[u8; 4]> {
        match (dress.get(key), doc.entry(key)) {
            (Some(c), _) => Ok(c.map(|v| ((v as u32 * 255 + 32767) / 65535) as u8)),
            (None, Some(swatch)) => Ok(rgba8(parse_hex(&swatch.color)?)),
            (None, None) => Ok(rgba8(parse_hex(key)?)),
        }
    };
    let mut notes: Vec<String> = Vec::new();
    let mut skipped = Vec::new();
    let mut kept: Vec<(&Layer, u16)> = Vec::new();
    listed(&doc.layers, 0, &mut kept, &mut skipped);
    if !kept.iter().any(|(l, _)| l.pixel()) {
        bail!(
            "this drawing has no pixel layers: an Aseprite file holds pixel art. Export a painting as a sheet (--sheet) or as numbered frames (--frames)"
        );
    }
    if !skipped.is_empty() {
        notes.push(format!(
            "left out, since an Aseprite file holds only pixel layers: {}",
            skipped.join(", ")
        ));
    }
    let frames = doc.timeline.frames;
    let fps = doc.timeline.fps;
    let keyed = kept.iter().any(|(l, _)| !l.keys.is_empty());
    let cuts: BTreeSet<u32> = match keyed {
        true => (0..frames).collect(),
        false => std::iter::once(0)
            .chain(
                kept.iter()
                    .flat_map(|(l, _)| {
                        l.cels()
                            .iter()
                            .flat_map(|c| [Some(c.frame), c.hold.map(|h| c.frame + h)])
                    })
                    .flatten(),
            )
            .chain(doc.timeline.clips.iter().flat_map(|c| [c.from, c.to + 1]))
            .filter(|f| *f < frames)
            .collect(),
    };
    let starts: Vec<u32> = cuts.into_iter().collect();
    if starts.len() > u16::MAX as usize {
        bail!("an Aseprite file holds at most 65535 frames");
    }
    let ends: Vec<u32> = starts.iter().skip(1).copied().chain([frames]).collect();
    let durations: Vec<u16> = starts
        .iter()
        .zip(&ends)
        .map(|(from, to)| (ms(*to, fps) - ms(*from, fps)).clamp(1, u16::MAX as u32) as u16)
        .collect();
    let segment_of = |frame: u32| starts.iter().rposition(|s| *s <= frame).unwrap_or(0) as u16;
    if kept.iter().any(|(l, _)| {
        ["rotate", "sx", "sy"].iter().any(|p| l.keys.contains_key(*p)) || (!l.pixel() && !l.keys.is_empty())
    }) {
        notes.push(
            "keyed turns and scales, and keys on groups, are not carried: only a pixel layer's own moves and opacity"
                .into(),
        );
    }
    if kept
        .iter()
        .any(|(l, _)| l.mask.is_some() || !l.effects.is_empty() || l.clip)
    {
        notes.push("masks, effects and clipping are not carried".into());
    }
    if doc.timeline.clips.iter().any(|c| c.fps.is_some()) {
        notes.push("clips with a speed of their own play at the drawing's speed in the file".into());
    }
    let mut cels: Vec<Cel> = Vec::new();
    kept.iter()
        .enumerate()
        .filter(|(_, (l, _))| l.pixel())
        .try_for_each(|(index, (layer, _))| {
            let mut seen: BTreeMap<(String, i32, i32, u8), u16> = BTreeMap::new();
            starts.iter().enumerate().try_for_each(|(segment, &frame)| {
                let Some(content) = layer.content_at(frame) else {
                    return Ok(());
                };
                let (opacity, moved) = layer.keyed(frame);
                let (dx, dy) = moved.map_or((0, 0), |m| (m.x.round() as i32, m.y.round() as i32));
                let own = match layer.opacity > 0.0 {
                    true => ((opacity / layer.opacity).clamp(0.0, 1.0) * 255.0).round() as u8,
                    false => 255,
                };
                let key = (content.id.clone(), dx, dy, own);
                if let Some(first) = seen.get(&key) {
                    cels.push(Cel {
                        layer: index as u16,
                        frame: segment as u16,
                        x: 0,
                        y: 0,
                        opacity: own,
                        picture: Picture::Link(*first),
                    });
                    return Ok(());
                }
                let marks = bundle.read_marks(&layer.id, &content.id)?;
                let grid = klad_core::pixel::grid_of(&marks, width, height);
                let filled: Vec<(u32, u32, &str)> = (0..height)
                    .flat_map(|y| (0..width).map(move |x| (x, y)))
                    .filter_map(|(x, y)| grid.at(x, y).map(|key| (x, y, key)))
                    .collect();
                let (Some(left), Some(right), Some(top), Some(bottom)) = (
                    filled.iter().map(|c| c.0).min(),
                    filled.iter().map(|c| c.0).max(),
                    filled.iter().map(|c| c.1).min(),
                    filled.iter().map(|c| c.1).max(),
                ) else {
                    return Ok(());
                };
                let (w, h) = (right - left + 1, bottom - top + 1);
                let mut rgba = vec![0u8; (w * h * 4) as usize];
                let mut colours: BTreeMap<&str, [u8; 4]> = BTreeMap::new();
                filled.iter().try_for_each(|(x, y, key)| {
                    let colour = match colours.get(key) {
                        Some(c) => *c,
                        None => {
                            let c = colour_of(key)?;
                            colours.insert(key, c);
                            c
                        }
                    };
                    let at = (((y - top) * w + (x - left)) * 4) as usize;
                    rgba[at..at + 4].copy_from_slice(&colour);
                    Ok::<_, anyhow::Error>(())
                })?;
                seen.insert(key, segment as u16);
                cels.push(Cel {
                    layer: index as u16,
                    frame: segment as u16,
                    x: (left as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    y: (top as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    opacity: own,
                    picture: Picture::Pixels {
                        width: w as u16,
                        height: h as u16,
                        rgba,
                    },
                });
                Ok::<_, anyhow::Error>(())
            })
        })?;
    cels.sort_by_key(|c| (c.frame, c.layer));
    let palette = doc
        .palette
        .iter()
        .map(|swatch| {
            let key = match swatch.id.is_empty() {
                true => swatch.color.as_str(),
                false => swatch.id.as_str(),
            };
            Ok(Colour {
                rgba: colour_of(key)?,
                name: swatch.name.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let layers = kept
        .iter()
        .map(|(layer, level)| klad_render::aseprite::Layer {
            name: layer.name.clone(),
            visible: layer.visible,
            group: !layer.pixel(),
            tilemap: false,
            level: *level,
            blend: BLENDS.iter().find(|(_, b)| *b == layer.blend).map_or(0, |(n, _)| *n),
            opacity: (layer.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            reference: layer.reference,
            background: false,
        })
        .collect();
    let tags = doc
        .timeline
        .clips
        .iter()
        .map(|clip| Tag {
            name: clip.name.clone(),
            from: segment_of(clip.from),
            to: segment_of(clip.to),
            direction: 0,
            repeat: match clip.looping {
                true => 0,
                false => 1,
            },
        })
        .collect();
    Ok((
        Sprite {
            width: width as u16,
            height: height as u16,
            palette,
            layers,
            durations,
            cels,
            tags,
            notes: Vec::new(),
        },
        notes,
    ))
}

pub fn export(path: &Path, out: Option<PathBuf>, variant: Option<&str>) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let (sprite, notes) = sprite_of(&bundle, variant)?;
    let bytes = klad_render::aseprite::write(&sprite)?;
    let file = out.unwrap_or_else(|| {
        bundle_dir(&bundle).join(format!("{}{}.aseprite", bundle_stem(&bundle), variant_tag(variant)))
    });
    write_atomic(&file, &bytes)?;
    println!("{}", file.display());
    eprintln!(
        "{}x{}, {} layers, {} frames, {} cels, {} palette colours, {} tags, {:.1} KB",
        sprite.width,
        sprite.height,
        sprite.layers.len(),
        sprite.durations.len(),
        sprite.cels.len(),
        sprite.palette.len(),
        sprite.tags.len(),
        bytes.len() as f64 / 1024.0
    );
    notes.iter().for_each(|note| eprintln!("note: {note}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_times_become_whole_frames_at_the_slowest_speed_that_fits() {
        let even = timing(&[100, 100, 100]);
        assert_eq!((even.fps, even.spans.clone(), even.exact), (10, vec![1, 1, 1], true));
        let held = timing(&[100, 300, 150]);
        assert_eq!((held.fps, held.spans.clone(), held.exact), (20, vec![2, 6, 3], true));
        let slow = timing(&[250, 250]);
        assert_eq!((slow.fps, slow.spans.clone()), (4, vec![1, 1]));
        let twelve = timing(&[83, 83, 84]);
        assert_eq!(
            (twelve.fps, twelve.spans.clone(), twelve.exact),
            (12, vec![1, 1, 1], false)
        );
        let one = timing(&[1000]);
        assert_eq!((one.fps, one.spans.clone(), one.exact), (1, vec![1], true));
        assert_eq!(ms(3, 12) - ms(0, 12), 250);
        assert_eq!((ms(1, 12), ms(2, 12) - ms(1, 12), ms(3, 12) - ms(2, 12)), (83, 84, 83));
    }

    #[test]
    fn names_are_made_fit_for_klad() {
        let tag = |name: &str| Tag {
            name: name.into(),
            from: 0,
            to: 0,
            direction: 0,
            repeat: 0,
        };
        assert_eq!(
            clip_names(&[tag("run"), tag(""), tag("run"), tag("run")]),
            ["run", "tag 2", "run 2", "run 3"]
        );
        let colour = |name: &str, rgba: [u8; 4]| Colour {
            rgba,
            name: name.into(),
        };
        let made = swatches(&[
            colour("skin", [240, 200, 160, 255]),
            colour("skin", [1, 2, 3, 128]),
            colour("#odd", [0, 0, 0, 255]),
        ]);
        let read: Vec<(&str, &str)> = made.iter().map(|s| (s.color.as_str(), s.name.as_str())).collect();
        assert_eq!(read, [("#f0c8a0", "skin"), ("#01020380", ""), ("#000000", "odd")]);
        assert!(
            is_file(Path::new("a/b.ASE")) && is_file(Path::new("hero.aseprite")) && !is_file(Path::new("hero.png"))
        );
    }
}
