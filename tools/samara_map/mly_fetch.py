"""Mapillary, шаг 3: параметры камер и снимки thumb_1024 для кадров из select.npz.

Метаданные — пачками по 50 через Graph API (computed_* — уточнённые SfM-ом
положение/ориентация; есть не у всех кадров). Ссылки на снимки подписанные и
протухают, поэтому снимки качаются сразу за своей пачкой метаданных.
Возобновляемо: готовые пачки в MLY/meta/*.jsonl, снимки в MLY/img.
Стоп, если на диске MLY осталось < MIN_FREE_GB или снимков > MAX_GB.
"""
import json
import shutil
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np
import requests

from mly_common import IMG, MLY, img_path, token

BATCH = 50
FIELDS = ("id,width,height,camera_type,camera_parameters,computed_compass_angle,compass_angle,"
          "computed_geometry,geometry,computed_rotation,captured_at,thumb_1024_url")
MIN_FREE_GB = 60
MAX_GB = 90
BATCH_PAR = 8              # пачек метаданных одновременно (по 6 потоков на снимки каждой)

sess = requests.Session()


def get_json(url, params):
    for attempt in range(8):
        try:
            r = sess.get(url, params=params, timeout=60)
            if r.status_code == 200:
                return r.json()
            if r.status_code in (429, 500, 502, 503, 504):
                time.sleep(2 ** attempt)
                continue
            print("  http", r.status_code, r.text[:200], flush=True)
            return None
        except requests.RequestException:
            time.sleep(2 ** attempt)
    return None


def download(item):
    p = img_path(item["id"])
    if p.exists():
        return p.stat().st_size
    url = item.get("thumb_1024_url")
    if not url:
        return 0
    for attempt in range(5):
        try:
            r = sess.get(url, timeout=60)
            if r.status_code == 200 and r.content[:2] == b"\xff\xd8":
                p.parent.mkdir(parents=True, exist_ok=True)
                tmp = p.with_suffix(".part")
                tmp.write_bytes(r.content)
                tmp.replace(p)
                return len(r.content)
        except requests.RequestException:
            pass
        time.sleep(2 ** attempt)
    return 0


def main():
    t0 = time.time()
    sel = np.load(MLY / "select.npz")
    ids = sel["img_id"]
    # сначала кадры, которые нужны большему числу зданий
    need = np.bincount(sel["pair_img"], minlength=len(ids))
    order = np.argsort(-need, kind="stable")
    ids = ids[order]
    meta_dir = MLY / "meta"
    meta_dir.mkdir(parents=True, exist_ok=True)
    IMG.mkdir(parents=True, exist_ok=True)
    tok = token()
    nb = (len(ids) + BATCH - 1) // BATCH
    done = set()
    for sp in meta_dir.glob("*.jsonl"):
        for line in open(sp, encoding="utf-8"):
            done.add(json.loads(line)["_batch"])
    todo = [bi for bi in range(nb) if bi not in done]
    print(f"[fetch] пачек {nb}, готово {len(done)}, осталось {len(todo)}", flush=True)
    lock = threading.Lock()
    inner = ThreadPoolExecutor(BATCH_PAR * 6)
    stat = dict(bytes=0, imgs=0, n=0, stop=None)

    def work(bi):
        if stat["stop"]:
            return
        free = shutil.disk_usage(MLY).free / 2 ** 30
        if free < MIN_FREE_GB:
            stat["stop"] = f"на диске свободно {free:.0f} ГБ"
            return
        if stat["bytes"] / 2 ** 30 > MAX_GB:
            stat["stop"] = f"скачано {MAX_GB} ГБ"
            return
        chunk = ids[bi * BATCH:(bi + 1) * BATCH]
        j = get_json("https://graph.mapillary.com/images",
                     {"access_token": tok, "image_ids": ",".join(map(str, chunk)), "fields": FIELDS})
        if j is None or "data" not in j:
            print(f"[fetch] пачка {bi}: нет ответа, пропуск", flush=True)
            return
        data = j["data"]
        sizes = list(inner.map(download, data))
        with lock:
            stat["bytes"] += sum(sizes)
            stat["imgs"] += sum(s > 0 for s in sizes)
            stat["n"] += 1
            with open(meta_dir / f"{bi // 1000:04d}.jsonl", "a", encoding="utf-8") as f:
                for d in data:
                    d.pop("thumb_1024_url", None)
                    d["_batch"] = bi
                    f.write(json.dumps(d) + "\n")
                if not data:
                    f.write(json.dumps({"_batch": bi, "id": None}) + "\n")
            if stat["n"] % 50 == 0:
                el = time.time() - t0
                print(f"[fetch] пачек {stat['n']}/{len(todo)}, снимков {stat['imgs']}, {stat['bytes']/2**30:.1f} ГБ, "
                      f"{stat['imgs']/max(el,1):.1f}/с, свободно {free:.0f} ГБ — {el/60:.0f} мин", flush=True)

    with ThreadPoolExecutor(BATCH_PAR) as ex:
        list(ex.map(work, todo))
    if stat["stop"]:
        print(f"[fetch] стоп: {stat['stop']}", flush=True)
    got_bytes, done_imgs = stat["bytes"], stat["imgs"]
    print(f"[fetch] готово: снимков {done_imgs}, {got_bytes/2**30:.1f} ГБ — {(time.time()-t0)/60:.0f} мин")


if __name__ == "__main__":
    if len(sys.argv) > 1:
        MAX_GB = float(sys.argv[1])
    main()
