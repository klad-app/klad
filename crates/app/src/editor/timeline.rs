use super::clips::{self, Hit, Strip};
use super::layers::{self, LayerRow};
use super::tweens::{self, Span};
use super::{Editor, ms};
use crate::commands::Cmd;
use crate::composite::CanvasDone;
use crate::frames::Player;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use anyhow::Result;
use egui::{Align2, Color32, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use klad_core::doc::{Cel, Kind, Layer, Loc, MASK};
use klad_core::ops::{Op, Place};
use klad_core::timing::{CelAction, CelSpec, FrameSpec, OPEN, ends};
use klad_render::onion::By;
use serde_json::{Map, json};
use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::{Duration, Instant};

const ROW: f32 = crate::theme::ROW;
const RULER: f32 = 18.0;
const NAMES: f32 = 236.0;
const EDGE: f32 = 8.0;
const PAST: f32 = 14.0;
const GRIP: f32 = 7.0;
const SETTLE: Duration = Duration::from_millis(150);
const FLIP_PX: f32 = 22.0;
const AHEAD: u32 = 4;
const PEN_WAIT: Duration = Duration::from_millis(60);
pub(super) const OPEN_WAIT: Duration = Duration::from_millis(400);
pub(super) const FILL: usize = 64;

pub(super) struct CanvasWait {
    asked: u64,
    frame: u32,
    active: Vec<usize>,
    rev: u64,
    rx: Receiver<Result<CanvasDone>>,
    since: Instant,
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Scrub,
    Range {
        from: u32,
        to: u32,
    },
    Hold {
        layer: String,
        cel: String,
        start: u32,
        end: u32,
        open: bool,
        to: u32,
    },
    Move {
        cel: String,
        from: u32,
        to: u32,
    },
    NewClip {
        from: u32,
        to: u32,
    },
    Select {
        from: u32,
        to: u32,
        first: usize,
        last: usize,
    },
    ClipEnd {
        index: usize,
        name: String,
        first: bool,
        from: u32,
        to: u32,
    },
}

pub(super) struct Flip {
    origin: u32,
    x: f32,
}

pub(super) struct Timeline {
    cell: f32,
    drag: Option<Drag>,
    pub folded: BTreeSet<String>,
    pub masks: BTreeSet<String>,
    fps: u32,
    length: u32,
    pub range: Option<(u32, u32)>,
    pub on: u32,
    pub player: Option<Player>,
    pub playing: String,
    moved_at: Instant,
    step_start: Option<Instant>,
    pub canvas_frame: Option<u32>,
    pub on_screen: Option<u32>,
    shown_key: Option<u64>,
    flip: Option<Flip>,
    pub roll: Option<u32>,
    direction: i32,
    cache_rev: u64,
    pub clip: Option<String>,
    pub clip_name: (String, String),
    pub clip_fps: u32,
    pub godot: (String, String),
    pub span: Option<Span>,
    pub ask: tweens::Ask,
    pub cells: Option<Vec<super::cells::CellCopy>>,
    pub geom: Option<Geom>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            cell: 18.0,
            drag: None,
            folded: BTreeSet::new(),
            masks: BTreeSet::new(),
            fps: 0,
            length: 0,
            range: None,
            on: 2,
            player: None,
            playing: String::new(),
            moved_at: Instant::now(),
            step_start: None,
            canvas_frame: None,
            on_screen: None,
            shown_key: None,
            flip: None,
            roll: None,
            direction: 0,
            cache_rev: u64::MAX,
            clip: None,
            clip_name: (String::new(), String::new()),
            clip_fps: 0,
            godot: (String::new(), String::new()),
            span: None,
            ask: tweens::Ask::default(),
            cells: None,
            geom: None,
        }
    }
}

fn span_of(
    rows: &[LayerRow],
    (from, to): (u32, u32),
    (first, last): (usize, usize),
    anchor: (u32, usize),
) -> Option<Span> {
    let (a, b) = (first.min(last), first.max(last));
    let ids: Vec<String> = rows[a..=b.min(rows.len().saturating_sub(1))]
        .iter()
        .filter(|r| matches!(r.kind, Kind::Paint | Kind::Vector))
        .map(|r| r.id.clone())
        .collect();
    (!ids.is_empty()).then(|| Span {
        from: from.min(to),
        to: from.max(to),
        rows: ids,
        anchor,
    })
}

#[derive(Clone, Debug, PartialEq)]
struct Block {
    cel: String,
    link: Option<String>,
    start: u32,
    end: u32,
    open: bool,
    tween: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geom {
    left: f32,
    top: f32,
    cell: f32,
    frames: u32,
    rows: usize,
}

impl Geom {
    fn x_of(&self, f: u32) -> f32 {
        self.left + f as f32 * self.cell
    }

    pub(super) fn cell_centre(&self, f: u32, row: usize) -> Pos2 {
        pos2(
            self.x_of(f) + self.cell / 2.0,
            self.top + RULER + ROW * (row as f32 + 0.5),
        )
    }

