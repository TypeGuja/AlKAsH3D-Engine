//! Обобщённая малолитражка (не конкретная модель) для расчёта удара: кузов из
//! силовых элементов и обшивки на `softbody`. Длина 4.2 м, ширина 1.76 м, масса
//! около 1250 кг. Сечения балок заданы размерами и марками стали, типичными для
//! таких машин, поэтому результат — оценка поведения машины такого класса, а не
//! прогноз для конкретного автомобиля.
//!
//! Оси: x — вперёд (нос при x = 4.2), y — вверх, z — вбок.

use crate::softbody::*;
use std::collections::HashMap;

const NI: i32 = 21;
const NJ: i32 = 6;
const NK: i32 = 8;
const DX: f64 = 0.2;
/// Щит между моторным отсеком и салоном.
const I_FIREWALL: i32 = 15;
/// Средняя стойка.
const I_B: i32 = 9;
/// Задняя стойка.
const I_C: i32 = 5;
/// Перегородка за задним сиденьем.
const I_REAR: i32 = 4;
/// Высота лини окон в узлах сетки.
const J_BELT: i32 = 4;

/// Куда относится грань обшивки (для показа).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Paint,
    Glass,
    Underbody,
}

/// Четырёхугольник обшивки: узлы по кругу и балки его сторон.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub nodes: [u32; 4],
    pub beams: [u32; 4],
    pub part: Part,
}

#[derive(Clone, Copy, Debug)]
pub struct Wheel {
    pub node: u32,
    pub front: bool,
}

pub struct Car {
    pub body: SoftBody,
    pub faces: Vec<Face>,
    pub wheels: Vec<Wheel>,
    /// Узлы пола позади средней стойки: тут стоит «датчик» замедления, этот участок
    /// не сминается при лобовом ударе.
    pub sensor: Vec<u32>,
    /// Узлы щита — по ним видно, насколько моторный отсек вдавило в салон.
    pub firewall: Vec<u32>,
    /// Пары узлов вдоль зада машины, задняя стенка — перегородка.
    pub rear_lines: Vec<(u32, u32)>,
    /// Самый передний ряд.
    pub nose_x: f64,
    pub mass: f64,
}

fn roof_height(x: f64) -> f64 {
    let lerp = |x0: f64, y0: f64, x1: f64, y1: f64| y0 + (y1 - y0) * ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
    if x < 0.2 {
        lerp(0.0, 0.82, 0.2, 1.0)
    } else if x < 0.9 {
        lerp(0.2, 1.0, 0.9, 1.04)
    } else if x < 1.4 {
        lerp(0.9, 1.04, 1.4, 1.42)
    } else if x < 2.6 {
        1.44
    } else if x < 3.2 {
        lerp(2.6, 1.44, 3.2, 1.0)
    } else if x < 4.0 {
        lerp(3.2, 1.0, 4.0, 0.84)
    } else {
        lerp(4.0, 0.84, 4.2, 0.68)
    }
}

fn half_width(x: f64, y: f64) -> f64 {
    let mut w: f64 = 0.88;
    if x > 3.8 {
        w -= (x - 3.8) * 0.3;
    }
    if x < 0.3 {
        w -= (0.3 - x) * 0.25;
    }
    if y > 1.0 {
        w -= (y - 1.0) * 0.55;
    }
    w
}

fn grid_pos(i: i32, j: i32, k: i32) -> V3 {
    let x = i as f64 * DX;
    let y = 0.22 + (roof_height(x) - 0.22) * j as f64 / NJ as f64;
    let z = (k as f64 / NK as f64 * 2.0 - 1.0) * half_width(x, y);
    v3(x, y, z)
}

fn bulkhead(i: i32) -> bool {
    i == I_FIREWALL || i == I_REAR
}

fn node_exists(i: i32, j: i32, k: i32) -> bool {
    i == 0 || i == NI || j == 0 || j == NJ || k == 0 || k == NK || bulkhead(i)
}

