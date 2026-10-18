# Официальные паппеты DHX -> пакет частей для движка (.mlpo).
#
# Персонаж — символ ~XX_Character из XFL. Ракурсы — ключевые кадры под
# меткой "Rotation" (поворот на 360°) плюс кадр 0 (основная поза 3/4).
# Ракурс — части (символы голова/грива/шея/торс/ноги/хвост/тень) с
# матрицей Flash; каждая часть рендерится один раз в своих координатах
# (начало = точка привязки = шарнир), с цветовым эффектом экземпляра
# (затемнение дальних ног и т.п.), и переиспользуется во всех ракурсах.
#
# Формат (little-endian): "MLPO", u32 версия=1, f32 TINT, u32 число текстур;
#   текстура: u32 ширина, u32 высота, RGBA8 строками сверху вниз (цвет —
#   под тонмаппинг движка, как в .mlps; прямая альфа, край растянут).
# u32 число персонажей; персонаж: u32 длина имени, имя utf-8, f32 высота (м),
#   u32 число ракурсов; ракурс: i32 кадр, f32 угол поворота (рад, 0 —
#   анфас, по часовой при взгляде сверху), u32 число частей; часть (снизу
#   вверх): u32 текстура, u32 роль, u32 длина имени, имя, f32 A B C D E F —
#   пиксель (u, v) текстуры -> метры X = A u + C v + E (вправо),
#   Y = B u + D v + F (вверх) от точки "земля под центром"; f32 шарнир X Y.
# Роли: 0 прочее, 1 торс, 2 шея, 3 голова, 4 грива/волосы, 5 передняя
#   нога, 6 задняя нога, 7 хвост, 8 крыло, 9 тень.
# Версия 2: после шарнира части — u32 вид черты (0 — обычная часть), u32 число
#   состояний; состояние: u32 текстура (0xFFFFFFFF — пусто) и f32 A..F. Голова
#   разобрана на слои: основа (глаза, череп...), черты лица по одной со всеми
#   кадрами их символов (эмоции и липсинк DHX), верх (чёлка, рог, уши).
#   Виды черт: 1 веки (Blink), 2 бровь, 3 рот, 4 ноздри, 5 румянец;
#   6 передняя нога, 7 задняя нога — состояния = позы символа ноги.
#
# python export_official.py [имена...] [--out пакет] [--sheet]
import io
import json
import os
import re
import struct
import sys
import xml.etree.ElementTree as ET

import numpy as np
import resvg_py
from PIL import Image, ImageDraw, ImageFont

ROOT = "G:/mlp_characters/mlp_2d"
sys.path.insert(0, os.path.join(ROOT, "tools", "venv", "Lib", "site-packages"))
sys.path.insert(0, "G:/mlp_characters")
from xfl2svg.color_effect import ColorEffect  # noqa: E402
from xfl2svg.svg_renderer import SvgRenderer  # noqa: E402
from xfl2svg.xfl_reader import XflReader  # noqa: E402
from export_engine import TINT, bleed, encode  # noqa: E402
from soften_magic import soften_alpha  # noqa: E402

NS = "{http://ns.adobe.com/xfl/2008/}"
OUT = os.path.join(ROOT, "engine", "official.mlpo")
# Пикселей текстуры на единицу Flash.
PX = 2.0
FIELD = 600
# Метров на единицу Flash: по росту Твайлайт (земля — макушка) = 1.05 м,
# общий масштаб для всех — паппеты нарисованы в одном масштабе сериала.
REF_NAME, REF_HEIGHT = "twilight_sparkle", 1.05

ROLES = [("shadow", 9), ("tail", 7), ("wing", 8), ("hindleg", 6), ("hind", 6), ("backleg", 6), ("frontleg", 5),
         ("foreleg", 5), ("leg", 5), ("neck", 2), ("manebit", 4), ("mane", 4), ("hair", 4), ("head", 3),
         ("horn", 3), ("torso", 1), ("chest", 1), ("body", 1)]


def role_of(symbol):
    s = symbol.lower().replace("_", "").replace(" ", "")
    for key, r in ROLES:
        if key in s:
            return r
    return 0


