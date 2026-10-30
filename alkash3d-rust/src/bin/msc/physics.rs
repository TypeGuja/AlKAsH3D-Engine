//! Предметы мира как твёрдые тела: детали, инструменты, товары — всё, у
//! чего в оригинале был Rigidbody (в `BODY_ROOTS`, см. world.rs).
//!
//! Форма тела — коллайдеры Unity самого объекта и его потомков, сведённые к
//! набору контактных сфер: углы и центры граней боксов (r ≈ 1.5 см), центры
//! сфер, концы капсул; сеточный коллайдер — углы его ограничивающего бокса.
//! Каждая точка проверяется против треугольников мира (`Ground::contact`) и
//! против боксов других тел. Удар — импульс с восстановлением и кулоновым
//! трением по честному тензору инерции бокса; вылезание из поверхности —
//! позиционная коррекция. Тело засыпает, когда почти не движется, и
//! просыпается, когда его берут, толкают или на него что-то падает.
//! Упрощения: тело-тело — точка против бокса (без рёбер), шаг фиксированный.

use alkash3d_rs::math::{Mat3, Mat4, Quat, Vec3};

use crate::ground::Ground;
use crate::world::World;
use crate::world_file::Shape;

const G: f32 = 9.81;
const RESTITUTION: f32 = 0.25;
const FRICTION: f32 = 0.6;
const STEP: f32 = 1.0 / 120.0;

pub struct Body {
    pub node: usize,
    pub mass: f32,
    inv_mass: f32,
    inv_inertia: Vec3,
    /// Центр масс в мире и поворот тела.
    pub com: Vec3,
    pub rot: Quat,
    /// Центр масс в координатах узла (без масштаба узла) и масштаб узла.
    com_node: Vec3,
    scale: Vec3,
    pub vel: Vec3,
    pub ang: Vec3,
    /// Контактные сферы в системе тела (от центра масс).
    samples: Vec<(Vec3, f32)>,
    /// Полуразмеры бокса вокруг центра масс в системе тела.
    pub half: Vec3,
    pub awake: bool,
    still: f32,
    pub held: bool,
    /// Установлено на машину/деталь — физика не считает, позу задаёт сборка.
    pub attached: bool,
    /// Ключ детали для сборки (нормализованное имя), если это деталь.
    pub key: String,
    pub name: String,
}

