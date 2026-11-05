use crate::brush::{Angle, Brush, BrushSet, StrokeBlend, Texture, Tip, smoothstep, speed_amount, tilt_amount};
use crate::cut;
use crate::tiles::{Mode, Raster, StrokeBuffer, TILE, Tiles};
use anyhow::Result;
use half::f16;
use klad_core::brushes::PaintDef;
use klad_core::color::Rgba;
use klad_core::cut::Cut;
use klad_core::rng::{Rng, hash2_unit};
use klad_core::stroke::{Mark, Sample, Stroke};

const MIN_RADIUS: f32 = 0.6;
const SPEED_EASE: f32 = 0.5;
const SQUASH: f32 = 0.85;
const DEEPEST: f32 = 32.0;

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
    p: f32,
    tilt: (f32, f32),
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
    Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        p: a.p + (b.p - a.p) * t,
        tilt: (
            a.tilt.0 + (b.tilt.0 - a.tilt.0) * t,
            a.tilt.1 + (b.tilt.1 - a.tilt.1) * t,
        ),
    }
}

fn segment(from: (f32, f32), to: (f32, f32)) -> f32 {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    libm::sqrtf(dx * dx + dy * dy)
}

pub fn arc_length(samples: &[Sample]) -> f32 {
    samples
        .iter()
        .map(Sample::pos)
        .fold((None, 0.0f32), |(last, total), here| match last {
            None => (Some(here), total),
            Some(prev) => {
                let length = segment(prev, here);
                if length < 1e-4 {
                    (Some(prev), total)
                } else {
                    (Some(here), total + length)
                }
            }
        })
        .1
}

pub fn needs_repaint(stroke: &Stroke) -> bool {
    stroke.taper != [0.0, 0.0] || stroke.pressureless()
}

pub struct StrokePainter {
    brush: Brush,
    size: f32,
    flow: f32,
    rng: Rng,
    seed: u32,
    last: Option<Point>,
    heading: f32,
    speed: f32,
    carry: f32,
    walked: f32,
    taper: [f32; 2],
    length: Option<f32>,
    pressureless: bool,
    dabs: usize,
    first: [f32; 3],
    second: [f32; 3],
    shift: [f32; 3],
    reservoir: Option<[f32; 3]>,
    reservoir_height: Option<f32>,
    previous: Option<(f32, f32)>,
    bristles: Option<Box<Bristles>>,
}

const STROKE_COLOUR_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

impl StrokePainter {
    pub fn new(brush: &Brush, stroke: &Stroke) -> Self {
        let length = (stroke.taper != [0.0, 0.0]).then(|| arc_length(&stroke.samples));
        let mut painter = Self::live(brush, stroke);
        painter.length = length;
        painter.pressureless = stroke.pressureless();
        if length.is_some_and(|l| l <= 0.0) {
            painter.taper = [0.0, 0.0];
        }
        painter
    }

    pub fn live(brush: &Brush, stroke: &Stroke) -> Self {
        let colour = stroke.color();
        let first = [colour[0], colour[1], colour[2]];
        let second = stroke.second_color().map_or(first, |c| [c[0], c[1], c[2]]);
        let shift = match &brush.def.color {
            Some(c) => {
                let mut rng = Rng::new(stroke.seed as u64 ^ STROKE_COLOUR_SEED);
                [
                    c.stroke_hue / 360.0 * rng.signed(),
                    c.stroke_saturation * rng.signed(),
                    c.stroke_value * rng.signed(),
                ]
            }
            None => [0.0; 3],
        };
        Self {
            brush: brush.clone(),
            size: stroke.size,
            flow: stroke.flow,
            rng: Rng::new(stroke.seed as u64),
            seed: stroke.seed,
            last: None,
            heading: 0.0,
            speed: 0.0,
            carry: 0.0,
            walked: 0.0,
            taper: stroke.taper,
            length: None,
            pressureless: false,
            dabs: 0,
            first,
            second,
            shift,
            reservoir: None,
            reservoir_height: None,
            previous: None,
            bristles: (brush.def.body > 0.0)
                .then(|| Box::new(Bristles::new(stroke.seed ^ 0x5bd1_e995, stroke.size, brush.scale))),
        }
    }

    pub fn dabs(&self) -> usize {
        self.dabs
    }

    pub fn mode(&self) -> Mode {
        mode_of(&self.brush)
    }

    pub fn push(&mut self, sample: &Sample, buffer: &mut StrokeBuffer) {
        self.push_on(sample, buffer, None);
    }

    pub fn push_on(&mut self, sample: &Sample, buffer: &mut StrokeBuffer, under: Option<&Raster>) {
        buffer.mode = self.mode();
        buffer.body = self.brush.def.body;
        buffer.lifting = buffer.mode != Mode::Flat && under.is_some_and(Raster::has_height);
        let (x, y) = sample.pos();
        let next = Point {
            x,
            y,
            p: if self.pressureless { 1.0 } else { sample.pressure() },
            tilt: sample.tilt(),
        };
        let Some(prev) = self.last else {
            self.dab(next, 0.0, buffer, under);
            self.last = Some(next);
            return;
        };
        let length = segment((prev.x, prev.y), (next.x, next.y));
        if length < 1e-4 {
            self.last = Some(Point {
                p: next.p,
                tilt: next.tilt,
                ..prev
            });
            return;
        }
        if sample.dt_ms > 0 {
            let now = length / sample.dt_ms as f32 / self.brush.scale;
            self.speed += (now - self.speed) * SPEED_EASE;
        }
        self.heading = libm::atan2f(next.y - prev.y, next.x - prev.x);
        let mut travelled = 0.0f32;
        loop {
            let here = lerp(prev, next, travelled / length);
            let step = (self.spacing(here, self.walked + travelled) - self.carry).max(0.0);
            if travelled + step > length {
                self.carry += length - travelled;
                break;
            }
            travelled += step;
            self.carry = 0.0;
            self.dab(
                lerp(prev, next, travelled / length),
                self.walked + travelled,
                buffer,
                under,
            );
        }
        self.walked += length;
        self.last = Some(next);
    }

    fn taper_at(&self, along: f32) -> f32 {
        let [into, out] = self.taper;
        let k_in = if into > 0.0 {
            smoothstep(0.0, 1.0, along / into)
        } else {
            1.0
        };
        let k_out = match self.length {
            Some(total) if out > 0.0 => smoothstep(0.0, 1.0, (total - along) / out),
            _ => 1.0,
        };
        k_in.min(k_out)
    }

    fn size_here(&self, at: Point, along: f32) -> f32 {
        let size = self
            .brush
            .size_at(self.size, at.p, tilt_amount(at.tilt), speed_amount(self.speed));
        size * self.taper_at(along)
    }

    fn spacing(&self, at: Point, along: f32) -> f32 {
        (self.size_here(at, along) * self.brush.def.spacing).max(0.5)
    }

    fn colour_here(&mut self, at: Point, along: f32) -> Option<[f32; 3]> {
        let c = self.brush.def.color?;
        let towards_second = {
            let pressure = c.pressure.map_or(0.0, |curve| 1.0 - curve.at(at.p).clamp(0.0, 1.0));
            let along = match c.along > 0.0 {
                true => (along / c.along).min(1.0),
                false => 0.0,
            };
            pressure.max(along)
        };
        let base: [f32; 3] = std::array::from_fn(|i| self.first[i] + (self.second[i] - self.first[i]) * towards_second);
        let jitter = match c.hue > 0.0 || c.saturation > 0.0 || c.value > 0.0 {
            true => [
                c.hue / 360.0 * self.rng.signed(),
                c.saturation * self.rng.signed(),
                c.value * self.rng.signed(),
            ],
            false => [0.0; 3],
        };
        let [h, s, v] = hsv(base);
        Some(rgb([
            (h + self.shift[0] + jitter[0]).rem_euclid(1.0),
            (s + self.shift[1] + jitter[1]).clamp(0.0, 1.0),
            (v + self.shift[2] + jitter[2]).clamp(0.0, 1.0),
        ]))
    }

