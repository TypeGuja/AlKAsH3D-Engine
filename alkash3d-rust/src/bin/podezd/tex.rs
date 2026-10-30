//! Процедурные материалы подъезда: для каждого — альбедо (sRGB), карта
//! нормалей (касательное пространство, из карты высот) и metallic-roughness
//! (G — шероховатость). Шум периодический: текстуры повторяются без швов.

pub struct Material {
    pub size: u32,
    pub albedo: Vec<u8>,
    /// RGB — нормаль, A — высота (0..1 от низа до верха рельефа).
    pub normal: Vec<u8>,
    pub mr: Vec<u8>,
    /// Перепад высот рельефа, м, и размер тайла по U и V, м.
    pub relief: f32,
    pub tile_u: f32,
    pub tile_v: f32,
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374761393)
        .wrapping_add((y as u32).wrapping_mul(668265263))
        .wrapping_add(seed.wrapping_mul(2246822519));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

/// Value noise с периодом `px` x `py` узлов решётки (бесшовно на тайле).
fn vnoise(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (xi, yi) = (x0 as i32, y0 as i32);
    let h = |i: i32, j: i32| hash(i.rem_euclid(px), j.rem_euclid(py), seed);
    let (a, b, c, d) = (h(xi, yi), h(xi + 1, yi), h(xi, yi + 1), h(xi + 1, yi + 1));
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

/// Фрактальный шум на тайле: u, v в 0..1, `fx`/`fy` — частота первой октавы
/// (целые — иначе шов), результат 0..1.
fn fbm(u: f32, v: f32, fx: i32, fy: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 0.5, 0.0);
    for o in 0..octaves {
        let k = 1 << o;
        sum += vnoise(u * (fx * k) as f32, v * (fy * k) as f32, fx * k, fy * k, seed.wrapping_add(o * 131)) * amp;
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// ДОБАВЛЕНО (текстуры): клеточный шум Уорли на тайле — по точке на
/// ячейку решётки `fx` x `fy` (с повтором). Возвращает расстояния до
/// ближайшей и второй точки (в долях ячейки) и номер ближайшей ячейки:
/// F2 - F1 ≈ 0 — граница клеток (трещины кракелюра, сетка мозаики).
fn worley(u: f32, v: f32, fx: i32, fy: i32, seed: u32) -> (f32, f32, (i32, i32)) {
    let (x, y) = (u * fx as f32, v * fy as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id) = (9.0f32, 9.0f32, (0, 0));
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (xi + i, yi + j);
            let (wx, wy) = (cx.rem_euclid(fx), cy.rem_euclid(fy));
            let px = cx as f32 + hash(wx, wy, seed);
            let py = cy as f32 + hash(wx, wy, seed + 1);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = (wx, wy);
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, id)
}

/// Один тексель: цвет sRGB 0..1, высота (в метрах), шероховатость.
struct Px {
    c: [f32; 3],
    h: f32,
    r: f32,
}

/// Собрать материал: `f(u, v)` на каждый тексель; нормали — из высот
/// конечными разностями (`world` — размер тайла в метрах, чтобы наклон был
/// честным: высота и шаг текселя в одних единицах).
fn make(size: u32, world: [f32; 2], f: impl Fn(f32, f32) -> Px + Sync) -> Material {
    use rayon::prelude::*;
    let n = size as usize;
    // тексели независимы — по ядрам (атлас ступеней 2048² один считал бы секунды)
    let px: Vec<Px> = (0..n * n).into_par_iter().map(|i| f(((i % n) as f32 + 0.5) / n as f32, ((i / n) as f32 + 0.5) / n as f32)).collect();
    let col: Vec<[f32; 3]> = px.iter().map(|p| p.c).collect();
    let hgt: Vec<f32> = px.iter().map(|p| p.h).collect();
    let rgh: Vec<f32> = px.iter().map(|p| p.r).collect();
    let (dx, dy) = (world[0] / n as f32, world[1] / n as f32);
    let mut albedo = vec![0u8; n * n * 4];
    let mut normal = vec![0u8; n * n * 4];
    let mut mr = vec![0u8; n * n * 4];
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let (hmin, hmax) = hgt.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, c), &h| (a.min(h), c.max(h)));
    let hr = (hmax - hmin).max(1e-9);
    // ИЗМЕНЕНО (текстуры): построчно по ядрам — на 2048² однопоточно
    // это было заметной долей загрузки
    albedo.par_chunks_mut(n * 4).zip(normal.par_chunks_mut(n * 4)).zip(mr.par_chunks_mut(n * 4)).enumerate().for_each(|(y, ((al, no), m))| {
        for x in 0..n {
            let at = |i: usize, j: usize| hgt[(j % n) * n + (i % n)];
            let gx = (at(x + 1, y) - at(x + n - 1, y)) / (2.0 * dx);
            let gy = (at(x, y + 1) - at(x, y + n - 1)) / (2.0 * dy);
            let (nx, ny, nz) = (-gx, -gy, 1.0);
            let l = (nx * nx + ny * ny + nz * nz).sqrt();
            let c = col[y * n + x];
            al[x * 4..x * 4 + 4].copy_from_slice(&[b(c[0]), b(c[1]), b(c[2]), 255]);
            no[x * 4..x * 4 + 4].copy_from_slice(&[b(nx / l * 0.5 + 0.5), b(ny / l * 0.5 + 0.5), b(nz / l * 0.5 + 0.5), b((hgt[y * n + x] - hmin) / hr)]);
            m[x * 4..x * 4 + 4].copy_from_slice(&[0, b(rgh[y * n + x]), 0, 255]);
        }
    });
    Material { size, albedo, normal, mr, relief: hmax - hmin, tile_u: world[0], tile_v: world[1] }
}

/// Скан настоящей поверхности (ambientCG, CC0): по текселю — яркость цвета
/// относительно средней, нормаль XY (DirectX: зелёный вниз по v, как у
/// `make`) и отклонение шероховатости от средней. Даёт процедурному
/// материалу мелкую фактуру; раскладка (полоса, грязь у пола, затёртость)
/// и палитра остаются свои.
pub struct Scan {
    w: usize,
    h: usize,
    /// Физический размер скана, м.
    world: [f32; 2],
    px: Vec<[f32; 4]>,
}

fn read_png(path: &std::path::Path) -> Option<(usize, usize, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    // серые карты (шероховатость) — 1–2 канала, цвет и нормали — 3–4
    let ch = info.color_type.samples();
    let (w, h) = (info.width as usize, info.height as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for p in buf[..w * h * ch].chunks_exact(ch) {
        match ch {
            1 | 2 => rgb.extend([p[0], p[0], p[0]]),
            _ => rgb.extend([p[0], p[1], p[2]]),
        }
    }
    Some((w, h, rgb))
}

/// Загрузить скан `id` из assets/podezd/scan; нет файлов — `None`, материал
/// остаётся чисто процедурным.
pub fn load_scan(id: &str, world: [f32; 2]) -> Option<Scan> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/podezd/scan").join(id);
    let map = |kind: &str| read_png(&dir.join(format!("{id}_1K-PNG_{kind}.png")));
    let (Some(col), Some(nrm), Some(rgh)) = (map("Color"), map("NormalDX"), map("Roughness")) else {
        eprintln!("[ПОДЪЕЗД] скан {id} не найден в {}", dir.display());
        return None;
    };
    let (w, h) = (col.0, col.1);
    if (nrm.0, nrm.1) != (w, h) || (rgh.0, rgh.1) != (w, h) {
        eprintln!("[ПОДЪЕЗД] скан {id}: карты разного размера");
        return None;
    }
    let mut px: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let c = &col.2[i * 3..i * 3 + 3];
            let lum = (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0;
            let n = &nrm.2[i * 3..i * 3 + 3];
            [lum, n[0] as f32 / 127.5 - 1.0, n[1] as f32 / 127.5 - 1.0, rgh.2[i * 3] as f32 / 255.0]
        })
        .collect();
    // яркость — в долях средней, шероховатость — отклонение от средней:
    // фактура не сдвигает общий тон и блеск материала
    let k = 1.0 / px.len() as f32;
    let (ml, mr) = px.iter().fold((0.0, 0.0), |(l, r), p| (l + p[0] * k, r + p[3] * k));
    for p in &mut px {
        p[0] /= ml.max(1e-3);
        p[3] -= mr;
    }
    Some(Scan { w, h, world, px })
}

impl Scan {
    /// Билинейно, с повтором; u, v — в долях скана.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (x, y) = (u * self.w as f32 - 0.5, v * self.h as f32 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let at = |i: i32, j: i32| self.px[j.rem_euclid(self.h as i32) as usize * self.w + i.rem_euclid(self.w as i32) as usize];
        let (xi, yi) = (x0 as i32, y0 as i32);
        let (a, b, c, d) = (at(xi, yi), at(xi + 1, yi), at(xi, yi + 1), at(xi + 1, yi + 1));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * tx;
            let bot = c[i] + (d[i] - c[i]) * tx;
            top + (bot - top) * ty
        })
    }
}

