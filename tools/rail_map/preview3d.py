"""Программный рендер OBJ-чанков железной дороги — посмотреть сборку, не запуская движок.

  python -I preview3d.py <папка_3d> --km 110.5 --look 1.2 [--off 6 --h 2] [--out shot.png]

Камера ставится на маршрут в точке --km (км от начала выгрузки), смещена вбок на --off м,
на высоту --h м над головкой рельса, и смотрит на точку маршрута через --look км.
Цвет = средний цвет albedo-текстуры материала × освещение (солнце + небо) + туман.
"""
import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image
from pyproj import Transformer
from shapely.geometry import LineString

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_rail3d as B  # noqa: E402

W, H = 1600, 900
FOV = math.radians(60)
SUN = np.array([-0.45, 0.75, 0.35])
SUN /= np.linalg.norm(SUN)
SKY_TOP, SKY_HOR = np.array([0.38, 0.55, 0.82]), np.array([0.80, 0.86, 0.92])
FOG_DIST = 4000.0


def load_obj(path):
    V, N, mats, faces = [], [], [], []
    vb = nb = 0
    cur = None
    lv, ln = [], []
    with open(path, encoding="utf-8") as f:
        for line in f:
            c = line[:2]
            if c == "v ":
                lv.append(line[2:])
            elif c == "vn":
                ln.append(line[3:])
            elif c == "f ":
                faces.append((cur, line[2:]))
            elif line.startswith("usemtl"):
                cur = line.split()[1]
    if not faces:
        return None
    V = np.loadtxt(lv, dtype=np.float32).reshape(-1, 3)
    N = np.loadtxt(ln, dtype=np.float32).reshape(-1, 3)
    out = {}
    by = {}
    for m, s in faces:
        by.setdefault(m, []).append(s.replace("/", " "))
    for m, lines in by.items():
        a = np.loadtxt(lines, dtype=np.int64).reshape(-1, 9)
        out[m] = (V[a[:, [0, 3, 6]] - 1], N[a[:, [2, 5, 8]] - 1])
    return out


