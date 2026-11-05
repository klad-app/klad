use std::path::Path;
use std::process::Command;

fn main() {
    let git = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.git");
    ["HEAD", "refs", "packed-refs"]
        .iter()
        .map(|p| git.join(p))
        .filter(|p| p.exists())
        .for_each(|p| println!("cargo:rerun-if-changed={}", p.display()));
    println!("cargo:rerun-if-env-changed=KLAD_BUILD");
    let described = std::env::var("KLAD_BUILD").ok().or_else(|| {
        Command::new("git")
            .args(["describe", "--tags", "--always"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    });
    println!("cargo:rustc-env=KLAD_BUILD={}", described.unwrap_or_default());
}
