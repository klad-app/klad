use crate::path;
use anyhow::{Result, bail};
use std::collections::BTreeSet;

pub type Cell = [i64; 2];

pub const MAX_SIZE: u32 = 64;

pub fn line(from: Cell, to: Cell) -> Vec<Cell> {
    let (dx, dy) = ((to[0] - from[0]).abs(), (to[1] - from[1]).abs());
    let (sx, sy) = ((to[0] - from[0]).signum(), (to[1] - from[1]).signum());
    let mut at = from;
    let mut error = dx - dy;
    let mut out = Vec::with_capacity((dx.max(dy) + 1) as usize);
    loop {
        out.push(at);
        if at == to {
            return out;
        }
        let twice = 2 * error;
        if twice > -dy {
            error -= dy;
            at[0] += sx;
        }
        if twice < dx {
            error += dx;
            at[1] += sy;
        }
    }
}

pub fn cell_at(x: f32, y: f32) -> Cell {
    [x.floor() as i64, y.floor() as i64]
}

pub fn trace(points: &[(f32, f32)]) -> Vec<Cell> {
    let cells: Vec<Cell> = points.iter().map(|p| cell_at(p.0, p.1)).collect();
    let Some(&first) = cells.first() else {
        return Vec::new();
    };
    cells.windows(2).fold(vec![first], |mut out, pair| {
        out.extend(line(pair[0], pair[1]).into_iter().skip(1));
        out
    })
}

pub fn extend(run: &mut Vec<Cell>, to: Cell) {
    match run.last().copied() {
        None => run.push(to),
        Some(last) if last == to => {}
        Some(last) => run.extend(line(last, to).into_iter().skip(1)),
    }
}

pub fn perfect(run: &[Cell]) -> Vec<Cell> {
    perfect_keeping(run, &BTreeSet::new())
}

pub fn perfect_keeping(run: &[Cell], keep: &BTreeSet<usize>) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::with_capacity(run.len());
    let mut i = 0;
    while i < run.len() {
        let corner = i > 0 && i + 1 < run.len() && !keep.contains(&i) && {
            let (before, at, after) = (out[out.len() - 1], run[i], run[i + 1]);
            let touches = |a: Cell, b: Cell| (a[0] == b[0]) != (a[1] == b[1]);
            touches(before, at) && touches(after, at) && before[0] != after[0] && before[1] != after[1]
        };
        if !corner {
            out.push(run[i]);
        }
        i += 1;
    }
    out
}

fn turns(points: &[(f32, f32)], at: usize) -> bool {
    if at == 0 || at + 1 >= points.len() {
        return false;
    }
    let (a, b, c) = (points[at - 1], points[at], points[at + 1]);
    let (ux, uy, vx, vy) = (b.0 - a.0, b.1 - a.1, c.0 - b.0, c.1 - b.1);
    let (lu, lv) = (libm::sqrtf(ux * ux + uy * uy), libm::sqrtf(vx * vx + vy * vy));
    lu > 1e-6 && lv > 1e-6 && (ux * vx + uy * vy) / (lu * lv) < 0.8
}

pub struct Run {
    pub cells: Vec<Cell>,
    pub corners: BTreeSet<usize>,
}

pub fn run_of(points: &[(f32, f32)]) -> Run {
    let cells: Vec<Cell> = points.iter().map(|p| cell_at(p.0, p.1)).collect();
    let Some(&first) = cells.first() else {
        return Run {
            cells: Vec::new(),
            corners: BTreeSet::new(),
        };
    };
    let mut corners = BTreeSet::new();
    let traced = cells.windows(2).enumerate().fold(vec![first], |mut out, (i, pair)| {
        out.extend(line(pair[0], pair[1]).into_iter().skip(1));
        if turns(points, i + 1) {
            corners.insert(out.len() - 1);
        }
        out
    });
    Run { cells: traced, corners }
}

pub fn stamp(run: &[Cell], size: u32) -> Vec<Cell> {
    let size = size.clamp(1, MAX_SIZE) as i64;
    let low = (size - 1) / 2;
    let mut seen = BTreeSet::new();
    run.iter()
        .flat_map(|c| (0..size).flat_map(move |j| (0..size).map(move |i| [c[0] - low + i, c[1] - low + j])))
        .filter(|c| seen.insert(*c))
        .collect()
}

pub fn of_path(d: &str) -> Result<Vec<Run>> {
    let lines = path::flatten(d)?;
    let runs: Vec<Run> = lines
        .iter()
        .map(|line| run_of(&line.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>()))
        .filter(|run| !run.cells.is_empty())
        .collect();
    if runs.is_empty() {
        bail!("the path has no points");
    }
    Ok(runs)
}

