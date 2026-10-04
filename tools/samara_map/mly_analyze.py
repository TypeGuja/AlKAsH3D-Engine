"""Mapillary, шаг 4: разбор кадров — цвет фасада и высота карниза по каждому зданию.

На кадр:
  1. SegFormer (ADE20K) размечает пиксели: стена/здание, окно, небо, дерево, дорога, машина…
  2. Камера ставится по SfM-позе Mapillary (computed_*), а где её нет — по GPS и курсу.
     Затем поза подгоняется: нижний пояс стен окрестных домов (0–6 м), отрисованный
     из камеры, совмещается с маской «здание» (сдвиг ≈ поправка курса/тангажа,
     перебор смещений камеры ≈ ошибка GPS).
  3. Из окончательной позы рисуется буфер «чья стена» (стены продлены вверх
     бесконечно, по z-порядку) — так перекрытия домами учитываются сами.
  4. Для каждого здания из select.npz:
     цвет — медиана пикселей «стена/здание» на высоте 2.5 м и выше (окна, деревья,
     машины, небо отброшены), баланс белого по асфальту;
     высота — в столбцах, где над стеной сразу небо, луч камеры пересекается с
     плоскостью стены: это высота карниза над землёй у стены.

Выход: MLY/analysis/*.jsonl — строка на пару (кадр, здание).
Запуск: venv на диске MLY (torch с CUDA, transformers):  G:/samara_mapillary/venv/Scripts/python.exe mly_analyze.py
  --debug N  — разобрать N кадров и сохранить наложения в MLY/debug.
"""
import json
import math
import os
from pathlib import Path
import sys
import time
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor

os.environ.setdefault("OMP_NUM_THREADS", "1")
os.environ.setdefault("HF_HOME", str(Path(os.environ.get("SAMARA_MLY", "G:/samara_mapillary")) / "hf"))
os.environ.setdefault("HF_HUB_OFFLINE", "1")       # модель уже в кэше; проверка обновлений на hub подвисала
import cv2
import numpy as np

import config as C
from mly_common import CAM_H, MLY, facade_edges, img_path, load_buildings

SEG_MODEL = "nvidia/segformer-b2-finetuned-ade-512-512"
SEG_LONG = 512                    # длинная сторона кадра для разметки и анализа
NEAR_R = 80.0                     # стены в этом радиусе рисуются
BAND_H = 6.0                      # нижний пояс стен для подгонки позы (выше — высота неизвестна)
MIN_PX = 60                       # меньше пикселей стены — цвет не берём
WORKERS = 6

# группы классов ADE20K (по именам из config модели)
G_BUILD = {"wall", "building", "house", "skyscraper", "tower", "column", "pillar"}
G_WIN = {"windowpane", "door", "double door", "screen door", "shutter", "balcony", "awning", "signboard"}
G_SKY = {"sky"}
G_VEG = {"tree", "plant", "palm", "grass", "flower"}
G_ROAD = {"road", "sidewalk", "path", "earth", "floor", "dirt track", "runway", "snow", "ground", "land"}
G_CAR = {"car", "truck", "bus", "van", "minibike", "bicycle", "person"}


# ---------------------------------------------------------------- рабочий процесс (геометрия)

W = {}                            # состояние воркера


def _init_worker():
    import pickle
    cv2.setNumThreads(1)                # 6 процессов × потоки OpenCV перегружали 8 ядер
    from scipy.spatial import cKDTree
    sel = np.load(MLY / "select.npz")
    keys = [tuple(k) for k in sel["bld_key"]]
    bmap = {b["key"]: b for b in load_buildings()}
    E0, E1, EN, EB = [], [], [], []
    # ожидаемый верх дома (тег height, этажность из OSM или оценка bld_levels, иначе 2 этажа):
    # выше него пиксели «стены» скорее принадлежат дому позади
    W["btop"] = np.full(len(keys), 9.0)
    for bi, k in enumerate(keys):
        b = bmap.get(k)
        if b is None:
            continue
        W["btop"][bi] = b["height_tagged"] or (b["levels"] or 2) * 3.0 + 0.6
        for p, q, n in facade_edges(b["geom"], min_len=0.5):
            E0.append(p); E1.append(q); EN.append(n); EB.append(bi)
    W["E0"], W["E1"], W["EN"], W["EB"] = map(np.array, (E0, E1, EN, EB))
    W["EM"] = (W["E0"] + W["E1"]) / 2
    W["tree"] = cKDTree(W["EM"])
    g = json.load(open(C.WORK / "grid.json"))
    W["grid"] = g
    W["H"] = np.load(C.WORK / "heights.npy", mmap_mode="r")
    from mly_common import transformer
    W["tr"] = transformer()