class Xfl:
    """XFL с кешем рендера символов."""

    def __init__(self, path):
        self.path = path
        self.reader = XflReader(path)
        self.renderer = SvgRenderer(self.reader)
        self.cache = {}

    def length(self, sym):
        return self.reader.get_timeline(sym, "symbol").last_frame + 1

    def render(self, sym, frame):
        """(RGBA, (ox, oy)) — где в картинке начало координат символа; None, если пусто."""
        key = (sym, frame)
        if key in self.cache:
            return self.cache[key]
        svg = self.renderer.render(sym, frame, 100, 100, "symbol", copy=True)
        text = ET.tostring(svg.getroot(), encoding="unicode")
        px = int(2 * FIELD * PX)
        text = re.sub(r'width="[^"]*" height="[^"]*" viewBox="[^"]*"',
                      f'width="{px}px" height="{px}px" viewBox="{-FIELD} {-FIELD} {2 * FIELD} {2 * FIELD}"', text, count=1)
        im = Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(svg_string=text, width=px)))).convert("RGBA")
        bb = im.getbbox()
        if not bb:
            res = (None, (0, 0))
        else:
            o = FIELD * PX
            res = (im.crop(bb), (o - bb[0], o - bb[1]))
        self.cache[key] = res
        return res


def frame_at(layer, idx):
    for fr in layer.iter(NS + "DOMFrame"):
        i, d = int(fr.get("index")), int(fr.get("duration", "1"))
        if i <= idx < i + d:
            return fr
    return None


def sym_root(xfl, sym):
    sys.path.insert(0, os.path.join(ROOT, "tools"))
    from char_frame import sym_file
    return ET.parse(sym_file(xfl.path, sym)).getroot()


def char_parts(xfl, root, idx):
    """Части персонажа на кадре idx, снизу вверх: (символ, кадр символа, матрица, цветэффект,
    черта лица (вид, число кадров) | None, шарнир-подмена (tx, ty) | None). Голова — слоями."""
    layers = [l for l in root.iter(NS + "DOMLayer") if l.get("layerType") not in ("guide", "folder", "mask")
              and l.get("visible") != "false"]
    out = []
    for layer in reversed(layers):
        if layer.get("name", "").upper() == "AUDIO":
            continue
        fr = frame_at(layer, idx)
        if fr is None:
            continue
        els = fr.find(NS + "elements")
        if els is None:
            continue
        for el in els:
            if el.tag != NS + "DOMSymbolInstance":
                continue
            sym = el.get("libraryItemName")
            if "RotationNotes" in sym or "Rotation_Notes" in sym:
                continue
            if "ThisFrameNotFinal" in sym:
                # Кадр помечен аниматорами как недоделанный — ракурс не берём.
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
            mat = [float(m.get(k, d)) if m is not None else d for k, d in (("a", 1), ("b", 0), ("c", 0), ("d", 1), ("tx", 0), ("ty", 0))]
            c = el.find(NS + "color/" + NS + "Color")
            ce = ColorEffect.from_xfl(c) if c is not None else ColorEffect()
            if HEAD_RE.match(sym):
                out.extend(head_parts(xfl, sym, min(sub, n - 1), mat, ce))
            else:
                r = role_of(sym)
                # Ноги: все позы символа (DHX менял позы ноги в шаге) — как состояния.
                leg = (LEG_FRONT, min(n, 16)) if r == 5 else (LEG_HIND, min(n, 16)) if r == 6 else None
                if r in (4, 7) and n >= ANIM_MIN and loop != "single frame":
                    leg = (ANIM, min(n, ANIM_CAP))
                out.append((sym, min(sub, n - 1), mat, ce, leg if n > 1 else None, None))
    return out


# Метка Claude: полуоборот 0..180° равным шагом (один ключ на ракурс), см. skills/animate.
TURN180 = "Turn180"
# Вариант B (магия рога / раскрытые крылья): 33 ракурса после метки Turn180B; в пакете
# их углы сдвинуты на VARIANT_B (движок выбирает ракурсы своего варианта).
TURN180B = "Turn180B"
TURN_COUNT = 33
VARIANT_B = 10.0


