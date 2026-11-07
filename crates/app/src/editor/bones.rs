use super::{Editor, Tool};
use crate::jobs::{Spec, Tag};
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::{Context, Result, bail};
use egui::{Color32, Key, Modifiers, Pos2, RichText, Vec2};
use klad_core::doc::Kind;
use klad_core::ops::{BendSpec, BoneSpec, IkSpec, Op, WeightsSpec};
use klad_core::rig::{Bend, Bone, CELL, Ik, Pose, Reach, Weights, applied, moved, painted, subtree, tips, weights_for};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Draw,
    Weights,
    Pose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grip {
    Head,
    Tail,
    Whole,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Dragging {
    New(Option<String>),
    Bone(String, Grip),
}

#[derive(Clone, Debug)]
pub(super) struct Drag {
    pub from: Vec2,
    pub to: Vec2,
    pub what: Dragging,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Chain {
    pub owner: String,
    pub parent: Option<String>,
    pub head: [f32; 2],
    pub joined: bool,
    pub drawn: Vec<String>,
    pub to: Vec2,
    pub pressed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Posing {
    Turn(String),
    Reach(String, Vec2),
    Root(Vec2),
}

pub(super) struct BoneTool {
    pub mode: Mode,
    pub selected: Option<String>,
    pub drag: Option<Drag>,
    pub chain: Option<Chain>,
    pub dabs: Vec<[f32; 4]>,
    pub radius: f32,
    pub strength: f32,
    pub pose: BTreeMap<String, f32>,
    pub root: [f32; 2],
    pub reach: BTreeMap<String, Reach>,
    pub loaded: Option<(u32, String, Option<Pose>)>,
    pub posing: Option<Posing>,
    pub name: String,
    pub heat: Option<(String, u64, egui::TextureHandle)>,
}

impl Default for BoneTool {
    fn default() -> Self {
        Self {
            mode: Mode::Draw,
            selected: None,
            drag: None,
            chain: None,
            dabs: Vec::new(),
            radius: 24.0,
            strength: 1.0,
            pose: BTreeMap::new(),
            root: [0.0, 0.0],
            reach: BTreeMap::new(),
            loaded: None,
            posing: None,
            name: String::new(),
            heat: None,
        }
    }
}

const NEAR: f32 = 10.0;
const CLICK: f32 = 5.0;
const JOINED: f32 = 0.5;
const TARGET: Color32 = Color32::from_rgb(255, 120, 50);

type Later = Box<dyn FnOnce(&mut Editor) -> Result<()>>;

fn near_segment(p: Vec2, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (a, b) = (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + d * t - p).length()
}

fn at(p: [f32; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn limb(bones: &[Bone], k: &Ik) -> Vec<String> {
    let parent = bones
        .iter()
        .find(|b| b.name == k.bone)
        .and_then(|b| b.parent.clone())
        .filter(|_| k.chain == 2);
    std::iter::once(k.bone.clone()).chain(parent).collect()
}

pub(super) fn pose_args(bundle: &std::path::Path, owner: &str, frame: u32, pose: &Pose) -> Vec<String> {
    let mut args = vec![
        "pose".to_string(),
        bundle.to_string_lossy().to_string(),
        owner.to_string(),
        "--from".into(),
        frame.to_string(),
        "--to".into(),
        frame.to_string(),
    ];
    pose.angles
        .iter()
        .filter(|(_, v)| v.abs() > 1e-3)
        .for_each(|(k, v)| args.extend(["--bone".to_string(), format!("{k}={v:.2}")]));
    if pose.root != [0.0, 0.0] {
        args.extend(["--root".to_string(), format!("{:.2},{:.2}", pose.root[0], pose.root[1])]);
    }
    pose.ik.iter().for_each(|(k, r)| {
        args.extend([
            "--target".to_string(),
            format!("{k}={:.2},{:.2}", r.target[0], r.target[1]),
        ]);
        if r.mix != 1.0 {
            args.extend(["--mix".to_string(), format!("{k}={:.3}", r.mix)]);
        }
    });
    args
}

impl Editor {
    pub(super) fn rig_owner(&self) -> Option<String> {
        let doc = &self.scene.bundle.doc;
        let path = self.scene.active.clone();
        (1..=path.len())
            .rev()
            .map(|n| doc.layer_at(&path[..n]))
            .find(|l| !l.bones.is_empty())
            .or_else(|| match path.len() {
                0 => None,
                1 => Some(doc.layer_at(&path)).filter(|l| matches!(l.kind(), Kind::Paint | Kind::Group)),
                _ => Some(doc.layer_at(&path[..1])),
            })
            .map(|l| l.id.clone())
    }

    fn rig_bones(&self) -> Vec<Bone> {
        self.rig_owner()
            .and_then(|id| self.scene.bundle.doc.layer(&id).map(|l| l.bones.clone()))
            .unwrap_or_default()
    }

    fn rig_ik(&self) -> Vec<Ik> {
        self.rig_owner()
            .and_then(|id| self.scene.bundle.doc.layer(&id).map(|l| l.ik.clone()))
            .unwrap_or_default()
    }

    fn shown_pose(&self) -> Pose {
        match self.bones.mode {
            Mode::Pose => Pose {
                rest: String::new(),
                angles: self.bones.pose.clone(),
                root: self.bones.root,
                ik: self.bones.reach.clone(),
            },
            _ => Pose::default(),
        }
    }

    fn stored_pose(&self, owner: &str) -> Option<Pose> {
        let layer = self.scene.bundle.doc.layer(owner)?;
        painted(layer)
            .iter()
            .find_map(|l| l.content_at(self.scene.frame).and_then(|c| c.pose.clone()))
    }

    pub(super) fn bone_keys(&mut self, ui: &mut egui::Ui) {
        self.tend_chain();
        self.tend_pose();
        if self.bones.chain.is_none() {
            return;
        }
        let (enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if enter || escape {
            self.end_chain();
        }
    }

    pub(super) fn end_chain(&mut self) {
        if self.bones.chain.take().is_some() {
            self.say(Tone::Info, "chain ended");
        }
    }

    fn tend_chain(&mut self) {
        let Some(mut chain) = self.bones.chain.take() else {
            return;
        };
        if self.tool != Tool::Bone || self.bones.mode != Mode::Draw || self.rig_owner().as_deref() != Some(&chain.owner)
        {
            return;
        }
        let bones = self.rig_bones();
        let exists = |n: &str| bones.iter().any(|b| b.name == n);
        if chain.parent.as_deref().is_some_and(|p| !exists(p)) {
            while chain.drawn.last().is_some_and(|n| !exists(n)) {
                chain.drawn.pop();
            }
            let Some(last) = chain.drawn.last() else {
                return;
            };
            chain.parent = Some(last.clone());
            chain.joined = true;
        }
        if chain.joined
            && let Some(p) = chain.parent.as_ref().and_then(|p| bones.iter().find(|b| &b.name == p))
        {
            chain.head = p.tail;
        }
        self.bones.chain = Some(chain);
    }

    fn tend_pose(&mut self) {
        if self.bones.mode != Mode::Pose {
            self.bones.loaded = None;
            return;
        }
        let Some(owner) = self.rig_owner() else {
            return;
        };
        let stored = self.stored_pose(&owner);
        let key = (self.scene.frame, owner, stored.clone());
        if self.bones.loaded.as_ref() == Some(&key) {
            return;
        }
        let pose = stored.unwrap_or_default();
        self.bones.pose = pose.angles;
        self.bones.root = pose.root;
        self.bones.reach = pose.ik;
        self.bones.posing = None;
        self.bones.loaded = Some(key);
    }

    fn add_bone(&mut self, parent: Option<String>, head: [f32; 2], tail: [f32; 2]) -> Result<Bone> {
        let owner = self
            .rig_owner()
            .context("pick the layer or group the bones go on first")?;
        let taken = self.rig_bones();
        let name = (1..)
            .map(|n| format!("bone {n}"))
            .find(|n| !taken.iter().any(|b| &b.name == n))
            .unwrap_or_default();
        self.apply(vec![Op::Bone(BoneSpec {
            layer: owner,
            add: Some(name.clone()),
            parent: parent.clone(),
            head: Some(head),
            tail: Some(tail),
            ..BoneSpec::default()
        })])?;
        self.pick_bone(Some(name.clone()));
        Ok(Bone {
            name,
            parent,
            head,
            tail,
        })
    }

    fn chain_on(&mut self, drawn: Vec<String>, made: &Bone) {
        let Some(owner) = self.rig_owner() else {
            return;
        };
        let bones = self.rig_bones();
        let sibling = self.modifiers.shift;
        let joined = |b: &Bone| {
            b.parent
                .as_ref()
                .and_then(|p| bones.iter().find(|o| &o.name == p))
                .is_some_and(|p| (at(p.tail) - at(b.head)).length() <= JOINED)
        };
        self.bones.chain = Some(Chain {
            owner,
            parent: match sibling {
                true => made.parent.clone(),
                false => Some(made.name.clone()),
            },
            head: if sibling { made.head } else { made.tail },
            joined: !sibling || joined(made),
            drawn,
            to: at(made.tail),
            pressed: false,
        });
    }

    fn chain_up(&mut self) -> Result<()> {
        self.tend_chain();
        let Some(chain) = self.bones.chain.clone() else {
            return Ok(());
        };
        if !chain.pressed {
            return Ok(());
        }
        if let Some(c) = self.bones.chain.as_mut() {
            c.pressed = false;
        }
        if (chain.to - at(chain.head)).length() * self.zoom() < CLICK {
            self.end_chain();
            return Ok(());
        }
        let made = self.add_bone(chain.parent.clone(), chain.head, [chain.to.x, chain.to.y])?;
        let mut drawn = chain.drawn;
        drawn.push(made.name.clone());
        self.chain_on(drawn, &made);
        Ok(())
    }

    fn limb_of(&self, name: &str) -> Option<String> {
        let bones = self.rig_bones();
        self.rig_ik()
            .into_iter()
            .filter(|k| self.bones.reach.get(&k.bone).is_some_and(|r| r.mix > 0.0))
            .find(|k| limb(&bones, k).iter().any(|b| b == name))
            .map(|k| k.bone)
    }

    fn ik_on(&mut self, end: &str) {
        let bones = self.rig_bones();
        let Some(i) = bones.iter().position(|b| b.name == end) else {
            return;
        };
        let tip = tips(&bones, &self.rig_ik(), &self.shown_pose())[i].1;
        self.bones
            .reach
            .insert(end.to_string(), Reach { target: tip, mix: 1.0 });
    }

    fn ik_off(&mut self, end: &str) {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let solved = applied(&bones, &ik, &self.shown_pose());
        if let Some(k) = ik.iter().find(|k| k.bone == end) {
            limb(&bones, k).into_iter().for_each(|b| {
                if let Some(v) = solved.get(&b) {
                    self.bones.pose.insert(b, *v);
                }
            });
        }
        self.bones.reach.remove(end);
    }

    fn zoom(&self) -> f32 {
        self.view.map_or(1.0, |v| v.zoom)
    }

    pub(super) fn pick_bone(&mut self, name: Option<String>) {
        self.bones.name = name.clone().unwrap_or_default();
        self.bones.selected = name;
        self.bones.heat = None;
    }

    fn grip_at(&self, c: Vec2) -> Option<(String, String, Grip)> {
        let bones = self.rig_bones();
        let reach = NEAR / self.zoom();
        let at = |p: [f32; 2]| (Vec2::new(p[0], p[1]) - c).length();
        let picked = self.bones.selected.as_deref();
        let joint = |b: &Bone| {
            b.parent
                .as_ref()
                .and_then(|p| bones.iter().find(|o| &o.name == p))
                .filter(|p| (p.tail[0] - b.head[0]).hypot(p.tail[1] - b.head[1]) <= JOINED)
                .map(|p| p.name.clone())
        };
        bones
            .iter()
            .flat_map(|b| {
                [
                    (Grip::Tail, at(b.tail)),
                    (Grip::Head, at(b.head)),
                    (Grip::Whole, near_segment(c, b.head, b.tail)),
                ]
                .into_iter()
                .filter(move |(_, d)| *d <= reach)
                .map(move |(g, d)| ((g == Grip::Whole, picked != Some(b.name.as_str()), d), b, g))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, b, g)| match (g, joint(b)) {
                (Grip::Head, Some(parent)) => (b.name.clone(), parent, Grip::Tail),
                _ => (b.name.clone(), b.name.clone(), g),
            })
    }

    fn drag_spec(&self, drag: &Drag) -> Option<BoneSpec> {
        let Dragging::Bone(name, grip) = &drag.what else {
            return None;
        };
        let bone = self.rig_bones().into_iter().find(|b| &b.name == name)?;
        let d = drag.to - drag.from;
        let by = |p: [f32; 2]| [p[0] + d.x, p[1] + d.y];
        Some(BoneSpec {
            layer: self.rig_owner()?,
            moved: Some(name.clone()),
            head: (*grip == Grip::Head).then(|| by(bone.head)),
            tail: (*grip == Grip::Tail).then(|| by(bone.tail)),
            by: (*grip == Grip::Whole).then_some([d.x, d.y]),
            ..BoneSpec::default()
        })
    }

    fn drawn_bones(&self) -> Vec<Bone> {
        let bones = self.rig_bones();
        self.bones
            .drag
            .as_ref()
            .filter(|d| (d.to - d.from).length() * self.zoom() >= CLICK)
            .and_then(|d| self.drag_spec(d))
            .and_then(|s| moved(&bones, s.moved.as_deref()?, s.head, s.tail, s.by).ok())
            .unwrap_or(bones)
    }

    fn bone_at(&self, c: Vec2) -> Option<String> {
        let bones = self.rig_bones();
        let zoom = self.zoom();
        let shown = tips(&bones, &self.rig_ik(), &self.shown_pose());
        bones
            .iter()
            .zip(shown)
            .map(|(b, (h, t))| (b.name.clone(), near_segment(c, h, t)))
            .filter(|(_, d)| *d <= NEAR / zoom)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    }

    pub(super) fn bone_down(&mut self, c: Vec2) {
        match self.bones.mode {
            Mode::Draw if self.bones.chain.is_some() => {
                if let Some(chain) = self.bones.chain.as_mut() {
                    chain.pressed = true;
                    chain.to = c;
                }
            }
            Mode::Draw if self.modifiers.alt => {
                let bones = self.rig_bones();
                let reach = NEAR / self.zoom();
                let from_tail = bones
                    .iter()
                    .find(|b| (at(b.tail) - c).length() <= reach)
                    .map(|b| (b.name.clone(), at(b.tail)));
                let (parent, from) = match from_tail {
                    Some((name, tail)) => (Some(name), tail),
                    None => (self.bone_at(c), c),
                };
                self.bones.drag = Some(Drag {
                    from,
                    to: c,
                    what: Dragging::New(parent),
                });
            }
            Mode::Draw => {
                let what = match self.grip_at(c) {
                    Some((picked, moving, grip)) => {
                        if self.bones.selected.as_ref() != Some(&picked) {
                            self.pick_bone(Some(picked));
                        }
                        Dragging::Bone(moving, grip)
                    }
                    None => Dragging::New(None),
                };
                self.bones.drag = Some(Drag { from: c, to: c, what });
            }
            Mode::Weights if self.modifiers.command => {
                let found = self.bone_at(c);
                if found.is_some() {
                    self.pick_bone(found);
                }
            }
            Mode::Weights => {
                self.bones.dabs.clear();
                self.bone_dab(c);
            }
            Mode::Pose => self.pose_down(c),
        }
    }

    fn pose_down(&mut self, c: Vec2) {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let shown = tips(&bones, &ik, &self.shown_pose());
        let reach = NEAR / self.zoom();
        let alt = self.modifiers.alt;
        let handle = ik
            .iter()
            .filter_map(|k| {
                let i = bones.iter().position(|b| b.name == k.bone)?;
                let tip = shown[i].1;
                let target = self.bones.reach.get(&k.bone).map(|r| r.target);
                let d = (at(tip) - c)
                    .length()
                    .min(target.map_or(f32::MAX, |t| (at(t) - c).length()));
                (d <= reach).then(|| (d, k.bone.clone(), target.unwrap_or(tip)))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if !alt && let Some((_, end, target)) = handle {
            let target = self
                .bones
                .reach
                .entry(end.clone())
                .or_insert(Reach { target, mix: 1.0 })
                .target;
            self.bones.posing = Some(Posing::Reach(end.clone(), at(target) - c));
            self.pick_bone(Some(end));
            return;
        }
        let root = bones
            .iter()
            .zip(&shown)
            .filter(|(b, _)| b.parent.is_none())
            .map(|(b, (h, _))| ((at(*h) - c).length(), b.name.clone()))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if !alt && let Some((_, name)) = root {
            self.bones.posing = Some(Posing::Root(c));
            self.pick_bone(Some(name));
            return;
        }
        let Some(name) = self.bone_at(c) else {
            self.bones.posing = None;
            return;
        };
        self.pick_bone(Some(name.clone()));
        self.bones.posing = match self.limb_of(&name) {
            Some(end) if !alt => {
                let target = self.bones.reach[&end].target;
                Some(Posing::Reach(end, at(target) - c))
            }
            Some(end) => {
                self.ik_off(&end);
                Some(Posing::Turn(name))
            }
            None => Some(Posing::Turn(name)),
        };
    }

    fn bone_dab(&mut self, c: Vec2) {
        let value = if self.modifiers.alt { 0.0 } else { self.bones.strength };
        if self
            .bones
            .dabs
            .last()
            .is_none_or(|d| (Vec2::new(d[0], d[1]) - c).length() > self.bones.radius * 0.25)
        {
            self.bones.dabs.push([c.x, c.y, self.bones.radius, value]);
        }
    }

    pub(super) fn bone_move(&mut self, c: Vec2) {
        match self.bones.mode {
            Mode::Draw => {
                if let Some(chain) = self.bones.chain.as_mut() {
                    chain.to = c;
                }
                if let Some(drag) = self.bones.drag.as_mut() {
                    drag.to = c;
                }
            }
            Mode::Weights => {
                if !self.bones.dabs.is_empty() {
                    self.bone_dab(c);
                }
            }
            Mode::Pose => match self.bones.posing.clone() {
                Some(Posing::Turn(name)) => {
                    let bones = self.rig_bones();
                    let Some(i) = bones.iter().position(|b| b.name == name) else {
                        return;
                    };
                    let shown = tips(&bones, &self.rig_ik(), &self.shown_pose());
                    let (head, tail) = shown[i];
                    let now = (tail[1] - head[1]).atan2(tail[0] - head[0]);
                    let want = (c.y - head[1]).atan2(c.x - head[0]);
                    let mut turn = (want - now).to_degrees();
                    turn = (turn + 540.0).rem_euclid(360.0) - 180.0;
                    *self.bones.pose.entry(name).or_insert(0.0) += turn;
                }
                Some(Posing::Reach(end, offset)) => {
                    if let Some(r) = self.bones.reach.get_mut(&end) {
                        r.target = [c.x + offset.x, c.y + offset.y];
                    }
                }
                Some(Posing::Root(last)) => {
                    self.bones.root = [self.bones.root[0] + c.x - last.x, self.bones.root[1] + c.y - last.y];
                    self.bones.posing = Some(Posing::Root(c));
                }
                None => {}
            },
        }
    }

    pub(super) fn bone_up(&mut self) -> Result<()> {
        match self.bones.mode {
            Mode::Draw if self.bones.chain.is_some() => self.chain_up()?,
            Mode::Draw => {
                let Some(drag) = self.bones.drag.take() else {
                    return Ok(());
                };
                let short = (drag.to - drag.from).length() * self.zoom() < CLICK;
                let parent = match (&drag.what, short) {
                    (Dragging::Bone(..), true) => return Ok(()),
                    (Dragging::Bone(..), false) => {
                        let spec = self.drag_spec(&drag).context("that bone is gone")?;
                        return self.apply(vec![Op::Bone(spec)]);
                    }
                    (Dragging::New(_), true) => {
                        self.pick_bone(self.bone_at(drag.to));
                        return Ok(());
                    }
                    (Dragging::New(parent), false) => parent.clone(),
                };
                let made = self.add_bone(parent, [drag.from.x, drag.from.y], [drag.to.x, drag.to.y])?;
                self.chain_on(vec![made.name.clone()], &made);
            }
            Mode::Weights => {
                let dabs = std::mem::take(&mut self.bones.dabs);
                if dabs.is_empty() {
                    return Ok(());
                }
                let bone = self
                    .bones
                    .selected
                    .clone()
                    .context("pick a bone to paint its weights: cmd click it, or pick it in the skeleton list")?;
                self.paint_weights(WeightsSpec {
                    layer: self.rig_owner().context("no skeleton here")?,
                    frame: self.scene.frame,
                    auto: false,
                    bone: Some(bone),
                    set: dabs,
                    part: None,
                    value: 1.0,
                    smooth: false,
                    resolved: None,
                })?;
            }
            Mode::Pose => self.bones.posing = None,
        }
        Ok(())
    }

    fn paint_weights(&mut self, mut spec: WeightsSpec) -> Result<()> {
        let bundle = &self.scene.bundle;
        let made = weights_for(bundle, &bundle.doc, &spec, &BTreeMap::new())?;
        spec.resolved = Some(made);
        self.apply(vec![Op::Weights(spec)])?;
        self.bones.heat = None;
        Ok(())
    }

    pub(super) fn bone_edit(&mut self, edit: impl FnOnce(&mut BoneSpec)) -> Result<()> {
        let mut spec = BoneSpec {
            layer: self.rig_owner().context("no skeleton here")?,
            ..BoneSpec::default()
        };
        edit(&mut spec);
        self.apply(vec![Op::Bone(spec)])
    }

    fn pose_drawing(&mut self) -> Result<()> {
        let owner = self.rig_owner().context("no skeleton here")?;
        let pose = self.shown_pose();
        if pose.angles.values().all(|v| v.abs() < 1e-3) && pose.root == [0.0, 0.0] && pose.ik.is_empty() {
            bail!("turn a bone first: drag a bone's end in pose mode");
        }
        let frame = self.scene.frame;
        self.start_job(Spec {
            tag: Tag::Edit,
            name: format!("pose at frame {frame}"),
            exe: PathBuf::new(),
            args: pose_args(&self.scene.bundle.root, &owner, frame, &pose),
            env: Vec::new(),
            reruns: 3,
            summary: |out, _| {
                out.lines()
                    .find(|l| l.starts_with("posed"))
                    .unwrap_or_default()
                    .to_string()
            },
            failure: |_, err| super::tweens::failure(err),
        });
        Ok(())
    }

    fn ik_spec(&self, edit: impl FnOnce(&mut IkSpec)) -> Option<IkSpec> {
        let mut spec = IkSpec {
            layer: self.rig_owner()?,
            ..IkSpec::default()
        };
        edit(&mut spec);
        Some(spec)
    }

    fn ik_menu(&mut self, ui: &mut egui::Ui, picked: &str) -> Option<Later> {
        let (bones, ik) = (self.rig_bones(), self.rig_ik());
        let parent = bones.iter().find(|b| b.name == picked)?.parent.clone();
        let mut chosen: Option<IkSpec> = None;
        let name = picked.to_string();
        match ik.iter().find(|k| k.bone == picked) {
            Some(k) => {
                let label = match (k.chain, &parent) {
                    (2, Some(p)) => format!("IK: limb with {p}"),
                    _ => "IK: aim".to_string(),
                };
                ui.menu_button(label, |ui| {
                    if k.chain == 2 {
                        [
                            (Bend::Clockwise, BendSpec::Clockwise, "bends clockwise"),
                            (
                                Bend::Counterclockwise,
                                BendSpec::Counterclockwise,
                                "bends counterclockwise",
                            ),
                        ]
                        .into_iter()
                        .for_each(|(bend, spec, word)| {
                            if ui
                                .selectable_label(k.bend == bend, word)
                                .hint("which way the joint between the two bones bends on screen")
                                .clicked()
                                && k.bend != bend
                            {
                                chosen = self.ik_spec(|s| {
                                    s.set = Some(name.clone());
                                    s.bend = Some(spec);
                                });
                                ui.close();
                            }
                        });
                        let mut soft = k.softness;
                        let r = ui
                            .add(egui::Slider::new(&mut soft, 0.0..=100.0).text("softness"))
                            .hint("px: the limb slows this far before full reach, so it does not snap straight");
                        if (r.drag_stopped() || (r.changed() && !r.dragged())) && soft != k.softness {
                            chosen = self.ik_spec(|s| {
                                s.set = Some(name.clone());
                                s.softness = Some(soft);
                            });
                        }
                        if ui
                            .button("make it an aim")
                            .hint("turn this bone alone to point at the target")
                            .clicked()
                        {
                            chosen = self.ik_spec(|s| {
                                s.set = Some(name.clone());
                                s.chain = Some(1);
                            });
                            ui.close();
                        }
                    } else if let Some(p) = &parent
                        && ui
                            .button(format!("make it a limb with {p}"))
                            .hint("turn this bone and its parent so its end reaches the target")
                            .clicked()
                    {
                        chosen = self.ik_spec(|s| {
                            s.set = Some(name.clone());
                            s.chain = Some(2);
                        });
                        ui.close();
                    }
                    if ui.button("remove IK").hint("poses keep their look, as turns").clicked() {
                        chosen = self.ik_spec(|s| s.rm = Some(name.clone()));
                        ui.close();
                    }
                })
                .response
                .hint("this bone's IK: in pose mode, drag its end and the limb follows");
            }
            None => {
                ui.menu_button("add IK", |ui| {
                    if let Some(p) = &parent
                        && ui
                            .button(format!("limb: {picked} and {p}"))
                            .hint(
                                "in pose mode, drag this bone's end and both bones turn to reach it, bending as drawn",
                            )
                            .clicked()
                    {
                        chosen = self.ik_spec(|s| {
                            s.add = Some(name.clone());
                            s.chain = Some(2);
                        });
                        ui.close();
                    }
                    if ui
                        .button(format!("aim: {picked}"))
                        .hint("in pose mode, drag this bone's end and it turns to point there")
                        .clicked()
                    {
                        chosen = self.ik_spec(|s| {
                            s.add = Some(name.clone());
                            s.chain = Some(1);
                        });
                        ui.close();
                    }
                })
                .response
                .hint("IK: pose this bone by where its end goes instead of by turning it");
            }
        }
        chosen.map(|spec| -> Later { Box::new(move |e| e.apply(vec![Op::Ik(spec)])) })
    }

    pub(super) fn bone_options(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut run: Option<Later> = None;
        [
            (
                Mode::Draw,
                "draw",
                "drag on empty canvas to draw a chain of bones; drag a bone's end or middle to move it; alt drag from a bone's end for a child",
            ),
            (
                Mode::Weights,
                "weights",
                "paint the picked bone's weight; alt removes it; cmd click a bone to pick it",
            ),
            (
                Mode::Pose,
                "pose",
                "drag a bone's end to turn it, its children follow; drag an IK limb's end and the limb reaches for it",
            ),
        ]
        .iter()
        .for_each(|(mode, label, hint)| {
            if ui
                .selectable_label(self.bones.mode == *mode, *label)
                .hint(*hint)
                .clicked()
            {
                self.bones.mode = *mode;
                self.bones.loaded = None;
                self.bones.posing = None;
            }
        });
        ui.separator();
        if let Some(later) = self.skeleton_list(ui) {
            run = Some(later);
        }
        ui.separator();
        match self.bones.mode {
            Mode::Draw => {
                if let Some(picked) = self.bones.selected.clone() {
                    let label = ui.label("name");
                    let r = ui
                        .add(egui::TextEdit::singleline(&mut self.bones.name).desired_width(110.0))
                        .labelled_by(label.id)
                        .hint("the picked bone's name; Return renames it");
                    if r.lost_focus() && self.bones.name.trim() != picked && !self.bones.name.trim().is_empty() {
                        let to = self.bones.name.trim().to_string();
                        run = Some(Box::new(move |e| {
                            e.bone_edit(|s| {
                                s.rename = Some(picked);
                                s.to_name = Some(to.clone());
                            })?;
                            e.pick_bone(Some(to));
                            Ok(())
                        }));
                    } else if let Some(later) = self.parent_menu(ui, &picked) {
                        run = Some(later);
                    } else if let Some(later) = self.ik_menu(ui, &picked) {
                        run = Some(later);
                    } else if ui.button("remove").hint("the picked bone and its children").clicked() {
                        run = Some(Box::new(move |e| {
                            e.end_chain();
                            e.bone_edit(|s| s.rm = Some(picked))?;
                            e.pick_bone(None);
                            Ok(())
                        }));
                    }
                }
                let (hint, more) = match (&self.bones.chain, &self.bones.selected) {
                    (Some(_), _) => (
                        "click: the next bone; Esc: done",
                        "each click places the next bone of the chain; shift as you let go starts the next one from the same joint; Esc, Enter, a right click or a click on the chain's open end ends it",
                    ),
                    (None, Some(_)) => (
                        "drag its ends or middle; alt drag: a child",
                        "drag an end or the middle to move it; alt drag from its end starts a chain from there; a drag on empty canvas starts a new chain",
                    ),
                    (None, None) => (
                        "drag to draw a chain of bones",
                        "a drag draws a bone and the chain carries on from its end: each click adds the next; click a bone to pick it",
                    ),
                };
                ui.label(RichText::new(hint).color(theme::WEAK)).hint(more);
            }
            Mode::Weights => {
                ui.add(
                    egui::Slider::new(&mut self.bones.radius, 4.0..=200.0)
                        .logarithmic(true)
                        .text("size"),
                )
                .hint("the weight brush's radius in px");
                ui.add(egui::Slider::new(&mut self.bones.strength, 0.0..=1.0).text("weight"))
                    .hint("the weight a dab paints; alt paints 0");
                if ui
                    .button("automatic weights")
                    .hint("start again from the bones, by distance")
                    .clicked()
                {
                    run = Some(Box::new(|e| {
                        e.paint_weights(WeightsSpec {
                            layer: e.rig_owner().context("no skeleton here")?,
                            frame: e.scene.frame,
                            auto: true,
                            bone: None,
                            set: Vec::new(),
                            part: None,
                            value: 1.0,
                            smooth: false,
                            resolved: None,
                        })
                    }));
                }
                if ui.button("smooth").hint("blur every bone's weights a little").clicked() {
                    run = Some(Box::new(|e| {
                        e.paint_weights(WeightsSpec {
                            layer: e.rig_owner().context("no skeleton here")?,
                            frame: e.scene.frame,
                            auto: false,
                            bone: None,
                            set: Vec::new(),
                            part: None,
                            value: 1.0,
                            smooth: true,
                            resolved: None,
                        })
                    }));
                }
                ui.label(
                    RichText::new(
                        self.bones
                            .selected
                            .as_deref()
                            .map_or("pick a bone: cmd click it, or pick it in the list".to_string(), |b| {
                                format!("painting {b}")
                            }),
                    )
                    .color(theme::WEAK),
                );
            }
            Mode::Pose => {
                if ui
                    .button("make this pose a drawing")
                    .hint("bends the drawing here (or its rest drawing) into this pose, as klad pose")
                    .clicked()
                {
                    run = Some(Box::new(|e| e.pose_drawing()));
                }
                if ui
                    .button("back to rest")
                    .hint("forget the turns, the move and the targets")
                    .clicked()
                {
                    self.bones.pose.clear();
                    self.bones.root = [0.0, 0.0];
                    self.bones.reach.clear();
                }
                let (bones, ik) = (self.rig_bones(), self.rig_ik());
                let held = self
                    .bones
                    .selected
                    .as_ref()
                    .and_then(|p| ik.iter().find(|k| limb(&bones, k).contains(p)))
                    .map(|k| k.bone.clone());
                let how = "drag a bone's end to turn it; drag the end of an IK limb and the limb reaches for it; drag a root bone's head to move the whole while IK ends stay; alt drag turns a bone by hand, taking its limb off IK";
                match held {
                    Some(end) => {
                        let mut on = self.bones.reach.contains_key(&end);
                        if ui
                            .checkbox(&mut on, "IK")
                            .hint(format!(
                                "on: {end}'s limb reaches for its target; off: turn it by hand, as it is now. {how}"
                            ))
                            .changed()
                        {
                            match on {
                                true => self.ik_on(&end),
                                false => self.ik_off(&end),
                            }
                        }
                        if let Some(r) = self.bones.reach.get_mut(&end) {
                            ui.spacing_mut().slider_width = 70.0;
                            ui.add(egui::Slider::new(&mut r.mix, 0.0..=1.0).text("mix"))
                                .hint("0: the limb's own turns; 1: it reaches the target");
                        }
                    }
                    None => {
                        ui.label(RichText::new("drag an end; alt: turn by hand").color(theme::WEAK))
                            .hint(how);
                    }
                }
            }
        }
        if let Some(f) = run {
            f(self)?;
        }
        Ok(())
    }

    fn skeleton_list(&mut self, ui: &mut egui::Ui) -> Option<Later> {
        let bones = self.rig_bones();
        let doc = &self.scene.bundle.doc;
        if bones.is_empty() {
            let owners: Vec<(String, String)> = doc
                .all_layers()
                .into_iter()
                .filter(|l| !l.bones.is_empty())
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect();
            if owners.is_empty() {
                ui.label(RichText::new("no skeleton yet").color(theme::WEAK));
                return None;
            }
            let here = self
                .scene
                .active_layer()
                .map_or("this layer".to_string(), |l| l.name.clone());
            ui.label(RichText::new(format!("{here} has no bones; skeletons are on")).color(theme::WEAK));
            return owners.into_iter().fold(None, |run, (id, name)| {
                let clicked = ui
                    .button(format!("{} {name}", theme::icon::TURN))
                    .hint(format!("make {name} the active layer to see and edit its bones"))
                    .clicked();
                run.or_else(|| {
                    clicked.then(|| -> Later {
                        Box::new(move |e| {
                            let Some(klad_core::doc::Loc::Layer(path)) = e.scene.bundle.doc.locate(&id) else {
                                bail!("that layer is gone");
                            };
                            e.pick_bone(None);
                            e.activate(path)
                        })
                    })
                })
            });
        }
        let owner = self
            .rig_owner()
            .and_then(|o| doc.layer(&o).map(|l| l.name.clone()))
            .unwrap_or_default();
        let picked = self
            .bones
            .selected
            .clone()
            .filter(|p| bones.iter().any(|b| &b.name == p));
        let depth = |b: &Bone| {
            std::iter::successors(b.parent.clone(), |p| {
                bones.iter().find(|o| &o.name == p).and_then(|o| o.parent.clone())
            })
            .count()
        };
        let mut chosen = None;
        ui.menu_button(
            format!("{} {}", theme::icon::TURN, picked.as_deref().unwrap_or("pick a bone")),
            |ui| {
                ui.label(
                    RichText::new(format!("the bones of {owner}"))
                        .color(theme::WEAK)
                        .small(),
                );
                bones.iter().for_each(|b| {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0 * depth(b) as f32);
                        if ui
                            .selectable_label(picked.as_ref() == Some(&b.name), &b.name)
                            .hint(match &b.parent {
                                Some(p) => format!("pick {}, a child of {p}", b.name),
                                None => format!("pick {}, a root bone", b.name),
                            })
                            .clicked()
                        {
                            chosen = Some(b.name.clone());
                            ui.close();
                        }
                    });
                });
            },
        )
        .response
        .hint(format!("the skeleton of {owner}, each bone under its parent: pick one"));
        chosen.map(|name| -> Later {
            Box::new(move |e| {
                e.end_chain();
                e.pick_bone(Some(name));
                Ok(())
            })
        })
    }

    fn parent_menu(&mut self, ui: &mut egui::Ui, picked: &str) -> Option<Later> {
        let bones = self.rig_bones();
        let current = bones.iter().find(|b| b.name == picked)?.parent.clone();
        let below = subtree(&bones, picked);
        let mut chosen: Option<Option<String>> = None;
        let label = ui.label("parent");
        egui::ComboBox::from_id_salt("bones.parent")
            .selected_text(current.as_deref().unwrap_or("none"))
            .show_ui(ui, |ui| {
                std::iter::once(None)
                    .chain(
                        bones
                            .iter()
                            .filter(|b| !below.contains(&b.name))
                            .map(|b| Some(b.name.clone())),
                    )
                    .for_each(|p| {
                        if ui
                            .selectable_label(p == current, p.as_deref().unwrap_or("none"))
                            .clicked()
                            && p != current
                        {
                            chosen = Some(p);
                        }
                    });
            })
            .response
            .labelled_by(label.id)
            .hint("the bone this one hangs from; none makes it a root");
        let name = picked.to_string();
        chosen.map(|parent| -> Later {
            Box::new(move |e| {
                e.bone_edit(|s| {
                    s.reparent = Some(name);
                    s.parent = parent;
                })
            })
        })
    }

    fn heat_texture(&mut self, bone: &str) -> Option<egui::TextureHandle> {
        let rev = self.scene.bundle.doc.rev;
        if let Some((b, r, t)) = &self.bones.heat
            && b == bone
            && *r == rev
        {
            return Some(t.clone());
        }
        let owner = self.rig_owner()?;
        let bones = self.rig_bones();
        let i = bones.iter().position(|b| b.name == bone)?;
        let weights: Weights = self.scene.bundle.read_weights(&owner).ok()?;
        let (w, h) = (weights.width as usize, weights.height as usize);
        let pixels: Vec<Color32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = [(x as f32 + 0.5) * CELL as f32, (y as f32 + 0.5) * CELL as f32];
                let v = weights.at(&bones, p)[i];
                Color32::from_rgba_unmultiplied(240, (60.0 + 160.0 * v) as u8, 40, (v * 150.0) as u8)
            })
            .collect();
        let image = egui::ColorImage::new([w, h], pixels);
        let texture = self
            .ctx
            .load_texture("bone weights", image, egui::TextureOptions::LINEAR);
        self.bones.heat = Some((bone.to_string(), rev, texture.clone()));
        Some(texture)
    }

    pub(super) fn decorate_bones(
        &mut self,
        painter: &egui::Painter,
        place: impl Fn(Vec2) -> Pos2,
        hover: Option<Vec2>,
    ) {
        if self.tool != super::Tool::Bone {
            return;
        }
        if self.bones.mode == Mode::Weights
            && let Some(bone) = self.bones.selected.clone()
            && let Some(texture) = self.heat_texture(&bone)
        {
            let canvas = self.canvas_size();
            let rect = egui::Rect::from_two_pos(place(Vec2::ZERO), place(canvas));
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let bones = self.drawn_bones();
        let ik = self.rig_ik();
        let pose = self.shown_pose();
        let shown = tips(&bones, &ik, &pose);
        let editing = self.bones.mode == Mode::Draw;
        let held: Vec<String> = ik
            .iter()
            .filter(|k| pose.ik.get(&k.bone).is_some_and(|r| r.mix > 0.0))
            .flat_map(|k| limb(&bones, k))
            .collect();
        bones.iter().zip(&shown).for_each(|(b, (h, t))| {
            let (a, z) = (place(at(*h)), place(at(*t)));
            let picked = self.bones.selected.as_ref() == Some(&b.name);
            let colour = if picked {
                theme::ACCENT
            } else {
                Color32::from_rgb(120, 200, 240)
            };
            painter.line_segment([a, z], egui::Stroke::new(6.0, Color32::from_black_alpha(150)));
            painter.line_segment([a, z], egui::Stroke::new(3.0, colour));
            if held.contains(&b.name) {
                painter.line_segment([a, z], egui::Stroke::new(1.2, Color32::from_black_alpha(200)));
            }
            painter.circle_filled(a, 4.5, colour);
            painter.circle_stroke(z, 4.0, egui::Stroke::new(1.5, colour));
            if ik.iter().any(|k| k.bone == b.name) {
                painter.rect_stroke(
                    egui::Rect::from_center_size(z, egui::vec2(11.0, 11.0)),
                    1.0,
                    egui::Stroke::new(1.5, TARGET),
                    egui::StrokeKind::Middle,
                );
            }
            if picked && editing {
                [a, z].into_iter().for_each(|p| {
                    painter.circle_filled(p, 6.5, Color32::WHITE);
                    painter.circle_stroke(p, 6.5, egui::Stroke::new(2.0, theme::ACCENT));
                });
            }
            painter.text(
                a + (z - a) * 0.5 + egui::vec2(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                &b.name,
                egui::FontId::proportional(11.0),
                colour,
            );
        });
        if let Some(Drag {
            from,
            to,
            what: Dragging::New(_),
        }) = &self.bones.drag
        {
            painter.line_segment([place(*from), place(*to)], egui::Stroke::new(2.0, theme::ACCENT));
        }
        if editing && let Some(chain) = &self.bones.chain {
            let (a, z) = (place(at(chain.head)), place(chain.to));
            painter.extend(egui::Shape::dashed_line(
                &[a, z],
                egui::Stroke::new(2.0, theme::ACCENT),
                6.0,
                4.0,
            ));
            painter.circle_stroke(a, 6.5, egui::Stroke::new(1.5, theme::ACCENT));
        }
        if self.bones.mode == Mode::Pose {
            ik.iter().for_each(|k| {
                let (Some(r), Some(i)) = (pose.ik.get(&k.bone), bones.iter().position(|b| b.name == k.bone)) else {
                    return;
                };
                let (end, target) = (place(at(shown[i].1)), place(at(r.target)));
                if (end - target).length() > 2.0 {
                    painter.extend(egui::Shape::dashed_line(
                        &[end, target],
                        egui::Stroke::new(1.5, TARGET),
                        4.0,
                        3.0,
                    ));
                }
                let stroke = egui::Stroke::new(2.0, TARGET);
                painter.circle_stroke(target, 7.0, egui::Stroke::new(4.0, Color32::from_black_alpha(150)));
                painter.circle_stroke(target, 7.0, stroke);
                painter.line_segment([target - egui::vec2(11.0, 0.0), target + egui::vec2(11.0, 0.0)], stroke);
                painter.line_segment([target - egui::vec2(0.0, 11.0), target + egui::vec2(0.0, 11.0)], stroke);
            });
        }
        if editing
            && self.bones.drag.is_none()
            && let Some(p) = hover
        {
            let cursor = match (self.modifiers.alt, self.grip_at(p)) {
                (true, _) => None,
                (false, Some((_, _, Grip::Whole))) => Some(egui::CursorIcon::Move),
                (false, Some(_)) => Some(egui::CursorIcon::Grab),
                (false, None) => None,
            };
            if let Some(c) = cursor {
                self.ctx.set_cursor_icon(c);
            }
        }
        if self.bones.mode == Mode::Weights
            && let Some(p) = hover
        {
            let r = self.bones.radius * self.view.map_or(1.0, |v| v.zoom);
            painter.circle_stroke(place(p), r, egui::Stroke::new(1.0, theme::ACCENT));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pose_from_the_editor_is_the_cli_pose() {
        let mut pose = Pose {
            angles: BTreeMap::from([("fore".to_string(), -45.0), ("upper".to_string(), 0.0)]),
            ..Pose::default()
        };
        assert_eq!(
            pose_args(std::path::Path::new("/w/a.klad"), "l1", 4, &pose).join(" "),
            "pose /w/a.klad l1 --from 4 --to 4 --bone fore=-45.00"
        );
        pose.root = [-3.0, 6.5];
        pose.ik.insert(
            "shin".into(),
            Reach {
                target: [410.0, 742.25],
                mix: 1.0,
            },
        );
        pose.ik.insert(
            "hand".into(),
            Reach {
                target: [-5.0, 20.0],
                mix: 0.25,
            },
        );
        assert_eq!(
            pose_args(std::path::Path::new("/w/a.klad"), "l1", 4, &pose).join(" "),
            "pose /w/a.klad l1 --from 4 --to 4 --bone fore=-45.00 --root -3.00,6.50 --target hand=-5.00,20.00 --mix hand=0.250 --target shin=410.00,742.25"
        );
    }
}