def terrain(x, z):
    g, H = W["grid"], W["H"]
    fx = (np.asarray(x, np.float64) - g["x0"]) / g["step"]
    fz = (np.asarray(z, np.float64) - g["z0"]) / g["step"]
    i = np.clip(np.floor(fx).astype(np.int64), 0, H.shape[1] - 2)
    j = np.clip(np.floor(fz).astype(np.int64), 0, H.shape[0] - 2)
    u, v = np.clip(fx - i, 0, 1), np.clip(fz - j, 0, 1)
    return (H[j, i] * (1 - u) * (1 - v) + H[j, i + 1] * u * (1 - v) + H[j + 1, i] * (1 - u) * v + H[j + 1, i + 1] * u * v)


def rot_from_compass(yaw_deg, pitch_deg=0.0):
    """world(ENU)->camera (x вправо, y вниз, z вперёд)."""
    y, p = math.radians(yaw_deg), math.radians(pitch_deg)
    fwd = np.array([math.sin(y) * math.cos(p), math.cos(y) * math.cos(p), math.sin(p)])
    right = np.array([math.cos(y), -math.sin(y), 0.0])
    down = np.cross(fwd, right)
    return np.stack([right, down, fwd])


def to_enu(P):
    """точки карты (X, Y вверх, Z юг) -> ENU (восток, север, вверх)."""
    P = np.asarray(P, np.float64)
    return np.stack([P[..., 0], -P[..., 2], P[..., 1]], -1)


def clip_near(poly, zn=0.3):
    out = []
    n = len(poly)
    for k in range(n):
        a, b = poly[k], poly[(k + 1) % n]
        ia, ib = a[2] >= zn, b[2] >= zn
        if ia:
            out.append(a)
        if ia != ib:
            t = (zn - a[2]) / (b[2] - a[2])
            out.append(a + (b - a) * t)
    return np.array(out) if len(out) >= 3 else None