/// Силовое сечение ребра сетки (i, j, k)-(i2, j2, k2). None — обычный лист.
fn structural(a: (i32, i32, i32), b: (i32, i32, i32)) -> Option<(Section, f64)> {
    let along_i = a.1 == b.1 && a.2 == b.2 && a.0 != b.0;
    let along_j = a.0 == b.0 && a.2 == b.2 && a.1 != b.1;
    let along_k = a.0 == b.0 && a.1 == b.1 && a.2 != b.2;
    let (i, j, k) = (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2));
    let side = k == 0 || k == NK;
    // момент пластического шарнира тонкостенной трубы ~ 1.5 sy b^2 t
    let tube = |b: f64, t: f64, sy: f64, share: f64| {
        let mut s = Section::square_tube(b, t, sy);
        s.ea *= share;
        s.yield_tension *= share;
        s.hardening *= share;
        s.peak_compression *= share;
        s.plateau_compression *= share;
        (s, 1.5 * sy * b * b * t * share)
    };
    if along_i {
        // крэш-боксы перед лонжеронами: сминаются первыми при слабом ударе
        if i >= NI - 1 && (j == 0 || j == 1) && (k == 2 || k == 6) {
            return Some(tube(0.07, 0.0013, 350e6, 0.5));
        }
        // лонжероны моторного отсека: две линии на одну трубу
        if i >= I_FIREWALL && (j == 0 || j == 1) && (k == 2 || k == 6) {
            return Some(tube(0.09, 0.0016, 420e6, 0.5));
        }
        // подрамник снизу
        if i >= I_FIREWALL && j == 0 && (k == 1 || k == 7) {
            return Some(tube(0.06, 0.0015, 350e6, 1.0));
        }
        // верхний путь нагрузки: усилитель крыла над моторным отсеком
        if i >= I_FIREWALL && j == NJ && side {
            return Some(tube(0.06, 0.0012, 350e6, 1.0));
        }
        // продолжение лонжеронов под полом салона
        if i >= I_REAR && i < I_FIREWALL && j == 0 && (k == 2 || k == 6) {
            return Some(tube(0.08, 0.0016, 500e6, 1.0));
        }
        // пороги: самый прочный продольный элемент салона
        if i >= I_REAR && i < I_FIREWALL && j == 0 && side {
            return Some(tube(0.12, 0.0018, 700e6, 1.0));
        }
        // задние лонжероны
        if i < I_REAR && j == 0 && (k == 2 || k == 6) {
            return Some(tube(0.08, 0.0014, 350e6, 1.0));
        }
        // усилители задних крыльев: зад несёт свою массу при торможении
        if i < I_C && j == NJ && side {
            return Some(tube(0.06, 0.0012, 350e6, 1.0));
        }
        if i < I_REAR && j == 0 && side {
            return Some(tube(0.08, 0.0012, 350e6, 1.0));
        }
        // рама крыши и передние стойки (идут по верхней грани сетки)
        if i >= I_C && i < I_FIREWALL + 1 && j == NJ && side {
            return Some(tube(0.07, 0.0015, 700e6, 1.0));
        }
    }
    if along_j {
        // средняя стойка, задняя стойка, щитовая стойка
        if side && (i == I_B || i == I_C || i == I_FIREWALL) {
            return Some(tube(0.09, 0.0016, 700e6, 1.0));
        }
    }
    if along_k {
        // передний бампер: балка усилителя
        if i == NI && (j == 1 || j == 2) {
            return Some(tube(0.07, 0.002, 700e6, 0.5));
        }
        if i == 0 && j == 1 {
            return Some(tube(0.06, 0.0015, 500e6, 1.0));
        }
        // поперечины пола и дуги крыши
        if j == 0 && (i == I_FIREWALL || i == I_B || i == I_C || i == I_REAR) {
            return Some(tube(0.07, 0.0014, 500e6, 1.0));
        }
        if j == NJ && (i == I_B || i == I_C || i == 13) {
            return Some(tube(0.05, 0.0012, 500e6, 1.0));
        }
        if j == NJ && i == I_FIREWALL {
            return Some(tube(0.06, 0.0014, 500e6, 1.0));
        }
    }
    None
}

