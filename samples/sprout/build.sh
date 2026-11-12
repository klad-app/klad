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
out="${1:-$here/sprout.klad}"
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
rm -rf "$out"
"$klad" new "$out" --pixel 32x32 --layers 0 --fps 8 --frames 12 >/dev/null
"${PYTHON:-python3}" "$here/generate.py" | "$klad" apply "$out" >/dev/null
"$klad" frames "$out"
"$klad" check "$out" --all
"$klad" hash "$out" --all --no-cache
"$klad" export "$out" --scale 8 -o "${out%.klad}.png"
"$klad" export "$out" --gif --scale 8
"$klad" export "$out" --sheet all -o "${out%.klad}-sheet.png"
