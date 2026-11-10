use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SAME_PIXELS: bool = cfg!(target_arch = "aarch64");

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    fs::read_dir(from).unwrap().map(Result::unwrap).for_each(|entry| {
        let target = to.join(entry.file_name());
        match entry.file_type().unwrap().is_dir() {
            true => copy(&entry.path(), &target),
            false => {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    });
}

fn hashes(bundle: &Path) -> BTreeMap<String, String> {
    let out = Command::new(env!("CARGO_BIN_EXE_klad"))
        .args(["hash", bundle.to_str().unwrap(), "--all", "--no-cache"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} does not open: {}",
        bundle.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace().skip(1);
            Some((words.next()?.to_string(), words.next()?.to_string()))
        })
        .collect()
}

fn accepted(root: &Path) -> BTreeMap<(String, String), String> {
    fs::read_to_string(root.join("accepted.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            Some((
                (words.next()?.to_string(), words.next()?.to_string()),
                words.next()?.to_string(),
            ))
        })
        .collect()
}

#[test]
fn drawings_saved_by_every_release_open_and_render_as_they_did() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/formats");
    let accepted = accepted(&root);
    let mut versions: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("hashes.txt").is_file())
        .collect();
    versions.sort();
    assert!(!versions.is_empty(), "tests/formats holds the drawings of each release");
    let scratch = std::env::temp_dir().join(format!("klad-formats-{}", std::process::id()));
    let mut formats = Vec::new();
    let mut wrong = Vec::new();
    versions.iter().for_each(|version| {
        let name = version.file_name().unwrap().to_string_lossy().to_string();
        let mut saved: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        fs::read_to_string(version.join("hashes.txt"))
            .unwrap()
            .lines()
            .filter_map(|l| {
                let mut words = l.split_whitespace();
                Some((
                    words.next()?.to_string(),
                    words.next()?.to_string(),
                    words.next()?.to_string(),
                ))
            })
            .for_each(|(bundle, frame, digest)| {
                saved.entry(bundle).or_default().insert(frame, digest);
            });
        saved.iter().for_each(|(bundle, frames)| {
            let copied = scratch.join(&name).join(bundle);
            copy(&version.join(bundle), &copied);
            let doc: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(copied.join("doc.json")).unwrap()).unwrap();
            formats.push(doc["format"].as_str().unwrap_or_default().to_string());
            let now = hashes(&copied);
            frames.iter().for_each(|(frame, digest)| {
                let key = (format!("{name}/{bundle}"), frame.clone());
                let expected = accepted.get(&key).unwrap_or(digest);
                let differs = match now.get(frame) {
                    None => true,
                    Some(found) => SAME_PIXELS && found != expected,
                };
                if differs {
                    wrong.push(format!(
                        "{name}/{bundle} frame {frame}: was {expected}, now {}",
                        now.get(frame).map_or("missing", String::as_str)
                    ));
                }
            });
        });
    });
    let _ = fs::remove_dir_all(&scratch);
    assert!(
        wrong.is_empty(),
        "drawings from a release no longer open or render as they did (fix it, or list the change in tests/formats/accepted.txt and the release notes; pixels are compared on {}, where the hashes were made, and elsewhere each frame only has to render):\n{}",
        "aarch64",
        wrong.join("\n")
    );
    assert!(
        formats.iter().any(|f| f != klad_core::doc::FORMAT),
        "an older format is among them, so a migration is opened: {formats:?}"
    );
}
