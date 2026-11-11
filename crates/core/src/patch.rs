use crate::doc::{Cel, Document, Layer, LayerContent, Node};
use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const LIST_KEYS: &[&str] = &["layers", "cels", "children", "nodes", "rev"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    pub old: Value,
    pub new: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Patch(pub Vec<Change>);

type Out = Vec<Change>;

fn to_map<T: Serialize>(item: &T) -> Result<Map<String, Value>> {
    match serde_json::to_value(item)? {
        Value::Object(mut map) => {
            LIST_KEYS.iter().for_each(|k| {
                map.remove(*k);
            });
            Ok(map)
        }
        _ => bail!("expected an object"),
    }
}

fn shell(out: &mut Out, at: &str, a: Map<String, Value>, b: Map<String, Value>) {
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let (old, new): (Map<String, Value>, Map<String, Value>) = keys
        .into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            let pick = |m: &Map<String, Value>| m.get(k).cloned().unwrap_or(Value::Null);
            ((k.clone(), pick(&a)), (k.clone(), pick(&b)))
        })
        .unzip();
    if !old.is_empty() {
        out.push(Change {
            at: at.to_string(),
            index: None,
            old: Value::Object(old),
            new: Value::Object(new),
        });
    }
}

fn list<T: PartialEq + Serialize>(
    out: &mut Out,
    at: &str,
    a: &[T],
    b: &[T],
    same: fn(&T, &T) -> bool,
    inner: fn(&mut Out, &str, &T, &T) -> Result<()>,
) -> Result<()> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let room = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if ma.is_empty() && mb.is_empty() {
        return Ok(());
    }
    if ma.len() == mb.len() && ma.iter().zip(mb).all(|(x, y)| same(x, y)) {
        return ma
            .iter()
            .zip(mb)
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .try_for_each(|(i, (x, y))| inner(out, &format!("{at}/{}", prefix + i), x, y));
    }
    out.push(Change {
        at: at.to_string(),
        index: Some(prefix),
        old: serde_json::to_value(ma)?,
        new: serde_json::to_value(mb)?,
    });
    Ok(())
}

fn doc_shell(doc: &Document) -> Document {
    Document {
        format: doc.format.clone(),
        rev: 0,
        canvas: doc.canvas.clone(),
        timeline: doc.timeline.clone(),
        palette: doc.palette.clone(),
        variants: doc.variants.clone(),
        layers: Vec::new(),
        export: doc.export.clone(),
        atmosphere: doc.atmosphere.clone(),
        next_id: doc.next_id,
        here: doc.here.clone(),
    }
}

fn layer_shell(layer: &Layer) -> Layer {
    let content = match &layer.content {
        LayerContent::Paint { pixel, .. } => LayerContent::Paint {
            cels: Vec::new(),
            pixel: *pixel,
        },
        LayerContent::Vector { .. } => LayerContent::Vector { cels: Vec::new() },
        LayerContent::Group { .. } => LayerContent::Group { children: Vec::new() },
        LayerContent::Adjust { adjust } => LayerContent::Adjust { adjust: adjust.clone() },
        LayerContent::Bones {} => LayerContent::Bones {},
    };
    Layer {
        id: layer.id.clone(),
        name: layer.name.clone(),
        content,
        opacity: layer.opacity,
        blend: layer.blend,
        visible: layer.visible,
        locked: layer.locked,
        clip: layer.clip,
        lock_alpha: layer.lock_alpha,
        mask: layer.mask.clone(),
        effects: layer.effects.clone(),
        depth: layer.depth,
        reference: layer.reference,
        bones: layer.bones.clone(),
        ik: layer.ik.clone(),
        rig: layer.rig.clone(),
        keys: layer.keys.clone(),
    }
}

fn cel_shell(cel: &Cel) -> Cel {
    Cel {
        id: cel.id.clone(),
        frame: cel.frame,
        hold: cel.hold,
        link: cel.link.clone(),
        tween: cel.tween.clone(),
        nodes: Vec::new(),
        parts: cel.parts.clone(),
        pose: cel.pose.clone(),
    }
}

fn node_shell(node: &Node) -> Node {
    let mut shape = node.shape.clone();
    if let Some(children) = shape.children_mut() {
        children.clear();
    }
    Node {
        id: node.id.clone(),
        name: node.name.clone(),
        shape,
        fill: node.fill.clone(),
        stroke: node.stroke.clone(),
        transform: node.transform,
        opacity: node.opacity,
        blur: node.blur,
        clip: node.clip.clone(),
        visible: node.visible,
        keys: node.keys.clone(),
    }
}

