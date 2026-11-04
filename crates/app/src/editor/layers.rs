use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use anyhow::Result;
use egui::{Color32, ColorImage, Rect, RichText, Sense, TextureHandle, TextureOptions, UiBuilder, vec2};
use klad_brush::Raster;
use klad_core::doc::{Blend, Kind, Layer, Loc, MASK};
use klad_core::ops::{Op, Place};
use rayon::prelude::*;
use serde_json::{Map, json};
use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, channel};

pub use crate::composite::{THUMB, thumbnail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Where {
    Panel,
    Timeline,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LayerRow {
    pub path: Vec<usize>,
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub depth: usize,
    pub visible: bool,
    pub locked: bool,
    pub clip: bool,
    pub lock_alpha: bool,
    pub has_mask: bool,
    pub mask: bool,
    pub folded: bool,
    pub cel: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Rename {
    pub id: String,
    pub text: String,
    pub place: Where,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Drop {
    Above(String),
    Below(String),
    Into(String),
}

impl Drop {
    fn place(&self) -> Place {
        match self {
            Drop::Above(id) => Place {
                above: Some(id.clone()),
                ..Place::default()
            },
            Drop::Below(id) => Place {
                below: Some(id.clone()),
                ..Place::default()
            },
            Drop::Into(id) => Place {
                into: Some(id.clone()),
                ..Place::default()
            },
        }
    }
}

pub(super) fn drop_at(rows: &[(Rect, LayerRow)], dragged: &LayerRow, y: f32) -> Option<Result<Drop, String>> {
    let (rect, target) = rows.iter().find(|(r, _)| r.top() <= y && y < r.bottom())?;
    if dragged.mask {
        return Some(Err("a mask stays with its layer; drag the layer".into()));
    }
    if target.id == dragged.id {
        return None;
    }
    if target.path.starts_with(&dragged.path) {
        return Some(Err(format!("{} cannot go inside itself", dragged.name)));
    }
    if target.mask {
        return Some(Err("nothing goes beside a mask; drop on the layer it masks".into()));
    }
    let k = (y - rect.top()) / rect.height().max(1.0);
    Some(Ok(match (target.kind == Kind::Group, k) {
        (true, k) if (0.25..0.75).contains(&k) => Drop::Into(target.id.clone()),
        (_, k) if k < 0.5 => Drop::Above(target.id.clone()),
        _ => Drop::Below(target.id.clone()),
    }))
}

type Made = Vec<(String, ColorImage)>;

#[derive(Default)]
pub(super) struct Thumbs {
    made: BTreeMap<String, (u64, TextureHandle)>,
    wanted: BTreeMap<String, Raster>,
    making: Option<(u64, Receiver<Made>)>,
}

impl Editor {
    pub(super) fn layer_rows(&self, place: Where) -> Vec<LayerRow> {
        fn walk(
            ed: &Editor,
            place: Where,
            layers: &[Layer],
            path: &mut Vec<usize>,
            depth: usize,
            out: &mut Vec<LayerRow>,
        ) {
            layers.iter().enumerate().rev().for_each(|(i, layer)| {
                path.push(i);
                let folded = place == Where::Timeline && ed.timeline.folded.contains(&layer.id);
                out.push(ed.row_of(layer, path, depth, false, folded));
                if let Some(mask) = layer.mask.as_deref() {
                    path.push(MASK);
                    let shown =
                        place == Where::Panel || ed.timeline.masks.contains(&layer.id) || ed.scene.active == *path;
                    if shown {
                        out.push(ed.row_of(mask, path, depth + 1, true, false));
                    }
                    path.pop();
                }
                if !folded {
                    walk(ed, place, layer.children(), path, depth + 1, out);
                }
                path.pop();
            });
        }
        let mut out = Vec::new();
        walk(self, place, &self.scene.bundle.doc.layers, &mut Vec::new(), 0, &mut out);
        out
    }

    fn row_of(&self, layer: &Layer, path: &[usize], depth: usize, mask: bool, folded: bool) -> LayerRow {
        let cel = layer
            .content_at(self.scene.frame)
            .map(|c| c.link.clone().unwrap_or_else(|| c.id.clone()));
        LayerRow {
            path: path.to_vec(),
            id: layer.id.clone(),
            name: layer.name.clone(),
            kind: layer.kind(),
            depth,
            visible: layer.visible,
            locked: layer.locked,
            clip: layer.clip,
            lock_alpha: layer.lock_alpha,
            has_mask: layer.mask.is_some(),
            mask,
            folded,
            cel,
        }
    }

    fn thumb_key(&self) -> u64 {
        self.scene.bundle.doc.rev ^ ((self.scene.frame as u64) << 40)
    }

    pub(super) fn thumb(&mut self, _ctx: &egui::Context, cel: &str) -> Option<TextureHandle> {
        let key = self.thumb_key();
        let made = self.thumbs.made.get(cel).map(|(k, t)| (*k == key, t.clone()));
        if let Some((true, texture)) = made {
            return Some(texture);
        }
        if let Some(raster) = self.scene.raster_of(cel) {
            self.thumbs.wanted.insert(cel.to_string(), raster.clone());
        }
        made.map(|(_, texture)| texture)
    }

    pub(super) fn keep_thumbs(&mut self, images: Made) {
        let key = self.thumb_key();
        images.into_iter().for_each(|(cel, image)| {
            let texture = self
                .ctx
                .load_texture(format!("thumb {cel}"), image, TextureOptions::LINEAR);
            self.thumbs.made.insert(cel, (key, texture));
        });
    }

    pub(super) fn tend_thumbs(&mut self, ctx: &egui::Context) {
        let finished = self
            .thumbs
            .making
            .as_ref()
            .and_then(|(key, rx)| rx.try_recv().ok().map(|images| (*key, images)));
        if let Some((key, images)) = finished {
            self.thumbs.making = None;
            images.into_iter().for_each(|(cel, image)| {
                let texture = ctx.load_texture(format!("thumb {cel}"), image, TextureOptions::LINEAR);
                self.thumbs.made.insert(cel, (key, texture));
            });
        }
        if self.thumbs.making.is_some() || self.thumbs.wanted.is_empty() {
            return;
        }
        let wanted = std::mem::take(&mut self.thumbs.wanted);
        let size = self.scene.size();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let images: Vec<(String, ColorImage)> = crate::background().install(|| {
                wanted
                    .into_par_iter()
                    .map(|(cel, raster)| (cel, thumbnail(&raster, size, THUMB)))
                    .collect()
            });
            let _ = tx.send(images);
            ctx.request_repaint();
        });
        self.thumbs.making = Some((self.thumb_key(), rx));
    }

    fn small_toggle(ui: &mut egui::Ui, glyph: &str, on: bool, hint: String) -> bool {
        let colour = if on { theme::TEXT } else { theme::WEAK };
        ui.add(
            egui::Button::new(RichText::new(glyph).size(15.0).color(colour))
                .frame(false)
                .min_size(vec2(22.0, theme::TARGET)),
        )
        .hint(hint)
        .clicked()
    }

    pub(super) fn layer_row(&mut self, ui: &mut egui::Ui, row: &LayerRow, place: Where, rect: Rect) -> egui::Response {
        let id = ui.scope_id().with(("layer row", &row.id, place == Where::Panel));
        let response = ui.interact(rect, id, Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &row.name));
        let response = response.hint(format!(
            "{}: click to draw on it, cmd click to select strokes on it too (S), double-click its name to rename it, drag it to move it",
            row.name
        ));
        let active = self.scene.active == row.path;
        let also = self.also.contains(&row.id);
        let painter = ui.painter_at(rect);
        if active {
            painter.rect_filled(rect, 3.0, Color32::from_gray(54));
        } else if also {
            painter.rect_filled(rect, 3.0, Color32::from_gray(47));
        } else if response.hovered() {
            painter.rect_filled(rect, 3.0, Color32::from_gray(42));
        }
        if also {
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                1.0,
                theme::ACCENT,
            );
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(2.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 2.0;
        let mut ops: Vec<Op> = Vec::new();
        if !row.mask {
            let eye = if row.visible { icon::EYE } else { icon::EYE_OFF };
            if Self::small_toggle(&mut child, eye, row.visible, format!("show or hide {}", row.name)) {
                ops.push(set(&row.id, "visible", json!(!row.visible)));
            }
            if Self::small_toggle(
                &mut child,
                icon::LOCK,
                row.locked,
                format!("lock or unlock {}", row.name),
            ) {
                ops.push(set(&row.id, "locked", json!(!row.locked)));
            }
        } else {
            child.add_space(46.0);
        }
        child.add_space(row.depth as f32 * 12.0);
        if row.kind == Kind::Group && place == Where::Timeline {
            let caret = if row.folded { icon::FOLDED } else { icon::OPEN };
            if Self::small_toggle(&mut child, caret, true, format!("fold or open the group {}", row.name))
                && !self.timeline.folded.remove(&row.id)
            {
                self.timeline.folded.insert(row.id.clone());
            }
        }
        let kind = match (row.mask, row.kind) {
            (true, _) => icon::MASK,
            (_, Kind::Group) => icon::LAYERS,
            (_, Kind::Adjust) => icon::ADJUST,
            (_, Kind::Vector) => icon::POINTS,
            _ => icon::BRUSH,
        };
        child.label(RichText::new(kind).color(theme::WEAK));
        let rename = |text: String| Rename {
            id: row.id.clone(),
            text,
            place,
        };
        let renaming = self
            .renaming
            .as_ref()
            .is_some_and(|r| r.id == row.id && r.place == place);
        if renaming {
            let mut text = self.renaming.as_ref().map(|r| r.text.clone()).unwrap_or_default();
            let label = child.label("rename");
            let edit = child
                .add(egui::TextEdit::singleline(&mut text).desired_width(90.0))
                .labelled_by(label.id)
                .hint("the new name; Return keeps it, Esc leaves it");
            edit.request_focus();
            let (enter, escape) = child.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            match (enter || edit.lost_focus(), escape) {
                (_, true) => self.renaming = None,
                (true, _) => {
                    let name = text.trim().to_string();
                    if !name.is_empty() && name != row.name {
                        ops.push(set(&row.id, "name", json!(name)));
                    }
                    self.renaming = None;
                }
                _ => self.renaming = Some(rename(text)),
            }
        } else {
            let name = child.add(
                egui::Label::new(RichText::new(&row.name).color(if active { Color32::WHITE } else { theme::TEXT }))
                    .truncate()
                    .sense(Sense::click()),
            );
            if name.double_clicked() {
                self.renaming = Some(rename(row.name.clone()));
            }
            if name.clicked() {
                self.row_click(ui, row);
            }
        }
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if place == Where::Panel && row.path.len() == 1 && !row.mask {
                let depth = self.scene.bundle.doc.layer(&row.id).and_then(|l| l.depth).unwrap_or(0.0);
                if let Some(v) = self.number(
                    ui,
                    &format!("{}.row.depth", row.id),
                    "",
                    depth,
                    0.0..=100.0,
                    0.02,
                    &format!("{}'s depth: 0 is near; deeper planes take more fog and move less when you hold D", row.name),
                ) {
                    ops.push(set(&row.id, "depth", if v <= 0.0 { json!(null) } else { json!(v) }));
                }
            }
            if place == Where::Panel
                && let Some(cel) = row.cel.clone()
                && let Some(texture) = self.thumb(ui.ctx(), &cel)
            {
                ui.add(egui::Image::new(&texture).fit_to_exact_size(vec2(THUMB.0 as f32, THUMB.1 as f32)));
            }
            if place == Where::Timeline && matches!(row.kind, Kind::Paint | Kind::Vector) && !row.mask {
                let marked = self.scene.onion_marked.contains(&row.id);
                if Self::small_toggle(
                    ui,
                    icon::ONION,
                    marked,
                    format!("show the onion of {} too, not only the active layer's", row.name),
                ) {
                    if !self.scene.onion_marked.remove(&row.id) {
                        self.scene.onion_marked.insert(row.id.clone());
                    }
                    self.check(|e| e.onion_changed());
                }
            }
            if row.has_mask {
                let r = ui
                    .add(
                        egui::Button::new(RichText::new(icon::MASK).size(14.0))
                            .selected(self.scene.active.last() == Some(&MASK) && self.scene.active.starts_with(&row.path))
                            .min_size(vec2(22.0, theme::TARGET)),
                    )
                    .hint("the mask: click to paint on it, alt click turns it on or off, right-click shows its row in the timeline");
                if r.clicked() {
                    match ui.input(|i| i.modifiers.alt) {
                        true => {
                            let mask = self.scene.bundle.doc.layer_at(&row.path).mask.as_deref().map(|m| (m.id.clone(), m.visible));
                            if let Some((mask, on)) = mask {
                                ops.push(set(&mask, "visible", json!(!on)));
                            }
                        }
                        false => {
                            let mut path = row.path.clone();
                            path.push(MASK);
                            self.queued_activate = Some(path);
                        }
                    }
                }
                if r.secondary_clicked() && !self.timeline.masks.remove(&row.id) {
                    self.timeline.masks.insert(row.id.clone());
                }
            }
            [(row.clip, icon::CLIP, "clipped to the layer below"), (row.lock_alpha, icon::LOCK, "alpha locked")]
                .iter()
                .filter(|(on, _, _)| *on)
                .for_each(|(_, glyph, what)| {
                    ui.label(RichText::new(*glyph).size(12.0).color(theme::ACCENT)).on_hover_text(*what);
                });
        });
        if response.clicked() && !renaming {
            self.row_click(ui, row);
        }
        if response.double_clicked() && !row.mask {
            self.renaming = Some(rename(row.name.clone()));
        }
        if !ops.is_empty() {
            self.check(|e| e.apply(ops));
        }
        response
    }

    fn row_click(&mut self, ui: &egui::Ui, row: &LayerRow) {
        let adding = ui.input(|i| i.modifiers.command || i.modifiers.shift);
        if !adding || row.mask {
            self.also.clear();
            self.queued_activate = Some(row.path.clone());
            return;
        }
        if self.scene.active == row.path {
            return;
        }
        if row.kind != Kind::Paint {
            self.say(
                Tone::Refused,
                format!(
                    "{} is a {} layer: strokes to select are on paint layers",
                    row.name,
                    row.kind.name()
                ),
            );
            return;
        }
        match self.also.iter().position(|id| *id == row.id) {
            Some(i) => {
                self.also.remove(i);
            }
            None => self.also.push(row.id.clone()),
        }
        self.say(
            Tone::Info,
            match self.also.len() {
                0 => "strokes select on the active layer only".to_string(),
                n => format!("S selects strokes on the active layer and {n} more; drag them together"),
            },
        );
    }

    pub(super) fn layer_drags(
        &mut self,
        ui: &egui::Ui,
        place: Where,
        placed: &[(Rect, LayerRow)],
        responses: &[egui::Response],
    ) {
        if let Some(i) = responses.iter().position(|r| r.drag_started()) {
            self.layer_drag = Some((placed[i].1.clone(), place));
        }
        let Some(dragged) = self
            .layer_drag
            .as_ref()
            .filter(|(_, from)| *from == place)
            .map(|(row, _)| row.clone())
        else {
            return;
        };
        let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        let target = pointer.and_then(|p| drop_at(placed, &dragged, p.y));
        if let (Some(Ok(drop)), Some(p)) = (&target, pointer) {
            let painter = ui.painter();
            let row = placed
                .iter()
                .find(|(r, _)| r.top() <= p.y && p.y < r.bottom())
                .map(|(r, _)| *r);
            if let Some(r) = row {
                match drop {
                    Drop::Above(_) => {
                        painter.line_segment([r.left_top(), r.right_top()], egui::Stroke::new(2.5, theme::ACCENT));
                    }
                    Drop::Below(_) => {
                        painter.line_segment(
                            [r.left_bottom(), r.right_bottom()],
                            egui::Stroke::new(2.5, theme::ACCENT),
                        );
                    }
                    Drop::Into(_) => {
                        painter.rect_stroke(r, 3.0, egui::Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
                    }
                }
            }
        }
        if ui.input(|i| i.pointer.any_released()) {
            self.layer_drag = None;
            match target {
                Some(Ok(drop)) => {
                    let op = Op::Move {
                        id: dragged.id.clone(),
                        place: drop.place(),
                    };
                    self.check(|e| e.apply(vec![op]));
                }
                Some(Err(why)) => self.say(Tone::Refused, why),
                None => {}
            }
        }
    }

    pub(super) fn layer_header(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(layer) = self
            .scene
            .active_layer()
            .cloned()
            .filter(|_| !self.scene.active_is_mask())
        else {
            return Ok(());
        };
        let id = layer.id.clone();
        let mut props = Vec::new();
        let mut blend = layer.blend;
        egui::ComboBox::from_id_salt(format!("{id}.blend.panel"))
            .width(84.0)
            .selected_text(layer.blend.name())
            .show_ui(ui, |ui| {
                Blend::ALL.iter().for_each(|b| {
                    ui.selectable_value(&mut blend, *b, b.name());
                });
            })
            .response
            .hint("how the active layer mixes with what is below it");
        if blend != layer.blend {
            props.push(("blend".to_string(), json!(blend)));
        }
        if let Some(v) = self.number(
            ui,
            &format!("{id}.o"),
            "opacity ",
            layer.opacity,
            0.0..=1.0,
            0.01,
            "the active layer's opacity",
        ) {
            props.push(("opacity".to_string(), json!(v)));
        }
        self.set(&id, props)
    }

    pub(super) fn layers_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal_wrapped(|ui| {
            [
                (Cmd::NewPaintLayer, Some("paint")),
                (Cmd::NewVectorLayer, Some("vector")),
                (Cmd::NewGroupLayer, Some("group")),
                (Cmd::NewAdjustLayer, Some("adjust")),
                (Cmd::DuplicateLayer, None),
                (Cmd::GroupLayer, None),
                (Cmd::DeleteLayer, None),
            ]
            .into_iter()
            .for_each(|(cmd, label)| {
                self.command_button(ui, cmd, label);
            });
        });
        let rows = self.layer_rows(Where::Panel);
        let width = ui.available_width();
        let mut placed = Vec::new();
        let mut responses = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        rows.into_iter().for_each(|row| {
            let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
            responses.push(self.layer_row(ui, &row, Where::Panel, rect));
            placed.push((rect, row));
        });
        self.layer_drags(ui, Where::Panel, &placed, &responses);
        Ok(())
    }

    pub(super) fn duplicate_layer(&mut self) -> Result<()> {
        let Some(id) = self.scene.active_id().filter(|_| !self.scene.active_is_mask()) else {
            return Ok(());
        };
        let Some(applied) = self.timeline_apply(vec![Op::Duplicate { layer: id }])? else {
            return Ok(());
        };
        if let Some(Loc::Layer(path)) = applied.added.first().and_then(|id| self.scene.bundle.doc.locate(id)) {
            self.activate(path)?;
        }
        Ok(())
    }

    pub(super) fn delete_layer(&mut self) -> Result<()> {
        let Some(layer) = self.scene.active_layer().cloned() else {
            return Ok(());
        };
        let parent = self.scene.active[..self.scene.active.len() - 1].to_vec();
        self.apply(vec![Op::Remove {
            ids: vec![layer.id.clone()],
        }])?;
        let fallback = self
            .layer_rows(Where::Panel)
            .into_iter()
            .find(|r| !r.mask)
            .map(|r| r.path);
        if let Some(path) = (!parent.is_empty() && self.scene.bundle.doc.locate_path(&parent).is_some())
            .then_some(parent)
            .or(fallback)
        {
            self.activate(path)?;
        }
        self.say(Tone::Done, format!("deleted {} (cmd Z brings it back)", layer.name));
        Ok(())
    }

    pub(super) fn new_group(&mut self, wrap: bool) -> Result<()> {
        let active = self.scene.active_layer().cloned();
        let mut taken = self.scene.bundle.doc.ids();
        let group = (0..)
            .map(|n| format!("group{n}"))
            .find(|id| !taken.contains(id))
            .unwrap_or_default();
        taken.insert(group.clone());
        let place = match &active {
            Some(l) if !self.scene.active_is_mask() => Place {
                above: Some(l.id.clone()),
                ..Place::default()
            },
            _ => Place::default(),
        };
        let mut ops = vec![Op::AddLayer {
            layer: json!({"kind": "group", "id": group, "name": if wrap { active.as_ref().map_or("group".to_string(), |l| format!("{} group", l.name)) } else { "group".to_string() }}),
            place,
        }];
        if let Some(l) = active.filter(|_| wrap && !self.scene.active_is_mask()) {
            ops.push(Op::Move {
                id: l.id.clone(),
                place: Place {
                    into: Some(group.clone()),
                    ..Place::default()
                },
            });
        }
        self.apply(ops)
    }
}

