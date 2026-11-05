use super::*;
use klad_core::doc::{Node, Shape};
use klad_core::look::{Curves, Displace, Hsl, Rays};

fn disc(id: &str, cx: f32, cy: f32, r: f32, color: &str) -> Layer {
    Layer::vector(
        id,
        id,
        &format!("c{id}"),
        vec![Node::new(&format!("n{id}"), Shape::Ellipse { cx, cy, rx: r, ry: r }).filled(color)],
    )
}

fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, color: &str) -> Layer {
    Layer::vector(
        id,
        id,
        &format!("c{id}"),
        vec![
            Node::new(
                &format!("n{id}"),
                Shape::Rect {
                    x,
                    y,
                    w,
                    h,
                    radius: 0.0,
                },
            )
            .filled(color),
        ],
    )
}

fn rasters(doc: &Document) -> Rasters {
    fn walk(layers: &[Layer], doc: &Document, out: &mut Rasters) {
        layers.iter().for_each(|l| {
            if let Some(cel) = l.cels().first().filter(|_| l.kind() == Kind::Vector) {
                let raster = rasterize_nodes(&cel.nodes, doc.canvas.width, doc.canvas.height).unwrap();
                out.insert(cel.id.clone(), raster);
            }
            walk(l.children(), doc, out);
        });
    }
    let mut out = Rasters::new();
    walk(&doc.layers, doc, &mut out);
    out
}

fn full(doc: &Document) -> Frame {
    compose(doc, 0, &rasters(doc), View::scaled(1.0)).unwrap()
}

fn tiled(doc: &Document) -> Frame {
    let layers: Vec<&Layer> = doc.layers.iter().collect();
    let raster = flatten(doc, 0, &rasters(doc), &layers, doc.canvas.background.as_deref()).unwrap();
    read_rgba8(&raster_image(&raster).unwrap()).unwrap()
}

