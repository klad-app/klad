use crate::attach;
use crate::color::{parse_hex, to_u16};
use crate::cut::{self, Cut, Keep, inside_polygon};
use crate::doc::MarkIds;
use crate::fill::{Coverage, Fill, FillSource};
use crate::stroke::{Mark, POS_SCALE, Sample, Stroke};
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub type Affine = [f32; 6];

pub const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
const STROKE_KEYS: &[&str] = &["color", "size", "opacity", "flow", "brush", "taper", "path"];
const FILL_KEYS: &[&str] = &["color", "opacity"];
const CELL_KEYS: &[&str] = &["color", "opacity", "entry"];
pub const TRANSFORM_KEYS: &[&str] = &["x", "y", "rotate", "sx", "sy", "pivot"];

pub fn parse_address(text: &str) -> Option<(&str, u32)> {
    let (cel, mark) = text.split_once(':')?;
    let id = crate::doc::parse_mark(mark)?;
    crate::doc::valid_id(cel).then_some((cel, id))
}

pub fn address(cel: &str, id: u32) -> String {
    format!("{cel}:{}", crate::doc::mark_name(id))
}

pub fn apply_affine(a: Affine, (x, y): (f32, f32)) -> (f32, f32) {
    (a[0] * x + a[1] * y + a[4], a[2] * x + a[3] * y + a[5])
}

pub fn invert(a: Affine) -> Option<Affine> {
    let det = a[0] * a[3] - a[1] * a[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let (ia, ib, ic, id) = (a[3] / det, -a[1] / det, -a[2] / det, a[0] / det);
    Some([ia, ib, ic, id, -(ia * a[4] + ib * a[5]), -(ic * a[4] + id * a[5])])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkTransform {
    pub x: f32,
    pub y: f32,
    pub rotate: f32,
    pub sx: f32,
    pub sy: f32,
    pub pivot: Option<[f32; 2]>,
}

impl Default for MarkTransform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            rotate: 0.0,
            sx: 1.0,
            sy: 1.0,
            pivot: None,
        }
    }
}

impl MarkTransform {
    pub fn parse(map: &mut Map<String, Value>) -> Result<Self> {
        let mut t = Self::default();
        let mut number = |key: &str, slot: &mut f32| -> Result<()> {
            if let Some(v) = map.remove(key) {
                *slot = v.as_f64().ok_or_else(|| anyhow!("{key} must be a number"))? as f32;
            }
            Ok(())
        };
        number("x", &mut t.x)?;
        number("y", &mut t.y)?;
        number("rotate", &mut t.rotate)?;
        number("sx", &mut t.sx)?;
        number("sy", &mut t.sy)?;
        if let Some(scale) = map.remove("scale") {
            let k = scale.as_f64().ok_or_else(|| anyhow!("scale must be a number"))? as f32;
            t.sx *= k;
            t.sy *= k;
        }
        t.pivot = match map.remove("pivot") {
            None | Some(Value::Null) => None,
            Some(v) => Some(serde_json::from_value(v).map_err(|_| anyhow!("pivot must be [x, y]"))?),
        };
        if t.sx == 0.0 || t.sy == 0.0 {
            bail!("sx and sy cannot be 0");
        }
        Ok(t)
    }

