#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
klad="${KLAD:?set KLAD to the klad binary, or run just icon}"
if [ -n "${1:-}" ]; then
  mkdir -p "$1"
  work="$(cd "$1" && pwd)"
else
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
fi
draw() {
  rm -rf "$work/$1.klad"
  "$klad" new "$work/$1.klad" --width 1024 --height 1024 --layers 0 >/dev/null
  for brush in icon-wet icon-dry; do
    python3 "$here/generate.py" --brush "$brush" | "$klad" brushes "$work/$1.klad" --set "$brush" - >/dev/null
  done
  python3 "$here/generate.py" ${2:-} | "$klad" apply "$work/$1.klad" >/dev/null
}
draw icon
draw icon-small --small
"$klad" render "$work/icon.klad" --out "$here/icon.png" >/dev/null
"$klad" render "$work/icon-small.klad" --out "$here/icon-small.png" >/dev/null
for size in 16 24 32 48 64 128 256 512 1024; do
  drawing=icon
  [ "$size" -le 64 ] && drawing=icon-small
  "$klad" render "$work/$drawing.klad" --scale "$(python3 -c "print($size/1024)")" --out "$work/icon-$size.png" >/dev/null
done
python3 "$here/pack.py" "$work" "$here/klad.icns" "$here/klad.ico"
echo "$here/icon.png $here/icon-small.png $here/klad.icns $here/klad.ico"
