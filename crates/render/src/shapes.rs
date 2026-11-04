use crate::overlay;
use anyhow::{Result, anyhow, bail};
use klad_core::cut::inside_polygon;
use klad_core::doc::{Document, Item, Shape};
use klad_core::path::{flatten as flatten_d, resample};
use klad_core::pathedit;
use klad_core::rng::Rng;

pub type P = [f32; 2];
pub type Run = Vec<[f32; 3]>;

const STEP: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub node: String,
    pub points: Vec<P>,
    pub closed: bool,
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale(a: P, k: f32) -> P {
    [a[0] * k, a[1] * k]
}

fn len(a: P) -> f32 {
    libm::sqrtf(a[0] * a[0] + a[1] * a[1])
}

fn unit(a: P) -> P {
    let l = len(a);
    if l < 1e-6 { [0.0, 0.0] } else { scale(a, 1.0 / l) }
}

fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn affine(m: [f32; 6], p: P) -> P {
    [m[0] * p[0] + m[1] * p[1] + m[4], m[2] * p[0] + m[3] * p[1] + m[5]]
}

pub fn area(polygon: &[P]) -> f32 {
    let n = polygon.len();
    (0..n)
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        / 2.0
}

fn node_outlines(doc: &Document, id: &str, step: f32, out: &mut Vec<Outline>) -> Result<()> {
    let Some(Item::Node(_, _, node)) = doc.item(id) else {
        bail!("{id} is not a vector shape (rect, ellipse, path or group)");
    };
    if let Shape::Group { children } = &node.shape {
        return children.iter().try_for_each(|c| node_outlines(doc, &c.id, step, out));
    }
    let m = overlay::local_to_canvas(doc, id)?.ok_or_else(|| anyhow!("{id} has no place on the canvas"))?;
    let subs = pathedit::from_shape(&node.shape)?;
    pathedit::flatten(&subs, step)
        .into_iter()
        .zip(subs.iter())
        .filter(|(points, _)| points.len() >= 2)
        .for_each(|(points, s)| {
            let mut points: Vec<P> = points.iter().map(|p| affine(m, *p)).collect();
            if s.closed && points.len() > 2 && len(sub(points[0], points[points.len() - 1])) < 0.01 {
                points.pop();
            }
            out.push(Outline {
                node: id.to_string(),
                points,
                closed: s.closed,
            });
        });
    Ok(())
}

pub fn outlines(doc: &Document, ids: &[String]) -> Result<Vec<Outline>> {
    let mut out = Vec::new();
    ids.iter().try_for_each(|id| node_outlines(doc, id, STEP, &mut out))?;
    Ok(out)
}

pub fn path_outlines(d: &str) -> Result<Vec<Outline>> {
    Ok(flatten_d(d)?
        .into_iter()
        .filter(|l| l.len() >= 2)
        .map(|mut points| {
            let closed = points.len() > 2 && len(sub(points[0], points[points.len() - 1])) < 0.01;
            if closed {
                points.pop();
            }
            Outline {
                node: "path".into(),
                points,
                closed,
            }
        })
        .collect())
}

fn even(points: &[P], closed: bool) -> Vec<P> {
    let mut ring = points.to_vec();
    if closed && !ring.is_empty() {
        ring.push(ring[0]);
    }
    let mut out = resample(&ring, STEP);
    if closed && out.len() > 2 && len(sub(out[0], out[out.len() - 1])) < STEP * 0.5 {
        out.pop();
    }
    out
}

fn tangents(points: &[P], closed: bool) -> Vec<P> {
    let n = points.len();
    (0..n)
        .map(|i| {
            let prev = match (i, closed) {
                (0, true) => points[n - 1],
                (0, false) => points[0],
                _ => points[i - 1],
            };
            let next = match (i + 1 == n, closed) {
                (true, true) => points[0],
                (true, false) => points[i],
                _ => points[i + 1],
            };
            unit(sub(next, prev))
        })
        .collect()
}

pub fn outward(points: &[P], closed: bool) -> Vec<P> {
    let lefts: Vec<P> = tangents(points, closed).iter().map(|t| [t[1], -t[0]]).collect();
    if !closed || points.len() < 3 {
        return lefts;
    }
    let flip = area(points) < 0.0;
    lefts
        .into_iter()
        .map(|n| if flip { scale(n, -1.0) } else { n })
        .collect()
}

