"""Mapillary, шаг 1: индекс всех снимков над картой из векторных тайлов покрытия.

Тайлы z14 слоя mly1_public (по одному запросу на тайл) содержат каждую точку
съёмки с курсом, временем, качеством и id — этого хватает, чтобы выбрать нужные
кадры, не дёргая API на каждый из ~7.5 млн снимков.

Выход: MLY/index.npz — x, z (м, координаты карты), compass (°), t (мс UTC),
quality, id (int64), seq (хэш последовательности), pano, sun (высота солнца, °).
"""
import math
import time
import zlib
from concurrent.futures import ThreadPoolExecutor

import numpy as np
import requests

import config as C
from mly_common import MLY, lonlat_to_xz, sun_elevation, token

Z = 14


def tile_range():
    import json
    g = json.load(open(C.WORK / "grid.json"))
    from mly_common import transformer
    tr = transformer(to_local=False)
    x0, z0 = g["x0"], g["z0"]
    x1, z1 = x0 + g["nx"] * g["step"], z0 + g["nz"] * g["step"]
    ll = [tr.transform(x, -z) for x in (x0, x1) for z in (z0, z1)]
    W, E = min(p[0] for p in ll), max(p[0] for p in ll)
    S, N = min(p[1] for p in ll), max(p[1] for p in ll)
    tx = lambda lon: int((lon + 180) / 360 * 2 ** Z)
    ty = lambda lat: int((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * 2 ** Z)
    return [(x, y) for x in range(tx(W), tx(E) + 1) for y in range(ty(N), ty(S) + 1)]


def main():
    import mapbox_vector_tile as mvt
    t0 = time.time()
    cache = MLY / "cov14"
    cache.mkdir(parents=True, exist_ok=True)
    tok = token()

    def get(t):
        p = cache / f"{t[0]}_{t[1]}.mvt"
        if not p.exists():
            r = requests.get(f"https://tiles.mapillary.com/maps/vtp/mly1_public/2/{Z}/{t[0]}/{t[1]}?access_token={tok}", timeout=60)
            r.raise_for_status()
            p.write_bytes(r.content)
        return t, p.read_bytes()

    cols = {k: [] for k in ("lon", "lat", "compass", "t", "quality", "id", "seq", "pano")}
    tiles = tile_range()
    with ThreadPoolExecutor(8) as ex:
        for (tx, ty), b in ex.map(get, tiles):
            feats = mvt.decode(b).get("image", {}).get("features", [])
            if not feats:
                continue
            ext = 4096
            gx = np.array([f["geometry"]["coordinates"][0] for f in feats], float)
            gy = np.array([f["geometry"]["coordinates"][1] for f in feats], float)
            cols["lon"].append((tx + gx / ext) / 2 ** Z * 360 - 180)
            n = math.pi - 2 * math.pi * (ty + 1 - gy / ext) / 2 ** Z
            cols["lat"].append(np.degrees(np.arctan(np.sinh(n))))
            P = [f["properties"] for f in feats]
            cols["compass"].append(np.array([p.get("compass_angle", np.nan) for p in P], np.float32))
            cols["t"].append(np.array([p.get("captured_at", 0) for p in P], np.int64))
            cols["quality"].append(np.array([p.get("quality_score", 0.5) for p in P], np.float32))
            cols["id"].append(np.array([int(p["id"]) for p in P], np.int64))
            cols["seq"].append(np.array([zlib.crc32(str(p.get("sequence_id", "")).encode()) for p in P], np.uint32))
            cols["pano"].append(np.array([bool(p.get("is_pano")) for p in P]))
    a = {k: np.concatenate(v) for k, v in cols.items()}
    _, first = np.unique(a["id"], return_index=True)         # точки на границе тайлов
    a = {k: v[first] for k, v in a.items()}
    x, z = lonlat_to_xz(a["lon"], a["lat"])
    sun = sun_elevation(a["t"], a["lon"], a["lat"]).astype(np.float32)
    np.savez(MLY / "index.npz", x=x.astype(np.float32), z=z.astype(np.float32), compass=a["compass"], t=a["t"],
             quality=a["quality"], id=a["id"], seq=a["seq"], pano=a["pano"], sun=sun)
    print(f"[mly_index] тайлов {len(tiles)}, снимков {len(x)}, днём (солнце > 5°) {np.mean(sun > 5)*100:.0f}% — {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
