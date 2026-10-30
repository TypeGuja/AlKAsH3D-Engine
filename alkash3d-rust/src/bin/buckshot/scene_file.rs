//! Чтение сцены Buckshot Roulette. Два источника:
//! - `.altex` версии 3 (блок сцены, `altex_scene.rs`) + `.alfar` рядом со
//!   светом — формат движка, его делает `brsc2altex`;
//! - `world.bin` (BRSC) — сырой дамп Godot-сцены от `_dump_v2.gd` (лежит
//!   рядом с распакованным проектом игры, НЕ в репо). Little-endian, блоки
//!   по порядку: текстуры, материалы, меши, дерево узлов (локальные
//!   трансформы + индекс родителя), AnimationPlayer'ы со всеми анимациями.

use alkash3d_rs::math::{Mat4, Quat, Vec3, Vec4};

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Material {
    /// sRGB-цвет из Godot (albedo_color), альфа отдельно.
    pub albedo: [f32; 4],
    pub albedo_tex: i32,
    pub normal_tex: i32,
    pub roughness: f32,
    pub metallic: f32,
    pub emission: [f32; 3],
    /// BaseMaterial3D.transparency: 0 — непрозрачный, 2 — alpha scissor, остальное — смешивание.
    pub transparency: u32,
    pub unshaded: bool,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
}

pub struct Surface {
    /// position(3) normal(3) uv(2) tangent(4) — 12 f32 на вершину.
    pub vertices: Vec<[f32; 12]>,
    pub indices: Vec<u32>,
    /// Скин (формат v3): 4 индекса привязок (bind) и 4 веса на вершину.
    pub skin: Option<(Vec<[u32; 4]>, Vec<[f32; 4]>)>,
}

pub struct MeshData {
    pub surfaces: Vec<Surface>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Node,
    Spatial,
    Mesh,
    Light,
    Camera,
    Canvas,
    Audio,
    Skeleton,
    Label,
}

/// Label3D: текст в мире.
#[derive(Clone, Debug)]
pub struct LabelInfo {
    pub text: String,
    pub font_size: f32,
    pub pixel_size: f32,
    /// modulate (sRGB + альфа).
    pub color: [f32; 4],
    /// HorizontalAlignment: 0 — влево, 1 — по центру, 2 — вправо.
    pub align: u32,
    /// 0 — нет, иначе текст повёрнут к камере.
    pub billboard: u32,
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub rest: Mat4,
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct LightInfo {
    pub spot: bool,
    pub color: [f32; 3],
    pub energy: f32,
    pub range: f32,
    pub angle: f32,
    pub attenuation: f32,
    #[allow(dead_code)]
    pub shadow: bool,
}

pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub kind: Kind,
    pub visible: bool,
    pub top_level: bool,
    pub xf: Mat4,
    pub mesh: Option<usize>,
    pub materials: Vec<i32>,
    pub light: Option<LightInfo>,
    pub fov: f32,
    pub audio: String,
    /// Скиннингованный меш (v3): узел Skeleton3D и привязки (кость, bind pose).
    pub skeleton: Option<usize>,
    pub binds: Vec<(i32, Mat4)>,
    /// Узел Skeleton3D (v3): кости.
    pub bones: Vec<Bone>,
    pub label: Option<LabelInfo>,
}

/// Строки и Vec2 из дорожек сохраняются, но порт их пока не читает.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum Value {
    None,
    Float(f32),
    Vec3(Vec3),
    Quat(Quat),
    Color([f32; 4]),
    Xf(Mat4),
    Str(String),
    Vec2([f32; 2]),
}

