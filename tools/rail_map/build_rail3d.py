"""3D-сборка железной дороги из выгрузки extract_rail.py — в формат «папки города»
(тот же, что у tools/samara_map, его читает эдитор: File > Import City Folder):

    data/rail_map/<name>_3d/
        manifest.json, rail.mtl, textures/, chunks/chunk_<gx>_<gz>.obj

Система координат — как у карты Самары (начало = пл. Куйбышева, X восток, Y = высота
над уровнем моря, Z юг), чтобы обе карты совпадали.

Что строится:
  * рельеф ±1.5 км вокруг маршрута: 16 м сетка, у путей — 4 м сетка с насыпями/выемками
    (откосы 1:1.5 от бровки до земли), вода из OSM — плоским уровнем;
  * отметка земляного полотна F — продольно сглаженный рельеф вдоль путей (под мостами
    и водой не учитывается, поэтому мост идёт ровно между берегами);
  * каждый путь: балластная призма, ж/б шпалы (1840 шт/км), рельсы Р65 (колея 1520);
  * мосты: плита с балластом, ограждения, опоры до земли/дна;
  * контактная сеть на электрифицированных путях: опоры через 60 м с консолями,
    несущий трос и контактный провод (опору ставим с той стороны, где нет соседнего пути);
  * платформы (высокие, 1100 мм над головкой рельса), обрезанные по габариту от путей.

Запуск:
  python -I build_rail3d.py                      # вся линия
  python -I build_rail3d.py --km 110 123         # только участок маршрута (проба)
"""
import argparse
import json
import math
import multiprocessing as mp
import pickle
import shutil
import sys
import time
from collections import defaultdict
from pathlib import Path

import numpy as np
import osmium
import shapely
import shapely.ops
from PIL import Image, ImageDraw
from pyproj import Transformer
from scipy import ndimage
from shapely import wkb as swkb
from shapely.geometry import LineString, Point, Polygon, MultiPolygon
from shapely.ops import triangulate

sys.path.insert(0, str(Path(__file__).resolve().parent))   # python -I не кладёт папку скрипта в путь
import extract_rail as ER  # noqa: E402

ROOT = ER.ROOT
SAMARA = ROOT / "data" / "samara_map"
ORIGIN_LAT, ORIGIN_LON = 53.1955, 50.1010             # = tools/samara_map/config.py
PROJ = f"+proj=tmerc +lat_0={ORIGIN_LAT} +lon_0={ORIGIN_LON} +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs"

CHUNK = 256.0
SUPER = 8                       # суперплитка 8x8 чанков = единица работы
GRID = 16.0                     # шаг грубого рельефа
FINE = 4.0                      # шаг рельефа у путей
FINE_NEAR = 48.0                # чанк считается «у путей», если путь ближе этого
TERRAIN_HALF = 1500.0           # ширина рельефа от оси маршрута

# земляное полотно и верхнее строение пути (м)
F_SIGMA = 250.0                 # продольное сглаживание отметки полотна
BALLAST_H = 0.50                # балластная призма над полотном
BALLAST_TOP_HW = 1.70           # полуширина плеча призмы
BALLAST_TOE_HW = 2.45           # полуширина низа призмы (откос 1:1.5)
SLOPE = 1.5                     # откос насыпи/выемки 1:1.5
SLEEPER_STEP = 1.0 / 1.840      # эпюра 1840 шт/км
SLEEPER_HL, SLEEPER_HW = 1.35, 0.115
SLEEPER_UP = 0.05               # шпала выступает над балластом
GAUGE_C = (1.520 + 0.072) / 2   # ось рельса от оси пути
RAIL_H = 0.180
# половина профиля Р65 (x от оси рельса, y от подошвы) — от края подошвы к оси головки
RAIL_HALF = [(0.075, 0.012), (0.009, 0.032), (0.009, 0.140), (0.036, 0.150), (0.036, 0.180), (0.0, 0.181)]
WIRE_CONTACT = 6.25             # контактный провод над головкой рельса
WIRE_MESSENGER = 7.70
MAST_STEP = 60.0
MAST_OFF = 3.3                  # опора от оси пути
MAST_CLEAR = 2.9                # до оси соседнего пути
MAST_H = 9.6
PLATFORM_H = 1.10               # над головкой рельса
PLATFORM_GAP = 1.92             # край платформы от оси пути
PIER_STEP = 40.0
TRUSS_PIER_STEP = 110.0         # пролёт фермы у длинных мостов
TRUSS_WET = 300.0               # мост над водой длиннее этого — сквозные фермы, судоходный габарит
TRUSS_H = 8.6
TRUSS_PANEL = 11.0
NAV_CLEARANCE = 16.0            # низ пролётного строения над водой (судоходный габарит)
SMALL_CLEARANCE = 4.0
DECK_DEPTH = 1.6                # от верха балласта до низа плиты
RAMP_GRADE = 0.008              # уклон подходов к мосту
WATER_TAGS = {("natural", "water"), ("waterway", "riverbank"), ("landuse", "reservoir"), ("water", None)}
WORKERS = 2                     # пользователь просит не грузить машину тяжёлыми параллельными задачами

MATS = ["grass", "gravel", "ballast", "rail_steel", "concrete", "steel_grey", "cable", "water", "asphalt"]

_G = {}                          # данные воркера


# ------------------------------------------------------------------ подготовка

class WaterHandler(osmium.SimpleHandler):
    def __init__(self, bbox):
        super().__init__()
        self.bbox = bbox
        self.wkb = osmium.geom.WKBFactory()
        self.polys = []

    def area(self, a):
        t = a.tags
        if not (t.get("natural") == "water" or t.get("waterway") == "riverbank"
                or t.get("landuse") == "reservoir" or "water" in t):
            return
        try:
            g = swkb.loads(self.wkb.create_multipolygon(a), hex=True)
        except Exception:
            return
        x0, y0, x1, y1 = g.bounds
        b = self.bbox
        if x1 < b[0] or x0 > b[2] or y1 < b[1] or y0 > b[3]:
            return
        self.polys.append(g)


def chaikin(pts, it=2):
    """Сглаживание ломаной OSM (концы на месте — там стрелки и стыки)."""
    p = np.asarray(pts, np.float64)
    for _ in range(it):
        if len(p) < 3:
            break
        q = 0.75 * p[:-1] + 0.25 * p[1:]
        r = 0.25 * p[:-1] + 0.75 * p[1:]
        mid = np.empty((2 * (len(p) - 1), 2))
        mid[0::2], mid[1::2] = q, r
        p = np.vstack([p[:1], mid[1:-1], p[-1:]])
    return p


