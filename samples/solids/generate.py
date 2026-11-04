import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "docs", "skill"))
import drawkit

W, H = 1200, 900
F = 1500.0
EYE = (0.0, 34.0, 0.0)
HORIZON = 170.0
CX = 600.0
TABLE_Z = 165.0

L = (lambda v: tuple(c / math.sqrt(sum(x * x for x in v)) for c in v))((-0.47, 0.86, -0.21))
KEY = (1.0, 0.86, 0.68)
AMB = (0.040, 0.048, 0.066)
PLASTER = (0.82, 0.80, 0.76)
TERRACOTTA = (0.60, 0.29, 0.16)
CELADON = (0.40, 0.58, 0.50)
TABLE = (0.46, 0.39, 0.31)
WALL = (0.085, 0.088, 0.098)

SPHERE = {"c": (-4.0, 7.0, 82.0), "r": 7.0}
CUBE = {"c": (16.0, 0.0, 101.0), "s": 17.0, "yaw": math.radians(33)}
CYL = {"c": (-27.0, 0.0, 112.0), "r": 5.0, "h": 16.0}

rnd = random.Random(7)
drawkit.seed(7)


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def addv(a, b):
    return tuple(x + y for x, y in zip(a, b))


def mul(a, k):
    return tuple(x * k for x in a)


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def norm(v):
    d = math.sqrt(dot(v, v)) or 1.0
    return tuple(x / d for x in v)


CAMERA = drawkit.Camera(EYE, F, CX, HORIZON)


def project(p):
    return CAMERA.project(p)


def to_eye(p):
    return norm(sub(EYE, p))


def encode(c):
    c = max(0.0, min(1.0, c))
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def hexc(rgb):
    return "#" + "".join(f"{round(encode(c) * 255):02x}" for c in rgb)


def value(rgb):
    y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
    return 10 * max(0.0, y) ** (1 / 3)


def jitter(rgb, amount):
    k = 1 + rnd.uniform(-amount, amount)
    warm = rnd.uniform(-amount, amount) * 0.5
    return (rgb[0] * k * (1 + warm), rgb[1] * k, rgb[2] * k * (1 - warm))


def cube_local(p):
    c, yaw = CUBE["c"], CUBE["yaw"]
    x, y, z = sub(p, c)
    return (x * math.cos(yaw) + z * math.sin(yaw), y, -x * math.sin(yaw) + z * math.cos(yaw))


def cube_dir_local(d):
    yaw = CUBE["yaw"]
    x, y, z = d
    return (x * math.cos(yaw) + z * math.sin(yaw), y, -x * math.sin(yaw) + z * math.cos(yaw))


def hits(p, d):
    c, r = SPHERE["c"], SPHERE["r"]
    oc = sub(c, p)
    t = dot(oc, d)
    if t > 0 and dot(oc, oc) - t * t < r * r:
        return True
    q, e = cube_local(p), cube_dir_local(d)
    h = CUBE["s"] / 2
    lo, hi = (-h, 0.0, -h), (h, CUBE["s"], h)
    t0, t1 = 1e-4, 1e9
    for i in range(3):
        if abs(e[i]) < 1e-9:
            if q[i] < lo[i] or q[i] > hi[i]:
                t0 = 2e9
            continue
        a, b = (lo[i] - q[i]) / e[i], (hi[i] - q[i]) / e[i]
        t0, t1 = max(t0, min(a, b)), min(t1, max(a, b))
    if t0 <= t1:
        return True
    cc, rr, hh = CYL["c"], CYL["r"], CYL["h"]
    ox, oz = p[0] - cc[0], p[2] - cc[2]
    a = d[0] ** 2 + d[2] ** 2
    b = 2 * (ox * d[0] + oz * d[2])
    k = ox * ox + oz * oz - rr * rr
    disc = b * b - 4 * a * k
    if a > 1e-9 and disc >= 0:
        for t in ((-b - math.sqrt(disc)) / (2 * a), (-b + math.sqrt(disc)) / (2 * a)):
            if t > 1e-4 and 0 <= p[1] + t * d[1] <= hh:
                return True
    if d[1] > 1e-6:
        t = (hh - p[1]) / d[1]
        if t > 0 and (ox + t * d[0]) ** 2 + (oz + t * d[2]) ** 2 <= rr * rr:
            return True
    return False


SOFT = [norm(addv(L, (rnd.uniform(-0.05, 0.05), rnd.uniform(-0.05, 0.05), rnd.uniform(-0.05, 0.05)))) for _ in range(10)]


def lit_fraction(p):
    return sum(0 if hits(p, d) else 1 for d in SOFT) / len(SOFT)


def occlusion(p):
    c, r = SPHERE["c"], SPHERE["r"]
    ds = math.hypot(p[0] - c[0], p[2] - c[2])
    ao = 1 - 0.6 * math.exp(-(ds / (0.55 * r)) ** 2)
    q = cube_local(p)
    h = CUBE["s"] / 2
    dc = math.hypot(max(0.0, abs(q[0]) - h), max(0.0, abs(q[2]) - h))
    ao *= 1 - 0.55 * math.exp(-dc / 0.9)
    cc, rr = CYL["c"], CYL["r"]
    dy = max(0.0, math.hypot(p[0] - cc[0], p[2] - cc[2]) - rr)
    ao *= 1 - 0.55 * math.exp(-dy / 0.8)
    return ao


