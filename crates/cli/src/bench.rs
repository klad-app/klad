use crate::layers::compress_raster;
use anyhow::Result;
use klad_brush::{BrushSet, CelReplay, Raster, StrokeBuffer, draw_stroke, replay};
use klad_core::doc::{Document, Layer, Node, Shape};
use klad_core::rng::Rng;
use klad_core::stroke::{Mark, Stroke, decode_cel, encode_cel, encode_raw, marks};
use klad_core::synth::{Gesture, Synth};
use klad_render::{View, compose};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::time::Instant;

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn mb(bytes: usize) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

pub fn run(count: usize, size: u32, seed: u64) -> Result<()> {
    let brushes = BrushSet::presets();
    let strokes = Synth::canvas(size as f32, size as f32).strokes(count, seed);
    stroke_format(&strokes)?;
    brush_speed(&brushes)?;
    repaint_speed(&brushes)?;
    replay_speed(&strokes, size, &brushes)?;
    still_speed(&brushes, seed)?;
    compose_speed(&brushes, seed)?;
    Ok(())
}

fn stroke_format(strokes: &[Stroke]) -> Result<()> {
    let samples: usize = strokes.iter().map(|s| s.samples.len()).sum();
    let start = Instant::now();
    let all = marks(strokes.iter().cloned());
    let raw = encode_raw(&all);
    let packed = encode_cel(&all)?;
    let encode = ms(start);
    let start = Instant::now();
    let back = decode_cel(&packed)?;
    let decode = ms(start);
    assert_eq!(back.len(), strokes.len());
    println!(
        "stroke format    {} strokes, {:.1} samples each at 200 Hz",
        strokes.len(),
        samples as f64 / strokes.len() as f64
    );
    println!("  plain f32      {} (24 B per sample)", mb(samples * 24));
    println!("  delta varint   {}", mb(raw.len()));
    println!(
        "  zstd           {} = {:.0} B per stroke, {:.2} B per sample",
        mb(packed.len()),
        packed.len() as f64 / strokes.len() as f64,
        packed.len() as f64 / samples as f64
    );
    println!("  encode {encode:.1} ms, decode {decode:.1} ms");
    Ok(())
}

fn brush_speed(brushes: &BrushSet) -> Result<()> {
    println!("brush speed      one 1000 px stroke at pressure 0.8");
    ["pencil", "ink", "soft", "chalk"].iter().try_for_each(|name| {
        [10.0f32, 60.0, 300.0].iter().try_for_each(|&size| {
            let mut rng = Rng::new(3);
            let gesture = Gesture {
                start: (500.0, 1000.0),
                heading: 0.0,
                length: 1000.0,
                speed: 1000.0,
                curl: 1.0,
                pressure: 0.8,
                attack: 0.01,
                release: 0.01,
                tilt: (0.0, 0.0),
            };
            let mut stroke = Stroke::new(1, name, [0.2, 0.2, 0.2, 1.0], size, 5);
            stroke.samples = gesture.samples(200.0, &mut rng);
            let reps = if size >= 300.0 { 3 } else { 20 };
            let start = Instant::now();
            let dabs: usize = (0..reps)
                .map(|_| replay(2048, 2048, &[Mark::Stroke(stroke.clone())], brushes).map(|r| r.dabs))
                .sum::<Result<usize>>()?;
            let per = ms(start) / reps as f64;
            println!(
                "  {name:<7}{size:>4} px  {per:>7.2} ms  {:>5} dabs  {:>7.0} dabs/s",
                dabs / reps,
                (dabs / reps) as f64 / (per / 1000.0)
            );
            Ok(())
        })
    })
}

fn repaint_speed(brushes: &BrushSet) -> Result<()> {
    println!("pen-up repaint   a tapered ink stroke is drawn again at pen up so its end can taper");
    let ink = brushes.get("ink")?;
    let mut buffer = StrokeBuffer::new(2048, 2048);
    [(1000.0f32, 12.0f32), (1000.0, 60.0), (3000.0, 12.0), (3000.0, 400.0)]
        .iter()
        .for_each(|&(length, size)| {
            let mut rng = Rng::new(4);
            let gesture = Gesture {
                start: (1024.0, 1024.0),
                heading: 0.3,
                length,
                speed: 400.0,
                curl: 4.0,
                pressure: 0.8,
                attack: 0.05,
                release: 0.1,
                tilt: (0.0, 0.0),
            };
            let mut stroke = Stroke::new(1, "ink", [0.1, 0.1, 0.1, 1.0], size, 5).tapered(ink.def.taper);
            stroke.samples = gesture.samples(220.0, &mut rng);
            let reps = 5;
            let start = Instant::now();
            (0..reps).for_each(|_| {
                buffer.reset();
                draw_stroke(&mut buffer, ink, &stroke);
            });
            println!(
                "  {length:>5.0} px line at {size:>4} px  {:>7.2} ms  ({} samples)",
                ms(start) / reps as f64,
                stroke.samples.len()
            );
        });
    Ok(())
}

fn replay_layers(layers: &[Vec<Stroke>], size: u32, brushes: &BrushSet, threads: usize) -> Result<(Vec<Raster>, f64)> {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let start = Instant::now();
    let rasters = pool.install(|| {
        layers
            .par_iter()
            .map(|l| replay(size, size, &marks(l.iter().cloned()), brushes).map(|r| r.raster))
            .collect::<Result<Vec<_>>>()
    })?;
    Ok((rasters, ms(start)))
}

