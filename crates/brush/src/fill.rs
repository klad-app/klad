use crate::brush::StrokeBlend;
use crate::engine::blend_coverage;
use crate::tiles::{Raster, TILE, TILE_PX};
use half::f16;
use klad_core::fill::{Coverage, Fill, Seed};
use rayon::prelude::*;

const SUB: usize = 4;

fn f16_floor_bits(v: f32) -> u16 {
    if v <= 0.0 {
        return 0;
    }
    let bits = f16::from_f32(v).to_bits();
    if f16::from_bits(bits).to_f32() < v {
        bits + 1
    } else {
        bits
    }
}

fn f16_ceil_bits(v: f32) -> u16 {
    let bits = f16::from_f32(v.min(65504.0)).to_bits();
    if f16::from_bits(bits).to_f32() > v {
        bits.saturating_sub(1)
    } else {
        bits
    }
}

#[derive(Clone, Copy)]
struct Accept {
    lo: [u16; 4],
    hi: [u16; 4],
}

impl Accept {
    fn around(px: [f16; 4], tolerance: f32) -> Self {
        let v = px.map(f16::to_f32);
        Self {
            lo: v.map(|c| f16_floor_bits(c - tolerance)),
            hi: v.map(|c| f16_ceil_bits(c + tolerance)),
        }
    }

    fn takes(&self, px: &[f16]) -> bool {
        (0..4).all(|c| {
            let bits = px[c].to_bits() & 0x7fff;
            bits >= self.lo[c] && bits <= self.hi[c]
        })
    }
}

fn pixel(raster: &Raster, x: usize, y: usize) -> [f16; 4] {
    match raster.get(x / TILE, y / TILE) {
        Some(tile) => {
            let at = ((y % TILE) * TILE + x % TILE) * 4;
            [tile[at], tile[at + 1], tile[at + 2], tile[at + 3]]
        }
        None => [f16::ZERO; 4],
    }
}

fn barrier(reference: &Raster, accept: &Accept) -> Vec<u8> {
    let (w, h) = (reference.width as usize, reference.height as usize);
    let empty = !accept.takes(&[f16::ZERO; 4]) as u8;
    let mut out = vec![0u8; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let ty = y / TILE;
        (0..reference.cols).for_each(|tx| {
            let x0 = tx * TILE;
            let x1 = (x0 + TILE).min(w);
            match reference.get(tx, ty) {
                None => row[x0..x1].fill(empty),
                Some(tile) => {
                    let line = &tile[(y % TILE) * TILE * 4..];
                    row[x0..x1].iter_mut().enumerate().for_each(|(i, b)| {
                        *b = !accept.takes(&line[i * 4..i * 4 + 4]) as u8;
                    });
                }
            }
        });
    });
    out
}

pub fn distance_sq(mask: &[u8], w: usize, h: usize, reach: usize) -> Vec<u32> {
    let cap = (reach + 1).min(u16::MAX as usize - 1) as u16;
    let mut horizontal = vec![cap; w * h];
    horizontal
        .par_chunks_mut(w)
        .zip(mask.par_chunks(w))
        .for_each(|(dist, row)| {
            let mut last: Option<usize> = None;
            (0..w).for_each(|x| {
                if row[x] != 0 {
                    last = Some(x);
                }
                if let Some(l) = last {
                    dist[x] = dist[x].min((x - l).min(cap as usize) as u16);
                }
            });
            last = None;
            (0..w).rev().for_each(|x| {
                if row[x] != 0 {
                    last = Some(x);
                }
                if let Some(l) = last {
                    dist[x] = dist[x].min((l - x).min(cap as usize) as u16);
                }
            });
        });
    let far = (cap as u32) * (cap as u32);
    let mut out = vec![far; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let rows = y.saturating_sub(reach)..(y + reach + 1).min(h);
        row.iter_mut().enumerate().for_each(|(x, o)| {
            *o = rows.clone().fold(far, |best, yy| {
                let dx = horizontal[yy * w + x] as u32;
                let dy = yy.abs_diff(y) as u32;
                best.min(dx * dx + dy * dy)
            });
        });
    });
    out
}

pub fn dilate(mask: &[u8], w: usize, h: usize, radius: f32) -> Vec<u8> {
    if radius <= 0.0 {
        return mask.to_vec();
    }
    let limit = (radius * radius).floor() as u32;
    distance_sq(mask, w, h, radius.floor() as usize)
        .par_iter()
        .map(|&d| (d <= limit) as u8)
        .collect()
}

fn spread(allowed: &[u8], region: &mut [u8], w: usize, h: usize, x: usize, y: usize) {
    let open = |region: &[u8], x: usize, y: usize| allowed[y * w + x] != 0 && region[y * w + x] == 0;
    if x >= w || y >= h || !open(region, x, y) {
        return;
    }
    let mut stack = vec![(x, y)];
    while let Some((x, y)) = stack.pop() {
        if !open(region, x, y) {
            continue;
        }
        let mut left = x;
        while left > 0 && open(region, left - 1, y) {
            left -= 1;
        }
        let mut right = x;
        while right + 1 < w && open(region, right + 1, y) {
            right += 1;
        }
        region[y * w + left..=y * w + right].fill(1);
        [y.checked_sub(1), (y + 1 < h).then_some(y + 1)]
            .into_iter()
            .flatten()
            .for_each(|ny| {
                let mut inside = false;
                (left..=right).for_each(|nx| {
                    let here = open(region, nx, ny);
                    if here && !inside {
                        stack.push((nx, ny));
                    }
                    inside = here;
                });
            });
    }
}