def densify(p, step):
    seg = np.linalg.norm(np.diff(p, axis=0), axis=1)
    s = np.concatenate([[0.0], np.cumsum(seg)])
    n = max(int(math.ceil(s[-1] / step)), 1)
    t = np.linspace(0.0, s[-1], n + 1)
    return np.stack([np.interp(t, s, p[:, 0]), np.interp(t, s, p[:, 1])], 1), t


def prep(args, out):
    t0 = time.time()
    src = ROOT / "data" / "rail_map" / args.name
    corr = json.loads((src / "corridor.geojson").read_text(encoding="utf-8"))
    route = json.loads((src / "route.geojson").read_text(encoding="utf-8"))
    tr = Transformer.from_crs("EPSG:4326", PROJ, always_xy=True)

    def loc(coords):
        xs, ys = tr.transform([c[0] for c in coords], [c[1] for c in coords])
        return np.stack([xs, -np.asarray(ys)], 1)

    rline = LineString(loc(route["features"][0]["geometry"]["coordinates"]))
    if args.km:
        a, b = args.km[0] * 1000, args.km[1] * 1000
        rline = shapely.ops.substring(rline, a, b)
    area = rline.buffer(TERRAIN_HALF)
    shapely.prepare(area)

    tracks, platforms, nodes = [], [], []
    for f in corr["features"]:
        p = f["properties"]
        g = f["geometry"]
        if g["type"] == "Point":
            nodes.append((p, loc([g["coordinates"]])[0]))
            continue
        xy = loc(g["coordinates"])
        if not area.intersects(LineString(xy)):
            continue
        rw = p.get("railway")
        if rw in ("rail", "narrow_gauge", "light_rail", "disused", "preserved"):
            if rw == "disused" and p.get("disused:railway") is None and p.get("service") == "spur":
                continue
            c = densify(chaikin(xy), 2.0)[0]
            tracks.append({"id": p["osm"], "xy": c, "tags": p,
                           "bridge": p.get("bridge", "no") != "no",
                           "elec": rw == "rail" and p.get("electrified") in ("contact_line", "yes")})
        elif rw == "platform" or p.get("public_transport") == "platform":
            platforms.append((p, xy))
    print(f"путей {len(tracks)}, платформ {len(platforms)}")

    # чанки рельефа
    x0, z0, x1, z1 = area.bounds
    gx0, gz0 = int(math.floor(x0 / CHUNK)), int(math.floor(z0 / CHUNK))
    gx1, gz1 = int(math.floor(x1 / CHUNK)), int(math.floor(z1 / CHUNK))
    chunks = set()
    for gx in range(gx0, gx1 + 1):
        for gz in range(gz0, gz1 + 1):
            if area.intersects(shapely.box(gx * CHUNK, gz * CHUNK, (gx + 1) * CHUNK, (gz + 1) * CHUNK)):
                chunks.add((gx, gz))
    print(f"чанков {len(chunks)}")

    # решётка 16 м на весь bbox (+ запас под сглаживание)
    pad = 3 * F_SIGMA
    LX0 = math.floor((x0 - pad) / GRID) * GRID
    LZ0 = math.floor((z0 - pad) / GRID) * GRID
    nx = int((x1 + pad - LX0) / GRID) + 2
    nz = int((z1 + pad - LZ0) / GRID) + 2
    xs = LX0 + np.arange(nx) * GRID
    zs = LZ0 + np.arange(nz) * GRID
    inv = Transformer.from_crs(PROJ, "EPSG:4326", always_xy=True)
    lon_c, lat_c = inv.transform([x0 - pad, x1 + pad, x0 - pad, x1 + pad], [-(z0 - pad), -(z0 - pad), -(z1 + pad), -(z1 + pad)])
    dem, dlon0, dlat0, dlon, dlat = ER.find_dem_tiles(min(lon_c), min(lat_c), max(lon_c), max(lat_c))
    X, Z = np.meshgrid(xs, zs)
    lon, lat = inv.transform(X.ravel(), -Z.ravel())
    col = (np.asarray(lon) - dlon0) / dlon
    row = (dlat0 - np.asarray(lat)) / dlat
    G = ndimage.map_coordinates(dem, [row, col], order=1, mode="nearest").reshape(nz, nx).astype(np.float32)
    del X, Z, lon, lat, col, row
    # DSM -> земля: открытие срезает кроны и постройки (уже ~80 м)
    G = ndimage.gaussian_filter(ndimage.grey_opening(G, size=5), 1.0).astype(np.float32)
    print(f"рельеф {nx}x{nz} за {time.time() - t0:.0f} с")

    def to_px(xy):
        return [((x - LX0) / GRID, (z - LZ0) / GRID) for x, z in xy]

    # вода
    lon_b = (min(lon_c), min(lat_c), max(lon_c), max(lat_c))
    wh = WaterHandler(lon_b)
    wh.apply_file(str(ER.PBF), locations=True)
    water_img = Image.new("L", (nx, nz), 0)
    dw = ImageDraw.Draw(water_img)
    water_polys = []
    for g in wh.polys:
        for poly in (g.geoms if isinstance(g, MultiPolygon) else [g]):
            ext = loc(poly.exterior.coords)
            holes = [loc(r.coords) for r in poly.interiors]
            lp = Polygon(ext, holes)
            if not lp.is_valid:
                lp = lp.buffer(0)
            if lp.is_empty or not lp.intersects(area):
                continue
            water_polys.append(lp)
            dw.polygon(to_px(ext), fill=255)
            for hr in holes:
                dw.polygon(to_px(hr), fill=0)
    W = np.asarray(water_img) > 0
    lab, nlab = ndimage.label(W)
    if nlab:
        lvl = ndimage.median(G, lab, index=np.arange(1, nlab + 1))
        T = G.copy()
        wl = np.asarray(lvl, np.float32)[lab[W] - 1]
        T[W] = np.minimum(T[W], wl) - 0.0
        WL = np.full_like(G, np.nan)
        WL[W] = wl
    else:
        T, WL = G.copy(), np.full_like(G, np.nan)
    print(f"водоёмов {len(water_polys)}")

    # отметка полотна: нормированная свёртка рельефа вдоль путей (без мостов и воды)
    trk = Image.new("L", (nx, nz), 0)
    brg = Image.new("L", (nx, nz), 0)
    dt, db = ImageDraw.Draw(trk), ImageDraw.Draw(brg)
    for t in tracks:
        (db if t["bridge"] else dt).line(to_px(t["xy"]), fill=255, width=1 if not t["bridge"] else 7)
    w = ((np.asarray(trk) > 0) & ~(np.asarray(brg) > 0) & ~W).astype(np.float32)
    s = F_SIGMA / GRID
    num = ndimage.gaussian_filter(G * w, s)
    den = ndimage.gaussian_filter(w, s)
    F = np.where(den > 1e-4, num / np.maximum(den, 1e-6), G).astype(np.float32)

    # мосты над водой: полотно не ниже воды + габарит (судоходный у длинных), подходы
    # к ним поднимаются с уклоном RAMP_GRADE — насыпи потом строит conform рельефа
    for t in tracks:
        if not t["bridge"]:
            continue
        p, _ = densify(t["xy"], 4.0)
        px = (p[:, 0] - LX0) / GRID
        pz = (p[:, 1] - LZ0) / GRID
        wet = ndimage.map_coordinates(W.astype(np.float32), [pz, px], order=0) > 0.5
        wet_len = wet.sum() * 4.0
        t["truss"] = wet_len > TRUSS_WET
        if wet_len == 0:
            continue
        wl = np.nanmin(ndimage.map_coordinates(np.nan_to_num(WL, nan=1e4), [pz, px], order=0)[wet])
        clr = NAV_CLEARANCE if wet_len > TRUSS_WET else SMALL_CLEARANCE
        hb = wl + clr + DECK_DEPTH - BALLAST_H
        rng = (hb - wl) / RAMP_GRADE + 200.0
        i0 = max(int(pz.min() - rng / GRID), 0)
        i1 = min(int(pz.max() + rng / GRID) + 1, nz)
        j0 = max(int(px.min() - rng / GRID), 0)
        j1 = min(int(px.max() + rng / GRID) + 1, nx)
        m = Image.new("L", (j1 - j0, i1 - i0), 0)
        ImageDraw.Draw(m).line([(x - j0, z - i0) for x, z in zip(px, pz)], fill=255, width=1)
        dist = ndimage.distance_transform_edt(np.asarray(m) == 0) * GRID
        ramp = hb - RAMP_GRADE * dist
        sub = F[i0:i1, j0:j1]
        np.maximum(sub, ramp.astype(np.float32), out=sub)
        print(f"  мост {t['id']}: над водой {wet_len:.0f} м, вода {wl:.1f} м, полотно {hb:.1f} м")

    # опоры контактной сети
    tree = shapely.STRtree([LineString(t["xy"]) for t in tracks])
    masts, wires = [], []
    for ti, t in enumerate(tracks):
        if not t["elec"]:
            continue
        p, s_arc = densify(t["xy"], 1.0)
        L = s_arc[-1]
        n = max(int(round(L / MAST_STEP)), 1)
        stations = []
        for k in range(n + 1):
            d = L * k / n
            i = min(int(np.searchsorted(s_arc, d)), len(p) - 1)
            a = p[max(i - 1, 0)]
            b = p[min(i + 1, len(p) - 1)]
            tg = (b - a) / max(np.linalg.norm(b - a), 1e-6)
            r = np.array([-tg[1], tg[0]])
            best = None
            if t.get("truss"):
                # на ферменном мосту провода держат поперечные связи фермы, опор нет
                stations.append((p[i], tg, r, 0.0))
                continue
            for side in (1.0, -1.0):
                m = p[i] + side * MAST_OFF * r
                near = tree.query(Point(m).buffer(MAST_CLEAR + 0.1))
                clear = min((LineString(tracks[j]["xy"]).distance(Point(m)) for j in near if j != ti), default=99.0)
                if clear >= MAST_CLEAR and (best is None or clear > best[1]):
                    best = (side, clear)
            stations.append((p[i], tg, r, best[0] if best else None))
        for (pa, _, ra, sa), (pb, _, rb, sb) in zip(stations[:-1], stations[1:]):
            if sa is not None and sb is not None:
                wires.append((pa, pb))
        for pc, tg, r, side in stations:
            if side:
                masts.append((pc, r * side, t["bridge"]))
    print(f"опор КС {len(masts)}, пролётов {len(wires)}")

    # платформы: полигон (или полоса 3 м у линии), обрезанный по габариту от путей
    plats = []
    for p, xy in platforms:
        g = Polygon(xy) if len(xy) >= 4 and np.allclose(xy[0], xy[-1]) else LineString(xy).buffer(1.5, cap_style="flat")
        if not g.is_valid:
            g = g.buffer(0)
        near = tree.query(g.buffer(PLATFORM_GAP))
        if len(near):
            g = g.difference(shapely.union_all([LineString(tracks[j]["xy"]).buffer(PLATFORM_GAP, cap_style="flat")
                                               for j in near]))
        for poly in (g.geoms if hasattr(g, "geoms") else [g]):
            if isinstance(poly, Polygon) and poly.area > 6.0:
                plats.append(poly)
    print(f"платформ (кусков) {len(plats)} за {time.time() - t0:.0f} с")

    np.savez(out / "_lattice.npz", T=T, F=F, WL=WL, X0=LX0, Z0=LZ0)
    with open(out / "_prep.pkl", "wb") as fh:
        pickle.dump({"tracks": tracks, "masts": masts, "wires": wires, "plats": plats,
                     "water": water_polys, "chunks": sorted(chunks)}, fh)
    return sorted(chunks)


