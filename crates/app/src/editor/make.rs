use super::Editor;
use crate::color;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, Pos2, Rect, Stroke, Vec2, vec2};
use klad_core::doc::{Item, Kind, Loc, Node, Shape};
use klad_core::generators::Align;
use klad_core::ops::{Op, Place};
use klad_core::stroke::{POS_SCALE, Sample};
use klad_render::overlay;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ShapeKind {
    #[default]
    Rect,
    Ellipse,
}

#[derive(Default)]
pub(super) struct ShapeTool {
    pub kind: ShapeKind,
    pub radius: f32,
    pub drag: Option<(Vec2, Vec2)>,
}

pub(super) fn mirrored(s: &Sample, axis: f32) -> Sample {
    Sample {
        x: (2.0 * axis * POS_SCALE).round() as i32 - s.x,
        tilt_x: s.tilt_x.saturating_neg(),
        ..*s
    }
}

fn bounds(key: &str) -> (std::ops::RangeInclusive<f32>, f64) {
    match key {
        "count" => (1.0..=10_000.0, 0.1),
        "columns" => (0.0..=1000.0, 0.1),
        "seed" => (0.0..=99_999.0, 0.2),
        "scale" => (0.01..=10.0, 0.01),
        "size0" | "size1" => (0.01..=100.0, 0.01),
        "turn" | "turn0" | "turn1" | "angle" => (-360.0..=360.0, 0.5),
        "w" | "h" => (1.0..=100_000.0, 1.0),
        "size" => (1.0..=4096.0, 0.5),
        "leading" => (0.1..=10.0, 0.01),
        "tracking" => (-200.0..=1000.0, 0.2),
        _ => (-100_000.0..=100_000.0, 0.5),
    }
}

pub(super) fn shape_box((a, b): (Vec2, Vec2), square: bool, centred: bool) -> [f32; 4] {
    let d = b - a;
    let d = match square {
        true => {
            let m = d.x.abs().max(d.y.abs());
            vec2(m * d.x.signum(), m * d.y.signum())
        }
        false => d,
    };
    let (x0, y0, x1, y1) = match centred {
        true => (a.x - d.x, a.y - d.y, a.x + d.x, a.y + d.y),
        false => (a.x, a.y, a.x + d.x, a.y + d.y),
    };
    [x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs()]
}

fn on_vector(editor: &Editor) -> Option<String> {
    editor
        .scene
        .active_layer()
        .filter(|l| l.kind() == Kind::Vector && !l.locked && !editor.scene.active_is_mask())
        .map(|l| l.id.clone())
}

impl Editor {
    pub(super) fn shape_now(&self) -> Option<[f32; 4]> {
        self.shapes
            .drag
            .map(|d| shape_box(d, self.modifiers.shift, self.modifiers.alt))
    }

    pub(super) fn shape_down(&mut self, c: Vec2) {
        if on_vector(self).is_none() {
            self.say(
                Tone::Refused,
                "U draws rectangles and ellipses on a vector layer: add one with the layers panel's vector button",
            );
            return;
        }
        self.shapes.drag = Some((c, c));
    }

    pub(super) fn shape_move(&mut self, c: Vec2) {
        if let Some((_, b)) = self.shapes.drag.as_mut() {
            *b = c;
        }
    }

    pub(super) fn shape_up(&mut self) -> Result<()> {
        let made = self.shape_now();
        self.shapes.drag = None;
        let (Some([x, y, w, h]), Some(layer)) = (made, on_vector(self)) else {
            return Ok(());
        };
        if w < 2.0 || h < 2.0 {
            return Ok(());
        }
        let (colour, outline) = (color::hex(self.color), color::hex(self.second));
        let (mut node, what) = match self.shapes.kind {
            ShapeKind::Rect => (
                json!({"type": "rect", "x": x, "y": y, "w": w, "h": h, "radius": self.shapes.radius.min(w / 2.0).min(h / 2.0)}),
                "rectangle",
            ),
            ShapeKind::Ellipse => (
                json!({"type": "ellipse", "cx": x + w / 2.0, "cy": y + h / 2.0, "rx": w / 2.0, "ry": h / 2.0}),
                "ellipse",
            ),
        };
        node["fill"] = match self.pen_fill {
            true => json!(colour),
            false => json!("none"),
        };
        if self.pen_stroke || !self.pen_fill {
            node["stroke"] = json!({ "paint": outline, "width": self.pen_width });
        }
        let frame = self.scene.frame;
        self.apply(vec![Op::AddNode {
            node,
            place: Place {
                into: Some(layer),
                frame: Some(frame),
                ..Place::default()
            },
        }])?;
        self.used_colour();
        self.say(Tone::Done, format!("{what} added: V moves it, A edits its points"));
        Ok(())
    }

