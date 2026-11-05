use crate::color::parse_hex;
use crate::doc::{Cel, Clip, Document, Item, Kind, Layer, LayerContent, Loc, MASK, Node, Shape, Swatch, Tween};
use crate::fill::{self, Coverage, Fill, FillRef, FillSource, Seed};
use crate::markedit::{LassoAction, MarkChange, MarkTransform, Source, parse_address};
use crate::path::{flatten, length, profile, resample};
use crate::pathedit::{self, PathEdit};
use crate::retime::{self, Retime, RetimeSpec};
use crate::rng::{FNV_OFFSET, fnv};
use crate::stroke::{Author, Mark, Sample, Stroke};
use crate::timing::{self, CelAction, CelSpec, FrameSpec};
use crate::tween::{Match, Spacing, Unpaired};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

const LAYER_KEYS: &[&str] = &[
    "id",
    "name",
    "kind",
    "opacity",
    "blend",
    "visible",
    "locked",
    "clip",
    "lockAlpha",
    "mask",
    "cels",
    "children",
    "effects",
    "adjust",
    "depth",
    "reference",
];
const CEL_KEYS: &[&str] = &["id", "frame", "hold", "link", "tween", "nodes"];
const NODE_KEYS: &[&str] = &[
    "id",
    "name",
    "type",
    "fill",
    "stroke",
    "transform",
    "opacity",
    "blur",
    "clip",
    "visible",
];
const TRANSFORM_KEYS: &[&str] = &["x", "y", "rotate", "sx", "sy", "pivot"];
const STROKE_KEYS: &[&str] = &["paint", "width", "cap", "join", "dash"];
const STRUCTURE: &[&str] = &["id", "kind", "type", "cels", "children", "nodes", "mask"];

fn shape_keys(kind: &str) -> Result<&'static [&'static str]> {
    Ok(match kind {
        "rect" => &["x", "y", "w", "h", "radius"],
        "ellipse" => &["cx", "cy", "rx", "ry"],
        "path" => &["d"],
        "group" => &["children"],
        "rays" => &[
            "x", "y", "angle", "spread", "count", "length", "start", "falloff", "noise", "seed",
        ],
        "image" => &["src", "x", "y", "w", "h"],
        "scatter" => &["x", "y", "w", "h", "count", "seed", "size", "turn", "avoid", "children"],
        "repeat" => &["count", "step", "turn", "pivot", "scale", "columns", "row", "children"],
        "mirror" => &["x", "y", "angle", "children"],
        "text" => &[
            "text", "font", "x", "y", "size", "tracking", "leading", "align", "weight",
        ],
        other => bail!(
            "unknown node type {other:?}: use rect, ellipse, path, group, rays, image, scatter, repeat, mirror or text"
        ),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Place {
    pub above: Option<String>,
    pub below: Option<String>,
    pub into: Option<String>,
    pub index: Option<usize>,
    pub frame: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    AddLayer {
        layer: Value,
        place: Place,
    },
    AddNode {
        node: Value,
        place: Place,
    },
    AddCel {
        cel: Value,
        layer: String,
    },
    AddMask {
        mask: Value,
        layer: String,
        hide: bool,
    },
    Set {
        id: String,
        props: Map<String, Value>,
    },
    Remove {
        ids: Vec<String>,
    },
    Move {
        id: String,
        place: Place,
    },
    Stroke(StrokeSpec),
    Fill(FillSpec),
    Palette(PaletteEdit),
    Transform {
        ids: Vec<String>,
        transform: MarkTransform,
    },
    Path {
        id: String,
        edit: PathEdit,
    },
    Lasso(LassoSpec),
    Copy(CopySpec),
    Timeline {
        fps: Option<u32>,
        frames: Option<u32>,
        origin: Option<Option<[f32; 2]>>,
    },
    Atmosphere(Option<crate::look::Atmosphere>),
    Frames(FrameSpec),
    Cel(CelSpec),
    Retime(RetimeSpec),
    Inbetween(InbetweenSpec),
    Ink(InkSpec),
    Duplicate {
        layer: String,
    },
    Part {
        ids: Vec<String>,
        name: String,
    },
    Light(LightSpec),
    Hatch(HatchSpec),
    Paint(PaintSpec),
    Scatter(ScatterSpec),
    Foliage(FoliageSpec),
    Grass(GrassSpec),
    PaintOver(PaintOverSpec),
    Bone(BoneSpec),
    Weights(WeightsSpec),
    Pose(PoseSpec),
    Clip(ClipEdit),
    Export {
        godot: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipEdit {
    Add(Clip),
    Set {
        name: String,
        rename: Option<String>,
        from: Option<u32>,
        to: Option<u32>,
        looping: Option<bool>,
        fps: Option<Option<u32>>,
    },
    Remove {
        name: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InbetweenSpec {
    pub layers: Vec<String>,
    pub from: Option<u32>,
    pub to: Option<u32>,
    pub count: Option<u32>,
    pub spacing: Spacing,
    pub matching: Match,
    pub pairs: Vec<[u32; 2]>,
    pub redo: bool,
    pub force: bool,
    pub looping: bool,
    pub keep_drawn: bool,
    pub unpaired: Unpaired,
    pub only: Vec<String>,
    pub keep_cels: Vec<String>,
    pub force_cels: Vec<String>,
    pub resolved: Option<Inbetweens>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inbetweens {
    pub cels: Vec<TweenCel>,
    pub holds: Vec<(String, u32)>,
    pub removed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TweenCel {
    pub layer: String,
    pub cel: String,
    pub frame: u32,
    pub hold: Option<u32>,
    pub tween: Tween,
    pub marks: Vec<Mark>,
    pub fresh: bool,
    pub parts: BTreeMap<String, Vec<u32>>,
    pub pose: Option<crate::rig::Pose>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CopyTarget {
    Cel(String),
    Frame { frame: u32, layer: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CopySpec {
    pub ids: Vec<String>,
    pub to: CopyTarget,
    pub transform: Option<MarkTransform>,
    pub cut: Option<Vec<[f32; 2]>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LassoSpec {
    pub layer: String,
    pub frame: u32,
    pub polygon: Vec<[f32; 2]>,
    pub cut: bool,
    pub action: LassoAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteEdit {
    Add {
        color: String,
        name: String,
        index: Option<usize>,
    },
    Remove {
        key: String,
    },
    Set {
        swatches: Vec<Swatch>,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default)]
    pub at: Option<[f32; 2]>,
    #[serde(default)]
    pub polygon: Option<Vec<[f32; 2]>>,
    #[serde(default, rename = "ref")]
    pub reference: Option<String>,
    #[serde(default = "default_tolerance")]
    pub tolerance: f32,
    #[serde(default)]
    pub gap: f32,
    #[serde(default = "default_expand")]
    pub expand: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "full")]
    pub opacity: f32,
    #[serde(default)]
    pub like: Option<String>,
    #[serde(default)]
    pub frames: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub outside: Option<Vec<[f32; 2]>>,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(skip)]
    pub coverage: Option<Coverage>,
    #[serde(skip)]
    pub carried: Option<Vec<(String, Vec<Mark>)>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default = "default_brush")]
    pub brush: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_size")]
    pub size: f32,
    #[serde(default = "full")]
    pub opacity: f32,
    #[serde(default = "full")]
    pub flow: f32,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub points: Option<Vec<[f32; 3]>>,
    #[serde(default)]
    pub pressure: Vec<f32>,
    #[serde(default)]
    pub tilt: [f32; 2],
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default)]
    pub seed: Option<u32>,
    #[serde(default)]
    pub taper: Option<[f32; 2]>,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<String>,
}

fn default_brush() -> String {
    "ink".into()
}

fn default_color() -> String {
    "#000000".into()
}

fn default_size() -> f32 {
    8.0
}

fn default_speed() -> f32 {
    700.0
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InkSpec {
    pub layer: String,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub frame: u32,
    #[serde(default = "ink_weight")]
    pub weight: f32,
    #[serde(default = "default_light")]
    pub light: [f32; 2],
    #[serde(default = "ink_break", rename = "break")]
    pub break_deg: f32,
    #[serde(default)]
    pub gap: f32,
    #[serde(default = "ink_wobble")]
    pub wobble: f32,
    #[serde(default)]
    pub taper: Option<[f32; 2]>,
    #[serde(default = "default_brush")]
    pub brush: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "full")]
    pub opacity: f32,
    #[serde(default = "full")]
    pub hand: f32,
    #[serde(default = "first_seed")]
    pub seed: u32,
    #[serde(default)]
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LightSpec {
    pub from: Vec<String>,
    #[serde(default)]
    pub frame: u32,
    #[serde(default = "default_light")]
    pub light: [f32; 2],
    #[serde(default)]
    pub lamp: Option<[f32; 3]>,
    #[serde(default)]
    pub shade: Option<String>,
    #[serde(default)]
    pub rim: Option<String>,
    #[serde(default = "light_depth")]
    pub depth: f32,
    #[serde(default = "light_soft")]
    pub soft: u32,
    #[serde(default = "rim_width")]
    pub rim_width: f32,
    #[serde(default = "shade_color")]
    pub shade_color: String,
    #[serde(default = "rim_color")]
    pub rim_color: String,
    #[serde(default = "shade_opacity")]
    pub shade_opacity: f32,
    #[serde(default = "rim_opacity")]
    pub rim_opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HatchSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default, rename = "in")]
    pub within: Vec<String>,
    #[serde(default)]
    pub polygon: Option<Vec<[f32; 2]>>,
    #[serde(default = "hatch_angle")]
    pub angle: f32,
    #[serde(default)]
    pub along: bool,
    #[serde(default = "hatch_spacing")]
    pub spacing: f32,
    #[serde(default = "hatch_length")]
    pub length: [f32; 2],
    #[serde(default = "hatch_jitter")]
    pub jitter: f32,
    #[serde(default = "texture_weight")]
    pub weight: f32,
    #[serde(default = "default_brush")]
    pub brush: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "full")]
    pub opacity: f32,
    #[serde(default = "full")]
    pub hand: f32,
    #[serde(default = "first_seed")]
    pub seed: u32,
    #[serde(default)]
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PaintSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default, rename = "in")]
    pub within: Vec<String>,
    #[serde(default)]
    pub polygon: Option<Vec<[f32; 2]>>,
    #[serde(default = "paint_brush")]
    pub brush: String,
    #[serde(default = "paint_bold")]
    pub bold: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "paint_form")]
    pub form: String,
    #[serde(default = "paint_axis")]
    pub axis: f32,
    #[serde(default = "paint_facing")]
    pub facing: [f32; 3],
    #[serde(default = "default_light")]
    pub light: [f32; 2],
    #[serde(default = "paint_elevation")]
    pub elevation: f32,
    #[serde(default)]
    pub lamp: Option<[f32; 3]>,
    #[serde(default = "paint_key")]
    pub key: String,
    #[serde(default = "paint_fill")]
    pub fill: String,
    #[serde(default = "paint_fill_strength")]
    pub fill_strength: f32,
    #[serde(default = "paint_bounce")]
    pub bounce: String,
    #[serde(default = "paint_bounce_strength")]
    pub bounce_strength: f32,
    #[serde(default = "paint_core")]
    pub core: f32,
    #[serde(default)]
    pub shadow: Option<String>,
    #[serde(default = "paint_shadow_mix")]
    pub shadow_mix: f32,
    #[serde(default = "hatch_angle")]
    pub angle: f32,
    #[serde(default = "paint_weight")]
    pub weight: f32,
    #[serde(default = "full")]
    pub decisive: f32,
    #[serde(default = "paint_vary")]
    pub vary: f32,
    #[serde(default = "full")]
    pub hand: f32,
    #[serde(default = "full")]
    pub opacity: f32,
    #[serde(default = "first_seed")]
    pub seed: u32,
    #[serde(default)]
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScatterSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default, rename = "in")]
    pub within: Vec<String>,
    #[serde(default)]
    pub polygon: Option<Vec<[f32; 2]>>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default = "scatter_mark")]
    pub mark: String,
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default = "scatter_spacing")]
    pub spacing: f32,
    #[serde(default = "scatter_size")]
    pub size: [f32; 2],
    #[serde(default = "scatter_turn")]
    pub turn: [f32; 2],
    #[serde(default = "texture_weight")]
    pub weight: f32,
    #[serde(default = "default_brush")]
    pub brush: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "full")]
    pub opacity: f32,
    #[serde(default = "full")]
    pub hand: f32,
    #[serde(default = "first_seed")]
    pub seed: u32,
    #[serde(default)]
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoliageSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default, rename = "in")]
    pub within: Vec<String>,
    #[serde(default = "foliage_color")]
    pub color: String,
    #[serde(default)]
    pub lit: Option<String>,
    #[serde(default)]
    pub shade: Option<String>,
    #[serde(default)]
    pub glow: Option<String>,
    #[serde(default = "default_light")]
    pub light: [f32; 2],
    #[serde(default)]
    pub lamp: Option<[f32; 3]>,
    #[serde(default = "foliage_leaf")]
    pub leaf: f32,
    #[serde(default)]
    pub mass: Option<String>,
    #[serde(default = "full")]
    pub density: f32,
    #[serde(default = "foliage_brush")]
    pub brush: String,
    #[serde(default = "foliage_body")]
    pub body: bool,
    #[serde(default = "first_seed")]
    pub seed: u32,
    #[serde(default)]
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrassSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default, rename = "in")]
    pub within: Vec<String>,
    #[serde(default = "grass_color")]
    pub color: String,
    #[serde(default)]
    pub lit: Option<String>,
    #[serde(default)]
    pub shade: Option<String>,
    #[serde(default = "default_light")]
    pub light: [f32; 2],
    #[serde(default)]
    pub lamp: Option<[f32; 3]>,
    #[serde(default = "grass_blade")]
    pub blade: f32,
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default = "grass_far")]
    pub far: f32,
    #[serde(default = "grass_wind")]
    pub wind: f32,
    #[serde(default = "full")]
    pub density: f32,
    #[serde(default = "grass_edge")]
    pub edge: f32,
    #[serde(default)]
    pub patch: Option<f32>,
    #[serde(default = "grass_brush")]
    pub brush: String,
    #[serde(default)]
    pub kinds: Vec<GrassKind>,
    #[serde(default = "first_seed")]
    pub seed: u32,
    #[serde(default)]
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoneSpec {
    pub layer: String,
    #[serde(default)]
    pub add: Option<String>,
    #[serde(default)]
    pub rm: Option<String>,
    #[serde(default)]
    pub rename: Option<String>,
    #[serde(default, rename = "as")]
    pub to_name: Option<String>,
    #[serde(default, rename = "move")]
    pub moved: Option<String>,
    #[serde(default)]
    pub reparent: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub head: Option<[f32; 2]>,
    #[serde(default)]
    pub tail: Option<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightsSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub bone: Option<String>,
    #[serde(default)]
    pub set: Vec<[f32; 4]>,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default = "full")]
    pub value: f32,
    #[serde(default)]
    pub smooth: bool,
    #[serde(skip)]
    pub resolved: Option<crate::rig::Weights>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoseSpec {
    pub layer: String,
    #[serde(default)]
    pub from: u32,
    pub to: u32,
    #[serde(default)]
    pub bones: BTreeMap<String, f32>,
    #[serde(default)]
    pub root: [f32; 2],
    #[serde(default)]
    pub hold: Option<u32>,
    #[serde(skip)]
    pub resolved: Option<Vec<PosedCel>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PosedCel {
    pub layer: String,
    pub cel: String,
    pub frame: u32,
    pub hold: Option<u32>,
    pub pose: crate::rig::Pose,
    pub marks: Vec<Mark>,
    pub fresh: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintOverSpec {
    pub layer: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default, rename = "in")]
    pub within: Vec<String>,
    #[serde(default = "paintover_sizes")]
    pub sizes: Vec<f32>,
    #[serde(default = "paintover_threshold")]
    pub threshold: f32,
    #[serde(default = "paintover_length")]
    pub length: [f32; 2],
    #[serde(default = "paintover_curve")]
    pub curve: f32,
    #[serde(default = "paintover_brush")]
    pub brush: String,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default = "paintover_vary")]
    pub vary: f32,
    #[serde(default)]
    pub like: Option<String>,
    #[serde(default = "paintover_part")]
    pub part: String,
    #[serde(default = "first_seed")]
    pub seed: u32,
}

