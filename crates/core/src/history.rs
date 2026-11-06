use crate::bundle::{Bundle, now_ms, write_atomic};
use crate::doc::Document;
use crate::migrate::migrate;
use crate::patch::{Patch, diff};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const KEEP: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub seq: u64,
    pub rev: u64,
    pub author: String,
    pub at_ms: u64,
    pub summary: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub absent: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub patch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    Undo,
    Redo,
}

impl Stack {
    fn dir(self) -> &'static str {
        match self {
            Stack::Undo => "undo",
            Stack::Redo => "redo",
        }
    }

    fn other(self) -> Self {
        match self {
            Stack::Undo => Stack::Redo,
            Stack::Redo => Stack::Undo,
        }
    }
}

fn stack_dir(bundle: &Bundle, stack: Stack) -> PathBuf {
    bundle.cache_dir().join("history").join(stack.dir())
}

fn entry_dir(bundle: &Bundle, stack: Stack, seq: u64) -> PathBuf {
    stack_dir(bundle, stack).join(format!("{seq:010}"))
}

fn seqs(bundle: &Bundle, stack: Stack) -> Result<Vec<u64>> {
    let dir = stack_dir(bundle, stack);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<u64> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("meta.json").exists())
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(*s));
    Ok(out)
}

fn read_entry(bundle: &Bundle, stack: Stack, seq: u64) -> Result<Entry> {
    let text = fs::read_to_string(entry_dir(bundle, stack, seq).join("meta.json"))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn entries(bundle: &Bundle, stack: Stack) -> Result<Vec<Entry>> {
    Ok(seqs(bundle, stack)?
        .into_iter()
        .filter_map(|seq| read_entry(bundle, stack, seq).ok())
        .collect())
}

fn next_seq(bundle: &Bundle) -> Result<u64> {
    let newest = |stack| seqs(bundle, stack).map(|s| s.first().copied().unwrap_or(0));
    Ok(newest(Stack::Undo)?.max(newest(Stack::Redo)?) + 1)
}

fn write_entry(bundle: &Bundle, stack: Stack, entry: &Entry, patch: &Patch) -> Result<()> {
    let dir = entry_dir(bundle, stack, entry.seq);
    fs::create_dir_all(dir.join("files"))?;
    write_atomic(
        &dir.join("patch.json.zst"),
        &zstd::encode_all(&serde_json::to_vec(patch)?[..], 3)?,
    )?;
    entry.files.iter().try_for_each(|rel| {
        let source = bundle.root.join(rel);
        if source.exists() {
            let packed = zstd::encode_all(&fs::read(&source)?[..], 3)?;
            write_atomic(&dir.join("files").join(rel), &packed)?;
        }
        Ok::<_, anyhow::Error>(())
    })?;
    write_atomic(&dir.join("meta.json"), serde_json::to_string_pretty(entry)?.as_bytes())
}

fn relative(bundle: &Bundle, path: &Path) -> String {
    path.strip_prefix(&bundle.root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn absent(bundle: &Bundle, files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| !bundle.root.join(f).exists())
        .cloned()
        .collect()
}

pub fn record(bundle: &Bundle, restore: &Patch, author: &str, summary: &str, files: &[PathBuf]) -> Result<()> {
    let files: Vec<String> = files.iter().map(|f| relative(bundle, f)).collect();
    let entry = Entry {
        seq: next_seq(bundle)?,
        rev: bundle.doc.rev,
        author: author.to_string(),
        at_ms: now_ms(),
        summary: summary.to_string(),
        absent: absent(bundle, &files),
        files,
        patch: true,
    };
    write_entry(bundle, Stack::Undo, &entry, restore)?;
    let redo = stack_dir(bundle, Stack::Redo);
    if redo.exists() {
        fs::remove_dir_all(&redo)?;
    }
    seqs(bundle, Stack::Undo)?
        .iter()
        .skip(KEEP)
        .try_for_each(|old| fs::remove_dir_all(entry_dir(bundle, Stack::Undo, *old)))?;
    Ok(())
}

pub fn checkpoint(bundle: &Bundle, after: &Document, author: &str, summary: &str, files: &[PathBuf]) -> Result<()> {
    record(bundle, &diff(after, &bundle.doc)?, author, summary, files)
}

fn read_patch(dir: &Path) -> Result<Patch> {
    let bytes = zstd::decode_all(&fs::read(dir.join("patch.json.zst"))?[..])?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn snapshot_doc(dir: &Path) -> Result<Document> {
    let text = zstd::decode_all(&fs::read(dir.join("doc.json.zst"))?[..])?;
    let value: serde_json::Value = serde_json::from_slice(&text).context("reading the snapshot document")?;
    serde_json::from_value(migrate(value)?).context("reading the snapshot document")
}

fn restored(current: &Document, entry: &Entry, dir: &Path) -> Result<Document> {
    if entry.patch {
        read_patch(dir)?
            .applied(current)
            .with_context(|| format!("step #{}: the document no longer matches it", entry.seq))
    } else {
        snapshot_doc(dir)
    }
}

fn restore_files(bundle: &Bundle, entry: &Entry, dir: &Path) -> Result<()> {
    entry.files.iter().try_for_each(|rel| {
        let target = bundle.root.join(rel);
        if entry.absent.contains(rel) {
            if target.exists() {
                fs::remove_file(&target)?;
            }
        } else {
            let packed = fs::read(dir.join("files").join(rel)).with_context(|| format!("snapshot of {rel}"))?;
            write_atomic(&target, &zstd::decode_all(&packed[..])?)?;
        }
        Ok::<_, anyhow::Error>(())
    })
}

pub fn peek(bundle: &Bundle, stack: Stack) -> Result<Option<(Entry, Option<Patch>)>> {
    let Some(seq) = seqs(bundle, stack)?.first().copied() else {
        return Ok(None);
    };
    let entry = read_entry(bundle, stack, seq)?;
    let patch = if entry.patch {
        Some(read_patch(&entry_dir(bundle, stack, seq))?)
    } else {
        None
    };
    Ok(Some((entry, patch)))
}

pub fn step(bundle: &mut Bundle, stack: Stack) -> Result<Option<Entry>> {
    let Some(seq) = seqs(bundle, stack)?.first().copied() else {
        return Ok(None);
    };
    let entry = read_entry(bundle, stack, seq)?;
    let dir = entry_dir(bundle, stack, seq);
    let mirror = Entry {
        seq: next_seq(bundle)?,
        rev: bundle.doc.rev,
        author: entry.author.clone(),
        at_ms: entry.at_ms,
        summary: entry.summary.clone(),
        files: entry.files.clone(),
        absent: absent(bundle, &entry.files),
        patch: true,
    };
    if entry.patch {
        let patch = read_patch(&dir)?;
        if let Err(e) = patch.apply_in_place(&mut bundle.doc) {
            bundle.reload()?;
            return Err(e.context(format!("step #{seq}: the document no longer matches it")));
        }
        write_entry(bundle, stack.other(), &mirror, &patch.reversed())?;
    } else {
        let doc = snapshot_doc(&dir)?;
        write_entry(bundle, stack.other(), &mirror, &diff(&doc, &bundle.doc)?)?;
        bundle.doc = Document {
            rev: bundle.doc.rev,
            ..doc
        };
    }
    restore_files(bundle, &entry, &dir)?;
    bundle.save()?;
    fs::remove_dir_all(dir)?;
    Ok(Some(entry))
}

pub struct Before {
    pub doc: Document,
    pub files: BTreeMap<String, Option<Vec<u8>>>,
}

pub fn before(bundle: &Bundle, seq: u64) -> Result<Before> {
    let chain: Vec<u64> = seqs(bundle, Stack::Undo)?.into_iter().filter(|s| *s >= seq).collect();
    if chain.last() != Some(&seq) {
        return Err(anyhow!("no undo step #{seq}"));
    }
    chain.iter().try_fold(
        Before {
            doc: bundle.doc.clone(),
            files: BTreeMap::new(),
        },
        |mut state, &s| {
            let entry = read_entry(bundle, Stack::Undo, s)?;
            let dir = entry_dir(bundle, Stack::Undo, s);
            state.doc = restored(&state.doc, &entry, &dir)?;
            entry.files.iter().try_for_each(|rel| {
                let bytes = if entry.absent.contains(rel) {
                    None
                } else {
                    Some(zstd::decode_all(&fs::read(dir.join("files").join(rel))?[..])?)
                };
                state.files.insert(rel.clone(), bytes);
                Ok::<_, anyhow::Error>(())
            })?;
            Ok(state)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Layer;
    use crate::ops::parse_lines;
    use crate::synth::Synth;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn undo_and_redo_walk_edits_and_stroke_files() {
        let dir = scratch("walk");
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        bundle
            .apply(&parse_lines(r#"{"op":"add","layer":{"kind":"vector","name":"shapes"}}"#).unwrap())
            .unwrap();
        let with_layer = bundle.doc.layers.len();
        let strokes = Synth::canvas(64.0, 64.0).strokes(3, 1);
        checkpoint(
            &bundle,
            &bundle.doc,
            "test",
            "paint",
            &[bundle.strokes_path("l1", "c1")],
        )
        .unwrap();
        bundle
            .write_marks("l1", "c1", &crate::stroke::marks(strokes.clone()))
            .unwrap();
        bundle.save().unwrap();
        assert_eq!(entries(&bundle, Stack::Undo).unwrap().len(), 2);
        let made = entries(&bundle, Stack::Undo).unwrap()[0].clone();
        let undone = step(&mut bundle, Stack::Undo).unwrap().unwrap();
        assert_eq!(undone.summary, "paint");
        assert!(!bundle.strokes_path("l1", "c1").exists());
        let waiting = entries(&bundle, Stack::Redo).unwrap()[0].clone();
        assert_eq!((waiting.author.as_str(), waiting.at_ms), ("test", made.at_ms));
        let undone = step(&mut bundle, Stack::Undo).unwrap().unwrap();
        assert!(undone.summary.contains("add layer"));
        assert_eq!(bundle.doc.layers.len(), with_layer - 1);
        assert!(step(&mut bundle, Stack::Undo).unwrap().is_none());
        step(&mut bundle, Stack::Redo).unwrap().unwrap();
        step(&mut bundle, Stack::Redo).unwrap().unwrap();
        assert_eq!(bundle.doc.layers.len(), with_layer);
        assert_eq!(bundle.read_strokes("l1", "c1").unwrap(), strokes);
        let reopened = Bundle::open(&dir).unwrap();
        assert_eq!(reopened.doc.rev, bundle.doc.rev);
        bundle
            .apply(&parse_lines(r#"{"op":"set","id":"l1","name":"x"}"#).unwrap())
            .unwrap();
        assert!(entries(&bundle, Stack::Redo).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn steps_store_changes_and_old_snapshot_steps_still_undo() {
        let dir = scratch("patch");
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::vector("v", "shapes", "c1", Vec::new()));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let big: String = (0..400)
            .map(|i| {
                format!(r#"{{"op":"add","node":{{"id":"n{i}","type":"rect","x":{i},"y":0,"w":5,"h":5}},"in":"v"}}"#)
            })
            .collect::<Vec<_>>()
            .join("\n");
        bundle.apply(&parse_lines(&big).unwrap()).unwrap();
        let legacy_doc = bundle.doc.clone();
        let legacy = Entry {
            seq: 50,
            rev: bundle.doc.rev,
            author: "m1".into(),
            at_ms: 0,
            summary: "old style".into(),
            files: Vec::new(),
            absent: Vec::new(),
            patch: false,
        };
        let legacy_dir = entry_dir(&bundle, Stack::Undo, 50);
        fs::create_dir_all(&legacy_dir).unwrap();
        let text = serde_json::to_string(&legacy_doc).unwrap();
        fs::write(
            legacy_dir.join("doc.json.zst"),
            zstd::encode_all(text.as_bytes(), 3).unwrap(),
        )
        .unwrap();
        fs::write(legacy_dir.join("meta.json"), serde_json::to_string(&legacy).unwrap()).unwrap();
        bundle
            .apply(&parse_lines(r#"{"op":"set","id":"n7","x":99}"#).unwrap())
            .unwrap();
        let newest = seqs(&bundle, Stack::Undo).unwrap()[0];
        assert_eq!(newest, 51);
        let packed = fs::metadata(entry_dir(&bundle, Stack::Undo, newest).join("patch.json.zst"))
            .unwrap()
            .len();
        assert!(packed < 200, "{packed} bytes");
        bundle.apply(&parse_lines(r#"{"op":"rm","id":"n3"}"#).unwrap()).unwrap();
        let plain = |d: &Document| Document { rev: 0, ..d.clone() };
        assert_eq!(plain(&super::before(&bundle, newest).unwrap().doc), plain(&legacy_doc));
        step(&mut bundle, Stack::Undo).unwrap();
        step(&mut bundle, Stack::Undo).unwrap();
        assert_eq!(plain(&bundle.doc), plain(&legacy_doc));
        bundle.doc.layers[0].name = "drifted".into();
        bundle.save().unwrap();
        let undone = step(&mut bundle, Stack::Undo).unwrap().unwrap();
        assert_eq!(undone.summary, "old style");
        assert_eq!(plain(&bundle.doc), plain(&legacy_doc));
        step(&mut bundle, Stack::Redo).unwrap();
        assert_eq!(bundle.doc.layers[0].name, "drifted");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_step_that_no_longer_fits_fails_and_changes_nothing() {
        let dir = scratch("stale");
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::vector("v", "shapes", "c1", Vec::new()));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        bundle
            .apply(&parse_lines(r#"{"op":"set","id":"v","name":"one"}"#).unwrap())
            .unwrap();
        bundle.doc.layers[0].name = "edited outside history".into();
        bundle.save().unwrap();
        let rev = bundle.doc.rev;
        let err = step(&mut bundle, Stack::Undo).unwrap_err();
        assert!(format!("{err:#}").contains("no longer matches"), "{err:#}");
        assert_eq!(Bundle::open(&dir).unwrap().doc.rev, rev);
        assert!(entries(&bundle, Stack::Redo).unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore = "timing: LANDSCAPE=<copy of landscape.klad> cargo test --release -p klad-core undo_timing -- --ignored --nocapture"]
    fn undo_timing() {
        let path = std::env::var("LANDSCAPE").expect("LANDSCAPE");
        let mut bundle = Bundle::open(&path).unwrap();
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let tree = bundle
            .doc
            .layer("trees")
            .and_then(|l| l.cels().first())
            .and_then(|c| c.nodes.first())
            .map(|n| n.id.clone())
            .unwrap();
        (0..3).for_each(|round| {
            let line = format!(r#"{{"op":"set","id":"{tree}","transform.x":{}}}"#, 10 * (round + 1));
            let t = std::time::Instant::now();
            bundle.apply_as(&parse_lines(&line).unwrap(), "editor").unwrap();
            let edit = ms(t);
            let t = std::time::Instant::now();
            step(&mut bundle, Stack::Undo).unwrap().unwrap();
            let undo = ms(t);
            let t = std::time::Instant::now();
            step(&mut bundle, Stack::Redo).unwrap().unwrap();
            println!(
                "move {tree}: edit {edit:.1} ms, undo {undo:.1} ms, redo {:.1} ms",
                ms(t)
            );
        });
    }
}
