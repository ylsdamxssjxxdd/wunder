import struct

SRC = r"D:\proj\wunder\frontend-slint\assets\app-icon.rgba"
DST = r"D:\proj\wunder\frontend-slint\assets\app-icon.ico"

raw = open(SRC, "rb").read()
side = 32
assert len(raw) == side * side * 4, f"unexpected rgba size: {len(raw)}"

# BMP-style ICO entry: BITMAPINFOHEADER with doubled height, BGRA rows
# bottom-up, followed by an all-transparent AND mask.
header = struct.pack(
    "<IiiHHIIiiII",
    40, side, side * 2, 1, 32, 0,
    side * side * 4 + side * 4, 0, 0, 0, 0,
)
xor = bytearray(side * side * 4)
for y in range(side):
    src_row = y * side * 4
    dst_row = (side - 1 - y) * side * 4
    for x in range(side):
        r, g, b, a = raw[src_row + x * 4: src_row + x * 4 + 4]
        dst = dst_row + x * 4
        xor[dst] = b
        xor[dst + 1] = g
        xor[dst + 2] = r
        xor[dst + 3] = a
and_mask = b"\x00" * (side * 4)
image = header + bytes(xor) + and_mask

ico = struct.pack("<HHH", 0, 1, 1)
ico += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(image), 22)
ico += image
open(DST, "wb").write(ico)
print("wrote", DST, len(ico), "bytes")
