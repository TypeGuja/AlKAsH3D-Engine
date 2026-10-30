//! ДОБАВЛЕНО (звук): все звуки подъезда и двора синтезируются кодом при
//! запуске — без записей и чужих файлов. Шаги по снегу (хруст — россыпь
//! коротких шумовых «зёрен») и по бетону (щелчок каблука, шарканье и гулкое
//! эхо лестничной клетки, запечённое ревербератором Шрёдера), скрип и хлопок
//! входной двери с доводчиком, писк домофона и щелчок магнитного замка,
//! гудение ламп накаливания (100 Гц — удвоенная частота сети, с
//! гармониками), щелчок выключателя, ветер на улице, пакет в контейнер.
//!
//! Моно 44.1 кГц. Шаги и интерфейсные звуки — 2D (это сам игрок), остальное
//! — в точке мира (движок сам ставит слушателя в камеру).

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::{AttenuationParams, SoundHandle};

const SR: u32 = 44100;
const VARIANTS: u32 = 4;

/// Детерминированный генератор шума (xorshift): одни и те же звуки при каждом запуске.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    fn uni(&mut self) -> f32 {
        self.next() * 0.5 + 0.5
    }
}

fn secs(t: f32) -> usize {
    (t * SR as f32) as usize
}

/// Однополюсный фильтр нижних частот, частота среза `fc`.
fn lowpass(x: &mut [f32], fc: f32) {
    let a = 1.0 - (-2.0 * std::f32::consts::PI * fc / SR as f32).exp();
    let mut y = 0.0;
    for v in x.iter_mut() {
        y += a * (*v - y);
        *v = y;
    }
}

fn highpass(x: &mut [f32], fc: f32) {
    let mut lp = x.to_vec();
    lowpass(&mut lp, fc);
    for (v, l) in x.iter_mut().zip(lp) {
        *v -= l;
    }
}

/// Ревербератор Шрёдера: 4 гребенчатых фильтра параллельно и 2 фазовращателя
/// последовательно. `rt` — время спада на 60 дБ (бетонная клетка — ~1.3 с),
/// `wet` — доля эха.
fn reverb(x: &[f32], rt: f32, wet: f32, tail: f32) -> Vec<f32> {
    let n = x.len() + secs(tail);
    let mut out = vec![0.0f32; n];
    let combs = [0.0297f32, 0.0371, 0.0411, 0.0437];
    for d in combs {
        let len = secs(d);
        let g = 10f32.powf(-3.0 * d / rt);
        let mut buf = vec![0.0f32; len];
        let mut lp = 0.0;
        for i in 0..n {
            let input = if i < x.len() { x[i] } else { 0.0 };
            let k = i % len;
            let y = buf[k];
            // бетон гасит высокие в каждом отражении
            lp += 0.35 * (y - lp);
            buf[k] = input + lp * g;
            out[i] += y * 0.25;
        }
    }
    for (d, g) in [(0.005f32, 0.7f32), (0.0017, 0.7)] {
        let len = secs(d).max(1);
        let mut buf = vec![0.0f32; len];
        // буфер — очередь задержки: в начале самый старый отсчёт
        for v in out.iter_mut() {
            let b = buf[0];
            let y = -g * *v + b;
            buf.rotate_left(1);
            let last = buf.len() - 1;
            buf[last] = *v + g * y;
            *v = y;
        }
    }
    let mut mix = vec![0.0f32; n];
    for i in 0..n {
        let dry = if i < x.len() { x[i] } else { 0.0 };
        mix[i] = dry * (1.0 - wet) + out[i] * wet;
    }
    mix
}

