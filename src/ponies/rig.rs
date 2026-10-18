//! 2D-риг персонажа (перекладка) и его анимация.
//!
//! Пакет MLPR пишет `export_rig.py`: у персонажа части (тело, голова с
//! гривой, хвост, ноги) — каждая своя текстура и прямоугольник в метрах от
//! точки "середина под копытами", шарнир, родитель и порядок рисования.
//! Часть — квад, у которого начало координат в шарнире: повернуть часть —
//! повернуть инстанс. Фигура стоит в вертикальной плоскости, развёрнутой к
//! камере вокруг вертикали; части разнесены по глубине на доли миллиметра
//! по порядку рисования (альфа-тест, без сортировки).
//! Зеркало (персонаж идёт в другую сторону, чем нарисован) — отдельные
//! отражённые меши, углы и x — с обратным знаком.

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::{EulerRot, Quat, Vec3};

pub struct Part {
    pub name: String,
    pub parent: Option<usize>,
    pub z: i32,
    /// Шарнир (м), x вправо, y вверх — в рисунке "мордой как нарисован".
    pub pivot: [f32; 2],
    mesh: usize,
    mirrored: usize,
}

pub struct Rig {
    pub name: String,
    pub height: f32,
    pub width: f32,
    /// 1 — на рисунке мордой вправо, -1 — влево.
    pub face: i32,
    pub main: bool,
    pub parts: Vec<Part>,
}

/// Поза: угол каждой части (рад, против часовой — для фигуры мордой
/// вправо: нога вперёд, голова вверх, хвост вверх), смещение тела (м).
pub struct Pose {
    pub angle: Vec<f32>,
    pub lift: f32,
}

pub fn read_pack(engine: &mut AlkashEngine, path: &str) -> Result<(f32, Vec<Rig>), String> {
    let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = data.get(at..at + n).ok_or_else(|| format!("{}: файл обрывается", path))?;
        at += n;
        Ok(s)
    };
    if take(4)? != b"MLPR" {
        return Err(format!("{}: не MLPR", path));
    }
    let u32_ = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let i32_ = |b: &[u8]| i32::from_le_bytes(b.try_into().unwrap());
    let f32_ = |b: &[u8]| f32::from_le_bytes(b.try_into().unwrap());
    let version = u32_(take(4)?);
    if version != 1 {
        return Err(format!("{}: версия {} не поддерживается", path, version));
    }
    let tint = f32_(take(4)?);
    let count = u32_(take(4)?) as usize;
    let mut rigs = Vec::with_capacity(count);
    for _ in 0..count {
        let n = u32_(take(4)?) as usize;
        let name = String::from_utf8_lossy(take(n)?).into_owned();
        let height = f32_(take(4)?);
        let face = i32_(take(4)?);
        let main = u32_(take(4)?) == 1;
        let np = u32_(take(4)?) as usize;
        let mut parts = Vec::with_capacity(np);
        let (mut xmin, mut xmax) = (f32::MAX, f32::MIN);
        for _ in 0..np {
            let n = u32_(take(4)?) as usize;
            let pname = String::from_utf8_lossy(take(n)?).into_owned();
            let parent = i32_(take(4)?);
            let z = i32_(take(4)?);
            let v: Vec<f32> = (0..6).map(|_| take(4).map(f32_)).collect::<Result<_, _>>()?;
            let (w, h) = (u32_(take(4)?), u32_(take(4)?));
            let rgba = take((w * h * 4) as usize)?;
            // Загрузка сотен текстур — секунды; окно должно отвечать.
            engine.process_messages();
            let tex = engine.create_texture_rgba(w, h, rgba).ok_or("текстура части не создалась")?;
            let pivot = [v[0], v[1]];
            let rect = [v[2] - v[0], v[3] - v[1], v[4] - v[0], v[5] - v[1]];
            xmin = xmin.min(v[2]);
            xmax = xmax.max(v[4]);
            let mesh = engine.add_mesh(part_quad(rect, false, tex)?);
            let mirrored = engine.add_mesh(part_quad(rect, true, tex)?);
            parts.push(Part { name: pname, parent: (parent >= 0).then_some(parent as usize), z, pivot, mesh, mirrored });
        }
        rigs.push(Rig { name, height, width: xmax - xmin, face, main, parts });
    }
    Ok((tint, rigs))
}

