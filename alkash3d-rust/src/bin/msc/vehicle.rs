//! Машины мира. Корень машины (например "HAYOSIKO(1500kg, 250)") — подвижное
//! поддерево `World`, его двигает модель ниже, высоту и наклон кузова даёт
//! рельеф под колёсами (`Ground`).
//!
//! Модель — "велосипедная" с честной продольной динамикой: тяга двигателя
//! ограничена мощностью (F = P / v) и сцеплением шин, против неё —
//! аэродинамика ½ρCdA·v², качение и составляющая веса на уклоне. Поворот —
//! кинематика по колёсной базе, но угол руля ограничен сцеплением: боковое
//! ускорение v²/R не больше μg (на скорости руль "короче", как у настоящей
//! машины, а не телепорт по дуге). Заносов и подвески нет — это упрощение.
//! PlayMaker-логика оригинала (зажигание, топливо, поломки) не перенесена.

use alkash3d_rs::math::{Quat, Vec3};

use crate::ground::Ground;
use crate::world::World;

const G: f32 = 9.81;
const AIR: f32 = 1.2;

#[derive(Clone, Copy)]
pub struct Spec {
    /// Префикс имени корня в мире.
    pub root: &'static str,
    pub label: &'static str,
    /// Мощность, Вт.
    pub power: f32,
    /// Ограничение скорости передачами/регулятором, м/с.
    pub v_max: f32,
    /// Cd·A, м².
    pub cd_a: f32,
    /// Сцепление шин.
    pub grip: f32,
    pub max_steer: f32,
    pub idle_sound: &'static str,
    pub start_sound: &'static str,
    /// Передаточные числа (для звука двигателя): скорость на 1000 об/мин по передачам, м/с.
    pub gears: &'static [f32],
    pub redline: f32,
}

pub struct Vehicle {
    pub spec: Spec,
    pub root: usize,
    pub mass: f32,
    pub position: Vec3,
    pub yaw: f32,
    pub speed: f32,
    pub steer: f32,
    pub rpm: f32,
    pub gear: usize,
    wheelbase: f32,
    track: f32,
    lift: f32,
    scale: Vec3,
    pitch: f32,
    roll: f32,
    pub head: Option<usize>,
    wheels: Vec<(usize, Quat, bool, f32)>,
    spin: f32,
    /// Доля мощности двигателя (Satsuma: 0 — заглушен, меньше 1 — плохо отрегулирован).
    pub power_scale: f32,
    /// Колёса/привод на месте — машина может катиться (Satsuma без колёс стоит).
    pub rolling: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Controls {
    /// 0..1
    pub throttle: f32,
    /// 0..1
    pub brake: f32,
    /// -1 (влево) .. 1 (вправо)
    pub steer: f32,
    pub handbrake: bool,
}

/// Масса из имени корня, как в оригинале: "HAYOSIKO(1500kg, 250)" -> 1500.
fn mass_from_name(name: &str) -> Option<f32> {
    let i = name.find("kg")?;
    let digits: String = name[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().ok()
}

impl Vehicle {
    pub fn new(world: &World, ground: &Ground, root: usize, spec: Spec, default_mass: f32) -> Vehicle {
        let g = world.global[root];
        let (scale, rot, pos) = g.to_scale_rotation_translation();
        // Вперёд у моделей Unity — +Z, после зеркалирования — -Z.
        let f = rot * Vec3::NEG_Z;
        let yaw = (-f.x).atan2(-f.z);
        let ground_y = ground.height_below(pos + Vec3::Y * 1.5).unwrap_or(pos.y);

        // Колёса: узлы wheelFL/FR/RL/RR (у грузовика — wheelRL1/RL2 …).
        let mut wheels = Vec::new();
        let mut front_z: Vec<f32> = Vec::new();
        let mut rear_z: Vec<f32> = Vec::new();
        let mut xs: Vec<f32> = Vec::new();
        let inv = g.inverse();
        let mut stack = vec![root];
        while let Some(i) = stack.pop() {
            let n = &world.nodes[i];
            if n.name.starts_with("wheel") && n.name.len() <= 8 && n.name[5..].starts_with(['F', 'R']) {
                let front = n.name[5..].starts_with('F');
                let local = inv.transform_point3(world.global[i].w_axis.truncate());
                if front { front_z.push(local.z) } else { rear_z.push(local.z) }
                xs.push(local.x.abs());
                let radius = 0.33;
                wheels.push((i, n.local.rot, front, radius));
                continue;
            }
            stack.extend_from_slice(&world.children[i]);
        }
        let avg = |v: &[f32], d: f32| if v.is_empty() { d } else { v.iter().sum::<f32>() / v.len() as f32 };
        let wheelbase = (avg(&rear_z, 1.3) - avg(&front_z, -1.3)).abs().max(1.5);
        let track = (avg(&xs, 0.75) * 2.0).max(1.0);
        Vehicle {
            spec,
            root,
            mass: mass_from_name(&world.nodes[root].name).unwrap_or(default_mass),
            position: Vec3::new(pos.x, ground_y, pos.z),
            yaw,
            speed: 0.0,
            steer: 0.0,
            rpm: 800.0,
            gear: 0,
            wheelbase,
            track,
            lift: pos.y - ground_y,
            scale,
            pitch: 0.0,
            roll: 0.0,
            head: world.find_in(root, "DriverHeadPivot"),
            wheels,
            spin: 0.0,
            power_scale: 1.0,
            rolling: true,
        }
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch) * Quat::from_rotation_z(self.roll)
    }

    pub fn forward(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw) * Vec3::NEG_Z
    }