pub fn offset(polygon: &[P], by: f32) -> Vec<P> {
    if by == 0.0 || polygon.len() < 3 {
        return polygon.to_vec();
    }
    let normals = outward(polygon, true);
    polygon
        .iter()
        .zip(normals)
        .map(|(p, n)| add(*p, scale(n, by)))
        .collect()
}

pub fn shifted(polygon: &[P], by: P) -> Vec<P> {
    polygon.iter().map(|p| add(*p, by)).collect()
}

fn turn(a: P, b: P) -> f32 {
    let cross = a[0] * b[1] - a[1] * b[0];
    libm::atan2f(cross, dot(a, b)).abs().to_degrees()
}

#[derive(Clone, Copy, Debug)]
pub struct InkRules {
    pub weight: f32,
    pub light: P,
    pub break_deg: f32,
    pub gap: f32,
    pub wobble: f32,
    pub seed: u32,
}

impl Default for InkRules {
    fn default() -> Self {
        Self {
            weight: 6.0,
            light: [-1.0, -1.0],
            break_deg: 60.0,
            gap: 0.0,
            wobble: 0.6,
            seed: 1,
        }
    }
}

fn smooth(values: &[f32], closed: bool, reach: usize) -> Vec<f32> {
    let n = values.len() as i64;
    (0..n)
        .map(|i| {
            let picks: Vec<f32> = (i - reach as i64..=i + reach as i64)
                .filter_map(|j| match closed {
                    true => Some(values[j.rem_euclid(n) as usize]),
                    false => (0..n).contains(&j).then(|| values[j as usize]),
                })
                .collect();
            picks.iter().sum::<f32>() / picks.len().max(1) as f32
        })
        .collect()
}

pub fn ink(outlines: &[Outline], rules: &InkRules) -> Vec<Run> {
    let light = unit(rules.light);
    let closed_shapes: Vec<(usize, &Vec<P>)> = outlines
        .iter()
        .enumerate()
        .filter(|(_, o)| o.closed && o.points.len() >= 3)
        .map(|(i, o)| (i, &o.points))
        .collect();
    outlines
        .iter()
        .enumerate()
        .flat_map(|(index, outline)| {
            let points = even(&outline.points, outline.closed);
            if points.len() < 2 {
                return Vec::new();
            }
            let n = points.len();
            let tangent = tangents(&points, outline.closed);
            let normal = outward(&points, outline.closed);
            let corner = |i: usize| -> bool {
                let (a, b) = match (i, outline.closed) {
                    (0, false) => return false,
                    (i, false) if i + 1 == n => return false,
                    _ => (
                        sub(points[i], points[(i + n - 1) % n]),
                        sub(points[(i + 1) % n], points[i]),
                    ),
                };
                turn(a, b) > rules.break_deg
                    || turn(
                        sub(points[i], points[(i + n - 2) % n]),
                        sub(points[(i + 2) % n], points[i]),
                    ) > rules.break_deg * 1.4
            };
            let corners: Vec<usize> = (0..n).filter(|i| corner(*i)).fold(Vec::new(), |mut acc, i| {
                if acc.last().is_none_or(|l| i > *l + 2) {
                    acc.push(i);
                }
                acc
            });
            let raw: Vec<f32> = (0..n)
                .map(|i| {
                    let facing = dot(normal[i], light);
                    let lit = 0.55 + 0.45 * (1.0 - facing) / 2.0;
                    let probe = add(points[i], scale(normal[i], 1.5));
                    let inner = closed_shapes
                        .iter()
                        .any(|(j, poly)| *j != index && inside_polygon(poly, (probe[0], probe[1])));
                    let silhouette = if inner { 0.7 } else { 1.0 };
                    let bend = turn(tangent[(i + n - 1) % n], tangent[(i + 1) % n]).to_radians() / (2.0 * STEP);
                    let curve = 1.0 - 0.35 * (bend * rules.weight).min(1.0);
                    lit * silhouette * curve
                })
                .collect();
            let pressure = smooth(&raw, outline.closed, 4);
            let mut rng = Rng::new(rules.seed as u64 * 7919 + index as u64);
            let (p1, p2) = (
                rng.range(0.0, core::f32::consts::TAU),
                rng.range(0.0, core::f32::consts::TAU),
            );
            let wobbled: Vec<[f32; 3]> = (0..n)
                .map(|i| {
                    let s = i as f32 * STEP;
                    let w = rules.wobble * (0.6 * libm::sinf(s * 0.05 + p1) + 0.4 * libm::sinf(s * 0.13 + p2));
                    let p = add(points[i], scale(normal[i], w));
                    [
                        p[0],
                        p[1],
                        (pressure[i] * (1.0 + 0.05 * libm::sinf(s * 0.09 + p2))).clamp(0.15, 1.0),
                    ]
                })
                .collect();
            let runs: Vec<Run> = match (outline.closed, corners.is_empty()) {
                (true, true) => {
                    let start = (0..n)
                        .min_by(|a, b| wobbled[*a][2].total_cmp(&wobbled[*b][2]))
                        .unwrap_or(0);
                    let overlap = (12.0 / STEP) as usize;
                    vec![(start..=start + n + overlap).map(|i| wobbled[i % n]).collect()]
                }
                (true, false) => (0..corners.len())
                    .map(|k| {
                        let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
                        let end = if b <= a { b + n } else { b };
                        (a..=end).map(|i| wobbled[i % n]).collect()
                    })
                    .collect(),
                (false, _) => {
                    let mut cuts = vec![0];
                    cuts.extend(corners.iter().copied());
                    cuts.push(n - 1);
                    cuts.windows(2).map(|w| wobbled[w[0]..=w[1]].to_vec()).collect()
                }
            };
            runs.into_iter()
                .map(|run| trim(&run, rules.gap))
                .filter(|run| run.len() >= 2 && run_length(run) >= 3.0)
                .collect()
        })
        .collect()
}

