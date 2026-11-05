import json
import math
import random

LIGHT = [-0.6, -0.8]
KEY = "#ffe6bd"
COLOURS = {"ink": "#17110c", "iron": "#343944", "wood": "#7a5534", "wood side": "#3f2a19", "shadow": "#5b4a6e",
           "rim": "#ffe2b0", "gold": "#e8c26a", "cream": "#f0e2c2"}
BOARD = (262, 300, 900, 556)


def path(points, closed=True):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)
    return d + (" Z" if closed else "")


def chipped(x0, y0, x1, y1, seed, corner=14):
    rnd = random.Random(seed)
    points = []
    for (ax, ay), (bx, by) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))]:
        length = math.hypot(bx - ax, by - ay)
        ux, uy = (bx - ax) / length, (by - ay) / length
        nx, ny = -uy, ux
        steps = max(2, int((length - 2 * corner) / 26))
        for i in range(steps + 1):
            t = corner + (length - 2 * corner) * i / steps
            wob = 0 if i in (0, steps) else rnd.uniform(-1.4, 1.4) - (3.0 if rnd.random() < 0.12 else 0)
            points.append((ax + ux * t + nx * wob, ay + uy * t + ny * wob))
    return path(points)


def ring(cx, cy, rx, ry):
    return (f"M{cx} {cy - ry} C {cx + rx * 1.33} {cy - ry} {cx + rx * 1.33} {cy + ry} {cx} {cy + ry} "
            f"C {cx - rx * 1.33} {cy + ry} {cx - rx * 1.33} {cy - ry} {cx} {cy - ry} Z")


ARM = ("M88 142 L 902 146 C 918 146 930 152 930 160 C 930 168 918 172 902 170 L 88 168 Z")
BRACE = ("M88 236 C 150 236 210 214 268 190 C 312 172 348 166 380 168 L 380 178 C 350 177 318 184 276 202 "
         "C 214 228 156 250 88 252 Z")
SHAPES = {
    "plate": {"type": "rect", "x": 58, "y": 96, "w": 34, "h": 196, "radius": 5},
    "arm": {"type": "path", "d": ARM},
    "brace": {"type": "path", "d": BRACE},
    "board": {"type": "path", "d": chipped(*BOARD, seed=7)},
    "board-side": {"type": "path", "d": path([(BOARD[0] + 6, BOARD[3] - 4), (BOARD[2] - 4, BOARD[3] - 4),
                                              (BOARD[2] - 10, BOARD[3] + 16), (BOARD[0] + 12, BOARD[3] + 15)])},
}
FLATS = [("plate", "iron"), ("arm", "iron"), ("brace", "iron"), ("board-side", "wood side"), ("board", "wood")]
HANGERS = [(328, 168), (836, 170)]
EYES = [(328, 300), (836, 300)]


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}


def ink(path=None, shapes=None, weight=4.0, seed=1, color="ink"):
    op = {"op": "ink", "layer": "lines", "weight": weight, "light": LIGHT, "color": color, "seed": seed}
    return {**op, "path": path} if path else {**op, "from": shapes}


def chain(top, bottom, seed):
    (x, y0), (_, y1) = top, bottom
    count = max(2, int((y1 - y0) / 22))
    links = []
    for i in range(count):
        cy = y0 + (i + 0.5) * (y1 - y0) / count
        rx, ry = (5.5, 9.5) if i % 2 == 0 else (2.2, 9.5)
        links.append(ink(path=ring(x, round(cy, 1), rx, ry), weight=2.6, seed=seed + i))
    return links


def board():
    rnd = random.Random(11)
    out = [{"op": "set", "id": "signboard", "lockAlpha": True}]
    out.append({"op": "paint", "layer": "signboard", "in": ["board"], "color": COLOURS["wood"], "light": LIGHT, "key": KEY,
                "form": "flat", "facing": [0, -0.15, 1], "angle": 1, "weight": 16, "decisive": 0.8, "seed": 3,
                "shadow": COLOURS["shadow"], "shadowMix": 0.4})
    x0, y0, x1, y1 = BOARD
    for i in range(26):
        y = y0 + 14 + (y1 - y0 - 28) * (i + rnd.uniform(-0.3, 0.3)) / 25
        a, b = x0 + rnd.uniform(-20, 80), x1 - rnd.uniform(-20, 80)
        bend = rnd.uniform(-3, 3)
        tone = rnd.choice(["#5a3c22", "#8f673f", "#4b321d", "#9a7046"])
        out.append({"op": "stroke", "layer": "signboard", "brush": "dry-brush", "size": rnd.uniform(3, 7), "color": tone,
                    "opacity": rnd.uniform(0.25, 0.5), "path": f"M{a:.0f} {y:.1f} Q {(a + b) / 2:.0f} {y + bend:.1f} {b:.0f} {y:.1f}",
                    "pressure": [0.1, 0.8, 1, 0.7, 0.1]})
    for cx, cy, rx in [(470, 470, 16), (760, 352, 11)]:
        out.append({"op": "stroke", "layer": "signboard", "brush": "oil-load", "size": 5, "color": "#3f2a18", "opacity": 0.7,
                    "path": ring(cx, cy, rx, rx * 0.45), "pressure": [0.3, 1, 0.8]})
    out.append({"op": "set", "id": "signboard", "lockAlpha": False})
    out.append({"op": "set", "id": "signboard", "effects": [{"relief": {"light": LIGHT, "strength": 1.4, "softness": 2.2,
                                                                     "highlight": "#ffe9c4", "shade": "#20140c"}}]})
    return out