#[derive(Clone, Debug)]
pub enum Keys {
    /// Value / position / rotation / scale / bezier: (время, easing, значение).
    Value(Vec<(f32, f32, Value)>),
    /// Вызов метода скрипта: (время, имя, аргументы).
    Method(Vec<(f32, String, Vec<Value>)>),
    /// Звук: (время, путь к ресурсу).
    Audio(Vec<(f32, String)>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackType {
    Value,
    Position,
    Rotation,
    Scale,
    Other,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub target: Option<usize>,
    /// NodePath целиком ("../door:rotation"); нужен конвертеру.
    #[allow(dead_code)]
    pub path: String,
    /// Godot Animation.TrackType как есть.
    #[allow(dead_code)]
    pub type_id: u32,
    /// Свойство после ':' в NodePath ("position", "visible", "light_energy"…).
    pub property: String,
    pub kind: TrackType,
    /// 0 — ближайший ключ, 1 — линейно, 2/3/4 — кубически (у нас линейно).
    pub interp: u32,
    pub keys: Keys,
}

#[derive(Clone, Debug)]
pub struct Animation {
    pub name: String,
    pub length: f32,
    pub looped: bool,
    pub tracks: Vec<Track>,
}

pub struct Player {
    pub node: usize,
    /// AnimationPlayer.speed_scale (v4; раньше считался 1).
    pub speed: f32,
    pub animations: Vec<Animation>,
}

pub struct SceneFile {
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
    pub meshes: Vec<MeshData>,
    pub nodes: Vec<Node>,
    pub players: Vec<Player>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err(format!("файл сцены обрывается на байте {} (нужно ещё {})", self.pos, n));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut out = [0.0; N];
        for v in out.iter_mut() {
            *v = self.f32()?;
        }
        Ok(out)
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn index(&mut self) -> Result<Option<usize>, String> {
        let i = self.i32()?;
        Ok(if i < 0 { None } else { Some(i as usize) })
    }
    fn xf(&mut self) -> Result<Mat4, String> {
        let f = self.floats::<12>()?;
        Ok(Mat4::from_cols(
            Vec4::new(f[0], f[1], f[2], 0.0),
            Vec4::new(f[3], f[4], f[5], 0.0),
            Vec4::new(f[6], f[7], f[8], 0.0),
            Vec4::new(f[9], f[10], f[11], 1.0),
        ))
    }
    fn value(&mut self) -> Result<Value, String> {
        Ok(match self.u32()? {
            0 => Value::None,
            1 => Value::Float(self.f32()?),
            2 => Value::Vec3(Vec3::from(self.floats::<3>()?)),
            3 => Value::Quat(Quat::from_array(self.floats::<4>()?)),
            4 => Value::Color(self.floats::<4>()?),
            5 => Value::Xf(self.xf()?),
            6 => Value::Str(self.string()?),
            7 => Value::Vec2(self.floats::<2>()?),
            t => return Err(format!("неизвестный тип значения {} на байте {}", t, self.pos)),
        })
    }
}

impl SceneFile {
    pub fn load(path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
        if data.starts_with(b"ALKALTEX") {
            drop(data);
            return Self::load_altex(path);
        }
        let mut r = Reader { data: &data, pos: 0 };
        if r.take(4)? != b"BRSC" {
            return Err(format!("{}: неверная сигнатура (ожидалось BRSC)", path));
        }
        let version = r.u32()?;
        if !(2..=4).contains(&version) {
            return Err(format!("{}: версия формата {}, нужна 2–4 (перевыгрузи _dump_v2.gd)", path, version));
        }

        let count = r.u32()?;
        let mut textures = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let width = r.u32()?;
            let height = r.u32()?;
            let rgba = r.take((width * height * 4) as usize)?.to_vec();
            textures.push(Texture { width, height, rgba });
        }

        let count = r.u32()?;
        let mut materials = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let albedo = r.floats::<4>()?;
            let albedo_tex = r.i32()?;
            let normal_tex = r.i32()?;
            let roughness = r.f32()?;
            let metallic = r.f32()?;
            let emission = r.floats::<3>()?;
            let transparency = r.u32()?;
            let unshaded = r.u32()? != 0;
            let uv = r.floats::<4>()?;
            materials.push(Material {
                albedo,
                albedo_tex,
                normal_tex,
                roughness,
                metallic,
                emission,
                transparency,
                unshaded,
                uv_scale: [uv[0], uv[1]],
                uv_offset: [uv[2], uv[3]],
            });
        }

        let count = r.u32()?;
        let mut meshes = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let surface_count = r.u32()?;
            let mut surfaces = Vec::with_capacity(surface_count as usize);
            for _ in 0..surface_count {
                let vc = r.u32()? as usize;
                let mut vertices = Vec::with_capacity(vc);
                for _ in 0..vc {
                    vertices.push(r.floats::<12>()?);
                }
                let ic = r.u32()? as usize;
                let mut indices = Vec::with_capacity(ic);
                for _ in 0..ic {
                    indices.push(r.u32()?);
                }
                let mut skin = None;
                if version >= 3 && r.u32()? == 1 {
                    let mut bones = Vec::with_capacity(vc);
                    let mut weights = Vec::with_capacity(vc);
                    for _ in 0..vc {
                        bones.push([r.u32()?, r.u32()?, r.u32()?, r.u32()?]);
                        weights.push(r.floats::<4>()?);
                    }
                    skin = Some((bones, weights));
                }
                surfaces.push(Surface { vertices, indices, skin });
            }
            meshes.push(MeshData { surfaces });
        }