HEAD_RE = re.compile(r"^~.*_Head$")
# Виды частей-ног с позами (продолжение видов черт лица).
LEG_FRONT, LEG_HIND = 6, 7
# Анимация по времени (вид 10): текущая грива/хвост аликорнов (длинные таймлайны), свечение магии.
ANIM = 10
ANIM_MIN = 40       # грива/хвост с таймлайном длиннее — зацикленная анимация, короче — набор поз
ANIM_CAP = 100
MAGIC_RE = re.compile(r"(magic|aura)", re.I)
# Кадры черт, которые берёт движок: веки — блоки форм глаз (+100 без верхних ресниц),
# рот/ноздри — два настроения × метки фонем DHX (+0..2 раскрытия).
_PH = (0, 4, 9, 11, 14, 18, 21, 23, 25)
USED = {
    1: {b + o + l for b in (0, 10, 20, 30, 50) for o in range(10) for l in (0, 100)},
    3: {m + p + o for m in (0, 40) for p in _PH for o in range(3)},
    4: {m + p + o for m in (0, 40) for p in _PH for o in range(3)},
}
NONE_TEX = 0xFFFFFFFF
# Черты лица: (имя слоя головы, вид, сколько первых кадров символа выгрузить).
# Имена слоёв у файлов DHX разные: BLINK / BLINK_3-4, LIP SYNC / LIPSYNC / Lypsync, BROW / Brow.
FEATURES = [(re.compile(r"^BLINK(_.*)?$", re.I), 1, 160), (re.compile(r"^(BROW|CLAUDE_BROW.*)$", re.I), 2, 20),
            (re.compile(r"(L[IY]P ?SYNC|^MOUTH$)", re.I), 3, 80), (re.compile(r"NOSTRIL", re.I), 4, 80), (re.compile(r"^CHEEK$"), 5, 8)]


# Веки без блоков эмоций (Луна: только моргание 0–6 и то же без верхних ресниц 100–106):
# формы глаз собираются из кадров её же моргания — 1 чуть опущено, 2 почти закрыто, 3 закрыто, 4 на середине.
# Блок → кадр формы: half lid 1, squinty 2, cheerie 3, sad 4 (веко на середине — тяжёлый грустный взгляд).
BLINK_EMO = {10: 1, 20: 2, 30: 3, 50: 4}


def blink_frame(length, k):
    """Кадр символа век для состояния k (раскладка движка) у символа длиной length."""
    if length >= 110:
        return k
    base = 100 if k >= 100 and length > 100 else 0
    off = k % 100
    blk, o = off // 10 * 10, off % 10
    e = BLINK_EMO.get(blk)
    if e is None:
        return base + min(o, 6)
    return base + (0 if o == 0 else 3 if o in (2, 3) else e)


def feature_of(layer_name):
    for rx, kind, cap in FEATURES:
        if rx.search(layer_name):
            return kind, cap
    return None


def compose(A, B):
    """Матрицы Flash [a b c d tx ty]: сначала B, потом A."""
    a1, b1, c1, d1, x1, y1 = A
    a2, b2, c2, d2, x2, y2 = B
    return [a1 * a2 + c1 * b2, b1 * a2 + d1 * b2, a1 * c2 + c1 * d2, b1 * c2 + d1 * d2, a1 * x2 + c1 * y2 + x1, b1 * x2 + d1 * y2 + y1]


def el_matrix(el):
    m = el.find(NS + "matrix/" + NS + "Matrix")
    return [float(m.get(k, d)) if m is not None else d for k, d in (("a", 1), ("b", 0), ("c", 0), ("d", 1), ("tx", 0), ("ty", 0))]


def group_symbol(xfl, head, keep, tag):
    """Синтетический символ: голова, в которой видны только слои keep (остальные — guide)."""
    name = f"{head}::{tag}" if tag.startswith("L") else f"{head}::{tag}_{abs(hash(tuple(sorted(keep)))) % 10**8}"
    if name not in xfl.reader.symbols:
        layers_el = sym_root(xfl, head).find(".//" + NS + "layers")
        for i, l in enumerate(layers_el.findall(NS + "DOMLayer")):
            if i not in keep:
                l.set("layerType", "guide")
        xfl.reader.symbols[name] = xfl.reader.setup_timeline(layers_el)
    return name


