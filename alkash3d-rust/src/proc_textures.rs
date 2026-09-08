// src/proc_textures.rs
//! Процедурные текстуры для My Summer Car-like демо (`main_car.rs`).
//!
//! ПОЧЕМУ процедурные, а не PNG/JPG с диска: в движке нет декодера
//! растровых форматов вообще (см. `Cargo.toml` — ни `image`, ни `png` не
//! подключены; собственный формат `.altex` хранит ТОЛЬКО уже готовые сырые
//! RGBA8-байты, см. `AltexFile::add_texture` в `altex_format.rs`, он их не
//! декодирует, а просто копирует). Тащить внешний крейт-декодер ради
//! нескольких текстур — лишняя зависимость; тащить чужие/скачанные
//! изображения — прямое нарушение подхода "своя игра, не копия ассетов My
//! Summer Car" (см. шапку `main_car.rs`). Вместо этого текстуры считаются
//! на CPU при старте сцены (доли миллисекунды на текстуру такого размера,
//! один раз, не каждый кадр) и грузятся в GPU через
//! `AlkashEngine::create_texture_rgba`.
//!
//! Общий метод — fractal value noise (`fbm`, несколько октав `value_noise`)
//! поверх процедурного узора (доски/кладка/гофра/протектор), не просто
//! однотонная заливка со случайным дребезгом пикселей — так материал
//! читается на глаз ("это дерево", "это бетон"), а не просто "шум".

/// Детерминированный хэш пары целых координат + seed в [0,1) — тот же
/// принцип, что и в стандартных integer-hash функциях (умножение на большие
/// нечётные константы + xor-сдвиги, чтобы разбросать биты) — НЕ
/// криптографический, просто нужен быстрый воспроизводимый "случайный" шум
/// без внешней зависимости от `rand` (версии `rand` в `Cargo.toml`
/// конфликтуют — 0.8 транзитивно и 0.10 напрямую, см. обсуждение в сессии —
/// проще и надёжнее не тянуть `rand` в новый код вообще).
fn hash01(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374761393)
        .wrapping_add((y as u32).wrapping_mul(668265263))
        .wrapping_add(seed.wrapping_mul(2246822519));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h as f32) / (u32::MAX as f32)
}

/// Билинейно интерполированный "value noise" в одной октаве — сглаженный
/// (smoothstep) между хэшами соседних целых узлов решётки.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let (xi, yi) = (x0 as i32, y0 as i32);
    let (tx, ty) = (x - x0, y - y0);
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);

    let a = hash01(xi, yi, seed);
    let b = hash01(xi + 1, yi, seed);
    let c = hash01(xi, yi + 1, seed);
    let d = hash01(xi + 1, yi + 1, seed);

    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

/// Fractal Brownian Motion — сумма нескольких октав `value_noise` убывающей
/// амплитуды и растущей частоты, нормализованная в [0,1]. Стандартный
/// приём получить "органический" шум (пятна грязи, разводы бетона) вместо
/// однородного зерна одной октавы.
fn fbm(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let mut total = 0.0f32;
    let mut amp = 0.5f32;
    let mut freq = 1.0f32;
    let mut max = 0.0f32;
    for i in 0..octaves {
        total += value_noise(x * freq, y * freq, seed.wrapping_add(i * 97 + 1)) * amp;
        max += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    total / max.max(1e-6)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Один RGBA8-пиксель, вычисляемый через замыкание `f(x, y) -> [r,g,b]`
/// (0..1 каждый канал) — общий каркас "пройтись по width*height и записать
/// 4 байта", который иначе дублировался бы в каждой функции-генераторе
/// ниже. Alpha всегда 255 (все материалы демо непрозрачны).
fn build(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> [f32; 3]) -> (u32, u32, Vec<u8>) {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = f(x, y);
            pixels.push((clamp01(r) * 255.0) as u8);
            pixels.push((clamp01(g) * 255.0) as u8);
            pixels.push((clamp01(b) * 255.0) as u8);
            pixels.push(255);
        }
    }
    (width, height, pixels)
}

