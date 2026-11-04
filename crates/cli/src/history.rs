use crate::agent::{Look, Output, PREVIEW_SIDE, preview, render_look};
use anyhow::{Result, anyhow, bail};
use klad_core::bundle::{Bundle, now_ms, write_atomic};
use klad_core::history::{Entry, Stack, before, entries, step};
use klad_render::View;
use klad_render::overlay::side_by_side;
use serde_json::json;
use std::fs;
use std::path::Path;

fn ago(at_ms: u64) -> String {
    let secs = now_ms().saturating_sub(at_ms) / 1000;
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

pub fn log(path: &Path, limit: usize, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let undo = entries(&bundle, Stack::Undo)?;
    let redo = entries(&bundle, Stack::Redo)?;
    if as_json {
        let value = json!({ "rev": bundle.doc.rev, "undo": undo.iter().take(limit).collect::<Vec<_>>(), "redo": redo });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!(
        "rev {}  {} undo steps, {} redo steps",
        bundle.doc.rev,
        undo.len(),
        redo.len()
    );
    undo.iter().take(limit).for_each(|e| {
        println!(
            "#{:<4} from rev {:<5} {:<7} {:>8}  {}",
            e.seq,
            e.rev,
            e.author,
            ago(e.at_ms),
            e.summary
        );
    });
    if undo.len() > limit {
        println!("... {} older", undo.len() - limit);
    }
    Ok(())
}

pub fn walk(path: &Path, stack: Stack, out: &Output) -> Result<()> {
    let mut bundle = Bundle::open(path)?;
    let word = if stack == Stack::Undo { "undid" } else { "redid" };
    match step(&mut bundle, stack)? {
        Some(entry) => {
            bundle.stamp("cli")?;
            let render = preview(&bundle, out)?;
            if out.json {
                let value =
                    json!({ "rev": bundle.doc.rev, word: entry, "render": render.map(|p| p.display().to_string()) });
                println!("{value}");
            } else {
                println!("{word} #{}: {} (now rev {})", entry.seq, entry.summary, bundle.doc.rev);
                if let Some(p) = render {
                    println!("{}", p.display());
                }
            }
        }
        None => println!("nothing to {}", if stack == Stack::Undo { "undo" } else { "redo" }),
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    fs::read_dir(from)?.try_for_each(|e| {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)
        } else {
            fs::copy(e.path(), target).map(|_| ()).map_err(Into::into)
        }
    })
}

fn before_bundle(bundle: &Bundle, entry: &Entry) -> Result<Bundle> {
    let state = before(bundle, entry.seq)?;
    let dir = bundle.cache_dir().join("diff").join(entry.seq.to_string());
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    copy_dir(&bundle.root.join("strokes"), &dir.join("strokes"))?;
    copy_dir(&bundle.root.join("brushes"), &dir.join("brushes"))?;
    state.files.iter().try_for_each(|(rel, bytes)| {
        let target = dir.join(rel);
        match bytes {
            Some(bytes) => write_atomic(&target, bytes),
            None if target.exists() => fs::remove_file(&target).map_err(Into::into),
            None => Ok(()),
        }
    })?;
    write_atomic(&dir.join("doc.json"), serde_json::to_string(&state.doc)?.as_bytes())?;
    Bundle::open(&dir)
}

pub fn diff(path: &Path, seq: Option<u64>, frame: u32, scale: Option<f32>) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let undo = entries(&bundle, Stack::Undo)?;
    let entry = match seq {
        Some(seq) => undo
            .into_iter()
            .find(|e| e.seq == seq)
            .ok_or_else(|| anyhow!("no undo step #{seq}"))?,
        None => undo.into_iter().next().ok_or_else(|| anyhow!("no history yet"))?,
    };
    let before = before_bundle(&bundle, &entry)?;
    let longest = bundle.doc.canvas.width.max(bundle.doc.canvas.height) as f32;
    if before.doc.canvas != bundle.doc.canvas {
        bail!("the canvas changed size in #{}; render each side separately", entry.seq);
    }
    let view = View::scaled(scale.unwrap_or((PREVIEW_SIDE / 2.0 / longest).min(1.0)));
    let look = Look::plain(None);
    let scratch = bundle.cache_dir().join("diff").join(format!("{}-parts", entry.seq));
    let left = render_look(&before, frame, view, &look, Some(scratch.join("before.png")), true)?.frame;
    let right = render_look(&bundle, frame, view, &look, Some(scratch.join("after.png")), true)?.frame;
    let labels = (
        format!("before #{} rev {}", entry.seq, entry.rev),
        format!("now rev {}", bundle.doc.rev),
    );
    let mut pair = side_by_side(&[(&left, &labels.0), (&right, &labels.1)])?;
    let out = bundle
        .cache_dir()
        .join("renders")
        .join(format!("diff-{}.png", entry.seq));
    write_atomic(&out, &pair.png()?)?;
    println!("#{} {}: {}", entry.seq, entry.author, entry.summary);
    println!("{}", out.display());
    Ok(())
}

