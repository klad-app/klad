use anyhow::Result;
use klad_core::bundle::Bundle;
use klad_core::color::parse_hex;
use klad_core::doc::{Blend, Document, Layer, Linear, Node, Paint, Radial, Shape, Stop};
use klad_core::rng::Rng;
use klad_core::stroke::{Stroke, marks};
use libm::{cosf, sinf};
use std::f32::consts::{PI, TAU};
use std::path::Path;

struct Scene {
    w: f32,
    h: f32,
    k: f32,
}

impl Scene {
    fn far(&self, x: f32) -> f32 {
        let t = x / self.w * TAU;
        self.h * (0.58 + 0.04 * sinf(t * 1.3 + 0.5) + 0.02 * sinf(t * 3.1))
    }

    fn near(&self, x: f32) -> f32 {
        let t = x / self.w * TAU;
        self.h * (0.74 + 0.05 * sinf(t * 0.8 + 2.0) + 0.015 * sinf(t * 5.0))
    }

    fn ridge(&self, profile: impl Fn(f32) -> f32) -> String {
        let steps = (self.w / 16.0) as usize;
        let ridge: String = (0..=steps)
            .map(|i| {
                let x = self.w * i as f32 / steps as f32;
                format!(" L{x:.1} {:.1}", profile(x))
            })
            .collect();
        format!("M0 {h}{ridge} L{w} {h} Z", w = self.w, h = self.h)
    }
}

fn stroke(id: usize, brush: &str, hex: &str, size: f32, rng: &mut Rng, points: Vec<(f32, f32, f32)>) -> Result<Stroke> {
    Ok(Stroke::new(id as u32, brush, parse_hex(hex)?, size, rng.next_u64() as u32).with_points(&points, 5.0))
}

fn glow_strokes(s: &Scene, rng: &mut Rng) -> Result<Vec<Stroke>> {
    let (cx, cy) = (s.w * 0.72, s.h * 0.24);
    [(0.3, 0.7), (0.18, 0.85), (0.1, 1.0)]
        .iter()
        .enumerate()
        .map(|(i, &(size, p))| {
            let r = 0.035 * s.w;
            let points = (0..=48)
                .map(|j| {
                    let a = j as f32 / 48.0 * TAU;
                    (cx + cosf(a) * r, cy + sinf(a) * r, p)
                })
                .collect();
            stroke(i + 1, "soft", "#f6e3b4", s.w * size, rng, points)
        })
        .collect()
}

fn cloud_strokes(s: &Scene, rng: &mut Rng) -> Result<Vec<Stroke>> {
    let mut strokes: Vec<Stroke> = (0..18)
        .map(|i| {
            let band = s.h * (0.16 + 0.05 * (i % 5) as f32) + rng.signed() * 12.0 * s.k;
            let x0 = rng.range(-0.1, 0.8) * s.w;
            let length = rng.range(0.18, 0.4) * s.w;
            let wave = rng.range(4.0, 12.0) * s.k;
            let phase = rng.range(0.0, TAU);
            let peak = rng.range(0.55, 0.9);
            let points = (0..=60)
                .map(|j| {
                    let t = j as f32 / 60.0;
                    let taper = (t / 0.2).min(1.0) * ((1.0 - t) / 0.3).min(1.0);
                    (
                        x0 + length * t,
                        band + sinf(t * 7.0 + phase) * wave,
                        peak * (0.25 + 0.75 * taper),
                    )
                })
                .collect();
            let size = rng.range(0.018, 0.03) * s.w;
            stroke(i + 1, "chalk", "#9fb0c8", size, rng, points)
        })
        .collect::<Result<_>>()?;
    let cuts = (0..2)
        .map(|i| {
            let x = s.w * (0.25 + 0.3 * i as f32);
            let points = (0..=20)
                .map(|j| {
                    let t = j as f32 / 20.0;
                    (x + t * 0.04 * s.w, s.h * (0.14 + 0.08 * t), 0.4 + 0.6 * sinf(t * PI))
                })
                .collect();
            stroke(100 + i, "eraser", "#000000", 0.012 * s.w, rng, points)
        })
        .collect::<Result<Vec<_>>>()?;
    strokes.extend(cuts);
    Ok(strokes)
}

fn haze(s: &Scene, rng: &mut Rng) -> Result<Vec<Stroke>> {
    [(0.5, 0.22, 0.8), (0.53, 0.14, 0.6), (0.44, 0.3, 0.5)]
        .iter()
        .enumerate()
        .map(|(i, &(y, size, p))| {
            let points = (0..=40)
                .map(|j| {
                    let t = j as f32 / 40.0;
                    (
                        -0.1 * s.w + 1.2 * s.w * t,
                        s.h * y + sinf(t * 5.0 + i as f32) * 20.0 * s.k,
                        p,
                    )
                })
                .collect();
            stroke(i + 1, "soft", "#3f5f80", size * s.w, rng, points)
        })
        .collect()
}

