"""Выгрузка железной дороги из OSM: маршрут между двумя станциями + коридор вокруг него.

Берёт .osm.pbf (по умолчанию — выгрузку Самарской области из кэша генератора Самары),
строит граф путей, находит главный ход между станциями (кратчайший путь, у которого
второстепенные пути — подъездные, станционные, депо — дороже) и сохраняет всё
железнодорожное в коридоре вокруг него.

Выход (data/rail_map/<name>/):
  route.geojson     — ось маршрута + станции на нём (lon/lat)
  corridor.geojson  — все пути, платформы, станции, сигналы, стрелки, переезды,
                      опоры контактной сети, пикетные столбики в коридоре (lon/lat, теги OSM)
  corridor_local.json — то же в метрах (X восток, Z юг, Y = высота DEM), начало — середина маршрута
  profile.csv       — продольный профиль маршрута: км, x, z, высота
  stats.txt, preview.png

Запуск: python -I extract_rail.py [--from Самара] [--to "Сызрань I"] [--name samara_syzran]
"""
import argparse
import csv
import heapq
import json
import math
import re
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import osmium
import shapely
import tifffile
from PIL import Image, ImageDraw, ImageFont
from pyproj import Transformer
from scipy import ndimage
from shapely.geometry import LineString, Point

ROOT = Path(__file__).resolve().parents[2]          # AlKAsH3D-Engine/
SAMARA_CACHE = ROOT / "data" / "samara_map" / "_cache"
CACHE = ROOT / "data" / "rail_map" / "_cache"
PBF = SAMARA_CACHE / "samara_oblast.osm.pbf"
DEM_DIRS = (CACHE, SAMARA_CACHE)

BBOX = (48.20, 52.85, 50.45, 53.50)                  # lon/lat с запасом вокруг Самара — Сызрань
CORRIDOR = 1500.0                                    # м по обе стороны от оси маршрута
PROFILE_STEP = 25.0                                  # м, шаг продольного профиля
BRIDGE_MARGIN = 40.0                                 # м за краем моста, где DEM ещё «проваливается»

TRACK_KINDS = {"rail", "narrow_gauge", "light_rail", "disused", "preserved"}
ROUTABLE = {"rail"}
# второстепенные пути дороже — чтобы маршрут шёл по главному ходу, а не через парки станций
SERVICE_COST = {None: 1.0, "siding": 4.0, "crossover": 2.0, "yard": 6.0, "spur": 8.0}
USAGE_COST = {"main": 1.0, "branch": 1.3, "industrial": 6.0, "military": 6.0, "tourism": 6.0}

NODE_KINDS = {
    "railway": {"station", "halt", "stop", "signal", "switch", "railway_crossing", "level_crossing",
                "crossing", "buffer_stop", "milestone", "derail", "tram_stop", "border", "owner_change",
                "km_post", "phone", "vacancy_detection"},
    "power": {"catenary_mast", "portal"},
    "public_transport": {"station", "stop_position", "platform"},
}
AREA_RAIL = {"platform", "station"}                  # railway=* полигоны/линии, кроме путей
KEEP_TAGS = None                                     # None = все теги OSM


def node_kind(tags):
    for k, vals in NODE_KINDS.items():
        v = tags.get(k)
        if v in vals:
            return f"{k}={v}"
    return None


