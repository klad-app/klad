use crate::Rasters;
use half::f16;
use klad_brush::Raster;
use klad_brush::tiles::TILE_PX;
use klad_core::doc::{Document, Kind, Layer};
use klad_core::timing::ends;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

pub type Onions = BTreeMap<String, Raster>;

pub const BEFORE: [f32; 3] = [0.88, 0.16, 0.16];
pub const AFTER: [f32; 3] = [0.1, 0.62, 0.24];
const TINT: f32 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Drawings,
    Frames,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Onion {
    pub before: u32,
    pub after: u32,
    pub by: By,
    pub opacity: f32,
    pub falloff: f32,
    pub tint: bool,
    pub wrap: Option<(u32, u32)>,
}

impl Default for Onion {
    fn default() -> Self {
        Self {
            before: 2,
            after: 2,
            by: By::Drawings,
            opacity: 0.5,
            falloff: 0.6,
            tint: true,
            wrap: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    pub cel: String,
    pub step: i32,
    pub alpha: f32,
    pub tint: Option<[f32; 3]>,
}

struct Exposure {
    start: u32,
    content: String,
}

fn exposures(layer: &Layer, from: u32, to: u32) -> Vec<Exposure> {
    let cels = layer.cels();
    cels.iter()
        .zip(ends(cels))
        .filter(|(c, end)| c.frame <= to && *end > from)
        .map(|(c, _)| Exposure {
            start: c.frame.max(from),
            content: layer.content_of(c).id.clone(),
        })
        .collect()
}

fn window(onion: &Onion, frame: u32, frames: u32) -> (u32, u32, bool) {
    match onion.wrap {
        Some((a, b)) if a <= frame && frame <= b && b < frames => (a, b, true),
        _ => (0, frames.saturating_sub(1), false),
    }
}

fn by_drawings(layer: &Layer, frame: u32, onion: &Onion, (from, to, wraps): (u32, u32, bool)) -> Vec<(i32, String)> {
    let list = exposures(layer, from, to);
    let current = layer.cel_at(frame).map_or(frame, |c| c.frame.max(from));
    let earlier: Vec<&Exposure> = list.iter().filter(|e| e.start < current).rev().collect();
    let later: Vec<&Exposure> = list.iter().filter(|e| e.start > frame).collect();
    let (before, after): (Vec<&Exposure>, Vec<&Exposure>) = match wraps {
        true => (
            earlier.iter().chain(later.iter().rev()).copied().collect(),
            later.iter().chain(earlier.iter().rev()).copied().collect(),
        ),
        false => (earlier, later),
    };
    let side = |list: Vec<&Exposure>, count: u32, sign: i32| -> Vec<(i32, String)> {
        list.into_iter()
            .take(count as usize)
            .enumerate()
            .map(|(k, e)| (sign * (k as i32 + 1), e.content.clone()))
            .collect()
    };
    let mut out = side(before, onion.before, -1);
    out.extend(side(after, onion.after, 1));
    out
}

fn by_frames(layer: &Layer, frame: u32, onion: &Onion, (from, to, wraps): (u32, u32, bool)) -> Vec<(i32, String)> {
    let len = (to - from + 1) as i64;
    let at = |k: i64| -> Option<u32> {
        let f = frame as i64 + k;
        match wraps {
            true => Some((from as i64 + (f - from as i64).rem_euclid(len)) as u32),
            false => (from as i64..=to as i64).contains(&f).then_some(f as u32),
        }
    };
    let side = |count: u32, sign: i64| -> Vec<(i32, String)> {
        (1..=count as i64)
            .map_while(|k| at(sign * k).map(|f| (k, f)))
            .filter_map(|(k, f)| layer.content_at(f).map(|c| ((sign * k) as i32, c.id.clone())))
            .collect()
    };
    let mut out = side(onion.before, -1);
    out.extend(side(onion.after, 1));
    out
}

pub fn ghosts(layer: &Layer, frame: u32, frames: u32, onion: &Onion) -> Vec<Ghost> {
    if layer.kind() == Kind::Group || frames == 0 {
        return Vec::new();
    }
    let span = window(onion, frame, frames);
    let steps = match onion.by {
        By::Drawings => by_drawings(layer, frame, onion, span),
        By::Frames => by_frames(layer, frame, onion, span),
    };
    let current = layer.content_at(frame).map(|c| c.id.clone());
    let mut nearest: Vec<(i32, String)> = steps
        .into_iter()
        .filter(|(_, cel)| Some(cel) != current.as_ref())
        .collect();
    nearest.sort_by_key(|(step, _)| (step.abs(), *step > 0));
    let mut seen = BTreeSet::new();
    let mut out: Vec<Ghost> = nearest
        .into_iter()
        .filter(|(_, cel)| seen.insert(cel.clone()))
        .map(|(step, cel)| Ghost {
            alpha: onion.opacity * onion.falloff.powi(step.abs() - 1),
            tint: onion.tint.then_some(if step < 0 { BEFORE } else { AFTER }),
            cel,
            step,
        })
        .collect();
    out.reverse();
    out
}

pub fn animated(doc: &Document) -> Vec<String> {
    doc.all_layers()
        .into_iter()
        .filter(|l| matches!(l.kind(), Kind::Paint | Kind::Vector) && !doc.is_mask(&l.id))
        .filter(|l| {
            l.cels()
                .iter()
                .map(|c| &l.content_of(c).id)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        })
        .map(|l| l.id.clone())
        .collect()
}

pub fn plan(doc: &Document, frame: u32, layers: &[String], onion: &Onion) -> Vec<(String, Vec<Ghost>)> {
    layers
        .iter()
        .filter_map(|id| doc.layer(id))
        .filter(|l| l.visible)
        .map(|l| (l.id.clone(), ghosts(l, frame, doc.timeline.frames, onion)))
        .filter(|(_, g)| !g.is_empty())
        .collect()
}

pub fn needed(doc: &Document, plan: &[(String, Vec<Ghost>)]) -> Vec<(String, String, Kind)> {
    plan.iter()
        .filter_map(|(layer, ghosts)| doc.layer(layer).map(|l| (l, ghosts)))
        .flat_map(|(l, ghosts)| ghosts.iter().map(move |g| (l.id.clone(), g.cel.clone(), l.kind())))
        .collect()
}

fn mix(acc: &mut [f32], tile: &[f16], alpha: f32, tint: Option<[f32; 3]>) {
    acc.as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(tile.as_chunks::<4>().0)
        .for_each(|(d, s)| {
            let a = s[3].to_f32();
            if a <= 0.0 {
                return;
            }
            let rgb = [0, 1, 2].map(|c| {
                let v = s[c].to_f32();
                match tint {
                    Some(t) => v * (1.0 - TINT) + t[c] * a * TINT,
                    None => v,
                }
            });
            let src = [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, a * alpha];
            let keep = 1.0 - src[3];
            (0..4).for_each(|c| d[c] = src[c] + d[c] * keep);
        });
}

pub fn paint(ghosts: &[Ghost], rasters: &Rasters, width: u32, height: u32) -> Raster {
    let mut out = Raster::new(width, height);
    let sources: Vec<(&Raster, &Ghost)> = ghosts
        .iter()
        .filter_map(|g| rasters.get(&g.cel).map(|r| (r, g)))
        .collect();
    let tiles: BTreeSet<usize> = sources.iter().flat_map(|(r, _)| r.occupied().map(|(i, _)| i)).collect();
    let done: Vec<(usize, Vec<f16>)> = tiles
        .into_par_iter()
        .map(|index| {
            let mut acc = vec![0.0f32; TILE_PX * 4];
            sources.iter().for_each(|(r, g)| {
                if let Some(tile) = r.at(index) {
                    mix(&mut acc, tile, g.alpha, g.tint);
                }
            });
            (index, acc.into_iter().map(f16::from_f32).collect())
        })
        .collect();
    done.into_iter()
        .filter(|(_, t)| t.as_chunks::<4>().0.iter().any(|px| px[3].to_f32() > 0.0))
        .for_each(|(i, t)| out.put(i, t.into_boxed_slice()));
    out
}

pub fn build(doc: &Document, plan: &[(String, Vec<Ghost>)], rasters: &Rasters) -> Onions {
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    plan.iter()
        .map(|(layer, ghosts)| (layer.clone(), paint(ghosts, rasters, w, h)))
        .filter(|(_, r)| r.count() > 0)
        .collect()
}

pub fn plan_hash(plan: &[(String, Vec<Ghost>)], version: impl Fn(&str) -> u64) -> u64 {
    use klad_core::rng::{FNV_OFFSET, fnv};
    plan.iter().fold(FNV_OFFSET, |h, (layer, ghosts)| {
        ghosts.iter().fold(fnv(h, layer.as_bytes()), |h, g| {
            let tint = g.tint.unwrap_or([-1.0; 3]);
            let numbers = [g.alpha, tint[0], tint[1], tint[2]].map(f32::to_le_bytes).concat();
            fnv(fnv(fnv(h, g.cel.as_bytes()), &numbers), &version(&g.cel).to_le_bytes())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Cel;

    fn layer(cels: Vec<Cel>) -> Layer {
        let mut l = Layer::paint("l", "l", "x");
        *l.cels_mut().unwrap() = cels;
        l
    }

    fn held(id: &str, frame: u32, hold: u32) -> Cel {
        Cel {
            hold: Some(hold),
            ..Cel::at(id, frame)
        }
    }

    fn link(id: &str, frame: u32, to: &str) -> Cel {
        Cel {
            link: Some(to.into()),
            ..Cel::at(id, frame)
        }
    }

    fn steps(g: &[Ghost]) -> Vec<(i32, &str)> {
        g.iter().map(|g| (g.step, g.cel.as_str())).collect()
    }

    #[test]
    fn drawings_before_and_after_skip_holds_links_to_the_current_and_repeats() {
        let l = layer(vec![
            held("a", 0, 2),
            held("b", 2, 2),
            held("c", 4, 2),
            held("d", 6, 2),
            link("e", 8, "b"),
        ]);
        let onion = Onion::default();
        assert_eq!(steps(&ghosts(&l, 4, 10, &onion)), [(-2, "a"), (1, "d"), (-1, "b")]);
        assert_eq!(steps(&ghosts(&l, 5, 10, &onion)), steps(&ghosts(&l, 4, 10, &onion)));
        assert_eq!(steps(&ghosts(&l, 2, 10, &onion)), [(2, "d"), (1, "c"), (-1, "a")]);
        let g = ghosts(&l, 4, 10, &onion);
        assert_eq!((g[2].alpha, g[2].tint), (0.5, Some(BEFORE)));
        assert!((g[0].alpha - 0.3).abs() < 1e-6);
        assert_eq!(g[1].tint, Some(AFTER));
    }

    #[test]
    fn frames_count_distance_in_frames_and_blanks_show_neighbours() {
        let l = layer(vec![held("a", 0, 2), held("b", 2, 2), held("c", 6, 2)]);
        let onion = Onion {
            by: By::Frames,
            before: 3,
            after: 3,
            tint: false,
            ..Onion::default()
        };
        assert_eq!(steps(&ghosts(&l, 2, 8, &onion)), [(-1, "a")]);
        assert_eq!(steps(&ghosts(&l, 3, 8, &onion)), [(3, "c"), (-2, "a")]);
        assert_eq!(steps(&ghosts(&l, 4, 8, &onion)), [(-3, "a"), (2, "c"), (-1, "b")]);
        assert!(ghosts(&l, 4, 8, &onion).iter().all(|g| g.tint.is_none()));
        let drawings = Onion {
            tint: false,
            ..Onion::default()
        };
        assert_eq!(steps(&ghosts(&l, 4, 8, &drawings)), [(-2, "a"), (1, "c"), (-1, "b")]);
    }

    #[test]
    fn a_looping_range_wraps_the_onion() {
        let l = layer(vec![
            held("a", 0, 2),
            held("b", 2, 2),
            held("c", 4, 2),
            held("d", 6, 2),
            held("z", 8, 4),
        ]);
        let wrap = Onion {
            wrap: Some((0, 7)),
            before: 1,
            after: 1,
            ..Onion::default()
        };
        assert_eq!(steps(&ghosts(&l, 6, 12, &wrap)), [(1, "a"), (-1, "c")]);
        assert_eq!(steps(&ghosts(&l, 0, 12, &wrap)), [(1, "b"), (-1, "d")]);
        assert_eq!(steps(&ghosts(&l, 9, 12, &wrap)), [(-1, "d")]);
        let frames = Onion {
            by: By::Frames,
            before: 2,
            after: 2,
            ..wrap.clone()
        };
        assert_eq!(steps(&ghosts(&l, 7, 12, &frames)), [(-2, "c"), (1, "a")]);
        let unwrapped = Onion { wrap: None, ..wrap };
        assert_eq!(steps(&ghosts(&l, 6, 12, &unwrapped)), [(1, "z"), (-1, "c")]);
    }

    #[test]
    fn painted_ghosts_tint_fade_and_stack_furthest_first() {
        let mut solid = Raster::new(256, 256);
        let px = [0.2, 0.4, 0.6, 1.0].map(f16::from_f32);
        solid.put(
            0,
            px.iter()
                .copied()
                .cycle()
                .take(TILE_PX * 4)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let rasters = Rasters::from([("a".to_string(), solid.clone()), ("b".to_string(), solid)]);
        let plain = paint(
            &[Ghost {
                cel: "a".into(),
                step: -1,
                alpha: 0.5,
                tint: None,
            }],
            &rasters,
            256,
            256,
        );
        let got: Vec<f32> = plain.at(0).unwrap()[..4].iter().map(|v| v.to_f32()).collect();
        assert!((got[0] - 0.1).abs() < 1e-3 && (got[3] - 0.5).abs() < 1e-3, "{got:?}");
        let tinted = paint(
            &[
                Ghost {
                    cel: "a".into(),
                    step: -2,
                    alpha: 0.25,
                    tint: Some(BEFORE),
                },
                Ghost {
                    cel: "b".into(),
                    step: 1,
                    alpha: 0.5,
                    tint: Some(AFTER),
                },
            ],
            &rasters,
            256,
            256,
        );
        let got: Vec<f32> = tinted.at(0).unwrap()[..4].iter().map(|v| v.to_f32()).collect();
        assert!(got[1] > got[0], "the nearer green ghost is on top: {got:?}");
        assert!((got[3] - (0.5 + 0.25 * 0.5)).abs() < 1e-3, "{got:?}");
        assert_eq!(paint(&[], &rasters, 256, 256).count(), 0);
    }
}
