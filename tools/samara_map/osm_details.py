"""Детали улиц из OSM, запекаемые в геометрию чанков (вызывается из build_chunks).

Принцип — минимальная погрешность:
  * ПОЗИЦИЯ каждого объекта берётся из OSM как есть (узел перехода, светофора,
    подъезда, опоры ЛЭП, вывески — там, где он в данных);
  * ОРИЕНТАЦИЯ — из окружающей геометрии OSM: касательная ближайшей дороги,
    сегмент стены, на котором лежит узел подъезда, соседние пролёты ЛЭП;
  * РАЗМЕРЫ — из тегов (width, height, step_count, cables, voltage, diameter),
    а если их нет — по ГОСТ/типовым значениям (они указаны у констант);
  * чего нет в данных — не выдумывается (никаких случайных качелей на площадках).

Объекты запекаются в OBJ чанка (как фонари), а не в props/*.csv: эдитор
импортирует только геометрию чанков.

Координаты: X — восток, Y — вверх, Z — юг. В плоскости (x, z) «правая» сторона
для направления t = (tx, tz) — r = (-tz, tx) (едешь на восток — справа юг);
у shapely это ЛЕВАЯ сторона (offset_curve(+d) смещает к r).
"""
import json
import math
from collections import defaultdict

import numpy as np
import shapely

import config as C
from materials import MATERIALS

UP = np.array([0.0, 1.0, 0.0])

# --- размеры по нормативам / типовые (м)
ZEBRA_STRIPE_W = 0.4        # ГОСТ Р 51256, разметка 1.14.1: полосы 0.4 м
ZEBRA_PITCH = 1.0           # полоса + промежуток (0.6 м)
ZEBRA_LEN = 4.0             # ширина перехода вдоль дороги: 4 м, на магистралях 6 м
SIGN_BOTTOM = 2.0           # ГОСТ Р 52289: низ знака над тротуаром — не ниже 2 м
SIGN_SIZE = 0.7             # типоразмер II в городе: 700 мм
TROLLEY_H = 5.8             # контактный провод троллейбуса над проезжей частью
TROLLEY_SPACING = 0.6       # расстояние между «+» и «−» проводами
TRAM_WIRE_H = 6.0
CATENARY_POLE_STEP = 35.0   # шаг опор контактной сети
STEP_RISE = 0.16            # высота ступени, если step_count нет
SIGN_TEXT_H = 0.45          # высота букв вывески
WIRE_R = 0.007              # контактный провод МФ-100: Ø ~12 мм

# какие виды пропсов из точек OSM ставятся внутри зданий — их не рисуем
INDOOR_OK = {"atm", "vending_machine", "telephone", "parcel_locker"}


def _h(terr, x, z):
    return float(terr.height(x, z))


def _unit(v):
    n = np.linalg.norm(v)
    return v / n if n > 1e-9 else v


def h01(x, z, salt=0):
    v = (int(x * 7.3) * 73856093) ^ (int(z * 7.3) * 19349663) ^ (salt * 83492791)
    v = (v ^ (v >> 13)) * 1274126177 & 0xFFFFFFFF
    return (v & 0xFFFFFF) / float(0x1000000)


def num(s, default=None):
    if s is None:
        return default
    try:
        s = str(s).replace(",", ".").split(";")[0].strip().split()[0].rstrip("m")
        return float(s)
    except Exception:
        return default


# ------------------------------------------------------------------ геометрия

class Geo:
    """Накопитель треугольников по материалам -> один Mesh.add на материал."""

    def __init__(self):
        self.d = defaultdict(lambda: ([], [], []))

    def tri(self, mat, p, n, uv):
        P, N, T = self.d[mat]
        P.append(p); N.append(n); T.append(uv)

    def quad(self, mat, a, b, c, d, n, uv=((0, 0), (1, 0), (1, 1), (0, 1))):
        a, b, c, d = (np.asarray(v, np.float64) for v in (a, b, c, d))
        n = np.asarray(n, np.float64)
        nn = (n, n, n)
        self.tri(mat, (a, b, c), nn, (uv[0], uv[1], uv[2]))
        self.tri(mat, (a, c, d), nn, (uv[0], uv[2], uv[3]))

    def box(self, mat, c, ax, ay, az, hx, hy, hz, face_uv=None):
        """Ориентированный параллелепипед: центр c, оси ax/ay/az (единичные), полуразмеры.
        UV — в метрах / тайл материала; face_uv=(ось, знак) — эта грань получает UV 0..1
        (картинка знака/двери целиком)."""
        c = np.asarray(c, np.float64)
        axes = [np.asarray(a, np.float64) for a in (ax, ay, az)]
        half = (hx, hy, hz)
        tile = MATERIALS[mat]["tile"] or (1.0, 1.0)
        for k in range(3):
            i, j = (k + 1) % 3, (k + 2) % 3
            for s in (1, -1):
                n = axes[k] * s
                fc = c + n * half[k]
                ei, ej = axes[i] * half[i], axes[j] * half[j]
                a_, b_, c_, d_ = fc - ei - ej, fc + ei - ej, fc + ei + ej, fc - ei + ej
                if face_uv == (k, s):
                    uv = ((0, 0), (1, 0), (1, 1), (0, 1))
                    # картинка не зеркалится: u растёт вправо, если смотреть на грань снаружи
                    if np.dot(np.cross(axes[i], axes[j]), n) < 0:
                        uv = ((1, 0), (0, 0), (0, 1), (1, 1))
                else:
                    ui, vj = 2 * half[i] / tile[0], 2 * half[j] / tile[1]
                    uv = ((0, 0), (ui, 0), (ui, vj), (0, vj))
                self.quad(mat, a_, b_, c_, d_, n, uv)

    def beam(self, mat, p0, p1, w, h=None, up=UP):
        """Брус прямоугольного сечения w x h между точками p0 и p1."""
        p0 = np.asarray(p0, np.float64); p1 = np.asarray(p1, np.float64)
        ax = p1 - p0
        L = np.linalg.norm(ax)
        if L < 1e-4:
            return
        ax /= L
        ref = up if abs(np.dot(ax, up)) < 0.95 else np.array([1.0, 0, 0])
        az = _unit(np.cross(ax, ref))
        ay = np.cross(az, ax)
        self.box(mat, (p0 + p1) / 2, ax, ay, az, L / 2, (h or w) / 2, w / 2)

    def cyl(self, mat, p0, p1, r0, r1=None, n=8, caps=False):
        p0 = np.asarray(p0, np.float64); p1 = np.asarray(p1, np.float64)
        r1 = r0 if r1 is None else r1
        ax = p1 - p0
        L = np.linalg.norm(ax)
        if L < 1e-4:
            return
        ax /= L
        ref = UP if abs(np.dot(ax, UP)) < 0.95 else np.array([1.0, 0, 0])
        e1 = _unit(np.cross(ax, ref)); e2 = np.cross(ax, e1)
        tile = MATERIALS[mat]["tile"] or (1.0, 1.0)
        ang = np.linspace(0, 2 * np.pi, n + 1)
        dirs = [np.cos(a) * e1 + np.sin(a) * e2 for a in ang]
        circ = 2 * np.pi * max(r0, r1)
        for k in range(n):
            d0, d1 = dirs[k], dirs[k + 1]
            a, b = p0 + d0 * r0, p0 + d1 * r0
            c_, d_ = p1 + d1 * r1, p1 + d0 * r1
            u0, u1 = circ * k / n / tile[0], circ * (k + 1) / n / tile[0]
            v1 = L / tile[1]
            nm0, nm1 = d0, d1
            self.tri(mat, (a, b, c_), (nm0, nm1, nm1), ((u0, 0), (u1, 0), (u1, v1)))
            self.tri(mat, (a, c_, d_), (nm0, nm1, nm0), ((u0, 0), (u1, v1), (u0, v1)))
        if caps:
            for pc, r, s in ((p0, r0, -1), (p1, r1, 1)):
                if r <= 0:
                    continue
                for k in range(n):
                    a, b = pc + dirs[k] * r, pc + dirs[k + 1] * r
                    nn = ax * s
                    self.tri(mat, (pc, a, b), (nn, nn, nn), ((0.5, 0.5), (0.5, 0), (1, 0.5)))

    def tube(self, mat, pts, r, n=3):
        """Провод: трубка по ломаной (без торцов)."""
        pts = np.asarray(pts, np.float64)
        for a, b in zip(pts[:-1], pts[1:]):
            self.cyl(mat, a, b, r, r, n)

    def flush(self, mesh):
        for mat, (P, N, T) in self.d.items():
            if P:
                mesh.add(mat, np.array(P), np.array(N), np.array(T))
        self.d.clear()


def frame_t(t):
    """t — единичное направление в плоскости (x, z) -> 3D-оси (вперёд, вправо)."""
    f = np.array([t[0], 0.0, t[1]])
    r = np.array([-t[1], 0.0, t[0]])
    return f, r


# ------------------------------------------------------------------ текст вывесок

_GLYPHS = None


def glyphs():
    global _GLYPHS
    if _GLYPHS is None:
        _GLYPHS = json.loads((C.OUT / "textures" / "glyphs.json").read_text(encoding="utf-8"))["chars"]
    return _GLYPHS


def text_width(text):
    g = glyphs()
    return sum(g.get(ch, g["?"])["w"] for ch in text)


def text_quads(geo, mat, text, origin, right, up, normal, height):
    """Строка текста квадами из атласа: origin — левый нижний угол строки."""
    g = glyphs()
    x = 0.0
    for ch in text:
        m = g.get(ch, g["?"])
        w = m["w"] * height
        if ch != " ":
            a = origin + right * x
            b = a + right * w
            geo.quad(mat, a, b, b + up * height, a + up * height, normal,
                     ((m["u0"], m["v0"]), (m["u1"], m["v0"]), (m["u1"], m["v1"]), (m["u0"], m["v1"])))
        x += w
    return x


# ------------------------------------------------------------------ подготовка суперплитки

class Det:
    pass


def _bucket(points, items):
    """Точечные объекты -> по чанкам."""
    out = defaultdict(list)
    for (x, z), it in zip(points, items):
        out[C.chunk_of(x, z)].append(it)
    return out


_POWER = None


def power_lines():
    """ЛЭП целиком (global.pkl) — один раз на процесс."""
    global _POWER
    if _POWER is None:
        import pickle
        from shapely import wkb as swkb
        g = pickle.load(open(C.WORK / "global.pkl", "rb"))
        _POWER = [(t, swkb.loads(w)) for t, w in g.get("power_lines", [])]
    return _POWER