fn paintover_sizes() -> Vec<f32> {
    vec![32.0, 16.0, 8.0]
}

fn paintover_threshold() -> f32 {
    6.0
}

fn paintover_length() -> [f32; 2] {
    [1.0, 10.0]
}

fn paintover_curve() -> f32 {
    0.5
}

fn paintover_brush() -> String {
    "oil".into()
}

fn paintover_vary() -> f32 {
    0.025
}

fn paintover_part() -> String {
    "paint over".into()
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrassKind {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "full")]
    pub share: f32,
    #[serde(default)]
    pub brush: Option<String>,
    #[serde(default = "full")]
    pub length: f32,
    #[serde(default = "full")]
    pub width: f32,
    #[serde(default)]
    pub curl: f32,
    #[serde(default)]
    pub spread: f32,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub hue: f32,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default = "grass_head_size")]
    pub head_size: f32,
    #[serde(default)]
    pub low: bool,
    #[serde(default = "grass_patchy")]
    pub patchy: f32,
}

fn grass_head_size() -> f32 {
    0.32
}

fn grass_patchy() -> f32 {
    0.5
}

fn grass_color() -> String {
    "#5f8a3e".into()
}

fn grass_blade() -> f32 {
    36.0
}

fn grass_far() -> f32 {
    0.45
}

fn grass_wind() -> f32 {
    8.0
}

fn grass_edge() -> f32 {
    1.35
}

fn grass_brush() -> String {
    "blade".into()
}

fn foliage_color() -> String {
    "#4f7a3c".into()
}

fn foliage_leaf() -> f32 {
    14.0
}

fn foliage_brush() -> String {
    "leaves".into()
}

fn foliage_body() -> bool {
    true
}

fn ink_weight() -> f32 {
    6.0
}

fn default_light() -> [f32; 2] {
    [-1.0, -1.0]
}

fn ink_break() -> f32 {
    60.0
}

fn ink_wobble() -> f32 {
    0.6
}

fn first_seed() -> u32 {
    1
}

fn light_depth() -> f32 {
    14.0
}

fn light_soft() -> u32 {
    3
}

fn rim_width() -> f32 {
    4.0
}

fn shade_color() -> String {
    "#5a4a6a".into()
}

fn rim_color() -> String {
    "#fff1cc".into()
}

fn shade_opacity() -> f32 {
    0.6
}

fn rim_opacity() -> f32 {
    0.85
}

fn hatch_angle() -> f32 {
    30.0
}

fn hatch_spacing() -> f32 {
    6.0
}

fn hatch_length() -> [f32; 2] {
    [12.0, 30.0]
}

fn hatch_jitter() -> f32 {
    0.3
}

fn texture_weight() -> f32 {
    2.0
}

fn paint_brush() -> String {
    "oil".into()
}

fn paint_bold() -> String {
    "oil-load".into()
}

fn paint_form() -> String {
    "round".into()
}

fn paint_axis() -> f32 {
    90.0
}

fn paint_facing() -> [f32; 3] {
    [0.0, 0.0, 1.0]
}

fn paint_elevation() -> f32 {
    0.6
}

fn paint_key() -> String {
    "#fff3e2".into()
}

fn paint_fill() -> String {
    "#8a9ab8".into()
}

fn paint_fill_strength() -> f32 {
    0.16
}

fn paint_bounce() -> String {
    "#b08a66".into()
}

fn paint_bounce_strength() -> f32 {
    0.1
}

fn paint_shadow_mix() -> f32 {
    0.7
}

fn paint_core() -> f32 {
    0.35
}

fn paint_weight() -> f32 {
    14.0
}

fn paint_vary() -> f32 {
    0.03
}

pub const PAINT_FORMS: [&str; 3] = ["round", "flat", "cylinder"];

fn scatter_mark() -> String {
    "M0 0 L0 -18".into()
}

fn scatter_spacing() -> f32 {
    14.0
}

fn scatter_size() -> [f32; 2] {
    [0.7, 1.2]
}

fn scatter_turn() -> [f32; 2] {
    [-15.0, 15.0]
}

fn full() -> f32 {
    1.0
}

fn default_tolerance() -> f32 {
    fill::TOLERANCE
}