/// Наложить фактуру скана на материал с силой `k` (0..1): яркость цвета,
/// нормаль (whiteout-смешение) и разброс шероховатости. Скан повторяется
/// целое число раз на тайл — иначе на стыке тайлов шов; от этого он чуть
/// растянут, если размеры не кратны.
pub fn detail(m: &mut Material, s: &Scan, k: f32) {
    let n = m.size as usize;
    let ru = (m.tile_u / s.world[0]).round().max(1.0);
    let rv = (m.tile_v / s.world[1]).round().max(1.0);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    use rayon::prelude::*;
    let Material { albedo, normal, mr, .. } = m;
    albedo.par_chunks_mut(n * 4).zip(normal.par_chunks_mut(n * 4)).zip(mr.par_chunks_mut(n * 4)).enumerate().for_each(|(y, ((al, no), mrr))| {
        for x in 0..n {
            let p = s.sample((x as f32 + 0.5) / n as f32 * ru, (y as f32 + 0.5) / n as f32 * rv);
            let i = x * 4;
            let f = 1.0 + (p[0] - 1.0) * k;
            for c in 0..3 {
                al[i + c] = b(al[i + c] as f32 / 255.0 * f);
            }
            let a = [no[i] as f32 / 127.5 - 1.0, no[i + 1] as f32 / 127.5 - 1.0, no[i + 2] as f32 / 127.5 - 1.0];
            let (sx, sy) = (p[1] * k, p[2] * k);
            let sz = (1.0 - sx * sx - sy * sy).max(0.0).sqrt();
            let (nx, ny, nz) = (a[0] + sx, a[1] + sy, a[2] * sz);
            let l = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
            no[i] = b(nx / l * 0.5 + 0.5);
            no[i + 1] = b(ny / l * 0.5 + 0.5);
            no[i + 2] = b(nz / l * 0.5 + 0.5);
            mrr[i + 1] = b(mrr[i + 1] as f32 / 255.0 + p[3] * k);
        }
    });
}

fn srgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

/// Масляная краска нижней части стены. Тайл 2 м по горизонтали и ровно
/// высота панели (1.5 м) по вертикали: v = 0 — верх панели (тёмная
/// разделительная полоса), v = 1 — пол (там затёрто обувью и шваброй).
pub fn wall_paint(size: u32) -> Material {
    let base = srgb(92, 132, 116);
    let plaster = srgb(196, 192, 180);
    let stripe = srgb(92, 46, 38);
    make(size, [2.0, 1.5], |u, v| {
        let tint = fbm(u, v, 3, 2, 3, 11) - 0.5;
        // мазки кистью — вертикальные (вытянутый по v шум)
        let strokes = fbm(u, v, 160, 6, 2, 12) - 0.5;
        let mut c = lerp3(base, [base[0] * 1.08, base[1] * 1.06, base[2] * 1.05], tint * 0.8 + 0.4);
        // штукатурка под краской не ровная: волны ±1.5 мм на 20–50 см
        let wave = (fbm(u, v, 6, 4, 3, 17) - 0.5) * 0.003;
        let mut h = wave + strokes * 0.00006 + fbm(u, v, 48, 36, 3, 13) * 0.00012;
        let mut r = 0.34 + tint * 0.06;
        // сколы краски — до штукатурки
        let chip = fbm(u, v, 24, 18, 4, 14) * (0.55 + 0.45 * smooth(0.6, 1.0, v));
        if chip > 0.7 {
            let k = smooth(0.7, 0.72, chip);
            c = lerp3(c, plaster, k);
            h -= 0.0004 * k;
            r = r + (0.9 - r) * k;
        }
        // затёртость и грязь у пола
        let low = smooth(0.78, 1.0, v);
        let scuff = fbm(u, v, 40, 8, 3, 15);
        c = lerp3(c, [c[0] * 0.62, c[1] * 0.6, c[2] * 0.56], low * (0.5 + scuff * 0.5));
        r += low * 0.25;
        // захватанное руками на высоте ~1 м
        let hands = smooth(0.25, 0.4, v) * (1.0 - smooth(0.45, 0.6, v)) * smooth(0.55, 0.8, fbm(u, v, 10, 4, 3, 16));
        c = lerp3(c, [c[0] * 0.8, c[1] * 0.78, c[2] * 0.74], hands);
        // разделительная полоса сверху (~2 см)
        if v < 0.014 {
            c = stripe;
            r = 0.4;
        }
        Px { c, h, r: r.clamp(0.0, 1.0) }
    })
}

/// Известковая побелка (верх стен, потолки). Тайл 2 x 2 м.
pub fn whitewash(size: u32) -> Material {
    let base = srgb(214, 210, 198);
    make(size, [2.0, 2.0], |u, v| {
        let stains = fbm(u, v, 4, 4, 4, 21);
        let grain = fbm(u, v, 128, 128, 2, 22);
        let streak = fbm(u, v, 90, 5, 2, 23) - 0.5;
        let mut c = lerp3(base, srgb(186, 176, 152), smooth(0.55, 0.85, stains) * 0.7);
        c = lerp3(c, [c[0] * 0.92, c[1] * 0.92, c[2] * 0.92], grain * 0.5);
        let wave = (fbm(u, v, 5, 5, 3, 24) - 0.5) * 0.003;
        let h = wave + grain * 0.0003 + streak * 0.00008 + stains * 0.0004;
        Px { c, h, r: 0.93 }
    })
}

/// Бетонная проступь: u — поперёк марша (вся ширина 1.2 м), v — от носка
/// (v = 0) к подступенку. Посередине затёрто: светлее и глаже.
/// ИЗМЕНЕНО (реализм: все ~70 ступеней были одной текстурой — те же пятна,
/// тот же щебень, та же затёртость посередине): атлас вариантов проступи,
/// ячейка на ступень: столбец — отливка (свой щебень, свои пятна, своя
/// тропа), строка — износ (0 — почти не хожено, 3 — стёрто до лунки: нижние
/// марши, по ним ходят все жильцы). Ячейка — проступь 1.2 x 0.3 м: u от перил
/// (0) к стене, v от носка (0) к подступенку. Сетка 4 x 4, а не полосы в
/// столбик: тайл 4.8 x 1.2 м с тем же соотношением сторон 4:1, что у одной
/// проступи, — глубина рельефа (parallax, одно число в долях тайла по u)
/// ведёт себя как раньше.
pub const TREAD_LEVELS: usize = 4;
pub const TREAD_CASTS: usize = 4;

pub fn treads(size: u32) -> Material {
    let base = srgb(132, 129, 122);
    let (nu, nv) = (TREAD_CASTS as f32, TREAD_LEVELS as f32);
    make(size, [1.2 * nu, 0.3 * nv], move |uu, vv| {
        let (cu, cv) = (((uu * nu) as usize).min(TREAD_CASTS - 1), ((vv * nv) as usize).min(TREAD_LEVELS - 1));
        let (u, v) = (uu * nu - cu as f32, vv * nv - cv as f32);
        let (level, cast) = (cv, cu as u32);
        let s = 31 + cast * 17;
        let agg = fbm(u, v, 96, 24, 3, s);
        let blot = fbm(u, v, 6, 2, 3, s + 1);
        // тропа: ближе к перилам (держатся за поручень), у каждой отливки
        // чуть своя; стирается передняя половина — туда встаёт стопа
        let amp = [0.3, 0.55, 0.8, 1.1][level];
        let path_u = 0.36 + 0.1 * hash(cast as i32, 0, 35);
        let path_w = 0.19 + 0.06 * hash(cast as i32, 1, 35);
        let along = (-((u - path_u) / path_w).powi(2)).exp();
        let wear = amp * along * (1.0 - smooth(0.5, 1.0, v)) * (0.85 + 0.3 * fbm(u, v, 8, 2, 2, s + 2));
        let mut c = lerp3(base, srgb(110, 106, 100), smooth(0.4, 0.7, blot));
        // щебень заполнителя: где стёрто — вскрыт и отполирован
        if agg > 0.7 - 0.12 * wear.min(1.0) {
            let stone = if hash((u * 400.0) as i32, (v * 100.0) as i32, s + 3) > 0.5 { srgb(150, 146, 138) } else { srgb(100, 96, 92) };
            c = lerp3(c, stone, 0.35 + 0.3 * wear.min(1.0));
        }
        c = lerp3(c, [c[0] * 1.15, c[1] * 1.15, c[2] * 1.15], wear.min(1.0) * 0.7);
        // грязь у подступенка и у стен — где не ступают; чем больше ходят,
        // тем больше её натаскано
        let edge = smooth(0.8, 1.0, v).max(1.0 - smooth(0.0, 0.08, u)).max(smooth(0.92, 1.0, u));
        let grime = 0.45 + 0.1 * level as f32 + 0.15 * (1.0 - along) * smooth(0.3, 0.7, fbm(u, v, 4, 1, 3, s + 4));
        c = lerp3(c, [c[0] * 0.6, c[1] * 0.58, c[2] * 0.55], (edge * grime).min(0.85));
        // носок скруглён и выщерблен — на тропе сбит сильнее
        let nose = 1.0 - smooth(0.0, 0.06 + 0.04 * wear.min(1.0), v);
        let h = agg * 0.0006 - wear * 0.0032 - nose * (0.004 + 0.003 * wear) * (0.5 + fbm(u, v, 30, 2, 2, s + 5));
        Px { c, h, r: (0.82 - wear * 0.32).max(0.4) }
    })
}

