"""Текстуры деталей улиц из OSM (osm_details.py) + атлас букв для вывесок.

Размер каждой текстуры — MATERIALS[name]["size"] (мелочи 128–256 px, атлас 1024),
чтобы десятки новых материалов не раздували видеопамять. Знаки ПДД нарисованы
по ГОСТ Р 52290 в упрощённом виде (форма, цвета, символ).

Атлас букв: textures/glyphs_<цвет>_albedo.png + textures/glyphs.json с метриками
(UV и ширина каждого символа в долях высоты строки) — по ним osm_details
раскладывает текст вывесок квадами.
"""
import json

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

import config as C
from materials import MATERIALS

TEX = C.OUT / "textures"
FONT = "C:/Windows/Fonts/arialbd.ttf"

# символы вывесок: латиница, кириллица, цифры, пунктуация
CHARSET = ("".join(chr(c) for c in range(32, 127))
           + "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдеёжзийклмнопрстуфхцчшщъыьэюя"
           + "«»№–—’“”…°")
GLYPH_COLORS = {
    "white": (1.0, 1.0, 1.0),
    "yellow": (1.0, 0.82, 0.15),
    "red": (1.0, 0.18, 0.12),
    "green": (0.15, 0.85, 0.3),
    "blue": (0.25, 0.55, 1.0),
}


def size_of(name):
    return MATERIALS[name].get("size", 1024)


def noise(n, freq, seed):
    g = np.random.default_rng(seed).random((freq, freq)).astype(np.float32)
    return ndimage.zoom(g, n / freq, order=3, mode="grid-wrap")[:n, :n]


def normal_from_height(h, strength):
    dx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 0.5 * strength
    dy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 0.5 * strength
    n = np.dstack([-dx, dy, np.ones_like(h)])
    n /= np.linalg.norm(n, axis=2, keepdims=True)
    return n * 0.5 + 0.5


def save(name, albedo, height=0.0, rough=None, nstrength=2.0):
    n = size_of(name)
    TEX.mkdir(parents=True, exist_ok=True)
    albedo = np.asarray(albedo, np.float32)
    if albedo.shape[:2] != (n, n):
        albedo = np.asarray(Image.fromarray((np.clip(albedo, 0, 1) * 255).astype(np.uint8)).resize((n, n), Image.LANCZOS),
                            np.float32) / 255
    height = np.broadcast_to(np.asarray(height, np.float32), (n, n)).astype(np.float32)
    r = MATERIALS[name]["rough"] if rough is None else rough
    rough = np.broadcast_to(np.asarray(r, np.float32), (n, n))
    Image.fromarray((np.clip(albedo, 0, 1) * 255).astype(np.uint8)).save(TEX / f"{name}_albedo.png", optimize=True)
    Image.fromarray((normal_from_height(height, nstrength * n / 256) * 255).astype(np.uint8)) \
        .save(TEX / f"{name}_normal.png", optimize=True)
    Image.fromarray((np.clip(rough, 0, 1) * 255).astype(np.uint8)).save(TEX / f"{name}_rough.png", optimize=True)


def flat(name, rgb, grain=0.06, seed=0, freq=16):
    n = size_of(name)
    nz = noise(n, freq, seed)
    a = np.ones((n, n, 3), np.float32) * np.asarray(rgb, np.float32) * (1 + (nz[..., None] - 0.5) * grain * 2)
    save(name, a, nz * 0.2)


def canvas(name, bg):
    n = size_of(name) * 2          # рисуем вдвое крупнее и уменьшаем — сглаженные края
    img = Image.new("RGB", (n, n), tuple(int(c * 255) for c in bg))
    return img, ImageDraw.Draw(img), n


def finish(name, img):
    save(name, np.asarray(img, np.float32) / 255)


# ---------------------------------------------------------------- знаки ПДД

BLUE = (0.0, 0.32, 0.65)
RED = (0.80, 0.06, 0.08)
WHITE = (0.96, 0.96, 0.96)
YELLOW = (0.98, 0.78, 0.0)
BLACK = (0.05, 0.05, 0.05)


