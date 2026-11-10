use crate::agent::{Look, onion_layers, render_path};
use crate::layers::load_cels;
use anyhow::Result;
use klad_brush::BrushSet;
use klad_core::bundle::{Bundle, write_atomic};
use klad_core::doc::{Document, Kind};
use klad_render::onion::{self, Onions};
use klad_render::sheet::{self, Plan};
use klad_render::{Frame, View, compose_view};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BATCH_BYTES: usize = 768 << 20;

pub struct Sheet {
    pub spec: String,
    pub drawings: bool,
    pub cell: u32,
    pub columns: Option<usize>,
}

pub(crate) type Cels = BTreeSet<(String, String, Kind)>;

pub(crate) fn shown(doc: &Document, frame: u32) -> Vec<String> {
    let keyed = doc.keyed_state(frame).into_iter().flat_map(|(id, values)| {
        values
            .into_iter()
            .map(move |(prop, bits)| format!("{id} {prop} {bits:08x}"))
    });
    doc.all_layers()
        .iter()
        .map(|l| l.content_at(frame).map_or(String::new(), |c| c.id.clone()))
        .chain(keyed)
        .collect()
}

pub(crate) fn frame_cels(doc: &Document, frame: u32, ghosts: &[(String, Vec<onion::Ghost>)]) -> Cels {
    doc.paint_content_at(frame)
        .iter()
        .map(|(l, c)| (l.id.clone(), c.id.clone(), Kind::Paint))
        .chain(onion::needed(doc, ghosts))
        .collect()
}

pub(crate) fn batches(frames: Vec<(u32, Cels)>, cel_bytes: usize) -> Vec<Vec<(u32, Cels)>> {
    let limit = (BATCH_BYTES / cel_bytes.max(1)).max(1);
    frames
        .into_iter()
        .fold(Vec::<Vec<(u32, Cels)>>::new(), |mut out, (frame, cels)| {
            let fits = out.last().is_some_and(|batch| {
                let union: BTreeSet<&(String, String, Kind)> = batch.iter().flat_map(|(_, c)| c).chain(&cels).collect();
                union.len() <= limit
            });
            match out.last_mut() {
                Some(batch) if fits => batch.push((frame, cels)),
                _ => out.push(vec![(frame, cels)]),
            }
            out
        })
}

