use anyhow::Result;
use klad_core::color::parse_hex;
use klad_core::doc::Blend;
use klad_core::look::{self, Displace, Effect, Glow, Haze, Noise, Outline, Relief, Shadow};
use klad_core::rng::hash2_unit;
use rayon::prelude::*;

pub type Px = [f32; 4];

const SHARP: f32 = 2.0;
const MAX_FACTOR: i32 = 32;
const FAR: f64 = 1e20;

#[derive(Clone, Debug, PartialEq)]
pub struct Img {
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub px: Vec<Px>,
}

impl Img {
    pub fn empty(x: i32, y: i32, w: usize, h: usize) -> Self {
        Self {
            x,
            y,
            w,
            h,
            px: vec![[0.0; 4]; w * h],
        }
    }

    pub fn at(&self, gx: i32, gy: i32) -> Px {
        let (lx, ly) = (gx - self.x, gy - self.y);
        if lx < 0 || ly < 0 || lx as usize >= self.w || ly as usize >= self.h {
            return [0.0; 4];
        }
        self.px[ly as usize * self.w + lx as usize]
    }

    fn each(&self, f: impl Fn(i32, i32, Px) -> Px + Sync) -> Img {
        let px = self
            .px
            .par_chunks(self.w.max(1))
            .enumerate()
            .flat_map_iter(|(row, line)| {
                let gy = self.y + row as i32;
                line.iter()
                    .enumerate()
                    .map(move |(col, p)| (self.x + col as i32, gy, *p))
                    .map(|(gx, gy, p)| f(gx, gy, p))
                    .collect::<Vec<_>>()
            })
            .collect();
        Img {
            px,
            ..self.clone_shape()
        }
    }

