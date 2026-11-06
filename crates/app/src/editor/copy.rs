use super::{Editor, Part, Picked, Tool, ms};
use crate::messages::Tone;
use anyhow::Result;
use klad_core::doc::Kind;
use klad_core::markedit::{self, MarkChange, Side, address};
use klad_core::ops::{CopySpec, CopyTarget, Op};
use klad_core::stroke::Mark;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Piece {
    layer: String,
    cel: String,
    cut: Option<Vec<[f32; 2]>>,
    around: Vec<u32>,
    whole: Vec<u32>,
}

impl Piece {
    fn count(&self) -> usize {
        self.around.len() + self.whole.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Copied {
    pieces: Vec<Piece>,
}

impl Copied {
    fn count(&self) -> usize {
        self.pieces.iter().map(Piece::count).sum()
    }

    fn whence(&self) -> String {
        match &self.pieces[..] {
            [one] => one.cel.clone(),
            many => format!("{} layers", many.len()),
        }
    }
}

pub(super) fn piece(layer: &str, cel: &str, ids: &[u32], cut: Option<&Vec<[f32; 2]>>, marks: &[Mark]) -> Piece {
    let (around, whole, cut) = match cut {
        Some(polygon) => {
            let around: Vec<u32> = markedit::sides(marks, polygon)
                .into_iter()
                .filter(|(_, side)| *side != Side::Outside)
                .map(|(id, _)| id)
                .collect();
            let whole = ids.iter().copied().filter(|id| !around.contains(id)).collect();
            let cut = (!around.is_empty()).then(|| polygon.clone());
            (around, whole, cut)
        }
        None => (Vec::new(), ids.to_vec(), None),
    };
    Piece {
        layer: layer.to_string(),
        cel: cel.to_string(),
        cut,
        around,
        whole,
    }
}

pub(super) fn paste_ops(piece: &Piece, to: &CopyTarget) -> Vec<Op> {
    let op = |ids: &[u32], cut: Option<Vec<[f32; 2]>>| {
        Op::Copy(CopySpec {
            ids: ids.iter().map(|id| address(&piece.cel, *id)).collect(),
            to: to.clone(),
            transform: None,
            cut,
        })
    };
    let around = (!piece.around.is_empty()).then(|| op(&piece.around, piece.cut.clone()));
    let whole = (!piece.whole.is_empty()).then(|| op(&piece.whole, None));
    around.into_iter().chain(whole).collect()
}

impl Editor {
    pub(super) fn copied(&self) -> Option<Copied> {
        let picked = self.picked.as_ref()?;
        let layer = self.scene.active_id()?;
        let pieces: Vec<Piece> = std::iter::once(piece(
            &layer,
            &picked.cel,
            &picked.ids,
            picked.cut.as_ref(),
            self.scene.active_marks(),
        ))
        .chain(picked.more.iter().map(|p| {
            piece(
                &p.layer,
                &p.cel,
                &p.ids,
                picked.cut.as_ref(),
                self.scene.marks_of(&p.cel),
            )
        }))
        .filter(|p| p.count() > 0)
        .collect();
        (!pieces.is_empty()).then_some(Copied { pieces })
    }

    pub(super) fn copy_picked(&mut self, ctx: &egui::Context) {
        let Some(copied) = self.copied() else {
            self.say(Tone::Refused, "select strokes with S first; cmd C copies them");
            return;
        };
        ctx.copy_text(format!("klad: {} marks of {}", copied.count(), copied.whence()));
        self.say(
            Tone::Done,
            format!(
                "copied {} marks of {}: go to another frame and cmd V puts them there, in place",
                copied.count(),
                copied.whence()
            ),
        );
        self.clipboard = Some(copied);
    }

    pub(super) fn paste(&mut self) -> Result<()> {
        match self.clipboard.clone() {
            Some(copied) => self.paste_copied(copied, "pasted"),
            None => Ok(()),
        }
    }

    fn target(&self, layer: &klad_core::doc::Layer) -> CopyTarget {
        let frame = self.scene.frame;
        match layer.content_at(frame) {
            Some(cel) => CopyTarget::Cel(cel.id.clone()),
            None => CopyTarget::Frame {
                frame,
                layer: Some(layer.id.clone()),
            },
        }
    }

    pub(super) fn paste_copied(&mut self, copied: Copied, word: &str) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let paintable = |l: &&klad_core::doc::Layer| l.kind() == Kind::Paint && !l.locked;
        let targets: Vec<(&Piece, CopyTarget)> = match &copied.pieces[..] {
            [one] => {
                let Some(layer) = self.scene.active_layer().filter(paintable) else {
                    self.say(Tone::Refused, "make a paint layer active to paste strokes into it");
                    return Ok(());
                };
                vec![(one, self.target(layer))]
            }
            many => {
                let missing: Vec<&str> = many
                    .iter()
                    .filter(|p| doc.layer(&p.layer).filter(paintable).is_none())
                    .map(|p| p.layer.as_str())
                    .collect();
                if !missing.is_empty() {
                    self.say(
                        Tone::Refused,
                        format!(
                            "could not {word}: the layers {} are gone or locked; unlock them, or pick another layer",
                            missing.join(", ")
                        ),
                    );
                    return Ok(());
                }
                many.iter()
                    .filter_map(|p| doc.layer(&p.layer).map(|l| (p, self.target(l))))
                    .collect()
            }
        };
        let frame = self.scene.frame;
        let start = Instant::now();
        let ops: Vec<Op> = targets.iter().flat_map(|(p, to)| paste_ops(p, to)).collect();
        let (dirty, applied) = match self.scene.apply_with(&ops, &self.brushes) {
            Ok(done) => done,
            Err(e) => {
                self.say(Tone::Refused, format!("could not {word} here: {e:#}"));
                return Ok(());
            }
        };
        self.dirty.extend(dirty);
        let made: Vec<(String, String, u32)> = applied
            .marks
            .iter()
            .flat_map(|p| {
                p.changes.iter().flat_map(move |c| match c {
                    MarkChange::Paste { made, .. } => {
                        made.iter().map(|m| (p.layer.clone(), p.cel.clone(), m.to)).collect()
                    }
                    _ => Vec::new(),
                })
            })
            .collect();
        let active = self.scene.active_cel();
        if let Some(cel) = active.clone().filter(|_| !made.is_empty()) {
            if self.tool != Tool::Select {
                self.tool = Tool::Strokes;
            }
            self.selection = None;
            let ids = made
                .iter()
                .filter(|(_, c, _)| *c == cel)
                .map(|(_, _, id)| *id)
                .collect();
            let more =
                made.iter()
                    .filter(|(_, c, _)| *c != cel)
                    .fold(Vec::<Part>::new(), |mut parts, (layer, c, id)| {
                        match parts.iter_mut().find(|p| p.cel == *c) {
                            Some(p) => p.ids.push(*id),
                            None => parts.push(Part {
                                layer: layer.clone(),
                                cel: c.clone(),
                                ids: vec![*id],
                            }),
                        }
                        parts
                    });
            self.picked = Some(Picked {
                more,
                ..Picked::whole(cel.clone(), ids)
            });
            self.say(
                Tone::Done,
                format!(
                    "{word} {} marks into {} on frame {frame} in {:.0} ms (one cmd Z takes them off)",
                    made.len(),
                    copied.whence(),
                    ms(start)
                ),
            );
        }
        self.forget_missing_selection();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::stroke::Stroke;

    fn line(id: u32, x: f32) -> Mark {
        Stroke::new(id, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1)
            .with_points(&[(x, 10.0, 1.0), (x, 90.0, 1.0)], 8.0)
            .into()
    }

    #[test]
    fn a_cut_selection_copies_the_parts_inside_and_whole_picks_whole() {
        let marks = vec![line(1, 20.0), line(2, 50.0), line(3, 200.0)];
        let polygon = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]];
        let cut = piece("l4", "c4", &[3], Some(&polygon), &marks);
        assert_eq!((cut.around.clone(), cut.whole.clone()), (vec![1, 2], vec![3]));
        let ops = paste_ops(&cut, &CopyTarget::Cel("c9".into()));
        assert_eq!(ops.len(), 2);
        let Op::Copy(first) = &ops[0] else { panic!() };
        assert_eq!(first.ids, ["c4:s1", "c4:s2"]);
        assert_eq!(first.cut.as_deref(), Some(&polygon[..]));
        let Op::Copy(second) = &ops[1] else { panic!() };
        assert_eq!(
            (second.ids.clone(), second.cut.clone()),
            (vec!["c4:s3".to_string()], None)
        );
        let plain = piece("l4", "c4", &[2], None, &marks);
        assert_eq!(paste_ops(&plain, &CopyTarget::Cel("c9".into())).len(), 1);
        assert_eq!(piece("l4", "c4", &[], None, &marks).count(), 0);
    }
}
