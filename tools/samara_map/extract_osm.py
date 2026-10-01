"""Этап 1: .osm.pbf -> спроецированные геометрии, разложенные по суперплиткам.

Выход (CACHE/work):
  region.pkl           — граница города (+буфер) и список чанков, которые строим
  global.pkl           — полные (не обрезанные) геометрии для рельефа/мостов
  bins/st_<sx>_<sz>.pkl — признаки каждой суперплитки 2x2 км
"""
import pickle
import math
import time
from collections import defaultdict

import numpy as np
import osmium
import shapely
from shapely import wkb as swkb
from pyproj import Transformer

import config as C

KEEP = {
    "building", "building:part", "building:levels", "building:min_level", "height", "min_height",
    "roof:shape", "roof:height", "roof:levels", "roof:colour", "building:colour", "building:material",
    "amenity", "religion", "historic", "name", "highway", "area", "area:highway", "surface", "lanes",
    "width", "oneway", "bridge", "tunnel", "layer", "lit", "sidewalk", "railway", "service", "usage",
    "waterway", "water", "natural", "landuse", "leisure", "barrier", "material", "man_made",
    "leaf_type", "sport", "covered", "location", "parking", "level", "tracks", "embankment",
    # --- детали улиц (osm_details.py)
    "crossing", "crossing:markings", "crossing_ref", "traffic_signals", "traffic_signals:direction", "direction",
    "public_transport", "bus", "tram", "trolleybus", "shelter", "bench", "bin", "trolley_wire",
    "entrance", "door", "addr:housenumber", "addr:street", "shop", "brand", "opening_hours", "craft", "office",
    "power", "cables", "voltage", "circuits", "tourism", "memorial", "artwork_type", "playground",
    "step_count", "incline", "handrail", "ramp", "maxspeed", "turn:lanes", "turn:lanes:forward",
    "turn:lanes:backward", "lanes:forward", "lanes:backward", "kerb", "emergency", "advertising", "start_date",
    "building:architecture", "diameter_crown", "circumference", "genus", "species", "denotation", "cutting",
    "ref", "operator", "colour", "electrified", "frequency", "line", "structure", "design", "support",
    "highway:crossing", "footway", "button_operated", "tactile_paving", "lit", "backrest", "seats", "fountain", "diameter",
}

# точки, которые генератор ставит в мир (osm_details.py) — всё, у чего есть место и смысл в 3D
DETAIL_NODE = {
    "highway": {"street_lamp", "traffic_signals", "crossing", "bus_stop", "give_way", "stop", "steps"},
    "railway": {"tram_stop", "subway_entrance", "crossing", "level_crossing"},
    "public_transport": {"platform", "stop_position"},
    "amenity": {"bench", "waste_basket", "fountain", "post_box", "telephone", "vending_machine", "drinking_water",
                "bicycle_parking", "clock", "recycling", "waste_disposal", "shelter", "atm", "parcel_locker"},
    "leisure": {"bench", "playground", "picnic_table"},
    "playground": None, "entrance": None, "shop": None, "craft": None, "office": None,
    "historic": {"memorial", "monument", "wayside_cross", "wayside_shrine", "boundary_stone", "cannon", "tank"},
    "tourism": {"artwork", "information"},
    "power": {"tower", "pole", "portal", "transformer", "catenary_mast"},
    "barrier": {"gate", "bollard", "lift_gate", "kerb", "block", "swing_gate", "turnstile", "entrance"},
    "man_made": {"street_cabinet", "flagpole", "mast", "tower", "chimney", "water_tower", "manhole"},
    "emergency": {"fire_hydrant", "phone"},
    "advertising": None,
    "natural": {"tree", "rock", "stone"},
}
# у кафе/ресторанов/аптек и т.п. на точке — вывеска (amenity с названием)
SIGN_AMENITY = {"cafe", "restaurant", "fast_food", "bar", "pub", "pharmacy", "bank", "clinic", "dentist",
                "doctors", "post_office", "cinema", "theatre", "library", "car_wash", "fuel", "veterinary",
                "bureau_de_change", "money_transfer", "ice_cream", "nightclub", "kindergarten", "school"}


def is_detail_node(t):
    for k, vals in DETAIL_NODE.items():
        v = t.get(k)
        if v is not None and (vals is None or v in vals):
            return True
    return t.get("amenity") in SIGN_AMENITY and "name" in t
