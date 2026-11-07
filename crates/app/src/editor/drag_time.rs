use super::{Editor, Picked, Tool};
use crate::commands::Cmd;
use crate::settings::Settings;
use egui::{Pos2, vec2};
use egui_kittest::Harness;
use klad_core::bundle::Bundle;
use klad_core::doc::{Blend, Document, Kind, Layer, Loc, Node, Shape};
use klad_core::look::{Effect, Noise};
use klad_core::stroke::{Mark, Stroke};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, fill: &str) -> Node {
    Node::new(
        id,
        Shape::Rect {
            x,
            y,
            w,
            h,
            radius: 0.0,
        },
    )
    .filled(fill)
}

fn layered_poster(dir: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(dir);
    let mut doc = Document::new(2480, 3508);
    doc.canvas.background = Some("#f2ede1".into());
    let stars: Vec<Node> = std::iter::once(rect("sky", 150.0, 900.0, 2180.0, 1450.0, "#173b52"))
        .chain((0..40).map(|i| {
            let (x, y) = (200.0 + (i * 397 % 2100) as f32, 950.0 + (i * 211 % 600) as f32);
            Node::new(
                &format!("star{i}"),
                Shape::Ellipse {
                    cx: x,
                    cy: y,
                    rx: 6.0,
                    ry: 6.0,
                },
            )
            .filled("#f6f1e4")
        }))
        .collect();
    let part = |k: usize| -> Vec<Layer> {
        let id = format!("part{k}");
        let shade = |suffix: &str, blend: Blend| Layer {
            clip: true,
            blend,
            ..Layer::paint(
                &format!("{id}-{suffix}"),
                &format!("{id}-{suffix}"),
                &format!("c{id}-{suffix}"),
            )
        };
        let (x, y) = (300.0 + (k % 5) as f32 * 420.0, 1150.0 + (k / 5) as f32 * 650.0);
        let stripes = Layer {
            clip: true,
            ..Layer::vector(
                &format!("{id}-stripes"),
                &format!("{id}-stripes"),
                &format!("c{id}-stripes"),
                (0..3)
                    .map(|s| {
                        rect(
                            &format!("{id}s{s}"),
                            x - 150.0,
                            y - 120.0 + s as f32 * 90.0,
                            300.0,
                            40.0,
                            "#ffffff",
                        )
                    })
                    .collect(),
            )
        };
        vec![
            Layer::paint(&id, &id, &format!("c{id}")),
            stripes,
            shade("shade", Blend::Multiply),
            shade("rim", Blend::Normal),
            Layer::paint(&format!("{id}-ink"), &format!("{id}-ink"), &format!("c{id}-ink")),
        ]
    };
    let mut scene = Layer::group(
        "scene",
        "scene",
        std::iter::once(Layer::vector("stars", "stars", "cstars", stars))
            .chain((0..10).flat_map(part))
            .collect(),
    );
    scene.effects = vec![Effect::Noise(Noise {
        amount: 0.035,
        size: 2.0,
        seed: 1,
        mono: true,
    })];
    let layout: Vec<Node> = (0..6)
        .map(|i| {
            rect(
                &format!("chip{i}"),
                150.0 + i as f32 * 360.0,
                150.0,
                320.0,
                96.0,
                "#6fd6cf",
            )
        })
        .chain([
            rect("band", -20.0, 2400.0, 2520.0, 130.0, "#6fd6cf"),
            rect("block", 150.0, 2620.0, 2180.0, 636.0, "#173b52"),
            rect("date1", 162.0, 2632.0, 580.0, 300.0, "#0e2b3c"),
            rect("venue1", 754.0, 2632.0, 1564.0, 300.0, "#f2ede1"),
            rect("date2", 162.0, 2944.0, 580.0, 300.0, "#0e2b3c"),
            rect("venue2", 754.0, 2944.0, 1564.0, 300.0, "#f2ede1"),
            rect("footer", -20.0, 3388.0, 2520.0, 140.0, "#0e2b3c"),
        ])
        .collect();
    doc.layers = vec![
        Layer::vector(
            "page",
            "page",
            "cpage",
            vec![rect("paper", 0.0, 0.0, 2480.0, 3508.0, "#f2ede1")],
        ),
        scene,
        Layer::vector("layout", "layout", "clayout", layout),
    ];
    doc.next_id = 1000;
    let bundle = Bundle::create(dir, doc).unwrap();
    let stroke = |id: u32, color: [f32; 4], size: f32, points: &[(f32, f32, f32)]| -> Vec<Mark> {
        vec![Stroke::new(id, "ink", color, size, id).with_points(points, 8.0).into()]
    };
    (0..10).for_each(|k| {
        let id = format!("part{k}");
        let (x, y) = (300.0 + (k % 5) as f32 * 420.0, 1150.0 + (k / 5) as f32 * 650.0);
        let blob: Vec<(f32, f32, f32)> = (0..12)
            .map(|i| (x - 120.0 + i as f32 * 20.0, y + ((i * 37 % 9) as f32 - 4.0) * 12.0, 1.0))
            .collect();
        [
            ("", [0.43, 0.84, 0.81, 1.0], 220.0),
            ("-shade", [0.45, 0.55, 0.62, 1.0], 120.0),
            ("-rim", [1.0, 1.0, 1.0, 1.0], 30.0),
            ("-ink", [0.09, 0.23, 0.32, 1.0], 10.0),
        ]
        .iter()
        .enumerate()
        .for_each(|(j, (suffix, color, size))| {
            let layer = format!("{id}{suffix}");
            bundle
                .write_marks(
                    &layer,
                    &format!("c{layer}"),
                    &stroke(1 + j as u32, *color, *size, &blob),
                )
                .unwrap();
        });
    });
    dir.to_path_buf()
}