    fn dab(&mut self, at: Point, along: f32, buffer: &mut StrokeBuffer, under: Option<&Raster>) {
        let jitter = [
            self.rng.signed(),
            self.rng.signed(),
            self.rng.signed(),
            self.rng.signed(),
        ];
        let (tilt, speed) = (tilt_amount(at.tilt), speed_amount(self.speed));
        let size = self.size_here(at, along) * (1.0 + self.brush.def.jitter.size * jitter[0]);
        let flow = self.brush.flow_at(self.flow, at.p, tilt, speed);
        let cap = self.brush.opacity_at(at.p, tilt, speed);
        let angle = match self.brush.angle {
            Angle::Fixed(a) => a,
            Angle::Follow => self.heading,
            Angle::Tilt => libm::atan2f(at.tilt.1, at.tilt.0),
        } + self.brush.def.jitter.angle * jitter[1];
        let cx = at.x + self.brush.def.jitter.scatter * size * jitter[2];
        let cy = at.y + self.brush.def.jitter.scatter * size * jitter[3];
        let mut dual = [[0.0f32; 3]; 8];
        let count = self.brush.dual.as_ref().map_or(0, |d| d.count.min(8) as usize);
        let scatter = self.brush.dual.as_ref().map_or(0.0, |d| d.scatter);
        dual.iter_mut().take(count).for_each(|d| {
            *d = [
                scatter * self.rng.signed(),
                scatter * self.rng.signed(),
                std::f32::consts::PI * self.rng.signed(),
            ]
        });
        let mut color = self.colour_here(at, along);
        let mut flow = flow;
        let mut lift = None;
        if let Some(PaintDef::Wet {
            wetness,
            pickup,
            dilution,
        }) = self.brush.def.paint
        {
            let base = color.unwrap_or(self.first);
            let picked = pick(buffer, under, cx, cy, size * 0.5);
            let reservoir = self.reservoir.get_or_insert(base);
            if picked[3] > 1e-4 {
                let k = pickup * picked[3];
                (0..3).for_each(|i| reservoir[i] += (picked[i] / picked[3] - reservoir[i]) * k);
            }
            color = Some(std::array::from_fn(|i| base[i] + (reservoir[i] - base[i]) * wetness));
            flow *= 1.0 - dilution * 0.9;
            if buffer.lifting {
                let picked_height = pick_height(buffer, under, cx, cy, size * 0.5);
                let held = self.reservoir_height.get_or_insert(picked_height);
                *held += (picked_height - *held) * pickup * picked[3].min(1.0);
                lift = Some(picked_height + (*held - picked_height) * wetness);
            }
        }
        let shift = self.previous.map_or((0.0, 0.0), |(px, py)| (cx - px, cy - py));
        self.previous = Some((cx, cy));
        self.dabs += 1;
        let dab = Dab {
            cx,
            cy,
            radius: size * 0.5,
            angle,
            flow,
            cap,
            p: at.p,
            dual,
            count,
            color,
            lift,
            pick: klad_core::rng::hash2(self.dabs as i32, self.seed as i32, 0x1eaf) as usize,
        };
        match self.brush.def.paint {
            Some(PaintDef::Smudge { strength }) => {
                stamp_replace(buffer, &self.brush, dab, under, Replace::Smudge(shift), strength)
            }
            Some(PaintDef::Blend { strength }) => {
                stamp_replace(buffer, &self.brush, dab, under, Replace::Blend, strength)
            }
            _ => {
                let bristles = self.bristles.as_deref().map(|b| (b, b.load(along)));
                stamp(buffer, &self.brush, dab, bristles)
            }
        }
    }
}

pub fn mode_of(brush: &Brush) -> Mode {
    match brush.def.paint {
        Some(PaintDef::Smudge { .. } | PaintDef::Blend { .. }) => Mode::Replace,
        Some(PaintDef::Wet { .. }) => Mode::Colour,
        None if brush.def.color.is_some() => Mode::Colour,
        None => Mode::Flat,
    }
}

fn hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = match d <= 1e-6 {
        true => 0.0,
        false if max == r => ((g - b) / d).rem_euclid(6.0) / 6.0,
        false if max == g => ((b - r) / d + 2.0) / 6.0,
        false => ((r - g) / d + 4.0) / 6.0,
    };
    let s = if max <= 1e-6 { 0.0 } else { d / max };
    [h, s, max]
}

fn rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h6 = h * 6.0;
    let sector = (h6.floor() as i32).rem_euclid(6);
    let f = h6 - h6.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn state_at(buffer: &StrokeBuffer, under: Option<&Raster>, x: i64, y: i64) -> [f32; 4] {
    let grid = buffer.grid();
    if x < 0 || y < 0 || x >= grid.width as i64 || y >= grid.height as i64 {
        return [0.0; 4];
    }
    let (x, y) = (x as usize, y as usize);
    let index = (y / TILE) * grid.cols + x / TILE;
    let px = (y % TILE) * TILE + x % TILE;
    let replaced = buffer.mode == Mode::Replace && buffer.coverage(index).is_some_and(|c| c[px] > 0.0);
    match (replaced, buffer.paint(index)) {
        (true, Some(target)) => [
            target[px * 4],
            target[px * 4 + 1],
            target[px * 4 + 2],
            target[px * 4 + 3],
        ],
        _ => under
            .and_then(|r| r.at(index))
            .map_or([0.0; 4], |t| std::array::from_fn(|c| t[px * 4 + c].to_f32())),
    }
}

fn height_at(buffer: &StrokeBuffer, under: Option<&Raster>, x: i64, y: i64) -> f32 {
    let grid = buffer.grid();
    if x < 0 || y < 0 || x >= grid.width as i64 || y >= grid.height as i64 {
        return 0.0;
    }
    let (x, y) = (x as usize, y as usize);
    let index = (y / TILE) * grid.cols + x / TILE;
    let px = (y % TILE) * TILE + x % TILE;
    let replaced = buffer.mode == Mode::Replace && buffer.coverage(index).is_some_and(|c| c[px] > 0.0);
    match (replaced, buffer.lift(index)) {
        (true, Some(lift)) => lift[px],
        _ => under.and_then(|r| r.height_tile(index)).map_or(0.0, |t| t[px].to_f32()),
    }
}

fn pick_height(buffer: &StrokeBuffer, under: Option<&Raster>, cx: f32, cy: f32, radius: f32) -> f32 {
    let r = radius * 0.4;
    let points = [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)];
    points
        .iter()
        .map(|(dx, dy)| height_at(buffer, under, (cx + dx).floor() as i64, (cy + dy).floor() as i64))
        .sum::<f32>()
        / points.len() as f32
}

fn pick(buffer: &StrokeBuffer, under: Option<&Raster>, cx: f32, cy: f32, radius: f32) -> [f32; 4] {
    let r = radius * 0.4;
    let points = [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)];
    let sum = points.iter().fold([0.0f32; 4], |acc, (dx, dy)| {
        let s = state_at(buffer, under, (cx + dx).floor() as i64, (cy + dy).floor() as i64);
        std::array::from_fn(|i| acc[i] + s[i])
    });
    sum.map(|v| v / points.len() as f32)
}

struct Dab {
    cx: f32,
    cy: f32,
    radius: f32,
    angle: f32,
    flow: f32,
    cap: f32,
    p: f32,
    dual: [[f32; 3]; 8],
    count: usize,
    color: Option<[f32; 3]>,
    lift: Option<f32>,
    pick: usize,
}

const GROOVES: usize = 256;
const RUN: f32 = 8.0;
const EDGE: f32 = 2.5;
const PROFILE: usize = 128;

struct Profile {
    lips: [f32; PROFILE + 1],
    caps: [f32; PROFILE + 1],
    depth: f32,
    spread: f32,
}

struct Bristles {
    grooves: [f32; GROOVES + 1],
    run: f32,
    tall: f32,
    scale: f32,
    radius: f32,
}

impl Bristles {
    fn new(seed: u32, size: f32, scale: f32) -> Self {
        let count = (size / scale / 3.5).clamp(3.0, 48.0);
        let wave = |x: f32, row: i32| {
            let i = libm::floorf(x);
            let f = x - i;
            let t = f * f * (3.0 - 2.0 * f);
            let at = |k: f32| hash2_unit(k as i32, row, seed);
            at(i) + (at(i + 1.0) - at(i)) * t
        };
        Self {
            grooves: std::array::from_fn(|k| {
                let x = k as f32 / GROOVES as f32 * count;
                wave(x, 0) * 0.65 + wave(x * 2.3, 1) * 0.35
            }),
            run: size * RUN,
            tall: libm::sqrtf(size / scale / 24.0).clamp(0.4, 1.6),
            scale,
            radius: size * 0.5,
        }
    }

    fn profile(&self, radius: f32, roundness: f32, load: f32, p: f32) -> Profile {
        let band = (EDGE * self.scale / (radius * roundness).max(1e-3)).clamp(0.35, 0.6);
        let k = load * self.tall * (0.3 + 0.7 * p.clamp(0.0, 1.0));
        let at = |i: usize| i as f32 / PROFILE as f32;
        Profile {
            lips: std::array::from_fn(|i| {
                let across = at(i);
                k * (0.72 + 0.43 * smoothstep(1.0 - 3.0 * band, 1.0 - band, across))
                    * (1.0 - smoothstep(1.0 - band, 1.0, across))
            }),
            caps: std::array::from_fn(|i| 1.0 - smoothstep(1.0 - band, 1.0, at(i))),
            depth: 0.3 + 0.3 * ((1.0 - load) / 0.45).clamp(0.0, 1.0),
            spread: radius / self.radius,
        }
    }

