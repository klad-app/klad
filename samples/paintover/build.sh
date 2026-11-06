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
still="${1:-$here/still.klad}"
walk="${still%still.klad}walk.klad"
for out in "$still" "$walk"; do
  if [ -e "$out" ] && [ "${FORCE:-0}" != 1 ]; then
    echo "$out exists and may hold edits made in the editor; give another path, or FORCE=1 to rebuild it" >&2
    exit 1
  fi
done
KLAD="$klad" FORCE=1 bash "$root/samples/lemons/build.sh" "$still" >/dev/null
"$klad" add "$still" layer paint name=brushwork >/dev/null
"$klad" paintover "$still" brushwork --sizes 32,16,8 --seed 2 2>&1 >/dev/null
"$klad" render "$still" -o "${still%.klad}.png" | tail -1
"$klad" hash "$still" --no-cache | sed -n 1p
KLAD="$klad" FORCE=1 bash "$root/samples/creature/build.sh" "$walk" >/dev/null
"${PYTHON:-python3}" "$here/build.py" "$klad" "$walk"
"$klad" render "$walk" --sheet walk --drawings -o "${walk%.klad}-sheet.png" | tail -1
"$klad" hash "$walk" --all --no-cache | sed -n 1,3p
