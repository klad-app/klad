import json
import math
import random

W, H = 1920, 1080
FLOOR = 822.0
LAMP = (1142.0, 438.0)
COLOURS = {
    "deep top": "#161c2c", "deep": "#273148", "far": "#3f4c66", "back": "#1b2232", "stone": "#5b6477",
    "stone dark": "#3a4153", "floor": "#4d5466", "floor front": "#222834", "near": "#090c13", "ink": "#0d1018",
    "root": "#141821", "moss": "#4d6158", "iron": "#22252c", "amber": "#ffbf6e", "key": "#ffc983",
    "fill": "#6276a3", "shadow": "#4c5a86", "mote": "#ffd9a8",
}


SKETCH_CENTRES = {}
SKETCH_CORNERS = {}


def noise(seed, octaves=3):
    rnd = random.Random(seed)
    tables = [[rnd.uniform(-1, 1) for _ in range(512)] for _ in range(octaves)]

    def at(t):
        total, amp, norm = 0.0, 1.0, 0.0
        for o in range(octaves):
            x = t * (2 ** o)
            i = math.floor(x)
            f = x - i
            f = f * f * (3 - 2 * f)
            a, b = tables[o][i % 512], tables[o][(i + 1) % 512]
            total += (a + (b - a) * f) * amp
            norm += amp
            amp *= 0.5
        return total / norm

    return at


def path(points, closed=True):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)
    return d + (" Z" if closed else "")


def layer(id, kind, into=None, **extra):
    op = {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}
    return {**op, "in": into} if into else op


def node(into, id, points, **extra):
    n = {"type": "path", "d": path(points), **extra}
    if id:
        n["id"] = id
    return {"op": "add", "node": n, "in": into}


def radial(center, radius, stops):
    return {"radial": {"center": [round(center[0], 1), round(center[1], 1)], "radius": radius, "stops": stops}}


def linear(a, b, stops):
    return {"linear": {"from": list(a), "to": list(b), "stops": stops}}


def edge(a, b, steps, amp, seed, freq=0.08):
    n = noise(seed)
    length = math.hypot(b[0] - a[0], b[1] - a[1]) or 1
    nx, ny = -(b[1] - a[1]) / length, (b[0] - a[0]) / length
    out = []
    for i in range(steps + 1):
        t = i / steps
        d = n(t * length * freq) * amp * math.sin(math.pi * t) ** 0.3
        out.append((a[0] + (b[0] - a[0]) * t + nx * d, a[1] + (b[1] - a[1]) * t + ny * d))
    return out


def chipped(corners, seed, chip=7.0, amp=1.4):
    rnd = random.Random(seed)
    out = []
    count = len(corners)
    for i in range(count):
        a, b = corners[i], corners[(i + 1) % count]
        length = math.hypot(b[0] - a[0], b[1] - a[1])
        cut_a = min(length * 0.3, rnd.uniform(1.5, chip))
        cut_b = min(length * 0.3, rnd.uniform(1.5, chip))
        ux, uy = (b[0] - a[0]) / length, (b[1] - a[1]) / length
        start = (a[0] + ux * cut_a, a[1] + uy * cut_a)
        end = (b[0] - ux * cut_b, b[1] - uy * cut_b)
        steps = max(2, int(length / 9))
        line = edge(start, end, steps, amp, seed * 31 + i, 0.15)
        if length > 40 and rnd.random() < 0.35:
            k = rnd.randint(1, steps - 1)
            nx, ny = -uy, ux
            depth = rnd.uniform(2.5, 6.0)
            line[k] = (line[k][0] - nx * depth, line[k][1] - ny * depth)
        out += line
    return out


