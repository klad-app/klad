import json
import sys

POST = ("M431 1088 C 419 950 451 760 435 600 C 419 470 425 380 449 330 C 481 260 600 250 646 300 "
        "C 668 328 674 362 664 392 L 650 388 C 658 366 654 338 636 318 C 600 280 506 286 488 336 "
        "C 466 384 462 470 478 600 C 494 760 466 950 474 1088 Z")
POD = "M650 480 C 702 508 775 562 772 650 C 770 730 720 785 650 788 C 580 785 528 730 528 650 C 526 562 598 508 650 480 Z"
STONE = ("M300 1180 C 298 1122 362 1088 452 1083 C 548 1078 646 1084 702 1112 C 742 1132 748 1176 722 1196 "
         "C 600 1206 420 1206 322 1200 C 307 1198 300 1191 300 1180 Z")
STONE_SIDE = ("M301 1172 C 380 1162 600 1160 744 1162 C 744 1180 736 1192 722 1196 "
              "C 600 1206 420 1206 322 1200 C 307 1198 300 1191 301 1172 Z")


def ring(cx, cy, rx, ry):
    return (f"M{cx} {cy - ry} C {cx + rx * 1.33} {cy - ry} {cx + rx * 1.33} {cy + ry} {cx} {cy + ry} "
            f"C {cx - rx * 1.33} {cy + ry} {cx - rx * 1.33} {cy - ry} {cx} {cy - ry} Z")


SHAPES = {
    "post": {"type": "path", "d": POST},
    "knob": {"type": "ellipse", "cx": 657, "cy": 392, "rx": 10, "ry": 10},
    "edge-link": {"type": "path", "d": "M649 419 C 649 415 655 415 655 419 L 654 436 C 654 440 648 440 648 436 Z"},
    "bead": {"type": "ellipse", "cx": 650, "cy": 456, "rx": 7, "ry": 5},
    "cap": {"type": "path", "d": "M624 484 C 624 466 676 466 676 484 C 664 492 636 492 624 484 Z"},
    "pod": {"type": "path", "d": POD},
    "window": {"type": "ellipse", "cx": 650, "cy": 655, "rx": 50, "ry": 70},
    "stone": {"type": "path", "d": STONE},
    "stone-side": {"type": "path", "d": STONE_SIDE},
    "left-tuft": {"type": "rect", "x": 292, "y": 1192, "w": 44, "h": 8},
    "right-tuft": {"type": "rect", "x": 690, "y": 1190, "w": 52, "h": 8},
}
FLATS = [("post", "iron"), ("knob", "iron"), ("edge-link", "chain"), ("bead", "chain"), ("cap", "iron"),
         ("pod", "bone"), ("window", "glow"), ("stone", "stone"), ("stone-side", "stone dark")]
LINKS = [ring(654, 410, 5.5, 8), ring(651, 445, 5.5, 8)]
DETAILS = [
    "M620 500 C 568 560 562 720 612 768", "M680 500 C 732 560 738 720 688 768", "M650 492 L 650 574", "M650 736 L 650 770",
    "M650 790 C 654 818 646 846 626 852 C 612 856 604 846 612 838",
    "M522 1090 L 536 1110 L 528 1132 L 540 1150", "M640 1094 C 660 1108 668 1124 688 1132",
]
COLOURS = {"ink": "#15161c", "iron": "#3b4352", "bone": "#d3cab3", "glow": "#a9ecff", "stone": "#6b7584",
           "stone dark": "#4a5260", "chain": "#283039", "shadow": "#56607e", "rim": "#e2f5ff"}
LIGHT = [-0.6, -0.8]


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}


def lit(shapes, depth, soft, rim_width, strength=0.8):
    return {"op": "light", "from": shapes, "light": LIGHT, "shade": "shade", "rim": "rim", "depth": depth, "soft": soft,
            "shadeColor": COLOURS["shadow"], "rimColor": COLOURS["rim"], "rimWidth": rim_width, "shadeOpacity": strength}