def shade(p, n, albedo, shadowed=None, gloss=0.0):
    key = max(0.0, dot(n, L)) * (lit_fraction(addv(p, mul(n, 0.05))) if shadowed is None else shadowed)
    sky = 0.5 * (1 + n[1])
    bounce = 0.5 * (1 - n[1]) * 0.26
    out = [a * (k * key + am * sky * 1.6 + t * k * bounce) for a, k, am, t in zip(albedo, KEY, AMB, TABLE)]
    if gloss:
        h = norm(addv(L, to_eye(p)))
        s = gloss * max(0.0, dot(n, h)) ** 70 * key
        out = [c + k * s for c, k in zip(out, KEY)]
    return tuple(out)


def table_colour(x, z, shadowed=True, occluded=True):
    p = (x, 0.0, z)
    frac = lit_fraction(p) if shadowed else 1.0
    ao = occlusion(p) if occluded else 1.0
    light_pool = 1 - 0.35 * min(1.0, math.hypot(x + 2, z - 95) / 70) ** 2
    out = [a * (k * L[1] * frac * light_pool + am * 1.6) * ao for a, k, am in zip(TABLE, KEY, AMB)]
    return tuple(out), frac


def path(points):
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)


def poly(points):
    return path(points) + " Z"


def stroke(layer, brush, size, rgb, points, pressure=None, taper=None, opacity=1.0, bow=True):
    if pressure is not None:
        pts, _ = drawkit.resample(points, max(2, len(points)))
        op = {"op": "stroke", "layer": layer, "brush": brush, "size": round(size, 1), "color": hexc(rgb), "path": path(pts), "pressure": list(pressure), "taper": list(taper or (0, 0))}
        if opacity < 1:
            op["opacity"] = round(opacity, 2)
        return op
    return drawkit.stroke(layer, brush, size, rgb, points, opacity=opacity, bow=bow)


def basis(axis):
    a = norm(axis)
    t = (0.0, 1.0, 0.0) if abs(a[1]) < 0.9 else (1.0, 0.0, 0.0)
    u = norm((a[1] * t[2] - a[2] * t[1], a[2] * t[0] - a[0] * t[2], a[0] * t[1] - a[1] * t[0]))
    w = (a[1] * u[2] - a[2] * u[1], a[2] * u[0] - a[0] * u[2], a[0] * u[1] - a[1] * u[0])
    return a, u, w


def sphere_silhouette():
    return drawkit.silhouette(CAMERA, drawkit.sphere_points(SPHERE["c"], SPHERE["r"], 30, 60))


def cube_vertices():
    c = CUBE["c"]
    return drawkit.box((c[0], 0.0, c[2]), (CUBE["s"],) * 3, CUBE["yaw"])[0]


def cylinder_points(n=72):
    c = CYL["c"]
    return drawkit.cylinder_points((c[0], 0.0, c[2]), CYL["r"], CYL["h"], n)