def head_parts(xfl, sym, sub, H, ce):
    """Голова слоями, снизу вверх: (символ, кадр, матрица, цветэффект, черта|None, шарнир)."""
    layers = sym_root(xfl, sym).find(".//" + NS + "layers").findall(NS + "DOMLayer")
    # Черты — только выше глаз (слоёв-масок): ниже бывают одноимённые слои (рот внутри
    # черепа у Bon Bon), а глаза с масками должны остаться целой группой.
    eye = min([i for i, l in enumerate(layers) if l.get("layerType") == "mask"] or [len(layers)])
    feats = {i: (feature_of(l.get("name", "")) if i < eye else None) for i, l in enumerate(layers)}
    fidx = [i for i, f in feats.items() if f]
    piv = (H[4], H[5])
    if not fidx:
        return [(sym, sub, H, ce, None, piv)]
    lo, hi = min(fidx), max(fidx)
    # Слои с магией рога (HornMagic/Aura): в этом кадре — отдельная анимированная часть.
    magic = {}
    for i, l in enumerate(layers):
        fr = frame_at(l, sub)
        els = fr.find(NS + "elements") if fr is not None else None
        for el in (els if els is not None else []):
            if el.tag == NS + "DOMSymbolInstance" and MAGIC_RE.search(el.get("libraryItemName", "")):
                magic[i] = el
    def magic_part(i):
        el = magic[i]
        fs = el.get("libraryItemName")
        n = xfl.length(fs)
        return (fs, 0, compose(H, el_matrix(el)), ce, (ANIM, min(n, ANIM_CAP)) if n > 1 else None, piv)
    out = [(group_symbol(xfl, sym, set(range(hi + 1, len(layers))) - set(magic), "under"), sub, H, ce, None, piv)]
    out += [magic_part(i) for i in sorted(magic, reverse=True) if i > hi]
    for i in range(hi, lo - 1, -1):
        l = layers[i]
        f = feats[i]
        brow_guide = l.get("name", "").startswith("CLAUDE_BROW")
        if l.get("layerType") in ("guide", "folder") and not brow_guide:
            continue
        if i in magic:
            out.append(magic_part(i))
            continue
        if f is None:
            out.append((group_symbol(xfl, sym, {i}, f"L{i}"), sub, H, ce, None, piv))
            continue
        fr = frame_at(l, sub)
        els = fr.find(NS + "elements") if fr is not None else None
        if els is None:
            continue
        for el in els:
            if el.tag != NS + "DOMSymbolInstance":
                continue
            fs = el.get("libraryItemName")
            n = xfl.length(fs)
            ff = int(el.get("firstFrame", "0"))
            if el.get("loop", "loop") != "single frame":
                ff = (ff + sub - int(fr.get("index"))) % max(n, 1)
            c = el.find(NS + "color/" + NS + "Color")
            fce = ColorEffect.from_xfl(c) if c is not None else ce
            kind, cap = f
            if kind == 5 and not re.search(r"cheek|blush", fs, re.I):
                # в слое CHEEK в этом кадре не румянец (у Селестии — ухо): обычная часть одним кадром
                out.append((fs, min(ff, n - 1), compose(H, el_matrix(el)), fce, None, piv))
                continue
            out.append((fs, min(ff, n - 1), compose(H, el_matrix(el)), fce, (kind, cap if kind == 1 and n < 110 else min(cap, n)), piv))
    out.append((group_symbol(xfl, sym, set(range(0, lo)) - set(magic), "over"), sub, H, ce, None, piv))
    out += [magic_part(i) for i in sorted(magic, reverse=True) if i < lo]
    return out


def rotation_label(root):
    """(начало, длина) основной метки Rotation или None (см. rotation_frames).
    Метка Turn180, если есть, важнее: до конца таймлайна персонажа."""
    t = [int(fr.get("index")) for fr in root.iter(NS + "DOMFrame") if fr.get("name") == TURN180]
    if t:
        return (t[0], TURN_COUNT)
    labs = [(int(fr.get("index")), int(fr.get("duration", "1"))) for fr in root.iter(NS + "DOMFrame") if fr.get("name") == "Rotation"]
    if not labs:
        return None
    at100 = [l for l in labs if l[0] == 100]
    short = [l for l in labs if l[1] < 100]
    return at100[0] if at100 else max(short or labs, key=lambda l: l[1])


