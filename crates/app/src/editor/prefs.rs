use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use egui::{RichText, vec2};

impl Editor {
    pub(super) fn defaults_section(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let d = &mut self.settings.defaults;
        ui.horizontal_wrapped(|ui| {
            let now = match (&d.brush, d.size) {
                (Some(b), Some(s)) => format!("new windows start with {b} at {s:.1} px"),
                _ => "new windows start with the ink brush at 12 px".to_string(),
            };
            ui.label(now);
        });
        let (brush, size) = (self.brush.clone(), self.size);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("use the current brush and size")
                .hint("the brush and size you have now become the ones every window starts with")
                .clicked()
            {
                d.brush = Some(brush.clone());
                d.size = Some(size);
                changed = true;
            }
            if ui
                .add_enabled(d.brush.is_some(), egui::Button::new("forget them"))
                .hint("start with the ink brush at 12 px again")
                .clicked()
            {
                d.brush = None;
                d.size = None;
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("shift K exposes on");
            [1, 2, 3].into_iter().for_each(|n| {
                if ui
                    .selectable_value(&mut d.exposure, n, format!("{n}s"))
                    .hint("how many frames each new drawing holds when shift K makes it")
                    .clicked()
                {
                    changed = true;
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("onion before");
            changed |= ui
                .add(egui::DragValue::new(&mut d.onion_before).range(0..=8))
                .hint("drawings shown before, in red, when onion skin is on")
                .changed();
            ui.label("after");
            changed |= ui
                .add(egui::DragValue::new(&mut d.onion_after).range(0..=8))
                .hint("drawings shown after, in green")
                .changed();
            ui.label("opacity");
            changed |= ui
                .add(egui::DragValue::new(&mut d.onion_opacity).range(0.05..=1.0).speed(0.01))
                .hint("how strong the nearest ghost is")
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("around the canvas");
            changed |= ui
                .color_edit_button_srgb(&mut d.surround)
                .hint("the grey around the canvas; neutral keeps colours true")
                .changed();
            if ui.button("neutral grey").hint("back to the neutral grey").clicked() {
                d.surround = theme::SURROUND;
                changed = true;
            }
        });
        if changed {
            self.settings_dirty = true;
            self.use_defaults();
        }
    }

    pub(super) fn scale_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = ui.label("UI scale");
            let mut scale = self.settings.ui_scale;
            let r = ui
                .add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05))
                .labelled_by(label.id)
                .hint("bigger targets for a pen on a big tablet; everything but the canvas grows");
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                self.settings.ui_scale = scale;
                self.settings_dirty = true;
            }
        });
        let mut check = self.settings.check_updates;
        if ui
            .checkbox(&mut check, "check for a new version once a day")
            .hint("asks the release page on GitHub at most once a day; nothing is installed without you")
            .changed()
        {
            self.settings.check_updates = check;
            self.settings_dirty = true;
        }
    }

    pub(super) fn keys_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = ui.label("find");
            ui.add(egui::TextEdit::singleline(&mut self.key_filter).desired_width(180.0))
                .labelled_by(label.id)
                .hint("type part of a command's name or key");
            if ui
                .button("reset every key")
                .hint("every command back to its first key")
                .clicked()
            {
                self.bindings.reset_all();
                self.settings.keys = self.bindings.to_settings();
                self.settings_dirty = true;
                self.say(Tone::Done, "every key is back to its first one");
            }
        });
        if let Some(cmd) = self.capturing {
            ui.label(
                RichText::new(format!(
                    "press the new key for {} (Esc keeps the old one)",
                    cmd.spec().name
                ))
                .color(theme::ACCENT),
            );
        }
        let filter = self.key_filter.trim().to_lowercase();
        let shown: Vec<Cmd> = Cmd::ALL
            .into_iter()
            .filter(|c| {
                filter.is_empty()
                    || c.spec().name.contains(filter.as_str())
                    || self.bindings.label(*c).to_lowercase().contains(filter.as_str())
            })
            .collect();
        let mut action: Option<(Cmd, &str)> = None;
        egui::ScrollArea::vertical()
            .id_salt("keys list")
            .max_height(320.0)
            .show(ui, |ui| {
                shown.iter().for_each(|cmd| {
                    let spec = cmd.spec();
                    ui.horizontal(|ui| {
                        let key = self.bindings.label(*cmd);
                        let key = if key.is_empty() { "no key".to_string() } else { key };
                        [(spec.name.to_string(), 220.0, theme::TEXT), (key, 110.0, theme::WEAK)]
                            .into_iter()
                            .for_each(|(text, width, colour)| {
                                ui.allocate_ui_with_layout(
                                    vec2(width, theme::TARGET),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(width);
                                        ui.add(egui::Label::new(RichText::new(text).color(colour)).truncate());
                                    },
                                );
                            });
                        if spec.is_hold() {
                            return;
                        }
                        if ui
                            .button("change")
                            .hint(format!("press a new key for {}", spec.name))
                            .clicked()
                        {
                            action = Some((*cmd, "change"));
                        }
                        if ui
                            .add_enabled(!self.bindings.keys(*cmd).is_empty(), egui::Button::new("no key"))
                            .hint("take its key away; it stays in the palette and on its button")
                            .clicked()
                        {
                            action = Some((*cmd, "clear"));
                        }
                        if ui
                            .add_enabled(!self.bindings.is_default(*cmd), egui::Button::new("reset"))
                            .hint("its first key again")
                            .clicked()
                        {
                            action = Some((*cmd, "reset"));
                        }
                    });
                });
            });
        match action {
            Some((cmd, "change")) => self.capturing = Some(cmd),
            Some((cmd, "clear")) => {
                self.bindings.clear(cmd);
                self.settings.keys = self.bindings.to_settings();
                self.settings_dirty = true;
            }
            Some((cmd, _)) => {
                let before = self.bindings.clone();
                self.bindings.reset(cmd);
                let first = self.bindings.keys(cmd);
                let clash = first.iter().find_map(|k| self.bindings.taken_by(*k, cmd));
                match clash {
                    Some(other) => {
                        self.bindings = before;
                        self.say(
                            Tone::Refused,
                            format!("its first key now runs {}; change that one first", other.spec().name),
                        );
                    }
                    None => {
                        self.settings.keys = self.bindings.to_settings();
                        self.settings_dirty = true;
                    }
                }
            }
            None => {}
        }
    }
}