pub fn flood_region(allowed: &[u8], w: usize, h: usize, x: usize, y: usize) -> Vec<u8> {
    let mut region = vec![0u8; w * h];
    spread(allowed, &mut region, w, h, x, y);
    region
}

fn real_regions(free: &[u8], inner: &[u8], deep: &[u8], w: usize, h: usize) -> Vec<u8> {
    let others = and(free, |i| inner[i] == 0);
    let mut rivals = vec![0u8; w * h];
    (0..w * h)
        .filter(|&i| deep[i] == 0 && others[i] != 0)
        .for_each(|i| spread(&others, &mut rivals, w, h, i % w, i / w));
    (0..w * h)
        .into_par_iter()
        .map(|i| (inner[i] != 0 || rivals[i] != 0) as u8)
        .collect()
}

const OWN: u8 = 1;
const RIVAL: u8 = 2;

fn neighbours(i: usize, w: usize, h: usize) -> impl Iterator<Item = (usize, u32)> {
    let (x, y) = ((i % w) as isize, (i / w) as isize);
    [
        (-1, 0, 3),
        (1, 0, 3),
        (0, -1, 3),
        (0, 1, 3),
        (-1, -1, 4),
        (1, -1, 4),
        (-1, 1, 4),
        (1, 1, 4),
    ]
    .into_iter()
    .filter_map(move |(dx, dy, cost)| {
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h).then(|| (ny as usize * w + nx as usize, cost))
    })
}

fn nearest_free(open: &[u8], free: &[u8], w: usize, h: usize, from: usize, limit: usize) -> Option<usize> {
    let mut seen = vec![false; w * h];
    let mut ring = vec![from];
    seen[from] = true;
    (0..=limit).find_map(|_| {
        if let Some(&hit) = ring.iter().find(|&&i| free[i] != 0) {
            return Some(Some(hit));
        }
        let next: Vec<usize> = ring
            .iter()
            .flat_map(|&i| neighbours(i, w, h).map(|(n, _)| n))
            .filter(|&n| open[n] != 0 && !std::mem::replace(&mut seen[n], true))
            .collect();
        if next.is_empty() {
            return Some(None);
        }
        ring = next;
        None
    })?
}

fn claim(open: &[u8], free: &[u8], inner: &[u8], w: usize, h: usize) -> Vec<u8> {
    let band = |i: usize| open[i] != 0 && free[i] == 0;
    let sources: Vec<usize> = (0..h)
        .into_par_iter()
        .flat_map_iter(|y| (y * w..(y + 1) * w).filter(|&i| free[i] != 0 && neighbours(i, w, h).any(|(n, _)| band(n))))
        .collect();
    let mut dist = vec![u32::MAX; w * h];
    let mut label = vec![0u8; w * h];
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new()];
    sources.iter().for_each(|&i| {
        dist[i] = 0;
        label[i] = if inner[i] != 0 { OWN } else { RIVAL };
        buckets[0].push(i);
    });
    let mut d = 0;
    while d < buckets.len() {
        let list = std::mem::take(&mut buckets[d]);
        list.into_iter().for_each(|i| {
            if dist[i] != d as u32 {
                return;
            }
            neighbours(i, w, h).filter(|&(n, _)| band(n)).for_each(|(n, cost)| {
                let nd = d as u32 + cost;
                if nd < dist[n] || (nd == dist[n] && label[i] == OWN && label[n] == RIVAL) {
                    dist[n] = nd;
                    label[n] = label[i];
                    let at = nd as usize;
                    if buckets.len() <= at {
                        buckets.resize_with(at + 1, Vec::new);
                    }
                    buckets[at].push(n);
                }
            });
        });
        d += 1;
    }
    (0..w * h)
        .into_par_iter()
        .map(|i| (inner[i] != 0 || (band(i) && label[i] == OWN)) as u8)
        .collect()
}

fn and(a: &[u8], b: impl Fn(usize) -> bool + Sync) -> Vec<u8> {
    a.par_iter().enumerate().map(|(i, &v)| (v != 0 && b(i)) as u8).collect()
}

