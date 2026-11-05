use crate::agent::{self, Output, parse_props};
use anyhow::{Result, anyhow, bail};
use klad_core::bundle::Bundle;
use klad_core::look::{ADJUSTMENTS, Adjustment, EFFECTS, Effect};
use klad_core::ops::Op;
use serde_json::{Map, Value, json};
use std::path::Path;

const LAYER_KEYS: [&str; 9] = [
    "id", "name", "opacity", "blend", "visible", "locked", "clip", "mask", "effects",
];

fn with_props(mut value: Value, props: Map<String, Value>) -> Result<Value> {
    let inner = value
        .as_object_mut()
        .and_then(|m| m.values_mut().next())
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("cannot set fields here"))?;
    props.into_iter().for_each(|(k, v)| {
        inner.insert(k, v);
    });
    Ok(value)
}

pub fn effect_of(kind: &str, props: &[String]) -> Result<Effect> {
    let start = Effect::named(kind).ok_or_else(|| anyhow!("no effect {kind:?}: use {}", EFFECTS.join(", ")))?;
    let effect: Effect = serde_json::from_value(with_props(serde_json::to_value(&start)?, parse_props(props)?)?)?;
    effect.validate()?;
    Ok(effect)
}

pub fn adjust_layer(rest: &[String]) -> Result<Value> {
    let Some((kind, props)) = rest.split_first().filter(|(k, _)| !k.contains('=')) else {
        bail!(
            "say which adjustment: klad add <bundle> layer adjust {} [key=value ...]",
            ADJUSTMENTS.join("|")
        );
    };
    let start =
        Adjustment::named(kind).ok_or_else(|| anyhow!("no adjustment {kind:?}: use {}", ADJUSTMENTS.join(", ")))?;
    let (own, layer): (Map<String, Value>, Map<String, Value>) = parse_props(props)?
        .into_iter()
        .partition(|(k, _)| !LAYER_KEYS.contains(&k.split('.').next().unwrap_or("")));
    let adjust: Adjustment = serde_json::from_value(with_props(serde_json::to_value(&start)?, own)?)?;
    adjust.validate()?;
    let mut value = agent::nest(layer);
    value["kind"] = json!("adjust");
    value["adjust"] = serde_json::to_value(&adjust)?;
    if value.get("name").is_none() {
        value["name"] = json!(adjust.label());
    }
    Ok(value)
}

pub enum Change {
    List,
    Add(String, Vec<String>),
    Remove(usize),
    Clear,
}

pub fn effect(path: &Path, layer: &str, change: Change, out: &Output) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let found = bundle.doc.find_layer(layer)?;
    let mut list = found.effects.clone();
    match change {
        Change::List => {
            if list.is_empty() {
                println!("layer {layer} has no effects; add one of {}", EFFECTS.join(", "));
            }
            list.iter()
                .enumerate()
                .try_for_each(|(i, e)| serde_json::to_string(e).map(|json| println!("{i}: {json}")))?;
            return Ok(());
        }
        Change::Add(kind, props) => list.push(effect_of(&kind, &props)?),
        Change::Remove(i) if i < list.len() => {
            list.remove(i);
        }
        Change::Remove(i) => bail!("layer {layer} has {} effects; there is no {i}", list.len()),
        Change::Clear => list.clear(),
    }
    let props: Map<String, Value> = [("effects".to_string(), serde_json::to_value(&list)?)]
        .into_iter()
        .collect();
    agent::edit(
        path,
        &[Op::Set {
            id: found.id.clone(),
            props,
        }],
        out,
    )
}

fn imported(bundle: &Path, props: &mut Map<String, Value>) -> Result<Value> {
    let file = match props.remove("src") {
        Some(Value::String(s)) => s,
        _ => bail!("say which picture: src=photo.jpg (a PNG or JPEG file)"),
    };
    let bytes = std::fs::read(&file).map_err(|e| anyhow!("reading {file}: {e}"))?;
    let doc = Bundle::open(bundle)?.doc;
    let (src, w, h) = klad_render::assets::import_image(bundle, &bytes)?;
    let (cw, ch) = (doc.canvas.width as f32, doc.canvas.height as f32);
    let fit = (cw / w as f32).min(ch / h as f32).min(1.0);
    let (pw, ph) = (w as f32 * fit, h as f32 * fit);
    let take = |props: &mut Map<String, Value>, key: &str, or: f32| {
        props.remove(key).and_then(|v| v.as_f64()).map_or(or, |v| v as f32)
    };
    let (x, y) = (take(props, "x", (cw - pw) / 2.0), take(props, "y", (ch - ph) / 2.0));
    let (pw, ph) = (take(props, "w", pw), take(props, "h", ph));
    Ok(json!({ "type": "image", "src": src, "x": x, "y": y, "w": pw, "h": ph }))
}

