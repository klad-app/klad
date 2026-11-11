use std::process::Command;

#[test]
fn the_binary_runs() {
    let out = Command::new(env!("CARGO_BIN_EXE_klad"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("klad"));
}

#[test]
fn skill_prints_the_klad_skill_and_the_drawing_skill() {
    let skill = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .arg("skill")
            .args(args)
            .output()
            .unwrap()
    };
    let said = |args: &[&str]| String::from_utf8_lossy(&skill(args).stdout).to_string();
    assert!(said(&[]).starts_with("---\nname: klad\n"));
    assert_eq!(said(&["klad"]), said(&[]));
    let drawing = said(&["drawing"]);
    assert!(drawing.starts_with("---\nname: klad-drawing\n"));
    assert!(drawing.contains("## The order of work"));
    let unknown = skill(&["painting"]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no skill named painting"));
}

#[test]
fn a_drawing_moves_and_a_layer_duplicates_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let path = bundle.to_str().unwrap();
    draw(&["new", path, "--width", "200", "--height", "100", "--frames", "12"]);
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 90", "--size", "4"]);
    draw(&["cel", path, "dup", "c2", "--frame", "8", "--hold", "2"]);
    draw(&["cel", path, "move", "c2", "--frame", "2"]);
    assert!(draw(&["duplicate", path, "paint 1"]).contains("added"));
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(bundle.join("doc.json")).unwrap()).unwrap();
    let layers = doc["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1]["name"], "paint 1 copy");
    let starts = |l: &serde_json::Value| -> Vec<u64> {
        l["cels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["frame"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(starts(&layers[0]), [2, 10]);
    assert_eq!(starts(&layers[1]), [2, 10]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_error_is_one_line_and_the_backtrace_waits_for_klad_backtrace() {
    let missing = std::env::temp_dir().join(format!("draw-binary-missing-{}.klad", std::process::id()));
    let run = |backtrace: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_klad"));
        command
            .args(["ls", missing.to_str().unwrap()])
            .env("RUST_BACKTRACE", "1");
        match backtrace {
            Some(v) => command.env("KLAD_BACKTRACE", v),
            None => command.env_remove("KLAD_BACKTRACE"),
        };
        let out = command.output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let plain = run(None);
    assert_eq!(plain.lines().count(), 1, "{plain}");
    assert!(
        plain.starts_with("error: reading ") && plain.contains("doc.json: "),
        "{plain}"
    );
    assert_eq!(run(Some("0")), plain);
    let traced = run(Some("1"));
    assert!(traced.contains("Caused by") && traced.contains("backtrace"), "{traced}");
}

#[test]
fn the_client_makes_edits_reads_and_renders_a_drawing() {
    use klad_client::{Error, Klad, json};
    let dir = std::env::temp_dir().join(format!("draw-binary-client-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let klad = Klad::at(env!("CARGO_BIN_EXE_klad"));
    assert!(klad.version().unwrap().starts_with("klad"));
    let art = klad.drawing(dir.join("art.klad"));
    art.create(320, 200, Some("#e9e4da")).unwrap();
    let ids = art
        .apply([
            json!({"op": "add", "layer": {"kind": "vector", "name": "sky"}}),
            json!({"op": "stroke", "layer": "l1", "brush": "ink", "size": 6, "color": "#1b1b1b", "path": "M20 100 L300 100"}),
        ])
        .unwrap();
    assert!(!ids.is_empty());
    let layer = art.document().unwrap()["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "sky")
        .map(|l| l["id"].as_str().unwrap().to_string())
        .unwrap();
    art.apply([json!({"op": "add", "node": {"type": "rect", "x": 10, "y": 10, "w": 50, "h": 30, "fill": "#336699"}, "in": layer, "frame": 0})])
        .unwrap();
    assert!(art.ls(0).unwrap().to_string().contains("sky"));
    let strokes = art.strokes("l1", 0).unwrap();
    assert!(strokes.to_string().contains("ink"));
    let png = art.render(&dir.join("art.png"), 0, 0.5).unwrap();
    assert!(std::fs::metadata(png).unwrap().len() > 0);
    art.undo().unwrap();
    assert!(!art.document().unwrap().to_string().contains("#336699"));
    let missing = art.show("n999").unwrap_err();
    assert!(matches!(missing, Error::Failed { .. }), "{missing}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn updates_says_what_the_feed_holds() {
    let dir = std::env::temp_dir().join(format!("draw-binary-updates-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let feed = dir.join("latest.json");
    let run = |feed: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .arg("updates")
            .env("KLAD_UPDATE_FEED", feed)
            .output()
            .unwrap()
    };
    std::fs::write(
        &feed,
        r#"{"tag_name":"v9.9.9","html_url":"https://github.com/klad-app/klad/releases","body":"- a test"}"#,
    )
    .unwrap();
    let out = run(&feed);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "klad 9.9.9 is out: a test\nhttps://github.com/klad-app/klad/releases\n"
    );
    std::fs::write(&feed, r#"{"tag_name":"v0.0.1","html_url":"https://example.org"}"#).unwrap();
    assert!(String::from_utf8_lossy(&run(&feed).stdout).contains("is the newest version"));
    let missing = run(&dir.join("missing.json"));
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).starts_with("error: could not check: reading"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn samples_are_listed_and_copied_to_draw_on() {
    let dir = std::env::temp_dir().join(format!("draw-binary-samples-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let shipped = dir.join("shipped");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .env("KLAD_SAMPLES", &shipped)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    std::fs::create_dir_all(&shipped).unwrap();
    let rock = shipped.join("rock/rock.klad");
    let out = run(&["new", rock.to_str().unwrap(), "--width", "64", "--height", "64"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::write(
        shipped.join("shipped.json"),
        r#"[{"name":"rock","bundle":"rock/rock.klad","about":"a rock on its own"}]"#,
    )
    .unwrap();
    let listed = String::from_utf8_lossy(&run(&["samples"]).stdout).to_string();
    assert!(listed.starts_with("rock       a rock on its own"), "{listed}");
    let copied = run(&["samples", "--copy", "rock"]);
    assert!(copied.status.success(), "{}", String::from_utf8_lossy(&copied.stderr));
    assert!(
        dir.join("rock.klad/doc.json").is_file(),
        "the copy goes in the current folder"
    );
    let into = dir.join("mine");
    let out = run(&["samples", "--copy", "rock", "--into", into.to_str().unwrap()]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("copied rock to {}", into.join("rock.klad").display())
    );
    let wrong = run(&["samples", "--copy", "stone"]);
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("no sample stone; there are rock"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_report_holds_the_version_the_log_with_home_hidden_and_zips_the_drawing() {
    let dir = std::env::temp_dir().join(format!("draw-binary-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let home = dir.join("home");
    let log = dir.join("klad.log");
    std::fs::write(
        &log,
        format!("2026-11-06 17:00:00 editor: could not read {}/x.png\n", home.display()),
    )
    .unwrap();
    let bundle = dir.join("drawings/rock.klad");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_klad"))
            .args(args)
            .env("KLAD_LOG", &log)
            .env("HOME", &home)
            .output()
            .unwrap()
    };
    assert!(
        run(&["new", bundle.to_str().unwrap(), "--width", "64", "--height", "48"])
            .status
            .success()
    );
    let out = run(&["report", "--drawing", bundle.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        text.starts_with("klad ") && text.contains("from the command line"),
        "{text}"
    );
    assert!(
        text.contains("could not read ~/x.png"),
        "the home folder reads as ~: {text}"
    );
    assert!(
        text.contains("The drawing (64 by 48 px, 1 layer, 1 frame) is attached as rock for a report.zip."),
        "{text}"
    );
    assert!(dir.join("drawings/rock for a report.zip").is_file());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn solo_shows_one_layer_and_undo_brings_the_rest_back() {
    let dir = std::env::temp_dir().join(format!("draw-binary-solo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    run(&["new", path, "--width", "64", "--height", "64", "--layers", "3"]);
    let shown = || -> Vec<bool> {
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(bundle.join("doc.json")).unwrap()).unwrap();
        doc["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["visible"].as_bool().unwrap_or(true))
            .collect()
    };
    run(&["solo", path, "paint 2"]);
    assert_eq!(shown(), [false, true, false]);
    assert!(run(&["solo", path, "paint 2"]).contains("already shows alone"));
    run(&["undo", path]);
    assert_eq!(shown(), [true, true, true]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_chain_and_its_ik_are_set_up_and_posed_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("klad-binary-ik-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let klad = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let path = bundle.to_str().unwrap();
    klad(&["new", path, "--width", "400", "--height", "400", "--frames", "6"]);
    klad(&[
        "stroke",
        path,
        "l1",
        "--path",
        "M100 100 L100 200 L100 300",
        "--size",
        "6",
    ]);
    klad(&["add", path, "layer", "bones", "name=bones"]);
    let made = klad(&[
        "bone",
        path,
        "bones",
        "--chain",
        "thigh,shin",
        "--through",
        "100,100 100,200 100,300",
    ]);
    assert!(made.contains("chain thigh, shin"), "{made}");
    let rigged = klad(&["attach", path, "l1", "--bend", "--skeleton", "bones"]);
    assert!(rigged.contains("bends with bones"), "{rigged}");
    let ik = klad(&["ik", path, "bones", "--add", "shin", "--softness", "4"]);
    assert!(ik.contains("a limb with thigh") && ik.contains("softness 4"), "{ik}");
    klad(&["ik", path, "bones", "--set", "shin", "--bend", "flip"]);
    klad(&[
        "pose",
        path,
        "bones",
        "--from",
        "0",
        "--to",
        "3",
        "--target",
        "shin=180,240",
        "--mix",
        "shin=0.5",
        "--root",
        "-10,0",
    ]);
    let listed = klad(&["bones", path]);
    assert!(
        listed.contains("ik shin: a limb with thigh, bending counterclockwise, softness 4 px"),
        "{listed}"
    );
    assert!(
        listed.contains("root -10,0 target shin=180,240 mix 0.5 solved shin="),
        "{listed}"
    );
    let nan = Command::new(env!("CARGO_BIN_EXE_klad"))
        .args([
            "pose",
            path,
            "bones",
            "--from",
            "0",
            "--to",
            "4",
            "--target",
            "shin=nan,240",
        ])
        .output()
        .unwrap();
    assert!(!nan.status.success(), "a target that is not a number is refused");
    klad(&["ls", path]);
    let json: serde_json::Value = serde_json::from_str(&klad(&["bones", path, "--json"])).unwrap();
    assert_eq!(json[0]["ik"][0]["bone"], "shin");
    assert!(json[0]["poses"][0]["solved"]["thigh"].is_number());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_checkout_under_the_cache_renders_the_drawing_it_brings() {
    let dir = std::env::temp_dir().join(format!("draw-binary-checkout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let render = || {
        let printed = draw(&["render", path]);
        std::fs::read(printed.lines().last().unwrap().trim()).unwrap()
    };
    let files = || ["doc.json", "strokes/l1/c2.strokes"].map(|f| std::fs::read(bundle.join(f)).ok());
    let put = |kept: &[Option<Vec<u8>>; 2]| {
        ["doc.json", "strokes/l1/c2.strokes"]
            .iter()
            .zip(kept)
            .for_each(|(f, bytes)| match bytes {
                Some(bytes) => std::fs::write(bundle.join(f), bytes).unwrap(),
                None => {
                    let _ = std::fs::remove_file(bundle.join(f));
                }
            });
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    let base = files();
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 90", "--size", "8"]);
    let one = files();
    let first = render();
    put(&base);
    draw(&["stroke", path, "l1", "--path", "M10 90 L190 10", "--size", "8"]);
    let other = render();
    assert_ne!(
        first, other,
        "the other branch, after as many edits, is not the first one's render"
    );
    put(&one);
    assert_eq!(
        render(),
        first,
        "checking the first branch out again renders it, not the cached other"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_merge_that_left_an_id_used_twice_is_repaired() {
    let dir = std::env::temp_dir().join(format!("draw-binary-repair-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100", "--layers", "2"]);
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 90"]);
    draw(&["stroke", path, "l3", "--path", "M10 90 L190 10"]);
    let doc = std::fs::read_to_string(bundle.join("doc.json")).unwrap();
    std::fs::write(bundle.join("doc.json"), doc.replace("\"l3\"", "\"l1\"")).unwrap();
    std::fs::rename(
        bundle.join("strokes/l3/c4.strokes"),
        bundle.join("strokes/l1/c4.strokes"),
    )
    .unwrap();
    let refused = run(&["info", path]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        !refused.status.success() && said.contains("id l1 is used twice") && said.contains("klad repair"),
        "{said}"
    );
    let repaired = draw(&["repair", path]);
    assert!(repaired.contains("layer \"paint 2\": l1 is now l5"), "{repaired}");
    assert!(
        repaired.contains("moved strokes/l1/c4.strokes to strokes/l5/c4.strokes"),
        "{repaired}"
    );
    let info = draw(&["info", path]);
    assert!(
        info.contains("l1      c2") && info.contains("l5      c4") && info.matches("1 strokes").count() == 2,
        "{info}"
    );
    assert!(draw(&["repair", path]).contains("nothing to repair"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn shapes_align_spread_and_centre_on_the_canvas_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-align-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "400", "--height", "200", "--sketch"]);
    let add = |x: &str, y: &str, w: &str| {
        let made = draw(&[
            "add",
            path,
            "rect",
            "--in",
            "l3",
            &format!("x={x}"),
            &format!("y={y}"),
            &format!("w={w}"),
            "h=20",
        ]);
        made.lines()
            .find_map(|line| line.strip_prefix("added "))
            .unwrap_or_else(|| panic!("an id in: {made}"))
            .to_string()
    };
    let (a, b, c) = (add("10", "10", "20"), add("100", "40", "40"), add("50", "80", "10"));
    let boxes = || {
        [&a, &b, &c].map(|id| {
            let node: serde_json::Value = serde_json::from_str(&draw(&["show", path, id])).unwrap();
            [0, 1].map(|i| node["bounds"][i].as_f64().unwrap())
        })
    };
    draw(&["align", path, &a, &b, &c, "--y", "top", "--spread", "x"]);
    assert_eq!(boxes(), [[10.0, 10.0], [100.0, 10.0], [60.0, 10.0]]);
    draw(&["align", path, &b, "--x", "centre", "--y", "middle", "--to", "canvas"]);
    assert_eq!(boxes()[1], [180.0, 90.0]);
    draw(&["align", path, &b, &c, "--x", "left", "--to", &a]);
    assert_eq!(boxes().map(|at| at[0]), [10.0, 10.0, 10.0]);
    assert!(draw(&["align", path, &b, &c, "--x", "left", "--to", &a]).contains("nothing moved"));
    let log = draw(&["log", path]);
    assert_eq!(
        log.lines().filter(|line| line.contains("set n")).count(),
        3,
        "one history step each: {log}"
    );
    let refused = run(&["align", path, &a, "--x", "left"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("nothing to line up with"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_pixel_drawing_is_made_drawn_listed_and_edited_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-pixel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let json = |args: &[&str]| -> serde_json::Value { serde_json::from_str(&draw(args)).unwrap() };
    draw(&["new", path, "--pixel", "16x12"]);
    let tree = json(&["ls", path, "--json"]);
    assert_eq!(
        (tree["canvas"]["width"].as_u64(), tree["canvas"]["height"].as_u64()),
        (Some(16), Some(12))
    );
    let layer = tree["layers"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(tree["layers"][0]["kind"], "pixel");
    draw(&["palette", path, "--add", "#f0c8a0", "--name", "skin"]);
    let made = draw(&[
        "pixel",
        path,
        &layer,
        "--color",
        "#2b2d42",
        "--cells",
        "3-12,2 3,3-8 12,3-8 3-12,9",
    ]);
    assert!(
        made.contains("pixel marks") && made.contains("changed palette"),
        "{made}"
    );
    draw(&["pixel", path, &layer, "--color", "skin", "--cells", "4-11,3-8"]);
    draw(&["pixel", path, &layer, "--erase", "--cells", "7,5 8,5"]);
    let listed = json(&["strokes", path, &layer, "--json"]);
    let marks = listed["fills"].as_array().unwrap();
    assert_eq!(marks.len(), 3);
    assert_eq!(marks[0]["cells"].as_array().unwrap().len(), 32);
    assert_eq!(marks[1]["cells"].as_array().unwrap().len(), 48);
    assert_eq!(
        (marks[2]["erase"].as_bool(), marks[2]["cells"].as_array().unwrap().len()),
        (Some(true), 2)
    );
    let palette = json(&["palette", path, "--json"]);
    assert_eq!(palette.as_array().unwrap().len(), 2);
    assert_eq!(
        marks[1]["entry"], palette[0]["id"],
        "the mark is bound to the skin entry"
    );
    assert_eq!(marks[0]["entry"], palette[1]["id"]);
    let text = draw(&["strokes", path, &layer]);
    assert!(
        text.contains("3 pixel marks, 82 cells") && text.contains("skin") && text.contains("erases"),
        "{text}"
    );
    let first = marks[0]["address"].as_str().unwrap().to_string();
    draw(&["set", path, &first, "color=skin"]);
    draw(&["transform", path, &first, "--move", "1,0"]);
    let after = json(&["strokes", path, &layer, "--json"]);
    assert_eq!(after["fills"][0]["entry"], palette[0]["id"]);
    assert_eq!(after["fills"][0]["bounds"][0].as_f64(), Some(4.0));
    assert!(draw(&["ls", path]).contains("3 pixel marks"));
    let refused = run(&["stroke", path, &layer, "--path", "M1 1 L9 9"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("pixel layer"));
    let bad = run(&["pixel", path, &layer, "--color", "skin", "--cells", "3;4"]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("is not x,y"));
    let added = draw(&["add", path, "layer", "pixel", "name=lines"]);
    let lines_id = added
        .lines()
        .find_map(|line| line.strip_prefix("added "))
        .unwrap_or_else(|| panic!("an id in: {added}"))
        .to_string();
    draw(&[
        "pixel",
        path,
        "lines",
        "--color",
        "skin",
        "--path",
        "M0.5 11.5 C 4 2 11 2 15.5 11.5",
    ]);
    draw(&[
        "pixel",
        path,
        &lines_id,
        "--color",
        "skin",
        "--path",
        "M1.5 1.5 L5.5 1.5 L5.5 4.5",
    ]);
    draw(&["pixel", path, &lines_id, "--erase", "--size", "3", "--cells", "5,1"]);
    let lines = json(&["strokes", path, &lines_id, "--json"]);
    let cells = |at: usize| -> Vec<(i64, i64)> {
        lines["fills"][at]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c[0].as_i64().unwrap(), c[1].as_i64().unwrap()))
            .collect()
    };
    let curve = cells(0);
    assert!(curve.contains(&(0, 11)) && curve.contains(&(15, 11)), "{curve:?}");
    let doubled = curve.iter().any(|&(x, y)| {
        let has = |dx: i64, dy: i64| curve.contains(&(x + dx, y + dy));
        (has(1, 0) || has(-1, 0))
            && (has(0, 1) || has(0, -1))
            && !(has(1, 0) && has(-1, 0))
            && !(has(0, 1) && has(0, -1))
    });
    assert!(!doubled, "a curve is one pixel thin with no corner doubled: {curve:?}");
    assert_eq!(cells(1).len(), 8, "a path's own corner is kept");
    assert_eq!(cells(2).len(), 9, "a size is a square of whole pixels");
    draw(&["rm", path, &lines_id]);
    let wide = run(&[
        "pixel", path, &layer, "--color", "skin", "--cells", "1,1", "--size", "99",
    ]);
    assert!(String::from_utf8_lossy(&wide.stderr).contains("from 1 to 64"));
    let shown = run(&["render", path]);
    assert!(
        String::from_utf8_lossy(&shown.stderr).contains("512x384"),
        "a pixel drawing renders enlarged by a whole number: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let hash = |out: &std::process::Output| {
        String::from_utf8_lossy(&out.stderr)
            .split_whitespace()
            .skip_while(|word| *word != "hash")
            .nth(1)
            .map(String::from)
    };
    let gridded = run(&["render", path, "--overlay", "grid"]);
    assert!(
        gridded.status.success() && hash(&gridded) != hash(&shown),
        "the grid overlay draws its lines"
    );
    let real = run(&["render", path, "--scale", "1"]);
    assert!(String::from_utf8_lossy(&real.stderr).contains("16x12"));
    draw(&["add", path, "layer", "pixel", "name=shade"]);
    assert_eq!(json(&["ls", path, "--json"])["layers"][0]["kind"], "pixel");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_palette_recolours_its_pixels_loads_palettes_and_groups_ramps_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-palette-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let json = |args: &[&str]| -> serde_json::Value { serde_json::from_str(&draw(args)).unwrap() };
    draw(&["new", path, "--pixel", "8"]);
    draw(&["palette", path, "--add", "#f0c8a0", "--name", "skin"]);
    draw(&["pixel", path, "l1", "--color", "skin", "--cells", "1-3,1-3"]);
    draw(&["palette", path, "--recolour", "skin", "--to", "#e8b796"]);
    let marks = json(&["strokes", path, "l1", "--json"]);
    assert_eq!(
        marks["fills"][0]["color"], "#e8b796ff",
        "the pixels follow their palette colour"
    );
    draw(&["palette", path, "--ramp", "skin", "--base", "#e8b796", "--steps", "3"]);
    let palette = json(&["palette", path, "--json"]);
    let ramp: Vec<(&str, &str)> = palette
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap_or(""), s["ramp"].as_str().unwrap_or("")))
        .collect();
    assert_eq!(
        ramp,
        [("skin-1", "skin"), ("skin", "skin"), ("skin-3", "skin")],
        "the colour the palette has takes its place in the middle of its ramp"
    );
    assert!(draw(&["palettes"]).contains("pico-8"));
    assert_eq!(json(&["palettes", "--json"])[2]["colors"].as_array().unwrap().len(), 16);
    draw(&["palette", path, "--load", "four-greens"]);
    let file = dir.join("mine.hex");
    std::fs::write(&file, "081820\nabcdef\n").unwrap();
    draw(&["palette", path, "--load", file.to_str().unwrap()]);
    assert_eq!(
        json(&["palette", path, "--json"]).as_array().unwrap().len(),
        8,
        "three of the ramp, the four greens, one new from the file"
    );
    draw(&["palette", path, "--ramp", "greens", "--of", "#081820,#346856,#88c070"]);
    assert!(draw(&["palette", path]).contains("ramp greens"));
    let refused = run(&["palette", path, "--load", "nope"]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("comes with klad"));
    let lonely = run(&["palette", path, "--ramp", "x"]);
    assert!(String::from_utf8_lossy(&lonely.stderr).contains("--base"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sprite_is_written_as_a_grid_of_characters_and_read_back_the_same() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = std::env::temp_dir().join(format!("draw-binary-grid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("sprite.klad");
    let path = bundle.to_str().unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--pixel", "8x6"]);
    draw(&["palette", path, "--add", "#22223b", "--name", "ink"]);
    let text = "i = ink\nb = #f2c14e\n\n..iiii..\n.ibbbbi.\n.ibbbbi.\n..iiii..\n";
    let mut child = Command::new(env!("CARGO_BIN_EXE_klad"))
        .args(["pixel", path, "l1", "--grid", "-", "--at", "0,1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let made = child.wait_with_output().unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    assert!(String::from_utf8_lossy(&made.stdout).contains("pixel marks"));
    let marks: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert_eq!(
        marks["fills"].as_array().unwrap().len(),
        2,
        "one mark for each colour of the grid"
    );
    let plain = draw(&["pixels", path, "l1", "--plain"]);
    assert_eq!(
        plain.trim_end(),
        ". = empty\ni = ink\na = #f2c14e\n\n........\n..iiii..\n.iaaaai.\n.iaaaai.\n..iiii..\n........"
    );
    let ruled = draw(&["pixels", path, "l1"]);
    assert!(ruled.contains("8x6") && ruled.contains(" 1  ..iiii.."), "{ruled}");
    let read: serde_json::Value = serde_json::from_str(&draw(&["pixels", path, "l1", "--json"])).unwrap();
    assert_eq!(read["rows"][2], ".iaaaai.");
    assert_eq!(read["legend"]["i"]["name"], "ink");
    assert_eq!((read["width"].as_u64(), read["height"].as_u64()), (Some(8), Some(6)));
    let copy = dir.join("copy.klad");
    let copy_path = copy.to_str().unwrap();
    draw(&["new", copy_path, "--pixel", "8x6"]);
    draw(&["palette", copy_path, "--add", "#22223b", "--name", "ink"]);
    let saved = dir.join("sprite.txt");
    std::fs::write(&saved, &plain).unwrap();
    draw(&["pixel", copy_path, "l1", "--grid", saved.to_str().unwrap()]);
    assert_eq!(
        draw(&["hash", copy_path]),
        draw(&["hash", path]),
        "what klad pixels prints draws the same picture again"
    );
    std::fs::write(&saved, "i = ink\n\n..q..\n").unwrap();
    let unknown = run(&["pixel", path, "l1", "--grid", saved.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("the legend does not name"));
    draw(&["add", path, "layer", "paint", "name=paint"]);
    let wrong = run(&["pixels", path, "paint"]);
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("reads pixel layers"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn recolour_moves_strokes_together_and_keeps_their_shades() {
    let dir = std::env::temp_dir().join(format!("draw-binary-recolour-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    draw(&["stroke", path, "l1", "--path", "M10 10 L190 10", "--color", "#2f5a30"]);
    draw(&["stroke", path, "l1", "--path", "M10 50 L190 50", "--color", "#a8dc84"]);
    draw(&["recolour", path, "c2:s1", "c2:s2", "--to", "#e3962f"]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    let colours: Vec<String> = listed["strokes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["color"].as_str().unwrap().to_string())
        .collect();
    let lab = |hex: &str| klad_core::color::to_oklab(klad_core::color::parse_hex(hex).unwrap());
    assert!(lab(&colours[0])[0] < lab(&colours[1])[0], "{colours:?}");
    assert!(colours.iter().all(|c| lab(c)[2] > 0.05), "both warm now: {colours:?}");
    let log = draw(&["log", path]);
    assert_eq!(log.matches("cli").count(), 3, "one history step: {log}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_eraser_stroke_attaches_to_the_lines_it_crosses_and_says_so() {
    let dir = std::env::temp_dir().join(format!("draw-binary-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    draw(&["new", path, "--width", "200", "--height", "100"]);
    draw(&["stroke", path, "l1", "--path", "M10 20 L190 20"]);
    draw(&["stroke", path, "l1", "--path", "M10 80 L190 80"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "12",
        "--path",
        "M100 5 L100 35",
    ]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    let strokes = listed["strokes"].as_array().unwrap();
    assert_eq!(strokes[2]["on"], serde_json::json!(["c2:s1"]));
    assert_eq!(strokes[0]["attached"], serde_json::json!(["c2:s3"]));
    assert!(strokes[1].get("attached").is_none());
    assert!(draw(&["strokes", path, "l1"]).contains("on s1"));
    assert!(strokes.iter().all(|s| s.get("hidden").is_none()), "cut, not wiped");
    draw(&["stroke", path, "l1", "--path", "M150 50 L160 50"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "40",
        "--path",
        "M140 50 L170 50",
    ]);
    let wiped: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert_eq!(wiped["strokes"][3]["hidden"], serde_json::json!(true));
    draw(&["detach", path, "c2:s1"]);
    let listed: serde_json::Value = serde_json::from_str(&draw(&["strokes", path, "l1", "--json"])).unwrap();
    assert!(listed["strokes"][2].get("on").is_none(), "detached from its line");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn modifiers_move_go_and_copy_with_their_marks_and_attach_again_when_moved_alone() {
    let dir = std::env::temp_dir().join(format!("draw-binary-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle = dir.join("a.klad");
    let path = bundle.to_str().unwrap();
    let draw = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let listed = |frame: &str| -> Vec<serde_json::Value> {
        let text = draw(&["strokes", path, "l1", "--frame", frame, "--json"]);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["strokes"].as_array().unwrap().clone()
    };
    draw(&["new", path, "--width", "400", "--height", "200"]);
    draw(&["stroke", path, "l1", "--path", "M10 40 L390 40"]);
    draw(&["stroke", path, "l1", "--path", "M10 140 L390 140"]);
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "12",
        "--path",
        "M100 25 L100 55",
    ]);
    draw(&["transform", path, "c2:s1", "--move", "0,20"]);
    let eraser = listed("0")[2].clone();
    assert_eq!(eraser["on"], serde_json::json!(["c2:s1"]));
    assert_eq!(
        eraser["bounds"][1],
        serde_json::json!(45.0),
        "the eraser came along with its line"
    );
    draw(&["transform", path, "c2:s3", "--move", "0,80"]);
    assert_eq!(
        listed("0")[2]["on"],
        serde_json::json!(["c2:s2"]),
        "moved on its own, it attaches to what it lands on"
    );
    draw(&["rm", path, "c2:s2"]);
    assert_eq!(listed("0").len(), 1, "the eraser went with the only line it was on");
    draw(&[
        "stroke",
        path,
        "l1",
        "--brush",
        "eraser",
        "--size",
        "12",
        "--path",
        "M200 45 L200 75",
    ]);
    let map = draw(&["copy", path, "c2:s1", "--frame", "4"]);
    assert_eq!(map.matches("->").count(), 2, "the line and its eraser: {map}");
    let copies = listed("4");
    assert_eq!(copies.len(), 2);
    assert_eq!(copies[1]["on"], serde_json::json!([copies[0]["address"]]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_brush_with_a_feathered_or_fringed_edge_goes_in_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("draw-binary-feather-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_klad")).args(args).output().unwrap();
    let draw = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let eraser = |feather: &str, fringe: &str| {
        format!(
            r#"{{"format":"klad-brush/1","name":"Eraser","tip":{{"round":{{"hardness":0.85}}}},"spacing":0.06,"blend":"erase","feather":{feather},"fringe":{fringe}}}"#
        )
    };
    let hash = |name: &str, brush: Option<String>| {
        let path = dir.join(format!("{name}.klad"));
        let path = path.to_str().unwrap();
        draw(&["new", path, "--width", "200", "--height", "100"]);
        draw(&[
            "fill",
            path,
            "l1",
            "--polygon",
            "0,0 200,0 200,100 0,100",
            "--color",
            "#204060",
        ]);
        let id = match brush {
            Some(def) => {
                let file = dir.join(format!("{name}.json"));
                std::fs::write(&file, def).unwrap();
                assert!(draw(&["brushes", path, "--set", "soft-eraser", file.to_str().unwrap()]).contains("added"));
                "soft-eraser"
            }
            None => "eraser",
        };
        draw(&[
            "stroke",
            path,
            "l1",
            "--brush",
            id,
            "--size",
            "40",
            "--path",
            "M20 50 L180 50",
        ]);
        let out = run(&["render", path, "-o", dir.join(format!("{name}.png")).to_str().unwrap()]);
        let said = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
        (
            draw(&["brushes", path]),
            said.split("hash ").nth(1).unwrap()[..16].to_string(),
        )
    };
    let (_, plain) = hash("plain", None);
    let (listed, feathered) = hash("feathered", Some(eraser("0.8", "0")));
    assert!(listed.contains("round 0.85 +feather"), "{listed}");
    assert_ne!(plain, feathered, "the feather softens the edge");
    let (listed, fringed) = hash("fringed", Some(eraser("0", "1")));
    assert!(listed.contains("round 0.85 +fringe"), "{listed}");
    assert_ne!(plain, fringed, "the fringe frays it");
    let file = dir.join("bad.json");
    std::fs::write(&file, eraser("2", "0")).unwrap();
    let path = dir.join("plain.klad");
    let out = run(&[
        "brushes",
        path.to_str().unwrap(),
        "--set",
        "bad",
        file.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("feather"));
    let _ = std::fs::remove_dir_all(&dir);
}
