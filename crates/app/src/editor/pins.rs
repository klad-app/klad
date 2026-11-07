use super::Editor;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use anyhow::Result;
use egui::{Color32, Pos2, Rect, RichText, Vec2, vec2};
use klad_core::notes::{self, Note};

pub(super) const PIN: f32 = 11.0;

pub(super) fn hit_pin(pins: &[Note], frame: u32, at: Vec2, zoom: f32) -> Option<u32> {
    let reach = (PIN + 4.0) / zoom.max(1e-3);
    pins.iter()
        .filter(|n| n.frame == frame)
        .map(|n| (n.id, (n.x - at.x).hypot(n.y - at.y)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

pub(super) fn hit_any_pin(pins: &[Note], frame: u32, at: Vec2, zoom: f32) -> Option<(u32, u32)> {
    let reach = (PIN + 4.0) / zoom.max(1e-3);
    pins.iter()
        .filter(|n| !n.done || n.frame == frame)
        .map(|n| (n, (n.x - at.x).hypot(n.y - at.y)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| {
            (a.0.frame != frame, a.1)
                .partial_cmp(&(b.0.frame != frame, b.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(n, _)| (n.id, n.frame))
}

impl Editor {
    pub(super) fn draw_pins(&self, painter: &egui::Painter, place: impl Fn(Vec2) -> Pos2) {
        let frame = self.scene.frame;
        let mut pins: Vec<&Note> = self.pins.iter().filter(|n| !n.done || n.frame == frame).collect();
        pins.sort_by_key(|n| n.frame == frame);
        pins.into_iter().for_each(|n| {
            let p = place(vec2(n.x, n.y));
            let here = n.frame == frame;
            let open = self.note_open == Some(n.id);
            let fill = match (n.done, here) {
                (true, _) => Color32::from_rgba_unmultiplied(120, 200, 120, 200),
                (false, true) => Color32::from_rgb(255, 196, 60),
                (false, false) => Color32::from_rgba_unmultiplied(255, 196, 60, 90),
            };
            if open {
                painter.circle_stroke(p, PIN + 4.0, egui::Stroke::new(2.5, theme::ACCENT));
            }
            painter.circle_filled(p, PIN, fill);
            painter.circle_stroke(
                p,
                PIN,
                egui::Stroke::new(
                    if open { 2.5 } else { 1.5 },
                    Color32::from_black_alpha(if here { 180 } else { 90 }),
                ),
            );
            let label = if n.done {
                icon::CHECK.to_string()
            } else {
                n.id.to_string()
            };
            painter.text(
                p,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(12.0),
                Color32::from_black_alpha(if here { 255 } else { 140 }),
            );
            if !here {
                painter.text(
                    p + vec2(PIN + 3.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("frame {}", n.frame),
                    egui::FontId::proportional(11.0),
                    Color32::from_black_alpha(150),
                );
            }
        });
        if let Some(at) = self.note_at {
            painter.circle_stroke(place(at), PIN, egui::Stroke::new(2.0, Color32::from_rgb(255, 196, 60)));
        }
    }

    pub(super) fn press_note(&mut self, at: Vec2) {
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        match hit_pin(&self.pins, self.scene.frame, at, zoom) {
            Some(id) => self.open_pin(id),
            None => {
                self.note_open = None;
                self.note_at = Some(at);
                self.note_text.clear();
            }
        }
    }

    pub(super) fn select_pin(&mut self, at: Vec2) -> Result<()> {
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        match hit_any_pin(&self.pins, self.scene.frame, at, zoom) {
            Some((id, frame)) => {
                self.go_to(frame)?;
                self.open_pin(id);
                self.say(Tone::Info, format!("note {id} on frame {frame}: Delete removes it"));
            }
            None => {
                self.note_open = None;
                self.note_at = None;
            }
        }
        Ok(())
    }

    pub(super) fn remove_open_pin(&mut self) -> Result<()> {
        if let Some(id) = self.note_open.take() {
            notes::remove(&self.scene.bundle, id)?;
            self.reread_pins();
            self.say(Tone::Done, format!("note {id} removed"));
        }
        Ok(())
    }

    pub(super) fn open_pin(&mut self, id: u32) {
        self.note_at = None;
        self.note_open = Some(id);
        self.note_edit = self
            .pins
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.text.clone())
            .unwrap_or_default();
    }

    pub(super) fn reread_pins(&mut self) {
        self.pins = notes::read(&self.scene.bundle);
    }

    pub(super) fn pin_popover(&mut self, ctx: &egui::Context) -> Result<()> {
        let (Some(view), rect) = (self.view, self.canvas_rect) else {
            return Ok(());
        };
        let at = match (self.note_at, self.note_open) {
            (Some(at), _) => at,
            (None, Some(id)) => match self.pins.iter().find(|n| n.id == id) {
                Some(n) => vec2(n.x, n.y),
                None => {
                    self.note_open = None;
                    return Ok(());
                }
            },
            _ => return Ok(()),
        };
        let p = self.screen(view, rect, at);
        let spot = Rect::from_min_size(p + vec2(PIN + 6.0, PIN + 6.0), vec2(0.0, 0.0)).min;
        let mut result = Ok(());
        egui::Area::new(egui::Id::unique("pin popover"))
            .fixed_pos(spot)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    result = match self.note_open {
                        Some(id) => self.open_pin_ui(ui, id),
                        None => self.new_pin_ui(ui, at),
                    };
                });
            });
        result
    }

    fn new_pin_ui(&mut self, ui: &mut egui::Ui, at: Vec2) -> Result<()> {
        let label = ui.label(format!(
            "a note for the agent at {:.0}, {:.0}, frame {}",
            at.x, at.y, self.scene.frame
        ));
        let response = ui
            .add(egui::TextEdit::multiline(&mut self.note_text).desired_rows(3))
            .labelled_by(label.id)
            .hint("what the agent should know or do here");
        if self.note_text.is_empty() && !response.has_focus() {
            response.request_focus();
        }
        let mut pin = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            pin = ui
                .add_enabled(!self.note_text.trim().is_empty(), egui::Button::new("pin it"))
                .hint("save the note where you clicked; the agent reads it with klad notes")
                .clicked();
            cancel = ui.button("cancel").hint("drop this note").clicked();
        });
        if pin {
            notes::add(
                &self.scene.bundle,
                (at.x, at.y),
                self.scene.frame,
                &self.note_text,
                "user",
            )?;
            self.note_at = None;
            self.note_text.clear();
            self.reread_pins();
            self.say(Tone::Done, "note pinned: the agent reads it with klad notes");
        }
        if cancel {
            self.note_at = None;
        }
        Ok(())
    }

    fn open_pin_ui(&mut self, ui: &mut egui::Ui, id: u32) -> Result<()> {
        let Some(note) = self.pins.iter().find(|n| n.id == id).cloned() else {
            self.note_open = None;
            return Ok(());
        };
        let label = ui.label(format!(
            "note {} on frame {}, by {}{}",
            note.id,
            note.frame,
            note.author,
            if note.done { ", done" } else { "" }
        ));
        ui.add(egui::TextEdit::multiline(&mut self.note_edit).desired_rows(3))
            .labelled_by(label.id)
            .hint("edit the note; save keeps it");
        let mut done = note.done;
        let (mut save, mut remove, mut close) = (false, false, false);
        ui.horizontal(|ui| {
            save = ui
                .add_enabled(
                    self.note_edit.trim() != note.text && !self.note_edit.trim().is_empty(),
                    egui::Button::new("save"),
                )
                .hint("keep the edited text")
                .clicked();
            ui.checkbox(&mut done, "done")
                .hint("done: the agent has acted on it (it marks this itself with klad notes --done)");
            remove = ui.button("remove").hint("delete this note").clicked();
            close = ui.button("close").hint("close this note").clicked();
        });
        if save {
            notes::set_text(&self.scene.bundle, id, &self.note_edit)?;
        }
        if done != note.done {
            notes::set_done(&self.scene.bundle, id, done)?;
        }
        if remove {
            self.remove_open_pin()?;
            return Ok(());
        }
        if close {
            self.note_open = None;
        }
        if save || remove || done != note.done {
            self.reread_pins();
        }
        Ok(())
    }

    pub(super) fn notes_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let open: Vec<Note> = self.pins.iter().filter(|n| !n.done).cloned().collect();
        let done: Vec<Note> = self.pins.iter().filter(|n| n.done).cloned().collect();
        if open.is_empty() {
            ui.label(RichText::new("no open notes: N, then click the canvas to pin one").color(theme::WEAK));
        }
        let mut go: Option<(u32, u32)> = None;
        let mut reopen = None;
        open.iter().for_each(|n| {
            if ui
                .add(egui::Button::new(format!("{}. frame {}: {}", n.id, n.frame, n.text)).truncate())
                .hint("go to its frame and open it")
                .clicked()
            {
                go = Some((n.frame, n.id));
            }
        });
        let mut clear = false;
        if !done.is_empty() {
            let id = ui.make_persistent_id("done notes");
            let header = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    let named = ui
                        .add(egui::Button::new(format!("done ({})", done.len())).frame(false))
                        .hint("show or hide the notes the agent marked done")
                        .clicked();
                    let cleared = ui
                        .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.button("clear all")
                                .hint("remove every done note, as klad notes --clear-done")
                                .clicked()
                        })
                        .inner;
                    (named, cleared)
                });
            let (_, head, _) = header.body(|ui| {
                done.iter().for_each(|n| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("open again")
                            .hint("the agent should look at it again")
                            .clicked()
                        {
                            reopen = Some(n.id);
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!(
                                            "{} {}. frame {}: {}",
                                            icon::CHECK,
                                            n.id,
                                            n.frame,
                                            n.text
                                        ))
                                        .color(theme::WEAK),
                                    )
                                    .truncate(),
                                )
                                .hint("go to its frame and open it")
                                .clicked()
                            {
                                go = Some((n.frame, n.id));
                            }
                        });
                    });
                });
            });
            let (named, cleared) = head.inner;
            clear = cleared;
            if named {
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                state.toggle(ui);
                state.store(ui.ctx());
            }
        }
        if clear {
            let removed = notes::clear_done(&self.scene.bundle)?;
            self.reread_pins();
            self.say(
                Tone::Done,
                format!("removed {removed} done note{}", if removed == 1 { "" } else { "s" }),
            );
        }
        if let Some(id) = reopen {
            notes::set_done(&self.scene.bundle, id, false)?;
            self.reread_pins();
        }
        if let Some((frame, id)) = go {
            self.go_to(frame)?;
            self.open_pin(id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, x: f32, y: f32, frame: u32) -> Note {
        Note {
            id,
            x,
            y,
            frame,
            text: format!("note {id}"),
            author: "user".into(),
            at_ms: 0,
            done: false,
        }
    }

    #[test]
    fn a_press_opens_the_nearest_pin_of_its_frame_within_reach() {
        let pins = vec![
            note(1, 100.0, 100.0, 0),
            note(2, 108.0, 100.0, 0),
            note(3, 100.0, 100.0, 4),
        ];
        assert_eq!(hit_pin(&pins, 0, vec2(106.0, 101.0), 1.0), Some(2));
        assert_eq!(hit_pin(&pins, 0, vec2(98.0, 100.0), 1.0), Some(1));
        assert_eq!(hit_pin(&pins, 0, vec2(130.0, 100.0), 1.0), None);
        assert_eq!(hit_pin(&pins, 0, vec2(130.0, 100.0), 0.5), Some(2));
        assert_eq!(hit_pin(&pins, 4, vec2(101.0, 100.0), 1.0), Some(3));
        assert_eq!(hit_pin(&pins, 2, vec2(101.0, 100.0), 1.0), None);
        assert_eq!(hit_any_pin(&pins, 2, vec2(101.0, 100.0), 1.0), Some((1, 0)));
        assert_eq!(hit_any_pin(&pins, 4, vec2(101.0, 100.0), 1.0), Some((3, 4)));
        assert_eq!(hit_any_pin(&pins, 0, vec2(130.0, 100.0), 1.0), None);
    }
}
