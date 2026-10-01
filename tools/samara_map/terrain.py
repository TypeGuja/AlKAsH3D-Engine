"""Этап 2: рельеф (Copernicus DEM GLO-30) на локальной сетке 16 м + мосты в 3D + дальний рельеф.

Copernicus — это DSM (поверхность с домами и кронами), поэтому:
  * под зданиями высота восстанавливается из окружающей земли (normalized convolution);
  * внутри лесов вычитается оценка высоты крон (плавно от опушки);
  * полотно дорог не трогают ни маска зданий, ни поправка на кроны (над асфальтом
    нет ни крыш, ни деревьев), а затем рельеф вдоль дорог притягивается к их
    собственному сглаженному продольному профилю (см. conform_roads);
  * водоёмы получают плоский уровень, берег поднимается не ниже уровня воды.

Выход (CACHE/work): heights.npy, water_level.npy, grid.json, bridges3d.pkl
            (OUT) far_terrain.obj
"""
import json
import math
import pickle
import time

import numpy as np
import shapely
import tifffile
from PIL import Image, ImageDraw
from pyproj import Transformer
from scipy import ndimage
from scipy.sparse import lil_matrix
from scipy.sparse.linalg import spsolve
from shapely import wkb as swkb

import config as C
from build_chunks import ROAD_W, road_width

FOREST_CANOPY = 10.0     # м, насколько DSM Copernicus в лесу выше земли
FOREST_RAMP = 60.0       # м от опушки до полной поправки
BLD_OPEN_CELLS = 7       # окно нижней огибающей DSM (7 клеток = 112 м): уже квартала с дворами
BLD_GROUND_DH = 1.5      # м: клетка не выше огибающей на столько — видимая земля

# Дороги: по ним рельеф выравнивается (conform_roads). Только проезжие дороги и
# рельсы — у тропинок и дорожек в парках DSM видит кроны, им верить нельзя.
RAIL_W = {"rail": 4.0, "tram": 3.0, "light_rail": 4.0, "narrow_gauge": 3.0}
ROAD_SAMPLE = 8.0        # м, шаг продольного профиля
ROAD_MEDIAN = 5          # отсчётов (40 м): убирает одиночные выбросы
ROAD_OPEN = 7            # отсчётов (56 м): открытие срезает горбы короче (кроны/крыши у дороги)
ROAD_SIGMA = 2.0         # отсчётов (16 м): итоговое сглаживание профиля
ROAD_SHOULDER = 2.0      # м за краем полотна, где рельеф ещё точно на уровне дороги
ROAD_BLEND = 16.0        # м, дальше — плавный переход к окружающему рельефу
_to_geo = Transformer.from_crs(C.PROJ, "EPSG:4326", always_xy=True)


def load_dem():
    tiles = []
    for name, (lon0, lat0) in sorted(C.DEM_TILES.items(), key=lambda kv: kv[1][0]):
        a = tifffile.imread(str(C.CACHE / name)).astype(np.float32)
        tiles.append((lon0, lat0, a))
    lon0, lat0 = tiles[0][0], tiles[0][1]
    dem = np.hstack([t[2] for t in tiles])
    dlon = 1.0 / tiles[0][2].shape[1]
    dlat = 1.0 / tiles[0][2].shape[0]
    return dem, lon0, lat0, dlon, dlat


def sample_dem(dem, lon0, lat0, dlon, dlat, xs, zs):
    lon, lat = _to_geo.transform(xs, -zs)
    col = (np.asarray(lon) - lon0) / dlon
    row = (lat0 - np.asarray(lat)) / dlat
    return ndimage.map_coordinates(dem, [row, col], order=1, mode="nearest").astype(np.float32)