pub fn flood(reference: &Raster, seed: &Seed) -> Coverage {
    let (w, h) = (reference.width as usize, reference.height as usize);
    if seed.x < 0.0 || seed.y < 0.0 || seed.x >= w as f32 || seed.y >= h as f32 {
        return Coverage::default();
    }
    let (sx, sy) = (seed.x as usize, seed.y as usize);
    let accept = Accept::around(pixel(reference, sx, sy), seed.tolerance.clamp(0.0, 1.0));
    let lines = barrier(reference, &accept);
    let open: Vec<u8> = lines.par_iter().map(|&b| (b == 0) as u8).collect();
    let radius = (seed.gap.max(0.0) + 1.0) / 2.0;
    let region = if seed.gap > 0.0 {
        let distance = distance_sq(&lines, w, h, (radius + 2.0).floor() as usize);
        let (near, far) = (
            (radius * radius).floor() as u32,
            ((radius + 2.0) * (radius + 2.0)).floor() as u32,
        );
        let free: Vec<u8> = distance.par_iter().map(|&d| (d > near) as u8).collect();
        let deep: Vec<u8> = distance.par_iter().map(|&d| (d <= far) as u8).collect();
        match nearest_free(&open, &free, w, h, sy * w + sx, (radius * 2.0).ceil() as usize + 2) {
            Some(start) => {
                let inner = flood_region(&free, w, h, start % w, start / w);
                let real = real_regions(&free, &inner, &deep, w, h);
                claim(&open, &real, &inner, w, h)
            }
            None => flood_region(&open, w, h, sx, sy),
        }
    } else {
        flood_region(&open, w, h, sx, sy)
    };
    let grown = if seed.expand > 0.0 {
        let wide = dilate(&region, w, h, seed.expand);
        and(&wide, |i| region[i] != 0 || lines[i] != 0)
    } else {
        region.clone()
    };
    let alpha = match seed.expand > 0.0 {
        true => feathered(&region, &grown, w, h),
        false => grown.par_iter().map(|&v| v * 255).collect(),
    };
    Coverage::from_mask(reference.width, reference.height, &alpha)
}

fn feathered(region: &[u8], grown: &[u8], w: usize, h: usize) -> Vec<u8> {
    const WEIGHTS: [[u32; 3]; 3] = [[1, 2, 1], [2, 4, 2], [1, 2, 1]];
    let near = |x: usize, y: usize, of: &[u8]| -> u32 {
        (0..3)
            .flat_map(|dy| (0..3).map(move |dx| (dx, dy)))
            .filter_map(|(dx, dy)| {
                let (nx, ny) = ((x + dx).checked_sub(1)?, (y + dy).checked_sub(1)?);
                (nx < w && ny < h && of[ny * w + nx] != 0).then_some(WEIGHTS[dy][dx])
            })
            .sum()
    };
    (0..w * h)
        .into_par_iter()
        .map(|i| match grown[i] != 0 {
            true => 255,
            false => {
                let (x, y) = (i % w, i / w);
                match near(x, y, region) {
                    0 => 0,
                    _ => ((near(x, y, grown) * 255 + 8) / 16).min(127) as u8,
                }
            }
        })
        .collect()
}

pub fn polygon(points: &[[f32; 2]], width: u32, height: u32) -> Coverage {
    let (w, h) = (width as usize, height as usize);
    if points.len() < 3 || w == 0 || h == 0 {
        return Coverage::default();
    }
    let (y_lo, y_hi) = points
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    let rows = (y_lo.floor().max(0.0) as usize)..(y_hi.ceil().clamp(0.0, h as f32) as usize);
    let edges: Vec<([f32; 2], [f32; 2])> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| (*a, *b))
        .filter(|(a, b)| a[1] != b[1])
        .collect();
    let mut alpha = vec![0u8; w * h];
    alpha
        .par_chunks_mut(w)
        .enumerate()
        .filter(|(y, _)| rows.contains(y))
        .for_each(|(y, row)| {
            let mut counts = vec![0i32; w * SUB + 1];
            (0..SUB).for_each(|k| {
                let sy = y as f32 + (k as f32 + 0.5) / SUB as f32;
                let mut crossings: Vec<(f32, i32)> = edges
                    .iter()
                    .filter_map(|(a, b)| {
                        let (lo, hi) = if a[1] < b[1] { (a, b) } else { (b, a) };
                        (sy >= lo[1] && sy < hi[1]).then(|| {
                            let t = (sy - a[1]) / (b[1] - a[1]);
                            (a[0] + (b[0] - a[0]) * t, if a[1] < b[1] { 1 } else { -1 })
                        })
                    })
                    .collect();
                crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
                let mut winding = 0;
                crossings.windows(2).for_each(|pair| {
                    winding += pair[0].1;
                    if winding != 0 {
                        let sub = |x: f32| ((x * SUB as f32 - 0.5).ceil().clamp(0.0, (w * SUB) as f32)) as usize;
                        let (from, to) = (sub(pair[0].0), sub(pair[1].0));
                        if from < to {
                            counts[from] += 1;
                            counts[to] -= 1;
                        }
                    }
                });
            });
            let mut running = 0;
            let per_sub: Vec<i32> = counts[..w * SUB]
                .iter()
                .map(|c| {
                    running += c;
                    running
                })
                .collect();
            row.iter_mut().enumerate().for_each(|(x, a)| {
                let hits: i32 = per_sub[x * SUB..(x + 1) * SUB].iter().sum();
                *a = ((hits * 255 + (SUB * SUB) as i32 / 2) / (SUB * SUB) as i32) as u8;
            });
        });
    Coverage::from_mask(width, height, &alpha)
}