def prepare(expanded, ctx, window):
    """Собирает всё нужное деталям из признаков суперплитки (kind, tags, geom)."""
    from build_chunks import ROAD_W, road_width
    d = Det()
    d.nodes = []                 # (x, z, tags)
    d.roads = []                 # проезжие: (line, w, tags)
    d.ways = []                  # все highway-линии: (line, w, tags) — для ориентации скамеек и т.п.
    d.foot_crossings = []        # footway=crossing — точное направление пешеходного перехода
    d.trolley = []               # (line, w, tags)
    d.trams = []                 # линии трамвая
    d.kerbs = []
    d.steps = []
    d.turn_roads = []            # дороги с turn:lanes
    d.platform_areas = []        # (poly, tags)
    d.shelter_areas = []
    d.fountain_areas = []
    d.memorial_areas = []
    d.scrub_areas = []
    d.tree_rows = list(ctx.tree_rows)
    for kind, t, g in expanded:
        if kind == "node":
            d.nodes.append((g.x, g.y, t))
        elif kind == "line":
            hw = t.get("highway")
            if hw:
                w = road_width(t, hw) or 2.0
                d.ways.append((g, w, t))
                if hw in ROAD_W:
                    d.roads.append((g, w, t))
                    if t.get("trolley_wire") == "yes":
                        d.trolley.append((g, w, t))
                    if any(k in t for k in ("turn:lanes", "turn:lanes:forward", "turn:lanes:backward")):
                        d.turn_roads.append((g, w, t))
                if t.get("footway") == "crossing" or hw == "crossing":
                    d.foot_crossings.append(g)
                if hw == "steps":
                    d.steps.append((g, t))
            if t.get("railway") == "tram":
                d.trams.append((g, t))
            if t.get("barrier") == "kerb":
                d.kerbs.append(g)
        elif kind == "area":
            if t.get("public_transport") == "platform" and t.get("railway") != "platform":
                d.platform_areas.append((g, t))
            if t.get("amenity") == "shelter":
                d.shelter_areas.append((g, t))
            if t.get("amenity") == "fountain":
                d.fountain_areas.append((g, t))
            if t.get("historic") in ("memorial", "monument") or t.get("tourism") == "artwork":
                d.memorial_areas.append((g, t))
            if t.get("natural") == "scrub":
                d.scrub_areas.append(g)
    d.road_tree = shapely.STRtree([r[0] for r in d.roads]) if d.roads else None
    d.way_tree = shapely.STRtree([r[0] for r in d.ways]) if d.ways else None
    d.foot_tree = shapely.STRtree(d.foot_crossings) if d.foot_crossings else None
    d.tram_tree = shapely.STRtree([g for g, _ in d.trams]) if d.trams else None
    d.barrier_tree = shapely.STRtree([b[0] for b in ctx.barriers]) if ctx.barriers else None
    d.node_by_chunk = _bucket([(x, z) for x, z, _ in d.nodes], d.nodes)
    # здания по индексу ctx.buildings — для вывесок/подъездов/досок
    d.bld_ids = {}
    # ЛЭП, задевающие окно суперплитки
    win = shapely.box(*window)
    d.power = [(t, g) for t, g in power_lines() if g.intersects(win)]
    sup = defaultdict(str)
    for x, z, t in d.nodes:
        if t.get("power") in ("tower", "pole", "portal", "catenary_mast"):
            sup[(round(x, 1), round(z, 1))] = t["power"]
    d.supports = sup
    d.wall_used = defaultdict(list)    # (здание, сегмент) -> занятые интервалы вывесок
    d.placed = []                      # уже поставленные столбы/павильоны — от дублей
    return d


# ------------------------------------------------------------------ поиск по геометрии

def nearest_line(tree, items, x, z, maxd):
    """Ближайшая линия из items (кортежи, линия первым элементом) в пределах maxd."""
    if tree is None:
        return None
    p = shapely.Point(x, z)
    i = tree.nearest(p)
    if i is None:
        return None
    it = items[i]
    line = it[0] if isinstance(it, tuple) else it
    if line.distance(p) > maxd:
        return None
    return it


def tangent_at(line, x, z):
    """Единичная касательная линии в ближайшей к (x, z) точке (по ходу геометрии)."""
    p = shapely.Point(x, z)
    s = line.project(p)
    a = line.interpolate(max(s - 1.0, 0.0))
    b = line.interpolate(min(s + 1.0, line.length))
    t = np.array([b.x - a.x, b.y - a.y])
    n = np.linalg.norm(t)
    return t / n if n > 1e-9 else np.array([1.0, 0.0]), s


def facing_way(d, x, z, maxd=30.0):
    """Направление (единичное, в плоскости) от точки к ближайшей дороге/дорожке."""
    it = nearest_line(d.way_tree, d.ways, x, z, maxd)
    if it is None:
        return None
    q = it[0].interpolate(it[0].project(shapely.Point(x, z)))
    v = np.array([q.x - x, q.y - z])
    n = np.linalg.norm(v)
    return v / n if n > 0.05 else None


def _face_or(d, x, z, default=(1.0, 0.0)):
    """Куда смотрит объект: к ближайшей дороге/дорожке, иначе — default."""
    f = facing_way(d, x, z)
    return f if f is not None else np.array(default)


def inside_building(ctx, x, z):
    if ctx.bld_tree is None:
        return False
    p = shapely.Point(x, z)
    return len(ctx.bld_tree.query(p, predicate="intersects")) > 0


def wall_at(ctx, x, z, maxd):
    """Ближайший сегмент стены здания: (индекс здания, № сегмента, a, b, наружная нормаль, расстояние)."""
    if ctx.bld_tree is None:
        return None
    p = shapely.Point(x, z)
    best = None
    for i in ctx.bld_tree.query(p.buffer(maxd)):
        g = ctx.buildings[i][0]
        for poly in shapely.get_parts(g):
            if poly.geom_type != "Polygon":
                continue
            co = np.asarray(poly.exterior.coords)
            for k in range(len(co) - 1):
                a, b = co[k], co[k + 1]
                ab = b - a
                L2 = float(ab @ ab)
                if L2 < 1e-6:
                    continue
                u = np.clip(((x - a[0]) * ab[0] + (z - a[1]) * ab[1]) / L2, 0, 1)
                q = a + ab * u
                dist = math.hypot(x - q[0], z - q[1])
                if dist <= maxd and (best is None or dist < best[5]):
                    t = ab / math.sqrt(L2)
                    n = np.array([t[1], -t[0]])
                    mid = (a + b) / 2
                    if poly.contains(shapely.Point(mid[0] + n[0] * 0.05, mid[1] + n[1] * 0.05)):
                        n = -n
                    best = (i, k, a, b, n, dist)
    return best


# ------------------------------------------------------------------ дорожная обстановка

def _road_at(d, x, z, maxd=4.0):
    it = nearest_line(d.road_tree, d.roads, x, z, maxd)
    if it is None:
        return None
    line, w, t = it
    tv, s = tangent_at(line, x, z)
    return line, w, t, tv, s


def _oneway(t):
    v = t.get("oneway")
    if v in ("yes", "1", "true"):
        return 1
    if v == "-1":
        return -1
    if t.get("junction") == "roundabout" or t.get("highway") in ("motorway", "motorway_link"):
        return 1
    return 0


def _is_marked(t):
    c, m = t.get("crossing"), t.get("crossing:markings")
    if c in ("unmarked", "no", "informal") or m == "no":
        return False
    return c in ("marked", "zebra", "traffic_signals", "uncontrolled", "pelican", "toucan") or (m not in (None, "no"))


def _sign(geo, terr, x, z, face, mat, bottom=SIGN_BOTTOM, size=SIGN_SIZE, two_sided=False, pole=True):
    """Знак на стойке: face — куда смотрит лицевая сторона (единичный вектор в плоскости)."""
    g = _h(terr, x, z)
    f = np.array([face[0], 0.0, face[1]])
    right = np.cross(UP, f)            # вправо, если смотреть на лицо знака
    if pole:
        geo.cyl("steel_grey", (x, g - 0.3, z), (x, g + bottom + size, z), 0.03, n=6)
    c = np.array([x, g + bottom + size / 2, z]) + f * 0.04
    geo.box(mat, c, -right, UP, f, size / 2, size / 2, 0.01, face_uv=(2, 1))
    if two_sided:
        geo.box(mat, c - f * 0.025, right, UP, -f, size / 2, size / 2, 0.01, face_uv=(2, 1))
    else:
        geo.box("sign_back", c - f * 0.021, right, UP, -f, size / 2, size / 2, 0.001)


def crossings(env):
    """«Зебры» (1.14.1) по узлам highway=crossing + знаки 5.19 у нерегулируемых."""
    d, terr, geo = env.d, env.terr, env.geo
    asphalt = env.classes.get("asphalt")
    stripes = []
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if t.get("highway") != "crossing" or not _is_marked(t):
            continue
        ra = _road_at(d, x, z)
        if ra is None:
            continue
        line, w, rt, tv, _ = ra
        cw = 6.0 if rt.get("highway", "").replace("_link", "") in ("trunk", "primary", "motorway") else ZEBRA_LEN
        r = np.array([-tv[1], tv[0]])
        # точное направление пешеходного пути — из footway=crossing, если он нарисован
        p = r
        fc = nearest_line(d.foot_tree, d.foot_crossings, x, z, 3.0)
        if fc is not None:
            pv, _ = tangent_at(fc, x, z)
            if abs(pv @ r) > 0.5:
                p = pv if pv @ r > 0 else -pv
        span = (w / 2) / max(abs(p @ r), 0.5)
        for s in np.arange(-span + ZEBRA_STRIPE_W / 2, span, ZEBRA_PITCH):
            a = np.array([x + p[0] * s, z + p[1] * s])
            corners = [a + tv * cw / 2 + r * ZEBRA_STRIPE_W / 2, a - tv * cw / 2 + r * ZEBRA_STRIPE_W / 2,
                       a - tv * cw / 2 - r * ZEBRA_STRIPE_W / 2, a + tv * cw / 2 - r * ZEBRA_STRIPE_W / 2]
            stripes.append(shapely.Polygon(corners))
        if t.get("crossing") != "traffic_signals":
            ow = _oneway(rt)
            for sgn in (1, -1):
                if ow and ow != sgn:
                    continue
                # справа по ходу подъезжающих (+t: справа r), лицом к ним
                sx = x + r[0] * sgn * (w / 2 + 0.6) - tv[0] * sgn * cw / 2
                sz = z + r[1] * sgn * (w / 2 + 0.6) - tv[1] * sgn * cw / 2
                if env.inside(sx, sz) and not inside_building(env.ctx, sx, sz):
                    _sign(geo, terr, sx, sz, -tv * sgn, "sign_crossing")
    if stripes:
        g = shapely.intersection(shapely.union_all(stripes), env.boxg)
        if asphalt is not None:
            g = shapely.intersection(g, asphalt)
        if not g.is_empty:
            env.drape("road_marking", g, 0.025)


