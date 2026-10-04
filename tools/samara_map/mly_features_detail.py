"""Mapillary, объекты — подробности через API: на скольких кадрах распознан и куда смотрит.

В тайлах у объекта только вид и точка. API по id отдаёт ещё:
  images            — кадры, на которых объект распознан (с точками съёмки): настоящий знак
                      или фонарь виден с многих проездов, мусор (отражение, реклама, вывеска) —
                      на одном-двух;
  aligned_direction — азимут, куда смотрит лицо объекта (в сторону снимавших его камер).

Качаются только нужные карте виды (mly_features.CLASSES); опоры — только у трамвайных и
троллейбусных линий (остальные карта не использует). Возобновляемо.
Выход: MLY/features_detail/*.jsonl — {id, n, dir, cams: [[lon, lat], ...]}
"""
import glob
import json
import math
import pickle
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np
import requests

import config as C
from mly_common import MLY, lonlat_to_xz, token
from mly_features import CLASSES

BATCH = 50
PAR = 8
FIELDS = "id,aligned_direction,images"


def wanted_ids():
    """id нужных объектов из тайлов: [(id, класс, x, z)]."""
    import mapbox_vector_tile as mvt
    out = {}
    Z = 14
    for layer in ("mly_map_feature_point", "mly_map_feature_traffic_sign"):
        for f in glob.glob(str(MLY / layer / "*.mvt")):
            tx, ty = (int(v) for v in f.replace("\\", "/").rsplit("/", 1)[1][:-4].split("_"))
            for lay in mvt.decode(open(f, "rb").read()).values():
                for ft in lay["features"]:
                    cls = CLASSES.get(ft["properties"].get("value"))
                    if cls is None:
                        continue
                    gx, gy = ft["geometry"]["coordinates"]
                    lon = (tx + gx / 4096) / 2 ** Z * 360 - 180
                    n = math.pi - 2 * math.pi * (ty + 1 - gy / 4096) / 2 ** Z
                    out[int(ft["properties"]["id"])] = (cls, lon, math.degrees(math.atan(math.sinh(n))))
    ids = list(out)
    a = np.array([out[i][1:] for i in ids])
    x, z = lonlat_to_xz(a[:, 0], a[:, 1])
    return [(i, out[i][0], float(xx), float(zz)) for i, xx, zz in zip(ids, x, z)]


def near_wires(items):
    """Опоры — только в 12 м от линий трамвая и троллейбуса (по бинам extract_osm)."""
    import shapely
    from shapely import wkb as swkb
    lines = []
    for f in sorted((C.WORK / "bins").glob("st_*.pkl")):
        for kind, t, w in pickle.load(open(f, "rb")):
            if kind == "line" and (t.get("railway") == "tram" or t.get("trolley_wire") == "yes"):
                lines.append(swkb.loads(w))
    if not lines:
        return set()
    zone = shapely.union_all([l.buffer(12.0) for l in lines])
    shapely.prepare(zone)
    poles = [(i, x, z) for i, c, x, z in items if c == "pole"]
    P = np.array([(x, z) for _, x, z in poles]).reshape(-1, 2)
    ok = shapely.contains_xy(zone, P[:, 0], P[:, 1]) if len(P) else np.zeros(0, bool)
    return {poles[k][0] for k in np.where(ok)[0]}


def main():
    t0 = time.time()
    items = wanted_ids()
    keep_poles = near_wires(items)
    ids = [i for i, c, x, z in items if c != "pole" or i in keep_poles]
    # сначала знаки и светофоры, потом фонари, мебель, опоры
    order = {"sign": 0, "signal": 1, "signal_ped": 1, "crosswalk": 2, "street_light": 3, "pole": 5}
    cls_of = {i: c for i, c, x, z in items}
    ids.sort(key=lambda i: order.get(cls_of[i].split("_")[0] if cls_of[i].startswith("sign_") else cls_of[i], 4))
    out_dir = MLY / "features_detail"
    out_dir.mkdir(exist_ok=True)
    done = set()
    for f in out_dir.glob("*.jsonl"):
        for line in open(f, encoding="utf-8"):
            done.add(json.loads(line)["id"])
    todo = [i for i in ids if i not in done]
    print(f"[detail] объектов {len(items)}, нужных {len(ids)} (опор у контактной сети {len(keep_poles)}), "
          f"готово {len(done)}, осталось {len(todo)} — {time.time()-t0:.0f}s", flush=True)
    tok = token()
    sess = requests.Session()
    lock = threading.Lock()
    fout = open(out_dir / f"part_{int(time.time())}.jsonl", "a", encoding="utf-8")
    stat = dict(n=0, got=0)

    def work(chunk):
        for attempt in range(6):
            try:
                r = sess.get("https://graph.mapillary.com/", params={"ids": ",".join(map(str, chunk)),
                                                                     "fields": FIELDS, "access_token": tok}, timeout=90)
                if r.status_code == 200:
                    j = r.json()
                    break
                if r.status_code in (429, 500, 502, 503, 504):
                    time.sleep(2 ** attempt)
                    continue
                # пачка с битым id целиком отклоняется — по одному
                if len(chunk) > 1:
                    for i in chunk:
                        work([i])
                    return
                j = {}
                break
            except requests.RequestException:
                time.sleep(2 ** attempt)
        else:
            return
        rows = []
        for i in chunk:
            d = j.get(str(i))
            if d is None:
                rows.append({"id": i, "n": 0, "dir": None, "cams": []})
                continue
            cams = [im["geometry"]["coordinates"] for im in (d.get("images") or {}).get("data", []) if im.get("geometry")]
            rows.append({"id": i, "n": len(cams), "dir": d.get("aligned_direction"),
                         "cams": [[round(c[0], 7), round(c[1], 7)] for c in cams[:40]]})
        with lock:
            for row in rows:
                fout.write(json.dumps(row) + "\n")
            stat["n"] += len(chunk)
            stat["got"] += sum(1 for row in rows if row["n"] > 0)
            if stat["n"] % 5000 < BATCH:
                fout.flush()
                el = time.time() - t0
                print(f"[detail] {stat['n']}/{len(todo)}, с кадрами {stat['got']}, {stat['n']/max(el,1):.0f}/с — {el/60:.0f} мин", flush=True)

    chunks = [todo[k:k + BATCH] for k in range(0, len(todo), BATCH)]
    with ThreadPoolExecutor(PAR) as ex:
        list(ex.map(work, chunks))
    fout.close()
    print(f"[detail] готово: {stat['n']} объектов — {(time.time()-t0)/60:.0f} мин")


if __name__ == "__main__":
    main()