/// Земля двора — смесь бурой грязи и пятен травы (2 октавы крупного `fbm`
/// решают, "трава тут или грязь", третья мелкая октава даёт зерно внутри
/// каждого пятна) + редкие тёмные камешки (одиночные пиксели по хэшу).
pub fn dirt_grass(size: u32) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 6.0, y as f32 / size as f32 * 6.0);
        let patch = fbm(fx, fy, 11, 3);
        let grain = fbm(fx * 8.0, fy * 8.0, 23, 2);
        let dirt = [0.32, 0.22, 0.13];
        let grass = [0.20, 0.34, 0.10];
        let t = clamp01((patch - 0.35) * 2.2);
        let mut c = [
            lerp(dirt[0], grass[0], t),
            lerp(dirt[1], grass[1], t),
            lerp(dirt[2], grass[2], t),
        ];
        let shade = 0.85 + grain * 0.3;
        for ch in c.iter_mut() {
            *ch *= shade;
        }
        if hash01(x as i32, y as i32, 77) > 0.985 {
            let k = 0.4;
            c = [c[0] * k, c[1] * k, c[2] * k];
        }
        c
    })
}

/// Гравийная/утоптанная площадка перед гаражом — серо-бурый мелкий шум,
/// без травяных пятен (в отличие от `dirt_grass`) — там, где реально ездят.
pub fn gravel(size: u32) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 10.0, y as f32 / size as f32 * 10.0);
        let n = fbm(fx, fy, 41, 4);
        let base = [0.34, 0.30, 0.26];
        let shade = 0.7 + n * 0.55;
        let mut c = [base[0] * shade, base[1] * shade, base[2] * shade];
        if hash01(x as i32, y as i32, 99) > 0.965 {
            let k = 1.35;
            c = [c[0] * k, c[1] * k, c[2] * k];
        }
        c
    })
}

/// Асфальт подъездной дорожки — тёмно-серый, мельче зерно, чем гравий.
pub fn asphalt(size: u32) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 14.0, y as f32 / size as f32 * 14.0);
        let n = fbm(fx, fy, 51, 3);
        let base = [0.10, 0.10, 0.11];
        let shade = 0.75 + n * 0.5;
        [base[0] * shade, base[1] * shade, base[2] * shade]
    })
}

/// Бетонная стена — крупные "панели" (сетка тёмных швов) + мелкий шум
/// внутри каждой панели, лёгкие потёки (вертикально вытянутый шум).
pub fn concrete_wall(size: u32) -> (u32, u32, Vec<u8>) {
    let panel = 4u32; // швов на текстуру
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 8.0, y as f32 / size as f32 * 8.0);
        let noise = fbm(fx, fy, 13, 3);
        let streak = fbm(fx * 0.3, fy * 1.6, 61, 2) * 0.15;
        let base = 0.55 + noise * 0.3 - streak;

        let px = (x as f32 / size as f32 * panel as f32).fract();
        let py = (y as f32 / size as f32 * panel as f32).fract();
        let edge = px.min(1.0 - px).min(py).min(1.0 - py);
        let joint = if edge < 0.012 { 0.55 } else { 1.0 };

        let v = clamp01(base) * joint;
        [v * 0.62, v * 0.62, v * 0.60]
    })
}

/// Гофрированный металл (гаражные ворота) — вертикальные волны (синус по
/// X формирует свет/тень ребра), лёгкая ржавая крапинка поверх базового
/// цвета `base_rgb` (позволяет сделать и серые, и рыжие ворота).
pub fn corrugated_metal(size: u32, base_rgb: [f32; 3]) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let fx = x as f32 / size as f32 * 24.0;
        let wave = (fx * std::f32::consts::TAU).sin();
        let ridge = 0.65 + wave * 0.35;

        let (nx, ny) = (x as f32 / size as f32 * 5.0, y as f32 / size as f32 * 5.0);
        let rust_mask = fbm(nx, ny, 31, 3);
        let rust_color = [0.35, 0.16, 0.08];
        let rust_t = clamp01((rust_mask - 0.62) * 3.0);

        let c = [
            lerp(base_rgb[0] * ridge, rust_color[0], rust_t),
            lerp(base_rgb[1] * ridge, rust_color[1], rust_t),
            lerp(base_rgb[2] * ridge, rust_color[2], rust_t),
        ];
        c
    })
}