# ------------------------------------------------------------------ геометрия

class Mesh:
    def __init__(self):
        self.g = defaultdict(list)          # mat -> [(P, N, UV)] , P (n,3,3)

    def add(self, mat, P, N, UV):
        if len(P):
            self.g[mat].append((np.asarray(P, np.float32), np.asarray(N, np.float32), np.asarray(UV, np.float32)))

    def tris(self):
        return sum(len(e[0]) for v in self.g.values() for e in v)


def lattice_sample(A, x, z, X0, Z0):
    return ndimage.map_coordinates(A, [(np.asarray(z) - Z0) / GRID, (np.asarray(x) - X0) / GRID],
                                   order=1, mode="nearest")


def quads(P00, P01, P10, P11, N0, N1, uv):
    """Лента квадов: P?? (n,3); 0/1 первый индекс — вдоль, второй — поперёк."""
    A = np.stack([P00, P10, P11], 1)
    B = np.stack([P00, P11, P01], 1)
    NA = np.stack([N0, N1, N1], 1)
    NB = np.stack([N0, N1, N0], 1)
    (u0, u1, v0, v1) = uv
    UA = np.stack([np.stack([u0, v0], -1), np.stack([u0, v1], -1), np.stack([u1, v1], -1)], 1)
    UB = np.stack([np.stack([u0, v0], -1), np.stack([u1, v1], -1), np.stack([u1, v0], -1)], 1)
    return np.concatenate([A, B]), np.concatenate([NA, NB]), np.concatenate([UA, UB])


