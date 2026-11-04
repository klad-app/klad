use crate::shapes::{P, Run, area};
use klad_core::color;
use klad_core::cut::inside_polygon;
use klad_core::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Form {
    Round,
    Flat { facing: [f32; 3] },
    Cylinder { axis: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lighting {
    Direction { toward: [f32; 3] },
    Lamp { at: [f32; 3] },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub lab: [f32; 3],
    pub mix: f32,
}

impl Shadow {
    pub fn of(rgba: color::Rgba, mix: f32) -> Self {
        Self {
            lab: color::to_oklab(rgba),
            mix: mix.clamp(0.0, 1.0),
        }
    }

    fn tint(&self, rgb: [f32; 3], lit: f32) -> [f32; 3] {
        let k = self.mix * (1.0 - smooth(-0.1, 0.3, lit));
        if k <= 0.0 {
            return rgb;
        }
        let [l, a, b] = color::to_oklab(color::from_linear(rgb, 1.0));
        let fit = libm::sqrtf((l / self.lab[0].max(1e-3)).min(1.0));
        let (ta, tb) = (self.lab[1] * fit, self.lab[2] * fit);
        let (ma, mb) = (a + (ta - a) * k, b + (tb - b) * k);
        let from = libm::sqrtf(a * a + b * b);
        let to = libm::sqrtf(ta * ta + tb * tb);
        let mixed = libm::sqrtf(ma * ma + mb * mb);
        let keep = match mixed > 1e-4 {
            true => (from + (to - from) * k) / mixed,
            false => 1.0,
        };
        color::to_linear(color::from_oklab([l, ma * keep, mb * keep], 1.0))
    }
}

fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug)]
pub struct FormRules {
    pub form: Form,
    pub light: Lighting,
    pub colour: [f32; 3],
    pub key: [f32; 3],
    pub fill: [f32; 3],
    pub bounce: [f32; 3],
    pub core: f32,
    pub weight: f32,
    pub angle: f32,
    pub decisive: f32,
    pub vary: f32,
    pub seed: u32,
    pub spill: bool,
    pub shadow: Option<Shadow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormStroke {
    pub run: Run,
    pub rgb: [f32; 3],
    pub size: f32,
    pub bold: bool,
    pub opacity: f32,
    pub hand: f32,
}

const STEP: f32 = 2.0;
const CUT: f32 = 0.14;

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: P) -> f32 {
    libm::sqrtf(a[0] * a[0] + a[1] * a[1])
}

fn unit3(v: [f32; 3]) -> [f32; 3] {
    let d = libm::sqrtf(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).max(1e-6);
    [v[0] / d, v[1] / d, v[2] / d]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn nearest(poly: &[P], p: P) -> (f32, P) {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let ab = sub(b, a);
            let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-9))
                .clamp(0.0, 1.0);
            let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
            (len(sub(p, q)), q)
        })
        .fold((f32::MAX, [0.0, 0.0]), |best, c| if c.0 < best.0 { c } else { best })
}

fn inside(poly: &[P], p: P) -> bool {
    inside_polygon(poly, (p[0], p[1]))
}

fn across(poly: &[P], p: P, dir: P) -> (f32, f32) {
    let hits: Vec<f32> = (0..poly.len())
        .filter_map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let e = sub(b, a);
            let den = dir[0] * e[1] - dir[1] * e[0];
            if den.abs() < 1e-9 {
                return None;
            }
            let w = sub(a, p);
            let s = (w[0] * e[1] - w[1] * e[0]) / den;
            let u = (w[0] * dir[1] - w[1] * dir[0]) / den;
            (0.0..=1.0).contains(&u).then_some(s)
        })
        .collect();
    let lo = hits.iter().copied().filter(|s| *s <= 0.0).fold(f32::MIN, f32::max);
    let hi = hits.iter().copied().filter(|s| *s >= 0.0).fold(f32::MAX, f32::min);
    (
        if lo == f32::MIN { 0.0 } else { lo },
        if hi == f32::MAX { 0.0 } else { hi },
    )
}

struct Field {
    origin: P,
    cell: f32,
    nx: usize,
    ny: usize,
    signed: Vec<f32>,
}

