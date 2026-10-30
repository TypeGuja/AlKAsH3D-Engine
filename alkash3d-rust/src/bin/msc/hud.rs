//! Экранный текст. Растровый шрифт `font.bin` (Consolas Bold, лежит в папке
//! данных) собирается в одну текстуру-атлас: белые буквы с альфой, у
//! каждого символа — свой квад с UV своей ячейки. Квады не освещаются
//! (`Mesh::unlit`), прозрачный фон отсекается по альфе. Строка — инстансы
//! квадов в плоскости перед камерой; цвет — через tint, делённый на
//! экспозицию, чтобы надписи не "слепли" и не гасли вместе с адаптацией.
//!
//! font.bin: u32 0, u32 высота, u32 число глифов; на глиф u32 код символа,
//! u32 ширина, `высота` строк-битмасок u32 (бит 0 — левый пиксель).

use std::collections::HashMap;

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::{EulerRot, Mat3, Quat, Vec3};

pub struct Hud {
    height: u32,
    /// символ -> (меш, ширина в пикселях шрифта)
    glyphs: HashMap<char, (usize, f32)>,
    /// Квад [0,1]² для подложек.
    quad: usize,
    exposure: f32,
}

#[derive(Clone, Copy)]
pub struct Text<'a> {
    pub text: &'a str,
    /// x, y в долях экрана -1..1 (как NDC), y вверх — базовая линия строки.
    pub x: f32,
    pub y: f32,
    /// Высота строки в долях высоты экрана.
    pub size: f32,
    pub color: [f32; 3],
    /// -1 — влево, 0 — по центру, 1 — вправо.
    pub align: f32,
    /// Подложка: непрозрачность тёмного прямоугольника под строкой (0 — нет).
    pub backing: f32,
}

