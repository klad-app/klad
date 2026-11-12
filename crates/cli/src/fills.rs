use crate::agent::{Output, preview};
use crate::layers::{self, Vectors};
use anyhow::{Context, Result, anyhow, bail};
use klad_brush::{BrushSet, Raster, flood, paint_fill, polygon, replay, replay_onto};
use klad_core::bundle::Bundle;
use klad_core::doc::{Document, Kind};
use klad_core::fill::{Coverage, FillRef, FillSource};
use klad_core::ops::{Applied, Op, apply};
use klad_core::stroke::Mark;
use klad_render::{Rasters, fill_reference};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

fn needs_vectors(doc: &Document, reference: &FillRef) -> bool {
    match reference {
        FillRef::This => false,
        FillRef::All => true,
        FillRef::Layer(id) => doc.layer(id).is_some_and(|l| l.kind() != Kind::Paint),
    }
}

pub struct Computed {
    pub coverage: Coverage,
    pub reference_ms: f64,
    pub fill_ms: f64,
}

pub fn compute(doc: &Document, frame: u32, rasters: &Rasters, cel: &str, source: &FillSource) -> Result<Computed> {
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let start = Instant::now();
    match source {
        FillSource::Seed { seed, reference } => {
            let image = fill_reference(doc, frame, rasters, cel, reference)?;
            let reference_ms = ms(start);
            let start = Instant::now();
            let coverage = flood(&image, seed);
            Ok(Computed {
                coverage,
                reference_ms,
                fill_ms: ms(start),
            })
        }
        FillSource::Polygon(points) => Ok(Computed {
            coverage: polygon(points, w, h),
            reference_ms: 0.0,
            fill_ms: ms(start),
        }),
        FillSource::Cells { .. } => bail!("a pixel mark keeps its own cells: there is nothing to compute"),
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

struct Running<'a> {
    bundle: &'a Bundle,
    brushes: BrushSet,
    rasters: Rasters,
    stored: BTreeMap<String, Vec<Mark>>,
    painted: BTreeMap<String, Vec<Mark>>,
    vectors: Vectors,
}

impl Running<'_> {
    fn ensure(&mut self, layer: &str, cel: &str) -> Result<()> {
        if !self.rasters.contains_key(cel) {
            let raster = layers::load_one(self.bundle, &self.brushes, layer, cel)?;
            self.rasters.insert(cel.to_string(), raster);
        }
        Ok(())
    }

    fn catch_up(&mut self, doc: &Document, applied: &Applied, frame: u32) -> Result<()> {
        doc.paint_content_at(frame)
            .iter()
            .try_for_each(|(layer, cel)| self.ensure(&layer.id, &cel.id))?;
        applied.marks.iter().try_for_each(|pending| {
            self.ensure(&pending.layer, &pending.cel)?;
            if !self.stored.contains_key(&pending.cel) {
                let marks = self.bundle.read_marks(&pending.layer, &pending.cel)?;
                self.stored.insert(pending.cel.clone(), marks);
            }
            let stored = self.stored[&pending.cel].clone();
            let mut marks = stored.clone();
            pending.changes.clone().iter_mut().try_for_each(|c| {
                self.bundle
                    .apply_change(&self.bundle.doc, &applied.marks, &mut marks, c, &pending.cel)
            })?;
            let before = self.painted.get(&pending.cel).cloned().unwrap_or(stored);
            let (w, h) = (doc.canvas.width, doc.canvas.height);
            let raster = self
                .rasters
                .get_mut(&pending.cel)
                .ok_or_else(|| anyhow!("cel {} has no pixels", pending.cel))?;
            if marks.len() >= before.len() && marks[..before.len()] == before[..] {
                replay_onto(raster, &marks[before.len()..], &self.brushes)?;
            } else {
                *raster = replay(w, h, &marks, &self.brushes)?.raster;
            }
            self.painted.insert(pending.cel.clone(), marks);
            Ok(())
        })
    }
}

