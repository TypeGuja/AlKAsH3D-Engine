# Официальные паппеты DHX -> части для движка.
#
# Символ персонажа (~XX_Character) на кадре — набор экземпляров символов-
# частей (голова, грива, шея, торс, ноги, хвост, тень) с матрицей Flash
# (a b c d tx ty) по слоям. Каждую часть рендерим отдельно в её собственных
# координатах (начало = точка привязки символа = шарнир), обрезаем по
# альфе и запоминаем, где в картинке лежит начало координат.
#
# python official_parts.py check <имя_из_roster> [кадр]   — части по
#     отдельности + сборка обратно по матрицам рядом с целым кадром.
# python official_parts.py turn <имя_из_roster>           — лист кадров поворота.
import io
import json
import os
import re
import subprocess
import sys
import tempfile

import resvg_py
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from char_frame import describe  # noqa: E402

ROOT = "G:/mlp_2d"
VENV_PY = os.path.join(ROOT, "tools", "venv", "Scripts", "python.exe")
# Пикселей на единицу Flash при рендере частей.
SCALE = 4.0
# Поле вокруг начала координат символа (единицы Flash), куда влезает любая часть.
FIELD = 400


def render_symbol(xfl, sym, frame):
    """Символ на кадре (0-based) -> (RGBA, (ox, oy)) — где в картинке начало координат символа."""
    with tempfile.TemporaryDirectory() as d:
        r = subprocess.run([VENV_PY, "-m", "xfl2svg", xfl, sym, d, "--first-frame", str(frame + 1), "--last-frame", str(frame + 1),
                            "--timeline-type", "symbol", "--no-background"], capture_output=True, text=True)
        svgs = sorted(f for f in os.listdir(d) if f.endswith(".svg"))
        if not svgs:
            raise RuntimeError(f"xfl2svg не отрендерил {sym}@{frame}: {r.stderr[-300:]}")
        svg = open(os.path.join(d, svgs[0]), encoding="utf-8").read()
    px = int(2 * FIELD * SCALE)
    svg = re.sub(r'width="[^"]*" height="[^"]*" viewBox="[^"]*"',
                 f'width="{px}px" height="{px}px" viewBox="{-FIELD} {-FIELD} {2 * FIELD} {2 * FIELD}"', svg, count=1)
    im = Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(svg_string=svg, width=px)))).convert("RGBA")
    bb = im.getbbox()
    if not bb:
        return None, (0, 0)
    im = im.crop(bb)
    o = FIELD * SCALE
    return im, (o - bb[0], o - bb[1])


def roster():
    return json.load(open(os.path.join(ROOT, "roster.json"), encoding="utf-8"))


def parts_of(name, frame):
    r = roster()[name]
    xfl = os.path.join(ROOT, r["xfl"])
    out = []
    for layer, tag, item, ff, loop, mat, pt in describe(xfl, r["symbol"], frame):
        if tag != "DOMSymbolInstance" or not item:
            continue
        if layer in ("AUDIO",) or item.endswith("RotationNotes"):
            continue
        sub = int(ff) if ff else 0
        im, origin = render_symbol(xfl, item, sub)
        if im is None:
            continue
        out.append({"layer": layer, "symbol": item, "frame": sub, "matrix": mat, "image": im, "origin": origin})
    return out


def compose(parts, size=900, scale=1.0):
    """Сборка частей по их матрицам (слои снизу вверх) — проверка, что части и шарниры верны."""
    canvas = Image.new("RGBA", (size, size), (255, 255, 255, 0))
    cx = cy = size / 2
    for p in reversed(parts):
        a, b, c, d, tx, ty = p["matrix"]
        im, (ox, oy) = p["image"], p["origin"]
        k = scale / SCALE
        # Пиксель части (u, v) -> координаты символа ((u-ox)/SCALE, (v-oy)/SCALE)
        # -> персонажа (a x + c y + tx, b x + d y + ty) -> холст.
        # PIL affine берёт обратное отображение: холст -> исходная картинка.
        A = [a * k, c * k, tx * scale + cx - (a * ox + c * oy) * k,
             b * k, d * k, ty * scale + cy - (b * ox + d * oy) * k]
        det = A[0] * A[4] - A[1] * A[3]
        inv = [A[4] / det, -A[1] / det, 0, -A[3] / det, A[0] / det, 0]
        inv[2] = -(inv[0] * A[2] + inv[1] * A[5])
        inv[5] = -(inv[3] * A[2] + inv[4] * A[5])
        layer = im.transform((size, size), Image.AFFINE, inv, resample=Image.BICUBIC)
        canvas.alpha_composite(layer)
    return canvas


