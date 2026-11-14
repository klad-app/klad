use crate::agent::{self, Output, parse_props};
use crate::layers;
use anyhow::{Result, anyhow, bail};
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::maps::{Light, LightKind, Maps};
use klad_core::ops::Op;
use klad_render::maps::{Lit, Map};
use klad_render::{Frame, View};
use std::path::{Path, PathBuf};
use std::time::Instant;

const SURFACE_KEYS: &[&str] = &["bevel", "profile", "strength", "body", "range"];
const TOP_KEYS: &[&str] = &["ambient", "shininess"];

pub fn maps(path: &Path, props: &[String], on: bool, off: bool, out: &Output) -> Result<()> {
    let bundle = Bundle::open(path)?;
    if props.is_empty() && !on && !off {
        match &bundle.doc.maps {
            Some(maps) => println!("{}", serde_json::to_string(maps)?),
            None => println!(
                "maps are off: klad maps <bundle> --on, or set the surface: klad maps <bundle> bevel=8 profile=round strength=1"
            ),
        }
        return Ok(());
    }
    if on && off {
        bail!("--on or --off, not both");
    }
    let op = match off {
        true => Op::Maps(None),
        false => Op::Maps(Some(changed(bundle.doc.maps.clone().unwrap_or_default(), props)?)),
    };
    agent::edit(path, &[op], out)
}

fn changed(maps: Maps, props: &[String]) -> Result<Maps> {
    let mut value = serde_json::to_value(maps)?;
    parse_props(props)?.into_iter().try_for_each(|(key, v)| {
        let place = match key.as_str() {
            k if SURFACE_KEYS.contains(&k) => &mut value["surface"],
            k if TOP_KEYS.contains(&k) => &mut value,
            other => bail!(
                "maps has no {other:?}: the drawing's surface takes {} and the light preview {}",
                SURFACE_KEYS.join(", "),
                TOP_KEYS.join(", ")
            ),
        };
        place[key.as_str()] = v;
        Ok(())
    })?;
    let maps: Maps = serde_json::from_value(value)?;
    maps.validate()?;
    Ok(maps)
}

pub struct LightArgs {
    pub add: Option<String>,
    pub change: Option<usize>,
    pub rm: Option<usize>,
    pub clear: bool,
    pub at: Option<[f32; 2]>,
    pub height: Option<f32>,
    pub color: Option<String>,
    pub energy: Option<f32>,
    pub range: Option<f32>,
}

impl LightArgs {
    fn shaped(&self, mut light: Light) -> Light {
        if let Some(at) = self.at {
            light.at = at;
        }
        if let Some(height) = self.height {
            light.height = height;
        }
        if let Some(color) = &self.color {
            light.color = color.clone();
        }
        if let Some(energy) = self.energy {
            light.energy = energy;
        }
        if let Some(range) = self.range {
            light.range = range;
        }
        light
    }

    fn fields(&self) -> bool {
        self.at.is_some()
            || self.height.is_some()
            || self.color.is_some()
            || self.energy.is_some()
            || self.range.is_some()
    }
}

fn describe(i: usize, light: &Light) -> String {
    match light.kind {
        LightKind::Point => format!(
            "{i}  point at {},{} height {} px, {} x{} reaching {} px",
            light.at[0], light.at[1], light.height, light.color, light.energy, light.range
        ),
        LightKind::Directional => format!(
            "{i}  directional from {},{} height {}, {} x{}",
            light.at[0], light.at[1], light.height, light.color, light.energy
        ),
    }
}

pub fn lights(path: &Path, args: &LightArgs, out: &Output) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let mut maps = bundle.doc.maps.clone().unwrap_or_default();
    let chosen = [args.add.is_some(), args.change.is_some(), args.rm.is_some(), args.clear]
        .iter()
        .filter(|on| **on)
        .count();
    if chosen > 1 {
        bail!("one of --add, --set, --rm or --clear at a time");
    }
    if chosen == 0 {
        if args.fields() {
            bail!("say what to do with the light: --add point|directional, or --set <n> to change light n");
        }
        match maps.lights.is_empty() {
            true => println!("no lights: klad lights <bundle> --add point --at x,y [--height 100] [--color #ffffff]"),
            false => maps
                .lights
                .iter()
                .enumerate()
                .for_each(|(i, l)| println!("{}", describe(i + 1, l))),
        }
        println!("ambient {}", maps.ambient);
        return Ok(());
    }
    let pick = |n: usize, count: usize| -> Result<usize> {
        match n {
            1.. if n <= count => Ok(n - 1),
            _ => Err(anyhow!("there is no light {n}: the drawing has {count}")),
        }
    };
    if let Some(kind) = &args.add {
        let light = match kind.as_str() {
            "point" => args.shaped(Light::default()),
            "directional" => args.shaped(Light::directional([-1.0, -1.0], 0.5)),
            other => bail!("--add takes point or directional, not {other:?}"),
        };
        maps.lights.push(light);
    }
    if let Some(n) = args.change {
        let at = pick(n, maps.lights.len())?;
        if !args.fields() {
            bail!("--set {n} needs what to change: --at, --height, --color, --energy or --range");
        }
        maps.lights[at] = args.shaped(maps.lights[at].clone());
    }
    if let Some(n) = args.rm {
        let at = pick(n, maps.lights.len())?;
        maps.lights.remove(at);
    }
    if args.clear {
        maps.lights.clear();
    }
    maps.validate()?;
    agent::edit(path, &[Op::Maps(Some(maps))], out)
}