pub fn resolve(bundle: &Bundle, ops: &mut [Op]) -> Result<Vec<String>> {
    if ops.iter().any(|op| matches!(op, Op::Fill(f) if f.like.is_some())) {
        if ops.len() > 1 {
            bail!(
                "a fill like another drawing's needs a batch of its own: it reads the drawings from the bundle as they are"
            );
        }
        let Op::Fill(spec) = &mut ops[0] else {
            unreachable!("checked above");
        };
        return crate::colour::like(bundle, spec);
    }
    if !ops.iter().any(|op| matches!(op, Op::Fill(_))) {
        return Ok(Vec::new());
    }
    let mut running = Running {
        bundle,
        brushes: BrushSet::for_bundle(bundle)?,
        rasters: Rasters::new(),
        stored: BTreeMap::new(),
        painted: BTreeMap::new(),
        vectors: Vectors::default(),
    };
    let mut doc = bundle.doc.clone();
    let mut applied = Applied::default();
    let mut notes = Vec::new();
    ops.iter_mut().enumerate().try_for_each(|(i, op)| {
        if let Op::Fill(spec) = op {
            let source = spec.source(&doc).with_context(|| format!("op {}", i + 1))?;
            if matches!(source, FillSource::Seed { .. }) {
                running.catch_up(&doc, &applied, spec.frame)?;
            }
            let mut rasters = running.rasters.clone();
            if let FillSource::Seed { reference, .. } = &source
                && needs_vectors(&doc, reference)
            {
                running.vectors.add_to(&doc, spec.frame, &mut rasters)?;
            }
            let cel = doc
                .layer(&spec.layer)
                .and_then(|l| l.content_at(spec.frame))
                .map(|c| c.id.clone())
                .unwrap_or_default();
            let done = compute(&doc, spec.frame, &rasters, &cel, &source)?;
            if done.coverage.is_empty() {
                bail!(
                    "op {}: the fill on {} covers no pixels (outside the canvas, or a polygon with no area)",
                    i + 1,
                    spec.layer
                );
            }
            notes.push(format!(
                "fill on {}: {} px, reference {:.0} ms, fill {:.0} ms",
                spec.layer,
                done.coverage.pixels(),
                done.reference_ms,
                done.fill_ms
            ));
            spec.coverage = Some(done.coverage);
        }
        apply(&mut doc, op, &mut applied).with_context(|| format!("op {}", i + 1))
    })?;
    match notes.len() {
        0..=4 => Ok(notes),
        n => Ok(vec![format!("{n} fills computed")]),
    }
}

