"""Mapillary, шаг 2: какие кадры смотреть для каждого здания.

Для каждой стены (отрезок контура ≥ 4 м) — дневные кадры в 6–40 м с её внешней
стороны, у которых стена попадает в кадр (курс камеры ±32° от направления на
середину стены; курс в тайлах неточный, ±10–15°) и видна не слишком вскользь.
Луч камера→стена не должен пересекать другие здания. Из кандидатов на здание
берутся лучшие K, не больше 2 с одного проезда (разные дни/свет усредняются),
с предпочтением кадров, уже выбранных для соседей — меньше качать.

Выход: MLY/select.npz — img_id, img_x, img_z, img_compass (уникальные кадры),
pair_img, pair_bld (индексы), bld_key (N×2 float) — ключи зданий как в bld_levels.
"""
import math
import time

import numpy as np
import shapely
from scipy.spatial import cKDTree

from mly_common import MLY, facade_edges, load_buildings

R_MAX, R_MIN = 40.0, 6.0
HALF_FOV = 32.0
MIN_COS_INC = 0.2           # не более ~78° от нормали стены
K_PER_BLD = 10
PER_SEQ = 2
CAND = 40                   # кандидатов на здание до проверки перекрытия
MIN_AREA = 25.0


def main():
    t0 = time.time()
    B = [b for b in load_buildings() if b["geom"].area >= MIN_AREA]
    print(f"[select] зданий {len(B)} — {time.time()-t0:.0f}s", flush=True)
    E0, E1, EN, EB = [], [], [], []
    for i, b in enumerate(B):
        for p, q, n in facade_edges(b["geom"]):
            E0.append(p); E1.append(q); EN.append(n); EB.append(i)
    E0, E1, EN, EB = map(np.array, (E0, E1, EN, EB))
    EM = (E0 + E1) / 2
    EL = np.hypot(*(E1 - E0).T)
    print(f"[select] стен {len(EB)}", flush=True)

    ix = np.load(MLY / "index.npz")
    ok = (ix["sun"] > 5) & ~ix["pano"] & np.isfinite(ix["compass"])
    I = {k: ix[k][ok] for k in ("x", "z", "compass", "quality", "id", "seq")}
    P = np.column_stack([I["x"], I["z"]]).astype(np.float64)
    tree = cKDTree(P)
    print(f"[select] дневных кадров {len(P)} — {time.time()-t0:.0f}s", flush=True)

    # кандидаты: (здание, кадр, оценка)
    cb, ci, cs = [], [], []
    step = 4000
    for s in range(0, len(EB), step):
        sl = slice(s, s + step)
        lists = tree.query_ball_point(EM[sl], R_MAX)
        cnt = np.array([len(l) for l in lists])
        if cnt.sum() == 0:
            continue
        e = np.repeat(np.arange(s, s + len(lists)), cnt)
        j = np.concatenate([np.asarray(l, dtype=np.int64) for l in lists if l])
        v = EM[e] - P[j]                                 # камера -> стена
        d = np.hypot(v[:, 0], v[:, 1])
        out = -(v * EN[e]).sum(1)                        # насколько камера снаружи
        cinc = out / np.maximum(d, 1e-6)
        brg = np.degrees(np.arctan2(v[:, 0], -v[:, 1])) % 360   # азимут: X восток, Z юг
        dh = np.abs((brg - I["compass"][j] + 180) % 360 - 180)
        m = (d > R_MIN) & (out > 2.0) & (cinc > MIN_COS_INC) & (dh < HALF_FOV)
        e, j, d, cinc, dh = e[m], j[m], d[m], cinc[m], dh[m]
        # видимая ширина стены (угол) и близость к центру кадра; 12–25 м — лучшее расстояние
        ang = np.minimum(EL[e] * cinc / d, 1.5)
        dist_w = np.clip(1 - np.abs(d - 18) / 30, 0.2, 1)
        sc = I["quality"][j] * ang * dist_w * (1.2 - dh / HALF_FOV * 0.5)
        cb.append(EB[e]); ci.append(j); cs.append(sc.astype(np.float32))
        if (s // step) % 25 == 0:
            print(f"  стены {s}/{len(EB)}, пар {sum(len(x) for x in cb)} — {time.time()-t0:.0f}s", flush=True)
    cb, ci, cs = np.concatenate(cb), np.concatenate(ci), np.concatenate(cs)
    # одна пара здание–кадр: оценки разных стен складываются
    key = cb.astype(np.int64) * len(P) + ci
    uk, inv = np.unique(key, return_inverse=True)
    cs = np.bincount(inv, weights=cs).astype(np.float32)
    cb, ci = (uk // len(P)).astype(np.int64), (uk % len(P)).astype(np.int64)
    print(f"[select] пар здание–кадр {len(cb)}, зданий с кадрами {len(np.unique(cb))} — {time.time()-t0:.0f}s", flush=True)

    # топ-CAND на здание
    o = np.lexsort((-cs, cb))
    cb, ci, cs = cb[o], ci[o], cs[o]
    start = np.r_[0, np.flatnonzero(np.diff(cb)) + 1]
    rank = np.arange(len(cb)) - np.repeat(start, np.diff(np.r_[start, len(cb)]))
    m = rank < CAND
    cb, ci, cs = cb[m], ci[m], cs[m]

    # перекрытие: луч камера -> ближайшая точка контура не должен задевать другие здания
    geoms = [b["geom"] for b in B]
    str_tree = shapely.STRtree(geoms)
    cam = shapely.points(P[ci])
    tgt = shapely.shortest_line(cam, np.array(geoms, dtype=object)[cb])
    # укорачиваем на 0.5 м у стены, чтобы не цеплять само здание на углах
    co = shapely.get_coordinates(tgt).reshape(-1, 2, 2)
    dv = co[:, 1] - co[:, 0]
    dl = np.maximum(np.hypot(*dv.T), 1e-6)
    co[:, 1] = co[:, 0] + dv * ((dl - 0.5) / dl)[:, None]
    lines = shapely.linestrings(co)
    hit_l, hit_g = str_tree.query(lines, predicate="intersects")
    blocked = np.zeros(len(cb), bool)
    bad = hit_g != cb[hit_l]
    blocked[hit_l[bad]] = True
    print(f"[select] перекрыто {blocked.mean()*100:.0f}% кандидатов — {time.time()-t0:.0f}s", flush=True)
    cb, ci, cs = cb[~blocked], ci[~blocked], cs[~blocked]

    # жадный выбор: K на здание, ≤ PER_SEQ с проезда, бонус уже выбранным кадрам
    popular = np.bincount(ci, minlength=len(P))
    cs = cs * (1 + 0.15 * np.log1p(popular[ci]))
    o = np.lexsort((-cs, cb))
    cb, ci = cb[o], ci[o]
    pb, pi = [], []
    start = np.r_[0, np.flatnonzero(np.diff(cb)) + 1, len(cb)]
    seq = I["seq"]
    for a, z in zip(start[:-1], start[1:]):
        used = {}
        n = 0
        for j in ci[a:z]:
            s_ = seq[j]
            if used.get(s_, 0) >= PER_SEQ:
                continue
            used[s_] = used.get(s_, 0) + 1
            pb.append(cb[a]); pi.append(j)
            n += 1
            if n >= K_PER_BLD:
                break
    pb, pi = np.array(pb), np.array(pi)
    ui, pinv = np.unique(pi, return_inverse=True)
    np.savez(MLY / "select.npz", img_id=I["id"][ui], img_x=I["x"][ui], img_z=I["z"][ui], img_compass=I["compass"][ui],
             pair_img=pinv.astype(np.int32), pair_bld=pb.astype(np.int32),
             bld_key=np.array([b["key"] for b in B]))
    nb = len(np.unique(pb))
    print(f"[select] зданий с кадрами {nb} из {len(B)} ({nb/len(B)*100:.0f}%), пар {len(pb)}, "
          f"уникальных кадров {len(ui)} — {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