fn default_expand() -> f32 {
    fill::EXPAND
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingMarks {
    pub layer: String,
    pub cel: String,
    pub changes: Vec<MarkChange>,
    pub sealed: usize,
}

impl PendingMarks {
    pub fn added(&self) -> impl Iterator<Item = &Mark> {
        self.changes.iter().flat_map(|c| match c {
            MarkChange::Add(marks) => &marks[..],
            _ => &[],
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub dropped_cels: Vec<(String, String)>,
    pub marks: Vec<PendingMarks>,
    pub notes: Vec<String>,
    pub weights: Vec<(String, crate::rig::Weights)>,
    pub renamed_bones: Vec<(String, String, String)>,
}

impl Applied {
    pub(crate) fn copy(&mut self, layer: &str, cel: &str, from_layer: &str, from_cel: &str) {
        let upto = self
            .marks
            .iter_mut()
            .find(|p| p.layer == from_layer && p.cel == from_cel)
            .map_or(0, |p| {
                p.sealed = p.changes.len();
                p.sealed
            });
        self.push(
            layer,
            cel,
            MarkChange::Copy {
                layer: from_layer.to_string(),
                cel: from_cel.to_string(),
                upto,
            },
        );
    }

    pub(crate) fn paste(
        &mut self,
        layer: &str,
        cel: &str,
        sources: Vec<(String, String, Vec<u32>)>,
        transform: Option<MarkTransform>,
        cut: Option<Vec<[f32; 2]>>,
    ) {
        let sources = sources
            .into_iter()
            .map(|(from_layer, from_cel, ids)| {
                let upto = self
                    .marks
                    .iter_mut()
                    .find(|p| p.layer == from_layer && p.cel == from_cel)
                    .map_or(0, |p| {
                        p.sealed = p.changes.len();
                        p.sealed
                    });
                Source {
                    layer: from_layer,
                    cel: from_cel,
                    ids,
                    upto,
                }
            })
            .collect();
        self.push(
            layer,
            cel,
            MarkChange::Paste {
                sources,
                transform,
                cut,
                made: Vec::new(),
            },
        );
    }

    fn push(&mut self, layer: &str, cel: &str, change: MarkChange) {
        let at = match self.marks.iter().position(|p| p.layer == layer && p.cel == cel) {
            Some(at) => at,
            None => {
                self.marks.push(PendingMarks {
                    layer: layer.to_string(),
                    cel: cel.to_string(),
                    changes: Vec::new(),
                    sealed: 0,
                });
                self.marks.len() - 1
            }
        };
        let pending = &mut self.marks[at];
        let open = pending.changes.len() > pending.sealed;
        match (pending.changes.last_mut(), change) {
            (Some(MarkChange::Add(last)), MarkChange::Add(more)) if open => last.extend(more),
            (_, change) => pending.changes.push(change),
        }
    }
}

impl FillSpec {
    pub fn seeded(layer: &str, at: [f32; 2]) -> Self {
        Self {
            layer: layer.to_string(),
            frame: 0,
            at: Some(at),
            polygon: None,
            reference: None,
            tolerance: fill::TOLERANCE,
            gap: 0.0,
            expand: fill::EXPAND,
            color: default_color(),
            opacity: 1.0,
            like: None,
            frames: None,
            shape: None,
            outside: None,
            part: None,
            coverage: None,
            carried: None,
        }
    }

    pub fn check(&self) -> Result<()> {
        match (&self.like, &self.at, &self.polygon) {
            (Some(_), None, None) => return Ok(()),
            (Some(_), _, _) => {
                bail!("a fill like another drawing's carries that drawing's fills; leave out at and polygon")
            }
            (None, _, _) if self.frames.is_some() => bail!("frames goes with like: the drawing whose fills to carry"),
            _ => {}
        }
        match (&self.at, &self.polygon, &self.shape) {
            (Some(_), None, None) => {}
            (None, Some(points), None) if points.len() >= 3 => {}
            (None, None, Some(_)) => {}
            (None, Some(_), None) => bail!("a polygon fill needs at least 3 points"),
            _ => bail!(
                "a fill needs one of \"at\": [x, y] (flood fill), \"polygon\": [[x, y], ...] (lasso fill) or \"shape\": <node id>"
            ),
        }
        if self.outside.as_ref().is_some_and(|p| p.len() < 3) {
            bail!("outside needs at least 3 points: the fill keeps what lies outside that polygon");
        }
        if !(0.0..=1.0).contains(&self.tolerance) || !(0.0..=1.0).contains(&self.opacity) {
            bail!("fill tolerance and opacity must be within 0..1");
        }
        if !(0.0..=fill::MAX_GAP).contains(&self.gap) || !(0.0..=fill::MAX_EXPAND).contains(&self.expand) {
            bail!(
                "fill gap must be within 0..{} px and expand within 0..{} px",
                fill::MAX_GAP,
                fill::MAX_EXPAND
            );
        }
        Ok(())
    }

    pub fn reference(&self, doc: &Document) -> Result<FillRef> {
        let reference = match self.reference.as_deref().map(str::trim) {
            None | Some("this") => FillRef::This,
            Some("all") => FillRef::All,
            Some(key) => FillRef::Layer(
                doc.find_layer(key)
                    .map_err(|e| anyhow!("fill reference: {e}; or use this or all"))?
                    .id
                    .clone(),
            ),
        };
        Ok(match reference {
            FillRef::Layer(id) if id == self.layer => FillRef::This,
            other => other,
        })
    }

    pub fn source(&self, doc: &Document) -> Result<FillSource> {
        self.check()?;
        if let Some(shape) = &self.shape {
            bail!("a fill from the shape {shape} is turned into polygons by klad fill --shape or klad apply first");
        }
        Ok(match (&self.at, &self.polygon) {
            (Some([x, y]), _) => FillSource::Seed {
                seed: Seed {
                    x: *x,
                    y: *y,
                    tolerance: self.tolerance,
                    gap: self.gap,
                    expand: self.expand,
                },
                reference: self.reference(doc)?,
            },
            (_, Some(points)) => FillSource::Polygon(points.clone()),
            _ => unreachable!("checked above"),
        })
    }

    pub fn build(&self, doc: &Document) -> Result<Fill> {
        let source = self.source(doc)?;
        let Some(coverage) = self.coverage.clone() else {
            bail!("the fill's coverage has not been computed (klad fill and klad apply compute it)");
        };
        let color = parse_hex(&doc.resolve_color(&self.color)?)?;
        let mut fill = Fill::new(0, color, source, coverage).by(Author::Agent);
        fill.opacity = self.opacity;
        if let Some(polygon) = &self.outside {
            fill.cuts.push(crate::cut::Cut {
                keep: crate::cut::Keep::Outside,
                polygon: polygon.clone(),
            });
        }
        Ok(fill)
    }
}

impl StrokeSpec {
    pub fn build(&self) -> Result<Vec<Stroke>> {
        if self.size <= 0.0 || !(0.0..=1.0).contains(&self.opacity) || self.flow <= 0.0 {
            bail!("stroke size and flow must be positive and opacity within 0..1");
        }
        if self.speed <= 0.0 {
            bail!("stroke speed must be positive");
        }
        let taper = self.taper.unwrap_or_default();
        if taper.iter().any(|t| !(0.0..=10_000.0).contains(t)) {
            bail!("taper lengths must be within 0..10000 px");
        }
        let color = parse_hex(&self.color)?;
        let second = self.second.as_deref().map(parse_hex).transpose()?;
        let step = (self.speed / 200.0).clamp(1.0, 4.0);
        let dt = step / self.speed * 1000.0;
        let lines: Vec<Vec<[f32; 3]>> = match (&self.path, &self.points) {
            (Some(d), None) => flatten(d)?
                .into_iter()
                .map(|line| {
                    let total = length(&line).max(1e-3);
                    let even = resample(&line, step);
                    let mut walked = 0.0;
                    even.iter()
                        .enumerate()
                        .map(|(i, p)| {
                            if i > 0 {
                                walked +=
                                    libm::sqrtf((p[0] - even[i - 1][0]).powi(2) + (p[1] - even[i - 1][1]).powi(2));
                            }
                            [p[0], p[1], profile(&self.pressure, walked / total)]
                        })
                        .collect()
                })
                .collect(),
            (None, Some(points)) => vec![points.clone()],
            _ => bail!("a stroke needs either \"path\" (SVG path data) or \"points\" ([[x, y, pressure], ...])"),
        };
        let base = self.seed.unwrap_or_else(|| {
            fnv(
                FNV_OFFSET,
                format!("{:?}{:?}{}", self.path, self.points, self.layer).as_bytes(),
            ) as u32
        });
        Ok(lines
            .into_iter()
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(i, line)| {
                let mut stroke =
                    Stroke::new(0, &self.brush, color, self.size, base.wrapping_add(i as u32)).by(Author::Agent);
                stroke.opacity = self.opacity;
                stroke.flow = self.flow;
                stroke.taper = taper;
                stroke.second = second.map(crate::color::to_u16);
                stroke.samples = line
                    .iter()
                    .enumerate()
                    .map(|(k, [x, y, p])| {
                        Sample::quantize(*x, *y, *p, (self.tilt[0], self.tilt[1]), if k == 0 { 0.0 } else { dt })
                    })
                    .collect();
                stroke
            })
            .collect())
    }
}

fn take_str(map: &mut Map<String, Value>, key: &str) -> Result<Option<String>> {
    match map.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => bail!("{key} must be a string id, not {other}"),
    }
}

fn take_place(map: &mut Map<String, Value>) -> Result<Place> {
    let number = |map: &mut Map<String, Value>, key: &str| -> Result<Option<u64>> {
        match map.remove(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| anyhow!("{key} must be a whole number")),
        }
    };
    Ok(Place {
        above: take_str(map, "above")?,
        below: take_str(map, "below")?,
        into: take_str(map, "in")?,
        index: number(map, "index")?.map(|v| v as usize),
        frame: number(map, "frame")?.map(|v| v as u32),
    })
}

fn no_leftovers(map: &Map<String, Value>, op: &str) -> Result<()> {
    match map.keys().next() {
        Some(key) => bail!("{op} does not take {key:?}"),
        None => Ok(()),
    }
}

impl Op {
    pub fn parse(value: Value) -> Result<Self> {
        let Value::Object(mut map) = value else {
            bail!("an op must be a JSON object");
        };
        let op = take_str(&mut map, "op")?.ok_or_else(|| anyhow!("missing \"op\" (add, set, rm or mv)"))?;
        let parsed = match op.as_str() {
            "add" => {
                if let Some(layer) = map.remove("layer") {
                    Op::AddLayer {
                        layer,
                        place: take_place(&mut map)?,
                    }
                } else if let Some(node) = map.remove("node") {
                    Op::AddNode {
                        node,
                        place: take_place(&mut map)?,
                    }
                } else if let Some(cel) = map.remove("cel") {
                    let layer = take_str(&mut map, "in")?.ok_or_else(|| anyhow!("add cel needs \"in\": a layer id"))?;
                    Op::AddCel { cel, layer }
                } else if let Some(mask) = map.remove("mask") {
                    let layer = take_str(&mut map, "in")?
                        .ok_or_else(|| anyhow!("add mask needs \"in\": the layer it masks"))?;
                    let hide = match map.remove("hide") {
                        None | Some(Value::Null) => false,
                        Some(Value::Bool(b)) => b,
                        Some(_) => bail!("hide must be true or false"),
                    };
                    Op::AddMask { mask, layer, hide }
                } else {
                    bail!("add needs a \"layer\", \"node\", \"cel\" or \"mask\" object");
                }
            }
            "set" => {
                let id = take_str(&mut map, "id")?.ok_or_else(|| anyhow!("set needs an id"))?;
                return Ok(Op::Set { id, props: map });
            }
            "rm" => {
                let mut ids: Vec<String> = match map.remove("ids") {
                    Some(Value::Array(items)) => items
                        .into_iter()
                        .map(|v| {
                            v.as_str()
                                .map(String::from)
                                .ok_or_else(|| anyhow!("ids must be strings"))
                        })
                        .collect::<Result<_>>()?,
                    Some(_) => bail!("ids must be a list"),
                    None => Vec::new(),
                };
                ids.extend(take_str(&mut map, "id")?);
                if ids.is_empty() {
                    bail!("rm needs an id or ids");
                }
                Op::Remove { ids }
            }
            "mv" => Op::Move {
                id: take_str(&mut map, "id")?.ok_or_else(|| anyhow!("mv needs an id"))?,
                place: take_place(&mut map)?,
            },
            "stroke" => {
                let spec: StrokeSpec = serde_json::from_value(Value::Object(std::mem::take(&mut map)))?;
                Op::Stroke(spec)
            }
            "fill" => {
                let spec: FillSpec = serde_json::from_value(Value::Object(std::mem::take(&mut map)))?;
                spec.check()?;
                Op::Fill(spec)
            }
            "palette" => Op::Palette(parse_palette(&mut map)?),
            "transform" => {
                let mut ids: Vec<String> = match map.remove("ids") {
                    Some(v) => serde_json::from_value(v).map_err(|_| anyhow!("ids must be a list of strings"))?,
                    None => Vec::new(),
                };
                ids.extend(take_str(&mut map, "id")?);
                if ids.is_empty() {
                    bail!("transform needs an id or ids (strokes and fills, <cel>:s<id>)");
                }
                Op::Transform {
                    ids,
                    transform: MarkTransform::parse(&mut map)?,
                }
            }
            "path" => Op::Path {
                id: take_str(&mut map, "id")?.ok_or_else(|| anyhow!("path needs the id of a path, rect or ellipse"))?,
                edit: PathEdit::parse(&mut map)?,
            },
            "lasso" => Op::Lasso(parse_lasso(&mut map)?),
            "copy" => Op::Copy(parse_copy(&mut map)?),
            "atmosphere" => match map.remove("off") {
                Some(Value::Bool(true)) => Op::Atmosphere(None),
                Some(other) if other != Value::Bool(false) => bail!("atmosphere: off takes true or false"),
                _ => Op::Atmosphere(Some(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?)),
            },
            "timeline" => Op::Timeline {
                fps: whole(&mut map, "fps")?,
                frames: whole(&mut map, "frames")?,
                origin: match map.remove("origin") {
                    None => None,
                    Some(Value::Null) => Some(None),
                    Some(v) => Some(Some(
                        serde_json::from_value(v).map_err(|_| anyhow!("origin must be [x, y] or null"))?,
                    )),
                },
            },
            "frames" => Op::Frames(parse_frames(&mut map)?),
            "cel" => Op::Cel(parse_cel(&mut map)?),
            "retime" => Op::Retime(parse_retime(&mut map)?),
            "inbetween" => Op::Inbetween(parse_inbetween(&mut map)?),
            "part" => {
                let ids: Vec<String> = serde_json::from_value(
                    map.remove("ids")
                        .ok_or_else(|| anyhow!("part needs \"ids\": the marks, as c4:s12"))?,
                )
                .map_err(|_| anyhow!("part \"ids\" is a list of marks, as [\"c4:s12\"]"))?;
                let name = take_str(&mut map, "name")?.ok_or_else(|| {
                    anyhow!("part needs \"name\": the part's name (\"\" takes the marks out of their part)")
                })?;
                Op::Part { ids, name }
            }
            "duplicate" => Op::Duplicate {
                layer: take_str(&mut map, "layer")?
                    .ok_or_else(|| anyhow!("duplicate needs \"layer\": the layer to copy"))?,
            },
            "ink" => Op::Ink(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "light" => Op::Light(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "hatch" => Op::Hatch(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "paint" => Op::Paint(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "scatter" => Op::Scatter(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "foliage" => Op::Foliage(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "paintover" => Op::PaintOver(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "bone" => Op::Bone(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "weights" => Op::Weights(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "pose" => Op::Pose(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "grass" => Op::Grass(serde_json::from_value(Value::Object(std::mem::take(&mut map)))?),
            "clip" => Op::Clip(parse_clip(&mut map)?),
            "export" => Op::Export {
                godot: match map.remove("godot") {
                    Some(Value::Null) => None,
                    Some(Value::String(dir)) => Some(dir),
                    _ => bail!("export needs \"godot\": a folder relative to the bundle, or null"),
                },
            },
            other => {
                bail!(
                    "unknown op {other:?}: use add, set, rm, mv, stroke, fill, palette, transform, path, lasso, copy, timeline, frames, cel, clip, retime, inbetween, duplicate, ink, light, hatch, scatter or export"
                )
            }
        };
        no_leftovers(&map, &op)?;
        Ok(parsed)
    }
}

fn whole(map: &mut Map<String, Value>, key: &str) -> Result<Option<u32>> {
    match map.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n <= u32::MAX as u64 / 2)
            .map(|n| Some(n as u32))
            .ok_or_else(|| anyhow!("{key} must be a whole number")),
    }
}

fn flag(map: &mut Map<String, Value>, key: &str) -> Result<bool> {
    match map.remove(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(b),
        Some(_) => bail!("{key} must be true or false"),
    }
}

fn parse_frames(map: &mut Map<String, Value>) -> Result<FrameSpec> {
    let (insert, at) = match (whole(map, "insert")?, whole(map, "remove")?) {
        (Some(at), None) => (true, at),
        (None, Some(at)) => (false, at),
        _ => bail!("frames needs \"insert\": <frame> or \"remove\": <frame>"),
    };
    let layers = match map.remove("layers") {
        None | Some(Value::Null) => None,
        Some(v) => Some(serde_json::from_value(v).map_err(|_| anyhow!("layers must be a list of layer ids"))?),
    };
    Ok(FrameSpec {
        insert,
        at,
        count: whole(map, "count")?.unwrap_or(1),
        layers,
    })
}

fn parse_inbetween(map: &mut Map<String, Value>) -> Result<InbetweenSpec> {
    let layers: Vec<String> = match map.remove("layers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(one)) => vec![one],
        Some(v) => serde_json::from_value(v).map_err(|_| anyhow!("layers must be a list of layer ids"))?,
    };
    let text = |map: &mut Map<String, Value>, key: &str| -> Result<String> {
        match map.remove(key) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(s)) => Ok(s),
            Some(Value::Array(values)) if key == "spacing" => {
                Ok(values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","))
            }
            Some(other) => bail!("{key} must be a string, not {other}"),
        }
    };
    let spacing = Spacing::parse(&text(map, "spacing")?)?;
    let matching = Match::parse(&text(map, "match")?)?;
    let unpaired = Unpaired::parse(&text(map, "unpaired")?)?;
    let cels = |map: &mut Map<String, Value>, key: &str| -> Result<Vec<String>> {
        match map.remove(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(v) => serde_json::from_value(v).map_err(|_| anyhow!("{key} must be a list of cel ids")),
        }
    };
    let (only, keep_cels, force_cels) = (cels(map, "only")?, cels(map, "keepCels")?, cels(map, "forceCels")?);
    let pairs: Vec<[u32; 2]> = match map.remove("pair") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => {
            serde_json::from_value(v).map_err(|_| anyhow!("pair must be a list of [first key id, second key id]"))?
        }
    };
    let spec = InbetweenSpec {
        layers,
        from: whole(map, "from")?,
        to: whole(map, "to")?,
        count: whole(map, "count")?,
        spacing,
        matching,
        pairs,
        redo: flag(map, "redo")?,
        force: flag(map, "force")?,
        looping: flag(map, "loop")?,
        keep_drawn: flag(map, "keepDrawn")?,
        unpaired,
        only,
        keep_cels,
        force_cels,
        resolved: None,
    };
    if !spec.redo && (spec.layers.is_empty() || spec.from.is_none() || (spec.to.is_none() && !spec.looping)) {
        bail!(
            "inbetween needs \"layers\", \"from\" and \"to\" (the frames of the two keys; with \"loop\" the end of the loop), or \"redo\": true"
        );
    }
    if (spec.keep_drawn || !spec.only.is_empty() || !spec.keep_cels.is_empty() || !spec.force_cels.is_empty())
        && !spec.redo
    {
        bail!("keepDrawn, only, keepCels and forceCels choose what a redo remakes; give \"redo\": true");
    }
    if spec.count == Some(0) {
        bail!("count must be at least 1");
    }
    Ok(spec)
}

pub fn rig_owner<'a>(doc: &'a Document, id: &str) -> Result<&'a Layer> {
    let layer = doc.find_layer(id)?;
    match layer.kind() {
        Kind::Paint | Kind::Group => Ok(layer),
        _ => bail!("bones go on a paint layer or a group; {} is neither", layer.name),
    }
}

fn bone(doc: &mut Document, spec: &BoneSpec, out: &mut Applied) -> Result<()> {
    let id = rig_owner(doc, &spec.layer)?.id.clone();
    let path = layer_path(doc, &id)?;
    let mut bones = doc.layer_at(&path).bones.clone();
    let find = |bones: &[crate::rig::Bone], name: &str| -> Result<usize> {
        bones.iter().position(|b| b.name == name).ok_or_else(|| {
            anyhow!(
                "no bone {name} on {}: {}",
                spec.layer,
                bones.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", ")
            )
        })
    };
    match (&spec.add, &spec.rm, &spec.rename, &spec.moved, &spec.reparent) {
        (Some(name), None, None, None, None) => {
            let (Some(head), Some(tail)) = (spec.head, spec.tail) else {
                bail!("a new bone needs \"head\" and \"tail\": [x, y] each");
            };
            if bones.iter().any(|b| &b.name == name) {
                bail!("there is already a bone {name} on {}", spec.layer);
            }
            if let Some(p) = &spec.parent {
                find(&bones, p)?;
            }
            bones.push(crate::rig::Bone {
                name: name.clone(),
                parent: spec.parent.clone(),
                head,
                tail,
            });
            out.notes.push(format!("bone {name} on {}", spec.layer));
        }
        (None, Some(name), None, None, None) => {
            find(&bones, name)?;
            let mut gone = vec![name.clone()];
            while let Some(next) = bones
                .iter()
                .find(|b| b.parent.as_ref().is_some_and(|p| gone.contains(p)) && !gone.contains(&b.name))
                .map(|b| b.name.clone())
            {
                gone.push(next);
            }
            bones.retain(|b| !gone.contains(&b.name));
            out.notes.push(format!("removed {}", gone.join(", ")));
        }
        (None, None, Some(name), None, None) => {
            let Some(to) = &spec.to_name else {
                bail!("rename needs \"as\": the new name");
            };
            let at = find(&bones, name)?;
            if bones.iter().any(|b| &b.name == to) {
                bail!("there is already a bone {to}");
            }
            bones[at].name = to.clone();
            bones
                .iter_mut()
                .filter(|b| b.parent.as_ref() == Some(name))
                .for_each(|b| b.parent = Some(to.clone()));
            let rigged: Vec<Vec<usize>> = crate::doc::family_paths(doc, &path);
            rigged.iter().for_each(|p| {
                if let Some(cels) = doc.layer_at_mut(p).cels_mut() {
                    cels.iter_mut().filter_map(|c| c.pose.as_mut()).for_each(|pose| {
                        if let Some(v) = pose.angles.remove(name) {
                            pose.angles.insert(to.clone(), v);
                        }
                    });
                }
            });
            out.renamed_bones.push((id.clone(), name.clone(), to.clone()));
        }
        (None, None, None, Some(name), None) => {
            let at = find(&bones, name)?;
            if let Some(h) = spec.head {
                bones[at].head = h;
            }
            if let Some(t) = spec.tail {
                bones[at].tail = t;
            }
        }
        (None, None, None, None, Some(name)) => {
            let at = find(&bones, name)?;
            if let Some(p) = &spec.parent {
                find(&bones, p)?;
                if p == name {
                    bail!("a bone cannot be its own parent");
                }
            }
            bones[at].parent = spec.parent.clone();
            let mut sorted: Vec<crate::rig::Bone> = Vec::new();
            while sorted.len() < bones.len() {
                let before = sorted.len();
                bones.iter().for_each(|b| {
                    let ready = b.parent.as_ref().is_none_or(|p| sorted.iter().any(|s| &s.name == p));
                    if ready && !sorted.iter().any(|s| s.name == b.name) {
                        sorted.push(b.clone());
                    }
                });
                if sorted.len() == before {
                    bail!("that parent would make a loop");
                }
            }
            bones = sorted;
        }
        _ => bail!("a bone op takes one of add, rm, rename, move or reparent"),
    }
    crate::rig::check(&bones)?;
    doc.layer_at_mut(&path).bones = bones;
    out.changed.push(id);
    Ok(())
}

fn posed(doc: &mut Document, made: &[PosedCel], out: &mut Applied) -> Result<()> {
    made.iter().try_for_each(|cel| {
        let path = layer_path(doc, &cel.layer)?;
        let cels = doc
            .layer_at_mut(&path)
            .cels_mut()
            .ok_or_else(|| anyhow!("layer {} has no cels", cel.layer))?;
        match cel.fresh {
            true => {
                if let Some(c) = cels.iter().find(|c| c.frame == cel.frame) {
                    bail!("layer {} already has {} at frame {}", cel.layer, c.id, cel.frame);
                }
                let at = cels.iter().position(|c| c.frame > cel.frame).unwrap_or(cels.len());
                cels.insert(
                    at,
                    Cel {
                        hold: cel.hold,
                        pose: Some(cel.pose.clone()),
                        ..Cel::at(&cel.cel, cel.frame)
                    },
                );
                if let Some(n) = cel.cel.strip_prefix('c').and_then(|n| n.parse::<u64>().ok()) {
                    doc.next_id = doc.next_id.max(n);
                }
                out.added.push(cel.cel.clone());
            }
            false => {
                let existing = cels
                    .iter_mut()
                    .find(|c| c.id == cel.cel)
                    .ok_or_else(|| anyhow!("no cel {} on layer {}", cel.cel, cel.layer))?;
                existing.pose = Some(cel.pose.clone());
                out.changed.push(cel.cel.clone());
            }
        }
        out.push(&cel.layer, &cel.cel, MarkChange::Put(cel.marks.clone()));
        Ok(())
    })
}

fn inbetweens(doc: &mut Document, made: &Inbetweens, out: &mut Applied) -> Result<()> {
    made.removed.iter().try_for_each(|id| remove(doc, id, out))?;
    made.holds.iter().try_for_each(|(id, hold)| {
        let Some(Loc::Cel(path, c)) = doc.locate(id) else {
            bail!("no cel {id}");
        };
        if let Some(cels) = doc.layer_at_mut(&path).cels_mut() {
            cels[c].hold = Some(*hold);
        }
        Ok(())
    })?;
    made.cels.iter().try_for_each(|cel| {
        let path = layer_path(doc, &cel.layer)?;
        let cels = doc
            .layer_at_mut(&path)
            .cels_mut()
            .ok_or_else(|| anyhow!("layer {} has no cels", cel.layer))?;
        match cel.fresh {
            true => {
                if let Some(c) = cels.iter().find(|c| c.frame == cel.frame) {
                    bail!("layer {} already has {} at frame {}", cel.layer, c.id, cel.frame);
                }
                let at = cels.iter().position(|c| c.frame > cel.frame).unwrap_or(cels.len());
                cels.insert(
                    at,
                    Cel {
                        hold: cel.hold,
                        tween: Some(cel.tween.clone()),
                        parts: cel.parts.clone(),
                        pose: cel.pose.clone(),
                        ..Cel::at(&cel.cel, cel.frame)
                    },
                );
                if let Some(n) = cel.cel.strip_prefix('c').and_then(|n| n.parse::<u64>().ok()) {
                    doc.next_id = doc.next_id.max(n);
                }
                out.added.push(cel.cel.clone());
            }
            false => {
                let existing = cels
                    .iter_mut()
                    .find(|c| c.id == cel.cel)
                    .ok_or_else(|| anyhow!("no cel {} on layer {}", cel.cel, cel.layer))?;
                existing.tween = Some(cel.tween.clone());
                existing.parts = cel.parts.clone();
                existing.pose = cel.pose.clone();
                out.changed.push(cel.cel.clone());
            }
        }
        out.push(&cel.layer, &cel.cel, MarkChange::Put(cel.marks.clone()));
        Ok(())
    })
}

fn parse_retime(map: &mut Map<String, Value>) -> Result<RetimeSpec> {
    let (Some(from), Some(to)) = (whole(map, "from")?, whole(map, "to")?) else {
        bail!("retime needs \"from\" and \"to\": the frames to retime, both included");
    };
    let on = whole(map, "on")?;
    let fit = whole(map, "fit")?;
    let (reverse, pingpong) = (flag(map, "reverse")?, flag(map, "pingpong")?);
    let action = match (on, fit, reverse, pingpong) {
        (Some(n), None, false, false) => Retime::On(n),
        (None, Some(n), false, false) => Retime::Fit(n),
        (None, None, true, false) => Retime::Reverse,
        (None, None, false, true) => Retime::Pingpong,
        _ => bail!(
            "retime needs one of \"on\": <frames per drawing>, \"fit\": <frames>, \"reverse\": true or \"pingpong\": true"
        ),
    };
    let layers = match map.remove("layers") {
        None | Some(Value::Null) => None,
        Some(v) => Some(serde_json::from_value(v).map_err(|_| anyhow!("layers must be a list of layer ids"))?),
    };
    Ok(RetimeSpec {
        from,
        to,
        action,
        layers,
    })
}

fn parse_cel(map: &mut Map<String, Value>) -> Result<CelSpec> {
    let actions = [
        ("new", take_str(map, "new")?),
        ("dup", take_str(map, "dup")?),
        ("link", take_str(map, "link")?),
        ("unlink", take_str(map, "unlink")?),
        ("move", take_str(map, "move")?),
    ];
    let chosen: Vec<(&str, String)> = actions
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| (name, v)))
        .collect();
    let action = match &chosen[..] {
        [("new", layer)] => CelAction::New { layer: layer.clone() },
        [("dup", cel)] => CelAction::Dup { cel: cel.clone() },
        [("link", cel)] => CelAction::Link { cel: cel.clone() },
        [("unlink", cel)] => CelAction::Unlink { cel: cel.clone() },
        [("move", cel)] => CelAction::Move { cel: cel.clone() },
        _ => bail!(
            "cel needs one of \"new\": <layer>, \"dup\": <cel>, \"link\": <cel>, \"unlink\": <cel> or \"move\": <cel>"
        ),
    };
    let hold = whole(map, "hold")?;
    if hold == Some(0) {
        bail!("hold must be at least 1");
    }
    Ok(CelSpec {
        action,
        id: take_str(map, "id")?,
        frame: whole(map, "frame")?,
        hold,
        insert: flag(map, "insert")?,
    })
}