    pub fn affine(&self, pivot: [f32; 2]) -> Affine {
        let (s, c) = (
            libm::sinf(self.rotate.to_radians()),
            libm::cosf(self.rotate.to_radians()),
        );
        let (a, b, cc, d) = (c * self.sx, -s * self.sy, s * self.sx, c * self.sy);
        let [px, py] = pivot;
        [
            a,
            b,
            cc,
            d,
            px + self.x - (a * px + b * py),
            py + self.y - (cc * px + d * py),
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarkChange {
    Add(Vec<Mark>),
    Set {
        id: u32,
        props: Map<String, Value>,
    },
    Remove {
        id: u32,
    },
    Transform {
        ids: Vec<u32>,
        transform: MarkTransform,
    },
    Lasso {
        polygon: Vec<[f32; 2]>,
        cut: bool,
        action: LassoAction,
        chosen: Vec<u32>,
    },
    Copy {
        layer: String,
        cel: String,
        upto: usize,
    },
    Paste {
        sources: Vec<Source>,
        transform: Option<MarkTransform>,
        cut: Option<Vec<[f32; 2]>>,
        made: Vec<Copied>,
    },
    Put(Vec<Mark>),
    Part {
        name: String,
        count: usize,
        ids: Vec<u32>,
    },
    Detach {
        ids: Vec<u32>,
    },
    Entry {
        entry: String,
        color: Option<[u16; 4]>,
        changed: Vec<u32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub layer: String,
    pub cel: String,
    pub ids: Vec<u32>,
    pub upto: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    pub cel: String,
    pub from: u32,
    pub to: u32,
}

impl MarkChange {
    pub fn ids(&self) -> Vec<u32> {
        match self {
            MarkChange::Add(marks) => marks.iter().map(Mark::id).collect(),
            MarkChange::Set { id, .. } | MarkChange::Remove { id } => vec![*id],
            MarkChange::Transform { ids, .. } => ids.clone(),
            MarkChange::Lasso { chosen, .. } => chosen.clone(),
            MarkChange::Paste { made, .. } => made.iter().map(|c| c.to).collect(),
            MarkChange::Put(marks) => marks.iter().map(Mark::id).collect(),
            MarkChange::Detach { ids } => ids.clone(),
            MarkChange::Entry { changed, .. } => changed.clone(),
            MarkChange::Copy { .. } | MarkChange::Part { .. } => Vec::new(),
        }
    }

    pub fn brushes(&self) -> Vec<&str> {
        match self {
            MarkChange::Add(marks) | MarkChange::Put(marks) => {
                crate::stroke::strokes_of(marks).map(|s| s.brush.as_str()).collect()
            }
            MarkChange::Set { props, .. } => props.get("brush").and_then(Value::as_str).into_iter().collect(),
            _ => Vec::new(),
        }
    }
}

pub fn union(marks: &[&Mark]) -> Option<[f32; 4]> {
    marks
        .iter()
        .filter_map(|m| m.bounds())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

fn number(props: &Map<String, Value>, key: &str) -> Result<Option<f32>> {
    props
        .get(key)
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| anyhow!("{key} must be a number"))
        })
        .transpose()
}

fn check_keys(props: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<()> {
    props.keys().try_for_each(|key| {
        if allowed.contains(&key.as_str()) {
            Ok(())
        } else {
            bail!("a {what} has no {key:?}; it takes {}", allowed.join(", "))
        }
    })
}

fn opacity(props: &Map<String, Value>) -> Result<Option<f32>> {
    let value = number(props, "opacity")?;
    if value.is_some_and(|o| !(0.0..=1.0).contains(&o)) {
        bail!("opacity must be within 0..1");
    }
    Ok(value)
}

fn color(props: &Map<String, Value>) -> Result<Option<[u16; 4]>> {
    props
        .get("color")
        .map(|v| {
            let text = v.as_str().ok_or_else(|| anyhow!("color must be a string"))?;
            Ok(to_u16(parse_hex(text)?))
        })
        .transpose()
}

fn set_stroke(stroke: &mut Stroke, props: &Map<String, Value>) -> Result<()> {
    check_keys(props, STROKE_KEYS, "stroke")?;
    if let Some(c) = color(props)? {
        stroke.color = c;
    }
    if let Some(o) = opacity(props)? {
        stroke.opacity = o;
    }
    match (number(props, "size")?, number(props, "flow")?) {
        (Some(s), _) if s <= 0.0 => bail!("size must be positive"),
        (_, Some(f)) if f <= 0.0 => bail!("flow must be positive"),
        (size, flow) => {
            stroke.size = size.unwrap_or(stroke.size);
            stroke.flow = flow.unwrap_or(stroke.flow);
        }
    }
    if let Some(brush) = props.get("brush") {
        let id = brush.as_str().ok_or_else(|| anyhow!("brush must be a brush id"))?;
        if !crate::doc::valid_id(id) {
            bail!("brush id {id:?} may only use letters, digits, - and _");
        }
        stroke.brush = id.to_string();
    }
    match props.get("taper") {
        None => {}
        Some(Value::Null) => stroke.taper = [0.0, 0.0],
        Some(v) => {
            let taper: [f32; 2] =
                serde_json::from_value(v.clone()).map_err(|_| anyhow!("taper must be [in, out] lengths in px"))?;
            if taper.iter().any(|t| !(0.0..=10_000.0).contains(t)) {
                bail!("taper lengths must be within 0..10000 px");
            }
            stroke.taper = taper;
        }
    }
    if let Some(path) = props.get("path") {
        let d = path.as_str().ok_or_else(|| anyhow!("path must be SVG path data"))?;
        reshape(stroke, d)?;
    }
    Ok(())
}

fn running(points: &[[f32; 2]]) -> Vec<f32> {
    points
        .iter()
        .scan((None::<[f32; 2]>, 0.0f32), |(last, total), p| {
            if let Some(q) = last {
                *total += libm::sqrtf((p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]));
            }
            *last = Some(*p);
            Some(*total)
        })
        .collect()
}

fn at_length(line: &[[f32; 2]], lengths: &[f32], at: f32) -> [f32; 2] {
    let i = lengths.partition_point(|l| *l < at).clamp(1, line.len().max(2) - 1);
    let (a, b) = (line[i - 1], line[i]);
    let span = (lengths[i] - lengths[i - 1]).max(1e-6);
    let t = ((at - lengths[i - 1]) / span).clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub fn reshape(stroke: &mut Stroke, d: &str) -> Result<()> {
    let line: Vec<[f32; 2]> = crate::path::flatten(d)?.into_iter().flatten().collect();
    if line.is_empty() {
        bail!("the path has no points");
    }
    let place = |s: &mut Sample, p: [f32; 2]| {
        s.x = (p[0] * POS_SCALE).round() as i32;
        s.y = (p[1] * POS_SCALE).round() as i32;
    };
    if line.len() == 1 {
        stroke.samples.iter_mut().for_each(|s| place(s, line[0]));
        return Ok(());
    }
    let old: Vec<[f32; 2]> = stroke
        .samples
        .iter()
        .map(|s| {
            let (x, y) = s.pos();
            [x, y]
        })
        .collect();
    let (before, after) = (running(&old), running(&line));
    let (from, to) = (
        before.last().copied().unwrap_or(0.0),
        after.last().copied().unwrap_or(0.0),
    );
    stroke.samples.iter_mut().zip(&before).for_each(|(s, walked)| {
        let t = if from > 0.0 { walked / from } else { 0.0 };
        place(s, at_length(&line, &after, t * to));
    });
    Ok(())
}

fn set_fill(fill: &mut Fill, props: &Map<String, Value>) -> Result<()> {
    match &mut fill.source {
        FillSource::Cells { erase: true, .. } => {
            bail!("an erasing mark has no colour or opacity to set; remove it instead")
        }
        FillSource::Cells { entry, .. } => {
            check_keys(props, CELL_KEYS, "pixel mark")?;
            if let Some(bound) = props.get("entry").and_then(Value::as_str) {
                *entry = bound.to_string();
            }
        }
        _ => check_keys(props, FILL_KEYS, "fill")?,
    }
    if let Some(c) = color(props)? {
        fill.color = c;
    }
    if let Some(o) = opacity(props)? {
        fill.opacity = o;
    }
    Ok(())
}

pub fn transform_stroke(stroke: &mut Stroke, a: Affine) {
    let scale = libm::sqrtf((a[0] * a[3] - a[1] * a[2]).abs());
    stroke.samples.iter_mut().for_each(|s| {
        let (x, y) = apply_affine(a, s.pos());
        let (tx, ty) = s.tilt();
        let amount = libm::sqrtf(tx * tx + ty * ty);
        let (dx, dy) = (a[0] * tx + a[1] * ty, a[2] * tx + a[3] * ty);
        let len = libm::sqrtf(dx * dx + dy * dy);
        let tilt = if len > 1e-6 {
            (dx / len * amount, dy / len * amount)
        } else {
            (tx, ty)
        };
        *s = Sample::quantize(x, y, s.pressure(), tilt, s.dt_ms as f32);
    });
    stroke.size = (stroke.size * scale).max(0.05);
    stroke.taper = stroke.taper.map(|t| t * scale);
    stroke.cuts.iter_mut().for_each(|c| c.map(|p| mapped(a, p)));
}

fn mapped(a: Affine, p: [f32; 2]) -> [f32; 2] {
    let (x, y) = apply_affine(a, (p[0], p[1]));
    [x, y]
}

fn translation(a: Affine) -> Option<(i64, i64)> {
    let plain = a[0] == 1.0 && a[1] == 0.0 && a[2] == 0.0 && a[3] == 1.0;
    plain.then(|| (a[4].round() as i64, a[5].round() as i64))
}

fn shifted(c: &Coverage, dx: i64, dy: i64) -> Coverage {
    let (x0, y0) = (c.x as i64 + dx, c.y as i64 + dy);
    let (cut_x, cut_y) = ((-x0).max(0), (-y0).max(0));
    let (w, h) = (c.width as i64 - cut_x, c.height as i64 - cut_y);
    if w <= 0 || h <= 0 {
        return Coverage::default();
    }
    let alpha = (cut_y..c.height as i64)
        .flat_map(|row| {
            let start = (row * c.width as i64 + cut_x) as usize;
            c.alpha[start..start + w as usize].iter().copied()
        })
        .collect();
    Coverage {
        x: (x0 + cut_x) as u32,
        y: (y0 + cut_y) as u32,
        width: w as u32,
        height: h as u32,
        alpha,
    }
}

fn resampled(c: &Coverage, a: Affine, hard: bool) -> Coverage {
    let Some(inverse) = invert(a) else {
        return Coverage::default();
    };
    let (x0, y0, x1, y1) = (c.x as f32, c.y as f32, (c.x + c.width) as f32, (c.y + c.height) as f32);
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|p| apply_affine(a, p));
    let lo = |f: fn(&(f32, f32)) -> f32| corners.iter().map(f).fold(f32::MAX, f32::min);
    let hi = |f: fn(&(f32, f32)) -> f32| corners.iter().map(f).fold(f32::MIN, f32::max);
    let (bx0, by0) = (lo(|p| p.0).floor().max(0.0) as u32, lo(|p| p.1).floor().max(0.0) as u32);
    let (bx1, by1) = (hi(|p| p.0).ceil().max(0.0) as u32, hi(|p| p.1).ceil().max(0.0) as u32);
    if bx1 <= bx0 || by1 <= by0 {
        return Coverage::default();
    }
    let (w, h) = (bx1 - bx0, by1 - by0);
    let at = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 {
            return 0.0;
        }
        c.at(x as u32, y as u32) as f32
    };
    let mask: Vec<u8> = (0..h)
        .flat_map(|j| (0..w).map(move |i| (i, j)))
        .map(|(i, j)| {
            let (sx, sy) = apply_affine(inverse, ((bx0 + i) as f32 + 0.5, (by0 + j) as f32 + 0.5));
            if hard {
                return match at(sx.floor() as i64, sy.floor() as i64) > 0.0 {
                    true => 255,
                    false => 0,
                };
            }
            let (fx, fy) = (sx - 0.5, sy - 0.5);
            let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
            let (tx, ty) = (fx - ix as f32, fy - iy as f32);
            let top = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
            let bottom = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
            (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8
        })
        .collect();
    let mut out = Coverage::from_mask(w, h, &mask);
    if !out.is_empty() {
        out.x += bx0;
        out.y += by0;
    }
    out
}

pub fn transform_fill(fill: &mut Fill, a: Affine) {
    let map = |p: [f32; 2]| mapped(a, p);
    match &mut fill.source {
        FillSource::Seed { seed, .. } => [seed.x, seed.y] = map([seed.x, seed.y]),
        FillSource::Polygon(points) => points.iter_mut().for_each(|p| *p = map(*p)),
        FillSource::Cells { .. } => {}
    }
    let (coverage, region) = match translation(a) {
        Some((dx, dy)) => (
            shifted(&fill.coverage, dx, dy),
            [1.0, 0.0, 0.0, 1.0, dx as f32, dy as f32],
        ),
        None => (resampled(&fill.coverage, a, fill.on_grid()), a),
    };
    fill.coverage = coverage;
    fill.cuts.iter_mut().for_each(|c| c.map(|p| mapped(region, p)));
}

pub fn transform_mark(mark: &mut Mark, a: Affine) {
    match mark {
        Mark::Stroke(s) => transform_stroke(s, a),
        Mark::Fill(f) => transform_fill(f, a),
    }
}

fn to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let span = dx * dx + dy * dy;
    let t = if span < 1e-9 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / span).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + dx * t - p.0, a.1 + dy * t - p.1);
    libm::sqrtf(x * x + y * y)
}

