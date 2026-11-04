import json
import math
import sys

BODY = (512.0, 640.0)
GROUND = 820.0
DEPTH = 12.0
LAYERS = ["shadow", "far", "colour", "shade", "rim", "lines"]

IDLE = [
    (0, 2, {"bob": 0, "sx": 1.0, "sy": 1.0, "tilt": 0, "sprout": 0, "eye": 9, "near": (0, 0), "far": (0, 0)}),
    (2, 1, {"bob": 7, "sx": 1.04, "sy": 0.95, "tilt": 0, "sprout": -8, "eye": 5, "near": (0, 0), "far": (0, 0)}),
]
WALK = [
    (3, 2, {"bob": 8, "sx": 1.03, "sy": 0.97, "tilt": 2, "sprout": -3, "eye": 9, "near": (14, 0), "far": (-14, 0)}),
    (5, 2, {"bob": -6, "sx": 0.98, "sy": 1.03, "tilt": 5, "sprout": -8, "eye": 9, "near": (0, 0), "far": (4, 26)}),
    (7, 2, {"bob": 8, "sx": 1.03, "sy": 0.97, "tilt": -2, "sprout": 3, "eye": 9, "near": (-14, 0), "far": (14, 0)}),
    (9, 2, {"bob": -6, "sx": 0.98, "sy": 1.03, "tilt": -5, "sprout": 8, "eye": 9, "near": (4, 26), "far": (0, 0)}),
]
REORDERED = {7, 9}


def body(pose):
    tilt = math.radians(pose["tilt"])
    ct, st = math.cos(tilt), math.sin(tilt)

    def world(u, v):
        x, y = u * pose["sx"], 100 + (v - 100) * pose["sy"]
        return (BODY[0] + x * ct - y * st, BODY[1] + pose["bob"] + x * st + y * ct)

    return world


def oval(world, cx, cy, rx, ry, a0=0.0, a1=360.0, steps=40):
    return [
        world(cx + rx * math.cos(math.radians(a0 + (a1 - a0) * i / steps)), cy + ry * math.sin(math.radians(a0 + (a1 - a0) * i / steps)))
        for i in range(steps + 1)
    ]


def rim_y(u):
    return -10 + 14 * (1 - (u / 150) ** 2)


def path(points):
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)


def poly(points):
    return [[round(x, 1), round(y, 1)] for x, y in points]


def ink(frame, points, size, taper=(0, 0), pressure=(0.8, 1, 1, 0.85), layer="lines", color="ink", brush="ink", part="body"):
    return {
        "op": "stroke",
        "layer": layer,
        "frame": frame,
        "brush": brush,
        "color": color,
        "size": size,
        "path": path(points),
        "pressure": list(pressure),
        "taper": list(taper),
        "part": part,
    }


def leg(world, angle, placed, depth=0.0):
    step, lift = placed
    def belly(a):
        r = math.radians(a)
        return world(130 * math.cos(r), -6.5 + 106 * math.sin(r))

    hip = belly(angle)
    planted = BODY[0] + 130 * math.cos(math.radians(angle))
    f = (planted + step, GROUND - 16 - depth - lift)
    dx, dy = f[0] - hip[0], f[1] - hip[1]
    n = math.hypot(dx, dy) or 1.0
    d = (dx / n, dy / n)
    side = (-d[1], d[0])
    w = 19
    right = (f[0] - side[0] * w, f[1] - side[1] * w)
    left = (f[0] + side[0] * w, f[1] + side[1] * w)
    p1, p2 = belly(angle - 11), belly(angle + 11)
    down = [(p1[0] + (right[0] - p1[0]) * t / 6, p1[1] + (right[1] - p1[1]) * t / 6) for t in range(6)]
    turn = [
        (f[0] + w * (-side[0] * math.cos(a) + d[0] * math.sin(a)), f[1] + w * (-side[1] * math.cos(a) + d[1] * math.sin(a)))
        for a in (math.pi * i / 10 for i in range(11))
    ]
    up = [(left[0] + (p2[0] - left[0]) * t / 6, left[1] + (p2[1] - left[1]) * t / 6) for t in range(1, 7)]
    return down + turn + up, ((hip[0] + f[0]) / 2, (hip[1] + f[1]) / 2)


