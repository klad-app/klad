#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
klad="${1:?give the klad binary of the release}"
version="${2:?give the version, as in tests/formats/<version>}"
samples="${3:-$root/samples}"
dest="$root/tests/formats/$version"
if [ -e "$dest" ]; then
    echo "$dest is there already: a saved version never changes" >&2
    exit 1
fi
python="$(command -v python3 || command -v python)"
"$python" - "$samples" "$dest" "$klad" <<'PY'
import pathlib, shutil, subprocess, sys
src, dest, klad = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
names = ["ink-shapes", "lantern", "bounce", "creature", "rig", "sprout", "ground"]
missing = [n for n in names if not (src / n / f"{n}.klad" / "doc.json").is_file()]
if missing:
    sys.exit(f"build the samples first (just samples): {', '.join(missing)} missing")
dest.mkdir(parents=True)
lines = []
for name in names:
    bundle = src / name / f"{name}.klad"
    shutil.copytree(bundle, dest / bundle.name, ignore=shutil.ignore_patterns(".cache"))
    done = subprocess.run([klad, "hash", str(dest / bundle.name), "--all", "--no-cache"], capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"klad hash {bundle.name} failed:\n{done.stderr}")
    shutil.rmtree(dest / bundle.name / ".cache", ignore_errors=True)
    lines += [f"{bundle.name} {frame} {digest}" for _, frame, digest in (l.split() for l in done.stdout.splitlines())]
(dest / "hashes.txt").write_text("\n".join(lines) + "\n")
PY
echo "$dest"
