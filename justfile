target_dir := env("KLAD_TARGET_DIR", if os() == "macos" { "target" } else { "target/" + os() })
klad := target_dir / "release" / "klad" + if os() == "windows" { ".exe" } else { "" }

export CARGO_TARGET_DIR := target_dir
export KLAD_TARGET_DIR := target_dir

default: build

build:
    cargo build --release -p klad-cli
    {{klad}} --version

test:
    cargo test --workspace
    python3 docs/skill/drawkit.py --check

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

samples: build
    for s in lantern ink-shapes bounce creature courier solids lemons ruin sign tree meadow; do FORCE=1 bash samples/$s/build.sh >/dev/null && echo "samples/$s/$s.draw"; done

app:
    bash packaging/macos/app.sh

dist target=`rustc -vV | sed -n 's/^host: //p'`:
    bash packaging/archive.sh {{target}}

icon: build
    KLAD={{klad}} bash packaging/icon/build.sh

notices:
    bash licenses/notices.sh

notices-check:
    bash licenses/notices.sh "${TMPDIR:-/tmp}/draw-notices.md"
    diff -q "${TMPDIR:-/tmp}/draw-notices.md" THIRD-PARTY-NOTICES.md

audit: notices-check
    @cargo deny --version >/dev/null 2>&1 || { echo "cargo-deny is not installed: cargo install cargo-deny --locked" >&2; exit 1; }
    cargo deny check

fmt:
    cargo fmt --all

bench *args: build
    {{klad}} bench {{args}}

demo bundle="scratch/demo.draw": build
    {{klad}} demo {{bundle}} --force
    {{klad}} render {{bundle}}

hash bundle="scratch/demo.draw": build
    {{klad}} hash {{bundle}} --no-cache

open bundle="scratch/pen.draw": build
    {{klad}} open {{bundle}}

stress bundle="scratch/stress.draw": build
    {{klad}} stress {{bundle}}

ui-pictures:
    cargo test -p klad-app -- --ignored panel_pictures

ui-time:
    cargo test --release -p klad-app -- --ignored ui_time_per_frame --nocapture
