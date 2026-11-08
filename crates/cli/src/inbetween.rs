use crate::colour::needs_vectors;
use crate::layers::{self, Vectors};
use anyhow::{Result, anyhow, bail};
use klad_brush::{BrushSet, Raster, accepts, flood, nearest_accepted, paint_fill, polygon, replay_onto};
use klad_core::bundle::Bundle;
use klad_core::doc::{Cel, Document, Kind, Layer, Loc, Tween};
use klad_core::fill::{FillRef, FillSource};
use klad_core::markedit::address;
use klad_core::ops::{InbetweenSpec, Inbetweens, Op, TweenCel, apply_all};
use klad_core::stroke::Mark;
use klad_core::tween::{self, Anchor, Lines, Match, Plan, Unpaired, made_hash};
use klad_render::{Rasters, fill_reference};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

struct Key {
    layer: String,
    name: String,
    a: String,
    b: String,
    a_frame: u32,
    plan: Plan,
    parts: Vec<String>,
    unpaired: Unpaired,
}

struct Kept {
    index: usize,
    hand: Vec<Mark>,
    before: Vec<Mark>,
    parts: BTreeMap<String, Vec<u32>>,
}

fn parts_of(doc: &Document, cel: &str) -> BTreeMap<String, Vec<u32>> {
    match doc.locate(cel) {
        Some(Loc::Cel(path, c)) => doc.layer_at(&path).cels()[c].parts.clone(),
        _ => BTreeMap::new(),
    }
}

type Group = (Vec<u32>, Vec<u32>);
type Line = Vec<(f32, f32)>;

fn shared_parts(doc: &Document, a: &str, b: &str) -> (Vec<String>, Vec<Group>) {
    let (pa, pb) = (parts_of(doc, a), parts_of(doc, b));
    pa.iter()
        .filter_map(|(name, xs)| pb.get(name).map(|ys| (name.clone(), (xs.clone(), ys.clone()))))
        .unzip()
}

fn key(
    bundle: &Bundle,
    layer: &Layer,
    (a, b): (&str, &str),
    a_frame: u32,
    (how, pairs, unpaired): (Match, &[[u32; 2]], Unpaired),
) -> Result<Key> {
    let (names, groups) = shared_parts(&bundle.doc, a, b);
    Ok(Key {
        layer: layer.id.clone(),
        name: layer.name.clone(),
        a: a.to_string(),
        b: b.to_string(),
        a_frame,
        plan: tween::plan_parts(
            &bundle.read_marks(&layer.id, a)?,
            &bundle.read_marks(&layer.id, b)?,
            how,
            pairs,
            &groups,
        ),
        parts: names,
        unpaired,
    })
}

fn kept_on_top(made: &mut [TweenCel], kept: Vec<Kept>, notes: &mut Vec<String>) {
    kept.into_iter().for_each(|k| {
        let cel = &mut made[k.index];
        let old: BTreeMap<u32, &Mark> = k.before.iter().map(|m| (m.id(), m)).collect();
        let aligned: Vec<(u32, Line, Line)> = cel
            .marks
            .iter()
            .filter_map(Mark::as_stroke)
            .filter_map(|s| {
                let before = old.get(&s.id)?.as_stroke()?;
                Some((s.id, tween::line_of(before), tween::line_of(s)))
            })
            .collect();
        let ids: Vec<u32> = aligned.iter().map(|a| a.0).collect();
        let (from, to): (Lines, Lines) = aligned.into_iter().map(|(_, a, b)| (a, b)).unzip();
        let mut next = cel.marks.iter().map(Mark::id).max().unwrap_or(0) + 1;
        let count = k.hand.len();
        k.hand.into_iter().for_each(|hand| {
            let part = k
                .parts
                .iter()
                .find(|(_, list)| list.contains(&hand.id()))
                .map(|(n, _)| n.clone());
            let within: Option<Vec<usize>> = part.as_ref().map(|name| {
                let members = &k.parts[name];
                ids.iter()
                    .enumerate()
                    .filter(|(_, id)| members.contains(id))
                    .map(|(i, _)| i)
                    .collect()
            });
            let within = within.filter(|w| !w.is_empty());
            let mut moved = match &hand {
                Mark::Fill(f) if matches!(f.source, FillSource::Seed { .. }) => hand.clone(),
                _ => tween::carry(&hand, &from, &to, within.as_deref()),
            };
            moved.set_id(next);
            if let Some(name) = part {
                cel.parts.entry(name).or_default().push(next);
            }
            next += 1;
            cel.marks.push(moved);
        });
        notes.push(format!(
            "  {}: {count} mark{} drawn by hand kept on top",
            cel.cel,
            if count == 1 { "" } else { "s" }
        ));
    });
}

fn keep_parts(doc: &Document, made: &mut [TweenCel]) {
    made.iter_mut().for_each(|c| {
        let ids: BTreeSet<u32> = c.marks.iter().map(Mark::id).collect();
        c.parts = parts_of(doc, &c.tween.from)
            .into_iter()
            .map(|(name, list)| (name, list.into_iter().filter(|i| ids.contains(i)).collect::<Vec<_>>()))
            .filter(|(_, list)| !list.is_empty())
            .collect();
    });
}

struct Job {
    key: usize,
    index: usize,
    frame: u32,
    t: f32,
}

struct Done {
    index: usize,
    marks: Vec<Mark>,
    failed: Vec<String>,
}

fn source(cel: &Cel) -> String {
    cel.link.clone().unwrap_or_else(|| cel.id.clone())
}

fn paint_layers(doc: &Document, keys: &[String]) -> Result<Vec<String>> {
    fn walk(layer: &Layer, out: &mut Vec<String>) -> Result<()> {
        match layer.kind() {
            Kind::Paint => {
                out.push(layer.id.clone());
                Ok(())
            }
            Kind::Group => layer.children().iter().try_for_each(|c| walk(c, out)),
            Kind::Vector => bail!("{} is a vector layer; in-betweens are made on paint layers", layer.id),
            Kind::Adjust => bail!(
                "{} is an adjustment layer; in-betweens are made on paint layers",
                layer.id
            ),
        }
    }
    let mut out = Vec::new();
    keys.iter().try_for_each(|k| walk(doc.find_layer(k)?, &mut out))?;
    let mut seen = BTreeSet::new();
    out.retain(|id| seen.insert(id.clone()));
    Ok(out)
}

fn spread(from: u32, to: u32, count: u32) -> Vec<u32> {
    let span = (to - from) as u64;
    (1..=count as u64)
        .map(|i| from + ((2 * i * span + count as u64 + 1) / (2 * (count as u64 + 1))) as u32)
        .collect()
}

