import json
import math
import random
import sys

rnd = random.Random(7)


def seed(n):
    rnd.seed(n)


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def addv(a, b):
    return tuple(x + y for x, y in zip(a, b))


def mul(a, k):
    return tuple(x * k for x in a)


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def norm(v):
    d = math.sqrt(dot(v, v)) or 1.0
    return tuple(x / d for x in v)


def lerp(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


class Camera:
    def __init__(self, eye, focal, centre, horizon):
        self.eye, self.focal, self.cx, self.horizon = tuple(eye), focal, centre, horizon

    def project(self, p):
        x, y, z = sub(p, self.eye)
        z = max(z, 1e-3)
        return (self.cx + self.focal * x / z, self.horizon - self.focal * y / z)

    def to_eye(self, p):
        return norm(sub(self.eye, p))

    def faces_viewer(self, point, normal):
        return dot(normal, self.to_eye(point)) > 0


def hull(points):
    pts = sorted(set((round(x, 3), round(y, 3)) for x, y in points))
    if len(pts) < 3:
        return pts

    def half(ps):
        out = []
        for p in ps:
            while len(out) >= 2 and (out[-1][0] - out[-2][0]) * (p[1] - out[-2][1]) - (out[-1][1] - out[-2][1]) * (p[0] - out[-2][0]) <= 0:
                out.pop()
            out.append(p)
        return out[:-1]

    return half(pts) + half(pts[::-1])


def box(centre, size, yaw=0.0):
    sx, sy, sz = size
    c, s = math.cos(yaw), math.sin(yaw)
    corners = {}
    for ix in (-1, 1):
        for iy in (0, 1):
            for iz in (-1, 1):
                lx, lz = ix * sx / 2, iz * sz / 2
                corners[(ix, iy, iz)] = (centre[0] + lx * c - lz * s, centre[1] + iy * sy, centre[2] + lx * s + lz * c)
    faces = {
        "top": ([(-1, 1, -1), (1, 1, -1), (1, 1, 1), (-1, 1, 1)], (0.0, 1.0, 0.0)),
        "bottom": ([(-1, 0, -1), (1, 0, -1), (1, 0, 1), (-1, 0, 1)], (0.0, -1.0, 0.0)),
        "x-": ([(-1, 0, -1), (-1, 0, 1), (-1, 1, 1), (-1, 1, -1)], (-c, 0.0, -s)),
        "x+": ([(1, 0, -1), (1, 0, 1), (1, 1, 1), (1, 1, -1)], (c, 0.0, s)),
        "z-": ([(-1, 0, -1), (1, 0, -1), (1, 1, -1), (-1, 1, -1)], (s, 0.0, -c)),
        "z+": ([(-1, 0, 1), (1, 0, 1), (1, 1, 1), (-1, 1, 1)], (-s, 0.0, c)),
    }
    return corners, {name: ([corners[k] for k in keys], n) for name, (keys, n) in faces.items()}


def visible_faces(camera, faces):
    out = {}
    for name, (pts, n) in faces.items():
        centre = mul(tuple(sum(c) for c in zip(*pts)), 1 / len(pts))
        if camera.faces_viewer(centre, n):
            out[name] = (pts, n)
    return out


def sphere_points(centre, radius, rings=24, sides=48):
    return [addv(centre, (radius * math.sin(ph) * math.cos(th), radius * math.cos(ph), radius * math.sin(ph) * math.sin(th)))
            for th in [2 * math.pi * i / sides for i in range(sides)] for ph in [math.pi * j / (rings - 1) for j in range(rings)]]


def circle_points(centre, radius, n=72, y=0.0):
    return [(centre[0] + radius * math.cos(2 * math.pi * i / n), centre[1] + y, centre[2] + radius * math.sin(2 * math.pi * i / n)) for i in range(n)]


def cylinder_points(base, radius, height, n=72):
    return circle_points(base, radius, n) + circle_points(base, radius, n, height)


def extrusion_points(outline, height):
    return [p for x, z in outline for p in ((x, 0.0, z), (x, height, z))]


def silhouette(camera, points3):
    return hull([camera.project(p) for p in points3])


def shadow(points3, toward_light, ground=0.0):
    l = norm(toward_light)
    if l[1] <= 1e-6:
        raise ValueError("the light must come from above the ground to cast a shadow")
    return [(p[0] - l[0] * (p[1] - ground) / l[1], ground, p[2] - l[2] * (p[1] - ground) / l[1]) for p in points3]


def shadow_shape(camera, points3, toward_light, ground=0.0):
    return hull([camera.project(p) for p in points3 + shadow(points3, toward_light, ground) if p[1] <= ground + 1e-6])


def encode(c):
    c = max(0.0, min(1.0, c))
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def hexc(rgb, alpha=None):
    out = "#" + "".join(f"{round(encode(c) * 255):02x}" for c in rgb)
    return out if alpha is None else out + f"{round(max(0.0, min(1.0, alpha)) * 255):02x}"


def path(points):
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)


def poly(points):
    return path(points) + " Z"


