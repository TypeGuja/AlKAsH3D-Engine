"""Процедурные бесшовные текстуры (albedo / normal / roughness) + samara.mtl.

Никаких снимков из Google/Яндекса: всё генерируется здесь, поэтому текстуры
свободно используются в игре. Нормали — OpenGL-конвенция (+Y вверх, как ждёт
OBJ/Blender); для DirectX-конвенции инвертируйте зелёный канал.
"""
import numpy as np
from PIL import Image
from scipy import ndimage

import config as C
from materials import MATERIALS

S = 1024
TEX = C.OUT / "textures"


# ---------------------------------------------------------------- шум

def noise(freq, seed, size=S):
    rng = np.random.default_rng(seed)
    g = rng.random((freq, freq)).astype(np.float32)
    return ndimage.zoom(g, size / freq, order=3, mode="grid-wrap")[:size, :size]


def fbm(base, octaves, seed, gain=0.5, size=S):
    out = np.zeros((size, size), np.float32)
    amp, tot, f = 1.0, 0.0, base
    for o in range(octaves):
        if f > size // 2:
            break
        out += amp * noise(f, seed + o * 101, size)
        tot += amp
        amp *= gain
        f *= 2
    return out / tot


def norm01(a):
    a = a - a.min()
    return a / max(a.max(), 1e-6)


def speckle(seed, density, size=S):
    rng = np.random.default_rng(seed)
    return (rng.random((size, size)) < density).astype(np.float32)


def mix(a, b, t):
    t = np.asarray(t)[..., None] if np.ndim(t) == 2 else t
    return np.asarray(a) * (1 - t) + np.asarray(b) * t


def col(rgb, size=S):
    return np.ones((size, size, 3), np.float32) * np.asarray(rgb, np.float32)


def tint(img, n, amount):
    return img * (1 + (n[..., None] - 0.5) * amount)


# ---------------------------------------------------------------- вывод

def normal_from_height(h, strength):
    dx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 0.5 * strength
    dy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 0.5 * strength
    # строка 0 — верх картинки (V=1), поэтому +Y нормали = -d/drow
    n = np.dstack([-dx, dy, np.ones_like(h)])
    n /= np.linalg.norm(n, axis=2, keepdims=True)
    return n * 0.5 + 0.5