fn crop(frame: &Frame, [x, y, w, h]: [i32; 4], scale: f32) -> Result<Frame> {
    let s = |v: i32| (v as f32 * scale).round() as i64;
    let (x, y, w, h) = (s(x), s(y), s(w).max(1), s(h).max(1));
    if x < 0 || y < 0 || x + w > frame.width as i64 || y + h > frame.height as i64 {
        bail!("the region is outside the canvas");
    }
    let row = frame.width as usize * 4;
    let rgba = (y as usize..(y + h) as usize)
        .flat_map(|r| {
            frame.rgba[r * row + x as usize * 4..r * row + (x + w) as usize * 4]
                .iter()
                .copied()
        })
        .collect();
    Ok(Frame {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

pub fn render(
    path: &Path,
    out: Option<PathBuf>,
    frame: u32,
    (scale, region): (Option<f32>, Option<[i32; 4]>),
    map: Option<&str>,
    lit: bool,
) -> Result<()> {
    let start = Instant::now();
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let longest = region.map_or(doc.canvas.width.max(doc.canvas.height) as f32, |r| {
        r[2].max(r[3]) as f32
    });
    let scale = scale.or_else(|| agent::pixel_scale(&bundle, longest)).unwrap_or(1.0);
    let rasters = layers::load_at(
        &bundle,
        frame,
        &BrushSet::for_bundle(&bundle)?,
        klad_render::export::detail_of(doc, scale),
    )?;
    let (picture, tag) = match (map, lit) {
        (Some(_), true) => bail!("--map shows one map, --lit the picture lit by the drawing's lights: one at a time"),
        (Some(name), false) => {
            let map = Map::parse(name)?;
            (
                klad_render::maps::render(doc, frame, &rasters, scale)?.frame(map),
                format!("-{}", map.name()),
            )
        }
        (None, true) => {
            let lights = klad_render::maps::settings(doc).lights;
            if lights.is_empty() {
                bail!("no lights to light it with: klad lights <bundle> --add point --at x,y");
            }
            (
                Lit::prepare(doc, frame, &rasters, scale)?.shade(&lights)?,
                "-lit".to_string(),
            )
        }
        (None, false) => {
            bail!("render a map with --map normal|height|specular|emission, or the lit picture with --lit")
        }
    };
    let mut picture = match region {
        Some(r) => crop(&picture, r, scale)?,
        None => picture,
    };
    let view = View {
        scale,
        region,
        references: false,
    };
    let path = out.unwrap_or_else(|| agent::render_path(&bundle, frame, view, &tag));
    write_atomic(&path, &picture.png()?)?;
    println!("{}", path.display());
    eprintln!(
        "{}x{}  hash {:016x}  {:.0} ms",
        picture.width,
        picture.height,
        picture.hash(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub fn hash(path: &Path, frames: &[u32], map: &str) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let map = Map::parse(map)?;
    let brushes = BrushSet::for_bundle(&bundle)?;
    frames.iter().try_for_each(|frame| {
        let rasters = layers::load_at(&bundle, *frame, &brushes, 1.0)?;
        let picture = klad_render::maps::render(&bundle.doc, *frame, &rasters, 1.0)?.frame(map);
        println!("frame {frame:<4} {} {:016x}", map.name(), picture.hash());
        Ok(())
    })
}

pub fn parse_at(text: &str) -> Result<[f32; 2], String> {
    let parts: Vec<f32> = text
        .split(',')
        .map(|p| p.trim().parse::<f32>().map_err(|_| format!("{text:?} is not x,y")))
        .collect::<Result<_, _>>()?;
    match parts[..] {
        [x, y] => Ok([x, y]),
        _ => Err(format!("{text:?} is not x,y")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_props_go_to_the_surface_or_the_preview() {
        let maps = changed(
            Maps::default(),
            &["bevel=12".into(), "profile=chamfer".into(), "ambient=#101020".into()],
        )
        .unwrap();
        assert_eq!(maps.surface.bevel, 12.0);
        assert_eq!(maps.surface.profile, klad_core::maps::Profile::Chamfer);
        assert_eq!(maps.ambient, "#101020");
        assert!(
            changed(Maps::default(), &["glow=1".into()])
                .unwrap_err()
                .to_string()
                .contains("bevel")
        );
        assert!(changed(Maps::default(), &["bevel=-2".into()]).is_err());
    }

    #[test]
    fn a_region_crops_at_the_render_scale() {
        let frame = Frame {
            width: 4,
            height: 4,
            rgba: (0..64).collect(),
        };
        let part = crop(&frame, [1, 1, 1, 1], 2.0).unwrap();
        assert_eq!((part.width, part.height), (2, 2));
        assert_eq!(part.pixel(0, 0), frame.pixel(2, 2));
        assert!(crop(&frame, [3, 3, 2, 2], 1.0).is_err());
        assert_eq!(parse_at("3, 4.5").unwrap(), [3.0, 4.5]);
        assert!(parse_at("3").is_err());
    }
}
