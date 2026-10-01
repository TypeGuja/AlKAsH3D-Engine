"""Данные OSM без геометрии в мире — для трафика, навигации, транспорта, UI.

Выход (samara_map/data/), координаты — как у карты: x (восток), y (высота по
рельефу), z (юг), метры:
  roads.json   — все highway-линии: класс, название, maxspeed, полосы
                 (lanes, lanes:forward/backward, turn:lanes*), oneway, ширина,
                 покрытие, освещение, тротуары, мост/слой, троллейбусные провода;
  rails.json   — ж/д и трамвайные пути;
  pois.json    — магазины/заведения/офисы/достопримечательности: название, тип,
                 часы работы (opening_hours), бренд, адрес, если есть;
  transit.json — остановки (stop_position/platform/bus_stop/tram_stop) и маршруты
                 автобусов/троллейбусов/трамваев: номер, название, от/до, порядок
                 остановок и линия маршрута по путям.

Запуск после extract_osm.py и terrain.py: python export_data.py
"""
import json
import pickle

import numpy as np
from shapely import wkb as swkb

import config as C
import terrain as T

ROAD_KEYS = ("highway", "name", "ref", "maxspeed", "lanes", "lanes:forward", "lanes:backward", "turn:lanes",
             "turn:lanes:forward", "turn:lanes:backward", "oneway", "width", "surface", "lit", "sidewalk",
             "bridge", "tunnel", "layer", "trolley_wire", "junction", "service", "footway", "incline", "step_count")
POI_KEYS = ("name", "brand", "shop", "amenity", "office", "craft", "tourism", "historic", "leisure", "opening_hours",
            "addr:street", "addr:housenumber", "operator", "level", "start_date", "memorial", "artwork_type")
STOP_KEYS = ("name", "public_transport", "highway", "railway", "bus", "trolleybus", "tram", "shelter", "bench",
             "ref", "operator")


