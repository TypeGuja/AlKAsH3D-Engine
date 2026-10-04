"""Общее для этапов Mapillary (mly_*.py): пути, токен, проекция, загрузка зданий.

Снимки Mapillary (CC BY-SA 4.0) — уличная съёмка Самары, в основном 2020–2021,
телефон на торпеде. По ним уточняются цвет фасада и высота домов.

Данные Mapillary большие (тайлы покрытия, снимки) и лежат не в samara_map/_cache,
а на отдельном диске: переменная SAMARA_MLY, по умолчанию G:/samara_mapillary.
Токен — файл mapillary_token.txt там же (Client Token вида MLY|…).
"""
import math
import os
import pickle
from pathlib import Path

import numpy as np

import config as C

MLY = Path(os.environ.get("SAMARA_MLY", "G:/samara_mapillary"))
IMG = MLY / "img"                 # снимки thumb_1024: img/<последние 2 цифры id>/<id>.jpg
TZ_HOURS = 4                      # Самара, UTC+4
CAM_H = 1.35                      # высота камеры над дорогой (телефон на торпеде легковушки)


def token():
    return (MLY / "mapillary_token.txt").read_text().strip()


def transformer(to_local=True):
    from pyproj import Transformer
    if to_local:
        return Transformer.from_crs("EPSG:4326", C.PROJ, always_xy=True)
    return Transformer.from_crs(C.PROJ, "EPSG:4326", always_xy=True)


def lonlat_to_xz(lon, lat):
    x, y = transformer().transform(np.asarray(lon), np.asarray(lat))
    return x, -y                                   # Z = юг


def sun_elevation(t_ms, lon, lat):
    """Высота солнца, градусы (приближённая формула, точности ±1° хватает)."""
    t = np.asarray(t_ms, dtype=np.float64) / 1000.0
    d = t / 86400.0 + 2440587.5 - 2451545.0       # дни от J2000
    g = np.radians((357.529 + 0.98560028 * d) % 360)
    q = (280.459 + 0.98564736 * d) % 360
    L = np.radians((q + 1.915 * np.sin(g) + 0.020 * np.sin(2 * g)) % 360)
    e = np.radians(23.439 - 0.00000036 * d)
    ra = np.arctan2(np.cos(e) * np.sin(L), np.cos(L))
    dec = np.arcsin(np.sin(e) * np.sin(L))
    gmst = (18.697374558 + 24.06570982441908 * d) % 24
    ha = np.radians((gmst * 15 + np.asarray(lon)) % 360) - ra
    la = np.radians(lat)
    return np.degrees(np.arcsin(np.sin(la) * np.sin(dec) + np.cos(la) * np.cos(dec) * np.cos(ha)))


def img_path(iid):
    s = str(int(iid))
    return IMG / s[-2:] / f"{s}.jpg"


def load_buildings():
    """Здания (kind == 'building') из бинов extract_osm, как их видит bld_levels:
    список dict(key, geom, tags, levels, levels_tagged, height_tagged).
    key = representative_point, округлённая до 0.1 м — тот же ключ, что в bld_levels.pkl."""
    from shapely import wkb as swkb
    from build_chunks import num
    est = {}
    p = C.WORK / "bld_levels.pkl"
    if p.exists():
        est = pickle.load(open(p, "rb"))
    out, seen = [], set()
    for f in sorted((C.WORK / "bins").glob("st_*.pkl")):
        for kind, t, w in pickle.load(open(f, "rb")):
            if kind != "building":
                continue
            g = swkb.loads(w)
            rp = g.representative_point()
            k = (round(rp.x, 1), round(rp.y, 1))
            if k in seen:
                continue
            seen.add(k)
            lt = num(t.get("building:levels"))
            ht = num(t.get("height"))
            out.append(dict(key=k, geom=g, tags=t, levels_tagged=lt, height_tagged=ht,
                            levels=lt if lt is not None else est.get(k)))
    return out


def facade_edges(g, min_len=4.0):
    """Внешние стены: (p0, p1, наружная нормаль) для отрезков длиннее min_len."""
    import shapely
    out = []
    for poly in shapely.get_parts(g):
        if poly.geom_type != "Polygon":
            continue
        co = np.asarray(shapely.orient_polygons(poly).exterior.coords)[:, :2]
        for p, q in zip(co[:-1], co[1:]):
            d = q - p
            L = math.hypot(*d)
            if L >= min_len:
                out.append((p, q, np.array([d[1], -d[0]]) / L))
    return out


# ---------------------------------------------------------------- объекты Mapillary для сборки чанков

_FEATURES = None


def mly_features():
    """CACHE/work/mly_features.pkl (mly_features.py) или пусто, если этапа не было."""
    global _FEATURES
    if _FEATURES is None:
        f = C.WORK / "mly_features.pkl"
        _FEATURES = pickle.load(open(f, "rb")) if f.exists() else {"points": {}, "cov": None}
    return _FEATURES