def save(name, albedo, height, rough, nstrength=4.0):
    TEX.mkdir(parents=True, exist_ok=True)
    rough = np.broadcast_to(np.asarray(rough, np.float32), (S, S))
    height = np.broadcast_to(np.asarray(height, np.float32), (S, S))
    Image.fromarray((np.clip(albedo, 0, 1) * 255).astype(np.uint8)).save(TEX / f"{name}_albedo.png", optimize=True)
    Image.fromarray((normal_from_height(height.astype(np.float32), nstrength * S / 256) * 255).astype(np.uint8)) \
        .save(TEX / f"{name}_normal.png", optimize=True)
    r = Image.fromarray((np.clip(rough, 0, 1) * 255).astype(np.uint8)).resize((S // 2, S // 2), Image.BILINEAR)
    r.save(TEX / f"{name}_rough.png", optimize=True)


yy, xx = np.mgrid[0:S, 0:S].astype(np.float32)


def rect(mask_or_img, x0, y0, x1, y1, value):
    mask_or_img[int(y0):int(y1), int(x0):int(x1)] = value


def box_mask(x0, y0, x1, y1):
    return (xx >= x0) & (xx < x1) & (yy >= y0) & (yy < y1)


# ---------------------------------------------------------------- земля

def t_grass(name="grass", base=(0.19, 0.30, 0.09), dry=(0.36, 0.36, 0.16), dirt_amt=0.0, seed=1):
    n1 = fbm(4, 7, seed)
    n2 = fbm(16, 5, seed + 7)
    a = mix(col(base), col(dry), np.clip((n1 - 0.45) * 2.2, 0, 1) * 0.6)
    blades = speckle(seed + 3, 0.25)
    blades = ndimage.gaussian_filter(blades, 0.6)
    a = tint(a, n2, 0.35) * (0.85 + 0.3 * blades[..., None])
    h = 0.6 * n2 + 0.4 * blades
    if dirt_amt > 0:
        d = np.clip((fbm(6, 6, seed + 11) - (1 - dirt_amt)) * 5, 0, 1)
        a = mix(a, tint(col((0.30, 0.25, 0.18)), fbm(32, 4, seed + 12), 0.4), d)
        h = h * (1 - d) + d * fbm(32, 4, seed + 13) * 0.5
    save(name, a, h, 0.9 + 0.08 * n2, 3)


def t_forest_floor():
    n = fbm(8, 7, 21)
    a = mix(col((0.22, 0.17, 0.10)), col((0.16, 0.20, 0.08)), norm01(fbm(3, 5, 22)))
    needles = ndimage.gaussian_filter(speckle(23, 0.2), 0.7)
    a = tint(a, n, 0.5) * (0.8 + 0.4 * needles[..., None])
    leaves = np.clip((fbm(40, 3, 24) - 0.6) * 6, 0, 1)
    a = mix(a, col((0.40, 0.28, 0.12)), leaves * 0.6)
    save("forest_floor", a, 0.5 * n + 0.5 * needles, 0.93 + 0.05 * n, 4)


def t_farmland():
    furrow = 0.5 + 0.5 * np.sin(yy / S * 2 * np.pi * 24)
    n = fbm(6, 6, 31)
    a = mix(col((0.34, 0.27, 0.18)), col((0.42, 0.40, 0.22)), norm01(fbm(3, 4, 32)) * 0.6)
    a = tint(a, n, 0.4) * (0.8 + 0.25 * furrow[..., None])
    save("farmland", a, 0.6 * furrow + 0.4 * n, 0.95, 5)


def t_simple(name, c1, c2, seed, grain=0.3, grain_scale=64, nstr=4, rough=0.9):
    n = fbm(4, 7, seed)
    g = fbm(grain_scale, 3, seed + 1)
    a = tint(mix(col(c1), col(c2), norm01(n)), g, grain)
    save(name, a, 0.5 * n + 0.5 * g, rough + 0.06 * (g - 0.5), nstr)


def t_gravel():
    # камни — мелкий шум плюс раздутые точки, сглаженные
    n = fbm(96, 3, 42)
    cells = ndimage.grey_dilation(speckle(43, 0.004), size=(9, 9))
    stones = norm01(ndimage.gaussian_filter(n + cells * 0.6, 1.2))
    a = tint(mix(col((0.42, 0.40, 0.37)), col((0.58, 0.55, 0.50)), stones), fbm(8, 4, 44), 0.3)
    save("gravel", a, stones, 0.88, 6)


def t_asphalt():
    base = fbm(4, 7, 51)
    agg_l = ndimage.gaussian_filter(speckle(52, 0.035), 0.5)
    agg_d = ndimage.gaussian_filter(speckle(53, 0.05), 0.5)
    a = col((0.17, 0.17, 0.18)) * (0.8 + 0.4 * base[..., None])
    a = a + 0.25 * agg_l[..., None] - 0.08 * agg_d[..., None]
    # трещины: тонкие линии от гребней шума
    cr = np.abs(fbm(6, 5, 54) - 0.5)
    cracks = np.clip(1 - cr * 60, 0, 1) * (fbm(3, 3, 55) > 0.55)
    a *= (1 - 0.6 * cracks[..., None])
    # пятна/заплатки
    patch = np.clip((fbm(3, 4, 56) - 0.68) * 8, 0, 1)
    a = mix(a, a * 0.75, patch)
    h = 0.4 * agg_l - 0.3 * agg_d + 0.3 * base - 0.8 * cracks
    save("asphalt", a, h, 0.82 - 0.1 * patch + 0.05 * agg_l, 3)


def t_paving():
    # плитка "кирпичик" 0.2 x 0.1 м вперевязку, тайл 2.4 м
    px = S / 2.4
    bw, bh = 0.2 * px, 0.1 * px
    row = np.floor(yy / bh)
    off = (row % 2) * bw / 2
    colidx = np.floor((xx + off) / bw)
    fx = ((xx + off) % bw) / bw
    fy = (yy % bh) / bh
    joint = np.minimum.reduce([fx, 1 - fx, fy * 2, (1 - fy) * 2]) * bw
    j = np.clip(joint / 2.5, 0, 1)
    rng = np.random.default_rng(61)
    tone = rng.random(int(row.max() + 2) * int(colidx.max() + 3)).astype(np.float32)
    tid = (row * (colidx.max() + 3) + colidx).astype(int) % len(tone)
    tv = tone[tid]
    a = mix(col((0.50, 0.50, 0.50)), col((0.62, 0.58, 0.54)), tv * 0.8)
    a = tint(a, fbm(64, 3, 62), 0.25) * (0.55 + 0.45 * j[..., None])
    save("paving", a, j * 0.8 + 0.2 * fbm(64, 3, 63), 0.75 + 0.15 * (1 - j), 3)


def t_concrete(name="concrete", slab=2.0, tile=4.0, c=(0.56, 0.55, 0.52), seed=71):
    n = fbm(4, 7, seed)
    pores = ndimage.gaussian_filter(speckle(seed + 1, 0.02), 0.4)
    a = tint(col(c), n, 0.3) - 0.1 * pores[..., None]
    per = S * slab / tile
    jx = np.minimum(xx % per, per - xx % per)
    jy = np.minimum(yy % per, per - yy % per)
    joint = np.clip(np.minimum(jx, jy) / 3, 0, 1)
    stains = np.clip((fbm(3, 5, seed + 2) - 0.6) * 4, 0, 1)
    a = a * (0.6 + 0.4 * joint[..., None]) * (1 - 0.2 * stains[..., None])
    save(name, a, 0.3 * n - 0.3 * pores + 0.4 * joint, 0.85 + 0.05 * n, 3)


def t_pitch(name, c, lines=True, seed=81):
    n = fbm(8, 6, seed)
    fib = ndimage.gaussian_filter(speckle(seed + 1, 0.3), 0.5)
    a = tint(col(c), n, 0.25) * (0.88 + 0.2 * fib[..., None])
    save(name, a, 0.5 * fib + 0.5 * n, 0.85, 2)


def t_water():
    n1 = fbm(8, 6, 91)
    n2 = fbm(24, 4, 92)
    a = mix(col((0.05, 0.11, 0.12)), col((0.08, 0.16, 0.17)), n1)
    save("water", a, 0.6 * n2 + 0.4 * n1, 0.05 + 0.05 * n2, 1.5)


def t_far_ground():
    n = fbm(3, 6, 101)
    fields = norm01(fbm(6, 3, 102))
    a = mix(col((0.22, 0.31, 0.12)), col((0.45, 0.42, 0.24)), np.clip((fields - 0.5) * 3, 0, 1))
    forest = np.clip((fbm(4, 5, 103) - 0.55) * 6, 0, 1)
    a = mix(a, col((0.10, 0.17, 0.07)), forest)
    save("far_ground", tint(a, n, 0.3), n, 0.95, 1)


def t_road_marking():
    n = fbm(16, 5, 111)
    wear = np.clip((fbm(8, 5, 112) - 0.62) * 5, 0, 1)
    a = mix(col((0.86, 0.86, 0.83)), col((0.25, 0.25, 0.26)), wear * 0.8)
    save("road_marking", tint(a, n, 0.1), 0.3 * n, 0.6, 1)


def t_ballast():
    n = fbm(128, 2, 121)
    stones = norm01(ndimage.gaussian_filter(n + 0.5 * ndimage.grey_dilation(speckle(122, 0.004), size=(7, 7)), 1.0))
    a = tint(mix(col((0.33, 0.31, 0.29)), col((0.50, 0.47, 0.43)), stones), fbm(6, 4, 123), 0.35)
    h = stones * 0.6
    # бетонные шпалы: 6 шт на 3.3 м (шаг 0.55), длина 2.7 м по центру ширины 4.4 м
    x0, x1 = S * (0.85 / 4.4), S * (3.55 / 4.4)
    step = S / 6.0
    sw = S * 0.3 / 3.3
    for k in range(6):
        y0 = k * step + (step - sw) / 2
        m = box_mask(x0, y0, x1, y0 + sw)
        a[m] = tint(col((0.55, 0.54, 0.51)), fbm(32, 4, 124 + k), 0.2)[m]
        h[m] = 0.9
    # рыжий налёт по центру (ржавчина от колёс)
    rust = np.exp(-((xx - S / 2) / (S * 0.12)) ** 2)
    a = mix(a, a * np.array([1.05, 0.85, 0.7]), rust * 0.5)
    save("ballast", a, h, 0.9, 6)


def t_rail_steel():
    n = fbm(16, 5, 131)
    a = mix(col((0.36, 0.33, 0.31)), col((0.45, 0.28, 0.18)), np.clip((n - 0.4) * 2, 0, 1))
    top = yy < S * 0.25
    a[top] = col((0.62, 0.62, 0.64))[top]
    save("rail_steel", a, n * 0.3, np.where(top, 0.25, 0.6), 1)


# ---------------------------------------------------------------- фасады

def glass_block(a, h, r, x0, y0, x1, y1, frame=(0.92, 0.92, 0.9), fw=10, glass=(0.10, 0.14, 0.18), mull=True, seed=0):
    m_out = box_mask(x0, y0, x1, y1)
    a[m_out] = frame
    h[m_out] = 0.35
    r[m_out] = 0.5
    gx0, gy0, gx1, gy1 = x0 + fw, y0 + fw, x1 - fw, y1 - fw
    m_in = box_mask(gx0, gy0, gx1, gy1)
    grad = np.clip((yy - gy0) / max(gy1 - gy0, 1), 0, 1)
    refl = (0.6 + 0.8 * (1 - grad)) * (0.9 + 0.2 * noise(4, seed))
    g = col(glass) * refl[..., None]
    a[m_in] = g[m_in]
    h[m_in] = 0.1
    r[m_in] = 0.05
    if mull:
        cx = (gx0 + gx1) / 2
        m = box_mask(cx - fw / 2, gy0, cx + fw / 2, gy1)
        a[m] = frame; h[m] = 0.3; r[m] = 0.5


def facade_panel():
    n = fbm(8, 6, 201)
    a = tint(col((0.72, 0.70, 0.66)), n, 0.15)
    mosaic = ndimage.gaussian_filter(speckle(202, 0.3), 0.8)
    a *= (0.92 + 0.12 * mosaic[..., None])
    h = 0.5 + 0.1 * mosaic
    r = np.full((S, S), 0.85, np.float32)
    cw, ch = S / 2, S / 2         # пролёт 3.2 м x этаж 2.8 м
    pxm_x, pxm_y = cw / 3.2, ch / 2.8
    seam = (np.minimum(xx % cw, cw - xx % cw) < 3) | (np.minimum(yy % ch, ch - yy % ch) < 3)
    a[seam] *= 0.55; h[seam] = 0.2
    for fx in range(2):
        for fy in range(2):
            x0 = fx * cw + (cw - 1.5 * pxm_x) / 2
            ybot = (fy + 1) * ch - 0.9 * pxm_y
            glass_block(a, h, r, x0, ybot - 1.5 * pxm_y, x0 + 1.5 * pxm_x, ybot, seed=203 + fx * 2 + fy)
    streak = np.clip((fbm(2, 3, 204) - 0.5), 0, 1) * np.clip((yy / S), 0, 1)
    a *= (1 - 0.25 * streak[..., None])
    save("facade_panel", a, h, r, 3)


def brick_pattern(c_a, c_b, mortar, px_per_m, seed):
    bw, bh, mj = 0.25 * px_per_m, 0.075 * px_per_m, max(1.5, 0.01 * px_per_m)
    row = np.floor(yy / bh)
    off = (row % 2) * bw / 2
    fx = (xx + off) % bw
    fy = yy % bh
    jm = (fx < mj) | (fy < mj)
    rng = np.random.default_rng(seed)
    ids = (row * 997 + np.floor((xx + off) / bw)).astype(np.int64)
    tone = rng.random(4096).astype(np.float32)[ids % 4096]
    a = mix(col(c_a), col(c_b), tone)
    a = tint(a, fbm(64, 3, seed + 1), 0.2)
    a[jm] = mortar
    h = np.where(jm, 0.2, 0.6 + 0.1 * fbm(64, 3, seed + 2))
    return a, h.astype(np.float32)


def facade_brick():
    a, h = brick_pattern((0.80, 0.78, 0.72), (0.70, 0.68, 0.62), (0.62, 0.61, 0.58), S / 6.0, 211)   # силикатный кирпич
    r = np.full((S, S), 0.85, np.float32)
    cw = S / 2; pm = S / 6.0
    for fx in range(2):
        for fy in range(2):
            x0 = fx * cw + (cw - 1.4 * pm) / 2
            ybot = (fy + 1) * cw - 0.85 * pm
            lint = box_mask(x0 - 8, ybot - 1.5 * pm - 0.2 * pm, x0 + 1.4 * pm + 8, ybot - 1.5 * pm)
            a[lint] = (0.6, 0.6, 0.58); h[lint] = 0.7
            glass_block(a, h, r, x0, ybot - 1.5 * pm, x0 + 1.4 * pm, ybot, seed=212 + fx + fy * 2)
            sill = box_mask(x0 - 6, ybot, x0 + 1.4 * pm + 6, ybot + 8)
            a[sill] = (0.45, 0.45, 0.46); h[sill] = 0.8
    save("facade_brick", a, h, r, 4)


def facade_historic():
    n = fbm(6, 6, 221)
    a = tint(col((0.86, 0.73, 0.50)), n, 0.2)      # охра старой Самары
    h = 0.5 + 0.05 * n
    r = np.full((S, S), 0.85, np.float32)
    pm = S / 8.0
    cw = S / 2
    trim = (0.93, 0.91, 0.86)
    for fy in range(2):
        top = fy * cw
        band = box_mask(0, top, S, top + 0.35 * pm)       # карниз
        a[band] = trim; h[band] = 0.85
        for fx in range(2):
            wx0 = fx * cw + (cw - 1.3 * pm) / 2
            wx1 = wx0 + 1.3 * pm
            wy1 = top + cw - 0.9 * pm
            wy0 = wy1 - 2.2 * pm
            sur = box_mask(wx0 - 0.15 * pm, wy0 - 0.1 * pm, wx1 + 0.15 * pm, wy1 + 0.1 * pm)
            cx = (wx0 + wx1) / 2
            arch_r = (wx1 - wx0) / 2 + 0.15 * pm
            arch = (np.hypot(xx - cx, yy - wy0) < arch_r) & (yy < wy0)
            a[sur | arch] = trim; h[sur | arch] = 0.75
            glass_block(a, h, r, wx0, wy0, wx1, wy1, frame=(0.35, 0.25, 0.18), fw=8, seed=222 + fx + fy)
            inner = (np.hypot(xx - cx, yy - wy0) < (wx1 - wx0) / 2 - 8) & (yy < wy0)
            a[inner] = (0.12, 0.15, 0.18); h[inner] = 0.1; r[inner] = 0.05
            keystone = box_mask(cx - 0.12 * pm, wy0 - arch_r - 0.05 * pm, cx + 0.12 * pm, wy0 - arch_r + 0.3 * pm)
            a[keystone] = trim; h[keystone] = 0.9
    # пилястры
    for k in range(3):
        m = box_mask(k * cw - 0.2 * pm, 0, k * cw + 0.2 * pm, S)
        a[m] = tint(col(trim), n, 0.1)[m]; h[m] = 0.7
    save("facade_historic", a, h, r, 4)


def facade_commercial():
    n = fbm(6, 6, 231)
    a = tint(col((0.66, 0.64, 0.60)), n, 0.12)
    h = 0.5 + 0.05 * n
    r = np.full((S, S), 0.7, np.float32)
    pm = S / 8.0
    cw = S / 2
    for fy in range(2):
        for fx in range(2):
            x0 = fx * cw + 0.3 * pm
            y1 = (fy + 1) * cw - 0.5 * pm
            glass_block(a, h, r, x0, y1 - 2.8 * pm, x0 + 3.4 * pm, y1, frame=(0.2, 0.2, 0.22), fw=8,
                        glass=(0.14, 0.18, 0.2), mull=True, seed=232 + fx + fy)
    save("facade_commercial", a, h, r, 3)


def facade_glass():
    a = np.zeros((S, S, 3), np.float32); h = np.zeros((S, S), np.float32); r = np.zeros((S, S), np.float32)
    grad = norm01(fbm(2, 3, 241) + (1 - yy / S) * 0.8)
    a[:] = mix(col((0.10, 0.20, 0.24)), col((0.38, 0.52, 0.58)), grad)
    r[:] = 0.06
    pw, ph = S / 4, S / 2
    mx = np.minimum(xx % pw, pw - xx % pw) < 5
    my = np.minimum(yy % ph, ph - yy % ph) < 5
    spandrel = (yy % ph) > ph * 0.86
    a[spandrel] = (0.12, 0.13, 0.14); r[spandrel] = 0.3; h[spandrel] = 0.2
    a[mx | my] = (0.72, 0.74, 0.76); r[mx | my] = 0.35; h[mx | my] = 0.6
    save("facade_glass", a, h, r, 2)


def wall_industrial():
    rib = 0.5 + 0.5 * np.sin(xx / S * 2 * np.pi * 40)
    n = fbm(4, 6, 251)
    a = tint(col((0.55, 0.60, 0.64)), n, 0.2) * (0.85 + 0.2 * rib[..., None])
    rust = np.clip((fbm(6, 6, 252) - 0.62) * 5, 0, 1) * np.clip(yy / S, 0.2, 1)
    a = mix(a, col((0.45, 0.28, 0.16)), rust * 0.7)
    h = rib * 0.8
    r = 0.55 + 0.3 * rust
    # ленточное остекление вверху (4.4–5.3 м из 6)
    y0, y1 = S * (1 - 5.3 / 6), S * (1 - 4.4 / 6)
    m = box_mask(0, y0, S, y1)
    a[m] = mix(col((0.16, 0.2, 0.22)), col((0.32, 0.38, 0.4)), noise(4, 253))[m]
    h[m] = 0.2; r[m] = 0.1
    frames = m & (np.minimum(xx % (S / 8), S / 8 - xx % (S / 8)) < 4)
    a[frames] = (0.3, 0.3, 0.32)
    save("wall_industrial", a, h, r, 3)


def wall_wood():
    plank = S / 12.0                                   # 12 досок на 3 м
    fy = (yy % plank) / plank
    shade = 0.75 + 0.25 * np.sin(fy * np.pi)
    grain = fbm(4, 6, 261)
    streak = ndimage.zoom(np.random.default_rng(262).random((64, 4)), (S / 64, S / 4), order=1, mode="grid-wrap")[:S, :S]
    a = tint(col((0.55, 0.62, 0.52)), grain * 0.5 + streak * 0.5, 0.25) * shade[..., None]  # крашеная (зелёная) обшивка
    h = shade.astype(np.float32)
    r = np.full((S, S), 0.8, np.float32)
    pm = S / 6.0
    for k in range(2):
        cx = S / 4 + k * S / 2
        x0, x1 = cx - 0.6 * pm, cx + 0.6 * pm
        y1 = S - 0.9 * S / 3                             # подоконник 0.9 м (тайл по v = 3 м)
        y0 = y1 - 1.4 * S / 3
        nal = box_mask(x0 - 0.18 * pm, y0 - 0.35 * pm, x1 + 0.18 * pm, y1 + 0.12 * pm)   # наличник
        a[nal] = (0.94, 0.93, 0.9); h[nal] = 0.9
        glass_block(a, h, r, x0, y0, x1, y1, frame=(0.94, 0.93, 0.9), fw=10, seed=263 + k)
    save("wall_wood", a, h, r, 3)


def wall_garage():
    n = fbm(6, 6, 271)
    a, h = brick_pattern((0.55, 0.28, 0.20), (0.45, 0.22, 0.16), (0.55, 0.52, 0.48), S / 6.0, 272)
    r = np.full((S, S), 0.85, np.float32)
    pv = S / 3.0; pu = S / 6.0
    for k in range(2):
        x0 = k * S / 2 + 0.2 * pu
        x1 = x0 + 2.6 * pu
        y0 = S - 2.2 * pv
        m = box_mask(x0, y0, x1, S)
        rib = 0.5 + 0.5 * np.sin(xx / S * 2 * np.pi * 60)
        g = tint(col((0.36, 0.26, 0.18) if k == 0 else (0.22, 0.32, 0.22)), n, 0.3) * (0.85 + 0.2 * rib[..., None])
        rust = np.clip((fbm(8, 5, 273 + k) - 0.6) * 5, 0, 1)
        g = mix(g, col((0.42, 0.25, 0.13)), rust * 0.6)
        a[m] = g[m]; h[m] = (0.3 + 0.2 * rib)[m]; r[m] = 0.5
        split = box_mask((x0 + x1) / 2 - 2, y0, (x0 + x1) / 2 + 2, S)
        a[split] = (0.1, 0.1, 0.1)
    slab = box_mask(0, 0, S, 0.25 * pv)
    a[slab] = tint(col((0.55, 0.54, 0.5)), n, 0.2)[slab]; h[slab] = 0.8
    save("wall_garage", a, h, r, 3)


def wall_brick_plain():
    a, h = brick_pattern((0.58, 0.27, 0.19), (0.48, 0.22, 0.16), (0.6, 0.57, 0.52), S / 3.0, 281)
    dirt = np.clip((yy / S - 0.85) * 6, 0, 1)          # грязь у земли
    a *= (1 - 0.3 * dirt[..., None])
    save("wall_brick_plain", a, h, 0.85, 4)


def concrete_fence():
    n = fbm(6, 6, 291)
    a = tint(col((0.63, 0.62, 0.58)), n, 0.25)
    pu, pv = S / 4.0, S / 2.5
    # "чешуя" ПО-2: ромбы
    d = (np.abs(((xx / (0.25 * pu)) % 1) - 0.5) + np.abs(((yy / (0.25 * pv)) % 1) - 0.5))
    h = np.clip(1 - d * 1.6, 0, 1).astype(np.float32)
    frame = (xx < 0.08 * pu) | (xx > S - 0.08 * pu) | (yy < 0.1 * pv) | (yy > S - 0.1 * pv)
    h[frame] = 0.9
    a *= (0.85 + 0.2 * h[..., None])
    stains = np.clip((fbm(3, 5, 292) - 0.55) * 4, 0, 1) * (yy / S)
    a *= (1 - 0.35 * stains[..., None])
    save("concrete_fence", a, h, 0.9, 5)


def fence_metal():
    # профнастил С-8, зелёный RAL 6005: шаг профиля ~0.115 м -> 17 рёбер на тайл 2 м
    per = S / 17.0
    f = (xx % per) / per
    prof = np.clip(np.minimum(f, 1 - f) * 4 - 0.4, 0, 1)
    a = tint(col((0.07, 0.24, 0.16)), fbm(8, 5, 301), 0.25) * (0.8 + 0.3 * prof[..., None])
    save("fence_metal", a, prof, 0.45, 3)


def hedge():
    leaves = norm01(ndimage.gaussian_filter(speckle(311, 0.12), 1.2) + fbm(16, 4, 312) * 0.6)
    a = mix(col((0.06, 0.14, 0.04)), col((0.22, 0.36, 0.10)), leaves)
    save("hedge", a, leaves, 0.85, 6)


# ---------------------------------------------------------------- крыши

def roof_flat():
    n = fbm(4, 7, 321)
    grit = ndimage.gaussian_filter(speckle(322, 0.2), 0.5)
    a = tint(col((0.20, 0.20, 0.21)), n, 0.35) * (0.85 + 0.25 * grit[..., None])
    seams = np.minimum(yy % (S / 8), S / 8 - yy % (S / 8)) < 3
    a[seams] *= 0.7
    puddle = np.clip((fbm(3, 5, 323) - 0.66) * 6, 0, 1)
    a = mix(a, a * 0.8, puddle)
    save("roof_flat", a, 0.5 * grit + 0.3 * n - 0.3 * seams, 0.9 - 0.5 * puddle, 3)


def roof_metal():
    per = S / 8.0                              # фальц каждые 0.5 м
    d = np.minimum(xx % per, per - xx % per)
    seam = np.clip(1 - d / 5, 0, 1)
    n = fbm(4, 6, 331)
    a = tint(col((0.30, 0.14, 0.10)), n, 0.2) * (0.9 + 0.2 * seam[..., None])   # RAL 8017-ish
    fade = np.clip((fbm(2, 4, 332) - 0.5) * 2, 0, 1)
    a = mix(a, a * 1.25, fade * 0.5)
    save("roof_metal", a, seam, 0.45 + 0.1 * n, 3)


def roof_tile():
    ph, pw = S / 10.0, S / 10.0
    fy = (yy % ph) / ph
    row = np.floor(yy / ph)
    fx = ((xx + (row % 2) * pw / 2) % pw) / pw
    bump = np.sin(fx * np.pi) * (0.4 + 0.6 * fy)
    n = fbm(8, 5, 341)
    a = tint(col((0.55, 0.25, 0.15)), n, 0.3) * (0.7 + 0.4 * bump[..., None])
    save("roof_tile", a, bump, 0.7, 5)


def dome_gold():
    n = fbm(8, 6, 351)
    a = tint(col((1.0, 0.77, 0.34)), n, 0.12)
    save("dome_gold", a, n * 0.2, 0.2 + 0.1 * n, 1)


# ---------------------------------------------------------------- фонари

def lamp_pole():
    # цинковая "звёздочка" (spangle): крупные кристаллы разного тона + лёгкий вертикальный потёк
    cells = ndimage.zoom(np.random.default_rng(391).random((24, 24)), S / 24, order=0, mode="grid-wrap")[:S, :S]
    fine = fbm(64, 3, 392)
    tone = 0.62 + 0.12 * cells + 0.05 * fine
    streak = np.clip((fbm(2, 4, 393) - 0.55) * 3, 0, 1) * np.clip(yy / S, 0, 1)
    a = np.dstack([tone, tone * 1.01, tone * 1.03]) * (1 - 0.18 * streak[..., None])
    save("lamp_pole", a, 0.3 * cells + 0.2 * fine, 0.35 + 0.2 * cells, 1)


def lamp_housing():
    n = fbm(8, 5, 401)
    a = tint(col((0.78, 0.78, 0.76)), n, 0.08)
    dirt = np.clip((fbm(4, 5, 402) - 0.6) * 3, 0, 1)
    a *= (1 - 0.15 * dirt[..., None])
    save("lamp_housing", a, n * 0.2, 0.5, 1)


def lamp_glass():
    n = fbm(16, 4, 411)
    a = tint(col((0.93, 0.92, 0.88)), n, 0.05)
    save("lamp_glass", a, n * 0.1, 0.2, 0.5)


# ---------------------------------------------------------------- деревья

def bark():
    fis = np.abs(fbm(8, 6, 361) - 0.5)
    stretch = ndimage.zoom(np.random.default_rng(362).random((16, 128)), (S / 16, S / 128), order=3, mode="grid-wrap")[:S, :S]
    h = norm01(stretch * 0.7 + fis)
    a = tint(col((0.30, 0.25, 0.20)), h, 0.6)
    save("bark", a, h, 0.92, 6)


def t_leaves(name, c_dark, c_light, seed, dens):
    leaves = norm01(ndimage.gaussian_filter(speckle(seed, dens), 1.5) + 0.7 * fbm(12, 5, seed + 1))
    a = mix(col(c_dark), col(c_light), leaves)
    save(name, a, leaves, 0.8, 5)


def write_mtl():
    lines = ["# samara.mtl — материалы карты Самары (текстуры процедурные, см. tools/samara_map/textures.py)",
             "# map_Kd = albedo (sRGB), map_Bump/norm = normal map (OpenGL +Y), map_Pr = roughness", ""]
    for name, m in MATERIALS.items():
        ns = max(2.0, (1 - m["rough"]) ** 2 * 900)
        lines += [
            f"newmtl {name}",
            "Ka 0 0 0", "Kd 1 1 1", "Ks 0.04 0.04 0.04", f"Ns {ns:.1f}", "d 1", "illum 2",
            f"Pr {m['rough']:.2f}", f"Pm {m['metal']:.2f}",
            f"map_Kd textures/{name}_albedo.png",
            f"map_Bump -bm 1.0 textures/{name}_normal.png",
            f"norm textures/{name}_normal.png",
            f"map_Pr textures/{name}_rough.png",
            "",
        ]
    (C.OUT / "samara.mtl").write_text("\n".join(lines), encoding="utf-8")


def main():
    import time
    t0 = time.time()
    t_grass("grass")
    t_grass("grass_urban", base=(0.22, 0.30, 0.11), dirt_amt=0.35, seed=5)
    t_grass("meadow", base=(0.33, 0.36, 0.15), dry=(0.52, 0.47, 0.26), seed=9)
    t_forest_floor()
    t_farmland()
    t_simple("dirt", (0.33, 0.26, 0.19), (0.42, 0.34, 0.25), 33, rough=0.9)
    t_simple("sand", (0.72, 0.64, 0.48), (0.80, 0.73, 0.57), 35, grain=0.2, grain_scale=128, nstr=2)
    t_gravel()
    t_asphalt()
    t_paving()
    t_concrete()
    t_pitch("pitch", (0.20, 0.42, 0.16))
    t_pitch("tartan", (0.55, 0.20, 0.14), seed=85)
    t_water()
    t_far_ground()
    t_road_marking()
    t_ballast()
    t_rail_steel()
    facade_panel(); facade_brick(); facade_historic(); facade_commercial(); facade_glass()
    wall_industrial(); wall_wood(); wall_garage(); wall_brick_plain(); concrete_fence(); fence_metal(); hedge()
    roof_flat(); roof_metal(); roof_tile(); dome_gold()
    lamp_pole(); lamp_housing(); lamp_glass()
    bark()
    t_leaves("leaves", (0.07, 0.16, 0.04), (0.30, 0.45, 0.12), 371, 0.08)
    t_leaves("pine_needles", (0.03, 0.10, 0.05), (0.14, 0.26, 0.12), 381, 0.15)
    missing = [m for m in MATERIALS if not (TEX / f"{m}_albedo.png").exists()]
    assert not missing, f"нет текстур для {missing}"
    write_mtl()
    print(f"[tex] {len(MATERIALS)} материалов за {time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
