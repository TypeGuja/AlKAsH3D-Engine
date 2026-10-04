"""Склейка чанков в один OBJ (группы по материалам, а не по чанкам).

python merge.py OUT.obj [gx0 gz0 gx1 gz1] [--yshift auto|<метры>]
Без диапазона — все чанки. --yshift auto: опустить всё так, чтобы земля у
начала координат (пл. Куйбышева) оказалась на Y≈0 (удобно для редактора).
"""
import io
import re
import sys
import time
from collections import defaultdict

import numpy as np

import config as C


def parse_chunk(text):
    """-> V, VT, VN (массивы) и {mat: faces (n,9) 1-based локальные индексы}."""
    V, VT, VN = [], [], []
    faces = defaultdict(list)
    mat = None
    for block in re.split(r"^usemtl ", text, flags=re.M)[1:]:
        mat, body = block.split("\n", 1)
        v = re.findall(r"^v (.+)$", body, re.M)
        vt = re.findall(r"^vt (.+)$", body, re.M)
        vn = re.findall(r"^vn (.+)$", body, re.M)
        f = re.findall(r"^f (.+)$", body, re.M)
        V.append(np.array(" ".join(v).split(), float).reshape(-1, 3) if v else np.zeros((0, 3)))
        VT.append(np.array(" ".join(vt).split(), float).reshape(-1, 2) if vt else np.zeros((0, 2)))
        VN.append(np.array(" ".join(vn).split(), float).reshape(-1, 3) if vn else np.zeros((0, 3)))
        if f:
            faces[mat].append(np.array(" ".join(f).replace("/", " ").split(), np.int64).reshape(-1, 9))
    return (np.concatenate(V), np.concatenate(VT), np.concatenate(VN),
            {m: np.concatenate(a) for m, a in faces.items()})


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    out = C.OUT / args[0]
    yshift = 0.0
    if "--yshift" in sys.argv:
        val = sys.argv[sys.argv.index("--yshift") + 1]
        args = [a for a in args if a != val]
        if val == "auto":
            import json
            g = json.load(open(C.WORK / "grid.json"))
            H = np.load(C.WORK / "heights.npy", mmap_mode="r")
            yshift = -round(float(H[int(-g["z0"] / g["step"]), int(-g["x0"] / g["step"])]))
        else:
            yshift = float(val)
    files = sorted((C.OUT / "chunks").glob("chunk_*.obj"))
    if len(args) >= 5:
        gx0, gz0, gx1, gz1 = map(int, args[1:5])
        keep = []
        for f in files:
            gx, gz = map(int, f.stem.split("_")[1:3])
            if gx0 <= gx <= gx1 and gz0 <= gz <= gz1:
                keep.append(f)
        files = keep
    t0 = time.time()
    tmp = {}
    ov = ot = on = 0
    tris = 0
    with open(out, "w", newline="\n", encoding="utf-8") as fo:
        fo.write(f"# Самара — склейка {len(files)} чанков 256 м (tools/samara_map/merge.py)\n")
        fo.write("# X восток, Y вверх, Z юг, метры; начало координат — пл. Куйбышева.\n")
        fo.write(f"# Сдвиг по высоте: Y_файла = высота_над_морем {yshift:+.0f} м\n" if yshift else
                 "# Y = высота над уровнем моря (м)\n")
        fo.write("# (c) OpenStreetMap contributors (ODbL); рельеф Copernicus DEM GLO-30 (c) DLR/Airbus/ESA\n")
        fo.write("mtllib samara.mtl\n")
        for k, f in enumerate(files):
            V, VT, VN, faces = parse_chunk(f.read_text(encoding="utf-8"))
            if yshift:
                V[:, 1] += yshift
            np.savetxt(fo, V, fmt="v %.2f %.2f %.2f")
            np.savetxt(fo, VT, fmt="vt %.3f %.3f")
            np.savetxt(fo, VN, fmt="vn %.3f %.3f %.3f")
            for m, F in faces.items():
                F = F + np.array([ov, ot, on] * 3)
                if m not in tmp:
                    tmp[m] = open(C.OUT_WORK / f"_merge_{out.stem}_{m}.txt", "w", newline="\n")
                buf = tmp[m]
                np.savetxt(buf, F, fmt="f %d/%d/%d %d/%d/%d %d/%d/%d")
                tris += len(F)
            ov += len(V); ot += len(VT); on += len(VN)
            if k % 500 == 0:
                print(f"[merge] {k}/{len(files)} — {time.time()-t0:.0f}s", flush=True)
        for m, buf in sorted(tmp.items()):
            buf.close()
            fo.write(f"o {m}\nusemtl {m}\n")
            with open(C.OUT_WORK / f"_merge_{out.stem}_{m}.txt", encoding="utf-8") as fi:
                while True:
                    s = fi.read(1 << 24)
                    if not s:
                        break
                    fo.write(s)
            (C.OUT_WORK / f"_merge_{out.stem}_{m}.txt").unlink()
    print(f"[merge] {out.name}: {len(files)} чанков, {tris:,} треугольников, {ov:,} вершин, "
          f"yshift {yshift:+.0f}, {out.stat().st_size/2**20:.0f} МБ, {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