def blocks_column(x0, x1, top, seed, broken=False):
    rnd = random.Random(seed)
    out = []
    y = FLOOR
    row = 0
    while y > top + 20:
        h = rnd.uniform(34, 70) if row else rnd.uniform(56, 72)
        y_top = max(top, y - h)
        kind = rnd.random()
        if kind < 0.4:
            cuts = [x0 + rnd.uniform(-5, 4), x1 + rnd.uniform(-4, 5)]
        elif kind < 0.85:
            m = rnd.uniform(0.3, 0.7)
            cuts = [x0 + rnd.uniform(-4, 3), x0 + (x1 - x0) * m, x1 + rnd.uniform(-3, 4)]
        else:
            cuts = [x0 + rnd.uniform(-3, 3), x0 + (x1 - x0) * rnd.uniform(0.22, 0.38), x0 + (x1 - x0) * rnd.uniform(0.6, 0.78), x1 + rnd.uniform(-3, 3)]
        for i in range(len(cuts) - 1):
            a, b = cuts[i] + 2, cuts[i + 1] - 2
            lean = rnd.uniform(-2, 2)
            sag = rnd.uniform(-2.5, 2.5)
            corners = [(a + lean, y_top + 2 + sag), (b + lean, y_top + 2 - sag), (b, y - 2), (a, y - 2)]
            out.append(corners)
        y = y_top
        row += 1
    if broken:
        last = out.pop()
        x_a, x_b = last[3][0], last[2][0]
        base_y = last[3][1]
        for k in range(4):
            w = (x_b - x_a) * rnd.uniform(0.22, 0.4)
            x = x_a + (x_b - x_a - w) * rnd.uniform(0, 1)
            h = rnd.uniform(14, 34)
            out.append([(x, base_y - h * rnd.uniform(0.6, 1)), (x + w * rnd.uniform(0.5, 0.8), base_y - h), (x + w, base_y - h * 0.4), (x + w, base_y - 1), (x, base_y - 1)])
    return out


def voussoirs(centre, inner, outer, a0, a1, count, seed):
    rnd = random.Random(seed)
    out = []
    span = (a1 - a0) / count
    for i in range(count):
        s, e = a0 + span * i + 0.6 / inner * 57.3 * 0.03, a0 + span * (i + 1) - 0.6 / inner * 57.3 * 0.03
        s, e = math.radians(s), math.radians(e)
        r_out = outer + rnd.uniform(-6, 8)
        pts = [(centre[0] + inner * math.cos(s), centre[1] - inner * math.sin(s)),
               (centre[0] + r_out * math.cos(s), centre[1] - r_out * math.sin(s)),
               (centre[0] + r_out * math.cos(e), centre[1] - r_out * math.sin(e)),
               (centre[0] + inner * math.cos(e), centre[1] - inner * math.sin(e))]
        out.append(pts)
    return out


def settle(points, x):
    cx = sum(p[0] for p in points) / len(points)
    low = max(p[1] for p in points)
    return [(px - cx + x, py - low + FLOOR + 5) for px, py in points]


def turned(points, centre, degrees, shift):
    a = math.radians(degrees)
    ca, sa = math.cos(a), math.sin(a)
    return [(centre[0] + (x - centre[0]) * ca - (y - centre[1]) * sa + shift[0],
             centre[1] + (x - centre[0]) * sa + (y - centre[1]) * ca + shift[1]) for x, y in points]


def tower(x, w, h, base, seed):
    rnd = random.Random(seed)
    top = base - h
    left = edge((x, base), (x + rnd.uniform(-6, 6), top + rnd.uniform(0, 30)), 24, 3, seed)
    broken = []
    steps = 10
    for i in range(steps + 1):
        t = i / steps
        bx = x + w * t
        by = top + rnd.uniform(-14, 26) + (40 if 0.35 < t < 0.65 and rnd.random() < 0.8 else 0)
        broken.append((bx, by))
    right = edge((x + w + rnd.uniform(-6, 6), broken[-1][1]), (x + w, base), 24, 3, seed + 1)
    return left + broken + right


