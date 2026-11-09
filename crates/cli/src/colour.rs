use crate::agent::Output;
use crate::layers::{self, Vectors};
use anyhow::{Result, anyhow, bail};
use klad_brush::{BrushSet, Raster, carry_seed, flood, paint_fill, regions, replay, replay_onto, sample};
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::color::parse_hex;
use klad_core::doc::{Document, Kind};
use klad_core::fill::{Fill, FillRef, FillSource};
use klad_core::markedit::address;
use klad_core::ops::FillSpec;
use klad_core::stroke::{Author, Mark};
use klad_render::{Rasters, View, compose, fill_reference};
use rayon::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn needs_vectors(doc: &Document, reference: Option<&FillRef>) -> bool {
    match reference {
        Some(FillRef::All) => true,
        Some(FillRef::Layer(id)) => doc.layer(id).is_some_and(|l| l.kind() != Kind::Paint),
        _ => false,
    }
}

pub fn seed_colours(
    bundle: &Bundle,
    cel: &str,
    frame: u32,
    marks: &[Mark],
    ids: &[u32],
    brushes: &BrushSet,
) -> Result<BTreeMap<u32, [f32; 4]>> {
    let doc = &bundle.doc;
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let mut rasters = layers::load(bundle, frame, brushes, true)?.rasters;
    if marks
        .iter()
        .filter_map(Mark::as_fill)
        .any(|f| ids.contains(&f.id) && needs_vectors(doc, f.reference()))
    {
        Vectors::default().add_to(doc, frame, &mut rasters)?;
    }
    let mut own = Raster::new(w, h);
    let mut found = BTreeMap::new();
    marks.iter().try_for_each(|mark| {
        if let Mark::Fill(fill) = mark
            && let FillSource::Seed { seed, reference } = &fill.source
            && ids.contains(&fill.id)
        {
            rasters.insert(cel.to_string(), own.clone());
            let image = fill_reference(doc, frame, &rasters, cel, reference)?;
            found.insert(fill.id, sample(&image, seed.x, seed.y));
        }
        match mark {
            Mark::Fill(fill) => {
                paint_fill(&mut own, fill);
            }
            stroke => {
                replay_onto(&mut own, std::slice::from_ref(stroke), brushes)?;
            }
        }
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(found)
}

fn drawing_frame(doc: &Document, layer: &str, cel: &str) -> Result<u32> {
    let owner = doc.layer(layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
    let found = owner
        .cels()
        .iter()
        .find(|c| c.id == cel)
        .ok_or_else(|| anyhow!("{cel} is not a cel of {}", owner.name))?;
    if let Some(source) = &found.link {
        bail!("{cel} is a link to {source}; carry the fills of {source}");
    }
    Ok(found.frame)
}

pub fn like(bundle: &Bundle, spec: &mut FillSpec) -> Result<Vec<String>> {
    let start = Instant::now();
    let doc = &bundle.doc;
    let owner = doc.find_layer(&spec.layer)?;
    if owner.kind() != Kind::Paint {
        bail!(
            "{} is a {} layer; fills live on paint layers",
            owner.id,
            owner.kind().name()
        );
    }
    let layer = owner.id.clone();
    let source = spec.like.clone().unwrap_or_default();
    let at = drawing_frame(doc, &layer, &source)?;
    let (from, to) = klad_render::sheet::range(doc, spec.frames.as_deref().unwrap_or("all"))?;
    let marks = bundle.read_marks(&layer, &source)?;
    let floods: Vec<&Fill> = marks
        .iter()
        .filter_map(Mark::as_fill)
        .filter(|f| matches!(f.source, FillSource::Seed { .. }))
        .collect();
    if floods.is_empty() {
        bail!("{source} has no flood fills to carry (lasso fills stay where they are drawn)");
    }
    let brushes = BrushSet::for_bundle(bundle)?;
    let ids: Vec<u32> = floods.iter().map(|f| f.id).collect();
    let colours = seed_colours(bundle, &source, at, &marks, &ids, &brushes)?;
    let mut targets: Vec<(String, u32)> = Vec::new();
    (from..=to).for_each(|f| {
        if let Some(cel) = owner.content_at(f)
            && cel.id != source
            && !targets.iter().any(|(c, _)| *c == cel.id)
        {
            targets.push((cel.id.clone(), f));
        }
    });
    if targets.is_empty() {
        bail!(
            "{} shows no drawing other than {source} on frames {from} to {to}",
            owner.name
        );
    }
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let results = targets
        .par_iter()
        .map(|(cel, frame)| {
            let mut rasters: Rasters = layers::load(bundle, *frame, &brushes, true)?.rasters;
            if floods.iter().any(|f| needs_vectors(doc, f.reference())) {
                Vectors::default().add_to(doc, *frame, &mut rasters)?;
            }
            let mut out = bundle.read_marks(&layer, cel)?;
            let mut failed = Vec::new();
            let mut carried = 0;
            floods.iter().try_for_each(|fill| {
                let FillSource::Seed { seed, reference } = &fill.source else {
                    return Ok(());
                };
                let place = out.iter().position(|m| m.id() == fill.id && m.as_fill().is_some());
                let before = place.unwrap_or(out.len());
                let own = replay(w, h, &out[..before], &brushes)?.raster;
                rasters.insert(cel.clone(), own);
                let image = fill_reference(doc, *frame, &rasters, cel, reference)?;
                let colour = colours.get(&fill.id).copied().unwrap_or([0.0; 4]);
                let why = |text: &str| format!("{} on frame {frame}: {text}", address(&source, fill.id));
                let Some([x, y]) = carry_seed(&image, colour, seed, &fill.coverage) else {
                    failed.push(why("no region there overlaps it"));
                    return Ok(());
                };
                let moved = klad_core::fill::Seed { x, y, ..*seed };
                let coverage = flood(&image, &moved);
                let (px, old) = (coverage.pixels(), fill.coverage.pixels());
                if coverage.is_empty() {
                    failed.push(why("it covers no pixels"));
                    return Ok(());
                }
                if px as f32 > old as f32 * 2.5 + 256.0 {
                    failed.push(why(&format!("it leaked ({px} px where {source} has {old})")));
                    return Ok(());
                }
                let id = match place {
                    Some(_) => fill.id,
                    None if out.iter().any(|m| m.id() == fill.id) => bundle.doc.mark_ids(cel).next(&out),
                    None => fill.id,
                };
                let made = Mark::Fill(Fill {
                    id,
                    source: FillSource::Seed {
                        seed: moved,
                        reference: reference.clone(),
                    },
                    coverage,
                    author: Author::Agent,
                    ..(*fill).clone()
                });
                match place {
                    Some(i) => out[i] = made,
                    None => out.push(made),
                }
                carried += 1;
                Ok::<_, anyhow::Error>(())
            })?;
            Ok((cel.clone(), *frame, out, carried, failed))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut notes = Vec::new();
    let mut failed_all = Vec::new();
    results.iter().for_each(|(cel, frame, _, carried, failed)| {
        notes.push(format!(
            "{cel} (frame {frame}): {carried} of {} fills carried",
            floods.len()
        ));
        failed_all.extend(failed.iter().cloned());
    });
    if !failed_all.is_empty() {
        notes.push(format!("no fill for: {}", failed_all.join("; ")));
    }
    notes.push(format!(
        "carried the fills of {source} to {} drawings in {:.0} ms",
        results.len(),
        start.elapsed().as_secs_f64() * 1000.0
    ));
    spec.carried = Some(
        results
            .into_iter()
            .filter(|(_, _, _, carried, _)| *carried > 0)
            .map(|(cel, _, marks, _, _)| (cel, marks))
            .collect(),
    );
    Ok(notes)
}

pub struct Pocket {
    pub bounds: [u32; 4],
    pub pixels: usize,
}

pub struct FrameGaps {
    pub frame: u32,
    pub cel: Option<String>,
    pub pockets: Vec<Pocket>,
    pub mask: Vec<bool>,
}

fn background(doc: &Document) -> [f32; 4] {
    doc.canvas
        .background
        .as_deref()
        .and_then(|b| parse_hex(b).ok())
        .map_or([0.0; 4], |[r, g, b, a]| [r * a, g * a, b * a, a])
}

pub fn pockets(
    bundle: &Bundle,
    layer: &str,
    frame: u32,
    reference: &FillRef,
    (tolerance, gap, min): (f32, f32, usize),
    brushes: &BrushSet,
) -> Result<FrameGaps> {
    let doc = &bundle.doc;
    let owner = doc.layer(layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
    let cel = owner.content_at(frame).map(|c| c.id.clone());
    let mut rasters = layers::load(bundle, frame, brushes, true)?.rasters;
    if needs_vectors(doc, Some(reference)) {
        Vectors::default().add_to(doc, frame, &mut rasters)?;
    }
    let marks = match &cel {
        Some(c) => bundle.read_marks(layer, c)?,
        None => Vec::new(),
    };
    let image = fill_reference(doc, frame, &rasters, cel.as_deref().unwrap_or(""), reference)?;
    let open = match reference {
        FillRef::All => background(doc),
        _ => [0.0; 4],
    };
    let found = regions(&image, open, tolerance, gap, 1.0);
    let (w, h) = (found.width, found.height);
    let mut covered = vec![false; w * h];
    marks.iter().filter_map(Mark::as_fill).for_each(|f| {
        let c = &f.coverage;
        (0..c.height).for_each(|row| {
            (0..c.width).for_each(|col| {
                let (x, y) = ((c.x + col) as usize, (c.y + row) as usize);
                if x < w && y < h && c.alpha[(row * c.width + col) as usize] > 0 {
                    covered[y * w + x] = true;
                }
            });
        });
    });
    let mut bare: BTreeMap<u32, (usize, [u32; 4])> = BTreeMap::new();
    let mut mask = vec![false; w * h];
    (0..w * h).for_each(|i| {
        let label = found.labels[i];
        if label == 0 || covered[i] || found.border[label as usize - 1] {
            return;
        }
        mask[i] = true;
        let (x, y) = ((i % w) as u32, (i / w) as u32);
        let entry = bare.entry(label).or_insert((0, [u32::MAX, u32::MAX, 0, 0]));
        entry.0 += 1;
        let b = &mut entry.1;
        *b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
    });
    let pockets: Vec<Pocket> = bare
        .into_values()
        .filter(|(n, _)| *n >= min)
        .map(|(pixels, [x0, y0, x1, y1])| Pocket {
            bounds: [x0, y0, x1 - x0, y1 - y0],
            pixels,
        })
        .collect();
    Ok(FrameGaps {
        frame,
        cel,
        pockets,
        mask,
    })
}

fn gap_render(bundle: &Bundle, found: &FrameGaps, rasters_from: &BrushSet) -> Result<PathBuf> {
    let doc = &bundle.doc;
    let loaded = layers::load(bundle, found.frame, rasters_from, true)?;
    let mut image = compose(doc, found.frame, &loaded.rasters, View::scaled(1.0))?;
    let w = image.width as usize;
    found.mask.iter().enumerate().filter(|(_, m)| **m).for_each(|(i, _)| {
        let at = i * 4;
        let px = &mut image.rgba[at..at + 4];
        px[0] = ((px[0] as u32 + 255 * 3) / 4) as u8;
        px[1] = (px[1] as u32 / 4) as u8;
        px[2] = ((px[2] as u32 + 255 * 3) / 4) as u8;
        px[3] = 255;
    });
    found.pockets.iter().for_each(|p| {
        let [x, y, bw, bh] = p.bounds;
        let (x0, y0) = (x.saturating_sub(3) as usize, y.saturating_sub(3) as usize);
        let (x1, y1) = (
            ((x + bw + 3) as usize).min(w - 1),
            ((y + bh + 3) as usize).min(image.height as usize - 1),
        );
        let mut put = |x: usize, y: usize| {
            let at = (y * w + x) * 4;
            image.rgba[at..at + 4].copy_from_slice(&[230, 20, 160, 255]);
        };
        (x0..=x1).for_each(|x| {
            put(x, y0);
            put(x, y1);
        });
        (y0..=y1).for_each(|y| {
            put(x0, y);
            put(x1, y);
        });
    });
    let path = bundle
        .cache_dir()
        .join("renders")
        .join(format!("r{:016x}-gaps-f{}.png", bundle.seen, found.frame));
    write_atomic(&path, &image.png()?)?;
    Ok(path)
}

pub struct GapAsk {
    pub layer: String,
    pub frames: String,
    pub reference: Option<String>,
    pub gap: Option<f32>,
    pub tolerance: Option<f32>,
    pub min: usize,
}

fn marks_on(bundle: &Bundle, layer: &str, frames: (u32, u32)) -> Result<Vec<Mark>> {
    let Some(owner) = bundle.doc.layer(layer) else {
        bail!("no layer {layer}");
    };
    let cels: std::collections::BTreeSet<String> = (frames.0..=frames.1)
        .filter_map(|f| owner.content_at(f).map(|c| c.id.clone()))
        .collect();
    let marks = cels
        .iter()
        .map(|cel| bundle.read_marks(layer, cel))
        .collect::<Result<Vec<_>>>()?;
    Ok(marks.into_iter().flatten().collect())
}

fn seeded(bundle: &Bundle, layer: &str, frames: (u32, u32)) -> Result<Vec<(FillRef, f32, f32)>> {
    Ok(marks_on(bundle, layer, frames)?
        .iter()
        .filter_map(Mark::as_fill)
        .filter_map(|f| match &f.source {
            FillSource::Seed { seed, reference } => Some((reference.clone(), seed.gap, seed.tolerance)),
            _ => None,
        })
        .collect())
}

fn filled_from(bundle: &Bundle, lines: &str, frames: (u32, u32)) -> Result<Option<String>> {
    let counts = bundle
        .doc
        .all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Paint && l.id != lines)
        .map(|l| {
            let from = seeded(bundle, &l.id, frames)?
                .iter()
                .filter(|(r, _, _)| *r == FillRef::Layer(lines.to_string()))
                .count();
            Ok((from, l.id.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(counts
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .max_by_key(|(n, _)| *n)
        .map(|(_, id)| id))
}

fn settle(bundle: &Bundle, layer: &str, frames: (u32, u32), ask: &GapAsk) -> Result<(FillRef, f32, f32)> {
    let doc = &bundle.doc;
    let owner = doc.layer(layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
    let seen = seeded(bundle, layer, frames)?;
    let reference = match &ask.reference {
        Some(key) => {
            let spec = FillSpec {
                reference: Some(key.clone()),
                ..FillSpec::seeded(layer, [0.0, 0.0])
            };
            spec.reference(doc)?
        }
        None => {
            let mut counts: BTreeMap<String, (usize, FillRef)> = BTreeMap::new();
            seen.iter().for_each(|(r, _, _)| {
                counts.entry(r.name().to_string()).or_insert((0, r.clone())).0 += 1;
            });
            match counts.into_values().max_by_key(|(n, _)| *n) {
                Some((_, r)) => r,
                None if marks_on(bundle, layer, frames)?.iter().any(|m| m.as_fill().is_none()) => FillRef::This,
                None => bail!(
                    "{} has no strokes or flood fills on those frames and no layer fills from it; give --ref (this, all or a layer)",
                    owner.name
                ),
            }
        }
    };
    let gap = ask
        .gap
        .unwrap_or_else(|| seen.iter().map(|(_, g, _)| *g).fold(0.0, f32::max));
    let tolerance = ask
        .tolerance
        .unwrap_or_else(|| seen.first().map_or(klad_core::fill::TOLERANCE, |(_, _, t)| *t));
    Ok((reference, gap, tolerance))
}

pub fn gaps(path: &Path, ask: &GapAsk, out: &Output) -> Result<()> {
    let start = Instant::now();
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let owner = doc.find_layer(&ask.layer)?;
    if owner.kind() != Kind::Paint {
        bail!(
            "{} is a {} layer; fills live on paint layers",
            owner.id,
            owner.kind().name()
        );
    }
    let span = klad_render::sheet::range(doc, &ask.frames)?;
    let layer = match &ask.reference {
        None if seeded(&bundle, &owner.id, span)?.is_empty() => {
            filled_from(&bundle, &owner.id, span)?.unwrap_or_else(|| owner.id.clone())
        }
        _ => owner.id.clone(),
    };
    let asked = owner.name.clone();
    let owner = doc.layer(&layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
    if owner.name != asked && !out.json {
        println!(
            "{asked} has no flood fills; {} fills from it, so it is checked",
            owner.name
        );
    }
    let (reference, gap, tolerance) = settle(&bundle, &layer, span, ask)?;
    let brushes = BrushSet::for_bundle(&bundle)?;
    let found = (span.0..=span.1)
        .into_par_iter()
        .map(|f| pockets(&bundle, &layer, f, &reference, (tolerance, gap, ask.min), &brushes))
        .collect::<Result<Vec<_>>>()?;
    let with: Vec<&FrameGaps> = found.iter().filter(|f| !f.pockets.is_empty()).collect();
    let shown: Vec<&FrameGaps> = match out.render {
        true => found.iter().collect(),
        false => with.clone(),
    };
    let renders: Vec<(u32, PathBuf)> = shown
        .par_iter()
        .map(|f| Ok((f.frame, gap_render(&bundle, f, &brushes)?)))
        .collect::<Result<Vec<_>>>()?;
    if out.json {
        let frames: Vec<serde_json::Value> = found
            .iter()
            .map(|f| {
                json!({
                    "frame": f.frame,
                    "cel": f.cel,
                    "pockets": f.pockets.iter().map(|p| json!({"bounds": p.bounds, "pixels": p.pixels})).collect::<Vec<_>>(),
                    "render": renders.iter().find(|(fr, _)| *fr == f.frame).map(|(_, p)| p.display().to_string()),
                })
            })
            .collect();
        println!(
            "{}",
            json!({"layer": layer, "ref": reference.name(), "gap": gap, "frames": frames})
        );
        return Ok(());
    }
    println!(
        "{} against {} (gap {gap}, tolerance {tolerance}), frames {} to {}:",
        owner.name,
        reference.name(),
        span.0,
        span.1
    );
    found.iter().for_each(|f| {
        let cel = f.cel.as_deref().unwrap_or("nothing");
        match f.pockets.len() {
            0 => println!("  frame {} ({cel}): no pockets", f.frame),
            n => {
                let list: Vec<String> = f
                    .pockets
                    .iter()
                    .map(|p| {
                        let [x, y, w, h] = p.bounds;
                        format!("{x},{y} {w}x{h} ({} px)", p.pixels)
                    })
                    .collect();
                println!(
                    "  frame {} ({cel}): {n} pocket{} no fill covers: {}",
                    f.frame,
                    if n == 1 { "" } else { "s" },
                    list.join(", ")
                );
            }
        }
        if let Some((_, path)) = renders.iter().find(|(fr, _)| *fr == f.frame) {
            println!("    {}", path.display());
        }
    });
    let total: usize = with.iter().map(|f| f.pockets.len()).sum();
    println!(
        "{} in {:.0} ms",
        match total {
            0 => "nothing left uncovered".to_string(),
            n => format!("{n} pockets on {} frames", with.len()),
        },
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::ops::{Op, parse_lines};
    use std::fs;

    fn ask(frames: &str) -> GapAsk {
        GapAsk {
            layer: "colour".into(),
            frames: frames.into(),
            reference: Some("lines".into()),
            gap: None,
            tolerance: None,
            min: 12,
        }
    }

    fn found(dir: &Path, frames: (u32, u32)) -> Vec<usize> {
        let bundle = Bundle::open(dir).unwrap();
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let reference = FillRef::Layer("lines".into());
        (frames.0..=frames.1)
            .map(|f| {
                pockets(&bundle, "colour", f, &reference, (0.1, 3.0, 12), &brushes)
                    .unwrap()
                    .pockets
                    .len()
            })
            .collect()
    }

    #[test]
    fn gaps_find_uncoloured_pockets_and_like_carries_the_flats_into_them() {
        let dir = crate::inbetween::tests::swing("like");
        let mut bundle = Bundle::open(&dir).unwrap();
        let turn_back = parse_lines(
            r#"{"op":"transform","ids":["ln6:s1","ln6:s2","ln6:s3","ln6:s4","ln6:s5"],"rotate":-52,"pivot":[80,100]}
{"op":"rm","ids":["col6:s1","col6:s2"]}"#,
        )
        .unwrap();
        bundle.apply(&turn_back).unwrap();
        assert_eq!(found(&dir, (5, 7)), [0, 2, 2]);
        let out = Output {
            json: false,
            render: false,
            scale: None,
            frame: 0,
            overlay: None,
        };
        gaps(&dir, &ask("all"), &out).unwrap();
        let lines = GapAsk {
            layer: "lines".into(),
            reference: None,
            ..ask("all")
        };
        gaps(&dir, &lines, &out).unwrap();
        let opened = Bundle::open(&dir).unwrap();
        assert_eq!(filled_from(&opened, "lines", (0, 7)).unwrap(), Some("colour".into()));
        assert_eq!(settle(&opened, "lines", (0, 7), &lines).unwrap().0, FillRef::This);
        let spec = FillSpec {
            at: None,
            like: Some("col0".into()),
            frames: Some("1-7".into()),
            ..FillSpec::seeded("colour", [0.0, 0.0])
        };
        crate::agent::edit(&dir, &[Op::Fill(spec)], &out).unwrap();
        assert_eq!(found(&dir, (0, 7)), [0; 8]);
        let bundle = Bundle::open(&dir).unwrap();
        let carried = bundle.read_marks("colour", "col6").unwrap();
        assert_eq!(carried.iter().map(Mark::id).collect::<Vec<_>>(), [1, 2]);
        assert!(carried.iter().all(|m| m.author() == Author::Agent));
        let again = FillSpec {
            at: None,
            like: Some("col0".into()),
            frames: Some("6-6".into()),
            ..FillSpec::seeded("colour", [0.0, 0.0])
        };
        crate::agent::edit(&dir, &[Op::Fill(again)], &out).unwrap();
        assert_eq!(
            Bundle::open(&dir).unwrap().read_marks("colour", "col6").unwrap().len(),
            2
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