/// Грань сетки — стекло: лобовое, заднее и боковые окна.
fn is_glass(c: [(i32, i32, i32); 4]) -> bool {
    let (mut i0, mut i1, mut j0, mut j1, mut k0, mut k1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for (i, j, k) in c {
        i0 = i0.min(i);
        i1 = i1.max(i);
        j0 = j0.min(j);
        j1 = j1.max(j);
        k0 = k0.min(k);
        k1 = k1.max(k);
    }
    let top = j0 == NJ && j1 == NJ;
    let side = (k0 == 0 && k1 == 0) || (k0 == NK && k1 == NK);
    if top && (13..16).contains(&i0) && i1 <= 16 {
        return true;
    }
    if top && (I_REAR + 1..7).contains(&i0) && i1 <= 7 {
        return true;
    }
    if side && j0 >= J_BELT && i0 >= I_C && i1 <= 14 {
        return !(i0 == I_B - 1 && false);
    }
    false
}

/// Собрать машину: передний край при x = nose_x, колёса на земле y = 0.
pub fn build_car(dt: f64) -> Car {
    let mut s = SoftBody::new(dt);
    s.ground_y = Some(0.0);
    let mut nodes: HashMap<(i32, i32, i32), u32> = HashMap::new();

    // обшивка и перегородки
    const SHELL_MASS: f64 = 720.0;
    let mut coords = Vec::new();
    for i in 0..=NI {
        for j in 0..=NJ {
            for k in 0..=NK {
                if node_exists(i, j, k) {
                    coords.push((i, j, k));
                }
            }
        }
    }
    // узлы лонжеронов внутри моторного отсека
    let rail_inner: Vec<(i32, i32, i32)> = (I_FIREWALL..=NI).flat_map(|i| [(i, 1, 2), (i, 1, 6)]).filter(|c| !node_exists(c.0, c.1, c.2)).collect();
    let shell_each = SHELL_MASS / (coords.len() + rail_inner.len()) as f64;
    for &c in coords.iter().chain(rail_inner.iter()) {
        let id = s.add_node(grid_pos(c.0, c.1, c.2), shell_each);
        nodes.insert(c, id);
        s.self_radius[id as usize] = 0.035;
    }

    // балки: каждая грань даёт стороны и диагонали
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    let mut faces = Vec::new();
    let sheet = Section::sheet(0.2, 0.0008, 220e6);
    let sheet_diag = Section::sheet(0.28, 0.0006, 220e6);
    let plate = Section::sheet(0.2, 0.0015, 300e6);
    let glass = Section::glass(0.2, 0.0045);
    let mut edge_beam = |s: &mut SoftBody, edges: &mut HashMap<(u32, u32), u32>, a: u32, b: u32, section: Section| -> u32 {
        let key = (a.min(b), a.max(b));
        *edges.entry(key).or_insert_with(|| s.add_beam(a, b, section))
    };

    let mut add_face = |s: &mut SoftBody, edges: &mut HashMap<(u32, u32), u32>, faces: &mut Vec<Face>, c: [(i32, i32, i32); 4], visible: bool, base: Section, part: Part| {
        let ids = c.map(|p| nodes[&p]);
        let glassy = is_glass(c);
        let mut beams = [0u32; 4];
        for e in 0..4 {
            let (p, q) = (c[e], c[(e + 1) % 4]);
            let section = match structural(p, q) {
                Some((sec, _)) => sec,
                None => {
                    if glassy {
                        glass
                    } else {
                        base
                    }
                }
            };
            beams[e] = edge_beam(s, edges, ids[e], ids[(e + 1) % 4], section);
        }
        let diag = if glassy { glass } else { sheet_diag };
        edge_beam(s, edges, ids[0], ids[2], diag);
        edge_beam(s, edges, ids[1], ids[3], diag);
        if visible {
            faces.push(Face { nodes: ids, beams, part: if glassy { Part::Glass } else { part } });
        }
    };

    // передняя и задняя стенки, перегородки
    for i in 0..=NI {
        if i != 0 && i != NI && !bulkhead(i) {
            continue;
        }
        for j in 0..NJ {
            for k in 0..NK {
                let c = [(i, j, k), (i, j + 1, k), (i, j + 1, k + 1), (i, j, k + 1)];
                let c = if i == 0 { [c[0], c[3], c[2], c[1]] } else { c };
                let base = if bulkhead(i) { plate } else { sheet };
                add_face(&mut s, &mut edges, &mut faces, c, !bulkhead(i), base, Part::Paint);
            }
        }
    }
    // низ и верх
    for i in 0..NI {
        for k in 0..NK {
            add_face(&mut s, &mut edges, &mut faces, [(i, 0, k), (i, 0, k + 1), (i + 1, 0, k + 1), (i + 1, 0, k)], true, sheet, Part::Underbody);
            add_face(&mut s, &mut edges, &mut faces, [(i, NJ, k), (i + 1, NJ, k), (i + 1, NJ, k + 1), (i, NJ, k + 1)], true, sheet, Part::Paint);
        }
    }
    // борта
    for i in 0..NI {
        for j in 0..NJ {
            add_face(&mut s, &mut edges, &mut faces, [(i, j, 0), (i + 1, j, 0), (i + 1, j + 1, 0), (i, j + 1, 0)], true, sheet, Part::Paint);
            add_face(&mut s, &mut edges, &mut faces, [(i, j, NK), (i, j + 1, NK), (i + 1, j + 1, NK), (i + 1, j, NK)], true, sheet, Part::Paint);
        }
    }

    // лонжероны внутри отсека: линия j = 1 и связи с нижней линией и щитом
    let rail = structural((I_FIREWALL, 0, 2), (I_FIREWALL + 1, 0, 2)).unwrap().0;
    for k in [2, 6] {
        for i in I_FIREWALL..NI {
            let a = nodes[&(i, 1, k)];
            let b = nodes[&(i + 1, 1, k)];
            edge_beam(&mut s, &mut edges, a, b, rail);
            // раскосы коробчатой балки
            let low = nodes[&(i, 0, k)];
            let low2 = nodes[&(i + 1, 0, k)];
            edge_beam(&mut s, &mut edges, a, low, sheet);
            edge_beam(&mut s, &mut edges, a, low2, sheet_diag);
        }
        let a = nodes[&(NI, 1, k)];
        let low = nodes[&(NI, 0, k)];
        edge_beam(&mut s, &mut edges, a, low, sheet);
    }

    // шарниры: все прямые линии сетки держат изгиб
    let mut hinge_list = Vec::new();
    for (&c, &m) in &nodes {
        for d in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
            let p = (c.0 - d.0, c.1 - d.1, c.2 - d.2);
            let q = (c.0 + d.0, c.1 + d.1, c.2 + d.2);
            let (Some(&a), Some(&b)) = (nodes.get(&p), nodes.get(&q)) else { continue };
            let (Some(&e0), Some(&e1)) = (edges.get(&(a.min(m), a.max(m))), edges.get(&(b.min(m), b.max(m)))) else { continue };
            let strong = structural(p, c).zip(structural(c, q));
            let (k, moment) = match strong {
                // упругая жёсткость трубы E*I/L, I ~ 2/3 b^3 t; ограничится устойчивостью шага
                Some(((_, m0), (_, m1))) => (1e6, m0.min(m1)),
                // панель выпуклая и со штамповкой: жёстче плоского листа той же толщины раз в десять
                None => (1500.0, 30.0),
            };
            hinge_list.push((a, m, b, k, moment, [e0, e1]));
        }
    }
    hinge_list.sort_by_key(|h| (h.0, h.1, h.2));
    for (a, m, b, k, moment, e) in hinge_list {
        s.add_hinge(a, m, b, k, moment, e);
    }

    // двигатель с коробкой: тяжёлый жёсткий блок на опорах
    let engine_center = v3(3.5, 0.56, 0.0);
    let half = [0.24, 0.17, 0.24];
    let mut engine = Vec::new();
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                let id = s.add_node(engine_center + v3(sx * half[0], sy * half[1], sz * half[2]), 180.0 / 8.0);
                s.self_radius[id as usize] = 0.09;
                engine.push(id);
            }
        }
    }
    let rigid = Section { ea: 2e9, yield_tension: 1e9, hardening: 0.0, peak_compression: 1e9, plateau_compression: 1e9, rupture_strain: 10.0, bottom_strain: 0.9, brittle: false };
    for a in 0..engine.len() {
        for b in a + 1..engine.len() {
            s.add_beam(engine[a], engine[b], rigid);
        }
    }
    // опоры двигателя: срываются при большой нагрузке
    let mount = Section { ea: 2e7, yield_tension: 15e3, hardening: 0.0, peak_compression: 15e3, plateau_compression: 8e3, rupture_strain: 0.3, bottom_strain: 0.7, brittle: false };
    for (&e, &(i, j, k)) in engine.iter().zip([(16, 1, 2), (16, 1, 6), (16, 3, 2), (16, 3, 6), (19, 1, 2), (19, 1, 6), (19, 3, 2), (19, 3, 6)].iter()) {
        let target = nodes.get(&(i, j, k)).copied().unwrap_or_else(|| nodes[&(i, 1, k)]);
        s.add_beam(e, target, mount);
    }

    // масса кузова — в основном в силовых элементах: лист весит мало и не должен
    // складываться от собственной инерции при торможении
    let has_structure = |c: (i32, i32, i32)| {
        [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)].iter().any(|d| {
            let q = (c.0 + d.0, c.1 + d.1, c.2 + d.2);
            nodes.contains_key(&q) && structural(c, q).is_some()
        })
    };
    const STRUCTURE_WEIGHT: f64 = 8.0;
    let weight_sum: f64 = nodes.keys().map(|&c| if has_structure(c) { STRUCTURE_WEIGHT } else { 1.0 }).sum();
    for (&c, &id) in &nodes {
        s.mass[id as usize] = SHELL_MASS * if has_structure(c) { STRUCTURE_WEIGHT } else { 1.0 } / weight_sum;
    }

    // пассажиры и салон: масса на полу
    let floor: Vec<u32> = (I_REAR..I_FIREWALL).flat_map(|i| (0..=NK).map(move |k| (i, 0, k))).map(|c| nodes[&c]).collect();
    let extra = 200.0 / floor.len() as f64;
    for &id in &floor {
        s.mass[id as usize] += extra;
    }

    // колёса: узел-шар на упругой подвеске
    let mut wheels = Vec::new();
    for (x, front) in [(3.55, true), (0.72, false)] {
        for side in [-1.0, 1.0] {
            let id = s.add_node(v3(x, 0.3, side * 0.76), 22.0);
            s.radius[id as usize] = 0.3;
            s.friction[id as usize] = 0.02;
            // с кузовом колесо не сталкивается: оно в арке, и шар такого радиуса
            // пересекал бы её узлы уже при сборке
            wheels.push(Wheel { node: id, front });
            let i = (x / DX).round() as i32;
            let kk = if side < 0.0 { 1 } else { NK - 1 };
            let edge = if side < 0.0 { 0 } else { NK };
            for c in [(i - 1, 0, kk), (i + 1, 0, kk), (i - 1, 2, edge), (i + 1, 2, edge), (i, 0, kk)] {
                let target = nodes[&c];
                let l = (s.pos[id as usize] - s.pos[target as usize]).len();
                // пружина подвески ~30 кН/м на колесо по вертикали (балки косые); рычаги рвутся при сильном ударе
                let spring = Section { ea: 4e4 * l, yield_tension: 12e3, hardening: 0.0, peak_compression: 18e3, plateau_compression: 12e3, rupture_strain: 3.0, bottom_strain: 0.8, brittle: false };
                s.add_beam(id, target, spring);
            }
        }
    }

    // трение и контакт обшивки
    for f in s.friction.iter_mut() {
        if *f == 0.5 {
            *f = 0.35;
        }
    }

    // балки обшивки, которые видит столб
    s.surface_beams = (0..s.beams.len() as u32).filter(|&b| {
        let beam = &s.beams[b as usize];
        beam.section.bottom_strain < 0.8 && beam.section.ea < 2e9
    }).collect();

    // щит шире в контакте: двигатель, сорвавшись с опор, не проходит между его узлами
    for j in 0..=NJ {
        for k in 0..=NK {
            s.self_radius[nodes[&(I_FIREWALL, j, k)] as usize] = 0.09;
        }
    }

    let sensor: Vec<u32> = (I_C..=I_B).flat_map(|i| (0..=NK).map(move |k| (i, 0, k))).map(|c| nodes[&c]).collect();
    let firewall: Vec<u32> = (0..=NJ).flat_map(|j| (0..=NK).map(move |k| (I_FIREWALL, j, k))).map(|c| nodes[&c]).collect();
    // пары «задняя стенка — перегородка»: по ним видно, не смялся ли зад при лобовом ударе
    let mut rear_lines = Vec::new();
    for j in 0..=NJ {
        for k in 0..=NK {
            if let (Some(&a), Some(&b)) = (nodes.get(&(0, j, k)), nodes.get(&(I_REAR, j, k))) {
                rear_lines.push((a, b));
            }
        }
    }
    let mass = s.mass.iter().sum();
    s.finalize(0.01);
    Car { body: s, faces, wheels, sensor, firewall, rear_lines, nose_x: NI as f64 * DX, mass }
}