    fn load(&self, along: f32) -> f32 {
        1.0 - 0.45 * smoothstep(0.0, self.run.max(1.0), along)
    }

    fn thickness(&self, profile: &Profile, d: f32, v: f32) -> f32 {
        let lateral = (v * profile.spread).clamp(-1.0, 1.0);
        let wave = self.grooves[((lateral * 0.5 + 0.5) * GROOVES as f32) as usize];
        let groove = 1.0 - profile.depth * (1.0 - wave);
        lookup(&profile.lips, v.abs()) * lookup(&profile.caps, d) * groove
    }
}

fn lookup(table: &[f32; PROFILE + 1], at: f32) -> f32 {
    let x = at.clamp(0.0, 1.0) * PROFILE as f32;
    let i = (x as usize).min(PROFILE - 1);
    let f = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

const RING: [(f32, f32); 8] = [
    (1.0, 0.0),
    (0.707, 0.707),
    (0.0, 1.0),
    (-0.707, 0.707),
    (-1.0, 0.0),
    (-0.707, -0.707),
    (0.0, -1.0),
    (0.707, -0.707),
];

enum Replace {
    Smudge((f32, f32)),
    Blend,
}

fn tip_value(tip: &Tip, u: f32, v: f32, d2: f32, radius: f32, pick: usize) -> f32 {
    match tip {
        Tip::Round { hardness } => {
            let d = libm::sqrtf(d2);
            let edge = ((1.0 - d) * radius + 0.5).clamp(0.0, 1.0);
            let t = ((d - hardness) / (1.0 - hardness).max(1e-4)).clamp(0.0, 1.0);
            (1.0 - t * t * (3.0 - 2.0 * t)).min(edge)
        }
        Tip::Mask(mask) => mask.sample(u, v),
        Tip::Masks(masks) => masks[pick % masks.len()].sample(u, v),
    }
}

struct Edge {
    solid: f32,
    fade: f32,
    soft: f32,
}

impl Edge {
    fn of(tip: &Tip, radius: f32) -> Option<Edge> {
        match tip {
            Tip::Round { hardness } => {
                let solid = hardness * radius;
                let fade = (radius - solid).max(1e-4);
                Some(Edge {
                    solid,
                    fade,
                    soft: smoothstep(0.5, 2.0, fade),
                })
            }
            Tip::Mask(_) | Tip::Masks(_) => None,
        }
    }

    fn cover(&self, reach: f32) -> (f32, f32) {
        if reach + 0.5 <= self.solid {
            return (1.0, 1.0);
        }
        if reach - 0.5 >= self.solid + self.fade {
            return (0.0, self.soft);
        }
        let ramp = |t: f32| match t {
            t if t <= 0.0 => 0.0,
            t if t >= 1.0 => t - 0.5,
            t => t * t * t * (1.0 - 0.5 * t),
        };
        let seen =
            self.fade * (ramp((reach + 0.5 - self.solid) / self.fade) - ramp((reach - 0.5 - self.solid) / self.fade));
        let cover = (1.0 - seen).clamp(0.0, 1.0);
        (cover, cover + (1.0 - cover) * self.soft)
    }
}

fn dual_value(brush: &Brush, dab: &Dab, u: f32, v: f32, radius: f32) -> f32 {
    let Some(dual) = brush.dual.as_ref() else {
        return 1.0;
    };
    dab.dual.iter().take(dab.count).fold(0.0f32, |most, [dx, dy, turn]| {
        let (su, sv) = ((u - dx) / dual.size, (v - dy) / dual.size);
        let (sin, cos) = (libm::sinf(*turn), libm::cosf(*turn));
        let (ru, rv) = (su * cos + sv * sin, sv * cos - su * sin);
        let d2 = ru * ru + rv * rv;
        match d2 >= 1.0 {
            true => most,
            false => most.max(tip_value(&dual.tip, ru, rv, d2, radius * dual.size, 0)),
        }
    })
}

fn texture_value(texture: &Texture, x: usize, y: usize, pressure: f32) -> f32 {
    let g = match texture.scale == 1.0 {
        true => texture.grain.at(x, y),
        false => texture
            .grain
            .at((x as f32 / texture.scale) as usize, (y as f32 / texture.scale) as usize),
    };
    let g = match texture.contrast == 1.0 {
        true => g,
        false => ((g - 0.5) * texture.contrast + 0.5).clamp(0.0, 1.0),
    };
    let strength = match texture.pressure == 0.0 {
        true => texture.strength,
        false => texture.strength * (1.0 - texture.pressure * pressure),
    };
    strength * (1.0 - g)
}

struct Area {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    radius: f32,
    flow: f32,
    reach2: f32,
}

fn area(buffer: &StrokeBuffer, dab: &Dab) -> Option<Area> {
    let (radius, flow) = if dab.radius < MIN_RADIUS {
        let k = dab.radius / MIN_RADIUS;
        (MIN_RADIUS, dab.flow * k * k)
    } else {
        (dab.radius, dab.flow)
    };
    let reach = radius + 1.0;
    let reach2 = (reach / radius) * (reach / radius);
    let (width, height) = (buffer.grid().width as f32, buffer.grid().height as f32);
    let x0 = (dab.cx - reach).floor().max(0.0) as usize;
    let y0 = (dab.cy - reach).floor().max(0.0) as usize;
    let x1 = (dab.cx + reach).ceil().clamp(0.0, width) as usize;
    let y1 = (dab.cy + reach).ceil().clamp(0.0, height) as usize;
    (x0 < x1 && y0 < y1).then_some(Area {
        x0,
        y0,
        x1,
        y1,
        radius,
        flow,
        reach2,
    })
}

fn stamp(buffer: &mut StrokeBuffer, brush: &Brush, dab: Dab, bristles: Option<(&Bristles, f32)>) {
    let Some(Area {
        x0,
        y0,
        x1,
        y1,
        radius,
        flow,
        reach2,
    }) = area(buffer, &dab)
    else {
        return;
    };
    let (cx, cy, cap) = (dab.cx, dab.cy, dab.cap);
    let (sin, cos) = (libm::sinf(dab.angle), libm::cosf(dab.angle));
    let inv_radius = 1.0 / radius;
    let inv_round = 1.0 / brush.def.roundness;
    let cols = buffer.grid().cols;
    let edge = Edge::of(&brush.tip, radius);
    let profile = bristles.map(|(b, load)| (b, b.profile(radius, brush.def.roundness, load, dab.p)));
    for ty in y0 / TILE..=(y1 - 1) / TILE {
        for tx in x0 / TILE..=(x1 - 1) / TILE {
            if buffer.skip(ty * cols + tx) {
                continue;
            }
            let Tiles {
                coverage: tile,
                mut paint,
                mut lift,
                mut ridge,
            } = buffer.tiles_mut(ty * cols + tx);
            let (ox, oy) = (tx * TILE, ty * TILE);
            for y in y0.max(oy)..y1.min(oy + TILE) {
                let py = y as f32 + 0.5 - cy;
                let row = (y - oy) * TILE;
                for x in x0.max(ox)..x1.min(ox + TILE) {
                    let px = x as f32 + 0.5 - cx;
                    let u = (px * cos + py * sin) * inv_radius;
                    let v = (py * cos - px * sin) * inv_round * inv_radius;
                    let d2 = u * u + v * v;
                    if d2 >= reach2 {
                        continue;
                    }
                    let (cover, reach) = match &edge {
                        Some(edge) => edge.cover(libm::sqrtf(d2) * radius),
                        None => (tip_value(&brush.tip, u, v, d2, radius, dab.pick), 1.0),
                    };
                    let mut a = cover * flow;
                    if dab.count > 0 {
                        a *= dual_value(brush, &dab, u, v, radius);
                    }
                    if let Some(texture) = &brush.grain {
                        a = (a - texture_value(texture, x, y, dab.p)).max(0.0);
                    }
                    if let (Some(ridge), Some((bristles, profile))) = (ridge.as_deref_mut(), &profile)
                        && a > 0.0
                    {
                        let r = &mut ridge[row + x - ox];
                        *r = r.max(bristles.thickness(profile, libm::sqrtf(d2), v));
                    }
                    if a > 0.0 && reach > 0.0 {
                        let at = row + x - ox;
                        let c = &mut tile[at];
                        let rate = match reach < 1.0 {
                            true => (a / reach).min(1.0),
                            false => a.min(1.0),
                        };
                        let gain = (cap * reach - *c) * rate;
                        if gain > 0.0 {
                            *c += gain;
                            if let (Some(paint), Some(colour)) = (paint.as_deref_mut(), dab.color)
                                && gain > 0.0
                            {
                                let w = &mut paint[at * 4..at * 4 + 4];
                                (0..3).for_each(|i| w[i] += colour[i] * gain);
                                w[3] += gain;
                            }
                            if let (Some(lift), Some(height)) = (lift.as_deref_mut(), dab.lift) {
                                lift[at] += height * gain;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn stamp_replace(
    buffer: &mut StrokeBuffer,
    brush: &Brush,
    dab: Dab,
    under: Option<&Raster>,
    how: Replace,
    strength: f32,
) {
    let Some(Area {
        x0,
        y0,
        x1,
        y1,
        radius,
        flow,
        reach2,
    }) = area(buffer, &dab)
    else {
        return;
    };
    if let Replace::Smudge((0.0, 0.0)) = how {
        return;
    }
    let (sin, cos) = (libm::sinf(dab.angle), libm::cosf(dab.angle));
    let (inv_radius, inv_round) = (1.0 / radius, 1.0 / brush.def.roundness);
    let cols = buffer.grid().cols;
    let lifting = buffer.lifting;
    let changes: Vec<(usize, usize, [f32; 4], f32)> = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| buffer.allowed((y / TILE) * cols + x / TILE))
        .filter_map(|(x, y)| {
            let (px, py) = (x as f32 + 0.5 - dab.cx, y as f32 + 0.5 - dab.cy);
            let u = (px * cos + py * sin) * inv_radius;
            let v = (py * cos - px * sin) * inv_round * inv_radius;
            let d2 = u * u + v * v;
            if d2 >= reach2 {
                return None;
            }
            let mut a = tip_value(&brush.tip, u, v, d2, radius, dab.pick) * flow;
            if dab.count > 0 {
                a *= dual_value(brush, &dab, u, v, radius);
            }
            if let Some(texture) = &brush.grain {
                a = (a - texture_value(texture, x, y, dab.p)).max(0.0);
            }
            let a = (a * strength * dab.cap).min(1.0);
            if a <= 0.0 {
                return None;
            }
            let state = |x: i64, y: i64| {
                let s = state_at(buffer, under, x, y);
                let h = match lifting {
                    true => height_at(buffer, under, x, y),
                    false => 0.0,
                };
                [s[0], s[1], s[2], s[3], h]
            };
            let here = state(x as i64, y as i64);
            let from = match how {
                Replace::Smudge((dx, dy)) => state((x as f32 - dx).round() as i64, (y as f32 - dy).round() as i64),
                Replace::Blend => {
                    let reach = (radius * 0.3).max(1.0);
                    let sum = RING.iter().fold(here, |acc, (dx, dy)| {
                        let s = state(
                            (x as f32 + dx * reach).round() as i64,
                            (y as f32 + dy * reach).round() as i64,
                        );
                        std::array::from_fn(|c| acc[c] + s[c])
                    });
                    sum.map(|v| v / (RING.len() + 1) as f32)
                }
            };
            let next: [f32; 5] = std::array::from_fn(|c| here[c] + (from[c] - here[c]) * a);
            Some((x, y, [next[0], next[1], next[2], next[3]], next[4]))
        })
        .collect();
    changes.into_iter().for_each(|(x, y, value, height)| {
        let index = (y / TILE) * cols + x / TILE;
        let at = (y % TILE) * TILE + x % TILE;
        let tiles = buffer.tiles_mut(index);
        tiles.coverage[at] = 1.0;
        if let Some(paint) = tiles.paint {
            paint[at * 4..at * 4 + 4].copy_from_slice(&value);
        }
        if let Some(lift) = tiles.lift {
            lift[at] = height;
        }
        if let Some(ridge) = tiles.ridge {
            ridge[at] = 1.0;
        }
    });
}

pub fn blend_coverage(target: &mut [f16], coverage: &[f32], color: Rgba, opacity: f32, blend: StrokeBlend) {
    let alpha = (opacity * color[3]).clamp(0.0, 1.0);
    coverage
        .iter()
        .zip(target.as_chunks_mut::<4>().0)
        .filter(|(c, _)| **c > 0.0)
        .for_each(|(&c, px)| {
            let a = c * alpha;
            let keep = 1.0 - a;
            match blend {
                StrokeBlend::Normal => {
                    (0..3).for_each(|i| px[i] = f16::from_f32(color[i] * a + px[i].to_f32() * keep));
                    px[3] = f16::from_f32(a + px[3].to_f32() * keep);
                }
                StrokeBlend::Erase => px.iter_mut().for_each(|v| *v = f16::from_f32(v.to_f32() * keep)),
                StrokeBlend::Atop => {
                    let under = px[3].to_f32();
                    (0..3).for_each(|i| px[i] = f16::from_f32(color[i] * a * under + px[i].to_f32() * keep));
                }
            }
        });
}

fn blend_colours(target: &mut [f16], coverage: &[f32], paint: &[f32], color: Rgba, opacity: f32, blend: StrokeBlend) {
    let alpha = (opacity * color[3]).clamp(0.0, 1.0);
    coverage
        .iter()
        .zip(paint.as_chunks::<4>().0)
        .zip(target.as_chunks_mut::<4>().0)
        .filter(|((c, _), _)| **c > 0.0)
        .for_each(|((&c, w), px)| {
            let colour: [f32; 3] = match w[3] > 0.0 {
                true => std::array::from_fn(|i| w[i] / w[3]),
                false => [color[0], color[1], color[2]],
            };
            let a = c * alpha;
            let keep = 1.0 - a;
            match blend {
                StrokeBlend::Normal => {
                    (0..3).for_each(|i| px[i] = f16::from_f32(colour[i] * a + px[i].to_f32() * keep));
                    px[3] = f16::from_f32(a + px[3].to_f32() * keep);
                }
                StrokeBlend::Erase => px.iter_mut().for_each(|v| *v = f16::from_f32(v.to_f32() * keep)),
                StrokeBlend::Atop => {
                    let under = px[3].to_f32();
                    (0..3).for_each(|i| px[i] = f16::from_f32(colour[i] * a * under + px[i].to_f32() * keep));
                }
            }
        });
}

fn blend_replace(target: &mut [f16], coverage: &[f32], paint: &[f32], opacity: f32, lock: bool) {
    coverage
        .iter()
        .zip(paint.as_chunks::<4>().0)
        .zip(target.as_chunks_mut::<4>().0)
        .filter(|((c, _), _)| **c > 0.0)
        .for_each(|((&c, w), px)| {
            let a = (c * opacity).clamp(0.0, 1.0);
            let alpha = px[3].to_f32();
            let next: [f32; 4] = std::array::from_fn(|i| px[i].to_f32() + (w[i] - px[i].to_f32()) * a);
            match lock && next[3] > 1e-6 {
                true => (0..4).for_each(|i| px[i] = f16::from_f32(next[i] * alpha / next[3])),
                false if lock => {}
                false => (0..4).for_each(|i| px[i] = f16::from_f32(next[i])),
            }
        });
}

fn land(
    target: &mut [f16],
    coverage: &[f32],
    paint: Option<&[f32]>,
    mode: Mode,
    color: Rgba,
    opacity: f32,
    blend: StrokeBlend,
) {
    match (mode, paint) {
        (Mode::Colour, Some(paint)) => blend_colours(target, coverage, paint, color, opacity, blend),
        (Mode::Replace, Some(paint)) => blend_replace(target, coverage, paint, opacity, blend == StrokeBlend::Atop),
        _ => blend_coverage(target, coverage, color, opacity, blend),
    }
}

pub fn land_tile(
    target: &mut [f16],
    buffer: &StrokeBuffer,
    index: usize,
    color: Rgba,
    opacity: f32,
    blend: StrokeBlend,
) {
    if let Some(coverage) = buffer.coverage(index) {
        land(
            target,
            coverage,
            buffer.paint(index),
            buffer.mode,
            color,
            opacity,
            blend,
        );
    }
}

pub fn commit(
    raster: &mut Raster,
    buffer: &mut StrokeBuffer,
    color: Rgba,
    opacity: f32,
    blend: StrokeBlend,
) -> Vec<usize> {
    commit_cut(raster, buffer, color, opacity, Some(blend), &[])
}

fn commit_cut(
    raster: &mut Raster,
    buffer: &mut StrokeBuffer,
    color: Rgba,
    opacity: f32,
    blend: Option<StrokeBlend>,
    cuts: &[Cut],
) -> Vec<usize> {
    let touched = buffer.touched().to_vec();
    let cols = raster.cols;
    let mode = buffer.mode;
    let allowed: Vec<usize> = touched.iter().copied().filter(|&i| buffer.allowed(i)).collect();
    allowed.into_iter().for_each(|index| {
        let Some(blend) = blend else {
            return;
        };
        let factor = cut::factor(cuts, ((index % cols) * TILE, (index / cols) * TILE));
        let Some(coverage) = buffer.coverage_mut(index) else {
            return;
        };
        if let Some(factor) = factor {
            coverage.iter_mut().zip(&factor).for_each(|(c, k)| *c *= k);
            if coverage.iter().all(|c| *c <= 0.0) {
                return;
            }
        }
        let target = match (blend, mode) {
            (StrokeBlend::Normal, _) | (_, Mode::Replace) => Some(raster.ensure(index)),
            (StrokeBlend::Erase | StrokeBlend::Atop, _) => raster.at_mut(index),
        };
        if let (Some(target), Some(coverage)) = (target, buffer.coverage(index)) {
            land(target, coverage, buffer.paint(index), mode, color, opacity, blend);
        }
        land_body(raster, buffer, index, color, opacity, blend);
    });
    buffer.reset();
    touched
}

fn land_body(raster: &mut Raster, buffer: &StrokeBuffer, index: usize, color: Rgba, opacity: f32, blend: StrokeBlend) {
    let (body, lift, ridge) = (buffer.body, buffer.lift(index), buffer.ridge(index));
    let Some(coverage) = buffer.coverage(index) else {
        return;
    };
    let erase = blend == StrokeBlend::Erase && buffer.mode != Mode::Replace;
    if !erase && body <= 0.0 && lift.is_none() {
        return;
    }
    if erase && raster.height_tile(index).is_none() {
        return;
    }
    let alpha = match buffer.mode {
        Mode::Replace => opacity.clamp(0.0, 1.0),
        _ => (opacity * color[3]).clamp(0.0, 1.0),
    };
    let atop = blend == StrokeBlend::Atop;
    let under: Option<Vec<f32>> = atop.then(|| match raster.at(index) {
        Some(t) => t.as_chunks::<4>().0.iter().map(|p| p[3].to_f32()).collect(),
        None => vec![0.0; TILE * TILE],
    });
    let weights: Option<Vec<f32>> = match buffer.mode {
        Mode::Colour => buffer
            .paint(index)
            .map(|p| p.as_chunks::<4>().0.iter().map(|w| w[3]).collect()),
        _ => None,
    };
    let mode = buffer.mode;
    let heights = raster.heights_mut().ensure(index);
    coverage
        .iter()
        .zip(heights.iter_mut())
        .enumerate()
        .filter(|(_, (c, _))| **c > 0.0)
        .for_each(|(px, (&c, h))| {
            let mut a = (c * alpha).clamp(0.0, 1.0);
            if let Some(under) = &under {
                a *= under[px];
            }
            let mut value = h.to_f32();
            if erase {
                value *= 1.0 - a;
            } else {
                if let Some(lift) = lift {
                    let target = match (mode, &weights) {
                        (Mode::Colour, Some(w)) if w[px] > 0.0 => lift[px] / w[px],
                        (Mode::Colour, _) => value,
                        _ => lift[px],
                    };
                    value += (target - value) * a;
                }
                if body > 0.0 {
                    let thick = ridge.map_or(1.0, |r| r[px]);
                    let laid = smoothstep(0.0, 0.35, c) * (0.5 + 0.5 * alpha) * under.as_ref().map_or(1.0, |u| u[px]);
                    value = value * (1.0 - SQUASH * a) + body * laid * thick;
                }
            }
            *h = f16::from_f32(value.clamp(0.0, DEEPEST));
        });
}

pub fn stroke_blend(brush: &Brush, lock: bool) -> Option<StrokeBlend> {
    match (brush.blend, lock) {
        (blend, false) => Some(blend),
        (StrokeBlend::Erase, true) => None,
        (_, true) => Some(StrokeBlend::Atop),
    }
}

pub fn land_stroke(raster: &mut Raster, buffer: &mut StrokeBuffer, stroke: &Stroke, brush: &Brush) -> Vec<usize> {
    commit_cut(
        raster,
        buffer,
        stroke.color(),
        stroke.opacity,
        stroke_blend(brush, stroke.lock),
        &stroke.cuts,
    )
}

pub fn draw_stroke(buffer: &mut StrokeBuffer, brush: &Brush, stroke: &Stroke) -> usize {
    draw_stroke_on(buffer, brush, stroke, None)
}

pub fn draw_stroke_on(buffer: &mut StrokeBuffer, brush: &Brush, stroke: &Stroke, under: Option<&Raster>) -> usize {
    let mut painter = StrokePainter::new(brush, stroke);
    buffer.mode = painter.mode();
    stroke.samples.iter().for_each(|s| painter.push_on(s, buffer, under));
    painter.dabs()
}

pub fn paint_stroke(raster: &mut Raster, buffer: &mut StrokeBuffer, brush: &Brush, stroke: &Stroke) -> usize {
    let dabs = draw_stroke_on(buffer, brush, stroke, Some(raster));
    land_stroke(raster, buffer, stroke, brush);
    dabs
}

pub struct Replayed {
    pub raster: Raster,
    pub dabs: usize,
}

pub fn paint_mark(
    raster: &mut Raster,
    buffer: &mut StrokeBuffer,
    brushes: &BrushSet,
    mark: &Mark,
) -> Result<Vec<usize>> {
    Ok(match mark {
        Mark::Stroke(stroke) => {
            let brush = brushes.get(&stroke.brush)?;
            draw_stroke_on(buffer, brush, stroke, Some(raster));
            land_stroke(raster, buffer, stroke, brush)
        }
        Mark::Fill(fill) => crate::fill::paint_fill_where(raster, fill, |i| buffer.allowed(i)),
    })
}

pub fn replay_onto(raster: &mut Raster, marks: &[Mark], brushes: &BrushSet) -> Result<usize> {
    let mut buffer = StrokeBuffer::new(raster.width, raster.height);
    marks.iter().try_fold(0usize, |n, mark| {
        Ok::<_, anyhow::Error>(
            n + match mark {
                Mark::Stroke(stroke) => paint_stroke(raster, &mut buffer, brushes.get(&stroke.brush)?, stroke),
                Mark::Fill(fill) => {
                    crate::fill::paint_fill(raster, fill);
                    0
                }
            },
        )
    })
}

pub fn replay(width: u32, height: u32, marks: &[Mark], brushes: &BrushSet) -> Result<Replayed> {
    let mut raster = Raster::new(width, height);
    let dabs = replay_onto(&mut raster, marks, brushes)?;
    Ok(Replayed { raster, dabs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::stroke::marks;
    use klad_core::synth::Synth;

    fn alpha(raster: &Raster, x: usize, y: usize) -> f32 {
        raster
            .get(x / TILE, y / TILE)
            .map(|t| t[((y % TILE) * TILE + x % TILE) * 4 + 3].to_f32())
            .unwrap_or(0.0)
    }

    fn m1() -> BrushSet {
        let mut set = BrushSet::presets();
        ["pencil", "ink", "soft", "chalk", "eraser"]
            .iter()
            .for_each(|id| set.insert(id, klad_core::brushes::legacy_m1(id).unwrap()));
        set
    }

    fn line(brush: &str, size: f32, from: (f32, f32), to: (f32, f32), pressure: (f32, f32)) -> Stroke {
        let points: Vec<(f32, f32, f32)> = (0..=40)
            .map(|i| {
                let t = i as f32 / 40.0;
                (
                    from.0 + (to.0 - from.0) * t,
                    from.1 + (to.1 - from.1) * t,
                    pressure.0 + (pressure.1 - pressure.0) * t,
                )
            })
            .collect();
        Stroke::new(1, brush, [0.0, 0.0, 0.0, 1.0], size, 9).with_points(&points, 5.0)
    }

    fn width_at(raster: &Raster, x: usize) -> usize {
        (0..raster.height as usize)
            .filter(|&y| alpha(raster, x, y) > 0.5)
            .count()
    }

    #[test]
    fn ink_line_tapers_with_pressure() {
        let brushes = BrushSet::presets();
        let stroke = line("ink", 20.0, (20.0, 100.0), (480.0, 100.0), (0.1, 1.0));
        let out = replay(512, 200, &[stroke.into()], &brushes).unwrap();
        let (thin, thick) = (width_at(&out.raster, 40), width_at(&out.raster, 460));
        assert!(thin >= 1 && thin < thick, "thin {thin} thick {thick}");
        assert!((18..=21).contains(&thick), "thick {thick}");
        assert!(alpha(&out.raster, 250, 100) > 0.99);
        assert_eq!(alpha(&out.raster, 250, 150), 0.0);
    }

    #[test]
    fn a_hard_line_keeps_its_antialiased_edge() {
        let brushes = BrushSet::presets();
        [3.0, 5.0, 12.0].iter().for_each(|&size| {
            let stroke = line("ink", size, (10.0, 20.0), (250.0, 60.0), (1.0, 1.0));
            let out = replay(260, 80, &[stroke.into()], &brushes).unwrap();
            let soft = (60..200)
                .filter(|&x| {
                    let column: Vec<f32> = (0..80).map(|y| alpha(&out.raster, x, y)).collect();
                    let top = column.iter().position(|&a| a > 0.02).unwrap();
                    let bottom = column.iter().rposition(|&a| a > 0.02).unwrap();
                    column[top] < 0.95 && column[bottom] < 0.95
                })
                .count();
            assert!(soft > 130, "size {size}: {soft} of 140 columns have soft edges");
        });
    }

    #[test]
    fn soft_tips_build_up_where_a_stroke_comes_back_and_hard_tips_do_not() {
        let brushes = BrushSet::presets();
        let there = |brush: &str, size: f32| line(brush, size, (20.0, 64.0), (300.0, 64.0), (1.0, 1.0));
        let back = |brush: &str, size: f32| {
            let points: Vec<(f32, f32, f32)> = (0..=80)
                .map(|i| match i <= 40 {
                    true => (20.0 + 7.0 * i as f32, 64.0, 1.0),
                    false => (300.0 - 7.0 * (i - 40) as f32, 64.0, 1.0),
                })
                .collect();
            Stroke::new(1, brush, [0.0, 0.0, 0.0, 1.0], size, 9).with_points(&points, 5.0)
        };
        let at = |stroke: Stroke, x: usize, y: usize| {
            alpha(&replay(320, 128, &[stroke.into()], &brushes).unwrap().raster, x, y)
        };
        assert!(at(back("airbrush", 40.0), 160, 76) > at(there("airbrush", 40.0), 160, 76) + 0.02);
        let edge = at(there("ink", 12.0), 160, 69);
        assert!(edge > 0.1 && edge < 0.9, "edge {edge}");
        assert!((at(back("ink", 12.0), 160, 69) - edge).abs() < 0.01);
    }

    #[test]
    fn dab_count_follows_spacing() {
        let brushes = BrushSet::presets();
        let stroke = line("ink", 20.0, (10.0, 50.0), (410.0, 50.0), (1.0, 1.0));
        let out = replay(512, 100, &[stroke.into()], &brushes).unwrap();
        let expected = 400.0 / (20.0 * 0.04);
        assert!((out.dabs as f32 - expected).abs() < 3.0, "dabs {}", out.dabs);
    }

    #[test]
    fn single_sample_makes_a_dot() {
        let brushes = BrushSet::presets();
        let dot = Stroke::new(1, "ink", [1.0, 0.0, 0.0, 1.0], 10.0, 1).with_points(&[(64.0, 64.0, 1.0)], 0.0);
        let out = replay(128, 128, &[dot.into()], &brushes).unwrap();
        assert_eq!(out.dabs, 1);
        assert!(alpha(&out.raster, 64, 64) > 0.99);
        assert_eq!(alpha(&out.raster, 64, 80), 0.0);
    }

    #[test]
    fn eraser_removes_paint() {
        let brushes = BrushSet::presets();
        let paint = line("ink", 30.0, (20.0, 64.0), (300.0, 64.0), (1.0, 1.0));
        let mut erase = line("eraser", 40.0, (150.0, 10.0), (150.0, 120.0), (1.0, 1.0));
        erase.id = 2;
        let out = replay(320, 128, &[paint.into(), erase.into()], &brushes).unwrap();
        assert!(alpha(&out.raster, 60, 64) > 0.99);
        assert!(alpha(&out.raster, 150, 64) < 0.01);
    }

    #[test]
    fn replay_is_deterministic() {
        let brushes = BrushSet::presets();
        let strokes = marks(Synth::canvas(512.0, 512.0).strokes(60, 4));
        let a = replay(512, 512, &strokes, &brushes).unwrap();
        let b = replay(512, 512, &strokes, &brushes).unwrap();
        assert_eq!(a.raster.hash(), b.raster.hash());
        assert_eq!(a.dabs, b.dabs);
        let other = replay(512, 512, &marks(Synth::canvas(512.0, 512.0).strokes(60, 5)), &brushes).unwrap();
        assert_ne!(a.raster.hash(), other.raster.hash());
    }

    #[test]
    fn m1_brushes_as_data_paint_the_m1_pixels() {
        let fixtures = [
            ("pencil", 0xfb2d7140ad2324e6_u64, 10166),
            ("ink", 0xf2a6edba413a1461, 11493),
            ("soft", 0xd8ba2d80b00ee7d8, 9921),
            ("chalk", 0x3820705eaab7d054, 9239),
            ("eraser", 0xe76b9c2658038834, 10880),
        ];
        let brushes = m1();
        fixtures.iter().for_each(|&(name, hash, dabs)| {
            let mut synth = Synth::canvas(512.0, 512.0);
            synth.brushes = vec!["ink".into(), name.to_string()];
            let out = replay(512, 512, &marks(synth.strokes(40, 77)), &brushes).unwrap();
            assert_eq!((out.raster.hash(), out.dabs), (hash, dabs), "{name}");
        });
    }

    #[test]
    fn taper_thins_both_ends_by_length_whatever_the_pressure() {
        let brushes = BrushSet::presets();
        let flat = line("ink", 20.0, (20.0, 100.0), (480.0, 100.0), (1.0, 1.0));
        let tapered = flat.clone().tapered([60.0, 120.0]);
        let plain = replay(512, 200, &[flat.into()], &brushes).unwrap();
        let out = replay(512, 200, &[tapered.into()], &brushes).unwrap();
        let widths = [30, 50, 250, 400, 470].map(|x| width_at(&out.raster, x));
        assert_eq!(width_at(&plain.raster, 30), width_at(&plain.raster, 250));
        assert!(widths[0] < widths[1] && widths[1] < widths[2], "{widths:?}");
        assert!(widths[4] < widths[3] && widths[3] < widths[2], "{widths:?}");
        assert_eq!(widths[2], width_at(&plain.raster, 250));
        assert!(alpha(&out.raster, 18, 100) < 0.05);
    }

    #[test]
    fn a_tapered_tap_still_leaves_a_dot() {
        let brushes = BrushSet::presets();
        let dot = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 10.0, 1)
            .with_points(&[(64.0, 64.0, 1.0), (64.0, 64.0, 1.0)], 4.0)
            .tapered([10.0, 24.0]);
        let out = replay(128, 128, &[dot.into()], &brushes).unwrap();
        assert!(alpha(&out.raster, 64, 64) > 0.99);
    }

    #[test]
    fn an_all_zero_pressure_stroke_draws_as_if_pressure_were_missing() {
        let brushes = BrushSet::presets();
        let zero = line("ink", 20.0, (20.0, 100.0), (480.0, 100.0), (0.0, 0.0));
        let full = line("ink", 20.0, (20.0, 100.0), (480.0, 100.0), (1.0, 1.0));
        let a = replay(512, 200, &[zero.into()], &brushes).unwrap();
        let b = replay(512, 200, &[full.into()], &brushes).unwrap();
        assert_eq!(a.raster.hash(), b.raster.hash());
        let light = line("ink", 20.0, (20.0, 100.0), (480.0, 100.0), (0.0, 0.002));
        let thin = replay(512, 200, &[light.into()], &brushes).unwrap();
        assert!(width_at(&thin.raster, 250) < 3);
    }

    #[test]
    fn live_painting_then_a_repaint_matches_replay() {
        let brushes = BrushSet::presets();
        let stroke = line("ink", 16.0, (20.0, 60.0), (300.0, 90.0), (0.2, 0.9)).tapered([10.0, 24.0]);
        let brush = brushes.get("ink").unwrap();
        let mut buffer = StrokeBuffer::new(320, 160);
        let mut live = StrokePainter::live(brush, &stroke);
        stroke.samples.iter().for_each(|s| live.push(s, &mut buffer));
        assert!(needs_repaint(&stroke));
        buffer.reset();
        draw_stroke(&mut buffer, brush, &stroke);
        let mut raster = Raster::new(320, 160);
        commit(&mut raster, &mut buffer, stroke.color(), stroke.opacity, brush.blend);
        let replayed = replay(320, 160, &[stroke.into()], &brushes).unwrap().raster;
        assert_eq!(raster.hash(), replayed.hash());
        let untapered = line("ink", 16.0, (20.0, 60.0), (300.0, 90.0), (0.2, 0.9));
        assert!(!needs_repaint(&untapered));
    }

    #[test]
    fn marker_strokes_cap_their_own_overlap_but_build_up_across_strokes() {
        let brushes = BrushSet::presets();
        let there = line("marker", 30.0, (20.0, 64.0), (300.0, 64.0), (1.0, 1.0));
        let mut back = line("marker", 30.0, (300.0, 64.0), (20.0, 64.0), (1.0, 1.0));
        back.id = 2;
        let mut once = there.clone();
        once.samples.extend(back.samples.iter().skip(1).copied());
        let single = replay(320, 128, &[once.into()], &brushes).unwrap();
        let double = replay(320, 128, &[there.into(), back.into()], &brushes).unwrap();
        let (a, b) = (alpha(&single.raster, 160, 64), alpha(&double.raster, 160, 64));
        assert!((a - 0.7).abs() < 0.02, "one stroke {a}");
        assert!(b > a + 0.15, "two strokes {b}");
    }

    #[test]
    fn strokes_off_canvas_are_clipped() {
        let brushes = BrushSet::presets();
        let stroke = line("soft", 80.0, (-200.0, -50.0), (900.0, 300.0), (1.0, 1.0));
        let out = replay(256, 256, &[stroke.into()], &brushes).unwrap();
        assert!(out.raster.count() > 0);
    }

    fn rgba(raster: &Raster, x: usize, y: usize) -> [f32; 4] {
        raster
            .get(x / TILE, y / TILE)
            .map(|t| std::array::from_fn(|c| t[((y % TILE) * TILE + x % TILE) * 4 + c].to_f32()))
            .unwrap_or([0.0; 4])
    }

    fn coloured(brush: &str, size: f32, from: (f32, f32), to: (f32, f32), colour: Rgba, seed: u32) -> Stroke {
        let mut s = line(brush, size, from, to, (1.0, 1.0));
        s.color = klad_core::color::to_u16(colour);
        s.seed = seed;
        s
    }

    #[test]
    fn picture_tips_and_dual_tips_leave_their_marks() {
        let brushes = BrushSet::presets();
        let ink = replay(
            512,
            128,
            &[line("ink", 40.0, (40.0, 64.0), (470.0, 64.0), (1.0, 1.0)).into()],
            &brushes,
        )
        .unwrap()
        .raster;
        ["flat-bristle", "round-bristle", "sponge", "dry-brush"]
            .iter()
            .for_each(|name| {
                let out = replay(
                    512,
                    128,
                    &[line(name, 40.0, (40.0, 64.0), (470.0, 64.0), (1.0, 1.0)).into()],
                    &brushes,
                )
                .unwrap()
                .raster;
                let covered = |r: &Raster| {
                    (40..470)
                        .flat_map(|x| (40..90).map(move |y| (x, y)))
                        .filter(|&(x, y)| alpha(r, x, y) > 0.5)
                        .count()
                };
                let (mine, solid) = (covered(&out), covered(&ink));
                assert!(mine > solid / 10 && mine < solid, "{name}: {mine} of {solid}");
            });
        let mut plain = klad_core::brushes::library("dry-brush").unwrap();
        plain.dual = None;
        let mut set = BrushSet::presets();
        set.insert("plain", plain);
        let count = |name: &str| {
            let r = replay(
                512,
                128,
                &[line(name, 40.0, (40.0, 64.0), (470.0, 64.0), (1.0, 1.0)).into()],
                &set,
            )
            .unwrap()
            .raster;
            (0..512)
                .flat_map(|x| (0..128).map(move |y| (x, y)))
                .filter(|&(x, y)| alpha(&r, x, y) > 0.3)
                .count()
        };
        assert!(
            count("dry-brush") < count("plain") * 9 / 10,
            "the dual tip takes paint away"
        );
    }

    #[test]
    fn texture_stays_on_the_canvas_under_two_strokes() {
        let mut def = klad_core::brushes::library("soft").unwrap();
        def.grain = Some(klad_core::brushes::GrainDef {
            paper: None,
            image: Some("sponge".into()),
            strength: 1.0,
            scale: 1.0,
            contrast: 2.0,
            pressure: 0.0,
        });
        let mut set = BrushSet::presets();
        set.insert("textured", def);
        let a = coloured("textured", 60.0, (100.0, 64.0), (300.0, 64.0), [0.0, 0.0, 0.0, 1.0], 1);
        let b = coloured("textured", 60.0, (300.0, 64.0), (100.0, 64.0), [0.0, 0.0, 0.0, 1.0], 99);
        let ra = replay(400, 128, &[a.into()], &set).unwrap().raster;
        let rb = replay(400, 128, &[b.into()], &set).unwrap().raster;
        let holes = |r: &Raster| {
            (150..250)
                .flat_map(|x| (55..74).map(move |y| (x, y)))
                .filter(|&(x, y)| alpha(r, x, y) < 0.05)
                .collect::<Vec<_>>()
        };
        assert!(!holes(&ra).is_empty());
        assert_eq!(holes(&ra), holes(&rb), "the texture's holes are where they were");
    }

    #[test]
    fn colour_jitter_repeats_with_its_seed_and_two_tone_follows_pressure() {
        let brushes = BrushSet::presets();
        let leaves = |seed| coloured("leaves", 30.0, (40.0, 64.0), (470.0, 64.0), [0.2, 0.5, 0.2, 1.0], seed);
        let a = replay(512, 128, &[leaves(4).into()], &brushes).unwrap().raster;
        let b = replay(512, 128, &[leaves(4).into()], &brushes).unwrap().raster;
        let c = replay(512, 128, &[leaves(5).into()], &brushes).unwrap().raster;
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        let greens: Vec<[f32; 4]> = (0..512)
            .flat_map(|x| (0..128).map(move |y| (x, y)))
            .map(|(x, y)| rgba(&a, x, y))
            .filter(|p| p[3] > 0.95)
            .collect();
        assert!(greens.iter().any(|p| (p[1] - 0.5).abs() > 0.02), "the colour varies");
        assert!(greens.iter().all(|p| p[1] > p[0] && p[1] > p[2]), "and stays green");
        let tone = |pressure: f32| {
            let mut s = line("two-tone", 20.0, (20.0, 32.0), (200.0, 32.0), (pressure, pressure));
            s.color = klad_core::color::to_u16([1.0, 0.0, 0.0, 1.0]);
            s.second = Some(klad_core::color::to_u16([0.0, 0.0, 1.0, 1.0]));
            let r = replay(256, 64, &[s.into()], &brushes).unwrap().raster;
            rgba(&r, 110, 32)
        };
        let (hard, light) = (tone(1.0), tone(0.1));
        assert!(hard[0] > 0.95 && hard[2] < 0.05, "{hard:?}");
        assert!(light[2] > 0.8 && light[0] < 0.2, "{light:?}");
    }

    fn base_and(brush: &str) -> Vec<Mark> {
        let red = coloured("ink", 60.0, (40.0, 64.0), (220.0, 64.0), [1.0, 0.0, 0.0, 1.0], 1);
        let mut over = coloured(brush, 24.0, (100.0, 64.0), (400.0, 64.0), [0.0, 0.0, 1.0, 1.0], 2);
        over.id = 2;
        vec![red.into(), over.into()]
    }

    #[test]
    fn wet_paint_picks_up_what_it_crosses_and_smudge_and_blend_move_it() {
        let brushes = BrushSet::presets();
        let wet = replay(512, 128, &base_and("wet"), &brushes).unwrap().raster;
        let (on_red, beyond) = (rgba(&wet, 200, 64), rgba(&wet, 330, 64));
        assert!(on_red[0] > 0.15 && on_red[2] > 0.15, "mixed on the red: {on_red:?}");
        assert!(beyond[0] > 0.05, "red carried past it: {beyond:?}");
        let dry = replay(512, 128, &base_and("ink"), &brushes).unwrap().raster;
        assert!(rgba(&dry, 330, 64)[0] < 0.01);
        let smudged = replay(512, 128, &base_and("smudge"), &brushes).unwrap().raster;
        assert_eq!(rgba(&dry, 262, 64)[0], 0.0, "the red ends before x 262");
        let pulled = rgba(&smudged, 262, 64);
        assert!(
            pulled[0] > 0.05 && pulled[3] > 0.05,
            "red pulled past its end: {pulled:?}"
        );
        assert!(pulled[2] < 0.01, "smudge lays no colour of its own");
        let edge = |r: &Raster| (225..260).filter(|&x| (0.1..0.9).contains(&rgba(r, x, 64)[3])).count();
        let blended = replay(512, 128, &base_and("blender"), &brushes).unwrap().raster;
        assert!(edge(&blended) > edge(&dry), "the blender softens the red's end");
    }

    #[test]
    fn an_edit_under_a_smudge_replays_the_smudge_correctly() {
        let brushes = BrushSet::presets();
        let marks = base_and("smudge");
        let (mut cel, mut raster, _) = crate::replay::CelReplay::replay(512, 128, marks.clone(), &brushes).unwrap();
        let mut edited = marks.clone();
        if let Mark::Stroke(s) = &mut edited[0] {
            s.color = klad_core::color::to_u16([0.0, 0.8, 0.0, 1.0]);
        }
        cel.update(&mut raster, edited.clone(), &brushes).unwrap();
        let fresh = replay(512, 128, &edited, &brushes).unwrap().raster;
        assert_eq!(raster.hash(), fresh.hash());
        assert!(rgba(&raster, 262, 64)[1] > 0.05, "the smudge now pulls green");
    }

    #[test]
    fn live_wet_painting_lands_as_its_replay() {
        let brushes = BrushSet::presets();
        let marks = base_and("wet");
        let Mark::Stroke(wet) = &marks[1] else { panic!() };
        let mut raster = replay(512, 128, &marks[..1], &brushes).unwrap().raster;
        let brush = brushes.get("wet").unwrap();
        let mut buffer = StrokeBuffer::new(512, 128);
        let mut live = StrokePainter::new(brush, wet);
        wet.samples
            .iter()
            .for_each(|s| live.push_on(s, &mut buffer, Some(&raster)));
        commit(&mut raster, &mut buffer, wet.color(), wet.opacity, brush.blend);
        assert_eq!(raster.hash(), replay(512, 128, &marks, &brushes).unwrap().raster.hash());
    }

    fn height(raster: &Raster, x: usize, y: usize) -> f32 {
        raster
            .height_tile(raster.index(x / TILE, y / TILE))
            .map_or(0.0, |t| t[(y % TILE) * TILE + x % TILE].to_f32())
    }

    fn thick_and(brush: &str) -> Vec<Mark> {
        let oil = coloured("oil-load", 60.0, (40.0, 64.0), (220.0, 64.0), [0.8, 0.6, 0.2, 1.0], 1);
        let mut over = coloured(brush, 24.0, (100.0, 64.0), (400.0, 64.0), [0.2, 0.3, 0.8, 1.0], 2);
        over.id = 2;
        vec![oil.into(), over.into()]
    }

    #[test]
    fn a_brush_without_body_leaves_no_height() {
        let brushes = BrushSet::presets();
        ["ink", "pencil", "soft", "wet", "smudge", "chalk"]
            .iter()
            .for_each(|b| {
                let out = replay(512, 128, &base_and(b), &brushes).unwrap().raster;
                assert!(!out.has_height(), "{b} left height");
            });
    }

    #[test]
    fn paint_with_body_ridges_at_its_edges_builds_where_strokes_overlap_and_replays_the_same() {
        let brushes = BrushSet::presets();
        let one = replay(512, 128, &thick_and("ink")[..1], &brushes).unwrap().raster;
        assert!(one.has_height());
        let column: Vec<f32> = (0..128).map(|y| height(&one, 130, y)).collect();
        let top = column.iter().position(|&h| h > 0.05).unwrap();
        let bottom = column.iter().rposition(|&h| h > 0.05).unwrap();
        let reach = (bottom - top) as f32 / 2.0;
        let centre = (top + bottom) as f32 / 2.0;
        let band = |from: f32, to: f32| {
            let ys: Vec<usize> = (top..=bottom)
                .filter(|&y| ((y as f32 - centre).abs() / reach) >= from && ((y as f32 - centre).abs() / reach) < to)
                .collect();
            let n = ys.len() * 80;
            ys.iter()
                .flat_map(|&y| (100..180).map(move |x| (x, y)))
                .map(|(x, y)| height(&one, x, y))
                .sum::<f32>()
                / n as f32
        };
        let (middle, rim) = (band(0.0, 0.3), band(0.55, 0.8));
        assert!(middle > 0.3, "middle {middle}");
        assert!(rim > middle * 1.08, "a lip at the edge: rim {rim} middle {middle}");
        assert_eq!(height(&one, 130, 110), 0.0);
        let column: Vec<f32> = (40..88).map(|y| height(&one, 130, y)).collect();
        let grooves = column.windows(3).filter(|w| w[1] < w[0] && w[1] < w[2]).count();
        assert!(grooves >= 4, "bristles leave grooves along the stroke: {grooves}");
        let middle = height(&one, 130, 64);
        let mut twice = thick_and("oil-load");
        twice[1] = coloured("oil-load", 60.0, (130.0, 64.0), (300.0, 64.0), [0.8, 0.6, 0.2, 1.0], 2).into();
        let two = replay(512, 128, &twice, &brushes).unwrap().raster;
        let overlap = height(&two, 175, 64);
        assert!(
            overlap > middle * 1.02 && overlap < middle * 1.5,
            "new paint pushes most of the wet paint under it aside: {overlap} over {middle}"
        );
        assert_eq!(two.hash(), replay(512, 128, &twice, &brushes).unwrap().raster.hash());
        let (mut cel, mut raster, _) = crate::replay::CelReplay::replay(512, 128, twice.clone(), &brushes).unwrap();
        let mut edited = twice.clone();
        if let Mark::Stroke(s) = &mut edited[1] {
            s.size = 40.0;
        }
        cel.update(&mut raster, edited.clone(), &brushes).unwrap();
        assert_eq!(
            raster.hash(),
            replay(512, 128, &edited, &brushes).unwrap().raster.hash()
        );
    }

    #[test]
    fn the_eraser_takes_paint_height_smudge_and_wet_drag_it_and_the_blender_flattens_it() {
        let brushes = BrushSet::presets();
        let thick = replay(512, 128, &thick_and("ink")[..1], &brushes).unwrap().raster;
        let erased = replay(512, 128, &thick_and("eraser"), &brushes).unwrap().raster;
        assert!(height(&erased, 150, 64) < height(&thick, 150, 64) * 0.2);
        assert_eq!(height(&thick, 262, 64), 0.0, "the paint ends before x 262");
        let smudged = replay(512, 128, &thick_and("smudge"), &brushes).unwrap().raster;
        assert!(height(&smudged, 262, 64) > 0.05, "smudge pulls the paint along");
        let wet = replay(512, 128, &thick_and("wet"), &brushes).unwrap().raster;
        assert!(height(&wet, 262, 64) > 0.02, "wet paint carries some of it");
        let steepest = |r: &Raster| {
            (60..68)
                .flat_map(|y| (140..280).map(move |x| (x, y)))
                .map(|(x, y)| (height(r, x + 1, y) - height(r, x, y)).abs())
                .fold(0.0, f32::max)
        };
        let blended = replay(512, 128, &thick_and("blender"), &brushes).unwrap().raster;
        assert!(
            steepest(&blended) < steepest(&thick) * 0.8,
            "the blender flattens the paint: {} against {}",
            steepest(&blended),
            steepest(&thick)
        );
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    #[ignore]
    fn painting_cost_per_sample_at_120_px() {
        let brushes = BrushSet::presets();
        ["ink", "dry-brush", "flat-bristle", "wet", "smudge", "blender"]
            .iter()
            .for_each(|name| {
                let brush = brushes.get(name).unwrap();
                let points: Vec<(f32, f32, f32)> = (0..300).map(|i| (100.0 + i as f32 * 2.0, 300.0, 0.8)).collect();
                let stroke = Stroke::new(1, name, [0.3, 0.4, 0.5, 1.0], 120.0, 3).with_points(&points, 4.0);
                let under = replay(
                    1024,
                    600,
                    &[Stroke::new(9, "ink", [0.8, 0.2, 0.1, 1.0], 200.0, 1)
                        .with_points(&[(100.0, 300.0, 1.0), (900.0, 300.0, 1.0)], 4.0)
                        .into()],
                    &brushes,
                )
                .unwrap()
                .raster;
                let mut buffer = StrokeBuffer::new(1024, 600);
                let mut painter = StrokePainter::live(brush, &stroke);
                let mut times: Vec<f64> = stroke
                    .samples
                    .iter()
                    .map(|s| {
                        let t = std::time::Instant::now();
                        painter.push_on(s, &mut buffer, Some(&under));
                        t.elapsed().as_secs_f64() * 1000.0
                    })
                    .collect();
                times.sort_by(f64::total_cmp);
                eprintln!(
                    "{name} at 120 px: per pen sample median {:.2} ms, p99 {:.2}, {} dabs",
                    times[times.len() / 2],
                    times[times.len() * 99 / 100],
                    painter.dabs()
                );
            });
    }
}