def rgb8(c):
    return tuple(int(v * 255) for v in c)


def walker(d, cx, cy, s, fill):
    """Упрощённый пешеход знака 5.19 (голова, туловище, шаг)."""
    d.ellipse([cx - 0.06 * s, cy - 0.34 * s, cx + 0.06 * s, cy - 0.22 * s], fill=fill)
    w = int(0.05 * s)
    d.line([(cx, cy - 0.2 * s), (cx - 0.02 * s, cy + 0.05 * s)], fill=fill, width=w)
    d.line([(cx - 0.02 * s, cy + 0.05 * s), (cx - 0.14 * s, cy + 0.3 * s)], fill=fill, width=w)
    d.line([(cx - 0.02 * s, cy + 0.05 * s), (cx + 0.12 * s, cy + 0.3 * s)], fill=fill, width=w)
    d.line([(cx, cy - 0.15 * s), (cx - 0.13 * s, cy + 0.0 * s)], fill=fill, width=w)
    d.line([(cx, cy - 0.15 * s), (cx + 0.14 * s, cy - 0.02 * s)], fill=fill, width=w)


def sign_crossing():
    img, d, n = canvas("sign_crossing", BLUE)
    m = n * 0.1
    d.polygon([(n / 2, m), (n - m, n - m), (m, n - m)], fill=rgb8(WHITE))
    for k in range(4):          # «зебра» под пешеходом
        x = n * (0.3 + 0.11 * k)
        d.rectangle([x, n * 0.78, x + n * 0.06, n * 0.84], fill=rgb8(BLACK))
    walker(d, n / 2, n * 0.58, n * 0.55, rgb8(BLACK))
    finish("sign_crossing", img)


def sign_give_way():
    img, d, n = canvas("sign_give_way", (0.6, 0.6, 0.6))   # фон вне треугольника — оборотная сторона не видна
    d.polygon([(n * 0.02, n * 0.1), (n * 0.98, n * 0.1), (n / 2, n * 0.93)], fill=rgb8(RED))
    d.polygon([(n * 0.17, n * 0.19), (n * 0.83, n * 0.19), (n / 2, n * 0.76)], fill=rgb8(WHITE))
    finish("sign_give_way", img)


def sign_stop():
    img, d, n = canvas("sign_stop", (0.6, 0.6, 0.6))
    import math
    oct_ = [(n / 2 + n * 0.48 * math.cos(math.radians(22.5 + 45 * k)), n / 2 + n * 0.48 * math.sin(math.radians(22.5 + 45 * k))) for k in range(8)]
    d.polygon(oct_, fill=rgb8(RED))
    oct2 = [(n / 2 + n * 0.43 * math.cos(math.radians(22.5 + 45 * k)), n / 2 + n * 0.43 * math.sin(math.radians(22.5 + 45 * k))) for k in range(8)]
    d.polygon(oct2, outline=rgb8(WHITE), width=int(n * 0.015))
    f = ImageFont.truetype(FONT, int(n * 0.26))
    d.text((n / 2, n / 2), "STOP", font=f, fill=rgb8(WHITE), anchor="mm")
    finish("sign_stop", img)


def sign_bus_stop():
    img, d, n = canvas("sign_bus_stop", YELLOW)    # 5.16 в городе — на жёлтом фоне
    d.rectangle([n * 0.04, n * 0.04, n * 0.96, n * 0.96], outline=rgb8(BLACK), width=int(n * 0.02))
    # автобус сбоку
    d.rounded_rectangle([n * 0.18, n * 0.3, n * 0.82, n * 0.68], radius=int(n * 0.05), fill=rgb8(BLACK))
    for k in range(4):
        d.rectangle([n * (0.23 + 0.14 * k), n * 0.36, n * (0.33 + 0.14 * k), n * 0.5], fill=rgb8(YELLOW))
    for x in (0.32, 0.68):
        d.ellipse([n * (x - 0.07), n * 0.62, n * (x + 0.07), n * 0.76], fill=rgb8(BLACK))
        d.ellipse([n * (x - 0.03), n * 0.66, n * (x + 0.03), n * 0.72], fill=rgb8(YELLOW))
    finish("sign_bus_stop", img)


