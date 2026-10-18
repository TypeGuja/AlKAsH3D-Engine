//! Кровь: брызги-капли (летят по баллистике), пятна там, где капля упала
//! на стол или пол (растекаются), и картинки ран/подтёков для пони
//! (`official::Wound`). Все текстуры — процедурные, свои.
//!
//! Капли и раны рисуются без освещения (как пони) с яркостью от света
//! вокруг; пятна на столе и полу — обычные освещённые декали.

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::Vec3;

use crate::collide::Collider;
use crate::items::GRAVITY;

/// Сколько пятен держим (старые исчезают).
const MAX_STAINS: usize = 400;
/// Капли живут не дольше (с).
const DROP_LIFE: f32 = 4.0;

struct Drop {
    pos: Vec3,
    vel: Vec3,
    size: f32,
    age: f32,
}

struct Stain {
    pos: Vec3,
    angle: f32,
    size: f32,
    age: f32,
    mesh: usize,
}

pub struct Blood {
    drop_mesh: usize,
    stain_meshes: [usize; 2],
    /// Для пони: пятно раны, царапины, синяк, рваная рана (дробь в упор); подтёк.
    pub wound_meshes: [usize; 4],
    pub drip_mesh: usize,
    drops: Vec<Drop>,
    stains: Vec<Stain>,
    rng: u32,
}

/// Псевдослучайное 0..1 по целым координатам (для текстур).
fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Клякса: неровное пятно с брызгами вокруг (альфа — маска). `rgb` — цвет.
fn splat_texture(n: u32, seed: u32, rgb: [f32; 3], blobs: usize) -> Vec<u8> {
    let c = n as f32 * 0.5;
    // Центр — несколько перекрывающихся кругов, вокруг — мелкие капли.
    let mut circles: Vec<(f32, f32, f32)> = vec![(c, c, n as f32 * 0.24)];
    for k in 0..blobs {
        let a = hash(k as i32, 1, seed) * std::f32::consts::TAU;
        let d = n as f32 * (0.12 + 0.3 * hash(k as i32, 2, seed));
        let r = n as f32 * (0.02 + 0.09 * hash(k as i32, 3, seed) * (1.0 - d / (n as f32 * 0.45)).max(0.15));
        circles.push((c + a.cos() * d, c + a.sin() * d, r));
    }
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let inside = circles.iter().map(|&(cx, cy, r)| r - ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt()).fold(f32::MIN, f32::max);
            let a = (inside * 0.8 + 0.5).clamp(0.0, 1.0);
            // Темнее к краю (засыхает), чуть пятнисто.
            let k = 0.75 + 0.25 * hash(x as i32 / 3, y as i32 / 3, seed + 7) - (1.0 - (inside / (n as f32 * 0.2)).clamp(0.0, 1.0)) * 0.25;
            let ch = |v: f32| (v * k * 255.0).clamp(0.0, 255.0) as u8;
            rgba.extend([ch(rgb[0]), ch(rgb[1]), ch(rgb[2]), (a * 255.0) as u8]);
        }
    }
    rgba
}

/// Рваная рана: клякса с почти чёрной серединой (глубокая) и неровным краем.
fn gore_texture(n: u32, seed: u32, rgb: [f32; 3]) -> Vec<u8> {
    let mut rgba = splat_texture(n, seed, rgb, 18);
    let c = n as f32 * 0.5;
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt() / (n as f32 * 0.28);
            let ragged = 0.15 * hash(x as i32 / 2, y as i32 / 2, seed + 5);
            let k = (0.18 + 0.82 * (d + ragged).clamp(0.0, 1.0)).min(1.0);
            let i = ((y * n + x) * 4) as usize;
            for ch in 0..3 {
                rgba[i + ch] = (rgba[i + ch] as f32 * k) as u8;
            }
        }
    }
    rgba
}