    pub(super) fn decorate_shape(
        &self,
        painter: &egui::Painter,
        to_screen: &dyn Fn(f32, f32) -> Pos2,
        accent: Color32,
    ) {
        let Some([x, y, w, h]) = self.shape_now() else {
            return;
        };
        let line = Stroke::new(1.5, accent);
        match self.shapes.kind {
            ShapeKind::Rect => {
                let r = Rect::from_two_pos(to_screen(x, y), to_screen(x + w, y + h));
                painter.rect_stroke(r, 0.0, line, egui::StrokeKind::Middle);
            }
            ShapeKind::Ellipse => {
                let points: Vec<Pos2> = (0..=64)
                    .map(|i| i as f32 / 64.0 * std::f32::consts::TAU)
                    .map(|a| to_screen(x + w / 2.0 + a.cos() * w / 2.0, y + h / 2.0 + a.sin() * h / 2.0))
                    .collect();
                painter.add(egui::Shape::line(points, line));
            }
        }
    }

    pub(super) fn wrap(&mut self, kind: &str) -> Result<()> {
        let Some(id) = self.selection.clone() else {
            self.say(Tone::Refused, "select a shape with V first");
            return Ok(());
        };
        let doc = &self.scene.bundle.doc;
        let (Some(Item::Node(_, cel, node)), Some(Loc::Node(_, _, path))) = (doc.item(&id), doc.locate(&id)) else {
            self.say(Tone::Refused, "select a shape with V first");
            return Ok(());
        };
        let Some([x, y, w, h]) = overlay::node_world_box(doc, &id)? else {
            self.say(Tone::Refused, format!("{id} draws nothing to {kind}"));
            return Ok(());
        };
        let parent = path[..path.len() - 1]
            .iter()
            .fold((cel.id.clone(), &cel.nodes[..]), |(_, list), i| {
                (list[*i].id.clone(), list[*i].children())
            })
            .0;
        let index = path.last().copied();
        let mut child = serde_json::to_value(node)?;
        let taken = doc.ids();
        let made = (1..)
            .map(|n| format!("{kind}{n}"))
            .find(|i| !taken.contains(i))
            .unwrap_or_default();
        let generator = match kind {
            "repeat" => json!({"id": made, "type": "repeat", "count": 4, "step": [w + 12.0, 0.0], "children": [child]}),
            "mirror" => json!({"id": made, "type": "mirror", "x": x + w, "y": y, "angle": 90, "children": [child]}),
            _ => {
                let t = &node.transform;
                child["transform"]["x"] = json!(t.x - (x + w / 2.0));
                child["transform"]["y"] = json!(t.y - (y + h / 2.0));
                json!({"id": made, "type": "scatter", "x": x - w * 2.0, "y": y - h, "w": w * 5.0, "h": h * 3.0, "count": 12, "children": [child]})
            }
        };
        let frame = self.scene.frame;
        self.apply(vec![
            Op::Remove { ids: vec![id.clone()] },
            Op::AddNode {
                node: generator,
                place: Place {
                    into: Some(parent),
                    index,
                    frame: Some(frame),
                    ..Place::default()
                },
            },
        ])?;
        self.selection = Some(made);
        self.say(
            Tone::Done,
            format!("{id} is now inside a {kind}: the inspector sets how many copies and where they go"),
        );
        Ok(())
    }

    pub(super) fn unwrap(&mut self) -> Result<()> {
        let Some(id) = self.selection.clone() else {
            return Ok(());
        };
        let doc = &self.scene.bundle.doc;
        let (Some(Item::Node(_, cel, node)), Some(Loc::Node(_, _, path))) = (doc.item(&id), doc.locate(&id)) else {
            return Ok(());
        };
        if !node.shape.generates() {
            return Ok(());
        }
        let parent = path[..path.len() - 1]
            .iter()
            .fold((cel.id.clone(), &cel.nodes[..]), |(_, list), i| {
                (list[*i].id.clone(), list[*i].children())
            })
            .0;
        let index = path.last().copied().unwrap_or(0);
        let shift = match &node.shape {
            Shape::Scatter(s) => Some((s.x + s.w / 2.0, s.y + s.h / 2.0)),
            _ => None,
        };
        let children: Vec<Value> = node
            .children()
            .iter()
            .map(|child| {
                let mut value = serde_json::to_value(child)?;
                if let Some((dx, dy)) = shift {
                    value["transform"]["x"] = json!(child.transform.x + dx);
                    value["transform"]["y"] = json!(child.transform.y + dy);
                }
                Ok(value)
            })
            .collect::<Result<_>>()?;
        let first = node.children().first().map(|c| c.id.clone());
        let kind = node.type_name();
        let frame = self.scene.frame;
        let ops = std::iter::once(Op::Remove { ids: vec![id.clone()] })
            .chain(children.into_iter().enumerate().map(|(k, value)| Op::AddNode {
                node: value,
                place: Place {
                    into: Some(parent.clone()),
                    index: Some(index + k),
                    frame: Some(frame),
                    ..Place::default()
                },
            }))
            .collect();
        self.apply(ops)?;
        self.selection = first;
        self.say(Tone::Done, format!("the {kind} is gone; its shapes stay"));
        Ok(())
    }

