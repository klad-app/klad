import json
import math
import random
import sys

W, H = 1600, 760
LIGHT = [-0.9, -0.45]
COLOURS = {"sky top": "#7fa3c9", "sky low": "#f2dcb0", "hill far": "#8ea6a8", "hill": "#6f8f7c", "field": "#6f8a3c",
           "field deep": "#3e5426", "bank": "#55702f", "bank deep": "#2a3a1c", "tuft": "#6d8a37"}


def path(points, closed=True):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)
    return d + (" Z" if closed else "")


def ridge(x0, x1, y, amp, seed, step=20):
    rnd = random.Random(seed)
    phases = [rnd.uniform(0, math.tau) for _ in range(3)]
    return [(x, y + amp * (0.6 * math.sin(x * 0.004 + phases[0]) + 0.3 * math.sin(x * 0.011 + phases[1])
                           + 0.1 * math.sin(x * 0.03 + phases[2]))) for x in range(x0, x1 + step, step)]


def blade(root, length, width, lean, bend):
    left, right = [], []
    for i in range(13):
        t = i / 12
        x = root[0] + length * t
        y = root[1] + lean * length * t + bend * length * t * t
        w = width * (1 - t) ** 0.8 / 2
        left.append((x, y - w))
        right.append((x, y + w))
    return path(left + right[::-1])


def picture(name):
    rnd = random.Random(sum(map(ord, name)))
    blades = 7 if name == "tuft" else 5
    out = [{"op": "add", "layer": {"id": "p", "kind": "vector", "name": "p"}}]
    for i in range(blades):
        root = (14, 128 + rnd.uniform(-10, 10))
        length = rnd.uniform(150, 232)
        lean = (i - blades / 2) / blades * rnd.uniform(0.35, 0.6)
        out.append({"op": "add", "node": {"type": "path", "d": blade(root, length, rnd.uniform(9, 14), lean, rnd.uniform(-0.15, 0.15)),
                                          "fill": "#000000"}, "in": "p"})
    if name == "tuft-b":
        stalk = [(14 + t * 236, 128 - 6 * math.sin(t * 2)) for t in [k / 10 for k in range(11)]]
        out.append({"op": "add", "node": {"type": "path", "d": path(stalk, False), "fill": "none",
                                          "stroke": {"paint": "#000000", "width": 3}}, "in": "p"})
        out.append({"op": "add", "node": {"type": "ellipse", "cx": 226, "cy": 124, "rx": 22, "ry": 7, "fill": "#000000"}, "in": "p"})
    return out


def tuft_brush():
    return {"format": "draw-anything-brush/1", "name": "Tuft", "tip": {"image": {"image": "tuft", "more": ["tuft-b"]}},
            "spacing": 1.0, "angle": "follow", "jitter": {"size": 0.2, "angle": 0.15},
            "color": {"value": 0.05, "saturation": 0.04, "pressure": {"min": 0.0}}}


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out += [
        layer("sketch", "vector", visible=False),
        layer("sky", "vector"),
        layer("hills", "vector"),
        layer("field", "paint"),
        layer("bank", "paint"),
    ]
    sky = {"linear": {"from": [0, 0], "to": [0, 470], "stops": [[0, COLOURS["sky top"]], [1, COLOURS["sky low"]]]}}
    out.append({"op": "add", "node": {"type": "rect", "x": 0, "y": 0, "w": W, "h": H, "fill": sky}, "in": "sky"})
    far = ridge(-20, W + 20, 390, 40, 3)
    out.append({"op": "add", "node": {"type": "path", "d": path(far + [(W + 20, 520), (-20, 520)]), "fill": COLOURS["hill far"],
                                      "blur": 2}, "in": "hills"})
    near = ridge(-20, W + 20, 430, 26, 5)
    out.append({"op": "add", "node": {"type": "path", "d": path(near + [(W + 20, 540), (-20, 540)]), "fill": COLOURS["hill"],
                                      "blur": 1}, "in": "hills"})
    field = ridge(-20, W + 20, 470, 12, 7, 16) + [(W + 20, H + 20), (-20, H + 20)]
    crest = [(-20, 560), (60, 540), (180, 512), (300, 498), (420, 506), (560, 540), (720, 590), (900, 640), (1080, 690),
             (1260, 740), (1300, H + 20), (-20, H + 20)]
    out += [{"op": "add", "node": {"id": "field-shape", "type": "path", "d": path(field)}, "in": "sketch"},
            {"op": "add", "node": {"id": "bank-shape", "type": "path", "d": path(crest)}, "in": "sketch"}]
    out.append({"op": "fill", "layer": "field", "shape": "field-shape", "color": "field deep", "expand": 0})
    out.append({"op": "grass", "layer": "field", "in": ["field-shape"], "color": COLOURS["field"], "light": LIGHT, "blade": 30,
                "far": 0.22, "wind": 12, "seed": 11})
    out.append({"op": "fill", "layer": "bank", "shape": "bank-shape", "color": "bank deep", "expand": 0})
    kinds = [
        {"name": "fine", "share": 0.4, "width": 0.7, "curl": 0.12},
        {"name": "broad", "share": 0.22, "length": 0.72, "width": 1.7, "curl": 0.55, "hue": -10, "patchy": 0.6},
        {"name": "dry", "share": 0.12, "length": 0.9, "width": 0.55, "curl": 0.3, "color": "#a99d5c"},
        {"name": "heads", "share": 0.06, "length": 1.45, "width": 0.3, "curl": 0.1, "hue": 6, "head": "seed-head", "patchy": 0.7},
        {"name": "tufts", "share": 0.005, "brush": "tuft", "length": 2.4, "color": COLOURS["tuft"], "patchy": 0.8},
        {"name": "clover", "share": 0.16, "brush": "clover", "length": 0.5, "hue": -14, "low": True, "patchy": 0.85},
    ]
    out.append({"op": "grass", "layer": "bank", "in": ["bank-shape"], "color": COLOURS["bank"], "light": LIGHT, "blade": 58,
                "far": 0.8, "wind": 14, "edge": 1.5, "kinds": kinds, "seed": 13})
    return out


if __name__ == "__main__":
    if sys.argv[1:2] == ["--picture"]:
        print("\n".join(json.dumps(op) for op in picture(sys.argv[2])))
    elif sys.argv[1:2] == ["--brush"]:
        print(json.dumps(tuft_brush()))
    else:
        print("\n".join(json.dumps(op) for op in ops()))
