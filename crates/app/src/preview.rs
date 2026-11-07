use crate::composite::{compose_items, dilate, effect_reach};
use anyhow::{Result, anyhow, bail};
use half::f16;
use klad_brush::tiles::TILE_PX;
use klad_brush::{BrushSet, CelReplay, Raster};
use klad_core::bundle::Bundle;
use klad_core::doc::{Kind, Layer, Node};
use klad_core::ops::{Op, apply_all};
use klad_render::onion::Onions;
use klad_render::{Rasters, changed_tiles, rasterize_nodes, rasterize_tiles};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

pub struct Snapshot {
    pub rev: u64,
    pub bundle: Bundle,
    pub frame: u32,
    pub rasters: Rasters,
    pub cels: BTreeMap<String, CelReplay>,
    pub onions: Option<Onions>,
    pub brushes: BrushSet,
    pub vectors: OnceLock<BTreeMap<String, Vec<Node>>>,
}

pub struct Job {
    pub ops: Vec<Op>,
    pub snap: Arc<Snapshot>,
    pub shown: BTreeSet<usize>,
    pub visible: BTreeSet<usize>,
    pub cancel: Arc<AtomicBool>,
}

pub struct Done {
    pub ops: Vec<Op>,
    pub tiles: Vec<(usize, Vec<f16>)>,
    pub changed: BTreeSet<usize>,
    pub hidden: BTreeSet<usize>,
    pub exact: bool,
}

fn vector_nodes(doc: &klad_core::doc::Document, frame: u32) -> BTreeMap<String, Vec<Node>> {
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Vector)
        .filter_map(|l| l.content_at(frame))
        .map(|c| (c.id.clone(), c.nodes.clone()))
        .collect()
}

impl Job {
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("the drag ended");
        }
        Ok(())
    }

    pub fn run(self) -> Result<Done> {
        let snap = &self.snap;
        let (doc, mut applied) = apply_all(&snap.bundle.doc, &self.ops)?;
        self.check()?;
        let (w, h) = (doc.canvas.width, doc.canvas.height);
        let mut rasters = snap.rasters.clone();
        let mut changed = BTreeSet::new();
        let before = snap.vectors.get_or_init(|| vector_nodes(&snap.bundle.doc, snap.frame));
        let redrawn = vector_nodes(&doc, snap.frame)
            .into_iter()
            .filter(|(cel, nodes)| before.get(cel) != Some(nodes))
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|(cel, nodes)| {
                let old = before.get(&cel);
                let drawn = rasters.get(&cel);
                let tiles = match (old, drawn) {
                    (Some(old), Some(_)) => changed_tiles(old, &nodes, w, h)?,
                    _ => None,
                };
                let raster = match (tiles, drawn) {
                    (Some(tiles), Some(drawn)) => {
                        let mut patched = drawn.clone();
                        rasterize_tiles(&nodes, w, h, &tiles)?
                            .into_iter()
                            .for_each(|(i, tile)| match tile {
                                Some(tile) => patched.put(i, tile.into_boxed_slice()),
                                None => patched.clear(i),
                            });
                        patched
                    }
                    _ => rasterize_nodes(&nodes, w, h)?,
                };
                let differs = match drawn {
                    Some(drawn) => drawn.differs(&raster),
                    None => raster.occupied().map(|(i, _)| i).collect(),
                };
                Ok((cel, raster, differs))
            })
            .collect::<Result<Vec<_>>>()?;
        redrawn.into_iter().for_each(|(cel, raster, differs)| {
            changed.extend(differs);
            rasters.insert(cel, raster);
        });
        let plan = applied.marks.clone();
        let exact = applied.marks.is_empty();
        applied.marks.iter_mut().try_for_each(|pending| {
            let replay = snap
                .cels
                .get(&pending.cel)
                .ok_or_else(|| anyhow!("cel {} is not in memory", pending.cel))?;
            let mut replay = replay.clone();
            let mut marks = replay.marks().to_vec();
            pending
                .changes
                .iter_mut()
                .try_for_each(|change| snap.bundle.apply_change(&plan, &mut marks, change, &pending.cel))?;
            let mut raster = rasters.get(&pending.cel).cloned().unwrap_or_else(|| Raster::new(w, h));
            changed.extend(replay.update(&mut raster, marks, &snap.brushes)?.tiles);
            rasters.insert(pending.cel.clone(), raster);
            Ok::<_, anyhow::Error>(())
        })?;
        self.check()?;
        let cols = (w as usize).div_ceil(klad_brush::TILE);
        let rows = (h as usize).div_ceil(klad_brush::TILE);
        let reach = effect_reach(&doc, &doc.layers);
        let changed: BTreeSet<usize> = dilate(&changed.into_iter().collect::<Vec<_>>(), reach, cols, rows)
            .into_iter()
            .collect();
        let wanted: Vec<usize> = changed.union(&self.shown).copied().collect();
        let (now, hidden): (Vec<usize>, Vec<usize>) = wanted.into_iter().partition(|i| self.visible.contains(i));
        let layers: Vec<&Layer> = doc.layers.iter().collect();
        let background = doc.canvas.background.as_deref();
        let tiles = now
            .par_iter()
            .map(|&i| {
                self.check()?;
                let tile = compose_items(
                    &doc,
                    snap.frame,
                    &rasters,
                    snap.onions.as_ref(),
                    &layers,
                    i,
                    background,
                    None,
                )?;
                Ok((i, tile.unwrap_or_else(|| vec![f16::ZERO; TILE_PX * 4])))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Done {
            ops: self.ops,
            tiles,
            changed,
            hidden: hidden.into_iter().collect(),
            exact,
        })
    }
}
