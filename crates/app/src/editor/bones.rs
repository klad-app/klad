use super::Editor;
use crate::jobs::{Spec, Tag};
use crate::theme::{self, Hint};
use anyhow::{Context, Result, bail};
use egui::{Color32, Pos2, RichText, Vec2};
use klad_core::doc::Kind;
use klad_core::ops::{BoneSpec, Op, WeightsSpec};
use klad_core::rig::{Bone, CELL, Pose, Weights, tips, weights_for};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Draw,
    Weights,
    Pose,
}

pub(super) struct BoneTool {
    pub mode: Mode,
    pub selected: Option<String>,
    pub drag: Option<(Vec2, Vec2, Option<String>)>,
    pub dabs: Vec<[f32; 4]>,
    pub radius: f32,
    pub strength: f32,
    pub pose: BTreeMap<String, f32>,
    pub turning: Option<String>,
    pub name: String,
    pub heat: Option<(String, u64, egui::TextureHandle)>,
}

impl Default for BoneTool {
    fn default() -> Self {
        Self {
            mode: Mode::Draw,
            selected: None,
            drag: None,
            dabs: Vec::new(),
            radius: 24.0,
            strength: 1.0,
            pose: BTreeMap::new(),
            turning: None,
            name: String::new(),
            heat: None,
        }
    }
}

const NEAR: f32 = 10.0;

type Later = Box<dyn FnOnce(&mut Editor) -> Result<()>>;

fn near_segment(p: Vec2, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (a, b) = (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + d * t - p).length()
}

