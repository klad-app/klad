const MAIN_STACK: u32 = 8 * 1024 * 1024;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:{MAIN_STACK}"),
        _ => println!("cargo:rustc-link-arg-bins=-Wl,--stack,{MAIN_STACK}"),
    }
}
