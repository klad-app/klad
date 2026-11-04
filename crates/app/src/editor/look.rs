use super::Editor;
use crate::color;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, RichText, Sense, Stroke, pos2, vec2};
use klad_core::doc::{Blend, Layer, LayerContent, Paint, Stop};
use klad_core::look::{self, ADJUSTMENTS, Adjustment, Curves, EFFECTS, Effect, Rays};
use serde_json::Value;

const NEAR: f32 = 14.0;
const CHANNELS: [(&str, Color32); 4] = [
    ("rgb", Color32::from_gray(220)),
    ("red", Color32::from_rgb(235, 90, 90)),
    ("green", Color32::from_rgb(110, 210, 120)),
    ("blue", Color32::from_rgb(100, 150, 255)),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Touch {
    #[default]
    Still,
    Preview,
    Commit,
}

impl Touch {
    fn of(r: &egui::Response) -> Self {
        match (r.drag_stopped(), r.changed(), r.dragged()) {
            (true, _, _) => Touch::Commit,
            (_, true, true) => Touch::Preview,
            (_, true, false) => Touch::Commit,
            _ => Touch::Still,
        }
    }

    fn add(&mut self, other: Touch) {
        *self = (*self).max(other);
    }
}

pub(super) struct Draft {
    pub id: String,
    pub original: Layer,
}

#[derive(Default)]
pub(super) struct LookState {
    pub draft: Option<Draft>,
    pub channel: usize,
    pub dragging: Option<usize>,
}

fn slide(
    ui: &mut egui::Ui,
    touch: &mut Touch,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    text: &str,
    hint: &str,
) {
    let r = ui.add(egui::Slider::new(value, range).text(text)).hint(hint);
    touch.add(Touch::of(&r));
}

fn check(ui: &mut egui::Ui, touch: &mut Touch, value: &mut bool, text: &str, hint: &str) {
    if ui.checkbox(value, text).hint(hint).changed() {
        touch.add(Touch::Commit);
    }
}

fn swatch(ui: &mut egui::Ui, touch: &mut Touch, hex: &mut String, current: [f32; 4], what: &str) {
    let rgba = color::rgba_of(hex).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    if color::swatch(ui, rgba, false, theme::TARGET)
        .hint(format!("{what}, {hex}; click to use the current colour"))
        .clicked()
    {
        *hex = color::hex(current);
        touch.add(Touch::Commit);
    }
}

fn blend_pick(ui: &mut egui::Ui, touch: &mut Touch, value: &mut Blend, salt: &str, hint: &str) {
    let before = *value;
    egui::ComboBox::from_id_salt(salt)
        .width(110.0)
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            Blend::ALL.iter().for_each(|b| {
                ui.selectable_value(value, *b, b.name());
            });
        })
        .response
        .hint(hint);
    if *value != before {
        touch.add(Touch::Commit);
    }
}

fn seed(ui: &mut egui::Ui, touch: &mut Touch, value: &mut u32) {
    let r = ui
        .add(egui::DragValue::new(value).range(0..=99_999).prefix("seed "))
        .hint("another seed gives another pattern of the same kind");
    touch.add(Touch::of(&r));
}