def glow():
    halo = {"radial": {"center": [650, 655], "radius": 260, "stops": [[0, "#7fd6ff66"], [0.45, "#3f9fd022"], [1, "#2a6f8800"]]}}
    core = {"radial": {"center": [650, 668], "radius": 64, "stops": [[0, "#f4feffff"], [0.5, "#bff0ff88"], [1, "#6fd0ff00"]]}}
    motes = [(560, 560, 4), (742, 520, 3), (770, 712, 5), (590, 830, 3), (708, 452, 2.5), (520, 700, 2.5)]
    return [
        {"op": "add", "node": {"type": "ellipse", "cx": 650, "cy": 655, "rx": 260, "ry": 260, "fill": halo}, "in": "halo"},
        {"op": "add", "node": {"type": "ellipse", "cx": 650, "cy": 668, "rx": 64, "ry": 64, "fill": core}, "in": "light"},
    ] + [
        {"op": "add", "node": {"type": "ellipse", "cx": x, "cy": y, "rx": r, "ry": r, "fill": "#dff8ffcc", "blur": r * 0.8}, "in": "light"}
        for x, y, r in motes
    ]


def ink(path=None, shapes=None, weight=3.5, seed=1, color="ink"):
    op = {"op": "ink", "layer": "lines", "weight": weight, "light": LIGHT, "color": color, "seed": seed}
    return {**op, "path": path} if path else {**op, "from": shapes}


PAINTED = "--painted" in sys.argv


def painted():
    def paint(shapes, colour, seed, **extra):
        return {"op": "paint", "layer": "rim", "in": shapes, "color": COLOURS[colour], "light": LIGHT, "seed": seed, **extra}
    return [
        paint(["post"], "iron", 1, form="cylinder", axis=86, weight=7),
        paint(["cap"], "iron", 2, weight=5),
        paint(["pod"], "bone", 3, weight=11),
        {"op": "fill", "layer": "rim", "shape": "window", "color": COLOURS["glow"], "expand": 2},
        paint(["stone"], "stone", 4, form="flat", facing=[0, -1, 0.8], angle=8, weight=11),
        paint(["stone-side"], "stone dark", 5, form="flat", facing=[0, 0.3, 1], angle=4, weight=9, brush="dry-brush", decisive=0.5),
    ]


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out += [
        layer("sketch", "vector", visible=False),
        layer("halo", "vector", blend="screen"),
        layer("flats", "paint"),
        layer("shade", "paint", blend="multiply", opacity=0.8, clip=True),
        layer("rim", "paint", clip=True),
        layer("lines", "paint"),
        layer("light", "vector", blend="add"),
    ]
    out += [{"op": "add", "node": {**node, "id": sid}, "in": "sketch"} for sid, node in SHAPES.items()]
    out += [{"op": "fill", "layer": "flats", "shape": sid, "color": c, "expand": 2} for sid, c in FLATS]
    if PAINTED:
        out += painted()
    else:
        out += [lit(["post", "cap", "stone"], 22, 2, 5), lit(["pod"], 44, 1, 6, 1.0)]
        out.append({"op": "hatch", "layer": "shade", "in": ["stone-side"], "angle": 78, "spacing": 16, "length": [8, 14],
                    "weight": 2, "color": "shadow", "opacity": 0.55, "seed": 5})
    out.append(ink(shapes=["knob", "cap", "pod", "window", "stone"], weight=9, seed=3))
    out.append(ink(path=POST.removesuffix(" Z"), weight=9, seed=2))
    out.append(ink(shapes=["edge-link", "bead"], weight=3, seed=9))
    out += [ink(path=d, weight=3, seed=10 + i) for i, d in enumerate(LINKS)]
    out += [ink(path=d, weight=3.5, seed=4 + i) for i, d in enumerate(DETAILS)]
    out.append(ink(path="M310 1170 C 420 1160 600 1158 738 1160", weight=2.5, seed=20))
    out += [{"op": "scatter", "layer": "lines", "in": [tuft], "avoid": [], "mark": mark, "count": 7,
             "size": [0.9, 1.6], "turn": turn, "weight": 5, "color": "ink", "seed": 6 + i}
            for i, (tuft, mark, turn) in enumerate([("left-tuft", "M0 0 Q -5 -30 -14 -58", [-30, 4]),
                                                    ("right-tuft", "M0 0 Q 5 -30 14 -58", [-4, 30])])]
    return out + glow()


if __name__ == "__main__":
    print("\n".join(json.dumps(op) for op in ops()))
