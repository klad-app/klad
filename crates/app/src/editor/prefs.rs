use super::Editor;
use crate::commands::Cmd;
use crate::layout::Layout;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use egui::{Align, RichText, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Pen,
    Drawing,
    Keys,
    Layout,
    Updates,
}

impl Section {
    pub(super) const ALL: [Section; 5] = [
        Section::Pen,
        Section::Drawing,
        Section::Keys,
        Section::Layout,
        Section::Updates,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Section::Pen => "Pen",
            Section::Drawing => "Drawing",
            Section::Keys => "Keys",
            Section::Layout => "Layout",
            Section::Updates => "Updates",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Pen => icon::PEN,
            Section::Drawing => icon::BRUSH,
            Section::Keys => icon::KEYBOARD,
            Section::Layout => icon::SIDEBAR,
            Section::Updates => icon::DOWNLOAD,
        }
    }

    fn about(self) -> &'static str {
        match self {
            Section::Pen => {
                "How hard you press, and the pressure your strokes record. Press on the tablet and watch the bars."
            }
            Section::Drawing => "What every new window starts with.",
            Section::Keys => "The key for every command. Change one by pressing the new key.",
            Section::Layout => "The size of everything around the canvas, and the panels.",
            Section::Updates => "klad asks its release page for a newer version. It never installs one by itself.",
        }
    }
}

const SIDEBAR: f32 = 160.0;
const LABELS: f32 = 150.0;

pub(super) fn grid(ui: &mut egui::Ui, id: &str, rows: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(LABELS)
        .spacing(vec2(16.0, 10.0))
        .show(ui, rows);
}

pub(super) fn row_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(RichText::new(text).color(theme::WEAK))
}

impl Editor {
    pub(super) fn settings_modal(&mut self, ctx: &egui::Context) {
        if !self.windows.settings {
            return;
        }
        let height = (ctx.content_rect().height() - 160.0).clamp(280.0, 520.0);
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} Settings", icon::SETTINGS))
                        .strong()
                        .size(theme::HEADING),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    close = ui
                        .add(egui::Button::new(RichText::new(icon::CLOSE).size(theme::ICON)).frame(false))
                        .hint("close the settings (Esc)")
                        .clicked();
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(SIDEBAR, height),
                    egui::Layout::top_down_justified(Align::Min),
                    |ui| {
                        ui.set_min_size(vec2(SIDEBAR, height));
                        Section::ALL.into_iter().for_each(|section| {
                            let text = RichText::new(format!("{}  {}", section.icon(), section.name()));
                            if ui
                                .add(
                                    egui::Button::selectable(self.settings_section == section, text)
                                        .min_size(vec2(SIDEBAR, 32.0)),
                                )
                                .hint(section.about())
                                .clicked()
                            {
                                self.settings_section = section;
                            }
                        });
                    },
                );
                ui.separator();
                let width = ui.available_width();
                ui.allocate_ui_with_layout(vec2(width, height), egui::Layout::top_down(Align::Min), |ui| {
                    ui.set_min_size(vec2(width, height));
                    let section = self.settings_section;
                    ui.label(RichText::new(section.name()).strong().size(theme::HEADING));
                    ui.add(egui::Label::new(RichText::new(section.about()).color(theme::WEAK)).wrap());
                    ui.add_space(12.0);
                    match section {
                        Section::Keys => self.keys_section(ui),
                        _ => {
                            egui::ScrollArea::vertical()
                                .id_salt(("settings", section.name()))
                                .auto_shrink([false, false])
                                .show(ui, |ui| match section {
                                    Section::Pen => self.pen_panel(ui),
                                    Section::Drawing => self.defaults_section(ui),
                                    Section::Layout => self.layout_section(ui),
                                    _ => self.updates_section(ui),
                                });
                        }
                    }
                });
            });
        });
        if close || (modal.should_close() && self.capturing.is_none()) {
            self.windows.settings = false;
        }
    }

    pub(super) fn defaults_section(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let (brush, size) = (self.brush.clone(), self.size);
        let d = &mut self.settings.defaults;
        grid(ui, "settings drawing", |ui| {
            row_label(ui, "Brush");
            ui.vertical(|ui| {
                ui.label(match (&d.brush, d.size) {
                    (Some(b), Some(s)) => format!("{b} at {s:.1} px"),
                    _ => "the ink brush at 12 px".to_string(),
                });
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
            });
            ui.end_row();
            row_label(ui, "New drawings hold");
            ui.horizontal(|ui| {
                [1, 2, 3].into_iter().for_each(|n| {
                    if ui
                        .selectable_value(
                            &mut d.exposure,
                            n,
                            format!("{n} frame{}", if n == 1 { "" } else { "s" }),
                        )
                        .hint("how many frames each new drawing holds when shift K makes it")
                        .clicked()
                    {
                        changed = true;
                    }
                });
            });
            ui.end_row();
            row_label(ui, "Onion skin");
            ui.horizontal(|ui| {
                ui.label("before");
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
            ui.end_row();
            row_label(ui, "Around the canvas");
            ui.horizontal(|ui| {
                changed |= ui
                    .color_edit_button_srgb(&mut d.surround)
                    .hint("the grey around the canvas; neutral keeps colours true")
                    .changed();
                if ui.button("neutral grey").hint("back to the neutral grey").clicked() {
                    d.surround = theme::SURROUND;
                    changed = true;
                }
            });
            ui.end_row();
        });
        if changed {
            self.settings_dirty = true;
            self.use_defaults();
        }
    }

    fn layout_section(&mut self, ui: &mut egui::Ui) {
        grid(ui, "settings layout", |ui| {
            let label = row_label(ui, "Interface size");
            let mut scale = self.settings.ui_scale;
            ui.spacing_mut().slider_width = 220.0;
            let r = ui
                .add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05))
                .labelled_by(label.id)
                .hint("bigger targets for a pen on a big tablet; everything but the canvas grows");
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                self.settings.ui_scale = scale;
                self.settings_dirty = true;
            }
            ui.end_row();
            row_label(ui, "Panels");
            ui.vertical(|ui| {
                self.command_check(ui, Cmd::Timeline, "timeline");
                self.command_check(ui, Cmd::Numbers, "numbers");
                if ui
                    .button("reset the layout")
                    .hint("every panel back where it starts, at its first size")
                    .clicked()
                {
                    self.settings.layout = Layout::default();
                    self.settings_dirty = true;
                }
            });
            ui.end_row();
        });
        ui.add_space(16.0);
        ui.label(
            RichText::new("Tab hides the panels, shift Tab the dock, cmd K finds any command.").color(theme::WEAK),
        );
    }

    fn updates_section(&mut self, ui: &mut egui::Ui) {
        grid(ui, "settings updates", |ui| {
            row_label(ui, "This version");
            ui.label(format!("klad {}", crate::VERSION));
            ui.end_row();
            row_label(ui, "Every day");
            let mut check = self.settings.check_updates;
            if ui
                .checkbox(&mut check, "check for a new version once a day")
                .hint("asks the release page on GitHub at most once a day; nothing is installed without you")
                .changed()
            {
                self.settings.check_updates = check;
                self.settings_dirty = true;
            }
            ui.end_row();
            row_label(ui, "Now");
            ui.vertical(|ui| self.update_rows(ui));
            ui.end_row();
        });
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
            .max_height(ui.available_height())
            .auto_shrink([false, false])
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
