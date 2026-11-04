use crate::grain::Grain;
use crate::images::{self, Pictures};
use anyhow::{Result, anyhow, bail};
use klad_core::brushes::{self, AngleDef, BrushBlend, BrushDef, SPEED_FULL, TIP_IMAGES, TipDef};
use klad_core::bundle::Bundle;
use klad_core::rng::{FNV_OFFSET, fnv, hash2_unit};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Debug)]
pub enum Tip {
    Round { hardness: f32 },
    Mask(Arc<TipMask>),
}

#[derive(Clone, Copy, Debug)]
pub enum Angle {
    Fixed(f32),
    Follow,
    Tilt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeBlend {
    Normal,
    Erase,
    Atop,
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub grain: Arc<Grain>,
    pub strength: f32,
    pub scale: f32,
    pub contrast: f32,
    pub pressure: f32,
}

#[derive(Clone, Debug)]
pub struct Dual {
    pub tip: Tip,
    pub size: f32,
    pub scatter: f32,
    pub count: u32,
}

#[derive(Clone, Debug)]
pub struct Brush {
    pub def: BrushDef,
    pub tip: Tip,
    pub angle: Angle,
    pub grain: Option<Texture>,
    pub dual: Option<Dual>,
    pub blend: StrokeBlend,
}

#[derive(Clone, Debug)]
pub struct TipMask {
    pub size: usize,
    pub data: Vec<f32>,
}

impl TipMask {
    pub fn chalk(seed: u32) -> Self {
        let size = 128;
        let data = (0..size * size)
            .map(|i| {
                let (x, y) = (i % size, i / size);
                let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let d = libm::sqrtf(u * u + v * v);
                let body = 1.0 - smoothstep(0.55, 1.0, d);
                let clump = (hash2_unit(x as i32 / 6, y as i32 / 6, seed) * 0.6
                    + hash2_unit(x as i32 / 2, y as i32 / 2, seed + 1) * 0.4)
                    .clamp(0.0, 1.0);
                body * smoothstep(0.3, 0.6, clump)
            })
            .collect();
        Self { size, data }
    }

    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let n = self.size as f32;
        let fx = ((u * 0.5 + 0.5) * n - 0.5).clamp(0.0, n - 1.001);
        let fy = ((v * 0.5 + 0.5) * n - 0.5).clamp(0.0, n - 1.001);
        let (x, y) = (fx as usize, fy as usize);
        let (ax, ay) = (fx - x as f32, fy - y as f32);
        let at = |x: usize, y: usize| self.data[y * self.size + x];
        let top = at(x, y) + (at(x + 1, y) - at(x, y)) * ax;
        let bottom = at(x, y + 1) + (at(x + 1, y + 1) - at(x, y + 1)) * ax;
        top + (bottom - top) * ay
    }
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn memo<K: Ord + Copy, V>(cache: &Memo<K, V>, key: K, make: impl FnOnce() -> V) -> Arc<V> {
    let cache = cache.get_or_init(Mutex::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return hit;
    }
    let made = Arc::new(make());
    match cache.lock() {
        Ok(mut c) => c.entry(key).or_insert(made).clone(),
        Err(_) => made,
    }
}

type Memo<K, V> = OnceLock<Mutex<BTreeMap<K, Arc<V>>>>;

static GRAINS: Memo<(u32, usize), Grain> = OnceLock::new();
static CHALK: Memo<u32, TipMask> = OnceLock::new();
static LIBRARY_TIPS: Memo<usize, TipMask> = OnceLock::new();
static PICTURE_TIPS: Memo<u64, TipMask> = OnceLock::new();
static PICTURE_GRAINS: Memo<u64, Grain> = OnceLock::new();
static LIBRARY_GRAINS: Memo<usize, Grain> = OnceLock::new();

fn library_index(name: &str) -> Option<usize> {
    TIP_IMAGES.iter().position(|n| *n == name)
}

fn missing(name: &str) -> anyhow::Error {
    anyhow!(
        "the image {name:?} is not in the bundle's brushes/images folder (nor a library picture: {})",
        TIP_IMAGES.join(", ")
    )
}

fn tip_of(def: &TipDef, pictures: &Pictures) -> Result<Tip> {
    Ok(match def {
        TipDef::Round { hardness } => Tip::Round { hardness: *hardness },
        TipDef::Chalk { seed } => Tip::Mask(memo(&CHALK, *seed, || TipMask::chalk(*seed))),
        TipDef::Image { image } => match (library_index(image), pictures.get(image)) {
            (_, Some(picture)) => Tip::Mask(memo(&PICTURE_TIPS, picture.hash, || images::tip_from(picture))),
            (Some(i), None) => Tip::Mask(memo(&LIBRARY_TIPS, i, || {
                images::library_tip(TIP_IMAGES[i]).expect("a library tip")
            })),
            (None, None) => return Err(missing(image)),
        },
    })
}

pub fn tilt_amount(tilt: (f32, f32)) -> f32 {
    libm::sqrtf(tilt.0 * tilt.0 + tilt.1 * tilt.1).min(1.0)
}

pub fn speed_amount(px_per_ms: f32) -> f32 {
    (px_per_ms / SPEED_FULL).clamp(0.0, 1.0)
}

impl Brush {
    pub fn from_def(def: &BrushDef) -> Self {
        Self::with_pictures(def, &Pictures::new()).unwrap_or_else(|_| {
            let mut plain = def.clone();
            plain.tip = TipDef::Round { hardness: 0.5 };
            plain.dual = None;
            plain.grain = None;
            Self::with_pictures(&plain, &Pictures::new()).expect("a plain brush builds")
        })
    }

