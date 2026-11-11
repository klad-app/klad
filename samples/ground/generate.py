import json

T = 16
COLUMNS, ROWS = 4, 3

PALETTE = [
    ("ink", "#231d2b", ""),
    ("grass-1", "#2c5a45", "grass"),
    ("grass-2", "#4a8a3c", "grass"),
    ("grass-3", "#8cc24a", "grass"),
    ("dirt-1", "#4b2f2d", "dirt"),
    ("dirt-2", "#7d5136", "dirt"),
    ("dirt-3", "#b07f4f", "dirt"),
    ("stone-1", "#3c3f52", "stone"),
    ("stone-2", "#6b7084", "stone"),
    ("stone-3", "#a7adbd", "stone"),
    ("petal", "#f2c14e", ""),
]

LEGEND = {
    "k": "ink",
    "d": "grass-1",
    "g": "grass-2",
    "l": "grass-3",
    "s": "dirt-1",
    "b": "dirt-2",
    "t": "dirt-3",
    "n": "stone-1",
    "m": "stone-2",
    "h": "stone-3",
    "p": "petal",
}

PEBBLES = [
    [(3, 7, "ss"), (10, 6, "tt"), (6, 11, "ss"), (12, 12, "tt"), (2, 13, "tt")],
    [(4, 6, "tt"), (11, 8, "ss"), (2, 10, "ss"), (8, 13, "tt"), (12, 12, "ss")],
    [(2, 6, "ss"), (9, 9, "tt"), (5, 12, "tt"), (11, 13, "ss"), (13, 7, "tt")],
    [(5, 3, "ss"), (11, 2, "tt"), (2, 8, "tt"), (8, 7, "ss"), (4, 12, "ss"), (11, 11, "tt")],
    [(3, 2, "tt"), (10, 4, "ss"), (6, 8, "tt"), (12, 9, "ss"), (3, 11, "ss"), (9, 13, "tt")],
    [(6, 2, "ss"), (2, 5, "tt"), (11, 6, "tt"), (4, 9, "ss"), (9, 11, "ss"), (13, 13, "tt")],
]

CAP = [
    "llgllllgllllllgl",
    "gggggggggggggggg",
    "gggggggggggggggg",
    "gddggdddggddgggg",
    "bddbbbbbbbddbbbb",
]


def tile(fill="."):
    return [[fill] * T for _ in range(T)]


def dirt(pebbles):
    grid = tile("b")
    for x, y, run in PEBBLES[pebbles]:
        for i, c in enumerate(run):
            grid[y][x + i] = c
    return grid


def cap(grid):
    for y, row in enumerate(CAP):
        for x, c in enumerate(row):
            grid[y][x] = c
    return grid


def edge(grid, left=False, right=False, bottom=False, top=False):
    if left:
        for y in range(T):
            grid[y][0] = "d" if grid[y][0] in "gl" else "s"
    if right:
        for y in range(T):
            grid[y][T - 1] = "d" if grid[y][T - 1] in "gl" else "s"
    if bottom:
        grid[T - 1] = ["s"] * T
    if top and left:
        grid[0][0] = "."
        grid[0][1] = "g"
    if top and right:
        grid[0][T - 1] = "."
        grid[0][T - 2] = "g"
    if bottom and left:
        grid[T - 1][0] = "."
    if bottom and right:
        grid[T - 1][T - 1] = "."
    return grid


PLATFORM = [
    ".gllllgllllllgg.",
    "dggggggggggggggd",
    "dggggggggggggggd",
    "dgddggdddggddggd",
    "sbddbbbbbbbddbbs",
    "sbbbbbbbttbbbbbs",
    "sbbssbbbbbbbbbbs",
    "sbbbbbbbbbbssbbs",
    ".sbbbttbbbbbbbs.",
    "..ssbbbbbbbbss..",
    "....ssssssss....",
    "................",
    "................",
    "................",
    "................",
    "................",
]

STONE = [
    ".hhhhhhhhhhhhhh.",
    "hhmmmmmmmmmmmmmn",
    "hmmmmmmmmmmmmmmn",
    "hmmmmmmmmnnmmmmn",
    "hmmmmmmmnnmmmmmn",
    "hmmmmmmmnmmmmmmn",
    "hmmhhmmmmmmmmmmn",
    "hmmhmmmmmmmmmmmn",
    "hmmmmmmmmmmmmmmn",
    "hmmmmmmmmmmhhmmn",
    "hmmmnnmmmmmhmmmn",
    "hmmnnmmmmmmmmmmn",
    "hmmmmmmmmmmmmmmn",
    "hmmmmmmmmmmmmmnn",
    "hnnnnnnnnnnnnnnn",
    ".nnnnnnnnnnnnnn.",
]

PLANT = [
    "................",
    "................",
    "................",
    "................",
    ".........pp.....",
    "........pkkp....",
    "........pkkp....",
    ".........pp.....",
    "..........d.....",
    "....l.....d.....",
    "....l.....d.....",
    ".l..g.....d..l..",
    ".l..g..l..d..l..",
    "..g.g..l..d.g...",
    "..g.gg.g..d.g...",
    "..gggggggggggg..",
]


def text(part):
    return [list(row) for row in part]


TILES = [
    [edge(cap(dirt(0)), left=True, top=True), cap(dirt(1)), edge(cap(dirt(2)), right=True, top=True), text(PLATFORM)],
    [edge(dirt(3), left=True), dirt(4), edge(dirt(5), right=True), text(STONE)],
    [edge(dirt(1), left=True, bottom=True), edge(dirt(0), bottom=True), edge(dirt(2), right=True, bottom=True), text(PLANT)],
]


def main():
    canvas = [["."] * (T * COLUMNS) for _ in range(T * ROWS)]
    for row, tiles in enumerate(TILES):
        for column, grid in enumerate(tiles):
            for y in range(T):
                for x in range(T):
                    canvas[row * T + y][column * T + x] = grid[y][x]
    out = [
        {
            "op": "palette",
            "set": [{"color": c, "name": n, **({"ramp": r} if r else {})} for n, c, r in PALETTE],
        },
        {"op": "add", "layer": {"id": "l-tiles", "kind": "pixel", "name": "tiles"}},
        {"op": "pixel", "layer": "l-tiles", "rows": ["".join(row) for row in canvas], "legend": LEGEND},
    ]
    for op in out:
        print(json.dumps(op))


main()
