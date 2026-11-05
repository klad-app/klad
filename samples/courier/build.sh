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
out="${1:-$here/courier.draw}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
rm -rf "$out"
"$klad" new "$out" --width 640 --height 560 --layers 0 --fps 12 --frames 16 >/dev/null
python3 "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" clip "$out" --add idle --from 0 --to 7 --loop >/dev/null
"$klad" clip "$out" --add run --from 8 --to 15 --loop >/dev/null
"$klad" timeline "$out" --origin 300,500 >/dev/null
"$klad" render "$out" --sheet idle --drawings --cell 280 -o "${out%.draw}-idle.png"
"$klad" render "$out" --sheet run --drawings --cell 280 -o "${out%.draw}-run.png"
"$klad" hash "$out" --all --no-cache
"$klad" export "$out" --godot "${out%.draw}-godot" >/dev/null
