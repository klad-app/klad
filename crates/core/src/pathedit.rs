use crate::doc::Shape;
use crate::path::{Token, resample, tokens};
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value};
use std::f32::consts::PI;

pub type P = [f32; 2];

const KAPPA: f32 = 0.552_284_8;
const NEAR: f32 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub at: P,
    pub inn: P,
    pub out: P,
}

impl Anchor {
    pub fn corner(at: P) -> Self {
        Self { at, inn: at, out: at }
    }

    pub fn has_in(&self) -> bool {
        dist(self.inn, self.at) > NEAR
    }

    pub fn has_out(&self) -> bool {
        dist(self.out, self.at) > NEAR
    }

    pub fn smooth(&self) -> bool {
        if !self.has_in() || !self.has_out() {
            return false;
        }
        let (a, b) = (sub(self.inn, self.at), sub(self.out, self.at));
        let cross = a[0] * b[1] - a[1] * b[0];
        let dot = a[0] * b[0] + a[1] * b[1];
        dot < 0.0 && cross.abs() <= 0.02 * len(a) * len(b)
    }

    fn shift(&mut self, by: P) {
        self.at = add(self.at, by);
        self.inn = add(self.inn, by);
        self.out = add(self.out, by);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: P, k: f32) -> P {
    [a[0] * k, a[1] * k]
}

fn len(a: P) -> f32 {
    libm::sqrtf(a[0] * a[0] + a[1] * a[1])
}

fn dist(a: P, b: P) -> f32 {
    len(sub(a, b))
}

fn unit(a: P) -> P {
    let l = len(a);
    if l < 1e-9 { [0.0, 0.0] } else { mul(a, 1.0 / l) }
}

fn lerp(a: P, b: P, t: f32) -> P {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn cubic_at(p: [P; 4], t: f32) -> P {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [
        a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0],
        a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1],
    ]
}

pub fn point_on(p: [P; 4], t: f32) -> P {
    if dist(p[1], p[0]) <= NEAR && dist(p[2], p[3]) <= NEAR {
        lerp(p[0], p[3], t)
    } else {
        cubic_at(p, t)
    }
}

struct Builder {
    subs: Vec<Subpath>,
    current: Option<Subpath>,
}

impl Builder {
    fn start(&mut self, at: P) {
        self.finish();
        self.current = Some(Subpath {
            anchors: vec![Anchor::corner(at)],
            closed: false,
        });
    }

    fn last(&mut self) -> Result<&mut Subpath> {
        self.current.as_mut().ok_or_else(|| anyhow!("path must start with M"))
    }

    fn line(&mut self, to: P) -> Result<()> {
        self.last()?.anchors.push(Anchor::corner(to));
        Ok(())
    }

    fn cubic(&mut self, c1: P, c2: P, to: P) -> Result<()> {
        let sub = self.last()?;
        if let Some(prev) = sub.anchors.last_mut() {
            prev.out = c1;
        }
        sub.anchors.push(Anchor {
            at: to,
            inn: c2,
            out: to,
        });
        Ok(())
    }

    fn close(&mut self) {
        if let Some(mut path) = self.current.take() {
            let n = path.anchors.len();
            if n > 1 && dist(path.anchors[n - 1].at, path.anchors[0].at) <= NEAR {
                let last = path.anchors.pop().expect("two anchors");
                path.anchors[0].inn = add(path.anchors[0].at, sub(last.inn, last.at));
            }
            path.closed = true;
            self.subs.push(path);
        }
    }

    fn finish(&mut self) {
        if let Some(sub) = self.current.take() {
            self.subs.push(sub);
        }
    }
}

fn angle(u: P, v: P) -> f32 {
    libm::atan2f(u[0] * v[1] - u[1] * v[0], u[0] * v[0] + u[1] * v[1])
}

#[allow(clippy::too_many_arguments)]
fn arc(b: &mut Builder, from: P, r: P, rotation: f32, large: bool, sweep: bool, to: P) -> Result<()> {
    let (mut rx, mut ry) = (r[0].abs(), r[1].abs());
    if dist(from, to) < 1e-6 {
        return Ok(());
    }
    if rx < 1e-6 || ry < 1e-6 {
        return b.line(to);
    }
    let phi = rotation.to_radians();
    let (s, c) = (libm::sinf(phi), libm::cosf(phi));
    let (dx, dy) = ((from[0] - to[0]) / 2.0, (from[1] - to[1]) / 2.0);
    let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lambda > 1.0 {
        rx *= libm::sqrtf(lambda);
        ry *= libm::sqrtf(lambda);
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let coef = sign * libm::sqrtf((num / den).max(0.0));
    let (cxp, cyp) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
    let (cx, cy) = (
        c * cxp - s * cyp + (from[0] + to[0]) / 2.0,
        s * cxp + c * cyp + (from[1] + to[1]) / 2.0,
    );
    let start = [(x1 - cxp) / rx, (y1 - cyp) / ry];
    let end = [(-x1 - cxp) / rx, (-y1 - cyp) / ry];
    let theta = angle([1.0, 0.0], start);
    let mut delta = angle(start, end);
    if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    }
    let pieces = (delta.abs() / (PI / 2.0)).ceil().max(1.0) as usize;
    let step = delta / pieces as f32;
    let k = 4.0 / 3.0 * libm::tanf(step / 4.0);
    let point = |t: f32| {
        let (st, ct) = (libm::sinf(t), libm::cosf(t));
        [cx + rx * ct * c - ry * st * s, cy + rx * ct * s + ry * st * c]
    };
    let slope = |t: f32| {
        let (st, ct) = (libm::sinf(t), libm::cosf(t));
        [-rx * st * c - ry * ct * s, -rx * st * s + ry * ct * c]
    };
    (0..pieces).try_for_each(|i| {
        let (t1, t2) = (theta + step * i as f32, theta + step * (i + 1) as f32);
        let end = if i + 1 == pieces { to } else { point(t2) };
        b.cubic(add(point(t1), mul(slope(t1), k)), sub(end, mul(slope(t2), k)), end)
    })
}

