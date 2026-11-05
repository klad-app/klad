#!/usr/bin/env python3
import argparse
import json
import re
import shutil
import sys
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = "google/fonts"
META = "https://fonts.google.com/metadata/fonts"

SHIPPED = [
    ("Inter", "sans"),
    ("Nunito", "sans"),
    ("Montserrat", "sans"),
    ("Rubik", "sans"),
    ("Barlow", "sans"),
    ("Oswald", "sans"),
    ("Exo 2", "sans"),
    ("Atkinson Hyperlegible", "sans"),
    ("EB Garamond", "serif"),
    ("Lora", "serif"),
    ("Playfair Display", "serif"),
    ("Cormorant", "serif"),
    ("Alegreya", "serif"),
    ("Cinzel", "serif"),
    ("Libre Baskerville", "serif"),
    ("JetBrains Mono", "mono"),
    ("IBM Plex Mono", "mono"),
    ("Space Mono", "mono"),
    ("Share Tech Mono", "mono"),
    ("Bangers", "display"),
    ("Luckiest Guy", "display"),
    ("Lilita One", "display"),
    ("Fredoka", "display"),
    ("Bungee", "display"),
    ("Righteous", "display"),
    ("Black Ops One", "display"),
    ("Creepster", "display"),
    ("Special Elite", "display"),
    ("Abril Fatface", "display"),
    ("Alfa Slab One", "display"),
    ("Rye", "display"),
    ("Monoton", "display"),
    ("Orbitron", "display"),
    ("Audiowide", "display"),
    ("Pirata One", "display"),
    ("Uncial Antiqua", "display"),
    ("MedievalSharp", "display"),
    ("Caveat", "handwriting"),
    ("Patrick Hand", "handwriting"),
    ("Permanent Marker", "handwriting"),
    ("Indie Flower", "handwriting"),
    ("Kalam", "handwriting"),
    ("Amatic SC", "handwriting"),
    ("Dancing Script", "handwriting"),
    ("Pacifico", "handwriting"),
    ("Rock Salt", "handwriting"),
    ("Kaushan Script", "handwriting"),
    ("Press Start 2P", "pixel"),
    ("VT323", "pixel"),
    ("Silkscreen", "pixel"),
    ("Pixelify Sans", "pixel"),
    ("Jersey 10", "pixel"),
    ("Micro 5", "pixel"),
    ("Jacquard 12", "pixel"),
]

GROUPS = {
    "Sans Serif": "sans",
    "Serif": "serif",
    "Monospace": "mono",
    "Display": "display",
    "Handwriting": "handwriting",
}
LICENCES = {"ofl": ("OFL-1.1", "OFL.txt"), "apache": ("Apache-2.0", "LICENSE.txt")}
WEIGHTS = {
    100: "Thin",
    200: "ExtraLight",
    300: "Light",
    400: "Regular",
    500: "Medium",
    600: "SemiBold",
    700: "Bold",
    800: "ExtraBold",
    900: "Black",
    1000: "ExtraBlack",
}


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "klad-fonts"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def raw(commit, path):
    return fetch(f"https://raw.githubusercontent.com/{REPO}/{commit}/{path}")


def folders(commit):
    top = json.loads(fetch(f"https://api.github.com/repos/{REPO}/git/trees/{commit}"))
    trees = {t["path"]: t["sha"] for t in top["tree"] if t["type"] == "tree"}
    return {
        entry["path"]: licence
        for licence in LICENCES
        for entry in json.loads(fetch(f"https://api.github.com/repos/{REPO}/git/trees/{trees[licence]}"))["tree"]
        if entry["type"] == "tree"
    }


def key(family):
    return re.sub(r"[^a-z0-9]", "", family.lower())


def metadata(text):
    fonts = [
        dict(re.findall(r'(\w+): "?([^"\n]*)"?', block))
        for block in re.findall(r"fonts \{(.*?)\n\}", text, re.S)
    ]
    return fonts


def clean(filename):
    return re.sub(r"\[[^\]]*\]", "", filename)


def style(font):
    weight = int(font["weight"])
    name = WEIGHTS.get(weight, str(weight))
    italic = font.get("style") == "italic"
    return {"Regular": "Italic"}.get(name, name + " Italic") if italic else name


def chosen(fonts):
    upright = [f for f in fonts if f.get("style") != "italic"]
    variable = [f for f in upright if "[" in f["filename"]]
    if variable:
        return variable[:1]
    weights = {int(f["weight"]): f for f in upright}
    regular = weights.get(400) or min(upright, key=lambda f: abs(int(f["weight"]) - 400))
    bold = weights.get(700)
    return [regular] + ([bold] if bold and bold is not regular else [])


def catalogue(commit, where):
    families = json.loads(fetch(META))["familyMetadataList"]
    entries = sorted(
        (
            {
                "family": f["family"],
                "group": GROUPS.get(f["category"], "display"),
                "path": f"{where[key(f['family'])]}/{key(f['family'])}",
                "rank": f.get("defaultSort", 99999),
            }
            for f in families
            if f.get("isOpenSource", True) and key(f["family"]) in where
        ),
        key=lambda e: e["family"].lower(),
    )
    return {"source": f"https://github.com/{REPO}", "commit": commit, "families": entries}


def ship(commit, where, family, group):
    folder = key(family)
    licence = where[folder]
    text = raw(commit, f"{licence}/{folder}/METADATA.pb").decode()
    fonts = metadata(text)
    picked = chosen(fonts)
    out = HERE / folder
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    spdx, licence_file = LICENCES[licence]
    (out / licence_file).write_bytes(raw(commit, f"{licence}/{folder}/{licence_file}"))
    files = []
    for font in picked:
        name = clean(font["filename"])
        (out / name).write_bytes(raw(commit, f"{licence}/{folder}/{font['filename']}"))
        files.append({"file": name, "style": style(font)})
    print(f"{family}: {', '.join(f['file'] for f in files)}", file=sys.stderr)
    return {
        "family": family,
        "group": group,
        "licence": spdx,
        "folder": folder,
        "licenceFile": licence_file,
        "copyright": picked[0].get("copyright", ""),
        "files": files,
    }


def main():
    parser = argparse.ArgumentParser(description="Fetch the fonts klad ships and the catalogue of open fonts it can download")
    parser.add_argument("--commit", help="the google/fonts commit to pin; default: its main branch now")
    args = parser.parse_args()
    commit = args.commit or json.loads(fetch(f"https://api.github.com/repos/{REPO}/commits/main"))["sha"]
    where = folders(commit)
    listed = catalogue(commit, where)
    lines = ",\n".join(json.dumps(f, ensure_ascii=False, separators=(",", ":")) for f in listed["families"])
    head = json.dumps({k: v for k, v in listed.items() if k != "families"}, separators=(",", ":"))[:-1]
    (HERE / "catalogue.json").write_text(f'{head},"families":[\n{lines}\n]}}\n')
    shipped = [ship(commit, where, family, group) for family, group in SHIPPED]
    manifest = {"source": f"https://github.com/{REPO}", "commit": commit, "families": shipped}
    (HERE / "fonts.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