def sweep(p, y, r, profile, mat, vscale, keep):
    """Протяжка поперечного профиля [(off, dy), ...] вдоль p (n,2) с высотой y (n),
    поперечным вектором r (n,2). keep (n-1) — какие сегменты брать. -> (mat, P, N, UV, seg)"""
    s = np.concatenate([[0.0], np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))])
    out = []
    plen = np.concatenate([[0.0], np.cumsum([math.hypot(b[0] - a[0], b[1] - a[1])
                                             for a, b in zip(profile[:-1], profile[1:])])])
    up = np.array([0.0, 1.0, 0.0])
    idx = np.nonzero(keep)[0]
    if len(idx) == 0:
        return []
    for k in range(len(profile) - 1):
        (oa, ya), (ob, yb) = profile[k], profile[k + 1]
        # нормаль ребра профиля в поперечном сечении: (ya - yb, ob - oa) повёрнутая наружу (вверх/в сторону)
        ex, ey = ob - oa, yb - ya
        nn = np.array([ey, -ex]) / max(math.hypot(ex, ey), 1e-9)
        if nn[1] < 0 or (abs(nn[1]) < 1e-6 and nn[0] * (oa + ob) < 0):
            nn = -nn

        def pt(i, o, dy):
            return np.stack([p[i, 0] + r[i, 0] * o, y[i] + dy, p[i, 1] + r[i, 1] * o], 1)

        def nrm(i):
            n3 = np.stack([r[i, 0] * nn[0], np.full(len(i), nn[1]), r[i, 1] * nn[0]], 1)
            return n3

        i0, i1 = idx, idx + 1
        P00, P01 = pt(i0, oa, ya), pt(i0, ob, yb)
        P10, P11 = pt(i1, oa, ya), pt(i1, ob, yb)
        N0 = nrm(i0)
        uv = (np.full(len(idx), plen[k]), np.full(len(idx), plen[k + 1]), s[i0] / vscale, s[i1] / vscale)
        P, N, UV = quads(P00, P01, P10, P11, N0, N0, uv)
        out.append((mat, P, N, UV, np.concatenate([idx, idx])))
    return out


def frame(p):
    d = np.gradient(p, axis=0)
    d /= np.maximum(np.linalg.norm(d, axis=1, keepdims=True), 1e-9)
    return d, np.stack([-d[:, 1], d[:, 0]], 1)


