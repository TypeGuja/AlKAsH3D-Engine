//! altex_scene.rs — блок сцены `.altex` (версия 3): дерево узлов,
//! скелеты, скин-веса, анимации, текст в мире и то, чего нет в POD-
//! `altex_format::Material` (прозрачность, unshaded, UV-трансформ).
//!
//! Геометрия, материалы и текстуры остаются в обычных блоках `.altex`, блок
//! сцены только ссылается на них индексами. Свет хранится в `.alfar` рядом
//! (тот же путь, расширение `.alfar`): узел-источник держит индекс своего
//! `IndividualLight`, сам источник подписан полным путём узла.
//!
//! Формат блока — little-endian поток: сигнатура `ALTSCENE`, версия, затем
//! массивы с u32-счётчиком впереди; строки — u32-длина + UTF-8 без нуля;
//! индексы — i32, -1 = нет; трансформ — 12 f32 (базис по столбцам + сдвиг).

pub const SCENE_MAGIC: &[u8; 8] = b"ALTSCENE";
pub const SCENE_VERSION: u32 = 1;

/// Трансформ 3x4: столбцы X, Y, Z базиса, затем сдвиг.
pub type Xform = [f32; 12];

/// Тип узла (совпадает с порядком в дампе Godot-сцены).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Node = 0,
    Spatial = 1,
    Mesh = 2,
    Light = 3,
    Camera = 4,
    Canvas = 5,
    Audio = 6,
    Skeleton = 7,
    Label = 8,
}

impl NodeKind {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Node,
            1 => Self::Spatial,
            2 => Self::Mesh,
            3 => Self::Light,
            4 => Self::Camera,
            5 => Self::Canvas,
            7 => Self::Skeleton,
            8 => Self::Label,
            _ => Self::Audio,
        }
    }
}

pub const NODE_VISIBLE: u32 = 1;
pub const NODE_TOP_LEVEL: u32 = 2;

/// Набор поверхностей одного исходного меша: `count` подряд идущих
/// `altex_format::Mesh`, начиная с `first`.
#[derive(Debug, Clone, Copy)]
pub struct MeshGroup {
    pub first: u32,
    pub count: u32,
}

/// Скин одной поверхности (`altex_format::Mesh` с индексом `mesh`):
/// 4 индекса привязок и 4 веса на вершину.
#[derive(Debug, Clone)]
pub struct MeshSkin {
    pub mesh: u32,
    pub bones: Vec<[u32; 4]>,
    pub weights: Vec<[f32; 4]>,
}

