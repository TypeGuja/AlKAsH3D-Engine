"""Быстрый софт-рендер чанков для проверки глазами (без GPU и без движка).

python preview.py gx0 gz0 gx1 gz1 out.png [yaw_deg] [pitch_deg]
Косая ортографическая проекция, алгоритм художника, цвет = средний цвет albedo
материала * ламберт от солнца.
"""
import sys
import math

import numpy as np
from PIL import Image, ImageDraw

import config as C


def load_obj(path):
    V, faces = [], []
    mat = None
    for line in open(path, encoding="utf-8"):
        if line.startswith("v "):
            V.append([float(x) for x in line.split()[1:4]])
        elif line.startswith("usemtl"):
            mat = line.split()[1]
        elif line.startswith("f "):
            idx = [int(p.split("/")[0]) - 1 for p in line.split()[1:4]]
            faces.append((idx, mat))
    V = np.array(V)
    return V, faces


def main():
    gx0, gz0, gx1, gz1 = map(int, sys.argv[1:5])
    out = sys.argv[5]
    yaw = math.radians(float(sys.argv[6]) if len(sys.argv) > 6 else 30)
    pitch = math.radians(float(sys.argv[7]) if len(sys.argv) > 7 else 40)
    colors = {}
    tris, mats = [], []
    for gx in range(gx0, gx1 + 1):
        for gz in range(gz0, gz1 + 1):
            p = C.OUT / "chunks" / f"chunk_{gx}_{gz}.obj"
            if not p.exists():
                continue
            V, faces = load_obj(p)
            for idx, m in faces:
                tris.append(V[idx]); mats.append(m)
    T = np.array(tris)
    for m in set(mats):
        im = Image.open(C.OUT / "textures" / f"{m}_albedo.png").convert("RGB").resize((8, 8))
        colors[m] = np.asarray(im, np.float64).reshape(-1, 3).mean(0) / 255
    n = np.cross(T[:, 1] - T[:, 0], T[:, 2] - T[:, 0])
    n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-9)
    sun = np.array([-0.4, 0.8, -0.45]); sun /= np.linalg.norm(sun)
    # камера: смотрит с юго-запада вниз под углом pitch
    fwd = np.array([math.sin(yaw) * math.cos(pitch), -math.sin(pitch), -math.cos(yaw) * math.cos(pitch)])
    right = np.cross(fwd, [0, 1, 0]); right /= np.linalg.norm(right)
    up = np.cross(right, fwd)
    c = T.reshape(-1, 3).mean(0)
    rel = T - c
    sx = rel @ right; sy = rel @ up; depth = (rel @ fwd).mean(1)
    facing = (n @ -fwd) > 0                                   # отсечение задних граней
    W = 1800
    span = max(np.ptp(sx), np.ptp(sy))
    s = W / span
    H = int(np.ptp(sy) * s) + 20
    img = Image.new("RGB", (W + 20, H), (150, 180, 210))
    d = ImageDraw.Draw(img)
    order = np.argsort(-depth)
    lam = np.clip(n @ sun, 0, 1) * 0.75 + 0.3
    for i in order:
        if not facing[i]:
            continue
        col = np.clip(colors[mats[i]] * lam[i], 0, 1)
        pts = [((sx[i, k] - sx.min()) * s + 10, H - 10 - (sy[i, k] - sy.min()) * s) for k in range(3)]
        d.polygon(pts, fill=tuple(int(v * 255) for v in col))
    img.save(out)
    print(out, len(T), "треугольников")


if __name__ == "__main__":
    main()
