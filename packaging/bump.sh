#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/Cargo.toml"
part="${1:-patch}"
current="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$manifest")"
IFS=. read -r major minor patch <<< "$current"
case "$part" in
  major) next="$((major + 1)).0.0" ;;
  minor) next="$major.$((minor + 1)).0" ;;
  patch) next="$major.$minor.$((patch + 1))" ;;
  *) echo "just build takes patch (the default), minor or major, not $part" >&2; exit 1 ;;
esac
sed -i.bump "/^\[workspace.package\]/,/^\[/s/^version = \"$current\"/version = \"$next\"/" "$manifest"
rm -f "$manifest.bump"
echo "klad $current -> $next"