/// Поля материала сверх `altex_format::Material` (по индексу материала).
#[derive(Debug, Clone, Copy)]
pub struct MaterialExtra {
    /// 0 — непрозрачный, 2 — alpha scissor, остальное — смешивание.
    pub transparency: u32,
    pub unshaded: u32,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub rest: Xform,
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct Label {
    pub text: String,
    pub font_size: f32,
    pub pixel_size: f32,
    pub color: [f32; 4],
    pub align: u32,
    pub billboard: u32,
}

#[derive(Debug, Clone)]
pub struct SceneNode {
    pub name: String,
    pub parent: i32,
    pub kind: NodeKind,
    pub flags: u32,
    pub xf: Xform,
    /// Индекс в `AltexScene::mesh_groups`.
    pub mesh_group: i32,
    /// Материал на каждую поверхность (-1 — материал меша).
    pub materials: Vec<i32>,
    /// Индекс источника в `.alfar` рядом.
    pub light: i32,
    pub fov: f32,
    pub audio: String,
    /// Узел-скелет скиннингованного меша.
    pub skeleton: i32,
    /// Привязки скина: (кость, bind pose).
    pub binds: Vec<(i32, Xform)>,
    pub bones: Vec<Bone>,
    pub label: Option<Label>,
}

#[derive(Debug, Clone)]
pub enum AnimValue {
    None,
    Float(f32),
    Vec3([f32; 3]),
    Quat([f32; 4]),
    Color([f32; 4]),
    Xf(Xform),
    Str(String),
    Vec2([f32; 2]),
}

#[derive(Debug, Clone)]
pub enum TrackKeys {
    /// (время, easing, значение).
    Value(Vec<(f32, f32, AnimValue)>),
    /// (время, метод, аргументы).
    Method(Vec<(f32, String, Vec<AnimValue>)>),
    /// (время, путь к звуку).
    Audio(Vec<(f32, String)>),
}

/// Тип дорожки (как Godot Animation.TrackType).
pub const TRACK_VALUE: u32 = 0;
pub const TRACK_POSITION: u32 = 1;
pub const TRACK_ROTATION: u32 = 2;
pub const TRACK_SCALE: u32 = 3;
pub const TRACK_METHOD: u32 = 5;
pub const TRACK_BEZIER: u32 = 6;
pub const TRACK_AUDIO: u32 = 7;

#[derive(Debug, Clone)]
pub struct Track {
    pub target: i32,
    /// NodePath дорожки ("../door:rotation").
    pub path: String,
    pub track_type: u32,
    pub interp: u32,
    pub keys: TrackKeys,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub length: f32,
    pub looped: bool,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct AnimPlayer {
    pub node: i32,
    pub speed: f32,
    pub animations: Vec<Animation>,
}

#[derive(Debug, Clone, Default)]
pub struct AltexScene {
    pub nodes: Vec<SceneNode>,
    pub mesh_groups: Vec<MeshGroup>,
    pub skins: Vec<MeshSkin>,
    pub material_extras: Vec<MaterialExtra>,
    pub players: Vec<AnimPlayer>,
}

struct W(Vec<u8>);

impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(n as u32);
    }
    fn value(&mut self, v: &AnimValue) {
        match v {
            AnimValue::None => self.u32(0),
            AnimValue::Float(f) => {
                self.u32(1);
                self.f32s(&[*f]);
            }
            AnimValue::Vec3(a) => {
                self.u32(2);
                self.f32s(a);
            }
            AnimValue::Quat(a) => {
                self.u32(3);
                self.f32s(a);
            }
            AnimValue::Color(a) => {
                self.u32(4);
                self.f32s(a);
            }
            AnimValue::Xf(a) => {
                self.u32(5);
                self.f32s(a);
            }
            AnimValue::Str(s) => {
                self.u32(6);
                self.str(s);
            }
            AnimValue::Vec2(a) => {
                self.u32(7);
                self.f32s(a);
            }
        }
    }
}

struct R<'a> {
    data: &'a [u8],
    pos: usize,
}

fn bad(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> std::io::Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return Err(bad(format!("блок сцены .altex обрывается на байте {}", self.pos)));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self) -> std::io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> std::io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> std::io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32s<const N: usize>(&mut self) -> std::io::Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in out.iter_mut() {
            *v = self.f32()?;
        }
        Ok(out)
    }
    fn str(&mut self) -> std::io::Result<String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    /// Счётчик массива; защита от мусора — каждый элемент занимает хотя бы байт.
    fn len(&mut self) -> std::io::Result<usize> {
        let n = self.u32()? as usize;
        if n > self.data.len() - self.pos {
            return Err(bad(format!("неверный счётчик {} на байте {}", n, self.pos)));
        }
        Ok(n)
    }
    fn value(&mut self) -> std::io::Result<AnimValue> {
        Ok(match self.u32()? {
            0 => AnimValue::None,
            1 => AnimValue::Float(self.f32()?),
            2 => AnimValue::Vec3(self.f32s()?),
            3 => AnimValue::Quat(self.f32s()?),
            4 => AnimValue::Color(self.f32s()?),
            5 => AnimValue::Xf(self.f32s()?),
            6 => AnimValue::Str(self.str()?),
            7 => AnimValue::Vec2(self.f32s()?),
            t => return Err(bad(format!("неизвестный тип значения {} на байте {}", t, self.pos))),
        })
    }
}

impl AltexScene {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.0.extend_from_slice(SCENE_MAGIC);
        w.u32(SCENE_VERSION);

        w.len(self.mesh_groups.len());
        for g in &self.mesh_groups {
            w.u32(g.first);
            w.u32(g.count);
        }

