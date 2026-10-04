"""Атлас дорожных знаков ГОСТ Р 52290 для знаков, распознанных Mapillary.

Картинки — векторные знаки из Wikimedia Commons (RU_road_sign_<номер>.svg, общественное
достояние: официальные символы), растеризуются resvg в клетки 128 px атласа 2048×2048.
Для каждого знака по альфа-каналу вычисляется контур формы (треугольник, круг, ромб,
восьмиугольник, прямоугольник) — знак в чанке строится плоскостью ровно этой формы
(шейдер движка прозрачность не поддерживает, квадрат с фоном вокруг треугольника выглядел бы
чужеродно). Размеры — типоразмер II (город): треугольник 0.9 м, круг/квадрат 0.7 м,
таблички 8.x — 0.7 м по ширине.

MLY_TO_GOST — соответствие видов Mapillary номерам ГОСТ для самых частых в Самаре видов
(114 видов с ≥ 100 распознаваниями покрывают 98% знаков). Указатели направлений и знаки
с произвольным текстом (6.9–6.13, 5.23–5.26, «texts-*») пропущены: их надпись не известна.

Выход: textures/sign_atlas_{albedo,normal,rough}.png и textures/sign_atlas.json —
{номер: {uv: [u0, v0, u1, v1], outline: [[u, v], ...] (0..1 внутри клетки, v вверх), size: [w, h] м}}
Кэш SVG: CACHE/signs_svg.
"""
import io
import json
import math
import time

import numpy as np
import requests
from PIL import Image

import config as C

ATLAS = 2048
CELL = 128
UA = {"User-Agent": "AlKAsH3D-map-builder/1.0 (https://github.com/; vibe768246@gmail.com)"}

MLY_TO_GOST = {
    # предупреждающие
    "warning--roadworks--g1": "1.25", "warning--children--g1": "1.23", "warning--pedestrians-crossing--g1": "1.22",
    "warning--pedestrians-crossing--g5": "1.22", "warning--icy-road--g1": "1.15", "warning--trams-crossing--g1": "1.5",
    "warning--road-narrows-right--g1": "1.20.2", "warning--road-narrows-left--g1": "1.20.3",
    "warning--road-bump--g1": "1.17", "warning--uneven-road--g1": "1.16", "warning--two-way-traffic--g1": "1.21",
    "warning--bicycles-crossing--g1": "1.24", "warning--railroad-crossing--g4": "1.1",
    "warning--railroad-crossing-without-barriers--g3": "1.2", "warning--traffic-signals--g1": "1.8",
    "warning--other-danger--g1": "1.33", "warning--curve-left--g1": "1.11.2", "warning--curve-right--g1": "1.11.1",
    "warning--crossroads-with-priority-to-the-right--g1": "1.6",
    "complementary--chevron-right--g5": "1.34.1", "complementary--chevron-left--g5": "1.34.2",
    # приоритета
    "regulatory--priority-road--g1": "2.1", "regulatory--end-of-priority-road--g1": "2.2",
    "warning--junction-with-a-side-road-perpendicular-right--g1": "2.3.2",
    "warning--junction-with-a-side-road-perpendicular-left--g1": "2.3.3",
    "regulatory--yield--g1": "2.4", "regulatory--stop--g1": "2.5", "regulatory--stop--g10": "2.5",
    "regulatory--priority-over-oncoming-vehicles--g1": "2.7",
    # запрещающие
    "regulatory--no-entry--g1": "3.1", "regulatory--road-closed-to-vehicles--g3": "3.2",
    "regulatory--no-heavy-goods-vehicles--g1": "3.4", "regulatory--weight-limit--g1": "3.11",
    "regulatory--height-limit--g1": "3.13", "regulatory--no-right-turn--g1": "3.18.1",
    "regulatory--no-left-turn--g1": "3.18.2", "regulatory--no-overtaking--g1": "3.20",
    "regulatory--maximum-speed-limit-5--g1": "3.24-5", "regulatory--maximum-speed-limit-20--g1": "3.24-20",
    "regulatory--maximum-speed-limit-40--g1": "3.24-40", "regulatory--maximum-speed-limit-50--g1": "3.24-50",
    "regulatory--maximum-speed-limit-60--g1": "3.24-60",
    "regulatory--no-stopping--g1": "3.27", "regulatory--no-stopping--g3": "3.27",
    "regulatory--no-parking--g1": "3.28", "regulatory--no-parking--g4": "3.29", "regulatory--no-parking--g3": "3.30",
    # предписывающие
    "regulatory--go-straight--g1": "4.1.1", "regulatory--go-straight--g3": "4.1.1",
    "regulatory--turn-right-ahead--g1": "4.1.2", "regulatory--turn-right-ahead--g2": "4.1.2",
    "regulatory--turn-left-ahead--g1": "4.1.3", "regulatory--turn-left--g1": "4.1.3",
    "regulatory--go-straight-or-turn-right--g1": "4.1.4", "regulatory--go-straight-or-turn-left--g1": "4.1.5",
    "regulatory--turn-left-or-right--g1": "4.1.6", "regulatory--keep-right--g1": "4.2.1",
    "regulatory--keep-left--g1": "4.2.2", "regulatory--pass-on-either-side--g1": "4.2.3",
    "regulatory--pass-on-either-side--g2": "4.2.3", "regulatory--roundabout--g1": "4.3",
    "regulatory--bicycles-only--g1": "4.4.1", "regulatory--pedestrians-only--g1": "4.5.1",
    "regulatory--pedestrians-only--g2": "4.5.1", "information--minimum-speed-40--g1": "4.6",
    # особых предписаний
    "information--limited-access-road--g1": "5.3", "regulatory--one-way-straight--g1": "5.5",
    "regulatory--end-of-one-way-straight--g1": "5.6", "regulatory--one-way-right--g1": "5.7.1",
    "regulatory--one-way-left--g1": "5.7.2", "information--bus-stop--g1": "5.16",
    "information--pedestrians-crossing--g1": "5.19.1", "information--road-bump--g1": "5.20",
    "information--living-street--g1": "5.21", "information--end-of-living-street--g1": "5.22",
    # информационные
    "information--parking--g1": "6.4", "information--parking--g2": "6.4", "information--parking--g5": "6.4",
    "information--dead-end--g1": "6.8.1", "information--dead-end-left--g1": "6.8.2",
    # таблички
    "complementary--distance--g1": "8.1.1", "complementary--distance--g2": "8.1.1",
    "complementary--one-direction-left--g1": "8.3.2", "complementary--both-directions--g2": "8.3.3",
    "complementary--two-way-traffic--g4": "8.3.3", "complementary--disabled-persons--g1": "8.17",
    "complementary--camera--g1": "8.23", "complementary--tow-away-zone--g1": "8.24",
}

