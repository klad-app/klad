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
    let bundle = dir.join("a.draw");
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
    let missing = std::env::temp_dir().join(format!("draw-binary-missing-{}.draw", std::process::id()));
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
    let art = klad.drawing(dir.join("art.draw"));
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