fn effect_controls(
    ui: &mut egui::Ui,
    effect: &mut Effect,
    touch: &mut Touch,
    current: [f32; 4],
    layers: &[(String, String)],
    height: f32,
) {
    match effect {
        Effect::Haze(h) => {
            slide(
                ui,
                touch,
                &mut h.amount,
                0.0..=1.0,
                "amount",
                "how far the colours go towards the haze colour",
            );
            slide(
                ui,
                touch,
                &mut h.desaturate,
                0.0..=1.0,
                "greyer",
                "how much colour the haze takes out",
            );
            ui.horizontal(|ui| {
                swatch(ui, touch, &mut h.color, current, "the haze colour");
                let mut graded = h.top.is_some();
                check(
                    ui,
                    touch,
                    &mut graded,
                    "by height",
                    "a second colour at the top, blending down to the first",
                );
                if graded != h.top.is_some() {
                    h.top = graded.then(|| color::hex(current));
                    h.span = height;
                }
                if let Some(top) = h.top.as_mut() {
                    swatch(ui, touch, top, current, "the haze colour at the top");
                }
            });
        }
        Effect::Blur(b) => slide(ui, touch, &mut b.radius, 0.0..=64.0, "radius", "how soft, in px"),
        Effect::Glow(g) => {
            slide(
                ui,
                touch,
                &mut g.radius,
                0.0..=128.0,
                "radius",
                "how far the glow spreads, in px",
            );
            slide(
                ui,
                touch,
                &mut g.strength,
                0.0..=3.0,
                "strength",
                "how bright the glow is",
            );
            slide(
                ui,
                touch,
                &mut g.threshold,
                0.0..=1.0,
                "from brightness",
                "0 glows from every pixel; higher glows only from the bright ones (bloom)",
            );
            ui.horizontal(|ui| {
                let mut tinted = g.color.is_some();
                check(ui, touch, &mut tinted, "tint", "glow in one colour instead of the layer's own");
                if tinted != g.color.is_some() {
                    g.color = tinted.then(|| color::hex(current));
                }
                if let Some(hex) = g.color.as_mut() {
                    swatch(ui, touch, hex, current, "the glow's colour");
                }
                blend_pick(
                    ui,
                    touch,
                    &mut g.blend,
                    "glow.blend",
                    "how the glow mixes with what is under the layer: screen and add light it up, normal lays it like paint",
                );
            });
        }
        Effect::Outline(o) => {
            slide(ui, touch, &mut o.width, 0.0..=32.0, "width", "the line's width, in px");
            ui.horizontal(|ui| {
                swatch(ui, touch, &mut o.color, current, "the outline's colour");
                check(
                    ui,
                    touch,
                    &mut o.inside,
                    "inside",
                    "draw the line inside the edge instead of around it",
                );
            });
        }
        Effect::Shadow(s) => {
            slide(
                ui,
                touch,
                &mut s.x,
                -64.0..=64.0,
                "across",
                "how far right the shadow falls, in px",
            );
            slide(
                ui,
                touch,
                &mut s.y,
                -64.0..=64.0,
                "down",
                "how far down the shadow falls, in px",
            );
            slide(
                ui,
                touch,
                &mut s.radius,
                0.0..=64.0,
                "softness",
                "how soft the shadow is, in px",
            );
            swatch(
                ui,
                touch,
                &mut s.color,
                current,
                "the shadow's colour (its alpha is its strength)",
            );
        }
        Effect::Noise(n) => {
            slide(ui, touch, &mut n.amount, 0.0..=1.0, "amount", "how strong the grain is");
            slide(
                ui,
                touch,
                &mut n.size,
                1.0..=16.0,
                "grain size",
                "the size of a grain, in px",
            );
            ui.horizontal(|ui| {
                check(
                    ui,
                    touch,
                    &mut n.mono,
                    "grey",
                    "the same grain on every channel; off gives coloured grain",
                );
                seed(ui, touch, &mut n.seed);
            });
        }
        Effect::Displace(d) => {
            slide(
                ui,
                touch,
                &mut d.amount,
                0.0..=64.0,
                "amount",
                "how far pixels are pushed, in px",
            );
            slide(
                ui,
                touch,
                &mut d.size,
                4.0..=256.0,
                "wave size",
                "the size of the waves that push, in px",
            );
            ui.horizontal(|ui| {
                let shown =
                    d.by.as_ref()
                        .and_then(|id| layers.iter().find(|(l, _)| l == id))
                        .map_or("waves".to_string(), |(_, name)| name.clone());
                let before = d.by.clone();
                egui::ComboBox::from_id_salt("displace.by")
                    .width(120.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.by, None, "waves");
                        layers.iter().for_each(|(id, name)| {
                            ui.selectable_value(&mut d.by, Some(id.clone()), name);
                        });
                    })
                    .response
                    .hint(
                        "push by smooth waves, or by a layer: its red pushes across, its green down (grey stays put)",
                    );
                if d.by != before {
                    touch.add(Touch::Commit);
                }
                seed(ui, touch, &mut d.seed);
            });
        }
        Effect::Relief(r) => {
            slide(
                ui,
                touch,
                &mut r.strength,
                0.0..=8.0,
                "strength",
                "how deep the paint's ridges look; only paint from brushes with body has ridges",
            );
            slide(
                ui,
                touch,
                &mut r.softness,
                0.0..=8.0,
                "softness",
                "how much the ridges are smoothed, in px",
            );
            ui.horizontal(|ui| {
                light_dial(ui, touch, &mut r.light);
                ui.vertical(|ui| {
                    swatch(
                        ui,
                        touch,
                        &mut r.highlight,
                        current,
                        "the colour ridges facing the light take",
                    );
                    swatch(
                        ui,
                        touch,
                        &mut r.shade,
                        current,
                        "the colour sides away from the light take",
                    );
                });
            });
        }
    }
}