def merge_close(P, r):
    """Один объект, распознанный дважды (разные проезды), — точки ближе r сливаются в среднее."""
    P = np.asarray(P, np.float64).reshape(-1, 2)
    if len(P) < 2:
        return P
    from scipy.sparse import coo_matrix
    from scipy.sparse.csgraph import connected_components
    from scipy.spatial import cKDTree
    pairs = cKDTree(P).query_pairs(r, output_type="ndarray")
    if len(pairs) == 0:
        return P
    g = coo_matrix((np.ones(len(pairs)), (pairs[:, 0], pairs[:, 1])), shape=(len(P), len(P)))
    n, lab = connected_components(g, directed=False)
    out = np.zeros((n, 2))
    np.add.at(out, lab, P)
    return out / np.bincount(lab)[:, None]


def mly_points(cls, window, merge_r=3.0):
    """Точки класса в окне (xmin, zmin, xmax, zmax), дубли слиты."""
    P = mly_features()["points"].get(cls)
    if P is None or len(P) == 0:
        return np.zeros((0, 2))
    x0, z0, x1, z1 = window
    m = (P[:, 0] >= x0) & (P[:, 0] < x1) & (P[:, 1] >= z0) & (P[:, 1] < z1)
    return merge_close(P[m], merge_r)


def mly_raw(cls, window):
    """Точки класса в окне без слияния + (кадров, азимут лица°) на точку."""
    F = mly_features()
    P = F["points"].get(cls)
    M = F.get("meta", {}).get(cls)
    if P is None or len(P) == 0:
        return np.zeros((0, 2)), np.zeros((0, 2))
    if M is None:
        M = np.column_stack([np.full(len(P), -1.0), np.full(len(P), np.nan)])
    x0, z0, x1, z1 = window
    m = (P[:, 0] >= x0) & (P[:, 0] < x1) & (P[:, 1] >= z0) & (P[:, 1] < z1)
    return P[m].astype(np.float64), M[m].astype(np.float64)


def covered(x, z):
    """Снималась ли улица в точке (по растру покрытия Mapillary, 10 м). Векторно."""
    cov = mly_features()["cov"]
    x, z = np.asarray(x, np.float64), np.asarray(z, np.float64)
    if cov is None:
        return np.zeros(np.shape(x), bool)
    m = cov["mask"]
    i = ((x - cov["x0"]) / cov["step"]).astype(np.int64)
    j = ((z - cov["z0"]) / cov["step"]).astype(np.int64)
    ok = (i >= 0) & (j >= 0) & (i < m.shape[1]) & (j < m.shape[0])
    out = np.zeros(np.shape(x), bool)
    out[ok] = m[j[ok], i[ok]] > 0
    return out


def cluster_along(P, lines, tree, along, lateral, maxd=12.0, far_r=5.0):
    """Один объект Mapillary распознаётся несколько раз (разные проезды, каждый светильник/
    сторона знака отдельно), и глубина по снимкам с торпеды определяется хуже всего — дубли
    растянуты вдоль улицы. У дороги (ближе maxd к оси) точки одной стороны группируются жадно,
    без цепочек: группа открывается первой точкой и забирает следующие не дальше `along` вдоль
    оси и `lateral` поперёк. Вдали от дорог — слияние в радиусе far_r."""
    import shapely
    P = np.asarray(P, np.float64).reshape(-1, 2)
    if len(P) == 0 or tree is None:
        return merge_close(P, far_r)
    pts = shapely.points(P)
    k = tree.nearest(pts)
    L = np.array(lines, dtype=object)[k]
    dist = shapely.distance(L, pts)
    s = shapely.line_locate_point(L, pts)
    q0 = shapely.get_coordinates(shapely.line_interpolate_point(L, np.maximum(s - 1.0, 0)))
    q1 = shapely.get_coordinates(shapely.line_interpolate_point(L, s + 1.0))
    q = shapely.get_coordinates(shapely.line_interpolate_point(L, s))
    t = q1 - q0
    t /= np.maximum(np.hypot(t[:, 0], t[:, 1]), 1e-6)[:, None]
    lat = (P[:, 0] - q[:, 0]) * -t[:, 1] + (P[:, 1] - q[:, 1]) * t[:, 0]
    road = dist < maxd
    out = []
    idx = np.where(road)[0]
    for kk in np.unique(k[idx]):
        g = idx[k[idx] == kk]
        g = g[np.argsort(s[g])]
        groups = []                      # открытые группы: [s0, lat0, [индексы]]
        for i in g:
            still = []
            for grp in groups:           # закрываем группы, от начала которых ушли дальше along
                if s[i] - grp[0] > along:
                    out.append(P[grp[2]].mean(0))
                else:
                    still.append(grp)
            groups = still
            for grp in groups:
                if abs(lat[i] - grp[1]) < lateral:
                    grp[2].append(i)
                    break
            else:
                groups.append([s[i], lat[i], [i]])
        out += [P[grp[2]].mean(0) for grp in groups]
    return np.vstack([np.array(out).reshape(-1, 2), merge_close(P[~road], far_r).reshape(-1, 2)])
