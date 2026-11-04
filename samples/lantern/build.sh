#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
klad="${KLAD:-$root/${CARGO_TARGET_DIR:-target}/release/klad}"
out="${1:-$here/lantern.draw}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
rm -rf "$out"
"$klad" new "$out" --width 1024 --height 1280 --layers 0 >/dev/null
python3 "$here/generate.py" ${PAINTED:+--painted} | "$klad" apply "$out" >/dev/null
"$klad" render "$out" -o "${out%.draw}.png"
"$klad" export "$out" --scale 2 -o "${out%.draw}@2x.png"
"$klad" hash "$out" --no-cache
