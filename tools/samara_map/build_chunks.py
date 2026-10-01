"""Этап 3: сборка чанков 256x256 м в OBJ (+ инстансы деревьев/фонарей в CSV).

Каждый chunks/chunk_<gx>_<gz>.obj содержит геометрию в МИРОВЫХ координатах
(X восток, Y высота, Z юг), материалы из ../samara.mtl, по группе `o` на материал.
Здание попадает в тот чанк, где лежит его представительная точка (как в
split_mesh_by_chunk редактора — объект целиком, без разрезания).
"""
import json
import math
import pickle
import sys
import time
from collections import defaultdict
from multiprocessing import Pool

import numpy as np
import shapely
import shapely.ops
from shapely import wkb as swkb

import config as C
from materials import MATERIALS
from models import LAMP_LIGHT

UP = np.array([0.0, 1.0, 0.0])

# ------------------------------------------------------------------ теги

def num(s, default=None):
    if s is None:
        return default
    try:
        s = str(s).replace(",", ".").split(";")[-1].strip().split()[0].rstrip("m")
        return float(s)
    except Exception:
        return default


def h01(x, z, salt=0):
    v = (int(x * 7.3) * 73856093) ^ (int(z * 7.3) * 19349663) ^ (salt * 83492791)
    v = (v ^ (v >> 13)) * 1274126177 & 0xFFFFFFFF
    return (v & 0xFFFFFF) / float(0x1000000)


ROAD_W = {"motorway": 22, "trunk": 18, "primary": 14, "secondary": 11, "tertiary": 9, "unclassified": 7,
          "residential": 6.5, "living_street": 5.5, "service": 4.0, "road": 6, "busway": 7,
          "motorway_link": 7, "trunk_link": 7, "primary_link": 7, "secondary_link": 6.5, "tertiary_link": 6}
PATH_W = {"pedestrian": 6, "footway": 2.2, "path": 1.5, "cycleway": 2.0, "steps": 2.5, "bridleway": 2.0, "track": 3.5}
UNPAVED = {"unpaved", "dirt", "ground", "grass", "compacted", "gravel", "fine_gravel", "sand", "earth", "mud", "pebblestone", "grass_paver", "wood"}
PAVING = {"paving_stones", "sett", "cobblestone", "concrete:plates", "unhewn_cobblestone", "bricks", "tiles"}

# приоритеты разбиения земли (меньше = главнее)
P_WATER, P_RAILBED, P_ROAD, P_PAVE, P_DIRTPATH, P_SPORT, P_SAND, P_HARD, P_GREEN, P_FOREST, P_OPEN, P_URBAN = range(12)


def surface_mat(surf, default):
    if surf in UNPAVED:
        return "gravel" if surf in ("gravel", "fine_gravel", "pebblestone") else ("sand" if surf == "sand" else "dirt")
    if surf in PAVING:
        return "paving"
    if surf in ("asphalt", "concrete"):
        return "asphalt" if surf == "asphalt" else "concrete"
    return default


def road_width(t, hw):
    w = num(t.get("width"))
    if w and 1 < w < 60:
        return w
    lanes = num(t.get("lanes"))
    if lanes and hw in ROAD_W:
        return lanes * 3.5 + 1.0
    if hw == "service" and t.get("service") in ("driveway", "alley"):
        return 3.2
    if hw == "service" and t.get("service") == "parking_aisle":
        return 5.0
    return ROAD_W.get(hw, PATH_W.get(hw))


def area_class(t):
    lu, nat, lei = t.get("landuse"), t.get("natural"), t.get("leisure")
    if nat == "water" or t.get("waterway") in ("riverbank", "dock") or lu in ("reservoir", "basin"):
        return P_WATER, "water"
    ah = t.get("area:highway") or (t.get("highway") if t.get("area") == "yes" else None)
    if ah:
        if ah in ("footway", "pedestrian", "platform", "path", "steps"):
            return P_PAVE, surface_mat(t.get("surface"), "paving")
        return P_ROAD, surface_mat(t.get("surface"), "asphalt")
    if t.get("amenity") == "parking" or lu == "garages":
        return P_ROAD, surface_mat(t.get("surface"), "asphalt")
    if t.get("railway") == "platform":
        return P_PAVE, "paving"
    if lei == "pitch":
        s = t.get("surface")
        return P_SPORT, "tartan" if s == "tartan" else ("asphalt" if s == "asphalt" else ("sand" if s == "sand" else "pitch"))
    if lei == "track":
        return P_SPORT, "tartan"
    if nat in ("beach", "sand") or lei == "playground":
        return P_SAND, "sand"
    if lu in ("construction", "brownfield", "landfill", "quarry"):
        return P_HARD, "dirt"
    if lu == "railway" or nat in ("bare_rock", "scree", "shingle"):
        return P_HARD, "gravel"
    if lu == "industrial":
        return P_HARD, "concrete"
    if lei in ("park", "garden", "golf_course", "dog_park") or lu in ("grass", "village_green", "recreation_ground", "cemetery", "orchard", "flowerbed"):
        return P_GREEN, "grass"
    if lu == "forest" or nat == "wood":
        return P_FOREST, "forest_floor"
    if nat in ("scrub", "heath", "grassland", "wetland") or lu in ("meadow",):
        return P_OPEN, "meadow"
    if lu in ("farmland", "farmyard"):
        return P_OPEN, "farmland"
    if lu == "allotments":
        return P_URBAN, "grass_urban"
    if lu in ("residential", "commercial", "retail", "education", "institutional", "military"):
        return P_URBAN, "grass_urban"
    return None


# ------------------------------------------------------------------ рельеф

class Terrain:
    def __init__(self):
        g = json.load(open(C.WORK / "grid.json"))
        self.x0, self.z0, self.step = g["x0"], g["z0"], g["step"]
        self.H = np.load(C.WORK / "heights.npy", mmap_mode="r")
        self.L = np.load(C.WORK / "water_level.npy", mmap_mode="r")

    def window(self, xmin, zmin, xmax, zmax):
        s = self.step
        i0 = max(int(math.floor((xmin - self.x0) / s)) - 3, 0)
        j0 = max(int(math.floor((zmin - self.z0) / s)) - 3, 0)
        i1 = min(int(math.ceil((xmax - self.x0) / s)) + 3, self.H.shape[1] - 1)
        j1 = min(int(math.ceil((zmax - self.z0) / s)) + 3, self.H.shape[0] - 1)
        self.i0, self.j0 = i0, j0
        self.h = np.array(self.H[j0:j1 + 1, i0:i1 + 1], np.float64)
        gz, gx = np.gradient(self.h, s)
        n = np.dstack([-gx, np.ones_like(self.h), -gz])
        self.n = n / np.linalg.norm(n, axis=2, keepdims=True)
        lv = np.array(self.L[j0:j1 + 1, i0:i1 + 1], np.float64)
        from scipy import ndimage
        nanm = np.isnan(lv)
        if (~nanm).any():
            d, (jj, ii) = ndimage.distance_transform_edt(nanm, return_indices=True)
            filled = lv[jj, ii]
            filled[d > 4] = np.nan
        else:
            filled = lv
        self.level = filled

    def _loc(self, x, z):
        fx = (np.asarray(x, np.float64) - self.x0) / self.step - self.i0
        fz = (np.asarray(z, np.float64) - self.z0) / self.step - self.j0
        i = np.clip(np.floor(fx).astype(np.int64), 0, self.h.shape[1] - 2)
        j = np.clip(np.floor(fz).astype(np.int64), 0, self.h.shape[0] - 2)
        return i, j, fx - i, fz - j

    def _interp(self, arr, x, z):
        i, j, u, v = self._loc(x, z)
        a00 = arr[j, i]; a10 = arr[j, i + 1]; a01 = arr[j + 1, i]; a11 = arr[j + 1, i + 1]
        lower = (u + v) <= 1
        if arr.ndim == 3:
            u = u[..., None]; v = v[..., None]; lower = lower[..., None]
        return np.where(lower, a00 + (a10 - a00) * u + (a01 - a00) * v,
                        a11 + (a01 - a11) * (1 - u) + (a10 - a11) * (1 - v))

    def height(self, x, z):
        return self._interp(self.h, x, z)

    def normal(self, x, z):
        n = self._interp(self.n, x, z)
        return n / np.linalg.norm(n, axis=-1, keepdims=True)

    def water_level(self, x, z):
        i, j, u, v = self._loc(x, z)
        ii = np.clip(np.rint(i + u).astype(int), 0, self.level.shape[1] - 1)
        jj = np.clip(np.rint(j + v).astype(int), 0, self.level.shape[0] - 1)
        return self.level[jj, ii]

    def grid_tris(self, x0, z0, x1, z1):
        """Треугольники сетки рельефа внутри прямоугольника (диагональ как в _interp)."""
        s = self.step
        xs = np.arange(x0, x1, s)
        zs = np.arange(z0, z1, s)
        X, Z = np.meshgrid(xs, zs)
        X = X.ravel(); Z = Z.ravel()
        a = np.stack([X, Z], 1); b = np.stack([X + s, Z], 1); c = np.stack([X, Z + s], 1); d = np.stack([X + s, Z + s], 1)
        t1 = np.stack([a, b, c, a], 1)
        t2 = np.stack([b, d, c, b], 1)
        return shapely.polygons(np.concatenate([t1, t2]))


