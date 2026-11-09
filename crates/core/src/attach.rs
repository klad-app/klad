use crate::brushes::{BrushBlend, BrushDef, Curve, PaintDef};
use crate::fill::Fill;
use crate::stroke::{Mark, Sample, Stroke};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Erase,
    Smudge,
    Blend,
}

pub fn modifier(def: &BrushDef) -> Option<Modifier> {
    match (def.blend, def.paint) {
        (BrushBlend::Erase, _) => Some(Modifier::Erase),
        (_, Some(PaintDef::Smudge { .. })) => Some(Modifier::Smudge),
        (_, Some(PaintDef::Blend { .. })) => Some(Modifier::Blend),
        _ => None,
    }
}

pub fn reach(stroke: &Stroke, def: &BrushDef) -> f32 {
    let most = |c: Option<Curve>| c.map_or(1.0, |c| c.min.abs().max(c.max.abs()));
    let size = &def.dynamics.size;
    let radius =
        0.5 * stroke.size * most(size.pressure) * most(size.tilt) * most(size.speed) * (1.0 + def.jitter.size.abs());
    (radius.max(0.6) * (1.0 + 2.0 * def.jitter.scatter.abs()) + 2.0) * 1.25
}

type P = (f32, f32);

struct Shape {
    segments: Vec<[P; 2]>,
    radius: f32,
    bounds: [f32; 4],
}

impl Shape {
    fn of(stroke: &Stroke, radius: f32) -> Option<Self> {
        let points: Vec<P> = stroke.samples.iter().map(Sample::pos).collect();
        let first = *points.first()?;
        let segments = match points.len() {
            1 => vec![[first, first]],
            _ => points.windows(2).map(|w| [w[0], w[1]]).collect(),
        };
        let bounds = points.iter().fold([first.0, first.1, first.0, first.1], |b, p| {
            [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]
        });
        Some(Self {
            segments,
            radius,
            bounds,
        })
    }

    fn padded(&self, pad: f32) -> [f32; 4] {
        let [x0, y0, x1, y1] = self.bounds;
        [x0 - pad, y0 - pad, x1 + pad, y1 + pad]
    }
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn to_segment(p: P, [a, b]: [P; 2]) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = match len2 > 0.0 {
        true => (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0),
        false => 0.0,
    };
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (x * x + y * y).sqrt()
}

fn cross(o: P, a: P, b: P) -> f32 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn between(s: [P; 2], t: [P; 2]) -> f32 {
    let crossed = cross(s[0], s[1], t[0]) * cross(s[0], s[1], t[1]) < 0.0
        && cross(t[0], t[1], s[0]) * cross(t[0], t[1], s[1]) < 0.0;
    match crossed {
        true => 0.0,
        false => to_segment(s[0], t)
            .min(to_segment(s[1], t))
            .min(to_segment(t[0], s))
            .min(to_segment(t[1], s)),
    }
}

struct Grid {
    cell: f32,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
    shape: Shape,
}

impl Grid {
    fn new(shape: Shape) -> Self {
        let cell = shape.radius.max(8.0);
        let mut cells: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        shape.segments.iter().enumerate().for_each(|(i, [a, b])| {
            let [x0, y0, x1, y1] = [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)];
            let (cx0, cy0, cx1, cy1) = (
                (x0 / cell).floor() as i32,
                (y0 / cell).floor() as i32,
                (x1 / cell).floor() as i32,
                (y1 / cell).floor() as i32,
            );
            (cy0..=cy1).for_each(|cy| (cx0..=cx1).for_each(|cx| cells.entry((cx, cy)).or_default().push(i)));
        });
        Self { cell, cells, shape }
    }

    fn near(&self, segment: [P; 2], pad: f32) -> bool {
        let limit = self.shape.radius + pad;
        let [a, b] = segment;
        let reach = |v: f32, lo: bool| match lo {
            true => ((v - limit) / self.cell).floor() as i32,
            false => ((v + limit) / self.cell).floor() as i32,
        };
        let (cx0, cy0, cx1, cy1) = (
            reach(a.0.min(b.0), true),
            reach(a.1.min(b.1), true),
            reach(a.0.max(b.0), false),
            reach(a.1.max(b.1), false),
        );
        (cy0..=cy1).any(|cy| {
            (cx0..=cx1).any(|cx| {
                self.cells
                    .get(&(cx, cy))
                    .is_some_and(|found| found.iter().any(|&i| between(segment, self.shape.segments[i]) <= limit))
            })
        })
    }

    fn touches_stroke(&self, stroke: &Stroke, radius: f32) -> bool {
        Shape::of(stroke, radius).is_some_and(|other| {
            overlap(self.shape.padded(self.shape.radius), other.padded(radius))
                && other.segments.iter().any(|s| self.near(*s, radius))
        })
    }

    fn touches_fill(&self, fill: &Fill) -> bool {
        let c = &fill.coverage;
        let Some(box_) = c.bounds() else {
            return false;
        };
        let [x0, y0, x1, y1] = self.shape.padded(self.shape.radius + 1.0);
        if !overlap(box_, [x0, y0, x1, y1]) {
            return false;
        }
        let (fx0, fy0) = (x0.max(box_[0]).floor() as u32, y0.max(box_[1]).floor() as u32);
        let (fx1, fy1) = (x1.min(box_[2]).ceil() as u32, y1.min(box_[3]).ceil() as u32);
        (fy0..fy1).any(|y| {
            (fx0..fx1).any(|x| {
                c.at(x, y) > 0 && {
                    let p = (x as f32 + 0.5, y as f32 + 0.5);
                    self.near([p, p], 0.75)
                }
            })
        })
    }
}

