use crate::layers::{self, Vectors};
use crate::shapes::areas;
use anyhow::{Context, Result, anyhow, bail};
use klad_brush::BrushSet;
use klad_core::bundle::Bundle;
use klad_core::color;
use klad_core::doc::{Document, Kind, Layer, Loc};
use klad_core::ops::{Op, PaintOverSpec, StrokeSpec};
use klad_core::rng::Rng;
use klad_core::stroke::{Mark, Stroke};
use klad_core::tween::{self, Lines, Match};
use klad_render::{Rasters, flatten};
use rayon::prelude::*;
use serde_json::json;

type Lab = [f32; 3];
type P = [f32; 2];

const EDGE: f32 = 0.12;

pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub lab: Vec<Lab>,
    pub alpha: Vec<f32>,
}

impl Picture {
    fn index(&self, x: f32, y: f32) -> Option<usize> {
        let (xi, yi) = (x.floor(), y.floor());
        (xi >= 0.0 && yi >= 0.0 && (xi as usize) < self.w && (yi as usize) < self.h)
            .then(|| yi as usize * self.w + xi as usize)
    }
}

pub fn difference(a: Lab, b: Lab) -> f32 {
    100.0 * ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn box_pass(data: &[[f32; 4]], w: usize, h: usize, r: usize, rows: bool) -> Vec<[f32; 4]> {
    let (lines, len) = if rows { (h, w) } else { (w, h) };
    let at = move |line: usize, i: usize| if rows { line * w + i } else { i * w + line };
    let mut out = vec![[0.0; 4]; w * h];
    let done: Vec<Vec<(usize, [f32; 4])>> = (0..lines)
        .into_par_iter()
        .map(|line| {
            let mut sum = [0.0f32; 4];
            let span = (2 * r + 1) as f32;
            let get = |i: isize| data[at(line, i.clamp(0, len as isize - 1) as usize)];
            (-(r as isize)..=r as isize).for_each(|i| {
                let v = get(i);
                (0..4).for_each(|c| sum[c] += v[c]);
            });
            (0..len)
                .map(|i| {
                    let value = sum.map(|s| s / span);
                    let (add, drop) = (get(i as isize + r as isize + 1), get(i as isize - r as isize));
                    (0..4).for_each(|c| sum[c] += add[c] - drop[c]);
                    (at(line, i), value)
                })
                .collect()
        })
        .collect();
    done.into_iter().flatten().for_each(|(i, v)| out[i] = v);
    out
}

pub fn blur(picture: &Picture, radius: f32) -> Picture {
    let r = (radius * 0.5).round() as usize;
    let mut data: Vec<[f32; 4]> = picture
        .lab
        .iter()
        .zip(&picture.alpha)
        .map(|(l, a)| [l[0], l[1], l[2], *a])
        .collect();
    if r > 0 {
        (0..3).for_each(|_| {
            data = box_pass(&data, picture.w, picture.h, r, true);
            data = box_pass(&data, picture.w, picture.h, r, false);
        });
    }
    Picture {
        w: picture.w,
        h: picture.h,
        lab: data.iter().map(|v| [v[0], v[1], v[2]]).collect(),
        alpha: data.iter().map(|v| v[3]).collect(),
    }
}

fn scanline(polygons: &[Vec<P>], w: usize, h: usize) -> Vec<bool> {
    let mut mask = vec![false; w * h];
    (0..h).for_each(|y| {
        let fy = y as f32 + 0.5;
        let mut xs: Vec<f32> = polygons
            .iter()
            .flat_map(|poly| {
                (0..poly.len()).filter_map(move |i| {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    ((a[1] > fy) != (b[1] > fy)).then(|| a[0] + (fy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
                })
            })
            .collect();
        xs.sort_by(f32::total_cmp);
        xs.chunks(2).filter(|c| c.len() == 2).for_each(|c| {
            let (x0, x1) = (
                (c[0] - 0.5).ceil().max(0.0) as usize,
                ((c[1] - 0.5).floor() + 1.0).clamp(0.0, w as f32) as usize,
            );
            (x0..x1).for_each(|x| mask[y * w + x] = true);
        });
    });
    mask
}

fn grow(mask: &[bool], w: usize, h: usize, by: f32) -> Vec<bool> {
    let picture = Picture {
        w,
        h,
        lab: vec![[0.0; 3]; w * h],
        alpha: mask.iter().map(|&m| if m { 1.0 } else { 0.0 }).collect(),
    };
    blur(&picture, by * 2.0).alpha.iter().map(|&a| a > 0.001).collect()
}

fn family(layer: &Layer) -> Vec<&Layer> {
    std::iter::once(layer)
        .chain(layer.children().iter().flat_map(family))
        .collect()
}

fn layers_of<'a>(doc: &'a Document, spec: &PaintOverSpec) -> Result<Vec<&'a Layer>> {
    if !spec.from.is_empty() {
        return spec.from.iter().map(|key| doc.find_layer(key)).collect();
    }
    let target = doc.find_layer(&spec.layer)?;
    let top = doc
        .layers
        .iter()
        .position(|l| family(l).iter().any(|c| c.id == target.id))
        .ok_or_else(|| anyhow!("paint over: {} is not in the layer stack", spec.layer))?;
    Ok(doc.layers[..top].iter().filter(|l| l.visible && !l.reference).collect())
}

pub fn source(bundle: &Bundle, spec: &PaintOverSpec, frame: u32) -> Result<Picture> {
    let doc = &bundle.doc;
    let chosen = layers_of(doc, spec)?;
    if chosen.is_empty() {
        bail!("paint over: nothing below {} to paint from; give --from", spec.layer);
    }
    let brushes = BrushSet::for_bundle(bundle)?;
    let mut rasters = Rasters::new();
    doc.paint_content_at(frame)
        .iter()
        .try_for_each(|(layer, cel)| -> Result<()> {
            rasters.insert(cel.id.clone(), layers::load_one(bundle, &brushes, &layer.id, &cel.id)?);
            Ok(())
        })?;
    Vectors::default().add_to(doc, frame, &mut rasters)?;
    let background = spec
        .from
        .is_empty()
        .then_some(doc.canvas.background.as_deref())
        .flatten();
    let flat = flatten(doc, frame, &rasters, &chosen, background)?;
    let (w, h) = (doc.canvas.width as usize, doc.canvas.height as usize);
    let mut lab = vec![[0.0; 3]; w * h];
    let mut alpha = vec![0.0; w * h];
    flat.occupied().for_each(|(index, tile)| {
        let (ox, oy) = (
            (index % flat.cols) * klad_brush::TILE,
            (index / flat.cols) * klad_brush::TILE,
        );
        tile.chunks(4).enumerate().for_each(|(i, px)| {
            let (x, y) = (ox + i % klad_brush::TILE, oy + i / klad_brush::TILE);
            if x >= w || y >= h {
                return;
            }
            let a = px[3].to_f32();
            if a <= 1e-4 {
                return;
            }
            let rgb = [px[0].to_f32() / a, px[1].to_f32() / a, px[2].to_f32() / a, 1.0];
            lab[y * w + x] = color::to_oklab(rgb);
            alpha[y * w + x] = a;
        });
    });
    Ok(Picture { w, h, lab, alpha })
}

struct Canvas {
    lab: Vec<Lab>,
    painted: Vec<bool>,
}

impl Canvas {
    fn new(n: usize) -> Self {
        Self {
            lab: vec![[0.0; 3]; n],
            painted: vec![false; n],
        }
    }

    fn error(&self, reference: &Picture, i: usize) -> f32 {
        match self.painted[i] {
            true => difference(self.lab[i], reference.lab[i]),
            false => 100.0,
        }
    }

    fn paint(&mut self, w: usize, h: usize, points: &[P], radius: f32, lab: Lab) {
        let r = radius.max(0.5);
        let step = (r * 0.5).max(0.5);
        let dense: Vec<P> = points
            .windows(2)
            .flat_map(|s| {
                let (a, b) = (s[0], s[1]);
                let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().max(1.0) as usize;
                (0..n).map(move |k| {
                    let t = k as f32 / n as f32;
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                })
            })
            .chain(points.last().copied())
            .collect();
        dense.iter().for_each(|c| {
            let (x0, x1) = (
                (c[0] - r).floor().max(0.0) as usize,
                ((c[0] + r).ceil() as usize).min(w),
            );
            let (y0, y1) = (
                (c[1] - r).floor().max(0.0) as usize,
                ((c[1] + r).ceil() as usize).min(h),
            );
            (y0..y1).for_each(|y| {
                (x0..x1).for_each(|x| {
                    if (x as f32 + 0.5 - c[0]).hypot(y as f32 + 0.5 - c[1]) <= r {
                        self.lab[y * w + x] = lab;
                        self.painted[y * w + x] = true;
                    }
                })
            })
        });
    }
}

fn lightness(p: &Picture, x: isize, y: isize) -> f32 {
    let (x, y) = (
        x.clamp(0, p.w as isize - 1) as usize,
        y.clamp(0, p.h as isize - 1) as usize,
    );
    p.lab[y * p.w + x][0] * p.alpha[y * p.w + x].min(1.0)
}

fn gradient(p: &Picture, at: P) -> P {
    let (x, y) = (at[0].floor() as isize, at[1].floor() as isize);
    let l = |dx: isize, dy: isize| lightness(p, x + dx, y + dy);
    let gx = (l(1, -1) + 2.0 * l(1, 0) + l(1, 1)) - (l(-1, -1) + 2.0 * l(-1, 0) + l(-1, 1));
    let gy = (l(-1, 1) + 2.0 * l(0, 1) + l(1, 1)) - (l(-1, -1) + 2.0 * l(0, -1) + l(1, -1));
    [gx, gy]
}

fn drift(at: P, seed: u32) -> P {
    let cell = 220.0;
    let (x, y) = (at[0] / cell, at[1] / cell);
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let h = |dx: f32, dy: f32| klad_core::rng::hash2_unit((ix + dx) as i32, (iy + dy) as i32, seed);
    let top = h(0.0, 0.0) + (h(1.0, 0.0) - h(0.0, 0.0)) * sx;
    let bottom = h(0.0, 1.0) + (h(1.0, 1.0) - h(0.0, 1.0)) * sx;
    let base = (klad_core::rng::hash2_unit(0, 0, seed ^ 0x5eed) - 0.5) * 0.6;
    let angle = base + (top + (bottom - top) * sy - 0.5) * 0.9;
    [angle.cos(), angle.sin()]
}

pub struct Painted {
    pub points: Vec<P>,
    pub size: f32,
    pub lab: Lab,
}

struct Rules<'a> {
    reference: &'a Picture,
    canvas: &'a Canvas,
    region: &'a [bool],
    radius: f32,
    steps: [usize; 2],
    curve: f32,
    threshold: f32,
    seed: u32,
}

