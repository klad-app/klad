mod between;
mod bones;
mod brushwin;
mod buffer;
mod cells;
mod checks;
mod clips;
mod commands;
mod compare;
mod copy;
mod depth;
mod export;
mod first;
mod history;
mod keys;
mod layers;
mod look;
mod make;
mod options;
mod paintover;
mod panels;
mod pins;
mod prefs;
mod selection;
mod shift;
mod text;
mod timeline;
mod tweens;
#[cfg(test)]
pub(crate) use commands::install_job;
#[cfg(test)]
mod drag_time;
#[cfg(test)]
mod ui_tests;

use crate::color::{self, Hsva};
use crate::commands::Cmd;
use crate::composite::{Preview, Scene, Tiles};
use crate::frames::FrameCache;
use crate::gpu::{CanvasPaint, Outbox};
use crate::jobs::Jobs;
use crate::lift;
use crate::messages::{Messages, Tone};
use crate::pen::{self, Pen, PenEvent, Phase, device_key};
use crate::preview;
use crate::settings::{self, Settings};
use crate::stats::{Stats, lock};
use crate::sync::{Watch, watch};
use crate::theme::Hint;
use crate::view::View;
use anyhow::{Result, anyhow};
use egui::{Color32, Key, Pos2, Rect, Sense, Vec2, vec2};
use klad_brush::{
    Brush, BrushSet, PressureCurve, Raster, Raw, Recorder, StrokeBuffer, StrokePainter, flood, needs_repaint, polygon,
    stroke_blend,
};
use klad_core::brushes::{BrushDef, Curve, TipDef, pick_id};
use klad_core::bundle::now_ms;
use klad_core::color::parse_hex;
use klad_core::doc::{Item, Kind, Linear, Loc, MASK, Node, Paint, Radial, Shape, Stop};
use klad_core::fill::{self, Fill, FillRef, FillSource, Seed};
use klad_core::history::Stack;
use klad_core::markedit::{self, LassoAction, MarkTransform, address, apply_affine, invert};
use klad_core::notes::{self, Note};
use klad_core::ops::{LassoSpec, Op, apply_all};
use klad_core::pathedit::{self, PathEdit, Side, Subpath};
use klad_core::rng::{FNV_OFFSET, Rng, fnv};
use klad_core::stroke::{Mark, Stroke, encode_cel};
use klad_render::{overlay, vector};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HANDLE: f32 = 12.0;
const REFILL_MS: f64 = 8.0;
const FINISH_WAIT: Duration = Duration::from_secs(2);
const DRAG_SLOP: f32 = 3.0;
const KNOB: f32 = 30.0;
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn raw_of(e: &PenEvent, c: Vec2, dt_ms: f32) -> Raw {
    Raw {
        x: c.x,
        y: c.y,
        pressure: e.recorded_pressure(),
        tilt: e.tilt,
        dt_ms,
    }
}

fn to_canvas(m: [f32; 6], p: [f32; 2]) -> Vec2 {
    let (x, y) = apply_affine(m, (p[0], p[1]));
    vec2(x, y)
}

fn to_local(m: [f32; 6], c: Vec2) -> [f32; 2] {
    invert(m).map_or([c.x, c.y], |inverse| {
        let (x, y) = apply_affine(inverse, (c.x, c.y));
        [x, y]
    })
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Brush,
    Fill,
    Select,
    Strokes,
    Direct,
    Pen,
    Shape,
    Text,
    Note,
    Bone,
}