struct Run {
    h: Harness<'static, Editor>,
}

impl Run {
    fn step(&mut self) -> f64 {
        let t = Instant::now();
        self.h.step();
        ms(t)
    }

    fn settle(&mut self, done: impl Fn(&Editor) -> bool) -> (f64, f64) {
        let start = Instant::now();
        let mut worst = 0.0f64;
        while !done(self.h.state()) && start.elapsed() < Duration::from_secs(120) {
            worst = worst.max(self.step());
            std::thread::sleep(Duration::from_millis(4));
        }
        (ms(start), worst)
    }

    fn calm(&mut self) -> (f64, f64) {
        self.settle(|ed| ed.canvas_current() && ed.dirty.is_empty() && ed.drag.is_none() && ed.preview.is_none())
    }

    fn screen(&self, (x, y): (f32, f32)) -> Pos2 {
        let ed = self.h.state();
        let rect = ed.canvas_rect;
        rect.min + ed.view.unwrap().to_screen(vec2(x, y), rect.size())
    }

    fn layer_path(&self, id: &str) -> Vec<usize> {
        match self.h.state().scene.bundle.doc.locate(id) {
            Some(Loc::Layer(path)) => path,
            _ => panic!("no layer {id}"),
        }
    }

    fn drag(&mut self, what: &str, from: (f32, f32), by: (f32, f32)) {
        let (a, b) = (self.screen(from), self.screen((from.0 + by.0, from.1 + by.1)));
        self.h.hover_at(a);
        self.step();
        self.h.drag_at(a);
        let press = self.step();
        let moving = Instant::now();
        let mut shown = None;
        let mut updates = 0;
        let mut worst = 0.0f64;
        (1..=16).for_each(|i| {
            self.h.hover_at(a + (b - a) * (i as f32 / 16.0));
            worst = worst.max(self.step());
            let now = self
                .h
                .state()
                .preview
                .as_ref()
                .and_then(|p| p.done.as_ref().map(|d| format!("{:?}", d.0)));
            if now.is_some() && now != shown {
                updates += 1;
                shown = now;
            }
            std::thread::sleep(Duration::from_millis(16));
        });
        let moved = ms(moving);
        let tiles = self.h.state().preview.as_ref().map_or(0, |p| p.shown.len());
        self.h.drop_at(b);
        let release = self.step();
        let (after, after_worst) = self.calm();
        println!(
            "{what}: press {press:.1} ms, {updates} previews in {moved:.0} ms of moving ({tiles} tiles, slowest frame {worst:.1} ms), release {release:.1} ms, settled {after:.0} ms later (slowest {after_worst:.1} ms)"
        );
    }
}