def _signal_head(geo, base, face, h_center, vehicle=True):
    """Корпус светофора с тремя линзами (Т.1) или двумя (пешеходный П.1)."""
    f = np.array([face[0], 0.0, face[1]])
    right = np.cross(UP, f)
    n = 3 if vehicle else 2
    hh = 0.15 * n + 0.05
    c = base + UP * h_center
    geo.box("signal_body", c, right, UP, f, 0.16, hh, 0.12)
    for k in range(n):
        lc = c + UP * ((k - (n - 1) / 2) * 0.3) + f * 0.12
        geo.cyl("signal_lens", lc, lc + f * 0.02, 0.1, n=10, caps=True)
        geo.box("signal_body", lc + f * 0.1 + UP * 0.1, right, UP, f, 0.13, 0.01, 0.1)     # козырёк


def _signal_pole(env, x, z, face, ped_face=None):
    for px, pz in env.d.placed:
        if (px - x) ** 2 + (pz - z) ** 2 < 9.0:
            return
    env.d.placed.append((x, z))
    g = _h(env.terr, x, z)
    base = np.array([x, g, z])
    env.geo.cyl("steel_grey", base - UP * 0.3, base + UP * 3.6, 0.06, 0.05, n=8)
    _signal_head(env.geo, base, face, 3.05)
    if ped_face is not None:
        _signal_head(env.geo, base, ped_face, 2.3, vehicle=False)


def traffic_signals(env):
    """Светофоры: на узле перехода/подхода — по столбу справа от каждого подхода,
    в центре перекрёстка — по столбу на каждой ветке, лицом к подъезжающим."""
    d = env.d
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if not (t.get("highway") == "traffic_signals" or t.get("crossing") == "traffic_signals"):
            continue
        p = shapely.Point(x, z)
        arms = []
        if d.road_tree is not None:
            for i in d.road_tree.query(p.buffer(0.5)):
                line, w, rt = d.roads[i]
                co = np.asarray(line.coords)
                k = int(np.argmin(np.hypot(co[:, 0] - x, co[:, 1] - z)))
                if math.hypot(co[k, 0] - x, co[k, 1] - z) > 0.5:
                    continue
                ow = _oneway(rt)
                for nb, along in ((k + 1, 1), (k - 1, -1)):
                    if 0 <= nb < len(co):
                        a = co[nb] - co[k]
                        n = np.linalg.norm(a)
                        # по ветке a к узлу едут, если движение идёт против a
                        if n > 0.1 and (ow == 0 or ow == -along):
                            arms.append((a / n, w))
        if len(arms) >= 3:
            # центр перекрёстка: столб на каждой ветке, справа от подъезжающих
            W = max(w for _, w in arms)
            for a, w in arms:
                dvec = -a
                r = np.array([-dvec[1], dvec[0]])
                sx = x + a[0] * (W / 2 + 2.0) + r[0] * (w / 2 + 0.7)
                sz = z + a[1] * (W / 2 + 2.0) + r[1] * (w / 2 + 0.7)
                if env.inside(sx, sz) and not inside_building(env.ctx, sx, sz):
                    _signal_pole(env, sx, sz, a)
            continue
        ra = _road_at(d, x, z)
        if ra is None:
            continue
        line, w, rt, tv, _ = ra
        r = np.array([-tv[1], tv[0]])
        ow = _oneway(rt)
        dirn = t.get("traffic_signals:direction") or t.get("direction")
        gap = (ZEBRA_LEN / 2 + 1.0) if (t.get("crossing") or t.get("highway") == "crossing") else 1.0
        for sgn in (1, -1):
            if ow and ow != sgn:
                continue
            if (dirn == "forward" and sgn != 1) or (dirn == "backward" and sgn != -1):
                continue
            sx = x + r[0] * sgn * (w / 2 + 0.7) - tv[0] * sgn * gap
            sz = z + r[1] * sgn * (w / 2 + 0.7) - tv[1] * sgn * gap
            if env.inside(sx, sz) and not inside_building(env.ctx, sx, sz):
                ped = (-r * sgn) if t.get("crossing") == "traffic_signals" else None
                _signal_pole(env, sx, sz, -tv * sgn, ped)


def priority_signs(env):
    """2.4 «Уступите дорогу» / 2.5 «STOP»: справа, лицом к подъезжающим к перекрёстку."""
    d = env.d
    for x, z, t in d.node_by_chunk.get(env.key, []):
        hw = t.get("highway")
        if hw not in ("give_way", "stop"):
            continue
        ra = _road_at(d, x, z)
        if ra is None:
            continue
        line, w, rt, tv, s = ra
        dirn = t.get("direction")
        if dirn == "forward":
            sgn = 1
        elif dirn == "backward":
            sgn = -1
        else:
            # знак ставят на второстепенной дороге перед перекрёстком — едут к ближнему концу линии
            sgn = 1 if (line.length - s) < s else -1
        r = np.array([-tv[1], tv[0]]) * sgn
        sx, sz = x + r[0] * (w / 2 + 0.6), z + r[1] * (w / 2 + 0.6)
        _sign(env.geo, env.terr, sx, sz, -tv * sgn, "sign_give_way" if hw == "give_way" else "sign_stop")


def _arrow_polys(kind, length=5.0):
    """Стрелка разметки 1.18 в локальных координатах (x — по ходу, y — вправо)."""
    sw, hl, hw = 0.15, 1.5, 0.45
    if kind in ("through", "none", ""):
        return [[(0, -sw), (length - hl, -sw), (length - hl, -hw), (length, 0), (length - hl, hw), (length - hl, sw), (0, sw)]]
    side = -1 if kind in ("left", "slight_left", "sharp_left", "reverse") else 1
    bend = length * 0.5
    # хвост по ходу, затем изгиб в сторону поворота и наконечник
    shaft = [(0, -sw), (bend + sw, -sw), (bend + sw, sw), (0, sw)]
    arm_end = side * 1.5
    arm = [(bend - sw, 0), (bend + sw, 0), (bend + sw, arm_end), (bend - sw, arm_end)]
    head = [(bend - hw, arm_end), (bend + hw, arm_end), (bend, arm_end + side * hl)]
    return [shaft, arm, head]


def turn_arrows(env):
    """Стрелки полос по turn:lanes перед концом участка (по ходу), каждая — в центре своей полосы."""
    d = env.d
    polys = []
    for line, w, t in d.turn_roads:
        if not line.intersects(env.boxg) or line.length < 25:
            continue
        ow = _oneway(t)
        groups = []
        if ow:
            v = t.get("turn:lanes") or t.get("turn:lanes:forward")
            if v:
                groups.append((v, ow, w))
        else:
            if t.get("turn:lanes:forward"):
                groups.append((t["turn:lanes:forward"], 1, w / 2))
            if t.get("turn:lanes:backward"):
                groups.append((t["turn:lanes:backward"], -1, w / 2))
        for v, sgn, width in groups:
            lanes = v.split("|")
            n = len(lanes)
            s_end = line.length - 8.0 if sgn == 1 else 8.0
            for dist in (s_end, s_end - sgn * 25.0):
                if not (5.0 < dist < line.length - 5.0):
                    continue
                pt = line.interpolate(dist)
                tv, _ = tangent_at(line, pt.x, pt.y)
                fwd = tv * sgn
                right = np.array([-fwd[1], fwd[0]])
                c0 = np.array([pt.x, pt.y])
                for k, lane in enumerate(lanes):
                    # полосы перечислены слева направо по ходу; при двустороннем — правая половина
                    off = (-width / 2 + width * (k + 0.5) / n) if ow else width * (k + 0.5) / n
                    centre = c0 + right * off - fwd * 5.0
                    for kind in (lane.split(";") if lane else []):
                        if kind in ("none", ""):
                            continue
                        for pl in _arrow_polys(kind):
                            pg = shapely.Polygon([centre + fwd * px + right * py for px, py in pl])
                            if pg.is_valid and pg.area > 0.01:
                                polys.append(pg)
    if polys:
        g = shapely.intersection(shapely.union_all(polys), env.boxg)
        asphalt = env.classes.get("asphalt")
        if asphalt is not None:
            g = shapely.intersection(g, asphalt)
        if not g.is_empty:
            env.drape("road_marking", g, 0.025)


def level_crossings(env):
    """Ж/д переезды: настил поперёк путей на ширину дороги."""
    d = env.d
    panels = []
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if t.get("railway") not in ("level_crossing", "crossing"):
            continue
        maxw = 3.0 if t.get("railway") == "level_crossing" else 2.0
        it = nearest_line(d.road_tree if t.get("railway") == "level_crossing" else d.way_tree,
                          d.roads if t.get("railway") == "level_crossing" else d.ways, x, z, maxw)
        if it is None:
            continue
        tv, _ = tangent_at(it[0], x, z)
        w = it[1]
        r = np.array([-tv[1], tv[0]])
        a = np.array([x, z])
        panels.append(shapely.Polygon([a + tv * 2.0 + r * w / 2, a - tv * 2.0 + r * w / 2,
                                       a - tv * 2.0 - r * w / 2, a + tv * 2.0 - r * w / 2]))
    if panels:
        g = shapely.intersection(shapely.union_all(panels), env.boxg)
        if not g.is_empty:
            env.drape("concrete", g, 0.06)


# ------------------------------------------------------------------ остановки