def sign_tram_stop():
    img, d, n = canvas("sign_tram_stop", YELLOW)
    d.rectangle([n * 0.04, n * 0.04, n * 0.96, n * 0.96], outline=rgb8(BLACK), width=int(n * 0.02))
    d.line([(n * 0.5, n * 0.12), (n * 0.5, n * 0.28)], fill=rgb8(BLACK), width=int(n * 0.02))   # токоприёмник
    d.line([(n * 0.38, n * 0.12), (n * 0.62, n * 0.12)], fill=rgb8(BLACK), width=int(n * 0.02))
    d.rounded_rectangle([n * 0.2, n * 0.28, n * 0.8, n * 0.74], radius=int(n * 0.06), fill=rgb8(BLACK))
    for k in range(3):
        d.rectangle([n * (0.26 + 0.17 * k), n * 0.35, n * (0.38 + 0.17 * k), n * 0.5], fill=rgb8(YELLOW))
    d.rectangle([n * 0.15, n * 0.8, n * 0.85, n * 0.83], fill=rgb8(BLACK))
    finish("sign_tram_stop", img)


def sign_metro():
    img, d, n = canvas("sign_metro", WHITE)
    f = ImageFont.truetype(FONT, int(n * 0.8))
    d.text((n / 2, n / 2), "М", font=f, fill=rgb8(RED), anchor="mm")
    finish("sign_metro", img)


# ---------------------------------------------------------------- прочее

def door_metal():
    n = size_of("door_metal")
    nz = noise(n, 16, 501)
    a = np.ones((n, n, 3), np.float32) * np.array([0.32, 0.22, 0.16], np.float32)    # коричневая порошковая краска
    a *= (1 + (nz[..., None] - 0.5) * 0.1)
    yy, xx = np.mgrid[0:n, 0:n] / n
    h = np.zeros((n, n), np.float32)
    frame = (xx < 0.05) | (xx > 0.95) | (yy < 0.03)
    a[frame] *= 0.6
    panel = (xx > 0.14) & (xx < 0.86) & (((yy > 0.08) & (yy < 0.45)) | ((yy > 0.52) & (yy < 0.92)))
    h[panel] = 1.0
    h = ndimage.gaussian_filter(h, n / 200)
    handle = (xx > 0.8) & (xx < 0.86) & (yy > 0.47) & (yy < 0.55)
    a[handle] = (0.75, 0.75, 0.72)
    save("door_metal", a, h, nstrength=3.0)


def granite():
    n = size_of("granite")
    speck = (np.random.default_rng(511).random((n, n)) < 0.25).astype(np.float32)
    base = 0.45 + 0.15 * noise(n, 32, 512) - 0.15 * ndimage.gaussian_filter(speck, 0.6)
    a = np.dstack([base * 1.02, base * 0.98, base * 0.96])
    save("granite", a, noise(n, 64, 513) * 0.3)


def bronze():
    n = size_of("bronze")
    nz = noise(n, 8, 521)
    patina = np.clip((noise(n, 16, 522) - 0.55) * 3, 0, 1)
    a = np.dstack([0.45 + 0.1 * nz, 0.33 + 0.08 * nz, 0.18 + 0.05 * nz])
    a = a * (1 - patina[..., None] * 0.6) + np.array([0.25, 0.42, 0.36]) * patina[..., None] * 0.6
    save("bronze", a, nz * 0.3)


def wood_planks():
    n = size_of("wood_planks")
    yy, xx = np.mgrid[0:n, 0:n] / n
    grain = noise(n, 8, 531)
    stripes = (np.sin(xx * 6.28 * 40 + grain * 8) * 0.5 + 0.5)
    gaps = ((yy * 4) % 1) < 0.04
    a = np.dstack([0.42 + 0.08 * stripes, 0.28 + 0.06 * stripes, 0.16 + 0.04 * stripes])
    a[gaps] *= 0.3
    save("wood_planks", a, stripes * 0.3 - gaps * 0.5)