fn worst(a: &Frame, b: &Frame) -> u8 {
    a.rgba
        .iter()
        .zip(&b.rgba)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

fn effects(json: &str) -> Vec<Effect> {
    serde_json::from_str(json).unwrap()
}

#[test]
fn effects_compose_the_same_in_tiles_as_in_one_piece() {
    let mut doc = Document::new(600, 420);
    doc.canvas.background = Some("#203040".into());
    let mut lantern = disc("l1", 300.0, 240.0, 70.0, "#e0a040");
    lantern.effects = effects(
        r##"[{"shadow":{"x":8,"y":10,"radius":5}},{"outline":{"width":5,"color":"#101010"}},{"noise":{"amount":0.15,"size":2}},{"displace":{"amount":5,"size":30}},{"glow":{"radius":16,"color":"#ffd080"}},{"blur":{"radius":1}}]"##,
    );
    let mut halo = disc("l2", 250.0, 255.0, 30.0, "#80c0ff");
    halo.effects = effects(r#"[{"glow":{"radius":40,"strength":1.5,"blend":"add"}}]"#);
    halo.opacity = 0.8;
    doc.layers = vec![lantern, halo];
    let (a, b) = (full(&doc), tiled(&doc));
    assert_eq!(worst(&a, &b), 0, "tiles differ from the whole");
    let plain = {
        let mut d = doc.clone();
        d.layers.iter_mut().for_each(|l| l.effects.clear());
        full(&d)
    };
    let (x, y) = (300 + 70 + 3, 240);
    assert_ne!(
        a.pixel(x, y),
        plain.pixel(x, y),
        "the outline and glow show past the edge"
    );
    assert!(
        a.pixel(300 + 70 + 20, 240)[0] > plain.pixel(300 + 70 + 20, 240)[0] + 10,
        "the glow spreads"
    );
}

#[test]
fn blend_modes_meet_their_formulas() {
    let back = parse_hex("#3366cc").unwrap();
    let top = parse_hex("#e08040").unwrap();
    Blend::ALL.iter().for_each(|&mode| {
        let mut doc = Document::new(32, 32);
        let mut over = rect("l2", 0.0, 0.0, 32.0, 32.0, "#e08040");
        over.blend = mode;
        over.opacity = 0.6;
        doc.layers = vec![rect("l1", 0.0, 0.0, 32.0, 32.0, "#3366cc"), over];
        let got = full(&doc).pixel(16, 16);
        let source = [top[0] * 0.6, top[1] * 0.6, top[2] * 0.6, 0.6];
        let want = look::composite(mode, back, source);
        let want: Vec<u8> = (0..3).map(|c| (want[c] / want[3] * 255.0).round() as u8).collect();
        assert!(
            got.iter().zip(&want).all(|(g, w)| g.abs_diff(*w) <= 2),
            "{}: {got:?} vs {want:?}",
            mode.name()
        );
    });
}

#[test]
fn adjustments_change_what_is_under_them_in_their_group_or_clip() {
    let mut doc = Document::new(512, 300);
    let curve = Adjustment::Curves(Curves {
        points: vec![[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]],
        ..Curves::default()
    });
    let group = Layer::group(
        "g",
        "g",
        vec![
            rect("l2", 256.0, 0.0, 256.0, 150.0, "#808080"),
            Layer::adjust("a1", "curves", curve.clone()),
        ],
    );
    let mut turn = Layer::adjust(
        "a2",
        "hue",
        Adjustment::Hsl(Hsl {
            hue: 120.0,
            ..Hsl::default()
        }),
    );
    turn.clip = true;
    doc.layers = vec![
        rect("l1", 0.0, 0.0, 256.0, 150.0, "#808080"),
        group,
        rect("l3", 100.0, 180.0, 200.0, 80.0, "#ff0000"),
        turn,
    ];
    let frame = full(&doc);
    assert_eq!(frame.pixel(100, 75), [128, 128, 128, 255], "outside the group");
    let expected = (curve.prepare().unwrap().apply([128.0 / 255.0; 3])[0] * 255.0).round() as u8;
    assert!(
        frame.pixel(400, 75)[0].abs_diff(expected) <= 1,
        "{:?} vs {expected}",
        frame.pixel(400, 75)
    );
    let p = frame.pixel(200, 220);
    assert!(p[1] > 240 && p[0] < 10, "the clipped hue turn {p:?}");
    assert_eq!(
        frame.pixel(50, 220)[3],
        0,
        "the clip keeps the adjustment off empty pixels"
    );
    assert!(worst(&frame, &tiled(&doc)) <= 1);
}

#[test]
fn displacement_follows_another_layer() {
    let mut doc = Document::new(400, 200);
    let mut map = rect("l9", 0.0, 0.0, 400.0, 200.0, "#ff8080");
    map.visible = false;
    let mut body = rect("l1", 0.0, 0.0, 200.0, 200.0, "#204060");
    body.effects = vec![Effect::Displace(Displace {
        amount: 10.0,
        by: Some("l9".into()),
        ..Displace::default()
    })];
    doc.layers = vec![map, body];
    let frame = full(&doc);
    assert!(frame.pixel(185, 100)[3] > 250 && frame.pixel(195, 100)[3] < 5);
    assert!(worst(&frame, &tiled(&doc)) <= 1);
}

#[test]
fn rays_shine_where_they_point_and_repeat() {
    let mut doc = Document::new(400, 400);
    doc.canvas.background = Some("#000000".into());
    let mut node = Node::new(
        "n1",
        Shape::Rays(Rays {
            x: 200.0,
            y: 0.0,
            spread: 30.0,
            count: 6,
            length: 380.0,
            ..Rays::default()
        }),
    )
    .filled("#fff2c0");
    node.blur = 4.0;
    doc.layers = vec![Layer::vector("l1", "rays", "c1", vec![node])];
    let (a, b) = (full(&doc), full(&doc));
    assert_eq!(a.hash(), b.hash());
    let lit = (100..300).map(|y| a.pixel(200, y)[0] as u32).sum::<u32>();
    let dark = (100..300).map(|y| a.pixel(20, y)[0] as u32).sum::<u32>();
    assert!(lit > 2000 && dark == 0, "lit {lit} dark {dark}");
    assert!(worst(&a, &tiled(&doc)) <= 1);
}

#[test]
#[ignore = "timing: cargo test --release -p klad-render glow_timing -- --ignored --nocapture"]
fn glow_timing() {
    use std::time::Instant;
    let mut doc = Document::new(2048, 2048);
    doc.canvas.background = Some("#101820".into());
    let mut lamps = Layer::vector(
        "l1",
        "lamps",
        "c1",
        (0..40)
            .map(|i| {
                let (x, y) = ((i % 8) as f32 * 250.0 + 120.0, (i / 8) as f32 * 400.0 + 200.0);
                Node::new(
                    &format!("n{i}"),
                    Shape::Ellipse {
                        cx: x,
                        cy: y,
                        rx: 40.0,
                        ry: 40.0,
                    },
                )
                .filled("#ffc060")
            })
            .collect(),
    );
    lamps.effects = vec![Effect::Glow(klad_core::look::Glow {
        radius: 32.0 / 3.0,
        ..klad_core::look::Glow::default()
    })];
    doc.layers = vec![rect("l0", 0.0, 0.0, 2048.0, 2048.0, "#203040"), lamps];
    let rasters = rasters(&doc);
    let time = |f: &dyn Fn()| {
        let runs: Vec<f64> = (0..5)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        runs.iter().cloned().fold(f64::MAX, f64::min)
    };
    let whole = time(&|| {
        compose(&doc, 0, &rasters, View::scaled(1.0)).unwrap();
    });
    let layers: Vec<&Layer> = doc.layers.iter().collect();
    let tiled = time(&|| {
        flatten(&doc, 0, &rasters, &layers, doc.canvas.background.as_deref()).unwrap();
    });
    let items: Vec<TileItem> = doc.layers.iter().map(TileItem::Layer).collect();
    let one = time(&|| {
        compose_tile(
            &items,
            &TileJob {
                frame: 0,
                rasters: &rasters,
                background: doc.canvas.background.as_deref(),
                tx: 3,
                ty: 3,
                swap: None,
                onions: None,
                doc: &doc,
                references: false,
            },
        )
        .unwrap();
    });
    println!(
        "2048 px with a 32 px glow on 40 lamps: whole canvas {whole:.0} ms, all 64 tiles {tiled:.0} ms, one dirty tile {one:.2} ms"
    );
}

fn plane(id: &str, depth: Option<f32>, child: Layer) -> Layer {
    let mut g = Layer::group(id, id, vec![child]);
    g.depth = depth;
    g
}

#[test]
fn far_planes_take_the_fog_and_blur_and_near_ones_stay() {
    let mut doc = Document::new(300, 200);
    doc.canvas.background = Some("#000000".into());
    doc.atmosphere = Some(klad_core::look::Atmosphere {
        color: "#c0d0e0".into(),
        density: 1.0,
        blur: 2.0,
        desaturate: 0.5,
        ..klad_core::look::Atmosphere::default()
    });
    doc.layers = vec![
        plane("far", Some(2.0), rect("l1", 20.0, 20.0, 100.0, 160.0, "#ff0000")),
        plane("near", Some(0.0), rect("l2", 180.0, 20.0, 100.0, 160.0, "#ff0000")),
    ];
    let frame = full(&doc);
    assert_eq!(
        frame.pixel(230, 100),
        [255, 0, 0, 255],
        "a plane at depth 0 stays as it is"
    );
    let far = frame.pixel(70, 100);
    assert!(
        far[1] > 120 && far[2] > 120 && far[0] > 150,
        "the far plane is mostly fog: {far:?}"
    );
    assert!(
        frame.pixel(20, 100)[1] > frame.pixel(70, 100)[1] / 3,
        "and its edge is soft"
    );
    assert_eq!(worst(&frame, &tiled(&doc)), 0);
}

#[test]
fn references_stay_out_of_renders_unless_asked() {
    let mut doc = Document::new(64, 64);
    let mut photo = rect("l1", 0.0, 0.0, 64.0, 64.0, "#336699");
    photo.reference = true;
    doc.layers = vec![photo];
    let hidden = compose(&doc, 0, &rasters(&doc), View::scaled(1.0)).unwrap();
    assert_eq!(hidden.pixel(32, 32)[3], 0);
    let shown = compose(
        &doc,
        0,
        &rasters(&doc),
        View {
            references: true,
            ..View::scaled(1.0)
        },
    )
    .unwrap();
    assert_eq!(shown.pixel(32, 32), [0x33, 0x66, 0x99, 255]);
}

#[test]
fn generators_draw_their_copies_the_same_in_tiles() {
    use klad_core::generators::{Mirror, Repeat, Scatter};
    let dot = |id: &str| {
        Node::new(
            id,
            Shape::Ellipse {
                cx: 0.0,
                cy: 0.0,
                rx: 8.0,
                ry: 8.0,
            },
        )
        .filled("#204060")
    };
    let mut doc = Document::new(600, 300);
    let nodes = vec![
        Node::new(
            "r",
            Shape::Repeat(Repeat {
                count: 3,
                step: [60.0, 0.0],
                children: vec![
                    Node::new(
                        "r1",
                        Shape::Rect {
                            x: 20.0,
                            y: 20.0,
                            w: 30.0,
                            h: 30.0,
                            radius: 0.0,
                        },
                    )
                    .filled("#a03020"),
                ],
                ..Repeat::default()
            }),
        ),
        Node::new(
            "m",
            Shape::Mirror(Mirror {
                x: 300.0,
                children: vec![
                    Node::new(
                        "m1",
                        Shape::Path {
                            d: "M250 100 L280 100 L280 140 Z".into(),
                        },
                    )
                    .filled("#208040"),
                ],
                ..Mirror::default()
            }),
        ),
        Node::new(
            "s",
            Shape::Scatter(Scatter {
                x: 20.0,
                y: 180.0,
                w: 560.0,
                h: 100.0,
                count: 30,
                children: vec![dot("s1")],
                ..Scatter::default()
            }),
        ),
    ];
    doc.layers = vec![Layer::vector("l1", "gen", "c1", nodes)];
    let frame = full(&doc);
    [35u32, 95, 155]
        .iter()
        .for_each(|x| assert_eq!(frame.pixel(*x, 35)[0], 0xa0, "copy at {x}"));
    assert_eq!(frame.pixel(215, 35)[3], 0, "only three copies");
    assert!(
        frame.pixel(275, 130)[1] > 100 && frame.pixel(325, 130)[1] > 100,
        "the mirror shows both sides"
    );
    let dots = (180..280)
        .flat_map(|y| (0..600).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.pixel(x, y)[3] > 0)
        .count();
    assert!(dots > 2000, "the scatter lays its copies ({dots} px)");
    assert_eq!(worst(&frame, &tiled(&doc)), 0);
}

#[test]
fn text_draws_the_font_s_outlines_and_names_a_missing_font() {
    use klad_core::generators::Text;
    let root = std::env::temp_dir().join(format!("draw-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/assets/Phosphor.ttf");
    let name = crate::assets::import_font(&root, &font).unwrap();
    crate::assets::add_root(&root);
    let text = |font: &str| {
        Node::new(
            "t",
            Shape::Text(Text {
                text: "\u{E472}\u{E434}".into(),
                font: font.into(),
                x: 20.0,
                y: 20.0,
                size: 64.0,
                ..Text::default()
            }),
        )
        .filled("#101010")
    };
    let mut doc = Document::new(200, 120);
    doc.layers = vec![Layer::vector("l1", "sign", "c1", vec![text(&name)])];
    let frame = full(&doc);
    let inked = (0..120)
        .flat_map(|y| (0..200).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.pixel(x, y)[3] > 128)
        .count();
    assert!(inked > 300, "the glyphs are drawn ({inked} px)");
    assert_eq!(frame.hash(), full(&doc).hash());
    doc.layers = vec![Layer::vector("l1", "sign", "c1", vec![text("Missing.ttf")])];
    let err = compose(&doc, 0, &Rasters::new(), View::scaled(1.0)).err().unwrap();
    assert!(format!("{err:#}").contains("Missing.ttf"), "{err:#}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn text_is_shaped_kerned_and_weighted_with_a_caret_at_every_letter() {
    use crate::text::{layout, text_path, weight_axis};
    use klad_core::generators::Text;
    let root = std::env::temp_dir().join(format!("draw-shaped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let file = crate::fonts::bring(&root, "Inter").unwrap().file;
    assert_eq!(file, "Inter.ttf");
    assert!(
        root.join("fonts/Inter.licence.txt").is_file(),
        "the licence goes with the font"
    );
    crate::assets::add_root(&root);
    let words = |text: &str, weight: Option<f32>| Text {
        text: text.into(),
        font: file.clone(),
        size: 100.0,
        weight,
        ..Text::default()
    };
    let width = |t: &Text| layout(t).unwrap().lines[0].width;
    let pair = width(&words("AV", None));
    let apart = width(&words("A", None)) + width(&words("V", None));
    assert!(
        pair < apart - 3.0,
        "A and V kern from the GPOS table ({pair} against {apart})"
    );
    let bold = text_path(&words("Hello", Some(800.0))).unwrap().bounds().width();
    let thin = text_path(&words("Hello", Some(200.0))).unwrap().bounds().width();
    assert!(
        bold > thin + 10.0,
        "the weight axis moves the outlines ({bold} against {thin})"
    );
    let axis = weight_axis(&std::fs::read(root.join("fonts/Inter.ttf")).unwrap()).unwrap();
    assert_eq!((axis.min, axis.max), (100.0, 900.0));
    let two = layout(&words("ab\ncd", None)).unwrap();
    assert_eq!(two.lines.len(), 2);
    assert_eq!(two.lines[1].start, 3);
    let carets: Vec<usize> = two.lines.iter().flat_map(|l| l.carets.iter().map(|c| c.0)).collect();
    assert_eq!(carets, [0, 1, 2, 3, 4, 5]);
    let (x, top, bottom) = two.caret(4).unwrap();
    assert!(x > 0.0 && top > two.lines[0].baseline && bottom > top);
    assert_eq!(two.nearest(x + 1.0, top + 10.0), 4);
    assert!(
        two.lines[0].carets.windows(2).all(|w| w[1].1 > w[0].1),
        "carets run left to right"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_imported_picture_draws_where_it_is_placed() {
    use klad_core::generators::Picture;
    let root = std::env::temp_dir().join(format!("draw-picture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let rgba = crate::assets::Rgba {
        width: 2,
        height: 2,
        pixels: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255],
    };
    let (src, w, h) = crate::assets::import_image(&root, &crate::assets::encode_png(&rgba).unwrap()).unwrap();
    crate::assets::add_root(&root);
    let mut doc = Document::new(100, 100);
    doc.layers = vec![Layer::vector(
        "l1",
        "photo",
        "c1",
        vec![Node::new(
            "p",
            Shape::Image(Picture {
                src,
                x: 10.0,
                y: 10.0,
                w: w as f32 * 40.0,
                h: h as f32 * 40.0,
            }),
        )],
    )];
    let frame = full(&doc);
    assert_eq!(frame.pixel(15, 15), [255, 0, 0, 255]);
    assert_eq!(frame.pixel(85, 85), [255, 255, 255, 255]);
    assert_eq!(frame.pixel(5, 5)[3], 0);
    let _ = std::fs::remove_dir_all(&root);
}

fn painted(brush: &str, light: [f32; 2]) -> (Document, Rasters) {
    use klad_core::stroke::Stroke;
    let mut doc = Document::new(400, 200);
    doc.canvas.background = Some("#f0e8d8".into());
    let mut layer = Layer::paint("l1", "paint", "c1");
    layer.effects = vec![Effect::Relief(look::Relief {
        light,
        ..look::Relief::default()
    })];
    doc.layers = vec![layer];
    let points: Vec<(f32, f32, f32)> = (0..=40).map(|i| (40.0 + i as f32 * 8.0, 100.0, 1.0)).collect();
    let stroke = Stroke::new(1, brush, [0.75, 0.45, 0.2, 1.0], 56.0, 3).with_points(&points, 5.0);
    let raster = klad_brush::replay(400, 200, &[stroke.into()], &klad_brush::BrushSet::presets())
        .unwrap()
        .raster;
    let mut rasters = Rasters::new();
    rasters.insert("c1".into(), raster);
    (doc, rasters)
}

fn light_of(frame: &Frame, ys: std::ops::Range<u32>) -> u32 {
    ys.flat_map(|y| (120..280).map(move |x| (x, y)))
        .map(|(x, y)| frame.pixel(x, y)[..3].iter().map(|&c| c as u32).sum::<u32>())
        .sum()
}

#[test]
fn relief_lights_paint_edges_facing_the_light_and_leaves_flat_paint_alone() {
    let compose_with = |doc: &Document, rasters: &Rasters| compose(doc, 0, rasters, View::scaled(1.0)).unwrap();
    let (doc, rasters) = painted("oil-load", [0.0, -1.0]);
    let lit = compose_with(&doc, &rasters);
    let mut bare = doc.clone();
    bare.layers[0].effects.clear();
    let plain = compose_with(&bare, &rasters);
    assert_ne!(lit.hash(), plain.hash());
    let (top, bottom) = (78..86, 114..122);
    assert!(
        light_of(&lit, top.clone()) > light_of(&plain, top.clone()),
        "the top edge faces the light"
    );
    assert!(
        light_of(&lit, bottom.clone()) < light_of(&plain, bottom.clone()),
        "the bottom edge faces away"
    );
    let (from_below, _) = painted("oil-load", [0.0, 1.0]);
    let under = compose_with(&from_below, &rasters);
    assert!(
        light_of(&under, bottom.clone()) > light_of(&lit, bottom),
        "turning the light turns the relief"
    );
    let flat_rasters = painted("ink", [0.0, -1.0]).1;
    assert!(!flat_rasters["c1"].has_height());
    assert_eq!(
        compose_with(&doc, &flat_rasters).hash(),
        compose_with(&bare, &flat_rasters).hash()
    );
    let layers: Vec<&Layer> = doc.layers.iter().collect();
    let tiles = flatten(&doc, 0, &rasters, &layers, doc.canvas.background.as_deref()).unwrap();
    assert!(worst(&lit, &read_rgba8(&raster_image(&tiles).unwrap()).unwrap()) <= 1);
}

#[test]
#[ignore = "timing: cargo test --release -p klad-render relief_timing -- --ignored --nocapture"]
fn relief_timing() {
    use klad_core::stroke::Stroke;
    use std::time::Instant;
    let mut doc = Document::new(2048, 2048);
    doc.canvas.background = Some("#f0e8d8".into());
    let mut layer = Layer::paint("l1", "paint", "c1");
    layer.effects = vec![Effect::Relief(look::Relief::default())];
    doc.layers = vec![layer];
    let mut rng = klad_core::rng::Rng::new(7);
    let strokes: Vec<klad_core::stroke::Mark> = (0..600)
        .map(|i| {
            let (x, y) = (rng.unit() * 1900.0 + 74.0, rng.unit() * 1900.0 + 74.0);
            let turn = rng.signed() * 3.1;
            let points: Vec<(f32, f32, f32)> = (0..=12)
                .map(|k| {
                    let t = k as f32 / 12.0 * 160.0;
                    (x + t * turn.cos(), y + t * turn.sin(), 0.6 + 0.4 * (k as f32 / 12.0))
                })
                .collect();
            let colour = [0.5 + rng.unit() * 0.4, 0.4 + rng.unit() * 0.3, 0.2, 1.0];
            Stroke::new(i + 1, "oil-load", colour, 24.0 + rng.unit() * 40.0, i)
                .with_points(&points, 5.0)
                .into()
        })
        .collect();
    let started = Instant::now();
    let raster = klad_brush::replay(2048, 2048, &strokes, &klad_brush::BrushSet::presets())
        .unwrap()
        .raster;
    let replayed = started.elapsed().as_secs_f64() * 1000.0;
    let mut thin = klad_brush::BrushSet::presets();
    let mut def = thin.def("oil-load").unwrap().clone();
    def.body = 0.0;
    thin.insert("oil-load", def);
    let started = Instant::now();
    klad_brush::replay(2048, 2048, &strokes, &thin).unwrap();
    let flat_replay = started.elapsed().as_secs_f64() * 1000.0;
    let heights = raster.height_bytes() >> 20;
    let mut rasters = Rasters::new();
    rasters.insert("c1".into(), raster);
    let time = |doc: &Document, f: &dyn Fn(&Document)| {
        (0..5)
            .map(|_| {
                let t = Instant::now();
                f(doc);
                t.elapsed().as_secs_f64() * 1000.0
            })
            .fold(f64::MAX, f64::min)
    };
    let whole = |d: &Document| {
        compose(d, 0, &rasters, View::scaled(1.0)).unwrap();
    };
    let one = |d: &Document| {
        let items: Vec<TileItem> = d.layers.iter().map(TileItem::Layer).collect();
        compose_tile(
            &items,
            &TileJob {
                frame: 0,
                rasters: &rasters,
                background: d.canvas.background.as_deref(),
                tx: 3,
                ty: 3,
                swap: None,
                onions: None,
                doc: d,
                references: false,
            },
        )
        .unwrap();
    };
    let mut flat = doc.clone();
    flat.layers[0].effects.clear();
    println!(
        "2048 px, 600 oil-load strokes (replay {replayed:.0} ms, {flat_replay:.0} ms without body; heights {heights} MB): whole canvas {:.0} ms with relief, {:.0} ms without; one dirty tile {:.2} ms with, {:.2} ms without",
        time(&doc, &whole),
        time(&flat, &whole),
        time(&doc, &one),
        time(&flat, &one)
    );
}
