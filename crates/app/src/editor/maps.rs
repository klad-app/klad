use super::Editor;
use crate::color;
use crate::messages::Tone;
use crate::theme::Hint;
use anyhow::Result;
use egui::{Color32, Key, Mesh, Rect, TextureHandle, TextureOptions, pos2, vec2};
use klad_core::doc::{Document, Kind, Layer};
use klad_core::maps::{Light, LightKind, Maps, Profile, Role, Surface};
use klad_core::ops::Op;
use klad_render::Frame;
use klad_render::maps::{Lit, Map};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Show {
    Map(Map),
    Lit,
}

impl Show {
    fn name(self) -> &'static str {
        match self {
            Show::Map(map) => map.name(),
            Show::Lit => "lit",
        }
    }
}

enum Ready {
    Map(egui::ColorImage),
    Lit(Arc<Lit>),
}

#[derive(Clone, Copy, PartialEq)]
struct Want {
    rev: u64,
    frame: u32,
    kind: Option<Map>,
}

#[derive(Default)]
pub(super) struct MapsView {
    pub(super) show: Option<Show>,
    want: Option<Want>,
    job: Option<(Want, Receiver<Result<Ready>>)>,
    lit: Option<(Want, Arc<Lit>)>,
    texture: Option<(Want, Option<[f32; 2]>, usize, TextureHandle)>,
}

fn preview_scale(doc: &Document) -> f32 {
    match doc.pixel_art() {
        true => 1.0,
        false => (1536.0 / doc.canvas.width.max(doc.canvas.height) as f32).clamp(1.0 / 16.0, 1.0),
    }
}

fn image_of(frame: &Frame) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba)
}

fn pointer_light(doc: &Document, at: [f32; 2]) -> Light {
    let longest = doc.canvas.width.max(doc.canvas.height) as f32;
    Light {
        at,
        height: (longest * 0.15).round(),
        range: longest,
        ..Light::default()
    }
}

pub(super) const NEXT: [Option<Map>; 5] = [
    Some(Map::Normal),
    Some(Map::Height),
    Some(Map::Specular),
    Some(Map::Emission),
    None,
];