fn hatching(s: &Scene, rng: &mut Rng) -> Result<Vec<Stroke>> {
    let gap = 7.0 * s.k;
    let columns = (s.w / gap) as usize;
    let hatches: Vec<(f32, f32, f32, f32)> = (0..columns * 3)
        .filter_map(|i| {
            let x = (i % columns) as f32 * gap + rng.signed() * 2.0 * s.k;
            let top = s.far(x) + 0.012 * s.h;
            let bottom = top + 0.35 * (s.near(x) - top).max(1.0);
            let y = rng.range(top, bottom);
            let length = rng.range(0.012, 0.03) * s.w;
            let (ex, ey) = (x + length * 0.5, y - length * 0.86);
            (ey > s.far(ex) + 0.01 * s.h).then_some((x, y, ex, ey))
        })
        .collect();
    hatches
        .into_iter()
        .enumerate()
        .map(|(i, (x, y, ex, ey))| {
            let p = rng.range(0.35, 0.8);
            let points = (0..=10)
                .map(|j| {
                    let t = j as f32 / 10.0;
                    (x + (ex - x) * t, y + (ey - y) * t, p * (0.6 + 0.4 * sinf(t * PI)))
                })
                .collect();
            stroke(i + 1, "pencil", "#7d93ab", 5.0 * s.k, rng, points)
        })
        .collect()
}

fn grass(s: &Scene, rng: &mut Rng) -> Result<Vec<Stroke>> {
    (0..(700.0 * s.k) as usize)
        .map(|i| {
            let x = rng.range(0.0, s.w);
            let base = s.near(x) + 0.006 * s.h;
            let height = rng.range(0.015, 0.055) * s.h;
            let lean = rng.range(-0.35, 0.35);
            let bend = rng.range(-0.25, 0.25);
            let points = (0..=12)
                .map(|j| {
                    let t = j as f32 / 12.0;
                    (
                        x + (lean * t + bend * t * t) * height,
                        base - t * height,
                        1.0 - 0.95 * t,
                    )
                })
                .collect();
            stroke(i + 1, "ink", "#0b1219", rng.range(0.004, 0.007) * s.w, rng, points)
        })
        .collect()
}

fn vertical(top: f32, bottom: f32, stops: &[(f32, &str)]) -> Paint {
    Paint::Linear {
        linear: Linear {
            from: [0.0, top],
            to: [0.0, bottom],
            stops: stops.iter().map(|(at, c)| Stop(*at, c.to_string())).collect(),
        },
    }
}

fn shape(id: &str, name: &str, shape: Shape, fill: Paint) -> Node {
    let mut node = Node::new(id, shape);
    node.name = name.to_string();
    node.fill = Some(fill);
    node
}

pub fn write(path: &Path, size: u32, seed: u64) -> Result<Bundle> {
    let s = Scene {
        w: size as f32,
        h: size as f32,
        k: size as f32 / 2048.0,
    };
    let mut rng = Rng::new(seed);
    let mut doc = Document::new(size, size);
    let sky = vec![
        shape(
            "n1",
            "sky",
            Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: s.w,
                h: s.h,
                radius: 0.0,
            },
            vertical(0.0, s.h * 0.62, &[(0.0, "#121d31"), (0.6, "#24395a"), (1.0, "#4a6a8c")]),
        ),
        shape(
            "n2",
            "moon",
            Shape::Ellipse {
                cx: s.w * 0.72,
                cy: s.h * 0.24,
                rx: s.w * 0.05,
                ry: s.w * 0.05,
            },
            Paint::Radial {
                radial: Radial {
                    center: [s.w * 0.70, s.h * 0.225],
                    radius: s.w * 0.06,
                    stops: vec![Stop(0.0, "#fffbea".into()), Stop(1.0, "#e9d9ae".into())],
                },
            },
        ),
    ];
    let mut glow = Layer::paint("glow", "moon glow", "c3");
    glow.blend = Blend::Screen;
    glow.opacity = 0.9;
    let mut clouds = Layer::paint("clouds", "clouds", "c4");
    clouds.opacity = 0.85;
    let mut hatch = Layer::paint("hatch", "hill hatching", "c6");
    hatch.opacity = 0.45;
    let far = shape(
        "n3",
        "far ridge",
        Shape::Path {
            d: s.ridge(|x| s.far(x)),
        },
        vertical(s.h * 0.52, s.h, &[(0.0, "#2c4560"), (1.0, "#1c2b3d")]),
    );
    let near = shape(
        "n4",
        "near ridge",
        Shape::Path {
            d: s.ridge(|x| s.near(x)),
        },
        vertical(s.h * 0.68, s.h, &[(0.0, "#152230"), (1.0, "#0b121a")]),
    );
    doc.layers = vec![
        Layer::vector("sky", "sky", "c1", sky),
        Layer::paint("haze", "horizon haze", "c2"),
        glow,
        clouds,
        Layer::vector("far", "far hills", "c5", vec![far]),
        hatch,
        Layer::group(
            "front",
            "foreground",
            vec![
                Layer::vector("near", "near hills", "c7", vec![near]),
                Layer::paint("grass", "grass", "c8"),
            ],
        ),
    ];
    doc.next_id = 8;
    let bundle = Bundle::create(path, doc)?;
    bundle.write_marks("haze", "c2", &marks(haze(&s, &mut rng)?))?;
    bundle.write_marks("glow", "c3", &marks(glow_strokes(&s, &mut rng)?))?;
    bundle.write_marks("clouds", "c4", &marks(cloud_strokes(&s, &mut rng)?))?;
    bundle.write_marks("hatch", "c6", &marks(hatching(&s, &mut rng)?))?;
    bundle.write_marks("grass", "c8", &marks(grass(&s, &mut rng)?))?;
    Ok(bundle)
}
