# klad

_Last updated: 2026-11-04 19:41_

Agent-first 2D art application in Rust: a `klad` CLI for agents and a native editor for people with a pen, sharing one document bundle on disk. What and why: `docs/DESIGN.md`. How to build, test and send a change: `CONTRIBUTING.md`. How an agent draws with it: `docs/skill/SKILL.md`.

## Toolchain

- Rust stable through rustup with clippy and rustfmt (`rust-toolchain.toml`), `just` for recipes. `just` builds into a folder per platform, `target/` on macOS and `target/linux` (or `target/windows`) elsewhere, whatever `CARGO_TARGET_DIR` says, so a checkout shared between a Mac and a Linux machine never mixes their binaries (`KLAD_TARGET_DIR` overrides it); `just build` ends by running the binary, so a binary for the wrong platform fails there. Run cargo from the repository root, with `CARGO_TARGET_DIR` set to the same folder when calling cargo directly.
- The editor (`crates/app`, `klad open`) runs on macOS for now. It compiles and its tests run on Linux too; `klad open` exists only in the macOS build.
- The editor's panels run headless in tests through `egui_kittest` (`Editor::open` needs no GPU): click, type, drag, read sizes and accessible names. `just ui-pictures` and `cargo test -p klad-app -- --ignored start_pictures` render the UI into `scratch/ui/` with a blank canvas; `editor_picture` draws the canvas too (the kittest renderer gets the canvas resources, and the canvas is redrawn once it has caught up, since uploads made in a pass kittest did not render are lost). `just ui-time` measures UI work per frame.
- `cargo test -p klad-app` alone does not rebuild the `klad` binary that the job and thumbnail tests run; `just test` (the workspace) does.
- macOS-only code (`crates/app/src/pen.rs`, `crates/app/src/picker.rs`) can be type-checked from Linux with `rustup target add aarch64-apple-darwin` and a small scratch crate holding the module and its objc2 dependencies (`cargo check --offline --target aarch64-apple-darwin`); the full workspace cannot, `zstd-sys` needs the macOS SDK.

## Working

- Every doc in `docs/` starts with a `_Last updated: YYYY-MM-DD HH:MM_` line; update it when the doc changes.
- Never commit build output or binaries, and check `git status` before `git add -A`. `samples/` holds generators only; the bundles and pictures they make stay out of git.
- Commit messages and docs name no one: say what changed and why.

**Done means**

- Tests for new behaviour in the crate that owns it.
- `just lint` (fmt check plus clippy with `-D warnings`), `just test` and `just audit` are clean.
- A change that moves a sample's hash says why.

## Standing rules

- Every feature has a CLI form and an editor form (DESIGN principle 7): a change that adds one adds the other, with tests on both sides, unless it only makes sense on one side (the editor's view and settings; the CLI's batch, hash and test tools; the ops that draw from shapes, `ink`, `light`, `hatch`, `paint`, `scatter` and `fill --shape`, which stay CLI only).
- No comments in code except `// TODO:`.
- Iterator chains over `for` loops where they read better; plain loops in hot pixel paths where they are clearer or faster.
- Determinism in `core`, `brush` and `render`: transcendental math through `libm`, one seeded RNG (`klad_core::rng`), no wall clock, no `HashMap` iteration order in output, parallelism only over independent units (tiles, image rows).
- Bundles are never broken by a format change: bump the format version and add a migration. The format id inside `doc.json` stays `draw-anything/N` (the project's earlier name) so every bundle keeps opening.
- Permissive dependencies only: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, BSL-1.0, Unicode-3.0, CC0-1.0; fonts bundled in the editor may be OFL-1.1 or the Ubuntu Font Licence (egui's). No GPL, LGPL or MPL unless dual licensed with one of these. `deny.toml` holds the list and `just audit` checks it; every shipped licence text is in `THIRD-PARTY-NOTICES.md` (`just notices`).
- The project is source-available, not open source: the PolyForm Internal Use License 1.0.0 with a personal-use permission, copyright Indicode BV (`LICENSE`).
- No em-dashes in docs, messages or UI text.
