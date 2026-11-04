use crate::doc::{Cel, Document, Kind, Layer, LayerContent, Loc, MASK};
use crate::ops::Applied;
use crate::timing::{OPEN, ends};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retime {
    On(u32),
    Fit(u32),
    Reverse,
    Pingpong,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetimeSpec {
    pub from: u32,
    pub to: u32,
    pub action: Retime,
    pub layers: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Run {
    start: u32,
    end: u32,
    cel: Option<String>,
}

fn runs_of(cels: &[Cel]) -> Vec<Run> {
    let mut out = Vec::new();
    let mut at = 0;
    cels.iter().zip(ends(cels)).for_each(|(c, end)| {
        if c.frame > at {
            out.push(Run {
                start: at,
                end: c.frame,
                cel: None,
            });
        }
        out.push(Run {
            start: c.frame,
            end,
            cel: Some(c.id.clone()),
        });
        at = end;
    });
    if at != OPEN {
        out.push(Run {
            start: at,
            end: OPEN,
            cel: None,
        });
    }
    out
}

fn content_at(runs: &[Run], frame: u32) -> Option<&str> {
    runs.iter()
        .find(|r| r.start <= frame && frame < r.end)
        .and_then(|r| r.cel.as_deref())
}

fn leaf_paths(doc: &Document, keys: Option<&[String]>) -> Result<Vec<Vec<usize>>> {
    fn walk(layer: &Layer, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if layer.kind() != Kind::Group {
            out.push(path.clone());
        }
        if let Some(mask) = layer.mask.as_deref() {
            path.push(MASK);
            walk(mask, path, out);
            path.pop();
        }
        layer.children().iter().enumerate().for_each(|(i, c)| {
            path.push(i);
            walk(c, path, out);
            path.pop();
        });
    }
    let roots: Vec<Vec<usize>> = match keys {
        None => (0..doc.layers.len()).map(|i| vec![i]).collect(),
        Some(keys) => keys
            .iter()
            .map(|k| {
                let id = doc.find_layer(k)?.id.clone();
                match doc.locate(&id) {
                    Some(Loc::Layer(path)) => Ok(path),
                    _ => bail!("{k} is not a layer"),
                }
            })
            .collect::<Result<_>>()?,
    };
    let mut out = Vec::new();
    roots.iter().for_each(|root| {
        let mut path = root.clone();
        walk(doc.layer_at(root), &mut path, &mut out);
    });
    Ok(out.into_iter().collect::<BTreeSet<_>>().into_iter().collect())
}

fn order(action: Retime, starts: &[u32], from: u32, length: u32) -> Result<Vec<(usize, u32)>> {
    let k = starts.len();
    let len = |i: usize| starts.get(i + 1).copied().unwrap_or(from + length) - starts[i];
    Ok(match action {
        Retime::On(n) => (0..k).map(|i| (i, n)).collect(),
        Retime::Reverse => (0..k).rev().map(|i| (i, len(i))).collect(),
        Retime::Pingpong => (0..k)
            .chain((1..k.saturating_sub(1)).rev())
            .map(|i| (i, len(i)))
            .collect(),
        Retime::Fit(n) => {
            if (n as usize) < k {
                bail!(
                    "frames {from} to {} show {k} different pictures; fit them into at least {k} frames",
                    from + length - 1
                );
            }
            let mut placed: Vec<u32> = Vec::with_capacity(k + 1);
            (0..k).for_each(|i| {
                let ideal = ((starts[i] - from) as u64 * n as u64 + length as u64 / 2) / length as u64;
                let low = placed.last().map_or(0, |p| p + 1);
                let high = n - (k - i) as u32;
                placed.push((ideal as u32).clamp(low, high));
            });
            placed.push(n);
            (0..k).map(|i| (i, placed[i + 1] - placed[i])).collect()
        }
    })
}

fn rebuild(doc: &mut Document, path: &[usize], runs: Vec<Run>, out: &mut Applied) {
    let merged = runs.into_iter().fold(Vec::<Run>::new(), |mut acc, run| {
        match acc.last_mut() {
            Some(last) if last.cel == run.cel && last.end == run.start => last.end = run.end,
            _ if run.end > run.start => acc.push(run),
            _ => {}
        }
        acc
    });
    let old: BTreeMap<String, Cel> = doc
        .layer_at(path)
        .cels()
        .iter()
        .map(|c| (c.id.clone(), c.clone()))
        .collect();
    let shown: Vec<&Run> = merged.iter().filter(|r| r.cel.is_some()).collect();
    let mut seen = BTreeSet::new();
    let mut taken = doc.ids();
    let cels: Vec<Cel> = shown
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let id = run.cel.clone().unwrap_or_default();
            let next = shown.get(i + 1).map_or(OPEN, |r| r.start);
            let hold = (run.end != OPEN && run.end != next).then(|| run.end - run.start);
            let original = &old[&id];
            if seen.insert(id.clone()) {
                let kept = (original.hold.is_some() && run.end != OPEN).then(|| run.end - run.start);
                return Cel {
                    frame: run.start,
                    hold: kept.or(hold),
                    ..original.clone()
                };
            }
            let fresh = doc.alloc_id_in('c', &mut taken);
            out.added.push(fresh.clone());
            Cel {
                link: Some(original.link.clone().unwrap_or(id)),
                hold,
                ..Cel::at(&fresh, run.start)
            }
        })
        .collect();
    if let LayerContent::Paint { cels: list } | LayerContent::Vector { cels: list } =
        &mut doc.layer_at_mut(path).content
    {
        *list = cels;
    }
}