pub fn fill_tiles(raster: &Raster, fill: &Fill) -> Vec<usize> {
    let c = &fill.coverage;
    let (w, h) = (raster.width as usize, raster.height as usize);
    let (x0, y0) = (c.x as usize, c.y as usize);
    let (x1, y1) = ((x0 + c.width as usize).min(w), (y0 + c.height as usize).min(h));
    if c.is_empty() || x0 >= x1 || y0 >= y1 {
        return Vec::new();
    }
    let mut seen = vec![false; raster.cols * raster.rows];
    (y0..y1).for_each(|y| {
        (x0..x1)
            .filter(|&x| c.at(x as u32, y as u32) > 0)
            .for_each(|x| seen[(y / TILE) * raster.cols + x / TILE] = true);
    });
    (y0 / TILE..=(y1 - 1) / TILE)
        .flat_map(|ty| (x0 / TILE..=(x1 - 1) / TILE).map(move |tx| (tx, ty)))
        .map(|(tx, ty)| raster.index(tx, ty))
        .filter(|&i| seen[i])
        .collect()
}

pub fn paint_fill(raster: &mut Raster, fill: &Fill) -> Vec<usize> {
    paint_fill_where(raster, fill, |_| true)
}

pub fn paint_fill_where(raster: &mut Raster, fill: &Fill, allowed: impl Fn(usize) -> bool) -> Vec<usize> {
    let c = &fill.coverage;
    if c.is_empty() {
        return Vec::new();
    }
    let (w, h) = (raster.width as usize, raster.height as usize);
    let (x0, y0) = (c.x as usize, c.y as usize);
    let (x1, y1) = ((x0 + c.width as usize).min(w), (y0 + c.height as usize).min(h));
    if x0 >= x1 || y0 >= y1 {
        return Vec::new();
    }
    let color = fill.color();
    let tiles: Vec<usize> = (y0 / TILE..=(y1 - 1) / TILE)
        .flat_map(|ty| (x0 / TILE..=(x1 - 1) / TILE).map(move |tx| (tx, ty)))
        .map(|(tx, ty)| raster.index(tx, ty))
        .collect();
    tiles
        .into_iter()
        .filter(|&index| {
            if !allowed(index) {
                return false;
            }
            let Some((buffer, paint)) = fill_tile(raster, fill, index) else {
                return false;
            };
            if paint {
                let target = match fill.lock {
                    true => raster.at_mut(index),
                    false => Some(raster.ensure(index)),
                };
                let blend = if fill.lock {
                    StrokeBlend::Atop
                } else {
                    StrokeBlend::Normal
                };
                if let Some(target) = target {
                    blend_coverage(target, &buffer, color, fill.opacity, blend);
                }
            }
            true
        })
        .collect()
}

pub(crate) fn fill_tile(raster: &Raster, fill: &Fill, index: usize) -> Option<(Vec<f32>, bool)> {
    let c = &fill.coverage;
    let (w, h) = (raster.width as usize, raster.height as usize);
    let (x0, y0) = (c.x as usize, c.y as usize);
    let (x1, y1) = ((x0 + c.width as usize).min(w), (y0 + c.height as usize).min(h));
    let (tx, ty) = (index % raster.cols, index / raster.cols);
    let (ox, oy) = (tx * TILE, ty * TILE);
    if c.is_empty() || x0.max(ox) >= x1.min(ox + TILE) || y0.max(oy) >= y1.min(oy + TILE) {
        return None;
    }
    let mut buffer = vec![0.0f32; TILE_PX];
    let mut any = false;
    (y0.max(oy)..y1.min(oy + TILE)).for_each(|y| {
        (x0.max(ox)..x1.min(ox + TILE)).for_each(|x| {
            let a = c.at(x as u32, y as u32);
            if a > 0 {
                buffer[(y - oy) * TILE + x - ox] = a as f32 / 255.0;
                any = true;
            }
        });
    });
    if !any {
        return None;
    }
    let mut paint = true;
    if let Some(factor) = crate::cut::factor(&fill.cuts, (ox, oy)) {
        buffer.iter_mut().zip(&factor).for_each(|(c, k)| *c *= k);
        paint = buffer.iter().any(|c| *c > 0.0);
    }
    Some((buffer, paint))
}

pub fn sample(raster: &Raster, x: f32, y: f32) -> [f32; 4] {
    if x < 0.0 || y < 0.0 || x >= raster.width as f32 || y >= raster.height as f32 {
        return [0.0; 4];
    }
    pixel(raster, x as usize, y as usize).map(f16::to_f32)
}

pub fn accepts(raster: &Raster, x: f32, y: f32, colour: [f32; 4], tolerance: f32) -> bool {
    let px = sample(raster, x, y);
    let accept = Accept::around(colour.map(f16::from_f32), tolerance.clamp(0.0, 1.0));
    accept.takes(&px.map(f16::from_f32))
}

