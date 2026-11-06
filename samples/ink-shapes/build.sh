#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
case "$(uname -s)" in
  Darwin) built=target ;;
  Linux) built=target/linux ;;
  *) built=target/windows ;;
esac
klad="${KLAD:-$root/${KLAD_TARGET_DIR:-$built}/release/klad}"
out="${1:-$here/ink-shapes.klad}"
rm -rf "$out"
"$klad" new "$out" --width 960 --height 900 --layers 0 --background '#f4efe6' >/dev/null
"${PYTHON:-python3}" "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" render "$out" -o "${out%.klad}.png"
"$klad" hash "$out" --no-cache