fn touches(mark: &Mark, at: (f32, f32), slack: f32) -> bool {
    match mark {
        Mark::Stroke(s) => {
            let reach = s.size / 2.0 + slack;
            let points: Vec<(f32, f32)> = s.samples.iter().map(Sample::pos).collect();
            match points.len() {
                0 => false,
                1 => to_segment(at, points[0], points[0]) <= reach,
                _ => points.windows(2).any(|w| to_segment(at, w[0], w[1]) <= reach),
            }
        }
        Mark::Fill(f) => at.0 >= 0.0 && at.1 >= 0.0 && f.coverage.at(at.0 as u32, at.1 as u32) > 0,
    }
}

pub fn hit(marks: &[Mark], at: (f32, f32), slack: f32) -> Option<u32> {
    pick(marks, at, slack, |_| false)
}

pub fn pick(marks: &[Mark], at: (f32, f32), slack: f32, hidden: impl Fn(&Mark) -> bool) -> Option<u32> {
    let found = |slack: f32| {
        marks
            .iter()
            .rev()
            .find(|m| cut::keeps(m.cuts(), at) && touches(m, at, slack) && !hidden(m))
            .map(Mark::id)
    };
    found(0.0).or_else(|| (slack > 0.0).then(|| found(slack)).flatten())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Inside,
    Crossing,
    Outside,
}

