//! Экранный текст без поддержки прозрачности в движке: каждая буква —
//! отдельный меш из прямоугольников (пиксели растрового шрифта, склеенные
//! в горизонтальные отрезки), светящийся за счёт `material_emissive`.
//! Строка — набор `MeshInstance` этих мешей перед камерой; меши создаются
//! один раз на символ и цвет, каждый кадр меняются только инстансы.
//!
//! `font.bin` генерируется скриптом из системного Consolas Bold (лежит в
//! папке данных рядом со scene.bin): u32 0, u32 высота, u32 число глифов,
//! дальше на глиф u32 код символа, u32 ширина, `высота` строк-битмасок u32.

use std::collections::HashMap;

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::{Mat3, Quat, Vec3};

struct Glyph {
    width: u32,
    rows: Vec<u32>,
}

pub struct Font {
    height: u32,
    glyphs: HashMap<char, Glyph>,
    /// (символ, цвет) -> индекс меша в движке.
    meshes: HashMap<(char, u32), Option<usize>>,
}

#[derive(Clone, Copy)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    fn key(self) -> u32 {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u32;
        q(self.0) << 16 | q(self.1) << 8 | q(self.2)
    }
}

pub struct TextLine<'a> {
    pub text: &'a str,
    /// Позиция по экрану: x, y в долях от -1..1 (как NDC), y вверх.
    pub x: f32,
    pub y: f32,
    /// Высота строки в долях высоты экрана.
    pub size: f32,
    pub color: Rgb,
    /// -1 — выравнивание по левому краю, 0 — по центру, 1 — по правому.
    pub align: f32,
}

/// (вперёд, вправо, вверх) камеры движка.
pub fn camera_basis(engine: &AlkashEngine) -> (Vec3, Vec3, Vec3) {
    let cam = &engine.camera;
    let forward = (cam.target - cam.position).normalize();
    let right = forward.cross(cam.up).normalize();
    (forward, right, right.cross(forward))
}

/// Эйлеровы углы `MeshInstance` (x, y, z), при которых локальная плоскость
/// XY смотрит в камеру.
pub fn facing_camera(engine: &AlkashEngine) -> (f32, f32, f32) {
    let (forward, right, up) = camera_basis(engine);
    let rot = Quat::from_mat3(&Mat3::from_cols(right, up, -forward));
    let (rz, ry, rx) = rot.to_euler(alkash3d_rs::math::EulerRot::ZYX);
    (rx, ry, rz)
}

impl Font {
    pub fn load(path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
        let word = |i: usize| -> Result<u32, String> {
            data.get(i * 4..i * 4 + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| format!("{}: файл обрывается", path))
        };
        let height = word(1)?;
        let count = word(2)? as usize;
        let mut glyphs = HashMap::new();
        let mut at = 3;
        for _ in 0..count {
            let code = word(at)?;
            let width = word(at + 1)?;
            let mut rows = Vec::with_capacity(height as usize);
            for r in 0..height as usize {
                rows.push(word(at + 2 + r)?);
            }
            at += 2 + height as usize;
            if let Some(c) = char::from_u32(code) {
                glyphs.insert(c, Glyph { width, rows });
            }
        }
        Ok(Self { height, glyphs, meshes: HashMap::new() })
    }

    fn advance(&self, c: char) -> f32 {
        self.glyphs.get(&c).map(|g| g.width as f32).unwrap_or(self.height as f32 * 0.5)
    }

    /// Ширина строки в долях NDC по x (для экранных кнопок), `size` — как в `TextLine`.
    pub fn ndc_width(&self, text: &str, size: f32, aspect: f32) -> f32 {
        self.line_width(text) * size * 2.0 / (self.height as f32 * aspect)
    }

