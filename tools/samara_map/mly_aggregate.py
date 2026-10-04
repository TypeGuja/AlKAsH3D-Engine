"""Mapillary, шаг 5: сводка по зданиям — усреднение всех кадров здания.

Вход: MLY/analysis/*.jsonl (mly_analyze.py) — строка на пару (кадр, здание).
Отбор: поза камеры подогнана уверенно (fit — доля нижнего пояса стен, попавшая на
«здание» по разметке), в пояс не попало небо.

Цвет: взвешенная медиана по кадрам (вес — √пикселей × fit) линейного RGB после баланса
белого по асфальту; экспозиция телефона частично выравнивается по яркости асфальта.
Высота карниза: медиана по кадрам, где над стеной сразу небо; «не ниже» — где дом
выходит за верх кадра. Проверка — на зданиях с building:levels из OSM (печатается).

Высота в карту не идёт (build_chunks): по снимкам с торпеды она хуже оценки по соседям —
см. проверку ниже и комментарий в build_chunks.building_style.

Выход: CACHE/work/bld_photo.pkl — {ключ здания: dict(rgb=(r,g,b) линейное альбедо,
n — кадров с цветом, h — высота карниза, м, h_n — кадров с высотой, h_lo — «не ниже», м)}
"""
import json
import pickle
from collections import defaultdict

import numpy as np

import config as C
from mly_common import MLY, load_buildings

MIN_FIT = 0.35
MAX_SKY_BAND = 0.15
MIN_PX = 60
ROAD_ALBEDO = 0.10             # асфальт (линейное), к нему частично тянется экспозиция
EXPO_POWER = 0.5               # 0 — не трогать экспозицию, 1 — полностью по асфальту
LUM_RANGE = (0.05, 0.70)       # правдоподобное альбедо фасада


def wmedian(v, w):
    o = np.argsort(v)
    c = np.cumsum(w[o])
    return float(v[o][np.searchsorted(c, c[-1] / 2)])


