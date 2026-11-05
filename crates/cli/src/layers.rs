use anyhow::{Context, Result};
use klad_brush::cache::{CelCache, How, Write};
use klad_brush::{BrushSet, CelReplay, Raster};
use klad_core::bundle::Bundle;
use klad_core::doc::{Document, Kind, Node};
use klad_core::rng::{FNV_OFFSET, fnv};
use klad_core::stroke::decode_cel;
use klad_render::{Rasters, rasterize_nodes, rasterize_nodes_at};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fs;

pub struct Loaded {
    pub rasters: Rasters,
    pub replayed: usize,
    pub updated: usize,
    pub cached: usize,
    pub strokes: usize,
    pub dabs: usize,
}

struct CelLoad {
    cel: String,
    raster: Raster,
    cached: bool,
    updated: bool,
    strokes: usize,
    dabs: usize,
}

pub fn load(bundle: &Bundle, frame: u32, brushes: &BrushSet, use_cache: bool) -> Result<Loaded> {
    let cels: Vec<(String, String)> = bundle
        .doc
        .paint_content_at(frame)
        .iter()
        .map(|(layer, cel)| (layer.id.clone(), cel.id.clone()))
        .collect();
    let loads = cels
        .par_iter()
        .map(|(layer, cel)| load_cel(bundle, brushes, layer, cel, use_cache))
        .collect::<Result<Vec<_>>>()?;
    Ok(Loaded {
        replayed: loads.iter().filter(|l| !l.cached && !l.updated).count(),
        updated: loads.iter().filter(|l| l.updated).count(),
        cached: loads.iter().filter(|l| l.cached).count(),
        strokes: loads.iter().map(|l| l.strokes).sum(),
        dabs: loads.iter().map(|l| l.dabs).sum(),
        rasters: loads.into_iter().map(|l| (l.cel, l.raster)).collect(),
    })
}

pub fn prepare(bundle: &Bundle, brushes: &BrushSet, from: u32, to: u32) -> Result<Loaded> {
    let cels: std::collections::BTreeSet<(String, String)> = (from..=to)
        .flat_map(|frame| {
            bundle
                .doc
                .paint_content_at(frame)
                .into_iter()
                .map(|(layer, cel)| (layer.id.clone(), cel.id.clone()))
        })
        .collect();
    let loads = cels
        .par_iter()
        .map(|(layer, cel)| load_cel(bundle, brushes, layer, cel, true).map(|l| (l.cached, l.updated)))
        .collect::<Result<Vec<_>>>()?;
    Ok(Loaded {
        replayed: loads.iter().filter(|(c, u)| !c && !u).count(),
        updated: loads.iter().filter(|(_, u)| *u).count(),
        cached: loads.iter().filter(|(c, _)| *c).count(),
        strokes: 0,
        dabs: 0,
        rasters: Rasters::new(),
    })
}

pub fn load_one(bundle: &Bundle, brushes: &BrushSet, layer: &str, cel: &str) -> Result<Raster> {
    Ok(load_cel(bundle, brushes, layer, cel, true)?.raster)
}

pub fn load_cels(bundle: &Bundle, brushes: &BrushSet, cels: &[(String, String, Kind)]) -> Result<Rasters> {
    load_cels_at(bundle, brushes, cels, 1.0)
}

pub fn load_cels_at(
    bundle: &Bundle,
    brushes: &BrushSet,
    cels: &[(String, String, Kind)],
    detail: f32,
) -> Result<Rasters> {
    let (w, h) = (bundle.doc.canvas.width, bundle.doc.canvas.height);
    let scaled = (detail != 1.0).then(|| brushes.at_scale(detail));
    cels.par_iter()
        .map(|(layer, cel, kind)| {
            let raster = match (kind, &scaled) {
                (Kind::Vector, None) => rasterize_nodes(&nodes_of(&bundle.doc, cel), w, h)?,
                (Kind::Vector, Some(_)) => rasterize_nodes_at(&nodes_of(&bundle.doc, cel), w, h, detail)?,
                (_, None) => load_cel(bundle, brushes, layer, cel, true)?.raster,
                (_, Some(scaled)) => replay_at(bundle, scaled, layer, cel, detail)?,
            };
            Ok((cel.clone(), raster))
        })
        .collect()
}

pub fn load_at(bundle: &Bundle, frame: u32, brushes: &BrushSet, detail: f32) -> Result<Rasters> {
    if detail == 1.0 {
        return Ok(load(bundle, frame, brushes, true)?.rasters);
    }
    let cels: Vec<(String, String, Kind)> = bundle
        .doc
        .paint_content_at(frame)
        .iter()
        .map(|(layer, cel)| (layer.id.clone(), cel.id.clone(), Kind::Paint))
        .collect();
    load_cels_at(bundle, brushes, &cels, detail)
}

