import json
import sys
from pathlib import Path

fonts = Path(sys.argv[1])
manifest = json.loads((fonts / "fonts.json").read_text())
families = manifest["families"]
print("## Fonts for text\n")
print(
    "klad ships these font families for text nodes, in the `fonts` folder next to the binary, each with its licence file. "
    "They come unchanged from the Google Fonts repository (https://github.com/google/fonts, commit "
    f"`{manifest['commit']}`). A drawing that uses one holds a copy of the font file and its licence. "
    "Families downloaded later through `klad fonts --get` keep their own licence file in the per-user fonts folder.\n"
)
for family in families:
    copyright = family["copyright"] or "see the licence file"
    files = ", ".join(f["file"] for f in family["files"])
    print(f"- {family['family']} ({files}): {family['licence']}. {copyright}")
print()
for spdx, name in [("OFL-1.1", "SIL Open Font License 1.1"), ("Apache-2.0", "Apache License 2.0")]:
    sample = next((f for f in families if f["licence"] == spdx), None)
    if sample is None:
        continue
    text = (fonts / sample["folder"] / sample["licenceFile"]).read_text()
    if spdx == "OFL-1.1":
        text = text[text.index("This Font Software is licensed"):]
    print(f"### {name}\n\nThe families above marked {spdx} are under this licence; each family's copyright line is in the list above.\n")
    print("```text")
    print(text.rstrip())
    print("```\n")
