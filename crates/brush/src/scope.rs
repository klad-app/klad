use crate::brush::{Brush, BrushSet, StrokeBlend};
use crate::engine::{blend_colours, blend_coverage, draw_stroke_on, land_body, land_stroke, stroke_blend};
use crate::fill::{fill_tile, paint_fill_where};
use crate::tiles::{Mode, Raster, StrokeBuffer, TILE};
use anyhow::Result;
use half::f16;
use klad_core::attach::reach;
use klad_core::brushes::{BrushBlend, PaintDef};
use klad_core::fill::Fill;
use klad_core::stroke::{Mark, Stroke};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Rect {
    fn of(b: [f32; 4], width: u32, height: u32) -> Option<Self> {
        let clip = |v: f32, most: u32| (v.max(0.0) as usize).min(most as usize);
        let r = Self {
            x0: clip(b[0].floor(), width),
            y0: clip(b[1].floor(), height),
            x1: clip(b[2].ceil(), width),
            y1: clip(b[3].ceil(), height),
        };
        (r.x0 < r.x1 && r.y0 < r.y1).then_some(r)
    }

    fn of_tile(index: usize, cols: usize) -> Self {
        let (ox, oy) = ((index % cols) * TILE, (index / cols) * TILE);
        Self {
            x0: ox,
            y0: oy,
            x1: ox + TILE,
            y1: oy + TILE,
        }
    }

    fn meet(&self, o: &Self) -> Option<Self> {
        let r = Self {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        };
        (r.x0 < r.x1 && r.y0 < r.y1).then_some(r)
    }

    fn width(&self) -> usize {
        self.x1 - self.x0
    }

    fn tiles(&self, cols: usize) -> impl Iterator<Item = usize> + use<> {
        let (x0, x1) = (self.x0 / TILE, (self.x1 - 1) / TILE);
        (self.y0 / TILE..=(self.y1 - 1) / TILE).flat_map(move |ty| (x0..=x1).map(move |tx| ty * cols + tx))
    }
}

fn padded(mark: &Mark, brushes: &BrushSet) -> Option<[f32; 4]> {
    match mark {
        Mark::Stroke(s) => {
            let pad = brushes.def(&s.brush).map_or(s.size * 0.5 + 2.0, |d| reach(s, d));
            s.bounds()
                .map(|[x0, y0, x1, y1]| [x0 - pad, y0 - pad, x1 + pad, y1 + pad])
        }
        Mark::Fill(f) => f.coverage.bounds(),
    }
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    Linear,
    Not,
}

fn reads(mark: &Mark, brushes: &BrushSet) -> Option<Reads> {
    let stroke = mark.as_stroke()?;
    match brushes.def(&stroke.brush)?.paint? {
        PaintDef::Smudge { .. } | PaintDef::Blend { .. } if !stroke.lock => Some(Reads::Linear),
        _ => Some(Reads::Not),
    }
}

pub fn erases(stroke: &Stroke, brushes: &BrushSet) -> bool {
    !stroke.lock
        && brushes
            .def(&stroke.brush)
            .is_some_and(|d| d.blend == BrushBlend::Erase && d.paint.is_none())
}

#[derive(Clone, Debug)]
struct Plan {
    at: usize,
    first: usize,
    targets: BTreeSet<u32>,
    rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Below,
    Group,
    Above,
}

#[derive(Clone)]
struct Share {
    plan: usize,
    below: Vec<f16>,
    group: Vec<f16>,
    above: Vec<f16>,
}

impl Share {
    fn layer(&mut self, layer: Layer) -> &mut Vec<f16> {
        match layer {
            Layer::Below => &mut self.below,
            Layer::Group => &mut self.group,
            Layer::Above => &mut self.above,
        }
    }
}

#[derive(Clone, Default)]
pub struct Scope {
    width: u32,
    height: u32,
    plans: Vec<Plan>,
    order: Vec<usize>,
    erasers: BTreeMap<usize, usize>,
    open: Vec<Share>,
    cursor: usize,
}

