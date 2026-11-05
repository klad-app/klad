import json
import math
import sys

PLAIN = "--plain" in sys.argv
RELIEF = "--relief" in sys.argv
LIGHT = [-0.7, -0.7]
KEY = "#fff1dc"
SHADOW = "#9d8fc4"
COLOURS = {"wall": "#ece3cf", "table": "#b8a78b", "clay": "#b9603d", "clay inside": "#8f4630", "lemon": "#e6bf34",
           "cast": "#b4a6c4", "contact": "#8d7fa6"}


def lemon(cx, cy, rx, ry, turn):
    a = math.radians(turn)
    points = []
    for i in range(96):
        t = 2 * math.pi * i / 96
        tip = max(math.exp(-((t % math.pi) ** 2) / 0.06), math.exp(-((t % math.pi - math.pi) ** 2) / 0.06))
        x, y = rx * math.cos(t) * (1 + 0.1 * tip), ry * math.sin(t) * (1 - 0.2 * tip)
        points.append((cx + x * math.cos(a) - y * math.sin(a), cy + x * math.sin(a) + y * math.cos(a)))
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points) + " Z"


SHAPES = {
    "table": {"type": "rect", "x": 0, "y": 560, "w": 1200, "h": 340},
    "inside": {"type": "ellipse", "cx": 560, "cy": 470, "rx": 260, "ry": 70},
    "lemon-a": {"type": "path", "d": lemon(468, 482, 106, 72, -14)},
    "lemon-b": {"type": "path", "d": lemon(652, 474, 100, 70, 18)},
    "bowl": {"type": "path", "d": "M300 470 C 300 604 420 694 560 694 C 700 694 820 604 820 470 "
                                  "C 820 509 704 540 560 540 C 416 540 300 509 300 470 Z"},
    "lemon-c": {"type": "path", "d": lemon(905, 706, 102, 70, -8)},
}
RELIEF_LOOK = {"table": (1.2, 1.8), "inside": (2, 0.8), "lemon-a": (4, 3.5), "lemon-b": (3.7, 3.3), "bowl": (1.7, 2.05), "lemon-c": (5.1, 4.4)}
PAINT = {
    "table": dict(color="table", form="flat", facing=[0, -1, 0.9], angle=2, weight=20, decisive=0.6),
    "inside": dict(color="clay inside", form="flat", facing=[0.35, 0.5, 1], angle=0, weight=12),
    "lemon-a": dict(color="lemon", weight=9),
    "lemon-b": dict(color="lemon", weight=9),
    "bowl": dict(color="clay", weight=14),
    "lemon-c": dict(color="lemon", weight=9),
}


def layer(name, kind, **extra):
    return {"op": "add", "layer": {"id": f"{name}-layer", "kind": kind, "name": name, **extra}}


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out += [layer("wall", "vector"), layer("table", "paint"), layer("cast", "vector", blend="multiply")]
    out += [layer(id, "paint") for id in ("inside", "lemon-a", "lemon-b", "bowl")]
    out += [layer("cast-front", "vector", blend="multiply"), layer("lemon-c", "paint"), layer("sketch", "vector", visible=False)]
    wall = {"linear": {"from": [0, 0], "to": [900, 560], "stops": [[0, "#f6efdf"], [1, COLOURS["wall"]]]}}
    out.append({"op": "add", "node": {"type": "rect", "x": 0, "y": 0, "w": 1200, "h": 560, "fill": wall}, "in": "wall-layer"})
    out += [{"op": "add", "node": {"id": id, **shape}, "in": "sketch-layer"} for id, shape in SHAPES.items()]
    for id, settings in PAINT.items():
        out.append({"op": "fill", "layer": f"{id}-layer", "shape": id, "color": settings["color"]})
        out.append({"op": "set", "id": f"{id}-layer", "lockAlpha": True})
        paint = {"op": "paint", "layer": f"{id}-layer", "in": [id], "light": LIGHT, "key": KEY, "fillStrength": 0.32,
                 "bounceStrength": 0.16, "seed": len(out), **settings}
        if not PLAIN:
            paint.update(shadow=SHADOW, shadowMix=0.7)
        out.append(paint)
        out.append({"op": "set", "id": f"{id}-layer", "lockAlpha": False})
        if RELIEF:
            strength, softness = RELIEF_LOOK[id]
            out.append({"op": "set", "id": f"{id}-layer", "effects": [{"relief": {"light": LIGHT, "highlight": KEY, "strength": strength, "softness": softness}}]})
    shadows = [("cast", 650, 704, 285, 40, COLOURS["cast"], 16), ("cast", 610, 694, 200, 16, COLOURS["contact"], 6),
               ("cast-front", 948, 768, 110, 20, COLOURS["cast"], 10), ("cast-front", 925, 768, 70, 8, COLOURS["contact"], 4)]
    out += [{"op": "add", "node": {"type": "ellipse", "cx": x, "cy": y, "rx": rx, "ry": ry, "fill": c, "blur": b}, "in": f"{l}-layer"}
            for l, x, y, rx, ry, c, b in shadows]
    return out


print("\n".join(json.dumps(o) for o in ops()))