fn shifted(frame: u32, delta: i64) -> u32 {
    match frame {
        OPEN => OPEN,
        f => (f as i64 + delta).max(0) as u32,
    }
}

fn warp(frame: u32, from: u32, length: u32, new_length: u32) -> u32 {
    let end = from + length;
    match frame {
        f if f <= from => f,
        f if f >= end => f - length + new_length,
        f => from + (((f - from) as u64 * new_length as u64 + length as u64 / 2) / length as u64) as u32,
    }
}

pub fn retime(doc: &mut Document, spec: &RetimeSpec, out: &mut Applied) -> Result<String> {
    let (from, to) = (spec.from, spec.to);
    let frames = doc.timeline.frames;
    if from > to {
        bail!("retime frames run from {from} to {to}: the first frame comes after the last");
    }
    if to >= frames {
        bail!("frame {to} is past the end of the timeline ({frames} frames)");
    }
    match spec.action {
        Retime::On(0) => bail!("on must be at least 1"),
        Retime::Fit(0) => bail!("fit must be at least 1"),
        _ => {}
    }
    let paths = leaf_paths(doc, spec.layers.as_deref())?;
    let runs: Vec<Vec<Run>> = paths.iter().map(|p| runs_of(doc.layer_at(p).cels())).collect();
    let starts: Vec<u32> = (from..=to)
        .filter(|&f| f == from || runs.iter().any(|r| content_at(r, f) != content_at(r, f - 1)))
        .collect();
    let length = to + 1 - from;
    let segments = order(spec.action, &starts, from, length)?;
    let new_length: u32 = segments.iter().map(|(_, n)| n).sum();
    let delta = new_length as i64 - length as i64;
    paths.iter().zip(runs).for_each(|(path, runs)| {
        let before = runs.iter().filter(|r| r.start < from).map(|r| Run {
            end: r.end.min(from),
            ..r.clone()
        });
        let mut at = from;
        let middle: Vec<Run> = segments
            .iter()
            .map(|(i, n)| {
                let run = Run {
                    start: at,
                    end: at + n,
                    cel: content_at(&runs, starts[*i]).map(String::from),
                };
                at += n;
                run
            })
            .collect();
        let after = runs.iter().filter(|r| r.end == OPEN || r.end > to + 1).map(|r| Run {
            start: shifted(r.start.max(to + 1), delta),
            end: shifted(r.end, delta),
            cel: r.cel.clone(),
        });
        let all: Vec<Run> = before.chain(middle).chain(after).collect();
        rebuild(doc, path, all, out);
    });
    match &spec.layers {
        None => {
            doc.timeline.clips.iter_mut().for_each(|clip| {
                let (a, b) = (
                    warp(clip.from, from, length, new_length),
                    warp(clip.to + 1, from, length, new_length),
                );
                clip.from = a;
                clip.to = b.max(a + 1) - 1;
            });
            doc.timeline.frames = shifted(frames, delta).max(1);
        }
        Some(_) if delta > 0 => {
            let reach = paths
                .iter()
                .map(|p| {
                    let cels = doc.layer_at(p).cels();
                    cels.last().map_or(0, |c| match c.hold {
                        None => frames + delta as u32,
                        Some(h) => c.frame + h,
                    })
                })
                .max()
                .unwrap_or(0);
            doc.timeline.frames = frames.max(reach);
        }
        Some(_) => {}
    }
    out.changed.push("timeline".into());
    let what = match spec.action {
        Retime::On(n) => format!("on {n}s"),
        Retime::Fit(n) => format!("fitted into {n} frames"),
        Retime::Reverse => "reversed".into(),
        Retime::Pingpong => "ping-ponged".into(),
    };
    let shift = match delta {
        0 => String::new(),
        d => format!("; later frames moved by {d:+}"),
    };
    Ok(format!(
        "frames {from} to {to} ({} picture{}) {what}: now frames {from} to {}{shift}",
        starts.len(),
        if starts.len() == 1 { "" } else { "s" },
        from + new_length - 1
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Clip;
    use crate::ops::{Op, apply_all};
    use crate::timing::{Cell, cells};
    use serde_json::json;

    fn cel(id: &str, frame: u32, hold: Option<u32>, link: Option<&str>) -> Cel {
        Cel {
            hold,
            link: link.map(String::from),
            ..Cel::at(id, frame)
        }
    }

    fn doc(layers: Vec<Vec<Cel>>, frames: u32) -> Document {
        let mut doc = Document::new(64, 64);
        doc.timeline.frames = frames;
        doc.layers = layers
            .into_iter()
            .enumerate()
            .map(|(i, cels)| {
                let mut layer = Layer::paint(&format!("l{}", i + 1), &format!("layer {}", i + 1), "x");
                *layer.cels_mut().unwrap() = cels;
                layer
            })
            .collect();
        doc.next_id = 100;
        doc
    }

    fn sheet(doc: &Document, layer: &str) -> String {
        cells(doc.layer(layer).unwrap(), doc.timeline.frames)
            .iter()
            .map(|c| match c {
                Cell::Drawing { cel, start: true } => cel.trim_start_matches('c').to_string(),
                Cell::Link {
                    source, start: true, ..
                } => format!(">{}", source.trim_start_matches('c')),
                Cell::Drawing { .. } | Cell::Link { .. } => "|".into(),
                Cell::Blank => ".".into(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn run(doc: &Document, op: serde_json::Value) -> Result<(Document, Applied)> {
        apply_all(doc, &[Op::parse(op)?])
    }

    fn ones() -> Document {
        doc(
            vec![
                vec![
                    cel("c1", 0, Some(1), None),
                    cel("c2", 1, Some(1), None),
                    cel("c3", 2, Some(1), None),
                    cel("c4", 3, Some(1), None),
                    cel("c9", 4, None, None),
                ],
                vec![cel("c5", 0, None, None)],
            ],
            6,
        )
    }

    #[test]
    fn on_twos_and_back_keeps_every_layer_in_step() {
        let start = ones();
        assert_eq!(sheet(&start, "l1"), "1 2 3 4 9 |");
        let (twos, _) = run(&start, json!({"op":"retime","from":0,"to":3,"on":2})).unwrap();
        assert_eq!(sheet(&twos, "l1"), "1 | 2 | 3 | 4 | 9 |");
        assert_eq!(sheet(&twos, "l2"), "5 | | | | | | | | |");
        assert_eq!(twos.timeline.frames, 10);
        let (back, _) = run(&twos, json!({"op":"retime","from":0,"to":7,"on":1})).unwrap();
        assert_eq!(sheet(&back, "l1"), sheet(&start, "l1"));
        assert_eq!(back.layer("l1").unwrap().cels(), start.layer("l1").unwrap().cels());
        assert_eq!(back.timeline.frames, 6);
    }

    #[test]
    fn listed_layers_retime_alone_and_leave_clips_and_other_layers() {
        let mut start = ones();
        start.timeline.clips = vec![Clip::new("walk", 0, 3, true)];
        let (twos, _) = run(&start, json!({"op":"retime","from":0,"to":3,"on":2,"layers":["l1"]})).unwrap();
        assert_eq!(sheet(&twos, "l1"), "1 | 2 | 3 | 4 | 9 |");
        assert_eq!(sheet(&twos, "l2"), "5 | | | | | | | | |");
        assert_eq!(twos.timeline.clips[0], Clip::new("walk", 0, 3, true));
        let (all, _) = run(&start, json!({"op":"retime","from":0,"to":3,"on":2})).unwrap();
        assert_eq!((all.timeline.clips[0].from, all.timeline.clips[0].to), (0, 7));
    }

    #[test]
    fn fit_keeps_order_and_relative_timing_with_at_least_a_frame_each() {
        let start = doc(
            vec![vec![
                cel("c1", 0, Some(4), None),
                cel("c2", 4, Some(2), None),
                cel("c3", 6, Some(2), None),
                cel("c4", 8, None, None),
            ]],
            10,
        );
        let (fit, _) = run(&start, json!({"op":"retime","from":0,"to":7,"fit":4})).unwrap();
        assert_eq!(sheet(&fit, "l1"), "1 | 2 3 4 |");
        let (tight, _) = run(&start, json!({"op":"retime","from":0,"to":7,"fit":3})).unwrap();
        assert_eq!(sheet(&tight, "l1"), "1 2 3 4 |");
        let err = run(&start, json!({"op":"retime","from":0,"to":7,"fit":2})).unwrap_err();
        assert!(format!("{err:#}").contains("at least 3 frames"), "{err:#}");
        let (long, _) = run(&start, json!({"op":"retime","from":0,"to":7,"fit":16})).unwrap();
        assert_eq!(sheet(&long, "l1"), "1 | | | | | | | 2 | | | 3 | | | 4 |");
    }

    #[test]
    fn reverse_and_pingpong_reuse_drawings_through_links() {
        let start = ones();
        let (back, _) = run(&start, json!({"op":"retime","from":0,"to":3,"reverse":true})).unwrap();
        assert_eq!(sheet(&back, "l1"), "4 3 2 1 9 |");
        let (pp, applied) = run(&start, json!({"op":"retime","from":0,"to":3,"pingpong":true})).unwrap();
        assert_eq!(sheet(&pp, "l1"), "1 2 3 4 >3 >2 9 |");
        assert_eq!(applied.added.len(), 2);
        assert_eq!(pp.timeline.frames, 8);
        assert_eq!(sheet(&pp, "l2"), "5 | | | | | | |");
    }

    #[test]
    fn exposures_crossing_the_range_split_into_links_when_they_move() {
        let start = doc(
            vec![vec![
                cel("c1", 0, Some(3), None),
                cel("c2", 3, Some(1), None),
                cel("c3", 4, Some(2), Some("c1")),
            ]],
            6,
        );
        let (back, _) = run(&start, json!({"op":"retime","from":2,"to":5,"reverse":true})).unwrap();
        assert_eq!(sheet(&back, "l1"), "1 | >1 | 2 >1");
        back.validate().unwrap();
    }
}
