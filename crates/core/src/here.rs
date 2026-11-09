use crate::bundle::write_atomic;
use crate::doc::{Document, Here, tag_letters};
use crate::rng::{FNV_OFFSET, fnv};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub rev: u64,
    #[serde(default)]
    pub next: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marks: BTreeMap<String, u32>,
}

impl State {
    pub fn of(doc: &Document) -> Self {
        Self {
            tag: doc.here.tag.clone(),
            rev: doc.rev,
            next: doc.next_id,
            marks: doc.here.marks.clone(),
        }
    }

    pub fn here(&self) -> Here {
        Here {
            tag: self.tag.clone(),
            marks: self.marks.clone(),
        }
    }
}

pub fn path(root: &Path) -> PathBuf {
    root.join(".cache").join("copy.json")
}

pub fn read(root: &Path) -> Option<State> {
    serde_json::from_str(&fs::read_to_string(path(root)).ok()?).ok()
}

pub fn new_tag(root: &Path) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let hash = fnv(
        fnv(fnv(FNV_OFFSET, &nanos.to_le_bytes()), &std::process::id().to_le_bytes()),
        root.to_string_lossy().as_bytes(),
    );
    tag_letters(1 + (hash % (26 * 26)) as u32).unwrap_or_default()
}

pub fn load(root: &Path) -> (State, bool) {
    match read(root) {
        Some(state) => (state, false),
        None => (
            State {
                tag: new_tag(root),
                ..State::default()
            },
            true,
        ),
    }
}

pub fn start() -> State {
    State::default()
}

pub fn write(root: &Path, state: &State) -> Result<State> {
    let merged = match read(root) {
        Some(disk) if disk.tag == state.tag => State {
            rev: disk.rev.max(state.rev),
            next: disk.next.max(state.next),
            marks: disk
                .marks
                .into_iter()
                .chain(state.marks.clone())
                .fold(BTreeMap::new(), |mut all, (cel, top)| {
                    let at: &mut u32 = all.entry(cel).or_default();
                    *at = (*at).max(top);
                    all
                }),
            ..state.clone()
        },
        _ => state.clone(),
    };
    write_atomic(&path(root), serde_json::to_string_pretty(&merged)?.as_bytes())?;
    Ok(merged)
}

fn files(dir: &Path, rel: &str, out: &mut Vec<(String, fs::Metadata)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    entries.filter_map(|e| e.ok()).for_each(|entry| {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains(".tmp-") {
            return;
        }
        let rel = format!("{rel}/{name}");
        match entry.metadata() {
            Ok(meta) if meta.is_dir() => files(&entry.path(), &rel, out),
            Ok(meta) => out.push((rel, meta)),
            Err(_) => {}
        }
    });
}

fn stamp(meta: &fs::Metadata) -> Vec<u8> {
    let nanos = |t: std::io::Result<SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos())
    };
    let mut out = meta.len().to_le_bytes().to_vec();
    out.extend(nanos(meta.modified()).to_le_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        out.extend(meta.ino().to_le_bytes());
        out.extend(meta.ctime().to_le_bytes());
        out.extend(meta.ctime_nsec().to_le_bytes());
    }
    #[cfg(not(unix))]
    out.extend(nanos(meta.created()).to_le_bytes());
    out
}

pub fn fingerprint(root: &Path) -> u64 {
    let mut found = Vec::new();
    if let Ok(meta) = fs::metadata(root.join("doc.json")) {
        found.push(("doc.json".to_string(), meta));
    }
    ["strokes", "weights"]
        .iter()
        .for_each(|dir| files(&root.join(dir), dir, &mut found));
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.iter().fold(FNV_OFFSET, |hash, (rel, meta)| {
        fnv(fnv(hash, rel.as_bytes()), &stamp(meta))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("klad-here-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("strokes/l1")).unwrap();
        fs::write(dir.join("doc.json"), "{}").unwrap();
        dir
    }

    #[test]
    fn any_write_to_the_drawing_changes_the_fingerprint_and_the_cache_does_not() {
        let dir = folder("print");
        let first = fingerprint(&dir);
        assert_eq!(fingerprint(&dir), first, "reading changes nothing");
        write_atomic(&dir.join(".cache/renders/r1.png"), b"x").unwrap();
        write_atomic(&dir.join("strokes/l1/c1.strokes.tmp-1-1"), b"x").unwrap();
        assert_eq!(
            fingerprint(&dir),
            first,
            "the cache and half-written files are not the drawing"
        );
        write_atomic(&dir.join("strokes/l1/c1.strokes"), b"x").unwrap();
        let stroked = fingerprint(&dir);
        assert_ne!(stroked, first, "a new stroke file");
        write_atomic(&dir.join("strokes/l1/c1.strokes"), b"x").unwrap();
        assert_ne!(
            fingerprint(&dir),
            stroked,
            "the same bytes written again, as a checkout does"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_copy_gets_its_own_tag_and_keeps_counting() {
        let dir = folder("tag");
        let made = write(&dir, &start()).unwrap();
        assert_eq!(made.tag, "", "the copy a drawing is made in has no tag");
        assert_eq!(load(&dir), (made.clone(), false));
        write(
            &dir,
            &State {
                next: 40,
                ..made.clone()
            },
        )
        .unwrap();
        write(
            &dir,
            &State {
                next: 12,
                ..made.clone()
            },
        )
        .unwrap();
        assert_eq!(read(&dir).unwrap().next, 40, "counters never go back");
        fs::remove_dir_all(dir.join(".cache")).unwrap();
        let (cloned, fresh) = load(&dir);
        assert!(
            fresh && cloned.tag.len() == 2,
            "a clone or worktree has no .cache: a new copy: {cloned:?}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
