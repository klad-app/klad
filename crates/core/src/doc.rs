use crate::color::parse_hex;
use crate::generators::{Mirror, Picture, Repeat, Scatter, Text};
use crate::look::{Adjustment, Atmosphere, Effect, Rays};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAT: &str = "klad/3";
pub const MAX_SIZE: u32 = 16384;
pub const MASK: usize = usize::MAX;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub format: String,
    pub rev: u64,
    pub canvas: Canvas,
    #[serde(default)]
    pub timeline: Timeline,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<Swatch>,
    pub layers: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Exports::is_empty")]
    pub export: Exports,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atmosphere: Option<Atmosphere>,
    #[serde(default)]
    pub next_id: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exports {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub godot: Option<String>,
}

impl Exports {
    pub fn is_empty(&self) -> bool {
        self.godot.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swatch {
    pub color: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub fps: u32,
    pub frames: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            fps: 12,
            frames: 1,
            origin: None,
            clips: Vec::new(),
        }
    }
}

impl Timeline {
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.name == name)
    }

    pub fn clip_at(&self, frame: u32) -> Option<&Clip> {
        self.clips.iter().find(|c| c.from <= frame && frame <= c.to)
    }

    pub fn fps_of(&self, clip: &Clip) -> u32 {
        clip.fps.unwrap_or(self.fps)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub from: u32,
    pub to: u32,
    #[serde(default, rename = "loop")]
    pub looping: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
}

impl Clip {
    pub fn new(name: &str, from: u32, to: u32, looping: bool) -> Self {
        Self {
            name: name.to_string(),
            from,
            to,
            looping,
            fps: None,
        }
    }

