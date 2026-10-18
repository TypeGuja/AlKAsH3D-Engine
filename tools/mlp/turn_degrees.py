# Поворот официального паппета DHX с шагом 1° — проба.
#
# В исходниках поворот нарисован ключевыми кадрами (16–20 ракурсов на круг,
# у части фоновых пони часть кадров помечена "не доделан"). Промежуточные
# углы — как твин во Flash: части соседних ракурсов сопоставляются по слою,
# матрица каждой части плавно ведётся от одного ракурса к другому; рисунок
# части, если он в соседних ракурсах разный, сменяется посередине (чистым
# переключением — смешение двух разных рисунков даёт двоения); часть,
# которая есть только в одном ракурсе, появляется/исчезает там же.
#
# python turn_degrees.py [имя_из_roster] [--step градусы] [--size px]
#   -> G:/mlp_2d/official_parts/_turn/<имя>/: кадры deg_000.png..., <имя>.gif,
#      <имя>_sheet.png (каждые 15°).
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import export_official as eo  # noqa: E402

ROOT = eo.ROOT
NS = eo.NS


def char_layers(xfl, root, idx):
    """Части на кадре idx по слоям, снизу вверх: [(слой, символ, кадр символа, матрица, цветэффект)];
    None — кадр помечен аниматорами как недоделанный."""
    layers = [l for l in root.iter(NS + "DOMLayer") if l.get("layerType") not in ("guide", "folder", "mask")
              and l.get("visible") != "false"]
    out = []
    for li, layer in enumerate(reversed(layers)):
        if layer.get("name", "").upper() == "AUDIO":
            continue
        fr = eo.frame_at(layer, idx)
        if fr is None or fr.find(NS + "elements") is None:
            continue
        for k, el in enumerate(fr.find(NS + "elements")):
            if el.tag != NS + "DOMSymbolInstance":
                continue
            sym = el.get("libraryItemName")
            if "RotationNotes" in sym or "Rotation_Notes" in sym:
                continue
            if "ThisFrameNotFinal" in sym:
                return None
            ff = int(el.get("firstFrame", "0"))
            loop = el.get("loop", "loop")
            n = xfl.length(sym)
            if loop == "single frame":
                sub = ff
            elif loop == "play once":
                sub = min(ff + idx - int(fr.get("index")), n - 1)
            else:
                sub = (ff + idx - int(fr.get("index"))) % max(n, 1)
            m = el.find(NS + "matrix/" + NS + "Matrix")
            mat = [float(m.get(key, d)) if m is not None else d for key, d in (("a", 1), ("b", 0), ("c", 0), ("d", 1), ("tx", 0), ("ty", 0))]
            c = el.find(NS + "color/" + NS + "Color")
            ce = eo.ColorEffect.from_xfl(c) if c is not None else eo.ColorEffect()
            out.append(((li, k), sym, min(sub, n - 1), mat, ce))
    return out


def smooth(x):
    x = min(max(x, 0.0), 1.0)
    return x * x * (3 - 2 * x)


