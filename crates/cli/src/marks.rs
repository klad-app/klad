use anyhow::{Result, anyhow, bail};
use klad_core::bundle::Bundle;
use klad_core::cut::{Cut, Keep};
use klad_core::doc::Kind;
use klad_core::fill::{Fill, FillSource};
use klad_core::markedit::{Side, address, sides};
use klad_core::stroke::{Mark, Stroke};
use serde_json::{Value, json};
use std::path::Path;

pub fn hex8(rgba: [f32; 4]) -> String {
    let [r, g, b, a] = rgba.map(|c| (c * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

pub fn round4(v: f32) -> f64 {
    (v as f64 * 1e4).round() / 1e4
}

pub fn fill_json(cel: &str, f: &Fill) -> Value {
    let mut value = json!({
        "id": f.id,
        "address": address(cel, f.id),
        "author": f.author.name(),
        "color": hex8(f.color()),
        "opacity": round4(f.opacity),
        "bounds": f.bounds(),
        "pixels": f.coverage.pixels(),
    });
    match &f.source {
        FillSource::Seed { seed, reference } => {
            value["at"] = json!([round4(seed.x), round4(seed.y)]);
            value["ref"] = json!(reference.name());
            value["tolerance"] = json!(round4(seed.tolerance));
            value["gap"] = json!(round4(seed.gap));
            value["expand"] = json!(round4(seed.expand));
        }
        FillSource::Polygon(points) => value["polygon"] = json!(points),
    }
    flags_json(&mut value, f.lock, &f.cuts);
    value
}

pub fn flags(lock: bool, cuts: &[Cut]) -> String {
    let lock = lock.then_some("lock alpha".to_string());
    let cuts = cuts.iter().map(|c| {
        format!(
            "keeps {} a lasso",
            if c.keep == Keep::Inside { "inside" } else { "outside" }
        )
    });
    let all: Vec<String> = lock.into_iter().chain(cuts).collect();
    if all.is_empty() {
        String::new()
    } else {
        format!("  ({})", all.join(", "))
    }
}

fn flags_json(value: &mut Value, lock: bool, cuts: &[Cut]) {
    if lock {
        value["lockAlpha"] = json!(true);
    }
    if !cuts.is_empty() {
        value["cuts"] = Value::Array(
            cuts.iter()
                .map(|c| {
                    json!({
                        "keep": if c.keep == Keep::Inside { "inside" } else { "outside" },
                        "polygon": c.polygon,
                    })
                })
                .collect(),
        );
    }
}

pub fn lassoed(path: &Path, layer: &str, frame: u32, polygon: &[[f32; 2]], as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let owner = bundle.doc.find_layer(layer)?;
    if owner.kind() != Kind::Paint {
        bail!(
            "{layer} is a {} layer; the lasso works on paint layers",
            owner.kind().name()
        );
    }
    let cel = owner
        .content_at(frame)
        .ok_or_else(|| anyhow!("layer {layer} has no cel at frame {frame}"))?
        .id
        .clone();
    let marks = bundle.read_marks(&owner.id, &cel)?;
    let found = sides(&marks, polygon);
    let pick = |side: Side| -> Vec<String> {
        found
            .iter()
            .filter(|(_, s)| *s == side)
            .map(|(id, _)| address(&cel, *id))
            .collect()
    };
    let (inside, crossing) = (pick(Side::Inside), pick(Side::Crossing));
    if as_json {
        println!("{}", json!({ "cel": cel, "inside": inside, "crossing": crossing }));
        return Ok(());
    }
    println!("inside ({}): {}", inside.len(), inside.join(" "));
    println!("crossing the edge ({}): {}", crossing.len(), crossing.join(" "));
    println!("move them with --move, --rotate, --scale or --flip (add --cut to take the crossing parts), or --rm");
    Ok(())
}

pub fn stroke_json(cel: &str, s: &Stroke, points: bool) -> Value {
    let mut value = json!({
        "id": s.id,
        "address": address(cel, s.id),
        "author": s.author.name(),
        "brush": s.brush,
        "color": hex8(s.color()),
        "size": s.size,
        "opacity": s.opacity,
        "flow": s.flow,
        "taper": s.taper,
        "bounds": s.bounds(),
    });
    flags_json(&mut value, s.lock, &s.cuts);
    if let Some(second) = s.second_color() {
        value["second"] = json!(hex8(second));
    }
    if points {
        value["points"] = json!(
            s.samples
                .iter()
                .map(|v| {
                    let (x, y) = v.pos();
                    [x, y, v.pressure()]
                })
                .collect::<Vec<_>>()
        );
        let line: Vec<[f32; 2]> = s
            .samples
            .iter()
            .map(|v| {
                let (x, y) = v.pos();
                [x, y]
            })
            .collect();
        let fitted = klad_core::pathedit::fit_polyline(&line, (s.size * 0.12).clamp(1.0, 6.0));
        value["path"] = json!(klad_core::pathedit::to_d(&[fitted]));
    }
    value
}

pub fn mark_json(cel: &str, mark: &Mark) -> Value {
    match mark {
        Mark::Stroke(s) => {
            let mut value = stroke_json(cel, s, true);
            value["kind"] = json!("stroke");
            value
        }
        Mark::Fill(f) => {
            let mut value = fill_json(cel, f);
            value["kind"] = json!("fill");
            value
        }
    }
}