/// Царапины: несколько тонких косых полос.
fn scratch_texture(n: u32, seed: u32, rgb: [f32; 3]) -> Vec<u8> {
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    for k in 0..4 {
        let y0 = n as f32 * (0.2 + 0.6 * hash(k, 1, seed));
        let slope = -0.5 + hash(k, 2, seed) * 0.4;
        let (x0, x1) = (n as f32 * (0.05 + 0.2 * hash(k, 3, seed)), n as f32 * (0.7 + 0.25 * hash(k, 4, seed)));
        for x in 0..n {
            let fx = x as f32;
            if fx < x0 || fx > x1 {
                continue;
            }
            let t = (fx - x0) / (x1 - x0);
            let w = 1.6 * (t * std::f32::consts::PI).sin() + 0.4;
            let cy = y0 + (fx - x0) * slope;
            for y in 0..n {
                let d = (y as f32 - cy).abs();
                if d < w {
                    let i = ((y * n + x) * 4) as usize;
                    let a = ((w - d) / w).clamp(0.0, 1.0);
                    let k = 0.7 + 0.3 * hash(x as i32, y as i32, seed);
                    rgba[i] = (rgb[0] * k * 255.0) as u8;
                    rgba[i + 1] = (rgb[1] * k * 255.0) as u8;
                    rgba[i + 2] = (rgb[2] * k * 255.0) as u8;
                    rgba[i + 3] = rgba[i + 3].max((a * 255.0) as u8);
                }
            }
        }
    }
    rgba
}

/// Синяк: мягкое неровное пятно (края — полупрозрачные, отсекаются по альфе,
/// поэтому синяк — пятнистый, а не ровный круг).
fn bruise_texture(n: u32, seed: u32, rgb: [f32; 3]) -> Vec<u8> {
    let c = n as f32 * 0.5;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt() / c;
            let noise = hash(x as i32 / 4, y as i32 / 4, seed) * 0.45 + hash(x as i32 / 2, y as i32 / 2, seed + 1) * 0.2;
            let a = (1.0 - d * 1.2 + noise - 0.15).clamp(0.0, 1.0);
            let k = 0.8 + 0.2 * noise;
            rgba.extend([(rgb[0] * k * 255.0) as u8, (rgb[1] * k * 255.0) as u8, (rgb[2] * k * 255.0) as u8, (a * 255.0) as u8]);
        }
    }
    rgba
}

