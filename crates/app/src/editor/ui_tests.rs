use super::{Direct, Editor, Picked, Tool};
use crate::commands::Cmd;
use crate::layout::{self, Pane};
use crate::messages::Tone;
use crate::settings::Settings;
use crate::theme;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use klad_core::bundle::Bundle;
use klad_core::doc::{Document, Layer};
use klad_core::ops::parse_lines;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const LAYERS: [&str; 7] = ["lines", "colour", "shade", "notes", "rim", "ground", "sky"];

fn sample(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("draw-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut doc = Document::new(512, 384);
    doc.timeline.frames = 24;
    LAYERS.iter().rev().enumerate().for_each(|(i, name)| {
        doc.layers.push(Layer::paint(&format!("l{i}"), name, &format!("c{i}")));
    });
    doc.next_id = 40;
    let mut bundle = Bundle::create(&dir, doc).unwrap();
    bundle
        .apply(
            &parse_lines(
                r#"{"op":"stroke","layer":"l6","path":"M40 40 L300 200","size":8}
{"op":"stroke","layer":"l6","path":"M60 300 L400 80","size":5}
{"op":"add","mask":{"name":"mask"},"in":"l4"}
{"op":"add","layer":{"kind":"vector","name":"shapes"},"index":0}
{"op":"clip","add":"walk","from":0,"to":11,"loop":true}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let shapes = bundle.doc.find_layer("shapes").unwrap().id.clone();
    bundle
        .apply(
            &parse_lines(&format!(
                r##"{{"op":"add","node":{{"type":"rect","x":20,"y":20,"w":120,"h":80,"fill":"#a04030"}},"in":"{shapes}","frame":0}}"##
            ))
            .unwrap(),
        )
        .unwrap();
    dir
}

fn editor(path: &Path) -> Editor {
    let mut editor = Editor::open(egui::Context::default(), path).unwrap();
    editor.settings = Settings::default();
    editor.settings_path = None;
    editor
}

fn harness(editor: Editor, size: egui::Vec2) -> Harness<'static, Editor> {
    harness_stepping(editor, size, 0.25)
}

fn harness_stepping(editor: Editor, size: egui::Vec2, step: f32) -> Harness<'static, Editor> {
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(step)
        .build_ui_state(|ui, editor: &mut Editor| editor.frame(ui), editor);
    h.run_steps(4);
    if h.state().scene.bundle.doc.canvas.width <= 1024 {
        ready(&mut h);
    }
    h
}

fn activate(h: &mut Harness<'static, Editor>, name: &str) {
    let path = {
        let ed = h.state();
        let id = ed.scene.bundle.doc.find_layer(name).unwrap().id.clone();
        match ed.scene.bundle.doc.locate(&id) {
            Some(klad_core::doc::Loc::Layer(path)) => path,
            _ => panic!("no layer {name}"),
        }
    };
    h.state_mut().activate(path).unwrap();
    h.run_steps(2);
    ready(h);
}

fn ready(h: &mut Harness<'static, Editor>) {
    let start = std::time::Instant::now();
    while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(20) {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(h.state().canvas_current(), "the canvas never caught up");
}

const CONTROLS: [Role; 9] = [
    Role::Button,
    Role::CheckBox,
    Role::Slider,
    Role::SpinButton,
    Role::TextInput,
    Role::MultilineTextInput,
    Role::ComboBox,
    Role::RadioButton,
    Role::Switch,
];

fn controls(h: &Harness<'static, Editor>) -> Vec<(Role, String, Rect)> {
    h.query_all(By::new().predicate(|n| CONTROLS.contains(&n.role())))
        .map(|n| {
            let node = n.accesskit_node();
            (node.role(), node.label().unwrap_or_default(), n.rect())
        })
        .collect()
}

fn audit(h: &Harness<'static, Editor>, what: &str) -> usize {
    let hinted = theme::hinted(&h.ctx);
    let found = controls(h);
    let small: Vec<String> = found
        .iter()
        .filter(|(_, _, r)| r.height() < theme::TARGET - 0.5)
        .map(|(role, label, r)| format!("{role:?} {label:?} {:.1} high", r.height()))
        .collect();
    assert!(
        small.is_empty(),
        "{what}: controls under {} pt: {small:#?}",
        theme::TARGET
    );
    let bare: Vec<String> = found
        .iter()
        .filter(|(_, _, r)| !hinted.iter().any(|h| h.expand(1.5).contains_rect(*r)))
        .map(|(role, label, r)| format!("{role:?} {label:?} at {:.0},{:.0}", r.min.x, r.min.y))
        .collect();
    assert!(bare.is_empty(), "{what}: controls without a hover hint: {bare:#?}");
    found.len()
}

#[test]
fn every_control_is_pen_sized_and_has_a_hover_hint() {
    let path = sample("audit");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    let mut checked = 0;
    [Tool::Brush, Tool::Fill, Tool::Note].iter().for_each(|tool| {
        h.state_mut().tool = *tool;
        h.run_steps(2);
        checked += audit(&h, &format!("{tool:?}"));
    });
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().select_all_strokes();
    h.run_steps(2);
    assert!(h.state().picked.is_some());
    checked += audit(&h, "strokes selected");
    activate(&mut h, "shade");
    checked += audit(&h, "a layer with a mask");
    h.state_mut().invoke(Cmd::PaintMask).unwrap();
    h.run_steps(2);
    checked += audit(&h, "painting on a mask");
    activate(&mut h, "shapes");
    let rect = {
        let ed = h.state();
        let layer = ed.scene.bundle.doc.find_layer("shapes").unwrap();
        layer.cels()[0].nodes[0].id.clone()
    };
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(rect.clone());
    h.run_steps(2);
    checked += audit(&h, "a shape selected");
    h.state_mut().tool = Tool::Direct;
    h.state_mut().direct = Some(Direct {
        node: rect,
        points: BTreeSet::from([0, 1]),
    });
    h.run_steps(2);
    checked += audit(&h, "points selected");
    h.state_mut().settings.layout.numbers = true;
    h.state_mut().windows.settings = true;
    h.state_mut().windows.onion = true;
    h.state_mut().windows.log = true;
    h.state_mut().windows.brush = true;
    h.state_mut().say(Tone::Error, "a test error with a close button");
    h.run_steps(3);
    checked += audit(&h, "windows open");
    {
        let ed = h.state_mut();
        ed.settings.layout.numbers = false;
        ed.windows.settings = false;
        ed.windows.onion = false;
        ed.windows.log = false;
        ed.windows.brush = false;
    }
    let lines = h.state().scene.bundle.doc.find_layer("lines").unwrap().id.clone();
    h.state_mut()
        .apply(parse_lines(&format!(r#"{{"op":"cel","new":"{lines}","frame":6}}"#)).unwrap())
        .unwrap();
    {
        let ed = h.state_mut();
        ed.timeline.span = Some(super::tweens::Span {
            from: 0,
            to: 6,
            rows: vec![lines.clone()],
            anchor: (0, 0),
        });
        ed.between.last = Some(super::between::Request {
            layers: vec![lines.clone()],
            from: 0,
            to: 6,
            count: None,
            spacing: "even".into(),
            matching: "id".into(),
            unpaired: "leave".into(),
            pairs: vec![[1, 2]],
        });
        ed.between.report = super::between::report(
            "lines: c6 -> c40, 1 marks paired (0 flood fills), 2 line ends kept touching\n  unpaired, left out: c40:s2 (only in the second key)\n  fills left out: c6:s3 at frame 3: it leaked\n  c41 at frame 1 (t 0.2) on lines",
        );
        ed.between.redo = Some(vec![super::between::RedoRow {
            cel: "c41".into(),
            layer: lines.clone(),
            frame: 1,
            status: super::between::Status::Drawn(2),
        }]);
        ed.windows.between = true;
        ed.windows.clip = true;
    }
    h.run_steps(3);
    assert!(h.query_all(By::new().label_contains("pick its partner")).count() > 0);
    checked += audit(&h, "the in-between and clip windows");
    {
        let ed = h.state_mut();
        ed.windows.between = false;
        ed.windows.clip = false;
        ed.windows.export = true;
        ed.windows.font = true;
        ed.export.what = super::export::What::Sheet;
    }
    h.run_steps(3);
    assert!(h.query_all(By::new().label("page size px")).count() > 0);
    checked += audit(&h, "the export and font windows");
    {
        let ed = h.state_mut();
        ed.windows.export = false;
        ed.windows.font = false;
        ed.windows.paintover = true;
    }
    h.run_steps(3);
    assert!(h.query_all(By::new().label("carry it to the next drawings")).count() > 0);
    checked += audit(&h, "the paint-over window");
    {
        let ed = h.state_mut();
        ed.windows.paintover = false;
        ed.compare = Some(super::compare::Compared {
            report: serde_json::json!({
                "yours": {"value": [40.0, 66.9, 93.4], "chroma": 30.3, "texture": 14.2, "sharpness": 60.4},
                "reference": {"value": [37.3, 68.4, 94.3], "chroma": 27.5, "texture": 10.6, "sharpness": 57.3},
                "difference": 5.2,
                "words": ["10% more saturated", "34% busier"],
            }),
            heat: None,
        });
        ed.windows.compare = true;
    }
    h.run_steps(3);
    assert!(
        h.query_all(By::new().label("difference 5.2: 10% more saturated, 34% busier"))
            .count()
            > 0
    );
    checked += audit(&h, "the compare window");
    assert!(checked > 150, "only {checked} controls checked");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_shell_fits_a_small_window_without_scrolling() {
    let path = sample("fits");
    let mut h = harness(editor(&path), vec2(1280.0, 800.0));
    activate(&mut h, "lines");
    let bottom = 800.0 - layout::STATUS + 0.5;
    [Pane::Tool, Pane::Layers, Pane::Inspector, Pane::Notes, Pane::History]
        .iter()
        .for_each(|pane| {
            let title = h.get_by_label(pane.title());
            assert!(title.rect().bottom() <= bottom, "{} is below the window", pane.title());
        });
    let rows = ["shapes"]
        .iter()
        .chain(LAYERS.iter())
        .filter(|name| {
            h.query_all_by_label(name)
                .filter(|n| n.accesskit_node().role() == Role::Button)
                .any(|n| n.rect().bottom() <= bottom)
        })
        .count();
    assert!(rows >= 6, "only {rows} timeline rows show at the default height");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_layout_comes_back_as_it_was_left() {
    let path = sample("layout");
    let mut first = editor(&path);
    first.settings.layout.dock_width = 410.0;
    first.settings.layout.timeline_height = 300.0;
    first.settings.layout.toggle(Pane::History);
    let mut h = harness(first, vec2(1440.0, 900.0));
    let layout = h.state().settings.layout.clone();
    assert!((layout.dock_width - 410.0).abs() < 0.5, "{}", layout.dock_width);
    assert!(
        (layout.timeline_height - 300.0).abs() < 0.5,
        "{}",
        layout.timeline_height
    );
    assert!(h.query_by_label(Pane::History.title()).is_none());
    let mut again = editor(&path);
    again.settings.layout = layout.clone();
    let h2 = harness(again, vec2(1440.0, 900.0));
    assert_eq!(h2.state().settings.layout, layout);
    h.state_mut().tabs.push(false);
    h.run_steps(2);
    assert!(h.state().settings.layout.hidden);
    assert!(h.query_by_label(Pane::Layers.title()).is_none());
    h.state_mut().tabs.push(false);
    h.state_mut().tabs.push(true);
    h.run_steps(2);
    assert!(!h.state().settings.layout.hidden && !h.state().settings.layout.dock);
    assert!(h.query_by_label(Pane::Layers.title()).is_none());
    let _ = std::fs::remove_dir_all(&path);
}

fn palette_field<'a>(h: &'a Harness<'static, Editor>) -> egui_kittest::Node<'a> {
    let label: &'static str = Box::leak(format!("{} Find a command", theme::icon::SEARCH).into_boxed_str());
    h.get_by_role_and_label(Role::TextInput, label)
}

#[test]
fn cmd_k_finds_and_runs_commands_and_explains_refusals() {
    let path = sample("cmdk");
    let mut h = harness(editor(&path), vec2(1280.0, 800.0));
    activate(&mut h, "lines");
    h.hover_at(pos2(640.0, 200.0));
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    assert!(h.state().palette.is_some());
    palette_field(&h).type_text("onion");
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().palette.is_none());
    assert!(h.state().scene.onion_on);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(2);
    palette_field(&h).type_text("inb");
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let said = h.state().messages.latest().cloned().unwrap();
    assert_eq!(said.tone, Tone::Refused);
    assert!(said.text.starts_with("select frames first"), "{}", said.text);
    let shown: Vec<Rect> = h
        .query_all_by_label_contains("select frames first")
        .map(|n| n.rect())
        .collect();
    assert!(
        shown.iter().any(|r| (r.min - pos2(640.0, 200.0)).length() < 80.0),
        "no toast next to the pointer: {shown:?}"
    );
    assert!(
        shown.iter().any(|r| r.bottom() > 800.0 - layout::STATUS),
        "not in the status line: {shown:?}"
    );
    (0..3).for_each(|_| {
        h.key_press(Key::Period);
        h.run_steps(3);
    });
    assert_eq!(h.state().scene.frame, 3);
    h.key_press(Key::K);
    h.run_steps(2);
    let latest = h.state().messages.latest().cloned().unwrap();
    assert_eq!(latest.tone, Tone::Info, "{}", latest.text);
    assert!(latest.text.starts_with("new drawing"), "{}", latest.text);
    h.key_press(Key::O);
    h.run_steps(2);
    assert!(!h.state().scene.onion_on);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn every_command_answers_whether_it_can_run_and_toggles_say_their_state() {
    let path = sample("commands");
    let mut h = harness(editor(&path), vec2(1280.0, 800.0));
    activate(&mut h, "lines");
    let ed = h.state();
    let runnable: Vec<Cmd> = Cmd::ALL.into_iter().filter(|c| ed.can_run(*c).is_none()).collect();
    assert!(runnable.contains(&Cmd::NewDrawing) && runnable.contains(&Cmd::Play));
    assert!(ed.can_run(Cmd::Copy).is_some() && ed.can_run(Cmd::Pan).is_some());
    assert_eq!(ed.is_on(Cmd::Brush), Some(true));
    assert_eq!(ed.is_on(Cmd::LayersPane), Some(true));
    assert_eq!(ed.is_on(Cmd::Undo), None);
    h.state_mut().picked = Some(Picked::whole("c6".into(), vec![0]));
    h.state_mut().tool = Tool::Strokes;
    assert!(h.state().can_run(Cmd::Copy).is_none());
    let _ = std::fs::remove_dir_all(&path);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    std::fs::read_dir(from).unwrap().for_each(|e| {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        match e.file_type().unwrap().is_dir() {
            true if e.file_name() == ".cache" => {}
            true => copy_dir(&e.path(), &target),
            false => {
                std::fs::copy(e.path(), target).unwrap();
            }
        }
    });
}

#[test]
#[ignore]
fn ui_time_per_frame() {
    let creature = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/creature/creature.draw");
    let copy = std::env::temp_dir().join(format!("draw-ui-creature-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copy);
    if creature.exists() {
        copy_dir(&creature, &copy);
    }
    let sample = sample("time");
    let stress = std::env::temp_dir().join(format!("draw-ui-stress-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stress);
    let exe = std::env::current_exe().unwrap();
    let draw = exe.parent().and_then(Path::parent).unwrap().join("klad");
    if draw.exists() {
        let made = std::process::Command::new(&draw)
            .args(["stress", stress.to_str().unwrap(), "--frames", "48"])
            .output()
            .unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    }
    [
        ("sample", sample.clone()),
        ("creature", copy.clone()),
        ("stress animation", stress.clone()),
    ]
    .into_iter()
    .filter(|(_, p)| p.join("doc.json").exists())
    .for_each(|(name, path)| {
        let mut h = harness(editor(&path), vec2(1600.0, 1000.0));
        h.state_mut().settings.layout.numbers = true;
        h.state_mut().windows.log = true;
        h.state_mut().tool = Tool::Strokes;
        h.run_steps(10);
        let start = std::time::Instant::now();
        while !(h.state().canvas_current() && h.state().dirty.is_empty()) && start.elapsed().as_secs() < 120 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        h.run_steps(30);
        h.state_mut().invoke(Cmd::ResetNumbers).unwrap();
        (0..200).for_each(|i| {
            h.hover_at(pos2(300.0 + (i % 40) as f32 * 20.0, 200.0 + (i % 7) as f32 * 30.0));
            h.step();
        });
        let line = crate::stats::lock(&h.state().stats).ui_ms.spread();
        println!("ui per frame on the {name}, every panel and window open: {line} ms");
    });
    std::fs::remove_dir_all(&sample).unwrap();
    let _ = std::fs::remove_dir_all(&copy);
    let _ = std::fs::remove_dir_all(&stress);
}

#[test]
#[ignore]
fn panel_pictures() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch/ui");
    std::fs::create_dir_all(&out).unwrap();
    let creature = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/creature/creature.draw");
    let path = std::env::temp_dir().join(format!("draw-ui-pictures-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    copy_dir(&creature, &path);
    let mut h = Harness::builder()
        .with_size(vec2(1440.0, 900.0))
        .wgpu()
        .build_ui_state(|ui, editor: &mut Editor| editor.frame(ui), editor(&path));
    h.run_steps(6);
    let shot = |h: &mut Harness<'static, Editor>, name: &str| {
        h.run_steps(4);
        let picture = h.render().unwrap();
        picture.save(out.join(format!("{name}.png"))).unwrap();
    };
    shot(&mut h, "1-shell");
    h.hover_at(pos2(22.0, 30.0));
    h.run_steps(30);
    shot(&mut h, "2-strip-hint");
    h.hover_at(pos2(700.0, 260.0));
    h.run_steps(2);
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().invoke(Cmd::Unlink).unwrap();
    shot(&mut h, "3-refused-toast");
    h.state_mut().palette = Some(super::PaletteState::default());
    h.run_steps(2);
    palette_field(&h).type_text("in");
    shot(&mut h, "4-palette");
    h.key_press(Key::Escape);
    h.state_mut().settings.layout.numbers = true;
    h.state_mut().windows.settings = true;
    shot(&mut h, "5-numbers-and-settings");
    h.state_mut().settings.layout.numbers = false;
    h.state_mut().windows.settings = false;
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().select_all_strokes();
    shot(&mut h, "6-selection-bar");
    h.state_mut().picked = None;
    h.state_mut().tabs.push(false);
    shot(&mut h, "7-tab");
    h.state_mut().tabs.push(false);
    h.run_steps(2);
    let lines = h.state().scene.bundle.doc.find_layer("lines").unwrap().id.clone();
    let keys = {
        let doc = &h.state().scene.bundle.doc;
        let starts: Vec<u32> = doc.layer(&lines).unwrap().cels().iter().map(|c| c.frame).collect();
        starts.windows(2).find(|w| w[1] > w[0] + 1).map(|w| (w[0], w[1]))
    };
    if let Some((from, to)) = keys {
        h.state_mut().go_to(from).unwrap();
        h.state_mut().timeline.span = Some(super::tweens::Span {
            from,
            to,
            rows: vec![lines.clone()],
            anchor: (from, 0),
        });
        h.state_mut().windows.between = true;
        h.state_mut().between.report = super::between::report(&format!(
            "lines: c{from} -> c{to}, 15 marks paired (0 flood fills), by part first: body, leaf, 16 line ends kept touching\n  unpaired, left out: c{to}:s16 (only in the second key)\n  fills left out: colour{from}:s3 at frame {}: it leaked (900 px where the keys have 300 and 320)",
            from + 1
        ));
        shot(&mut h, "8-inbetween-panel");
        h.state_mut().windows.between = false;
        h.state_mut().timeline.span = None;
    }
    h.state_mut().windows.settings = true;
    h.state_mut().key_filter = "layer".into();
    shot(&mut h, "9-settings-keys");
    h.state_mut().windows.settings = false;
    [Pane::Tool, Pane::Layers, Pane::Inspector]
        .into_iter()
        .for_each(|pane| {
            if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
                p.collapsed = true;
            }
        });
    [Pane::Notes, Pane::History].into_iter().for_each(|pane| {
        if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
            p.collapsed = false;
        }
    });
    let frame = h.state().scene.frame;
    klad_core::notes::add(
        &h.state().scene.bundle,
        (520.0, 380.0),
        frame,
        "the sprout should lag more",
        "user",
    )
    .unwrap();
    klad_core::notes::add(
        &h.state().scene.bundle,
        (600.0, 700.0),
        frame + 2,
        "feet slide here",
        "user",
    )
    .unwrap();
    let done = klad_core::notes::add(&h.state().scene.bundle, (420.0, 300.0), frame, "darker rim", "user").unwrap();
    klad_core::notes::set_done(&h.state().scene.bundle, done.id, true).unwrap();
    h.state_mut().pins = klad_core::notes::read(&h.state().scene.bundle);
    h.state_mut().tool = Tool::Note;
    h.state_mut().open_pin(1);
    shot(&mut h, "10-history-notes-pins");
    h.state_mut().note_open = None;
    h.state_mut().timeline.range = Some((0, 9));
    h.state_mut().invoke(Cmd::Prepare).unwrap();
    settle(&mut h, |ed| ed.buffering.preparing.is_none());
    settle(&mut h, |ed| ed.buffering.job_idle());
    h.run_steps(4);
    shot(&mut h, "11-prepared-frames");
    h.state_mut().tool = Tool::Brush;
    h.state_mut().pick("dry-brush");
    h.state_mut().windows.brush = true;
    h.run_steps(4);
    shot(&mut h, "12-brush-window");
    h.state_mut().windows.brush = false;
    [(Pane::Inspector, false), (Pane::Notes, true), (Pane::History, true)]
        .into_iter()
        .for_each(|(pane, collapsed)| {
            if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
                p.collapsed = collapsed;
            }
        });
    h.state_mut().invoke(Cmd::NewPaintLayer).unwrap();
    let light = h.state().scene.active_id().unwrap();
    h.state_mut()
        .set(
            &light,
            vec![
                ("blend".into(), serde_json::json!("add")),
                (
                    "effects".into(),
                    serde_json::json!([{"glow": {"radius": 24, "strength": 0.8}}, {"outline": {"width": 3}}]),
                ),
            ],
        )
        .unwrap();
    h.run_steps(4);
    shot(&mut h, "13-effects");
    h.state_mut().invoke(Cmd::NewAdjustLayer).unwrap();
    h.run_steps(4);
    shot(&mut h, "14-curves");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn each_tool_shows_its_options_and_the_strip_shows_the_eraser() {
    let path = sample("options");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    [
        (Key::B, Cmd::Brush, "brush"),
        (Key::G, Cmd::Fill, "fill"),
        (Key::S, Cmd::Strokes, "select strokes"),
        (Key::V, Cmd::Shapes, "select and move"),
        (Key::A, Cmd::Points, "edit points"),
        (Key::P, Cmd::Pen, "pen"),
        (Key::N, Cmd::Note, "note"),
        (Key::E, Cmd::Eraser, "eraser"),
    ]
    .iter()
    .for_each(|(key, cmd, name)| {
        h.key_press(*key);
        h.run_steps(2);
        let label = format!("{} {name}", cmd.spec().icon);
        assert!(h.query_all_by_label(&label).count() == 1, "no options for {name}");
    });
    assert_eq!(h.state().is_on(Cmd::Eraser), Some(true));
    h.key_press(Key::B);
    h.run_steps(2);
    assert_eq!(h.state().is_on(Cmd::Eraser), Some(false));
    let rect = h.state().canvas_rect;
    let ctx = h.ctx.clone();
    let turned = crate::pen::PenEvent {
        phase: crate::pen::Phase::Move,
        pos: rect.center(),
        pressure: 0.0,
        tilt: (0.0, 0.0),
        eraser: true,
        tablet: true,
        device: 7,
        at: std::time::Instant::now(),
    };
    h.state_mut().on_pen(&ctx, turned, rect, true).unwrap();
    assert_eq!(h.state().is_on(Cmd::Eraser), Some(true));
    assert_eq!(h.state().is_on(Cmd::Brush), Some(false));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_stroke_in_a_flipped_view_lands_under_the_pen() {
    let path = sample("flip");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "notes");
    h.key_press(Key::H);
    h.run_steps(2);
    let view = h.state().view.unwrap();
    assert!(view.flip);
    let rect = h.state().canvas_rect;
    let (a, b) = (rect.center() + vec2(-60.0, -20.0), rect.center() + vec2(40.0, 30.0));
    h.hover_at(a);
    h.run_steps(1);
    h.drag_at(a);
    h.run_steps(1);
    (1..=8).for_each(|i| {
        h.hover_at(a + (b - a) * (i as f32 / 8.0));
        h.run_steps(1);
    });
    h.drop_at(b);
    h.run_steps(3);
    let marks = h.state().scene.active_marks().to_vec();
    let stroke = marks
        .last()
        .and_then(|m| m.as_stroke())
        .expect("a stroke on notes")
        .clone();
    let first = stroke.samples.first().unwrap().pos();
    let expect = view.to_canvas(a - rect.min, rect.size());
    assert!(
        (first.0 - expect.x).abs() < 1.5 && (first.1 - expect.y).abs() < 1.5,
        "{first:?} vs {expect:?}"
    );
    let unflipped = egui::Vec2::new(2.0 * view.center.x - expect.x, expect.y);
    assert!(
        (first.0 - unflipped.x).abs() > 5.0,
        "the stroke landed as if the view were not flipped"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn palette_swatches_move_by_dragging_and_go_from_their_corner() {
    let path = sample("palette");
    {
        let mut b = Bundle::open(&path).unwrap();
        b.apply(
            &parse_lines(
                r##"{"op":"palette","add":"#ff0000","name":"red"}
{"op":"palette","add":"#00ff00","name":"green"}
{"op":"palette","add":"#0000ff","name":"blue"}"##,
            )
            .unwrap(),
        )
        .unwrap();
    }
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let order = |h: &Harness<'static, Editor>| -> Vec<String> {
        h.state()
            .scene
            .bundle
            .doc
            .palette
            .iter()
            .map(|s| s.name.clone())
            .collect()
    };
    assert_eq!(order(&h), ["red", "green", "blue"]);
    let at = |h: &Harness<'static, Editor>, hex: &str| h.get_by_role_and_label(Role::Button, hex).rect();
    let (red, blue) = (at(&h, "#ff0000"), at(&h, "#0000ff"));
    h.drag_at(red.center());
    h.run_steps(1);
    (1..=6).for_each(|i| {
        h.hover_at(red.center() + (blue.center() - red.center()) * (i as f32 / 6.0));
        h.run_steps(1);
    });
    h.drop_at(blue.center());
    h.run_steps(3);
    assert_eq!(order(&h), ["green", "blue", "red"]);
    let green = at(&h, "#00ff00");
    let corner = egui::pos2(green.right() - 6.0, green.top() + 6.0);
    h.hover_at(corner);
    h.run_steps(1);
    h.drag_at(corner);
    h.run_steps(1);
    h.drop_at(corner);
    h.run_steps(3);
    assert_eq!(order(&h), ["blue", "red"]);
    let _ = std::fs::remove_dir_all(&path);
}

fn settle(h: &mut Harness<'static, Editor>, done: impl Fn(&Editor) -> bool) {
    let start = std::time::Instant::now();
    while !done(h.state()) && start.elapsed() < std::time::Duration::from_secs(20) {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(done(h.state()), "the editor did not settle");
}

fn drag(h: &mut Harness<'static, Editor>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    (1..=5).for_each(|i| {
        h.hover_at(from + (to - from) * (i as f32 / 5.0));
        h.run_steps(1);
    });
    h.drop_at(to);
    h.run_steps(1);
}

#[test]
fn a_shape_drag_started_while_the_last_one_is_written_still_happens() {
    let path = sample("held");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "shapes");
    let id = {
        let ed = h.state();
        ed.scene.bundle.doc.find_layer("shapes").unwrap().cels()[0].nodes[0]
            .id
            .clone()
    };
    let before = h.state().node_transform(&id).unwrap();
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(id.clone());
    settle(&mut h, |ed| {
        ed.lift
            .as_ref()
            .is_some_and(|l| l.ready.as_ref().is_some_and(|(_, p)| l.stage >= p.runs.len() + 2))
    });
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let middle = rect.min + view.to_screen(egui::Vec2::new(80.0, 60.0), rect.size());
    drag(&mut h, middle, middle + vec2(40.0, 0.0));
    let writing = h.state().writing();
    assert!(
        writing,
        "the first move was already written; the test would not cover a press during a write"
    );
    drag(&mut h, middle + vec2(40.0, 0.0), middle + vec2(40.0, 50.0));
    settle(&mut h, |ed| !ed.writing() && ed.drag.is_none() && ed.held.len() == 0);
    h.run_steps(3);
    settle(&mut h, |ed| !ed.writing() && ed.drag.is_none());
    let after = h.state().node_transform(&id).unwrap();
    let moved = (after.x - before.x, after.y - before.y);
    let expect = (40.0 / view.zoom, 50.0 / view.zoom);
    assert!(
        (moved.0 - expect.0).abs() < 2.0 && (moved.1 - expect.1).abs() < 2.0,
        "moved {moved:?}, expected {expect:?} (the first drag was {}being written when the second began)",
        if writing { "" } else { "not " }
    );
    let _ = std::fs::remove_dir_all(&path);
}

fn top_down(h: &Harness<'static, Editor>) -> Vec<String> {
    h.state()
        .layer_rows(super::layers::Where::Panel)
        .into_iter()
        .map(|r| r.name)
        .collect()
}

fn layer_named<'a>(h: &'a Harness<'static, Editor>, name: &str) -> &'a Layer {
    h.state().scene.bundle.doc.find_layer(name).unwrap()
}

#[test]
fn layer_rows_drag_rename_duplicate_group_and_delete() {
    let path = sample("rows");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    [Pane::Tool, Pane::Inspector].into_iter().for_each(|pane| {
        if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
            p.collapsed = true;
        }
    });
    h.run_steps(2);
    assert_eq!(
        top_down(&h)[..4],
        ["lines", "colour", "shade", "mask"].map(String::from)
    );
    let row = |h: &Harness<'static, Editor>, name: &str| {
        h.query_all(By::new().role(Role::Button).label(name))
            .map(|n| n.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap()
    };
    assert!(
        row(&h, "rim").top() < h.state().timeline.geom.unwrap().cell_centre(0, 0).y - 100.0,
        "the drags below start in the Layers pane, not the timeline"
    );
    let (rim, lines) = (row(&h, "rim"), row(&h, "lines"));
    drag(
        &mut h,
        egui::pos2(rim.right() - 60.0, rim.center().y),
        egui::pos2(lines.right() - 60.0, lines.top() + 3.0),
    );
    h.run_steps(2);
    assert_eq!(top_down(&h)[..2], ["rim", "lines"].map(String::from));
    activate(&mut h, "rim");
    h.state_mut().invoke(Cmd::RenameLayer).unwrap();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, "rename")
        .type_text("rim light");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(top_down(&h)[0], "rim light");
    activate(&mut h, "lines");
    h.state_mut().invoke(Cmd::DuplicateLayer).unwrap();
    h.run_steps(2);
    assert_eq!(
        top_down(&h)[..3],
        ["rim light", "lines copy", "lines"].map(String::from)
    );
    assert_eq!(h.state().scene.active_layer().unwrap().name, "lines copy");
    let copy = layer_named(&h, "lines copy").clone();
    let marks = h.state().scene.bundle.read_marks(&copy.id, &copy.cels()[0].id).unwrap();
    assert_eq!(marks.len(), 2);
    activate(&mut h, "notes");
    h.state_mut().invoke(Cmd::GroupLayer).unwrap();
    h.run_steps(2);
    let group = layer_named(&h, "notes group").clone();
    assert_eq!(
        group.children().iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["notes"]
    );
    let (colour, target) = (row(&h, "colour"), row(&h, "notes group"));
    drag(
        &mut h,
        egui::pos2(colour.right() - 60.0, colour.center().y),
        egui::pos2(target.right() - 60.0, target.center().y),
    );
    h.run_steps(2);
    let group = layer_named(&h, "notes group").clone();
    assert_eq!(
        group.children().iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["notes", "colour"]
    );
    activate(&mut h, "lines copy");
    h.state_mut().invoke(Cmd::DeleteLayer).unwrap();
    h.run_steps(2);
    assert!(h.state().scene.bundle.doc.find_layer("lines copy").is_err());
    assert!(h.state().scene.active_layer().is_some());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn timeline_cells_select_by_dragging_copy_paste_move_and_open_the_clip() {
    let path = sample("cells");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    let geom = h.state().timeline.geom.unwrap();
    drag(&mut h, geom.cell_centre(2, 0), geom.cell_centre(5, 0));
    h.run_steps(2);
    let span = h.state().timeline.span.clone().unwrap();
    assert_eq!((span.from, span.to, span.rows.len()), (2, 5, 1));
    h.state_mut().invoke(Cmd::CopyCells).unwrap();
    h.run_steps(1);
    h.state_mut().go_to(10).unwrap();
    h.state_mut().invoke(Cmd::PasteCells).unwrap();
    h.run_steps(2);
    let starts = |h: &Harness<'static, Editor>| -> Vec<(u32, Option<u32>)> {
        layer_named(h, "lines")
            .cels()
            .iter()
            .map(|c| (c.frame, c.hold))
            .collect()
    };
    assert!(starts(&h).contains(&(10, Some(4))), "{:?}", starts(&h));
    let pasted = layer_named(&h, "lines")
        .cels()
        .iter()
        .find(|c| c.frame == 10)
        .unwrap()
        .id
        .clone();
    let id = layer_named(&h, "lines").id.clone();
    assert_eq!(h.state().scene.bundle.read_marks(&id, &pasted).unwrap().len(), 2);
    h.state_mut().invoke(Cmd::MoveLater).unwrap();
    h.run_steps(2);
    assert!(starts(&h).contains(&(11, Some(4))), "{:?}", starts(&h));
    assert_eq!(h.state().scene.frame, 11);
    let geom = h.state().timeline.geom.unwrap();
    drag(
        &mut h,
        geom.cell_centre(11, 0) + vec2(3.0, 0.0),
        geom.cell_centre(14, 0),
    );
    h.run_steps(2);
    assert!(starts(&h).contains(&(14, Some(4))), "{:?}", starts(&h));
    h.state_mut().go_to(3).unwrap();
    h.state_mut().invoke(Cmd::ClipSettings).unwrap();
    h.run_steps(2);
    assert!(h.query_all(By::new().label_contains("Clip walk")).count() > 0);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_history_lists_you_and_the_agent_and_steps_back_to_a_clicked_step() {
    let path = sample("history");
    {
        let mut b = Bundle::open(&path).unwrap();
        b.apply(&parse_lines(r#"{"op":"stroke","layer":"l6","path":"M10 10 L50 50"}"#).unwrap())
            .unwrap();
        b.apply_as(
            &parse_lines(r#"{"op":"stroke","layer":"l6","path":"M60 10 L90 50"}"#).unwrap(),
            "editor",
        )
        .unwrap();
        b.apply(&parse_lines(r#"{"op":"stroke","layer":"l6","path":"M100 10 L140 50"}"#).unwrap())
            .unwrap();
    }
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    [Pane::Tool, Pane::Layers, Pane::Inspector]
        .into_iter()
        .for_each(|pane| {
            if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
                p.collapsed = true;
            }
        });
    if let Some(p) = h.state_mut().settings.layout.pane_mut(Pane::History) {
        p.collapsed = false;
    }
    h.run_steps(3);
    let strokes = |h: &Harness<'static, Editor>| h.state().scene.bundle.read_marks("l6", "c6").unwrap().len();
    assert_eq!(strokes(&h), 5);
    let rows = h.state_mut().history_rows();
    assert_eq!(
        rows.iter().take(3).map(|r| r.who.as_str()).collect::<Vec<_>>(),
        ["agent", "you", "agent"]
    );
    assert!(h.query_all(By::new().label_contains("the document is here")).count() > 0);
    let click = |h: &Harness<'static, Editor>, who: &str| {
        h.query_all(By::new().role(Role::Button).label_contains(who))
            .next()
            .unwrap()
            .click();
    };
    click(&h, "you  ");
    h.run_steps(3);
    assert_eq!(strokes(&h), 4);
    let rows = h.state_mut().history_rows();
    assert!(rows[0].undone && rows[0].what.contains("stroke"));
    assert_eq!(rows.iter().filter(|r| r.undone).count(), 1);
    click(&h, "undone  ");
    h.run_steps(3);
    assert_eq!(strokes(&h), 5);
    assert!(h.state_mut().history_rows().iter().all(|r| !r.undone));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_note_pinned_on_the_canvas_is_marked_done_by_the_agent_and_edited_from_its_pin() {
    use klad_core::notes;
    let path = sample("notes");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut().go_to(12).unwrap();
    if let Some(p) = h.state_mut().settings.layout.pane_mut(Pane::Notes) {
        p.collapsed = false;
    }
    h.state_mut().tool = Tool::Note;
    h.run_steps(3);
    let c = h.state().canvas_rect.center();
    drag(&mut h, c, c);
    h.run_steps(2);
    assert!(h.state().note_at.is_some());
    h.query_all(By::new().label_contains("a note for the agent"))
        .find(|n| n.accesskit_node().role() == Role::MultilineTextInput)
        .unwrap()
        .type_text("move the lamp left");
    h.run_steps(2);
    checked_popover(&h);
    h.get_by_role_and_label(Role::Button, "pin it").click();
    h.run_steps(2);
    let pinned = notes::read(&Bundle::open(&path).unwrap());
    assert_eq!(pinned.len(), 1);
    assert_eq!((pinned[0].frame, pinned[0].text.as_str()), (12, "move the lamp left"));
    notes::set_done(&Bundle::open(&path).unwrap(), pinned[0].id, true).unwrap();
    settle(&mut h, |e| e.pins.iter().any(|n| n.done));
    h.run_steps(2);
    assert!(h.query_all(By::new().label_contains("done (1)")).count() > 0);
    h.state_mut().go_to(0).unwrap();
    h.state_mut().open_pin(pinned[0].id);
    h.run_steps(2);
    h.state_mut().note_edit = "move the lamp right".into();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "save").click();
    h.run_steps(2);
    assert_eq!(
        notes::read(&Bundle::open(&path).unwrap())[0].text,
        "move the lamp right"
    );
    h.get_by_role_and_label(Role::Button, "close").click();
    h.run_steps(2);
    assert!(h.state().note_open.is_none());
    let _ = std::fs::remove_dir_all(&path);
}

fn checked_popover(h: &Harness<'static, Editor>) {
    assert!(audit(h, "the pin popover") > 0);
}

#[test]
fn a_key_changed_in_the_settings_runs_its_command_and_a_taken_key_is_refused() {
    let path = sample("keys");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.state_mut().windows.settings = true;
    h.state_mut().key_filter = "duplicate the layer".into();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "change").click();
    h.run_steps(2);
    assert_eq!(h.state().capturing, Some(Cmd::DuplicateLayer));
    h.key_press_modifiers(Modifiers::SHIFT, Key::D);
    h.run_steps(2);
    assert_eq!(h.state().settings.keys["layer.duplicate"], ["shift D"]);
    assert!(h.state().command_hint(Cmd::DuplicateLayer).contains("shift D"));
    h.state_mut().windows.settings = false;
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::D);
    h.run_steps(3);
    assert!(h.state().scene.bundle.doc.find_layer("lines copy").is_ok());
    h.state_mut().capturing = Some(Cmd::Brush);
    h.run_steps(1);
    h.key_press(Key::E);
    h.run_steps(2);
    let refused = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    assert!(refused.contains("eraser"), "{refused}");
    assert!(!h.state().settings.keys.contains_key("tool.brush"));
    h.state_mut().settings.ui_scale = 1.25;
    h.run_steps(2);
    assert!((h.ctx.zoom_factor() - 1.25).abs() < 1e-3);
    {
        let ed = h.state_mut();
        ed.settings.defaults.size = Some(30.0);
        ed.settings.defaults.exposure = 3;
        ed.settings.defaults.onion_before = 4;
        ed.use_defaults();
    }
    assert_eq!(h.state().size, 30.0);
    assert_eq!(h.state().timeline.on, 3);
    assert_eq!(h.state().scene.onion.before, 4);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn v_picks_and_drags_the_stroke_under_the_pen_and_alt_drags_a_copy() {
    let path = sample("move");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "notes");
    h.state_mut().tool = Tool::Select;
    h.run_steps(2);
    ready(&mut h);
    let marks = |h: &Harness<'static, Editor>| h.state().scene.bundle.read_marks("l6", "c6").unwrap();
    let start = |m: &klad_core::stroke::Mark| m.as_stroke().unwrap().samples[0].pos();
    let before = marks(&h);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let on_line = rect.min + view.to_screen(egui::Vec2::new(170.0, 120.0), rect.size());
    drag(&mut h, on_line, on_line + vec2(40.0, 0.0));
    settle(&mut h, |ed| !ed.writing() && ed.drag.is_none());
    if start(&marks(&h)[0]) == start(&before[0]) {
        ready(&mut h);
        drag(&mut h, on_line, on_line + vec2(40.0, 0.0));
        settle(&mut h, |ed| !ed.writing() && ed.drag.is_none());
    }
    h.run_steps(2);
    assert_eq!(h.state().scene.active_layer().unwrap().name, "lines");
    let after = marks(&h);
    assert_eq!(after.len(), before.len());
    let moved = start(&after[0]).0 - start(&before[0]).0;
    assert!((moved - 40.0 / view.zoom).abs() < 1.5, "moved {moved}");
    assert_eq!(start(&after[1]), start(&before[1]));
    let on_line = on_line + vec2(40.0, 0.0);
    h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut h, on_line, on_line + vec2(0.0, 30.0));
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    settle(&mut h, |ed| !ed.writing() && ed.drag.is_none());
    h.run_steps(2);
    let copied = marks(&h);
    assert_eq!(
        copied.len(),
        before.len() + 1,
        "alt drag leaves the original and moves a copy"
    );
    assert_eq!(start(&copied[0]), start(&after[0]));
    let dropped = start(copied.last().unwrap());
    assert!(
        (dropped.1 - start(&after[0]).1 - 30.0 / view.zoom).abs() < 1.5,
        "{dropped:?}"
    );
    assert_eq!(h.state().tool, Tool::Select);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn an_agent_edit_while_the_editor_is_open_shows_in_the_history_and_keeps_its_author() {
    let path = sample("agent-row");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    if let Some(p) = h.state_mut().settings.layout.pane_mut(Pane::History) {
        p.collapsed = false;
    }
    h.run_steps(3);
    Bundle::open(&path)
        .unwrap()
        .apply(&parse_lines(r#"{"op":"stroke","layer":"l6","path":"M100 100 L300 300"}"#).unwrap())
        .unwrap();
    settle(&mut h, |ed| {
        ed.scene.bundle.read_marks("l6", "c6").unwrap().len() == 3
            && ed.scene.disk_rev() == Some(ed.scene.bundle.doc.rev)
    });
    h.run_steps(2);
    let first = |h: &mut Harness<'static, Editor>| {
        let rows = h.state_mut().history_rows();
        (rows[0].who.clone(), rows[0].what.clone(), rows[0].undone)
    };
    assert_eq!(first(&mut h), ("agent".into(), "stroke on l6".into(), false));
    assert!(
        h.query_all(By::new().role(Role::Button).label_contains("agent  stroke on l6"))
            .count()
            > 0
    );
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    assert_eq!(first(&mut h), ("agent".into(), "stroke on l6".into(), true));
    h.state_mut().invoke(Cmd::Redo).unwrap();
    h.run_steps(2);
    assert_eq!(first(&mut h), ("agent".into(), "stroke on l6".into(), false));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_right_click_with_the_note_tool_goes_to_a_note_and_delete_removes_it() {
    use klad_core::notes;
    let path = sample("note-pick");
    notes::add(&Bundle::open(&path).unwrap(), (200.0, 150.0), 12, "the lamp", "user").unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut().tool = Tool::Note;
    h.run_steps(3);
    assert_eq!(h.state().scene.frame, 0);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let pin = rect.min + view.to_screen(egui::Vec2::new(200.0, 150.0), rect.size());
    h.hover_at(pin);
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos: pin,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos: pin,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    assert_eq!(h.state().scene.frame, 12);
    let id = notes::read(&Bundle::open(&path).unwrap())[0].id;
    assert_eq!(h.state().note_open, Some(id));
    assert!(h.state().note_at.is_none(), "a right click does not start a new note");
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert!(notes::read(&Bundle::open(&path).unwrap()).is_empty());
    assert!(h.state().note_open.is_none());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_drag_from_a_drawings_first_cell_selects_unless_it_starts_on_the_dot() {
    let path = sample("first-cell");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    let geom = h.state().timeline.geom.unwrap();
    let corner = geom.cell_centre(0, 0) + vec2(-7.0, -10.0);
    drag(&mut h, corner, geom.cell_centre(3, 0));
    h.run_steps(2);
    let span = h
        .state()
        .timeline
        .span
        .clone()
        .expect("a selection from the first cell");
    assert_eq!((span.from, span.to), (0, 3));
    let cels =
        |h: &Harness<'static, Editor>| -> Vec<u32> { layer_named(h, "lines").cels().iter().map(|c| c.frame).collect() };
    assert_eq!(cels(&h), [0]);
    drag(&mut h, geom.cell_centre(0, 0), geom.cell_centre(4, 0));
    h.run_steps(2);
    assert_eq!(cels(&h), [4], "the dot moves the drawing");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
#[ignore]
fn a_heavy_animation_keeps_the_ui_responsive_while_frames_load() {
    let stress = std::env::temp_dir().join(format!("draw-ui-heavy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stress);
    let exe = std::env::current_exe().unwrap();
    let draw = exe.parent().and_then(Path::parent).unwrap().join("klad");
    let size = std::env::var("KLAD_HEAVY_SIZE").unwrap_or_else(|_| "2048".into());
    let made = std::process::Command::new(&draw)
        .args(["stress", stress.to_str().unwrap(), "--frames", "12", "--size", &size])
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let t = std::time::Instant::now();
    let mut h = harness(editor(&stress), vec2(1600.0, 1000.0));
    let opened = t.elapsed().as_secs_f64() * 1000.0;
    let mut times: Vec<f64> = Vec::new();
    let mut step = |h: &mut Harness<'static, Editor>| {
        let t = std::time::Instant::now();
        h.run_steps(1);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(std::time::Duration::from_millis(8));
    };
    let start = std::time::Instant::now();
    while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(60) {
        step(&mut h);
    }
    let first = start.elapsed().as_secs_f64() * 1000.0;
    let mut ready = Vec::new();
    [2u32, 4, 6].into_iter().for_each(|f| {
        h.state_mut().go_to(f).unwrap();
        let start = std::time::Instant::now();
        while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(60) {
            step(&mut h);
        }
        ready.push((f, start.elapsed().as_millis(), h.state().scene.cache_bytes() >> 20));
    });
    h.state_mut().go_to(10).unwrap();
    h.run_steps(2);
    let c = h.state().canvas_rect.center();
    let t = std::time::Instant::now();
    h.hover_at(c);
    h.run_steps(1);
    h.drag_at(c);
    h.run_steps(1);
    let press = t.elapsed().as_secs_f64() * 1000.0;
    let said = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    h.drop_at(c);
    h.run_steps(1);
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    eprintln!(
        "{size} px: editor open {opened:.0} ms, frame 0 editable after {first:.0} ms more, frames ready (frame, ms, MB of drawings) {ready:?}, UI frames while loading: median {:.1} ms, p95 {:.1}, slowest {:.0} of {}; pen down on a loading frame {press:.0} ms ({said})",
        at(0.5),
        at(0.95),
        at(1.0),
        times.len()
    );
    assert!(said.contains("still loading"), "{said}");
    assert!(press < 250.0 && opened < 1500.0, "the UI waited");
    let start = std::time::Instant::now();
    while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(120) {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    drop(h);
    let mut h = harness(editor(&stress), vec2(1600.0, 1000.0));
    let start = std::time::Instant::now();
    while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(60) {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let warm_first = start.elapsed().as_millis();
    let warm: Vec<(u32, u128, u128)> = [2u32, 4, 6]
        .into_iter()
        .map(|f| {
            h.state_mut().go_to(f).unwrap();
            let start = std::time::Instant::now();
            let mut seen = None;
            while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(60) {
                h.run_steps(1);
                if seen.is_none() && h.state().timeline.on_screen == Some(f) {
                    seen = Some(start.elapsed().as_millis());
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            (f, seen.unwrap_or(0), start.elapsed().as_millis())
        })
        .collect();
    eprintln!(
        "{size} px reopened: frame 0 editable after {warm_first} ms, frames (frame, on screen ms, ready to draw ms) {warm:?}"
    );
    h.state_mut().timeline.range = Some((0, 11));
    let start = std::time::Instant::now();
    h.state_mut().invoke(Cmd::Prepare).unwrap();
    let mut prepare_frames: Vec<f64> = Vec::new();
    while h.state().buffering.preparing.is_some() && start.elapsed() < std::time::Duration::from_secs(600) {
        let t = std::time::Instant::now();
        h.run_steps(1);
        prepare_frames.push(t.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let prepared = start.elapsed().as_millis();
    settle(&mut h, |ed| {
        ed.buffering.job_idle() && ed.buffering.shown.watch.len() >= 12
    });
    prepare_frames.sort_by(f64::total_cmp);
    let after: Vec<(u32, u128, u128)> = [8u32, 10, 11]
        .into_iter()
        .map(|f| {
            h.state_mut().go_to(f).unwrap();
            let start = std::time::Instant::now();
            let mut seen = None;
            while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(60) {
                h.run_steps(1);
                if seen.is_none() && h.state().timeline.on_screen == Some(f) {
                    seen = Some(start.elapsed().as_millis());
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            (f, seen.unwrap_or(0), start.elapsed().as_millis())
        })
        .collect();
    eprintln!(
        "{size} px prepare frames 0 to 11: {prepared} ms (UI frames meanwhile median {:.1} ms, slowest {:.0}); frames never visited, after it (frame, on screen ms, ready to draw ms) {after:?}",
        prepare_frames[prepare_frames.len() / 2],
        prepare_frames.last().copied().unwrap_or(0.0)
    );
    let _ = std::fs::remove_dir_all(&stress);
}

#[test]
fn the_pen_draws_closed_and_open_paths_on_a_vector_layer() {
    let path = sample("pen");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    h.key_press(Key::P);
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Pen);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let click = |h: &mut Harness<'static, Editor>, p: egui::Pos2| {
        h.hover_at(p);
        h.run_steps(1);
        h.drag_at(p);
        h.run_steps(1);
        h.drop_at(p);
        h.run_steps(1);
    };
    click(&mut h, at(300.0, 100.0));
    let refused = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    assert!(refused.contains("vector layer"), "{refused}");
    activate(&mut h, "shapes");
    let nodes = |h: &Harness<'static, Editor>| layer_named(h, "shapes").cels()[0].nodes.clone();
    let before = nodes(&h).len();
    [(300.0, 100.0), (400.0, 120.0), (350.0, 220.0)]
        .into_iter()
        .for_each(|(x, y)| click(&mut h, at(x, y)));
    click(&mut h, at(300.0, 100.0));
    h.run_steps(2);
    let after = nodes(&h);
    assert_eq!(after.len(), before + 1);
    let added = after.last().unwrap();
    match &added.shape {
        klad_core::doc::Shape::Path { d } => assert!(d.trim_end().ends_with('Z'), "{d}"),
        other => panic!("{other:?}"),
    }
    assert!(added.fill.is_some() && added.stroke.is_some());
    click(&mut h, at(100.0, 300.0));
    drag(&mut h, at(200.0, 300.0), at(240.0, 260.0));
    h.key_press(Key::Enter);
    h.run_steps(2);
    let after = nodes(&h);
    assert_eq!(after.len(), before + 2);
    let open = after.last().unwrap();
    match &open.shape {
        klad_core::doc::Shape::Path { d } => {
            assert!(
                d.contains('C') && !d.contains('Z'),
                "a dragged point makes a curve: {d}"
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(open.fill.is_none() && open.stroke.is_some());
    assert!(h.state().pen_path.is_none());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn prepare_frames_buffers_the_clip_and_the_ruler_shows_it() {
    let path = sample("prepare");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    settle(&mut h, |ed| ed.buffering.job_idle());
    assert_eq!(h.state().prepare_span(), (0, 11), "the clip under the playhead");
    let pictures = h.state().scene.pictures();
    let on_disk = |dir: &Path| std::fs::read_dir(dir).map_or(0, |d| d.count());
    let before = on_disk(&pictures);
    h.get_by_role_and_label(Role::Button, Cmd::Prepare.spec().icon).click();
    h.run_steps(2);
    assert_eq!(h.state().is_on(Cmd::Prepare), Some(true));
    settle(&mut h, |ed| ed.buffering.preparing.is_none());
    settle(&mut h, |ed| {
        ed.buffering.shown.watch.len() >= 12 && ed.buffering.job_idle()
    });
    let shown = h.state().buffering.shown.clone();
    assert!(
        (0..12).all(|f| shown.watch.contains(&f) && shown.draw.contains(&f)),
        "{shown:?}"
    );
    assert!(on_disk(&pictures) >= before.max(1));
    let keys: Vec<u64> = (0..12)
        .map(|f| {
            let ed = h.state_mut();
            ed.frames.key(&mut ed.scene, f, false, &ed.brushes)
        })
        .collect();
    assert!(
        keys.iter()
            .all(|k| crate::composite::picture_path(&pictures, *k).exists()),
        "the clip's pictures are on disk"
    );
    let tiles = path.join(".cache/tiles");
    assert!(on_disk(&tiles) >= 2, "its drawings are cached");
    let said = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    assert!(said.contains("prepared"), "{said}");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_brush_window_edits_a_brush_shows_it_and_saves_it_in_the_drawing() {
    let path = sample("brush-window");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::F5);
    h.run_steps(3);
    assert!(h.state().windows.brush);
    let shown = |h: &Harness<'static, Editor>| h.state().brush_window_preview_key();
    let before = shown(&h);
    h.get_by_role_and_label(Role::CheckBox, "dual tip").click();
    h.run_steps(3);
    assert!(h.state().custom.as_ref().is_some_and(|d| d.dual.is_some()));
    assert_ne!(shown(&h), before, "the sample shows the change");
    h.get_by_label("wet").click();
    h.run_steps(3);
    assert!(matches!(
        h.state().custom.as_ref().and_then(|d| d.paint),
        Some(klad_core::brushes::PaintDef::Wet { .. })
    ));
    h.key_press(Key::F5);
    h.run_steps(2);
    let rect = h.state().canvas_rect;
    let (a, b) = (rect.center() + vec2(-80.0, 0.0), rect.center() + vec2(80.0, 20.0));
    h.key_press(Key::B);
    h.run_steps(1);
    let count = h.state().scene.active_marks().len();
    drag(&mut h, a, b);
    h.run_steps(2);
    assert_eq!(h.state().scene.active_marks().len(), count + 1);
    let drawn = h
        .state()
        .scene
        .active_marks()
        .last()
        .and_then(|m| m.as_stroke())
        .unwrap()
        .brush
        .clone();
    assert!(
        drawn.starts_with("ink-"),
        "drawn with the edited brush as a variant: {drawn}"
    );
    h.key_press(Key::F5);
    h.run_steps(2);
    h.state_mut().brush_window.name = "Wet Dry".into();
    h.get_by_role_and_label(Role::Button, "save in this drawing").click();
    h.run_steps(3);
    let saved = Bundle::open(&path)
        .unwrap()
        .brush("wet-dry")
        .unwrap()
        .expect("saved in the bundle");
    assert!(saved.dual.is_some() && saved.paint.is_some());
    assert_eq!(h.state().brush, "wet-dry");
    assert!(h.state().custom.is_none());
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 2);
        encoder.set_color(png::ColorType::Grayscale);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0, 255, 255, 0]).unwrap();
    }
    h.state_mut().add_brush_picture("checks", &png).unwrap();
    assert!(path.join("brushes/images/checks.png").exists());
    assert!(h.state().brushes.pictures().contains_key("checks"));
    assert!(h.state_mut().add_brush_picture("checks", &[1, 2, 3]).is_err());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_bends_a_brush_stroke_and_the_stroke_keeps_its_pressure() {
    let path = sample("reshape");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "notes");
    h.key_press(Key::A);
    h.run_steps(2);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let click = |h: &mut Harness<'static, Editor>, p: egui::Pos2| {
        h.hover_at(p);
        h.run_steps(1);
        h.drag_at(p);
        h.run_steps(1);
        h.drop_at(p);
        h.run_steps(2);
    };
    click(&mut h, at(170.0, 120.0));
    assert_eq!(h.state().scene.active_layer().unwrap().name, "lines");
    let node = h.state().direct.as_ref().map(|d| d.node.clone()).unwrap();
    assert_eq!(node, "c6:s1");
    let before = h.state().scene.bundle.read_marks("l6", "c6").unwrap()[0]
        .as_stroke()
        .unwrap()
        .clone();
    let first = h.state().shaped.as_ref().unwrap().subs[0].anchors[0].at;
    let start = at(first[0], first[1]);
    drag(&mut h, start, start + vec2(0.0, 60.0));
    settle(&mut h, |ed| ed.point_drag.is_none());
    h.run_steps(2);
    let after = h.state().scene.bundle.read_marks("l6", "c6").unwrap()[0]
        .as_stroke()
        .unwrap()
        .clone();
    assert_eq!(after.samples.len(), before.samples.len());
    let moved = after.samples[0].pos().1 - before.samples[0].pos().1;
    assert!((moved - 60.0 / view.zoom).abs() < 3.0, "the start moved {moved}");
    let end = (
        after.samples.last().unwrap().pos(),
        before.samples.last().unwrap().pos(),
    );
    assert!(
        (end.0.0 - end.1.0).abs() < 1.5 && (end.0.1 - end.1.1).abs() < 1.5,
        "{end:?}"
    );
    assert!(
        after
            .samples
            .iter()
            .zip(&before.samples)
            .all(|(a, b)| a.pressure == b.pressure)
    );
    assert_eq!(h.state().direct.as_ref().map(|d| d.node.clone()), Some(node));
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    let undone = h.state().scene.bundle.read_marks("l6", "c6").unwrap()[0]
        .as_stroke()
        .unwrap()
        .clone();
    assert_eq!(undone.samples, before.samples);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_inspector_adds_effects_and_adjustments_and_previews_them_without_history() {
    use klad_core::doc::Blend;
    use klad_core::history::{Stack, entries};
    use klad_core::look::{Adjustment, Effect};
    let path = sample("looks");
    let mut h = harness(editor(&path), vec2(1440.0, 1400.0));
    [
        (Pane::Inspector, false),
        (Pane::Notes, true),
        (Pane::History, true),
        (Pane::Layers, true),
        (Pane::Tool, true),
    ]
    .into_iter()
    .for_each(|(pane, collapsed)| {
        if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
            p.collapsed = collapsed;
        }
    });
    activate(&mut h, "lines");
    h.state_mut().invoke(Cmd::NewPaintLayer).unwrap();
    h.run_steps(3);
    let active = |h: &Harness<'static, Editor>| h.state().scene.active_layer().unwrap().clone();
    let light = active(&h).id;
    h.state_mut()
        .apply(
            parse_lines(&format!(
                r#"{{"op":"set","id":"{light}","blend":"add","effects":[{{"glow":{{"radius":24}}}}]}}"#
            ))
            .unwrap(),
        )
        .unwrap();
    h.run_steps(3);
    assert_eq!(active(&h).blend, Blend::Add);
    assert!(matches!(active(&h).effects[..], [Effect::Glow(_)]));
    h.get_by_label_contains("applied after the ones above").click();
    h.run_steps(2);
    h.get_by_label("outline").click();
    h.run_steps(3);
    assert!(matches!(active(&h).effects[..], [Effect::Glow(_), Effect::Outline(_)]));
    let steps = entries(&h.state().scene.bundle, Stack::Undo).unwrap().len();
    let rev = h.state().scene.bundle.doc.rev;
    let mut wide = active(&h);
    if let Effect::Glow(g) = &mut wide.effects[0] {
        g.radius = 60.0;
    }
    h.state_mut().preview_look(wide).unwrap();
    h.run_steps(2);
    assert!(matches!(&active(&h).effects[0], Effect::Glow(g) if g.radius == 60.0));
    assert_eq!(h.state().scene.bundle.doc.rev, rev, "a preview is not an edit");
    h.state_mut().end_look_preview().unwrap();
    assert!(matches!(&active(&h).effects[0], Effect::Glow(g) if g.radius == 24.0));
    assert_eq!(entries(&h.state().scene.bundle, Stack::Undo).unwrap().len(), steps);
    checked_audit(&mut h);
    h.state_mut().invoke(Cmd::NewAdjustLayer).unwrap();
    h.run_steps(3);
    assert!(matches!(active(&h).adjustment(), Some(Adjustment::Curves(_))));
    h.get_by_label_contains("what this adjustment layer does").click();
    h.run_steps(2);
    h.get_by_label("levels").click();
    h.run_steps(3);
    assert!(matches!(active(&h).adjustment(), Some(Adjustment::Levels(_))));
    checked_audit(&mut h);
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    assert!(matches!(active(&h).adjustment(), Some(Adjustment::Curves(_))));
    h.state_mut()
        .apply(parse_lines(r##"{"op":"add","layer":{"id":"shafts","kind":"vector","name":"shafts","blend":"screen","cels":[{"frame":0,"nodes":[{"id":"rays1","type":"rays","x":200,"y":0,"fill":"#fff0c0"}]}]}}"##).unwrap())
        .unwrap();
    activate(&mut h, "shafts");
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some("rays1".into());
    h.run_steps(3);
    assert!(h.query_all(By::new().label_contains("rays")).count() > 0);
    checked_audit(&mut h);
    let _ = std::fs::remove_dir_all(&path);
}

fn checked_audit(h: &mut Harness<'static, Editor>) {
    h.run_steps(2);
    assert!(audit(h, "effects and adjustments in the inspector") > 20);
}

#[test]
#[ignore = "timing: KLAD_HEAVY_SIZE=4096 cargo test --release -p klad-app heavy_edits -- --ignored --nocapture"]
fn heavy_edits_and_revisits_keep_the_ui_free() {
    let stress = std::env::temp_dir().join(format!("draw-ui-heavy-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stress);
    let exe = std::env::current_exe().unwrap();
    let draw = exe.parent().and_then(Path::parent).unwrap().join("klad");
    let size = std::env::var("KLAD_HEAVY_SIZE").unwrap_or_else(|_| "2048".into());
    let made = std::process::Command::new(&draw)
        .args(["stress", stress.to_str().unwrap(), "--frames", "12", "--size", &size])
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let mut h = harness(editor(&stress), vec2(1600.0, 1000.0));
    let wait = |h: &mut Harness<'static, Editor>| {
        let start = std::time::Instant::now();
        while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(120) {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        start.elapsed().as_millis()
    };
    wait(&mut h);
    [2u32, 4].into_iter().for_each(|f| {
        h.state_mut().go_to(f).unwrap();
        wait(&mut h);
    });
    h.state_mut().go_to(2).unwrap();
    let revisit = wait(&mut h);
    settle(&mut h, |ed| ed.buffering.job_idle());
    let layer = h.state().scene.bundle.doc.layers[3].id.clone();
    let edit = |h: &mut Harness<'static, Editor>, opacity: f32| {
        h.state_mut()
            .apply(parse_lines(&format!(r#"{{"op":"set","id":"{layer}","opacity":{opacity}}}"#)).unwrap())
            .unwrap();
        (0..40)
            .map(|_| {
                let t = std::time::Instant::now();
                h.run_steps(1);
                let spent = t.elapsed().as_secs_f64() * 1000.0;
                std::thread::sleep(std::time::Duration::from_millis(8));
                spent
            })
            .fold(0.0f64, f64::max)
    };
    let slowest: Vec<f64> = [0.9f32, 0.8, 1.0].iter().map(|o| edit(&mut h, *o)).collect();
    eprintln!(
        "{size} px: a frame seen before is drawable again in {revisit} ms; slowest UI frame in the 40 after each of three edits {slowest:.1?} ms"
    );
    let _ = std::fs::remove_dir_all(&stress);
}

#[test]
fn a_closed_panel_comes_back_from_the_strip() {
    let path = sample("panels");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    h.state_mut().invoke(Cmd::InspectorPane).unwrap();
    h.run_steps(2);
    assert!(!h.state().settings.layout.is_open(Pane::Inspector));
    h.get_by_role_and_label(Role::Button, theme::icon::SIDEBAR).click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, Pane::Inspector.title()).click();
    h.run_steps(3);
    assert!(h.state().settings.layout.is_open(Pane::Inspector));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn strokes_select_move_and_copy_across_cmd_clicked_layers() {
    let path = sample("across");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut()
        .apply(
            parse_lines(r##"{"op":"stroke","layer":"l5","path":"M50 60 L280 210","size":20,"color":"#d08040"}"##)
                .unwrap(),
        )
        .unwrap();
    activate(&mut h, "lines");
    let row = |h: &Harness<'static, Editor>, name: &str| {
        h.query_all(By::new().role(Role::Button).label(name))
            .map(|n| n.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap()
    };
    let colour = row(&h, "colour");
    let at = egui::pos2(colour.right() - 70.0, colour.center().y);
    h.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    h.hover_at(at);
    h.run_steps(1);
    [true, false].into_iter().for_each(|pressed| {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::COMMAND,
        });
        h.run_steps(1);
    });
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    assert_eq!(
        h.state().scene.active_layer().unwrap().name,
        "lines",
        "cmd click keeps the active layer"
    );
    assert_eq!(h.state().also, vec!["l5".to_string()]);
    h.key_press(Key::S);
    h.run_steps(2);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let corners = [(20.0, 20.0), (440.0, 20.0), (440.0, 340.0), (20.0, 340.0), (20.0, 24.0)];
    h.hover_at(at(corners[0].0, corners[0].1));
    h.run_steps(1);
    h.drag_at(at(corners[0].0, corners[0].1));
    h.run_steps(1);
    corners.iter().skip(1).for_each(|&(x, y)| {
        (1..=4).for_each(|_| {
            h.hover_at(at(x, y));
            h.run_steps(1);
        });
    });
    h.drop_at(at(20.0, 24.0));
    h.run_steps(2);
    let picked = h.state().picked.clone().expect("a selection on both layers");
    assert_eq!(
        (picked.ids.len(), picked.more.len(), picked.more[0].ids.len()),
        (2, 1, 1)
    );
    let starts = |h: &Harness<'static, Editor>| -> Vec<(f32, f32)> {
        ["l6", "l5"]
            .iter()
            .map(|layer| {
                let doc = &h.state().scene.bundle.doc;
                let cel = doc.layer(layer).unwrap().content_at(0).unwrap().id.clone();
                h.state().scene.bundle.read_marks(layer, &cel).unwrap()[0]
                    .as_stroke()
                    .unwrap()
                    .samples[0]
                    .pos()
            })
            .collect()
    };
    let before = starts(&h);
    let corners = h.state().picked_corners().unwrap().1;
    let middle = rect.min + view.to_screen((corners[0] + corners[2]) / 2.0, rect.size());
    drag(&mut h, middle, middle + vec2(40.0 * view.zoom, 0.0));
    settle(&mut h, |ed| ed.drag.is_none());
    h.run_steps(3);
    let after = starts(&h);
    before.iter().zip(&after).for_each(|(b, a)| {
        assert!(
            (a.0 - b.0 - 40.0).abs() < 1.5 && (a.1 - b.1).abs() < 1.5,
            "{b:?} -> {a:?}"
        );
    });
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    assert_eq!(starts(&h), before, "one undo puts both layers back");
    h.state_mut().invoke(Cmd::Redo).unwrap();
    h.run_steps(2);
    let counts = |h: &Harness<'static, Editor>| -> Vec<usize> {
        ["l6", "l5"]
            .iter()
            .map(|layer| {
                let doc = &h.state().scene.bundle.doc;
                let cel = doc.layer(layer).unwrap().content_at(0).unwrap().id.clone();
                h.state().scene.bundle.read_marks(layer, &cel).unwrap().len()
            })
            .collect()
    };
    let had = counts(&h);
    h.state_mut().select_all_strokes();
    h.state_mut().invoke(Cmd::DuplicateInPlace).unwrap();
    h.run_steps(3);
    assert_eq!(
        counts(&h),
        vec![had[0] * 2, had[1] * 2],
        "each layer gets its own copies"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_lasso_cut_spans_the_selected_layers() {
    use klad_core::markedit::{LassoAction, MarkTransform};
    let path = sample("across-cut");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut()
        .apply(
            parse_lines(r##"{"op":"stroke","layer":"l5","path":"M20 200 L480 200","size":20,"color":"#d08040"}"##)
                .unwrap(),
        )
        .unwrap();
    activate(&mut h, "lines");
    let cel = |h: &Harness<'static, Editor>, layer: &str| {
        h.state()
            .scene
            .bundle
            .doc
            .layer(layer)
            .unwrap()
            .content_at(0)
            .unwrap()
            .id
            .clone()
    };
    let (lines, colour) = (cel(&h, "l6"), cel(&h, "l5"));
    let count =
        |h: &Harness<'static, Editor>, layer: &str, c: &str| h.state().scene.bundle.read_marks(layer, c).unwrap().len();
    let had = (count(&h, "l6", &lines), count(&h, "l5", &colour));
    let polygon = vec![[0.0, 0.0], [200.0, 0.0], [200.0, 384.0], [0.0, 384.0]];
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().picked = Some(super::Picked {
        cel: lines.clone(),
        ids: Vec::new(),
        cut: Some(polygon),
        more: vec![super::Part {
            layer: "l5".into(),
            cel: colour.clone(),
            ids: Vec::new(),
        }],
    });
    h.state_mut().refresh_cut();
    h.state_mut()
        .edit_picked(LassoAction::Transform(MarkTransform {
            y: 30.0,
            ..Default::default()
        }))
        .unwrap();
    h.run_steps(2);
    assert!(
        count(&h, "l6", &lines) > had.0 && count(&h, "l5", &colour) > had.1,
        "both layers were cut"
    );
    let picked = h.state().picked.clone().unwrap();
    assert!(
        !picked.ids.is_empty() && !picked.more[0].ids.is_empty(),
        "the cut parts stay selected"
    );
    let _ = std::fs::remove_dir_all(&path);
}

fn canvas_at(h: &Harness<'static, Editor>, x: f32, y: f32) -> egui::Pos2 {
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size())
}

#[test]
fn rectangles_and_ellipses_drag_onto_a_vector_layer() {
    use klad_core::doc::Shape;
    let path = sample("shape-tool");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    h.key_press(Key::U);
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Shape);
    let nodes = |h: &Harness<'static, Editor>| {
        h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0]
            .nodes
            .clone()
    };
    let had = nodes(&h).len();
    {
        let ed = h.state_mut();
        ed.pen_fill = true;
        ed.pen_stroke = true;
        ed.color = [0.8, 0.2, 0.1, 1.0];
        ed.second = [0.1, 0.2, 0.8, 1.0];
    }
    h.run_steps(3);
    let (a, b) = (canvas_at(&h, 200.0, 100.0), canvas_at(&h, 280.0, 140.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    let made = nodes(&h);
    assert_eq!(made.len(), had + 1);
    assert_eq!(
        made[had].fill,
        Some(klad_core::doc::Paint::Solid(crate::color::hex([0.8, 0.2, 0.1, 1.0]))),
        "the fill takes the first colour"
    );
    assert_eq!(
        made[had].stroke.as_ref().map(|s| s.paint.clone()),
        Some(klad_core::doc::Paint::Solid(crate::color::hex([0.1, 0.2, 0.8, 1.0]))),
        "the outline takes the second colour"
    );
    match &made[had].shape {
        Shape::Rect { x, y, w, h, .. } => {
            assert!((x - 200.0).abs() < 1.5 && (y - 100.0).abs() < 1.5, "{x},{y}");
            assert!((w - 80.0).abs() < 1.5 && (h - 40.0).abs() < 1.5, "{w}x{h}");
        }
        other => panic!("{other:?}"),
    }
    h.state_mut().shapes.kind = super::make::ShapeKind::Ellipse;
    h.run_steps(3);
    h.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    let (a, b) = (canvas_at(&h, 300.0, 200.0), canvas_at(&h, 340.0, 220.0));
    drag(&mut h, a, b);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    match &nodes(&h)[had + 1].shape {
        Shape::Ellipse { cx, cy, rx, ry } => {
            assert!((rx - ry).abs() < 0.01 && (rx - 20.0).abs() < 1.5, "a circle: {rx} {ry}");
            assert!((cx - 320.0).abs() < 1.5 && (cy - 220.0).abs() < 1.5, "{cx},{cy}");
        }
        other => panic!("{other:?}"),
    }
    checked_audit(&mut h);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_selected_shape_turns_into_a_repeat_and_back() {
    use klad_core::doc::Shape;
    let path = sample("wrap");
    let mut h = harness(editor(&path), vec2(1440.0, 1200.0));
    if let Some(p) = h.state_mut().settings.layout.pane_mut(Pane::Inspector) {
        p.collapsed = false;
    }
    activate(&mut h, "shapes");
    let rect = h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0].nodes[0]
        .id
        .clone();
    h.state_mut().tool = Tool::Select;
    h.state_mut().selection = Some(rect.clone());
    h.run_steps(2);
    h.get_by_label_contains("repeat it").click();
    h.run_steps(3);
    let made = h.state().selection.clone().unwrap();
    let doc = &h.state().scene.bundle.doc;
    let Some(klad_core::doc::Item::Node(_, _, node)) = doc.item(&made) else {
        panic!()
    };
    assert!(matches!(&node.shape, Shape::Repeat(r) if r.count == 4 && r.children[0].id == rect));
    assert!(
        h.query_all(By::new().label_contains("copies")).count() > 0,
        "the inspector shows the repeat"
    );
    checked_audit(&mut h);
    h.get_by_label_contains("take the shapes out").click();
    h.run_steps(3);
    let doc = &h.state().scene.bundle.doc;
    assert!(doc.item(&made).is_none(), "the repeat is gone");
    let layer = doc.find_layer("shapes").unwrap();
    assert_eq!(layer.cels()[0].nodes[0].id, rect, "the rect is back where it was");
    assert_eq!(h.state().selection.as_deref(), Some(rect.as_str()));
    h.state_mut().invoke(Cmd::MakeMirror).unwrap();
    h.run_steps(2);
    let mirror = h.state().selection.clone().unwrap();
    h.key_press(Key::Delete);
    h.run_steps(2);
    let doc = &h.state().scene.bundle.doc;
    assert!(
        doc.item(&mirror).is_none() && doc.item(&rect).is_none(),
        "Delete removes a generator with its shapes"
    );
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    let doc = &h.state().scene.bundle.doc;
    assert!(
        matches!(doc.item(&rect), Some(klad_core::doc::Item::Node(_, _, n)) if matches!(n.shape, Shape::Rect { .. }))
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn mirror_painting_makes_two_plain_strokes_in_one_step() {
    let path = sample("mirror-paint");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    h.state_mut().invoke(Cmd::Symmetry).unwrap();
    assert_eq!(h.state().mirror, Some(256.0));
    h.state_mut().tool = Tool::Brush;
    let marks = |h: &Harness<'static, Editor>| h.state().scene.active_marks().to_vec();
    let had = marks(&h).len();
    let (a, b) = (canvas_at(&h, 60.0, 80.0), canvas_at(&h, 160.0, 200.0));
    drag(&mut h, a, b);
    settle(&mut h, |ed| ed.live.is_none());
    h.run_steps(2);
    let after = marks(&h);
    assert_eq!(after.len(), had + 2, "the stroke and its mirror image");
    let (one, two) = (after[had].as_stroke().unwrap(), after[had + 1].as_stroke().unwrap());
    let (p, q) = (one.samples[0].pos(), two.samples[0].pos());
    assert!(
        (p.0 + q.0 - 512.0).abs() < 0.2 && (p.1 - q.1).abs() < 0.01,
        "{p:?} {q:?}"
    );
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(2);
    assert_eq!(marks(&h).len(), had, "one undo takes both");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn holding_d_builds_a_plane_per_layer_and_the_inspector_sets_depth_and_fog() {
    let path = sample("parallax");
    let mut h = harness(editor(&path), vec2(1440.0, 1200.0));
    if let Some(p) = h.state_mut().settings.layout.pane_mut(Pane::Inspector) {
        p.collapsed = false;
    }
    activate(&mut h, "sky");
    h.state_mut()
        .apply(
            parse_lines(
                r##"{"op":"set","id":"l0","depth":3}
{"op":"atmosphere","color":"#a0b0c0"}"##,
            )
            .unwrap(),
        )
        .unwrap();
    h.run_steps(3);
    assert!(
        h.query_all(By::new().label_contains("density")).count() > 0,
        "the atmosphere shows for a plane"
    );
    checked_audit(&mut h);
    let shown = h.state().scene.bundle.doc.layers.iter().filter(|l| l.visible).count();
    h.key_down(Key::D);
    settle(&mut h, |ed| !ed.depth.planes.is_empty());
    h.key_up(Key::D);
    assert_eq!(h.state().depth.planes.len(), shown);
    assert!(h.state().depth.planes.iter().any(|(d, _)| *d == 3.0));
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn an_imported_picture_lands_on_its_own_layer_and_can_be_a_reference() {
    let path = sample("picture");
    let mut h = harness(editor(&path), vec2(1440.0, 1200.0));
    if let Some(p) = h.state_mut().settings.layout.pane_mut(Pane::Inspector) {
        p.collapsed = false;
    }
    activate(&mut h, "lines");
    let rgba = klad_render::assets::Rgba {
        width: 4,
        height: 2,
        pixels: vec![200; 32],
    };
    let png = klad_render::assets::encode_png(&rgba).unwrap();
    h.state_mut().import_picture(&png, "/tmp/photo.png").unwrap();
    h.run_steps(3);
    let layer = h.state().scene.active_layer().unwrap().clone();
    assert_eq!(layer.name, "photo");
    assert_eq!(layer.cels()[0].nodes[0].type_name(), "image");
    h.get_by_role_and_label(Role::CheckBox, "reference").click();
    h.run_steps(3);
    assert!(h.state().scene.active_layer().unwrap().reference);
    let _ = std::fs::remove_dir_all(&path);
}

fn hush(h: &mut Harness<'static, Editor>) {
    let ids: Vec<u64> = h.state().messages.log().map(|m| m.id).collect();
    ids.into_iter().for_each(|id| h.state_mut().messages.dismiss(id));
    h.run_steps(2);
}

#[test]
fn the_cli_only_features_run_from_the_editor_and_give_what_the_cli_gives() {
    use super::checks::{arcs_from, gaps_from};
    use crate::jobs::Tag;
    let klad = crate::start::draw_binary().expect("build the klad binary first (just test does)");
    let run = |args: &[&str]| {
        let out = std::process::Command::new(&klad).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let base = std::env::temp_dir().join(format!("draw-ui-cli-forms-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let (ours, theirs) = (base.join("ours.draw"), base.join("theirs.draw"));
    let (o, t) = (ours.to_str().unwrap(), theirs.to_str().unwrap());
    run(&[
        "new", o, "--width", "256", "--height", "160", "--layers", "0", "--frames", "4",
    ]);
    let ops = base.join("ops.jsonl");
    std::fs::write(
        &ops,
        r##"{"op":"add","layer":{"id":"colour","kind":"paint","name":"colour"}}
{"op":"add","layer":{"id":"lines","kind":"paint","name":"lines"}}
{"op":"stroke","layer":"lines","brush":"ink","size":4,"path":"M20 20 L110 20 L110 120 L20 120 Z","taper":[0,0]}
{"op":"stroke","layer":"lines","brush":"ink","size":4,"path":"M140 20 L230 20 L230 120 L140 120 Z","taper":[0,0]}
{"op":"fill","layer":"colour","at":[60,70],"ref":"lines","color":"#c04040"}"##,
    )
    .unwrap();
    run(&["apply", o, "--file", ops.to_str().unwrap()]);
    copy_dir(&ours, &theirs);
    let mut h = harness(editor(&ours), vec2(1280.0, 1400.0));
    let idle = |tag: Tag| move |ed: &Editor| !ed.jobs.busy(tag);

    activate(&mut h, "colour");
    h.state_mut().invoke(Cmd::ShowGaps).unwrap();
    settle(&mut h, |ed| ed.checks.gaps.is_some());
    let cli = gaps_from(&run(&["gaps", t, "colour", "--json", "--frame", "0"]), "colour").unwrap();
    assert_eq!(h.state().checks.gaps.as_ref().unwrap().frames, cli.frames);
    assert_eq!(cli.frames[0].1.len(), 1, "the right box has no fill");
    h.state_mut().invoke(Cmd::ShowGaps).unwrap();
    assert!(h.state().checks.gaps.is_none());

    activate(&mut h, "lines");
    h.state_mut().invoke(Cmd::ShowGaps).unwrap();
    settle(&mut h, |ed| ed.checks.gaps.is_some());
    assert_eq!(
        h.state().checks.gaps.as_ref().unwrap().frames,
        cli.frames,
        "gaps on the lines check the layer that fills from them"
    );
    h.state_mut().invoke(Cmd::ShowGaps).unwrap();
    let (cel, id) = {
        let ed = h.state();
        (ed.scene.active_cel().unwrap(), ed.scene.active_marks()[0].id())
    };
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().picked = Some(Picked::whole(cel.clone(), vec![id]));
    h.state_mut().checks.part = "box".into();
    h.run_steps(2);
    hush(&mut h);
    h.get_by_role_and_label(Role::Button, "make them this part").click();
    h.run_steps(2);
    run(&["part", t, &format!("{cel}:s{id}"), "--name", "box"]);
    let parts = |path: &Path| {
        let b = Bundle::open(path).unwrap();
        b.doc.find_layer("lines").unwrap().cels()[0].parts.clone()
    };
    assert_eq!(parts(&ours), parts(&theirs));
    assert_eq!(h.state().picked_part_names(), ["box"]);

    h.state_mut().invoke(Cmd::ShowArcs).unwrap();
    settle(&mut h, |ed| ed.checks.arcs.is_some());
    let cli = arcs_from(
        &run(&["arcs", t, "--track", "lines:box", "--from", "0", "--to", "3", "--json"]),
        "x",
    )
    .unwrap();
    assert_eq!(h.state().checks.arcs.as_ref().unwrap().lines, cli.lines);
    assert_eq!(cli.lines[0].len(), 4);
    h.state_mut().invoke(Cmd::ShowParts).unwrap();
    h.run_steps(2);

    activate(&mut h, "colour");
    h.state_mut().invoke(Cmd::Refill).unwrap();
    settle(&mut h, idle(Tag::Edit));
    run(&["refill", t, "colour", "--frame", "0"]);

    let font = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/Phosphor.ttf");
    h.state_mut().invoke(Cmd::AddFont).unwrap();
    h.state_mut().checks.font = font.into();
    h.run_steps(2);
    hush(&mut h);
    h.get_by_role_and_label(Role::Button, "add the font").click();
    h.run_steps(2);
    settle(&mut h, idle(Tag::Edit));
    run(&["font", t, font]);
    assert_eq!(h.state().fonts(), ["Phosphor.ttf"]);

    h.state_mut().invoke(Cmd::Export).unwrap();
    h.state_mut().export.out = base.join("ours.png").to_string_lossy().to_string();
    h.run_steps(2);
    hush(&mut h);
    h.get_by_role_and_label(Role::Button, "export").click();
    h.run_steps(2);
    settle(&mut h, idle(Tag::Export));
    run(&[
        "export",
        t,
        "--frame",
        "0",
        "-o",
        base.join("theirs.png").to_str().unwrap(),
    ]);
    assert_eq!(
        std::fs::read(base.join("ours.png")).unwrap(),
        std::fs::read(base.join("theirs.png")).unwrap()
    );

    let hash = |p: &str| run(&["hash", p, "--no-cache"]);
    assert_eq!(hash(o), hash(t));
    let errors: Vec<String> = h
        .state()
        .messages
        .log()
        .filter(|m| m.tone == Tone::Error)
        .map(|m| m.text.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn paint_with_body_from_the_brush_window_lit_by_a_relief_turned_with_its_dial() {
    use klad_core::look::Effect;
    let path = sample("body");
    let mut h = harness(editor(&path), vec2(1440.0, 1400.0));
    [
        (Pane::Inspector, false),
        (Pane::Notes, true),
        (Pane::History, true),
        (Pane::Layers, true),
        (Pane::Tool, true),
    ]
    .into_iter()
    .for_each(|(pane, collapsed)| {
        if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
            p.collapsed = collapsed;
        }
    });
    activate(&mut h, "lines");
    h.state_mut().pick("oil");
    h.key_press(Key::F5);
    h.run_steps(3);
    assert!(h.state().windows.brush);
    let track = h.get_by_role_and_label(Role::Slider, "body").rect();
    let at = egui::pos2(track.left() + track.width() * 0.9, track.center().y);
    drag(&mut h, at, at);
    h.run_steps(2);
    let body = h.state().custom.as_ref().map_or(0.0, |d| d.body);
    assert!(body > 0.7, "the slider sets the body: {body}");
    h.key_press(Key::F5);
    h.run_steps(2);
    let rect = h.state().canvas_rect;
    h.key_press(Key::B);
    h.run_steps(1);
    drag(
        &mut h,
        rect.center() + vec2(-120.0, 0.0),
        rect.center() + vec2(120.0, 10.0),
    );
    h.run_steps(3);
    assert!(
        h.state().scene.active_raster().is_some_and(|r| r.has_height()),
        "the stroke left paint height"
    );
    h.get_by_label_contains("applied after the ones above").click();
    h.run_steps(2);
    h.get_by_label("relief").click();
    h.run_steps(3);
    let relief = |h: &Harness<'static, Editor>| match h.state().scene.active_layer().unwrap().effects.first() {
        Some(Effect::Relief(r)) => r.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(relief(&h).light, [-1.0, -1.0]);
    let dial = h.get_by_label(super::look::DIAL).rect();
    let right = egui::pos2(dial.right() - 4.0, dial.center().y);
    drag(&mut h, right, right);
    h.run_steps(3);
    let light = relief(&h).light;
    assert!(
        light[0] > 0.9 && light[1].abs() < 0.2,
        "the light comes from the right: {light:?}"
    );
    checked_audit(&mut h);
    let _ = std::fs::remove_dir_all(&path);
}

fn click(h: &mut Harness<'static, Editor>, at: egui::Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    h.drag_at(at);
    h.run_steps(1);
    h.drop_at(at);
    h.run_steps(1);
}

fn texts(h: &Harness<'static, Editor>) -> Vec<(String, klad_core::generators::Text)> {
    h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0]
        .nodes
        .iter()
        .filter_map(|n| match &n.shape {
            klad_core::doc::Shape::Text(t) => Some((n.id.clone(), t.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn text_is_typed_on_the_canvas_and_edited_in_place() {
    let path = sample("text-tool");
    let mut h = harness_stepping(editor(&path), vec2(1440.0, 1000.0), 1.0 / 60.0);
    activate(&mut h, "shapes");
    h.key_press(Key::T);
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Text);
    h.state_mut().color = [0.1, 0.2, 0.3, 1.0];
    let at = canvas_at(&h, 200.0, 150.0);
    click(&mut h, at);
    h.run_steps(2);
    assert!(h.state().text_edit.is_some(), "a click starts a text");
    h.event(egui::Event::Text("Hello".into()));
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.event(egui::Event::Text("world".into()));
    h.run_steps(1);
    h.key_press(Key::B);
    h.run_steps(1);
    assert_eq!(h.state().tool, Tool::Text, "letters type, they do not switch tools");
    assert!(
        h.state()
            .text_edit
            .as_ref()
            .unwrap()
            .shown
            .as_ref()
            .is_some_and(|s| s.1.is_some()),
        "the typed words show on the canvas while typing"
    );
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().text_edit.is_none());
    let made = texts(&h);
    assert_eq!(made.len(), 1);
    let (id, text) = made[0].clone();
    assert_eq!(text.text, "Hello\nworld");
    assert_eq!(text.font, "Inter.ttf");
    assert!((text.x - 200.0).abs() < 1.5, "{}", text.x);
    let shipped = std::fs::read(klad_render::fonts::find("Inter").unwrap().family.dir.join("Inter.ttf")).unwrap();
    assert_eq!(
        std::fs::read(path.join("fonts/Inter.ttf")).unwrap(),
        shipped,
        "the shipped font is copied in"
    );
    assert!(path.join("fonts/Inter.licence.txt").is_file());
    let node = h.state().scene.bundle.doc.item(&id).map(|i| match i {
        klad_core::doc::Item::Node(_, _, n) => n.fill.clone(),
        _ => None,
    });
    assert_eq!(
        node.flatten(),
        Some(klad_core::doc::Paint::Solid(crate::color::hex([0.1, 0.2, 0.3, 1.0])))
    );
    h.key_press(Key::V);
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Select, "keys work again once typing ends");
    let on = canvas_at(&h, text.x + 20.0, text.y + text.size * 1.6);
    h.hover_at(on);
    h.run_steps(1);
    [true, false, true, false].iter().for_each(|down| match down {
        true => h.drag_at(on),
        false => h.drop_at(on),
    });
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Text, "a double click on text edits it");
    assert_eq!(
        h.state().text_edit.as_ref().and_then(|t| t.id.clone()),
        Some(id.clone())
    );
    h.key_press(Key::End);
    h.event(egui::Event::Text("!".into()));
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(2);
    let after = texts(&h);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].1.text, "Hello\nworld!");
    let shown = h.state().scene.bundle.doc.item(&id).map(|i| match i {
        klad_core::doc::Item::Node(_, _, n) => n.visible,
        _ => false,
    });
    assert_eq!(shown, Some(true), "the node hidden while typing is back");
    let at = canvas_at(&h, 400.0, 300.0);
    click(&mut h, at);
    h.run_steps(1);
    h.key_press(Key::T);
    h.run_steps(1);
    let at = canvas_at(&h, 60.0, 330.0);
    click(&mut h, at);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(texts(&h).len(), 1, "nothing typed adds nothing");
    h.key_press(Key::V);
    h.run_steps(2);
    [
        (Pane::Inspector, false),
        (Pane::Notes, true),
        (Pane::History, true),
        (Pane::Layers, true),
        (Pane::Tool, true),
    ]
    .into_iter()
    .for_each(|(pane, collapsed)| {
        if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
            p.collapsed = collapsed;
        }
    });
    h.state_mut().selection = Some(id.clone());
    h.run_steps(3);
    h.get_by_label_contains("picking one copies it into the drawing")
        .click();
    h.run_steps(3);
    assert!(h.query_by_label("Bangers").is_some(), "the picker lists klad's fonts");
    let search = h
        .query_all_by_label(crate::theme::icon::SEARCH)
        .find(|n| n.accesskit_node().role() == Role::TextInput)
        .unwrap();
    search.click();
    h.run_steps(1);
    h.event(egui::Event::Text("cinz".into()));
    h.run_steps(3);
    assert!(h.query_by_label("Bangers").is_none(), "the search narrows the list");
    h.get_by_label("Cinzel").click();
    h.run_steps(3);
    let picked = texts(&h)[0].1.clone();
    assert_eq!(
        (picked.font.as_str(), picked.text.as_str()),
        ("Cinzel.ttf", "Hello\nworld!")
    );
    assert!(path.join("fonts/Cinzel.ttf").is_file(), "picking a font copies it in");
}

#[test]
fn bones_are_drawn_weighted_and_posed_on_the_canvas() {
    let path = sample("bones");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::J);
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Bone);
    let (a, b, c) = (
        canvas_at(&h, 40.0, 40.0),
        canvas_at(&h, 170.0, 120.0),
        canvas_at(&h, 300.0, 200.0),
    );
    drag(&mut h, a, b);
    h.run_steps(2);
    drag(&mut h, b, c);
    h.run_steps(2);
    let bones = |h: &Harness<'static, Editor>| h.state().scene.bundle.doc.layer("l6").unwrap().bones.clone();
    let made = bones(&h);
    assert_eq!(made.len(), 2, "{made:?}");
    assert_eq!(
        made[1].parent.as_deref(),
        Some("bone 1"),
        "a bone started on a tail is its child"
    );
    assert!((made[1].tail[0] - 300.0).abs() < 2.0 && (made[1].tail[1] - 200.0).abs() < 2.0);
    h.state_mut().bones.name = "shin".into();
    h.state_mut()
        .bone_edit(|s| {
            s.rename = Some("bone 2".into());
            s.to_name = Some("shin".into());
        })
        .unwrap();
    assert_eq!(bones(&h)[1].name, "shin");
    h.state_mut().bones.mode = super::bones::Mode::Weights;
    h.state_mut().bones.selected = Some("shin".into());
    let (dab, more) = (canvas_at(&h, 250.0, 170.0), canvas_at(&h, 260.0, 175.0));
    drag(&mut h, dab, more);
    h.run_steps(2);
    let weights = h.state().scene.bundle.read_weights("l6").unwrap();
    assert!(
        weights.maps.contains_key("shin"),
        "painting weights writes the bone's map"
    );
    h.state_mut().bones.mode = super::bones::Mode::Pose;
    let (end, there) = (canvas_at(&h, 290.0, 190.0), canvas_at(&h, 200.0, 260.0));
    drag(&mut h, end, there);
    h.run_steps(2);
    let turned = h.state().bones.pose.get("shin").copied().unwrap_or(0.0);
    assert!(turned > 20.0, "dragging the shin's end turns it: {turned}");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_layer_is_keyed_from_the_inspector_and_eases_between_its_keys() {
    let path = sample("keyed-layer");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    let id = h.state().scene.active_id().unwrap();
    h.state_mut()
        .key_here(&id, std::collections::BTreeMap::from([("y".to_string(), 0.0)]), false)
        .unwrap();
    h.state_mut().scene.frame = 6;
    h.state_mut().key_ease = klad_core::keys::Ease::Linear;
    h.state_mut()
        .key_here(&id, std::collections::BTreeMap::from([("y".to_string(), -30.0)]), false)
        .unwrap();
    h.run_steps(2);
    let layer = h.state().scene.bundle.doc.find_layer("lines").unwrap().clone();
    assert_eq!(layer.keyed(3).1.unwrap().y, -15.0);
    assert!(
        h.query_all(By::new().label_contains("keys")).count() > 0,
        "the inspector shows the keys section"
    );
    h.state_mut()
        .key_here(&id, std::collections::BTreeMap::new(), true)
        .unwrap();
    assert_eq!(
        h.state().scene.bundle.doc.find_layer("lines").unwrap().keys["y"].len(),
        1
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_agent_panel_runs_a_scripted_turn_streams_it_and_undoes_it_in_one_go() {
    let path = sample("agent-panel");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    [Pane::Tool, Pane::Layers, Pane::Inspector, Pane::Notes, Pane::History]
        .into_iter()
        .for_each(|p| h.state_mut().settings.layout.toggle(p));
    h.state_mut().invoke(Cmd::AgentPane).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("understood").is_some(), "the notice comes first");
    h.get_by_label("understood").click();
    h.run_steps(2);
    let before = h.state().scene.bundle.read_marks("l6", "c6").unwrap().len();
    h.state_mut().agent.script = Some(vec![
        vec![
            serde_json::json!({"type": "text", "text": "Adding a stroke."}),
            serde_json::json!({"type": "tool_use", "id": "t1", "name": "apply", "input": {"ops": r##"{"op":"stroke","layer":"l6","brush":"ink","size":12,"color":"#802020","path":"M50 300 L 450 320"}"##}}),
        ],
        vec![serde_json::json!({"type": "tool_use", "id": "t2", "name": "look", "input": {}})],
        vec![serde_json::json!({"type": "text", "text": "One red stroke along the bottom."})],
    ]);
    h.state_mut().agent.input = "draw a red line at the bottom".into();
    h.get_by_label("send").click();
    h.run_steps(1);
    settle(&mut h, |ed| !ed.agent_running());
    h.run_steps(3);
    assert!(h.query_by_label("One red stroke along the bottom.").is_some());
    assert!(h.query_all(By::new().label_contains("apply 1 ops")).count() > 0);
    assert!(h.query_all(By::new().label_contains("look")).count() > 0);
    settle(&mut h, |ed| {
        ed.scene.bundle.read_marks("l6", "c6").map_or(0, |m| m.len()) == before + 1
    });
    assert_eq!(h.state().agent.last_edits, 1);
    h.get_by_label("undo this turn").click();
    h.run_steps(2);
    settle(&mut h, |ed| {
        ed.scene.bundle.read_marks("l6", "c6").map_or(0, |m| m.len()) == before
    });
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_problem_report_holds_the_version_the_system_and_the_last_messages() {
    let path = sample("report");
    let mut h = harness(editor(&path), vec2(1280.0, 900.0));
    h.state_mut().say(Tone::Error, "the fill leaked out of the lines");
    let report = h.state().problem_report();
    assert!(report.starts_with(&format!("klad {}", crate::VERSION)));
    assert!(report.contains(std::env::consts::OS) && report.contains("the fill leaked out of the lines"));
    assert!(
        !report.contains(&path.display().to_string()),
        "the drawing's path stays private"
    );
    let _ = std::fs::remove_dir_all(&path);
}
