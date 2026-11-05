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
out="${1:-$here/creature.draw}"
if ! "$klad" arcs --help >/dev/null 2>&1; then
  echo "$klad is older than this example (it has no klad arcs): run just build first" >&2
  exit 1
fi
if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
  echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it from generate.py" >&2
  exit 1
fi
layers=shadow,far,colour,shade,rim,lines
rm -rf "$out"
"$klad" new "$out" --width 1024 --height 1024 --layers 0 --fps 12 --frames 11 --background '#e9e4d8' >/dev/null
echo "keys: idle at 0 and 2, walk at 3, 5, 7 and 9 (parts: body, sprout, leaf, second leaf, near leg, far leg)"
python3 "$here/generate.py" | "$klad" apply "$out" >/dev/null 2>&1
"$klad" clip "$out" --add idle --from 0 --to 2 --loop >/dev/null
"$klad" clip "$out" --add walk --from 3 --to 10 --loop >/dev/null
echo "colour every key from the first one:"
"$klad" fill "$out" colour --like colour0 --frames all | grep -v '^changed\|^rev'
"$klad" gaps "$out" colour --sheet all | tail -1
echo "the walk keys' arcs, before in-betweening:"
"$klad" arcs "$out" --track 'lines:near leg' --track 'far:far leg' --clip walk --ground 823,811 --travel 7 --check \
  | grep 'under the ground\|slides\|swap'
echo "in-betweens (paired by part):"
for keys in "0 2" "3 5" "5 7" "7 9"; do
  set -- $keys
  "$klad" inbetween "$out" --layers "$layers" --from "$1" --to "$2" --spacing in-out | grep 'in-between.* in \|left out\|unpaired\|by part'
done
"$klad" inbetween "$out" --layers "$layers" --from 9 --loop --spacing in-out | grep 'in-between.* in \|left out\|unpaired\|by part'
echo "timing:"
"$klad" retime "$out" 0-2 --pingpong | sed -n 1p
"$klad" retime "$out" 0-3 --on 3 | sed -n 1p
"$klad" retime "$out" 12-19 --on 2 | sed -n 1p
"$klad" timeline "$out" --origin 512,820 >/dev/null
"$klad" clip "$out"
echo "the walk's arcs, in-betweened:"
"$klad" arcs "$out" --track 'lines:near leg' --track 'far:far leg' --clip walk --ground 823,811 --travel 3.5 --slack 3 --check \
  | grep 'under the ground\|slides\|swap'
"$klad" frames "$out"
"$klad" gaps "$out" colour --sheet all | tail -1
"$klad" hash "$out" --all --no-cache
"$klad" render "$out" --sheet idle --drawings --cell 300 -o "${out%.draw}-idle.png"
"$klad" render "$out" --sheet walk --drawings --cell 300 -o "${out%.draw}-walk.png"
"$klad" render "$out" --sheet walk --drawings --cell 300 --track 'lines:near leg' --track 'far:far leg' --track 'lines:second leaf' -o "${out%.draw}-walk-arcs.png"
"$klad" export "$out" --frame 0 --scale 2 -o "${out%.draw}@2x.png" >/dev/null
