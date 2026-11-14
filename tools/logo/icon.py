# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""The Evil BOOM icon: the logo's two words stacked on its slab (tools/logo/make.py's recipe),
on a black rounded tile. Writes, into OUT_DIR (native/assets):

    icon.png   256 x 256: the game windows' icon, and the Linux package's
    icon.ico   16 to 256: Windows' own format
    icon.res   icon.ico as a compiled Windows resource, which native/build.rs gives the linker
               (so the build needs no resource compiler)

    python3 tools/logo/icon.py TINOS_BOLD_TTF OUT_DIR

Needs Pillow and numpy, and Tinos Bold (Google Fonts, Apache-2.0; not in this repository)."""
import io
import struct
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageFilter
TTF, OUT = sys.argv[1], sys.argv[2]
N = 960
SIZES = (16, 24, 32, 48, 64, 128, 256)
rng = np.random.default_rng(7)
font = lambda px: ImageFont.truetype(TTF, px)
img = np.zeros((N, N, 3), np.float32)
# the slab: a floor in perspective, grey noise fading back
BACK, FRONT = 560, 730
inset = 100
slab = Image.new("L", (N, N), 0)
ImageDraw.Draw(slab).polygon(
    [(50, FRONT), (N - 50, FRONT), (N - 50 - inset, BACK), (50 + inset, BACK)], fill=255)
slab = np.asarray(slab.filter(ImageFilter.GaussianBlur(1)), np.float32) / 255
ys = np.clip((np.arange(N)[:, None] - BACK) / (FRONT - BACK), 0, 1)
noise = rng.random((N, N)).astype(np.float32)
grey = (0.25 + 0.45 * noise) * (0.25 + 0.75 * ys)
for k in range(3):
    img[..., k] += slab * grey * [0.75, 0.75, 0.85][k]
# its front edge, darker: a slab, not a sheet
edge = Image.new("L", (N, N), 0)
ImageDraw.Draw(edge).polygon([(50, FRONT), (N - 50, FRONT), (N - 50, FRONT + 26), (50, FRONT + 26)], fill=255)
edge = np.asarray(edge, np.float32) / 255
for k in range(3):
    img[..., k] += edge * (0.10 + 0.12 * noise) * [0.8, 0.8, 1.0][k]
# two beams of yellow speckled light across it
beams = Image.new("L", (N, N), 0)
d = ImageDraw.Draw(beams)
for cx in (330, 610):
    d.polygon([(cx - 40, BACK), (cx + 20, BACK), (cx + 75, FRONT), (cx - 65, FRONT)], fill=255)
beams = np.asarray(beams.filter(ImageFilter.GaussianBlur(10)), np.float32) / 255
speck = (rng.random((N, N)) < 0.55).astype(np.float32)
light = beams * speck * (0.6 + 0.4 * rng.random((N, N)))
img[..., 0] += light * 1.0
img[..., 1] += light * 0.9
img[..., 2] += light * 0.05
# the words: red, speckled a little, each one's shadow first
for text, px, top in (("EVIL", 250, 150), ("BOOM", 262, 470)):
    layer = Image.new("L", (N, N), 0)
    td = ImageDraw.Draw(layer)
    f = font(px)
    bb = td.textbbox((0, 0), text, font=f)
    td.text(((N - (bb[2] - bb[0])) // 2 - bb[0], top - bb[1]), text, font=f, fill=255)
    mask = np.asarray(layer, np.float32) / 255
    shadow = np.asarray(layer.filter(ImageFilter.GaussianBlur(6)), np.float32) / 255
    img *= (1 - 0.7 * shadow[..., None])
    grain = 0.8 + 0.2 * rng.random((N, N))
    red = np.stack([0.95 * grain, 0.05 * grain, 0.05 * grain], -1)
    img = img * (1 - mask[..., None]) + red * mask[..., None]
full = Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8))
# the tile: black, its corners rounded (clear outside it)
alpha = Image.new("L", (N, N), 0)
ImageDraw.Draw(alpha).rounded_rectangle((0, 0, N - 1, N - 1), radius=int(N * 0.14), fill=255)
full.putalpha(alpha)
at = {s: full.resize((s, s), Image.LANCZOS) for s in SIZES}
at[256].save(f"{OUT}/icon.png")


def ico_image(im):
    """One size as an .ico holds it: 256 as a PNG; smaller as a bitmap (its header, the pixels
    bottom row first as BGRA, then a 1-bit mask of what's clear, rows padded to 32 bits)."""
    s = im.size[0]
    if s >= 256:
        b = io.BytesIO()
        im.save(b, "PNG")
        return b.getvalue()
    header = struct.pack("<IiiHHIIiiII", 40, s, 2 * s, 1, 32, 0, 0, 0, 0, 0, 0)
    px = im.load()
    rows, mask = [], []
    for y in range(s - 1, -1, -1):
        rows.append(b"".join(bytes((px[x, y][2], px[x, y][1], px[x, y][0], px[x, y][3])) for x in range(s)))
        bits = bytearray((s + 31) // 32 * 4)
        for x in range(s):
            if px[x, y][3] == 0:
                bits[x // 8] |= 0x80 >> (x % 8)
        mask.append(bytes(bits))
    return header + b"".join(rows) + b"".join(mask)


images = [(s, ico_image(at[s])) for s in SIZES]
# (an entry: width, height (0: 256), colours, reserved, planes, bits a pixel, bytes; then where
# it is in the file, or in a resource its id)
entry = lambda s, data: struct.pack("<BBBBHHI", s % 256, s % 256, 0, 0, 1, 32, len(data))
ico = struct.pack("<HHH", 0, 1, len(images))
offset = 6 + 16 * len(images)
for s, data in images:
    ico += entry(s, data) + struct.pack("<I", offset)
    offset += len(data)
with open(f"{OUT}/icon.ico", "wb") as f:
    f.write(ico + b"".join(data for _, data in images))


def resource(kind, ident, data, flags):
    """One resource of a .res file: its header (type and name by number), then its data, each
    padded to 4 bytes."""
    head = struct.pack("<IIHHHHIHHII", len(data), 32, 0xFFFF, kind, 0xFFFF, ident, 0, flags, 0x0409, 0, 0)
    return head + data + b"\0" * (-len(data) % 4)


RT_ICON, RT_GROUP_ICON = 3, 14
res = resource(0, 0, b"", 0)  # (the empty one every .res file begins with)
group = struct.pack("<HHH", 0, 1, len(images))
for i, (s, data) in enumerate(images, 1):
    res += resource(RT_ICON, i, data, 0x1010)
    group += entry(s, data) + struct.pack("<H", i)
# (the group named 1: the lowest-numbered icon group is the one Windows shows for the program)
res += resource(RT_GROUP_ICON, 1, group, 0x1030)
with open(f"{OUT}/icon.res", "wb") as f:
    f.write(res)
