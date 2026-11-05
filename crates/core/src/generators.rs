use crate::doc::Node;
use crate::path::flatten;
use crate::rng::hash2_unit;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub type Affine = [f32; 6];

pub const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
const MAX_COPIES: u32 = 10_000;
const TRIES: i32 = 16;

pub fn then(a: Affine, b: Affine) -> Affine {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn translate(x: f32, y: f32) -> Affine {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

fn rotate(degrees: f32) -> Affine {
    let (s, c) = libm::sincosf(degrees.to_radians());
    [c, s, -s, c, 0.0, 0.0]
}

fn scale(k: f32) -> Affine {
    [k, 0.0, 0.0, k, 0.0, 0.0]
}

fn about(pivot: [f32; 2], m: Affine) -> Affine {
    then(then(translate(pivot[0], pivot[1]), m), translate(-pivot[0], -pivot[1]))
}

pub fn apply(m: Affine, (x, y): (f32, f32)) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scatter {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub count: u32,
    pub seed: u32,
    pub size: [f32; 2],
    pub turn: [f32; 2],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub avoid: Vec<String>,
    pub children: Vec<Node>,
}

impl Default for Scatter {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 200.0,
            count: 12,
            seed: 1,
            size: [0.8, 1.2],
            turn: [-20.0, 20.0],
            avoid: Vec::new(),
            children: Vec::new(),
        }
    }
}

fn inside(polygons: &[Vec<[f32; 2]>], (x, y): (f32, f32)) -> bool {
    polygons
        .iter()
        .map(|ring| {
            ring.iter()
                .zip(ring.iter().cycle().skip(1))
                .filter(|([x0, y0], [x1, y1])| (*y0 > y) != (*y1 > y) && x < x0 + (y - y0) * (x1 - x0) / (y1 - y0))
                .count()
        })
        .sum::<usize>()
        % 2
        == 1
}

impl Scatter {
    pub fn validate(&self) -> Result<()> {
        if self.count > MAX_COPIES {
            bail!("scatter: count {} is over {MAX_COPIES}", self.count);
        }
        if self.w < 0.0 || self.h < 0.0 {
            bail!("scatter: the region needs a positive width and height");
        }
        self.avoid.iter().try_for_each(|d| flatten(d).map(|_| ()))
    }

