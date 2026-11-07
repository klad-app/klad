use super::{Editor, Tool};
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::pen::PenEvent;
use crate::theme;
use anyhow::Result;
use egui::{Id, RichText, vec2};
use klad_core::doc::Item;
use klad_core::markedit::{LassoAction, MarkTransform};
use klad_core::ops::{Op, Place};
use klad_core::pathedit::PathEdit;
use serde_json::{Map, Value, json};

#[derive(Default)]
pub(super) struct Held {
    events: Vec<PenEvent>,
}

impl Held {
    pub fn admit(&mut self, busy: bool, events: Vec<PenEvent>) -> Vec<PenEvent> {
        match busy {
            true => {
                self.events.extend(events);
                Vec::new()
            }
            false => {
                let mut all = std::mem::take(&mut self.events);
                all.extend(events);
                all
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }
}

pub(super) enum Selected {
    Strokes,
    Shape(String),
    Points,
}

fn strip_ids(value: &mut Value) {
    if let Value::Object(map) = value {
        map.remove("id");
        map.values_mut().for_each(strip_ids);
    }
    if let Value::Array(items) = value {
        items.iter_mut().for_each(strip_ids);
    }
}

impl Editor {
    pub(super) fn selected(&self) -> Option<Selected> {
        match self.tool {
            Tool::Strokes => self.picked.as_ref().map(|_| Selected::Strokes),
            Tool::Direct => self
                .direct
                .as_ref()
                .filter(|d| !d.points.is_empty())
                .map(|_| Selected::Points),
            Tool::Select => self
                .selection
                .clone()
                .map(Selected::Shape)
                .or_else(|| self.picked.as_ref().map(|_| Selected::Strokes)),
            _ => None,
        }
    }

    pub(super) fn writing(&self) -> bool {
        self.path_commit.is_some()
    }

    fn middle_of_picked(&self) -> Option<[f32; 2]> {
        self.picked_corners()
            .map(|(_, c)| [(c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0])
    }

    fn set_node(&mut self, id: &str, props: Vec<(&str, Value)>) -> Result<()> {
        self.apply(vec![Op::Set {
            id: id.to_string(),
            props: props
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect::<Map<String, Value>>(),
        }])
    }

    pub(super) fn node_transform(&self, id: &str) -> Option<klad_core::doc::Transform> {
        match self.scene.bundle.doc.item(id) {
            Some(Item::Node(_, _, node)) => Some(node.transform),
            _ => None,
        }
    }

    pub(super) fn nudge(&mut self, dx: f32, dy: f32) -> Result<()> {
        let step = if self.key_shift { 10.0 } else { 1.0 };
        let (dx, dy) = (dx * step, dy * step);
        match self.selected() {
            Some(Selected::Strokes) => self.edit_picked(LassoAction::Transform(MarkTransform {
                x: dx,
                y: dy,
                ..Default::default()
            })),
            Some(Selected::Shape(id)) => {
                let Some(t) = self.node_transform(&id) else {
                    return Ok(());
                };
                self.set_node(
                    &id,
                    vec![("transform.x", json!(t.x + dx)), ("transform.y", json!(t.y + dy))],
                )
            }
            Some(Selected::Points) => {
                let points: Vec<usize> = self
                    .direct
                    .as_ref()
                    .map_or(Vec::new(), |d| d.points.iter().copied().collect());
                self.commit_path(PathEdit::Move { points, by: [dx, dy] })
            }
            None => Ok(()),
        }
    }

    pub(super) fn turn(&mut self, degrees: f32) -> Result<()> {
        match self.selected() {
            Some(Selected::Strokes) => {
                let pivot = self.middle_of_picked();
                self.edit_picked(LassoAction::Transform(MarkTransform {
                    rotate: degrees,
                    pivot,
                    ..Default::default()
                }))
            }
            Some(Selected::Shape(id)) => {
                let Some(t) = self.node_transform(&id) else {
                    return Ok(());
                };
                self.set_node(&id, vec![("transform.rotate", json!(t.rotate + degrees))])
            }
            _ => Ok(()),
        }
    }

    pub(super) fn flip(&mut self, horizontal: bool) -> Result<()> {
        match self.selected() {
            Some(Selected::Strokes) => match horizontal {
                true => self.flip_picked(-1.0, 1.0),
                false => self.flip_picked(1.0, -1.0),
            },
            Some(Selected::Shape(id)) => {
                let Some(t) = self.node_transform(&id) else {
                    return Ok(());
                };
                let scale = match horizontal {
                    true => ("transform.sx", json!(-t.sx)),
                    false => ("transform.sy", json!(-t.sy)),
                };
                self.set_node(&id, vec![scale, ("transform.rotate", json!(-t.rotate))])
            }
            _ => Ok(()),
        }
    }

    pub(super) fn duplicate_in_place(&mut self) -> Result<()> {
        match self.selected() {
            Some(Selected::Strokes) => match self.copied() {
                Some(copied) => self.paste_copied(copied, "duplicated"),
                None => Ok(()),
            },
            Some(Selected::Shape(id)) => {
                let Some(Item::Node(_, _, node)) = self.scene.bundle.doc.item(&id) else {
                    return Ok(());
                };
                let mut value = serde_json::to_value(node)?;
                strip_ids(&mut value);
                let op = Op::AddNode {
                    node: value,
                    place: Place {
                        above: Some(id.clone()),
                        frame: Some(self.scene.frame),
                        ..Place::default()
                    },
                };
                let applied = self.scene.apply_with(&[op], &self.brushes);
                match applied {
                    Ok((dirty, applied)) => {
                        self.dirty.extend(dirty);
                        self.selection = applied.added.first().cloned();
                        self.say(Tone::Done, format!("duplicated {id} in place"));
                        Ok(())
                    }
                    Err(e) => {
                        self.say(Tone::Refused, format!("could not duplicate it: {e:#}"));
                        Ok(())
                    }
                }
            }
            _ => Ok(()),
        }
    }

    pub(super) fn selection_bar(&mut self, ctx: &egui::Context) {
        let Some(selected) = self.selected() else {
            return;
        };
        let Some(view) = self.view else {
            return;
        };
        let rect = self.canvas_rect;
        let corners = match selected {
            Selected::Points => None,
            _ => self.handles().map(|(_, c)| c),
        };
        let at = match corners {
            Some(corners) => {
                let screen: Vec<egui::Pos2> = corners.iter().map(|c| self.screen(view, rect, *c)).collect();
                let (lo, hi) = screen
                    .iter()
                    .fold((screen[0], screen[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                match hi.y + 30.0 + 40.0 < rect.bottom() {
                    true => egui::pos2((lo.x + hi.x) / 2.0, hi.y + 30.0),
                    false => egui::pos2((lo.x + hi.x) / 2.0, lo.y - 72.0),
                }
            }
            None => egui::pos2(rect.center().x, rect.bottom() - 60.0),
        };
        let at = egui::pos2(
            at.x.clamp(rect.left() + 8.0, rect.right() - 8.0),
            at.y.max(rect.top() + 8.0),
        );
        let commands: Vec<Cmd> = match selected {
            Selected::Strokes => vec![
                Cmd::FlipH,
                Cmd::FlipV,
                Cmd::TurnLeft90,
                Cmd::TurnRight90,
                Cmd::Recolour,
                Cmd::Thinner,
                Cmd::Thicker,
                Cmd::Copy,
                Cmd::Paste,
                Cmd::DuplicateInPlace,
                Cmd::Delete,
                Cmd::Deselect,
            ],
            Selected::Shape(_) => vec![
                Cmd::FlipH,
                Cmd::FlipV,
                Cmd::TurnLeft90,
                Cmd::TurnRight90,
                Cmd::FillShape,
                Cmd::OutlineShape,
                Cmd::DuplicateInPlace,
                Cmd::Delete,
                Cmd::Deselect,
            ],
            Selected::Points => vec![
                Cmd::Corner,
                Cmd::Smooth,
                Cmd::NudgeLeft,
                Cmd::NudgeRight,
                Cmd::NudgeUp,
                Cmd::NudgeDown,
                Cmd::Delete,
                Cmd::Deselect,
            ],
        };
        let writing = self.writing();
        egui::Area::new(Id::unique("selection bar"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(at)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        commands.iter().for_each(|cmd| {
                            if *cmd == Cmd::Recolour {
                                let swatch = crate::color::color32(self.color);
                                let r = self.command_button(ui, *cmd, None);
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(r.rect.right_bottom() - vec2(9.0, 9.0), vec2(7.0, 7.0)),
                                    1.0,
                                    swatch,
                                );
                            } else {
                                self.command_button(ui, *cmd, None);
                            }
                        });
                        if let Some((_, chosen)) = self.chosen_marks().filter(|_| self.on_marks()) {
                            let opacity = chosen.first().map_or(1.0, |(_, m)| match m {
                                klad_core::stroke::Mark::Stroke(s) => s.opacity,
                                klad_core::stroke::Mark::Fill(f) => f.opacity,
                            });
                            if let Some(v) = self.number(
                                ui,
                                "bar.opacity",
                                "",
                                opacity,
                                0.0..=1.0,
                                0.01,
                                "the opacity of every selected stroke and fill",
                            ) {
                                let sets = chosen
                                    .iter()
                                    .map(|(cel, m)| (cel.clone(), m.id(), "opacity", json!(v)))
                                    .collect();
                                self.check(|e| e.set_picked(sets));
                            }
                        }
                        if writing {
                            ui.label(RichText::new("writing...").color(theme::WEAK));
                        }
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pen::Phase;
    use std::time::Instant;

    fn event(phase: Phase, x: f32) -> PenEvent {
        PenEvent {
            phase,
            pos: egui::pos2(x, 10.0),
            pressure: 1.0,
            tilt: (0.0, 0.0),
            eraser: false,
            tablet: false,
            device: 0,
            at: Instant::now(),
        }
    }

    #[test]
    fn a_press_while_an_edit_is_written_waits_for_it_in_order() {
        let mut held = Held::default();
        let first = vec![event(Phase::Down, 1.0), event(Phase::Move, 2.0)];
        assert!(held.admit(true, first).is_empty());
        assert!(held.admit(true, vec![event(Phase::Up, 3.0)]).is_empty());
        assert_eq!(held.len(), 3);
        let out = held.admit(false, vec![event(Phase::Down, 4.0)]);
        let xs: Vec<f32> = out.iter().map(|e| e.pos.x).collect();
        assert_eq!(xs, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(held.len(), 0);
        assert_eq!(held.admit(false, vec![event(Phase::Move, 5.0)]).len(), 1);
    }

    #[test]
    fn duplicated_shapes_lose_their_ids_everywhere() {
        let mut v = json!({"id": "n5", "type": "group", "children": [{"id": "n6", "type": "rect"}], "fill": {"linear": {"stops": []}}});
        strip_ids(&mut v);
        assert_eq!(
            v,
            json!({"type": "group", "children": [{"type": "rect"}], "fill": {"linear": {"stops": []}}})
        );
    }
}