fn describe(t: f32) -> String {
    format!("{:.2}", t)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn tween_of(key: &Key, t: f32, spec: &InbetweenSpec) -> Tween {
    Tween {
        from: key.a.clone(),
        to: key.b.clone(),
        t,
        matching: match spec.matching {
            Match::Id => String::new(),
            other => other.name().to_string(),
        },
        pairs: spec.pairs.clone(),
        made: String::new(),
        unpaired: spec.unpaired.name(),
    }
}

fn pairing_notes(key: &Key, notes: &mut Vec<String>) {
    let p = &key.plan.pairing;
    let mut line = format!(
        "{}: {} -> {}, {} marks paired ({} flood fills)",
        key.name,
        key.a,
        key.b,
        p.pairs.len(),
        key.plan.floods().len()
    );
    if !key.parts.is_empty() {
        line.push_str(&format!(", by part first: {}", key.parts.join(", ")));
    }
    let touching = key.plan.touching_ends();
    if touching > 0 {
        line.push_str(&format!(", {touching} line ends kept touching"));
    }
    notes.push(line);
    let alone: Vec<String> = p
        .alone_a
        .iter()
        .map(|id| format!("{} (only in the first key)", address(&key.a, *id)))
        .chain(
            p.alone_b
                .iter()
                .map(|id| format!("{} (only in the second key)", address(&key.b, *id))),
        )
        .chain(p.clashes.iter().map(|(x, y)| {
            format!(
                "{} and {} (one is a stroke or lasso fill, the other not)",
                address(&key.a, *x),
                address(&key.b, *y)
            )
        }))
        .collect();
    if !alone.is_empty() {
        let how = match key.unpaired {
            Unpaired::Leave => "left out".to_string(),
            other => format!("{} (flood fills left out)", other.name()),
        };
        notes.push(format!("  unpaired, {how}: {}", alone.join(", ")));
    }
}

pub fn resolve(bundle: &Bundle, ops: &mut [Op]) -> Result<Vec<String>> {
    if !ops.iter().any(|op| matches!(op, Op::Inbetween(_))) {
        return Ok(Vec::new());
    }
    if ops.len() > 1 {
        bail!("an inbetween op needs a batch of its own: it reads its keys from the bundle as it is");
    }
    let Op::Inbetween(spec) = &mut ops[0] else {
        unreachable!("checked above");
    };
    if spec.resolved.is_some() {
        return Ok(Vec::new());
    }
    let start = Instant::now();
    let (made, mut notes) = match spec.redo {
        true => redo(bundle, spec)?,
        false => make(bundle, spec)?,
    };
    let frames: BTreeSet<u32> = made.cels.iter().map(|c| c.frame).collect();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    notes.push(format!(
        "{} in-between{} on {} frame{} in {:.0} ms ({:.0} ms per frame)",
        made.cels.len(),
        if made.cels.len() == 1 { "" } else { "s" },
        frames.len(),
        if frames.len() == 1 { "" } else { "s" },
        ms,
        ms / frames.len().max(1) as f64
    ));
    spec.resolved = Some(made);
    Ok(notes)
}

fn make(bundle: &Bundle, spec: &InbetweenSpec) -> Result<(Inbetweens, Vec<String>)> {
    let doc = &bundle.doc;
    let Some(from) = spec.from else {
        bail!("give the frames of the two keys: --from and --to");
    };
    let span = doc
        .timeline
        .clip_at(from)
        .map_or((0, doc.timeline.frames), |c| (c.from, c.to + 1));
    let to = match (spec.to, spec.looping) {
        (Some(to), _) => to,
        (None, true) => span.1,
        (None, false) => bail!("give the frames of the two keys: --from and --to"),
    };
    if to <= from + 1 {
        bail!("there is no frame between the keys at {from} and {to}");
    }
    let room = to - from - 1;
    let count = match (spec.count, spec.spacing.count()) {
        (Some(c), Some(n)) if c as usize != n => {
            bail!("--count {c} and {n} spacing values disagree; give one of them")
        }
        (Some(c), _) => c,
        (None, Some(n)) => n as u32,
        (None, None) => room,
    };
    if count > room {
        bail!(
            "{count} in-betweens do not fit between frames {from} and {to}: there are {room} frames; give --count {room} or less"
        );
    }
    let frames = spread(from, to, count);
    let ts = spec.spacing.ts(count as usize);
    let mut staged = doc.clone();
    let mut taken = staged.ids();
    let mut keys = Vec::new();
    let mut made = Inbetweens::default();
    let mut notes = Vec::new();
    let mut jobs = Vec::new();
    let mut rigged: Vec<(usize, crate::rig::Posed, f32)> = Vec::new();
    paint_layers(doc, &spec.layers)?.into_iter().try_for_each(|layer| {
        let owner = doc.layer(&layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
        let name = owner.name.clone();
        let at_a = owner
            .cel_at(from)
            .ok_or_else(|| anyhow!("{name} shows nothing at frame {from}, so it has no first key there"))?;
        let at_b = match spec.looping {
            true => owner.cel_at(span.0).ok_or_else(|| {
                anyhow!(
                    "{name} shows nothing at frame {}, the loop's first frame, so the loop has no first drawing",
                    span.0
                )
            })?,
            false => owner
                .cel_at(to)
                .ok_or_else(|| anyhow!("{name} shows nothing at frame {to}, so it has no second key there"))?,
        };
        if !spec.looping && at_b.frame != to {
            bail!(
                "frame {to} of {name} shows {} from frame {}: the second key is the frame where a drawing starts (--to {})",
                at_b.id,
                at_b.frame,
                at_b.frame
            );
        }
        let (a, b) = (source(at_a), source(at_b));
        if a == b {
            notes.push(format!("{name} shows {a} at both keys; left as it is"));
            return Ok(());
        }
        let inside: Vec<&Cel> = owner.cels().iter().filter(|c| c.frame > from && c.frame < to).collect();
        let foreign: Vec<&str> = inside
            .iter()
            .filter(|c| !c.tween.as_ref().is_some_and(|t| t.from == a && t.to == b))
            .map(|c| c.id.as_str())
            .collect();
        if !foreign.is_empty() && !spec.force {
            bail!(
                "frames {} to {} of {name} already hold {}, which are not in-betweens of {a} and {b}; remove them, or --force replaces them",
                from + 1,
                to - 1,
                foreign.join(", ")
            );
        }
        let changed: Vec<String> = inside
            .iter()
            .filter(|c| !foreign.contains(&c.id.as_str()))
            .map(|c| Ok((c, bundle.read_marks(&layer, &c.id)?)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|(c, marks)| c.tween.as_ref().is_some_and(|t| made_hash(marks) != t.made))
            .map(|(c, _)| c.id.clone())
            .collect();
        if !changed.is_empty() && !spec.force {
            bail!(
                "{} on {name} were changed after they were made (drawn on or edited); remove them, or --force replaces them",
                changed.join(", ")
            );
        }
        made.removed.extend(inside.iter().map(|c| c.id.clone()));
        made.holds.push((at_a.id.clone(), frames[0] - at_a.frame));
        let key = key(
            bundle,
            owner,
            (&a, &b),
            from,
            (spec.matching, &spec.pairs, spec.unpaired),
        )?;
        let on_bones = crate::rig::posed_pair(bundle, &layer, &a, &b)?;
        frames.iter().zip(&ts).enumerate().for_each(|(i, (&frame, &t))| {
            let next = frames.get(i + 1).copied().unwrap_or(to);
            match &on_bones {
                Some(rig) => rigged.push((made.cels.len(), rig.clone(), t)),
                None => jobs.push(Job {
                    key: keys.len(),
                    index: made.cels.len(),
                    frame,
                    t,
                }),
            }
            made.cels.push(TweenCel {
                layer: layer.clone(),
                cel: staged.alloc_id_in('c', &mut taken),
                frame,
                hold: Some(next - frame),
                tween: tween_of(&key, t, spec),
                marks: Vec::new(),
                fresh: true,
                parts: BTreeMap::new(),
                pose: None,
            });
        });
        keys.push(key);
        Ok(())
    })?;
    if made.cels.is_empty() {
        bail!("nothing to in-between: every layer shows the same drawing at both keys");
    }
    let (staged, _) = apply_all(
        doc,
        &[Op::Inbetween(InbetweenSpec {
            resolved: Some(made.clone()),
            ..spec.clone()
        })],
    )?;
    keys.iter()
        .filter(|k| !rigged.iter().any(|(i, _, _)| made.cels[*i].layer == k.layer))
        .for_each(|k| pairing_notes(k, &mut notes));
    if !jobs.is_empty() {
        compute(bundle, &staged, &keys, &jobs, &mut made.cels, &mut notes)?;
    }
    rigged
        .iter()
        .for_each(|(i, rig, t)| on_bones(rig, *t, (doc.canvas.width, doc.canvas.height), &mut made.cels[*i]));
    bones_note(rigged.iter().map(|(i, _, _)| made.cels[*i].layer.as_str()), &mut notes);
    keep_parts(doc, &mut made.cels);
    made.cels.iter().for_each(|c| {
        notes.push(format!(
            "  {} at frame {} (t {}) on {}",
            c.cel,
            c.frame,
            describe(c.tween.t),
            c.layer
        ))
    });
    Ok((made, notes))
}

fn finish(cel: &mut TweenCel, marks: Vec<Mark>) {
    cel.tween.made = made_hash(&marks);
    cel.marks = marks;
}

fn on_bones(rig: &crate::rig::Posed, t: f32, canvas: (u32, u32), cel: &mut TweenCel) {
    let (marks, pose) = rig.at(t, canvas);
    finish(cel, marks);
    cel.pose = Some(pose);
}

fn bones_note<'a>(layers: impl Iterator<Item = &'a str>, notes: &mut Vec<String>) {
    let mut bent: Vec<&str> = layers.collect();
    bent.dedup();
    bent.iter().for_each(|l| {
        notes.push(format!(
            "{l}: in-betweened on bones (both keys are poses of one rest drawing)"
        ))
    });
}

type Rigs = BTreeMap<(String, String, String), Option<crate::rig::Posed>>;

fn redo(bundle: &Bundle, spec: &InbetweenSpec) -> Result<(Inbetweens, Vec<String>)> {
    let doc = &bundle.doc;
    let wanted: Option<Vec<String>> = (!spec.layers.is_empty())
        .then(|| paint_layers(doc, &spec.layers))
        .transpose()?;
    let mut keys: Vec<Key> = Vec::new();
    let mut made = Inbetweens::default();
    let mut jobs = Vec::new();
    let mut refused = Vec::new();
    let mut notes = Vec::new();
    let mut kept = Vec::new();
    let mut rigs: Rigs = BTreeMap::new();
    let mut rigged: Vec<(usize, (String, String, String), f32)> = Vec::new();
    doc.all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Paint)
        .filter(|l| wanted.as_ref().is_none_or(|w| w.contains(&l.id)))
        .flat_map(|l| l.cels().iter().map(move |c| (l, c)))
        .filter(|(_, c)| spec.from.is_none_or(|f| c.frame >= f) && spec.to.is_none_or(|t| c.frame <= t))
        .filter(|(_, c)| spec.only.is_empty() || spec.only.contains(&c.id))
        .try_for_each(|(layer, cel)| {
            let Some(tween) = &cel.tween else {
                return Ok(());
            };
            let drawing = |id: &str| layer.cels().iter().find(|c| c.id == id && c.link.is_none());
            let (Some(first), Some(_)) = (drawing(&tween.from), drawing(&tween.to)) else {
                bail!(
                    "{} is an in-between of {} and {}, but {} has no such drawing any more",
                    cel.id,
                    tween.from,
                    tween.to,
                    layer.name
                );
            };
            let marks = bundle.read_marks(&layer.id, &cel.id)?;
            let human = |m: &Mark| m.author() == klad_core::stroke::Author::Human;
            let keep_drawn = spec.keep_drawn || spec.keep_cels.contains(&cel.id);
            let force = spec.force || spec.force_cels.contains(&cel.id);
            if made_hash(&marks) != tween.made && keep_drawn {
                let (hand, before): (Vec<Mark>, Vec<Mark>) = marks.iter().cloned().partition(|m| human(m));
                if made_hash(&before) != tween.made {
                    notes.push(format!(
                        "  {}: marks the agent made were edited too; they are made again from the keys",
                        cel.id
                    ));
                }
                kept.push(Kept {
                    index: made.cels.len(),
                    hand,
                    before,
                    parts: cel.parts.clone(),
                });
            } else if made_hash(&marks) != tween.made && !force {
                let hand = marks.iter().filter(|m| human(m)).count();
                refused.push(match hand {
                    0 => format!("{} (changed since it was made)", cel.id),
                    n => format!("{} ({n} mark{} drawn by hand)", cel.id, if n == 1 { "" } else { "s" }),
                });
                return Ok(());
            }
            let pair = (layer.id.clone(), tween.from.clone(), tween.to.clone());
            if !rigs.contains_key(&pair) {
                let rig = crate::rig::posed_pair(bundle, &layer.id, &tween.from, &tween.to)?;
                rigs.insert(pair.clone(), rig);
            }
            if rigs[&pair].is_some() {
                rigged.push((made.cels.len(), pair, tween.t));
                made.cels.push(TweenCel {
                    layer: layer.id.clone(),
                    cel: cel.id.clone(),
                    frame: cel.frame,
                    hold: cel.hold,
                    tween: tween.clone(),
                    marks: Vec::new(),
                    fresh: false,
                    pose: None,
                    parts: BTreeMap::new(),
                });
                return Ok(());
            }
            let pairs = tween.pairs.clone();
            let matching = Match::parse(&tween.matching)?;
            let unpaired = Unpaired::parse(&tween.unpaired)?;
            let at = match keys
                .iter()
                .position(|k| k.layer == layer.id && k.a == tween.from && k.b == tween.to)
            {
                Some(at) => at,
                None => {
                    keys.push(key(
                        bundle,
                        layer,
                        (&tween.from, &tween.to),
                        first.frame,
                        (matching, &pairs, unpaired),
                    )?);
                    keys.len() - 1
                }
            };
            jobs.push(Job {
                key: at,
                index: made.cels.len(),
                frame: cel.frame,
                t: tween.t,
            });
            made.cels.push(TweenCel {
                layer: layer.id.clone(),
                cel: cel.id.clone(),
                frame: cel.frame,
                hold: cel.hold,
                tween: tween.clone(),
                marks: Vec::new(),
                fresh: false,
                pose: None,
                parts: BTreeMap::new(),
            });
            Ok(())
        })?;
    if !refused.is_empty() {
        bail!(
            "these in-betweens were changed after they were made, so --redo leaves them: {}; --keep-drawn remakes them and keeps the marks drawn by hand on top, --force remakes them all",
            refused.join(", ")
        );
    }
    if made.cels.is_empty() {
        bail!("no in-betweens to redo here (klad inbetween marks the cels it makes; klad frames shows them)");
    }
    keys.iter().for_each(|k| pairing_notes(k, &mut notes));
    if !jobs.is_empty() {
        compute(bundle, doc, &keys, &jobs, &mut made.cels, &mut notes)?;
    }
    let canvas = (doc.canvas.width, doc.canvas.height);
    rigged.iter().for_each(|(i, pair, t)| {
        if let Some(rig) = &rigs[pair] {
            on_bones(rig, *t, canvas, &mut made.cels[*i]);
        }
    });
    bones_note(rigged.iter().map(|(i, _, _)| made.cels[*i].layer.as_str()), &mut notes);
    keep_parts(doc, &mut made.cels);
    kept_on_top(&mut made.cels, kept, &mut notes);
    Ok((made, notes))
}

fn depends(keys: &[Key], jobs: &[&Job], j: usize, k: usize) -> bool {
    let key = &keys[jobs[j].key];
    let other = &keys[jobs[k].key].layer;
    key.plan.floods().iter().any(|(a, _)| match a.reference() {
        Some(FillRef::Layer(id)) => id == other,
        Some(FillRef::All) => true,
        _ => false,
    })
}

fn ordered<'a>(keys: &[Key], jobs: Vec<&'a Job>) -> Vec<&'a Job> {
    let mut left: Vec<usize> = (0..jobs.len()).collect();
    let mut out = Vec::new();
    while !left.is_empty() {
        let ready = left
            .iter()
            .position(|&j| {
                !left
                    .iter()
                    .any(|&k| k != j && depends(keys, &jobs, j, k) && !depends(keys, &jobs, k, j))
            })
            .unwrap_or(0);
        out.push(jobs[left.remove(ready)]);
    }
    out
}