fn walk(rules: &Rules, start: P) -> Option<Painted> {
    let r = rules.reference;
    let i0 = r.index(start[0], start[1])?;
    let colour = r.lab[i0];
    let mut points = vec![start];
    let mut last: Option<P> = None;
    for step in 1..=rules.steps[1] {
        let at = *points.last()?;
        let Some(i) = r.index(at[0], at[1]) else {
            break;
        };
        let here = r.lab[i];
        if step > rules.steps[0]
            && difference(here, rules.canvas.lab[i]) < difference(here, colour)
            && rules.canvas.painted[i]
        {
            break;
        }
        if difference(here, colour) > rules.threshold * 2.0 {
            break;
        }
        let g = gradient(r, at);
        let length = g[0].hypot(g[1]);
        let sure = (length / EDGE).clamp(0.0, 1.0);
        let along = match length > 1e-6 {
            true => [-g[1] / length, g[0] / length],
            false => drift(at, rules.seed),
        };
        let prior = last.unwrap_or_else(|| drift(at, rules.seed));
        let along = match along[0] * prior[0] + along[1] * prior[1] < 0.0 {
            true => [-along[0], -along[1]],
            false => along,
        };
        let k = match last {
            Some(_) => rules.curve * sure,
            None => sure,
        };
        let mut d = [k * along[0] + (1.0 - k) * prior[0], k * along[1] + (1.0 - k) * prior[1]];
        let n = d[0].hypot(d[1]).max(1e-6);
        d = [d[0] / n, d[1] / n];
        let next = [at[0] + rules.radius * d[0], at[1] + rules.radius * d[1]];
        match r.index(next[0], next[1]) {
            Some(j) if rules.region[j] && difference(r.lab[j], colour) <= rules.threshold * 2.0 => points.push(next),
            _ => break,
        }
        last = Some(d);
    }
    Some(Painted {
        points,
        size: rules.radius * 2.0,
        lab: colour,
    })
}

