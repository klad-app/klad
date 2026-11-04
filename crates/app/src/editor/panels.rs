use super::layers;
use super::{Editor, Tool, ms};
use crate::commands::{self, Cmd};
use crate::jobs::{Outcome, Spec, Tag};
use crate::layout::{self, Layout, Pane};
use crate::messages::{Message, Tone};
use crate::stats::lock;
use crate::theme::{self, Hint, icon};
use anyhow::Result;
use egui::{Align, Color32, Id, Key, Layout as Flow, Margin, Modifiers, RichText, Sense, UiBuilder, vec2};
use std::time::{Duration, Instant};

fn colour_of(tone: Tone) -> Color32 {
    match tone {
        Tone::Info => theme::TEXT,
        Tone::Done => theme::DONE,
        Tone::Refused => theme::REFUSED,
        Tone::Error => theme::ERROR,
    }
}

fn icon_of(tone: Tone) -> &'static str {
    match tone {
        Tone::Info => icon::INFO,
        Tone::Done => icon::CHECK,
        Tone::Refused | Tone::Error => icon::WARNING,
    }
}

fn ago(at: Instant, now: Instant) -> String {
    let s = now.duration_since(at).as_secs();
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

const STRIP_TOOLS: [Cmd; 9] = [
    Cmd::Brush,
    Cmd::Eraser,
    Cmd::Fill,
    Cmd::Strokes,
    Cmd::Shapes,
    Cmd::Points,
    Cmd::Pen,
    Cmd::ShapeTool,
    Cmd::Note,
];

impl Editor {
    pub(super) fn check(&mut self, f: impl FnOnce(&mut Self) -> Result<()>) {
        if let Err(e) = f(self) {
            self.error(e);
        }
    }

    pub(crate) fn frame(&mut self, ui: &mut egui::Ui) {
        let start = Instant::now();
        let ctx = ui.ctx().clone();
        if !self.themed {
            theme::install(&ctx);
            self.themed = true;
        }
        theme::clear_hints(&ctx);
        if (ctx.zoom_factor() - self.settings.ui_scale).abs() > 1e-3 {
            ctx.set_zoom_factor(self.settings.ui_scale);
        }
        let dt = ui.input(|i| i.unstable_dt);
        if dt < 0.1 {
            lock(&self.stats).frame_ms.push(dt * 1000.0);
        }
        self.pointer = ui.input(|i| i.pointer.latest_pos()).or(self.pointer);
        self.check(|e| e.sync());
        self.tend_jobs();
        self.check(|e| e.tab_keys());
        let window = ctx.content_rect().size();
        self.settings.layout.clamp(window);
        let pressing = ui.input(|i| i.pointer.any_down());
        egui::Panel::bottom("status")
            .exact_size(layout::STATUS)
            .resizable(false)
            .show(ui, |ui| self.status_line(ui));
        if self.settings.layout.shows_timeline() {
            let folded = self.settings.layout.timeline_folded;
            let (lo, hi) = Layout::timeline_range(window);
            let panel = match folded {
                true => egui::Panel::bottom("timeline folded")
                    .exact_size(layout::TIMELINE_FOLDED)
                    .resizable(false),
                false => egui::Panel::bottom("timeline")
                    .default_size(self.settings.layout.timeline_height)
                    .size_range(lo..=hi)
                    .resizable(true),
            };
            let shown = panel.show(ui, |ui| self.check(|e| e.timeline_panel(ui)));
            let height = shown.response.rect.height();
            if !folded && !pressing && (height - self.settings.layout.timeline_height).abs() > 0.5 {
                self.settings.layout.timeline_height = height;
                self.settings_dirty = true;
            }
        }
        if self.settings.layout.shows_strip() {
            egui::Panel::left("strip")
                .exact_size(layout::STRIP)
                .resizable(false)
                .show(ui, |ui| self.strip(ui));
        }
        if self.settings.layout.shows_dock() {
            let (lo, hi) = Layout::dock_range(window);
            let shown = egui::Panel::right("dock")
                .default_size(self.settings.layout.dock_width)
                .size_range(lo..=hi)
                .resizable(true)
                .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(Margin::ZERO))
                .show(ui, |ui| self.dock(ui));
            let width = shown.response.rect.width();
            if !pressing && (width - self.settings.layout.dock_width).abs() > 0.5 {
                self.settings.layout.dock_width = width;
                self.settings_dirty = true;
            }
        }
        if !self.settings.layout.hidden {
            egui::Panel::top("options")
                .resizable(false)
                .show(ui, |ui| self.check(|e| e.tool_options(ui)));
        }
        let panels = ms(start);
        egui::CentralPanel::no_frame().show(ui, |ui| self.check(|e| e.canvas(ui)));
        let after = Instant::now();
        self.windows(&ctx);
        self.import_window(&ctx);
        self.dropped_pictures(&ctx);
        self.tend_thumbs(&ctx);
        if let Some(path) = self.queued_activate.take() {
            self.check(|e| e.activate(path));
        }
        self.check(|e| e.tend_pick());
        self.check(|e| e.pin_popover(&ctx));
        self.check(|e| e.run_queued());
        lock(&self.stats).ui_ms.push((panels + ms(after)) as f32);
        self.save_settings(pressing);
        if let Some(wait) = self.messages.next_change(Instant::now()) {
            ctx.request_repaint_after(wait.max(Duration::from_millis(16)));
        }
        if !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn save_settings(&mut self, pressing: bool) {
        if !self.settings_dirty || pressing {
            return;
        }
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            self.error(format!("saving your settings: {e:#}"));
        }
        self.settings_dirty = false;
    }

    pub(super) fn start_job(&mut self, mut spec: Spec) {
        spec.exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.error(format!("cannot find the klad binary: {e}"));
                return;
            }
        };
        let name = spec.name.clone();
        match self.jobs.start(spec) {
            Ok(_) => self.say(Tone::Info, format!("{name}...")),
            Err(e) => self.error(format!("cannot run {name}: {e}")),
        }
    }

    fn say_job(&mut self, tone: Tone, text: String) {
        self.messages.say(tone, text, None, Instant::now());
    }

    fn tend_jobs(&mut self) {
        self.jobs.poll().into_iter().for_each(|e| {
            let took = e.took.as_secs_f32();
            match e.outcome {
                Outcome::Rerun => self.say_job(
                    Tone::Info,
                    format!("{}: the document changed while it ran, so it runs again", e.name),
                ),
                Outcome::Done(text) => {
                    if e.tag == Tag::Inbetween {
                        self.check(|ed| ed.reload());
                        self.between.report = super::between::report(&e.output);
                    }
                    let detail = if text.is_empty() {
                        String::new()
                    } else {
                        format!(": {text}")
                    };
                    self.measure("last job", format!("{} in {took:.1} s{detail}", e.name));
                    self.say_job(Tone::Done, format!("{} done in {took:.1} s{detail}", e.name));
                }
                Outcome::Failed(text) => {
                    if e.tag == Tag::Inbetween {
                        self.between.report = super::between::report(&e.output);
                    }
                    self.say_job(Tone::Error, format!("{} failed: {text}", e.name))
                }
                Outcome::Cancelled => self.say_job(Tone::Info, format!("{} cancelled", e.name)),
            }
        });
    }

    fn strip(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            STRIP_TOOLS.iter().for_each(|cmd| {
                let r = self.command_sized(ui, *cmd, None, 20.0, vec2(36.0, 36.0));
                self.key_letter(ui, r.rect, *cmd);
            });
            ui.add_space(4.0);
            self.colour_pair(ui);
            ui.separator();
            [Cmd::Undo, Cmd::Redo].iter().for_each(|cmd| {
                self.command_sized(ui, *cmd, None, 18.0, vec2(36.0, 32.0));
            });
        });
        ui.with_layout(Flow::bottom_up(Align::Center), |ui| {
            ui.add_space(6.0);
            [Cmd::Settings, Cmd::Numbers, Cmd::Palette].iter().for_each(|cmd| {
                self.command_sized(ui, *cmd, None, 18.0, vec2(36.0, 32.0));
            });
            self.panels_menu(ui);
            self.document_menu(ui);
        });
    }

    fn document_menu(&mut self, ui: &mut egui::Ui) {
        let button = egui::Button::new(RichText::new(icon::FILE).size(18.0)).min_size(vec2(36.0, 32.0));
        let name = self
            .scene
            .bundle
            .root
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, |ui| {
                ui.label(RichText::new(&name).strong());
                ui.separator();
                [
                    (Cmd::NewDocument, "new drawing"),
                    (Cmd::OpenDocument, "open"),
                    (Cmd::OpenRecent, "recent"),
                    (Cmd::CopyPath, "copy path"),
                    (Cmd::About, "about and licences"),
                ]
                .into_iter()
                .for_each(|(cmd, label)| {
                    if self.command_button(ui, cmd, Some(label)).clicked() {
                        ui.close();
                    }
                });
            })
            .0
            .hint(format!("{name}: new, open, recent drawings, copy its path"));
    }

    fn about_panel(ui: &mut egui::Ui) {
        static LINES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
        let lines = LINES.get_or_init(|| klad_core::notices::NOTICES.lines().collect());
        ui.label(RichText::new(format!("klad {}", env!("CARGO_PKG_VERSION"))).strong());
        ui.label("Source-available licence, copyright Indicode BV. Built on the open source work listed below; klad licences prints the same.");
        ui.add_space(6.0);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let row = ui.text_style_height(&egui::TextStyle::Small);
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .auto_shrink([false, false])
            .show_rows(ui, row, lines.len(), |ui, rows| {
                ui.set_width(600.0);
                lines[rows].iter().for_each(|line| {
                    ui.label(RichText::new(*line).small());
                });
            });
    }

    fn panels_menu(&mut self, ui: &mut egui::Ui) {
        let button = egui::Button::new(RichText::new(icon::SIDEBAR).size(18.0)).min_size(vec2(36.0, 32.0));
        egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, |ui| {
                [
                    (Cmd::ToolPane, Pane::Tool),
                    (Cmd::LayersPane, Pane::Layers),
                    (Cmd::InspectorPane, Pane::Inspector),
                    (Cmd::NotesPane, Pane::Notes),
                    (Cmd::HistoryPane, Pane::History),
                ]
                .into_iter()
                .for_each(|(cmd, pane)| {
                    self.command_check(ui, cmd, pane.title());
                });
                ui.separator();
                self.command_check(ui, Cmd::Timeline, "timeline");
            })
            .0
            .hint("the panels: tick one to show it, untick to close it");
    }

    fn dock(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let bodies = self.settings.layout.bodies(ui.available_height());
        let mut previous: Option<(Pane, bool)> = None;
        bodies.into_iter().for_each(|(pane, body)| {
            self.pane_title(ui, pane, body.is_none(), previous);
            if let Some(height) = body {
                egui::ScrollArea::vertical()
                    .id_salt(("pane", pane))
                    .max_height(height)
                    .min_scrolled_height(height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        egui::Frame::new()
                            .inner_margin(Margin::symmetric(8, 6))
                            .show(ui, |ui| self.check(|e| e.pane_body(ui, pane)));
                    });
            }
            previous = Some((pane, body.is_some()));
        });
    }

    fn pane_title(&mut self, ui: &mut egui::Ui, pane: Pane, collapsed: bool, previous: Option<(Pane, bool)>) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), layout::TITLE), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(42));
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(1.0, Color32::from_gray(24)),
        );
        if let Some((above, true)) = previous {
            let grip = egui::Rect::from_min_size(rect.min, vec2(rect.width(), 5.0));
            let r = ui.interact(grip, Id::unique(("pane grip", pane)), Sense::drag());
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if r.dragged() {
                let dy = r.drag_delta().y;
                self.settings.layout.resize_pane(above, dy);
            }
            if r.drag_stopped() {
                self.settings_dirty = true;
            }
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(4.0, 3.0)))
                .layout(Flow::left_to_right(Align::Center)),
        );
        let caret = if collapsed { icon::FOLDED } else { icon::OPEN };
        let toggle = child
            .add(
                egui::Button::new(RichText::new(caret).size(theme::ICON))
                    .frame(false)
                    .min_size(vec2(theme::TARGET, theme::TARGET)),
            )
            .hint(match collapsed {
                true => format!("open {}", pane.title()),
                false => format!("fold {} to its title", pane.title()),
            });
        child.label(RichText::new(pane.title()).strong());
        let mut close = false;
        child.with_layout(Flow::right_to_left(Align::Center), |ui| {
            close = ui
                .add(
                    egui::Button::new(RichText::new(icon::CLOSE).size(theme::ICON))
                        .frame(false)
                        .min_size(vec2(theme::TARGET, theme::TARGET)),
                )
                .hint(format!("close {}; cmd K brings it back", pane.title()))
                .clicked();
            if pane == Pane::Layers && !collapsed {
                self.check(|e| e.layer_header(ui));
            }
        });
        if toggle.clicked()
            && let Some(p) = self.settings.layout.pane_mut(pane)
        {
            p.collapsed = !p.collapsed;
            self.settings_dirty = true;
        }
        if close {
            self.settings.layout.toggle(pane);
            self.settings_dirty = true;
        }
    }

    fn pane_body(&mut self, ui: &mut egui::Ui, pane: Pane) -> Result<()> {
        match pane {
            Pane::Tool => {
                self.colour_panel(ui)?;
                ui.separator();
                self.brush_tiles(ui);
                Ok(())
            }
            Pane::Layers => self.layers_pane(ui),
            Pane::Inspector => match (self.tool, self.selection.clone()) {
                _ if self.on_marks() => {
                    self.strokes_inspector(ui)?;
                    ui.separator();
                    self.layer_inspector(ui)
                }
                (Tool::Direct, _) => {
                    self.path_inspector(ui)?;
                    ui.separator();
                    self.layer_inspector(ui)
                }
                (_, Some(id)) => self.node_inspector(ui, &id),
                (_, None) => self.layer_inspector(ui),
            },
            Pane::Notes => self.notes_pane(ui),
            Pane::History => self.history_pane(ui),
        }
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let mut cancel = None;
        let mut open_log = false;
        ui.horizontal_centered(|ui| {
            let frames = self.scene.bundle.doc.timeline.frames;
            ui.label(RichText::new(format!("frame {} of {frames}", self.shown_frame())).strong());
            if self.timeline.player.is_some() {
                ui.label(
                    RichText::new(format!("{} playing {}", icon::PLAY, self.timeline.playing)).color(theme::ACCENT),
                );
            }
            ui.separator();
            ui.label(self.draws_into());
            self.jobs.running(now).into_iter().for_each(|(id, name, took)| {
                ui.separator();
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(format!("{name} {:.1} s", took.as_secs_f32()));
                if ui
                    .add(egui::Button::new(icon::CLOSE).min_size(vec2(theme::TARGET, theme::TARGET)))
                    .hint(format!("cancel {name}"))
                    .clicked()
                {
                    cancel = Some(id);
                }
            });
            self.internal_work(now).into_iter().for_each(|what| {
                ui.separator();
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(what);
            });
            ui.with_layout(Flow::right_to_left(Align::Center), |ui| {
                if let Some(view) = self.view {
                    let degrees = view.angle.to_degrees().rem_euclid(360.0);
                    let flat = |text: String| egui::Button::new(RichText::new(text).color(theme::WEAK)).frame(false);
                    let zoom = ui
                        .add(flat(format!("{:.0}%", view.zoom * ui.ctx().pixels_per_point() * 100.0)))
                        .hint("zoom; click for 100%, double-click to fit the canvas");
                    if zoom.double_clicked() {
                        self.queued.push(Cmd::Fit);
                    } else if zoom.clicked() {
                        self.queued.push(Cmd::ActualSize);
                    }
                    if ui
                        .add(flat(format!("{degrees:.0}\u{b0}")))
                        .hint("the view's rotation; click to turn it upright")
                        .clicked()
                    {
                        self.queued.push(Cmd::ResetRotation);
                    }
                    if view.flip
                        && ui
                            .add(flat(format!("{} flipped", icon::FLIP_H)))
                            .hint("the view is mirrored (H); the document is not")
                            .clicked()
                    {
                        self.queued.push(Cmd::FlipView);
                    }
                }
                ui.separator();
                if let Some(m) = self.messages.latest() {
                    let text = RichText::new(format!("{} {}", icon_of(m.tone), m.text)).color(colour_of(m.tone));
                    open_log = ui
                        .add(egui::Button::new(text).frame(false).truncate())
                        .hint("the last message; click for the message log")
                        .clicked();
                }
            });
        });
        if let Some(id) = cancel {
            self.jobs.cancel(id);
        }
        if open_log {
            self.windows.log = true;
        }
    }

    fn internal_work(&self, now: Instant) -> Vec<String> {
        let slow = Duration::from_millis(600);
        let mut work = Vec::new();
        if self.lift.as_ref().is_some_and(|l| l.job.is_some()) && now.duration_since(self.lift_since) > slow {
            work.push("preparing the selection".to_string());
        }
        if self.path_commit.is_some() {
            work.push("drawing the edited path".to_string());
        }
        let loading = self.frames.jobs() > 0 && self.timeline.on_screen != Some(self.scene.frame);
        if self.timeline.player.is_none() && (loading || self.canvas_loading()) {
            work.push(format!("loading frame {}", self.scene.frame));
        }
        work.extend(self.preparing_text());
        work
    }

    fn window(
        ctx: &egui::Context,
        id: &str,
        (title, at): (String, egui::Pos2),
        open: &mut bool,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Window::new(&title)
            .id(Id::unique(id))
            .default_pos(at)
            .title_bar(false)
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&title).strong());
                    ui.with_layout(Flow::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(icon::CLOSE).size(theme::ICON))
                                    .frame(false)
                                    .min_size(vec2(theme::TARGET, theme::TARGET)),
                            )
                            .hint(format!("close {title}"))
                            .clicked()
                        {
                            *open = false;
                        }
                    });
                });
                ui.separator();
                add(ui);
            });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if !self.settings.layout.hidden || self.selected().is_some() {
            self.selection_bar(ctx);
        }
        let screen = ctx.content_rect();
        let layout = &self.settings.layout;
        let dock = if layout.shows_dock() { layout.dock_width } else { 0.0 };
        let strip = if layout.shows_strip() { layout::STRIP } else { 0.0 };
        let timeline = if layout.shows_timeline() {
            layout.timeline_height
        } else {
            0.0
        };
        let above = screen.bottom() - layout::STATUS - timeline;
        let numbers_at = egui::pos2(screen.right() - dock - 380.0, screen.top() + 16.0);
        let settings_at = egui::pos2(screen.center().x - 180.0, screen.top() + 60.0);
        let log_at = egui::pos2(screen.left() + strip + 16.0, (above - 380.0).max(screen.top() + 16.0));
        let near_timeline = egui::pos2(screen.left() + strip + 16.0, (above - 300.0).max(screen.top() + 16.0));
        self.palette_window(ctx);
        self.toasts(ctx);
        if self.settings.layout.numbers {
            let mut open = true;
            Self::window(
                ctx,
                "numbers",
                (format!("{} Numbers", icon::NUMBERS), numbers_at),
                &mut open,
                |ui| self.numbers_panel(ui),
            );
            if !open {
                self.settings.layout.numbers = false;
                self.settings_dirty = true;
            }
        }
        if self.windows.settings {
            let mut open = true;
            Self::window(
                ctx,
                "settings",
                (format!("{} Settings", icon::SETTINGS), settings_at),
                &mut open,
                |ui| self.settings_panel(ui),
            );
            self.windows.settings &= open;
        }
        if self.windows.brush {
            let mut open = true;
            Self::window(
                ctx,
                "brush",
                (format!("{} Brush", icon::BRUSH), settings_at),
                &mut open,
                |ui| self.check(|e| e.brush_window_ui(ui)),
            );
            self.windows.brush &= open;
        }
        if self.windows.about {
            let mut open = true;
            Self::window(
                ctx,
                "about",
                (format!("{} About klad", icon::INFO), settings_at),
                &mut open,
                Self::about_panel,
            );
            self.windows.about &= open;
        }
        if self.windows.log {
            let mut open = true;
            Self::window(
                ctx,
                "log",
                (format!("{} Messages", icon::LOG), log_at),
                &mut open,
                |ui| self.log_panel(ui),
            );
            self.windows.log &= open;
        }
        if self.windows.onion {
            let mut open = true;
            Self::window(
                ctx,
                "onion",
                (format!("{} Onion skin", icon::ONION), near_timeline),
                &mut open,
                |ui| self.check(|e| e.onion_settings(ui)),
            );
            self.windows.onion &= open;
        }
        if self.windows.link {
            let mut open = true;
            let mut chosen = None;
            let drawings: Vec<(String, u32)> = self
                .scene
                .active_layer()
                .map(|l| {
                    l.cels()
                        .iter()
                        .filter(|c| c.link.is_none())
                        .map(|c| (c.id.clone(), c.frame))
                        .collect()
                })
                .unwrap_or_default();
            let frame = self.scene.frame;
            let thumbs: Vec<_> = drawings.iter().map(|(id, _)| self.thumb(ctx, id)).collect();
            Self::window(
                ctx,
                "link",
                (format!("{} Show a drawing here", icon::LINK), near_timeline),
                &mut open,
                |ui| {
                    ui.label(format!("frame {frame} will show the drawing you pick, as a link"));
                    ui.horizontal_wrapped(|ui| {
                        drawings.iter().zip(&thumbs).for_each(|((id, f), thumb)| {
                            let text = format!("frame {f}");
                            let button = match thumb {
                                Some(t) => egui::Button::image_and_text(
                                    egui::Image::new(t).fit_to_exact_size(egui::vec2(
                                        layers::THUMB.0 as f32 * 2.0,
                                        layers::THUMB.1 as f32 * 2.0,
                                    )),
                                    text,
                                ),
                                None => egui::Button::new(text),
                            };
                            if ui
                                .add(button)
                                .hint(format!("show {id} again here; drawing on it changes both"))
                                .clicked()
                            {
                                chosen = Some(id.clone());
                            }
                        });
                    });
                },
            );
            if let Some(id) = chosen {
                self.check(|e| e.link_here(id));
                open = false;
            }
            self.windows.link &= open;
        }
        if self.windows.between {
            let mut open = true;
            Self::window(
                ctx,
                "between",
                (format!("{} In-betweens", icon::INBETWEEN), near_timeline),
                &mut open,
                |ui| self.check(|e| e.between_window(ui)),
            );
            self.windows.between &= open;
        }
        if self.windows.clip {
            let mut open = true;
            let title = self
                .current_clip()
                .map_or("Clip".to_string(), |c| format!("Clip {}", c.name));
            Self::window(
                ctx,
                "clip",
                (format!("{} {title}", icon::CLIP_BAR), near_timeline),
                &mut open,
                |ui| self.check(|e| e.clip_window(ui)),
            );
            self.windows.clip &= open;
        }
    }

    fn numbers_panel(&mut self, ui: &mut egui::Ui) {
        let r = &self.readout;
        ui.add(egui::ProgressBar::new(r.pressure).text(format!("pressure {:.3}", r.pressure)));
        ui.label(format!(
            "tilt {:.2}, {:.2}   {}{}   {} samples/s",
            r.tilt.0,
            r.tilt.1,
            if r.tablet { "tablet" } else { "mouse" },
            if r.eraser { ", eraser end" } else { "" },
            r.samples.len()
        ));
        let lines = {
            let s = lock(&self.stats);
            [
                format!(
                    "frame        {}  ({:.0} fps)",
                    s.frame_ms.summary(),
                    1000.0 / s.frame_ms.avg().max(0.001)
                ),
                format!("pen to GPU   {}", s.latency_ms.summary()),
                format!("ui           {}", s.ui_ms.spread()),
                format!("compose      {}", s.compose_ms.summary()),
                format!("tiles        {}", s.tiles.summary()),
                format!("upload       {}", s.upload_ms.summary()),
                format!("frame step   {}", s.step_ms.summary()),
                format!("frame upload {}", s.frame_upload_ms.summary()),
            ]
        };
        lines.iter().for_each(|l| {
            ui.label(RichText::new(l).monospace());
        });
        ui.label(
            RichText::new(format!(
                "frame cache  {} frames, {} MB, {} composing",
                self.frames.len(),
                self.frames.bytes() >> 20,
                self.frames.jobs()
            ))
            .monospace(),
        );
        ui.label(RichText::new(format!("drawings     {} MB in memory", self.scene.cache_bytes() >> 20)).monospace());
        ui.separator();
        let measured: Vec<(String, String)> = self.measured.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        measured.into_iter().for_each(|(k, v)| {
            ui.label(RichText::new(format!("{k}: {v}")).small());
        });
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::Spin, "spin (fps test)");
            self.command_button(ui, Cmd::ResetNumbers, Some("reset"));
        });
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pen calibration");
        self.pen_panel(ui);
        ui.separator();
        ui.heading("Defaults");
        self.defaults_section(ui);
        ui.separator();
        ui.heading("Keys");
        self.keys_section(ui);
        ui.separator();
        ui.heading("Layout");
        self.scale_section(ui);
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::Numbers, "numbers panel");
            self.command_check(ui, Cmd::Timeline, "timeline");
            if ui
                .button("reset the layout")
                .hint("every panel back where it starts, at its first size")
                .clicked()
            {
                self.settings.layout = Layout::default();
                self.settings_dirty = true;
            }
        });
        ui.small("Tab hides the panels, shift Tab the dock, cmd K finds any command.");
    }

    fn log_panel(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let log: Vec<Message> = self.messages.log().rev().cloned().collect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if log.is_empty() {
                ui.label("no messages yet");
            }
            log.iter().for_each(|m| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(icon_of(m.tone)).color(colour_of(m.tone)));
                    ui.label(RichText::new(ago(m.at, now)).color(theme::WEAK).small());
                    let times = if m.times > 1 {
                        format!(" (x{})", m.times)
                    } else {
                        String::new()
                    };
                    ui.label(format!("{}{times}", m.text));
                });
            });
        });
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let screen = ctx.content_rect();
        let shown: Vec<Message> = self.messages.toasts(now).into_iter().cloned().collect();
        let mut dismiss = None;
        let mut near = 0.0;
        let mut corner = screen.bottom() - layout::STATUS - 10.0;
        shown.iter().rev().for_each(|m| {
            let (pos, pivot) = match m.anchor {
                Some(p) => {
                    let at = p + vec2(18.0, 16.0 + near);
                    near += 38.0;
                    (at, egui::Align2::LEFT_TOP)
                }
                None => {
                    let at = egui::pos2(screen.right() - 12.0, corner);
                    corner -= 40.0;
                    (at, egui::Align2::RIGHT_BOTTOM)
                }
            };
            let sticky = m.tone == Tone::Error;
            egui::Area::new(Id::unique(("toast", m.id)))
                .order(egui::Order::Tooltip)
                .pivot(pivot)
                .fixed_pos(pos)
                .interactable(sticky)
                .constrain(true)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(Color32::from_gray(30))
                        .stroke(egui::Stroke::new(1.0, colour_of(m.tone)))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(icon_of(m.tone))
                                        .color(colour_of(m.tone))
                                        .size(theme::ICON),
                                );
                                ui.label(&m.text);
                                if sticky
                                    && ui
                                        .add(
                                            egui::Button::new(icon::CLOSE).min_size(vec2(theme::TARGET, theme::TARGET)),
                                        )
                                        .hint("dismiss; the message log keeps it")
                                        .clicked()
                                {
                                    dismiss = Some(m.id);
                                }
                            });
                        });
                });
        });
        if let Some(id) = dismiss {
            self.messages.dismiss(id);
        }
    }

    fn palette_window(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.palette.take() else {
            return;
        };
        let recent = self.recent_commands();
        let found = commands::rank(&state.query, &recent);
        let mut run = None;
        let modal = egui::Modal::new(Id::unique("palette")).show(ctx, |ui| {
            ui.set_width(560.0);
            let (down, up, enter) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                )
            });
            let last = found.len().saturating_sub(1);
            state.selected = match (down, up) {
                (true, _) => (state.selected + 1).min(last),
                (_, true) => state.selected.saturating_sub(1),
                _ => state.selected.min(last),
            };
            let label = ui.label(RichText::new(format!("{} Find a command", icon::SEARCH)).strong());
            let edit = ui
                .add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("type what you want to do, or a key")
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id)
                .hint("type a few letters; arrows pick, Return runs, Esc closes");
            if !state.opened {
                edit.request_focus();
                state.opened = true;
            }
            if edit.changed() {
                state.selected = 0;
            }
            if enter {
                run = found.get(state.selected).copied();
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                if found.is_empty() {
                    ui.label("nothing matches");
                }
                found.iter().enumerate().for_each(|(i, cmd)| {
                    let spec = cmd.spec();
                    let reason = self.can_run(*cmd);
                    let colour = if reason.is_some() { theme::WEAK } else { theme::TEXT };
                    let row = ui
                        .add(
                            egui::Button::new(RichText::new(format!("{}  {}", spec.icon, spec.name)).color(colour))
                                .right_text(RichText::new(self.bindings.label(*cmd)).color(theme::WEAK))
                                .selected(i == state.selected)
                                .min_size(vec2(ui.available_width(), theme::TARGET)),
                        )
                        .hint(self.command_hint(*cmd));
                    if i == state.selected && (down || up) {
                        row.scroll_to_me(None);
                    }
                    if let Some(reason) = reason.filter(|_| !spec.is_hold()) {
                        ui.label(
                            RichText::new(format!("    {}: {reason}", spec.group.name()))
                                .small()
                                .color(theme::WEAK),
                        );
                    }
                    if row.clicked() {
                        run = Some(*cmd);
                    }
                });
            });
        });
        match run {
            Some(cmd) => self.queued.push(cmd),
            None if !modal.should_close() => self.palette = Some(state),
            None => {}
        }
    }
}