def far():
    out = [node("deep", None, [(0, 0), (W, 0), (W, H), (0, H)],
                fill=linear((0, 0), (0, H), [[0, COLOURS["deep top"]], [0.55, COLOURS["deep"]], [1, "#222a3e"]]))]
    out.append({"op": "add", "node": {"type": "ellipse", "cx": LAMP[0], "cy": LAMP[1] + 40, "rx": 900, "ry": 640,
                                      "fill": radial((LAMP[0], LAMP[1] + 40), 900, [[0, "#56668466"], [0.5, "#3a486230"], [1, "#27314800"]])},
                "in": "deep"})
    rnd = random.Random(4)
    for i, (x, w, h) in enumerate([(120, 120, 430), (420, 90, 300), (690, 140, 380), (1560, 110, 340), (1760, 150, 470)]):
        out.append(node("far-v", f"tower{i}", tower(x, w, h, 860, 40 + i), fill=COLOURS["far"], blur=1.2))
        for k in range(rnd.randint(1, 3)):
            wx, wy = x + w * rnd.uniform(0.3, 0.6), 860 - h * rnd.uniform(0.4, 0.8)
            out.append({"op": "add", "node": {"type": "path", "d": path([(wx - 9, wy + 26), (wx - 9, wy), (wx - 5, wy - 9), (wx, wy - 11),
                                                                          (wx + 5, wy - 9), (wx + 9, wy), (wx + 9, wy + 26)]),
                                              "fill": "#4a5772", "blur": 0.8}, "in": "far-v"})
    ceiling = [(0, 0), (W, 0)]
    n = noise(9)
    band = [(W - x, 120 + 40 * n(x * 0.004) + (60 * max(0, math.sin(x * 0.021 + 1)) ** 6)) for x in range(0, W + 1, 12)]
    out.append(node("far-v", "far-ceiling", ceiling + band, fill="#4b5873", blur=1.5))
    out.append(node("far-v", "far-floor", [(0, 870)] + [(x, 850 + 16 * n(x * 0.006 + 30)) for x in range(0, W + 1, 16)] + [(W, H), (0, H)],
                    fill="#55627c", blur=1))
    return out


def spike(x, top, length, width, seed, lean=0.0):
    rnd = random.Random(seed)
    left = [(x - width / 2 + rnd.uniform(-2, 2), top)]
    right = [(x + width / 2 + rnd.uniform(-2, 2), top)]
    steps = 9
    for i in range(1, steps + 1):
        t = i / steps
        w = width / 2 * (1 - t) ** 1.25 * (1 + 0.25 * math.sin(t * 7 + seed))
        cx = x + lean * t * t * length
        left.append((cx - w + rnd.uniform(-1.5, 1.5), top + length * t))
        right.append((cx + w + rnd.uniform(-1.5, 1.5), top + length * t))
    return left + list(reversed(right))