fn accept_colours(bundle: &Bundle, keys: &[Key], brushes: &BrushSet) -> Result<BTreeMap<(usize, u32), [f32; 4]>> {
    keys.iter()
        .enumerate()
        .filter(|(_, k)| !k.plan.floods().is_empty())
        .map(|(i, key)| {
            let ids: Vec<u32> = key.plan.floods().iter().map(|(a, _)| a.id).collect();
            let marks = bundle.read_marks(&key.layer, &key.a)?;
            let found = crate::colour::seed_colours(bundle, &key.a, key.a_frame, &marks, &ids, brushes)?;
            Ok(found.into_iter().map(move |(id, c)| ((i, id), c)))
        })
        .collect::<Result<Vec<_>>>()
        .map(|v| v.into_iter().flatten().collect())
}

fn lines_now(marks: &[Mark]) -> Lines {
    marks.iter().filter_map(Mark::as_stroke).map(tween::line_of).collect()
}

fn compute(
    bundle: &Bundle,
    staged: &Document,
    keys: &[Key],
    jobs: &[Job],
    cels: &mut [TweenCel],
    notes: &mut Vec<String>,
) -> Result<()> {
    let brushes = BrushSet::for_bundle(bundle)?;
    let accept = accept_colours(bundle, keys, &brushes)?;
    let view = Bundle {
        root: bundle.root.clone(),
        doc: staged.clone(),
    };
    let frames: BTreeSet<u32> = jobs.iter().map(|j| j.frame).collect();
    let ids: Vec<String> = cels.iter().map(|c| c.cel.clone()).collect();
    let done = frames
        .par_iter()
        .map(|&frame| {
            let here: Vec<&Job> = jobs.iter().filter(|j| j.frame == frame).collect();
            frame_job(&view, &brushes, keys, ordered(keys, here), frame, &ids, &accept)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut failed = Vec::new();
    done.into_iter().flatten().for_each(|d| {
        finish(&mut cels[d.index], d.marks);
        failed.extend(d.failed);
    });
    if !failed.is_empty() {
        notes.push(format!("  fills left out: {}", failed.join("; ")));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn frame_job(
    view: &Bundle,
    brushes: &BrushSet,
    keys: &[Key],
    jobs: Vec<&Job>,
    frame: u32,
    ids: &[String],
    accept: &BTreeMap<(usize, u32), [f32; 4]>,
) -> Result<Vec<Done>> {
    let doc = &view.doc;
    let (w, h) = (doc.canvas.width, doc.canvas.height);
    let mut rasters: Rasters = layers::load(view, frame, brushes, true)?.rasters;
    if jobs.iter().any(|j| {
        keys[j.key]
            .plan
            .floods()
            .iter()
            .any(|(a, _)| needs_vectors(doc, a.reference()))
    }) {
        Vectors::default().add_to(doc, frame, &mut rasters)?;
    }
    let mut lines_at: BTreeMap<String, (Lines, Lines, Lines)> = BTreeMap::new();
    jobs.iter()
        .map(|job| {
            let key = &keys[job.key];
            let cel = &ids[job.index];
            let mut marks = key.plan.at(job.t);
            let (la, lb) = key.plan.key_lines();
            let now = lines_now(&marks);
            let extra = key.plan.unpaired_at(job.t, key.unpaired, &now);
            lines_at.insert(key.layer.clone(), (la, lb, now));
            let mut own = Raster::new(w, h);
            let mut failed = Vec::new();
            let mut keep = vec![true; marks.len()];
            let floods = key.plan.floods();
            (0..marks.len()).try_for_each(|m| {
                if marks[m].as_stroke().is_some() {
                    replay_onto(&mut own, std::slice::from_ref(&marks[m]), brushes)?;
                    return Ok::<_, anyhow::Error>(());
                }
                match &mut marks[m] {
                    Mark::Stroke(_) => {}
                    Mark::Fill(fill) => {
                        let id = fill.id;
                        let what = |why: String| format!("{} at frame {frame}: {why}", address(&key.a, id));
                        match fill.source.clone() {
                            FillSource::Polygon(points) => fill.coverage = polygon(&points, w, h),
                            FillSource::Seed { mut seed, reference } => {
                                let Some((a, b)) = floods.iter().find(|(a, _)| a.id == id) else {
                                    return Ok(());
                                };
                                rasters.insert(cel.clone(), own.clone());
                                let image = fill_reference(doc, frame, &rasters, cel, &reference)?;
                                let lines = match &reference {
                                    FillRef::This => lines_at.get(&key.layer).cloned(),
                                    FillRef::Layer(other) => lines_at.get(other).cloned(),
                                    FillRef::All => Some(lines_at.values().fold(
                                        (Vec::new(), Vec::new(), Vec::new()),
                                        |(mut x, mut y, mut z), (a, b, c)| {
                                            x.extend(a.iter().cloned());
                                            y.extend(b.iter().cloned());
                                            z.extend(c.iter().cloned());
                                            (x, y, z)
                                        },
                                    )),
                                };
                                let (sa, sb) = match (&a.source, &b.source) {
                                    (FillSource::Seed { seed: x, .. }, FillSource::Seed { seed: y, .. }) => {
                                        ((x.x, x.y), (y.x, y.y))
                                    }
                                    _ => unreachable!("floods pair with floods"),
                                };
                                let carried = lines.and_then(|(la, lb, lt)| {
                                    let at = |anchor: Option<Anchor>| anchor.and_then(|an| tween::place(&an, &lt));
                                    Some((at(tween::anchor(&la, sa))?, at(tween::anchor(&lb, sb))?))
                                });
                                let t = job.t;
                                let ((ax, ay), (bx, by)) = carried.unwrap_or((sa, sb));
                                seed.x = ax + (bx - ax) * t;
                                seed.y = ay + (by - ay) * t;
                                let colour = accept.get(&(job.key, id)).copied().unwrap_or([0.0; 4]);
                                let reach = 12 + seed.gap.ceil() as u32;
                                if !accepts(&image, seed.x, seed.y, colour, seed.tolerance) {
                                    match nearest_accepted(&image, [seed.x, seed.y], colour, seed.tolerance, reach) {
                                        Some([x, y]) => (seed.x, seed.y) = (x, y),
                                        None => {
                                            failed.push(what("its seed lands on a line".into()));
                                            keep[m] = false;
                                            return Ok(());
                                        }
                                    }
                                }
                                fill.source = FillSource::Seed {
                                    seed,
                                    reference: reference.clone(),
                                };
                                fill.coverage = flood(&image, &seed);
                                let px = fill.coverage.pixels() as f32;
                                let (pa, pb) = (a.coverage.pixels() as f32, b.coverage.pixels() as f32);
                                let expected = pa + (pb - pa) * t;
                                if px > expected * 1.6 + 64.0 {
                                    failed.push(what(format!(
                                        "it leaked ({px:.0} px where the keys have {pa:.0} and {pb:.0})"
                                    )));
                                    keep[m] = false;
                                    return Ok(());
                                }
                                if px < expected * 0.4 {
                                    failed.push(what(format!(
                                        "it found a smaller region ({px:.0} px where the keys have {pa:.0} and {pb:.0})"
                                    )));
                                    keep[m] = false;
                                    return Ok(());
                                }
                            }
                        }
                        if fill.coverage.is_empty() {
                            failed.push(what("it covers no pixels".into()));
                            keep[m] = false;
                            return Ok(());
                        }
                        paint_fill(&mut own, fill);
                    }
                }
                Ok::<_, anyhow::Error>(())
            })?;
            let mut kept: Vec<Mark> = marks
                .into_iter()
                .zip(keep)
                .filter(|(_, k)| *k)
                .map(|(m, _)| m)
                .collect();
            let mut next = kept.iter().chain(&extra).map(Mark::id).max().unwrap_or(0) + 1;
            let taken: BTreeSet<u32> = kept.iter().map(Mark::id).collect();
            extra.into_iter().try_for_each(|mut m| {
                if taken.contains(&m.id()) || key.plan.pairing.alone_b.contains(&m.id()) {
                    m.set_id(next);
                    next += 1;
                }
                if let Mark::Fill(f) = &mut m
                    && let FillSource::Polygon(points) = &f.source
                {
                    f.coverage = polygon(points, w, h);
                    paint_fill(&mut own, f);
                } else {
                    replay_onto(&mut own, std::slice::from_ref(&m), brushes)?;
                }
                kept.push(m);
                Ok::<_, anyhow::Error>(())
            })?;
            rasters.insert(cel.clone(), own);
            Ok(Done {
                index: job.index,
                marks: kept,
                failed,
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::Output;
    use klad_core::doc::Layer;
    use klad_core::ops::parse_lines;
    use klad_core::stroke::Stroke;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-tween-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn quiet() -> Output {
        Output {
            json: false,
            render: false,
            scale: None,
            frame: 0,
            overlay: None,
        }
    }

    fn line(points: &[(f32, f32)], size: f32) -> String {
        let path: Vec<String> = points.iter().map(|(x, y)| format!("{x} {y}")).collect();
        format!(
            r##"{{"op":"stroke","layer":"lines","brush":"ink","size":{size},"color":"#202020","taper":[0,0],"path":"M{}"}}"##,
            path.join(" L")
        )
    }

    pub(crate) fn swing(name: &str) -> std::path::PathBuf {
        let dir = scratch(name);
        let mut doc = Document::new(320, 320);
        doc.timeline.frames = 8;
        let mut colour = Layer::paint("colour", "colour", "col0");
        colour.cels_mut().unwrap()[0].hold = Some(6);
        let mut lines = Layer::paint("lines", "lines", "ln0");
        lines.cels_mut().unwrap()[0].hold = Some(6);
        doc.layers = vec![colour, lines];
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let (x, y) = (80.0, 100.0);
        let text = [
            line(&[(x, y - 16.0), (x + 150.0, y - 16.0)], 4.0),
            line(&[(x, y + 16.0), (x + 150.0, y + 16.0)], 4.0),
            line(&[(x, y - 16.0), (x, y + 16.0)], 4.0),
            line(&[(x + 150.0, y - 16.0), (x + 150.0, y + 16.0)], 4.0),
            line(
                &[
                    (x + 150.0, y - 16.0),
                    (x + 180.0, y - 10.0),
                    (x + 180.0, y + 10.0),
                    (x + 150.0, y + 16.0),
                ],
                4.0,
            ),
            r##"{"op":"fill","layer":"colour","at":[120,100],"ref":"lines","gap":3,"color":"#d04030"}"##.to_string(),
            r##"{"op":"fill","layer":"colour","at":[245,100],"ref":"lines","gap":3,"color":"#f0e0c0"}"##.to_string(),
            r#"{"op":"cel","dup":"ln0","frame":6,"id":"ln6"}"#.to_string(),
            r#"{"op":"cel","dup":"col0","frame":6,"id":"col6"}"#.to_string(),
            r#"{"op":"transform","ids":["ln6:s1","ln6:s2","ln6:s3","ln6:s4","ln6:s5"],"rotate":60,"pivot":[80,100]}"#
                .to_string(),
            r#"{"op":"transform","ids":["col6:s1","col6:s2"],"rotate":60,"pivot":[80,100]}"#.to_string(),
        ]
        .join("\n");
        let mut ops = parse_lines(&text).unwrap();
        crate::fills::resolve(&bundle, &mut ops).unwrap();
        bundle.apply(&ops).unwrap();
        crate::fills::refill(&dir, "colour", 6, None, &quiet()).unwrap();
        dir
    }

    fn inbetween(dir: &std::path::Path, spec: InbetweenSpec) -> Result<()> {
        crate::agent::edit(dir, &[Op::Inbetween(spec)], &quiet())
    }

    fn keys() -> InbetweenSpec {
        InbetweenSpec {
            layers: vec!["lines".into(), "colour".into()],
            from: Some(0),
            to: Some(6),
            ..InbetweenSpec::default()
        }
    }

    fn stroked(points: &[(f32, f32)], frame: u32, part: &str) -> String {
        let path: Vec<String> = points.iter().map(|(x, y)| format!("{x} {y}")).collect();
        let part = match part {
            "" => String::new(),
            name => format!(r#","part":"{name}""#),
        };
        format!(
            r##"{{"op":"stroke","layer":"lines","frame":{frame},"brush":"ink","size":4,"color":"#202020","taper":[0,0]{part},"path":"M{}"}}"##,
            path.join(" L")
        )
    }

    fn two_keys(name: &str, second: &[String]) -> std::path::PathBuf {
        let dir = scratch(name);
        let mut doc = Document::new(320, 320);
        doc.timeline.frames = 8;
        let mut lines = Layer::paint("lines", "lines", "a0");
        lines.cels_mut().unwrap()[0].hold = Some(6);
        doc.layers = vec![lines];
        doc.next_id = 10;
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let text = [
            stroked(&[(40.0, 100.0), (200.0, 100.0)], 0, "body"),
            stroked(&[(40.0, 220.0), (120.0, 200.0), (200.0, 220.0)], 0, "leaf"),
            r#"{"op":"cel","new":"lines","frame":6,"id":"b6"}"#.to_string(),
        ]
        .into_iter()
        .chain(second.iter().cloned())
        .chain([r#"{"op":"clip","add":"walk","from":0,"to":7,"loop":true}"#.to_string()])
        .collect::<Vec<_>>()
        .join("\n");
        bundle.apply(&parse_lines(&text).unwrap()).unwrap();
        dir
    }

    fn mean(marks: &[Mark], id: u32) -> (f32, f32) {
        let s = marks.iter().find(|m| m.id() == id).unwrap().as_stroke().unwrap();
        let n = s.samples.len() as f32;
        s.samples
            .iter()
            .map(|q| q.pos())
            .fold((0.0, 0.0), |a, (x, y)| (a.0 + x / n, a.1 + y / n))
    }

    fn shown_at(bundle: &Bundle, frame: u32) -> (Cel, Vec<Mark>) {
        let cel = bundle.doc.layer("lines").unwrap().content_at(frame).unwrap().clone();
        let marks = bundle.read_marks("lines", &cel.id).unwrap();
        (cel, marks)
    }

    fn lines_between(from: u32, to: u32) -> InbetweenSpec {
        InbetweenSpec {
            layers: vec!["lines".into()],
            from: Some(from),
            to: Some(to),
            ..InbetweenSpec::default()
        }
    }

    #[test]
    fn parts_pair_keys_drawn_separately_and_a_loop_closes_on_its_first_drawing() {
        let dir = two_keys(
            "parts",
            &[
                stroked(&[(80.0, 220.0), (160.0, 200.0), (240.0, 220.0)], 6, "leaf"),
                stroked(&[(80.0, 100.0), (240.0, 100.0)], 6, "body"),
            ],
        );
        let bundle = Bundle::open(&dir).unwrap();
        let (a, b) = (
            bundle.read_marks("lines", "a0").unwrap(),
            bundle.read_marks("lines", "b6").unwrap(),
        );
        let by_id = tween::pair(&a, &b, Match::Id, &[]);
        assert!(by_id.pairs.iter().any(|p| (p.a, p.b) == (1, 1)));
        inbetween(&dir, lines_between(0, 6)).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let (mid, marks) = shown_at(&bundle, 3);
        assert!(mid.tween.is_some());
        assert!((mean(&marks, 1).1 - 100.0).abs() < 2.0, "{:?}", mean(&marks, 1));
        assert!(mean(&marks, 2).1 > 190.0);
        assert!((mean(&marks, 1).0 - 140.0).abs() < 2.0, "{:?}", mean(&marks, 1));
        assert_eq!(mid.parts.get("body"), Some(&vec![1]));
        assert_eq!(mid.parts.get("leaf"), Some(&vec![2]));
        let looping = InbetweenSpec {
            layers: vec!["lines".into()],
            from: Some(6),
            looping: true,
            ..InbetweenSpec::default()
        };
        inbetween(&dir, looping).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        assert_eq!(bundle.doc.timeline.frames, 8);
        let (last, marks) = shown_at(&bundle, 7);
        let t = last.tween.clone().unwrap();
        assert_eq!((t.from.as_str(), t.to.as_str(), last.frame), ("b6", "a0", 7));
        let (body, leaf) = (last.parts["body"][0], last.parts["leaf"][0]);
        assert_eq!((body, leaf), (2, 1));
        assert!(
            (mean(&marks, body).0 - 140.0).abs() < 2.0 && (mean(&marks, body).1 - 100.0).abs() < 2.0,
            "{:?}",
            mean(&marks, body)
        );
        assert!((mean(&marks, leaf).1 - 207.0).abs() < 8.0, "{:?}", mean(&marks, leaf));
        assert_eq!(bundle.doc.layer("lines").unwrap().cels().len(), 8);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn marks_in_one_key_only_pop_fade_or_draw_on() {
        let second = [
            stroked(&[(80.0, 100.0), (240.0, 100.0)], 6, "body"),
            stroked(&[(80.0, 220.0), (160.0, 200.0), (240.0, 220.0)], 6, "leaf"),
            stroked(&[(250.0, 40.0), (250.0, 160.0)], 6, ""),
        ];
        let counts = |dir: &std::path::Path| -> Vec<usize> {
            let bundle = Bundle::open(dir).unwrap();
            (1..6).map(|f| shown_at(&bundle, f).1.len()).collect()
        };
        let extra = |dir: &std::path::Path, frame: u32| -> Option<Stroke> {
            let (_, marks) = shown_at(&Bundle::open(dir).unwrap(), frame);
            marks.iter().filter_map(Mark::as_stroke).find(|s| s.id > 2).cloned()
        };
        let leave = two_keys("leave", &second);
        inbetween(&leave, lines_between(0, 6)).unwrap();
        assert_eq!(counts(&leave), [2, 2, 2, 2, 2]);
        let pop = two_keys("pop", &second);
        inbetween(
            &pop,
            InbetweenSpec {
                unpaired: Unpaired::Pop(0.5),
                ..lines_between(0, 6)
            },
        )
        .unwrap();
        assert_eq!(counts(&pop), [2, 2, 3, 3, 3]);
        let (cel, _) = shown_at(&Bundle::open(&pop).unwrap(), 4);
        assert_eq!(cel.tween.unwrap().unpaired, "pop");
        let fade = two_keys("fade", &second);
        inbetween(
            &fade,
            InbetweenSpec {
                unpaired: Unpaired::Fade,
                ..lines_between(0, 6)
            },
        )
        .unwrap();
        assert_eq!(counts(&fade), [3, 3, 3, 3, 3]);
        let (early, late) = (extra(&fade, 1).unwrap(), extra(&fade, 5).unwrap());
        assert!((early.opacity - 1.0 / 6.0).abs() < 0.02, "{}", early.opacity);
        assert!((late.opacity - 5.0 / 6.0).abs() < 0.02, "{}", late.opacity);
        let draw_on = two_keys("drawon", &second);
        inbetween(
            &draw_on,
            InbetweenSpec {
                unpaired: Unpaired::DrawOn,
                ..lines_between(0, 6)
            },
        )
        .unwrap();
        let (early, late) = (extra(&draw_on, 1).unwrap(), extra(&draw_on, 5).unwrap());
        assert!(
            early.samples.len() * 3 < late.samples.len(),
            "{} {}",
            early.samples.len(),
            late.samples.len()
        );
        let tip = |s: &Stroke| s.samples.iter().map(|q| q.pos().1).fold(0.0f32, f32::max);
        assert!(
            tip(&early) < 70.0 && tip(&late) > 130.0,
            "{} {}",
            tip(&early),
            tip(&late)
        );
        [leave, pop, fade, draw_on]
            .iter()
            .for_each(|d| fs::remove_dir_all(d).unwrap());
    }

    #[test]
    fn hand_marks_stay_on_top_through_two_redos_and_move_with_their_line() {
        let dir = swing("keep");
        inbetween(&dir, keys()).unwrap();
        let mut bundle = Bundle::open(&dir).unwrap();
        let mid = bundle.doc.layer("lines").unwrap().content_at(3).unwrap().id.clone();
        let mut drawn = bundle.read_marks("lines", &mid).unwrap();
        let top = drawn[0].as_stroke().unwrap().clone();
        let (x0, y0) = top.samples[top.samples.len() / 2].pos();
        drawn.push(Mark::Stroke(
            Stroke::new(99, "ink", [0.0, 0.0, 0.0, 1.0], 3.0, 1)
                .with_points(&[(x0 - 6.0, y0 - 12.0, 1.0), (x0 + 6.0, y0 - 18.0, 1.0)], 8.0),
        ));
        bundle.replace_marks("lines", &mid, &drawn, "editor", "pen").unwrap();
        let hand = |dir: &std::path::Path| -> Vec<Stroke> {
            Bundle::open(dir)
                .unwrap()
                .read_marks("lines", &mid)
                .unwrap()
                .into_iter()
                .filter(|m| m.author() == klad_core::stroke::Author::Human)
                .filter_map(|m| m.as_stroke().cloned())
                .collect()
        };
        let line = |dir: &std::path::Path| -> (f32, f32) {
            let marks = Bundle::open(dir).unwrap().read_marks("lines", &mid).unwrap();
            let s = marks.iter().find(|m| m.id() == 1).unwrap().as_stroke().unwrap().clone();
            s.samples[s.samples.len() / 2].pos()
        };
        let keep = InbetweenSpec {
            redo: true,
            keep_drawn: true,
            ..InbetweenSpec::default()
        };
        let shift = r#"{"op":"transform","ids":["ln6:s1","ln6:s2","ln6:s3","ln6:s4","ln6:s5"],"x":30}"#;
        (1..=2).for_each(|round| {
            let (start, under) = (hand(&dir)[0].samples[0].pos(), line(&dir));
            let mut bundle = Bundle::open(&dir).unwrap();
            bundle.apply(&parse_lines(shift).unwrap()).unwrap();
            let plain = InbetweenSpec {
                redo: true,
                ..InbetweenSpec::default()
            };
            let refused = inbetween(&dir, plain).unwrap_err();
            assert!(format!("{refused:#}").contains("--keep-drawn"), "{refused:#}");
            inbetween(&dir, keep.clone()).unwrap();
            let kept = hand(&dir);
            assert_eq!(kept.len(), 1, "round {round}");
            let (now, moved) = (kept[0].samples[0].pos(), line(&dir));
            let (dx, dy) = (moved.0 - under.0, moved.1 - under.1);
            assert!((dx - 15.0).abs() < 2.0, "round {round}: the line moved {dx},{dy}");
            assert!(
                (now.0 - start.0 - dx).abs() < 2.5 && (now.1 - start.1 - dy).abs() < 2.5,
                "round {round}: {start:?} -> {now:?}, the line moved {dx},{dy}"
            );
            let all = Bundle::open(&dir).unwrap().read_marks("lines", &mid).unwrap();
            assert_eq!(all.len(), 6);
            assert_eq!(all.last().unwrap().author(), klad_core::stroke::Author::Human);
        });
        let mut bundle = Bundle::open(&dir).unwrap();
        bundle.apply(&parse_lines(shift).unwrap()).unwrap();
        let other = bundle.doc.layer("lines").unwrap().content_at(2).unwrap().id.clone();
        let before = bundle.read_marks("lines", &other).unwrap();
        let chosen = InbetweenSpec {
            redo: true,
            only: vec![mid.clone()],
            force_cels: vec![mid.clone()],
            ..InbetweenSpec::default()
        };
        inbetween(&dir, chosen).unwrap();
        assert!(hand(&dir).is_empty());
        let bundle = Bundle::open(&dir).unwrap();
        assert_eq!(bundle.read_marks("lines", &other).unwrap(), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn in_betweens_turn_with_the_keys_and_refill_their_flats() {
        let dir = swing("make");
        inbetween(&dir, keys()).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let lines = bundle.doc.layer("lines").unwrap();
        let made: Vec<&Cel> = lines.cels().iter().filter(|c| c.tween.is_some()).collect();
        assert_eq!(made.iter().map(|c| c.frame).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
        assert_eq!(lines.cels()[0].hold, Some(1));
        let mid = made[2];
        let tween = mid.tween.as_ref().unwrap();
        assert_eq!((tween.from.as_str(), tween.to.as_str(), tween.t), ("ln0", "ln6", 0.5));
        let marks = bundle.read_marks("lines", &mid.id).unwrap();
        assert_eq!(made_hash(&marks), tween.made);
        let tip = marks[0].as_stroke().unwrap().samples.last().unwrap().pos();
        let (s, c) = (libm::sinf(30f32.to_radians()), libm::cosf(30f32.to_radians()));
        let want = (80.0 + 150.0 * c + 16.0 * s, 100.0 + 150.0 * s - 16.0 * c);
        assert!(
            (tip.0 - want.0).abs() < 1.5 && (tip.1 - want.1).abs() < 1.5,
            "{tip:?} {want:?}"
        );
        let colour = bundle.doc.layer("colour").unwrap();
        let flats = colour.content_at(3).unwrap();
        let fills = bundle.read_marks("colour", &flats.id).unwrap();
        assert_eq!(fills.len(), 2);
        let keyed = bundle.read_marks("colour", "col0").unwrap();
        fills.iter().zip(&keyed).for_each(|(f, k)| {
            let (now, then) = (
                f.as_fill().unwrap().coverage.pixels(),
                k.as_fill().unwrap().coverage.pixels(),
            );
            assert!((now as f32 - then as f32).abs() < then as f32 * 0.15, "{now} vs {then}");
        });
        let sleeve = fills[0].as_fill().unwrap();
        let mid_arm = (80.0 + 75.0 * c, 100.0 + 75.0 * s);
        assert_eq!(sleeve.coverage.at(mid_arm.0 as u32, mid_arm.1 as u32), 255);
        let again = inbetween(&dir, keys());
        assert!(again.is_ok(), "in-betweens of the same keys are replaced: {again:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn redo_follows_a_changed_key_and_leaves_hand_drawn_in_betweens() {
        let dir = swing("redo");
        inbetween(&dir, keys()).unwrap();
        let mut bundle = Bundle::open(&dir).unwrap();
        let mid = bundle.doc.layer("lines").unwrap().content_at(3).unwrap().id.clone();
        let before = bundle.read_marks("lines", &mid).unwrap();
        let moved =
            parse_lines(r#"{"op":"transform","ids":["ln6:s1","ln6:s2","ln6:s3","ln6:s4","ln6:s5"],"x":30}"#).unwrap();
        bundle.apply(&moved).unwrap();
        let redo = InbetweenSpec {
            redo: true,
            ..InbetweenSpec::default()
        };
        inbetween(&dir, redo.clone()).unwrap();
        let mut bundle = Bundle::open(&dir).unwrap();
        let after = bundle.read_marks("lines", &mid).unwrap();
        let shift = after[0].as_stroke().unwrap().samples[0].x - before[0].as_stroke().unwrap().samples[0].x;
        assert!((shift as f32 / 16.0 - 15.0).abs() < 1.0, "{shift}");
        let mut drawn = bundle.read_marks("lines", &mid).unwrap();
        drawn.push(Mark::Stroke(
            Stroke::new(99, "ink", [0.0, 0.0, 0.0, 1.0], 3.0, 1)
                .with_points(&[(10.0, 10.0, 1.0), (20.0, 20.0, 1.0)], 8.0),
        ));
        bundle.replace_marks("lines", &mid, &drawn, "editor", "pen").unwrap();
        let refused = inbetween(&dir, redo.clone()).unwrap_err();
        assert!(format!("{refused:#}").contains("1 mark drawn by hand"), "{refused:#}");
        let forced = InbetweenSpec { force: true, ..redo };
        inbetween(&dir, forced).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        assert_eq!(bundle.read_marks("lines", &mid).unwrap().len(), 5);
        let foreign = InbetweenSpec {
            from: Some(0),
            to: Some(6),
            layers: vec!["lines".into()],
            ..InbetweenSpec::default()
        };
        let mut doc_bundle = Bundle::open(&dir).unwrap();
        doc_bundle
            .apply(&parse_lines(&format!(r#"{{"op":"set","id":"{mid}","tween":null}}"#)).unwrap())
            .unwrap();
        let blocked = inbetween(&dir, foreign.clone()).unwrap_err();
        assert!(format!("{blocked:#}").contains("not in-betweens"), "{blocked:#}");
        let mut again = Bundle::open(&dir).unwrap();
        let next = again.doc.layer("lines").unwrap().content_at(2).unwrap().id.clone();
        let mut touched = again.read_marks("lines", &next).unwrap();
        touched.pop();
        again
            .replace_marks("lines", &next, &touched, "editor", "erase")
            .unwrap();
        again
            .apply(&parse_lines(&format!(r#"{{"op":"rm","id":"{mid}"}}"#)).unwrap())
            .unwrap();
        let kept = inbetween(&dir, foreign).unwrap_err();
        assert!(format!("{kept:#}").contains("changed after they were made"), "{kept:#}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
