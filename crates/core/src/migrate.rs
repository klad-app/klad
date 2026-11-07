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
        "klad/1" => {
            let mut value = value;
            value["format"] = Value::from(FORMAT);
            Ok(value)
        }
        old if old.starts_with("draw-anything/") => bail!(
            "a drawing in the old .draw format ({old}); this klad opens {FORMAT} drawings (.klad) only and does not convert old ones"
        ),
        "" => bail!("not a klad drawing: doc.json has no format"),
        other => bail!("format {other} is not {FORMAT}: made by another or a newer klad"),
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
        let first = json!({"format": "klad/1", "rev": 3, "layers": [{"id": "l1", "bones": [{"name": "hip", "head": [0, 0], "tail": [0, 9]}]}]});
        let mut moved = first.clone();
        moved["format"] = json!(FORMAT);
        assert_eq!(
            migrate(first).unwrap(),
            moved,
            "a klad/1 drawing opens as it was, in the new format"
        );
        assert!(
            migrate(json!({"format": "klad/7"}))
                .unwrap_err()
                .to_string()
                .contains("newer klad")
        );
        assert!(migrate(json!({})).unwrap_err().to_string().contains("no format"));
    }
}