fn fill_side(fill: &Fill, polygon: &[[f32; 2]]) -> Side {
    let c = &fill.coverage;
    let (inside, outside) = (0..c.height).fold((false, false), |(inside, outside), row| {
        if inside && outside {
            return (inside, outside);
        }
        let y = (c.y + row) as f32 + 0.5;
        let xs = cut::crossings(polygon, y);
        let line = &c.alpha[(row * c.width) as usize..((row + 1) * c.width) as usize];
        line.iter()
            .enumerate()
            .filter(|(_, a)| **a > 0)
            .map(|(i, _)| ((c.x + i as u32) as f32 + 0.5, y))
            .filter(|p| cut::keeps(&fill.cuts, *p))
            .fold((inside, outside), |(inside, outside), p| {
                let odd = xs.iter().filter(|x| **x > p.0).count() % 2 == 1;
                (inside || odd, outside || !odd)
            })
    });
    match (inside, outside) {
        (true, false) => Side::Inside,
        (true, true) => Side::Crossing,
        _ => Side::Outside,
    }
}

pub fn side(mark: &Mark, polygon: &[[f32; 2]]) -> Side {
    match mark {
        Mark::Stroke(s) => {
            let kept: Vec<(f32, f32)> = s
                .samples
                .iter()
                .map(Sample::pos)
                .filter(|p| cut::keeps(&s.cuts, *p))
                .collect();
            let inside = kept.iter().filter(|p| inside_polygon(polygon, **p)).count();
            match inside {
                0 => Side::Outside,
                n if n == kept.len() => Side::Inside,
                _ => Side::Crossing,
            }
        }
        Mark::Fill(f) => fill_side(f, polygon),
    }
}

pub fn lassoed(marks: &[Mark], polygon: &[[f32; 2]]) -> Vec<u32> {
    sides(marks, polygon)
        .into_iter()
        .filter(|(_, side)| *side == Side::Inside)
        .map(|(id, _)| id)
        .collect()
}

pub fn sides(marks: &[Mark], polygon: &[[f32; 2]]) -> Vec<(u32, Side)> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let following = attach::following(marks);
    marks
        .iter()
        .filter(|m| !following.contains(&m.id()))
        .map(|m| (m.id(), side(m, polygon)))
        .filter(|(_, side)| *side != Side::Outside)
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum LassoAction {
    Transform(MarkTransform),
    Remove,
}

pub fn split(marks: &mut Vec<Mark>, polygon: &[[f32; 2]], cut: bool, ids: MarkIds) -> Vec<u32> {
    let found = sides(marks, polygon);
    let mut next = ids.next(marks);
    let mut chosen: Vec<u32> = found
        .iter()
        .filter(|(_, s)| *s == Side::Inside)
        .map(|(id, _)| *id)
        .collect();
    if !cut {
        return chosen;
    }
    let crossing: Vec<u32> = found
        .iter()
        .filter(|(_, s)| *s == Side::Crossing)
        .map(|(id, _)| *id)
        .collect();
    let mut pieces: Vec<(u32, u32)> = Vec::new();
    let sliced = std::mem::take(marks).into_iter().flat_map(|mut mark| {
        if !crossing.contains(&mark.id()) {
            return vec![mark];
        }
        let mut part = mark.clone();
        part.set_id(next);
        chosen.push(next);
        pieces.push((mark.id(), next));
        next += 1;
        mark.cuts_mut().push(Cut {
            keep: Keep::Outside,
            polygon: polygon.to_vec(),
        });
        part.cuts_mut().push(Cut {
            keep: Keep::Inside,
            polygon: polygon.to_vec(),
        });
        vec![mark, part]
    });
    *marks = sliced.collect();
    attach::join(marks, &pieces);
    chosen
}

pub fn bounds_of(marks: &[Mark], ids: &[u32]) -> Option<[f32; 4]> {
    let chosen: Vec<&Mark> = marks.iter().filter(|m| ids.contains(&m.id())).collect();
    union(&chosen)
}

fn position(marks: &[Mark], cel: &str, id: u32) -> Result<usize> {
    marks
        .iter()
        .position(|m| m.id() == id)
        .ok_or_else(|| anyhow!("{} does not exist", address(cel, id)))
}

pub fn apply(marks: &mut Vec<Mark>, change: &mut MarkChange, cel: &str, ids: MarkIds) -> Result<()> {
    apply_following(marks, change, cel, ids).map(|_| ())
}