/// Квад части: прямоугольник [x0, y0, x1, y1] относительно шарнира,
/// нормаль +Z. Отражённый — x с обратным знаком (порядок вершин обратный,
/// чтобы лицевая сторона осталась той же).
fn part_quad(r: [f32; 4], mirror: bool, tex: u32) -> Result<Mesh, String> {
    let s = if mirror { -1.0 } else { 1.0 };
    let v = |x: f32, y: f32, u: f32, vv: f32| Vertex::with_normal_uv(x * s, y, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, u, vv);
    let verts = [v(r[0], r[1], 0.0, 1.0), v(r[2], r[1], 1.0, 1.0), v(r[2], r[3], 1.0, 0.0), v(r[0], r[3], 0.0, 0.0)];
    let idx: &[u32] = if mirror { &[0, 2, 1, 0, 3, 2] } else { &[0, 1, 2, 0, 2, 3] };
    let mut mesh = Mesh::from_vertices_and_indices(&verts, idx).map_err(|e| format!("{:?}", e))?;
    mesh.albedo_srv_index = Some(tex);
    mesh.alpha_cutoff = 0.5;
    mesh.unlit = true;
    Ok(mesh)
}

impl Rig {
    fn find(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.name == name)
    }

    /// Стойка: дыхание, голова чуть покачивается, хвост машет.
    pub fn idle(&self, t: f32, seed: f32) -> Pose {
        let mut angle = vec![0.0; self.parts.len()];
        if let Some(i) = self.find("head") {
            angle[i] = 0.045 * (t * 1.1 + seed).sin() + 0.02 * (t * 0.37 + seed * 2.0).sin();
        }
        if let Some(i) = self.find("tail") {
            angle[i] = 0.10 * (t * 1.7 + seed).sin();
        }
        Pose { angle, lift: 0.006 * self.height * (t * 2.1 + seed).sin() }
    }

    /// Шаг: диагональные пары ног вместе (рысь), тело подпрыгивает на
    /// каждом шаге, голова кивает, хвост отстаёт по фазе.
    /// `phase` — фаза цикла шага (рад).
    pub fn walk(&self, phase: f32) -> Pose {
        let mut angle = vec![0.0; self.parts.len()];
        let a = 0.38;
        let s = phase.sin();
        for (i, p) in self.parts.iter().enumerate() {
            angle[i] = match p.name.as_str() {
                "leg_front_near" | "leg_back_far" => a * s,
                "leg_front_far" | "leg_back_near" => -a * s,
                // Пара ног слита в один кусок — качается вполсилы.
                "leg_front" => 0.5 * a * s,
                "leg_back" => -0.5 * a * s,
                "head" => 0.05 * (2.0 * phase).sin(),
                "tail" => 0.18 * (phase + 1.2).sin(),
                _ => 0.0,
            };
        }
        Pose { angle, lift: 0.012 * self.height * (2.0 * phase).cos().abs() }
    }

    /// Инстансы частей. `at` — точка под копытами, `turn` — поворот плоскости
    /// фигуры вокруг вертикали, `right` — смотреть вправо (в своей плоскости).
    pub fn draw(&self, out: &mut Vec<MeshInstance>, pose: &Pose, at: Vec3, turn: f32, right: bool, tint: f32) {
        let mirror = (self.face > 0) != right;
        let sx = if mirror { -1.0 } else { 1.0 };
        let plane = Quat::from_rotation_y(turn);
        // Мировые (в плоскости фигуры) шарнир и угол каждой части: ребёнок
        // поворачивается вместе с родителем вокруг шарнира родителя.
        let mut world: Vec<Option<([f32; 2], f32)>> = vec![None; self.parts.len()];
        for _ in 0..self.parts.len() {
            for (i, p) in self.parts.iter().enumerate() {
                if world[i].is_some() {
                    continue;
                }
                let (pos, ang) = match p.parent {
                    None => ([p.pivot[0], p.pivot[1] + pose.lift], pose.angle[i]),
                    Some(pi) => {
                        let Some((ppos, pang)) = world[pi] else { continue };
                        let par = &self.parts[pi];
                        let (dx, dy) = (p.pivot[0] - par.pivot[0], p.pivot[1] - par.pivot[1]);
                        let (c, s) = (pang.cos(), pang.sin());
                        ([ppos[0] + c * dx - s * dy, ppos[1] + s * dx + c * dy], pang + pose.angle[i])
                    }
                };
                world[i] = Some((pos, ang));
            }
        }
        for (i, p) in self.parts.iter().enumerate() {
            let Some((pos, ang)) = world[i] else { continue };
            let local = Vec3::new(pos[0] * sx, pos[1], p.z as f32 * 0.0015);
            let w = at + plane * local;
            let rot = plane * Quat::from_rotation_z(ang * sx);
            let (rz, ry, rx) = rot.to_euler(EulerRot::ZYX);
            out.push(
                MeshInstance::new(if mirror { p.mirrored } else { p.mesh })
                    .at(w.x, w.y, w.z)
                    .rotated(rx, ry, rz)
                    .tinted(tint, tint, tint, 1.0),
            );
        }
    }
}
