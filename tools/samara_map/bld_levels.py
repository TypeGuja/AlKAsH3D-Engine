"""Этап 1б: этажность зданий без тегов — по соседям, у которых она есть в OSM.

`building:levels` есть только у ~17.7 тыс. из 92 тыс. зданий, остальным этажность
раньше угадывалась по площади (building_style). Дома одного квартала обычно одной
серии, поэтому лучшая оценка — взвешенная медиана этажности ближайших
размеченных зданий той же группы и похожей площади. Где таких соседей нет —
медиана по размеченным зданиям того же типа и класса площади; где и её нет —
остаётся старая догадка.

Проверка на самих размеченных зданиях (их тег прячется) печатается в лог.
Поверхность Copernicus (DSM) для этого не годится: проверено 2026-10-02, бугор
над 16–26-этажными домами 1–5 м, AUC «высокий/низкий» 0.43 — хуже случайного.

Выход: CACHE/work/bld_levels.pkl — {(round(x,1), round(z,1)) точки representative_point: этажей}
"""
import pickle
import time

import numpy as np
from scipy.spatial import cKDTree
from shapely import wkb as swkb

import config as C
from build_chunks import COMMERCIAL, GARAGE, HOUSE, INDUSTRIAL, PUBLIC, SHED, WORSHIP, num

KNN_K = 12             # соседей
KNN_RADIUS = 250.0     # м
AREA_RATIO = 2.5       # сосед годится, если площади отличаются не больше чем во столько раз
MIN_AREA = 60.0        # меньше — сараи/гаражи, догадка по площади и так верна
AREA_BINS = np.array([0, 60, 100, 150, 250, 400, 700, 1200, 2000, 4000, 1e9])
TABLE_MIN = 15         # зданий в клетке таблицы «тип × площадь», чтобы ей верить


def group_of(b):
    if b in GARAGE or b in SHED or b in WORSHIP or b == "roof":
        return None                                    # не оцениваем
    if b in INDUSTRIAL:
        return "ind"
    if b in COMMERCIAL:
        return "com"
    if b in PUBLIC:
        return "pub"
    if b in HOUSE:
        return "house"
    return "res"                                       # yes / apartments / residential / ...


def key_of(g):
    rp = g.representative_point()
    return round(rp.x, 1), round(rp.y, 1)


def main():
    t0 = time.time()
    keys, xs, zs, areas, types, groups, lv = [], [], [], [], [], [], []
    seen = set()
    for f in sorted((C.WORK / "bins").glob("st_*.pkl")):
        for kind, t, w in pickle.load(open(f, "rb")):
            if kind != "building":
                continue
            g = swkb.loads(w)
            k = key_of(g)
            if k in seen:
                continue
            seen.add(k)
            b = t.get("building", "yes")
            v = num(t.get("building:levels"))
            if v is None and num(t.get("height")) is not None:
                v = -1.0                                   # высота есть — этажность не нужна
            keys.append(k); xs.append(k[0]); zs.append(k[1]); areas.append(g.area)
            types.append(b); groups.append(group_of(b)); lv.append(np.nan if v is None else v)
    X = np.column_stack([xs, zs]); A = np.array(areas); LV = np.array(lv)
    T = np.array(types); G = np.array([g or "" for g in groups])
    ab = np.digitize(A, AREA_BINS)
    tagged = (LV >= 1) & (LV <= 40) & (G != "")
    print(f"[levels] зданий {len(keys)}, с этажностью {tagged.sum()} — {time.time()-t0:.0f}s")

    trees = {}
    for grp in set(G[tagged]):
        idx = np.where(tagged & (G == grp))[0]
        trees[grp] = (idx, cKDTree(X[idx]))

    def knn(i):
        if G[i] not in trees:
            return None
        idx, tree = trees[G[i]]
        d, j = tree.query(X[i], k=KNN_K + 1, distance_upper_bound=KNN_RADIUS)
        ok = np.isfinite(d)
        d, jj = d[ok], idx[j[ok]]
        keep = jj != i
        d, jj = d[keep], jj[keep]
        ar = np.abs(np.log(A[jj] / A[i]))
        m = ar < np.log(AREA_RATIO)
        if m.sum() < 2:
            return None
        wgt = 1 / (1 + d[m] / 50) / (1 + 2 * ar[m])
        v = LV[jj][m]
        o = np.argsort(v)
        cw = np.cumsum(wgt[o])
        return float(v[o][np.searchsorted(cw, cw[-1] / 2)])

    cells = {}
    for i in np.where(tagged)[0]:
        cells.setdefault((T[i], ab[i]), []).append(LV[i])
        cells.setdefault((G[i], ab[i]), []).append(LV[i])
    table = {k: float(np.median(v)) for k, v in cells.items() if len(v) >= TABLE_MIN}

    def predict(i):
        p = knn(i)
        if p is not None:
            return p, "knn"
        p = table.get((T[i], ab[i]), table.get((G[i], ab[i])))
        return (p, "table") if p is not None else (None, None)

    # проверка: прячем тег у каждого размеченного здания
    ti = np.where(tagged & (A >= MIN_AREA))[0]
    pr = [predict(i)[0] for i in ti]
    have = np.array([p is not None for p in pr])
    e = np.abs(np.array([p for p in pr if p is not None]) - LV[ti][have])
    print(f"[levels] проверка на {have.sum()} размеченных: ошибка {e.mean():.2f} эт., "
          f"точно {np.mean(e == 0)*100:.0f}%, ±1 {np.mean(e <= 1)*100:.0f}%, ≥4 {np.mean(e >= 4)*100:.0f}%")

    out, how = {}, {"knn": 0, "table": 0}
    for i in np.where(np.isnan(LV) & (G != "") & (A >= MIN_AREA))[0]:
        p, src = predict(i)
        if p is not None:
            out[keys[i]] = max(1, int(round(p)))
            how[src] += 1
    pickle.dump(out, open(C.WORK / "bld_levels.pkl", "wb"))
    print(f"[levels] оценено {len(out)} зданий без тега (по соседям {how['knn']}, по таблице {how['table']}) — {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