pub fn apply_following(marks: &mut Vec<Mark>, change: &mut MarkChange, cel: &str, ids: MarkIds) -> Result<Vec<u32>> {
    let before: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    let mut alone = Vec::new();
    match change {
        MarkChange::Add(added) => {
            let next = ids.next(marks);
            added
                .iter_mut()
                .enumerate()
                .for_each(|(i, m)| m.set_id(next + i as u32));
            marks.extend(added.iter().cloned());
        }
        MarkChange::Set { id, props } => {
            let at = position(marks, cel, *id)?;
            let what = address(cel, *id);
            let line: Option<Vec<(f32, f32)>> = marks[at]
                .as_stroke()
                .filter(|_| props.contains_key("path"))
                .map(|s| s.samples.iter().map(Sample::pos).collect());
            match &mut marks[at] {
                Mark::Stroke(s) => set_stroke(s, props),
                Mark::Fill(f) => set_fill(f, props),
            }
            .map_err(|e| anyhow!("set {what}: {e}"))?;
            if let Some(line) = line {
                attach::bend(marks, *id, &line, ids);
            }
        }
        MarkChange::Remove { id } => {
            let at = position(marks, cel, *id)?;
            marks.remove(at);
            attach::prune(marks, &before);
        }
        MarkChange::Transform { ids: chosen, transform } => {
            let pivot = pivot_of(marks, chosen, transform, cel)?;
            let followed = attach::follow(marks, chosen, ids);
            transform_marks(marks, &followed.moving, transform.affine(pivot), cel)?;
            alone = followed.alone;
        }
        MarkChange::Lasso {
            polygon,
            cut,
            action,
            chosen,
        } => {
            if polygon.len() < 3 {
                bail!("a lasso needs at least 3 points");
            }
            *chosen = split(marks, polygon, *cut, ids);
            if chosen.is_empty() {
                bail!(
                    "the lasso holds no strokes or fills of {cel}{}",
                    if *cut {
                        ""
                    } else {
                        " whole (cut takes the parts crossing its edge)"
                    }
                );
            }
            match action {
                LassoAction::Transform(transform) => {
                    let pivot = pivot_of(marks, chosen, transform, cel)?;
                    let followed = attach::follow(marks, chosen, ids);
                    transform_marks(marks, &followed.moving, transform.affine(pivot), cel)?;
                    alone = followed.alone;
                }
                LassoAction::Remove => {
                    marks.retain(|m| !chosen.contains(&m.id()));
                    attach::prune(marks, &before);
                }
            }
        }
        MarkChange::Put(put) => *marks = put.clone(),
        MarkChange::Part { count, ids, .. } if *count > 0 => {
            if *count > marks.len() {
                bail!("{cel} has {} marks, fewer than the {count} just added", marks.len());
            }
            *ids = marks[marks.len() - *count..].iter().map(Mark::id).collect();
        }
        MarkChange::Part { ids, .. } => {
            ids.iter().try_for_each(|id| position(marks, cel, *id).map(|_| ()))?;
        }
        MarkChange::Copy { cel: from, .. } => {
            bail!("{cel} copies the marks of {from}; copies are resolved by the bundle")
        }
        MarkChange::Paste { .. } => bail!("{cel} takes copies of other marks; copies are resolved by the bundle"),
        MarkChange::Detach { ids } => {
            ids.iter().try_for_each(|id| position(marks, cel, *id).map(|_| ()))?;
            attach::detach(marks, ids);
        }
        MarkChange::Entry { entry, color, changed } => {
            *changed = marks
                .iter_mut()
                .filter_map(|m| match m {
                    Mark::Fill(f) if f.entry() == Some(entry.as_str()) => Some(f),
                    _ => None,
                })
                .map(|f| {
                    match color {
                        Some(c) => f.color = *c,
                        None => {
                            if let FillSource::Cells { entry, .. } = &mut f.source {
                                entry.clear();
                            }
                        }
                    }
                    f.id
                })
                .collect();
        }
    }
    Ok(alone)
}

pub fn paste(
    marks: &mut Vec<Mark>,
    copies: Vec<(String, Mark)>,
    transform: Option<&MarkTransform>,
    cut: Option<&[[f32; 2]]>,
    ids: MarkIds,
) -> Result<Vec<Copied>> {
    let mut next = ids.next(marks);
    let (made, mut copied): (Vec<Copied>, Vec<Mark>) = copies
        .into_iter()
        .map(|(cel, mut mark)| {
            let from = mark.id();
            mark.set_id(next);
            next += 1;
            if let Some(polygon) = cut {
                mark.cuts_mut().push(Cut {
                    keep: Keep::Inside,
                    polygon: polygon.to_vec(),
                });
            }
            (
                Copied {
                    cel,
                    from,
                    to: mark.id(),
                },
                mark,
            )
        })
        .unzip();
    attach::remap(&mut copied, &made);
    if let Some(t) = transform {
        let pivot = match t.pivot {
            Some(p) => p,
            None => {
                let [x0, y0, x1, y1] =
                    union(&copied.iter().collect::<Vec<_>>()).ok_or_else(|| anyhow!("the copies cover nothing"))?;
                [(x0 + x1) / 2.0, (y0 + y1) / 2.0]
            }
        };
        let a = t.affine(pivot);
        copied.iter_mut().for_each(|m| transform_mark(m, a));
    }
    marks.extend(copied);
    Ok(made)
}