    pub fn frames(&self) -> u32 {
        self.to - self.from + 1
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub content: LayerContent,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: Blend,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub clip: bool,
    #[serde(default, rename = "lockAlpha", skip_serializing_if = "is_false")]
    pub lock_alpha: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Box<Layer>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reference: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bones: Vec<crate::rig::Bone>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ik: Vec<crate::rig::Ik>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: crate::keys::Keys,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LayerContent {
    Paint {
        #[serde(default)]
        cels: Vec<Cel>,
    },
    Vector {
        #[serde(default)]
        cels: Vec<Cel>,
    },
    Group {
        #[serde(default)]
        children: Vec<Layer>,
    },
    Adjust {
        adjust: Adjustment,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Paint,
    Vector,
    Group,
    Adjust,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Paint => "paint",
            Kind::Vector => "vector",
            Kind::Group => "group",
            Kind::Adjust => "adjust",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    ColorDodge,
    SoftLight,
    HardLight,
    Lighten,
    Darken,
    Color,
    Luminosity,
}

impl Blend {
    pub const ALL: [Blend; 12] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
        Blend::Add,
        Blend::ColorDodge,
        Blend::SoftLight,
        Blend::HardLight,
        Blend::Lighten,
        Blend::Darken,
        Blend::Color,
        Blend::Luminosity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Multiply => "multiply",
            Blend::Screen => "screen",
            Blend::Overlay => "overlay",
            Blend::Add => "add",
            Blend::ColorDodge => "color-dodge",
            Blend::SoftLight => "soft-light",
            Blend::HardLight => "hard-light",
            Blend::Lighten => "lighten",
            Blend::Darken => "darken",
            Blend::Color => "color",
            Blend::Luminosity => "luminosity",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cel {
    pub id: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tween: Option<Tween>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<crate::rig::Pose>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tween {
    pub from: String,
    pub to: String,
    pub t: f32,
    #[serde(default, rename = "match", skip_serializing_if = "String::is_empty")]
    pub matching: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub made: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unpaired: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<StrokeStyle>,
    #[serde(default, skip_serializing_if = "Transform::is_identity")]
    pub transform: Transform,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub blur: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: crate::keys::Keys,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Shape {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default, skip_serializing_if = "is_zero")]
        radius: f32,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    },
    Path {
        d: String,
    },
    Group {
        #[serde(default)]
        children: Vec<Node>,
    },
    Rays(Rays),
    Image(Picture),
    Scatter(Scatter),
    Repeat(Repeat),
    Mirror(Mirror),
    Text(Text),
}

impl Shape {
    pub fn children(&self) -> &[Node] {
        match self {
            Shape::Group { children }
            | Shape::Scatter(Scatter { children, .. })
            | Shape::Repeat(Repeat { children, .. })
            | Shape::Mirror(Mirror { children, .. }) => children,
            _ => &[],
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Shape::Group { children }
            | Shape::Scatter(Scatter { children, .. })
            | Shape::Repeat(Repeat { children, .. })
            | Shape::Mirror(Mirror { children, .. }) => Some(children),
            _ => None,
        }
    }

    pub fn generates(&self) -> bool {
        matches!(self, Shape::Scatter(_) | Shape::Repeat(_) | Shape::Mirror(_))
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Shape::Rays(r) => r.validate(),
            Shape::Image(p) => p.validate(),
            Shape::Scatter(s) => s.validate(),
            Shape::Repeat(r) => r.validate(),
            Shape::Text(t) => t.validate(),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Paint {
    Solid(String),
    Linear { linear: Linear },
    Radial { radial: Radial },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Linear {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub stops: Vec<Stop>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Radial {
    pub center: [f32; 2],
    pub radius: f32,
    pub stops: Vec<Stop>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop(pub f32, pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    #[serde(default = "black")]
    pub paint: Paint,
    #[serde(default = "one")]
    pub width: f32,
    #[serde(default)]
    pub cap: Cap,
    #[serde(default)]
    pub join: Join,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dash: Vec<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cap {
    Butt,
    #[default]
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Join {
    Miter,
    #[default]
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotate: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub sx: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub sy: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<[f32; 2]>,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            rotate: 0.0,
            sx: 1.0,
            sy: 1.0,
            pivot: None,
        }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

fn black() -> Paint {
    Paint::Solid("#000000".into())
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_true(v: &bool) -> bool {
    *v
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loc {
    Layer(Vec<usize>),
    Cel(Vec<usize>, usize),
    Node(Vec<usize>, usize, Vec<usize>),
}

pub enum Item<'a> {
    Layer(&'a Layer),
    Cel(&'a Layer, &'a Cel),
    Node(&'a Layer, &'a Cel, &'a Node),
}

impl Layer {
    fn with(id: &str, name: &str, content: LayerContent) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            content,
            opacity: 1.0,
            blend: Blend::Normal,
            visible: true,
            locked: false,
            clip: false,
            lock_alpha: false,
            mask: None,
            effects: Vec::new(),
            depth: None,
            reference: false,
            bones: Vec::new(),
            ik: Vec::new(),
            bone: None,
            keys: BTreeMap::new(),
        }
    }

    pub fn paint(id: &str, name: &str, cel: &str) -> Self {
        Self::with(
            id,
            name,
            LayerContent::Paint {
                cels: vec![Cel::at(cel, 0)],
            },
        )
    }

    pub fn vector(id: &str, name: &str, cel: &str, nodes: Vec<Node>) -> Self {
        let mut first = Cel::at(cel, 0);
        first.nodes = nodes;
        Self::with(id, name, LayerContent::Vector { cels: vec![first] })
    }

    pub fn group(id: &str, name: &str, children: Vec<Layer>) -> Self {
        Self::with(id, name, LayerContent::Group { children })
    }

    pub fn adjust(id: &str, name: &str, adjust: Adjustment) -> Self {
        Self::with(id, name, LayerContent::Adjust { adjust })
    }

    pub fn kind(&self) -> Kind {
        match self.content {
            LayerContent::Paint { .. } => Kind::Paint,
            LayerContent::Vector { .. } => Kind::Vector,
            LayerContent::Group { .. } => Kind::Group,
            LayerContent::Adjust { .. } => Kind::Adjust,
        }
    }

    pub fn adjustment(&self) -> Option<&Adjustment> {
        match &self.content {
            LayerContent::Adjust { adjust } => Some(adjust),
            _ => None,
        }
    }

    pub fn reads_under(&self) -> bool {
        self.adjustment().is_some_and(Adjustment::reads_under)
    }

    pub fn cels(&self) -> &[Cel] {
        match &self.content {
            LayerContent::Paint { cels } | LayerContent::Vector { cels } => cels,
            LayerContent::Group { .. } | LayerContent::Adjust { .. } => &[],
        }
    }

    pub fn cels_mut(&mut self) -> Option<&mut Vec<Cel>> {
        match &mut self.content {
            LayerContent::Paint { cels } | LayerContent::Vector { cels } => Some(cels),
            LayerContent::Group { .. } | LayerContent::Adjust { .. } => None,
        }
    }

    pub fn children(&self) -> &[Layer] {
        match &self.content {
            LayerContent::Group { children } => children,
            _ => &[],
        }
    }

    pub fn child(&self, index: usize) -> Option<&Layer> {
        match index {
            MASK => self.mask.as_deref(),
            i => self.children().get(i),
        }
    }

    fn child_mut(&mut self, index: usize) -> &mut Layer {
        match (index, &mut self.content) {
            (MASK, _) => self
                .mask
                .as_deref_mut()
                .expect("layer path runs through a missing mask"),
            (i, LayerContent::Group { children }) => &mut children[i],
            _ => panic!("layer path runs through a non-group layer"),
        }
    }

    pub fn mask_at(&self, frame: u32) -> Option<(&Layer, &Cel)> {
        let mask = self.mask.as_deref().filter(|m| m.visible)?;
        Some((mask, mask.content_at(frame)?))
    }

    pub fn cel_at(&self, frame: u32) -> Option<&Cel> {
        let cel = self.cels().iter().rev().find(|c| c.frame <= frame)?;
        match cel.hold {
            Some(hold) if frame >= cel.frame + hold => None,
            _ => Some(cel),
        }
    }

    pub fn content_of<'a>(&'a self, cel: &'a Cel) -> &'a Cel {
        cel.link
            .as_ref()
            .and_then(|id| self.cels().iter().find(|c| &c.id == id))
            .unwrap_or(cel)
    }

    pub fn content_at(&self, frame: u32) -> Option<&Cel> {
        self.cel_at(frame).map(|c| self.content_of(c))
    }
}

impl Cel {
    pub fn at(id: &str, frame: u32) -> Self {
        Self {
            id: id.to_string(),
            frame,
            hold: None,
            link: None,
            tween: None,
            nodes: Vec::new(),
            parts: BTreeMap::new(),
            pose: None,
        }
    }

    pub fn part_of(&self, id: u32) -> Option<&str> {
        self.parts
            .iter()
            .find(|(_, ids)| ids.contains(&id))
            .map(|(name, _)| name.as_str())
    }

    pub fn set_part(&mut self, name: &str, ids: &[u32]) {
        self.parts
            .values_mut()
            .for_each(|list| list.retain(|i| !ids.contains(i)));
        if !name.is_empty() {
            let list = self.parts.entry(name.to_string()).or_default();
            list.extend(ids.iter().filter(|i| !list.contains(i)).copied().collect::<Vec<_>>());
        }
        self.parts.retain(|_, list| !list.is_empty());
    }
}

impl Node {
    pub fn new(id: &str, shape: Shape) -> Self {
        Self {
            id: id.to_string(),
            name: String::new(),
            shape,
            fill: None,
            stroke: None,
            transform: Transform::default(),
            opacity: 1.0,
            blur: 0.0,
            clip: None,
            visible: true,
            keys: BTreeMap::new(),
        }
    }

    pub fn filled(mut self, hex: &str) -> Self {
        self.fill = Some(Paint::Solid(hex.to_string()));
        self
    }

    pub fn children(&self) -> &[Node] {
        self.shape.children()
    }

    pub fn type_name(&self) -> &'static str {
        match self.shape {
            Shape::Rect { .. } => "rect",
            Shape::Ellipse { .. } => "ellipse",
            Shape::Path { .. } => "path",
            Shape::Group { .. } => "group",
            Shape::Rays(_) => "rays",
            Shape::Image(_) => "image",
            Shape::Scatter(_) => "scatter",
            Shape::Repeat(_) => "repeat",
            Shape::Mirror(_) => "mirror",
            Shape::Text(_) => "text",
        }
    }
}

pub fn family_paths(doc: &Document, path: &[usize]) -> Vec<Vec<usize>> {
    let layer = doc.layer_at(path);
    std::iter::once(path.to_vec())
        .chain((0..layer.children().len()).flat_map(|i| family_paths(doc, &[path, &[i]].concat())))
        .collect()
}

pub fn walk_children<'a>(layer: &'a Layer, out: &mut Vec<&'a Layer>) {
    walk_layers(layer.children(), out);
}

fn walk_layers<'a>(layers: &'a [Layer], out: &mut Vec<&'a Layer>) {
    layers.iter().for_each(|l| {
        out.push(l);
        out.extend(l.mask.as_deref());
        walk_layers(l.children(), out);
    });
}

fn walk_nodes<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    nodes.iter().for_each(|n| {
        out.push(n);
        walk_nodes(n.children(), out);
    });
}

fn find_node(nodes: &[Node], id: &str, path: &mut Vec<usize>) -> bool {
    nodes.iter().enumerate().any(|(i, n)| {
        path.push(i);
        if n.id == id || find_node(n.children(), id, path) {
            return true;
        }
        path.pop();
        false
    })
}

fn find_here(layer: &Layer, id: &str, path: &[usize]) -> Option<Loc> {
    if layer.id == id {
        return Some(Loc::Layer(path.to_vec()));
    }
    layer.cels().iter().enumerate().find_map(|(c, cel)| {
        if cel.id == id {
            return Some(Loc::Cel(path.to_vec(), c));
        }
        let mut nodes = Vec::new();
        find_node(&cel.nodes, id, &mut nodes).then(|| Loc::Node(path.to_vec(), c, nodes))
    })
}

fn find_in(layers: &[Layer], id: &str, path: &mut Vec<usize>) -> Option<Loc> {
    layers.iter().enumerate().find_map(|(i, layer)| {
        path.push(i);
        let found = find_here(layer, id, path)
            .or_else(|| {
                let mask = layer.mask.as_deref()?;
                path.push(MASK);
                let found = find_here(mask, id, path);
                path.pop();
                found
            })
            .or_else(|| find_in(layer.children(), id, path));
        path.pop();
        found
    })
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn check_color(text: &str) -> Result<()> {
    if text == "none" {
        return Ok(());
    }
    parse_hex(text).map(|_| ())
}

fn check_paint(paint: &Paint) -> Result<()> {
    let stops = match paint {
        Paint::Solid(text) => return check_color(text),
        Paint::Linear { linear } => &linear.stops,
        Paint::Radial { radial } => {
            if radial.radius <= 0.0 {
                bail!("radial gradient radius must be positive");
            }
            &radial.stops
        }
    };
    if stops.is_empty() {
        bail!("a gradient needs at least one stop");
    }
    stops.iter().try_for_each(|Stop(at, color)| {
        if !(0.0..=1.0).contains(at) {
            bail!("gradient stop {at} is outside 0..1");
        }
        parse_hex(color).map(|_| ())
    })
}

fn check_node(node: &Node) -> Result<()> {
    let context = || format!("node {}", node.id);
    if !(0.0..=1.0).contains(&node.opacity) {
        bail!("{}: opacity {} is outside 0..1", context(), node.opacity);
    }
    if !(0.0..=500.0).contains(&node.blur) {
        bail!("{}: blur {} is outside 0..500", context(), node.blur);
    }
    match &node.shape {
        Shape::Rect { w, h, radius, .. } if *w < 0.0 || *h < 0.0 || *radius < 0.0 => {
            bail!("{}: width, height and radius cannot be negative", context())
        }
        Shape::Ellipse { rx, ry, .. } if *rx < 0.0 || *ry < 0.0 => {
            bail!("{}: radii cannot be negative", context())
        }
        Shape::Path { d } if d.trim().is_empty() => bail!("{}: empty path", context()),
        _ => {}
    }
    if node.clip.as_ref().is_some_and(|c| c.trim().is_empty()) {
        bail!("{}: empty clip path", context());
    }
    node.fill.iter().try_for_each(check_paint).with_context(context)?;
    if let Some(stroke) = &node.stroke {
        check_paint(&stroke.paint).with_context(context)?;
        if stroke.width < 0.0 {
            bail!("{}: stroke width cannot be negative", context());
        }
    }
    Ok(())
}

fn check_cels(layer: &Layer) -> Result<()> {
    let cels = layer.cels();
    cels.windows(2).try_for_each(|pair| {
        let (a, b) = (&pair[0], &pair[1]);
        if a.frame >= b.frame {
            bail!("layer {}: cels {} and {} are not in frame order", layer.id, a.id, b.id);
        }
        if a.hold.is_some_and(|h| a.frame + h > b.frame) {
            bail!("layer {}: cel {} overlaps cel {}", layer.id, a.id, b.id);
        }
        Ok(())
    })?;
    cels.iter().try_for_each(|cel| {
        if cel.hold == Some(0) {
            bail!("cel {}: hold must be at least 1", cel.id);
        }
        if let Some(target) = &cel.link {
            let Some(source) = cels.iter().find(|c| &c.id == target) else {
                bail!(
                    "cel {} links to {target}, which is not a cel of layer {}",
                    cel.id,
                    layer.id
                );
            };
            if source.link.is_some() {
                bail!("cel {} links to {target}, which is itself a link", cel.id);
            }
            if !cel.nodes.is_empty() {
                bail!("cel {} is a link and cannot hold nodes", cel.id);
            }
            if cel.tween.is_some() {
                bail!("cel {} is a link and cannot be an in-between", cel.id);
            }
        }
        if let Some(tween) = &cel.tween
            && !(tween.t > 0.0 && tween.t < 1.0)
        {
            bail!(
                "cel {}: an in-between's t must be between 0 and 1, not {}",
                cel.id,
                tween.t
            );
        }
        if layer.kind() == Kind::Paint && !cel.nodes.is_empty() {
            bail!(
                "cel {} belongs to paint layer {} and cannot hold nodes",
                cel.id,
                layer.id
            );
        }
        let mut nodes = Vec::new();
        walk_nodes(&cel.nodes, &mut nodes);
        nodes.iter().try_for_each(|n| check_node(n))
    })
}

fn check_mask(owner: &Layer, mask: &Layer) -> Result<()> {
    let context = format!("mask {} of layer {}", mask.id, owner.id);
    if mask.kind() != Kind::Paint {
        bail!("{context} must be a paint layer");
    }
    if mask.mask.is_some() || mask.clip {
        bail!("{context} cannot have a mask or clip");
    }
    if mask.opacity != 1.0 || mask.blend != Blend::Normal {
        bail!("{context} keeps opacity 1 and blend normal; hide it with visible=false");
    }
    Ok(())
}

impl Document {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            format: FORMAT.to_string(),
            rev: 0,
            canvas: Canvas {
                width,
                height,
                background: None,
            },
            timeline: Timeline::default(),
            palette: Vec::new(),
            layers: Vec::new(),
            export: Exports::default(),
            atmosphere: None,
            next_id: 0,
        }
    }

    pub fn all_layers(&self) -> Vec<&Layer> {
        let mut out = Vec::new();
        walk_layers(&self.layers, &mut out);
        out
    }

    pub fn ids(&self) -> BTreeSet<String> {
        self.all_layers()
            .iter()
            .flat_map(|layer| {
                let mut nodes = Vec::new();
                layer.cels().iter().for_each(|c| walk_nodes(&c.nodes, &mut nodes));
                std::iter::once(layer.id.clone())
                    .chain(layer.cels().iter().map(|c| c.id.clone()))
                    .chain(nodes.into_iter().map(|n| n.id.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn alloc_id(&mut self, prefix: char) -> String {
        let mut taken = self.ids();
        self.alloc_id_in(prefix, &mut taken)
    }

    pub fn alloc_id_in(&mut self, prefix: char, taken: &mut BTreeSet<String>) -> String {
        loop {
            self.next_id += 1;
            let id = format!("{prefix}{}", self.next_id);
            if taken.insert(id.clone()) {
                return id;
            }
        }
    }

    pub fn locate(&self, id: &str) -> Option<Loc> {
        find_in(&self.layers, id, &mut Vec::new())
    }

    pub fn locate_path(&self, path: &[usize]) -> Option<&Layer> {
        let (first, rest) = path.split_first()?;
        rest.iter()
            .try_fold(self.layers.get(*first)?, |layer, &i| layer.child(i))
    }

    pub fn layer_at(&self, path: &[usize]) -> &Layer {
        self.locate_path(path).expect("layer path leads nowhere")
    }

    pub fn layer_at_mut(&mut self, path: &[usize]) -> &mut Layer {
        let (first, rest) = path.split_first().expect("empty layer path");
        rest.iter()
            .fold(&mut self.layers[*first], |layer, &i| layer.child_mut(i))
    }

    pub fn is_mask(&self, id: &str) -> bool {
        matches!(self.locate(id), Some(Loc::Layer(path)) if path.last() == Some(&MASK))
    }

    pub fn layer_list_mut(&mut self, parent: &[usize]) -> Option<&mut Vec<Layer>> {
        if parent.is_empty() {
            return Some(&mut self.layers);
        }
        match &mut self.layer_at_mut(parent).content {
            LayerContent::Group { children } => Some(children),
            _ => None,
        }
    }

    pub fn item(&self, id: &str) -> Option<Item<'_>> {
        Some(match self.locate(id)? {
            Loc::Layer(path) => Item::Layer(self.layer_at(&path)),
            Loc::Cel(path, c) => {
                let layer = self.layer_at(&path);
                Item::Cel(layer, &layer.cels()[c])
            }
            Loc::Node(path, c, nodes) => {
                let layer = self.layer_at(&path);
                let cel = &layer.cels()[c];
                let (first, rest) = nodes.split_first()?;
                let node = rest.iter().fold(&cel.nodes[*first], |n, &i| &n.children()[i]);
                Item::Node(layer, cel, node)
            }
        })
    }

    pub fn layer(&self, id: &str) -> Option<&Layer> {
        match self.item(id)? {
            Item::Layer(layer) => Some(layer),
            _ => None,
        }
    }

    pub fn find_layer(&self, key: &str) -> Result<&Layer> {
        if let Some(layer) = self.layer(key) {
            return Ok(layer);
        }
        let named: Vec<&Layer> = self.all_layers().into_iter().filter(|l| l.name == key).collect();
        match named[..] {
            [layer] => Ok(layer),
            [] => bail!(
                "no layer {key:?}: layers are {}",
                self.all_layers()
                    .iter()
                    .map(|l| format!("{} ({})", l.id, l.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => bail!("{} layers are named {key:?}; use the layer id", named.len()),
        }
    }

    pub fn paint_content_at(&self, frame: u32) -> Vec<(&Layer, &Cel)> {
        self.all_layers()
            .into_iter()
            .filter(|l| l.kind() == Kind::Paint)
            .filter_map(|l| l.content_at(frame).map(|c| (l, c)))
            .collect()
    }

    pub fn paint_cels(&self) -> Vec<(&Layer, &Cel)> {
        self.all_layers()
            .into_iter()
            .filter(|l| l.kind() == Kind::Paint)
            .flat_map(|l| l.cels().iter().filter(|c| c.link.is_none()).map(move |c| (l, c)))
            .collect()
    }

    pub fn swatch(&self, key: &str) -> Option<usize> {
        let key = key.trim();
        self.palette
            .iter()
            .position(|s| !s.name.is_empty() && s.name == key)
            .or_else(|| {
                self.palette
                    .iter()
                    .position(|s| key.starts_with('#') && s.color.eq_ignore_ascii_case(key))
            })
            .or_else(|| key.parse::<usize>().ok().filter(|i| *i < self.palette.len()))
    }

    pub fn effects_of<'a>(&self, layer: &'a Layer) -> Cow<'a, [Effect]> {
        match (layer.depth, &self.atmosphere) {
            (Some(depth), Some(air)) if depth > 0.0 => Cow::Owned(
                layer
                    .effects
                    .iter()
                    .cloned()
                    .chain(air.effects(depth, self.canvas.height as f32))
                    .collect(),
            ),
            _ => Cow::Borrowed(&layer.effects),
        }
    }

    pub fn resolve_color(&self, text: &str) -> Result<String> {
        if text.trim().starts_with('#') {
            parse_hex(text)?;
            return Ok(text.trim().to_string());
        }
        match self
            .palette
            .iter()
            .find(|s| !s.name.is_empty() && s.name == text.trim())
        {
            Some(swatch) => Ok(swatch.color.clone()),
            None => {
                let names: Vec<&str> = self
                    .palette
                    .iter()
                    .map(|s| s.name.as_str())
                    .filter(|n| !n.is_empty())
                    .collect();
                if names.is_empty() {
                    bail!("colour {text:?} is not #rrggbb, #rrggbbaa or a palette name (the palette has no names)");
                }
                bail!(
                    "colour {text:?} is not #rrggbb, #rrggbbaa or a palette name: {}",
                    names.join(", ")
                )
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != FORMAT {
            bail!("format {} is not {FORMAT}", self.format);
        }
        let (w, h) = (self.canvas.width, self.canvas.height);
        if w == 0 || h == 0 || w > MAX_SIZE || h > MAX_SIZE {
            bail!("canvas {w}x{h} must be between 1 and {MAX_SIZE} on each side");
        }
        if let Some(bg) = &self.canvas.background {
            check_color(bg).context("canvas background")?;
        }
        let mut swatch_names = BTreeSet::new();
        self.palette.iter().try_for_each(|swatch| {
            parse_hex(&swatch.color).with_context(|| format!("palette colour {}", swatch.color))?;
            if swatch.name.starts_with('#') {
                bail!("palette name {} cannot start with #", swatch.name);
            }
            if !swatch.name.is_empty() && !swatch_names.insert(&swatch.name) {
                bail!("palette name {} is used twice", swatch.name);
            }
            Ok(())
        })?;
        let t = &self.timeline;
        if t.fps == 0 || t.fps > 240 || t.frames == 0 {
            bail!("timeline needs 1 to 240 fps and at least one frame");
        }
        let mut names = BTreeSet::new();
        t.clips.iter().try_for_each(|clip| {
            if clip.from > clip.to || clip.to >= t.frames {
                bail!(
                    "clip {} spans frames {} to {} outside 0 to {}",
                    clip.name,
                    clip.from,
                    clip.to,
                    t.frames - 1
                );
            }
            if !names.insert(&clip.name) {
                bail!("clip name {} is used twice", clip.name);
            }
            if clip.name.trim().is_empty() {
                bail!("a clip needs a name");
            }
            if clip.fps.is_some_and(|f| f == 0 || f > 240) {
                bail!("clip {} needs 1 to 240 fps", clip.name);
            }
            Ok(())
        })?;
        if t.origin.is_some_and(|o| !o.iter().all(|v| v.is_finite())) {
            bail!("timeline origin must be two numbers");
        }
        if self.export.godot.as_ref().is_some_and(|g| g.trim().is_empty()) {
            bail!("export.godot must be a folder");
        }
        if let Some(air) = &self.atmosphere {
            air.validate().context("atmosphere")?;
        }
        let layers = self.all_layers();
        if let Some(deep) = layers
            .iter()
            .find(|l| l.depth.is_some() && !self.layers.iter().any(|t| t.id == l.id))
        {
            bail!(
                "layer {}: depth is for layers at the top of the stack (the planes)",
                deep.id
            );
        }
        let mut seen = BTreeSet::new();
        let mut nodes = Vec::new();
        layers
            .iter()
            .for_each(|l| l.cels().iter().for_each(|c| walk_nodes(&c.nodes, &mut nodes)));
        layers
            .iter()
            .map(|l| &l.id)
            .chain(layers.iter().flat_map(|l| l.cels().iter().map(|c| &c.id)))
            .chain(nodes.iter().map(|n| &n.id))
            .try_for_each(|id| {
                if !valid_id(id) {
                    bail!("id {id:?} may only use letters, digits, - and _");
                }
                if !seen.insert(id) {
                    bail!("id {id} is used twice");
                }
                Ok(())
            })?;
        layers.iter().try_for_each(|layer| {
            if !(0.0..=1.0).contains(&layer.opacity) {
                bail!("layer {}: opacity {} is outside 0..1", layer.id, layer.opacity);
            }
            if layer.lock_alpha && layer.kind() != Kind::Paint {
                bail!("layer {}: lockAlpha is for paint layers", layer.id);
            }
            if let Some(mask) = &layer.mask {
                check_mask(layer, mask)?;
            }
            layer.effects.iter().try_for_each(|e| {
                e.validate()
                    .with_context(|| format!("layer {}: {} effect", layer.id, e.name()))
            })?;
            if let Some(adjust) = layer.adjustment() {
                adjust
                    .validate()
                    .with_context(|| format!("layer {}: {}", layer.id, adjust.label()))?;
            }
            nodes
                .iter()
                .try_for_each(|n| n.shape.validate().with_context(|| format!("node {}", n.id)))?;
            if let Some(depth) = layer.depth
                && (!depth.is_finite() || depth < 0.0)
            {
                bail!("layer {}: depth {depth} must be 0 or more", layer.id);
            }
            layer
                .cels()
                .iter()
                .filter_map(|c| c.pose.as_ref().map(|p| (c, p)))
                .try_for_each(|(c, p)| {
                    crate::rig::check_pose(p).with_context(|| format!("layer {}: cel {}", layer.id, c.id))
                })?;
            if let Some(bone) = &layer.bone {
                if layer.kind() != Kind::Paint {
                    bail!("layer {}: only paint layers follow a bone", layer.id);
                }
                let Some(Loc::Layer(path)) = self.locate(&layer.id) else {
                    bail!("layer {} is not in the drawing", layer.id);
                };
                let owner = crate::rig::skeleton_of(self, &path)
                    .ok_or_else(|| anyhow!("layer {}: follows bone {bone}, but no skeleton holds it", layer.id))?;
                if !owner.bones.iter().any(|b| &b.name == bone) {
                    bail!(
                        "layer {}: follows bone {bone}, which {} does not have",
                        layer.id,
                        owner.name
                    );
                }
            }
            if !layer.bones.is_empty() || !layer.ik.is_empty() {
                if !matches!(layer.kind(), Kind::Paint | Kind::Group) {
                    bail!("layer {}: bones go on a paint layer or a group", layer.id);
                }
                crate::rig::check(&layer.bones).with_context(|| format!("layer {}", layer.id))?;
                crate::rig::check_ik(&layer.bones, &layer.ik).with_context(|| format!("layer {}", layer.id))?;
            }
            check_cels(layer)
        })
    }
}

impl Layer {
    pub fn keyed(&self, frame: u32) -> (f32, Option<Transform>) {
        if self.keys.is_empty() {
            return (self.opacity, None);
        }
        let v = |p: &str, d: f32| crate::keys::value(&self.keys, p, frame).unwrap_or(d);
        let moved = ["x", "y", "rotate", "sx", "sy"]
            .iter()
            .any(|p| self.keys.contains_key(*p));
        (
            v("opacity", self.opacity),
            moved.then(|| Transform {
                x: v("x", 0.0),
                y: v("y", 0.0),
                rotate: v("rotate", 0.0),
                sx: v("sx", 1.0),
                sy: v("sy", 1.0),
                pivot: None,
            }),
        )
    }
}

impl Node {
    pub fn keyed(&self, frame: u32) -> std::borrow::Cow<'_, Node> {
        if self.keys.is_empty() {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut node = self.clone();
        let v = |p: &str, d: f32| crate::keys::value(&self.keys, p, frame).unwrap_or(d);
        node.transform.x = v("x", self.transform.x);
        node.transform.y = v("y", self.transform.y);
        node.transform.rotate = v("rotate", self.transform.rotate);
        node.transform.sx = v("sx", self.transform.sx);
        node.transform.sy = v("sy", self.transform.sy);
        node.opacity = v("opacity", self.opacity);
        std::borrow::Cow::Owned(node)
    }
}

pub fn nodes_at(nodes: &[Node], frame: u32) -> Vec<Node> {
    nodes
        .iter()
        .map(|n| {
            let mut node = n.keyed(frame).into_owned();
            if let Some(children) = node.shape.children_mut() {
                *children = nodes_at(children, frame);
            }
            node
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        let mut doc = Document::new(256, 128);
        doc.timeline.frames = 8;
        let mut lines = Layer::paint("l1", "lines", "c1");
        if let Some(cels) = lines.cels_mut() {
            cels[0].hold = Some(2);
            cels.push(Cel {
                hold: Some(2),
                ..Cel::at("c2", 2)
            });
            cels.push(Cel {
                link: Some("c1".into()),
                ..Cel::at("c3", 4)
            });
        }
        let shapes = Layer::vector(
            "l2",
            "shapes",
            "c4",
            vec![
                Node::new(
                    "n1",
                    Shape::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                        radius: 0.0,
                    },
                )
                .filled("#ff0000"),
                Node::new(
                    "n2",
                    Shape::Group {
                        children: vec![Node::new("n3", Shape::Path { d: "M0 0 L5 5".into() })],
                    },
                ),
            ],
        );
        doc.layers = vec![Layer::group("g1", "art", vec![lines]), shapes];
        doc.palette = vec![
            Swatch {
                color: "#1b1b1b".into(),
                name: "ink".into(),
            },
            Swatch {
                color: "#F4EFE6".into(),
                name: String::new(),
            },
        ];
        doc.next_id = 10;
        doc
    }

    #[test]
    fn json_round_trip_keeps_everything() {
        let doc = sample();
        let text = serde_json::to_string_pretty(&doc).unwrap();
        let back: Document = serde_json::from_str(&text).unwrap();
        assert_eq!(back, doc);
        assert!(text.contains("\"kind\": \"group\""));
        assert!(!text.contains("\"transform\""));
        back.validate().unwrap();
    }

    #[test]
    fn cels_resolve_holds_and_links() {
        let doc = sample();
        let lines = doc.layer("l1").unwrap();
        let at = |f| lines.cel_at(f).map(|c| c.id.as_str());
        assert_eq!(
            [at(0), at(1), at(2), at(3), at(4), at(7)],
            [Some("c1"), Some("c1"), Some("c2"), Some("c2"), Some("c3"), Some("c3")]
        );
        assert_eq!(lines.content_at(5).unwrap().id, "c1");
        let shapes = doc.layer("l2").unwrap();
        assert_eq!(shapes.cel_at(7).unwrap().id, "c4");
        assert_eq!(doc.paint_content_at(4).len(), 1);
        assert_eq!(doc.paint_cels().len(), 2);
    }

    #[test]
    fn hold_ends_a_cel() {
        let mut layer = Layer::paint("l", "l", "c");
        if let Some(cels) = layer.cels_mut() {
            cels[0].hold = Some(3);
        }
        assert!(layer.cel_at(2).is_some());
        assert!(layer.cel_at(3).is_none());
    }

    #[test]
    fn locate_finds_every_kind() {
        let doc = sample();
        assert_eq!(doc.locate("g1"), Some(Loc::Layer(vec![0])));
        assert_eq!(doc.locate("l1"), Some(Loc::Layer(vec![0, 0])));
        assert_eq!(doc.locate("c2"), Some(Loc::Cel(vec![0, 0], 1)));
        assert_eq!(doc.locate("n3"), Some(Loc::Node(vec![1], 0, vec![1, 0])));
        assert!(doc.locate("zz").is_none());
        assert!(matches!(doc.item("n3"), Some(Item::Node(_, _, n)) if n.type_name() == "path"));
    }

    #[test]
    fn ids_are_allocated_without_collisions() {
        let mut doc = sample();
        doc.next_id = 0;
        let id = doc.alloc_id('n');
        assert_eq!(id, "n4");
        assert!(!doc.ids().contains(&id));
    }

    #[test]
    fn validation_catches_mistakes() {
        let broken = |f: fn(&mut Document)| {
            let mut doc = sample();
            f(&mut doc);
            doc.validate().unwrap_err().to_string()
        };
        assert!(broken(|d| d.layers[1].id = "l1".into()).contains("used twice"));
        assert!(broken(|d| d.layers[1].id = "has space".into()).contains("may only use"));
        assert!(broken(|d| d.layer_at_mut(&[0, 0]).cels_mut().unwrap()[0].hold = Some(3)).contains("overlaps"));
        assert!(
            broken(|d| d.layer_at_mut(&[0, 0]).cels_mut().unwrap()[2].link = Some("c9".into())).contains("not a cel")
        );
        assert!(broken(|d| d.timeline.clips.push(Clip::new("x", 2, 9, true))).contains("clip"));
        assert!(
            broken(|d| d.layer_at_mut(&[1]).cels_mut().unwrap()[0].nodes[0].fill = Some(Paint::Solid("red".into())))
                .contains("node n1")
        );
        assert!(broken(|d| d.canvas.width = 0).contains("canvas"));
        assert!(broken(|d| d.palette[1].name = "ink".into()).contains("used twice"));
        assert!(broken(|d| d.palette[0].color = "black".into()).contains("palette"));
    }

    #[test]
    fn masks_are_layers_under_their_owner() {
        let mut doc = sample();
        let mut mask = Layer::paint("m1", "mask", "c9");
        mask.visible = false;
        doc.layer_at_mut(&[1]).mask = Some(Box::new(mask));
        doc.layer_at_mut(&[1]).clip = true;
        let text = serde_json::to_string_pretty(&doc).unwrap();
        assert!(text.contains("\"mask\": {") && text.contains("\"clip\": true"));
        let back: Document = serde_json::from_str(&text).unwrap();
        assert_eq!(back, doc);
        back.validate().unwrap();
        assert_eq!(doc.locate("m1"), Some(Loc::Layer(vec![1, MASK])));
        assert_eq!(doc.locate("c9"), Some(Loc::Cel(vec![1, MASK], 0)));
        assert_eq!(doc.layer_at(&[1, MASK]).id, "m1");
        assert!(doc.is_mask("m1") && !doc.is_mask("l2"));
        assert!(doc.ids().contains("c9"));
        assert!(doc.paint_cels().iter().any(|(l, c)| l.id == "m1" && c.id == "c9"));
        assert!(doc.layer("l2").unwrap().mask_at(0).is_none());
        doc.layer_at_mut(&[1, MASK]).visible = true;
        assert_eq!(
            doc.layer("l2").unwrap().mask_at(0).map(|(_, c)| c.id.as_str()),
            Some("c9")
        );
        doc.layer_at_mut(&[1, MASK]).opacity = 0.5;
        assert!(doc.validate().unwrap_err().to_string().contains("mask m1"));
        doc.layer_at_mut(&[1, MASK]).opacity = 1.0;
        doc.layer_at_mut(&[1]).lock_alpha = true;
        assert!(doc.validate().unwrap_err().to_string().contains("lockAlpha"));
    }

    #[test]
    fn palette_names_resolve_to_colours() {
        let doc = sample();
        assert_eq!(doc.resolve_color("ink").unwrap(), "#1b1b1b");
        assert_eq!(doc.resolve_color("#ff0000").unwrap(), "#ff0000");
        assert!(doc.resolve_color("skin").unwrap_err().to_string().contains("ink"));
        assert_eq!(
            [
                doc.swatch("ink"),
                doc.swatch("#f4efe6"),
                doc.swatch("1"),
                doc.swatch("7")
            ],
            [Some(0), Some(1), Some(1), None]
        );
        let text = serde_json::to_string(&doc).unwrap();
        assert!(text.contains(r##""palette":[{"color":"#1b1b1b","name":"ink"},{"color":"#F4EFE6"}]"##));
    }

    #[test]
    fn paints_parse_from_json() {
        let solid: Paint = serde_json::from_str(r##""#336699""##).unwrap();
        assert_eq!(solid, Paint::Solid("#336699".into()));
        let linear: Paint =
            serde_json::from_str(r##"{"linear":{"from":[0,0],"to":[0,100],"stops":[[0,"#000000"],[1,"#ffffff"]]}}"##)
                .unwrap();
        assert!(matches!(linear, Paint::Linear { .. }));
        check_paint(&linear).unwrap();
        let stroke: StrokeStyle = serde_json::from_str(r#"{"width":3}"#).unwrap();
        assert_eq!(stroke.paint, Paint::Solid("#000000".into()));
        assert_eq!(stroke.cap, Cap::Round);
    }
}