struct PenPath {
    anchors: Vec<pathedit::Anchor>,
    dragging: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Part {
    layer: String,
    cel: String,
    ids: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
struct Picked {
    cel: String,
    ids: Vec<u32>,
    cut: Option<Vec<[f32; 2]>>,
    more: Vec<Part>,
}

type PartMarks<'a> = (String, String, &'a [Mark], &'a [u32]);

impl Picked {
    fn whole(cel: String, ids: Vec<u32>) -> Self {
        Self {
            cel,
            ids,
            cut: None,
            more: Vec::new(),
        }
    }

    fn key(&self) -> String {
        let list = |cel: &str, ids: &[u32]| {
            let ids: Vec<String> = ids.iter().map(u32::to_string).collect();
            format!("{cel}:{}", ids.join(","))
        };
        let base = std::iter::once(list(&self.cel, &self.ids))
            .chain(self.more.iter().map(|p| list(&p.cel, &p.ids)))
            .collect::<Vec<_>>()
            .join(";");
        match &self.cut {
            Some(polygon) => {
                let bytes: Vec<u8> = polygon.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
                format!("{base}:cut{:x}", fnv(FNV_OFFSET, &bytes))
            }
            None => base,
        }
    }

    fn addresses(&self) -> Vec<String> {
        self.ids
            .iter()
            .map(|id| address(&self.cel, *id))
            .chain(
                self.more
                    .iter()
                    .flat_map(|p| p.ids.iter().map(|id| address(&p.cel, *id))),
            )
            .collect()
    }

    fn count(&self) -> usize {
        self.ids.len() + self.more.iter().map(|p| p.ids.len()).sum::<usize>()
    }
}

fn union_box(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
        (a, b) => a.or(b),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Direct {
    node: String,
    points: BTreeSet<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PointGrab {
    Anchors,
    Handle(usize, Side),
    Marquee { on_shape: bool },
}

#[derive(Clone, Copy, Debug)]
struct PointDrag {
    grab: PointGrab,
    start: Vec2,
    now: Vec2,
    free: bool,
}

struct PathCommit {
    job: Receiver<Result<(u64, Raster)>>,
    ops: Vec<Op>,
    cel: String,
    preview: Vec<Subpath>,
    node: String,
}

struct Shaped {
    node: String,
    rev: u64,
    subs: Vec<Subpath>,
    matrix: [f32; 6],
}

#[derive(Clone, Debug, PartialEq)]
struct FillTool {
    reference: FillRef,
    tolerance: f32,
    gap: f32,
    expand: f32,
    opacity: f32,
}

impl Default for FillTool {
    fn default() -> Self {
        Self {
            reference: FillRef::This,
            tolerance: fill::TOLERANCE,
            gap: 4.0,
            expand: fill::EXPAND,
            opacity: 1.0,
        }
    }
}

struct Lasso {
    points: Vec<Vec2>,
    start: Pos2,
    far: bool,
}

struct CutView {
    key: String,
    rev: u64,
    marks: Vec<Mark>,
    lassoed: Vec<u32>,
    chosen: Vec<u32>,
    more: Vec<PartView>,
}

struct PartView {
    layer: String,
    cel: String,
    marks: Vec<Mark>,
    lassoed: Vec<u32>,
    chosen: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grab {
    Move,
    Scale,
    Rotate,
    Stretch(usize),
}

fn grab(screen: [Pos2; 4], pos: Pos2, edges: bool) -> Option<Grab> {
    let reach = crate::theme::GRAB;
    let top = screen[0].lerp(screen[1], 0.5);
    let center = screen.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
    let knob = top + (top.to_vec2() - center).normalized() * KNOB;
    if pos.distance(knob) <= reach {
        return Some(Grab::Rotate);
    }
    if screen.iter().any(|c| pos.distance(*c) <= reach) {
        return Some(Grab::Scale);
    }
    let flat = screen.map(|p| p.to_vec2());
    if let Some(side) = (0..4).find(|&side| edges && pos.distance(edge(&flat, side).to_pos2()) <= reach) {
        return Some(Grab::Stretch(side));
    }
    let (min, max) = screen
        .iter()
        .fold((screen[0], screen[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    Rect::from_min_max(min, max).contains(pos).then_some(Grab::Move)
}

fn edge(corners: &[Vec2; 4], side: usize) -> Vec2 {
    (corners[side] + corners[(side + 1) % 4]) / 2.0
}

struct Drag {
    grab: Grab,
    node: String,
    strokes: bool,
    fresh: bool,
    copy: bool,
    start: Vec2,
    now: Vec2,
    center: Vec2,
    corners: [Vec2; 4],
    slop: f32,
    crossed: bool,
}

impl Drag {
    fn to(&mut self, at: Vec2) {
        self.now = at;
        self.crossed |= (self.now - self.start).length() > self.slop;
    }

    fn scale(&self) -> f32 {
        if self.grab != Grab::Scale {
            return 1.0;
        }
        ((self.now - self.center).length() / (self.start - self.center).length().max(1.0)).clamp(0.02, 50.0)
    }

    fn angle(&self) -> f32 {
        if self.grab != Grab::Rotate {
            return 0.0;
        }
        (self.now - self.center).angle() - (self.start - self.center).angle()
    }

    fn stretch(&self) -> Option<(usize, Vec2, f32)> {
        let Grab::Stretch(side) = self.grab else {
            return None;
        };
        let pivot = edge(&self.corners, (side + 2) % 4);
        let axis = if side % 2 == 0 { 1 } else { 0 };
        let span = self.start[axis] - pivot[axis];
        let k = if span.abs() < 1.0 {
            1.0
        } else {
            (self.now[axis] - pivot[axis]) / span
        };
        let k = if k.abs() < 0.02 {
            0.02f32.copysign(k)
        } else {
            k.clamp(-50.0, 50.0)
        };
        Some((axis, pivot, k))
    }

    fn affine(&self) -> [f32; 6] {
        let c = self.center;
        if let Some((axis, pivot, k)) = self.stretch() {
            return match axis {
                0 => [k, 0.0, 0.0, 1.0, pivot.x * (1.0 - k), 0.0],
                _ => [1.0, 0.0, 0.0, k, 0.0, pivot.y * (1.0 - k)],
            };
        }
        match self.grab {
            Grab::Move => {
                let d = self.now - self.start;
                [1.0, 0.0, 0.0, 1.0, d.x, d.y]
            }
            Grab::Scale => {
                let k = self.scale();
                [k, 0.0, 0.0, k, c.x * (1.0 - k), c.y * (1.0 - k)]
            }
            Grab::Rotate => {
                let (s, k) = self.angle().sin_cos();
                [k, -s, s, k, c.x - k * c.x + s * c.y, c.y - s * c.x - k * c.y]
            }
            Grab::Stretch(_) => IDENTITY,
        }
    }

    fn preview(&self) -> [Vec2; 4] {
        let a = self.affine();
        self.corners.map(|c| {
            let (x, y) = lift::affine_apply(a, (c.x, c.y));
            vec2(x, y)
        })
    }

    fn moved(&self) -> bool {
        self.crossed
    }

    fn mark_transform(&self) -> MarkTransform {
        let pivot = Some([self.center.x, self.center.y]);
        if let Some((axis, at, k)) = self.stretch() {
            return MarkTransform {
                sx: if axis == 0 { k } else { 1.0 },
                sy: if axis == 1 { k } else { 1.0 },
                pivot: Some([at.x, at.y]),
                ..Default::default()
            };
        }
        match self.grab {
            Grab::Move => {
                let d = self.now - self.start;
                MarkTransform {
                    x: d.x,
                    y: d.y,
                    ..Default::default()
                }
            }
            Grab::Scale => MarkTransform {
                sx: self.scale(),
                sy: self.scale(),
                pivot,
                ..Default::default()
            },
            Grab::Rotate => MarkTransform {
                rotate: self.angle().to_degrees(),
                pivot,
                ..Default::default()
            },
            Grab::Stretch(_) => MarkTransform::default(),
        }
    }
}

struct Finish {
    ops: Vec<Op>,
    rev: u64,
    dirty: BTreeSet<usize>,
    since: Instant,
}

struct PreviewState {
    snap: Arc<preview::Snapshot>,
    pending: BTreeSet<usize>,
    cancel: Arc<AtomicBool>,
    asked: Option<Vec<Op>>,
    asked_at: Option<Vec2>,
    job: Option<Receiver<Result<preview::Done>>>,
    shown: BTreeSet<usize>,
    done: Option<(Vec<Op>, bool, BTreeSet<usize>)>,
    finish: Option<Finish>,
    posed: Option<(u64, u32, String, klad_core::rig::Pose)>,
}

impl PreviewState {
    fn release(&mut self) -> Vec<usize> {
        self.cancel.store(true, AtomicOrdering::Relaxed);
        std::mem::take(&mut self.shown)
            .into_iter()
            .chain(self.finish.take().into_iter().flat_map(|f| f.dirty))
            .collect()
    }
}

struct Live {
    stroke: Stroke,
    painter: StrokePainter,
    brush: Brush,
    recorder: Recorder,
    last_at: Instant,
    twin: Option<(f32, Stroke, StrokePainter)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tuning {
    hardness: Option<f32>,
    size_min: f32,
    size_gamma: f32,
    taper: [f32; 2],
    stabilizer: f32,
    smoothing: f32,
}

impl Tuning {
    fn of(def: &BrushDef) -> Self {
        let curve = def.dynamics.size.pressure.unwrap_or(Curve {
            min: 1.0,
            max: 1.0,
            gamma: 1.0,
        });
        Self {
            hardness: def.hardness(),
            size_min: curve.min,
            size_gamma: curve.gamma,
            taper: def.taper,
            stabilizer: def.stabilizer,
            smoothing: def.smoothing,
        }
    }

    fn shape(&self, base: &BrushDef) -> BrushDef {
        let mut def = base.clone();
        if let (Some(hardness), TipDef::Round { .. }) = (self.hardness, &def.tip) {
            def.tip = TipDef::Round { hardness };
        }
        let old = Tuning::of(base);
        if (self.size_min, self.size_gamma) != (old.size_min, old.size_gamma) {
            def.dynamics.size.pressure = Some(Curve {
                min: self.size_min,
                max: 1.0,
                gamma: self.size_gamma,
            });
        }
        def
    }
}

#[derive(Default)]
struct Readout {
    pressure: f32,
    device: u64,
    tilt: (f32, f32),
    eraser: bool,
    tablet: bool,
    samples: VecDeque<Instant>,
}

#[derive(Default)]
struct Windows {
    settings: bool,
    brush: bool,
    log: bool,
    onion: bool,
    link: bool,
    clip: bool,
    between: bool,
    import: bool,
    about: bool,
    export: bool,
    report: bool,
    font: bool,
    paintover: bool,
    compare: bool,
}

#[derive(Default)]
struct PaletteState {
    query: String,
    selected: usize,
    opened: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    New,
    Open,
    Recent,
    Close,
    Path(std::path::PathBuf),
}

pub(crate) struct Editor {
    pub(crate) scene: Scene,
    brushes: BrushSet,
    embedded: BTreeMap<String, BrushDef>,
    buffer: StrokeBuffer,
    live: Option<Live>,
    tool: Tool,
    brush: String,
    previous_brush: String,
    tuning: Tuning,
    size: f32,
    color: [f32; 4],
    hsva: Hsva,
    swatch_name: String,
    fill: FillTool,
    lasso: Option<Lasso>,
    modifiers: egui::Modifiers,
    opacity: f32,
    flow: f32,
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) settings_dirty: bool,
    pub(crate) update: Option<crate::updates::Release>,
    pub(crate) update_feed: String,
    update_check: Option<std::sync::mpsc::Receiver<crate::updates::Checked>>,
    update_said: Option<String>,
    pub(crate) wants: Option<Want>,
    view: Option<View>,
    canvas_rect: Rect,
    navigating: bool,
    focused: bool,
    pen: Option<Pen>,
    readout: Readout,
    dirty: BTreeSet<usize>,
    pending: Option<Instant>,
    stats: Arc<Mutex<Stats>>,
    watch: Option<Watch>,
    external: Option<u64>,
    rng: Rng,
    spin: bool,
    messages: Messages,
    measured: BTreeMap<&'static str, String>,
    pointer: Option<Pos2>,
    jobs: Jobs,
    windows: Windows,
    palette: Option<PaletteState>,
    queued: Vec<Cmd>,
    tabs: Vec<bool>,
    themed: bool,
    key_shift: bool,
    held: selection::Held,
    sizing: Option<(f32, f32, Pos2)>,
    second: [f32; 4],
    samples: options::Samples,
    swatch_drag: Option<usize>,
    renaming: Option<layers::Rename>,
    queued_activate: Option<Vec<usize>>,
    solo_later: Option<String>,
    layer_drag: Option<(layers::LayerRow, layers::Where)>,
    canvas_wait: Option<timeline::CanvasWait>,
    canvas_gen: u64,
    buffering: buffer::BufferState,
    custom: Option<BrushDef>,
    brush_window: brushwin::BrushWindow,
    look: look::LookState,
    shapes: make::ShapeTool,
    text_tool: text::TextTool,
    text_edit: Option<text::Typing>,
    picker: text::Picker,
    mirror: Option<f32>,
    mirror_buffer: StrokeBuffer,
    typing: BTreeMap<String, String>,
    import_path: String,
    depth: depth::Parallax,
    batch: usize,
    pen_path: Option<PenPath>,
    pen_fill: bool,
    pen_stroke: bool,
    pen_width: f32,
    thumbs: layers::Thumbs,
    between: between::Between,
    checks: checks::Checks,
    export: export::Form,
    paintover: paintover::Form,
    compare: Option<compare::Compared>,
    bones: bones::BoneTool,
    shifting: Option<shift::Shifting>,
    first: first::FirstSteps,
    settings_section: prefs::Section,
    report_drawing: bool,
    #[cfg(test)]
    pub(crate) opened: Vec<String>,
    soloed: Option<(String, Vec<(String, bool)>)>,
    strip_tools: [f32; 2],
    strip_foot: [f32; 2],
    key_ease: klad_core::keys::Ease,
    bindings: crate::commands::Bindings,
    capturing: Option<Cmd>,
    keys_busy: bool,
    key_filter: String,
    selection: Option<String>,
    drag: Option<Drag>,
    edits: BTreeMap<String, f32>,
    texts: BTreeMap<String, String>,
    pins: Vec<Note>,
    note_at: Option<Vec2>,
    note_text: String,
    note_open: Option<u32>,
    note_edit: String,
    history: Option<(u64, Vec<history::Row>)>,
    shared: (Option<String>, Vec<String>, u32),
    ctx: egui::Context,
    sel_box: Option<(String, u64, [Vec2; 4])>,
    preview: Option<PreviewState>,
    outbox: Arc<Mutex<Outbox>>,
    zoom_px: f32,
    picked: Option<Picked>,
    also: Vec<String>,
    direct: Option<Direct>,
    shaped: Option<Shaped>,
    point_drag: Option<PointDrag>,
    path_commit: Option<PathCommit>,
    simplify_px: f32,
    under: Vec<String>,
    cut_lasso: bool,
    cut_view: Option<CutView>,
    timeline: timeline::Timeline,
    frames: FrameCache,
    clipboard: Option<copy::Copied>,
}

fn hex_of(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

fn color_of(hex: &str) -> Option<Color32> {
    let [r, g, b, a] = parse_hex(hex).ok()?.map(|v| (v * 255.0).round() as u8);
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

impl Editor {
    pub(crate) fn open(ctx: egui::Context, path: &Path) -> Result<Self> {
        let start = Instant::now();
        let scene = Scene::open_lazy(path)?;
        let brushes = BrushSet::for_bundle(&scene.bundle)?;
        let embedded = scene.bundle.brushes()?;
        let tuning = Tuning::of(
            brushes
                .def("ink")
                .ok_or_else(|| crate::report::bug("the ink brush is missing from the library"))?,
        );
        let settings_path = settings::default_path();
        let settings = settings_path.as_deref().map(Settings::load).unwrap_or_default();
        let open = format!(
            "{} layers, opened in {:.0} ms",
            scene.bundle.doc.all_layers().len(),
            ms(start)
        );
        let size = scene.size();
        let watch = watch(path, ctx.clone());
        let mut messages = Messages::default();
        if let Err(e) = &watch {
            messages.say(
                Tone::Error,
                format!("not watching the bundle, so agent edits will not show: {e}"),
                Instant::now(),
            );
        }
        let measured = BTreeMap::from([("open", open)]);
        let pins = notes::read(&scene.bundle);
        let repaint = ctx.clone();
        let mut frames = FrameCache::new(Arc::new(move || repaint.request_repaint()));
        frames.disk = Some(scene.pictures());
        let mut editor = Self {
            dirty: scene.all_tiles().into_iter().collect(),
            buffer: StrokeBuffer::new(size.0, size.1),
            scene,
            brushes,
            embedded,
            live: None,
            tool: Tool::Brush,
            brush: "ink".into(),
            previous_brush: "ink".into(),
            tuning,
            size: 12.0,
            color: [0.08, 0.08, 0.1, 1.0],
            hsva: Hsva::from_rgba([0.08, 0.08, 0.1, 1.0], 0.0),
            swatch_name: String::new(),
            fill: FillTool::default(),
            lasso: None,
            modifiers: egui::Modifiers::default(),
            opacity: 1.0,
            flow: 1.0,
            settings,
            settings_path,
            settings_dirty: false,
            update: None,
            update_feed: crate::updates::feed(),
            update_check: None,
            update_said: None,
            wants: None,
            view: None,
            canvas_rect: Rect::NOTHING,
            navigating: false,
            focused: false,
            pen: Pen::install(),
            readout: Readout::default(),
            pending: None,
            stats: Arc::default(),
            watch: watch.ok(),
            external: None,
            rng: Rng::new(now_ms()),
            spin: false,
            messages,
            measured,
            pointer: None,
            jobs: Jobs::default(),
            windows: Windows::default(),
            palette: None,
            queued: Vec::new(),
            tabs: Vec::new(),
            themed: false,
            key_shift: false,
            held: selection::Held::default(),
            sizing: None,
            second: [1.0, 1.0, 1.0, 1.0],
            samples: options::Samples::default(),
            swatch_drag: None,
            renaming: None,
            queued_activate: None,
            solo_later: None,
            layer_drag: None,
            canvas_wait: None,
            canvas_gen: 0,
            buffering: buffer::BufferState::default(),
            custom: None,
            brush_window: brushwin::BrushWindow::default(),
            look: look::LookState::default(),
            shapes: make::ShapeTool::default(),
            text_tool: text::TextTool::default(),
            text_edit: None,
            picker: text::Picker::default(),
            mirror: None,
            mirror_buffer: StrokeBuffer::new(size.0, size.1),
            typing: BTreeMap::new(),
            import_path: String::new(),
            depth: depth::Parallax::default(),
            batch: timeline::FILL,
            pen_path: None,
            pen_fill: true,
            pen_stroke: true,
            pen_width: 3.0,
            thumbs: layers::Thumbs::default(),
            between: between::Between::default(),
            checks: checks::Checks::default(),
            export: export::Form::default(),
            paintover: paintover::Form::default(),
            compare: None,
            bones: bones::BoneTool::default(),
            shifting: None,
            first: first::FirstSteps::default(),
            settings_section: prefs::Section::default(),
            report_drawing: false,
            #[cfg(test)]
            opened: Vec::new(),
            soloed: None,
            strip_tools: [570.0, 480.0],
            strip_foot: [186.0, 166.0],
            key_ease: klad_core::keys::Ease::Linear,
            bindings: crate::commands::Bindings::default(),
            capturing: None,
            keys_busy: false,
            key_filter: String::new(),
            selection: None,
            drag: None,
            edits: BTreeMap::new(),
            texts: BTreeMap::new(),
            pins,
            note_at: None,
            note_text: String::new(),
            note_open: None,
            note_edit: String::new(),
            history: None,
            shared: (None, Vec::new(), 0),
            ctx,
            sel_box: None,
            preview: None,
            outbox: Arc::default(),
            zoom_px: 1.0,
            picked: None,
            also: Vec::new(),
            direct: None,
            shaped: None,
            point_drag: None,
            path_commit: None,
            simplify_px: 1.5,
            under: Vec::new(),
            cut_lasso: true,
            cut_view: None,
            clipboard: None,
            timeline: timeline::Timeline::default(),
            frames,
        };
        editor.use_defaults();
        editor.request_canvas();
        editor.tend_canvas(timeline::OPEN_WAIT)?;
        Ok(editor)
    }

    pub(super) fn use_defaults(&mut self) {
        let d = self.settings.defaults.clone();
        if let Some(brush) = d.brush.filter(|b| self.brushes.def(b).is_some()) {
            self.brush = brush;
        }
        if let Some(size) = d.size {
            self.size = size.clamp(0.5, 500.0);
        }
        self.timeline.on = d.exposure.clamp(1, 8);
        self.scene.onion.before = d.onion_before;
        self.scene.onion.after = d.onion_after;
        self.scene.onion.opacity = d.onion_opacity.clamp(0.05, 1.0);
        self.bindings = crate::commands::Bindings::from_settings(&self.settings.keys);
    }

    pub(super) fn capture_key(&mut self, ui: &egui::Ui) {
        let Some(cmd) = self.capturing else {
            return;
        };
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        ui.ctx().input_mut(|i| i.consume_key(modifiers, key));
        self.capturing = None;
        if key == egui::Key::Escape {
            self.say(Tone::Info, "the key stays as it was");
            return;
        }
        let keys = crate::commands::Keys::pressed(key, modifiers);
        match self.bindings.bind(cmd, keys) {
            Ok(()) => {
                self.settings.keys = self.bindings.to_settings();
                self.settings_dirty = true;
                self.say(Tone::Done, format!("{} is now {}", cmd.spec().name, keys.label()));
            }
            Err(why) => self.say(Tone::Refused, why),
        }
    }

    fn canvas_size(&self) -> Vec2 {
        let (w, h) = self.scene.size();
        vec2(w as f32, h as f32)
    }

    pub(crate) fn say(&mut self, tone: Tone, text: impl Into<String>) {
        let text = text.into();
        if tone == Tone::Error {
            crate::report::log(&format!("editor: {text}"));
        }
        self.messages.say(tone, text, Instant::now());
        self.ctx.request_repaint();
    }

    pub(crate) fn error(&mut self, e: impl std::fmt::Display) {
        self.say(Tone::Error, format!("{e:#}"));
    }

    pub(crate) fn failed(&mut self, e: anyhow::Error) {
        match crate::report::is_bug(&e) {
            true => self.say(Tone::Error, crate::report::bug_message(&e)),
            false => self.error(e),
        }
    }

    fn measure(&mut self, key: &'static str, text: String) {
        self.measured.insert(key, text);
    }

    fn share_selection(&mut self) {
        let items: Vec<String> = self
            .selection
            .iter()
            .chain(self.direct.as_ref().map(|d| &d.node))
            .cloned()
            .chain(self.picked.iter().flat_map(Picked::addresses))
            .collect();
        let now = (self.scene.active_id(), items, self.scene.frame);
        if now != self.shared {
            let nodes = now.1.clone();
            if let Err(e) = notes::write_selection(&self.scene.bundle, now.0.clone(), nodes, self.scene.frame) {
                self.failed(e.context("the selection could not be shared with the agent"));
            }
            self.shared = now;
        }
    }

    fn sync(&mut self) -> Result<()> {
        if let Some(first) = self.watch.as_ref().and_then(|w| w.events.try_iter().min()) {
            self.external.get_or_insert(first);
            self.pins = notes::read(&self.scene.bundle);
        }
        let committing = self.path_commit.is_some();
        if self.live.is_some()
            || self.drag.is_some()
            || self.point_drag.is_some()
            || committing
            || self.external.is_none()
        {
            return Ok(());
        }
        if !self.scene.bundle.changed_on_disk() {
            self.external = None;
            return Ok(());
        }
        self.reload()
    }

    fn reload(&mut self) -> Result<()> {
        let seen = self.external.take();
        self.forget_typing_preview();
        self.checks.gaps = None;
        self.checks.arcs = None;
        let start = Instant::now();
        self.brushes.refresh(&self.scene.bundle)?;
        self.embedded = self.scene.bundle.brushes()?;
        let dirty = self.scene.reload(&self.brushes)?;
        let (w, h) = self.scene.size();
        if (self.buffer.grid().width, self.buffer.grid().height) != (w, h) {
            self.buffer = StrokeBuffer::new(w, h);
            self.mirror_buffer = StrokeBuffer::new(w, h);
            self.view = None;
        }
        let tiles = dirty.len();
        self.dirty.extend(dirty);
        self.forget_missing_selection();
        let took = ms(start);
        let text = match self.scene.bundle.read_stamp() {
            Some(s) => format!(
                "rev {} by {}\nfile event after {} ms\n{tiles} tiles refreshed after {} ms ({took:.0} ms to reload)",
                s.rev,
                s.author,
                seen.map_or(-1, |t| t as i64 - s.at_ms as i64),
                now_ms() as i64 - s.at_ms as i64
            ),
            None => format!("{tiles} tiles refreshed in {took:.0} ms"),
        };
        self.measure("sync", text);
        Ok(())
    }

    fn forget_missing_selection(&mut self) {
        let doc = &self.scene.bundle.doc;
        if self
            .selection
            .as_ref()
            .is_some_and(|id| !matches!(doc.item(id), Some(Item::Node(..))))
        {
            self.selection = None;
        }
        let gone = |id: &str| match markedit::parse_address(id) {
            Some((cel, mark)) => {
                self.scene.active_cel().as_deref() != Some(cel)
                    || !self.scene.active_marks().iter().any(|m| m.id() == mark)
            }
            None => !matches!(doc.item(id), Some(Item::Node(..))),
        };
        if self.direct.as_ref().is_some_and(|d| gone(&d.node)) {
            self.direct = None;
        }
        let active = self.scene.active_cel();
        let active_id = self.scene.active_id();
        let present: BTreeSet<u32> = self.scene.active_marks().iter().map(Mark::id).collect();
        let frame = self.scene.frame;
        let scene = &self.scene;
        let shown = |layer: &str| {
            scene
                .bundle
                .doc
                .layer(layer)
                .and_then(|l| l.content_at(frame))
                .map(|c| c.id.clone())
        };
        self.picked = self.picked.take().and_then(|mut p| {
            p.ids.retain(|id| present.contains(id));
            let cut = p.cut.is_some();
            p.more.retain_mut(|part| {
                if shown(&part.layer).as_deref() != Some(part.cel.as_str()) {
                    return false;
                }
                let have: BTreeSet<u32> = scene.marks_of(&part.cel).iter().map(Mark::id).collect();
                part.ids.retain(|id| have.contains(id));
                !part.ids.is_empty() || cut
            });
            (Some(&p.cel) == active.as_ref() && (!p.ids.is_empty() || cut || !p.more.is_empty())).then_some(p)
        });
        self.also.retain(|id| {
            Some(id) != active_id.as_ref() && scene.bundle.doc.layer(id).is_some_and(|l| l.kind() == Kind::Paint)
        });
    }

    fn apply(&mut self, ops: Vec<Op>) -> Result<()> {
        let dirty = self.commit(ops)?;
        self.dirty.extend(dirty);
        Ok(())
    }

    fn commit(&mut self, ops: Vec<Op>) -> Result<Vec<usize>> {
        self.checks.gaps = None;
        self.checks.arcs = None;
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        self.end_look_preview()?;
        self.end_typing_preview()?;
        let start = Instant::now();
        let dirty = self.scene.apply(&ops, &self.brushes)?;
        self.measure(
            "edit",
            format!("edit applied, {} tiles refreshed in {:.0} ms", dirty.len(), ms(start)),
        );
        self.forget_missing_selection();
        Ok(dirty)
    }

    fn set(&mut self, id: &str, props: Vec<(String, Value)>) -> Result<()> {
        if props.is_empty() {
            return Ok(());
        }
        self.apply(vec![Op::Set {
            id: id.to_string(),
            props: props.into_iter().collect::<Map<String, Value>>(),
        }])
    }

    fn step(&mut self, stack: Stack) -> Result<()> {
        if self.live.is_some() || self.drag.is_some() || self.point_drag.is_some() || self.path_commit.is_some() {
            return Ok(());
        }
        let start = Instant::now();
        let word = if stack == Stack::Undo { "undid" } else { "redid" };
        match self.scene.step(stack, &self.brushes)? {
            Some((summary, dirty)) => {
                self.measure("undo", format!("{word} {summary} in {:.0} ms", ms(start)));
                self.say(Tone::Info, format!("{word} {}", self.plain(&summary)));
                self.dirty.extend(dirty);
                self.forget_missing_selection();
            }
            None => self.say(
                Tone::Refused,
                format!("nothing to {}", if stack == Stack::Undo { "undo" } else { "redo" }),
            ),
        }
        Ok(())
    }

    pub(super) fn plain(&self, text: &str) -> String {
        let doc = &self.scene.bundle.doc;
        let drawing = |cel: &str| match doc.locate(cel) {
            Some(Loc::Cel(path, c)) => Some(format!(
                "the drawing at frame {} on {}",
                doc.layer_at(&path).cels()[c].frame,
                doc.layer_at(&path).name
            )),
            _ => None,
        };
        let word = |w: &str| -> Option<String> {
            if let Some((cel, id)) = klad_core::markedit::parse_address(w) {
                return Some(drawing(cel).map_or(format!("mark {id}"), |d| format!("mark {id} of {d}")));
            }
            let digits = w.len() > 1 && w[1..].chars().all(|c| c.is_ascii_digit());
            match w.chars().next() {
                Some('c') if digits => drawing(w),
                Some('l') if digits => doc.layer(w).map(|l| l.name.clone()),
                Some('n') if digits => match doc.item(w) {
                    Some(Item::Node(_, _, node)) => Some(format!("a {}", node.type_name())),
                    _ => None,
                },
                _ => None,
            }
        };
        text.split(' ')
            .map(|w| {
                let bare = w.trim_end_matches([',', ')', ':', '.', ';']);
                let lead = bare.trim_start_matches('(');
                let (head, tail) = (&bare[..bare.len() - lead.len()], &w[bare.len()..]);
                word(lead).map_or(w.to_string(), |p| format!("{head}{p}{tail}"))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn refresh_box(&mut self) {
        let rev = self.scene.bundle.doc.rev;
        let Some(id) = self.selection.clone() else {
            self.sel_box = None;
            return;
        };
        if self.sel_box.as_ref().is_some_and(|(s, r, _)| *s == id && *r == rev) {
            return;
        }
        self.sel_box = overlay::node_world_box(&self.scene.bundle.doc, &id)
            .ok()
            .flatten()
            .map(|[x, y, w, h]| {
                (
                    id,
                    rev,
                    [vec2(x, y), vec2(x + w, y), vec2(x + w, y + h), vec2(x, y + h)],
                )
            });
    }

    fn selection_corners(&self) -> Option<(String, [Vec2; 4])> {
        let (id, _, corners) = self.sel_box.as_ref()?;
        (self.selection.as_ref() == Some(id)).then(|| (id.clone(), *corners))
    }

    fn refresh_cut(&mut self) {
        let rev = self.scene.bundle.doc.rev;
        let Some((key, cel, polygon, ids, more)) = self.picked.as_ref().and_then(|p| {
            p.cut
                .clone()
                .map(|polygon| (p.key(), p.cel.clone(), polygon, p.ids.clone(), p.more.clone()))
        }) else {
            self.cut_view = None;
            return;
        };
        if self.cut_view.as_ref().is_some_and(|v| v.key == key && v.rev == rev) {
            return;
        }
        let doc = &self.scene.bundle.doc;
        let split = |cel: &str, source: &[Mark], ids: Vec<u32>| {
            let mut marks = source.to_vec();
            let lassoed = markedit::split(&mut marks, &polygon, true, doc.mark_ids(cel));
            let chosen: Vec<u32> = lassoed
                .iter()
                .copied()
                .chain(ids.into_iter().filter(|id| !lassoed.contains(id)))
                .collect();
            (marks, lassoed, chosen)
        };
        let (marks, lassoed, chosen) = split(&cel, self.scene.active_marks(), ids);
        let more = more
            .into_iter()
            .map(|p| {
                let (marks, lassoed, chosen) = split(&p.cel, self.scene.marks_of(&p.cel), p.ids);
                PartView {
                    layer: p.layer,
                    cel: p.cel,
                    marks,
                    lassoed,
                    chosen,
                }
            })
            .collect();
        self.cut_view = Some(CutView {
            key,
            rev,
            marks,
            lassoed,
            chosen,
            more,
        });
    }

    fn picked_parts(&self) -> Vec<PartMarks<'_>> {
        let Some(picked) = self.picked.as_ref() else {
            return Vec::new();
        };
        let view = self
            .cut_view
            .as_ref()
            .filter(|v| picked.cut.is_some() && v.key == picked.key());
        if picked.cut.is_some() && view.is_none() {
            return Vec::new();
        }
        let active = self.scene.active_id().unwrap_or_default();
        let first: PartMarks = match view {
            Some(v) => (active, picked.cel.clone(), &v.marks[..], &v.chosen[..]),
            None => (active, picked.cel.clone(), self.scene.active_marks(), &picked.ids[..]),
        };
        std::iter::once(first)
            .chain(
                picked
                    .more
                    .iter()
                    .enumerate()
                    .map(|(k, p)| match view.and_then(|v| v.more.get(k)) {
                        Some(pv) => (p.layer.clone(), p.cel.clone(), &pv.marks[..], &pv.chosen[..]),
                        None => (p.layer.clone(), p.cel.clone(), self.scene.marks_of(&p.cel), &p.ids[..]),
                    }),
            )
            .collect()
    }

    fn picked_corners(&self) -> Option<(String, [Vec2; 4])> {
        let picked = self.picked.as_ref()?;
        let [x0, y0, x1, y1] = self
            .picked_parts()
            .iter()
            .map(|(_, _, marks, ids)| markedit::bounds_of(marks, ids))
            .fold(None, union_box)?;
        Some((picked.key(), [vec2(x0, y0), vec2(x1, y0), vec2(x1, y1), vec2(x0, y1)]))
    }

    fn across(&self, action: LassoAction) -> LassoAction {
        let spans = self.picked.as_ref().is_some_and(|p| !p.more.is_empty());
        match action {
            LassoAction::Transform(t) if spans && t.pivot.is_none() => {
                let pivot = self
                    .picked_corners()
                    .map(|(_, c)| [(c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0]);
                LassoAction::Transform(MarkTransform { pivot, ..t })
            }
            other => other,
        }
    }

    fn picked_ops(&self, action: LassoAction) -> Option<(Vec<Op>, Option<Picked>)> {
        let picked = self.picked.clone()?;
        let action = self.across(action);
        let Some(polygon) = picked.cut.clone() else {
            return Some(match action {
                LassoAction::Transform(transform) => (
                    vec![Op::Transform {
                        ids: picked.addresses(),
                        transform,
                    }],
                    Some(picked),
                ),
                LassoAction::Remove => (
                    vec![Op::Remove {
                        ids: picked.addresses(),
                    }],
                    None,
                ),
            });
        };
        let view = self.cut_view.as_ref().filter(|v| v.key == picked.key())?;
        let layer = self.scene.active_id()?;
        let frame = self.scene.frame;
        let views: Vec<(String, &str, &[u32], &[u32])> =
            std::iter::once((layer, picked.cel.as_str(), &view.lassoed[..], &view.chosen[..]))
                .chain(
                    view.more
                        .iter()
                        .map(|v| (v.layer.clone(), v.cel.as_str(), &v.lassoed[..], &v.chosen[..])),
                )
                .collect();
        let lassos: Vec<Op> = views
            .iter()
            .filter(|(_, _, lassoed, _)| !lassoed.is_empty())
            .map(|(layer, ..)| {
                Op::Lasso(LassoSpec {
                    layer: layer.clone(),
                    frame,
                    polygon: polygon.clone(),
                    cut: true,
                    action: action.clone(),
                })
            })
            .collect();
        let extras: Vec<String> = views
            .iter()
            .flat_map(|(_, cel, lassoed, chosen)| {
                chosen
                    .iter()
                    .filter(|id| !lassoed.contains(id))
                    .map(|id| address(cel, *id))
            })
            .collect();
        let rest = (!extras.is_empty()).then_some(match &action {
            LassoAction::Transform(transform) => Op::Transform {
                ids: extras,
                transform: *transform,
            },
            LassoAction::Remove => Op::Remove { ids: extras },
        });
        let after = match action {
            LassoAction::Remove => None,
            LassoAction::Transform(_) => Some(Picked {
                cel: picked.cel.clone(),
                ids: view.chosen.clone(),
                cut: None,
                more: view
                    .more
                    .iter()
                    .map(|v| Part {
                        layer: v.layer.clone(),
                        cel: v.cel.clone(),
                        ids: v.chosen.clone(),
                    })
                    .collect(),
            }),
        };
        Some((lassos.into_iter().chain(rest).collect(), after))
    }

    fn edit_picked(&mut self, action: LassoAction) -> Result<()> {
        let Some((ops, after)) = self.picked_ops(action) else {
            return Ok(());
        };
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        Ok(())
    }

    fn on_marks(&self) -> bool {
        match self.tool {
            Tool::Strokes => true,
            Tool::Select => self.selection.is_none() && self.picked.is_some(),
            _ => false,
        }
    }

    fn handles(&self) -> Option<(String, [Vec2; 4])> {
        if let Some(found) = self.shift_handles() {
            return Some(found);
        }
        match self.tool {
            Tool::Direct => None,
            _ if self.on_marks() => self.picked_corners(),
            _ => self.selection_corners(),
        }
    }

    fn node_ops(&self, drag: &Drag) -> Result<Option<Vec<Op>>> {
        let node = match self.scene.bundle.doc.item(&drag.node) {
            Some(Item::Node(_, _, node)) => node.clone(),
            _ => return Ok(None),
        };
        let affine = drag.affine();
        let t = match drag.stretch() {
            Some((axis, _, k)) => lift::stretched(&node, affine, axis, k)?,
            None => lift::committed(&node, affine, drag.scale(), drag.angle().to_degrees())?,
        };
        let props: Map<String, Value> = [
            ("transform.x", json!(t.x)),
            ("transform.y", json!(t.y)),
            ("transform.rotate", json!(t.rotate)),
            ("transform.sx", json!(t.sx)),
            ("transform.sy", json!(t.sy)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        Ok(Some(vec![Op::Set {
            id: drag.node.clone(),
            props,
        }]))
    }

    fn drag_ops(&self) -> Option<Vec<Op>> {
        let drag = self.drag.as_ref().filter(|d| d.moved() && !d.copy)?;
        if self.shifting.is_some() {
            return self.shift_ops(drag, false).ok().flatten();
        }
        match drag.strokes {
            true => self
                .picked_ops(LassoAction::Transform(drag.mark_transform()))
                .map(|(ops, _)| ops),
            false => self.node_ops(drag).ok().flatten(),
        }
    }

    fn preview_cels(&self) -> Vec<String> {
        if self.shifting.is_some() {
            return self.shift_cels();
        }
        match self.drag.as_ref().is_some_and(|d| d.strokes) {
            true => self
                .picked
                .iter()
                .flat_map(|p| std::iter::once(p.cel.clone()).chain(p.more.iter().map(|m| m.cel.clone())))
                .collect(),
            false => Vec::new(),
        }
    }

    fn drop_preview(&mut self) {
        if let Some(mut state) = self.preview.take() {
            self.dirty.extend(state.release());
        }
    }

    fn settle(&mut self, pending: &BTreeSet<usize>, settled: &BTreeSet<usize>, rest: Vec<usize>) {
        self.dirty.retain(|i| !(settled.contains(i) && pending.contains(i)));
        self.dirty.extend(rest.into_iter().filter(|i| !settled.contains(i)));
    }

    fn finish_preview(&mut self, ops: Vec<Op>, dirty: Vec<usize>, before: u64) {
        let Some(mut state) = self.preview.take() else {
            self.dirty.extend(dirty);
            return;
        };
        if state.snap.rev != before {
            self.dirty.extend(dirty.into_iter().chain(state.release()));
            return;
        }
        match state.done.take() {
            Some((done, exact, composed)) if state.job.is_none() && done == ops => {
                let settled = match exact && !self.scene.uses_caches() {
                    true => composed,
                    false => BTreeSet::new(),
                };
                let pending = std::mem::take(&mut state.pending);
                let rest = dirty.into_iter().chain(state.release()).collect();
                self.settle(&pending, &settled, rest);
            }
            Some((_, false, _)) => self.dirty.extend(dirty.into_iter().chain(state.release())),
            done => {
                state.done = done;
                state.finish = Some(Finish {
                    ops,
                    rev: self.scene.bundle.doc.rev,
                    dirty: dirty.into_iter().collect(),
                    since: Instant::now(),
                });
                self.preview = Some(state);
            }
        }
    }

    fn tend_preview(&mut self, visible: BTreeSet<usize>) -> Result<Tiles> {
        if self.drag.is_some() && !matches!(self.tool, Tool::Select | Tool::Strokes) {
            self.drag = None;
        }
        let mut uploads = Vec::new();
        let rev = self.scene.bundle.doc.rev;
        let posing = self.live_pose();
        if let Some(mut state) = self.preview.take() {
            let mut keep = true;
            match state.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
                Some(Ok(done)) => {
                    state.job = None;
                    let composed: BTreeSet<usize> = done.tiles.iter().map(|(i, _)| *i).collect();
                    let earlier = std::mem::replace(
                        &mut state.shown,
                        done.changed.intersection(&composed).copied().collect(),
                    );
                    state.shown.extend(earlier.difference(&composed));
                    let current = state.finish.as_ref().map_or(state.snap.rev, |f| f.rev) == rev;
                    let last = state.finish.as_ref().is_some_and(|f| f.ops == done.ops);
                    if current {
                        uploads = done.tiles;
                    }
                    if !current {
                        keep = false;
                        self.dirty.extend(earlier.into_iter().chain(done.hidden));
                        self.dirty.extend(state.release());
                    } else if last {
                        keep = false;
                        let settled = match done.exact && !self.scene.uses_caches() {
                            true => composed,
                            false => BTreeSet::new(),
                        };
                        let pending = std::mem::take(&mut state.pending);
                        let rest = earlier.into_iter().chain(done.hidden).chain(state.release()).collect();
                        self.settle(&pending, &settled, rest);
                    } else {
                        state.done = Some((done.ops, done.exact, composed));
                    }
                }
                Some(Err(e)) => {
                    keep = false;
                    self.dirty.extend(state.release());
                    self.failed(e.context("the moved picture could not be drawn"));
                }
                None => {}
            }
            if keep {
                let gone = match &state.posed {
                    Some(_) => posing.is_none(),
                    None => self.drag.is_none(),
                };
                let stale = state.finish.is_none() && (gone || state.snap.rev != rev || !self.canvas_current());
                let late = state.finish.as_ref().is_some_and(|f| f.since.elapsed() > FINISH_WAIT);
                match stale || late {
                    true => {
                        uploads.clear();
                        self.dirty.extend(state.release());
                    }
                    false => self.preview = Some(state),
                }
            }
        }
        if self.preview.is_none()
            && self.live.is_none()
            && (posing.is_some() || self.drag.as_ref().is_some_and(|d| d.moved() && !d.copy))
            && self.canvas_current()
            && self.dirty.len() <= timeline::FILL
        {
            let cels = match &posing {
                Some(p) => p.cels.to_vec(),
                None => self.preview_cels(),
            };
            self.preview = self
                .scene
                .preview_snapshot(&cels, &self.brushes)
                .map(|snap| PreviewState {
                    snap: Arc::new(snap),
                    pending: self.dirty.clone(),
                    cancel: Arc::default(),
                    asked: None,
                    asked_at: None,
                    job: None,
                    shown: BTreeSet::new(),
                    done: None,
                    finish: None,
                    posed: None,
                });
        }
        let position = self.drag.as_ref().map(|d| d.now);
        let ready = self
            .preview
            .as_ref()
            .filter(|p| p.job.is_none())
            .map(|p| p.finish.as_ref().map(|f| f.ops.clone()).ok_or(p.asked_at != position));
        let wanted = match (ready, &posing) {
            (Some(Ok(finish)), _) => Some(finish),
            (Some(Err(_)), Some(p)) => match self.preview.as_ref().is_some_and(|s| s.posed.as_ref() == Some(&p.key)) {
                true => None,
                false => Some(p.ops.to_vec()),
            },
            (Some(Err(true)), None) => self.drag_ops(),
            _ => None,
        };

        if let (Some(state), Some(ops)) = (self.preview.as_mut(), wanted) {
            state.asked_at = position;
            if state.asked.as_ref() != Some(&ops) {
                let job = preview::Job {
                    ops: ops.clone(),
                    snap: state.snap.clone(),
                    shown: state.shown.clone(),
                    visible,
                    cancel: state.cancel.clone(),
                };
                state.asked = Some(ops);
                state.posed = posing.as_ref().map(|p| p.key.clone());
                let (tx, rx) = channel();
                let ctx = self.ctx.clone();
                crate::background().spawn(move || {
                    let _ = tx.send(job.run());
                    ctx.request_repaint();
                });
                state.job = Some(rx);
            }
        }
        Ok(uploads)
    }

    fn screen(&self, view: View, rect: Rect, c: Vec2) -> Pos2 {
        rect.min + view.to_screen(c, rect.size())
    }

    fn visible_tiles(&self, view: View, rect: Rect) -> BTreeSet<usize> {
        let size = rect.size();
        let corners = [vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), size].map(|p| view.to_canvas(p, size));
        let range = |lo: f32, hi: f32, count: usize| {
            let at = |v: f32| ((v / klad_brush::TILE as f32).floor() as i64).clamp(0, count as i64 - 1) as usize;
            at(lo).saturating_sub(1)..=(at(hi) + 1).min(count - 1)
        };
        let (xs, ys) = (corners.map(|c| c.x), corners.map(|c| c.y));
        let low = |v: [f32; 4]| v.into_iter().fold(f32::MAX, f32::min);
        let high = |v: [f32; 4]| v.into_iter().fold(f32::MIN, f32::max);
        let cols = self.scene.cols();
        range(low(ys), high(ys), self.scene.rows())
            .flat_map(|ty| range(low(xs), high(xs), cols).map(move |tx| ty * cols + tx))
            .collect()
    }

    fn edges(&self) -> bool {
        match self.tool {
            _ if self.on_marks() => true,
            Tool::Select => self
                .selection
                .as_ref()
                .and_then(|id| match self.scene.bundle.doc.item(id) {
                    Some(Item::Node(_, _, node)) => Some(lift::square(node)),
                    _ => None,
                })
                .unwrap_or(false),
            _ => false,
        }
    }

    fn grab_at(&self, view: View, rect: Rect, pos: Pos2) -> Option<(String, Grab, [Vec2; 4])> {
        let (id, corners) = self.handles()?;
        let screen = corners.map(|c| self.screen(view, rect, c));
        grab(screen, pos, self.edges()).map(|g| (id, g, corners))
    }

    fn start_drag(&mut self, id: String, grab: Grab, corners: [Vec2; 4], at: Vec2, fresh: bool) {
        self.drop_preview();
        let strokes = self.on_marks() && self.shifting.is_none();
        if !strokes && self.shifting.is_none() && !matches!(self.scene.bundle.doc.item(&id), Some(Item::Node(..))) {
            return;
        }
        let center = corners.iter().fold(Vec2::ZERO, |a, c| a + *c) / 4.0;
        self.drag = Some(Drag {
            grab,
            node: id,
            strokes,
            fresh,
            copy: grab == Grab::Move && self.modifiers.alt,
            start: at,
            now: at,
            center,
            corners,
            slop: DRAG_SLOP / self.view.map_or(1.0, |v| v.zoom),
            crossed: false,
        });
    }

    fn erased(&self) -> impl Fn(&[Mark], &Mark) -> bool + Copy + '_ {
        move |marks: &[Mark], m: &Mark| {
            let eraser = m.as_stroke().is_some_and(|s| {
                self.brushes
                    .get(&s.brush)
                    .is_ok_and(|b| b.blend == klad_brush::brush::StrokeBlend::Erase)
            });
            eraser || !self.wiped(marks, &[m.id()]).is_empty()
        }
    }

    fn wiped(&self, marks: &[Mark], among: &[u32]) -> BTreeSet<u32> {
        let attached = klad_core::stroke::strokes_of(marks).any(|s| s.on.iter().any(|t| among.contains(t)));
        match attached {
            false => BTreeSet::new(),
            true => {
                let (w, h) = self.scene.size();
                klad_brush::scope::hidden(w, h, marks, &self.brushes, Some(among))
            }
        }
    }

    fn layer_rank(&self, id: &str) -> Option<usize> {
        self.scene.bundle.doc.all_layers().iter().position(|l| l.id == id)
    }

    fn shape_hits(&self, at: Vec2) -> Result<Vec<(String, String)>> {
        let active = self.scene.active_id();
        Ok(overlay::hits(&self.scene.bundle.doc, self.scene.frame, (at.x, at.y))?
            .into_iter()
            .filter(|(_, layer)| self.settings.pick_all_layers || active.as_deref() == Some(layer.as_str()))
            .collect())
    }

    pub(super) fn mark_at(&self, at: Vec2, slack: f32) -> Option<(Vec<usize>, u32)> {
        self.scene
            .marks_at((at.x, at.y), slack, self.settings.pick_all_layers, self.erased())
    }

    fn select_at(&mut self, at: Vec2, cycle: bool) -> Result<()> {
        let hits = self.shape_hits(at)?;
        let current = self
            .selection
            .as_ref()
            .and_then(|id| hits.iter().position(|(n, _)| n == id));
        let next = match (current, cycle) {
            (Some(i), true) => hits.get((i + 1) % hits.len().max(1)),
            _ => hits.first(),
        };
        self.selection = next.map(|(n, _)| n.clone());
        self.refresh_box();
        Ok(())
    }

    fn finish_drag(&mut self) -> Result<()> {
        let Some(drag) = self.drag.take() else {
            return Ok(());
        };
        if self.shifting.is_some() {
            return self.finish_shift_drag(&drag);
        }
        if drag.strokes {
            return self.finish_stroke_drag(&drag);
        }
        if !drag.moved() {
            if drag.grab == Grab::Move && !drag.fresh {
                self.select_at(drag.start, true)?;
            }
            return Ok(());
        }
        let Some(ops) = self.node_ops(&drag)? else {
            self.drop_preview();
            return Ok(());
        };
        let before = self.scene.bundle.doc.rev;
        let dirty = self.commit(ops.clone())?;
        self.finish_preview(ops, dirty, before);
        Ok(())
    }

    fn grab_under(&mut self, c: Vec2, pos: Pos2) -> Result<()> {
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let slack = 5.0 / zoom;
        let on_picked = self.picked.as_ref().is_some_and(|p| !p.more.is_empty())
            && self
                .picked_parts()
                .iter()
                .any(|(_, _, marks, ids)| markedit::hit(marks, (c.x, c.y), slack).is_some_and(|id| ids.contains(&id)));
        if on_picked && let Some((key, corners)) = self.picked_corners() {
            self.selection = None;
            self.start_drag(key, Grab::Move, corners, c, true);
            return Ok(());
        }
        let stroke = self.mark_at(c, slack);
        let shape = self
            .shape_hits(c)?
            .first()
            .and_then(|(_, layer)| self.layer_rank(layer));
        let stroke_first = match (&stroke, shape) {
            (Some((path, _)), Some(shape)) => self
                .scene
                .bundle
                .doc
                .locate_path(path)
                .and_then(|l| self.layer_rank(&l.id))
                .is_some_and(|rank| rank > shape),
            (_, None) => true,
            (None, Some(_)) => false,
        };
        if !stroke_first {
            self.select_at(c, false)?;
            if let Some((id, corners)) = self.selection_corners() {
                self.picked = None;
                self.start_drag(id, Grab::Move, corners, c, true);
                return Ok(());
            }
        }
        self.selection = None;
        let Some((path, id)) = stroke else {
            self.lasso = Some(Lasso {
                points: vec![c],
                start: pos,
                far: false,
            });
            return Ok(());
        };
        if path != self.scene.active {
            self.activate(path)?;
            let name = self.scene.active_layer().map(|l| l.name.clone()).unwrap_or_default();
            self.say(Tone::Info, format!("on layer {name}"));
            if !self.ensure_canvas()? {
                self.say(Tone::Info, format!("{name} is still loading; press again in a moment"));
                return Ok(());
            }
        }
        let Some(cel) = self.scene.active_cel() else {
            return Ok(());
        };
        let held: Vec<u32> = self
            .picked
            .as_ref()
            .filter(|p| p.cel == cel && p.cut.is_none())
            .map_or(Vec::new(), |p| p.ids.clone());
        let ids = match (self.modifiers.shift, held.contains(&id)) {
            (true, true) => held.into_iter().filter(|x| *x != id).collect(),
            (true, false) => held.into_iter().chain([id]).collect(),
            (false, true) => held,
            (false, false) => vec![id],
        };
        self.picked = (!ids.is_empty()).then(|| Picked::whole(cel, ids));
        if let Some((key, corners)) = self.picked_corners() {
            self.start_drag(key, Grab::Move, corners, c, true);
        }
        Ok(())
    }

    fn copy_for_drag(&mut self) -> Result<()> {
        let Some(drag) = self.drag.as_mut().filter(|d| d.copy && d.moved()) else {
            return Ok(());
        };
        drag.copy = false;
        let strokes = drag.strokes;
        self.drop_preview();
        self.duplicate_in_place()?;
        if !strokes && let (Some(drag), Some(id)) = (self.drag.as_mut(), self.selection.clone()) {
            drag.node = id;
        }
        Ok(())
    }

    fn finish_stroke_drag(&mut self, drag: &Drag) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            return Ok(());
        };
        if !drag.moved() && drag.fresh {
            return Ok(());
        }
        if !drag.moved() {
            self.lasso = Some(Lasso {
                points: vec![drag.start],
                start: Pos2::ZERO,
                far: false,
            });
            return self.finish_pick();
        }
        let start = Instant::now();
        match self.picked_ops(LassoAction::Transform(drag.mark_transform())) {
            Some((ops, after)) => {
                let before = self.scene.bundle.doc.rev;
                let dirty = self.commit(ops.clone())?;
                self.picked = after;
                self.forget_missing_selection();
                self.finish_preview(ops, dirty, before);
            }
            None => self.drop_preview(),
        }
        let text = format!(
            "{} strokes and fills {} and redrawn in {:.0} ms",
            self.picked.as_ref().map_or(0, Picked::count),
            if picked.cut.is_some() { "cut, moved" } else { "moved" },
            ms(start)
        );
        self.measure("strokes", text);
        Ok(())
    }

    fn also_cels(&self) -> Vec<(String, String)> {
        self.pick_cels(false)
    }

    fn pick_cels(&self, every: bool) -> Vec<(String, String)> {
        let doc = &self.scene.bundle.doc;
        let active = self.scene.active_id();
        let layers: Vec<&klad_core::doc::Layer> = match every {
            true => doc.all_layers(),
            false => self.also.iter().filter_map(|id| doc.layer(id)).collect(),
        };
        layers
            .into_iter()
            .filter(|l| l.kind() == Kind::Paint && l.visible && !l.locked)
            .filter(|l| !every || active.as_deref() != Some(l.id.as_str()))
            .filter_map(|l| l.content_at(self.scene.frame).map(|c| (l.id.clone(), c.id.clone())))
            .collect()
    }

    fn finish_pick(&mut self) -> Result<()> {
        let Some(lasso) = self.lasso.take() else {
            return Ok(());
        };
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let click = !lasso.far || lasso.points.len() < 3;
        if click
            && !self.modifiers.shift
            && let Some((path, _)) = self.mark_at(lasso.points[0], 5.0 / zoom)
            && path != self.scene.active
        {
            self.activate(path)?;
        }
        let every = !click && self.settings.pick_all_layers;
        if every
            && self.scene.active_cel().is_none()
            && let Some((layer, _)) = self.pick_cels(true).into_iter().next()
            && let Some(klad_core::doc::Loc::Layer(path)) = self.scene.bundle.doc.locate(&layer)
        {
            self.activate(path)?;
        }
        let Some(cel) = self.scene.active_cel() else {
            self.say(Tone::Refused, "make a paint layer active to select its strokes");
            return Ok(());
        };
        let at = lasso.points[0];
        let erased = self.erased();
        let polygon: Option<Vec<[f32; 2]>> =
            (lasso.far && lasso.points.len() >= 3).then(|| lasso.points.iter().map(|p| [p.x, p.y]).collect());
        let pick = |marks: &[Mark]| -> (Vec<u32>, bool) {
            match &polygon {
                Some(polygon) => {
                    let sides = markedit::sides(marks, polygon);
                    let crossing = sides.iter().any(|(_, side)| *side == markedit::Side::Crossing);
                    let mut inside: Vec<u32> = sides
                        .into_iter()
                        .filter(|(_, side)| *side == markedit::Side::Inside)
                        .map(|(id, _)| id)
                        .collect();
                    let wiped = self.wiped(marks, &inside);
                    inside.retain(|id| !wiped.contains(id));
                    (inside, crossing)
                }
                None => (
                    markedit::pick(marks, (at.x, at.y), 5.0 / zoom, |m| erased(marks, m))
                        .into_iter()
                        .collect(),
                    false,
                ),
            }
        };
        let (found, crossing) = pick(self.scene.active_marks());
        let others: Vec<(String, String, Vec<u32>, bool)> = self
            .pick_cels(every)
            .into_iter()
            .map(|(layer, cel)| {
                let (ids, crossing) = pick(self.scene.marks_of(&cel));
                (layer, cel, ids, crossing)
            })
            .collect();
        let adding = self.modifiers.shift;
        let previous = self.picked.clone().filter(|p| adding && p.cel == cel);
        let crossed = crossing || others.iter().any(|o| o.3);
        let cut = polygon
            .clone()
            .filter(|_| self.cut_lasso && crossed)
            .or_else(|| previous.as_ref().and_then(|p| p.cut.clone()));
        let merge = |held: Vec<u32>, found: &[u32]| {
            found.iter().fold(held, |mut ids, id| {
                if adding && !lasso.far && ids.contains(id) {
                    ids.retain(|x| x != id);
                } else if !ids.contains(id) {
                    ids.push(*id);
                }
                ids
            })
        };
        let ids = merge(previous.as_ref().map_or(Vec::new(), |p| p.ids.clone()), &found);
        let more: Vec<Part> = others
            .into_iter()
            .filter_map(|(layer, cel, found, crossing)| {
                let held = previous
                    .as_ref()
                    .and_then(|p| p.more.iter().find(|q| q.cel == cel))
                    .map_or(Vec::new(), |q| q.ids.clone());
                let ids = merge(held, &found);
                (!ids.is_empty() || (cut.is_some() && crossing)).then_some(Part { layer, cel, ids })
            })
            .collect();
        self.picked = (!ids.is_empty() || cut.is_some() || !more.is_empty()).then_some(Picked { cel, ids, cut, more });
        Ok(())
    }

    fn refresh_shape(&mut self) {
        let rev = self.scene.bundle.doc.rev;
        let Some(id) = self.direct.as_ref().map(|d| d.node.clone()) else {
            self.shaped = None;
            return;
        };
        if self.shaped.as_ref().is_some_and(|s| s.node == id && s.rev == rev) {
            return;
        }
        if let Some(subs) = self.stroke_path(&id) {
            self.shaped = Some(Shaped {
                node: id,
                rev,
                subs,
                matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            });
            return;
        }
        let doc = &self.scene.bundle.doc;
        self.shaped = match doc.item(&id) {
            Some(Item::Node(_, _, node)) => pathedit::from_shape(&node.shape)
                .ok()
                .zip(overlay::local_to_canvas(doc, &id).ok().flatten())
                .map(|(subs, matrix)| Shaped {
                    node: id,
                    rev,
                    subs,
                    matrix,
                }),
            _ => None,
        };
    }

    fn point_edit(&self, drag: &PointDrag) -> Option<PathEdit> {
        let shaped = self.shaped.as_ref()?;
        let direct = self.direct.as_ref()?;
        let (a, b) = (to_local(shaped.matrix, drag.start), to_local(shaped.matrix, drag.now));
        match drag.grab {
            PointGrab::Anchors => Some(PathEdit::Move {
                points: direct.points.iter().copied().collect(),
                by: [b[0] - a[0], b[1] - a[1]],
            }),
            PointGrab::Handle(point, side) => Some(PathEdit::Handle {
                point,
                side,
                to: b,
                free: drag.free,
            }),
            PointGrab::Marquee { .. } => None,
        }
    }

    fn shown_subs(&self) -> Option<Vec<Subpath>> {
        if let Some(commit) = self.path_commit.as_ref() {
            return Some(commit.preview.clone());
        }
        let mut subs = self.shaped.as_ref()?.subs.clone();
        if let Some(edit) = self.point_drag.as_ref().and_then(|d| self.point_edit(d)) {
            let _ = edit.apply(&mut subs);
        }
        Some(subs)
    }

    fn direct_down(&mut self, view: View, rect: Rect, pos: Pos2, c: Vec2) -> Result<()> {
        let (alt, shift) = (self.modifiers.alt, self.modifiers.shift);
        enum Action {
            Handle(usize, Side),
            Anchor(usize, bool),
            Insert(usize, f32),
            Elsewhere,
        }
        let action = match self.shaped.as_ref() {
            Some(shaped) => {
                let m = shaped.matrix;
                let screen = |p: [f32; 2]| self.screen(view, rect, to_canvas(m, p));
                let anchors = pathedit::anchors(&shaped.subs);
                let selected: Vec<usize> = self
                    .direct
                    .as_ref()
                    .map_or(Vec::new(), |d| d.points.iter().copied().collect());
                let handle = selected.iter().find_map(|&i| {
                    let a = anchors.get(i)?.1;
                    [(Side::In, a.inn, a.has_in()), (Side::Out, a.out, a.has_out())]
                        .into_iter()
                        .find(|(_, p, has)| *has && screen(*p).distance(pos) <= HANDLE)
                        .map(|(side, _, _)| (i, side))
                });
                let anchor = anchors.iter().position(|(_, a)| screen(a.at).distance(pos) <= HANDLE);
                let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt() * view.zoom;
                let near = pathedit::nearest(&shaped.subs, to_local(m, c)).filter(|(_, _, d)| d * scale <= 8.0);
                match (handle, anchor, near) {
                    (Some((i, side)), _, _) => Action::Handle(i, side),
                    (None, Some(i), _) => Action::Anchor(i, anchors[i].1.has_in() || anchors[i].1.has_out()),
                    (None, None, Some((segment, t, _))) if alt => Action::Insert(segment, t),
                    _ => Action::Elsewhere,
                }
            }
            None => Action::Elsewhere,
        };
        match action {
            Action::Handle(i, side) => {
                self.point_drag = Some(PointDrag {
                    grab: PointGrab::Handle(i, side),
                    start: c,
                    now: c,
                    free: alt,
                });
            }
            Action::Anchor(i, curved) if alt => {
                let points = vec![i];
                return self.commit_path(if curved {
                    PathEdit::Corner { points }
                } else {
                    PathEdit::Smooth { points }
                });
            }
            Action::Anchor(i, _) => {
                if let Some(d) = self.direct.as_mut() {
                    if shift {
                        if !d.points.remove(&i) {
                            d.points.insert(i);
                        }
                    } else if !d.points.contains(&i) {
                        d.points = BTreeSet::from([i]);
                    }
                    if d.points.contains(&i) {
                        self.point_drag = Some(PointDrag {
                            grab: PointGrab::Anchors,
                            start: c,
                            now: c,
                            free: false,
                        });
                    }
                }
            }
            Action::Insert(segment, t) => return self.commit_path(PathEdit::Insert { segment, t }),
            Action::Elsewhere => {
                self.under = overlay::leaf_hits(&self.scene.bundle.doc, self.scene.frame, (c.x, c.y))?;
                let zoom = self.view.map_or(1.0, |v| v.zoom);
                let mut on_stroke = false;
                if self.under.is_empty()
                    && let Some((path, mark)) = self.mark_at(c, 5.0 / zoom)
                {
                    if path != self.scene.active {
                        self.activate(path)?;
                    }
                    let picked = self
                        .scene
                        .active_marks()
                        .iter()
                        .find(|m| m.id() == mark)
                        .map(|m| m.as_stroke().is_some());
                    match (picked, self.scene.active_cel()) {
                        (Some(true), Some(cel)) => {
                            let address = markedit::address(&cel, mark);
                            if self.direct.as_ref().map(|d| &d.node) != Some(&address) {
                                self.pick_shape(address);
                                return Ok(());
                            }
                            on_stroke = true;
                        }
                        (Some(false), _) => {
                            self.say(Tone::Info, "that is a fill: A reshapes strokes and vector shapes")
                        }
                        _ => {}
                    }
                }
                let current = self.direct.as_ref().map(|d| d.node.clone());
                let hit = match &current {
                    Some(id) if self.under.contains(id) => Some(id.clone()),
                    _ => self.under.first().cloned(),
                };
                match hit {
                    Some(id) if Some(&id) != current.as_ref() => self.pick_shape(id),
                    hit => {
                        self.point_drag = Some(PointDrag {
                            grab: PointGrab::Marquee {
                                on_shape: hit.is_some() || on_stroke,
                            },
                            start: c,
                            now: c,
                            free: false,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn pen_down(&mut self, view: View, rect: Rect, pos: Pos2, c: Vec2) -> Result<()> {
        let vector = self
            .scene
            .active_layer()
            .is_some_and(|l| l.kind() == Kind::Vector && !l.locked);
        if !vector {
            self.say(
                Tone::Refused,
                "the pen draws vector paths: pick a vector layer, or add one with \"vector\" in Layers",
            );
            return Ok(());
        }
        let first = self
            .pen_path
            .as_ref()
            .filter(|p| p.anchors.len() >= 2)
            .and_then(|p| p.anchors.first())
            .map(|a| self.screen(view, rect, vec2(a.at[0], a.at[1])));
        if first.is_some_and(|f| f.distance(pos) <= HANDLE + 2.0) {
            return self.commit_pen(true);
        }
        let path = self.pen_path.get_or_insert_with(|| PenPath {
            anchors: Vec::new(),
            dragging: false,
        });
        path.anchors.push(pathedit::Anchor::corner([c.x, c.y]));
        path.dragging = true;
        Ok(())
    }

    fn pen_move(&mut self, c: Vec2) {
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let Some(last) = self
            .pen_path
            .as_mut()
            .filter(|p| p.dragging)
            .and_then(|p| p.anchors.last_mut())
        else {
            return;
        };
        let pull = [c.x - last.at[0], c.y - last.at[1]];
        let (out, inn) = match pull[0].hypot(pull[1]) * zoom > 3.0 {
            true => ([c.x, c.y], [last.at[0] - pull[0], last.at[1] - pull[1]]),
            false => (last.at, last.at),
        };
        last.out = out;
        last.inn = inn;
    }

    fn pen_keys(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.pen_path.is_none() {
            return Ok(());
        }
        if self.tool != Tool::Pen {
            return self.commit_pen(false);
        }
        if self.keys_busy {
            return Ok(());
        }
        let (finish, cancel) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if cancel {
            self.pen_path = None;
            self.say(Tone::Info, "path dropped");
        }
        if finish {
            self.commit_pen(false)?;
        }
        Ok(())
    }

    fn commit_pen(&mut self, closed: bool) -> Result<()> {
        let Some(path) = self.pen_path.take() else {
            return Ok(());
        };
        if path.anchors.len() < 2 {
            self.say(Tone::Info, "a path needs two points: click again before Return");
            return Ok(());
        }
        let Some((layer, name)) = self
            .scene
            .active_layer()
            .filter(|l| l.kind() == Kind::Vector)
            .map(|l| (l.id.clone(), l.name.clone()))
        else {
            return Ok(());
        };
        let d = pathedit::to_d(&[Subpath {
            anchors: path.anchors,
            closed,
        }]);
        let (colour, outline) = (color::hex(self.color), color::hex(self.second));
        let fill = (closed && self.pen_fill).then(|| colour.clone());
        let mut node = json!({ "type": "path", "d": d });
        if let Some(fill) = &fill {
            node["fill"] = json!(fill);
        }
        if self.pen_stroke || fill.is_none() {
            node["stroke"] = json!({ "paint": outline, "width": self.pen_width, "cap": "round", "join": "round" });
        }
        let op = Op::AddNode {
            node,
            place: klad_core::ops::Place {
                into: Some(layer),
                frame: Some(self.scene.frame),
                ..Default::default()
            },
        };
        self.apply(vec![op])?;
        self.used_colour();
        self.say(
            Tone::Done,
            format!(
                "{} path added to {name}: V moves it, A edits its points",
                if closed { "closed" } else { "open" }
            ),
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn decorate_pen(
        &self,
        painter: &egui::Painter,
        path: &PenPath,
        hover: Option<Pos2>,
        view: View,
        rect: Rect,
        accent: Color32,
        dark: egui::Stroke,
    ) {
        let to_screen = |p: [f32; 2]| self.screen(view, rect, vec2(p[0], p[1]));
        let mut anchors = path.anchors.clone();
        if let Some(hover) = hover.filter(|h| !path.dragging && rect.contains(*h)) {
            let c = view.to_canvas(hover - rect.min, rect.size());
            anchors.push(pathedit::Anchor::corner([c.x, c.y]));
        }
        let sub = Subpath { anchors, closed: false };
        pathedit::flatten(&[sub], 2.0 / view.zoom.max(1e-3))
            .into_iter()
            .for_each(|line| {
                let points: Vec<Pos2> = line.into_iter().map(to_screen).collect();
                painter.add(egui::Shape::line(points.clone(), dark));
                painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, accent)));
            });
        if let Some(last) = path.anchors.last().filter(|a| a.has_out()) {
            [last.inn, last.out].into_iter().for_each(|h| {
                painter.line_segment([to_screen(last.at), to_screen(h)], egui::Stroke::new(1.0, accent));
                painter.circle_filled(to_screen(h), 3.5, accent);
            });
        }
        path.anchors.iter().enumerate().for_each(|(i, a)| {
            let p = to_screen(a.at);
            let size = if i == 0 && path.anchors.len() >= 2 {
                HANDLE + 3.0
            } else {
                HANDLE
            };
            let square = Rect::from_center_size(p, vec2(size, size));
            painter.rect_filled(square, 1.0, Color32::WHITE);
            painter.rect_stroke(square, 1.0, egui::Stroke::new(1.0, accent), egui::StrokeKind::Middle);
        });
    }

    fn pick_shape(&mut self, id: String) {
        self.direct = Some(Direct {
            node: id,
            points: BTreeSet::new(),
        });
        self.refresh_shape();
    }

    fn direct_up(&mut self) -> Result<()> {
        let Some(drag) = self.point_drag.take() else {
            return Ok(());
        };
        let zoom = self.view.map_or(1.0, |v| v.zoom);
        let moved = (drag.now - drag.start).length() * zoom > 2.0;
        match drag.grab {
            PointGrab::Marquee { on_shape } => {
                if !moved {
                    let current = self.direct.as_ref().map(|d| d.node.clone());
                    let below = current
                        .and_then(|id| self.under.iter().position(|u| *u == id))
                        .filter(|_| self.under.len() > 1)
                        .map(|i| self.under[(i + 1) % self.under.len()].clone());
                    match (on_shape, below) {
                        (true, Some(id)) => self.pick_shape(id),
                        (true, None) => {
                            if let Some(d) = self.direct.as_mut() {
                                d.points.clear();
                            }
                        }
                        (false, _) => self.direct = None,
                    }
                    return Ok(());
                }
                let Some(shaped) = self.shaped.as_ref() else {
                    return Ok(());
                };
                let (lo, hi) = (drag.start.min(drag.now), drag.start.max(drag.now));
                let inside: Vec<usize> = pathedit::anchors(&shaped.subs)
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, a))| {
                        let p = to_canvas(shaped.matrix, a.at);
                        p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
                    })
                    .map(|(i, _)| i)
                    .collect();
                let adding = self.modifiers.shift;
                if let Some(d) = self.direct.as_mut() {
                    if !adding {
                        d.points.clear();
                    }
                    d.points.extend(inside);
                }
                Ok(())
            }
            _ if !moved => Ok(()),
            _ => match self.point_edit(&drag) {
                Some(edit) => self.commit_path(edit),
                None => Ok(()),
            },
        }
    }

    fn stroke_path(&self, id: &str) -> Option<Vec<Subpath>> {
        let (cel, mark) = markedit::parse_address(id)?;
        if self.scene.active_cel().as_deref() != Some(cel) {
            return None;
        }
        let stroke = self
            .scene
            .active_marks()
            .iter()
            .find(|m| m.id() == mark)
            .and_then(|m| m.as_stroke())?;
        let points: Vec<[f32; 2]> = stroke
            .samples
            .iter()
            .map(|s| {
                let (x, y) = s.pos();
                [x, y]
            })
            .collect();
        Some(vec![pathedit::fit_polyline(
            &points,
            (stroke.size * 0.12).clamp(1.0, 6.0),
        )])
    }

    fn reshape_stroke(&mut self, id: String, edit: PathEdit) -> Result<()> {
        let Some(shaped) = self.shaped.as_ref().filter(|s| s.node == id) else {
            return Ok(());
        };
        let mut subs = shaped.subs.clone();
        if let Err(e) = edit.apply(&mut subs) {
            self.say(Tone::Refused, format!("{e:#}"));
            return Ok(());
        }
        let points = |subs: &[Subpath]| subs.iter().map(|s| s.anchors.len()).sum::<usize>();
        if points(&subs) != points(&shaped.subs)
            && let Some(d) = self.direct.as_mut()
        {
            d.points.clear();
        }
        let props: Map<String, Value> = [("path".to_string(), json!(pathedit::to_d(&subs)))]
            .into_iter()
            .collect();
        self.apply(vec![Op::Set { id: id.clone(), props }])?;
        self.shaped = Some(Shaped {
            node: id,
            rev: self.scene.bundle.doc.rev,
            subs,
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        });
        Ok(())
    }

    fn commit_path(&mut self, edit: PathEdit) -> Result<()> {
        let Some(id) = self.direct.as_ref().map(|d| d.node.clone()) else {
            return Ok(());
        };
        if self.path_commit.is_some() {
            return Ok(());
        }
        if markedit::parse_address(&id).is_some() {
            return self.reshape_stroke(id, edit);
        }
        let doc = &self.scene.bundle.doc;
        let Some(Loc::Node(path, c, _)) = doc.locate(&id) else {
            return Ok(());
        };
        let layer = doc.layer_at(&path);
        if layer.locked {
            self.say(
                Tone::Refused,
                format!(
                    "{} is locked: unlock it with the lock in the layers panel to change it",
                    layer.name
                ),
            );
            return Ok(());
        }
        let cel = layer.cels()[c].id.clone();
        let ops = vector::pinned(doc, &[Op::Path { id: id.clone(), edit }])?;
        let (next, _) = apply_all(doc, &ops)?;
        let nodes = match next.locate(&cel) {
            Some(Loc::Cel(p, c)) => next.layer_at(&p).cels()[c].nodes.clone(),
            _ => Vec::new(),
        };
        let preview = match next.item(&id) {
            Some(Item::Node(_, _, n)) => pathedit::from_shape(&n.shape).unwrap_or_default(),
            _ => Vec::new(),
        };
        let count = |subs: &[Subpath]| subs.iter().map(|s| s.anchors.len()).sum::<usize>();
        if Some(count(&preview)) != self.shaped.as_ref().map(|s| count(&s.subs))
            && let Some(d) = self.direct.as_mut()
        {
            d.points.clear();
        }
        let job = lift::render(nodes, self.scene.size(), self.ctx.clone());
        self.path_commit = Some(PathCommit {
            job,
            ops,
            cel,
            preview,
            node: id,
        });
        Ok(())
    }

    fn tend_path(&mut self) -> Result<()> {
        let Some(result) = self.path_commit.as_ref().and_then(|p| p.job.try_recv().ok()) else {
            return Ok(());
        };
        let Some(commit) = self.path_commit.take() else {
            return Ok(());
        };
        let start = Instant::now();
        let dirty = match result {
            Ok((hash, raster)) => self
                .scene
                .apply_rendered(&commit.ops, (commit.cel, hash, raster), &self.brushes)?,
            Err(e) => {
                self.failed(e.context("the edited path could not be drawn"));
                self.scene.apply(&commit.ops, &self.brushes)?
            }
        };
        self.dirty.extend(dirty);
        self.measure(
            "point edit",
            format!("point edit on {} written in {:.0} ms", commit.node, ms(start)),
        );
        self.forget_missing_selection();
        Ok(())
    }

    fn on_pen(&mut self, ctx: &egui::Context, e: PenEvent, rect: Rect, was_focused: bool) -> Result<()> {
        self.readout.pressure = e.pressure;
        self.readout.tilt = e.tilt;
        self.readout.eraser = e.eraser;
        self.readout.tablet = e.tablet;
        if e.tablet {
            self.readout.device = e.device;
            if e.phase != Phase::Up && self.settings.note_peak(&device_key(e.device), e.pressure) {
                self.settings_dirty = true;
            }
        }
        if e.phase == Phase::Move {
            self.readout.samples.push_back(e.at);
            while self
                .readout
                .samples
                .front()
                .is_some_and(|t| e.at.duration_since(*t) > Duration::from_secs(1))
            {
                self.readout.samples.pop_front();
            }
        }
        let Some(view) = self.view else {
            return Ok(());
        };
        let c = view.to_canvas(e.pos - rect.min, rect.size());
        let on_canvas = rect.contains(e.pos)
            && ctx
                .layer_id_at(e.pos)
                .is_none_or(|l| l.order == egui::Order::Background);
        let picking = self.modifiers.alt && !self.modifiers.command && matches!(self.tool, Tool::Brush | Tool::Fill);
        let sizing_now = self.modifiers.alt && self.modifiers.command && self.tool == Tool::Brush;
        match (e.phase, self.sizing) {
            (Phase::Down, None) if sizing_now && on_canvas && self.live.is_none() => {
                self.sizing = Some((e.pos.x, self.size, e.pos));
                return Ok(());
            }
            (Phase::Move, Some((x, start, _))) => {
                self.size = options::size_from_drag(start, e.pos.x - x);
                return Ok(());
            }
            (Phase::Up, Some(_)) => {
                self.sizing = None;
                return Ok(());
            }
            _ => {}
        }
        let pressing = e.phase == Phase::Down && was_focused && on_canvas && !self.navigating;
        if pressing && self.holding_frame_keys() {
            return Ok(());
        }
        let needs_canvas = matches!(self.tool, Tool::Brush | Tool::Fill | Tool::Strokes | Tool::Select);
        if pressing && !needs_canvas {
            self.hold_still();
        }
        if pressing && needs_canvas && !self.ensure_canvas()? {
            self.say(
                Tone::Info,
                format!("frame {} is still loading; draw again in a moment", self.scene.frame),
            );
            return Ok(());
        }
        match (self.tool, e.phase) {
            (_, Phase::Down) if !was_focused || !on_canvas || self.navigating => {}
            (_, Phase::Down) if picking => self.pick_colour(c, self.modifiers.shift)?,
            (Tool::Fill, Phase::Down) => {
                if self.scene.can_paint() {
                    self.lasso = Some(Lasso {
                        points: vec![c],
                        start: e.pos,
                        far: false,
                    });
                }
            }
            (Tool::Strokes, Phase::Down) => match self.grab_at(view, rect, e.pos) {
                Some((id, grab, corners)) => self.start_drag(id, grab, corners, c, false),
                None => {
                    self.lasso = Some(Lasso {
                        points: vec![c],
                        start: e.pos,
                        far: false,
                    });
                }
            },
            (Tool::Strokes, Phase::Up) if self.drag.is_some() => self.finish_drag()?,
            (Tool::Strokes, Phase::Up) => self.finish_pick()?,
            (Tool::Strokes, Phase::Move) if self.drag.is_some() => {
                if let Some(drag) = self.drag.as_mut() {
                    drag.to(c);
                }
                self.copy_for_drag()?;
            }
            (Tool::Direct, Phase::Down) if self.path_commit.is_some() => {}
            (Tool::Direct, Phase::Down) => self.direct_down(view, rect, e.pos, c)?,
            (Tool::Direct, Phase::Move) => {
                if let Some(drag) = self.point_drag.as_mut() {
                    drag.now = c;
                    drag.free |= self.modifiers.alt;
                }
            }
            (Tool::Direct, Phase::Up) => self.direct_up()?,
            (Tool::Fill | Tool::Strokes | Tool::Select, Phase::Move) if self.drag.is_none() => {
                if let Some(lasso) = self.lasso.as_mut() {
                    lasso.far |= (e.pos - lasso.start).length() > 6.0;
                    if lasso.points.last().is_none_or(|p| (*p - c).length() >= 1.0) {
                        lasso.points.push(c);
                    }
                }
            }
            (Tool::Fill, Phase::Up) => self.finish_fill()?,
            (Tool::Brush, Phase::Down) => {
                if self.live.is_none() && self.scene.can_paint() {
                    self.begin(&e, c)?;
                }
            }
            (Tool::Brush, Phase::Move) => {
                if let Some(live) = self.live.as_mut() {
                    let dt = e.at.duration_since(live.last_at).as_secs_f32() * 1000.0;
                    let sample = live.recorder.push(raw_of(&e, c, dt));
                    live.painter
                        .push_on(&sample, &mut self.buffer, self.scene.active_raster());
                    live.stroke.samples.push(sample);
                    if let Some((axis, copy, painter)) = live.twin.as_mut() {
                        let twin = make::mirrored(&sample, *axis);
                        painter.push_on(&twin, &mut self.mirror_buffer, self.scene.active_raster());
                        copy.samples.push(twin);
                    }
                    live.last_at = e.at;
                    self.pending.get_or_insert(e.at);
                }
            }
            (Tool::Brush, Phase::Up) => self.finish()?,
            (Tool::Select, Phase::Down) => match self.grab_at(view, rect, e.pos) {
                Some((id, grab, corners)) => self.start_drag(id, grab, corners, c, false),
                None => {
                    self.end_shift();
                    self.grab_under(c, e.pos)?
                }
            },
            (Tool::Select, Phase::Move) => {
                if let Some(drag) = self.drag.as_mut() {
                    drag.to(c);
                }
                self.copy_for_drag()?;
            }
            (Tool::Select, Phase::Up) if self.lasso.is_some() => match self.scene.active_cel() {
                Some(_) => self.finish_pick()?,
                None => {
                    self.lasso = None;
                    self.picked = None;
                }
            },
            (Tool::Select, Phase::Up) => self.finish_drag()?,
            (Tool::Note, Phase::Down) => self.press_note(c),
            (Tool::Shape, Phase::Down) => self.shape_down(c),
            (Tool::Shape, Phase::Move) => self.shape_move(c),
            (Tool::Shape, Phase::Up) => self.shape_up()?,
            (Tool::Text, Phase::Down) => self.text_down(c)?,
            (Tool::Bone, Phase::Down) => self.bone_down(c),
            (Tool::Bone, Phase::Move) => self.bone_move(c),
            (Tool::Bone, Phase::Up) => self.bone_up()?,
            (Tool::Pen, Phase::Down) => self.pen_down(view, rect, e.pos, c)?,
            (Tool::Pen, Phase::Move) => self.pen_move(c),
            (Tool::Pen, Phase::Up) => {
                if let Some(path) = self.pen_path.as_mut() {
                    path.dragging = false;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn shaped(&self) -> Result<(String, BrushDef)> {
        let base = self.brushes.def(&self.brush).ok_or_else(|| {
            anyhow!(
                "the brush {} is not there any more: pick another one in the brush panel",
                self.brush
            )
        })?;
        let def = match &self.custom {
            Some(def) => def.clone(),
            None => self.tuning.shape(base),
        };
        Ok((pick_id(&self.brush, &def, &self.embedded), def))
    }

    fn pick(&mut self, id: &str) {
        if let Some(def) = self.brushes.def(id) {
            self.tuning = self.tuned(id, def);
            self.brush = id.to_string();
            self.custom = None;
            self.brush_window.name = def.name.clone();
        }
    }

    fn tuned(&self, id: &str, def: &BrushDef) -> Tuning {
        let tuning = Tuning::of(def);
        match (id, self.settings.eraser_soft, tuning.hardness) {
            ("eraser", Some(soft), Some(_)) => Tuning {
                hardness: Some(1.0 - soft),
                ..tuning
            },
            _ => tuning,
        }
    }

    pub(super) fn eraser_soft(&self) -> Option<f32> {
        if self.is_on(Cmd::Eraser) != Some(true) {
            return None;
        }
        let hardness = self.brushes.def("eraser").and_then(BrushDef::hardness)?;
        Some(self.settings.eraser_soft.unwrap_or(1.0 - hardness))
    }

    pub(super) fn set_eraser_soft(&mut self, soft: f32) {
        self.settings.eraser_soft = Some(soft.clamp(0.0, 1.0));
        self.settings_dirty = true;
        if self.brush == "eraser" {
            self.tuning.hardness = Some(1.0 - soft.clamp(0.0, 1.0));
        }
    }

    fn begin(&mut self, e: &PenEvent, c: Vec2) -> Result<()> {
        let (name, def, tuning) = if e.eraser {
            let base = self
                .brushes
                .def("eraser")
                .cloned()
                .ok_or_else(|| crate::report::bug("the eraser brush is missing from the library"))?;
            let tuning = self.tuned("eraser", &base);
            let def = tuning.shape(&base);
            (pick_id("eraser", &def, &self.embedded), def, tuning)
        } else {
            let (name, def) = self.shaped()?;
            let tuning = match &self.custom {
                Some(custom) => Tuning {
                    taper: custom.taper,
                    stabilizer: custom.stabilizer,
                    smoothing: custom.smoothing,
                    ..self.tuning
                },
                None => self.tuning,
            };
            (name, def, tuning)
        };
        if self.brushes.def(&name) != Some(&def) {
            self.brushes.insert(&name, def);
        }
        let brush = self.brushes.get(&name)?.clone();
        let lock = self.scene.active_locks_alpha();
        if lock && stroke_blend(&brush, true).is_none() {
            self.say(
                Tone::Refused,
                "alpha is locked on this layer, so the eraser cannot change it (untick lock alpha)",
            );
            return Ok(());
        }
        let id = self.scene.next_mark_id();
        let mut stroke = Stroke::new(id, &name, self.color, self.size, self.rng.next_u64() as u32);
        stroke.opacity = self.opacity;
        stroke.flow = self.flow;
        stroke.taper = tuning.taper;
        stroke.lock = lock;
        let curve = if e.tablet {
            self.settings.curve(&device_key(e.device))
        } else {
            PressureCurve::default()
        };
        let mut recorder = Recorder::new(curve, tuning.stabilizer, tuning.smoothing);
        let sample = recorder.push(raw_of(e, c, 0.0));
        if brush.def.color.is_some_and(|c| c.uses_second()) {
            stroke.second = Some(klad_core::color::to_u16(self.second));
        }
        let mut painter = StrokePainter::live(&brush, &stroke);
        painter.push_on(&sample, &mut self.buffer, self.scene.active_raster());
        stroke.samples.push(sample);
        let twin = self.mirror.map(|axis| {
            let mut copy = stroke.clone();
            copy.id = id + 1;
            copy.seed = stroke.seed.wrapping_add(1);
            copy.samples = vec![make::mirrored(&sample, axis)];
            let mut painter = StrokePainter::live(&brush, &copy);
            painter.push_on(&copy.samples[0], &mut self.mirror_buffer, self.scene.active_raster());
            (axis, copy, painter)
        });
        self.live = Some(Live {
            stroke,
            painter,
            brush,
            recorder,
            last_at: e.at,
            twin,
        });
        self.pending.get_or_insert(e.at);
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        let Some(mut live) = self.live.take() else {
            return Ok(());
        };
        let start = Instant::now();
        let tail = live.recorder.finish();
        tail.iter()
            .for_each(|s| live.painter.push_on(s, &mut self.buffer, self.scene.active_raster()));
        live.painter.end(&mut self.buffer, self.scene.active_raster());
        if let Some((axis, copy, painter)) = live.twin.as_mut() {
            tail.iter().map(|s| make::mirrored(s, *axis)).for_each(|s| {
                painter.push_on(&s, &mut self.mirror_buffer, self.scene.active_raster());
                copy.samples.push(s);
            });
            painter.end(&mut self.mirror_buffer, self.scene.active_raster());
        }
        live.stroke.samples.extend(tail);
        let repainted = needs_repaint(&live.stroke);
        if repainted {
            self.dirty.extend(self.buffer.touched().iter().copied());
            self.buffer.reset();
            klad_brush::draw_stroke_on(&mut self.buffer, &live.brush, &live.stroke, self.scene.active_raster());
            if let Some((_, copy, _)) = &live.twin {
                self.dirty.extend(self.mirror_buffer.touched().iter().copied());
                self.mirror_buffer.reset();
                klad_brush::draw_stroke_on(&mut self.mirror_buffer, &live.brush, copy, self.scene.active_raster());
            }
        }
        let ended = ms(start);
        if self.external.is_some() {
            self.reload()?;
        }
        if self.scene.bundle.embed_brush(&live.stroke.brush, &live.brush.def)? {
            self.embedded.insert(live.stroke.brush.clone(), live.brush.def.clone());
        }
        let samples = live.stroke.samples.len();
        let dabs = live.painter.dabs();
        let alone = encode_cel(&[Mark::Stroke(live.stroke.clone())])?.len();
        let brush = live.stroke.brush.clone();
        let erasing = live.brush.blend == klad_brush::brush::StrokeBlend::Erase;
        let blend = stroke_blend(&live.brush, live.stroke.lock).unwrap_or(live.brush.blend);
        let attaching = Instant::now();
        live.stroke.on = self.attaching(&live.stroke, &live.brush.def);
        let attach = ms(attaching);
        let mut touched = self.scene.commit_stroke(live.stroke, &mut self.buffer, blend)?;
        if let Some((_, mut copy, _)) = live.twin {
            copy.id = self.scene.next_mark_id();
            copy.on = self.attaching(&copy, &live.brush.def);
            touched.extend(self.scene.commit_stroke(copy, &mut self.mirror_buffer, blend)?);
        }
        let commit = ms(start);
        self.dirty.extend(touched);
        let saving = Instant::now();
        let bytes = self.scene.save_active("stroke")?;
        self.first.drew |= !erasing;
        if !erasing {
            self.used_colour();
        }
        let count = self.scene.active_marks().len();
        let text = format!(
            "{brush}: {samples} samples, {dabs} live dabs, {alone} B packed alone\nlayer file {bytes} B for {count} strokes and fills ({:.0} B each)\npen up {ended:.1} ms{}, attach {attach:.1} ms, commit {commit:.1} ms, save {:.1} ms",
            bytes as f64 / count.max(1) as f64,
            if repainted { " (tapered repaint)" } else { "" },
            ms(saving)
        );
        self.measure("stroke", text);
        Ok(())
    }

    fn attaching(&self, stroke: &Stroke, def: &BrushDef) -> Vec<u32> {
        let marks = self.scene.active_marks();
        klad_core::attach::targets(marks, marks.len(), stroke, |id| match id == stroke.brush {
            true => Some(def),
            false => self.brushes.def(id).or_else(|| self.embedded.get(id)),
        })
    }

    fn set_colour(&mut self, rgba: [f32; 4]) {
        self.color = rgba;
        self.hsva = Hsva::from_rgba(rgba, self.hsva.h);
    }

    fn used_colour(&mut self) {
        if self.settings.used(&color::hex(self.color)) {
            self.settings_dirty = true;
        }
    }

    fn pick_colour(&mut self, at: Vec2, layer_only: bool) -> Result<()> {
        let where_from = if layer_only { "this layer" } else { "the image" };
        match self.scene.sample((at.x, at.y), layer_only)? {
            Some(rgba) => {
                self.pick_new_colour(rgba);
                self.say(Tone::Done, format!("picked {} from {where_from}", color::hex(rgba)));
            }
            None => self.say(Tone::Refused, format!("nothing to pick from {where_from} there")),
        }
        Ok(())
    }

    fn fill_reference(&self) -> FillRef {
        match &self.fill.reference {
            FillRef::Layer(id) if self.scene.active_id().as_ref() == Some(id) => FillRef::This,
            FillRef::Layer(id) if self.scene.bundle.doc.layer(id).is_none() => FillRef::This,
            other => other.clone(),
        }
    }

    fn finish_fill(&mut self) -> Result<()> {
        let Some(lasso) = self.lasso.take() else {
            return Ok(());
        };
        let start = Instant::now();
        let (w, h) = self.scene.size();
        let (source, coverage) = if lasso.far && lasso.points.len() >= 3 {
            let points: Vec<[f32; 2]> = lasso.points.iter().map(|p| [p.x, p.y]).collect();
            let coverage = polygon(&points, w, h);
            (FillSource::Polygon(points), coverage)
        } else {
            let at = lasso.points[0];
            let seed = Seed {
                x: at.x,
                y: at.y,
                tolerance: self.fill.tolerance,
                gap: self.fill.gap,
                expand: self.fill.expand,
            };
            let reference = self.fill_reference();
            let image = self.scene.fill_reference(&reference, &self.brushes)?;
            let coverage = flood(&image, &seed);
            (FillSource::Seed { seed, reference }, coverage)
        };
        let computed = ms(start);
        if coverage.is_empty() {
            self.say(
                Tone::Refused,
                "nothing to fill there: click inside closed lines, or raise the gap so small openings close",
            );
            return Ok(());
        }
        if self.external.is_some() {
            self.reload()?;
        }
        let pixels = coverage.pixels();
        let lassoed = matches!(source, FillSource::Polygon(_));
        let mut fill = Fill::new(self.scene.next_mark_id(), self.color, source, coverage);
        fill.opacity = self.fill.opacity;
        fill.lock = self.scene.active_locks_alpha();
        let committing = Instant::now();
        let touched = self.scene.commit_fill(fill, &self.brushes)?;
        self.dirty.extend(touched);
        let bytes = self.scene.save_active("fill")?;
        self.first.filled = true;
        self.used_colour();
        let text = format!(
            "{}: {pixels} px computed in {computed:.0} ms, painted and saved in {:.0} ms\nlayer file {bytes} B",
            if lassoed { "lasso fill" } else { "fill" },
            ms(committing)
        );
        self.measure("fill", text);
        Ok(())
    }

    fn navigate(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect) {
        let area = rect.size();
        let (zoom, scroll, rotation, hover, space, middle, primary, delta, time) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta,
                i.rotation_delta(),
                i.pointer.hover_pos(),
                i.key_down(Key::Space),
                i.pointer.middle_down(),
                i.pointer.primary_down(),
                i.pointer.delta(),
                i.time,
            )
        });
        self.navigating = (space && !ui.ctx().egui_wants_keyboard_input()) || middle;
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let at = hover.filter(|p| rect.contains(*p)).map_or(area / 2.0, |p| p - rect.min);
        if response.hovered() || response.dragged() {
            if zoom != 1.0 {
                view.zoom_at(zoom, at, area);
            }
            if scroll != Vec2::ZERO {
                view.pan(scroll);
            }
            if rotation != 0.0 {
                view.rotate_at(rotation, at, area);
            }
        }
        if self.navigating && (middle || primary) && self.live.is_none() {
            view.pan(delta);
        }
        if self.spin {
            view.rotate_at(0.01, area / 2.0, area);
            view.zoom_at(1.0 + 0.012 * (time as f32 * 1.5).sin(), area / 2.0, area);
            ui.ctx().request_repaint();
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.canvas_rect = rect;
        let area = rect.size();
        let canvas = self.canvas_size();
        self.view.get_or_insert_with(|| View::fit(canvas, area));
        self.navigate(ui, &response, rect);
        self.keep_typing_focus(ui, &response)?;
        self.typing_keys(ui)?;
        self.pen_keys(ui)?;
        self.bone_keys(ui);
        self.shift_keys(ui)?;
        self.command_keys(ui)?;
        let ctx = ui.ctx().clone();
        let was_focused = self.focused;
        self.focused = ctx.input(|i| i.focused);
        self.modifiers = ctx.input(|i| i.modifiers);
        self.refresh_shape();
        let events = match &self.pen {
            Some(pen) => pen.drain(ctx.content_rect().height()),
            None => pen::from_egui(&ui.input(|i| i.events.clone())),
        };
        let holding = self.writing() && matches!(self.tool, Tool::Select | Tool::Direct);
        self.held
            .admit(holding, events)
            .into_iter()
            .try_for_each(|e| self.on_pen(&ctx, e, rect, was_focused))?;
        if self.tool == Tool::Bone && response.secondary_clicked() {
            self.end_chain();
        }
        if self.tool == Tool::Note
            && response.secondary_clicked()
            && let (Some(p), Some(view)) = (response.interact_pointer_pos(), self.view)
        {
            self.select_pin(view.to_canvas(p - rect.min, rect.size()))?;
        }
        let view = self.view.unwrap_or_else(|| View::fit(canvas, area));
        self.double_click_text(&response, view, rect)?;
        self.zoom_px = view.zoom * ctx.pixels_per_point();
        self.refresh_box();
        self.refresh_cut();
        self.refresh_shape();
        self.tend_path()?;
        let level = match self.zoom_px >= 1.0 {
            true => 0,
            false => (1.0 / self.zoom_px).log2().floor().clamp(0.0, 4.0) as u32,
        };
        self.frames.set_level(level);
        let show = self.tend_frames()?;
        let view_uniform = view.uniform(area, ctx.pixels_per_point(), canvas);
        let (w, h) = self.scene.size();
        let visible = self.visible_tiles(view, rect);
        let previewed = self.tend_preview(visible)?;
        let mut tiles = std::mem::take(&mut self.dirty);
        if let Some(state) = self.preview.as_ref() {
            let held: BTreeSet<usize> = tiles.intersection(&state.shown).copied().collect();
            tiles.retain(|i| !held.contains(i));
            self.dirty.extend(held);
        }
        let mut stroke: Vec<usize> = self.buffer.take_dirty();
        stroke.extend(self.mirror_buffer.take_dirty());
        if !self.canvas_current() {
            self.dirty.extend(std::mem::take(&mut tiles));
            self.dirty.extend(stroke.drain(..));
        }
        let refill = self.live.is_none() && tiles.len() > self.batch;
        if refill {
            let later = tiles.split_off(&tiles.iter().nth(self.batch).copied().unwrap_or(usize::MAX));
            self.dirty.extend(later);
            ctx.request_repaint();
        }
        let tiles: Vec<usize> = tiles.into_iter().collect();
        let start = Instant::now();
        let preview = self.live.as_ref().map(|l| Preview {
            twin: l.twin.as_ref().map(|_| &self.mirror_buffer),
            buffer: &self.buffer,
            color: l.stroke.color(),
            opacity: l.stroke.opacity,
            blend: stroke_blend(&l.brush, l.stroke.lock).unwrap_or(l.brush.blend),
        });
        let uploads = if tiles.is_empty() && stroke.is_empty() {
            Vec::new()
        } else {
            self.scene.compose_painting(&stroke, &tiles, preview.as_ref())?
        };
        if !uploads.is_empty() {
            let spent = ms(start);
            if refill {
                self.batch = ((uploads.len() as f64 * REFILL_MS / spent.max(0.1)) as usize).clamp(8, timeline::FILL);
            }
            let mut stats = lock(&self.stats);
            stats.compose_ms.push(spent as f32);
            stats.tiles.push(uploads.len() as f32);
        }
        let uploads: Tiles = previewed.into_iter().chain(uploads).collect();
        let (frame_uploads, frame_drops) = self.frames.take();
        lock(&self.outbox).send(uploads, frame_uploads, frame_drops);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            CanvasPaint {
                show: show.filter(|_| self.preview.is_none()),
                outbox: self.outbox.clone(),
                uniform: view_uniform,
                size: (w, h),
                cols: self.scene.cols(),
                pending: self.pending.take(),
                stats: self.stats.clone(),
            },
        ));
        self.parallax(ui, view, rect);
        self.prepare_typing(&ctx, view, rect);
        self.decorate(ui, view, rect);
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(|p| view.to_canvas(p - rect.min, rect.size()));
        let painter = ui.painter().with_clip_rect(rect);
        self.decorate_bones(&painter, |c| rect.min + view.to_screen(c, rect.size()), hover);
        if self.live.is_some()
            || self.lasso.is_some()
            || self.drag.is_some()
            || self.point_drag.is_some()
            || self.path_commit.is_some()
            || self
                .preview
                .as_ref()
                .is_some_and(|p| p.job.is_some() || p.finish.is_some())
        {
            ctx.request_repaint();
        }
        self.share_selection();
        Ok(())
    }

    fn decorate(&self, ui: &egui::Ui, view: View, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect);
        let accent = Color32::from_rgb(64, 196, 255);
        let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(140));
        if let Some(picked) = self.picked.as_ref().filter(|_| self.on_marks()) {
            let thin = egui::Stroke::new(1.0, accent);
            let moving = self.drag.as_ref().filter(|d| d.strokes && d.moved()).map(Drag::affine);
            let to_screen = |x: f32, y: f32| {
                let (x, y) = moving.map_or((x, y), |a| lift::affine_apply(a, (x, y)));
                self.screen(view, rect, vec2(x, y))
            };
            let parts = self.picked_parts();
            let chosen = parts
                .iter()
                .flat_map(|(_, _, marks, ids)| marks.iter().filter(|m| ids.contains(&m.id())));
            chosen.for_each(|m| match m {
                Mark::Stroke(s) => {
                    let points: Vec<(f32, f32)> = s.samples.iter().map(|v| v.pos()).collect();
                    points
                        .chunk_by(|a, b| klad_core::cut::keeps(&s.cuts, *a) == klad_core::cut::keeps(&s.cuts, *b))
                        .filter(|run| klad_core::cut::keeps(&s.cuts, run[0]))
                        .for_each(|run| {
                            let line: Vec<Pos2> = run.iter().map(|&(x, y)| to_screen(x, y)).collect();
                            painter.add(egui::Shape::line(line, thin));
                        });
                }
                Mark::Fill(f) => {
                    if let Some([x0, y0, x1, y1]) = m.bounds().or(f.bounds()) {
                        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| to_screen(x, y));
                        painter.add(egui::Shape::closed_line(corners.to_vec(), thin));
                    }
                }
            });
            if let Some(polygon) = &picked.cut {
                let line: Vec<Pos2> = polygon.iter().map(|p| to_screen(p[0], p[1])).collect();
                painter.add(egui::Shape::dashed_line(
                    &[line.clone(), line[..1].to_vec()].concat(),
                    egui::Stroke::new(1.5, accent),
                    6.0,
                    4.0,
                ));
            }
        }
        if self.tool == Tool::Direct {
            self.decorate_points(&painter, view, rect, accent, dark);
        }
        self.decorate_shape(&painter, &|x, y| self.screen(view, rect, vec2(x, y)), accent);
        self.decorate_typing(&painter, view, rect, accent);
        if let Some(axis) = self.mirror.filter(|_| self.tool == Tool::Brush) {
            let h = self.scene.size().1 as f32;
            painter.add(egui::Shape::dashed_line(
                &[
                    self.screen(view, rect, vec2(axis, 0.0)),
                    self.screen(view, rect, vec2(axis, h)),
                ],
                egui::Stroke::new(1.0, accent),
                8.0,
                6.0,
            ));
        }
        if let Some(path) = self.pen_path.as_ref() {
            let hover = ui.input(|i| i.pointer.hover_pos());
            self.decorate_pen(&painter, path, hover, view, rect, accent, dark);
        }
        if let Some((_, corners)) = self.handles() {
            let corners = match &self.drag {
                Some(drag) => drag.preview(),
                None => corners,
            };
            let screen: Vec<Pos2> = corners.iter().map(|c| self.screen(view, rect, *c)).collect();
            painter.add(egui::Shape::closed_line(screen.clone(), dark));
            painter.add(egui::Shape::closed_line(screen.clone(), egui::Stroke::new(1.5, accent)));
            screen.iter().for_each(|p| {
                painter.rect_filled(Rect::from_center_size(*p, vec2(HANDLE, HANDLE)), 1.0, Color32::WHITE);
                painter.rect_stroke(
                    Rect::from_center_size(*p, vec2(HANDLE, HANDLE)),
                    1.0,
                    egui::Stroke::new(1.0, accent),
                    egui::StrokeKind::Middle,
                );
            });
            if self.edges() {
                let flat = [0, 1, 2, 3].map(|i| screen[i].to_vec2());
                (0..4).for_each(|side| {
                    let p = edge(&flat, side).to_pos2();
                    painter.circle_filled(p, HANDLE / 2.0 - 0.5, Color32::WHITE);
                    painter.circle_stroke(p, HANDLE / 2.0 - 0.5, egui::Stroke::new(1.0, accent));
                });
            }
            let top = screen[0].lerp(screen[1], 0.5);
            let center = screen.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
            let knob = top + (top.to_vec2() - center).normalized() * KNOB;
            painter.line_segment([top, knob], egui::Stroke::new(1.5, accent));
            painter.circle_filled(knob, HANDLE / 2.0 + 1.0, Color32::WHITE);
            painter.circle_stroke(knob, HANDLE / 2.0 + 1.0, egui::Stroke::new(1.0, accent));
        }
        self.draw_pins(&painter, |c| self.screen(view, rect, c));
        self.draw_checks(&painter, |c| self.screen(view, rect, c));
        if let Some(lasso) = self.lasso.as_ref().filter(|l| l.far) {
            let line: Vec<Pos2> = lasso.points.iter().map(|p| self.screen(view, rect, *p)).collect();
            let tint = if matches!(self.tool, Tool::Strokes | Tool::Select) {
                accent
            } else {
                color::color32(self.color)
            };
            painter.add(egui::Shape::closed_line(line.clone(), dark));
            painter.add(egui::Shape::closed_line(line, egui::Stroke::new(1.5, tint)));
        }
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
        if let Some(hover) = hover.filter(|_| self.modifiers.alt && matches!(self.tool, Tool::Brush | Tool::Fill)) {
            painter.circle_stroke(hover, 9.0, egui::Stroke::new(3.0, Color32::from_black_alpha(160)));
            painter.circle_stroke(hover, 9.0, egui::Stroke::new(1.5, Color32::WHITE));
            painter.circle_filled(hover, 5.0, color::color32(self.color));
            return;
        }
        if self.tool == Tool::Fill
            && let Some(hover) = hover
        {
            let arm = |d: Vec2| [hover + d * 3.0, hover + d * 9.0];
            [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)]
                .iter()
                .for_each(|d| {
                    painter.line_segment(arm(*d), egui::Stroke::new(3.0, Color32::from_black_alpha(160)));
                    painter.line_segment(arm(*d), egui::Stroke::new(1.5, Color32::WHITE));
                });
        }
        if let Some((_, _, origin)) = self.sizing {
            let radius = (self.size * 0.5 * view.zoom).max(1.5);
            painter.circle_stroke(origin, radius, egui::Stroke::new(1.0, Color32::from_gray(30)));
            painter.circle_stroke(origin, radius + 1.0, egui::Stroke::new(1.0, Color32::from_gray(235)));
            painter.text(
                origin + vec2(radius + 8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{:.1} px", self.size),
                egui::FontId::proportional(13.0),
                Color32::WHITE,
            );
            return;
        }
        if self.tool == Tool::Brush
            && let Some(hover) = hover
        {
            let radius = (self.size * 0.5 * view.zoom).max(1.5);
            painter.circle_stroke(hover, radius, egui::Stroke::new(1.0, Color32::from_gray(30)));
            painter.circle_stroke(hover, radius + 1.0, egui::Stroke::new(1.0, Color32::from_gray(235)));
        }
    }

    fn decorate_points(&self, painter: &egui::Painter, view: View, rect: Rect, accent: Color32, dark: egui::Stroke) {
        let (Some(shaped), Some(direct)) = (self.shaped.as_ref(), self.direct.as_ref()) else {
            return;
        };
        let Some(subs) = self.shown_subs() else {
            return;
        };
        let m = shaped.matrix;
        let screen = |p: [f32; 2]| self.screen(view, rect, to_canvas(m, p));
        let step = (2.0 / (view.zoom * (m[0] * m[3] - m[1] * m[2]).abs().sqrt()).max(1e-3)).clamp(0.05, 8.0);
        pathedit::flatten(&subs, step)
            .iter()
            .zip(&subs)
            .for_each(|(line, sub)| {
                let points: Vec<Pos2> = line.iter().map(|p| screen(*p)).collect();
                let shape = if sub.closed {
                    egui::Shape::closed_line(points, egui::Stroke::new(1.5, accent))
                } else {
                    egui::Shape::line(points, egui::Stroke::new(1.5, accent))
                };
                painter.add(shape);
            });
        let anchors = pathedit::anchors(&subs);
        let thin = egui::Stroke::new(1.0, accent);
        direct.points.iter().filter_map(|&i| anchors.get(i)).for_each(|(_, a)| {
            let at = screen(a.at);
            [(a.inn, a.has_in()), (a.out, a.has_out())]
                .into_iter()
                .filter(|(_, has)| *has)
                .for_each(|(p, _)| {
                    let h = screen(p);
                    painter.line_segment([at, h], thin);
                    painter.circle_filled(h, HANDLE / 2.0 - 0.5, Color32::WHITE);
                    painter.circle_stroke(h, HANDLE / 2.0 - 0.5, thin);
                });
        });
        let size = HANDLE - 2.0;
        anchors.iter().enumerate().for_each(|(i, (_, a))| {
            let square = Rect::from_center_size(screen(a.at), vec2(size, size));
            let fill = if direct.points.contains(&i) {
                accent
            } else {
                Color32::WHITE
            };
            painter.rect_filled(square, 0.0, fill);
            painter.rect_stroke(square, 0.0, thin, egui::StrokeKind::Middle);
        });
        if let Some(drag) = self.point_drag.filter(|d| matches!(d.grab, PointGrab::Marquee { .. })) {
            let (a, b) = (self.screen(view, rect, drag.start), self.screen(view, rect, drag.now));
            let area = Rect::from_two_pos(a, b);
            painter.rect_stroke(area, 0.0, dark, egui::StrokeKind::Middle);
            painter.rect_stroke(area, 0.0, thin, egui::StrokeKind::Middle);
        }
    }

    fn chosen_marks(&self) -> Option<(Picked, Vec<(String, Mark)>)> {
        let picked = self.picked.clone()?;
        let parts = self.picked_parts();
        if parts.is_empty() {
            return None;
        }
        let chosen = parts
            .iter()
            .flat_map(|(_, cel, marks, ids)| {
                marks
                    .iter()
                    .filter(|m| ids.contains(&m.id()))
                    .map(|m| (cel.clone(), m.clone()))
            })
            .collect();
        Some((picked, chosen))
    }

    fn set_picked(&mut self, sets: Vec<(String, u32, &'static str, Value)>) -> Result<()> {
        let Some(picked) = self.picked.clone().filter(|_| !sets.is_empty()) else {
            return Ok(());
        };
        let start = Instant::now();
        let (mut ops, after) = match &picked.cut {
            Some(_) => self
                .picked_ops(LassoAction::Transform(MarkTransform::default()))
                .ok_or_else(|| anyhow!("the selection changed underneath: select the strokes again"))?,
            None => (Vec::new(), Some(picked.clone())),
        };
        ops.extend(sets.into_iter().map(|(cel, id, key, value)| Op::Set {
            id: address(&cel, id),
            props: Map::from_iter([(key.to_string(), value)]),
        }));
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        self.measure("stroke edit", format!("stroke edit redrawn in {:.0} ms", ms(start)));
        Ok(())
    }

    pub(super) fn recolourable(&self) -> Option<String> {
        let id = self
            .selection
            .clone()
            .or_else(|| self.direct.as_ref().map(|d| d.node.clone()))?;
        match self.scene.bundle.doc.item(&id) {
            Some(Item::Node(_, _, node))
                if matches!(
                    node.shape,
                    Shape::Rect { .. } | Shape::Ellipse { .. } | Shape::Path { .. } | Shape::Text(_)
                ) =>
            {
                Some(id)
            }
            _ => None,
        }
    }

    fn recolour_shape(&mut self, outline: bool) -> Result<()> {
        let Some(id) = self.recolourable() else {
            return Ok(());
        };
        let hex = color::hex(self.color);
        let has_outline = matches!(
            self.scene.bundle.doc.item(&id),
            Some(Item::Node(_, _, node)) if node.stroke.is_some()
        );
        let props = match (outline, has_outline) {
            (false, _) => json!({"fill": hex}),
            (true, true) => json!({"stroke.paint": hex}),
            (true, false) => json!({"stroke": {"paint": hex, "width": self.pen_width}}),
        };
        let props = props.as_object().cloned().unwrap_or_default();
        self.apply(vec![Op::Set { id, props }])
    }

    pub(super) fn solo(&mut self, id: &str) -> Result<()> {
        let ops = match self.soloed.take() {
            Some((was, before)) if was == id => before
                .into_iter()
                .filter(|(layer, visible)| {
                    self.scene
                        .bundle
                        .doc
                        .layer(layer)
                        .is_some_and(|l| l.visible != *visible)
                })
                .map(|(layer, visible)| Op::Set {
                    id: layer,
                    props: Map::from_iter([("visible".to_string(), json!(visible))]),
                })
                .collect(),
            _ => {
                let before = self
                    .scene
                    .bundle
                    .doc
                    .all_layers()
                    .iter()
                    .map(|l| (l.id.clone(), l.visible))
                    .collect();
                let ops = klad_core::ops::solo(&self.scene.bundle.doc, id)?;
                self.soloed = Some((id.to_string(), before));
                ops
            }
        };
        if ops.is_empty() {
            return Ok(());
        }
        self.apply(ops)
    }

    fn recolour_picked(&mut self) -> Result<()> {
        let target = self.color;
        let sets = self.chosen_marks().map_or(Vec::new(), |(_, chosen)| {
            let colours: Vec<_> = chosen.iter().map(|(_, m)| m.color()).collect();
            klad_core::color::recolour(&colours, target)
                .into_iter()
                .zip(&chosen)
                .map(|(c, (cel, m))| (cel.clone(), m.id(), "color", json!(klad_core::color::to_hex(c))))
                .collect()
        });
        self.set_picked(sets)
    }

    fn attached_parts(&self) -> Vec<(String, String, Vec<u32>)> {
        self.picked_parts()
            .into_iter()
            .map(|(layer, cel, marks, ids)| {
                let chosen: BTreeSet<u32> = ids.iter().copied().collect();
                let attached = klad_core::stroke::strokes_of(marks)
                    .filter(|s| !chosen.contains(&s.id) && s.on.iter().any(|t| chosen.contains(t)))
                    .map(|s| s.id)
                    .collect();
                (layer, cel, attached)
            })
            .collect()
    }

    pub(super) fn attached_count(&self) -> usize {
        self.attached_parts().iter().map(|(_, _, ids)| ids.len()).sum()
    }

    pub(super) fn detachable(&self) -> bool {
        self.attached_count() > 0
            || self.picked_parts().iter().any(|(_, _, marks, ids)| {
                let live: BTreeSet<u32> = marks.iter().map(Mark::id).collect();
                klad_core::stroke::strokes_of(marks)
                    .any(|s| ids.contains(&s.id) && s.on.iter().any(|t| live.contains(t)))
            })
    }

    fn select_attached(&mut self) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            return Ok(());
        };
        if picked.cut.is_some() {
            self.say(
                Tone::Refused,
                "the lasso cut has not happened yet: move or edit the strokes first, then select what is attached",
            );
            return Ok(());
        }
        let parts = self.attached_parts();
        let count: usize = parts.iter().map(|(_, _, ids)| ids.len()).sum();
        let mut parts = parts.into_iter();
        let Some((_, cel, ids)) = parts.next() else {
            return Ok(());
        };
        self.picked = Some(Picked {
            more: parts
                .filter(|(_, _, ids)| !ids.is_empty())
                .map(|(layer, cel, ids)| Part { layer, cel, ids })
                .collect(),
            ..Picked::whole(cel, ids)
        });
        self.say(
            Tone::Done,
            format!("{count} attached selected: they move and go on their own now, and attach to what they land on"),
        );
        Ok(())
    }

    fn detach_picked(&mut self) -> Result<()> {
        let Some(picked) = self.picked.clone() else {
            return Ok(());
        };
        let count = self.attached_count();
        let (mut ops, after) = match &picked.cut {
            Some(_) => self
                .picked_ops(LassoAction::Transform(MarkTransform::default()))
                .ok_or_else(|| anyhow!("the selection changed underneath: select the strokes again"))?,
            None => (Vec::new(), Some(picked.clone())),
        };
        let ids: Vec<String> = after
            .iter()
            .flat_map(|p| {
                std::iter::once((p.cel.clone(), p.ids.clone()))
                    .chain(p.more.iter().map(|m| (m.cel.clone(), m.ids.clone())))
            })
            .flat_map(|(cel, ids)| ids.into_iter().map(move |id| address(&cel, id)))
            .collect();
        ops.push(Op::Detach { ids });
        self.apply(ops)?;
        self.picked = after;
        self.forget_missing_selection();
        self.say(
            Tone::Done,
            format!("{count} detached: they stay where they are and act on whatever is under them"),
        );
        Ok(())
    }

    fn size_picked(&mut self, k: f32) -> Result<()> {
        let sets = self.chosen_marks().map_or(Vec::new(), |(_, chosen)| {
            chosen
                .iter()
                .filter_map(|(cel, m)| {
                    m.as_stroke()
                        .map(|s| (cel.clone(), s.id, "size", json!((s.size * k).max(0.1))))
                })
                .collect()
        });
        self.set_picked(sets)
    }

    fn flip_picked(&mut self, sx: f32, sy: f32) -> Result<()> {
        let middle = self
            .picked_corners()
            .map(|(_, c)| [(c[0].x + c[2].x) / 2.0, (c[0].y + c[2].y) / 2.0]);
        let start = Instant::now();
        self.edit_picked(LassoAction::Transform(MarkTransform {
            sx,
            sy,
            pivot: middle,
            ..Default::default()
        }))?;
        self.measure("stroke edit", format!("stroke edit redrawn in {:.0} ms", ms(start)));
        Ok(())
    }

    fn select_all_strokes(&mut self) {
        let ids: Vec<u32> = self.scene.active_marks().iter().map(Mark::id).collect();
        let more: Vec<Part> = self
            .also_cels()
            .into_iter()
            .map(|(layer, cel)| Part {
                ids: self.scene.marks_of(&cel).iter().map(Mark::id).collect(),
                layer,
                cel,
            })
            .filter(|p| !p.ids.is_empty())
            .collect();
        if let Some(cel) = self.scene.active_cel().filter(|_| !ids.is_empty() || !more.is_empty()) {
            self.tool = Tool::Strokes;
            self.picked = Some(Picked {
                more,
                ..Picked::whole(cel, ids)
            });
        }
    }

    fn strokes_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some((picked, chosen)) = self.chosen_marks() else {
            ui.label(match self.also.is_empty() {
                true => "click a stroke or drag a loop around strokes on the active paint layer (shift adds); cmd click layers in the layers panel to select on several at once".to_string(),
                false => format!("click a stroke or drag a loop: it selects on the active layer and {} more (cmd clicked in the layers panel)", self.also.len()),
            });
            return Ok(());
        };
        let fills = chosen.iter().filter(|(_, m)| m.as_fill().is_some()).count();
        let place = match picked.more.len() {
            0 => self
                .scene
                .bundle
                .doc
                .paint_cels()
                .into_iter()
                .find(|(_, c)| c.id == picked.cel)
                .map_or_else(String::new, |(l, c)| format!("on {}, frame {}", l.name, c.frame)),
            n => format!("on {} layers", n + 1),
        };
        let attached = match self.attached_count() {
            0 => String::new(),
            n => format!(", {n} attached"),
        };
        ui.label(format!(
            "{} strokes, {fills} fills{attached} selected {place}{}",
            chosen.len() - fills,
            if picked.cut.is_some() {
                "\n(with the parts crossing the lasso edge)"
            } else {
                ""
            }
        ));
        ui.small("the bar under the selection edits it; drag inside the box to move, a corner to scale, an edge to stretch (past the other edge flips), the knob to turn; arrows nudge");
        ui.separator();
        ui.label(format!("part: {}", self.picked_part_names().join(", ")));
        let mut name = false;
        ui.horizontal(|ui| {
            let label = ui.label("name");
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut self.checks.part)
                        .desired_width(120.0)
                        .hint_text("leaf, near leg"),
                )
                .labelled_by(label.id)
                .hint("the part's name; Return or the button names the selected marks");
            name = ui
                .button("make them this part")
                .hint("name the selected marks as this part, as klad part does")
                .clicked()
                || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        });
        ui.small(
            "in-betweens pair marks by part and klad arcs tracks them; an empty name takes the marks out of their part",
        );
        if name {
            self.name_part()?;
        }
        Ok(())
    }

    fn path_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let Some(direct) = self.direct.clone() else {
            ui.label("click a stroke, path, rect or ellipse to edit its points");
            return Ok(());
        };
        if markedit::parse_address(&direct.node).is_some() {
            let points = self
                .shaped
                .as_ref()
                .map_or(0, |s| s.subs.iter().map(|p| p.anchors.len()).sum());
            ui.label(format!("a stroke: {points} points fitted to it"));
            ui.small("drag a point or a handle to bend the stroke (it keeps its pressure); alt click the line adds a point; Delete removes the selected points");
            return Ok(());
        }
        let Some(Item::Node(layer, _, node)) = self.scene.bundle.doc.item(&direct.node) else {
            self.direct = None;
            return Ok(());
        };
        let (kind, layer_name, is_path) = (
            node.type_name(),
            layer.name.clone(),
            matches!(node.shape, Shape::Path { .. }),
        );
        let total = self
            .shaped
            .as_ref()
            .map_or(0, |s| s.subs.iter().map(|p| p.anchors.len()).sum::<usize>());
        ui.label(format!(
            "{kind}{} on {layer_name}: {total} points, {} selected",
            if node.name.is_empty() {
                String::new()
            } else {
                format!(" \"{}\"", node.name)
            },
            direct.points.len()
        ));
        if !is_path {
            ui.label(format!("any point edit turns this {kind} into a path"));
        }
        if self.under.len() > 1 && self.under.contains(&direct.node) {
            ui.label("under the last click (clicking the shape again steps down):");
            let doc = &self.scene.bundle.doc;
            let rows: Vec<(String, String)> = self
                .under
                .iter()
                .take(12)
                .map(|id| {
                    let what = match doc.item(id) {
                        Some(Item::Node(_, _, n)) => format!("{id} {} {}", n.type_name(), n.name),
                        _ => id.clone(),
                    };
                    (id.clone(), what)
                })
                .collect();
            let mut pick = None;
            rows.into_iter().for_each(|(id, what)| {
                if ui.selectable_label(id == direct.node, what).clicked() {
                    pick = Some(id);
                }
            });
            if let Some(id) = pick {
                self.pick_shape(id);
                return Ok(());
            }
        }
        ui.small(
            "drag points or handles; shift click adds a point to the selection, drag on empty canvas selects in a box\n\
             alt click the outline: new point; alt click a point: corner or smooth; alt drag a handle: break it",
        );
        if self.path_commit.is_some() {
            ui.label("writing the last edit...");
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn number(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        current: f32,
        range: RangeInclusive<f32>,
        speed: f64,
        hint: &str,
    ) -> Option<f32> {
        let mut value = self.edits.get(key).copied().unwrap_or(current);
        let response = ui
            .add(egui::DragValue::new(&mut value).speed(speed).range(range).prefix(label))
            .hint(hint);
        if response.dragged() || response.has_focus() {
            self.edits.insert(key.to_string(), value);
            return None;
        }
        let pending = self.edits.remove(key);
        if response.drag_stopped() || response.lost_focus() || response.changed() {
            return Some(value);
        }
        pending.filter(|p| (p - current).abs() > f32::EPSILON)
    }

    fn text(&mut self, ui: &mut egui::Ui, key: &str, label: &str, current: &str, hint: &str) -> Option<String> {
        let mut value = self.texts.get(key).cloned().unwrap_or_else(|| current.to_string());
        let label = ui.label(label);
        let response = ui.text_edit_singleline(&mut value).labelled_by(label.id).hint(hint);
        if response.has_focus() {
            self.texts.insert(key.to_string(), value);
            return None;
        }
        self.texts.remove(key);
        (response.lost_focus() && value != current).then_some(value)
    }

    fn node_inspector(&mut self, ui: &mut egui::Ui, id: &str) -> Result<()> {
        let Some(Item::Node(layer, _, node)) = self.scene.bundle.doc.item(id) else {
            self.selection = None;
            return Ok(());
        };
        let (node, name) = (node.clone(), layer.name.clone());
        ui.label(format!("{} on {name}", node.type_name()));
        let mut props: Vec<(String, Value)> = Vec::new();
        ui.horizontal(|ui| {
            if let Some(name) = self.text(
                ui,
                &format!("{id}.name"),
                "name",
                &node.name,
                "the shape's name; Return keeps it",
            ) {
                props.push(("name".into(), json!(name)));
            }
        });
        if let Some(paint) = self.paint_editor(ui, id, &node) {
            props.push(("fill".into(), serde_json::to_value(paint)?));
        }
        if let Shape::Rays(rays) = &node.shape {
            let changed = ui.horizontal_wrapped(|ui| self.rays_section(ui, id, rays)).inner;
            if let Some(Value::Object(fields)) = changed {
                props.extend(fields);
            }
        }
        props.extend(self.shape_fields(ui, id, &node));
        let t = node.transform;
        ui.horizontal(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.x"),
                "x ",
                t.x,
                -1e5..=1e5,
                1.0,
                "move it across, in canvas px",
            ) {
                props.push(("transform.x".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.y"),
                "y ",
                t.y,
                -1e5..=1e5,
                1.0,
                "move it down, in canvas px",
            ) {
                props.push(("transform.y".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.r"),
                "rotate ",
                t.rotate,
                -3600.0..=3600.0,
                0.5,
                "turn it, in degrees clockwise",
            ) {
                props.push(("transform.rotate".into(), json!(v)));
            }
        });
        ui.horizontal(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.sx"),
                "sx ",
                t.sx,
                -100.0..=100.0,
                0.01,
                "scale across; negative mirrors",
            ) {
                props.push(("transform.sx".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.sy"),
                "sy ",
                t.sy,
                -100.0..=100.0,
                0.01,
                "scale up and down; negative mirrors",
            ) {
                props.push(("transform.sy".into(), json!(v)));
            }
        });
        ui.horizontal(|ui| {
            if let Some(v) = self.number(
                ui,
                &format!("{id}.o"),
                "opacity ",
                node.opacity,
                0.0..=1.0,
                0.01,
                "the shape's opacity",
            ) {
                props.push(("opacity".into(), json!(v)));
            }
            if let Some(v) = self.number(
                ui,
                &format!("{id}.b"),
                "blur ",
                node.blur,
                0.0..=200.0,
                0.2,
                "soften the whole shape, in px",
            ) {
                props.push(("blur".into(), json!(v)));
            }
        });
        let mut remove = false;
        ui.horizontal(|ui| {
            let mut visible = node.visible;
            if ui
                .checkbox(&mut visible, "visible")
                .hint("show or hide this shape")
                .changed()
            {
                props.push(("visible".into(), json!(visible)));
            }
            remove = ui.button("delete").hint("remove this shape (Delete)").clicked();
            if ui.button("deselect").hint("clear the selection (Esc)").clicked() {
                self.selection = None;
            }
        });
        self.set(id, props)?;
        if remove {
            self.selection = None;
            self.apply(vec![Op::Remove {
                ids: vec![id.to_string()],
            }])?;
            return Ok(());
        }
        self.animation_section(ui, id)
    }

    fn paint_editor(&mut self, ui: &mut egui::Ui, id: &str, node: &Node) -> Option<Paint> {
        let first = |stops: &[Stop]| stops.first().map_or("#000000".to_string(), |s| s.1.clone());
        let (kind, base) = match &node.fill {
            Some(Paint::Solid(hex)) if hex == "none" => ("none", "#000000".to_string()),
            Some(Paint::Solid(hex)) => ("solid", hex.clone()),
            None => ("solid", "#000000".to_string()),
            Some(Paint::Linear { linear }) => ("linear", first(&linear.stops)),
            Some(Paint::Radial { radial }) => ("radial", first(&radial.stops)),
        };
        let mut chosen = kind;
        let mut out: Option<Paint> = None;
        ui.horizontal(|ui| {
            ui.label("fill");
            egui::ComboBox::from_id_salt(format!("{id}.fillkind"))
                .selected_text(kind)
                .show_ui(ui, |ui| {
                    ["none", "solid", "linear", "radial"].iter().for_each(|k| {
                        ui.selectable_value(&mut chosen, *k, *k);
                    });
                })
                .response
                .hint("no fill, one colour, or a linear or radial gradient");
            if kind == "solid" {
                let mut colour = color_of(&base).unwrap_or(Color32::BLACK);
                if egui::color_picker::color_edit_button_srgba(ui, &mut colour, egui::color_picker::Alpha::OnlyBlend)
                    .hint("the fill colour")
                    .changed()
                {
                    out = Some(Paint::Solid(hex_of(colour)));
                }
                if ui
                    .button("use brush colour")
                    .hint("fill it with the current colour")
                    .clicked()
                {
                    out = Some(Paint::Solid(color::hex(self.color)));
                }
            }
        });
        if chosen != kind {
            let [x, y, w, h] = klad_render::vector::local_bounds(node)
                .ok()
                .flatten()
                .map_or([0.0, 0.0, 100.0, 100.0], |r| [r.left, r.top, r.width(), r.height()]);
            let stops = vec![Stop(0.0, base.clone()), Stop(1.0, "#ffffff".into())];
            return Some(match chosen {
                "none" => Paint::Solid("none".into()),
                "solid" => Paint::Solid(base),
                "linear" => Paint::Linear {
                    linear: Linear {
                        from: [x + w / 2.0, y],
                        to: [x + w / 2.0, y + h],
                        stops,
                    },
                },
                _ => Paint::Radial {
                    radial: Radial {
                        center: [x + w / 2.0, y + h / 2.0],
                        radius: (w.max(h) / 2.0).max(1.0),
                        stops,
                    },
                },
            });
        }
        match &node.fill {
            Some(Paint::Linear { linear }) => {
                let mut next = linear.clone();
                let mut changed = false;
                ui.horizontal(|ui| {
                    [("fx", 0, "from x "), ("fy", 1, "y ")]
                        .iter()
                        .for_each(|(k, c, label)| {
                            if let Some(v) = self.number(
                                ui,
                                &format!("{id}.g.{k}"),
                                label,
                                linear.from[*c],
                                -1e5..=1e5,
                                1.0,
                                "where the gradient starts, in the shape's own px",
                            ) {
                                next.from[*c] = v;
                                changed = true;
                            }
                        });
                });
                ui.horizontal(|ui| {
                    [("tx", 0, "to x "), ("ty", 1, "y ")].iter().for_each(|(k, c, label)| {
                        if let Some(v) = self.number(
                            ui,
                            &format!("{id}.g.{k}"),
                            label,
                            linear.to[*c],
                            -1e5..=1e5,
                            1.0,
                            "where the gradient ends, in the shape's own px",
                        ) {
                            next.to[*c] = v;
                            changed = true;
                        }
                    });
                });
                if let Some(stops) = self.stops_editor(ui, id, &linear.stops) {
                    next.stops = stops;
                    changed = true;
                }
                changed.then_some(Paint::Linear { linear: next }).or(out)
            }
            Some(Paint::Radial { radial }) => {
                let mut next = radial.clone();
                let mut changed = false;
                ui.horizontal(|ui| {
                    [("cx", 0, "centre x "), ("cy", 1, "y ")]
                        .iter()
                        .for_each(|(k, c, label)| {
                            if let Some(v) = self.number(
                                ui,
                                &format!("{id}.g.{k}"),
                                label,
                                radial.center[*c],
                                -1e5..=1e5,
                                1.0,
                                "the centre of the gradient, in the shape's own px",
                            ) {
                                next.center[*c] = v;
                                changed = true;
                            }
                        });
                    if let Some(v) = self.number(
                        ui,
                        &format!("{id}.g.r"),
                        "radius ",
                        radial.radius,
                        0.1..=1e5,
                        1.0,
                        "how far the gradient reaches",
                    ) {
                        next.radius = v;
                        changed = true;
                    }
                });
                if let Some(stops) = self.stops_editor(ui, id, &radial.stops) {
                    next.stops = stops;
                    changed = true;
                }
                changed.then_some(Paint::Radial { radial: next }).or(out)
            }
            _ => out,
        }
    }

    fn stops_editor(&mut self, ui: &mut egui::Ui, id: &str, stops: &[Stop]) -> Option<Vec<Stop>> {
        let mut next = stops.to_vec();
        let mut changed = false;
        let mut remove = None;
        stops.iter().enumerate().for_each(|(i, Stop(at, hex))| {
            ui.horizontal(|ui| {
                if let Some(v) = self.number(
                    ui,
                    &format!("{id}.g.s{i}"),
                    "stop ",
                    *at,
                    0.0..=1.0,
                    0.005,
                    "where this colour sits along the gradient, 0 to 1",
                ) {
                    next[i].0 = v;
                    changed = true;
                }
                let mut colour = color_of(hex).unwrap_or(Color32::BLACK);
                if egui::color_picker::color_edit_button_srgba(ui, &mut colour, egui::color_picker::Alpha::OnlyBlend)
                    .hint("this stop's colour")
                    .changed()
                {
                    next[i].1 = hex_of(colour);
                    changed = true;
                }
                if ui
                    .button("brush colour")
                    .hint("give this stop the current colour")
                    .clicked()
                {
                    next[i].1 = color::hex(self.color);
                    changed = true;
                }
                if stops.len() > 1 && ui.button(crate::theme::icon::CLOSE).hint("remove this stop").clicked() {
                    remove = Some(i);
                }
            });
        });
        if let Some(i) = remove {
            next.remove(i);
            changed = true;
        }
        if ui
            .button("add stop")
            .hint("a stop between the last two, in the current colour")
            .clicked()
        {
            let at = match stops {
                [.., a, b] => (a.0 + b.0) / 2.0,
                _ => 0.5,
            };
            next.push(Stop(at, color::hex(self.color)));
            changed = true;
        }
        if changed {
            next.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        changed.then_some(next)
    }

    fn activate(&mut self, path: Vec<usize>) -> Result<()> {
        if self.live.is_some() {
            return Ok(());
        }
        let id = self.scene.bundle.doc.locate_path(&path).map(|l| l.id.clone());
        let tinted = self.scene.tint_mask && (self.scene.active_is_mask() || path.last() == Some(&MASK));
        if path != self.scene.active {
            self.timeline.kept = (self.canvas_current() && !self.scene.onion_on && !tinted).then_some((
                self.scene.frame,
                self.scene.bundle.doc.rev,
                self.canvas_gen,
            ));
        }
        self.scene.set_active_later(path);
        if self.scene.onion_on {
            self.frames_changed();
        }
        self.also.retain(|a| Some(a) != id.as_ref());
        self.selection = None;
        self.picked = None;
        Ok(())
    }

    fn mask_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.scene.active_layer().is_none() {
            return Ok(());
        }
        let owner_path = self.scene.active[..self.scene.active.len() - 1].to_vec();
        let owner = self.scene.bundle.doc.layer_at(&owner_path).clone();
        ui.label(format!("the mask of {}", owner.name));
        ui.colored_label(Color32::from_rgb(255, 150, 150), self.mask_hint());
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::ToggleMask, "mask on");
            self.command_check(ui, Cmd::TintMask, "show the mask in red");
        });
        ui.horizontal_wrapped(|ui| {
            self.command_button(ui, Cmd::BackToLayer, Some("back to the layer"));
            self.command_button(ui, Cmd::RemoveMask, Some("remove mask"));
        });
        Ok(())
    }

    fn mask_hint(&self) -> &'static str {
        if self.brush == "eraser" {
            "the eraser is on: it shows the layer again where the mask hid it (E switches back to the brush)"
        } else {
            "the brush hides the layer where you paint (any colour); the eraser (E) shows it again"
        }
    }

    fn layer_inspector(&mut self, ui: &mut egui::Ui) -> Result<()> {
        if self.scene.active_is_mask() {
            return self.mask_inspector(ui);
        }
        let Some(layer) = self.scene.active_layer().cloned() else {
            ui.label("no layer selected");
            return Ok(());
        };
        let id = layer.id.clone();
        ui.label(format!("{} layer", layer.kind().name()));
        let mut props: Vec<(String, Value)> = Vec::new();
        ui.horizontal(|ui| {
            if let Some(name) = self.text(
                ui,
                &format!("{id}.name"),
                "name",
                &layer.name,
                "the layer's name; Return keeps it",
            ) {
                props.push(("name".into(), json!(name)));
            }
        });
        if layer.kind() == Kind::Bones {
            self.command_check(ui, Cmd::ToggleLocked, "locked");
            let linked = klad_core::rig::painted(&self.scene.bundle.doc, &id).len();
            ui.label(
                egui::RichText::new(format!(
                    "{} bones, {linked} layers rigged to them; draw and pose them with the bone tool (J)",
                    layer.bones.len()
                ))
                .color(crate::theme::WEAK),
            );
            return self.set(&id, props);
        }
        ui.horizontal_wrapped(|ui| {
            self.command_check(ui, Cmd::ToggleLocked, "locked");
            self.command_check(ui, Cmd::ToggleClip, "clip to layer below");
            if layer.kind() == Kind::Paint {
                self.command_check(ui, Cmd::ToggleLockAlpha, "lock alpha");
            }
        });
        ui.horizontal_wrapped(|ui| match &layer.mask {
            Some(_) => {
                self.command_check(ui, Cmd::ToggleMask, "mask on");
                self.command_button(ui, Cmd::PaintMask, Some("paint on the mask"));
                self.command_button(ui, Cmd::RemoveMask, Some("remove mask"));
            }
            None => {
                self.command_button(ui, Cmd::AddMask, Some("add mask"));
                self.command_button(ui, Cmd::AddMaskHiding, Some("add mask hiding all"));
            }
        });
        ui.horizontal_wrapped(|ui| {
            let mut reference = layer.reference;
            if ui
                .checkbox(&mut reference, "reference")
                .hint("shown here to trace over, left out of renders and exports (klad render --references shows it)")
                .changed()
            {
                props.push(("reference".into(), json!(reference)));
            }
        });
        self.set(&id, props)?;
        self.depth_section(ui, &layer)?;
        self.adjust_section(ui, &layer)?;
        self.effects_section(ui, &layer)?;
        self.animation_section(ui, &id)
    }

    fn colour_panel(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut picked: Option<[f32; 4]> = None;
        let mut hsva = self.hsva;
        ui.horizontal(|ui| {
            if color::picker(ui, &mut hsva, 150.0) {
                self.color = hsva.rgba();
                self.hsva = hsva;
            }
            ui.vertical(|ui| {
                color::swatch(ui, self.color, false, 44.0).hint("the current colour");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if let Some(text) = self.text(
                        ui,
                        "colour.hex",
                        "hex",
                        &color::hex(self.color),
                        "type #rrggbb or #rrggbbaa and press Return",
                    ) {
                        match color::rgba_of(&text) {
                            Some(rgba) => picked = Some(rgba),
                            None => self.say(
                                Tone::Refused,
                                format!("{text} is not a colour: type it as #rrggbb or #rrggbbaa, for example #c8a070"),
                            ),
                        }
                    }
                });
                let mut alpha = self.color[3];
                if ui
                    .add(egui::Slider::new(&mut alpha, 0.0..=1.0).text("alpha"))
                    .hint("how see-through the colour is")
                    .changed()
                {
                    let [r, g, b, _] = self.color;
                    self.set_colour([r, g, b, alpha]);
                }
            });
        });
        ui.label("palette");
        let edit = self.palette_row(ui);
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("name");
            ui.add(
                egui::TextEdit::singleline(&mut self.swatch_name)
                    .hint_text("optional")
                    .desired_width(110.0),
            )
            .labelled_by(label.id)
            .hint("a name for the colour; agents can use it in place of the hex");
            self.command_button(ui, Cmd::AddSwatch, Some("add colour"));
        });
        if !self.settings.recent.is_empty() {
            ui.label("recent");
            let recent = self.settings.recent.clone();
            let current = color::hex(self.color);
            ui.horizontal_wrapped(|ui| {
                recent.iter().for_each(|hex| {
                    if let Some(rgba) = color::rgba_of(hex)
                        && color::swatch(ui, rgba, hex.eq_ignore_ascii_case(&current), 24.0)
                            .hint(format!("{hex}, used lately"))
                            .clicked()
                    {
                        picked = Some(rgba);
                    }
                });
            });
        }
        if let Some(rgba) = picked {
            self.pick_new_colour(rgba);
        }
        match edit {
            Some(edit) => self.apply(vec![Op::Palette(edit)]),
            None => Ok(()),
        }
    }

    fn fill_panel(&mut self, ui: &mut egui::Ui) {
        let layers: Vec<(String, String)> = self
            .scene
            .bundle
            .doc
            .all_layers()
            .iter()
            .rev()
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let name_of = |r: &FillRef| match r {
            FillRef::This => "this layer".to_string(),
            FillRef::All => "all visible layers".to_string(),
            FillRef::Layer(id) => layers
                .iter()
                .find(|(l, _)| l == id)
                .map_or(id.clone(), |(_, name)| format!("{name} ({id})")),
        };
        let mut reference = self.fill.reference.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("inside the lines of");
            egui::ComboBox::from_id_salt("fill.ref")
                .selected_text(name_of(&reference))
                .width(170.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut reference, FillRef::This, name_of(&FillRef::This));
                    ui.selectable_value(&mut reference, FillRef::All, name_of(&FillRef::All));
                    layers.iter().for_each(|(id, _)| {
                        let r = FillRef::Layer(id.clone());
                        let text = name_of(&r);
                        ui.selectable_value(&mut reference, r, text);
                    });
                })
                .response
                .hint("the lines the fill stops at: this layer, everything visible, or a layer such as your lines");
        });
        self.fill.reference = reference;
        let f = &mut self.fill;
        ui.add(egui::Slider::new(&mut f.gap, 0.0..=fill::MAX_GAP).text("close gaps px"))
            .hint("openings in the lines up to this wide count as closed");
        ui.add(egui::Slider::new(&mut f.tolerance, 0.0..=1.0).text("tolerance"))
            .hint("how different a pixel may be from where you click and still fill");
        ui.add(egui::Slider::new(&mut f.expand, 0.0..=fill::MAX_EXPAND).text("grow under lines px"))
            .hint("the fill reaches this far under the lines, so no paper shows between line and colour");
        ui.add(egui::Slider::new(&mut f.opacity, 0.05..=1.0).text("opacity"))
            .hint("the fill's opacity");
        ui.small("click inside lines to fill, drag a loop to lasso fill");
        ui.horizontal_wrapped(|ui| {
            self.command_button(ui, Cmd::Refill, Some("refill this drawing"));
            self.command_button(ui, Cmd::ShowGaps, Some("show gaps"));
        });
        if !self.scene.can_paint() {
            ui.colored_label(Color32::LIGHT_YELLOW, "pick a paint layer to fill on");
        }
    }

    fn pen_panel(&mut self, ui: &mut egui::Ui) {
        let key = device_key(self.readout.device);
        let mut curve = self.settings.curve(&key);
        let raw = self.readout.pressure;
        let peak = self.settings.peak(&key);
        let before = curve;
        ui.spacing_mut().slider_width = 220.0;
        prefs::grid(ui, "settings pen", |ui| {
            prefs::row_label(ui, "Pen");
            ui.label(if self.readout.device == 0 {
                "not identified yet: move it away from the tablet and back".to_string()
            } else {
                key.clone()
            });
            ui.end_row();
            prefs::row_label(ui, "Pressure");
            ui.vertical(|ui| {
                ui.add(
                    egui::ProgressBar::new(raw)
                        .desired_width(280.0)
                        .text(format!("pen {raw:.3}")),
                );
                ui.add(
                    egui::ProgressBar::new(curve.apply(raw))
                        .desired_width(280.0)
                        .text(format!("recorded {:.3}", curve.apply(raw))),
                );
                ui.label(egui::RichText::new(format!("heaviest press seen {peak:.3}")).color(crate::theme::WEAK));
            });
            ui.end_row();
            prefs::row_label(ui, "Lightest");
            ui.add(egui::Slider::new(&mut curve.low, 0.0..=0.5))
                .hint("the pen's raw pressure that records as the lightest touch");
            ui.end_row();
            prefs::row_label(ui, "Heaviest");
            ui.add(egui::Slider::new(&mut curve.high, 0.05..=1.0))
                .hint("the pen's raw pressure that records as full pressure");
            ui.end_row();
            prefs::row_label(ui, "Curve");
            ui.add(egui::Slider::new(&mut curve.gamma, 0.3..=3.0).logarithmic(true))
                .hint("below 1 light presses record heavier, above 1 lighter");
            ui.end_row();
            prefs::row_label(ui, "");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("heaviest = my peak")
                    .hint("set heaviest to the hardest press seen with this pen")
                    .clicked()
                    && peak > 0.05
                {
                    curve.high = peak;
                }
                if ui.button("reset").hint("the default curve").clicked() {
                    curve = PressureCurve::default();
                }
                if ui
                    .button("forget peak")
                    .hint("start measuring the hardest press again")
                    .clicked()
                    && let Some(pen) = self.settings.pens.get_mut(&key)
                {
                    pen.peak = 0.0;
                    self.settings_dirty = true;
                }
            });
            ui.end_row();
            prefs::row_label(ui, "Response");
            let (rect, _) = ui.allocate_exact_size(vec2(220.0, 120.0), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, Color32::from_gray(28));
            let at = |x: f32, y: f32| egui::pos2(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
            let line: Vec<Pos2> = (0..=40)
                .map(|i| i as f32 / 40.0)
                .map(|x| at(x, curve.apply(x)))
                .collect();
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, Color32::LIGHT_BLUE)));
            painter.circle_filled(at(raw, curve.apply(raw)), 3.5, Color32::YELLOW);
            ui.end_row();
        });
        if curve != before {
            curve.high = curve.high.max(curve.low + 0.05);
            self.settings.set_curve(&key, curve);
            self.settings_dirty = true;
        }
    }
}

