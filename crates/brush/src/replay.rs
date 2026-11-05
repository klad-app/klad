use crate::brush::{BrushSet, StrokeBlend};
use crate::engine::{commit, draw_stroke_on, land_stroke};
use crate::fill::{fill_tiles, paint_fill_where};
use crate::tiles::{Raster, StrokeBuffer, TILE, TILE_PX, Tile};
use anyhow::Result;
use half::f16;
use klad_core::brushes::Curve;
use klad_core::stroke::{Mark, Sample, Stroke, fingerprint};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const EVERY: usize = 128;
const TILED_FROM: usize = 32;
pub const BUDGET: usize = 128 << 20;

#[derive(Clone)]
struct Checkpoint {
    at: usize,
    tiles: Vec<Tile>,
}

#[derive(Clone)]
pub struct CelReplay {
    width: u32,
    height: u32,
    marks: Vec<Mark>,
    keys: Vec<(u32, u64)>,
    footprints: Vec<Vec<u32>>,
    checkpoints: Vec<Checkpoint>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Updated {
    pub tiles: Vec<usize>,
    pub replayed: usize,
    pub full: bool,
}

fn painted(
    raster: &mut Raster,
    buffer: &mut StrokeBuffer,
    brushes: &BrushSet,
    mark: &Mark,
) -> Result<(Vec<usize>, usize)> {
    Ok(match mark {
        Mark::Stroke(stroke) => {
            let brush = brushes.get(&stroke.brush)?;
            let dabs = draw_stroke_on(buffer, brush, stroke, Some(raster));
            (land_stroke(raster, buffer, stroke, brush), dabs)
        }
        Mark::Fill(fill) => (paint_fill_where(raster, fill, |i| buffer.allowed(i)), 0),
    })
}

fn reach(stroke: &Stroke, brushes: &BrushSet) -> Option<f32> {
    let def = brushes.def(&stroke.brush)?;
    let most = |c: Option<Curve>| c.map_or(1.0, |c| c.min.abs().max(c.max.abs()));
    let size = &def.dynamics.size;
    let radius =
        0.5 * stroke.size * most(size.pressure) * most(size.tilt) * most(size.speed) * (1.0 + def.jitter.size.abs());
    Some((radius.max(0.6) * (1.0 + 2.0 * def.jitter.scatter.abs()) + 2.0) * 1.25)
}

fn guess(raster: &Raster, brushes: &BrushSet, mark: &Mark) -> Vec<usize> {
    let (cols, rows) = (raster.cols, raster.rows);
    let span = |a: f32, b: f32, n: usize| {
        let last = n as f32 * TILE as f32 - 1.0;
        ((a.clamp(0.0, last) as usize) / TILE)..=((b.clamp(0.0, last) as usize) / TILE)
    };
    match mark {
        Mark::Fill(fill) => crate::fill::fill_tiles(raster, fill),
        Mark::Stroke(stroke) => {
            let points: Vec<(f32, f32)> = stroke.samples.iter().map(Sample::pos).collect();
            let Some(pad) = reach(stroke, brushes).filter(|_| !points.is_empty()) else {
                return (0..cols * rows).collect();
            };
            let (x0, x1) = points
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
            let (y0, y1) = points
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            span(y0 - pad, y1 + pad, rows)
                .flat_map(|ty| span(x0 - pad, x1 + pad, cols).map(move |tx| ty * cols + tx))
                .collect()
        }
    }
}

type Tiled = (Raster, Vec<Vec<u32>>, usize);
type TileJob = (usize, Tile, Vec<(usize, Vec<usize>, usize)>);

fn tiled(width: u32, height: u32, marks: &[Mark], brushes: &BrushSet) -> Result<Option<Tiled>> {
    let mut raster = Raster::new(width, height);
    let tiles = raster.cols * raster.rows;
    let reads = marks.iter().any(|m| match m {
        Mark::Stroke(stroke) => brushes.def(&stroke.brush).is_none_or(|d| d.reads_pixels()),
        Mark::Fill(_) => false,
    });
    if reads || tiles < 2 || marks.len() < TILED_FROM {
        return Ok(None);
    }
    let guesses: Vec<Vec<usize>> = marks.iter().map(|m| guess(&raster, brushes, m)).collect();
    let mut jobs: Vec<Vec<usize>> = vec![Vec::new(); tiles];
    guesses
        .iter()
        .enumerate()
        .for_each(|(i, g)| g.iter().for_each(|&t| jobs[t].push(i)));
    let done: Vec<TileJob> = jobs
        .par_iter()
        .enumerate()
        .filter(|(_, job)| !job.is_empty())
        .map(|(t, job)| {
            let mut own = Raster::new(width, height);
            let mut buffer = StrokeBuffer::new(width, height);
            let mut only = vec![false; tiles];
            only[t] = true;
            buffer.set_mask(Some(only));
            let seen = job
                .iter()
                .map(|&i| {
                    let (footprint, dabs) = painted(&mut own, &mut buffer, brushes, &marks[i])?;
                    Ok((i, footprint, dabs))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((t, own.tile(t), seen))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut footprints: Vec<Option<Vec<u32>>> = vec![None; marks.len()];
    let mut fills: Vec<Vec<u32>> = vec![Vec::new(); marks.len()];
    let mut dabs = 0;
    done.iter().for_each(|(_, _, seen)| {
        seen.iter().for_each(|(i, footprint, n)| match &marks[*i] {
            Mark::Fill(_) => fills[*i].extend(footprint.iter().map(|&t| t as u32)),
            Mark::Stroke(_) => {
                if footprints[*i].is_none() {
                    footprints[*i] = Some(footprint.iter().map(|&t| t as u32).collect());
                    dabs += n;
                }
            }
        })
    });
    let footprints: Vec<Vec<u32>> = marks
        .iter()
        .enumerate()
        .map(|(i, m)| match m {
            Mark::Fill(_) => {
                let mut f = std::mem::take(&mut fills[i]);
                f.sort_unstable();
                f
            }
            Mark::Stroke(_) => footprints[i].take().unwrap_or_default(),
        })
        .collect();
    let guessed = footprints
        .iter()
        .zip(&guesses)
        .all(|(f, g)| f.iter().all(|t| g.contains(&(*t as usize))));
    if !guessed {
        return Ok(None);
    }
    done.iter().for_each(|(t, tile, _)| put(&mut raster, *t, tile));
    Ok(Some((raster, footprints, dabs)))
}

fn put(raster: &mut Raster, index: usize, tile: &Tile) {
    raster.restore(index, tile);
}

impl CelReplay {
    pub fn new(width: u32, height: u32) -> Self {
        let tiles = Raster::new(width, height);
        Self {
            width,
            height,
            marks: Vec::new(),
            keys: Vec::new(),
            footprints: Vec::new(),
            checkpoints: vec![Checkpoint {
                at: 0,
                tiles: vec![(None, None); tiles.cols * tiles.rows],
            }],
        }
    }

    pub fn replay(width: u32, height: u32, marks: Vec<Mark>, brushes: &BrushSet) -> Result<(Self, Raster, usize)> {
        let mut cel = Self::new(width, height);
        let mut raster = Raster::new(width, height);
        let mut buffer = StrokeBuffer::new(width, height);
        let dabs = marks.into_iter().try_fold(0usize, |dabs, mark| {
            let (footprint, more) = painted(&mut raster, &mut buffer, brushes, &mark)?;
            cel.record(&raster, mark, &footprint);
            Ok::<_, anyhow::Error>(dabs + more)
        })?;
        Ok((cel, raster, dabs))
    }

    pub fn replay_tiled(
        width: u32,
        height: u32,
        marks: Vec<Mark>,
        brushes: &BrushSet,
    ) -> Result<(Self, Raster, usize)> {
        match tiled(width, height, &marks, brushes)? {
            Some((raster, footprints, dabs)) => {
                let mut cel = Self::new(width, height);
                cel.keys = marks.iter().map(|m| (m.id(), fingerprint(m))).collect();
                cel.footprints = footprints;
                cel.marks = marks;
                Ok((cel, raster, dabs))
            }
            None => Self::replay(width, height, marks, brushes),
        }
    }

    pub fn from_cache(width: u32, height: u32, keys: Vec<(u32, u64)>, footprints: Vec<Vec<u32>>) -> Self {
        Self {
            keys,
            footprints,
            ..Self::new(width, height)
        }
    }

    pub fn with_marks(mut self, marks: Vec<Mark>) -> Option<Self> {
        let same = marks.len() == self.keys.len()
            && marks
                .iter()
                .zip(&self.keys)
                .all(|(m, k)| (m.id(), fingerprint(m)) == *k)
            && self.footprints.len() == self.keys.len();
        same.then(|| {
            self.marks = marks;
            self
        })
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    pub fn keys(&self) -> &[(u32, u64)] {
        &self.keys
    }

    pub fn footprints(&self) -> &[Vec<u32>] {
        &self.footprints
    }

    pub fn checkpoints(&self) -> Vec<usize> {
        self.checkpoints.iter().map(|c| c.at).collect()
    }

    fn record(&mut self, raster: &Raster, mark: Mark, footprint: &[usize]) {
        self.keys.push((mark.id(), fingerprint(&mark)));
        self.footprints.push(footprint.iter().map(|&t| t as u32).collect());
        self.marks.push(mark);
        self.maybe_checkpoint(raster);
    }

    fn maybe_checkpoint(&mut self, raster: &Raster) {
        let Some(last) = self.checkpoints.last() else {
            return;
        };
        if self.keys.len() < last.at + EVERY {
            return;
        }
        let changed: BTreeSet<u32> = self.footprints[last.at..].iter().flatten().copied().collect();
        let mut tiles = last.tiles.clone();
        changed
            .iter()
            .for_each(|&t| tiles[t as usize] = raster.tile(t as usize));
        self.checkpoints.push(Checkpoint {
            at: self.keys.len(),
            tiles,
        });
        while self.checkpoint_bytes() > BUDGET && self.checkpoints.len() > 2 {
            let mut n = 0;
            self.checkpoints.retain(|_| {
                n += 1;
                n == 1 || n % 2 == 0
            });
        }
    }

    pub fn drop_checkpoints(&mut self) {
        self.checkpoints.truncate(1);
    }

    pub fn checkpoint_bytes(&self) -> usize {
        let unique: BTreeSet<(usize, usize)> = self
            .checkpoints
            .iter()
            .flat_map(|c| c.tiles.iter())
            .flat_map(|(colour, height)| {
                let colour = colour.as_ref().map(|t| (Arc::as_ptr(t) as *const f16 as usize, 4));
                let height = height.as_ref().map(|t| (Arc::as_ptr(t) as *const f16 as usize, 1));
                colour.into_iter().chain(height)
            })
            .collect();
        unique.iter().map(|(_, c)| c).sum::<usize>() * TILE_PX * std::mem::size_of::<f16>()
    }

    pub fn commit_stroke(
        &mut self,
        raster: &mut Raster,
        stroke: Stroke,
        buffer: &mut StrokeBuffer,
        blend: StrokeBlend,
    ) -> Vec<usize> {
        let touched = commit(raster, buffer, stroke.color(), stroke.opacity, blend);
        self.record(raster, Mark::Stroke(stroke), &touched);
        touched
    }

    pub fn paint(&mut self, raster: &mut Raster, mark: Mark, brushes: &BrushSet) -> Result<Vec<usize>> {
        let mut buffer = StrokeBuffer::new(self.width, self.height);
        let (footprint, _) = painted(raster, &mut buffer, brushes, &mark)?;
        self.record(raster, mark, &footprint);
        Ok(footprint)
    }

    fn rebuild(&mut self, raster: &mut Raster, next: Vec<Mark>, brushes: &BrushSet) -> Result<Updated> {
        let before: BTreeSet<usize> = raster.occupied().map(|(i, _)| i).collect();
        let replayed = next.len();
        let (cel, fresh, _) = Self::replay(self.width, self.height, next, brushes)?;
        let tiles = before
            .into_iter()
            .chain(fresh.occupied().map(|(i, _)| i))
            .collect::<BTreeSet<_>>();
        *self = cel;
        *raster = fresh;
        Ok(Updated {
            tiles: tiles.into_iter().collect(),
            replayed,
            full: true,
        })
    }

    pub fn update(&mut self, raster: &mut Raster, next: Vec<Mark>, brushes: &BrushSet) -> Result<Updated> {
        let keys: Vec<(u32, u64)> = next.iter().map(|m| (m.id(), fingerprint(m))).collect();
        let old: BTreeMap<(u32, u64), usize> = self.keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
        let kept: Vec<Option<usize>> = keys.iter().map(|k| old.get(k).copied()).collect();
        let unchanged = kept.iter().enumerate().take_while(|(i, k)| **k == Some(*i)).count();
        let reads = next[unchanged..].iter().any(|m| match m {
            Mark::Stroke(stroke) => brushes.def(&stroke.brush).is_some_and(|d| d.reads_pixels()),
            Mark::Fill(_) => false,
        });
        if reads || !kept.iter().flatten().is_sorted_by(|a, b| a < b) || self.footprints.len() != self.keys.len() {
            return self.rebuild(raster, next, brushes);
        }
        let mut kept_old = vec![false; self.keys.len()];
        kept.iter().flatten().for_each(|&j| kept_old[j] = true);
        let prefix = kept.iter().enumerate().take_while(|(i, k)| **k == Some(*i)).count();
        let base = self.checkpoints.iter().rposition(|c| c.at <= prefix).unwrap_or(0);
        let start = self.checkpoints[base].at;
        let tiles = raster.cols * raster.rows;
        let mut dirty = vec![false; tiles];
        self.footprints
            .iter()
            .zip(&kept_old)
            .filter(|(_, kept)| !**kept)
            .flat_map(|(f, _)| f)
            .for_each(|&t| dirty[t as usize] = true);
        let origin = self.checkpoints[base].tiles.clone();
        (0..tiles)
            .filter(|&t| dirty[t])
            .for_each(|t| put(raster, t, &origin[t]));
        let positions: Vec<usize> = kept
            .iter()
            .enumerate()
            .filter(|(_, k)| k.is_some())
            .map(|(i, _)| i)
            .collect();
        let mut later = self.checkpoints[base + 1..]
            .iter()
            .map(|c| {
                let before = kept_old[..c.at.min(kept_old.len())].iter().filter(|k| **k).count();
                let at = if before == 0 { 0 } else { positions[before - 1] + 1 };
                (at, c.tiles.clone())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .peekable();
        let mut snapshots: Vec<Checkpoint> = Vec::new();
        let snapshot = |at: usize, old: Vec<Tile>, dirty: &[bool], raster: &Raster, out: &mut Vec<Checkpoint>| {
            if out.last().is_some_and(|c| c.at == at) {
                return;
            }
            let mut tiles = old;
            (0..dirty.len())
                .filter(|&t| dirty[t])
                .for_each(|t| tiles[t] = raster.tile(t));
            out.push(Checkpoint { at, tiles });
        };
        let mut last_kept = vec![0usize; tiles];
        kept.iter().enumerate().skip(start).for_each(|(i, k)| {
            if let Some(j) = k {
                self.footprints[*j].iter().for_each(|&t| last_kept[t as usize] = i);
            }
        });
        let mut footprints: Vec<Vec<u32>> = self.footprints[..start].to_vec();
        let mut buffer = StrokeBuffer::new(self.width, self.height);
        let mut catchup = StrokeBuffer::new(self.width, self.height);
        buffer.set_mask(Some(dirty.clone()));
        let mut replayed = 0;
        for (i, mark) in next.iter().enumerate().skip(start) {
            while let Some((_, old)) = later.next_if(|(at, _)| *at == i) {
                snapshot(i, old, &dirty, raster, &mut snapshots);
            }
            if let Some(j) = kept[i] {
                let footprint = &self.footprints[j];
                if footprint.iter().any(|&t| dirty[t as usize]) {
                    painted(raster, &mut buffer, brushes, mark)?;
                    replayed += 1;
                }
                footprints.push(footprint.clone());
                continue;
            }
            buffer.set_mask(None);
            let footprint: Vec<usize> = match mark {
                Mark::Stroke(stroke) => {
                    draw_stroke_on(&mut buffer, brushes.get(&stroke.brush)?, stroke, Some(raster));
                    buffer.touched().to_vec()
                }
                Mark::Fill(fill) => fill_tiles(raster, fill),
            };
            let (fresh, settled): (Vec<usize>, Vec<usize>) = footprint
                .iter()
                .copied()
                .filter(|&t| !dirty[t])
                .partition(|&t| last_kept[t] > i);
            settled.iter().for_each(|&t| dirty[t] = true);
            if !fresh.is_empty() {
                let mut only = vec![false; tiles];
                fresh.iter().for_each(|&t| {
                    only[t] = true;
                    put(raster, t, &origin[t]);
                });
                catchup.set_mask(Some(only.clone()));
                (start..i)
                    .filter(|&k| footprints[k].iter().any(|&t| only[t as usize]))
                    .try_for_each(|k| painted(raster, &mut catchup, brushes, &next[k]).map(|_| ()))?;
                fresh.iter().for_each(|&t| dirty[t] = true);
            }
            buffer.set_mask(Some(dirty.clone()));
            match mark {
                Mark::Stroke(stroke) => {
                    land_stroke(raster, &mut buffer, stroke, brushes.get(&stroke.brush)?);
                }
                Mark::Fill(fill) => {
                    paint_fill_where(raster, fill, |t| dirty[t]);
                }
            }
            footprints.push(footprint.iter().map(|&t| t as u32).collect());
            replayed += 1;
        }
        later.for_each(|(at, old)| snapshot(at.min(next.len()), old, &dirty, raster, &mut snapshots));
        self.checkpoints.truncate(base + 1);
        self.checkpoints.extend(snapshots.into_iter().filter(|c| c.at > start));
        self.marks = next;
        self.keys = keys;
        self.footprints = footprints;
        self.maybe_checkpoint(raster);
        Ok(Updated {
            tiles: (0..tiles).filter(|&t| dirty[t]).collect(),
            replayed,
            full: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::replay;
    use klad_core::fill::{Coverage, Fill, FillRef, FillSource, Seed};
    use klad_core::markedit::{self, MarkChange, MarkTransform};
    use klad_core::stroke::marks;
    use klad_core::synth::Synth;
    use serde_json::json;

    const W: u32 = 1024;
    const H: u32 = 768;

    fn sample(n: usize) -> Vec<Mark> {
        let mut synth = Synth::canvas(W as f32, H as f32);
        synth.brushes = vec![
            "ink".into(),
            "pencil".into(),
            "chalk".into(),
            "eraser".into(),
            "marker".into(),
        ];
        let mut all = marks(synth.strokes(n, 7));
        let mut mask = vec![0u8; (W * H) as usize];
        (300..420).for_each(|y| (200..700).for_each(|x| mask[y * W as usize + x] = 200));
        let fill = Fill::new(
            0,
            [0.2, 0.5, 0.9, 1.0],
            FillSource::Seed {
                seed: Seed {
                    x: 300.0,
                    y: 350.0,
                    tolerance: 0.1,
                    gap: 0.0,
                    expand: 2.0,
                },
                reference: FillRef::This,
            },
            Coverage::from_mask(W, H, &mask),
        );
        all.insert(n / 3, Mark::Fill(fill));
        all.iter_mut().enumerate().for_each(|(i, m)| m.set_id(i as u32 + 1));
        all
    }

    fn brushes() -> BrushSet {
        BrushSet::presets()
    }

    #[test]
    fn tiles_replayed_on_their_own_match_one_replay_for_every_dry_brush() {
        let set = brushes();
        let mut synth = Synth::canvas(W as f32, H as f32);
        synth.brushes = set
            .defs()
            .filter(|(_, d)| !d.reads_pixels())
            .map(|(id, _)| id.to_string())
            .collect();
        assert!(synth.brushes.len() > 10, "{:?}", synth.brushes);
        let mut all = sample(30);
        all.extend(marks(synth.strokes(260, 11)));
        all.iter_mut().enumerate().for_each(|(i, m)| m.set_id(i as u32 + 1));
        let (one, raster, dabs) = CelReplay::replay(W, H, all.clone(), &set).unwrap();
        let (tiled, fast, fast_dabs) = CelReplay::replay_tiled(W, H, all.clone(), &set).unwrap();
        assert_eq!(fast.hash(), raster.hash());
        assert_eq!(
            (fast_dabs, tiled.footprints(), tiled.keys()),
            (dabs, one.footprints(), one.keys())
        );
        assert_eq!(tiled.checkpoints(), [0], "the tiled path was taken");
        let mut wet = all.clone();
        let mut synth = Synth::canvas(W as f32, H as f32);
        synth.brushes = vec!["wet".into()];
        wet.extend(marks(synth.strokes(2, 5)));
        wet.iter_mut().enumerate().for_each(|(i, m)| m.set_id(i as u32 + 1));
        let (serial, _, _) = CelReplay::replay_tiled(W, H, wet, &set).unwrap();
        assert!(
            serial.checkpoints().len() > 1,
            "a brush that reads pixels replays in order"
        );
    }

    fn edited(marks: &[Mark], changes: Vec<MarkChange>) -> Vec<Mark> {
        let mut out = marks.to_vec();
        changes
            .into_iter()
            .for_each(|mut c| markedit::apply(&mut out, &mut c, "c1").unwrap());
        out
    }

    fn check(cel: &CelReplay, raster: &Raster, expected: &[Mark]) {
        let full = replay(W, H, expected, &brushes()).unwrap().raster;
        assert_eq!(raster.hash(), full.hash());
        let fresh = CelReplay::replay(W, H, expected.to_vec(), &brushes()).unwrap().0;
        assert_eq!(cel.footprints(), fresh.footprints());
        assert_eq!(cel.keys(), fresh.keys());
    }

    fn set(id: u32, props: serde_json::Value) -> MarkChange {
        MarkChange::Set {
            id,
            props: props.as_object().unwrap().clone(),
        }
    }

    #[test]
    fn edits_replay_from_the_nearest_checkpoint_to_the_same_pixels() {
        let brushes = brushes();
        let start = sample(400);
        let (mut cel, mut raster, _) = CelReplay::replay(W, H, start.clone(), &brushes).unwrap();
        assert_eq!(cel.checkpoints(), [0, 128, 256, 384]);
        let steps: Vec<Vec<MarkChange>> = vec![
            vec![set(390, json!({"color":"#ff0000"}))],
            vec![set(395, json!({"color":"#00ffff"}))],
            vec![set(5, json!({"color":"#00ff00","size":30}))],
            vec![MarkChange::Remove { id: 200 }],
            vec![
                MarkChange::Remove { id: 150 },
                set(300, json!({"brush":"chalk","size":40})),
            ],
            vec![set(151, json!({"opacity":0.3}))],
            vec![MarkChange::Transform {
                ids: vec![10, 11, 12, 140],
                transform: MarkTransform {
                    x: 120.0,
                    y: -40.0,
                    rotate: 20.0,
                    ..Default::default()
                },
            }],
            vec![MarkChange::Transform {
                ids: vec![134],
                transform: MarkTransform {
                    x: 31.0,
                    y: 7.0,
                    ..Default::default()
                },
            }],
            vec![MarkChange::Add(sample(3)[..2].to_vec())],
        ];
        assert!(start[133].as_fill().is_some());
        let late = edited(&start, vec![set(390, json!({"color":"#ff0000"}))]);
        let mut probe = cel.clone();
        let mut probe_raster = raster.clone();
        let done = probe.update(&mut probe_raster, late, &brushes).unwrap();
        assert!(done.replayed <= 401 - 384, "{}", done.replayed);
        let mut current = start;
        steps.into_iter().for_each(|changes| {
            current = edited(&current, changes);
            let done = cel.update(&mut raster, current.clone(), &brushes).unwrap();
            assert!(!done.full);
            check(&cel, &raster, &current);
        });
        assert_eq!(cel.marks(), &current[..]);
        let back = sample(400);
        let done = cel.update(&mut raster, back.clone(), &brushes).unwrap();
        assert!(!done.full);
        check(&cel, &raster, &back);
    }

    #[test]
    fn checkpoints_survive_edits_and_reordered_marks_fall_back_to_a_full_replay() {
        let brushes = brushes();
        let start = sample(300);
        let (mut cel, mut raster, _) = CelReplay::replay(W, H, start.clone(), &brushes).unwrap();
        let fewer = edited(&start, vec![MarkChange::Remove { id: 3 }, MarkChange::Remove { id: 4 }]);
        cel.update(&mut raster, fewer.clone(), &brushes).unwrap();
        assert_eq!(cel.checkpoints(), [0, 126, 254]);
        let late = edited(&fewer, vec![set(290, json!({"color":"#123456"}))]);
        let done = cel.update(&mut raster, late.clone(), &brushes).unwrap();
        assert!(done.replayed < 50, "{}", done.replayed);
        check(&cel, &raster, &late);
        let mut swapped = late.clone();
        swapped.swap(10, 20);
        let done = cel.update(&mut raster, swapped.clone(), &brushes).unwrap();
        assert!(done.full);
        check(&cel, &raster, &swapped);
    }

    #[test]
    fn appending_paints_only_the_new_marks() {
        let brushes = brushes();
        let start = sample(300);
        let (mut cel, mut raster, _) = CelReplay::replay(W, H, start[..290].to_vec(), &brushes).unwrap();
        let done = cel.update(&mut raster, start.clone(), &brushes).unwrap();
        assert_eq!(done.replayed, 11);
        check(&cel, &raster, &start);
        let mut inserted = start.clone();
        let mut extra = sample(2)[1].clone();
        extra.set_id(900);
        inserted.insert(150, extra);
        cel.update(&mut raster, inserted.clone(), &brushes).unwrap();
        check(&cel, &raster, &inserted);
    }

    #[test]
    fn a_cache_without_marks_or_checkpoints_still_updates_exactly() {
        let brushes = brushes();
        let start = sample(150);
        let (cel, mut raster, _) = CelReplay::replay(W, H, start.clone(), &brushes).unwrap();
        let mut restored = CelReplay::from_cache(W, H, cel.keys().to_vec(), cel.footprints().to_vec());
        let next = edited(&start, vec![set(20, json!({"size":3})), MarkChange::Remove { id: 90 }]);
        let done = restored.update(&mut raster, next.clone(), &brushes).unwrap();
        assert!(!done.full && done.replayed < 150);
        check(&restored, &raster, &next);
    }

    #[test]
    fn appended_and_committed_strokes_extend_the_record() {
        let brushes = brushes();
        let start = sample(130);
        let (mut cel, mut raster, _) = CelReplay::replay(W, H, start[..100].to_vec(), &brushes).unwrap();
        start[100..]
            .iter()
            .for_each(|m| drop(cel.paint(&mut raster, m.clone(), &brushes).unwrap()));
        check(&cel, &raster, &start);
        assert_eq!(cel.checkpoints(), [0, 128]);
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-brush edit_timing -- --ignored --nocapture"]
    fn edit_timing() {
        let brushes = brushes();
        let dense = std::env::var("DENSE").is_ok();
        let side = if dense { 640.0 } else { 2048.0 };
        let mut synth = Synth::canvas(side, side);
        synth.brushes = vec!["ink".into(), "pencil".into(), "marker".into()];
        let mut start = marks(synth.strokes(1000, 3));
        start.iter_mut().enumerate().for_each(|(i, m)| m.set_id(i as u32 + 1));
        let t = std::time::Instant::now();
        let (cel, raster, _) = CelReplay::replay(2048, 2048, start.clone(), &brushes).unwrap();
        println!(
            "replay of 1000 strokes on 2048: {:.0} ms, checkpoints {:?}, {} MB",
            t.elapsed().as_secs_f64() * 1000.0,
            cel.checkpoints(),
            cel.checkpoint_bytes() >> 20
        );
        [1u32, 250, 500, 750, 990, 1000].iter().for_each(|&id| {
            let next = edited(&start, vec![set(id, json!({"color":"#ff0000"}))]);
            let (mut c, mut r) = (cel.clone(), raster.clone());
            let t = std::time::Instant::now();
            let done = c.update(&mut r, next, &brushes).unwrap();
            println!(
                "recolour stroke {id}: {:.1} ms, {} marks replayed, {} tiles",
                t.elapsed().as_secs_f64() * 1000.0,
                done.replayed,
                done.tiles.len()
            );
        });
        let ids: Vec<u32> = (940..990).collect();
        let next = edited(
            &start,
            vec![MarkChange::Transform {
                ids,
                transform: MarkTransform {
                    x: 40.0,
                    y: 25.0,
                    ..Default::default()
                },
            }],
        );
        let (mut c, mut r) = (cel.clone(), raster.clone());
        let t = std::time::Instant::now();
        let done = c.update(&mut r, next, &brushes).unwrap();
        println!(
            "move 50 strokes: {:.1} ms, {} replayed, {} tiles",
            t.elapsed().as_secs_f64() * 1000.0,
            done.replayed,
            done.tiles.len()
        );
    }

    fn px(raster: &Raster, x: usize, y: usize) -> [f32; 4] {
        raster
            .get(x / crate::TILE, y / crate::TILE)
            .map(|t| {
                let at = ((y % crate::TILE) * crate::TILE + x % crate::TILE) * 4;
                [t[at], t[at + 1], t[at + 2], t[at + 3]].map(|v| v.to_f32())
            })
            .unwrap_or([0.0; 4])
    }

    #[test]
    fn locked_strokes_only_land_where_the_layer_has_alpha() {
        let mut mask = vec![0u8; (W * H) as usize];
        (100..200).for_each(|y| (100..400).for_each(|x| mask[y * W as usize + x] = 255));
        let flat = Fill::new(
            1,
            [0.9, 0.8, 0.2, 1.0],
            FillSource::Polygon(vec![[100.0, 100.0], [400.0, 100.0], [400.0, 200.0]]),
            Coverage::from_mask(W, H, &mask),
        );
        let mut shade = klad_core::stroke::Stroke::new(2, "ink", [0.1, 0.1, 0.4, 1.0], 40.0, 3)
            .with_points(&[(50.0, 150.0, 1.0), (450.0, 150.0, 1.0)], 4.0);
        shade.lock = true;
        let mut eraser = klad_core::stroke::Stroke::new(3, "eraser", [0.0, 0.0, 0.0, 1.0], 30.0, 3)
            .with_points(&[(50.0, 120.0, 1.0), (450.0, 120.0, 1.0)], 4.0);
        eraser.lock = true;
        let marks = vec![Mark::Fill(flat), Mark::Stroke(shade), Mark::Stroke(eraser)];
        let raster = replay(W, H, &marks, &brushes()).unwrap().raster;
        let inside = px(&raster, 250, 150);
        assert!(
            (inside[3] - 1.0).abs() < 1e-3 && inside[2] > 0.35 && inside[0] < 0.15,
            "{inside:?}"
        );
        assert_eq!(px(&raster, 60, 150), [0.0; 4]);
        assert_eq!(px(&raster, 420, 150), [0.0; 4]);
        assert!((px(&raster, 250, 120)[3] - 1.0).abs() < 1e-3);
        let (cel, mut live, _) = CelReplay::replay(W, H, marks[..1].to_vec(), &brushes()).unwrap();
        let mut cel = cel;
        cel.update(&mut live, marks.clone(), &brushes()).unwrap();
        assert_eq!(live.hash(), raster.hash());
    }

    #[test]
    fn a_cut_and_moved_lasso_leaves_everything_outside_alone() {
        let all = sample(60);
        let polygon = vec![[300.0, 200.0], [620.0, 220.0], [600.0, 520.0], [280.0, 480.0]];
        let before = replay(W, H, &all, &brushes()).unwrap().raster;
        let mut split = all.clone();
        let chosen = markedit::split(&mut split, &polygon, true);
        assert!(chosen.len() > all.len() / 10, "{} chosen", chosen.len());
        let unmoved = replay(W, H, &split, &brushes()).unwrap().raster;
        let far = |x: usize, y: usize| !(270..=630).contains(&x) || !(190..=530).contains(&y);
        let worst = (0..H as usize)
            .step_by(3)
            .flat_map(|y| (0..W as usize).step_by(3).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (a, b) = (px(&before, x, y), px(&unmoved, x, y));
                (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0f32, f32::max)
            })
            .fold(0.0f32, f32::max);
        assert!(worst < 0.3, "splitting without moving changed a pixel by {worst}");
        let moved = edited(
            &all,
            vec![MarkChange::Lasso {
                polygon: polygon.clone(),
                cut: true,
                action: markedit::LassoAction::Transform(MarkTransform {
                    x: 300.0,
                    y: 150.0,
                    ..Default::default()
                }),
                chosen: Vec::new(),
            }],
        );
        let after = replay(W, H, &moved, &brushes()).unwrap().raster;
        let reference = replay(
            W,
            H,
            &split
                .iter()
                .filter(|m| !chosen.contains(&m.id()))
                .cloned()
                .collect::<Vec<_>>(),
            &brushes(),
        )
        .unwrap()
        .raster;
        (0..H as usize)
            .step_by(2)
            .flat_map(|y| (0..W as usize).step_by(2).map(move |x| (x, y)))
            .filter(|&(x, y)| far(x, y) && (x < 570 || y < 340))
            .for_each(|(x, y)| assert_eq!(px(&after, x, y), px(&reference, x, y), "({x},{y})"));
        let (mut cel, mut raster, _) = CelReplay::replay(W, H, all.clone(), &brushes()).unwrap();
        cel.update(&mut raster, moved.clone(), &brushes()).unwrap();
        check(&cel, &raster, &moved);
    }
}