fn parse_clip(map: &mut Map<String, Value>) -> Result<ClipEdit> {
    let fps = match map.remove("fps") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(Some(
            v.as_u64()
                .filter(|n| (1..=240).contains(n))
                .ok_or_else(|| anyhow!("fps must be 1 to 240, or null for the document's"))? as u32,
        )),
    };
    let looping = match map.remove("loop") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(b),
        Some(_) => bail!("loop must be true or false"),
    };
    let (from, to) = (whole(map, "from")?, whole(map, "to")?);
    if let Some(name) = take_str(map, "add")? {
        let (Some(from), Some(to)) = (from, to) else {
            bail!("clip add needs \"from\" and \"to\" (frames, both included)");
        };
        return Ok(ClipEdit::Add(Clip {
            fps: fps.flatten(),
            ..Clip::new(&name, from, to, looping.unwrap_or(false))
        }));
    }
    if let Some(name) = take_str(map, "set")? {
        return Ok(ClipEdit::Set {
            name,
            rename: take_str(map, "name")?,
            from,
            to,
            looping,
            fps,
        });
    }
    if let Some(name) = take_str(map, "rm")? {
        return Ok(ClipEdit::Remove { name });
    }
    bail!(
        "clip needs \"add\": <name> (with from, to, loop, fps), \"set\": <name> (from, to, loop, fps, name) or \"rm\": <name>"
    )
}

fn clip_index(doc: &Document, name: &str) -> Result<usize> {
    doc.timeline.clips.iter().position(|c| c.name == name).ok_or_else(|| {
        let names: Vec<&str> = doc.timeline.clips.iter().map(|c| c.name.as_str()).collect();
        match names.is_empty() {
            true => anyhow!("no clip {name:?}: the document has no clips"),
            false => anyhow!("no clip {name:?}: clips are {}", names.join(", ")),
        }
    })
}

fn edit_clip(doc: &mut Document, edit: &ClipEdit) -> Result<()> {
    let clips = &doc.timeline.clips;
    match edit {
        ClipEdit::Add(clip) => {
            if clips.iter().any(|c| c.name == clip.name) {
                bail!("there is a clip {:?} already; use set to change it", clip.name);
            }
            let at = clips.iter().position(|c| c.from > clip.from).unwrap_or(clips.len());
            doc.timeline.clips.insert(at, clip.clone());
        }
        ClipEdit::Set {
            name,
            rename,
            from,
            to,
            looping,
            fps,
        } => {
            let at = clip_index(doc, name)?;
            let clip = &mut doc.timeline.clips[at];
            if let Some(new) = rename {
                clip.name = new.clone();
            }
            clip.from = from.unwrap_or(clip.from);
            clip.to = to.unwrap_or(clip.to);
            clip.looping = looping.unwrap_or(clip.looping);
            clip.fps = fps.unwrap_or(clip.fps);
            let moved = doc.timeline.clips.remove(at);
            let clips = &doc.timeline.clips;
            let at = clips.iter().position(|c| c.from > moved.from).unwrap_or(clips.len());
            doc.timeline.clips.insert(at, moved);
        }
        ClipEdit::Remove { name } => {
            let at = clip_index(doc, name)?;
            doc.timeline.clips.remove(at);
        }
    }
    Ok(())
}

fn parse_copy(map: &mut Map<String, Value>) -> Result<CopySpec> {
    let mut ids: Vec<String> = match map.remove("ids") {
        Some(v) => serde_json::from_value(v).map_err(|_| anyhow!("ids must be a list of strings"))?,
        None => Vec::new(),
    };
    ids.extend(take_str(map, "id")?);
    if ids.is_empty() {
        bail!("copy needs ids: the strokes and fills to copy (<cel>:s<id>)");
    }
    let to = match (take_str(map, "to")?, whole(map, "frame")?) {
        (Some(cel), None) => {
            if map.contains_key("layer") {
                bail!("copy takes \"layer\" with \"frame\"; \"to\" names the cel itself");
            }
            CopyTarget::Cel(cel)
        }
        (None, Some(frame)) => CopyTarget::Frame {
            frame,
            layer: take_str(map, "layer")?,
        },
        _ => {
            bail!("copy needs \"to\": <cel> or \"frame\": <n> (with \"layer\" when the marks come from another layer)")
        }
    };
    let cut = match map.remove("cut") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let polygon: Vec<[f32; 2]> =
                serde_json::from_value(v).map_err(|_| anyhow!("cut must be a polygon [[x, y], ...]"))?;
            if polygon.len() < 3 {
                bail!("a cut polygon needs at least 3 points");
            }
            Some(polygon)
        }
    };
    let transform = MarkTransform::parse(map)?;
    let moved = MarkTransform {
        pivot: None,
        ..transform
    } != MarkTransform::default();
    Ok(CopySpec {
        ids,
        to,
        transform: moved.then_some(transform),
        cut,
    })
}

fn copy(doc: &mut Document, spec: &CopySpec, out: &mut Applied) -> Result<String> {
    let mut sources: Vec<(String, String, Vec<u32>)> = Vec::new();
    spec.ids.iter().try_for_each(|id| {
        let (cel, mark) =
            parse_address(id).ok_or_else(|| anyhow!("copy takes strokes and fills (<cel>:s<id>), not {id}"))?;
        let layer = mark_owner(doc, cel)?;
        match sources.iter_mut().find(|(_, c, _)| c == cel) {
            Some((_, _, ids)) if ids.contains(&mark) => {}
            Some((_, _, ids)) => ids.push(mark),
            None => sources.push((layer, cel.to_string(), vec![mark])),
        }
        Ok::<_, anyhow::Error>(())
    })?;
    let (layer, cel) = match &spec.to {
        CopyTarget::Cel(cel) => {
            if let Some(Item::Cel(_, c)) = doc.item(cel)
                && let Some(source) = &c.link
            {
                bail!("cel {cel} links to {source}; copy into {source} instead");
            }
            (mark_owner(doc, cel)?, cel.clone())
        }
        CopyTarget::Frame { frame, layer } => {
            let layer = match layer {
                Some(key) => doc.find_layer(key)?.id.clone(),
                None => {
                    let layers: BTreeSet<&str> = sources.iter().map(|(l, _, _)| l.as_str()).collect();
                    match layers.len() {
                        1 => sources[0].0.clone(),
                        _ => bail!(
                            "the marks come from layers {}; say which layer they go to with \"layer\"",
                            layers.into_iter().collect::<Vec<_>>().join(" and ")
                        ),
                    }
                }
            };
            let cel = paint_cel_at(doc, &layer, *frame, out)?;
            (layer, cel)
        }
    };
    out.paste(&layer, &cel, sources, spec.transform, spec.cut.clone());
    out.changed.push(cel.clone());
    Ok(cel)
}