/// Бетон (подступенки, торцы плит). Тайл 1 x 1 м.
pub fn concrete(size: u32) -> Material {
    let base = srgb(128, 125, 118);
    make(size, [1.0, 1.0], |u, v| {
        let blot = fbm(u, v, 5, 5, 4, 41);
        let pores = fbm(u, v, 160, 160, 2, 42);
        let mut c = lerp3(base, srgb(104, 100, 94), smooth(0.35, 0.75, blot));
        let mut h = blot * 0.0006;
        if pores > 0.72 {
            c = [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7];
            h -= 0.0008;
        }
        Px { c, h, r: 0.88 }
    })
}

/// Метлахская плитка 50 мм: терракота и беж, узор «шашкой с бордюром»,
/// швы 2 мм. Тайл 0.6 м (12 плиток).
pub fn tiles(size: u32) -> Material {
    let red = srgb(146, 66, 46);
    let beige = srgb(196, 172, 136);
    let grout = srgb(96, 90, 82);
    make(size, [0.6, 0.6], |u, v| {
        let (tu, tv) = (u * 12.0, v * 12.0);
        let (iu, iv) = (tu.floor() as i32, tv.floor() as i32);
        let (fu, fv) = (tu.fract(), tv.fract());
        // узор: беж, каждая третья по диагонали — терракота
        let is_red = (iu + iv).rem_euclid(3) == 0 || (iu - iv).rem_euclid(6) == 0;
        let var = hash(iu, iv, 51);
        let mut c = if is_red { red } else { beige };
        c = [c[0] * (0.92 + var * 0.16), c[1] * (0.92 + var * 0.14), c[2] * (0.92 + var * 0.12)];
        let dirt = fbm(u, v, 3, 3, 4, 52);
        c = lerp3(c, [c[0] * 0.7, c[1] * 0.66, c[2] * 0.6], smooth(0.5, 0.8, dirt) * 0.6);
        // ДОБАВЛЕНО (текстуры): глазурь затёрта — у части плиток светлее
        // и матовее (по ним ходят), терракота выцветает сильнее беж
        let worn = smooth(0.4, 0.9, hash(iu, iv, 57)) * smooth(0.3, 0.7, fbm(u, v, 2, 2, 3, 58));
        c = lerp3(c, [c[0] * 1.1 + 0.04, c[1] * 1.08 + 0.04, c[2] * 1.06 + 0.04], worn * if is_red { 0.45 } else { 0.2 });
        let g = 0.002 / 0.05; // ширина шва в долях плитки (2 мм / 50 мм)
        let edge = fu.min(1.0 - fu).min(fv.min(1.0 - fv));
        let in_grout = edge < g * 0.5;
        let chip = hash(iu, iv, 53) > 0.93 && fbm(u, v, 120, 120, 2, 54) > 0.6;
        let mut h: f32;
        let mut r = 0.45 + dirt * 0.2;
        if in_grout {
            c = lerp3(grout, [grout[0] * 0.45, grout[1] * 0.43, grout[2] * 0.4], smooth(0.2, 0.8, dirt));
            h = -0.0015;
            r = 0.92;
        } else {
            // кромка плитки слегка скруглена; каждая посажена на раствор
            // чуть по-своему — наклон и высота (блик ломается от плитки
            // к плитке, как на настоящем полу)
            let (sx, sy) = (hash(iu, iv, 59) - 0.5, hash(iu, iv, 60) - 0.5);
            h = -0.0006 * (1.0 - smooth(g * 0.5, g * 0.5 + 0.05, edge)) + ((fu - 0.5) * sx + (fv - 0.5) * sy) * 0.0012 + (hash(iu, iv, 61) - 0.5) * 0.0005;
            r += worn * 0.2;
            if chip {
                c = lerp3(c, grout, 0.5);
                h -= 0.001;
                r = 0.85;
            }
            // выбитая плитка — серый раствор с отпечатком, на 8 мм ниже
            if hash(iu, iv, 55) > 0.985 {
                let mortar = fbm(u, v, 200, 200, 2, 56);
                c = lerp3(srgb(118, 114, 106), srgb(90, 86, 80), mortar);
                h = -0.008 + mortar * 0.002;
                r = 0.95;
            }
        }
        Px { c, h, r }
    })
}

/// Окрашенный металл (балясины, щитки, почтовые ящики, радиатор): цвет
/// задаётся тоном вершин, текстура — светлая основа с потёртостями.
/// ИЗМЕНЕНО (текстуры): кроме потёртостей — краска в несколько слоёв,
/// сколы до красного грунта (сурика) и дальше до металла, пыль и
/// натёки краски (красили кистью поверх старого).
pub fn painted_metal(size: u32) -> Material {
    make(size, [0.5, 0.5], |u, v| {
        let orange = fbm(u, v, 12, 12, 4, 61);
        let wear = smooth(0.8, 0.86, orange) * 0.7;
        let mut c = lerp3(srgb(236, 236, 236), srgb(70, 62, 56), wear);
        let mut h = fbm(u, v, 64, 64, 3, 62) * 0.00008 - wear * 0.0002;
        let mut r = 0.38 + wear * 0.4;
        // натёки: вертикальные валики краски
        let sag = fbm(u, v, 40, 3, 3, 63);
        h += smooth(0.6, 0.8, sag) * 0.0001;
        // сколы: край чешуйки — грунт-сурик, середина — металл
        let chip = fbm(u, v, 14, 14, 4, 64) * (0.75 + 0.25 * fbm(u, v, 2, 2, 2, 66));
        if chip > 0.72 {
            let k = smooth(0.72, 0.725, chip);
            let bare = smooth(0.745, 0.76, chip);
            c = lerp3(c, srgb(150, 62, 44), k);
            c = lerp3(c, srgb(84, 80, 76), bare);
            h -= 0.00015 * k;
            r += (0.6 - r) * k;
        }
        // пыль и копоть
        let dust = fbm(u, v, 3, 3, 4, 65);
        c = lerp3(c, [c[0] * 0.78, c[1] * 0.76, c[2] * 0.72], smooth(0.45, 0.85, dust) * 0.5);
        r += smooth(0.45, 0.85, dust) * 0.15;
        Px { c, h, r: r.min(1.0) }
    })
}

/// Поручень из ПВХ: тёмно-коричневый, гладкий, захватанный.
pub fn handrail(size: u32) -> Material {
    make(size, [0.5, 0.1], |u, v| {
        let n = fbm(u, v, 40, 4, 3, 71);
        let c = lerp3(srgb(86, 44, 26), srgb(64, 32, 20), n);
        Px { c, h: n * 0.00005, r: 0.25 + n * 0.15 }
    })
}

/// Дермантин двери: стёжка ромбами по шнуру с гвоздиками в узлах, ромбы
/// пухлые. Тайл ровно на полотно 0.85 x 2.0 м (4 x 8 ромбов).
pub fn leatherette(size: u32) -> Material {
    make(size, [0.85, 2.0], |u, v| {
        let (a, b) = (u * 4.0 + v * 8.0, u * 4.0 - v * 8.0);
        let (fa, fb) = (a.fract(), b.rem_euclid(1.0));
        let da = fa.min(1.0 - fa);
        let db = fb.min(1.0 - fb);
        let seam = da.min(db);
        let puff = (da * 2.0).min(1.0) * (db * 2.0).min(1.0);
        let grain = fbm(u, v, 200, 400, 2, 81);
        let mut c = lerp3(srgb(54, 36, 30), srgb(40, 26, 22), grain);
        let mut h = puff.sqrt() * 0.012 + grain * 0.00005;
        let mut r = 0.42 + grain * 0.1;
        // гвоздик с латунной шляпкой в узле
        let (na, nb) = ((a - a.round()).abs(), (b - b.round()).abs());
        let nr = (na * na + nb * nb).sqrt();
        if nr < 0.045 {
            // шляпка — невысокий купол над стёжкой, без отвесного края
            // (отвесный край рельефа POM растягивает в «колышек»)
            let dome = (1.0 - (nr / 0.045).powi(2)).max(0.0).sqrt();
            c = srgb(150, 120, 60);
            h = h.max(0.004 + 0.003 * dome);
            r = 0.3;
        } else if seam < 0.02 {
            c = [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7];
        }
        // по краю полотна затёрто до основы
        let edge = (1.0 - smooth(0.0, 0.05, u.min(1.0 - u))).max(1.0 - smooth(0.0, 0.02, v.min(1.0 - v)));
        c = lerp3(c, srgb(72, 58, 46), edge * 0.5);
        r += edge * 0.2;
        Px { c, h, r }
    })
}