def box(mat, c, ax, ay_half, ab_half, y0, y1, cx_dir):
    """Параллелепипед: центр c (x,z), ось a = cx_dir (2,), полуразмеры вдоль a и поперёк."""
    a = np.asarray(cx_dir, np.float64)
    a = a / max(np.linalg.norm(a), 1e-9)
    b = np.array([-a[1], a[0]])
    corners = [c + sa * ay_half * a + sb * ab_half * b for sa, sb in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    P, N, UV = [], [], []
    for k in range(4):
        q0, q1 = corners[k], corners[(k + 1) % 4]
        e = q1 - q0
        n = np.array([e[1], 0.0, -e[0]])
        n /= max(np.linalg.norm(n), 1e-9)
        mid = (q0 + q1) / 2 - c
        if n[0] * mid[0] + n[2] * mid[1] < 0:
            n = -n
        A = [[q0[0], y0, q0[1]], [q1[0], y0, q1[1]], [q1[0], y1, q1[1]], [q0[0], y1, q0[1]]]
        L = np.linalg.norm(e)
        P += [[A[0], A[1], A[2]], [A[0], A[2], A[3]]]
        N += [[n] * 3] * 2
        UV += [[[0, y0], [L, y0], [L, y1]], [[0, y0], [L, y1], [0, y1]]]
    top = [[c2[0], y1, c2[1]] for c2 in corners]
    P += [[top[0], top[2], top[1]], [top[0], top[3], top[2]]]
    N += [[[0, 1, 0]] * 3] * 2
    UV += [[[t[0], t[2]] for t in (top[0], top[2], top[1])], [[t[0], t[2]] for t in (top[0], top[3], top[2])]]
    return mat, np.array(P), np.array(N), np.array(UV)


def wire(mat, a, b, ya, yb, r=0.012):
    """Провод: четырёхгранная «труба» между точками."""
    A = np.array([a[0], ya, a[1]])
    B = np.array([b[0], yb, b[1]])
    d = B - A
    L = np.linalg.norm(d)
    if L < 1e-3:
        return None
    d /= L
    s = np.cross(d, [0, 1, 0])
    if np.linalg.norm(s) < 1e-3:                    # вертикальная стойка
        s = np.cross(d, [1, 0, 0])
    s /= np.linalg.norm(s)
    u = np.cross(s, d)
    ring = [s * r, u * r, -s * r, -u * r]
    P, N, UV = [], [], []
    for k in range(4):
        o0, o1 = ring[k], ring[(k + 1) % 4]
        n = (o0 + o1)
        n /= np.linalg.norm(n)
        q = [A + o0, B + o0, B + o1, A + o1]
        P += [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]
        N += [[n] * 3] * 2
        UV += [[[0, 0], [L, 0], [L, 1]], [[0, 0], [L, 1], [0, 1]]]
    return mat, np.array(P), np.array(N), np.array(UV)


def rail_profile():
    half = RAIL_HALF
    left = [(-x, y) for x, y in half]
    right = [(x, y) for x, y in reversed(half)]
    return left + right[1:]


def sparse_idx(p, y, max_turn=math.radians(0.3), max_len=24.0, max_dy=0.02):
    """Индексы точек плотной оси, которых хватает для протяжки: на прямой — раз в
    max_len, на кривой — чаще (поворот ≤ max_turn), на переломе профиля — по высоте."""
    tg = np.diff(p, axis=0)
    ang = np.arctan2(tg[:, 1], tg[:, 0])
    seg = np.linalg.norm(tg, axis=1)
    keep = [0]
    a0, s0 = ang[0], 0.0
    slope0 = 0.0
    for i in range(1, len(p) - 1):
        s0 += seg[i - 1]
        da = abs((ang[i] - a0 + math.pi) % (2 * math.pi) - math.pi)
        j = keep[-1]
        sl = (y[i] - y[j]) / max(s0, 1e-6)
        if da > max_turn or s0 >= max_len or abs(sl - slope0) * s0 > max_dy:
            keep.append(i)
            a0, s0 = ang[i], 0.0
            slope0 = sl
    keep.append(len(p) - 1)
    return np.array(keep)


def track_geometry(t, T, F, X0, Z0):
    """-> список (mat, P, N, UV, seg_xz_mid (m,2))."""
    p = t["xy"]
    if len(p) < 2:
        return []
    p, _ = densify(p, 2.0)
    tg, r = frame(p)
    f = lattice_sample(F, p[:, 0], p[:, 1], X0, Z0)
    dense = (p, tg, r, f)
    k = sparse_idx(p, f)
    p, tg, r, f = p[k], tg[k], r[k], f[k]
    mid = (p[:-1] + p[1:]) / 2
    keep = np.ones(len(p) - 1, bool)
    res = []

    def add(items):
        for mat, P, N, UV, seg in items:
            res.append((mat, P, N, UV, mid[seg]))

    btop = f + BALLAST_H
    if t["bridge"]:
        deck = [(-2.8, -0.25), (2.8, -0.25)]
        add(sweep(p, btop, r, deck, "ballast", 4.0, keep))
        add(sweep(p, btop, r, [(-2.8, -1.6), (-2.8, -0.25)], "concrete", 4.0, keep))
        add(sweep(p, btop, r, [(2.8, -0.25), (2.8, -1.6)], "concrete", 4.0, keep))
        add(sweep(p, btop, r, [(2.8, -1.6), (-2.8, -1.6)], "concrete", 4.0, keep))
        for side in (-1, 1):
            add(sweep(p, btop, r, [(side * 2.75, -0.25), (side * 2.75, 0.85)], "steel_grey", 4.0, keep))
            add(sweep(p, btop, r, [(side * 2.85, 0.85), (side * 2.85, -0.25)], "steel_grey", 4.0, keep))
        # опоры
        truss = t.get("truss", False)
        step = TRUSS_PIER_STEP if truss else PIER_STEP
        s = np.concatenate([[0.0], np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))])
        n_span = max(int(round(s[-1] / step)), 1)
        step = s[-1] / n_span
        for d in np.arange(step, s[-1] - 1.0, step):
            i = min(int(np.searchsorted(s, d)), len(p) - 1)
            c = np.array([np.interp(d, s, p[:, 0]), np.interp(d, s, p[:, 1])])
            g = float(lattice_sample(T, [c[0]], [c[1]], X0, Z0)[0])
            top = float(np.interp(d, s, btop)) - DECK_DEPTH
            if top - g > 1.0:
                ha, hb_ = (1.6, 4.5) if truss else (1.0, 2.4)
                mat, P, N, UV = box("concrete", c, None, ha, hb_, g - 3.0, top, tg[i])
                res.append((mat, P, N, UV, np.repeat(c[None], len(P), 0)))
        if truss:
            # сквозные фермы по сторонам: верхний пояс, стойки, раскосы, верхние связи
            def at(d, side, dy):
                q = np.array([np.interp(d, s, p[:, 0]), np.interp(d, s, p[:, 1])])
                rr = np.array([np.interp(d, s, r[:, 0]), np.interp(d, s, r[:, 1])])
                return q + rr * side, float(np.interp(d, s, btop)) + dy

            for span in range(n_span):
                a0, a1 = span * step, (span + 1) * step
                npan = max(int(round((a1 - a0) / TRUSS_PANEL)), 2)
                ds_ = np.linspace(a0, a1, npan + 1)
                for side in (-3.1, 3.1):
                    for k in range(npan):
                        lo0, ylo0 = at(ds_[k], side, -0.3)
                        lo1, ylo1 = at(ds_[k + 1], side, -0.3)
                        hi0, yhi0 = at(ds_[k], side, TRUSS_H)
                        hi1, yhi1 = at(ds_[k + 1], side, TRUSS_H)
                        beams = [(lo0, lo1, ylo0, ylo1, 0.30), (hi0, hi1, yhi0, yhi1, 0.30),
                                 (lo0, hi0, ylo0, yhi0, 0.15),
                                 ((lo0, hi1, ylo0, yhi1, 0.18) if k < npan / 2 else (hi0, lo1, yhi0, ylo1, 0.18))]
                        if k == npan - 1:
                            beams.append((lo1, hi1, ylo1, yhi1, 0.15))
                        for a_, b_, ya, yb, rad in beams:
                            item = wire("steel_grey", a_, b_, ya, yb, rad)
                            if item:
                                mat, P, N, UV = item
                                res.append((mat, P, N, UV, np.repeat(((a_ + b_) / 2)[None], len(P), 0)))
                for k in range(npan + 1):
                    l_, yl = at(ds_[k], -3.1, TRUSS_H)
                    r_, yr = at(ds_[k], 3.1, TRUSS_H)
                    item = wire("steel_grey", l_, r_, yl, yr, 0.12)
                    if item:
                        mat, P, N, UV = item
                        res.append((mat, P, N, UV, np.repeat(((l_ + r_) / 2)[None], len(P), 0)))
    else:
        prism = [(-BALLAST_TOE_HW, -0.05), (-BALLAST_TOP_HW, BALLAST_H), (BALLAST_TOP_HW, BALLAST_H),
                 (BALLAST_TOE_HW, -0.05)]
        add(sweep(p, f, r, prism, "ballast", 4.0, keep))

    # шпалы — по плотной оси
    pd, tgd, _, fd = dense
    s = np.concatenate([[0.0], np.cumsum(np.linalg.norm(np.diff(pd, axis=0), axis=1))])
    ds = np.arange(SLEEPER_STEP / 2, s[-1], SLEEPER_STEP)
    if len(ds):
        cx = np.interp(ds, s, pd[:, 0])
        cz = np.interp(ds, s, pd[:, 1])
        tx = np.interp(ds, s, tgd[:, 0])
        tz = np.interp(ds, s, tgd[:, 1])
        nrm = np.hypot(tx, tz)
        tx, tz = tx / nrm, tz / nrm
        rx, rz = -tz, tx
        yb = np.interp(ds, s, fd + BALLAST_H)
        y1 = yb + SLEEPER_UP
        y0 = yb - 0.03
        c = np.stack([cx, cz], 1)
        tv = np.stack([tx, tz], 1)
        rv = np.stack([rx, rz], 1)
        k = [c + sa * SLEEPER_HW * tv + sb * SLEEPER_HL * rv for sa, sb in ((-1, -1), (1, -1), (1, 1), (-1, 1))]

        def v3(q, yy):
            return np.stack([q[:, 0], yy, q[:, 1]], 1)

        up = np.tile([0.0, 1.0, 0.0], (len(ds), 1))
        P = [np.stack([v3(k[0], y1), v3(k[2], y1), v3(k[1], y1)], 1),
             np.stack([v3(k[0], y1), v3(k[3], y1), v3(k[2], y1)], 1)]
        N = [np.stack([up] * 3, 1)] * 2
        uvq =np.array([[0.0, 0.0], [0.23, 2.7], [0.23, 0.0]]), np.array([[0.0, 0.0], [0.0, 2.7], [0.23, 2.7]])
        UV = [np.broadcast_to(uvq[0], (len(ds), 3, 2)), np.broadcast_to(uvq[1], (len(ds), 3, 2))]
        # боковые грани вдоль шпалы (торцы почти не видны — их нет)
        for a_, b_, sgn in ((k[0], k[3], -1.0), (k[2], k[1], 1.0)):
            n = np.stack([tv[:, 0] * sgn, np.zeros(len(ds)), tv[:, 1] * sgn], 1)
            P += [np.stack([v3(a_, y0), v3(b_, y0), v3(b_, y1)], 1), np.stack([v3(a_, y0), v3(b_, y1), v3(a_, y1)], 1)]
            N += [np.stack([n] * 3, 1)] * 2
            UV += [np.broadcast_to(np.array([[0, 0], [2.7, 0], [2.7, 0.08]]), (len(ds), 3, 2)),
                   np.broadcast_to(np.array([[0, 0], [2.7, 0.08], [0, 0.08]]), (len(ds), 3, 2))]
        for Pi, Ni, Ui in zip(P, N, UV):
            res.append(("concrete", Pi, Ni, np.array(Ui), c))

    # рельсы
    rbase = btop + SLEEPER_UP + 0.015
    prof = rail_profile()
    for side in (-1, 1):
        pr = [(side * GAUGE_C + x, y) for x, y in prof]
        add(sweep(p, rbase, r, pr, "rail_steel", 2.0, keep))
    return res


