use super::{Editor, Tool};
use crate::color;
use crate::commands::Cmd;
use crate::theme::{self, Hint, icon};
use anyhow::{Result, anyhow};
use egui::{Color32, ColorImage, RichText, Sense, TextureHandle, TextureOptions, vec2};
use klad_brush::{BrushSet, Raster, TILE, replay};
use klad_core::brushes::BrushDef;
use klad_core::ops::{Op, PaletteEdit, parse_lines};
use klad_core::rng::{FNV_OFFSET, fnv};
use klad_core::stroke::Mark;
use std::collections::BTreeMap;

pub const SAMPLE: (u32, u32) = (132, 30);
pub const SIZES: [f32; 8] = [2.0, 4.0, 8.0, 12.0, 20.0, 32.0, 64.0, 128.0];

pub fn size_from_drag(start: f32, dx: f32) -> f32 {
    (start * 2f32.powf(dx / 120.0)).clamp(0.5, 800.0)
}

pub fn moved<T: Clone>(items: &[T], from: usize, to: usize) -> Vec<T> {
    let mut out = items.to_vec();
    if from >= out.len() {
        return out;
    }
    let item = out.remove(from);
    out.insert(to.min(out.len()), item);
    out
}

pub fn sample(brushes: &BrushSet, id: &str) -> Result<Raster> {
    sample_sized(brushes, id, SAMPLE, 14.0)
}

pub fn sample_sized(brushes: &BrushSet, id: &str, (w, h): (u32, u32), largest: f32) -> Result<Raster> {
    let def = brushes.def(id).ok_or_else(|| anyhow!("no brush {id}"))?;
    let size = (h as f32 * 0.42).min(largest);
    let second = match def.color.is_some_and(|c| c.uses_second()) {
        true => r##","second":"#6a8cff""##,
        false => "",
    };
    let bands = match def.paint.is_some() {
        true => format!(
            r##"{{"op":"stroke","layer":"sample","brush":"ink","size":{s},"color":"#c0603c","path":"M{x0} 0 L{x0} {h}","seed":1}}
{{"op":"stroke","layer":"sample","brush":"ink","size":{s},"color":"#3c68c0","path":"M{x1} 0 L{x1} {h}","seed":2}}
"##,
            s = w as f32 * 0.18,
            x0 = w as f32 * 0.35,
            x1 = w as f32 * 0.65,
        ),
        false => String::new(),
    };
    let line = format!(
        r##"{bands}{{"op":"stroke","layer":"sample","brush":"{id}","size":{size},"color":"#e6e6e6","path":"M6 {a} C {b} {c} {d} {e} {f} {g}","pressure":[0.15,0.9,1,0.55,0.2],"taper":[{t0},{t1}],"seed":7{second}}}"##,
        a = h as f32 * 0.7,
        b = w as f32 * 0.3,
        c = h as f32 * 0.05,
        d = w as f32 * 0.6,
        e = h as f32 * 1.05,
        f = w as f32 - 6.0,
        g = h as f32 * 0.3,
        t0 = def.taper[0].min(30.0),
        t1 = def.taper[1].min(40.0),
    );
    let marks: Vec<Mark> = parse_lines(&line)?
        .into_iter()
        .map(|op| match op {
            Op::Stroke(spec) => spec.build(),
            _ => Err(anyhow!("the sample stroke did not parse")),
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, mut stroke)| {
            stroke.id = i as u32 + 1;
            Mark::Stroke(stroke)
        })
        .collect();
    Ok(replay(w, h, &marks, brushes)?.raster)
}

pub fn image_of(raster: &Raster, (w, h): (u32, u32), background: Color32) -> ColorImage {
    let bg = background.to_array().map(|c| c as f32 / 255.0);
    let pixels = (0..h as usize)
        .flat_map(|y| (0..w as usize).map(move |x| (x, y)))
        .map(|(x, y)| {
            let rgba = raster.get(x / TILE, y / TILE).map_or([0.0; 4], |t| {
                let i = ((y % TILE) * TILE + x % TILE) * 4;
                [t[i].to_f32(), t[i + 1].to_f32(), t[i + 2].to_f32(), t[i + 3].to_f32()]
            });
            let [r, g, b] =
                [0, 1, 2].map(|k| ((rgba[k] + bg[k] * (1.0 - rgba[3])).clamp(0.0, 1.0) * 255.0).round() as u8);
            Color32::from_rgb(r, g, b)
        })
        .collect();
    ColorImage {
        size: [w as usize, h as usize],
        source_size: vec2(w as f32, h as f32),
        pixels,
    }
}