# запасные имена файлов в Commons
ALT_NAMES = {"4.6": ["RU_road_sign_4.6-40.svg", "RU_road_sign_4.6.svg"],
             "1.34.1": ["RU_road_sign_1.34.1.svg", "RU_road_sign_1.34.1_(right).svg", "RU_road_sign_1.34.2.svg"],
             "1.34.2": ["RU_road_sign_1.34.2.svg"]}


def physical_size(code, aspect):
    """(ширина, высота) щита, м — типоразмер II; aspect = w/h картинки."""
    g = code.split(".")[0]
    if g == "1" or code == "2.4" or code.startswith("2.3"):
        w = 0.9                               # треугольники
    elif code in ("2.1", "2.2"):
        w = 0.9                               # ромб (диагональ квадрата 0.7 м с каймой)
    elif g == "8":
        w = 0.7                               # таблички
    else:
        w = 0.7
    return w, w / max(aspect, 1e-3)


def fetch_svg(code):
    cache = C.CACHE / "signs_svg"
    cache.mkdir(parents=True, exist_ok=True)
    names = ALT_NAMES.get(code, []) + [f"RU_road_sign_{code}.svg"]
    for n in names:
        p = cache / n
        if p.exists() and p.stat().st_size > 100:
            return p.read_text(encoding="utf-8", errors="ignore")
        for attempt in range(3):
            try:
                r = requests.get(f"https://commons.wikimedia.org/wiki/Special:FilePath/{n}", headers=UA, timeout=60)
                if r.status_code == 200 and b"<svg" in r.content[:4000]:
                    p.write_bytes(r.content)
                    return r.text
                if r.status_code == 404:
                    break
            except requests.RequestException:
                pass
            time.sleep(2 + attempt * 3)
        time.sleep(0.5)                       # вежливо к Commons
    return None