/// Масляная краска по дереву (наличники, рамы, подоконник): белая,
/// пожелтевшая, слоями. Тайл 0.5 м.
/// ИЗМЕНЕНО (текстуры): было гладким цветом. Теперь краска лежит по
/// волокнам дерева (они читаются рельефом сквозь слои), старая масляная
/// краска потрескалась «кракелюром» мелкими клетками, местами отскочила
/// чешуйками до серого состарившегося дерева. Волокна — вдоль v (по
/// длине доски у дверей и стоек рам).
pub fn wood_paint(size: u32) -> Material {
    make(size, [0.5, 0.5], |u, v| {
        let warp = fbm(u, v, 3, 2, 3, 93) * 0.6;
        let grain = fbm(u + warp * 0.05, v, 48, 2, 3, 91);
        let rings = ((u * 60.0 + warp * 6.0).sin() * 0.5 + 0.5) * fbm(u, v, 6, 1, 2, 94);
        let yell = fbm(u, v, 3, 3, 3, 92);
        let mut c = lerp3(srgb(232, 228, 212), srgb(206, 196, 164), yell);
        let mut h = grain * 0.0003 + rings * 0.00015;
        let mut r = 0.32 + yell * 0.12;
        // кракелюр: клетки вытянуты вдоль волокон
        let (f1, f2, _) = worley(u, v, 40, 16, 95);
        let crack = 1.0 - smooth(0.0, 0.06, f2 - f1);
        let aged = smooth(0.35, 0.7, fbm(u, v, 3, 3, 3, 96));
        c = lerp3(c, [c[0] * 0.5, c[1] * 0.48, c[2] * 0.44], crack * aged * 0.8);
        h -= crack * aged * 0.0002;
        // отскочившая краска — до дерева
        // чешуйки вытянуты вдоль волокон и держатся кучками у краёв/ручек
        let chip = fbm(u, v, 32, 6, 4, 97) * (0.7 + 0.3 * fbm(u, v, 3, 3, 2, 98));
        if chip > 0.66 {
            let k = smooth(0.66, 0.67, chip);
            let wood = lerp3(srgb(150, 132, 108), srgb(108, 94, 78), grain);
            c = lerp3(c, wood, k);
            h -= 0.0004 * k;
            r += (0.85 - r) * k;
        }
        Px { c, h, r }
    })
}

/// Снег во дворе: утоптанный, с колеями от следов. Тайл 4 м.
pub fn snow(size: u32) -> Material {
    // ИЗМЕНЕНО (текстуры): было похоже на песок — мелкий рисунок скана
    // («червячки») и ровный тон. Теперь: мягкие перемёты, утоптанные
    // пятна серее и глаже, зерно наста, крупинки-блёстки
    // (почти зеркальные тексели — ловят свет фонаря и мерцают).
    make(size, [4.0, 4.0], |u, v| {
        let drift = fbm(u, v, 2, 2, 5, 101);
        let ripple = fbm(u, v, 6, 3, 3, 103);
        let steps = fbm(u, v, 40, 40, 2, 102);
        let trod = smooth(0.5, 0.85, steps) * smooth(0.35, 0.6, fbm(u, v, 4, 4, 3, 104));
        let mut c = lerp3(srgb(240, 242, 246), srgb(206, 210, 218), trod * 0.6 + (1.0 - drift) * 0.12);
        let r0 = 0.78 - trod * 0.3;
        let mut h = drift * 0.04 + ripple * 0.012 - trod * 0.002;
        // мелкая зернистость наста — без неё снег «пластиковый»
        let crust = fbm(u, v, 160, 160, 2, 105);
        h += crust * 0.0005;
        c = lerp3(c, [c[0] * 0.96, c[1] * 0.96, c[2] * 0.98], crust * 0.5);
        let glint = hash((u * size as f32) as i32, (v * size as f32) as i32, 108);
        let mut r = r0;
        if glint > 0.993 {
            r = 0.08;
            c = srgb(250, 250, 252);
        }
        h += glint * 0.0003;
        Px { c, h, r }
    })
}

/// Фасад панельного дома напротив: панели 3.2 x 2.8 м со швами. Тайл
/// ровно на одну панель.
pub fn facade(size: u32) -> Material {
    make(size, [3.2, 2.8], |u, v| {
        let edge = u.min(1.0 - u) * 3.2;
        let edge_v = v.min(1.0 - v) * 2.8;
        let seam = edge.min(edge_v) < 0.02;
        let blot = fbm(u, v, 3, 3, 4, 111);
        let mut c = lerp3(srgb(178, 170, 156), srgb(140, 134, 124), smooth(0.4, 0.8, blot));
        // потёки под швами
        c = lerp3(c, [c[0] * 0.75, c[1] * 0.75, c[2] * 0.75], smooth(0.7, 1.0, v) * fbm(u, v, 30, 2, 2, 112) * 0.6);
        let mut h = blot * 0.002;
        if seam {
            c = srgb(70, 66, 62);
            h = -0.01;
        }
        Px { c, h, r: 0.9 }
    })
}

/// Самодельный коврик из лоскутов: полосы выцветших тканей, ворс.
pub fn rug(size: u32) -> Material {
    let cols = [srgb(120, 40, 38), srgb(70, 80, 110), srgb(150, 120, 70), srgb(60, 90, 60), srgb(110, 100, 95), srgb(140, 70, 50)];
    make(size, [0.68, 0.44], |u, v| {
        let stripe = (v * 22.0 + fbm(u, v, 6, 2, 2, 121) * 1.5).floor() as i32;
        let c = cols[(hash(stripe, 0, 122) * cols.len() as f32) as usize % cols.len()];
        let fuzz = fbm(u, v, 160, 100, 2, 123);
        let dirt = fbm(u, v, 4, 3, 3, 124);
        let mut c = lerp3(c, [c[0] * 0.55, c[1] * 0.52, c[2] * 0.5], smooth(0.45, 0.8, dirt));
        c = lerp3(c, [c[0] * 1.25, c[1] * 1.25, c[2] * 1.25], fuzz * 0.4);
        Px { c, h: fuzz * 0.002, r: 0.97 }
    })
}

/// Печатное объявление: заголовок жирнее, строки текста из «слов» разной
/// длины, поля; бумага с волокнами, по краям чуть замята. Весь лист —
/// один тайл (u поперёк, v сверху вниз).
pub fn paper(size: u32) -> Material {
    make(size, [0.21, 0.297], |u, v| {
        let fiber = fbm(u, v, 60, 80, 2, 131);
        let mut c = lerp3(srgb(244, 242, 234), srgb(226, 222, 210), fiber * 0.6);
        let line_h = 0.032;
        let in_margin = u > 0.1 && u < 0.9 && v > 0.08 && v < 0.85;
        if in_margin {
            let row = ((v - 0.08) / line_h).floor() as i32;
            let fv = ((v - 0.08) / line_h).fract();
            let title = row < 2;
            let ink_h = if title { 0.55 } else { 0.4 };
            if fv > 0.25 && fv < 0.25 + ink_h && row != 2 {
                // слово/пробел: ячейки по 0.012 u, длина слова — от хэша
                let cell = ((u - 0.1) / 0.011).floor() as i32;
                let mut pos = 0;
                let mut ink = false;
                let mut k = 0;
                while pos <= cell {
                    let len = 2 + (hash(row, k, 132) * 8.0) as i32;
                    if cell < pos + len {
                        ink = true;
                        break;
                    }
                    pos += len + 1;
                    k += 1;
                }
                // последняя строка абзаца короче
                let short = row % 6 == 5 && u > 0.55;
                if ink && !short {
                    let t = if title { 0.15 } else { 0.32 };
                    c = lerp3(c, srgb(40, 40, 46), 1.0 - t);
                }
            }
        }
        let curl = (1.0 - smooth(0.0, 0.06, u.min(1.0 - u).min(v.min(1.0 - v)))) * fbm(u, v, 8, 8, 2, 133);
        Px { c, h: fiber * 0.00005 + curl * 0.002, r: 0.9 }
    })
}

