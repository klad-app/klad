import json
import math

W, H = 640, 560
GROUND = 500.0
IDLE, RUN = 8, 8
LIGHT = [-0.6, -0.8]
COLOURS = {"ink": "#121319", "shell": "#33405a", "shell far": "#232b3c", "belly": "#5c6a80", "face": "#a9b4c2",
           "eye": "#ffb24a", "iron": "#262c38", "glow": "#ffd27a", "shade": "#2a2f4a", "rim": "#cfe0f0",
           "warm rim": "#ffbf6a", "ground": "#8c86a8"}


def lerp(a, b, t):
    return a + (b - a) * t


def pose(frame):
    if frame < IDLE:
        t = frame / IDLE
        breath = math.sin(2 * math.pi * t)
        return {"x": 300, "y": 388 + 3 * breath, "lean": 0.0, "sx": 1 + 0.015 * breath, "sy": 1 - 0.02 * breath,
                "antenna": 4 * math.sin(2 * math.pi * t + 1.0), "swing": 6 * math.sin(2 * math.pi * t - 0.9) + 4,
                "near": (312, GROUND), "far": (291, GROUND - 2), "lift": (0, 0), "blink": 1.0 if frame == 5 else 0.0,
                "run": 0.0, "t": t}
    t = (frame - IDLE) / RUN
    bob = 8 * math.cos(4 * math.pi * t)

    def foot(phase):
        p = phase % 1.0
        if p < 0.5:
            u = p / 0.5
            return (300 + lerp(46, -50, u), GROUND), 0.0
        u = (p - 0.5) / 0.5
        return (300 + lerp(-50, 46, u * u * (3 - 2 * u)), GROUND - 34 * math.sin(math.pi * u) ** 0.8), math.sin(math.pi * u)

    near, near_lift = foot(t)
    far, far_lift = foot(t + 0.5)
    return {"x": 306, "y": 380 + bob, "lean": -9 + 2.5 * math.sin(4 * math.pi * t), "sx": 1 + 0.03 * math.cos(4 * math.pi * t),
            "sy": 1 - 0.035 * math.cos(4 * math.pi * t), "antenna": -22 + 9 * math.sin(4 * math.pi * t - 1.2),
            "swing": 24 + 16 * math.sin(2 * math.pi * t - 1.4), "near": near, "far": far, "lift": (near_lift, far_lift),
            "blink": 0.0, "run": 1.0, "t": t}


def body(p):
    a = math.radians(p["lean"])
    ca, sa = math.cos(a), math.sin(a)

    def world(u, v):
        x, y = u * p["sx"], v * p["sy"]
        return (p["x"] + x * ca - y * sa, p["y"] + x * sa + y * ca)

    return world


def top(u):
    k = min(1.0, abs(u) / 112)
    return -96 * (1 - k ** 2.6) ** (1 / 2.6) - 6 * (u / 112)


def bottom(u):
    return 34 * (1 - (u / 112) ** 2)


def path(points, closed=True):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)
    return d + (" Z" if closed else "")


def shell(world):
    us = [-112 + 224 * i / 48 for i in range(49)]
    return [world(u, top(u)) for u in us] + [world(u, bottom(u)) for u in reversed(us)]


def belly(world):
    us = [-104 + 208 * i / 32 for i in range(33)]
    return [world(u, bottom(u) - 15 * (1 - (u / 112) ** 2) ** 0.5) for u in us] + [world(u, bottom(u)) for u in reversed(us)]


def ellipse(world, cx, cy, rx, ry, turn=0.0, steps=36):
    a = math.radians(turn)
    return [world(cx + rx * math.cos(s) * math.cos(a) - ry * math.sin(s) * math.sin(a),
                  cy + rx * math.cos(s) * math.sin(a) + ry * math.sin(s) * math.cos(a))
            for s in [2 * math.pi * i / steps for i in range(steps)]]


def ik(hip, foot, upper, lower):
    dx, dy = foot[0] - hip[0], foot[1] - hip[1]
    d = min(math.hypot(dx, dy), upper + lower - 0.5)
    base = math.atan2(dy, dx)
    bend = math.acos(max(-1, min(1, (upper * upper + d * d - lower * lower) / (2 * upper * d))))
    knee_a = base - bend
    return (hip[0] + upper * math.cos(knee_a), hip[1] + upper * math.sin(knee_a))