impl Scope {
    pub fn plan(width: u32, height: u32, marks: &[Mark], brushes: &BrushSet) -> Self {
        let mut scope = Self {
            width,
            height,
            ..Self::default()
        };
        if !marks.iter().filter_map(Mark::as_stroke).any(|s| !s.on.is_empty()) {
            return scope;
        }
        let position: BTreeMap<u32, usize> = marks.iter().enumerate().map(|(i, m)| (m.id(), i)).collect();
        let readers: Vec<(usize, Reads, [f32; 4])> = marks
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((i, reads(m, brushes)?, padded(m, brushes)?)))
            .collect();
        scope.plans = marks
            .iter()
            .enumerate()
            .filter_map(|(at, m)| {
                let stroke = m.as_stroke().filter(|s| !s.on.is_empty() && erases(s, brushes))?;
                let targets: BTreeSet<u32> = stroke
                    .on
                    .iter()
                    .copied()
                    .filter(|id| position.get(id).is_some_and(|&p| p < at))
                    .collect();
                let first = targets.iter().map(|id| position[id]).min()?;
                let mut reach = padded(m, brushes)?;
                loop {
                    let near: Vec<&(usize, Reads, [f32; 4])> = readers
                        .iter()
                        .filter(|(j, _, b)| *j > first && *j < at && overlap(reach, *b))
                        .collect();
                    if near.iter().any(|(_, kind, _)| *kind == Reads::Not) {
                        return None;
                    }
                    let grown = near.iter().fold(reach, |r, (_, _, b)| {
                        [r[0].min(b[0]), r[1].min(b[1]), r[2].max(b[2]), r[3].max(b[3])]
                    });
                    if grown == reach {
                        break;
                    }
                    reach = grown;
                }
                Some(Plan {
                    at,
                    first,
                    targets,
                    rect: Rect::of(reach, width, height)?,
                })
            })
            .collect();
        scope.order = (0..scope.plans.len()).collect();
        scope.order.sort_by_key(|&p| scope.plans[p].first);
        scope.erasers = scope.plans.iter().enumerate().map(|(p, plan)| (plan.at, p)).collect();
        scope
    }

    pub fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }

    pub fn fresh(&self) -> Self {
        Self {
            open: Vec::new(),
            cursor: 0,
            ..self.clone()
        }
    }

    pub fn widen(&self, start: usize, dirty: &[bool], cols: usize) -> usize {
        self.plans
            .iter()
            .filter(|p| p.first < start && p.at >= start && p.rect.tiles(cols).any(|t| dirty[t]))
            .map(|p| p.first)
            .min()
            .unwrap_or(start)
    }

    fn before(&mut self, at: usize, raster: &Raster) {
        let plans = &self.plans;
        self.open.retain(|s| plans[s.plan].at >= at);
        while let Some(&p) = self.order.get(self.cursor).filter(|&&p| self.plans[p].first <= at) {
            self.cursor += 1;
            let plan = &self.plans[p];
            if plan.at <= at {
                continue;
            }
            let rect = plan.rect;
            let size = rect.width() * (rect.y1 - rect.y0) * 4;
            let mut below = vec![f16::ZERO; size];
            rect.tiles(raster.cols).for_each(|t| {
                let (Some(r), Some(src)) = (Rect::of_tile(t, raster.cols).meet(&rect), raster.at(t)) else {
                    return;
                };
                let (ox, oy) = ((t % raster.cols) * TILE, (t / raster.cols) * TILE);
                (r.y0..r.y1).for_each(|y| {
                    let from = ((y - oy) * TILE + r.x0 - ox) * 4;
                    let to = ((y - rect.y0) * rect.width() + r.x0 - rect.x0) * 4;
                    let n = r.width() * 4;
                    below[to..to + n].copy_from_slice(&src[from..from + n]);
                });
            });
            self.open.push(Share {
                plan: p,
                below,
                group: vec![f16::ZERO; size],
                above: vec![f16::ZERO; size],
            });
        }
    }

    pub fn paint(
        &mut self,
        raster: &mut Raster,
        buffer: &mut StrokeBuffer,
        brushes: &BrushSet,
        mark: &Mark,
        at: usize,
    ) -> Result<(Vec<usize>, usize)> {
        self.before(at, raster);
        Ok(match mark {
            Mark::Stroke(stroke) => {
                let brush = brushes.get(&stroke.brush)?;
                let dabs = draw_stroke_on(buffer, brush, stroke, Some(raster));
                (self.land(raster, buffer, stroke, brush, at), dabs)
            }
            Mark::Fill(fill) => {
                self.follow_fill(raster, fill, |i| buffer.allowed(i));
                (paint_fill_where(raster, fill, |i| buffer.allowed(i)), 0)
            }
        })
    }

    pub fn land(
        &mut self,
        raster: &mut Raster,
        buffer: &mut StrokeBuffer,
        stroke: &Stroke,
        brush: &Brush,
        at: usize,
    ) -> Vec<usize> {
        self.before(at, raster);
        match self.erasers.get(&at).copied() {
            Some(p) if self.open.iter().any(|s| s.plan == p) => self.erase(raster, buffer, stroke, p),
            _ => {
                self.follow_stroke(raster, buffer, stroke, brush);
                land_stroke(raster, buffer, stroke, brush)
            }
        }
    }

    fn hit(&self, tiles: &[usize], cols: usize) -> Vec<usize> {
        (0..self.open.len())
            .filter(|&k| {
                let rect = self.plans[self.open[k].plan].rect;
                tiles.iter().any(|&t| Rect::of_tile(t, cols).meet(&rect).is_some())
            })
            .collect()
    }

    fn layers(&self, k: usize, id: u32, blend: StrokeBlend) -> Vec<Layer> {
        match (blend, self.plans[self.open[k].plan].targets.contains(&id)) {
            (StrokeBlend::Erase, _) => vec![Layer::Below, Layer::Group, Layer::Above],
            (_, true) => vec![Layer::Group],
            (_, false) => vec![Layer::Above],
        }
    }

    fn follow_stroke(&mut self, raster: &Raster, buffer: &StrokeBuffer, stroke: &Stroke, brush: &Brush) {
        if self.open.is_empty() {
            return;
        }
        let Some(blend) = stroke_blend(brush, stroke.lock) else {
            return;
        };
        let cols = raster.cols;
        let tiles: Vec<usize> = buffer
            .touched()
            .iter()
            .copied()
            .filter(|&t| buffer.allowed(t))
            .collect();
        let hit = self.hit(&tiles, cols);
        if hit.is_empty() {
            return;
        }
        if buffer.mode == Mode::Replace {
            hit.into_iter().for_each(|k| {
                [Layer::Below, Layer::Group, Layer::Above]
                    .into_iter()
                    .for_each(|layer| self.rerun(k, layer, &tiles, stroke, brush))
            });
            return;
        }
        let (color, opacity) = (stroke.color(), stroke.opacity);
        tiles.iter().for_each(|&t| {
            let (Some(coverage), Some(written)) = (buffer.coverage(t), buffer.rect(t)) else {
                return;
            };
            let origin = ((t % cols) * TILE, (t / cols) * TILE);
            let coverage: Cow<[f32]> = match crate::cut::factor(&stroke.cuts, origin) {
                Some(f) => Cow::Owned(coverage.iter().zip(&f).map(|(c, k)| c * k).collect()),
                None => Cow::Borrowed(coverage),
            };
            let area = Rect {
                x0: origin.0 + written[0],
                y0: origin.1 + written[1],
                x1: origin.0 + written[2],
                y1: origin.1 + written[3],
            };
            let paint = match buffer.mode {
                Mode::Colour => buffer.paint(t),
                _ => None,
            };
            hit.iter().for_each(|&k| {
                self.layers(k, stroke.id, blend)
                    .into_iter()
                    .for_each(|layer| self.land_on(k, layer, cols, t, area, &coverage, paint, color, opacity, blend));
            });
        });
    }

    fn follow_fill(&mut self, raster: &Raster, fill: &Fill, allowed: impl Fn(usize) -> bool) {
        if self.open.is_empty() {
            return;
        }
        let Some(bounds) = fill
            .coverage
            .bounds()
            .and_then(|b| Rect::of(b, self.width, self.height))
        else {
            return;
        };
        let cols = raster.cols;
        let tiles: Vec<usize> = bounds.tiles(cols).filter(|&t| allowed(t)).collect();
        let hit = self.hit(&tiles, cols);
        if hit.is_empty() {
            return;
        }
        let blend = match fill.lock {
            true => StrokeBlend::Atop,
            false => StrokeBlend::Normal,
        };
        tiles.iter().for_each(|&t| {
            let Some((coverage, true)) = fill_tile(raster, fill, t) else {
                return;
            };
            let area = Rect::of_tile(t, cols);
            hit.iter().for_each(|&k| {
                self.layers(k, fill.id, blend).into_iter().for_each(|layer| {
                    self.land_on(
                        k,
                        layer,
                        cols,
                        t,
                        area,
                        &coverage,
                        None,
                        fill.color(),
                        fill.opacity,
                        blend,
                    )
                });
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn land_on(
        &mut self,
        k: usize,
        layer: Layer,
        cols: usize,
        tile: usize,
        area: Rect,
        coverage: &[f32],
        paint: Option<&[f32]>,
        color: [f32; 4],
        opacity: f32,
        blend: StrokeBlend,
    ) {
        let rect = self.plans[self.open[k].plan].rect;
        let Some(r) = rect.meet(&area) else {
            return;
        };
        let (ox, oy) = ((tile % cols) * TILE, (tile / cols) * TILE);
        let px = self.open[k].layer(layer);
        (r.y0..r.y1).for_each(|y| {
            let at = (y - oy) * TILE + (r.x0 - ox);
            let n = r.width();
            let row = ((y - rect.y0) * rect.width() + (r.x0 - rect.x0)) * 4;
            let target = &mut px[row..row + n * 4];
            let cover = &coverage[at..at + n];
            match paint {
                Some(p) => blend_colours(target, cover, &p[at * 4..(at + n) * 4], color, opacity, blend),
                None => blend_coverage(target, cover, color, opacity, blend),
            }
        });
    }

    fn rerun(&mut self, k: usize, layer: Layer, allowed: &[usize], stroke: &Stroke, brush: &Brush) {
        let rect = self.plans[self.open[k].plan].rect;
        let mut tmp = Raster::new(self.width, self.height);
        let cols = tmp.cols;
        let tiles: Vec<usize> = rect.tiles(cols).collect();
        let rows = |t: usize| {
            let r = Rect::of_tile(t, cols).meet(&rect);
            let (ox, oy) = ((t % cols) * TILE, (t / cols) * TILE);
            r.into_iter().flat_map(move |r| {
                (r.y0..r.y1).map(move |y| {
                    (
                        ((y - oy) * TILE + r.x0 - ox) * 4,
                        ((y - rect.y0) * rect.width() + r.x0 - rect.x0) * 4,
                        r.width() * 4,
                    )
                })
            })
        };
        let px = self.open[k].layer(layer);
        if px.iter().all(|v| *v == f16::ZERO) {
            return;
        }
        tiles.iter().for_each(|&t| {
            let target = tmp.ensure(t);
            rows(t).for_each(|(at, row, n)| target[at..at + n].copy_from_slice(&px[row..row + n]));
        });
        let mut buffer = StrokeBuffer::new(self.width, self.height);
        let mut only = vec![false; tmp.cols * tmp.rows];
        tiles
            .iter()
            .filter(|t| allowed.contains(t))
            .for_each(|&t| only[t] = true);
        buffer.set_mask(Some(only));
        draw_stroke_on(&mut buffer, brush, stroke, Some(&tmp));
        land_stroke(&mut tmp, &mut buffer, stroke, brush);
        tiles.iter().for_each(|&t| {
            if let Some(src) = tmp.at(t) {
                rows(t).for_each(|(at, row, n)| px[row..row + n].copy_from_slice(&src[at..at + n]));
            }
        });
    }

    fn erase(&mut self, raster: &mut Raster, buffer: &mut StrokeBuffer, stroke: &Stroke, p: usize) -> Vec<usize> {
        let Some(at) = self.open.iter().position(|s| s.plan == p) else {
            return Vec::new();
        };
        let share = self.open.swap_remove(at);
        let mine = &self.plans[p];
        let rect = mine.rect;
        let reaches: Vec<(bool, bool)> = self
            .open
            .iter()
            .map(|s| {
                let theirs = &self.plans[s.plan].targets;
                (!mine.targets.is_disjoint(theirs), !mine.targets.is_subset(theirs))
            })
            .collect();
        let (color, opacity) = (stroke.color(), stroke.opacity);
        let alpha = (opacity * color[3]).clamp(0.0, 1.0);
        let cols = raster.cols;
        let touched = buffer.touched().to_vec();
        let allowed: Vec<usize> = touched.iter().copied().filter(|&t| buffer.allowed(t)).collect();
        allowed.into_iter().for_each(|t| {
            let origin = ((t % cols) * TILE, (t / cols) * TILE);
            if let (Some(f), Some(coverage)) = (crate::cut::factor(&stroke.cuts, origin), buffer.coverage_mut(t)) {
                coverage.iter_mut().zip(&f).for_each(|(c, k)| *c *= k);
            }
            let (Some(coverage), Some(written)) = (buffer.coverage(t), buffer.rect(t)) else {
                return;
            };
            let area = Rect {
                x0: origin.0 + written[0],
                y0: origin.1 + written[1],
                x1: origin.0 + written[2],
                y1: origin.1 + written[3],
            };
            let keep_at = |x: usize, y: usize| {
                let c = coverage[(y - origin.1) * TILE + x - origin.0];
                (c > 0.0).then_some(1.0 - c * alpha)
            };
            if let (Some(r), Some(target)) = (rect.meet(&area), raster.at_mut(t)) {
                (r.y0..r.y1).for_each(|y| {
                    (r.x0..r.x1).for_each(|x| {
                        let Some(keep) = keep_at(x, y) else {
                            return;
                        };
                        let s = ((y - rect.y0) * rect.width() + x - rect.x0) * 4;
                        let px = ((y - origin.1) * TILE + x - origin.0) * 4;
                        let get = |v: &[f16]| -> [f32; 4] { std::array::from_fn(|i| v[s + i].to_f32()) };
                        let (below, group, above) = (get(&share.below), get(&share.group), get(&share.above));
                        let kept: [f32; 4] = std::array::from_fn(|i| group[i] * keep);
                        let under: [f32; 4] = std::array::from_fn(|i| kept[i] + below[i] * (1.0 - kept[3]));
                        (0..4).for_each(|i| target[px + i] = f16::from_f32(above[i] + under[i] * (1.0 - above[3])));
                    });
                });
            }
            self.open.iter_mut().zip(&reaches).for_each(|(other, (group, rest))| {
                let theirs = self.plans[other.plan].rect;
                let Some(r) = theirs.meet(&rect).and_then(|r| r.meet(&area)) else {
                    return;
                };
                (r.y0..r.y1).for_each(|y| {
                    (r.x0..r.x1).for_each(|x| {
                        let Some(keep) = keep_at(x, y) else {
                            return;
                        };
                        let o = ((y - theirs.y0) * theirs.width() + x - theirs.x0) * 4;
                        let scale = |v: &mut [f16]| {
                            (0..4).for_each(|i| v[o + i] = f16::from_f32(v[o + i].to_f32() * keep));
                        };
                        if *group {
                            scale(&mut other.group);
                        }
                        if *rest {
                            scale(&mut other.above);
                            scale(&mut other.below);
                        }
                    });
                });
            });
            land_body(raster, buffer, t, color, opacity, StrokeBlend::Erase);
        });
        buffer.reset();
        touched
    }
}

pub fn hidden(width: u32, height: u32, marks: &[Mark], brushes: &BrushSet, among: Option<&[u32]>) -> BTreeSet<u32> {
    let position: BTreeMap<u32, usize> = marks.iter().enumerate().map(|(i, m)| (m.id(), i)).collect();
    let mut by: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    marks.iter().enumerate().for_each(|(at, m)| {
        if let Some(s) = m.as_stroke().filter(|s| !s.on.is_empty() && erases(s, brushes)) {
            s.on.iter()
                .filter(|id| position.get(id).is_some_and(|&p| p < at))
                .filter(|id| among.is_none_or(|a| a.contains(id)))
                .for_each(|id| by.entry(*id).or_default().push(at))
        }
    });
    let strength = |at: usize| {
        marks[at]
            .as_stroke()
            .map_or(0.0, |s| (s.opacity * s.color()[3]).clamp(0.0, 1.0))
    };
    let mut coverages: BTreeMap<usize, BTreeMap<usize, Vec<f32>>> = BTreeMap::new();
    let mut cover = |at: usize| -> BTreeMap<usize, Vec<f32>> {
        coverages
            .entry(at)
            .or_insert_with(|| coverage_of(width, height, &marks[at], brushes))
            .clone()
    };
    by.into_iter()
        .filter(|(id, erasers)| {
            let most = erasers.iter().fold(1.0f32, |left, &e| left * (1.0 - strength(e)));
            if most >= 1.0 / 255.0 {
                return false;
            }
            let Some(&at) = position.get(id) else {
                return false;
            };
            let mark = &marks[at];
            let alpha = match mark {
                Mark::Stroke(s) => (s.opacity * s.color()[3]).clamp(0.0, 1.0),
                Mark::Fill(f) => (f.opacity * f.color()[3]).clamp(0.0, 1.0),
            };
            let own = coverage_of(width, height, mark, brushes);
            let erased: Vec<(BTreeMap<usize, Vec<f32>>, f32)> =
                erasers.iter().map(|&e| (cover(e), strength(e))).collect();
            own.iter().all(|(t, c)| {
                c.iter()
                    .enumerate()
                    .filter(|(_, v)| **v * alpha >= 1.0 / 255.0)
                    .all(|(i, v)| {
                        let left = erased.iter().fold(v * alpha, |left, (tiles, s)| {
                            left * (1.0 - tiles.get(t).map_or(0.0, |e| e[i]) * s)
                        });
                        left < 1.0 / 255.0
                    })
            })
        })
        .map(|(id, _)| id)
        .collect()
}

fn coverage_of(width: u32, height: u32, mark: &Mark, brushes: &BrushSet) -> BTreeMap<usize, Vec<f32>> {
    let raster = Raster::new(width, height);
    let cols = raster.cols;
    match mark {
        Mark::Stroke(stroke) => {
            let Ok(brush) = brushes.get(&stroke.brush) else {
                return BTreeMap::new();
            };
            let mut buffer = StrokeBuffer::new(width, height);
            draw_stroke_on(&mut buffer, brush, stroke, None);
            buffer
                .touched()
                .iter()
                .filter_map(|&t| {
                    let c = buffer.coverage(t)?;
                    let origin = ((t % cols) * TILE, (t / cols) * TILE);
                    Some((
                        t,
                        match crate::cut::factor(&stroke.cuts, origin) {
                            Some(f) => c.iter().zip(&f).map(|(c, k)| c * k).collect(),
                            None => c.to_vec(),
                        },
                    ))
                })
                .collect()
        }
        Mark::Fill(fill) => fill
            .coverage
            .bounds()
            .and_then(|b| Rect::of(b, width, height))
            .map(|r| {
                r.tiles(cols)
                    .filter_map(|t| fill_tile(&raster, fill, t).map(|(c, _)| (t, c)))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::CelReplay;
    use klad_core::brushes::BrushDef;
    use klad_core::fill::{Coverage, FillSource};
    use klad_core::markedit::{self, MarkChange, MarkTransform};
    use klad_core::stroke::marks;
    use klad_core::synth::Synth;

    const W: u32 = 768;
    const H: u32 = 512;

    fn set() -> BrushSet {
        let mut set = BrushSet::presets();
        let soft = r#"{"format":"klad-brush/1","name":"Soft eraser","tip":{"round":{"hardness":0.0}},"spacing":0.05,"flow":0.3,"blend":"erase"}"#;
        set.insert("soft-eraser", BrushDef::parse(soft).unwrap());
        set
    }

    fn line(id: u32, brush: &str, colour: [f32; 4], size: f32, points: &[(f32, f32)]) -> Mark {
        let pts: Vec<(f32, f32, f32)> = points.iter().map(|&(x, y)| (x, y, 1.0)).collect();
        Stroke::new(id, brush, colour, size, id).with_points(&pts, 4.0).into()
    }

    fn attached(mut marks: Vec<Mark>, brushes: &BrushSet) -> Vec<Mark> {
        (0..marks.len()).for_each(|at| klad_core::attach::attach(&mut marks, at, |id| brushes.def(id)));
        marks
    }

    fn loose(marks: &[Mark]) -> Vec<Mark> {
        marks
            .iter()
            .cloned()
            .map(|mut m| {
                if let Mark::Stroke(s) = &mut m {
                    s.on.clear();
                }
                m
            })
            .collect()
    }

    fn render(marks: &[Mark], brushes: &BrushSet) -> Raster {
        CelReplay::replay(W, H, marks.to_vec(), brushes).unwrap().1
    }

    fn pixel(r: &Raster, x: usize, y: usize) -> [f32; 4] {
        r.get(x / TILE, y / TILE).map_or([0.0; 4], |t| {
            let i = ((y % TILE) * TILE + x % TILE) * 4;
            std::array::from_fn(|c| t[i + c].to_f32())
        })
    }

    fn painting(brushes: &BrushSet) -> Vec<Mark> {
        let mut synth = Synth::canvas(W as f32, H as f32);
        synth.brushes = vec![
            "ink".into(),
            "pencil".into(),
            "marker".into(),
            "eraser".into(),
            "soft-eraser".into(),
            "two-tone".into(),
        ];
        let mut all = marks(synth.strokes(140, 11));
        let mut mask = vec![0u8; (W * H) as usize];
        (100..400).for_each(|y| (50..700).for_each(|x| mask[y * W as usize + x] = 230));
        let fill = Fill::new(
            0,
            [0.9, 0.85, 0.7, 1.0],
            FillSource::Polygon(vec![[50.0, 100.0], [700.0, 100.0], [700.0, 400.0]]),
            Coverage::from_mask(W, H, &mask),
        );
        all.insert(0, Mark::Fill(fill));
        all.iter_mut().enumerate().for_each(|(i, m)| {
            m.set_id(i as u32 + 1);
            if let Mark::Stroke(s) = m
                && i % 9 == 4
            {
                s.lock = true;
            }
        });
        attached(all, brushes)
    }

    #[test]
    fn attached_erasers_draw_what_they_drew_when_nothing_moved() {
        let brushes = set();
        let marks = painting(&brushes);
        let scoped = marks
            .iter()
            .filter_map(Mark::as_stroke)
            .filter(|s| !s.on.is_empty())
            .count();
        assert!(scoped > 10, "the painting has attached erasers: {scoped}");
        assert!(!Scope::plan(W, H, &marks, &brushes).is_empty());
        assert_eq!(
            render(&marks, &brushes).hash(),
            render(&loose(&marks), &brushes).hash(),
            "hard and soft erasers, fills, lock alpha and colour brushes: bit for bit"
        );
    }

    #[test]
    fn a_smudge_between_an_eraser_and_its_marks_moves_their_share_along() {
        let brushes = set();
        let marks = attached(
            vec![
                line(1, "ink", [0.8, 0.2, 0.1, 1.0], 30.0, &[(100.0, 200.0), (400.0, 200.0)]),
                line(2, "ink", [0.1, 0.3, 0.8, 1.0], 30.0, &[(100.0, 240.0), (400.0, 240.0)]),
                line(
                    3,
                    "smudge",
                    [0.0; 4],
                    40.0,
                    &[(150.0, 190.0), (250.0, 260.0), (350.0, 190.0)],
                ),
                line(4, "blender", [0.0; 4], 50.0, &[(200.0, 220.0), (300.0, 220.0)]),
                line(
                    5,
                    "soft-eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    60.0,
                    &[(220.0, 180.0), (260.0, 260.0)],
                ),
                line(
                    6,
                    "eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    14.0,
                    &[(330.0, 170.0), (330.0, 270.0)],
                ),
            ],
            &brushes,
        );
        assert_eq!(marks[4].as_stroke().unwrap().on, vec![1, 2]);
        assert_eq!(render(&marks, &brushes).hash(), render(&loose(&marks), &brushes).hash());
    }

    #[test]
    fn tiles_on_their_own_and_one_replay_agree_with_attached_erasers() {
        let brushes = set();
        let marks: Vec<Mark> = painting(&brushes)
            .into_iter()
            .filter(|m| m.as_stroke().is_none_or(|s| s.brush != "two-tone"))
            .collect();
        let (_, tiled, _) = CelReplay::replay_tiled(W, H, marks.clone(), &brushes).unwrap();
        assert_eq!(tiled.hash(), render(&marks, &brushes).hash());
    }

    #[test]
    fn a_leaf_moved_with_its_eraser_leaves_the_paint_it_lands_on_whole() {
        let brushes = set();
        ["eraser", "soft-eraser"].iter().for_each(|eraser| {
            let mut marks = attached(
                vec![
                    line(1, "ink", [0.2, 0.6, 0.2, 1.0], 80.0, &[(400.0, 100.0), (400.0, 400.0)]),
                    line(2, "ink", [0.9, 0.7, 0.1, 1.0], 20.0, &[(100.0, 250.0), (200.0, 250.0)]),
                    line(3, eraser, [0.0, 0.0, 0.0, 1.0], 40.0, &[(150.0, 200.0), (150.0, 300.0)]),
                ],
                &brushes,
            );
            assert_eq!(marks[2].as_stroke().unwrap().on, vec![2]);
            let mut change = MarkChange::Transform {
                ids: vec![2],
                transform: MarkTransform {
                    x: 250.0,
                    ..Default::default()
                },
            };
            markedit::apply(&mut marks, &mut change, "c1", Default::default()).unwrap();
            let scoped = render(&marks, &brushes);
            let biting = render(&loose(&marks), &brushes);
            let (x, y) = (400, 215);
            assert_eq!(pixel(&scoped, x, y)[3], 1.0, "{eraser}: the green stroke stays whole");
            assert!(pixel(&biting, x, y)[3] < 0.9, "{eraser}: unattached, it would bite");
            assert!(
                pixel(&scoped, 400, 250)[3] > 0.99,
                "the moved leaf is still cut where it was cut"
            );
            assert!(
                pixel(&scoped, 380, 250)[0] > 0.5,
                "{eraser}: at the cut the green shows through, not a hole"
            );
        });
    }

    #[test]
    fn a_moved_eraser_erases_its_marks_together_over_what_they_landed_on() {
        let brushes = set();
        let mut marks = attached(
            vec![
                line(
                    1,
                    "marker",
                    [0.1, 0.5, 0.2, 1.0],
                    90.0,
                    &[(300.0, 150.0), (500.0, 350.0)],
                ),
                line(
                    2,
                    "marker",
                    [0.9, 0.3, 0.1, 0.8],
                    40.0,
                    &[(100.0, 250.0), (220.0, 250.0)],
                ),
                line(
                    3,
                    "marker",
                    [0.2, 0.2, 0.9, 0.6],
                    30.0,
                    &[(170.0, 200.0), (170.0, 300.0)],
                ),
                line(
                    4,
                    "soft-eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    50.0,
                    &[(130.0, 230.0), (140.0, 270.0)],
                ),
            ],
            &brushes,
        );
        assert_eq!(marks[3].as_stroke().unwrap().on, vec![2, 3]);
        let mut change = MarkChange::Transform {
            ids: vec![2, 3],
            transform: MarkTransform {
                x: 260.0,
                ..Default::default()
            },
        };
        markedit::apply(&mut marks, &mut change, "c1", Default::default()).unwrap();
        let scoped = render(&marks, &brushes);
        let below = render(&marks[..1], &brushes);
        let mut group = render(&marks[1..3], &brushes);
        let rubber = marks[3].as_stroke().unwrap();
        let mut buffer = StrokeBuffer::new(W, H);
        let brush = brushes.get(&rubber.brush).unwrap();
        draw_stroke_on(&mut buffer, brush, rubber, Some(&group));
        land_stroke(&mut group, &mut buffer, rubber, brush);
        let expected = |x: usize, y: usize| {
            let (g, b) = (pixel(&group, x, y), pixel(&below, x, y));
            std::array::from_fn::<f32, 4, _>(|c| g[c] + b[c] * (1.0 - g[3]))
        };
        let worst = (100..420usize)
            .flat_map(|y| (300..560usize).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (a, b) = (pixel(&scoped, x, y), expected(x, y));
                (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0f32, f32::max)
            })
            .fold(0.0f32, f32::max);
        assert!(worst < 2.0 / 255.0, "off by {worst}");
    }

    #[test]
    fn an_edit_updates_as_a_fresh_replay_would_draw_it() {
        let brushes = set();
        let marks = painting(&brushes);
        let (mut cel, mut raster, _) = CelReplay::replay(W, H, marks.clone(), &brushes).unwrap();
        let mut edited = marks.clone();
        let chosen: Vec<u32> = edited
            .iter()
            .filter(|m| m.as_stroke().is_some_and(|s| s.on.is_empty() && s.brush == "ink"))
            .take(6)
            .map(Mark::id)
            .collect();
        let mut change = MarkChange::Transform {
            ids: chosen,
            transform: MarkTransform {
                x: 37.0,
                y: -21.0,
                ..Default::default()
            },
        };
        markedit::apply(&mut edited, &mut change, "c1", Default::default()).unwrap();
        assert!(edited.len() > marks.len(), "some erasers split");
        let updated = cel.update(&mut raster, edited.clone(), &brushes).unwrap();
        assert!(!updated.full);
        assert_eq!(raster.hash(), render(&edited, &brushes).hash());
        let mut gone = edited.clone();
        let erasers: Vec<u32> = gone
            .iter()
            .filter_map(Mark::as_stroke)
            .filter(|s| !s.on.is_empty())
            .take(3)
            .map(|s| s.id)
            .collect();
        gone.retain(|m| !erasers.contains(&m.id()));
        cel.update(&mut raster, gone.clone(), &brushes).unwrap();
        assert_eq!(raster.hash(), render(&gone, &brushes).hash());
    }

    #[test]
    fn a_line_moved_with_its_trimmed_overshoot_stays_trimmed_and_the_line_it_lands_on_stays_whole() {
        let brushes = set();
        let mut marks = attached(
            vec![
                line(1, "ink", [0.1, 0.1, 0.1, 1.0], 6.0, &[(300.0, 100.0), (300.0, 400.0)]),
                line(2, "ink", [0.1, 0.1, 0.1, 1.0], 6.0, &[(100.0, 200.0), (180.0, 200.0)]),
                line(
                    3,
                    "eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    16.0,
                    &[(165.0, 185.0), (165.0, 215.0)],
                ),
            ],
            &brushes,
        );
        assert_eq!(marks[2].as_stroke().unwrap().on, vec![2]);
        let mut change = MarkChange::Transform {
            ids: vec![2],
            transform: MarkTransform {
                x: 135.0,
                ..Default::default()
            },
        };
        markedit::apply(&mut marks, &mut change, "c1", Default::default()).unwrap();
        let drawn = render(&marks, &brushes);
        assert!(
            pixel(&drawn, 300, 192)[3] > 0.99,
            "the line it landed on is whole under the trim"
        );
        assert!(pixel(&drawn, 305, 200)[3] < 0.01, "the overshoot is still trimmed");
        assert!(pixel(&drawn, 280, 200)[3] > 0.99, "the line itself is still there");
    }

    #[test]
    fn a_blend_across_an_edge_goes_with_the_figure_and_blends_where_it_lands() {
        let brushes = set();
        let mut marks = attached(
            vec![
                line(
                    1,
                    "marker",
                    [0.2, 0.3, 0.8, 1.0],
                    120.0,
                    &[(80.0, 250.0), (680.0, 250.0)],
                ),
                line(
                    2,
                    "marker",
                    [0.9, 0.8, 0.2, 1.0],
                    60.0,
                    &[(150.0, 250.0), (250.0, 250.0)],
                ),
                line(3, "blender", [0.0; 4], 40.0, &[(280.0, 220.0), (280.0, 280.0)]),
            ],
            &brushes,
        );
        assert_eq!(marks[2].as_stroke().unwrap().on, vec![1, 2]);
        let mut change = MarkChange::Transform {
            ids: vec![2],
            transform: MarkTransform {
                x: 300.0,
                ..Default::default()
            },
        };
        markedit::apply(&mut marks, &mut change, "c1", Default::default()).unwrap();
        assert_eq!(
            marks
                .iter()
                .filter_map(Mark::as_stroke)
                .map(|s| (s.id, s.on.clone()))
                .collect::<Vec<_>>(),
            vec![(1, vec![]), (2, vec![]), (3, vec![1]), (4, vec![2])]
        );
        let with = render(&marks, &brushes);
        let without = render(&marks[..3], &brushes);
        let most = (555..610)
            .map(|x| (pixel(&with, x, 250)[2] - pixel(&without, x, 250)[2]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            most > 0.02,
            "the figure's copy of the blend softens its new edge: {most}"
        );
    }

    #[test]
    fn wet_paint_between_an_eraser_and_its_marks_leaves_it_erasing_in_order() {
        let brushes = set();
        let build = |wet: bool| {
            let mut marks = attached(
                vec![
                    line(1, "ink", [0.2, 0.6, 0.2, 1.0], 80.0, &[(400.0, 100.0), (400.0, 400.0)]),
                    line(2, "ink", [0.9, 0.7, 0.1, 1.0], 20.0, &[(100.0, 250.0), (200.0, 250.0)]),
                    line(
                        3,
                        if wet { "wet" } else { "ink" },
                        [0.3, 0.3, 0.9, 1.0],
                        10.0,
                        &[(120.0, 240.0), (160.0, 240.0)],
                    ),
                    line(
                        4,
                        "eraser",
                        [0.0, 0.0, 0.0, 1.0],
                        40.0,
                        &[(150.0, 200.0), (150.0, 300.0)],
                    ),
                ],
                &brushes,
            );
            let mut change = MarkChange::Transform {
                ids: vec![2, 3],
                transform: MarkTransform {
                    x: 250.0,
                    ..Default::default()
                },
            };
            markedit::apply(&mut marks, &mut change, "c1", Default::default()).unwrap();
            pixel(&render(&marks, &brushes), 400, 215)[3]
        };
        assert_eq!(build(false), 1.0);
        assert!(
            build(true) < 0.9,
            "a wet stroke in its range: erased in order, as before"
        );
    }

    #[test]
    fn a_stroke_its_erasers_cover_completely_is_hidden() {
        let brushes = set();
        let marks = attached(
            vec![
                line(1, "ink", [0.0, 0.0, 0.0, 1.0], 6.0, &[(100.0, 100.0), (140.0, 100.0)]),
                line(2, "ink", [0.0, 0.0, 0.0, 1.0], 6.0, &[(100.0, 200.0), (300.0, 200.0)]),
                line(
                    3,
                    "eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    60.0,
                    &[(90.0, 100.0), (150.0, 100.0)],
                ),
                line(
                    4,
                    "eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    60.0,
                    &[(150.0, 200.0), (180.0, 200.0)],
                ),
                line(
                    5,
                    "soft-eraser",
                    [0.0, 0.0, 0.0, 1.0],
                    200.0,
                    &[(200.0, 200.0), (210.0, 200.0)],
                ),
            ],
            &brushes,
        );
        assert_eq!(
            hidden(W, H, &marks, &brushes, None),
            BTreeSet::from([1]),
            "wiped whole, not cut in part or faded"
        );
        assert!(hidden(W, H, &marks, &brushes, Some(&[2])).is_empty());
    }
}
