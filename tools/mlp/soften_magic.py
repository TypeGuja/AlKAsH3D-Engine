"""Смягчить магию рога в готовом пакете MLPO v2 (на месте).

xfl2svg не умеет фильтры Flash (свечение/размытие), поэтому аура магии рендерится
сплошной заливкой. Здесь альфа текстур частей магии становится мягкой и
полупрозрачной: alpha' = k1 * alpha + k2 * blur(alpha). Цвет не трогаем
(после bleed он определён и в прозрачных пикселях).

    python soften_magic.py pack.mlpo [--dry]

Повторный запуск усиливает эффект. all_v3.mlpo (2026-10-14) прошёл два прохода:
(0.45, 0.55, 5) и (0.35, 0.3, 8) — те же, что `soften_alpha` делает в export_official.py,
поэтому пакеты, выгруженные после этого, обрабатывать не нужно.
"""
import re
import struct
import sys

import numpy as np
from PIL import Image, ImageFilter

MAGIC_RE = re.compile(r"(magic|aura)", re.I)
K_SOLID, K_GLOW, RADIUS = 0.35, 0.3, 8
PASSES = ((0.45, 0.55, 5), (0.35, 0.3, 8))


def soften_alpha(alpha):
    """uint8-альфа магии -> мягкая полупрозрачная (оба прохода, как в пакете all_v3)."""
    a = alpha.astype(np.float32)
    for k1, k2, r in PASSES:
        glow = np.asarray(Image.fromarray(a.clip(0, 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(r)), np.float32)
        a = k1 * a + k2 * glow
    return np.clip(a, 0, 255).astype(np.uint8)


def main(path, dry):
    f = open(path, "r+b")

    def rd(fmt):
        n = struct.calcsize(fmt)
        return struct.unpack(fmt, f.read(n))

    assert f.read(4) == b"MLPO"
    ver, _tint, ntex = rd("<IfI")
    assert ver == 2, ver
    tex = []
    for _ in range(ntex):
        w, h = rd("<II")
        tex.append((f.tell(), w, h))
        f.seek(w * h * 4, 1)
    magic = set()
    (nchars,) = rd("<I")
    for _ in range(nchars):
        (nl,) = rd("<I")
        f.read(nl)
        _height, nviews = rd("<fI")
        for _ in range(nviews):
            _fr, _ang, nparts = rd("<ifI")
            for _ in range(nparts):
                ti, _role = rd("<II")
                (sl,) = rd("<I")
                sym = f.read(sl).decode("utf-8")
                f.read(4 * 8)
                _kind, nst = rd("<II")
                ids = [ti]
                for _ in range(nst):
                    ids.append(rd("<I")[0])
                    f.read(24)
                if MAGIC_RE.search(sym):
                    magic.update(i for i in ids if i != 0xFFFFFFFF)
    print("текстур магии:", len(magic))
    if dry:
        return
    for i in sorted(magic):
        off, w, h = tex[i]
        f.seek(off)
        a = np.frombuffer(f.read(w * h * 4), np.uint8).reshape(h, w, 4).copy()
        al = Image.fromarray(a[:, :, 3])
        glow = np.asarray(al.filter(ImageFilter.GaussianBlur(RADIUS)), np.float32)
        a[:, :, 3] = np.clip(K_SOLID * a[:, :, 3] + K_GLOW * glow, 0, 255).astype(np.uint8)
        f.seek(off)
        f.write(a.tobytes())
    f.close()
    print("готово")


if __name__ == "__main__":
    main(sys.argv[1], "--dry" in sys.argv)