def main():
    exp = pickle.load(open(C.WORK / "export.pkl", "rb"))
    gj = json.load(open(C.WORK / "grid.json"))
    H = np.load(C.WORK / "heights.npy")

    def xyz(coords):
        co = np.asarray(coords, np.float64)[:, :2]
        y = T.grid_height(H, gj["x0"], gj["z0"], co[:, 0], co[:, 1])
        return np.round(np.column_stack([co[:, 0], y, co[:, 1]]), 2).tolist()

    out = C.OUT / "data"
    out.mkdir(exist_ok=True)

    roads, rails, way_geom = [], [], {}
    for t, w in exp["lines"]:
        g = swkb.loads(w)
        if g.geom_type != "LineString":
            continue
        pts = xyz(g.coords)
        wid = t.get("_wid")
        way_geom[wid] = pts
        if "highway" in t:
            roads.append({"id": wid, **{k: t[k] for k in ROAD_KEYS if k in t}, "points": pts})
        elif "railway" in t:
            rails.append({"id": wid, "railway": t["railway"], **{k: t[k] for k in ("name", "electrified", "bridge", "tunnel") if k in t}, "points": pts})
    json.dump(roads, open(out / "roads.json", "w", encoding="utf-8"), ensure_ascii=False)
    json.dump(rails, open(out / "rails.json", "w", encoding="utf-8"), ensure_ascii=False)

    pois, stops, node_pos = [], {}, {}
    for t, x, y_north, nid in exp["nodes"]:
        z = -y_north                        # Transformer даёт (x, север) -> z = -север
        y = float(T.grid_height(H, gj["x0"], gj["z0"], np.array([x]), np.array([z]))[0])
        p = [round(x, 2), round(y, 2), round(z, 2)]
        node_pos[nid] = p
        if t.get("public_transport") in ("stop_position", "platform") or t.get("highway") == "bus_stop" \
                or t.get("railway") == "tram_stop":
            stops[nid] = {"id": nid, **{k: t[k] for k in STOP_KEYS if k in t}, "pos": p}
        elif any(k in t for k in ("shop", "office", "craft", "tourism", "historic")) or \
                (t.get("amenity") and t.get("name")):
            pois.append({"id": nid, **{k: t[k] for k in POI_KEYS if k in t}, "pos": p})
    json.dump(pois, open(out / "pois.json", "w", encoding="utf-8"), ensure_ascii=False)

    # платформы-полигоны (PTv2: остановка маршрута — линия/полигон платформы)
    plat = {}
    for wid, t, x, z in exp.get("platforms", []):
        y = float(T.grid_height(H, gj["x0"], gj["z0"], np.array([x]), np.array([z]))[0])
        plat[wid] = {"id": wid, "kind": "platform_area", **{k: t[k] for k in STOP_KEYS if k in t},
                     "pos": [round(x, 2), round(y, 2), round(z, 2)]}
    for t, w in exp["lines"]:
        if t.get("public_transport") == "platform" and t.get("_wid") in way_geom:
            g = way_geom[t["_wid"]]
            plat[t["_wid"]] = {"id": t["_wid"], "kind": "platform_line", **{k: t[k] for k in STOP_KEYS if k in t},
                               "pos": g[len(g) // 2]}

    routes = []
    for t, members in exp["routes"]:
        st = []
        for typ, ref, role in members:
            if typ == "n" and ref in node_pos:
                st.append({"id": ref, "role": role, "pos": node_pos[ref], "name": stops.get(ref, {}).get("name")})
            elif typ == "w" and role.startswith("platform") and ref in plat:
                st.append({"id": ref, "role": role, "pos": plat[ref]["pos"], "name": plat[ref].get("name")})
        ways = [ref for typ, ref, role in members if typ == "w" and role in ("", "forward", "backward")]
        geoms = [way_geom[w] for w in ways if w in way_geom]
        path = []
        for k, g in enumerate(geoms):
            def d2(a, b):
                return (a[0] - b[0]) ** 2 + (a[2] - b[2]) ** 2
            if not path:
                # первый путь ориентируем по второму: его конец должен смыкаться со вторым
                if len(geoms) > 1:
                    nxt = geoms[1]
                    if min(d2(g[0], nxt[0]), d2(g[0], nxt[-1])) < min(d2(g[-1], nxt[0]), d2(g[-1], nxt[-1])):
                        g = g[::-1]
                path = list(g)
                continue
            if d2(path[-1], g[-1]) < d2(path[-1], g[0]):
                g = g[::-1]                 # путь в отношении может идти «против» своей геометрии
            path += g[1:] if d2(path[-1], g[0]) < 1.0 else g
        if not st and not path:
            continue
        # стыки соседних путей: разрыв > 1 м — дыра в самом отношении OSM
        # (неполный маршрут или его часть за пределами карты)
        breaks = 0
        for g1, g2 in zip(geoms[:-1], geoms[1:]):
            e1, e2 = (g1[0], g1[-1]), (g2[0], g2[-1])
            if min((a[0] - b[0]) ** 2 + (a[2] - b[2]) ** 2 for a in e1 for b in e2) > 1.0:
                breaks += 1
        t = {**t, "_breaks": breaks}
        routes.append({**{k: t[k] for k in ("route", "ref", "name", "from", "to", "operator", "colour", "network") if k in t},
                       "stops": st, "ways": ways, "path": path, "ways_missing": sum(1 for w in ways if w not in way_geom),
                       "breaks": t["_breaks"]})
    json.dump({"stops": list(stops.values()) + list(plat.values()), "routes": routes}, open(out / "transit.json", "w", encoding="utf-8"), ensure_ascii=False)
    print(f"[data] дорог {len(roads)}, путей {len(rails)}, POI {len(pois)}, остановок {len(stops)}, маршрутов {len(routes)}")


if __name__ == "__main__":
    main()