def rasterize(polys, x0, z0, step, nx, nz, values=None, mode="L"):
    img = Image.new(mode, (nx, nz), 0)
    d = ImageDraw.Draw(img)
    for k, g in enumerate(polys):
        v = 1 if values is None else values[k]
        for p in shapely.get_parts(g):
            if p.geom_type != "Polygon" or p.is_empty:
                continue
            ext = [((x - x0) / step, (z - z0) / step) for x, z in p.exterior.coords]
            if len(ext) >= 3:
                d.polygon(ext, fill=v)
            for r in p.interiors:
                hole = [((x - x0) / step, (z - z0) / step) for x, z in r.coords]
                if len(hole) >= 3:
                    d.polygon(hole, fill=0)
    return np.array(img)


def inpaint(h, mask, sigmas=(1.5, 3, 6, 12, 24, 48)):
    w = (~mask).astype(np.float32)
    out = h.copy()
    remaining = mask.copy()
    hw = h * w
    for s in sigmas:
        num = ndimage.gaussian_filter(hw, s)
        den = ndimage.gaussian_filter(w, s)
        ok = remaining & (den > 0.2)
        out[ok] = num[ok] / den[ok]
        remaining &= ~ok
        if not remaining.any():
            break
    if remaining.any():
        out[remaining] = np.median(h[~mask])
    return out


def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0, 1)
    return t * t * (3 - 2 * t)


def load_roads():
    """Проезжие дороги и рельсы из бинов этапа 1 -> [(LineString, полуширина м)].

    В бинах линии нарезаны по суперплиткам с перекрытием, поэтому внутри
    группы одной ширины они объединяются и склеиваются в цепочки (от
    перекрёстка до перекрёстка). Мостов здесь нет (они в global.pkl, у них
    своя высота — build_bridges), тоннелей тоже (отброшены в extract_osm).
    """
    from collections import defaultdict
    groups = defaultdict(list)
    for f in sorted((C.WORK / "bins").glob("st_*.pkl")):
        for kind, tags, g in pickle.load(open(f, "rb")):
            if kind != "line":
                continue
            hw, rw = tags.get("highway"), tags.get("railway")
            if hw in ROAD_W:
                w = road_width(tags, hw)
            elif rw in RAIL_W:
                w = RAIL_W[rw]
            else:
                continue
            groups[round(w * 2) / 4].append(swkb.loads(g))
    out = []
    for hw, gs in groups.items():
        merged = shapely.line_merge(shapely.union_all(gs))
        out += [(p, hw) for p in shapely.get_parts(merged) if p.length > 0.5]
    return out