def capsule(points, widths):
    left, right = [], []
    for i, (x, y) in enumerate(points):
        a = points[max(0, i - 1)]
        b = points[min(len(points) - 1, i + 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        n = math.hypot(dx, dy) or 1
        nx, ny = -dy / n, dx / n
        w = widths[i] / 2
        left.append((x + nx * w, y + ny * w))
        right.append((x - nx * w, y - ny * w))
    return left + list(reversed(right))


def bezier(a, c, b, steps=12):
    return [((1 - t) ** 2 * a[0] + 2 * (1 - t) * t * c[0] + t * t * b[0], (1 - t) ** 2 * a[1] + 2 * (1 - t) * t * c[1] + t * t * b[1])
            for t in [i / steps for i in range(steps + 1)]]


def head(world):
    points = []
    for i in range(48):
        a = 2 * math.pi * i / 48
        bump = 0.32 * math.exp(-((math.atan2(math.sin(a - 0.55), math.cos(a - 0.55))) ** 2) / 0.12)
        r = 1 + bump
        points.append(world(106 + 42 * r * math.cos(a), 16 + 34 * r * math.sin(a) * (0.9 if math.sin(a) < 0 else 1)))
    return points


def leg(world, hip_uv, foot, lift):
    hip = world(*hip_uv)
    ankle = (foot[0], foot[1] - 10)
    knee = ik(hip, ankle, 41, 41)
    thigh = [(lerp(hip[0], knee[0], k), lerp(hip[1], knee[1], k)) for k in [i / 6 for i in range(7)]]
    shin = [(lerp(knee[0], ankle[0], k), lerp(knee[1], ankle[1], k)) for k in [i / 6 for i in range(1, 7)]]
    line = thigh + shin
    widths = [lerp(26, 17, i / 6) for i in range(7)] + [lerp(17, 10, i / 6) for i in range(1, 7)]
    toe = (ankle[0] + 18, foot[1] - 3 - 8 * lift)
    a = math.degrees(math.atan2(toe[1] - ankle[1], toe[0] - ankle[0]))
    return capsule(line, widths), ellipse(lambda u, v: (u, v), (ankle[0] + toe[0]) / 2 + 2, (ankle[1] + toe[1]) / 2 + 2, 17, 8, a, 24)


def antenna(world, base_uv, angle, length, curl):
    base = world(*base_uv)
    a = math.radians(angle - 50)
    tip = (base[0] + length * math.cos(a), base[1] + length * math.sin(a))
    mid = (lerp(base[0], tip[0], 0.5) - curl * math.sin(a), lerp(base[1], tip[1], 0.5) + curl * math.cos(a))
    return bezier(base, mid, tip, 14), tip


def shapes(f):
    p = pose(f)
    world = body(p)
    s = {}
    s["shell"] = shell(world)
    s["belly"] = belly(world)
    s["head"] = head(world)
    blink = p["blink"]
    s["eye"] = ellipse(world, 124, 6, 7.5, 7.5 * (1 - 0.85 * blink), 0, 20)
    s["near leg"], s["near foot"] = leg(world, (18, 34), p["near"], p["lift"][0])
    s["far leg"], s["far foot"] = leg(world, (-20, 30), p["far"], p["lift"][1])
    hook_base = world(-52, -86)
    hook = world(-140, -174)
    swing = math.radians(p["swing"])
    hang = (hook[0] + 34 * math.sin(swing), hook[1] + 34 * math.cos(swing))
    turn = -p["swing"] * 0.6
    s["lantern"] = ellipse(lambda u, v: (u, v), hang[0], hang[1] + 6, 20, 25, turn, 30)
    s["window"] = ellipse(lambda u, v: (u, v), hang[0], hang[1] + 8, 11, 15, turn, 24)
    s["cap"] = ellipse(lambda u, v: (u, v), hang[0] - 22 * math.sin(math.radians(turn)), hang[1] - 18, 11, 6, turn, 16)
    lines = {
        "pole": bezier(hook_base, world(-66, -136), world(-82, -178), 10) + bezier(world(-82, -178), world(-100, -206), world(-132, -192), 8)[1:]
        + bezier(world(-132, -192), world(-142, -186), world(-140, -174), 4)[1:],
        "chain": [hook, (lerp(hook[0], hang[0], 0.5), lerp(hook[1], hang[1], 0.5) - 2), (hang[0] - 22 * math.sin(math.radians(turn)), hang[1] - 22)],
    }
    seams, lips = [], []
    for u in (-78, -46, -14, 18, 50, 80):
        line = [world(u - 16 * k * k + 6 * (1 - k), lerp(top(u), bottom(u) - 14, k)) for k in [i / 10 for i in range(11)]]
        seams.append(line)
        lips.append([world(u + 5 - 16 * k * k + 6 * (1 - k), lerp(top(u + 5), bottom(u) - 16, k)) for k in [i / 10 for i in range(8)]])
    near_antenna, near_tip = antenna(world, (122, -12), p["antenna"], 92, -16)
    far_antenna, far_tip = antenna(world, (106, -16), p["antenna"] - 14, 82, -14)
    mouth = [world(150, 40), world(140, 44), world(130, 43)]
    return p, s, lines, (seams, lips), (near_antenna, near_tip, far_antenna, far_tip), mouth, hang


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, "cels": [{"id": f"{id.replace(' ', '-')}0", "frame": 0, "hold": 1}], **extra}}