    fn line_width(&self, text: &str) -> f32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    /// Меш глифа в пикселях шрифта: x вправо, y вверх, базовая линия —
    /// низ ячейки; лицевая сторона смотрит в +Z.
    fn glyph_mesh(&mut self, engine: &mut AlkashEngine, c: char, color: Rgb) -> Option<usize> {
        if let Some(m) = self.meshes.get(&(c, color.key())) {
            return *m;
        }
        let built = self.glyphs.get(&c).and_then(|g| {
            let h = self.height as f32;
            let mut verts = Vec::new();
            let mut idx = Vec::new();
            for (row, bits) in g.rows.iter().enumerate() {
                let mut x = 0;
                while x < 32 {
                    if bits >> x & 1 == 0 {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < 32 && bits >> x & 1 == 1 {
                        x += 1;
                    }
                    let (x0, x1) = (start as f32, x as f32);
                    let (y0, y1) = (h - row as f32 - 1.0, h - row as f32);
                    let base = verts.len() as u32;
                    for (px, py) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                        verts.push(Vertex::with_normal(px, py, 0.0, 0.0, 0.0, 1.0, color.0, color.1, color.2, 1.0));
                    }
                    idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
            }
            if verts.is_empty() {
                return None;
            }
            let mut mesh = Mesh::from_vertices_and_indices(&verts, &idx).ok()?;
            // Яркость надписи не зависит от фонарей — она сама светится.
            mesh.material_emissive = [0.7, 0.7, 0.7];
            mesh.material_roughness = 1.0;
            Some(engine.add_mesh(mesh))
        });
        self.meshes.insert((c, color.key()), built);
        built
    }

    /// Label3D: строка в мире. `m` — мировой трансформ узла, `height` —
    /// высота шрифта в метрах (font_size * pixel_size), текст идёт вдоль +X
    /// узла, лицом к +Z (как в Godot), выравнивание 0/1/2 — влево/центр/вправо.
    pub fn draw_world(
        &mut self,
        engine: &mut AlkashEngine,
        out: &mut Vec<MeshInstance>,
        text: &str,
        m: alkash3d_rs::math::Mat4,
        height: f32,
        color: Rgb,
        align: u32,
        billboard: bool,
    ) {
        use alkash3d_rs::math::Mat4;
        // Ячейка font.bin выше кегля примерно в 1.22 раза (36 px шрифт в 44 px).
        let px = height / (self.height as f32 / 1.22);
        let width = self.line_width(text) * px;
        let x0 = match align {
            1 => -width * 0.5,
            2 => -width,
            _ => 0.0,
        };
        let y0 = -(self.height as f32) * px * 0.5;
        let base = if billboard {
            let (s, _, t) = m.to_scale_rotation_translation();
            let (forward, right, up) = camera_basis(engine);
            let rot = Quat::from_mat3(&Mat3::from_cols(right, up, -forward));
            Mat4::from_scale_rotation_translation(s, rot, t)
        } else {
            m
        };
        let mut pen = 0.0;
        for c in text.chars() {
            if let Some(mesh) = self.glyph_mesh(engine, c, color) {
                let g = base * Mat4::from_translation(Vec3::new(x0 + pen * px, y0, 0.0)) * Mat4::from_scale(Vec3::splat(px));
                let (s, r, t) = g.to_scale_rotation_translation();
                let (rz, ry, rx) = r.to_euler(alkash3d_rs::math::EulerRot::ZYX);
                out.push(MeshInstance::new(mesh).at(t.x, t.y, t.z).rotated(rx, ry, rz).scaled(s.x, s.y, s.z));
            }
            pen += self.advance(c);
        }
    }

    /// Чёрная подложка под строкой (непрозрачный прямоугольник чуть дальше
    /// плоскости текста), чтобы надпись читалась на любом фоне.
    /// `quad` — меш квадрата [-1, 1] в плоскости XY.
    pub fn draw_backing(&self, engine: &AlkashEngine, out: &mut Vec<MeshInstance>, line: &TextLine, quad: usize) {
        let (forward, right, up) = camera_basis(engine);
        let dist = engine.camera.near * 4.0;
        let half_h = dist * (engine.camera.fov * 0.5).tan();
        let half_w = half_h * engine.camera.aspect;
        let px = line.size * half_h * 2.0 / self.height as f32;
        let width = self.line_width(line.text) * px;
        let height = self.height as f32 * px;
        let left = line.x * half_w - width * (line.align + 1.0) * 0.5;
        let pad = height * 0.25;
        // Чуть дальше букв, с поправкой на перспективу.
        let k = 1.02;
        let center = engine.camera.position
            + (forward * dist + up * (line.y * half_h + height * 0.5) + right * (left + width * 0.5)) * k;
        let (rx, ry, rz) = facing_camera(engine);
        let sx = (width * 0.5 + pad) * k;
        let sy = (height * 0.5 + pad * 0.6) * k;
        out.push(MeshInstance::new(quad).at(center.x, center.y, center.z).rotated(rx, ry, rz).scaled(sx, sy, 1.0));
    }

    /// Добавляет инстансы букв строки перед камерой.
    pub fn draw(&mut self, engine: &mut AlkashEngine, out: &mut Vec<MeshInstance>, line: &TextLine) {
        let (forward, right, up) = camera_basis(engine);
        let cam_pos = engine.camera.position;
        // Плоскость текста — чуть дальше near, чтобы не резалась и не
        // залезала в геометрию стола.
        let dist = engine.camera.near * 4.0;
        let half_h = dist * (engine.camera.fov * 0.5).tan();
        let half_w = half_h * engine.camera.aspect;
        let px = line.size * half_h * 2.0 / self.height as f32;
        let width = self.line_width(line.text) * px;
        let left = line.x * half_w - width * (line.align + 1.0) * 0.5;
        let origin = cam_pos + forward * dist + up * (line.y * half_h) + right * left;

        let (rx, ry, rz) = facing_camera(engine);
        let mut pen = 0.0;
        for c in line.text.chars() {
            if let Some(mesh) = self.glyph_mesh(engine, c, line.color) {
                let p: Vec3 = origin + right * (pen * px);
                out.push(MeshInstance::new(mesh).at(p.x, p.y, p.z).rotated(rx, ry, rz).scaled(px, px, px));
            }
            pen += self.advance(c);
        }
    }
}