/// Чем заканчивается удар.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Жёсткая стена поперёк дороги.
    Wall,
    /// Стена, перекрывающая `overlap` ширины машины (доля 0..1) со стороны z > 0.
    OffsetWall { overlap: f64 },
    /// Столб, смещение от оси машины, м.
    Pole { offset: f64 },
}

/// Итог расчёта для вывода и сверки.
#[derive(Clone, Debug, Default)]
pub struct CrashReport {
    pub speed_kmh: f64,
    pub mass: f64,
    pub kinetic_in: f64,
    /// Наибольшее перемещение пола после контакта: сколько смялся перед, м.
    pub dynamic_crush: f64,
    /// Остаточное сокращение от носа до датчика в конце, м.
    pub residual_crush: f64,
    /// Насколько щит подъехал к средней стойке, м.
    pub firewall_intrusion: f64,
    /// Наибольшее укорочение зада машины, м: при лобовом ударе он почти цел.
    pub rear_shortening: f64,
    pub peak_g: f64,
    pub mean_g: f64,
    pub stop_time: f64,
    pub rebound: f64,
    pub plastic: f64,
    pub damping: f64,
    pub friction: f64,
    pub rupture: f64,
    pub contact_damping: f64,
    pub kinetic_left: f64,
    pub elastic_left: f64,
    /// Расхождение баланса энергии, доля начальной кинетической.
    pub balance_error: f64,
    pub broken_beams: usize,
    pub total_beams: usize,
    pub broken_glass_faces: usize,
    /// Наибольшая скорость узла в конце, м/с: признак численной неустойчивости.
    pub max_node_speed: f64,
}

