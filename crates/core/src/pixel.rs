use crate::doc::Document;
use crate::path;
use crate::stroke::Mark;
use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

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

pub fn rect(at: Cell, size: [i64; 2], filled: bool) -> Vec<Cell> {
    let (w, h) = (size[0].max(0), size[1].max(0));
    (0..h)
        .flat_map(|j| (0..w).map(move |i| (i, j)))
        .filter(|&(i, j)| filled || i == 0 || j == 0 || i == w - 1 || j == h - 1)
        .map(|(i, j)| [at[0] + i, at[1] + j])
        .collect()
}

fn ellipse_outline(at: Cell, size: [i64; 2]) -> Vec<Cell> {
    let (mut x0, mut y0, mut x1, mut y1) = (at[0], at[1], at[0] + size[0] - 1, at[1] + size[1] - 1);
    let (mut a, b) = ((x1 - x0).abs(), (y1 - y0).abs());
    let mut b1 = b & 1;
    let (mut dx, mut dy) = (4 * (1 - a) * b * b, 4 * (b1 + 1) * a * a);
    let mut err = dx + dy + b1 * a * a;
    if x0 > x1 {
        x0 = x1;
        x1 += a;
    }
    if y0 > y1 {
        y0 = y1;
    }
    y0 += (b + 1) / 2;
    y1 = y0 - b1;
    a = 8 * a * a;
    b1 = 8 * b * b;
    let mut out = Vec::new();
    loop {
        out.extend([[x1, y0], [x0, y0], [x0, y1], [x1, y1]]);
        let e2 = 2 * err;
        if e2 <= dy {
            y0 += 1;
            y1 -= 1;
            dy += a;
            err += dy;
        }
        if e2 >= dx || 2 * err > dy {
            x0 += 1;
            x1 -= 1;
            dx += b1;
            err += dx;
        }
        if x0 > x1 {
            break;
        }
    }
    while y0 - y1 <= b {
        out.extend([[x0 - 1, y0], [x1 + 1, y0], [x0 - 1, y1], [x1 + 1, y1]]);
        y0 += 1;
        y1 -= 1;
    }
    out
}

pub fn ellipse(at: Cell, size: [i64; 2], filled: bool) -> Vec<Cell> {
    if size[0] <= 0 || size[1] <= 0 {
        return Vec::new();
    }
    let outline: BTreeSet<Cell> = ellipse_outline(at, size).into_iter().collect();
    if !filled {
        return outline.into_iter().collect();
    }
    let mut rows: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    outline.iter().for_each(|c| {
        let span = rows.entry(c[1]).or_insert((c[0], c[0]));
        *span = (span.0.min(c[0]), span.1.max(c[0]));
    });
    rows.into_iter()
        .flat_map(|(y, (x0, x1))| (x0..=x1).map(move |x| [x, y]))
        .collect()
}

pub fn flood(grid: &Grid, at: Cell, diagonal: bool) -> Vec<Cell> {
    let (w, h) = (grid.width as i64, grid.height as i64);
    if at[0] < 0 || at[1] < 0 || at[0] >= w || at[1] >= h {
        return Vec::new();
    }
    let key = |c: Cell| grid.at(c[0] as u32, c[1] as u32);
    let wanted = key(at);
    let mut seen = vec![false; (w * h) as usize];
    let mut queue = vec![at];
    seen[(at[1] * w + at[0]) as usize] = true;
    let mut out = Vec::new();
    while let Some(c) = queue.pop() {
        out.push(c);
        let steps: &[(i64, i64)] = match diagonal {
            true => &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)],
            false => &[(1, 0), (-1, 0), (0, 1), (0, -1)],
        };
        steps
            .iter()
            .map(|(dx, dy)| [c[0] + dx, c[1] + dy])
            .filter(|n| n[0] >= 0 && n[1] >= 0 && n[0] < w && n[1] < h)
            .for_each(|n| {
                let i = (n[1] * w + n[0]) as usize;
                if !seen[i] && key(n) == wanted {
                    seen[i] = true;
                    queue.push(n);
                }
            });
    }
    out.sort_by_key(|c| (c[1], c[0]));
    out
}

const BAYER: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

pub fn dithered(cell: Cell, percent: u32) -> bool {
    let level = (percent.min(100) * 16 + 50) / 100;
    BAYER[cell[1].rem_euclid(4) as usize][cell[0].rem_euclid(4) as usize] < level
}