pub struct Plan<'a> {
    pub sizes: &'a [f32],
    pub threshold: f32,
    pub length: [f32; 2],
    pub curve: f32,
    pub seed: u32,
    pub region: &'a [bool],
    pub focus: Option<&'a [bool]>,
}

pub fn paint(picture: &Picture, plan: &Plan, canvas_from: &[Painted]) -> Vec<Painted> {
    let (w, h) = (picture.w, picture.h);
    let mut canvas = Canvas::new(w * h);
    canvas_from
        .iter()
        .for_each(|s| canvas.paint(w, h, &s.points, s.size * 0.5, s.lab));
    let mut rng = Rng::new(plan.seed as u64 ^ 0x9a17);
    let last = plan.sizes.len().saturating_sub(1);
    let mut out = Vec::new();
    plan.sizes.iter().enumerate().for_each(|(pass, &size)| {
        let radius = (size * 0.5).max(1.0);
        let reference = blur(picture, radius);
        let grid = radius.max(1.0) as usize;
        let mut starts: Vec<P> = (0..h.div_ceil(grid))
            .flat_map(|gy| (0..w.div_ceil(grid)).map(move |gx| (gx * grid, gy * grid)))
            .filter_map(|(x0, y0)| {
                let cells: Vec<usize> = (y0..(y0 + grid).min(h))
                    .flat_map(|y| (x0..(x0 + grid).min(w)).map(move |x| y * w + x))
                    .filter(|&i| plan.region[i] && (pass != last || plan.focus.is_none_or(|f| f[i])))
                    .collect();
                if cells.is_empty() {
                    return None;
                }
                let errors: Vec<f32> = cells.iter().map(|&i| canvas.error(&reference, i)).collect();
                let mean = errors.iter().sum::<f32>() / errors.len() as f32;
                (mean > plan.threshold).then(|| {
                    let worst = errors
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .map_or(cells[0], |(k, _)| cells[k]);
                    [(worst % w) as f32 + 0.5, (worst / w) as f32 + 0.5]
                })
            })
            .collect();
        (0..starts.len()).rev().for_each(|i| {
            let j = (rng.unit() * (i + 1) as f32) as usize;
            starts.swap(i, j.min(i));
        });
        let rules = Rules {
            reference: &reference,
            canvas: &canvas,
            region: plan.region,
            radius,
            steps: [
                (plan.length[0] * 2.0).round().max(0.0) as usize,
                (plan.length[1] * 2.0).round().max(1.0) as usize,
            ],
            curve: plan.curve,
            threshold: plan.threshold,
            seed: plan.seed.wrapping_add(pass as u32),
        };
        let made: Vec<Painted> = starts.par_iter().filter_map(|&s| walk(&rules, s)).collect();
        made.iter()
            .for_each(|s| canvas.paint(w, h, &s.points, s.size * 0.5, s.lab));
        out.extend(made);
    });
    out
}