fn parse_lasso(map: &mut Map<String, Value>) -> Result<LassoSpec> {
    let layer = take_str(map, "layer")?.ok_or_else(|| anyhow!("lasso needs \"layer\": a paint layer id"))?;
    let frame = match map.remove("frame") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(|| anyhow!("frame must be a whole number"))? as u32,
    };
    let polygon: Vec<[f32; 2]> = match map.remove("polygon") {
        Some(v) => serde_json::from_value(v).map_err(|_| anyhow!("polygon must be [[x, y], ...]"))?,
        None => bail!("lasso needs \"polygon\": [[x, y], ...]"),
    };
    if polygon.len() < 3 {
        bail!("a lasso polygon needs at least 3 points");
    }
    let cut = flag(map, "cut")?;
    let action = if flag(map, "rm")? {
        LassoAction::Remove
    } else {
        LassoAction::Transform(MarkTransform::parse(map)?)
    };
    Ok(LassoSpec {
        layer,
        frame,
        polygon,
        cut,
        action,
    })
}

fn parse_palette(map: &mut Map<String, Value>) -> Result<PaletteEdit> {
    if let Some(color) = map.remove("add") {
        let color = color
            .as_str()
            .ok_or_else(|| anyhow!("palette add takes a colour string, #rrggbb or #rrggbbaa"))?
            .to_string();
        let name = take_str(map, "name")?.unwrap_or_default();
        let index = match map.remove("index") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| anyhow!("index must be a whole number"))? as usize),
        };
        return Ok(PaletteEdit::Add { color, name, index });
    }
    if let Some(key) = map.remove("rm") {
        let key = match key {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            other => bail!("palette rm takes a name, a colour or an index, not {other}"),
        };
        return Ok(PaletteEdit::Remove { key });
    }
    if let Some(list) = map.remove("set") {
        let swatches: Vec<Swatch> = serde_json::from_value(list)
            .context("palette set takes a list like [{\"color\":\"#1b1b1b\",\"name\":\"ink\"}]")?;
        return Ok(PaletteEdit::Set { swatches });
    }
    bail!("palette needs \"add\" (with \"name\"), \"rm\" or \"set\"")
}

fn edit_palette(doc: &mut Document, edit: &PaletteEdit) -> Result<()> {
    match edit {
        PaletteEdit::Add { color, name, index } => {
            parse_hex(color)?;
            let same_name = (!name.is_empty())
                .then(|| doc.palette.iter().position(|s| &s.name == name))
                .flatten();
            if let Some(at) = same_name {
                doc.palette[at].color = color.clone();
                return Ok(());
            }
            if name.is_empty()
                && doc
                    .palette
                    .iter()
                    .any(|s| s.name.is_empty() && s.color.eq_ignore_ascii_case(color))
            {
                return Ok(());
            }
            let at = index.unwrap_or(doc.palette.len()).min(doc.palette.len());
            doc.palette.insert(
                at,
                Swatch {
                    color: color.clone(),
                    name: name.clone(),
                },
            );
        }
        PaletteEdit::Remove { key } => {
            let at = doc
                .swatch(key)
                .ok_or_else(|| anyhow!("the palette has no {key:?} (give a name, a colour or an index)"))?;
            doc.palette.remove(at);
        }
        PaletteEdit::Set { swatches } => doc.palette = swatches.clone(),
    }
    Ok(())
}

fn check_keys(value: &Value, allowed: &[&str], what: &str) -> Result<()> {
    let Some(map) = value.as_object() else {
        bail!("{what} must be a JSON object");
    };
    map.keys().try_for_each(|key| {
        if allowed.contains(&key.as_str()) {
            Ok(())
        } else {
            bail!("{what} has no {key:?}; it takes {}", allowed.join(", "))
        }
    })
}

fn check_node_keys(value: &Value) -> Result<()> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("a node needs a type"))?;
    let allowed: Vec<&str> = NODE_KEYS.iter().chain(shape_keys(kind)?).copied().collect();
    check_keys(value, &allowed, &format!("a {kind} node"))?;
    if let Some(t) = value.get("transform") {
        check_keys(t, TRANSFORM_KEYS, "transform")?;
    }
    if let Some(s) = value.get("stroke") {
        check_keys(s, STROKE_KEYS, "stroke")?;
    }
    value
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .try_for_each(check_node_keys)
}

fn ensure_id(doc: &mut Document, taken: &mut BTreeSet<String>, value: &mut Value, prefix: char) -> Result<String> {
    match value.get("id").and_then(Value::as_str) {
        Some(id) if !taken.insert(id.to_string()) => bail!("id {id} is already used"),
        Some(id) => Ok(id.to_string()),
        None => {
            let id = doc.alloc_id_in(prefix, taken);
            value["id"] = Value::String(id.clone());
            Ok(id)
        }
    }
}

fn assign_node_ids(doc: &mut Document, taken: &mut BTreeSet<String>, value: &mut Value) -> Result<String> {
    let id = ensure_id(doc, taken, value, 'n')?;
    if let Some(children) = value.get_mut("children").and_then(Value::as_array_mut) {
        children
            .iter_mut()
            .try_for_each(|c| assign_node_ids(doc, taken, c).map(|_| ()))?;
    }
    Ok(id)
}

fn assign_layer_ids(doc: &mut Document, taken: &mut BTreeSet<String>, value: &mut Value) -> Result<String> {
    check_keys(value, LAYER_KEYS, "a layer")?;
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("a layer needs a kind: paint, vector, group or adjust"))?
        .to_string();
    let id = ensure_id(doc, taken, value, 'l')?;
    if value.get("name").is_none() {
        value["name"] = Value::String(format!("{kind} {id}"));
    }
    match kind.as_str() {
        "paint" | "vector" => {
            if value.get("cels").is_none_or(|c| c.as_array().is_none_or(Vec::is_empty)) {
                value["cels"] = serde_json::json!([{ "frame": 0 }]);
            }
            if let Some(cels) = value.get_mut("cels").and_then(Value::as_array_mut) {
                cels.iter_mut().try_for_each(|cel| {
                    check_keys(cel, CEL_KEYS, "a cel")?;
                    ensure_id(doc, taken, cel, 'c')?;
                    cel.get_mut("nodes")
                        .and_then(Value::as_array_mut)
                        .into_iter()
                        .flatten()
                        .try_for_each(|n| {
                            check_node_keys(n)?;
                            assign_node_ids(doc, taken, n).map(|_| ())
                        })
                })?;
            }
        }
        "group" => {
            if let Some(children) = value.get_mut("children").and_then(Value::as_array_mut) {
                children
                    .iter_mut()
                    .try_for_each(|c| assign_layer_ids(doc, taken, c).map(|_| ()))?;
            }
        }
        "adjust" => {
            if value.get("adjust").is_none() {
                bail!(
                    "an adjustment layer needs adjust: {}",
                    crate::look::ADJUSTMENTS.map(|a| format!("{{\"{a}\":{{}}}}")).join(", ")
                );
            }
        }
        other => bail!("unknown layer kind {other:?}: use paint, vector, group or adjust"),
    }
    if let Some(mask) = value.get_mut("mask").filter(|m| !m.is_null()) {
        mask_defaults(mask)?;
        assign_layer_ids(doc, taken, mask)?;
    }
    Ok(id)
}

fn mask_defaults(mask: &mut Value) -> Result<()> {
    let Some(map) = mask.as_object_mut() else {
        bail!("a mask must be a JSON object");
    };
    match map.get("kind").and_then(Value::as_str) {
        None => {
            map.insert("kind".into(), json!("paint"));
        }
        Some("paint") => {}
        Some(other) => bail!("a mask is a paint layer, not {other}"),
    }
    map.entry("name").or_insert_with(|| json!("mask"));
    Ok(())
}

fn layer_path(doc: &Document, id: &str) -> Result<Vec<usize>> {
    match doc.locate(id) {
        Some(Loc::Layer(path)) => Ok(path),
        Some(_) => bail!("{id} is not a layer"),
        None => bail!("no item {id}"),
    }
}

fn fresh_layer(
    doc: &mut Document,
    taken: &mut BTreeSet<String>,
    layer: &Layer,
    copies: &mut Vec<(String, String, String, String)>,
) -> Layer {
    let mut copy = layer.clone();
    copy.id = doc.alloc_id_in('l', taken);
    let paint = layer.kind() == Kind::Paint;
    let fresh: Vec<(String, String)> = layer
        .cels()
        .iter()
        .map(|c| (c.id.clone(), doc.alloc_id_in('c', taken)))
        .collect();
    let rename = |id: &str| {
        fresh
            .iter()
            .find(|(old, _)| old == id)
            .map_or(id.to_string(), |(_, new)| new.clone())
    };
    let nodes: Vec<Vec<Node>> = layer
        .cels()
        .iter()
        .map(|c| crate::timing::renumbered(doc, &c.nodes))
        .collect();
    let copy_id = copy.id.clone();
    if let Some(cels) = copy.cels_mut() {
        cels.iter_mut().zip(nodes).for_each(|(c, nodes)| {
            let old = c.id.clone();
            c.id = rename(&old);
            c.link = c.link.as_deref().map(rename);
            if let Some(t) = c.tween.as_mut() {
                t.from = rename(&t.from);
                t.to = rename(&t.to);
            }
            c.nodes = nodes;
            if paint && c.link.is_none() {
                copies.push((copy_id.clone(), c.id.clone(), layer.id.clone(), old));
            }
        });
    }
    if let LayerContent::Group { children } = &mut copy.content {
        *children = layer
            .children()
            .iter()
            .map(|child| fresh_layer(doc, taken, child, copies))
            .collect();
    }
    copy.mask = layer
        .mask
        .as_deref()
        .map(|m| Box::new(fresh_layer(doc, taken, m, copies)));
    copy
}

fn duplicate_layer(doc: &mut Document, id: &str, out: &mut Applied) -> Result<String> {
    let path = layer_path(doc, id)?;
    if path.last() == Some(&MASK) {
        bail!("{id} is a mask; duplicate the layer it masks");
    }
    let source = doc.layer_at(&path).clone();
    let mut taken = doc.ids();
    let mut copies = Vec::new();
    let mut copy = fresh_layer(doc, &mut taken, &source, &mut copies);
    copy.name = format!("{} copy", source.name);
    let new_id = copy.id.clone();
    let (parent, index) = (path[..path.len() - 1].to_vec(), path[path.len() - 1] + 1);
    doc.layer_list_mut(&parent)
        .ok_or_else(|| anyhow!("{id} has no list to go into"))?
        .insert(index, copy);
    copies
        .iter()
        .for_each(|(layer, cel, from_layer, from_cel)| out.copy(layer, cel, from_layer, from_cel));
    Ok(new_id)
}

fn layer_slot(doc: &Document, place: &Place) -> Result<(Vec<usize>, usize)> {
    let sibling = |id: &str, offset: usize| -> Result<(Vec<usize>, usize)> {
        let mut path = layer_path(doc, id)?;
        let index = path.pop().unwrap_or_default();
        if index == MASK {
            bail!("{id} is a mask; layers go above or below the layer it masks");
        }
        Ok((path, index + offset))
    };
    let (parent, index) = match (&place.above, &place.below, &place.into) {
        (Some(id), _, _) => sibling(id, 1)?,
        (_, Some(id), _) => sibling(id, 0)?,
        (_, _, Some(id)) => {
            let path = layer_path(doc, id)?;
            let group = doc.layer_at(&path);
            if group.kind() != Kind::Group {
                bail!("{id} is a {} layer; layers go into group layers", group.kind().name());
            }
            let len = group.children().len();
            (path, place.index.unwrap_or(len))
        }
        _ => (Vec::new(), place.index.unwrap_or(doc.layers.len())),
    };
    let len = if parent.is_empty() {
        doc.layers.len()
    } else {
        doc.layer_at(&parent).children().len()
    };
    if index > len {
        bail!("index {index} is past the end ({len} layers)");
    }
    Ok((parent, index))
}

struct NodeSlot {
    layer: Vec<usize>,
    cel: usize,
    parent: Vec<usize>,
    index: Option<usize>,
}

fn node_list<'a>(doc: &'a mut Document, layer: &[usize], cel: usize, parent: &[usize]) -> Result<&'a mut Vec<Node>> {
    let cels = doc
        .layer_at_mut(layer)
        .cels_mut()
        .ok_or_else(|| anyhow!("not a layer with cels"))?;
    parent
        .iter()
        .try_fold(&mut cels[cel].nodes, |list, &i| match list[i].shape.children_mut() {
            Some(children) => Ok(children),
            None => bail!("node path runs through a non-group node"),
        })
}

fn cel_for_nodes(doc: &mut Document, path: &[usize], frame: u32) -> Result<usize> {
    let layer = doc.layer_at(path);
    if layer.kind() != Kind::Vector {
        bail!(
            "{} is a {} layer; shapes go into vector layers",
            layer.id,
            layer.kind().name()
        );
    }
    if let Some(cel) = layer.cel_at(frame).filter(|_| frame < doc.timeline.frames) {
        if let Some(target) = &cel.link {
            bail!(
                "cel {} at frame {frame} links to {target}; add to {target} instead",
                cel.id
            );
        }
        let id = cel.id.clone();
        return Ok(layer.cels().iter().position(|c| c.id == id).unwrap_or_default());
    }
    let id = doc.alloc_id('c');
    let cels = doc
        .layer_at_mut(path)
        .cels_mut()
        .ok_or_else(|| anyhow!("layer has no cels"))?;
    let index = cels.iter().position(|c| c.frame > frame).unwrap_or(cels.len());
    cels.insert(index, Cel::at(&id, frame));
    Ok(index)
}

fn node_slot(doc: &mut Document, place: &Place) -> Result<NodeSlot> {
    let sibling = |doc: &Document, id: &str, offset: usize| -> Result<NodeSlot> {
        match doc.locate(id) {
            Some(Loc::Node(layer, cel, mut nodes)) => {
                let index = nodes.pop().unwrap_or_default();
                Ok(NodeSlot {
                    layer,
                    cel,
                    parent: nodes,
                    index: Some(index + offset),
                })
            }
            Some(_) => bail!("{id} is not a node"),
            None => bail!("no item {id}"),
        }
    };
    if let Some(id) = &place.above {
        return sibling(doc, id, 1);
    }
    if let Some(id) = &place.below {
        return sibling(doc, id, 0);
    }
    let Some(id) = &place.into else {
        bail!("say where the node goes: in=<vector layer, cel or group node>, above=<node> or below=<node>");
    };
    let slot = match doc.locate(id) {
        Some(Loc::Layer(layer)) => NodeSlot {
            cel: cel_for_nodes(doc, &layer, place.frame.unwrap_or(0))?,
            layer,
            parent: Vec::new(),
            index: place.index,
        },
        Some(Loc::Cel(layer, cel)) => {
            let owner = doc.layer_at(&layer);
            let target = &owner.cels()[cel];
            if owner.kind() != Kind::Vector {
                bail!(
                    "cel {id} belongs to a {} layer; shapes go into vector layers",
                    owner.kind().name()
                );
            }
            if let Some(source) = &target.link {
                bail!("cel {id} links to {source}; add to {source} instead");
            }
            NodeSlot {
                layer,
                cel,
                parent: Vec::new(),
                index: place.index,
            }
        }
        Some(Loc::Node(layer, cel, nodes)) => NodeSlot {
            layer,
            cel,
            parent: nodes,
            index: place.index,
        },
        None => bail!("no item {id}"),
    };
    Ok(slot)
}

