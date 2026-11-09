use crate::brushes::{BrushBlend, BrushDef, Curve, PaintDef};
use crate::doc::MarkIds;
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

pub fn following(marks: &[Mark]) -> BTreeSet<u32> {
    let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    crate::stroke::strokes_of(marks)
        .filter(|s| s.on.iter().any(|id| live.contains(id)))
        .map(|s| s.id)
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Followed {
    pub moving: Vec<u32>,
    pub alone: Vec<u32>,
}

pub fn follow(marks: &mut Vec<Mark>, chosen: &[u32], ids: MarkIds) -> Followed {
    let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    let picked: BTreeSet<u32> = chosen.iter().copied().collect();
    let mut next = ids.next(marks);
    let mut followed = Followed {
        moving: chosen.to_vec(),
        alone: Vec::new(),
    };
    let out = std::mem::take(marks).into_iter().flat_map(|mark| {
        let Mark::Stroke(mut stroke) = mark else {
            return vec![mark];
        };
        let targets: Vec<u32> = stroke.on.iter().copied().filter(|id| live.contains(id)).collect();
        let held: Vec<u32> = targets.iter().copied().filter(|id| picked.contains(id)).collect();
        let whole = held.len() == targets.len();
        match (picked.contains(&stroke.id), targets.is_empty(), held.is_empty(), whole) {
            (_, true, ..) | (false, _, true, _) | (true, _, _, true) => {}
            (true, ..) => followed.alone.push(stroke.id),
            (false, _, _, true) => followed.moving.push(stroke.id),
            (false, ..) => {
                let copy = Stroke {
                    id: next,
                    on: held.clone(),
                    ..stroke.clone()
                };
                next += 1;
                followed.moving.push(copy.id);
                stroke.on = targets.into_iter().filter(|id| !held.contains(id)).collect();
                return vec![Mark::Stroke(stroke), Mark::Stroke(copy)];
            }
        }
        vec![Mark::Stroke(stroke)]
    });
    *marks = out.collect();
    followed
}

pub fn bend(marks: &mut Vec<Mark>, target: u32, old: &[(f32, f32)], ids: MarkIds) {
    let Some(new) = marks
        .iter()
        .find(|m| m.id() == target)
        .and_then(Mark::as_stroke)
        .map(|s| s.samples.iter().map(Sample::pos).collect::<Vec<_>>())
    else {
        return;
    };
    let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    let (from, to) = ([old.to_vec()], [new]);
    let bent = |stroke: &mut Stroke| {
        stroke.samples.iter_mut().for_each(|sample| {
            let moved = crate::tween::anchor(&from, sample.pos()).and_then(|a| crate::tween::place(&a, &to));
            if let Some((x, y)) = moved {
                sample.x = (x * crate::stroke::POS_SCALE).round() as i32;
                sample.y = (y * crate::stroke::POS_SCALE).round() as i32;
            }
        })
    };
    let mut next = ids.next(marks);
    let out = std::mem::take(marks).into_iter().flat_map(|mark| {
        let Mark::Stroke(mut stroke) = mark else {
            return vec![mark];
        };
        if stroke.id == target || !stroke.on.contains(&target) {
            return vec![Mark::Stroke(stroke)];
        }
        let others: Vec<u32> = stroke
            .on
            .iter()
            .copied()
            .filter(|id| *id != target && live.contains(id))
            .collect();
        if others.is_empty() {
            bent(&mut stroke);
            return vec![Mark::Stroke(stroke)];
        }
        let mut copy = Stroke {
            id: next,
            on: vec![target],
            ..stroke.clone()
        };
        next += 1;
        bent(&mut copy);
        stroke.on = others;
        vec![Mark::Stroke(stroke), Mark::Stroke(copy)]
    });
    *marks = out.collect();
}

pub fn prune(marks: &mut Vec<Mark>, before: &BTreeSet<u32>) {
    let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
    let gone = |id: &u32| before.contains(id) && !live.contains(id);
    marks.retain(|m| match m {
        Mark::Stroke(s) if s.on.iter().any(gone) => s.on.iter().any(|id| live.contains(id)),
        _ => true,
    });
    marks.iter_mut().for_each(|m| {
        if let Mark::Stroke(s) = m {
            s.on.retain(|id| !gone(id));
        }
    });
}

pub fn join(marks: &mut [Mark], pieces: &[(u32, u32)]) {
    marks.iter_mut().for_each(|m| {
        if let Mark::Stroke(s) = m {
            let more: Vec<u32> = pieces
                .iter()
                .filter(|(from, _)| s.on.contains(from))
                .map(|(_, part)| *part)
                .collect();
            s.on.extend(more);
        }
    });
}

pub fn detach(marks: &mut [Mark], ids: &[u32]) -> usize {
    marks
        .iter_mut()
        .filter_map(|m| match m {
            Mark::Stroke(s) if !s.on.is_empty() && (ids.contains(&s.id) || s.on.iter().any(|t| ids.contains(t))) => {
                s.on.clear();
                Some(())
            }
            _ => None,
        })
        .count()
}

pub fn remap(copied: &mut [Mark], made: &[crate::markedit::Copied]) {
    let to: BTreeMap<(&str, u32), u32> = made.iter().map(|c| ((c.cel.as_str(), c.from), c.to)).collect();
    copied.iter_mut().zip(made).for_each(|(m, c)| {
        if let Mark::Stroke(s) = m {
            s.on =
                s.on.iter()
                    .filter_map(|id| to.get(&(c.cel.as_str(), *id)).copied())
                    .collect();
        }
    });
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

    fn crossed() -> Vec<Mark> {
        let mut eraser = line(3, "eraser", 12.0, (100.0, 30.0), (100.0, 110.0));
        if let Mark::Stroke(s) = &mut eraser {
            s.on = vec![1, 2];
        }
        vec![
            line(1, "ink", 6.0, (10.0, 50.0), (200.0, 50.0)),
            line(2, "ink", 6.0, (10.0, 90.0), (200.0, 90.0)),
            eraser,
        ]
    }

    fn edit(marks: &mut Vec<Mark>, mut change: crate::markedit::MarkChange) -> Vec<u32> {
        crate::markedit::apply_following(marks, &mut change, "c1", MarkIds::default()).unwrap()
    }

    fn moved(x: f32) -> crate::markedit::MarkTransform {
        crate::markedit::MarkTransform {
            x,
            ..Default::default()
        }
    }

    fn on(marks: &[Mark], at: usize) -> (u32, Vec<u32>) {
        let s = marks[at].as_stroke().unwrap();
        (s.id, s.on.clone())
    }

    fn start_x(marks: &[Mark], at: usize) -> f32 {
        marks[at].as_stroke().unwrap().samples[0].pos().0
    }

    #[test]
    fn moving_every_mark_of_a_modifier_takes_it_along_and_moving_some_splits_it() {
        let mut marks = crossed();
        edit(
            &mut marks,
            crate::markedit::MarkChange::Transform {
                ids: vec![1, 2],
                transform: moved(40.0),
            },
        );
        assert_eq!(marks.len(), 3);
        assert_eq!(start_x(&marks, 2), 140.0, "the eraser came along");
        let mut marks = crossed();
        edit(
            &mut marks,
            crate::markedit::MarkChange::Transform {
                ids: vec![1],
                transform: moved(40.0),
            },
        );
        assert_eq!(marks.len(), 4);
        assert_eq!(
            on(&marks, 2),
            (3, vec![2]),
            "the original stays with the line that stayed"
        );
        assert_eq!(
            on(&marks, 3),
            (4, vec![1]),
            "a copy right after it goes with the moved line"
        );
        assert_eq!((start_x(&marks, 2), start_x(&marks, 3)), (100.0, 140.0));
    }

    #[test]
    fn a_turn_is_about_the_chosen_marks_not_the_ones_that_follow() {
        let mut marks = crossed();
        edit(
            &mut marks,
            crate::markedit::MarkChange::Transform {
                ids: vec![1, 2],
                transform: crate::markedit::MarkTransform {
                    rotate: 180.0,
                    ..Default::default()
                },
            },
        );
        let first = marks[0].as_stroke().unwrap().samples[0].pos();
        assert!(
            (first.0 - 200.0).abs() < 0.1 && (first.1 - 90.0).abs() < 0.1,
            "{first:?}"
        );
    }

    #[test]
    fn deleting_marks_drops_them_from_modifiers_and_a_modifier_left_with_none_goes() {
        let mut marks = crossed();
        edit(&mut marks, crate::markedit::MarkChange::Remove { id: 1 });
        assert_eq!(on(&marks, 1), (3, vec![2]));
        edit(&mut marks, crate::markedit::MarkChange::Remove { id: 2 });
        assert!(marks.is_empty(), "the eraser went with its last line");
        let mut loose = vec![
            line(1, "ink", 6.0, (0.0, 0.0), (9.0, 0.0)),
            line(2, "eraser", 6.0, (0.0, 0.0), (9.0, 0.0)),
        ];
        edit(&mut loose, crate::markedit::MarkChange::Remove { id: 1 });
        assert_eq!(loose.len(), 1, "an unattached eraser stays, as before");
    }

    #[test]
    fn a_lasso_takes_plain_marks_and_their_modifiers_follow() {
        let mut marks = crossed();
        let around_first = vec![[0.0, 40.0], [220.0, 40.0], [220.0, 60.0], [0.0, 60.0]];
        assert_eq!(crate::markedit::lassoed(&marks, &around_first), vec![1]);
        let around_eraser = vec![[90.0, 20.0], [110.0, 20.0], [110.0, 120.0], [90.0, 120.0]];
        assert!(
            crate::markedit::lassoed(&marks, &around_eraser).is_empty(),
            "an attached eraser is not picked on its own"
        );
        let mut change = crate::markedit::MarkChange::Lasso {
            polygon: around_first,
            cut: false,
            action: crate::markedit::LassoAction::Transform(moved(40.0)),
            chosen: Vec::new(),
        };
        crate::markedit::apply_following(&mut marks, &mut change, "c1", MarkIds::default()).unwrap();
        assert_eq!(on(&marks, 3), (4, vec![1]));
        assert_eq!(start_x(&marks, 3), 140.0);
    }

    #[test]
    fn a_piece_cut_off_by_the_lasso_joins_the_modifiers_of_its_mark() {
        let mut marks = crossed();
        let left_half = vec![[0.0, 40.0], [60.0, 40.0], [60.0, 60.0], [0.0, 60.0]];
        let mut change = crate::markedit::MarkChange::Lasso {
            polygon: left_half,
            cut: true,
            action: crate::markedit::LassoAction::Transform(moved(0.0)),
            chosen: Vec::new(),
        };
        crate::markedit::apply_following(&mut marks, &mut change, "c1", MarkIds::default()).unwrap();
        let ids: Vec<u32> = marks.iter().map(Mark::id).collect();
        assert_eq!(
            ids,
            vec![1, 4, 2, 3, 5],
            "the piece sits after its mark, the eraser's copy after the eraser"
        );
        assert_eq!(on(&marks, 3), (3, vec![1, 2]));
        assert_eq!(on(&marks, 4), (5, vec![4]));
    }

    #[test]
    fn a_modifier_moved_on_its_own_is_reported_for_attaching_again() {
        let mut marks = crossed();
        let alone = edit(
            &mut marks,
            crate::markedit::MarkChange::Transform {
                ids: vec![3],
                transform: moved(300.0),
            },
        );
        assert_eq!(alone, vec![3]);
        assert_eq!(marks.len(), 3);
    }

    #[test]
    fn detaching_a_modifier_or_its_marks_leaves_it_where_it_is() {
        let mut marks = crossed();
        edit(&mut marks, crate::markedit::MarkChange::Detach { ids: vec![1] });
        assert_eq!(on(&marks, 2), (3, vec![]));
        let mut marks = crossed();
        edit(&mut marks, crate::markedit::MarkChange::Detach { ids: vec![3] });
        assert_eq!(on(&marks, 2), (3, vec![]));
        edit(
            &mut marks,
            crate::markedit::MarkChange::Transform {
                ids: vec![1, 2],
                transform: moved(40.0),
            },
        );
        assert_eq!(start_x(&marks, 2), 100.0, "a detached eraser stays put");
    }

    #[test]
    fn bending_a_stroke_bends_its_erasers_and_splits_one_it_shares() {
        let bend = |marks: &mut Vec<Mark>| {
            edit(
                marks,
                crate::markedit::MarkChange::Set {
                    id: 1,
                    props: serde_json::json!({"path": "M10 50 L200 240"})
                        .as_object()
                        .unwrap()
                        .clone(),
                },
            );
        };
        let mut alone = crossed();
        if let Mark::Stroke(s) = &mut alone[2] {
            s.on = vec![1];
        }
        bend(&mut alone);
        let eraser = alone[2].as_stroke().unwrap();
        let ends = (eraser.samples[0].pos(), eraser.samples[1].pos());
        let near = |p: (f32, f32), q: (f32, f32)| (p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5;
        assert!(
            near(ends.0, (114.14, 125.86)) && near(ends.1, (57.57, 182.43)),
            "the cut stays halfway along the line and turns with it: {ends:?}"
        );
        let mut shared = crossed();
        bend(&mut shared);
        assert_eq!(
            on(&shared, 2),
            (3, vec![2]),
            "the eraser stays with the line that did not bend"
        );
        assert_eq!(on(&shared, 3).1, vec![1], "a copy right after it bent with the line");
        assert_eq!(start_x(&shared, 2), 100.0);
        assert_ne!(
            shared[3].as_stroke().unwrap().samples[0].pos(),
            shared[2].as_stroke().unwrap().samples[0].pos()
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