/// Доски (забор/ящики) — горизонтальные полосы досок с тёмным швом между
/// ними + продольные волокна древесины (растянутый по X шум).
pub fn wood_plank(size: u32, base_rgb: [f32; 3]) -> (u32, u32, Vec<u8>) {
    let planks = 6u32;
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 18.0, y as f32 / size as f32 * 2.0);
        let grain = fbm(fx, fy, 71, 3);

        let py = (y as f32 / size as f32 * planks as f32).fract();
        let seam = if py < 0.03 || py > 0.97 { 0.45 } else { 1.0 };

        let shade = 0.7 + grain * 0.5;
        let c = [base_rgb[0] * shade * seam, base_rgb[1] * shade * seam, base_rgb[2] * shade * seam];
        c
    })
}

/// Кирпич — прямоугольная кладка со смещением рядов (классический
/// "brick offset"), тёмный шов раствора, лёгкий разброс оттенка кирпича.
pub fn brick(size: u32, base_rgb: [f32; 3]) -> (u32, u32, Vec<u8>) {
    let rows = 8u32;
    let cols = 4u32;
    build(size, size, |x, y| {
        let u = x as f32 / size as f32 * cols as f32;
        let v = y as f32 / size as f32 * rows as f32;
        let row = v.floor() as i32;
        let offset = if row % 2 == 0 { 0.0 } else { 0.5 };
        let uu = (u + offset).fract();
        let vv = v.fract();

        let mortar = uu < 0.03 || uu > 0.97 || vv < 0.05 || vv > 0.95;
        let shade_variation = hash01(row, (u + offset).floor() as i32, 5) * 0.25 + 0.85;

        if mortar {
            [0.62, 0.60, 0.56]
        } else {
            [base_rgb[0] * shade_variation, base_rgb[1] * shade_variation, base_rgb[2] * shade_variation]
        }
    })
}

/// Автомобильная краска — почти однородный цвет `base_rgb` с очень мелким
/// "металлик" шумом (лёгкая вариация яркости) — тайлится редко (сам кузов
/// обычно меньше одного повтора), поэтому узор минимальный, задача этой
/// текстуры — не "рисунок", а не-плоская, чуть шумная поверхность вместо
/// идеально ровной заливки (которая на PBR-освещении выглядит "пластиково").
pub fn car_paint(size: u32, base_rgb: [f32; 3]) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 30.0, y as f32 / size as f32 * 30.0);
        let fleck = fbm(fx, fy, 91, 2);
        let shade = 0.92 + fleck * 0.16;
        [base_rgb[0] * shade, base_rgb[1] * shade, base_rgb[2] * shade]
    })
}

/// Резина колеса — почти чёрная, с шумом + грубым "протекторным" узором
/// (диагональные полосы) для намёка на рисунок протектора без полноценной
/// геометрии.
pub fn tire_rubber(size: u32) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 16.0, y as f32 / size as f32 * 16.0);
        let noise = fbm(fx, fy, 111, 3);
        let tread = ((x as f32 / size as f32 * 40.0 + y as f32 / size as f32 * 10.0).sin() * 0.5 + 0.5) > 0.6;
        let base = 0.10 + noise * 0.06;
        let v = if tread { base * 0.55 } else { base };
        [v, v, v * 1.02]
    })
}

/// Хром/металлическая отделка (бампер, диски) — светло-серый с мелким
/// шумом, чуть вытянутым по одной оси, как заводская шлифовка.
pub fn chrome(size: u32) -> (u32, u32, Vec<u8>) {
    build(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32 * 40.0, y as f32 / size as f32 * 4.0);
        let n = fbm(fx, fy, 131, 2);
        let v = 0.55 + n * 0.35;
        [v, v, v * 1.02]
    })
}
