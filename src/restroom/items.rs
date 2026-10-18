//! Физика вещей в пустоте: гильзы и предметы Buckshot (префабы сцены).
//!
//! Движение, вращение и столкновения вещей между собой считает Inertial
//! (`alkash3d-inertial`, физический плагин движка): каждая вещь — коробка
//! по габаритам своей модели. Плоскости Inertial бесконечные, а стол —
//! нет (вещь за краем стола "висела" бы в воздухе), поэтому со столом и
//! полом вещи сталкивает наш треугольный коллайдер: после шага физики
//! восемь углов коробки проверяются по геометрии — путь каждого угла за
//! кадр (луч от прошлого положения, чтобы быстрая вещь не проскакивала
//! тонкую столешницу) и касание; вошедший угол выталкивает тело и даёт
//! импульс в точке контакта (с вращением и трением) — вещь кувыркается и
//! ложится плашмя, а не катится как шар.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::math::{Mat3, Quat, Vec3};
use alkash3d_rs::{PhysicsBody, PhysicsConfig};

use crate::collide::Collider;
use crate::graph::{Graph, Trs};

/// Inertial собирается GNU-таргетом (см. его .cargo/config.toml): свежая DLL
/// и рантаймы gfortran — здесь, а не в target/release (там устаревшая сборка).
const INERTIAL_DLL: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";
/// Гравитация сцены: 1 единица ≈ 10 см.
pub const GRAVITY: f32 = -98.0;
/// Упругость и трение о стол/пол.
const RESTITUTION: f32 = 0.25;
const FRICTION: f32 = 0.45;
/// Сколько вещей одновременно (старые убираются).
const MAX_ITEMS: usize = 40;

/// Вещь: префаб, полуразмеры коробки, центр коробки в координатах префаба, масса.
#[derive(Clone, Copy)]
pub struct Kind {
    pub prefab: &'static str,
    half: Vec3,
    center: Vec3,
    mass: f32,
}

pub const SHELL: Kind = Kind { prefab: "shell", half: Vec3::new(0.2, 0.2, 0.55), center: Vec3::ZERO, mass: 0.05 };
pub const ITEMS: [Kind; 5] = [
    Kind { prefab: "item_handsaw", half: Vec3::new(0.6, 0.2, 2.1), center: Vec3::new(0.2, 0.0, 0.0), mass: 0.6 },
    Kind { prefab: "item_magnifying glass", half: Vec3::new(0.9, 0.15, 2.0), center: Vec3::new(0.0, 0.15, 0.3), mass: 0.3 },
    Kind { prefab: "item_beer can", half: Vec3::new(0.7, 1.3, 0.7), center: Vec3::new(0.0, 1.3, 0.0), mass: 0.4 },
    Kind { prefab: "item_cigarettes", half: Vec3::new(0.6, 0.35, 1.15), center: Vec3::new(0.0, 0.35, 0.35), mass: 0.1 },
    Kind { prefab: "item_handcuffs", half: Vec3::new(1.5, 0.3, 1.2), center: Vec3::new(-0.1, 0.2, -0.5), mass: 0.3 },
];

struct Item {
    body: i32,
    spawned: u32,
    kind: Kind,
    /// Углы коробки в мире в прошлом кадре (после столкновений).
    prev: Option<[Vec3; 8]>,
}

pub struct Items {
    items: Vec<Item>,
    ok: bool,
}