        w.len(self.skins.len());
        for s in &self.skins {
            w.u32(s.mesh);
            w.len(s.bones.len());
            for (b, wt) in s.bones.iter().zip(&s.weights) {
                for i in b {
                    w.u32(*i);
                }
                w.f32s(wt);
            }
        }

        w.len(self.material_extras.len());
        for m in &self.material_extras {
            w.u32(m.transparency);
            w.u32(m.unshaded);
            w.f32s(&m.uv_scale);
            w.f32s(&m.uv_offset);
        }

        w.len(self.nodes.len());
        for n in &self.nodes {
            w.str(&n.name);
            w.i32(n.parent);
            w.u32(n.kind as u32);
            w.u32(n.flags);
            w.f32s(&n.xf);
            w.i32(n.mesh_group);
            w.len(n.materials.len());
            for m in &n.materials {
                w.i32(*m);
            }
            w.i32(n.light);
            w.f32s(&[n.fov]);
            w.str(&n.audio);
            w.i32(n.skeleton);
            w.len(n.binds.len());
            for (b, xf) in &n.binds {
                w.i32(*b);
                w.f32s(xf);
            }
            w.len(n.bones.len());
            for b in &n.bones {
                w.str(&b.name);
                w.i32(b.parent);
                w.f32s(&b.rest);
                w.f32s(&b.pos);
                w.f32s(&b.rot);
                w.f32s(&b.scale);
            }
            match &n.label {
                None => w.u32(0),
                Some(l) => {
                    w.u32(1);
                    w.str(&l.text);
                    w.f32s(&[l.font_size, l.pixel_size]);
                    w.f32s(&l.color);
                    w.u32(l.align);
                    w.u32(l.billboard);
                }
            }
        }

