# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Quarkboy
"""The Evil BOOM logo: red serif letters over a slab in perspective with speckled yellow light
across it, a homage to Boom's own. Writes the full size and the stream column's (444 wide, as
native/assets/logo.png).

    python3 tools/logo/make.py TINOS_BOLD_TTF OUT_DIR

Needs Pillow and numpy, and Tinos Bold (Google Fonts, Apache-2.0; not in this repository)."""
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageFilter
TTF, OUT = sys.argv[1], sys.argv[2]
W, H = 960, 440
rng = np.random.default_rng(7)
font = lambda px: ImageFont.truetype(TTF, px)
img = np.zeros((H, W, 3), np.float32)
# the slab: a floor in perspective, grey noise fading back
slab = Image.new("L", (W, H), 0)
ImageDraw.Draw(slab).polygon([(60, 330), (900, 330), (830, 200), (130, 200)], fill=255)
slab = np.asarray(slab.filter(ImageFilter.GaussianBlur(1)), np.float32) / 255
ys = np.linspace(0, 1, H)[:, None]
noise = rng.random((H, W)).astype(np.float32)
grey = (0.25 + 0.45 * noise) * np.clip((ys - 0.42) * 2.2, 0.15, 1.0)
for k in range(3):
    img[..., k] += slab * grey * [0.75, 0.75, 0.85][k]
# its front edge, darker: a slab, not a sheet
edge = Image.new("L", (W, H), 0)
ImageDraw.Draw(edge).polygon([(60, 330), (900, 330), (900, 348), (60, 348)], fill=255)
edge = np.asarray(edge, np.float32) / 255
for k in range(3):
    img[..., k] += edge * (0.10 + 0.12 * noise) * [0.8, 0.8, 1.0][k]
# two beams of yellow speckled light across it
beams = Image.new("L", (W, H), 0)
d = ImageDraw.Draw(beams)
for cx in (330, 600):
    d.polygon([(cx - 40, 200), (cx + 20, 200), (cx + 70, 330), (cx - 60, 330)], fill=255)
beams = np.asarray(beams.filter(ImageFilter.GaussianBlur(10)), np.float32) / 255
speck = (rng.random((H, W)) < 0.55).astype(np.float32)
light = beams * speck * (0.6 + 0.4 * rng.random((H, W)))
img[..., 0] += light * 1.0
img[..., 1] += light * 0.9
img[..., 2] += light * 0.05
# the letters: red, speckled a little, their shadow first
txt = Image.new("L", (W, H), 0)
td = ImageDraw.Draw(txt)
f = font(250)
bb = td.textbbox((0, 0), "BOOM", font=f)
x0 = (W - (bb[2] - bb[0])) // 2 - bb[0]
td.text((x0, 140 - bb[1]), "BOOM", font=f, fill=255)
mask = np.asarray(txt, np.float32) / 255
shadow = np.asarray(txt.filter(ImageFilter.GaussianBlur(6)), np.float32) / 255
img *= (1 - 0.7 * shadow[..., None])
grain = 0.8 + 0.2 * rng.random((H, W))
red = np.stack([0.95 * grain, 0.05 * grain, 0.05 * grain], -1)
img = img * (1 - mask[..., None]) + red * mask[..., None]
# "Evil" above, where the original's credit was
top = Image.new("L", (W, H), 0)
tdraw = ImageDraw.Draw(top)
f2 = font(92)
bb2 = tdraw.textbbox((0, 0), "Evil", font=f2)
tdraw.text(((W - (bb2[2] - bb2[0])) // 2 - bb2[0], 28 - bb2[1]), "Evil", font=f2, fill=255)
tm = np.asarray(top, np.float32) / 255
glow = np.asarray(top.filter(ImageFilter.GaussianBlur(8)), np.float32) / 255
img += glow[..., None] * np.array([0.45, 0.0, 0.25]) * 0.8
img = img * (1 - tm[..., None]) + np.array([0.9, 0.08, 0.1]) * tm[..., None]
out = Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8)).crop((40, 10, W - 40, 368))
out.save(f"{OUT}/logo-full.png")
out.resize((444, int(444 * out.height / out.width)), Image.LANCZOS).save(f"{OUT}/logo.png")