pub fn parse(d: &str) -> Result<Vec<Subpath>> {
    let tokens = tokens(d)?;
    let mut b = Builder {
        subs: Vec::new(),
        current: None,
    };
    let (mut pos, mut start): (P, P) = ([0.0, 0.0], [0.0, 0.0]);
    let (mut last_cubic, mut last_quad): (Option<P>, Option<P>) = (None, None);
    let mut command = ' ';
    let mut i = 0;
    let take = |i: &mut usize, n: usize| -> Result<Vec<f32>> {
        let values: Vec<f32> = tokens
            .get(*i..*i + n)
            .unwrap_or(&[])
            .iter()
            .map_while(|t| match t {
                Token::Number(v) => Some(*v),
                Token::Command(_) => None,
            })
            .collect();
        if values.len() != n {
            bail!("a path command needs {n} numbers there");
        }
        *i += n;
        Ok(values)
    };
    while i < tokens.len() {
        if let Token::Command(c) = tokens[i] {
            command = c;
            i += 1;
            if matches!(c, 'Z' | 'z') {
                b.close();
                pos = start;
                last_cubic = None;
                last_quad = None;
                continue;
            }
        }
        if command == ' ' {
            bail!("path must start with a command");
        }
        let upper = command.to_ascii_uppercase();
        if upper != 'M' && b.current.is_none() {
            b.start(pos);
        }
        let rel = command.is_ascii_lowercase();
        let at = |p: P, x: f32, y: f32| if rel { [p[0] + x, p[1] + y] } else { [x, y] };
        let (mut cubic_control, mut quad_control) = (None, None);
        match upper {
            'M' => {
                let v = take(&mut i, 2)?;
                pos = at(pos, v[0], v[1]);
                start = pos;
                b.start(pos);
                command = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let v = take(&mut i, 2)?;
                pos = at(pos, v[0], v[1]);
                b.line(pos)?;
            }
            'H' => {
                let v = take(&mut i, 1)?;
                pos = [if rel { pos[0] + v[0] } else { v[0] }, pos[1]];
                b.line(pos)?;
            }
            'V' => {
                let v = take(&mut i, 1)?;
                pos = [pos[0], if rel { pos[1] + v[0] } else { v[0] }];
                b.line(pos)?;
            }
            'C' | 'S' => {
                let (c1, rest) = if upper == 'C' {
                    let v = take(&mut i, 6)?;
                    (at(pos, v[0], v[1]), v[2..].to_vec())
                } else {
                    let v = take(&mut i, 4)?;
                    (last_cubic.map_or(pos, |c| sub(mul(pos, 2.0), c)), v)
                };
                let (c2, end) = (at(pos, rest[0], rest[1]), at(pos, rest[2], rest[3]));
                b.cubic(c1, c2, end)?;
                cubic_control = Some(c2);
                pos = end;
            }
            'Q' | 'T' => {
                let (q, end) = if upper == 'Q' {
                    let v = take(&mut i, 4)?;
                    (at(pos, v[0], v[1]), at(pos, v[2], v[3]))
                } else {
                    let v = take(&mut i, 2)?;
                    (last_quad.map_or(pos, |c| sub(mul(pos, 2.0), c)), at(pos, v[0], v[1]))
                };
                b.cubic(lerp(pos, q, 2.0 / 3.0), lerp(end, q, 2.0 / 3.0), end)?;
                quad_control = Some(q);
                pos = end;
            }
            'A' => {
                let v = take(&mut i, 7)?;
                let end = at(pos, v[5], v[6]);
                arc(&mut b, pos, [v[0], v[1]], v[2], v[3] != 0.0, v[4] != 0.0, end)?;
                pos = end;
            }
            other => bail!("unknown path command {other:?}"),
        }
        last_cubic = cubic_control;
        last_quad = quad_control;
    }
    b.finish();
    if b.subs.iter().all(|s| s.anchors.len() < 2) {
        bail!("the path has no segments");
    }
    Ok(b.subs)
}

fn num(v: f32) -> String {
    let text = format!("{:.2}", v);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" | "" => "0".into(),
        t => t.into(),
    }
}

fn segment(out: &mut String, a: &Anchor, b: &Anchor) {
    if !a.has_out() && !b.has_in() {
        out.push_str(&format!(" L{} {}", num(b.at[0]), num(b.at[1])));
    } else {
        out.push_str(&format!(
            " C{} {} {} {} {} {}",
            num(a.out[0]),
            num(a.out[1]),
            num(b.inn[0]),
            num(b.inn[1]),
            num(b.at[0]),
            num(b.at[1])
        ));
    }
}