impl Editor {
    #[cfg(test)]
    pub(crate) fn redraw_canvas(&mut self) {
        self.dirty.extend(self.scene.all_tiles());
    }

    pub(crate) fn clear(&self) -> [f32; 4] {
        crate::theme::surround(self.settings.defaults.surround)
    }

    pub(crate) fn take_tabs(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if !ctx.egui_wants_keyboard_input() {
            self.tabs.extend(crate::layout::take_tabs(raw_input));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grab_finds_every_handle_where_the_press_lands() {
        use egui::pos2;
        let screen = [
            pos2(100.0, 100.0),
            pos2(300.0, 100.0),
            pos2(300.0, 200.0),
            pos2(100.0, 200.0),
        ];
        assert_eq!(grab(screen, pos2(200.0, 100.0 - KNOB), true), Some(Grab::Rotate));
        assert_eq!(
            grab(screen, pos2(200.0 + 9.0, 100.0 - KNOB - 6.0), true),
            Some(Grab::Rotate)
        );
        assert_eq!(grab(screen, pos2(311.0, 104.0), true), Some(Grab::Scale));
        assert_eq!(grab(screen, pos2(92.0, 207.0), false), Some(Grab::Scale));
        assert_eq!(grab(screen, pos2(200.0, 111.0), true), Some(Grab::Stretch(0)));
        assert_eq!(grab(screen, pos2(309.0, 150.0), true), Some(Grab::Stretch(1)));
        assert_eq!(grab(screen, pos2(200.0, 189.0), true), Some(Grab::Stretch(2)));
        assert_eq!(grab(screen, pos2(91.0, 150.0), true), Some(Grab::Stretch(3)));
        assert_eq!(grab(screen, pos2(309.0, 150.0), false), None);
        assert_eq!(grab(screen, pos2(295.0, 150.0), false), Some(Grab::Move));
        assert_eq!(grab(screen, pos2(200.0, 150.0), true), Some(Grab::Move));
        assert_eq!(grab(screen, pos2(330.0, 150.0), true), None);
    }

    #[test]
    fn stroke_drags_commit_what_the_preview_shows() {
        [
            Grab::Move,
            Grab::Scale,
            Grab::Rotate,
            Grab::Stretch(0),
            Grab::Stretch(1),
            Grab::Stretch(3),
        ]
        .iter()
        .for_each(|&grab| {
            let drag = Drag {
                grab,
                node: "c1:1,2".into(),
                strokes: true,
                fresh: false,
                copy: false,
                start: vec2(140.0, 80.0),
                now: vec2(190.0, 20.0),
                center: vec2(100.0, 100.0),
                corners: [
                    vec2(50.0, 50.0),
                    vec2(150.0, 50.0),
                    vec2(150.0, 150.0),
                    vec2(50.0, 150.0),
                ],
                slop: 0.5,
                crossed: true,
            };
            let committed = drag.mark_transform();
            let a = committed.affine(committed.pivot.unwrap_or([0.0, 0.0]));
            drag.affine()
                .iter()
                .zip(a)
                .for_each(|(p, c)| assert!((p - c).abs() < 1e-3, "{grab:?}: {:?} vs {a:?}", drag.affine()));
        });
    }

    #[test]
    fn points_map_between_canvas_and_the_shape() {
        let m = [0.0, -2.0, 2.0, 0.0, 100.0, 50.0];
        let local = to_local(m, vec2(80.0, 70.0));
        let back = to_canvas(m, local);
        assert!((back - vec2(80.0, 70.0)).length() < 1e-3);
        let picked = Picked::whole("c4".into(), vec![3, 12]);
        assert_eq!(picked.addresses(), ["c4:s3", "c4:s12"]);
        assert_eq!(picked.key(), "c4:3,12");
        let cut = Picked {
            cut: Some(vec![[0.0, 0.0], [5.0, 0.0], [5.0, 5.0]]),
            ..picked.clone()
        };
        assert!(cut.key().starts_with("c4:3,12:cut"));
    }

    #[test]
    fn hardness_and_curve_make_a_variant_but_feel_settings_do_not() {
        let set = BrushSet::presets();
        let ink = set.def("ink").unwrap();
        let tuning = Tuning::of(ink);
        assert_eq!(&tuning.shape(ink), ink);
        let soft = set.def("soft").unwrap();
        assert_eq!(&Tuning::of(soft).shape(soft), soft);
        let feel = Tuning {
            taper: [0.0, 0.0],
            stabilizer: 0.0,
            smoothing: 80.0,
            ..tuning
        };
        assert_eq!(&feel.shape(ink), ink);
        let harder = Tuning {
            hardness: Some(1.0),
            ..tuning
        };
        assert_eq!(harder.shape(ink).hardness(), Some(1.0));
        let lighter = Tuning {
            size_min: 0.3,
            ..tuning
        };
        let shaped = lighter.shape(ink);
        assert_eq!(shaped.dynamics.size.pressure.map(|c| c.min), Some(0.3));
        let none = BTreeMap::new();
        assert_eq!(pick_id("ink", &tuning.shape(ink), &none), "ink");
        assert!(pick_id("ink", &shaped, &none).starts_with("ink-"));
    }
}