/// ДОБАВЛЕНО (текстуры двора): кирпичная кладка в точке (x, y) метров —
/// ложковая перевязка, кирпич 250 x 65, шов 10 мм. `cols` и `rows` — сколько
/// кирпичей и рядов на `w` x `h` метров (целые — кладка бесшовна на тайле).
/// Возвращает цвет кирпича или шва, высоту (м) и шероховатость.
fn brickwork(x: f32, y: f32, w: f32, h: f32, cols: i32, rows: i32, seed: u32) -> Px {
    let (pw, ph) = (w / cols as f32, h / rows as f32);
    let row = (y / ph).floor() as i32;
    let shift = if row.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
    let bx = x / pw + shift;
    let col = bx.floor() as i32;
    let (fx, fy) = ((bx - bx.floor()) * pw, (y / ph - row as f32) * ph);
    let id = (col.rem_euclid(cols), row.rem_euclid(rows));
    let mortar = 0.01;
    let edge = fx.min(pw - fx).min(fy.min(ph - fy));
    let (u, v) = (x / w, y / h);
    let grit = fbm(u.rem_euclid(1.0), v.rem_euclid(1.0), 64, 64, 3, seed + 7);
    if edge < mortar * 0.5 {
        // шов: серо-бежевый раствор, утоплен на 6 мм, выкрошен местами
        let c = lerp3(srgb(118, 112, 102), srgb(82, 78, 72), grit);
        return Px { c, h: -0.006 - grit * 0.002, r: 0.95 };
    }
    // кирпич: свой тон (от оранжевого до тёмного пережжённого), фактура
    // глины, скруглённая и сбитая кромка
    let t = hash(id.0, id.1, seed);
    let mut c = if t < 0.15 {
        srgb(82, 44, 36)
    } else if t < 0.65 {
        lerp3(srgb(124, 64, 48), srgb(108, 56, 44), hash(id.0, id.1, seed + 1))
    } else {
        lerp3(srgb(138, 80, 62), srgb(118, 70, 56), hash(id.0, id.1, seed + 2))
    };
    // копоть и грязь пятнами по всей кладке
    let grime = fbm(u.rem_euclid(1.0), v.rem_euclid(1.0), 4, 4, 4, seed + 5);
    c = lerp3(c, [c[0] * 0.7, c[1] * 0.68, c[2] * 0.66], smooth(0.4, 0.8, grime) * 0.6);
    let face = fbm(u.rem_euclid(1.0), v.rem_euclid(1.0), 96, 96, 3, seed + 3);
    c = lerp3(c, [c[0] * 0.78, c[1] * 0.74, c[2] * 0.72], face * 0.6);
    let round = smooth(mortar * 0.5, mortar * 0.5 + 0.006, edge);
    let chip = fbm(u.rem_euclid(1.0), v.rem_euclid(1.0), 40, 40, 2, seed + 4);
    let mut hgt = -0.004 * (1.0 - round) + face * 0.0008;
    if chip > 0.76 && edge < 0.02 {
        // сбитый угол: светлее (свежий излом), глубже
        c = lerp3(c, srgb(150, 96, 76), 0.35);
        hgt -= 0.004;
    }
    Px { c, h: hgt, r: 0.86 + face * 0.08 }
}

/// Кирпичная ограда: красный кирпич, местами следы старой побелки и
/// высолы (белый налёт соли от сырости). Тайл 1.04 м: 4 кирпича, 14 рядов.
pub fn brick(size: u32) -> Material {
    make(size, [1.04, 1.04], |u, v| {
        let mut p = brickwork(u * 1.04, v * 1.04, 1.04, 1.04, 4, 14, 141);
        let lime = fbm(u, v, 3, 3, 4, 142);
        let salt = fbm(u, v, 5, 5, 4, 143) * smooth(0.3, 1.0, v);
        // побелка держится в швах и порах дольше, чем на кирпиче
        let k = smooth(0.62, 0.8, lime) * if p.h < -0.005 { 0.9 } else { 0.55 };
        p.c = lerp3(p.c, srgb(212, 206, 194), k);
        p.c = lerp3(p.c, srgb(196, 190, 182), smooth(0.6, 0.85, salt) * 0.45);
        p.r = (p.r + k * 0.05).min(1.0);
        p
    })
}

/// Штукатурка фасада по кирпичу, крашеная, светлая (как на панораме
/// самарского двора): мелкое зерно, неровная затирка, заплатки свежей
/// штукатурки другого тона, волосяные трещины, а где отвалилась — кирпич.
/// Тайл 4.8 м, чтобы повтор заплат и отбитых мест не бросался в глаза.
pub fn plaster(size: u32) -> Material {
    let base = srgb(198, 188, 168);
    make(size, [4.8, 4.8], move |u, v| {
        let grain = fbm(u, v, 384, 384, 2, 151);
        let trowel = fbm(u, v, 10, 10, 4, 152) - 0.5;
        let dirt = fbm(u, v, 3, 3, 5, 153);
        let mut c = lerp3(base, srgb(170, 156, 134), smooth(0.45, 0.8, dirt) * 0.7);
        c = lerp3(c, [c[0] * 0.93, c[1] * 0.93, c[2] * 0.93], grain * 0.6);
        let mut h = trowel * 0.0025 + grain * 0.0005;
        let mut r = 0.9;
        // заплатки: прямоугольники по сетке 8 x 10 (≈0.6 x 0.5 м), край
        // неровный, тон свежее/серее, чуть выше старой поверхности
        let (pu, pv) = (u * 8.0, v * 10.0);
        let (iu, iv) = (pu.floor() as i32, pv.floor() as i32);
        if hash(iu, iv, 154) > 0.86 {
            let wob = (fbm(u, v, 40, 40, 2, 155) - 0.5) * 0.25;
            let inner = (pu.fract().min(1.0 - pu.fract())).min(pv.fract().min(1.0 - pv.fract())) + wob;
            let k = smooth(0.08, 0.12, inner);
            let tone = if hash(iu, iv, 156) > 0.5 { srgb(186, 182, 172) } else { srgb(222, 210, 186) };
            c = lerp3(c, tone, k * 0.8);
            h += k * 0.0015;
        }
        // волосяные трещины: узкий «гребень» шума
        let crack = (fbm(u, v, 7, 7, 4, 157) - 0.5).abs();
        let ck = (1.0 - smooth(0.0, 0.005, crack)) * smooth(0.66, 0.78, fbm(u, v, 4, 4, 3, 158));
        c = lerp3(c, [c[0] * 0.55, c[1] * 0.53, c[2] * 0.5], ck * 0.8);
        h -= ck * 0.0008;
        // отбитая штукатурка: внутри — кирпичная кладка на 15 мм глубже,
        // по краю — светлый свежий скол
        let spall = fbm(u, v, 9, 9, 5, 159) * (0.6 + 0.4 * smooth(0.45, 0.75, fbm(u, v, 2, 2, 2, 161)));
        if spall > 0.69 {
            let k = smooth(0.69, 0.702, spall);
            let br = brickwork(u * 4.8, v * 4.8, 4.8, 4.8, 18, 64, 160);
            let rim = smooth(0.685, 0.692, spall) * (1.0 - smooth(0.698, 0.71, spall));
            c = lerp3(c, br.c, k);
            c = lerp3(c, srgb(214, 206, 190), rim * 0.5);
            h = h * (1.0 - k) + (br.h - 0.006) * k;
            r = r + (br.r - r) * k;
        }
        Px { c, h, r }
    })
}

/// Цоколь: красно-коричневая краска по цементной штукатурке — облезла
/// пятнами до серого, перекрашена заплатами другого тона, высолы. Тайл 1 м.
pub fn plinth(size: u32) -> Material {
    let paint = srgb(104, 50, 40);
    make(size, [1.0, 1.0], move |u, v| {
        let grain = fbm(u, v, 128, 128, 2, 171);
        let blot = fbm(u, v, 3, 3, 4, 172);
        let mut c = lerp3(paint, srgb(84, 44, 38), smooth(0.4, 0.8, blot) * 0.6);
        // перекрашено заплатами (другая краска из другой банки)
        let re = fbm(u, v, 2, 2, 3, 173);
        c = lerp3(c, srgb(114, 60, 46), smooth(0.6, 0.64, re) * 0.8);
        c = lerp3(c, [c[0] * 0.9, c[1] * 0.9, c[2] * 0.9], grain * 0.5);
        let mut h = grain * 0.0004 + (blot - 0.5) * 0.002;
        let mut r = 0.6 + grain * 0.1;
        let peel = fbm(u, v, 8, 8, 5, 174);
        if peel > 0.71 {
            let k = smooth(0.71, 0.72, peel);
            let cem = lerp3(srgb(118, 114, 108), srgb(92, 88, 84), fbm(u, v, 64, 64, 2, 175));
            c = lerp3(c, cem, k);
            h -= 0.0006 * k;
            r += (0.93 - r) * k;
        }
        let salt = fbm(u, v, 6, 6, 4, 176);
        c = lerp3(c, srgb(170, 164, 156), smooth(0.7, 0.9, salt) * 0.35);
        Px { c, h, r }
    })
}

