//! Сборка машины: места установки деталей, болты, двигатель Satsuma.
//!
//! Логика оригинала — PlayMaker, её данные не читаются, поэтому места
//! установки восстановлены по иерархии сцены, как её устроили авторы:
//! - "копия": на машине лежит выключенная установленная копия детали
//!   ("radiator(xxxxx)", "coil spring(rlxxx)") с болтами `BoltPM`;
//!   установка — свободная деталь прячется, копия включается;
//! - "точка": узел "pivot_<деталь>" (двери, крылья, ГБЦ на блоке) — деталь
//!   встаёт в точку и едет вместе с ней; болты — в самой детали ("Bolts")
//!   или в группе "Masked<Деталь>" у детали-основы.
//! Зона, куда подносить деталь, — триггер-коллайдер "trigger_<деталь>" или
//! сама копия; если триггера нет — 45 см вокруг места.
//! Детали, которые в сцене выключены, но имеют место на машине, считаются
//! заводскими: стоят и затянуты (подрамник, подвеска, тормоза, рулевое).

use std::collections::HashMap;

use alkash3d_rs::math::{Mat4, Quat, Vec3};

use crate::physics::{norm_key, ray_box, Physics};
use crate::world::World;
use crate::world_file::Shape;

/// Где лежат детали машины (остальные предметы на машину не ставятся).
pub fn is_part_root(root: &str) -> bool {
    root == "CARPARTS" || root == "STORE"
}

/// Затяжка болта: щелчков ключа до упора (как в оригинале).
pub const BOLT_STEPS: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Copy,
    Pivot,
}

pub struct Slot {
    pub key: String,
    pub target: usize,
    pub mode: Mode,
    /// Узел-зона (локальный бокс триггеров) или None — радиус вокруг места.
    trigger: Option<(usize, Vec3, Vec3)>,
    /// Корень, к которому относится место (машина или деталь-основа).
    pub host: usize,
    pub bolts: Vec<usize>,
    /// Болты места без болтов самой детали (у мест "в точку" они добавляются при установке).
    base_bolts: Vec<usize>,
    pub tight: Vec<u8>,
    /// Установленное тело (индекс в `Physics::bodies`).
    pub installed: Option<usize>,
    /// Доворот детали относительно точки (двери: в сцене точка стоит под
    /// открытую створку, закрывал её скрипт).
    extra: Quat,
}

impl Slot {
    pub fn fixed(&self) -> bool {
        self.installed.is_some() && self.tight.iter().all(|&t| t >= BOLT_STEPS)
    }
    pub fn loose(&self) -> bool {
        self.tight.iter().all(|&t| t == 0)
    }
}

/// Синонимы: имя свободной детали -> ключ места.
fn alias(k: &str) -> &str {
    match k {
        "battery0" => "battery",
        "oilfilter0" => "oilfilter",
        "stocksteeringwheel" | "gtsteeringwheel" => "steeringwheel",
        "rockercovergt" => "rockercover",
        "dashboardmeters" => "meters",
        "clockgauge" => "clock",
        "subwooferpanel" => "panelsubwoofer",
        "backpanel" => "panelstock",
        "rearlightleft" => "rearlight",
        "rearlightright" => "rearlight",
        k if k.starts_with("wheelsteel") || k.starts_with("wheelgt") => "wheel",
        k => k,
    }
}

const SIDES: &[&str] = &["", "fl", "fr", "rl", "rr", "l", "r", "left", "right", "1", "2", "3", "4"];

/// Подходит ли деталь с ключом `part` к месту с ключом `slot`.
pub fn matches(part: &str, slot: &str) -> bool {
    let p = alias(part);
    SIDES.iter().any(|s| slot.len() == p.len() + s.len() && slot.starts_with(p) && slot.ends_with(s))
}

