use crate::bundle::{Bundle, now_ms, write_atomic};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub frame: u32,
    pub text: String,
    pub author: String,
    pub at_ms: u64,
    #[serde(default)]
    pub done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub layer: Option<String>,
    pub nodes: Vec<String>,
    pub frame: u32,
    pub at_ms: u64,
}

fn notes_path(bundle: &Bundle) -> PathBuf {
    bundle.cache_dir().join("notes.json")
}

fn selection_path(bundle: &Bundle) -> PathBuf {
    bundle.cache_dir().join("selection.json")
}

pub fn read(bundle: &Bundle) -> Vec<Note> {
    fs::read_to_string(notes_path(bundle))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write(bundle: &Bundle, notes: &[Note]) -> Result<()> {
    write_atomic(&notes_path(bundle), serde_json::to_string_pretty(notes)?.as_bytes())
}

pub fn add(bundle: &Bundle, (x, y): (f32, f32), frame: u32, text: &str, author: &str) -> Result<Note> {
    let mut notes = read(bundle);
    let note = Note {
        id: notes.iter().map(|n| n.id).max().map_or(1, |m| m + 1),
        x,
        y,
        frame,
        text: text.trim().to_string(),
        author: author.to_string(),
        at_ms: now_ms(),
        done: false,
    };
    notes.push(note.clone());
    write(bundle, &notes)?;
    Ok(note)
}

pub fn set_done(bundle: &Bundle, id: u32, done: bool) -> Result<bool> {
    let mut notes = read(bundle);
    let found = notes.iter_mut().find(|n| n.id == id).map(|n| n.done = done).is_some();
    if found {
        write(bundle, &notes)?;
    }
    Ok(found)
}

pub fn set_text(bundle: &Bundle, id: u32, text: &str) -> Result<bool> {
    let mut notes = read(bundle);
    let found = notes
        .iter_mut()
        .find(|n| n.id == id)
        .map(|n| n.text = text.trim().to_string())
        .is_some();
    if found {
        write(bundle, &notes)?;
    }
    Ok(found)
}

pub fn remove(bundle: &Bundle, id: u32) -> Result<()> {
    let notes: Vec<Note> = read(bundle).into_iter().filter(|n| n.id != id).collect();
    write(bundle, &notes)
}

pub fn read_selection(bundle: &Bundle) -> Option<Selection> {
    serde_json::from_str(&fs::read_to_string(selection_path(bundle)).ok()?).ok()
}

pub fn write_selection(bundle: &Bundle, layer: Option<String>, nodes: Vec<String>, frame: u32) -> Result<()> {
    let selection = Selection {
        layer,
        nodes,
        frame,
        at_ms: now_ms(),
    };
    write_atomic(
        &selection_path(bundle),
        serde_json::to_string_pretty(&selection)?.as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    #[test]
    fn notes_and_selection_round_trip() {
        let dir = std::env::temp_dir().join(format!("draw-notes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bundle = Bundle::create(&dir, Document::new(32, 32)).unwrap();
        assert!(read(&bundle).is_empty());
        let a = add(&bundle, (4.0, 5.0), 0, " too dark ", "user").unwrap();
        let b = add(&bundle, (9.0, 9.0), 0, "move this left", "user").unwrap();
        assert_eq!((a.id, b.id, a.text.as_str()), (1, 2, "too dark"));
        assert!(set_done(&bundle, 1, true).unwrap());
        assert!(!set_done(&bundle, 7, true).unwrap());
        remove(&bundle, 2).unwrap();
        assert_eq!(read(&bundle), vec![Note { done: true, ..a }]);
        write_selection(&bundle, Some("l1".into()), vec!["n3".into()], 0).unwrap();
        assert_eq!(read_selection(&bundle).unwrap().nodes, vec!["n3"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