def sprout(world, sway):
    a = math.radians(sway)
    base = (0.0, -180.0)

    def turned(u, v):
        x, y = u - base[0], v - base[1]
        return world(base[0] + x * math.cos(a) - y * math.sin(a), base[1] + x * math.sin(a) + y * math.cos(a))

    stem = [turned(10 * math.sin(t * 2.4), -180 - 78 * t) for t in (i / 14 for i in range(15))]
    tip = (10 * math.sin(2.4), -258.0)
    leaf = [
        turned(tip[0] - 64 * t, tip[1] - 14 * t + s * 22 * math.sin(math.pi * t) ** 0.8)
        for s, ts in ((-1, range(17)), (1, range(16, -1, -1)))
        for t in (i / 16 for i in ts)
    ]
    vein = [turned(tip[0] - 14 - 36 * t, tip[1] - 3 - 8 * t) for t in (i / 8 for i in range(9))]
    bud = (10 * math.sin(0.5 * 2.4), -180 - 78 * 0.5)
    ux, uy = math.cos(math.radians(-32)), math.sin(math.radians(-32))

    def second(t, side):
        w = side * 15 * math.sin(math.pi * t) ** 0.8
        return turned(bud[0] + ux * 46 * t - uy * w, bud[1] + uy * 46 * t + ux * w)

    leaf2 = [second(t, s) for s, ts in ((-1, range(17)), (1, range(16, -1, -1))) for t in (i / 16 for i in ts)]
    vein2 = [second(0.22 + 0.5 * t, 0) for t in (i / 6 for i in range(7))]
    return stem, leaf, vein, turned(tip[0] - 32, tip[1] - 14), (leaf2, vein2, second(0.5, -0.5))


def drawing(frame, pose):
    world = body(pose)
    out = []
    dome = oval(world, 0, -10, 150, 170, 180, 360, 48)
    rim = [world(u, rim_y(u)) for u in (-150 + 300 * i / 30 for i in range(31))]
    belly = oval(world, 0, -6.5, 130, 106, 180, 0, 40)
    hood = [world(u, rim_y(u) + 54 * math.sin(math.pi * (u - 4) / 122) ** 0.7) for u in (4 + 122 * i / 24 for i in range(25))]
    eyes = [oval(world, 44, 31, 6, pose["eye"], 0, 360, 16), oval(world, 84, 29, 6, pose["eye"], 0, 360, 16)]
    ridges = [oval(world, 0, -10, 108, 124, a0, a1, 10) for a0, a1 in ((212, 236), (256, 284), (304, 328))]
    stem, leaf, vein, leaf_seed, (leaf2, vein2, leaf2_seed) = sprout(world, pose["sprout"])
    near, near_seed = leg(world, 70, pose["near"])
    far, _ = leg(world, 104, pose["far"], DEPTH)
    out += [
        {"op": "fill", "layer": "far", "frame": frame, "polygon": poly(far), "color": "far", "part": "far leg"},
        ink(frame, far, 7, pressure=(0.8, 1, 1, 0.8), layer="far", part="far leg"),
        ink(frame, dome, 9, pressure=(0.7, 1, 1, 1, 0.8)),
        ink(frame, rim, 6),
        ink(frame, belly, 8, pressure=(0.8, 1, 1, 0.8)),
        ink(frame, hood, 5, pressure=(0.6, 1, 1, 0.6)),
        ink(frame, eyes[0], 4, pressure=(1, 1)),
        ink(frame, eyes[1], 4, pressure=(1, 1)),
    ]
    out += [ink(frame, r, 4, (8, 10), (0.4, 1, 0.5)) for r in ridges]
    sprouting = [
        ink(frame, stem, 6, (0, 10), (1, 0.9, 0.7), part="sprout"),
        ink(frame, leaf, 5, pressure=(1, 1), part="leaf"),
        ink(frame, vein, 3, (2, 8), (0.8, 0.4), part="leaf"),
        ink(frame, near, 7, pressure=(0.8, 1, 1, 0.8), part="near leg"),
    ]
    second = [
        ink(frame, leaf2, 5, pressure=(1, 1), part="second leaf"),
        ink(frame, vein2, 3, (2, 6), (0.8, 0.4), part="second leaf"),
    ]
    out += second + sprouting if frame in REORDERED else sprouting + second
    shade_shell = oval(world, 0, -10, 175, 195, 290, 362, 16) + oval(world, 0, -10, 112, 138, 362, 290, 16)
    shade_belly = [world(-150 + 300 * i / 16, 70 - 34 * (i / 16) - 18 * math.sin(math.pi * i / 16)) for i in range(17)] + [
        world(150, 140),
        world(-150, 140),
    ]
    out += [
        {"op": "fill", "layer": "shade", "frame": frame, "polygon": poly(shade_shell), "color": "shade", "part": "body"},
        {"op": "fill", "layer": "shade", "frame": frame, "polygon": poly(shade_belly), "color": "shade", "part": "body"},
    ]
    out += [
        ink(frame, oval(world, 0, -10, 139, 158, 196, 246, 14), 7, (24, 30), (0.3, 1, 0.4), "rim", "rim"),
        ink(frame, oval(world, 0, -6.5, 119, 95, 148, 172, 8), 5, (10, 14), (0.3, 1, 0.3), "rim", "rim"),
    ]
    spread = 1 - pose["bob"] / 60
    shadow = [
        (BODY[0] + 150 * spread * math.cos(2 * math.pi * i / 32), GROUND + 3 + 15 * spread * math.sin(2 * math.pi * i / 32))
        for i in range(32)
    ]
    out.append({"op": "fill", "layer": "shadow", "frame": frame, "polygon": poly(shadow), "color": "ground", "part": "shadow"})
    seeds = {
        "shell": world(-40, -90),
        "body": world(-50, 40),
        "face": world(64, 44),
        "eye": world(44, 31),
        "eye2": world(84, 29),
        "leaf": leaf_seed,
        "leaf2": leaf2_seed,
        "leg": near_seed,
    }
    return out, seeds