def conform_roads(h, h_bld, forest_depth, roads, x0, z0):
    """Притянуть рельеф вдоль дорог к их сглаженному продольному профилю.

    Профиль снимается по оси дороги каждые ROAD_SAMPLE м с рельефа без зданий
    (h_bld), а глубоко в лесу — с рельефа с поправкой на кроны (h). Затем:
    медиана (одиночные выбросы DSM), морфологическое открытие (срезает
    горбы короче ~56 м: кроны и края крыш, попавшие в 30-метровый пиксель
    DSM; настоящий подъём дороги длиннее и не страдает — плавная выпуклость
    радиусом 1 км теряет на 56 м лишь ~0.4 м) и лёгкое гауссово сглаживание.

    Узел сетки на расстоянии d от оси дороги полуширины hw получает вес
    1 при d <= hw + ROAD_SHOULDER и линейно до 0 к hw + ROAD_SHOULDER +
    ROAD_BLEND; высота = смесь рельефа и профиля с этим весом. На перекрёстках
    профили соседних дорог усредняются (ближняя — с большим весом).
    """
    from scipy.spatial import cKDTree

    seg_d, seg_xy, seg_n = [], [], []
    for line, _ in roads:
        n = max(2, int(math.ceil(line.length / ROAD_SAMPLE)) + 1)
        d = np.linspace(0.0, line.length, n)
        seg_d.append(d)
        seg_xy.append(shapely.get_coordinates(shapely.line_interpolate_point(line, d)))
        seg_n.append(n)
    xy = np.concatenate(seg_xy)
    a = grid_height(h_bld, x0, z0, xy[:, 0], xy[:, 1])
    b = grid_height(h, x0, z0, xy[:, 0], xy[:, 1])
    t = smoothstep(20.0, 50.0, grid_height(forest_depth, x0, z0, xy[:, 0], xy[:, 1]))
    y_all = a + (b - a) * t

    X, Z, T, HW = [], [], [], []
    k = 0
    for (line, hw), d, n in zip(roads, seg_d, seg_n):
        y = y_all[k:k + n]
        k += n
        if n >= 3:
            y = ndimage.median_filter(y, size=min(ROAD_MEDIAN, n), mode="nearest")
            y = ndimage.grey_opening(y, size=min(ROAD_OPEN, n), mode="nearest")
            y = ndimage.gaussian_filter1d(y, ROAD_SIGMA, mode="nearest")
        m = max(2, int(math.ceil(line.length / 4.0)) + 1)     # плотнее для поиска ближайших
        dd = np.linspace(0.0, line.length, m)
        q = shapely.get_coordinates(shapely.line_interpolate_point(line, dd))
        X.append(q[:, 0]); Z.append(q[:, 1]); T.append(np.interp(dd, d, y)); HW.append(np.full(m, hw))
    X = np.concatenate(X); Z = np.concatenate(Z); T = np.concatenate(T); HW = np.concatenate(HW)

    reach = float(HW.max()) + ROAD_SHOULDER + ROAD_BLEND
    nz, nx = h.shape
    near = np.zeros((nz, nx), bool)
    ii = np.clip(np.rint((X - x0) / C.GRID).astype(int), 0, nx - 1)
    jj = np.clip(np.rint((Z - z0) / C.GRID).astype(int), 0, nz - 1)
    near[jj, ii] = True
    near = ndimage.binary_dilation(near, iterations=int(math.ceil(reach / C.GRID)) + 1)
    nj, ni = np.nonzero(near)

    tree = cKDTree(np.column_stack([X, Z]))
    out = h.copy()
    K = 16
    for s in range(0, len(nj), 400_000):
        bj, bi = nj[s:s + 400_000], ni[s:s + 400_000]
        pts = np.column_stack([x0 + bi * C.GRID, z0 + bj * C.GRID])
        dist, idx = tree.query(pts, k=K, distance_upper_bound=reach)
        ok = np.isfinite(dist)
        idx = np.where(ok, idx, 0)
        f = np.clip(1.0 - (dist - (HW[idx] + ROAD_SHOULDER)) / ROAD_BLEND, 0.0, 1.0) * ok
        w = f / (1.0 + np.where(ok, dist, 0.0))
        wsum = w.sum(axis=1)
        has = wsum > 0
        target = np.where(has, (w * T[idx]).sum(axis=1) / np.where(has, wsum, 1.0), 0.0)
        W = f.max(axis=1)
        cur = out[bj, bi]
        out[bj, bi] = np.where(has, cur + W * (target - cur), cur)
    return out.astype(np.float32)