impl Hud {
    pub fn load(engine: &mut AlkashEngine, path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
        let word = |i: usize| -> Result<u32, String> {
            data.get(i * 4..i * 4 + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| format!("{}: файл обрывается", path))
        };
        let height = word(1)?;
        let count = word(2)? as usize;
        let mut list = Vec::with_capacity(count);
        let mut at = 3;
        for _ in 0..count {
            let code = word(at)?;
            let width = word(at + 1)?;
            let rows: Vec<u32> = (0..height as usize).map(|r| word(at + 2 + r)).collect::<Result<_, _>>()?;
            at += 2 + height as usize;
            if let Some(c) = char::from_u32(code) {
                list.push((c, width, rows));
            }
        }

        // Атлас: ячейки 32 x (высота + 2), 16 в ряд, по пикселю зазора.
        let (cw, ch) = (34u32, height + 2);
        let cols = 16u32;
        let rows_n = (list.len() as u32).div_ceil(cols).max(1);
        let (aw, ah) = ((cols * cw).next_power_of_two(), (rows_n * ch).next_power_of_two());
        let mut rgba = vec![0u8; (aw * ah * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px[..3].fill(255);
        }
        let mut glyphs = HashMap::new();
        for (k, (c, width, bits)) in list.iter().enumerate() {
            let (gx, gy) = ((k as u32 % cols) * cw + 1, (k as u32 / cols) * ch + 1);
            for (r, row) in bits.iter().enumerate() {
                for x in 0..32u32 {
                    if row >> x & 1 == 1 {
                        let i = (((gy + r as u32) * aw + gx + x) * 4 + 3) as usize;
                        rgba[i] = 255;
                    }
                }
            }
            glyphs.insert(*c, (gx, gy, *width));
        }
        let tex = engine.create_texture_rgba(aw, ah, &rgba).ok_or("атлас шрифта не создался")?;

        let h = height as f32;
        let mut meshes = HashMap::new();
        for (c, (gx, gy, width)) in glyphs {
            let w = width as f32;
            let (u0, u1) = (gx as f32 / aw as f32, (gx as f32 + w) / aw as f32);
            let (v0, v1) = (gy as f32 / ah as f32, (gy as f32 + h) / ah as f32);
            let v = |x: f32, y: f32, u: f32, vv: f32| Vertex::with_normal_uv(x, y, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, u, vv);
            let verts = [v(0.0, 0.0, u0, v1), v(w, 0.0, u1, v1), v(w, h, u1, v0), v(0.0, h, u0, v0)];
            let Ok(mut mesh) = Mesh::from_vertices_and_indices(&verts, &[0, 1, 2, 0, 2, 3]) else { continue };
            mesh.albedo_srv_index = Some(tex);
            mesh.alpha_cutoff = 0.5;
            mesh.unlit = true;
            meshes.insert(c, (engine.add_mesh(mesh), w));
        }
        let qv = |x: f32, y: f32| Vertex::with_normal(x, y, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        let mut quad = Mesh::from_vertices_and_indices(&[qv(0.0, 0.0), qv(1.0, 0.0), qv(1.0, 1.0), qv(0.0, 1.0)], &[0, 1, 2, 0, 2, 3]).map_err(|e| format!("{:?}", e))?;
        quad.unlit = true;
        quad.blend = true;
        let quad = engine.add_mesh(quad);
        Ok(Hud { height, glyphs: meshes, quad, exposure: 1.0 })
    }

    pub fn set_exposure(&mut self, e: f32) {
        self.exposure = e.max(1e-4);
    }

    fn advance(&self, c: char) -> f32 {
        self.glyphs.get(&c).map(|g| g.1).unwrap_or(self.height as f32 * 0.5)
    }

    /// Плоскость текста перед камерой: (начало экрана, вправо, вверх, пол-высоты, пол-ширины, поворот).
    fn frame(engine: &AlkashEngine) -> (Vec3, Vec3, Vec3, f32, f32, (f32, f32, f32)) {
        let cam = &engine.camera;
        let forward = (cam.target - cam.position).normalize();
        let right = forward.cross(cam.up).normalize();
        let up = right.cross(forward);
        let dist = cam.near * 2.0;
        let half_h = dist * (cam.fov * 0.5).tan();
        let half_w = half_h * cam.aspect;
        let rot = Quat::from_mat3(&Mat3::from_cols(right, up, -forward));
        let (rz, ry, rx) = rot.to_euler(EulerRot::ZYX);
        (cam.position + forward * dist, right, up, half_h, half_w, (rx, ry, rz))
    }

    pub fn draw(&self, engine: &AlkashEngine, out: &mut Vec<MeshInstance>, t: Text) {
        let (center, right, up, half_h, half_w, (rx, ry, rz)) = Self::frame(engine);
        let px = t.size * half_h * 2.0 / self.height as f32;
        let width: f32 = t.text.chars().map(|c| self.advance(c)).sum::<f32>() * px;
        let left = t.x * half_w - width * (t.align + 1.0) * 0.5;
        let origin = center + up * (t.y * half_h) + right * left;
        let k = 1.0 / self.exposure;
        let mut pen = 0.0;
        for c in t.text.chars() {
            if let Some(&(mesh, _)) = self.glyphs.get(&c) {
                let p = origin + right * (pen * px);
                out.push(
                    MeshInstance::new(mesh)
                        .at(p.x, p.y, p.z)
                        .rotated(rx, ry, rz)
                        .scaled(px, px, px)
                        .tinted(t.color[0] * k, t.color[1] * k, t.color[2] * k, 1.0),
                );
            }
            pen += self.advance(c);
        }
        if t.backing > 0.0 {
            // Чуть дальше букв (с поправкой на перспективу), с полями.
            let pad = self.height as f32 * px * 0.3;
            let s = 1.01;
            let o = engine.camera.position + (origin - right * pad - up * pad * 0.7 - engine.camera.position) * s;
            let w = (width + pad * 2.0) * s;
            let h = (self.height as f32 * px + pad * 1.4) * s;
            out.push(MeshInstance::new(self.quad).at(o.x, o.y, o.z).rotated(rx, ry, rz).scaled(w, h, 1.0).tinted(0.0, 0.0, 0.0, t.backing));
        }
    }
}
