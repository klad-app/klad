use crate::doc::valid_id;
use crate::rng::{FNV_OFFSET, fnv};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BRUSH_FORMAT: &str = "draw-anything-brush/1";
pub const SPEED_FULL: f32 = 5.0;

const LIBRARY: &[(&str, &str)] = &[
    ("ink", include_str!("../brushes/library/ink.json")),
    ("pencil", include_str!("../brushes/library/pencil.json")),
    ("marker", include_str!("../brushes/library/marker.json")),
    ("airbrush", include_str!("../brushes/library/airbrush.json")),
    ("soft", include_str!("../brushes/library/soft.json")),
    ("chalk", include_str!("../brushes/library/chalk.json")),
    ("eraser", include_str!("../brushes/library/eraser.json")),
    ("flat-bristle", include_str!("../brushes/library/flat-bristle.json")),
    ("round-bristle", include_str!("../brushes/library/round-bristle.json")),
    ("sponge", include_str!("../brushes/library/sponge.json")),
    ("fog", include_str!("../brushes/library/fog.json")),
    ("leaves", include_str!("../brushes/library/leaves.json")),
    ("grass", include_str!("../brushes/library/grass.json")),
    ("splatter", include_str!("../brushes/library/splatter.json")),
    ("wet", include_str!("../brushes/library/wet.json")),
    ("smudge", include_str!("../brushes/library/smudge.json")),
    ("blender", include_str!("../brushes/library/blender.json")),
    ("dry-brush", include_str!("../brushes/library/dry-brush.json")),
    ("two-tone", include_str!("../brushes/library/two-tone.json")),
    ("oil", include_str!("../brushes/library/oil.json")),
    ("oil-load", include_str!("../brushes/library/oil-load.json")),
];