pub(super) const DIAL: &str = "where the light comes from";

fn light_dial(ui: &mut egui::Ui, touch: &mut Touch, light: &mut [f32; 2]) {
    let side = 64.0;
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, DIAL));
    let response = response.hint(format!(
        "{DIAL}: click or drag towards it ({:.2}, {:.2})",
        light[0], light[1]
    ));
    let centre = rect.center();
    let reach = side * 0.5 - 6.0;
    let pressed = response.clicked() || response.dragged() || response.drag_stopped();
    if let Some(at) = response.interact_pointer_pos().filter(|_| pressed) {
        let towards = at - centre;
        if towards.length() > 3.0 {
            let unit = towards.normalized();
            let next = [(unit.x * 100.0).round() / 100.0, (unit.y * 100.0).round() / 100.0];
            if next != *light {
                *light = next;
                touch.add(match response.dragged() && !response.drag_stopped() {
                    true => Touch::Preview,
                    false => Touch::Commit,
                });
            }
        }
    }
    if response.drag_stopped() {
        touch.add(Touch::Commit);
    }
    let length = light[0].hypot(light[1]).max(1e-6);
    let sun = centre + vec2(light[0] / length, light[1] / length) * reach;
    let painter = ui.painter_at(rect);
    let line = ui.visuals().widgets.inactive.fg_stroke;
    painter.circle_stroke(centre, reach, Stroke::new(1.0, line.color.gamma_multiply(0.5)));
    painter.arrow(sun, (centre - sun) * 0.8, Stroke::new(1.5, line.color));
    painter.circle_filled(sun, 5.0, Color32::from_rgb(255, 214, 120));
}