/// Запись удара для показа: положения узлов с частотой `frame_rate`.
pub struct CrashRecording {
    pub frame_rate: f64,
    /// По кадрам: x, y, z каждого узла подряд.
    pub frames: Vec<Vec<f32>>,
    /// Для каждой балки — номер кадра, с которого она порвана (u32::MAX — цела).
    pub beam_break_frame: Vec<u32>,
    pub g_by_frame: Vec<f32>,
    pub speed_by_frame: Vec<f32>,
    pub report: CrashReport,
    pub faces: Vec<Face>,
    pub wheels: Vec<Wheel>,
    /// Узлы датчика на полу (ряды от задних к передним): по ним видно курс машины.
    pub sensor: Vec<u32>,
    /// Препятствие в координатах расчёта.
    pub obstacle: Obstacle,
}

fn sensor_velocity(s: &SoftBody, sensor: &[u32]) -> f64 {
    let (mut p, mut m) = (0.0, 0.0);
    for &i in sensor {
        p += s.vel[i as usize].x * s.mass[i as usize];
        m += s.mass[i as usize];
    }
    p / m
}

fn sensor_x(s: &SoftBody, sensor: &[u32]) -> f64 {
    sensor.iter().map(|&i| s.pos[i as usize].x).sum::<f64>() / sensor.len() as f64
}