/// Профлист С8 (ограда контейнерной площадки): трапециевидные гофры с
/// шагом 115 мм, высота 8 мм; краска светлая (цвет — тоном вершин), по
/// рёбрам вытерта, ржавые потёки вниз от крепежа. Тайл 0.46 м (4 гофра).
pub fn corrugated(size: u32) -> Material {
    make(size, [0.46, 0.46], |u, v| {
        let ribs = 4.0;
        let f = (u * ribs).fract();
        // трапеция: полка 40%, скосы по 10%
        let prof = smooth(0.0, 0.1, f) * (1.0 - smooth(0.5, 0.6, f));
        let dirt = fbm(u, v, 2, 2, 4, 181);
        let mut c = lerp3(srgb(226, 228, 224), srgb(170, 172, 166), smooth(0.4, 0.8, dirt) * 0.7);
        let mut r = 0.45 + dirt * 0.15;
        // ребро гофра — краска вытерта до металла (серее, глаже)
        let crest = smooth(0.05, 0.1, f) * (1.0 - smooth(0.0, 0.05, (f - 0.1).abs())) + smooth(0.0, 0.03, 0.03 - (f - 0.5).abs());
        c = lerp3(c, srgb(160, 160, 158), crest.min(1.0) * 0.4 * smooth(0.5, 0.8, fbm(u, v, 6, 24, 2, 182)));
        // ржавые потёки: вытянутый вниз шум
        let streak = fbm(u, v, 24, 2, 3, 183);
        let rk = smooth(0.62, 0.8, streak) * smooth(0.3, 0.7, fbm(u, v, 3, 3, 3, 184));
        c = lerp3(c, srgb(150, 80, 40), rk * 0.8);
        r += rk * 0.3;
        Px { c, h: prof * 0.008 + fbm(u, v, 96, 96, 2, 185) * 0.00005, r: r.min(1.0) }
    })
}

/// Крашеная сталь с ржавчиной (ворота гаражей, мусорные баки): краска
/// с «апельсиновой коркой», царапины до металла, ржавые пятна с вздутой
/// вокруг краской и потёки ржавчины вниз. Цвет краски — тоном вершин.
/// Тайл 1 м.
pub fn rusty_steel(size: u32) -> Material {
    make(size, [1.0, 1.0], |u, v| {
        let peel = fbm(u, v, 64, 64, 2, 191);
        let dirt = fbm(u, v, 3, 3, 4, 192);
        let mut c = lerp3(srgb(220, 220, 216), srgb(160, 158, 150), smooth(0.4, 0.85, dirt) * 0.6);
        let mut h = peel * 0.00008;
        let mut r = 0.42 + dirt * 0.2;
        // царапины: тонкие наклонные штрихи
        let s = (u * 0.8 + v * 0.3) * 40.0;
        let si = s.floor() as i32;
        let along = fbm(u, v, 6, 6, 2, 193 + si.rem_euclid(5) as u32);
        if hash(si, 0, 194) > 0.8 && (s.fract() - 0.5).abs() < 0.04 && along > 0.6 {
            c = srgb(120, 118, 114);
            r = 0.35;
            h -= 0.0001;
        }
        // ржавчина: пятна и потёки вниз (v растёт вниз по стене)
        let spot = fbm(u, v, 7, 7, 5, 195);
        let streak = fbm(u, v, 30, 3, 3, 196) * smooth(0.45, 0.75, fbm(u, v, 4, 2, 3, 197));
        let rk = smooth(0.66, 0.7, spot).max(smooth(0.6, 0.78, streak) * 0.7);
        if rk > 0.0 {
            let rust = lerp3(srgb(156, 74, 34), srgb(96, 46, 26), fbm(u, v, 80, 80, 2, 198));
            c = lerp3(c, rust, rk);
            r += (0.9 - r) * rk;
            // вздутая краска по краю пятна
            let ring = smooth(0.6, 0.66, spot) * (1.0 - smooth(0.66, 0.7, spot));
            h += ring * 0.0005 - smooth(0.68, 0.75, spot) * 0.0003;
        }
        Px { c, h, r: r.min(1.0) }
    })
}

/// Альфа-декаль: цвет и альфа (0 — прозрачно) по (u, v).
pub struct Decal {
    pub size: u32,
    pub rgba: Vec<u8>,
}

