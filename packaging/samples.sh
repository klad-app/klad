#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
klad="${1:?give the klad binary that builds the samples}"
dest="${2:?give the folder the samples go in}"
python="$(command -v python3 || command -v python)"
rm -rf "$dest"
mkdir -p "$dest"
"$python" - "$root/samples" "$dest" "$klad" "$python" <<'PY'
import json, os, pathlib, shutil, subprocess, sys
src, dest, klad, python = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4]
def run(command, env=None):
    done = subprocess.run(command, env=env, capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"{' '.join(command)} failed:\n{done.stderr}")
for entry in json.loads((src / "shipped.json").read_text()):
    bundle = src / entry["bundle"]
    if not (bundle / "doc.json").is_file():
        env = dict(os.environ, KLAD=klad, PYTHON=python, FORCE="1")
        run(["bash", str(bundle.parent / "build.sh"), str(bundle)], env)
    shutil.copytree(bundle, dest / entry["bundle"], ignore=shutil.ignore_patterns(".cache"))
    canvas = json.loads((bundle / "doc.json").read_text())["canvas"]
    scale = min(1.0, 336 / max(canvas["width"], canvas["height"]))
    thumb = (dest / entry["bundle"]).with_name(bundle.stem + ".thumb.png")
    run([klad, "render", str(bundle), "--scale", f"{scale:.4f}", "-o", str(thumb)])
shutil.copy2(src / "shipped.json", dest / "shipped.json")
PY
echo "$dest"