def mat_colors(d):
    cols = {}
    for m in B.MATS:
        p = d / "textures" / f"{m}_albedo.png"
        if p.exists():
            a = np.asarray(Image.open(p).convert("RGB"), np.float32) / 255.0
            cols[m] = (a.reshape(-1, 3).mean(0)) ** 2.2   # sRGB -> линейный
        else:
            cols[m] = np.array([0.5, 0.5, 0.5])
    return cols


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dir")
    ap.add_argument("--name", default="samara_syzran")
    ap.add_argument("--km", type=float, required=True)
    ap.add_argument("--look", type=float, default=1.0)
    ap.add_argument("--off", type=float, default=6.0)
    ap.add_argument("--h", type=float, default=2.0)
    ap.add_argument("--radius", type=float, default=1600.0)
    ap.add_argument("--out")
    a = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")
    d = Path(a.dir)
    route = json.loads((B.ROOT / "data" / "rail_map" / a.name / "route.geojson").read_text(encoding="utf-8"))
    tr = Transformer.from_crs("EPSG:4326", B.PROJ, always_xy=True)
    co = route["features"][0]["geometry"]["coordinates"]
    xs, ys = tr.transform([c[0] for c in co], [c[1] for c in co])
    line = LineString(list(zip(xs, -np.asarray(ys))))
    p0 = np.array(line.interpolate(a.km * 1000).coords[0])
    p1 = np.array(line.interpolate(a.km * 1000 + 5).coords[0])
    pl = np.array(line.interpolate((a.km + a.look) * 1000).coords[0])
    t = (p1 - p0) / np.linalg.norm(p1 - p0)
    r = np.array([-t[1], t[0]])
    lat = np.load(d / "_lattice.npz")
    F, X0, Z0 = lat["F"], float(lat["X0"]), float(lat["Z0"])
    rail_top = B.BALLAST_H + B.SLEEPER_UP + 0.015 + B.RAIL_H
    cam2 = p0 + r * a.off
    fy = float(B.lattice_sample(F, [p0[0]], [p0[1]], X0, Z0)[0])
    cam = np.array([cam2[0], fy + rail_top + a.h, cam2[1]])
    fl = float(B.lattice_sample(F, [pl[0]], [pl[1]], X0, Z0)[0])
    tgt = np.array([pl[0], fl + rail_top + 1.0, pl[1]])

    fwd = tgt - cam
    fwd /= np.linalg.norm(fwd)
    right = np.cross(fwd, [0, 1, 0])
    right /= np.linalg.norm(right)
    up = np.cross(right, fwd)
    f = (W / 2) / math.tan(FOV / 2)

    cols = mat_colors(d)
    man = json.loads((d / "manifest.json").read_text(encoding="utf-8"))
    img = np.zeros((H, W, 3), np.float32)
    yy = np.linspace(1, -1, H)[:, None]
    sky = SKY_HOR + (SKY_TOP - SKY_HOR) * np.clip(yy * 1.5 + 0.2, 0, 1)
    img[:] = sky[:, None, :].reshape(H, 1, 3)
    zbuf = np.full((H, W), np.inf, np.float32)
    ntri = 0
    for c in man["chunks"]:
        cx, cz = (c["gx"] + 0.5) * B.CHUNK, (c["gz"] + 0.5) * B.CHUNK
        v = np.array([cx - cam[0], cz - cam[2]])
        dist = np.linalg.norm(v)
        if dist > a.radius:
            continue
        if dist > 400 and np.dot(v / dist, fwd[[0, 2]] / np.linalg.norm(fwd[[0, 2]])) < 0.3:
            continue
        o = load_obj(d / c["file"])
        if not o:
            continue
        for m, (P, N) in o.items():
            rel = P - cam
            zc = rel @ fwd
            vis = (zc > 0.3).all(1)
            P, N, rel, zc = P[vis], N[vis], rel[vis], zc[vis]
            if not len(P):
                continue
            sx = W / 2 + f * (rel @ right) / zc
            sy = H / 2 - f * (rel @ up) / zc
            n = N.mean(1)
            n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-9)
            lam = np.clip(n @ SUN, 0, 1)
            amb = 0.35 + 0.15 * n[:, 1]
            shade = (lam * 1.0 + amb)[:, None] * cols.get(m, np.array([0.5, 0.5, 0.5]))
            depth = zc.mean(1)
            fog = np.clip(depth / FOG_DIST, 0, 1)[:, None] ** 1.2
            colr = shade * (1 - fog) + SKY_HOR * fog
            x0 = np.clip(np.floor(sx.min(1)), 0, W - 1).astype(int)
            x1 = np.clip(np.ceil(sx.max(1)), 0, W - 1).astype(int)
            y0 = np.clip(np.floor(sy.min(1)), 0, H - 1).astype(int)
            y1 = np.clip(np.ceil(sy.max(1)), 0, H - 1).astype(int)
            onscreen = (sx.max(1) >= 0) & (sx.min(1) < W) & (sy.max(1) >= 0) & (sy.min(1) < H)
            for i in np.nonzero(onscreen)[0]:
                bx0, bx1, by0, by1 = x0[i], x1[i], y0[i], y1[i]
                if bx1 - bx0 > W or by1 - by0 > H:
                    continue
                gxs, gys = np.meshgrid(np.arange(bx0, bx1 + 1) + 0.5, np.arange(by0, by1 + 1) + 0.5)
                (ax, bx_, cx_), (ay, by_, cy_) = sx[i], sy[i]
                den = (by_ - cy_) * (ax - cx_) + (cx_ - bx_) * (ay - cy_)
                if abs(den) < 1e-9:
                    continue
                w0 = ((by_ - cy_) * (gxs - cx_) + (cx_ - bx_) * (gys - cy_)) / den
                w1 = ((cy_ - ay) * (gxs - cx_) + (ax - cx_) * (gys - cy_)) / den
                w2 = 1 - w0 - w1
                inside = (w0 >= -1e-4) & (w1 >= -1e-4) & (w2 >= -1e-4)
                if not inside.any():
                    continue
                iz = w0 / zc[i, 0] + w1 / zc[i, 1] + w2 / zc[i, 2]
                z = 1.0 / np.maximum(iz, 1e-9)
                sub = zbuf[by0:by1 + 1, bx0:bx1 + 1]
                win = inside & (z < sub)
                sub[win] = z[win]
                img[by0:by1 + 1, bx0:bx1 + 1][win] = colr[i]
                ntri += 1
    out = Path(a.out) if a.out else d / f"preview_km{a.km:g}.png"
    rgb = np.clip(img ** (1 / 2.2), 0, 1)
    Image.fromarray((rgb * 255).astype(np.uint8)).save(out)
    print(f"{ntri} треугольников → {out}")


if __name__ == "__main__":
    main()