        w.len(self.players.len());
        for p in &self.players {
            w.i32(p.node);
            w.f32s(&[p.speed]);
            w.len(p.animations.len());
            for a in &p.animations {
                w.str(&a.name);
                w.f32s(&[a.length]);
                w.u32(a.looped as u32);
                w.len(a.tracks.len());
                for t in &a.tracks {
                    w.i32(t.target);
                    w.str(&t.path);
                    w.u32(t.track_type);
                    w.u32(t.interp);
                    match &t.keys {
                        TrackKeys::Value(k) => {
                            w.u32(0);
                            w.len(k.len());
                            for (time, ease, v) in k {
                                w.f32s(&[*time, *ease]);
                                w.value(v);
                            }
                        }
                        TrackKeys::Method(k) => {
                            w.u32(1);
                            w.len(k.len());
                            for (time, m, args) in k {
                                w.f32s(&[*time]);
                                w.str(m);
                                w.len(args.len());
                                for v in args {
                                    w.value(v);
                                }
                            }
                        }
                        TrackKeys::Audio(k) => {
                            w.u32(2);
                            w.len(k.len());
                            for (time, s) in k {
                                w.f32s(&[*time]);
                                w.str(s);
                            }
                        }
                    }
                }
            }
        }
        w.0
    }

    pub fn from_bytes(data: &[u8]) -> std::io::Result<Self> {
        let mut r = R { data, pos: 0 };
        if r.take(8)? != SCENE_MAGIC {
            return Err(bad("блок сцены .altex: неверная сигнатура".into()));
        }
        let version = r.u32()?;
        if version != SCENE_VERSION {
            return Err(bad(format!("блок сцены .altex: версия {}, поддерживается {}", version, SCENE_VERSION)));
        }
        let mut scene = AltexScene::default();

        for _ in 0..r.len()? {
            scene.mesh_groups.push(MeshGroup { first: r.u32()?, count: r.u32()? });
        }

        for _ in 0..r.len()? {
            let mesh = r.u32()?;
            let n = r.len()?;
            let mut bones = Vec::with_capacity(n);
            let mut weights = Vec::with_capacity(n);
            for _ in 0..n {
                bones.push([r.u32()?, r.u32()?, r.u32()?, r.u32()?]);
                weights.push(r.f32s()?);
            }
            scene.skins.push(MeshSkin { mesh, bones, weights });
        }

        for _ in 0..r.len()? {
            let transparency = r.u32()?;
            let unshaded = r.u32()?;
            let uv_scale = r.f32s()?;
            let uv_offset = r.f32s()?;
            scene.material_extras.push(MaterialExtra { transparency, unshaded, uv_scale, uv_offset });
        }

        for _ in 0..r.len()? {
            let name = r.str()?;
            let parent = r.i32()?;
            let kind = NodeKind::from_u32(r.u32()?);
            let flags = r.u32()?;
            let xf = r.f32s()?;
            let mesh_group = r.i32()?;
            let mut materials = Vec::new();
            for _ in 0..r.len()? {
                materials.push(r.i32()?);
            }
            let light = r.i32()?;
            let fov = r.f32()?;
            let audio = r.str()?;
            let skeleton = r.i32()?;
            let mut binds = Vec::new();
            for _ in 0..r.len()? {
                let b = r.i32()?;
                binds.push((b, r.f32s()?));
            }
            let mut bones = Vec::new();
            for _ in 0..r.len()? {
                bones.push(Bone {
                    name: r.str()?,
                    parent: r.i32()?,
                    rest: r.f32s()?,
                    pos: r.f32s()?,
                    rot: r.f32s()?,
                    scale: r.f32s()?,
                });
            }
            let label = if r.u32()? == 1 {
                Some(Label {
                    text: r.str()?,
                    font_size: r.f32()?,
                    pixel_size: r.f32()?,
                    color: r.f32s()?,
                    align: r.u32()?,
                    billboard: r.u32()?,
                })
            } else {
                None
            };
            scene.nodes.push(SceneNode { name, parent, kind, flags, xf, mesh_group, materials, light, fov, audio, skeleton, binds, bones, label });
        }

        for _ in 0..r.len()? {
            let node = r.i32()?;
            let speed = r.f32()?;
            let mut animations = Vec::new();
            for _ in 0..r.len()? {
                let name = r.str()?;
                let length = r.f32()?;
                let looped = r.u32()? != 0;
                let mut tracks = Vec::new();
                for _ in 0..r.len()? {
                    let target = r.i32()?;
                    let path = r.str()?;
                    let track_type = r.u32()?;
                    let interp = r.u32()?;
                    let keys = match r.u32()? {
                        0 => {
                            let mut k = Vec::new();
                            for _ in 0..r.len()? {
                                let time = r.f32()?;
                                let ease = r.f32()?;
                                k.push((time, ease, r.value()?));
                            }
                            TrackKeys::Value(k)
                        }
                        1 => {
                            let mut k = Vec::new();
                            for _ in 0..r.len()? {
                                let time = r.f32()?;
                                let m = r.str()?;
                                let mut args = Vec::new();
                                for _ in 0..r.len()? {
                                    args.push(r.value()?);
                                }
                                k.push((time, m, args));
                            }
                            TrackKeys::Method(k)
                        }
                        2 => {
                            let mut k = Vec::new();
                            for _ in 0..r.len()? {
                                let time = r.f32()?;
                                k.push((time, r.str()?));
                            }
                            TrackKeys::Audio(k)
                        }
                        t => return Err(bad(format!("неизвестный вид ключей {} на байте {}", t, r.pos))),
                    };
                    tracks.push(Track { target, path, track_type, interp, keys });
                }
                animations.push(Animation { name, length, looped, tracks });
            }
            scene.players.push(AnimPlayer { node, speed, animations });
        }

        if r.pos != data.len() {
            return Err(bad(format!("блок сцены .altex: лишние {} байт в конце", data.len() - r.pos)));
        }
        Ok(scene)
    }

    /// Полный путь узла вида "main/tabletop parent/main tabletop".
    pub fn path(&self, mut i: usize) -> String {
        let mut parts = vec![self.nodes[i].name.as_str()];
        while self.nodes[i].parent >= 0 {
            i = self.nodes[i].parent as usize;
            parts.push(self.nodes[i].name.as_str());
        }
        parts.reverse();
        parts.join("/")
    }
}
