use std::path::{Path, PathBuf};

pub const NATIVE: bool = !cfg!(test);

#[cfg(target_os = "macos")]
pub fn choose_folder(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    choose(start, prompt, true)
}

#[cfg(target_os = "macos")]
pub fn choose_file(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    choose(start, prompt, false)
}

#[cfg(target_os = "macos")]
fn choose(start: Option<&Path>, prompt: &str, folders: bool) -> Option<PathBuf> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(folders);
    panel.setCanChooseFiles(!folders);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(folders);
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