    fn fonts(&self) -> Vec<String> {
        let dir = self.scene.bundle.root.join(klad_render::assets::FONTS);
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn field(&mut self, ui: &mut egui::Ui, id: &str, key: &str, label: &str, value: f32, hint: &str) -> Option<f32> {
        let (range, speed) = bounds(key);
        self.number(ui, &format!("{id}.{key}.{label}"), label, value, range, speed, hint)
    }

    pub(super) fn shape_fields(&mut self, ui: &mut egui::Ui, id: &str, node: &Node) -> Vec<(String, Value)> {
        let mut out: Vec<(String, Value)> = Vec::new();
        let mut put = |key: &str, value: Value| out.push((key.to_string(), value));
        match &node.shape {
            Shape::Scatter(s) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("scatter").strong().color(theme::TEXT));
                    self.command_button(ui, crate::commands::Cmd::Unwrap, Some("take the shapes out"));
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(v) = self.field(ui, id, "count", "copies ", s.count as f32, "how many copies") {
                        put("count", json!(v.round().max(0.0) as u32));
                    }
                    if let Some(v) = self.field(
                        ui,
                        id,
                        "seed",
                        "seed ",
                        s.seed as f32,
                        "another seed places them differently",
                    ) {
                        put("seed", json!(v.round().max(0.0) as u32));
                    }
                    [
                        ("x", "x ", s.x),
                        ("y", "y ", s.y),
                        ("w", "width ", s.w),
                        ("h", "height ", s.h),
                    ]
                    .iter()
                    .for_each(|(key, label, v)| {
                        if let Some(n) =
                            self.field(ui, id, key, label, *v, "the region the copies land in, in canvas px")
                        {
                            put(key, json!(n));
                        }
                    });
                    let size = (
                        self.field(ui, id, "size0", "smallest ", s.size[0], "the smallest copy, as a scale"),
                        self.field(ui, id, "size1", "largest ", s.size[1], "the largest copy, as a scale"),
                    );
                    if size.0.is_some() || size.1.is_some() {
                        put(
                            "size",
                            json!([size.0.unwrap_or(s.size[0]), size.1.unwrap_or(s.size[1])]),
                        );
                    }
                    let turn = (
                        self.field(ui, id, "turn0", "turn from ", s.turn[0], "the least turn, in degrees"),
                        self.field(ui, id, "turn1", "to ", s.turn[1], "the most turn, in degrees"),
                    );
                    if turn.0.is_some() || turn.1.is_some() {
                        put(
                            "turn",
                            json!([turn.0.unwrap_or(s.turn[0]), turn.1.unwrap_or(s.turn[1])]),
                        );
                    }
                });
            }
            Shape::Repeat(r) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("repeat").strong().color(theme::TEXT));
                    self.command_button(ui, crate::commands::Cmd::Unwrap, Some("take the shapes out"));
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(v) = self.field(
                        ui,
                        id,
                        "count",
                        "copies ",
                        r.count as f32,
                        "how many copies, the first included",
                    ) {
                        put("count", json!(v.round().max(0.0) as u32));
                    }
                    let step = (
                        self.field(ui, id, "step0", "step x ", r.step[0], "how far each copy moves across"),
                        self.field(ui, id, "step1", "step y ", r.step[1], "how far each copy moves down"),
                    );
                    if step.0.is_some() || step.1.is_some() {
                        put(
                            "step",
                            json!([step.0.unwrap_or(r.step[0]), step.1.unwrap_or(r.step[1])]),
                        );
                    }
                    if let Some(v) = self.field(
                        ui,
                        id,
                        "turn",
                        "turn ",
                        r.turn,
                        "each copy turns this much more, in degrees, about the pivot (an arc)",
                    ) {
                        put("turn", json!(v));
                    }
                    let pivot = (
                        self.field(ui, id, "pivot0", "pivot x ", r.pivot[0], "the point copies turn about"),
                        self.field(ui, id, "pivot1", "pivot y ", r.pivot[1], "the point copies turn about"),
                    );
                    if pivot.0.is_some() || pivot.1.is_some() {
                        put(
                            "pivot",
                            json!([pivot.0.unwrap_or(r.pivot[0]), pivot.1.unwrap_or(r.pivot[1])]),
                        );
                    }
                    if let Some(v) = self.field(
                        ui,
                        id,
                        "scale",
                        "scale ",
                        r.scale,
                        "each copy is this much bigger (1 keeps the size)",
                    ) {
                        put("scale", json!(v.max(0.01)));
                    }
                    if let Some(v) = self.field(
                        ui,
                        id,
                        "columns",
                        "columns ",
                        r.columns as f32,
                        "0 makes a line; more makes a grid this many wide",
                    ) {
                        put("columns", json!(v.round().max(0.0) as u32));
                    }
                    let row = (
                        self.field(ui, id, "row0", "row x ", r.row[0], "how far each grid row moves across"),
                        self.field(ui, id, "row1", "row y ", r.row[1], "how far each grid row moves down"),
                    );
                    if row.0.is_some() || row.1.is_some() {
                        put("row", json!([row.0.unwrap_or(r.row[0]), row.1.unwrap_or(r.row[1])]));
                    }
                });
            }
            Shape::Mirror(m) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("mirror").strong().color(theme::TEXT));
                    self.command_button(ui, crate::commands::Cmd::Unwrap, Some("take the shapes out"));
                });
                ui.horizontal_wrapped(|ui| {
                    [
                        ("x", "axis x ", m.x),
                        ("y", "axis y ", m.y),
                        ("angle", "angle ", m.angle),
                    ]
                    .iter()
                    .for_each(|(key, label, v)| {
                        if let Some(n) = self.field(
                            ui,
                            id,
                            key,
                            label,
                            *v,
                            "the mirror's axis: a point it passes through and its angle (90 is upright)",
                        ) {
                            put(key, json!(n));
                        }
                    });
                });
            }
            Shape::Text(t) => {
                ui.label(egui::RichText::new("text").strong().color(theme::TEXT));
                let key = format!("{id}.text");
                let mut text = self.typing.get(&key).cloned().unwrap_or_else(|| t.text.clone());
                let edit = ui
                    .add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_rows(2)
                            .desired_width(220.0),
                    )
                    .hint("the words; a new line starts a new line of text");
                if edit.has_focus() {
                    self.typing.insert(key.clone(), text.clone());
                } else if let Some(typed) = self.typing.remove(&key)
                    && typed != t.text
                {
                    put("text", json!(typed));
                }
                let fonts = self.fonts();
                let mut font = t.font.clone();
                egui::ComboBox::from_id_salt(format!("{id}.font"))
                    .width(160.0)
                    .selected_text(&font)
                    .show_ui(ui, |ui| {
                        fonts.iter().for_each(|f| {
                            ui.selectable_value(&mut font, f.clone(), f);
                        });
                    })
                    .response
                    .hint("the font file, from the bundle's fonts folder (klad font adds one)");
                if font != t.font {
                    put("font", json!(font));
                }
                ui.horizontal_wrapped(|ui| {
                    [("size", "size ", t.size), ("tracking", "tracking ", t.tracking), ("leading", "line height ", t.leading), ("x", "x ", t.x), ("y", "y ", t.y)]
                        .iter()
                        .for_each(|(key, label, v)| {
                            if let Some(n) = self.field(ui, id, key, label, *v, "size in px, tracking in px between letters, line height as a multiple of the size, x and y the anchor") {
                                put(key, json!(n));
                            }
                        });
                    [(Align::Left, "left"), (Align::Center, "centre"), (Align::Right, "right")]
                        .iter()
                        .for_each(|(align, label)| {
                            if ui
                                .selectable_label(t.align == *align, *label)
                                .hint("which end of each line sits at x")
                                .clicked()
                                && t.align != *align
                            {
                                put("align", json!(align));
                            }
                        });
                });
            }
            Shape::Image(p) => {
                ui.label(format!("picture {}", p.src));
                ui.horizontal_wrapped(|ui| {
                    [
                        ("x", "x ", p.x),
                        ("y", "y ", p.y),
                        ("w", "width ", p.w),
                        ("h", "height ", p.h),
                    ]
                    .iter()
                    .for_each(|(key, label, v)| {
                        if let Some(n) = self.field(
                            ui,
                            id,
                            key,
                            label,
                            *v,
                            "where the picture sits and how big, in canvas px",
                        ) {
                            put(key, json!(n));
                        }
                    });
                });
            }
            _ => {}
        }
        if !node.shape.generates() && !matches!(node.shape, Shape::Group { .. }) {
            ui.horizontal_wrapped(|ui| {
                self.command_button(ui, crate::commands::Cmd::MakeRepeat, Some("repeat it"));
                self.command_button(ui, crate::commands::Cmd::MakeScatter, Some("scatter it"));
                self.command_button(ui, crate::commands::Cmd::MakeMirror, Some("mirror it"));
            });
        }
        out
    }

    pub(super) fn import_picture(&mut self, bytes: &[u8], name: &str) -> Result<()> {
        let (src, w, h) = match klad_render::assets::import_image(&self.scene.bundle.root, bytes) {
            Ok(made) => made,
            Err(e) => {
                self.say(Tone::Refused, format!("{name}: {e:#}"));
                return Ok(());
            }
        };
        let (cw, ch) = self.scene.size();
        let fit = (cw as f32 / w as f32).min(ch as f32 / h as f32).min(1.0);
        let (pw, ph) = (w as f32 * fit, h as f32 * fit);
        let node = json!({"type": "image", "src": src, "x": (cw as f32 - pw) / 2.0, "y": (ch as f32 - ph) / 2.0, "w": pw, "h": ph});
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("picture")
            .to_string();
        let frame = self.scene.frame;
        self.add_layer_value(json!({"kind": "vector", "name": stem, "cels": [{"frame": frame, "nodes": [node]}]}))?;
        self.say(
            Tone::Done,
            format!(
                "{name} placed on a new layer: V moves and scales it, the inspector fades it or makes it a reference"
            ),
        );
        Ok(())
    }

    pub(super) fn import_window(&mut self, ctx: &egui::Context) {
        if !self.windows.import {
            return;
        }
        let mut open = true;
        egui::Window::new("import a picture")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let label = ui.label("PNG or JPEG file");
                ui.add(egui::TextEdit::singleline(&mut self.import_path).desired_width(260.0))
                    .labelled_by(label.id)
                    .hint("the picture's path; dropping the file on the window works too");
                if ui
                    .button("import")
                    .hint("copy the picture into the drawing and place it on a new layer")
                    .clicked()
                {
                    let path = self.import_path.trim().to_string();
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            self.check(|e| e.import_picture(&bytes, &path));
                            self.windows.import = false;
                        }
                        Err(e) => self.say(Tone::Refused, format!("reading {path}: {e}")),
                    }
                }
                ui.small("or drop a PNG or JPEG on the canvas");
            });
        if !open {
            self.windows.import = false;
        }
    }

    pub(super) fn dropped_pictures(&mut self, ctx: &egui::Context) {
        if self.windows.brush {
            return;
        }
        let files: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| ["png", "jpg", "jpeg"].contains(&e.to_ascii_lowercase().as_str()))
                })
                .collect()
        });
        files.into_iter().for_each(|path| match std::fs::read(&path) {
            Ok(bytes) => self.check(|e| e.import_picture(&bytes, &path.display().to_string())),
            Err(e) => self.say(Tone::Refused, format!("reading {}: {e}", path.display())),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_seeds_and_sizes_cannot_go_below_their_floor() {
        [
            ("count", 1.0),
            ("columns", 0.0),
            ("seed", 0.0),
            ("scale", 0.01),
            ("size0", 0.01),
            ("w", 1.0),
            ("size", 1.0),
        ]
        .iter()
        .for_each(|(key, floor)| assert_eq!(*bounds(key).0.start(), *floor, "{key}"));
        assert!(*bounds("x").0.start() < 0.0);
    }

    #[test]
    fn shift_squares_and_alt_draws_from_the_centre() {
        let drag = (vec2(10.0, 10.0), vec2(40.0, 20.0));
        assert_eq!(shape_box(drag, false, false), [10.0, 10.0, 30.0, 10.0]);
        assert_eq!(shape_box(drag, true, false), [10.0, 10.0, 30.0, 30.0]);
        assert_eq!(shape_box(drag, false, true), [-20.0, 0.0, 60.0, 20.0]);
        assert_eq!(
            shape_box((vec2(10.0, 10.0), vec2(0.0, 5.0)), true, false),
            [0.0, 0.0, 10.0, 10.0]
        );
        let s = Sample::quantize(30.0, 7.0, 0.5, (0.3, 0.1), 4.0);
        let m = mirrored(&s, 100.0);
        assert_eq!(m.pos(), (170.0, 7.0));
        assert_eq!(mirrored(&m, 100.0), s);
    }
}
