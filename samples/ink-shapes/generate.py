import json

SHAPES = [
    ("circle", {"type": "ellipse", "cx": 0, "cy": 0, "rx": 70, "ry": 70}),
    ("box", {"type": "rect", "x": -70, "y": -55, "w": 140, "h": 110, "radius": 18}),
    ("star", {"type": "path", "d": "M0 -80 L22 -26 L78 -24 L34 10 L50 66 L0 34 L-50 66 L-34 10 L-78 -24 L-22 -26 Z"}),
    ("blob", {"type": "path", "d": "M-60 10 C -70 -50 -10 -80 30 -60 C 80 -40 90 20 50 50 C 10 80 -50 70 -60 10 Z"}),
]
WAYS = [
    {"weight": 6},
    {"weight": 10, "break": 45, "gap": 3, "light": [1, 1]},
    {"weight": 5, "wobble": 1.5, "light": [0, -1], "brush": "pencil"},
]


def ops():
    out = [
        {"op": "add", "layer": {"id": "sketch", "kind": "vector", "name": "sketch", "visible": False}},
        {"op": "add", "layer": {"id": "lines", "kind": "paint", "name": "lines"}},
    ]
    for col, way in enumerate(WAYS):
        ids = []
        for row, (name, node) in enumerate(SHAPES):
            sid = f"{name}{col}"
            ids.append(sid)
            out.append({"op": "add", "node": {**node, "id": sid, "transform": {"x": 170 + col * 300, "y": 120 + row * 210}}, "in": "sketch"})
        out.append({"op": "ink", "layer": "lines", "from": ids, "color": "#15161c", **way})
    return out


if __name__ == "__main__":
    print("\n".join(json.dumps(op) for op in ops()))