/// Ключ места по имени узла (без "pivot_"/"trigger_", особые случаи).
fn slot_key_of(name: &str) -> Option<String> {
    let l = name.to_lowercase();
    if l == "carmotorpivot" {
        return Some("block".into());
    }
    if l == "pivot_wheel_standard" {
        return Some("wheel".into());
    }
    if let Some(rest) = l.strip_prefix("pivot_") {
        return Some(norm_key(rest));
    }
    None
}

fn trigger_key_of(name: &str) -> Option<String> {
    let l = name.to_lowercase();
    if l.starts_with("triggerwheel") {
        return Some("wheel".into());
    }
    if l.starts_with("trigger_motor") {
        return Some("block".into());
    }
    l.strip_prefix("trigger_").map(norm_key)
}

/// Локальный бокс коллайдеров узла (в координатах узла).
fn collider_box(world: &World, i: usize, triggers: bool) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for c in &world.nodes[i].colliders {
        if c.trigger != triggers {
            continue;
        }
        let (a, b) = match c.shape {
            Shape::Box { center, size } => (center - size * 0.5, center + size * 0.5),
            Shape::Sphere { center, radius } => (center - Vec3::splat(radius), center + Vec3::splat(radius)),
            Shape::Capsule { center, radius, height, .. } => (center - Vec3::splat(height.max(radius * 2.0) * 0.5), center + Vec3::splat(height.max(radius * 2.0) * 0.5)),
            Shape::Mesh { .. } => (Vec3::splat(-0.2), Vec3::splat(0.2)),
        };
        lo = lo.min(a);
        hi = hi.max(b);
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

fn descendants(world: &World, root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        out.push(i);
        stack.extend_from_slice(&world.children[i]);
    }
    out
}

/// Болты в поддереве (без входа во вложенные установленные копии/детали).
fn bolts_under(world: &World, root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        if world.nodes[i].name == "BoltPM" {
            out.push(i);
            continue;
        }
        for &c in &world.children[i] {
            let n = &world.nodes[c].name;
            // Вложенные копии других деталей и группы "Masked<Деталь>" —
            // это болты других деталей.
            if c != root && ((n.contains('(') && !n.contains("(Clone)")) || n.starts_with("Masked")) {
                continue;
            }
            stack.push(c);
        }
    }
    out
}

/// Регулировки двигателя (в оригинале — винт карбюратора, поворот
/// трамблёра, гайки коромысел): заводится и тянет, только когда близко к
/// норме. Значения — в долях от нормы: 0 — идеально.
pub struct Tuning {
    pub mixture: f32,
    pub timing: f32,
    pub valves: f32,
}

pub struct Assembly {
    pub slots: Vec<Slot>,
    pub body_slot: HashMap<usize, usize>,
    pub satsuma: Option<usize>,
    /// Узел тела -> индекс тела.
    node_body: HashMap<usize, usize>,
    pub tuning: Tuning,
    /// Заряд аккумулятора 0..1.
    pub battery: f32,
    pub engine_on: bool,
    pub ignition: bool,
    pub crank: f32,
}

/// Без чего двигатель Satsuma не заведётся (ключи мест).
const ENGINE_PARTS: &[&str] = &[
    "block", "crankshaft", "mainbearing1", "mainbearing2", "mainbearing3", "piston1", "piston2", "piston3", "piston4",
    "headgasket", "cylinderhead", "camshaft", "camshaftgear", "timingchain", "timingcover", "rockershaft", "oilpan",
    "flywheel", "clutchdisc", "clutchpressureplate", "clutchcoverplate", "gearbox", "starter", "alternator",
    "distributor", "carburator", "fuelpump", "electrics", "battery", "fueltank", "crankshaftpulley", "waterpump",
    "enginepl", "drivegear",
];
/// Без чего Satsuma не поедет (кроме двигателя).
const DRIVE_PARTS: &[&str] = &["subframe", "halfshaft", "wheel", "gearlinkage", "steeringrack"];

