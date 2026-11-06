use anyhow::Result;
use klad_core::bundle::now_ms;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

pub struct Watch {
    _watcher: RecommendedWatcher,
    pub events: Receiver<u64>,
}

pub fn relevant(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    !name.contains(".tmp-")
        && (name == "doc.json" || name == "sync.json" || name == "notes.json" || name.ends_with(".strokes"))
}

pub fn watch(root: &Path, ctx: egui::Context) -> Result<Watch> {
    let (tx, events) = channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res
            && event.paths.iter().any(|p| relevant(p))
        {
            let _ = tx.send(now_ms());
            ctx.request_repaint();
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(Watch {
        _watcher: watcher,
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_document_files_count() {
        assert!(relevant(Path::new("a.klad/doc.json")));
        assert!(relevant(Path::new("a.klad/.cache/sync.json")));
        assert!(relevant(Path::new("a.klad/.cache/notes.json")));
        assert!(relevant(Path::new("a.klad/strokes/l1/c0.strokes")));
        assert!(!relevant(Path::new("a.klad/doc.json.tmp-123")));
        assert!(!relevant(Path::new("a.klad/.cache/tiles/l1.raster")));
    }
}
