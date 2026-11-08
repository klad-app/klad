use super::{Drag, Editor, Tool};
use crate::messages::Tone;
use anyhow::{Context, Result};
use egui::{Key, Modifiers, RichText, Vec2, vec2};
use klad_core::doc::Kind;
use klad_core::ops::{Op, ShiftSpec};
use klad_render::shift::{Targets, shifted, targets};

pub(super) struct Shifting {
    pub layer: String,
    pub every: bool,
    pub frame: u32,
    pub rev: u64,
    pub found: Targets,
    pub corners: [Vec2; 4],
}

fn corners(found: &Targets) -> Option<[Vec2; 4]> {
    let [x0, y0, x1, y1] = found.bounds?;
    Some([vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)])
}

impl Editor {
    pub(super) fn start_shift(&mut self) -> Result<()> {
        let layer = self
            .scene
            .active_id()
            .context("pick a layer or group to transform first")?;
        let frame = self.scene.frame;
        let found = targets(&self.scene.bundle, &layer, Some(frame))?;
        let Some(box_) = corners(&found) else {
            self.say(
                Tone::Refused,
                format!("{} has nothing to transform at this frame", found.name),
            );
            return Ok(());
        };
        self.say(
            Tone::Info,
            format!(
                "transforming {}: drag inside to move, a corner to scale, outside to turn; Esc when done",
                found.name
            ),
        );
        self.shifting = Some(Shifting {
            layer,
            every: true,
            frame,
            rev: self.scene.bundle.doc.rev,
            found,
            corners: box_,
        });
        self.selection = None;
        self.picked = None;
        self.tool = Tool::Select;
        Ok(())
    }

    pub(super) fn end_shift(&mut self) {
        if self.shifting.take().is_some() {
            self.say(Tone::Info, "layer transform done");
        }
    }

    pub(super) fn shift_keys(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(shifting) = &self.shifting else {
            return Ok(());
        };
        if self.tool != Tool::Select || self.scene.active_id().as_deref() != Some(shifting.layer.as_str()) {
            self.shifting = None;
            return Ok(());
        }
        if !self.keys_busy {
            let done = ui.input_mut(|i| {
                i.consume_key(Modifiers::NONE, Key::Escape) || i.consume_key(Modifiers::NONE, Key::Enter)
            });
            if done {
                self.end_shift();
                return Ok(());
            }
        }
        let (rev, frame) = (self.scene.bundle.doc.rev, self.scene.frame);
        if self.drag.is_none() && (shifting.rev != rev || shifting.frame != frame) {
            let layer = shifting.layer.clone();
            let found = targets(&self.scene.bundle, &layer, Some(frame))?;
            match (corners(&found), self.shifting.as_mut()) {
                (Some(box_), Some(s)) => {
                    s.found = found;
                    s.corners = box_;
                    s.rev = rev;
                    s.frame = frame;
                }
                _ => self.shifting = None,
            }
        }
        Ok(())
    }

    pub(super) fn shift_handles(&self) -> Option<(String, [Vec2; 4])> {
        let s = self.shifting.as_ref().filter(|_| self.tool == Tool::Select)?;
        Some((format!("layer:{}", s.layer), s.corners))
    }

    pub(super) fn shift_ops(&self, drag: &Drag, every: bool) -> Result<Option<Vec<Op>>> {
        let Some(s) = &self.shifting else {
            return Ok(None);
        };
        let mut transform = drag.mark_transform();
        let only = (!every).then_some(s.frame);
        let found = match every {
            true => targets(&self.scene.bundle, &s.layer, None)?,
            false => s.found.clone(),
        };
        let resolved = shifted(&found, &mut transform)?;
        Ok(Some(vec![Op::Shift(ShiftSpec {
            layer: s.layer.clone(),
            transform,
            only,
            resolved: Some(resolved),
        })]))
    }

    pub(super) fn shift_cels(&self) -> Vec<String> {
        self.shifting
            .as_ref()
            .map(|s| s.found.marks.iter().map(|(cel, _)| cel.clone()).collect())
            .unwrap_or_default()
    }

    pub(super) fn finish_shift_drag(&mut self, drag: &Drag) -> Result<()> {
        if !drag.moved() {
            return Ok(());
        }
        let every = self.shifting.as_ref().is_some_and(|s| s.every);
        let (Some(shown), Some(ops)) = (self.shift_ops(drag, false)?, self.shift_ops(drag, every)?) else {
            self.drop_preview();
            return Ok(());
        };
        let before = self.scene.bundle.doc.rev;
        let dirty = self.commit(ops)?;
        self.finish_preview(shown, dirty, before);
        Ok(())
    }

    pub(super) fn shift_options(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.shifting.as_mut() else {
            return;
        };
        let kind = match self.scene.bundle.doc.layer(&s.layer).map(|l| l.kind()) {
            Some(Kind::Group) => "group",
            _ => "layer",
        };
        ui.label(RichText::new(format!("transforming the {kind} {}", s.found.name)).strong());
        ui.checkbox(&mut s.every, "every drawing").on_hover_text(
            "on: every drawing on it moves (with its bones and poses); off: only the drawing shown here",
        );
        ui.label(RichText::new("drag inside, a corner or outside; Esc: done").color(crate::theme::WEAK))
            .on_hover_text("drag inside to move, a corner to scale, an edge to stretch, outside to turn; Esc, Return or a click outside ends it");
    }
}