def boom_stripes():
    n = size_of("boom_stripes")
    _, xx = np.mgrid[0:n, 0:n] / n
    red = (xx % 1) < 0.5           # тайл 1 м: 0.5 м красного + 0.5 м белого
    a = np.where(red[..., None], np.array([0.75, 0.06, 0.05]), np.array([0.92, 0.92, 0.9]))
    save("boom_stripes", a.astype(np.float32))


def signal_lens():
    flat("signal_lens", (0.05, 0.05, 0.05), grain=0.02, seed=541)


def glyph_atlas():
    """Атлас символов: ячейки по сетке, белые буквы на чёрном. Метрики — в glyphs.json."""
    n = size_of("glyphs_white")
    cols = 16
    rows = int(np.ceil(len(CHARSET) / cols))
    cw, ch = n // cols, n // rows
    # кегль: по высоте строки, но так, чтобы самая широкая буква (Ш, Щ, Ж, №)
    # целиком влезала в ячейку — иначе UV соседних символов захватят её край
    font_px = int(ch * 0.78)
    while True:
        font = ImageFont.truetype(FONT, font_px)
        if max(font.getlength(c) for c in CHARSET) <= cw - 4:
            break
        font_px -= 1
    ascent, descent = font.getmetrics()
    img = Image.new("L", (n, n), 0)
    d = ImageDraw.Draw(img)
    meta = {"line_h_px": ch, "chars": {}}
    for k, chr_ in enumerate(CHARSET):
        cx, cy = (k % cols) * cw, (k // cols) * ch
        adv = font.getlength(chr_)
        x = cx + (cw - adv) / 2
        y = cy + (ch - (ascent + descent)) / 2
        d.text((x, y), chr_, font=font, fill=255)
        # UV ячейки (V вверх: строка 0 картинки — верх, V=1) и ширина символа
        # в долях высоты строки — по ней ставится следующий символ
        meta["chars"][chr_] = {
            "u0": (x - 1) / n, "u1": (x + adv + 1) / n,
            "v0": 1 - (cy + ch) / n, "v1": 1 - cy / n,
            "w": (adv + 2) / ch,
        }
    a = np.asarray(img, np.float32) / 255
    for color, rgb in GLYPH_COLORS.items():
        save(f"glyphs_{color}", a[..., None] * np.asarray(rgb, np.float32), 0.0, nstrength=0.0)
    (TEX / "glyphs.json").write_text(json.dumps(meta, ensure_ascii=False), encoding="utf-8")


def main():
    flat("steel_grey", (0.42, 0.44, 0.45), seed=551)
    flat("steel_lattice", (0.58, 0.6, 0.6), grain=0.15, seed=552, freq=32)
    flat("cable", (0.22, 0.22, 0.22), seed=553)
    flat("insulator", (0.35, 0.55, 0.45), seed=554)          # зеленоватое стекло изоляторов
    flat("signal_body", (0.06, 0.06, 0.06), seed=555)
    signal_lens()
    sign_crossing(); sign_give_way(); sign_stop(); sign_bus_stop(); sign_tram_stop(); sign_metro()
    flat("sign_back", (0.6, 0.62, 0.63), grain=0.1, seed=556)
    flat("sign_board", (0.0, 0.0, 0.0), grain=0.0, seed=557)   # чёрный, как фон атласа букв
    glyph_atlas()
    door_metal()
    flat("entrance_lamp_glass", (0.92, 0.9, 0.85), grain=0.03, seed=558)
    flat("glass_shelter", (0.12, 0.16, 0.18), grain=0.04, seed=559)
    granite(); bronze(); wood_planks(); boom_stripes()
    flat("paint_red", (0.62, 0.08, 0.06), seed=560)
    flat("paint_blue", (0.08, 0.25, 0.6), seed=561)
    flat("paint_yellow", (0.85, 0.65, 0.08), seed=562)
    flat("paint_green", (0.12, 0.35, 0.18), seed=563)


if __name__ == "__main__":
    main()
