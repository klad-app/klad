use crate::doc::{Document, Layer, LayerContent, Node};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Renamed {
    pub what: &'static str,
    pub from: String,
    pub to: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsedTwice;

impl std::fmt::Display for UsedTwice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "klad repair gives every later one a new id")
    }
}

impl std::error::Error for UsedTwice {}

fn layers_mut(layers: &mut [Layer], f: &mut dyn FnMut(&mut Layer)) {
    layers.iter_mut().for_each(|layer| {
        f(layer);
        if let Some(mask) = layer.mask.as_deref_mut() {
            f(mask);
        }
        if let LayerContent::Group { children } = &mut layer.content {
            layers_mut(children, f);
        }
    });
}

fn nodes_mut(nodes: &mut [Node], f: &mut dyn FnMut(&mut Node)) {
    nodes.iter_mut().for_each(|node| {
        f(node);
        if let Some(children) = node.shape.children_mut() {
            nodes_mut(children, f);
        }
    });
}

fn follow(slot: &mut Option<String>, renames: &[(String, String)], kept: &dyn Fn(&str) -> bool) {
    if let Some(id) = slot.as_mut() {
        follow_id(id, renames, kept);
    }
}

fn follow_id(id: &mut String, renames: &[(String, String)], kept: &dyn Fn(&str) -> bool) {
    if let Some((_, to)) = renames.iter().find(|(from, _)| from == id && !kept(from)) {
        *id = to.clone();
    }
}

pub fn ids(doc: &mut Document) -> Vec<Renamed> {
    let mut taken = doc.ids();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut layers = std::mem::take(&mut doc.layers);
    layers_mut(&mut layers, &mut |layer| {
        if !seen.insert(layer.id.clone()) {
            let to = doc.alloc_id_in('l', &mut taken);
            out.push(Renamed {
                what: "layer",
                from: std::mem::replace(&mut layer.id, to.clone()),
                to,
                name: layer.name.clone(),
            });
        }
    });
    layers_mut(&mut layers, &mut |layer| {
        let name = layer.name.clone();
        let Some(cels) = layer.cels_mut() else {
            return;
        };
        let mut renames = Vec::new();
        cels.iter_mut().for_each(|cel| {
            if !seen.insert(cel.id.clone()) {
                let to = doc.alloc_id_in('c', &mut taken);
                let from = std::mem::replace(&mut cel.id, to.clone());
                out.push(Renamed {
                    what: "drawing",
                    from: from.clone(),
                    to: to.clone(),
                    name: format!("{name}, frame {}", cel.frame),
                });
                renames.push((from, to));
            }
        });
        let here: BTreeSet<String> = cels.iter().map(|c| c.id.clone()).collect();
        let kept = |id: &str| here.contains(id);
        cels.iter_mut().for_each(|cel| {
            follow(&mut cel.link, &renames, &kept);
            if let Some(tween) = cel.tween.as_mut() {
                follow_id(&mut tween.from, &renames, &kept);
                follow_id(&mut tween.to, &renames, &kept);
            }
            if let Some(pose) = cel.pose.as_mut() {
                follow_id(&mut pose.rest, &renames, &kept);
            }
        });
    });
    layers_mut(&mut layers, &mut |layer| {
        let Some(cels) = layer.cels_mut() else {
            return;
        };
        cels.iter_mut().for_each(|cel| {
            let mut renames = Vec::new();
            nodes_mut(&mut cel.nodes, &mut |node| {
                if !seen.insert(node.id.clone()) {
                    let to = doc.alloc_id_in('n', &mut taken);
                    let from = std::mem::replace(&mut node.id, to.clone());
                    out.push(Renamed {
                        what: "shape",
                        from: from.clone(),
                        to: to.clone(),
                        name: match node.name.is_empty() {
                            true => node.type_name().to_string(),
                            false => node.name.clone(),
                        },
                    });
                    renames.push((from, to));
                }
            });
            if renames.is_empty() {
                return;
            }
            let mut here = BTreeSet::new();
            nodes_mut(&mut cel.nodes, &mut |node| {
                here.insert(node.id.clone());
            });
            nodes_mut(&mut cel.nodes, &mut |node| {
                follow(&mut node.clip, &renames, &|id: &str| here.contains(id));
            });
        });
    });
    doc.layers = layers;
    out
}

pub fn cel_files(doc: &Document) -> Vec<(String, Vec<String>)> {
    doc.all_layers()
        .iter()
        .map(|l| (l.id.clone(), l.cels().iter().map(|c| c.id.clone()).collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Cel, Shape};

    fn square(id: &str) -> Node {
        Node::new(
            id,
            Shape::Rect {
                x: 0.0,
                y: 0.0,
                w: 4.0,
                h: 4.0,
                radius: 0.0,
            },
        )
    }

    #[test]
    fn the_first_keeps_its_id_and_every_later_one_gets_a_new_one() {
        let mut doc = Document::new(64, 64);
        doc.layers.push(Layer::paint("l48qa", "mine", "c49qa"));
        let mut theirs = Layer::paint("l48qa", "theirs", "c49qa");
        if let Some(cels) = theirs.cels_mut() {
            cels.push(Cel {
                link: Some("c49qa".into()),
                ..Cel::at("c50qa", 4)
            });
        }
        doc.layers.push(theirs);
        doc.layers.push(Layer::vector(
            "l60",
            "shapes",
            "c61",
            vec![square("n62"), square("n62")],
        ));
        doc.next_id = 62;
        assert!(doc.validate().unwrap_err().to_string().contains("used twice"));
        let renamed = ids(&mut doc);
        doc.validate().unwrap();
        assert_eq!(
            renamed
                .iter()
                .map(|r| (r.what, r.from.as_str(), r.to.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("layer", "l48qa", "l63"),
                ("drawing", "c49qa", "c64"),
                ("shape", "n62", "n65")
            ]
        );
        assert_eq!(doc.layers[0].id, "l48qa", "the first one keeps it");
        assert_eq!(
            doc.layers[1].cels()[1].link.as_deref(),
            Some("c64"),
            "a link in its own layer follows it"
        );
        assert!(ids(&mut doc).is_empty(), "nothing left to repair");
    }
}