# Поворот короче — только пол-оборота (анфас -> бок -> спина), второй бок
# аниматоры получали отражением.
HALF_TURN_MAX = 20


def rotation_frames(root):
    """Ключевые кадры под меткой Rotation (без звуковых слоёв), по порядку.

    Меток Rotation бывает несколько (у части персонажей — ещё на 0–99 и
    короткие в конце): основной поворот — метка на кадре 100, иначе самая
    длинная короче 100 кадров."""
    lab = rotation_label(root)
    if lab is None:
        return []
    a, n = lab
    starts = set()
    for layer in root.iter(NS + "DOMLayer"):
        if layer.get("layerType") in ("guide", "folder") or layer.get("name", "").upper() == "AUDIO":
            continue
        for fr in layer.iter(NS + "DOMFrame"):
            i = int(fr.get("index"))
            if a <= i < a + n:
                starts.add(i)
    return sorted(starts)


def apply_effect(im, ce):
    if ce == ColorEffect():
        return im
    a = np.asarray(im).astype(np.float32) / 255.0
    mul = np.array(ce.multiplier, np.float32)
    off = np.array(ce.offset, np.float32)
    # Цветэффект Flash — к непремноженному цвету.
    out = np.clip(a * mul + off, 0, 1)
    return Image.fromarray((out * 255 + 0.5).astype(np.uint8), "RGBA")