fn run_length(run: &[[f32; 3]]) -> f32 {
    run.windows(2)
        .map(|w| len([w[1][0] - w[0][0], w[1][1] - w[0][1]]))
        .sum()
}

fn trim(run: &[[f32; 3]], gap: f32) -> Run {
    if gap <= 0.0 {
        return run.to_vec();
    }
    let drop = (gap / STEP).ceil() as usize;
    match run.len() > drop * 2 + 2 {
        true => run[drop..run.len() - drop].to_vec(),
        false => Vec::new(),
    }
}

pub struct LightCuts {
    pub polygon: Vec<P>,
    pub outside: Vec<P>,
    pub share: f32,
}

pub fn shade(outlines: &[Outline], light: P, depth: f32, soft: usize) -> Vec<LightCuts> {
    let l = unit(light);
    let steps = soft.max(1);
    outlines
        .iter()
        .filter(|o| o.closed && o.points.len() >= 3)
        .flat_map(|o| {
            (0..steps).map(move |k| {
                let spread = match steps {
                    1 => 1.0,
                    _ => 0.6 + 0.8 * k as f32 / (steps - 1) as f32,
                };
                LightCuts {
                    polygon: o.points.clone(),
                    outside: shifted(&o.points, scale(l, depth * spread)),
                    share: 1.0 / steps as f32,
                }
            })
        })
        .collect()
}

pub fn rim(outlines: &[Outline], light: P, width: f32) -> Vec<LightCuts> {
    let l = unit(light);
    outlines
        .iter()
        .filter(|o| o.closed && o.points.len() >= 3)
        .map(|o| LightCuts {
            polygon: o.points.clone(),
            outside: shifted(&o.points, scale(l, -width)),
            share: 1.0,
        })
        .collect()
}

fn inside_any(areas: &[Vec<P>], p: P) -> bool {
    areas.iter().any(|a| inside_polygon(a, (p[0], p[1])))
}

fn taper_profile(k: usize, count: usize) -> f32 {
    let t = k as f32 / (count.max(2) - 1) as f32;
    libm::powf(libm::sinf(core::f32::consts::PI * t).max(0.0), 0.6).max(0.15)
}

fn sampled(points: &[P]) -> Run {
    let even = resample(points, STEP);
    let count = even.len();
    even.iter()
        .enumerate()
        .map(|(k, p)| [p[0], p[1], taper_profile(k, count)])
        .collect()
}

