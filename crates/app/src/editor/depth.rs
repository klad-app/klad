use super::Editor;
use crate::color;
use crate::messages::Tone;
use crate::theme::Hint;
use anyhow::Result;
use egui::{Color32, Key, Rect, TextureHandle, TextureOptions, pos2, vec2};
use klad_core::doc::Layer;
use klad_core::look::Atmosphere;
use klad_core::ops::Op;
use serde_json::json;
use std::sync::mpsc::Receiver;

type Planes = Vec<(f32, egui::ColorImage)>;

#[derive(Default)]
pub(super) struct Parallax {
    key: (u64, u32),
    pub(super) planes: Vec<(f32, TextureHandle)>,
    job: Option<Receiver<Result<Planes>>>,
}

pub(super) const SHIFT: f32 = 0.08;

impl Editor {
    pub(super) fn depth_section(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        if !self.scene.bundle.doc.layers.iter().any(|l| l.id == layer.id) {
            return Ok(());
        }
        ui.separator();
        let id = layer.id.clone();
        ui.horizontal(|ui| {
            let depth = layer.depth.unwrap_or(0.0);
            if let Some(v) = self.number(
                ui,
                &format!("{id}.depth"),
                "depth ",
                depth,
                0.0..=100.0,
                0.02,
                "how far away this plane is: 0 is near; deeper planes take more of the atmosphere and move less in the parallax preview (hold D)",
            ) {
                let value = if v <= 0.0 { json!(null) } else { json!(v) };
                self.check(|e| e.set(&id, vec![("depth".into(), value)]));
            }
        });
        let air = self.scene.bundle.doc.atmosphere.clone();
        let mut on = air.is_some();
        if ui
            .checkbox(&mut on, "atmosphere")
            .hint("fog, blur and paler colour for the planes with a depth, more the deeper they are")
            .changed()
        {
            return self.apply(vec![Op::Atmosphere(on.then(Atmosphere::default))]);
        }
        let Some(air) = air else {
            return Ok(());
        };
        let mut next = air.clone();
        let current = self.color;
        ui.horizontal_wrapped(|ui| {
            let fog = color::rgba_of(&air.color).unwrap_or([0.6, 0.65, 0.7, 1.0]);
            if color::swatch(ui, fog, false, crate::theme::TARGET)
                .hint(format!(
                    "the fog colour, {}; click to use the current colour",
                    air.color
                ))
                .clicked()
            {
                next.color = color::hex(current);
            }
            let mut graded = air.top.is_some();
            if ui
                .checkbox(&mut graded, "by height")
                .hint("a second fog colour at the top of the picture, blending down to the first")
                .changed()
            {
                next.top = graded.then(|| color::hex(current));
            }
            if let Some(top) = &air.top {
                let rgba = color::rgba_of(top).unwrap_or([0.8, 0.85, 0.9, 1.0]);
                if color::swatch(ui, rgba, false, crate::theme::TARGET)
                    .hint(format!(
                        "the fog colour at the top, {top}; click to use the current colour"
                    ))
                    .clicked()
                {
                    next.top = Some(color::hex(current));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            [
                ("density ", air.density, "how fast the fog thickens with depth"),
                ("blur ", air.blur, "px of blur per unit of depth"),
                ("paler ", air.desaturate, "how much colour each unit of depth takes out"),
            ]
            .iter()
            .enumerate()
            .for_each(|(i, (label, value, hint))| {
                if let Some(v) = self.number(ui, &format!("air.{i}"), label, *value, 0.0..=20.0, 0.01, hint) {
                    match i {
                        0 => next.density = v,
                        1 => next.blur = v,
                        _ => next.desaturate = v,
                    }
                }
            });
        });
        if next != air {
            return self.apply(vec![Op::Atmosphere(Some(next))]);
        }
        Ok(())
    }

    pub(super) fn parallax(&mut self, ui: &egui::Ui, view: crate::view::View, rect: Rect) {
        let typing = ui.ctx().egui_wants_keyboard_input();
        let held = !typing && ui.input(|i| i.key_down(Key::D) && !i.modifiers.any());
        if !held {
            return;
        }
        let key = (self.scene.bundle.doc.rev, self.scene.frame);
        if self.depth.key != key || (self.depth.planes.is_empty() && self.depth.job.is_none()) {
            let job = self.scene.planes_job();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(job());
                ctx.request_repaint();
            });
            self.depth = Parallax {
                key,
                planes: Vec::new(),
                job: Some(rx),
            };
        }
        if let Some(done) = self.depth.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.depth.job = None;
            match done {
                Ok(images) => {
                    self.depth.planes = images
                        .into_iter()
                        .enumerate()
                        .map(|(i, (depth, image))| {
                            (
                                depth,
                                ui.ctx()
                                    .load_texture(format!("plane {i}"), image, TextureOptions::LINEAR),
                            )
                        })
                        .collect();
                }
                Err(e) => self.say(Tone::Error, format!("the parallax preview could not be made: {e:#}")),
            }
        }
        let painter = ui.painter().with_clip_rect(rect);
        if self.depth.planes.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "preparing the planes...",
                egui::FontId::proportional(16.0),
                Color32::WHITE,
            );
            ui.ctx().request_repaint();
            return;
        }
        let (w, h) = self.scene.size();
        let canvas = Rect::from_two_pos(
            self.screen(view, rect, vec2(0.0, 0.0)),
            self.screen(view, rect, vec2(w as f32, h as f32)),
        );
        let pointer = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
        let offset = pointer - rect.center();
        painter.rect_filled(rect, 0.0, Color32::from_gray(30));
        self.depth.planes.iter().for_each(|(depth, texture)| {
            let shift = offset * SHIFT / (1.0 + depth.max(0.0));
            painter.image(
                texture.id(),
                canvas.translate(shift),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        });
        ui.ctx().request_repaint();
    }
}