AREA_KEYS = ("building", "building:part", "landuse", "natural", "leisure", "water", "area:highway")
LINE_HIGHWAY_SKIP = {"proposed", "construction", "abandoned", "platform", "raceway", "bus_stop", "elevator", "corridor"}
RAIL_KEEP = {"rail", "tram", "light_rail", "narrow_gauge", "subway", "funicular"}
WATERWAY_LINES = {"river", "canal", "stream", "ditch", "drain"}
BARRIERS = {"wall", "fence", "retaining_wall", "city_wall", "hedge", "guard_rail", "handrail", "kerb", "jersey_barrier"}
POWER_LINES = {"line", "minor_line", "cable"}
ROUTE_KINDS = {"bus", "trolleybus", "tram", "share_taxi", "subway", "train", "light_rail"}
NATURAL_AREAS_SKIP = {"coastline", "tree_row", "tree", "peak", "cliff", "ridge", "valley", "spring"}

_wkb = osmium.geom.WKBFactory()
_to_local = Transformer.from_crs("EPSG:4326", C.PROJ, always_xy=True)


def project(geom):
    def f(c):
        x, y = _to_local.transform(c[:, 0], c[:, 1])
        return np.column_stack([x, -np.asarray(y)])   # Z = -север
    return shapely.transform(geom, f)


def tags_of(o):
    return {t.k: t.v for t in o.tags if t.k in KEEP}


class Grab(osmium.SimpleHandler):
    def __init__(self):
        super().__init__()
        self.lon0, self.lat0, self.lon1, self.lat1 = C.BBOX_LONLAT
        self.areas = []      # (tags, wkb lon/lat)
        self.lines = []      # (tags, wkb lon/lat, node_ids)
        self.nodes = []      # (tags, lon, lat)
        self.boundary = None
        self.nonbridge_nodes = set()
        self.routes = []     # маршруты транспорта: (tags, [(тип, id, роль)])
        self.platforms = []  # платформы-полигоны остановок: (id линии, tags, wkb) — члены маршрутов PTv2

    def relation(self, r):
        t = r.tags
        if t.get("type") == "route" and t.get("route") in ROUTE_KINDS:
            self.routes.append(({k: v for k, v in t}, [(m.type, m.ref, m.role) for m in r.members]))

    def _inside(self, lon, lat):
        return self.lon0 <= lon <= self.lon1 and self.lat0 <= lat <= self.lat1

    def node(self, n):
        t = n.tags
        if len(t) and is_detail_node(t):
            if n.location.valid() and self._inside(n.location.lon, n.location.lat):
                self.nodes.append((tags_of(n), n.location.lon, n.location.lat, n.id))

    def way(self, w):
        t = w.tags
        hw, rw, ww, bar = t.get("highway"), t.get("railway"), t.get("waterway"), t.get("barrier")
        # платформы остановок, нарисованные линией вдоль бордюра
        is_pt_platform = (t.get("public_transport") == "platform" or hw == "platform") and not w.is_closed()
        want = ((hw and hw not in LINE_HIGHWAY_SKIP and t.get("area") != "yes")
                or is_pt_platform or t.get("power") in POWER_LINES
                or rw in RAIL_KEEP or rw == "platform"
                or ww in WATERWAY_LINES or bar in BARRIERS or t.get("natural") in ("tree_row", "cliff"))
        if not want or len(w.nodes) < 2:
            return
        try:
            first = w.nodes[0].location
            last = w.nodes[-1].location
            if not (self._inside(first.lon, first.lat) or self._inside(last.lon, last.lat)):
                return
        except Exception:
            return
        if rw == "platform" and w.is_closed():
            return                                    # площадная платформа придёт через area()
        try:
            g = _wkb.create_linestring(w)
        except Exception:
            return
        ids = [nd.ref for nd in w.nodes]
        if (hw or rw) and t.get("bridge") in (None, "no"):
            self.nonbridge_nodes.update(ids)
        tg = tags_of(w)
        tg["_wid"] = w.id          # id линии — по нему маршруты транспорта собираются из путей
        self.lines.append((tg, g, ids))

    def area(self, a):
        t = a.tags
        if not a.from_way() and a.orig_id() == C.BOUNDARY_REL:
            self.boundary = _wkb.create_multipolygon(a)
            return
        hw = t.get("highway")
        wanted = any(k in t for k in AREA_KEYS) or t.get("amenity") in ("parking", "fountain", "shelter") \
            or t.get("public_transport") == "platform" or t.get("historic") in ("memorial", "monument") \
            or t.get("tourism") == "artwork" \
            or t.get("waterway") in ("riverbank", "dock") or t.get("railway") == "platform" \
            or t.get("man_made") in ("pier", "bridge") \
            or (hw in ("pedestrian", "footway", "service", "track", "platform") and t.get("area") == "yes")
        if not wanted:
            return
        if t.get("natural") in NATURAL_AREAS_SKIP and not any(k in t for k in ("building", "landuse")):
            return
        if t.get("building") == "no" and "building:part" not in t and not any(k in t for k in ("landuse", "natural", "leisure", "amenity")):
            return
        if t.get("location") == "underground" or t.get("parking") == "underground":
            return
        try:
            near = False
            for ring in a.outer_rings():
                for nd in ring:
                    if self._inside(nd.lon, nd.lat):
                        near = True
                    break
                if near:
                    break
            if not near and a.from_way():
                return
        except Exception:
            return
        try:
            g = _wkb.create_multipolygon(a)
        except Exception:
            return
        if a.from_way() and (t.get("public_transport") == "platform" or t.get("highway") == "platform"
                             or t.get("railway") == "platform"):
            self.platforms.append((a.orig_id(), tags_of(a), g))
        self.areas.append((tags_of(a), g))