class Collect(osmium.SimpleHandler):
    def __init__(self, bbox):
        super().__init__()
        self.lon0, self.lat0, self.lon1, self.lat1 = bbox
        self.ways = []        # (id, [node ids], [(lon, lat)], tags)
        self.nodes = []       # (id, lon, lat, kind, tags)

    def inside(self, lon, lat):
        return self.lon0 <= lon <= self.lon1 and self.lat0 <= lat <= self.lat1

    def node(self, n):
        if not n.tags:
            return
        t = dict(n.tags)
        kind = node_kind(t)
        if kind is None or not n.location.valid():
            return
        lon, lat = n.location.lon, n.location.lat
        if self.inside(lon, lat):
            self.nodes.append((n.id, lon, lat, kind, t))

    def way(self, w):
        rw = w.tags.get("railway")
        if rw is None and w.tags.get("public_transport") != "platform":
            return
        if rw not in TRACK_KINDS and rw not in AREA_RAIL and w.tags.get("public_transport") != "platform":
            return
        try:
            refs = [nd.ref for nd in w.nodes]
            pts = [(nd.lon, nd.lat) for nd in w.nodes]
        except osmium.InvalidLocationError:
            return
        if len(pts) < 2 or not any(self.inside(*p) for p in pts):
            return
        self.ways.append((w.id, refs, pts, dict(w.tags)))


def find_dem_tiles(lon0, lat0, lon1, lat1):
    """Мозаика тайлов 1°×1° (одинаковой ширины в пределах полосы широт 50–60°), север сверху."""
    rows = []
    for lat in range(math.floor(lat1), math.floor(lat0) - 1, -1):
        row = []
        for lon in range(math.floor(lon0), math.floor(lon1) + 1):
            name = f"dem_N{lat:02d}_00_E{lon:03d}_00.tif"
            p = next((d / name for d in DEM_DIRS if (d / name).exists()), None)
            if p is None:
                raise SystemExit(f"нет тайла рельефа {name} (Copernicus GLO-30) в {CACHE}")
            row.append(tifffile.imread(str(p)).astype(np.float32))
        rows.append(np.hstack(row))
    dem = np.vstack(rows)
    ny, nx = rows[0].shape[0], rows[0].shape[1] // (math.floor(lon1) - math.floor(lon0) + 1)
    return dem, float(math.floor(lon0)), float(math.floor(lat1) + 1), 1.0 / nx, 1.0 / ny


def way_cost(tags):
    c = SERVICE_COST.get(tags.get("service"), 3.0)
    c *= USAGE_COST.get(tags.get("usage"), 1.5)
    return c


def build_graph(ways, geod_len):
    g = defaultdict(list)                            # node -> [(nbr, cost, length, way_idx)]
    for wi, (wid, refs, pts, tags) in enumerate(ways):
        if tags.get("railway") not in ROUTABLE:
            continue
        c = way_cost(tags)
        for i in range(len(refs) - 1):
            L = geod_len(pts[i], pts[i + 1])
            g[refs[i]].append((refs[i + 1], L * c, L, wi))
            g[refs[i + 1]].append((refs[i], L * c, L, wi))
    return g


def dijkstra(g, src_set, dst_set):
    dist = {s: 0.0 for s in src_set}
    prev = {}
    pq = [(0.0, s) for s in src_set]
    heapq.heapify(pq)
    while pq:
        d, u = heapq.heappop(pq)
        if d > dist.get(u, math.inf):
            continue
        if u in dst_set:
            path = [u]
            while path[-1] in prev:
                path.append(prev[path[-1]][0])
            return path[::-1], prev
        for v, c, _, wi in g[u]:
            nd = d + c
            if nd < dist.get(v, math.inf):
                dist[v] = nd
                prev[v] = (u, wi)
                heapq.heappush(pq, (nd, v))
    return None, prev


