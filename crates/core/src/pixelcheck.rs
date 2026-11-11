use crate::color::{Rgba, to_oklab};
use crate::pixel::Grid;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const ARM: u32 = 2;
const BAND: u32 = 4;
const STEP: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Stray,
    Corner,
    Jagged,
    Banding,
    OffPalette,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Stray, Kind::Corner, Kind::Jagged, Kind::Banding, Kind::OffPalette];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Stray => "stray pixel",
            Kind::Corner => "doubled corner",
            Kind::Jagged => "jagged line",
            Kind::Banding => "banding",
            Kind::OffPalette => "off the palette",
        }
    }

    pub fn advice(self) -> &'static str {
        match self {
            Kind::Stray => {
                "a single pixel with no neighbour of its colour: keep it if it is a detail, else join it to a cluster or remove it"
            }
            Kind::Corner => "an L in a one-pixel line, which reads as a thick spot: remove the corner pixel",
            Kind::Jagged => "a short run between longer ones breaks the line's rhythm: even out the run lengths",
            Kind::Banding => {
                "a one-pixel band that follows the edge next to it: break it up, or let the shade form its own shape"
            }
            Kind::OffPalette => {
                "pixels in a colour the palette does not hold: recolour them to a palette colour, or add the colour"
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    pub kind: Kind,
    pub at: [u32; 2],
    pub to: [u32; 2],
    pub key: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beside: Vec<String>,
    pub note: String,
}

type Cell = (i64, i64);

const AROUND: [Cell; 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

struct Field<'a> {
    grid: &'a Grid,
    wrap: bool,
}

impl<'a> Field<'a> {
    fn at(&self, (x, y): Cell) -> Option<&'a str> {
        let (w, h) = (self.grid.width as i64, self.grid.height as i64);
        let (x, y) = match self.wrap {
            true => (x.rem_euclid(w.max(1)), y.rem_euclid(h.max(1))),
            false => (x, y),
        };
        match x >= 0 && y >= 0 && x < w && y < h {
            true => self.grid.at(x as u32, y as u32),
            false => None,
        }
    }

    fn is(&self, cell: Cell, key: &str) -> bool {
        self.at(cell) == Some(key)
    }

    fn same(&self, (x, y): Cell, key: &str) -> Vec<Cell> {
        AROUND
            .iter()
            .filter(|(dx, dy)| self.is((x + dx, y + dy), key))
            .copied()
            .collect()
    }

    fn arm(&self, (x, y): Cell, (dx, dy): Cell, key: &str) -> u32 {
        let most = self.grid.width.max(self.grid.height) as i64;
        (1..=most)
            .take_while(|n| self.is((x + dx * n, y + dy * n), key))
            .count() as u32
    }

    fn cells(&self) -> impl Iterator<Item = (Cell, &'a str)> + '_ {
        let (w, h) = (self.grid.width as i64, self.grid.height as i64);
        (0..h)
            .flat_map(move |y| (0..w).map(move |x| (x, y)))
            .filter_map(|cell| self.at(cell).map(|key| (cell, key)))
    }
}

fn place((x, y): Cell) -> [u32; 2] {
    [x.max(0) as u32, y.max(0) as u32]
}

fn strays(field: &Field) -> Vec<Finding> {
    field
        .cells()
        .filter(|(cell, key)| field.same(*cell, key).is_empty())
        .map(|(cell, key)| Finding {
            kind: Kind::Stray,
            at: place(cell),
            to: place(cell),
            key: key.to_string(),
            beside: Vec::new(),
            note: "alone: no neighbour of its colour".into(),
        })
        .collect()
}

fn elbow(field: &Field, cell: Cell, key: &str) -> Option<[Cell; 2]> {
    const BESIDE: [Cell; 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    let arms: Vec<Cell> = BESIDE
        .iter()
        .filter(|(dx, dy)| field.is((cell.0 + dx, cell.1 + dy), key))
        .copied()
        .collect();
    let [a, b] = arms.as_slice() else {
        return None;
    };
    let square = a.0 * b.0 + a.1 * b.1 == 0;
    let inside = (cell.0 + a.0 + b.0, cell.1 + a.1 + b.1);
    let meant = field.arm(cell, *a, key) >= ARM && field.arm(cell, *b, key) >= ARM;
    (square && !field.is(inside, key) && !meant).then_some([(cell.0 + a.0, cell.1 + a.1), (cell.0 + b.0, cell.1 + b.1)])
}

fn corners(field: &Field) -> Vec<Finding> {
    field
        .cells()
        .filter(|(cell, key)| {
            let Some(arms) = elbow(field, *cell, key) else {
                return false;
            };
            let thin = field.same(*cell, key).len() <= 3;
            let alone = arms.iter().all(|arm| elbow(field, *arm, key).is_none());
            let goes_on = arms.iter().any(|arm| {
                field
                    .same(*arm, key)
                    .iter()
                    .map(|(dx, dy)| (arm.0 + dx, arm.1 + dy))
                    .any(|next| next != *cell && !arms.contains(&next))
            });
            thin && alone && goes_on
        })
        .map(|(cell, key)| Finding {
            kind: Kind::Corner,
            at: place(cell),
            to: place(cell),
            key: key.to_string(),
            beside: Vec::new(),
            note: "an L in a one-pixel line".into(),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lie {
    Across,
    Down,
    Dot,
}

#[derive(Clone, Copy, Debug)]
struct Stretch {
    lie: Lie,
    first: Cell,
    last: Cell,
    length: u32,
}

fn chains(field: &Field, key: &str, cells: &[Cell]) -> Vec<Vec<Cell>> {
    let thin: BTreeSet<Cell> = cells
        .iter()
        .filter(|cell| {
            let same = field.same(**cell, key);
            same.len() <= 2
        })
        .copied()
        .collect();
    let next_of = |cell: Cell, seen: &BTreeSet<Cell>| -> Option<Cell> {
        let mut around: Vec<Cell> = AROUND
            .iter()
            .map(|(dx, dy)| (cell.0 + dx, cell.1 + dy))
            .filter(|c| thin.contains(c) && !seen.contains(c))
            .collect();
        around.sort_by_key(|c| ((c.0 - cell.0).abs() + (c.1 - cell.1).abs(), c.1, c.0));
        around.first().copied()
    };
    let ends: Vec<Cell> = thin
        .iter()
        .filter(|cell| {
            AROUND
                .iter()
                .filter(|(dx, dy)| thin.contains(&(cell.0 + dx, cell.1 + dy)))
                .count()
                <= 1
        })
        .copied()
        .collect();
    let mut seen: BTreeSet<Cell> = BTreeSet::new();
    let mut made = Vec::new();
    ends.iter().chain(thin.iter()).for_each(|start| {
        if seen.contains(start) {
            return;
        }
        let mut chain = vec![*start];
        seen.insert(*start);
        while let Some(next) = next_of(*chain.last().unwrap_or(start), &seen) {
            seen.insert(next);
            chain.push(next);
        }
        made.push(chain);
    });
    made
}

fn stretches(chain: &[Cell]) -> Vec<Stretch> {
    let mut made: Vec<Stretch> = Vec::new();
    chain.iter().for_each(|cell| {
        let joins = made.last().map(|run| {
            let (dx, dy) = (cell.0 - run.last.0, cell.1 - run.last.1);
            match (run.lie, dx.abs(), dy.abs()) {
                (Lie::Dot | Lie::Across, 1, 0) => Some(Lie::Across),
                (Lie::Dot | Lie::Down, 0, 1) => Some(Lie::Down),
                _ => None,
            }
        });
        match (joins, made.last_mut()) {
            (Some(Some(lie)), Some(run)) => {
                run.lie = lie;
                run.last = *cell;
                run.length += 1;
            }
            _ => made.push(Stretch {
                lie: Lie::Dot,
                first: *cell,
                last: *cell,
                length: 1,
            }),
        }
    });
    made
}

fn jagged(field: &Field) -> Vec<Finding> {
    let mut by_key: BTreeMap<&str, Vec<Cell>> = BTreeMap::new();
    field
        .cells()
        .for_each(|(cell, key)| by_key.entry(key).or_default().push(cell));
    by_key
        .iter()
        .flat_map(|(key, cells)| {
            chains(field, key, cells)
                .into_iter()
                .flat_map(|chain| {
                    let runs = stretches(&chain);
                    runs.windows(3)
                        .filter(|w| {
                            let (a, b, c) = (w[0], w[1], w[2]);
                            let lies = a.lie == c.lie && a.lie != Lie::Dot && (b.lie == a.lie || b.lie == Lie::Dot);
                            let along = |run: &Stretch| match a.lie {
                                Lie::Down => (run.last.1 - run.first.1).signum(),
                                _ => (run.last.0 - run.first.0).signum(),
                            };
                            let aside = |from: &Stretch, to: &Stretch| match a.lie {
                                Lie::Down => (to.first.0 - from.last.0).signum(),
                                _ => (to.first.1 - from.last.1).signum(),
                            };
                            let onward = along(&a) == along(&c) && (b.length == 1 || along(&b) == along(&a));
                            let one_way = aside(&a, &b) == aside(&b, &c) && aside(&a, &b) != 0;
                            lies && onward && one_way && b.length + STEP <= a.length && b.length + STEP <= c.length
                        })
                        .map(|w| Finding {
                            kind: Kind::Jagged,
                            at: place((w[1].first.0.min(w[1].last.0), w[1].first.1.min(w[1].last.1))),
                            to: place((w[1].first.0.max(w[1].last.0), w[1].first.1.max(w[1].last.1))),
                            key: key.to_string(),
                            beside: Vec::new(),
                            note: format!(
                                "a run of {} between runs of {} and {}",
                                w[1].length, w[0].length, w[2].length
                            ),
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn banding(field: &Field, colour: &dyn Fn(&str) -> Option<Rgba>) -> Vec<Finding> {
    let light = |key: &str| colour(key).map(|c| to_oklab(c)[0]);
    let between = |a: &str, b: &str, c: &str| -> bool {
        match (light(a), light(b), light(c)) {
            (Some(la), Some(lb), Some(lc)) => (la < lb && lb < lc) || (la > lb && lb > lc),
            _ => false,
        }
    };
    let (w, h) = (field.grid.width as i64, field.grid.height as i64);
    let sides = |cell: Cell, down: bool| -> Option<(&str, &str, &str)> {
        let (before, after) = match down {
            true => ((cell.0 - 1, cell.1), (cell.0 + 1, cell.1)),
            false => ((cell.0, cell.1 - 1), (cell.0, cell.1 + 1)),
        };
        let (a, b, c) = (field.at(before)?, field.at(cell)?, field.at(after)?);
        (a != b && b != c && a != c).then_some((a, b, c))
    };
    let lines = |down: bool| -> Vec<Finding> {
        let (outer, inner) = match down {
            true => (w, h),
            false => (h, w),
        };
        (0..outer)
            .flat_map(|fixed| {
                let cell = move |moving: i64| match down {
                    true => (fixed, moving),
                    false => (moving, fixed),
                };
                let mut found: Vec<Finding> = Vec::new();
                let mut start = 0;
                while start < inner {
                    let Some(first) = sides(cell(start), down) else {
                        start += 1;
                        continue;
                    };
                    let length = (start..inner)
                        .take_while(|at| sides(cell(*at), down) == Some(first))
                        .count() as i64;
                    if length as u32 >= BAND && between(first.0, first.1, first.2) {
                        found.push(Finding {
                            kind: Kind::Banding,
                            at: place(cell(start)),
                            to: place(cell(start + length - 1)),
                            key: first.1.to_string(),
                            beside: vec![first.0.to_string(), first.2.to_string()],
                            note: format!("one pixel wide for {length} pixels"),
                        });
                    }
                    start += length;
                }
                found
            })
            .collect()
    };
    lines(false).into_iter().chain(lines(true)).collect()
}

fn off_palette(field: &Field) -> Vec<Finding> {
    let mut seen: BTreeMap<&str, (Cell, usize)> = BTreeMap::new();
    field
        .cells()
        .filter(|(_, key)| key.starts_with('#'))
        .for_each(|(cell, key)| seen.entry(key).or_insert((cell, 0)).1 += 1);
    seen.into_iter()
        .map(|(key, (first, count))| Finding {
            kind: Kind::OffPalette,
            at: place(first),
            to: place(first),
            key: key.to_string(),
            beside: Vec::new(),
            note: match count {
                1 => "1 pixel".to_string(),
                n => format!("{n} pixels, the first here"),
            },
        })
        .collect()
}

pub fn check(grid: &Grid, wrap: bool, colour: &dyn Fn(&str) -> Option<Rgba>) -> Vec<Finding> {
    let field = Field { grid, wrap };
    let mut found: Vec<Finding> = strays(&field)
        .into_iter()
        .chain(corners(&field))
        .chain(jagged(&field))
        .chain(banding(&field, colour))
        .chain(off_palette(&field))
        .collect();
    found.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(a.at[1].cmp(&b.at[1]))
            .then(a.at[0].cmp(&b.at[0]))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> Grid {
        Grid {
            width: rows[0].chars().count() as u32,
            height: rows.len() as u32,
            cells: rows
                .iter()
                .flat_map(|row| row.chars())
                .map(|c| match c {
                    '.' => None,
                    '?' => Some("#ff00ff".to_string()),
                    other => Some(other.to_string()),
                })
                .collect(),
        }
    }

    fn shade(key: &str) -> Option<Rgba> {
        let v = match key {
            "a" => 0.1,
            "b" => 0.4,
            "c" => 0.8,
            "i" => 0.0,
            _ => 0.6,
        };
        Some([v, v, v, 1.0])
    }

    fn found(rows: &[&str], kind: Kind) -> Vec<([u32; 2], [u32; 2])> {
        check(&grid(rows), false, &shade)
            .into_iter()
            .filter(|f| f.kind == kind)
            .map(|f| (f.at, f.to))
            .collect()
    }

    #[test]
    fn a_pixel_with_no_neighbour_of_its_colour_is_stray() {
        let rows = ["i....", ".bb..", ".bb.i", "....."];
        assert_eq!(found(&rows, Kind::Stray), [([0, 0], [0, 0]), ([4, 2], [4, 2])]);
        assert!(found(&["ii", "ii"], Kind::Stray).is_empty());
        assert_eq!(
            check(&grid(&["i..", "...", "..i"]), true, &shade)
                .iter()
                .filter(|f| f.kind == Kind::Stray)
                .count(),
            0,
            "on a canvas that wraps the two corners touch"
        );
    }

    #[test]
    fn an_l_in_a_thin_line_is_a_doubled_corner_but_a_box_corner_is_not() {
        let slip = ["i....", ".ii..", "..i..", "...i."];
        assert_eq!(found(&slip, Kind::Corner), [([2, 1], [2, 1])]);
        let clean = ["i....", ".i...", "..i..", "...i."];
        assert!(found(&clean, Kind::Corner).is_empty());
        let stairs = ["ii...", ".ii..", "..ii.", "...ii"];
        assert!(
            found(&stairs, Kind::Corner).is_empty(),
            "steps all the way are a diagonal two pixels wide, not a slip"
        );
        let boxed = ["iiiii", "i...i", "i...i", "iiiii"];
        assert!(
            found(&boxed, Kind::Corner).is_empty(),
            "arms of two or more both ways are a corner that is meant"
        );
        let small = ["iiii", "i..i", "iiii"];
        assert!(found(&small, Kind::Corner).is_empty(), "also on a small box");
        let filled = ["iii", "iii"];
        assert!(found(&filled, Kind::Corner).is_empty());
        let hook = ["iiiii", "....i", "....."];
        assert_eq!(
            found(&hook, Kind::Corner),
            [([4, 0], [4, 0])],
            "a short arm is not a box"
        );
        let blob = ["ii.", "i..", "..."];
        assert!(
            found(&blob, Kind::Corner).is_empty(),
            "three pixels are a cluster, not a line"
        );
        let wedge = ["..iii", "iii..", "ii...", "i...."];
        assert!(
            found(&wedge, Kind::Corner).is_empty(),
            "the stepped edge of a shape is not a line"
        );
    }

    #[test]
    fn a_short_run_between_longer_ones_is_jagged() {
        let jag = ["iiii.........", "....i........", ".....iiii....", "............."];
        assert_eq!(found(&jag, Kind::Jagged), [([4, 1], [4, 1])]);
        let note = check(&grid(&jag), false, &shade)
            .into_iter()
            .find(|f| f.kind == Kind::Jagged)
            .unwrap()
            .note;
        assert_eq!(note, "a run of 1 between runs of 4 and 4");
        let even = ["iii......", "...iii...", "......iii"];
        assert!(found(&even, Kind::Jagged).is_empty());
        let arc = ["i........", ".i.......", "..ii.....", "....iiii.", "........i"];
        assert!(
            found(&arc, Kind::Jagged).is_empty(),
            "runs that grow along a curve are fine"
        );
        let steep = ["i..", "i..", "i..", "i..", ".i.", "..i", "..i", "..i", "..i"];
        assert_eq!(
            found(&steep, Kind::Jagged),
            [([1, 4], [1, 4])],
            "the same down a column"
        );
        let bump = ["iiii.iiii", "....i...."];
        assert!(
            found(&bump, Kind::Jagged).is_empty(),
            "a notch that comes back is not a step"
        );
    }

    #[test]
    fn a_one_pixel_band_between_a_darker_and_a_lighter_colour_is_banding() {
        let bands = ["aaaaaa", "bbbbbb", "cccccc"];
        assert_eq!(found(&bands, Kind::Banding), [([0, 1], [5, 1])]);
        let wide = ["aaaaaa", "bbbbbb", "bbbbbb", "cccccc"];
        assert!(
            found(&wide, Kind::Banding).is_empty(),
            "two pixels wide is a shape of its own"
        );
        let line = ["cccccc", "iiiiii", "cccccc"];
        assert!(
            found(&line, Kind::Banding).is_empty(),
            "a line through one colour is a detail"
        );
        let dark = ["bbbbbb", "iiiiii", "cccccc"];
        assert!(
            found(&dark, Kind::Banding).is_empty(),
            "darker than both sides is a line, not a step"
        );
        let short = ["aaa", "bbb", "ccc"];
        assert!(found(&short, Kind::Banding).is_empty());
        let edge = ["......", "bbbbbb", "cccccc"];
        assert!(
            found(&edge, Kind::Banding).is_empty(),
            "an outline next to nothing is not banding"
        );
        let down = ["abc", "abc", "abc", "abc"];
        assert_eq!(found(&down, Kind::Banding), [([1, 0], [1, 3])]);
    }

    #[test]
    fn colours_that_are_not_palette_entries_are_listed_once_with_their_count() {
        let rows = ["bb?", "b??", "bbb"];
        let all = check(&grid(&rows), false, &shade);
        let off: Vec<&Finding> = all.iter().filter(|f| f.kind == Kind::OffPalette).collect();
        assert_eq!(off.len(), 1);
        assert_eq!(
            (off[0].at, off[0].key.as_str(), off[0].note.as_str()),
            ([2, 0], "#ff00ff", "3 pixels, the first here")
        );
        assert!(
            check(&grid(&["bb", "bb"]), false, &shade).is_empty(),
            "a clean drawing has no findings"
        );
        assert_eq!(Kind::ALL.len(), 5);
        assert!(Kind::ALL.iter().all(|k| !k.name().is_empty() && !k.advice().is_empty()));
    }
}