def back():
    out = []
    n = noise(21)
    left = [(0, 0), (520, 0)]
    for i in range(41):
        t = i / 40
        y = t * H
        x = 380 + 120 * n(t * 3.0) + 90 * math.sin(t * 5.2) - 140 * t
        left.append((x, y))
    left += [(0, H)]
    out.append(node("back-v", "left-wall", left, fill=COLOURS["back"], blur=0.6))
    right = [(W, 0), (1500, 0)]
    for i in range(41):
        t = i / 40
        right.append((1600 - 80 * n(t * 3.3 + 7) + 120 * t + 40 * math.sin(t * 6.1), t * H))
    right += [(W, H)]
    out.append(node("back-v", "right-wall", right, fill=COLOURS["back"], blur=0.6))
    roof = [(0, 0), (W, 0)] + [(W - x, 150 + 34 * n(x * 0.005 + 50) + 30 * math.sin(x * 0.011)) for x in range(0, W + 1, 12)]
    out.append(node("back-v", "roof", roof, fill=COLOURS["back"], blur=0.6))
    rnd = random.Random(23)
    for i in range(16):
        x = rnd.uniform(380, 1700)
        top = 120 + 30 * n(x * 0.005 + 50)
        out.append(node("back-v", None, spike(x, top, rnd.uniform(60, 200), rnd.uniform(22, 52), 100 + i, rnd.uniform(-0.08, 0.08)),
                        fill=COLOURS["back"], blur=0.5))
    for i in range(6):
        x = rnd.uniform(420, 1650)
        out.append(node("back-v", None, chipped([(x, 868), (x + rnd.uniform(60, 140), 868 - rnd.uniform(4, 10)),
                                                  (x + rnd.uniform(70, 150), 870 + rnd.uniform(26, 60)), (x - 10, 870 + rnd.uniform(26, 60))],
                                                 300 + i, 14, 3), fill="#2d3649", blur=0.5))
    for i, (cx, span, height, keep) in enumerate(((560, 150, 300, 0.85), (1720, 120, 250, 0.55), (300, 110, 220, 0.4))):
        arch_pts = []
        for k in range(25):
            a = math.pi * (1 - k / 24 * keep)
            arch_pts.append((cx + span * math.cos(a), 868 - height - span * 0.9 * math.sin(a)))
        inner = [(cx + (span - 40) * math.cos(math.pi * (1 - k / 24 * keep)), 868 - height - (span - 40) * 0.9 * math.sin(math.pi * (1 - k / 24 * keep)))
                 for k in range(24, -1, -1)]
        left_leg = [(cx - span, 868), (cx - span, 868 - height)]
        out.append(node("back-v", None, left_leg + arch_pts + inner + [(cx - span + 40, 868 - height), (cx - span + 40, 868)],
                        fill="#222a3c", blur=0.8))
        if keep > 0.8:
            out.append(node("back-v", None, [(cx + span - 40, 868), (cx + span - 40, 868 - height), (cx + span, 868 - height), (cx + span, 868)],
                            fill="#222a3c", blur=0.8))
    out.append(node("back-v", "back-floor", [(0, 900)] + [(x, 872 + 12 * n(x * 0.01 + 80)) for x in range(0, W + 1, 20)] + [(W, H), (0, H)],
                    fill="#2b3446", blur=0.5))
    return out


