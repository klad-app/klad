use crate::color::{from_oklab, from_u16, to_oklab, to_u16};
use crate::fill::{Coverage, Fill, FillSource};
use crate::markedit::Affine;
use crate::stroke::{Author, Mark, Sample, Stroke, fingerprint};
use anyhow::{Result, anyhow, bail};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Match {
    #[default]
    Id,
    Order,
    Near,
}

impl Match {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "" | "id" => Match::Id,
            "order" => Match::Order,
            "near" => Match::Near,
            other => bail!("match takes id, order or near, not {other:?}"),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Match::Id => "id",
            Match::Order => "order",
            Match::Near => "near",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Unpaired {
    #[default]
    Leave,
    Pop(f32),
    Fade,
    DrawOn,
}

impl Unpaired {
    pub fn parse(text: &str) -> Result<Self> {
        let mut words = text.split_whitespace();
        let mode = match (words.next().unwrap_or(""), words.next()) {
            ("" | "leave", None) => Unpaired::Leave,
            ("pop", None) => Unpaired::Pop(0.5),
            ("pop", Some(at)) => {
                let at: f32 = at
                    .parse()
                    .map_err(|_| anyhow!("pop takes the moment it happens, 0 to 1, not {at:?}"))?;
                if !(0.0..=1.0).contains(&at) {
                    bail!("pop happens between 0 and 1, not at {at}");
                }
                Unpaired::Pop(at)
            }
            ("fade", None) => Unpaired::Fade,
            ("draw-on", None) => Unpaired::DrawOn,
            _ => bail!("unpaired takes leave, pop, fade or draw-on (pop may take a moment: pop 0.3), not {text:?}"),
        };
        if words.next().is_some() {
            bail!("unpaired takes one mode, not {text:?}");
        }
        Ok(mode)
    }