pub fn render(path: &Path, args: &Sheet, look: &Look, out: Option<PathBuf>) -> Result<()> {
    let start = Instant::now();
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    let brushes = BrushSet::for_bundle(&bundle)?;
    let (from, to) = sheet::range(doc, &args.spec)?;
    let plan: Plan = sheet::plan(doc, from, to, args.drawings);
    let with_onion = look.wants_onion()?;
    let kinds = look.kinds()?;
    let arcs = match kinds.arcs {
        true => {
            let tracks = look.tracks()?;
            if tracks.is_empty() {
                anyhow::bail!("--overlay arcs needs --track: a mark (c4:s12), a part (leaf) or a point (x,y)");
            }
            crate::arcs::arcs_overlay(&crate::arcs::track(&bundle, &tracks, (from, to))?)
        }
        false => Vec::new(),
    };
    let parts: BTreeMap<u32, Vec<klad_render::overlay::Mark>> = match kinds.parts {
        true => (from..=to)
            .map(|f| Ok((f, crate::arcs::part_marks(&bundle, f)?)))
            .collect::<Result<_>>()?,
        false => BTreeMap::new(),
    };
    let layers = if with_onion {
        onion_layers(&bundle, look)?
    } else {
        Vec::new()
    };
    let ghosts = |frame: u32| match with_onion {
        true => onion::plan(doc, frame, &layers, &look.onion),
        false => Vec::new(),
    };
    let mut distinct: BTreeMap<Vec<String>, u32> = BTreeMap::new();
    let picks: Vec<u32> = plan
        .spots
        .iter()
        .map(|s| match with_onion {
            true => s.from,
            false => *distinct.entry(shown(doc, s.from)).or_insert(s.from),
        })
        .collect();
    let unique: Vec<u32> = picks.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let scale = args.cell as f32 / w.max(h) as f32;
    let work: Vec<(u32, Cels)> = unique.iter().map(|&f| (f, frame_cels(doc, f, &ghosts(f)))).collect();
    let mut images: BTreeMap<u32, Frame> = BTreeMap::new();
    batches(work, w as usize * h as usize * 8)
        .into_iter()
        .try_for_each(|batch| {
            let needed: Vec<(String, String, Kind)> = batch
                .iter()
                .flat_map(|(_, c)| c.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let rasters = load_cels(&bundle, &brushes, &needed)?;
            let done = batch
                .par_iter()
                .map(|(frame, _)| {
                    let plan = ghosts(*frame);
                    let onions: Option<Onions> = with_onion.then(|| onion::build(doc, &plan, &rasters));
                    let overlay = (kinds.arcs || kinds.parts).then(|| klad_render::overlay::Overlay {
                        marks: parts.get(frame).cloned().unwrap_or_default(),
                        boxes: kinds.parts,
                        ids: kinds.parts,
                        arcs: arcs.clone(),
                        frame: Some(*frame),
                        ..Default::default()
                    });
                    let image = compose_view(
                        doc,
                        *frame,
                        &rasters,
                        View::scaled(scale),
                        overlay.as_ref(),
                        onions.as_ref(),
                    )?;
                    Ok((*frame, image))
                })
                .collect::<Result<Vec<_>>>()?;
            images.extend(done);
            Ok::<_, anyhow::Error>(())
        })?;
    let refs: Vec<&Frame> = picks.iter().map(|f| &images[f]).collect();
    let clip = doc.timeline.clip(&args.spec);
    let heading = format!(
        "{}frames {from}-{to} at {} fps{}{}",
        clip.map_or(String::new(), |c| format!(
            "clip {}{}: ",
            c.name,
            if c.looping { " (loops)" } else { "" }
        )),
        clip.map_or(doc.timeline.fps, |c| doc.timeline.fps_of(c)),
        if args.drawings { ", one cell per drawing" } else { "" },
        if with_onion { ", onion skin" } else { "" }
    );
    let columns = args.columns.unwrap_or_else(|| sheet::columns_for(refs.len()));
    let mut picture = sheet::draw(&plan, &refs, columns, &heading)?;
    let tag = format!(
        "-sheet-{}{}{}-{}",
        args.spec.replace(['/', ' '], "_"),
        if args.drawings { "-drawings" } else { "" },
        if with_onion { "-onion" } else { "" },
        args.cell
    );
    let path = out.unwrap_or_else(|| render_path(&bundle, from, View::scaled(1.0), &tag));
    write_atomic(&path, &picture.png()?)?;
    println!("{}", path.display());
    eprintln!(
        "{}x{}  hash {:016x}  {} cells, {} frames composed  {:.0} ms",
        picture.width,
        picture.height,
        picture.hash(),
        refs.len(),
        unique.len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Cel, Layer};
    use klad_core::stroke::{Stroke, marks};

    #[test]
    fn batches_keep_their_cels_under_the_budget() {
        let cel = |i: usize| (format!("l{i}"), format!("c{i}"), Kind::Paint);
        let frames: Vec<(u32, Cels)> = (0..6)
            .map(|f| (f, [cel(f as usize), cel(99)].into_iter().collect()))
            .collect();
        let sizes = |b: Vec<Vec<(u32, Cels)>>| b.iter().map(Vec::len).collect::<Vec<_>>();
        assert_eq!(sizes(batches(frames.clone(), BATCH_BYTES / 4)), [3, 3]);
        assert_eq!(sizes(batches(frames.clone(), BATCH_BYTES)), [1, 1, 1, 1, 1, 1]);
        assert_eq!(sizes(batches(frames, 1)), [6]);
    }

    #[test]
    fn a_sheet_with_onion_renders_and_holds_its_hash() {
        let dir = std::env::temp_dir().join(format!("klad-cli-sheet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(96, 64);
        doc.canvas.background = Some("#ffffff".into());
        doc.timeline.frames = 4;
        let mut ball = Layer::paint("l1", "ball", "c1");
        *ball.cels_mut().unwrap() = vec![
            Cel {
                hold: Some(2),
                ..Cel::at("c1", 0)
            },
            Cel::at("c2", 2),
        ];
        doc.layers = vec![ball];
        let bundle = Bundle::create(&dir, doc).unwrap();
        let dot = |x: f32| {
            marks(vec![
                Stroke::new(1, "ink", [0.1, 0.1, 0.1, 1.0], 20.0, 1).with_points(&[(x, 32.0, 1.0)], 0.0),
            ])
        };
        bundle.write_marks("l1", "c1", &dot(24.0)).unwrap();
        bundle.write_marks("l1", "c2", &dot(72.0)).unwrap();
        let args = Sheet {
            spec: "all".into(),
            drawings: false,
            cell: 48,
            columns: None,
        };
        let out = dir.join("sheet.png");
        let look = Look::plain(Some("onion".into()));
        render(&dir, &args, &look, Some(out.clone())).unwrap();
        let first = std::fs::read(&out).unwrap();
        assert!(first.starts_with(&[0x89, b'P', b'N', b'G']));
        render(&dir, &args, &look, Some(out.clone())).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