# ------------------------------------------------------------------ меш

class Mesh:
    def __init__(self):
        self.g = defaultdict(list)

    def add(self, mat, P, N, UV, want=None):
        """P,N: (n,3,3), UV: (n,3,2). want: (n,3) желаемое направление нормали грани
        — по нему выправляется обход (CCW снаружи, как принято в OBJ)."""
        if len(P) == 0:
            return
        P = np.array(P, np.float64); N = np.array(N, np.float64); UV = np.array(UV, np.float64)
        cr = np.cross(P[:, 1] - P[:, 0], P[:, 2] - P[:, 0])
        area = np.linalg.norm(cr, axis=1)
        ok = area > 1e-5
        want = N.mean(1) if want is None else np.array(want, np.float64)
        flip = (cr * want).sum(1) < 0
        idx = np.array([0, 2, 1])
        P[flip] = P[flip][:, idx]; N[flip] = N[flip][:, idx]; UV[flip] = UV[flip][:, idx]
        self.g[mat].append((P[ok], N[ok], UV[ok]))

    def tri_count(self):
        return sum(sum(len(p) for p, _, _ in lst) for lst in self.g.values())

    def write(self, path, header):
        out = [header, "mtllib ../samara.mtl"]
        vo = to = no = 0
        stats = {}
        chunks = []
        for mat in sorted(self.g):
            P = np.concatenate([p for p, _, _ in self.g[mat]]).reshape(-1, 3)
            N = np.concatenate([n for _, n, _ in self.g[mat]]).reshape(-1, 3)
            T = np.concatenate([t for _, _, t in self.g[mat]]).reshape(-1, 2)
            if len(P) == 0:
                continue
            pq = np.rint(P * 100).astype(np.int64)
            nq = np.rint(N * 1000).astype(np.int64)
            tq = np.rint(T * 1000).astype(np.int64)
            # после округления до 1 см «иголки» могут выродиться или перевернуться — выкидываем
            q3 = pq.reshape(-1, 3, 3).astype(np.float64)
            cr = np.cross(q3[:, 1] - q3[:, 0], q3[:, 2] - q3[:, 0])
            good = ((cr * N.reshape(-1, 3, 3).mean(1)).sum(1) > 0) & (np.linalg.norm(cr, axis=1) > 0)
            good3 = np.repeat(good, 3)
            pq, nq, tq = pq[good3], nq[good3], tq[good3]
            if len(pq) == 0:
                continue
            up, ip = np.unique(pq, axis=0, return_inverse=True)
            un, inn = np.unique(nq, axis=0, return_inverse=True)
            ut, it = np.unique(tq, axis=0, return_inverse=True)
            ip = ip.ravel(); inn = inn.ravel(); it = it.ravel()
            chunks.append((mat, up, un, ut, ip + vo + 1, it + to + 1, inn + no + 1))
            vo += len(up); to += len(ut); no += len(un)
            stats[mat] = len(pq) // 3
        import io
        buf = io.StringIO()
        buf.write("\n".join(out) + "\n")
        for mat, up, un, ut, ip, it, inn in chunks:
            buf.write(f"o {mat}\nusemtl {mat}\n")
            np.savetxt(buf, up / 100.0, fmt="v %.2f %.2f %.2f")
            np.savetxt(buf, ut / 1000.0, fmt="vt %.3f %.3f")
            np.savetxt(buf, un / 1000.0, fmt="vn %.3f %.3f %.3f")
            f = np.stack([ip, it, inn], 1).reshape(-1, 9)
            np.savetxt(buf, f, fmt="f %d/%d/%d %d/%d/%d %d/%d/%d")
        with open(path, "w", newline="\n", encoding="utf-8") as fh:
            fh.write(buf.getvalue())
        return stats


def tris_from_polys(polys):
    """Полигоны -> (n,3,2) треугольники (constrained Delaunay, дырки учитываются)."""
    polys = np.asarray(polys, dtype=object)
    if len(polys) == 0:
        return np.zeros((0, 3, 2))
    parts = shapely.get_parts(polys)
    parts = parts[shapely.get_type_id(parts) == 3]
    parts = parts[shapely.area(parts) > 1e-4]
    if len(parts) == 0:
        return np.zeros((0, 3, 2))
    tri = shapely.get_parts(shapely.constrained_delaunay_triangles(parts))
    tri = tri[shapely.get_type_id(tri) == 3]
    if len(tri) == 0:
        return np.zeros((0, 3, 2))
    co = shapely.get_coordinates(tri).reshape(-1, 4, 2)[:, :3]
    return co


def planar_uv(xz, mat):
    tu, tv = MATERIALS[mat]["tile"]
    return np.stack([xz[..., 0] / tu, -xz[..., 1] / tv], -1)


# ------------------------------------------------------------------ земля

def drape(mesh, terr, mat, geom, grid_tris, dy=0.0, flat_y=None, water=False):
    if geom is None or geom.is_empty:
        return
    pieces = shapely.intersection(grid_tris, geom)
    pieces = pieces[~shapely.is_empty(pieces)]
    xz = tris_from_polys(pieces)
    if len(xz) == 0:
        return
    x, z = xz[..., 0], xz[..., 1]
    if water:
        y = terr.water_level(x, z)
        ground = terr.height(x, z)
        y = np.where(np.isnan(y), ground - 0.3, y)
        N = np.broadcast_to(UP, xz.shape[:2] + (3,))
    else:
        y = terr.height(x, z) + dy
        N = terr.normal(x, z)
    P = np.stack([x, y, z], -1)
    mesh.add(mat, P, N, planar_uv(xz, mat), want=np.broadcast_to(UP, (len(P), 3)))


def safe_union(geoms):
    geoms = [g for g in geoms if g is not None and not g.is_empty]
    if not geoms:
        return None
    try:
        return shapely.union_all(shapely.set_precision(np.array(geoms, dtype=object), 0.01))
    except Exception:
        return shapely.union_all([shapely.make_valid(g) for g in geoms])


def safe_diff(a, b):
    if a is None or b is None:
        return a
    try:
        return shapely.difference(a, b, grid_size=0.01)
    except Exception:
        return shapely.difference(shapely.make_valid(a).buffer(0), shapely.make_valid(b).buffer(0))


# ------------------------------------------------------------------ здания

HISTORIC_BOX = (-2500, -1100, 1700, 2900)   # старая Самара, локальные x/z (эвристика)
HOUSE = {"house", "detached", "semidetached_house", "bungalow", "dacha", "farm", "cabin", "terrace"}
SHED = {"shed", "hut", "barn", "stable", "farm_auxiliary", "greenhouse", "sty", "cowshed", "toilets"}
GARAGE = {"garage", "garages", "carport"}
INDUSTRIAL = {"industrial", "warehouse", "factory", "hangar", "manufacture", "storage_tank", "service", "transformer_tower", "silo"}
COMMERCIAL = {"commercial", "retail", "supermarket", "kiosk", "office", "mall", "shop"}
WORSHIP = {"church", "cathedral", "chapel", "mosque", "synagogue", "temple", "monastery"}
PUBLIC = {"school", "kindergarten", "hospital", "university", "college", "public", "civic", "government", "train_station", "sports_hall", "stadium"}