def render(svg):
    """SVG -> RGBA, вписанный в CELL×CELL с сохранением пропорций; aspect = w/h."""
    import resvg_py
    import re
    m = re.search(r'viewBox="\s*[-\d.]+\s+[-\d.]+\s+([\d.]+)\s+([\d.]+)', svg)
    if m:
        vw, vh = float(m.group(1)), float(m.group(2))
    else:
        mw = re.search(r'width="([\d.]+)', svg); mh = re.search(r'height="([\d.]+)', svg)
        vw, vh = (float(mw.group(1)), float(mh.group(1))) if mw and mh else (1.0, 1.0)
    aspect = vw / vh
    if aspect >= 1:
        w, h = CELL, max(8, int(round(CELL / aspect)))
    else:
        w, h = max(8, int(round(CELL * aspect))), CELL
    png = resvg_py.svg_to_bytes(svg_string=svg, width=w * 2, height=h * 2)
    im = Image.open(io.BytesIO(bytes(png))).convert("RGBA").resize((w, h), Image.LANCZOS)
    return im, aspect


def outline(alpha, ox, oy):
    """Контур формы по альфе: выпуклая оболочка непрозрачных пикселей, упрощённая (≤ ~20 вершин).
    Координаты — в пикселях клетки (ox, oy — сдвиг картинки в клетке)."""
    import shapely
    ys, xs = np.nonzero(alpha > 100)
    if len(xs) < 10:
        return None
    pts = shapely.multipoints(np.column_stack([xs + ox + 0.5, ys + oy + 0.5]))
    hull = shapely.convex_hull(pts)
    tol = 0.8
    while True:
        s = hull.simplify(tol)
        if len(s.exterior.coords) <= 21 or tol > 6:
            break
        tol *= 1.4
    co = np.asarray(s.exterior.coords)[:-1]
    # чуть внутрь, чтобы край не цеплял соседнюю клетку при фильтрации
    c = co.mean(0)
    return c + (co - c) * 0.985


def main():
    t0 = time.time()
    codes = sorted(set(MLY_TO_GOST.values()), key=lambda c: [int(x) if x.isdigit() else 999 for x in c.replace("-", ".").split(".")])
    atlas = Image.new("RGB", (ATLAS, ATLAS), (150, 155, 158))   # фон = оборот знака (оцинковка)
    meta, missing = {}, []
    per_row = ATLAS // CELL
    k = 0
    for code in codes:
        svg = fetch_svg(code)
        if svg is None:
            missing.append(code)
            continue
        try:
            im, aspect = render(svg)
        except Exception as e:
            missing.append(f"{code} ({e})")
            continue
        col, row = k % per_row, k // per_row
        ox, oy = (CELL - im.width) // 2, (CELL - im.height) // 2
        ol = outline(np.asarray(im)[:, :, 3], ox, oy)
        if ol is None:
            missing.append(code)
            continue
        # белая подложка под полупрозрачные края, затем знак
        cell = Image.new("RGB", (CELL, CELL), (150, 155, 158))
        cell.paste(im, (ox, oy), im)
        atlas.paste(cell, (col * CELL, row * CELL))
        u0, v0 = col * CELL / ATLAS, 1 - (row + 1) * CELL / ATLAS
        meta[code] = dict(uv=[u0, v0, u0 + CELL / ATLAS, v0 + CELL / ATLAS],
                          outline=[[round(x / CELL, 4), round(1 - y / CELL, 4)] for x, y in ol],
                          size=[round(v, 3) for v in physical_size(code, aspect)])
        k += 1
    tex = C.OUT / "textures"
    tex.mkdir(parents=True, exist_ok=True)
    atlas.save(tex / "sign_atlas_albedo.png", optimize=True)
    Image.new("RGB", (16, 16), (128, 128, 255)).save(tex / "sign_atlas_normal.png")
    Image.new("L", (16, 16), int(0.4 * 255)).save(tex / "sign_atlas_rough.png")
    json.dump(meta, open(tex / "sign_atlas.json", "w", encoding="utf-8"), ensure_ascii=False, indent=0)
    print(f"[signs] знаков в атласе {len(meta)} из {len(codes)}; нет в Commons: {missing} — {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