def _shelter(env, cx, cz, along, away, length=4.0, depth=1.6, name=None):
    """Павильон остановки: открыт к дороге (away — от дороги), задняя и боковые стенки стеклянные."""
    for px, pz in env.d.placed:
        if (px - cx) ** 2 + (pz - cz) ** 2 < 100.0:
            return
    env.d.placed.append((cx, cz))
    geo, terr = env.geo, env.terr
    g = _h(terr, cx, cz)
    a = np.array([along[0], 0.0, along[1]])
    b = np.array([away[0], 0.0, away[1]])
    c = np.array([cx, g, cz])
    H = 2.5
    for sa in (-1, 1):
        for sb in (-1, 1):
            p = c + a * sa * (length / 2 - 0.05) + b * sb * (depth / 2 - 0.05)
            geo.box("steel_grey", p + UP * H / 2, a, UP, b, 0.04, H / 2, 0.04)
    geo.box("steel_grey", c + UP * (H + 0.05), a, UP, b, length / 2 + 0.15, 0.06, depth / 2 + 0.2)      # крыша
    geo.box("glass_shelter", c + b * (depth / 2 - 0.05) + UP * 1.3, a, UP, b, length / 2 - 0.1, 1.05, 0.01)  # задняя стенка
    for sa in (-1, 1):
        geo.box("glass_shelter", c + a * sa * (length / 2 - 0.05) + UP * 1.3, b, UP, a, depth / 2 - 0.1, 1.05, 0.01)
    geo.box("wood_planks", c + b * (depth / 2 - 0.35) + UP * 0.45, a, UP, b, length / 2 - 0.4, 0.03, 0.2)    # скамья
    if name:
        # табличка с названием остановки на козырьке, лицом к дороге
        front = -b
        right = np.cross(UP, front)
        th = 0.2
        tw = text_width(name) * th
        tw_max = length - 0.2
        if tw > tw_max:
            th *= tw_max / tw
            tw = tw_max
        bc = c + front * (depth / 2 + 0.22) + UP * (H - 0.12)
        geo.box("sign_board", bc, right, UP, front, tw / 2 + 0.1, th / 2 + 0.06, 0.02)
        text_quads(geo, "glyphs_white", name, bc + front * 0.021 - right * tw / 2 - UP * th / 2, right, UP, front, th)


def stops(env):
    """Платформы (бортик 0.18 м), павильоны (shelter=yes / amenity=shelter), знаки 5.16/5.17 с названием."""
    from build_chunks import ring_walls, flat_cap
    d, geo, terr = env.d, env.geo, env.terr
    done_signs = []
    for poly, t in d.platform_areas:
        rp = poly.representative_point()
        if not env.inside(rp.x, rp.y):
            continue
        for p in shapely.get_parts(poly):
            if p.geom_type != "Polygon" or p.area > 3000:
                continue
            p = shapely.orient_polygons(p)
            ext = np.asarray(p.exterior.coords)
            gnd = terr.height(ext[:, 0], ext[:, 1])
            ring_walls(env.mesh, p.exterior, float(gnd.min()) - 0.1, float(gnd.mean()) + 0.18, float(gnd.min()) - 0.1, "granite")
            flat_cap(env.mesh, p, float(gnd.mean()) + 0.18, "paving")
        if t.get("shelter") == "yes" or t.get("covered") == "yes":
            mrr = shapely.minimum_rotated_rectangle(poly)
            co = np.asarray(mrr.exterior.coords)[:4]
            e1, e2 = co[1] - co[0], co[2] - co[1]
            along = e1 if np.linalg.norm(e1) >= np.linalg.norm(e2) else e2
            L = float(np.linalg.norm(along))
            along = along / max(L, 1e-6)
            to_road = facing_way(d, rp.x, rp.y) if d.road_tree is not None else None
            it = nearest_line(d.road_tree, d.roads, rp.x, rp.y, 40.0)
            if it is not None:
                q = it[0].interpolate(it[0].project(rp))
                to_road = np.array([q.x - rp.x, q.y - rp.y])
                to_road /= max(np.linalg.norm(to_road), 1e-6)
            away = -to_road if to_road is not None else np.array([-along[1], along[0]])
            away = away - along * (away @ along)
            away /= max(np.linalg.norm(away), 1e-6)
            _shelter(env, rp.x, rp.y, along, away, length=min(max(L * 0.7, 2.5), 6.0), name=t.get("name"))
    for poly, t in d.shelter_areas:
        rp = poly.representative_point()
        if env.inside(rp.x, rp.y) and t.get("shelter_type", "public_transport") == "public_transport":
            it = nearest_line(d.road_tree, d.roads, rp.x, rp.y, 30.0)
            if it is not None:
                tv, _ = tangent_at(it[0], rp.x, rp.y)
                q = it[0].interpolate(it[0].project(rp))
                away = np.array([rp.x - q.x, rp.y - q.y]); away /= max(np.linalg.norm(away), 1e-6)
                _shelter(env, rp.x, rp.y, tv, away, name=t.get("name"))
    for x, z, t in d.node_by_chunk.get(env.key, []):
        is_bus = t.get("highway") == "bus_stop" or (t.get("public_transport") == "platform" and (t.get("bus") == "yes" or t.get("trolleybus") == "yes"))
        is_tram = t.get("railway") == "tram_stop" or (t.get("public_transport") == "platform" and t.get("tram") == "yes")
        is_shelter = t.get("amenity") == "shelter" and t.get("shelter_type", "public_transport") == "public_transport"
        if not (is_bus or is_tram or is_shelter):
            continue
        it = nearest_line(d.road_tree, d.roads, x, z, 25.0)
        if is_tram and d.tram_tree is not None:
            tr = nearest_line(d.tram_tree, d.trams, x, z, 25.0)
            if tr is not None:
                it = (tr[0], 2.6, tr[1])
        if it is None:
            continue
        line, w, rt = it
        tv, _ = tangent_at(line, x, z)
        q = line.interpolate(line.project(shapely.Point(x, z)))
        away = np.array([x - q.x, z - q.y])
        dist = np.linalg.norm(away)
        if dist < 0.3:
            # узел стоит на самой оси (stop_position/трамвайный путь) — тротуар справа
            away = np.array([-tv[1], tv[0]])
        else:
            away /= dist
        # знак — у края проезжей части, на линии узла
        sx, sz = q.x + away[0] * (w / 2 + 0.5), q.y + away[1] * (w / 2 + 0.5)
        if any((sx - a) ** 2 + (sz - b) ** 2 < 400.0 for a, b in done_signs):
            continue
        done_signs.append((sx, sz))
        if (is_bus or is_tram) and env.inside(sx, sz) and not inside_building(env.ctx, sx, sz):
            # 5.16/5.17 ставят перпендикулярно дороге, двусторонним
            _sign(geo, terr, sx, sz, tv, "sign_tram_stop" if is_tram and not is_bus else "sign_bus_stop", two_sided=True)
        if t.get("shelter") == "yes" or is_shelter:
            cx, cz = (x, z) if dist >= 1.5 else (q.x + away[0] * (w / 2 + 2.0), q.y + away[1] * (w / 2 + 2.0))
            if env.inside(cx, cz) and not inside_building(env.ctx, cx, cz):
                _shelter(env, cx, cz, tv, away, name=t.get("name"))


# ------------------------------------------------------------------ контактная сеть

def _wire_points(terr, line, off, h, step=4.0):
    """Ломаная провода над дорогой: смещение off вбок (к r), высота h над рельефом."""
    if abs(off) > 1e-3:
        line = line.offset_curve(off, quad_segs=2)
        if line.is_empty or line.geom_type != "LineString":
            return None
    L = line.length
    if L < 1.0:
        return None
    s = np.linspace(0, L, max(2, int(L / step) + 1))
    xy = shapely.get_coordinates(shapely.line_interpolate_point(line, s))
    y = terr.height(xy[:, 0], xy[:, 1]) + h
    return np.column_stack([xy[:, 0], y, xy[:, 1]])


def _emit_wire(env, pts, r=WIRE_R, mat="cable"):
    """Провод — только куски, середина которых в этом чанке (соседний чанк нарисует свои)."""
    if pts is None or len(pts) < 2:
        return
    mids = (pts[:-1] + pts[1:]) / 2
    x0, z0, x1, z1 = env.box
    m = (mids[:, 0] >= x0) & (mids[:, 0] < x1) & (mids[:, 2] >= z0) & (mids[:, 2] < z1)
    for k in np.nonzero(m)[0]:
        env.geo.cyl(mat, pts[k], pts[k + 1], r, r, 3)


def _pole_ok(env, x, z):
    if not env.inside(x, z) or inside_building(env.ctx, x, z):
        return False
    asph = env.classes.get("asphalt")
    return asph is None or not asph.contains(shapely.Point(x, z))


def trolley(env):
    """Троллейбусная контактная сеть: пара проводов над правой полосой каждого направления
    на 5.8 м, опоры по обеим сторонам каждые ~35 м с поперечной растяжкой."""
    terr, geo = env.terr, env.geo
    for line, w, t in env.d.trolley:
        if not line.intersects(env.boxg.buffer(w + 5)):
            continue
        ow = _oneway(t)
        lane = max(w / 2 - 1.8, 1.0) if not ow else max(w / 2 - 1.8, 0.0)
        for sgn in ((ow,) if ow else (1, -1)):
            for dw in (-TROLLEY_SPACING / 2, TROLLEY_SPACING / 2):
                _emit_wire(env, _wire_points(terr, line, sgn * lane + dw, TROLLEY_H))
        L = line.length
        for s in np.arange(CATENARY_POLE_STEP / 2, L, CATENARY_POLE_STEP):
            p = line.interpolate(s)
            tv, _ = tangent_at(line, p.x, p.y)
            r = np.array([-tv[1], tv[0]])
            ends = []
            for sg in (1, -1):
                px, pz = p.x + r[0] * sg * (w / 2 + 0.8), p.y + r[1] * sg * (w / 2 + 0.8)
                gy = _h(terr, px, pz)
                if _pole_ok(env, px, pz):
                    geo.cyl("steel_grey", (px, gy - 0.5, pz), (px, gy + 9.0, pz), 0.13, 0.09, n=8)
                ends.append(np.array([px, gy + 6.6, pz]))
            if env.inside(p.x, p.y):
                geo.cyl("cable", ends[0], ends[1], 0.006, n=3)


def trams(env):
    """Трамвайная контактная сеть: провод над осью пути на 6 м, опоры каждые ~35 м с консолью.
    Если рядом параллельный путь (двухпутка) — общая опора между путями."""
    d, terr, geo = env.d, env.terr, env.geo
    for line, t in d.trams:
        if not line.intersects(env.boxg.buffer(10)):
            continue
        if t.get("electrified") == "no":
            continue
        _emit_wire(env, _wire_points(terr, line, 0.0, TRAM_WIRE_H))
        for s in np.arange(CATENARY_POLE_STEP / 2, line.length, CATENARY_POLE_STEP):
            p = line.interpolate(s)
            if not env.inside(p.x, p.y):
                continue
            tv, _ = tangent_at(line, p.x, p.y)
            r = np.array([-tv[1], tv[0]])
            side, off = None, None
            for sg in (1, -1):
                q = shapely.Point(p.x + r[0] * sg * 4.0, p.y + r[1] * sg * 4.0)
                near = [g for g, _ in (d.trams[i] for i in d.tram_tree.query(q.buffer(1.5))) if g is not line] if d.tram_tree is not None else []
                if near:
                    side, off = sg, 2.0
                    break
            if side is not None:
                if side == -1:
                    continue          # общую опору ставит путь, у которого сосед справа
            else:
                side, off = 1, 2.6
                if not _pole_ok(env, p.x + r[0] * off, p.y + r[1] * off):
                    side = -1
            px, pz = p.x + r[0] * side * off, p.y + r[1] * side * off
            if not _pole_ok(env, px, pz) and off != 2.0:
                continue
            gy = _h(terr, px, pz)
            geo.cyl("steel_grey", (px, gy - 0.5, pz), (px, gy + 7.5, pz), 0.13, 0.09, n=8)
            top = np.array([px, gy + 6.4, pz])
            geo.cyl("steel_grey", top, np.array([p.x, _h(terr, p.x, p.y) + TRAM_WIRE_H + 0.3, p.y]), 0.04, n=4)
            if off == 2.0:     # консоль и к соседнему пути
                q2 = np.array([p.x + r[0] * side * 4.0, p.y + r[1] * side * 4.0])
                geo.cyl("steel_grey", top, np.array([q2[0], _h(terr, *q2) + TRAM_WIRE_H + 0.3, q2[1]]), 0.04, n=4)


