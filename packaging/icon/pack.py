import struct
import sys
from pathlib import Path

ICNS = [("icp4", 16), ("icp5", 32), ("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic10", 1024), ("ic11", 32),
        ("ic12", 64), ("ic13", 256), ("ic14", 512)]
ICO = [16, 24, 32, 48, 64, 128, 256]


def icns(pngs, out):
    chunks = b"".join(kind.encode() + struct.pack(">I", 8 + len(pngs[size])) + pngs[size] for kind, size in ICNS)
    out.write_bytes(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)


def ico(pngs, out):
    head = struct.pack("<HHH", 0, 1, len(ICO))
    offset = 6 + 16 * len(ICO)
    entries, data = b"", b""
    for size in ICO:
        png = pngs[size]
        side = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    out.write_bytes(head + entries + data)


if __name__ == "__main__":
    folder = Path(sys.argv[1])
    sizes = {s for _, s in ICNS} | set(ICO)
    pngs = {s: (folder / f"icon-{s}.png").read_bytes() for s in sizes}
    icns(pngs, Path(sys.argv[2]))
    ico(pngs, Path(sys.argv[3]))