fn radius_of<'a>(stroke: &Stroke, def_of: &impl Fn(&str) -> Option<&'a BrushDef>) -> f32 {
    def_of(&stroke.brush).map_or(stroke.size * 0.5 + 2.0, |def| reach(stroke, def))
}

fn plain_near<'a>(earlier: &[Mark], grid: &Grid, def_of: &impl Fn(&str) -> Option<&'a BrushDef>) -> Vec<u32> {
    earlier
        .iter()
        .filter(|m| match m {
            Mark::Fill(f) => grid.touches_fill(f),
            Mark::Stroke(s) => {
                def_of(&s.brush).and_then(modifier).is_none() && grid.touches_stroke(s, radius_of(s, def_of))
            }
        })
        .map(Mark::id)
        .collect()
}

pub fn targets<'a>(
    marks: &[Mark],
    before: usize,
    stroke: &Stroke,
    def_of: impl Fn(&str) -> Option<&'a BrushDef>,
) -> Vec<u32> {
    let Some(def) = def_of(&stroke.brush).filter(|d| modifier(d).is_some()) else {
        return Vec::new();
    };
    let Some(shape) = Shape::of(stroke, reach(stroke, def)) else {
        return Vec::new();
    };
    let grid = Grid::new(shape);
    let earlier = &marks[..before.min(marks.len())];
    let mut found: BTreeSet<u32> = plain_near(earlier, &grid, &def_of).into_iter().collect();
    earlier.iter().enumerate().for_each(|(at, m)| {
        let Some(s) = m.as_stroke() else {
            return;
        };
        let reads = def_of(&s.brush)
            .and_then(modifier)
            .is_some_and(|k| k != Modifier::Erase);
        if !reads || !grid.touches_stroke(s, radius_of(s, &def_of)) {
            return;
        }
        match s.on.is_empty() {
            false => found.extend(s.on.iter().copied()),
            true => {
                let Some(own) = def_of(&s.brush).and_then(|d| Shape::of(s, reach(s, d))) else {
                    return;
                };
                found.extend(plain_near(&earlier[..at], &Grid::new(own), &def_of));
            }
        }
    });
    let live: BTreeSet<u32> = earlier.iter().map(Mark::id).collect();
    found.intersection(&live).copied().collect()
}

pub fn live_on(marks: &[Mark], stroke: &Stroke) -> Vec<u32> {
    match stroke.on.is_empty() {
        true => Vec::new(),
        false => {
            let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
            stroke.on.iter().copied().filter(|id| live.contains(id)).collect()
        }
    }
}

pub fn attached(marks: &[Mark]) -> BTreeMap<u32, Vec<u32>> {
    let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    crate::stroke::strokes_of(marks).for_each(|s| {
        s.on.iter()
            .filter(|id| live.contains(id))
            .for_each(|id| map.entry(*id).or_default().push(s.id))
    });
    map
}