class Cam:
    def __init__(self, Cpos, R, f, w, h, margin=0):
        self.C, self.R, self.f, self.w, self.h, self.m = Cpos, R, f, w, h, margin

    def project_quads(self, Q):
        """Q: (n,4,3) точки карты -> список (индекс, полигон в пикселях холста, глубина)."""
        c = (to_enu(Q) - to_enu(self.C)) @ self.R.T
        zc = c[..., 2]
        vis = (zc >= 0.3).any(1)
        full = (zc >= 0.3).all(1)
        W_, H_ = self.w + 2 * self.m, self.h + 2 * self.m
        out = []
        idx = np.where(full)[0]
        if len(idx):
            cc = c[idx]
            u = self.f * cc[..., 0] / cc[..., 2] + self.w / 2 + self.m
            v = self.f * cc[..., 1] / cc[..., 2] + self.h / 2 + self.m
            ok = (u.max(1) >= 0) & (u.min(1) <= W_) & (v.max(1) >= 0) & (v.min(1) <= H_)
            dep = np.linalg.norm(cc.mean(1), axis=1)
            for k in np.where(ok)[0]:
                out.append((int(idx[k]), np.stack([u[k], v[k]], 1), float(dep[k])))
        for i in np.where(vis & ~full)[0]:
            poly = clip_near(c[i])
            if poly is None:
                continue
            u = self.f * poly[:, 0] / poly[:, 2] + self.w / 2 + self.m
            v = self.f * poly[:, 1] / poly[:, 2] + self.h / 2 + self.m
            if u.max() < 0 or u.min() > W_ or v.max() < 0 or v.min() > H_:
                continue
            out.append((int(i), np.stack([u, v], 1), float(np.linalg.norm(c[i].mean(0)))))
        return out

    def render_mask(self, Q):
        img = np.zeros((self.h + 2 * self.m, self.w + 2 * self.m), np.float32)
        items = self.project_quads(Q)
        if items:
            polys = [np.round(np.clip(p, -1e4, 1e4) * 4).astype(np.int32) for _, p, _ in items]
            cv2.fillPoly(img, polys, 1.0, lineType=cv2.LINE_8, shift=2)
        return img

    def render(self, Q, vals, dtype=np.float32, fill=0):
        img = np.full((self.h + 2 * self.m, self.w + 2 * self.m), fill, dtype)
        items = self.project_quads(Q)
        items.sort(key=lambda t: -t[2])                    # дальние сначала
        for i, poly, _ in items:
            cv2.fillPoly(img, [np.round(np.clip(poly, -1e4, 1e4) * 4).astype(np.int32)], float(vals[i]), lineType=cv2.LINE_8, shift=2)
        return img

    def rays(self):
        """направления лучей пикселей кадра (без поля) в координатах карты, (h,w,3)."""
        uu, vv = np.meshgrid(np.arange(self.w) + 0.5, np.arange(self.h) + 0.5)
        d = np.stack([(uu - self.w / 2) / self.f, (vv - self.h / 2) / self.f, np.ones_like(uu)], -1)
        e = d @ self.R                                      # camera -> ENU
        return np.stack([e[..., 0], e[..., 2], -e[..., 1]], -1)


def wall_quads(idx, base, top_h):
    E0, E1 = W["E0"][idx], W["E1"][idx]
    g0, g1 = terrain(E0[:, 0], E0[:, 1]), terrain(E1[:, 0], E1[:, 1])
    lo0, lo1 = g0 + base, g1 + base
    hi0, hi1 = g0 + top_h, g1 + top_h
    Q = np.stack([np.stack([E0[:, 0], lo0, E0[:, 1]], 1), np.stack([E1[:, 0], lo1, E1[:, 1]], 1),
                  np.stack([E1[:, 0], hi1, E1[:, 1]], 1), np.stack([E0[:, 0], hi0, E0[:, 1]], 1)], 1)
    return Q


