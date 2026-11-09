use anyhow::{Context, Result, bail};
use klad_core::bundle::{Blank, Bundle, now_ms};
use klad_core::color::parse_hex;
use klad_core::doc::{Cel, Kind};
use klad_core::stroke::{Mark, marks};
use klad_core::synth::Synth;
use notify::{Config, PollWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

pub fn new(path: &Path, blank: &Blank) -> Result<Bundle> {
    Bundle::blank(path, blank, "cli")
}

pub fn stress(
    path: &Path,
    size: u32,
    layers: usize,
    strokes: usize,
    seed: u64,
    (frames, every): (u32, u32),
) -> Result<()> {
    if every == 0 {
        bail!("--every must be at least 1");
    }
    let mut bundle = new(
        path,
        &Blank {
            width: size,
            height: size,
            layers,
            background: Some("#f4efe6".into()),
            frames,
            ..Blank::default()
        },
    )?;
    let doc = &mut bundle.doc;
    let paths: Vec<usize> = (0..doc.layers.len()).collect();
    paths.iter().for_each(|&i| {
        let starts: Vec<u32> = (0..frames).step_by(every as usize).collect();
        let ids: Vec<String> = starts.iter().skip(1).map(|_| doc.alloc_id('c')).collect();
        if let Some(cels) = doc.layers[i].cels_mut() {
            cels.extend(ids.iter().zip(&starts[1..]).map(|(id, f)| Cel::at(id, *f)));
        }
    });
    bundle.save()?;
    let synth = Synth::canvas(size as f32, size as f32);
    bundle
        .doc
        .paint_cels()
        .iter()
        .enumerate()
        .try_for_each(|(i, (layer, cel))| {
            bundle
                .write_marks(&layer.id, &cel.id, &marks(synth.strokes(strokes, seed + i as u64)))
                .map(|_| ())
        })
}

fn top_paint_layer(bundle: &Bundle) -> Result<String> {
    bundle
        .doc
        .all_layers()
        .into_iter()
        .rev()
        .find(|l| l.kind() == Kind::Paint)
        .map(|l| l.id.clone())
        .context("the bundle has no paint layer")
}

pub struct Scribble {
    pub layer: Option<String>,
    pub frame: u32,
    pub count: usize,
    pub seed: Option<u64>,
    pub size: Option<f32>,
    pub color: String,
}

pub fn scribble(path: &Path, opts: Scribble) -> Result<()> {
    let Scribble {
        layer,
        frame,
        count,
        seed,
        size,
        color,
    } = opts;
    let start = Instant::now();
    let mut bundle = Bundle::open(path)?;
    let layer = match layer {
        Some(id) => id,
        None => top_paint_layer(&bundle)?,
    };
    let owner = bundle.doc.layer(&layer).with_context(|| format!("no layer {layer}"))?;
    if owner.kind() != Kind::Paint {
        bail!("{layer} is not a paint layer");
    }
    let cel = owner
        .content_at(frame)
        .with_context(|| format!("layer {layer} has no cel at frame {frame}"))?
        .id
        .clone();
    let mut strokes = bundle.read_marks(&layer, &cel)?;
    let next = bundle.doc.mark_ids(&cel).next(&strokes);
    let (w, h) = (bundle.doc.canvas.width as f32, bundle.doc.canvas.height as f32);
    let mut synth = Synth::canvas(w, h);
    synth.brushes = vec!["ink".into()];
    synth.palette = vec![parse_hex(&color)?];
    synth.min_len = synth.max_len * 0.5;
    let added = synth
        .strokes(count, seed.unwrap_or_else(now_ms))
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s.id = next + i as u32;
            if let Some(size) = size {
                s.size = size;
            }
            Mark::Stroke(s)
        });
    strokes.extend(added);
    klad_core::history::checkpoint(
        &bundle,
        &bundle.doc,
        "cli",
        &format!("scribble on {layer}"),
        &[bundle.strokes_path(&layer, &cel)],
    )?;
    bundle.write_marks(&layer, &cel, &strokes)?;
    bundle.save()?;
    let stamp = bundle.stamp("cli")?;
    println!(
        "rev {} on {layer}: {} marks, written in {:.0} ms",
        stamp.rev,
        strokes.len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

pub fn watch(path: &Path, poll: bool, seconds: u64) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let (tx, rx) = channel();
    let handler = move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res
            && event.paths.iter().any(|p| p.ends_with("sync.json"))
        {
            let _ = tx.send(now_ms());
        }
    };
    let mut watcher: Box<dyn Watcher> = if poll {
        Box::new(PollWatcher::new(
            handler,
            Config::default().with_poll_interval(Duration::from_millis(20)),
        )?)
    } else {
        Box::new(notify::recommended_watcher(handler)?)
    };
    watcher.watch(&bundle.cache_dir(), RecursiveMode::NonRecursive)?;
    println!(
        "watching {} for {seconds} s ({})",
        bundle.stamp_path().display(),
        if poll { "polling every 20 ms" } else { "native events" }
    );
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut last = bundle.read_stamp().map_or(0, |s| s.rev);
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Ok(seen) = rx.recv_timeout(left) else {
            break;
        };
        match bundle.read_stamp() {
            Some(stamp) if stamp.rev != last && stamp.author != "cli" => {
                last = stamp.rev;
                println!(
                    "rev {} by {}: seen after {} ms",
                    stamp.rev,
                    stamp.author,
                    seen as i64 - stamp.at_ms as i64
                );
            }
            Some(stamp) => last = stamp.rev,
            None => {}
        }
    }
    Ok(())
}