    pub fn step(&mut self, c: Controls, ground: &Ground, dt: f32) {
        let m = self.mass;
        let v = self.speed;

        // --- руль: ограничение сцеплением, R = L / tan δ, v²/R ≤ μg ---
        let grip_limit = if v.abs() > 1.0 { (self.spec.grip * G * self.wheelbase / (v * v)).atan() } else { self.spec.max_steer };
        let target = c.steer.clamp(-1.0, 1.0) * self.spec.max_steer.min(grip_limit);
        // Руль крутится с конечной скоростью, к центру возвращается быстрее.
        let rate = if target.abs() < self.steer.abs() { 3.0 } else { 1.6 };
        self.steer += (target - self.steer).clamp(-rate * dt, rate * dt);

        // --- продольные силы ---
        let traction = self.spec.grip * m * G * 0.6;
        let mut force = 0.0;
        if c.throttle > 0.0 {
            let governor = (1.0 - (v.max(0.0) / self.spec.v_max).powi(6)).clamp(0.0, 1.0);
            force += c.throttle * (self.spec.power * self.power_scale / v.abs().max(2.0)).min(traction) * governor;
        }
        // Тормоз: при движении вперёд — тормозит, стоя — задний ход.
        let mut brake = 0.0;
        if c.brake > 0.0 {
            if v > 0.5 {
                brake += c.brake * self.spec.grip * m * G * 0.8;
            } else {
                let governor = (1.0 - (-v.min(0.0) / 5.0).powi(4)).clamp(0.0, 1.0);
                force -= c.brake * (self.spec.power * self.power_scale * 0.5 / v.abs().max(2.0)).min(traction) * governor;
            }
        }
        if c.handbrake {
            brake += self.spec.grip * m * G * 0.5;
        }
        // Вес на уклоне (нос вверх — тянет назад).
        let slope = -m * G * self.pitch.sin();
        let mut nv = v + (force + slope) / m * dt;
        // Аэродинамика, качение и тормоза только гасят скорость до нуля.
        let resist = 0.5 * AIR * self.spec.cd_a * v * v + 0.015 * m * G * (v.abs() > 0.05) as u8 as f32 + brake;
        let dv = resist / m * dt;
        nv = if nv.abs() <= dv { 0.0 } else { nv - nv.signum() * dv };
        self.speed = if self.rolling { nv } else { 0.0 };

        // --- курс и позиция ---
        if self.speed.abs() > 0.01 {
            self.yaw -= self.speed / self.wheelbase * self.steer.tan() * dt;
        }
        let before = self.position;
        let f = self.forward();
        self.position += f * self.speed * dt;

        // Стена/дом/дерево на уровне бампера — удар.
        let probe = Vec3::Y * 0.7;
        let reach = self.wheelbase * 0.5 + 1.0;
        let bumper = self.position + f * self.speed.signum() * reach;
        if self.speed.abs() > 0.05 && ground.wall_between(before + probe, bumper + probe) {
            self.position = before;
            self.speed *= -0.15;
        }

        // --- рельеф под колёсами: высота и наклон кузова ---
        let r = f.cross(Vec3::Y);
        let half = self.wheelbase * 0.5;
        let base = self.position.y;
        let sample = |p: Vec3| ground.height_below(p + Vec3::Y * 1.2).unwrap_or(base);
        let front = sample(self.position + f * half);
        let back = sample(self.position - f * half);
        let left = sample(self.position - r * self.track * 0.5);
        let right = sample(self.position + r * self.track * 0.5);
        let target_y = (front + back + left + right) * 0.25;
        let k = (dt * 12.0).min(1.0);
        self.position.y += (target_y - self.position.y) * k;
        self.pitch += (((front - back) / (2.0 * half)).atan() - self.pitch) * k;
        // Поворот вокруг Z на +θ поднимает правый (+X) борт.
        self.roll += (((right - left) / self.track).atan() - self.roll) * k;

        // --- обороты для звука: автомат по скорости ---
        let gears = self.spec.gears;
        let kmh_per_krpm = |g: usize| gears[g.min(gears.len() - 1)];
        let mut rpm = self.speed.abs() / kmh_per_krpm(self.gear) * 1000.0;
        if rpm > self.spec.redline * 0.85 && self.gear + 1 < gears.len() {
            self.gear += 1;
        } else if self.gear > 0 && self.speed.abs() / kmh_per_krpm(self.gear - 1) * 1000.0 < self.spec.redline * 0.55 {
            self.gear -= 1;
        }
        rpm = (self.speed.abs() / kmh_per_krpm(self.gear) * 1000.0).max(800.0 + 2200.0 * c.throttle * (self.speed.abs() < 1.0) as u8 as f32);
        self.rpm += (rpm.min(self.spec.redline) - self.rpm) * (dt * 6.0).min(1.0);
        self.spin += self.speed * dt;
    }