    pub fn with_pictures(def: &BrushDef, pictures: &Pictures) -> Result<Self> {
        let tip = tip_of(&def.tip, pictures)?;
        let grain = def
            .grain
            .as_ref()
            .map(|g| {
                let grain = match (&g.paper, &g.image) {
                    (Some(paper), _) => {
                        let key = (paper.seed, paper.feature);
                        memo(&GRAINS, key, || Grain::paper(key.0, key.1))
                    }
                    (None, Some(image)) => match (pictures.get(image), library_index(image)) {
                        (Some(picture), _) => memo(&PICTURE_GRAINS, picture.hash, || Grain::from_picture(picture)),
                        (None, Some(i)) => memo(&LIBRARY_GRAINS, i, || {
                            let tip = images::library_tip(TIP_IMAGES[i]).expect("a library picture");
                            Grain::from_picture(&images::Picture {
                                width: tip.size,
                                height: tip.size,
                                data: tip.data,
                                hash: 0,
                            })
                        }),
                        (None, None) => return Err(missing(image)),
                    },
                    (None, None) => bail!("a grain is either paper or an image"),
                };
                Ok(Texture {
                    grain,
                    strength: g.strength,
                    scale: g.scale,
                    contrast: g.contrast,
                    pressure: g.pressure,
                })
            })
            .transpose()?;
        let dual = def
            .dual
            .as_ref()
            .map(|d| {
                Ok::<_, anyhow::Error>(Dual {
                    tip: tip_of(&d.tip, pictures)?,
                    size: d.size,
                    scatter: d.scatter,
                    count: d.count,
                })
            })
            .transpose()?;
        Ok(Self {
            def: def.clone(),
            tip,
            dual,
            angle: match def.angle {
                AngleDef::Fixed(a) => Angle::Fixed(a),
                AngleDef::Follow => Angle::Follow,
                AngleDef::Tilt => Angle::Tilt,
            },
            grain,
            blend: match def.blend {
                BrushBlend::Normal => StrokeBlend::Normal,
                BrushBlend::Erase => StrokeBlend::Erase,
            },
        })
    }

    pub fn size_at(&self, base: f32, pressure: f32, tilt: f32, speed: f32) -> f32 {
        base * self.def.dynamics.size.at(pressure, tilt, speed)
    }

    pub fn flow_at(&self, base: f32, pressure: f32, tilt: f32, speed: f32) -> f32 {
        base * self.def.flow * self.def.dynamics.flow.at(pressure, tilt, speed)
    }

    pub fn opacity_at(&self, pressure: f32, tilt: f32, speed: f32) -> f32 {
        self.def.opacity * self.def.dynamics.opacity.at(pressure, tilt, speed)
    }
}

#[derive(Clone)]
struct Entry {
    def: BrushDef,
    built: OnceLock<Result<Brush, String>>,
}

impl Entry {
    fn new(def: BrushDef) -> Self {
        Self {
            def,
            built: OnceLock::new(),
        }
    }
}

#[derive(Clone)]
pub struct BrushSet {
    entries: BTreeMap<String, Entry>,
    pictures: Arc<Pictures>,
}

impl BrushSet {
    pub fn presets() -> Self {
        Self {
            entries: brushes::library_ids()
                .into_iter()
                .filter_map(|id| brushes::library(id).map(|def| (id.to_string(), Entry::new(def))))
                .collect(),
            pictures: Arc::new(Pictures::new()),
        }
    }

    pub fn for_bundle(bundle: &Bundle) -> Result<Self> {
        let mut set = Self::presets();
        set.pictures = Arc::new(images::load(bundle)?);
        bundle.brushes()?.into_iter().for_each(|(id, def)| set.insert(&id, def));
        Ok(set)
    }

    pub fn pictures(&self) -> &Pictures {
        &self.pictures
    }