def main():
    sel = np.load(MLY / "select.npz")
    keys = [tuple(map(float, k)) for k in sel["bld_key"]]
    rows = defaultdict(list)
    n_all = n_ok = 0
    for f in sorted((MLY / "analysis").glob("*.jsonl")):
        for line in open(f, encoding="utf-8"):
            r = json.loads(line)
            if r.get("b", -1) < 0:
                continue
            n_all += 1
            if r.get("fit", 0) < MIN_FIT or r.get("sky_band", 1) > MAX_SKY_BAND:
                continue
            n_ok += 1
            rows[r["b"]].append(r)
    print(f"[aggregate] пар {n_all}, с уверенной позой {n_ok}, зданий {len(rows)}")

    out = {}
    for b, rs in rows.items():
        d = {}
        cs, ws = [], []
        for r in rs:
            if "rgb_wb" not in r or r.get("npx", 0) < MIN_PX:
                continue
            c = np.array(r["rgb_wb"], np.float64)
            rl = r.get("road_lum")
            if rl is not None and np.isfinite(rl) and rl > 0.005:
                c *= np.clip((ROAD_ALBEDO / rl) ** EXPO_POWER, 0.6, 1.7)
            cs.append(c)
            ws.append(np.sqrt(r["npx"]) * r["fit"])
        if cs:
            cs, ws = np.array(cs), np.array(ws)
            rgb = np.array([wmedian(cs[:, k], ws) for k in range(3)])
            lum = rgb @ [0.2126, 0.7152, 0.0722]
            lo, hi = LUM_RANGE
            if lum > 0:
                rgb *= np.clip(lum, lo, hi) / lum
            d["rgb"] = tuple(round(float(v), 4) for v in rgb)
            d["n"] = len(cs)
        hs = [(r["h"], r["h_n"]) for r in rs if "h" in r and r.get("h_iqr", 99) < 3.0 and r["fit"] >= 0.4]
        if hs:
            h = np.array([v for v, _ in hs]); w = np.array([n for _, n in hs], float)
            if len(hs) >= 2 or (w[0] >= 12):
                d["h"] = round(wmedian(h, w), 2)
                d["h_n"] = len(hs)
        ge = [r["h_ge"] for r in rs if "h_ge" in r and r["fit"] >= 0.4]
        if len(ge) >= 2:
            lo_ = float(np.sort(ge)[-2])          # второе по величине — устойчиво к одному промаху
            d["h_lo"] = round(lo_, 1)
            if "h" in d and d["h"] < 0.85 * lo_:
                del d["h"]                        # карниз «нашёлся» ниже, чем дом виден — не верим
        if d:
            out[keys[b]] = d
    # Абсолютную яркость по зимним кадрам телефона не восстановить (снег сбивает экспозицию,
    # заснеженный асфальт — плохой эталон). Со снимков берутся оттенок и яркость домов друг
    # относительно друга, а общий уровень по городу приводится к стенам текстур фасадов.
    tex = []
    from PIL import Image
    for m in ("facade_panel", "facade_brick", "facade_historic", "facade_commercial"):
        f = C.OUT / "textures" / f"{m}_albedo.png"
        if f.exists():
            a_ = (np.asarray(Image.open(f).convert("RGB"), np.float64).reshape(-1, 3) / 255.0) ** 2.2
            l_ = a_ @ [0.2126, 0.7152, 0.0722]
            tex.append(float(np.median(l_[l_ >= np.percentile(l_, 33)])))
    lums = np.array([np.array(v["rgb"]) @ [0.2126, 0.7152, 0.0722] for v in out.values() if "rgb" in v])
    if tex and len(lums):
        k = float(np.median(tex)) / float(np.median(lums))
        for v in out.values():
            if "rgb" in v:
                c = np.array(v["rgb"]) * k
                c *= min(1.0, LUM_RANGE[1] / max(float(c @ [0.2126, 0.7152, 0.0722]), 1e-6))
                v["rgb"] = tuple(round(float(x), 4) for x in c)
        print(f"[aggregate] яркость: медиана фото {np.median(lums):.3f} -> стены текстур {np.median(tex):.3f} (×{k:.2f})")
    pickle.dump(out, open(C.WORK / "bld_photo.pkl", "wb"))
    nc = sum("rgb" in v for v in out.values()); nh = sum("h" in v for v in out.values())
    print(f"[aggregate] зданий: с цветом {nc}, с высотой {nh}, с «не ниже» {sum('h_lo' in v for v in out.values())}")

    # проверка высоты на зданиях с этажностью из OSM
    B = {b["key"]: b for b in load_buildings()}
    err, pairs = [], []
    for k, v in out.items():
        b = B.get(k)
        if b is None or "h" not in v or b["levels_tagged"] is None:
            continue
        L = b["levels_tagged"]
        if not 1 <= L <= 40:
            continue
        est = max(1, round((v["h"] - 0.6) / 3.0))
        err.append(est - L)
        pairs.append((L, v["h"]))
    if err:
        e = np.abs(np.array(err))
        P = np.array(pairs)
        fh = np.median(P[:, 1] / P[:, 0])
        print(f"[aggregate] проверка на {len(e)} зданиях с этажностью в OSM: ошибка {e.mean():.2f} эт., "
              f"точно {np.mean(e == 0)*100:.0f}%, ±1 {np.mean(e <= 1)*100:.0f}%, ≥3 {np.mean(e >= 3)*100:.0f}%, "
              f"смещение {np.mean(err):+.2f}; медиана высоты на этаж {fh:.2f} м")
        for lo, hi in ((1, 2), (3, 5), (6, 9), (10, 40)):
            m = (P[:, 0] >= lo) & (P[:, 0] <= hi)
            if m.sum():
                print(f"    {lo}–{hi} эт.: {m.sum()} зданий, ±1 {np.mean(e[m] <= 1)*100:.0f}%, смещение {np.mean(np.array(err)[m]):+.2f}")


if __name__ == "__main__":
    main()