def resample(points, n):
    lengths = [0.0]
    for a, b in zip(points, points[1:]):
        lengths.append(lengths[-1] + math.dist(a, b))
    total = lengths[-1] or 1.0
    out = []
    for i in range(n):
        target = total * i / (n - 1)
        k = max(1, next((j for j, l in enumerate(lengths) if l >= target), len(lengths) - 1))
        a, b = points[k - 1], points[k]
        seg = (lengths[k] - lengths[k - 1]) or 1.0
        t = (target - lengths[k - 1]) / seg
        out.append((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
    return out, total


def hand(points, bow=True, strength=1.0):
    pts, length = resample(points, 11)
    attack, release = rnd.uniform(0.12, 0.35), rnd.uniform(0.2, 0.5)
    start, peak, end = rnd.uniform(0.08, 0.3), rnd.uniform(0.82, 1.0), rnd.uniform(0.03, 0.18)
    sway, arc, width = rnd.uniform(-0.06, 0.06), rnd.uniform(-0.05, 0.05), rnd.uniform(0.65, 1.35)
    tin, tout = rnd.uniform(0.4, 0.8), rnd.uniform(0.5, 0.9)
    (x0, y0), (x1, y1) = pts[0], pts[-1]
    dx, dy = x1 - x0, y1 - y0
    d = math.hypot(dx, dy) or 1.0
    bend = arc * length * strength if bow else 0.0
    pts = [(x - dy / d * bend * math.sin(math.pi * i / 10), y + dx / d * bend * math.sin(math.pi * i / 10)) for i, (x, y) in enumerate(pts)]

    def ease(a, b, u):
        u = max(0.0, min(1.0, u))
        return a + (b - a) * (3 * u * u - 2 * u * u * u)

    pressure = []
    for i in range(11):
        t = i / 10
        if t < attack:
            p = ease(start, peak, t / attack)
        elif t > 1 - release:
            p = ease(peak, end, (t - (1 - release)) / release)
        else:
            p = peak * (1 - sway * math.sin(math.pi * (t - attack) / max(0.01, 1 - attack - release)))
        pressure.append(round(max(0.02, min(1.0, 1 - strength + strength * p)), 3))
    taper = [round(strength * length * attack * tin, 1), round(strength * length * release * tout, 1)]
    return pts, pressure, taper, 1 + strength * (width - 1)


def stroke(layer, brush, size, rgb, points, opacity=1.0, bow=True, strength=1.0, frame=0):
    pts, pressure, taper, width = hand(points, bow, strength)
    op = {"op": "stroke", "layer": layer, "brush": brush, "size": round(size * width, 1), "color": hexc(rgb) if isinstance(rgb, tuple) else rgb,
          "path": path(pts), "pressure": pressure, "taper": taper, "frame": frame}
    if opacity < 1:
        op["opacity"] = round(opacity, 2)
    return op


def vector_layer(id, name, **extra):
    return {"op": "add", "layer": {"id": id, "kind": "vector", "name": name, **extra}}


def paint_layer(id, name, shape=None, colour=None, **extra):
    ops = [{"op": "add", "layer": {"id": id, "kind": "paint", "name": name, **extra}}]
    if shape:
        ops.append({"op": "fill", "layer": id, "shape": shape, "color": hexc(colour) if isinstance(colour, tuple) else colour})
        ops.append({"op": "set", "id": id, "lockAlpha": True})
    return ops


def add(into, node):
    return {"op": "add", "node": node, "in": into}


def emit(ops, out=sys.stdout):
    out.write("\n".join(json.dumps(o) for o in ops) + "\n")


def check():
    cam = Camera((0.0, 34.0, 0.0), 1500.0, 600.0, 170.0)
    corners, faces = box((16.0, 0.0, 101.0), (17.0, 17.0, 17.0), math.radians(33))

    def meet(a, b, c, d):
        (x1, y1), (x2, y2), (x3, y3), (x4, y4) = (cam.project(p) for p in (a, b, c, d))
        den = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4)
        t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / den
        return (x1 + t * (x2 - x1), y1 + t * (y2 - y1))

    left = meet(corners[(-1, 0, -1)], corners[(-1, 0, 1)], corners[(1, 1, -1)], corners[(1, 1, 1)])
    right = meet(corners[(-1, 0, -1)], corners[(1, 0, -1)], corners[(-1, 1, 1)], corners[(1, 1, 1)])
    assert abs(left[1] - cam.horizon) < 0.5 and abs(right[1] - cam.horizon) < 0.5, (left, right)
    near, far = cam.project(corners[(-1, 0, -1)]), cam.project(corners[(1, 0, 1)])
    near_h = near[1] - cam.project(corners[(-1, 1, -1)])[1]
    far_h = far[1] - cam.project(corners[(1, 1, 1)])[1]
    assert near_h > far_h, (near_h, far_h)
    assert set(visible_faces(cam, faces)) == {"top", "x-", "z-"}, set(visible_faces(cam, faces))

    def roundness(y):
        ring = [cam.project(p) for p in circle_points((-27.0, 0.0, 112.0), 5.0, 72, y)]
        xs, ys = [p[0] for p in ring], [p[1] for p in ring]
        return (max(ys) - min(ys)) / (max(xs) - min(xs))

    assert roundness(0.0) > roundness(16.0) > 0.0
    toward = (-0.47, 0.86, -0.21)
    cast = shadow([(0.0, 10.0, 100.0)], toward)[0]
    assert cast[1] == 0.0 and cast[0] > 0.0 and cast[2] > 100.0, cast
    seed(3)
    pts, pressure, taper, width = hand([(0, 0), (120, 0)], bow=False)
    assert pressure[0] < max(pressure) * 0.5 and pressure[-1] < max(pressure) * 0.4 and taper[1] > 0 and all(abs(y) < 1e-9 for _, y in pts)
    seed(3)
    assert hand([(0, 0), (120, 0)], bow=False) == (pts, pressure, taper, width)
    print("drawkit: vanishing points on one horizon, far edges shorter, rims rounder away from eye level, shadows away from the light, hands repeat: ok")


if __name__ == "__main__" and "--check" in sys.argv:
    check()
