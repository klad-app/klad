#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$here/../THIRD-PARTY-NOTICES.md}"
if ! cargo about --version >/dev/null 2>&1; then
  echo "cargo-about is not installed: cargo install cargo-about --locked --features cli" >&2
  exit 1
fi
crates="$(mktemp)"
log="$(mktemp)"
trap 'rm -f "$crates" "$log"' EXIT
if ! cargo about generate --config "$here/about.toml" "$here/about.hbs" -o "$crates" 2>"$log"; then
  cat "$log" >&2
  exit 1
fi
section() {
  printf '### %s\n\n%s\n\n' "$1" "$2"
  shift 2
  for file in "$@"; do
    printf '```text\n'
    cat "$here/bundled/$file"
    printf '\n```\n\n'
  done
}
{
  cat "$crates"
  printf '## Built into Skia\n\n`skia-bindings` links a prebuilt Skia (m157) with these libraries compiled in. On Linux, FreeType and fontconfig come from the system and are not shipped.\n\n'
  section "Skia" "The 2D library that draws every pixel. BSD 3-Clause." skia.txt
  section "libpng" "PNG decoding and encoding inside Skia." libpng.txt
  section "zlib" "Compression for PNG and PDF inside Skia." zlib.txt
  section "libjpeg-turbo" "JPEG decoding and encoding inside Skia. IJG, BSD 3-Clause and zlib licences." libjpeg-turbo.md libjpeg-turbo-ijg.txt
  section "Expat" "XML parsing inside Skia. MIT." expat.txt
  section "Wuffs" "GIF decoding inside Skia. MIT and Apache-2.0." wuffs.txt
  printf '## Fonts\n\nThe editor shows its interface in these fonts. Ubuntu Light (Ubuntu Font Licence) and Noto Emoji (SIL Open Font License) come with egui; their licence texts are above, under epaint_default_fonts.\n\n'
  section "Phosphor icons" "The editor's icons. MIT." phosphor.txt
  section "Hack" "egui's monospace font. MIT, with the Bitstream Vera licence for the glyphs it builds on." hack.txt
  section "emoji-icon-font and egui-icons" "egui's icon glyphs. MIT." emoji-icon-font.txt egui-icons.txt
  python3 "$here/fonts.py" "$here/../fonts"
} > "$out"