    /// Записать позу машины и колёс в мир.
    pub fn apply(&self, world: &mut World) {
        let n = &mut world.nodes[self.root];
        n.local.pos = self.position + Vec3::Y * self.lift;
        n.local.rot = self.rotation();
        n.local.scale = self.scale;
        for &(i, base, front, radius) in &self.wheels {
            // Unity: ось колеса — X узла; вперёд — -Z, значит качение — вокруг -X.
            let spin = Quat::from_rotation_x(-self.spin / radius);
            let steer = if front { Quat::from_rotation_y(-self.steer) } else { Quat::IDENTITY };
            world.nodes[i].local.rot = steer * base * spin;
        }
        world.update_subtree(self.root);
    }

    /// Точка глаз водителя в мире (узел DriverHeadPivot оригинала).
    pub fn head(&self, world: &World) -> Vec3 {
        match self.head {
            Some(h) => world.global[h].w_axis.truncate(),
            None => self.position + Vec3::Y * 1.4,
        }
    }

    /// Вытолкнуть сферу игрока из кузова (бокс по колёсной базе и колее, по горизонтали).
    pub fn push_player(&self, center: Vec3, radius: f32) -> Vec3 {
        let half = Vec3::new(self.track * 0.5 + 0.15, 1.0, self.wheelbase * 0.5 + 0.8);
        let rot = Quat::from_rotation_y(self.yaw);
        let q = rot.inverse() * (center - (self.position + Vec3::Y * half.y));
        if q.y.abs() > half.y + 0.3 {
            return Vec3::ZERO;
        }
        let pen = Vec3::new(half.x + radius - q.x.abs(), 0.0, half.z + radius - q.z.abs());
        if pen.x <= 0.0 || pen.z <= 0.0 {
            return Vec3::ZERO;
        }
        let local = if pen.x < pen.z { Vec3::new(pen.x * q.x.signum(), 0.0, 0.0) } else { Vec3::new(0.0, 0.0, pen.z * q.z.signum()) };
        rot * local
    }

    pub fn half_width(&self) -> f32 {
        self.track * 0.5
    }

    pub fn kmh(&self) -> f32 {
        self.speed.abs() * 3.6
    }
}
