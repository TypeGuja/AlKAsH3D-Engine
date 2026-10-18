//! Персонаж с нескольких сторон (как спрайты в старых 3D-играх): пять
//! рисунков — спереди, 3/4 спереди, сбоку, 3/4 сзади, сзади (боковые —
//! головой вправо). По углу между тем, куда смотрит персонаж, и
//! направлением на камеру выбирается рисунок; другой бок — отражение.
//! Пакет — MLPS (`views.mlps` от `export_views.py`), спрайты "<имя>/<ракурс>".

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::Vec3;

const VIEWS: [&str; 5] = ["front", "front_3q", "side", "back_3q", "back"];

struct View {
    mesh: usize,
    mirrored: usize,
}

pub struct Turnable {
    pub name: String,
    /// По индексам VIEWS; недостающий — ближайший имеющийся.
    views: Vec<Option<View>>,
    pub width: f32,
}

/// Квад ширины `w`, высоты `h`, низ по центру в начале координат, нормаль +Z.
fn quad(w: f32, h: f32, mirror: bool, tex: u32) -> Result<Mesh, String> {
    let (u0, u1) = if mirror { (1.0, 0.0) } else { (0.0, 1.0) };
    let v = |x: f32, y: f32, u: f32, vv: f32| Vertex::with_normal_uv(x, y, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, u, vv);
    let hw = w * 0.5;
    let verts = [v(-hw, 0.0, u0, 1.0), v(hw, 0.0, u1, 1.0), v(hw, h, u1, 0.0), v(-hw, h, u0, 0.0)];
    let mut mesh = Mesh::from_vertices_and_indices(&verts, &[0, 1, 2, 0, 2, 3]).map_err(|e| format!("{:?}", e))?;
    mesh.albedo_srv_index = Some(tex);
    mesh.alpha_cutoff = 0.5;
    mesh.unlit = true;
    Ok(mesh)
}

pub fn read_pack(engine: &mut AlkashEngine, path: &str) -> Result<(f32, Vec<Turnable>), String> {
    let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = data.get(at..at + n).ok_or_else(|| format!("{}: файл обрывается", path))?;
        at += n;
        Ok(s)
    };
    if take(4)? != b"MLPS" {
        return Err(format!("{}: не MLPS", path));
    }
    let u32_ = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let _version = u32_(take(4)?);
    let tint = f32::from_le_bytes(take(4)?.try_into().unwrap());
    let count = u32_(take(4)?) as usize;
    let mut out: Vec<Turnable> = Vec::new();
    for _ in 0..count {
        let n = u32_(take(4)?) as usize;
        let full = String::from_utf8_lossy(take(n)?).into_owned();
        let height = f32::from_le_bytes(take(4)?.try_into().unwrap());
        let _face = take(4)?;
        let (w, h) = (u32_(take(4)?), u32_(take(4)?));
        let rgba = take((w * h * 4) as usize)?;
        let Some((name, view)) = full.split_once('/') else { continue };
        let Some(vi) = VIEWS.iter().position(|v| *v == view) else { continue };
        engine.process_messages();
        let tex = engine.create_texture_rgba(w, h, rgba).ok_or("текстура ракурса не создалась")?;
        let width = height * w as f32 / h as f32;
        let mesh = engine.add_mesh(quad(width, height, false, tex)?);
        let mirrored = engine.add_mesh(quad(width, height, true, tex)?);
        let t = match out.iter_mut().find(|t| t.name == name) {
            Some(t) => t,
            None => {
                out.push(Turnable { name: name.to_string(), views: (0..VIEWS.len()).map(|_| None).collect(), width: 0.0 });
                out.last_mut().unwrap()
            }
        };
        t.width = t.width.max(width);
        t.views[vi] = Some(View { mesh, mirrored });
    }
    Ok((tint, out))
}

impl Turnable {
    /// `facing` — куда смотрит персонаж (горизонтальный единичный вектор),
    /// `cam` — позиция камеры.
    pub fn draw(&self, out: &mut Vec<MeshInstance>, at: Vec3, facing: Vec3, cam: Vec3, tint: f32) {
        let mut c = cam - at;
        c.y = 0.0;
        let c = c.normalize_or_zero();
        // Угол между взглядом персонажа и направлением на камеру: 0 — смотрит в камеру.
        let ang = facing.dot(c).clamp(-1.0, 1.0).acos();
        let mut vi = ((ang / std::f32::consts::PI) * 4.0).round() as usize;
        // Нет такого рисунка — ближайший имеющийся.
        if self.views[vi.min(4)].is_none() {
            vi = (0..5).filter(|i| self.views[*i].is_some()).min_by_key(|i| (*i as i32 - vi as i32).abs()).unwrap_or(0);
        }
        let Some(v) = &self.views[vi.min(4)] else { return };
        // Голова на рисунке справа; персонаж повернут к зрителю левым боком
        // (взгляд на экране вправо) — как есть, иначе зеркально.
        let right = Vec3::Y.cross(c);
        let mesh = if facing.dot(right) >= 0.0 { v.mesh } else { v.mirrored };
        let turn = c.x.atan2(c.z);
        out.push(MeshInstance::new(mesh).at(at.x, at.y, at.z).rotated(0.0, turn, 0.0).tinted(tint, tint, tint, 1.0));
    }
}
