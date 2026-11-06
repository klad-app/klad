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
out="${1:-$here/meadow.klad}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
rm -rf "$out"
"$klad" new "$out" --width 1600 --height 760 --layers 0 >/dev/null
for name in tuft tuft-b; do
  "$klad" new "$work/$name.klad" --width 256 --height 256 --layers 0 >/dev/null
  "${PYTHON:-python3}" "$here/generate.py" --picture "$name" | "$klad" apply "$work/$name.klad" >/dev/null
  "$klad" render "$work/$name.klad" -o "$work/$name.png" >/dev/null
  "$klad" brushes "$out" --picture "$name" "$work/$name.png" >/dev/null
done
"${PYTHON:-python3}" "$here/generate.py" --brush > "$work/tuft.json"
"$klad" brushes "$out" --set tuft "$work/tuft.json" >/dev/null
"${PYTHON:-python3}" "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" render "$out" -o "${out%.klad}.png"
"$klad" hash "$out" --no-cache