/// Рассчитать удар на эталоне Rust.
pub fn simulate(target: Target, speed_kmh: f64, duration: f64, frame_rate: f64, progress: impl FnMut(f64)) -> CrashRecording {
    simulate_with(None, target, speed_kmh, duration, frame_rate, progress)
}

/// Рассчитать удар. `kernel` — шаг на Fortran (None — эталон на Rust), `progress`
/// получает долю готовности 0..1.
pub fn simulate_with(kernel: Option<StepKernel>, target: Target, speed_kmh: f64, duration: f64, frame_rate: f64, mut progress: impl FnMut(f64)) -> CrashRecording {
    let dt = 5e-5;
    let mut car = build_car(dt);
    car.body.kernel = kernel;
    let wall_gap = 0.25;
    let wall_x = car.nose_x + wall_gap;
    let s = &mut car.body;

    // встать на подвеску: тот же расчёт с вязким гашением, до старта
    for _ in 0..(0.6 / dt) as usize {
        for v in s.vel.iter_mut() {
            *v = *v * 0.9985;
        }
        s.step();
    }
    s.ledger = EnergyLedger::default();
    for b in s.beams.iter_mut() {
        b.buckled = false;
    }

    let obstacle = match target {
        Target::Wall => Obstacle::Block { min: v3(wall_x, -1.0, -6.0), max: v3(wall_x + 1.0, 4.0, 6.0) },
        Target::OffsetWall { overlap } => {
            let edge = 0.88 - 2.0 * 0.88 * overlap;
            Obstacle::Block { min: v3(wall_x, -1.0, edge), max: v3(wall_x + 1.0, 4.0, 6.0) }
        }
        Target::Pole { offset } => Obstacle::Pole { x: wall_x + 0.15, z: offset, radius: 0.15, height: 4.0 },
    };
    s.obstacles.push(obstacle);

    let speed = speed_kmh / 3.6;
    for v in s.vel.iter_mut() {
        v.x += speed;
    }
    let start_energy = s.energy_total();
    let kinetic_in = s.kinetic_energy();
    let start_x = sensor_x(s, &car.sensor);
    let fw_start: Vec<V3> = car.firewall.iter().map(|&i| s.pos[i as usize]).collect();
    let sensor_center: Vec<V3> = car.sensor.iter().map(|&i| s.pos[i as usize]).collect();
    let rear_start: Vec<f64> = car.rear_lines.iter().map(|&(a, b)| (s.pos[a as usize] - s.pos[b as usize]).len()).collect();

    let steps = (duration / dt) as usize;
    let per_frame = ((1.0 / frame_rate) / dt).round().max(1.0) as usize;
    let mut rec = CrashRecording {
        frame_rate,
        frames: Vec::with_capacity(steps / per_frame + 1),
        beam_break_frame: vec![u32::MAX; s.beams.len()],
        g_by_frame: Vec::new(),
        speed_by_frame: Vec::new(),
        report: CrashReport::default(),
        faces: car.faces.clone(),
        wheels: car.wheels.clone(),
        sensor: car.sensor.clone(),
        obstacle,
    };
    let mut record = |s: &SoftBody, rec: &mut CrashRecording| {
        let mut f = Vec::with_capacity(s.pos.len() * 3);
        for p in &s.pos {
            f.push(p.x as f32);
            f.push(p.y as f32);
            f.push(p.z as f32);
        }
        rec.frames.push(f);
    };
    record(s, &mut rec);

    // датчик: скорость каждые 1 мс, ускорение — среднее за скользящие 10 мс (как низкочастотный фильтр)
    let mut velocity_history: Vec<(f64, f64)> = Vec::new();
    let mut peak_g = 0.0f64;
    let mut max_travel = 0.0f64;
    let mut stop_time = None;
    let contact_time: Option<f64> = None;
    let mut rebound = 0.0f64;
    let mut min_sensor_velocity = speed;
    for n in 1..=steps {
        s.step();
        if n % per_frame == 0 {
            let frame = rec.frames.len() as u32;
            for (b, slot) in s.beams.iter().zip(rec.beam_break_frame.iter_mut()) {
                if b.broken && *slot == u32::MAX {
                    *slot = frame;
                }
            }
            record(s, &mut rec);
            let v = sensor_velocity(s, &car.sensor);
            velocity_history.push((s.time, v));
            let g = if velocity_history.len() > 1 {
                let back = velocity_history.len().saturating_sub((0.01 * frame_rate).round().max(1.0) as usize + 1);
                let (t0, v0) = velocity_history[back];
                ((v0 - v) / (s.time - t0).max(1e-6)) / 9.81
            } else {
                0.0
            };
            rec.g_by_frame.push(g as f32);
            rec.speed_by_frame.push(v as f32);
            peak_g = peak_g.max(g);
            if stop_time.is_none() && v <= 0.0 {
                stop_time = Some(s.time);
            }
            min_sensor_velocity = min_sensor_velocity.min(v);
            rebound = rebound.max(-v);
            max_travel = max_travel.max(sensor_x(s, &car.sensor) - start_x);
            progress(n as f64 / steps as f64);
        }
    }

    let end_x = sensor_x(s, &car.sensor);
    let fw_now: f64 = car.firewall.iter().map(|&i| s.pos[i as usize].x).sum::<f64>() / car.firewall.len() as f64;
    let fw_was: f64 = fw_start.iter().map(|p| p.x).sum::<f64>() / fw_start.len() as f64;
    let sensor_was: f64 = sensor_center.iter().map(|p| p.x).sum::<f64>() / sensor_center.len() as f64;
    // нос — только по целым узлам с невысокой скоростью: оторванные детали не считаются
    let nose_now = car
        .body
        .pos
        .iter()
        .zip(&car.body.vel)
        .filter(|(_, v)| v.len() < 30.0)
        .map(|(p, _)| p.x)
        .fold(f64::MIN, f64::max);
    let total = car.body.energy_total();
    // контакт — по постоянной скорости до препятствия: при косом ударе датчик не показателен
    let t_contact = 0.6 + wall_gap / speed;
    let _ = contact_time;
    // не остановилась за время расчёта (косой удар: машину развернуло) — времени нет
    let t_stop = stop_time.unwrap_or(t_contact);
    let mean_g = if t_stop > t_contact { speed / (t_stop - t_contact) / 9.81 } else { 0.0 };
    let (kinetic, elastic) = (car.body.kinetic_energy(), car.body.elastic_energy());
    let l = car.body.ledger;
    let broken = car.body.beams.iter().filter(|b| b.broken).count();
    let broken_glass = car.faces.iter().filter(|f| f.part == Part::Glass && f.beams.iter().any(|&b| car.body.beams[b as usize].broken)).count();
    rec.report = CrashReport {
        speed_kmh,
        mass: car.mass,
        kinetic_in,
        dynamic_crush: (max_travel - wall_gap).max(0.0),
        residual_crush: car.nose_x - sensor_was - (nose_now - end_x),
        firewall_intrusion: (sensor_was - end_x) - (fw_was - fw_now),
        rear_shortening: car.rear_lines.iter().zip(&rear_start).map(|(&(a, b), l)| l - (car.body.pos[a as usize] - car.body.pos[b as usize]).len()).fold(0.0, f64::max),
        peak_g,
        mean_g,
        stop_time: t_stop - t_contact,
        rebound,
        plastic: l.plastic,
        damping: l.damping,
        friction: l.friction,
        rupture: l.rupture,
        contact_damping: l.contact_damping,
        kinetic_left: kinetic,
        elastic_left: elastic,
        balance_error: (total - start_energy).abs() / kinetic_in.max(1.0),
        broken_beams: broken,
        total_beams: car.body.beams.len(),
        broken_glass_faces: broken_glass,
        max_node_speed: car.body.vel.iter().map(|v| v.len()).fold(0.0, f64::max),
    };
    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_overlaps_when_assembled() {
        let car = build_car(5e-5);
        let s = &car.body;
        for a in 0..s.pos.len() {
            for b in a + 1..s.pos.len() {
                let reach = s.self_radius[a] + s.self_radius[b];
                let linked = s.beams.iter().any(|x| (x.a as usize, x.b as usize) == (a, b) || (x.a as usize, x.b as usize) == (b, a));
                if s.self_radius[a] > 0.0 && s.self_radius[b] > 0.0 && !linked {
                    let d = (s.pos[a] - s.pos[b]).len();
                    assert!(d > reach, "узлы {} и {} пересекаются: {:.3} < {:.3}", a, b, d, reach);
                }
            }
        }
    }

    #[test]
    fn no_damage_from_its_own_weight() {
        let mut car = build_car(5e-5);
        for _ in 0..12000 {
            for v in car.body.vel.iter_mut() {
                *v = *v * 0.9985;
            }
            car.body.step();
        }
        // обшивка под своим весом может чуть просесть, но не на десятки джоулей
        assert!(car.body.ledger.plastic < 50.0, "течение {:.0} Дж", car.body.ledger.plastic);
    }

    #[test]
    fn car_rests_on_its_wheels() {
        let mut car = build_car(5e-5);
        for _ in 0..30000 {
            for v in car.body.vel.iter_mut() {
                *v = *v * 0.9985;
            }
            car.body.step();
        }
        for w in &car.wheels {
            let y = car.body.pos[w.node as usize].y;
            assert!((y - 0.3).abs() < 0.03, "колесо на высоте {}", y);
        }
        let lowest = car.sensor.iter().map(|&i| car.body.pos[i as usize].y).fold(f64::MAX, f64::min);
        assert!(lowest > 0.03, "пол лежит на земле: {}", lowest);
        assert_eq!(car.body.beams.iter().filter(|b| b.broken).count(), 0, "от веса ничего рваться не должно");
    }

    #[test]
    #[ignore = "долго без оптимизации: cargo test --lib -- --ignored"]
    fn low_speed_tap_barely_damages_the_car() {
        let r = simulate(Target::Wall, 8.0, 0.4, 1000.0, |_| {}).report;
        assert!(r.residual_crush < 0.2, "смятие {}", r.residual_crush);
    }

    #[test]
    #[ignore = "долго без оптимизации: cargo test --lib -- --ignored"]
    fn energy_balance_holds_in_a_frontal_crash() {
        let r = simulate(Target::Wall, 56.0, 0.3, 1000.0, |_| {}).report;
        assert!(r.balance_error < 0.05, "расхождение {:.1}%", r.balance_error * 100.0);
    }

    /// Сверка с порядком величин из крэш-тестов: лобовой удар 56 км/ч в жёсткую
    /// стену, малолитражка. Диапазоны широкие: машина обобщённая.
    #[test]
    #[ignore = "долго без оптимизации: cargo test --lib -- --ignored"]
    fn frontal_56_kmh_matches_real_crash_tests() {
        let r = simulate(Target::Wall, 56.0, 0.3, 1000.0, |_| {}).report;
        assert!((0.4..1.0).contains(&r.dynamic_crush), "смятие {:.2} м", r.dynamic_crush);
        assert!((12.0..35.0).contains(&r.mean_g), "среднее {:.1} g", r.mean_g);
        assert!((50.0..180.0).contains(&(r.stop_time * 1000.0)), "время {:.0} мс", r.stop_time * 1000.0);
        assert!(r.firewall_intrusion < 0.25, "щит вдавило на {:.2} м", r.firewall_intrusion);
        assert!(r.rear_shortening < 0.03, "зад смялся на {:.3} м", r.rear_shortening);
    }
}