def full_frame(name, frame, size=900, scale=1.0):
    r = roster()[name]
    im, (ox, oy) = render_symbol(os.path.join(ROOT, r["xfl"]), r["symbol"], frame)
    canvas = Image.new("RGBA", (size, size), (255, 255, 255, 0))
    k = scale / SCALE
    im = im.resize((max(1, int(im.width * k)), max(1, int(im.height * k))), Image.LANCZOS)
    canvas.alpha_composite(im, (int(size / 2 - ox * k), int(size / 2 - oy * k)))
    return canvas


def on_white(im):
    bg = Image.new("RGB", im.size, (255, 255, 255))
    bg.paste(im, (0, 0), im)
    return bg


if __name__ == "__main__":
    mode, name = sys.argv[1], sys.argv[2]
    out = os.path.join(ROOT, "official_parts", "_check")
    os.makedirs(out, exist_ok=True)
    font = ImageFont.truetype("arial.ttf", 16)
    if mode == "check":
        frame = int(sys.argv[3]) if len(sys.argv) > 3 else 0
        parts = parts_of(name, frame)
        a = on_white(full_frame(name, frame, 500, 2.0))
        b = on_white(compose(parts, 500, 2.0))
        # Части по отдельности, с крестиком в шарнире.
        tiles = []
        for p in parts:
            t = on_white(p["image"].copy())
            t.thumbnail((180, 180))
            k = t.width / p["image"].width
            dr = ImageDraw.Draw(t)
            x, y = p["origin"][0] * k, p["origin"][1] * k
            dr.line((x - 6, y, x + 6, y), fill=(255, 0, 0), width=2)
            dr.line((x, y - 6, x, y + 6), fill=(255, 0, 0), width=2)
            tiles.append((p["symbol"], t))
        cols = 6
        rows = (len(tiles) + cols - 1) // cols
        sheet = Image.new("RGB", (1000 + 40, 520 + rows * 210), (235, 235, 235))
        sheet.paste(a, (10, 10))
        sheet.paste(b, (530, 10))
        dr = ImageDraw.Draw(sheet)
        dr.text((14, 12), "целый кадр", fill=(200, 0, 0), font=font)
        dr.text((534, 12), "сборка из частей", fill=(200, 0, 0), font=font)
        for i, (s, t) in enumerate(tiles):
            x, y = 10 + (i % cols) * 170, 520 + (i // cols) * 210
            sheet.paste(t, (x, y + 22))
            dr.text((x, y + 2), s[-22:], fill=(0, 0, 160), font=font)
        p = os.path.join(out, f"{name}_{frame}.png")
        sheet.save(p)
        print(p, len(parts), "частей")
    elif mode == "turn":
        frames = list(range(100, 132, 2))
        tiles = [(f, on_white(full_frame(name, f, 360, 1.2))) for f in frames]
        sheet = Image.new("RGB", (8 * 360, 2 * 380), (255, 255, 255))
        dr = ImageDraw.Draw(sheet)
        for i, (f, t) in enumerate(tiles):
            x, y = (i % 8) * 360, (i // 8) * 380
            sheet.paste(t, (x, y + 20))
            dr.text((x + 6, y + 2), str(f), fill=(200, 0, 0), font=font)
        p = os.path.join(out, f"{name}_turn.png")
        sheet.save(p)
        print(p)