def bucket(acc, chunk_set, mat, P, N, UV, xz):
    g = np.floor(xz / CHUNK).astype(np.int64)
    keys, inv = np.unique(g, axis=0, return_inverse=True)
    inv = inv.ravel()
    for k, (gx, gz) in enumerate(keys):
        key = (int(gx), int(gz))
        if key in chunk_set:
            m = inv == k
            acc[key].add(mat, P[m], N[m], UV[m])


def terrain_chunk(mesh, gx, gz, T, F, WL, X0, Z0, near_tree, near_lines, fine):
    step = FINE if fine else GRID
    n = int(CHUNK / step) + 1
    xs = gx * CHUNK + np.arange(n) * step
    zs = gz * CHUNK + np.arange(n) * step
    X, Z = np.meshgrid(xs, zs)
    x, z = X.ravel(), Z.ravel()
    h = lattice_sample(T, x, z, X0, Z0).astype(np.float64)
    wl = lattice_sample(np.nan_to_num(WL, nan=-1e4), x, z, X0, Z0)
    if (wl > -1e3).all():
        # чанк целиком под водой — одна плоскость
        y = float(np.median(wl))
        x0, z0, x1, z1 = gx * CHUNK, gz * CHUNK, (gx + 1) * CHUNK, (gz + 1) * CHUNK
        P = np.array([[[x0, y, z0], [x1, y, z1], [x1, y, z0]], [[x0, y, z0], [x0, y, z1], [x1, y, z1]]])
        mesh.add("water", P, np.tile([0.0, 1.0, 0.0], (2, 3, 1)), P[:, :, [0, 2]] / 8.0)
        return
    dist = np.full(len(x), 1e9)
    if fine and near_lines:
        pts = shapely.points(x, z)
        idx, d = near_tree.query_nearest(pts, return_distance=True, max_distance=80.0)
        dist[idx[0]] = d
        f = lattice_sample(F, x, z, X0, Z0)
        lim = np.maximum(dist - BALLAST_TOE_HW, 0.0) / SLOPE
        conf = f - 0.05 + np.clip(h - (f - 0.05), -lim, lim)
        h = np.where(dist < 80.0, conf, h)
    H = h.reshape(n, n)
    # нормали по сетке
    gzz, gxx = np.gradient(H, step)
    Nn = np.stack([-gxx, np.ones_like(H), -gzz], -1)
    Nn /= np.linalg.norm(Nn, axis=-1, keepdims=True)
    V = np.stack([X, H, Z], -1)
    i, j = np.meshgrid(np.arange(n - 1), np.arange(n - 1))
    i, j = i.ravel(), j.ravel()
    a, b, c, d = (j, i), (j, i + 1), (j + 1, i + 1), (j + 1, i)
    P = np.concatenate([np.stack([V[a], V[c], V[b]], 1), np.stack([V[a], V[d], V[c]], 1)])
    N = np.concatenate([np.stack([Nn[a], Nn[c], Nn[b]], 1), np.stack([Nn[a], Nn[d], Nn[c]], 1)])
    UV = P[:, :, [0, 2]] / 8.0
    cen = P.mean(1)
    D = dist.reshape(n, n)
    dmin = np.concatenate([np.minimum.reduce([D[a], D[b], D[c]]), np.minimum.reduce([D[a], D[d], D[c]])])
    WLm = wl.reshape(n, n)
    wet = np.concatenate([np.minimum.reduce([WLm[a], WLm[b], WLm[c]]), np.minimum.reduce([WLm[a], WLm[d], WLm[c]])]) > -1e3
    mats = np.where(wet, "water", np.where(dmin < 7.0, "gravel", "grass"))
    for m in ("grass", "gravel", "water"):
        k = mats == m
        if k.any():
            Pm = P[k].copy()
            Nm = N[k].copy()
            if m == "water":
                wlv = np.concatenate([WLm[a], WLm[a]])[k]
                Pm[:, :, 1] = wlv[:, None]
                Nm[:] = [0, 1, 0]
            mesh.add(m, Pm, Nm, UV[k])


