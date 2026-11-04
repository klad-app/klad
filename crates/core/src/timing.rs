use crate::doc::{Cel, Document, Kind, Layer, LayerContent, Loc, Node, valid_id};
use crate::ops::Applied;
use anyhow::{Result, anyhow, bail};
use std::collections::BTreeSet;

pub const OPEN: u32 = u32::MAX;

pub fn ends(cels: &[Cel]) -> Vec<u32> {
    cels.iter()
        .enumerate()
        .map(|(i, c)| {
            let next = cels.get(i + 1).map_or(OPEN, |n| n.frame);
            c.hold.map_or(next, |h| c.frame.saturating_add(h).min(next))
        })
        .collect()
}

pub fn exposure(layer: &Layer, cel: &str) -> Option<(u32, u32)> {
    let cels = layer.cels();
    let at = cels.iter().position(|c| c.id == cel)?;
    Some((cels[at].frame, ends(cels)[at]))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shift {
    Insert { at: u32, count: u32 },
    Room { at: u32, count: u32 },
    Remove { at: u32, count: u32 },
}

impl Shift {
    pub fn start(self, s: u32) -> u32 {
        match self {
            Shift::Insert { at, count } if s > at => s + count,
            Shift::Room { at, count } if s >= at => s + count,
            Shift::Remove { at, .. } if s < at => s,
            Shift::Remove { at, count } if s < at + count => at,
            Shift::Remove { count, .. } => s - count,
            _ => s,
        }
    }

    pub fn end(self, e: u32) -> u32 {
        match self {
            _ if e == OPEN => OPEN,
            Shift::Insert { at, count } | Shift::Room { at, count } if e > at => e + count,
            Shift::Remove { at, .. } if e <= at => e,
            Shift::Remove { at, count } if e <= at + count => at,
            Shift::Remove { count, .. } => e - count,
            _ => e,
        }
    }

    fn grows(self) -> Option<u32> {
        match self {
            Shift::Insert { count, .. } | Shift::Room { count, .. } => Some(count),
            Shift::Remove { .. } => None,
        }
    }
}

fn remap(cels: &[Cel], shift: Shift) -> (Vec<Cel>, Vec<Cel>) {
    let moved: Vec<(Cel, u32)> = cels
        .iter()
        .zip(ends(cels))
        .map(|(c, e)| {
            let start = shift.start(c.frame);
            (
                Cel {
                    frame: start,
                    ..c.clone()
                },
                shift.end(e),
            )
        })
        .collect();
    let (kept, gone): (Vec<_>, Vec<_>) = moved.into_iter().partition(|(c, e)| *e > c.frame);
    let starts: Vec<u32> = kept.iter().map(|(c, _)| c.frame).collect();
    let kept = kept
        .into_iter()
        .enumerate()
        .map(|(i, (cel, end))| {
            let next = starts.get(i + 1).copied().unwrap_or(OPEN);
            let hold = match cel.hold {
                None if end == next || end == OPEN => None,
                _ => Some(end - cel.frame),
            };
            Cel { hold, ..cel }
        })
        .collect();
    (kept, gone.into_iter().map(|(c, _)| c).collect())
}

pub fn hand_over(layer: &mut Layer, gone: Vec<Cel>, out: &mut Applied) {
    let (id, paint) = (layer.id.clone(), layer.kind() == Kind::Paint);
    let Some(cels) = layer.cels_mut() else {
        return;
    };
    gone.into_iter().filter(|g| g.link.is_none()).for_each(|drawing| {
        let users: Vec<usize> = cels
            .iter()
            .enumerate()
            .filter(|(_, c)| c.link.as_deref() == Some(drawing.id.as_str()))
            .map(|(i, _)| i)
            .collect();
        if let Some((&heir, rest)) = users.split_first() {
            let heir_id = cels[heir].id.clone();
            cels[heir].link = None;
            rest.iter().for_each(|&i| cels[i].link = Some(heir_id.clone()));
            if paint {
                out.copy(&id, &heir_id, &id, &drawing.id);
            } else {
                cels[heir].nodes = drawing.nodes;
            }
        }
        if paint {
            out.dropped_cels.push((id.clone(), drawing.id));
        }
    });
}

fn shift_layer(layer: &mut Layer, shift: Shift, out: &mut Applied) {
    if let Some(cels) = layer.cels_mut() {
        let (kept, gone) = remap(cels, shift);
        *cels = kept;
        out.removed.extend(gone.iter().map(|c| c.id.clone()));
        hand_over(layer, gone, out);
    }
    if let Some(mask) = layer.mask.as_deref_mut() {
        shift_layer(mask, shift, out);
    }
    if let LayerContent::Group { children } = &mut layer.content {
        children.iter_mut().for_each(|c| shift_layer(c, shift, out));
    }
}

fn reach(layer: &Layer, frames: u32, count: u32) -> u32 {
    let own = layer.cels().last().map_or(0, |c| match c.hold {
        None => frames + count,
        Some(h) => c.frame + h,
    });
    layer
        .mask
        .iter()
        .map(|m| reach(m, frames, count))
        .chain(layer.children().iter().map(|c| reach(c, frames, count)))
        .fold(own, u32::max)
}

fn grow_for(doc: &mut Document, paths: &[Vec<usize>], count: u32) {
    let before = doc.timeline.frames;
    let need = paths
        .iter()
        .map(|p| reach(doc.layer_at(p), before, count))
        .max()
        .unwrap_or(0);
    doc.timeline.frames = before.max(need);
}

fn layer_path(doc: &Document, key: &str) -> Result<Vec<usize>> {
    let id = doc.find_layer(key)?.id.clone();
    match doc.locate(&id) {
        Some(Loc::Layer(path)) => Ok(path),
        _ => bail!("{key} is not a layer"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameSpec {
    pub insert: bool,
    pub at: u32,
    pub count: u32,
    pub layers: Option<Vec<String>>,
}

pub fn frames(doc: &mut Document, spec: &FrameSpec, out: &mut Applied) -> Result<()> {
    if spec.count == 0 {
        bail!("count must be at least 1");
    }
    let shift = if spec.insert {
        Shift::Insert {
            at: spec.at,
            count: spec.count,
        }
    } else {
        Shift::Remove {
            at: spec.at,
            count: spec.count,
        }
    };
    match &spec.layers {
        None => {
            doc.layers.iter_mut().for_each(|l| shift_layer(l, shift, out));
            doc.timeline.clips.retain_mut(|clip| {
                let (from, end) = (shift.start(clip.from), shift.end(clip.to + 1));
                clip.from = from;
                clip.to = end.saturating_sub(1);
                end > from
            });
            doc.timeline.frames = match shift.grows() {
                Some(count) => doc.timeline.frames + count,
                None => shift.end(doc.timeline.frames).max(1),
            };
        }
        Some(keys) => {
            let paths = keys.iter().map(|k| layer_path(doc, k)).collect::<Result<Vec<_>>>()?;
            let outermost: Vec<Vec<usize>> = paths
                .iter()
                .filter(|p| !paths.iter().any(|q| q.len() < p.len() && p.starts_with(q)))
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            outermost
                .iter()
                .for_each(|path| shift_layer(doc.layer_at_mut(path), shift, out));
            if let Some(count) = shift.grows() {
                grow_for(doc, &outermost, count);
            }
        }
    }
    out.changed.push("timeline".into());
    Ok(())
}

pub fn timeline(doc: &mut Document, fps: Option<u32>, frames: Option<u32>) -> Result<()> {
    if let Some(n) = frames {
        let late = doc
            .all_layers()
            .into_iter()
            .flat_map(|l| l.cels().iter().map(move |c| (l, c)))
            .filter(|(_, c)| c.frame >= n)
            .max_by_key(|(_, c)| c.frame);
        if let Some((layer, cel)) = late {
            bail!(
                "cel {} of layer {} starts at frame {}; keep at least {} frames or remove frames first",
                cel.id,
                layer.id,
                cel.frame,
                cel.frame + 1
            );
        }
        if let Some(clip) = doc.timeline.clips.iter().find(|c| c.from >= n) {
            bail!(
                "clip {} starts at frame {}; keep at least {} frames or remove the clip first",
                clip.name,
                clip.from,
                clip.from + 1
            );
        }
        doc.timeline.clips.iter_mut().for_each(|c| c.to = c.to.min(n - 1));
        doc.timeline.frames = n;
    }
    if let Some(fps) = fps {
        doc.timeline.fps = fps;
    }
    Ok(())
}

pub fn grow(doc: &mut Document) {
    let last = doc
        .all_layers()
        .iter()
        .flat_map(|l| l.cels().iter().map(|c| c.frame + 1))
        .max()
        .unwrap_or(1);
    doc.timeline.frames = doc.timeline.frames.max(last);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CelAction {
    New { layer: String },
    Dup { cel: String },
    Link { cel: String },
    Unlink { cel: String },
    Move { cel: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CelSpec {
    pub action: CelAction,
    pub id: Option<String>,
    pub frame: Option<u32>,
    pub hold: Option<u32>,
    pub insert: bool,
}

fn place(layer: &str, cels: &mut Vec<Cel>, mut cel: Cel, hold: Option<u32>, fallback: Option<u32>) -> Result<()> {
    if let Some(c) = cels.iter().find(|c| c.frame == cel.frame) {
        bail!("layer {layer} already has {} starting at frame {}", c.id, cel.frame);
    }
    let ends = ends(cels);
    let covering = cels
        .iter()
        .zip(&ends)
        .position(|(c, e)| c.frame < cel.frame && cel.frame < *e);
    cel.hold = hold;
    match covering {
        Some(i) => {
            if let Some(h) = cels[i].hold {
                if hold.is_none() {
                    cel.hold = Some(cels[i].frame + h - cel.frame);
                }
                cels[i].hold = Some(cel.frame - cels[i].frame);
            }
        }
        None if hold.is_none() => cel.hold = fallback,
        None => {}
    }
    let index = cels.iter().position(|c| c.frame > cel.frame).unwrap_or(cels.len());
    cels.insert(index, cel);
    Ok(())
}

fn make_room(doc: &mut Document, path: &[usize], at: u32, count: u32, out: &mut Applied) -> Result<()> {
    let layer = doc.layer_at(path);
    let cels = layer.cels();
    if let Some((c, _)) = cels.iter().zip(ends(cels)).find(|(c, e)| c.frame < at && at < *e) {
        bail!(
            "frame {at} is inside the exposure of {} (from frame {}); insert at the start or end of an exposure, or leave insert off to take over the rest of it",
            c.id,
            c.frame
        );
    }
    shift_layer(doc.layer_at_mut(path), Shift::Room { at, count }, out);
    grow_for(doc, &[path.to_vec()], count);
    Ok(())
}

fn move_cel(doc: &mut Document, id: &str, to: u32, out: &mut Applied) -> Result<String> {
    let (path, c) = cel_loc(doc, id)?;
    let frames = doc.timeline.frames;
    let layer_id = doc.layer_at(&path).id.clone();
    let cels = doc
        .layer_at_mut(&path)
        .cels_mut()
        .ok_or_else(|| anyhow!("layer {layer_id} has no cels"))?;
    let me = cels[c].clone();
    if me.frame == to {
        return Ok(id.to_string());
    }
    let end = match ends(cels)[c] {
        OPEN => frames.max(me.frame + 1),
        e => e,
    };
    let length = end - me.frame;
    if c > 0 && ends(cels)[c - 1] == me.frame && cels[c - 1].hold.is_none() {
        cels[c - 1].hold = Some(me.frame - cels[c - 1].frame);
    }
    cels.remove(c);
    let e = ends(cels);
    if let Some(i) = cels.iter().zip(&e).position(|(x, end)| x.frame < to && to < *end) {
        cels[i].hold = Some(to - cels[i].frame);
    }
    let landed = cels.iter().map(|x| x.frame).filter(|f| *f >= to).min();
    if let Some(at) = landed.filter(|at| *at < to + length) {
        shift_layer(
            doc.layer_at_mut(&path),
            Shift::Room {
                at,
                count: to + length - at,
            },
            out,
        );
    }
    let cels = doc
        .layer_at_mut(&path)
        .cels_mut()
        .ok_or_else(|| anyhow!("layer {layer_id} has no cels"))?;
    let mut moved = me;
    moved.frame = to;
    moved.hold = Some(length);
    let index = cels.iter().position(|x| x.frame > to).unwrap_or(cels.len());
    cels.insert(index, moved);
    grow(doc);
    out.changed.push(id.to_string());
    Ok(id.to_string())
}

pub(crate) fn renumbered(doc: &mut Document, nodes: &[Node]) -> Vec<Node> {
    fn fresh(doc: &mut Document, taken: &mut BTreeSet<String>, node: &Node) -> Node {
        let mut copy = node.clone();
        copy.id = doc.alloc_id_in('n', taken);
        if let Some(children) = copy.shape.children_mut() {
            *children = children.iter().map(|c| fresh(doc, taken, c)).collect();
        }
        copy
    }
    let mut taken = doc.ids();
    nodes.iter().map(|n| fresh(doc, &mut taken, n)).collect()
}

fn cel_loc(doc: &Document, id: &str) -> Result<(Vec<usize>, usize)> {
    match doc.locate(id) {
        Some(Loc::Cel(path, c)) => Ok((path, c)),
        Some(_) => bail!("{id} is not a cel"),
        None => bail!("no cel {id}"),
    }
}

fn drawing_layer(doc: &Document, path: &[usize]) -> Result<()> {
    let layer = doc.layer_at(path);
    if layer.kind() == Kind::Group {
        bail!("{} is a group; drawings live in paint and vector layers", layer.id);
    }
    Ok(())
}

pub fn cel(doc: &mut Document, spec: &CelSpec, out: &mut Applied) -> Result<String> {
    let (path, source, fallback, frame) = match &spec.action {
        CelAction::New { layer } => {
            let path = layer_path(doc, layer)?;
            drawing_layer(doc, &path)?;
            let frame = spec.frame.ok_or_else(|| anyhow!("a new drawing needs a frame"))?;
            (path, None, None, frame)
        }
        CelAction::Dup { cel } | CelAction::Link { cel } => {
            let (path, c) = cel_loc(doc, cel)?;
            let layer = doc.layer_at(&path);
            let from = &layer.cels()[c];
            let source = from.link.clone().unwrap_or_else(|| from.id.clone());
            let end = ends(layer.cels())[c];
            let after = if end == OPEN {
                doc.timeline.frames.max(from.frame + 1)
            } else {
                end
            };
            (path, Some(source), from.hold, spec.frame.unwrap_or(after))
        }
        CelAction::Unlink { cel } => return unlink(doc, cel, out),
        CelAction::Move { cel } => {
            let to = spec
                .frame
                .ok_or_else(|| anyhow!("moving a drawing needs \"frame\": where it goes"))?;
            return move_cel(doc, cel, to, out);
        }
    };
    if spec.insert {
        let count = spec.hold.or(fallback).unwrap_or(1);
        make_room(doc, &path, frame, count, out)?;
    }
    let id = match &spec.id {
        Some(id) if !valid_id(id) => bail!("id {id:?} may only use letters, digits, - and _"),
        Some(id) if doc.locate(id).is_some() => bail!("id {id} is already used"),
        Some(id) => id.clone(),
        None => doc.alloc_id('c'),
    };
    let mut cel = Cel::at(&id, frame);
    let layer_id = doc.layer_at(&path).id.clone();
    let kind = doc.layer_at(&path).kind();
    match (&spec.action, &source) {
        (CelAction::Link { .. }, Some(source)) => cel.link = Some(source.clone()),
        (CelAction::Dup { .. }, Some(source)) if kind == Kind::Vector => {
            let nodes = doc
                .layer_at(&path)
                .cels()
                .iter()
                .find(|c| &c.id == source)
                .map(|c| c.nodes.clone())
                .unwrap_or_default();
            cel.nodes = renumbered(doc, &nodes);
        }
        (CelAction::Dup { .. }, Some(source)) => {
            out.copy(&layer_id, &id, &layer_id, source);
            cel.parts = doc
                .layer_at(&path)
                .cels()
                .iter()
                .find(|c| &c.id == source)
                .map(|c| c.parts.clone())
                .unwrap_or_default();
        }
        _ => {}
    }
    let cels = doc
        .layer_at_mut(&path)
        .cels_mut()
        .ok_or_else(|| anyhow!("layer {layer_id} has no cels"))?;
    place(&layer_id, cels, cel, spec.hold, fallback)?;
    out.added.push(id.clone());
    Ok(id)
}

fn unlink(doc: &mut Document, id: &str, out: &mut Applied) -> Result<String> {
    let (path, c) = cel_loc(doc, id)?;
    let layer = doc.layer_at(&path);
    let layer_id = layer.id.clone();
    let Some(source) = layer.cels()[c].link.clone() else {
        bail!("cel {id} is a drawing of its own, not a link");
    };
    if layer.kind() == Kind::Paint {
        out.copy(&layer_id, id, &layer_id, &source);
        let parts = layer
            .cels()
            .iter()
            .find(|c| c.id == source)
            .map(|c| c.parts.clone())
            .unwrap_or_default();
        if let Some(cels) = doc.layer_at_mut(&path).cels_mut() {
            cels[c].parts = parts;
        }
    } else {
        let nodes = layer
            .cels()
            .iter()
            .find(|c| c.id == source)
            .map(|c| c.nodes.clone())
            .unwrap_or_default();
        let fresh = renumbered(doc, &nodes);
        if let Some(cels) = doc.layer_at_mut(&path).cels_mut() {
            cels[c].nodes = fresh;
        }
    }
    if let Some(cels) = doc.layer_at_mut(&path).cels_mut() {
        cels[c].link = None;
    }
    out.changed.push(id.to_string());
    Ok(id.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Drawing { cel: String, start: bool },
    Link { cel: String, source: String, start: bool },
    Blank,
}

pub fn cells(layer: &Layer, frames: u32) -> Vec<Cell> {
    let cels = layer.cels();
    let ends = ends(cels);
    (0..frames)
        .map(|f| {
            let found = cels
                .iter()
                .zip(&ends)
                .find(|(c, e)| c.frame <= f && f < **e)
                .map(|(c, _)| c);
            match found {
                None => Cell::Blank,
                Some(c) => match &c.link {
                    Some(source) => Cell::Link {
                        cel: c.id.clone(),
                        source: source.clone(),
                        start: c.frame == f,
                    },
                    None => Cell::Drawing {
                        cel: c.id.clone(),
                        start: c.frame == f,
                    },
                },
            }
        })
        .collect()
}

pub fn sheet_layers(doc: &Document) -> Vec<&Layer> {
    fn walk<'a>(layers: &'a [Layer], out: &mut Vec<&'a Layer>) {
        layers.iter().rev().for_each(|l| {
            if l.kind() != Kind::Group {
                out.push(l);
            }
            out.extend(l.mask.as_deref());
            walk(l.children(), out);
        });
    }
    let mut out = Vec::new();
    walk(&doc.layers, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Layer;
    use crate::doc::Shape;
    use crate::ops::{Op, apply_all};
    use serde_json::json;

    fn cel(id: &str, frame: u32, hold: Option<u32>, link: Option<&str>) -> Cel {
        Cel {
            hold,
            link: link.map(String::from),
            ..Cel::at(id, frame)
        }
    }

    fn doc_with(cels: Vec<Cel>, frames: u32) -> Document {
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = frames;
        let mut layer = Layer::paint("l1", "lines", "c0");
        *layer.cels_mut().unwrap() = cels;
        doc.layers = vec![layer];
        doc.next_id = 100;
        doc
    }

    fn sheet(doc: &Document) -> String {
        cells(doc.layer("l1").unwrap(), doc.timeline.frames)
            .iter()
            .map(|c| match c {
                Cell::Drawing { cel, start: true } => cel.trim_start_matches('c').to_string(),
                Cell::Link {
                    source, start: true, ..
                } => format!(">{}", source.trim_start_matches('c')),
                Cell::Drawing { .. } | Cell::Link { .. } => "|".into(),
                Cell::Blank => ".".into(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn run(doc: &Document, ops: &[serde_json::Value]) -> Result<(Document, Applied)> {
        let ops: Vec<Op> = ops.iter().map(|v| Op::parse(v.clone())).collect::<Result<_>>()?;
        apply_all(doc, &ops)
    }

    fn twos() -> Document {
        doc_with(
            vec![
                cel("c1", 0, Some(2), None),
                cel("c2", 2, Some(2), None),
                cel("c3", 4, Some(2), Some("c1")),
                cel("c4", 6, None, None),
            ],
            8,
        )
    }

    #[test]
    fn inserting_lengthens_the_exposure_and_shifts_what_follows() {
        let doc = twos();
        assert_eq!(sheet(&doc), "1 | 2 | >1 | 4 |");
        let (doc, _) = run(&doc, &[json!({"op":"frames","insert":2,"count":2})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 | | | >1 | 4 |");
        assert_eq!(doc.timeline.frames, 10);
        let (doc, _) = run(&doc, &[json!({"op":"frames","insert":1})]).unwrap();
        assert_eq!(sheet(&doc), "1 | | 2 | | | >1 | 4 |");
    }

    #[test]
    fn removing_shortens_exposures_and_drops_drawings_left_without_frames() {
        let (doc, _) = run(&twos(), &[json!({"op":"frames","remove":1,"count":2})]).unwrap();
        assert_eq!(sheet(&doc), "1 2 >1 | 4 |");
        let (doc, applied) = run(&twos(), &[json!({"op":"frames","remove":2,"count":2})]).unwrap();
        assert_eq!(sheet(&doc), "1 | >1 | 4 |");
        assert_eq!(doc.timeline.frames, 6);
        assert_eq!(applied.removed, ["c2"]);
        assert_eq!(applied.dropped_cels, [("l1".to_string(), "c2".to_string())]);
        let (doc, _) = run(&twos(), &[json!({"op":"frames","remove":7})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 | >1 | 4");
    }

    #[test]
    fn a_removed_drawing_goes_to_its_first_link() {
        let (doc, applied) = run(&twos(), &[json!({"op":"frames","remove":0,"count":2})]).unwrap();
        assert_eq!(sheet(&doc), "2 | 3 | 4 |");
        assert_eq!(doc.layer("l1").unwrap().cels()[1].link, None);
        assert_eq!(applied.dropped_cels, [("l1".to_string(), "c1".to_string())]);
        let copy = &applied.marks[0];
        assert_eq!((copy.layer.as_str(), copy.cel.as_str()), ("l1", "c3"));
        let (doc, _) = run(&twos(), &[json!({"op":"rm","id":"c1"})]).unwrap();
        assert_eq!(sheet(&doc), ". . 2 | 3 | 4 |");
    }

    #[test]
    fn a_hold_ending_where_a_removed_drawing_was_keeps_its_blank() {
        let doc = doc_with(
            vec![
                cel("c1", 0, None, None),
                cel("c2", 2, Some(1), None),
                cel("c3", 5, None, None),
            ],
            7,
        );
        assert_eq!(sheet(&doc), "1 | 2 . . 3 |");
        let (doc, _) = run(&doc, &[json!({"op":"frames","remove":2})]).unwrap();
        assert_eq!(sheet(&doc), "1 | . . 3 |");
    }

    #[test]
    fn new_drawings_take_over_the_rest_of_an_exposure() {
        let (doc, applied) = run(&twos(), &[json!({"op":"cel","new":"l1","frame":3})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 101 >1 | 4 |");
        assert_eq!(applied.added, ["c101"]);
        let (doc, _) = run(&twos(), &[json!({"op":"cel","new":"l1","frame":7})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 | >1 | 4 101");
        let taken = run(&twos(), &[json!({"op":"cel","new":"l1","frame":2})]);
        assert!(format!("{:#}", taken.unwrap_err()).contains("already has c2"));
        let (doc, _) = run(&twos(), &[json!({"op":"cel","new":"l1","frame":11})]).unwrap();
        assert_eq!(doc.timeline.frames, 12);
    }

    #[test]
    fn dup_and_link_land_after_their_source_and_can_make_room() {
        let (doc, applied) = run(&twos(), &[json!({"op":"cel","dup":"c2","insert":true})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 | 101 | >1 | 4 |");
        assert_eq!(doc.timeline.frames, 10);
        let copy = &applied.marks[0];
        assert_eq!(copy.cel, "c101");
        let (doc, _) = run(&twos(), &[json!({"op":"cel","link":"c3","frame":7})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 | >1 | 4 >1");
        let inside = run(&twos(), &[json!({"op":"cel","dup":"c2","frame":3,"insert":true})]);
        assert!(format!("{:#}", inside.unwrap_err()).contains("inside the exposure"));
    }

    #[test]
    fn unlink_makes_a_copy_of_the_source() {
        let (doc, applied) = run(&twos(), &[json!({"op":"cel","unlink":"c3"})]).unwrap();
        assert_eq!(sheet(&doc), "1 | 2 | 3 | 4 |");
        assert_eq!(applied.marks[0].cel, "c3");
        let own = run(&twos(), &[json!({"op":"cel","unlink":"c2"})]);
        assert!(format!("{:#}", own.unwrap_err()).contains("not a link"));
    }

    #[test]
    fn vector_drawings_copy_nodes_with_new_ids() {
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = 4;
        let node = Node::new(
            "n1",
            Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: 4.0,
                h: 4.0,
                radius: 0.0,
            },
        );
        doc.layers = vec![Layer::vector("l1", "shapes", "c1", vec![node])];
        doc.next_id = 10;
        let (doc, _) = run(&doc, &[json!({"op":"cel","dup":"c1","frame":2})]).unwrap();
        let cels = doc.layer("l1").unwrap().cels();
        assert_eq!(cels[1].nodes.len(), 1);
        assert_ne!(cels[1].nodes[0].id, "n1");
        doc.validate().unwrap();
    }

    #[test]
    fn listed_layers_shift_alone_and_masks_follow() {
        let mut doc = twos();
        doc.layers.push(Layer::paint("l2", "colour", "c9"));
        doc.layers[1].mask = Some(Box::new(Layer::paint("l3", "mask", "c8")));
        doc.layers[1].cels_mut().unwrap()[0].frame = 2;
        doc.layers[1].mask.as_mut().unwrap().cels_mut().unwrap()[0].frame = 2;
        let (doc, _) = run(&doc, &[json!({"op":"frames","insert":0,"count":3,"layers":["l2"]})]).unwrap();
        assert_eq!(doc.layer("l2").unwrap().cels()[0].frame, 5);
        assert_eq!(doc.layer("l3").unwrap().cels()[0].frame, 5);
        assert_eq!(sheet(&doc), "1 | 2 | >1 | 4 | | | |");
        assert_eq!(doc.timeline.frames, 11);
    }

    #[test]
    fn a_listed_layer_lengthens_the_timeline_only_as_far_as_it_needs() {
        let mut doc = twos();
        let mut blink = Layer::paint("l2", "blink", "c9");
        blink.cels_mut().unwrap()[0].hold = Some(3);
        doc.layers.push(blink);
        let (doc, _) = run(&doc, &[json!({"op":"frames","insert":1,"layers":["l2"]})]).unwrap();
        assert_eq!(doc.timeline.frames, 8);
        assert_eq!(doc.layer("l2").unwrap().cels()[0].hold, Some(4));
        let (doc, _) = run(&doc, &[json!({"op":"frames","insert":2,"count":6,"layers":["l2"]})]).unwrap();
        assert_eq!(doc.timeline.frames, 10);
        let (doc, _) = run(&doc, &[json!({"op":"frames","insert":0,"layers":["l1"]})]).unwrap();
        assert_eq!(doc.timeline.frames, 11);
    }

    #[test]
    fn clips_shift_with_all_layers_and_go_when_removed() {
        let mut doc = twos();
        doc.timeline.clips = vec![
            crate::doc::Clip::new("a", 0, 1, false),
            crate::doc::Clip::new("b", 4, 7, true),
        ];
        let (grown, _) = run(&doc, &[json!({"op":"frames","insert":5,"count":2})]).unwrap();
        assert_eq!((grown.timeline.clips[1].from, grown.timeline.clips[1].to), (4, 9));
        let (shrunk, _) = run(&doc, &[json!({"op":"frames","remove":0,"count":2})]).unwrap();
        assert_eq!(shrunk.timeline.clips.len(), 1);
        assert_eq!((shrunk.timeline.clips[0].from, shrunk.timeline.clips[0].to), (2, 5));
    }

    #[test]
    fn timeline_sets_fps_and_refuses_to_cut_cels() {
        let (doc, _) = run(&twos(), &[json!({"op":"timeline","fps":24,"frames":12})]).unwrap();
        assert_eq!((doc.timeline.fps, doc.timeline.frames), (24, 12));
        let cut = run(&twos(), &[json!({"op":"timeline","frames":5})]);
        assert!(format!("{:#}", cut.unwrap_err()).contains("c4"));
    }

    #[test]
    fn strokes_past_the_end_grow_the_timeline() {
        let doc = doc_with(vec![cel("c1", 0, None, None)], 1);
        let (doc, _) = run(
            &doc,
            &[json!({"op":"stroke","layer":"l1","frame":5,"path":"M0 0 L9 9"})],
        )
        .unwrap();
        assert_eq!(doc.timeline.frames, 6);
    }
}
