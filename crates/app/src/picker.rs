use std::path::{Path, PathBuf};

pub const NATIVE: bool = cfg!(target_os = "macos");

#[cfg(target_os = "macos")]
pub fn choose_folder(start: Option<&Path>, prompt: &str) -> Option<PathBuf> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
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
pub fn choose_folder(_start: Option<&Path>, _prompt: &str) -> Option<PathBuf> {
    None
}
