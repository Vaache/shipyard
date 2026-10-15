import struct
import sys
from pathlib import Path


def pack_ico(pngs, out):
    entries = []
    for p in pngs:
        data = Path(p).read_bytes()
        if data[:8] != b"\x89PNG\r\n\x1a\n":
            raise SystemExit(f"{p} is not a PNG")
        width, height = struct.unpack(">II", data[16:24])
        entries.append((width, height, data))
    entries.sort(key=lambda e: e[0])
    header = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    directory = b""
    blobs = b""
    for width, height, data in entries:
        directory += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(data), offset + len(blobs))
        blobs += data
    Path(out).write_bytes(header + directory + blobs)


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit("usage: make-ico.py OUT.ico IN-16.png IN-32.png ...")
    pack_ico(sys.argv[2:], sys.argv[1])
