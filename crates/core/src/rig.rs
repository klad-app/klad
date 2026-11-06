use crate::bundle::Bundle;
use crate::doc::{Document, Kind, Layer};
use crate::fill::{Coverage, FillSource};
use crate::ops::{WeightsSpec, rig_owner};
use crate::stroke::{Mark, POS_SCALE};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CELL: u32 = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bone {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub head: [f32; 2],
    pub tail: [f32; 2],
}

impl Bone {
    pub fn angle(&self) -> f32 {
        libm::atan2f(self.tail[1] - self.head[1], self.tail[0] - self.head[0])
    }

    pub fn length(&self) -> f32 {
        (self.tail[0] - self.head[0]).hypot(self.tail[1] - self.head[1])
    }

    fn distance(&self, p: [f32; 2]) -> f32 {
        let (a, b) = (self.head, self.tail);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
        (a[0] + dx * t - p[0]).hypot(a[1] + dy * t - p[1])
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub rest: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub angles: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub root: [f32; 2],
}

fn is_origin(p: &[f32; 2]) -> bool {
    p[0] == 0.0 && p[1] == 0.0
}

impl Pose {
    pub fn mix(&self, other: &Pose, t: f32) -> Pose {
        let names: std::collections::BTreeSet<&String> = self.angles.keys().chain(other.angles.keys()).collect();
        Pose {
            rest: self.rest.clone(),
            angles: names
                .into_iter()
                .map(|n| {
                    let (a, b) = (
                        self.angles.get(n).copied().unwrap_or(0.0),
                        other.angles.get(n).copied().unwrap_or(0.0),
                    );
                    (n.clone(), a + (b - a) * t)
                })
                .collect(),
            root: [
                self.root[0] + (other.root[0] - self.root[0]) * t,
                self.root[1] + (other.root[1] - self.root[1]) * t,
            ],
        }
    }
}

pub fn check(bones: &[Bone]) -> Result<()> {
    bones.iter().enumerate().try_for_each(|(i, b)| {
        if b.name.trim().is_empty() {
            bail!("a bone needs a name");
        }
        if bones[..i].iter().any(|o| o.name == b.name) {
            bail!("two bones are called {}", b.name);
        }
        if let Some(p) = &b.parent
            && !bones[..i].iter().any(|o| &o.name == p)
        {
            bail!("bone {}: its parent {p} must come before it", b.name);
        }
        if b.length() < 1e-3 {
            bail!("bone {}: its head and tail are the same point", b.name);
        }
        Ok(())
    })
}

pub fn subtree(bones: &[Bone], name: &str) -> Vec<String> {
    let mut found = vec![name.to_string()];
    bones.iter().for_each(|b| {
        if b.parent.as_ref().is_some_and(|p| found.contains(p)) && !found.contains(&b.name) {
            found.push(b.name.clone());
        }
    });
    found
}

const JOINED: f32 = 0.5;

pub fn moved(
    bones: &[Bone],
    name: &str,
    head: Option<[f32; 2]>,
    tail: Option<[f32; 2]>,
    by: Option<[f32; 2]>,
) -> Result<Vec<Bone>> {
    let at = bones
        .iter()
        .position(|b| b.name == name)
        .ok_or_else(|| anyhow!("no bone {name}"))?;
    let mut out = bones.to_vec();
    match (head, tail, by) {
        (None, None, Some([dx, dy])) => {
            let moving = subtree(bones, name);
            out.iter_mut().filter(|b| moving.contains(&b.name)).for_each(|b| {
                b.head = [b.head[0] + dx, b.head[1] + dy];
                b.tail = [b.tail[0] + dx, b.tail[1] + dy];
            });
        }
        (None, None, None) => bail!("say where: head, tail or by"),
        (_, _, Some(_)) => bail!("by moves the whole bone; give it without head or tail"),
        (head, tail, None) => {
            let old = bones[at].tail;
            if let Some(h) = head {
                out[at].head = h;
            }
            if let Some(t) = tail {
                out[at].tail = t;
                out.iter_mut()
                    .filter(|b| {
                        b.parent.as_deref() == Some(name) && (b.head[0] - old[0]).hypot(b.head[1] - old[1]) <= JOINED
                    })
                    .for_each(|b| b.head = t);
            }
        }
    }
    Ok(out)
}

type Mat = [f32; 6];

fn mul(a: Mat, b: Mat) -> Mat {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn invert(m: Mat) -> Mat {
    let det = m[0] * m[3] - m[1] * m[2];
    let d = if det.abs() < 1e-12 { 1.0 } else { 1.0 / det };
    let (a, b, c, e) = (m[3] * d, -m[1] * d, -m[2] * d, m[0] * d);
    [a, b, c, e, -(a * m[4] + c * m[5]), -(b * m[4] + e * m[5])]
}

fn placed(at: [f32; 2], angle: f32) -> Mat {
    let (s, c) = (libm::sinf(angle), libm::cosf(angle));
    [c, s, -s, c, at[0], at[1]]
}

fn apply(m: Mat, p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

pub fn skinning(bones: &[Bone], pose: &Pose) -> Vec<Mat> {
    let rest: Vec<Mat> = bones.iter().map(|b| placed(b.head, b.angle())).collect();
    let mut posed: Vec<Mat> = Vec::with_capacity(bones.len());
    bones.iter().enumerate().for_each(|(i, b)| {
        let turn = placed(
            [0.0, 0.0],
            pose.angles.get(&b.name).copied().unwrap_or(0.0).to_radians(),
        );
        let world = match b.parent.as_ref().and_then(|p| bones.iter().position(|o| &o.name == p)) {
            Some(p) => mul(posed[p], mul(mul(invert(rest[p]), rest[i]), turn)),
            None => mul([1.0, 0.0, 0.0, 1.0, pose.root[0], pose.root[1]], mul(rest[i], turn)),
        };
        posed.push(world);
    });
    posed.iter().zip(&rest).map(|(p, r)| mul(*p, invert(*r))).collect()
}

pub fn tips(bones: &[Bone], pose: &Pose) -> Vec<([f32; 2], [f32; 2])> {
    let mats = skinning(bones, pose);
    bones
        .iter()
        .zip(&mats)
        .map(|(b, m)| (apply(*m, b.head), apply(*m, b.tail)))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weights {
    pub width: u32,
    pub height: u32,
    pub maps: BTreeMap<String, Vec<u8>>,
}

impl Weights {
    pub fn new(canvas: (u32, u32)) -> Self {
        Self {
            width: canvas.0.div_ceil(CELL),
            height: canvas.1.div_ceil(CELL),
            maps: BTreeMap::new(),
        }
    }

    pub fn dims(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let names: Vec<&String> = self.maps.keys().collect();
        let head = serde_json::json!({"width": self.width, "height": self.height, "bones": names});
        let mut raw = serde_json::to_vec(&head)?;
        raw.push(b'\n');
        self.maps.values().for_each(|m| raw.extend_from_slice(m));
        Ok(zstd::encode_all(&raw[..], 9)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let raw = zstd::decode_all(bytes)?;
        let split = raw
            .iter()
            .position(|&b| b == b'\n')
            .context("weights file without a header")?;
        let head: serde_json::Value = serde_json::from_slice(&raw[..split])?;
        let (width, height) = (
            head["width"].as_u64().unwrap_or(0) as u32,
            head["height"].as_u64().unwrap_or(0) as u32,
        );
        let n = (width * height) as usize;
        let names: Vec<String> = serde_json::from_value(head["bones"].clone())?;
        let body = &raw[split + 1..];
        if body.len() != n * names.len() {
            bail!("weights file is cut short");
        }
        Ok(Self {
            width,
            height,
            maps: names
                .into_iter()
                .enumerate()
                .map(|(i, name)| (name, body[i * n..(i + 1) * n].to_vec()))
                .collect(),
        })
    }

    fn map(&mut self, bone: &str) -> &mut Vec<u8> {
        let n = (self.width * self.height) as usize;
        self.maps.entry(bone.to_string()).or_insert_with(|| vec![0; n])
    }

    pub fn auto(bones: &[Bone], canvas: (u32, u32)) -> Self {
        let mut out = Self::new(canvas);
        let (w, h) = (out.width, out.height);
        let mut maps: Vec<Vec<u8>> = vec![vec![0; (w * h) as usize]; bones.len()];
        (0..h).for_each(|gy| {
            (0..w).for_each(|gx| {
                let p = [(gx as f32 + 0.5) * CELL as f32, (gy as f32 + 0.5) * CELL as f32];
                let raw: Vec<f32> = bones
                    .iter()
                    .map(|b| 1.0 / (b.distance(p) + b.length() * 0.15 + 1.0).powi(3))
                    .collect();
                let sum: f32 = raw.iter().sum::<f32>().max(1e-12);
                raw.iter().enumerate().for_each(|(i, r)| {
                    maps[i][(gy * w + gx) as usize] = (r / sum * 255.0).round() as u8;
                });
            })
        });
        out.maps = bones.iter().map(|b| b.name.clone()).zip(maps).collect();
        out
    }

    pub fn dab(&mut self, bone: &str, at: [f32; 2], radius: f32, value: f32) {
        let (w, h) = (self.width, self.height);
        let r = (radius / CELL as f32).max(0.5);
        let (cx, cy) = (at[0] / CELL as f32, at[1] / CELL as f32);
        let map = self.map(bone);
        let (x0, x1) = ((cx - r).floor().max(0.0) as u32, ((cx + r).ceil() as u32).min(w));
        let (y0, y1) = ((cy - r).floor().max(0.0) as u32, ((cy + r).ceil() as u32).min(h));
        (y0..y1).for_each(|y| {
            (x0..x1).for_each(|x| {
                let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) / r;
                if d < 1.0 {
                    let k = 1.0 - d * d * (3.0 - 2.0 * d);
                    let v = &mut map[(y * w + x) as usize];
                    let now = *v as f32 / 255.0;
                    *v = ((now + (value.clamp(0.0, 1.0) - now) * k) * 255.0).round() as u8;
                }
            })
        });
    }

    pub fn fill(&mut self, bone: &str, cells: &[(u32, u32)], value: f32) {
        let w = self.width;
        let v = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let map = self.map(bone);
        cells.iter().for_each(|&(x, y)| map[(y * w + x) as usize] = v);
    }

    pub fn smooth(&mut self) {
        let (w, h) = (self.width as i64, self.height as i64);
        self.maps.values_mut().for_each(|m| {
            let before = m.clone();
            (0..h).for_each(|y| {
                (0..w).for_each(|x| {
                    let (sum, n) = (-1..=1)
                        .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                        .map(|(dx, dy)| (x + dx, y + dy))
                        .filter(|(x, y)| *x >= 0 && *y >= 0 && *x < w && *y < h)
                        .fold((0u32, 0u32), |(s, n), (x, y)| {
                            (s + before[(y * w + x) as usize] as u32, n + 1)
                        });
                    m[(y * w + x) as usize] = (sum / n.max(1)) as u8;
                })
            })
        });
    }

    pub fn at(&self, bones: &[Bone], p: [f32; 2]) -> Vec<f32> {
        let (gx, gy) = (p[0] / CELL as f32 - 0.5, p[1] / CELL as f32 - 0.5);
        let (x0, y0) = (gx.floor(), gy.floor());
        let (tx, ty) = (gx - x0, gy - y0);
        let (w, h) = (self.width as i64, self.height as i64);
        let get = |m: &Vec<u8>, x: i64, y: i64| m[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize] as f32 / 255.0;
        let raw: Vec<f32> = bones
            .iter()
            .map(|b| {
                self.maps.get(&b.name).map_or(0.0, |m| {
                    let (x, y) = (x0 as i64, y0 as i64);
                    let top = get(m, x, y) + (get(m, x + 1, y) - get(m, x, y)) * tx;
                    let bottom = get(m, x, y + 1) + (get(m, x + 1, y + 1) - get(m, x, y + 1)) * tx;
                    top + (bottom - top) * ty
                })
            })
            .collect();
        let sum: f32 = raw.iter().sum();
        match sum > 1e-4 {
            true => raw.iter().map(|r| r / sum).collect(),
            false => {
                let nearest = bones
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
                    .map_or(0, |(i, _)| i);
                (0..bones.len()).map(|i| if i == nearest { 1.0 } else { 0.0 }).collect()
            }
        }
    }
}

pub struct Rig<'a> {
    pub bones: &'a [Bone],
    pub weights: &'a Weights,
    pub mats: Vec<Mat>,
}

impl<'a> Rig<'a> {
    pub fn new(bones: &'a [Bone], weights: &'a Weights, pose: &Pose) -> Self {
        Self {
            bones,
            weights,
            mats: skinning(bones, pose),
        }
    }

    pub fn moved(&self, p: [f32; 2]) -> [f32; 2] {
        let w = self.weights.at(self.bones, p);
        w.iter()
            .zip(&self.mats)
            .filter(|(k, _)| **k > 0.0)
            .fold([0.0, 0.0], |acc, (k, m)| {
                let q = apply(*m, p);
                [acc[0] + q[0] * k, acc[1] + q[1] * k]
            })
    }

    fn stretch(&self, p: [f32; 2]) -> f32 {
        let (o, x, y) = (
            self.moved(p),
            self.moved([p[0] + 1.0, p[1]]),
            self.moved([p[0], p[1] + 1.0]),
        );
        let det = (x[0] - o[0]) * (y[1] - o[1]) - (x[1] - o[1]) * (y[0] - o[0]);
        det.abs().sqrt().clamp(0.25, 4.0)
    }

    fn coverage(&self, c: &Coverage, canvas: (u32, u32)) -> Coverage {
        if c.is_empty() {
            return c.clone();
        }
        let step = 4u32;
        let (cols, rows) = (c.width.div_ceil(step) + 1, c.height.div_ceil(step) + 1);
        let rest = |i: u32, j: u32| {
            [
                (c.x + (i * step).min(c.width)) as f32,
                (c.y + (j * step).min(c.height)) as f32,
            ]
        };
        let posed: Vec<[f32; 2]> = (0..rows)
            .flat_map(|j| (0..cols).map(move |i| (i, j)))
            .map(|(i, j)| self.moved(rest(i, j)))
            .collect();
        let (w, h) = (canvas.0 as usize, canvas.1 as usize);
        let mut mask = vec![0u8; w * h];
        let sample = |p: [f32; 2]| -> u8 {
            let (x, y) = (p[0].floor(), p[1].floor());
            match x >= 0.0 && y >= 0.0 {
                true => c.at(x as u32, y as u32),
                false => 0,
            }
        };
        (0..rows - 1).for_each(|j| {
            (0..cols - 1).for_each(|i| {
                let corner = |di: u32, dj: u32| ((i + di, j + dj), posed[((j + dj) * cols + i + di) as usize]);
                [
                    [corner(0, 0), corner(1, 0), corner(1, 1)],
                    [corner(0, 0), corner(1, 1), corner(0, 1)],
                ]
                .iter()
                .for_each(|tri| {
                    let [a, b, d] = [tri[0].1, tri[1].1, tri[2].1];
                    let area = (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]);
                    if area.abs() < 1e-6 {
                        return;
                    }
                    let lo = [
                        a[0].min(b[0]).min(d[0]).floor().max(0.0),
                        a[1].min(b[1]).min(d[1]).floor().max(0.0),
                    ];
                    let hi = [
                        a[0].max(b[0]).max(d[0]).ceil().min(w as f32),
                        a[1].max(b[1]).max(d[1]).ceil().min(h as f32),
                    ];
                    let rests = [
                        rest(tri[0].0.0, tri[0].0.1),
                        rest(tri[1].0.0, tri[1].0.1),
                        rest(tri[2].0.0, tri[2].0.1),
                    ];
                    (lo[1] as usize..hi[1] as usize).for_each(|y| {
                        (lo[0] as usize..hi[0] as usize).for_each(|x| {
                            let p = [x as f32 + 0.5, y as f32 + 0.5];
                            let u = ((b[0] - p[0]) * (d[1] - p[1]) - (b[1] - p[1]) * (d[0] - p[0])) / area;
                            let v = ((d[0] - p[0]) * (a[1] - p[1]) - (d[1] - p[1]) * (a[0] - p[0])) / area;
                            let k = 1.0 - u - v;
                            if u < -1e-4 || v < -1e-4 || k < -1e-4 {
                                return;
                            }
                            let q = [
                                rests[0][0] * u + rests[1][0] * v + rests[2][0] * k,
                                rests[0][1] * u + rests[1][1] * v + rests[2][1] * k,
                            ];
                            let m = &mut mask[y * w + x];
                            *m = (*m).max(sample(q));
                        })
                    });
                });
            })
        });
        Coverage::from_mask(canvas.0, canvas.1, &mask)
    }

    pub fn pose(&self, marks: &[Mark], canvas: (u32, u32)) -> Vec<Mark> {
        if self.mats.iter().all(|m| {
            *m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
                || m.iter()
                    .zip([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
                    .all(|(a, b)| (a - b).abs() < 1e-5)
        }) {
            return marks.to_vec();
        }
        marks
            .iter()
            .map(|mark| {
                let mut out = mark.clone();
                match &mut out {
                    Mark::Stroke(s) => {
                        let n = s.samples.len().max(1) as f32;
                        let stretch = s.samples.iter().map(|q| self.stretch(q.pos().into())).sum::<f32>() / n;
                        s.samples.iter_mut().for_each(|q| {
                            let (x, y) = q.pos();
                            let p = self.moved([x, y]);
                            q.x = (p[0] * POS_SCALE).round() as i32;
                            q.y = (p[1] * POS_SCALE).round() as i32;
                        });
                        s.size *= stretch;
                    }
                    Mark::Fill(f) => {
                        if let FillSource::Polygon(points) = &mut f.source {
                            points.iter_mut().for_each(|p| *p = self.moved(*p));
                        }
                        if let FillSource::Seed { seed, .. } = &mut f.source {
                            let p = self.moved([seed.x, seed.y]);
                            (seed.x, seed.y) = (p[0], p[1]);
                        }
                        f.coverage = self.coverage(&f.coverage, canvas);
                    }
                }
                out
            })
            .collect()
    }
}

pub fn family(layer: &Layer) -> Vec<&Layer> {
    std::iter::once(layer)
        .chain(layer.children().iter().flat_map(family))
        .collect()
}

pub fn painted(owner: &Layer) -> Vec<&Layer> {
    family(owner).into_iter().filter(|l| l.kind() == Kind::Paint).collect()
}

pub fn weights_for(
    bundle: &Bundle,
    doc: &Document,
    spec: &WeightsSpec,
    current: &BTreeMap<String, Weights>,
) -> Result<Weights> {
    let owner = rig_owner(doc, &spec.layer)?;
    let canvas = (doc.canvas.width, doc.canvas.height);
    if owner.bones.is_empty() {
        bail!("{} has no bones yet: add them with klad bone first", owner.name);
    }
    let mut w = match current.get(&owner.id) {
        Some(w) => w.clone(),
        None => bundle.read_weights(&owner.id)?,
    };
    if (w.width, w.height) != Weights::new(canvas).dims() {
        w = Weights::new(canvas);
    }
    if spec.auto {
        let fresh = Weights::auto(&owner.bones, canvas);
        match &spec.bone {
            Some(b) => {
                let map = fresh.maps.get(b).cloned().ok_or_else(|| anyhow!("no bone {b}"))?;
                w.maps.insert(b.clone(), map);
            }
            None => w = fresh,
        }
    }
    let bone = || -> Result<String> {
        let b = spec.bone.clone().context("say which bone: \"bone\"")?;
        if !owner.bones.iter().any(|o| o.name == b) {
            bail!("no bone {b} on {}", owner.name);
        }
        Ok(b)
    };
    if !spec.set.is_empty() {
        let b = bone()?;
        spec.set.iter().for_each(|d| w.dab(&b, [d[0], d[1]], d[2], d[3]));
    }
    if let Some(part) = &spec.part {
        let b = bone()?;
        let mut cells = Vec::new();
        painted(owner).iter().try_for_each(|layer| -> Result<()> {
            let Some(cel) = layer.content_at(spec.frame) else {
                return Ok(());
            };
            let Some(ids) = cel.parts.get(part) else {
                return Ok(());
            };
            let marks = bundle.read_marks(&layer.id, &cel.id)?;
            marks.iter().filter(|m| ids.contains(&m.id())).for_each(|m| match m {
                Mark::Stroke(s) => s.samples.iter().for_each(|q| {
                    let (x, y) = q.pos();
                    let r = (s.size * 0.5 / CELL as f32).ceil() as i64 + 1;
                    let (cx, cy) = ((x / CELL as f32) as i64, (y / CELL as f32) as i64);
                    (-r..=r)
                        .flat_map(|dy| (-r..=r).map(move |dx| (cx + dx, cy + dy)))
                        .for_each(|c| cells.push(c));
                }),
                Mark::Fill(f) => {
                    let c = &f.coverage;
                    (c.y..c.y + c.height).step_by(CELL as usize / 2).for_each(|y| {
                        (c.x..c.x + c.width)
                            .step_by(CELL as usize / 2)
                            .filter(|&x| c.at(x, y) > 0)
                            .for_each(|x| cells.push(((x / CELL) as i64, (y / CELL) as i64)));
                    });
                }
            });
            Ok(())
        })?;
        let (gw, gh) = (w.width as i64, w.height as i64);
        let mut cells: Vec<(u32, u32)> = cells
            .into_iter()
            .filter(|(x, y)| *x >= 0 && *y >= 0 && *x < gw && *y < gh)
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
        cells.sort_unstable();
        cells.dedup();
        if cells.is_empty() {
            bail!("no marks in the part {part} at frame {}", spec.frame);
        }
        w.fill(&b, &cells, spec.value);
        if spec.value >= 1.0 {
            let others: Vec<String> = w.maps.keys().filter(|k| **k != b).cloned().collect();
            others.iter().for_each(|o| w.fill(o, &cells, 0.0));
        }
    }
    if spec.smooth {
        w.smooth();
    }
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::Stroke;

    fn leg() -> Vec<Bone> {
        vec![
            Bone {
                name: "thigh".into(),
                parent: None,
                head: [100.0, 100.0],
                tail: [100.0, 200.0],
            },
            Bone {
                name: "shin".into(),
                parent: Some("thigh".into()),
                head: [100.0, 200.0],
                tail: [100.0, 300.0],
            },
        ]
    }

    #[test]
    fn a_moved_tail_takes_the_joined_children_along_and_by_moves_the_family() {
        let mut bones = leg();
        bones.push(Bone {
            name: "spur".into(),
            parent: Some("thigh".into()),
            head: [120.0, 150.0],
            tail: [140.0, 150.0],
        });
        let bent = moved(&bones, "thigh", None, Some([130.0, 190.0]), None).unwrap();
        assert_eq!(bent[0].tail, [130.0, 190.0]);
        assert_eq!(bent[1].head, [130.0, 190.0], "the shin starts where the thigh ends");
        assert_eq!(bent[1].tail, [100.0, 300.0]);
        assert_eq!(bent[2].head, [120.0, 150.0], "a child that starts elsewhere stays");
        let shifted = moved(&bones, "thigh", None, None, Some([5.0, -10.0])).unwrap();
        assert!(shifted.iter().all(|b| {
            let was = bones.iter().find(|o| o.name == b.name).unwrap();
            b.head == [was.head[0] + 5.0, was.head[1] - 10.0] && b.tail == [was.tail[0] + 5.0, was.tail[1] - 10.0]
        }));
        let shin = moved(&bones, "shin", None, None, Some([5.0, 0.0])).unwrap();
        assert_eq!(shin[0], bones[0], "moving a child leaves its parent");
        assert!(moved(&bones, "shin", Some([0.0, 0.0]), None, Some([1.0, 1.0])).is_err());
        assert_eq!(subtree(&bones, "thigh"), ["thigh", "shin", "spur"]);
    }

    #[test]
    fn automatic_weights_add_up_and_fall_off_across_the_joint() {
        let bones = leg();
        let w = Weights::auto(&bones, (200, 400));
        let at = |y: f32| w.at(&bones, [100.0, y]);
        assert!(
            at(130.0)[0] > 0.9 && at(270.0)[1] > 0.9,
            "{:?} {:?}",
            at(130.0),
            at(270.0)
        );
        let joint = at(200.0);
        assert!((joint[0] - 0.5).abs() < 0.15, "{joint:?}");
        assert!(((joint[0] + joint[1]) - 1.0).abs() < 1e-3);
        let back = Weights::decode(&w.encode().unwrap()).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn a_painted_dab_changes_only_near_it() {
        let bones = leg();
        let mut w = Weights::auto(&bones, (200, 400));
        let before = w.clone();
        w.dab("thigh", [100.0, 260.0], 16.0, 1.0);
        assert!(w.at(&bones, [100.0, 260.0])[0] > before.at(&bones, [100.0, 260.0])[0] + 0.3);
        assert_eq!(w.at(&bones, [100.0, 120.0]), before.at(&bones, [100.0, 120.0]));
        let changed: Vec<usize> = (0..w.maps["thigh"].len())
            .filter(|&i| w.maps["thigh"][i] != before.maps["thigh"][i])
            .collect();
        assert!(!changed.is_empty());
        assert!(changed.iter().all(|&i| {
            let (x, y) = ((i as u32 % w.width) * CELL, (i as u32 / w.width) * CELL);
            (x as f32 - 100.0).hypot(y as f32 - 260.0) <= 16.0 + CELL as f32 * 1.5
        }));
    }

    #[test]
    fn a_bent_knee_bends_a_line_smoothly_and_the_rest_pose_changes_nothing() {
        let bones = leg();
        let weights = Weights::auto(&bones, (200, 400));
        let points: Vec<(f32, f32, f32)> = (0..=40).map(|i| (100.0, 100.0 + i as f32 * 5.0, 1.0)).collect();
        let line: Mark = Stroke::new(1, "ink", [0.0, 0.0, 0.0, 1.0], 6.0, 1)
            .with_points(&points, 8.0)
            .into();
        let rest = Rig::new(&bones, &weights, &Pose::default());
        assert_eq!(rest.pose(std::slice::from_ref(&line), (200, 400)), vec![line.clone()]);
        let mut bent = Pose::default();
        bent.angles.insert("shin".into(), 60.0);
        let posed = Rig::new(&bones, &weights, &bent).pose(std::slice::from_ref(&line), (200, 400));
        let pts: Vec<(f32, f32)> = posed[0].as_stroke().unwrap().samples.iter().map(|q| q.pos()).collect();
        assert!((pts[0].0 - 100.0).abs() < 0.5 && (pts[0].1 - 100.0).abs() < 0.5);
        let end = pts[pts.len() - 1];
        let expected = [
            100.0 - 100.0 * 60f32.to_radians().sin(),
            200.0 + 100.0 * 60f32.to_radians().cos(),
        ];
        assert!(
            (end.0 - expected[0]).abs() < 3.0 && (end.1 - expected[1]).abs() < 3.0,
            "{end:?} {expected:?}"
        );
        let turns: Vec<f32> = pts
            .windows(3)
            .map(|w| {
                let a = libm::atan2f(w[1].1 - w[0].1, w[1].0 - w[0].0);
                let b = libm::atan2f(w[2].1 - w[1].1, w[2].0 - w[1].0);
                (b - a).abs()
            })
            .collect();
        assert!(turns.iter().all(|t| *t < 20f32.to_radians()), "no kink: {turns:?}");
        assert!(
            turns.iter().filter(|t| **t > 2f32.to_radians()).count() >= 4,
            "the bend spreads: {turns:?}"
        );
        let mut rigid = Weights::new((200, 400));
        rigid.fill(
            "thigh",
            &(0..100).flat_map(|y| (0..50).map(move |x| (x, y))).collect::<Vec<_>>(),
            1.0,
        );
        let mut swing = Pose::default();
        swing.angles.insert("thigh".into(), 90.0);
        let moved = Rig::new(&bones, &rigid, &swing).pose(std::slice::from_ref(&line), (200, 400));
        let s = moved[0].as_stroke().unwrap();
        let (a, b) = (s.samples[0].pos(), s.samples[s.samples.len() - 1].pos());
        assert!(
            ((b.0 - a.0).hypot(b.1 - a.1) - 200.0).abs() < 1.0,
            "a mark held by one bone moves rigidly"
        );
    }
}