def main():
    t0 = time.time()
    C.WORK.mkdir(parents=True, exist_ok=True)
    (C.WORK / "bins").mkdir(exist_ok=True)
    h = Grab()
    h.apply_file(str(C.PBF), locations=True, idx="flex_mem")
    print(f"[osm] прочитано: areas={len(h.areas)} lines={len(h.lines)} nodes={len(h.nodes)} за {time.time()-t0:.0f}s")
    assert h.boundary is not None, "граница городского округа не найдена"

    boundary = project(swkb.loads(h.boundary, hex=True))
    region = boundary.buffer(C.REGION_BUFFER, quad_segs=4)
    rminx, rminz, rmaxx, rmaxz = region.bounds
    chunks = []
    boxes = []
    for gx in range(math.floor(rminx / C.CHUNK), math.floor(rmaxx / C.CHUNK) + 1):
        for gz in range(math.floor(rminz / C.CHUNK), math.floor(rmaxz / C.CHUNK) + 1):
            chunks.append((gx, gz))
            boxes.append(shapely.box(gx * C.CHUNK, gz * C.CHUNK, (gx + 1) * C.CHUNK, (gz + 1) * C.CHUNK))
    shapely.prepare(region)
    keep = shapely.intersects(region, np.array(boxes))
    chunks = [c for c, k in zip(chunks, keep) if k]
    print(f"[osm] чанков 256м в регионе: {len(chunks)}")
    with open(C.WORK / "region.pkl", "wb") as f:
        pickle.dump({"boundary": boundary.wkb, "region": region.wkb, "chunks": chunks}, f)

    rbox = shapely.box(rminx - 500, rminz - 500, rmaxx + 500, rmaxz + 500)
    supers = sorted({C.super_of_chunk(*c) for c in chunks})
    ss = C.SUPER * C.CHUNK
    bins = defaultdict(list)          # (sx,sz) -> [(kind, tags, wkb)]
    glob = {"buildings": [], "forest": [], "water": [], "bridges": [], "nonbridge_nodes": None,
            "far_water": [], "power_lines": []}

    def st_boxes_for(g):
        minx, minz, maxx, maxz = g.bounds
        for sx in range(math.floor((minx - C.BIN_MARGIN) / ss), math.floor((maxx + C.BIN_MARGIN) / ss) + 1):
            for sz in range(math.floor((minz - C.BIN_MARGIN) / ss), math.floor((maxz + C.BIN_MARGIN) / ss) + 1):
                yield sx, sz

    super_set = set(supers)

    def add_clipped(kind, tags, g):
        for key in st_boxes_for(g):
            if key not in super_set:
                continue
            sx, sz = key
            b = shapely.box(sx * ss - C.BIN_MARGIN, sz * ss - C.BIN_MARGIN,
                            (sx + 1) * ss + C.BIN_MARGIN, (sz + 1) * ss + C.BIN_MARGIN)
            if g.within(b):
                bins[key].append((kind, tags, g.wkb))
            else:
                c = shapely.intersection(g, b)
                if not c.is_empty:
                    bins[key].append((kind, tags, c.wkb))

    def add_point_like(kind, tags, g, pt):
        gx, gz = C.chunk_of(pt.x, pt.y)
        key = C.super_of_chunk(gx, gz)
        if key in super_set:
            bins[key].append((kind, tags, g.wkb))

    n_b = 0
    for tags, w in h.areas:
        g = project(swkb.loads(w, hex=True))
        if not g.is_valid:
            g = shapely.make_valid(g)
            g = shapely.get_parts(g)
            g = shapely.union_all([p for p in g if p.geom_type in ("Polygon", "MultiPolygon")]) if len(g) else None
            if g is None or g.is_empty:
                continue
        if not g.intersects(rbox):
            # всё равно нужна вода для дальнего рельефа
            if "water" in tags or tags.get("natural") == "water" or tags.get("waterway") == "riverbank":
                glob["far_water"].append(g.simplify(20).wkb)
            continue
        is_bld = ("building" in tags and tags.get("building") != "no") or "building:part" in tags
        if is_bld:
            if "building" in tags and tags.get("building") != "no":
                glob["buildings"].append(g.wkb)
            add_point_like("building" if "building:part" not in tags else "part", tags, g, g.representative_point())
            n_b += 1
            # здание может ещё и нести landuse и т.п. — такие случаи редки, игнорируем
            continue
        if tags.get("landuse") == "forest" or tags.get("natural") == "wood":
            glob["forest"].append(g.wkb)
        if tags.get("natural") == "water" or tags.get("waterway") in ("riverbank", "dock") \
                or tags.get("landuse") in ("reservoir", "basin"):
            glob["water"].append(g.wkb)
            glob["far_water"].append(g.simplify(20).wkb)
        if tags.get("railway") == "platform" or tags.get("man_made") in ("pier", "bridge"):
            add_point_like("prism", tags, g, g.representative_point())
            if tags.get("man_made") == "bridge":
                continue
        add_clipped("area", tags, g)

    for tags, w, ids in h.lines:
        g = project(swkb.loads(w, hex=True))
        if not g.intersects(rbox):
            continue
        if tags.get("tunnel") not in (None, "no"):
            continue
        is_bridge = tags.get("bridge") not in (None, "no") and ("highway" in tags or "railway" in tags)
        if is_bridge:
            glob["bridges"].append((tags, g.wkb, ids))
            continue
        if tags.get("waterway") in WATERWAY_LINES:
            glob["water"].append(("line", tags, g.wkb))
        if tags.get("power") in POWER_LINES:
            # целиком, без обрезки по суперплиткам: вершины линии — это опоры,
            # обрезка дала бы ложную «опору» на границе плитки
            glob["power_lines"].append((tags, g.wkb))
            continue
        add_clipped("line", tags, g)

    for tags, lon, lat, nid in h.nodes:
        x, y = _to_local.transform(lon, lat)
        p = shapely.Point(x, -y)
        if not p.intersects(rbox):
            continue
        tags["_id"] = nid
        add_point_like("node", tags, p, p)

    glob["nonbridge_nodes"] = h.nonbridge_nodes
    # для export_data.py: полные (не обрезанные) линии дорог/рельсов с тегами + маршруты
    export_lines = [(t, project(swkb.loads(w, hex=True)).wkb) for t, w, _ in h.lines
                    if "highway" in t or "railway" in t or "public_transport" in t]
    platforms = []
    for wid, t, w in h.platforms:
        c = project(swkb.loads(w, hex=True)).representative_point()
        platforms.append((wid, t, c.x, c.y))
    with open(C.WORK / "export.pkl", "wb") as f:
        pickle.dump({"lines": export_lines, "routes": h.routes, "platforms": platforms,
                     "nodes": [(t, *_to_local.transform(lon, lat), nid) for t, lon, lat, nid in h.nodes]},
                    f, protocol=pickle.HIGHEST_PROTOCOL)
    with open(C.WORK / "global.pkl", "wb") as f:
        pickle.dump(glob, f, protocol=pickle.HIGHEST_PROTOCOL)
    for (sx, sz), items in bins.items():
        with open(C.WORK / "bins" / f"st_{sx}_{sz}.pkl", "wb") as f:
            pickle.dump(items, f, protocol=pickle.HIGHEST_PROTOCOL)
    print(f"[osm] зданий/частей: {n_b}, суперплиток: {len(bins)}, мостов: {len(glob['bridges'])}, "
          f"лесов: {len(glob['forest'])}, вод: {len(glob['water'])} — {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