def gate():
    out, sketch = [], []
    left_blocks = blocks_column(862, 992, 336, 7)
    right_blocks = blocks_column(1292, 1422, 452, 8, broken=True)
    centre, inner, outer = (1142, 340), 150, 236
    arch = voussoirs(centre, inner, outer, 180, 62, 9, 11)
    fallen = [turned(v, (sum(p[0] for p in v) / 4, sum(p[1] for p in v) / 4), a, s)
              for v, a, s in zip(voussoirs(centre, inner, outer, 62, 0, 3, 12), (78, -32, 128), ((0, 0), (0, 0), (0, 0)))]
    fallen = [settle(v, x) for v, x in zip(fallen, (1488, 1578, 1690))]
    pieces = {"left": left_blocks, "right": right_blocks, "arch": arch, "fallen": fallen}
    ids = {}
    for name, polys in pieces.items():
        ids[name] = []
        for i, corners in enumerate(polys):
            sid = f"{name}{i}"
            SKETCH_CENTRES[sid] = (sum(p[0] for p in corners) / len(corners), sum(p[1] for p in corners) / len(corners))
            SKETCH_CORNERS[sid] = corners
            sketch.append(node("sketch", sid, chipped(corners, sum(map(ord, sid)) * 7 + i, 8 if name != "fallen" else 12, 1.6)))
            ids[name].append(sid)
    floor_top = [(-20, FLOOR + 8)] + [(x, FLOOR - 4 + 6 * noise(70)(x * 0.01)) for x in range(0, W + 41, 24)] + [(W + 20, FLOOR + 40), (-20, FLOOR + 40)]
    floor_front = [(-20, FLOOR + 34)] + [(x, FLOOR + 30 + 8 * noise(71)(x * 0.008)) for x in range(0, W + 41, 24)] + [(W + 20, H + 20), (-20, H + 20)]
    sketch += [node("sketch", "floor-top", floor_top), node("sketch", "floor-front", floor_front)]
    out += sketch
    paint = lambda lay, shapes, colour, seed, **kw: {"op": "paint", "layer": lay, "in": shapes, "color": colour, "lamp": [LAMP[0], LAMP[1], 110],
                                                     "key": COLOURS["key"], "fill": COLOURS["fill"], "fillStrength": 0.28,
                                                     "bounce": "#6b5a4a", "bounceStrength": 0.1, "shadow": COLOURS["shadow"], "shadowMix": 0.6,
                                                     "seed": seed, **kw}
    for lay, shapes, colour in (("floor", ["floor-top"], "floor"), ("floor", ["floor-front"], "floor front"),
                                ("left-pillar", ids["left"], "stone"), ("right-pillar", ids["right"], "stone"),
                                ("arch", ids["arch"], "stone"), ("fallen", ids["fallen"], "stone dark")):
        out += [{"op": "fill", "layer": lay, "shape": s, "color": colour, "expand": 0.5} for s in shapes]
    for lay in ("floor", "left-pillar", "right-pillar", "arch", "fallen"):
        out.append({"op": "set", "id": lay, "lockAlpha": True})
    out.append(paint("floor", ["floor-top"], "floor", 5, form="flat", facing=[0, -1, 0.35], angle=1, weight=16, decisive=0.7))
    lip = [(x, FLOOR + 30 + 8 * noise(71)(x * 0.008)) for x in range(700, 1600, 12)]
    out.append({"op": "stroke", "layer": "floor-rocks", "brush": "soft", "size": 7, "color": "#e0a868", "opacity": 0.5,
                "path": path(lip, False), "pressure": [0.05, 0.6, 1, 0.6, 0.05]})
    out.append(paint("floor", ["floor-front"], "floor front", 6, form="flat", facing=[0, 0.2, 1], angle=88, weight=18, decisive=0.3, brush="dry-brush"))
    stone = dict(brush="chalk", bold="dry-brush", vary=0.07)
    out.append(paint("left-pillar", ids["left"], "stone", 7, form="flat", facing=[0, 0, 1], angle=38, weight=8, decisive=1, **stone))
    out.append(paint("right-pillar", ids["right"], "stone", 8, form="flat", facing=[0, 0, 1], angle=-36, weight=8, decisive=1, **stone))
    out.append(paint("arch", ids["arch"], "stone", 9, form="flat", facing=[0, 0.4, 1], angle=62, weight=8, decisive=1, **stone))
    out.append(paint("fallen", ids["fallen"], "stone dark", 10, form="flat", facing=[0, -0.6, 1], angle=24, weight=7, decisive=0.8, **stone))
    for lay in ("floor", "left-pillar", "right-pillar", "arch", "fallen"):
        out.append({"op": "set", "id": lay, "lockAlpha": False})
    reliefs = {"left-pillar": [1, -0.25], "right-pillar": [-1, -0.25], "arch": [0, 1], "fallen": [-0.6, -0.8], "floor": [0, -1]}
    out += [{"op": "set", "id": lay, "effects": [{"relief": {"light": light, "strength": 1.6, "highlight": "#ffd9a0", "shade": "#141826"}}]}
            for lay, light in reliefs.items()]
    near = lambda s: math.hypot(*(a - b for a, b in zip(centre_of(s), LAMP)))
    for name in ("left", "right", "arch"):
        out.append({"op": "light", "from": ids[name], "lamp": [LAMP[0], LAMP[1], 60], "shade": f"{name}-shade",
                    "depth": 7, "soft": 2, "shadeColor": "#2c3046", "shadeOpacity": 0.9})
        for lo, hi, strength in ((0, 230, 0.65), (230, 380, 0.38), (380, 9999, 0.16)):
            band = [s for s in ids[name] if lo <= near(s) < hi]
            if band:
                out.append({"op": "light", "from": band, "lamp": [LAMP[0], LAMP[1], 60], "rim": f"{name}-rim", "rimColor": "#ffb867",
                            "rimWidth": 4 if lo == 0 else 3, "rimOpacity": strength})
    rnd = random.Random(808)
    for sid in [s for name in ("left", "right", "arch") for s in ids[name] if near(s) < 300]:
        if rnd.random() < 0.45:
            cx, cy = centre_of(sid)
            x, y = cx + rnd.uniform(-30, 30), cy + rnd.uniform(-12, 12)
            pts = [(x, y)]
            a = rnd.uniform(0, 2 * math.pi)
            for k in range(rnd.randint(3, 6)):
                a += rnd.uniform(-0.9, 0.9)
                step = rnd.uniform(5, 11)
                x, y = x + step * math.cos(a), y + step * math.sin(a)
                pts.append((x, y))
            out.append({"op": "stroke", "layer": "gate-ink", "brush": "ink", "size": rnd.uniform(1.6, 2.6), "color": "ink",
                        "path": path(pts, False), "pressure": [0.3, 1, 0.7, 0.1], "taper": [3, 10]})
    for sid in [s for s in ids["left"] + ids["right"]]:
        cx, cy = centre_of(sid)
        if rnd.random() < 0.55:
            top_y = min(p[1] for p in SKETCH_CORNERS[sid])
            width = (max(p[0] for p in SKETCH_CORNERS[sid]) - min(p[0] for p in SKETCH_CORNERS[sid])) * rnd.uniform(0.3, 0.8)
            x0 = cx - width / 2 + rnd.uniform(-10, 10)
            out.append({"op": "add", "node": {"type": "rect", "id": f"moss-{sid}", "x": round(x0, 1), "y": round(top_y - 3, 1),
                                              "w": round(width, 1), "h": 5}, "in": "sketch"})
            out.append({"op": "scatter", "layer": "moss", "in": [f"moss-{sid}"], "mark": "M0 0 Q 2 -5 1 -10", "count": int(width / 6),
                        "size": [0.6, 1.3], "turn": [-40, 40], "weight": 3, "color": "moss", "seed": int(cx + cy)})
    focus = [s for name in ("left", "right", "arch") for s in ids[name]]
    out.append({"op": "ink", "layer": "gate-ink", "from": [s for s in focus if near(s) < 330], "weight": 4.4,
                "light": [0, -1], "color": "ink", "seed": 3, "break": 50, "gap": 2})
    out.append({"op": "ink", "layer": "gate-ink", "from": [s for s in focus if near(s) >= 330] + ids["fallen"], "weight": 3, "light": [0, -1],
                "color": "ink", "seed": 4, "opacity": 0.75, "break": 50, "gap": 3})
    return out, ids