impl Editor {
    #[cfg(test)]
    pub(super) fn map_shown(&self) -> Option<&'static str> {
        let want = self.maps.want?;
        self.maps
            .texture
            .as_ref()
            .filter(|t| t.0.kind == want.kind)
            .map(|_| self.maps.show.map_or("lit", Show::name))
    }

    pub(super) fn next_map(&mut self) {
        let now = match self.maps.show {
            Some(Show::Map(map)) => NEXT.iter().position(|m| *m == Some(map)),
            _ => None,
        };
        let next = match now {
            Some(i) => NEXT[(i + 1) % NEXT.len()],
            None => NEXT[0],
        };
        self.maps.show = next.map(Show::Map);
        let shown = self.maps.show.map_or("the picture", Show::name);
        self.say(Tone::Info, format!("showing {shown}"));
    }

    pub(super) fn toggle_lit(&mut self) {
        self.maps.show = match self.maps.show {
            Some(Show::Lit) => None,
            _ => Some(Show::Lit),
        };
        if self.maps.show.is_some() && klad_render::maps::settings(&self.scene.bundle.doc).lights.is_empty() {
            self.say(
                Tone::Info,
                "no preview lights yet: add one in the light maps panel, or hold L to light it with the pointer",
            );
        }
    }

    fn start_maps_job(&mut self, want: Want, ctx: &egui::Context) {
        let doc = self.scene.bundle.doc.clone();
        let rasters = self.scene.rasters_now();
        let frame = want.frame;
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let scale = preview_scale(&doc);
            let done = match want.kind {
                Some(map) => klad_render::maps::render(&doc, frame, &rasters, scale)
                    .map(|set| Ready::Map(image_of(&set.frame(map)))),
                None => Lit::prepare(&doc, frame, &rasters, scale).map(|lit| Ready::Lit(Arc::new(lit))),
            };
            let _ = tx.send(done);
            ctx.request_repaint();
        });
        self.maps.job = Some((want, rx));
    }

    pub(super) fn map_overlay(&mut self, ui: &egui::Ui, view: crate::view::View, rect: Rect) {
        let typing = ui.ctx().egui_wants_keyboard_input();
        let held = !typing && ui.input(|i| i.key_down(Key::L) && !i.modifiers.any());
        let show = match held {
            true => Some(Show::Lit),
            false => self.maps.show,
        };
        let Some(show) = show else {
            return;
        };
        let want = Want {
            rev: self.scene.bundle.doc.rev,
            frame: self.scene.frame,
            kind: match show {
                Show::Map(map) => Some(map),
                Show::Lit => None,
            },
        };
        self.maps.want = Some(want);
        if let Some((asked, rx)) = &self.maps.job
            && let Ok(done) = rx.try_recv()
        {
            let asked = *asked;
            self.maps.job = None;
            match done {
                Ok(Ready::Map(image)) => {
                    let texture = ui.ctx().load_texture("light map", image, TextureOptions::LINEAR);
                    self.maps.texture = Some((asked, None, 0, texture));
                }
                Ok(Ready::Lit(lit)) => self.maps.lit = Some((asked, lit)),
                Err(e) => {
                    self.maps.show = None;
                    self.say(Tone::Error, format!("the light maps could not be made: {e:#}"));
                    return;
                }
            }
        }
        let stale = match want.kind {
            Some(_) => self.maps.texture.as_ref().is_none_or(|t| t.0 != want),
            None => self.maps.lit.as_ref().is_none_or(|l| l.0 != want),
        };
        if stale && self.maps.job.is_none() {
            self.start_maps_job(want, ui.ctx());
        }
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(|p| {
                let c = view.to_canvas(p - rect.min, rect.size());
                [c.x, c.y]
            });
        if want.kind.is_none() {
            let doc = &self.scene.bundle.doc;
            let mut lights = klad_render::maps::settings(doc).lights;
            let at = match held {
                true => pointer,
                false => None,
            };
            lights.extend(at.map(|p| pointer_light(doc, p)));
            let count = lights.len();
            let fresh = self
                .maps
                .texture
                .as_ref()
                .is_some_and(|t| t.0 == want && t.1 == at && t.2 == count);
            if let (Some((made, lit)), false) = (&self.maps.lit, fresh) {
                match lit.shade(&lights) {
                    Ok(frame) => {
                        let made = *made;
                        let texture = ui
                            .ctx()
                            .load_texture("lit preview", image_of(&frame), TextureOptions::LINEAR);
                        self.maps.texture = Some((made, at, count, texture));
                    }
                    Err(e) => self.say(Tone::Error, format!("the light preview failed: {e:#}")),
                }
            }
        }
        let shown = self.maps.texture.as_ref().filter(|t| match want.kind {
            Some(_) => t.0.kind == want.kind,
            None => t.0.kind.is_none(),
        });
        let painter = ui.painter().with_clip_rect(rect);
        let Some((_, _, _, texture)) = shown else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "making the light maps...",
                egui::FontId::proportional(16.0),
                Color32::WHITE,
            );
            ui.ctx().request_repaint();
            return;
        };
        let (w, h) = self.scene.size();
        let corners = [
            vec2(0.0, 0.0),
            vec2(w as f32, 0.0),
            vec2(w as f32, h as f32),
            vec2(0.0, h as f32),
        ]
        .map(|c| self.screen(view, rect, c));
        let mut mesh = Mesh::with_texture(texture.id());
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .iter()
            .zip(corners)
            .for_each(|((u, v), p)| {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p,
                    uv: pos2(*u, *v),
                    color: Color32::WHITE,
                })
            });
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(mesh);
        if self.maps.job.is_some() || held {
            ui.ctx().request_repaint();
        }
    }

    fn set_maps(&mut self, maps: Option<Maps>) -> Result<()> {
        self.apply(vec![Op::Maps(maps)])
    }

    fn surface_fields(&mut self, ui: &mut egui::Ui, key: &str, surface: &Surface) -> Option<Surface> {
        let mut next = surface.clone();
        ui.horizontal_wrapped(|ui| {
            Profile::ALL.iter().for_each(|p| {
                if ui
                    .selectable_label(surface.profile == *p, p.name())
                    .hint(match p {
                        Profile::Round => "the rim rounds over like a cushion's edge, steep at the outline",
                        Profile::Chamfer => "a straight slope from the outline to the top",
                        Profile::Pillow => "soft at the outline, steepest halfway in",
                        Profile::Flat => "no rim: only paint with body and height layers shape it",
                    })
                    .clicked()
                {
                    next.profile = *p;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            [
                (
                    "bevel ",
                    surface.bevel,
                    0.0..=512.0,
                    0.5,
                    "px the rim rounds over, in from the outline",
                ),
                (
                    "strength ",
                    surface.strength,
                    0.0..=10.0,
                    0.02,
                    "how steep the slopes read in the normal map",
                ),
                (
                    "body ",
                    surface.body,
                    0.0..=10.0,
                    0.02,
                    "how much paint with body raises it; 0 for none",
                ),
                (
                    "range ",
                    surface.range,
                    0.0..=512.0,
                    0.5,
                    "px a white height layer raises it (black lowers)",
                ),
            ]
            .into_iter()
            .enumerate()
            .for_each(|(i, (label, value, range, speed, hint))| {
                if let Some(v) = self.number(ui, &format!("{key}.{i}"), label, value, range, speed, hint) {
                    match i {
                        0 => next.bevel = v,
                        1 => next.strength = v,
                        2 => next.body = v,
                        _ => next.range = v,
                    }
                }
            });
        });
        (next != *surface).then_some(next)
    }

    fn layer_maps(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        let id = layer.id.clone();
        let top = self.scene.bundle.doc.layers.iter().any(|l| l.id == id);
        ui.label(egui::RichText::new(format!("Layer {}", layer.name)).strong());
        if matches!(layer.kind(), Kind::Paint | Kind::Vector) {
            let mut role = layer.map;
            ui.horizontal_wrapped(|ui| {
                ui.label("draws");
                if ui
                    .selectable_label(role.is_none(), "colour")
                    .hint("an ordinary layer: drawn in the picture, part of the surface")
                    .clicked()
                {
                    role = None;
                }
                Role::ALL.iter().for_each(|r| {
                    if ui
                        .selectable_label(role == Some(*r), r.name())
                        .hint(match r {
                            Role::Height => {
                                "a height layer: white raises the surface, black lowers it; never drawn in the picture"
                            }
                            Role::Normal => {
                                "a normal layer: its colours are directions (#8080ff faces out), over the surface's own"
                            }
                            Role::Specular => "a specular layer: brighter is shinier under the preview lights",
                            Role::Emission => "an emission layer: what glows whatever the light",
                        })
                        .clicked()
                    {
                        role = Some(*r);
                    }
                });
            });
            if role != layer.map {
                let value = role.map_or(Value::Null, |r| json!(r.name()));
                return self.set(&id, vec![("map".into(), value)]);
            }
        }
        if top && layer.map.is_none() {
            let mut own = layer.surface.is_some();
            if ui
                .checkbox(&mut own, "a surface of its own")
                .hint("this layer rounds in from its own outline instead of being part of the drawing's surface")
                .changed()
            {
                let value = match own {
                    true => serde_json::to_value(klad_render::maps::settings(&self.scene.bundle.doc).surface)?,
                    false => Value::Null,
                };
                return self.set(&id, vec![("surface".into(), value)]);
            }
            if let Some(surface) = &layer.surface
                && let Some(next) = self.surface_fields(ui, &format!("{id}.surface"), surface)
            {
                return self.set(&id, vec![("surface".into(), serde_json::to_value(next)?)]);
            }
        }
        Ok(())
    }

    pub(super) fn maps_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal_wrapped(|ui| {
            ui.label("show");
            let mut show = self.maps.show;
            if ui.selectable_label(show.is_none(), "picture").clicked() {
                show = None;
            }
            Map::ALL.iter().for_each(|m| {
                if ui.selectable_label(show == Some(Show::Map(*m)), m.name()).clicked() {
                    show = Some(Show::Map(*m));
                }
            });
            if ui
                .selectable_label(show == Some(Show::Lit), "lit")
                .hint("the picture lit by the preview lights; hold L to light it with the pointer")
                .clicked()
            {
                show = Some(Show::Lit);
            }
            self.maps.show = show;
        });
        let maps = self.scene.bundle.doc.maps.clone();
        let mut on = maps.is_some();
        if ui
            .checkbox(&mut on, "light maps")
            .hint("normal, height, specular and emission maps for a game's 2D lights, made from what is drawn")
            .changed()
        {
            return self.set_maps(on.then(Maps::default));
        }
        let current = maps.clone().unwrap_or_default();
        ui.label(egui::RichText::new("The drawing's surface").strong());
        if let Some(next) = self.surface_fields(ui, "maps.surface", &current.surface) {
            return self.set_maps(Some(Maps {
                surface: next,
                ..current
            }));
        }
        if let Some(layer) = self.scene.active_layer().cloned() {
            ui.separator();
            self.layer_maps(ui, &layer)?;
        }
        ui.separator();
        ui.label(egui::RichText::new("Preview lights").strong());
        let mut next = current.clone();
        let paint = self.color;
        ui.horizontal_wrapped(|ui| {
            let ambient = color::rgba_of(&current.ambient).unwrap_or([0.2, 0.2, 0.25, 1.0]);
            if color::swatch(ui, ambient, false, crate::theme::TARGET)
                .hint(format!(
                    "the ambient colour, {}, where no light reaches (Godot's CanvasModulate); click to use the current colour",
                    current.ambient
                ))
                .clicked()
            {
                next.ambient = color::hex(paint);
            }
            ui.label("ambient");
            if let Some(v) = self.number(
                ui,
                "maps.shininess",
                "shininess ",
                current.shininess,
                1.0..=512.0,
                0.5,
                "how tight the highlights on the specular map are",
            ) {
                next.shininess = v;
            }
        });
        let (w, h) = self.scene.size();
        let mut gone = None;
        current.lights.iter().enumerate().for_each(|(i, light)| {
            ui.horizontal_wrapped(|ui| {
                let rgba = color::rgba_of(&light.color).unwrap_or([1.0; 4]);
                if color::swatch(ui, rgba, false, crate::theme::TARGET)
                    .hint(format!(
                        "light {}, {}; click to use the current colour",
                        i + 1,
                        light.color
                    ))
                    .clicked()
                {
                    next.lights[i].color = color::hex(paint);
                }
                let point = light.kind == LightKind::Point;
                ui.label(match point {
                    true => "point",
                    false => "from",
                });
                let fields: Vec<(&str, f32, std::ops::RangeInclusive<f32>, f64, &str)> = match point {
                    true => vec![
                        (
                            "x ",
                            light.at[0],
                            -(w as f32)..=2.0 * w as f32,
                            1.0,
                            "where it is across the canvas",
                        ),
                        (
                            "y ",
                            light.at[1],
                            -(h as f32)..=2.0 * h as f32,
                            1.0,
                            "where it is down the canvas",
                        ),
                        ("height ", light.height, 0.0..=4096.0, 1.0, "px in front of the canvas"),
                        ("energy ", light.energy, 0.0..=16.0, 0.02, "how bright"),
                        ("range ", light.range, 1.0..=16384.0, 2.0, "px to where it fades out"),
                    ],
                    false => vec![
                        (
                            "x ",
                            light.at[0],
                            -1.0..=1.0,
                            0.01,
                            "the direction it comes from, across",
                        ),
                        ("y ", light.at[1], -1.0..=1.0, 0.01, "the direction it comes from, down"),
                        (
                            "height ",
                            light.height,
                            0.0..=1.0,
                            0.01,
                            "0 along the picture, 1 straight from the front",
                        ),
                        ("energy ", light.energy, 0.0..=16.0, 0.02, "how bright"),
                    ],
                };
                fields
                    .into_iter()
                    .enumerate()
                    .for_each(|(k, (label, value, range, speed, hint))| {
                        if let Some(v) = self.number(ui, &format!("light.{i}.{k}"), label, value, range, speed, hint) {
                            let l = &mut next.lights[i];
                            match k {
                                0 => l.at[0] = v,
                                1 => l.at[1] = v,
                                2 => l.height = v,
                                3 => l.energy = v,
                                _ => l.range = v,
                            }
                        }
                    });
                if ui
                    .small_button(crate::theme::icon::TRASH)
                    .hint(format!("remove light {}", i + 1))
                    .clicked()
                {
                    gone = Some(i);
                }
            });
        });
        if let Some(i) = gone {
            next.lights.remove(i);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("add a point light")
                .hint("a light above the top left of the drawing; move it with x and y")
                .clicked()
            {
                next.lights.push(pointer_light(
                    &self.scene.bundle.doc,
                    [w as f32 * 0.25, h as f32 * 0.25],
                ));
            }
            if ui
                .button("add a directional light")
                .hint("light from one direction everywhere, like the sun")
                .clicked()
            {
                next.lights.push(Light::directional([-1.0, -1.0], 0.5));
            }
        });
        if next != current {
            if next.validate().is_err() {
                return Ok(());
            }
            if maps.is_none() && next.lights.is_empty() && next == Maps::default() {
                return Ok(());
            }
            return self.set_maps(Some(next));
        }
        Ok(())
    }
}