def main():
    t0 = time.time()
    region = pickle.load(open(C.WORK / "region.pkl", "rb"))
    glob = pickle.load(open(C.WORK / "global.pkl", "rb"))
    chunks = region["chunks"]
    gxs = [c[0] for c in chunks]
    gzs = [c[1] for c in chunks]
    x0 = (min(gxs) - 2) * C.CHUNK
    z0 = (min(gzs) - 2) * C.CHUNK
    x1 = (max(gxs) + 3) * C.CHUNK
    z1 = (max(gzs) + 3) * C.CHUNK
    nx = int(round((x1 - x0) / C.GRID)) + 1
    nz = int(round((z1 - z0) / C.GRID)) + 1
    print(f"[terrain] сетка {nx}x{nz} ({(x1-x0)/1000:.1f} x {(z1-z0)/1000:.1f} км), шаг {C.GRID} м")

    dem, lon0, lat0, dlon, dlat = load_dem()
    h = np.empty((nz, nx), np.float32)
    xs = x0 + np.arange(nx) * C.GRID
    for j in range(nz):
        h[j] = sample_dem(dem, lon0, lat0, dlon, dlat, xs, np.full(nx, z0 + j * C.GRID))
    print(f"[terrain] DEM выбран: {h.min():.1f}..{h.max():.1f} м — {time.time()-t0:.0f}s")

    # --- дороги: узлы сетки на самом полотне — DSM там честно видит землю
    roads = load_roads()
    road_polys = [g.buffer(hw, quad_segs=2) for g, hw in roads]
    on_road = rasterize(road_polys, x0 - C.GRID / 2, z0 - C.GRID / 2, C.GRID, nx, nz) > 0
    print(f"[terrain] дорог {len(roads)} цепочек ({sum(g.length for g, _ in roads)/1000:.0f} км), "
          f"полотно {on_road.mean()*100:.1f}% сетки — {time.time()-t0:.0f}s")

    # --- здания: восстановить землю под ними
    # Расширение маски на клетку ловит размытые края крыш в DSM (пиксель 30 м),
    # но НЕ на полотне дороги: раньше узлы проезжей части у домов заменялись
    # интерполяцией из дворов, и дорога проседала/вспухала на 1–4 м.
    #
    # Клетки кольца, где DSM лежит на нижней огибающей (открытие окном
    # BLD_OPEN_CELLS), — видимая земля (улица, двор, площадь): их тоже не
    # заливаем, они — опорные точки для восстановления. Без этого в плотной
    # застройке кольца сливались в одно пятно на десятки км², улицы внутри
    # исчезали, и заливка тянула высоту с краёв пятна — центр города на
    # 60–70 м проседал почти до уровня Волги.
    blds = [swkb.loads(b) for b in glob["buildings"]]
    bcore = rasterize(blds, x0 - C.GRID / 2, z0 - C.GRID / 2, C.GRID, nx, nz) > 0
    envelope = ndimage.grey_opening(h, size=(BLD_OPEN_CELLS, BLD_OPEN_CELLS))
    ground_seen = (h - envelope) < BLD_GROUND_DH
    ring = ndimage.binary_dilation(bcore, iterations=1) & ~bcore
    bmask = bcore | (ring & ~on_road & ~ground_seen)
    h = inpaint(h, bmask)
    h_bld = h.copy()
    print(f"[terrain] под зданиями восстановлено {bmask.mean()*100:.1f}% сетки — {time.time()-t0:.0f}s")

    # --- леса: снять кроны
    # Над полотном дороги крон нет — поправка там 0 и нарастает от края дороги,
    # как от опушки. Раньше полигон леса/парка, подходящий к дороге, опускал
    # соседний узел на 10 м, и асфальт между узлами проваливался на 8–13 м.
    forests = [swkb.loads(b) for b in glob["forest"]]
    fmask = rasterize(forests, x0 - C.GRID / 2, z0 - C.GRID / 2, C.GRID, nx, nz) > 0
    fdist = ndimage.distance_transform_edt(fmask & ~on_road) * C.GRID
    h -= (FOREST_CANOPY * smoothstep(0, FOREST_RAMP, fdist)).astype(np.float32)
    h = ndimage.gaussian_filter(h, 0.8).astype(np.float32)
    print(f"[terrain] лес {fmask.mean()*100:.1f}% сетки — {time.time()-t0:.0f}s")

    # --- дороги: плавный продольный профиль
    # глубоко в лесу (оба края — лес) дорогу берём с поправкой на кроны, иначе — без неё
    forest_depth = ndimage.distance_transform_edt(fmask) * C.GRID
    h = conform_roads(h, h_bld, forest_depth, roads, x0, z0)
    print(f"[terrain] дороги выровнены — {time.time()-t0:.0f}s")

    # --- вода: плоские уровни
    wpolys = [swkb.loads(w) for w in glob["water"] if not isinstance(w, tuple)]
    labels = rasterize(wpolys, x0 - C.GRID / 2, z0 - C.GRID / 2, C.GRID, nx, nz,
                       values=list(range(1, len(wpolys) + 1)), mode="I").astype(np.int32)
    level = np.full((nz, nx), np.nan, np.float32)
    if labels.max() > 0:
        idx = np.arange(1, labels.max() + 1)
        lv = ndimage.labeled_comprehension(h, labels, idx, lambda v: np.percentile(v, 10), np.float32, np.nan)
        lut = np.concatenate([[np.nan], lv]).astype(np.float32)
        level = lut[labels]
    water = ~np.isnan(level)
    # мелкие водоёмы, не попавшие в сетку, уровень возьмут из рельефа в этапе 3
    if water.any():
        dist, (ij, ii) = ndimage.distance_transform_edt(~water, return_indices=True)
        near_level = level[ij, ii]
        shore = (~water) & (dist <= 3)
        h[shore] = np.maximum(h[shore], near_level[shore] + 0.25)
        # дно под водой — но не под полотном дороги (дамбы, насыпи, неточные
        # полигоны берега): иначе дорога проваливается к уровню воды
        sink = water & ~on_road
        h[sink] = level[sink] - 1.0
    print(f"[terrain] вода {water.mean()*100:.1f}% сетки, водоёмов {len(wpolys)} — {time.time()-t0:.0f}s")

    np.save(C.WORK / "heights.npy", h)
    np.save(C.WORK / "water_level.npy", level)
    json.dump({"x0": x0, "z0": z0, "nx": nx, "nz": nz, "step": C.GRID}, open(C.WORK / "grid.json", "w"))

    build_bridges(glob, h, x0, z0)
    print(f"[terrain] мосты — {time.time()-t0:.0f}s")
    far_terrain(dem, lon0, lat0, dlon, dlat, glob, chunks, x0, z0, h)
    print(f"[terrain] готово — {time.time()-t0:.0f}s")