fn pressure(n: usize, k: usize) -> f32 {
    let t = if n <= 1 { 0.5 } else { k as f32 / (n - 1) as f32 };
    let smooth = |a: f32, b: f32, x: f32| {
        let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    };
    (0.4 + 0.6 * smooth(0.0, 0.25, t)) * (1.0 - 0.75 * smooth(0.65, 1.0, t))
}

fn hex(lab: Lab, seed: u32, vary: f32) -> String {
    let jitter = |k: i32| klad_core::rng::hash2_unit(seed as i32, k, 0x7a11) * 2.0 - 1.0;
    let shifted = [
        lab[0] + vary * 0.6 * jitter(1),
        lab[1] + vary * 0.3 * jitter(2),
        lab[2] + vary * 0.6 * jitter(3),
    ];
    color::to_hex(color::from_oklab(shifted, 1.0))
}

fn stroke_op(
    spec: &PaintOverSpec,
    part: &str,
    points: &[P],
    size: f32,
    colour: String,
    opacity: f32,
    seed: u32,
) -> Result<Op> {
    let run: Vec<[f32; 3]> = match points.len() {
        1 => vec![
            [points[0][0] - size * 0.15, points[0][1], 0.7],
            [points[0][0] + size * 0.15, points[0][1], 0.7],
        ],
        n => points
            .iter()
            .enumerate()
            .map(|(k, p)| [p[0], p[1], pressure(n, k)])
            .collect(),
    };
    let spec: StrokeSpec = serde_json::from_value(json!({
        "layer": spec.layer,
        "frame": spec.frame,
        "brush": spec.brush,
        "color": colour,
        "size": size,
        "opacity": opacity,
        "points": run,
        "seed": seed,
        "part": part,
    }))?;
    Ok(Op::Stroke(spec))
}

fn region(doc: &Document, spec: &PaintOverSpec, picture: &Picture) -> Result<Vec<bool>> {
    let alpha = picture.alpha.iter().map(|&a| a > 0.5);
    match spec.within.is_empty() {
        true => Ok(alpha.collect()),
        false => {
            let polygons = areas(doc, &spec.within, &None, "paint over")?;
            Ok(scanline(&polygons, picture.w, picture.h)
                .into_iter()
                .zip(alpha)
                .map(|(a, b)| a && b)
                .collect())
        }
    }
}