def centre_of(sid):
    return SKETCH_CENTRES[sid]


def lantern():
    out = []
    chain = [(LAMP[0], 196), (LAMP[0] + 2, 260), (LAMP[0] - 1, 340), (LAMP[0], LAMP[1] - 40)]
    hook = [(LAMP[0] - 7, 188), (LAMP[0] - 7, 200), (LAMP[0], 206), (LAMP[0] + 7, 200), (LAMP[0] + 7, 188)]
    out.append({"op": "ink", "layer": "lantern-ink", "path": path(hook, False), "weight": 4, "color": "ink", "seed": 29})
    out.append({"op": "ink", "layer": "lantern-ink", "path": path(chain, False), "weight": 3, "color": "ink", "seed": 30, "wobble": 0.6})
    for k, y in enumerate(range(214, int(LAMP[1] - 44), 22)):
        out.append({"op": "add", "node": {"type": "ellipse", "cx": LAMP[0] + (k % 2) * 1.5, "cy": y, "rx": 3.6 if k % 2 else 1.8, "ry": 6,
                                          "fill": "none", "stroke": {"paint": COLOURS["iron"], "width": 2.2}}, "in": "lantern-v"})
    cage = [(LAMP[0] - 30, LAMP[1] - 20), (LAMP[0] - 20, LAMP[1] - 40), (LAMP[0] + 20, LAMP[1] - 40), (LAMP[0] + 30, LAMP[1] - 20),
            (LAMP[0] + 26, LAMP[1] + 34), (LAMP[0] + 12, LAMP[1] + 46), (LAMP[0] - 12, LAMP[1] + 46), (LAMP[0] - 26, LAMP[1] + 34)]
    out.append({"op": "add", "node": {"type": "path", "d": path(cage), "fill": "#ffe2b0"}, "in": "lantern-v"})
    for dx in (-16, 0, 16):
        out.append({"op": "ink", "layer": "lantern-ink", "path": path([(LAMP[0] + dx * 0.8, LAMP[1] - 40), (LAMP[0] + dx * 1.3, LAMP[1] - 8),
                                                                        (LAMP[0] + dx * 0.9, LAMP[1] + 44)], False),
                    "weight": 3.2, "color": "ink", "seed": 31 + dx})
    out.append({"op": "ink", "layer": "lantern-ink", "path": path(cage), "weight": 4, "color": "ink", "seed": 34, "light": [0, -1]})
    cap = [(LAMP[0] - 34, LAMP[1] - 38), (LAMP[0] - 10, LAMP[1] - 54), (LAMP[0] + 10, LAMP[1] - 54), (LAMP[0] + 34, LAMP[1] - 38)]
    out.append({"op": "add", "node": {"type": "path", "d": path(cap), "fill": COLOURS["iron"]}, "in": "lantern-v"})
    return out