pub const HATCH_GAP: (f32, f32) = (0.3, 1.1);

#[derive(Clone, Copy, Debug)]
pub struct HatchRules {
    pub angle: f32,
    pub along: bool,
    pub spacing: f32,
    pub length: (f32, f32),
    pub jitter: f32,
    pub gap: (f32, f32),
    pub seed: u32,
}

fn pieces(line: &[P], rules: &HatchRules, rng: &mut Rng, keep: impl Fn(P) -> bool) -> Vec<Run> {
    let total = klad_core::path::length(line);
    if total < 1.0 {
        return Vec::new();
    }
    let even = resample(line, STEP);
    let mut out = Vec::new();
    let mut at = rng.range(0.0, rules.spacing);
    while at < total {
        let size = rng.range(rules.length.0, rules.length.1.max(rules.length.0));
        let (a, b) = (
            (at / STEP) as usize,
            (((at + size) / STEP) as usize).min(even.len() - 1),
        );
        let wiggle = rng.signed() * rules.jitter * rules.spacing * 0.5;
        let piece: Vec<P> = even[a.min(even.len() - 1)..=b]
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let t = if k + 1 < (b - a + 1) { k } else { k.saturating_sub(1) };
                let dir = unit(sub(even[(a + t + 1).min(even.len() - 1)], even[a + t]));
                add(*p, scale([-dir[1], dir[0]], wiggle))
            })
            .collect();
        piece
            .split(|p| !keep(*p))
            .filter(|s| s.len() >= 3 && klad_core::path::length(s) >= rules.length.0 * 0.4)
            .for_each(|s| out.push(sampled(s)));
        at += size + rng.range(rules.spacing * rules.gap.0, rules.spacing * rules.gap.1);
    }
    out
}

pub fn hatch(areas: &[Vec<P>], rules: &HatchRules) -> Vec<Run> {
    let mut rng = Rng::new(rules.seed as u64 * 104_729 + 17);
    let keep = |p: P| inside_any(areas, p);
    if rules.along {
        return areas
            .iter()
            .flat_map(|poly| {
                let base = area_ref(poly).abs();
                (1..400)
                    .map(|k| offset(poly, -(k as f32) * rules.spacing))
                    .take_while(|ring| ring.len() >= 3 && area_ref(ring).abs() > rules.spacing * rules.spacing * 2.0)
                    .take_while(|ring| area_ref(ring).abs() < base * 1.01)
                    .collect::<Vec<_>>()
            })
            .flat_map(|ring| {
                let mut line = ring.clone();
                line.push(ring[0]);
                pieces(&line, rules, &mut rng, keep)
            })
            .collect();
    }
    let a = rules.angle.to_radians();
    let u = [libm::cosf(a), libm::sinf(a)];
    let v = [-u[1], u[0]];
    let all: Vec<P> = areas.iter().flatten().copied().collect();
    if all.is_empty() {
        return Vec::new();
    }
    let (vmin, vmax) = all.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        let d = dot(*p, v);
        (lo.min(d), hi.max(d))
    });
    let (umin, umax) = all.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        let d = dot(*p, u);
        (lo.min(d), hi.max(d))
    });
    let lines = ((vmax - vmin) / rules.spacing).floor() as usize;
    (0..=lines)
        .flat_map(|k| {
            let t = vmin + rules.spacing * (k as f32 + 0.5) + rng.signed() * rules.jitter * rules.spacing * 0.3;
            let line = vec![
                add(scale(v, t), scale(u, umin - 2.0)),
                add(scale(v, t), scale(u, umax + 2.0)),
            ];
            pieces(&line, rules, &mut rng, keep)
        })
        .collect()
}

fn area_ref(polygon: &[P]) -> f32 {
    area(polygon)
}

#[derive(Clone, Copy, Debug)]
pub struct ScatterRules {
    pub count: Option<usize>,
    pub spacing: f32,
    pub size: (f32, f32),
    pub turn: (f32, f32),
    pub seed: u32,
}

