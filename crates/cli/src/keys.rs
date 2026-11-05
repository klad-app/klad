use anyhow::Result;
use klad_core::bundle::Bundle;
use klad_core::doc::{Document, Node};
use klad_core::keys::Keys;
use serde_json::json;

fn walk<'a>(nodes: &'a [Node], out: &mut Vec<(String, String, &'a Keys)>) {
    nodes.iter().for_each(|n| {
        if !n.keys.is_empty() {
            out.push((
                n.id.clone(),
                if n.name.is_empty() {
                    n.type_name().to_string()
                } else {
                    n.name.clone()
                },
                &n.keys,
            ));
        }
        walk(n.shape.children(), out);
    });
}

pub fn keyed(doc: &Document) -> Vec<(String, String, &Keys)> {
    let mut out = Vec::new();
    doc.all_layers().into_iter().for_each(|l| {
        if !l.keys.is_empty() {
            out.push((l.id.clone(), l.name.clone(), &l.keys));
        }
        l.cels().iter().for_each(|c| walk(&c.nodes, &mut out));
    });
    out
}

pub fn list(bundle: &Bundle, only: Option<&str>, as_json: bool) -> Result<()> {
    let found: Vec<_> = keyed(&bundle.doc)
        .into_iter()
        .filter(|(id, name, _)| only.is_none_or(|o| o == id || o == name))
        .collect();
    if as_json {
        let rows: Vec<_> = found
            .iter()
            .map(|(id, name, keys)| json!({"id": id, "name": name, "keys": keys}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if found.is_empty() {
        println!("no keys yet: klad key <bundle> <layer or shape> --frame 0 y=0");
    }
    found.iter().for_each(|(id, name, keys)| {
        println!("{name} ({id})");
        keys.iter().for_each(|(prop, list)| {
            let shown: Vec<String> = list
                .iter()
                .map(|k| match k.ease {
                    klad_core::keys::Ease::Linear => format!("{}:{}", k.frame, k.value),
                    e => format!(
                        "{}:{} {}",
                        k.frame,
                        k.value,
                        serde_json::to_string(&e).unwrap_or_default().trim_matches('"')
                    ),
                })
                .collect();
            println!("  {prop:<8} {}", shown.join("  "));
        });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Look, Output, render_look};
    use klad_core::doc::Layer;
    use klad_core::ops::parse_lines;
    use klad_render::View;

    fn centre(bundle: &Bundle, frame: u32, dark: impl Fn(&[u8]) -> bool) -> (f32, f32) {
        let out = std::env::temp_dir().join(format!("klad-keys-{}-{frame}.png", std::process::id()));
        let f = render_look(
            bundle,
            frame,
            View::scaled(1.0),
            &Look::plain(None),
            Some(out.clone()),
            false,
        )
        .unwrap()
        .frame;
        let _ = std::fs::remove_file(&out);
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        f.rgba
            .chunks(4)
            .enumerate()
            .filter(|(_, p)| dark(p))
            .for_each(|(i, _)| {
                sx += (i % f.width as usize) as f32;
                sy += (i / f.width as usize) as f32;
                n += 1.0;
            });
        (sx / n, sy / n)
    }

    #[test]
    fn a_keyed_layer_and_shape_move_between_their_keys_with_the_ease() {
        let dir = std::env::temp_dir().join(format!("klad-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(300, 200);
        doc.canvas.background = Some("#ffffff".into());
        doc.timeline.frames = 12;
        doc.layers = vec![
            Layer::paint("l1", "float", "c1"),
            Layer::vector("l2", "shapes", "c2", Vec::new()),
        ];
        Bundle::create(&dir, doc).unwrap();
        let ops = r##"{"op":"stroke","layer":"l1","brush":"ink","size":20,"color":"#000000","path":"M140 100 L160 100"}
{"op":"add","node":{"id":"dot","type":"ellipse","cx":50,"cy":50,"rx":10,"ry":10,"fill":"#ff0000"},"in":"l2"}
{"op":"key","id":"l1","frame":0,"y":0}
{"op":"key","id":"l1","frame":10,"y":-60,"ease":"in-out"}
{"op":"key","id":"dot","frame":0,"x":0,"ease":"step"}
{"op":"key","id":"dot","frame":10,"x":200}"##;
        let quiet = Output {
            json: false,
            render: false,
            scale: None,
            frame: 0,
            overlay: None,
        };
        crate::agent::edit(&dir, &parse_lines(ops).unwrap(), &quiet).unwrap();
        let bundle = Bundle::open(&dir).unwrap();
        let black = |p: &[u8]| p[0] < 60 && p[1] < 60;
        let red = |p: &[u8]| p[0] > 200 && p[1] < 80;
        let (at0, at5, at10) = (
            centre(&bundle, 0, black),
            centre(&bundle, 5, black),
            centre(&bundle, 10, black),
        );
        assert!(
            (at0.1 - 100.0).abs() < 1.5 && (at5.1 - 70.0).abs() < 1.5 && (at10.1 - 40.0).abs() < 1.5,
            "{at0:?} {at5:?} {at10:?}"
        );
        assert!(
            (centre(&bundle, 9, red).0 - 50.0).abs() < 1.5,
            "a step key holds until the next"
        );
        assert!((centre(&bundle, 11, red).0 - 250.0).abs() < 1.5);
        assert_eq!(keyed(&bundle.doc).len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
