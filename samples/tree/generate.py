import json
import math
import random

LIGHT = [-0.6, -0.8]
KEY = "#fff0cf"
GROUND = 1176
COLOURS = {"bark": "#5d4a3a", "bark dark": "#3b2f27", "leaf back": "#3c6233", "leaf": "#4d7a3b", "leaf front": "#5a8740",
           "grass": "#4a7034", "deep": "#25392a"}

CLUMPS = [
    ("clump1", 318, 452, 170, 128, "leaf back", 3),
    ("clump2", 700, 436, 182, 136, "leaf back", 5),
    ("clump3", 512, 300, 206, 150, "leaf back", 7),
    ("clump4", 238, 610, 150, 112, "leaf", 11),
    ("clump5", 790, 600, 160, 116, "leaf", 13),
    ("clump6", 508, 520, 214, 150, "leaf", 17),
    ("clump7", 390, 700, 150, 104, "leaf front", 19),
    ("clump8", 640, 704, 156, 106, "leaf front", 23),
    ("clump9", 470, 390, 150, 108, "leaf front", 29),
]


def path(points, closed=True):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)
    return d + (" Z" if closed else "")


def blob(cx, cy, rx, ry, seed, count=72):
    rnd = random.Random(seed)
    waves = [(rnd.randint(3, 5), rnd.uniform(0, math.tau), rnd.uniform(0.08, 0.12)),
             (rnd.randint(6, 9), rnd.uniform(0, math.tau), rnd.uniform(0.06, 0.09)),
             (rnd.randint(13, 17), rnd.uniform(0, math.tau), rnd.uniform(0.02, 0.035))]
    points = []
    for i in range(count):
        a = math.tau * i / count
        r = 1 + sum(amp * math.sin(k * a + ph) for k, ph, amp in waves)
        r *= 1 - 0.07 * max(0.0, math.sin(a))
        points.append((cx + rx * r * math.cos(a), cy + ry * r * math.sin(a)))
    return path(points)


def limb(points, w0, w1):
    left, right = [], []
    for i, (x, y) in enumerate(points):
        a, b = points[max(i - 1, 0)], points[min(i + 1, len(points) - 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        d = math.hypot(dx, dy) or 1
        w = (w0 + (w1 - w0) * i / (len(points) - 1)) / 2
        left.append((x - dy / d * w, y + dx / d * w))
        right.append((x + dy / d * w, y - dx / d * w))
    return path(left + right[::-1])


def trunk():
    left = [(408, GROUND + 6), (446, GROUND - 14), (466, GROUND - 70), (470, 1020), (474, 900), (480, 800), (486, 700)]
    right = [(544, 700), (548, 800), (552, 900), (556, 1020), (560, GROUND - 70), (582, GROUND - 16), (626, GROUND + 6)]
    return path(left + right + [(560, GROUND + 14), (512, GROUND + 18), (462, GROUND + 14)])


LIMBS = [
    ([(500, 760), (470, 690), (420, 620), (360, 560)], 40, 14),
    ([(528, 750), (566, 676), (622, 606), (690, 540)], 38, 14),
    ([(514, 720), (512, 620), (506, 520), (500, 420)], 36, 12),
    ([(470, 690), (420, 700), (330, 660)], 18, 8),
]


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}


def wood(shape, axis, seed):
    return {"op": "paint", "layer": "wood", "in": [shape], "color": COLOURS["bark"], "form": "cylinder", "axis": axis,
            "light": LIGHT, "key": KEY, "shadow": "#4a4660", "shadowMix": 0.5, "weight": 9, "decisive": 0.6, "seed": seed}


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out += [
        layer("sketch", "vector", visible=False),
        layer("ground", "vector"),
        layer("wood", "paint"),
        layer("canopy", "paint"),
        layer("grass", "paint"),
    ]
    sketch = [("trunk", trunk())] + [(f"limb{i}", limb(*l)) for i, l in enumerate(LIMBS)]
    sketch += [(cid, blob(cx, cy, rx, ry, seed)) for cid, cx, cy, rx, ry, _, seed in CLUMPS]
    sketch += [(f"{cid}-core", blob(cx, cy + 6, rx * 0.84, ry * 0.8, seed)) for cid, cx, cy, rx, ry, _, seed in CLUMPS]
    sketch.append(("crown", blob(510, 500, 430, 330, 41)))
    sketch.append(("base", path([(380, GROUND - 6), (650, GROUND - 6), (650, GROUND + 26), (380, GROUND + 26)])))
    out += [{"op": "add", "node": {"id": sid, "type": "path", "d": d}, "in": "sketch"} for sid, d in sketch]
    out.append({"op": "add", "node": {"type": "ellipse", "cx": 520, "cy": GROUND + 8, "rx": 300, "ry": 34, "fill": "#1d2a1666",
                                      "blur": 14}, "in": "ground"})
    out.append({"op": "add", "node": {"type": "ellipse", "cx": 500, "cy": GROUND + 4, "rx": 120, "ry": 16, "fill": "#14200f99",
                                      "blur": 6}, "in": "ground"})
    for sid in ["trunk"] + [f"limb{i}" for i in range(len(LIMBS))]:
        out.append({"op": "fill", "layer": "wood", "shape": sid, "color": "bark", "expand": 1})
    out.append({"op": "set", "id": "wood", "lockAlpha": True})
    out.append(wood("trunk", 92, 2))
    out += [wood(f"limb{i}", [125, 52, 92, 160][i], 3 + i) for i in range(len(LIMBS))]
    out.append({"op": "stroke", "layer": "wood", "brush": "soft", "size": 90, "color": "#2b2622", "opacity": 0.55,
                "path": "M512 720 L 512 900", "pressure": [1, 0.7, 0.1]})
    rnd = random.Random(31)
    for i in range(46):
        t = rnd.random()
        x = 470 + 84 * t + rnd.uniform(-4, 4)
        top, bottom = rnd.uniform(760, 1000), rnd.uniform(1040, GROUND - 10)
        lit = t < 0.4 and rnd.random() < 0.7
        points = [(x + 3 * math.sin(y * 0.03 + i), y) for y in range(int(top), int(bottom), 24)]
        out.append({"op": "stroke", "layer": "wood", "brush": "dry-brush", "size": rnd.uniform(3, 7),
                    "color": "#8a735c" if lit else "#2f261f", "opacity": rnd.uniform(0.3, 0.6),
                    "path": path(points, False), "pressure": [0.2, 0.9, 1, 0.8, 0.15]})
    out.append({"op": "set", "id": "wood", "lockAlpha": False})
    out.append({"op": "set", "id": "wood", "effects": [{"relief": {"light": LIGHT, "strength": 1.6, "softness": 1.6}}]})
    for cid, cx, cy, rx, ry, colour, seed in CLUMPS:
        out.append({"op": "fill", "layer": "canopy", "shape": f"{cid}-core", "color": "deep", "expand": 0})
        out.append({"op": "foliage", "layer": "canopy", "in": [cid], "color": COLOURS[colour], "light": LIGHT,
                    "leaf": 17, "mass": "crown", "seed": seed})
    out.append({"op": "scatter", "layer": "grass", "in": ["base"], "mark": "M0 0 Q 2 -16 6 -30", "count": 70,
                "size": [0.7, 1.4], "turn": [-25, 25], "weight": 4, "brush": "ink", "color": "grass", "seed": 5})
    return out


if __name__ == "__main__":
    print("\n".join(json.dumps(op) for op in ops()))
