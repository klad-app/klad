#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
target="${1:?give a target triple, for example x86_64-unknown-linux-gnu}"
out="${2:-$root/dist}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
export KLAD_BUILD="${KLAD_BUILD:-$(git -C "$root" describe --tags --always 2>/dev/null || echo "$version")}"
cargo build --profile dist -p klad-cli --target "$target" --manifest-path "$root/Cargo.toml"
built="${CARGO_TARGET_DIR:-$root/target}/$target/dist"
case "$target" in
  *windows*) platform=windows; ext=.exe ;;
  *apple*) platform=macos; ext= ;;
  *) platform=linux; ext= ;;
esac
arch="${target%%-*}"
name="klad-$version-$platform-$arch"
stage="$out/$name"
rm -rf "$stage"
mkdir -p "$stage"
cp "$built/klad$ext" "$stage/"
[ -f "$built/klad-editor$ext" ] && cp "$built/klad-editor$ext" "$stage/"
mkdir -p "$stage/fonts"
(cd "$root/fonts" && tar cf - --exclude update.py .) | (cd "$stage/fonts" && tar xf -)
cp "$root/LICENSE" "$root/LICENSE-MIT" "$root/THIRD-PARTY-NOTICES.md" "$root/docs/skill/SKILL.md" "$stage/"
cp "$root/docs/skill/drawing/SKILL.md" "$stage/SKILL-drawing.md"
cp "$root/packaging/icon/icon.png" "$stage/klad.png"
bash "$root/packaging/samples.sh" "${KLAD:-$stage/klad$ext}" "$stage/samples" >/dev/null
case "$platform" in
  windows)
    (cd "$out" && rm -f "$name.zip" && (command -v 7z >/dev/null && 7z a -tzip "$name.zip" "$name" >/dev/null || powershell -NoProfile -Command "Compress-Archive -Path '$name' -DestinationPath '$name.zip'"))
    echo "$out/$name.zip"
    ;;
  *)
    (cd "$out" && tar czf "$name.tar.gz" "$name")
    echo "$out/$name.tar.gz"
    ;;
esac
if [ "$platform" = linux ] && [ -n "${APPIMAGETOOL:-}" ]; then
  app="$out/$name.AppDir"
  rm -rf "$app"
  mkdir -p "$app/usr/bin" "$app/usr/share/klad"
  cp "$stage/klad" "$stage/klad-editor" "$app/usr/bin/"
  cp -R "$stage/fonts" "$stage/samples" "$app/usr/share/klad/"
  cp "$root/packaging/linux/AppRun" "$app/AppRun"
  cp "$root/packaging/linux/klad.desktop" "$app/klad.desktop"
  cp "$root/packaging/icon/icon.png" "$app/klad.png"
  ARCH="$arch" "$APPIMAGETOOL" "$app" "$out/klad-$version-$arch.AppImage" >/dev/null
  echo "$out/klad-$version-$arch.AppImage"
fi