fn replay_speed(strokes: &[Stroke], size: u32, brushes: &BrushSet) -> Result<()> {
    let start = Instant::now();
    let single = replay(size, size, &marks(strokes.iter().cloned()), brushes)?;
    let one_layer = ms(start);
    println!(
        "replay           {} strokes on one {size}x{size} layer: {:.2} s, {} dabs, {} tiles, {}",
        strokes.len(),
        one_layer / 1000.0,
        single.dabs,
        single.raster.count(),
        mb(single.raster.bytes())
    );
    let start = Instant::now();
    let packed = compress_raster(&single.raster)?;
    let write = ms(start);
    let start = Instant::now();
    let restored = Raster::decode(&zstd::decode_all(&packed[..])?)?;
    let read = ms(start);
    println!(
        "  tile cache     {} compressed, write {write:.0} ms, read {read:.0} ms, intact {}",
        mb(packed.len()),
        restored.hash() == single.raster.hash()
    );
    let layers: Vec<Vec<Stroke>> = (0..8)
        .map(|k| strokes.iter().skip(k).step_by(8).cloned().collect())
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let (serial, serial_ms) = replay_layers(&layers, size, brushes, 1)?;
    let (parallel, parallel_ms) = replay_layers(&layers, size, brushes, threads)?;
    let same = serial.iter().zip(&parallel).all(|(a, b)| a.hash() == b.hash());
    println!(
        "  as 8 layers    1 thread {:.2} s, {threads} threads {:.2} s, identical pixels {same}",
        serial_ms / 1000.0,
        parallel_ms / 1000.0
    );
    Ok(())
}

fn still_speed(brushes: &BrushSet, seed: u64) -> Result<()> {
    let (w, h) = (960u32, 480u32);
    let mut rng = Rng::new(seed ^ 0x5717);
    let all: Vec<Mark> = (0..41_731u32)
        .map(|i| {
            let (x, y) = (rng.unit() * w as f32, rng.unit() * h as f32);
            let a = rng.unit() * std::f32::consts::TAU;
            let length = 8.0 + rng.unit() * 32.0;
            let size = match i < 31_735 {
                true => 4.0 + rng.unit() * 12.0,
                false => 1.5 + rng.unit() * 1.5,
            };
            let points: Vec<(f32, f32, f32)> = (0..5)
                .map(|k| {
                    let t = k as f32 / 4.0;
                    (x + a.cos() * length * t, y + a.sin() * length * t, [0.2, 0.9, 1.0, 0.8, 0.1][k])
                })
                .collect();
            let colour = [0.3 + rng.unit() * 0.6, 0.3 + rng.unit() * 0.5, 0.3 + rng.unit() * 0.4, 1.0];
            Stroke::new(i + 1, "oil", colour, size, i).with_points(&points, 8.0).into()
        })
        .collect();
    let start = Instant::now();
    let (_, serial, dabs) = CelReplay::replay(w, h, all.clone(), brushes)?;
    let one = ms(start);
    let start = Instant::now();
    let (_, tiled, _) = CelReplay::replay_tiled(w, h, all, brushes)?;
    let spread = ms(start);
    println!(
        "still            41731 oil strokes at {w}x{h}, {dabs} dabs: in order {:.2} s, tile by tile {:.2} s, identical pixels {}",
        one / 1000.0,
        spread / 1000.0,
        serial.hash() == tiled.hash()
    );
    Ok(())
}

fn compose_speed(brushes: &BrushSet, seed: u64) -> Result<()> {
    let size = 2048u32;
    let synth = Synth::canvas(size as f32, size as f32);
    let layers: Vec<Vec<Stroke>> = (0..8).map(|k| synth.strokes(1500, seed + 100 + k)).collect();
    let (rasters, _) = replay_layers(&layers, size, brushes, 8)?;
    let mut doc = Document::new(size, size);
    let background = Shape::Rect {
        x: 0.0,
        y: 0.0,
        w: 2048.0,
        h: 2048.0,
        radius: 0.0,
    };
    doc.layers.push(Layer::vector(
        "bg",
        "bg",
        "bgc",
        vec![Node::new("n1", background).filled("#203040")],
    ));
    doc.layers
        .extend((0..8).map(|k| Layer::paint(&format!("p{k}"), "paint", &format!("pc{k}"))));
    let hill = Shape::Path {
        d: "M0 2048 L0 1600 C 600 1400 1400 1800 2048 1500 L2048 2048 Z".into(),
    };
    doc.layers.push(Layer::vector(
        "fg",
        "fg",
        "fgc",
        vec![Node::new("n2", hill).filled("#101820")],
    ));
    let map: BTreeMap<String, Raster> = rasters
        .into_iter()
        .enumerate()
        .map(|(k, r)| (format!("pc{k}"), r))
        .collect();
    [1.0f32, 0.375].iter().try_for_each(|&scale| {
        let start = Instant::now();
        let mut frame = compose(&doc, 0, &map, View::scaled(scale))?;
        let composed = ms(start);
        let start = Instant::now();
        let png = frame.png()?;
        println!(
            "compose          10 layers {size}x{size} at {scale}: {composed:.0} ms, PNG {:.0} ms ({}), out {}x{}",
            ms(start),
            mb(png.len()),
            frame.width,
            frame.height
        );
        Ok(())
    })
}