impl Assembly {
    pub fn new(world: &mut World, physics: &mut Physics, satsuma: Option<usize>) -> Assembly {
        // Ключи всех свободных деталей: только из ящиков с запчастями и магазина
        // (иначе "DoorLeft" ворот гаража совпадал бы с местом двери машины).
        let part_keys: Vec<String> = physics.bodies.iter().filter(|b| is_part_root(&world.nodes[b.node].root)).map(|b| b.key.clone()).collect();
        let is_part = |k: &str| part_keys.iter().any(|p| matches(p, k));
        let mut hosts: Vec<usize> = physics.bodies.iter().map(|b| b.node).collect();
        hosts.extend(satsuma);
        let mut slots = Vec::new();
        for &host in &hosts {
            let nodes = descendants(world, host);
            let mut copy_keys = Vec::new();
            for &i in &nodes {
                let name = &world.nodes[i].name;
                if i == host || !name.contains('(') || name.contains("(Clone)") || !world.nodes[i].dynamic {
                    continue;
                }
                let k = norm_key(name);
                if k.is_empty() || !is_part(&k) {
                    continue;
                }
                copy_keys.push(k.clone());
                slots.push(Slot { key: k, target: i, mode: Mode::Copy, trigger: None, host, bolts: bolts_under(world, i), base_bolts: Vec::new(), tight: Vec::new(), installed: None, extra: Quat::IDENTITY });
            }
            for &i in &nodes {
                let Some(k) = slot_key_of(&world.nodes[i].name) else { continue };
                if !is_part(&k) || copy_keys.contains(&k) {
                    continue;
                }
                slots.push(Slot { key: k, target: i, mode: Mode::Pivot, trigger: None, host, bolts: Vec::new(), base_bolts: Vec::new(), tight: Vec::new(), installed: None, extra: Quat::IDENTITY });
            }
            // Зоны: триггеры этого хозяина.
            let triggers: Vec<(usize, String)> = nodes
                .iter()
                .filter_map(|&i| trigger_key_of(&world.nodes[i].name).map(|k| (i, k)))
                .filter(|(i, _)| collider_box(world, *i, true).is_some())
                .collect();
            for s in slots.iter_mut().filter(|s| s.host == host) {
                if let Some(b) = collider_box(world, s.target, true) {
                    s.trigger = Some((s.target, b.0, b.1));
                    continue;
                }
                let tp = world.global[s.target].w_axis.truncate();
                let best = triggers
                    .iter()
                    .filter(|(_, k)| k == &s.key || matches(k, &s.key) || matches(&s.key, k))
                    .min_by(|a, b| (world.global[a.0].w_axis.truncate() - tp).length().total_cmp(&(world.global[b.0].w_axis.truncate() - tp).length()));
                if let Some(&(t, _)) = best {
                    let b = collider_box(world, t, true).unwrap();
                    s.trigger = Some((t, b.0, b.1));
                }
            }
        }
        // Болты деталей "в точку": в самой детали ("Bolts") и группы
        // "Masked<Деталь>" у основы.
        for s in slots.iter_mut().filter(|s| s.mode == Mode::Pivot) {
            let masked: Vec<usize> = descendants(world, s.host)
                .into_iter()
                .filter(|&i| {
                    let n = world.nodes[i].name.to_lowercase();
                    let Some(m) = n.strip_prefix("masked") else { return false };
                    let m = norm_key(m);
                    s.key.starts_with(&m) || (m == "cylhead" && s.key == "cylinderhead") || (m.starts_with("bearing") && s.key == format!("main{}", m))
                })
                .collect();
            for m in masked {
                s.bolts.extend(bolts_under(world, m));
            }
        }
        for s in &mut slots {
            s.base_bolts = s.bolts.clone();
            s.tight = vec![0; s.bolts.len()];
        }

        let mut a = Assembly {
            slots,
            body_slot: HashMap::new(),
            satsuma,
            node_body: physics.bodies.iter().enumerate().map(|(i, b)| (b.node, i)).collect(),
            tuning: Tuning { mixture: 0.6, timing: -0.5, valves: 0.7 },
            battery: 0.9,
            engine_on: false,
            ignition: false,
            crank: 0.0,
        };
        // Копии на машине прячем: что стоит, решает сборка.
        for s in &a.slots {
            if s.mode == Mode::Copy {
                world.set_active(s.target, false);
            }
        }
        // Заводское: свободная деталь выключена в сцене, а место для неё есть.
        for bi in 0..physics.bodies.len() {
            let node = physics.bodies[bi].node;
            if world.nodes[node].active {
                continue;
            }
            let key = physics.bodies[bi].key.clone();
            if !is_part_root(&world.nodes[node].root) {
                continue;
            }
            if let Some(si) = (0..a.slots.len()).find(|&si| a.slots[si].installed.is_none() && matches(&key, &a.slots[si].key)) {
                a.install(world, physics, bi, si);
                a.slots[si].tight.iter_mut().for_each(|t| *t = BOLT_STEPS);
            }
        }
        let copies = a.slots.iter().filter(|s| s.mode == Mode::Copy).count();
        println!(
            "[MSC] сборка: мест {} (копий {}, точек {}), болтов {}, заводских деталей {}",
            a.slots.len(),
            copies,
            a.slots.len() - copies,
            a.slots.iter().map(|s| s.bolts.len()).sum::<usize>(),
            a.body_slot.len()
        );
        a
    }