fn pivot_of(marks: &[Mark], ids: &[u32], transform: &MarkTransform, cel: &str) -> Result<[f32; 2]> {
    let at: Vec<usize> = ids.iter().map(|id| position(marks, cel, *id)).collect::<Result<_>>()?;
    Ok(match transform.pivot {
        Some(p) => p,
        None => {
            let chosen: Vec<&Mark> = at.iter().map(|&i| &marks[i]).collect();
            let [x0, y0, x1, y1] = union(&chosen).ok_or_else(|| anyhow!("nothing to transform"))?;
            [(x0 + x1) / 2.0, (y0 + y1) / 2.0]
        }
    })
}

fn transform_marks(marks: &mut [Mark], ids: &[u32], a: Affine, cel: &str) -> Result<()> {
    let at: Vec<usize> = ids.iter().map(|id| position(marks, cel, *id)).collect::<Result<_>>()?;
    at.iter().for_each(|&i| transform_mark(&mut marks[i], a));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::{FillRef, Seed};
    use serde_json::json;

    fn line(id: u32) -> Mark {
        Stroke::new(id, "ink", [0.0, 0.0, 0.0, 1.0], 10.0, 1)
            .with_points(&[(10.0, 10.0, 1.0), (30.0, 10.0, 1.0)], 4.0)
            .tapered([4.0, 8.0])
            .into()
    }

    fn square(id: u32) -> Mark {
        let mut mask = vec![0u8; 64 * 64];
        (20..30).for_each(|y| (20..40).for_each(|x| mask[y * 64 + x] = 255));
        let source = FillSource::Seed {
            seed: Seed {
                x: 25.0,
                y: 25.0,
                tolerance: 0.1,
                gap: 0.0,
                expand: 2.0,
            },
            reference: FillRef::This,
        };
        Fill::new(id, [1.0, 0.0, 0.0, 1.0], source, Coverage::from_mask(64, 64, &mask)).into()
    }

    fn props(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_pick_takes_the_mark_under_the_point_before_one_merely_near_it() {
        let wide: Mark = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 20.0, 1)
            .with_points(&[(10.0, 50.0, 1.0), (90.0, 50.0, 1.0)], 4.0)
            .into();
        let thin: Mark = Stroke::new(2, "ink", [0.0, 0.0, 0.0, 1.0], 2.0, 1)
            .with_points(&[(10.0, 46.0, 1.0), (130.0, 46.0, 1.0)], 4.0)
            .into();
        let marks = vec![wide, thin];
        assert_eq!(
            pick(&marks, (50.0, 52.0), 6.0, |_| false),
            Some(1),
            "under the point wins"
        );
        assert_eq!(
            pick(&marks, (50.0, 46.5), 6.0, |_| false),
            Some(2),
            "the top one where both cover"
        );
        assert_eq!(
            pick(&marks, (120.0, 49.0), 6.0, |_| false),
            Some(2),
            "near only the thin one: slack"
        );
        assert_eq!(
            pick(&marks, (50.0, 46.5), 6.0, |m| m.id() == 2),
            Some(1),
            "hidden marks are skipped"
        );
        assert_eq!(pick(&marks, (50.0, 20.0), 6.0, |_| false), None);
    }

    #[test]
    fn addresses_name_a_cel_and_a_mark() {
        assert_eq!(parse_address("c4:s12"), Some(("c4", 12)));
        assert_eq!(parse_address("c4:12"), None);
        assert_eq!(parse_address("n5"), None);
        assert_eq!(parse_address("c 4:s1"), None);
        assert_eq!(address("c4", 12), "c4:s12");
    }

    #[test]
    fn set_changes_strokes_and_fills_and_rejects_typos() {
        let mut marks = vec![line(1), square(2)];
        let mut set = MarkChange::Set {
            id: 1,
            props: props(json!({"color":"#ff0000","size":4,"brush":"pencil","taper":null})),
        };
        apply(&mut marks, &mut set, "c1", MarkIds::default()).unwrap();
        let s = marks[0].as_stroke().unwrap();
        assert_eq!(
            (s.color, s.size, s.brush.as_str(), s.taper),
            ([65535, 0, 0, 65535], 4.0, "pencil", [0.0, 0.0])
        );
        let mut fill = MarkChange::Set {
            id: 2,
            props: props(json!({"opacity":0.5})),
        };
        apply(&mut marks, &mut fill, "c1", MarkIds::default()).unwrap();
        assert_eq!(marks[1].as_fill().unwrap().opacity, 0.5);
        let mut typo = MarkChange::Set {
            id: 2,
            props: props(json!({"size":3})),
        };
        let err = apply(&mut marks, &mut typo, "c1", MarkIds::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("c1:s2") && err.contains("color, opacity"), "{err}");
        let mut missing = MarkChange::Remove { id: 9 };
        assert!(
            apply(&mut marks, &mut missing, "c1", MarkIds::default())
                .unwrap_err()
                .to_string()
                .contains("c1:s9")
        );
    }

    #[test]
    fn transforms_move_rotate_and_scale_about_the_middle() {
        let mut marks = vec![line(1)];
        let mut moved = MarkChange::Transform {
            ids: vec![1],
            transform: MarkTransform {
                x: 5.0,
                y: -2.5,
                ..Default::default()
            },
        };
        apply(&mut marks, &mut moved, "c1", MarkIds::default()).unwrap();
        assert_eq!(marks[0].as_stroke().unwrap().samples[0].pos(), (15.0, 7.5));
        let mut turned = MarkChange::Transform {
            ids: vec![1],
            transform: MarkTransform {
                rotate: 90.0,
                sx: 2.0,
                sy: 2.0,
                ..Default::default()
            },
        };
        apply(&mut marks, &mut turned, "c1", MarkIds::default()).unwrap();
        let s = marks[0].as_stroke().unwrap();
        let (a, b) = (s.samples[0].pos(), s.samples[1].pos());
        assert!((a.0 - 25.0).abs() < 0.07 && (a.1 + 12.5).abs() < 0.07, "{a:?}");
        assert!((b.0 - 25.0).abs() < 0.07 && (b.1 - 27.5).abs() < 0.07, "{b:?}");
        assert_eq!((s.size, s.taper), (20.0, [8.0, 16.0]));
    }

    #[test]
    fn fills_move_by_whole_pixels_and_resample_otherwise() {
        let mut marks = vec![square(1)];
        let mut moved = MarkChange::Transform {
            ids: vec![1],
            transform: MarkTransform {
                x: -25.2,
                y: 3.0,
                ..Default::default()
            },
        };
        apply(&mut marks, &mut moved, "c1", MarkIds::default()).unwrap();
        let f = marks[0].as_fill().unwrap();
        assert_eq!(
            (f.coverage.x, f.coverage.y, f.coverage.width, f.coverage.height),
            (0, 23, 15, 10)
        );
        assert_eq!(f.coverage.pixels(), 150);
        let mut marks = vec![square(1)];
        let mut doubled = MarkChange::Transform {
            ids: vec![1],
            transform: MarkTransform {
                sx: 2.0,
                sy: 2.0,
                pivot: Some([20.0, 20.0]),
                ..Default::default()
            },
        };
        apply(&mut marks, &mut doubled, "c1", MarkIds::default()).unwrap();
        let f = marks[0].as_fill().unwrap();
        assert_eq!((f.coverage.x, f.coverage.y), (20, 20));
        assert_eq!((f.coverage.width, f.coverage.height), (40, 20));
        assert!(f.coverage.pixels() >= 780);
        assert!(matches!(f.source, FillSource::Seed { seed, .. } if seed.x == 30.0));
    }

    #[test]
    fn pixel_marks_stay_hard_when_scaled_flipped_or_turned() {
        let corner = || -> Mark {
            Fill::cells(
                1,
                [1.0, 0.0, 0.0, 1.0],
                "p1",
                false,
                Coverage::from_cells(&[[4, 4], [5, 4], [6, 4], [4, 5]]),
            )
            .into()
        };
        let changed = |transform: MarkTransform| -> Coverage {
            let mut marks = vec![corner()];
            let mut change = MarkChange::Transform {
                ids: vec![1],
                transform,
            };
            apply(&mut marks, &mut change, "c1", MarkIds::default()).unwrap();
            let fill = marks[0].as_fill().unwrap();
            assert_eq!(fill.entry(), Some("p1"));
            assert!(fill.coverage.alpha.iter().all(|a| *a == 0 || *a == 255));
            fill.coverage.clone()
        };
        let big = changed(MarkTransform {
            sx: 3.0,
            sy: 3.0,
            pivot: Some([0.0, 0.0]),
            ..Default::default()
        });
        assert_eq!((big.x, big.y, big.width, big.height), (12, 12, 9, 6));
        assert_eq!(big.pixels(), 4 * 9, "every pixel becomes a 3x3 block");
        assert_eq!(
            (big.at(12, 12), big.at(20, 14), big.at(15, 15), big.at(14, 17)),
            (255, 255, 0, 255)
        );
        let flipped = changed(MarkTransform {
            sx: -1.0,
            pivot: Some([5.5, 5.0]),
            ..Default::default()
        });
        assert_eq!(flipped.cells().collect::<Vec<_>>(), [[4, 4], [5, 4], [6, 4], [6, 5]]);
        let turned = changed(MarkTransform {
            rotate: 90.0,
            pivot: Some([5.0, 5.0]),
            ..Default::default()
        });
        assert_eq!(turned.cells().collect::<Vec<_>>(), [[4, 4], [5, 4], [5, 5], [5, 6]]);
        assert_eq!(turned.pixels(), 4, "a quarter turn keeps every pixel");
    }

    #[test]
    fn clicks_pick_the_topmost_mark_and_lassos_take_whole_marks() {
        let mut marks = vec![square(1), line(2)];
        assert_eq!(hit(&marks, (20.0, 13.0), 0.0), Some(2));
        assert_eq!(hit(&marks, (25.0, 25.0), 0.0), Some(1));
        assert_eq!(hit(&marks, (20.0, 17.0), 0.0), None);
        assert_eq!(hit(&marks, (20.0, 17.0), 3.0), Some(2));
        let around_line = [[5.0, 5.0], [35.0, 5.0], [35.0, 15.0], [5.0, 15.0]];
        assert_eq!(lassoed(&marks, &around_line), vec![2]);
        let around_all = [[0.0, 0.0], [50.0, 0.0], [50.0, 50.0], [0.0, 50.0]];
        assert_eq!(lassoed(&marks, &around_all), vec![1, 2]);
        marks.swap(0, 1);
        assert_eq!(hit(&marks, (20.0, 25.0), 0.0), Some(1));
        assert_eq!(bounds_of(&marks, &[2]), Some([5.0, 5.0, 35.0, 15.0]));
    }

    fn arm(id: u32) -> Mark {
        Stroke::new(id, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1)
            .with_points(&[(0.0, 30.0, 1.0), (30.0, 30.0, 1.0), (60.0, 30.0, 1.0)], 4.0)
            .into()
    }

    #[test]
    fn a_cut_lasso_splits_crossing_marks_and_moves_the_inside() {
        let hand = [[20.0, 0.0], [70.0, 0.0], [70.0, 60.0], [20.0, 60.0]];
        let marks = vec![arm(1), line(2), square(3)];
        assert_eq!(
            sides(&marks, &hand),
            vec![(1, Side::Crossing), (2, Side::Crossing), (3, Side::Inside)]
        );
        let mut marks = vec![arm(1), square(2)];
        let polygon = vec![[15.0, 15.0], [70.0, 15.0], [70.0, 45.0], [15.0, 45.0]];
        assert_eq!(sides(&marks, &polygon), vec![(1, Side::Crossing), (2, Side::Inside)]);
        let mut whole = MarkChange::Lasso {
            polygon: polygon.clone(),
            cut: false,
            action: LassoAction::Transform(MarkTransform {
                x: 100.0,
                ..Default::default()
            }),
            chosen: Vec::new(),
        };
        let mut plain = marks.clone();
        apply(&mut plain, &mut whole, "c1", MarkIds::default()).unwrap();
        assert_eq!(whole.ids(), vec![2]);
        assert_eq!(plain[0], marks[0]);
        let mut change = MarkChange::Lasso {
            polygon: polygon.clone(),
            cut: true,
            action: LassoAction::Transform(MarkTransform {
                x: 100.0,
                ..Default::default()
            }),
            chosen: Vec::new(),
        };
        apply(&mut marks, &mut change, "c1", MarkIds::default()).unwrap();
        assert_eq!(marks.iter().map(Mark::id).collect::<Vec<_>>(), [1, 3, 2]);
        assert_eq!(change.ids(), vec![2, 3]);
        let (stay, part) = (marks[0].as_stroke().unwrap(), marks[1].as_stroke().unwrap());
        assert_eq!(stay.samples, arm(1).as_stroke().unwrap().samples);
        assert_eq!(stay.cuts[0].keep, Keep::Outside);
        assert_eq!(part.cuts[0].keep, Keep::Inside);
        assert_eq!(part.samples[0].pos(), (100.0, 30.0));
        assert_eq!(part.cuts[0].polygon[0], [115.0, 15.0]);
        assert!(!cut::keeps(&stay.cuts, (30.0, 30.0)) && cut::keeps(&part.cuts, (130.0, 30.0)));
        assert_eq!(hit(&marks, (30.0, 30.0), 0.0), None);
        assert_eq!(hit(&marks, (5.0, 30.0), 0.0), Some(1));
        assert_eq!(hit(&marks, (130.0, 30.0), 0.0), Some(3));
        let mut gone = vec![arm(1), square(2)];
        let mut rm = MarkChange::Lasso {
            polygon,
            cut: true,
            action: LassoAction::Remove,
            chosen: Vec::new(),
        };
        apply(&mut gone, &mut rm, "c1", MarkIds::default()).unwrap();
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].cuts()[0].keep, Keep::Outside);
        let mut empty = MarkChange::Lasso {
            polygon: vec![[500.0, 500.0], [510.0, 500.0], [510.0, 510.0]],
            cut: true,
            action: LassoAction::Remove,
            chosen: Vec::new(),
        };
        assert!(apply(&mut gone, &mut empty, "c1", MarkIds::default()).is_err());
    }

    #[test]
    fn flips_mirror_strokes_and_cuts() {
        let mut marks = vec![arm(1)];
        marks[0].cuts_mut().push(Cut {
            keep: Keep::Inside,
            polygon: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 40.0], [0.0, 40.0]],
        });
        let mut flip = MarkChange::Transform {
            ids: vec![1],
            transform: MarkTransform {
                sx: -1.0,
                pivot: Some([30.0, 30.0]),
                ..Default::default()
            },
        };
        apply(&mut marks, &mut flip, "c1", MarkIds::default()).unwrap();
        let s = marks[0].as_stroke().unwrap();
        assert_eq!(s.samples[0].pos(), (60.0, 30.0));
        assert_eq!(s.size, 4.0);
        assert_eq!(s.cuts[0].polygon[1], [50.0, 0.0]);
    }

    #[test]
    fn adds_take_the_next_ids() {
        let mut marks = vec![line(4)];
        let mut add = MarkChange::Add(vec![line(0), square(0)]);
        apply(&mut marks, &mut add, "c1", MarkIds::default()).unwrap();
        assert_eq!(marks.iter().map(Mark::id).collect::<Vec<_>>(), [4, 5, 6]);
        assert_eq!(add.ids(), [5, 6]);
    }

    #[test]
    fn a_reshaped_stroke_keeps_its_pressure_along_the_new_path() {
        let points: Vec<(f32, f32, f32)> = (0..=20)
            .map(|i| (i as f32 * 5.0, 10.0, 0.2 + i as f32 * 0.04))
            .collect();
        let stroke = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 8.0, 3).with_points(&points, 5.0);
        let mut bent = stroke.clone();
        reshape(&mut bent, "M10 0 L10 200").unwrap();
        assert_eq!(bent.samples.len(), stroke.samples.len());
        bent.samples
            .iter()
            .zip(&stroke.samples)
            .enumerate()
            .for_each(|(i, (b, s))| {
                let (x, y) = b.pos();
                assert!(
                    (x - 10.0).abs() < 0.1 && (y - i as f32 * 10.0).abs() < 0.1,
                    "{i}: {x},{y}"
                );
                assert_eq!((b.pressure, b.tilt_x, b.dt_ms), (s.pressure, s.tilt_x, s.dt_ms));
            });
        let mut same = stroke.clone();
        reshape(&mut same, "M0 10 L100 10").unwrap();
        assert_eq!(same, stroke);
        let mut marks = vec![Mark::Stroke(stroke)];
        let mut change = MarkChange::Set {
            id: 1,
            props: serde_json::from_str(r#"{"path":"M0 0 C 30 -40 70 40 100 0"}"#).unwrap(),
        };
        apply(&mut marks, &mut change, "c1", MarkIds::default()).unwrap();
        let Mark::Stroke(curved) = &marks[0] else { panic!() };
        assert!(curved.samples[5].pos().1 < -5.0, "{:?}", curved.samples[5].pos());
        assert!(reshape(&mut curved.clone(), "").is_err());
    }
}
