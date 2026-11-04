use std::path::{Path, PathBuf};

const SYSTEM_LIBS: [(&str, &str); 2] = [("freetype", "6"), ("fontconfig", "1")];

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let system = PathBuf::from(format!("/usr/lib/{arch}-linux-gnu"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let linked: Vec<bool> = SYSTEM_LIBS
        .iter()
        .map(|(name, major)| link_runtime_only(&system, &out, name, major))
        .collect();
    if linked.iter().any(|&l| l) {
        println!("cargo:rustc-link-search=native={}", out.display());
    }
}

fn link_runtime_only(system: &Path, out: &Path, name: &str, major: &str) -> bool {
    let dev = system.join(format!("lib{name}.so"));
    let runtime = system.join(format!("lib{name}.so.{major}"));
    if dev.exists() || !runtime.exists() {
        return false;
    }
    let link = out.join(format!("lib{name}.so"));
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&runtime, &link).is_ok()
}
