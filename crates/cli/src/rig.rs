use anyhow::{Context, Result, anyhow, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::{Document, Layer};
use klad_core::ops::{Applied, Op, PoseSpec, PosedCel, apply, rig_owner};
use klad_core::rig::{Bone, CELL, Ik, Pose, Rig, Weights, applied, between, family, painted, tips, weights_for};
use klad_core::stroke::Mark;
use klad_render::overlay::Heat;
use serde_json::json;
use std::collections::BTreeMap;

pub fn owners(doc: &Document) -> Vec<&Layer> {
    doc.all_layers().into_iter().filter(|l| !l.bones.is_empty()).collect()
}

fn pose_at(owner: &Layer, frame: u32) -> Pose {
    painted(owner)
        .iter()
        .find_map(|l| l.content_at(frame).and_then(|c| c.pose.clone()))
        .unwrap_or_default()
}

pub fn bones_at(bundle: &Bundle, frame: u32) -> Result<Vec<klad_render::overlay::Shown>> {
    Ok(owners(&bundle.doc)
        .into_iter()
        .flat_map(|owner| {
            let pose = pose_at(owner, frame);
            owner
                .bones
                .iter()
                .zip(tips(&owner.bones, &owner.ik, &pose))
                .map(|(b, (h, t))| (b.name.clone(), h, t))
                .collect::<Vec<_>>()
        })
        .collect())
}

pub fn targets_at(bundle: &Bundle, frame: u32) -> Vec<([f32; 2], [f32; 2])> {
    owners(&bundle.doc)
        .into_iter()
        .flat_map(|owner| {
            let pose = pose_at(owner, frame);
            let shown = tips(&owner.bones, &owner.ik, &pose);
            pose.ik
                .iter()
                .filter_map(|(bone, reach)| {
                    let i = owner.bones.iter().position(|b| &b.name == bone)?;
                    owner
                        .ik
                        .iter()
                        .any(|c| &c.bone == bone)
                        .then_some((shown[i].1, reach.target))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn heat(bundle: &Bundle, bone: &str) -> Result<Heat> {
    let owner = owners(&bundle.doc)
        .into_iter()
        .find(|o| o.bones.iter().any(|b| b.name == bone))
        .ok_or_else(|| anyhow!("no bone {bone} in this drawing"))?;
    let weights = bundle.read_weights(&owner.id)?;
    let at: Vec<u8> = (0..weights.height)
        .flat_map(|y| (0..weights.width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let p = [(x as f32 + 0.5) * CELL as f32, (y as f32 + 0.5) * CELL as f32];
            let i = owner.bones.iter().position(|b| b.name == bone).unwrap_or(0);
            (weights.at(&owner.bones, p)[i] * 255.0).round() as u8
        })
        .collect();
    Ok(Heat {
        cell: CELL as f32,
        width: weights.width,
        values: at,
    })
}

fn rest_of(layer: &Layer, frame: u32) -> Option<String> {
    let cel = layer.content_at(frame)?;
    Some(cel.pose.as_ref().map_or_else(|| cel.id.clone(), |p| p.rest.clone()))
}

fn posed(bundle: &Bundle, doc: &mut Document, spec: &PoseSpec, weights: &Weights) -> Result<Vec<PosedCel>> {
    let owner = rig_owner(doc, &spec.layer)?.clone();
    if owner.bones.is_empty() {
        bail!("{} has no bones yet: add them with klad bone first", owner.name);
    }
    if let Some(b) = spec.bones.keys().find(|b| !owner.bones.iter().any(|o| &o.name == *b)) {
        bail!("no bone {b} on {}", owner.name);
    }
    if let Some(b) = spec.ik.keys().find(|b| !owner.ik.iter().any(|c| &c.bone == *b)) {
        bail!(
            "no IK on {b} on {}: add one first with klad ik --add {b}{}",
            owner.name,
            match owner.ik.is_empty() {
                true => String::new(),
                false => format!(
                    " (the IKs end on {})",
                    owner.ik.iter().map(|c| c.bone.as_str()).collect::<Vec<_>>().join(", ")
                ),
            }
        );
    }
    let canvas = (doc.canvas.width, doc.canvas.height);
    let layers: Vec<Layer> = painted(&owner).into_iter().cloned().collect();
    layers
        .iter()
        .filter_map(|layer| rest_of(layer, spec.from).map(|rest| (layer, rest)))
        .map(|(layer, rest)| {
            let pose = Pose {
                rest: rest.clone(),
                angles: spec.bones.clone(),
                root: spec.root,
                ik: spec.ik.clone(),
            };
            let marks = bundle.read_marks(&layer.id, &rest)?;
            let made = Rig::new(&owner.bones, &owner.ik, weights, &pose).pose(&marks, canvas);
            let here = layer.cels().iter().find(|c| c.frame == spec.to);
            let (cel, fresh) = match here {
                Some(c) if c.pose.as_ref().is_some_and(|p| p.rest == rest) => (c.id.clone(), false),
                Some(c) if c.id == rest => bail!(
                    "frame {} holds the rest drawing {rest} on {}: pose into another frame",
                    spec.to,
                    layer.name
                ),
                Some(c) => bail!(
                    "frame {} already has the drawing {} on {}, not a pose of {rest}",
                    spec.to,
                    c.id,
                    layer.name
                ),
                None => (doc.alloc_id('c'), true),
            };
            Ok(PosedCel {
                layer: layer.id.clone(),
                cel,
                frame: spec.to,
                hold: spec.hold,
                pose,
                marks: made,
                fresh,
            })
        })
        .collect()
}

pub fn resolve(bundle: &Bundle, ops: &mut [Op]) -> Result<Vec<String>> {
    if !ops.iter().any(|op| matches!(op, Op::Weights(_) | Op::Pose(_))) {
        return Ok(Vec::new());
    }
    let mut doc = bundle.doc.clone();
    let mut applied = Applied::default();
    let mut weights: BTreeMap<String, Weights> = BTreeMap::new();
    let mut notes = Vec::new();
    ops.iter_mut().enumerate().try_for_each(|(i, op)| {
        match op {
            Op::Weights(spec) => {
                let made = weights_for(bundle, &doc, spec, &weights).with_context(|| format!("op {}", i + 1))?;
                let owner = rig_owner(&doc, &spec.layer)?.id.clone();
                weights.insert(owner, made.clone());
                spec.resolved = Some(made);
            }
            Op::Pose(spec) => {
                let owner = rig_owner(&doc, &spec.layer)?.id.clone();
                let w = match weights.get(&owner) {
                    Some(w) => w.clone(),
                    None => bundle.read_weights(&owner)?,
                };
                let w = match w.maps.is_empty() {
                    true => {
                        let o = rig_owner(&doc, &spec.layer)?;
                        Weights::auto(&o.bones, (doc.canvas.width, doc.canvas.height))
                    }
                    false => w,
                };
                let made = posed(bundle, &mut doc, spec, &w).with_context(|| format!("op {}", i + 1))?;
                notes.push(format!(
                    "posed {} drawings at frame {}: {}",
                    made.len(),
                    spec.to,
                    made.iter().map(|c| c.cel.as_str()).collect::<Vec<_>>().join(" ")
                ));
                spec.resolved = Some(made);
            }
            _ => {}
        }
        apply(&mut doc, op, &mut applied).with_context(|| format!("op {}", i + 1))
    })?;
    Ok(notes)
}

pub fn list(bundle: &Bundle, as_json: bool) -> Result<()> {
    let doc = &bundle.doc;
    let rigs: Vec<serde_json::Value> = owners(doc)
        .iter()
        .map(|o| {
            let weights = bundle.read_weights(&o.id).ok();
            json!({
                "layer": o.id,
                "name": o.name,
                "bones": o.bones.iter().map(|b| json!({
                    "name": b.name,
                    "parent": b.parent,
                    "head": b.head,
                    "tail": b.tail,
                    "weights": weights.as_ref().is_some_and(|w| w.maps.contains_key(&b.name)),
                })).collect::<Vec<_>>(),
                "ik": o.ik.iter().map(|c| json!({
                    "bone": c.bone,
                    "chain": c.chain,
                    "bend": c.bend,
                    "softness": c.softness,
                })).collect::<Vec<_>>(),
                "poses": painted(o).iter().flat_map(|l| l.cels().iter().filter_map(|c| c.pose.as_ref().map(|p| {
                    let mut entry = json!({"cel": c.id, "frame": c.frame, "rest": p.rest, "angles": p.angles, "root": p.root});
                    if !p.ik.is_empty() {
                        entry["ik"] = json!(p.ik);
                        entry["solved"] = json!(applied(&o.bones, &o.ik, p));
                    }
                    entry
                }))).collect::<Vec<_>>(),
            })
        })
        .collect();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&rigs)?);
        return Ok(());
    }
    if rigs.is_empty() {
        println!("no bones yet: klad bone <bundle> <layer> --add hip --head x,y --tail x,y");
    }
    owners(doc).iter().for_each(|o| {
        println!("{} ({})", o.name, o.id);
        let weights = bundle.read_weights(&o.id).ok();
        o.bones.iter().for_each(|b: &Bone| {
            let painted = weights.as_ref().is_some_and(|w| w.maps.contains_key(&b.name));
            println!(
                "  {:<14} {:<12} {:.0},{:.0} -> {:.0},{:.0}{}",
                b.name,
                b.parent.as_deref().unwrap_or("-"),
                b.head[0],
                b.head[1],
                b.tail[0],
                b.tail[1],
                if painted { "" } else { "  no weights yet" }
            );
        });
        o.ik.iter().for_each(|c: &Ik| {
            let parent = o.bones.iter().find(|b| b.name == c.bone).and_then(|b| b.parent.clone());
            match (c.chain, parent) {
                (2, Some(p)) => println!(
                    "  ik {}: a limb with {p}, bending {}{}",
                    c.bone,
                    serde_json::to_value(c.bend)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default(),
                    match c.softness > 0.0 {
                        true => format!(", softness {} px", c.softness),
                        false => String::new(),
                    }
                ),
                _ => println!("  ik {}: an aim", c.bone),
            }
        });
        painted(o).iter().for_each(|l| {
            l.cels()
                .iter()
                .filter_map(|c| c.pose.as_ref().map(|p| (c, p)))
                .for_each(|(c, p)| {
                    let mut said: Vec<String> = p.angles.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    if p.root != [0.0, 0.0] {
                        said.push(format!("root {},{}", p.root[0], p.root[1]));
                    }
                    p.ik.iter().for_each(|(k, r)| {
                        said.push(match r.mix == 1.0 {
                            true => format!("target {k}={},{}", r.target[0], r.target[1]),
                            false => format!("target {k}={},{} mix {}", r.target[0], r.target[1], r.mix),
                        })
                    });
                    if !p.ik.is_empty() && !o.ik.is_empty() {
                        let solved: Vec<String> = applied(&o.bones, &o.ik, p)
                            .iter()
                            .map(|(k, v)| format!("{k}={v:.2}"))
                            .collect();
                        said.push(format!("solved {}", solved.join(" ")));
                    }
                    println!("  pose {} at {} from {}: {}", c.id, c.frame, p.rest, said.join(" "));
                })
        });
    });
    Ok(())
}

#[derive(Clone)]
pub struct Posed {
    bones: Vec<Bone>,
    ik: Vec<Ik>,
    weights: Weights,
    a: Pose,
    b: Pose,
    rest: Vec<Mark>,
}

impl Posed {
    pub fn at(&self, t: f32, canvas: (u32, u32)) -> (Vec<Mark>, Pose) {
        let pose = between(&self.bones, &self.ik, &self.a, &self.b, t);
        (
            Rig::new(&self.bones, &self.ik, &self.weights, &pose).pose(&self.rest, canvas),
            pose,
        )
    }
}

pub fn posed_pair(bundle: &Bundle, layer: &str, a: &str, b: &str) -> Result<Option<Posed>> {
    let doc = &bundle.doc;
    let Some(paint) = doc.layer(layer) else {
        return Ok(None);
    };
    let pose = |id: &str| paint.cels().iter().find(|c| c.id == id).and_then(|c| c.pose.clone());
    let rest = |id: &str| Pose {
        rest: id.to_string(),
        ..Pose::default()
    };
    let (pa, pb) = match (pose(a), pose(b)) {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) if x.rest == b => (x, rest(b)),
        (None, Some(y)) if y.rest == a => (rest(a), y),
        _ => return Ok(None),
    };
    if pa.rest != pb.rest {
        return Ok(None);
    }
    let Some(owner) = owners(doc)
        .into_iter()
        .find(|o| family(o).iter().any(|l| l.id == layer))
    else {
        return Ok(None);
    };
    let weights = bundle.read_weights(&owner.id)?;
    let weights = match weights.maps.is_empty() {
        true => Weights::auto(&owner.bones, (doc.canvas.width, doc.canvas.height)),
        false => weights,
    };
    Ok(Some(Posed {
        bones: owner.bones.clone(),
        ik: owner.ik.clone(),
        weights,
        a: pa.clone(),
        b: pb,
        rest: bundle.read_marks(layer, &pa.rest)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Output;
    use klad_core::ops::parse_lines;

    fn quiet() -> Output {
        Output {
            json: false,
            render: false,
            scale: None,
            frame: 0,
            overlay: None,
        }
    }

    fn arm(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("klad-rig-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(600, 400);
        doc.layers = vec![Layer::paint("l1", "arm", "c1")];
        doc.timeline.frames = 9;
        doc.next_id = 10;
        Bundle::create(&dir, doc).unwrap();
        let ops = r##"{"op":"stroke","layer":"l1","brush":"ink","size":20,"color":"#c06a3a","path":"M100 200 L 300 200 L 480 200","part":"arm"}
{"op":"fill","layer":"l1","polygon":[[470,185],[520,188],[522,212],[470,215]],"color":"#e0b080","part":"hand"}"##;
        crate::agent::edit(&dir, &parse_lines(ops).unwrap(), &quiet()).unwrap();
        dir
    }

    fn edit(dir: &std::path::Path, ops: &str) -> Result<()> {
        crate::agent::edit(dir, &parse_lines(ops)?, &quiet())
    }

    fn end_of(bundle: &Bundle, frame: u32) -> (f32, f32) {
        let cel = bundle.doc.layer("l1").unwrap().content_at(frame).unwrap().id.clone();
        let marks = bundle.read_marks("l1", &cel).unwrap();
        let s = marks.iter().find_map(Mark::as_stroke).unwrap();
        s.samples[s.samples.len() - 1].pos()
    }

    #[test]
    fn an_ik_limb_reaches_its_target_and_stays_planted_through_the_in_betweens() {
        let dir = arm("ik");
        edit(
            &dir,
            r#"{"op":"bone","layer":"l1","chain":["upper","fore","hand"],"through":[[100,200],[300,200],[480,200],[520,200]]}
{"op":"ik","layer":"l1","add":"fore"}
{"op":"weights","layer":"l1","auto":true}"#,
        )
        .unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let owner = bundle.doc.layer("l1").unwrap().clone();
        assert_eq!(owner.ik.len(), 1);
        assert_eq!(owner.bones[1].parent.as_deref(), Some("upper"));
        assert!(
            edit(
                &dir,
                r#"{"op":"pose","layer":"l1","from":0,"to":4,"ik":{"hand":{"target":[1,1]}}}"#
            )
            .is_err(),
            "a target needs an IK on that bone"
        );
        let target = [400.0, 300.0];
        edit(
            &dir,
            r#"{"op":"pose","layer":"l1","from":0,"to":4,"ik":{"fore":{"target":[400,300]}}}
{"op":"pose","layer":"l1","from":0,"to":8,"root":[40,-30],"ik":{"fore":{"target":[400,300]}}}"#,
        )
        .unwrap();
        edit(&dir, r#"{"op":"inbetween","layers":["l1"],"from":4,"to":8}"#).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        (4..=8).for_each(|f| {
            let end = end_of(&bundle, f);
            assert!(
                (end.0 - target[0]).hypot(end.1 - target[1]) < 4.0,
                "frame {f}: the forearm's end stays on its target while the shoulder moves: {end:?}"
            );
        });
        let mid = bundle
            .doc
            .layer("l1")
            .unwrap()
            .content_at(6)
            .unwrap()
            .pose
            .clone()
            .unwrap();
        assert_eq!(mid.ik["fore"].target, target, "the in-between keeps the target");
        assert!((mid.root[0] - 20.0).abs() < 1e-3 && (mid.root[1] + 15.0).abs() < 1e-3);
        let shown = targets_at(&bundle, 6);
        assert_eq!(shown.len(), 1);
        assert!((shown[0].0[0] - target[0]).hypot(shown[0].0[1] - target[1]) < 0.01 && shown[0].1 == target);
        let solved = applied(
            &owner.bones,
            &owner.ik,
            bundle
                .doc
                .layer("l1")
                .unwrap()
                .content_at(4)
                .unwrap()
                .pose
                .as_ref()
                .unwrap(),
        );
        let out = dir.join("godot");
        crate::scene::rig(&dir, Some(out.clone())).unwrap();
        let scene = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "tscn"))
            .unwrap();
        let text = std::fs::read_to_string(scene).unwrap();
        let track = text
            .split("tracks/")
            .find(|t| t.contains("Skeleton2D/upper:rotation\""))
            .unwrap();
        let block = text[text.find(track).unwrap()..].to_string();
        let values: Vec<f32> = block
            .split("\"values\": [")
            .nth(1)
            .unwrap()
            .split(']')
            .next()
            .unwrap()
            .split(',')
            .map(|v| v.trim().parse().unwrap())
            .collect();
        let wanted = solved["upper"] * (std::f32::consts::PI / 180.0);
        assert!(
            values.iter().any(|v| (v - wanted).abs() < 1e-3),
            "the rig exports the solved turn {wanted}: {values:?}"
        );
        assert!(
            text.contains("NodePath(\"Skeleton2D/upper:position\")") && text.contains("Vector2(140, 170)"),
            "the root move goes to Godot too, so the planted end stays put there"
        );
        edit(&dir, r#"{"op":"ik","layer":"l1","rm":"fore"}"#).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let kept = bundle
            .doc
            .layer("l1")
            .unwrap()
            .content_at(4)
            .unwrap()
            .pose
            .clone()
            .unwrap();
        assert!(kept.ik.is_empty() && (kept.angles["upper"] - solved["upper"]).abs() < 1e-4);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bones_weights_and_poses_through_the_cli_and_undo() {
        let dir = arm("pose");
        edit(
            &dir,
            r#"{"op":"bone","layer":"l1","add":"upper","head":[100,200],"tail":[300,200]}
{"op":"bone","layer":"l1","add":"fore","parent":"upper","head":[300,200],"tail":[480,200]}
{"op":"bone","layer":"l1","add":"hand","parent":"fore","head":[480,200],"tail":[520,200]}"#,
        )
        .unwrap();
        assert!(
            edit(
                &dir,
                r#"{"op":"bone","layer":"l1","add":"fore","head":[0,0],"tail":[1,1]}"#
            )
            .is_err()
        );
        edit(
            &dir,
            r#"{"op":"weights","layer":"l1","auto":true}
{"op":"weights","layer":"l1","bone":"hand","part":"hand"}"#,
        )
        .unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let w = bundle.read_weights("l1").unwrap();
        assert_eq!(w.maps.len(), 3);
        let bones = &bundle.doc.layer("l1").unwrap().bones;
        assert!(
            w.at(bones, [500.0, 200.0])[2] > 0.99,
            "the hand part is the hand bone's alone"
        );
        edit(
            &dir,
            r#"{"op":"pose","layer":"l1","from":0,"to":4,"bones":{"fore":-90}}"#,
        )
        .unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let end = end_of(&bundle, 4);
        assert!(
            (end.0 - 300.0).abs() < 4.0 && (end.1 - 20.0).abs() < 4.0,
            "the forearm points up: {end:?}"
        );
        let posed = bundle.doc.layer("l1").unwrap().content_at(4).unwrap();
        assert_eq!(posed.pose.as_ref().unwrap().rest, "c1");
        edit(
            &dir,
            r#"{"op":"pose","layer":"l1","from":4,"to":4,"bones":{"fore":-45}}"#,
        )
        .unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        assert_eq!(
            bundle.doc.layer("l1").unwrap().cels().len(),
            2,
            "posing a pose again re-poses it"
        );
        edit(&dir, r#"{"op":"inbetween","layers":["l1"],"from":0,"to":4}"#).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let mid = bundle.doc.layer("l1").unwrap().content_at(2).unwrap();
        let angle = mid.pose.as_ref().unwrap().angles["fore"];
        assert!(angle < -5.0 && angle > -40.0, "{angle}");
        let end = end_of(&bundle, 2);
        let reach = (end.0 - 300.0).hypot(end.1 - 200.0);
        assert!(
            (reach - 180.0).abs() < 4.0,
            "the forearm keeps its length on bones: {reach}"
        );
        edit(&dir, r#"{"op":"bone","layer":"l1","rename":"fore","as":"forearm"}"#).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        assert!(bundle.read_weights("l1").unwrap().maps.contains_key("forearm"));
        assert!(
            bundle
                .doc
                .layer("l1")
                .unwrap()
                .content_at(4)
                .unwrap()
                .pose
                .as_ref()
                .unwrap()
                .angles
                .contains_key("forearm")
        );
        let mut bundle = Bundle::open(&dir).unwrap();
        klad_core::history::step(&mut bundle, klad_core::history::Stack::Undo).unwrap();
        assert!(
            bundle.read_weights("l1").unwrap().maps.contains_key("fore"),
            "undo restores the weights file"
        );
        edit(&dir, r#"{"op":"bone","layer":"l1","move":"fore","tail":[470,230]}"#).unwrap();
        let bones = Bundle::open(&dir).unwrap().doc.layer("l1").unwrap().bones.clone();
        assert_eq!(
            bones[2].head,
            [470.0, 230.0],
            "the hand starts where the forearm now ends"
        );
        edit(&dir, r#"{"op":"bone","layer":"l1","move":"upper","by":[0,-20]}"#).unwrap();
        let lifted = Bundle::open(&dir).unwrap().doc.layer("l1").unwrap().bones.clone();
        assert!(
            lifted
                .iter()
                .zip(&bones)
                .all(|(a, b)| a.head == [b.head[0], b.head[1] - 20.0] && a.tail == [b.tail[0], b.tail[1] - 20.0]),
            "by moves a bone with its children: {lifted:?}"
        );
        edit(&dir, r#"{"op":"bone","layer":"l1","rm":"upper"}"#).unwrap();
        assert!(
            Bundle::open(&dir).unwrap().doc.layer("l1").unwrap().bones.is_empty(),
            "removing a bone removes its children"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