pub fn outline(grid: &Grid, inside: bool, diagonal: bool) -> Vec<Cell> {
    let (w, h) = (grid.width as i64, grid.height as i64);
    let filled =
        |c: Cell| c[0] >= 0 && c[1] >= 0 && c[0] < w && c[1] < h && grid.at(c[0] as u32, c[1] as u32).is_some();
    let steps: &[(i64, i64)] = match diagonal {
        true => &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)],
        false => &[(1, 0), (-1, 0), (0, 1), (0, -1)],
    };
    (0..h)
        .flat_map(|y| (0..w).map(move |x| [x, y]))
        .filter(|c| filled(*c) == inside)
        .filter(|c| steps.iter().any(|(dx, dy)| filled([c[0] + dx, c[1] + dy]) != inside))
        .collect()
}

pub fn merged(grids: &[Grid]) -> Option<Grid> {
    let first = grids.first()?;
    let cells = (0..first.cells.len())
        .map(|i| grids.iter().rev().find_map(|g| g.cells.get(i).cloned().flatten()))
        .collect();
    Some(Grid {
        width: first.width,
        height: first.height,
        cells,
    })
}

pub const EMPTY: char = '.';

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<Option<String>>,
}

impl Grid {
    pub fn at(&self, x: u32, y: u32) -> Option<&str> {
        self.cells.get((y * self.width + x) as usize)?.as_deref()
    }

    pub fn keys(&self) -> Vec<&str> {
        let mut seen = BTreeSet::new();
        self.cells
            .iter()
            .flatten()
            .map(String::as_str)
            .filter(|k| seen.insert(*k))
            .collect()
    }
}

pub fn grid_of(marks: &[Mark], width: u32, height: u32) -> Grid {
    let mut cells: Vec<Option<String>> = vec![None; width as usize * height as usize];
    crate::stroke::fills_of(marks).filter(|f| f.on_grid()).for_each(|fill| {
        let key = match fill.entry() {
            Some(entry) => entry.to_string(),
            None => crate::color::to_hex(fill.color()),
        };
        fill.coverage
            .cells()
            .filter(|c| c[0] < width && c[1] < height)
            .filter(|c| crate::cut::keeps(&fill.cuts, (c[0] as f32 + 0.5, c[1] as f32 + 0.5)))
            .for_each(|c| {
                let at = (c[1] * width + c[0]) as usize;
                match (fill.erases(), fill.lock) {
                    (true, _) => cells[at] = None,
                    (false, true) if cells[at].is_none() => {}
                    (false, _) => cells[at] = Some(key.clone()),
                }
            });
    });
    Grid { width, height, cells }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    pub sign: char,
    pub key: String,
    pub name: String,
    pub color: String,
}

const SIGNS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#@%&*+=~^:;!?$";

pub fn legend(doc: &Document, grid: &Grid) -> Vec<Named> {
    let used: BTreeSet<&str> = grid.keys().into_iter().collect();
    let entries = doc
        .palette
        .iter()
        .map(|s| (s.id.clone(), s.name.clone(), s.color.clone()))
        .chain(
            grid.keys()
                .into_iter()
                .filter(|k| doc.entry(k).is_none())
                .map(|k| (k.to_string(), String::new(), k.to_string())),
        );
    let mut taken: BTreeSet<char> = BTreeSet::new();
    entries
        .map(|(key, name, color)| {
            let wished = name.chars().find(|c| c.is_ascii_alphanumeric());
            let sign = wished
                .into_iter()
                .chain(wished.map(|c| c.to_ascii_uppercase()))
                .chain(SIGNS.chars())
                .find(|c| !taken.contains(c))
                .unwrap_or('?');
            taken.insert(sign);
            Named { sign, key, name, color }
        })
        .filter(|n| used.contains(n.key.as_str()))
        .collect()
}

pub fn rows(grid: &Grid, legend: &[Named]) -> Vec<String> {
    (0..grid.height)
        .map(|y| {
            (0..grid.width)
                .map(|x| match grid.at(x, y) {
                    Some(key) => legend.iter().find(|n| n.key == key).map_or('?', |n| n.sign),
                    None => EMPTY,
                })
                .collect()
        })
        .collect()
}