def building_style(t, area, cx, cz):
    b = t.get("building") or t.get("building:part") or "yes"
    mat_tag = t.get("building:material")
    r = h01(cx, cz, 1)
    lv = num(t.get("building:levels"))
    hist = HISTORIC_BOX[0] < cx < HISTORIC_BOX[2] and HISTORIC_BOX[1] < cz < HISTORIC_BOX[3]
    if b in GARAGE:
        facade, lv = "wall_garage", lv or 1
    elif b in SHED:
        facade, lv = "wall_wood", lv or 1
    elif b in HOUSE:
        facade, lv = ("wall_wood" if r < 0.55 else "facade_brick"), lv or (1 if area < 90 or r < 0.5 else 2)
    elif b in INDUSTRIAL:
        facade, lv = "wall_industrial", lv or (1 if area < 3000 else 2)
    elif b in WORSHIP or t.get("amenity") == "place_of_worship":
        facade, lv = "facade_historic", lv or 2
    elif b in COMMERCIAL:
        lv = lv or (1 if area < 400 else 2)
        facade = "facade_glass" if lv >= 7 else "facade_commercial"
    elif b in PUBLIC:
        lv = lv or 3
        facade = "facade_brick" if r < 0.6 else "facade_commercial"
    else:  # apartments / residential / yes / ...
        if lv is None:
            if area < 50:
                facade, lv = "wall_garage" if r < 0.5 else "wall_wood", 1
                return finish_style(t, b, facade, lv, area, r, cx, cz)
            if area < 150:
                facade, lv = "wall_wood", 1 if r < 0.6 else 2
                return finish_style(t, b, facade, lv, area, r, cx, cz, house_like=True)
            lv = 3 if area < 300 else (5 if area < 700 else (5 if r < 0.6 else 9) if area < 2000 else 9)
        if hist and lv <= 4:
            facade = "facade_historic"
        elif lv <= 4:
            facade = "facade_brick" if r < 0.6 else "facade_historic"
        elif lv <= 5:
            facade = "facade_panel" if r < 0.5 else "facade_brick"
        elif lv <= 10:
            facade = "facade_panel" if r < 0.7 else "facade_brick"
        else:
            facade = "facade_panel" if r < 0.6 else ("facade_glass" if r < 0.75 else "facade_brick")
    if mat_tag == "glass":
        facade = "facade_glass"
    elif mat_tag == "wood":
        facade = "wall_wood"
    elif mat_tag in ("brick", "stone"):
        facade = "facade_brick"
    elif mat_tag in ("metal", "steel"):
        facade = "wall_industrial"
    elif mat_tag in ("concrete", "panel", "reinforced_concrete"):
        facade = "facade_panel"
    return finish_style(t, b, facade, lv, area, r, cx, cz, house_like=b in HOUSE)


def finish_style(t, b, facade, lv, area, r, cx, cz, house_like=False):
    floor_h = MATERIALS[facade].get("floor", 3.0)
    lv = max(1, min(int(round(lv)), 60))
    shape = t.get("roof:shape")
    if shape is None:
        if (house_like or facade == "wall_wood") and area < 400:
            shape = "gabled" if r < 0.6 else "hipped"
        elif facade == "wall_garage" or b in GARAGE:
            shape = "flat"
        else:
            shape = "flat"
    roof_mat = "roof_flat"
    if shape in ("gabled", "hipped", "pyramidal", "skillion", "gambrel", "mansard", "half-hipped", "cone"):
        roof_mat = "roof_metal" if h01(cx, cz, 2) < 0.75 else "roof_tile"
    if shape in ("dome", "onion"):
        roof_mat = "dome_gold" if (t.get("amenity") == "place_of_worship" or b in WORSHIP or t.get("religion")) else "roof_metal"
    return dict(facade=facade, levels=lv, floor_h=floor_h, shape=shape, roof_mat=roof_mat)


def ring_walls(mesh, ring, yb, yt, yref, mat):
    co = np.asarray(ring.coords)[:, :2]
    p, q = co[:-1], co[1:]
    d = q - p
    L = np.hypot(d[:, 0], d[:, 1])
    keep = L > 0.05
    p, q, d, L = p[keep], q[keep], d[keep], L[keep]
    if len(p) == 0:
        return
    n2 = np.stack([d[:, 1], -d[:, 0]], 1) / L[:, None]          # наружу для CCW-внешнего кольца
    tu, tv = MATERIALS[mat]["tile"]
    k = np.where(L > tu * 0.35, np.maximum(0.5, np.round(L / tu * 2) / 2), L / tu)
    n = len(p)
    ybv = np.broadcast_to(np.asarray(yb, np.float64), (n,))
    ytv = np.broadcast_to(np.asarray(yt, np.float64), (n,))
    yrv = np.broadcast_to(np.asarray(yref, np.float64), (n,))
    P0 = np.stack([p[:, 0], ybv, p[:, 1]], 1); P1 = np.stack([q[:, 0], ybv, q[:, 1]], 1)
    P2 = np.stack([q[:, 0], ytv, q[:, 1]], 1); P3 = np.stack([p[:, 0], ytv, p[:, 1]], 1)
    v0 = (ybv - yrv) / tv; v1 = (ytv - yrv) / tv
    u0 = np.zeros(n); u1 = -k
    UV0 = np.stack([u0, v0], 1); UV1 = np.stack([u1, v0], 1); UV2 = np.stack([u1, v1], 1); UV3 = np.stack([u0, v1], 1)
    Nw = np.stack([n2[:, 0], np.zeros(n), n2[:, 1]], 1)
    P = np.concatenate([np.stack([P0, P1, P2], 1), np.stack([P0, P2, P3], 1)])
    UV = np.concatenate([np.stack([UV0, UV1, UV2], 1), np.stack([UV0, UV2, UV3], 1)])
    Nn = np.concatenate([Nw, Nw])
    mesh.add(mat, P, np.repeat(Nn[:, None], 3, 1), UV, want=Nn)


def flat_cap(mesh, poly, y, mat, down=False):
    xz = tris_from_polys([poly])
    if len(xz) == 0:
        return
    P = np.stack([xz[..., 0], np.full(xz.shape[:2], y), xz[..., 1]], -1)
    n = -UP if down else UP
    mesh.add(mat, P, np.broadcast_to(n, P.shape), planar_uv(xz, mat), want=np.broadcast_to(n, (len(P), 3)))


def add_quads_tris(mesh, mat, polys3d, uvs, facenormal_hint=None):
    """Полигоны (списки 3D точек, выпуклые) -> треугольники веером, нормаль грани плоская."""
    P, UV, W = [], [], []
    for k, (pts, uv) in enumerate(zip(polys3d, uvs)):
        pts = np.asarray(pts, np.float64); uv = np.asarray(uv, np.float64)
        n = np.cross(pts[1] - pts[0], pts[2] - pts[0])
        if np.linalg.norm(n) < 1e-9 and len(pts) > 3:
            n = np.cross(pts[2] - pts[0], pts[3] - pts[0])
        hint = facenormal_hint[k] if facenormal_hint is not None else n
        if np.dot(n, hint) < 0:
            n = -n
        nn = n / max(np.linalg.norm(n), 1e-9)
        for i in range(1, len(pts) - 1):
            P.append([pts[0], pts[i], pts[i + 1]]); UV.append([uv[0], uv[i], uv[i + 1]]); W.append(nn)
    if P:
        W = np.array(W)
        mesh.add(mat, np.array(P), np.repeat(W[:, None], 3, 1), np.array(UV), want=W)