def sphere_strokes(layer):
    c, r = SPHERE["c"], SPHERE["r"]
    a, u, w = basis(L)
    scale = F / (c[2] - EYE[2])
    ops = []
    for step, size, opacity, jit, keep in ((7, 30, 1.0, 0.015, lambda al: True), (3.5, 15, 1.0, 0.012, lambda al: True), (1.6, 7, 0.9, 0.01, lambda al: al < 40 or 60 < al < 112)):
        alpha = rnd.uniform(0.5, step)
        while alpha < 180:
            ring = math.radians(alpha)
            circumference = 2 * math.pi * math.sin(ring) * r * scale
            if keep(alpha) and circumference > 2:
                width = min(size, max(3.0, circumference / 2.2))
                count = max(2, int(circumference / (width * 2.0)) + 1)
                span = 2 * math.pi / count * 1.7
                beta = rnd.uniform(0, 2 * math.pi)
                for k in range(count):
                    b0 = beta + 2 * math.pi * k / count
                    pts3 = [addv(c, mul(addv(mul(a, math.cos(ring)), mul(addv(mul(u, math.cos(b)), mul(w, math.sin(b))), math.sin(ring))), r)) for b in [b0 - span / 2 + span * i / 5 for i in range(6)]]
                    seen = [p for p in pts3 if dot(norm(sub(p, c)), to_eye(p)) > 0.03]
                    if len(seen) < len(pts3):
                        runs, run = [], []
                        for p in pts3:
                            if dot(norm(sub(p, c)), to_eye(p)) > 0.03:
                                run.append(p)
                            else:
                                runs.append(run)
                                run = []
                        runs.append(run)
                        seen = max(runs, key=len)
                    if len(seen) < 2:
                        continue
                    mid = seen[len(seen) // 2]
                    n = norm(sub(mid, c))
                    rgb = jitter(shade(mid, n, PLASTER), jit)
                    ops.append(stroke(layer, "oil", width * (0.9 + 0.2 * rnd.random()), rgb, [project(p) for p in seen], opacity=opacity, bow=False))
            alpha += step * rnd.uniform(0.85, 1.15)
    return ops


def cube_faces():
    v = cube_vertices()
    faces = {
        "top": [(-1, 1, -1), (1, 1, -1), (1, 1, 1), (-1, 1, 1)],
        "x-": [(-1, 0, -1), (-1, 0, 1), (-1, 1, 1), (-1, 1, -1)],
        "x+": [(1, 0, -1), (1, 0, 1), (1, 1, 1), (1, 1, -1)],
        "z-": [(-1, 0, -1), (1, 0, -1), (1, 1, -1), (-1, 1, -1)],
        "z+": [(-1, 0, 1), (1, 0, 1), (1, 1, 1), (-1, 1, 1)],
    }
    yaw = CUBE["yaw"]
    normals = {"top": (0.0, 1.0, 0.0), "x-": (-math.cos(yaw), 0.0, -math.sin(yaw)), "x+": (math.cos(yaw), 0.0, math.sin(yaw)),
               "z-": (math.sin(yaw), 0.0, -math.cos(yaw)), "z+": (-math.sin(yaw), 0.0, math.cos(yaw))}
    out = []
    for name, keys in faces.items():
        pts = [v[k] for k in keys]
        centre = mul(addv(addv(pts[0], pts[1]), addv(pts[2], pts[3])), 0.25)
        if dot(normals[name], to_eye(centre)) > 0:
            out.append((name, pts, normals[name]))
    return out


def cube_strokes(layer, only):
    ops = []
    for name, (p0, p1, p2, p3), n in cube_faces():
        if name != only:
            continue
        along = sub(p1, p0)
        up = sub(p3, p0)
        height = abs(project(p3)[1] - project(p0)[1]) + abs(project(p2)[1] - project(p1)[1])
        for size, brush, jit, opacity in ((24, "oil", 0.03, 1.0), (12, "oil", 0.04, 0.85), (9, "dry-brush", 0.05, 0.12), (5, "chalk", 0.05, 0.3)):
            rows = max(3, int(height / 2 / (size * 0.42)))
            for i in range(rows):
                t = (i + rnd.uniform(0.25, 0.75)) / rows
                s0 = rnd.uniform(-0.12, 0.05)
                while s0 < 1.0:
                    s1 = min(1.06, s0 + rnd.uniform(0.25, 0.5))
                    pts3 = [addv(p0, addv(mul(along, x), mul(up, t + rnd.uniform(-0.006, 0.006)))) for x in (s0, (2 * s0 + s1) / 3, (s0 + 2 * s1) / 3, s1)]
                    rgb = jitter(shade(pts3[1], n, TERRACOTTA), jit)
                    ops.append(stroke(layer, brush, size * (0.85 + 0.3 * rnd.random()), rgb, [project(p) for p in pts3], opacity=opacity))
                    if s1 >= 1.0:
                        break
                    s0 = s1 - rnd.uniform(0.1, 0.18)
    return ops


def interior_edges():
    faces = [set(pts) for _, pts, _ in [(n, [tuple(round(c, 4) for c in p) for p in ps], m) for n, ps, m in cube_faces()]]
    v = cube_vertices()
    keyed = {tuple(round(c, 4) for c in p): k for k, p in v.items()}
    out = []
    for i in range(len(faces)):
        for j in range(i + 1, len(faces)):
            shared = faces[i] & faces[j]
            if len(shared) == 2:
                a, b = sorted(shared)
                out.append((keyed[a], keyed[b]))
    return out


def cube_chips(layer):
    v = cube_vertices()
    edges = interior_edges()
    ops = []
    for a, b in edges:
        pa, pb = v[a], v[b]
        for _ in range(7):
            t = rnd.uniform(0.08, 0.92)
            p = addv(pa, mul(sub(pb, pa), t))
            x, y = project(p)
            dx, dy = rnd.uniform(-2, 2), rnd.uniform(-2, 2)
            shadow = shade(p, norm(addv((0.0, 0.3, 0.0), (rnd.uniform(-1, 1), 0, rnd.uniform(-1, 1)))), TERRACOTTA)
            ops.append(stroke(layer, "chalk", rnd.uniform(3, 6), mul(shadow, 0.55), [(x, y), (x + dx, y + dy)], pressure=(0.6, 1), taper=(1, 2), opacity=0.8))
            ops.append(stroke(layer, "chalk", rnd.uniform(2, 3.5), mul(shade(p, (0.0, 1.0, 0.0), TERRACOTTA), 1.15), [(x - dx * 0.6, y - dy * 0.6), (x, y)], pressure=(0.6, 1), taper=(1, 2), opacity=0.7))
    return ops


def cylinder_strokes(layer):
    c, r, h = CYL["c"], CYL["r"], CYL["h"]
    ops = []
    term = math.atan2(-L[0], L[2])
    for size, step, jit, opacity in ((20, 0.07, 0.004, 1.0), (10, 0.04, 0.003, 0.9)):
        phi = rnd.uniform(0, step)
        while phi < 2 * math.pi:
            n = (math.cos(phi), 0.0, math.sin(phi))
            mid = (c[0] + r * n[0], h / 2, c[2] + r * n[2])
            near_terminator = abs(dot(n, L)) < 0.22
            if dot(n, to_eye(mid)) > -0.05 and (size > 12 or near_terminator or dot(n, L) > 0.3):
                y0 = -h * rnd.uniform(0.35, 0.5)
                while y0 < h:
                    y1 = h * rnd.uniform(1.35, 1.5)
                    pts3 = [(c[0] + r * n[0], y, c[2] + r * n[2]) for y in [y0 + (y1 - y0) * i / 4 for i in range(5)]]
                    rgb = jitter(shade(pts3[2], n, CELADON), jit)
                    ops.append(stroke(layer, "oil", size, rgb, [project(p) for p in pts3], opacity=opacity, bow=False))
                    if y1 >= h:
                        break
                    y0 = y1 - 0.25 * h
            phi += step * rnd.uniform(0.85, 1.15)
    return ops


def cylinder_top(layer):
    c, r, h = CYL["c"], CYL["r"], CYL["h"]
    ops = []
    for size, rings in ((12, 7), (6, 14)):
        for i in range(rings):
            rr = r * (i + rnd.uniform(0.2, 0.9)) / rings
            a0 = rnd.uniform(0, 2 * math.pi)
            count = max(3, int(rr * 2.2))
            for k in range(count):
                a = a0 + 2 * math.pi * k / count
                pts3 = [(c[0] + rr * math.cos(t), h, c[2] + rr * math.sin(t)) for t in (a, a + 1.5 / count * math.pi, a + 3 / count * math.pi)]
                rgb = jitter(shade(pts3[1], (0.0, 1.0, 0.0), CELADON), 0.008)
                ops.append(stroke(layer, "oil", size, rgb, [project(p) for p in pts3], opacity=0.95))
    return ops


def cylinder_glint(layer):
    c, r, h = CYL["c"], CYL["r"], CYL["h"]
    ops = []
    hv = norm(addv(L, to_eye((c[0], h / 2, c[2]))))
    phi = math.atan2(hv[2], hv[0])
    n = (math.cos(phi), 0.0, math.sin(phi))
    glint = tuple(min(1.0, 0.25 + x * 0.8) for x in shade((c[0] + r * n[0], h / 2, c[2] + r * n[2]), n, CELADON))
    for k, (y0, y1, size, op) in enumerate(((1.5, 14.5, 7, 0.5), (3.0, 12.5, 3.5, 0.8), (6.0, 10.5, 2, 0.9))):
        pts = [project((c[0] + r * math.cos(phi + 0.02 * k), y, c[2] + r * math.sin(phi + 0.02 * k))) for y in [y0 + (y1 - y0) * i / 4 for i in range(5)]]
        ops.append(stroke(layer, "oil", size, glint, pts, opacity=op, bow=False))
    rim = [project((c[0] + r * math.cos(t), h, c[2] + r * math.sin(t))) for t in [math.pi * (0.95 + 0.9 * i / 10) for i in range(11)]]
    ops.append(stroke(layer, "oil", 2.5, glint, rim, opacity=0.55))
    return ops


def table_strokes(layer):
    ops = []
    for size, brush, jit, opacity in ((30, "oil", 0.02, 1.0), (14, "oil", 0.02, 0.9), (6, "chalk", 0.03, 0.25)):
        y = TABLE_Y - size * 0.3
        while y < H + size:
            z = EYE[1] * F / max(1.0, y - HORIZON)
            x0 = rnd.uniform(-60, -10)
            while x0 < W + 10:
                length = rnd.uniform(60, 150) * (size / 30) ** 0.5
                xs = [x0 + length * i / 4 for i in range(5)]
                cols = [table_colour((x - CX) * z / F, z, False, False) for x in xs]
                rgb = jitter(cols[2][0], jit)
                ops.append(stroke(layer, brush, size * (0.85 + 0.3 * rnd.random()), rgb, [(x, y + rnd.uniform(-0.8, 0.8)) for x in xs], opacity=opacity))
                x0 += length * rnd.uniform(0.55, 0.75)
            y += size * 0.4 * (1 + (y - TABLE_Y) / 900)
    return ops


def shadow_ops():
    c, r = SPHERE["c"], SPHERE["r"]
    sphere_pts = [addv(c, (r * math.sin(ph) * math.cos(th), r * math.cos(ph), r * math.sin(ph) * math.sin(th))) for th in [2 * math.pi * i / 48 for i in range(48)] for ph in [math.pi * j / 15 for j in range(16)]]
    casters = [("sphere", sphere_pts, (c[0], 0.0, c[2])), ("cube", list(cube_vertices().values()), (CUBE["c"][0], 0.0, CUBE["c"][2])), ("cyl", cylinder_points(48), (CYL["c"][0], 0.0, CYL["c"][2]))]
    ops = [{"op": "add", "layer": {"id": "shadows", "kind": "vector", "name": "cast shadows", "blend": "multiply"}}]
    texture = []
    for name, pts, base in casters:
        ground = drawkit.shadow(pts, L)
        d = poly(drawkit.silhouette(CAMERA, ground))
        col = hexc(shadow_colour(ground, base))
        bx, by = project(base)
        near = poly([(bx + 150 * math.cos(t), by + 70 * math.sin(t)) for t in [2 * math.pi * i / 48 for i in range(48)]])
        xs3, zs3 = [q[0] for q in ground], [q[2] for q in ground]
        cxz = ((min(xs3) + max(xs3)) / 2, (min(zs3) + max(zs3)) / 2)
        pool = 1 - 0.35 * min(1.0, math.hypot(cxz[0] + 2, cxz[1] - 95) / 70) ** 2
        ratio = tuple((am * 1.6 + k * 0.035) / (k * L[1] * pool + am * 1.6) for am, k in zip(AMB, KEY))
        col = hexc(ratio)
        ops.append({"op": "add", "node": {"id": f"{name}-shadow-soft", "type": "path", "d": d, "fill": col, "blur": 9}, "in": "shadows"})
        ops.append({"op": "add", "node": {"id": f"{name}-shadow-near", "type": "path", "d": d, "fill": col, "blur": 2.5, "clip": near}, "in": "shadows"})
    cc, rr = CYL["c"], CYL["r"]
    cyl_front = [project((cc[0] + rr * 1.04 * math.cos(t), 0.0, cc[2] + rr * 1.04 * math.sin(t))) for t in [math.pi * (0.95 + 1.1 * i / 16) for i in range(17)]]
    v = cube_vertices()
    cube_front = []
    for name, pts, n in cube_faces():
        if name != "top":
            cube_front.append([project(p) for p in pts if p[1] < 0.01])
    sc, sr = SPHERE["c"], SPHERE["r"]
    sphere_foot = [project((sc[0] + sr * 0.55 * math.cos(t), 0.0, sc[2] + sr * 0.55 * math.sin(t))) for t in [2 * math.pi * i / 32 for i in range(32)]]
    contact = [("cyl-contact", path(cyl_front), 9, 5), ("sphere-contact", poly(sphere_foot), 0, 7)] + [(f"cube-contact{i}", path(seg), 10, 5) for i, seg in enumerate(cube_front)]
    for name, d, width, blur in contact:
        node = {"id": name, "type": "path", "d": d, "blur": blur}
        if width:
            node.update({"fill": "none", "stroke": {"paint": "#6e6a66", "width": width, "cap": "round", "join": "round"}})
        else:
            node.update({"fill": "#5e5a56"})
        ops.append({"op": "add", "node": node, "in": "shadows"})
    return ops


def project_inverse(x, y):
    z = EYE[1] * F / max(1.0, y - HORIZON)
    return ((x - CX) * z / F, 0.0, z)


def shadow_colour(ground, base):
    xs = [q[0] for q in ground]
    zs = [q[2] for q in ground]
    x, z = (min(xs) + max(xs)) / 2, (min(zs) + max(zs)) / 2
    ao = occlusion((x, 0.0, z))
    return tuple(a * (am * 1.6 + k * 0.035) * ao for a, am, k in zip(TABLE, AMB, KEY))


TABLE_Y = HORIZON + F * EYE[1] / TABLE_Z


def wall_strokes(layer):
    ops = []
    for size, n, opacity in ((110, 150, 0.45), (55, 220, 0.35)):
        for _ in range(n):
            x, y = rnd.uniform(-20, W + 20), rnd.uniform(-20, TABLE_Y - size * 0.5)
            glow = math.exp(-((x - 560) ** 2 + (y - TABLE_Y) ** 2) / (2 * 420 ** 2))
            rgb = warm_cool(jitter(mul(WALL, 0.85 + 0.5 * glow), 0.05), rnd.uniform(-0.04, 0.04))
            ang = rnd.uniform(1.4, 1.75)
            length = rnd.uniform(60, 160)
            pts = [(x + math.cos(ang) * length * t, y + math.sin(ang) * length * t) for t in (0, 0.33, 0.66, 1)]
            ops.append(stroke(layer, "oil-load" if size < 80 else "oil", size * (0.8 + 0.4 * rnd.random()), rgb, pts, opacity=opacity))
    return ops


def accents(layer):
    ops = []
    v = cube_vertices()
    dark = (0.012, 0.01, 0.009)
    for name, pts, n in cube_faces():
        if name != "top":
            continue
        for i in range(4):
            a3, b3 = pts[i], pts[(i + 1) % 4]
            mid = mul(addv(a3, b3), 0.5)
            outward = norm(sub(mid, (CUBE["c"][0], CUBE["s"], CUBE["c"][2])))
            if dot(outward, to_eye(mid)) <= 0.1:
                continue
            for k in range(3):
                t0 = rnd.uniform(0.0, 0.6)
                t1 = min(1.0, t0 + rnd.uniform(0.25, 0.5))
                seg = [project(addv(a3, mul(sub(b3, a3), t0 + (t1 - t0) * j / 3))) for j in range(4)]
                ops.append(stroke(layer, "oil", rnd.uniform(2, 3.2), mul(shade(mid, norm(addv((0.0, 1.0, 0.0), outward)), TERRACOTTA), 1.25), seg, opacity=0.65, bow=False))
    cc, rr = CYL["c"], CYL["r"]
    front = [project((cc[0] + rr * math.cos(t), 0.0, cc[2] + rr * math.sin(t))) for t in [math.pi * (1 + i / 12) for i in range(13)]]
    ops.append(stroke(layer, "oil", 4.5, dark, front[::-1], opacity=0.8, bow=False))
    return ops


def warm_cool(rgb, amount):
    return (rgb[0] * (1 + amount), rgb[1] * (1 + amount * 0.2), rgb[2] * (1 - amount))


def ring_point(c, r, frame, alpha, beta):
    a, u, w = frame
    return addv(c, mul(addv(mul(a, math.cos(alpha)), mul(addv(mul(u, math.cos(beta)), mul(w, math.sin(beta))), math.sin(alpha))), r))


def visible_run(points, ok):
    runs, run = [], []
    for p in points:
        if ok(p):
            run.append(p)
        else:
            runs.append(run)
            run = []
    runs.append(run)
    return max(runs, key=len)


def decisive_sphere(layer):
    c, r = SPHERE["c"], SPHERE["r"]
    frame = basis(L)
    scale = F / (c[2] - EYE[2])
    ops = []
    bands = [(60, 2, 72, (12, 24), "oil-load", 0.05, 0.03, 0.95), (26, 74, 98, (8, 14), "oil-load", 0.035, 0.05, 0.9), (16, 100, 150, (14, 22), "oil", 0.02, -0.03, 0.75)]
    for count, a0, a1, (s0, s1), brush, jit, temp, opacity in bands:
        for _ in range(count):
            alpha = math.radians(rnd.uniform(a0, a1))
            beta = rnd.uniform(0, 2 * math.pi)
            length = rnd.uniform(40, 95)
            span = min(2.6, length / max(1.0, r * scale * math.sin(alpha)))
            pts3 = [ring_point(c, r, frame, alpha + rnd.uniform(-0.01, 0.01), beta + span * (i / 6 - 0.5)) for i in range(7)]
            seen = visible_run(pts3, lambda p: dot(norm(sub(p, c)), to_eye(p)) > 0.04)
            if len(seen) < 3:
                continue
            mid = seen[len(seen) // 2]
            rgb = warm_cool(jitter(shade(mid, norm(sub(mid, c)), PLASTER), jit), temp + rnd.uniform(-0.03, 0.03))
            ops.append(stroke(layer, brush, rnd.uniform(s0, s1), rgb, [project(p) for p in seen], opacity=opacity, bow=False))
    return ops


def sphere_pits(layer):
    c, r = SPHERE["c"], SPHERE["r"]
    frame = basis(L)
    ops = []
    light2 = norm((L[0], -L[1]))
    for _ in range(170):
        alpha = math.radians(min(150.0, max(5.0, rnd.gauss(80, 28))))
        p = ring_point(c, r, frame, alpha, rnd.uniform(0, 2 * math.pi))
        n = norm(sub(p, c))
        if dot(n, to_eye(p)) < 0.25:
            continue
        x, y = project(p)
        local = shade(p, n, PLASTER)
        size = rnd.uniform(1.1, 2.4)
        dx, dy = light2[0] * size * 0.6, light2[1] * size * 0.6
        ops.append(stroke(layer, "oil", size, mul(local, 0.62), [(x + dx * 0.2, y + dy * 0.2), (x + dx, y + dy + 0.4)], pressure=(0.8, 1), taper=(0.5, 0.8), opacity=0.8))
        if dot(n, L) > 0.05:
            ops.append(stroke(layer, "oil", size * 0.8, mul(local, 1.1), [(x - dx, y - dy), (x - dx * 0.2, y - dy * 0.2 + 0.4)], pressure=(0.8, 1), taper=(0.5, 0.8), opacity=0.7))
    return ops


def sphere_edges(layer):
    c, r = SPHERE["c"], SPHERE["r"]
    sil = sphere_silhouette()
    cx, cy = project(c)
    ops = []
    for x, y in sil:
        ang = math.atan2(y - cy, x - cx)
        if not (0.1 < ang < 2.2) or rnd.random() < 0.55:
            continue
        nx, ny = math.cos(ang), math.sin(ang)
        tx, ty = -ny, nx
        inner = project(addv(c, mul(norm((nx, -ny, -0.15)), r)))
        col = shade(addv(c, mul(norm((nx, -ny, 0.1)), r)), norm((nx, -ny, 0.1)), PLASTER)
        col = tuple(0.5 * a + 0.5 * b for a, b in zip(col, mul(TABLE, 0.11)))
        ops.append(stroke(layer, "oil", rnd.uniform(5, 9), col, [(x - tx * 9 - nx * 2, y - ty * 9 - ny * 2), (x + nx * 1.5, y + ny * 1.5), (x + tx * 9 - nx * 2, y + ty * 9 - ny * 2)], opacity=0.55, bow=False))
    return ops


def decisive_cube(layer, only):
    ops = []
    for name, (p0, p1, p2, p3), n in cube_faces():
        if name != only:
            continue
        along, up = sub(p1, p0), sub(p3, p0)
        lit = dot(n, L) > 0.1
        for _ in range(48 if lit else 14):
            t = rnd.uniform(0.02, 0.98)
            s0 = rnd.uniform(-0.05, 0.85)
            s1 = min(1.05, s0 + rnd.uniform(0.12, 0.32))
            cross = lit and rnd.random() < 0.2
            if cross:
                pts3 = [addv(p0, addv(mul(along, s0 + rnd.uniform(-0.01, 0.01)), mul(up, max(0.0, min(1.0, t - 0.1 + 0.2 * i / 4))))) for i in range(5)]
            else:
                tilt = rnd.uniform(-0.03, 0.03)
                pts3 = [addv(p0, addv(mul(along, s0 + (s1 - s0) * i / 4), mul(up, t + tilt * i / 4))) for i in range(5)]
            rgb = warm_cool(jitter(shade(pts3[2], n, TERRACOTTA), 0.06 if lit else 0.025), rnd.uniform(-0.04, 0.05))
            ops.append(stroke(layer, "oil-load" if lit else "oil", rnd.uniform(12, 26) if lit else rnd.uniform(18, 30), rgb, [project(p) for p in pts3], opacity=0.9 if lit else 0.6))
    return ops


def cube_grain(layer, only):
    ops = []
    for name, (p0, p1, p2, p3), n in cube_faces():
        if name != only:
            continue
        along, up = sub(p1, p0), sub(p3, p0)
        lit = dot(n, L) > 0.1
        spots = []
        for _ in range(16 if lit else 5):
            cu, cv = rnd.uniform(0.05, 0.95), rnd.uniform(0.05, 0.95)
            spread = rnd.uniform(0.02, 0.07)
            spots += [(min(0.99, max(0.01, rnd.gauss(cu, spread))), min(0.99, max(0.01, rnd.gauss(cv, spread)))) for _ in range(rnd.randint(4, 14))]
        spots += [(rnd.uniform(0.01, 0.99), rnd.uniform(0.01, 0.99)) for _ in range(50 if lit else 15)]
        for su, sv in spots:
            p = addv(p0, addv(mul(along, su), mul(up, sv)))
            x, y = project(p)
            local = shade(p, n, TERRACOTTA)
            k = rnd.choice((0.8, 0.86, 1.1, 1.16)) if lit else rnd.choice((0.88, 1.1))
            size = rnd.uniform(0.8, 2.4) * rnd.choice((1, 1, 1, 1.8))
            ang = rnd.uniform(0, math.pi)
            ops.append(stroke(layer, "oil", size, mul(local, k), [(x, y), (x + math.cos(ang) * size * 1.4, y + math.sin(ang) * size * 1.4)], pressure=(0.9, 1), taper=(0.4, 0.6), opacity=0.55 if lit else 0.35))
        bottom = sorted([p0, p1, p2, p3], key=lambda q: q[1])[:2]
        if name != "top":
            for _ in range(10):
                t0 = rnd.uniform(0, 0.8)
                a3 = addv(bottom[0], mul(sub(bottom[1], bottom[0]), t0))
                b3 = addv(bottom[0], mul(sub(bottom[1], bottom[0]), min(1.0, t0 + rnd.uniform(0.15, 0.3))))
                lift = (0.0, rnd.uniform(0.4, 1.4), 0.0)
                ops.append(stroke(layer, "oil", rnd.uniform(5, 10), mul(shade(a3, n, TERRACOTTA), 0.7), [project(addv(a3, lift)), project(addv(mul(addv(a3, b3), 0.5), lift)), project(addv(b3, lift))], opacity=0.4))
    return ops


def cube_wear(layer):
    ops = []
    faces = {name: n for name, pts, n in cube_faces()}
    for a, b in interior_edges():
        v = cube_vertices()
        pa, pb = v[a], v[b]
        for t in sorted(rnd.uniform(0.06, 0.94) for _ in range(rnd.randint(2, 5))):
            p = addv(pa, mul(sub(pb, pa), t))
            x, y = project(p)
            ex, ey = project(addv(p, mul(norm(sub(pb, pa)), 1.0)))
            dx, dy = ex - x, ey - y
            d = math.hypot(dx, dy) or 1
            tx, ty = dx / d, dy / d
            nx, ny = -ty, tx
            size = rnd.uniform(2.0, 4.5) * rnd.choice((1, 1, 1.7))
            length = rnd.uniform(4, 14)
            dark = mul(shade(p, norm((0.3, -0.2, 0.4)), TERRACOTTA), 0.6)
            light = mul(shade(p, (0.0, 1.0, 0.0), TERRACOTTA), 1.2)
            side = rnd.choice((-1, 1))
            ops.append(stroke(layer, "oil-load", size, dark, [(x - tx * length / 2 + nx * side * 1.2, y - ty * length / 2 + ny * side * 1.2), (x + nx * side * 2, y + ny * side * 2), (x + tx * length / 2 + nx * side * 1.2, y + ty * length / 2 + ny * side * 1.2)], opacity=0.85, bow=False))
            ops.append(stroke(layer, "oil", size * 0.45, light, [(x - tx * length / 2 - nx * side * 0.6, y - ty * length / 2 - ny * side * 0.6), (x + tx * length / 2 - nx * side * 0.6, y + ty * length / 2 - ny * side * 0.6)], opacity=0.75, bow=False))
    return ops


def cylinder_detail(layer):
    c, r, h = CYL["c"], CYL["r"], CYL["h"]
    ops = []
    for _ in range(30):
        phi = rnd.uniform(0, 2 * math.pi)
        n = (math.cos(phi), 0.0, math.sin(phi))
        if dot(n, L) < 0.25 or dot(n, to_eye((c[0], h / 2, c[2]))) < 0.1:
            continue
        y0 = rnd.uniform(1.5, h * 0.55)
        y1 = min(h - 1.0, y0 + rnd.uniform(0.3, 0.6) * h)
        pts = [project((c[0] + r * n[0], y0 + (y1 - y0) * i / 4, c[2] + r * n[2])) for i in range(5)]
        rgb = warm_cool(jitter(shade((c[0] + r * n[0], (y0 + y1) / 2, c[2] + r * n[2]), n, CELADON), 0.03), rnd.uniform(-0.03, 0.03))
        ops.append(stroke(layer, "oil-load", rnd.uniform(7, 14), rgb, pts, opacity=0.8, bow=False))
    for band, y, k, size, opacity in (("foot", 0.35, None, 6, 0.95), ("pool", 1.15, 0.62, 5, 0.85)):
        for i in range(36):
            phi = 2 * math.pi * i / 36
            n = (math.cos(phi), 0.0, math.sin(phi))
            if dot(n, to_eye((c[0], y, c[2]))) < 0.0:
                continue
            pts = [project((c[0] + r * math.cos(phi + d), y + rnd.uniform(-0.05, 0.05), c[2] + r * math.sin(phi + d))) for d in (-0.12, 0.0, 0.12)]
            local = shade((c[0] + r * n[0], y, c[2] + r * n[2]), n, CELADON)
            rgb = shade((c[0] + r * n[0], y, c[2] + r * n[2]), n, (0.66, 0.5, 0.38)) if band == "foot" else mul(local, k)
            ops.append(stroke(layer, "oil", size, rgb, pts, opacity=opacity, bow=False))
    for _ in range(34):
        phi = rnd.uniform(0, 2 * math.pi)
        y = rnd.uniform(2.0, h - 0.5)
        n = (math.cos(phi), 0.0, math.sin(phi))
        if dot(n, L) < 0.1 or dot(n, to_eye((c[0], y, c[2]))) < 0.15:
            continue
        pts = []
        for _ in range(4):
            pts.append(project((c[0] + r * math.cos(phi), y, c[2] + r * math.sin(phi))))
            phi += rnd.uniform(-0.12, 0.12)
            y += rnd.uniform(-0.9, 0.9)
        local = shade((c[0] + r * n[0], y, c[2] + r * n[2]), n, CELADON)
        ops.append(stroke(layer, "oil", rnd.uniform(0.8, 1.3), mul(local, 0.72), pts, pressure=(0.6, 1, 0.9, 0.5), taper=(1, 2), opacity=0.55, bow=False))
    for _ in range(10):
        phi = rnd.uniform(0, 2 * math.pi)
        n = (math.cos(phi), 0.0, math.sin(phi))
        if dot(n, L) > -0.05 or dot(n, to_eye((c[0], 2, c[2]))) < 0.05:
            continue
        pts = [project((c[0] + r * n[0], y, c[2] + r * n[2])) for y in (0.6, 1.6, 2.6, 3.4)]
        local = shade((c[0] + r * n[0], 1.5, c[2] + r * n[2]), n, CELADON)
        rgb = tuple(0.85 * a + 0.15 * b for a, b in zip(local, table_colour(c[0] + 8, c[2] - 6, False, False)[0]))
        ops.append(stroke(layer, "oil", rnd.uniform(4, 7), rgb, pts, opacity=0.35, bow=False))
    return ops


def table_marks(layer):
    ops = []
    for _ in range(90):
        y = rnd.uniform(TABLE_Y + 8, H)
        z = EYE[1] * F / (y - HORIZON)
        x = rnd.uniform(-20, W)
        length = rnd.uniform(70, 200)
        xs = [x + length * i / 4 for i in range(5)]
        col = table_colour((x + length / 2 - CX) * z / F, z, False, False)[0]
        rgb = warm_cool(jitter(col, 0.035), rnd.uniform(-0.03, 0.04))
        ops.append(stroke(layer, "oil-load", rnd.uniform(14, 30) * (0.6 + 0.6 * (y - TABLE_Y) / (H - TABLE_Y)), rgb, [(xx, y + rnd.uniform(-1.5, 1.5)) for xx in xs], opacity=0.55))
    return ops


def scene():
    ops = [{"op": "add", "layer": {"id": "sketch", "kind": "vector", "name": "construction", "visible": False}},
           {"op": "add", "layer": {"id": "wall-base", "kind": "vector", "name": "wall base"}},
           {"op": "add", "node": {"id": "wall-grad", "type": "rect", "x": -5, "y": -5, "w": W + 10, "h": TABLE_Y + 6, "fill": {"radial": {"center": [560, TABLE_Y], "radius": 900, "stops": [[0, hexc(mul(WALL, 1.3))], [0.6, hexc(mul(WALL, 0.95))], [1, hexc(mul(WALL, 0.7))]]}}}, "in": "wall-base"}]
    shapes = {
        "table-shape": poly([(-5, TABLE_Y), (W + 5, TABLE_Y), (W + 5, H + 5), (-5, H + 5)]),
        "sphere-shape": poly(sphere_silhouette()),
        "cube-shape": poly(drawkit.hull([project(p) for p in cube_vertices().values()])),
        "cyl-shape": poly(drawkit.hull([project(p) for p in cylinder_points()])),
    }
    cc, rr, hh = CYL["c"], CYL["r"], CYL["h"]
    shapes["cyl-top-shape"] = poly([project((cc[0] + rr * math.cos(t), hh, cc[2] + rr * math.sin(t))) for t in [2 * math.pi * i / 72 for i in range(72)]])
    for name, pts, n in cube_faces():
        shapes[f"cube-{name}-shape"] = poly([project(p) for p in pts])
    ops += [{"op": "add", "node": {"id": k, "type": "path", "d": d}, "in": "sketch"} for k, d in shapes.items()]
    a = L
    lit_mid = norm(addv(mul(a, 0.55), (0.0, 0.0, -0.45)))
    ops.append({"op": "add", "layer": {"id": "wall", "kind": "paint", "name": "wall strokes"}})
    ops += wall_strokes("wall")
    ops.append({"op": "add", "layer": {"id": "table", "kind": "paint", "name": "table"}})
    ops.append({"op": "fill", "layer": "table", "shape": "table-shape", "color": hexc(table_colour(0, 120)[0])})
    ops.append({"op": "set", "id": "table", "lockAlpha": True})
    ops += table_strokes("table") + table_marks("table") + shadow_ops()
    ops.append({"op": "add", "layer": {"id": "cylinder", "kind": "paint", "name": "cylinder"}})
    ops.append({"op": "fill", "layer": "cylinder", "shape": "cyl-shape", "color": hexc(shade((cc[0] - 3, 8, cc[2] - 4), norm((-0.6, 0.0, -0.8)), CELADON))})
    ops.append({"op": "set", "id": "cylinder", "lockAlpha": True})
    ops += cylinder_strokes("cylinder") + cylinder_detail("cylinder")
    ops.append({"op": "add", "layer": {"id": "cylinder-top", "kind": "paint", "name": "cylinder top"}})
    ops.append({"op": "fill", "layer": "cylinder-top", "shape": "cyl-top-shape", "color": hexc(shade((cc[0], hh, cc[2]), (0.0, 1.0, 0.0), CELADON))})
    ops.append({"op": "set", "id": "cylinder-top", "lockAlpha": True})
    ops += cylinder_top("cylinder-top")
    ops.append({"op": "add", "layer": {"id": "cylinder-glint", "kind": "paint", "name": "cylinder glint"}})
    ops += cylinder_glint("cylinder-glint")
    for name, pts, n in cube_faces():
        centre = mul(addv(addv(pts[0], pts[1]), addv(pts[2], pts[3])), 0.25)
        lid = f"cube-{name}"
        ops.append({"op": "add", "layer": {"id": lid, "kind": "paint", "name": f"cube {name}"}})
        ops.append({"op": "fill", "layer": lid, "shape": f"cube-{name}-shape", "color": hexc(shade(centre, n, TERRACOTTA))})
        ops.append({"op": "set", "id": lid, "lockAlpha": True})
        ops += cube_strokes(lid, name) + decisive_cube(lid, name) + cube_grain(lid, name)
    ops.append({"op": "add", "layer": {"id": "cube-chips", "kind": "paint", "name": "cube chips"}})
    ops += cube_wear("cube-chips")
    ops.append({"op": "add", "layer": {"id": "sphere", "kind": "paint", "name": "sphere"}})
    ops.append({"op": "fill", "layer": "sphere", "shape": "sphere-shape", "color": hexc(shade(addv(SPHERE["c"], mul(lit_mid, SPHERE["r"])), lit_mid, PLASTER))})
    ops.append({"op": "set", "id": "sphere", "lockAlpha": True})
    ops += sphere_strokes("sphere") + decisive_sphere("sphere") + sphere_pits("sphere")
    ops.append({"op": "add", "layer": {"id": "edges", "kind": "paint", "name": "lost edges"}})
    ops += sphere_edges("edges")
    ops.append({"op": "add", "layer": {"id": "accents", "kind": "paint", "name": "accents"}})
    ops += accents("accents")
    return ops


print("\n".join(json.dumps(o) for o in scene()))