pub fn attach<'a>(marks: &mut [Mark], at: usize, def_of: impl Fn(&str) -> Option<&'a BrushDef>) {
    let on = match marks.get(at).and_then(Mark::as_stroke) {
        Some(stroke) if stroke.on.is_empty() => targets(marks, at, stroke, &def_of),
        _ => return,
    };
    if let Some(Mark::Stroke(stroke)) = marks.get_mut(at) {
        stroke.on = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brushes::library;
    use crate::fill::{Coverage, FillSource};

    fn defs() -> BTreeMap<String, BrushDef> {
        ["ink", "eraser", "smudge", "blender", "wet", "airbrush"]
            .iter()
            .map(|id| (id.to_string(), library(id).unwrap()))
            .collect()
    }

    fn line(id: u32, brush: &str, size: f32, from: P, to: P) -> Mark {
        Stroke::new(id, brush, [0.1, 0.1, 0.1, 1.0], size, id)
            .with_points(&[(from.0, from.1, 1.0), (to.0, to.1, 1.0)], 4.0)
            .into()
    }

    fn found(marks: &[Mark], stroke: &Mark) -> Vec<u32> {
        let defs = defs();
        targets(marks, marks.len(), stroke.as_stroke().unwrap(), |id| defs.get(id))
    }

    #[test]
    fn an_eraser_attaches_to_the_marks_under_it_and_never_to_later_ones() {
        let marks = vec![
            line(1, "ink", 6.0, (10.0, 50.0), (200.0, 50.0)),
            line(2, "ink", 6.0, (10.0, 90.0), (200.0, 90.0)),
            line(3, "ink", 6.0, (10.0, 300.0), (200.0, 300.0)),
        ];
        let eraser = line(4, "eraser", 12.0, (100.0, 30.0), (100.0, 110.0));
        assert_eq!(
            found(&marks, &eraser),
            vec![1, 2],
            "the two lines it crosses, not the far one"
        );
        let defs = defs();
        assert_eq!(
            targets(&marks, 1, eraser.as_stroke().unwrap(), |id| defs.get(id)),
            vec![1],
            "only marks before it in the order"
        );
        let far = line(5, "eraser", 12.0, (400.0, 400.0), (420.0, 420.0));
        assert!(found(&marks, &far).is_empty(), "over nothing it attaches to nothing");
        let paint = line(6, "ink", 12.0, (100.0, 30.0), (100.0, 110.0));
        assert!(found(&marks, &paint).is_empty(), "paint strokes never attach");
    }

    #[test]
    fn a_line_end_inside_the_eraser_counts_even_between_samples() {
        let long = line(1, "ink", 4.0, (0.0, 0.0), (1000.0, 0.0));
        let eraser = line(2, "eraser", 10.0, (500.0, -20.0), (500.0, 20.0));
        assert_eq!(found(&[long], &eraser), vec![1]);
        let near = line(3, "ink", 4.0, (0.0, 12.0), (1000.0, 12.0));
        let dot = line(4, "eraser", 10.0, (500.0, 0.0), (500.0, 0.0));
        assert_eq!(
            found(&[near], &dot),
            vec![3],
            "generous: the brushes' reach, not their size"
        );
    }

    #[test]
    fn fills_count_by_their_pixels() {
        let mut mask = vec![0u8; 200 * 200];
        (50..60).for_each(|y| (50..150).for_each(|x| mask[y * 200 + x] = 255));
        let fill: Mark = Fill::new(
            1,
            [1.0, 0.0, 0.0, 1.0],
            FillSource::Polygon(vec![[50.0, 50.0], [150.0, 50.0], [150.0, 60.0]]),
            Coverage::from_mask(200, 200, &mask),
        )
        .into();
        let over = line(2, "eraser", 8.0, (100.0, 40.0), (100.0, 70.0));
        assert_eq!(found(std::slice::from_ref(&fill), &over), vec![1]);
        let beside = line(3, "eraser", 8.0, (100.0, 80.0), (100.0, 120.0));
        assert!(found(&[fill], &beside).is_empty());
    }

    #[test]
    fn reaching_a_smear_takes_the_marks_the_smudge_moved() {
        let leaf = line(1, "ink", 6.0, (0.0, 0.0), (40.0, 0.0));
        let mut smudge = line(2, "smudge", 20.0, (40.0, 0.0), (120.0, 0.0));
        if let Mark::Stroke(s) = &mut smudge {
            s.on = vec![1];
        }
        let trim = line(3, "eraser", 10.0, (110.0, -20.0), (110.0, 20.0));
        assert_eq!(
            found(&[leaf.clone(), smudge], &trim),
            vec![1],
            "the smear is the leaf's paint, so trimming it attaches to the leaf"
        );
        let loose = line(2, "smudge", 20.0, (30.0, 0.0), (120.0, 0.0));
        assert_eq!(
            found(&[leaf, loose], &trim),
            vec![1],
            "an unattached smudge counts by what was under it"
        );
    }

    #[test]
    fn attach_fills_in_a_stroke_already_in_the_list() {
        let defs = defs();
        let mut marks = vec![
            line(1, "ink", 6.0, (10.0, 50.0), (200.0, 50.0)),
            line(2, "blender", 30.0, (60.0, 40.0), (80.0, 60.0)),
        ];
        attach(&mut marks, 1, |id| defs.get(id));
        assert_eq!(marks[1].as_stroke().unwrap().on, vec![1]);
    }
}