fn to_pcm(x: &[f32], peak: f32) -> Vec<i16> {
    let m = x.iter().fold(1e-6f32, |a, v| a.max(v.abs()));
    let k = peak / m;
    x.iter().map(|v| ((v * k).clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

/// Хруст снега: сжатие корки — десятки коротких щелчков-«зёрен»; сначала
/// пятка (гуще), потом носок; под ними глухое уплотнение.
fn step_snow(seed: u32) -> Vec<f32> {
    let mut r = Rng(seed * 7919 + 1);
    let mut x = vec![0.0f32; secs(0.32)];
    let grains = 26 + (r.uni() * 14.0) as usize;
    for g in 0..grains {
        let t = if g < grains * 2 / 3 { r.uni() * 0.09 } else { 0.11 + r.uni() * 0.1 };
        let len = secs(0.002 + r.uni() * 0.005);
        let amp = 0.3 + r.uni() * 0.7;
        let s0 = secs(t);
        for i in 0..len {
            if s0 + i < x.len() {
                x[s0 + i] += r.next() * amp * (-(i as f32) / (len as f32 * 0.35)).exp();
            }
        }
    }
    // корка хрустит в 1–4 кГц; выше — только призвук
    highpass(&mut x, 900.0);
    lowpass(&mut x, 4500.0);
    lowpass(&mut x, 4500.0);
    let mut thump: Vec<f32> = (0..secs(0.12)).map(|i| r.next() * (-(i as f32) / secs(0.03) as f32).exp()).collect();
    lowpass(&mut thump, 180.0);
    for (i, v) in thump.into_iter().enumerate() {
        x[i] += v * 2.5;
    }
    x
}

/// Шаг по бетону в клетке: щелчок каблука (резонанс подошвы), шарканье, эхо.
fn step_concrete(seed: u32, rt: f32, wet: f32) -> Vec<f32> {
    let mut r = Rng(seed * 104729 + 3);
    let mut x = vec![0.0f32; secs(0.25)];
    let f = 170.0 + r.uni() * 80.0;
    for i in 0..secs(0.06) {
        let t = i as f32 / SR as f32;
        x[i] += (2.0 * std::f32::consts::PI * f * t).sin() * (-t / 0.012).exp() * 0.8;
    }
    let mut click: Vec<f32> = (0..secs(0.006)).map(|i| r.next() * (-(i as f32) / secs(0.0012) as f32).exp()).collect();
    highpass(&mut click, 2500.0);
    for (i, v) in click.into_iter().enumerate() {
        x[i] += v * 2.4;
    }
    let s0 = secs(0.07 + r.uni() * 0.03);
    let mut scuff: Vec<f32> = (0..secs(0.07)).map(|i| r.next() * (std::f32::consts::PI * i as f32 / secs(0.07) as f32).sin()).collect();
    highpass(&mut scuff, 1200.0);
    lowpass(&mut scuff, 5000.0);
    for (i, v) in scuff.into_iter().enumerate() {
        x[s0 + i] += v * 0.12;
    }
    reverb(&x, rt, wet, rt)
}

/// Скрип петель при открывании: прилипание–проскальзывание — серия
/// импульсов с дрожащим периодом через резонансы полотна, тон ползёт вверх.
fn door_creak() -> Vec<f32> {
    let mut r = Rng(42);
    let n = secs(0.85);
    let mut x = vec![0.0f32; n];
    let mut t = 0.03f32;
    while t < 0.8 {
        let s = t / 0.8;
        let f = 180.0 + 260.0 * s + 40.0 * (s * 9.0).sin();
        let amp = (s * std::f32::consts::PI).sin().powf(0.6) * (0.7 + 0.3 * r.uni());
        let i0 = secs(t);
        for k in 0..secs(0.004) {
            if i0 + k < n {
                x[i0 + k] += amp * (-(k as f32) / secs(0.0009) as f32).exp() * if k % 2 == 0 { 1.0 } else { -0.6 };
            }
        }
        t += (1.0 / f) * (0.85 + 0.3 * r.uni());
    }
    // дерево полотна: две полосы резонансов (300–1100 и 1800–3200 Гц);
    // ниже 250 Гц скрипа нет — импульсы несимметричны и давали гул и смещение
    let mut a = x.clone();
    let mut b = x.clone();
    highpass(&mut a, 300.0);
    lowpass(&mut a, 1100.0);
    highpass(&mut b, 1800.0);
    lowpass(&mut b, 3200.0);
    let mut y: Vec<f32> = a.iter().zip(&b).map(|(p, q)| p * 1.0 + q * 0.6).collect();
    highpass(&mut y, 250.0);
    // щелчок защёлки в начале — металл, 1–3 кГц
    let mut latch: Vec<f32> = (0..secs(0.012)).map(|i| r.next() * (-(i as f32) / secs(0.0015) as f32).exp()).collect();
    highpass(&mut latch, 800.0);
    lowpass(&mut latch, 3000.0);
    for (i, v) in latch.into_iter().enumerate() {
        y[i] += v * 0.6;
    }
    // однополюсные фильтры пологие (6 дБ/окт) — резкие импульсы пролезали
    // шипением выше 4 кГц; двойной срез оставляет сам скрип
    lowpass(&mut y, 3500.0);
    lowpass(&mut y, 3500.0);
    reverb(&y, 0.9, 0.25, 0.6)
}

/// Хлопок двери доводчиком: удар полотна о коробку, дребезг железа, эхо.
fn door_slam() -> Vec<f32> {
    let mut r = Rng(7);
    let n = secs(0.6);
    let mut x = vec![0.0f32; n];
    for i in 0..n {
        let t = i as f32 / SR as f32;
        x[i] += (2.0 * std::f32::consts::PI * 72.0 * t).sin() * (-t / 0.09).exp() * 1.2;
        x[i] += (2.0 * std::f32::consts::PI * 131.0 * t).sin() * (-t / 0.05).exp() * 0.5;
        // дребезг железной полосы и ручки — негармонические частоты
        for (f, a, d) in [(870.0f32, 0.12f32, 0.18f32), (1390.0, 0.08, 0.12), (2240.0, 0.05, 0.09), (3170.0, 0.03, 0.06)] {
            x[i] += (2.0 * std::f32::consts::PI * f * t).sin() * a * (-t / d).exp() * (1.0 + 0.5 * (t * 60.0).sin());
        }
    }
    let mut noise: Vec<f32> = (0..secs(0.05)).map(|i| r.next() * (-(i as f32) / secs(0.008) as f32).exp()).collect();
    lowpass(&mut noise, 1500.0);
    for (i, v) in noise.into_iter().enumerate() {
        x[i] += v * 1.5;
    }
    reverb(&x, 1.4, 0.35, 1.4)
}

/// Домофон: три писка и щелчок магнитного замка с гулом катушки.
fn intercom() -> Vec<f32> {
    let n = secs(0.95);
    let mut x = vec![0.0f32; n];
    for k in 0..3 {
        let s0 = secs(0.02 + k as f32 * 0.15);
        for i in 0..secs(0.09) {
            let t = i as f32 / SR as f32;
            let env = (t / 0.004).min(1.0) * ((0.09 - t) / 0.006).clamp(0.0, 1.0);
            let ph = 2.0 * std::f32::consts::PI * 1046.0 * t;
            x[s0 + i] += (ph.sin() + 0.3 * (3.0 * ph).sin()) * env * 0.5;
        }
    }
    let s0 = secs(0.5);
    for i in 0..secs(0.4) {
        let t = i as f32 / SR as f32;
        x[s0 + i] += (2.0 * std::f32::consts::PI * 50.0 * t).sin().signum() * 0.06 * (-t / 0.25).exp();
        if i < secs(0.01) {
            x[s0 + i] += ((i * 2654435761usize) % 1000) as f32 / 500.0 - 1.0;
        }
    }
    x
}

/// Гудение лампы накаливания: нить и патрон дрожат с удвоенной частотой
/// сети (100 Гц) и гармониками. Ровно 200 периодов за 2 с — петля без шва.
fn lamp_hum() -> Vec<f32> {
    let mut r = Rng(11);
    (0..secs(2.0))
        .map(|i| {
            let t = i as f32 / SR as f32;
            let w = 2.0 * std::f32::consts::PI * 100.0 * t;
            w.sin() * 0.6 + (2.0 * w).sin() * 0.25 + (3.0 * w).sin() * 0.12 + (5.0 * w).sin() * 0.05 + r.next() * 0.01
        })
        .collect()
}

fn switch_click() -> Vec<f32> {
    let mut r = Rng(5);
    let mut x = vec![0.0f32; secs(0.12)];
    for s in [0.0f32, 0.028] {
        let s0 = secs(s);
        for i in 0..secs(0.006) {
            x[s0 + i] += r.next() * (-(i as f32) / secs(0.001) as f32).exp();
        }
    }
    highpass(&mut x, 1500.0);
    lowpass(&mut x, 6000.0);
    // корпус клавиши — короткий «пластиковый» тон
    for s in [0.0f32, 0.028] {
        let s0 = secs(s);
        for i in 0..secs(0.02) {
            let t = i as f32 / SR as f32;
            x[s0 + i] += (2.0 * std::f32::consts::PI * 2400.0 * t).sin() * 0.5 * (-t / 0.004).exp();
        }
    }
    reverb(&x, 1.0, 0.25, 0.5)
}

/// Ветер: коричневый шум с порывами; конец переходит в начало (петля без щелчка).
fn wind() -> Vec<f32> {
    let mut r = Rng(99);
    let len = secs(8.0);
    let mut x: Vec<f32> = (0..len + secs(1.0)).map(|_| r.next()).collect();
    lowpass(&mut x, 380.0);
    lowpass(&mut x, 380.0);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR as f32;
        // порывы: медленная огибающая с периодом, кратным длине петли
        let g = 0.55 + 0.3 * (2.0 * std::f32::consts::PI * t / 8.0).sin() + 0.15 * (2.0 * std::f32::consts::PI * t * 3.0 / 8.0).sin();
        *v *= g;
    }
    let fade = secs(1.0);
    let mut out = x[..len].to_vec();
    for i in 0..fade {
        let a = i as f32 / fade as f32;
        out[i] = out[i] * a + x[len + i] * (1.0 - a);
    }
    out
}

/// Пакет в бак: глухой удар и шелест плёнки, гул пустого железа.
fn trash_drop() -> Vec<f32> {
    let mut r = Rng(13);
    let n = secs(0.9);
    let mut x = vec![0.0f32; n];
    let mut thud: Vec<f32> = (0..secs(0.15)).map(|i| r.next() * (-(i as f32) / secs(0.03) as f32).exp()).collect();
    lowpass(&mut thud, 300.0);
    for (i, v) in thud.into_iter().enumerate() {
        x[i] += v * 3.0;
    }
    let mut rustle: Vec<f32> = (0..secs(0.3)).map(|i| r.next() * (std::f32::consts::PI * i as f32 / secs(0.3) as f32).sin()).collect();
    highpass(&mut rustle, 2500.0);
    for (i, v) in rustle.into_iter().enumerate() {
        x[i] += v * 0.25;
    }
    for i in 0..n {
        let t = i as f32 / SR as f32;
        x[i] += (2.0 * std::f32::consts::PI * 210.0 * t).sin() * 0.25 * (-t / 0.25).exp();
    }
    x
}

/// Собака лает через пару дворов: «гав» — тон 450–600 Гц с гармониками и
/// шумом дыхания, 2–3 раза; далеко — верх срезан, эхо от домов.
fn dog_bark(seed: u32) -> Vec<f32> {
    let mut r = Rng(seed * 31 + 17);
    let n = secs(2.4);
    let mut x = vec![0.0f32; n];
    let barks = 2 + (r.uni() * 2.0) as usize;
    let mut t0 = 0.05;
    for _ in 0..barks {
        let len = 0.12 + 0.06 * r.uni();
        let f0 = 450.0 + 150.0 * r.uni();
        let s0 = secs(t0);
        let mut ph = 0.0f32;
        for i in 0..secs(len) {
            let t = i as f32 / SR as f32;
            let k = t / len;
            // тон срывается вверх и падает, огибающая — резкая атака
            let f = f0 * (1.0 + 0.35 * (std::f32::consts::PI * k).sin());
            ph += 2.0 * std::f32::consts::PI * f / SR as f32;
            let env = (k / 0.08).min(1.0) * (1.0 - k).powf(1.5);
            let v = ph.sin() + 0.5 * (2.0 * ph).sin() + 0.3 * (3.0 * ph).sin() + 0.4 * r.next();
            if s0 + i < n {
                x[s0 + i] += v * env;
            }
        }
        t0 += len + 0.18 + 0.15 * r.uni();
    }
    lowpass(&mut x, 1800.0);
    highpass(&mut x, 200.0);
    reverb(&x, 1.6, 0.45, 0.2)
}

/// Машина по улице за гаражами: шина по снегу и мотор — нарастает и уходит.
fn car_pass() -> Vec<f32> {
    let mut r = Rng(77);
    let n = secs(7.0);
    let mut x: Vec<f32> = (0..n).map(|_| r.next()).collect();
    lowpass(&mut x, 700.0);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR as f32;
        let env = (-((t - 3.5) / 1.6).powi(2)).exp();
        let motor = (2.0 * std::f32::consts::PI * (38.0 + 6.0 * (t - 3.5).tanh()) * t).sin() * 0.4;
        *v = (*v + motor) * env;
    }
    x
}

#[derive(Clone, Copy, PartialEq)]
pub enum Surface {
    /// Снег во дворе.
    Snow,
    /// Бетон и плитка на улице (крыльцо, дорожка) — короткое эхо от фасада.
    Street,
    /// Бетон в клетке — гулкое эхо.
    Stairwell,
}

pub struct Sfx {
    rng: Rng,
    /// Общая громкость (PODEZD_VOLUME, по умолчанию 1).
    master: f32,
    /// Гудение ламп: индекс лампы и её петля.
    hums: Vec<(usize, SoundHandle)>,
    wind: Option<SoundHandle>,
}

impl Sfx {
    /// Синтезировать клипы и отдать их звуковому движку (без звука — ничего).
    pub fn new(engine: &mut AlkashEngine) -> Sfx {
        let t0 = std::time::Instant::now();
        if let Some(a) = engine.audio.as_mut() {
            fn add(a: &mut alkash3d_rs::AudioEngine, name: String, x: Vec<f32>, peak: f32) {
                a.add_clip_pcm16(&name, &to_pcm(&x, peak), SR, 1);
            }
            for v in 0..VARIANTS {
                add(a, format!("step_snow{v}"), step_snow(v), 0.9);
                add(a, format!("step_stair{v}"), step_concrete(v, 1.3, 0.38), 0.9);
                add(a, format!("step_street{v}"), step_concrete(v + 10, 0.5, 0.15), 0.9);
            }
            add(a, "door_creak".into(), door_creak(), 0.8);
            add(a, "door_slam".into(), door_slam(), 0.95);
            add(a, "intercom".into(), intercom(), 0.6);
            add(a, "lamp_hum".into(), lamp_hum(), 0.5);
            add(a, "switch".into(), switch_click(), 0.7);
            add(a, "wind".into(), wind(), 0.7);
            add(a, "trash".into(), trash_drop(), 0.85);
            add(a, "dog0".into(), dog_bark(0), 0.7);
            add(a, "dog1".into(), dog_bark(1), 0.7);
            add(a, "car".into(), car_pass(), 0.7);
            println!("[ПОДЪЕЗД] звук синтезирован: {:.2} с", t0.elapsed().as_secs_f32());
        }
        let master = std::env::var("PODEZD_VOLUME").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        Sfx { rng: Rng(12345), master, hums: Vec::new(), wind: None }
    }

    pub fn step(&mut self, engine: &mut AlkashEngine, surface: Surface, run: bool) {
        let v = (self.rng.uni() * VARIANTS as f32) as u32 % VARIANTS;
        let name = match surface {
            Surface::Snow => format!("step_snow{v}"),
            Surface::Street => format!("step_street{v}"),
            Surface::Stairwell => format!("step_stair{v}"),
        };
        let vol = (if run { 0.55 } else { 0.38 }) * (0.85 + 0.3 * self.rng.uni());
        let pitch = 0.92 + 0.16 * self.rng.uni();
        if let Some(a) = engine.audio.as_mut() {
            if let Ok(h) = a.play_sound_2d(&name, vol * self.master, false) {
                a.set_sound_pitch(h, pitch);
            }
        }
    }

    /// Звук в точке мира.
    pub fn at(&mut self, engine: &mut AlkashEngine, name: &str, pos: Vec3, vol: f32, max_dist: f32) {
        if let Some(a) = engine.audio.as_mut() {
            let att = AttenuationParams { min_distance: 0.6, max_distance: max_dist, model: 1, doppler_factor: 0.0 };
            let _ = a.play_sound_3d(name, pos, vol * self.master, false, att);
        }
    }

    /// Гудение лампы `idx` в точке `pos`, громкость — по накалу (0 — тишина).
    pub fn hum(&mut self, engine: &mut AlkashEngine, idx: usize, pos: Vec3, level: f32) {
        let Some(a) = engine.audio.as_mut() else { return };
        let h = match self.hums.iter().find(|(i, _)| *i == idx) {
            Some(&(_, h)) => h,
            None => {
                let att = AttenuationParams { min_distance: 0.4, max_distance: 7.0, model: 1, doppler_factor: 0.0 };
                let Ok(h) = a.play_sound_3d("lamp_hum", pos, 0.0, true, att) else { return };
                self.hums.push((idx, h));
                h
            }
        };
        a.set_sound_volume(h, 0.09 * level * self.master);
    }

    /// Ночной район издалека: собака, машина — изредка, случайно, если
    /// слышно улицу (`outside` 0..1: снаружи 1, у открытой двери меньше).
    pub fn ambience(&mut self, engine: &mut AlkashEngine, dt: f32, outside: f32) {
        if outside <= 0.0 {
            return;
        }
        // в среднем раз в ~25 с
        if self.rng.uni() > dt / 25.0 {
            return;
        }
        let pick = self.rng.uni();
        let name = if pick < 0.35 { "dog0" } else if pick < 0.7 { "dog1" } else { "car" };
        let vol = if name == "car" { 0.18 } else { 0.12 } * outside * (0.6 + 0.4 * self.rng.uni());
        let pitch = 0.9 + 0.2 * self.rng.uni();
        if let Some(a) = engine.audio.as_mut() {
            if let Ok(h) = a.play_sound_2d(name, vol * self.master, false) {
                a.set_sound_pitch(h, pitch);
            }
        }
    }

    /// Ветер: снаружи слышен в полную силу, в клетке — глухо, сильнее в открытую дверь.
    pub fn wind(&mut self, engine: &mut AlkashEngine, level: f32) {
        let Some(a) = engine.audio.as_mut() else { return };
        if self.wind.is_none() {
            self.wind = a.play_sound_2d("wind", 0.0, true).ok();
        }
        if let Some(h) = self.wind {
            a.set_sound_volume(h, level * self.master);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &std::path::Path, x: &[i16]) {
        let mut b = Vec::new();
        let data_len = (x.len() * 2) as u32;
        b.extend(b"RIFF");
        b.extend((36 + data_len).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(SR.to_le_bytes());
        b.extend((SR * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(data_len.to_le_bytes());
        for v in x {
            b.extend(v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// Все клипы синтезируются, конечные, не тишина; PODEZD_SOUND_DUMP=папка —
    /// записать их WAV-файлами (послушать без игры).
    #[test]
    fn clips_synthesize() {
        let clips: Vec<(&str, Vec<f32>)> = vec![
            ("step_snow", step_snow(0)),
            ("step_stair", step_concrete(0, 1.3, 0.38)),
            ("step_street", step_concrete(10, 0.5, 0.15)),
            ("door_creak", door_creak()),
            ("door_slam", door_slam()),
            ("intercom", intercom()),
            ("lamp_hum", lamp_hum()),
            ("switch", switch_click()),
            ("wind", wind()),
            ("trash", trash_drop()),
            ("dog", dog_bark(0)),
            ("car", car_pass()),
        ];
        let dump = std::env::var("PODEZD_SOUND_DUMP").ok();
        for (name, x) in clips {
            assert!(!x.is_empty() && x.iter().all(|v| v.is_finite()), "{name}: пусто или NaN");
            let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
            assert!(peak > 1e-4, "{name}: тишина");
            if let Some(d) = &dump {
                wav(&std::path::Path::new(d).join(format!("{name}.wav")), &to_pcm(&x, 0.9));
            }
        }
    }
}
