#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="${1:-$root/dist}"
targets="${KLAD_MAC_TARGETS:-aarch64-apple-darwin}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
build="$(git -C "$root" describe --tags --always 2>/dev/null || echo "$version")"
export KLAD_BUILD="$build"
binaries=()
for target in $targets; do
  rustup target add "$target" >/dev/null 2>&1 || true
  cargo build --profile dist -p klad-cli --target "$target" --manifest-path "$root/Cargo.toml"
  binaries+=("${CARGO_TARGET_DIR:-$root/target}/$target/dist")
done
app="$out/klad.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for exe in klad klad-editor; do
  inputs=()
  for dir in "${binaries[@]}"; do inputs+=("$dir/$exe"); done
  if [ "${#inputs[@]}" -gt 1 ]; then
    lipo -create "${inputs[@]}" -output "$app/Contents/MacOS/$exe"
  else
    cp "${inputs[0]}" "$app/Contents/MacOS/$exe"
  fi
done
sed -e "s/@VERSION@/$version/" -e "s/@BUILD@/$build/" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$root/packaging/icon/klad.icns" "$app/Contents/Resources/"
rsync -a --exclude update.py "$root/fonts/" "$app/Contents/Resources/fonts/"
cp "$root/LICENSE" "$root/LICENSE-MIT" "$root/THIRD-PARTY-NOTICES.md" "$app/Contents/Resources/"
mkdir -p "$app/Contents/Resources/skill"
cp "$root/docs/skill/SKILL.md" "$app/Contents/Resources/skill/"
codesign --force --deep --sign - "$app" 2>/dev/null || true
echo "$app"
if [ "${KLAD_DMG:-1}" = "1" ]; then
  stage="$(mktemp -d)"
  cp -R "$app" "$stage/"
  ln -s /Applications "$stage/Applications"
  dmg="$out/klad-$version-macos.dmg"
  rm -f "$dmg"
  hdiutil create -volname "klad $version" -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null
  rm -rf "$stage"
  echo "$dmg"
fi