def roots():
    out = []
    rnd = random.Random(55)
    for i in range(5):
        a = math.radians(rnd.uniform(100, 172))
        x0, y0 = 1142 + 146 * math.cos(a), 340 - 146 * math.sin(a) - 4
        length = rnd.uniform(70, 190)
        sway = rnd.uniform(8, 20)
        pts = [(x0 + sway * math.sin(k * 0.5 + i) * (k / 10) ** 1.3, y0 + length * k / 10) for k in range(11)]
        out.append({"op": "stroke", "layer": "roots", "brush": "ink", "size": rnd.uniform(3, 6.5), "color": "root",
                    "path": path(pts, False), "pressure": [0.9, 0.8, 0.5, 0.15], "taper": [0, length * 0.6]})
    return out


def near():
    out = []
    n = noise(91)
    rock = [(-20, H + 20), (-20, 860)]
    for i in range(30):
        t = i / 29
        x = t * 520
        rock.append((x, 900 - 90 * math.sin(t * math.pi) ** 0.7 + 22 * n(t * 6) - 40 * (1 - t)))
    rock += [(560, H + 20)]
    out.append(node("near-v", "near-left", rock, fill=COLOURS["near"]))
    rock2 = [(W + 20, H + 20), (W + 20, 820)]
    for i in range(30):
        t = i / 29
        rock2.append((W - t * 380, 860 - 60 * math.sin(t * math.pi) ** 0.6 + 18 * n(t * 5 + 9) + 60 * t))
    rock2 += [(W - 420, H + 20)]
    out.append(node("near-v", "near-right", rock2, fill=COLOURS["near"]))
    hang = [(-20, -20), (460, -20)]
    for i in range(40):
        t = i / 39
        x = 460 - t * 480
        y = 40 + 30 * n(t * 7 + 3) + 220 * max(0, math.sin(t * 9.0)) ** 3 * (1 - t * 0.4) + 160 * t
        hang.append((x, y))
    out.append(node("near-v", "near-roof", hang, fill=COLOURS["near"]))
    rnd = random.Random(92)
    for i in range(5):
        x = rnd.uniform(40, 420)
        out.append(node("near-v", None, spike(x, 40 + 20 * n(x * 0.01), rnd.uniform(120, 300), rnd.uniform(26, 50), 500 + i,
                                              rnd.uniform(-0.1, 0.1)), fill=COLOURS["near"]))
    out.append({"op": "add", "node": {"type": "rect", "id": "tuft-left", "x": 40, "y": 836, "w": 460, "h": 30}, "in": "sketch"})
    out.append({"op": "add", "node": {"type": "rect", "id": "tuft-right", "x": W - 380, "y": 812, "w": 340, "h": 40}, "in": "sketch"})
    out += [{"op": "scatter", "layer": "near-ink", "in": [tuft], "mark": mark, "count": count, "size": [0.7, 1.7], "turn": turn,
             "weight": 6, "color": "near", "seed": 60 + i}
            for i, (tuft, mark, turn, count) in enumerate([("tuft-left", "M0 0 Q 4 -40 14 -84", [-35, 20], 34),
                                                           ("tuft-right", "M0 0 Q -4 -40 -14 -84", [-20, 35], 26)])]
    return out