fn replay_at(bundle: &Bundle, brushes: &BrushSet, layer: &str, cel: &str, detail: f32) -> Result<Raster> {
    let path = bundle.strokes_path(layer, cel);
    let bytes = if path.exists() { fs::read(&path)? } else { Vec::new() };
    let mut marks = match bytes.is_empty() {
        true => Vec::new(),
        false => decode_cel(&bytes).with_context(|| format!("cel {cel}"))?,
    };
    marks
        .iter_mut()
        .for_each(|m| klad_core::markedit::transform_mark(m, [detail, 0.0, 0.0, detail, 0.0, 0.0]));
    let (w, h) = (
        (bundle.doc.canvas.width as f32 * detail).ceil() as u32,
        (bundle.doc.canvas.height as f32 * detail).ceil() as u32,
    );
    Ok(klad_brush::replay(w, h, &marks, brushes)
        .with_context(|| format!("cel {cel} at {detail}x"))?
        .raster)
}

fn nodes_of(doc: &Document, cel: &str) -> Vec<Node> {
    match doc.item(cel) {
        Some(klad_core::doc::Item::Cel(_, c)) => c.nodes.clone(),
        _ => Vec::new(),
    }
}

#[derive(Default)]
pub struct Vectors {
    known: BTreeMap<String, (u64, Raster)>,
}

impl Vectors {
    pub fn add_to(&mut self, doc: &Document, frame: u32, rasters: &mut Rasters) -> Result<()> {
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let cels: Vec<(String, u64, Vec<Node>)> = doc
            .all_layers()
            .into_iter()
            .filter(|l| l.kind() == Kind::Vector)
            .filter_map(|l| l.content_at(frame))
            .map(|c| {
                let nodes = klad_core::doc::nodes_at(&c.nodes, frame);
                let hash = fnv(FNV_OFFSET, &serde_json::to_vec(&nodes).unwrap_or_default());
                (c.id.clone(), hash, nodes)
            })
            .collect();
        let fresh = cels
            .par_iter()
            .filter(|(id, hash, _)| self.known.get(id).is_none_or(|(h, _)| h != hash))
            .map(|(id, hash, nodes)| Ok((id.clone(), (*hash, rasterize_nodes(nodes, w, h)?))))
            .collect::<Result<Vec<_>>>()?;
        self.known.extend(fresh);
        cels.iter().for_each(|(id, _, _)| {
            if let Some((_, raster)) = self.known.get(id) {
                rasters.insert(id.clone(), raster.clone());
            }
        });
        Ok(())
    }
}

fn load_cel(bundle: &Bundle, brushes: &BrushSet, layer: &str, cel: &str, use_cache: bool) -> Result<CelLoad> {
    let cache = CelCache::new(bundle, brushes, Write::Always);
    let load = match use_cache {
        true => cache.load(bundle, brushes, layer, cel, false)?,
        false => {
            let (width, height) = (bundle.doc.canvas.width, bundle.doc.canvas.height);
            let path = bundle.strokes_path(layer, cel);
            let bytes = if path.exists() { fs::read(&path)? } else { Vec::new() };
            let marks = match bytes.is_empty() {
                true => Vec::new(),
                false => decode_cel(&bytes).with_context(|| format!("cel {cel}"))?,
            };
            let count = marks.len();
            let (replay, raster, dabs) =
                CelReplay::replay_tiled(width, height, marks, brushes).with_context(|| format!("cel {cel}"))?;
            cache.store(cel, cache.key(&bytes), &raster, &replay)?;
            klad_brush::cache::CelLoad {
                raster,
                replay,
                file_hash: 0,
                how: How::Replayed,
                strokes: count,
                dabs,
            }
        }
    };
    Ok(CelLoad {
        cel: cel.to_string(),
        raster: load.raster,
        cached: load.how == How::Cached,
        updated: matches!(load.how, How::Updated(_)),
        strokes: load.strokes,
        dabs: load.dabs,
    })
}