fn insert_node(doc: &mut Document, slot: NodeSlot, node: Node) -> Result<()> {
    let list = node_list(doc, &slot.layer, slot.cel, &slot.parent)?;
    let index = slot.index.unwrap_or(list.len());
    if index > list.len() {
        bail!("index {index} is past the end ({} nodes)", list.len());
    }
    list.insert(index, node);
    Ok(())
}

fn set_path(target: &mut Value, path: &str, value: Value) -> Result<()> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.first().is_some_and(|p| STRUCTURE.contains(p)) {
        bail!("{path} cannot be set; use add, rm or mv for structure");
    }
    let (last, parents) = parts.split_last().ok_or_else(|| anyhow!("empty property name"))?;
    let parent = parents.iter().try_fold(target, |node, part| {
        let map = node
            .as_object_mut()
            .ok_or_else(|| anyhow!("{path}: {part} is not an object"))?;
        let child = map.entry(part.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if child.is_null() {
            *child = Value::Object(Map::new());
        }
        Ok::<_, anyhow::Error>(child)
    })?;
    let map = parent
        .as_object_mut()
        .ok_or_else(|| anyhow!("{path}: parent is not an object"))?;
    if value.is_null() {
        map.remove(*last);
    } else {
        map.insert(last.to_string(), value);
    }
    Ok(())
}

fn patched<T: serde::Serialize + serde::de::DeserializeOwned>(
    item: &T,
    props: &Map<String, Value>,
    check: impl Fn(&Value) -> Result<()>,
) -> Result<T> {
    let mut value = serde_json::to_value(item)?;
    props.iter().try_for_each(|(k, v)| set_path(&mut value, k, v.clone()))?;
    check(&value)?;
    Ok(serde_json::from_value(value)?)
}

fn set(doc: &mut Document, id: &str, props: &Map<String, Value>) -> Result<()> {
    match doc.locate(id).ok_or_else(|| anyhow!("no item {id}"))? {
        Loc::Layer(path) => {
            let layer = doc.layer_at_mut(&path);
            *layer = patched(&*layer, props, |v| check_keys(v, LAYER_KEYS, "a layer"))?;
        }
        Loc::Cel(path, c) => {
            let cels = doc
                .layer_at_mut(&path)
                .cels_mut()
                .ok_or_else(|| anyhow!("layer has no cels"))?;
            cels[c] = patched(&cels[c], props, |v| check_keys(v, CEL_KEYS, "a cel"))?;
            cels.sort_by_key(|c| c.frame);
        }
        Loc::Node(path, c, mut nodes) => {
            let index = nodes.pop().unwrap_or_default();
            let list = node_list(doc, &path, c, &nodes)?;
            list[index] = patched(&list[index], props, check_node_keys)?;
        }
    }
    Ok(())
}

fn paint_cels_of(layer: &Layer) -> Vec<(String, String)> {
    let own = layer
        .cels()
        .iter()
        .filter(|c| layer.kind() == Kind::Paint && c.link.is_none())
        .map(|c| (layer.id.clone(), c.id.clone()));
    own.chain(layer.mask.iter().flat_map(|m| paint_cels_of(m)))
        .chain(layer.children().iter().flat_map(paint_cels_of))
        .collect()
}

fn remove(doc: &mut Document, id: &str, out: &mut Applied) -> Result<()> {
    let Some(loc) = doc.locate(id) else {
        return Ok(());
    };
    match loc {
        Loc::Layer(mut path) if path.last() == Some(&MASK) => {
            path.pop();
            if let Some(mask) = doc.layer_at_mut(&path).mask.take() {
                out.dropped_cels.extend(paint_cels_of(&mask));
            }
        }
        Loc::Layer(mut path) => {
            let index = path.pop().unwrap_or_default();
            let list = doc
                .layer_list_mut(&path)
                .ok_or_else(|| anyhow!("parent is not a group"))?;
            let layer = list.remove(index);
            out.dropped_cels.extend(paint_cels_of(&layer));
        }
        Loc::Cel(path, c) => {
            let layer = doc.layer_at_mut(&path);
            let cel = layer.cels_mut().ok_or_else(|| anyhow!("layer has no cels"))?.remove(c);
            timing::hand_over(layer, vec![cel], out);
        }
        Loc::Node(path, c, mut nodes) => {
            let index = nodes.pop().unwrap_or_default();
            node_list(doc, &path, c, &nodes)?.remove(index);
        }
    }
    out.removed.push(id.to_string());
    Ok(())
}

fn move_item(doc: &mut Document, id: &str, place: &Place) -> Result<()> {
    match doc.locate(id).ok_or_else(|| anyhow!("no item {id}"))? {
        Loc::Layer(path) if path.last() == Some(&MASK) => {
            bail!("{id} is a mask and stays with its layer; rm it and add a mask to the other layer")
        }
        Loc::Layer(mut path) => {
            let index = path.pop().unwrap_or_default();
            let layer = doc
                .layer_list_mut(&path)
                .ok_or_else(|| anyhow!("parent is not a group"))?
                .remove(index);
            let (parent, at) = layer_slot(doc, place).map_err(|e| anyhow!("{e} (a layer cannot move into itself)"))?;
            doc.layer_list_mut(&parent)
                .ok_or_else(|| anyhow!("target is not a group"))?
                .insert(at, layer);
        }
        Loc::Node(path, c, mut nodes) => {
            let index = nodes.pop().unwrap_or_default();
            let node = node_list(doc, &path, c, &nodes)?.remove(index);
            let slot = node_slot(doc, place).map_err(|e| anyhow!("{e} (a node cannot move into itself)"))?;
            insert_node(doc, slot, node)?;
        }
        Loc::Cel(..) => bail!("cels move by setting their frame: set {id} frame=<n>"),
    }
    Ok(())
}

pub fn apply(doc: &mut Document, op: &Op, out: &mut Applied) -> Result<()> {
    match op {
        Op::AddLayer { layer, place } => {
            let mut value = layer.clone();
            let id = assign_layer_ids(doc, &mut doc.ids(), &mut value)?;
            let layer: Layer = serde_json::from_value(value)?;
            let (parent, index) = layer_slot(doc, place)?;
            doc.layer_list_mut(&parent)
                .ok_or_else(|| anyhow!("target is not a group"))?
                .insert(index, layer);
            out.added.push(id);
        }
        Op::AddNode { node, place } => {
            let mut value = node.clone();
            check_node_keys(&value)?;
            let id = assign_node_ids(doc, &mut doc.ids(), &mut value)?;
            let node: Node = serde_json::from_value(value)?;
            let slot = node_slot(doc, place)?;
            insert_node(doc, slot, node)?;
            out.added.push(id);
        }
        Op::AddCel { cel, layer } => {
            let mut value = cel.clone();
            check_keys(&value, CEL_KEYS, "a cel")?;
            let id = ensure_id(doc, &mut doc.ids(), &mut value, 'c')?;
            let cel: Cel = serde_json::from_value(value)?;
            let path = layer_path(doc, layer)?;
            let target = doc.layer_at_mut(&path);
            let cels = match &mut target.content {
                LayerContent::Paint { cels } | LayerContent::Vector { cels } => cels,
                LayerContent::Group { .. } => bail!("{layer} is a group; cels go into paint or vector layers"),
                LayerContent::Adjust { .. } => bail!("{layer} is an adjustment; cels go into paint or vector layers"),
            };
            if cels.iter().any(|c| c.frame == cel.frame) {
                bail!("layer {layer} already has a cel starting at frame {}", cel.frame);
            }
            let index = cels.iter().position(|c| c.frame > cel.frame).unwrap_or(cels.len());
            cels.insert(index, cel);
            out.added.push(id);
        }
        Op::AddMask { mask, layer, hide } => {
            let path = layer_path(doc, layer)?;
            if path.last() == Some(&MASK) {
                bail!("{layer} is a mask itself");
            }
            if doc.layer_at(&path).mask.is_some() {
                bail!("layer {layer} already has a mask; rm it first");
            }
            let mut value = mask.clone();
            mask_defaults(&mut value)?;
            let id = assign_layer_ids(doc, &mut doc.ids(), &mut value)?;
            let added: Layer = serde_json::from_value(value)?;
            let cel = added.cels().first().map(|c| c.id.clone());
            doc.layer_at_mut(&path).mask = Some(Box::new(added));
            out.added.push(id.clone());
            if let (true, Some(cel)) = (*hide, cel) {
                out.push(&id, &cel, MarkChange::Add(vec![Mark::Fill(everything(doc))]));
            }
        }
        Op::Lasso(spec) => {
            let path = layer_path(doc, &spec.layer)?;
            let target = doc.layer_at(&path);
            let cel = target
                .content_at(spec.frame)
                .ok_or_else(|| anyhow!("layer {} has no cel at frame {}", spec.layer, spec.frame))?;
            let cel = cel.id.clone();
            let layer = mark_owner(doc, &cel)?;
            out.push(
                &layer,
                &cel,
                MarkChange::Lasso {
                    polygon: spec.polygon.clone(),
                    cut: spec.cut,
                    action: spec.action.clone(),
                    chosen: Vec::new(),
                },
            );
            out.changed.push(spec.layer.clone());
        }
        Op::Set { id, props } => {
            match parse_address(id) {
                Some((cel, mark)) => {
                    let layer = mark_owner(doc, cel)?;
                    let mut props = props.clone();
                    if let Some(color) = props.get("color").and_then(Value::as_str) {
                        let hex = doc.resolve_color(color).with_context(|| format!("set {id}"))?;
                        props.insert("color".into(), Value::String(hex));
                    }
                    out.push(&layer, cel, MarkChange::Set { id: mark, props });
                }
                None => set(doc, id, props).with_context(|| format!("set {id}"))?,
            }
            out.changed.push(id.clone());
        }
        Op::Remove { ids } => {
            ids.iter().try_for_each(|id| {
                if parse_address(id).is_none() && doc.locate(id).is_none() && !out.removed.contains(id) {
                    bail!("no item {id}");
                }
                Ok(())
            })?;
            ids.iter().try_for_each(|id| match parse_address(id) {
                Some((cel, mark)) => {
                    let layer = mark_owner(doc, cel)?;
                    out.push(&layer, cel, MarkChange::Remove { id: mark });
                    out.removed.push(id.clone());
                    Ok(())
                }
                None => remove(doc, id, out),
            })?;
        }
        Op::Transform { ids, transform } => {
            let mut cels: Vec<(String, Vec<u32>)> = Vec::new();
            ids.iter().try_for_each(|id| {
                let (cel, mark) = parse_address(id)
                    .ok_or_else(|| anyhow!("transform takes strokes and fills (<cel>:s<id>), not {id}; shapes take set {id} transform.x=..."))?;
                match cels.iter_mut().find(|(c, _)| c == cel) {
                    Some((_, marks)) => marks.push(mark),
                    None => cels.push((cel.to_string(), vec![mark])),
                }
                Ok::<_, anyhow::Error>(())
            })?;
            if cels.len() > 1 && transform.pivot.is_none() {
                bail!(
                    "the ids span cels {}; give a pivot so they turn and scale about one point",
                    cels.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>().join(" and ")
                );
            }
            cels.into_iter().try_for_each(|(cel, marks)| {
                let layer = mark_owner(doc, &cel)?;
                out.push(
                    &layer,
                    &cel,
                    MarkChange::Transform {
                        ids: marks,
                        transform: *transform,
                    },
                );
                Ok::<_, anyhow::Error>(())
            })?;
            out.changed.extend(ids.iter().cloned());
        }
        Op::Move { id, place } => {
            move_item(doc, id, place).with_context(|| format!("mv {id}"))?;
            out.changed.push(id.clone());
        }
        Op::Stroke(spec) => {
            let resolved = StrokeSpec {
                color: doc.resolve_color(&spec.color)?,
                second: spec.second.as_deref().map(|c| doc.resolve_color(c)).transpose()?,
                ..spec.clone()
            };
            let strokes = resolved.build().with_context(|| format!("stroke on {}", spec.layer))?;
            let cel = paint_cel_at(doc, &spec.layer, spec.frame, out)?;
            let lock = locks_alpha(doc, &spec.layer);
            out.push(
                &spec.layer,
                &cel,
                MarkChange::Add(
                    strokes
                        .iter()
                        .cloned()
                        .map(|s| Mark::Stroke(Stroke { lock, ..s }))
                        .collect(),
                ),
            );
            if let Some(name) = &spec.part {
                out.push(
                    &spec.layer,
                    &cel,
                    MarkChange::Part {
                        name: name.clone(),
                        count: strokes.len(),
                        ids: Vec::new(),
                    },
                );
            }
        }
        Op::Fill(spec) if spec.like.is_some() => {
            let Some(carried) = &spec.carried else {
                bail!(
                    "fills like another drawing's are carried by klad fill --like (or klad apply), which computes them first"
                );
            };
            carried.iter().try_for_each(|(cel, marks)| {
                let layer = mark_owner(doc, cel)?;
                out.push(&layer, cel, MarkChange::Put(marks.clone()));
                out.changed.push(cel.clone());
                Ok::<_, anyhow::Error>(())
            })?;
        }
        Op::Fill(spec) => {
            let mut fill = spec.build(doc).with_context(|| format!("fill on {}", spec.layer))?;
            fill.lock = locks_alpha(doc, &spec.layer);
            let cel = paint_cel_at(doc, &spec.layer, spec.frame, out)?;
            out.push(&spec.layer, &cel, MarkChange::Add(vec![Mark::Fill(fill)]));
            if let Some(name) = &spec.part {
                out.push(
                    &spec.layer,
                    &cel,
                    MarkChange::Part {
                        name: name.clone(),
                        count: 1,
                        ids: Vec::new(),
                    },
                );
            }
        }
        Op::Part { ids, name } => {
            let mut by_cel: BTreeMap<String, Vec<u32>> = BTreeMap::new();
            ids.iter().try_for_each(|text| {
                let (cel, id) = crate::markedit::parse_address(text)
                    .ok_or_else(|| anyhow!("{text:?} is not a mark address like c4:s12"))?;
                by_cel.entry(cel.to_string()).or_default().push(id);
                Ok::<_, anyhow::Error>(())
            })?;
            if by_cel.is_empty() {
                bail!("part needs at least one mark");
            }
            by_cel.into_iter().try_for_each(|(cel, ids)| {
                let layer = mark_owner(doc, &cel)?;
                out.push(
                    &layer,
                    &cel,
                    MarkChange::Part {
                        name: name.clone(),
                        count: 0,
                        ids,
                    },
                );
                out.changed.push(cel);
                Ok::<_, anyhow::Error>(())
            })?;
        }
        Op::Copy(spec) => {
            copy(doc, spec, out)?;
        }
        Op::Path { id, edit } => {
            edit_path(doc, id, edit).with_context(|| format!("path {id}"))?;
            out.changed.push(id.clone());
        }
        Op::Palette(edit) => {
            edit_palette(doc, edit)?;
            if !out.changed.iter().any(|c| c == "palette") {
                out.changed.push("palette".into());
            }
        }
        Op::Atmosphere(air) => {
            if let Some(air) = air {
                air.validate()?;
            }
            doc.atmosphere = air.clone();
            out.changed.push("atmosphere".into());
        }
        Op::Timeline { fps, frames, origin } => {
            timing::timeline(doc, *fps, *frames)?;
            if let Some(origin) = origin {
                doc.timeline.origin = *origin;
            }
            out.changed.push("timeline".into());
        }
        Op::Frames(spec) => timing::frames(doc, spec, out)?,
        Op::Retime(spec) => {
            let note = retime::retime(doc, spec, out)?;
            out.notes.push(note);
        }
        Op::Duplicate { layer } => {
            let id = duplicate_layer(doc, layer, out)?;
            out.added.push(id);
        }
        Op::Ink(_)
        | Op::Light(_)
        | Op::Hatch(_)
        | Op::Paint(_)
        | Op::Scatter(_)
        | Op::Foliage(_)
        | Op::Grass(_)
        | Op::PaintOver(_) => {
            bail!(
                "ink, light, hatch, paint, scatter, foliage, grass and paintover are turned into strokes and fills by klad ink, klad light, klad hatch, klad paint, klad scatter, klad foliage, klad grass, klad paintover or klad apply first"
            );
        }
        Op::Bone(spec) => bone(doc, spec, out)?,
        Op::Weights(spec) => {
            let Some(made) = &spec.resolved else {
                bail!("weights are computed by klad weights (or klad apply) first");
            };
            rig_owner(doc, &spec.layer)?;
            out.weights.retain(|(l, _)| l != &spec.layer);
            out.weights.push((spec.layer.clone(), made.clone()));
            out.changed.push(spec.layer.clone());
        }
        Op::Pose(spec) => {
            let Some(made) = &spec.resolved else {
                bail!("poses are computed by klad pose (or klad apply) first");
            };
            posed(doc, made, out)?;
        }
        Op::Inbetween(spec) => {
            let Some(made) = &spec.resolved else {
                bail!(
                    "in-betweens are made by klad inbetween (or klad apply), which computes their strokes and fills first"
                );
            };
            inbetweens(doc, made, out)?;
        }
        Op::Cel(spec) => {
            timing::cel(doc, spec, out)?;
        }
        Op::Clip(edit) => {
            edit_clip(doc, edit)?;
            out.changed.push("timeline".into());
        }
        Op::Export { godot } => {
            doc.export.godot = godot.clone();
            out.changed.push("export".into());
        }
    }
    Ok(())
}

