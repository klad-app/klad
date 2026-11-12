# Contributing

Thank you for looking. klad is young and changes fast; issues with a drawing that shows the problem are the most useful thing you can send.

## Before you start

- Bugs: open an issue with what you did, what you saw (run the command again with `KLAD_BACKTRACE=1` for the details of an error), `klad --version`, your OS, and if you can the `.klad` folder (zipped) or the commands that make it.
- Larger changes: open an issue first, so we can agree on the shape before you build it.
- Security problems: see `SECURITY.md`, not the issue tracker.

## Building

- Rust through [rustup](https://rustup.rs); `rust-toolchain.toml` picks the version. [`just`](https://github.com/casey/just) runs the recipes.
- macOS: the Xcode command line tools. Linux: `pkg-config`, `libfreetype-dev`, `libfontconfig1-dev`, `libxkbcommon-dev` and `libwayland-dev`. Windows: Visual Studio's C++ build tools; the check on every pull request builds it there.
- The first build downloads a prebuilt Skia for your platform (`skia-bindings` does this), so it needs network once.
- `just build` counts the patch version up and builds `klad` and `klad-editor` for this machine (`just build minor`, `just build major`; `just compile`, or `just` on its own, builds without counting up), `just test`, `just lint`. They build into `target/` on macOS and `target/linux` on Linux, so one checkout can be built from both. `just audit` needs [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) and [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) (`cargo install cargo-deny --locked` and `cargo install cargo-about --locked --features cli`).
- The editor is `klad open`, or `klad-editor` on its own (the app on a Mac, the program to start on Windows). `cargo build --no-default-features -p klad-cli` builds `klad` without the editor, for servers. Its panels are tested headless with `egui_kittest`; `cargo test -p klad-app -- --ignored editor_picture` (the editor with a sample drawing on its canvas), `start_pictures` and `just ui-pictures` write pictures of the UI to `scratch/ui/`.

## The rules the code follows

These come from `CLAUDE.md`, which is also the guide for coding agents working on the repository.

- No comments in code except `// TODO:`. Names and small functions carry the meaning.
- Iterator chains where they read better than loops; plain loops in hot pixel code where they are clearer or faster.
- Determinism: `core`, `brush` and `render` give the same pixels on every machine. Transcendental math goes through `libm`, randomness through the seeded `klad_core::rng`, no clock, no `HashMap` order in output, parallelism only over independent tiles or rows. A change that moves a sample's hash says why.
- Drawings never break: a format change bumps the format version (`klad/N`) and adds a migration, so older `.klad` drawings open unchanged, and updates `docs/format.md`. A migration converts, it never drops: what a drawing holds survives every later version. (`klad/4` dropping the rigs of `klad/1` to `klad/3`, before 1.0, was the last exception.) `tests/formats/` holds drawings saved by each release with their frame hashes; the test opens them all, and a change that makes one render differently is fixed, or listed in `tests/formats/accepted.txt` (`<version>/<bundle> <frame> <new hash>`) and in the release notes.
- Dependencies are permissive only (MIT, Apache-2.0, BSD, ISC, Zlib and the others in `deny.toml`). Adding one means `just audit` passes and `just notices` regenerates `THIRD-PARTY-NOTICES.md` in the same change.
- Every feature has a CLI form and an editor form: a new command or op also gets its place in the editor, and an editor feature gets its command or op, with tests on both sides. The ops that draw from shapes (`ink`, `light`, `hatch`, `paint`, `scatter`) are the exception and stay CLI only.
- New behaviour comes with tests in the crate that owns it. `just lint` and `just test` are clean before a pull request.
- No em-dashes in docs or messages; every doc in `docs/` starts with a `_Last updated:_` line.
- Guides in `docs/guide/` are short how-to pages: one task per section, titled as the goal, done the shortest way in numbered steps, under about 300 words a page, with no catalogues of options.

## Sending a change

1. Fork, branch from `main`, keep the change to one thing.
2. `just lint`, `just test` and `just audit` pass.
3. Describe what changed and how you checked it; for anything visual, attach a render (`klad render`) before and after.
4. The first time, the CLA assistant asks you in the pull request to sign the contributor licence agreement (`CLA.md`): one click, and it covers every later change. Under it your contribution is licensed to Indicode BV for inclusion in klad and in the builds of klad it sells; you keep the copyright to your work.

klad is source-available, not open source (`LICENSE`): its licence lets you send changes to Indicode BV to propose them, so a fork on GitHub for pull requests is fine; publishing klad or builds of it elsewhere is not. `docs/format.md`, `docs/cli.md`, `docs/skill/` and `crates/client/` are MIT (`LICENSE-MIT`); `klad-client` depends on nothing in the other crates, so it stays MIT on its own.

## Releases

The Mac and Windows builds are sold on itch.io (and later Steam); Linux is built from source. GitHub has the release notes, not downloads.

1. `just build minor` (or `major`) sets the release's version; move the changes under "Unreleased" in `CHANGELOG.md` to a section `## x.y.z (date)` with that version.. `just formats x.y.z` saves the samples with their hashes into `tests/formats/x.y.z/` (after `just samples`). Commit.
2. On a Mac with [butler](https://itch.io/docs/butler/) logged in: `ITCH_TARGET=<user>/<page> just itch-mac` builds the app (arm64 and x86_64 in one) and pushes it to the page's `mac` channel.
3. Tag `vx.y.z` and push the tag. The `release` workflow runs the checks, builds the Windows version and pushes it to the `windows` channel (with the `BUTLER_API_KEY` secret and the `ITCH_TARGET` variable set in the repository), then publishes that changelog section as the release notes, which the editor's update check reads.
4. The builds are not signed yet.