    fn frame_at(&self, x: f32) -> u32 {
        (((x - self.left) / self.cell).floor().max(0.0) as u32).min(self.frames.saturating_sub(1))
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        (y >= self.top + RULER)
            .then(|| ((y - self.top - RULER) / ROW).floor() as usize)
            .filter(|r| *r < self.rows)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Press {
    Ruler(u32),
    End(usize, Block),
    Start(usize, Block),
    Cell(usize, u32),
}

fn press_at(g: &Geom, blocks: &[Vec<Block>], p: Pos2) -> Option<Press> {
    if p.y < g.top {
        return None;
    }
    let f = g.frame_at(p.x);
    let Some(r) = g.row_at(p.y) else {
        return Some(Press::Ruler(f));
    };
    let list = blocks.get(r)?;
    if let Some(b) = list
        .iter()
        .find(|b| (p.x - g.x_of(b.end)).abs() <= EDGE && p.x > g.x_of(b.start) + g.cell / 2.0)
    {
        return Some(Press::End(r, b.clone()));
    }
    match list.iter().find(|b| b.start <= f && f < b.end) {
        Some(b) if b.start == f && g.cell_centre(f, r).distance(p) <= GRIP => Some(Press::Start(r, b.clone())),
        _ => Some(Press::Cell(r, f)),
    }
}

fn drag_of(press: &Press, shift: bool, layer: impl Fn(usize) -> String) -> Drag {
    match press {
        Press::Ruler(f) if shift => Drag::Range { from: *f, to: *f },
        Press::Ruler(_) => Drag::Scrub,
        Press::End(r, b) => Drag::Hold {
            layer: layer(*r),
            cel: b.cel.clone(),
            start: b.start,
            end: b.end,
            open: b.open,
            to: b.end,
        },
        Press::Start(_, b) => Drag::Move {
            cel: b.cel.clone(),
            from: b.start,
            to: b.start,
        },
        Press::Cell(r, f) => Drag::Select {
            from: *f,
            to: *f,
            first: *r,
            last: *r,
        },
    }
}

fn blocks(layer: &Layer, frames: u32) -> Vec<Block> {
    let cels = layer.cels();
    cels.iter()
        .zip(ends(cels))
        .filter(|(c, _)| c.frame < frames)
        .map(|(c, end)| Block {
            cel: c.id.clone(),
            link: c.link.clone(),
            start: c.frame,
            end: end.min(frames),
            open: end == OPEN,
            tween: c.tween.is_some(),
        })
        .collect()
}

fn exposed(layer: &Layer, frame: u32) -> Option<(Cel, u32)> {
    let cels = layer.cels();
    cels.iter()
        .zip(ends(cels))
        .find(|(c, e)| c.frame <= frame && frame < *e)
        .map(|(c, e)| (c.clone(), e))
}

impl Editor {
    pub(super) fn busy(&self) -> bool {
        self.live.is_some()
            || self.drag.is_some()
            || self.point_drag.is_some()
            || self.path_commit.is_some()
            || self.lift.as_ref().is_some_and(|l| l.commit.is_some())
    }

    pub(crate) fn canvas_current(&self) -> bool {
        self.timeline.canvas_frame == Some(self.scene.frame) && !self.scene.stale
    }

    pub(super) fn cacheable(&self) -> bool {
        !(self.scene.active_is_mask() && self.scene.tint_mask)
    }

    pub(super) fn go_to(&mut self, frame: u32) -> Result<()> {
        if self.timeline.player.is_some() {
            self.stop_playback();
        }
        let last = self.scene.bundle.doc.timeline.frames - 1;
        let frame = frame.min(last);
        if self.busy() || frame == self.scene.frame {
            return Ok(());
        }
        self.timeline.direction = if frame > self.scene.frame { 1 } else { -1 };
        self.scene.move_to(frame);
        let now = Instant::now();
        self.timeline.moved_at = now;
        self.timeline.step_start = Some(now);
        self.selection = None;
        self.picked = None;
        self.direct = None;
        self.cut_view = None;
        self.lasso = None;
        self.drop_lift();
        Ok(())
    }

    pub(super) fn invalidate_canvas(&mut self) {
        self.canvas_gen += 1;
        self.scene.stale = true;
    }

    pub(super) fn canvas_loading(&self) -> bool {
        self.canvas_wait.as_ref().is_some_and(|w| w.frame == self.scene.frame) && !self.canvas_current()
    }

    fn canvas_wanted(&self, wait: &CanvasWait) -> bool {
        wait.asked == self.canvas_gen
            && wait.frame == self.scene.frame
            && wait.active == self.scene.active
            && wait.rev == self.scene.bundle.doc.rev
    }

    pub(super) fn request_canvas(&mut self) {
        if self.canvas_wait.is_some() {
            return;
        }
        let mut request = self.scene.canvas_request(&self.brushes);
        if self.cacheable() {
            let (frame, onion) = (self.scene.frame, self.scene.onion_on);
            let key = self.frames.key(&mut self.scene, frame, onion, &self.brushes);
            request = request.with_picture(
                key,
                self.frames.level,
                self.scene.pictures(),
                self.buffering.writes.clone(),
            );
        }
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::background().install(|| request.run()));
            ctx.request_repaint();
        });
        self.canvas_wait = Some(CanvasWait {
            asked: self.canvas_gen,
            frame: self.scene.frame,
            active: self.scene.active.clone(),
            rev: self.scene.bundle.doc.rev,
            rx,
            since: Instant::now(),
        });
    }

    pub(super) fn tend_canvas(&mut self, wait: Duration) -> Result<()> {
        let Some(pending) = self.canvas_wait.as_ref() else {
            return Ok(());
        };
        let received = match wait.is_zero() {
            true => pending.rx.try_recv().map_err(|e| e == TryRecvError::Disconnected),
            false => pending
                .rx
                .recv_timeout(wait)
                .map_err(|e| e == RecvTimeoutError::Disconnected),
        };
        let result = match received {
            Ok(result) => result,
            Err(gone) => {
                if gone {
                    self.canvas_wait = None;
                }
                return Ok(());
            }
        };
        let Some(pending) = self.canvas_wait.take() else {
            return Ok(());
        };
        let mut done = match result {
            Ok(done) => done,
            Err(e) => {
                self.error(format!("loading frame {}: {e:#}", pending.frame));
                return Ok(());
            }
        };
        let wanted = self.canvas_wanted(&pending);
        let (frame, loaded) = (done.frame, done.loaded);
        self.frames.heavy |= done.bytes > crate::frames::HEAVY;
        self.scene.lean = self.frames.heavy;
        let thumbs = std::mem::take(&mut done.thumbs);
        if self.scene.adopt(done, wanted) {
            self.keep_thumbs(thumbs);
            self.dirty.extend(self.scene.all_tiles());
            self.timeline.canvas_frame = Some(self.scene.frame);
            self.measure(
                "canvas",
                format!(
                    "frame {frame} ready to draw on {:.0} ms after it was asked for ({loaded} drawings loaded for it, {} strokes and fills in memory)",
                    ms(pending.since),
                    self.scene.total_marks()
                ),
            );
        }
        Ok(())
    }

    fn catch_up(&mut self, wait: Duration) -> Result<bool> {
        if self.canvas_current() {
            return Ok(true);
        }
        if !self.scene.stale && self.scene.has_frame(self.scene.frame) {
            self.dirty.extend(self.scene.all_tiles());
            self.timeline.canvas_frame = Some(self.scene.frame);
            return Ok(true);
        }
        self.request_canvas();
        self.tend_canvas(wait)?;
        if !self.canvas_current() && self.canvas_wait.is_none() {
            self.request_canvas();
            self.tend_canvas(wait)?;
        }
        Ok(self.canvas_current())
    }

    pub(super) fn hold_still(&mut self) {
        if self.timeline.player.is_some() {
            self.stop_playback();
        }
        self.timeline.roll = None;
    }

    pub(super) fn ensure_canvas(&mut self) -> Result<bool> {
        self.hold_still();
        if !self.canvas_current() {
            if !self.catch_up(PEN_WAIT)? {
                return Ok(false);
            }
            self.timeline.on_screen = Some(self.scene.frame);
        }
        Ok(true)
    }

    fn shown(&mut self, frame: u32, cached: bool) {
        self.timeline.on_screen = Some(frame);
        if frame != self.scene.frame {
            return;
        }
        if let Some(start) = self.timeline.step_start.take() {
            let took = ms(start);
            crate::stats::lock(&self.stats).step_ms.push(took as f32);
            let text = format!(
                "frame {frame} shown in {took:.1} ms {}, {} MB of drawings and {} MB of frames ({}) in memory",
                if cached { "from the frame cache" } else { "composed" },
                self.scene.cache_bytes() >> 20,
                self.frames.bytes() >> 20,
                self.frames.len()
            );
            self.measure("frame step", text);
        }
    }

