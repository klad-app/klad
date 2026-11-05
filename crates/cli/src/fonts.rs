use anyhow::{Result, bail};
use klad_core::bundle::Bundle;
use klad_core::doc::{Item, Shape};
use klad_core::ops::Op;
use klad_render::assets::FONTS;
use klad_render::fonts::{self, Family, bring, is_font};
use serde_json::{Value, json};
use std::path::Path;

fn styles(f: &Family) -> String {
    let faces: Vec<String> = f
        .files
        .iter()
        .map(|x| {
            let axis = std::fs::read(f.path(x))
                .ok()
                .and_then(|b| klad_render::text::weight_axis(&b));
            match axis {
                Some(a) => format!("{} (weight {}-{})", x.style, a.min, a.max),
                None => x.style.clone(),
            }
        })
        .collect();
    faces.join(", ")
}

fn bundle_fonts(bundle: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(bundle.join(FONTS))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| is_font(n))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

pub fn list(
    bundle: Option<&Path>,
    group: Option<&str>,
    search: Option<&str>,
    get: Option<&str>,
    json: bool,
) -> Result<()> {
    if let Some(g) = group.filter(|g| !fonts::GROUPS.contains(g)) {
        bail!("no group {g}: {}", fonts::GROUPS.join(", "));
    }
    if let Some(name) = get {
        let family = fonts::download(name)?;
        println!(
            "{}: {} in {}; text nodes take font=\"{}\"",
            family.family,
            styles(&family),
            family.dir.display(),
            family.family
        );
        return Ok(());
    }
    let installed = fonts::installed();
    let have = |name: &str| installed.iter().any(|f| fonts::key(&f.family) == fonts::key(name));
    if let Some(words) = search {
        let found: Vec<fonts::Listed> = fonts::search(words)?
            .into_iter()
            .filter(|f| group.is_none_or(|g| f.group == g))
            .collect();
        match json {
            true => {
                let rows: Vec<Value> = found
                    .iter()
                    .map(|f| json!({"family": f.family, "group": f.group, "here": have(&f.family)}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            }
            false if found.is_empty() => println!("no open font family matches {words}"),
            false => {
                found.iter().take(40).for_each(|f| {
                    let mark = if have(&f.family) { "here" } else { "download" };
                    println!("{:<32} {:<12} {mark}", f.family, f.group);
                });
                if found.len() > 40 {
                    println!("... {} more; add words to narrow it", found.len() - 40);
                }
                println!("klad fonts --get \"<family>\" downloads one; klad font <bundle> \"<family>\" also does");
            }
        }
        return Ok(());
    }
    let shown: Vec<&Family> = installed
        .iter()
        .filter(|f| group.is_none_or(|g| f.group == g))
        .collect();
    let own = bundle.map(bundle_fonts).unwrap_or_default();
    if json {
        let rows: Vec<Value> = shown
            .iter()
            .map(|f| {
                json!({
                    "family": f.family,
                    "group": f.group,
                    "licence": f.licence,
                    "shipped": f.shipped,
                    "files": f.files.iter().map(|x| json!({"file": x.file, "style": x.style})).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"fonts": rows, "bundle": own}))?
        );
        return Ok(());
    }
    if shown.is_empty() && group.is_none() {
        println!("klad's fonts folder was not found next to the binary; KLAD_FONTS can point at it");
    }
    fonts::GROUPS
        .iter()
        .filter(|g| group.is_none_or(|w| w == **g))
        .for_each(|g| {
            let in_group: Vec<&&Family> = shown.iter().filter(|f| f.group == *g).collect();
            if !in_group.is_empty() {
                println!("{g}");
                in_group.iter().for_each(|f| {
                    let from = if f.shipped { "" } else { "  downloaded" };
                    println!("  {:<24} {:<10} {}{from}", f.family, f.licence, styles(f));
                });
            }
        });
    if let Some(bundle) = bundle {
        match own.is_empty() {
            true => println!("{} has no fonts yet", bundle.display()),
            false => println!("in {}: {}", bundle.display(), own.join(", ")),
        }
    }
    println!(
        "text nodes take font=\"<family>\" or font=\"<family> <style>\"; klad fonts --search finds more to download"
    );
    Ok(())
}

pub fn font(bundle: &Path, name: Option<&str>) -> Result<()> {
    let Some(name) = name else {
        let names = bundle_fonts(bundle);
        match names.is_empty() {
            true => {
                println!("no fonts yet: klad font <bundle> \"Inter\" copies one of klad's, or give a .ttf or .otf file")
            }
            false => names.iter().for_each(|n| println!("{n}")),
        }
        return Ok(());
    };
    let file = Path::new(name);
    let (made, weight) = match file.is_file() {
        true => (klad_render::assets::import_font(bundle, file)?, None),
        false => {
            let chosen = bring(bundle, name)?;
            if let Some(family) = &chosen.fetched {
                eprintln!("downloaded {family} from Google Fonts");
            }
            (chosen.file, chosen.weight)
        }
    };
    match weight {
        Some(w) => println!("{made}: text nodes use it as font={made} weight={w}"),
        None => println!("{made}: text nodes use it as font={made}"),
    }
    Ok(())
}

fn settle_node(root: &Path, node: &mut Value, said: &mut Vec<String>) -> Result<()> {
    if node.get("type").and_then(Value::as_str) == Some("text")
        && let Some(name) = node.get("font").and_then(Value::as_str).map(str::to_string)
    {
        let chosen = bring(root, &name)?;
        if let Some(family) = chosen.fetched {
            said.push(family);
        }
        node["font"] = json!(chosen.file);
        if let Some(w) = chosen.weight.filter(|_| node.get("weight").is_none()) {
            node["weight"] = json!(w);
        }
    }
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        children.iter_mut().try_for_each(|c| settle_node(root, c, said))?;
    }
    Ok(())
}

pub fn settle(bundle: &Bundle, ops: &mut [Op]) -> Result<()> {
    let mut said = Vec::new();
    ops.iter_mut().try_for_each(|op| match op {
        Op::AddNode { node, .. } => settle_node(&bundle.root, node, &mut said),
        Op::Set { id, props } => {
            let text = matches!(bundle.doc.item(id), Some(Item::Node(_, _, n)) if matches!(n.shape, Shape::Text(_)));
            match (text, props.get("font").and_then(Value::as_str).map(str::to_string)) {
                (true, Some(name)) => {
                    let chosen = bring(&bundle.root, &name)?;
                    said.extend(chosen.fetched);
                    props.insert("font".into(), json!(chosen.file));
                    if let Some(w) = chosen.weight.filter(|_| !props.contains_key("weight")) {
                        props.insert("weight".into(), json!(w));
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    })?;
    said.iter().for_each(|f| eprintln!("downloaded {f} from Google Fonts"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klad_core::doc::{Document, Layer};
    use klad_core::ops::parse_lines;

    #[test]
    fn text_takes_a_family_name_and_copies_the_shipped_file_in() {
        let dir = std::env::temp_dir().join(format!("klad-cli-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(400, 200);
        doc.layers = vec![Layer::vector("l1", "sign", "c1", Vec::new())];
        let mut bundle = Bundle::create(&dir, doc).unwrap();
        let mut ops = parse_lines(
            r##"{"op":"add","node":{"id":"t1","type":"text","text":"INN","font":"Inter Bold","fill":"#202020"},"in":"l1"}"##,
        )
        .unwrap();
        settle(&bundle, &mut ops).unwrap();
        bundle.apply(&ops).unwrap();
        let text = |b: &Bundle| match b.doc.item("t1") {
            Some(Item::Node(_, _, n)) => match &n.shape {
                Shape::Text(t) => t.clone(),
                _ => panic!("not text"),
            },
            _ => panic!("no t1"),
        };
        let made = text(&bundle);
        assert_eq!((made.font.as_str(), made.weight), ("Inter.ttf", Some(700.0)));
        let shipped = fonts::find("Inter").unwrap();
        assert_eq!(
            std::fs::read(dir.join("fonts/Inter.ttf")).unwrap(),
            std::fs::read(shipped.family.path(&shipped.face)).unwrap(),
            "the drawing holds the same file the editor copies in"
        );
        let mut ops = parse_lines(r#"{"op":"set","id":"t1","font":"Pirata One"}"#).unwrap();
        settle(&bundle, &mut ops).unwrap();
        bundle.apply(&ops).unwrap();
        assert_eq!(text(&bundle).font, "PirataOne-Regular.ttf");
        let mut wrong = parse_lines(r#"{"op":"set","id":"t1","font":"No Such Typeface Anywhere"}"#).unwrap();
        let err = settle(&bundle, &mut wrong).unwrap_err().to_string();
        assert!(err.contains("klad fonts"), "{err}");
        assert_eq!(
            bundle_fonts(&dir),
            ["Inter.ttf", "PirataOne-Regular.ttf"],
            "licence files are not listed as fonts"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