def pick_station(nodes, ways, pattern):
    rx = re.compile(pattern, re.I)
    hits = [(lon, lat, t.get("name")) for _, lon, lat, kind, t in nodes
            if kind in ("railway=station", "public_transport=station") and rx.fullmatch(t.get("name", "") or "")]
    if not hits:
        names = sorted({t.get("name") for _, _, _, k, t in nodes if k == "railway=station" and t.get("name")})
        raise SystemExit(f"станция /{pattern}/ не найдена. Есть: {', '.join(names)}")
    return hits[0]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--from", dest="src", default=r"Самара")
    ap.add_argument("--to", dest="dst", default=r"Сызрань[- ]?(I|1)")
    ap.add_argument("--name", default="samara_syzran")
    ap.add_argument("--pbf", default=str(PBF))
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")         # python -I игнорирует PYTHONIOENCODING
    out =ROOT / "data" / "rail_map" / args.name
    out.mkdir(parents=True, exist_ok=True)

    t0 = time.time()
    h = Collect(BBOX)
    h.apply_file(args.pbf, locations=True)
    print(f"OSM: {len(h.ways)} линий, {len(h.nodes)} точек за {time.time() - t0:.0f} с")

    geod = Transformer.from_crs("EPSG:4326", "+proj=tmerc +lat_0=53.2 +lon_0=49.3 +ellps=WGS84 +units=m",
                                always_xy=True)

    def geod_len(a, b):
        (x0, y0), (x1, y1) = geod.transform(*a), geod.transform(*b)
        return math.hypot(x1 - x0, y1 - y0)

    g = build_graph(h.ways, geod_len)
    node_ll = {}
    for _, refs, pts, tags in h.ways:
        for r, p in zip(refs, pts):
            node_ll[r] = p

    def nearest_graph_nodes(lon, lat, radius=600.0):
        x, y = geod.transform(lon, lat)
        res = []
        for nid in g:
            nx, ny = geod.transform(*node_ll[nid])
            if math.hypot(nx - x, ny - y) < radius:
                res.append(nid)
        return set(res)

    s_lon, s_lat, s_name = pick_station(h.nodes, h.ways, args.src)
    d_lon, d_lat, d_name = pick_station(h.nodes, h.ways, args.dst)
    path, prev = dijkstra(g, nearest_graph_nodes(s_lon, s_lat), nearest_graph_nodes(d_lon, d_lat))
    if path is None:
        raise SystemExit("пути между станциями в графе нет (разрыв в OSM?)")
    route_ll = [node_ll[n] for n in path]
    route_ways = Counter(prev[n][1] for n in path[1:])
    print(f"маршрут {s_name} → {d_name}: {len(path)} узлов, {len(route_ways)} линий OSM")

    # локальная система: начало в середине маршрута, X восток, Z юг
    mid = route_ll[len(route_ll) // 2]
    proj = f"+proj=tmerc +lat_0={mid[1]:.6f} +lon_0={mid[0]:.6f} +k=1 +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs"
    to_loc = Transformer.from_crs("EPSG:4326", proj, always_xy=True)

    def loc(pts):
        xs, ys = to_loc.transform([p[0] for p in pts], [p[1] for p in pts])
        return [(x, -y) for x, y in zip(xs, ys)]

    route_xz = loc(route_ll)
    route_line = LineString(route_xz)
    length = route_line.length
    corridor = route_line.buffer(CORRIDOR)
    shapely.prepare(corridor)

    # рельеф
    lons = [p[0] for p in route_ll]
    lats = [p[1] for p in route_ll]
    dem, dlon0, dlat0, dlon, dlat = find_dem_tiles(min(lons) - 0.05, min(lats) - 0.05, max(lons) + 0.05, max(lats) + 0.05)
    to_geo = Transformer.from_crs(proj, "EPSG:4326", always_xy=True)

    def elev(xz):
        xz = np.asarray(xz, dtype=np.float64).reshape(-1, 2)
        lon, lat = to_geo.transform(xz[:, 0], -xz[:, 1])
        col = (np.asarray(lon) - dlon0) / dlon
        row = (dlat0 - np.asarray(lat)) / dlat
        return ndimage.map_coordinates(dem, [row, col], order=1, mode="nearest")

    # коридор: пути/платформы/точки
    feats_geo, feats_loc = [], []
    kinds = Counter()
    track_len = Counter()
    route_way_ids = {h.ways[wi][0] for wi in route_ways}
    for wid, refs, pts, tags in h.ways:
        xz = loc(pts)
        geom = LineString(xz)
        if not corridor.intersects(geom):
            continue
        rw = tags.get("railway") or "platform"
        kinds[f"way railway={rw}"] += 1
        if rw in TRACK_KINDS:
            track_len[(rw, tags.get("service") or tags.get("usage") or "-")] += geom.length
        y = elev(xz)
        on_route = wid in route_way_ids
        feats_geo.append({"type": "Feature", "properties": {"osm": f"w{wid}", "on_route": on_route, **tags},
                          "geometry": {"type": "LineString", "coordinates": pts}})
        feats_loc.append({"osm": f"w{wid}", "on_route": on_route, "tags": tags,
                          "pts": [[round(x, 2), round(float(yy), 2), round(z, 2)] for (x, z), yy in zip(xz, y)]})
    nodes_loc = []
    for nid, lon, lat, kind, tags in h.nodes:
        (x, z), = loc([(lon, lat)])
        if not corridor.contains(Point(x, z)):
            continue
        kinds[kind] += 1
        ch = route_line.project(Point(x, z))
        off = route_line.distance(Point(x, z))
        feats_geo.append({"type": "Feature", "properties": {"osm": f"n{nid}", "kind": kind, **tags},
                          "geometry": {"type": "Point", "coordinates": [lon, lat]}})
        nodes_loc.append({"osm": f"n{nid}", "kind": kind, "tags": tags, "chainage_m": round(ch, 1),
                          "offset_m": round(off, 1),
                          "pos": [round(x, 2), round(float(elev([(x, z)])[0]), 2), round(z, 2)]})

    # станции на маршруте (≤ 400 м от оси), по порядку
    stations = sorted((n for n in nodes_loc
                       if n["kind"] in ("railway=station", "railway=halt") and n["offset_m"] < 400),
                      key=lambda n: n["chainage_m"])
    seen = set()
    stations = [s for s in stations if not (s["tags"].get("name") in seen or seen.add(s["tags"].get("name")))]

    # профиль
    ds = np.arange(0.0, length, PROFILE_STEP).tolist() + [length]
    prof_xz = [route_line.interpolate(d).coords[0] for d in ds]
    prof_y = elev(prof_xz)
    prof_y_s = ndimage.median_filter(prof_y, size=9)   # DSM: срезать кроны/постройки у полотна
    # мосты и тоннели: DEM видит воду/овраг под мостом или гору над тоннелем — там полотно
    # идёт прямой между высотами у краёв сооружения
    spans = []
    for wi in route_ways:
        tags = h.ways[wi][3]
        if tags.get("bridge", "no") == "no" and tags.get("tunnel", "no") == "no":
            continue
        a, b = sorted(route_line.project(Point(p)) for p in loc(h.ways[wi][2])[::len(h.ways[wi][2]) - 1])
        spans.append([a - BRIDGE_MARGIN, b + BRIDGE_MARGIN, tags.get("bridge", "no") != "no"])
    spans.sort()
    merged = []
    for s in spans:
        if merged and s[0] <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], s[1])
            merged[-1][2] = merged[-1][2] or s[2]
        else:
            merged.append(s)
    ds_np = np.asarray(ds)
    struct = np.zeros(len(ds), dtype=bool)
    for a, b, _ in merged:
        inside = (ds_np >= a) & (ds_np <= b)
        if not inside.any():
            continue
        i0 = max(int(np.argmax(inside)) - 1, 0)
        i1 = min(len(ds) - 1 - int(np.argmax(inside[::-1])) + 1, len(ds) - 1)
        prof_y_s[i0:i1 + 1] = np.interp(ds_np[i0:i1 + 1], [ds_np[i0], ds_np[i1]], [prof_y_s[i0], prof_y_s[i1]])
        struct[inside] = True
    with open(out / "profile.csv", "w", newline="", encoding="utf-8") as f:
        wr = csv.writer(f)
        wr.writerow(["km", "x", "z", "dem_m", "track_m", "structure"])
        for d, (x, z), y, ys, st in zip(ds, prof_xz, prof_y, prof_y_s, struct):
            wr.writerow([f"{d / 1000:.3f}", f"{x:.1f}", f"{z:.1f}", f"{y:.1f}", f"{ys:.1f}", int(st)])

    route_geo = {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"name": f"{s_name} — {d_name}", "length_m": round(length, 1)},
         "geometry": {"type": "LineString", "coordinates": route_ll}}] + [
        {"type": "Feature", "properties": {"name": s["tags"].get("name"), "kind": s["kind"],
                                           "km": round(s["chainage_m"] / 1000, 2)},
         "geometry": {"type": "Point", "coordinates": list(to_geo.transform(s["pos"][0], -s["pos"][2]))}}
        for s in stations]}
    (out / "route.geojson").write_text(json.dumps(route_geo, ensure_ascii=False), encoding="utf-8")
    (out / "corridor.geojson").write_text(json.dumps({"type": "FeatureCollection", "features": feats_geo},
                                                     ensure_ascii=False), encoding="utf-8")
    (out / "corridor_local.json").write_text(json.dumps({
        "proj": proj, "axes": "X east, Y = DEM height ASL (m), Z south", "corridor_m": CORRIDOR,
        "route": {"from": s_name, "to": d_name, "length_m": round(length, 1),
                  "pts": [[round(x, 2), round(float(y), 2), round(z, 2)] for (x, z), y in zip(route_xz, elev(route_xz))]},
        "stations": stations, "ways": feats_loc, "nodes": nodes_loc}, ensure_ascii=False), encoding="utf-8")

    lines = [f"Маршрут: {s_name} → {d_name}", f"Длина по оси: {length / 1000:.1f} км",
             f"Высота DEM: {prof_y_s.min():.0f}–{prof_y_s.max():.0f} м",
             f"Макс. уклон (на 500 м): {max_grade(ds, prof_y_s):.1f} ‰", "", "Станции и о.п. на маршруте:"]
    lines += [f"  {s['chainage_m'] / 1000:7.1f} км  {s['tags'].get('name', '?')}  ({s['kind'].split('=')[1]})"
              for s in stations]
    lines += ["", "Мосты/тоннели на главном ходу (≥ 100 м):"]
    lines += [f"  {a / 1000:7.2f}–{b / 1000:.2f} км  {'мост' if br else 'тоннель'}  {(b - a - 2 * BRIDGE_MARGIN):.0f} м"
              for a, b, br in merged if b - a - 2 * BRIDGE_MARGIN >= 100]
    lines += ["", f"В коридоре ±{CORRIDOR / 1000:.1f} км:"]
    lines += [f"  {k}: {v}" for k, v in kinds.most_common()]
    lines += ["", "Длина путей в коридоре (км):"]
    lines += [f"  {rw} / {sv}: {L / 1000:.1f}" for (rw, sv), L in sorted(track_len.items(), key=lambda kv: -kv[1])]
    elec = Counter(h.ways[wi][3].get("electrified", "?") for wi in route_ways)
    lines += ["", f"Электрификация главного хода (линий OSM): {dict(elec)}"]
    (out / "stats.txt").write_text("\n".join(lines), encoding="utf-8")
    print("\n".join(lines[:8 + len(stations)]))

    preview(out / "preview.png", feats_loc, route_xz, stations, ds, prof_y_s, f"{s_name} — {d_name}")
    print(f"готово за {time.time() - t0:.0f} с → {out}")