pub fn plain(grid: &Grid, legend: &[Named]) -> String {
    let lines = legend.iter().map(|n| {
        let value = match n.name.is_empty() {
            true => n.color.as_str(),
            false => n.name.as_str(),
        };
        format!("{} = {value}", n.sign)
    });
    std::iter::once(format!("{EMPTY} = empty"))
        .chain(lines)
        .chain(std::iter::once(String::new()))
        .chain(rows(grid, legend))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn ruled(grid: &Grid, legend: &[Named]) -> String {
    let pad = grid.height.saturating_sub(1).to_string().len().max(2);
    let tens: String = (0..grid.width)
        .map(|x| match x % 10 {
            0 => char::from_digit((x / 10) % 10, 10).unwrap_or(' '),
            _ => ' ',
        })
        .collect();
    let ones: String = (0..grid.width)
        .map(|x| char::from_digit(x % 10, 10).unwrap_or(' '))
        .collect();
    let lines = legend.iter().map(|n| {
        format!(
            "  {} {}",
            n.sign,
            [n.name.as_str(), n.color.as_str(), n.key.as_str()]
                .iter()
                .filter(|part| !part.is_empty())
                .fold(Vec::<&str>::new(), |mut kept, part| {
                    if !kept.contains(part) {
                        kept.push(part);
                    }
                    kept
                })
                .join(" ")
        )
    });
    std::iter::once(format!("  {EMPTY} empty"))
        .chain(lines)
        .chain([
            String::new(),
            format!("{:pad$}  {}", "", tens.trim_end()),
            format!("{:pad$}  {ones}", ""),
        ])
        .chain(
            rows(grid, legend)
                .into_iter()
                .enumerate()
                .map(|(y, row)| format!("{y:>pad$}  {row}")),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_text(text: &str) -> Result<(BTreeMap<String, String>, Vec<String>)> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let first = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(lines.len());
    let lines = &lines[first..];
    let Some(gap) = lines.iter().position(|l| l.trim().is_empty()) else {
        bail!(
            "a pixel grid is a legend, a blank line, then the rows: lines like \"i = ink\" first, then rows like \"..iiii..\""
        );
    };
    let legend = lines[..gap]
        .iter()
        .map(|line| {
            let line = line.trim();
            let sign = line.chars().next().unwrap_or(' ');
            let rest = line[sign.len_utf8()..].trim_start();
            match rest.strip_prefix('=') {
                Some(value) if !sign.is_whitespace() && !value.trim().is_empty() => {
                    Ok((sign.to_string(), value.trim().to_string()))
                }
                _ => bail!(
                    "{line:?} is not a legend line: one character, =, then a palette name, a colour, erase or empty"
                ),
            }
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let rows: Vec<String> = lines[gap..]
        .iter()
        .skip_while(|l| l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    let rows: Vec<String> = rows
        .iter()
        .take(rows.iter().rposition(|r| !r.is_empty()).map_or(0, |last| last + 1))
        .cloned()
        .collect();
    if rows.is_empty() {
        bail!("the pixel grid has a legend but no rows after the blank line");
    }
    Ok((legend, rows))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sign {
    Empty,
    Erase,
    Colour(String),
}

pub fn meaning(legend: &BTreeMap<String, String>, sign: char) -> Option<Sign> {
    match legend.get(&sign.to_string()).map(|v| v.trim()) {
        Some("empty" | "skip") => Some(Sign::Empty),
        Some("erase") => Some(Sign::Erase),
        Some(value) => Some(Sign::Colour(value.to_string())),
        None if sign == EMPTY => Some(Sign::Empty),
        None => None,
    }
}

pub fn placed(rows: &[String], legend: &BTreeMap<String, String>, at: Cell) -> Result<Vec<(char, Sign, Vec<Cell>)>> {
    let mut found: Vec<(char, Sign, Vec<Cell>)> = Vec::new();
    rows.iter().enumerate().try_for_each(|(y, row)| {
        row.chars().enumerate().try_for_each(|(x, sign)| {
            let cell = [at[0] + x as i64, at[1] + y as i64];
            match found.iter_mut().find(|(s, _, _)| *s == sign) {
                Some((_, _, cells)) => cells.push(cell),
                None => {
                    let Some(meaning) = meaning(legend, sign) else {
                        bail!(
                            "row {y}, column {x} holds {sign:?}, which the legend does not name: it has {}",
                            legend.keys().map(String::as_str).collect::<Vec<_>>().join(" ")
                        );
                    };
                    found.push((sign, meaning, vec![cell]));
                }
            }
            Ok(())
        })
    })?;
    Ok(found
        .into_iter()
        .filter(|(_, meaning, _)| !matches!(meaning, Sign::Empty))
        .collect())
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

    #[test]
    fn a_layer_reads_as_a_grid_of_signs_and_a_grid_text_reads_back_as_the_same_pixels() {
        use crate::doc::Swatch;
        use crate::fill::{Coverage, Fill};
        let swatch = |color: &str, name: &str, id: &str| Swatch {
            color: color.into(),
            name: name.into(),
            id: id.into(),
            ramp: String::new(),
        };
        let mut doc = Document::new(6, 4);
        doc.palette = vec![
            swatch("#22223b", "ink", "p1"),
            swatch("#f2c14e", "iris", "p2"),
            swatch("#ffffff", "", "p3"),
            swatch("#000000", "unused", "p4"),
        ];
        let cells = |list: &[[u32; 2]]| Coverage::from_cells(list);
        let colour = |hex: &str| crate::color::parse_hex(hex).unwrap();
        let mut locked = Fill::cells(4, colour("#ffffff"), "p3", false, cells(&[[1, 1], [5, 3]]));
        locked.lock = true;
        let mut cut = Fill::cells(6, colour("#f2c14e"), "p2", false, cells(&[[0, 3], [1, 3], [2, 3]]));
        cut.cuts = vec![crate::cut::Cut {
            keep: crate::cut::Keep::Inside,
            polygon: vec![[0.0, 2.9], [2.0, 2.9], [2.0, 4.1], [0.0, 4.1]],
        }];
        let marks: Vec<Mark> = vec![
            Fill::cells(
                1,
                colour("#22223b"),
                "p1",
                false,
                cells(&[[0, 0], [1, 0], [2, 0], [1, 1]]),
            )
            .into(),
            Fill::cells(2, colour("#f2c14e"), "p2", false, cells(&[[2, 0], [3, 0]])).into(),
            Fill::cells(3, colour("#000000"), "", true, cells(&[[0, 0], [4, 2]])).into(),
            locked.into(),
            Fill::cells(5, colour("#ff00ff"), "", false, cells(&[[5, 0]])).into(),
            cut.into(),
        ];
        let grid = grid_of(&marks, 6, 4);
        assert_eq!(grid.at(0, 0), None, "an erased pixel is empty");
        assert_eq!(grid.at(2, 0), Some("p2"), "a later mark covers an earlier one");
        assert_eq!(grid.at(1, 1), Some("p3"), "a locked mark paints where there is paint");
        assert_eq!(grid.at(5, 3), None, "and nowhere else");
        assert_eq!(
            grid.at(5, 0),
            Some("#ff00ff"),
            "a mark bound to nothing shows by its colour"
        );
        assert_eq!(
            (grid.at(1, 3), grid.at(2, 3)),
            (Some("p2"), None),
            "a cut mark shows only its kept side"
        );
        let named = legend(&doc, &grid);
        let signs: Vec<(char, &str)> = named.iter().map(|n| (n.sign, n.key.as_str())).collect();
        assert_eq!(
            signs,
            [('i', "p1"), ('I', "p2"), ('a', "p3"), ('b', "#ff00ff")],
            "the first letter of the name, its capital when taken, then the next free sign; unused colours get none"
        );
        assert_eq!(rows(&grid, &named), [".iII.b", ".a....", "......", "II...."]);
        let text = plain(&grid, &named);
        assert_eq!(
            text,
            ". = empty\ni = ink\nI = iris\na = #ffffff\nb = #ff00ff\n\n.iII.b\n.a....\n......\nII...."
        );
        let ruled = ruled(&grid, &named);
        assert!(
            ruled.contains("  i ink #22223b p1") && ruled.contains(" 0  .iII.b"),
            "{ruled}"
        );
        let (read_legend, read_rows) = parse_text(&format!("\n{text}\n\n")).unwrap();
        assert_eq!(read_rows, rows(&grid, &named));
        let placed = placed(&read_rows, &read_legend, [10, 20]).unwrap();
        let drawn: Vec<(char, usize)> = placed.iter().map(|(sign, _, cells)| (*sign, cells.len())).collect();
        assert_eq!(drawn, [('i', 1), ('I', 4), ('b', 1), ('a', 1)]);
        assert_eq!(placed[0].2, [[11, 20]], "the grid lands where it is put");
        let erasing: BTreeMap<String, String> = [("x", "erase"), ("o", "ink")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let both = super::placed(&["xo.".to_string()], &erasing, [0, 0]).unwrap();
        assert!(matches!(both[0].1, Sign::Erase) && matches!(&both[1].1, Sign::Colour(c) if c == "ink"));
        let unnamed = super::placed(&["o?".to_string()], &erasing, [0, 0])
            .unwrap_err()
            .to_string();
        assert!(
            unnamed.contains("row 0, column 1") && unnamed.contains("'?'"),
            "{unnamed}"
        );
        assert!(
            parse_text("i = ink\n..ii..")
                .unwrap_err()
                .to_string()
                .contains("a blank line")
        );
        assert!(
            parse_text("ink\n\n..ii..")
                .unwrap_err()
                .to_string()
                .contains("not a legend line")
        );
        assert!(parse_text("i = ink\n\n\n").unwrap_err().to_string().contains("no rows"));
    }

    #[test]
    fn rectangles_ellipses_and_floods_are_whole_pixels() {
        assert_eq!(rect([2, 3], [4, 3], false).len(), 10);
        assert_eq!(rect([2, 3], [4, 3], true).len(), 12);
        assert_eq!(rect([0, 0], [1, 1], false), [[0, 0]]);
        assert!(rect([0, 0], [0, 5], true).is_empty());
        let ring = ellipse([0, 0], [7, 7], false);
        let has = |cells: &[Cell], x: i64, y: i64| cells.contains(&[x, y]);
        assert!(has(&ring, 3, 0) && has(&ring, 0, 3) && has(&ring, 6, 3) && has(&ring, 3, 6));
        assert!(
            !has(&ring, 0, 0) && !has(&ring, 3, 3),
            "no corners of the box, nothing inside"
        );
        assert!(
            ring.iter()
                .all(|c| ring.contains(&[6 - c[0], c[1]]) && ring.contains(&[c[0], 6 - c[1]]))
        );
        let even = ellipse([10, 20], [8, 4], false);
        assert!(
            even.iter()
                .all(|c| (10..18).contains(&c[0]) && (20..24).contains(&c[1])),
            "{even:?}"
        );
        assert!(
            even.iter()
                .all(|c| even.contains(&[27 - c[0], c[1]]) && even.contains(&[c[0], 43 - c[1]]))
        );
        let disc = ellipse([0, 0], [7, 7], true);
        assert!(has(&disc, 3, 3) && disc.len() > ring.len());
        assert!(ring.iter().all(|c| disc.contains(c)));
        assert_eq!(ellipse([5, 5], [1, 1], true), [[5, 5]]);
        assert_eq!(ellipse([5, 5], [3, 1], false).len(), 3);
        let grid = |rows: &[&str]| Grid {
            width: rows[0].len() as u32,
            height: rows.len() as u32,
            cells: rows
                .iter()
                .flat_map(|row| row.chars())
                .map(|c| (c != '.').then(|| c.to_string()))
                .collect(),
        };
        let walls = grid(&["..a..", ".a.a.", "..a..", "....."]);
        assert_eq!(
            flood(&walls, [2, 1], false),
            [[2, 1]],
            "four neighbours stop at a diamond"
        );
        assert_eq!(
            flood(&walls, [2, 1], true).len(),
            20 - 4,
            "eight slip out between its corners"
        );
        assert_eq!(
            flood(&walls, [2, 0], false),
            [[2, 0]],
            "a fill takes pixels of exactly the seed's colour"
        );
        assert_eq!(
            flood(&walls, [2, 0], true).len(),
            4,
            "and the diamond is joined across its corners"
        );
        assert_eq!(flood(&walls, [0, 0], false).len(), 15);
        assert!(flood(&walls, [9, 9], false).is_empty());
        let under = grid(&["bb...", ".....", ".....", "....."]);
        let both = merged(&[under, walls.clone()]).unwrap();
        assert_eq!(
            (both.at(0, 0), both.at(2, 0), both.at(4, 3)),
            (Some("b"), Some("a"), None)
        );
        assert!(merged(&[]).is_none());
    }
}