/// Нормализованное имя: без "(Clone)"/"(xxxxx)" и прочих скобок, только буквы и цифры.
pub fn norm_key(name: &str) -> String {
    let base = match name.find('(') {
        Some(i) => &name[..i],
        None => name,
    };
    base.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Точки коллайдера в мире: (точка, радиус).
fn shape_samples(shape: Shape, m: Mat4, mesh_box: impl Fn(usize) -> Option<(Vec3, Vec3)>, out: &mut Vec<(Vec3, f32)>) {
    let (s, _, _) = m.to_scale_rotation_translation();
    let sc = s.abs().max_element();
    let corners = |lo: Vec3, hi: Vec3, out: &mut Vec<(Vec3, f32)>| {
        for k in 0..8 {
            let p = Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
            out.push((m.transform_point3(p), 0.015));
        }
        let c = (lo + hi) * 0.5;
        for (axis, v) in [(0, lo.x), (0, hi.x), (1, lo.y), (1, hi.y), (2, lo.z), (2, hi.z)] {
            let mut p = c;
            p[axis] = v;
            out.push((m.transform_point3(p), 0.015));
        }
    };
    match shape {
        Shape::Box { center, size } => corners(center - size * 0.5, center + size * 0.5, out),
        Shape::Sphere { center, radius } => out.push((m.transform_point3(center), radius * sc)),
        Shape::Capsule { center, radius, height, axis } => {
            let ax = match axis {
                0 => Vec3::X,
                2 => Vec3::Z,
                _ => Vec3::Y,
            };
            let half = (height * 0.5 - radius).max(0.0);
            for t in [-1.0, 0.0, 1.0] {
                out.push((m.transform_point3(center + ax * half * t), radius * sc));
            }
        }
        Shape::Mesh { mesh, .. } => {
            if let Some((lo, hi)) = mesh_box(mesh) {
                corners(lo, hi, out);
            }
        }
    }
}

impl Body {
    /// Тело из узла мира и его коллайдеров. `mesh_box` — локальный бокс
    /// меша-коллайдера по индексу меша файла.
    pub fn new(world: &World, node: usize, mesh_box: &impl Fn(usize) -> Option<(Vec3, Vec3)>) -> Option<Body> {
        let g = world.global[node];
        let mut pts = Vec::new();
        let mut stack = vec![node];
        while let Some(i) = stack.pop() {
            for c in &world.nodes[i].colliders {
                if !c.trigger {
                    shape_samples(c.shape, world.global[i], mesh_box, &mut pts);
                }
            }
            for &ch in &world.children[i] {
                // Выключенные части (болты, установленные копии) — не форма тела.
                if world.nodes[ch].active && !world.nodes[ch].name.starts_with("trigger") {
                    stack.push(ch);
                }
            }
        }
        if pts.is_empty() {
            return None;
        }
        let (scale, rot, origin) = g.to_scale_rotation_translation();
        let inv = rot.inverse();
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (p, r) in &pts {
            let q = inv * (*p - origin);
            lo = lo.min(q - Vec3::splat(*r));
            hi = hi.max(q + Vec3::splat(*r));
        }
        let center_rot = (lo + hi) * 0.5;
        let half = ((hi - lo) * 0.5).max(Vec3::splat(0.01));
        let com = origin + rot * center_rot;
        let mass = world.nodes[node].rigidbody.map(|r| r.mass).unwrap_or(1.0).clamp(0.05, 2000.0);
        // Тензор инерции сплошного бокса.
        let (x, y, z) = ((half.x * 2.0).powi(2), (half.y * 2.0).powi(2), (half.z * 2.0).powi(2));
        let inertia = Vec3::new(y + z, x + z, x + y) * (mass / 12.0);
        let samples = pts.iter().map(|(p, r)| (inv * (*p - com), *r)).collect();
        Some(Body {
            node,
            mass,
            inv_mass: 1.0 / mass,
            inv_inertia: Vec3::ONE / inertia.max(Vec3::splat(1e-5)),
            com,
            rot,
            com_node: center_rot / scale,
            scale,
            vel: Vec3::ZERO,
            ang: Vec3::ZERO,
            samples,
            half,
            awake: false,
            still: 0.0,
            held: false,
            attached: false,
            key: norm_key(&world.nodes[node].name),
            name: world.nodes[node].name.clone(),
        })
    }

    /// Мировая матрица узла тела по текущему состоянию.
    pub fn node_matrix(&self) -> Mat4 {
        let origin = self.com - self.rot * (self.com_node * self.scale);
        Mat4::from_scale_rotation_translation(self.scale, self.rot, origin)
    }

    /// Поставить тело по мировой матрице узла (сборка, вынуть из машины).
    pub fn set_from_node_matrix(&mut self, m: Mat4) {
        let (_, rot, origin) = m.to_scale_rotation_translation();
        self.rot = rot;
        self.com = origin + rot * (self.com_node * self.scale);
    }

    pub fn wake(&mut self) {
        self.awake = true;
        self.still = 0.0;
    }

    fn inv_inertia_world(&self) -> Mat3 {
        let r = Mat3::from_quat(self.rot);
        r * Mat3::from_diagonal(self.inv_inertia) * r.transpose()
    }

    fn apply_impulse(&mut self, j: Vec3, rp: Vec3) {
        self.vel += j * self.inv_mass;
        self.ang += self.inv_inertia_world() * rp.cross(j);
    }

    /// Точка внутри бокса тела (с запасом `r`): нормаль наружу и глубина.
    fn box_contact(&self, p: Vec3, r: f32) -> Option<(Vec3, f32)> {
        let q = self.rot.inverse() * (p - self.com);
        let pen = self.half + Vec3::splat(r) - q.abs();
        if pen.min_element() <= 0.0 {
            return None;
        }
        let axis = if pen.x < pen.y && pen.x < pen.z { 0 } else if pen.y < pen.z { 1 } else { 2 };
        let mut n = Vec3::ZERO;
        n[axis] = q[axis].signum();
        Some((self.rot * n, pen[axis]))
    }

    /// Луч в бокс тела: расстояние.
    pub fn ray(&self, o: Vec3, d: Vec3) -> Option<f32> {
        let inv = self.rot.inverse();
        ray_box(inv * (o - self.com), inv * d, -self.half, self.half)
    }
}

/// Луч в бокс [lo, hi] (координаты бокса): расстояние до входа.
pub fn ray_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = f32::MAX;
    for a in 0..3 {
        if d[a].abs() < 1e-9 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

pub struct Physics {
    pub bodies: Vec<Body>,
    acc: f32,
}

impl Physics {
    pub fn new(world: &World, mesh_box: impl Fn(usize) -> Option<(Vec3, Vec3)>) -> Physics {
        let bodies: Vec<Body> = (0..world.nodes.len())
            // И выключенные: это детали, стоящие на машине с завода (их снимают).
            .filter(|&i| world.nodes[i].body_root)
            .filter_map(|i| Body::new(world, i, &mesh_box))
            .collect();
        Physics { bodies, acc: 0.0 }
    }

    /// Шаг симуляции; `hold` — (тело, точка перед камерой) в руках игрока.
    pub fn step(&mut self, ground: &Ground, dt: f32, hold: Option<(usize, Vec3)>, visible: &[bool]) {
        self.acc = (self.acc + dt).min(0.1);
        while self.acc >= STEP {
            self.acc -= STEP;
            self.substep(ground, STEP, hold, visible);
        }
    }

    fn substep(&mut self, ground: &Ground, dt: f32, hold: Option<(usize, Vec3)>, visible: &[bool]) {
        for bi in 0..self.bodies.len() {
            if !self.bodies[bi].awake || self.bodies[bi].attached || !visible[self.bodies[bi].node] {
                continue;
            }
            {
                let b = &mut self.bodies[bi];
                if let Some((h, target)) = hold.filter(|h| h.0 == bi) {
                    // Рука — пружина с демпфером: тяжёлое тянется медленнее.
                    let k = (40.0 / b.mass.max(1.0)).clamp(2.0, 25.0);
                    let want = ((target - b.com) * k).clamp_length_max(12.0);
                    b.vel += (want - b.vel) * (dt * 15.0).min(1.0);
                    b.ang *= 1.0 - (dt * 10.0).min(1.0);
                    let _ = h;
                } else {
                    b.vel.y -= G * dt;
                }
                b.vel *= 1.0 - 0.02 * dt;
                b.ang *= 1.0 - 0.3 * dt;
                b.com += b.vel * dt;
                let w = b.ang;
                if w.length_squared() > 1e-12 {
                    b.rot = (Quat::from_scaled_axis(w * dt) * b.rot).normalize();
                }
            }
            // Контакты: мир и другие тела.
            for _ in 0..2 {
                let samples = self.bodies[bi].samples.clone();
                let mut contacts: Vec<(Vec3, Vec3, f32, Option<usize>)> = Vec::new();
                let (com, rot, reach) = {
                    let b = &self.bodies[bi];
                    (b.com, b.rot, b.half.length() + 0.1)
                };
                for (s, r) in &samples {
                    let p = com + rot * *s;
                    if let Some((n, d)) = ground.contact(p, *r) {
                        contacts.push((p, n, d, None));
                    }
                    for oi in 0..self.bodies.len() {
                        if oi == bi || self.bodies[oi].attached || !visible[self.bodies[oi].node] {
                            continue;
                        }
                        let o = &self.bodies[oi];
                        if (o.com - com).length() > reach + o.half.length() {
                            continue;
                        }
                        if let Some((n, d)) = o.box_contact(p, *r) {
                            contacts.push((p, n, d, Some(oi)));
                        }
                    }
                }
                if contacts.is_empty() {
                    break;
                }
                let count = contacts.len() as f32;
                for (p, n, depth, other) in contacts {
                    let b = &mut self.bodies[bi];
                    let rp = p - b.com;
                    let vp = b.vel + b.ang.cross(rp);
                    let vn = vp.dot(n);
                    b.com += n * (depth * 0.8 / count.sqrt());
                    if vn < 0.0 {
                        let iw = b.inv_inertia_world();
                        let k = b.inv_mass + n.dot((iw * rp.cross(n)).cross(rp));
                        let e = if vn > -0.5 { 0.0 } else { RESTITUTION };
                        let j = -(1.0 + e) * vn / k / count;
                        b.apply_impulse(n * j, rp);
                        // Трение.
                        let vp = b.vel + b.ang.cross(rp);
                        let vt = vp - n * vp.dot(n);
                        let vt_len = vt.length();
                        if vt_len > 1e-5 {
                            let t = vt / vt_len;
                            let kt = b.inv_mass + t.dot((iw * rp.cross(t)).cross(rp));
                            let jt = (vt_len / kt / count).min(FRICTION * j);
                            b.apply_impulse(-t * jt, rp);
                        }
                        if let Some(oi) = other {
                            // Удар будит соседа и толкает его.
                            let push = n * (-j * 0.5);
                            let o = &mut self.bodies[oi];
                            if !o.held {
                                o.wake();
                                o.vel += push * o.inv_mass;
                            }
                        }
                    }
                }
            }
            // Сон.
            let b = &mut self.bodies[bi];
            if b.vel.length() < 0.06 && b.ang.length() < 0.15 && hold.is_none_or(|h| h.0 != bi) {
                b.still += dt;
                if b.still > 0.6 {
                    b.awake = false;
                    b.vel = Vec3::ZERO;
                    b.ang = Vec3::ZERO;
                }
            } else {
                b.still = 0.0;
            }
        }
    }

    /// Ближайшее тело на луче (`d` единичный) не дальше `max`.
    pub fn pick(&self, world: &World, o: Vec3, d: Vec3, max: f32) -> Option<(usize, f32)> {
        self.bodies
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.attached && world.visible[b.node])
            .filter_map(|(i, b)| b.ray(o, d).filter(|&t| t <= max).map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Записать позы проснувшихся тел в мир.
    pub fn apply(&self, world: &mut World) {
        for b in &self.bodies {
            if b.attached || (!b.awake && !b.held) {
                continue;
            }
            world.set_world_matrix(b.node, b.node_matrix());
        }
    }

    /// Вытолкнуть сферу игрока из тел (горизонтально); тела, в которые
    /// игрок упёрся, получают толчок. Возвращает сдвиг игрока.
    pub fn push_player(&mut self, world: &World, center: Vec3, radius: f32, vel: Vec3) -> Vec3 {
        let mut shift = Vec3::ZERO;
        for b in &mut self.bodies {
            if b.attached || b.held || !world.visible[b.node] || (b.com - center).length() > b.half.length() + radius + 0.5 {
                continue;
            }
            if let Some((n, d)) = b.box_contact(center + shift, radius) {
                let n_h = Vec3::new(n.x, 0.0, n.z);
                if n_h.length_squared() < 1e-4 {
                    continue;
                }
                let n_h = n_h.normalize();
                // Лёгкое игрок отталкивает, тяжёлое (> 40 кг) держит игрока.
                if b.mass < 40.0 {
                    b.wake();
                    b.vel += -n_h * (vel.dot(-n_h).max(0.0) * (8.0 / b.mass.max(1.0)).min(1.0) + 0.3);
                }
                shift += n_h * d;
            }
        }
        shift
    }

    /// Верх тел под точкой (игрок стоит на ящике).
    pub fn floor_below(&self, world: &World, p: Vec3, radius: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for b in &self.bodies {
            if b.attached || b.held || !world.visible[b.node] {
                continue;
            }
            let q = b.rot.inverse() * (p - b.com);
            if q.x.abs() < b.half.x + radius * 0.5 && q.z.abs() < b.half.z + radius * 0.5 {
                // Верх бокса в мире над этой точкой (для почти горизонтальных тел).
                let top = b.com.y + (b.rot * Vec3::new(0.0, b.half.y, 0.0)).y.abs().max(b.half.y * 0.5);
                if top <= p.y && best.is_none_or(|h| top > h) {
                    best = Some(top);
                }
            }
        }
        best
    }
}
