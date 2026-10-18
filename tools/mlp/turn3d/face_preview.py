"""Сборка ракурса из пакета MLPO v2 с эмоцией — как в движке (official.rs), на CPU.

python face_preview.py pack.mlpo out.png [персонаж]
"""
import struct
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

NONE = 0xFFFFFFFF
EYES = {"normal": 0, "half": 10, "squinty": 20, "cheerie": 30, "sad": 50}
PHON = {"rest": 0, "aei": 4, "rou": 11, "s": 14, "l": 18, "f": 21, "th": 23, "mbp": 25}


def read(path, want=None):
    b = open(path, "rb").read()
    o = 4
    ver, tint, nt = struct.unpack_from("<IfI", b, o); o += 12
    tex = []
    for _ in range(nt):
        w, h = struct.unpack_from("<II", b, o); o += 8
        tex.append((o, w, h)); o += w * h * 4
    nc, = struct.unpack_from("<I", b, o); o += 4
    chars = []
    for _ in range(nc):
        l, = struct.unpack_from("<I", b, o); o += 4
        name = b[o:o + l].decode(); o += l
        hgt, nv = struct.unpack_from("<fI", b, o); o += 8
        views = []
        for _ in range(nv):
            fr, ang, np_ = struct.unpack_from("<ifI", b, o); o += 12
            parts = []
            for _ in range(np_):
                ti, role, ln = struct.unpack_from("<III", b, o); o += 12
                sym = b[o:o + ln].decode(); o += ln
                aff = struct.unpack_from("<6f", b, o); o += 24
                o += 8
                kind, ns = (0, 0)
                if ver >= 2:
                    kind, ns = struct.unpack_from("<II", b, o); o += 8
                st = []
                for _ in range(ns):
                    sti, = struct.unpack_from("<I", b, o); o += 4
                    st.append((sti, struct.unpack_from("<6f", b, o))); o += 24
                parts.append((ti, role, sym, aff, kind, st))
            views.append((fr, ang, parts))
        chars.append((name, hgt, views))
    return b, tex, chars


def img(b, tex, ti):
    o, w, h = tex[ti]
    a = np.frombuffer(b, np.uint8, w * h * 4, o).reshape(h, w, 4).copy()
    return Image.fromarray(a, "RGBA")


def frame_of(kind, e):
    if kind == 1:
        return EYES[e["eyes"]] + e["blink"] + (100 if e["brow"] is not None else 0)
    if kind in (3, 4):
        return (0 if e["mood"] == "happy" else 40) + PHON[e["ph"]]
    if kind == 2:
        return e["brow"]
    if kind == 5:
        return e["blush"]
    return 0


def render(b, tex, parts, e, T=360, k=None, crop=None):
    canvas = Image.new("RGBA", (T, T), (255, 255, 255, 255))
    k = k or T / 1.3
    for ti, role, sym, aff, kind, st in parts:
        if role == 9:
            continue
        if kind:
            f = frame_of(kind, e)
            if f is None:
                continue
            sti, aff = st[min(f, len(st) - 1)]
            ti = sti
        if ti == NONE:
            continue
        A, B, C, D, E, F = aff
        im = img(b, tex, ti)
        M = [k * A, k * C, T / 2 + k * E, -k * B, -k * D, T - 10 - k * F]
        det = M[0] * M[4] - M[1] * M[3]
        inv = [M[4] / det, -M[1] / det, 0, -M[3] / det, M[0] / det, 0]
        inv[2] = -(inv[0] * M[2] + inv[1] * M[5])
        inv[5] = -(inv[3] * M[2] + inv[4] * M[5])
        canvas.alpha_composite(im.transform((T, T), Image.AFFINE, inv, resample=Image.BICUBIC))
    return canvas.crop(crop) if crop else canvas


if __name__ == "__main__":
    pack, out = sys.argv[1], sys.argv[2]
    who = sys.argv[3] if len(sys.argv) > 3 else None
    b, tex, chars = read(pack)
    name, hgt, views = next(c for c in chars if who is None or c[0] == who)
    exprs = [
        dict(eyes="normal", blink=0, mood="happy", ph="rest", brow=None, blush=None),
        dict(eyes="normal", blink=2, mood="happy", ph="rest", brow=None, blush=None),
        dict(eyes="half", blink=1, mood="sad", ph="rest", brow=3, blush=None),
        dict(eyes="squinty", blink=1, mood="happy", ph="aei", brow=6, blush=None),
        dict(eyes="cheerie", blink=1, mood="happy", ph="aei", brow=None, blush=1),
        dict(eyes="sad", blink=1, mood="sad", ph="rou", brow=9, blush=None),
        dict(eyes="normal", blink=0, mood="happy", ph="mbp", brow=1, blush=None),
        dict(eyes="normal", blink=0, mood="sad", ph="s", brow=None, blush=None),
    ]
    vsel = [1, 3, 5, 9, 13, 17, 22]  # 0°, 11°, 22.5°, 45°, 67.5°, 90°, 118°
    T = 300
    font = ImageFont.truetype("arial.ttf", 12)
    sheet = Image.new("RGB", (T * len(exprs), T * len(vsel) // 2 + 20), "white")
    d = ImageDraw.Draw(sheet)
    for c, e in enumerate(exprs):
        d.text((c * T + 4, 2), f"{e['eyes']} b{e['blink']} {e['mood']} {e['ph']} brow={e['brow']} blush={e['blush']}", fill="black", font=font)
        for r, vi in enumerate(vsel):
            fr, ang, parts = views[vi]
            im = render(b, tex, parts, e, T=T * 2, crop=(0, 0, T * 2, T))  # верх — голова
            im = im.resize((T, T // 2))
            sheet.paste(im.convert("RGB"), (c * T, 20 + r * T // 2))
            d.text((c * T + 4, 20 + r * T // 2 + 2), f"{np.degrees(ang):.1f}°", fill="red", font=font)
    sheet.save(out)
    print(out, sheet.size)