impl Field {
    fn new(poly: &[P], lo: P, hi: P) -> Self {
        let cell = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / 200.0).max(1.0);
        let pad = cell * 4.0;
        let origin = [lo[0] - pad, lo[1] - pad];
        let nx = ((hi[0] - lo[0] + 2.0 * pad) / cell) as usize + 2;
        let ny = ((hi[1] - lo[1] + 2.0 * pad) / cell) as usize + 2;
        let signed = (0..ny)
            .flat_map(|j| (0..nx).map(move |i| (i, j)))
            .map(|(i, j)| {
                let p = [origin[0] + cell * i as f32, origin[1] + cell * j as f32];
                let d = nearest(poly, p).0;
                if inside(poly, p) { d } else { -d }
            })
            .collect();
        Field {
            origin,
            cell,
            nx,
            ny,
            signed,
        }
    }

    fn at(&self, p: P) -> f32 {
        let x = ((p[0] - self.origin[0]) / self.cell).clamp(0.0, (self.nx - 1) as f32 - 1e-3);
        let y = ((p[1] - self.origin[1]) / self.cell).clamp(0.0, (self.ny - 1) as f32 - 1e-3);
        let (i, j) = (x as usize, y as usize);
        let (fx, fy) = (x - i as f32, y - j as f32);
        let v = |i: usize, j: usize| self.signed[j * self.nx + i];
        let top = v(i, j) * (1.0 - fx) + v(i + 1, j) * fx;
        let bottom = v(i, j + 1) * (1.0 - fx) + v(i + 1, j + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    fn inward(&self, p: P) -> P {
        let e = self.cell;
        let gx = self.at([p[0] + e, p[1]]) - self.at([p[0] - e, p[1]]);
        let gy = self.at([p[0], p[1] + e]) - self.at([p[0], p[1] - e]);
        let d = libm::sqrtf(gx * gx + gy * gy);
        match d > 1e-6 {
            true => [gx / d, gy / d],
            false => [0.0, 0.0],
        }
    }
}

struct Shape<'a> {
    poly: &'a [P],
    field: Field,
    reach: f32,
    reference: f32,
    lo: P,
    hi: P,
}