    pub fn add_picture(&mut self, name: &str, picture: images::Picture) {
        let mut pictures = (*self.pictures).clone();
        pictures.insert(name.to_string(), Arc::new(picture));
        self.pictures = Arc::new(pictures);
        self.entries.values_mut().for_each(|e| *e = Entry::new(e.def.clone()));
    }

    pub fn refresh(&mut self, bundle: &Bundle) -> Result<()> {
        let pictures = images::load(bundle)?;
        if pictures != *self.pictures {
            self.pictures = Arc::new(pictures);
            self.entries.values_mut().for_each(|e| *e = Entry::new(e.def.clone()));
        }
        bundle.brushes()?.into_iter().for_each(|(id, def)| {
            if self.def(&id) != Some(&def) {
                self.insert(&id, def);
            }
        });
        Ok(())
    }

    pub fn insert(&mut self, id: &str, def: BrushDef) {
        self.entries.insert(id.to_string(), Entry::new(def));
    }

    pub fn def(&self, id: &str) -> Option<&BrushDef> {
        self.entries.get(id).map(|e| &e.def)
    }

    pub fn defs(&self) -> impl Iterator<Item = (&str, &BrushDef)> {
        self.entries.iter().map(|(id, e)| (id.as_str(), &e.def))
    }

    pub fn fingerprint(&self) -> u64 {
        let brushes = self.entries.iter().fold(FNV_OFFSET, |h, (id, e)| {
            fnv(fnv(h, id.as_bytes()), &e.def.fingerprint().to_le_bytes())
        });
        self.pictures.iter().fold(brushes, |h, (name, p)| {
            fnv(fnv(h, name.as_bytes()), &p.hash.to_le_bytes())
        })
    }

    pub fn get(&self, id: &str) -> Result<&Brush> {
        let entry = self.entries.get(id).ok_or_else(|| {
            let known: Vec<&str> = self.entries.keys().map(String::as_str).collect();
            anyhow!("unknown brush {id:?}: brushes are {}", known.join(", "))
        })?;
        entry
            .built
            .get_or_init(|| Brush::with_pictures(&entry.def, &self.pictures).map_err(|e| format!("brush {id}: {e:#}")))
            .as_ref()
            .map_err(|e| anyhow!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_builds() {
        let set = BrushSet::presets();
        assert!(brushes::library_ids().iter().all(|id| set.get(id).is_ok()));
        assert!(set.get("nope").unwrap_err().to_string().contains("marker"));
        assert_eq!(set.get("eraser").unwrap().blend, StrokeBlend::Erase);
    }

    #[test]
    fn ink_tapers_with_pressure() {
        let set = BrushSet::presets();
        let ink = set.get("ink").unwrap();
        assert!(ink.size_at(20.0, 0.1, 0.0, 0.0) < ink.size_at(20.0, 0.5, 0.0, 0.0));
        assert_eq!(ink.size_at(20.0, 1.0, 0.0, 0.0), 20.0);
    }

    #[test]
    fn textures_are_shared_between_brushes() {
        let a = BrushSet::presets();
        let b = BrushSet::presets();
        let (Some(ga), Some(gb)) = (&a.get("pencil").unwrap().grain, &b.get("pencil").unwrap().grain) else {
            panic!("pencil has grain");
        };
        assert!(Arc::ptr_eq(&ga.grain, &gb.grain));
    }

    #[test]
    fn chalk_mask_has_holes_and_body() {
        let mask = TipMask::chalk(7);
        assert!(mask.sample(0.0, 0.0) >= 0.0);
        assert_eq!(mask.sample(1.0, 1.0), 0.0);
        let filled = mask.data.iter().filter(|&&v| v > 0.5).count();
        assert!(filled > mask.data.len() / 10 && filled < mask.data.len() / 2);
    }

    #[test]
    fn tilt_and_speed_feed_the_curves() {
        let mut def = brushes::library("ink").unwrap();
        def.dynamics.size.tilt = Some(brushes::Curve {
            min: 1.0,
            max: 2.0,
            gamma: 1.0,
        });
        def.dynamics.size.speed = Some(brushes::Curve {
            min: 1.0,
            max: 0.5,
            gamma: 1.0,
        });
        let brush = Brush::from_def(&def);
        let upright = brush.size_at(10.0, 1.0, tilt_amount((0.0, 0.0)), speed_amount(0.0));
        let tilted = brush.size_at(10.0, 1.0, tilt_amount((0.6, 0.8)), speed_amount(0.0));
        let fast = brush.size_at(10.0, 1.0, 0.0, speed_amount(50.0));
        assert_eq!(upright, 10.0);
        assert!((tilted - 20.0).abs() < 1e-4);
        assert!((fast - 5.0).abs() < 1e-4);
    }
}