struct Like {
    strokes: Vec<(Stroke, Vec<P>, bool)>,
    remove: Vec<String>,
    from: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Rides {
    Line(usize),
    Nearest,
    Still,
    Gone,
}

#[derive(Default)]
struct Sources {
    from: Lines,
    to: Lines,
    marks: Vec<(Mark, Rides)>,
}

fn covers(mark: &Mark, at: P, reach: f32) -> bool {
    match mark {
        Mark::Stroke(s) => {
            let line = tween::line_of(s);
            let near = s.size * 0.5 + reach;
            match line.len() {
                0 => false,
                1 => (line[0].0 - at[0]).hypot(line[0].1 - at[1]) <= near,
                _ => line.windows(2).any(|w| {
                    let (a, b) = ([w[0].0, w[0].1], [w[1].0, w[1].1]);
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let t =
                        (((at[0] - a[0]) * dx + (at[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                    (a[0] + dx * t - at[0]).hypot(a[1] + dy * t - at[1]) <= near
                }),
            }
        }
        Mark::Fill(f) => match &f.source {
            klad_core::fill::FillSource::Polygon(points) => crate::shapes::inside(std::slice::from_ref(points), at),
            klad_core::fill::FillSource::Seed { .. } | klad_core::fill::FillSource::Cells { .. } => {
                at[0] >= 0.0 && at[1] >= 0.0 && f.coverage.at(at[0] as u32, at[1] as u32) > 0
            }
        },
    }
}

impl Sources {
    fn rides(&self, at: P) -> Rides {
        self.marks
            .iter()
            .rev()
            .find(|(m, _)| covers(m, at, 1.0))
            .map_or(Rides::Still, |(_, r)| *r)
    }
}

fn outline_of(mark: &Mark) -> Option<Vec<(f32, f32)>> {
    match mark {
        Mark::Stroke(s) => Some(tween::line_of(s)),
        Mark::Fill(f) => match &f.source {
            klad_core::fill::FillSource::Polygon(points) => {
                Some(points.iter().chain(points.first()).map(|p| (p[0], p[1])).collect())
            }
            klad_core::fill::FillSource::Seed { .. } | klad_core::fill::FillSource::Cells { .. } => None,
        },
    }
}

fn sources(bundle: &Bundle, spec: &PaintOverSpec, from: u32) -> Result<Sources> {
    let doc = &bundle.doc;
    let mut out = Sources::default();
    layers_of(doc, spec)?
        .into_iter()
        .flat_map(family)
        .filter(|l| l.kind() == Kind::Paint)
        .try_for_each(|layer| -> Result<()> {
            let Some(a) = layer.content_at(from) else {
                return Ok(());
            };
            let ma = bundle.read_marks(&layer.id, &a.id)?;
            let b = layer.content_at(spec.frame);
            if b.is_some_and(|b| b.id == a.id) {
                out.marks.extend(ma.into_iter().map(|m| (m, Rides::Still)));
                return Ok(());
            }
            let Some(b) = b else {
                out.marks.extend(ma.into_iter().map(|m| (m, Rides::Gone)));
                return Ok(());
            };
            let mb = bundle.read_marks(&layer.id, &b.id)?;
            let parts = tween::Parts {
                a: a.parts.clone(),
                b: b.parts.clone(),
            };
            let pairing = tween::pair_parts(&ma, &mb, Match::Id, &[], &parts);
            ma.into_iter().for_each(|m| {
                let pair = pairing.pairs.iter().find(|p| p.a == m.id());
                let rides = match pair {
                    None => Rides::Gone,
                    Some(p) => {
                        let other = mb.iter().find(|n| n.id() == p.b);
                        match (outline_of(&m), other.and_then(outline_of)) {
                            (Some(la), Some(mut lb)) => {
                                if p.reversed {
                                    lb.reverse();
                                }
                                out.from.push(la);
                                out.to.push(lb);
                                Rides::Line(out.from.len() - 1)
                            }
                            _ => Rides::Nearest,
                        }
                    }
                };
                out.marks.push((m, rides));
            });
            Ok(())
        })?;
    Ok(out)
}

fn like(bundle: &Bundle, spec: &PaintOverSpec, cel: &str) -> Result<Like> {
    let doc = &bundle.doc;
    let layer = doc.find_layer(&spec.layer)?;
    match doc.locate(cel) {
        Some(Loc::Cel(path, _)) if doc.layer_at(&path).id == layer.id => {}
        _ => bail!("paint over --like {cel}: not a drawing on {}", spec.layer),
    }
    let from = (0..doc.timeline.frames.max(1))
        .find(|&f| layer.content_at(f).is_some_and(|c| c.id == cel))
        .ok_or_else(|| anyhow!("paint over --like {cel}: that drawing is not shown on any frame"))?;
    if layer.content_at(spec.frame).is_some_and(|c| c.id == cel) {
        bail!(
            "paint over --like {cel}: frame {} shows that drawing itself; make a new drawing there first",
            spec.frame
        );
    }
    let marks = bundle.read_marks(&layer.id, cel)?;
    let owner = layer.cels().iter().find(|c| c.id == cel).context("no such drawing")?;
    let ids: Option<&Vec<u32>> = owner.parts.get(&spec.part);
    let found = sources(bundle, spec, from)?;
    let strokes = marks
        .iter()
        .filter_map(Mark::as_stroke)
        .filter(|s| ids.is_none_or(|ids| ids.contains(&s.id)))
        .map(|s| {
            let line = tween::line_of(s);
            let n = line.len().max(1) as f32;
            let centre = line.iter().fold([0.0, 0.0], |c, p| [c[0] + p.0 / n, c[1] + p.1 / n]);
            let mark = Mark::Stroke(s.clone());
            let rides = found.rides(centre);
            let moved = match rides {
                Rides::Line(i) => tween::carry(&mark, &found.from, &found.to, Some(&[i])),
                Rides::Nearest => tween::carry(&mark, &found.from, &found.to, None),
                Rides::Still | Rides::Gone => mark,
            };
            let points = moved.as_stroke().map_or_else(Vec::new, |m| {
                m.samples
                    .iter()
                    .map(|q| {
                        let (x, y) = q.pos();
                        [x, y]
                    })
                    .collect()
            });
            (s.clone(), points, rides != Rides::Gone)
        })
        .collect();
    let remove = layer
        .content_at(spec.frame)
        .map(|c| {
            let added = format!("{} added", spec.part);
            c.parts
                .iter()
                .filter(|(name, _)| **name == spec.part || name.starts_with(&added))
                .flat_map(|(_, ids)| ids)
                .map(|id| format!("{}:s{id}", c.id))
                .collect()
        })
        .unwrap_or_default();
    Ok(Like { strokes, remove, from })
}

pub fn expand(bundle: &Bundle, spec: &PaintOverSpec) -> Result<Vec<Op>> {
    let doc = &bundle.doc;
    let target = doc.find_layer(&spec.layer)?;
    if target.kind() != Kind::Paint {
        bail!("paint over draws on a paint layer; {} is not one", spec.layer);
    }
    if spec.sizes.is_empty() || spec.sizes.iter().any(|s| *s <= 0.0) {
        bail!("paint over needs brush sizes above 0, biggest first: --sizes 32,16,8");
    }
    let picture = source(bundle, spec, spec.frame)?;
    let region = region(doc, spec, &picture)?;
    if !region.iter().any(|r| *r) {
        bail!("paint over: the source has no paint where it would draw");
    }
    let focus = spec
        .focus
        .as_ref()
        .map(|id| -> Result<Vec<bool>> {
            let polygons = areas(doc, std::slice::from_ref(id), &None, "paint over --focus")?;
            let near = spec.sizes.last().copied().unwrap_or(8.0) * 3.0;
            Ok(grow(
                &scanline(&polygons, picture.w, picture.h),
                picture.w,
                picture.h,
                near,
            ))
        })
        .transpose()?;
    let plan = Plan {
        sizes: &spec.sizes,
        threshold: spec.threshold,
        length: spec.length,
        curve: spec.curve.clamp(0.0, 1.0),
        seed: spec.seed,
        region: &region,
        focus: focus.as_deref(),
    };
    let mut rng = Rng::new(spec.seed as u64 ^ 0xc0102);
    let mut ops = Vec::new();
    let mut kept = Vec::new();
    if let Some(cel) = &spec.like {
        let carried = like(bundle, spec, cel)?;
        if !carried.remove.is_empty() {
            ops.extend(klad_core::ops::parse_lines(
                &json!({"op": "rm", "ids": carried.remove}).to_string(),
            )?);
        }
        let mut blurred: std::collections::BTreeMap<u32, Picture> = std::collections::BTreeMap::new();
        carried
            .strokes
            .iter()
            .try_for_each(|(stroke, points, alive)| -> Result<()> {
                let key = (stroke.size * 0.5).round() as u32;
                let reference = blurred.entry(key).or_insert_with(|| blur(&picture, stroke.size * 0.5));
                let at = points.first().copied().unwrap_or([0.0, 0.0]);
                let found = reference.index(at[0], at[1]).filter(|&i| region[i] && *alive);
                let crosses = found.is_some_and(|i| {
                    let step = (points.len() / 6).max(1);
                    points.iter().step_by(step).any(|p| {
                        reference.index(p[0], p[1]).is_none_or(|j| {
                            !region[j] || difference(reference.lab[j], reference.lab[i]) > spec.threshold * 2.5
                        })
                    })
                });
                let found = found.filter(|_| !crosses);
                let (lab, opacity) = match found {
                    Some(i) => (reference.lab[i], stroke.opacity),
                    None => (color::to_oklab(color::from_u16(stroke.color)), 0.0),
                };
                ops.push(stroke_op(
                    spec,
                    &spec.part,
                    points,
                    stroke.size,
                    hex(lab, stroke.seed, spec.vary),
                    opacity,
                    stroke.seed,
                )?);
                if opacity > 0.0 {
                    kept.push(Painted {
                        points: points.clone(),
                        size: stroke.size,
                        lab,
                    });
                }
                Ok(())
            })?;
    }
    let part = match spec.like {
        Some(_) => format!("{} added at {}", spec.part, spec.frame),
        None => spec.part.clone(),
    };
    let changed: Option<Vec<bool>> = match &spec.like {
        Some(cel) => {
            let from = like(bundle, spec, cel)?.from;
            let before = source(bundle, spec, from)?;
            let moved: Vec<bool> = before
                .lab
                .iter()
                .zip(&picture.lab)
                .zip(before.alpha.iter().zip(&picture.alpha))
                .map(|((a, b), (x, y))| (x - y).abs() > 0.25 || difference(*a, *b) > spec.threshold * 0.5)
                .collect();
            let near = spec.sizes.first().copied().unwrap_or(16.0);
            let mut covered = Canvas::new(picture.w * picture.h);
            kept.iter()
                .for_each(|k| covered.paint(picture.w, picture.h, &k.points, k.size * 0.5, k.lab));
            Some(
                grow(&moved, picture.w, picture.h, near)
                    .iter()
                    .zip(&region)
                    .zip(&covered.painted)
                    .map(|((m, r), c)| *r && (*m || !*c))
                    .collect(),
            )
        }
        None => None,
    };
    let plan = Plan {
        region: changed.as_deref().unwrap_or(&region),
        ..plan
    };
    let made = paint(&picture, &plan, &kept);
    made.iter().enumerate().try_for_each(|(k, s)| -> Result<()> {
        let size = s.size * (0.9 + 0.2 * rng.unit());
        let seed = spec.seed.wrapping_mul(7919).wrapping_add(k as u32);
        ops.push(stroke_op(
            spec,
            &part,
            &s.points,
            size,
            hex(s.lab, seed, spec.vary),
            1.0,
            seed,
        )?);
        Ok(())
    })?;
    if ops.is_empty() {
        bail!("paint over: nothing to paint; the layer already matches the source within --threshold");
    }
    Ok(ops)
}

pub fn resolve(bundle: &Bundle, ops: &mut Vec<Op>) -> Result<Vec<String>> {
    if !ops.iter().any(|op| matches!(op, Op::PaintOver(_))) {
        return Ok(Vec::new());
    }
    if ops.len() > 1 {
        bail!("paint over needs a batch of its own: it reads the drawing from the bundle as it is");
    }
    let Some(Op::PaintOver(spec)) = ops.first() else {
        unreachable!("checked above");
    };
    let spec = spec.clone();
    let made = expand(bundle, &spec)?;
    let strokes = made.iter().filter(|o| matches!(o, Op::Stroke(_))).count();
    *ops = made;
    Ok(vec![format!(
        "paint over on {}: {strokes} strokes{}",
        spec.layer,
        spec.like
            .as_ref()
            .map_or(String::new(), |c| format!(", carried from {c}"))
    )])
}

pub fn chain(
    path: &std::path::Path,
    spec: &PaintOverSpec,
    (first, last): (u32, u32),
    out: &crate::agent::Output,
) -> Result<()> {
    let mut previous = spec
        .like
        .clone()
        .context("--frames carries a paint-over from drawing to drawing: give the first with --like")?;
    let mut made: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut painted = 0;
    (first..=last).try_for_each(|frame| -> Result<()> {
        let bundle = Bundle::open(path)?;
        let doc = &bundle.doc;
        let target = doc.find_layer(&spec.layer)?;
        let starting: Vec<klad_core::doc::Cel> = layers_of(doc, spec)?
            .into_iter()
            .flat_map(family)
            .filter(|l| l.kind() == Kind::Paint)
            .filter_map(|l| l.cel_at(frame).filter(|c| c.frame == frame).cloned())
            .collect();
        if starting.is_empty() {
            return Ok(());
        }
        let hold = starting.iter().filter_map(|c| c.hold).min();
        if let Some(source) = starting.iter().find_map(|c| c.link.as_ref())
            && starting.iter().all(|c| c.link.is_some())
        {
            if let Some(mine) = made.get(source) {
                let mut op = json!({"op": "cel", "link": mine, "frame": frame});
                if let Some(h) = hold {
                    op["hold"] = json!(h);
                }
                crate::agent::edit(path, &klad_core::ops::parse_lines(&op.to_string())?, out)?;
            }
            return Ok(());
        }
        let own = target
            .cel_at(frame)
            .filter(|c| c.frame == frame && c.link.is_none())
            .map(|c| c.id.clone());
        let cel = match own {
            Some(id) => id,
            None => {
                let mut op = json!({"op": "cel", "new": spec.layer, "frame": frame});
                if let Some(h) = hold {
                    op["hold"] = json!(h);
                }
                crate::agent::edit(path, &klad_core::ops::parse_lines(&op.to_string())?, out)?;
                let bundle = Bundle::open(path)?;
                let target = bundle.doc.find_layer(&spec.layer)?;
                target
                    .cel_at(frame)
                    .map(|c| c.id.clone())
                    .context("the new drawing did not land")?
            }
        };
        if cel == previous {
            starting.iter().for_each(|c| {
                made.insert(c.id.clone(), cel.clone());
            });
            return Ok(());
        }
        let step = PaintOverSpec {
            frame,
            like: Some(previous.clone()),
            ..spec.clone()
        };
        crate::agent::edit(path, &[Op::PaintOver(step)], out)?;
        starting.iter().for_each(|c| {
            made.insert(c.id.clone(), cel.clone());
        });
        previous = cel;
        painted += 1;
        Ok(())
    })?;
    eprintln!("paint over carried to {painted} drawings, frames {first} to {last}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Document;
    use klad_core::ops::parse_lines;

    fn bundle(name: &str, w: u32, h: u32, ops: &str) -> (std::path::PathBuf, Bundle) {
        let dir = std::env::temp_dir().join(format!("klad-paintover-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(w, h);
        doc.layers = vec![
            Layer::paint("src", "source", "c1"),
            Layer::paint("work", "brushwork", "c2"),
        ];
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let mut ops = parse_lines(ops).unwrap();
        crate::fills::resolve(&bundle, &mut ops).unwrap();
        bundle.apply(&ops).unwrap();
        (dir, bundle)
    }

    fn spec(sizes: &[f32]) -> PaintOverSpec {
        serde_json::from_value(json!({"layer": "work", "sizes": sizes, "seed": 3})).unwrap()
    }

    fn strokes(ops: &[Op]) -> Vec<&StrokeSpec> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Stroke(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn points(s: &StrokeSpec) -> Vec<[f32; 3]> {
        s.points.clone().unwrap()
    }

    fn rect(x: f32, w: f32, h: f32, colour: &str) -> String {
        format!(
            r#"{{"op":"fill","layer":"src","polygon":[[{x},0],[{},0],[{},{h}],[{x},{h}]],"color":"{colour}"}}"#,
            x + w,
            x + w
        )
    }

    #[test]
    fn strokes_stay_on_their_side_of_an_edge_and_repeat_with_a_seed() {
        let ops = [rect(0.0, 120.0, 160.0, "#c0402a"), rect(120.0, 120.0, 160.0, "#2f5fb0")].join("\n");
        let (dir, bundle) = bundle("edge", 240, 160, &ops);
        let made = expand(&bundle, &spec(&[16.0, 8.0])).unwrap();
        let found = strokes(&made);
        assert!(found.len() > 20, "{}", found.len());
        found.iter().for_each(|s| {
            let xs: Vec<f32> = points(s).iter().map(|p| p[0]).collect();
            let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
            assert!(
                hi <= 120.0 + 6.0 || lo >= 120.0 - 6.0,
                "a stroke crossed the edge: {lo}..{hi}"
            );
            assert_eq!(s.part.as_deref(), Some("paint over"));
        });
        assert_eq!(
            expand(&bundle, &spec(&[16.0, 8.0])).unwrap(),
            made,
            "the same seed paints the same strokes"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn strokes_run_along_bands_and_small_brushes_only_go_where_detail_is() {
        let bands: Vec<String> = (0..16)
            .map(|i| {
                let v = 40 + i * 12;
                format!(
                    r##"{{"op":"fill","layer":"src","polygon":[[0,{}],[160,{}],[160,{}],[0,{}]],"color":"#{v:02x}{v:02x}{v:02x}"}}"##,
                    i * 10,
                    i * 10,
                    i * 10 + 10,
                    i * 10 + 10
                )
            })
            .collect();
        let checks: Vec<String> = (0..20)
            .flat_map(|i| (0..20).map(move |j| (i, j)))
            .filter(|(i, j)| (i + j) % 2 == 0)
            .map(|(i, j)| {
                let (x, y) = (160 + i * 8, j * 8);
                format!(
                    r##"{{"op":"fill","layer":"src","polygon":[[{x},{y}],[{},{y}],[{},{}],[{x},{}]],"color":"#202020"}}"##,
                    x + 8,
                    x + 8,
                    y + 8,
                    y + 8
                )
            })
            .collect();
        let ops = [rect(160.0, 160.0, 160.0, "#e8e8e8")]
            .into_iter()
            .chain(bands)
            .chain(checks)
            .collect::<Vec<_>>()
            .join("\n");
        let (dir, bundle) = bundle("bands", 320, 160, &ops);
        let made = expand(&bundle, &spec(&[24.0, 6.0])).unwrap();
        let found = strokes(&made);
        let (mut along, mut across) = (0.0, 0.0);
        found
            .iter()
            .filter(|s| s.size > 12.0 && points(s).iter().all(|p| p[0] < 150.0))
            .for_each(|s| {
                let p = points(s);
                let (a, b) = (p[0], p[p.len() - 1]);
                along += (b[0] - a[0]).abs();
                across += (b[1] - a[1]).abs();
            });
        assert!(
            along > 3.0 * across,
            "strokes over horizontal bands run sideways: {along} against {across}"
        );
        let small: Vec<f32> = found
            .iter()
            .filter(|s| s.size < 12.0)
            .map(|s| points(s)[0][0])
            .collect();
        let right = small.iter().filter(|x| **x >= 160.0).count();
        assert!(
            right * 4 > small.len() * 3,
            "small strokes go where the detail is: {right} of {}",
            small.len()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_paint_over_carried_to_the_next_key_moves_with_the_marks_and_in_betweens_without_boiling() {
        let disc = |cx: f32| {
            let pts: Vec<String> = (0..24)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / 24.0;
                    format!("[{:.1},{:.1}]", cx + 40.0 * a.cos(), 100.0 + 40.0 * a.sin())
                })
                .collect();
            format!(
                r##"{{"op":"fill","layer":"src","polygon":[{}],"color":"#d0a030","frame":FRAME}}"##,
                pts.join(",")
            )
        };
        let ops = [
            r#"{"op":"timeline","frames":8}"#.to_string(),
            rect(0.0, 320.0, 200.0, "#486080").replace(r#""color""#, r#""frame":0,"color""#),
            disc(110.0).replace("FRAME", "0"),
            r#"{"op":"cel","new":"src","frame":4,"id":"k4"}"#.to_string(),
            rect(0.0, 320.0, 200.0, "#486080").replace(r#""color""#, r#""frame":4,"color""#),
            disc(140.0).replace("FRAME", "4"),
            r#"{"op":"cel","new":"work","frame":4,"id":"w4"}"#.to_string(),
        ]
        .join("\n");
        let (dir, mut bundle) = bundle("like", 320, 200, &ops);
        let first = expand(&bundle, &spec(&[16.0, 8.0])).unwrap();
        bundle.apply(&first).unwrap();
        let mut carry = spec(&[16.0, 8.0]);
        carry.frame = 4;
        carry.like = Some("c2".into());
        let second = expand(&bundle, &carry).unwrap();
        let (a, b) = (strokes(&first), strokes(&second));
        let carried: Vec<&&StrokeSpec> = b.iter().filter(|s| s.part.as_deref() == Some("paint over")).collect();
        assert_eq!(carried.len(), a.len(), "every stroke is carried, in order");
        let (mut on_disc, mut still) = (0, 0);
        a.iter().zip(&carried).for_each(|(s, t)| {
            let (p, q) = (points(s)[0], points(t)[0]);
            let inside = (p[0] - 110.0).hypot(p[1] - 100.0) < 30.0;
            let away = (p[0] - 110.0).hypot(p[1] - 100.0) > 70.0 && (p[0] - 140.0).hypot(p[1] - 100.0) > 70.0;
            if inside {
                on_disc += 1;
                assert!(
                    (q[0] - p[0] - 30.0).abs() < 2.0 && (q[1] - p[1]).abs() < 2.0,
                    "{p:?} -> {q:?}"
                );
            }
            if away {
                still += 1;
                assert!(
                    (p[0] - q[0]).abs() < 0.1 && (p[1] - q[1]).abs() < 0.1,
                    "the background stays: {p:?} {q:?}"
                );
                assert_eq!(
                    s.seed, t.seed,
                    "a carried stroke keeps its seed, so its texture does not boil"
                );
            }
        });
        assert!(on_disc > 3 && still > 10, "{on_disc} on the disc, {still} still");
        bundle.apply(&second).unwrap();
        let mut tween = parse_lines(r#"{"op":"inbetween","layers":["work"],"from":0,"to":4}"#).unwrap();
        let notes = crate::inbetween::resolve(&bundle, &mut tween).unwrap();
        let Op::Inbetween(made) = &tween[0] else { panic!() };
        let resolved = made.resolved.as_ref().unwrap();
        assert_eq!(resolved.cels.len(), 3, "{notes:?}");
        let marks = |c: &klad_core::ops::TweenCel| c.marks.len();
        assert!(
            resolved.cels.iter().all(|c| marks(c) >= a.len()),
            "every carried stroke is paired: {notes:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