impl<'a> Shape<'a> {
    fn new(poly: &'a [P], rules: &FormRules) -> Self {
        let (lo, hi) = poly.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), q| {
            ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])])
        });
        let field = Field::new(poly, lo, hi);
        let (best, reach) = field
            .signed
            .iter()
            .enumerate()
            .fold((0, 1.0f32), |best, (k, d)| if *d > best.1 { (k, *d) } else { best });
        let centre = [
            field.origin[0] + field.cell * (best % field.nx) as f32,
            field.origin[1] + field.cell * (best / field.nx) as f32,
        ];
        let mut shape = Shape {
            poly,
            field,
            reach,
            reference: 1.0,
            lo,
            hi,
        };
        if let Lighting::Lamp { at } = rules.light {
            let h = shape.height(&rules.form, centre);
            shape.reference =
                libm::sqrtf((at[0] - centre[0]).powi(2) + (at[1] - centre[1]).powi(2) + (at[2] - h).powi(2)).max(1.0);
        }
        shape
    }

    fn dome(&self, q: P) -> f32 {
        let v = (self.field.at(q) / self.reach).clamp(0.0, 1.0);
        self.reach * libm::sqrtf((1.0 - (1.0 - v) * (1.0 - v)).max(0.0))
    }

    fn round(&self, p: P) -> (f32, [f32; 3]) {
        let u = (self.field.at(p) / self.reach).clamp(0.0, 1.0);
        let g = self.field.inward(p);
        let rise = libm::sqrtf((1.0 - (1.0 - u) * (1.0 - u)).max(1e-4));
        let slope = (1.0 - u) / rise;
        let sharp = unit3([-g[0] * slope, -g[1] * slope, 1.0]);
        let blend = ((u - 0.2) / 0.25).clamp(0.0, 1.0);
        if blend == 0.0 {
            return (self.reach * rise, sharp);
        }
        let e = (self.reach * 0.14).max(1.5);
        let hx = (self.dome([p[0] + e, p[1]]) - self.dome([p[0] - e, p[1]])) / (2.0 * e);
        let hy = (self.dome([p[0], p[1] + e]) - self.dome([p[0], p[1] - e])) / (2.0 * e);
        let soft = unit3([-hx, -hy, 1.0]);
        let mixed = [0, 1, 2].map(|c| sharp[c] * (1.0 - blend) + soft[c] * blend);
        (self.reach * rise, unit3(mixed))
    }

    fn cylinder(&self, p: P, axis: f32) -> (f32, [f32; 3]) {
        let a = axis.to_radians();
        let perp = [-libm::sinf(a), libm::cosf(a)];
        let (s1, s2) = across(self.poly, p, perp);
        let half = ((s2 - s1) / 2.0).max(1e-3);
        let t = (-(s1 + s2) / 2.0 / half).clamp(-1.0, 1.0);
        let z = libm::sqrtf((1.0 - t * t).max(0.0));
        (half * z, unit3([perp[0] * t, perp[1] * t, z]))
    }

    fn height(&self, form: &Form, p: P) -> f32 {
        match form {
            Form::Round => self.round(p).0,
            Form::Cylinder { axis } => self.cylinder(p, *axis).0,
            Form::Flat { .. } => 0.0,
        }
    }

    fn surface(&self, form: &Form, p: P) -> (f32, [f32; 3]) {
        match form {
            Form::Round => self.round(p),
            Form::Cylinder { axis } => self.cylinder(p, *axis),
            Form::Flat { facing } => (0.0, unit3(*facing)),
        }
    }

    fn light(&self, rules: &FormRules, p: P) -> (f32, f32, [f32; 3]) {
        let (h, n) = self.surface(&rules.form, p);
        let (l, falloff) = match rules.light {
            Lighting::Direction { toward } => (unit3(toward), 1.0),
            Lighting::Lamp { at } => {
                let v = [at[0] - p[0], at[1] - p[1], at[2] - h];
                let d = libm::sqrtf(dot3(v, v)).max(1.0);
                (unit3(v), ((self.reference / d) * (self.reference / d)).clamp(0.05, 3.0))
            }
        };
        (dot3(n, l), falloff, n)
    }

    fn colour(&self, rules: &FormRules, p: P) -> ([f32; 3], f32) {
        let (facing, falloff, n) = self.light(rules, p);
        let key = facing.max(0.0) * falloff;
        let sky = 0.5 * (1.0 - n[1]);
        let ground = 0.5 * (1.0 + n[1]);
        let core = match facing < 0.08 {
            true => 1.0 - rules.core * libm::expf(-((facing + 0.12) / 0.1).powi(2)),
            false => 1.0,
        };
        let rgb = [0, 1, 2]
            .map(|c| rules.colour[c] * (rules.key[c] * key + (rules.fill[c] * sky + rules.bounce[c] * ground) * core));
        let lit = facing * falloff;
        (rules.shadow.map_or(rgb, |s| s.tint(rgb, lit)), lit)
    }

    fn band(&self, rules: &FormRules, p: P) -> f32 {
        let (facing, falloff, _) = self.light(rules, p);
        facing * falloff
    }

    fn direction(&self, rules: &FormRules, p: P) -> P {
        let toward = |a: f32| [libm::cosf(a.to_radians()), libm::sinf(a.to_radians())];
        match rules.form {
            Form::Flat { .. } => toward(rules.angle),
            Form::Cylinder { axis } => toward(axis),
            Form::Round => {
                let e = 1.5;
                let gx = self.band(rules, [p[0] + e, p[1]]) - self.band(rules, [p[0] - e, p[1]]);
                let gy = self.band(rules, [p[0], p[1] + e]) - self.band(rules, [p[0], p[1] - e]);
                match libm::sqrtf(gx * gx + gy * gy) > 1e-4 {
                    true => {
                        let d = libm::sqrtf(gx * gx + gy * gy);
                        [-gy / d, gx / d]
                    }
                    false => {
                        let g = self.field.inward(p);
                        [-g[1], g[0]]
                    }
                }
            }
        }
    }

    fn trace(&self, rules: &FormRules, start: P, length: f32, limit: f32) -> Vec<P> {
        let level = self.band(rules, start);
        let walk = |sign: f32| {
            let mut out = Vec::new();
            let (mut p, mut prev) = (start, [0.0f32, 0.0f32]);
            let mut travelled = 0.0;
            while travelled < length / 2.0 {
                let mut d = self.direction(rules, p);
                if prev != [0.0, 0.0] && d[0] * prev[0] + d[1] * prev[1] < 0.0 {
                    d = [-d[0], -d[1]];
                }
                if prev == [0.0, 0.0] {
                    d = [d[0] * sign, d[1] * sign];
                }
                let next = [p[0] + d[0] * STEP, p[1] + d[1] * STEP];
                if self.field.at(next) < limit || (self.band(rules, next) - level).abs() > CUT {
                    break;
                }
                out.push(next);
                (p, prev) = (next, d);
                travelled += STEP;
            }
            out
        };
        let back = walk(-1.0);
        let ahead = walk(1.0);
        back.into_iter()
            .rev()
            .chain(std::iter::once(start))
            .chain(ahead)
            .collect()
    }
}

