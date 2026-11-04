import json
import math

W, H = 1024, 768
FLOOR = 610
TOP = 190
R = 74
X = W / 2
FRAMES = 20

DRAWINGS = [
    (0, 2, 0.0),
    (2, 2, 0.12),
    (4, 1, 0.36),
    (5, 1, 0.56),
    (6, 1, 0.8),
    (7, 1, 1.0),
    (8, 1, 0.82),
    (9, 1, 0.6),
    (10, 1, 0.42),
    (11, 2, 0.22),
    (13, 2, 0.06),
]
REST = (15, 5)


def stretch(frame):
    return {6: (0.86, 1.18), 7: (1.32, 0.7), 8: (0.88, 1.15), 5: (0.94, 1.07), 9: (0.94, 1.06)}.get(frame, (1.0, 1.0))


def place(frame, depth):
    sx, sy = stretch(frame)
    y = TOP + (FLOOR - R * sy - TOP) * depth
    return X, y, R * sx, R * sy


def ellipse(cx, cy, rx, ry, steps=72, start=-math.pi / 2):
    return [
        (cx + rx * math.cos(start + 2 * math.pi * i / steps), cy + ry * math.sin(start + 2 * math.pi * i / steps))
        for i in range(steps + 1)
    ]


def path(points):
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)


def poly(points):
    return [[round(x, 1), round(y, 1)] for x, y in points]


def stripe(cx, cy, rx, ry, angle):
    ca, sa = math.cos(angle), math.sin(angle)
    points = []
    for i in range(33):
        t = -1.08 + 2.16 * i / 32
        u, v = t, 0.28 * (1 - t * t)
        points.append((cx + rx * (u * ca - v * sa), cy + ry * (u * sa + v * ca)))
    return points


def seeds(cx, cy, rx, ry, angle):
    ca, sa = math.cos(angle), math.sin(angle)
    near = (cx + rx * 0.55 * -sa, cy + ry * 0.55 * ca)
    far = (cx - rx * 0.5 * -sa, cy - ry * 0.5 * ca)
    return near, far


def ink(frame, points, size, pressure, taper):
    return {
        "op": "stroke",
        "layer": "lines",
        "frame": frame,
        "brush": "ink",
        "color": "ink",
        "size": size,
        "path": path(points),
        "pressure": pressure,
        "taper": taper,
    }


def ops():
    out = [
        {"op": "palette", "add": "#1d1a22", "name": "ink"},
        {"op": "palette", "add": "#d8543c", "name": "red"},
        {"op": "palette", "add": "#f2e3c4", "name": "cream"},
        {"op": "palette", "add": "#cdb58f", "name": "ground"},
        {"op": "palette", "add": "#3a3046", "name": "shade"},
    ]
    first, hold0, _ = DRAWINGS[0]
    for layer, name, extra in [
        ("floor", "floor", {}),
        ("shadow", "shadow", {"blend": "multiply", "opacity": 0.45}),
        ("colour", "colour", {}),
        ("lines", "lines", {}),
    ]:
        cels = [{"id": f"{layer}0", "frame": 0}] if layer == "floor" else [{"id": f"{layer}0", "frame": 0, "hold": hold0}]
        out.append({"op": "add", "layer": {"id": layer, "kind": "paint", "name": name, "cels": cels, **extra}})
    out.append(
        {
            "op": "fill",
            "layer": "floor",
            "polygon": poly([(0, FLOOR), (W, FLOOR), (W, H), (0, H)]),
            "color": "ground",
        }
    )
    out.append(
        {
            "op": "stroke",
            "layer": "floor",
            "brush": "ink",
            "color": "ink",
            "size": 5,
            "path": f"M40 {FLOOR + 1} L{W - 40} {FLOOR - 1}",
            "pressure": [0.5, 1, 1, 0.5],
            "taper": [60, 60],
        }
    )
    for frame, hold, depth in DRAWINGS:
        if frame != first:
            for layer in ("lines", "colour", "shadow"):
                out.append({"op": "cel", "new": layer, "frame": frame, "hold": hold, "id": f"{layer}{frame}"})
        cx, cy, rx, ry = place(frame, depth)
        angle = 0.35 + 2 * math.pi * frame / REST[0]
        out.append(ink(frame, ellipse(cx, cy, rx, ry), 7, [0.85, 1, 0.9, 1, 0.85], [0, 0]))
        out.append(ink(frame, stripe(cx, cy, rx, ry, angle), 5, [0.6, 1, 0.6], [6, 6]))
        near, far = seeds(cx, cy, rx, ry, angle)
        for (sx, sy), colour in ((near, "red"), (far, "cream")):
            out.append(
                {
                    "op": "fill",
                    "layer": "colour",
                    "frame": frame,
                    "at": [round(sx, 1), round(sy, 1)],
                    "ref": "lines",
                    "gap": 4,
                    "color": colour,
                }
            )
        spread = 0.45 + 0.55 * depth
        out.append(
            {
                "op": "fill",
                "layer": "shadow",
                "frame": frame,
                "polygon": poly(ellipse(cx, FLOOR + 6, R * 1.25 * spread, 14 * spread, 48)),
                "color": "shade",
            }
        )
    rest, rest_hold = REST
    for layer in ("lines", "colour", "shadow"):
        out.append({"op": "cel", "link": f"{layer}0", "frame": rest, "hold": rest_hold, "id": f"{layer}{rest}"})
    out.append({"op": "clip", "add": "bounce", "from": 0, "to": rest - 1, "loop": True})
    out.append({"op": "clip", "add": "rest", "from": rest, "to": rest + rest_hold - 1, "fps": 6})
    return out


if __name__ == "__main__":
    for op in ops():
        print(json.dumps(op))
