"""Mapillary, объекты: что алгоритмы Mapillary распознали на снимках и привязали к карте.

Тайлы mly_map_feature_point и mly_map_feature_traffic_sign (z14) — точки объектов
(положение триангулировано по нескольким кадрам, точность — метры). Берём то, чем
можно заменить догадки генератора:
  street_light   — фонари (вместо расстановки «через каждые 30–40 м»)
  pole           — опоры (контактная сеть трамвая/троллейбуса, ЛЭП, освещение)
  signal / signal_ped — светофоры автомобильные / пешеходные
  bench, trash, hydrant, bike_rack, phone, mailbox, cctv — мелкие предметы
  crosswalk      — разметка «зебра»
  sign_<номер ГОСТ>  — знаки (все виды атласа sign_atlas.py: 1.22, 2.1, 3.27, 5.19.1, 8.24 …)

И растр покрытия съёмкой (10 м): где улица снималась, отсутствие фонаря на снимках —
тоже данные (фонарей там нет), а где не снималась — остаётся процедурная расстановка.

Выход: CACHE/work/mly_features.pkl — {"points": {класс: (N,2) x,z}, "cov": dict(x0, z0, step, mask uint8)}
"""
import glob
import json
import math
import pickle
import time

import numpy as np

import config as C
from mly_common import MLY, lonlat_to_xz

Z = 14
CLASSES = {
    "object--street-light": "street_light",
    "object--support--utility-pole": "pole",
    "object--support--pole": "pole",
    "object--traffic-light--general-upright": "signal",
    "object--traffic-light--general-single": "signal",
    "object--traffic-light--general-horizontal": "signal",
    "object--traffic-light--pedestrians": "signal_ped",
    "object--bench": "bench",
    "object--trash-can": "trash",
    "object--fire-hydrant": "hydrant",
    "object--bike-rack": "bike_rack",
    "object--phone-booth": "phone",
    "object--mailbox": "mailbox",
    "object--cctv-camera": "cctv",
    "marking--discrete--crosswalk-zebra": "crosswalk",
    "construction--flat--crosswalk-plain": "crosswalk",
}
# знаки — все виды из атласа (sign_atlas.MLY_TO_GOST), класс «sign_<номер ГОСТ>»
from sign_atlas import MLY_TO_GOST
CLASSES.update({k: f"sign_{v}" for k, v in MLY_TO_GOST.items()})
COV_STEP = 10.0

# Минимум кадров, на которых объект распознан (mly_features_detail.py). Проверка 2026-10-03 на
# знаках «переход» (17,7 тыс.): у перехода (≤ 30 м) стоят 58–60% распознанных на 1–2 кадрах
# (почти случайность для плотного центра), 70% — на 3, 80% — на 4–5, 87% — на 6–10, 99% — на 11+.
# Остальное — копии одного знака с отдельных проездов, отражения, реклама.
# Фонари: распознанные на 1–3 кадрах в ~25% стоят на отшибе (> 40 м от фонаря, видного на 6+
# кадрах), на 4–5 — в 12%, на 6–10 — в 5%; опоры видны на десятках кадров (медиана 47).
MIN_IMAGES = {"signal": 4, "signal_ped": 4, "street_light": 4}
MIN_IMAGES_SIGN = 5
MIN_IMAGES_DEFAULT = 3


def min_images(cls):
    if cls.startswith("sign_"):
        return MIN_IMAGES_SIGN
    return MIN_IMAGES.get(cls, MIN_IMAGES_DEFAULT)


def load_detail():
    """id -> (кадров, азимут лица) из MLY/features_detail (если этап был)."""
    det = {}
    for f in glob.glob(str(MLY / "features_detail" / "*.jsonl")):
        for line in open(f, encoding="utf-8"):
            r = json.loads(line)
            det[int(r["id"])] = (r["n"], r["dir"])
    return det


def main():
    t0 = time.time()
    pts = {}
    for layer in ("mly_map_feature_point", "mly_map_feature_traffic_sign"):
        import mapbox_vector_tile as mvt
        for f in glob.glob(str(MLY / layer / "*.mvt")):
            tx, ty = (int(v) for v in f.replace("\\", "/").rsplit("/", 1)[1][:-4].split("_"))
            for lay in mvt.decode(open(f, "rb").read()).values():
                for ft in lay["features"]:
                    cls = CLASSES.get(ft["properties"].get("value"))
                    if cls is None:
                        continue
                    gx, gy = ft["geometry"]["coordinates"]
                    lon = (tx + gx / 4096) / 2 ** Z * 360 - 180
                    n = math.pi - 2 * math.pi * (ty + 1 - gy / 4096) / 2 ** Z
                    lat = math.degrees(math.atan(math.sinh(n)))
                    pts.setdefault(cls, []).append((ft["properties"]["id"], lon, lat))
    det = load_detail()
    out, meta = {}, {}
    dropped = {}
    for cls, v in pts.items():
        ids = np.array([int(r[0]) for r in v], dtype=np.int64)
        a = np.array([(r[1], r[2]) for r in v])
        _, first = np.unique(ids, return_index=True)        # объект на стыке тайлов
        ids, a = ids[first], a[first]
        # кадры и направление; объекты без подробностей (опоры вдали от контактной сети) не трогаем
        nd = [det.get(int(i), (-1, None)) for i in ids]
        n = np.array([x[0] for x in nd], np.int32)
        dr = np.array([np.nan if x[1] is None else x[1] for x in nd], np.float32)
        keep = (n < 0) | (n >= min_images(cls))
        dropped[cls] = int((~keep).sum())
        x, z = lonlat_to_xz(a[keep, 0], a[keep, 1])
        out[cls] = np.column_stack([x, z]).astype(np.float32)
        meta[cls] = np.column_stack([n[keep], dr[keep]]).astype(np.float32)
    print("[features] " + ", ".join(f"{k} {len(v)} (−{dropped[k]})" for k, v in sorted(out.items(), key=lambda kv: -len(kv[1]))))

    g = json.load(open(C.WORK / "grid.json"))
    x0, z0 = g["x0"], g["z0"]
    nx = int(g["nx"] * g["step"] / COV_STEP) + 1
    nz = int(g["nz"] * g["step"] / COV_STEP) + 1
    ix = np.load(MLY / "index.npz")
    i = ((ix["x"] - x0) / COV_STEP).astype(np.int64)
    j = ((ix["z"] - z0) / COV_STEP).astype(np.int64)
    ok = (i >= 0) & (i < nx) & (j >= 0) & (j < nz) & ~ix["pano"]
    cnt = np.zeros((nz, nx), np.int32)
    np.add.at(cnt, (j[ok], i[ok]), 1)
    # улица «снята», если вокруг клетки ≥ 3 кадров (один проезд по улице даёт кадр каждые 2–5 м)
    from scipy import ndimage
    near = ndimage.uniform_filter(cnt.astype(np.float32), size=3) * 9
    mask = (near >= 3).astype(np.uint8)
    pickle.dump({"points": out, "meta": meta, "cov": dict(x0=x0, z0=z0, step=COV_STEP, mask=mask)},
                open(C.WORK / "mly_features.pkl", "wb"))
    print(f"[features] покрытие: {mask.sum()} клеток по {COV_STEP:.0f} м — {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