fn curve_box(ui: &mut egui::Ui, points: &mut Vec<[f32; 2]>, state: &mut LookState, touch: &mut Touch, colour: Color32) {
    if points.is_empty() {
        *points = vec![[0.0, 0.0], [1.0, 1.0]];
    }
    let side = ui.available_width().clamp(120.0, 240.0);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let response = response.hint(
        "the curve: dark on the left, light on the right; drag a point to bend it, press empty space to add one, double-click a point to remove it",
    );
    let to_screen = |p: [f32; 2]| pos2(rect.left() + p[0] * side, rect.bottom() - p[1] * side);
    let from_screen = |p: egui::Pos2| {
        [
            ((p.x - rect.left()) / side).clamp(0.0, 1.0),
            ((rect.bottom() - p.y) / side).clamp(0.0, 1.0),
        ]
    };
    let nearest = |at: egui::Pos2, points: &[[f32; 2]]| {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, to_screen(*p).distance(at)))
            .filter(|(_, d)| *d <= NEAR)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let insert = |points: &mut Vec<[f32; 2]>, p: [f32; 2]| {
        let at = points.iter().position(|q| q[0] > p[0]).unwrap_or(points.len());
        points.insert(at, p);
        at
    };
    let pointer = response.interact_pointer_pos();
    if response.double_clicked()
        && let Some(i) = pointer.and_then(|at| nearest(at, points))
        && i > 0
        && i + 1 < points.len()
    {
        points.remove(i);
        touch.add(Touch::Commit);
    } else if response.drag_started()
        && let Some(at) = pointer
    {
        state.dragging = Some(nearest(at, points).unwrap_or_else(|| insert(points, from_screen(at))));
        touch.add(Touch::Preview);
    } else if response.clicked()
        && let Some(at) = pointer
        && nearest(at, points).is_none()
    {
        insert(points, from_screen(at));
        touch.add(Touch::Commit);
    }
    if response.dragged()
        && let (Some(i), Some(at)) = (state.dragging, pointer)
        && i < points.len()
    {
        let [x, y] = from_screen(at);
        let low = if i == 0 { 0.0 } else { points[i - 1][0] + 0.01 };
        let high = if i + 1 == points.len() {
            1.0
        } else {
            points[i + 1][0] - 0.01
        };
        points[i] = [x.clamp(low, high.max(low)), y];
        touch.add(Touch::Preview);
    }
    if response.drag_stopped() {
        state.dragging = None;
        touch.add(Touch::Commit);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_gray(24));
    (1..4).for_each(|k| {
        let t = k as f32 / 4.0;
        let faint = Stroke::new(1.0, Color32::from_gray(48));
        painter.line_segment([to_screen([t, 0.0]), to_screen([t, 1.0])], faint);
        painter.line_segment([to_screen([0.0, t]), to_screen([1.0, t])], faint);
    });
    painter.line_segment(
        [to_screen([0.0, 0.0]), to_screen([1.0, 1.0])],
        Stroke::new(1.0, Color32::from_gray(70)),
    );
    if let Ok(prepared) = Adjustment::Curves(Curves {
        points: points.clone(),
        ..Curves::default()
    })
    .prepare()
    {
        let line: Vec<egui::Pos2> = (0..=64)
            .map(|i| i as f32 / 64.0)
            .map(|v| to_screen([v, prepared.apply([v; 3])[0].clamp(0.0, 1.0)]))
            .collect();
        painter.add(egui::Shape::line(line, Stroke::new(2.0, colour)));
    }
    points.iter().enumerate().for_each(|(i, p)| {
        let held = state.dragging == Some(i);
        painter.circle(
            to_screen(*p),
            if held { 7.0 } else { 5.5 },
            if held { colour } else { Color32::from_gray(30) },
            Stroke::new(2.0, colour),
        );
    });
}

fn stops_controls(ui: &mut egui::Ui, stops: &mut Vec<Stop>, touch: &mut Touch, current: [f32; 4]) {
    let mut gone = None;
    let count = stops.len();
    stops.iter_mut().enumerate().for_each(|(i, stop)| {
        ui.push_id(("stop", i), |ui| {
            ui.horizontal(|ui| {
                swatch(ui, touch, &mut stop.1, current, "this stop's colour");
                let r = ui
                    .add(egui::Slider::new(&mut stop.0, 0.0..=1.0).text("at"))
                    .hint("where on the dark to light scale this colour sits");
                touch.add(Touch::of(&r));
                if count > 1 && ui.button(theme::icon::TRASH).hint("remove this stop").clicked() {
                    gone = Some(i);
                }
            });
        });
    });
    if let Some(i) = gone {
        stops.remove(i);
        touch.add(Touch::Commit);
    }
    if ui
        .button(format!("{} add a stop", theme::icon::PLUS))
        .hint("a stop in the current colour halfway along")
        .clicked()
    {
        stops.push(Stop(0.5, color::hex(current)));
        touch.add(Touch::Commit);
    }
}

