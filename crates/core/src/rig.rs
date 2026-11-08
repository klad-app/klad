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
        libm::hypotf(self.tail[0] - self.head[0], self.tail[1] - self.head[1])
    }

    fn distance(&self, p: [f32; 2]) -> f32 {
        let (a, b) = (self.head, self.tail);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
        libm::hypotf(a[0] + dx * t - p[0], a[1] + dy * t - p[1])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bend {
    #[default]
    Clockwise,
    Counterclockwise,
}

impl Bend {
    pub fn flipped(self) -> Bend {
        match self {
            Bend::Clockwise => Bend::Counterclockwise,
            Bend::Counterclockwise => Bend::Clockwise,
        }
    }

    fn sign(self) -> f64 {
        match self {
            Bend::Clockwise => 1.0,
            Bend::Counterclockwise => -1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ik {
    pub bone: String,
    #[serde(default = "limb", skip_serializing_if = "is_limb")]
    pub chain: u8,
    #[serde(default, skip_serializing_if = "is_clockwise")]
    pub bend: Bend,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub softness: f32,
}

fn limb() -> u8 {
    2
}

fn is_limb(n: &u8) -> bool {
    *n == 2
}

fn is_clockwise(b: &Bend) -> bool {
    *b == Bend::Clockwise
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reach {
    pub target: [f32; 2],
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub mix: f32,
}

fn full() -> f32 {
    1.0
}

fn is_full(v: &f32) -> bool {
    *v == 1.0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub rest: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub angles: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub root: [f32; 2],
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ik: BTreeMap<String, Reach>,
}

fn is_origin(p: &[f32; 2]) -> bool {
    p[0] == 0.0 && p[1] == 0.0
}

pub fn near_degrees(angle: f32, to: f32) -> f32 {
    to + ((angle - to + 180.0).rem_euclid(360.0) - 180.0)
}

pub fn limb_of(bones: &[Bone], c: &Ik) -> Vec<String> {
    let parent = bones
        .iter()
        .find(|b| b.name == c.bone)
        .and_then(|b| b.parent.clone())
        .filter(|_| c.chain == 2);
    std::iter::once(c.bone.clone()).chain(parent).collect()
}

fn as_turns(bones: &[Bone], ik: &[Ik], keyed: &mut Pose, other: &Pose, ends: &[&Ik]) {
    let solved = applied(bones, ik, keyed);
    ends.iter().for_each(|c| {
        limb_of(bones, c).into_iter().for_each(|b| {
            if let Some(v) = solved.get(&b) {
                let to = other.angles.get(&b).copied().unwrap_or(0.0);
                keyed.angles.insert(b, near_degrees(*v, to));
            }
        });
        keyed.ik.remove(&c.bone);
    });
}

pub fn between(bones: &[Bone], ik: &[Ik], a: &Pose, b: &Pose, t: f32) -> Pose {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let (mut a, mut b) = (a.clone(), b.clone());
    let only = |p: &Pose, q: &Pose| -> Vec<&Ik> {
        ik.iter()
            .filter(|c| p.ik.contains_key(&c.bone) && !q.ik.contains_key(&c.bone))
            .collect()
    };
    let (in_a, in_b) = (only(&a, &b), only(&b, &a));
    if !in_a.is_empty() {
        let other = b.clone();
        as_turns(bones, ik, &mut a, &other, &in_a);
    }
    if !in_b.is_empty() {
        let other = a.clone();
        as_turns(bones, ik, &mut b, &other, &in_b);
    }
    let names: std::collections::BTreeSet<String> = a.angles.keys().chain(b.angles.keys()).cloned().collect();
    let reaches = ik
        .iter()
        .filter_map(|c| {
            let (x, y) = (a.ik.get(&c.bone)?, b.ik.get(&c.bone)?);
            Some((
                c.bone.clone(),
                Reach {
                    target: [lerp(x.target[0], y.target[0]), lerp(x.target[1], y.target[1])],
                    mix: lerp(x.mix, y.mix),
                },
            ))
        })
        .collect();
    Pose {
        rest: a.rest.clone(),
        angles: names
            .into_iter()
            .map(|n| {
                let at = |p: &Pose| p.angles.get(&n).copied().unwrap_or(0.0);
                let v = lerp(at(&a), at(&b));
                (n, v)
            })
            .collect(),
        root: [lerp(a.root[0], b.root[0]), lerp(a.root[1], b.root[1])],
        ik: reaches,
    }
}

pub fn target_for_end(bones: &[Bone], ik: &[Ik], pose: &Pose, bone: &str) -> Option<[f32; 2]> {
    let c = ik.iter().find(|c| c.bone == bone)?;
    let end = bones.iter().position(|b| b.name == bone)?;
    let shown = tips(bones, ik, pose);
    let tip = shown[end].1;
    let Some(a) = parent_of(bones, end).filter(|_| c.chain == 2) else {
        return Some(tip);
    };
    let (upper, lower) = (&bones[a], &bones[end]);
    let l1 = libm::hypot(
        (lower.head[0] - upper.head[0]) as f64,
        (lower.head[1] - upper.head[1]) as f64,
    );
    let reach = l1 + lower.length() as f64;
    let softness = c.softness as f64;
    let s = shown[a].0;
    let (dx, dy) = ((tip[0] - s[0]) as f64, (tip[1] - s[1]) as f64);
    let r = libm::hypot(dx, dy);
    if softness <= 0.0 || r <= reach - softness || r < NEAR {
        return Some(tip);
    }
    let d = match r >= reach {
        true => reach + softness,
        false => reach - softness + 2.0 * softness * (1.0 - ((reach - r) / softness).sqrt()),
    };
    Some([(s[0] as f64 + dx * d / r) as f32, (s[1] as f64 + dy * d / r) as f32])
}

pub fn bent_against(bones: &[Bone], ik: &[Ik], pose: &Pose, bone: &str) -> bool {
    let Some(c) = ik.iter().find(|c| c.bone == bone && c.chain == 2) else {
        return false;
    };
    let Some(end) = bones.iter().position(|b| b.name == bone) else {
        return false;
    };
    let Some(a) = parent_of(bones, end) else {
        return false;
    };
    let shown = tips(bones, ik, pose);
    let (s, e, q) = (shown[a].0, shown[end].0, shown[end].1);
    let first = libm::atan2((e[1] - s[1]) as f64, (e[0] - s[0]) as f64);
    let second = libm::atan2((q[1] - e[1]) as f64, (q[0] - e[0]) as f64);
    let kappa = wrap(second - first);
    match c.bend {
        Bend::Clockwise => kappa < -RAD,
        Bend::Counterclockwise => kappa > RAD,
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
        if !b.head.iter().chain(&b.tail).all(|v| v.is_finite()) {
            bail!("bone {}: its head and tail must be numbers", b.name);
        }
        if b.length() < 1e-3 {
            bail!("bone {}: its head and tail are the same point", b.name);
        }
        Ok(())
    })
}

pub fn check_pose(pose: &Pose) -> Result<()> {
    if let Some((bone, _)) = pose.angles.iter().find(|(_, a)| !a.is_finite()) {
        bail!("pose: the turn of {bone} must be a number");
    }
    if !pose.root.iter().all(|v| v.is_finite()) {
        bail!("pose: root must be two numbers");
    }
    pose.ik.iter().try_for_each(|(bone, r)| {
        if !r.target.iter().all(|v| v.is_finite()) {
            bail!("pose: the target of {bone} must be two numbers");
        }
        if !(0.0..=1.0).contains(&r.mix) {
            bail!(
                "pose: the mix of {bone} goes from 0 (its turns) to 1 (the target), not {}",
                r.mix
            );
        }
        Ok(())
    })
}

pub fn check_ik(bones: &[Bone], ik: &[Ik]) -> Result<()> {
    ik.iter().enumerate().try_for_each(|(i, c)| {
        let Some(at) = bones.iter().position(|b| b.name == c.bone) else {
            bail!("IK on {}: there is no bone {}", c.bone, c.bone);
        };
        if ik[..i].iter().any(|o| o.bone == c.bone) {
            bail!("two IKs end on {}", c.bone);
        }
        match c.chain {
            1 => {}
            2 => {
                let Some(p) = parent_of(bones, at) else {
                    bail!(
                        "IK on {}: a limb (chain 2) turns the bone and its parent, and {} has none: make it an aim (chain 1)",
                        c.bone,
                        c.bone
                    );
                };
                let (h, q) = (bones[at].head, bones[p].head);
                if libm::hypotf(h[0] - q[0], h[1] - q[1]) < 1e-3 {
                    bail!(
                        "IK on {}: it starts where its parent {} starts, so turning {} cannot carry it to a target: make it an aim (chain 1)",
                        c.bone,
                        bones[p].name,
                        bones[p].name
                    );
                }
            }
            n => bail!("IK on {}: chain is 1 (an aim) or 2 (a limb), not {n}", c.bone),
        }
        if !(c.softness.is_finite() && c.softness >= 0.0) {
            bail!("IK on {}: softness is a distance in px, 0 or more", c.bone);
        }
        Ok(())
    })
}

pub fn rest_bend(bones: &[Bone], bone: &str) -> Bend {
    let Some(at) = bones.iter().position(|b| b.name == bone) else {
        return Bend::Clockwise;
    };
    let Some(p) = parent_of(bones, at) else {
        return Bend::Clockwise;
    };
    let (b, a) = (&bones[at], &bones[p]);
    let along = libm::atan2((b.head[1] - a.head[1]) as f64, (b.head[0] - a.head[0]) as f64);
    match wrap(rest_angle(b) - along) < -RAD {
        true => Bend::Counterclockwise,
        false => Bend::Clockwise,
    }
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

fn parent_of(bones: &[Bone], i: usize) -> Option<usize> {
    let p = bones[i].parent.as_ref()?;
    bones[..i].iter().position(|o| &o.name == p)
}

const RAD: f64 = std::f64::consts::PI / 180.0;
const NEAR: f64 = 1e-4;

fn rest_angle(b: &Bone) -> f64 {
    libm::atan2((b.tail[1] - b.head[1]) as f64, (b.tail[0] - b.head[0]) as f64)
}

fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

struct Frame {
    head: [f64; 2],
    turn: f64,
}

fn frames(bones: &[Bone], turns: &[f64], root: [f32; 2]) -> Vec<Frame> {
    let mut out: Vec<Frame> = Vec::with_capacity(bones.len());
    bones.iter().enumerate().for_each(|(i, b)| {
        let frame = match parent_of(bones, i) {
            Some(p) => {
                let (dx, dy) = (
                    (b.head[0] - bones[p].head[0]) as f64,
                    (b.head[1] - bones[p].head[1]) as f64,
                );
                let (s, c) = (libm::sin(out[p].turn), libm::cos(out[p].turn));
                Frame {
                    head: [out[p].head[0] + c * dx - s * dy, out[p].head[1] + s * dx + c * dy],
                    turn: out[p].turn + turns[i],
                }
            }
            None => Frame {
                head: [(b.head[0] + root[0]) as f64, (b.head[1] + root[1]) as f64],
                turn: turns[i],
            },
        };
        out.push(frame);
    });
    out
}

fn soft(d: f64, reach: f64, softness: f64) -> f64 {
    match softness > 0.0 {
        false => d,
        true if d <= reach - softness => d,
        true if d >= reach + softness => reach,
        true => {
            let over = d - reach + softness;
            d - over * over / (4.0 * softness)
        }
    }
}

fn aim(bones: &[Bone], end: usize, world: &[Frame], target: [f64; 2]) -> Vec<(usize, f64)> {
    let head = world[end].head;
    let (vx, vy) = (target[0] - head[0], target[1] - head[1]);
    if libm::hypot(vx, vy) < NEAR {
        return Vec::new();
    }
    let above = parent_of(bones, end).map_or(0.0, |p| world[p].turn);
    vec![(end, libm::atan2(vy, vx) - rest_angle(&bones[end]) - above)]
}

fn solve(bones: &[Bone], c: &Ik, end: usize, world: &[Frame], target: [f64; 2]) -> Vec<(usize, f64)> {
    let Some(a) = parent_of(bones, end).filter(|_| c.chain == 2) else {
        return aim(bones, end, world, target);
    };
    let (upper, lower) = (&bones[a], &bones[end]);
    let (jx, jy) = (
        (lower.head[0] - upper.head[0]) as f64,
        (lower.head[1] - upper.head[1]) as f64,
    );
    let l1 = libm::hypot(jx, jy);
    if l1 < NEAR {
        return aim(bones, end, world, target);
    }
    let along = libm::atan2(jy, jx);
    let l2 = libm::hypot(
        (lower.tail[0] - lower.head[0]) as f64,
        (lower.tail[1] - lower.head[1]) as f64,
    );
    let s = world[a].head;
    let (dx, dy) = (target[0] - s[0], target[1] - s[1]);
    let d = libm::hypot(dx, dy);
    let u = match d < NEAR {
        true => {
            let fk = along + world[a].turn;
            [libm::cos(fk), libm::sin(fk)]
        }
        false => [dx / d, dy / d],
    };
    let n = [-u[1], u[0]];
    let reach = l1 + l2;
    let r = soft(d, reach, c.softness as f64).clamp((l1 - l2).abs().max(NEAR), reach);
    let a_len = (r * r + l1 * l1 - l2 * l2) / (2.0 * r);
    let h = (l1 * l1 - a_len * a_len).max(0.0).sqrt();
    let sign = c.bend.sign();
    let elbow = [
        s[0] + a_len * u[0] - sign * h * n[0],
        s[1] + a_len * u[1] - sign * h * n[1],
    ];
    let tip = [s[0] + r * u[0], s[1] + r * u[1]];
    let first = libm::atan2(elbow[1] - s[1], elbow[0] - s[0]);
    let second = libm::atan2(tip[1] - elbow[1], tip[0] - elbow[0]);
    let above = parent_of(bones, a).map_or(0.0, |p| world[p].turn);
    vec![
        (a, first - along - above),
        (end, second - first - (rest_angle(lower) - along)),
    ]
}

fn order<'a>(bones: &[Bone], ik: &'a [Ik]) -> Vec<(usize, &'a Ik)> {
    let mut found: Vec<(usize, usize, &Ik)> = ik
        .iter()
        .filter_map(|c| {
            let end = bones.iter().position(|b| b.name == c.bone)?;
            let top = match c.chain {
                2 => parent_of(bones, end).unwrap_or(end),
                _ => end,
            };
            Some((top, end, c))
        })
        .collect();
    found.sort_by_key(|(top, end, _)| (*top, *end));
    found.into_iter().map(|(_, end, c)| (end, c)).collect()
}

pub fn applied(bones: &[Bone], ik: &[Ik], pose: &Pose) -> BTreeMap<String, f32> {
    let mut angles = pose.angles.clone();
    if pose.ik.is_empty() || ik.is_empty() {
        return angles;
    }
    let mut turns: Vec<f64> = bones
        .iter()
        .map(|b| angles.get(&b.name).map_or(0.0, |a| *a as f64 * RAD))
        .collect();
    let mut touched = std::collections::BTreeSet::new();
    order(bones, ik).into_iter().for_each(|(end, c)| {
        let Some(reach) = pose.ik.get(&c.bone) else {
            return;
        };
        let mix = if reach.mix.is_finite() {
            reach.mix.clamp(0.0, 1.0) as f64
        } else {
            0.0
        };
        let target = [reach.target[0] as f64, reach.target[1] as f64];
        if mix <= 0.0 || !target.iter().all(|v| v.is_finite()) {
            return;
        }
        let world = frames(bones, &turns, pose.root);
        solve(bones, c, end, &world, target).into_iter().for_each(|(i, turn)| {
            turns[i] += mix * wrap(turn - turns[i]);
            touched.insert(i);
        });
    });
    touched.into_iter().for_each(|i| {
        angles.insert(bones[i].name.clone(), (turns[i] / RAD) as f32);
    });
    angles
}

pub fn skinning(bones: &[Bone], ik: &[Ik], pose: &Pose) -> Vec<Mat> {
    let angles = applied(bones, ik, pose);
    let rest: Vec<Mat> = bones.iter().map(|b| placed(b.head, b.angle())).collect();
    let mut posed: Vec<Mat> = Vec::with_capacity(bones.len());
    bones.iter().enumerate().for_each(|(i, b)| {
        let degrees = angles.get(&b.name).copied().unwrap_or(0.0);
        let turn = placed([0.0, 0.0], degrees * (std::f32::consts::PI / 180.0));
        let world = match parent_of(bones, i) {
            Some(p) => mul(posed[p], mul(mul(invert(rest[p]), rest[i]), turn)),
            None => mul([1.0, 0.0, 0.0, 1.0, pose.root[0], pose.root[1]], mul(rest[i], turn)),
        };
        posed.push(world);
    });
    posed.iter().zip(&rest).map(|(p, r)| mul(*p, invert(*r))).collect()
}

pub fn tips(bones: &[Bone], ik: &[Ik], pose: &Pose) -> Vec<([f32; 2], [f32; 2])> {
    let mats = skinning(bones, ik, pose);
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub skeleton: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone: Option<String>,
}

pub struct Rig<'a> {
    pub bones: &'a [Bone],
    pub weights: &'a Weights,
    pub mats: Vec<Mat>,
    pub on: Option<usize>,
}

impl<'a> Rig<'a> {
    pub fn new(bones: &'a [Bone], ik: &[Ik], weights: &'a Weights, pose: &Pose) -> Self {
        Self {
            bones,
            weights,
            mats: skinning(bones, ik, pose),
            on: None,
        }
    }

    pub fn following(self, bone: Option<&str>) -> Self {
        let on = bone.and_then(|b| self.bones.iter().position(|o| o.name == b));
        Self { on, ..self }
    }

    pub fn moved(&self, p: [f32; 2]) -> [f32; 2] {
        if let Some(m) = self.on.and_then(|i| self.mats.get(i)) {
            return apply(*m, p);
        }
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
        let mats: Vec<&Mat> = match self.on {
            Some(i) => self.mats.get(i).into_iter().collect(),
            None => self.mats.iter().collect(),
        };
        if mats.iter().all(|m| {
            **m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
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

pub fn moved_weights(w: &Weights, back: crate::markedit::Affine) -> Weights {
    let cell = CELL as f32;
    let (width, height) = (w.width as i64, w.height as i64);
    Weights {
        width: w.width,
        height: w.height,
        maps: w
            .maps
            .iter()
            .map(|(bone, map)| {
                let moved = (0..height)
                    .flat_map(|y| (0..width).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        let (sx, sy) =
                            crate::markedit::apply_affine(back, ((x as f32 + 0.5) * cell, (y as f32 + 0.5) * cell));
                        let (cx, cy) = ((sx / cell).floor() as i64, (sy / cell).floor() as i64);
                        match (0..width).contains(&cx) && (0..height).contains(&cy) {
                            true => map[(cy * width + cx) as usize],
                            false => 0,
                        }
                    })
                    .collect();
                (bone.clone(), moved)
            })
            .collect(),
    }
}

pub fn skeletons(doc: &Document) -> Vec<&Layer> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Bones)
        .collect()
}

pub fn painted<'a>(doc: &'a Document, skeleton: &str) -> Vec<&'a Layer> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Paint && l.rig.as_ref().is_some_and(|r| r.skeleton == skeleton))
        .collect()
}

pub fn painted_paths(doc: &Document, skeleton: &str) -> Vec<Vec<usize>> {
    painted(doc, skeleton)
        .iter()
        .filter_map(|l| match doc.locate(&l.id) {
            Some(crate::doc::Loc::Layer(path)) => Some(path),
            _ => None,
        })
        .collect()
}

pub fn linked<'a>(doc: &'a Document, layer: &Layer) -> Option<&'a Layer> {
    let link = layer.rig.as_ref()?;
    doc.layer(&link.skeleton).filter(|s| s.kind() == Kind::Bones)
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
        painted(doc, &owner.id).iter().try_for_each(|layer| -> Result<()> {
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

    const GOLDEN: [u32; 3] = [1101004800, 3257723871, 1097306278];

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

    fn bone(name: &str, parent: Option<&str>, head: [f32; 2], tail: [f32; 2]) -> Bone {
        Bone {
            name: name.into(),
            parent: parent.map(str::to_string),
            head,
            tail,
        }
    }

    fn limb_on(bone: &str, bend: Bend, softness: f32) -> Ik {
        Ik {
            bone: bone.into(),
            chain: 2,
            bend,
            softness,
        }
    }

    fn reaching(targets: &[(&str, [f32; 2], f32)]) -> Pose {
        Pose {
            ik: targets
                .iter()
                .map(|(b, t, m)| (b.to_string(), Reach { target: *t, mix: *m }))
                .collect(),
            ..Pose::default()
        }
    }

    fn end_of(bones: &[Bone], ik: &[Ik], pose: &Pose, name: &str) -> [f32; 2] {
        let i = bones.iter().position(|b| b.name == name).unwrap();
        tips(bones, ik, pose)[i].1
    }

    fn apart(a: [f32; 2], b: [f32; 2]) -> f32 {
        (a[0] - b[0]).hypot(a[1] - b[1])
    }

    fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    }

    #[test]
    fn a_limb_reaches_its_target_with_an_offset_joint_a_turned_parent_and_a_root_move() {
        let bones = vec![
            bone("pelvis", None, [200.0, 100.0], [200.0, 150.0]),
            bone("thigh", Some("pelvis"), [215.0, 155.0], [220.0, 255.0]),
            bone("shin", Some("thigh"), [222.0, 258.0], [230.0, 360.0]),
        ];
        let target = [300.0, 330.0];
        [Bend::Clockwise, Bend::Counterclockwise].into_iter().for_each(|bend| {
            let ik = vec![limb_on("shin", bend, 0.0)];
            let mut pose = reaching(&[("shin", target, 1.0)]);
            pose.angles.insert("pelvis".into(), 20.0);
            pose.root = [15.0, -10.0];
            let shown = tips(&bones, &ik, &pose);
            assert!(apart(shown[2].1, target) < 0.01, "{bend:?}: {:?}", shown[2].1);
            let side = cross(shown[1].0, target, shown[2].0);
            match bend {
                Bend::Clockwise => assert!(side < 0.0, "a clockwise knee sits on its side: {side}"),
                Bend::Counterclockwise => assert!(side > 0.0, "a counterclockwise knee sits on the other: {side}"),
            }
            let fk = tips(&bones, &[], &pose);
            assert_eq!(shown[0], fk[0], "the pelvis is not part of the limb");
        });
    }

    #[test]
    fn out_of_reach_the_limb_points_straight_at_the_target_and_too_close_it_folds() {
        let bones = vec![
            bone("thigh", None, [100.0, 100.0], [100.0, 200.0]),
            bone("shin", Some("thigh"), [100.0, 200.0], [100.0, 260.0]),
        ];
        let ik = vec![limb_on("shin", Bend::Clockwise, 0.0)];
        let far = end_of(&bones, &ik, &reaching(&[("shin", [500.0, 100.0], 1.0)]), "shin");
        assert!(apart(far, [260.0, 100.0]) < 0.01, "straight at full reach: {far:?}");
        let near = end_of(&bones, &ik, &reaching(&[("shin", [110.0, 100.0], 1.0)]), "shin");
        assert!(apart(near, [140.0, 100.0]) < 0.01, "folded as far as it goes: {near:?}");
        let on = tips(&bones, &ik, &reaching(&[("shin", [100.0, 100.0], 1.0)]));
        assert!(on.iter().all(|(h, t)| h.iter().chain(t).all(|v| v.is_finite())));
        let aim = vec![Ik {
            bone: "shin".into(),
            chain: 1,
            bend: Bend::Clockwise,
            softness: 0.0,
        }];
        let pointed = end_of(&bones, &aim, &reaching(&[("shin", [400.0, 200.0], 1.0)]), "shin");
        assert!(
            apart(pointed, [160.0, 200.0]) < 0.01,
            "an aim turns the bone alone: {pointed:?}"
        );
        let still = tips(&bones, &aim, &reaching(&[("shin", [100.0, 200.0], 1.0)]));
        assert_eq!(
            still,
            tips(&bones, &[], &Pose::default()),
            "a target on its head keeps the turn"
        );
    }

    #[test]
    fn softness_eases_the_limb_into_full_reach() {
        let bones = vec![
            bone("thigh", None, [0.0, 0.0], [0.0, 100.0]),
            bone("shin", Some("thigh"), [0.0, 100.0], [0.0, 200.0]),
        ];
        let sweep = |softness: f32| -> (Vec<f32>, Vec<f32>) {
            let ik = vec![limb_on("shin", Bend::Clockwise, softness)];
            (0..=240)
                .map(|i| {
                    let d = 140.0 + i as f32 * 0.5;
                    let pose = reaching(&[("shin", [0.0, d], 1.0)]);
                    let reach = end_of(&bones, &ik, &pose, "shin")[1];
                    (reach, applied(&bones, &ik, &pose)["thigh"])
                })
                .unzip()
        };
        let (hard, hard_turn) = sweep(0.0);
        let (soft, soft_turn) = sweep(20.0);
        assert!(soft.windows(2).all(|w| w[1] >= w[0] - 1e-3), "it never pulls back");
        assert!(
            (soft[0] - 140.0).abs() < 0.01,
            "reaches the target well inside full reach"
        );
        assert!(
            (soft[240] - 200.0).abs() < 0.01,
            "straight once the target is softness past full reach"
        );
        assert!(
            (soft[120] - 195.0).abs() < 0.01 && hard[120] == 200.0,
            "at full reach a soft limb is still a quarter of its softness short: {}",
            soft[120]
        );
        let step = |turns: &[f32]| turns.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(
            step(&hard_turn) > 2.0,
            "without softness the knee snaps: {}",
            step(&hard_turn)
        );
        assert!(step(&soft_turn) < 0.6, "with softness it eases: {}", step(&soft_turn));
    }

    #[test]
    fn mix_blends_from_the_turns_to_the_target_the_short_way() {
        let bones = vec![
            bone("upper", None, [0.0, 0.0], [100.0, 0.0]),
            bone("fore", Some("upper"), [100.0, 0.0], [200.0, 0.0]),
        ];
        let ik = vec![Ik {
            bone: "fore".into(),
            chain: 1,
            bend: Bend::Clockwise,
            softness: 0.0,
        }];
        let at = |mix: f32| {
            let mut pose = reaching(&[("fore", [100.0, 100.0], mix)]);
            pose.angles.insert("fore".into(), -170.0);
            applied(&bones, &ik, &pose)["fore"]
        };
        let same = |a: f32, b: f32| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs() < 1e-3;
        assert_eq!(at(0.0), -170.0, "mix 0 is the turns");
        assert!(same(at(1.0), 90.0), "{}", at(1.0));
        assert!(
            same(at(0.5), 140.0) && (at(0.5) + 170.0).abs() < 90.0,
            "half way the short way round, through 180: {}",
            at(0.5)
        );
    }

    #[test]
    fn constraints_are_solved_parent_first_whatever_their_order() {
        let bones = vec![
            bone("thigh", None, [0.0, 0.0], [0.0, 100.0]),
            bone("shin", Some("thigh"), [0.0, 100.0], [0.0, 200.0]),
            bone("foot", Some("shin"), [0.0, 200.0], [30.0, 200.0]),
        ];
        let ik = vec![
            Ik {
                bone: "foot".into(),
                chain: 1,
                bend: Bend::Clockwise,
                softness: 0.0,
            },
            limb_on("shin", Bend::Clockwise, 0.0),
        ];
        let pose = reaching(&[("shin", [60.0, 150.0], 1.0), ("foot", [200.0, 150.0], 1.0)]);
        let shown = tips(&bones, &ik, &pose);
        assert!(
            apart(shown[1].1, [60.0, 150.0]) < 0.01,
            "the leg reaches: {:?}",
            shown[1].1
        );
        let (h, t) = shown[2];
        let pointing = libm::atan2f(t[1] - h[1], t[0] - h[0]);
        let wanted = libm::atan2f(150.0 - h[1], 200.0 - h[0]);
        assert!(
            (pointing - wanted).abs() < 1e-4,
            "the foot aims from where the leg put it"
        );
    }

    #[test]
    fn in_betweens_mix_the_targets_so_a_planted_foot_stays_planted() {
        let bones = vec![
            bone("thigh", None, [0.0, 0.0], [0.0, 100.0]),
            bone("shin", Some("thigh"), [0.0, 100.0], [0.0, 200.0]),
        ];
        let ik = vec![limb_on("shin", Bend::Clockwise, 0.0)];
        let key = |x: f32| Pose {
            root: [x, 0.0],
            ..reaching(&[("shin", [0.0, 180.0], 1.0)])
        };
        let (a, b) = (key(-60.0), key(60.0));
        (0..=10).map(|i| i as f32 / 10.0).for_each(|t| {
            let foot = end_of(&bones, &ik, &between(&bones, &ik, &a, &b, t), "shin");
            assert!(apart(foot, [0.0, 180.0]) < 1e-3, "t {t}: {foot:?}");
        });
        let turned = |p: &Pose| Pose {
            angles: applied(&bones, &ik, p),
            root: p.root,
            ..Pose::default()
        };
        let slid = end_of(
            &bones,
            &[],
            &between(&bones, &[], &turned(&a), &turned(&b), 0.5),
            "shin",
        );
        assert!(
            (slid[1] - 189.74).abs() < 0.05,
            "mixing the solved turns instead pushes the foot through the floor: {slid:?}"
        );
    }

    #[test]
    fn a_limb_on_ik_in_one_key_only_hands_over_without_a_jump() {
        let bones = vec![
            bone("upper", None, [0.0, 0.0], [100.0, 0.0]),
            bone("fore", Some("upper"), [100.0, 0.0], [200.0, 0.0]),
        ];
        let ik = vec![limb_on("fore", Bend::Clockwise, 0.0)];
        let a = reaching(&[("fore", [120.0, 80.0], 1.0)]);
        let mut b = Pose::default();
        b.angles.insert("upper".into(), 60.0);
        let at = |t: f32| end_of(&bones, &ik, &between(&bones, &ik, &a, &b, t), "fore");
        assert!(apart(at(0.0), [120.0, 80.0]) < 0.01);
        assert!(apart(at(1.0), end_of(&bones, &[], &b, "fore")) < 0.01);
        let path: Vec<[f32; 2]> = (0..=20).map(|i| at(i as f32 / 20.0)).collect();
        assert!(
            path.windows(2).all(|w| apart(w[0], w[1]) < 25.0),
            "no jump on the way: {path:?}"
        );
    }

    #[test]
    fn bad_constraints_are_refused_and_the_rest_bend_is_read_from_the_drawing() {
        let bones = vec![
            bone("thigh", None, [0.0, 0.0], [0.0, 100.0]),
            bone("shin", Some("thigh"), [0.0, 100.0], [-30.0, 190.0]),
            bone("spur", Some("thigh"), [0.0, 0.0], [20.0, 10.0]),
        ];
        let refused = |ik: Vec<Ik>| check_ik(&bones, &ik).unwrap_err().to_string();
        assert!(refused(vec![limb_on("calf", Bend::Clockwise, 0.0)]).contains("no bone calf"));
        assert!(refused(vec![limb_on("thigh", Bend::Clockwise, 0.0)]).contains("has none"));
        assert!(refused(vec![limb_on("spur", Bend::Clockwise, 0.0)]).contains("starts where its parent"));
        assert!(
            refused(vec![Ik {
                chain: 3,
                ..limb_on("shin", Bend::Clockwise, 0.0)
            }])
            .contains("not 3")
        );
        assert!(refused(vec![limb_on("shin", Bend::Clockwise, -1.0)]).contains("softness"));
        assert!(
            refused(vec![
                limb_on("shin", Bend::Clockwise, 0.0),
                limb_on("shin", Bend::Counterclockwise, 0.0)
            ])
            .contains("two IKs")
        );
        assert!(check_ik(&bones, &[limb_on("shin", Bend::Clockwise, 4.0)]).is_ok());
        assert_eq!(
            rest_bend(&bones, "shin"),
            Bend::Clockwise,
            "the shin turns clockwise off the thigh"
        );
        let mirrored = vec![
            bones[0].clone(),
            bone("shin", Some("thigh"), [0.0, 100.0], [30.0, 190.0]),
        ];
        assert_eq!(rest_bend(&mirrored, "shin"), Bend::Counterclockwise);
        let straight = vec![
            bones[0].clone(),
            bone("shin", Some("thigh"), [0.0, 100.0], [0.0, 200.0]),
        ];
        assert_eq!(rest_bend(&straight, "shin"), Bend::Clockwise);
        let ik = vec![limb_on("shin", rest_bend(&bones, "shin"), 0.0)];
        let rest_tip = end_of(&bones, &[], &Pose::default(), "shin");
        let at_rest = tips(&bones, &ik, &reaching(&[("shin", rest_tip, 1.0)]));
        assert!(
            at_rest
                .iter()
                .zip(tips(&bones, &[], &Pose::default()))
                .all(|(a, b)| apart(a.0, b.0) < 1e-3 && apart(a.1, b.1) < 1e-3),
            "a target on the limb's end at rest moves nothing"
        );
    }

    #[test]
    fn a_limb_handed_over_from_one_key_never_jumps_whatever_its_mix() {
        let bones = vec![
            bone("upper", None, [300.0, 300.0], [400.0, 300.0]),
            bone("fore", Some("upper"), [400.0, 300.0], [500.0, 300.0]),
        ];
        let ik = vec![limb_on("fore", Bend::Clockwise, 0.0)];
        let mut b = Pose::default();
        b.angles.insert("upper".into(), -150.0);
        [1.0, 0.5].into_iter().for_each(|mix| {
            let a = reaching(&[("fore", [150.0, 300.0], mix)]);
            let at = |t: f32| end_of(&bones, &ik, &between(&bones, &ik, &a, &b, t), "fore");
            assert!(
                apart(at(0.0), end_of(&bones, &ik, &a, "fore")) < 0.01,
                "mix {mix}: starts as the key shows"
            );
            assert!(
                apart(at(1.0), end_of(&bones, &ik, &b, "fore")) < 0.01,
                "mix {mix}: ends on the other key"
            );
            let path: Vec<[f32; 2]> = (0..=400).map(|i| at(i as f32 / 400.0)).collect();
            let step = path.windows(2).map(|w| apart(w[0], w[1])).fold(0.0, f32::max);
            assert!(step < 3.0, "mix {mix}: the hand never jumps, at most {step} px a step");
        });
    }

    #[test]
    fn a_target_made_at_a_limbs_end_moves_nothing_even_with_softness() {
        let straight = vec![
            bone("thigh", None, [300.0, 100.0], [300.0, 200.0]),
            bone("shin", Some("thigh"), [300.0, 200.0], [300.0, 300.0]),
        ];
        let bent = vec![
            straight[0].clone(),
            bone("shin", Some("thigh"), [300.0, 200.0], [320.0, 297.0]),
        ];
        [(&straight, 20.0), (&bent, 20.0), (&bent, 0.0)]
            .into_iter()
            .for_each(|(bones, softness)| {
                let ik = vec![limb_on("shin", rest_bend(bones, "shin"), softness)];
                let fk = tips(bones, &ik, &Pose::default());
                let target = target_for_end(bones, &ik, &Pose::default(), "shin").unwrap();
                let held = tips(bones, &ik, &reaching(&[("shin", target, 1.0)]));
                assert!(
                    held.iter()
                        .zip(&fk)
                        .all(|(a, b)| apart(a.0, b.0) < 1e-3 && apart(a.1, b.1) < 1e-3),
                    "softness {softness}: {held:?} {fk:?}"
                );
            });
        let ik = vec![limb_on("shin", Bend::Clockwise, 0.0)];
        let mut against = Pose::default();
        against.angles.insert("thigh".into(), 20.0);
        against.angles.insert("shin".into(), -40.0);
        assert!(
            bent_against(&straight, &ik, &against, "shin"),
            "a knee turned the other way is noticed"
        );
        let mut along = Pose::default();
        along.angles.insert("shin".into(), 40.0);
        assert!(!bent_against(&straight, &ik, &along, "shin"));
    }

    #[test]
    fn poses_that_are_not_numbers_or_mix_out_of_range_are_refused() {
        let good = reaching(&[("shin", [1.0, 2.0], 0.5)]);
        assert!(check_pose(&good).is_ok());
        let wrong = |pose: Pose| check_pose(&pose).unwrap_err().to_string();
        assert!(wrong(reaching(&[("shin", [f32::NAN, 2.0], 1.0)])).contains("target of shin"));
        assert!(wrong(reaching(&[("shin", [1.0, 2.0], 7.0)])).contains("mix of shin"));
        assert!(
            wrong(Pose {
                root: [f32::INFINITY, 0.0],
                ..Pose::default()
            })
            .contains("root")
        );
        let mut turned = Pose::default();
        turned.angles.insert("thigh".into(), f32::NAN);
        assert!(wrong(turned).contains("thigh"));
        let mut far = bone("far", None, [0.0, 0.0], [1.0, 0.0]);
        far.tail[0] = f32::INFINITY;
        assert!(check(&[far]).is_err());
    }

    #[test]
    fn the_solver_gives_the_same_numbers_everywhere() {
        let bones = vec![
            bone("pelvis", None, [200.0, 100.0], [200.0, 150.0]),
            bone("thigh", Some("pelvis"), [215.0, 155.0], [220.0, 255.0]),
            bone("shin", Some("thigh"), [222.0, 258.0], [230.0, 360.0]),
        ];
        let ik = vec![limb_on("shin", Bend::Clockwise, 6.0)];
        let mut pose = reaching(&[("shin", [300.0, 330.0], 0.8)]);
        pose.angles.insert("pelvis".into(), 20.0);
        pose.angles.insert("thigh".into(), -12.5);
        pose.root = [15.0, -10.0];
        let solved = applied(&bones, &ik, &pose);
        let bits: Vec<u32> = ["pelvis", "thigh", "shin"]
            .iter()
            .map(|b| solved[*b].to_bits())
            .collect();
        assert_eq!(bits, GOLDEN, "{solved:?}");
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
        let rest = Rig::new(&bones, &[], &weights, &Pose::default());
        assert_eq!(rest.pose(std::slice::from_ref(&line), (200, 400)), vec![line.clone()]);
        let mut bent = Pose::default();
        bent.angles.insert("shin".into(), 60.0);
        let posed = Rig::new(&bones, &[], &weights, &bent).pose(std::slice::from_ref(&line), (200, 400));
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
        let moved = Rig::new(&bones, &[], &rigid, &swing).pose(std::slice::from_ref(&line), (200, 400));
        let s = moved[0].as_stroke().unwrap();
        let (a, b) = (s.samples[0].pos(), s.samples[s.samples.len() - 1].pos());
        assert!(
            ((b.0 - a.0).hypot(b.1 - a.1) - 200.0).abs() < 1.0,
            "a mark held by one bone moves rigidly"
        );
    }
}
