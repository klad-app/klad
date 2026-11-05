import json

INK = "#1d2230"
EMBER = "#d4612c"


def layer(id, kind, **extra):
    return {"op": "add", "layer": {"id": id, "kind": kind, "name": id, **extra}}


def stroke(path, size, colour, pressure, brush="oil-load"):
    return {"op": "stroke", "layer": "paint", "brush": brush, "size": size, "color": colour, "path": path,
            "pressure": pressure}


def ops():
    paper = {"linear": {"from": [0, 100], "to": [0, 924], "stops": [[0, "#f8f1e4"], [1, "#e6d6bd"]]}}
    return [
        layer("plate", "vector"),
        {"op": "add", "node": {"type": "rect", "x": 100, "y": 100, "w": 824, "h": 824, "radius": 186, "fill": paper},
         "in": "plate"},
        {"op": "set", "id": "plate", "effects": [{"shadow": {"x": 0, "y": 14, "radius": 22, "color": "#00000044"}}]},
        layer("paint", "paint", clip=True),
        stroke("M396 226 C 390 420 402 600 388 806", 128, INK, [0.75, 1, 1, 0.95, 0.7], brush="oil"),
        stroke("M440 540 C 530 456 620 366 748 250", 108, INK, [1, 0.95, 0.75, 0.45], brush="oil"),
        stroke("M452 498 C 560 594 650 690 760 808", 118, EMBER, [1, 1, 0.9, 0.7, 0.4], brush="oil"),
        {"op": "set", "id": "paint", "effects": [{"relief": {"light": [-0.6, -0.8], "strength": 1.6, "softness": 2.2}}]},
    ]


if __name__ == "__main__":
    print("\n".join(json.dumps(op) for op in ops()))