fn adjust_controls(
    ui: &mut egui::Ui,
    adjust: &mut Adjustment,
    state: &mut LookState,
    touch: &mut Touch,
    current: [f32; 4],
) {
    match adjust {
        Adjustment::Hsl(h) => {
            slide(
                ui,
                touch,
                &mut h.hue,
                -180.0..=180.0,
                "hue",
                "turns every colour around the colour wheel, in degrees",
            );
            slide(
                ui,
                touch,
                &mut h.saturation,
                -1.0..=1.0,
                "saturation",
                "less colour to the left, more to the right",
            );
            slide(
                ui,
                touch,
                &mut h.lightness,
                -1.0..=1.0,
                "lightness",
                "towards black to the left, towards white to the right",
            );
        }
        Adjustment::Levels(l) => {
            slide(
                ui,
                touch,
                &mut l.black,
                0.0..=1.0,
                "black point",
                "this and darker becomes the darkest output",
            );
            slide(
                ui,
                touch,
                &mut l.white,
                0.0..=1.0,
                "white point",
                "this and lighter becomes the lightest output",
            );
            slide(
                ui,
                touch,
                &mut l.gamma,
                0.1..=4.0,
                "midtones",
                "above 1 lifts the middle tones, below 1 sinks them",
            );
            slide(
                ui,
                touch,
                &mut l.low,
                0.0..=1.0,
                "darkest output",
                "how dark the darkest pixels end up",
            );
            slide(
                ui,
                touch,
                &mut l.high,
                0.0..=1.0,
                "lightest output",
                "how light the lightest pixels end up",
            );
        }
        Adjustment::Curves(c) => {
            ui.horizontal(|ui| {
                CHANNELS.iter().enumerate().for_each(|(i, (name, colour))| {
                    if ui
                        .selectable_label(state.channel == i, RichText::new(*name).color(*colour))
                        .hint(match i {
                            0 => "edit the curve for all channels at once",
                            _ => "edit one channel's curve; it applies before the rgb curve",
                        })
                        .clicked()
                    {
                        state.channel = i;
                    }
                });
                if ui
                    .button("reset")
                    .hint("set this channel's curve back to a straight line")
                    .clicked()
                {
                    match state.channel {
                        1 => c.red.clear(),
                        2 => c.green.clear(),
                        3 => c.blue.clear(),
                        _ => c.points = vec![[0.0, 0.0], [1.0, 1.0]],
                    }
                    touch.add(Touch::Commit);
                }
            });
            let colour = CHANNELS[state.channel.min(3)].1;
            let points = match state.channel {
                1 => &mut c.red,
                2 => &mut c.green,
                3 => &mut c.blue,
                _ => &mut c.points,
            };
            curve_box(ui, points, state, touch, colour);
        }
        Adjustment::Balance(b) => {
            [
                ("shadows", &mut b.shadows),
                ("midtones", &mut b.midtones),
                ("highlights", &mut b.highlights),
            ]
            .into_iter()
            .for_each(|(name, values)| {
                ui.label(name);
                ui.push_id(name, |ui| {
                    slide(
                        ui,
                        touch,
                        &mut values[0],
                        -1.0..=1.0,
                        "cyan to red",
                        "shift this range towards red or cyan",
                    );
                    slide(
                        ui,
                        touch,
                        &mut values[1],
                        -1.0..=1.0,
                        "magenta to green",
                        "shift this range towards green or magenta",
                    );
                    slide(
                        ui,
                        touch,
                        &mut values[2],
                        -1.0..=1.0,
                        "yellow to blue",
                        "shift this range towards blue or yellow",
                    );
                });
            });
            check(
                ui,
                touch,
                &mut b.keep_light,
                "keep the light",
                "shift the colours without making pixels lighter or darker",
            );
        }
        Adjustment::GradientMap(g) => stops_controls(ui, &mut g.stops, touch, current),
        Adjustment::Fill(f) => match &mut f.paint {
            Paint::Solid(hex) => {
                ui.horizontal(|ui| {
                    swatch(ui, touch, hex, current, "the fill's colour");
                    ui.label("blend and opacity are the layer's, above");
                });
            }
            _ => {
                ui.label("a gradient fill; change it with klad set");
            }
        },
    }
}