        let count = r.u32()?;
        let mut nodes = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name = r.string()?;
            let parent = r.index()?;
            let kind = match r.u32()? {
                0 => Kind::Node,
                1 => Kind::Spatial,
                2 => Kind::Mesh,
                3 => Kind::Light,
                4 => Kind::Camera,
                5 => Kind::Canvas,
                7 => Kind::Skeleton,
                8 => Kind::Label,
                _ => Kind::Audio,
            };
            let flags = r.u32()?;
            let xf = r.xf()?;
            let mesh = r.index()?;
            let mc = r.u32()?;
            let mut mats = Vec::with_capacity(mc as usize);
            for _ in 0..mc {
                mats.push(r.i32()?);
            }
            let mut light = None;
            let mut fov = 0.0;
            let mut audio = String::new();
            match kind {
                Kind::Light => {
                    let spot = r.u32()? == 1;
                    let shadow = r.u32()? != 0;
                    let c = r.floats::<4>()?;
                    let p = r.floats::<4>()?;
                    light = Some(LightInfo {
                        spot,
                        color: [c[0], c[1], c[2]],
                        energy: p[0],
                        range: p[1],
                        angle: p[2],
                        attenuation: p[3],
                        shadow,
                    });
                }
                Kind::Camera => fov = r.f32()?,
                Kind::Audio => audio = r.string()?,
                _ => {}
            }
            let mut skeleton = None;
            let mut binds = Vec::new();
            let mut bones = Vec::new();
            if version >= 3 && kind == Kind::Mesh {
                skeleton = r.index()?;
                let n = r.u32()?;
                for _ in 0..n {
                    let bone = r.i32()?;
                    binds.push((bone, r.xf()?));
                }
            }
            let mut label = None;
            if kind == Kind::Label {
                let text = r.string()?;
                let font_size = r.f32()?;
                let pixel_size = r.f32()?;
                let color = r.floats::<4>()?;
                let align = r.u32()?;
                let billboard = r.u32()?;
                let _outline = r.f32()?;
                let _outline_color = r.floats::<4>()?;
                label = Some(LabelInfo { text, font_size, pixel_size, color, align, billboard });
            }
            if version >= 3 && kind == Kind::Skeleton {
                let n = r.u32()?;
                for _ in 0..n {
                    let name = r.string()?;
                    let parent = r.index()?;
                    let rest = r.xf()?;
                    let pos = Vec3::from(r.floats::<3>()?);
                    let rot = Quat::from_array(r.floats::<4>()?);
                    let scale = Vec3::from(r.floats::<3>()?);
                    bones.push(Bone { name, parent, rest, pos, rot, scale });
                }
            }
            nodes.push(Node {
                name,
                parent,
                kind,
                visible: flags & 1 != 0,
                top_level: flags & 2 != 0,
                xf,
                mesh,
                materials: mats,
                light,
                fov,
                audio,
                skeleton,
                binds,
                bones,
                label,
            });
        }

        let count = r.u32()?;
        let mut players = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let node = r.index()?.ok_or("AnimationPlayer без узла")?;
            let _root = r.index()?;
            let speed = if version >= 4 { r.f32()? } else { 1.0 };
            let anim_count = r.u32()?;
            let mut animations = Vec::with_capacity(anim_count as usize);
            for _ in 0..anim_count {
                let name = r.string()?;
                let length = r.f32()?;
                let looped = r.u32()? != 0;
                let track_count = r.u32()?;
                let mut tracks = Vec::with_capacity(track_count as usize);
                for _ in 0..track_count {
                    let target = r.index()?;
                    let path = r.string()?;
                    let tt = r.u32()?;
                    let interp = r.u32()?;
                    let _enabled = r.u32()?;
                    let key_count = r.u32()?;
                    // Godot Animation.TrackType: 0 value, 1 position_3d, 2 rotation_3d,
                    // 3 scale_3d, 4 blend shape, 5 method, 6 bezier, 7 audio, 8 animation.
                    let keys = match tt {
                        5 => {
                            let mut k = Vec::with_capacity(key_count as usize);
                            for _ in 0..key_count {
                                let t = r.f32()?;
                                let m = r.string()?;
                                let n = r.u32()?;
                                let mut args = Vec::with_capacity(n as usize);
                                for _ in 0..n {
                                    args.push(r.value()?);
                                }
                                k.push((t, m, args));
                            }
                            Keys::Method(k)
                        }
                        7 => {
                            let mut k = Vec::with_capacity(key_count as usize);
                            for _ in 0..key_count {
                                let t = r.f32()?;
                                k.push((t, r.string()?));
                            }
                            Keys::Audio(k)
                        }
                        _ => {
                            let mut k = Vec::with_capacity(key_count as usize);
                            for _ in 0..key_count {
                                let t = r.f32()?;
                                let ease = r.f32()?;
                                k.push((t, ease, r.value()?));
                            }
                            Keys::Value(k)
                        }
                    };
                    let property = path.split_once(':').map(|(_, p)| p.to_string()).unwrap_or_default();
                    let kind = match tt {
                        0 | 6 => TrackType::Value,
                        1 => TrackType::Position,
                        2 => TrackType::Rotation,
                        3 => TrackType::Scale,
                        _ => TrackType::Other,
                    };
                    tracks.push(Track { target, path, type_id: tt, property, kind, interp, keys });
                }
                animations.push(Animation { name, length, looped, tracks });
            }
            players.push(Player { node, speed, animations });
        }

        if r.pos != data.len() {
            return Err(format!("{}: после разбора осталось {} лишних байт", path, data.len() - r.pos));
        }
        Ok(Self { textures, materials, meshes, nodes, players })
    }

    /// Сцена из `.altex` (блок сцены) и `.alfar` с тем же именем (свет).
    fn load_altex(path: &str) -> Result<Self, String> {
        use alkash3d_rs::altex_scene::{AnimValue, NodeKind, TrackKeys};

        let altex = alkash3d_rs::altex_format::AltexFile::load(path).map_err(|e| format!("{}: {}", path, e))?;
        let ext = altex.scene.as_ref().ok_or_else(|| format!("{}: нет блока сцены (нужен .altex версии 3 от brsc2altex)", path))?;
        let alfar_path = std::path::Path::new(path).with_extension("alfar");
        let alfar = alkash3d_rs::AlfarFile::load(&alfar_path.to_string_lossy()).ok();

        let mut textures = Vec::with_capacity(altex.textures.len());
        for (i, t) in altex.textures.iter().enumerate() {
            if t.data_size == 0 {
                return Err(format!("{}: текстура {} внешняя, не поддерживается", path, i));
            }
            let rgba = altex.texture_data[t.data_offset as usize..(t.data_offset + t.data_size) as usize].to_vec();
            textures.push(Texture { width: t.width, height: t.height, rgba });
        }

        let tex = |m: u32| if m == 0xFFFF_FFFF { -1 } else { m as i32 };
        let materials = altex
            .materials
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let x = ext.material_extras.get(i);
                Material {
                    albedo: m.albedo,
                    albedo_tex: tex(m.albedo_map),
                    normal_tex: tex(m.normal_map),
                    roughness: m.roughness,
                    metallic: m.metallic,
                    emission: m.emissive,
                    transparency: x.map_or(0, |x| x.transparency),
                    unshaded: x.is_some_and(|x| x.unshaded != 0),
                    uv_scale: x.map_or([1.0, 1.0], |x| x.uv_scale),
                    uv_offset: x.map_or([0.0, 0.0], |x| x.uv_offset),
                }
            })
            .collect();

        let skins: std::collections::HashMap<u32, &alkash3d_rs::altex_scene::MeshSkin> = ext.skins.iter().map(|s| (s.mesh, s)).collect();
        let mut meshes = Vec::with_capacity(ext.mesh_groups.len());
        for g in &ext.mesh_groups {
            let mut surfaces = Vec::with_capacity(g.count as usize);
            for mi in g.first..g.first + g.count {
                let m = altex.meshes.get(mi as usize).ok_or_else(|| format!("{}: нет меша {}", path, mi))?;
                let vs = &altex.vertices[m.vertex_offset as usize..(m.vertex_offset + m.vertex_count) as usize];
                let vertices = vs
                    .iter()
                    .map(|v| {
                        let (n, t) = (Vec3::from(v.normal), Vec3::from(v.tangent));
                        // Знак w: битангенс = cross(n, t) * w, при t ∥ n — n * w.
                        let c = n.cross(t);
                        let axis = if c.length_squared() > 1e-12 { c } else { n };
                        let w = if axis.dot(Vec3::from(v.bitangent)) < 0.0 { -1.0 } else { 1.0 };
                        let [px, py, pz] = v.position;
                        [px, py, pz, n.x, n.y, n.z, v.uv[0], v.uv[1], t.x, t.y, t.z, w]
                    })
                    .collect();
                let indices = altex.indices[m.index_offset as usize..(m.index_offset + m.index_count) as usize].to_vec();
                let skin = skins.get(&mi).map(|s| (s.bones.clone(), s.weights.clone()));
                surfaces.push(Surface { vertices, indices, skin });
            }
            meshes.push(MeshData { surfaces });
        }

        let opt = |i: i32| (i >= 0).then_some(i as usize);
        let xf = |f: &[f32; 12]| {
            Mat4::from_cols(
                Vec4::new(f[0], f[1], f[2], 0.0),
                Vec4::new(f[3], f[4], f[5], 0.0),
                Vec4::new(f[6], f[7], f[8], 0.0),
                Vec4::new(f[9], f[10], f[11], 1.0),
            )
        };
        let mut nodes = Vec::with_capacity(ext.nodes.len());
        for n in &ext.nodes {
            let kind = match n.kind {
                NodeKind::Node => Kind::Node,
                NodeKind::Spatial => Kind::Spatial,
                NodeKind::Mesh => Kind::Mesh,
                NodeKind::Light => Kind::Light,
                NodeKind::Camera => Kind::Camera,
                NodeKind::Canvas => Kind::Canvas,
                NodeKind::Audio => Kind::Audio,
                NodeKind::Skeleton => Kind::Skeleton,
                NodeKind::Label => Kind::Label,
            };
            let light = match (&alfar, opt(n.light)) {
                (Some(f), Some(i)) => {
                    let l = f.lights.get(i).ok_or_else(|| format!("{}: нет источника {} в .alfar", path, i))?;
                    Some(LightInfo {
                        spot: l.light_type == alkash3d_rs::LightType::Spot as u32,
                        color: l.color,
                        energy: l.intensity,
                        range: l.range,
                        angle: l.spot_outer_angle,
                        attenuation: l.falloff_custom,
                        shadow: l.casts_shadows != 0,
                    })
                }
                (None, Some(_)) => return Err(format!("{}: в сцене есть свет, но не прочитан {}", path, alfar_path.display())),
                _ => None,
            };
            nodes.push(Node {
                name: n.name.clone(),
                parent: opt(n.parent),
                kind,
                visible: n.flags & alkash3d_rs::altex_scene::NODE_VISIBLE != 0,
                top_level: n.flags & alkash3d_rs::altex_scene::NODE_TOP_LEVEL != 0,
                xf: xf(&n.xf),
                mesh: opt(n.mesh_group),
                materials: n.materials.clone(),
                light,
                fov: n.fov,
                audio: n.audio.clone(),
                skeleton: opt(n.skeleton),
                binds: n.binds.iter().map(|(b, m)| (*b, xf(m))).collect(),
                bones: n
                    .bones
                    .iter()
                    .map(|b| Bone {
                        name: b.name.clone(),
                        parent: opt(b.parent),
                        rest: xf(&b.rest),
                        pos: Vec3::from(b.pos),
                        rot: Quat::from_array(b.rot),
                        scale: Vec3::from(b.scale),
                    })
                    .collect(),
                label: n.label.as_ref().map(|l| LabelInfo {
                    text: l.text.clone(),
                    font_size: l.font_size,
                    pixel_size: l.pixel_size,
                    color: l.color,
                    align: l.align,
                    billboard: l.billboard,
                }),
            });
        }

        let value = |v: &AnimValue| match v {
            AnimValue::None => Value::None,
            AnimValue::Float(f) => Value::Float(*f),
            AnimValue::Vec3(a) => Value::Vec3(Vec3::from(*a)),
            AnimValue::Quat(a) => Value::Quat(Quat::from_array(*a)),
            AnimValue::Color(a) => Value::Color(*a),
            AnimValue::Xf(a) => Value::Xf(xf(a)),
            AnimValue::Str(s) => Value::Str(s.clone()),
            AnimValue::Vec2(a) => Value::Vec2(*a),
        };
        let mut players = Vec::with_capacity(ext.players.len());
        for p in &ext.players {
            let node = opt(p.node).ok_or("AnimationPlayer без узла")?;
            let animations = p
                .animations
                .iter()
                .map(|a| Animation {
                    name: a.name.clone(),
                    length: a.length,
                    looped: a.looped,
                    tracks: a
                        .tracks
                        .iter()
                        .map(|t| Track {
                            target: opt(t.target),
                            path: t.path.clone(),
                            type_id: t.track_type,
                            property: t.path.split_once(':').map(|(_, p)| p.to_string()).unwrap_or_default(),
                            kind: match t.track_type {
                                0 | 6 => TrackType::Value,
                                1 => TrackType::Position,
                                2 => TrackType::Rotation,
                                3 => TrackType::Scale,
                                _ => TrackType::Other,
                            },
                            interp: t.interp,
                            keys: match &t.keys {
                                TrackKeys::Value(k) => Keys::Value(k.iter().map(|(time, e, v)| (*time, *e, value(v))).collect()),
                                TrackKeys::Method(k) => Keys::Method(k.iter().map(|(time, m, args)| (*time, m.clone(), args.iter().map(value).collect())).collect()),
                                TrackKeys::Audio(k) => Keys::Audio(k.clone()),
                            },
                        })
                        .collect(),
                })
                .collect();
            players.push(Player { node, speed: p.speed, animations });
        }

        Ok(Self { textures, materials, meshes, nodes, players })
    }

    /// Полный путь узла вида "main/tabletop parent/main tabletop".
    pub fn path(&self, mut i: usize) -> String {
        let mut parts = vec![self.nodes[i].name.as_str()];
        while let Some(p) = self.nodes[i].parent {
            parts.push(self.nodes[p].name.as_str());
            i = p;
        }
        parts.reverse();
        parts.join("/")
    }
}