def pitched_roof(mesh, poly, e, rh, shape, roof_mat, wall_mat, yref, overhang=0.3):
    rect = shapely.minimum_rotated_rectangle(poly)
    co = np.asarray(rect.exterior.coords)[:4]
    e1 = co[1] - co[0]; e2 = co[2] - co[1]
    if np.hypot(*e1) >= np.hypot(*e2):
        a_vec, b_vec = e1, e2
    else:
        a_vec, b_vec = e2, e1
    L = np.hypot(*a_vec) / 2; W = np.hypot(*b_vec) / 2
    if W < 0.5:
        return False
    a = a_vec / (2 * L); b = b_vec / (2 * W)
    c = co.mean(0)
    tu, tv = MATERIALS[roof_mat]["tile"]
    slope = rh / W
    Lo, Wo = L + overhang, W + overhang
    ee = e - overhang * slope
    r = e + rh

    def P(pa, pb, y):
        q = c + a * pa + b * pb
        return np.array([q[0], y, q[1]])

    slope_len = math.hypot(Wo, rh + overhang * slope)
    faces, uvs, hints = [], [], []
    if shape == "gabled" or L - W < 0.3 or shape not in ("hipped", "pyramidal"):
        for sgn in (1, -1):
            faces.append([P(-Lo, sgn * Wo, ee), P(Lo, sgn * Wo, ee), P(Lo, 0, r), P(-Lo, 0, r)])
            uvs.append([(-Lo / tu, 0), (Lo / tu, 0), (Lo / tu, slope_len / tv), (-Lo / tu, slope_len / tv)])
            hints.append(np.array([b[0] * sgn, 1.0, b[1] * sgn]))
        add_quads_tris(mesh, roof_mat, faces, uvs, hints)
        # фронтоны (стеной)
        gf, gu, gh = [], [], []
        twu, twv = MATERIALS[wall_mat]["tile"]
        for sgn in (1, -1):
            gf.append([P(sgn * L, W, e), P(sgn * L, -W, e), P(sgn * L, 0, r)])
            gu.append([(0, (e - yref) / twv), (-2 * W / twu, (e - yref) / twv), (-W / twu, (r - yref) / twv)])
            gh.append(np.array([a[0] * sgn, 0, a[1] * sgn]))
        add_quads_tris(mesh, wall_mat, gf, gu, gh)
    else:
        ridge = max(L - W, 0.0) if shape == "hipped" else 0.0
        for sgn in (1, -1):
            faces.append([P(-Lo, sgn * Wo, ee), P(Lo, sgn * Wo, ee), P(ridge, 0, r), P(-ridge, 0, r)])
            uvs.append([(-Lo / tu, 0), (Lo / tu, 0), (ridge / tu, slope_len / tv), (-ridge / tu, slope_len / tv)])
            hints.append(np.array([b[0] * sgn, 1.0, b[1] * sgn]))
            faces.append([P(sgn * Lo, Wo, ee), P(sgn * Lo, -Wo, ee), P(sgn * ridge, 0, r)])
            uvs.append([(-Wo / tu, 0), (Wo / tu, 0), (0, slope_len / tv)])
            hints.append(np.array([a[0] * sgn, 1.0, a[1] * sgn]))
        add_quads_tris(mesh, roof_mat, faces, uvs, hints)
    return True


def dome_roof(mesh, poly, e, rh, shape, mat, segs=20, rings=8):
    c = poly.centroid
    R = max(math.sqrt(poly.area / math.pi), 0.5)
    rh = rh or (R * (1.6 if shape == "onion" else 1.0))
    prof = []
    for k in range(rings + 1):
        t = k / rings
        if shape == "onion":
            rad = R * (1.0 + 0.35 * math.sin(t * math.pi * 0.9)) * (1 - t) ** 0.8
        else:
            rad = R * math.cos(t * math.pi / 2)
        y = e + rh * (math.sin(t * math.pi / 2) if shape != "onion" else t)
        prof.append((rad, y))
    tu, tv = MATERIALS[mat]["tile"]
    P, UV, W = [], [], []
    for k in range(rings):
        (r0, y0), (r1, y1) = prof[k], prof[k + 1]
        for s in range(segs):
            a0 = 2 * math.pi * s / segs; a1 = 2 * math.pi * (s + 1) / segs
            q = [(c.x + r0 * math.cos(a0), y0, c.y + r0 * math.sin(a0)), (c.x + r0 * math.cos(a1), y0, c.y + r0 * math.sin(a1)),
                 (c.x + r1 * math.cos(a1), y1, c.y + r1 * math.sin(a1)), (c.x + r1 * math.cos(a0), y1, c.y + r1 * math.sin(a0))]
            am = (a0 + a1) / 2
            w = np.array([math.cos(am), (r0 - r1) / max(y1 - y0, 1e-3), math.sin(am)])
            uv = [(s / segs * 2 * math.pi * R / tu, y0 / tv), ((s + 1) / segs * 2 * math.pi * R / tu, y0 / tv),
                  ((s + 1) / segs * 2 * math.pi * R / tu, y1 / tv), (s / segs * 2 * math.pi * R / tu, y1 / tv)]
            P += [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]; UV += [[uv[0], uv[1], uv[2]], [uv[0], uv[2], uv[3]]]
            W += [w, w]
    W = np.array(W); W /= np.linalg.norm(W, axis=1, keepdims=True)
    mesh.add(mat, np.array(P), np.repeat(W[:, None], 3, 1), np.array(UV), want=W)


def building(mesh, terr, geom, t, is_part=False):
    geom = shapely.make_valid(geom)
    polys = [p for p in shapely.get_parts(geom) if p.geom_type == "Polygon" and p.area > 6]
    if not polys:
        return 0
    area = sum(p.area for p in polys)
    rp = geom.representative_point()
    st = building_style(t, area, rp.x, rp.y)
    ext = np.concatenate([np.asarray(p.exterior.coords)[:, :2] for p in polys])
    g = terr.height(ext[:, 0], ext[:, 1])
    gmin, gref = float(g.min()), float(np.mean(g))
    fh = st["floor_h"]
    height = num(t.get("height"))
    rh = num(t.get("roof:height"))
    if rh is None and num(t.get("roof:levels")):
        rh = num(t.get("roof:levels")) * fh
    minh = num(t.get("min_height"))
    if minh is None and num(t.get("building:min_level")) is not None:
        minh = num(t.get("building:min_level")) * fh
    shape = st["shape"]
    if height is None:
        wall_h = st["levels"] * fh + (0.6 if shape == "flat" else 0.0)
        top = wall_h
    else:
        top = height
    b = t.get("building")
    if b == "roof":                                    # навес: только плита
        yt = gref + (top if height else 3.5)
        for p in polys:
            flat_cap(mesh, p, yt, "roof_flat")
            flat_cap(mesh, p, yt - 0.3, "concrete", down=True)
            ring_walls(mesh, shapely.orient_polygons(p).exterior, yt - 0.3, yt, yt - 0.3, "concrete")
        return 1
    if rh is None:
        rect = shapely.minimum_rotated_rectangle(geom)
        rc = np.asarray(rect.exterior.coords)
        sides = np.hypot(*(rc[1] - rc[0])), np.hypot(*(rc[2] - rc[1]))
        short = min(sides)
        rh = {"gabled": 0.35, "hipped": 0.35, "pyramidal": 0.45, "dome": 0.5, "onion": 0.9, "cone": 0.8}.get(shape, 0) * short
        rh = min(rh, 6.0) if shape in ("gabled", "hipped") else rh
    if height is not None and shape != "flat":
        eave = max(top - rh, 2.0)
    else:
        eave = top if shape == "flat" else top
    yb = gref + minh if minh else gmin - 0.5
    ye = gref + eave
    wall_mat = st["facade"]
    for p in polys:
        p = shapely.orient_polygons(p)
        ring_walls(mesh, p.exterior, yb, ye, gref if not minh else yb, wall_mat)
        for hole in p.interiors:
            ring_walls(mesh, hole, yb, ye, gref if not minh else yb, wall_mat)
        if minh:
            flat_cap(mesh, p, yb, "concrete", down=True)
    rect_like = len(polys) == 1 and not polys[0].interiors and \
        polys[0].area / max(shapely.minimum_rotated_rectangle(polys[0]).area, 1e-6) > 0.8
    done = False
    if shape in ("gabled", "hipped", "pyramidal", "half-hipped", "gambrel", "mansard", "skillion") and rect_like and rh > 0.3:
        s = "gabled" if shape in ("gabled", "gambrel", "skillion") else ("pyramidal" if shape == "pyramidal" else "hipped")
        done = pitched_roof(mesh, polys[0], ye, rh, s, st["roof_mat"], wall_mat, gref)
    elif shape in ("dome", "onion", "cone") and len(polys) == 1:
        for p in polys:
            flat_cap(mesh, p, ye, "roof_flat")
        dome_roof(mesh, polys[0], ye, rh, "onion" if shape == "onion" else "dome", st["roof_mat"])
        done = True
    if not done:
        for p in polys:
            flat_cap(mesh, p, ye, "roof_flat")
    return 1


# ------------------------------------------------------------------ протяжки (ж/д, мосты, заборы)

def densify(co, step):
    out = [co[0]]
    for p, q in zip(co[:-1], co[1:]):
        L = np.hypot(*(q - p))
        m = max(1, int(math.ceil(L / step)))
        for s in range(1, m + 1):
            out.append(p + (q - p) * s / m)
    return np.array(out)