pub(super) fn pose_args(
    bundle: &std::path::Path,
    owner: &str,
    frame: u32,
    pose: &BTreeMap<String, f32>,
) -> Vec<String> {
    let mut args = vec![
        "pose".to_string(),
        bundle.to_string_lossy().to_string(),
        owner.to_string(),
        "--from".into(),
        frame.to_string(),
        "--to".into(),
        frame.to_string(),
    ];
    pose.iter()
        .filter(|(_, v)| v.abs() > 1e-3)
        .for_each(|(k, v)| args.extend(["--bone".to_string(), format!("{k}={v:.2}")]));
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

    fn shown_pose(&self) -> Pose {
        Pose {
            angles: self.bones.pose.clone(),
            ..Pose::default()
        }
    }

    fn bone_at(&self, c: Vec2) -> Option<String> {
        let bones = self.rig_bones();
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let shown = tips(&bones, &self.shown_pose());
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
            Mode::Draw => {
                let bones = self.rig_bones();
                let zoom = self.view.map_or(1.0, |v| v.zoom);
                let parent = bones
                    .iter()
                    .filter(|b| (Vec2::new(b.tail[0], b.tail[1]) - c).length() <= NEAR / zoom)
                    .map(|b| (b.name.clone(), Vec2::new(b.tail[0], b.tail[1])))
                    .next();
                let start = parent.as_ref().map_or(c, |p| p.1);
                self.bones.drag = Some((start, c, parent.map(|p| p.0)));
            }
            Mode::Weights => {
                self.bones.dabs.clear();
                self.bone_dab(c);
            }
            Mode::Pose => {
                self.bones.turning = self.bone_at(c);
                if let Some(b) = &self.bones.turning {
                    self.bones.selected = Some(b.clone());
                }
            }
        }
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
                if let Some(drag) = self.bones.drag.as_mut() {
                    drag.1 = c;
                }
            }
            Mode::Weights => {
                if !self.bones.dabs.is_empty() {
                    self.bone_dab(c);
                }
            }
            Mode::Pose => {
                let Some(name) = self.bones.turning.clone() else {
                    return;
                };
                let bones = self.rig_bones();
                let Some(i) = bones.iter().position(|b| b.name == name) else {
                    return;
                };
                let shown = tips(&bones, &self.shown_pose());
                let (head, tail) = shown[i];
                let now = (tail[1] - head[1]).atan2(tail[0] - head[0]);
                let want = (c.y - head[1]).atan2(c.x - head[0]);
                let mut turn = (want - now).to_degrees();
                turn = (turn + 540.0).rem_euclid(360.0) - 180.0;
                *self.bones.pose.entry(name).or_insert(0.0) += turn;
            }
        }
    }

    pub(super) fn bone_up(&mut self) -> Result<()> {
        match self.bones.mode {
            Mode::Draw => {
                let Some((a, b, parent)) = self.bones.drag.take() else {
                    return Ok(());
                };
                if (b - a).length() < 6.0 {
                    self.bones.selected = self.bone_at(b);
                    self.bones.name = self.bones.selected.clone().unwrap_or_default();
                    return Ok(());
                }
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
                    rm: None,
                    rename: None,
                    to_name: None,
                    moved: None,
                    reparent: None,
                    parent,
                    head: Some([a.x, a.y]),
                    tail: Some([b.x, b.y]),
                })])?;
                self.bones.name = name.clone();
                self.bones.selected = Some(name);
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
                    .context("pick a bone to paint its weights: click it in draw mode")?;
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
            Mode::Pose => self.bones.turning = None,
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
            add: None,
            rm: None,
            rename: None,
            to_name: None,
            moved: None,
            reparent: None,
            parent: None,
            head: None,
            tail: None,
        };
        edit(&mut spec);
        self.apply(vec![Op::Bone(spec)])
    }

    fn pose_drawing(&mut self) -> Result<()> {
        let owner = self.rig_owner().context("no skeleton here")?;
        if self.bones.pose.values().all(|v| v.abs() < 1e-3) {
            bail!("turn a bone first: drag a bone's end in pose mode");
        }
        let frame = self.scene.frame;
        self.start_job(Spec {
            tag: Tag::Edit,
            name: format!("pose at frame {frame}"),
            exe: PathBuf::new(),
            args: pose_args(&self.scene.bundle.root, &owner, frame, &self.bones.pose),
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
        self.bones.pose.clear();
        Ok(())
    }

    pub(super) fn bone_options(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut run: Option<Later> = None;
        [
            (
                Mode::Draw,
                "draw",
                "drag to draw a bone; start on a bone's end to make it its child; click a bone to pick it",
            ),
            (
                Mode::Weights,
                "weights",
                "paint the picked bone's weight; alt removes it",
            ),
            (Mode::Pose, "pose", "drag a bone's end to turn it, its children follow"),
        ]
        .iter()
        .for_each(|(mode, label, hint)| {
            if ui
                .selectable_label(self.bones.mode == *mode, *label)
                .hint(*hint)
                .clicked()
            {
                self.bones.mode = *mode;
                if *mode == Mode::Pose {
                    self.bones.pose = self
                        .rig_owner()
                        .and_then(|o| self.scene.bundle.doc.layer(&o).cloned())
                        .and_then(|o| {
                            klad_core::rig::painted(&o)
                                .iter()
                                .find_map(|l| l.content_at(self.scene.frame).and_then(|c| c.pose.clone()))
                        })
                        .map(|p| p.angles)
                        .unwrap_or_default();
                }
            }
        });
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
                            e.bones.selected = Some(to);
                            Ok(())
                        }));
                    } else if ui.button("remove").hint("the picked bone and its children").clicked() {
                        run = Some(Box::new(move |e| {
                            e.bone_edit(|s| s.rm = Some(picked))?;
                            e.bones.selected = None;
                            Ok(())
                        }));
                    }
                } else {
                    ui.label(RichText::new("drag on the canvas to draw a bone").color(theme::WEAK));
                }
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
                            .map_or("click a bone in draw mode first".to_string(), |b| {
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
                if ui.button("back to rest").hint("forget the turns").clicked() {
                    self.bones.pose.clear();
                }
            }
        }
        if let Some(f) = run {
            f(self)?;
        }
        Ok(())
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
        let bones = self.rig_bones();
        let shown = tips(&bones, &self.shown_pose());
        bones.iter().zip(shown).for_each(|(b, (h, t))| {
            let (a, z) = (place(Vec2::new(h[0], h[1])), place(Vec2::new(t[0], t[1])));
            let picked = self.bones.selected.as_ref() == Some(&b.name);
            let colour = if picked {
                theme::ACCENT
            } else {
                Color32::from_rgb(120, 200, 240)
            };
            painter.line_segment([a, z], egui::Stroke::new(6.0, Color32::from_black_alpha(150)));
            painter.line_segment([a, z], egui::Stroke::new(3.0, colour));
            painter.circle_filled(a, 4.5, colour);
            painter.circle_stroke(z, 4.0, egui::Stroke::new(1.5, colour));
            painter.text(
                a + (z - a) * 0.5 + egui::vec2(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                &b.name,
                egui::FontId::proportional(11.0),
                colour,
            );
        });
        if let Some((a, b, _)) = self.bones.drag {
            painter.line_segment([place(a), place(b)], egui::Stroke::new(2.0, theme::ACCENT));
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
        let pose = BTreeMap::from([("fore".to_string(), -45.0), ("upper".to_string(), 0.0)]);
        assert_eq!(
            pose_args(std::path::Path::new("/w/a.draw"), "l1", 4, &pose).join(" "),
            "pose /w/a.draw l1 --from 4 --to 4 --bone fore=-45.00"
        );
    }
}
