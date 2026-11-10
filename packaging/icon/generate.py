import json
import random
import sys

INK = "#1d2230"
INK_DRY = "#39425c"
EMBER = "#d4612c"
EMBER_DRY = "#f09a48"
POINT = "#1aa7f2"
SCALE = 0.84
SHIFT = -28


def place(points):
    return [(round(512 + (x - 512) * SCALE + SHIFT, 1), round(512 + (y - 512) * SCALE, 1)) for x, y in points]


STEM = place([(392, 206), (384, 400), (394, 610), (372, 826)])
ARM = place([(760, 214), (716, 330), (624, 436), (404, 520)])
LEG = place([(404, 520), (486, 546), (571, 592), (628, 640), (685, 688), (748, 752), (782, 824)])


def brush(fringe, grain):
    return {
        "format": "klad-brush/1", "name": "Icon brush", "tip": {"round": {"hardness": 0.92}}, "spacing": 0.03,
        "roundness": 0.85, "angle": "follow", "dynamics": {"size": {"pressure": {"min": 0.05}}},
        "grain": {"paper": {"seed": 29, "feature": 4}, "strength": grain, "pressure": 0.6}, "fringe": fringe,
        "body": 0.8, "color": {"pressure": {"min": 0.0}},
    }


BRUSHES = {"icon-wet": brush(0.3, 0.14), "icon-dry": brush(0.75, 0.3)}


def curves(points):
    return [[points[i], *points[i + 1:i + 4]] for i in range(0, len(points) - 1, 3)]


def d(points):
    rest = " ".join("C " + " ".join(f"{x} {y}" for x, y in curve[1:]) for curve in curves(points))
    return f"M{points[0][0]} {points[0][1]} {rest}"


def at(curve, t):
    u = 1 - t
    return tuple(u ** 3 * curve[0][k] + 3 * u * u * t * curve[1][k] + 3 * u * t * t * curve[2][k] + t ** 3 * curve[3][k]
                 for k in (0, 1))


def along(points, steps=36):
    pieces = curves(points)
    return [at(curve, i / steps) for curve in pieces for i in range(steps)] + [pieces[-1][3]]


def offset(line, by):
    def normal(i):
        a, b = line[max(i - 1, 0)], line[min(i + 1, len(line) - 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        size = (dx * dx + dy * dy) ** 0.5 or 1
        return -dy / size, dx / size
    return [(round(x + normal(i)[0] * by, 1), round(y + normal(i)[1] * by, 1)) for i, (x, y) in enumerate(line)]


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}


def node(layer, **props):
    return {"op": "add", "node": props, "in": layer}


def stroke(path, size, colour, second, pressure, brush="icon-wet", **extra):
    return {"op": "stroke", "layer": "paint", "brush": brush, "size": round(size * SCALE, 1), "color": colour,
            "second": second, "path": path, "pressure": pressure, "taper": [0, 0], **extra}


def streaks(points, width, tones, seed, count, heavy):
    rng = random.Random(seed)
    line = along(points)

    def one(i):
        start = rng.uniform(0, 0.55)
        end = min(start + rng.uniform(0.3, 0.7), 0.97)
        first = int(start * len(line))
        moved = offset(line[first:max(int(end * len(line)), first + 4)], width * heavy * SCALE * rng.uniform(-0.38, 0.38))
        tone = tones[rng.random() < 0.55]
        return stroke("M" + " L".join(f"{x} {y}" for x, y in moved), width * rng.uniform(0.08, 0.22), tone, tone,
                      [0.2, 0.8, 1, 0.8, 0.5, 0.15], brush="icon-dry", opacity=round(rng.uniform(0.16, 0.34), 2),
                      seed=seed * 100 + i)
    return [one(i) for i in range(count)]


def paint(heavy):
    ember = ["#e77a33", "#b94412"]
    ink = ["#343c55", "#11141d"]
    lock = {"op": "set", "id": "paint", "lockAlpha": True}
    free = {"op": "set", "id": "paint", "lockAlpha": False}
    return [
        stroke(d(ARM + LEG[1:]), 156 * heavy, EMBER, EMBER_DRY, [0.16, 0.36, 0.5, 0.56, 0.58, 0.7, 0.9, 1, 0.98, 0.8, 0.5, 0.2]),
        stroke(d(LEG[3:]), 148 * heavy, EMBER, EMBER_DRY, [0.96, 0.9, 0.66, 0.34, 0.1], brush="icon-dry"),
        lock,
        *streaks(LEG, 130, ember, 4, 9, heavy),
        *streaks(ARM, 74, ember, 5, 5, heavy),
        free,
        stroke(d(STEM), 140 * heavy, INK, INK_DRY, [0.86, 1, 0.98, 0.96, 0.92, 0.84, 0.7]),
        stroke("M" + " L".join(f"{x} {y}" for x, y in along(STEM)[20:]), 132 * heavy, INK, INK_DRY,
               [0.94, 0.9, 0.78, 0.5, 0.22], brush="icon-dry"),
        lock,
        *streaks(STEM, 120, ink, 6, 8, heavy),
        free,
        {"op": "set", "id": "paint", "effects": [{"relief": {"light": [-0.6, -0.8], "strength": 3.6, "softness": 4}}]},
    ]


def points():
    line = {"paint": POINT, "width": 5, "cap": "round", "join": "round"}
    handles = (LEG[2], LEG[4])
    anchors = [(ARM[0], False), (ARM[3], False), (LEG[6], False), (LEG[3], True)]
    return [
        layer("points", "vector"),
        node("points", type="path", d=d(ARM + LEG[1:]), fill="none", stroke=line),
        node("points", type="path", d=f"M{handles[0][0]} {handles[0][1]} L{handles[1][0]} {handles[1][1]}", fill="none",
             stroke=line),
        *[node("points", type="ellipse", cx=x, cy=y, rx=12.5, ry=12.5, fill="#ffffff", stroke={"paint": POINT, "width": 5})
          for x, y in handles],
        *[node("points", type="rect", x=x - 16, y=y - 16, w=32, h=32, fill=POINT if chosen else "#ffffff",
               stroke={"paint": "#ffffff" if chosen else POINT, "width": 5}) for (x, y), chosen in anchors],
        {"op": "set", "id": "points", "effects": [{"shadow": {"x": 0, "y": 3, "radius": 5, "color": "#00000050"}}]},
    ]


def ops(small):
    paper = {"linear": {"from": [0, 100], "to": [0, 924], "stops": [[0, "#f9f2e5"], [1, "#e8d9c0"]]}}
    rim = {"linear": {"from": [0, 100], "to": [0, 924],
                      "stops": [[0, "#ffffffd0"], [0.35, "#ffffff00"], [0.7, "#00000000"], [1, "#0000001c"]]}}
    return [
        layer("plate", "vector"),
        node("plate", type="rect", x=100, y=100, w=824, h=824, radius=186, fill=paper),
        node("plate", type="rect", x=102, y=102, w=820, h=820, radius=184, fill="none", stroke={"paint": rim, "width": 4}),
        {"op": "set", "id": "plate", "effects": [{"shadow": {"x": 0, "y": 14, "radius": 22, "color": "#00000044"}}]},
        layer("paint", "paint", clip=True),
        *paint(1.34 if small else 1),
        *([] if small else points()),
    ]


if __name__ == "__main__":
    if sys.argv[1:2] == ["--brush"]:
        print(json.dumps(BRUSHES[sys.argv[2]]))
    else:
        print("\n".join(json.dumps(op) for op in ops("--small" in sys.argv)))
