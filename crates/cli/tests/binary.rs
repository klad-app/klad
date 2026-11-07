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
    let made = klad(&[
        "bone",
        path,
        "l1",
        "--chain",
        "thigh,shin",
        "--through",
        "100,100 100,200 100,300",
    ]);
    assert!(made.contains("chain thigh, shin"), "{made}");
    let ik = klad(&["ik", path, "l1", "--add", "shin", "--softness", "4"]);
    assert!(ik.contains("a limb with thigh") && ik.contains("softness 4"), "{ik}");
    klad(&["ik", path, "l1", "--set", "shin", "--bend", "flip"]);
    klad(&[
        "pose",
        path,
        "l1",
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
    assert!(listed.contains("root -10,0 target shin=180,240 mix 0.5"), "{listed}");
    let json: serde_json::Value = serde_json::from_str(&klad(&["bones", path, "--json"])).unwrap();
    assert_eq!(json[0]["ik"][0]["bone"], "shin");
    assert!(json[0]["poses"][0]["solved"]["thigh"].is_number());
    std::fs::remove_dir_all(&dir).unwrap();
}