    fn forget_old_frames(&mut self) {
        if self.frames.is_empty() {
            return;
        }
        let frames = self.scene.bundle.doc.timeline.frames;
        let onion = self.scene.onion_on;
        let live: BTreeSet<u64> = (0..frames)
            .flat_map(|f| [false, onion].into_iter().map(move |o| (f, o)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|(f, o)| self.frames.key(&mut self.scene, f, o, &self.brushes))
            .collect();
        self.frames.forget_unused(&live);
    }

    pub(super) fn frames_changed(&mut self) {
        self.frames.changed();
        self.forget_old_frames();
    }

    pub(super) fn tend_frames(&mut self) -> Result<Option<u64>> {
        self.tend_canvas(Duration::ZERO)?;
        let now = Instant::now();
        let rev = self.scene.bundle.doc.rev;
        if rev != self.timeline.cache_rev {
            self.timeline.cache_rev = rev;
            self.frames_changed();
        }
        if self.timeline.player.as_ref().is_some_and(|p| p.finished(now)) {
            self.stop_playback();
        }
        self.frames.collect(&mut self.scene);
        self.scene.lean = self.frames.heavy;
        if let Some(e) = self.frames.error.take() {
            self.error(e);
        }
        let show = match self.timeline.player.is_some() {
            true => self.tend_playback(now),
            false => self.tend_still(now)?,
        };
        let keep: BTreeSet<u64> = show.into_iter().collect();
        self.frames.prune(&keep);
        Ok(show)
    }

    fn tend_still(&mut self, now: Instant) -> Result<Option<u64>> {
        let onion = self.scene.onion_on;
        let target = self.timeline.roll.unwrap_or(self.scene.frame);
        let cacheable = self.cacheable();
        let key = cacheable.then(|| self.frames.key(&mut self.scene, target, onion, &self.brushes));
        let cached = key.filter(|k| self.frames.has(*k));
        let rolling = self.timeline.roll.is_some();
        if !rolling && !self.canvas_current() {
            let settled = now.duration_since(self.timeline.moved_at) >= SETTLE && self.timeline.flip.is_none();
            let loaded = self.scene.has_frame(self.scene.frame);
            let heavy = self.frames.heavy;
            if (loaded && (cached.is_none() || settled)) || !cacheable || (heavy && settled) {
                self.catch_up(Duration::ZERO)?;
            }
            if cached.is_some() && !self.canvas_current() {
                self.ctx.request_repaint_after(SETTLE);
            }
        }
        if !rolling && self.canvas_current() {
            let filling = self.dirty.len() > FILL && self.live.is_none();
            if let Some(key) = self.timeline.shown_key.filter(|k| filling && self.frames.has(*k)) {
                self.frames.touch(key);
                self.ctx.request_repaint();
                return Ok(Some(key));
            }
            self.timeline.shown_key = None;
            self.shown(self.scene.frame, false);
            self.prefetch(onion);
            return Ok(None);
        }
        if let Some(key) = cached {
            self.frames.touch(key);
            self.timeline.shown_key = Some(key);
            self.shown(target, true);
            self.prefetch(onion);
            return Ok(Some(key));
        }
        let seen = key.is_some_and(|k| self.frames.on_disk(k));
        let loading_here = !seen && (self.frames.heavy || self.canvas_wait.as_ref().is_some_and(|w| w.frame == target));
        if !loading_here {
            self.frames.request(&mut self.scene, target, onion, &self.brushes, true);
        }
        Ok(self.timeline.shown_key.filter(|k| self.frames.has(*k)))
    }

    fn prefetch(&mut self, onion: bool) {
        let direction = self.timeline.direction;
        if direction == 0 || self.busy() || !self.cacheable() {
            return;
        }
        let last = self.scene.bundle.doc.timeline.frames as i64 - 1;
        let from = self.scene.frame as i64;
        (1..=AHEAD as i64)
            .map(|k| from + k * direction as i64)
            .filter(|f| (0..=last).contains(f))
            .filter(|f| {
                !self.frames.heavy || {
                    let key = self.frames.key(&mut self.scene, *f as u32, onion, &self.brushes);
                    self.frames.on_disk(key)
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .try_for_each(|f| {
                match self
                    .frames
                    .request(&mut self.scene, f as u32, onion, &self.brushes, false)
                {
                    true => Some(()),
                    false => None,
                }
            });
    }

    fn tend_playback(&mut self, now: Instant) -> Option<u64> {
        let player = self.timeline.player.as_mut()?;
        let (frames, scene, brushes) = (&mut self.frames, &mut self.scene, &self.brushes);
        player.tick(now, |f| {
            let key = frames.key(scene, f, false, brushes);
            frames.has(key)
        });
        let length = (player.to - player.from + 1) as usize;
        player
            .ahead(now, length)
            .into_iter()
            .try_for_each(|f| match frames.request(scene, f, false, brushes, false) {
                true => Some(()),
                false => None,
            });
        let wait = player.next_in(now);
        let shown = player.shown();
        self.ctx.request_repaint_after(wait);
        let key = shown.map(|f| frames.key(scene, f, false, brushes));
        if let Some(f) = shown {
            self.timeline.on_screen = Some(f);
        }
        key.filter(|k| self.frames.has(*k))
    }

    pub(super) fn play(&mut self) -> Result<()> {
        if self.timeline.player.is_some() {
            self.stop_playback();
            return Ok(());
        }
        if self.busy() {
            return Ok(());
        }
        let t = &self.scene.bundle.doc.timeline;
        let span = clips::play_span(t, self.scene.frame, self.timeline.range, self.timeline.clip.as_deref());
        self.timeline.roll = None;
        self.timeline.flip = None;
        self.timeline.player = Some(Player::new(
            Instant::now(),
            self.scene.frame,
            (span.from, span.to),
            span.fps,
            span.once,
        ));
        self.timeline.playing = format!(
            "{} at {} fps{}",
            span.what,
            span.fps,
            if span.once { ", once" } else { "" }
        );
        Ok(())
    }

    pub(super) fn stop_playback(&mut self) {
        let Some(player) = self.timeline.player.take() else {
            return;
        };
        let played = format!("played {}: {}", self.timeline.playing, player.summary());
        self.measure("last play", played.clone());
        self.say(Tone::Done, played);
        if let Some(frame) = player.shown() {
            if frame != self.scene.frame {
                self.scene.move_to(frame);
                self.timeline.direction = 0;
            }
            self.timeline.shown_key = Some(self.frames.key(&mut self.scene, frame, false, &self.brushes));
        }
        self.timeline.moved_at = Instant::now();
    }

    pub(super) fn shown_frame(&self) -> u32 {
        match (&self.timeline.player, self.timeline.roll) {
            (Some(p), _) => p.shown().unwrap_or(self.scene.frame),
            (None, Some(roll)) => roll,
            _ => self.scene.frame,
        }
    }

    pub(super) fn waiting(&self) -> bool {
        self.timeline.on_screen != Some(self.scene.frame)
    }

    pub(super) fn toggle_onion(&mut self) -> Result<()> {
        self.scene.onion_on = !self.scene.onion_on;
        self.onion_changed()
    }

    pub(super) fn onion_changed(&mut self) -> Result<()> {
        self.scene.onion.wrap = self.timeline.range;
        self.invalidate_canvas();
        self.frames_changed();
        Ok(())
    }

    pub(super) fn previous_drawing(&self) -> Option<u32> {
        let layer = self.scene.active_layer()?;
        let frame = self.scene.frame;
        let start = layer.cel_at(frame).map_or(frame, |c| c.frame);
        let starts: Vec<u32> = layer.cels().iter().map(|c| c.frame).collect();
        let earlier = starts.iter().rev().find(|f| **f < start).copied();
        match self.timeline.range {
            Some((a, b)) if (a..=b).contains(&frame) && earlier.is_none_or(|f| f < a) => starts
                .iter()
                .rev()
                .find(|f| **f > start && **f <= b)
                .copied()
                .or(earlier),
            _ => earlier,
        }
    }

    pub(super) fn flip_and_roll(&mut self, ui: &egui::Ui) -> Result<()> {
        let (q, w) = ui.input(|i| (i.key_down(Key::Q), i.key_down(Key::W)));
        let x = ui.input(|i| i.pointer.latest_pos()).map(|p| p.x);
        match (q, self.timeline.flip.as_ref(), x) {
            (true, None, Some(x)) if !self.busy() => {
                if self.timeline.player.is_some() {
                    self.stop_playback();
                }
                self.timeline.flip = Some(Flip {
                    origin: self.scene.frame,
                    x,
                });
            }
            (true, Some(flip), Some(x)) => {
                let last = self.scene.bundle.doc.timeline.frames as i64 - 1;
                let offset = ((x - flip.x) / FLIP_PX).round() as i64;
                let frame = (flip.origin as i64 + offset).clamp(0, last) as u32;
                if frame != self.scene.frame {
                    self.go_to(frame)?;
                }
            }
            (false, Some(_), _) => {
                self.timeline.flip = None;
                self.timeline.moved_at = Instant::now();
            }
            _ => {}
        }
        self.timeline.roll = match w && self.timeline.player.is_none() && !self.busy() {
            true => self.timeline.roll.or_else(|| self.previous_drawing()),
            false => None,
        };
        Ok(())
    }

    pub(super) fn holding_frame_keys(&self) -> bool {
        self.timeline.flip.is_some() || self.timeline.roll.is_some()
    }

    fn active_layer_id(&self) -> Option<String> {
        self.scene.active_id()
    }

    pub(super) fn here(&self) -> Option<(String, Option<(Cel, u32)>)> {
        let layer = self.scene.active_layer()?;
        if !matches!(layer.kind(), Kind::Paint | Kind::Vector) {
            return None;
        }
        Some((layer.id.clone(), exposed(layer, self.scene.frame)))
    }

    pub(super) fn drawing_starts(&self) -> Vec<u32> {
        self.scene
            .active_layer()
            .map(|l| l.cels().iter().map(|c| c.frame).collect())
            .unwrap_or_default()
    }

    pub(super) fn next_drawing(&mut self) -> Result<()> {
        let Some(layer) = self
            .scene
            .active_layer()
            .filter(|l| matches!(l.kind(), Kind::Paint | Kind::Vector))
            .cloned()
        else {
            return Ok(());
        };
        let on = self.timeline.on.max(1);
        let frame = self.scene.frame;
        let frames = self.scene.bundle.doc.timeline.frames;
        let mut ops = Vec::new();
        let at = match exposed(&layer, frame) {
            None => frame,
            Some((cel, end)) if end == OPEN => {
                let at = cel.frame + on * (frame + 1 - cel.frame).div_ceil(on);
                ops.push(Op::Set {
                    id: cel.id.clone(),
                    props: Map::from_iter([("hold".to_string(), json!(at - cel.frame))]),
                });
                at
            }
            Some((_, end)) => end,
        };
        let taken = layer.cels().iter().any(|c| c.frame == at);
        ops.push(Op::Cel(CelSpec {
            action: CelAction::New {
                layer: layer.id.clone(),
            },
            id: None,
            frame: Some(at),
            hold: Some(on),
            insert: taken,
        }));
        if at + on > frames.max(at + 1) && !taken {
            ops.push(Op::Timeline {
                fps: None,
                frames: Some(at + on),
                origin: None,
            });
        }
        if self.timeline_apply(ops)?.is_some() {
            self.go_to(at)?;
        }
        Ok(())
    }

    pub(super) fn timeline_apply(&mut self, ops: Vec<Op>) -> Result<Option<klad_core::ops::Applied>> {
        if ops.is_empty() {
            return Ok(None);
        }
        let start = Instant::now();
        match self.scene.apply_with(&ops, &self.brushes) {
            Ok((dirty, applied)) => {
                self.dirty.extend(dirty);
                self.forget_missing_selection();
                let text = format!("{} in {:.0} ms", klad_core::bundle::summarize(&ops), ms(start));
                self.measure("timeline edit", text.clone());
                self.say(Tone::Info, text);
                Ok(Some(applied))
            }
            Err(e) => {
                self.say(Tone::Refused, format!("{e:#}"));
                Ok(None)
            }
        }
    }

    fn cel_op(&mut self, action: CelAction, frame: Option<u32>, insert: bool) -> Result<Option<String>> {
        let spec = CelSpec {
            action,
            id: None,
            frame,
            hold: None,
            insert,
        };
        Ok(self
            .timeline_apply(vec![Op::Cel(spec)])?
            .and_then(|a| a.added.first().cloned()))
    }

    pub(super) fn new_drawing(&mut self) -> Result<()> {
        let Some((layer, _)) = self.here() else {
            return Ok(());
        };
        self.cel_op(CelAction::New { layer }, Some(self.scene.frame), false)?;
        Ok(())
    }

    pub(super) fn duplicate_next(&mut self) -> Result<()> {
        let Some((_, Some((cel, end)))) = self.here() else {
            self.say(Tone::Refused, "nothing to duplicate on this frame");
            return Ok(());
        };
        let frames = self.scene.bundle.doc.timeline.frames;
        let at = if end == OPEN { frames.max(cel.frame + 1) } else { end };
        if self
            .cel_op(CelAction::Dup { cel: cel.id }, None, end != OPEN)?
            .is_some()
        {
            self.go_to(at)?;
        }
        Ok(())
    }

    pub(super) fn link_here(&mut self, source: String) -> Result<()> {
        self.cel_op(CelAction::Link { cel: source }, Some(self.scene.frame), false)?;
        Ok(())
    }

    pub(super) fn unlink_here(&mut self) -> Result<()> {
        if let Some((_, Some((cel, _)))) = self
            .here()
            .filter(|(_, c)| c.as_ref().is_some_and(|(c, _)| c.link.is_some()))
        {
            self.cel_op(CelAction::Unlink { cel: cel.id }, None, false)?;
        }
        Ok(())
    }

    pub(super) fn delete_here(&mut self) -> Result<()> {
        if let Some((_, Some((cel, _)))) = self.here() {
            self.timeline_apply(vec![Op::Remove { ids: vec![cel.id] }])?;
        }
        Ok(())
    }

    pub(super) fn frames_at(&mut self, insert: bool, every_layer: bool) -> Result<()> {
        let Some(layer) = self.active_layer_id() else {
            return Ok(());
        };
        if !insert && self.scene.bundle.doc.timeline.frames == 1 {
            return Ok(());
        }
        let spec = FrameSpec {
            insert,
            at: self.scene.frame,
            count: 1,
            layers: (!every_layer).then(|| vec![layer]),
        };
        self.timeline_apply(vec![Op::Frames(spec)])?;
        Ok(())
    }

    pub(super) fn resize(
        &mut self,
        layer: &str,
        cel: &str,
        (start, end, open): (u32, u32, bool),
        to: u32,
    ) -> Result<()> {
        let to = to.max(start + 1);
        let op = match to.cmp(&end) {
            std::cmp::Ordering::Equal => return Ok(()),
            std::cmp::Ordering::Greater => Op::Frames(FrameSpec {
                insert: true,
                at: end - 1,
                count: to - end,
                layers: Some(vec![layer.to_string()]),
            }),
            std::cmp::Ordering::Less if open => Op::Set {
                id: cel.to_string(),
                props: Map::from_iter([("hold".to_string(), json!(to - start))]),
            },
            std::cmp::Ordering::Less => Op::Frames(FrameSpec {
                insert: false,
                at: to,
                count: end - to,
                layers: Some(vec![layer.to_string()]),
            }),
        };
        self.timeline_apply(vec![op])?;
        Ok(())
    }

    pub(super) fn hold(&mut self, delta: i32) -> Result<()> {
        let Some((layer, Some((cel, end)))) = self.here() else {
            return Ok(());
        };
        let frames = self.scene.bundle.doc.timeline.frames;
        let (open, end) = (end == OPEN, end.min(frames));
        if delta < 0 && end - cel.frame <= 1 {
            self.say(
                Tone::Refused,
                format!("{} already shows for one frame; delete it instead", cel.id),
            );
            return Ok(());
        }
        let to = (end as i64 + delta as i64).max(cel.frame as i64 + 1) as u32;
        self.resize(&layer, &cel.id, (cel.frame, end, open), to)
    }

    pub(super) fn add_layer(&mut self, kind: &str) -> Result<()> {
        let frame = self.scene.frame;
        self.add_layer_value(json!({"kind": kind, "cels": [{"frame": frame}]}))
    }

    pub(super) fn add_layer_value(&mut self, layer: serde_json::Value) -> Result<()> {
        let above = self.scene.active.clone();
        let owner = match above.last() {
            Some(&MASK) => above[..above.len() - 1].to_vec(),
            _ => above,
        };
        let place = match self.scene.bundle.doc.locate_path(&owner) {
            Some(layer) => Place {
                above: Some(layer.id.clone()),
                ..Place::default()
            },
            None => Place::default(),
        };
        let Some(applied) = self.timeline_apply(vec![Op::AddLayer { layer, place }])? else {
            return Ok(());
        };
        if let Some(Loc::Layer(path)) = applied.added.first().and_then(|id| self.scene.bundle.doc.locate(id)) {
            self.activate(path)?;
        }
        Ok(())
    }

    fn timeline_rows(&self) -> Vec<LayerRow> {
        self.layer_rows(layers::Where::Timeline)
    }

    fn timeline_header(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let t = self.scene.bundle.doc.timeline.clone();
        if !ui.memory(|m| m.focused().is_some()) {
            self.timeline.fps = t.fps;
            self.timeline.length = t.frames;
        }
        let mut ops = Vec::new();
        let mut range = self.timeline.range;
        let mut go = None;
        let folded = self.settings.layout.timeline_folded;
        ui.horizontal_wrapped(|ui| {
            let caret = if folded { icon::FOLDED } else { icon::OPEN };
            if ui
                .add(
                    egui::Button::new(RichText::new(caret).size(theme::ICON))
                        .min_size(vec2(theme::TARGET, theme::TARGET)),
                )
                .hint("fold the timeline to this row, or open it again")
                .clicked()
            {
                self.settings.layout.timeline_folded = !folded;
                self.settings_dirty = true;
            }
            let shown = self.shown_frame();
            let mut typed = shown;
            let label = ui.label(RichText::new("frame").strong());
            let field = ui
                .add(egui::DragValue::new(&mut typed).range(0..=t.frames - 1))
                .labelled_by(label.id)
                .hint("the frame on screen; drag, or double-click and type a frame to go there");
            if typed != shown && (field.lost_focus() || field.dragged() || field.changed()) {
                go = Some(typed);
            }
            ui.label(RichText::new(format!("of {}", t.frames)).color(theme::WEAK));
            if self.timeline.player.is_none() && ((self.frames.jobs() > 0 && self.waiting()) || self.canvas_loading()) {
                ui.label(RichText::new("loading").color(theme::WEAK));
            }
            [
                Cmd::FirstFrame,
                Cmd::PrevDrawing,
                Cmd::PrevFrame,
                Cmd::Play,
                Cmd::NextFrame,
                Cmd::NextDrawing,
                Cmd::LastFrame,
            ]
            .into_iter()
            .for_each(|cmd| {
                self.command_button(ui, cmd, None);
            });
            ui.separator();
            self.command_button(ui, Cmd::LoopRange, None);
            if let Some((a, b)) = range.as_mut() {
                ui.add(egui::DragValue::new(a).range(0..=t.frames - 1))
                    .hint("the loop range's first frame");
                ui.label("to");
                ui.add(egui::DragValue::new(b).range(0..=t.frames - 1))
                    .hint("the loop range's last frame");
                *b = (*b).max(*a);
            }
            self.command_button(ui, Cmd::Prepare, None);
            if let Some(text) = self.preparing_text() {
                ui.label(RichText::new(text).color(theme::WEAK));
            }
            ui.separator();
            self.command_button(ui, Cmd::Onion, None);
            self.command_button(ui, Cmd::OnionSettings, None);
            ui.separator();
            ui.label("shift K on");
            [
                (Cmd::ExposeOnes, "1s"),
                (Cmd::ExposeTwos, "2s"),
                (Cmd::ExposeThrees, "3s"),
            ]
            .into_iter()
            .for_each(|(cmd, label)| {
                self.command_button(ui, cmd, Some(label));
            });
            ui.separator();
            ui.label("fps");
            let fps = ui
                .add(egui::DragValue::new(&mut self.timeline.fps).range(1..=240))
                .hint("frames per second of the document; drag, or double-click and type");
            ui.label("length");
            let length = ui
                .add(egui::DragValue::new(&mut self.timeline.length).range(1..=100_000))
                .hint("the timeline's length in frames");
            let done = |r: &egui::Response| r.drag_stopped() || r.lost_focus();
            if (done(&fps) || done(&length)) && (self.timeline.fps != t.fps || self.timeline.length != t.frames) {
                ops.push(Op::Timeline {
                    fps: (self.timeline.fps != t.fps).then_some(self.timeline.fps),
                    frames: (self.timeline.length != t.frames).then_some(self.timeline.length),
                    origin: None,
                });
            }
            ui.label("zoom");
            ui.add(egui::Slider::new(&mut self.timeline.cell, 8.0..=40.0).show_value(false))
                .hint("the width of a frame in the timeline");
        });
        self.timeline_apply(ops)?;
        if let Some(frame) = go {
            self.go_to(frame)?;
        }
        if range != self.timeline.range {
            self.timeline.range = range;
            self.onion_changed()?;
        }
        if folded {
            return Ok(());
        }
        self.selection_toolbar(ui)?;
        Ok(())
    }

    pub(super) fn onion_settings(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut onion = self.scene.onion.clone();
        let mut on = self.scene.onion_on;
        ui.checkbox(&mut on, "onion skin (O)")
            .hint("see the drawings before (red) and after (green) under the active layer");
        ui.horizontal(|ui| {
            ui.label("before");
            ui.add(egui::DragValue::new(&mut onion.before).range(0..=12))
                .hint("how many drawings or frames before this one");
            ui.label("after");
            ui.add(egui::DragValue::new(&mut onion.after).range(0..=12))
                .hint("how many drawings or frames after this one");
        });
        ui.horizontal(|ui| {
            ui.label("count");
            ui.selectable_value(&mut onion.by, By::Drawings, "drawings")
                .hint("count drawings: each one once, at its nearest distance");
            ui.selectable_value(&mut onion.by, By::Frames, "frames")
                .hint("count frames: on twos the nearest drawing is two frames off and fainter");
        });
        ui.add(egui::Slider::new(&mut onion.opacity, 0.05..=1.0).text("opacity"))
            .hint("the nearest ghost's opacity");
        ui.add(egui::Slider::new(&mut onion.falloff, 0.1..=1.0).text("falloff"))
            .hint("each step further keeps this much of the opacity");
        ui.checkbox(&mut onion.tint, "tint: red before, green after")
            .hint("off: the ghosts keep their own colours");
        ui.small("The dot at the right of a layer's name in the timeline shows that layer's onion too.");
        if onion != self.scene.onion || on != self.scene.onion_on {
            self.scene.onion = onion;
            self.scene.onion_on = on;
            self.onion_changed()?;
        }
        Ok(())
    }

    pub(super) fn draws_into(&self) -> String {
        let frame = self.scene.frame;
        let frames = self.scene.bundle.doc.timeline.frames;
        let Some(layer) = self.scene.active_layer() else {
            return "no active layer".into();
        };
        let name = &layer.name;
        if layer.kind() == Kind::Group {
            return format!("{name} is a group: pick a paint layer to draw");
        }
        if layer.kind() == Kind::Adjust {
            return format!("{name} is an adjustment layer: it changes what is under it; pick a paint layer to draw");
        }
        let span = |start: u32, end: u32| match end.min(frames) - start {
            1 => format!("frame {start}"),
            _ => format!("frames {start} to {}", end.min(frames) - 1),
        };
        match exposed(layer, frame) {
            None => format!("{name}: frame {frame} is empty, a stroke starts a drawing"),
            Some((cel, _)) if cel.link.is_some() => {
                let source = cel.link.clone().unwrap_or_default();
                format!(
                    "draws into {source} on {name} through the link {} (it changes wherever {source} shows)",
                    cel.id
                )
            }
            Some((cel, end)) => format!("draws into {} on {name}, {}", cel.id, span(cel.frame, end)),
        }
    }

    fn names(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let band = clips::band_height(&self.scene.bundle.doc.timeline.clips);
        let (strip, _) = ui.allocate_exact_size(vec2(NAMES, band), Sense::hover());
        ui.painter().text(
            pos2(strip.left() + 4.0, strip.top() + clips::LANE / 2.0),
            Align2::LEFT_CENTER,
            "clips",
            FontId::proportional(11.0),
            Color32::GRAY,
        );
        let (rect, _) = ui.allocate_exact_size(vec2(NAMES, RULER), Sense::hover());
        ui.painter().text(
            rect.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            "layers",
            FontId::proportional(11.0),
            Color32::GRAY,
        );
        let rows = self.layer_rows(layers::Where::Timeline);
        let mut placed = Vec::new();
        let mut responses = Vec::new();
        rows.into_iter().for_each(|row| {
            let (rect, _) = ui.allocate_exact_size(vec2(NAMES, ROW), Sense::hover());
            responses.push(self.layer_row(ui, &row, layers::Where::Timeline, rect));
            placed.push((rect, row));
        });
        self.layer_drags(ui, layers::Where::Timeline, &placed, &responses);
        Ok(())
    }

    fn grid(&mut self, ui: &mut egui::Ui, rows: &[LayerRow]) -> Result<()> {
        let doc = &self.scene.bundle.doc;
        let frames = doc.timeline.frames;
        let cw = self.timeline.cell;
        let clip_list = doc.timeline.clips.clone();
        let band = clips::band_height(&clip_list);
        let size = vec2(frames as f32 * cw + PAST, band + RULER + rows.len() as f32 * ROW);
        let (rect, response) = ui.allocate_exact_size(size, Sense::CLICK | Sense::DRAG);
        let painter = ui.painter_at(rect);
        let top = rect.top() + band;
        painter.rect_filled(rect, 0.0, Color32::from_gray(26));
        painter.rect_filled(
            Rect::from_min_max(pos2(rect.left() + frames as f32 * cw, top), rect.max),
            0.0,
            Color32::from_gray(18),
        );
        let strip = Strip {
            rect: Rect::from_min_max(rect.min, pos2(rect.right(), top)),
            cell: cw,
            lanes: clips::lanes(&clip_list),
        };
        let dragged_span: Vec<(usize, u32, u32)> = match &self.timeline.drag {
            Some(Drag::ClipEnd { index, from, to, .. }) => vec![(*index, *from, *to)],
            _ => Vec::new(),
        };
        strip.paint(
            &painter,
            &clip_list,
            doc.timeline.fps,
            self.timeline.clip.as_deref(),
            &dragged_span,
        );
        if let Some(Drag::NewClip { from, to }) = &self.timeline.drag {
            strip.outline(&painter, *from, *to);
        }
        let x_of = |f: u32| rect.left() + f as f32 * cw;
        let row_rect =
            |i: usize| Rect::from_min_size(pos2(rect.left(), top + RULER + i as f32 * ROW), vec2(rect.width(), ROW));
        let shown = self.shown_frame();
        let step = if cw >= 14.0 { 1 } else { 5 };
        (0..frames).for_each(|f| {
            let x = x_of(f);
            let strong = f % 5 == 0;
            painter.line_segment(
                [pos2(x, top + RULER), pos2(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_gray(if strong { 52 } else { 38 })),
            );
            if f % step == 0 {
                painter.text(
                    pos2(x + cw / 2.0, top + RULER / 2.0),
                    Align2::CENTER_CENTER,
                    f.to_string(),
                    FontId::monospace(if cw >= 18.0 { 10.0 } else { 9.0 }),
                    if strong { Color32::LIGHT_GRAY } else { Color32::GRAY },
                );
            }
        });
        let ready = &self.buffering.shown;
        (0..frames).for_each(|f| {
            let at = |y: f32, h: f32| Rect::from_min_size(pos2(x_of(f), top + y), vec2(cw, h));
            if ready.watch.contains(&f) {
                painter.rect_filled(at(1.0, 3.0), 0.0, Color32::from_rgb(90, 200, 110));
            }
            if ready.draw.contains(&f) {
                painter.rect_filled(at(4.0, 2.0), 0.0, Color32::from_rgb(90, 160, 230));
            }
        });
        if response.hover_pos().is_some_and(|p| p.y >= top && p.y < top + 7.0) {
            response.clone().on_hover_text_at_pointer(
                "green: the frame's picture is buffered, it shows at once; blue: its drawings are cached, so drawing there starts within seconds. The lightning (prepare frames) fills both",
            );
        }
        let band = match &self.timeline.drag {
            Some(Drag::Range { from, to }) => Some(((*from).min(*to), (*from).max(*to))),
            _ => self.timeline.range,
        };
        if let Some((a, b)) = band {
            painter.rect_filled(
                Rect::from_min_max(pos2(x_of(a), top), pos2(x_of(b + 1), top + RULER)),
                0.0,
                Color32::from_rgba_unmultiplied(255, 170, 60, 70),
            );
            painter.line_segment(
                [pos2(x_of(a), top + RULER - 1.0), pos2(x_of(b + 1), top + RULER - 1.0)],
                Stroke::new(2.0, Color32::from_rgb(255, 170, 60)),
            );
        }
        rows.iter().enumerate().for_each(|(i, row)| {
            let r = row_rect(i);
            if self.scene.active == row.path {
                painter.rect_filled(r, 0.0, Color32::from_gray(36));
            }
            painter.line_segment(
                [r.left_bottom(), r.right_bottom()],
                Stroke::new(1.0, Color32::from_gray(40)),
            );
            let Some(layer) = doc.locate_path(&row.path) else {
                return;
            };
            blocks(layer, frames).iter().for_each(|b| {
                let end = match &self.timeline.drag {
                    Some(Drag::Hold { cel, to, .. }) if *cel == b.cel => *to,
                    _ => b.end,
                };
                let fill = match (&b.link, layer.kind()) {
                    (Some(_), _) => Color32::from_rgb(112, 92, 146),
                    (None, Kind::Vector) => Color32::from_rgb(78, 124, 96),
                    _ if b.tween => Color32::from_rgb(100, 136, 166),
                    _ => Color32::from_rgb(76, 104, 140),
                };
                let label = match &b.link {
                    Some(source) => format!("->{source}"),
                    None if b.tween => format!("{}*", b.cel),
                    None => b.cel.clone(),
                };
                let moved_to = match &self.timeline.drag {
                    Some(Drag::Move { cel, from, to }) if *cel == b.cel && from != to => Some(*to),
                    _ => None,
                };
                let at = |start: u32, end: u32| {
                    Rect::from_min_max(
                        pos2(x_of(start) + 1.0, r.top() + 3.0),
                        pos2(x_of(end) - 1.0, r.bottom() - 3.0),
                    )
                };
                let faded = moved_to.is_some();
                paint_block(&painter, at(b.start, end), cw, fill, &label, faded);
                if let Some(to) = moved_to {
                    let ghost = at(to, to + (b.end - b.start));
                    paint_block(&painter, ghost, cw, fill, &label, false);
                    painter.rect_stroke(ghost, 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
                }
            });
        });
        let selected = match &self.timeline.drag {
            Some(Drag::Select { from, to, first, last }) => {
                span_of(rows, (*from, *to), (*first, *last), (*from, *first))
            }
            _ => self.timeline.span.clone(),
        };
        if let Some(span) = &selected {
            let found: Vec<usize> = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| span.rows.contains(&r.id))
                .map(|(i, _)| i)
                .collect();
            if let (Some(&a), Some(&b)) = (found.first(), found.last()) {
                let area = Rect::from_min_max(
                    pos2(x_of(span.from), row_rect(a).top()),
                    pos2(x_of(span.to + 1), row_rect(b).bottom()),
                );
                painter.rect_filled(area, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 34));
                painter.rect_stroke(
                    area,
                    2.0,
                    Stroke::new(1.5, Color32::from_gray(235)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        painter.rect_filled(
            Rect::from_min_size(pos2(x_of(shown), rect.top()), vec2(cw, rect.height())),
            0.0,
            Color32::from_rgba_unmultiplied(64, 196, 255, 46),
        );
        painter.line_segment(
            [pos2(x_of(shown), rect.top()), pos2(x_of(shown), rect.bottom())],
            Stroke::new(1.5, Color32::from_rgb(64, 196, 255)),
        );
        let geom = Geom {
            left: rect.left(),
            top,
            cell: cw,
            frames,
            rows: rows.len(),
        };
        self.timeline.geom = Some(geom);
        let row_blocks: Vec<Vec<Block>> = rows
            .iter()
            .map(|r| doc.locate_path(&r.path).map_or(Vec::new(), |l| blocks(l, frames)))
            .collect();
        let at = |p: Pos2| -> (u32, Option<usize>) { (geom.frame_at(p.x), geom.row_at(p.y)) };
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let press = ui.input(|i| i.pointer.press_origin());
        let on_edge = |p: Pos2| matches!(press_at(&geom, &row_blocks, p), Some(Press::End(..)));
        let in_strip = |p: Pos2| p.y < top;
        let on_clip_end =
            |p: Pos2| in_strip(p) && matches!(strip.hit(&clip_list, p), Some((_, Hit::First | Hit::Last)));
        let resizing = matches!(self.timeline.drag, Some(Drag::Hold { .. } | Drag::ClipEnd { .. }));
        let hover_end = response.hover_pos().is_some_and(|p| on_edge(p) || on_clip_end(p));
        let hover_dot = response
            .hover_pos()
            .is_some_and(|p| matches!(press_at(&geom, &row_blocks, p), Some(Press::Start(..))));
        if resizing || (self.timeline.drag.is_none() && hover_end) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if matches!(self.timeline.drag, Some(Drag::Move { .. })) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if self.timeline.drag.is_none() && hover_dot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let mut pick: Option<(u32, Option<usize>)> = None;
        let shift = ui.input(|i| i.modifiers.shift);
        if response.drag_started()
            && let Some(p) = press.or(pointer)
        {
            let (f, row) = at(p);
            let clip_hit = in_strip(p).then(|| strip.hit(&clip_list, p)).flatten();
            if let Some((i, _)) = clip_hit {
                self.timeline.clip = Some(clip_list[i].name.clone());
            }
            self.timeline.drag = match press_at(&geom, &row_blocks, p) {
                _ if in_strip(p) => match clip_hit {
                    Some((i, hit @ (Hit::First | Hit::Last))) => Some(Drag::ClipEnd {
                        index: i,
                        name: clip_list[i].name.clone(),
                        first: hit == Hit::First,
                        from: clip_list[i].from,
                        to: clip_list[i].to,
                    }),
                    Some(_) => Some(Drag::Scrub),
                    None => Some(Drag::NewClip { from: f, to: f }),
                },
                Some(press) => Some(drag_of(&press, shift, |r| rows[r].id.clone())),
                None => None,
            };
            if matches!(
                self.timeline.drag,
                Some(Drag::Scrub | Drag::Hold { .. } | Drag::Move { .. } | Drag::Select { .. })
            ) {
                pick = Some((f, row));
            }
        }
        if response.dragged()
            && let Some(p) = pointer
        {
            let edge = ((p.x - rect.left()) / cw).round().max(0.0) as u32;
            let (f, under) = at(p);
            match &mut self.timeline.drag {
                Some(Drag::Scrub) => self.go_to(f)?,
                Some(Drag::Select { to, last, .. }) => {
                    *to = f;
                    if let Some(r) = under {
                        *last = r;
                    }
                }
                Some(Drag::Range { to, .. }) => *to = f,
                Some(Drag::Hold { start, to, .. }) => *to = edge.max(*start + 1),
                Some(Drag::Move { to, .. }) => *to = f,
                Some(Drag::NewClip { to, .. }) => *to = f,
                Some(Drag::ClipEnd {
                    first: true, from, to, ..
                }) => *from = edge.min(*to),
                Some(Drag::ClipEnd { from, to, .. }) => *to = edge.saturating_sub(1).max(*from).min(frames - 1),
                None => {}
            }
        }
        if response.drag_stopped() {
            match self.timeline.drag.take() {
                Some(Drag::Range { from, to }) => {
                    self.timeline.range = Some((from.min(to), from.max(to)));
                    self.onion_changed()?;
                }
                Some(Drag::Hold {
                    layer,
                    cel,
                    start,
                    end,
                    open,
                    to,
                }) => self.resize(&layer, &cel, (start, end, open), to)?,
                Some(Drag::Move { cel, from, to }) if from != to => {
                    let op = Op::Cel(CelSpec {
                        action: CelAction::Move { cel },
                        id: None,
                        frame: Some(to),
                        hold: None,
                        insert: false,
                    });
                    if self.timeline_apply(vec![op])?.is_some() {
                        self.go_to(to)?;
                    }
                }
                Some(Drag::NewClip { from, to }) => self.add_clip(from, to)?,
                Some(Drag::Select { from, to, first, last }) => {
                    self.timeline.span = span_of(rows, (from, to), (first, last), (from, first));
                }
                Some(Drag::ClipEnd { name, from, to, .. }) => self.reach_clip(&name, from, to)?,
                _ => {}
            }
        }
        let mut extended = false;
        if response.clicked()
            && shift
            && let Some(p) = pointer
            && let (f, Some(r)) = at(p)
        {
            let anchor = self.timeline.span.as_ref().map_or_else(
                || {
                    (
                        self.scene.frame,
                        rows.iter().position(|row| row.path == self.scene.active).unwrap_or(r),
                    )
                },
                |s| s.anchor,
            );
            self.timeline.span = span_of(rows, (anchor.0, f), (anchor.1, r), anchor);
            extended = true;
        }
        if (response.clicked() || response.secondary_clicked())
            && !extended
            && let Some(p) = pointer
        {
            if in_strip(p) && response.clicked() {
                self.timeline.clip = strip.hit(&clip_list, p).map(|(i, _)| clip_list[i].name.clone());
                self.windows.clip = self.timeline.clip.is_some();
            }
            if response.clicked() {
                self.timeline.span = None;
            }
            pick = Some(at(p));
        }
        if let Some((f, row)) = pick {
            if let Some(path) = row.map(|r| rows[r].path.clone()).filter(|p| *p != self.scene.active) {
                let leaf = self
                    .scene
                    .bundle
                    .doc
                    .locate_path(&path)
                    .is_some_and(|l| matches!(l.kind(), Kind::Paint | Kind::Vector));
                if leaf {
                    self.activate(path)?;
                }
            }
            self.go_to(f)?;
        }
        let mut chosen: Option<String> = None;
        let mut actions: Vec<Cmd> = Vec::new();
        let drawings: Vec<(String, u32)> = self
            .scene
            .active_layer()
            .map(|l| {
                l.cels()
                    .iter()
                    .filter(|c| c.link.is_none())
                    .map(|c| (c.id.clone(), c.frame))
                    .collect()
            })
            .unwrap_or_default();
        let span = self.timeline.span.clone();
        let span_keys = span.as_ref().and_then(|s| tweens::keys(&self.scene.bundle.doc, s));
        let mut span_choice = None;
        let ask = &mut self.timeline.ask;
        response.context_menu(|ui| {
            [
                Cmd::NewDrawing,
                Cmd::Duplicate,
                Cmd::Unlink,
                Cmd::DeleteDrawing,
                Cmd::HoldLonger,
                Cmd::HoldShorter,
                Cmd::InsertFrame,
                Cmd::InsertFrameAll,
                Cmd::RemoveFrame,
                Cmd::RemoveFrameAll,
                Cmd::NewClip,
            ]
            .into_iter()
            .for_each(|cmd| {
                let spec = cmd.spec();
                let key = self.bindings.label(cmd);
                let label = match key.is_empty() {
                    true => format!("{} {}", spec.icon, spec.name),
                    false => format!("{} {} ({key})", spec.icon, spec.name),
                };
                if ui.button(label).clicked() {
                    actions.push(cmd);
                    ui.close();
                }
            });
            ui.menu_button("show a drawing here (link)", |ui| {
                drawings.iter().for_each(|(id, frame)| {
                    if ui.button(format!("{id} (frame {frame})")).clicked() {
                        chosen = Some(id.clone());
                        ui.close();
                    }
                });
            });
            match &span {
                Some(span) => span_choice = tweens::span_menu(ui, span, span_keys, ask),
                None => {
                    ui.separator();
                    ui.small(
                        "shift click a cell (or shift drag over cells) to select frames for retiming and in-betweens",
                    );
                }
            }
        });
        if let Some(choice) = span_choice {
            self.span_action(choice)?;
        }
        self.queued.extend(actions);
        if let Some(source) = chosen {
            self.link_here(source)?;
        }
        Ok(())
    }

    pub(super) fn timeline_panel(&mut self, ui: &mut egui::Ui) -> Result<()> {
        self.tend_buffer();
        self.timeline_header(ui)?;
        if self.settings.layout.timeline_folded {
            return Ok(());
        }
        let rows = self.timeline_rows();
        let mut result = Ok(());
        egui::ScrollArea::vertical()
            .id_salt("timeline rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.vertical(|ui| {
                        if let Err(e) = self.names(ui) {
                            result = Err(e);
                        }
                    });
                    egui::ScrollArea::horizontal()
                        .id_salt("timeline frames")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if let Err(e) = self.grid(ui, &rows) {
                                result = Err(e);
                            }
                        });
                });
            });
        result
    }
}

fn paint_block(painter: &egui::Painter, block: Rect, cw: f32, fill: Color32, label: &str, faded: bool) {
    let fill = if faded { fill.gamma_multiply(0.35) } else { fill };
    painter.rect_filled(block, 3.0, fill);
    let grips = block.width() > 12.0;
    if grips {
        [3.0, 6.0].iter().for_each(|dx| {
            let x = block.right() - dx;
            painter.line_segment(
                [pos2(x, block.top() + 3.0), pos2(x, block.bottom() - 3.0)],
                Stroke::new(1.0, Color32::from_gray(210)),
            );
        });
    }
    let dot = pos2(block.left() - 1.0 + cw / 2.0, block.center().y);
    painter.circle_filled(dot, 3.5, if faded { Color32::from_gray(150) } else { Color32::WHITE });
    if block.width() > cw * 1.5 {
        let room = Rect::from_min_max(
            pos2(block.left() - 1.0 + cw, block.top()),
            pos2(block.right() - if grips { 9.0 } else { 2.0 }, block.bottom()),
        );
        painter.with_clip_rect(room.intersect(painter.clip_rect())).text(
            room.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::monospace(10.0),
            Color32::from_gray(if faded { 140 } else { 230 }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(cel: &str, start: u32, end: u32) -> Block {
        Block {
            cel: cel.into(),
            link: None,
            start,
            end,
            open: false,
            tween: false,
        }
    }

    #[test]
    fn a_press_on_the_timeline_starts_the_drag_its_place_asks_for() {
        let g = Geom {
            left: 100.0,
            top: 40.0,
            cell: 20.0,
            frames: 24,
            rows: 2,
        };
        let rows = vec![vec![block("c1", 0, 4), block("c2", 4, 6)], vec![block("c9", 2, 3)]];
        let row0 = 40.0 + RULER + ROW / 2.0;
        let row1 = row0 + ROW;
        assert_eq!(press_at(&g, &rows, pos2(130.0, 30.0)), None);
        assert_eq!(press_at(&g, &rows, pos2(130.0, 45.0)), Some(Press::Ruler(1)));
        assert_eq!(
            press_at(&g, &rows, pos2(105.0, row0)),
            Some(Press::Start(0, rows[0][0].clone()))
        );
        assert_eq!(press_at(&g, &rows, pos2(150.0, row0)), Some(Press::Cell(0, 2)));
        assert_eq!(
            press_at(&g, &rows, pos2(102.0, row0 - ROW / 2.0 + 2.0)),
            Some(Press::Cell(0, 0))
        );
        assert_eq!(press_at(&g, &rows, pos2(118.0, row0)), Some(Press::Cell(0, 0)));
        assert_eq!(
            press_at(&g, &rows, pos2(180.0 - 7.0, row0)),
            Some(Press::End(0, rows[0][0].clone()))
        );
        assert_eq!(
            press_at(&g, &rows, pos2(180.0 + 7.0, row0)),
            Some(Press::End(0, rows[0][0].clone()))
        );
        assert_eq!(
            press_at(&g, &rows, pos2(195.0, row0)),
            Some(Press::Start(0, rows[0][1].clone()))
        );
        assert_eq!(press_at(&g, &rows, pos2(400.0, row0)), Some(Press::Cell(0, 15)));
        assert_eq!(
            press_at(&g, &rows, pos2(165.0, row1)),
            Some(Press::End(1, rows[1][0].clone()))
        );
        assert_eq!(press_at(&g, &rows, pos2(130.0, row1 + ROW)), Some(Press::Ruler(1)));
        let origin = pos2(180.0 - 4.0, row0);
        let moved = origin + vec2(6.0, 0.0);
        assert!(matches!(press_at(&g, &rows, origin), Some(Press::End(..))));
        assert!(matches!(press_at(&g, &rows, moved), Some(Press::End(..))));
        let name = |r: usize| format!("l{r}");
        assert_eq!(drag_of(&Press::Ruler(3), false, name), Drag::Scrub);
        assert_eq!(drag_of(&Press::Ruler(3), true, name), Drag::Range { from: 3, to: 3 });
        assert!(matches!(
            drag_of(&Press::Cell(1, 5), false, name),
            Drag::Select { from: 5, first: 1, .. }
        ));
        assert!(matches!(drag_of(&Press::Cell(1, 5), true, name), Drag::Select { .. }));
        assert!(matches!(
            drag_of(&Press::Start(0, rows[0][1].clone()), false, name),
            Drag::Move { from: 4, .. }
        ));
        assert!(
            matches!(drag_of(&Press::End(0, rows[0][0].clone()), false, name), Drag::Hold { end: 4, ref layer, .. } if layer == "l0")
        );
    }
}
