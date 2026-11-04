use crate::brushes::{self, BrushDef};
use crate::doc::{Document, Layer, Loc};
use crate::history::checkpoint;
use crate::markedit::{self, LassoAction, MarkChange};
use crate::migrate::migrate;
use crate::ops::{Applied, ClipEdit, FillSpec, Op, PendingMarks, apply_all};
use crate::stroke::{Mark, Stroke, decode_cel, encode_cel, strokes_of};
use crate::timing::{CelAction, FrameSpec};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStamp {
    pub rev: u64,
    pub author: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Blank {
    pub width: u32,
    pub height: u32,
    pub layers: usize,
    pub background: Option<String>,
    pub fps: u32,
    pub frames: u32,
    pub sketch: bool,
}

impl Default for Blank {
    fn default() -> Self {
        Self {
            width: 2048,
            height: 2048,
            layers: 1,
            background: None,
            fps: 12,
            frames: 1,
            sketch: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changed {
    pub disk: u64,
    pub here: u64,
}

impl std::fmt::Display for Changed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the document changed while this edit was being made (rev {} on disk, {} when it started); nothing was written, run it again",
            self.disk, self.here
        )
    }
}

impl std::error::Error for Changed {}

pub fn disk_rev(root: &Path) -> Option<u64> {
    let path = root.join("doc.json");
    let mut head = vec![0u8; 4096];
    let read = {
        use std::io::Read;
        fs::File::open(&path).ok()?.read(&mut head).ok()?
    };
    let text = String::from_utf8_lossy(&head[..read]);
    let quick = text.find("\"rev\"").and_then(|at| {
        let rest = text[at + 5..].trim_start().strip_prefix(':')?.trim_start();
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let end = rest[digits.len()..].chars().next()?;
        (!digits.is_empty() && matches!(end, ',' | '\n' | ' ' | '\r' | '}')).then(|| digits.parse().ok())?
    });
    quick.or_else(|| {
        let text = fs::read_to_string(&path).ok()?;
        serde_json::from_str::<serde_json::Value>(&text).ok()?["rev"].as_u64()
    })
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub struct Bundle {
    pub root: PathBuf,
    pub doc: Document,
}

fn name_parts(doc: &mut Document, pending: &[PendingMarks]) {
    let source_part = |doc: &Document, cel: &str, id: u32| -> Option<String> {
        match doc.locate(cel) {
            Some(Loc::Cel(path, c)) => doc.layer_at(&path).cels()[c].part_of(id).map(String::from),
            _ => None,
        }
    };
    pending.iter().for_each(|p| {
        let named: Vec<(String, Vec<u32>)> = p
            .changes
            .iter()
            .flat_map(|change| match change {
                MarkChange::Part { name, ids, .. } => vec![(name.clone(), ids.clone())],
                MarkChange::Paste { made, .. } => made
                    .iter()
                    .filter_map(|c| source_part(doc, &c.cel, c.from).map(|name| (name, vec![c.to])))
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        if named.is_empty() {
            return;
        }
        if let Some(Loc::Cel(path, c)) = doc.locate(&p.cel)
            && let Some(cels) = doc.layer_at_mut(&path).cels_mut()
        {
            named.iter().for_each(|(name, ids)| cels[c].set_part(name, ids));
        }
    });
}

impl Bundle {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        remember(&root);
        let path = root.join("doc.json");
        let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let raw: serde_json::Value =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let migrated = migrate(raw).with_context(|| format!("upgrading {}", path.display()))?;
        let doc: Document =
            serde_json::from_value(migrated.value.clone()).with_context(|| format!("reading {}", path.display()))?;
        doc.validate().with_context(|| format!("checking {}", path.display()))?;
        let mut bundle = Self { root, doc };
        if migrated.changed() {
            migrated.moves.iter().try_for_each(|m| {
                let from = bundle.root.join(&m.from);
                if from.exists() {
                    write_atomic(&bundle.root.join(&m.to), &fs::read(&from)?)?;
                }
                Ok::<_, anyhow::Error>(())
            })?;
            if migrated.embed_m1_brushes {
                bundle.embed_m1_brushes()?;
            }
            bundle.save()?;
            migrated.moves.iter().for_each(|m| {
                let _ = fs::remove_file(bundle.root.join(&m.from));
            });
            eprintln!("upgraded {} to {}", bundle.root.display(), bundle.doc.format);
        }
        Ok(bundle)
    }

    pub fn brush_path(&self, id: &str) -> PathBuf {
        self.root.join("brushes").join(format!("{id}.json"))
    }

    pub fn brushes(&self) -> Result<BTreeMap<String, BrushDef>> {
        let dir = self.root.join("brushes");
        if !dir.exists() {
            return Ok(BTreeMap::new());
        }
        fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .map(|path| {
                let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let text = fs::read_to_string(&path)?;
                let def = BrushDef::parse(&text).with_context(|| format!("brush {}", path.display()))?;
                Ok((id, def))
            })
            .collect()
    }

    pub fn brush(&self, id: &str) -> Result<Option<BrushDef>> {
        let path = self.brush_path(id);
        if path.exists() {
            let def = BrushDef::parse(&fs::read_to_string(&path)?).with_context(|| format!("brush {id}"))?;
            return Ok(Some(def));
        }
        Ok(brushes::library(id))
    }

    pub fn embed_brush(&self, id: &str, def: &BrushDef) -> Result<bool> {
        let path = self.brush_path(id);
        if path.exists() {
            return Ok(false);
        }
        if !crate::doc::valid_id(id) {
            bail!("brush id {id:?} may only use letters, digits, - and _");
        }
        def.validate().with_context(|| format!("brush {id}"))?;
        write_atomic(&path, def.to_json()?.as_bytes())?;
        Ok(true)
    }

    pub fn brush_ids(&self) -> Result<Vec<String>> {
        let embedded = self.brushes()?;
        let mut ids: Vec<String> = embedded.keys().cloned().collect();
        ids.extend(
            brushes::library_ids()
                .into_iter()
                .filter(|id| !embedded.contains_key(*id))
                .map(String::from),
        );
        Ok(ids)
    }

    fn used_brushes(&self) -> Result<BTreeSet<String>> {
        self.doc
            .paint_cels()
            .iter()
            .try_fold(BTreeSet::new(), |mut used, (layer, cel)| {
                used.extend(self.read_strokes(&layer.id, &cel.id)?.into_iter().map(|s| s.brush));
                Ok(used)
            })
    }

    fn embed_m1_brushes(&self) -> Result<()> {
        self.used_brushes()?.iter().try_for_each(|id| {
            match brushes::legacy_m1(id).or_else(|| brushes::library(id)) {
                Some(def) => {
                    self.embed_brush(id, &def)?;
                }
                None => eprintln!("{}: strokes use unknown brush {id:?}", self.root.display()),
            }
            Ok(())
        })
    }

    pub fn embed_brushes_of(&self, marks: &[Mark]) -> Result<()> {
        self.embed_library(strokes_of(marks).map(|s| s.brush.as_str()))
    }

    fn embed_library<'a>(&self, ids: impl Iterator<Item = &'a str>) -> Result<()> {
        let wanted: BTreeSet<&str> = ids.collect();
        let missing = wanted
            .into_iter()
            .filter(|id| !self.brush_path(id).exists())
            .map(|id| match brushes::library(id) {
                Some(def) => Ok((id, def)),
                None => bail!("unknown brush {id:?}: brushes are {}", self.brush_ids()?.join(", ")),
            })
            .collect::<Result<Vec<_>>>()?;
        missing
            .iter()
            .try_for_each(|(id, def)| self.embed_brush(id, def).map(|_| ()))
    }

    pub fn create(root: impl AsRef<Path>, doc: Document) -> Result<Self> {
        doc.validate()?;
        let root = root.as_ref().to_path_buf();
        remember(&root);
        fs::create_dir_all(&root)?;
        write_atomic(&root.join(".gitignore"), b".cache/\n")?;
        let mut bundle = Self { root, doc };
        bundle.save()?;
        Ok(bundle)
    }

    pub fn blank(root: impl AsRef<Path>, blank: &Blank, author: &str) -> Result<Self> {
        let root = root.as_ref();
        if root.join("doc.json").exists() {
            bail!("{} already exists", root.display());
        }
        let mut doc = Document::new(blank.width, blank.height);
        doc.canvas.background = blank.background.clone();
        doc.timeline.fps = blank.fps;
        doc.timeline.frames = blank.frames;
        doc.layers = (1..=blank.layers)
            .map(|i| {
                let (layer, cel) = (doc.alloc_id('l'), doc.alloc_id('c'));
                Layer::paint(&layer, &format!("paint {i}"), &cel)
            })
            .collect();
        if blank.sketch {
            let (layer, cel) = (doc.alloc_id('l'), doc.alloc_id('c'));
            doc.layers.push(Layer::vector(&layer, "sketch", &cel, Vec::new()));
        }
        let bundle = Self::create(root, doc)?;
        bundle.stamp(author)?;
        Ok(bundle)
    }

    pub fn save(&mut self) -> Result<()> {
        self.doc.rev += 1;
        let mut text = serde_json::to_string_pretty(&self.doc)?;
        text.push('\n');
        write_atomic(&self.root.join("doc.json"), text.as_bytes())
    }

    pub fn strokes_path(&self, layer: &str, cel: &str) -> PathBuf {
        self.root.join("strokes").join(layer).join(format!("{cel}.strokes"))
    }

    pub fn read_marks(&self, layer: &str, cel: &str) -> Result<Vec<Mark>> {
        let path = self.strokes_path(layer, cel);
        if !path.exists() {
            return Ok(Vec::new());
        }
        decode_cel(&fs::read(&path)?).with_context(|| format!("reading {}", path.display()))
    }

    pub fn read_strokes(&self, layer: &str, cel: &str) -> Result<Vec<Stroke>> {
        Ok(strokes_of(&self.read_marks(layer, cel)?).cloned().collect())
    }

    pub fn write_marks(&self, layer: &str, cel: &str, marks: &[Mark]) -> Result<u64> {
        self.embed_brushes_of(marks)?;
        let bytes = encode_cel(marks)?;
        write_atomic(&self.strokes_path(layer, cel), &bytes)?;
        Ok(bytes.len() as u64)
    }

    pub fn apply(&mut self, ops: &[Op]) -> Result<Applied> {
        self.apply_as(ops, "cli")
    }

    pub fn apply_as(&mut self, ops: &[Op], author: &str) -> Result<Applied> {
        let (mut doc, mut applied) = apply_all(&self.doc, ops)?;
        self.embed_library(
            applied
                .marks
                .iter()
                .flat_map(|p| p.changes.iter().flat_map(MarkChange::brushes)),
        )?;
        let plan = applied.marks.clone();
        let written = applied
            .marks
            .iter_mut()
            .map(|pending| {
                let mut marks = self.read_marks(&pending.layer, &pending.cel)?;
                pending
                    .changes
                    .iter_mut()
                    .try_for_each(|change| self.apply_change(&plan, &mut marks, change, &pending.cel))?;
                Ok((pending.layer.clone(), pending.cel.clone(), marks))
            })
            .collect::<Result<Vec<_>>>()?;
        name_parts(&mut doc, &applied.marks);
        let touched: Vec<PathBuf> = written
            .iter()
            .map(|(layer, cel, _)| self.strokes_path(layer, cel))
            .chain(applied.dropped_cels.iter().map(|(l, c)| self.strokes_path(l, c)))
            .collect();
        checkpoint(self, &doc, author, &summarize(ops), &touched)?;
        self.doc = doc;
        written
            .iter()
            .try_for_each(|(layer, cel, marks)| self.write_marks(layer, cel, marks).map(|_| ()))?;
        self.save()?;
        applied.dropped_cels.iter().for_each(|(layer, cel)| {
            let _ = fs::remove_file(self.strokes_path(layer, cel));
        });
        Ok(applied)
    }

    pub fn apply_change(
        &self,
        plan: &[PendingMarks],
        marks: &mut Vec<Mark>,
        change: &mut MarkChange,
        cel: &str,
    ) -> Result<()> {
        match change {
            MarkChange::Copy { layer, cel, upto } => {
                *marks = self.marks_after(plan, layer, cel, *upto)?;
                Ok(())
            }
            MarkChange::Paste {
                sources,
                transform,
                cut,
                made,
            } => {
                let copies = sources
                    .iter()
                    .map(|source| {
                        let from = self.marks_after(plan, &source.layer, &source.cel, source.upto)?;
                        source
                            .ids
                            .iter()
                            .try_for_each(|id| match from.iter().any(|m| m.id() == *id) {
                                true => Ok(()),
                                false => bail!("{} does not exist", markedit::address(&source.cel, *id)),
                            })?;
                        Ok(from
                            .into_iter()
                            .filter(|m| source.ids.contains(&m.id()))
                            .map(|m| (source.cel.clone(), m))
                            .collect::<Vec<_>>())
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                *made = markedit::paste(marks, copies, transform.as_ref(), cut.as_deref())?;
                Ok(())
            }
            _ => markedit::apply(marks, change, cel),
        }
    }

    fn marks_after(&self, plan: &[PendingMarks], layer: &str, cel: &str, upto: usize) -> Result<Vec<Mark>> {
        let mut marks = self.read_marks(layer, cel)?;
        if let Some(pending) = plan.iter().find(|p| p.layer == layer && p.cel == cel) {
            pending
                .changes
                .iter()
                .take(upto)
                .try_for_each(|c| self.apply_change(plan, &mut marks, &mut c.clone(), cel))?;
        }
        Ok(marks)
    }

    pub fn replace_marks(&mut self, layer: &str, cel: &str, marks: &[Mark], author: &str, summary: &str) -> Result<()> {
        checkpoint(self, &self.doc, author, summary, &[self.strokes_path(layer, cel)])?;
        self.write_marks(layer, cel, marks)?;
        self.save()
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(".cache")
    }

    pub fn reload(&mut self) -> Result<()> {
        self.doc = Self::open(&self.root)?.doc;
        Ok(())
    }

    pub fn ensure_unchanged(&self) -> Result<()> {
        match disk_rev(&self.root) {
            Some(disk) if disk != self.doc.rev => Err(Changed {
                disk,
                here: self.doc.rev,
            }
            .into()),
            _ => Ok(()),
        }
    }

    pub fn stamp_path(&self) -> PathBuf {
        self.cache_dir().join("sync.json")
    }

    pub fn stamp(&self, author: &str) -> Result<SyncStamp> {
        let stamp = SyncStamp {
            rev: self.doc.rev,
            author: author.to_string(),
            at_ms: now_ms(),
        };
        write_atomic(&self.stamp_path(), serde_json::to_string(&stamp)?.as_bytes())?;
        Ok(stamp)
    }

    pub fn read_stamp(&self) -> Option<SyncStamp> {
        serde_json::from_str(&fs::read_to_string(self.stamp_path()).ok()?).ok()
    }
}

pub fn summarize(ops: &[Op]) -> String {
    let parts: Vec<String> = ops
        .iter()
        .map(|op| match op {
            Op::AddLayer { layer, .. } => format!("add layer {}", layer["kind"].as_str().unwrap_or("")),
            Op::AddNode { node, .. } => format!("add {}", node["type"].as_str().unwrap_or("node")),
            Op::AddCel { layer, .. } => format!("add cel to {layer}"),
            Op::AddMask { layer, .. } => format!("add mask to {layer}"),
            Op::Lasso(spec) => format!(
                "lasso {}on {}",
                match (spec.cut, &spec.action) {
                    (true, LassoAction::Remove) => "cut and delete ",
                    (false, LassoAction::Remove) => "delete ",
                    (true, _) => "cut and move ",
                    (false, _) => "move ",
                },
                spec.layer
            ),
            Op::Set { id, .. } => format!("set {id}"),
            Op::Remove { ids } => format!("rm {}", ids.join(" ")),
            Op::Move { id, .. } => format!("mv {id}"),
            Op::Stroke(spec) => format!("stroke on {}", spec.layer),
            Op::Fill(FillSpec {
                like: Some(cel), layer, ..
            }) => format!("fills on {layer} like {cel}"),
            Op::Fill(spec) => format!("fill on {}", spec.layer),
            Op::Palette(_) => "palette".to_string(),
            Op::Transform { ids, .. } => format!("transform {}", ids.join(" ")),
            Op::Path { id, .. } => format!("path {id}"),
            Op::Copy(spec) => format!(
                "copy {} mark{} to {}",
                spec.ids.len(),
                if spec.ids.len() == 1 { "" } else { "s" },
                match &spec.to {
                    crate::ops::CopyTarget::Cel(cel) => cel.clone(),
                    crate::ops::CopyTarget::Frame { frame, .. } => format!("frame {frame}"),
                }
            ),
            Op::Timeline { origin: Some(_), .. } => "timeline origin".to_string(),
            Op::Timeline { .. } => "timeline".to_string(),
            Op::Atmosphere(Some(_)) => "atmosphere".to_string(),
            Op::Atmosphere(None) => "atmosphere off".to_string(),
            Op::Clip(edit) => match edit {
                ClipEdit::Add(clip) => format!("add clip {}", clip.name),
                ClipEdit::Set { name, .. } => format!("set clip {name}"),
                ClipEdit::Remove { name } => format!("rm clip {name}"),
            },
            Op::Export { .. } => "export folder".to_string(),
            Op::Frames(FrameSpec { insert, at, count, .. }) => format!(
                "{} {count} frame{} at {at}",
                if *insert { "insert" } else { "remove" },
                if *count == 1 { "" } else { "s" }
            ),
            Op::Duplicate { layer } => format!("duplicate layer {layer}"),
            Op::Part { ids, name } if name.is_empty() => format!("take {} out of their part", ids.join(", ")),
            Op::Part { ids, name } => format!("part {name}: {}", ids.join(", ")),
            Op::Ink(spec) => format!("ink {} on {}", spec.from.join(", "), spec.layer),
            Op::Light(spec) => format!("light {}", spec.from.join(", ")),
            Op::Hatch(spec) => format!("hatch on {}", spec.layer),
            Op::Paint(spec) => format!("paint on {}", spec.layer),
            Op::Scatter(spec) => format!("scatter on {}", spec.layer),
            Op::Inbetween(spec) if spec.redo => "redo in-betweens".to_string(),
            Op::Inbetween(spec) => format!(
                "in-between {} frames {} to {}",
                spec.layers.join(","),
                spec.from.unwrap_or_default(),
                spec.to.unwrap_or_default()
            ),
            Op::Retime(spec) => format!(
                "retime frames {} to {} {}",
                spec.from,
                spec.to,
                match spec.action {
                    crate::retime::Retime::On(n) => format!("on {n}s"),
                    crate::retime::Retime::Fit(n) => format!("into {n} frames"),
                    crate::retime::Retime::Reverse => "reversed".to_string(),
                    crate::retime::Retime::Pingpong => "ping-pong".to_string(),
                }
            ),
            Op::Cel(spec) => match &spec.action {
                CelAction::New { layer } => format!("new drawing on {layer}"),
                CelAction::Dup { cel } => format!("duplicate {cel}"),
                CelAction::Link { cel } => format!("link {cel}"),
                CelAction::Unlink { cel } => format!("unlink {cel}"),
                CelAction::Move { cel } => format!("move {cel} to frame {}", spec.frame.unwrap_or(0)),
            },
        })
        .collect();
    let grouped = parts.iter().fold(Vec::<(String, usize)>::new(), |mut acc, part| {
        match acc.last_mut() {
            Some((last, n)) if last == part => *n += 1,
            _ => acc.push((part.clone(), 1)),
        }
        acc
    });
    let text = grouped
        .iter()
        .map(|(part, n)| if *n > 1 { format!("{part} x{n}") } else { part.clone() })
        .collect::<Vec<_>>()
        .join(", ");
    if text.chars().count() > 120 {
        format!("{}...", text.chars().take(117).collect::<String>())
    } else {
        text
    }
}

static OPENED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

fn remember(root: &Path) {
    let mut opened = OPENED.lock().unwrap_or_else(|e| e.into_inner());
    if !opened.iter().any(|r| r == root) {
        opened.push(root.to_path_buf());
    }
}

pub fn opened() -> Vec<PathBuf> {
    OPENED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}-{n}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Synth;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-bundle-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_writer_finds_out_when_the_document_changed_under_it() {
        use crate::ops::parse_lines;
        let dir = scratch("changed");
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        doc.next_id = 10;
        let mut first = Bundle::create(&dir, doc).unwrap();
        assert_eq!(disk_rev(&dir), Some(first.doc.rev));
        let mut second = Bundle::open(&dir).unwrap();
        assert!(first.ensure_unchanged().is_ok());
        second
            .apply(&parse_lines(r#"{"op":"stroke","layer":"l1","path":"M1 1 L60 60"}"#).unwrap())
            .unwrap();
        assert_eq!(disk_rev(&dir), Some(second.doc.rev));
        let refused = first.ensure_unchanged().unwrap_err();
        assert_eq!(
            refused.downcast_ref::<Changed>(),
            Some(&Changed {
                disk: second.doc.rev,
                here: first.doc.rev
            })
        );
        first.reload().unwrap();
        assert!(first.ensure_unchanged().is_ok());
        fs::write(dir.join("doc.json"), "{\n  \"rev\":\n 99, \"format\": \"x\"}").unwrap();
        assert_eq!(disk_rev(&dir), Some(99));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_moved_drawing_pushes_the_drawings_it_lands_on_and_leaves_its_frames_blank() {
        use crate::ops::parse_lines;
        let dir = scratch("move");
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = 12;
        let mut layer = Layer::paint("l1", "ink", "c1");
        if let Some(cels) = layer.cels_mut() {
            cels[0].hold = Some(2);
            cels.push(crate::doc::Cel::at("c2", 2));
            cels[1].hold = Some(2);
            cels.push(crate::doc::Cel::at("c3", 4));
            cels[2].hold = Some(3);
            cels.push(crate::doc::Cel::at("c4", 7));
        }
        doc.layers.push(layer);
        doc.next_id = 10;
        let mut b = Bundle::create(&dir, doc).unwrap();
        let starts = |b: &Bundle| -> Vec<(String, u32, Option<u32>)> {
            b.doc
                .layer("l1")
                .unwrap()
                .cels()
                .iter()
                .map(|c| (c.id.clone(), c.frame, c.hold))
                .collect()
        };
        b.apply(&parse_lines(r#"{"op":"cel","move":"c1","frame":9}"#).unwrap())
            .unwrap();
        assert_eq!(starts(&b)[0], ("c2".into(), 2, Some(2)));
        assert!(starts(&b).contains(&("c1".into(), 9, Some(2))));
        assert!(starts(&b).contains(&("c4".into(), 7, Some(2))), "{:?}", starts(&b));
        b.apply(&parse_lines(r#"{"op":"cel","move":"c1","frame":5}"#).unwrap())
            .unwrap();
        let after = starts(&b);
        assert!(after.contains(&("c3".into(), 4, Some(1))), "{after:?}");
        assert!(after.contains(&("c1".into(), 5, Some(2))), "{after:?}");
        assert!(after.contains(&("c4".into(), 7, Some(2))), "{after:?}");
        b.apply(&parse_lines(r#"{"op":"cel","move":"c1","frame":6}"#).unwrap())
            .unwrap();
        let after = starts(&b);
        assert!(after.contains(&("c1".into(), 6, Some(2))), "{after:?}");
        assert!(after.contains(&("c4".into(), 8, Some(2))), "{after:?}");
        (0..3).for_each(|_| {
            crate::history::step(&mut b, crate::history::Stack::Undo).unwrap();
        });
        assert_eq!(starts(&b)[0], ("c1".into(), 0, Some(2)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parts_name_marks_and_follow_them_through_dup_unlink_and_copy() {
        use crate::ops::parse_lines;
        let dir = scratch("parts");
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = 6;
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        doc.next_id = 10;
        let mut b = Bundle::create(&dir, doc).unwrap();
        let run = |b: &mut Bundle, text: &str| b.apply(&parse_lines(text).unwrap()).map(|_| ());
        let parts = |b: &Bundle, cel: &str| -> Vec<(String, Vec<u32>)> {
            match b.doc.locate(cel) {
                Some(Loc::Cel(path, c)) => b.doc.layer_at(&path).cels()[c].parts.clone().into_iter().collect(),
                _ => panic!("no cel {cel}"),
            }
        };
        let named = |pairs: &[(&str, &[u32])]| -> Vec<(String, Vec<u32>)> {
            pairs.iter().map(|(n, ids)| (n.to_string(), ids.to_vec())).collect()
        };
        run(
            &mut b,
            r#"{"op":"stroke","layer":"l1","path":"M1 1 L30 30","part":"leaf"}
{"op":"stroke","layer":"l1","path":"M30 1 L1 30"}
{"op":"stroke","layer":"l1","path":"M5 5 L9 9","part":"leaf"}
{"op":"part","ids":["c1:s2"],"name":"stem"}"#,
        )
        .unwrap();
        assert_eq!(parts(&b, "c1"), named(&[("leaf", &[1, 3]), ("stem", &[2])]));
        run(&mut b, r#"{"op":"part","ids":["c1:s2"],"name":"leaf"}"#).unwrap();
        assert_eq!(parts(&b, "c1"), named(&[("leaf", &[1, 3, 2])]));
        let refused = format!(
            "{:#}",
            run(&mut b, r#"{"op":"part","ids":["c1:s9"],"name":"x"}"#).unwrap_err()
        );
        assert!(refused.contains("c1:s9 does not exist"), "{refused}");
        run(
            &mut b,
            r#"{"op":"cel","dup":"c1","frame":2,"id":"c2"}
{"op":"cel","link":"c1","frame":4,"id":"c3"}"#,
        )
        .unwrap();
        run(&mut b, r#"{"op":"cel","unlink":"c3"}"#).unwrap();
        assert_eq!(parts(&b, "c2"), parts(&b, "c1"));
        assert_eq!(parts(&b, "c3"), parts(&b, "c1"));
        run(
            &mut b,
            r#"{"op":"part","ids":["c2:s3"],"name":""}
{"op":"copy","ids":["c1:s1","c1:s3"],"to":"c2","x":4}"#,
        )
        .unwrap();
        assert_eq!(parts(&b, "c2"), named(&[("leaf", &[1, 2, 4, 5])]));
        crate::history::step(&mut b, crate::history::Stack::Undo).unwrap();
        assert_eq!(parts(&b, "c2"), parts(&b, "c1"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_duplicated_layer_copies_its_drawings_links_and_mask_with_new_ids() {
        use crate::ops::parse_lines;
        let dir = scratch("duplicate");
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = 6;
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        doc.next_id = 10;
        let mut b = Bundle::create(&dir, doc).unwrap();
        b.apply(
            &parse_lines(
                r#"{"op":"stroke","layer":"l1","path":"M1 1 L60 60"}
{"op":"set","id":"c1","hold":2}
{"op":"cel","link":"c1","frame":3}
{"op":"add","mask":{"name":"m"},"in":"l1"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let made = b
            .apply(&parse_lines(r#"{"op":"duplicate","layer":"l1"}"#).unwrap())
            .unwrap();
        let copy = made.added[0].clone();
        let layers: Vec<String> = b.doc.layers.iter().map(|l| l.id.clone()).collect();
        assert_eq!(layers, ["l1".to_string(), copy.clone()]);
        let dup = b.doc.layer(&copy).unwrap().clone();
        assert_eq!(dup.name, "ink copy");
        let cels = dup.cels();
        assert_eq!(cels.len(), 2);
        assert_ne!(cels[0].id, "c1");
        assert_eq!(cels[1].link.as_deref(), Some(cels[0].id.as_str()));
        assert_eq!(
            b.read_marks(&copy, &cels[0].id).unwrap(),
            b.read_marks("l1", "c1").unwrap()
        );
        assert!(
            dup.mask
                .as_ref()
                .is_some_and(|m| m.id != b.doc.layer("l1").unwrap().mask.as_ref().unwrap().id)
        );
        b.doc.validate().unwrap();
        crate::history::step(&mut b, crate::history::Stack::Undo).unwrap();
        assert_eq!(b.doc.layers.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn drawings_copy_their_strokes_keep_ids_and_undo() {
        use crate::history::{Stack, step};
        use crate::ops::parse_lines;
        let dir = scratch("drawings");
        let mut doc = Document::new(256, 256);
        doc.timeline.frames = 4;
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let mut marks = crate::stroke::marks(Synth::canvas(256.0, 256.0).strokes(3, 2));
        marks[1].set_id(7);
        bundle.write_marks("l1", "c1", &marks).unwrap();
        let ops = parse_lines(
            r#"{"op":"stroke","layer":"l1","path":"M10 10 L90 90"}
{"op":"cel","dup":"c1","frame":2}
{"op":"stroke","layer":"l1","path":"M20 10 L20 90"}"#,
        )
        .unwrap();
        let applied = bundle.apply(&ops).unwrap();
        let copy = applied.added[0].clone();
        let source = bundle.read_marks("l1", "c1").unwrap();
        let copied = bundle.read_marks("l1", &copy).unwrap();
        assert_eq!(source.len(), 5);
        assert_eq!(copied, source[..4].to_vec());
        assert_eq!(copied.iter().map(Mark::id).collect::<Vec<_>>(), [0, 7, 2, 8]);
        bundle
            .apply(
                &parse_lines(&format!(
                    r#"{{"op":"cel","link":"{copy}","frame":3}}
{{"op":"rm","id":"{copy}"}}"#
                ))
                .unwrap(),
            )
            .unwrap();
        let heir = bundle.doc.layer("l1").unwrap().cels()[1].clone();
        assert_eq!((heir.frame, heir.link.clone()), (3, None));
        assert_eq!(bundle.read_marks("l1", &heir.id).unwrap(), copied);
        assert!(!bundle.strokes_path("l1", &copy).exists());
        step(&mut bundle, Stack::Undo).unwrap();
        assert_eq!(bundle.read_marks("l1", &copy).unwrap(), copied);
        assert!(!bundle.strokes_path("l1", &heir.id).exists());
        step(&mut bundle, Stack::Undo).unwrap();
        assert!(!bundle.strokes_path("l1", &copy).exists());
        assert_eq!(bundle.read_marks("l1", "c1").unwrap(), marks);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copies_take_new_ids_move_on_the_way_and_undo_in_one_step() {
        use crate::history::{Stack, step};
        use crate::markedit::{Copied, MarkChange};
        use crate::ops::parse_lines;
        let dir = scratch("copies");
        let mut doc = Document::new(256, 256);
        doc.timeline.frames = 4;
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        doc.layers.push(Layer::paint("l2", "other", "c2"));
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let ops = parse_lines(
            r#"{"op":"set","id":"c1","hold":1}
{"op":"stroke","layer":"l1","path":"M10 10 L90 10"}
{"op":"stroke","layer":"l1","path":"M10 40 L90 40"}
{"op":"stroke","layer":"l1","path":"M10 70 L90 70"}"#,
        )
        .unwrap();
        bundle.apply(&ops).unwrap();
        let source = bundle.read_marks("l1", "c1").unwrap();
        let copy = parse_lines(
            r#"{"op":"copy","ids":["c1:s3","c1:s1"],"frame":2,"x":5,"y":-4}
{"op":"stroke","layer":"l1","path":"M50 50 L60 60"}
{"op":"copy","ids":["c1:s4"],"to":"c2"}"#,
        )
        .unwrap();
        let applied = bundle.apply(&copy).unwrap();
        let target = bundle.doc.layer("l1").unwrap().cels()[1].clone();
        assert_eq!((target.frame, applied.added.clone()), (2, vec![target.id.clone()]));
        let made: Vec<Copied> = applied
            .marks
            .iter()
            .flat_map(|p| p.changes.iter())
            .flat_map(|c| match c {
                MarkChange::Paste { made, .. } => made.clone(),
                _ => Vec::new(),
            })
            .collect();
        let pair = |cel: &str, from: u32, to: u32| Copied {
            cel: cel.to_string(),
            from,
            to,
        };
        assert_eq!(made, [pair("c1", 1, 1), pair("c1", 3, 2), pair("c1", 4, 1)]);
        let copied = bundle.read_marks("l1", &target.id).unwrap();
        let first = copied[0].as_stroke().unwrap().samples[0].pos();
        assert_eq!((copied.len(), first), (2, (15.0, 6.0)));
        assert_eq!(copied[1].as_stroke().unwrap().samples[0].pos(), (15.0, 66.0));
        let other = bundle.read_marks("l2", "c2").unwrap();
        assert_eq!(other[0].as_stroke().unwrap().samples, {
            let s = bundle.read_marks("l1", "c1").unwrap();
            s[3].as_stroke().unwrap().samples.clone()
        });
        step(&mut bundle, Stack::Undo).unwrap();
        assert_eq!(bundle.read_marks("l1", "c1").unwrap(), source);
        assert!(bundle.read_marks("l2", "c2").unwrap().is_empty());
        assert_eq!(bundle.doc.layer("l1").unwrap().cels().len(), 1);
        let cut =
            parse_lines(r#"{"op":"copy","ids":["c1:s1"],"to":"c1","cut":[[0,0],[50,0],[50,100],[0,100]]}"#).unwrap();
        bundle.apply(&cut).unwrap();
        let marks = bundle.read_marks("l1", "c1").unwrap();
        assert_eq!((marks.len(), marks[3].cuts().len()), (4, 1));
        let link = parse_lines(r#"{"op":"cel","link":"c1","frame":3,"id":"k"}"#).unwrap();
        bundle.apply(&link).unwrap();
        let refused = bundle.apply(&parse_lines(r#"{"op":"copy","ids":["c1:s1"],"to":"k"}"#).unwrap());
        assert!(format!("{:#}", refused.unwrap_err()).contains("copy into c1 instead"));
        let missing = bundle.apply(&parse_lines(r#"{"op":"copy","ids":["c1:s9"],"to":"c2"}"#).unwrap());
        assert!(format!("{:#}", missing.unwrap_err()).contains("c1:s9 does not exist"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bundle_round_trip() {
        let dir = scratch("trip");
        let mut doc = Document::new(256, 256);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let bundle = Bundle::create(&dir, doc.clone()).unwrap();
        let strokes = Synth::canvas(256.0, 256.0).strokes(10, 2);
        bundle
            .write_marks("l1", "c1", &crate::stroke::marks(strokes.clone()))
            .unwrap();
        let opened = Bundle::open(&dir).unwrap();
        assert_eq!(opened.doc.rev, 1);
        assert_eq!(opened.doc.layers, doc.layers);
        assert_eq!(opened.read_strokes("l1", "c1").unwrap(), strokes);
        assert!(opened.read_strokes("l1", "missing").unwrap().is_empty());
        let stamp = opened.stamp("cli").unwrap();
        assert_eq!(opened.read_stamp(), Some(stamp));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn v0_bundles_upgrade_on_open_and_keep_their_strokes() {
        let dir = scratch("upgrade");
        fs::create_dir_all(dir.join("strokes/ink")).unwrap();
        let v0 = r##"{"format":"draw-anything/0","rev":3,"canvas":{"width":128,"height":128},
            "layers":[{"id":"bg","name":"bg","kind":"vector","shapes":[{"type":"rect","x":0,"y":0,"w":128,"h":128,"fill":"#102030"}]},
                      {"id":"ink","name":"ink","kind":"paint"}]}"##;
        fs::write(dir.join("doc.json"), v0).unwrap();
        let strokes = Synth::canvas(128.0, 128.0).strokes(4, 1);
        fs::write(
            dir.join("strokes/ink/c0.strokes"),
            encode_cel(&crate::stroke::marks(strokes.clone())).unwrap(),
        )
        .unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        assert_eq!(bundle.doc.format, crate::doc::FORMAT);
        assert_eq!(bundle.doc.rev, 4);
        let cel = bundle.doc.layer("ink").unwrap().cels()[0].id.clone();
        assert_eq!(bundle.read_strokes("ink", &cel).unwrap(), strokes);
        assert!(!dir.join("strokes/ink/c0.strokes").exists());
        let again = Bundle::open(&dir).unwrap();
        assert_eq!(again.doc.rev, 4);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn m1_bundles_embed_the_m1_brushes_they_use() {
        let dir = scratch("m1");
        let mut doc = Document::new(64, 64);
        doc.format = "draw-anything/1".into();
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let text = serde_json::to_string(&doc).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("doc.json"), text).unwrap();
        let mut synth = Synth::canvas(64.0, 64.0);
        synth.brushes = vec!["ink".into(), "chalk".into()];
        fs::create_dir_all(dir.join("strokes/l1")).unwrap();
        fs::write(
            dir.join("strokes/l1/c1.strokes"),
            encode_cel(&crate::stroke::marks(synth.strokes(6, 3))).unwrap(),
        )
        .unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        assert_eq!(bundle.doc.format, crate::doc::FORMAT);
        let embedded = bundle.brushes().unwrap();
        assert_eq!(embedded.keys().collect::<Vec<_>>(), ["chalk", "ink"]);
        assert_eq!(embedded["ink"], brushes::legacy_m1("ink").unwrap());
        assert_ne!(embedded["ink"], brushes::library("ink").unwrap());
        assert_eq!(bundle.brush("marker").unwrap(), brushes::library("marker"));
        assert_eq!(Bundle::open(&dir).unwrap().doc.rev, bundle.doc.rev);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn strokes_embed_their_brush_on_first_use() {
        let dir = scratch("embed");
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let stroke = |brush: &str| {
            crate::ops::parse_lines(&format!(
                r#"{{"op":"stroke","layer":"l1","brush":"{brush}","path":"M1 1 L40 40"}}"#
            ))
            .unwrap()
        };
        bundle.apply(&stroke("marker")).unwrap();
        assert_eq!(bundle.brushes().unwrap()["marker"], brushes::library("marker").unwrap());
        let rev = bundle.doc.rev;
        let err = bundle.apply(&stroke("nope")).unwrap_err().to_string();
        assert!(err.contains("unknown brush") && err.contains("marker"), "{err}");
        assert_eq!(Bundle::open(&dir).unwrap().doc.rev, rev);
        assert_eq!(bundle.read_strokes("l1", "c1").unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn masks_clips_locks_and_lassos_go_through_ops() {
        let dir = scratch("masks");
        let mut doc = Document::new(128, 64);
        doc.layers.push(Layer::paint("l1", "flat", "c1"));
        doc.layers.push(Layer::paint("l2", "shade", "c2"));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let run = |bundle: &mut Bundle, text: &str| bundle.apply(&crate::ops::parse_lines(text).unwrap());
        let added = run(&mut bundle, r#"{"op":"add","mask":{},"in":"l2","hide":true}"#).unwrap();
        let mask = added.added[0].clone();
        let shade = bundle.doc.layer("l2").unwrap();
        let mask_cel = shade.mask.as_ref().unwrap().cels()[0].id.clone();
        assert_eq!(shade.mask.as_ref().unwrap().name, "mask");
        let hidden = bundle.read_marks(&mask, &mask_cel).unwrap();
        assert_eq!(hidden[0].as_fill().unwrap().coverage.pixels(), 128 * 64);
        assert!(run(&mut bundle, r#"{"op":"add","mask":{},"in":"l2"}"#).is_err());
        assert!(
            run(
                &mut bundle,
                r#"{"op":"mv","id":"l1","above":"MASK"}"#.replace("MASK", &mask).as_str()
            )
            .is_err()
        );
        assert!(run(&mut bundle, r#"{"op":"set","id":"l2","mask":null}"#).is_err());
        run(&mut bundle, r#"{"op":"set","id":"l2","clip":true,"lockAlpha":true}"#).unwrap();
        run(
            &mut bundle,
            r#"{"op":"stroke","layer":"l2","path":"M0 30 L120 30","size":6}
{"op":"stroke","layer":"l1","path":"M0 10 L120 10","size":6}"#,
        )
        .unwrap();
        assert!(bundle.read_marks("l2", "c2").unwrap()[0].locked());
        assert!(!bundle.read_marks("l1", "c1").unwrap()[0].locked());
        let line = format!(r#"{{"op":"stroke","layer":"{mask}","path":"M0 0 L10 10","brush":"eraser"}}"#);
        run(&mut bundle, &line).unwrap();
        assert_eq!(bundle.read_marks(&mask, &mask_cel).unwrap().len(), 2);
        let lasso = run(
            &mut bundle,
            r#"{"op":"lasso","layer":"l2","polygon":[[40,0],[80,0],[80,64],[40,64]],"cut":true,"x":0,"y":20}"#,
        )
        .unwrap();
        assert_eq!(lasso.marks[0].changes[0].ids(), vec![2]);
        let marks = bundle.read_marks("l2", "c2").unwrap();
        assert_eq!(marks.len(), 2);
        assert!(marks.iter().all(|m| m.locked() && m.cuts().len() == 1));
        assert!(
            run(
                &mut bundle,
                r#"{"op":"lasso","layer":"l2","polygon":[[40,0],[80,0],[80,8]],"rm":true}"#
            )
            .unwrap_err()
            .to_string()
            .contains("holds no strokes")
        );
        run(&mut bundle, &format!(r#"{{"op":"rm","id":"{mask}"}}"#)).unwrap();
        assert!(bundle.doc.layer("l2").unwrap().mask.is_none());
        assert!(!bundle.strokes_path(&mask, &mask_cel).exists());
        crate::history::step(&mut bundle, crate::history::Stack::Undo).unwrap();
        assert_eq!(bundle.read_marks(&mask, &mask_cel).unwrap().len(), 2);
        assert_eq!(
            Bundle::open(&dir).unwrap().doc.layer(&mask).map(|m| m.id.clone()),
            Some(mask)
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn apply_saves_and_drops_removed_stroke_files() {
        let dir = scratch("apply");
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        bundle
            .write_marks(
                "l1",
                "c1",
                &crate::stroke::marks(Synth::canvas(64.0, 64.0).strokes(2, 1)),
            )
            .unwrap();
        let ops = crate::ops::parse_lines(r#"{"op":"rm","id":"l1"}"#).unwrap();
        let applied = bundle.apply(&ops).unwrap();
        assert_eq!(applied.removed, vec!["l1"]);
        assert!(!bundle.strokes_path("l1", "c1").exists());
        assert_eq!(Bundle::open(&dir).unwrap().doc.rev, 2);
        fs::remove_dir_all(&dir).unwrap();
    }
}
