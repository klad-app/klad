use anyhow::{Result, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::{Clip, Document, Timeline};
use klad_core::timing::{Cell, cells, ends, sheet_layers};
use serde_json::{Value, json};
use std::path::Path;

fn labels(layer: &klad_core::doc::Layer, to: u32) -> Vec<String> {
    let tweens: Vec<&str> = layer
        .cels()
        .iter()
        .filter(|c| c.tween.is_some())
        .map(|c| c.id.as_str())
        .collect();
    cells(layer, to + 1)
        .iter()
        .map(|cell| match cell {
            Cell::Drawing { cel, start: true } if tweens.contains(&cel.as_str()) => format!("{cel}*"),
            other => label(other),
        })
        .collect()
}

fn label(cell: &Cell) -> String {
    match cell {
        Cell::Drawing { cel, start: true } => cel.clone(),
        Cell::Link {
            cel,
            source,
            start: true,
        } => format!("{cel}->{source}"),
        Cell::Drawing { .. } | Cell::Link { .. } => "|".into(),
        Cell::Blank => ".".into(),
    }
}

fn clip_marks(doc: &Document, frame: u32) -> String {
    doc.timeline
        .clips
        .iter()
        .filter(|c| c.from <= frame && frame <= c.to)
        .map(|c| {
            let open = if c.from == frame { "[" } else { "" };
            let close = if c.to == frame { "]" } else { "" };
            format!("{open}{}{close}", c.name)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn short(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}~", &text[..at.saturating_sub(1)]),
        None => text.to_string(),
    }
}

pub fn sheet_json(doc: &Document, from: u32, to: u32) -> Value {
    let layers: Vec<Value> = sheet_layers(doc)
        .iter()
        .map(|layer| {
            let mask_of = doc
                .all_layers()
                .iter()
                .find(|l| l.mask.as_deref().is_some_and(|m| m.id == layer.id))
                .map(|l| l.id.clone());
            let ends = ends(layer.cels());
            let cels: Vec<Value> = layer
                .cels()
                .iter()
                .zip(ends)
                .map(|(c, end)| {
                    json!({
                        "id": c.id,
                        "frame": c.frame,
                        "hold": c.hold,
                        "link": c.link,
                        "tween": c.tween,
                        "end": (end != klad_core::timing::OPEN).then_some(end),
                    })
                })
                .collect();
            let all: Vec<String> = labels(layer, to);
            json!({
                "id": layer.id,
                "name": layer.name,
                "kind": layer.kind_name(),
                "maskOf": mask_of,
                "cels": cels,
                "cells": all[from as usize..].to_vec(),
            })
        })
        .collect();
    json!({
        "fps": doc.timeline.fps,
        "frames": doc.timeline.frames,
        "clips": doc.timeline.clips,
        "from": from,
        "to": to,
        "layers": layers,
    })
}

pub fn sheet(path: &Path, from: Option<u32>, to: Option<u32>, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let last = doc.timeline.frames - 1;
    let (from, to) = (from.unwrap_or(0), to.unwrap_or(last).min(last));
    if from > to {
        bail!("--from {from} is past --to {to} (the last frame is {last})");
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(&sheet_json(doc, from, to))?);
        return Ok(());
    }
    let layers = sheet_layers(doc);
    let columns: Vec<Vec<String>> = layers.iter().map(|l| labels(l, to)[from as usize..].to_vec()).collect();
    let heads: Vec<(String, String)> = layers
        .iter()
        .map(|l| {
            let mask = if doc.is_mask(&l.id) { " (mask)" } else { "" };
            (l.id.clone(), short(&format!("{}{mask}", l.name), 14))
        })
        .collect();
    let widths: Vec<usize> = heads
        .iter()
        .zip(&columns)
        .map(|((id, name), cells)| {
            cells
                .iter()
                .map(String::len)
                .chain([id.len(), name.chars().count()])
                .max()
                .unwrap_or(1)
                + 2
        })
        .collect();
    let row = |first: String, items: Vec<String>, tail: String| {
        let body: String = items
            .iter()
            .zip(&widths)
            .map(|(item, w)| format!("{item:<w$}", w = *w))
            .collect();
        println!("{}", format!("{first:>5}  {body}{tail}").trim_end());
    };
    println!(
        "{} frames at {} fps{}",
        doc.timeline.frames,
        doc.timeline.fps,
        if from > 0 || to < last {
            format!(", frames {from} to {to}")
        } else {
            String::new()
        }
    );
    let clips = !doc.timeline.clips.is_empty();
    row(
        "frame".into(),
        heads.iter().map(|(id, _)| id.clone()).collect(),
        if clips { "clips".into() } else { String::new() },
    );
    row(
        String::new(),
        heads.iter().map(|(_, n)| n.clone()).collect(),
        String::new(),
    );
    (from..=to).for_each(|f| {
        let items = columns.iter().map(|c| c[(f - from) as usize].clone()).collect();
        row(f.to_string(), items, clip_marks(doc, f));
    });
    if columns.iter().flatten().any(|c| c.ends_with('*')) {
        println!("c5* is an in-between (klad show c5 names its keys; klad inbetween --redo remakes it)");
    }
    doc.timeline
        .clips
        .iter()
        .for_each(|c| println!("clip {}", clip_line(&doc.timeline, c)));
    Ok(())
}

pub fn clip_line(t: &Timeline, c: &Clip) -> String {
    let fps = t.fps_of(c);
    format!(
        "{}  frames {} to {} ({} frame{}, {:.2} s at {fps} fps{}), {}",
        c.name,
        c.from,
        c.to,
        c.frames(),
        if c.frames() == 1 { "" } else { "s" },
        c.frames() as f64 / fps as f64,
        if c.fps.is_some() { ", its own" } else { "" },
        if c.looping { "loops" } else { "plays once" }
    )
}

pub fn show_timeline(bundle: &Bundle) {
    let doc = &bundle.doc;
    let t = &doc.timeline;
    println!("{} frames at {} fps", t.frames, t.fps);
    let (w, h) = (doc.canvas.width as f32, doc.canvas.height as f32);
    match t.origin {
        Some([x, y]) => println!("origin {x},{y}"),
        None => println!("origin {},{} (the centre)", w / 2.0, h / 2.0),
    }
    if let Some(dir) = &doc.export.godot {
        println!("godot export folder {dir}");
    }
    t.clips.iter().for_each(|c| println!("clip {}", clip_line(t, c)));
}

pub fn list_clips(bundle: &Bundle, as_json: bool) -> Result<()> {
    let t = &bundle.doc.timeline;
    if as_json {
        let clips: Vec<Value> = t
            .clips
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "from": c.from,
                    "to": c.to,
                    "loop": c.looping,
                    "fps": t.fps_of(c),
                    "ownFps": c.fps.is_some(),
                    "seconds": c.frames() as f64 / t.fps_of(c) as f64,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&clips)?);
        return Ok(());
    }
    if t.clips.is_empty() {
        println!("no clips (the timeline is {} frames at {} fps)", t.frames, t.fps);
    }
    t.clips.iter().for_each(|c| println!("{}", clip_line(t, c)));
    Ok(())
}