def analyze(job):
    """job: dict(meta, labels (h,w) uint8 групп, rgb (h,w,3) uint8, targets [bld_idx])."""
    if not W:
        _init_worker()
    meta, lab, rgb, targets = job["meta"], job["labels"], job["rgb"], job["targets"]
    h, w = lab.shape
    geo = (meta.get("computed_geometry") or meta.get("geometry"))["coordinates"]
    x, yn = W["tr"].transform(geo[0], geo[1])
    cx, cz = x, -yn
    sfm = "computed_rotation" in meta and meta.get("computed_geometry") is not None
    if sfm:
        R = cv2.Rodrigues(np.array(meta["computed_rotation"], np.float64))[0]
    else:
        R = rot_from_compass(meta.get("computed_compass_angle", meta.get("compass_angle", 0.0)))
    f = float((meta.get("camera_parameters") or [0.85])[0]) * max(w, h)
    cy = float(terrain(cx, cz)) + CAM_H
    near = np.array(W["tree"].query_ball_point([cx, cz], NEAR_R), dtype=np.int64)
    if len(near) == 0:
        return []
    # только стены, обращённые к камере
    facing = ((np.array([cx, cz]) - W["EM"][near]) * W["EN"][near]).sum(1) > 0
    near = near[facing]
    if len(near) == 0:
        return []

    # карта очков для подгонки: стена +1, небо −1.5, дорога −0.7, остальное 0
    S = np.zeros((h, w), np.float32)
    S[(lab == 1) | (lab == 2)] = 1.0
    S[lab == 3] = -1.5
    S[lab == 5] = -0.7
    S2 = cv2.resize(S, (w // 2, h // 2), interpolation=cv2.INTER_AREA)
    Qband = wall_quads(near, -1.0, BAND_H)
    ones = np.ones(len(near))
    m = int(f * math.tan(math.radians(3.0 if sfm else 10.0)))
    shifts = [(0.0, 0.0)] if sfm else [(a, b) for a in (-4, -2, 0, 2, 4) for b in (-4, -2, 0, 2, 4)]
    if sfm:
        shifts = [(a, b) for a in (-1.5, 0, 1.5) for b in (-1.5, 0, 1.5)]
    best = (-1e9, None)
    for ox, oz in shifts:
        cam = Cam(np.array([cx + ox, cy, cz + oz]), R, f, w, h, margin=m)
        Rm = cam.render_mask(Qband)
        if Rm.sum() < 50:
            continue
        # сопоставление на половинном разрешении (в 4–8 раз быстрее), сдвиг — обратно в пиксели кадра
        res = cv2.matchTemplate(cv2.resize(Rm, (Rm.shape[1] // 2, Rm.shape[0] // 2), interpolation=cv2.INTER_AREA), S2, cv2.TM_CCORR)
        _, mx, _, loc = cv2.minMaxLoc(res)
        loc = (loc[0] * 2, loc[1] * 2)
        mx *= 4
        pen = 0.002 * (ox * ox + oz * oz) * Rm.sum() / 25     # не уходить далеко от GPS без нужды
        if mx - pen > best[0]:
            best = (mx - pen, (ox, oz, loc, Rm))
    if best[1] is None:
        return []
    ox, oz, (lx, ly), Rm = best[1]
    # сдвиг холста (lx - m, ly - m) == поворот камеры; переводим в поправку позы (малые углы)
    du, dv = lx - m, ly - m
    yaw_c = math.atan2(du, f)
    pit_c = math.atan2(dv, f)
    Ry = cv2.Rodrigues(np.array([0.0, -yaw_c, 0.0]))[0]      # поворот вокруг оси «вниз» камеры
    Rp = cv2.Rodrigues(np.array([pit_c, 0.0, 0.0]))[0]
    R2 = Rp @ Ry @ R
    Cpos = np.array([cx + ox, cy, cz + oz])
    cam = Cam(Cpos, R2, f, w, h)
    band = cam.render_mask(Qband) > 0
    fit = float(((lab[band] == 1) | (lab[band] == 2)).mean()) if band.any() else 0.0
    sky_in_band = float((lab[band] == 3).mean()) if band.any() else 1.0

    # буфер «чья стена»: индекс стены (в near) + 1, стены до 300 м вверх
    Qall = wall_quads(near, -1.0, 300.0)
    ebuf = cam.render(Qall, np.arange(1, len(near) + 1), np.float32).astype(np.int32)
    rays = cam.rays()
    # высота точки стены под каждым пикселем
    k = np.maximum(ebuf - 1, 0)
    e_idx = near[k]
    p0 = W["E0"][e_idx]; nrm = W["EN"][e_idx]
    dxz = rays[..., [0, 2]]
    den = (dxz * nrm).sum(-1)
    t = ((p0 - np.array([Cpos[0], Cpos[2]])) * nrm).sum(-1) / np.where(np.abs(den) > 1e-6, den, 1e-6)
    hx, hz = Cpos[0] + t * dxz[..., 0], Cpos[2] + t * dxz[..., 1]
    em = W["EM"][e_idx]
    gwall = terrain(em[..., 0], em[..., 1])
    hgt = Cpos[1] + t * rays[..., 1] - gwall
    hgt[ebuf == 0] = np.nan
    bbuf = np.where(ebuf > 0, W["EB"][e_idx], -1)

    # баланс белого по асфальту (нейтрально-серый)
    road = (lab == 5)
    road[: h // 2] = False
    lin = (rgb.astype(np.float32) / 255.0) ** 2.2
    gains = np.ones(3, np.float32)
    road_lum = float("nan")
    if road.sum() > 200:
        rm = np.median(lin[road], axis=0)
        road_lum = float(rm.mean())
        gains = np.clip(rm.mean() / np.maximum(rm, 1e-4), 0.85, 1.18)
    sky_px = lab == 3
    sky_lum = float(np.median(lin[sky_px].mean(1))) if sky_px.sum() > 200 else float("nan")

    out = []
    for b in targets:
        mb = bbuf == b
        if mb.sum() < MIN_PX // 2:
            out.append(dict(b=int(b), npx=0, fit=fit))
            continue
        top = float(W["btop"][b])
        wallpx = mb & (lab == 1) & (hgt > 2.5) & (hgt < max(top - 0.3, 3.5))
        r = dict(b=int(b), fit=round(fit, 3), sky_band=round(sky_in_band, 3), sfm=sfm, top=round(top, 1),
                 npx=int(wallpx.sum()), nwin=int((mb & (lab == 2)).sum()),
                 dist=round(float(np.nanmedian(np.where(mb, t, np.nan))), 1),
                 road_lum=round(road_lum, 4), sky_lum=round(sky_lum, 4))
        if r["npx"] >= MIN_PX:
            px = lin[wallpx]
            l = px.mean(1)
            keep = (l > np.percentile(l, 10)) & (l < np.percentile(l, 90)) & (px.max(1) < 0.97)
            px = px[keep] if keep.sum() > 20 else px
            r["rgb"] = [round(float(v), 4) for v in np.median(px, axis=0)]
            r["rgb_wb"] = [round(float(v), 4) for v in np.median(px, axis=0) * gains]
            r["hmin"] = round(float(np.nanmin(hgt[wallpx])), 1)
        # высота: в каждом столбце — самая верхняя «стена» этого дома, над ней небо
        cols = np.where(mb.any(0))[0]
        hs, cens = [], []
        for u in cols[::2]:
            col = mb[:, u]
            rows = np.where(col & ((lab[:, u] == 1) | (lab[:, u] == 2)))[0]
            if len(rows) < 4:
                continue
            top = rows.min()
            # стена должна быть сплошной ниже top (≥ 70% пикселей столбца дома — стена/окно)
            seg_rows = np.arange(top, rows.max() + 1)
            if np.mean(col[seg_rows] & ((lab[seg_rows, u] == 1) | (lab[seg_rows, u] == 2))) < 0.7:
                continue
            if top <= 1:
                v = hgt[top, u]
                if np.isfinite(v):
                    cens.append(float(v))
                continue
            above = lab[max(0, top - 4):top, u]
            if len(above) and np.mean(above == 3) >= 0.75:
                v = hgt[top, u]
                if np.isfinite(v) and 2 < v < max(2.2 * top, top + 9.0):
                    hs.append(float(v))
        if len(hs) >= 4:
            r["h"] = round(float(np.median(hs)), 2)
            r["h_n"] = len(hs)
            r["h_iqr"] = round(float(np.subtract(*np.percentile(hs, [75, 25]))), 2)
        if len(cens) >= 4:
            r["h_ge"] = round(float(np.median(cens)), 1)
        out.append(r)
    if job.get("debug"):
        out.append(dict(_debug=True, bbuf=bbuf, band=band))
    return out


# ---------------------------------------------------------------- главный процесс (GPU)

def load_meta():
    meta = {}
    for f in sorted((MLY / "meta").glob("*.jsonl")):
        for line in open(f, encoding="utf-8"):
            d = json.loads(line)
            if d.get("id"):
                meta[int(d["id"])] = d
    return meta


def main(debug=0):
    import torch
    from transformers import SegformerForSemanticSegmentation
    t0 = time.time()
    sel = np.load(MLY / "select.npz")
    img_id = sel["img_id"]
    targets = {}
    for pi, pb in zip(sel["pair_img"], sel["pair_bld"]):
        targets.setdefault(int(img_id[pi]), []).append(int(pb))
    meta = load_meta()
    out_dir = MLY / "analysis"
    out_dir.mkdir(exist_ok=True)
    done = set()
    for f in out_dir.glob("*.jsonl"):
        for line in open(f, encoding="utf-8"):
            done.add(json.loads(line)["img"])
    todo = [i for i in targets if i in meta and i not in done and img_path(i).exists()]
    if debug:
        rng = np.random.default_rng(1)
        todo = list(rng.choice(todo, size=min(debug, len(todo)), replace=False))
    print(f"[analyze] кадров с метаданными {len(meta)}, готово {len(done)}, к разбору {len(todo)}", flush=True)

    dev = "cuda" if torch.cuda.is_available() else "cpu"
    torch.backends.cudnn.benchmark = True
    model = SegformerForSemanticSegmentation.from_pretrained(SEG_MODEL).to(dev).eval()
    if dev == "cuda":
        model = model.half()
    names = {int(k): v.split(";")[0].strip().lower() for k, v in model.config.id2label.items()}
    lut = np.zeros(256, np.uint8)                   # 0 прочее, 1 стена, 2 окно, 3 небо, 4 растительность, 5 дорога, 6 машина
    for k, n in names.items():
        for code, grp in ((1, G_BUILD), (2, G_WIN), (3, G_SKY), (4, G_VEG), (5, G_ROAD), (6, G_CAR)):
            if n in grp:
                lut[k] = code
    print(f"[analyze] модель {SEG_MODEL} на {dev}; групп классов: " +
          ", ".join(f"{c}:{int((lut == c).sum())}" for c in range(1, 7)), flush=True)
    mean = torch.tensor([0.485, 0.456, 0.406], device=dev).view(1, 3, 1, 1)
    std = torch.tensor([0.229, 0.224, 0.225], device=dev).view(1, 3, 1, 1)

    def load(iid):
        im = cv2.imread(str(img_path(iid)), cv2.IMREAD_COLOR)
        if im is None:
            return iid, None
        h, w = im.shape[:2]
        s = SEG_LONG / max(h, w)
        im = cv2.resize(im, (int(round(w * s)) // 4 * 4, int(round(h * s)) // 4 * 4), interpolation=cv2.INTER_AREA)
        return iid, cv2.cvtColor(im, cv2.COLOR_BGR2RGB)

    io = ThreadPoolExecutor(4)
    pool = ProcessPoolExecutor(WORKERS, initializer=_init_worker)
    pending = []
    fout = open(out_dir / f"part_{int(time.time())}.jsonl", "a", encoding="utf-8") if not debug else None
    n_done, n_pairs = 0, 0
    dbg_dir = MLY / "debug"
    if debug:
        dbg_dir.mkdir(exist_ok=True)

    def drain(block):
        nonlocal n_done, n_pairs
        # block: ждём, пока в работе не останется WORKERS*3 кадров (GPU и воркеры заняты одновременно)
        while pending and ((block and len(pending) > WORKERS * 3) or pending[0][1].done()):
            iid, fut, rgb, lab = pending.pop(0)
            try:
                res = fut.result()
            except Exception as ex:
                print(f"  кадр {iid}: {ex!r}", flush=True)
                res = []
            dbg = [r for r in res if r.get("_debug")]
            res = [r for r in res if not r.get("_debug")]
            if fout:
                for r in res:
                    r["img"] = iid
                    fout.write(json.dumps(r) + "\n")
                if not res:
                    fout.write(json.dumps({"img": iid, "b": -1}) + "\n")
            n_done += 1
            n_pairs += sum(1 for r in res if "rgb" in r)
            if dbg:
                save_debug(dbg_dir, iid, rgb, lab, dbg[0], res)
            if n_done % 1000 == 0:
                fout and fout.flush()
                el = time.time() - t0
                print(f"[analyze] {n_done}/{len(todo)} кадров, пар с цветом {n_pairs}, {n_done/el:.1f} кадр/с — {el/60:.0f} мин", flush=True)

    BATCH = 16
    PREFETCH = 256                  # читаем порциями: io.map на весь список держал бы все кадры в памяти
    batch = []
    for c0 in range(0, len(todo), PREFETCH):
        for item in io.map(load, todo[c0:c0 + PREFETCH]):
            if item[1] is not None:
                batch.append(item)
            if len(batch) < BATCH:
                continue
            run_batch(batch, model, mean, std, dev, lut, meta, targets, pool, pending, debug)
            batch = []
            drain(len(pending) > WORKERS * 6)
    if batch:
        run_batch(batch, model, mean, std, dev, lut, meta, targets, pool, pending, debug)
    drain(True)
    if fout:
        fout.close()
    print(f"[analyze] готово: {n_done} кадров, пар с цветом {n_pairs} — {(time.time()-t0)/60:.0f} мин")


def run_batch(batch, model, mean, std, dev, lut, meta, targets, pool, pending, debug):
    import torch
    groups = {}
    for iid, rgb in batch:                           # одинаковый размер — в один тензор
        groups.setdefault(rgb.shape, []).append((iid, rgb))
    for shape, items in groups.items():
        x = torch.from_numpy(np.stack([r for _, r in items])).to(dev).permute(0, 3, 1, 2).float() / 255.0
        x = (x - mean) / std
        with torch.no_grad():
            lg = model(pixel_values=x.half() if dev == "cuda" else x).logits
            # 150 карт классов в полный размер — ~2.5 ГБ на пачку; argmax на половинном и ×2 ближайшим
            lg = torch.nn.functional.interpolate(lg, size=(shape[0] // 2, shape[1] // 2), mode="bilinear", align_corners=False)
            cls = lg.argmax(1).to(torch.uint8).cpu().numpy()
        for (iid, rgb), c in zip(items, cls):
            lab = cv2.resize(lut[c], (shape[1], shape[0]), interpolation=cv2.INTER_NEAREST)
            job = dict(meta=meta[iid], labels=lab, rgb=rgb, targets=targets[iid], debug=bool(debug))
            pending.append((iid, pool.submit(analyze, job), rgb if debug else None, lab if debug else None))


def save_debug(d, iid, rgb, lab, dbg, res):
    pal = np.array([[0, 0, 0], [200, 120, 60], [60, 160, 255], [150, 220, 255], [40, 160, 40], [90, 90, 90], [220, 40, 200]], np.uint8)
    seg = pal[lab]
    over = rgb.copy()
    bb = dbg["bbuf"]
    tg = {r["b"] for r in res}
    rng = np.random.default_rng(0)
    for b in np.unique(bb):
        if b < 0:
            continue
        col = np.array([255, 255, 0]) if b in tg else rng.integers(60, 200, 3)
        m = bb == b
        edge = m ^ cv2.erode(m.astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
        over[edge] = col
    over[dbg["band"] & ~cv2.erode(dbg["band"].astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)] = (255, 0, 0)
    txt = "; ".join(f"b{r['b']} h={r.get('h','-')} ge={r.get('h_ge','-')} px={r.get('npx',0)} fit={r.get('fit',0)}" for r in res)
    sw = np.zeros((40, rgb.shape[1], 3), np.uint8)
    x0 = 0
    for r in res:
        if "rgb_wb" in r:
            c = (np.clip(np.array(r["rgb_wb"]), 0, 1) ** (1 / 2.2) * 255).astype(np.uint8)
            sw[:, x0:x0 + 60] = c
            x0 += 62
    im = np.vstack([np.hstack([over, seg]), np.hstack([sw, np.zeros_like(sw)])])
    cv2.imwrite(str(d / f"{iid}.jpg"), cv2.cvtColor(im, cv2.COLOR_RGB2BGR))
    with open(d / "debug.txt", "a", encoding="utf-8") as f:
        f.write(f"{iid}: {txt}\n")


if __name__ == "__main__":
    dbg = 0
    if "--debug" in sys.argv:
        dbg = int(sys.argv[sys.argv.index("--debug") + 1])
    main(dbg)