pub fn image_layer(bundle: &Path, props: &[String]) -> Result<Value> {
    let mut props = parse_props(props)?;
    let file = props.get("src").and_then(Value::as_str).unwrap_or_default().to_string();
    let node = imported(bundle, &mut props)?;
    let mut layer = agent::nest(props);
    layer["kind"] = json!("vector");
    if layer.get("name").is_none() {
        let stem = Path::new(&file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("picture");
        layer["name"] = json!(stem);
    }
    layer["cels"] = json!([{ "frame": 0, "nodes": [node] }]);
    Ok(layer)
}

pub fn image_node(bundle: &Path, props: &[String]) -> Result<Value> {
    let mut props = parse_props(props)?;
    let mut node = imported(bundle, &mut props)?;
    props.into_iter().for_each(|(k, v)| node[k] = v);
    Ok(node)
}

pub fn atmosphere(path: &Path, props: &[String], off: bool, out: &Output) -> Result<()> {
    let bundle = Bundle::open(path)?;
    if props.is_empty() && !off {
        match &bundle.doc.atmosphere {
            Some(air) => println!("{}", serde_json::to_string(air)?),
            None => {
                println!("no atmosphere: klad atmosphere <bundle> color=#9aa8b8 density=0.5 blur=1 desaturate=0.15")
            }
        }
        return Ok(());
    }
    let op = match off {
        true => Op::Atmosphere(None),
        false => {
            let start = serde_json::to_value(bundle.doc.atmosphere.clone().unwrap_or_default())?;
            let mut map = start.as_object().cloned().unwrap_or_default();
            parse_props(props)?.into_iter().for_each(|(k, v)| {
                map.insert(k, v);
            });
            Op::Atmosphere(Some(serde_json::from_value(Value::Object(map))?))
        }
    };
    agent::edit(path, &[op], out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_layer_imports_its_file_and_fits_the_canvas() {
        let dir = std::env::temp_dir().join(format!("klad-cli-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bundle = dir.join("art.draw");
        Bundle::create(&bundle, klad_core::doc::Document::new(200, 100)).unwrap();
        let rgba = klad_render::assets::Rgba {
            width: 400,
            height: 100,
            pixels: vec![120; 400 * 100 * 4],
        };
        let file = dir.join("wide.png");
        std::fs::write(&file, klad_render::assets::encode_png(&rgba).unwrap()).unwrap();
        let layer = image_layer(&bundle, &[format!("src={}", file.display()), "reference=true".into()]).unwrap();
        assert_eq!(
            (layer["kind"].clone(), layer["name"].clone(), layer["reference"].clone()),
            (json!("vector"), json!("wide"), json!(true))
        );
        let node = &layer["cels"][0]["nodes"][0];
        assert_eq!(
            (node["w"].as_f64(), node["h"].as_f64(), node["y"].as_f64()),
            (Some(200.0), Some(50.0), Some(25.0))
        );
        assert!(bundle.join("images").join(node["src"].as_str().unwrap()).exists());
        assert!(image_layer(&bundle, &["name=x".into()]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn effects_and_adjustments_take_their_fields() {
        let glow = effect_of("glow", &["radius=30".into(), "color=#ffd080".into()]).unwrap();
        assert!(matches!(glow, Effect::Glow(ref g) if g.radius == 30.0 && g.color.as_deref() == Some("#ffd080")));
        assert!(effect_of("glow", &["radios=3".into()]).is_err());
        assert!(effect_of("sparkle", &[]).is_err());
        let layer = adjust_layer(&[
            "curves".into(),
            "points=[[0,0],[0.5,0.3],[1,1]]".into(),
            "name=dusk".into(),
            "opacity=0.7".into(),
        ])
        .unwrap();
        assert_eq!(layer["name"], "dusk");
        assert_eq!(layer["opacity"], 0.7);
        assert!((layer["adjust"]["curves"]["points"][1][1].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert!(adjust_layer(&["levels".into(), "white=0.1".into(), "black=0.5".into()]).is_err());
    }
}