    pub fn crop(&self, x: i32, y: i32, w: usize, h: usize) -> Img {
        let px = (0..h)
            .flat_map(|row| (0..w).map(move |col| (x + col as i32, y + row as i32)))
            .map(|(gx, gy)| self.at(gx, gy))
            .collect();
        Img { x, y, w, h, px }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Space {
    pub scale: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Space {
    fn canvas(&self, gx: i32, gy: i32) -> (f32, f32) {
        (
            (gx as f32 + 0.5 - self.tx) / self.scale,
            (gy as f32 + 0.5 - self.ty) / self.scale,
        )
    }
}

pub struct Pass {
    pub img: Img,
    pub blend: Blend,
}

fn factor(sigma: f32) -> i32 {
    let mut f = 1;
    while sigma / (2 * f) as f32 >= SHARP && f < MAX_FACTOR {
        f *= 2;
    }
    f
}

pub fn margin(effects: &[Effect], scale: f32) -> i32 {
    let slack: i32 = effects
        .iter()
        .map(|e| match e {
            Effect::Blur(b) => factor(b.radius * scale) * 2 + 2,
            Effect::Glow(g) => factor(g.radius * scale) * 2 + 2,
            Effect::Shadow(s) => factor(s.radius * scale) * 2 + 2,
            Effect::Displace(_) => 2,
            Effect::Relief(r) => factor(r.softness * scale) * 2 + 2,
            _ => 1,
        })
        .sum();
    (look::reach(effects) * scale).ceil() as i32 + slack
}

pub type Layer<'a> = &'a dyn Fn(&str) -> Result<Option<Img>>;
pub type Height<'a> = &'a dyn Fn() -> Result<Option<Img>>;

pub fn run(effects: &[Effect], img: Img, space: Space, by: Layer, height: Height) -> Result<(Vec<Pass>, Img)> {
    let s = space.scale;
    effects.iter().try_fold((Vec::new(), img), |(mut passes, img), effect| {
        let img = match effect {
            Effect::Blur(b) => blur(&img, b.radius * s),
            Effect::Glow(g) => {
                let glow = glow(&img, g, s)?;
                match g.blend {
                    Blend::Normal => over(&img, &glow),
                    blend => {
                        passes.push(Pass { img: glow, blend });
                        img
                    }
                }
            }
            Effect::Outline(o) => outline(&img, o, s)?,
            Effect::Shadow(sh) => over(&img, &shadow(&img, sh, s)?),
            Effect::Noise(n) => noise(&img, n, space),
            Effect::Haze(h) => haze(&img, h, space)?,
            Effect::Displace(d) => {
                let map = match &d.by {
                    Some(id) => by(id)?,
                    None => None,
                };
                displace(&img, d, space, map.as_ref())
            }
            Effect::Relief(r) => match height()? {
                Some(map) => relief(&img, r, s, &map)?,
                None => img,
            },
        };
        Ok((passes, img))
    })
}

const LIFT: f32 = 0.85;
const DROP: f32 = 0.55;
const STACK: f32 = 1.6;

pub fn relief(img: &Img, r: &Relief, scale: f32, height: &Img) -> Result<Img> {
    let [hr, hg, hb, _] = parse_hex(&r.highlight)?;
    let [sr, sg, sb, _] = parse_hex(&r.shade)?;
    let (high, low) = ([hr, hg, hb], [sr, sg, sb]);
    let length = r.light[0].hypot(r.light[1]).max(1e-6);
    let (lx, ly) = (r.light[0] / length, r.light[1] / length);
    let stacked = height.each(|_, _, p| [STACK * libm::tanhf(p[0] / STACK), 0.0, 0.0, p[3]]);
    let soft = blur(&stacked, r.softness * scale);
    let k = r.strength * scale * 0.5;
    Ok(img.each(|gx, gy, p| {
        if p[3] <= 1e-5 {
            return p;
        }
        let h = |dx: i32, dy: i32| soft.at(gx + dx, gy + dy)[0];
        let (hx, hy) = ((h(1, 0) - h(-1, 0)) * k, (h(0, 1) - h(0, -1)) * k);
        let facing = -(hx * lx + hy * ly);
        let e = facing / libm::sqrtf(1.0 + hx * hx + hy * hy);
        if e.abs() < 1e-5 {
            return p;
        }
        let a = p[3];
        let un = [p[0] / a, p[1] / a, p[2] / a];
        let lit = libm::sqrtf((0.2126 * un[0] + 0.7152 * un[1] + 0.0722 * un[2]).clamp(0.0, 1.0));
        let out: [f32; 3] = match e > 0.0 {
            true => {
                let t = (e * LIFT * lit).min(1.0);
                std::array::from_fn(|i| un[i] + (high[i] - un[i]) * t)
            }
            false => {
                let t = (-e * DROP).min(1.0);
                std::array::from_fn(|i| un[i] * (1.0 + (low[i] - 1.0) * t))
            }
        };
        [out[0] * a, out[1] * a, out[2] * a, a]
    }))
}

pub fn over(top: &Img, under: &Img) -> Img {
    top.each(|gx, gy, t| {
        let u = under.at(gx, gy);
        let k = 1.0 - t[3];
        [t[0] + u[0] * k, t[1] + u[1] * k, t[2] + u[2] * k, t[3] + u[3] * k]
    })
}

pub fn blur(img: &Img, sigma: f32) -> Img {
    if sigma < 0.25 || img.w == 0 || img.h == 0 {
        return img.clone();
    }
    let f = factor(sigma);
    if f == 1 {
        return gaussian(img, sigma);
    }
    let small = downsample(img, f);
    let small_sigma = (sigma * sigma - (f * f) as f32 / 4.0).max(1.0).sqrt() / f as f32;
    upsample(&gaussian(&small, small_sigma), f, img)
}

fn downsample(img: &Img, f: i32) -> Img {
    let (cx0, cy0) = (img.x.div_euclid(f), img.y.div_euclid(f));
    let cx1 = (img.x + img.w as i32 - 1).div_euclid(f);
    let cy1 = (img.y + img.h as i32 - 1).div_euclid(f);
    let (w, h) = ((cx1 - cx0 + 1) as usize, (cy1 - cy0 + 1) as usize);
    let area = (f * f) as f32;
    let px = (0..h)
        .into_par_iter()
        .flat_map_iter(|row| {
            let cy = cy0 + row as i32;
            (0..w)
                .map(move |col| {
                    let cx = cx0 + col as i32;
                    let sum = (0..f).fold([0.0f32; 4], |acc, dy| {
                        (0..f).fold(acc, |acc, dx| {
                            let p = img.at(cx * f + dx, cy * f + dy);
                            [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2], acc[3] + p[3]]
                        })
                    });
                    sum.map(|v| v / area)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    Img {
        x: cx0,
        y: cy0,
        w,
        h,
        px,
    }
}

fn upsample(small: &Img, f: i32, like: &Img) -> Img {
    let k = f as f32;
    like.each(|gx, gy, _| {
        let u = (gx as f32 + 0.5) / k - 0.5;
        let v = (gy as f32 + 0.5) / k - 0.5;
        let (i, j) = (u.floor() as i32, v.floor() as i32);
        let (fu, fv) = (u - i as f32, v - j as f32);
        let (a, b, c, d) = (
            small.at(i, j),
            small.at(i + 1, j),
            small.at(i, j + 1),
            small.at(i + 1, j + 1),
        );
        [0, 1, 2, 3].map(|ch| {
            let top = a[ch] + (b[ch] - a[ch]) * fu;
            let bottom = c[ch] + (d[ch] - c[ch]) * fu;
            top + (bottom - top) * fv
        })
    })
}

fn kernel(sigma: f32) -> Vec<f32> {
    let radius = (sigma * 3.0).ceil() as i32;
    let weights: Vec<f32> = (-radius..=radius)
        .map(|i| libm::expf(-((i * i) as f32) / (2.0 * sigma * sigma)))
        .collect();
    let total: f32 = weights.iter().sum();
    weights.iter().map(|w| w / total).collect()
}

fn gaussian(img: &Img, sigma: f32) -> Img {
    let taps = kernel(sigma);
    let reach = (taps.len() / 2) as i32;
    let (w, h) = (img.w as i32, img.h as i32);
    let pass = |src: &[Px], horizontal: bool| -> Vec<Px> {
        (0..h)
            .into_par_iter()
            .flat_map_iter(|y| {
                let taps = &taps;
                (0..w)
                    .map(move |x| {
                        taps.iter().enumerate().fold([0.0f32; 4], |acc, (k, wt)| {
                            let o = k as i32 - reach;
                            let (sx, sy) = match horizontal {
                                true => (x + o, y),
                                false => (x, y + o),
                            };
                            if sx < 0 || sy < 0 || sx >= w || sy >= h {
                                return acc;
                            }
                            let p = src[(sy * w + sx) as usize];
                            [
                                acc[0] + p[0] * wt,
                                acc[1] + p[1] * wt,
                                acc[2] + p[2] * wt,
                                acc[3] + p[3] * wt,
                            ]
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let across = pass(&img.px, true);
    Img {
        px: pass(&across, false),
        ..img.clone_shape()
    }
}

impl Img {
    fn clone_shape(&self) -> Img {
        Img {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            px: Vec::new(),
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn glow(img: &Img, g: &Glow, scale: f32) -> Result<Img> {
    let tint = g.color.as_deref().map(parse_hex).transpose()?;
    let lit = img.each(|_, _, p| {
        let a = p[3];
        if a <= 0.0 {
            return [0.0; 4];
        }
        let weight = match g.threshold > 0.0 {
            true => smoothstep(
                g.threshold,
                g.threshold + 0.1,
                look::luma([p[0] / a, p[1] / a, p[2] / a]),
            ),
            false => 1.0,
        };
        match tint {
            Some(c) => {
                let k = a * weight * c[3];
                [c[0] * k, c[1] * k, c[2] * k, k]
            }
            None => p.map(|v| v * weight),
        }
    });
    let soft = blur(&lit, g.radius * scale);
    Ok(soft.each(|_, _, p| {
        let a = (p[3] * g.strength).clamp(0.0, 1.0);
        let c = |v: f32| (v * g.strength).clamp(0.0, a);
        [c(p[0]), c(p[1]), c(p[2]), a]
    }))
}

fn dt1d(f: &[f64]) -> Vec<f64> {
    let n = f.len();
    if n == 0 {
        return Vec::new();
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let cross =
        |q: usize, p: usize| ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
    (1..n).for_each(|q| {
        let mut s = cross(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = cross(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    });
    let mut k = 0usize;
    (0..n)
        .map(|q| {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let p = v[k];
            let d = q as f64 - p as f64;
            d * d + f[p]
        })
        .collect()
}

fn distances(img: &Img, seed: impl Fn(Px) -> bool + Sync) -> Vec<f32> {
    let (w, h) = (img.w, img.h);
    let start: Vec<f64> = img.px.iter().map(|p| if seed(*p) { 0.0 } else { FAR }).collect();
    let columns: Vec<Vec<f64>> = (0..w)
        .into_par_iter()
        .map(|x| dt1d(&(0..h).map(|y| start[y * w + x]).collect::<Vec<_>>()))
        .collect();
    (0..h)
        .into_par_iter()
        .flat_map_iter(|y| {
            dt1d(&(0..w).map(|x| columns[x][y]).collect::<Vec<_>>())
                .into_iter()
                .map(|d| d.sqrt() as f32)
        })
        .collect()
}

fn outline(img: &Img, o: &Outline, scale: f32) -> Result<Img> {
    let c = parse_hex(&o.color)?;
    let width = o.width * scale;
    let paint = |cov: f32| {
        let a = cov * c[3];
        [c[0] * a, c[1] * a, c[2] * a, a]
    };
    match o.inside {
        false => {
            let d = distances(img, |p| p[3] >= 0.5);
            let ring = Img {
                px: d.iter().map(|d| paint((width + 0.5 - d).clamp(0.0, 1.0))).collect(),
                ..img.clone_shape()
            };
            Ok(over(img, &ring))
        }
        true => {
            let d = distances(img, |p| p[3] < 0.5);
            let edge = Img {
                px: d
                    .iter()
                    .zip(&img.px)
                    .map(|(d, p)| paint((width + 0.5 - d).clamp(0.0, 1.0) * p[3]))
                    .collect(),
                ..img.clone_shape()
            };
            Ok(over(&edge, img))
        }
    }
}

fn shadow(img: &Img, sh: &Shadow, scale: f32) -> Result<Img> {
    let c = parse_hex(&sh.color)?;
    let (dx, dy) = ((sh.x * scale).round() as i32, (sh.y * scale).round() as i32);
    let moved = img.each(|gx, gy, _| [0.0, 0.0, 0.0, img.at(gx - dx, gy - dy)[3]]);
    Ok(blur(&moved, sh.radius * scale).each(|_, _, p| {
        let a = p[3] * c[3];
        [c[0] * a, c[1] * a, c[2] * a, a]
    }))
}

fn noise(img: &Img, n: &Noise, space: Space) -> Img {
    img.each(|gx, gy, p| {
        if p[3] <= 0.0 {
            return p;
        }
        let (cx, cy) = space.canvas(gx, gy);
        let (ix, iy) = ((cx / n.size).floor() as i32, (cy / n.size).floor() as i32);
        let value = |k: u32| hash2_unit(ix, iy, n.seed.wrapping_mul(3).wrapping_add(k)) * 2.0 - 1.0;
        let d = match n.mono {
            true => [value(0); 3],
            false => [value(0), value(1), value(2)],
        };
        let a = p[3];
        [
            (p[0] + d[0] * n.amount * a).clamp(0.0, a),
            (p[1] + d[1] * n.amount * a).clamp(0.0, a),
            (p[2] + d[2] * n.amount * a).clamp(0.0, a),
            a,
        ]
    })
}

fn haze(img: &Img, h: &Haze, space: Space) -> Result<Img> {
    let bottom = parse_hex(&h.color)?;
    let top = h.top.as_deref().map(parse_hex).transpose()?;
    Ok(img.each(|gx, gy, p| {
        let a = p[3];
        if a <= 0.0 {
            return p;
        }
        let fog = match top.filter(|_| h.span > 0.0) {
            Some(t) => {
                let k = (space.canvas(gx, gy).1 / h.span).clamp(0.0, 1.0);
                [0, 1, 2, 3].map(|c| t[c] + (bottom[c] - t[c]) * k)
            }
            None => bottom,
        };
        let un = [p[0] / a, p[1] / a, p[2] / a];
        let l = look::luma(un);
        let k = h.amount.clamp(0.0, 1.0) * fog[3];
        let out = [0, 1, 2].map(|c| {
            let grey = un[c] + (l - un[c]) * h.desaturate.clamp(0.0, 1.0);
            (grey + (fog[c] - grey) * k) * a
        });
        [out[0], out[1], out[2], a]
    }))
}

fn smooth_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (i, j) = (ix as i32, iy as i32);
    let h = |a: i32, b: i32| hash2_unit(a, b, seed);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = h(i, j) + (h(i + 1, j) - h(i, j)) * sx;
    let bottom = h(i, j + 1) + (h(i + 1, j + 1) - h(i, j + 1)) * sx;
    top + (bottom - top) * sy
}

fn sample(img: &Img, gx: i32, gy: i32, dx: f32, dy: f32) -> Px {
    let (fx, fy) = (dx.floor(), dy.floor());
    let (i, j) = (gx + fx as i32, gy + fy as i32);
    let (fu, fv) = (dx - fx, dy - fy);
    let (a, b, c, d) = (img.at(i, j), img.at(i + 1, j), img.at(i, j + 1), img.at(i + 1, j + 1));
    [0, 1, 2, 3].map(|ch| {
        let top = a[ch] + (b[ch] - a[ch]) * fu;
        let bottom = c[ch] + (d[ch] - c[ch]) * fu;
        top + (bottom - top) * fv
    })
}

fn displace(img: &Img, d: &Displace, space: Space, map: Option<&Img>) -> Img {
    let reach = d.amount * space.scale;
    img.each(|gx, gy, _| {
        let (ox, oy) = match map {
            Some(m) => {
                let p = m.at(gx, gy);
                match p[3] > 0.0 {
                    true => ((p[0] / p[3] - 0.5) * 2.0, (p[1] / p[3] - 0.5) * 2.0),
                    false => (0.0, 0.0),
                }
            }
            None => {
                let (cx, cy) = space.canvas(gx, gy);
                let (u, v) = (cx / d.size, cy / d.size);
                (
                    smooth_noise(u, v, d.seed) * 2.0 - 1.0,
                    smooth_noise(u, v, d.seed ^ 0x9E37_79B9) * 2.0 - 1.0,
                )
            }
        };
        sample(img, gx, gy, ox * reach, oy * reach)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(x: i32, y: i32, w: usize, h: usize, centre: (f32, f32), radius: f32) -> Img {
        let mut img = Img::empty(x, y, w, h);
        img.px = (0..h)
            .flat_map(|r| (0..w).map(move |c| (c, r)))
            .map(|(c, r)| {
                let (gx, gy) = (x as f32 + c as f32 + 0.5, y as f32 + r as f32 + 0.5);
                let d = ((gx - centre.0).powi(2) + (gy - centre.1).powi(2)).sqrt();
                let a = (radius - d + 0.5).clamp(0.0, 1.0);
                [a, 0.5 * a, 0.0, a]
            })
            .collect();
        img
    }

    #[test]
    fn a_window_with_its_margin_gives_the_full_image_pixels() {
        let full = disc(-40, -40, 400, 300, (150.0, 140.0), 60.0);
        let list: Vec<Effect> = serde_json::from_str(
            r##"[{"glow":{"radius":10,"color":"#ffd080","blend":"normal"}},{"outline":{"width":4}},{"shadow":{"x":5,"y":-7,"radius":3}},{"noise":{"amount":0.2,"size":3}},{"displace":{"amount":6,"size":20}},{"blur":{"radius":2}},{"outline":{"width":3,"inside":true,"color":"#2040ff"}}]"##,
        )
        .unwrap();
        let space = Space {
            scale: 1.0,
            tx: 0.0,
            ty: 0.0,
        };
        let none = |_: &str| Ok(None);
        let flat = || Ok(None);
        let (_, reference) = run(&list, full.clone(), space, &none, &flat).unwrap();
        let m = margin(&list, 1.0);
        let (tx, ty, tw, th) = (128, 64, 64, 64);
        let window = full.crop(tx - m, ty - m, tw + 2 * m as usize, th + 2 * m as usize);
        let (_, local) = run(&list, window, space, &none, &flat).unwrap();
        let (a, b) = (reference.crop(tx, ty, tw, th), local.crop(tx, ty, tw, th));
        let worst =
            a.px.iter()
                .zip(&b.px)
                .flat_map(|(p, q)| p.iter().zip(q).map(|(x, y)| (x - y).abs()))
                .fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "tile and full image differ by {worst}");
        assert!(a.px.iter().any(|p| p[3] > 0.0) && a.px.iter().any(|p| p[3] < 1.0));
    }

    #[test]
    fn the_outline_rings_the_shape_at_its_width() {
        let shape = disc(0, 0, 100, 100, (50.0, 50.0), 20.0);
        let o = Outline {
            width: 5.0,
            color: "#000000".into(),
            inside: false,
        };
        let out = outline(&shape, &o, 1.0).unwrap();
        assert!(out.at(50 + 23, 50)[3] > 0.99 && out.at(50 + 23, 50)[0] < 0.01);
        assert!(out.at(50 + 27, 50)[3] < 0.01);
        assert_eq!(out.at(50, 50), shape.at(50, 50));
    }

    #[test]
    fn blur_keeps_what_there_is_and_spreads_it() {
        let shape = disc(0, 0, 200, 200, (100.0, 100.0), 30.0);
        let total = |i: &Img| i.px.iter().map(|p| p[3]).sum::<f32>();
        [1.5f32, 6.0, 24.0].iter().for_each(|s| {
            let soft = blur(&shape, *s);
            assert!((total(&soft) - total(&shape)).abs() / total(&shape) < 0.01, "sigma {s}");
            assert!(soft.at(100 + 30 + (*s as i32), 100)[3] > 0.01, "sigma {s}");
        });
    }

    #[test]
    fn noise_and_displacement_repeat_with_their_seed() {
        let shape = disc(0, 0, 64, 64, (32.0, 32.0), 20.0);
        let space = Space {
            scale: 1.0,
            tx: 0.0,
            ty: 0.0,
        };
        let n = Noise {
            amount: 0.3,
            size: 2.0,
            seed: 4,
            mono: false,
        };
        assert_eq!(noise(&shape, &n, space), noise(&shape, &n, space));
        assert_ne!(
            noise(&shape, &n, space),
            noise(&shape, &Noise { seed: 5, ..n.clone() }, space)
        );
        let d = Displace::default();
        assert_eq!(displace(&shape, &d, space, None), displace(&shape, &d, space, None));
        assert_ne!(displace(&shape, &d, space, None), shape);
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    #[ignore = "timing: cargo test --release -p klad-render glow_steps -- --ignored --nocapture"]
    fn glow_steps() {
        use std::time::Instant;
        let mut img = Img::empty(-38, -38, 332, 332);
        img.px.iter_mut().enumerate().for_each(|(i, p)| {
            if (i / 332 + i % 332) % 7 < 3 {
                *p = [0.8, 0.6, 0.2, 1.0];
            }
        });
        let g = Glow {
            radius: 32.0 / 3.0,
            ..Glow::default()
        };
        let best = |f: &dyn Fn()| {
            (0..20)
                .map(|_| {
                    let t = Instant::now();
                    f();
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .fold(f64::MAX, f64::min)
        };
        let each = best(&|| {
            img.each(|_, _, p| p);
        });
        let blurred = best(&|| {
            blur(&img, g.radius);
        });
        let glowed = best(&|| {
            glow(&img, &g, 1.0).unwrap();
        });
        let crop = best(&|| {
            img.crop(0, 0, 256, 256);
        });
        println!("332 px window: each {each:.2} ms, blur {blurred:.2} ms, glow {glowed:.2} ms, crop {crop:.2} ms");
    }
}
