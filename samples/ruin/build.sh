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
out="${1:-$here/ruin.draw}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
rm -rf "$out"
"$klad" new "$out" --width 1920 --height 1080 --layers 0 --background '#1c2232' >/dev/null
python3 "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" render "$out" -o "${out%.draw}.png"
"$klad" hash "$out" --no-cache | sed -n 1p