def platform_geometry(poly, F, X0, Z0):
    c = poly.representative_point()
    f = float(lattice_sample(F, [c.x], [c.y], X0, Z0)[0])
    top = f + BALLAST_H + SLEEPER_UP + 0.015 + RAIL_H + PLATFORM_H
    bot = f - 0.1
    tris = [t for t in triangulate(poly) if poly.contains(t.representative_point())]
    P, N, UV = [], [], []
    for t in tris:
        q = list(t.exterior.coords)[:3]
        P.append([[q[0][0], top, q[0][1]], [q[2][0], top, q[2][1]], [q[1][0], top, q[1][1]]])
        N.append([[0, 1, 0]] * 3)
        UV.append([[q[0][0] / 4, q[0][1] / 4], [q[2][0] / 4, q[2][1] / 4], [q[1][0] / 4, q[1][1] / 4]])
    out = [("asphalt", np.array(P), np.array(N), np.array(UV))] if P else []
    P, N, UV = [], [], []
    for ring in [poly.exterior] + list(poly.interiors):
        co = list(ring.coords)
        for q0, q1 in zip(co[:-1], co[1:]):
            e = np.array([q1[0] - q0[0], q1[1] - q0[1]])
            L = np.linalg.norm(e)
            if L < 1e-3:
                continue
            n = np.array([e[1], 0, -e[0]]) / L
            A = [[q0[0], bot, q0[1]], [q1[0], bot, q1[1]], [q1[0], top, q1[1]], [q0[0], top, q0[1]]]
            P += [[A[0], A[1], A[2]], [A[0], A[2], A[3]]]
            N += [[n] * 3] * 2
            UV += [[[0, bot], [L, bot], [L, top]], [[0, bot], [L, top], [0, top]]]
    if P:
        out.append(("concrete", np.array(P), np.array(N), np.array(UV)))
    return out


# ------------------------------------------------------------------ OBJ

def write_obj(path, mesh, header):
    import io
    buf = io.StringIO()
    buf.write(header + "\nmtllib ../rail.mtl\n")
    vo = to = no = 0
    total = 0
    for mat in sorted(mesh.g):
        P = np.concatenate([e[0] for e in mesh.g[mat]]).reshape(-1, 3).astype(np.float64)
        N = np.concatenate([e[1] for e in mesh.g[mat]]).reshape(-1, 3).astype(np.float64)
        T = np.concatenate([e[2] for e in mesh.g[mat]]).reshape(-1, 2).astype(np.float64)
        if not len(P):
            continue
        # рельсы — до мм (профиль Р65 мельче 1 см), остальное — до 1 см
        scale = 1000 if mat == "rail_steel" else 100
        pq = np.rint(P * scale).astype(np.int64)
        nq = np.rint(N * 1000).astype(np.int64)
        tq = np.rint(T * 1000).astype(np.int64)
        q3 = pq.reshape(-1, 3, 3).astype(np.float64)
        cr = np.cross(q3[:, 1] - q3[:, 0], q3[:, 2] - q3[:, 0])
        nm = N.reshape(-1, 3, 3).mean(1)
        flip = (cr * nm).sum(1) < 0
        good = np.linalg.norm(cr, axis=1) > 0
        pq3 = pq.reshape(-1, 3, 3)
        nq3 = nq.reshape(-1, 3, 3)
        tq3 = tq.reshape(-1, 3, 2)
        o = np.array([0, 2, 1])
        pq3[flip] = pq3[flip][:, o]
        nq3[flip] = nq3[flip][:, o]
        tq3[flip] = tq3[flip][:, o]
        pq, nq, tq = pq3[good].reshape(-1, 3), nq3[good].reshape(-1, 3), tq3[good].reshape(-1, 2)
        if not len(pq):
            continue
        up, ip = np.unique(pq, axis=0, return_inverse=True)
        un, inn = np.unique(nq, axis=0, return_inverse=True)
        ut, it = np.unique(tq, axis=0, return_inverse=True)
        buf.write(f"o {mat}\nusemtl {mat}\n")
        np.savetxt(buf, up / scale, fmt="v %.3f %.3f %.3f" if scale == 1000 else "v %.2f %.2f %.2f")
        np.savetxt(buf, ut / 1000.0, fmt="vt %.3f %.3f")
        np.savetxt(buf, un / 1000.0, fmt="vn %.3f %.3f %.3f")
        f = np.stack([ip.ravel() + vo + 1, it.ravel() + to + 1, inn.ravel() + no + 1], 1).reshape(-1, 9)
        np.savetxt(buf, f, fmt="f %d/%d/%d %d/%d/%d %d/%d/%d")
        vo += len(up)
        to += len(ut)
        no += len(un)
        total += len(pq) // 3
    with open(path, "w", newline="\n", encoding="utf-8") as fh:
        fh.write(buf.getvalue())
    return total


# ------------------------------------------------------------------ воркер

def init_worker(out):
    sys.stdout.reconfigure(encoding="utf-8")
    lat = np.load(out / "_lattice.npz")
    _G.update({k: lat[k] for k in ("T", "F", "WL")})
    _G["X0"], _G["Z0"] = float(lat["X0"]), float(lat["Z0"])
    with open(out / "_prep.pkl", "rb") as fh:
        d = pickle.load(fh)
    _G.update(d)
    _G["chunk_set"] = set(map(tuple, d["chunks"]))
    _G["lines"] = [LineString(t["xy"]) for t in d["tracks"]]
    _G["tree"] = shapely.STRtree(_G["lines"])
    _G["ground_lines"] = [i for i, t in enumerate(d["tracks"]) if not t["bridge"]]
    _G["out"] = out
    mp_ = [Point(m[0]) for m in d["masts"]]
    _G["mast_tree"] = shapely.STRtree(mp_) if mp_ else None
    wm = [Point((a + b) / 2) for a, b in d["wires"]]
    _G["wire_tree"] = shapely.STRtree(wm) if wm else None
    _G["plat_tree"] = shapely.STRtree(d["plats"]) if d["plats"] else None