pub fn to_d(subs: &[Subpath]) -> String {
    let mut out = String::new();
    subs.iter().filter(|s| !s.anchors.is_empty()).for_each(|s| {
        let first = &s.anchors[0];
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("M{} {}", num(first.at[0]), num(first.at[1])));
        s.anchors.windows(2).for_each(|w| segment(&mut out, &w[0], &w[1]));
        if s.closed {
            let last = &s.anchors[s.anchors.len() - 1];
            if last.has_out() || first.has_in() {
                segment(&mut out, last, first);
            }
            out.push_str(" Z");
        }
    });
    out
}

fn rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Subpath {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.0 {
        return Subpath {
            anchors: [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
                .into_iter()
                .map(Anchor::corner)
                .collect(),
            closed: true,
        };
    }
    let k = r * KAPPA;
    let round = |at: P, inn: P, out: P| Anchor {
        at,
        inn: add(at, inn),
        out: add(at, out),
    };
    let (x1, y1) = (x + w, y + h);
    Subpath {
        anchors: vec![
            round([x + r, y], [-k, 0.0], [0.0, 0.0]),
            round([x1 - r, y], [0.0, 0.0], [k, 0.0]),
            round([x1, y + r], [0.0, -k], [0.0, 0.0]),
            round([x1, y1 - r], [0.0, 0.0], [0.0, k]),
            round([x1 - r, y1], [k, 0.0], [0.0, 0.0]),
            round([x + r, y1], [0.0, 0.0], [-k, 0.0]),
            round([x, y1 - r], [0.0, k], [0.0, 0.0]),
            round([x, y + r], [0.0, 0.0], [0.0, -k]),
        ],
        closed: true,
    }
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Subpath {
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let anchor = |at: P, inn: P| Anchor {
        at,
        inn: add(at, inn),
        out: sub(at, inn),
    };
    Subpath {
        anchors: vec![
            anchor([cx + rx, cy], [0.0, -ky]),
            anchor([cx, cy + ry], [kx, 0.0]),
            anchor([cx - rx, cy], [0.0, ky]),
            anchor([cx, cy - ry], [-kx, 0.0]),
        ],
        closed: true,
    }
}

pub fn from_shape(shape: &Shape) -> Result<Vec<Subpath>> {
    Ok(match shape {
        Shape::Path { d } => parse(d)?,
        Shape::Rect { x, y, w, h, radius } => vec![rect(*x, *y, *w, *h, *radius)],
        Shape::Ellipse { cx, cy, rx, ry } => vec![ellipse(*cx, *cy, *rx, *ry)],
        Shape::Group { .. } => bail!("groups have no points; edit the paths inside"),
        Shape::Rays(_) => bail!("rays have no points; set their origin, angle, spread and length"),
        Shape::Image(_) => bail!("a picture has no points; V moves and scales it"),
        Shape::Text(_) => bail!("text has no points; change it in the inspector"),
        Shape::Scatter(_) | Shape::Repeat(_) | Shape::Mirror(_) => {
            bail!("a generator has no points; edit the shapes inside it")
        }
    })
}

pub fn locate(subs: &[Subpath], index: usize) -> Result<(usize, usize)> {
    let mut left = index;
    subs.iter()
        .enumerate()
        .find_map(|(s, sub)| {
            if left < sub.anchors.len() {
                Some((s, left))
            } else {
                left -= sub.anchors.len();
                None
            }
        })
        .ok_or_else(|| {
            let total: usize = subs.iter().map(|s| s.anchors.len()).sum();
            anyhow!(
                "point {index} does not exist; the path has points 0 to {}",
                total.saturating_sub(1)
            )
        })
}

pub fn anchors(subs: &[Subpath]) -> Vec<(usize, Anchor)> {
    subs.iter()
        .enumerate()
        .flat_map(|(s, sub)| sub.anchors.iter().map(move |a| (s, *a)))
        .collect()
}

fn neighbours(sub: &Subpath, i: usize) -> (Option<usize>, Option<usize>) {
    let n = sub.anchors.len();
    let prev = if i > 0 {
        Some(i - 1)
    } else {
        (sub.closed && n > 2).then_some(n - 1)
    };
    let next = if i + 1 < n {
        Some(i + 1)
    } else {
        (sub.closed && n > 2).then_some(0)
    };
    (prev, next)
}

pub fn segment_points(sub: &Subpath, i: usize) -> Option<[P; 4]> {
    let (_, next) = neighbours(sub, i);
    let (a, b) = (sub.anchors[i], sub.anchors[next?]);
    Some([a.at, a.out, b.inn, b.at])
}

pub fn flatten(subs: &[Subpath], step: f32) -> Vec<Vec<P>> {
    subs.iter()
        .map(|sub| {
            let count = if sub.closed {
                sub.anchors.len()
            } else {
                sub.anchors.len().saturating_sub(1)
            };
            let mut points = vec![sub.anchors[0].at];
            (0..count).filter_map(|i| segment_points(sub, i)).for_each(|p| {
                let hull = dist(p[0], p[1]) + dist(p[1], p[2]) + dist(p[2], p[3]);
                let n = (hull / step.max(0.1)).ceil().clamp(1.0, 400.0) as usize;
                points.extend((1..=n).map(|k| cubic_at(p, k as f32 / n as f32)));
            });
            points
        })
        .collect()
}

pub fn nearest(subs: &[Subpath], to: P) -> Option<(usize, f32, f32)> {
    let mut base = 0;
    subs.iter()
        .flat_map(|sub| {
            let first = base;
            base += sub.anchors.len();
            (0..sub.anchors.len()).filter_map(move |i| segment_points(sub, i).map(|p| (first + i, p)))
        })
        .map(|(index, p)| {
            let hull = dist(p[0], p[1]) + dist(p[1], p[2]) + dist(p[2], p[3]);
            let n = (hull / 2.0).ceil().clamp(8.0, 400.0) as usize;
            let (t, d) = (0..=n)
                .map(|k| {
                    let t = k as f32 / n as f32;
                    (t, dist(point_on(p, t), to))
                })
                .fold((0.0, f32::MAX), |best, c| if c.1 < best.1 { c } else { best });
            (index, t, d)
        })
        .fold(None, |best: Option<(usize, f32, f32)>, c| match best {
            Some(b) if b.2 <= c.2 => Some(b),
            _ => Some(c),
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    In,
    Out,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathEdit {
    Convert,
    Move {
        points: Vec<usize>,
        by: P,
    },
    MoveTo {
        point: usize,
        to: P,
    },
    Handle {
        point: usize,
        side: Side,
        to: P,
        free: bool,
    },
    Insert {
        segment: usize,
        t: f32,
    },
    Add {
        at: P,
    },
    Remove {
        points: Vec<usize>,
    },
    Corner {
        points: Vec<usize>,
    },
    Smooth {
        points: Vec<usize>,
    },
    Simplify {
        tolerance: f32,
    },
}

fn indices(value: Value, key: &str) -> Result<Vec<usize>> {
    match value {
        Value::Number(n) => Ok(vec![
            n.as_u64().ok_or_else(|| anyhow!("{key} takes point numbers"))? as usize
        ]),
        other => serde_json::from_value(other).map_err(|_| anyhow!("{key} takes a point number or a list of them")),
    }
}

fn point(map: &mut Map<String, Value>, key: &str) -> Result<Option<P>> {
    map.remove(key)
        .map(|v| serde_json::from_value(v).map_err(|_| anyhow!("{key} takes [x, y]")))
        .transpose()
}

impl PathEdit {
    pub fn parse(map: &mut Map<String, Value>) -> Result<Self> {
        if let Some(v) = map.remove("move") {
            let points = indices(v, "move")?;
            return match (point(map, "to")?, point(map, "by")?, &points[..]) {
                (Some(to), None, [one]) => Ok(PathEdit::MoveTo { point: *one, to }),
                (None, Some(by), _) => Ok(PathEdit::Move { points, by }),
                (Some(_), None, _) => bail!("move with \"to\" takes one point; use \"by\": [dx, dy] for several"),
                _ => bail!("move needs \"to\": [x, y] or \"by\": [dx, dy]"),
            };
        }
        if let Some(v) = map.remove("handle") {
            let point_index = indices(v, "handle")?;
            let [one] = point_index[..] else {
                bail!("handle takes one point");
            };
            let free = map.remove("free").and_then(|v| v.as_bool()).unwrap_or(false);
            return match (point(map, "in")?, point(map, "out")?) {
                (Some(to), None) => Ok(PathEdit::Handle {
                    point: one,
                    side: Side::In,
                    to,
                    free,
                }),
                (None, Some(to)) => Ok(PathEdit::Handle {
                    point: one,
                    side: Side::Out,
                    to,
                    free,
                }),
                _ => bail!("handle needs \"in\": [x, y] or \"out\": [x, y]"),
            };
        }
        if let Some(v) = map.remove("insert") {
            let segment = indices(v, "insert")?;
            let [one] = segment[..] else {
                bail!("insert takes one segment (the point it starts at)");
            };
            let t = map.remove("t").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            if !(0.0..=1.0).contains(&t) {
                bail!("t must be within 0..1");
            }
            return Ok(PathEdit::Insert { segment: one, t });
        }
        if let Some(at) = point(map, "add")? {
            return Ok(PathEdit::Add { at });
        }
        if let Some(v) = map.remove("rm") {
            return Ok(PathEdit::Remove {
                points: indices(v, "rm")?,
            });
        }
        if let Some(v) = map.remove("corner") {
            return Ok(PathEdit::Corner {
                points: indices(v, "corner")?,
            });
        }
        if let Some(v) = map.remove("smooth") {
            return Ok(PathEdit::Smooth {
                points: indices(v, "smooth")?,
            });
        }
        if let Some(v) = map.remove("simplify") {
            let tolerance = v.as_f64().ok_or_else(|| anyhow!("simplify takes a tolerance in px"))? as f32;
            if !(0.01..=100.0).contains(&tolerance) {
                bail!("simplify tolerance must be within 0.01..100 px");
            }
            return Ok(PathEdit::Simplify { tolerance });
        }
        Ok(PathEdit::Convert)
    }

    pub fn apply(&self, subs: &mut Vec<Subpath>) -> Result<()> {
        match self {
            PathEdit::Convert => {}
            PathEdit::Move { points, by } => points.iter().try_for_each(|&i| {
                let (s, a) = locate(subs, i)?;
                subs[s].anchors[a].shift(*by);
                Ok::<_, anyhow::Error>(())
            })?,
            PathEdit::MoveTo { point, to } => {
                let (s, a) = locate(subs, *point)?;
                let anchor = &mut subs[s].anchors[a];
                anchor.shift(sub(*to, anchor.at));
            }
            PathEdit::Handle { point, side, to, free } => {
                let (s, a) = locate(subs, *point)?;
                let anchor = &mut subs[s].anchors[a];
                let keep = anchor.smooth() && !free;
                let (moved, other) = match side {
                    Side::In => (&mut anchor.inn, &mut anchor.out),
                    Side::Out => (&mut anchor.out, &mut anchor.inn),
                };
                *moved = *to;
                if keep {
                    let reach = dist(*other, anchor.at);
                    *other = add(anchor.at, mul(unit(sub(anchor.at, *to)), reach));
                }
            }
            PathEdit::Insert { segment, t } => {
                let (s, a) = locate(subs, *segment)?;
                let sub_path = &mut subs[s];
                let p = segment_points(sub_path, a)
                    .ok_or_else(|| anyhow!("point {segment} ends an open path; there is no segment after it"))?;
                let next = (a + 1) % sub_path.anchors.len();
                let straight = !sub_path.anchors[a].has_out() && !sub_path.anchors[next].has_in();
                let fresh = if straight {
                    Anchor::corner(lerp(p[0], p[3], *t))
                } else {
                    let (p01, p12, p23) = (lerp(p[0], p[1], *t), lerp(p[1], p[2], *t), lerp(p[2], p[3], *t));
                    let (p012, p123) = (lerp(p01, p12, *t), lerp(p12, p23, *t));
                    sub_path.anchors[a].out = p01;
                    sub_path.anchors[next].inn = p23;
                    Anchor {
                        at: lerp(p012, p123, *t),
                        inn: p012,
                        out: p123,
                    }
                };
                sub_path.anchors.insert(a + 1, fresh);
            }
            PathEdit::Add { at } => {
                let (segment, t, _) = nearest(subs, *at).ok_or_else(|| anyhow!("the path has no segments"))?;
                PathEdit::Insert { segment, t }.apply(subs)?;
            }
            PathEdit::Remove { points } => {
                let mut found: Vec<(usize, usize)> = points.iter().map(|&i| locate(subs, i)).collect::<Result<_>>()?;
                found.sort();
                found.dedup();
                found.iter().rev().for_each(|&(s, a)| {
                    subs[s].anchors.remove(a);
                });
                subs.retain(|s| s.anchors.len() >= 2);
                if subs.is_empty() {
                    bail!("that leaves no segments; remove the shape instead");
                }
            }
            PathEdit::Corner { points } => points.iter().try_for_each(|&i| {
                let (s, a) = locate(subs, i)?;
                let anchor = &mut subs[s].anchors[a];
                anchor.inn = anchor.at;
                anchor.out = anchor.at;
                Ok::<_, anyhow::Error>(())
            })?,
            PathEdit::Smooth { points } => points.iter().try_for_each(|&i| {
                let (s, a) = locate(subs, i)?;
                let sub_path = &subs[s];
                let (prev, next) = neighbours(sub_path, a);
                let here = sub_path.anchors[a].at;
                let before = prev.map(|p| sub_path.anchors[p].at);
                let after = next.map(|n| sub_path.anchors[n].at);
                let dir = unit(sub(after.unwrap_or(here), before.unwrap_or(here)));
                let anchor = &mut subs[s].anchors[a];
                anchor.inn = before.map_or(here, |b| sub(here, mul(dir, dist(here, b) / 3.0)));
                anchor.out = after.map_or(here, |n| add(here, mul(dir, dist(here, n) / 3.0)));
                Ok::<_, anyhow::Error>(())
            })?,
            PathEdit::Simplify { tolerance } => {
                *subs = subs.iter().map(|s| simplify(s, *tolerance)).collect();
            }
        }
        Ok(())
    }
}

fn turn(sub: &Subpath, i: usize) -> f32 {
    let (prev, next) = neighbours(sub, i);
    let (Some(p), Some(n)) = (prev, next) else {
        return PI;
    };
    let a = &sub.anchors[i];
    let into = if a.has_in() {
        sub_dir(a.at, a.inn)
    } else {
        sub_dir(a.at, sub.anchors[p].at)
    };
    let from = if a.has_out() {
        sub_dir(a.out, a.at)
    } else {
        sub_dir(sub.anchors[n].at, a.at)
    };
    angle(into, from).abs()
}

fn sub_dir(a: P, b: P) -> P {
    unit(sub(a, b))
}

pub fn fit_polyline(points: &[P], tolerance: f32) -> Subpath {
    let spaced = match points.len() {
        0 | 1 => points.to_vec(),
        _ => resample(points, tolerance.max(1.0)),
    };
    let rough = Subpath {
        anchors: spaced.iter().map(|p| Anchor::corner(*p)).collect(),
        closed: false,
    };
    match rough.anchors.len() {
        0..=2 => rough,
        _ => simplify(&rough, tolerance),
    }
}

fn simplify(source: &Subpath, tolerance: f32) -> Subpath {
    let n = source.anchors.len();
    let corners: Vec<usize> = (0..n).filter(|&i| turn(source, i) > 0.9).collect();
    let starts: Vec<usize> = match (source.closed, corners.first()) {
        (true, Some(_)) => corners.clone(),
        (true, None) => vec![0],
        (false, _) => std::iter::once(0)
            .chain(corners.iter().copied().filter(|&c| c > 0 && c + 1 < n))
            .collect(),
    };
    let runs: Vec<Vec<P>> = starts
        .iter()
        .enumerate()
        .map(|(k, &from)| {
            let to = match starts.get(k + 1) {
                Some(&next) => next,
                None if source.closed => starts[0] + n,
                None => n - 1,
            };
            let mut points = vec![source.anchors[from % n].at];
            (from..to).for_each(|i| {
                if let Some(p) = segment_points(source, i % n) {
                    let hull = dist(p[0], p[1]) + dist(p[1], p[2]) + dist(p[2], p[3]);
                    let steps = (hull / 2.0).ceil().clamp(1.0, 400.0) as usize;
                    points.extend((1..=steps).map(|s| cubic_at(p, s as f32 / steps as f32)));
                }
            });
            resample(&points, (tolerance / 2.0).clamp(0.5, 4.0))
        })
        .collect();
    let mut anchors: Vec<Anchor> = Vec::new();
    runs.iter().filter(|r| r.len() >= 2).for_each(|run| {
        let (first, last) = (run[0], run[run.len() - 1]);
        let curves = if straight(run, tolerance / 2.0) {
            vec![[first, first, last, last]]
        } else {
            fit(
                run,
                sub_dir(run[1], first),
                sub_dir(run[run.len() - 2], last),
                tolerance,
            )
        };
        curves.iter().for_each(|c| {
            match anchors.last_mut() {
                Some(last) if dist(last.at, c[0]) <= NEAR => last.out = c[1],
                _ => anchors.push(Anchor {
                    at: c[0],
                    inn: c[0],
                    out: c[1],
                }),
            }
            anchors.push(Anchor {
                at: c[3],
                inn: c[2],
                out: c[3],
            });
        });
    });
    if source.closed && anchors.len() > 1 && dist(anchors[0].at, anchors[anchors.len() - 1].at) <= NEAR {
        let last = anchors.pop().expect("two anchors");
        anchors[0].inn = last.inn;
    }
    let tidy = |p: P, at: P| if dist(p, at) < 0.05 { at } else { p };
    anchors.iter_mut().for_each(|a| {
        a.inn = tidy(a.inn, a.at);
        a.out = tidy(a.out, a.at);
    });
    Subpath {
        anchors,
        closed: source.closed,
    }
}

fn straight(points: &[P], within: f32) -> bool {
    let (a, b) = (points[0], points[points.len() - 1]);
    let along = unit(sub(b, a));
    let span = dist(a, b);
    span > NEAR
        && points.iter().all(|p| {
            let v = sub(*p, a);
            let t = v[0] * along[0] + v[1] * along[1];
            (v[0] * along[1] - v[1] * along[0]).abs() <= within && (-within..=span + within).contains(&t)
        })
}

fn chord(points: &[P]) -> Vec<f32> {
    let mut u = vec![0.0f32];
    points
        .windows(2)
        .for_each(|w| u.push(u[u.len() - 1] + dist(w[0], w[1])));
    let total = u[u.len() - 1].max(1e-9);
    u.iter_mut().for_each(|v| *v /= total);
    u
}

fn generate(points: &[P], u: &[f32], t1: P, t2: P) -> [P; 4] {
    let (first, last) = (points[0], points[points.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    points.iter().zip(u).for_each(|(p, &t)| {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let (a1, a2) = (mul(t1, b1), mul(t2, b2));
        c00 += a1[0] * a1[0] + a1[1] * a1[1];
        c01 += a1[0] * a2[0] + a1[1] * a2[1];
        c11 += a2[0] * a2[0] + a2[1] * a2[1];
        let rest = sub(*p, add(mul(first, b0 + b1), mul(last, b2 + b3)));
        x0 += a1[0] * rest[0] + a1[1] * rest[1];
        x1 += a2[0] * rest[0] + a2[1] * rest[1];
    });
    let det = c00 * c11 - c01 * c01;
    let span = dist(first, last);
    let (mut l, mut r) = if det.abs() > 1e-12 {
        ((x0 * c11 - c01 * x1) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (span / 3.0, span / 3.0)
    };
    if l < span * 1e-6 || r < span * 1e-6 {
        l = span / 3.0;
        r = span / 3.0;
    }
    [first, add(first, mul(t1, l)), add(last, mul(t2, r)), last]
}

fn worst(points: &[P], curve: [P; 4], u: &[f32]) -> (f32, usize) {
    points
        .iter()
        .zip(u)
        .enumerate()
        .map(|(i, (p, &t))| (dist(cubic_at(curve, t), *p), i))
        .fold((0.0, points.len() / 2), |best, c| if c.0 > best.0 { c } else { best })
}

fn reparameterize(points: &[P], u: &[f32], c: [P; 4]) -> Vec<f32> {
    let d1 = [
        mul(sub(c[1], c[0]), 3.0),
        mul(sub(c[2], c[1]), 3.0),
        mul(sub(c[3], c[2]), 3.0),
    ];
    let d2 = [mul(sub(d1[1], d1[0]), 2.0), mul(sub(d1[2], d1[1]), 2.0)];
    points
        .iter()
        .zip(u)
        .map(|(p, &t)| {
            let s = 1.0 - t;
            let q = cubic_at(c, t);
            let q1 = add(add(mul(d1[0], s * s), mul(d1[1], 2.0 * s * t)), mul(d1[2], t * t));
            let q2 = add(mul(d2[0], s), mul(d2[1], t));
            let diff = sub(q, *p);
            let num = diff[0] * q1[0] + diff[1] * q1[1];
            let den = q1[0] * q1[0] + q1[1] * q1[1] + diff[0] * q2[0] + diff[1] * q2[1];
            if den.abs() < 1e-9 {
                t
            } else {
                (t - num / den).clamp(0.0, 1.0)
            }
        })
        .collect()
}

fn fit(points: &[P], t1: P, t2: P, tolerance: f32) -> Vec<[P; 4]> {
    if points.len() == 2 {
        let span = dist(points[0], points[1]) / 3.0;
        return vec![[
            points[0],
            add(points[0], mul(t1, span)),
            add(points[1], mul(t2, span)),
            points[1],
        ]];
    }
    let mut u = chord(points);
    let mut curve = generate(points, &u, t1, t2);
    let (mut error, mut split) = worst(points, curve, &u);
    if error <= tolerance {
        return vec![curve];
    }
    if error <= tolerance * 4.0 {
        for _ in 0..4 {
            u = reparameterize(points, &u, curve);
            curve = generate(points, &u, t1, t2);
            (error, split) = worst(points, curve, &u);
            if error <= tolerance {
                return vec![curve];
            }
        }
    }
    let split = split.clamp(1, points.len() - 2);
    let center = sub_dir(points[split - 1], points[split + 1]);
    let mut left = fit(&points[..=split], t1, center, tolerance);
    left.extend(fit(&points[split..], mul(center, -1.0), t2, tolerance));
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn near(a: P, b: P) -> bool {
        dist(a, b) < 0.02
    }

    fn edit(d: &str, value: Value) -> String {
        let mut subs = parse(d).unwrap();
        let mut map = value.as_object().unwrap().clone();
        PathEdit::parse(&mut map).unwrap().apply(&mut subs).unwrap();
        to_d(&subs)
    }

    #[test]
    fn paths_parse_to_anchors_and_write_back() {
        let d = "M10 10 L20 10 C25 10 30 15 30 20 Q30 30 20 30 Z M100 100 h10 v10";
        let subs = parse(d).unwrap();
        assert_eq!(subs.len(), 2);
        assert!(subs[0].closed && !subs[1].closed);
        assert_eq!(subs[0].anchors.len(), 4);
        assert!(near(subs[0].anchors[1].out, [25.0, 10.0]));
        assert!(near(subs[0].anchors[3].inn, [26.67, 30.0]));
        assert_eq!(subs[1].anchors.len(), 3);
        let text = to_d(&subs);
        assert_eq!(parse(&text).unwrap(), parse(&to_d(&parse(&text).unwrap())).unwrap());
        assert!(text.starts_with("M10 10 L20 10 C25 10 30 15 30 20 C"), "{text}");
        assert!(text.ends_with("M100 100 L110 100 L110 110"), "{text}");
        let closing = parse("M0 0 L10 0 L10 10 L0 0 Z").unwrap();
        assert_eq!(closing[0].anchors.len(), 3);
        assert_eq!(to_d(&closing), "M0 0 L10 0 L10 10 Z");
    }

    #[test]
    fn arcs_become_curves_that_end_where_they_should() {
        let subs = parse("M0 50 A50 50 0 0 1 100 50").unwrap();
        let line = &flatten(&subs, 1.0)[0];
        assert!(near(*line.last().unwrap(), [100.0, 50.0]));
        let top = line.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!((top - 0.0).abs() < 0.2, "{top}");
        assert!(line.iter().all(|p| (dist(*p, [50.0, 50.0]) - 50.0).abs() < 0.1));
    }

    #[test]
    fn points_move_handles_follow_and_smooth_points_stay_smooth() {
        let d = "M0 0 C10 -10 20 -10 30 0 C40 10 50 10 60 0";
        assert_eq!(
            edit(d, json!({"move":1,"to":[30,5]})),
            "M0 0 C10 -10 20 -5 30 5 C40 15 50 10 60 0"
        );
        let subs = parse(&edit(d, json!({"handle":1,"out":[30,20]}))).unwrap();
        let a = subs[0].anchors[1];
        assert!(
            near(a.out, [30.0, 20.0]) && near(a.inn, [30.0, -14.14]) && a.smooth(),
            "{a:?}"
        );
        let free = parse(&edit(d, json!({"handle":1,"out":[30,20],"free":true}))).unwrap();
        assert!(!free[0].anchors[1].smooth());
        assert_eq!(
            edit(d, json!({"move":[0,2],"by":[1,2]})),
            "M1 2 C11 -8 20 -10 30 0 C40 10 51 12 61 2"
        );
        assert_eq!(edit("M0 0 L10 0 L20 0", json!({"corner":1})), "M0 0 L10 0 L20 0");
        let smooth = parse(&edit("M0 0 L10 10 L20 0", json!({"smooth":1}))).unwrap();
        assert!(smooth[0].anchors[1].smooth());
    }

    #[test]
    fn inserting_splits_a_curve_without_changing_it() {
        let d = "M0 0 C0 100 100 100 100 0";
        let split = parse(&edit(d, json!({"insert":0,"t":0.5}))).unwrap();
        assert_eq!(split[0].anchors.len(), 3);
        assert!(near(split[0].anchors[1].at, [50.0, 75.0]));
        let before = flatten(&parse(d).unwrap(), 0.5);
        let after = flatten(&split, 0.5);
        before[0].iter().step_by(7).for_each(|p| {
            let closest = after[0].iter().map(|q| dist(*p, *q)).fold(f32::MAX, f32::min);
            assert!(closest < 0.6, "{p:?} {closest}");
        });
        assert_eq!(edit("M0 0 L10 0", json!({"insert":0,"t":0.25})), "M0 0 L2.5 0 L10 0");
        assert_eq!(
            edit("M0 0 L10 0 L10 10 Z", json!({"insert":2})),
            "M0 0 L10 0 L10 10 L5 5 Z"
        );
        assert!(
            PathEdit::parse(&mut json!({"insert":1}).as_object().unwrap().clone())
                .unwrap()
                .apply(&mut parse("M0 0 L10 0").unwrap())
                .is_err()
        );
    }

    #[test]
    fn adding_a_point_near_a_segment_splits_it_there() {
        assert_eq!(
            edit("M0 0 L100 0 L100 100", json!({"add":[103,40]})),
            "M0 0 L100 0 L100 40 L100 100"
        );
    }

    #[test]
    fn removing_points_joins_their_neighbours() {
        assert_eq!(edit("M0 0 L10 0 L20 0 L30 5", json!({"rm":[1,2]})), "M0 0 L30 5");
        let mut subs = parse("M0 0 L10 0").unwrap();
        let mut map = json!({"rm":0}).as_object().unwrap().clone();
        assert!(PathEdit::parse(&mut map).unwrap().apply(&mut subs).is_err());
    }

    #[test]
    fn shapes_convert_to_the_same_outline() {
        let rect = from_shape(&Shape::Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 50.0,
            radius: 0.0,
        })
        .unwrap();
        assert_eq!(to_d(&rect), "M10 20 L110 20 L110 70 L10 70 Z");
        let ellipse = from_shape(&Shape::Ellipse {
            cx: 0.0,
            cy: 0.0,
            rx: 40.0,
            ry: 20.0,
        })
        .unwrap();
        let outline = flatten(&ellipse, 0.5);
        assert!(outline[0].iter().all(|p| {
            let r = (p[0] / 40.0).powi(2) + (p[1] / 20.0).powi(2);
            (r - 1.0).abs() < 0.002
        }));
        let round = from_shape(&Shape::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 40.0,
            radius: 10.0,
        })
        .unwrap();
        assert_eq!(round[0].anchors.len(), 8);
        assert!(
            round[0].anchors[1..]
                .iter()
                .step_by(2)
                .all(|a| !a.has_in() || !a.has_out())
        );
    }

    #[test]
    fn simplify_turns_a_dense_polyline_into_few_smooth_points() {
        let points: Vec<String> = (0..=250)
            .map(|i| {
                let x = i as f32 * 4.0;
                let y = 300.0 + 80.0 * libm::sinf(x / 160.0) + 20.0 * libm::sinf(x / 37.0);
                format!("{x} {y:.2}")
            })
            .collect();
        let d = format!("M{} L{} L1000 600 L0 600 Z", points[0], points[1..].join(" L"));
        let dense = parse(&d).unwrap();
        let simple = parse(&edit(&d, json!({"simplify":1.0}))).unwrap();
        let (before, after) = (dense[0].anchors.len(), simple[0].anchors.len());
        assert!(after * 8 < before, "{before} -> {after}");
        let fine = flatten(&simple, 0.5);
        dense[0].anchors.iter().for_each(|a| {
            let closest = fine[0].iter().map(|q| dist(a.at, *q)).fold(f32::MAX, f32::min);
            assert!(closest < 1.6, "{:?} is {closest} away", a.at);
        });
        assert!(simple[0].anchors.iter().any(|a| a.at == [1000.0, 600.0] && !a.smooth()));
    }

    #[test]
    fn the_nearest_segment_is_found() {
        let subs = parse("M0 0 L100 0 L100 100").unwrap();
        let (segment, t, d) = nearest(&subs, [100.0, 30.0]).unwrap();
        assert_eq!(segment, 1);
        assert!((t - 0.3).abs() < 0.02 && d < 0.5);
    }
}