#[test]
#[ignore = "timing: just drag-time [bundle], which drags in a copy of the bundle or in a generated poster"]
fn drag_time() {
    let dir = std::env::temp_dir().join(format!("klad-drag-time-{}", std::process::id()));
    let path = match std::env::var("KLAD_DRAG_BUNDLE").ok().filter(|v| !v.is_empty()) {
        Some(given) => {
            let copy = dir.join("copy.klad");
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let copied = std::process::Command::new("cp")
                .arg("-R")
                .arg(&given)
                .arg(&copy)
                .status()
                .unwrap();
            assert!(copied.success(), "cannot copy {given}");
            copy
        }
        None => layered_poster(&dir.join("poster.klad")),
    };
    let opened = Instant::now();
    let mut editor = Editor::open(egui::Context::default(), &path).unwrap();
    editor.settings = Settings::default();
    editor.settings_path = None;
    let h = Harness::builder()
        .with_size(vec2(1600.0, 1000.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, editor: &mut Editor| editor.frame(ui), editor);
    let mut run = Run { h };
    let (ready, worst) = run.calm();
    println!(
        "{}: open and first canvas in {:.0} ms (slowest frame {worst:.1} ms)",
        path.display(),
        ms(opened).max(ready)
    );
    let doc = run.h.state().scene.bundle.doc.clone();
    let boxed = |n: &Node| klad_render::vector::node_box(n).ok().flatten();
    let top = doc
        .layers
        .iter()
        .rev()
        .find(|l| l.kind() == Kind::Vector && l.visible && !l.locked && !l.cels().is_empty());
    let deep = doc.all_layers().into_iter().find(|l| {
        l.kind() == Kind::Vector
            && l.visible
            && !l.locked
            && !l.cels().is_empty()
            && !doc.layers.iter().any(|t| t.id == l.id)
    });
    let shapes: Vec<(&str, &Layer, Node)> = top
        .into_iter()
        .flat_map(|l| {
            let mut sized: Vec<(Node, f32)> = l.cels()[0]
                .nodes
                .iter()
                .filter(|n| n.visible)
                .filter_map(|n| boxed(n).map(|b| (n.clone(), b[2] * b[3])))
                .collect();
            sized.sort_by(|a, b| a.1.total_cmp(&b.1));
            let small = sized
                .get(sized.len() / 3)
                .map(|(n, _)| ("small shape on the top layer", l, n.clone()));
            let big = sized
                .last()
                .map(|(n, _)| ("biggest shape on the top layer", l, n.clone()));
            small.into_iter().chain(big)
        })
        .chain(deep.and_then(|l| {
            l.cels()[0]
                .nodes
                .iter()
                .find(|n| boxed(n).is_some())
                .map(|n| ("shape in a group", l, n.clone()))
        }))
        .collect();
    shapes.iter().for_each(|(what, layer, node)| {
        let [x, y, w, h] = boxed(node).unwrap();
        let path = run.layer_path(&layer.id);
        run.h.state_mut().activate(path).unwrap();
        run.calm();
        run.h.state_mut().tool = Tool::Select;
        run.h.state_mut().selection = Some(node.id.clone());
        run.calm();
        run.drag(
            &format!("{what} ({} in {}, {w:.0}x{h:.0})", node.id, layer.name),
            (x + w / 2.0, y + h / 2.0),
            (180.0, -120.0),
        );
        run.h.state_mut().invoke(Cmd::Undo).unwrap();
        run.h.state_mut().selection = None;
        run.calm();
    });
    let painted = doc
        .all_layers()
        .into_iter()
        .filter(|l| l.kind() == Kind::Paint && l.visible && !l.locked && (l.clip || l.children().is_empty()))
        .filter_map(|l| l.cels().first().map(|c| (l, c.id.clone())))
        .find(|(l, c)| {
            !doc.layers.iter().any(|t| t.id == l.id) && bundle_marks(&path, &l.id, c).is_some_and(|m| !m.is_empty())
        });
    if let Some((layer, cel)) = painted {
        let path = run.layer_path(&layer.id);
        run.h.state_mut().activate(path).unwrap();
        run.calm();
        run.h.state_mut().tool = Tool::Strokes;
        let marks = run.h.state().scene.marks_of(&cel).to_vec();
        if let Some(mark) = marks.last() {
            let [x0, y0, x1, y1] = mark.bounds().unwrap();
            run.h.state_mut().picked = Some(Picked::whole(cel.clone(), vec![mark.id()]));
            run.calm();
            run.drag(
                &format!("strokes in {} ({} marks)", layer.name, marks.len()),
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                (120.0, 80.0),
            );
            run.h.state_mut().invoke(Cmd::Undo).unwrap();
            run.h.state_mut().picked = None;
            run.calm();
        }
    }
    let other = doc
        .all_layers()
        .into_iter()
        .rev()
        .find(|l| l.kind() == Kind::Paint && l.visible)
        .map(|l| l.id.clone());
    if let Some(id) = other {
        let path = run.layer_path(&id);
        let start = Instant::now();
        run.h.state_mut().activate(path).unwrap();
        let (_, worst) = run.calm();
        println!(
            "switching to layer {id}: {:.0} ms (slowest frame {worst:.1} ms)",
            ms(start)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn bundle_marks(path: &Path, layer: &str, cel: &str) -> Option<Vec<Mark>> {
    Bundle::open(path).ok()?.read_marks(layer, cel).ok()
}
