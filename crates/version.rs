use std::path::Path;
use std::process::Command;

const MAIN_STACK: u32 = 8 * 1024 * 1024;

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
    main_stack();
}

fn main_stack() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if !windows || env!("CARGO_PKG_NAME") != "klad-cli" {
        return;
    }
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:{MAIN_STACK}"),
        _ => println!("cargo:rustc-link-arg-bins=-Wl,--stack,{MAIN_STACK}"),
    }
}