fn def_key(id: &str, def: &BrushDef) -> u64 {
    fnv(
        FNV_OFFSET,
        format!("{id}{}", serde_json::to_string(def).unwrap_or_default()).as_bytes(),
    )
}

#[derive(Default)]
pub(super) struct Samples {
    textures: BTreeMap<String, (u64, Option<TextureHandle>)>,
}

impl Samples {
    fn texture(&mut self, ctx: &egui::Context, brushes: &BrushSet, id: &str, def: &BrushDef) -> Option<TextureHandle> {
        let key = def_key(id, def);
        if let Some((k, texture)) = self.textures.get(id).filter(|(k, _)| *k == key) {
            let _ = k;
            return texture.clone();
        }
        let texture = sample(brushes, id)
            .ok()
            .map(|raster| image_of(&raster, SAMPLE, theme::PANEL))
            .map(|image| ctx.load_texture(format!("brush sample {id}"), image, TextureOptions::LINEAR));
        self.textures.insert(id.to_string(), (key, texture.clone()));
        texture
    }
}

fn label_of(id: &str, def: &BrushDef) -> String {
    match id.rsplit_once('-') {
        Some((base, tail)) if tail.len() == 6 && tail.chars().all(|c| c.is_ascii_hexdigit()) => {
            format!("{base} variant {tail}")
        }
        _ => def.name.clone(),
    }
}

impl Editor {
    pub(super) fn pick_new_colour(&mut self, rgba: [f32; 4]) {
        if rgba != self.color {
            self.second = self.color;
            self.set_colour(rgba);
        }
    }

    pub(super) fn swap_colours(&mut self) {
        let (a, b) = (self.color, self.second);
        self.second = a;
        self.set_colour(b);
    }