pub fn scatter(areas: &[Vec<P>], avoid: &[Vec<P>], mark: &[Vec<P>], rules: &ScatterRules) -> Vec<Run> {
    let mut rng = Rng::new(rules.seed as u64 * 15_485_863 + 3);
    let all: Vec<P> = areas.iter().flatten().copied().collect();
    if all.is_empty() || mark.is_empty() {
        return Vec::new();
    }
    let (lo, hi) = all.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
    });
    let fits = |p: P| inside_any(areas, p) && !inside_any(avoid, p);
    let spots: Vec<P> = match rules.count {
        Some(count) => {
            let mut found = Vec::new();
            let mut tries = 0;
            while found.len() < count && tries < count * 60 {
                tries += 1;
                let p = [rng.range(lo[0], hi[0]), rng.range(lo[1], hi[1])];
                if fits(p) {
                    found.push(p);
                }
            }
            found
        }
        None => {
            let s = rules.spacing.max(1.0);
            let (cols, rows) = (
                ((hi[0] - lo[0]) / s).ceil() as usize,
                ((hi[1] - lo[1]) / s).ceil() as usize,
            );
            (0..rows)
                .flat_map(|r| (0..cols).map(move |c| (r, c)))
                .map(|(r, c)| {
                    [
                        lo[0] + (c as f32 + 0.5 + rng.signed() * 0.4) * s,
                        lo[1] + (r as f32 + 0.5 + rng.signed() * 0.4) * s,
                    ]
                })
                .filter(|p| fits(*p))
                .collect()
        }
    };
    spots
        .iter()
        .flat_map(|at| {
            let k = rng.range(rules.size.0, rules.size.1.max(rules.size.0));
            let a = rng.range(rules.turn.0, rules.turn.1.max(rules.turn.0)).to_radians();
            let (s, c) = (libm::sinf(a), libm::cosf(a));
            mark.iter()
                .map(|line| {
                    let placed: Vec<P> = line
                        .iter()
                        .map(|p| add(*at, [k * (p[0] * c - p[1] * s), k * (p[0] * s + p[1] * c)]))
                        .collect();
                    sampled(&placed)
                })
                .collect::<Vec<_>>()
        })
        .filter(|run| run.len() >= 2)
        .collect()
}

pub fn mark_lines(d: &str) -> Result<Vec<Vec<P>>> {
    Ok(flatten_d(d)?.into_iter().filter(|l| l.len() >= 2).collect())
}

pub struct Hand {
    pub run: Run,
    pub width: f32,
    pub taper: [f32; 2],
}

fn ease(a: f32, b: f32, u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    a + (b - a) * (3.0 * u * u - 2.0 * u * u * u)
}