fn set(id: &str, key: &str, value: serde_json::Value) -> Op {
    Op::Set {
        id: id.to_string(),
        props: Map::from_iter([(key.to_string(), value)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_brush::TILE;

    fn row_rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(egui::pos2(x, y), vec2(w, h))
    }

    fn row(id: &str, path: Vec<usize>, kind: Kind, mask: bool) -> LayerRow {
        LayerRow {
            path,
            id: id.into(),
            name: id.into(),
            kind,
            depth: 0,
            visible: true,
            locked: false,
            clip: false,
            lock_alpha: false,
            has_mask: false,
            mask,
            folded: false,
            cel: None,
        }
    }

    fn rows() -> Vec<(Rect, LayerRow)> {
        [
            row("top", vec![3], Kind::Paint, false),
            row("fx", vec![2], Kind::Group, false),
            row("inner", vec![2, 0], Kind::Paint, false),
            row("m", vec![1, MASK], Kind::Paint, true),
            row("base", vec![0], Kind::Paint, false),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, r)| (row_rect(0.0, i as f32 * 30.0, 200.0, 30.0), r))
        .collect()
    }

    #[test]
    fn a_dragged_row_lands_above_below_or_inside_a_group_or_says_why_not() {
        let rows = rows();
        let base = rows[4].1.clone();
        assert_eq!(drop_at(&rows, &base, 3.0), Some(Ok(Drop::Above("top".into()))));
        assert_eq!(drop_at(&rows, &base, 27.0), Some(Ok(Drop::Below("top".into()))));
        assert_eq!(drop_at(&rows, &base, 45.0), Some(Ok(Drop::Into("fx".into()))));
        assert_eq!(drop_at(&rows, &base, 31.0), Some(Ok(Drop::Above("fx".into()))));
        assert_eq!(drop_at(&rows, &base, 66.0), Some(Ok(Drop::Above("inner".into()))));
        assert!(matches!(drop_at(&rows, &base, 95.0), Some(Err(_))));
        assert_eq!(drop_at(&rows, &base, 125.0), None);
        assert_eq!(drop_at(&rows, &base, 500.0), None);
        let fx = rows[1].1.clone();
        assert!(matches!(drop_at(&rows, &fx, 70.0), Some(Err(why)) if why.contains("inside itself")));
        let mask = rows[3].1.clone();
        assert!(matches!(drop_at(&rows, &mask, 10.0), Some(Err(why)) if why.contains("mask")));
    }

    #[test]
    fn a_thumbnail_shows_where_the_layer_has_paint() {
        let mut raster = Raster::new(200, 100);
        (0..100).for_each(|y| {
            (0..100).for_each(|x| {
                let index = raster.index(x / TILE, y / TILE);
                let tile = raster.ensure(index);
                let i = ((y % TILE) * TILE + x % TILE) * 4;
                [1.0, 0.0, 0.0, 1.0]
                    .iter()
                    .enumerate()
                    .for_each(|(k, v)| tile[i + k] = half::f16::from_f32(*v));
            });
        });
        let image = thumbnail(&raster, (200, 100), (40, 20));
        assert_eq!(image.size, [40, 20]);
        let at = |x: usize, y: usize| image.pixels[y * 40 + x];
        assert_eq!(at(5, 10), Color32::from_rgb(255, 0, 0));
        assert_ne!(at(35, 10), Color32::from_rgb(255, 0, 0));
        assert_eq!(at(35, 10).r(), at(35, 10).g());
        let mut line = Raster::new(400, 200);
        (0..200).for_each(|y| {
            let index = line.index(200 / TILE, y / TILE);
            let tile = line.ensure(index);
            let i = ((y % TILE) * TILE + 200 % TILE) * 4;
            [0.0, 0.0, 0.0, 1.0]
                .iter()
                .enumerate()
                .for_each(|(k, v)| tile[i + k] = half::f16::from_f32(*v));
        });
        let thin = thumbnail(&line, (400, 200), (40, 20));
        assert!(thin.pixels[10 * 40 + 20].r() < 160, "{:?}", thin.pixels[10 * 40 + 20]);
    }
}