def export(names, out_path, sheet):
    roster = json.load(open(os.path.join(ROOT, "roster.json"), encoding="utf-8"))
    xfls = {}
    # Текстуры не держим в памяти (их тысячи): сырые байты — сразу во временный файл рядом
    # с пакетом, для листа — сжатый PNG. В списке: (смещение в файле, w, h, png).
    textures = []
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    spill_path = out_path + ".tex.tmp"
    spill = open(spill_path, "w+b")
    tex_key = {}
    tex_hash = {}
    chars = []
    meters = None
    for name in names:
        r = roster[name]
        if r["source"] != "official":
            print("пропуск (не официальный):", name)
            continue
        # MLPO_XFL="имя=путь;..." — подменить XFL персонажа (например, копия с Turn180).
        over = dict(kv.split("=", 1) for kv in os.environ.get("MLPO_XFL", "").split(";") if "=" in kv)
        xp = over.get(name) or os.path.join(ROOT, r["xfl"])
        if xp not in xfls:
            xfls[xp] = Xfl(xp)
        xfl = xfls[xp]
        root = sym_root(xfl, r["symbol"])
        rot_all = rotation_frames(root)
        # Углы — по месту в полном круге, даже если часть кадров пропущена.
        lab = rotation_label(root)
        if any(fr.get("name") == TURN180 for fr in root.iter(NS + "DOMFrame")):
            # Ровный полуоборот: последний ключ (пустой хвост после ракурсов) не в счёт.
            rot_all = [f for f in rot_all if char_parts(xfl, root, f)]
            angles = {f: np.pi * i / max(len(rot_all) - 1, 1) for i, f in enumerate(rot_all)}
            tb = [int(fr.get("index")) for fr in root.iter(NS + "DOMFrame") if fr.get("name") == TURN180B]
            if tb:
                rb = [tb[0] + i for i in range(TURN_COUNT)]
                angles.update({f: VARIANT_B + np.pi * i / (TURN_COUNT - 1) for i, f in enumerate(rb)})
                rot_all = rot_all + rb
        elif lab and lab[1] <= HALF_TURN_MAX:
            # Пол-оборота: от анфаса (0) до спины (π) включительно; второй бок — зеркало в движке.
            angles = {f: np.pi * i / max(len(rot_all) - 1, 1) for i, f in enumerate(rot_all)}
        else:
            angles = {f: 2 * np.pi * i / max(len(rot_all), 1) for i, f in enumerate(rot_all)}
        angles.setdefault(0, np.pi / 4)
        frames = [0] + [f for f in rot_all if f != 0]
        views = []
        seen = set()
        skipped = 0
        for f in frames:
            parts = char_parts(xfl, root, f)
            if parts is None:
                skipped += 1
                continue
            # Пустые недоделки: не помечены, но почти ничего не рисуют.
            if sum(1 for s, sub, m, ce, *_ in parts if xfl.render(s, sub)[0] is not None) < 3:
                skipped += 1
                continue
            sig = tuple((s, sub, tuple(round(x, 2) for x in m), ce) for s, sub, m, ce, *_ in parts)
            if not parts or sig in seen:
                continue
            seen.add(sig)
            views.append((f, parts))
        # Земля и центр — по кадру 0: тень, иначе низ рисунка.
        f0 = views[0][1]
        ground = None
        xs, ys = [], []
        for sym, sub, m, ce, *_ in f0:
            im, (ox, oy) = xfl.render(sym, sub)
            if im is None:
                continue
            if role_of(sym) == 9:
                ground = m[5]
                continue
            a, b, c, d, tx, ty = m
            for u, v in ((0, 0), (im.width, 0), (0, im.height), (im.width, im.height)):
                x, y = (u - ox) / PX, (v - oy) / PX
                xs.append(a * x + c * y + tx)
                ys.append(b * x + d * y + ty)
        top, bottom = min(ys), max(ys)
        ground = bottom if ground is None else ground
        cx = (min(xs) + max(xs)) / 2
        if meters is None:
            ref = REF_NAME if REF_NAME in names else name
            if ref != name:
                print("масштаб — по", name)
            meters = REF_HEIGHT / (ground - top)
        s = meters
        height = (ground - top) * s
        cv = []

        def tex(sym, sub, ce):
            """(индекс текстуры, начало координат) или None; одинаковые картинки — одна текстура."""
            key = (xp, sym, sub, ce)
            if key not in tex_key:
                im, origin = xfl.render(sym, sub)
                if im is None:
                    tex_key[key] = None
                else:
                    im = apply_effect(im, ce)
                    if MAGIC_RE.search(sym):
                        # Свечение магии: фильтров Flash xfl2svg не знает — смягчаем альфу сами.
                        arr = np.asarray(im).copy()
                        arr[:, :, 3] = soften_alpha(arr[:, :, 3])
                        im = Image.fromarray(arr, "RGBA")
                    h = hash((im.tobytes(), im.size))
                    if h in tex_hash:
                        tex_key[key] = (tex_hash[h], origin)
                    else:
                        rgba = bleed(np.asarray(im))
                        rgba[:, :, :3] = encode(rgba[:, :, :3])
                        off = spill.tell()
                        spill.write(rgba.tobytes())
                        png = io.BytesIO()
                        im.save(png, "PNG", compress_level=1)
                        textures.append((off, im.width, im.height, png.getvalue()))
                        tex_hash[h] = len(textures) - 1
                        tex_key[key] = (len(textures) - 1, origin)
            return tex_key[key]

        def affine(m, origin):
            ox, oy = origin
            a, b, c, d, tx, ty = m
            return (s * a / PX, -s * b / PX, s * c / PX, -s * d / PX,
                    s * (tx - (a * ox + c * oy) / PX - cx), s * (ground - ty + (b * ox + d * oy) / PX))

        for f, parts in views:
            pl = []
            for sym, sub, m, ce, feat, pv in parts:
                t = tex(sym, sub, ce)
                if t is None and feat is None:
                    continue
                ti, aff = (t[0], affine(m, t[1])) if t else (NONE_TEX, (0.0,) * 6)
                px, py = pv if pv else (m[4], m[5])
                states = None
                if feat:
                    kind, n = feat
                    states = (kind, [])
                    for k in range(n):
                        # Кадры, которые движок никогда не берёт (Expression в official.rs), — пустые.
                        if k not in USED.get(kind, range(n)):
                            states[1].append((NONE_TEX, (0.0,) * 6))
                            continue
                        tk = tex(sym, blink_frame(xfl.length(sym), k) if kind == 1 else k, ce)
                        states[1].append((tk[0], affine(m, tk[1])) if tk else (NONE_TEX, (0.0,) * 6))
                role = 3 if ("::" in sym or (feat and (feat[0] < LEG_FRONT or MAGIC_RE.search(sym)))) else role_of(sym)
                pl.append((ti, role, sym, aff, (s * (px - cx), s * (ground - py)), states))
            cv.append((f, pl))
        # Угол ракурса: кадр 0 — основная поза (3/4), поворот — равномерно по
        # кругу от анфаса (первый ключ) до последнего.
        chars.append((name, height, [(f, float(angles[f]), pl) for f, pl in cv]))
        # Кэш рендера символов нужен только в пределах одного персонажа.
        xfl.cache.clear()
        print(f"{name}: {len(cv)} ракурсов (недоделанных пропущено {skipped}), высота {height:.2f} м, текстур всего {len(textures)}", flush=True)

    spill.flush()
    with open(out_path, "wb") as fo:
        fo.write(b"MLPO" + struct.pack("<IfI", 2, TINT, len(textures)))
        for off, w, h, _ in textures:
            spill.seek(off)
            fo.write(struct.pack("<II", w, h) + spill.read(w * h * 4))
        fo.write(struct.pack("<I", len(chars)))
        for name, height, views in chars:
            nb = name.encode("utf-8")
            fo.write(struct.pack("<I", len(nb)) + nb + struct.pack("<fI", height, len(views)))
            for f, ang, pl in views:
                fo.write(struct.pack("<ifI", f, ang, len(pl)))
                for ti, role, sym, aff, piv, states in pl:
                    sb = sym.encode("utf-8")
                    fo.write(struct.pack("<II", ti, role) + struct.pack("<I", len(sb)) + sb + struct.pack("<6f", *aff) + struct.pack("<2f", *piv))
                    kind, st = states if states else (0, [])
                    fo.write(struct.pack("<II", kind, len(st)))
                    for sti, saff in st:
                        fo.write(struct.pack("<I", sti) + struct.pack("<6f", *saff))
    spill.close()
    os.remove(spill_path)
    print("пакет", out_path, os.path.getsize(out_path) // 1024, "КБ")
    if sheet:
        make_sheet(chars, textures, out_path)


def make_sheet(chars, textures, out_path):
    """Контрольный лист: каждый ракурс, собранный из данных пакета (как в движке)."""
    T = 220
    cols = max(len(v) for _, _, v in chars)
    font = ImageFont.truetype("arial.ttf", 13)
    sheet = Image.new("RGB", (cols * T, len(chars) * (T + 18)), (255, 255, 255))
    dr = ImageDraw.Draw(sheet)
    for row, (name, height, views) in enumerate(chars):
        k = (T - 20) / max(height * 1.15, 0.5)  # пикселей на метр
        for col, (f, ang, pl) in enumerate(views):
            canvas = Image.new("RGBA", (T, T), (255, 255, 255, 0))
            for ti, role, sym, (A, B, C, D, E, F), piv, states in pl:
                if ti == NONE_TEX or (states and states[0] == 2):
                    continue
                im = Image.open(io.BytesIO(textures[ti][3])).convert("RGBA")
                # экран: x = T/2 + k X, y = T - 10 - k Y
                M = [k * A, k * C, T / 2 + k * E, -k * B, -k * D, T - 10 - k * F]
                det = M[0] * M[4] - M[1] * M[3]
                if abs(det) < 1e-9:
                    continue
                inv = [M[4] / det, -M[1] / det, 0, -M[3] / det, M[0] / det, 0]
                inv[2] = -(inv[0] * M[2] + inv[1] * M[5])
                inv[5] = -(inv[3] * M[2] + inv[4] * M[5])
                canvas.alpha_composite(im.transform((T, T), Image.AFFINE, inv, resample=Image.BICUBIC))
            x, y = col * T, row * (T + 18)
            sheet.paste(canvas, (x, y + 18), canvas)
            dr.text((x + 3, y + 2), f"{name[:14]} {f} {np.degrees(ang):.0f}°", fill=(200, 0, 0), font=font)
    p = os.path.splitext(out_path)[0] + "_sheet.png"
    sheet.save(p)
    print("лист", p, sheet.size)


if __name__ == "__main__":
    args = sys.argv[1:]
    out = OUT
    if "--out" in args:
        i = args.index("--out")
        out = args[i + 1]
        del args[i:i + 2]
    sheet = "--sheet" in args
    args = [a for a in args if a != "--sheet"]
    roster = json.load(open(os.path.join(ROOT, "roster.json"), encoding="utf-8"))
    names = args or [n for n, r in roster.items() if r["source"] == "official"]
    if REF_NAME in names:
        names.remove(REF_NAME)
        names.insert(0, REF_NAME)
    export(names, out, sheet)
