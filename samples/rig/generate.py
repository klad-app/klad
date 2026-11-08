import json
import math
import sys

INK = "#1f1d26"
SKIN = "#e2b48c"
COAT = "#3f6f8f"
COAT_DARK = "#2c4f66"


def layer(id, kind, within=None, **extra):
    op = {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}
    if within:
        op["in"] = within
    return op


def ellipse(cx, cy, rx, ry, n=40):
    return [[round(cx + rx * math.cos(2 * math.pi * k / n), 1), round(cy + ry * math.sin(2 * math.pi * k / n), 1)] for k in range(n)]


def fill(layer, points, colour, part):
    return {"op": "fill", "layer": layer, "polygon": points, "color": colour, "part": part}


def stroke(layer, path, size, colour, part, brush="ink"):
    return {"op": "stroke", "layer": layer, "brush": brush, "size": size, "color": colour, "path": path, "part": part,
            "pressure": [0.6, 1, 1, 0.8]}


def drawing():
    out = [
        {"op": "timeline", "frames": 24, "fps": 12},
        layer("sky", "paint", depth=3),
        layer("figure", "group"),
        layer("colour", "paint", "figure"),
        layer("lines", "paint", "figure"),
        layer("bones", "bones", "figure"),
        layer("lantern", "vector"),
        fill("sky", [[0, 560], [800, 540], [800, 800], [0, 800]], "#6f7f6a", "hill"),
        fill("sky", ellipse(620, 160, 60, 60), "#f4e4b0", "moon"),
        fill("colour", ellipse(400, 470, 90, 130), COAT, "body"),
        fill("colour", ellipse(400, 290, 62, 66), SKIN, "head"),
        stroke("colour", "M452 390 L 560 360", 44, COAT_DARK, "upper arm", "round-bristle"),
        stroke("colour", "M560 360 L 650 330", 36, COAT_DARK, "forearm", "round-bristle"),
        fill("colour", ellipse(672, 322, 22, 20), SKIN, "hand"),
        stroke("lines", "M400 340 C 300 360 300 590 400 600 C 500 590 500 360 400 340", 6, INK, "body"),
        stroke("lines", "M400 224 C 330 228 330 352 400 356 C 470 352 470 228 400 224", 5, INK, "head"),
        stroke("lines", "M452 368 L 560 338 M 456 412 L 562 384", 4, INK, "upper arm"),
        stroke("lines", "M560 342 L 650 312 M 562 380 L 652 350", 4, INK, "forearm"),
        stroke("lines", "M376 286 L 382 300 M 420 286 L 426 300", 5, INK, "head"),
        {"op": "add", "node": {"id": "flame", "type": "ellipse", "cx": 170, "cy": 260, "rx": 20, "ry": 28, "fill": "#ffcc66",
                               "blur": 6}, "in": "lantern"},
        {"op": "add", "node": {"id": "lamp", "type": "rect", "x": 150, "y": 230, "w": 40, "h": 64, "radius": 8,
                               "fill": "none", "stroke": {"paint": INK, "width": 4}}, "in": "lantern"},
    ]
    return out


def rig():
    bone = lambda name, parent, head, tail: {"op": "bone", "layer": "bones", "add": name, "head": head, "tail": tail,
                                            **({"parent": parent} if parent else {})}
    return [
        bone("body", None, [400, 590], [400, 360]),
        bone("head", "body", [400, 360], [400, 230]),
        bone("upper", "body", [452, 392], [560, 360]),
        bone("fore", "upper", [560, 360], [650, 330]),
        bone("hand", "fore", [650, 330], [694, 316]),
        {"op": "attach", "layer": "colour", "skeleton": "bones"},
        {"op": "attach", "layer": "lines", "skeleton": "bones"},
    ]


def weights():
    return [
        {"op": "weights", "layer": "bones", "auto": True},
        {"op": "weights", "layer": "bones", "bone": "head", "part": "head"},
        {"op": "weights", "layer": "bones", "bone": "body", "part": "body"},
        {"op": "weights", "layer": "bones", "bone": "hand", "part": "hand"},
    ]


def poses():
    wave = [(4, {"upper": -35, "fore": -55, "hand": -10}), (8, {"upper": -50, "fore": -20, "hand": 25}),
            (12, {"upper": -35, "fore": -55, "hand": -10}), (16, {"upper": -50, "fore": -20, "hand": 25})]
    return [{"op": "pose", "layer": "bones", "from": 0, "to": f, "bones": b, "hold": 8 if f == 16 else 4} for f, b in wave]


def keys():
    return [
        {"op": "key", "id": "lantern", "frame": 0, "y": 0, "ease": "in-out"},
        {"op": "key", "id": "lantern", "frame": 12, "y": -18, "ease": "in-out"},
        {"op": "key", "id": "lantern", "frame": 23, "y": 0},
        {"op": "key", "id": "flame", "frame": 0, "opacity": 1, "ease": "in-out"},
        {"op": "key", "id": "flame", "frame": 6, "opacity": 0.55, "ease": "in-out"},
        {"op": "key", "id": "flame", "frame": 12, "opacity": 1},
        {"op": "clip", "add": "wave", "from": 0, "to": 23, "loop": True},
    ]


if __name__ == "__main__":
    part = sys.argv[1] if len(sys.argv) > 1 else "drawing"
    ops = {"drawing": drawing, "rig": rig, "weights": weights, "poses": poses, "keys": keys}[part]()
    print("\n".join(json.dumps(op) for op in ops))
