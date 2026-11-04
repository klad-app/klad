# Contributing

Thank you for looking. klad is young and changes fast; issues with a drawing that shows the problem are the most useful thing you can send.

## Before you start

- Bugs: open an issue with what you did, what you saw, `klad --version`, your OS, and if you can the `.draw` folder (zipped) or the commands that make it.
- Larger changes: open an issue first, so we can agree on the shape before you build it.
- Security problems: see `SECURITY.md`, not the issue tracker.

## Building

- Rust through [rustup](https://rustup.rs); `rust-toolchain.toml` picks the version. [`just`](https://github.com/casey/just) runs the recipes.
- macOS: the Xcode command line tools. Linux: `pkg-config`, `libfreetype-dev` and `libfontconfig1-dev`. Windows is not tested yet.
- The first build downloads a prebuilt Skia for your platform (`skia-bindings` does this), so it needs network once.
- `just build`, `just test`, `just lint`. `just audit` needs [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) and [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) (`cargo install cargo-deny --locked` and `cargo install cargo-about --locked --features cli`).
- The editor is `klad open`. Its panels are tested headless with `egui_kittest`; `cargo test -p klad-app -- --ignored editor_picture` (the editor with a sample drawing on its canvas), `start_pictures` and `just ui-pictures` write pictures of the UI to `scratch/ui/`.

## The rules the code follows

These come from `CLAUDE.md`, which is also the guide for coding agents working on the repository.

- No comments in code except `// TODO:`. Names and small functions carry the meaning.
- Iterator chains where they read better than loops; plain loops in hot pixel code where they are clearer or faster.
- Determinism: `core`, `brush` and `render` give the same pixels on every machine. Transcendental math goes through `libm`, randomness through the seeded `klad_core::rng`, no clock, no `HashMap` order in output, parallelism only over independent tiles or rows. A change that moves a sample's hash says why.
- Bundles never break: a format change bumps the format version and adds a migration, so older drawings open unchanged.
- Dependencies are permissive only (MIT, Apache-2.0, BSD, ISC, Zlib and the others in `deny.toml`). Adding one means `just audit` passes and `just notices` regenerates `THIRD-PARTY-NOTICES.md` in the same change.
- New behaviour comes with tests in the crate that owns it. `just lint` and `just test` are clean before a pull request.
- No em-dashes in docs or messages; every doc in `docs/` starts with a `_Last updated:_` line.

## Sending a change

1. Fork, branch from `main`, keep the change to one thing.
2. `just lint`, `just test` and `just audit` pass.
3. Describe what changed and how you checked it; for anything visual, attach a render (`klad render`) before and after.

By sending a contribution you agree that Indicode BV may publish it under the licence of the project.
