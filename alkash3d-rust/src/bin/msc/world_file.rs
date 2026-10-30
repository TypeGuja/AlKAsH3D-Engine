//! Чтение `world.mscw` — мира My Summer Car, выгруженного из Unity
//! дампером `tools/msc/msc_dump.py` (в .gitignore, как и
//! данные). Формат описан в шапке дампера; координаты уже переведены в
//! правостороннюю систему движка (Z зеркален), UV — с началом сверху.

use alkash3d_rs::math::{Quat, Vec3};

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Как материал ведёт себя со светом и прозрачностью.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Opaque,
    /// Альфа-вырез (листва, решётки, таблички с прозрачным фоном).
    Cutout,
    /// Всё затухает с альфой (Standard Fade, Legacy Transparent).
    Fade,
    /// Тонкое стекло: альфа — только у слоя грязи/тонировки, отражение остаётся.
    Glass,
    /// Озеро/река.
    Water,
    /// Светится сам (лампы, экраны, фары).
    Emissive,
    /// Не рисуется (служебные шейдеры, частицы, облака-скрипты).
    Hidden,
}

pub struct Material {
    /// Имя и шейдер Unity — для отладки выгрузки.
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub shader: String,
    pub kind: Kind,
    /// sRGB-цвет Unity `_Color`, альфа отдельно.
    pub color: [f32; 4],
    pub albedo_tex: i32,
    pub normal_tex: i32,
    pub roughness: f32,
    pub metallic: f32,
    pub cutoff: f32,
    pub emission: [f32; 3],
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    /// Детальная текстура (Unity Detail / _DetailAlbedoMap) и её масштаб UV.
    pub detail_tex: i32,
    pub detail_scale: [f32; 2],
}

pub struct Surface {
    /// position(3) normal(3) uv(2) tangent(4).
    pub vertices: Vec<[f32; 12]>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Root,
    Empty,
    Mesh,
    Light,
    Camera,
}

#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub spot: bool,
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub half_angle: f32,
}