fn decal(size: u32, f: impl Fn(f32, f32) -> ([f32; 3], f32)) -> Decal {
    let n = size as usize;
    let mut rgba = Vec::with_capacity(n * n * 4);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    for y in 0..n {
        for x in 0..n {
            let (c, a) = f((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
            rgba.extend([b(c[0]), b(c[1]), b(c[2]), b(a)]);
        }
    }
    Decal { size, rgba }
}

/// Копоть над лампой: плотнее всего прямо над колбой и плавно (по Гауссу)
/// сходит на нет, с языками конвекции и рваными пятнами — оседает
/// неравномерно. Ровное плотное ядро с кромкой читалось как дыра в потолке.
pub fn soot(size: u32) -> Decal {
    decal(size, |u, v| {
        let (dx, dy) = (u - 0.5, v - 0.5);
        let r = (dx * dx + dy * dy).sqrt() * 2.0;
        let ang = dy.atan2(dx);
        let tongues = 0.8 + 0.2 * (ang * 5.0 + fbm(u, v, 3, 3, 2, 141) * 6.0).sin();
        let blot = smooth(0.3, 0.75, fbm(u, v, 6, 6, 4, 142));
        let grain = fbm(u, v, 40, 40, 2, 143);
        let fall = (-4.0 * (r / tongues).powi(2)).exp() * (1.0 - smooth(0.85, 1.0, r));
        let a = fall * (0.3 + 0.7 * blot) * (0.8 + 0.2 * grain) * 0.6;
        (srgb(58, 50, 42), a)
    })
}

/// Надписи маркером: несколько «слов» — петли и штрихи от случайного
/// блуждания пера. Тайл — две надписи по u (берётся половина).
pub fn scribble(size: u32) -> Decal {
    let n = size as usize;
    let mut alpha = vec![0.0f32; n * n];
    let mut seed = 151u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0
    };
    for half in 0..2 {
        let words = 2 + (rnd() * 3.0) as usize;
        for w in 0..words {
            let (mut x, mut y) = (0.05 + half as f32 * 0.5 + rnd() * 0.1, 0.2 + w as f32 * 0.27 + rnd() * 0.05);
            let mut vy = 0.0f32;
            let steps = 120 + (rnd() * 160.0) as usize;
            for _ in 0..steps {
                vy += (rnd() - 0.5) * 0.004 - (y - (0.2 + w as f32 * 0.27)) * 0.02;
                let vx = 0.0025 + (rnd() - 0.3) * 0.004;
                x += vx;
                y += vy * 0.6;
                if x > half as f32 * 0.5 + 0.48 {
                    break;
                }
                let (cx, cy) = ((x * n as f32) as i32, (y * n as f32) as i32);
                for oy in -1..=1 {
                    for ox in -1..=1 {
                        let (px, py) = (cx + ox, cy + oy);
                        if px >= 0 && py >= 0 && (px as usize) < n && (py as usize) < n {
                            let a = &mut alpha[py as usize * n + px as usize];
                            *a = a.max(if ox == 0 && oy == 0 { 0.9 } else { 0.55 });
                        }
                    }
                }
            }
        }
    }
    decal(size, |u, v| {
        let (x, y) = (((u * n as f32) as usize).min(n - 1), ((v * n as f32) as usize).min(n - 1));
        ([1.0, 1.0, 1.0], alpha[y * n + x] * (0.75 + 0.25 * fbm(u, v, 40, 40, 2, 152)))
    })
}

/// Паутина в углу: радиальные нити из угла (u = v = 0) и провисшие
/// поперечные дуги между ними, в пыли.
pub fn web(size: u32) -> Decal {
    decal(size, |u, v| {
        let r = (u * u + v * v).sqrt();
        let ang = v.atan2(u); // 0..pi/2
        let spokes = 9.0;
        let fa = (ang / (std::f32::consts::FRAC_PI_2) * spokes).fract();
        let spoke = (1.0 - smooth(0.0, 0.025 / (r + 0.05), fa.min(1.0 - fa))) * smooth(0.0, 0.05, r);
        // дуги: окружности с провисом к середине между спицами
        let sag = (fa * std::f32::consts::PI).sin() * 0.03;
        let rings = ((r + sag) * 14.0).fract();
        let ring = (1.0 - smooth(0.0, 0.06, rings.min(1.0 - rings))) * smooth(0.05, 0.1, r);
        let torn = smooth(0.35, 0.6, fbm(u, v, 6, 6, 3, 161));
        let a = (spoke.max(ring) * 0.55 * torn) * (1.0 - smooth(0.8, 1.0, r));
        ([0.82, 0.8, 0.76], a)
    })
}

/// Атлас цифр 0–9 (штампованные на латуни — тёмные штрихи), 10 ячеек по
/// горизонтали. Каждая цифра — ломаные в квадрате ячейки (0..1, y вниз),
/// растеризуются по расстоянию до отрезков.
pub fn digits() -> (u32, u32, Vec<u8>) {
    fn circle(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<(f32, f32)> {
        (0..=20).map(|k| {
            let a = std::f32::consts::TAU * k as f32 / 20.0;
            (cx + rx * a.cos(), cy + ry * a.sin())
        }).collect()
    }
    let strokes: Vec<Vec<Vec<(f32, f32)>>> = vec![
        vec![circle(0.5, 0.5, 0.27, 0.38)],
        vec![vec![(0.33, 0.25), (0.55, 0.1), (0.55, 0.9)]],
        vec![vec![(0.22, 0.27), (0.35, 0.12), (0.62, 0.1), (0.78, 0.25), (0.76, 0.42), (0.22, 0.9), (0.8, 0.9)]],
        vec![vec![(0.22, 0.14), (0.78, 0.1), (0.45, 0.45), (0.72, 0.53), (0.8, 0.72), (0.62, 0.9), (0.22, 0.86)]],
        vec![vec![(0.65, 0.9), (0.65, 0.1), (0.15, 0.65), (0.85, 0.65)]],
        vec![vec![(0.78, 0.1), (0.28, 0.1), (0.24, 0.45), (0.6, 0.42), (0.8, 0.6), (0.75, 0.84), (0.5, 0.92), (0.2, 0.85)]],
        vec![vec![(0.74, 0.12), (0.46, 0.1), (0.26, 0.34), (0.22, 0.7), (0.4, 0.9), (0.68, 0.88), (0.8, 0.68), (0.66, 0.5), (0.36, 0.5), (0.23, 0.62)]],
        vec![vec![(0.2, 0.1), (0.8, 0.1), (0.42, 0.9)]],
        vec![circle(0.5, 0.29, 0.2, 0.19), circle(0.5, 0.7, 0.24, 0.21)],
        vec![vec![(0.77, 0.38), (0.64, 0.5), (0.34, 0.5), (0.2, 0.32), (0.32, 0.12), (0.6, 0.1), (0.78, 0.3), (0.74, 0.64), (0.56, 0.88), (0.26, 0.88)]],
    ];
    let (cell, h) = (64u32, 64u32);
    let w = cell * 10;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let d = (x / cell) as usize;
            let (u, v) = (((x % cell) as f32 + 0.5) / cell as f32, (y as f32 + 0.5) / h as f32);
            let mut best = 1e9f32;
            for line in &strokes[d] {
                for seg in line.windows(2) {
                    let ((ax, ay), (bx, by)) = (seg[0], seg[1]);
                    let (dx, dy) = (bx - ax, by - ay);
                    let t = (((u - ax) * dx + (v - ay) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                    let (px, py) = (ax + dx * t - u, ay + dy * t - v);
                    best = best.min((px * px + py * py).sqrt());
                }
            }
            let a = 1.0 - smooth(0.055, 0.075, best);
            rgba.extend([30, 24, 16, (a * 255.0) as u8]);
        }
    }
    (w, h, rgba)
}

/// Знак «Осторожно! Электрическое напряжение»: жёлтый треугольник с
/// чёрной каймой и молнией (стрела-зигзаг), выцветший.
pub fn warning(size: u32) -> Decal {
    // точка внутри треугольника с вершинами a, b, c (барицентрически)
    fn tri(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
        let d = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| (p.0 - r.0) * (q.1 - r.1) - (q.0 - r.0) * (p.1 - r.1);
        let (d1, d2, d3) = (d(p, a, b), d(p, b, c), d(p, c, a));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        if neg && pos { 0.0 } else { 1.0 }
    }
    decal(size, |u, v| {
        let p = (u, v);
        let outer = tri(p, (0.5, 0.06), (0.97, 0.9), (0.03, 0.9));
        let inner = tri(p, (0.5, 0.17), (0.86, 0.84), (0.14, 0.84));
        // молния: два наклонных параллелограмма и остриё
        let bolt = tri(p, (0.52, 0.3), (0.42, 0.58), (0.5, 0.58)).max(tri(p, (0.52, 0.3), (0.6, 0.3), (0.5, 0.58)))
            .max(tri(p, (0.47, 0.55), (0.58, 0.55), (0.44, 0.78))).max(tri(p, (0.44, 0.78), (0.36, 0.7), (0.5, 0.7)));
        let fade = 0.85 + 0.15 * fbm(u, v, 6, 6, 3, 171);
        let yellow = [0.95 * fade, 0.78 * fade, 0.12 * fade];
        let black = [0.05, 0.05, 0.05];
        let c = if inner > 0.5 && bolt < 0.5 { yellow } else { black };
        (c, outer)
    })
}

/// Лужицы талого снега: четыре варианта в четвертях текстуры. Вода
/// мутная (грязь с обуви), альфа — форма лужи с рваным краем.
pub fn puddles(size: u32) -> Decal {
    decal(size, |u, v| {
        let (qu, qv) = ((u * 2.0).fract(), (v * 2.0).fract());
        let seed = (u * 2.0) as u32 + 2 * (v * 2.0) as u32;
        let (dx, dy) = ((qu - 0.5) * 2.0, (qv - 0.5) * 2.0);
        let r = (dx * dx + dy * dy).sqrt();
        let edge = 0.62 + 0.3 * (fbm(u, v, 8, 8, 4, 181 + seed) - 0.5) * 2.0;
        let a = 1.0 - smooth(edge - 0.08, edge, r);
        let mud = fbm(u, v, 20, 20, 3, 191);
        ([0.08 + 0.04 * mud, 0.07 + 0.03 * mud, 0.06 + 0.02 * mud], a * 0.38)
    })
}

/// Морозный узор на стекле: ветвистые «папоротники» кристаллов растут от
/// нижнего края и углов; белёсые, полупрозрачные. Тайл — две разные
/// половины по u.
pub fn frost(size: u32) -> Decal {
    decal(size, |u, v| {
        // v = 1 — низ стекла; к низу и к краям — гуще
        let edge = (1.0 - (u * 2.0).fract().min(1.0 - (u * 2.0).fract()) * 2.0).max(0.0);
        let bottom = v;
        let reach = smooth(0.62, 1.05, bottom * 0.85 + edge * 0.3);
        // ветвление: вытянутый шум под 45° и мелкие иглы
        let fern = fbm(u + v * 0.6, v - u * 0.6, 14, 14, 5, 201);
        let needles = fbm(u * 1.0 + v, v * 1.0 - u, 90, 90, 2, 202);
        let a = smooth(0.42, 0.62, fern * 0.7 + needles * 0.3 + reach * 0.45 - 0.2) * reach;
        ([1.0, 1.0, 1.0], a * 0.45)
    })
}

/// Горящее окно напротив: свет комнаты сквозь занавеску — вертикальные
/// складки (ткань тоньше на гребне — светлее), края штор плотнее, в
/// середине щель, где видна лампа под потолком. Четыре варианта по u.
pub fn lit_window(size: u32) -> (u32, u32, Vec<u8>) {
    let n = size as usize;
    let mut rgba = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
            let q = (u * 4.0) as u32;
            let lu = (u * 4.0).fract();
            let gap = [0.08, 0.0, 0.18, 0.04][q as usize];
            let center = 0.5 + [0.0, 0.1, -0.08, 0.05][q as usize];
            let folds = 0.75 + 0.25 * ((lu * [26.0, 34.0, 20.0, 30.0][q as usize]) * std::f32::consts::TAU).sin().powi(2);
            let mut k = folds;
            // щель между шторами — открытый вид в комнату (ярче)
            if (lu - center).abs() < gap {
                k = 1.6 + 0.4 * (1.0 - v);
            }
            // к краям окна шторы собраны плотнее
            let edge = (lu.min(1.0 - lu) / 0.12).min(1.0);
            k *= 0.45 + 0.55 * edge;
            // сверху светлее (лампа под потолком), снизу подоконник темнее
            k *= 0.75 + 0.35 * (1.0 - v) - 0.25 * smooth(0.85, 1.0, v);
            let c = (k.clamp(0.0, 2.0) * 127.0) as u8;
            rgba.extend([c, c, c, 255]);
        }
    }
    (size, size, rgba)
}

/// Грязь на оконном стекле: потёки сверху вниз (дождь по пыли), пыльный
/// налёт к краям рамы, отпечатки. Альфа — доля непрозрачного слоя.
pub fn glass_dirt(size: u32) -> Decal {
    decal(size, |u, v| {
        let base = 0.25 + 0.2 * fbm(u, v, 4, 4, 4, 211);
        // потёки: узкие вертикальные полосы, гуще книзу
        let col = fbm(u, v * 0.05, 24, 1, 3, 212);
        let streak = smooth(0.62, 0.7, col) * (0.3 + 0.7 * v);
        // к краям (у рамы) пыли больше
        let (eu, ev) = ((u * 4.0).fract(), (v * 3.0).fract());
        let edge = 1.0 - smooth(0.0, 0.15, eu.min(1.0 - eu).min(ev.min(1.0 - ev)));
        let a = (base + streak * 0.25 + edge * 0.35).min(1.0);
        ([0.72, 0.7, 0.64], a)
    })
}