def lettering():
    cx = (BOARD[0] + BOARD[2]) / 2
    gold = {"linear": {"from": [0, 330], "to": [0, 470], "stops": [[0, "#f6dc93"], [0.55, "#d9a94e"], [1, "#a8742a"]]}}
    edge = {"paint": "#2a1a0c", "width": 2.5, "join": "round"}
    return [
        {"op": "add", "node": {"id": "title", "type": "text", "text": "THE CROOKED", "font": "Cinzel Bold", "size": 58,
                               "x": cx, "y": 334, "align": "center", "tracking": 3, "fill": gold, "stroke": edge}, "in": "lettering"},
        {"op": "add", "node": {"id": "name", "type": "text", "text": "LANTERN", "font": "Cinzel Black", "size": 84,
                               "x": cx, "y": 396, "align": "center", "tracking": 6, "fill": gold, "stroke": edge}, "in": "lettering"},
        {"op": "add", "node": {"id": "subtitle", "type": "text", "text": "rooms, ale and a warm fire", "font": "Caveat",
                               "size": 36, "weight": 600, "x": cx, "y": 492, "align": "center", "fill": COLOURS["cream"]},
         "in": "lettering"},
        {"op": "set", "id": "lettering", "effects": [{"shadow": {"x": 2, "y": 3, "radius": 2.5, "color": "#1a0e06b0"}}]},
    ]


def wear():
    rnd = random.Random(23)
    x0, y0, x1, y1 = BOARD
    out = []
    for i in range(14):
        y = 336 + 150 * (i + rnd.uniform(-0.4, 0.4)) / 13
        a = rnd.uniform(x0 + 60, x0 + 260)
        b = a + rnd.uniform(160, 420)
        out.append({"op": "stroke", "layer": "wear", "brush": "dry-brush", "size": rnd.uniform(2.5, 5), "color": "#6b4a2a",
                    "opacity": rnd.uniform(0.4, 0.8), "path": f"M{a:.0f} {y:.1f} L {b:.0f} {y + rnd.uniform(-1.5, 1.5):.1f}",
                    "pressure": [0.05, 0.7, 1, 0.6, 0.05]})
    return out


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out += [
        layer("sketch", "vector", visible=False),
        layer("iron", "paint"),
        layer("iron-shade", "paint", blend="multiply", opacity=0.85, clip=True),
        layer("iron-rim", "paint", clip=True),
        layer("signboard", "paint"),
        layer("board-shade", "paint", blend="multiply", opacity=0.75, clip=True),
        layer("board-rim", "paint", clip=True),
        layer("lettering", "vector", clip=True),
        layer("wear", "paint", blend="multiply", opacity=0.45, clip=True),
        layer("lines", "paint"),
    ]
    out += [{"op": "add", "node": {**node, "id": sid}, "in": "sketch"} for sid, node in SHAPES.items()]
    out += [{"op": "fill", "layer": "signboard" if sid.startswith("board") else "iron", "shape": sid, "color": c, "expand": 1.5}
            for sid, c in FLATS]
    out.append({"op": "light", "from": ["plate", "arm", "brace"], "light": LIGHT, "shade": "iron-shade", "rim": "iron-rim",
                "depth": 8, "soft": 2, "shadeColor": COLOURS["shadow"], "rimColor": COLOURS["rim"], "rimWidth": 3})
    out += board()
    out.append({"op": "light", "from": ["board"], "light": LIGHT, "shade": "board-shade", "rim": "board-rim",
                "depth": 26, "soft": 3, "shadeColor": "#6a4f62", "rimColor": "#ffd9a0", "rimWidth": 4})
    out += lettering()
    out += wear()
    out.append(ink(shapes=["plate", "arm", "brace"], weight=4.5, seed=2))
    out.append(ink(shapes=["board"], weight=6, seed=3))
    out.append(ink(shapes=["board-side"], weight=4, seed=4))
    out += [ink(path=ring(x, y, 3.5, 3.5), weight=3, seed=20 + i) for i, (x, y) in enumerate([(75, 118), (75, 270)])]
    for i, ((hx, hy), (ex, ey)) in enumerate(zip(HANGERS, EYES)):
        out.append(ink(path=ring(hx, hy + 6, 6, 8), weight=3.2, seed=30 + i))
        out.append(ink(path=ring(ex, ey - 8, 6, 7), weight=3.2, seed=34 + i))
        out += chain((hx, hy + 14), (ex, ey - 15), 40 + 10 * i)
    return out


if __name__ == "__main__":
    print("\n".join(json.dumps(op) for op in ops()))