fn varied(rgb: [f32; 3], rng: &mut Rng, vary: f32) -> [f32; 3] {
    let k = 1.0 + rng.signed() * vary;
    let warm = rng.signed() * vary * 0.5;
    [rgb[0] * k * (1.0 + warm), rgb[1] * k, rgb[2] * k * (1.0 - warm)]
}

struct Pass {
    size: f32,
    spacing: f32,
    length: (f32, f32),
    bold: bool,
    opacity: f32,
    vary: f32,
    keep: fn(f32) -> bool,
    scatter: f32,
    hand: f32,
}

fn everywhere(_: f32) -> bool {
    true
}

fn turning(band: f32) -> bool {
    band.abs() < 0.25 || band > 0.85
}

fn lit(band: f32) -> bool {
    band > 0.35
}

fn shadowed(band: f32) -> bool {
    band < 0.02
}

pub fn paint_form(areas: &[Vec<P>], rules: &FormRules) -> Vec<FormStroke> {
    let w = rules.weight.max(1.0);
    let passes = [
        Pass {
            size: w * 2.0,
            spacing: 0.55,
            length: (2.5, 5.0),
            bold: false,
            opacity: 1.0,
            vary: 1.0,
            keep: everywhere,
            scatter: 0.0,
            hand: 0.35,
        },
        Pass {
            size: w,
            spacing: 0.85,
            length: (2.5, 5.0),
            bold: false,
            opacity: 0.95,
            vary: 1.0,
            keep: everywhere,
            scatter: 0.0,
            hand: 1.0,
        },
        Pass {
            size: w * 0.55,
            spacing: 0.85,
            length: (2.5, 5.0),
            bold: false,
            opacity: 0.9,
            vary: 0.8,
            keep: turning,
            scatter: 0.0,
            hand: 1.0,
        },
        Pass {
            size: w * 1.1,
            spacing: 0.0,
            length: (1.8, 3.5),
            bold: true,
            opacity: 0.9,
            vary: 1.4,
            keep: lit,
            scatter: 0.25,
            hand: 1.0,
        },
        Pass {
            size: w * 1.7,
            spacing: 0.0,
            length: (2.0, 4.0),
            bold: false,
            opacity: 0.6,
            vary: 0.6,
            keep: shadowed,
            scatter: 0.08,
            hand: 1.0,
        },
    ];
    let mut rng = Rng::new(rules.seed as u64 * 48_271 + 29);
    let (body, finish) = passes.split_at(3);
    let shapes = shapes_of(areas, rules);
    shapes
        .iter()
        .flat_map(|shape| {
            let area = area(shape.poly).abs();
            let mut out: Vec<FormStroke> = body
                .iter()
                .flat_map(|pass| pass_strokes(shape, rules, pass, area, w, &mut rng))
                .collect();
            if rules.spill {
                out.extend(edge_strokes(shape, rules, w, &mut rng));
            }
            out.extend(
                finish
                    .iter()
                    .flat_map(|pass| pass_strokes(shape, rules, pass, area, w, &mut rng)),
            );
            out
        })
        .collect()
}