fn same_layer(a: &Layer, b: &Layer) -> bool {
    a.id == b.id && a.kind() == b.kind()
}

fn same_cel(a: &Cel, b: &Cel) -> bool {
    a.id == b.id
}

fn same_node(a: &Node, b: &Node) -> bool {
    a.id == b.id && a.type_name() == b.type_name()
}

fn layer(out: &mut Out, at: &str, a: &Layer, b: &Layer) -> Result<()> {
    shell(out, at, to_map(&layer_shell(a))?, to_map(&layer_shell(b))?);
    match (&a.content, &b.content) {
        (LayerContent::Group { children: x }, LayerContent::Group { children: y }) => {
            list(out, &format!("{at}/children"), x, y, same_layer, layer)
        }
        (
            LayerContent::Paint { cels: x, .. } | LayerContent::Vector { cels: x },
            LayerContent::Paint { cels: y, .. } | LayerContent::Vector { cels: y },
        ) => list(out, &format!("{at}/cels"), x, y, same_cel, cel),
        (LayerContent::Adjust { .. }, LayerContent::Adjust { .. })
        | (LayerContent::Bones {}, LayerContent::Bones {}) => Ok(()),
        _ => bail!("layer {} changed kind", a.id),
    }
}

fn cel(out: &mut Out, at: &str, a: &Cel, b: &Cel) -> Result<()> {
    shell(out, at, to_map(&cel_shell(a))?, to_map(&cel_shell(b))?);
    list(out, &format!("{at}/nodes"), &a.nodes, &b.nodes, same_node, node)
}

fn node(out: &mut Out, at: &str, a: &Node, b: &Node) -> Result<()> {
    shell(out, at, to_map(&node_shell(a))?, to_map(&node_shell(b))?);
    list(
        out,
        &format!("{at}/children"),
        a.children(),
        b.children(),
        same_node,
        node,
    )
}

pub fn diff(from: &Document, to: &Document) -> Result<Patch> {
    let mut out = Vec::new();
    shell(&mut out, "", to_map(&doc_shell(from))?, to_map(&doc_shell(to))?);
    list(&mut out, "/layers", &from.layers, &to.layers, same_layer, layer)?;
    Ok(Patch(out))
}

enum Target<'a> {
    Doc(&'a mut Document),
    Layers(&'a mut Vec<Layer>),
    Layer(&'a mut Layer),
    Cels(&'a mut Vec<Cel>),
    Cel(&'a mut Cel),
    Nodes(&'a mut Vec<Node>),
    Node(&'a mut Node),
}

fn nth<'a, T>(list: &'a mut [T], part: &str) -> Result<&'a mut T> {
    let index: usize = part.parse().map_err(|_| anyhow!("{part:?} is not an index"))?;
    let len = list.len();
    list.get_mut(index)
        .ok_or_else(|| anyhow!("index {index} is past the end ({len})"))
}

fn step<'a>(target: Target<'a>, part: &str) -> Result<Target<'a>> {
    Ok(match (target, part) {
        (Target::Doc(doc), "layers") => Target::Layers(&mut doc.layers),
        (Target::Layers(list), part) => Target::Layer(nth(list, part)?),
        (Target::Layer(layer), "cels") => match &mut layer.content {
            LayerContent::Paint { cels, .. } | LayerContent::Vector { cels } => Target::Cels(cels),
            LayerContent::Group { .. } | LayerContent::Adjust { .. } | LayerContent::Bones {} => {
                bail!("layer {} has no cels", layer.id)
            }
        },
        (Target::Layer(layer), "children") => match &mut layer.content {
            LayerContent::Group { children } => Target::Layers(children),
            _ => bail!("layer {} is not a group", layer.id),
        },
        (Target::Cels(list), part) => Target::Cel(nth(list, part)?),
        (Target::Cel(cel), "nodes") => Target::Nodes(&mut cel.nodes),
        (Target::Nodes(list), part) => Target::Node(nth(list, part)?),
        (Target::Node(node), "children") => match node.shape.children_mut() {
            Some(children) => Target::Nodes(children),
            None => bail!("node {} is not a group", node.id),
        },
        (_, part) => bail!("no {part:?} there"),
    })
}

fn splice<T: PartialEq + DeserializeOwned>(list: &mut Vec<T>, index: usize, old: &Value, new: &Value) -> Result<()> {
    let old: Vec<T> = serde_json::from_value(old.clone())?;
    let new: Vec<T> = serde_json::from_value(new.clone())?;
    let end = index + old.len();
    if end > list.len() || list[index..end] != old[..] {
        bail!("the items there are not the ones this change expects");
    }
    list.splice(index..end, new);
    Ok(())
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) if x.is_f64() || y.is_f64() => {
            x.as_f64().zip(y.as_f64()).is_some_and(|(x, y)| x as f32 == y as f32)
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(v, w)| same(v, w)),
        _ => a == b,
    }
}