pub fn drawn(runs: &[Run], size: u32) -> Vec<Cell> {
    let mut seen = BTreeSet::new();
    runs.iter()
        .map(|run| match size > 1 {
            true => run.cells.clone(),
            false => perfect_keeping(&run.cells, &run.corners),
        })
        .flat_map(|run| stamp(&run, size))
        .filter(|c| seen.insert(*c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_step_one_pixel_at_a_time_and_end_where_they_are_told() {
        assert_eq!(line([0, 0], [3, 0]), [[0, 0], [1, 0], [2, 0], [3, 0]]);
        assert_eq!(line([2, 2], [2, 2]), [[2, 2]]);
        assert_eq!(line([0, 0], [3, 3]), [[0, 0], [1, 1], [2, 2], [3, 3]]);
        let shallow = line([0, 0], [6, 2]);
        assert_eq!(shallow.len(), 7, "one pixel per column: {shallow:?}");
        assert_eq!((shallow[0], shallow[6]), ([0, 0], [6, 2]));
        assert!(
            shallow
                .windows(2)
                .all(|w| (w[1][0] - w[0][0]).abs() <= 1 && (w[1][1] - w[0][1]).abs() <= 1)
        );
        let back = line([6, 2], [0, 0]);
        assert_eq!(back.len(), 7);
        assert_eq!(back[6], [0, 0]);
    }

    #[test]
    fn a_traced_hand_drops_the_corner_pixels_that_double_a_line() {
        let run = trace(&[(0.5, 0.5), (1.5, 0.6), (1.4, 1.5), (2.5, 1.5), (2.5, 2.5)]);
        assert_eq!(
            run,
            [[0, 0], [1, 0], [1, 1], [2, 1], [2, 2]],
            "every step a pixel, corners and all"
        );
        assert_eq!(
            perfect(&run),
            [[0, 0], [1, 1], [2, 2]],
            "the L corners go, the diagonal stays"
        );
        let straight = [[0, 0], [1, 0], [2, 0], [2, 1], [2, 2]];
        assert_eq!(
            perfect(&straight),
            [[0, 0], [1, 0], [2, 1], [2, 2]],
            "one corner of a real turn goes"
        );
        assert_eq!(perfect(&[[4, 4]]), [[4, 4]]);
        assert_eq!(perfect(&[]), Vec::<Cell>::new());
        let mut live = Vec::new();
        [[0, 0], [0, 0], [3, 1]].iter().for_each(|c| extend(&mut live, *c));
        assert_eq!(live, [[0, 0], [1, 0], [2, 1], [3, 1]], "a fast pen leaves no holes");
    }

    #[test]
    fn a_size_is_a_square_of_whole_pixels_and_paths_become_pixels() {
        assert_eq!(stamp(&[[5, 5]], 1), [[5, 5]]);
        assert_eq!(stamp(&[[5, 5]], 2), [[5, 5], [6, 5], [5, 6], [6, 6]]);
        let three = stamp(&[[5, 5]], 3);
        assert_eq!(three.len(), 9);
        assert!(three.contains(&[4, 4]) && three.contains(&[6, 6]));
        assert_eq!(
            stamp(&[[0, 0], [1, 0]], 2).len(),
            6,
            "pixels two stamps share are kept once"
        );
        let runs = of_path("M1.5 1.5 L5.5 1.5 M0.5 3.5 L0.5 5.5").unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].cells, [[1, 1], [2, 1], [3, 1], [4, 1], [5, 1]]);
        assert_eq!(drawn(&runs, 1).len(), 8);
        let corner = of_path("M0.5 0.5 L3.5 0.5 L3.5 3.5").unwrap();
        assert_eq!(drawn(&corner, 1).len(), 7, "the corner a path asks for is kept");
        let curve = of_path("M0.5 8.5 C 3 8.5 8.5 3 8.5 0.5").unwrap();
        let cells = drawn(&curve, 1);
        assert_eq!((cells[0], cells[cells.len() - 1]), ([0, 8], [8, 0]));
        assert!(
            cells.windows(3).all(|w| {
                let side = |a: Cell, b: Cell| (a[0] == b[0]) != (a[1] == b[1]);
                !(side(w[0], w[1]) && side(w[2], w[1]) && w[0][0] != w[2][0] && w[0][1] != w[2][1])
            }),
            "a curve has no doubled corners: {cells:?}"
        );
        assert!(of_path("").is_err());
    }
}
