use crate::vector;
use anyhow::{Result, anyhow, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::{Cel, Kind, Layer, Node, Transform};
use klad_core::markedit::{MarkTransform, apply_affine, invert};
use klad_core::ops::{Op, ShiftSpec, Shifted};
use klad_core::rig;
use klad_core::stroke::Mark;

pub fn moved_node(node: &Node, affine: [f32; 6], scale: f32, degrees: f32) -> Result<Transform> {
    let mut t = node.transform;
    let pivot = match t.pivot {
        Some(p) => p,
        None => {
            let mut plain = node.clone();
            plain.transform = Transform::default();
            vector::local_bounds(&plain)?.map_or([0.0, 0.0], |r| [r.center_x(), r.center_y()])
        }
    };
    let (qx, qy) = apply_affine(affine, (t.x + pivot[0], t.y + pivot[1]));
    t.x = qx - pivot[0];
    t.y = qy - pivot[1];
    t.sx *= scale;
    t.sy *= scale;
    t.rotate += degrees;
    Ok(t)
}

pub fn resolve(bundle: &Bundle, ops: &mut [Op]) -> Result<()> {
    ops.iter_mut().try_for_each(|op| match op {
        Op::Shift(spec) if spec.resolved.is_none() => resolve_one(bundle, spec),
        _ => Ok(()),
    })
}

fn family(root: &Layer) -> Vec<&Layer> {
    let mut out = vec![root];
    klad_core::doc::walk_children(root, &mut out);
    out
}

pub fn moved_cels(root: &Layer, only: Option<u32>) -> Vec<(&Layer, &Cel)> {
    family(root)
        .into_iter()
        .filter(|l| matches!(l.kind(), Kind::Paint | Kind::Vector))
        .flat_map(|l| {
            l.cels()
                .iter()
                .filter(move |c| c.link.is_none())
                .filter(move |c| only.is_none_or(|f| l.content_at(f).is_some_and(|shown| shown.id == c.id)))
                .map(move |c| (l, c))
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct Targets {
    pub name: String,
    pub marks: Vec<(String, Vec<u32>)>,
    pub nodes: Vec<Node>,
    pub weights: Vec<(String, rig::Weights)>,
    pub bounds: Option<[f32; 4]>,
}

pub fn targets(bundle: &Bundle, layer: &str, only: Option<u32>) -> Result<Targets> {
    let doc = &bundle.doc;
    let root = doc.find_layer(layer)?;
    let cels = moved_cels(root, only);
    let mut boxes: Vec<[f32; 4]> = Vec::new();
    let mut marks = Vec::new();
    cels.iter()
        .filter(|(l, _)| l.kind() == Kind::Paint)
        .try_for_each(|(l, c)| {
            let found = bundle.read_marks(&l.id, &c.id)?;
            boxes.extend(found.iter().filter_map(Mark::bounds));
            if !found.is_empty() {
                marks.push((c.id.clone(), found.iter().map(Mark::id).collect::<Vec<_>>()));
            }
            Ok::<_, anyhow::Error>(())
        })?;
    let nodes: Vec<Node> = cels
        .iter()
        .filter(|(l, _)| l.kind() == Kind::Vector)
        .flat_map(|(_, c)| c.nodes.iter().cloned())
        .collect();
    if let Some(r) = vector::nodes_bounds(&nodes)? {
        boxes.push([r.left, r.top, r.right, r.bottom]);
    }
    if only.is_none() {
        family(root).iter().flat_map(|l| l.bones.iter()).for_each(|b| {
            boxes.push([
                b.head[0].min(b.tail[0]),
                b.head[1].min(b.tail[1]),
                b.head[0].max(b.tail[0]),
                b.head[1].max(b.tail[1]),
            ])
        });
    }
    let weights = match only {
        Some(_) => Vec::new(),
        None => family(root)
            .into_iter()
            .filter(|l| !l.bones.is_empty())
            .map(|l| bundle.read_weights(&l.id).map(|w| (l.id.clone(), w)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, w)| !w.maps.is_empty())
            .collect(),
    };
    Ok(Targets {
        name: root.name.clone(),
        marks,
        nodes,
        weights,
        bounds: boxes
            .into_iter()
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
    })
}

pub fn shifted(targets: &Targets, t: &mut MarkTransform) -> Result<Shifted> {
    let pivot = match (t.pivot, targets.bounds) {
        (Some(p), _) => p,
        (None, Some(w)) => [(w[0] + w[2]) / 2.0, (w[1] + w[3]) / 2.0],
        (None, None) => bail!("{} has nothing to transform", targets.name),
    };
    t.pivot = Some(pivot);
    let affine = t.affine(pivot);
    let even = (t.sx - t.sy).abs() < 1e-6;
    let moving = t.rotate == 0.0 && t.sx == 1.0 && t.sy == 1.0;
    let keyed = |n: &Node| {
        ["x", "y", "rotate", "sx", "sy", "scale"]
            .iter()
            .any(|k| n.keys.contains_key(*k))
    };
    let nodes: Vec<(String, Transform)> = targets
        .nodes
        .iter()
        .map(|n| {
            if !even {
                bail!(
                    "{} has shapes, which scale evenly: give one scale, or move and turn it only",
                    targets.name
                );
            }
            if keyed(n) && !moving {
                bail!(
                    "shape {} moves by its keys, so {} can only be moved, not turned or scaled",
                    n.id,
                    targets.name
                );
            }
            Ok((n.id.clone(), moved_node(n, affine, t.sx, t.rotate)?))
        })
        .collect::<Result<_>>()?;
    let back = invert(affine).ok_or_else(|| anyhow!("that transform flattens the layer to a line"))?;
    Ok(Shifted {
        marks: targets.marks.clone(),
        nodes,
        weights: targets
            .weights
            .iter()
            .map(|(id, w)| (id.clone(), rig::moved_weights(w, back)))
            .collect(),
    })
}

pub fn resolve_one(bundle: &Bundle, spec: &mut ShiftSpec) -> Result<()> {
    let found = targets(bundle, &spec.layer, spec.only)?;
    spec.resolved = Some(shifted(&found, &mut spec.transform)?);
    Ok(())
}