    /// Свободное место для детали, в зоне которого её центр.
    pub fn slot_for(&self, world: &World, physics: &Physics, bi: usize) -> Option<usize> {
        let b = &physics.bodies[bi];
        (0..self.slots.len()).find(|&si| {
            let s = &self.slots[si];
            if s.installed.is_some() || !is_part_root(&world.nodes[b.node].root) || !matches(&b.key, &s.key) || !world.visible[s.host] || s.host == b.node {
                return false;
            }
            match s.trigger {
                Some((t, lo, hi)) => {
                    let q = world.global[t].inverse().transform_point3(b.com);
                    let m = Vec3::splat(0.12);
                    q.cmpge(lo - m).all() && q.cmple(hi + m).all()
                }
                None => (world.global[s.target].w_axis.truncate() - b.com).length() < 0.45,
            }
        })
    }

    pub fn install(&mut self, world: &mut World, physics: &mut Physics, bi: usize, si: usize) {
        let s = &mut self.slots[si];
        s.installed = Some(bi);
        if s.mode == Mode::Pivot {
            // Болты самой детали ("Bolts/BoltPM" в ней) — тоже держат её на месте.
            s.bolts = s.base_bolts.clone();
            s.bolts.extend(bolts_under(world, physics.bodies[bi].node));
        }
        s.tight = vec![0; s.bolts.len()];
        self.body_slot.insert(bi, si);
        let b = &mut physics.bodies[bi];
        b.attached = true;
        b.held = false;
        b.awake = false;
        b.vel = Vec3::ZERO;
        b.ang = Vec3::ZERO;
        match s.mode {
            Mode::Copy => {
                world.set_active(b.node, false);
                // Копия и все её выключенные предки (например, "halfshaft_fl").
                let mut i = s.target;
                loop {
                    world.nodes[i].active = true;
                    match world.nodes[i].parent {
                        Some(p) if p != s.host => i = p,
                        _ => break,
                    }
                }
                world.update_subtree(s.host);
            }
            Mode::Pivot => {
                world.set_active(b.node, true);
                if s.key.starts_with("door") {
                    // Закрытая дверь лежит вдоль борта: петля спереди, центр
                    // створки позади неё и почти не отходит вбок от петли.
                    let t = world.global[s.target];
                    let (_, rot, pos) = t.to_scale_rotation_translation();
                    let up_local = rot.inverse() * Vec3::Y;
                    let axis = [Vec3::X, Vec3::Y, Vec3::Z].into_iter().max_by(|a, b| a.dot(up_local).abs().total_cmp(&b.dot(up_local).abs())).unwrap();
                    let (_, car_rot, _) = world.global[s.host].to_scale_rotation_translation();
                    // Вперёд у моделей Unity — +Z, после зеркала — -Z.
                    let fwd = car_rot * Vec3::NEG_Z;
                    let right = fwd.cross(Vec3::Y).normalize_or_zero();
                    let scale = world.global[b.node].to_scale_rotation_translation().0;
                    let mut best = (f32::MAX, Quat::IDENTITY);
                    for deg in [0.0f32, 90.0, -90.0, 180.0] {
                        let q = Quat::from_axis_angle(axis, deg.to_radians());
                        b.set_from_node_matrix(Mat4::from_scale_rotation_translation(scale, rot * q, pos));
                        let off = b.com - pos;
                        let behind = -off.dot(fwd);
                        let score = off.dot(right).abs() + if behind > 0.1 { 0.0 } else { 10.0 };
                        if score < best.0 {
                            best = (score, q);
                        }
                    }
                    s.extra = best.1;
                }
            }
        }
        self.follow(world, physics);
    }