const M1: &[(&str, &str)] = &[
    ("pencil", include_str!("../brushes/m1/pencil.json")),
    ("ink", include_str!("../brushes/m1/ink.json")),
    ("soft", include_str!("../brushes/m1/soft.json")),
    ("chalk", include_str!("../brushes/m1/chalk.json")),
    ("eraser", include_str!("../brushes/m1/eraser.json")),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrushDef {
    #[serde(default = "brush_format")]
    pub format: String,
    pub name: String,
    pub tip: TipDef,
    pub spacing: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub roundness: f32,
    #[serde(default, skip_serializing_if = "AngleDef::is_default")]
    pub angle: AngleDef,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub flow: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Dynamics::is_empty")]
    pub dynamics: Dynamics,
    #[serde(default, skip_serializing_if = "Jitter::is_zero")]
    pub jitter: Jitter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<GrainDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dual: Option<DualDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<PaintDef>,
    #[serde(default, skip_serializing_if = "BrushBlend::is_normal")]
    pub blend: BrushBlend,
    #[serde(default, skip_serializing_if = "is_untapered")]
    pub taper: [f32; 2],
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stabilizer: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub smoothing: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub body: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum TipDef {
    Round {
        hardness: f32,
    },
    Chalk {
        seed: u32,
    },
    Image {
        image: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        more: Vec<String>,
    },
}

pub const TIP_IMAGES: [&str; 10] = [
    "bristle-flat",
    "bristle-round",
    "sponge",
    "leaf",
    "grass",
    "splatter",
    "leaf-oval",
    "leaf-lance",
    "leaf-lobed",
    "leaf-twig",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DualDef {
    pub tip: TipDef,
    #[serde(default = "half", skip_serializing_if = "is_half")]
    pub size: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scatter: f32,
    #[serde(default = "three", skip_serializing_if = "is_three")]
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorDef {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hue: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub saturation: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub value: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stroke_hue: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stroke_saturation: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stroke_value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<Curve>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub along: f32,
}

impl ColorDef {
    pub fn uses_second(&self) -> bool {
        self.pressure.is_some() || self.along > 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum PaintDef {
    Wet { wetness: f32, pickup: f32, dilution: f32 },
    Smudge { strength: f32 },
    Blend { strength: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AngleDef {
    Fixed(f32),
    Follow,
    Tilt,
}

impl Default for AngleDef {
    fn default() -> Self {
        AngleDef::Fixed(0.0)
    }
}

impl AngleDef {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dynamics {
    #[serde(default, skip_serializing_if = "Channel::is_empty")]
    pub size: Channel,
    #[serde(default, skip_serializing_if = "Channel::is_empty")]
    pub flow: Channel,
    #[serde(default, skip_serializing_if = "Channel::is_empty")]
    pub opacity: Channel,
}

impl Dynamics {
    fn is_empty(&self) -> bool {
        self.size.is_empty() && self.flow.is_empty() && self.opacity.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<Curve>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<Curve>,
}

impl Channel {
    fn is_empty(&self) -> bool {
        self.pressure.is_none() && self.tilt.is_none() && self.speed.is_none()
    }

    pub fn at(&self, pressure: f32, tilt: f32, speed: f32) -> f32 {
        [(self.pressure, pressure), (self.tilt, tilt), (self.speed, speed)]
            .iter()
            .filter_map(|(curve, x)| curve.map(|c| c.at(*x)))
            .fold(1.0, |acc, v| acc * v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    #[serde(default)]
    pub min: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub max: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub gamma: f32,
}

impl Curve {
    pub fn at(&self, x: f32) -> f32 {
        self.min + (self.max - self.min) * libm::powf(x, self.gamma)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jitter {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub scatter: f32,
}

impl Jitter {
    fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrainDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<Paper>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub strength: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub contrast: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pressure: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paper {
    pub seed: u32,
    pub feature: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrushBlend {
    #[default]
    Normal,
    Erase,
}

impl BrushBlend {
    fn is_normal(&self) -> bool {
        *self == BrushBlend::Normal
    }
}

fn brush_format() -> String {
    BRUSH_FORMAT.to_string()
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn half() -> f32 {
    0.5
}

fn is_half(v: &f32) -> bool {
    *v == 0.5
}

fn three() -> u32 {
    3
}

fn is_three(v: &u32) -> bool {
    *v == 3
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_untapered(v: &[f32; 2]) -> bool {
    *v == [0.0, 0.0]
}

fn within(what: &str, v: f32, lo: f32, hi: f32) -> Result<()> {
    if !(lo..=hi).contains(&v) {
        bail!("{what} {v} is outside {lo}..{hi}");
    }
    Ok(())
}

pub fn valid_image(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("image name {name:?}: letters, digits, - and _ only");
    }
    Ok(())
}

impl TipDef {
    fn validate(&self, what: &str) -> Result<()> {
        match self {
            TipDef::Round { hardness } => within(&format!("{what} hardness"), *hardness, 0.0, 1.0),
            TipDef::Chalk { .. } => Ok(()),
            TipDef::Image { image, more } => [image].into_iter().chain(more).try_for_each(|i| valid_image(i)),
        }
    }
}

impl Curve {
    fn validate(&self, what: &str) -> Result<()> {
        within(&format!("{what} min"), self.min, 0.0, 4.0)?;
        within(&format!("{what} max"), self.max, 0.0, 4.0)?;
        within(&format!("{what} gamma"), self.gamma, 0.05, 10.0)
    }
}

impl Channel {
    fn validate(&self, what: &str) -> Result<()> {
        [("pressure", self.pressure), ("tilt", self.tilt), ("speed", self.speed)]
            .iter()
            .filter_map(|(input, curve)| curve.map(|c| (input, c)))
            .try_for_each(|(input, curve)| curve.validate(&format!("{what} {input}")))
    }
}

impl BrushDef {
    pub fn parse(text: &str) -> Result<Self> {
        let def: BrushDef = serde_json::from_str(text)?;
        def.validate()?;
        Ok(def)
    }

    pub fn validate(&self) -> Result<()> {
        if self.format != BRUSH_FORMAT {
            bail!("brush format {} is not {BRUSH_FORMAT}", self.format);
        }
        self.tip.validate("tip")?;
        if let Some(dual) = &self.dual {
            dual.tip.validate("dual tip")?;
            within("dual size", dual.size, 0.05, 2.0)?;
            within("dual scatter", dual.scatter, 0.0, 4.0)?;
            within("dual count", dual.count as f32, 1.0, 8.0)?;
        }
        if let Some(color) = &self.color {
            within("hue jitter", color.hue, 0.0, 180.0)?;
            within("stroke hue jitter", color.stroke_hue, 0.0, 180.0)?;
            [
                ("saturation jitter", color.saturation),
                ("value jitter", color.value),
                ("stroke saturation jitter", color.stroke_saturation),
                ("stroke value jitter", color.stroke_value),
            ]
            .iter()
            .try_for_each(|(what, v)| within(what, *v, 0.0, 1.0))?;
            if let Some(curve) = &color.pressure {
                curve.validate("colour pressure")?;
            }
            within("colour along", color.along, 0.0, 100_000.0)?;
        }
        match self.paint {
            Some(PaintDef::Wet {
                wetness,
                pickup,
                dilution,
            }) => {
                within("wetness", wetness, 0.0, 1.0)?;
                within("pickup", pickup, 0.0, 1.0)?;
                within("dilution", dilution, 0.0, 1.0)?;
            }
            Some(PaintDef::Smudge { strength } | PaintDef::Blend { strength }) => {
                within("strength", strength, 0.0, 1.0)?
            }
            None => {}
        }
        within("spacing", self.spacing, 0.01, 10.0)?;
        within("roundness", self.roundness, 0.05, 1.0)?;
        within("flow", self.flow, 0.001, 1.0)?;
        within("opacity", self.opacity, 0.001, 1.0)?;
        self.dynamics.size.validate("size")?;
        self.dynamics.flow.validate("flow")?;
        self.dynamics.opacity.validate("opacity")?;
        within("size jitter", self.jitter.size, 0.0, 1.0)?;
        within("angle jitter", self.jitter.angle, 0.0, 7.0)?;
        within("scatter", self.jitter.scatter, 0.0, 10.0)?;
        if let Some(grain) = &self.grain {
            within("grain strength", grain.strength, 0.0, 1.0)?;
            match (&grain.paper, &grain.image) {
                (Some(paper), None) => within("grain feature", paper.feature as f32, 1.0, 512.0)?,
                (None, Some(image)) => valid_image(image)?,
                _ => bail!("a grain is either paper or an image"),
            }
            within("grain scale", grain.scale, 0.05, 20.0)?;
            within("grain contrast", grain.contrast, 0.0, 8.0)?;
            within("grain pressure", grain.pressure, 0.0, 1.0)?;
        }
        within("taper in", self.taper[0], 0.0, 10_000.0)?;
        within("taper out", self.taper[1], 0.0, 10_000.0)?;
        within("stabilizer", self.stabilizer, 0.0, 500.0)?;
        within("smoothing", self.smoothing, 0.0, 500.0)?;
        within("body", self.body, 0.0, 1.0)
    }

    pub fn to_json(&self) -> Result<String> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }

    pub fn hardness(&self) -> Option<f32> {
        match self.tip {
            TipDef::Round { hardness } => Some(hardness),
            _ => None,
        }
    }

    pub fn images(&self) -> Vec<&str> {
        fn tip(t: &TipDef) -> Vec<&str> {
            match t {
                TipDef::Image { image, more } => [image].into_iter().chain(more).map(String::as_str).collect(),
                _ => Vec::new(),
            }
        }
        tip(&self.tip)
            .into_iter()
            .chain(self.dual.as_ref().map(|d| tip(&d.tip)).unwrap_or_default())
            .chain(self.grain.as_ref().and_then(|g| g.image.as_deref()))
            .filter(|name| !TIP_IMAGES.contains(name))
            .collect()
    }

    pub fn reads_pixels(&self) -> bool {
        self.paint.is_some()
    }

    pub fn fingerprint(&self) -> u64 {
        fnv(FNV_OFFSET, &serde_json::to_vec(self).unwrap_or_default())
    }
}

fn parse_set(set: &[(&str, &str)], id: &str) -> Option<BrushDef> {
    set.iter()
        .find(|(name, _)| *name == id)
        .and_then(|(_, text)| BrushDef::parse(text).ok())
}

pub fn library_ids() -> Vec<&'static str> {
    LIBRARY.iter().map(|(id, _)| *id).collect()
}

pub fn library(id: &str) -> Option<BrushDef> {
    parse_set(LIBRARY, id)
}

pub fn legacy_m1(id: &str) -> Option<BrushDef> {
    parse_set(M1, id)
}

fn base_of(id: &str) -> &str {
    match id.rsplit_once('-') {
        Some((base, tail)) if tail.len() == 6 && tail.chars().all(|c| c.is_ascii_hexdigit()) => base,
        _ => id,
    }
}

pub fn pick_id(base: &str, def: &BrushDef, embedded: &BTreeMap<String, BrushDef>) -> String {
    let base = base_of(base);
    let fits = |id: &str| match embedded.get(id) {
        Some(known) => known == def,
        None => library(id).is_none_or(|known| &known == def),
    };
    if valid_id(base) && fits(base) {
        return base.to_string();
    }
    let print = def.fingerprint();
    let short = format!("{base}-{:06x}", print & 0xff_ffff);
    if fits(&short) {
        return short;
    }
    format!("{base}-{print:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_brush_parses_and_round_trips() {
        LIBRARY.iter().chain(M1).for_each(|(id, text)| {
            let def = BrushDef::parse(text).unwrap_or_else(|e| panic!("{id}: {e:#}"));
            let back = BrushDef::parse(&def.to_json().unwrap()).unwrap();
            assert_eq!(back, def, "{id}");
        });
        assert_eq!(library_ids().len(), 21);
        assert!(library("ink").unwrap().taper[1] > 0.0);
        assert_eq!(legacy_m1("ink").unwrap().taper, [0.0, 0.0]);
        assert!(legacy_m1("marker").is_none());
    }

    #[test]
    fn curves_follow_min_max_and_gamma() {
        let curve = Curve {
            min: 0.2,
            max: 1.0,
            gamma: 2.0,
        };
        assert_eq!(curve.at(0.0), 0.2);
        assert_eq!(curve.at(1.0), 1.0);
        assert!((curve.at(0.5) - 0.4).abs() < 1e-6);
        let channel = Channel {
            pressure: Some(curve),
            tilt: None,
            speed: Some(Curve {
                min: 1.0,
                max: 0.5,
                gamma: 1.0,
            }),
        };
        assert!((channel.at(1.0, 0.3, 1.0) - 0.5).abs() < 1e-6);
        assert_eq!(Channel::default().at(0.1, 0.2, 0.3), 1.0);
    }

    #[test]
    fn bad_brushes_are_rejected() {
        let ink = library("ink").unwrap();
        let broken = |f: fn(&mut BrushDef)| {
            let mut def = ink.clone();
            f(&mut def);
            def.validate().unwrap_err().to_string()
        };
        assert!(broken(|d| d.spacing = 0.0).contains("spacing"));
        assert!(broken(|d| d.tip = TipDef::Round { hardness: 2.0 }).contains("hardness"));
        assert!(
            broken(|d| d.dynamics.size.pressure = Some(Curve {
                min: 0.0,
                max: 1.0,
                gamma: 0.0
            }))
            .contains("gamma")
        );
        assert!(BrushDef::parse(r#"{"name":"x","tip":{"round":{"hardness":1}},"spacing":0.1,"sise":3}"#).is_err());
    }

    #[test]
    fn ids_stay_put_until_a_brush_differs() {
        let ink = library("ink").unwrap();
        let none = BTreeMap::new();
        assert_eq!(pick_id("ink", &ink, &none), "ink");
        let mut harder = ink.clone();
        harder.tip = TipDef::Round { hardness: 1.0 };
        let variant = pick_id("ink", &harder, &none);
        assert!(variant.starts_with("ink-") && variant.len() == 10, "{variant}");
        assert_eq!(pick_id(&variant, &harder, &none), variant);
        let old = BTreeMap::from([("ink".to_string(), legacy_m1("ink").unwrap())]);
        assert_ne!(pick_id("ink", &ink, &old), "ink");
        assert_eq!(pick_id("ink", &legacy_m1("ink").unwrap(), &old), "ink");
        let mut custom = ink.clone();
        custom.name = "Mine".into();
        assert!(pick_id("mine", &custom, &none) == "mine");
    }
}