def grid_height(h, x0, z0, x, z):
    """Та же планарная интерполяция по треугольникам сетки, что и в сборке чанков."""
    fx = (np.asarray(x) - x0) / C.GRID
    fz = (np.asarray(z) - z0) / C.GRID
    i = np.clip(np.floor(fx).astype(int), 0, h.shape[1] - 2)
    j = np.clip(np.floor(fz).astype(int), 0, h.shape[0] - 2)
    u = fx - i
    v = fz - j
    h00 = h[j, i]; h10 = h[j, i + 1]; h01 = h[j + 1, i]; h11 = h[j + 1, i + 1]
    lower = (u + v) <= 1
    return np.where(lower, h00 + (h10 - h00) * u + (h01 - h00) * v,
                    h11 + (h01 - h11) * (1 - u) + (h10 - h11) * (1 - v))


def build_bridges(glob, h, x0, z0):
    """Высоты мостов: концы цепочек мостовых путей стоят на земле, середина —
    гармоническая (по длине — линейная) интерполяция, не ниже земли+просвет."""
    nonbridge = glob["nonbridge_nodes"]
    ways = []
    for tags, w, ids in glob["bridges"]:
        g = swkb.loads(w)
        co = np.asarray(g.coords)
        if len(ids) != len(co):
            ids = [ids[0]] + [("s", id(g), k) for k in range(1, len(co) - 1)] + [ids[-1]]
        ways.append((tags, co, ids))
    index = {}
    for _, co, ids in ways:
        for nid, c in zip(ids, co):
            index.setdefault(nid, c)
    keys = list(index)
    kid = {k: n for n, k in enumerate(keys)}
    pos = np.array([index[k] for k in keys])
    n = len(keys)
    deg = np.zeros(n, int)
    A = lil_matrix((n, n))
    for _, co, ids in ways:
        for a, b, ca, cb in zip(ids[:-1], ids[1:], co[:-1], co[1:]):
            ia, ib = kid[a], kid[b]
            wgt = 1.0 / max(np.hypot(*(ca - cb)), 0.5)
            A[ia, ib] -= wgt; A[ib, ia] -= wgt
            A[ia, ia] += wgt; A[ib, ib] += wgt
            deg[ia] += 1; deg[ib] += 1
    ground = grid_height(h, x0, z0, pos[:, 0], pos[:, 1])
    fixed = np.array([(k in nonbridge) or deg[kid[k]] <= 1 for k in keys])
    A = A.tocsr()
    rhs = np.zeros(n)
    M = A.tolil()
    for i in np.where(fixed)[0]:
        M.rows[i] = [i]; M.data[i] = [1.0]
        rhs[i] = ground[i]
    try:
        hb = spsolve(M.tocsc(), rhs)
        if not np.all(np.isfinite(hb)):
            raise ValueError
    except Exception:
        hb = ground + 6.0
    # расстояние вдоль графа до ближайшего фиксированного конца (для рампы просвета)
    from scipy.sparse.csgraph import dijkstra
    E = lil_matrix((n, n))
    for _, co, ids in ways:
        for a, b, ca, cb in zip(ids[:-1], ids[1:], co[:-1], co[1:]):
            d = max(np.hypot(*(ca - cb)), 0.01)
            E[kid[a], kid[b]] = d; E[kid[b], kid[a]] = d
    fixed_idx = np.where(fixed)[0]
    if len(fixed_idx):
        dist = dijkstra(E.tocsr(), directed=False, indices=fixed_idx, min_only=True)
    else:
        dist = np.full(n, 1e9)
    out = []
    for tags, co, ids in ways:
        try:
            layer = max(1, int(tags.get("layer", "1")))
        except ValueError:
            layer = 1
        clearance = 5.5 * layer
        ii = np.array([kid[k] for k in ids])
        pts = []
        for a in range(len(co) - 1):
            p, q = co[a], co[a + 1]
            L = np.hypot(*(q - p))
            m = max(1, int(math.ceil(L / 8.0)))
            for s in range(m):
                t = s / m
                pts.append((p + (q - p) * t, hb[ii[a]] * (1 - t) + hb[ii[a + 1]] * t,
                            dist[ii[a]] * (1 - t) + dist[ii[a + 1]] * t))
        pts.append((co[-1], hb[ii[-1]], dist[ii[-1]]))
        xz = np.array([p[0] for p in pts])
        hy = np.array([p[1] for p in pts])
        dd = np.array([p[2] for p in pts])
        g = grid_height(h, x0, z0, xz[:, 0], xz[:, 1])
        ramp = np.clip(dd / 60.0, 0, 1)
        hy = np.maximum(hy, g + clearance * ramp)
        out.append((tags, np.column_stack([xz[:, 0], hy, xz[:, 1]]).astype(np.float64)))
    pickle.dump(out, open(C.WORK / "bridges3d.pkl", "wb"))