    /// Снять деталь (в руки). Нельзя, пока на ней стоят другие детали.
    pub fn remove(&mut self, world: &mut World, physics: &mut Physics, si: usize) -> Option<usize> {
        let bi = self.slots[si].installed?;
        let node = physics.bodies[bi].node;
        if self.slots.iter().any(|s| s.host == node && s.installed.is_some()) {
            return None;
        }
        let m = world.global[self.slots[si].target];
        let s = &mut self.slots[si];
        s.installed = None;
        self.body_slot.remove(&bi);
        if s.mode == Mode::Copy {
            world.set_active(s.target, false);
        }
        world.set_active(node, true);
        let b = &mut physics.bodies[bi];
        b.attached = false;
        b.set_from_node_matrix(Mat4::from_scale_rotation_translation(world.global[node].to_scale_rotation_translation().0, m.to_scale_rotation_translation().1, m.w_axis.truncate()));
        b.wake();
        world.set_world_matrix(node, b.node_matrix());
        Some(bi)
    }

    /// Установленные детали едут за своими местами (машина, деталь-основа).
    pub fn follow(&self, world: &mut World, physics: &mut Physics) {
        for s in &self.slots {
            let Some(bi) = s.installed else { continue };
            let t = world.global[s.target];
            let node = physics.bodies[bi].node;
            let scale = world.global[node].to_scale_rotation_translation().0;
            let (_, rot, pos) = t.to_scale_rotation_translation();
            let m = Mat4::from_scale_rotation_translation(scale, rot * s.extra, pos);
            physics.bodies[bi].set_from_node_matrix(m);
            if s.mode == Mode::Pivot {
                world.set_world_matrix(node, m);
            }
        }
    }

    /// Болт на луче: (место, номер болта, расстояние).
    pub fn pick_bolt(&self, world: &World, o: Vec3, d: Vec3, max: f32) -> Option<(usize, usize, f32)> {
        let mut best: Option<(usize, usize, f32)> = None;
        for (si, s) in self.slots.iter().enumerate() {
            if s.installed.is_none() {
                continue;
            }
            for (k, &b) in s.bolts.iter().enumerate() {
                let c = world.global[b].w_axis.truncate();
                let t = (c - o).dot(d);
                if t < 0.0 || t > max {
                    continue;
                }
                let miss = (o + d * t - c).length();
                if miss < 0.035 && best.is_none_or(|x| t < x.2) {
                    best = Some((si, k, t));
                }
            }
        }
        best
    }