class Turn:
    def __init__(self, name, size):
        roster = json.load(open(os.path.join(ROOT, "roster.json"), encoding="utf-8"))
        r = roster[name]
        self.xfl = eo.Xfl(os.path.join(ROOT, r["xfl"]))
        root = eo.sym_root(self.xfl, r["symbol"])
        frames = eo.rotation_frames(root)
        lab = eo.rotation_label(root)
        self.half = bool(lab and lab[1] <= eo.HALF_TURN_MAX)
        self.views = []  # (угол в градусах, части)
        self.tex = {}
        skipped = 0
        for i, f in enumerate(frames):
            parts = char_layers(self.xfl, root, f)
            if parts is None:
                skipped += 1
                continue
            ang = 180.0 * i / max(len(frames) - 1, 1) if self.half else 360.0 * i / len(frames)
            # Пустые недоделки (не помечены, но почти ничего не рисуют) — тоже мимо.
            if sum(1 for p in parts if self.image(p[1], p[2], p[4])[0] is not None) < 3:
                skipped += 1
                continue
            self.views.append((ang, parts))
        print(f"{name}: ракурсов {len(self.views)} из {len(frames)} (недоделанных {skipped}), пол-оборота: {self.half}")
        self.size = size
        # Масштаб и центр — по первому ракурсу (анфас): земля внизу кадра.
        xs, ys = [], []
        for _, sym, sub, m, ce in self.views[0][1]:
            im, (ox, oy) = self.image(sym, sub, ce)
            if im is None:
                continue
            a, b, c, d, tx, ty = m
            for u, v in ((0, 0), (im.width, 0), (0, im.height), (im.width, im.height)):
                x, y = (u - ox) / eo.PX, (v - oy) / eo.PX
                xs.append(a * x + c * y + tx)
                ys.append(b * x + d * y + ty)
        self.cx = (min(xs) + max(xs)) / 2
        self.ground = max(ys)
        self.scale = size * 0.78 / (max(ys) - min(ys))

    def image(self, sym, sub, ce):
        key = (sym, sub, repr(ce))
        if key not in self.tex:
            im, origin = self.xfl.render(sym, sub)
            self.tex[key] = (None if im is None else eo.apply_effect(im, ce), origin)
        return self.tex[key]

    def draw(self, canvas, part, alpha):
        _, sym, sub, m, ce = part
        im, (ox, oy) = self.image(sym, sub, ce)
        if im is None or alpha <= 0.003:
            return
        a, b, c, d, tx, ty = m
        k = self.scale / eo.PX
        S = self.size
        # Пиксель части (u, v) -> координаты персонажа -> холст; PIL берёт обратное отображение.
        A = [a * k, c * k, (tx - self.cx) * self.scale + S / 2 - (a * ox + c * oy) * k,
             b * k, d * k, (ty - self.ground) * self.scale + S * 0.92 - (b * ox + d * oy) * k]
        det = A[0] * A[4] - A[1] * A[3]
        if abs(det) < 1e-9:
            return
        inv = [A[4] / det, -A[1] / det, 0, -A[3] / det, A[0] / det, 0]
        inv[2] = -(inv[0] * A[2] + inv[1] * A[5])
        inv[5] = -(inv[3] * A[2] + inv[4] * A[5])
        layer = im.transform((S, S), Image.AFFINE, inv, resample=Image.BICUBIC)
        if alpha < 0.999:
            r, g, bl, al = layer.split()
            al = al.point(lambda v: int(v * alpha))
            layer = Image.merge("RGBA", (r, g, bl, al))
        canvas.alpha_composite(layer)

    def frame(self, deg):
        """Кадр под углом deg (0 — анфас, по кругу)."""
        mirror = False
        if self.half and deg > 180.0:
            deg = 360.0 - deg
            mirror = True
        views = self.views
        span = 180.0 if self.half else 360.0
        # Соседние нарисованные ракурсы.
        angs = [v[0] for v in views]
        j = max(i for i, a in enumerate(angs) if a <= deg + 1e-6) if deg >= angs[0] else len(angs) - 1
        i2 = j + 1
        if i2 >= len(views):
            if self.half:
                i2 = j
            else:
                i2 = 0
        a0 = angs[j]
        a1 = angs[i2] if i2 != 0 or self.half else span
        t = 0.0 if a1 == a0 else ((deg - a0) % 360.0) / ((a1 - a0) % 360.0 or 360.0)
        A, B = dict((p[0], p) for p in views[j][1]), dict((p[0], p) for p in views[i2][1])
        # Порядок слоёв — как в ближайшем ракурсе.
        order = [p[0] for p in (views[j][1] if t < 0.5 else views[i2][1])]
        for key in (list(A) + list(B)):
            if key not in order:
                order.append(key)
        order.sort(key=lambda k: k)
        # Смена рисунка — чистым переключением посередине: соседние ракурсы
        # нарисованы по-разному, их смешение даёт полупрозрачные двоения.
        sw = 1.0 if t >= 0.5 else 0.0
        canvas = Image.new("RGBA", (self.size, self.size), (0, 0, 0, 0))
        for key in order:
            pa, pb = A.get(key), B.get(key)
            if pa and pb:
                m = [x + (y - x) * t for x, y in zip(pa[3], pb[3])]
                same = (pa[1], pa[2], repr(pa[4])) == (pb[1], pb[2], repr(pb[4]))
                if same:
                    self.draw(canvas, (key, pa[1], pa[2], m, pa[4]), 1.0)
                else:
                    self.draw(canvas, (key, pa[1], pa[2], m, pa[4]), 1.0 - sw)
                    self.draw(canvas, (key, pb[1], pb[2], m, pb[4]), sw)
            elif pa:
                self.draw(canvas, pa, 1.0 - sw)
            elif pb:
                self.draw(canvas, pb, sw)
        if mirror:
            canvas = canvas.transpose(Image.FLIP_LEFT_RIGHT)
        return canvas


def main():
    args = sys.argv[1:]
    step, size = 1.0, 480
    if "--step" in args:
        i = args.index("--step")
        step = float(args[i + 1])
        del args[i:i + 2]
    if "--size" in args:
        i = args.index("--size")
        size = int(args[i + 1])
        del args[i:i + 2]
    name = args[0] if args else "lyra_heartstrings"
    out = os.path.join(ROOT, "official_parts", "_turn", name)
    os.makedirs(out, exist_ok=True)
    turn = Turn(name, size)
    frames = []
    deg = 0.0
    while deg < 360.0 - 1e-6:
        im = turn.frame(deg)
        bg = Image.new("RGBA", im.size, (232, 232, 228, 255))
        bg.alpha_composite(im)
        bg = bg.convert("RGB")
        bg.save(os.path.join(out, f"deg_{int(round(deg)):03d}.png"))
        frames.append(bg)
        deg += step
    frames[0].save(os.path.join(out, f"{name}.gif"), save_all=True, append_images=frames[1:], duration=33, loop=0)
    # Лист: каждые 15°.
    picks = [f for i, f in enumerate(frames) if abs((i * step) % 15.0) < 1e-6]
    cols = 8
    rows = (len(picks) + cols - 1) // cols
    t = size // 2
    sheet = Image.new("RGB", (cols * t, rows * (t + 18)), (255, 255, 255))
    dr = ImageDraw.Draw(sheet)
    font = ImageFont.truetype("arial.ttf", 14)
    for i, f in enumerate(picks):
        x, y = (i % cols) * t, (i // cols) * (t + 18)
        sheet.paste(f.resize((t, t)), (x, y + 18))
        dr.text((x + 4, y + 2), f"{int(i * 15)}°", fill=(200, 0, 0), font=font)
    sheet.save(os.path.join(out, f"{name}_sheet.png"))
    print(f"{len(frames)} кадров -> {out}")


if __name__ == "__main__":
    main()