pub fn hand(run: &Run, rng: &mut Rng, strength: f32, bow: bool) -> Hand {
    let attack = rng.range(0.12, 0.35);
    let release = rng.range(0.2, 0.5);
    let (start, peak, end) = (rng.range(0.08, 0.3), rng.range(0.82, 1.0), rng.range(0.03, 0.18));
    let sway = rng.signed() * 0.06;
    let arc = rng.signed() * 0.05;
    let width = rng.range(0.65, 1.35);
    let (tin, tout) = (rng.range(0.4, 0.8), rng.range(0.5, 0.9));
    let k = strength.clamp(0.0, 1.0);
    if k == 0.0 || run.len() < 2 {
        return Hand {
            run: run.clone(),
            width: 1.0,
            taper: [0.0, 0.0],
        };
    }
    let mut along = vec![0.0f32];
    run.windows(2)
        .for_each(|w| along.push(along[along.len() - 1] + len(sub([w[1][0], w[1][1]], [w[0][0], w[0][1]]))));
    let length = along[along.len() - 1].max(1e-3);
    let (first, last) = (run[0], run[run.len() - 1]);
    let chord = unit(sub([last[0], last[1]], [first[0], first[1]]));
    let normal = [-chord[1], chord[0]];
    let bend = if bow { arc * length * k } else { 0.0 };
    let out = run
        .iter()
        .zip(&along)
        .map(|(q, s)| {
            let t = s / length;
            let curve = match t {
                t if t < attack => ease(start, peak, t / attack),
                t if t > 1.0 - release => ease(peak, end, (t - (1.0 - release)) / release),
                t => {
                    peak * (1.0
                        - sway * libm::sinf(core::f32::consts::PI * (t - attack) / (1.0 - attack - release).max(0.01)))
                }
            };
            let lift = bend * libm::sinf(core::f32::consts::PI * t);
            [
                q[0] + normal[0] * lift,
                q[1] + normal[1] * lift,
                (q[2] * (1.0 - k + k * curve)).clamp(0.02, 1.0),
            ]
        })
        .collect();
    Hand {
        run: out,
        width: 1.0 + k * (width - 1.0),
        taper: [k * length * attack * tin, k * length * release * tout],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(cx: f32, cy: f32, r: f32) -> Outline {
        Outline {
            node: "c".into(),
            points: (0..64)
                .map(|i| {
                    let a = i as f32 / 64.0 * core::f32::consts::TAU;
                    [cx + r * libm::cosf(a), cy + r * libm::sinf(a)]
                })
                .collect(),
            closed: true,
        }
    }

    fn square(x: f32, y: f32, s: f32) -> Outline {
        Outline {
            node: "s".into(),
            points: vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]],
            closed: true,
        }
    }

    fn pressure_near(runs: &[Run], at: P) -> f32 {
        runs.iter()
            .flatten()
            .min_by(|a, b| len(sub([a[0], a[1]], at)).total_cmp(&len(sub([b[0], b[1]], at))))
            .map(|p| p[2])
            .unwrap()
    }

    #[test]
    fn a_square_is_inked_as_four_strokes_and_a_circle_as_one() {
        let rules = InkRules {
            wobble: 0.0,
            ..InkRules::default()
        };
        assert_eq!(ink(&[square(0.0, 0.0, 100.0)], &rules).len(), 4);
        let ring = ink(&[circle(200.0, 200.0, 80.0)], &rules);
        assert_eq!(ring.len(), 1);
        let (first, last) = (ring[0][0], ring[0][ring[0].len() - 1]);
        assert!(
            len([last[0] - first[0], last[1] - first[1]]) > 8.0,
            "the ends overlap past the start"
        );
        assert!(
            ring[0].iter().all(|p| p[2] >= first[2] - 0.05),
            "a ring starts where its line is lightest"
        );
        let open = Outline {
            node: "o".into(),
            points: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]],
            closed: false,
        };
        assert_eq!(ink(&[open], &rules).len(), 2);
        let gapped = ink(&[square(0.0, 0.0, 100.0)], &InkRules { gap: 6.0, ..rules });
        assert!(gapped.iter().all(|r| run_length(r) < 92.0));
    }

    #[test]
    fn the_side_away_from_the_light_and_the_silhouette_are_heavier() {
        let rules = InkRules {
            wobble: 0.0,
            light: [-1.0, -1.0],
            ..InkRules::default()
        };
        let runs = ink(&[circle(200.0, 200.0, 80.0)], &rules);
        let lit = pressure_near(&runs, [200.0 - 56.0, 200.0 - 56.0]);
        let dark = pressure_near(&runs, [200.0 + 56.0, 200.0 + 56.0]);
        assert!(dark > lit + 0.2, "dark {dark} lit {lit}");
        let both = ink(&[circle(200.0, 200.0, 100.0), circle(200.0, 200.0, 30.0)], &rules);
        let outer = pressure_near(&both, [300.0, 200.0]);
        let inner = pressure_near(&both, [230.0, 200.0]);
        assert!(outer > inner * 1.25, "outer {outer} inner {inner}");
    }

    #[test]
    fn the_wobble_repeats_with_its_seed() {
        let rules = InkRules::default();
        let a = ink(&[circle(100.0, 100.0, 50.0)], &rules);
        assert_eq!(a, ink(&[circle(100.0, 100.0, 50.0)], &rules));
        assert_ne!(a, ink(&[circle(100.0, 100.0, 50.0)], &InkRules { seed: 2, ..rules }));
    }

    #[test]
    fn shade_lies_away_from_the_light_and_rim_towards_it() {
        let c = circle(200.0, 200.0, 80.0);
        [[-1.0, -1.0], [1.0, 0.0], [0.0, 1.0]].iter().for_each(|light: &P| {
            let l = unit(*light);
            let dark = add([200.0, 200.0], scale(l, -70.0));
            let bright = add([200.0, 200.0], scale(l, 70.0));
            let shaded = |p: P, cuts: &[LightCuts]| {
                cuts.iter()
                    .any(|s| inside_polygon(&s.polygon, (p[0], p[1])) && !inside_polygon(&s.outside, (p[0], p[1])))
            };
            let s = shade(std::slice::from_ref(&c), *light, 20.0, 3);
            assert_eq!(s.len(), 3);
            assert!(shaded(dark, &s) && !shaded(bright, &s) && !shaded([200.0, 200.0], &s));
            let r = rim(std::slice::from_ref(&c), *light, 6.0);
            let edge = add([200.0, 200.0], scale(l, 77.0));
            assert!(shaded(edge, &r) && !shaded(bright, &r) && !shaded(dark, &r));
        });
    }

    #[test]
    fn hatching_and_scatter_stay_inside_and_repeat() {
        let area = square(0.0, 0.0, 200.0).points;
        let rules = HatchRules {
            angle: 30.0,
            along: false,
            spacing: 8.0,
            length: (12.0, 30.0),
            jitter: 0.3,
            gap: HATCH_GAP,
            seed: 4,
        };
        let lines = hatch(std::slice::from_ref(&area), &rules);
        assert!(lines.len() > 40);
        assert!(
            lines
                .iter()
                .flatten()
                .all(|p| (-1.0..=201.0).contains(&p[0]) && (-1.0..=201.0).contains(&p[1]))
        );
        assert_eq!(lines, hatch(std::slice::from_ref(&area), &rules));
        let along = hatch(
            std::slice::from_ref(&circle(100.0, 100.0, 90.0).points),
            &HatchRules { along: true, ..rules },
        );
        assert!(along.len() > 20);
        let marks = mark_lines("M0 0 L0 -18").unwrap();
        let rules = ScatterRules {
            count: Some(30),
            spacing: 14.0,
            size: (0.7, 1.2),
            turn: (-15.0, 15.0),
            seed: 9,
        };
        let hole = square(50.0, 50.0, 100.0).points;
        let grass = scatter(std::slice::from_ref(&area), std::slice::from_ref(&hole), &marks, &rules);
        assert_eq!(grass.len(), 30);
        assert!(grass.iter().all(|r| !inside_polygon(&hole, (r[0][0], r[0][1]))));
        let grid = scatter(
            std::slice::from_ref(&area),
            &[],
            &marks,
            &ScatterRules { count: None, ..rules },
        );
        assert!(grid.len() > 150);
    }

    #[test]
    fn offsets_grow_and_shrink_polygons() {
        let s = square(0.0, 0.0, 100.0).points;
        assert!(area(&offset(&s, 5.0)).abs() > area(&s).abs());
        assert!(area(&offset(&s, -5.0)).abs() < area(&s).abs());
        let reversed: Vec<P> = s.iter().rev().copied().collect();
        assert!(area(&offset(&reversed, 5.0)).abs() > area(&s).abs());
    }

    #[test]
    fn a_hand_presses_in_peaks_and_lifts_off_and_repeats_with_its_seed() {
        let run: Run = (0..60).map(|i| [i as f32 * 2.0, 10.0, 1.0]).collect();
        let a = hand(&run, &mut Rng::new(9), 1.0, true);
        let b = hand(&run, &mut Rng::new(9), 1.0, true);
        assert_eq!(a.run, b.run);
        let peak = a.run.iter().map(|q| q[2]).fold(0.0f32, f32::max);
        assert!(peak >= 0.8);
        assert!(a.run[0][2] < peak * 0.5 && a.run[a.run.len() - 1][2] < peak * 0.4);
        assert!(a.taper[0] > 0.0 && a.taper[1] > a.taper[0] * 0.3);
        let widths: Vec<f32> = (0..20)
            .map(|s| hand(&run, &mut Rng::new(s), 1.0, false).width)
            .collect();
        assert!(widths.iter().cloned().fold(0.0f32, f32::max) - widths.iter().cloned().fold(9.0f32, f32::min) > 0.3);
        let straight = hand(&run, &mut Rng::new(3), 1.0, false);
        assert!(straight.run.iter().all(|q| (q[1] - 10.0).abs() < 1e-4));
        let none = hand(&run, &mut Rng::new(3), 0.0, true);
        assert_eq!(none.run, run);
        assert_eq!((none.width, none.taper), (1.0, [0.0, 0.0]));
    }
}