fn edit_path(doc: &mut Document, id: &str, edit: &PathEdit) -> Result<()> {
    let Some(Loc::Node(path, c, mut nodes)) = doc.locate(id) else {
        bail!("{id} is not a shape; path edits work on path, rect and ellipse nodes");
    };
    let index = nodes.pop().unwrap_or_default();
    let node = &mut node_list(doc, &path, c, &nodes)?[index];
    let mut subs = pathedit::from_shape(&node.shape)?;
    edit.apply(&mut subs)?;
    node.shape = Shape::Path {
        d: pathedit::to_d(&subs),
    };
    Ok(())
}

fn locks_alpha(doc: &Document, layer: &str) -> bool {
    doc.layer(layer).is_some_and(|l| l.lock_alpha)
}

pub fn everything(doc: &Document) -> Fill {
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let (fw, fh) = (w as f32, h as f32);
    let coverage = Coverage {
        x: 0,
        y: 0,
        width: w,
        height: h,
        alpha: vec![255; w as usize * h as usize],
    };
    Fill::new(
        0,
        [0.0, 0.0, 0.0, 1.0],
        FillSource::Polygon(vec![[0.0, 0.0], [fw, 0.0], [fw, fh], [0.0, fh]]),
        coverage,
    )
    .by(Author::Agent)
}

fn mark_owner(doc: &Document, cel: &str) -> Result<String> {
    match doc.locate(cel) {
        Some(Loc::Cel(path, c)) => {
            let layer = doc.layer_at(&path);
            if layer.kind() != Kind::Paint {
                bail!(
                    "{cel} belongs to {} layer {}; strokes and fills live in paint cels",
                    layer.kind().name(),
                    layer.id
                );
            }
            if let Some(source) = &layer.cels()[c].link {
                bail!("cel {cel} links to {source}; its strokes are {source}:s<id>");
            }
            Ok(layer.id.clone())
        }
        Some(_) => bail!("{cel} is not a cel; strokes are addressed as <cel>:s<id>"),
        None => bail!("no cel {cel}"),
    }
}

fn paint_cel_at(doc: &mut Document, layer: &str, frame: u32, out: &mut Applied) -> Result<String> {
    let path = layer_path(doc, layer)?;
    let target = doc.layer_at(&path);
    if target.kind() != Kind::Paint {
        bail!(
            "{layer} is a {} layer; strokes and fills go into paint layers",
            target.kind().name()
        );
    }
    if let Some(cel) = target.cel_at(frame).filter(|_| frame < doc.timeline.frames) {
        if let Some(source) = &cel.link {
            bail!(
                "cel {} at frame {frame} links to {source}; paint into {source} instead",
                cel.id
            );
        }
        return Ok(cel.id.clone());
    }
    let id = doc.alloc_id('c');
    let cels = doc
        .layer_at_mut(&path)
        .cels_mut()
        .ok_or_else(|| anyhow!("layer has no cels"))?;
    let index = cels.iter().position(|c| c.frame > frame).unwrap_or(cels.len());
    cels.insert(index, Cel::at(&id, frame));
    out.added.push(id.clone());
    Ok(id)
}

pub fn apply_all(doc: &Document, ops: &[Op]) -> Result<(Document, Applied)> {
    let mut next = doc.clone();
    let mut applied = Applied::default();
    ops.iter().enumerate().try_for_each(|(i, op)| {
        apply(&mut next, op, &mut applied).with_context(|| format!("op {}", i + 1))?;
        timing::grow(&mut next);
        Ok::<_, anyhow::Error>(())
    })?;
    next.validate()?;
    Ok((next, applied))
}

