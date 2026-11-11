use crate::doc::FORMAT;
use anyhow::{Result, bail};
use serde_json::Value;

pub fn migrate(value: Value) -> Result<Value> {
    let format = value
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match format.as_str() {
        FORMAT => Ok(value),
        "klad/4" | "klad/5" | "klad/6" => Ok(uncount(value)),
        "klad/1" | "klad/2" | "klad/3" => {
            let mut value = value;
            if let Some(layers) = value.get_mut("layers").and_then(Value::as_array_mut) {
                layers.iter_mut().for_each(unrig);
            }
            Ok(uncount(value))
        }
        old if old.starts_with("draw-anything/") => bail!(
            "a drawing in the old .draw format ({old}); this klad opens {FORMAT} drawings (.klad) only and does not convert old ones"
        ),
        "" => bail!("not a klad drawing: doc.json has no format"),
        other => bail!("format {other} is not {FORMAT}: made by another or a newer klad"),
    }
}

fn uncount(mut value: Value) -> Value {
    value["format"] = Value::from(FORMAT);
    if let Some(map) = value.as_object_mut() {
        map.remove("rev");
        map.remove("nextId");
    }
    value
}

fn unrig(layer: &mut Value) {
    let Some(map) = layer.as_object_mut() else {
        return;
    };
    ["bones", "ik", "bone", "rig"].iter().for_each(|k| {
        map.remove(*k);
    });
    if let Some(cels) = map.get_mut("cels").and_then(Value::as_array_mut) {
        cels.iter_mut().filter_map(Value::as_object_mut).for_each(|c| {
            c.remove("pose");
        });
    }
    ["children"].iter().for_each(|k| {
        if let Some(list) = map.get_mut(*k).and_then(Value::as_array_mut) {
            list.iter_mut().for_each(unrig);
        }
    });
    if let Some(mask) = map.get_mut("mask") {
        unrig(mask);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_documents_pass_and_old_ones_are_refused_with_the_reason() {
        let current = json!({"format": FORMAT, "rev": 1});
        assert_eq!(migrate(current.clone()).unwrap(), current);
        let old = migrate(json!({"format": "draw-anything/9"})).unwrap_err().to_string();
        assert!(old.contains("old .draw format") && old.contains(FORMAT), "{old}");
        let first = json!({"format": "klad/1", "rev": 3, "layers": [{"id": "l1", "kind": "paint", "cels": [{"id": "c1", "frame": 0}]}]});
        let mut moved = first.clone();
        moved["format"] = json!(FORMAT);
        moved.as_object_mut().unwrap().remove("rev");
        assert_eq!(
            migrate(json!({"format": "klad/4", "rev": 9, "nextId": 40, "layers": []})).unwrap(),
            json!({"format": FORMAT, "layers": []}),
            "klad/5 keeps no counters in doc.json: they live in the copy's .cache"
        );
        assert_eq!(
            migrate(json!({"format": "klad/5", "layers": []})).unwrap(),
            json!({"format": FORMAT, "layers": []}),
            "klad/6 adds attachments to stroke files; a klad/5 drawing opens unchanged"
        );
        assert_eq!(
            migrate(first).unwrap(),
            moved,
            "a klad/1 drawing opens as it was, in the new format"
        );
        let rigged = json!({"format": "klad/3", "layers": [{"id": "g1", "kind": "group", "bones": [{"name": "hip", "head": [0, 0], "tail": [0, 9]}], "ik": [], "children": [
            {"id": "l1", "kind": "paint", "bone": "hip", "cels": [{"id": "c1", "frame": 0}, {"id": "c2", "frame": 4, "pose": {"rest": "c1", "angles": {"hip": 20}}}]}
        ]}]});
        let opened = migrate(rigged).unwrap();
        assert_eq!(
            opened,
            json!({"format": FORMAT, "layers": [{"id": "g1", "kind": "group", "children": [
                {"id": "l1", "kind": "paint", "cels": [{"id": "c1", "frame": 0}, {"id": "c2", "frame": 4}]}
            ]}]}),
            "rigs from before bones layers are dropped; the drawings, posed ones too, stay"
        );
        assert_eq!(
            migrate(json!({"format": "klad/6", "layers": []})).unwrap(),
            json!({"format": FORMAT, "layers": []}),
            "klad/7 adds pixel layers and palette ids; a klad/6 drawing opens unchanged"
        );
        assert!(
            migrate(json!({"format": "klad/8"}))
                .unwrap_err()
                .to_string()
                .contains("newer klad")
        );
        assert!(migrate(json!({})).unwrap_err().to_string().contains("no format"));
    }
}
