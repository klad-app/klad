use anyhow::{Result, bail};
use klad_core::bundle::Bundle;
use klad_core::color::{Rgba, parse_hex};
use klad_core::doc::{Document, Layer};
use klad_core::pixelcheck::{Finding, Kind, check};
use serde_json::json;
use std::path::Path;

const SHOWN: usize = 12;

fn colour_of(doc: &Document, key: &str) -> Option<Rgba> {
    match doc.entry(key) {
        Some(swatch) => parse_hex(&swatch.color).ok(),
        None => parse_hex(key).ok(),
    }
}

fn name_of(doc: &Document, key: &str) -> String {
    match doc.entry(key) {
        Some(swatch) if !swatch.name.is_empty() => swatch.name.clone(),
        Some(swatch) => swatch.color.clone(),
        None => key.to_string(),
    }
}

fn spot(finding: &Finding) -> String {
    match finding.at == finding.to {
        true => format!("{},{}", finding.at[0], finding.at[1]),
        false => format!(
            "{},{} to {},{}",
            finding.at[0], finding.at[1], finding.to[0], finding.to[1]
        ),
    }
}

fn words(doc: &Document, finding: &Finding) -> String {
    match finding.beside.as_slice() {
        [a, b] => format!("{}, between {} and {}", finding.note, name_of(doc, a), name_of(doc, b)),
        _ => finding.note.clone(),
    }
}

struct Checked<'a> {
    layer: &'a Layer,
    frame: u32,
    found: Vec<Finding>,
}

fn drawings<'a>(layers: &[&'a Layer], frames: &[u32]) -> Vec<(&'a Layer, u32, String)> {
    layers
        .iter()
        .flat_map(|layer| {
            let mut seen: Vec<String> = Vec::new();
            frames
                .iter()
                .filter_map(|frame| {
                    let cel = layer.content_at(*frame)?;
                    match seen.contains(&cel.id) {
                        true => None,
                        false => {
                            seen.push(cel.id.clone());
                            Some((*layer, *frame, cel.id.clone()))
                        }
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn within<'a>(layer: &'a Layer, out: &mut Vec<&'a Layer>) {
    out.push(layer);
    layer.children().iter().for_each(|child| within(child, out));
}

pub fn run(path: &Path, layer: Option<&str>, frame: u32, all: bool, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    if frame >= doc.timeline.frames {
        bail!("frame {frame} is past the last frame {}", doc.timeline.frames - 1);
    }
    let layers: Vec<&Layer> = match layer {
        Some(key) => {
            let owner = doc.find_layer(key)?;
            let mut inside: Vec<&Layer> = Vec::new();
            within(owner, &mut inside);
            let pixel: Vec<&Layer> = inside.into_iter().filter(|l| l.pixel()).collect();
            if pixel.is_empty() {
                bail!(
                    "{} is a {} layer: klad check reads pixel layers",
                    owner.name,
                    owner.kind_name()
                );
            }
            pixel
        }
        None => doc.all_layers().into_iter().filter(|l| l.pixel()).collect(),
    };
    if layers.is_empty() {
        bail!("this drawing has no pixel layers: klad check reads pixel art");
    }
    let frames: Vec<u32> = match all {
        true => (0..doc.timeline.frames).collect(),
        false => vec![frame],
    };
    let checked: Vec<Checked> = drawings(&layers, &frames)
        .into_iter()
        .map(|(layer, frame, cel)| {
            let marks = bundle.read_marks(&layer.id, &cel)?;
            let grid = klad_core::pixel::grid_of(&marks, doc.canvas.width, doc.canvas.height);
            Ok(Checked {
                layer,
                frame,
                found: check(&grid, doc.canvas.wrap, &|key| colour_of(doc, key)),
            })
        })
        .collect::<Result<_>>()?;
    let total: usize = checked.iter().map(|c| c.found.len()).sum();
    if as_json {
        let findings: Vec<serde_json::Value> = checked
            .iter()
            .flat_map(|c| {
                c.found.iter().map(move |f| {
                    json!({
                        "layer": c.layer.id,
                        "frame": c.frame,
                        "kind": f.kind,
                        "at": f.at,
                        "to": f.to,
                        "color": name_of(doc, &f.key),
                        "note": words(doc, f),
                    })
                })
            })
            .collect();
        println!(
            "{}",
            json!({
                "checked": checked.iter().map(|c| json!({"layer": c.layer.id, "name": c.layer.name, "frame": c.frame})).collect::<Vec<_>>(),
                "findings": findings,
            })
        );
        return Ok(());
    }
    checked.iter().for_each(|c| {
        let head = format!("{} ({}), frame {}", c.layer.name, c.layer.id, c.frame);
        if c.found.is_empty() {
            println!("{head}: nothing to look at");
            return;
        }
        println!("{head}: {} to look at", c.found.len());
        Kind::ALL.iter().for_each(|kind| {
            let of_kind: Vec<&Finding> = c.found.iter().filter(|f| f.kind == *kind).collect();
            of_kind.iter().take(SHOWN).for_each(|f| {
                println!(
                    "  {:<16} {:<16} {:<12} {}",
                    kind.name(),
                    spot(f),
                    name_of(doc, &f.key),
                    words(doc, f)
                );
            });
            if of_kind.len() > SHOWN {
                println!(
                    "  {:<16} and {} more (--json lists them all)",
                    kind.name(),
                    of_kind.len() - SHOWN
                );
            }
        });
    });
    if total > 0 {
        println!();
        Kind::ALL
            .iter()
            .filter(|kind| checked.iter().any(|c| c.found.iter().any(|f| f.kind == **kind)))
            .for_each(|kind| println!("{}: {}", kind.name(), kind.advice()));
        println!("These are places to look at, not errors: a single pixel can be an eye.");
    }
    Ok(())
}