/// Трещины в побелке: тонкая ломаная вдоль всей длины (u), с ответвлениями
/// и осыпавшейся каймой. Четыре варианта по v (четверти).
pub fn cracks(size: u32) -> Decal {
    let n = size as usize;
    let mut alpha = vec![0.0f32; n * n];
    let mut seed = 271u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0
    };
    for q in 0..4 {
        let (v0, v1) = (q as f32 * 0.25, (q + 1) as f32 * 0.25);
        let mut paths: Vec<(f32, f32, f32, usize)> = vec![(0.0, (v0 + v1) * 0.5, 0.0, 0)];
        while let Some((mut x, mut y, mut dir, depth)) = paths.pop() {
            let steps = if depth == 0 { n * 2 } else { n / 6 };
            for _ in 0..steps {
                dir = (dir + (rnd() - 0.5) * 0.5).clamp(-0.9, 0.9);
                x += 0.5 / n as f32 * 2.0;
                y += dir * 0.5 / n as f32;
                if x >= 1.0 || y <= v0 + 0.01 || y >= v1 - 0.01 {
                    break;
                }
                let (px, py) = ((x * n as f32) as usize, (y * n as f32) as usize);
                if px < n && py < n {
                    let a = &mut alpha[py * n + px];
                    *a = a.max(if depth == 0 { 0.85 } else { 0.6 });
                    // осыпавшаяся побелка вокруг — светлее основной трещины
                    if py + 1 < n {
                        let b2 = &mut alpha[(py + 1) * n + px];
                        *b2 = b2.max(0.25);
                    }
                }
                if depth < 2 && rnd() < 0.004 {
                    paths.push((x, y, dir + if rnd() < 0.5 { -0.8 } else { 0.8 }, depth + 1));
                }
            }
        }
    }
    decal(size, |u, v| {
        let (x, y) = (((u * n as f32) as usize).min(n - 1), ((v * n as f32) as usize).min(n - 1));
        ([0.28, 0.25, 0.22], alpha[y * n + x])
    })
}

// ---------------------------------------------------------------------------
// ДОБАВЛЕНО (forward-декали двора): потёки и пятна для движковых декалей
// (engine::Decal) — без своей геометрии, проецируются на фасад.
// ---------------------------------------------------------------------------

/// Потёк грязи под подоконником: дождь смывает пыль с отлива — тёмные
/// вертикальные дорожки, гуще у края отлива, к низу сходят на нет.
pub fn dirt_drip(size: u32) -> Decal {
    decal(size, |u, v| {
        // струйки — по u, длина каждой своя
        let col = fbm(u, 0.0, 24, 1, 2, 171);
        let len = 0.35 + 0.6 * fbm(u, 0.5, 9, 1, 2, 172);
        let along = (1.0 - v / len).max(0.0);
        let streak = smooth(0.45, 0.75, col) * along.powf(0.6);
        let wash = (1.0 - v).powi(2) * 0.35 * smooth(0.2, 0.9, fbm(u, v, 6, 6, 3, 173));
        let edge = smooth(0.0, 0.12, u) * smooth(0.0, 0.12, 1.0 - u);
        ((srgb(70, 62, 52)), ((streak * 0.7 + wash) * edge).min(0.8))
    })
}

/// Ржавый потёк под кронштейном газовой трубы: рыжий у кронштейна,
/// узкой дорожкой вниз, с разводами.
pub fn rust_streak(size: u32) -> Decal {
    decal(size, |u, v| {
        let w = 0.08 + 0.1 * v + 0.03 * (fbm(u, v, 4, 8, 2, 181) - 0.5);
        let d = ((u - 0.5) / w).abs();
        let core = (1.0 - d).max(0.0).powf(0.7) * (1.0 - v).powf(1.3);
        let blot = smooth(0.3, 0.8, fbm(u, v, 8, 8, 3, 182));
        let c = lerp3(srgb(150, 70, 25), srgb(95, 45, 20), v);
        (c, (core * (0.6 + 0.4 * blot)).min(0.85))
    })
}

/// Атлас декалей двора 2048² (ячейки 512): 0 — надписи (обе половины
/// scribble), 1 — копоть, 2 — потёк грязи, 3 — ржавчина, 4 — трещины,
/// 5 — затёртая полоса, 6 — грязь у плинтуса.
/// Возвращает пиксели и uv-прямоугольник ячейки по номеру.
pub fn decal_atlas() -> (u32, Vec<u8>, impl Fn(usize) -> [f32; 4]) {
    const N: usize = 2048;
    const C: usize = 512;
    let mut rgba = vec![0u8; N * N * 4];
    let cells: [Decal; 7] = [scribble(C as u32), soot(C as u32), dirt_drip(C as u32), rust_streak(C as u32), cracks(C as u32), scuff(C as u32), baseboard(C as u32)];
    for (k, d) in cells.iter().enumerate() {
        let (cx, cy) = ((k % 4) * C, (k / 4) * C);
        for y in 0..C {
            let src = &d.rgba[y * C * 4..(y + 1) * C * 4];
            let dst = ((cy + y) * N + cx) * 4;
            rgba[dst..dst + C * 4].copy_from_slice(src);
        }
    }
    // полтекселя от края ячейки — фильтр не берёт соседнюю
    let rect = |k: usize| {
        let e = 1.0 / N as f32;
        let (u0, v0) = ((k % 4) as f32 * 0.25, (k / 4) as f32 * 0.25);
        [u0 + e, v0 + e, u0 + 0.25 - e, v0 + 0.25 - e]
    };
    (N as u32, rgba, rect)
}

/// ДОБАВЛЕНО (следы на снегу): отпечаток ботинка — подошва (носок) и
/// каблук, с протектором; примятый снег темнее и голубее по краю вдавлины.
/// Носок — к v = 0.
pub fn footprint(size: u32) -> Decal {
    decal(size, |u, v| {
        let x = (u - 0.5) * 2.0;
        // подошва: эллипс в верхних 2/3, каблук — в нижней трети
        let sole = 1.0 - ((x / 0.85).powi(2) + ((v - 0.33) / 0.33).powi(2));
        let heel = 1.0 - ((x / 0.72).powi(2) + ((v - 0.83) / 0.15).powi(2));
        let shape = smooth(0.0, 0.25, sole.max(heel));
        // протектор: поперечные полосы ёлочкой
        let tread = smooth(0.35, 0.65, (((v * 26.0) + x.abs() * 2.5).fract() - 0.5).abs() * 2.0);
        let rim = smooth(0.0, 0.15, sole.max(heel)) * (1.0 - smooth(0.15, 0.35, sole.max(heel)));
        // вдавлина: её стенки в тени — заметно темнее и холоднее снега вокруг
        let c = lerp3(srgb(120, 128, 145), srgb(80, 86, 102), tread * 0.5 + rim * 0.6);
        (c, shape * (0.6 + 0.25 * tread))
    })
}

/// ДОБАВЛЕНО (износ стен клетки): затёртая полоса на высоте плеча вдоль
/// марша — плечами, сумками, колясками: горизонтальные мазки, к краям реже.
pub fn scuff(size: u32) -> Decal {
    decal(size, |u, v| {
        let band = (-((v - 0.5) / 0.22).powi(2)).exp();
        let smear = smooth(0.5, 0.8, fbm(u, v * 0.15, 28, 2, 3, 191));
        let blot = smooth(0.4, 0.75, fbm(u, v, 6, 3, 3, 192));
        let edge = smooth(0.0, 0.1, u) * smooth(0.0, 0.1, 1.0 - u);
        (srgb(60, 55, 48), (band * (smear * 0.6 + blot * 0.25) * edge).min(0.55))
    })
}

/// Грязь у плинтуса: снизу темнее, следы швабры дугами, брызги.
pub fn baseboard(size: u32) -> Decal {
    decal(size, |u, v| {
        let low = smooth(0.25, 1.0, v);
        let arcs = smooth(0.6, 0.9, ((u * 9.0 + (v * 3.0).sin() * 0.5).fract() - 0.5).abs() * 2.0) * smooth(0.5, 0.9, v);
        let splash = smooth(0.7, 0.85, fbm(u, v, 40, 10, 2, 201)) * smooth(0.3, 0.8, v);
        let edge = smooth(0.0, 0.05, u) * smooth(0.0, 0.05, 1.0 - u);
        (srgb(55, 48, 40), ((low * 0.55 + arcs * 0.15 + splash * 0.3) * edge).min(0.7))
    })
}
