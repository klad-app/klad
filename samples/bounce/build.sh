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
out="${1:-$here/bounce.klad}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
rm -rf "$out"
"$klad" new "$out" --width 1024 --height 768 --layers 0 --fps 12 --frames 20 --background '#f4efe6' >/dev/null
"${PYTHON:-python3}" "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" render "$out" --frame 7 -o "${out%.klad}.png"
"$klad" frames "$out"
"$klad" hash "$out" --all --no-cache
echo "sheet and onion (the hash is on each second line):"
"$klad" render "$out" --sheet all -o "${out%.klad}-sheet.png"
"$klad" render "$out" --sheet all --drawings -o "${out%.klad}-drawings.png"
"$klad" render "$out" --frame 5 --overlay onion -o "${out%.klad}-onion.png"
echo "sprite sheets at 1x and 0.5x and a Godot export (hashes on the lines starting with hash):"
"$klad" export "$out" --sheet all -o "${out%.klad}-sprites.png"
"$klad" export "$out" --sheet all --scale 0.5 -o "${out%.klad}-sprites@0.5x.png"
"$klad" export "$out" --godot "${out%.klad}-godot" >/dev/null