/// Подтёк: вертикальная полоса сверху вниз, сужается, внизу — капля.
fn drip_texture(w: u32, h: u32, rgb: [f32; 3]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    let c = w as f32 * 0.5;
    for y in 0..h {
        let t = y as f32 / h as f32;
        let half = w as f32 * (0.32 - 0.14 * t);
        let bulb = if t > 0.86 { w as f32 * 0.4 * (1.0 - ((t - 0.93) / 0.07).powi(2)).max(0.0).sqrt() } else { 0.0 };
        let r = half.max(bulb);
        for x in 0..w {
            let d = (x as f32 + 0.5 - c).abs();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            rgba.extend([(rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8, (a * 255.0) as u8]);
        }
    }
    rgba
}

/// Квад 1×1: в плоскости XY с центром в начале (`top` — верх в начале,
/// вниз на 1) или в плоскости XZ (лежит, нормаль вверх).
fn quad(engine: &mut AlkashEngine, tex: u32, lying: bool, top: bool, unlit: bool) -> Option<usize> {
    let v = |x: f32, y: f32, u: f32, w: f32| {
        if lying {
            Vertex::with_normal_uv(x, 0.0, y, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, u, w)
        } else {
            Vertex::with_normal_uv(x, y, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, u, w)
        }
    };
    let (y0, y1) = if top { (-1.0, 0.0) } else { (-0.5, 0.5) };
    let verts = [v(-0.5, y0, 0.0, 1.0), v(0.5, y0, 1.0, 1.0), v(0.5, y1, 1.0, 0.0), v(-0.5, y1, 0.0, 0.0)];
    // Обход: лицом к +Z (стоячий) или вверх (лежачий).
    let idx: &[u32] = if lying { &[0, 2, 1, 0, 3, 2] } else { &[0, 1, 2, 0, 2, 3] };
    let mut mesh = Mesh::from_vertices_and_indices(&verts, idx).ok()?;
    mesh.albedo_srv_index = Some(tex);
    mesh.alpha_cutoff = 0.5;
    mesh.unlit = unlit;
    Some(engine.add_mesh(mesh))
}

impl Blood {
    pub fn new(engine: &mut AlkashEngine) -> Option<Blood> {
        // Кровь: тёмно-красная. Для освещённых пятен — альбедо; для того, что
        // без освещения (капли, раны на пони), — сразу итоговый линейный цвет.
        let wet = [0.32, 0.015, 0.012];
        let lit = [0.30, 0.02, 0.02];
        let mut tex = |rgba: Vec<u8>, n: u32, m: u32| engine.create_texture_rgba(n, m, &rgba);
        let t_drop = tex(splat_texture(32, 3, lit, 0), 32, 32)?;
        let t_stain0 = tex(splat_texture(128, 11, wet, 14), 128, 128)?;
        let t_stain1 = tex(splat_texture(128, 29, wet, 22), 128, 128)?;
        let t_wound = tex(splat_texture(64, 41, lit, 9), 64, 64)?;
        let t_scratch = tex(scratch_texture(64, 53, [0.36, 0.03, 0.03]), 64, 64)?;
        let t_bruise = tex(bruise_texture(64, 67, [0.16, 0.06, 0.14]), 64, 64)?;
        let t_drip = tex(drip_texture(16, 128, lit), 16, 128)?;
        let t_gore = tex(gore_texture(128, 83, [0.36, 0.02, 0.02]), 128, 128)?;
        Some(Blood {
            drop_mesh: quad(engine, t_drop, false, false, true)?,
            stain_meshes: [quad(engine, t_stain0, true, false, false)?, quad(engine, t_stain1, true, false, false)?],
            wound_meshes: [
                quad(engine, t_wound, false, false, true)?,
                quad(engine, t_scratch, false, false, true)?,
                quad(engine, t_bruise, false, false, true)?,
                quad(engine, t_gore, false, false, true)?,
            ],
            drip_mesh: quad(engine, t_drip, false, true, true)?,
            drops: Vec::new(),
            stains: Vec::new(),
            rng: 0x6d2b_79f5,
        })
    }

    pub fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn clear(&mut self) {
        self.drops.clear();
        self.stains.clear();
    }

    /// Брызги из `from` в сторону `dir` (конус), `n` капель.
    pub fn spray(&mut self, from: Vec3, dir: Vec3, n: usize) {
        let dir = dir.normalize_or(Vec3::Y);
        let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = side.cross(dir);
        for _ in 0..n {
            let (a, r) = (self.rand() * std::f32::consts::TAU, self.rand().sqrt() * 0.45);
            let d = (dir + side * (a.cos() * r) + up * (a.sin() * r)).normalize();
            let speed = 20.0 + self.rand() * 55.0;
            let size = 0.12 + self.rand() * self.rand() * 0.5;
            self.drops.push(Drop { pos: from, vel: d * speed, size, age: 0.0 });
        }
    }

    pub fn update(&mut self, dt: f32, col: &Collider) {
        let mut landed = Vec::new();
        self.drops.retain_mut(|d| {
            d.age += dt;
            d.vel.y += GRAVITY * dt;
            let step = d.vel * dt;
            let len = step.length();
            if len > 1e-5 {
                if let Some(t) = col.ray(d.pos, step / len, len, &[]) {
                    landed.push((d.pos + step / len * t, d.size));
                    return false;
                }
            }
            d.pos += step;
            d.age < DROP_LIFE
        });
        for (p, size) in landed {
            // Пятно чуть над поверхностью (стол и пол — горизонтальные).
            let mesh = self.stain_meshes[(self.rand() * 2.0) as usize % 2];
            let angle = self.rand() * std::f32::consts::TAU;
            let size = size * (3.0 + self.rand() * 3.0);
            if self.stains.len() >= MAX_STAINS {
                self.stains.remove(0);
            }
            self.stains.push(Stain { pos: p + Vec3::Y * (0.02 + self.stains.len() as f32 * 0.00005), angle, size, age: 0.0, mesh });
        }
        for s in &mut self.stains {
            s.age += dt;
        }
    }

    /// `facing` — поворот к камере (для капель), `light` — яркость вокруг.
    pub fn push_instances(&self, out: &mut Vec<MeshInstance>, facing: (f32, f32, f32), light: f32) {
        for s in &self.stains {
            // Растекается за полсекунды.
            let k = 0.35 + 0.65 * (s.age / 0.5).min(1.0);
            let sz = s.size * k;
            out.push(MeshInstance::new(s.mesh).at(s.pos.x, s.pos.y, s.pos.z).rotated(0.0, s.angle, 0.0).scaled(sz, 1.0, sz));
        }
        if light <= 0.001 {
            return;
        }
        let (rx, ry, rz) = facing;
        for d in &self.drops {
            out.push(MeshInstance::new(self.drop_mesh).at(d.pos.x, d.pos.y, d.pos.z).rotated(rx, ry, rz).scaled(d.size, d.size, d.size).tinted(light, light, light, 1.0));
        }
    }
}
