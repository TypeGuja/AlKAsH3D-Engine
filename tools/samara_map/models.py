"""Модели для инстансов из props/*.csv: деревья (процедурные) и фонарь (из ассетов редактора).

Все модели: Y вверх, метры, основание в (0,0,0). Поворот инстанса yaw_deg — вокруг +Y
(против часовой, если смотреть сверху); у фонаря рычаг смотрит по +X модели.
"""
import math
import shutil

import numpy as np

import config as C


def icosphere(sub):
    t = (1 + 5 ** 0.5) / 2
    V = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t), (0, 1, -t),
         (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
    F = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4), (11, 10, 2), (10, 7, 6),
         (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
    V = [np.array(v, float) / np.linalg.norm(v) for v in V]
    for _ in range(sub):
        cache = {}
        nf = []

        def mid(a, b):
            k = (min(a, b), max(a, b))
            if k not in cache:
                m = V[a] + V[b]
                V.append(m / np.linalg.norm(m))
                cache[k] = len(V) - 1
            return cache[k]
        for a, b, c in F:
            ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
            nf += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
        F = nf
    return np.array(V), np.array(F)


class Obj:
    def __init__(self):
        self.lines = []
        self.nv = 0

    def group(self, name, mat, V, N, UV, F):
        self.lines.append(f"o {name}\nusemtl {mat}")
        self.lines += [f"v {x:.3f} {y:.3f} {z:.3f}" for x, y, z in V]
        self.lines += [f"vt {u:.3f} {v:.3f}" for u, v in UV]
        self.lines += [f"vn {x:.3f} {y:.3f} {z:.3f}" for x, y, z in N]
        o = self.nv + 1
        self.lines += ["f " + " ".join(f"{i+o}/{i+o}/{i+o}" for i in f) for f in F]
        self.nv += len(V)

    def save(self, path, header):
        path.write_text(header + "\nmtllib ../samara.mtl\n" + "\n".join(self.lines) + "\n", encoding="utf-8")


def cylinder(r0, r1, h, segs, y0=0.0):
    V, N, UV, F = [], [], [], []
    for s in range(segs + 1):
        a = 2 * math.pi * s / segs
        c, sn = math.cos(a), math.sin(a)
        for r, y in ((r0, y0), (r1, y0 + h)):
            V.append((r * c, y, r * sn)); N.append((c, (r0 - r1) / h, sn)); UV.append((s / segs * 3, y / 2))
    for s in range(segs):
        a, b, c_, d = 2 * s, 2 * s + 1, 2 * s + 2, 2 * s + 3
        F += [(a, b, d), (a, d, c_)]
    N = [tuple(np.array(n) / np.linalg.norm(n)) for n in N]
    return V, N, UV, fix(V, F, N)


def fix(V, F, N):
    """Обход CCW снаружи: сверяем с нормалями вершин."""
    V = np.array(V); N = np.array(N)
    out = []
    for f in F:
        a, b, c = f
        n = np.cross(V[b] - V[a], V[c] - V[a])
        out.append(f if np.dot(n, N[a] + N[b] + N[c]) >= 0 else (a, c, b))
    return out


def crown(center, radius, squash, seed, sub=2):
    V, F = icosphere(sub)
    rng = np.random.default_rng(seed)
    bumps = rng.normal(0, 1, (6, 3))
    disp = 1 + 0.12 * np.sum([np.sin(V @ b * 2.5 + k) for k, b in enumerate(bumps)], axis=0) / 3
    P = V * radius * disp[:, None] * np.array([1, squash, 1]) + center
    N = V.copy()
    UV = np.stack([np.arctan2(V[:, 2], V[:, 0]) / (2 * math.pi) * 6, V[:, 1] * 3], 1)
    return P, N, UV, fix(P, [tuple(f) for f in F], N)


def tree_deciduous():
    o = Obj()
    o.group("trunk", "bark", *cylinder(0.22, 0.14, 4.2, 10))
    for k, (c, r, sq) in enumerate((((0, 6.3, 0), 2.9, 1.1), ((0.9, 5.4, 0.6), 2.0, 1.0), ((-0.8, 5.6, -0.5), 2.1, 1.0))):
        o.group(f"crown{k}", "leaves", *crown(np.array(c, float), r, sq, 10 + k))
    o.save(C.OUT / "models" / "tree_deciduous.obj", "# Лиственное дерево ~9 м (липа/тополь/берёза условно). Y вверх, основание в 0.")


def tree_pine():
    o = Obj()
    o.group("trunk", "bark", *cylinder(0.24, 0.08, 15.0, 10))
    for k, (y, r, h) in enumerate(((7.0, 2.6, 4.0), (9.5, 2.1, 3.6), (11.8, 1.6, 3.2), (13.8, 1.0, 2.6))):
        V, N, UV, F = cylinder(r, 0.05, h, 12, y0=y)
        o.group(f"crown{k}", "pine_needles", V, N, UV, F)
        # донышко яруса
        cV = [(0, y, 0)] + [(r * math.cos(2 * math.pi * s / 12), y, r * math.sin(2 * math.pi * s / 12)) for s in range(12)]
        cN = [(0, -1, 0)] * 13
        cUV = [(0.5, 0.5)] + [(0.5 + 0.5 * math.cos(2 * math.pi * s / 12), 0.5 + 0.5 * math.sin(2 * math.pi * s / 12)) for s in range(12)]
        cF = fix(cV, [(0, 1 + s, 1 + (s + 1) % 12) for s in range(12)], cN)
        o.group(f"crown{k}_base", "pine_needles", cV, cN, cUV, cF)
    o.save(C.OUT / "models" / "tree_pine.obj", "# Сосна ~16 м. Y вверх, основание в 0.")


# Геометрия уличного фонаря (метры). Точка света — центр рассеивателя
# светильника, чуть ниже него; по ней ставится свет в .alfar.
LAMP_POLE_H = 9.0
LAMP_REACH = 1.75
LAMP_LIGHT = (LAMP_REACH + 0.12, LAMP_POLE_H + 0.05, 0.0)


def _ring(center, axis, side_a, side_b, radius, sides, phase=0.0):
    pts, nrm = [], []
    for k in range(sides):
        a = 2 * math.pi * k / sides + phase
        d = side_a * math.cos(a) + side_b * math.sin(a)
        pts.append(center + d * radius)
        nrm.append(d)
    return pts, nrm


def tube(path, radii, sides, u_scale, v_scale):
    """Труба вдоль ломаной path (N,3) с радиусами radii (N,) — кольца
    перпендикулярны касательной, швы по UV. Возвращает V, N, UV, F."""
    path = np.asarray(path, float)
    V, N, UV, F = [], [], [], []
    dist = np.concatenate([[0], np.cumsum(np.linalg.norm(np.diff(path, axis=0), axis=1))])
    ref = np.array([0.0, 0.0, 1.0])
    for i, c in enumerate(path):
        t = path[min(i + 1, len(path) - 1)] - path[max(i - 1, 0)]
        t /= np.linalg.norm(t)
        a = np.cross(t, ref)
        if np.linalg.norm(a) < 1e-6:
            a = np.cross(t, [1.0, 0.0, 0.0])
        a /= np.linalg.norm(a)
        b = np.cross(t, a)
        for k in range(sides + 1):                    # +1 — шов UV
            ang = 2 * math.pi * k / sides
            d = a * math.cos(ang) + b * math.sin(ang)
            V.append(tuple(c + d * radii[i])); N.append(tuple(d))
            UV.append((k / sides * 2 * math.pi * radii[i] / u_scale, dist[i] / v_scale))
    row = sides + 1
    for i in range(len(path) - 1):
        for k in range(sides):
            a0, a1 = i * row + k, i * row + k + 1
            b0, b1 = (i + 1) * row + k, (i + 1) * row + k + 1
            F += [(a0, a1, b1), (a0, b1, b0)]
    return V, N, UV, fix(V, F, N)


def street_lamp():
    """Типовой городской фонарь: оцинкованная коническая опора ~9 м на
    фланце с закладной, одиночный изогнутый кронштейн ~1.75 м и консольный
    светодиодный светильник ("кобра") с матовым рассеивателем снизу.
    Основание в (0,0,0), кронштейн смотрит по +X (к дороге)."""
    o = Obj()
    # фланец + "стакан" основания
    # (фонарей в городе ~60 тыс. и они впекаются в чанки — поэтому бюджет ~150 треугольников)
    o.group("base", "lamp_pole", *cylinder(0.11, 0.1, 0.55, 8))
    # коническая опора (8 граней, как у настоящих гнутых опор)
    o.group("pole", "lamp_pole", *tube([[0, 0.5, 0], [0, LAMP_POLE_H - 0.35, 0]], [0.085, 0.048], 8, 0.6, 3.0))
    # кронштейн: из вершины опоры плавной дугой вбок и чуть вверх
    arc = []
    for k in range(6):
        t = k / 5
        x = LAMP_REACH * t
        y = LAMP_POLE_H - 0.45 + 0.5 * math.sin(t * math.pi / 2)
        arc.append([x, y, 0.0])
    arc[0] = [0.0, LAMP_POLE_H - 0.45, 0.0]
    o.group("arm", "lamp_pole", *tube(arc, [0.045] + [0.032] * 5, 5, 0.6, 3.0))
    # крышка опоры
    capV = [(0, LAMP_POLE_H - 0.35, 0)] + [(0.048 * math.cos(2 * math.pi * k / 8), LAMP_POLE_H - 0.35, 0.048 * math.sin(2 * math.pi * k / 8)) for k in range(8)]
    o.group("pole_cap", "lamp_pole", capV, [(0, 1, 0)] * 9, [(0.5, 0.5)] * 9, fix(capV, [(0, 1 + k, 1 + (k + 1) % 8) for k in range(8)], [(0, 1, 0)] * 9))
    # корпус светильника: лофт суперэллипсов вдоль +X, с лёгким подъёмом 5°
    tilt = math.radians(5)
    x0 = LAMP_REACH - 0.25
    sections = [(0.00, 0.06, 0.045), (0.14, 0.15, 0.08), (0.52, 0.17, 0.08), (0.80, 0.09, 0.04)]
    ring_n = 8
    V, N, UV, F = [], [], [], []
    y_mount = LAMP_POLE_H + 0.05
    for si, (dx, hw, hh) in enumerate(sections):
        cx = x0 + dx * math.cos(tilt)
        cy = y_mount + dx * math.sin(tilt)
        for k in range(ring_n):
            a = 2 * math.pi * k / ring_n
            ca, sa = math.cos(a), math.sin(a)
            # суперэллипс: плоское дно (под рассеиватель), скруглённый верх
            zz = hw * math.copysign(abs(ca) ** 0.6, ca)
            yy_ = hh * math.copysign(abs(sa) ** 0.6, sa) if sa > 0 else -0.03 * abs(sa)
            V.append((cx, cy + yy_, zz)); N.append((0.0, sa, ca)); UV.append((k / ring_n, dx))
    for si in range(len(sections) - 1):
        for k in range(ring_n):
            a0, a1 = si * ring_n + k, si * ring_n + (k + 1) % ring_n
            b0, b1 = (si + 1) * ring_n + k, (si + 1) * ring_n + (k + 1) % ring_n
            F += [(a0, b0, b1), (a0, b1, a1)]
    # торцы
    for si, sgn in ((0, -1), (len(sections) - 1, 1)):
        c = len(V)
        cx = x0 + sections[si][0] * math.cos(tilt)
        V.append((cx, y_mount + sections[si][0] * math.sin(tilt), 0.0)); N.append((sgn, 0, 0)); UV.append((0.5, 0.5))
        for k in range(ring_n):
            F.append((c, si * ring_n + k, si * ring_n + (k + 1) % ring_n))
    Nn = [tuple(np.array(n) / max(np.linalg.norm(n), 1e-9)) for n in N]
    # у торцов нормали колец не годятся для выправления — выправим по центроиду корпуса
    center = np.array([x0 + 0.4, y_mount, 0.0])
    Vn = np.array(V)
    F2 = []
    for f in F:
        a, b, c = f
        fn = np.cross(Vn[b] - Vn[a], Vn[c] - Vn[a])
        out = (Vn[a] + Vn[b] + Vn[c]) / 3 - center
        F2.append(f if np.dot(fn, out) >= 0 else (a, c, b))
    # плоское затенение: у торцов сглаженные нормали колец смотрели бы не туда
    hV, hN, hUV, hF = [], [], [], []
    for a, b, c in F2:
        fn = np.cross(Vn[b] - Vn[a], Vn[c] - Vn[a])
        fn = tuple(fn / max(np.linalg.norm(fn), 1e-12))
        k = len(hV)
        for i in (a, b, c):
            hV.append(V[i]); hN.append(fn); hUV.append(UV[i])
        hF.append((k, k + 1, k + 2))
    o.group("housing", "lamp_housing", hV, hN, hUV, hF)
    # рассеиватель: плоская линза снизу корпуса
    gx0, gx1 = x0 + 0.14, x0 + 0.66
    gy0 = y_mount + (gx0 - x0) * math.tan(tilt) - 0.032
    gy1 = y_mount + (gx1 - x0) * math.tan(tilt) - 0.032
    gV = [(gx0, gy0, -0.13), (gx1, gy1, -0.13), (gx1, gy1, 0.13), (gx0, gy0, 0.13)]
    gN = [(0, -1, 0)] * 4
    o.group("diffuser", "lamp_glass", gV, gN, [(0, 0), (1, 0), (1, 1), (0, 1)], fix(gV, [(0, 1, 2), (0, 2, 3)], gN))
    o.save(C.OUT / "models" / "street_lamp.obj",
           "# Уличный фонарь: оцинкованная коническая опора 9 м, кронштейн 1.75 м, консольный LED-светильник.\n"
           f"# Y вверх, основание в 0, кронштейн по +X. Точка света (центр рассеивателя): {LAMP_LIGHT}")
    tris = sum(1 for l in o.lines if l.startswith("f "))
    print(f"[models] street_lamp: {tris} треугольников")


def main():
    (C.OUT / "models").mkdir(parents=True, exist_ok=True)
    tree_deciduous()
    tree_pine()
    street_lamp()
    print("[models] tree_deciduous, tree_pine, street_lamp")


if __name__ == "__main__":
    main()