fn pass_strokes(shape: &Shape, rules: &FormRules, pass: &Pass, area: f32, w: f32, rng: &mut Rng) -> Vec<FormStroke> {
    let seeds: Vec<P> = match pass.scatter > 0.0 {
        true => {
            let count = (area / (w * w) * pass.scatter * rules.decisive).round() as usize;
            (0..count)
                .map(|_| [rng.range(shape.lo[0], shape.hi[0]), rng.range(shape.lo[1], shape.hi[1])])
                .collect()
        }
        false => {
            let step = (pass.size * pass.spacing).max(1.5);
            let (nx, ny) = (
                ((shape.hi[0] - shape.lo[0]) / step) as usize + 1,
                ((shape.hi[1] - shape.lo[1]) / step) as usize + 1,
            );
            (0..ny)
                .flat_map(|j| (0..nx).map(move |i| (i, j)))
                .map(|(i, j)| {
                    [
                        shape.lo[0] + step * (i as f32 + 0.5) + rng.signed() * step * 0.4,
                        shape.lo[1] + step * (j as f32 + 0.5) + rng.signed() * step * 0.4,
                    ]
                })
                .collect()
        }
    };
    seeds
        .into_iter()
        .filter_map(|seed| {
            let length = rng.range(pass.length.0, pass.length.1) * pass.size;
            let size = pass.size * rng.range(0.85, 1.15);
            let margin = (size * 0.2).min(shape.reach * 0.5);
            let (start, limit) = match rules.spill {
                true => (0.0, -size * 0.5),
                false => (margin, margin),
            };
            if shape.field.at(seed) < start || !(pass.keep)(shape.band(rules, seed)) {
                return None;
            }
            let line = shape.trace(rules, seed, length, limit);
            if line.len() < 3 {
                return None;
            }
            let (rgb, _) = shape.colour(rules, line[line.len() / 2]);
            Some(FormStroke {
                run: line.iter().map(|p| [p[0], p[1], 1.0]).collect(),
                rgb: varied(rgb, rng, rules.vary * pass.vary),
                size,
                bold: pass.bold,
                opacity: pass.opacity,
                hand: pass.hand,
            })
        })
        .collect::<Vec<_>>()
}

fn edge_strokes(shape: &Shape, rules: &FormRules, w: f32, rng: &mut Rng) -> Vec<FormStroke> {
    let poly = shape.poly;
    let points: Vec<P> = (0..poly.len())
        .flat_map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let n = (len(sub(b, a)) / STEP).ceil().max(1.0) as usize;
            (0..n).map(move |k| {
                let t = k as f32 / n as f32;
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
            })
        })
        .collect();
    let size = w * 0.9;
    let per = ((size * 3.0) / STEP).max(3.0) as usize;
    (0..points.len())
        .step_by((per / 3).max(1))
        .filter_map(|start| {
            let level = shape.band(rules, points[start]);
            let line: Vec<P> = (0..per)
                .map(|k| points[(start + k) % points.len()])
                .take_while(|p| (shape.band(rules, *p) - level).abs() <= CUT)
                .collect();
            if line.len() < 3 {
                return None;
            }
            let (rgb, _) = shape.colour(rules, line[line.len() / 2]);
            Some(FormStroke {
                run: line.iter().map(|p| [p[0], p[1], 1.0]).collect(),
                rgb: varied(rgb, rng, rules.vary),
                size: size * rng.range(0.85, 1.15),
                bold: false,
                opacity: 0.95,
                hand: 0.6,
            })
        })
        .collect()
}

fn shapes_of<'a>(areas: &'a [Vec<P>], rules: &FormRules) -> Vec<Shape<'a>> {
    let mut shapes: Vec<Shape> = areas
        .iter()
        .filter(|a| a.len() >= 3)
        .map(|a| Shape::new(a, rules))
        .collect();
    let nearest = shapes.iter().map(|s| s.reference).fold(f32::MAX, f32::min);
    shapes.iter_mut().for_each(|s| s.reference = nearest);
    shapes
}