    /// Установленная деталь на луче: (место, расстояние).
    pub fn pick_installed(&self, physics: &Physics, o: Vec3, d: Vec3, max: f32) -> Option<(usize, f32)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(si, s)| {
                let b = &physics.bodies[s.installed?];
                let inv = b.rot.inverse();
                ray_box(inv * (o - b.com), inv * d, -b.half, b.half).filter(|&t| t <= max).map(|t| (si, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Стоит ли и затянута деталь с этим ключом на Satsuma (или на её двигателе).
    fn has(&self, key: &str) -> bool {
        self.slots.iter().any(|s| s.fixed() && (s.key == key || s.key.starts_with(key)) && self.on_satsuma(s))
    }

    fn on_satsuma(&self, s: &Slot) -> bool {
        let Some(sat) = self.satsuma else { return false };
        // Хозяин — сама машина или деталь, которая (через цепочку) стоит на ней.
        let mut host = s.host;
        for _ in 0..12 {
            if host == sat {
                return true;
            }
            let Some(&bi) = self.node_body.get(&host) else { return false };
            let Some(&si) = self.body_slot.get(&bi) else { return false };
            host = self.slots[si].host;
        }
        false
    }

    /// Чего не хватает для запуска двигателя / езды.
    pub fn missing(&self, drive: bool) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = ENGINE_PARTS.iter().copied().filter(|k| !self.has(k)).collect();
        if drive {
            out.extend(DRIVE_PARTS.iter().copied().filter(|k| !self.has(k)));
            let wheels = self.slots.iter().filter(|s| s.key == "wheel" && s.fixed()).count();
            if wheels < 4 && !out.contains(&"wheel") {
                out.push("wheel");
            }
        }
        out
    }

    /// Качество работы двигателя 0..1 по регулировкам (1 — идеально).
    pub fn engine_quality(&self) -> f32 {
        let t = &self.tuning;
        let bad = t.mixture.abs() * 0.8 + t.timing.abs() + t.valves.abs() * 0.6;
        (1.0 - bad * 0.5).clamp(0.0, 1.0)
    }

    /// Отладка (MSC_AUTOBUILD): поставить и затянуть всё, что подходит —
    /// каждую свободную деталь подносят к её месту через `slot_for`, как
    /// игрок. Несколько проходов: детали ставятся на детали.
    pub fn auto_build(&mut self, world: &mut World, physics: &mut Physics) -> usize {
        let mut count = 0;
        for _ in 0..8 {
            let mut progress = false;
            for bi in 0..physics.bodies.len() {
                if physics.bodies[bi].attached || !world.visible[physics.bodies[bi].node] {
                    continue;
                }
                let key = physics.bodies[bi].key.clone();
                let saved = (physics.bodies[bi].com, physics.bodies[bi].rot);
                let mut placed = false;
                for si in 0..self.slots.len() {
                    let s = &self.slots[si];
                    if s.installed.is_some() || !matches(&key, &s.key) || !world.visible[s.host] || s.host == physics.bodies[bi].node {
                        continue;
                    }
                    // Поднести центр детали к зоне места.
                    let p = match s.trigger {
                        Some((t, lo, hi)) => world.global[t].transform_point3((lo + hi) * 0.5),
                        None => world.global[s.target].w_axis.truncate(),
                    };
                    physics.bodies[bi].com = p;
                    if self.slot_for(world, physics, bi) == Some(si) {
                        self.install(world, physics, bi, si);
                        self.slots[si].tight.iter_mut().for_each(|t| *t = BOLT_STEPS);
                        count += 1;
                        placed = true;
                        progress = true;
                        break;
                    }
                }
                if !placed {
                    (physics.bodies[bi].com, physics.bodies[bi].rot) = saved;
                }
            }
            self.follow(world, physics);
            if !progress {
                break;
            }
        }
        count
    }

    /// Регулировка детали по ключу: колесо мыши крутит винт/трамблёр/гайки.
    pub fn adjust(&mut self, key: &str, wheel: f32) -> Option<String> {
        let step = wheel * 0.05;
        let (v, label) = if key.starts_with("carburator") {
            (&mut self.tuning.mixture, "смесь")
        } else if key.starts_with("distributor") {
            (&mut self.tuning.timing, "опережение зажигания")
        } else if key.starts_with("rockershaft") {
            (&mut self.tuning.valves, "зазор клапанов")
        } else {
            return None;
        };
        *v = (*v + step).clamp(-1.5, 1.5);
        Some(format!("{}: {:+.2}", label, v))
    }
}