# ------------------------------------------------------------------ ЛЭП

def _voltage(t):
    v = t.get("voltage")
    try:
        return max(float(x) for x in str(v).split(";")) if v else None
    except ValueError:
        return None


def _line_params(t):
    """Высота опоры, полуширина траверсы и раскладка проводов по напряжению (типовые опоры)."""
    v = _voltage(t) or (10000 if t.get("power") == "minor_line" else 110000)
    if v >= 400000:
        H, A = 40.0, 13.0
    elif v >= 200000:
        H, A = 32.0, 9.0
    elif v >= 100000:
        H, A = 26.0, 5.5
    elif v >= 30000:
        H, A = 20.0, 3.5
    else:
        H, A = 10.0, 0.9
    try:
        cables = sum(int(x) for x in str(t.get("cables", "")).split(";") if x.strip())
    except ValueError:
        cables = 0
    if cables <= 0:
        cables = 3
    hv = v >= 30000
    phases = []
    if cables >= 6 and H >= 18:
        for k, (lat, dy) in enumerate(((0.8, 4), (1.0, 9), (0.8, 14))):
            phases += [(-A * lat, H - dy), (A * lat, H - dy)]
        phases = phases[:cables]
    elif hv:
        phases = [(-A, H - 4), (A, H - 4), (A * 0.7, H - 9)][:max(cables, 3)]
    else:
        phases = [(-A, H - 0.6), (0.0, H - 0.4), (A, H - 0.6)]
    return dict(H=H, A=A, hv=hv, phases=phases, gw=hv)


def _tower(geo, base, u, v, P):
    """Решётчатая опора: 4 пояса, распорки, раскосы, траверсы, гирлянды изоляторов."""
    H, A = P["H"], P["A"]
    U = np.array([u[0], 0, u[1]]); V = np.array([v[0], 0, v[1]])
    b0, b1 = H * 0.1, 0.6                      # полуширина у земли и у вершины тела
    body_top = H - 5.0
    def corner(su, sv, y):
        k = y / body_top
        hb = b0 + (b1 - b0) * min(k, 1.0)
        return base + U * su * hb + V * sv * hb + UP * y
    cs = ((1, 1), (1, -1), (-1, -1), (-1, 1))
    for su, sv in cs:
        geo.cyl("steel_lattice", corner(su, sv, -0.5), corner(su, sv, H - 1.0), 0.07, n=4)
    levels = np.linspace(0, body_top, max(3, int(body_top / 5) + 1))
    for y0, y1 in zip(levels[:-1], levels[1:]):
        for k in range(4):
            a, b = cs[k], cs[(k + 1) % 4]
            geo.cyl("steel_lattice", corner(*a, y1), corner(*b, y1), 0.035, n=3)
            geo.cyl("steel_lattice", corner(*a, y0), corner(*b, y1), 0.03, n=3)
    arm_ys = sorted({ph[1] for ph in P["phases"]})
    for y in arm_ys:
        span = max(abs(ph[0]) for ph in P["phases"] if ph[1] == y)
        for s in (1, -1):
            geo.cyl("steel_lattice", base + UP * (y + 0.0) + V * s * 0.6, base + UP * y + V * s * span, 0.06, n=4)
            geo.cyl("steel_lattice", base + UP * (y + 2.0) + V * s * 0.6, base + UP * y + V * s * span, 0.04, n=3)
    if P["gw"]:
        geo.cyl("steel_lattice", base + UP * (H - 1.0), base + UP * H, 0.08, n=4)
    for lat, y in P["phases"]:
        top = base + V * lat + UP * y
        geo.cyl("insulator", top, top - UP * 1.6, 0.1, n=6)


def _pole(geo, base, v, P):
    V = np.array([v[0], 0, v[1]])
    H = P["H"]
    geo.cyl("concrete", base - UP * 1.5, base + UP * H, 0.15, 0.1, n=8)
    geo.box("steel_grey", base + UP * (H - 0.7), V, UP, np.cross(V, UP), P["A"] + 0.2, 0.05, 0.05)
    for lat, y in P["phases"]:
        geo.cyl("insulator", base + V * lat + UP * (H - 0.7), base + V * lat + UP * (y + 0.0), 0.04, n=6)


def power(env):
    """ЛЭП: опоры в вершинах линии (тип — из узла power=tower/pole/portal, иначе по напряжению),
    провода между ними цепной линией с провисом ~3% пролёта."""
    d, terr, geo = env.d, env.terr, env.geo
    for t, line in d.power:
        if t.get("power") == "cable" and t.get("location", "underground") != "overhead":
            continue
        if t.get("location") == "underground":
            continue
        P = _line_params(t)
        co = np.asarray(line.coords)[:, :2]
        if len(co) < 2:
            continue
        n = len(co)
        dirs = []
        for k in range(n):
            a = co[max(k - 1, 0)]; b = co[min(k + 1, n - 1)]
            u = b - a
            u = u / max(np.linalg.norm(u), 1e-6)
            dirs.append(u)
        # опоры
        attach = []
        for k in range(n):
            x, z = co[k]
            u = dirs[k]; v = np.array([-u[1], u[0]])
            kind = d.supports.get((round(x, 1), round(z, 1))) or ("pole" if not P["hv"] else "tower")
            g = _h(terr, x, z)
            base = np.array([x, g, z])
            if env.inside(x, z):
                if kind == "pole":
                    _pole(geo, base, v, P)
                else:
                    _tower(geo, base, u, v, P)
            V = np.array([v[0], 0, v[1]])
            drop = 1.6 if kind != "pole" else 0.0
            pts = [base + V * lat + UP * (y - drop) for lat, y in P["phases"]]
            if P["gw"]:
                pts.append(base + UP * P["H"])
            attach.append(pts)
        # провода пролётов
        for k in range(n - 1):
            a_list, b_list = attach[k], attach[k + 1]
            span = float(np.linalg.norm(co[k + 1] - co[k]))
            if span < 1.0:
                continue
            sag = min(max(0.03 * span, 0.3), 15.0)
            m = max(2, int(span / 10.0) + 1)
            s = np.linspace(0, 1, m)[:, None]
            for a, b in zip(a_list, b_list):
                pts = a + (b - a) * s
                pts[:, 1] -= 4 * sag * (s[:, 0] * (1 - s[:, 0]))
                _emit_wire(env, pts, r=0.012 if P["hv"] else 0.006)


# ------------------------------------------------------------------ бордюры, ворота, лестницы

def kerbs(env):
    """barrier=kerb: гранитный бортовой камень 0.18 x 0.15 м над землёй."""
    for line in env.d.kerbs:
        if not line.intersects(env.boxg):
            continue
        pts = _wire_points(env.terr, line, 0.0, 0.0, step=2.0)
        if pts is None:
            continue
        mids = (pts[:-1] + pts[1:]) / 2
        for k in range(len(pts) - 1):
            if env.inside(mids[k, 0], mids[k, 2]):
                env.geo.beam("granite", pts[k] + UP * 0.025, pts[k + 1] + UP * 0.025, 0.18, 0.35)


def barrier_nodes(env):
    """Ворота, шлагбаумы, блоки, столбики — на линии забора вдоль неё, на дороге — поперёк."""
    d, geo, terr = env.d, env.geo, env.terr
    for x, z, t in d.node_by_chunk.get(env.key, []):
        b = t.get("barrier")
        if b not in ("gate", "swing_gate", "lift_gate", "block", "bollard"):
            continue
        g = _h(terr, x, z)
        base = np.array([x, g, z])
        way = nearest_line(d.way_tree, d.ways, x, z, 1.0)
        fence = None
        if env.ctx.barriers and d.barrier_tree is not None:
            i = d.barrier_tree.nearest(shapely.Point(x, z))
            if i is not None and env.ctx.barriers[i][0].distance(shapely.Point(x, z)) < 1.0:
                fence = env.ctx.barriers[i][0]
        if way is not None:
            tv, _ = tangent_at(way[0], x, z)
            across = np.array([-tv[1], tv[0]])
            width = num(t.get("width"), min(way[1] + 0.5, 7.0))
        elif fence is not None:
            across, _ = tangent_at(fence, x, z)
            width = num(t.get("width"), 3.0)
        else:
            across, width = np.array([1.0, 0.0]), num(t.get("width"), 3.0)
        A = np.array([across[0], 0, across[1]])
        F = np.cross(A, UP)
        if b in ("gate", "swing_gate"):
            for s in (-1, 1):
                geo.box("steel_grey", base + A * s * (width / 2 + 0.05) + UP * 1.0, A, UP, F, 0.05, 1.3, 0.05)
            geo.box("fence_metal", base + UP * 1.05, A, UP, F, width / 2, 0.9, 0.02)
        elif b == "lift_gate":
            post = base + A * (width / 2 + 0.3)
            geo.box("steel_grey", post + UP * 0.5, A, UP, F, 0.15, 0.5, 0.15)
            geo.cyl("boom_stripes", post + UP * 0.95, post + UP * 0.95 - A * (width + 0.3), 0.05, n=6, caps=True)
        elif b == "block":
            geo.box("concrete", base + UP * 0.3, A, UP, F, 0.5, 0.3, 0.3)
        else:
            geo.cyl("steel_grey", base - UP * 0.2, base + UP * 0.9, 0.07, n=8, caps=True)