impl Items {
    pub fn new(engine: &mut AlkashEngine) -> Items {
        let config = PhysicsConfig { max_bodies: 256, world_size: 200.0, cell_size: 4.0, solver_iterations: 8, use_simd: 0 };
        let ok = match engine.init_physics(INERTIAL_DLL, config) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[ITEMS] Inertial не загрузился ({:?}) — вещи без физики", e);
                false
            }
        };
        Items { items: Vec::new(), ok }
    }

    /// Убрать все вещи.
    pub fn clear(&mut self, engine: &mut AlkashEngine, g: &mut Graph) {
        for it in self.items.drain(..) {
            if let Some(p) = engine.physics.as_mut() {
                p.remove_body(it.body);
            }
            g.despawn(it.spawned);
        }
    }

    /// Бросить вещь: центр коробки в `pos`, поворот `rot`, скорость и вращение.
    pub fn throw(&mut self, engine: &mut AlkashEngine, g: &mut Graph, kind: Kind, pos: Vec3, rot: Quat, vel: Vec3, spin: Vec3) {
        if !self.ok {
            return;
        }
        if self.items.len() >= MAX_ITEMS {
            let old = self.items.remove(0);
            if let Some(p) = engine.physics.as_mut() {
                p.remove_body(old.body);
            }
            g.despawn(old.spawned);
        }
        let h = kind.half;
        let body = PhysicsBody {
            position: pos.to_array(),
            velocity: vel.to_array(),
            acceleration: [0.0; 3],
            angular_velocity: spin.to_array(),
            angular_acceleration: [0.0; 3],
            mass: kind.mass,
            inv_mass: 1.0 / kind.mass,
            restitution: 0.3,
            friction: 0.5,
            linear_damping: 0.02,
            angular_damping: 0.05,
            is_static: 0,
            is_asleep: 0,
            orientation: rot.to_array(),
            radius: h.max_element(),
            shape_type: alkash3d_rs::shape_type::BOX,
            half_extents: h.to_array(),
        };
        let Some(id) = engine.add_physics_body(body) else { return };
        let spawned = g.spawn(kind.prefab, 0, Trs { pos, rot, scale: Vec3::ONE }, 0);
        self.items.push(Item { body: id, spawned, kind, prev: None });
    }

    /// После шага физики (`engine.update`): удары о стол и пол, модели — за телами.
    pub fn update(&mut self, engine: &mut AlkashEngine, g: &mut Graph, col: &Collider) {
        for it in &mut self.items {
            let Some(b) = engine.get_physics_body(it.body) else { continue };
            // Тело, добавленное в этом кадре, до шага физики плагин отдаёт
            // "нулевым" (масса 0, ориентация 0,0,0,0): нормировка дала бы
            // NaN, а NaN, отправленный обратно в солвер, портит его память.
            let q = Quat::from_array(b.orientation);
            if std::env::var("RESTROOM_ITEMS_LOG").is_ok() {
                eprintln!("[ITEMS] тело {} {}: pos {:?} vel {:?} rot {:?} mass {}", it.body, it.kind.prefab, b.position, b.velocity, b.orientation, b.mass);
            }
            if b.mass <= 0.0 || q.length_squared() < 0.25 || !Vec3::from(b.position).is_finite() {
                continue;
            }
            let mut pos = Vec3::from(b.position);
            let rot = q.normalize();
            let mut v = Vec3::from(b.velocity);
            let mut w = Vec3::from(b.angular_velocity);
            let h = it.kind.half;
            // Обратный тензор инерции коробки в мире.
            let m = it.kind.mass;
            let i_local = Vec3::new(h.y * h.y + h.z * h.z, h.x * h.x + h.z * h.z, h.x * h.x + h.y * h.y) * (m * 4.0 / 12.0);
            let r3 = Mat3::from_quat(rot);
            let i_inv = r3 * Mat3::from_diagonal(i_local.recip()) * r3.transpose();
            let mut touched = false;
            for k in 0..8 {
                let s = Vec3::new(if k & 1 == 0 { -1.0 } else { 1.0 }, if k & 2 == 0 { -1.0 } else { 1.0 }, if k & 4 == 0 { -1.0 } else { 1.0 });
                let r = rot * (s * h);
                let corner = pos + r;
                // Путь угла за кадр: пересёк поверхность — выталкиваем на неё.
                let mut hit: Option<(Vec3, f32)> = None;
                if let Some(prev) = it.prev {
                    let step = corner - prev[k];
                    let len = step.length();
                    if len > 1e-4 {
                        if let Some((t, n)) = col.ray_hit(prev[k], step / len, len) {
                            hit = Some((n, (len - t) * (step / len).dot(-n) + 0.01));
                        }
                    }
                }
                if hit.is_none() {
                    let (d, _) = col.push_sphere(corner, 0.05, &[]);
                    let depth = d.length();
                    if depth > 1e-5 {
                        hit = Some((d / depth, depth));
                    }
                }
                let Some((n, depth)) = hit else { continue };
                touched = true;
                pos += n * depth.max(0.0);
                // Импульс в точке контакта: отскок по нормали + трение.
                let vp = v + w.cross(r);
                let vn = vp.dot(n);
                if vn < 0.0 {
                    let rn = r.cross(n);
                    let k_n = 1.0 / m + n.dot((i_inv * rn).cross(r));
                    let j = -(1.0 + RESTITUTION) * vn / k_n;
                    let vt = vp - n * vn;
                    let jt = if vt.length() > 1e-4 { -vt.normalize() * (FRICTION * j).min(vt.length() / k_n) } else { Vec3::ZERO };
                    let imp = n * j + jt;
                    v += imp / m;
                    w += i_inv * r.cross(imp);
                }
            }
            if touched && pos.is_finite() && v.is_finite() && w.is_finite() {
                // Лежит — гасим дрожание.
                if v.length() < 1.5 {
                    v *= 0.9;
                    w *= 0.85;
                }
                if std::env::var("RESTROOM_ITEMS_LOG").is_ok() {
                    eprintln!("[ITEMS]   -> pos {:?} vel {:?} w {:?}", pos, v, w);
                }
                engine.set_physics_transform(it.body, pos.to_array(), rot.to_array());
                engine.set_physics_velocity(it.body, v.to_array(), w.to_array());
            }
            let mut corners = [Vec3::ZERO; 8];
            for (k, c) in corners.iter_mut().enumerate() {
                let s = Vec3::new(if k & 1 == 0 { -1.0 } else { 1.0 }, if k & 2 == 0 { -1.0 } else { 1.0 }, if k & 4 == 0 { -1.0 } else { 1.0 });
                *c = pos + rot * (s * h);
            }
            it.prev = Some(corners);
            if let Some(sp) = g.spawned.get_mut(&it.spawned) {
                sp.local = Trs { pos: pos - rot * it.kind.center, rot, scale: Vec3::ONE };
            }
        }
    }
}
