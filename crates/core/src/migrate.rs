use crate::doc::FORMAT;
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

const V0: &str = "draw-anything/0";
const V1: &str = "draw-anything/1";
const V2: &str = "draw-anything/2";
const V3: &str = "draw-anything/3";
const V4: &str = "draw-anything/4";
const V5: &str = "draw-anything/5";
const V6: &str = "draw-anything/6";
const V7: &str = "draw-anything/7";
const V8: &str = "draw-anything/8";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMove {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Migrated {
    pub value: Value,
    pub moves: Vec<FileMove>,
    pub embed_m1_brushes: bool,
}

impl Migrated {
    pub fn changed(&self) -> bool {
        !self.moves.is_empty() || self.embed_m1_brushes
    }
}

pub fn migrate(value: Value) -> Result<Migrated> {
    let format = value
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match format.as_str() {
        FORMAT => Ok(Migrated {
            value,
            moves: Vec::new(),
            embed_m1_brushes: false,
        }),
        V2 | V3 | V4 | V5 | V6 | V7 | V8 => Ok(Migrated {
            value: to_current(value),
            moves: Vec::new(),
            embed_m1_brushes: false,
        }),
        V1 => Ok(Migrated {
            value: to_current(value),
            moves: Vec::new(),
            embed_m1_brushes: true,
        }),
        V0 => {
            let (v1, moves) = from_v0(&value);
            Ok(Migrated {
                value: to_current(v1),
                moves,
                embed_m1_brushes: true,
            })
        }
        other => bail!("unknown document format {other:?}"),
    }
}

fn to_current(mut value: Value) -> Value {
    value["format"] = Value::String(FORMAT.to_string());
    value
}

struct Ids {
    taken: BTreeSet<String>,
    next: u64,
}

impl Ids {
    fn alloc(&mut self, prefix: char) -> String {
        loop {
            self.next += 1;
            let id = format!("{prefix}{}", self.next);
            if self.taken.insert(id.clone()) {
                return id;
            }
        }
    }
}

fn strokes(layer: &str, cel: &str) -> PathBuf {
    PathBuf::from("strokes").join(layer).join(format!("{cel}.strokes"))
}

fn from_v0(v0: &Value) -> (Value, Vec<FileMove>) {
    let layers = v0["layers"].as_array().cloned().unwrap_or_default();
    let mut ids = Ids {
        taken: layers
            .iter()
            .filter_map(|l| l["id"].as_str().map(String::from))
            .collect(),
        next: 0,
    };
    let mut moves = Vec::new();
    let mut converted = Vec::new();
    for layer in &layers {
        let id = layer["id"].as_str().unwrap_or_default().to_string();
        let kind = layer["kind"].as_str().unwrap_or("paint");
        let cel_id = ids.alloc('c');
        let mut cel = json!({ "id": cel_id, "frame": 0 });
        if kind == "vector" {
            let nodes: Vec<Value> = layer["shapes"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut shape| {
                    shape["id"] = Value::String(ids.alloc('n'));
                    shape
                })
                .collect();
            cel["nodes"] = Value::Array(nodes);
        } else {
            moves.push(FileMove {
                from: strokes(&id, "c0"),
                to: strokes(&id, &cel_id),
            });
        }
        let mut out = Map::new();
        out.insert("id".into(), Value::String(id));
        out.insert("name".into(), layer["name"].clone());
        out.insert("kind".into(), Value::String(kind.to_string()));
        out.insert("cels".into(), json!([cel]));
        ["opacity", "blend", "visible"]
            .iter()
            .filter(|k| layer.get(**k).is_some())
            .for_each(|k| {
                out.insert((*k).to_string(), layer[*k].clone());
            });
        converted.push(Value::Object(out));
    }
    let doc = json!({
        "format": V1,
        "rev": v0["rev"].as_u64().unwrap_or(0),
        "canvas": v0["canvas"].clone(),
        "timeline": { "fps": 12, "frames": 1 },
        "layers": converted,
        "nextId": ids.next,
    });
    (doc, moves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, Kind, Shape};

    #[test]
    fn v0_documents_become_v1_with_cels_and_node_ids() {
        let v0 = json!({
            "format": "draw-anything/0", "rev": 4,
            "canvas": { "width": 64, "height": 32, "background": "#ffffff" },
            "layers": [
                { "id": "sky", "name": "sky", "kind": "vector",
                  "shapes": [{ "type": "rect", "x": 0, "y": 0, "w": 64, "h": 32, "fill": "#336699" }] },
                { "id": "c1", "name": "ink", "kind": "paint", "blend": "multiply", "opacity": 0.5 }
            ]
        });
        let out = migrate(v0).unwrap();
        assert!(out.embed_m1_brushes);
        let moves = out.moves;
        let doc: Document = serde_json::from_value(out.value).unwrap();
        doc.validate().unwrap();
        assert_eq!(doc.rev, 4);
        assert_eq!(doc.layers[0].kind(), Kind::Vector);
        let node = &doc.layers[0].cels()[0].nodes[0];
        assert!(matches!(node.shape, Shape::Rect { w, .. } if w == 64.0));
        assert!(node.id.starts_with('n'));
        let ink = &doc.layers[1];
        assert_eq!(ink.opacity, 0.5);
        assert_ne!(ink.cels()[0].id, "c1");
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from, PathBuf::from("strokes/c1/c0.strokes"));
        assert_eq!(
            moves[0].to,
            PathBuf::from(format!("strokes/c1/{}.strokes", ink.cels()[0].id))
        );
    }

    #[test]
    fn current_documents_pass_through_and_unknown_formats_fail() {
        let current = json!({ "format": FORMAT, "rev": 1 });
        let same = migrate(current.clone()).unwrap();
        assert_eq!(same.value, current);
        assert!(!same.changed());
        assert!(migrate(json!({ "format": "other/9" })).is_err());
    }

    #[test]
    fn v6_documents_only_change_format_and_have_no_parts() {
        let v6 = json!({ "format": "draw-anything/6", "rev": 2, "canvas": { "width": 8, "height": 8 },
            "timeline": { "fps": 12, "frames": 2 },
            "layers": [{ "id": "l1", "name": "lines", "kind": "paint", "cels": [{ "id": "c1", "frame": 0 }] }] });
        let out = migrate(v6).unwrap();
        assert_eq!(out.value["format"], FORMAT);
        assert!(!out.changed());
        let doc: crate::doc::Document = serde_json::from_value(out.value).unwrap();
        assert!(doc.layers[0].cels()[0].parts.is_empty());
    }

    #[test]
    fn v5_documents_only_change_format_and_read_tweens_after() {
        let v5 = json!({ "format": "draw-anything/5", "rev": 4, "canvas": { "width": 8, "height": 8 },
            "timeline": { "fps": 12, "frames": 4, "origin": [4, 6] },
            "layers": [{ "id": "l1", "name": "lines", "kind": "paint",
                "cels": [{ "id": "c1", "frame": 0, "hold": 1 }, { "id": "c2", "frame": 2 }] }] });
        let out = migrate(v5).unwrap();
        assert_eq!(out.value["format"], FORMAT);
        assert!(!out.changed());
        let mut doc: crate::doc::Document = serde_json::from_value(out.value).unwrap();
        doc.validate().unwrap();
        assert!(doc.layers[0].cels().iter().all(|c| c.tween.is_none()));
        let cels = doc.layers[0].cels_mut().unwrap();
        cels.insert(
            1,
            crate::doc::Cel {
                tween: Some(crate::doc::Tween {
                    from: "c1".into(),
                    to: "c2".into(),
                    t: 0.5,
                    matching: String::new(),
                    pairs: Vec::new(),
                    made: "ab12".into(),
                    unpaired: String::new(),
                }),
                hold: Some(1),
                ..crate::doc::Cel::at("c3", 1)
            },
        );
        doc.validate().unwrap();
        let text = serde_json::to_string(&doc.layers[0].cels()[1]).unwrap();
        assert_eq!(
            text,
            r#"{"id":"c3","frame":1,"hold":1,"tween":{"from":"c1","to":"c2","t":0.5,"made":"ab12"}}"#
        );
    }

    #[test]
    fn v4_documents_only_change_format() {
        let v4 = json!({ "format": "draw-anything/4", "rev": 9, "canvas": { "width": 8, "height": 8 },
            "timeline": { "fps": 12, "frames": 4, "clips": [{ "name": "run", "from": 0, "to": 3, "loop": true }] },
            "layers": [] });
        let out = migrate(v4).unwrap();
        assert_eq!(out.value["format"], FORMAT);
        assert!(!out.changed());
        let doc: crate::doc::Document = serde_json::from_value(out.value).unwrap();
        doc.validate().unwrap();
        assert_eq!(doc.timeline.clips[0].fps, None);
        assert_eq!(doc.timeline.fps_of(&doc.timeline.clips[0]), 12);
    }

    #[test]
    fn v3_documents_only_change_format() {
        let v3 = json!({ "format": "draw-anything/3", "rev": 5, "canvas": { "width": 8, "height": 8 }, "layers": [] });
        let out = migrate(v3).unwrap();
        assert_eq!(out.value["format"], FORMAT);
        assert!(!out.changed());
    }

    #[test]
    fn v2_documents_only_change_format() {
        let v2 = json!({ "format": "draw-anything/2", "rev": 3, "canvas": { "width": 8, "height": 8 }, "layers": [] });
        let out = migrate(v2).unwrap();
        assert_eq!(out.value["format"], FORMAT);
        assert!(out.moves.is_empty() && !out.embed_m1_brushes);
        assert_eq!(out.value["rev"], 3);
    }

    #[test]
    fn v1_documents_only_change_format_and_ask_for_m1_brushes() {
        let v1 = json!({ "format": "draw-anything/1", "rev": 7, "canvas": { "width": 8, "height": 8 }, "layers": [] });
        let out = migrate(v1).unwrap();
        assert_eq!(out.value["format"], FORMAT);
        assert_eq!(out.value["rev"], 7);
        assert!(out.moves.is_empty() && out.embed_m1_brushes && out.changed());
    }
}
