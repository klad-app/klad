use std::path::{Path, PathBuf};

pub const NATIVE: bool = !cfg!(test);

pub const CHOOSE_DRAWING: &str = if cfg!(target_os = "macos") {
    "Choose a .klad file..."
} else {
    "Choose a .klad folder..."
};

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Folder,
    File,
    Drawing,
}

#[cfg(target_os = "macos")]
pub fn choose_folder(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    choose(start, prompt, Pick::Folder)
}

#[cfg(target_os = "macos")]
pub fn choose_file(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    choose(start, prompt, Pick::File)
}

#[cfg(target_os = "macos")]
pub fn choose_drawing(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    choose(start, prompt, Pick::Drawing)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn only_drawings(panel: &objc2_app_kit::NSOpenPanel) {
    use objc2_foundation::{NSArray, NSString};
    // TODO: setAllowedContentTypes with the drawing's UTType once objc2-uniform-type-identifiers is a dependency
    panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str("klad")])));
}

#[cfg(target_os = "macos")]
fn choose(start: Option<&Path>, prompt: &str, pick: Pick) -> Option<PathBuf> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(pick != Pick::File);
    panel.setCanChooseFiles(pick != Pick::Folder);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(pick == Pick::Folder);
    if pick == Pick::Drawing {
        only_drawings(&panel);
    }
    panel.setPrompt(Some(&NSString::from_str(prompt)));
    if let Some(dir) = start.and_then(Path::to_str) {
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(dir))));
    }
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    panel
        .URL()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
}

#[cfg(not(target_os = "macos"))]
fn dialog(start: Option<&Path>, prompt: &str) -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new().set_title(prompt);
    match start.filter(|d| d.is_dir()) {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn choose_folder(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    dialog(start, prompt).pick_folder()
}

#[cfg(not(target_os = "macos"))]
pub fn choose_file(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    dialog(start, prompt).pick_file()
}

#[cfg(not(target_os = "macos"))]
pub fn choose_drawing(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    choose_folder(start, prompt)
}