    pub(super) fn colour_pair(&mut self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
        let back = egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(24.0, 24.0));
        let front = egui::Rect::from_min_size(rect.min, vec2(24.0, 24.0));
        let swap = ui
            .interact(back, ui.scope_id().with("second colour"), Sense::click())
            .hint(Cmd::SwapColours.spec().hint());
        swap.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "the second colour"));
        let current = ui
            .interact(front, ui.scope_id().with("current colour"), Sense::click())
            .hint("the current colour; the one behind it is the second, X swaps them");
        current.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "the current colour"));
        let painter = ui.painter();
        [(back, self.second), (front, self.color)].iter().for_each(|(r, c)| {
            painter.rect_filled(*r, 3.0, Color32::from_gray(200));
            painter.rect_filled(*r, 3.0, color::color32(*c));
            painter.rect_stroke(
                *r,
                3.0,
                egui::Stroke::new(1.5, Color32::from_gray(20)),
                egui::StrokeKind::Outside,
            );
        });
        if swap.clicked() || current.clicked() {
            self.queued.push(Cmd::SwapColours);
        }
    }

    fn brush_main(&mut self, ui: &mut egui::Ui) {
        let current = self.brush.clone();
        let choices: Vec<(String, String)> = self
            .brushes
            .defs()
            .map(|(id, d)| (id.to_string(), label_of(id, d)))
            .collect();
        let shown = choices
            .iter()
            .find(|(id, _)| *id == current)
            .map_or(current.clone(), |(_, l)| l.clone());
        let mut picked = None;
        egui::ComboBox::from_id_salt("options.brush")
            .selected_text(shown)
            .width(150.0)
            .show_ui(ui, |ui| {
                choices.iter().for_each(|(id, text)| {
                    if ui.selectable_label(*id == current, text).clicked() {
                        picked = Some(id.clone());
                    }
                });
            })
            .response
            .hint("the brush; the brush panel shows them with a sample stroke");
        if let Some(id) = picked {
            self.pick(&id);
        }
        ui.add(
            egui::Slider::new(&mut self.size, 0.5..=800.0)
                .logarithmic(true)
                .text("size"),
        )
        .hint("brush size in canvas px ([ and ], or hold cmd alt and drag sideways on the canvas)");
        if let Some(mut feather) = self.eraser_feather()
            && ui
                .add(egui::Slider::new(&mut feather, 0.0..=1.0).text("feather"))
                .hint("how far in the eraser's edge fades: 0 cuts clean, 1 fades out like an airbrush; the pen's eraser end too")
                .changed()
        {
            self.set_eraser_feather(feather);
        }
        ui.add(egui::Slider::new(&mut self.opacity, 0.05..=1.0).text("opacity"))
            .hint("a stroke never gets darker than this on its own");
        ui.add(egui::Slider::new(&mut self.flow, 0.02..=1.0).text("flow"))
            .hint("how much each dab lays down");
    }

    fn brush_more(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let current = self.brush.clone();
        let erasing = self.is_on(Cmd::Eraser) == Some(true);
        let t = &mut self.tuning;
        if let Some(hardness) = t.hardness.as_mut().filter(|_| !erasing) {
            ui.add(egui::Slider::new(hardness, 0.0..=1.0).text("hardness"))
                .hint("soft to hard edge; changing it draws as a variant of the brush");
            ui.add(egui::Slider::new(&mut t.feather, 0.0..=1.0).text("feather"))
                .hint("how far in the edge fades out like an airbrush, without dabs building it back up");
        }
        ui.add(egui::Slider::new(&mut t.size_min, 0.0..=1.0).text("lightest size"))
            .hint("the size at the lightest press, as a part of the full size");
        ui.add(
            egui::Slider::new(&mut t.size_gamma, 0.3..=3.0)
                .logarithmic(true)
                .text("pressure curve"),
        )
        .hint("below 1 light presses get wider sooner, above 1 later");
        ui.add(egui::Slider::new(&mut t.taper[0], 0.0..=200.0).text("taper in px"))
            .hint("how long the start of a stroke takes to reach full size");
        ui.add(egui::Slider::new(&mut t.taper[1], 0.0..=200.0).text("taper out px"))
            .hint("how long the end of a stroke takes to come to a point");
        ui.add(egui::Slider::new(&mut t.stabilizer, 0.0..=200.0).text("stabiliser ms"))
            .hint("the line follows the average of the pen's last this many ms; it catches up when you lift");
        ui.add(egui::Slider::new(&mut t.smoothing, 0.0..=100.0).text("pressure smoothing ms"))
            .hint("evens out pressure over this many ms");
        if ui
            .button("reset brush")
            .hint("the brush's own settings again")
            .clicked()
        {
            self.pick(&current);
        }
        let (id, _) = self.shaped()?;
        if id != self.brush {
            ui.small(format!(
                "hardness, feather or curve changed: draws as {id}, kept in the bundle"
            ));
        }
        Ok(())
    }

    fn vector_colours(&mut self, ui: &mut egui::Ui, fill: &str, outline: &str) {
        [(true, self.color, fill), (false, self.second, outline)]
            .into_iter()
            .for_each(|(first, rgba, hint)| {
                let label = if first { "fill" } else { "outline" };
                let on = if first {
                    &mut self.pen_fill
                } else {
                    &mut self.pen_stroke
                };
                ui.checkbox(on, label).hint(hint);
                if color::swatch(ui, rgba, false, theme::TARGET)
                    .hint(format!(
                        "the {label} colour, {}; click (or X) swaps the two colours",
                        color::hex(rgba)
                    ))
                    .clicked()
                {
                    self.queued.push(Cmd::SwapColours);
                }
            });
    }

    pub(super) fn tool_options(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal_wrapped(|ui| {
            let (cmd, name) = match self.tool {
                Tool::Brush if self.is_on(Cmd::Eraser) == Some(true) => (Cmd::Eraser, "eraser"),
                Tool::Brush => (Cmd::Brush, "brush"),
                Tool::Fill => (Cmd::Fill, "fill"),
                Tool::Strokes => (Cmd::Strokes, "select strokes"),
                Tool::Select => (Cmd::Shapes, "select and move"),
                Tool::Direct => (Cmd::Points, "edit points"),
                Tool::Pen => (Cmd::Pen, "pen"),
                Tool::Shape => (Cmd::ShapeTool, "rectangles and ellipses"),
                Tool::Text => (Cmd::TextTool, "text"),
                Tool::Note => (Cmd::Note, "note"),
                Tool::Bone => (Cmd::BoneTool, "bones"),
            };
            ui.label(RichText::new(format!("{} {name}", cmd.spec().icon)).strong());
            ui.separator();
            match self.tool {
                Tool::Brush => {
                    self.brush_main(ui);
                    ui.menu_button(format!("{} more", icon::MORE), |ui| {
                        self.check(|e| e.brush_more(ui));
                    })
                    .response
                    .hint("hardness, feather, pressure curve, taper, stabiliser and smoothing");
                    self.command_button(ui, Cmd::BrushWindow, Some("edit"));
                    self.command_check(ui, Cmd::Symmetry, "mirror");
                    if let Some(axis) = self.mirror.as_mut() {
                        ui.add(egui::DragValue::new(axis).speed(1.0).prefix("axis x ").suffix(" px"))
                            .hint("where the mirror line stands, in canvas px");
                    }
                    if self.custom.is_some() {
                        ui.label(RichText::new("edited, not saved").color(theme::WEAK));
                    }
                    if self.scene.active_is_mask() {
                        ui.colored_label(Color32::from_rgb(255, 150, 150), self.mask_hint());
                    }
                    let vector = self
                        .scene
                        .active_layer()
                        .is_some_and(|l| l.kind() == klad_core::doc::Kind::Vector);
                    if vector {
                        ui.colored_label(
                            theme::REFUSED,
                            "this is a vector layer: P draws paths on it; brushes paint on paint layers",
                        );
                    } else if !self.scene.can_paint() {
                        ui.colored_label(theme::REFUSED, "pick a paint layer to draw on");
                    }
                }
                Tool::Fill => self.fill_panel(ui),
                Tool::Strokes => {
                    self.pick_scope(ui);
                    self.command_check(ui, Cmd::CutLasso, "a lasso cuts strokes at its edge");
                    self.command_button(ui, Cmd::SelectAll, Some("select all"));
                    ui.label(RichText::new("click or lasso strokes; the bar under them edits them").color(theme::WEAK));
                }
                Tool::Select if self.shifting.is_some() => self.shift_options(ui),
                Tool::Select => {
                    self.pick_scope(ui);
                    ui.label(
                        RichText::new("click what you see and drag it; alt drag: a copy; drag on empty canvas: a lasso")
                            .color(theme::WEAK),
                    );
                }
                Tool::Direct => {
                    ui.label(RichText::new("click a stroke or a shape's outline, then drag its points").color(theme::WEAK));
                    self.command_button(ui, Cmd::Corner, Some("corner"));
                    self.command_button(ui, Cmd::Smooth, Some("smooth"));
                    ui.add(
                        egui::DragValue::new(&mut self.simplify_px)
                            .speed(0.05)
                            .range(0.1..=20.0)
                            .suffix(" px"),
                    )
                    .hint("how far simplified points may stray from the outline");
                    self.command_button(ui, Cmd::Simplify, Some("simplify"));
                    self.command_button(ui, Cmd::MakePath, Some("make it a path"));
                }
                Tool::Shape => {
                    [(super::make::ShapeKind::Rect, "rectangle"), (super::make::ShapeKind::Ellipse, "ellipse")]
                        .iter()
                        .for_each(|(kind, label)| {
                            if ui
                                .selectable_label(self.shapes.kind == *kind, *label)
                                .hint(format!("draw {label}s"))
                                .clicked()
                            {
                                self.shapes.kind = *kind;
                            }
                        });
                    if self.shapes.kind == super::make::ShapeKind::Rect {
                        ui.add(
                            egui::DragValue::new(&mut self.shapes.radius)
                                .speed(0.2)
                                .range(0.0..=500.0)
                                .prefix("corners ")
                                .suffix(" px"),
                        )
                        .hint("how round the rectangle's corners are");
                    }
                    self.vector_colours(ui, "fill the shape with the first colour", "outline the shape in the second colour");
                    ui.add(
                        egui::DragValue::new(&mut self.pen_width)
                            .speed(0.1)
                            .range(0.25..=100.0)
                            .suffix(" px"),
                    )
                    .hint("the outline's width");
                    ui.label(
                        RichText::new("drag on a vector layer; shift for a square or circle, alt from the centre")
                            .color(theme::WEAK),
                    );
                }
                Tool::Text => self.check(|e| e.text_options(ui)),
                Tool::Bone => self.check(|e| e.bone_options(ui)),
                Tool::Pen => {
                    self.vector_colours(
                        ui,
                        "closed paths are filled with the first colour",
                        "paths get an outline in the second colour; open paths always do",
                    );
                    ui.add(
                        egui::DragValue::new(&mut self.pen_width)
                            .speed(0.1)
                            .range(0.25..=100.0)
                            .suffix(" px"),
                    )
                    .hint("the outline's width");
                    match self
                        .scene
                        .active_layer()
                        .is_some_and(|l| l.kind() == klad_core::doc::Kind::Vector)
                    {
                        true => ui.label(
                            RichText::new(
                                "click for corners, drag for curves, click the first point to close, Return finishes, Esc drops it",
                            )
                            .color(theme::WEAK),
                        ),
                        false => ui.colored_label(theme::REFUSED, "pick a vector layer: the pen draws vector paths"),
                    };
                }
                Tool::Note => {
                    ui.label(
                        RichText::new(
                            "click the canvas to pin a note for the agent; right click a pin (any frame's) to go to it, Delete removes it",
                        )
                        .color(theme::WEAK),
                    );
                }
            }
        });
        Ok(())
    }

    fn pick_scope(&mut self, ui: &mut egui::Ui) {
        let all = self.settings.pick_all_layers;
        [
            (false, "this layer", "a click picks strokes on the active layer only"),
            (
                true,
                "all layers",
                "a click picks what you see, on any layer, and makes its layer active",
            ),
        ]
        .into_iter()
        .for_each(|(scope, label, hint)| {
            if ui.selectable_label(all == scope, label).hint(hint).clicked() && all != scope {
                self.queued.push(Cmd::PickAllLayers);
            }
        });
        ui.separator();
    }

    pub(super) fn brush_tiles(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("size");
            SIZES.iter().for_each(|s| {
                if ui
                    .selectable_label((self.size - s).abs() < 0.01, format!("{s:.0}"))
                    .hint(format!("brush size {s:.0} px"))
                    .clicked()
                {
                    self.size = *s;
                }
            });
        });
        let defs: Vec<(String, BrushDef)> = self.brushes.defs().map(|(id, d)| (id.to_string(), d.clone())).collect();
        let (own, library): (Vec<_>, Vec<_>) = defs.into_iter().partition(|(id, _)| self.embedded.contains_key(id));
        let ctx = ui.ctx().clone();
        let mut picked = None;
        [("in this document", own), ("library", library)]
            .into_iter()
            .filter(|(_, list)| !list.is_empty())
            .for_each(|(title, list)| {
                ui.label(RichText::new(title).color(theme::WEAK).small());
                list.iter().for_each(|(id, def)| {
                    let label = label_of(id, def);
                    let selected = self.brush == *id;
                    let texture = match id.as_str() {
                        "eraser" => None,
                        _ => self.samples.texture(&ctx, &self.brushes, id, def),
                    };
                    let text = RichText::new(match id.as_str() {
                        "eraser" => format!("{} {label}", icon::ERASER),
                        _ => label.clone(),
                    });
                    let button = match &texture {
                        Some(t) => egui::Button::image_and_text(
                            egui::Image::new(t).fit_to_exact_size(vec2(SAMPLE.0 as f32, SAMPLE.1 as f32)),
                            text,
                        ),
                        None => egui::Button::new(text),
                    };
                    if ui
                        .add(
                            button
                                .selected(selected)
                                .min_size(vec2(ui.available_width(), theme::TARGET)),
                        )
                        .hint(format!("{id}: {}", def.name))
                        .clicked()
                    {
                        picked = Some(id.clone());
                    }
                });
            });
        if let Some(id) = picked {
            self.tool = Tool::Brush;
            if id != "eraser" {
                self.previous_brush = id.clone();
            }
            self.pick(&id);
        }
    }

    pub(super) fn palette_row(&mut self, ui: &mut egui::Ui) -> Option<PaletteEdit> {
        let palette = self.scene.bundle.doc.palette.clone();
        let current = color::hex(self.color);
        let mut edit = None;
        let mut picked = None;
        let mut rects = Vec::new();
        ui.horizontal_wrapped(|ui| {
            palette.iter().enumerate().for_each(|(i, swatch)| {
                let rgba = color::rgba_of(&swatch.color).unwrap_or([0.0; 4]);
                let name = match swatch.name.is_empty() {
                    true => swatch.color.clone(),
                    false => format!("{}  {}", swatch.name, swatch.color),
                };
                let response = color::swatch_with(
                    ui,
                    rgba,
                    swatch.color.eq_ignore_ascii_case(&current),
                    28.0,
                    Sense::click_and_drag(),
                )
                .hint(format!(
                    "{name}\nclick to use it, drag to move it, the x in its corner removes it"
                ));
                rects.push(response.rect);
                let corner = egui::Rect::from_min_size(response.rect.right_top() - vec2(11.0, -1.0), vec2(10.0, 10.0));
                if response.hovered() && self.swatch_drag.is_none() {
                    ui.painter()
                        .circle_filled(corner.center(), 5.0, Color32::from_black_alpha(200));
                    ui.painter().text(
                        corner.center(),
                        egui::Align2::CENTER_CENTER,
                        icon::CLOSE,
                        egui::FontId::proportional(9.0),
                        Color32::WHITE,
                    );
                }
                if response.clicked() {
                    let on_corner = response
                        .interact_pointer_pos()
                        .is_some_and(|p| corner.expand(2.0).contains(p));
                    match on_corner {
                        true => {
                            let mut rest = palette.clone();
                            rest.remove(i);
                            edit = Some(PaletteEdit::Set { swatches: rest });
                        }
                        false => picked = Some(rgba),
                    }
                }
                if response.secondary_clicked() {
                    let mut rest = palette.clone();
                    rest.remove(i);
                    edit = Some(PaletteEdit::Set { swatches: rest });
                }
                if response.drag_started() {
                    self.swatch_drag = Some(i);
                }
            });
        });
        if let (Some(from), true) = (self.swatch_drag, ui.input(|i| i.pointer.any_released())) {
            let at = ui.input(|i| i.pointer.interact_pos());
            let to = at.and_then(|p| rects.iter().position(|r| r.expand(3.0).contains(p)));
            if let Some(to) = to.filter(|to| *to != from) {
                edit = Some(PaletteEdit::Set {
                    swatches: moved(&palette, from, to),
                });
            }
            self.swatch_drag = None;
        }
        if let Some(rgba) = picked {
            self.pick_new_colour(rgba);
        }
        edit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::bundle::Bundle;
    use klad_core::doc::Document;

    #[test]
    fn dragging_sideways_doubles_the_size_every_120_pt() {
        assert_eq!(size_from_drag(10.0, 0.0), 10.0);
        assert!((size_from_drag(10.0, 120.0) - 20.0).abs() < 1e-4);
        assert!((size_from_drag(10.0, -120.0) - 5.0).abs() < 1e-4);
        assert_eq!(size_from_drag(10.0, 10_000.0), 800.0);
        assert_eq!(size_from_drag(1.0, -10_000.0), 0.5);
    }

    #[test]
    fn a_swatch_moves_to_where_it_is_dropped() {
        assert_eq!(moved(&[0, 1, 2, 3], 0, 2), [1, 2, 0, 3]);
        assert_eq!(moved(&[0, 1, 2, 3], 3, 0), [3, 0, 1, 2]);
        assert_eq!(moved(&[0, 1, 2], 1, 9), [0, 2, 1]);
        assert_eq!(moved(&[0, 1, 2], 7, 0), [0, 1, 2]);
    }

    #[test]
    fn brush_samples_are_drawn_the_same_every_time_and_differ_between_brushes() {
        let dir = std::env::temp_dir().join(format!("draw-samples-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bundle = Bundle::create(&dir, Document::new(64, 64)).unwrap();
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let hashes: Vec<u64> = ["ink", "pencil", "marker", "airbrush"]
            .iter()
            .map(|id| sample(&brushes, id).unwrap().hash())
            .collect();
        assert_eq!(hashes[0], sample(&brushes, "ink").unwrap().hash());
        let distinct: std::collections::BTreeSet<u64> = hashes.iter().copied().collect();
        assert_eq!(distinct.len(), 4);
        let image = image_of(&sample(&brushes, "ink").unwrap(), SAMPLE, Color32::BLACK);
        assert_eq!(image.size, [SAMPLE.0 as usize, SAMPLE.1 as usize]);
        assert!(image.pixels.iter().any(|p| p.r() > 120));
        assert!(sample(&brushes, "no-such").is_err());
        assert_eq!(
            label_of("ink-3f0c9e", brushes.def("ink").unwrap()),
            "ink variant 3f0c9e"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