impl Editor {
    pub(super) fn rays_section(&mut self, ui: &mut egui::Ui, id: &str, rays: &Rays) -> Option<Value> {
        let mut r = rays.clone();
        let fields: [(&str, &mut f32, std::ops::RangeInclusive<f32>, f64, &str); 8] = [
            ("x ", &mut r.x, -1e5..=1e5, 1.0, "where the light comes from, across"),
            ("y ", &mut r.y, -1e5..=1e5, 1.0, "where the light comes from, down"),
            (
                "angle ",
                &mut r.angle,
                -360.0..=360.0,
                0.5,
                "the direction they shine, in degrees: 90 is straight down",
            ),
            (
                "spread ",
                &mut r.spread,
                0.0..=360.0,
                0.5,
                "how wide the fan of rays is, in degrees",
            ),
            ("length ", &mut r.length, 1.0..=1e5, 2.0, "how far they reach, in px"),
            (
                "gap ",
                &mut r.start,
                0.0..=1e5,
                1.0,
                "how wide the gap they shine through is, in px",
            ),
            (
                "falloff ",
                &mut r.falloff,
                0.05..=8.0,
                0.02,
                "how quickly they fade along their length",
            ),
            (
                "noise ",
                &mut r.noise,
                0.0..=1.0,
                0.01,
                "how much the rays differ in width, length and strength",
            ),
        ];
        let mut changed = false;
        ui.label(RichText::new(format!("{} rays", theme::icon::SUN)).strong());
        fields
            .into_iter()
            .enumerate()
            .for_each(|(i, (label, value, range, speed, hint))| {
                if i % 2 == 0 {
                    ui.end_row();
                }
                if let Some(v) = self.number(ui, &format!("{id}.rays.{label}"), label, *value, range, speed, hint) {
                    *value = v;
                    changed = true;
                }
            });
        let mut count = r.count as f32;
        if let Some(v) = self.number(
            ui,
            &format!("{id}.rays.count"),
            "count ",
            count,
            1.0..=256.0,
            0.1,
            "how many rays",
        ) {
            count = v;
            r.count = count.round() as u32;
            changed = true;
        }
        let mut seed = r.seed as f32;
        if let Some(v) = self.number(
            ui,
            &format!("{id}.rays.seed"),
            "seed ",
            seed,
            0.0..=99_999.0,
            0.2,
            "another seed gives other rays of the same kind",
        ) {
            seed = v;
            r.seed = seed.round() as u32;
            changed = true;
        }
        changed.then(|| serde_json::to_value(&r).unwrap_or(Value::Null))
    }

    pub(super) fn preview_look(&mut self, layer: Layer) -> Result<()> {
        if self.look.draft.as_ref().is_some_and(|d| d.id != layer.id) {
            self.end_look_preview()?;
        }
        if self.look.draft.is_none() {
            let Some(original) = self.scene.bundle.doc.find_layer(&layer.id).ok().cloned() else {
                return Ok(());
            };
            self.look.draft = Some(Draft {
                id: layer.id.clone(),
                original,
            });
        }
        self.scene.preview_layer(layer)?;
        self.dirty.extend(self.scene.all_tiles());
        Ok(())
    }

    pub(super) fn end_look_preview(&mut self) -> Result<()> {
        if let Some(draft) = self.look.draft.take() {
            self.scene.preview_layer(draft.original)?;
            self.dirty.extend(self.scene.all_tiles());
        }
        Ok(())
    }

    fn settle_look(&mut self, layer: &Layer, touch: Touch, key: &str, value: Value) -> Result<()> {
        match touch {
            Touch::Still => Ok(()),
            Touch::Preview => self.preview_look(layer.clone()),
            Touch::Commit => {
                self.end_look_preview()?;
                self.set(&layer.id, vec![(key.to_string(), value)])
            }
        }
    }