pub fn nearest_accepted(
    raster: &Raster,
    at: [f32; 2],
    colour: [f32; 4],
    tolerance: f32,
    radius: u32,
) -> Option<[f32; 2]> {
    let (cx, cy) = (at[0].floor() as i64, at[1].floor() as i64);
    let r = radius as i64;
    (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter(|(dx, dy)| dx * dx + dy * dy <= r * r)
        .map(|(dx, dy)| (cx + dx, cy + dy, dx * dx + dy * dy))
        .filter(|(x, y, _)| *x >= 0 && *y >= 0 && *x < raster.width as i64 && *y < raster.height as i64)
        .filter(|(x, y, _)| accepts(raster, *x as f32 + 0.5, *y as f32 + 0.5, colour, tolerance))
        .min_by_key(|(x, y, d)| (*d, *y, *x))
        .map(|(x, y, _)| [x as f32 + 0.5, y as f32 + 0.5])
}

pub struct Regions {
    pub width: usize,
    pub height: usize,
    pub labels: Vec<u32>,
    pub sizes: Vec<usize>,
    pub border: Vec<bool>,
    pub bounds: Vec<[u32; 4]>,
    pub centres: Vec<[f32; 2]>,
}

impl Regions {
    pub fn count(&self) -> usize {
        self.sizes.len()
    }

    pub fn at(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.labels[y * self.width + x]
    }
}

pub fn regions(reference: &Raster, colour: [f32; 4], tolerance: f32, gap: f32, margin: f32) -> Regions {
    let (w, h) = (reference.width as usize, reference.height as usize);
    let accept = Accept::around(colour.map(f16::from_f32), tolerance.clamp(0.0, 1.0));
    let lines = barrier(reference, &accept);
    let radius = if gap > 0.0 {
        (gap + 1.0) / 2.0 + margin
    } else {
        margin.max(0.5)
    };
    let distance = distance_sq(&lines, w, h, radius.ceil() as usize + 1);
    let limit = (radius * radius).floor() as u32;
    let core: Vec<bool> = distance.par_iter().map(|&d| d > limit).collect();
    let mut labels = vec![0u32; w * h];
    let (mut sizes, mut border, mut bounds, mut centres) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut stack = Vec::new();
    (0..w * h).for_each(|start| {
        if !core[start] || labels[start] != 0 {
            return;
        }
        let label = sizes.len() as u32 + 1;
        let (mut size, mut edge) = (0usize, false);
        let mut b = [u32::MAX, u32::MAX, 0, 0];
        let (mut sx, mut sy) = (0u64, 0u64);
        labels[start] = label;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            size += 1;
            sx += x as u64;
            sy += y as u64;
            edge |= x == 0 || y == 0 || x + 1 == w || y + 1 == h;
            b = [
                b[0].min(x as u32),
                b[1].min(y as u32),
                b[2].max(x as u32 + 1),
                b[3].max(y as u32 + 1),
            ];
            [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ]
            .into_iter()
            .flatten()
            .for_each(|n| {
                if core[n] && labels[n] == 0 {
                    labels[n] = label;
                    stack.push(n);
                }
            });
        }
        sizes.push(size);
        border.push(edge);
        bounds.push(b);
        centres.push([sx as f32 / size as f32 + 0.5, sy as f32 / size as f32 + 0.5]);
    });
    Regions {
        width: w,
        height: h,
        labels,
        sizes,
        border,
        bounds,
        centres,
    }
}