def frames(pts):
    t = np.gradient(pts, axis=0)
    t /= np.maximum(np.linalg.norm(t, axis=1, keepdims=True), 1e-9)
    side = np.stack([t[:, 1], -t[:, 0]], 1)
    return t, side


def sweep(mesh, pts, base_y, profile, seg_mask, terr, vtile_mat=None):
    """pts: (n,2) ось; base_y: (n,) высота оси; profile: список сегментов
    [(mat, [(off0, dy0, ground0), (off1, dy1, ground1)], (u0, u1))] — ground=True: y = рельеф+dy.
    v = пройденное расстояние / тайл материала. seg_mask: (n-1,) какие сегменты строить."""
    if len(pts) < 2 or not seg_mask.any():
        return
    t, side = frames(pts)
    dist = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(pts, axis=0).T))])
    for mat, ((o0, dy0, g0), (o1, dy1, g1)), (u0, u1) in profile:
        tv = MATERIALS[mat]["tile"][1]
        A = pts + side * o0; B = pts + side * o1
        ya = (terr.height(A[:, 0], A[:, 1]) if g0 else base_y) + dy0
        yb = (terr.height(B[:, 0], B[:, 1]) if g1 else base_y) + dy1
        PA = np.stack([A[:, 0], ya, A[:, 1]], 1); PB = np.stack([B[:, 0], yb, B[:, 1]], 1)
        k = np.where(seg_mask)[0]
        a0, a1, b0, b1 = PA[k], PA[k + 1], PB[k], PB[k + 1]
        v0 = dist[k] / tv; v1 = dist[k + 1] / tv
        # желаемая нормаль: (-dy, do) в плоскости профиля
        do = o1 - o0
        dyy = ((yb - ya)[k] + (yb - ya)[k + 1]) / 2
        s3 = np.stack([side[k, 0], np.zeros(len(k)), side[k, 1]], 1)
        want = s3 * (-dyy)[:, None] + UP[None] * do
        want /= np.maximum(np.linalg.norm(want, axis=1, keepdims=True), 1e-9)
        P = np.concatenate([np.stack([a0, b0, b1], 1), np.stack([a0, b1, a1], 1)])
        UV = np.concatenate([np.stack([np.stack([np.full_like(v0, u0), v0], 1), np.stack([np.full_like(v0, u1), v0], 1),
                                       np.stack([np.full_like(v0, u1), v1], 1)], 1),
                             np.stack([np.stack([np.full_like(v0, u0), v0], 1), np.stack([np.full_like(v0, u1), v1], 1),
                                       np.stack([np.full_like(v0, u0), v1], 1)], 1)])
        W = np.concatenate([want, want])
        mesh.add(mat, P, np.repeat(W[:, None], 3, 1), UV, want=W)


def seg_in_chunk(pts, box):
    mid = (pts[:-1] + pts[1:]) / 2
    return (mid[:, 0] >= box[0]) & (mid[:, 0] < box[2]) & (mid[:, 1] >= box[1]) & (mid[:, 1] < box[3])


def rail_profiles(top_dy, ballast=True):
    prof = []
    if ballast:
        prof += [("ballast", [(-2.2, -0.1, True), (-1.6, top_dy, False)], (0.0, 0.136)),
                 ("ballast", [(-1.6, top_dy, False), (1.6, top_dy, False)], (0.136, 0.864)),
                 ("ballast", [(1.6, top_dy, False), (2.2, -0.1, True)], (0.864, 1.0))]
    rh = 0.18 if ballast else 0.02
    for c in (-0.76, 0.76):
        prof += [("rail_steel", [(c - 0.036, top_dy, False), (c - 0.036, top_dy + rh, False)], (0.3, 0.7)),
                 ("rail_steel", [(c - 0.036, top_dy + rh, False), (c + 0.036, top_dy + rh, False)], (0.0, 0.2)),
                 ("rail_steel", [(c + 0.036, top_dy + rh, False), (c + 0.036, top_dy, False)], (0.3, 0.7))]
    return prof


def bridge_profile(w, top_mat, rail=False):
    hw = w / 2
    ut = MATERIALS[top_mat]["tile"][0]
    pr = [("concrete", [(-hw, -1.3, False), (-hw, 1.1, False)], (0, 0.6)),
          ("concrete", [(-hw, 1.1, False), (-hw + 0.3, 1.1, False)], (0, 0.1)),
          ("concrete", [(-hw + 0.3, 1.1, False), (-hw + 0.3, 0.0, False)], (0, 0.3)),
          (top_mat, [(-hw + 0.3, 0.0, False), (hw - 0.3, 0.0, False)],
           ((0.0, 1.0) if rail else ((-hw + 0.3) / ut, (hw - 0.3) / ut))),
          ("concrete", [(hw - 0.3, 0.0, False), (hw - 0.3, 1.1, False)], (0, 0.3)),
          ("concrete", [(hw - 0.3, 1.1, False), (hw, 1.1, False)], (0, 0.1)),
          ("concrete", [(hw, 1.1, False), (hw, -1.3, False)], (0, 0.6)),
          ("concrete", [(hw, -1.3, False), (-hw, -1.3, False)], (0, w / 4))]
    if rail:
        pr += rail_profiles(0.0, ballast=False)
    return pr


def box_mesh(mesh, mat, cx, cz, ax, half_a, half_b, y0, y1):
    a = np.array(ax) / max(np.hypot(*ax), 1e-9)
    b = np.array([a[1], -a[0]])
    c = np.array([cx, cz])
    corners = [c + a * sa * half_a + b * sb * half_b for sa, sb in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    faces, uvs, hints = [], [], []
    tu, tv = MATERIALS[mat]["tile"]
    for i in range(4):
        p, q = corners[i], corners[(i + 1) % 4]
        mid = (p + q) / 2 - c
        faces.append([(p[0], y0, p[1]), (q[0], y0, q[1]), (q[0], y1, q[1]), (p[0], y1, p[1])])
        L = np.hypot(*(q - p))
        uvs.append([(0, y0 / tv), (L / tu, y0 / tv), (L / tu, y1 / tv), (0, y1 / tv)])
        hints.append(np.array([mid[0], 0, mid[1]]))
    add_quads_tris(mesh, mat, faces, uvs, hints)


BARRIER = {"wall": ("concrete_fence", 2.2, 0.3), "city_wall": ("wall_brick_plain", 4.0, 0.8),
           "retaining_wall": ("concrete", 1.2, 0.4), "fence": ("fence_metal", 1.8, 0.06),
           "hedge": ("hedge", 1.3, 0.8), "guard_rail": ("rail_steel", 0.75, 0.05)}


def barrier(mesh, terr, line, t, box):
    kind = t.get("barrier")
    if kind not in BARRIER:
        return
    mat, hgt, thick = BARRIER[kind]
    if kind == "wall" and t.get("material") in ("brick", "stone"):
        mat = "wall_brick_plain"
    hgt = num(t.get("height"), hgt)
    co = densify(np.asarray(line.coords)[:, :2], 6.0)
    m = seg_in_chunk(co, box)
    if not m.any():
        return
    ht = MATERIALS[mat]["tile"]
    base = terr.height(co[:, 0], co[:, 1]) - 0.2
    ut = thick / ht[0]
    prof = [(mat, [(-thick / 2, 0, False), (-thick / 2, hgt + 0.2, False)], (0, 1)),
            (mat, [(-thick / 2, hgt + 0.2, False), (thick / 2, hgt + 0.2, False)], (0, ut)),
            (mat, [(thick / 2, hgt + 0.2, False), (thick / 2, 0, False)], (1, 0))]
    # v у стен — вдоль линии, поэтому для заборов меняем местами оси: u = расстояние, v = высота
    t_, side = frames(co)
    dist = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(co, axis=0).T))])
    k = np.where(m)[0]
    for o, sgn in ((-thick / 2, -1), (thick / 2, 1)):
        A = co + side * o
        a0 = np.stack([A[k, 0], base[k], A[k, 1]], 1); a1 = np.stack([A[k + 1, 0], base[k + 1], A[k + 1, 1]], 1)
        b0 = a0 + [0, hgt + 0.2, 0]; b1 = a1 + [0, hgt + 0.2, 0]
        u0 = dist[k] / ht[0]; u1 = dist[k + 1] / ht[0]
        v0 = np.full(len(k), -0.2 / ht[1]); v1 = np.full(len(k), hgt / ht[1])
        w = np.stack([side[k, 0] * sgn, np.zeros(len(k)), side[k, 1] * sgn], 1)
        P = np.concatenate([np.stack([a0, a1, b1], 1), np.stack([a0, b1, b0], 1)])
        UV = np.concatenate([np.stack([np.stack([u0, v0], 1), np.stack([u1, v0], 1), np.stack([u1, v1], 1)], 1),
                             np.stack([np.stack([u0, v0], 1), np.stack([u1, v1], 1), np.stack([u0, v1], 1)], 1)])
        W = np.concatenate([w, w])
        mesh.add(mat, P, np.repeat(W[:, None], 3, 1), UV, want=W)
    if thick > 0.1:
        sweep(mesh, co, base, [prof[1]], m, terr)