def steps(env):
    """Лестницы: ступени по step_count (иначе по ~16 см подъёма) от высоты начала до конца пути."""
    terr, geo = env.terr, env.geo
    for line, t in env.d.steps:
        if not line.intersects(env.boxg):
            continue
        L = line.length
        if L < 1.0:
            continue
        a, b = line.coords[0], line.coords[-1]
        h0, h1 = _h(terr, *a[:2]), _h(terr, *b[:2])
        dh = h1 - h0
        if abs(dh) < 0.3:
            continue
        n = int(num(t.get("step_count"), 0) or round(abs(dh) / STEP_RISE))
        n = max(2, min(n, 300))
        w = min(max(num(t.get("width"), 2.0), 1.0), 8.0)
        run = L / n
        tops = []
        for i in range(n):
            s = (i + 0.5) * run
            p = line.interpolate(s)
            tv, _ = tangent_at(line, p.x, p.y)
            yt = h0 + dh * (i + 1) / n          # верх i-й ступени (по ходу линии)
            gmin = float(np.min(terr.height(np.array([p.x - tv[1] * w / 2, p.x + tv[1] * w / 2]),
                                             np.array([p.y + tv[0] * w / 2, p.y - tv[0] * w / 2]))))
            tops.append((p, tv, yt))
            if not env.inside(p.x, p.y):
                continue
            yb = min(gmin, yt) - 0.3
            f, r = frame_t(tv)
            c = np.array([p.x, (yt + yb) / 2, p.y])
            geo.box("concrete", c, f, UP, r, run / 2 + 0.01, (yt - yb) / 2, w / 2)
        if t.get("handrail") in ("yes", "both") or t.get("handrail:left") == "yes" or t.get("handrail:right") == "yes":
            for side in (-1, 1):
                pts = [np.array([p.x, yt + 0.9, p.y]) + np.array([-tv[1], 0, tv[0]]) * side * (w / 2 - 0.1) for p, tv, yt in tops]
                _emit_wire(env, np.array(pts), r=0.025, mat="steel_grey")


# ------------------------------------------------------------------ подъезды, вывески

def _bld_info(ctx, i):
    g, t, _ = ctx.buildings[i]
    from build_chunks import building_style
    rp = g.representative_point()
    st = building_style(t, g.area, rp.x, rp.y)
    H = num(t.get("height")) or st["levels"] * st["floor_h"]
    return t, st, H


def entrances(env):
    """Подъезды: дверь на стене в точке узла, над жилыми — козырёк и светильник
    (источник света kind=entrance + плафон, светящийся от него)."""
    ctx, geo, terr = env.ctx, env.geo, env.terr
    for x, z, t in env.d.node_by_chunk.get(env.key, []):
        e = t.get("entrance")
        if e is None or e in ("no", "emergency_ward_entrance"):
            continue
        wa = wall_at(ctx, x, z, 1.5)
        if wa is None:
            continue
        i, k, a, b, n, _ = wa
        bt, st, H = _bld_info(ctx, i)
        tv = (b - a) / np.linalg.norm(b - a)
        T = np.array([tv[0], 0, tv[1]]); N = np.array([n[0], 0, n[1]])
        ab = b - a
        u = np.clip(((x - a[0]) * ab[0] + (z - a[1]) * ab[1]) / (ab @ ab), 0, 1)
        q = a + ab * u
        g = _h(terr, q[0], q[1])
        base = np.array([q[0], g, q[1]])
        garage = e == "garage" or t.get("door") in ("overhead", "garage")
        dw, dh = (2.6, 2.3) if garage else (num(t.get("width"), 1.2), 2.1)
        geo.box("door_metal", base + N * 0.04 + UP * dh / 2, -T, UP, N, dw / 2, dh / 2, 0.03, face_uv=(2, 1))
        residential = (bt.get("building") in ("apartments", "residential", "yes", "dormitory")) and st["levels"] >= 2
        if residential and e in ("staircase", "main", "yes", "home") and not garage and H > 3.2:
            geo.box("concrete", base + N * 0.6 + UP * 2.62, T, UP, N, 0.95, 0.06, 0.6)      # козырёк
            lamp = base + N * 0.12 + UP * 2.4
            geo.box("signal_body", lamp + UP * 0.04, T, UP, N, 0.14, 0.04, 0.07)
            geo.box("entrance_lamp_glass", lamp - UP * 0.025, T, UP, N, 0.13, 0.025, 0.06)
            env.lights.append((lamp[0], lamp[1] - 0.06, lamp[2], "entrance"))


_COLORS = ["white", "yellow", "red", "green", "blue"]


def _sign_color(t, name):
    a, s = t.get("amenity"), t.get("shop")
    if a == "pharmacy":
        return "green"
    if a in ("bank", "atm", "bureau_de_change", "money_transfer"):
        return "blue"
    if a in ("fast_food", "cafe", "restaurant", "bar", "pub", "ice_cream"):
        return "yellow" if h01(len(name), 3) < 0.5 else "red"
    if s in ("supermarket", "convenience", "alcohol"):
        return "red" if hash(name) % 2 else "white"
    return "white" if hash(name) % 3 else "yellow"


def _road_dist(d, x, z):
    it = nearest_line(d.way_tree, d.ways, x, z, 80.0)
    return it[0].distance(shapely.Point(x, z)) if it is not None else 80.0


def shop_signs(env):
    """Вывески: название из OSM (name/brand), на стене своего здания, обращённой к улице,
    над первым этажом; светящиеся буквы. Киоски без здания — отдельный ларёк."""
    from extract_osm import SIGN_AMENITY
    ctx, d, geo, terr = env.ctx, env.d, env.geo, env.terr
    pois = []
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if t.get("shop") or t.get("office") or t.get("craft") or t.get("amenity") in SIGN_AMENITY:
            pois.append((x, z, t, None))
    for i, (g, t, _) in env.bld_here:
        if t.get("shop") or t.get("amenity") in SIGN_AMENITY:
            rp = g.representative_point()
            pois.append((rp.x, rp.y, t, i))
    for x, z, t, own in pois:
        name = (t.get("name") or t.get("brand") or "").strip()
        if not name:
            continue
        if len(name) > 30:
            name = name[:29] + "…"
        if own is None:
            wa = wall_at(ctx, x, z, 25.0)
            if wa is None:
                if t.get("shop") == "kiosk":
                    _kiosk(env, x, z, name, t)
                continue
            bi = wa[0]
        else:
            bi = own
        g, bt, _ = ctx.buildings[bi]
        # стена: ближе к улице и к самому магазину
        best = None
        for poly in shapely.get_parts(g):
            if poly.geom_type != "Polygon":
                continue
            co = np.asarray(poly.exterior.coords)
            for k in range(len(co) - 1):
                a, b = co[k], co[k + 1]
                L = float(np.linalg.norm(b - a))
                if L < 2.5:
                    continue
                mid = (a + b) / 2
                score = _road_dist(d, *mid) + 0.7 * math.hypot(mid[0] - x, mid[1] - z)
                if best is None or score < best[0]:
                    tv = (b - a) / L
                    nrm = np.array([tv[1], -tv[0]])
                    if poly.contains(shapely.Point(mid[0] + nrm[0] * 0.05, mid[1] + nrm[1] * 0.05)):
                        nrm = -nrm
                    best = (score, k, a, b, L, tv, nrm)
        if best is None:
            continue
        _, k, a, b, L, tv, nrm = best
        th = SIGN_TEXT_H
        tw = text_width(name) * th
        if tw + 0.4 > L - 0.4:
            th = max((L - 0.8) / max(text_width(name), 1e-3), 0.0)
            tw = text_width(name) * th
        if th < 0.18:
            continue
        sl = tw + 0.4
        # центр — проекция магазина на стену, сдвиг до свободного места
        u = float(np.clip((np.array([x, z]) - a) @ tv, sl / 2 + 0.2, L - sl / 2 - 0.2))
        used = d.wall_used[(bi, k)]
        def free(c):
            return all(c + sl / 2 <= lo - 0.3 or c - sl / 2 >= hi + 0.3 for lo, hi in used)
        cand = [u] + [u + s * m for m in np.arange(0.5, L, 0.5) for s in (1, -1)]
        pos = next((c for c in cand if sl / 2 + 0.2 <= c <= L - sl / 2 - 0.2 and free(c)), None)
        if pos is None:
            continue
        used.append((pos - sl / 2, pos + sl / 2))
        bt_, st, H = _bld_info(ctx, bi)
        q = a + tv * pos
        gq = _h(terr, q[0], q[1])
        lvl = num(t.get("level"), 0) or 0
        y_off = (3.4 if st["levels"] >= 2 else max(H - 0.8, 2.4)) + max(lvl, 0) * st["floor_h"]
        y_off = min(y_off, H - th / 2 - 0.2) if H > 2.5 else y_off
        T = np.array([tv[0], 0, tv[1]]); N = np.array([nrm[0], 0, nrm[1]])
        right = np.cross(UP, N)          # вправо, если смотреть на вывеску снаружи
        c = np.array([q[0], gq + y_off, q[1]]) + N * 0.08
        geo.box("sign_board", c, right, UP, N, sl / 2, th / 2 + 0.08, 0.06)
        text_quads(geo, f"glyphs_{_sign_color(t, name)}", name, c + N * 0.061 - right * tw / 2 - UP * th / 2, right, UP, N, th)


def _kiosk(env, x, z, name, t):
    geo, terr = env.geo, env.terr
    face = _face_or(env.d, x, z)
    N = np.array([face[0], 0, face[1]]); R = np.cross(UP, N)
    g = _h(terr, x, z)
    base = np.array([x, g, z])
    geo.box("steel_grey", base + UP * 1.3, R, UP, N, 1.3, 1.3, 1.0)
    geo.box("glass_shelter", base + N * 1.01 + UP * 1.4, R, UP, N, 1.1, 0.7, 0.01)
    th = min(0.35, 2.4 / max(text_width(name), 1e-3))
    tw = text_width(name) * th
    c = base + UP * 2.85 + N * 1.0
    geo.box("sign_board", c, R, UP, N, max(tw / 2 + 0.1, 1.3), th / 2 + 0.06, 0.04)
    text_quads(geo, f"glyphs_{_sign_color(t, name)}", name, c + N * 0.041 - R * tw / 2 - UP * th / 2, R, UP, N, th)


# ------------------------------------------------------------------ памятники, фонтаны