def max_grade(ds, y, base=500.0):
    ds = np.asarray(ds)
    best = 0.0
    j = 0
    for i in range(len(ds)):
        while j < len(ds) and ds[j] - ds[i] < base:
            j += 1
        if j >= len(ds):
            break
        best = max(best, abs(y[j] - y[i]) / (ds[j] - ds[i]) * 1000)
    return best


def preview(path, ways, route_xz, stations, ds, prof, title):
    W, MH, PH = 2400, 1300, 420
    img = Image.new("RGB", (W, MH + PH), (247, 245, 240))
    dr = ImageDraw.Draw(img)
    try:
        font = ImageFont.truetype("C:/Windows/Fonts/arial.ttf", 18)
        big = ImageFont.truetype("C:/Windows/Fonts/arialbd.ttf", 30)
    except OSError:
        font = big = ImageFont.load_default()
    xs = [p[0] for p in route_xz]
    zs = [p[1] for p in route_xz]
    x0, x1, z0, z1 = min(xs) - 2500, max(xs) + 2500, min(zs) - 2500, max(zs) + 2500
    s = min((W - 80) / (x1 - x0), (MH - 80) / (z1 - z0))
    ox = (W - (x1 - x0) * s) / 2
    oz = (MH - (z1 - z0) * s) / 2

    def P(x, z):
        return (ox + (x - x0) * s, oz + (z - z0) * s)

    for w in ways:
        rw = w["tags"].get("railway")
        pts = [P(p[0], p[2]) for p in w["pts"]]
        if rw in ("platform",) or w["tags"].get("public_transport") == "platform":
            dr.line(pts, fill=(90, 140, 220), width=3)
        elif rw == "rail":
            col = (120, 120, 120) if not w["tags"].get("service") else (175, 175, 175)
            dr.line(pts, fill=col, width=2)
        else:
            dr.line(pts, fill=(200, 190, 170), width=1)
    dr.line([P(x, z) for x, z in route_xz], fill=(200, 30, 30), width=4)
    for st in stations:
        px, pz = P(st["pos"][0], st["pos"][2])
        big_st = st["kind"] == "railway=station"
        r = 7 if big_st else 4
        dr.ellipse((px - r, pz - r, px + r, pz + r), fill=(20, 20, 20) if big_st else (255, 255, 255),
                   outline=(20, 20, 20), width=2)
        if big_st:
            dr.text((px + 9, pz - 24), st["tags"].get("name", ""), fill=(20, 20, 20), font=font)
    dr.text((30, 20), title, fill=(20, 20, 20), font=big)
    # шкала 10 км
    dr.line((30, MH - 40, 30 + 10000 * s, MH - 40), fill=(20, 20, 20), width=3)
    dr.text((30, MH - 34), "10 км", fill=(20, 20, 20), font=font)

    # профиль
    top, bot = MH + 30, MH + PH - 40
    ymin, ymax = float(min(prof)) - 5, float(max(prof)) + 5
    L = ds[-1]

    def Q(d, y):
        return (60 + d / L * (W - 120), bot - (y - ymin) / (ymax - ymin) * (bot - top))

    dr.rectangle((60, top, W - 60, bot), outline=(160, 160, 160))
    for k in range(int(ymin // 50 + 1) * 50, int(ymax) + 1, 50):
        _, yy = Q(0, k)
        dr.line((60, yy, W - 60, yy), fill=(220, 220, 220))
        dr.text((8, yy - 10), f"{k} м", fill=(90, 90, 90), font=font)
    dr.line([Q(d, y) for d, y in zip(ds, prof)], fill=(30, 90, 30), width=2)
    for st in stations:
        if st["kind"] != "railway=station":
            continue
        qx, _ = Q(st["chainage_m"], ymin)
        dr.line((qx, top, qx, bot), fill=(200, 200, 200))
        dr.text((qx + 3, top + 2), st["tags"].get("name", ""), fill=(60, 60, 60), font=font)
    for km in range(0, int(L / 1000) + 1, 10):
        qx, _ = Q(km * 1000, ymin)
        dr.text((qx - 10, bot + 8), f"{km}", fill=(90, 90, 90), font=font)
    img.save(path)


if __name__ == "__main__":
    main()