# ------------------------------------------------------------------ фонари

_LAMP = None


def lamp_model():
    """models/street_lamp.obj -> {материал: (P, N, UV)} треугольниками."""
    global _LAMP
    if _LAMP is None:
        V, VT, VN, out, mat = [], [], [], defaultdict(lambda: ([], [], [])), None
        for line in open(C.OUT / "models" / "street_lamp.obj", encoding="utf-8"):
            if line.startswith("v "):
                V.append([float(x) for x in line.split()[1:4]])
            elif line.startswith("vt "):
                VT.append([float(x) for x in line.split()[1:3]])
            elif line.startswith("vn "):
                VN.append([float(x) for x in line.split()[1:4]])
            elif line.startswith("usemtl"):
                mat = line.split()[1]
            elif line.startswith("f "):
                idx = [[int(i) - 1 for i in c.split("/")] for c in line.split()[1:4]]
                P, N, T = out[mat]
                P.append([V[i[0]] for i in idx]); T.append([VT[i[1]] for i in idx]); N.append([VN[i[2]] for i in idx])
        _LAMP = {m: (np.array(P), np.array(N), np.array(T)) for m, (P, N, T) in out.items()}
    return _LAMP


def bake_lamp(mesh, x, y, z, yaw_deg, scale=1.0):
    """Впекает фонарный столб в меш чанка; возвращает мировую точку света.
    yaw — поворот вокруг +Y: +X модели (кронштейн) -> (cos, 0, -sin)."""
    th = math.radians(yaw_deg)
    c, sn = math.cos(th), math.sin(th)
    R = np.array([[c, 0.0, sn], [0.0, 1.0, 0.0], [-sn, 0.0, c]])
    off = np.array([x, y, z])
    for mat, (P, N, UV) in lamp_model().items():
        mesh.add(mat, P * scale @ R.T + off, N @ R.T, UV)
    return np.array(LAMP_LIGHT) * scale @ R.T + off


def yaw_towards(dx, dz):
    return math.degrees(math.atan2(-dz, dx))


# ------------------------------------------------------------------ суперплитка

class Ctx:
    pass


def prepare_super(sx, sz, terr, bridges):
    ss = C.SUPER * C.CHUNK
    items = pickle.load(open(C.WORK / "bins" / f"st_{sx}_{sz}.pkl", "rb"))
    terr.window(sx * ss - 400, sz * ss - 400, (sx + 1) * ss + 400, (sz + 1) * ss + 400)
    ctx = Ctx()
    ctx.ground = []          # (prio, mat, geom)
    ctx.buildings = []       # (geom, tags, is_part, rp)
    ctx.prisms = []
    ctx.rails = []           # (line, tags)
    ctx.barriers = []
    ctx.markings = []        # (line, tags, width)
    ctx.lamps_lines = []     # (line, width, kind, spacing, both_sides, scale)
    ctx.road_lines = []      # все проезжие оси — чтобы развернуть фонари из OSM к дороге
    ctx.trees = []           # (x, z, model)
    ctx.tree_rows = []
    ctx.forests = []         # (geom, leaf_type)
    ctx.parks = []
    expanded = []
    for kind, t, w in items:
        g = swkb.loads(w)
        if kind == "line":
            # обрезка по суперплитке может разбить линию на несколько кусков
            for part in shapely.get_parts(g):
                if part.geom_type == "LineString" and part.length > 0.1:
                    expanded.append((kind, t, part))
        else:
            expanded.append((kind, t, g))
    for kind, t, g in expanded:
        if kind in ("building", "part"):
            ctx.buildings.append((g, t, kind == "part"))
        elif kind == "prism":
            ctx.prisms.append((g, t))
        elif kind == "node":
            if t.get("natural") == "tree":
                ctx.trees.append((g.x, g.y, "tree_pine" if t.get("leaf_type") == "needleleaved" else "tree_deciduous"))
            elif t.get("highway") == "street_lamp":
                ctx.trees.append((g.x, g.y, "street_lamp"))
        elif kind == "area":
            cls = area_class(t)
            if cls:
                if cls[1] == "water":
                    g = g.buffer(0)
                ctx.ground.append((cls[0], cls[1], g))
            if t.get("landuse") == "forest" or t.get("natural") == "wood":
                ctx.forests.append((g, t.get("leaf_type")))
            if t.get("leisure") in ("park", "garden") or t.get("landuse") in ("cemetery", "village_green", "recreation_ground"):
                ctx.parks.append(g)
        elif kind == "line":
            hw, rw, ww = t.get("highway"), t.get("railway"), t.get("waterway")
            if hw:
                w_ = road_width(t, hw)
                if w_ is None:
                    continue
                if hw in ROAD_W:
                    mat = surface_mat(t.get("surface"), "asphalt")
                    prio = P_ROAD if mat in ("asphalt", "concrete", "paving") else P_DIRTPATH
                    ctx.ground.append((prio, mat, g.buffer(w_ / 2, quad_segs=3)))
                    sw = t.get("sidewalk", "both" if hw in ("primary", "secondary", "tertiary", "trunk") else None)
                    if sw in ("both", "left", "right", "yes"):
                        ctx.ground.append((P_PAVE, "paving", g.buffer(w_ / 2 + 2.5, quad_segs=3, cap_style="flat")))
                    lanes = num(t.get("lanes"), 2 if hw in ("primary", "secondary", "tertiary", "trunk", "motorway") else 1)
                    if lanes >= 2 and hw not in ("service", "living_street", "track") and t.get("oneway") not in ("yes", "1", "-1") \
                            and mat == "asphalt":
                        ctx.markings.append((g, lanes))
                    ctx.road_lines.append(g)
                    lit = t.get("lit")
                    base_hw = hw.replace("_link", "")
                    if lit != "no" and base_hw in ("motorway", "trunk", "primary", "secondary", "tertiary"):
                        # магистрали: LED, на широких — с обеих сторон вразнобой
                        ctx.lamps_lines.append((g, w_, "led", 32.0, w_ >= 13.0, 1.0))
                    elif lit != "no" and hw in ("residential", "unclassified", "living_street"):
                        ctx.lamps_lines.append((g, w_, "sodium", 40.0, False, 1.0))
                    elif lit == "yes":
                        ctx.lamps_lines.append((g, w_, "sodium", 30.0, False, 1.0))
                elif hw == "track":
                    ctx.ground.append((P_DIRTPATH, surface_mat(t.get("surface"), "dirt"), g.buffer(w_ / 2, quad_segs=2)))
                else:
                    default = {"path": "dirt", "cycleway": "asphalt", "steps": "concrete", "bridleway": "dirt"}.get(hw, "paving")
                    if t.get("lit") == "yes" and hw in ("footway", "pedestrian", "path", "cycleway"):
                        # освещённые аллеи/дорожки: тот же столб, но ~5 м
                        ctx.lamps_lines.append((g, w_, "park", 25.0, False, 0.55))
                    mat = surface_mat(t.get("surface"), default)
                    ctx.ground.append((P_PAVE if mat in ("paving", "asphalt", "concrete") else P_DIRTPATH, mat,
                                       g.buffer(w_ / 2, quad_segs=2)))
            elif rw:
                if rw == "platform":
                    ctx.ground.append((P_PAVE, "paving", g.buffer(2.0, quad_segs=2)))
                    continue
                ctx.rails.append((g, t))
                if rw != "tram":
                    ctx.ground.append((P_RAILBED, None, g.buffer(2.2, quad_segs=2, cap_style="flat")))
            elif ww:
                w_ = num(t.get("width"), {"river": 15, "canal": 8, "stream": 3, "ditch": 1.5, "drain": 1.5}.get(ww, 2))
                ctx.ground.append((P_WATER, "water", g.buffer(w_ / 2, quad_segs=2)))
            elif t.get("barrier"):
                ctx.barriers.append((g, t))
            elif t.get("natural") == "tree_row":
                ctx.tree_rows.append(g)
    ctx.ground.sort(key=lambda r: r[0])
    ctx.road_tree = shapely.STRtree(ctx.road_lines) if ctx.road_lines else None
    ctx.ground_tree = shapely.STRtree([r[2] for r in ctx.ground]) if ctx.ground else None
    ctx.bridges = [b for b in bridges if b[1][:, 0].max() > sx * ss - 64 and b[1][:, 0].min() < (sx + 1) * ss + 64
                   and b[1][:, 2].max() > sz * ss - 64 and b[1][:, 2].min() < (sz + 1) * ss + 64]
    ctx.bld_tree = shapely.STRtree([b[0] for b in ctx.buildings]) if ctx.buildings else None
    # здания, у которых есть части: контур не рисуем
    parts = [b for b in ctx.buildings if b[2]]
    ctx.skip_outline = set()
    if parts:
        pts = [p[0].representative_point() for p in parts]
        for i, (g, t, is_part) in enumerate(ctx.buildings):
            if not is_part and "building" in t:
                if any(g.contains(p) for p in pts):
                    ctx.skip_outline.add(i)
    return ctx