def build_super(key):
    sx, sz = key
    G = _G
    T, F, WL, X0, Z0 = G["T"], G["F"], G["WL"], G["X0"], G["Z0"]
    bx0, bz0 = sx * SUPER * CHUNK, sz * SUPER * CHUNK
    tile = shapely.box(bx0, bz0, bx0 + SUPER * CHUNK, bz0 + SUPER * CHUNK)
    mine = [c for c in G["chunk_set"] if c[0] // SUPER == sx and c[1] // SUPER == sz]
    if not mine:
        return []
    acc = defaultdict(Mesh)
    cs = set(mine)
    near = G["tree"].query(tile.buffer(FINE_NEAR + 100))
    ground = [i for i in near if not G["tracks"][i]["bridge"]]
    gtree = shapely.STRtree([G["lines"][i] for i in ground]) if ground else None
    for (gx, gz) in mine:
        cb = shapely.box(gx * CHUNK, gz * CHUNK, (gx + 1) * CHUNK, (gz + 1) * CHUNK)
        fine = gtree is not None and len(gtree.query(cb.buffer(FINE_NEAR))) > 0
        terrain_chunk(acc[(gx, gz)], gx, gz, T, F, WL, X0, Z0, gtree, ground, fine)
    for i in G["tree"].query(tile.buffer(10)):
        for mat, P, N, UV, xz in track_geometry(G["tracks"][i], T, F, X0, Z0):
            bucket(acc, cs, mat, P, N, UV, xz)
    # контактная сеть
    if G["mast_tree"] is not None:
        for mi in G["mast_tree"].query(tile):
            pc, side, on_bridge = G["masts"][mi]
            f = float(lattice_sample(F, [pc[0]], [pc[1]], X0, Z0)[0])
            rt = f + BALLAST_H + SLEEPER_UP + 0.015 + RAIL_H
            base = pc + side * MAST_OFF
            tgt = np.array([side[1], -side[0]])
            for item in (box("concrete", base, None, 0.2, 0.2, f - 0.5, rt + MAST_H, tgt),
                         box("steel_grey", pc + side * (MAST_OFF / 2), None, MAST_OFF / 2 + 0.2, 0.05,
                             rt + WIRE_MESSENGER - 0.05, rt + WIRE_MESSENGER + 0.05, side),
                         box("steel_grey", pc + side * (MAST_OFF / 2), None, MAST_OFF / 2 + 0.2, 0.04,
                             rt + WIRE_CONTACT + 0.25, rt + WIRE_CONTACT + 0.33, side)):
                mat, P, N, UV = item
                bucket(acc, cs, mat, P, N, UV, np.repeat(pc[None], len(P), 0))
    if G["wire_tree"] is not None:
        for wi in G["wire_tree"].query(tile):
            a, b = G["wires"][wi]
            fa, fb = lattice_sample(F, [a[0], b[0]], [a[1], b[1]], X0, Z0)
            ra = fa + BALLAST_H + SLEEPER_UP + 0.015 + RAIL_H
            rb = fb + BALLAST_H + SLEEPER_UP + 0.015 + RAIL_H
            mid = (a + b) / 2
            for item in (wire("cable", a, b, ra + WIRE_CONTACT, rb + WIRE_CONTACT, 0.007),
                         wire("cable", a, mid, ra + WIRE_MESSENGER, (ra + rb) / 2 + WIRE_MESSENGER - 0.9, 0.007),
                         wire("cable", mid, b, (ra + rb) / 2 + WIRE_MESSENGER - 0.9, rb + WIRE_MESSENGER, 0.007)):
                if item:
                    mat, P, N, UV = item
                    bucket(acc, cs, mat, P, N, UV, np.repeat(mid[None], len(P), 0))
    if G["plat_tree"] is not None:
        for pi in G["plat_tree"].query(tile):
            poly = G["plats"][pi]
            c = poly.representative_point()
            for mat, P, N, UV in platform_geometry(poly, F, X0, Z0):
                bucket(acc, cs, mat, P, N, UV, np.repeat([[c.x, c.y]], len(P), 0))
    res = []
    for (gx, gz), m in acc.items():
        name = f"chunk_{gx}_{gz}.obj"
        n = write_obj(G["out"] / "chunks" / name, m, f"# rail_map chunk {gx} {gz}")
        res.append({"file": f"chunks/{name}", "gx": gx, "gz": gz, "triangles": n})
    return res


# ------------------------------------------------------------------ main

def write_materials(out):
    text = (SAMARA / "samara.mtl").read_text(encoding="utf-8")
    blocks, cur = {}, None
    for line in text.splitlines():
        if line.startswith("newmtl "):
            cur = line.split(None, 1)[1].strip()
            blocks[cur] = [line]
        elif cur:
            blocks[cur].append(line)
    lines = ["# rail.mtl — материалы железной дороги (текстуры из tools/samara_map/textures.py)"]
    (out / "textures").mkdir(exist_ok=True)
    for m in MATS:
        if m not in blocks:
            raise SystemExit(f"в samara.mtl нет материала {m}")
        lines += blocks[m] + [""]
        for suf in ("albedo", "normal", "rough"):
            src = SAMARA / "textures" / f"{m}_{suf}.png"
            if src.exists():
                shutil.copy2(src, out / "textures" / src.name)
    (out / "rail.mtl").write_text("\n".join(lines), encoding="utf-8")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", default="samara_syzran")
    ap.add_argument("--km", nargs=2, type=float, help="только участок маршрута, км от начала")
    ap.add_argument("--out")
    ap.add_argument("--workers", type=int, default=WORKERS)
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")
    suffix = f"_km{int(args.km[0])}_{int(args.km[1])}" if args.km else ""
    out = Path(args.out) if args.out else ROOT / "data" / "rail_map" / f"{args.name}_3d{suffix}"
    if (out / "chunks").exists():
        shutil.rmtree(out / "chunks")
    (out / "chunks").mkdir(parents=True)
    t0 = time.time()
    chunks = prep(args, out)
    write_materials(out)
    supers = sorted({(gx // SUPER, gz // SUPER) for gx, gz in chunks})
    print(f"суперплиток {len(supers)}, воркеров {args.workers}")
    manifest_chunks = []
    with mp.Pool(args.workers, initializer=init_worker, initargs=(out,)) as pool:
        for k, res in enumerate(pool.imap_unordered(build_super, supers)):
            manifest_chunks += res
            if k % 10 == 0 or k == len(supers) - 1:
                tri = sum(c["triangles"] for c in manifest_chunks)
                print(f"  {k + 1}/{len(supers)}  треугольников {tri / 1e6:.1f} M  ({time.time() - t0:.0f} с)", flush=True)
    manifest_chunks.sort(key=lambda c: (c["gx"], c["gz"]))
    tri = sum(c["triangles"] for c in manifest_chunks)
    (out / "manifest.json").write_text(json.dumps({
        "name": f"Ж/д {args.name}{suffix}",
        "axes": {"x": "east", "y": "up (meters above sea level)", "z": "south", "handedness": "right"},
        "origin": {"lat": ORIGIN_LAT, "lon": ORIGIN_LON, "label": "площадь Куйбышева (как карта Самары)"},
        "projection": PROJ, "chunk_size": CHUNK, "terrain_grid": GRID,
        "materials": MATS, "triangles": tri, "chunks": manifest_chunks}, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"готово: {len(manifest_chunks)} чанков, {tri / 1e6:.1f} M треугольников, {time.time() - t0:.0f} с → {out}")


if __name__ == "__main__":
    main()
