import struct
import sys
import zlib

def corners_transparent(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        return False, "not a PNG"

    pos, idat, header = 8, b"", None
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length

    width, height, depth, color_type, _, _, interlace = header
    if color_type != 6:
        return False, "no alpha channel (color type %d)" % color_type
    if depth != 8 or interlace != 0:
        return False, "unsupported PNG layout (depth %d, interlace %d)" % (depth, interlace)

    bpp, stride = 4, width * 4
    raw = zlib.decompress(idat)
    rows, prev = [], bytearray(stride)
    for y in range(height):
        ftype = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if ftype == 1:
                line[i] = (line[i] + a) & 0xFF
            elif ftype == 2:
                line[i] = (line[i] + b) & 0xFF
            elif ftype == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif ftype == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        rows.append(line)
        prev = line

    alphas = [rows[y][x * 4 + 3] for x, y in
              [(0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1)]]
    if any(alphas):
        return False, "corner alpha values %s (want all 0)" % alphas
    return True, "corners transparent"

if __name__ == "__main__":
    ok, why = corners_transparent(sys.argv[1])
    print("    alpha check: " + why)
    sys.exit(0 if ok else 1)