    pub fn instances(&self) -> Vec<Affine> {
        let avoid: Vec<Vec<[f32; 2]>> = self.avoid.iter().filter_map(|d| flatten(d).ok()).flatten().collect();
        let seed = self.seed ^ 0x5CA7_7E12;
        (0..self.count.min(MAX_COPIES) as i32)
            .filter_map(|i| {
                let at = (0..TRIES).find_map(|t| {
                    let r = |k: i32| hash2_unit(i, t * 8 + k, seed);
                    let p = (self.x + self.w * r(0), self.y + self.h * r(1));
                    (!inside(&avoid, p)).then_some(p)
                })?;
                let r = |k: i32| hash2_unit(i, 1000 + k, seed);
                let size = self.size[0] + (self.size[1] - self.size[0]) * r(0);
                let turn = self.turn[0] + (self.turn[1] - self.turn[0]) * r(1);
                Some(then(then(translate(at.0, at.1), rotate(turn)), scale(size)))
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Repeat {
    pub count: u32,
    pub step: [f32; 2],
    pub turn: f32,
    pub pivot: [f32; 2],
    pub scale: f32,
    pub columns: u32,
    pub row: [f32; 2],
    pub children: Vec<Node>,
}

impl Default for Repeat {
    fn default() -> Self {
        Self {
            count: 4,
            step: [40.0, 0.0],
            turn: 0.0,
            pivot: [0.0, 0.0],
            scale: 1.0,
            columns: 0,
            row: [0.0, 40.0],
            children: Vec::new(),
        }
    }
}

impl Repeat {
    pub fn validate(&self) -> Result<()> {
        if self.count > MAX_COPIES {
            bail!("repeat: count {} is over {MAX_COPIES}", self.count);
        }
        if self.scale <= 0.0 {
            bail!("repeat: scale must be above 0");
        }
        Ok(())
    }

    pub fn instances(&self) -> Vec<Affine> {
        (0..self.count.min(MAX_COPIES))
            .map(|i| {
                let (col, row) = match self.columns {
                    0 => (i, 0),
                    n => (i % n, i / n),
                };
                let offset = (
                    self.step[0] * col as f32 + self.row[0] * row as f32,
                    self.step[1] * col as f32 + self.row[1] * row as f32,
                );
                let grow = libm::powf(self.scale, i as f32);
                then(
                    translate(offset.0, offset.1),
                    about(self.pivot, then(rotate(self.turn * i as f32), scale(grow))),
                )
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mirror {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub children: Vec<Node>,
}

impl Default for Mirror {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            angle: 90.0,
            children: Vec::new(),
        }
    }
}

impl Mirror {
    pub fn reflection(&self) -> Affine {
        let (s, c) = libm::sincosf(2.0 * self.angle.to_radians());
        about([self.x, self.y], [c, s, s, -c, 0.0, 0.0])
    }

    pub fn instances(&self) -> Vec<Affine> {
        vec![IDENTITY, self.reflection()]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Text {
    pub text: String,
    pub font: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub tracking: f32,
    pub leading: f32,
    pub align: Align,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            text: "text".into(),
            font: String::new(),
            x: 0.0,
            y: 0.0,
            size: 32.0,
            tracking: 0.0,
            leading: 1.2,
            align: Align::Left,
            weight: None,
        }
    }
}

impl Text {
    pub fn validate(&self) -> Result<()> {
        if self.font.trim().is_empty() {
            bail!("text needs a font: a file in the bundle's fonts folder");
        }
        if self.size <= 0.0 || self.size > 4096.0 {
            bail!("text: size {} is outside 0..4096", self.size);
        }
        if let Some(w) = self.weight.filter(|w| !(1.0..=1000.0).contains(w)) {
            bail!("text: weight {w} is outside 1..1000 (400 is regular, 700 bold)");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    pub src: String,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Picture {
    pub fn validate(&self) -> Result<()> {
        if self.src.trim().is_empty() || self.src.contains('/') || self.src.contains('\\') {
            bail!(
                "an image's src is a file name in the bundle's images folder, not {:?}",
                self.src
            );
        }
        if self.w <= 0.0 || self.h <= 0.0 {
            bail!("an image needs a width and height above 0");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_repeats_with_its_seed_and_keeps_out_of_what_it_avoids() {
        let s = Scatter {
            count: 200,
            avoid: vec!["M0 0 L200 0 L200 200 L0 200 Z".into()],
            ..Scatter::default()
        };
        let a = s.instances();
        assert_eq!(a, s.instances());
        assert_ne!(a, Scatter { seed: 2, ..s.clone() }.instances());
        assert!(
            a.iter().all(|m| !(m[4] < 200.0 && m[5] < 200.0)),
            "nothing lands in the avoided square"
        );
        assert!(a.len() > 150);
        assert!(
            a.iter()
                .all(|m| m[4] >= 0.0 && m[4] <= 400.0 && m[5] >= 0.0 && m[5] <= 200.0)
        );
    }

    #[test]
    fn repeat_steps_and_mirror_reflects() {
        let r = Repeat {
            count: 6,
            step: [10.0, 0.0],
            columns: 3,
            row: [0.0, 20.0],
            ..Repeat::default()
        };
        let at: Vec<(f32, f32)> = r.instances().iter().map(|m| apply(*m, (0.0, 0.0))).collect();
        assert_eq!(
            at,
            vec![
                (0.0, 0.0),
                (10.0, 0.0),
                (20.0, 0.0),
                (0.0, 20.0),
                (10.0, 20.0),
                (20.0, 20.0)
            ]
        );
        let arc = Repeat {
            count: 4,
            step: [0.0, 0.0],
            turn: 90.0,
            pivot: [0.0, 0.0],
            ..Repeat::default()
        };
        let p = apply(arc.instances()[1], (10.0, 0.0));
        assert!((p.0).abs() < 1e-4 && (p.1 - 10.0).abs() < 1e-4, "{p:?}");
        let m = Mirror {
            x: 100.0,
            ..Mirror::default()
        };
        let q = apply(m.reflection(), (130.0, 40.0));
        assert!((q.0 - 70.0).abs() < 1e-3 && (q.1 - 40.0).abs() < 1e-3, "{q:?}");
    }
}
