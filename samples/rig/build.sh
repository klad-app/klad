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
out="${1:-$here/rig.klad}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it" >&2
  exit 1
fi
rm -rf "$out"
"$klad" new "$out" --width 800 --height 800 --layers 0 --background '#e9e2d4' >/dev/null
for step in drawing rig weights poses keys; do
  python3 "$here/generate.py" "$step" | "$klad" apply "$out" >/dev/null
done
"$klad" inbetween "$out" --layers colour,lines --from 0 --to 4 | grep 'on bones' || true
for keys in "4 8" "8 12" "12 16"; do
  set -- $keys
  "$klad" inbetween "$out" --layers colour,lines --from "$1" --to "$2" >/dev/null
done
"$klad" bones "$out"
"$klad" render "$out" --frame 8 --overlay bones -o "${out%.klad}-bones.png" | tail -1
"$klad" render "$out" --sheet wave --drawings -o "${out%.klad}-sheet.png" | tail -1
"$klad" export "$out" --godot-rig "${out%.klad}-godot" --godot-scene "${out%.klad}-godot" 2>&1 | tail -2
"$klad" hash "$out" --all --no-cache | sed -n '1p;9p;13p'