/// Коллайдер Unity (локальные координаты узла, уже в правой системе).
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Box { center: Vec3, size: Vec3 },
    Sphere { center: Vec3, radius: f32 },
    /// `axis` — 0/1/2 (X/Y/Z узла), `height` — полная, вместе с полусферами.
    Capsule { center: Vec3, radius: f32, height: f32, axis: u32 },
    Mesh { mesh: usize, convex: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct Collider {
    pub shape: Shape,
    /// Триггер — зона, а не твёрдое тело (места установки деталей и т.п.).
    pub trigger: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Rigidbody {
    pub kinematic: bool,
    pub mass: f32,
}

/// HingeJoint: ось и пределы в градусах (уже в правой системе).
#[derive(Clone, Copy, Debug)]
pub struct Hinge {
    pub anchor: Vec3,
    pub axis: Vec3,
    pub min: f32,
    pub max: f32,
    pub connected: Option<usize>,
}

pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub kind: NodeKind,
    pub active: bool,
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
    pub mesh: Option<usize>,
    pub materials: Vec<i32>,
    pub light: Option<Light>,
    pub colliders: Vec<Collider>,
    pub rigidbody: Option<Rigidbody>,
    pub hinge: Option<Hinge>,
}

pub struct WorldFile {
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
    pub meshes: Vec<Vec<Surface>>,
    pub nodes: Vec<Node>,
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len());
        let Some(end) = end else {
            return Err(format!("файл мира обрывается на байте {} (нужно ещё {})", self.at, n));
        };
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let b = self.bytes(4 * N)?;
        Ok(std::array::from_fn(|i| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
    fn index(&mut self) -> Result<Option<usize>, String> {
        let i = self.i32()?;
        Ok((i >= 0).then_some(i as usize))
    }
}

impl WorldFile {
    pub fn load(path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
        let mut c = Cursor { data: &data, at: 0 };
        if c.bytes(4)? != b"MSCW" {
            return Err(format!("{}: это не мир MSCW (перевыгрузи msc_dump.py)", path));
        }
        let version = c.u32()?;
        if version != 3 {
            return Err(format!("{}: версия {}, нужна 3 (перевыгрузи msc_dump.py)", path, version));
        }

        let n = c.u32()? as usize;
        let mut textures = Vec::with_capacity(n);
        for _ in 0..n {
            let width = c.u32()?;
            let height = c.u32()?;
            let rgba = c.bytes(width as usize * height as usize * 4)?.to_vec();
            textures.push(Texture { width, height, rgba });
        }

        let n = c.u32()? as usize;
        let mut materials = Vec::with_capacity(n);
        for _ in 0..n {
            let name = c.string()?;
            let shader = c.string()?;
            let kind = match c.u32()? {
                0 => Kind::Opaque,
                1 => Kind::Cutout,
                2 => Kind::Fade,
                3 => Kind::Glass,
                4 => Kind::Water,
                5 => Kind::Emissive,
                _ => Kind::Hidden,
            };
            let color = c.f32s::<4>()?;
            let albedo_tex = c.i32()?;
            let normal_tex = c.i32()?;
            let [roughness, metallic, cutoff] = c.f32s::<3>()?;
            let emission = c.f32s::<3>()?;
            let uv = c.f32s::<4>()?;
            let detail_tex = c.i32()?;
            let detail_scale = c.f32s::<2>()?;
            materials.push(Material {
                name,
                shader,
                kind,
                color,
                albedo_tex,
                normal_tex,
                roughness,
                metallic,
                cutoff,
                emission,
                uv_scale: [uv[0], uv[1]],
                uv_offset: [uv[2], uv[3]],
                detail_tex,
                detail_scale,
            });
        }

        let n = c.u32()? as usize;
        let mut meshes = Vec::with_capacity(n);
        for _ in 0..n {
            let count = c.u32()? as usize;
            let mut surfaces = Vec::with_capacity(count);
            for _ in 0..count {
                let vc = c.u32()? as usize;
                let raw = c.bytes(vc * 48)?;
                let vertices = raw
                    .chunks_exact(48)
                    .map(|v| std::array::from_fn(|i| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap())))
                    .collect();
                let ic = c.u32()? as usize;
                let raw = c.bytes(ic * 4)?;
                let indices = raw.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
                surfaces.push(Surface { vertices, indices });
            }
            meshes.push(surfaces);
        }

        let n = c.u32()? as usize;
        let mut nodes = Vec::with_capacity(n);
        for _ in 0..n {
            let name = c.string()?;
            let parent = c.index()?;
            let kind = match c.u32()? {
                0 => NodeKind::Root,
                2 => NodeKind::Mesh,
                3 => NodeKind::Light,
                4 => NodeKind::Camera,
                _ => NodeKind::Empty,
            };
            let active = c.u32()? != 0;
            let pos = Vec3::from(c.f32s::<3>()?);
            let rot = Quat::from_array(c.f32s::<4>()?).normalize();
            let scale = Vec3::from(c.f32s::<3>()?);
            let mesh = c.index()?;
            let mc = c.u32()? as usize;
            let mut mats = Vec::with_capacity(mc);
            for _ in 0..mc {
                mats.push(c.i32()?);
            }
            let light = if kind == NodeKind::Light {
                let spot = c.u32()? == 1;
                let color = c.f32s::<3>()?;
                let [intensity, range, half_angle] = c.f32s::<3>()?;
                Some(Light { spot, color, intensity, range, half_angle })
            } else {
                None
            };
            let n = c.u32()? as usize;
            let mut colliders = Vec::with_capacity(n);
            for _ in 0..n {
                let ty = c.u32()?;
                let trigger = c.u32()? != 0;
                let center = Vec3::from(c.f32s::<3>()?);
                let size = c.f32s::<3>()?;
                let mesh = c.i32()?;
                let convex = c.u32()? != 0;
                let shape = match ty {
                    0 => Shape::Box { center, size: Vec3::from(size) },
                    1 => Shape::Sphere { center, radius: size[0] },
                    2 => Shape::Capsule { center, radius: size[0], height: size[1], axis: size[2] as u32 },
                    _ => Shape::Mesh { mesh: mesh.max(0) as usize, convex },
                };
                colliders.push(Collider { shape, trigger });
            }
            let rb = c.u32()?;
            let mass = c.f32s::<1>()?[0];
            let rigidbody = (rb != 0).then_some(Rigidbody { kinematic: rb == 2, mass });
            let hinge = if c.u32()? != 0 {
                let anchor = Vec3::from(c.f32s::<3>()?);
                let axis = Vec3::from(c.f32s::<3>()?);
                let [min, max] = c.f32s::<2>()?;
                let connected = c.index()?;
                Some(Hinge { anchor, axis, min, max, connected })
            } else {
                None
            };
            nodes.push(Node { name, parent, kind, active, pos, rot, scale, mesh, materials: mats, light, colliders, rigidbody, hinge });
        }

        if c.at != data.len() {
            return Err(format!("{}: после разбора осталось {} лишних байт", path, data.len() - c.at));
        }
        Ok(Self { textures, materials, meshes, nodes })
    }

    pub fn material(&self, i: i32) -> Option<&Material> {
        (i >= 0).then(|| self.materials.get(i as usize)).flatten()
    }
}
