#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let path = std::env::args_os()
        .skip(1)
        .find(|a| !a.to_string_lossy().starts_with("-psn_"))
        .map(std::path::PathBuf::from);
    if let Err(e) = klad_app::run(path) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
