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

fn skeleton(h: &Harness<'static, Editor>) -> Layer {
    klad_core::rig::skeletons(&h.state().scene.bundle.doc)
        .first()
        .map_or_else(|| Layer::skeleton("", ""), |l| (*l).clone())
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

fn internal(word: &str) -> bool {
    let mut chars = word.chars();
    let first = chars.next();
    let rest: String = chars.collect();
    let digits = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    let story = |head: char| {
        word.strip_prefix(head)
            .and_then(|r| r.split_once('.'))
            .is_some_and(|(a, b)| {
                !a.is_empty() && !b.is_empty() && (a.chars().chain(b.chars())).all(|c| c.is_ascii_digit())
            })
    };
    matches!(first, Some('c' | 'l' | 'n' | 's')) && digits || first == Some('M') && digits || story('S') || story('M')
}

fn developer_words(h: &Harness<'static, Editor>) -> Vec<String> {
    h.query_all(By::new().predicate(|n| n.label().is_some() || n.value().is_some()))
        .flat_map(|n| {
            let node = n.accesskit_node();
            [node.label(), node.value()].into_iter().flatten().collect::<Vec<_>>()
        })
        .filter(|text| {
            text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
                .map(|w| w.trim_end_matches('.'))
                .any(internal)
        })
        .collect()
}

fn scan(h: &Harness<'static, Editor>, what: &str) {
    let found = developer_words(h);
    assert!(
        found.is_empty(),
        "{what}: internal ids or story numbers in sight: {found:#?}"
    );
}

fn audit(h: &Harness<'static, Editor>, what: &str) -> usize {
    scan(h, what);
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
fn the_strip_shrinks_then_folds_its_foot_when_the_window_is_short() {
    let path = sample("strip");
    let in_strip = |h: &Harness<'static, Editor>, glyph: &str| {
        h.query_all_by_label(glyph)
            .map(|n| n.rect())
            .find(|r| r.center().x < layout::STRIP)
    };
    let tall = harness(editor(&path), vec2(1280.0, 1400.0));
    let (brush, redo) = (
        in_strip(&tall, theme::icon::BRUSH).unwrap(),
        in_strip(&tall, theme::icon::REDO).unwrap(),
    );
    let file = in_strip(&tall, theme::icon::FILE).expect("a tall window shows the document menu");
    let settings = in_strip(&tall, theme::icon::SETTINGS).unwrap();
    assert_eq!(brush.height(), 36.0);
    assert!(
        file.top() > redo.bottom() + 40.0,
        "a tall window keeps a gap: {redo:?} {file:?}"
    );
    assert!(
        settings.bottom() > tall.state().canvas_rect.bottom() - 20.0,
        "the foot sits at the bottom"
    );
    assert!(
        (settings.center().x - brush.center().x).abs() < 1.0,
        "one column: {brush:?} {settings:?}"
    );
    drop(tall);
    let smaller = harness(editor(&path), vec2(1280.0, 1100.0));
    let (brush, redo) = (
        in_strip(&smaller, theme::icon::BRUSH).unwrap(),
        in_strip(&smaller, theme::icon::REDO).unwrap(),
    );
    let file = in_strip(&smaller, theme::icon::FILE).expect("first the buttons get smaller");
    assert!(brush.height() < 36.0 && brush.height() >= theme::TARGET, "{brush:?}");
    assert!(file.top() >= redo.bottom(), "nothing overlaps: {redo:?} {file:?}");
    drop(smaller);
    let medium = harness(editor(&path), vec2(1280.0, 1000.0));
    assert!(in_strip(&medium, theme::icon::FILE).is_none(), "then the foot folds");
    let more = in_strip(&medium, theme::icon::MORE).expect("one menu holds the foot");
    let (brush, redo) = (
        in_strip(&medium, theme::icon::BRUSH).unwrap(),
        in_strip(&medium, theme::icon::REDO).unwrap(),
    );
    assert!(
        more.top() >= redo.bottom(),
        "nothing overlaps or hides: {redo:?} {more:?}"
    );
    assert!(
        (more.center().x - brush.center().x).abs() < 1.0,
        "one column: {brush:?} {more:?}"
    );
    drop(medium);
    let mut short = harness(editor(&path), vec2(1280.0, 700.0));
    let more = in_strip(&short, theme::icon::MORE).unwrap();
    let brush = in_strip(&short, theme::icon::BRUSH).unwrap();
    assert!(
        more.bottom() <= short.state().canvas_rect.bottom() + 1.0 && more.top() > brush.bottom(),
        "last the tools scroll, and the menu stays in view: {brush:?} {more:?}"
    );
    short.get_by_role_and_label(Role::Button, theme::icon::MORE).click();
    short.run_steps(2);
    short.get_by_label_contains("Settings…").click();
    short.run_steps(2);
    assert!(
        short.state().windows.settings,
        "the folded menu runs the foot's commands"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn settings_is_a_modal_with_a_sidebar_and_esc_closes_it_unless_a_key_is_being_set() {
    let path = sample("settings-modal");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut().invoke(Cmd::Settings).unwrap();
    h.run_steps(3);
    assert!(
        h.query_by_label_contains("heaviest = my peak").is_some(),
        "it opens on the pen"
    );
    assert!(h.query_by_label("reset every key").is_none(), "one section at a time");
    h.get_by_label_contains("Keys").click();
    h.run_steps(2);
    assert!(h.query_by_label("reset every key").is_some());
    assert!(h.query_by_label_contains("heaviest = my peak").is_none());
    h.get_by_label_contains("Updates").click();
    h.run_steps(2);
    assert!(h.query_by_label(&format!("klad {}", crate::VERSION)).is_some());
    h.state_mut().settings_section = super::prefs::Section::Keys;
    h.state_mut().capturing = Some(Cmd::Onion);
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(
        h.state().windows.settings,
        "Esc while a key is being set keeps the old key and the window"
    );
    assert!(h.state().capturing.is_none());
    h.state_mut().tool = Tool::Fill;
    h.key_press(Key::B);
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Fill, "keys behind the modal stay still");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().windows.settings, "Esc closes it");
    assert_eq!(Pane::Inspector.title(), "Layer settings");
    assert_eq!(Cmd::InspectorPane.menu(), "Layer settings");
    let _ = std::fs::remove_dir_all(&path);
}

fn click_with(h: &mut Harness<'static, Editor>, at: egui::Pos2, modifiers: Modifiers) {
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(at);
    h.run_steps(1);
    [true, false].into_iter().for_each(|pressed| {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
        h.run_steps(1);
    });
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

#[test]
fn a_lasso_picks_strokes_on_every_layer_when_all_layers_is_on() {
    let path = sample("lasso-all");
    Bundle::open(&path)
        .unwrap()
        .apply(&parse_lines(r#"{"op":"stroke","layer":"l5","path":"M200 300 L300 320","size":6}"#).unwrap())
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::S);
    h.run_steps(2);
    let lasso = |h: &mut Harness<'static, Editor>| {
        let corners = [(5.0, 5.0), (505.0, 5.0), (505.0, 379.0), (5.0, 379.0), (5.0, 10.0)];
        let at: Vec<egui::Pos2> = corners.iter().map(|(x, y)| canvas_at(h, *x, *y)).collect();
        h.hover_at(at[0]);
        h.run_steps(1);
        h.drag_at(at[0]);
        h.run_steps(1);
        at[1..].iter().for_each(|p| {
            h.hover_at(*p);
            h.run_steps(1);
        });
        h.drop_at(at[4]);
        h.run_steps(2);
    };
    h.state_mut().settings.pick_all_layers = true;
    lasso(&mut h);
    let picked = h.state().picked.clone().expect("the lasso picks");
    assert_eq!(picked.ids.len(), 2, "both strokes on lines");
    assert_eq!(picked.more.len(), 1, "and the stroke on colour");
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.state_mut().picked = None;
    h.state_mut().settings.pick_all_layers = false;
    lasso(&mut h);
    let picked = h.state().picked.clone().expect("the lasso picks");
    assert!(picked.more.is_empty(), "this layer only: {picked:?}");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_click_picks_the_stroke_you_see_on_any_layer_or_only_on_this_one() {
    let path = sample("pick-seen");
    Bundle::open(&path)
        .unwrap()
        .apply(&parse_lines(r#"{"op":"stroke","layer":"l5","path":"M100 100 L250 180","size":40}"#).unwrap())
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    ready(&mut h);
    let cel_of = |h: &Harness<'static, Editor>, name: &str| {
        h.state().scene.bundle.doc.find_layer(name).unwrap().cels()[0]
            .id
            .clone()
    };
    let (lines, colour) = (cel_of(&h, "lines"), cel_of(&h, "colour"));
    [Tool::Select, Tool::Strokes].into_iter().for_each(|tool| {
        activate(&mut h, "colour");
        h.state_mut().tool = tool;
        h.state_mut().picked = None;
        h.state_mut().settings.pick_all_layers = true;
        h.run_steps(2);
        let seen = canvas_at(&h, 170.0, 120.0);
        click_with(&mut h, seen, Modifiers::NONE);
        settle(&mut h, |ed| ed.scene.active_layer().is_some_and(|l| l.name == "lines"));
        if h.state().picked.is_none() {
            ready(&mut h);
            click_with(&mut h, seen, Modifiers::NONE);
        }
        assert_eq!(
            h.state().picked.as_ref().map(|p| p.cel.clone()),
            Some(lines.clone()),
            "{tool:?}: the line on top is picked, not the colour stroke under it on the active layer"
        );
        activate(&mut h, "colour");
        h.state_mut().picked = None;
        h.state_mut().settings.pick_all_layers = false;
        h.run_steps(2);
        click_with(&mut h, seen, Modifiers::NONE);
        assert_eq!(h.state().scene.active_layer().unwrap().name, "colour");
        assert_eq!(
            h.state().picked.as_ref().map(|p| p.cel.clone()),
            Some(colour.clone()),
            "{tool:?}: this layer only picks on the active layer"
        );
    });
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_selected_shape_takes_the_colour_as_its_fill_or_outline() {
    let path = sample("shape-colour");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "shapes");
    let id = h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0].nodes[0]
        .id
        .clone();
    h.state_mut().tool = Tool::Select;
    assert!(
        h.state().can_run(Cmd::FillShape).is_some(),
        "nothing selected greys it out"
    );
    h.state_mut().selection = Some(id.clone());
    h.state_mut().color = [0.0, 0.5, 1.0, 1.0];
    h.run_steps(2);
    let node = |h: &Harness<'static, Editor>| match h.state().scene.bundle.doc.item(&id) {
        Some(klad_core::doc::Item::Node(_, _, n)) => n.clone(),
        _ => panic!("the shape is gone"),
    };
    assert!(node(&h).stroke.is_none());
    h.state_mut().invoke(Cmd::FillShape).unwrap();
    h.run_steps(2);
    assert_eq!(
        serde_json::to_value(&node(&h).fill).unwrap(),
        serde_json::json!("#0080ff")
    );
    h.state_mut().color = [1.0, 0.0, 0.0, 1.0];
    h.state_mut().invoke(Cmd::OutlineShape).unwrap();
    h.run_steps(2);
    let outline = serde_json::to_value(node(&h).stroke.unwrap()).unwrap();
    assert_eq!(outline["paint"], "#ff0000", "{outline}");
    assert_eq!(
        serde_json::to_value(&node(&h).fill).unwrap(),
        serde_json::json!("#0080ff")
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn alt_click_on_an_eye_shows_only_that_layer_and_again_brings_the_rest_back() {
    let path = sample("solo");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    [Pane::Tool, Pane::Inspector, Pane::Notes, Pane::History]
        .into_iter()
        .for_each(|pane| {
            if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
                p.collapsed = true;
            }
        });
    h.state_mut().settings.layout.timeline = false;
    h.run_steps(3);
    let eye_of = |h: &Harness<'static, Editor>, name: &str| {
        let row = h
            .query_all_by_label(name)
            .map(|n| n.rect())
            .max_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap();
        h.query_all_by_label(theme::icon::EYE)
            .chain(h.query_all_by_label(theme::icon::EYE_OFF))
            .map(|n| n.rect())
            .find(|r| (r.center().y - row.center().y).abs() < 8.0 && r.right() <= row.left())
            .unwrap()
            .center()
    };
    let visible = |h: &Harness<'static, Editor>| -> Vec<String> {
        h.state()
            .scene
            .bundle
            .doc
            .layers
            .iter()
            .filter(|l| l.visible)
            .map(|l| l.name.clone())
            .collect()
    };
    let before = visible(&h);
    let eye = eye_of(&h, "colour");
    click_with(&mut h, eye, Modifiers::ALT);
    assert_eq!(visible(&h), ["colour"], "only colour shows");
    assert_eq!(
        h.state().is_on(Cmd::SoloLayer),
        Some(false),
        "it is not the active layer"
    );
    let eye = eye_of(&h, "colour");
    click_with(&mut h, eye, Modifiers::ALT);
    assert_eq!(visible(&h), before, "again: the rest comes back as it was");
    let eye = eye_of(&h, "colour");
    click_with(&mut h, eye, Modifiers::ALT);
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(3);
    assert_eq!(visible(&h), before, "one undo takes it back too");
    let _ = std::fs::remove_dir_all(&path);
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
        also: Default::default(),
    });
    h.run_steps(2);
    checked += audit(&h, "points selected");
    h.state_mut().settings.developer = true;
    h.state_mut().settings.layout.numbers = true;
    h.state_mut().windows.settings = true;
    h.state_mut().windows.onion = true;
    h.state_mut().windows.log = true;
    h.state_mut().windows.brush = true;
    h.state_mut().say(Tone::Error, "a test error with a close button");
    h.run_steps(3);
    assert!(
        h.query_by_label_contains("canvas       median").is_some(),
        "the numbers show the canvas work"
    );
    checked += audit(&h, "windows open");
    super::prefs::Section::ALL.into_iter().for_each(|section| {
        h.state_mut().settings_section = section;
        h.run_steps(2);
        checked += audit(&h, section.name());
    });
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
    let canvas = h.state().canvas_rect;
    assert!(
        shown
            .iter()
            .any(|r| r.left() - canvas.left() < 60.0 && (canvas.bottom() - r.bottom()).abs() < 40.0),
        "no toast in the canvas's bottom left corner: {shown:?} in {canvas:?}"
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
    let creature = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/creature/creature.klad");
    let copy = std::env::temp_dir().join(format!("draw-ui-creature-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copy);
    if creature.exists() {
        copy_dir(&creature, &copy);
    }
    let sample = sample("time");
    let stress = std::env::temp_dir().join(format!("draw-ui-stress-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stress);
    let exe = std::env::current_exe().unwrap();
    let draw = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join(format!("klad{}", std::env::consts::EXE_SUFFIX));
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
        h.state_mut().settings.developer = true;
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
    let creature = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/creature/creature.klad");
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
    h.state_mut().settings.developer = true;
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
    h.state_mut().settings_section = super::prefs::Section::Keys;
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
    let mut h = harness(editor(&path), vec2(1440.0, 1060.0));
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
fn a_shape_previews_only_its_tiles_while_dragged_and_two_drags_in_a_row_both_land() {
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
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let middle = rect.min + view.to_screen(egui::Vec2::new(120.0, 30.0), rect.size());
    h.hover_at(middle);
    h.run_steps(1);
    h.drag_at(middle);
    h.run_steps(1);
    (1..=5).for_each(|i| {
        h.hover_at(middle + vec2(8.0 * i as f32, 0.0));
        h.run_steps(1);
    });
    settle(&mut h, |ed| ed.preview.as_ref().is_some_and(|p| p.done.is_some()));
    let (shown, all) = {
        let ed = h.state();
        (ed.preview.as_ref().unwrap().shown.clone(), ed.scene.all_tiles().len())
    };
    assert!(
        !shown.is_empty() && shown.len() < all,
        "a press-and-drag previews the tiles under the shape: {shown:?} of {all}"
    );
    h.drop_at(middle + vec2(40.0, 0.0));
    h.run_steps(1);
    drag(&mut h, middle + vec2(40.0, 0.0), middle + vec2(40.0, 50.0));
    settle(&mut h, |ed| {
        ed.preview.is_none() && ed.drag.is_none() && ed.dirty.is_empty()
    });
    let after = h.state().node_transform(&id).unwrap();
    let moved = (after.x - before.x, after.y - before.y);
    let expect = (40.0 / view.zoom, 50.0 / view.zoom);
    assert!(
        (moved.0 - expect.0).abs() < 2.0 && (moved.1 - expect.1).abs() < 2.0,
        "moved {moved:?}, expected {expect:?}"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn after_a_drag_with_a_pan_every_tile_on_the_canvas_shows_the_committed_picture() {
    let dir = std::env::temp_dir().join(format!("draw-ui-panned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut doc = Document::new(2048, 1024);
    doc.canvas.background = Some("#ffffff".into());
    let mut noisy = Layer::group(
        "g",
        "g",
        vec![Layer::vector(
            "v",
            "shapes",
            "cv",
            vec![
                klad_core::doc::Node::new(
                    "n1",
                    klad_core::doc::Shape::Rect {
                        x: 40.0,
                        y: 40.0,
                        w: 200.0,
                        h: 150.0,
                        radius: 0.0,
                    },
                )
                .filled("#a04030"),
            ],
        )],
    );
    noisy.effects = vec![klad_core::look::Effect::Noise(klad_core::look::Noise::default())];
    doc.layers = vec![noisy];
    Bundle::create(&dir, doc).unwrap();
    let mut h = harness(editor(&dir), vec2(1440.0, 900.0));
    settle(&mut h, |ed| ed.canvas_current() && ed.dirty.is_empty());
    activate(&mut h, "shapes");
    h.state_mut().tool = Tool::Select;
    let mut view = h.state().view.unwrap();
    view.zoom = 3.0;
    view.center = egui::Vec2::new(200.0, 150.0);
    h.state_mut().view = Some(view);
    h.run_steps(2);
    let rect = h.state().canvas_rect;
    let at = |h: &Harness<'static, Editor>, x: f32, y: f32| {
        rect.min + h.state().view.unwrap().to_screen(egui::Vec2::new(x, y), rect.size())
    };
    let start = at(&h, 140.0, 115.0);
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    (1..=6).for_each(|i| {
        h.hover_at(start + vec2(30.0 * i as f32, 0.0));
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    });
    settle(&mut h, |ed| ed.preview.as_ref().is_some_and(|p| p.job.is_none()));
    let mut panned = h.state().view.unwrap();
    panned.center = egui::Vec2::new(1700.0, 800.0);
    h.state_mut().view = Some(panned);
    let end = start + vec2(400.0, 300.0);
    (1..=6).for_each(|i| {
        h.hover_at(start + vec2(180.0, 0.0) + (end - start - vec2(180.0, 0.0)) * (i as f32 / 6.0));
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    });
    h.drop_at(end);
    h.run_steps(1);
    canvas_shows_the_committed_picture(&mut h);
    let _ = std::fs::remove_dir_all(&dir);
}

fn canvas_shows_the_committed_picture(h: &mut Harness<'static, Editor>) {
    settle(h, |ed| ed.preview.is_none() && ed.drag.is_none() && ed.dirty.is_empty());
    let ed = h.state();
    let outbox = crate::stats::lock(&ed.outbox);
    let tiles = ed.scene.all_tiles();
    let committed = ed.scene.compose(&tiles, None).unwrap();
    committed.iter().for_each(|(i, want)| {
        let shown = outbox
            .tile(*i)
            .unwrap_or_else(|| panic!("tile {i} never reached the canvas"));
        let worst = shown
            .iter()
            .zip(want)
            .map(|(a, b)| (a.to_f32() - b.to_f32()).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst <= 2.0 / 255.0,
            "tile {i} shows {:.1}/255 off the committed picture",
            worst * 255.0
        );
    });
}

#[test]
fn strokes_and_copies_dragged_leave_the_committed_picture_on_the_canvas() {
    let path = sample("dragged");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    let cel = h.state().scene.active_cel().unwrap();
    let marks = h.state().scene.marks_of(&cel).to_vec();
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().picked = Some(Picked::whole(cel.clone(), vec![marks[0].id()]));
    h.run_steps(2);
    let [x0, y0, x1, y1] = marks[0].bounds().unwrap();
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let middle = at((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    drag(&mut h, middle, middle + vec2(60.0, 40.0));
    canvas_shows_the_committed_picture(&mut h);
    let moved = h.state().scene.marks_of(&cel)[0].bounds().unwrap();
    assert!(moved[0] > x0 + 10.0, "the stroke moved: {moved:?} from {x0}");
    activate(&mut h, "shapes");
    h.state_mut().tool = Tool::Select;
    let count = |h: &Harness<'static, Editor>| {
        h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0]
            .nodes
            .len()
    };
    let shapes = count(&h);
    let corner = at(120.0, 30.0);
    h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut h, corner, corner + vec2(0.0, 100.0));
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    canvas_shows_the_committed_picture(&mut h);
    assert_eq!(count(&h), shapes + 1, "alt drag made a copy");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn several_shapes_select_with_shift_or_a_lasso_move_together_align_and_distribute() {
    let path = sample("align");
    {
        let mut bundle = Bundle::open(&path).unwrap();
        let shapes = bundle.doc.find_layer("shapes").unwrap().id.clone();
        let rect = |x: u32, y: u32, w: u32| {
            format!(
                r##"{{"op":"add","node":{{"type":"rect","x":{x},"y":{y},"w":{w},"h":40,"fill":"#3060a0"}},"in":"{shapes}","frame":0}}"##
            )
        };
        bundle
            .apply(&parse_lines(&format!("{}\n{}", rect(300, 30, 60), rect(420, 300, 40))).unwrap())
            .unwrap();
    }
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "shapes");
    h.state_mut().tool = Tool::Select;
    let ids: Vec<String> = h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0]
        .nodes
        .iter()
        .map(|n| n.id.clone())
        .collect();
    let boxes = |h: &Harness<'static, Editor>| -> Vec<[f32; 4]> {
        ids.iter()
            .map(|id| {
                klad_render::overlay::node_world_box(&h.state().scene.bundle.doc, id)
                    .unwrap()
                    .unwrap()
            })
            .collect()
    };
    assert_eq!(boxes(&h).iter().map(|b| b[0]).collect::<Vec<_>>(), [20.0, 300.0, 420.0]);
    let at = on_screen(&h, 120.0, 30.0);
    click_with(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().shapes(), ids[..1], "a click takes one shape");
    assert!(
        h.state().can_run(Cmd::DistributeAcross).is_some(),
        "distributing needs three"
    );
    let at = on_screen(&h, 330.0, 50.0);
    click_with(&mut h, at, Modifiers::SHIFT);
    let at = on_screen(&h, 440.0, 320.0);
    click_with(&mut h, at, Modifiers::SHIFT);
    assert_eq!(h.state().shapes(), ids, "shift click adds each shape");
    assert!(
        h.query_all(By::new().label_contains(crate::theme::icon::ALIGN_TOP))
            .count()
            > 0,
        "the bar under several shapes aligns them"
    );
    let rev = h.state().scene.bundle.doc.rev;
    h.state_mut().invoke(Cmd::AlignTop).unwrap();
    h.run_steps(2);
    assert_eq!(boxes(&h).iter().map(|b| b[1]).collect::<Vec<_>>(), [20.0, 20.0, 20.0]);
    assert_eq!(h.state().scene.bundle.doc.rev, rev + 1, "one step for the three");
    h.state_mut().invoke(Cmd::DistributeAcross).unwrap();
    h.run_steps(2);
    assert_eq!(boxes(&h).iter().map(|b| b[0]).collect::<Vec<_>>(), [20.0, 250.0, 420.0]);
    let from = on_screen(&h, 120.0, 30.0);
    let to = on_screen(&h, 150.0, 30.0);
    drag(&mut h, from, to);
    settle(&mut h, |ed| ed.drag.is_none() && ed.preview.is_none());
    let moved = boxes(&h);
    [50.0, 280.0, 450.0].iter().zip(&moved).for_each(|(want, b)| {
        assert!((b[0] - want).abs() < 1.0, "the three moved together: {moved:?}");
    });
    assert_eq!(h.state().shapes(), ids, "and stay selected");
    let at = on_screen(&h, 470.0, 40.0);
    click_with(&mut h, at, Modifiers::SHIFT);
    assert_eq!(
        h.state().shapes(),
        ids[..2],
        "shift click on a selected shape lets it go"
    );
    h.state_mut().invoke(Cmd::Deselect).unwrap();
    h.run_steps(1);
    assert!(h.state().shapes().is_empty());
    let around =
        [(262.0, 4.0), (505.0, 4.0), (505.0, 75.0), (262.0, 75.0), (262.0, 10.0)].map(|(x, y)| on_screen(&h, x, y));
    h.hover_at(around[0]);
    h.run_steps(1);
    h.drag_at(around[0]);
    h.run_steps(1);
    around[1..].iter().for_each(|p| {
        h.hover_at(*p);
        h.run_steps(1);
    });
    h.drop_at(around[4]);
    h.run_steps(2);
    assert_eq!(
        h.state().shapes(),
        ids[1..],
        "a lasso on a shapes layer takes the shapes inside"
    );
    h.state_mut().invoke(Cmd::AlignLeft).unwrap();
    h.run_steps(2);
    let lined = boxes(&h);
    assert_eq!(
        lined[1][0], lined[2][0],
        "two shapes line up on the left one: {lined:?}"
    );
    h.key_press(Key::Delete);
    h.run_steps(2);
    let left = h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0]
        .nodes
        .len();
    assert_eq!(left, 1, "Delete removes every selected shape");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_pixel_layer_is_added_shows_its_marks_hard_and_keeps_brush_strokes_out() {
    let path = sample("pixel-layer");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "notes");
    assert!(h.state().can_run(Cmd::NewPixelLayer).is_none());
    h.state_mut().invoke(Cmd::NewPixelLayer).unwrap();
    h.run_steps(3);
    ready(&mut h);
    let layer = h.state().scene.active_layer().unwrap().clone();
    assert!(
        layer.pixel() && h.state().scene.on_grid(),
        "the new layer is a pixel layer and active"
    );
    assert!(
        h.query_by_label("pixel layer").is_some(),
        "layer settings say what kind of layer it is"
    );
    let rect = h.state().canvas_rect;
    let (a, b) = (rect.center() + vec2(-60.0, -20.0), rect.center() + vec2(40.0, 30.0));
    h.state_mut().tool = Tool::Brush;
    drag(&mut h, a, b);
    h.run_steps(3);
    ready(&mut h);
    assert!(
        !h.state().scene.active_marks().is_empty()
            && h.state()
                .scene
                .active_marks()
                .iter()
                .all(|m| m.as_fill().is_some_and(|f| f.on_grid())),
        "the brush tool draws whole pixels there, never a brush stroke"
    );
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(3);
    ready(&mut h);
    assert!(h.state().scene.active_marks().is_empty());
    h.state_mut()
        .apply(
            parse_lines(&format!(
                r##"{{"op":"pixel","layer":"{}","color":"#3a86ff","cells":[[10,10],[11,10],[10,11]]}}"##,
                layer.id
            ))
            .unwrap(),
        )
        .unwrap();
    h.run_steps(3);
    ready(&mut h);
    let ed = h.state();
    let raster = ed.scene.active_raster().expect("the pixel layer's drawing");
    let tile = raster.at(0).expect("the first tile is painted");
    let at = |x: usize, y: usize| -> [f32; 4] {
        let i = (y * klad_brush::TILE + x) * 4;
        [tile[i], tile[i + 1], tile[i + 2], tile[i + 3]].map(|v| v.to_f32())
    };
    let blue = klad_core::color::parse_hex("#3a86ff").unwrap();
    [(10, 10), (11, 10), (10, 11)].iter().for_each(|&(x, y)| {
        let got = at(x, y);
        assert!(
            got.iter().zip(blue).all(|(a, b)| (a - b).abs() < 1e-3),
            "{x},{y} is {got:?}, not {blue:?}"
        );
    });
    assert_eq!(at(11, 11), [0.0; 4], "no soft edge beside the pixels");
    assert_eq!(at(9, 10), [0.0; 4]);
    assert!(
        ed.scene.bundle.doc.swatch("#3a86ff").is_some(),
        "its colour joined the palette"
    );
    assert!(ed.scene.active_marks()[0].as_fill().is_some_and(|f| f.on_grid()));
    let _ = std::fs::remove_dir_all(&path);
}

fn pixel_sample(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("draw-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut bundle = Bundle::blank(
        &dir,
        &klad_core::bundle::Blank {
            width: 32,
            height: 24,
            pixel: true,
            ..klad_core::bundle::Blank::default()
        },
        "cli",
    )
    .unwrap();
    let layer = bundle.doc.layers[0].id.clone();
    bundle
        .apply(
            &parse_lines(&format!(
                "{}\n{}",
                format_args!(
                    r##"{{"op":"pixel","layer":"{layer}","color":"#2b2d42","cells":[[4,4],[5,4],[6,4],[4,5],[6,5],[4,6],[5,6],[6,6]]}}"##
                ),
                format_args!(r##"{{"op":"pixel","layer":"{layer}","color":"#ef233c","cells":[[5,5]]}}"##),
            ))
            .unwrap(),
        )
        .unwrap();
    dir
}

#[test]
fn a_pixel_drawing_zooms_in_whole_steps_and_shows_its_grid_the_pointers_pixel_and_its_real_size() {
    let path = pixel_sample("sharp");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    let ppp = h.ctx.pixels_per_point();
    let whole = |h: &Harness<'static, Editor>| {
        let px = h.state().view.unwrap().zoom * ppp;
        assert!(
            (px - px.round()).abs() < 1e-4,
            "{px} screen pixels per pixel is not a whole number"
        );
        px.round() as u32
    };
    assert!(whole(&h) >= 8, "a 32 pixel drawing opens enlarged");
    h.state_mut().invoke(Cmd::Fit).unwrap();
    h.run_steps(2);
    let fitted = whole(&h);
    let rect = h.state().canvas_rect;
    assert!(fitted >= 8, "and fits enlarged: {fitted}");
    assert!(
        32.0 * fitted as f32 / ppp <= rect.width() && 24.0 * fitted as f32 / ppp <= rect.height(),
        "and whole on the canvas"
    );
    assert!(h.state().grid_shows(), "zoomed in this far the grid shows");
    h.hover_at(rect.center());
    h.run_steps(1);
    (0..6).for_each(|_| {
        h.event(egui::Event::Zoom(0.9));
        h.run_steps(1);
        whole(&h);
    });
    let smaller = whole(&h);
    assert!(smaller < fitted, "pinching zooms out: {smaller} from {fitted}");
    (0..40).for_each(|_| {
        h.event(egui::Event::Zoom(0.8));
        h.run_steps(1);
    });
    assert!(
        h.state().view.unwrap().zoom * ppp < 1.0,
        "below real size the zoom is free again"
    );
    assert!(!h.state().grid_shows(), "and the grid is gone");
    h.state_mut().invoke(Cmd::Fit).unwrap();
    h.run_steps(2);
    assert_eq!(whole(&h), fitted, "fit comes back to the largest whole zoom");
    let at = on_screen(&h, 5.5, 7.5);
    h.hover_at(at);
    h.run_steps(2);
    assert_eq!(h.state().hover_cell, Some([5, 7]));
    assert!(
        h.query_by_label("5, 7").is_some(),
        "the status line names the pixel under the pointer"
    );
    h.state_mut().invoke(Cmd::PixelGrid).unwrap();
    h.run_steps(1);
    assert!(!h.state().grid_shows() && h.state().is_on(Cmd::PixelGrid) == Some(false));
    h.state_mut().invoke(Cmd::PixelGrid).unwrap();
    assert!(!h.state().settings.layout.is_open(Pane::Preview));
    h.state_mut().invoke(Cmd::PreviewPane).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Real size").is_some() && h.query_by_label("32 x 24").is_some());
    let (_, texture) = h.state().real.as_ref().expect("the drawing at its real size");
    assert_eq!(texture.size(), [32, 24]);
    h.get_by_label("2x").click();
    h.run_steps(2);
    assert_eq!(h.state().settings.real_size_scale, 2);
    let painting = sample("no-grid");
    let plain = harness(editor(&painting), vec2(1440.0, 900.0));
    assert!(
        plain
            .state()
            .can_run(Cmd::PixelGrid)
            .is_some_and(|why| why.contains("no pixel layer"))
    );
    let px = plain.state().view.unwrap().zoom * ppp;
    assert!(
        (px - px.round()).abs() > 1e-3,
        "a painting still fits at any zoom: {px}"
    );
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::remove_dir_all(&painting);
}

#[test]
fn a_clicked_palette_colour_is_updated_with_its_pixels_and_ramps_and_palettes_join_the_palette() {
    let path = pixel_sample("palette-edit");
    let mut h = harness(editor(&path), vec2(1440.0, 1060.0));
    let hexes = |h: &Harness<'static, Editor>| -> Vec<String> {
        h.state()
            .scene
            .bundle
            .doc
            .palette
            .iter()
            .map(|s| s.color.clone())
            .collect()
    };
    assert_eq!(hexes(&h), ["#2b2d42", "#ef233c"]);
    assert!(
        h.state()
            .can_run(Cmd::UpdateSwatch)
            .is_some_and(|why| why.contains("click a colour in the palette first"))
    );
    let red = h.get_by_role_and_label(Role::Button, "#ef233c").rect();
    h.hover_at(red.center());
    h.run_steps(1);
    h.drag_at(red.center());
    h.run_steps(1);
    h.drop_at(red.center());
    h.run_steps(2);
    assert_eq!(h.state().swatch_picked, Some(1));
    assert!(
        h.state()
            .can_run(Cmd::UpdateSwatch)
            .is_some_and(|why| why.contains("already is this colour"))
    );
    assert!(h.query_by_label_contains("update").is_none(), "nothing to update yet");
    let teal = klad_core::color::parse_hex("#2a9d8f").unwrap();
    h.state_mut().set_colour(teal);
    h.run_steps(2);
    h.state_mut().invoke(Cmd::UpdateSwatch).unwrap();
    h.run_steps(3);
    ready(&mut h);
    assert_eq!(hexes(&h), ["#2b2d42", "#2a9d8f"]);
    let marks = h.state().scene.active_marks().to_vec();
    let centre = marks[1].as_fill().unwrap();
    assert_eq!(
        klad_core::color::to_hex(centre.color()),
        "#2a9d8f",
        "the pixel painted with it changed"
    );
    assert_eq!(klad_core::color::to_hex(marks[0].as_fill().unwrap().color()), "#2b2d42");
    let raster = h.state().scene.active_raster().unwrap();
    let tile = raster.at(0).unwrap();
    let at = (5 * klad_brush::TILE + 5) * 4;
    assert!(
        (0..3).all(|i| (tile[at + i].to_f32() - teal[i]).abs() < 1e-3),
        "and the canvas shows it"
    );
    h.state_mut().swatch_name = "sea".into();
    h.state_mut().invoke(Cmd::AddRamp).unwrap();
    h.run_steps(2);
    let doc = h.state().scene.bundle.doc.clone();
    let ramp: Vec<&str> = doc.ramp("sea").iter().map(|s| s.color.as_str()).collect();
    assert_eq!(ramp.len(), 5);
    assert_eq!(ramp[2], "#2a9d8f", "the colour itself is the middle of its ramp");
    assert_eq!(
        doc.palette.len(),
        6,
        "the palette colour joined the ramp instead of being added twice"
    );
    h.state_mut().invoke(Cmd::LoadPalette).unwrap();
    h.run_steps(2);
    h.get_by_label("PICO-8").click();
    h.run_steps(3);
    assert_eq!(h.state().scene.bundle.doc.palette.len(), 22);
    assert!(
        !h.state().windows.palettes,
        "the window closes once a palette is loaded"
    );
    let said = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    assert!(said.contains("16 colours added"), "{said}");
    let file = path.with_extension("hex");
    std::fs::write(&file, "ff00ff\n00ffff\n000000\n").unwrap();
    h.state_mut().invoke(Cmd::LoadPalette).unwrap();
    h.state_mut().palette_path = file.display().to_string();
    h.run_steps(2);
    h.get_by_label("load").click();
    h.run_steps(3);
    assert_eq!(
        h.state().scene.bundle.doc.palette.len(),
        24,
        "two new colours: black is in the palette already"
    );
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_brush_tool_is_a_pixel_pencil_and_eraser_on_a_pixel_layer() {
    let path = pixel_sample("pencil");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    h.state_mut().tool = Tool::Brush;
    h.state_mut()
        .set_colour(klad_core::color::parse_hex("#2a9d8f").unwrap());
    h.run_steps(2);
    assert!(
        h.query_by_label_contains("pixel pencil").is_some(),
        "the options bar names the pencil"
    );
    let cells_of = |h: &Harness<'static, Editor>, at: usize| -> Vec<[u32; 2]> {
        h.state().scene.active_marks()[at]
            .as_fill()
            .unwrap()
            .coverage
            .cells()
            .collect()
    };
    let before = h.state().scene.active_marks().len();
    let (a, b) = (on_screen(&h, 10.5, 10.5), on_screen(&h, 16.5, 13.5));
    h.hover_at(a);
    h.run_steps(1);
    h.drag_at(a);
    h.run_steps(1);
    (1..=12).for_each(|i| {
        h.hover_at(a + (b - a) * (i as f32 / 12.0));
        h.run_steps(1);
    });
    assert!(h.state().pixel_live.is_some(), "the line shows while it is drawn");
    h.drop_at(b);
    h.run_steps(3);
    ready(&mut h);
    let marks = h.state().scene.active_marks().to_vec();
    assert_eq!(marks.len(), before + 1, "one drag is one mark");
    let line = marks[before].as_fill().unwrap();
    assert!(line.on_grid() && line.author == klad_core::stroke::Author::Human);
    assert_eq!(klad_core::color::to_hex(line.color()), "#2a9d8f");
    let teal = h
        .state()
        .scene
        .bundle
        .doc
        .swatch("#2a9d8f")
        .expect("the colour joined the palette");
    assert_eq!(line.entry(), Some(h.state().scene.bundle.doc.palette[teal].id.as_str()));
    let cells = cells_of(&h, before);
    assert_eq!(cells.len(), 7, "one pixel per column from 10 to 16: {cells:?}");
    assert!(cells.contains(&[10, 10]) && cells.contains(&[16, 13]));
    let raster = h.state().scene.active_raster().unwrap();
    assert!(raster.alpha_at(12.5, 11.5) + raster.alpha_at(12.5, 10.5) + raster.alpha_at(12.5, 12.5) >= 1.0);
    let far = on_screen(&h, 16.5, 20.5);
    click_with(&mut h, far, Modifiers::SHIFT);
    h.run_steps(3);
    assert_eq!(
        cells_of(&h, before + 1),
        (13..=20).map(|y| [16, y]).collect::<Vec<_>>(),
        "shift click draws a straight line from the last pixel"
    );
    h.key_press(Key::CloseBracket);
    h.key_press(Key::CloseBracket);
    h.run_steps(2);
    assert_eq!(h.state().pixel_size, 3, "] makes the pencil a pixel larger");
    let dot = on_screen(&h, 25.5, 5.5);
    click_with(&mut h, dot, Modifiers::NONE);
    h.run_steps(3);
    let block = cells_of(&h, before + 2);
    assert_eq!(block.len(), 9);
    assert!(block.contains(&[24, 4]) && block.contains(&[26, 6]));
    h.state_mut().invoke(Cmd::Eraser).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("pixel eraser").is_some());
    h.state_mut().pixel_size = 1;
    click_with(&mut h, dot, Modifiers::NONE);
    h.run_steps(3);
    ready(&mut h);
    let marks = h.state().scene.active_marks().to_vec();
    assert!(marks[before + 3].as_fill().unwrap().erases());
    let raster = h.state().scene.active_raster().unwrap();
    assert_eq!(raster.alpha_at(25.5, 5.5), 0.0, "the erased pixel is clear");
    assert_eq!(raster.alpha_at(24.5, 5.5), 1.0, "its neighbour is still there");
    let palette = h.state().scene.bundle.doc.palette.len();
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(3);
    assert_eq!(
        h.state().scene.active_marks().len(),
        before + 3,
        "undo takes the eraser mark back"
    );
    assert_eq!(h.state().scene.bundle.doc.palette.len(), palette);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_fill_and_shape_tools_draw_whole_pixels_on_a_pixel_layer() {
    let path = pixel_sample("fill-shape");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    h.state_mut()
        .set_colour(klad_core::color::parse_hex("#f2c14e").unwrap());
    let cells_of = |h: &Harness<'static, Editor>, at: usize| -> Vec<[u32; 2]> {
        h.state().scene.active_marks()[at]
            .as_fill()
            .unwrap()
            .coverage
            .cells()
            .collect()
    };
    let before = h.state().scene.active_marks().len();
    h.state_mut().tool = Tool::Fill;
    h.run_steps(2);
    assert!(
        h.query_by_label("across corners").is_some(),
        "the fill tool shows its pixel options"
    );
    let centre = on_screen(&h, 5.5, 5.5);
    click_with(&mut h, centre, Modifiers::NONE);
    h.run_steps(3);
    ready(&mut h);
    assert_eq!(
        cells_of(&h, before),
        [[5, 5]],
        "the fill takes the one red pixel inside the ring"
    );
    let mark = h.state().scene.active_marks()[before].as_fill().unwrap().clone();
    assert!(mark.on_grid() && mark.author == klad_core::stroke::Author::Human);
    assert_eq!(klad_core::color::to_hex(mark.color()), "#f2c14e");
    let outside = on_screen(&h, 20.5, 20.5);
    click_with(&mut h, outside, Modifiers::NONE);
    h.run_steps(3);
    ready(&mut h);
    assert_eq!(
        cells_of(&h, before + 1).len(),
        32 * 24 - 9,
        "outside the ring the fill takes every empty pixel and stops at the ring"
    );
    h.state_mut().invoke(Cmd::Undo).unwrap();
    h.run_steps(3);
    ready(&mut h);
    h.state_mut().tool = Tool::Shape;
    h.run_steps(2);
    assert!(
        h.query_by_label("filled").is_some(),
        "the shape tool shows its pixel options"
    );
    let (a, b) = (on_screen(&h, 10.5, 10.5), on_screen(&h, 17.5, 15.5));
    drag(&mut h, a, b);
    h.run_steps(3);
    ready(&mut h);
    let outline = cells_of(&h, before + 1);
    assert_eq!(
        outline.len(),
        2 * 8 + 2 * 4,
        "an 8 by 6 rectangle's outline: {outline:?}"
    );
    assert!(outline.contains(&[10, 10]) && outline.contains(&[17, 15]) && !outline.contains(&[12, 12]));
    h.state_mut().pixel_shape = super::PixelShape::Ellipse;
    h.state_mut().pixel_filled = true;
    let (a, b) = (on_screen(&h, 20.5, 2.5), on_screen(&h, 26.5, 8.5));
    drag(&mut h, a, b);
    h.run_steps(3);
    ready(&mut h);
    let disc = cells_of(&h, before + 2);
    assert!(
        disc.contains(&[23, 5]) && !disc.contains(&[20, 2]),
        "a filled circle, without its box's corners"
    );
    assert!(disc.len() > 25 && disc.len() < 49, "{}", disc.len());
    h.state_mut().pixel_shape = super::PixelShape::Line;
    let (a, b) = (on_screen(&h, 2.5, 20.5), on_screen(&h, 9.5, 20.5));
    drag(&mut h, a, b);
    h.run_steps(3);
    assert_eq!(cells_of(&h, before + 3), (2..=9).map(|x| [x, 20]).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn pixels_are_selected_by_box_or_colour_and_move_flip_turn_and_recolour_on_the_grid() {
    let path = pixel_sample("pixel-edit");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    h.state_mut().tool = Tool::Strokes;
    h.run_steps(2);
    assert!(h.query_by_label("box").is_some() && h.query_by_label_contains("same colour").is_some());
    let grid = |h: &Harness<'static, Editor>| -> Vec<String> {
        let ed = h.state();
        let doc = &ed.scene.bundle.doc;
        let grid = klad_core::pixel::grid_of(ed.scene.active_marks(), doc.canvas.width, doc.canvas.height);
        let legend = klad_core::pixel::legend(doc, &grid);
        klad_core::pixel::rows(&grid, &legend)
    };
    assert_eq!(&grid(&h)[4][..9], "....aaa..");
    assert_eq!(&grid(&h)[5][..9], "....aba..");
    let (a, b) = (on_screen(&h, 3.6, 3.6), on_screen(&h, 5.4, 7.4));
    h.hover_at(a);
    h.run_steps(1);
    h.drag_at(a);
    h.run_steps(1);
    (1..=6).for_each(|i| {
        h.hover_at(a + (b - a) * (i as f32 / 6.0));
        h.run_steps(1);
    });
    h.drop_at(b);
    h.run_steps(3);
    let picked = h.state().picked.clone().expect("the box holds pixels");
    assert!(
        picked.cut.is_some(),
        "the box crosses the ring, so it takes pixels, not whole marks"
    );
    assert_eq!(
        picked.cut.as_deref(),
        Some(&[[3.0, 3.0], [6.0, 3.0], [6.0, 8.0], [3.0, 8.0]][..]),
        "the box snaps out to whole pixels"
    );
    let (from, to) = (on_screen(&h, 4.5, 5.5), on_screen(&h, 14.2, 5.9));
    drag(&mut h, from, to);
    settle(&mut h, |ed| ed.drag.is_none() && ed.preview.is_none());
    h.run_steps(3);
    ready(&mut h);
    let rows = grid(&h);
    assert_eq!(
        &rows[4][..18],
        "......a.......aa..",
        "the left two columns of the ring moved 10 pixels: {rows:?}"
    );
    assert_eq!(&rows[5][..18], "......a.......ab..");
    assert_eq!(&rows[6][..18], "......a.......aa..");
    assert!(
        h.state()
            .scene
            .active_marks()
            .iter()
            .all(|m| m.cuts().is_empty() && m.as_fill().unwrap().coverage.alpha.iter().all(|a| *a == 0 || *a == 255)),
        "every mark is still whole pixels with no soft edge"
    );
    h.state_mut().invoke(Cmd::FlipH).unwrap();
    h.run_steps(3);
    ready(&mut h);
    let rows = grid(&h);
    assert_eq!(&rows[5][..18], "......a.......ba..", "flipped in place: {rows:?}");
    h.state_mut().invoke(Cmd::TurnRight90).unwrap();
    h.run_steps(3);
    ready(&mut h);
    let turned = grid(&h);
    let count = |rows: &[String], sign: char| rows.iter().flat_map(|r| r.chars()).filter(|c| *c == sign).count();
    assert_eq!(
        (count(&turned, 'a'), count(&turned, 'b')),
        (8, 1),
        "a quarter turn keeps every pixel: {turned:?}"
    );
    h.state_mut().invoke(Cmd::Deselect).unwrap();
    h.state_mut()
        .set_colour(klad_core::color::parse_hex("#2b2d42").unwrap());
    h.state_mut().invoke(Cmd::SelectSameColour).unwrap();
    h.run_steps(2);
    let same = h.state().picked.clone().expect("the ink marks");
    assert!(
        same.ids.len() >= 2 && same.cut.is_none(),
        "every mark in the ring's colour: {:?}",
        same.ids
    );
    h.state_mut()
        .set_colour(klad_core::color::parse_hex("#2a9d8f").unwrap());
    h.state_mut().invoke(Cmd::Recolour).unwrap();
    h.run_steps(3);
    ready(&mut h);
    let ed = h.state();
    let teal = ed
        .scene
        .bundle
        .doc
        .swatch("#2a9d8f")
        .expect("the new colour is a palette colour");
    let id = ed.scene.bundle.doc.palette[teal].id.clone();
    let recoloured = ed
        .scene
        .active_marks()
        .iter()
        .filter(|m| m.as_fill().unwrap().entry() == Some(id.as_str()))
        .count();
    assert_eq!(
        recoloured,
        same.ids.len(),
        "each takes exactly the colour and its palette entry"
    );
    assert!(
        ed.scene.bundle.doc.palette.len() <= 3,
        "no shades were invented: {:?}",
        ed.scene.bundle.doc.palette
    );
    let plain = sample("no-same-colour");
    let other = harness(editor(&plain), vec2(1440.0, 900.0));
    assert!(other.state().can_run(Cmd::SelectSameColour).is_some());
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::remove_dir_all(&plain);
}

#[test]
fn the_pencil_shades_and_dithers_and_a_selection_steps_along_its_ramp_and_takes_an_outline() {
    let path = pixel_sample("pixel-shade");
    {
        let mut bundle = Bundle::open(&path).unwrap();
        let layer = bundle.doc.layers[0].id.clone();
        let ramp = klad_core::palettes::ramp_swatches("green", "#4c9a54", 3).unwrap();
        let keys: Vec<String> = ramp.iter().map(|s| format!("\"{}\"", s.color)).collect();
        bundle
            .apply(
                &parse_lines(&format!(
                    "{}\n{}\n{}",
                    format_args!(
                        r##"{{"op":"palette","load":{}}}"##,
                        serde_json::to_string(&ramp).unwrap()
                    ),
                    format_args!(r##"{{"op":"palette","ramp":"green","of":[{}]}}"##, keys.join(",")),
                    format_args!(
                        r##"{{"op":"pixel","layer":"{layer}","color":"green-2","rect":[12,4,8,6],"filled":true}}"##
                    ),
                ))
                .unwrap(),
            )
            .unwrap();
    }
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    let grid = |h: &Harness<'static, Editor>| -> Vec<String> {
        let ed = h.state();
        let doc = &ed.scene.bundle.doc;
        let grid = klad_core::pixel::grid_of(ed.scene.active_marks(), doc.canvas.width, doc.canvas.height);
        klad_core::pixel::rows(&grid, &klad_core::pixel::legend(doc, &grid))
    };
    let green = |h: &Harness<'static, Editor>, name: &str| -> char {
        let ed = h.state();
        let doc = &ed.scene.bundle.doc;
        let grid = klad_core::pixel::grid_of(ed.scene.active_marks(), doc.canvas.width, doc.canvas.height);
        let at = doc.swatch(name).unwrap();
        klad_core::pixel::legend(doc, &grid)
            .iter()
            .find(|n| n.key == doc.palette[at].id || n.color == doc.palette[at].color)
            .map_or('?', |n| n.sign)
    };
    h.state_mut().tool = Tool::Brush;
    h.run_steps(2);
    assert!(h.query_by_label("lighter").is_some() && h.query_by_label("darker").is_some());
    h.state_mut().pixel_mode = super::PixelMode::Darker;
    let (a, b) = (on_screen(&h, 12.5, 8.5), on_screen(&h, 19.5, 8.5));
    drag(&mut h, a, b);
    h.run_steps(3);
    ready(&mut h);
    let mid = green(&h, "green-2");
    let dark = green(&h, "green-1");
    assert_eq!(
        &grid(&h)[8][12..20],
        dark.to_string().repeat(8),
        "the row the pencil went over is a step darker"
    );
    assert_eq!(&grid(&h)[7][12..20], mid.to_string().repeat(8));
    let empty = on_screen(&h, 28.5, 20.5);
    click_with(&mut h, empty, Modifiers::NONE);
    h.run_steps(3);
    let refused = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    assert!(refused.contains("in no ramp"), "{refused}");
    h.state_mut().pixel_mode = super::PixelMode::Draw;
    h.state_mut().pixel_dither = 50;
    h.state_mut().pixel_two = true;
    h.state_mut()
        .set_colour(klad_core::color::parse_hex("#22223b").unwrap());
    h.state_mut().second = klad_core::color::parse_hex("#ef233c").unwrap();
    h.state_mut().tool = Tool::Shape;
    h.state_mut().pixel_filled = true;
    let (a, b) = (on_screen(&h, 24.5, 12.5), on_screen(&h, 27.5, 13.5));
    drag(&mut h, a, b);
    h.run_steps(3);
    ready(&mut h);
    let rows = grid(&h);
    let (one, two) = (rows[12].as_bytes()[24], rows[12].as_bytes()[25]);
    assert!(
        one != two && one != b'.' && two != b'.',
        "two colours side by side: {}",
        &rows[12][24..28]
    );
    assert_eq!(
        (
            rows[12].as_bytes()[26],
            rows[13].as_bytes()[24],
            rows[13].as_bytes()[25]
        ),
        (one, two, one),
        "in a checkerboard"
    );
    h.state_mut().pixel_dither = 100;
    h.state_mut().tool = Tool::Strokes;
    let block = on_screen(&h, 15.5, 5.5);
    click_with(&mut h, block, Modifiers::NONE);
    h.run_steps(3);
    assert!(h.state().picked.is_some(), "the green block is selected");
    h.state_mut().invoke(Cmd::ShadeLighter).unwrap();
    h.run_steps(3);
    ready(&mut h);
    let light = green(&h, "green-3");
    assert_eq!(
        &grid(&h)[5][12..20],
        light.to_string().repeat(8),
        "the mark took the next lighter colour of its ramp"
    );
    h.state_mut().invoke(Cmd::ShadeLighter).unwrap();
    h.run_steps(2);
    let end = h.state().messages.latest().map(|m| m.text.clone()).unwrap_or_default();
    assert!(end.contains("light end"), "{end}");
    h.state_mut().invoke(Cmd::Deselect).unwrap();
    let before = h.state().scene.active_marks().len();
    h.state_mut()
        .set_colour(klad_core::color::parse_hex("#101018").unwrap());
    h.state_mut().invoke(Cmd::PixelOutline).unwrap();
    h.run_steps(3);
    ready(&mut h);
    assert_eq!(
        h.state().scene.active_marks().len(),
        before + 1,
        "the outline is one mark"
    );
    let outlined = grid(&h);
    assert_ne!(
        outlined[3].as_bytes()[13],
        b'.',
        "a pixel above the block is outlined: {}",
        outlined[3]
    );
    assert_eq!(outlined[3].as_bytes()[11], b'.', "its corner is left open");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_tap_that_wobbles_selects_and_a_drag_past_three_points_moves() {
    let path = sample("tap");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "shapes");
    let id = {
        let ed = h.state();
        ed.scene.bundle.doc.find_layer("shapes").unwrap().cels()[0].nodes[0]
            .id
            .clone()
    };
    h.state_mut().tool = Tool::Select;
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = rect.min + view.to_screen(egui::Vec2::new(120.0, 30.0), rect.size());
    let rev = h.state().scene.bundle.doc.rev;
    drag(&mut h, at, at + vec2(1.5, -1.0));
    settle(&mut h, |ed| ed.drag.is_none() && ed.preview.is_none());
    assert_eq!(h.state().selection.as_deref(), Some(id.as_str()), "the tap selects");
    assert_eq!(h.state().scene.bundle.doc.rev, rev, "a wobble is not a move");
    let before = h.state().node_transform(&id).unwrap();
    drag(&mut h, at, at + vec2(5.0, 0.0));
    settle(&mut h, |ed| ed.drag.is_none() && ed.preview.is_none());
    let after = h.state().node_transform(&id).unwrap();
    assert!(
        ((after.x - before.x) - 5.0 / view.zoom).abs() < 0.5,
        "moved {} for 5 pt at zoom {}",
        after.x - before.x,
        view.zoom
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn an_edit_or_a_tool_switch_during_a_drag_leaves_the_right_picture() {
    let path = sample("interrupted");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "shapes");
    h.state_mut().tool = Tool::Select;
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let corner = rect.min + view.to_screen(egui::Vec2::new(120.0, 30.0), rect.size());
    let press = |h: &mut Harness<'static, Editor>, from: egui::Pos2, steps: usize| {
        h.hover_at(from);
        h.run_steps(1);
        h.drag_at(from);
        h.run_steps(1);
        (1..=steps).for_each(|i| {
            h.hover_at(from + vec2(12.0 * i as f32, 6.0 * i as f32));
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.run_steps(1);
        });
    };
    press(&mut h, corner, 3);
    settle(&mut h, |ed| ed.preview.as_ref().is_some_and(|p| p.done.is_some()));
    let lines = h.state().scene.bundle.doc.find_layer("lines").unwrap().id.clone();
    h.state_mut()
        .apply(parse_lines(&format!(r#"{{"op":"set","id":"{lines}","opacity":0.4}}"#)).unwrap())
        .unwrap();
    (4..=6).for_each(|i| {
        h.hover_at(corner + vec2(12.0 * i as f32, 6.0 * i as f32));
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_steps(1);
    });
    h.drop_at(corner + vec2(72.0, 36.0));
    h.run_steps(1);
    canvas_shows_the_committed_picture(&mut h);
    let moved = h.state().scene.bundle.doc.rev;
    press(&mut h, corner + vec2(72.0, 36.0), 4);
    settle(&mut h, |ed| ed.preview.as_ref().is_some_and(|p| p.done.is_some()));
    h.state_mut().tool = Tool::Brush;
    h.run_steps(2);
    assert!(h.state().drag.is_none(), "a tool switch ends the drag");
    canvas_shows_the_committed_picture(&mut h);
    assert_eq!(h.state().scene.bundle.doc.rev, moved, "an ended drag commits nothing");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_layer_made_and_activated_at_once_or_mid_refill_still_draws() {
    let path = sample("made");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    settle(&mut h, |ed| ed.canvas_current() && ed.dirty.is_empty());
    h.state_mut()
        .apply(parse_lines(r#"{"op":"add","layer":{"kind":"vector","name":"late"}}"#).unwrap())
        .unwrap();
    let late = h.state().scene.bundle.doc.find_layer("late").unwrap().id.clone();
    h.state_mut()
        .apply(
            parse_lines(&format!(
                r##"{{"op":"add","node":{{"type":"rect","x":300,"y":200,"w":150,"h":120,"fill":"#2040c0"}},"in":"{late}","frame":0}}"##
            ))
            .unwrap(),
        )
        .unwrap();
    let path_of = |h: &Harness<'static, Editor>, id: &str| match h.state().scene.bundle.doc.locate(id) {
        Some(klad_core::doc::Loc::Layer(path)) => path,
        _ => panic!("no layer {id}"),
    };
    let late_path = path_of(&h, &late);
    h.state_mut().activate(late_path).unwrap();
    canvas_shows_the_committed_picture(&mut h);
    h.state_mut().redraw_canvas();
    let shade = h.state().scene.bundle.doc.find_layer("shade").unwrap().id.clone();
    let shade_path = path_of(&h, &shade);
    h.state_mut().activate(shade_path).unwrap();
    canvas_shows_the_committed_picture(&mut h);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn switching_layers_keeps_the_canvas_unless_the_picture_changes() {
    let path = sample("switch");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    h.state_mut().settings.developer = true;
    settle(&mut h, |ed| ed.canvas_current() && ed.dirty.is_empty());
    let all = h.state().scene.all_tiles().len();
    let switch = |h: &mut Harness<'static, Editor>, name: &str| {
        let path = {
            let ed = h.state();
            let id = ed.scene.bundle.doc.find_layer(name).unwrap().id.clone();
            match ed.scene.bundle.doc.locate(&id) {
                Some(klad_core::doc::Loc::Layer(path)) => path,
                _ => panic!("no layer {name}"),
            }
        };
        h.state_mut().invoke(Cmd::ResetNumbers).unwrap();
        h.state_mut().activate(path).unwrap();
        let start = std::time::Instant::now();
        while !h.state().canvas_current() && start.elapsed() < std::time::Duration::from_secs(20) {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        h.run_steps(2);
        assert!(h.state().dirty.is_empty());
        crate::stats::lock(&h.state().stats).tiles.max() as usize
    };
    assert_eq!(switch(&mut h, "shade"), 0, "a plain switch keeps every tile");
    h.state_mut().invoke(Cmd::Onion).unwrap();
    settle(&mut h, |ed| ed.canvas_current() && ed.dirty.is_empty());
    assert_eq!(
        switch(&mut h, "rim"),
        all,
        "with onion skins the picture follows the layer"
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
fn notes_stay_one_line_inside_the_pane_and_done_ones_clear_together() {
    use klad_core::notes;
    let path = sample("notes-pane");
    let long = "make the lantern glow warmer and push the shadow under the ledge further left, then soften the rim light on the far side of the rock";
    {
        let b = Bundle::open(&path).unwrap();
        (0..3).for_each(|i| {
            notes::add(&b, (40.0 * i as f32, 50.0), 0, &format!("{i} {long}"), "user").unwrap();
        });
        notes::set_done(&b, 2, true).unwrap();
        notes::set_done(&b, 3, true).unwrap();
    }
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    [
        (Pane::Tool, true),
        (Pane::Layers, true),
        (Pane::Inspector, true),
        (Pane::Notes, false),
    ]
    .into_iter()
    .for_each(|(pane, collapsed)| {
        if let Some(p) = h.state_mut().settings.layout.pane_mut(pane) {
            p.collapsed = collapsed;
        }
    });
    h.state_mut().reread_pins();
    h.run_steps(3);
    h.get_by_label_contains("done (2)").click();
    h.run_steps(2);
    let edge = h
        .query_all_by_label(theme::icon::CLOSE)
        .map(|n| n.rect().right())
        .fold(0.0, f32::max);
    assert!(edge <= 1440.0, "the panes' close buttons stay in the window: {edge}");
    let rows: Vec<Rect> = h
        .query_all_by_label_contains("make the lantern glow")
        .chain(h.query_all_by_label("open again"))
        .map(|n| n.rect())
        .collect();
    assert_eq!(rows.len(), 5, "{rows:?}");
    rows.iter().for_each(|r| {
        assert!(
            r.right() <= edge + 0.5,
            "a row runs past the pane: {r:?}, the pane ends at {edge}"
        );
        assert!(r.height() < 40.0, "a note stays one line: {r:?}");
    });
    h.get_by_label("clear all").click();
    h.run_steps(2);
    let left = notes::read(&Bundle::open(&path).unwrap());
    assert_eq!(
        left.iter().map(|n| n.id).collect::<Vec<_>>(),
        [1],
        "only the open note stays"
    );
    assert!(h.query_by_label_contains("done (").is_none());
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
    h.state_mut().settings_section = super::prefs::Section::Keys;
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
fn recolouring_several_strokes_keeps_each_ones_shade() {
    let path = sample("recolour");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r##"{"op":"stroke","layer":"l6","path":"M100 100 L300 100","color":"#2f5a30","size":20}
{"op":"stroke","layer":"l6","path":"M100 200 L300 200","color":"#a8dc84","size":20}"##,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.run_steps(3);
    let ids: Vec<u32> = h
        .state()
        .scene
        .marks_of("c6")
        .iter()
        .rev()
        .take(2)
        .map(|m| m.id())
        .collect();
    h.state_mut().tool = Tool::Strokes;
    h.state_mut().picked = Some(super::Picked {
        cel: "c6".into(),
        ids: ids.clone(),
        cut: None,
        more: Vec::new(),
    });
    h.state_mut().color = klad_core::color::parse_hex("#e3962f").unwrap();
    h.state_mut().invoke(Cmd::Recolour).unwrap();
    h.run_steps(2);
    let after: Vec<[f32; 3]> = ids
        .iter()
        .map(|id| {
            let marks = h.state().scene.bundle.read_marks("l6", "c6").unwrap();
            klad_core::color::to_oklab(marks.iter().find(|m| m.id() == *id).unwrap().color())
        })
        .collect();
    assert!(after[0][0] > after[1][0], "the light stroke stays lighter: {after:?}");
    assert!(after.iter().all(|lab| lab[2] > 0.05), "both turned warm: {after:?}");
    let _ = std::fs::remove_dir_all(&path);
}

fn pen_line(h: &mut Harness<'static, Editor>, a: egui::Pos2, b: egui::Pos2) {
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
}

#[test]
fn an_eraser_drawn_in_the_editor_attaches_to_the_strokes_it_crossed() {
    let path = sample("attach");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "notes");
    let c = h.state().canvas_rect.center();
    pen_line(&mut h, c + vec2(-80.0, 0.0), c + vec2(80.0, 0.0));
    pen_line(&mut h, c + vec2(-80.0, 120.0), c + vec2(80.0, 120.0));
    h.key_press(Key::E);
    h.run_steps(2);
    pen_line(&mut h, c + vec2(0.0, -30.0), c + vec2(0.0, 30.0));
    let marks = h.state().scene.active_marks().to_vec();
    assert_eq!(marks.len(), 3);
    assert_eq!(
        marks[2].as_stroke().unwrap().on,
        vec![marks[0].id()],
        "the eraser holds the line it crossed, not the one below"
    );
    let _ = std::fs::remove_dir_all(&path);
}

fn on_screen(h: &Harness<'static, Editor>, x: f32, y: f32) -> egui::Pos2 {
    let (view, rect) = (h.state().view.unwrap(), h.state().canvas_rect);
    rect.min + view.to_screen(vec2(x, y), rect.size())
}

fn stroke_on(h: &Harness<'static, Editor>, id: u32) -> Option<klad_core::stroke::Stroke> {
    h.state()
        .scene
        .bundle
        .read_marks("l6", "c6")
        .unwrap()
        .into_iter()
        .find(|m| m.id() == id)
        .and_then(|m| m.as_stroke().cloned())
}

#[test]
fn erasers_follow_their_strokes_and_attached_only_and_detach_reach_them() {
    let path = sample("follow");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r##"{"op":"stroke","layer":"l6","path":"M300 330 L480 330","size":20}
{"op":"stroke","layer":"l6","path":"M100 360 L250 360","size":20}
{"op":"stroke","layer":"l6","brush":"eraser","path":"M390 310 L390 350","size":30}"##,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.run_steps(3);
    let ids: Vec<u32> = h.state().scene.marks_of("c6").iter().map(|m| m.id()).collect();
    let (line, other, eraser) = (ids[ids.len() - 3], ids[ids.len() - 2], ids[ids.len() - 1]);
    assert_eq!(stroke_on(&h, eraser).unwrap().on, vec![line]);
    h.state_mut().tool = Tool::Strokes;
    let pick = |h: &mut Harness<'static, Editor>, ids: Vec<u32>| {
        h.state_mut().picked = Some(super::Picked {
            cel: "c6".into(),
            ids,
            cut: None,
            more: Vec::new(),
        });
        h.run_steps(2);
    };
    pick(&mut h, vec![line]);
    assert_eq!(h.state().attached_count(), 1);
    let button = format!("{} 1 attached", Cmd::AttachedOnly.spec().icon);
    assert_eq!(h.query_all_by_label(&button).count(), 1, "the selection bar shows it");
    let before = stroke_on(&h, eraser).unwrap().samples[0].pos();
    h.state_mut().invoke(Cmd::NudgeRight).unwrap();
    h.run_steps(2);
    assert_eq!(
        stroke_on(&h, eraser).unwrap().samples[0].pos().0,
        before.0 + 1.0,
        "the eraser came along"
    );
    h.state_mut().invoke(Cmd::AttachedOnly).unwrap();
    assert_eq!(h.state().picked.as_ref().unwrap().ids, vec![eraser]);
    pick(&mut h, vec![other]);
    assert!(
        h.state().can_run(Cmd::Detach).is_some(),
        "nothing attached to the other line"
    );
    pick(&mut h, vec![line]);
    h.state_mut().invoke(Cmd::Delete).unwrap();
    h.run_steps(2);
    assert!(
        stroke_on(&h, line).is_none() && stroke_on(&h, eraser).is_none(),
        "nothing left behind"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_eraser_has_a_feather_slider_that_stays_and_reaches_its_strokes() {
    let path = sample("feather-eraser");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "notes");
    h.key_press(Key::E);
    h.run_steps(2);
    assert_eq!(h.state().eraser_feather(), Some(0.0), "the library eraser cuts clean");
    assert!(
        h.query_all_by_label_contains("feather").count() > 0,
        "the slider is in the bar"
    );
    h.state_mut().set_eraser_feather(0.9);
    h.key_press(Key::B);
    h.run_steps(2);
    assert!(h.state().eraser_feather().is_none(), "brushes keep theirs under more");
    h.key_press(Key::E);
    h.run_steps(2);
    assert_eq!(
        h.state().eraser_feather(),
        Some(0.9),
        "it stays when the eraser comes back"
    );
    let c = h.state().canvas_rect.center();
    pen_line(&mut h, c + vec2(-40.0, 0.0), c + vec2(40.0, 0.0));
    let marks = h.state().scene.active_marks().to_vec();
    let brush = marks
        .last()
        .and_then(|m| m.as_stroke())
        .map(|s| s.brush.clone())
        .unwrap();
    let def = h.state().scene.bundle.brush(&brush).unwrap().unwrap();
    assert_eq!(def.feather, 0.9, "{brush} is a feathered eraser");
    assert_eq!(def.hardness(), Some(0.85), "{brush} keeps the eraser's hardness");
    assert_eq!(h.state().settings.eraser_feather, Some(0.9), "kept in the settings");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_stroke_its_eraser_wiped_out_cannot_be_clicked_until_detached() {
    let path = sample("wiped");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r##"{"op":"stroke","layer":"l6","path":"M300 330 L320 330","size":6}
{"op":"stroke","layer":"l6","brush":"eraser","path":"M290 330 L330 330","size":60}"##,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.run_steps(3);
    let ids: Vec<u32> = h.state().scene.marks_of("c6").iter().map(|m| m.id()).collect();
    let wiped = ids[ids.len() - 2];
    h.key_press(Key::S);
    h.run_steps(2);
    let at = on_screen(&h, 310.0, 330.0);
    let click = |h: &mut Harness<'static, Editor>| {
        h.state_mut().picked = None;
        click_with(h, at, Modifiers::NONE);
        h.state().picked.as_ref().map(|p| p.ids.clone()).unwrap_or_default()
    };
    assert!(!click(&mut h).contains(&wiped), "a wiped stroke is not picked");
    h.state_mut().invoke(Cmd::SelectAll).unwrap();
    h.state_mut().invoke(Cmd::Detach).unwrap();
    h.run_steps(2);
    assert_eq!(
        click(&mut h),
        vec![wiped],
        "detached, it picks again like any stroke under an eraser"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_checkout_under_the_open_editor_shows_the_drawing_it_brings() {
    let path = sample("checkout");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.run_steps(3);
    let file = path.join("strokes/l6/c6.strokes");
    let before = std::fs::read(&file).unwrap();
    let marks = h.state().scene.marks_of("c6").len();
    Bundle::open(&path)
        .unwrap()
        .apply(&parse_lines(r#"{"op":"stroke","layer":"l6","path":"M100 100 L300 300"}"#).unwrap())
        .unwrap();
    settle(&mut h, |ed| ed.scene.marks_of("c6").len() == marks + 1);
    std::fs::write(&file, &before).unwrap();
    settle(&mut h, |ed| {
        ed.scene.marks_of("c6").len() == marks && !ed.scene.bundle.changed_on_disk()
    });
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
        ed.scene.bundle.read_marks("l6", "c6").unwrap().len() == 3 && !ed.scene.bundle.changed_on_disk()
    });
    h.run_steps(2);
    let first = |h: &mut Harness<'static, Editor>| {
        let rows = h.state_mut().history_rows();
        (rows[0].who.clone(), rows[0].what.clone(), rows[0].undone)
    };
    assert_eq!(first(&mut h), ("agent".into(), "stroke on l6".into(), false));
    assert!(
        h.query_all(By::new().role(Role::Button).label_contains("agent  stroke on lines"))
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
    let draw = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join(format!("klad{}", std::env::consts::EXE_SUFFIX));
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
fn a_box_dragged_from_another_shape_picks_points_and_shift_adds_to_them() {
    let path = sample("point-box");
    let mut bundle = Bundle::open(&path).unwrap();
    let shapes = bundle.doc.find_layer("shapes").unwrap().id.clone();
    bundle
        .apply(
            &parse_lines(&format!(
                r##"{{"op":"add","node":{{"type":"rect","x":0,"y":0,"w":512,"h":384,"fill":"#203040"}},"in":"{shapes}","frame":0}}
{{"op":"add","node":{{"type":"rect","x":200,"y":150,"w":80,"h":60,"fill":"#d0b040"}},"in":"{shapes}","frame":0}}"##
            ))
            .unwrap(),
        )
        .unwrap();
    let (back, small) = {
        let nodes = &bundle.doc.find_layer("shapes").unwrap().cels()[0].nodes;
        (nodes[nodes.len() - 2].id.clone(), nodes[nodes.len() - 1].id.clone())
    };
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "shapes");
    h.key_press(Key::A);
    h.run_steps(2);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let picked = |h: &Harness<'static, Editor>| {
        let ed = h.state();
        let direct = ed.direct.clone().unwrap();
        let anchors = klad_core::pathedit::anchors(&ed.shaped.as_ref().unwrap().subs);
        let ys: Vec<f32> = direct.points.iter().map(|&i| anchors[i].1.at[1]).collect();
        (direct.node, ys)
    };
    click_with(&mut h, at(240.0, 180.0), Modifiers::NONE);
    assert_eq!(picked(&h), (small.clone(), vec![]));
    drag(&mut h, at(190.0, 140.0), at(290.0, 170.0));
    h.run_steps(2);
    assert_eq!(
        picked(&h),
        (small.clone(), vec![150.0, 150.0]),
        "a box from the shape behind"
    );
    h.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    drag(&mut h, at(190.0, 195.0), at(290.0, 225.0));
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    let (node, mut ys) = picked(&h);
    ys.sort_by(f32::total_cmp);
    assert_eq!(
        (node, ys),
        (small.clone(), vec![150.0, 150.0, 210.0, 210.0]),
        "shift adds the box"
    );
    let corner = at(200.0, 150.0);
    drag(&mut h, corner, corner + vec2(0.0, -30.0));
    settle(&mut h, |ed| ed.point_drag.is_none() && ed.path_commit.is_none());
    h.run_steps(2);
    let moved = klad_core::pathedit::anchors(&h.state().shaped.as_ref().unwrap().subs)
        .iter()
        .map(|(_, a)| a.at[1])
        .fold(f32::MAX, f32::min);
    assert!(
        (moved - (150.0 - 30.0 / view.zoom)).abs() < 1.0,
        "all four moved together, top at {moved}"
    );
    click_with(&mut h, at(60.0, 300.0), Modifiers::NONE);
    assert_eq!(picked(&h), (back, vec![]), "a click on the shape behind still picks it");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_shift_box_picks_the_points_of_every_shape_on_the_layer_and_they_move_together() {
    let path = sample("layer-box");
    let mut bundle = Bundle::open(&path).unwrap();
    let shapes = bundle.doc.find_layer("shapes").unwrap().id.clone();
    bundle
        .apply(
            &parse_lines(&format!(
                r##"{{"op":"add","node":{{"type":"rect","x":0,"y":0,"w":512,"h":384,"fill":"#203040"}},"in":"{shapes}","frame":0}}
{{"op":"add","node":{{"type":"rect","x":200,"y":150,"w":80,"h":60,"fill":"#d0b040"}},"in":"{shapes}","frame":0}}
{{"op":"add","node":{{"type":"rect","x":320,"y":150,"w":80,"h":60,"fill":"#40b0d0"}},"in":"{shapes}","frame":0}}"##
            ))
            .unwrap(),
        )
        .unwrap();
    let (left, right) = {
        let nodes = &bundle.doc.find_layer("shapes").unwrap().cels()[0].nodes;
        (nodes[nodes.len() - 2].id.clone(), nodes[nodes.len() - 1].id.clone())
    };
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "shapes");
    h.key_press(Key::A);
    h.run_steps(2);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let chosen = |h: &Harness<'static, Editor>| -> Vec<(String, usize)> {
        let direct = h.state().direct.clone().unwrap();
        let mut all: Vec<(String, usize)> = std::iter::once((direct.node, direct.points.len()))
            .chain(direct.also.into_iter().map(|(id, points)| (id, points.len())))
            .collect();
        all.sort();
        all
    };
    let top = |h: &Harness<'static, Editor>, id: &str| match h.state().scene.bundle.doc.item(id) {
        Some(klad_core::doc::Item::Node(_, _, node)) => klad_core::pathedit::from_shape(&node.shape)
            .unwrap()
            .iter()
            .flat_map(|sub| sub.anchors.iter().map(|a| a.at[1]))
            .fold(f32::MAX, f32::min),
        _ => f32::NAN,
    };
    assert!(h.state().direct.is_none());
    h.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    h.hover_at(at(190.0, 140.0));
    h.run_steps(1);
    h.drag_at(at(190.0, 140.0));
    h.run_steps(1);
    h.hover_at(at(300.0, 160.0));
    h.run_steps(2);
    let shown = h.state().point_box().expect("the box shows with nothing picked");
    assert!((shown.1.x - shown.0.x - 110.0).abs() < 2.0, "{shown:?}");
    h.hover_at(at(410.0, 170.0));
    h.run_steps(1);
    h.drop_at(at(410.0, 170.0));
    h.run_steps(1);
    assert!(h.state().point_box().is_none());
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    let mut both = vec![(left.clone(), 2), (right.clone(), 2)];
    both.sort();
    assert_eq!(chosen(&h), both, "the top corners of both, and none of the background");
    let corner = at(320.0, 150.0);
    drag(&mut h, corner, corner + vec2(0.0, -30.0));
    settle(&mut h, |ed| ed.point_drag.is_none() && ed.path_commit.is_none());
    h.run_steps(2);
    let lifted = 150.0 - 30.0 / view.zoom;
    assert!((top(&h, &left) - lifted).abs() < 1.0, "left top at {}", top(&h, &left));
    assert!(
        (top(&h, &right) - lifted).abs() < 1.0,
        "right top at {}",
        top(&h, &right)
    );
    assert_eq!(chosen(&h), both, "the points stay selected after the move");
    h.state_mut().invoke(Cmd::Undo).unwrap();
    settle(&mut h, |ed| ed.path_commit.is_none());
    h.run_steps(2);
    assert_eq!(
        (top(&h, &left), top(&h, &right)),
        (150.0, 150.0),
        "one undo takes both back"
    );
    let picked = h.state().direct.as_ref().unwrap().node.clone();
    let x = if picked == left { 190.0 } else { 310.0 };
    drag(&mut h, at(x, 195.0), at(x + 100.0, 225.0));
    h.run_steps(2);
    assert_eq!(chosen(&h), vec![(picked, 2)], "a plain box picks on the one shape only");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_shift_box_on_a_paint_layer_bends_several_strokes_at_once() {
    let path = sample("stroke-box");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    h.key_press(Key::A);
    h.run_steps(2);
    let view = h.state().view.unwrap();
    let rect = h.state().canvas_rect;
    let at = |x: f32, y: f32| rect.min + view.to_screen(egui::Vec2::new(x, y), rect.size());
    let starts = |h: &Harness<'static, Editor>| -> Vec<(f32, f32)> {
        h.state()
            .scene
            .bundle
            .read_marks("l6", "c6")
            .unwrap()
            .iter()
            .filter_map(|m| m.as_stroke())
            .map(|stroke| stroke.samples[0].pos())
            .collect()
    };
    let before = starts(&h);
    h.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    drag(&mut h, at(25.0, 25.0), at(75.0, 315.0));
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    let direct = h.state().direct.clone().unwrap();
    assert_eq!(
        (direct.points.len(), direct.also.len()),
        (1, 1),
        "the start of each stroke"
    );
    let first = h.state().shaped.as_ref().unwrap().subs[0].anchors[0].at;
    let start = at(first[0], first[1]);
    drag(&mut h, start, start + vec2(40.0, 0.0));
    settle(&mut h, |ed| ed.point_drag.is_none());
    h.run_steps(2);
    let moved: Vec<f32> = starts(&h).iter().zip(&before).map(|(a, b)| a.0 - b.0).collect();
    assert!(
        moved.iter().all(|by| (by - 40.0 / view.zoom).abs() < 3.0),
        "both starts moved: {moved:?}"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_bends_a_brush_stroke_and_the_stroke_keeps_its_pressure() {
    let path = sample("reshape");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(r#"{"op":"stroke","layer":"l6","brush":"eraser","path":"M60 40 L60 65","size":10}"#).unwrap(),
        )
        .unwrap();
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
    let eraser_y = |h: &Harness<'static, Editor>| {
        let marks = h.state().scene.bundle.read_marks("l6", "c6").unwrap();
        let eraser = marks.last().unwrap().as_stroke().unwrap().clone();
        assert_eq!(eraser.on, vec![1], "the eraser sits on the bent stroke");
        eraser.samples.iter().map(|s| s.pos().1).sum::<f32>() / eraser.samples.len() as f32
    };
    let bent = eraser_y(&h);
    assert!(
        bent - 52.5 > moved * 0.5,
        "the cut near the start moved down with the stroke: {bent} for a start moved {moved}"
    );
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
    let draw = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join(format!("klad{}", std::env::consts::EXE_SUFFIX));
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
    let folded = h.query_by_label(theme::icon::SIDEBAR).is_none();
    let menu = if folded {
        theme::icon::MORE
    } else {
        theme::icon::SIDEBAR
    };
    h.get_by_role_and_label(Role::Button, menu).click();
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
    let (ours, theirs) = (base.join("ours.klad"), base.join("theirs.klad"));
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
fn the_brush_window_feathers_and_frays_a_round_brush() {
    let path = sample("feather-fringe");
    let mut h = harness(editor(&path), vec2(1440.0, 1400.0));
    activate(&mut h, "lines");
    h.state_mut().pick("ink");
    h.key_press(Key::F5);
    h.run_steps(3);
    assert!(h.state().windows.brush);
    ["feather", "fringe"].iter().for_each(|label| {
        let track = h.get_by_role_and_label(Role::Slider, label).rect();
        let at = egui::pos2(track.left() + track.width() * 0.9, track.center().y);
        drag(&mut h, at, at);
        h.run_steps(2);
    });
    let edited = h.state().custom.clone().unwrap();
    assert!(
        edited.feather > 0.7 && edited.fringe > 0.7,
        "{} {}",
        edited.feather,
        edited.fringe
    );
    h.key_press(Key::F5);
    h.run_steps(2);
    let c = h.state().canvas_rect.center();
    pen_line(&mut h, c + vec2(-60.0, 0.0), c + vec2(60.0, 0.0));
    let brush = h
        .state()
        .scene
        .active_marks()
        .last()
        .and_then(|m| m.as_stroke())
        .map(|s| s.brush.clone())
        .unwrap();
    let def = h.state().scene.bundle.brush(&brush).unwrap().unwrap();
    assert_eq!((def.feather, def.fringe), (edited.feather, edited.fringe), "{brush}");
    let _ = std::fs::remove_dir_all(&path);
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
fn notices_stack_in_the_canvas_bottom_left_corner_not_at_the_pointer() {
    let path = sample("notices");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let canvas = h.state().canvas_rect;
    h.hover_at(canvas.center());
    h.run_steps(1);
    h.state_mut().say(crate::messages::Tone::Done, "first notice");
    h.state_mut().say(crate::messages::Tone::Refused, "second notice");
    h.run_steps(2);
    let (first, second) = (
        h.get_by_label("first notice").rect(),
        h.get_by_label("second notice").rect(),
    );
    assert!(
        second.left() - canvas.left() < 60.0 && canvas.bottom() - second.bottom() < 40.0,
        "the newest notice sits in the canvas's bottom left corner: {second:?} in {canvas:?}"
    );
    assert!(
        (first.left() - second.left()).abs() < 1.0 && first.bottom() < second.top(),
        "older ones stack above it: {first:?} {second:?}"
    );
    let _ = std::fs::remove_dir_all(&path);
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
    let shapes_cel = h.state().scene.bundle.doc.find_layer("shapes").unwrap().cels()[0]
        .id
        .clone();
    let drawn = h.state().scene.raster_of(&shapes_cel).cloned().unwrap();
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
    let editing = h.state().scene.raster_of(&shapes_cel).cloned().unwrap();
    assert!(
        !drawn.differs(&editing).is_empty(),
        "the text being edited leaves the layer, so its old words do not show under the typed ones"
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
fn first_steps_tick_as_a_shape_is_drawn_filled_and_handed_to_an_agent() {
    let path = sample("first-steps");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "colour");
    h.state_mut().start_first_steps();
    h.run_steps(2);
    assert!(
        h.query_by_label_contains("Draw a closed shape with the brush")
            .is_some()
    );
    let first = || super::first::FirstSteps {
        open: true,
        ..Default::default()
    };
    assert_eq!(h.state().first, first());
    h.key_press(Key::B);
    h.run_steps(1);
    let ring = [
        (150.0, 120.0),
        (260.0, 110.0),
        (290.0, 220.0),
        (170.0, 250.0),
        (150.0, 120.0),
    ];
    h.hover_at(canvas_at(&h, ring[0].0, ring[0].1));
    h.run_steps(1);
    h.drag_at(canvas_at(&h, ring[0].0, ring[0].1));
    h.run_steps(1);
    ring.iter().skip(1).for_each(|(x, y)| {
        h.hover_at(canvas_at(&h, *x, *y));
        h.run_steps(1);
    });
    h.drop_at(canvas_at(&h, ring[4].0, ring[4].1));
    h.run_steps(3);
    settle(&mut h, |ed| !ed.writing());
    assert_eq!(h.state().scene.active_marks().len(), 1);
    assert!(h.state().first.drew, "a stroke ticks the first step");
    assert!(!h.state().first.filled);
    h.key_press(Key::G);
    h.run_steps(1);
    let inside = canvas_at(&h, 215.0, 175.0);
    h.hover_at(inside);
    h.run_steps(1);
    [true, false].into_iter().for_each(|pressed| {
        h.event(egui::Event::PointerButton {
            pos: inside,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    });
    settle(&mut h, |ed| !ed.writing());
    assert!(h.state().first.filled, "a fill ticks the second step");
    h.get_by_label_contains("copy a request").click();
    h.run_steps(2);
    assert!(h.state().first.handed, "copying the request ticks the third");
    assert!(
        h.query_by_label_contains("Done. File > Close shows the samples")
            .is_some()
    );
    assert!(super::first::prompt(&path).contains(&format!("klad render \"{}\"", path.display())));
    h.state_mut().invoke(Cmd::FirstSteps).unwrap();
    h.run_steps(2);
    assert!(!h.state().first.open, "Help > First steps closes them again");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_menu_bar_reaches_commands_shows_their_state_and_greys_what_cannot_run() {
    let path = sample("menus");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let item = |h: &Harness<'static, Editor>, name: &str| {
        h.query_all_by_label_contains(name)
            .find(|n| n.accesskit_node().role() != Role::Window)
            .unwrap_or_else(|| panic!("no menu item {name}"))
            .rect()
    };
    h.get_by_label("File").click();
    h.run_steps(2);
    assert!(h.query_by_label("Close cmd W").is_some(), "items carry their keys");
    h.get_by_label("Export… cmd E").click();
    h.run_steps(3);
    assert!(h.state().windows.export, "File > Export… opens the export window");
    assert!(h.state().settings.layout.timeline);
    h.get_by_label("Window").click();
    h.run_steps(2);
    h.get_by_label("Timeline").click();
    h.run_steps(3);
    assert!(!h.state().settings.layout.timeline, "Window > Timeline hides it");
    h.get_by_label("Edit").click();
    h.run_steps(2);
    h.hover_at(item(&h, "Strokes").center());
    h.run_steps(3);
    let recolour = h
        .query_all_by_label("Recolour")
        .next()
        .expect("the strokes submenu opens");
    assert!(recolour.accesskit_node().is_disabled(), "nothing selected greys it out");
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.get_by_label("File").click();
    h.run_steps(2);
    h.get_by_label("Close cmd W").click();
    h.run_steps(2);
    assert_eq!(h.state().wants, Some(super::Want::Close));
    let _ = std::fs::remove_dir_all(&path);
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
    let canvas = h.state().canvas_rect;
    drag(&mut h, a, b);
    h.run_steps(2);
    assert_eq!(
        h.state().canvas_rect,
        canvas,
        "picking a bone keeps the canvas where it is"
    );
    h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut h, b, c);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    let bones = |h: &Harness<'static, Editor>| skeleton(h).bones;
    let made = bones(&h);
    assert_eq!(made.len(), 2, "{made:?}");
    assert_eq!(
        (
            skeleton(&h).name.as_str(),
            h.state().scene.active_layer().map(|l| l.name.clone())
        ),
        ("bones", Some("bones".to_string())),
        "the first bone makes a bones layer, which becomes the active one"
    );
    assert_eq!(
        made[1].parent.as_deref(),
        Some("bone 1"),
        "a bone alt dragged from a tail is its child"
    );
    assert!(
        (made[1].tail[0] - 300.0).abs() < 2.0 && (made[1].tail[1] - 200.0).abs() < 2.0,
        "{made:?}"
    );
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
    let weights = h.state().scene.bundle.read_weights(&skeleton(&h).id).unwrap();
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
fn cmd_t_moves_a_whole_layer_in_every_drawing_or_only_this_one() {
    let path = sample("shift");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r#"{"op":"cel","new":"l6","frame":6}
{"op":"stroke","layer":"l6","frame":6,"path":"M100 100 L200 150","size":6}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    let boxes = |h: &Harness<'static, Editor>| -> Vec<[f32; 4]> {
        let bundle = &h.state().scene.bundle;
        bundle
            .doc
            .layer("l6")
            .unwrap()
            .cels()
            .iter()
            .map(|c| {
                bundle
                    .read_marks("l6", &c.id)
                    .unwrap()
                    .iter()
                    .filter_map(klad_core::stroke::Mark::bounds)
                    .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
                    .unwrap()
            })
            .collect()
    };
    let near = |a: [f32; 4], b: [f32; 4], dx: f32, dy: f32| {
        (b[0] - a[0] - dx).abs() < 1.5 && (b[1] - a[1] - dy).abs() < 1.5 && (b[2] - b[0] - (a[2] - a[0])).abs() < 0.5
    };
    let before = boxes(&h);
    assert_eq!(before.len(), 2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.run_steps(2);
    let corners = h
        .state()
        .shifting
        .as_ref()
        .expect("cmd T starts a layer transform")
        .corners;
    assert_eq!(h.state().tool, Tool::Select);
    let centre = (corners[0] + corners[2]) / 2.0;
    let (from, to) = (
        canvas_at(&h, centre.x, centre.y),
        canvas_at(&h, centre.x + 40.0, centre.y + 20.0),
    );
    drag(&mut h, from, to);
    h.run_steps(3);
    let moved = boxes(&h);
    assert!(
        before.iter().zip(&moved).all(|(a, b)| near(*a, *b, 40.0, 20.0)),
        "every drawing moves: {before:?} {moved:?}"
    );
    assert!(h.state().shifting.is_some(), "the transform stays on for the next drag");
    h.state_mut().shifting.as_mut().unwrap().every = false;
    let corners = h.state().shifting.as_ref().unwrap().corners;
    let centre = (corners[0] + corners[2]) / 2.0;
    let (from, to) = (
        canvas_at(&h, centre.x, centre.y),
        canvas_at(&h, centre.x + 30.0, centre.y),
    );
    drag(&mut h, from, to);
    h.run_steps(3);
    let once = boxes(&h);
    assert!(
        near(moved[0], once[0], 30.0, 0.0),
        "the drawing shown moves: {:?} {:?}",
        moved[0],
        once[0]
    );
    assert_eq!(once[1], moved[1], "the other drawing stays");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().shifting.is_none(), "Esc ends it");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn bones_are_drawn_as_a_chain_that_fans_ends_and_takes_back_its_last_bone() {
    let path = sample("chain");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::J);
    h.run_steps(2);
    let bones = |h: &Harness<'static, Editor>| skeleton(h).bones;
    let points = [[40.0, 40.0], [120.0, 80.0], [200.0, 90.0], [260.0, 150.0]];
    let screen: Vec<egui::Pos2> = points.iter().map(|p| canvas_at(&h, p[0], p[1])).collect();
    drag(&mut h, screen[0], screen[1]);
    h.run_steps(2);
    assert!(h.state().bones.chain.is_some(), "letting go starts the next bone");
    click_with(&mut h, screen[2], Modifiers::NONE);
    click_with(&mut h, screen[3], Modifiers::NONE);
    let drawn = bones(&h);
    assert_eq!(drawn.len(), 3, "{drawn:?}");
    assert_eq!(drawn[1].parent.as_deref(), Some("bone 1"));
    assert_eq!(drawn[2].parent.as_deref(), Some("bone 2"));
    assert!(
        drawn.windows(2).all(|w| w[1].head == w[0].tail),
        "each bone starts on the last one's end"
    );
    assert_eq!(
        h.state().bones.selected.as_deref(),
        Some("bone 3"),
        "the new bone is picked"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(bones(&h).len(), 2, "cmd Z takes back the last bone");
    assert_eq!(
        h.state().bones.selected.as_deref(),
        Some("bone 2"),
        "and picks the one the chain goes on from"
    );
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(3);
    assert_eq!(
        h.state().bones.chain.as_ref().and_then(|c| c.parent.clone()).as_deref(),
        Some("bone 3"),
        "redo brings the chain back to the bone it brought back"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    let again = canvas_at(&h, 230.0, 40.0);
    click_with(&mut h, again, Modifiers::NONE);
    let redrawn = bones(&h);
    assert_eq!(redrawn.len(), 3);
    assert_eq!(
        redrawn[2].parent.as_deref(),
        Some("bone 2"),
        "and the chain carries on from the one before"
    );
    assert_eq!(redrawn[2].head, redrawn[1].tail);
    let joint = canvas_at(&h, redrawn[2].tail[0], redrawn[2].tail[1]) + vec2(7.0, 0.0);
    click_with(&mut h, joint, Modifiers::NONE);
    assert!(
        h.state().bones.chain.is_none(),
        "a click on the open end's ring ends the chain"
    );
    assert_eq!(bones(&h).len(), 3);
    let (root, hand) = (canvas_at(&h, 400.0, 300.0), canvas_at(&h, 400.0, 220.0));
    drag(&mut h, root, hand);
    h.run_steps(2);
    let fingers = [[360.0, 160.0], [400.0, 150.0], [440.0, 160.0]];
    fingers.iter().for_each(|f| {
        let at = canvas_at(&h, f[0], f[1]);
        click_with(&mut h, at, Modifiers::SHIFT);
    });
    let fanned = bones(&h);
    let palm = fanned
        .iter()
        .find(|b| b.head == fanned[3].head && b.parent.is_none())
        .unwrap()
        .name
        .clone();
    let spread: Vec<_> = fanned
        .iter()
        .filter(|b| b.parent.as_deref() == Some(palm.as_str()))
        .collect();
    assert_eq!(spread.len(), 3, "shift fans siblings from one joint: {fanned:?}");
    assert!(spread.iter().all(|b| b.head == fanned[3].tail));
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().bones.chain.is_none(), "Esc ends the chain");
    let count = bones(&h).len();
    let empty = canvas_at(&h, 500.0, 60.0);
    click_with(&mut h, empty, Modifiers::NONE);
    assert_eq!(bones(&h).len(), count, "a click with no chain adds nothing");
    let cli = sample("chain-cli");
    Bundle::open(&cli)
        .unwrap()
        .apply(
            &parse_lines(&format!(
                r#"{{"op":"add","layer":{{"id":"sk","kind":"bones","name":"bones"}}}}
{{"op":"bone","layer":"sk","chain":["bone 1","bone 2","bone 3"],"through":{}}}"#,
                serde_json::to_string(&[drawn[0].head, drawn[0].tail, drawn[1].tail, redrawn[2].tail]).unwrap()
            ))
            .unwrap(),
        )
        .unwrap();
    let made = Bundle::open(&cli).unwrap().doc.layer("sk").unwrap().bones.clone();
    assert_eq!(made[..2], redrawn[..2], "the editor's chain is klad bone --chain");
    assert_eq!(made[2], redrawn[2]);
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::remove_dir_all(&cli);
}

#[test]
fn a_bone_renamed_while_its_chain_is_drawn_stays_the_parent_of_the_next() {
    let path = sample("chain-rename");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::J);
    h.run_steps(2);
    let bones = |h: &Harness<'static, Editor>| skeleton(h).bones;
    let (a, b, c) = (
        canvas_at(&h, 40.0, 40.0),
        canvas_at(&h, 120.0, 80.0),
        canvas_at(&h, 200.0, 90.0),
    );
    drag(&mut h, a, b);
    h.run_steps(2);
    h.state_mut().bones.name = "thigh".into();
    drag(&mut h, c, c + vec2(1.0, 1.0));
    h.run_steps(2);
    let made = bones(&h);
    assert_eq!(made[0].name, "thigh", "the name typed in the field is kept: {made:?}");
    assert_eq!(
        made[1].parent.as_deref(),
        Some("thigh"),
        "and the chain carries on from it"
    );
    assert_eq!(made[1].head, made[0].tail);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn return_and_esc_go_to_the_palette_while_a_chain_is_drawn() {
    let path = sample("chain-keys");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::J);
    h.run_steps(2);
    let (a, b) = (canvas_at(&h, 40.0, 40.0), canvas_at(&h, 120.0, 80.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(2);
    assert!(h.state().palette.is_some());
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().palette.is_none(), "Esc closes the palette");
    assert!(h.state().bones.chain.is_some(), "and leaves the chain alone");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_bone_options_stay_one_row_whatever_is_picked() {
    let path = sample("bone-bar");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r#"{"op":"add","layer":{"id":"sk","kind":"bones","name":"bones"}}
{"op":"attach","layer":"l6","skeleton":"sk"}
{"op":"bone","layer":"sk","chain":["thigh","shin"],"through":[[100,60],[100,160],[110,260]]}
{"op":"ik","layer":"sk","add":"shin"}"#,
            )
            .unwrap(),
        )
        .unwrap();
    [vec2(1280.0, 800.0), vec2(1440.0, 900.0), vec2(1512.0, 982.0)]
        .into_iter()
        .for_each(|size| {
            let mut h = harness(editor(&path), size);
            activate(&mut h, "lines");
            h.key_press(Key::J);
            h.run_steps(3);
            let top = h.state().canvas_rect.min.y;
            use super::bones::Mode;
            [Mode::Draw, Mode::Weights, Mode::Pose].into_iter().for_each(|mode| {
                h.state_mut().bones.mode = mode;
                ["thigh", "shin"].into_iter().for_each(|bone| {
                    h.state_mut().pick_bone(Some(bone.into()));
                    h.run_steps(3);
                    assert_eq!(
                        h.state().canvas_rect.min.y,
                        top,
                        "{size:?}: picking {bone} in {mode:?} keeps the canvas where it is"
                    );
                });
            });
        });
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_bones_layer_takes_any_layers_in_the_drawing() {
    let path = sample("bones-layer");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.state_mut().invoke(Cmd::NewBonesLayer).unwrap();
    h.run_steps(2);
    let made = skeleton(&h);
    assert_eq!(
        h.state().scene.active_layer().map(|l| (l.id.clone(), l.kind())),
        Some((made.id.clone(), klad_core::doc::Kind::Bones)),
        "Layer > New bones layer makes one and makes it active"
    );
    h.key_press(Key::J);
    h.run_steps(2);
    assert!(
        h.query_by_label("preview").is_some(),
        "the bone tool says bones are a preview"
    );
    assert!(
        Cmd::NewBonesLayer.spec().about.contains("preview"),
        "and so does Layer > New bones layer"
    );
    let (a, b) = (canvas_at(&h, 60.0, 60.0), canvas_at(&h, 200.0, 120.0));
    drag(&mut h, a, b);
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(skeleton(&h).bones.len(), 1, "the bone goes on the bones layer");
    h.state_mut().pick_bone(Some("bone 1".into()));
    h.run_steps(2);
    ["colour", "lines"].into_iter().enumerate().for_each(|(n, layer)| {
        h.get_by_role_and_label(Role::Button, &format!("layers: {n}")).click();
        h.run_steps(2);
        h.get_by_role_and_label(Role::CheckBox, layer).click();
        h.run_steps(3);
    });
    let doc = &h.state().scene.bundle.doc;
    ["colour", "lines"].into_iter().for_each(|name| {
        let layer = doc.find_layer(name).unwrap();
        assert_eq!(
            layer.rig,
            Some(klad_core::rig::Link {
                skeleton: made.id.clone(),
                bone: Some("bone 1".into())
            }),
            "{name}, a layer next to the bones layer, follows its bone"
        );
    });
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_layer_attached_to_a_bone_moves_with_it_live_in_pose_mode() {
    let path = sample("attach-live");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r#"{"op":"add","layer":{"id":"sk","kind":"bones","name":"bones"}}
{"op":"attach","layer":"l6","skeleton":"sk"}
{"op":"bone","layer":"sk","chain":["thigh","shin"],"through":[[100,60],[100,160],[110,260]]}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::J);
    h.run_steps(2);
    h.state_mut().pick_bone(Some("shin".into()));
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "layers: 0").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "lines (bends)").click();
    h.run_steps(3);
    assert_eq!(
        h.state().scene.bundle.doc.layer("l6").unwrap().follows(),
        Some("shin"),
        "the picked bone's layers menu attaches a layer"
    );
    assert!(h.query_by_label("layers: 1").is_some(), "and counts it");
    h.get_by_label("weights").click();
    h.run_steps(2);
    assert!(
        h.query_by_label_contains("nothing bends by weights").is_some(),
        "with every layer attached, weights mode says there is nothing to bend"
    );
    h.get_by_label("pose").click();
    h.run_steps(2);
    let rev = h.state().scene.bundle.doc.rev;
    h.state_mut().bones.pose.insert("shin".into(), 90.0);
    let start = std::time::Instant::now();
    while !h
        .state()
        .preview
        .as_ref()
        .is_some_and(|p| p.posed.is_some() && p.job.is_none() && p.done.is_some())
        && start.elapsed() < std::time::Duration::from_secs(20)
    {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let preview = h.state().preview.as_ref().expect("posing shows the drawing live");
    assert!(preview.done.is_some(), "the posed drawing was drawn");
    let live = h.state().bones.live.clone().expect("the live pose");
    let klad_core::ops::Op::Pose(spec) = &live.ops[0] else {
        panic!("a pose op");
    };
    let made = &spec.resolved.as_ref().unwrap()[0];
    let line = made.marks.iter().find_map(|m| m.as_stroke()).unwrap();
    let first = line.samples[0].pos();
    assert!(
        (first.0 - 220.0).abs() < 0.5 && (first.1 - 100.0).abs() < 0.5,
        "the whole layer turns a quarter round the shin's head: {first:?}"
    );
    assert_eq!(
        h.state().scene.bundle.doc.rev,
        rev,
        "showing a pose changes nothing in the drawing"
    );
    h.get_by_label("draw").click();
    h.run_steps(3);
    assert!(
        h.state().preview.is_none(),
        "leaving pose mode shows the drawing as it is"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_leg_gets_its_ik_in_the_options_and_its_foot_is_dragged_and_planted_in_pose_mode() {
    let path = sample("leg-ik");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r#"{"op":"add","layer":{"id":"sk","kind":"bones","name":"bones"}}
{"op":"attach","layer":"l6","skeleton":"sk"}
{"op":"bone","layer":"sk","chain":["thigh","shin"],"through":[[100,60],[100,160],[110,260]]}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    activate(&mut h, "lines");
    h.key_press(Key::J);
    h.run_steps(2);
    h.state_mut().pick_bone(Some("shin".into()));
    h.run_steps(2);
    h.get_by_label("add IK").click();
    h.run_steps(2);
    h.get_by_label("limb: shin and thigh").click();
    h.run_steps(3);
    let ik = h.state().scene.bundle.doc.layer("sk").unwrap().ik.clone();
    assert_eq!(ik.len(), 1, "{ik:?}");
    assert_eq!((ik[0].bone.as_str(), ik[0].chain), ("shin", 2));
    assert!(
        h.query_by_label_contains("IK: limb").is_some(),
        "the options show the bone's IK"
    );
    h.get_by_label("pose").click();
    h.run_steps(2);
    let shown = |h: &Harness<'static, Editor>| {
        let ed = h.state();
        let layer = ed.scene.bundle.doc.layer("sk").unwrap();
        let pose = klad_core::rig::Pose {
            angles: ed.bones.pose.clone(),
            root: ed.bones.root,
            ik: ed.bones.reach.clone(),
            ..Default::default()
        };
        klad_core::rig::tips(&layer.bones, &layer.ik, &pose)
    };
    let canvas = h.state().canvas_rect;
    let (foot, there) = (canvas_at(&h, 110.0, 260.0), canvas_at(&h, 170.0, 220.0));
    drag(&mut h, foot, there);
    h.run_steps(2);
    assert_eq!(
        h.state().canvas_rect,
        canvas,
        "picking a limb keeps the canvas where it is"
    );
    let target = h.state().bones.reach.get("shin").map(|r| r.target).unwrap();
    assert!(
        (target[0] - 170.0).abs() < 1.5 && (target[1] - 220.0).abs() < 1.5,
        "{target:?}"
    );
    let end = shown(&h)[1].1;
    assert!(
        (end[0] - target[0]).abs() < 0.01 && (end[1] - target[1]).abs() < 0.01,
        "the foot reaches: {end:?}"
    );
    let (hip, moved) = (canvas_at(&h, 100.0, 60.0), canvas_at(&h, 130.0, 70.0));
    drag(&mut h, hip, moved);
    h.run_steps(2);
    let root = h.state().bones.root;
    assert!(
        (root[0] - 30.0).abs() < 1.5 && (root[1] - 10.0).abs() < 1.5,
        "the root's head moves it all: {root:?}"
    );
    h.get_by_label("draw").click();
    h.run_steps(2);
    h.get_by_label("pose").click();
    h.run_steps(2);
    assert!(
        h.state().bones.reach.contains_key("shin") && h.state().bones.root == root,
        "a trip to draw mode keeps the pose being built"
    );
    let planted = shown(&h)[1].1;
    assert!(
        (planted[0] - target[0]).abs() < 0.01 && (planted[1] - target[1]).abs() < 0.01,
        "the foot stays planted: {planted:?}"
    );
    let knee = shown(&h)[0].1;
    let (thigh, swung) = (
        canvas_at(&h, (130.0 + knee[0]) / 2.0, (70.0 + knee[1]) / 2.0),
        canvas_at(&h, 60.0, 140.0),
    );
    h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut h, thigh, swung);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    assert!(
        h.state().bones.reach.is_empty(),
        "alt drag takes the limb off IK and turns it by hand"
    );
    let args = super::bones::pose_args(
        &h.state().scene.bundle.root,
        "l6",
        0,
        &klad_core::rig::Pose {
            angles: h.state().bones.pose.clone(),
            root: h.state().bones.root,
            ..Default::default()
        },
    );
    assert!(args.iter().any(|a| a == "--root"), "{args:?}");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn bones_are_picked_from_the_list_and_the_canvas_and_edited_in_place() {
    let path = sample("bone-edit");
    Bundle::open(&path)
        .unwrap()
        .apply(
            &parse_lines(
                r#"{"op":"add","layer":{"id":"sk","kind":"bones","name":"bones"}}
{"op":"attach","layer":"l6","skeleton":"sk"}
{"op":"bone","layer":"sk","add":"thigh","head":[100,60],"tail":[100,160]}
{"op":"bone","layer":"sk","add":"shin","parent":"thigh","head":[100,160],"tail":[100,260]}
{"op":"bone","layer":"sk","add":"tail","head":[300,100],"tail":[400,100]}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    let bones = |h: &Harness<'static, Editor>| skeleton(h).bones;
    let bone = |h: &Harness<'static, Editor>, name: &str| bones(h).into_iter().find(|b| b.name == name).unwrap();
    activate(&mut h, "colour");
    h.key_press(Key::J);
    h.run_steps(2);
    assert_eq!(
        h.state().rig_owner().as_deref(),
        Some("sk"),
        "with any layer active, the bone tool works on the drawing's bones layer"
    );
    h.get_by_label_contains("pick a bone").click();
    h.run_steps(2);
    h.get_by_label("shin").click();
    h.run_steps(2);
    assert_eq!(
        h.state().bones.selected.as_deref(),
        Some("shin"),
        "picked from the list"
    );
    h.get_by_label("weights").click();
    h.run_steps(2);
    let tail = canvas_at(&h, 350.0, 100.0);
    h.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    h.hover_at(tail);
    h.run_steps(1);
    [true, false].into_iter().for_each(|pressed| {
        h.event(egui::Event::PointerButton {
            pos: tail,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::COMMAND,
        });
        h.run_steps(1);
    });
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    assert_eq!(
        h.state().bones.selected.as_deref(),
        Some("tail"),
        "cmd click picks in weights mode"
    );
    assert!(
        h.state()
            .scene
            .bundle
            .read_weights("l6")
            .map_or(true, |w| !w.maps.contains_key("tail")),
        "a cmd click paints no weight"
    );
    h.get_by_label("draw").click();
    h.run_steps(2);
    let (knee, there) = (canvas_at(&h, 100.0, 160.0), canvas_at(&h, 140.0, 170.0));
    drag(&mut h, knee, there);
    h.run_steps(2);
    let (thigh, shin) = (bone(&h, "thigh"), bone(&h, "shin"));
    assert!(
        (thigh.tail[0] - 140.0).abs() < 1.5 && (thigh.tail[1] - 170.0).abs() < 1.5,
        "{thigh:?}"
    );
    assert_eq!(shin.head, thigh.tail, "the knee moves the shin's head along");
    assert_eq!(shin.tail, [100.0, 260.0]);
    assert_eq!(
        h.state().bones.selected.as_deref(),
        Some("thigh"),
        "pressing a bone picks it"
    );
    let (middle, along) = (canvas_at(&h, 350.0, 100.0), canvas_at(&h, 350.0, 140.0));
    drag(&mut h, middle, along);
    h.run_steps(2);
    let moved = bone(&h, "tail");
    assert!(
        (moved.head[1] - 140.0).abs() < 1.5 && (moved.tail[1] - 140.0).abs() < 1.5,
        "{moved:?}"
    );
    h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    let (foot, toe) = (canvas_at(&h, 100.0, 260.0), canvas_at(&h, 160.0, 280.0));
    drag(&mut h, foot, toe);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    let foot = bones(&h).last().unwrap().clone();
    assert_eq!(
        foot.parent.as_deref(),
        Some("shin"),
        "alt drag from a tail makes a child"
    );
    assert_eq!(foot.head, [100.0, 260.0]);
    h.state_mut().pick_bone(Some("tail".into()));
    h.run_steps(2);
    h.get_by_label("parent").click();
    h.run_steps(2);
    h.get_by_label("thigh").click();
    h.run_steps(2);
    assert_eq!(
        bone(&h, "tail").parent.as_deref(),
        Some("thigh"),
        "the parent menu reparents"
    );
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
fn a_forced_error_reads_as_a_sentence_and_lands_in_the_report_with_the_drawing() {
    let path = sample("report");
    let log = path.with_extension("log");
    crate::report::log_to(Some(log.clone()));
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut()
        .check(|_| Err(crate::report::bug("a test forced this").context("the frames could not be prepared")));
    h.run_steps(2);
    let said = h.state().messages.log().last().unwrap().text.clone();
    assert!(
        said.starts_with("something went wrong inside klad (the frames could not be prepared: a test forced this)")
            && said.ends_with("Help > Report a problem sends it to hello@klad.studio or to klad's issues"),
        "{said}"
    );
    assert!(
        crate::report::last_lines(1)[0].ends_with(&format!("editor: {said}")),
        "errors go to klad's log"
    );
    h.state_mut().invoke(Cmd::ReportProblem).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("klad sends nothing by itself").is_some());
    h.get_by_label_contains("include the drawing").click();
    h.run_steps(1);
    h.get_by_label_contains("copy the report").click();
    h.run_steps(2);
    let zip = path.with_file_name(format!(
        "{} for a report.zip",
        path.file_stem().unwrap().to_string_lossy()
    ));
    assert!(zip.is_file(), "the drawing is zipped next to it");
    h.get_by_label("email it to support").click();
    h.run_steps(2);
    let mail = h.state().opened.last().cloned().unwrap_or_default();
    assert!(
        mail.starts_with("mailto:hello@klad.studio?subject=klad%20") && mail.contains("a%20test%20forced%20this"),
        "the report goes into an email to support: {mail}"
    );
    let names = crate::report::unzip_names(&std::fs::read(&zip).unwrap());
    assert!(names.iter().any(|(n, ok)| n.ends_with("/doc.json") && *ok));
    let report = h.state().problem_report(Some(&zip));
    assert!(report.starts_with(&format!("klad {}, from the editor", crate::VERSION)));
    assert!(report.contains(&crate::report::system()));
    assert!(report.contains("a test forced this") && report.contains("Last lines of klad's log:"));
    assert!(report.contains(&format!(
        "is attached as {}",
        zip.file_name().unwrap().to_string_lossy()
    )));
    assert!(
        !report.contains(&path.display().to_string()),
        "the drawing's path stays private"
    );
    assert!(h.state().messages.log().last().unwrap().text.contains("attach"));
    crate::report::log_to(None);
    let _ = std::fs::remove_file(&zip);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn check_now_in_about_says_what_the_releases_feed_holds() {
    let path = sample("check-now");
    let mut editor = editor(&path);
    let feed = path.with_extension("feed.json");
    std::fs::write(
        &feed,
        r#"{"tag_name":"v9.9.9","html_url":"https://github.com/klad-app/klad/releases","body":"- a test"}"#,
    )
    .unwrap();
    editor.update_feed = feed.display().to_string();
    editor.windows.about = true;
    let mut h = harness(editor, vec2(1440.0, 1000.0));
    h.run_steps(2);
    h.get_by_label("check now").click();
    let start = std::time::Instant::now();
    while h.query_by_label("klad 9.9.9 is out: a test").is_none() && start.elapsed().as_secs() < 10 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.query_by_label("klad 9.9.9 is out: a test").is_some());
    assert!(h.state().settings.updates_checked > 0);
    std::fs::write(&feed, "not json").unwrap();
    h.get_by_label("check now").click();
    let start = std::time::Instant::now();
    while h
        .query_by_label("could not check: the releases feed did not make sense")
        .is_none()
        && start.elapsed().as_secs() < 10
    {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        h.query_by_label("could not check: the releases feed did not make sense")
            .is_some()
    );
    let _ = std::fs::remove_file(&feed);
}

type Moment = fn(&mut Editor) -> anyhow::Result<()>;

#[test]
#[ignore]
fn guide_pictures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let samples = std::env::var("KLAD_SAMPLES").map_or_else(|_| root.join("samples"), PathBuf::from);
    let out = root.join("scratch/ui/guide");
    std::fs::create_dir_all(&out).unwrap();
    let moments: [(&str, &str, &str, Moment); 5] = [
        ("painting", "lemons/lemons.klad", "lemon-a", |ed| {
            ed.invoke(Cmd::Brush)?;
            ed.pick("oil");
            Ok(())
        }),
        ("animation", "creature/creature.klad", "lines", |ed| {
            ed.go_to(16)?;
            ed.invoke(Cmd::Onion)
        }),
        ("bones", "rig/rig.klad", "bones", |ed| {
            ed.invoke(Cmd::BoneTool)?;
            ed.pick_bone(Some("fore".into()));
            Ok(())
        }),
        ("brush", "lemons/lemons.klad", "lemon-a", |ed| {
            ed.pick("oil");
            ed.invoke(Cmd::BrushWindow)
        }),
        ("selection", "lantern/lantern.klad", "lines", |ed| {
            ed.tool = Tool::Strokes;
            ed.select_all_strokes();
            Ok(())
        }),
    ];
    moments.iter().for_each(|(name, sample, layer, moment)| {
        let source = std::fs::canonicalize(samples.join(sample))
            .expect("run just samples first, or point KLAD_SAMPLES at built samples");
        editor_picture(&source, layer, *moment, &out.join(format!("{name}.png")));
    });
}

fn editor_picture(source: &Path, layer: &str, moment: Moment, out: &Path) {
    editor_pictures(source, layer, moment, &[(None, out.to_path_buf())]);
}

fn editor_pictures(source: &Path, layer: &str, moment: Moment, shots: &[(Option<u32>, PathBuf)]) {
    let name = shots[0].1.file_stem().unwrap().to_string_lossy().to_string();
    let dir = std::env::temp_dir().join(format!("draw-guide-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(source.file_name().unwrap());
    let copied = std::process::Command::new("cp")
        .arg("-R")
        .arg(source)
        .arg(&path)
        .status()
        .unwrap();
    assert!(copied.success());
    let gpu = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let mut settings = Settings::default();
    settings.layout.timeline_height = 210.0;
    let mut first = crate::home::Home::with(settings, None, Some(gpu.clone()));
    first.open(&egui::Context::default(), &path).unwrap();
    let mut h = Harness::builder()
        .with_size(vec2(1600.0, 1000.0))
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(gpu))
        .build_ui_state(|ui, home: &mut crate::home::Home| home.frame(ui), first);
    let settle = |h: &mut Harness<'static, crate::home::Home>| {
        let started = std::time::Instant::now();
        while !h.state().editor.as_ref().unwrap().canvas_current() && started.elapsed().as_secs() < 180 {
            h.step();
            h.render().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        h.state_mut().editor.as_mut().unwrap().redraw_canvas();
        (0..8).for_each(|_| {
            h.step();
            h.render().unwrap();
        });
    };
    settle(&mut h);
    let ed = h.state_mut().editor.as_mut().unwrap();
    let id = ed.scene.bundle.doc.find_layer(layer).unwrap().id.clone();
    if let Some(klad_core::doc::Loc::Layer(at)) = ed.scene.bundle.doc.locate(&id) {
        ed.activate(at).unwrap();
    }
    moment(ed).unwrap();
    settle(&mut h);
    shots.iter().for_each(|(frame, out)| {
        if let Some(frame) = frame {
            h.state_mut().editor.as_mut().unwrap().go_to(*frame).unwrap();
            settle(&mut h);
        }
        h.render().unwrap().save(out).unwrap();
    });
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[ignore]
fn a_picture_of_the_editor() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let (bundle, layer, out) = (
        var("KLAD_PICTURE_BUNDLE"),
        var("KLAD_PICTURE_LAYER"),
        var("KLAD_PICTURE_OUT"),
    );
    let moment: Moment = |ed| {
        ed.invoke(Cmd::Brush)?;
        if let Ok(brush) = std::env::var("KLAD_PICTURE_BRUSH") {
            ed.pick(&brush);
        }
        let numbers = |name: &str| -> Option<Vec<f32>> {
            let text = std::env::var(name).ok()?;
            text.split(',').map(|n| n.trim().parse().ok()).collect()
        };
        if let Some(view) = ed.view.as_mut() {
            if let Some(&[zoom]) = numbers("KLAD_PICTURE_ZOOM").as_deref() {
                view.zoom *= zoom;
            }
            if let Some(&[x, y]) = numbers("KLAD_PICTURE_CENTRE").as_deref() {
                view.center = vec2(x, y);
            }
        }
        Ok(())
    };
    let frames = std::env::var("KLAD_PICTURE_FRAMES").ok().map(|spec| {
        let (from, to) = spec.split_once('-').expect("KLAD_PICTURE_FRAMES is a range like 0-40");
        from.parse::<u32>().unwrap()..=to.parse::<u32>().unwrap()
    });
    let shots: Vec<(Option<u32>, PathBuf)> = match frames {
        None => vec![(None, PathBuf::from(&out))],
        Some(range) => {
            std::fs::create_dir_all(&out).unwrap();
            range
                .map(|frame| (Some(frame), Path::new(&out).join(format!("{frame:02}.png"))))
                .collect()
        }
    };
    editor_pictures(&std::fs::canonicalize(bundle).unwrap(), &layer, moment, &shots);
}

#[test]
fn help_reaches_support_by_email_and_about_links_the_site() {
    let path = sample("support");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    h.state_mut().invoke(Cmd::ContactSupport).unwrap();
    h.run_steps(1);
    let mail = h.state().opened.last().cloned().unwrap_or_default();
    let encoded = |text: &str| {
        klad_core::contact::mailto("", text)
            .split("body=")
            .nth(1)
            .unwrap_or_default()
            .to_string()
    };
    assert!(mail.starts_with("mailto:hello@klad.studio?subject=klad%20"), "{mail}");
    assert!(
        mail.contains(&encoded(crate::VERSION)) && mail.contains(&encoded(&crate::report::system())),
        "the email holds the version and the system: {mail}"
    );
    h.state_mut().invoke(Cmd::About).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("klad.studio").is_some(), "About links the website");
    assert!(
        h.query_by_label("hello@klad.studio").is_some(),
        "and the support address"
    );
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn help_opens_the_guides_and_whats_new() {
    let path = sample("help-links");
    let mut h = harness(editor(&path), vec2(1440.0, 1000.0));
    [
        (Cmd::Guides, "https://klad.studio/docs"),
        (Cmd::WhatsNew, "https://klad.studio/docs/changelog"),
    ]
    .into_iter()
    .for_each(|(cmd, url)| {
        h.state_mut().invoke(cmd).unwrap();
        h.run_steps(1);
        assert_eq!(h.state().opened.last().map(String::as_str), Some(url), "{cmd:?}");
    });
    let help = crate::commands::MENUS.iter().find(|m| m.title == "Help").unwrap();
    [Cmd::FirstSteps, Cmd::Guides, Cmd::WhatsNew, Cmd::ContactSupport]
        .into_iter()
        .for_each(|cmd| {
            assert!(
                help.items
                    .iter()
                    .any(|i| matches!(i, crate::commands::Item::Run(c) if *c == cmd)),
                "Help has {cmd:?}"
            );
        });
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn internal_words_are_recognised() {
    ["c12", "l6", "n357", "s4", "M14", "S14.5", "M4.5"]
        .into_iter()
        .for_each(|w| assert!(internal(w), "{w}"));
    ["layer", "12", "2s", "s", "c", "1.000", "klad", "S", "Esc"]
        .into_iter()
        .for_each(|w| assert!(!internal(w), "{w}"));
}

#[test]
fn no_tool_shows_internal_ids_or_story_numbers() {
    let path = sample("no-ids");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    activate(&mut h, "lines");
    h.run_steps(3);
    scan(&h, "the default layout");
    [
        Tool::Brush,
        Tool::Fill,
        Tool::Select,
        Tool::Strokes,
        Tool::Direct,
        Tool::Pen,
        Tool::Shape,
        Tool::Text,
        Tool::Note,
        Tool::Bone,
    ]
    .into_iter()
    .for_each(|tool| {
        h.state_mut().tool = tool;
        h.run_steps(2);
        scan(&h, &format!("{tool:?}"));
    });
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn the_numbers_need_developer_tools_and_notes_start_folded() {
    let path = sample("developer");
    let mut h = harness(editor(&path), vec2(1440.0, 900.0));
    let fresh = crate::layout::Layout::default();
    [Pane::Notes, Pane::History].into_iter().for_each(|pane| {
        assert!(
            fresh.panes.iter().any(|p| p.pane == pane && p.collapsed),
            "{pane:?} starts folded"
        );
    });
    assert!(!h.state().settings.developer, "developer tools start off");
    h.state_mut().invoke(Cmd::Numbers).unwrap();
    h.run_steps(2);
    assert!(
        !h.state().settings.layout.numbers,
        "the numbers stay closed without developer tools"
    );
    assert!(
        h.query_by_label(theme::icon::NUMBERS).is_none(),
        "and the strip has no numbers button"
    );
    h.state_mut().settings.developer = true;
    h.state_mut().invoke(Cmd::Numbers).unwrap();
    h.run_steps(2);
    assert!(h.state().settings.layout.numbers, "with developer tools on they open");
    let _ = std::fs::remove_dir_all(&path);
}