    pub fn name(&self) -> String {
        match self {
            Unpaired::Leave => String::new(),
            Unpaired::Pop(at) if *at == 0.5 => "pop".into(),
            Unpaired::Pop(at) => format!("pop {at}"),
            Unpaired::Fade => "fade".into(),
            Unpaired::DrawOn => "draw-on".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Spacing {
    #[default]
    Even,
    In,
    Out,
    InOut,
    Values(Vec<f32>),
}

impl Spacing {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text.trim() {
            "" | "even" => Spacing::Even,
            "in" => Spacing::In,
            "out" => Spacing::Out,
            "in-out" | "inout" => Spacing::InOut,
            list => {
                let values: Vec<f32> = list
                    .split(',')
                    .map(|v| {
                        v.trim()
                            .parse::<f32>()
                            .map_err(|_| anyhow!("spacing {v:?} is not a number"))
                    })
                    .collect::<Result<_>>()?;
                if values.iter().any(|v| !(*v > 0.0 && *v < 1.0)) {
                    bail!("spacing values must lie between 0 and 1 (the keys), for example 0.25,0.5,0.75");
                }
                if values.windows(2).any(|w| w[1] <= w[0]) {
                    bail!("spacing values must grow from the first key to the second");
                }
                Spacing::Values(values)
            }
        })
    }

    pub fn count(&self) -> Option<usize> {
        match self {
            Spacing::Values(v) => Some(v.len()),
            _ => None,
        }
    }

    pub fn ts(&self, count: usize) -> Vec<f32> {
        let s = |i: usize| (i + 1) as f64 / (count + 1) as f64;
        (0..count)
            .map(|i| {
                let x = s(i);
                (match self {
                    Spacing::Even => x,
                    Spacing::In => x * x,
                    Spacing::Out => 1.0 - (1.0 - x) * (1.0 - x),
                    Spacing::InOut => x * x * (3.0 - 2.0 * x),
                    Spacing::Values(v) => v[i] as f64,
                }) as f32
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct P {
    x: f64,
    y: f64,
    p: f64,
    tx: f64,
    ty: f64,
    time: f64,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn mix(a: &P, b: &P, t: f64) -> P {
    P {
        x: lerp(a.x, b.x, t),
        y: lerp(a.y, b.y, t),
        p: lerp(a.p, b.p, t),
        tx: lerp(a.tx, b.tx, t),
        ty: lerp(a.ty, b.ty, t),
        time: lerp(a.time, b.time, t),
    }
}

fn points_of(stroke: &Stroke) -> Vec<P> {
    let mut time = 0.0;
    stroke
        .samples
        .iter()
        .map(|s| {
            time += s.dt_ms as f64;
            let (x, y) = s.pos();
            let (tx, ty) = s.tilt();
            P {
                x: x as f64,
                y: y as f64,
                p: s.pressure() as f64,
                tx: tx as f64,
                ty: ty as f64,
                time,
            }
        })
        .collect()
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    libm::sqrt((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1))
}

fn walked(points: &[P]) -> Vec<f64> {
    let mut total = 0.0;
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i > 0 {
                total += dist((points[i - 1].x, points[i - 1].y), (p.x, p.y));
            }
            total
        })
        .collect()
}

fn resample(points: &[P], n: usize) -> Vec<P> {
    match (points.len(), n) {
        (0, _) | (_, 0) => Vec::new(),
        (1, n) => vec![points[0]; n],
        (_, 1) => vec![points[0]],
        _ => {
            let along = walked(points);
            let total = along[along.len() - 1];
            if total < 1e-9 {
                return (0..n)
                    .map(|i| mix(&points[0], &points[points.len() - 1], i as f64 / (n - 1) as f64))
                    .collect();
            }
            let mut seg = 0;
            (0..n)
                .map(|i| {
                    let at = total * i as f64 / (n - 1) as f64;
                    while seg + 2 < points.len() && along[seg + 1] < at {
                        seg += 1;
                    }
                    let span = along[seg + 1] - along[seg];
                    let t = if span < 1e-12 {
                        0.0
                    } else {
                        ((at - along[seg]) / span).clamp(0.0, 1.0)
                    };
                    mix(&points[seg], &points[seg + 1], t)
                })
                .collect()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Sim {
    m: (f64, f64),
    c: (f64, f64),
}

const SAME: Sim = Sim {
    m: (1.0, 0.0),
    c: (0.0, 0.0),
};

fn cmul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

fn cdiv(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let d = b.0 * b.0 + b.1 * b.1;
    ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
}

fn fit(a: &[(f64, f64)], b: &[(f64, f64)]) -> Sim {
    let n = a.len().max(1) as f64;
    let centre = |v: &[(f64, f64)]| {
        let (sx, sy) = v.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
        (sx / n, sy / n)
    };
    let (ca, cb) = (centre(a), centre(b));
    let (dot, cross, norm) = a.iter().zip(b).fold((0.0, 0.0, 0.0), |(d, c, s), (p, q)| {
        let (ax, ay, bx, by) = (p.0 - ca.0, p.1 - ca.1, q.0 - cb.0, q.1 - cb.1);
        (d + ax * bx + ay * by, c + ax * by - ay * bx, s + ax * ax + ay * ay)
    });
    let m = if norm < 1e-6 || (dot.abs() < 1e-9 && cross.abs() < 1e-9) {
        (1.0, 0.0)
    } else {
        (dot / norm, cross / norm)
    };
    let turned = cmul(m, ca);
    Sim {
        m,
        c: (cb.0 - turned.0, cb.1 - turned.1),
    }
}

impl Sim {
    fn apply(&self, p: (f64, f64)) -> (f64, f64) {
        let q = cmul(self.m, p);
        (q.0 + self.c.0, q.1 + self.c.1)
    }

    fn then(&self, inner: &Sim) -> Sim {
        let c = cmul(self.m, inner.c);
        Sim {
            m: cmul(self.m, inner.m),
            c: (c.0 + self.c.0, c.1 + self.c.1),
        }
    }

    fn inverse(&self) -> Sim {
        let m = cdiv((1.0, 0.0), self.m);
        let c = cmul(m, self.c);
        Sim { m, c: (-c.0, -c.1) }
    }

    fn arc(&self, t: f64) -> Sim {
        let scale = libm::pow(libm::sqrt(self.m.0 * self.m.0 + self.m.1 * self.m.1), t);
        let angle = libm::atan2(self.m.1, self.m.0) * t;
        let m = (scale * libm::cos(angle), scale * libm::sin(angle));
        let gap = (1.0 - self.m.0, -self.m.1);
        let ratio = match gap.0 * gap.0 + gap.1 * gap.1 < 1e-12 {
            true => (t, 0.0),
            false => cdiv((1.0 - m.0, -m.1), gap),
        };
        Sim {
            m,
            c: cmul(self.c, ratio),
        }
    }

    fn affine(&self) -> [f64; 6] {
        [self.m.0, -self.m.1, self.m.1, self.m.0, self.c.0, self.c.1]
    }
}

fn affine32(a: [f64; 6]) -> Affine {
    a.map(|v| v as f32)
}

fn xy(points: &[P]) -> Vec<(f64, f64)> {
    points.iter().map(|p| (p.x, p.y)).collect()
}

pub type Lines = Vec<Vec<(f32, f32)>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub line: usize,
    u: f64,
    along: f64,
    across: f64,
}

fn frame_at(line: &[(f64, f64)], u: f64) -> Option<((f64, f64), (f64, f64))> {
    match line.len() {
        0 => None,
        1 => Some((line[0], (1.0, 0.0))),
        _ => {
            let pts: Vec<P> = line.iter().map(|&(x, y)| P { x, y, ..P::default() }).collect();
            let along = walked(&pts);
            let total = along[along.len() - 1];
            let at = total * u.clamp(0.0, 1.0);
            let seg = (0..line.len() - 1)
                .find(|&i| along[i + 1] >= at)
                .unwrap_or(line.len() - 2);
            let span = (along[seg + 1] - along[seg]).max(1e-12);
            let t = ((at - along[seg]) / span).clamp(0.0, 1.0);
            let (a, b) = (line[seg], line[seg + 1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = libm::sqrt(dx * dx + dy * dy);
            let tangent = if len < 1e-12 { (1.0, 0.0) } else { (dx / len, dy / len) };
            Some(((a.0 + dx * t, a.1 + dy * t), tangent))
        }
    }
}

fn nearest_on(line: &[(f64, f64)], at: (f64, f64), hint: Option<f64>) -> Option<(f64, f64)> {
    match line.len() {
        0 => None,
        1 => Some((0.0, dist(line[0], at))),
        _ => {
            let pts: Vec<P> = line.iter().map(|&(x, y)| P { x, y, ..P::default() }).collect();
            let along = walked(&pts);
            let total = along[along.len() - 1].max(1e-12);
            let found: Vec<(f64, f64)> = line
                .windows(2)
                .enumerate()
                .map(|(i, w)| {
                    let (a, b) = (w[0], w[1]);
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let span = dx * dx + dy * dy;
                    let t = if span < 1e-12 {
                        0.0
                    } else {
                        (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / span).clamp(0.0, 1.0)
                    };
                    let p = (a.0 + dx * t, a.1 + dy * t);
                    ((along[i] + (along[i + 1] - along[i]) * t) / total, dist(p, at))
                })
                .collect();
            let best = found.iter().map(|f| f.1).fold(f64::MAX, f64::min);
            found
                .into_iter()
                .filter(|f| f.1 <= best + 1.0)
                .min_by(|x, y| match hint {
                    Some(u) => (x.0 - u).abs().total_cmp(&(y.0 - u).abs()),
                    None => x.1.total_cmp(&y.1),
                })
        }
    }
}

fn anchor_on(line: &[(f64, f64)], index: usize, at: (f64, f64)) -> Option<(Anchor, f64)> {
    anchor_near(line, index, at, None)
}

fn anchor_near(line: &[(f64, f64)], index: usize, at: (f64, f64), hint: Option<f64>) -> Option<(Anchor, f64)> {
    let (u, d) = nearest_on(line, at, hint)?;
    let (p, tangent) = frame_at(line, u)?;
    let (ox, oy) = (at.0 - p.0, at.1 - p.1);
    Some((
        Anchor {
            line: index,
            u,
            along: ox * tangent.0 + oy * tangent.1,
            across: -ox * tangent.1 + oy * tangent.0,
        },
        d,
    ))
}

pub fn anchor(lines: &[Vec<(f32, f32)>], at: (f32, f32)) -> Option<Anchor> {
    let at = (at.0 as f64, at.1 as f64);
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let line: Vec<(f64, f64)> = line.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
            anchor_on(&line, i, at)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(a, _)| a)
}

fn place_on(line: &[(f64, f64)], a: &Anchor) -> Option<(f64, f64)> {
    let (p, tangent) = frame_at(line, a.u)?;
    Some((
        p.0 + a.along * tangent.0 - a.across * tangent.1,
        p.1 + a.along * tangent.1 + a.across * tangent.0,
    ))
}

pub fn place(anchor: &Anchor, lines: &[Vec<(f32, f32)>]) -> Option<(f32, f32)> {
    let line: Vec<(f64, f64)> = lines
        .get(anchor.line)?
        .iter()
        .map(|&(x, y)| (x as f64, y as f64))
        .collect();
    place_on(&line, anchor).map(|(x, y)| (x as f32, y as f32))
}

fn points_of_mark(mark: &Mark) -> Vec<(f32, f32)> {
    match mark {
        Mark::Stroke(s) => s.samples.iter().map(Sample::pos).collect(),
        Mark::Fill(f) => match &f.source {
            FillSource::Polygon(points) => points.iter().map(|p| (p[0], p[1])).collect(),
            FillSource::Seed { seed, .. } => vec![(seed.x, seed.y)],
        },
    }
}

fn wide(line: &[(f32, f32)]) -> Vec<(f64, f64)> {
    line.iter().map(|&(x, y)| (x as f64, y as f64)).collect()
}

pub fn carry(mark: &Mark, from: &Lines, to: &Lines, within: Option<&[usize]>) -> Mark {
    let pts = points_of_mark(mark);
    if pts.is_empty() {
        return mark.clone();
    }
    let n = pts.len() as f64;
    let centre = pts
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p.0 as f64 / n, a.1 + p.1 as f64 / n));
    let nearest = from
        .iter()
        .enumerate()
        .filter(|(i, _)| within.is_none_or(|w| w.contains(i)) && to.get(*i).is_some_and(|l| !l.is_empty()))
        .filter_map(|(i, line)| anchor_on(&wide(line), i, centre).map(|(_, d)| (i, d)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i);
    let Some(line) = nearest else {
        return mark.clone();
    };
    let (src, dst) = (wide(&from[line]), wide(&to[line]));
    let mv = |(x, y): (f32, f32)| -> (f32, f32) {
        anchor_on(&src, line, (x as f64, y as f64))
            .and_then(|(a, _)| place_on(&dst, &a))
            .map_or((x, y), |(x, y)| (x as f32, y as f32))
    };
    let mut out = mark.clone();
    match &mut out {
        Mark::Stroke(s) => s.samples.iter_mut().for_each(|q| {
            let (x, y) = mv(q.pos());
            q.x = (x * crate::stroke::POS_SCALE).round() as i32;
            q.y = (y * crate::stroke::POS_SCALE).round() as i32;
        }),
        Mark::Fill(f) => match &mut f.source {
            FillSource::Polygon(points) => points.iter_mut().for_each(|p| {
                let (x, y) = mv((p[0], p[1]));
                *p = [x, y];
            }),
            FillSource::Seed { seed, .. } => {
                (seed.x, seed.y) = mv((seed.x, seed.y));
            }
        },
    }
    out
}

fn drawn_part(stroke: &Stroke, share: f32, from_start: bool) -> Option<Stroke> {
    if share <= 0.0 || stroke.samples.len() < 2 {
        return (share >= 1.0).then(|| stroke.clone());
    }
    if share >= 1.0 {
        return Some(stroke.clone());
    }
    let pts: Vec<P> = points_of(stroke);
    let along = walked(&pts);
    let total = along[along.len() - 1];
    let keep: Vec<Sample> = stroke
        .samples
        .iter()
        .zip(&along)
        .filter(|(_, d)| match from_start {
            true => **d <= total * share as f64,
            false => **d >= total * (1.0 - share as f64),
        })
        .map(|(q, _)| *q)
        .collect();
    (keep.len() >= 2).then(|| Stroke {
        samples: keep,
        ..stroke.clone()
    })
}

fn shown(mark: &Mark, t: f32, how: Unpaired, first: bool) -> Option<Mark> {
    let presence = if first { 1.0 - t } else { t };
    match (how, mark) {
        (Unpaired::Leave, _) => None,
        (Unpaired::Pop(at), _) => (if first { t < at } else { t >= at }).then(|| mark.clone()),
        (Unpaired::Fade, Mark::Stroke(s)) => Some(Mark::Stroke(Stroke {
            opacity: s.opacity * presence,
            ..s.clone()
        })),
        (Unpaired::Fade, Mark::Fill(f)) => Some(Mark::Fill(Fill {
            opacity: f.opacity * presence,
            ..f.clone()
        })),
        (Unpaired::DrawOn, Mark::Stroke(s)) => drawn_part(s, presence, true).map(Mark::Stroke),
        (Unpaired::DrawOn, Mark::Fill(_)) => (presence >= 0.5).then(|| mark.clone()),
    }
}

pub fn line_of(stroke: &Stroke) -> Vec<(f32, f32)> {
    stroke.samples.iter().map(Sample::pos).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub a: u32,
    pub b: u32,
    pub reversed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pairing {
    pub pairs: Vec<Pair>,
    pub alone_a: Vec<u32>,
    pub alone_b: Vec<u32>,
    pub clashes: Vec<(u32, u32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Stroke,
    Polygon,
    Flood,
}

fn sort_of(mark: &Mark) -> Sort {
    match mark {
        Mark::Stroke(_) => Sort::Stroke,
        Mark::Fill(f) => match f.source {
            FillSource::Polygon(_) => Sort::Polygon,
            FillSource::Seed { .. } => Sort::Flood,
        },
    }
}

fn outline(mark: &Mark) -> Vec<P> {
    match mark {
        Mark::Stroke(s) => points_of(s),
        Mark::Fill(f) => match &f.source {
            FillSource::Polygon(points) => points
                .iter()
                .chain(points.first())
                .map(|p| P {
                    x: p[0] as f64,
                    y: p[1] as f64,
                    ..P::default()
                })
                .collect(),
            FillSource::Seed { seed, .. } => vec![P {
                x: seed.x as f64,
                y: seed.y as f64,
                ..P::default()
            }],
        },
    }
}

fn shape_cost(a: &Mark, b: &Mark) -> (f64, bool) {
    let (pa, pb) = (resample(&outline(a), 16), resample(&outline(b), 16));
    let mean = |q: &[P]| pa.iter().zip(q).map(|(x, y)| dist((x.x, x.y), (y.x, y.y))).sum::<f64>() / 16.0;
    let forward = mean(&pb);
    let back: Vec<P> = pb.iter().rev().copied().collect();
    let backward = mean(&back);
    match a.as_stroke().is_some() && backward < forward {
        true => (backward, true),
        false => (forward, false),
    }
}

pub type Groups = [(Vec<u32>, Vec<u32>)];

fn nearest(free_a: &[&Mark], free_b: &[&Mark], xs: &[u32], ys: &[u32]) -> Vec<(u32, u32, bool)> {
    let mut costs: Vec<(f64, u32, u32, bool)> = free_a
        .iter()
        .filter(|x| xs.contains(&x.id()))
        .flat_map(|x| {
            free_b
                .iter()
                .filter(|y| ys.contains(&y.id()) && sort_of(x) == sort_of(y))
                .map(|y| {
                    let (cost, rev) = shape_cost(x, y);
                    (cost, x.id(), y.id(), rev)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    costs.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)).then(p.2.cmp(&q.2)));
    let mut used_a = Vec::new();
    let mut used_b = Vec::new();
    costs
        .into_iter()
        .filter(|(_, x, y, _)| {
            let fresh = !used_a.contains(x) && !used_b.contains(y);
            if fresh {
                used_a.push(*x);
                used_b.push(*y);
            }
            fresh
        })
        .map(|(_, x, y, rev)| (x, y, rev))
        .collect()
}

pub fn pair(a: &[Mark], b: &[Mark], how: Match, forced: &[[u32; 2]]) -> Pairing {
    pair_parts(a, b, how, forced, &[])
}

pub fn pair_parts(a: &[Mark], b: &[Mark], how: Match, forced: &[[u32; 2]], parts: &Groups) -> Pairing {
    let mut out = Pairing::default();
    let mut free_a: Vec<&Mark> = a.iter().collect();
    let mut free_b: Vec<&Mark> = b.iter().collect();
    let take = |out: &mut Pairing, free_a: &mut Vec<&Mark>, free_b: &mut Vec<&Mark>, x: u32, y: u32, rev: bool| {
        let (Some(i), Some(j)) = (
            free_a.iter().position(|m| m.id() == x),
            free_b.iter().position(|m| m.id() == y),
        ) else {
            return;
        };
        if sort_of(free_a[i]) != sort_of(free_b[j]) {
            out.clashes.push((x, y));
        } else {
            out.pairs.push(Pair {
                a: x,
                b: y,
                reversed: rev,
            });
        }
        free_a.remove(i);
        free_b.remove(j);
    };
    forced
        .iter()
        .for_each(|[x, y]| take(&mut out, &mut free_a, &mut free_b, *x, *y, false));
    parts.iter().for_each(|(xs, ys)| {
        [Sort::Stroke, Sort::Polygon, Sort::Flood].iter().for_each(|sort| {
            let mine = |free: &[&Mark], ids: &[u32]| -> Vec<u32> {
                free.iter()
                    .filter(|m| sort_of(m) == *sort && ids.contains(&m.id()))
                    .map(|m| m.id())
                    .collect()
            };
            let (left, right) = (mine(&free_a, xs), mine(&free_b, ys));
            let chosen: Vec<(u32, u32, bool)> = match left.len() == right.len() {
                true => left.iter().zip(&right).map(|(&x, &y)| (x, y, false)).collect(),
                false => nearest(&free_a, &free_b, &left, &right),
            };
            chosen
                .into_iter()
                .for_each(|(x, y, rev)| take(&mut out, &mut free_a, &mut free_b, x, y, rev));
        });
    });
    match how {
        Match::Id => {
            let ids: Vec<u32> = free_a.iter().map(|m| m.id()).collect();
            ids.iter()
                .for_each(|&id| take(&mut out, &mut free_a, &mut free_b, id, id, false));
        }
        Match::Order => {
            [Sort::Stroke, Sort::Polygon, Sort::Flood].iter().for_each(|sort| {
                let xs: Vec<u32> = free_a.iter().filter(|m| sort_of(m) == *sort).map(|m| m.id()).collect();
                let ys: Vec<u32> = free_b.iter().filter(|m| sort_of(m) == *sort).map(|m| m.id()).collect();
                xs.iter()
                    .zip(&ys)
                    .for_each(|(&x, &y)| take(&mut out, &mut free_a, &mut free_b, x, y, false));
            });
        }
        Match::Near => {
            let (xs, ys): (Vec<u32>, Vec<u32>) = (
                free_a.iter().map(|m| m.id()).collect(),
                free_b.iter().map(|m| m.id()).collect(),
            );
            nearest(&free_a, &free_b, &xs, &ys)
                .into_iter()
                .for_each(|(x, y, rev)| take(&mut out, &mut free_a, &mut free_b, x, y, rev));
        }
    }
    let order: BTreeMap<u32, usize> = a.iter().enumerate().map(|(i, m)| (m.id(), i)).collect();
    out.pairs
        .sort_by_key(|p| order.get(&p.a).copied().unwrap_or(usize::MAX));
    out.alone_a = free_a.iter().map(|m| m.id()).collect();
    out.alone_b = free_b.iter().map(|m| m.id()).collect();
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Touch {
    other: usize,
    u: (f64, f64),
    along: (f64, f64),
    across: (f64, f64),
}

#[derive(Clone, Debug, PartialEq)]
struct StrokePlan {
    a: Stroke,
    b: Stroke,
    pa: Vec<P>,
    pb: Vec<P>,
    own: Sim,
    rel: Sim,
    parent: Option<usize>,
    rest: Vec<(f64, f64)>,
    touches: [Option<Touch>; 2],
}

#[derive(Clone, Debug, PartialEq)]
struct ShapePlan {
    a: Fill,
    b: Fill,
    pa: Vec<(f64, f64)>,
    fit: Sim,
    rest: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
enum Item {
    Stroke(StrokePlan),
    Polygon(ShapePlan),
    Flood { a: Fill, b: Fill },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    items: Vec<Item>,
    order: Vec<usize>,
    pub pairing: Pairing,
    alone: (Vec<Mark>, Vec<Mark>),
}

fn planned_points(pa: &[P], pb: &[P]) -> (Sim, Vec<(f64, f64)>) {
    let (xa, xb) = (xy(pa), xy(pb));
    let f = fit(&xa, &xb);
    let rest = xa
        .iter()
        .zip(&xb)
        .map(|(p, q)| {
            let m = f.apply(*p);
            (q.0 - m.0, q.1 - m.1)
        })
        .collect();
    (f, rest)
}

fn reach(stroke: &Stroke) -> f64 {
    stroke.size as f64 / 2.0 + 2.0
}

fn touch_of(
    strokes: &[(&Stroke, Vec<(f64, f64)>)],
    me: usize,
    end: usize,
    hint: Option<(usize, f64)>,
) -> Option<(usize, f64, f64, f64)> {
    let (own, line) = &strokes[me];
    let at = match end {
        0 => *line.first()?,
        _ => *line.last()?,
    };
    let slack = own.size as f64 * 0.25;
    strokes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != me && hint.is_none_or(|(h, _)| h == *i))
        .filter_map(|(i, (stroke, other))| {
            let (anchor, d) = anchor_near(other, i, at, hint.map(|(_, u)| u))?;
            (d <= reach(stroke) + slack).then_some((i, anchor, d))
        })
        .min_by(|x, y| x.2.total_cmp(&y.2))
        .map(|(i, a, _)| (i, a.u, a.along, a.across))
}

pub fn plan(a: &[Mark], b: &[Mark], how: Match, forced: &[[u32; 2]]) -> Plan {
    plan_parts(a, b, how, forced, &[])
}

pub fn plan_parts(a: &[Mark], b: &[Mark], how: Match, forced: &[[u32; 2]], parts: &Groups) -> Plan {
    let pairing = pair_parts(a, b, how, forced, parts);
    let find = |marks: &[Mark], id: u32| marks.iter().find(|m| m.id() == id).cloned();
    let items: Vec<Item> = pairing
        .pairs
        .iter()
        .filter_map(|p| Some((p, find(a, p.a)?, find(b, p.b)?)))
        .map(|(p, ma, mb)| match (ma, mb) {
            (Mark::Stroke(sa), Mark::Stroke(sb)) => {
                let (oa, mut ob) = (points_of(&sa), points_of(&sb));
                if p.reversed {
                    let end = ob.last().map_or(0.0, |q| q.time);
                    ob.reverse();
                    ob.iter_mut().for_each(|q| q.time = end - q.time);
                }
                let n = oa.len().max(ob.len()).max(1);
                let (pa, pb) = (resample(&oa, n), resample(&ob, n));
                let (own, rest) = planned_points(&pa, &pb);
                Item::Stroke(StrokePlan {
                    a: sa,
                    b: sb,
                    pa,
                    pb,
                    own,
                    rel: own,
                    parent: None,
                    rest,
                    touches: [None, None],
                })
            }
            (Mark::Fill(fa), Mark::Fill(fb)) => match (&fa.source, &fb.source) {
                (FillSource::Polygon(_), FillSource::Polygon(_)) => {
                    let (oa, ob) = (outline(&Mark::Fill(fa.clone())), outline(&Mark::Fill(fb.clone())));
                    let n = oa.len().max(ob.len()).max(4);
                    let (pa, pb) = (resample(&oa, n), resample(&ob, n));
                    let (fit, rest) = planned_points(&pa, &pb);
                    Item::Polygon(ShapePlan {
                        a: fa,
                        b: fb,
                        pa: xy(&pa),
                        fit,
                        rest,
                    })
                }
                _ => Item::Flood { a: fa, b: fb },
            },
            _ => unreachable!("pairs share their kind"),
        })
        .collect();
    let alone = |marks: &[Mark], ids: &[u32], clashed: &[u32]| -> Vec<Mark> {
        marks
            .iter()
            .filter(|m| ids.contains(&m.id()) || clashed.contains(&m.id()))
            .cloned()
            .collect()
    };
    let (clash_a, clash_b): (Vec<u32>, Vec<u32>) = pairing.clashes.iter().copied().unzip();
    let alone = (
        alone(a, &pairing.alone_a, &clash_a),
        alone(b, &pairing.alone_b, &clash_b),
    );
    let mut plan = Plan {
        items,
        order: Vec::new(),
        pairing,
        alone,
    };
    plan.find_touches();
    plan.link_parents();
    plan
}

impl Plan {
    fn find_touches(&mut self) {
        let strokes: Vec<usize> = (0..self.items.len())
            .filter(|&i| matches!(self.items[i], Item::Stroke(_)))
            .collect();
        let side = |pick_b: bool| -> Vec<(&Stroke, Vec<(f64, f64)>)> {
            strokes
                .iter()
                .map(|&i| match &self.items[i] {
                    Item::Stroke(s) if pick_b => (&s.b, xy(&s.pb)),
                    Item::Stroke(s) => (&s.a, xy(&s.pa)),
                    _ => unreachable!(),
                })
                .collect()
        };
        let (in_a, in_b) = (side(false), side(true));
        let found: Vec<(usize, usize, Touch)> = (0..strokes.len())
            .flat_map(|k| (0..2).map(move |end| (k, end)))
            .filter_map(|(k, end)| {
                let ta = touch_of(&in_a, k, end, None)?;
                let tb = touch_of(&in_b, k, end, Some((ta.0, ta.1)))?;
                ((tb.1 - ta.1).abs() <= 0.5).then(|| {
                    (
                        strokes[k],
                        end,
                        Touch {
                            other: strokes[ta.0],
                            u: (ta.1, tb.1),
                            along: (ta.2, tb.2),
                            across: (ta.3, tb.3),
                        },
                    )
                })
            })
            .collect();
        found.into_iter().for_each(|(i, end, touch)| {
            if let Item::Stroke(s) = &mut self.items[i] {
                s.touches[end] = Some(touch);
            }
        });
    }

    fn link_parents(&mut self) {
        let strokes: Vec<usize> = (0..self.items.len())
            .filter(|&i| matches!(self.items[i], Item::Stroke(_)))
            .collect();
        let stroke = |i: usize| match &self.items[i] {
            Item::Stroke(s) => Some(s),
            _ => None,
        };
        let mut next: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        strokes.iter().for_each(|&i| {
            stroke(i)
                .into_iter()
                .flat_map(|s| s.touches.iter().flatten())
                .for_each(|t| {
                    next.entry(i).or_default().push(t.other);
                    next.entry(t.other).or_default().push(i);
                });
        });
        next.values_mut().for_each(|v| {
            v.sort_unstable();
            v.dedup();
        });
        let length = |i: usize| stroke(i).map_or(0.0, |s| walked(&s.pa).last().copied().unwrap_or(0.0));
        let mut roots = strokes.clone();
        roots.sort_by(|x, y| length(*y).total_cmp(&length(*x)).then(x.cmp(y)));
        let mut parent: Vec<Option<usize>> = vec![None; self.items.len()];
        let mut seen = vec![false; self.items.len()];
        let mut order = Vec::new();
        roots.into_iter().for_each(|root| {
            if seen[root] {
                return;
            }
            seen[root] = true;
            let mut queue = std::collections::VecDeque::from([root]);
            while let Some(i) = queue.pop_front() {
                order.push(i);
                next.get(&i).into_iter().flatten().for_each(|&n| {
                    if !seen[n] {
                        seen[n] = true;
                        parent[n] = Some(i);
                        queue.push_back(n);
                    }
                });
            }
        });
        let owns: Vec<Sim> = self
            .items
            .iter()
            .map(|item| match item {
                Item::Stroke(s) => s.own,
                _ => SAME,
            })
            .collect();
        self.items.iter_mut().enumerate().for_each(|(i, item)| {
            if let Item::Stroke(s) = item {
                s.parent = parent[i];
                s.rel = match parent[i] {
                    Some(p) => owns[p].inverse().then(&s.own),
                    None => s.own,
                };
            }
        });
        self.order = order;
    }

    pub fn touching_ends(&self) -> usize {
        self.items
            .iter()
            .map(|item| match item {
                Item::Stroke(s) => s.touches.iter().flatten().count(),
                _ => 0,
            })
            .sum()
    }

    pub fn floods(&self) -> Vec<(&Fill, &Fill)> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Flood { a, b } => Some((a, b)),
                _ => None,
            })
            .collect()
    }

    pub fn unpaired_at(&self, t: f32, how: Unpaired, now: &Lines) -> Vec<Mark> {
        let (la, lb) = self.key_lines();
        let side = |marks: &[Mark], lines: &Lines, first: bool| -> Vec<Mark> {
            marks
                .iter()
                .filter(|m| !matches!(m, Mark::Fill(f) if matches!(f.source, FillSource::Seed { .. })))
                .filter_map(|m| shown(&carry(m, lines, now, None), t, how, first))
                .collect()
        };
        side(&self.alone.0, &la, true)
            .into_iter()
            .chain(side(&self.alone.1, &lb, false))
            .collect()
    }

    pub fn unpaired_floods(&self) -> usize {
        self.alone
            .0
            .iter()
            .chain(&self.alone.1)
            .filter(|m| matches!(m, Mark::Fill(f) if matches!(f.source, FillSource::Seed { .. })))
            .count()
    }

    pub fn key_lines(&self) -> (Lines, Lines) {
        let strokes: Vec<&StrokePlan> = self
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Stroke(s) => Some(s),
                _ => None,
            })
            .collect();
        let line = |pts: &[P]| pts.iter().map(|p| (p.x as f32, p.y as f32)).collect::<Vec<_>>();
        (
            strokes.iter().map(|s| line(&s.pa)).collect(),
            strokes.iter().map(|s| line(&s.pb)).collect(),
        )
    }

    pub fn at(&self, t: f32) -> Vec<Mark> {
        let t64 = t as f64;
        let mut moved: Vec<Option<Sim>> = vec![None; self.items.len()];
        self.order.iter().for_each(|&i| {
            if let Item::Stroke(s) = &self.items[i] {
                let own = s.rel.arc(t64);
                moved[i] = Some(match s.parent.and_then(|p| moved[p]) {
                    Some(up) => up.then(&own),
                    None => own,
                });
            }
        });
        let mut lines: Vec<Option<Vec<P>>> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| match item {
                Item::Stroke(s) => {
                    let a = moved[i].unwrap_or(SAME);
                    Some(
                        s.pa.iter()
                            .zip(&s.pb)
                            .zip(&s.rest)
                            .map(|((p, q), r)| {
                                let (x, y) = a.apply((p.x, p.y));
                                P {
                                    x: x + r.0 * t64,
                                    y: y + r.1 * t64,
                                    ..mix(p, q, t64)
                                }
                            })
                            .collect(),
                    )
                }
                _ => None,
            })
            .collect();
        (0..self.items.len()).for_each(|i| {
            let Item::Stroke(s) = &self.items[i] else {
                return;
            };
            (0..2).for_each(|end| {
                let Some(touch) = s.touches[end] else {
                    return;
                };
                let Some(other) = lines[touch.other].as_ref().map(|l| xy(l)) else {
                    return;
                };
                let anchor = Anchor {
                    line: 0,
                    u: lerp(touch.u.0, touch.u.1, t64),
                    along: lerp(touch.along.0, touch.along.1, t64),
                    across: lerp(touch.across.0, touch.across.1, t64),
                };
                let Some(target) = place_on(&other, &anchor) else {
                    return;
                };
                if let Some(line) = lines[i].as_mut() {
                    snap(line, end, target);
                }
            });
        });
        self.items
            .iter()
            .zip(lines)
            .zip(moved)
            .map(|((item, line), sim)| match (item, line) {
                (Item::Stroke(s), Some(line)) => Mark::Stroke(stroke_at(s, &line, sim.unwrap_or(SAME), t)),
                (Item::Polygon(f), _) => Mark::Fill(polygon_at(f, t)),
                (Item::Flood { a, b }, _) => Mark::Fill(flood_at(a, b, t)),
                _ => unreachable!(),
            })
            .collect()
    }
}

fn snap(line: &mut [P], end: usize, target: (f64, f64)) {
    if line.is_empty() {
        return;
    }
    let order: Vec<usize> = match end {
        0 => (0..line.len()).collect(),
        _ => (0..line.len()).rev().collect(),
    };
    let at = line[order[0]];
    let (dx, dy) = (target.0 - at.x, target.1 - at.y);
    if dx * dx + dy * dy < 1e-6 {
        return;
    }
    let ordered: Vec<P> = order.iter().map(|&i| line[i]).collect();
    let along = walked(&ordered);
    let total = along[along.len() - 1];
    let span = (total * 0.35).clamp(1e-3, 48.0);
    order.iter().zip(along).for_each(|(&i, s)| {
        if s < span {
            let x = 1.0 - s / span;
            let w = x * x * (3.0 - 2.0 * x);
            line[i].x += dx * w;
            line[i].y += dy * w;
        }
    });
}

fn lerp32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn colour(a: [u16; 4], b: [u16; 4], t: f32) -> [u16; 4] {
    if a == b {
        return a;
    }
    let (ca, cb) = (from_u16(a), from_u16(b));
    let (la, lb) = (to_oklab(ca), to_oklab(cb));
    let lab = std::array::from_fn(|i| lerp32(la[i], lb[i], t));
    to_u16(from_oklab(lab, lerp32(ca[3], cb[3], t)))
}

fn stroke_at(s: &StrokePlan, line: &[P], sim: Sim, t: f32) -> Stroke {
    let mut last = 0.0;
    let samples = line
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let dt = if i == 0 { 0.0 } else { (p.time - last).max(0.0) };
            last = p.time;
            Sample::quantize(
                p.x as f32,
                p.y as f32,
                p.p as f32,
                (p.tx as f32, p.ty as f32),
                dt as f32,
            )
        })
        .collect();
    let mut cuts = s.a.cuts.clone();
    let a = affine32(sim.affine());
    cuts.iter_mut().for_each(|c| {
        c.map(|p| {
            let (x, y) = crate::markedit::apply_affine(a, (p[0], p[1]));
            [x, y]
        })
    });
    Stroke {
        id: s.a.id,
        brush: if t < 0.5 { s.a.brush.clone() } else { s.b.brush.clone() },
        color: colour(s.a.color, s.b.color, t),
        size: lerp32(s.a.size, s.b.size, t).max(0.05),
        opacity: lerp32(s.a.opacity, s.b.opacity, t).clamp(0.0, 1.0),
        flow: lerp32(s.a.flow, s.b.flow, t).max(1e-3),
        seed: s.a.seed,
        author: Author::Agent,
        taper: [
            lerp32(s.a.taper[0], s.b.taper[0], t),
            lerp32(s.a.taper[1], s.b.taper[1], t),
        ],
        lock: s.a.lock,
        cuts,
        second: match (s.a.second, s.b.second) {
            (Some(a), Some(b)) => Some(colour(a, b, t)),
            (a, b) => a.or(b),
        },
        samples,
    }
}

fn polygon_at(f: &ShapePlan, t: f32) -> Fill {
    let a = f.fit.arc(t as f64);
    let points: Vec<[f32; 2]> =
        f.pa.iter()
            .zip(&f.rest)
            .take(f.pa.len().saturating_sub(1).max(3))
            .map(|(p, r)| {
                let (x, y) = a.apply(*p);
                [(x + r.0 * t as f64) as f32, (y + r.1 * t as f64) as f32]
            })
            .collect();
    let mut cuts = f.a.cuts.clone();
    let a32 = affine32(a.affine());
    cuts.iter_mut().for_each(|c| {
        c.map(|p| {
            let (x, y) = crate::markedit::apply_affine(a32, (p[0], p[1]));
            [x, y]
        })
    });
    Fill {
        id: f.a.id,
        color: colour(f.a.color, f.b.color, t),
        opacity: lerp32(f.a.opacity, f.b.opacity, t).clamp(0.0, 1.0),
        author: Author::Agent,
        source: FillSource::Polygon(points),
        coverage: Coverage::default(),
        lock: f.a.lock,
        cuts,
    }
}

fn flood_at(a: &Fill, b: &Fill, t: f32) -> Fill {
    let source = match (&a.source, &b.source) {
        (FillSource::Seed { seed: sa, reference }, FillSource::Seed { seed: sb, .. }) => FillSource::Seed {
            seed: crate::fill::Seed {
                x: lerp32(sa.x, sb.x, t),
                y: lerp32(sa.y, sb.y, t),
                ..*sa
            },
            reference: reference.clone(),
        },
        (source, _) => source.clone(),
    };
    Fill {
        id: a.id,
        color: colour(a.color, b.color, t),
        opacity: lerp32(a.opacity, b.opacity, t).clamp(0.0, 1.0),
        author: Author::Agent,
        source,
        coverage: Coverage::default(),
        lock: a.lock,
        cuts: a.cuts.clone(),
    }
}

pub fn made_hash(marks: &[Mark]) -> String {
    let hash = marks.iter().fold(crate::rng::FNV_OFFSET, |h, m| {
        crate::rng::fnv(h, &fingerprint(m).to_le_bytes())
    });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;
    use crate::fill::{FillRef, Seed};

    fn stroke(id: u32, points: &[(f32, f32)], size: f32) -> Mark {
        let pts: Vec<(f32, f32, f32)> = points.iter().map(|&(x, y)| (x, y, 1.0)).collect();
        Stroke::new(id, "ink", [0.0, 0.0, 0.0, 1.0], size, 7)
            .with_points(&pts, 8.0)
            .into()
    }

    fn arm(angle: f32) -> Vec<(f32, f32)> {
        let (s, c) = (libm::sinf(angle.to_radians()), libm::cosf(angle.to_radians()));
        (0..=20)
            .map(|i| {
                let r = i as f32 * 10.0;
                (100.0 + r * c, 100.0 + r * s)
            })
            .collect()
    }

    fn ends(mark: &Mark) -> ((f32, f32), (f32, f32)) {
        let s = mark.as_stroke().unwrap();
        (s.samples[0].pos(), s.samples[s.samples.len() - 1].pos())
    }

    fn length(mark: &Mark) -> f32 {
        let s = mark.as_stroke().unwrap();
        s.samples
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0].pos(), w[1].pos());
                libm::sqrtf((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2))
            })
            .sum()
    }

    #[test]
    fn spacing_bunches_in_betweens_where_asked() {
        assert_eq!(Spacing::Even.ts(3), [0.25, 0.5, 0.75]);
        assert_eq!(Spacing::In.ts(1), [0.25]);
        assert_eq!(Spacing::Out.ts(1), [0.75]);
        assert_eq!(Spacing::InOut.ts(1), [0.5]);
        let list = Spacing::parse("0.1,0.4").unwrap();
        assert_eq!((list.count(), list.ts(2)), (Some(2), vec![0.1, 0.4]));
        assert!(Spacing::parse("0.5,0.2").is_err() && Spacing::parse("0,1").is_err());
    }

    #[test]
    fn a_swinging_arm_turns_about_its_shoulder_and_keeps_its_length() {
        let a = [stroke(1, &arm(0.0), 6.0)];
        let b = [stroke(1, &arm(90.0), 6.0)];
        let plan = plan(&a, &b, Match::Id, &[]);
        let mid = plan.at(0.5);
        let (start, end) = ends(&mid[0]);
        assert!(
            (start.0 - 100.0).abs() < 0.2 && (start.1 - 100.0).abs() < 0.2,
            "{start:?}"
        );
        let diagonal = 200.0 / std::f32::consts::SQRT_2;
        assert!(
            (end.0 - 100.0 - diagonal).abs() < 0.5 && (end.1 - 100.0 - diagonal).abs() < 0.5,
            "{end:?}"
        );
        assert!((length(&mid[0]) - 200.0).abs() < 1.0, "{}", length(&mid[0]));
        let straight = {
            let (p, q) = (ends(&a[0]).1, ends(&b[0]).1);
            ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)
        };
        assert!(libm::sqrtf((end.0 - straight.0).powi(2) + (end.1 - straight.1).powi(2)) > 40.0);
        assert_eq!(mid[0].as_stroke().unwrap().author, Author::Agent);
    }

    #[test]
    fn a_hand_bending_at_the_wrist_rides_on_the_turning_forearm() {
        let forearm = |angle: f32| {
            let (s, c) = (libm::sinf(angle.to_radians()), libm::cosf(angle.to_radians()));
            (0..=20)
                .map(|i| (100.0 + i as f32 * 10.0 * c, 100.0 + i as f32 * 10.0 * s))
                .collect::<Vec<_>>()
        };
        let hand = |wrist: (f32, f32), angle: f32| {
            let (s, c) = (libm::sinf(angle.to_radians()), libm::cosf(angle.to_radians()));
            (0..=6)
                .map(|i| (wrist.0 + i as f32 * 10.0 * c, wrist.1 + i as f32 * 10.0 * s))
                .collect::<Vec<_>>()
        };
        let a = [
            stroke(1, &forearm(0.0), 6.0),
            stroke(2, &hand((300.0, 100.0), 0.0), 4.0),
        ];
        let b = [
            stroke(1, &forearm(90.0), 6.0),
            stroke(2, &hand((100.0, 300.0), 130.0), 4.0),
        ];
        let plan = plan(&a, &b, Match::Id, &[]);
        let mid = plan.at(0.5);
        let wrist = ends(&mid[0]).1;
        let (start, end) = ends(&mid[1]);
        let d = |p: (f32, f32), q: (f32, f32)| libm::sqrtf((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2));
        assert!(d(start, wrist) < 1.0, "{start:?} {wrist:?}");
        let want = (
            wrist.0 + 60.0 * libm::cosf(65f32.to_radians()),
            wrist.1 + 60.0 * libm::sinf(65f32.to_radians()),
        );
        assert!(d(end, want) < 1.5, "{end:?} {want:?}");
        assert!((length(&mid[1]) - 60.0).abs() < 1.0);
    }

    #[test]
    fn pairs_by_id_by_order_and_by_nearest_shape() {
        let a = vec![
            stroke(1, &[(0.0, 0.0), (10.0, 0.0)], 4.0),
            stroke(2, &[(0.0, 50.0), (10.0, 50.0)], 4.0),
        ];
        let b = vec![
            stroke(5, &[(10.0, 51.0), (0.0, 51.0)], 4.0),
            stroke(1, &[(1.0, 1.0), (11.0, 1.0)], 4.0),
        ];
        let by_id = pair(&a, &b, Match::Id, &[]);
        assert_eq!(by_id.pairs.len(), 1);
        assert_eq!((by_id.alone_a.clone(), by_id.alone_b.clone()), (vec![2], vec![5]));
        let by_order = pair(&a, &b, Match::Order, &[]);
        assert_eq!(
            by_order.pairs.iter().map(|p| (p.a, p.b)).collect::<Vec<_>>(),
            [(1, 5), (2, 1)]
        );
        let near = pair(&a, &b, Match::Near, &[]);
        assert_eq!(
            near.pairs.iter().map(|p| (p.a, p.b, p.reversed)).collect::<Vec<_>>(),
            [(1, 1, false), (2, 5, true)]
        );
        let forced = pair(&a, &b, Match::Id, &[[2, 1]]);
        assert_eq!(forced.pairs.iter().map(|p| (p.a, p.b)).collect::<Vec<_>>(), [(2, 1)]);
        assert_eq!(forced.alone_a, [1]);
    }

    #[test]
    fn line_ends_that_touch_in_both_keys_touch_in_between() {
        let bar = |dy: f32| stroke(1, &[(0.0, 100.0 + dy), (200.0, 100.0 + dy)], 6.0);
        let post_a = stroke(2, &[(60.0, 0.0), (60.0, 98.0)], 4.0);
        let post_b = stroke(2, &[(140.0, 20.0), (140.0, 158.0)], 4.0);
        let a = [bar(0.0), post_a];
        let b = [bar(60.0), post_b];
        let plan = plan(&a, &b, Match::Id, &[]);
        assert_eq!(plan.touching_ends(), 1);
        let mid = plan.at(0.5);
        let (_, end) = ends(&mid[1]);
        let bar_y = mid[0].as_stroke().unwrap().samples[0].pos().1;
        assert!((end.1 - (bar_y - 2.0)).abs() < 1.0, "{end:?} against {bar_y}");
        let open = [bar(0.0), stroke(2, &[(60.0, 0.0), (60.0, 60.0)], 4.0)];
        assert_eq!(super::plan(&open, &b, Match::Id, &[]).touching_ends(), 0);
    }

    #[test]
    fn polygon_fills_interpolate_and_floods_carry_their_settings() {
        let square = |x: f32| {
            Mark::Fill(Fill::new(
                3,
                [1.0, 0.0, 0.0, 1.0],
                FillSource::Polygon(vec![[x, 0.0], [x + 10.0, 0.0], [x + 10.0, 10.0], [x, 10.0]]),
                Coverage::default(),
            ))
        };
        let flood = |x: f32| {
            Mark::Fill(Fill::new(
                4,
                [0.0, 0.0, 1.0, 1.0],
                FillSource::Seed {
                    seed: Seed {
                        x,
                        y: 5.0,
                        tolerance: 0.2,
                        gap: 3.0,
                        expand: 2.0,
                    },
                    reference: FillRef::Layer("l1".into()),
                },
                Coverage::default(),
            ))
        };
        let plan = plan(&[square(0.0), flood(0.0)], &[square(40.0), flood(40.0)], Match::Id, &[]);
        let mid = plan.at(0.5);
        let FillSource::Polygon(points) = &mid[0].as_fill().unwrap().source else {
            panic!()
        };
        assert_eq!(points.len(), 4);
        assert!((points[0][0] - 20.0).abs() < 0.01, "{points:?}");
        let FillSource::Seed { seed, reference } = &mid[1].as_fill().unwrap().source else {
            panic!()
        };
        assert_eq!((seed.x, seed.gap, reference.name()), (20.0, 3.0, "l1"));
        assert_eq!(plan.floods().len(), 1);
    }

    #[test]
    fn colours_blend_between_keys_through_oklab() {
        let coloured = |c: Rgba| -> Mark {
            let pts: Vec<(f32, f32, f32)> = arm(0.0).iter().map(|&(x, y)| (x, y, 1.0)).collect();
            Stroke::new(1, "ink", c, 6.0, 7).with_points(&pts, 8.0).into()
        };
        let lit = [0.93, 0.62, 0.25, 1.0];
        let shade = [0.36, 0.38, 0.72, 1.0];
        let fill = |c: Rgba| {
            Mark::Fill(Fill::new(
                3,
                c,
                FillSource::Polygon(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]),
                Coverage::default(),
            ))
        };
        let made = plan(
            &[coloured(lit), fill(lit)],
            &[coloured(shade), fill(shade)],
            Match::Id,
            &[],
        );
        let mid = made.at(0.5);
        let (la, lb) = (to_oklab(lit), to_oklab(shade));
        let want: [f32; 3] = std::array::from_fn(|i| (la[i] + lb[i]) / 2.0);
        [mid[0].as_stroke().unwrap().color, mid[1].as_fill().unwrap().color]
            .iter()
            .for_each(|&c| {
                let got = to_oklab(from_u16(c));
                assert!((0..3).all(|i| (got[i] - want[i]).abs() < 0.004), "{got:?} {want:?}");
            });
        let same = plan(&[coloured(lit)], &[coloured(lit)], Match::Id, &[]);
        assert_eq!(
            same.at(0.5)[0].as_stroke().unwrap().color,
            to_u16(lit),
            "keys of one colour keep it exactly"
        );
    }

    #[test]
    fn anchors_carry_a_point_with_the_line_it_sits_by() {
        let line = vec![(0.0, 0.0), (100.0, 0.0)];
        let at = anchor(std::slice::from_ref(&line), (25.0, 10.0)).unwrap();
        let turned = vec![(0.0, 0.0), (0.0, 100.0)];
        let (x, y) = place(&at, &[turned]).unwrap();
        assert!((x + 10.0).abs() < 1e-3 && (y - 25.0).abs() < 1e-3, "{x},{y}");
    }
}