pub fn shade_at(areas: &[Vec<P>], rules: &FormRules, p: P) -> Option<([f32; 3], f32)> {
    shapes_of(areas, rules)
        .iter()
        .find(|s| inside(s.poly, p))
        .map(|s| s.colour(rules, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(cx: f32, cy: f32, r: f32) -> Vec<P> {
        (0..96)
            .map(|i| {
                let a = core::f32::consts::TAU * i as f32 / 96.0;
                [cx + r * libm::cosf(a), cy + r * libm::sinf(a)]
            })
            .collect()
    }

    fn rules(light: Lighting) -> FormRules {
        FormRules {
            form: Form::Round,
            light,
            colour: [0.6, 0.6, 0.6],
            key: [1.0, 0.95, 0.9],
            fill: [0.09, 0.1, 0.13],
            bounce: [0.08, 0.06, 0.05],
            core: 0.35,
            weight: 10.0,
            angle: 30.0,
            decisive: 1.0,
            vary: 0.03,
            seed: 4,
            spill: false,
            shadow: None,
        }
    }

    fn luma(rgb: [f32; 3]) -> f32 {
        0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
    }

    #[test]
    fn a_round_form_is_lit_on_the_light_side_and_its_strokes_stay_inside_and_on_one_band() {
        let shape = disc(100.0, 100.0, 60.0);
        let r = rules(Lighting::Direction {
            toward: [-1.0, 0.0, 0.5],
        });
        let strokes = paint_form(std::slice::from_ref(&shape), &r);
        assert!(strokes.len() > 40);
        assert!(
            strokes
                .iter()
                .all(|s| s.run.iter().all(|q| inside(&shape, [q[0], q[1]])))
        );
        let side = |left: bool| {
            let picked: Vec<f32> = strokes
                .iter()
                .filter(|s| {
                    !s.bold && (s.run[s.run.len() / 2][0] < 70.0) == left && (s.run[s.run.len() / 2][0] > 130.0) != left
                })
                .map(|s| luma(s.rgb))
                .collect();
            picked.iter().sum::<f32>() / picked.len().max(1) as f32
        };
        assert!(side(true) > side(false) * 2.0);
        let form = Shape::new(&shape, &r);
        assert!(strokes.iter().all(|s| {
            let bands: Vec<f32> = s.run.iter().map(|q| form.band(&r, [q[0], q[1]])).collect();
            bands.iter().cloned().fold(f32::MIN, f32::max) - bands.iter().cloned().fold(f32::MAX, f32::min)
                <= 2.0 * CUT + 0.02
        }));
        assert!(strokes.iter().any(|s| s.bold) && strokes.iter().any(|s| !s.bold));
        assert_eq!(strokes, paint_form(&[shape], &r));
    }

    #[test]
    fn on_a_locked_or_clipped_layer_strokes_run_past_the_edge_and_the_edge_is_cut_in() {
        let shape = disc(100.0, 100.0, 60.0);
        let r = FormRules {
            spill: true,
            ..rules(Lighting::Direction {
                toward: [-1.0, 0.0, 0.5],
            })
        };
        let strokes = paint_form(std::slice::from_ref(&shape), &r);
        let off = |q: &[f32; 3]| libm::sqrtf((q[0] - 100.0).powi(2) + (q[1] - 100.0).powi(2)) - 60.0;
        assert!(strokes.iter().any(|s| s.run.iter().any(|q| off(q) > 1.0)));
        assert!(strokes.iter().all(|s| s.run.iter().all(|q| off(q) < s.size)));
        assert!(
            strokes
                .iter()
                .filter(|s| s.run.iter().all(|q| off(q).abs() < 1.0))
                .count()
                > 20
        );
    }

    #[test]
    fn a_lamp_lights_the_near_form_more_and_a_flat_form_is_even() {
        let (near, far) = (disc(100.0, 100.0, 30.0), disc(400.0, 100.0, 30.0));
        let r = rules(Lighting::Lamp {
            at: [20.0, 100.0, 60.0],
        });
        let both = paint_form(&[near, far], &r);
        let mean = |left: bool| {
            let picked: Vec<f32> = both
                .iter()
                .filter(|s| (s.run[0][0] < 250.0) == left)
                .map(|s| luma(s.rgb))
                .collect();
            picked.iter().sum::<f32>() / picked.len().max(1) as f32
        };
        assert!(mean(true) > mean(false) * 1.5);
        let flat = FormRules {
            form: Form::Flat {
                facing: [0.0, -1.0, 1.0],
            },
            ..rules(Lighting::Direction {
                toward: [0.0, -1.0, 1.0],
            })
        };
        let square = vec![[0.0, 0.0], [80.0, 0.0], [80.0, 80.0], [0.0, 80.0]];
        let tones: Vec<f32> = paint_form(&[square], &flat)
            .iter()
            .filter(|s| !s.bold && s.opacity > 0.9)
            .map(|s| luma(s.rgb))
            .collect();
        let (lo, hi) = (
            tones.iter().cloned().fold(f32::MAX, f32::min),
            tones.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(hi / lo < 1.25);
    }

    #[test]
    fn a_shadow_colour_turns_the_shadow_side_towards_its_hue_and_keeps_the_values() {
        let shape = disc(100.0, 100.0, 60.0);
        let plain = FormRules {
            colour: color::to_linear(color::parse_hex("#c0603c").unwrap()),
            ..rules(Lighting::Direction {
                toward: [-1.0, 0.0, 0.5],
            })
        };
        let lavender = color::parse_hex("#a898d0").unwrap();
        let tinted = FormRules {
            shadow: Some(Shadow::of(lavender, 0.8)),
            ..plain
        };
        let at = |r: &FormRules, x: f32| shade_at(std::slice::from_ref(&shape), r, [x, 100.0]).unwrap();
        let lab = |rgb: [f32; 3]| color::to_oklab(color::from_linear(rgb, 1.0));
        let hue = |l: [f32; 3]| libm::atan2f(l[2], l[1]);
        let (lit, facing) = at(&plain, 55.0);
        assert!(facing > 0.3);
        assert_eq!(at(&tinted, 55.0).0, lit);
        let (dark, facing) = at(&plain, 150.0);
        assert!(facing < -0.1);
        let (before, after, target) = (lab(dark), lab(at(&tinted, 150.0).0), color::to_oklab(lavender));
        assert!((before[0] - after[0]).abs() < 0.02, "{before:?} {after:?}");
        let off = |l: [f32; 3]| (hue(l) - hue(target)).abs();
        assert!(off(after) < off(before) * 0.4, "{before:?} {after:?} {target:?}");
        let chroma = |l: [f32; 3]| libm::sqrtf(l[1] * l[1] + l[2] * l[2]);
        assert!(chroma(after) > 0.6 * chroma(before).min(chroma(target)));
        let jump = |r: &FormRules| {
            let steps: Vec<[f32; 3]> = (0..=50).map(|i| lab(at(r, 50.0 + 2.0 * i as f32).0)).collect();
            steps
                .windows(2)
                .map(|w| libm::sqrtf((0..3).map(|c| (w[1][c] - w[0][c]).powi(2)).sum()))
                .fold(0.0f32, f32::max)
        };
        assert!(
            jump(&tinted) < jump(&plain) * 1.5,
            "largest step {} against {}",
            jump(&tinted),
            jump(&plain)
        );
    }

    #[test]
    fn a_cylinder_is_painted_along_its_axis() {
        let tube = vec![[0.0, 0.0], [40.0, 0.0], [40.0, 160.0], [0.0, 160.0]];
        let r = FormRules {
            form: Form::Cylinder { axis: 90.0 },
            ..rules(Lighting::Direction {
                toward: [-1.0, -0.2, 0.6],
            })
        };
        let strokes = paint_form(&[tube], &r);
        assert!(strokes.iter().filter(|s| s.run.len() > 6).all(|s| {
            let (a, b) = (s.run[0], s.run[s.run.len() - 1]);
            (b[1] - a[1]).abs() > (b[0] - a[0]).abs() * 3.0
        }));
    }
}