def keys():
    out = [
        {"op": "palette", "add": "#17161d", "name": "ink"},
        {"op": "palette", "add": "#5f7d4c", "name": "moss"},
        {"op": "palette", "add": "#6b5643", "name": "bark"},
        {"op": "palette", "add": "#20263a", "name": "dusk"},
        {"op": "palette", "add": "#bff2ff", "name": "glow"},
        {"op": "palette", "add": "#8dbb6a", "name": "leaf"},
        {"op": "palette", "add": "#45372c", "name": "root"},
        {"op": "palette", "add": "#2a211b", "name": "far"},
        {"op": "palette", "add": "#44405a", "name": "shade"},
        {"op": "palette", "add": "#eef6cf", "name": "rim"},
        {"op": "palette", "add": "#3a3428", "name": "ground"},
    ]
    extra = {
        "shadow": {"blend": "multiply", "opacity": 0.35},
        "shade": {"blend": "multiply", "opacity": 0.7, "clip": True},
        "rim": {"clip": True},
    }
    first, hold, _ = IDLE[0]
    for layer in LAYERS:
        out.append(
            {"op": "add", "layer": {"id": layer, "kind": "paint", "name": layer, "cels": [{"id": f"{layer}{first}", "frame": first, "hold": hold}], **extra.get(layer, {})}}
        )
    for frame, hold, _ in IDLE[1:] + WALK:
        for layer in LAYERS:
            out.append({"op": "cel", "new": layer, "frame": frame, "hold": hold, "id": f"{layer}{frame}"})
    for frame, _, pose in IDLE + WALK:
        ops, seeds = drawing(frame, pose)
        out += ops
        if frame == first:
            colours = {"shell": "moss", "body": "bark", "face": "dusk", "eye": "glow", "eye2": "glow", "leaf": "leaf", "leg": "root", "leaf2": "leaf"}
            for name, colour in colours.items():
                x, y = seeds[name]
                gap = 2 if name.startswith("eye") else 4
                out.append({"op": "fill", "layer": "colour", "frame": frame, "at": [round(x, 1), round(y, 1)], "ref": "lines", "gap": gap, "color": colour})
    return out


def leaf(bundle):
    with open(f"{bundle}/doc.json") as f:
        doc = json.load(f)
    frames = {c["id"]: c["frame"] for layer in doc["layers"] for c in layer.get("cels", [])}
    out = []
    for key, _, pose in IDLE + WALK:
        ops, seeds = drawing(frames[f"lines{key}"], pose)
        out += [op for op in ops if op.get("part") == "second leaf"]
        if key == IDLE[0][0]:
            x, y = seeds["leaf2"]
            fill = {"op": "fill", "layer": "colour", "frame": frames[f"colour{key}"], "at": [round(x, 1), round(y, 1)]}
            out.append({**fill, "ref": "lines", "gap": 4, "color": "leaf"})
    return out


def redraw(bundle):
    with open(f"{bundle}/doc.json") as f:
        doc = json.load(f)
    frames = {c["id"]: c["frame"] for layer in doc["layers"] for c in layer.get("cels", [])}
    out = []
    for key, _, pose in IDLE + WALK:
        ops, _ = drawing(frames[f"lines{key}"], pose)
        drawn = [layer for layer in LAYERS if layer != "colour"]
        counts = {layer: sum(1 for op in ops if op.get("layer") == layer) for layer in drawn}
        out.append({"op": "rm", "ids": [f"{layer}{key}:s{i}" for layer in drawn for i in range(1, counts[layer] + 1)]})
        out += ops
    return out


if __name__ == "__main__":
    modes = {"redraw": redraw, "leaf": leaf}
    for op in modes[sys.argv[1]](sys.argv[2]) if sys.argv[1:2] and sys.argv[1] in modes else keys():
        print(json.dumps(op))