    pub(super) fn effects_section(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        if layer.reads_under() {
            return Ok(());
        }
        ui.separator();
        ui.label(RichText::new(format!("{} effects", theme::icon::SPARKLE)).strong())
            .hint("effects apply in order, top first, each time the layer composes; they stay editable");
        let mut list = layer.effects.clone();
        let mut touch = Touch::Still;
        let current = self.color;
        let height = self.scene.bundle.doc.canvas.height as f32;
        let layers: Vec<(String, String)> = self
            .scene
            .bundle
            .doc
            .all_layers()
            .into_iter()
            .filter(|l| l.id != layer.id)
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let mut order: Option<(usize, usize)> = None;
        let mut gone = None;
        let count = list.len();
        list.iter_mut().enumerate().for_each(|(i, effect)| {
            ui.push_id(("effect", i), |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(effect.name()).color(theme::TEXT));
                    if i > 0 && ui.button(theme::icon::UP).hint("apply this effect earlier").clicked() {
                        order = Some((i, i - 1));
                    }
                    if i + 1 < count && ui.button(theme::icon::DOWN).hint("apply this effect later").clicked() {
                        order = Some((i, i + 1));
                    }
                    if ui.button(theme::icon::TRASH).hint("remove this effect").clicked() {
                        gone = Some(i);
                    }
                });
                effect_controls(ui, effect, &mut touch, current, &layers, height);
            });
        });
        if let Some((a, b)) = order {
            list.swap(a, b);
            touch.add(Touch::Commit);
        }
        if let Some(i) = gone {
            list.remove(i);
            touch.add(Touch::Commit);
        }
        let mut added = None;
        egui::ComboBox::from_id_salt(format!("{}.effect.add", layer.id))
            .width(120.0)
            .height(EFFECTS.len() as f32 * (theme::TARGET + 6.0))
            .selected_text(format!("{} add an effect", theme::icon::PLUS))
            .show_ui(ui, |ui| {
                EFFECTS.iter().for_each(|name| {
                    if ui
                        .selectable_label(false, *name)
                        .hint(format!("add a {name} effect at the end"))
                        .clicked()
                    {
                        added = Effect::named(name);
                    }
                });
            })
            .response
            .hint("blur, glow, outline, shadow, grain, displacement, haze or relief, applied after the ones above");
        if let Some(effect) = added {
            list.push(effect);
            touch.add(Touch::Commit);
        }
        if touch == Touch::Still {
            return Ok(());
        }
        if let Some(e) = list.iter().find_map(|e| e.validate().err()) {
            self.say(Tone::Refused, format!("{e:#}"));
            return Ok(());
        }
        let value = serde_json::to_value(&list)?;
        let changed = Layer {
            effects: list,
            ..layer.clone()
        };
        self.settle_look(&changed, touch, "effects", value)
    }

    pub(super) fn adjust_section(&mut self, ui: &mut egui::Ui, layer: &Layer) -> Result<()> {
        let Some(mut adjust) = layer.adjustment().cloned() else {
            return Ok(());
        };
        ui.separator();
        let mut touch = Touch::Still;
        let before = adjust.name();
        let mut kind = before;
        egui::ComboBox::from_id_salt(format!("{}.adjust.kind", layer.id))
            .width(150.0)
            .selected_text(adjust.label())
            .show_ui(ui, |ui| {
                ADJUSTMENTS.iter().for_each(|name| {
                    ui.selectable_value(&mut kind, name, look::label_of(name));
                });
            })
            .response
            .hint("what this adjustment layer does to the layers under it in its group (or the one it is clipped to)");
        if kind != before
            && let Some(fresh) = Adjustment::named(kind)
        {
            adjust = fresh;
            touch.add(Touch::Commit);
        }
        let current = self.color;
        let mut state = std::mem::take(&mut self.look);
        adjust_controls(ui, &mut adjust, &mut state, &mut touch, current);
        self.look = state;
        if touch == Touch::Still {
            return Ok(());
        }
        if let Err(e) = adjust.validate() {
            self.say(Tone::Refused, format!("{e:#}"));
            return Ok(());
        }
        let value = serde_json::to_value(&adjust)?;
        let changed = Layer {
            content: LayerContent::Adjust { adjust },
            ..layer.clone()
        };
        self.settle_look(&changed, touch, "adjust", value)
    }
}