pub fn parse_lines(text: &str) -> Result<Vec<Op>> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with("//"))
        .map(|(n, line)| {
            let value: Value = serde_json::from_str(line).with_context(|| format!("line {}", n + 1))?;
            Op::parse(value).with_context(|| format!("line {}", n + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Paint;
    use crate::stroke::{Author, Sample};
    use serde_json::json;

    fn ops(lines: &[Value]) -> Vec<Op> {
        lines.iter().map(|v| Op::parse(v.clone()).unwrap()).collect()
    }

    fn base() -> Document {
        let mut doc = Document::new(512, 512);
        let (doc2, _) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"add","layer":{"kind":"vector","name":"shapes"}}),
                json!({"op":"add","layer":{"kind":"paint","name":"ink"}}),
            ]),
        )
        .unwrap();
        doc = doc2;
        doc
    }

    #[test]
    fn layers_and_nodes_get_ids_and_land_where_asked() {
        let doc = base();
        let (doc, applied) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"add","node":{"type":"rect","x":0,"y":0,"w":10,"h":10,"fill":"#ff0000"},"in":"l1"}),
                json!({"op":"add","node":{"type":"group","children":[{"type":"ellipse","cx":5,"cy":5,"rx":2,"ry":2}]},"in":"l1"}),
                json!({"op":"add","layer":{"kind":"group","name":"fx"},"below":"l1"}),
            ]),
        )
        .unwrap();
        assert_eq!(applied.added.len(), 3);
        assert_eq!(doc.layers[0].name, "fx");
        let cel = &doc.layer("l1").unwrap().cels()[0];
        assert_eq!(cel.nodes.len(), 2);
        assert_eq!(cel.nodes[1].children().len(), 1);
        assert!(doc.locate(&cel.nodes[1].children()[0].id).is_some());
    }

    #[test]
    fn set_patches_nested_values_and_rejects_typos() {
        let (doc, applied) = apply_all(
            &base(),
            &ops(&[
                json!({"op":"add","node":{"id":"sun","type":"ellipse","cx":5,"cy":5,"rx":2,"ry":2},"in":"l1"}),
                json!({"op":"set","id":"sun","fill":"#ffcc00","transform.rotate":15,"stroke.width":3}),
            ]),
        )
        .unwrap();
        assert_eq!(applied.changed, vec!["sun"]);
        let Some(Item::Node(_, _, sun)) = doc.item("sun") else {
            panic!()
        };
        assert_eq!(sun.fill, Some(Paint::Solid("#ffcc00".into())));
        assert_eq!(sun.transform.rotate, 15.0);
        assert_eq!(sun.stroke.as_ref().unwrap().width, 3.0);
        let typo = apply_all(&doc, &ops(&[json!({"op":"set","id":"sun","fil":"#000000"})]));
        assert!(format!("{:#}", typo.unwrap_err()).contains("has no \"fil\""));
        let structural = apply_all(&doc, &ops(&[json!({"op":"set","id":"l1","cels":[]})]));
        assert!(format!("{:#}", structural.unwrap_err()).contains("cannot be set"));
        let (cleared, _) = apply_all(&doc, &ops(&[json!({"op":"set","id":"sun","stroke":null})])).unwrap();
        let Some(Item::Node(_, _, sun)) = cleared.item("sun") else {
            panic!()
        };
        assert!(sun.stroke.is_none());
    }

    #[test]
    fn failed_batches_change_nothing() {
        let doc = base();
        let result = apply_all(
            &doc,
            &ops(&[
                json!({"op":"add","layer":{"kind":"paint"}}),
                json!({"op":"set","id":"nope","name":"x"}),
            ]),
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("op 2"));
        assert_eq!(doc.layers.len(), 2);
    }

    #[test]
    fn removing_paint_layers_reports_their_cels() {
        let doc = base();
        let ink = doc.layer("l3").unwrap().cels()[0].id.clone();
        let (doc, applied) = apply_all(&doc, &ops(&[json!({"op":"rm","id":"l3"})])).unwrap();
        assert_eq!(applied.dropped_cels, vec![("l3".to_string(), ink)]);
        assert_eq!(doc.layers.len(), 1);
    }

    #[test]
    fn cels_add_link_and_guard_their_links() {
        let doc = base();
        let first = doc.layer("l3").unwrap().cels()[0].id.clone();
        let (doc, _) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"set","id": first, "hold": 2}),
                json!({"op":"add","cel":{"frame":2,"link": first},"in":"l3"}),
            ]),
        )
        .unwrap();
        let layer = doc.layer("l3").unwrap();
        assert_eq!(layer.cels().len(), 2);
        assert_eq!(layer.content_at(3).unwrap().id, first);
        let (handed, applied) = apply_all(&doc, &ops(&[json!({"op":"rm","id": first})])).unwrap();
        let heir = &handed.layer("l3").unwrap().cels()[0];
        assert_eq!((heir.frame, heir.link.as_deref()), (2, None));
        assert!(matches!(&applied.marks[0].changes[0], MarkChange::Copy { cel, .. } if *cel == first));
        assert_eq!(applied.dropped_cels, [("l3".to_string(), first.clone())]);
        let overlap = apply_all(&doc, &ops(&[json!({"op":"set","id": first, "hold": 5})]));
        assert!(format!("{:#}", overlap.unwrap_err()).contains("overlaps"));
    }

    #[test]
    fn moves_reorder_layers_and_nodes() {
        let (doc, _) = apply_all(
            &base(),
            &ops(&[
                json!({"op":"add","node":{"id":"a","type":"path","d":"M0 0 L1 1"},"in":"l1"}),
                json!({"op":"add","node":{"id":"b","type":"path","d":"M0 0 L1 1"},"in":"l1"}),
                json!({"op":"add","layer":{"id":"g","kind":"group"}}),
                json!({"op":"mv","id":"l1","in":"g"}),
                json!({"op":"mv","id":"b","below":"a"}),
            ]),
        )
        .unwrap();
        assert_eq!(doc.locate("l1"), Some(Loc::Layer(vec![1, 0])));
        let cel = &doc.layer("l1").unwrap().cels()[0];
        assert_eq!(cel.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        let into_self = apply_all(&doc, &ops(&[json!({"op":"mv","id":"g","in":"l1"})]));
        assert!(into_self.is_err());
    }

    #[test]
    fn nodes_need_a_vector_home() {
        let mut doc = base();
        let (past, _) = apply_all(
            &doc,
            &ops(&[json!({"op":"add","node":{"type":"path","d":"M0 0 L2 2"},"in":"l1","frame":3})]),
        )
        .unwrap();
        assert_eq!((past.layer("l1").unwrap().cels().len(), past.timeline.frames), (2, 4));
        doc.timeline.frames = 8;
        let paint = apply_all(
            &doc,
            &ops(&[json!({"op":"add","node":{"type":"path","d":"M0 0"},"in":"l3"})]),
        );
        assert!(format!("{:#}", paint.unwrap_err()).contains("vector layers"));
        let nowhere = apply_all(&doc, &ops(&[json!({"op":"add","node":{"type":"path","d":"M0 0"}})]));
        assert!(format!("{:#}", nowhere.unwrap_err()).contains("say where"));
        let add_at_3 = json!({"op":"add","node":{"type":"path","d":"M0 0 L2 2"},"in":"l1","frame":3});
        let (held, _) = apply_all(&doc, &ops(std::slice::from_ref(&add_at_3))).unwrap();
        let cels = held.layer("l1").unwrap().cels();
        assert_eq!((cels.len(), cels[0].nodes.len()), (1, 1));
        let first = doc.layer("l1").unwrap().cels()[0].id.clone();
        let (framed, _) = apply_all(&doc, &ops(&[json!({"op":"set","id": first,"hold":2}), add_at_3])).unwrap();
        let cels = framed.layer("l1").unwrap().cels();
        assert_eq!(cels.len(), 2);
        assert_eq!((cels[1].frame, cels[1].nodes.len()), (3, 1));
    }

    #[test]
    fn stroke_ops_build_agent_strokes_into_paint_cels() {
        let doc = base();
        let line = json!({"op":"stroke","layer":"l3","brush":"ink","color":"#112233","size":10,
            "path":"M10 10 C 50 0 100 50 150 20 M 0 100 L 100 100","pressure":[0.2,1,0.1]});
        let (_, applied) = apply_all(&doc, &ops(&[line])).unwrap();
        let pending = &applied.marks[0];
        assert_eq!(pending.layer, "l3");
        let added: Vec<&Mark> = pending.added().collect();
        assert_eq!(added.len(), 2);
        let Mark::Stroke(first) = added[0] else { panic!() };
        assert_eq!(first.author, Author::Agent);
        assert!(first.samples.len() > 30);
        let pressures: Vec<f32> = first.samples.iter().map(Sample::pressure).collect();
        assert!(pressures[0] < 0.25 && pressures[pressures.len() / 2] > 0.8 && *pressures.last().unwrap() < 0.15);
        let vector = apply_all(&doc, &ops(&[json!({"op":"stroke","layer":"l1","path":"M0 0 L1 1"})]));
        assert!(format!("{:#}", vector.unwrap_err()).contains("paint layers"));
        let tapered = apply_all(
            &doc,
            &ops(&[json!({"op":"stroke","layer":"l3","path":"M0 0 L90 0","taper":[12,30]})]),
        )
        .unwrap()
        .1;
        assert_eq!(
            tapered.marks[0].added().next().unwrap().as_stroke().unwrap().taper,
            [12.0, 30.0]
        );
        assert!(
            apply_all(
                &doc,
                &ops(&[json!({"op":"stroke","layer":"l3","path":"M0 0 L9 0","taper":[-1,3]})])
            )
            .is_err()
        );
        let typo = Op::parse(json!({"op":"stroke","layer":"l3","path":"M0 0 L1 1","colour":"#fff"}));
        assert!(typo.is_err());
        let (framed, applied) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"set","id": doc.layer("l3").unwrap().cels()[0].id,"hold":1}),
                json!({"op":"stroke","layer":"l3","frame":2,"points":[[1,1,1],[5,5,0.5]]}),
            ]),
        )
        .unwrap();
        assert_eq!(framed.layer("l3").unwrap().cels().len(), 2);
        assert_eq!(
            applied.marks[0]
                .added()
                .next()
                .unwrap()
                .as_stroke()
                .unwrap()
                .samples
                .len(),
            2
        );
    }

    #[test]
    fn palette_ops_add_rename_remove_and_name_stroke_colours() {
        let (doc, applied) = apply_all(
            &base(),
            &ops(&[
                json!({"op":"palette","add":"#1b1b1b","name":"ink"}),
                json!({"op":"palette","add":"#f0c8a0","name":"skin"}),
                json!({"op":"palette","add":"#ffffff"}),
                json!({"op":"palette","add":"#ffffff"}),
                json!({"op":"palette","add":"#d0a080","name":"skin"}),
                json!({"op":"palette","add":"#ff0000","index":0}),
                json!({"op":"palette","rm":0}),
            ]),
        )
        .unwrap();
        assert_eq!(applied.changed, vec!["palette"]);
        let colors: Vec<(&str, &str)> = doc
            .palette
            .iter()
            .map(|s| (s.color.as_str(), s.name.as_str()))
            .collect();
        assert_eq!(colors, [("#1b1b1b", "ink"), ("#d0a080", "skin"), ("#ffffff", "")]);
        let (_, applied) = apply_all(
            &doc,
            &ops(&[json!({"op":"stroke","layer":"l3","path":"M0 0 L9 9","color":"skin"})]),
        )
        .unwrap();
        let stroke = applied.marks[0].added().next().unwrap().as_stroke().unwrap();
        assert_eq!(stroke.color, crate::color::to_u16(parse_hex("#d0a080").unwrap()));
        let unknown = apply_all(
            &doc,
            &ops(&[json!({"op":"stroke","layer":"l3","path":"M0 0 L9 9","color":"teal"})]),
        );
        assert!(format!("{:#}", unknown.unwrap_err()).contains("ink, skin"));
        let missing = apply_all(&doc, &ops(&[json!({"op":"palette","rm":"teal"})]));
        assert!(format!("{:#}", missing.unwrap_err()).contains("teal"));
        let (set, _) = apply_all(
            &doc,
            &ops(&[json!({"op":"palette","set":[{"color":"#000000","name":"night"}]})]),
        )
        .unwrap();
        assert_eq!(set.palette.len(), 1);
        assert!(Op::parse(json!({"op":"palette","add":"#000000","colour":"x"})).is_err());
        assert!(apply_all(&doc, &ops(&[json!({"op":"palette","add":"black"})])).is_err());
    }

    #[test]
    fn fill_ops_need_coverage_and_resolve_their_reference() {
        let doc = base();
        let parsed = Op::parse(json!({"op":"fill","layer":"l3","at":[10,10],"ref":"shapes","gap":4})).unwrap();
        let Op::Fill(mut spec) = parsed else { panic!() };
        assert_eq!(spec.reference(&doc).unwrap(), FillRef::Layer("l1".into()));
        let missing = apply_all(&doc, &[Op::Fill(spec.clone())]);
        assert!(format!("{:#}", missing.unwrap_err()).contains("coverage"));
        spec.coverage = Some(Coverage::from_mask(2, 2, &[255, 0, 0, 255]));
        let (_, applied) = apply_all(&doc, &[Op::Fill(spec.clone())]).unwrap();
        let Some(Mark::Fill(fill)) = applied.marks[0].added().next() else {
            panic!()
        };
        assert_eq!(fill.author, Author::Agent);
        assert_eq!(fill.reference(), Some(&FillRef::Layer("l1".into())));
        let own = FillSpec {
            reference: Some("l3".into()),
            ..spec.clone()
        };
        assert_eq!(own.reference(&doc).unwrap(), FillRef::This);
        let nowhere = FillSpec {
            reference: Some("lines".into()),
            ..spec.clone()
        };
        let err = nowhere.reference(&doc).unwrap_err().to_string();
        assert!(err.contains("l1 (shapes)") && err.contains("this or all"), "{err}");
        assert!(Op::parse(json!({"op":"fill","layer":"l3"})).is_err());
        assert!(Op::parse(json!({"op":"fill","layer":"l3","at":[1,1],"gap":-1})).is_err());
        assert!(Op::parse(json!({"op":"fill","layer":"l3","polygon":[[0,0],[5,5]]})).is_err());
        let lasso =
            Op::parse(json!({"op":"fill","layer":"l3","polygon":[[0,0],[5,0],[5,5]],"color":"#ff0000"})).unwrap();
        let Op::Fill(lasso) = lasso else { panic!() };
        assert!(matches!(lasso.source(&doc).unwrap(), FillSource::Polygon(p) if p.len() == 3));
    }

    #[test]
    fn path_ops_reshape_paths_and_convert_shapes() {
        let (doc, applied) = apply_all(
            &base(),
            &ops(&[
                json!({"op":"add","node":{"id":"ridge","type":"path","d":"M0 100 L50 60 L100 100"},"in":"l1"}),
                json!({"op":"add","node":{"id":"box","type":"rect","x":0,"y":0,"w":10,"h":10},"in":"l1"}),
                json!({"op":"path","id":"ridge","move":1,"to":[50,40]}),
                json!({"op":"path","id":"box"}),
            ]),
        )
        .unwrap();
        assert_eq!(applied.changed, ["ridge", "box"]);
        let shape = |id: &str| match doc.item(id) {
            Some(Item::Node(_, _, n)) => n.shape.clone(),
            _ => panic!(),
        };
        assert_eq!(
            shape("ridge"),
            Shape::Path {
                d: "M0 100 L50 40 L100 100".into()
            }
        );
        assert_eq!(
            shape("box"),
            Shape::Path {
                d: "M0 0 L10 0 L10 10 L0 10 Z".into()
            }
        );
        assert!(Op::parse(json!({"op":"path","id":"ridge","to":[1,1]})).is_err());
        let group = apply_all(
            &doc,
            &ops(&[
                json!({"op":"add","node":{"id":"g","type":"group","children":[]},"in":"l1"}),
                json!({"op":"path","id":"g","corner":0}),
            ]),
        );
        assert!(format!("{:#}", group.unwrap_err()).contains("groups have no points"));
    }

    #[test]
    fn clip_ops_add_set_rename_and_remove_clips_in_frame_order() {
        let mut doc = base();
        doc.timeline.frames = 24;
        let (doc, applied) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"clip","add":"run","from":12,"to":19,"loop":true,"fps":24}),
                json!({"op":"clip","add":"idle","from":0,"to":5}),
            ]),
        )
        .unwrap();
        assert_eq!(applied.changed, vec!["timeline", "timeline"]);
        let t = &doc.timeline;
        assert_eq!(
            t.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["idle", "run"]
        );
        assert_eq!((t.fps_of(&t.clips[0]), t.fps_of(&t.clips[1])), (12, 24));
        assert!(!t.clips[0].looping && t.clips[1].looping);
        assert_eq!(t.clip_at(14).map(|c| c.name.as_str()), Some("run"));
        assert_eq!(t.clip_at(8), None);
        let (doc, _) = apply_all(
            &doc,
            &ops(&[json!({"op":"clip","set":"run","name":"sprint","from":1,"loop":false,"fps":null})]),
        )
        .unwrap();
        let sprint = doc.timeline.clip("sprint").unwrap();
        assert_eq!(
            (sprint.from, sprint.to, sprint.looping, sprint.fps),
            (1, 19, false, None)
        );
        assert_eq!(
            doc.timeline.clips[1].name, "sprint",
            "kept in order of their first frame"
        );
        let text = serde_json::to_string(&doc.timeline).unwrap();
        assert!(text.contains(r#""name":"idle","from":0,"to":5,"loop":false"#), "{text}");
        let (gone, _) = apply_all(&doc, &ops(&[json!({"op":"clip","rm":"idle"})])).unwrap();
        assert_eq!(gone.timeline.clips.len(), 1);
        let fails = |line: Value| format!("{:#}", apply_all(&doc, &ops(&[line])).unwrap_err());
        assert!(fails(json!({"op":"clip","add":"idle","from":0,"to":1})).contains("already"));
        assert!(fails(json!({"op":"clip","set":"walk","to":3})).contains("idle, sprint"));
        assert!(fails(json!({"op":"clip","set":"idle","to":24})).contains("outside 0 to 23"));
        assert!(fails(json!({"op":"clip","set":"idle","from":9})).contains("spans frames 9 to 5"));
        assert!(fails(json!({"op":"clip","set":"idle","name":"sprint"})).contains("used twice"));
        assert!(Op::parse(json!({"op":"clip","add":"x","from":0})).is_err());
        assert!(Op::parse(json!({"op":"clip","add":"x","from":0,"to":1,"fps":0})).is_err());
        assert!(Op::parse(json!({"op":"clip","add":"x","from":0,"to":1,"speed":2})).is_err());
    }

    #[test]
    fn frame_ops_stretch_clips_and_a_shorter_timeline_trims_them() {
        let mut doc = base();
        doc.timeline.frames = 12;
        let (doc, _) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"clip","add":"a","from":0,"to":3,"loop":true}),
                json!({"op":"clip","add":"b","from":4,"to":11}),
            ]),
        )
        .unwrap();
        let spans = |d: &Document| d.timeline.clips.iter().map(|c| (c.from, c.to)).collect::<Vec<_>>();
        let (inserted, _) = apply_all(&doc, &ops(&[json!({"op":"frames","insert":3,"count":2})])).unwrap();
        assert_eq!(spans(&inserted), [(0, 5), (6, 13)]);
        let (removed, _) = apply_all(&doc, &ops(&[json!({"op":"frames","remove":2,"count":4})])).unwrap();
        assert_eq!(spans(&removed), [(0, 1), (2, 7)]);
        let (listed, _) = apply_all(
            &doc,
            &ops(&[json!({"op":"frames","insert":3,"count":2,"layers":["ink"]})]),
        )
        .unwrap();
        assert_eq!(spans(&listed), spans(&doc), "an insert on some layers leaves the clips");
        let (short, _) = apply_all(&doc, &ops(&[json!({"op":"timeline","frames":8})])).unwrap();
        assert_eq!(spans(&short), [(0, 3), (4, 7)]);
        let refused = apply_all(&doc, &ops(&[json!({"op":"timeline","frames":4})]));
        assert!(format!("{:#}", refused.unwrap_err()).contains("clip b starts at frame 4"));
    }

    #[test]
    fn the_origin_and_the_godot_folder_are_document_settings() {
        let (doc, _) = apply_all(
            &base(),
            &ops(&[
                json!({"op":"timeline","origin":[256,480]}),
                json!({"op":"export","godot":"../game/art/ball"}),
            ]),
        )
        .unwrap();
        assert_eq!(doc.timeline.origin, Some([256.0, 480.0]));
        assert_eq!(doc.export.godot.as_deref(), Some("../game/art/ball"));
        let text = serde_json::to_string(&doc).unwrap();
        assert!(text.contains(r#""export":{"godot":"../game/art/ball"}"#), "{text}");
        let back: Document = serde_json::from_str(&text).unwrap();
        assert_eq!(back, doc);
        let (cleared, _) = apply_all(
            &doc,
            &ops(&[
                json!({"op":"timeline","origin":null}),
                json!({"op":"export","godot":null}),
            ]),
        )
        .unwrap();
        assert_eq!((cleared.timeline.origin, cleared.export.godot), (None, None));
        assert!(!serde_json::to_string(&base()).unwrap().contains("export"));
        assert!(Op::parse(json!({"op":"export","unity":"x"})).is_err());
        assert!(Op::parse(json!({"op":"timeline","origin":[1]})).is_err());
    }

    #[test]
    fn json_lines_parse_with_comments_and_report_line_numbers() {
        let parsed = parse_lines("// setup\n{\"op\":\"rm\",\"id\":\"x\"}\n\n{\"op\":\"mv\",\"id\":\"y\",\"top\":1}\n");
        assert!(format!("{:#}", parsed.unwrap_err()).contains("line 4"));
        assert_eq!(parse_lines("{\"op\":\"rm\",\"ids\":[\"a\",\"b\"]}").unwrap().len(), 1);
    }
}