pub fn carry_seed(reference: &Raster, colour: [f32; 4], seed: &Seed, old: &Coverage) -> Option<[f32; 2]> {
    let found = regions(reference, colour, seed.tolerance, seed.gap, 0.0);
    let pixels: Vec<(u32, u32)> = (0..old.height)
        .flat_map(|row| (0..old.width).map(move |col| (col, row)))
        .filter(|&(col, row)| old.alpha[(row * old.width + col) as usize] > 0)
        .map(|(col, row)| (old.x + col, old.y + row))
        .collect();
    if pixels.is_empty() {
        return None;
    }
    let n = pixels.len() as f32;
    let centre = pixels.iter().fold([0.0f32, 0.0], |[x, y], &(px, py)| {
        [x + (px as f32 + 0.5) / n, y + (py as f32 + 0.5) / n]
    });
    let outside = old.x == 0
        || old.y == 0
        || (old.x + old.width) as usize >= found.width
        || (old.y + old.height) as usize >= found.height;
    let reach = (0.75 * n.sqrt()).max(24.0);
    let score = |l: usize| -> Option<(f32, (i64, i64))> {
        if found.border[l] && !outside {
            return None;
        }
        let [cx, cy] = found.centres[l];
        let distance = ((cx - centre[0]).powi(2) + (cy - centre[1]).powi(2)).sqrt();
        if distance > reach * 3.0 {
            return None;
        }
        let shift = ((cx - centre[0]).round() as i64, (cy - centre[1]).round() as i64);
        let label = l as u32 + 1;
        let inside = pixels
            .iter()
            .filter(|&&(x, y)| {
                let (sx, sy) = (x as i64 + shift.0, y as i64 + shift.1);
                sx >= 0 && sy >= 0 && found.at(sx as usize, sy as usize) == label
            })
            .count() as f32;
        let union = n + found.sizes[l] as f32 - inside;
        Some((inside / union.max(1.0) * libm::expf(-distance / reach), shift))
    };
    let (best, (_, shift)) = (0..found.count())
        .filter_map(|l| score(l).map(|s| (l, s)))
        .filter(|(_, (s, _))| *s > 0.0)
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0).then(b.0.cmp(&a.0)))?;
    let [bx0, by0, bx1, by1] = found.bounds[best];
    let label = best as u32 + 1;
    let target = (seed.x + shift.0 as f32, seed.y + shift.1 as f32);
    (by0..by1)
        .flat_map(|y| (bx0..bx1).map(move |x| (x, y)))
        .filter(|&(x, y)| found.at(x as usize, y as usize) == label)
        .map(|(x, y)| {
            let (dx, dy) = (x as f32 + 0.5 - target.0, y as f32 + 0.5 - target.1);
            ((dx * dx + dy * dy) as u64, y, x)
        })
        .min()
        .map(|(_, y, x)| [x as f32 + 0.5, y as f32 + 0.5])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BrushSet, replay};
    use klad_core::fill::{FillRef, FillSource};
    use klad_core::stroke::{Mark, Stroke};
    use std::time::Instant;

    fn rings(eye: (f32, f32)) -> Raster {
        let circle = |cx: f32, cy: f32, r: f32| -> Vec<(f32, f32, f32)> {
            (0..=180)
                .map(|i| {
                    let t = i as f32 / 180.0 * std::f32::consts::TAU;
                    (cx + r * libm::cosf(t), cy + r * libm::sinf(t), 1.0)
                })
                .collect()
        };
        let marks: Vec<Mark> = [circle(100.0, 100.0, 70.0), circle(eye.0, eye.1, 9.0)]
            .iter()
            .enumerate()
            .map(|(i, pts)| {
                Stroke::new(i as u32 + 1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 1)
                    .with_points(pts, 4.0)
                    .into()
            })
            .collect();
        replay(200, 200, &marks, &BrushSet::presets()).unwrap().raster
    }

    #[test]
    fn regions_and_carried_seeds_follow_a_small_shape_past_its_own_size() {
        let before = rings((80.0, 90.0));
        let seed = Seed {
            x: 80.0,
            y: 90.0,
            tolerance: 0.1,
            gap: 2.0,
            expand: 2.0,
        };
        let eye = flood(&before, &seed);
        assert!((100..400).contains(&eye.pixels()), "{}", eye.pixels());
        let found = regions(&before, [0.0; 4], 0.1, 2.0, 0.0);
        assert_eq!(found.border.iter().filter(|b| **b).count(), 1);
        assert!(found.count() >= 3);
        let after = rings((86.0, 112.0));
        let [x, y] = carry_seed(&after, [0.0; 4], &seed, &eye).unwrap();
        assert!((x - 86.0).abs() < 6.0 && (y - 112.0).abs() < 6.0, "{x},{y}");
        let moved = flood(&after, &Seed { x, y, ..seed });
        assert!(
            (moved.pixels() as i64 - eye.pixels() as i64).abs() < 40,
            "{} vs {}",
            moved.pixels(),
            eye.pixels()
        );
    }

    fn ink_ring(size: u32, gaps: &[(f32, f32)]) -> Raster {
        let (cx, cy, r) = (size as f32 / 2.0, size as f32 / 2.0, size as f32 * 0.35);
        let n = 720;
        let points: Vec<(f32, f32, f32)> = (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32 * std::f32::consts::TAU;
                (cx + r * libm::cosf(t), cy + r * libm::sinf(t), 1.0)
            })
            .collect();
        let arcs: Vec<Vec<(f32, f32, f32)>> = if gaps.is_empty() {
            vec![points]
        } else {
            let mut cuts: Vec<(usize, usize)> = gaps
                .iter()
                .map(|&(at, width)| {
                    let mid = (at / 360.0 * n as f32) as usize;
                    let half = ((width + 4.0) / 2.0 / (r * std::f32::consts::TAU) * n as f32).round() as usize;
                    (mid - half, mid + half)
                })
                .collect();
            cuts.sort();
            let mut arcs = Vec::new();
            let mut from = 0;
            cuts.iter().for_each(|&(a, b)| {
                arcs.push(points[from..=a].to_vec());
                from = b;
            });
            arcs.push(points[from..].to_vec());
            arcs
        };
        let marks: Vec<Mark> = arcs
            .iter()
            .enumerate()
            .map(|(i, arc)| {
                Mark::Stroke(Stroke::new(i as u32 + 1, "ink", [0.0, 0.0, 0.0, 1.0], 4.0, 3).with_points(arc, 4.0))
            })
            .collect();
        replay(size, size, &marks, &BrushSet::presets()).unwrap().raster
    }

    fn seed(x: f32, y: f32, gap: f32) -> Seed {
        Seed {
            x,
            y,
            tolerance: 0.1,
            gap,
            expand: 2.0,
        }
    }

    #[test]
    fn a_closed_ring_fills_inside_and_tucks_under_the_line() {
        let lines = ink_ring(256, &[]);
        let c = flood(&lines, &seed(128.0, 128.0, 0.0));
        let r = 256.0 * 0.35;
        assert_eq!(c.at(128, 128), 255);
        assert_eq!(c.at(128 + r as u32 - 3, 128), 255);
        assert_eq!(c.at(128 + r as u32 + 4, 128), 0);
        assert_eq!(c.at(5, 5), 0);
        let [x0, _, x1, _] = c.bounds().unwrap();
        assert!((x1 - x0) > 2.0 * r - 4.0 && (x1 - x0) < 2.0 * r + 4.0, "{x0} {x1}");
        let outside = flood(&lines, &seed(5.0, 5.0, 0.0));
        assert_eq!((outside.at(5, 5), outside.at(128, 128)), (255, 0));
    }

    fn ink(r: &mut Raster, x: usize, y: usize) {
        let at = ((y % TILE) * TILE + x % TILE) * 4;
        let index = r.index(x / TILE, y / TILE);
        r.ensure(index)[at..at + 4].copy_from_slice(&[f16::ZERO, f16::ZERO, f16::ZERO, f16::ONE]);
    }

    fn open_box(gaps: [usize; 3]) -> Raster {
        let mut lines = Raster::new(256, 256);
        (40..216).for_each(|i| {
            (0..3).for_each(|t| {
                let gap = |edge: usize| (128..128 + gaps[edge]).contains(&i);
                [
                    (!gap(0)).then_some((i, 40 + t)),
                    (!gap(1)).then_some((213 + t, i)),
                    (!gap(2)).then_some((40 + t, i)),
                    Some((i, 213 + t)),
                ]
                .into_iter()
                .flatten()
                .for_each(|(x, y)| ink(&mut lines, x, y));
            })
        });
        lines
    }

    #[test]
    fn gaps_up_to_the_gap_width_do_not_leak() {
        let lines = open_box([6, 4, 1]);
        let leaky = flood(&lines, &seed(128.0, 128.0, 0.0));
        assert_eq!(leaky.at(3, 3), 255, "without gap closing the fill leaks");
        let c = flood(&lines, &seed(128.0, 128.0, 6.0));
        assert_eq!((c.at(128, 128), c.at(43, 43), c.at(212, 212)), (255, 255, 255));
        assert_eq!((c.at(3, 3), c.at(250, 128), c.at(128, 30)), (0, 0, 0));
        let [x0, y0, x1, y1] = c.bounds().unwrap();
        assert!(
            x0 >= 36.0 && y0 >= 36.0 && x1 <= 220.0 && y1 <= 220.0,
            "{:?}",
            c.bounds()
        );
        let narrow = flood(&lines, &seed(128.0, 128.0, 4.0));
        assert_eq!(narrow.at(3, 3), 255, "a 6 px gap is wider than gap 4");
        let inked = ink_ring(256, &[(10.0, 5.0), (130.0, 3.0), (250.0, 1.0)]);
        assert_eq!(flood(&inked, &seed(128.0, 128.0, 0.0)).at(3, 3), 255);
        let ring = flood(&inked, &seed(128.0, 128.0, 6.0));
        let r = 256.0 * 0.35;
        assert_eq!((ring.at(128, 128), ring.at(3, 3), ring.at(250, 128)), (255, 0, 0));
        assert_eq!(ring.at(128 - (r as u32 - 5), 128), 255);
        let [x0, y0, x1, y1] = ring.bounds().unwrap();
        let reach = r + 10.0;
        assert!(
            x0 > 128.0 - reach && y0 > 128.0 - reach && x1 < 128.0 + reach && y1 < 128.0 + reach,
            "{:?}",
            ring.bounds()
        );
    }

    #[test]
    fn corners_fill_right_up_to_the_lines_with_gap_closing() {
        let mut lines = Raster::new(128, 128);
        (20..100).for_each(|i| {
            [(i, 20), (i, 99), (20, i), (99, i)]
                .iter()
                .for_each(|&(x, y)| ink(&mut lines, x, y));
        });
        let c = flood(&lines, &seed(60.0, 60.0, 8.0));
        assert_eq!(c.at(21, 21), 255);
        assert_eq!(c.at(98, 98), 255);
        assert_eq!(c.at(20, 20), 255);
        assert_eq!(c.at(10, 10), 0);
    }

    #[test]
    fn two_fills_meet_on_a_straight_line_across_a_gap() {
        let lines = open_box([12, 0, 0]);
        let inside = flood(&lines, &seed(128.0, 128.0, 12.0));
        let outside = flood(&lines, &seed(3.0, 3.0, 12.0));
        assert_eq!((inside.at(3, 3), outside.at(128, 128)), (0, 0));
        (128..140).for_each(|x| {
            let column: Vec<(u8, u8)> = (34..50).map(|y| (inside.at(x, y), outside.at(x, y))).collect();
            assert!(
                column.iter().all(|(a, b)| *a > 0 || *b > 0),
                "hole at x {x}: {column:?}"
            );
            let first_inside = column.iter().position(|(a, _)| *a > 0).unwrap();
            let last_outside = column.iter().rposition(|(_, b)| *b > 0).unwrap();
            assert!(last_outside <= first_inside + 2, "x {x}: {column:?}");
            assert!(
                column[first_inside].0 < 255 && column[last_outside].1 < 255,
                "each edge across the gap is feathered by a pixel: {column:?}"
            );
            assert!(
                (40..=44).contains(&(34 + first_inside)),
                "x {x}: inside starts at {}",
                34 + first_inside
            );
        });
    }

    #[test]
    fn sharp_points_fill_to_the_tip_and_clicks_near_a_line_still_close_gaps() {
        let mut lines = Raster::new(256, 256);
        let segment = |r: &mut Raster, a: (f32, f32), b: (f32, f32)| {
            (0..=400).for_each(|k| {
                let t = k as f32 / 400.0;
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                (0..3).for_each(|d| ink(r, x as usize + d, y as usize));
            })
        };
        segment(&mut lines, (40.0, 200.0), (128.0, 30.0));
        segment(&mut lines, (128.0, 30.0), (216.0, 200.0));
        segment(&mut lines, (40.0, 200.0), (120.0, 200.0));
        segment(&mut lines, (128.0, 200.0), (216.0, 200.0));
        let c = flood(&lines, &seed(128.0, 150.0, 10.0));
        assert_eq!(c.at(3, 3), 0, "the 5 px gap at the bottom is closed");
        assert_eq!(c.at(129, 44), 255, "the sharp top fills to near its tip");
        let near = flood(&lines, &seed(128.0, 195.0, 10.0));
        assert_eq!(near.at(3, 3), 0, "a click a few px from a line does not leak");
        assert_eq!(near.at(128, 100), 255);
    }

    #[test]
    fn dilation_is_a_round_disk() {
        let (w, h) = (21, 21);
        let mut mask = vec![0u8; w * h];
        mask[10 * w + 10] = 1;
        let disk = dilate(&mask, w, h, 5.0);
        assert_eq!(disk.iter().filter(|&&v| v == 1).count(), 81);
        assert_eq!(
            (
                disk[10 * w + 15],
                disk[10 * w + 16],
                disk[14 * w + 13],
                disk[14 * w + 14]
            ),
            (1, 0, 1, 0)
        );
    }

    #[test]
    fn polygons_cover_their_area_with_soft_edges() {
        let c = polygon(&[[10.0, 10.0], [50.0, 10.0], [50.0, 30.0], [10.0, 30.0]], 64, 64);
        assert_eq!((c.x, c.y, c.width, c.height), (10, 10, 40, 20));
        assert_eq!(c.pixels(), 800);
        let half = polygon(&[[10.5, 10.0], [50.0, 10.0], [50.0, 30.0], [10.5, 30.0]], 64, 64);
        assert_eq!(half.at(10, 20), 128);
        let tri = polygon(&[[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]], 64, 64);
        let area: f32 = tri.alpha.iter().map(|&a| a as f32 / 255.0).sum();
        assert!((area - 800.0).abs() < 8.0, "{area}");
        let bow = polygon(&[[0.0, 0.0], [20.0, 20.0], [20.0, 0.0], [0.0, 20.0]], 32, 32);
        assert!(bow.at(3, 10) > 200 && bow.at(10, 3) == 0);
    }

    #[test]
    fn fills_replay_between_strokes_in_order() {
        let brushes = BrushSet::presets();
        let line = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 10.0, 1)
            .with_points(&[(10.0, 32.0, 1.0), (120.0, 32.0, 1.0)], 5.0);
        let mut mask = vec![0u8; 128 * 64];
        mask.iter_mut().for_each(|a| *a = 255);
        let fill = Fill::new(
            2,
            [1.0, 0.0, 0.0, 1.0],
            FillSource::Seed {
                seed: seed(1.0, 1.0, 0.0),
                reference: FillRef::This,
            },
            Coverage::from_mask(128, 64, &mask),
        );
        let after = replay(
            128,
            64,
            &[Mark::Stroke(line.clone()), Mark::Fill(fill.clone())],
            &brushes,
        )
        .unwrap();
        let before = replay(128, 64, &[Mark::Fill(fill), Mark::Stroke(line)], &brushes).unwrap();
        let red = |r: &Raster| pixel(r, 60, 32)[0].to_f32();
        assert_eq!(red(&after.raster), 1.0);
        assert_eq!(red(&before.raster), 0.0);
        assert_eq!(pixel(&before.raster, 60, 5)[3].to_f32(), 1.0);
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-brush fill_timing -- --ignored --nocapture"]
    fn fill_timing() {
        let lines = ink_ring(2048, &[(45.0, 6.0), (135.0, 3.0)]);
        [0.0, 4.0, 8.0].iter().for_each(|&gap| {
            let start = Instant::now();
            let c = flood(&lines, &seed(1024.0, 1024.0, gap));
            println!(
                "2048 fill, gap {gap}: {:.0} ms, {} px",
                start.elapsed().as_secs_f64() * 1000.0,
                c.pixels()
            );
        });
    }
}