pub fn refill(path: &Path, layer: &str, frame: u32, only: Option<u32>, out: &Output) -> Result<()> {
    let start = Instant::now();
    let mut bundle = Bundle::open(path)?;
    let doc = bundle.doc.clone();
    let owner = doc.layer(layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
    if owner.kind() != Kind::Paint {
        bail!("{layer} is a {} layer; fills live on paint layers", owner.kind().name());
    }
    let cel = owner
        .content_at(frame)
        .ok_or_else(|| anyhow!("layer {layer} has no cel at frame {frame}"))?
        .id
        .clone();
    let brushes = BrushSet::for_bundle(&bundle)?;
    let mut rasters = layers::load(&bundle, frame, &brushes, true)?.rasters;
    let mut marks = bundle.read_marks(layer, &cel)?;
    if let Some(id) = only
        && !marks.iter().any(|m| m.id() == id && m.as_fill().is_some())
    {
        bail!("cel {cel} has no fill {id}");
    }
    let mut vectors = Vectors::default();
    let mut own = Raster::new(doc.canvas.width, doc.canvas.height);
    let mut changes = Vec::new();
    marks.iter_mut().try_for_each(|mark| {
        let redo = match mark {
            Mark::Fill(fill) if only.is_none_or(|id| id == fill.id) => match &fill.source {
                FillSource::Seed { reference, .. } => Some(reference.clone()),
                FillSource::Polygon(_) | FillSource::Cells { .. } => None,
            },
            _ => None,
        };
        if let (Some(reference), Mark::Fill(fill)) = (redo, &mut *mark) {
            rasters.insert(cel.clone(), own.clone());
            if needs_vectors(&doc, &reference) {
                vectors.add_to(&doc, frame, &mut rasters)?;
            }
            let done = compute(&doc, frame, &rasters, &cel, &fill.source)?;
            changes.push((fill.id, fill.coverage.pixels(), done.coverage.pixels()));
            fill.coverage = done.coverage;
        }
        match mark {
            Mark::Fill(fill) => {
                paint_fill(&mut own, fill);
            }
            stroke => {
                replay_onto(&mut own, std::slice::from_ref(stroke), &brushes)?;
            }
        }
        Ok::<_, anyhow::Error>(())
    })?;
    if changes.is_empty() {
        println!("no flood fills to recompute on {layer} cel {cel}");
        return Ok(());
    }
    bundle.replace_marks(layer, &cel, &marks, "cli", &format!("refill on {layer}"))?;
    bundle.stamp("cli")?;
    changes.iter().for_each(|(id, before, after)| {
        println!("fill {id}: {before} px -> {after} px");
    });
    println!(
        "refilled {} on {layer} cel {cel} in {:.0} ms\nrev {}",
        changes.len(),
        ms(start),
        bundle.doc.rev
    );
    if let Some(p) = preview(&bundle, out)? {
        println!("{}", p.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::Layer;
    use klad_core::ops::parse_lines;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-fills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn ring(layer: &str, cx: f32) -> String {
        let points: Vec<String> = (0..=240)
            .map(|i| {
                let t = i as f32 / 240.0 * std::f32::consts::TAU;
                format!("[{},{},1]", cx + 60.0 * libm::cosf(t), 128.0 + 60.0 * libm::sinf(t))
            })
            .collect();
        format!(
            r#"{{"op":"stroke","layer":"{layer}","brush":"ink","size":4,"taper":[0,0],"points":[{}]}}"#,
            points.join(",")
        )
    }

    #[test]
    fn agents_ink_and_fill_in_one_batch_and_refill_after_the_lines_change() {
        let dir = scratch("batch");
        let mut doc = Document::new(512, 256);
        doc.layers = vec![
            Layer::paint("fills", "fills", "c1"),
            Layer::paint("lines", "lines", "c2"),
        ];
        doc.next_id = 2;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let text = [
            ring("lines", 128.0),
            r##"{"op":"palette","add":"#d04030","name":"red"}"##.to_string(),
            r##"{"op":"fill","layer":"fills","at":[128,128],"ref":"lines","color":"red"}"##.to_string(),
            r##"{"op":"fill","layer":"fills","at":[10,10],"ref":"lines","color":"#204060"}"##.to_string(),
            r##"{"op":"fill","layer":"lines","at":[10,10],"color":"#ffffff"}"##.to_string(),
        ]
        .join("\n");
        let mut ops = parse_lines(&text).unwrap();
        let notes = resolve(&bundle, &mut ops).unwrap();
        assert_eq!(notes.len(), 3);
        bundle.apply(&ops).unwrap();
        let fills = bundle.read_marks("fills", "c1").unwrap();
        let inner = fills[0].as_fill().unwrap();
        assert_eq!(inner.reference(), Some(&FillRef::Layer("lines".into())));
        assert_eq!(
            inner.color(),
            klad_core::color::parse_hex("#d04030")
                .unwrap()
                .map(|c| (c * 65535.0).round() / 65535.0)
        );
        assert_eq!((inner.coverage.at(128, 128), inner.coverage.at(5, 5)), (255, 0));
        let inside = inner.coverage.pixels();
        assert!((10_000..12_500).contains(&inside), "{inside}");
        assert_eq!(fills[1].as_fill().unwrap().coverage.at(384, 128), 255);
        let lines = bundle.read_marks("lines", "c2").unwrap();
        assert_eq!(lines.len(), 2);
        let outside = lines[1].as_fill().unwrap();
        assert_eq!((outside.coverage.at(10, 10), outside.coverage.at(128, 128)), (255, 0));
        let mut more = parse_lines(&ring("lines", 384.0)).unwrap();
        resolve(&bundle, &mut more).unwrap();
        bundle.apply(&more).unwrap();
        let output = Output {
            json: false,
            render: false,
            scale: None,
            frame: 0,
            overlay: None,
        };
        refill(&dir, "fills", 0, None, &output).unwrap();
        let fills = Bundle::open(&dir).unwrap().read_marks("fills", "c1").unwrap();
        assert_eq!(fills[0].as_fill().unwrap().coverage.pixels(), inside);
        let background = fills[1].as_fill().unwrap();
        assert_eq!(
            (background.coverage.at(10, 10), background.coverage.at(384, 128)),
            (255, 0)
        );
        refill(&dir, "lines", 0, None, &output).unwrap();
        let after = Bundle::open(&dir).unwrap().read_marks("lines", "c2").unwrap();
        assert_eq!(after.len(), 3);
        let own = after[1].as_fill().unwrap();
        assert_eq!(
            own.coverage.at(384, 128),
            255,
            "a fill only sees the marks drawn before it"
        );
        assert!(refill(&dir, "lines", 0, Some(1), &output).is_err());
        let mut outside = parse_lines(r#"{"op":"fill","layer":"fills","at":[900,10]}"#).unwrap();
        let err = resolve(&Bundle::open(&dir).unwrap(), &mut outside)
            .unwrap_err()
            .to_string();
        assert!(err.contains("covers no pixels"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore = "timing: cargo test --release -p klad-cli fill_command_timing -- --ignored --nocapture"]
    fn fill_command_timing() {
        let dir = scratch("timing");
        let mut doc = Document::new(2048, 2048);
        doc.canvas.background = Some("#f4efe6".into());
        doc.layers = vec![
            Layer::paint("fills", "fills", "c1"),
            Layer::paint("lines", "lines", "c2"),
        ];
        doc.next_id = 2;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let lines: String = (0..12)
            .map(|i| {
                let (x, y) = (200.0 + (i % 4) as f32 * 450.0, 300.0 + (i / 4) as f32 * 600.0);
                format!(
                    r#"{{"op":"stroke","layer":"lines","size":6,"path":"M{x} {y} L{} {y} L{} {} L{x} {} Z"}}"#,
                    x + 380.0,
                    x + 380.0,
                    y + 500.0,
                    y + 500.0
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        bundle.apply(&parse_lines(&lines).unwrap()).unwrap();
        ["lines", "all"].iter().for_each(|reference| {
            let line = format!(r##"{{"op":"fill","layer":"fills","at":[1000,1000],"ref":"{reference}","gap":4}}"##);
            let mut ops = parse_lines(&line).unwrap();
            let start = Instant::now();
            let notes = resolve(&bundle, &mut ops).unwrap();
            println!("{reference}: {} ({:.0} ms in all)", notes[0], ms(start));
        });
        fs::remove_dir_all(&dir).unwrap();
    }
}