def memorials(env):
    """Памятники/мемориалы/арт-объекты: доски — на стену, бюсты/статуи — на постамент,
    стелы/обелиски — по типу. Высота — из тега height, если есть."""
    ctx, d, geo, terr = env.ctx, env.d, env.geo, env.terr
    items = [(x, z, t) for x, z, t in d.node_by_chunk.get(env.key, [])
             if t.get("historic") in ("memorial", "monument", "wayside_cross") or t.get("tourism") == "artwork"]
    for poly, t in d.memorial_areas:
        rp = poly.representative_point()
        if env.inside(rp.x, rp.y):
            items.append((rp.x, rp.y, t))
    for x, z, t in items:
        kind = t.get("memorial") or t.get("artwork_type") or ("statue" if t.get("historic") == "monument" else "stele")
        if t.get("historic") == "wayside_cross":
            kind = "cross"
        if kind in ("mural", "graffiti", "painting", "mosaic"):
            continue
        g = _h(terr, x, z)
        base = np.array([x, g, z])
        face = _face_or(d, x, z)
        N = np.array([face[0], 0, face[1]]); R = np.cross(UP, N)
        Ht = num(t.get("height"))
        if kind == "plaque":
            wa = wall_at(ctx, x, z, 6.0)
            if wa is not None:
                _, _, a, b, nrm, _ = wa
                ab = b - a
                u = np.clip(((x - a[0]) * ab[0] + (z - a[1]) * ab[1]) / (ab @ ab), 0.1, 0.9)
                q = a + ab * u
                Nw = np.array([nrm[0], 0, nrm[1]])
                c = np.array([q[0], _h(terr, *q) + 2.0, q[1]]) + Nw * 0.03
                geo.box("bronze", c, np.cross(UP, Nw), UP, Nw, 0.3, 0.22, 0.02)
                continue
            kind = "stele"
        if kind in ("statue", "bust", "sculpture", "war_memorial", "monument"):
            ped = 1.6 if kind == "bust" else 2.2
            total = Ht or (ped + (0.9 if kind == "bust" else 2.6))
            fig = max(total - ped, 0.6)
            geo.box("granite", base + UP * (ped / 2 - 0.2), R, UP, N, 0.8, ped / 2 + 0.2, 0.8)
            top = base + UP * ped
            if kind == "bust":
                geo.cyl("bronze", top, top + UP * fig * 0.45, 0.32, 0.25, n=10, caps=True)
                geo.cyl("bronze", top + UP * fig * 0.45, top + UP * fig, 0.17, 0.15, n=10, caps=True)
            else:
                geo.cyl("bronze", top, top + UP * fig * 0.5, 0.3, 0.25, n=10, caps=True)
                geo.cyl("bronze", top + UP * fig * 0.5, top + UP * fig * 0.85, 0.26, 0.2, n=10, caps=True)
                geo.cyl("bronze", top + UP * fig * 0.85, top + UP * fig, 0.13, 0.11, n=10, caps=True)
        elif kind == "obelisk":
            Hh = Ht or 8.0
            geo.box("granite", base + UP * 0.3, R, UP, N, 1.2, 0.5, 1.2)
            geo.cyl("granite", base + UP * 0.8, base + UP * Hh, 0.6, 0.25, n=4, caps=True)
        elif kind == "cross":
            Hh = Ht or 3.0
            geo.box("granite", base + UP * Hh / 2, R, UP, N, 0.08, Hh / 2, 0.08)
            geo.box("granite", base + UP * Hh * 0.72, R, UP, N, 0.5, 0.07, 0.08)
        else:      # stele, stone, war_memorial без статуи, прочее
            Hh = Ht or 2.5
            geo.box("granite", base + UP * 0.15, R, UP, N, 0.9, 0.25, 0.5)
            geo.box("granite", base + UP * (0.4 + Hh / 2), R, UP, N, 0.55, Hh / 2, 0.15)


def fountains(env):
    """Фонтаны: чаша по контуру (или круг по diameter, иначе Ø6 м), бортик 0.45 м, вода на 0.35 м."""
    from build_chunks import ring_walls, flat_cap
    d, terr, geo = env.d, env.terr, env.geo
    items = [(poly, t) for poly, t in d.fountain_areas if env.inside(*poly.representative_point().coords[0])]
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if t.get("amenity") == "fountain":
            r = (num(t.get("diameter"), 6.0)) / 2
            items.append((shapely.Point(x, z).buffer(r, quad_segs=8), t))
    for poly, t in items:
        for p in shapely.get_parts(poly):
            if p.geom_type != "Polygon" or p.area < 1.0:
                continue
            p = shapely.orient_polygons(p)
            ext = np.asarray(p.exterior.coords)
            g = float(terr.height(ext[:, 0], ext[:, 1]).mean())
            ring_walls(env.mesh, p.exterior, g - 0.2, g + 0.45, g - 0.2, "granite")
            inner = p.buffer(-0.35)
            if inner.is_empty:
                continue
            rim = p.difference(inner)
            flat_cap(env.mesh, rim, g + 0.45, "granite")
            for ip in shapely.get_parts(inner):
                if ip.geom_type == "Polygon":
                    flat_cap(env.mesh, ip, g + 0.35, "water")
            if p.area < 400:
                c = p.representative_point()
                geo.cyl("granite", (c.x, g, c.y), (c.x, g + 1.2, c.y), 0.35, 0.25, n=10, caps=True)


# ------------------------------------------------------------------ малые формы

def furniture(env):
    """Скамейки, урны, почтовые ящики, автоматы, контейнеры, гидранты, информационные стенды,
    реклама, мачты, трубы, водонапорные башни, детское оборудование (только то, что размечено)."""
    ctx, d, geo, terr = env.ctx, env.d, env.geo, env.terr
    for x, z, t in d.node_by_chunk.get(env.key, []):
        a, le, mm, pg = t.get("amenity"), t.get("leisure"), t.get("man_made"), t.get("playground")
        kind = None
        if a in ("bench",) or le == "bench":
            kind = "bench"
        elif a in ("waste_basket", "post_box", "vending_machine", "parcel_locker", "telephone", "drinking_water",
                   "bicycle_parking", "recycling", "waste_disposal", "atm", "clock"):
            kind = a
        elif le == "picnic_table":
            kind = "picnic_table"
        elif t.get("emergency") == "fire_hydrant" and t.get("fire_hydrant:type", "pillar") == "pillar":
            kind = "hydrant"
        elif t.get("tourism") == "information" and t.get("information") in (None, "board", "map"):
            kind = "board"
        elif t.get("advertising") in ("column", "billboard", "poster_box", "board"):
            kind = "ad_" + t["advertising"]
        elif mm in ("mast", "chimney", "water_tower", "flagpole", "street_cabinet", "tower"):
            kind = mm
        elif pg:
            kind = "play_" + pg
        if kind is None:
            continue
        if kind not in ("chimney", "water_tower", "mast", "tower") and inside_building(ctx, x, z):
            continue          # внутри зданий (банкоматы в ТЦ и т.п.) — не рисуем
        g = _h(terr, x, z)
        base = np.array([x, g, z])
        face = _face_or(d, x, z)
        N = np.array([face[0], 0, face[1]]); R = np.cross(UP, N)
        Ht = num(t.get("height"))
        if kind == "bench":
            geo.box("wood_planks", base + UP * 0.45, R, UP, N, 0.9, 0.03, 0.22)
            if t.get("backrest") != "no":
                geo.box("wood_planks", base + UP * 0.75 - N * 0.22, R, UP, N, 0.9, 0.2, 0.025)
            for s in (-1, 1):
                geo.box("steel_grey", base + R * s * 0.75 + UP * 0.22, R, UP, N, 0.03, 0.22, 0.2)
        elif kind == "waste_basket":
            geo.cyl("steel_grey", base, base + UP * 0.75, 0.2, 0.22, n=8, caps=True)
        elif kind == "post_box":
            geo.box("steel_grey", base + UP * 0.5, R, UP, N, 0.04, 0.5, 0.04)
            geo.box("paint_blue", base + UP * 1.25, R, UP, N, 0.22, 0.28, 0.15)
        elif kind in ("vending_machine", "atm"):
            geo.box("paint_red" if kind == "vending_machine" else "steel_grey", base + UP * 0.9, R, UP, N, 0.45, 0.9, 0.4)
        elif kind == "parcel_locker":
            geo.box("steel_grey", base + UP * 1.0, R, UP, N, 1.0, 1.0, 0.3)
        elif kind == "telephone":
            geo.box("steel_grey", base + UP * 1.1, R, UP, N, 0.35, 1.1, 0.3)
        elif kind == "drinking_water":
            geo.cyl("granite", base, base + UP * 1.0, 0.18, n=8, caps=True)
        elif kind == "bicycle_parking":
            for k in range(5):
                p = base + R * (k - 2) * 0.8
                geo.cyl("steel_grey", p, p + UP * 0.8, 0.025, n=4)
                geo.cyl("steel_grey", p + UP * 0.8, p + UP * 0.8 + N * 0.6, 0.025, n=4)
                geo.cyl("steel_grey", p + N * 0.6, p + UP * 0.8 + N * 0.6, 0.025, n=4)
        elif kind in ("recycling", "waste_disposal"):
            n = 1 if kind == "recycling" else 3
            for k in range(n):
                geo.box("paint_green", base + R * (k - (n - 1) / 2) * 1.4 + UP * 0.65, R, UP, N, 0.62, 0.65, 0.5)
        elif kind == "clock":
            geo.cyl("steel_grey", base, base + UP * 3.0, 0.07, n=8)
            geo.box("signal_body", base + UP * 3.3, R, UP, N, 0.3, 0.3, 0.12)
        elif kind == "picnic_table":
            geo.box("wood_planks", base + UP * 0.75, R, UP, N, 0.9, 0.03, 0.4)
            for s in (-1, 1):
                geo.box("wood_planks", base + UP * 0.45 + N * s * 0.65, R, UP, N, 0.9, 0.03, 0.15)
        elif kind == "hydrant":
            geo.cyl("paint_red", base, base + UP * 0.8, 0.1, n=8, caps=True)
        elif kind == "board":
            for s in (-1, 1):
                geo.box("steel_grey", base + R * s * 0.6 + UP * 1.0, R, UP, N, 0.04, 1.0, 0.04)
            geo.box("sign_back", base + UP * 1.4, R, UP, N, 0.7, 0.5, 0.03)
        elif kind == "ad_column":
            geo.cyl("sign_board", base, base + UP * (Ht or 3.0), 0.6, n=12, caps=True)
        elif kind == "ad_billboard":
            Hh = Ht or 9.0
            geo.cyl("steel_grey", base - UP * 0.5, base + UP * (Hh - 3.0), 0.3, n=8)
            geo.box("sign_board", base + UP * (Hh - 1.5), R, UP, N, 3.0, 1.5, 0.2)
        elif kind in ("ad_poster_box", "ad_board"):
            geo.box("sign_board", base + UP * 1.0, R, UP, N, 0.65, 1.0, 0.1)
        elif kind == "street_cabinet":
            geo.box("steel_grey", base + UP * 0.7, R, UP, N, 0.4, 0.7, 0.2)
        elif kind == "flagpole":
            geo.cyl("steel_grey", base, base + UP * (Ht or 10.0), 0.06, 0.04, n=6)
        elif kind in ("mast", "tower"):
            Hh = Ht or 30.0
            geo.cyl("steel_lattice", base, base + UP * Hh, max(Hh * 0.02, 0.3), 0.15, n=4)
        elif kind == "chimney":
            Hh = Ht or 40.0
            geo.cyl("wall_brick_plain", base, base + UP * Hh, max(Hh * 0.045, 1.0), max(Hh * 0.025, 0.6), n=12, caps=True)
        elif kind == "water_tower":
            Hh = Ht or 25.0
            geo.cyl("wall_brick_plain", base, base + UP * (Hh - 6), 2.5, 2.3, n=12)
            geo.cyl("steel_grey", base + UP * (Hh - 6), base + UP * Hh, 4.0, 4.0, n=14, caps=True)
        elif kind.startswith("play_"):
            p = kind[5:]
            if p in ("swing", "basketswing"):
                for s in (-1, 1):
                    geo.cyl("steel_grey", base + R * s * 1.3 - N * 0.8, base + R * s * 1.3 + UP * 2.2, 0.05, n=6)
                    geo.cyl("steel_grey", base + R * s * 1.3 + N * 0.8, base + R * s * 1.3 + UP * 2.2, 0.05, n=6)
                geo.cyl("steel_grey", base - R * 1.3 + UP * 2.2, base + R * 1.3 + UP * 2.2, 0.05, n=6)
                for s in (-0.5, 0.5):
                    geo.cyl("steel_grey", base + R * s + UP * 2.2, base + R * s + UP * 0.5, 0.01, n=3)
                    geo.box("paint_yellow", base + R * s + UP * 0.48, R, UP, N, 0.22, 0.02, 0.12)
            elif p == "slide":
                geo.box("paint_yellow", base - N * 0.6 + UP * 0.75, R, UP, N, 0.5, 0.75, 0.5)
                geo.beam("paint_yellow", base - N * 0.1 + UP * 1.5, base + N * 2.2 + UP * 0.2, 0.5, 0.05)
            elif p == "sandpit":
                geo.box("wood_planks", base + UP * 0.15, R, UP, N, 1.5, 0.15, 1.5)
            elif p in ("seesaw",):
                geo.box("steel_grey", base + UP * 0.25, R, UP, N, 0.1, 0.25, 0.1)
                geo.box("paint_yellow", base + UP * 0.5, N, UP, R, 1.6, 0.03, 0.12)
            elif p in ("roundabout",):
                geo.cyl("paint_yellow", base + UP * 0.3, base + UP * 0.35, 1.0, n=12, caps=True)
            else:   # climbingframe, structure и прочее — каркас
                for su in (-1, 1):
                    for sv in (-1, 1):
                        geo.cyl("paint_yellow", base + R * su * 0.8 + N * sv * 0.8, base + R * su * 0.8 + N * sv * 0.8 + UP * 1.8, 0.04, n=4)
                geo.box("paint_yellow", base + UP * 1.0, R, UP, N, 0.8, 0.03, 0.8)