def node(f, id, points, layer_id="sketch"):
    return {"op": "add", "node": {"id": f"{id.replace(' ', '-')}-{f}", "type": "path", "d": path(points)}, "in": layer_id, "frame": f}


def sid(f, name):
    return f"{name.replace(' ', '-')}-{f}"


def frame_ops(f):
    p, s, lines, (seams, lips), ants, mouth, hang = shapes(f)
    out = [node(f, k, v) for k, v in s.items()]
    flats = [("far leg", "shell far", "far"), ("far foot", "shell far", "far"), ("shell", "shell", "flats"), ("belly", "belly", "flats"),
             ("head", "face", "front"), ("eye", "eye", "front"),
             ("near leg", "shell", "flats"), ("near foot", "shell", "flats"),
             ("lantern", "iron", "flats"), ("cap", "iron", "flats"), ("window", "glow", "flats")]
    out += [{"op": "fill", "layer": lay, "shape": sid(f, k), "color": c, "expand": 1.5, "frame": f} for k, c, lay in flats]
    out.append({"op": "light", "from": [sid(f, "shell")], "light": LIGHT, "shade": "shade", "rim": "rim", "depth": 34, "soft": 3,
                "shadeColor": COLOURS["shade"], "rimColor": COLOURS["rim"], "rimWidth": 5, "shadeOpacity": 0.85, "frame": f})
    out.append({"op": "light", "from": [sid(f, "near leg"), sid(f, "belly")], "light": LIGHT, "shade": "shade",
                "rim": "rim", "depth": 12, "soft": 2, "shadeColor": COLOURS["shade"], "rimColor": COLOURS["rim"], "rimWidth": 3,
                "shadeOpacity": 0.7, "frame": f})
    out.append({"op": "light", "from": [sid(f, "head")], "light": LIGHT, "shade": "front-shade", "rim": "front-rim", "depth": 14,
                "soft": 2, "shadeColor": COLOURS["shade"], "rimColor": COLOURS["rim"], "rimWidth": 3, "shadeOpacity": 0.7, "frame": f})
    out.append({"op": "light", "from": [sid(f, "shell")], "lamp": [hang[0], hang[1] + 6, 40], "rim": "rim",
                "rimColor": COLOURS["warm rim"], "rimWidth": 4, "rimOpacity": 0.75, "frame": f})
    ink = lambda **kw: {"op": "ink", "layer": "lines", "light": LIGHT, "color": "ink", "frame": f, **kw}
    out.append(ink(**{"from": [sid(f, "far leg"), sid(f, "far foot")], "weight": 5, "seed": 3 + f}))
    out.append(ink(**{"from": [sid(f, "shell"), sid(f, "near leg"), sid(f, "near foot")], "weight": 7, "seed": 11 + f}))
    out.append(ink(**{"from": [sid(f, "head")], "weight": 7, "seed": 13 + f, "layer": "front-lines"}))
    out.append(ink(**{"from": [sid(f, "eye")], "weight": 3.2, "seed": 17 + f, "layer": "front-lines"}))
    out.append(ink(**{"from": [sid(f, "lantern"), sid(f, "cap")], "weight": 3.2, "seed": 19 + f}))
    out.append(ink(path=path(lines["pole"], False), weight=7, seed=23 + f, taper=[4, 6], **{"break": 170, "wobble": 0.3}))
    out.append(ink(path=path(lines["chain"], False), weight=2.4, seed=29 + f))
    out += [ink(path=path(seam, False), weight=3.6, seed=31 + f + i, taper=[4, 18]) for i, seam in enumerate(seams)]
    out += [{"op": "stroke", "layer": "rim", "frame": f, "brush": "ink", "size": 3.2, "color": "rim", "opacity": 0.45,
             "path": path(lip, False), "pressure": [0.3, 1, 0.6, 0.1], "taper": [6, 24]} for lip in lips]
    near_antenna, near_tip, far_antenna, far_tip = ants
    out.append(ink(path=path(far_antenna, False), weight=4.2, seed=41 + f, taper=[2, 0], wobble=0))
    out.append(ink(path=path(near_antenna, False), weight=5.2, seed=43 + f, taper=[2, 0], wobble=0))
    out.append(ink(path=path(mouth, False), weight=2.2, seed=47 + f, taper=[3, 4], layer="front-lines"))
    out += [{"op": "add", "node": {"type": "ellipse", "cx": round(tip[0], 1), "cy": round(tip[1], 1), "rx": r, "ry": r, "fill": COLOURS["ink"]},
             "in": "tips", "frame": f} for tip, r in ((near_tip, 6), (far_tip, 5))]
    squash = 1 - 0.25 * (GROUND - (p["y"] + 112)) / 40
    out.append({"op": "add", "node": {"type": "ellipse", "cx": round(p["x"] - 6, 1), "cy": GROUND + 3, "rx": round(96 * squash, 1), "ry": 11,
                                      "fill": "#0b0b1870", "blur": 6}, "in": "ground", "frame": f})
    halo = {"radial": {"center": [round(hang[0], 1), round(hang[1] + 6, 1)], "radius": 150,
                       "stops": [[0, "#ffc06a55"], [0.4, "#ff9d4a1c"], [1, "#ff8a3a00"]]}}
    core = {"radial": {"center": [round(hang[0], 1), round(hang[1] + 6, 1)], "radius": 26,
                       "stops": [[0, "#fff6dcff"], [0.55, "#ffd27a77"], [1, "#ffb24a00"]]}}
    out.append({"op": "add", "node": {"type": "ellipse", "cx": round(hang[0], 1), "cy": round(hang[1] + 6, 1), "rx": 150, "ry": 150, "fill": halo},
                "in": "halo", "frame": f})
    out.append({"op": "add", "node": {"type": "ellipse", "cx": round(hang[0], 1), "cy": round(hang[1] + 6, 1), "rx": 26, "ry": 26, "fill": core},
                "in": "light", "frame": f})
    return out


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out += [layer("sketch", "vector", visible=False), layer("ground", "vector"), layer("halo", "vector", blend="screen"),
            layer("far", "paint"), layer("flats", "paint"), layer("shade", "paint", blend="multiply", opacity=0.85, clip=True),
            layer("rim", "paint", clip=True), layer("lines", "paint"), layer("tips", "vector"), layer("front", "paint"),
            layer("front-shade", "paint", blend="multiply", opacity=0.85, clip=True), layer("front-rim", "paint", clip=True),
            layer("front-lines", "paint"), layer("light", "vector", blend="add")]
    names = ["sketch", "ground", "halo", "far", "flats", "shade", "rim", "lines", "tips", "front", "front-shade", "front-rim",
             "front-lines", "light"]
    out += [{"op": "cel", "new": n, "frame": f, "hold": 1, "id": f"{n.replace(' ', '-')}{f}"} for f in range(1, IDLE + RUN) for n in names]
    for f in range(IDLE + RUN):
        out += frame_ops(f)
    return out


print("\n".join(json.dumps(o) for o in ops()))
