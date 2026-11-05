#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
klad="${KLAD:?set KLAD to the klad binary, or run just icon}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"$klad" new "$work/icon.draw" --width 1024 --height 1024 --layers 0 >/dev/null
python3 "$here/generate.py" | "$klad" apply "$work/icon.draw" >/dev/null
"$klad" render "$work/icon.draw" --out "$here/icon.png" >/dev/null
for size in 16 24 32 48 64 128 256 512 1024; do
  "$klad" render "$work/icon.draw" --scale "$(python3 -c "print($size/1024)")" --out "$work/icon-$size.png" >/dev/null
done
python3 "$here/pack.py" "$work" "$here/klad.icns" "$here/klad.ico"
echo "$here/icon.png $here/klad.icns $here/klad.ico"