pub fn selection(path: &Path, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let Some(sel) = klad_core::notes::read_selection(&bundle) else {
        println!("nothing selected in the editor");
        return Ok(());
    };
    let nodes: Vec<serde_json::Value> = sel
        .nodes
        .iter()
        .map(|id| {
            let bounds = crate::agent::item_box(&bundle, id, sel.frame).ok().flatten();
            let kind = match (bundle.doc.item(id), crate::agent::find_mark(&bundle, id).ok().flatten()) {
                (Some(klad_core::doc::Item::Node(_, _, n)), _) => format!("{} {}", n.type_name(), n.name),
                (_, Some((_, klad_core::stroke::Mark::Stroke(s)))) => format!("stroke {} {:.1}px", s.brush, s.size),
                (_, Some((_, klad_core::stroke::Mark::Fill(_)))) => "fill".into(),
                _ => "missing".into(),
            };
            json!({ "id": id, "what": kind.trim(), "bounds": bounds })
        })
        .collect();
    let layer = sel.layer.as_deref().and_then(|id| bundle.doc.layer(id));
    if as_json {
        let value = json!({
            "layer": layer.map(|l| json!({ "id": l.id, "name": l.name, "kind": l.kind().name() })),
            "nodes": nodes,
            "frame": sel.frame,
            "atMs": sel.at_ms,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let at = if bundle.doc.timeline.frames > 1 {
        format!(" on frame {}", sel.frame)
    } else {
        String::new()
    };
    println!("selected {}{at}", ago(sel.at_ms));
    if let Some(l) = layer {
        println!("layer {} {} ({})", l.id, l.name, l.kind().name());
    }
    nodes.iter().for_each(|n| {
        let b = n["bounds"].as_array().map_or("".into(), |b| {
            let v: Vec<f64> = b.iter().filter_map(|x| x.as_f64()).collect();
            format!("{:.0},{:.0} {:.0}x{:.0}", v[0], v[1], v[2], v[3])
        });
        println!(
            "{} {}  {b}",
            n["id"].as_str().unwrap_or(""),
            n["what"].as_str().unwrap_or("")
        );
    });
    if let Some(first) = sel.nodes.first().or(sel.layer.as_ref()) {
        let frame = if sel.frame > 0 {
            format!(" --frame {}", sel.frame)
        } else {
            String::new()
        };
        println!(
            "look: klad render {}{frame} --focus {first} --overlay ids,bbox",
            path.display()
        );
    }
    Ok(())
}

pub fn notes(path: &Path, done: Option<u32>, all: bool, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    if let Some(id) = done {
        if !klad_core::notes::set_done(&bundle, id, true)? {
            bail!("no note #{id}");
        }
        println!("note #{id} marked done");
        return Ok(());
    }
    let list: Vec<klad_core::notes::Note> = klad_core::notes::read(&bundle)
        .into_iter()
        .filter(|n| all || !n.done)
        .collect();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    if list.is_empty() {
        println!("no open notes");
    }
    list.iter().for_each(|n| {
        let state = if n.done { " (done)" } else { "" };
        println!(
            "#{} at {:.0},{:.0} frame {} by {} {}{state}: {}",
            n.id,
            n.x,
            n.y,
            n.frame,
            n.author,
            ago(n.at_ms),
            n.text
        );
        let r = 160.0;
        println!(
            "   look: klad render {} --region {:.0},{:.0},{:.0},{:.0} --overlay ids,bbox",
            path.display(),
            (n.x - r).max(0.0),
            (n.y - r).max(0.0),
            r * 2.0,
            r * 2.0
        );
    });
    Ok(())
}