fn merge<T: Serialize + DeserializeOwned>(item: &T, old: &Value, new: &Value) -> Result<T> {
    let (Value::Object(old), Value::Object(new)) = (old, new) else {
        bail!("an item change needs objects");
    };
    let Value::Object(mut map) = serde_json::to_value(item)? else {
        bail!("expected an object");
    };
    old.iter().try_for_each(|(key, value)| {
        if !same(map.get(key).unwrap_or(&Value::Null), value) {
            bail!("{key} is not what this change expects");
        }
        Ok(())
    })?;
    new.iter().for_each(|(key, value)| match value {
        Value::Null => {
            map.remove(key);
        }
        v => {
            map.insert(key.clone(), v.clone());
        }
    });
    Ok(serde_json::from_value(Value::Object(map))?)
}

fn lists(content: &mut LayerContent) -> (Option<&mut Vec<Cel>>, Option<&mut Vec<Layer>>) {
    match content {
        LayerContent::Paint { cels, .. } | LayerContent::Vector { cels } => (Some(cels), None),
        LayerContent::Group { children } => (None, Some(children)),
        LayerContent::Adjust { .. } | LayerContent::Bones {} => (None, None),
    }
}

fn merge_layer(layer: &mut Layer, old: &Value, new: &Value) -> Result<()> {
    let (cels, children) = lists(&mut layer.content);
    let (cels, children) = (cels.map(std::mem::take), children.map(std::mem::take));
    let mut merged: Layer = merge(&*layer, old, new)?;
    match (lists(&mut merged.content), cels, children) {
        ((Some(slot), _), Some(cels), _) => *slot = cels,
        ((_, Some(slot)), _, Some(children)) => *slot = children,
        ((None, None), None, None) => {}
        _ => bail!("layer {} changed kind", layer.id),
    }
    *layer = merged;
    Ok(())
}

fn merge_doc(doc: &mut Document, old: &Value, new: &Value) -> Result<()> {
    let layers = std::mem::take(&mut doc.layers);
    let here = std::mem::take(&mut doc.here);
    *doc = Document {
        layers,
        rev: doc.rev,
        next_id: doc.next_id,
        here,
        ..merge(&*doc, old, new)?
    };
    Ok(())
}

fn merge_cel(cel: &mut Cel, old: &Value, new: &Value) -> Result<()> {
    let nodes = std::mem::take(&mut cel.nodes);
    *cel = Cel {
        nodes,
        ..merge(&*cel, old, new)?
    };
    Ok(())
}

fn merge_node(node: &mut Node, old: &Value, new: &Value) -> Result<()> {
    let children = node.shape.children_mut().map(std::mem::take).unwrap_or_default();
    let mut merged: Node = merge(&*node, old, new)?;
    if let Some(slot) = merged.shape.children_mut() {
        *slot = children;
    }
    *node = merged;
    Ok(())
}

fn apply_change(doc: &mut Document, change: &Change) -> Result<()> {
    let target = change.at.split('/').skip(1).try_fold(Target::Doc(doc), step)?;
    let (old, new) = (&change.old, &change.new);
    match (target, change.index) {
        (Target::Layers(list), Some(i)) => splice(list, i, old, new),
        (Target::Cels(list), Some(i)) => splice(list, i, old, new),
        (Target::Nodes(list), Some(i)) => splice(list, i, old, new),
        (Target::Doc(doc), None) => merge_doc(doc, old, new),
        (Target::Layer(layer), None) => merge_layer(layer, old, new),
        (Target::Cel(cel), None) => merge_cel(cel, old, new),
        (Target::Node(node), None) => merge_node(node, old, new),
        _ => bail!("the change does not fit what is there"),
    }
}

