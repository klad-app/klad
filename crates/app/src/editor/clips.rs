use super::Editor;
use crate::commands::Cmd;
use crate::jobs::{Spec, Tag};
use crate::theme::Hint;
use anyhow::Result;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, pos2};
use klad_core::doc::{Clip, Timeline as Clock};
use klad_core::ops::{ClipEdit, Op};
use std::path::{Path, PathBuf};

pub(super) const LANE: f32 = 16.0;
const END: f32 = 6.0;

pub(super) fn lanes(clips: &[Clip]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..clips.len()).collect();
    order.sort_by_key(|&i| (clips[i].from, clips[i].to));
    let mut ends: Vec<u32> = Vec::new();
    let mut lane = vec![0; clips.len()];
    order.into_iter().for_each(|i| {
        let c = &clips[i];
        let at = ends.iter().position(|&e| e < c.from).unwrap_or(ends.len());
        match ends.get_mut(at) {
            Some(e) => *e = c.to,
            None => ends.push(c.to),
        }
        lane[i] = at;
    });
    lane
}

pub(super) fn band_height(clips: &[Clip]) -> f32 {
    (lanes(clips).into_iter().max().map_or(1, |m| m + 2)) as f32 * LANE
}

pub(super) fn fresh_name(clips: &[Clip]) -> String {
    (1..)
        .map(|n| format!("clip{n}"))
        .find(|n| clips.iter().all(|c| &c.name != n))
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Span {
    pub from: u32,
    pub to: u32,
    pub fps: u32,
    pub once: bool,
    pub what: String,
}

pub(super) fn play_span(t: &Clock, frame: u32, range: Option<(u32, u32)>, chosen: Option<&str>) -> Span {
    if let Some((a, b)) = range.filter(|(a, b)| a <= b && *b < t.frames) {
        return Span {
            from: a,
            to: b,
            fps: t.fps,
            once: false,
            what: format!("the loop range {a} to {b}"),
        };
    }
    let picked = chosen
        .and_then(|n| t.clip(n))
        .filter(|c| c.from <= frame && frame <= c.to);
    match picked.or_else(|| t.clip_at(frame)) {
        Some(c) => Span {
            from: c.from,
            to: c.to.min(t.frames - 1),
            fps: t.fps_of(c),
            once: !c.looping,
            what: format!("clip {}", c.name),
        },
        None => Span {
            from: 0,
            to: t.frames - 1,
            fps: t.fps,
            once: false,
            what: "the timeline".into(),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    Body,
    First,
    Last,
}

pub(super) struct Strip {
    pub rect: Rect,
    pub cell: f32,
    pub lanes: Vec<usize>,
}

impl Strip {
    fn x_of(&self, f: u32) -> f32 {
        self.rect.left() + f as f32 * self.cell
    }

    fn bar(&self, from: u32, to: u32, lane: usize) -> Rect {
        let top = self.rect.top() + lane as f32 * LANE;
        Rect::from_min_max(
            pos2(self.x_of(from) + 1.0, top + 2.0),
            pos2(self.x_of(to + 1) - 1.0, top + LANE - 2.0),
        )
    }

    pub fn hit(&self, clips: &[Clip], p: Pos2) -> Option<(usize, Hit)> {
        let lane = ((p.y - self.rect.top()) / LANE).floor().max(0.0) as usize;
        clips
            .iter()
            .zip(&self.lanes)
            .enumerate()
            .filter(|(_, (_, l))| **l == lane)
            .find_map(|(i, (c, _))| {
                let (a, b) = (self.x_of(c.from), self.x_of(c.to + 1));
                match p.x {
                    x if (x - a).abs() <= END && x < (a + b) / 2.0 => Some((i, Hit::First)),
                    x if (x - b).abs() <= END => Some((i, Hit::Last)),
                    x if a < x && x < b => Some((i, Hit::Body)),
                    _ => None,
                }
            })
    }

    pub fn paint(
        &self,
        painter: &Painter,
        clips: &[Clip],
        fps: u32,
        chosen: Option<&str>,
        spans: &[(usize, u32, u32)],
    ) {
        painter.rect_filled(self.rect, 0.0, Color32::from_gray(20));
        clips.iter().zip(&self.lanes).enumerate().for_each(|(i, (c, &lane))| {
            let (from, to) = spans
                .iter()
                .find(|(k, _, _)| *k == i)
                .map_or((c.from, c.to), |&(_, a, b)| (a, b));
            let bar = self.bar(from, to, lane);
            let fill = match c.looping {
                true => Color32::from_rgb(52, 128, 118),
                false => Color32::from_rgb(150, 112, 52),
            };
            painter.rect_filled(bar, 3.0, fill);
            if chosen == Some(c.name.as_str()) {
                painter.rect_stroke(bar, 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
            }
            [bar.left() + 2.0, bar.right() - 2.0].iter().for_each(|&x| {
                painter.line_segment(
                    [pos2(x, bar.top() + 2.0), pos2(x, bar.bottom() - 2.0)],
                    Stroke::new(1.0, Color32::from_gray(220)),
                );
            });
            let label = format!(
                "{}{}{}",
                c.name,
                c.fps
                    .filter(|f| *f != fps)
                    .map_or(String::new(), |f| format!(", {f} fps")),
                if c.looping { ", loops" } else { ", once" }
            );
            painter.with_clip_rect(bar.shrink(3.0)).text(
                pos2(bar.left() + 6.0, bar.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(10.5),
                Color32::from_gray(240),
            );
        });
    }

    pub fn outline(&self, painter: &Painter, from: u32, to: u32) {
        let (a, b) = (from.min(to), from.max(to));
        painter.rect_stroke(
            Rect::from_min_max(
                pos2(self.x_of(a), self.rect.top() + 1.0),
                pos2(self.x_of(b + 1), self.rect.bottom() - 1.0),
            ),
            3.0,
            Stroke::new(1.5, Color32::YELLOW),
            egui::StrokeKind::Inside,
        );
    }
}

fn summary(stderr: &str) -> String {
    let keep: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("animations:") || l.starts_with("kept ") || l.contains(" in all"))
        .collect();
    keep.join("; ")
}

fn failure(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}

pub(super) fn resolve(home: &Path, folder: &str) -> PathBuf {
    let path = PathBuf::from(folder.trim());
    match path.is_absolute() {
        true => path,
        false => home.join(path),
    }
}

impl Editor {
    fn bundle_home(&self) -> PathBuf {
        let root = self.scene.bundle.root.components().collect::<PathBuf>();
        match root.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }

    pub(super) fn add_clip(&mut self, from: u32, to: u32) -> Result<()> {
        let (from, to) = (from.min(to), from.max(to));
        let name = fresh_name(&self.scene.bundle.doc.timeline.clips);
        let clip = Clip::new(&name, from, to, true);
        if self.timeline_apply(vec![Op::Clip(ClipEdit::Add(clip))])?.is_some() {
            self.timeline.clip = Some(name);
            self.timeline.clip_name = (String::new(), String::new());
        }
        Ok(())
    }

    pub(super) fn reach_clip(&mut self, name: &str, from: u32, to: u32) -> Result<()> {
        let Some(clip) = self.scene.bundle.doc.timeline.clip(name).cloned() else {
            return Ok(());
        };
        if (clip.from, clip.to) == (from, to) {
            return Ok(());
        }
        let op = Op::Clip(ClipEdit::Set {
            name: name.to_string(),
            rename: None,
            from: Some(from),
            to: Some(to),
            looping: None,
            fps: None,
        });
        self.timeline_apply(vec![op])?;
        Ok(())
    }

    pub(super) fn current_clip(&self) -> Option<Clip> {
        let t = &self.scene.bundle.doc.timeline;
        let frame = self.scene.frame;
        let chosen = self.timeline.clip.as_deref().and_then(|n| t.clip(n));
        chosen.or_else(|| t.clip_at(frame)).cloned()
    }

    pub(super) fn clip_row(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let t = self.scene.bundle.doc.timeline.clone();
        if self.timeline.clip.as_deref().is_some_and(|n| t.clip(n).is_none()) {
            self.timeline.clip = None;
        }
        let stored = self.scene.bundle.doc.export.godot.clone().unwrap_or_default();
        if self.timeline.godot.0 != stored {
            self.timeline.godot = (stored.clone(), stored.clone());
        }
        let current = self.current_clip();
        let mut ops: Vec<Op> = Vec::new();
        ui.horizontal_wrapped(|ui| {
            match &current {
                None if t.clips.is_empty() => {
                    ui.small("clips: drag along the strip above the frame numbers to make one (exports turn each clip into an animation)");
                }
                None => {
                    ui.small(format!(
                        "no clip at frame {}: click a clip's bar to pick it, or drag along the empty row of the strip for a new one",
                        self.scene.frame
                    ));
                }
                Some(clip) => {
                    let set = |rename, looping, fps| {
                        Op::Clip(ClipEdit::Set {
                            name: clip.name.clone(),
                            rename,
                            from: None,
                            to: None,
                            looping,
                            fps,
                        })
                    };
                    if self.timeline.clip_name.0 != clip.name {
                        self.timeline.clip_name = (clip.name.clone(), clip.name.clone());
                    }
                    let label = ui.label("clip");
                    let name = ui
                        .add(egui::TextEdit::singleline(&mut self.timeline.clip_name.1).desired_width(90.0))
                        .labelled_by(label.id)
                        .hint("the clip's name, which exports use for the animation; Return keeps it");
                    let typed = self.timeline.clip_name.1.trim().to_string();
                    if name.lost_focus() && typed != clip.name && !typed.is_empty() {
                        ops.push(set(Some(typed.clone()), None, None));
                        self.timeline.clip = Some(typed);
                    }
                    ui.label(format!("frames {} to {} ({} frames)", clip.from, clip.to, clip.frames()));
                    self.command_check(ui, Cmd::ClipLoops, "loops");
                    self.command_check(ui, Cmd::ClipOwnFps, "own fps");
                    if let Some(fps) = clip.fps {
                        if !ui.memory(|m| m.focused().is_some()) || self.timeline.clip_fps == 0 {
                            self.timeline.clip_fps = fps;
                        }
                        let r = ui
                            .add(egui::DragValue::new(&mut self.timeline.clip_fps).range(1..=240))
                            .hint("the clip's own frames per second");
                        if (r.drag_stopped() || r.lost_focus()) && self.timeline.clip_fps != fps {
                            ops.push(set(None, None, Some(Some(self.timeline.clip_fps))));
                        }
                    } else {
                        ui.label(format!("({} fps, the document's)", t.fps));
                    }
                    self.command_button(ui, Cmd::DeleteClip, Some("delete clip"));
                }
            }
            ui.separator();
            let label = ui.label("Godot folder");
            ui.add(
                egui::TextEdit::singleline(&mut self.timeline.godot.1)
                    .desired_width(180.0)
                    .hint_text("next to the bundle, e.g. ../game/art"),
            )
            .labelled_by(label.id)
            .hint("where export to Godot writes, relative to the folder holding the bundle");
            self.command_button(ui, Cmd::ExportGodot, Some("export to Godot"));
        });
        self.timeline_apply(ops).map(|_| ())
    }

    pub(super) fn godot_folder(&self) -> String {
        self.timeline.godot.1.trim().to_string()
    }

    pub(super) fn export_godot(&mut self) {
        let typed = self.godot_folder();
        let mut args: Vec<String> = vec![
            "export".into(),
            self.scene.bundle.root.to_string_lossy().to_string(),
            "--godot".into(),
        ];
        if typed != self.timeline.godot.0 {
            args.push(resolve(&self.bundle_home(), &typed).to_string_lossy().to_string());
            args.push("--store".into());
        }
        self.start_job(Spec {
            tag: Tag::Export,
            name: format!("export to Godot ({typed})"),
            exe: PathBuf::new(),
            args,
            env: Vec::new(),
            reruns: 3,
            summary: |_, err| summary(err),
            failure: |_, err| failure(err),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_clips_take_separate_lanes() {
        let clips = vec![
            Clip::new("a", 0, 5, true),
            Clip::new("b", 3, 9, false),
            Clip::new("c", 6, 8, true),
            Clip::new("d", 10, 12, true),
        ];
        assert_eq!(lanes(&clips), [0, 1, 0, 0]);
        assert_eq!(
            band_height(&clips),
            3.0 * LANE,
            "two lanes of bars and a free one to drag new clips on"
        );
        assert_eq!(band_height(&[]), LANE);
        assert_eq!(fresh_name(&clips), "clip1");
        assert_eq!(fresh_name(&[Clip::new("clip1", 0, 1, true)]), "clip2");
    }

    #[test]
    fn return_plays_the_loop_range_then_the_clip_under_the_playhead_then_everything() {
        let mut t = Clock {
            frames: 24,
            ..Clock::default()
        };
        t.clips = vec![
            Clip::new("idle", 0, 5, true),
            Clip {
                fps: Some(24),
                ..Clip::new("jump", 4, 11, false)
            },
        ];
        let span = |frame, range, chosen| {
            let s = play_span(&t, frame, range, chosen);
            (s.from, s.to, s.fps, s.once)
        };
        assert_eq!(span(2, None, None), (0, 5, 12, false));
        assert_eq!(span(8, None, None), (4, 11, 24, true));
        assert_eq!(
            span(4, None, None),
            (0, 5, 12, false),
            "the first clip holding the frame"
        );
        assert_eq!(
            span(4, None, Some("jump")),
            (4, 11, 24, true),
            "unless another one is picked"
        );
        assert_eq!(
            span(2, None, Some("jump")),
            (0, 5, 12, false),
            "a picked clip away from the playhead"
        );
        assert_eq!(span(15, None, None), (0, 23, 12, false));
        assert_eq!(span(8, Some((6, 9)), None), (6, 9, 12, false));
        assert_eq!(play_span(&t, 8, None, None).what, "clip jump");
    }

    #[test]
    fn the_strip_finds_bars_and_their_ends() {
        let clips = vec![Clip::new("a", 2, 5, true), Clip::new("b", 4, 9, false)];
        let strip = Strip {
            rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 2.0 * LANE)),
            cell: 20.0,
            lanes: lanes(&clips),
        };
        assert_eq!(strip.hit(&clips, pos2(41.0, 8.0)), Some((0, Hit::First)));
        assert_eq!(strip.hit(&clips, pos2(70.0, 8.0)), Some((0, Hit::Body)));
        assert_eq!(strip.hit(&clips, pos2(118.0, 8.0)), Some((0, Hit::Last)));
        assert_eq!(strip.hit(&clips, pos2(150.0, 8.0)), None);
        assert_eq!(strip.hit(&clips, pos2(150.0, 24.0)), Some((1, Hit::Body)));
        assert_eq!(strip.hit(&clips, pos2(199.0, 24.0)), Some((1, Hit::Last)));
        assert_eq!(
            resolve(Path::new("/work/art"), "../game/sprites"),
            PathBuf::from("/work/art/../game/sprites")
        );
        assert_eq!(resolve(Path::new("/work/art"), "/abs/x"), PathBuf::from("/abs/x"));
    }
}