def light():
    out = []
    out.append({"op": "add", "node": {"type": "ellipse", "cx": LAMP[0], "cy": LAMP[1], "rx": 620, "ry": 620,
                                      "fill": radial(LAMP, 620, [[0, "#ffb25e66"], [0.25, "#e9944a2a"], [0.6, "#8a6a7a10"], [1, "#00000000"]])},
                "in": "halo"})
    out.append({"op": "add", "node": {"type": "ellipse", "cx": LAMP[0], "cy": FLOOR + 6, "rx": 360, "ry": 46,
                                      "fill": radial((LAMP[0], FLOOR + 6), 360, [[0, "#ffc27a55"], [1, "#ffb05a00"]]), "blur": 8},
                "in": "halo"})
    out.append({"op": "add", "node": {"type": "ellipse", "cx": LAMP[0], "cy": LAMP[1] + 8, "rx": 70, "ry": 70,
                                      "fill": radial((LAMP[0], LAMP[1] + 8), 70, [[0, "#fff4dcff"], [0.35, "#ffd08acc"], [1, "#ffb05a00"]])},
                "in": "light"})
    rnd = random.Random(77)
    for i in range(34):
        a = rnd.uniform(0, 2 * math.pi)
        r = rnd.uniform(70, 420) ** 1.0
        x, y = LAMP[0] + r * math.cos(a) * 1.3, LAMP[1] + r * math.sin(a) * 0.8
        s = rnd.uniform(1.4, 3.8) * (1.2 - r / 520)
        out.append({"op": "add", "node": {"type": "ellipse", "cx": round(x, 1), "cy": round(y, 1), "rx": round(s, 2), "ry": round(s, 2),
                                          "fill": COLOURS["mote"], "blur": round(s * 0.6, 2), "opacity": round(0.4 + 0.6 * (1 - r / 420), 2)},
                    "in": "light"})
    return out


def ops():
    out = [{"op": "palette", "add": c, "name": n} for n, c in COLOURS.items()]
    out.append({"op": "atmosphere", "color": "#4f6080", "top": "#2a3550", "density": 0.3, "blur": 0.9, "desaturate": 0.12})
    out += [
        layer("sketch", "vector", visible=False),
        layer("far-plane", "group", depth=2.6),
        layer("deep", "vector", into="far-plane"),
        layer("far-v", "vector", into="far-plane"),
        layer("back-plane", "group", depth=1.3),
        layer("back-v", "vector", into="back-plane"),
        layer("gate-plane", "group", depth=0.25),
        layer("floor", "paint", into="gate-plane"),
        layer("floor-rocks", "paint", into="gate-plane"),
        layer("fallen", "paint", into="gate-plane"),
        layer("left-pillar", "paint", into="gate-plane"),
        layer("left-shade", "paint", into="gate-plane", clip=True, blend="multiply"),
        layer("left-rim", "paint", into="gate-plane", clip=True, blend="screen"),
        layer("right-pillar", "paint", into="gate-plane"),
        layer("right-shade", "paint", into="gate-plane", clip=True, blend="multiply"),
        layer("right-rim", "paint", into="gate-plane", clip=True, blend="screen"),
        layer("arch", "paint", into="gate-plane"),
        layer("arch-shade", "paint", into="gate-plane", clip=True, blend="multiply"),
        layer("arch-rim", "paint", into="gate-plane", clip=True, blend="screen"),
        layer("moss", "paint", into="gate-plane"),
        layer("roots", "paint", into="gate-plane"),
        layer("gate-ink", "paint", into="gate-plane"),
        layer("lantern-v", "vector", into="gate-plane"),
        layer("lantern-ink", "paint", into="gate-plane"),
        layer("halo", "vector", blend="screen"),
        layer("near-plane", "group"),
        layer("near-v", "vector", into="near-plane"),
        layer("near-ink", "paint", into="near-plane"),
        layer("light", "vector", blend="add"),
    ]
    out += far() + back()
    g, ids = gate()
    out += g + lantern() + roots() + near() + light()
    return out


if __name__ == "__main__":
    print("\n".join(json.dumps(o) for o in ops()))