def build_chunk(gx, gz, ctx, terr):
    x0, z0 = gx * C.CHUNK, gz * C.CHUNK
    x1, z1 = x0 + C.CHUNK, z0 + C.CHUNK
    box = shapely.box(x0, z0, x1, z1)
    boxt = (x0, z0, x1, z1)
    mesh = Mesh()
    props = []
    grid_tris = terr.grid_tris(x0, z0, x1, z1)

    # --- земля: разбиение по приоритетам
    taken = None
    classes = {}
    if ctx.ground_tree is not None:
        idx = ctx.ground_tree.query(box)
        byp = defaultdict(list)
        for i in sorted(idx):
            prio, mat, g = ctx.ground[i]
            byp[(prio, mat)].append(g)
        for (prio, mat) in sorted(byp, key=lambda k: (k[0], str(k[1]))):
            try:
                g = shapely.intersection(safe_union(byp[(prio, mat)]), box)
            except Exception:
                continue
            if g is None or g.is_empty:
                continue
            gg = safe_diff(g, taken) if taken is not None else g
            taken = g if taken is None else safe_union([taken, g])
            if mat is not None and gg is not None and not gg.is_empty:
                classes[mat] = safe_union([classes[mat], gg]) if mat in classes else gg
    rest = safe_diff(box, taken) if taken is not None else box
    if rest is not None and not rest.is_empty:
        classes["grass"] = safe_union([classes["grass"], rest]) if "grass" in classes else rest
    for mat, g in classes.items():
        try:
            drape(mesh, terr, mat, g, grid_tris, water=(mat == "water"))
        except Exception as e:
            print(f"[warn] chunk {gx},{gz} ground {mat}: {e}", flush=True)

    # --- разметка (осевая), поверх асфальта
    for line, lanes in ctx.markings:
        if not line.intersects(box):
            continue
        L = line.length
        if L < 30:
            continue
        cut = shapely.ops.substring(line, 12, L - 12) if L > 30 else None
        if cut is None or cut.is_empty or cut.geom_type != "LineString":
            continue
        if lanes >= 4:
            geo = shapely.union_all([cut.offset_curve(0.12), cut.offset_curve(-0.12)]).buffer(0.06, cap_style="flat")
        else:
            dashes = [shapely.ops.substring(cut, s, min(s + 3, cut.length)) for s in np.arange(0, cut.length, 9.0)]
            geo = shapely.union_all([d.buffer(0.07, cap_style="flat") for d in dashes if d.length > 0.5])
        geo = shapely.intersection(geo, box)
        if "asphalt" in classes:
            geo = shapely.intersection(geo, classes["asphalt"])
        drape(mesh, terr, "road_marking", geo, grid_tris, dy=0.02)

    # --- здания
    if ctx.bld_tree is not None:
        for i in ctx.bld_tree.query(box):
            g, t, is_part = ctx.buildings[i]
            if i in ctx.skip_outline:
                continue
            rp = g.representative_point()
            if not (x0 <= rp.x < x1 and z0 <= rp.y < z1):
                continue
            try:
                building(mesh, terr, g, t, is_part)
            except Exception as e:
                print(f"[warn] building @ {rp.x:.0f},{rp.y:.0f}: {e}", flush=True)

    # --- платформы / пирсы
    for g, t in ctx.prisms:
        rp = g.representative_point()
        if not (x0 <= rp.x < x1 and z0 <= rp.y < z1):
            continue
        polys = [p for p in shapely.get_parts(shapely.make_valid(g)) if p.geom_type == "Polygon"]
        for p in polys:
            p = shapely.orient_polygons(p)
            ext = np.asarray(p.exterior.coords)
            gnd = terr.height(ext[:, 0], ext[:, 1])
            if t.get("man_made") == "pier":
                lv = terr.water_level(ext[:, 0], ext[:, 1])
                base = float(np.nanmin(lv)) if not np.all(np.isnan(lv)) else float(gnd.min())
                yb, yt = base - 1.5, base + 1.2
            else:
                yb, yt = float(gnd.min()) - 0.3, float(gnd.mean()) + 1.1
            ring_walls(mesh, p.exterior, yb, yt, yb, "concrete")
            flat_cap(mesh, p, yt, "paving")

    # --- ж/д пути на земле
    for line, t in ctx.rails:
        if not line.intersects(box):
            continue
        co = densify(np.asarray(line.coords)[:, :2], 4.0)
        m = seg_in_chunk(co, boxt)
        if not m.any():
            continue
        g = terr.height(co[:, 0], co[:, 1])
        if t.get("railway") == "tram":
            sweep(mesh, co, g + 0.005, rail_profiles(0.0, ballast=False), m, terr)
        else:
            gs = np.convolve(np.pad(g, 3, mode="edge"), np.ones(7) / 7, mode="valid")   # сгладить продольный профиль
            sweep(mesh, co, gs, rail_profiles(0.3), m, terr)

    # --- мосты
    for tags, pts3 in ctx.bridges:
        co = pts3[:, [0, 2]]
        m = seg_in_chunk(co, boxt)
        if not m.any():
            continue
        y = pts3[:, 1]
        rw = tags.get("railway")
        hw = tags.get("highway")
        if rw:
            w = 5.0; top = "ballast"; prof = bridge_profile(w, top, rail=True)
        else:
            w = (road_width(tags, hw) or 6.0) + 0.6
            top = "asphalt" if hw in ROAD_W else surface_mat(tags.get("surface"), "paving")
            prof = bridge_profile(w, top)
        sweep(mesh, co, y, prof, m, terr)
        # опоры каждые ~30 м
        dist = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(co, axis=0).T))])
        marks = np.where(np.diff(np.floor(dist / 30.0)) > 0)[0] + 1
        t_, side = frames(co)
        for k in marks:
            if k >= len(co) - 1 or not (x0 <= co[k, 0] < x1 and z0 <= co[k, 1] < z1):
                continue
            gnd = float(terr.height(co[k, 0], co[k, 1]))
            lv = float(terr.water_level(co[k, 0], co[k, 1]))
            bottom = min(gnd, lv - 2.0) if not math.isnan(lv) else gnd - 1.0
            if y[k] - 1.3 - bottom > 2.0:
                box_mesh(mesh, "concrete", co[k, 0], co[k, 1], t_[k], 0.8, max(w * 0.3, 1.0), bottom, y[k] - 1.3)

    # --- заборы/стены
    for line, t in ctx.barriers:
        if line.intersects(box):
            try:
                barrier(mesh, terr, line, t, boxt)
            except Exception as e:
                print(f"[warn] barrier: {e}", flush=True)

    # --- инстансы: деревья, фонари
    def inside(x, z):
        return x0 <= x < x1 and z0 <= z < z1

    lights = []
    lamp_pts = []
    for x, z, model in ctx.trees:
        if not inside(x, z):
            continue
        if model == "street_lamp":
            # фонарь из OSM: разворачиваем кронштейн к ближайшей дороге
            yaw = h01(x, z, 5) * 360
            if ctx.road_tree is not None:
                pt = shapely.Point(x, z)
                line = ctx.road_lines[ctx.road_tree.nearest(pt)]
                q = line.interpolate(line.project(pt))
                dx, dz = q.x - x, q.y - z
                if 0.3 < math.hypot(dx, dz) < 40:
                    yaw = yaw_towards(dx, dz)
            y = float(terr.height(x, z)) - 0.05
            lx, ly, lz = bake_lamp(mesh, x, y, z, yaw)
            lights.append((lx, ly, lz, "led"))
            lamp_pts.append((x, z))
            continue
        props.append((model, x, float(terr.height(x, z)), z, h01(x, z, 5) * 360, 0.8 + 0.5 * h01(x, z, 6)))
    for line in ctx.tree_rows:
        if not line.intersects(box):
            continue
        for d in np.arange(3.0, line.length, 7.0):
            p = line.interpolate(d)
            if inside(p.x, p.y):
                props.append(("tree_deciduous", p.x, float(terr.height(p.x, p.y)), p.y, h01(p.x, p.y, 5) * 360, 0.8 + 0.5 * h01(p.x, p.y, 6)))
    blocked = safe_union([classes.get(k) for k in ("asphalt", "paving", "water", "concrete", "sand", "tartan", "pitch", "gravel")])
    if blocked is not None:
        shapely.prepare(blocked)

    def scatter(geom, spacing, keep, salt):
        g = shapely.intersection(geom, box)
        if g.is_empty:
            return np.zeros((0, 2))
        gx_ = np.arange(x0 + spacing / 2, x1, spacing); gz_ = np.arange(z0 + spacing / 2, z1, spacing)
        X, Z = np.meshgrid(gx_, gz_)
        X = X.ravel(); Z = Z.ravel()
        jx = np.array([h01(a, b, salt) for a, b in zip(X, Z)]) - 0.5
        jz = np.array([h01(a, b, salt + 1) for a, b in zip(X, Z)]) - 0.5
        X = X + jx * spacing * 0.8; Z = Z + jz * spacing * 0.8
        k = np.array([h01(a, b, salt + 2) for a, b in zip(X, Z)]) < keep
        X, Z = X[k], Z[k]
        m = shapely.contains_xy(g, X, Z)
        if blocked is not None and len(X):
            m &= ~shapely.contains_xy(blocked, X, Z)
        return np.stack([X[m], Z[m]], 1)

    for g, leaf in ctx.forests:
        if not g.intersects(box):
            continue
        pts = scatter(g, 7.0, 1.0, 11)
        for x, z in pts:
            if leaf == "needleleaved":
                model = "tree_pine"
            elif leaf == "broadleaved":
                model = "tree_deciduous"
            else:
                model = "tree_pine" if h01(x, z, 7) < 0.45 else "tree_deciduous"
            props.append((model, x, float(terr.height(x, z)), z, h01(x, z, 5) * 360, 0.75 + 0.6 * h01(x, z, 6)))
    for g in ctx.parks:
        if not g.intersects(box):
            continue
        for x, z in scatter(g, 12.0, 0.6, 21):
            props.append(("tree_deciduous", x, float(terr.height(x, z)), z, h01(x, z, 5) * 360, 0.8 + 0.5 * h01(x, z, 6)))
    bl = None
    if ctx.bld_tree is not None:
        bl = ctx.bld_tree
    road_surface = classes.get("asphalt")
    if road_surface is not None:
        road_surface = road_surface.buffer(-0.3)
        shapely.prepare(road_surface)
    for line, w, kind, spacing, both, scale in ctx.lamps_lines:
        if not line.intersects(box):
            continue
        L = line.length
        for side in ((1, -1) if both else (1,)):
            start = 15.0 if side == 1 else 15.0 + spacing / 2      # вторая сторона — вразнобой
            for d in np.arange(start, L, spacing):
                p = line.interpolate(d)
                q = line.interpolate(min(d + 1.0, L))
                tx, tz = q.x - p.x, q.y - p.y
                n = math.hypot(tx, tz)
                if n < 1e-6:
                    continue
                tx, tz = tx / n, tz / n
                sx_, sz_ = tz * side, -tx * side                  # наружу от оси дороги
                off = w / 2 + 0.8 * scale
                lx, lz = p.x + sx_ * off, p.y + sz_ * off
                if not inside(lx, lz):
                    continue
                if any((lx - a) ** 2 + (lz - b) ** 2 < 100.0 for a, b in lamp_pts):
                    continue
                if bl is not None and len(bl.query(shapely.Point(lx, lz), predicate="intersects")):
                    continue
                if "water" in classes and classes["water"].contains(shapely.Point(lx, lz)):
                    continue
                # отступ от края одной улицы у перекрёстка может попасть на проезжую часть другой
                if road_surface is not None and shapely.contains_xy(road_surface, lx, lz):
                    continue
                y = float(terr.height(lx, lz)) - 0.05
                light = bake_lamp(mesh, lx, y, lz, yaw_towards(-sx_, -sz_), scale)
                lights.append((light[0], light[1], light[2], kind))
                lamp_pts.append((lx, lz))
    return mesh, props, lights