fn touched_cel(doc: &Document, at: &str) -> Option<String> {
    let parts: Vec<&str> = at.split('/').skip(1).collect();
    let mut layers: &[Layer] = &doc.layers;
    let mut rest = &parts[..];
    loop {
        match rest {
            ["layers" | "children", i, tail @ ..] => {
                let layer = layers.get(i.parse::<usize>().ok()?)?;
                match tail {
                    ["cels", c, "nodes", ..] => return Some(layer.cels().get(c.parse::<usize>().ok()?)?.id.clone()),
                    ["children", ..] => {
                        layers = layer.children();
                        rest = tail;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}

impl Patch {
    pub fn touched_cels(&self, doc: &Document) -> Option<std::collections::BTreeSet<String>> {
        self.0.iter().map(|c| touched_cel(doc, &c.at)).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn reversed(&self) -> Self {
        Self(
            self.0
                .iter()
                .rev()
                .map(|c| Change {
                    at: c.at.clone(),
                    index: c.index,
                    old: c.new.clone(),
                    new: c.old.clone(),
                })
                .collect(),
        )
    }

    pub fn apply_in_place(&self, doc: &mut Document) -> Result<()> {
        self.0.iter().try_for_each(|change| {
            let at = if change.at.is_empty() {
                "the document"
            } else {
                &change.at
            };
            apply_change(doc, change).with_context(|| format!("at {at}"))
        })
    }

    pub fn applied(&self, doc: &Document) -> Result<Document> {
        let mut next = doc.clone();
        self.apply_in_place(&mut next)?;
        next.validate()?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{apply_all, parse_lines};

    fn edited(doc: &Document, lines: &str) -> Document {
        apply_all(doc, &parse_lines(lines).unwrap()).unwrap().0
    }

    fn sample() -> Document {
        edited(
            &Document::new(512, 512),
            r##"{"op":"add","layer":{"id":"v","kind":"vector","name":"shapes"}}
{"op":"add","layer":{"id":"g","kind":"group","name":"fx","children":[{"id":"p","kind":"paint","name":"ink"}]}}
{"op":"add","node":{"id":"a","type":"rect","x":0,"y":0,"w":10,"h":10,"fill":"#ff0000"},"in":"v"}
{"op":"add","node":{"id":"grp","type":"group","children":[{"id":"b","type":"ellipse","cx":5,"cy":5,"rx":2,"ry":2},{"id":"c","type":"path","d":"M0 0 L5 5"}]},"in":"v"}
{"op":"add","node":{"id":"d","type":"rect","x":1,"y":1,"w":2,"h":2},"in":"v"}
{"op":"palette","add":"#102030","name":"ink"}"##,
        )
    }

    fn round_trip(before: &Document, after: &Document) -> Patch {
        let forward = diff(before, after).unwrap();
        let text = serde_json::to_string(&forward).unwrap();
        let forward: Patch = serde_json::from_str(&text).unwrap();
        let doc = forward.applied(before).unwrap();
        assert_eq!(
            &Document {
                rev: after.rev,
                ..doc.clone()
            },
            after
        );
        let doc = forward.reversed().applied(&doc).unwrap();
        assert_eq!(&Document { rev: before.rev, ..doc }, before);
        forward
    }

    #[test]
    fn small_edits_make_small_patches_that_undo_and_redo() {
        let doc = sample();
        let cases = [
            r#"{"op":"set","id":"c","d":"M0 0 L9 9","transform.rotate":15}"#,
            r#"{"op":"set","id":"v","name":"renamed","opacity":0.5}"#,
            r#"{"op":"set","id":"a","fill":null}"#,
            r#"{"op":"rm","id":"b"}"#,
            r#"{"op":"add","node":{"type":"rect","x":0,"y":0,"w":3,"h":3},"below":"d"}"#,
            r#"{"op":"mv","id":"d","below":"a"}"#,
            r#"{"op":"add","layer":{"kind":"paint","name":"new"},"in":"g"}"#,
            r#"{"op":"mv","id":"p","above":"g"}"#,
            r#"{"op":"set","id":"p","visible":false}"#,
            r##"{"op":"palette","add":"#ffffff","name":"paper"}"##,
            r#"{"op":"add","cel":{"frame":3},"in":"p"}"#,
        ];
        cases.iter().for_each(|line| {
            let after = edited(&doc, line);
            let patch = round_trip(&doc, &after);
            assert!(!patch.is_empty(), "{line}");
            let size = serde_json::to_string(&patch).unwrap().len();
            assert!(size < 800, "{line}: {size} bytes");
        });
        assert!(diff(&doc, &doc).unwrap().is_empty());
    }

    #[test]
    fn effects_and_adjustments_undo_and_redo() {
        let doc = edited(
            &sample(),
            r#"{"op":"add","layer":{"id":"k","kind":"adjust","name":"dusk","adjust":{"curves":{}}}}"#,
        );
        [
            r#"{"op":"set","id":"v","effects":[{"glow":{"radius":8}},{"outline":{"width":2}}]}"#,
            r#"{"op":"set","id":"k","adjust":{"levels":{"black":0.1}},"opacity":0.6}"#,
            r#"{"op":"set","id":"k","adjust":{"curves":{"points":[[0,0],[0.5,0.2],[1,1]]}}}"#,
            r#"{"op":"set","id":"g","blend":"soft-light"}"#,
            r#"{"op":"add","node":{"type":"rays","x":10,"y":0,"count":5},"in":"v"}"#,
            r##"{"op":"atmosphere","color":"#a0b0c0","density":0.8}"##,
            r#"{"op":"set","id":"g","depth":2.5,"reference":true}"#,
            r##"{"op":"add","node":{"type":"scatter","w":100,"h":50,"count":9,"children":[{"type":"ellipse","cx":0,"cy":0,"rx":4,"ry":3,"fill":"#406020"}]},"in":"v"}"##,
            r#"{"op":"add","node":{"type":"mirror","x":50,"children":[{"type":"path","d":"M10 10 L40 30"}]},"in":"v"}"#,
            r#"{"op":"add","node":{"type":"text","text":"Inn","font":"sign.ttf","size":40},"in":"v"}"#,
            r#"{"op":"add","node":{"type":"image","src":"ab12.png","w":64,"h":32},"in":"v"}"#,
        ]
        .iter()
        .for_each(|line| {
            let after = edited(&doc, line);
            assert!(!round_trip(&doc, &after).is_empty(), "{line}");
        });
    }

    #[test]
    fn nested_changes_point_at_the_node() {
        let doc = sample();
        let after = edited(&doc, r#"{"op":"set","id":"c","d":"M1 1 L2 2"}"#);
        let patch = diff(&doc, &after).unwrap();
        assert_eq!(patch.0.len(), 1);
        assert_eq!(patch.0[0].at, "/layers/0/cels/0/nodes/1/children/1");
        assert_eq!(patch.0[0].new, serde_json::json!({"d": "M1 1 L2 2"}));
    }

    #[test]
    fn patches_name_the_cels_whose_nodes_they_touch() {
        let doc = sample();
        let cels = |line: &str| diff(&doc, &edited(&doc, line)).unwrap().touched_cels(&doc);
        let first = doc.layers[0].cels()[0].id.clone();
        assert_eq!(
            cels(r#"{"op":"set","id":"c","d":"M1 1 L2 2"}"#),
            Some([first.clone()].into())
        );
        assert_eq!(cels(r#"{"op":"rm","id":"d"}"#), Some([first].into()));
        assert_eq!(cels(r#"{"op":"set","id":"v","opacity":0.5}"#), None);
        assert_eq!(cels(r#"{"op":"add","cel":{"frame":3},"in":"p"}"#), None);
    }

    #[test]
    fn a_stored_patch_still_matches_after_its_numbers_go_through_text() {
        let doc = sample();
        let moved = edited(
            &doc,
            r#"{"op":"set","id":"d","transform.x":17.841812133789062,"transform.y":121.22406005859375,"transform.sx":0.6802818775177002,"transform.sy":0.6802818775177002}"#,
        );
        let scaled = edited(&moved, r#"{"op":"set","id":"d","transform.sx":1.9749854803085327}"#);
        let restore = diff(&scaled, &moved).unwrap();
        let text = serde_json::to_string(&restore).unwrap();
        let stored: Patch = serde_json::from_str(&text).unwrap();
        let reloaded: Document = serde_json::from_str(&serde_json::to_string(&scaled).unwrap()).unwrap();
        assert_eq!(
            stored.applied(&reloaded).unwrap(),
            Document {
                rev: reloaded.rev,
                ..moved
            }
        );
        assert!(same(
            &serde_json::json!(121.22406005859375),
            &serde_json::json!(121.22406005859376)
        ));
        assert!(!same(&serde_json::json!(121.2), &serde_json::json!(121.3)));
    }

    #[test]
    fn patches_refuse_a_document_that_moved_on() {
        let doc = sample();
        let after = edited(&doc, r#"{"op":"set","id":"a","x":5}"#);
        let patch = diff(&doc, &after).unwrap();
        let other = edited(&doc, r#"{"op":"set","id":"a","x":7}"#);
        let err = patch.applied(&other).unwrap_err();
        assert!(format!("{err:#}").contains("not what this change expects"), "{err:#}");
        let removed = edited(&doc, r#"{"op":"rm","id":"d"}"#);
        let add_back = diff(&removed, &doc).unwrap();
        assert!(add_back.applied(&doc).is_err());
        assert_eq!(
            add_back.applied(&removed).unwrap(),
            Document {
                rev: removed.rev,
                ..doc
            }
        );
    }
}
