import json

W, H = 32, 32
FPS = 8

PALETTE = [
    ("ink", "#231d2b", ""),
    ("leaf-1", "#2c5a45", "leaf"),
    ("leaf-2", "#4a8a3c", "leaf"),
    ("leaf-3", "#8cc24a", "leaf"),
    ("soil-1", "#4b2f2d", "soil"),
    ("soil-2", "#7d5136", "soil"),
    ("soil-3", "#b07f4f", "soil"),
    ("visor", "#27304f", ""),
    ("eye", "#e9f1ff", ""),
]

LEGEND = {
    "k": "ink",
    "d": "leaf-1",
    "g": "leaf-2",
    "l": "leaf-3",
    "s": "soil-1",
    "b": "soil-2",
    "t": "soil-3",
    "v": "visor",
    "e": "eye",
}

BODY = [
    "......kkkkkk......",
    "....kklllgggkk....",
    "...klllggggggdk...",
    "..kllgggggggggdk..",
    ".klgggggggggggddk.",
    ".kggggggggggggddk.",
    "kgggggggggggggdddk",
    "kggggggggggggddddk",
    "kddddddddddddddddk",
    ".kttbbbbbbvvvvvvk.",
    ".ktbbbbbbvvevvevk.",
    ".kbbbbbbbvvevvevk.",
    "..kbbbbbbbvvvvvk..",
    "...kssbbbbsssvk...",
    "....kkkkkkkkkk....",
]

SQUASH = BODY[:6] + BODY[7:]

SPROUT = {
    "up": [
        "..ll...l....",
        ".lggg.lgg...",
        "..ggg.gg....",
        "....k.k.....",
        ".....k......",
    ],
    "back": [
        ".ll...l.....",
        "lggg.lgg....",
        ".ggg.gg.....",
        "....k.k.....",
        ".....k......",
    ],
    "fore": [
        "...ll...l...",
        "..lggg.lgg..",
        "...ggg.gg...",
        "....k.k.....",
        ".....k......",
    ],
}

LEGS = {
    "stand": [
        "...kk..ss...",
        "...kk..ss...",
        "...kk..ss...",
        "...kkk.kkk..",
    ],
    "tuck": [
        "...kk..ss...",
        "...kk..ss...",
        "...kkk.kkk..",
    ],
    "stride": [
        "..kk...ss...",
        ".kk.....ss..",
        "kk.......ss.",
        "kkk......kkk",
    ],
    "pass-near": [
        "...kk.ss....",
        "...kk.ss....",
        "...kkkss....",
        "......kkk...",
    ],
    "stride-far": [
        "..ss...kk...",
        ".ss.....kk..",
        "ss.......kk.",
        "kkk......kkk",
    ],
    "pass-far": [
        "...kk.ss....",
        "...kk.ss....",
        "...kk.kkk...",
        "...kkk......",
    ],
}

def blank():
    return [["."] * W for _ in range(H)]


def stamp(grid, part, x, y):
    for dy, row in enumerate(part):
        for dx, c in enumerate(row):
            if c != "." and 0 <= y + dy < H and 0 <= x + dx < W:
                grid[y + dy][x + dx] = c


def rows(grid):
    return ["".join(row) for row in grid]


POSES = [
    (0, dict(body=BODY, by=8, sprout="up", legs="stand")),
    (4, dict(body=SQUASH, by=9, sprout="back", legs="tuck")),
    (8, dict(body=BODY, by=9, sprout="back", legs="stride")),
    (9, dict(body=BODY, by=8, sprout="up", legs="pass-near")),
    (10, dict(body=BODY, by=9, sprout="fore", legs="stride-far")),
    (11, dict(body=BODY, by=8, sprout="up", legs="pass-far")),
]
BX = 7


def main():
    out = [
        {
            "op": "palette",
            "set": [{"color": c, "name": n, **({"ramp": r} if r else {})} for n, c, r in PALETTE],
        }
    ]
    cels = [{"frame": start} for start, _ in POSES]
    for layer in ("legs", "body", "sprout"):
        out.append({"op": "add", "layer": {"id": f"l-{layer}", "kind": "pixel", "name": layer, "cels": cels}})
    for frame, pose in POSES:
        legs, body, sprout = blank(), blank(), blank()
        stamp(legs, LEGS[pose["legs"]], BX + 3, pose["by"] + len(pose["body"]) - 1)
        stamp(body, pose["body"], BX, pose["by"])
        stamp(sprout, SPROUT[pose["sprout"]], BX + 3, pose["by"] - 5)
        for layer, grid in (("legs", legs), ("body", body), ("sprout", sprout)):
            out.append({"op": "pixel", "layer": f"l-{layer}", "frame": frame, "rows": rows(grid), "legend": LEGEND})
    out.append({"op": "clip", "add": "idle", "from": 0, "to": 7, "loop": True})
    out.append({"op": "clip", "add": "walk", "from": 8, "to": 11, "loop": True})
    out.append({"op": "timeline", "origin": [16, 26]})
    for op in out:
        print(json.dumps(op))


main()