def metro(env):
    """Входы в метро: навес над лестничным спуском вдоль улицы + стойка с красной «М»."""
    d, geo, terr = env.d, env.geo, env.terr
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if t.get("railway") != "subway_entrance":
            continue
        it = nearest_line(d.road_tree, d.roads, x, z, 40.0) or nearest_line(d.way_tree, d.ways, x, z, 40.0)
        tv = tangent_at(it[0], x, z)[0] if it is not None else np.array([1.0, 0.0])
        f, r = frame_t(tv)
        g = _h(terr, x, z)
        base = np.array([x, g, z])
        for sl in (-1, 1):
            geo.box("glass_shelter", base + r * sl * 1.5 + UP * 1.3, f, UP, r, 3.5, 1.3, 0.02)
        geo.box("glass_shelter", base - f * 3.5 + UP * 1.3, r, UP, f, 1.5, 1.3, 0.02)
        geo.box("steel_grey", base + UP * 2.7, f, UP, r, 3.7, 0.08, 1.7)
        geo.box("concrete", base + UP * 0.05, f, UP, r, 3.5, 0.05, 1.5)
        post = base + f * 4.3 + r * 1.8
        geo.cyl("steel_grey", post - UP * 0.3, post + UP * 3.0, 0.06, n=6)
        _sign(geo, terr, post[0], post[2], tv, "sign_metro", bottom=2.4, size=0.8, two_sided=True, pole=False)


# ------------------------------------------------------------------ растительность из OSM

_MODELS = {}


def _model(name):
    if name not in _MODELS:
        from collections import defaultdict as dd
        V, Nn, T, out = [], [], [], dd(lambda: ([], [], []))
        mat = None
        for line in open(C.OUT / "models" / f"{name}.obj", encoding="utf-8"):
            p = line.split()
            if not p:
                continue
            if p[0] == "v":
                V.append([float(v) for v in p[1:4]])
            elif p[0] == "vn":
                Nn.append([float(v) for v in p[1:4]])
            elif p[0] == "vt":
                T.append([float(v) for v in p[1:3]])
            elif p[0] == "usemtl":
                mat = p[1]
            elif p[0] == "f":
                idx = [[int(i) - 1 for i in c.split("/")] for c in p[1:4]]
                P_, N_, T_ = out[mat]
                P_.append([V[i[0]] for i in idx]); T_.append([T[i[1]] for i in idx]); N_.append([Nn[i[2]] for i in idx])
        _MODELS[name] = {m: (np.array(a), np.array(b), np.array(c)) for m, (a, b, c) in out.items()}
    return _MODELS[name]


def _bake(mesh, name, x, y, z, yaw_deg, sxz, sy):
    c, s = math.cos(math.radians(yaw_deg)), math.sin(math.radians(yaw_deg))
    Rm = np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])
    S = np.diag([sxz, sy, sxz])
    M = Rm @ S
    Ninv = Rm @ np.diag([1 / sxz, 1 / sy, 1 / sxz])
    for mat, (P, N, T) in _model(name).items():
        P2 = P @ M.T + np.array([x, y, z])
        N2 = N @ Ninv.T
        N2 /= np.linalg.norm(N2, axis=-1, keepdims=True)
        mesh.add(mat, P2, N2, T)


_CONIFER = {"Pinus", "Picea", "Abies", "Larix", "Juniperus", "Thuja"}


def trees(env):
    """Деревья из OSM (natural=tree, tree_row) — запечены в чанк: порода (leaf_type/genus),
    высота (height) и крона (diameter_crown), если размечены."""
    d, terr = env.d, env.terr
    MODEL_H = {"tree_deciduous": 9.35, "tree_pine": 16.4}     # высота моделей models/*.obj, м (измерено)
    MODEL_CROWN = {"tree_deciduous": 6.2, "tree_pine": 5.2}
    pts = []
    for x, z, t in d.node_by_chunk.get(env.key, []):
        if t.get("natural") == "tree":
            pts.append((x, z, t))
    for line in d.tree_rows:
        if not line.intersects(env.boxg):
            continue
        for s in np.arange(3.0, line.length, 7.0):
            p = line.interpolate(s)
            if env.inside(p.x, p.y):
                pts.append((p.x, p.y, {}))
    for x, z, t in pts:
        conifer = t.get("leaf_type") == "needleleaved" or t.get("genus") in _CONIFER
        name = "tree_pine" if conifer else "tree_deciduous"
        k = 0.8 + 0.5 * h01(x, z, 6)
        Ht, Dc = num(t.get("height")), num(t.get("diameter_crown"))
        sy = Ht / MODEL_H[name] if Ht else k
        sxz = Dc / MODEL_CROWN[name] if Dc else sy
        _bake(env.mesh, name, x, _h(terr, x, z), z, h01(x, z, 5) * 360, sxz, sy)


def scrub(env):
    """Кустарник (natural=scrub): кусты по площади полигона (шаг ~5 м), не на дорожках."""
    d, geo, terr = env.d, env.geo, env.terr
    blocked = env.blocked
    for poly in d.scrub_areas:
        if not poly.intersects(env.boxg):
            continue
        g = shapely.intersection(poly, env.boxg)
        if g.is_empty:
            continue
        x0, z0, x1, z1 = env.box
        X, Z = np.meshgrid(np.arange(x0 + 2.5, x1, 5.0), np.arange(z0 + 2.5, z1, 5.0))
        X = X.ravel() + (np.array([h01(a, b, 31) for a, b in zip(X.ravel(), Z.ravel())]) - 0.5) * 4
        Z = Z.ravel() + (np.array([h01(a, b, 32) for a, b in zip(X, Z.ravel())]) - 0.5) * 4
        keep = shapely.contains_xy(g, X, Z)
        if blocked is not None:
            keep &= ~shapely.contains_xy(blocked, X, Z)
        for x, z in zip(X[keep], Z[keep]):
            if h01(x, z, 33) > 0.55:
                continue
            r = 0.6 + 0.6 * h01(x, z, 34)
            y = _h(terr, x, z)
            geo.cyl("leaves", (x, y - 0.1, z), (x, y + r * 0.9, z), r, r * 0.6, n=6, caps=True)


# ------------------------------------------------------------------ сборка

class Env:
    pass


def build(mesh, ctx, terr, gx, gz, classes, drape, lights, blocked):
    d = ctx.det
    env = Env()
    env.mesh, env.ctx, env.terr, env.d = mesh, ctx, terr, d
    env.geo = Geo()
    x0, z0 = gx * C.CHUNK, gz * C.CHUNK
    env.box = (x0, z0, x0 + C.CHUNK, z0 + C.CHUNK)
    env.boxg = shapely.box(*env.box)
    env.key = (gx, gz)
    env.classes = classes
    env.drape = drape
    env.lights = lights
    env.blocked = blocked
    env.inside = lambda x, z: x0 <= x < x0 + C.CHUNK and z0 <= z < z0 + C.CHUNK
    env.bld_here = []
    if ctx.bld_tree is not None:
        for i in ctx.bld_tree.query(env.boxg):
            rp = ctx.buildings[i][0].representative_point()
            if env.inside(rp.x, rp.y):
                env.bld_here.append((i, ctx.buildings[i]))
    stats = {}
    for fn in (crossings, turn_arrows, level_crossings, traffic_signals, priority_signs, stops, trolley, trams,
               power, kerbs, barrier_nodes, steps, entrances, shop_signs, memorials, fountains, furniture,
               metro, trees, scrub):
        try:
            fn(env)
        except Exception as e:
            print(f"[warn] chunk {gx},{gz} {fn.__name__}: {type(e).__name__}: {e}", flush=True)
    env.geo.flush(mesh)
    return stats