def far_terrain(dem, lon0, lat0, dlon, dlat, glob, chunks, x0n, z0n, hnear):
    step = C.FAR_GRID
    gx = [c[0] for c in chunks]
    gz = [c[1] for c in chunks]
    pad = 25000.0
    fx0 = math.floor((min(gx) * C.CHUNK - pad) / step) * step
    fz0 = math.floor((min(gz) * C.CHUNK - pad) / step) * step
    fx1 = (max(gx) + 1) * C.CHUNK + pad
    fz1 = (max(gz) + 1) * C.CHUNK + pad
    nx = int((fx1 - fx0) / step) + 1
    nz = int((fz1 - fz0) / step) + 1
    xs = fx0 + np.arange(nx) * step
    H = np.empty((nz, nx), np.float32)
    for j in range(nz):
        H[j] = sample_dem(dem, lon0, lat0, dlon, dlat, xs, np.full(nx, fz0 + j * step))
    H = ndimage.gaussian_filter(H, 0.7)
    # покрытие ближними чанками -> утопить дальний рельеф под ними
    cov = np.zeros((nz, nx), bool)
    for (cx, cz) in chunks:
        i0 = int(math.floor((cx * C.CHUNK - fx0) / step)); i1 = int(math.ceil(((cx + 1) * C.CHUNK - fx0) / step))
        j0 = int(math.floor((cz * C.CHUNK - fz0) / step)); j1 = int(math.ceil(((cz + 1) * C.CHUNK - fz0) / step))
        cov[max(j0, 0):j1 + 1, max(i0, 0):i1 + 1] = True
    inside = ndimage.distance_transform_edt(cov) * step
    H -= (C.FAR_SINK * smoothstep(0, 300, inside)).astype(np.float32)
    # вода дальнего плана
    wat = [swkb.loads(w) for w in glob["far_water"]]
    wm = rasterize(wat, fx0 - step / 2, fz0 - step / 2, step, nx, nz) > 0
    if wm.any():
        lv = np.percentile(H[wm & ~cov], 10) if (wm & ~cov).any() else np.percentile(H[wm], 10)
        H[wm & ~cov] = min(lv, 28.5)

    X, Z = np.meshgrid(xs, fz0 + np.arange(nz) * step)
    gzd, gxd = np.gradient(H, step)
    N = np.dstack([-gxd, np.ones_like(H), -gzd])
    N /= np.linalg.norm(N, axis=2, keepdims=True)
    vid = np.arange(nx * nz).reshape(nz, nx) + 1
    a = vid[:-1, :-1].ravel(); b = vid[:-1, 1:].ravel(); c = vid[1:, :-1].ravel(); d = vid[1:, 1:].ravel()
    cellwater = (wm[:-1, :-1] & wm[:-1, 1:] & wm[1:, :-1] & wm[1:, 1:]).ravel() & ~cov[:-1, :-1].ravel()
    uv_scale = 64.0
    with open(C.OUT / "far_terrain.obj", "w", newline="\n") as f:
        f.write("# Дальний рельеф вокруг Самары (Copernicus DEM GLO-30, шаг %.0f м)\n" % step)
        f.write("# Под ближними чанками утоплен на %.1f м. Координаты как у chunks/*.obj.\n" % C.FAR_SINK)
        f.write("mtllib samara.mtl\n")
        np.savetxt(f, np.column_stack([X.ravel(), H.ravel(), Z.ravel()]), fmt="v %.2f %.2f %.2f")
        np.savetxt(f, np.column_stack([X.ravel() / uv_scale, -Z.ravel() / uv_scale]), fmt="vt %.3f %.3f")
        np.savetxt(f, N.reshape(-1, 3), fmt="vn %.3f %.3f %.3f")
        for name, sel in (("far_ground", ~cellwater), ("water", cellwater)):
            f.write(f"o far_{name}\nusemtl {name}\n")
            # CCW при взгляде сверху (+Y) в правой системе с Z на юг: a, c, b / b, c, d
            tris = np.concatenate([np.column_stack([a[sel], c[sel], b[sel]]), np.column_stack([b[sel], c[sel], d[sel]])])
            t = np.repeat(tris, 3, axis=1)
            np.savetxt(f, t, fmt="f %d/%d/%d %d/%d/%d %d/%d/%d")


def main_far_only():
    """Пересобрать только far_terrain.obj (не трогая heights.npy)."""
    region = pickle.load(open(C.WORK / "region.pkl", "rb"))
    glob = pickle.load(open(C.WORK / "global.pkl", "rb"))
    dem, lon0, lat0, dlon, dlat = load_dem()
    far_terrain(dem, lon0, lat0, dlon, dlat, glob, region["chunks"], None, None, None)


if __name__ == "__main__":
    import sys
    if len(sys.argv) > 1 and sys.argv[1] == "far":
        main_far_only()
    else:
        main()
