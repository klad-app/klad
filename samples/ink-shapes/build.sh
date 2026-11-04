#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
klad="${KLAD:-$root/${CARGO_TARGET_DIR:-target}/release/klad}"
out="${1:-$here/ink-shapes.draw}"
rm -rf "$out"
"$klad" new "$out" --width 960 --height 900 --layers 0 --background '#f4efe6' >/dev/null
python3 "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" render "$out" -o "${out%.draw}.png"
"$klad" hash "$out" --no-cache