def work(args):
    sx, sz, chunks = args
    t0 = time.time()
    terr = Terrain()
    bridges = pickle.load(open(C.WORK / "bridges3d.pkl", "rb"))
    ctx = prepare_super(sx, sz, terr, bridges)
    out = []
    for gx, gz in chunks:
        mesh, props, lights = build_chunk(gx, gz, ctx, terr)
        name = f"chunk_{gx}_{gz}"
        stats = mesh.write(C.OUT / "chunks" / f"{name}.obj",
                           f"# Самара, чанк {gx},{gz}: X {gx*C.CHUNK:.0f}..{(gx+1)*C.CHUNK:.0f}, Z {gz*C.CHUNK:.0f}..{(gz+1)*C.CHUNK:.0f}\n"
                           f"# Данные: (c) OpenStreetMap contributors (ODbL); рельеф: Copernicus DEM GLO-30 (c) DLR/Airbus/ESA")
        if props:
            with open(C.OUT / "props" / f"{name}.csv", "w", newline="\n") as f:
                f.write("model,x,y,z,yaw_deg,scale\n")
                for m, x, y, z, yaw, s in props:
                    f.write(f"{m},{x:.2f},{y:.2f},{z:.2f},{yaw:.1f},{s:.2f}\n")
        if lights:
            with open(C.OUT / "lights" / f"{name}.csv", "w", newline="\n") as f:
                f.write("x,y,z,kind\n")
                for x, y, z, kind in lights:
                    f.write(f"{x:.2f},{y:.2f},{z:.2f},{kind}\n")
        out.append({"gx": gx, "gz": gz, "tris": stats, "props": len(props), "lamps": len(lights)})
    return (sx, sz, out, time.time() - t0)


def main():
    region = pickle.load(open(C.WORK / "region.pkl", "rb"))
    chunks = region["chunks"]
    only = None
    if len(sys.argv) > 1:
        # отладка: python build_chunks.py sx,sz [sx,sz ...]
        only = {tuple(int(v) for v in a.split(",")) for a in sys.argv[1:]}
    (C.OUT / "chunks").mkdir(parents=True, exist_ok=True)
    (C.OUT / "props").mkdir(parents=True, exist_ok=True)
    (C.OUT / "lights").mkdir(parents=True, exist_ok=True)
    by_super = defaultdict(list)
    for c in chunks:
        by_super[C.super_of_chunk(*c)].append(c)
    jobs = [(sx, sz, sorted(v)) for (sx, sz), v in sorted(by_super.items()) if only is None or (sx, sz) in only]
    # сначала плотный город: суперплитки ближе к центру
    jobs.sort(key=lambda j: j[0] ** 2 + j[1] ** 2)
    t0 = time.time()
    results = []
    done_chunks = 0
    total = sum(len(j[2]) for j in jobs)
    with Pool(C.WORKERS if only is None else 1, maxtasksperchild=8) as pool:
        for sx, sz, out, dt in pool.imap_unordered(work, jobs):
            results += out
            done_chunks += len(out)
            el = time.time() - t0
            print(f"[chunks] st {sx},{sz}: {len(out)} чанков за {dt:.0f}s | {done_chunks}/{total} "
                  f"| прошло {el/60:.1f} мин, осталось ~{el/done_chunks*(total-done_chunks)/60:.0f} мин", flush=True)
    if only is None:
        json.dump(results, open(C.WORK / "chunk_stats.json", "w"))
    print(f"[chunks] готово: {done_chunks} чанков за {(time.time()-t0)/60:.1f} мин")


if __name__ == "__main__":
    main()
