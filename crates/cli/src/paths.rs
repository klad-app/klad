use crate::agent::{self, Output};
use anyhow::{Result, anyhow, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::{Item, Shape};
use klad_core::ops::Op;
use klad_core::pathedit::{self, Anchor, PathEdit, Side};
use serde_json::{Value, json};
use std::path::Path;

pub struct Edits {
    pub move_to: Option<Vec<String>>,
    pub nudge: Option<Vec<String>>,
    pub handle: Option<Vec<String>>,
    pub insert: Option<String>,
    pub add: Option<[f32; 2]>,
    pub rm: Option<String>,
    pub corner: Option<String>,
    pub smooth: Option<String>,
    pub simplify: Option<f32>,
    pub convert: bool,
}

fn numbers(text: &str) -> Result<Vec<usize>> {
    text.split(',')
        .map(|v| {
            v.trim()
                .parse::<usize>()
                .map_err(|_| anyhow!("{v:?} is not a point number"))
        })
        .collect()
}

fn pair(text: &str) -> Result<[f32; 2]> {
    let v: Vec<f32> = text
        .split(',')
        .map(|v| v.trim().parse::<f32>().map_err(|_| anyhow!("{text:?} is not x,y")))
        .collect::<Result<_>>()?;
    v.try_into().map_err(|_| anyhow!("{text:?} is not x,y"))
}

fn single(text: &str) -> Result<usize> {
    match numbers(text)?[..] {
        [one] => Ok(one),
        _ => bail!("{text:?}: give one point number"),
    }
}

pub fn edits(e: &Edits) -> Result<Vec<PathEdit>> {
    let mut out = Vec::new();
    if e.convert {
        out.push(PathEdit::Convert);
    }
    if let Some(tolerance) = e.simplify {
        out.push(PathEdit::Simplify { tolerance });
    }
    if let Some(v) = &e.move_to {
        out.push(PathEdit::MoveTo {
            point: single(&v[0])?,
            to: pair(&v[1])?,
        });
    }
    if let Some(v) = &e.nudge {
        out.push(PathEdit::Move {
            points: numbers(&v[0])?,
            by: pair(&v[1])?,
        });
    }
    if let Some(v) = &e.handle {
        let side = match v[1].as_str() {
            "in" => Side::In,
            "out" => Side::Out,
            other => bail!("--handle takes in or out, not {other:?}"),
        };
        out.push(PathEdit::Handle {
            point: single(&v[0])?,
            side,
            to: pair(&v[2])?,
            free: false,
        });
    }
    if let Some(text) = &e.insert {
        let (segment, t) = match text.split_once(',') {
            Some((s, t)) => (
                single(s)?,
                t.trim().parse::<f32>().map_err(|_| anyhow!("{t:?} is not a number"))?,
            ),
            None => (single(text)?, 0.5),
        };
        out.push(PathEdit::Insert { segment, t });
    }
    if let Some(at) = e.add {
        out.push(PathEdit::Add { at });
    }
    if let Some(text) = &e.corner {
        out.push(PathEdit::Corner { points: numbers(text)? });
    }
    if let Some(text) = &e.smooth {
        out.push(PathEdit::Smooth { points: numbers(text)? });
    }
    if let Some(text) = &e.rm {
        out.push(PathEdit::Remove { points: numbers(text)? });
    }
    Ok(out)
}

fn fmt(p: [f32; 2]) -> String {
    format!("{:.2},{:.2}", p[0], p[1])
        .replace(".00,", ",")
        .trim_end_matches(".00")
        .to_string()
}

fn point_json(i: usize, a: &Anchor) -> Value {
    json!({
        "i": i,
        "at": a.at,
        "in": a.has_in().then_some(a.inn),
        "out": a.has_out().then_some(a.out),
        "smooth": a.smooth(),
    })
}

pub fn list(path: &Path, id: &str, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let Some(Item::Node(layer, _, node)) = bundle.doc.item(id) else {
        bail!("{id} is not a shape");
    };
    let subs = pathedit::from_shape(&node.shape)?;
    let converted = !matches!(node.shape, Shape::Path { .. });
    if as_json {
        let mut i = 0;
        let value = json!({
            "id": node.id,
            "type": node.type_name(),
            "layer": layer.id,
            "transform": node.transform,
            "subpaths": subs.iter().map(|s| {
                let points: Vec<Value> = s.anchors.iter().map(|a| { i += 1; point_json(i - 1, a) }).collect();
                json!({ "closed": s.closed, "points": points })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let total: usize = subs.iter().map(|s| s.anchors.len()).sum();
    println!(
        "{} {}{} in layer {} ({}): {} subpath{}, {total} points",
        node.type_name(),
        node.id,
        if node.name.is_empty() {
            String::new()
        } else {
            format!(" \"{}\"", node.name)
        },
        layer.id,
        layer.name,
        subs.len(),
        if subs.len() == 1 { "" } else { "s" }
    );
    if converted {
        println!(
            "any point edit turns this {} into a path with these points",
            node.type_name()
        );
    }
    if !node.transform.is_identity() {
        println!("coordinates are the shape's own, before its transform");
    }
    let mut i = 0;
    subs.iter().enumerate().for_each(|(k, s)| {
        if subs.len() > 1 {
            println!("subpath {k}{}", if s.closed { ", closed" } else { "" });
        } else if s.closed {
            println!("closed");
        }
        s.anchors.iter().for_each(|a| {
            let kind = if a.smooth() {
                "smooth"
            } else if a.has_in() || a.has_out() {
                "curve"
            } else {
                "corner"
            };
            let handles = [
                a.has_in().then(|| format!("in {}", fmt(a.inn))),
                a.has_out().then(|| format!("out {}", fmt(a.out))),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("  ");
            println!("{i:>5}  {kind:<6}  {:<18} {handles}", fmt(a.at));
            i += 1;
        });
    });
    Ok(())
}

pub fn run(path: &Path, id: &str, e: &Edits, as_json: bool, out: &Output) -> Result<()> {
    let ops: Vec<Op> = edits(e)?
        .into_iter()
        .map(|edit| Op::Path {
            id: id.to_string(),
            edit,
        })
        .collect();
    if ops.is_empty() {
        return list(path, id, as_json);
    }
    agent::edit(path, &ops, out)?;
    if !out.json {
        list(path, id, false)?;
    }
    Ok(())
}