pub fn compress_raster(raster: &Raster) -> Result<Vec<u8>> {
    klad_brush::cache::compress(raster)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_brush::{PressureCurve, Raw, Recorder, StrokeBuffer, StrokePainter, commit, draw_stroke, needs_repaint};
    use klad_core::doc::{Document, Layer};
    use klad_core::ops::{Op, StrokeSpec};
    use klad_core::stroke::Stroke;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-layers-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn alpha(raster: &Raster, x: usize, y: usize) -> f32 {
        let t = klad_brush::TILE;
        raster
            .get(x / t, y / t)
            .map(|tile| tile[((y % t) * t + x % t) * 4 + 3].to_f32())
            .unwrap_or(0.0)
    }

    fn width_at(raster: &Raster, x: usize) -> usize {
        (0..raster.height as usize)
            .filter(|&y| alpha(raster, x, y) > 0.5)
            .count()
    }

    #[test]
    fn a_calibrated_stabilised_pen_stroke_replays_to_the_same_pixels_in_the_cli() {
        let dir = scratch("pen");
        let mut doc = Document::new(512, 256);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let bundle = Bundle::create(&dir, doc).unwrap();
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let brush = brushes.get("ink").unwrap();
        let def = brush.def.clone();
        let heavy = PressureCurve {
            low: 0.0,
            high: 0.88,
            gamma: 1.0,
        };
        let mut recorder = Recorder::new(heavy, def.stabilizer, def.smoothing);
        let mut stroke = Stroke::new(1, "ink", [0.05, 0.05, 0.08, 1.0], 24.0, 9).tapered(def.taper);
        let mut buffer = StrokeBuffer::new(512, 256);
        let mut live = StrokePainter::live(brush, &stroke);
        (0..180).for_each(|i| {
            let t = i as f32 / 179.0;
            let raw = Raw {
                x: 30.0 + 450.0 * t,
                y: 128.0 + 30.0 * libm::sinf(t * 6.0) + if i % 2 == 0 { 1.5 } else { -1.5 },
                pressure: 0.88 * libm::sinf(t * std::f32::consts::PI).min(0.999),
                tilt: (0.3, -0.2),
                dt_ms: if i == 0 { 0.0 } else { 4.5 },
            };
            let sample = recorder.push(raw);
            live.push(&sample, &mut buffer);
            stroke.samples.push(sample);
        });
        let tail = recorder.finish();
        tail.iter().for_each(|s| live.push(s, &mut buffer));
        stroke.samples.extend(tail);
        assert!(stroke.samples.iter().any(|s| s.pressure() > 0.97));
        assert!(needs_repaint(&stroke));
        buffer.reset();
        draw_stroke(&mut buffer, brush, &stroke);
        let mut editor = Raster::new(512, 256);
        commit(&mut editor, &mut buffer, stroke.color(), stroke.opacity, brush.blend);
        bundle.write_marks("l1", "c1", &[stroke.into()]).unwrap();
        let cli = load(&Bundle::open(&dir).unwrap(), 0, &brushes, false).unwrap();
        assert_eq!(cli.rasters["c1"].hash(), editor.hash());
        let cached = load(&Bundle::open(&dir).unwrap(), 0, &brushes, true).unwrap();
        assert_eq!(cached.cached, 1);
        assert_eq!(cached.rasters["c1"].hash(), editor.hash());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn agent_strokes_take_the_brush_taper_unless_told_otherwise() {
        let dir = scratch("taper");
        let mut doc = Document::new(512, 128);
        doc.layers.push(Layer::paint("l1", "ink", "c1"));
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let spec = |taper: Option<[f32; 2]>| {
            Op::Stroke(StrokeSpec {
                layer: "l1".into(),
                frame: 0,
                brush: "ink".into(),
                color: "#000000".into(),
                size: 16.0,
                opacity: 1.0,
                flow: 1.0,
                path: Some("M20 64 L490 64".into()),
                points: None,
                pressure: vec![1.0],
                tilt: [0.0, 0.0],
                speed: 700.0,
                seed: Some(3),
                taper,
                part: None,
                second: None,
            })
        };
        let mut ops = vec![spec(None), spec(Some([0.0, 0.0])), spec(Some([12.0, 30.0]))];
        crate::agent::resolve_strokes(&bundle, &mut ops).unwrap();
        bundle.apply(&ops).unwrap();
        let strokes = bundle.read_strokes("l1", "c1").unwrap();
        let tapers: Vec<[f32; 2]> = strokes.iter().map(|s| s.taper).collect();
        assert_eq!(tapers, [[10.0, 24.0], [0.0, 0.0], [12.0, 30.0]]);
        let brushes = BrushSet::for_bundle(&bundle).unwrap();
        let one = |stroke: &Stroke| {
            klad_brush::replay(512, 128, &[stroke.clone().into()], &brushes)
                .unwrap()
                .raster
        };
        let (plain, tapered) = (one(&strokes[1]), one(&strokes[2]));
        assert_eq!(width_at(&plain, 22), width_at(&plain, 250));
        assert!(width_at(&tapered, 22) < width_at(&tapered, 250) / 2);
        assert!(width_at(&tapered, 480) < width_at(&tapered, 250) / 2);
        let mut bad = vec![Op::Stroke(StrokeSpec {
            brush: "felt".into(),
            ..match spec(None) {
                Op::Stroke(s) => s,
                _ => unreachable!(),
            }
        })];
        let err = crate::agent::resolve_strokes(&bundle, &mut bad)
            .unwrap_err()
            .to_string();
        assert!(err.contains("felt") && err.contains("marker"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
