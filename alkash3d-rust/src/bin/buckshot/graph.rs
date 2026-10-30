//! Граф сцены поверх движка: дерево узлов Godot с локальными трансформами,
//! которые меняют анимации и игровой код, и вывод его в `mesh_instances`.
//!
//! Узлы делятся на два класса:
//! - статические (ни один AnimationPlayer их не двигает и игра не трогает)
//!   — запекаются в мировые координаты прямо в вершины один раз; трансформ
//!   инстанса единичный, без потерь на разложении базиса;
//! - динамические — меш в локальной системе узла, инстанс каждый кадр
//!   получает мировой трансформ, разложенный на позицию/поворот/масштаб.
//!
//! Префабы (предметы, гильзы) — отдельные поддеревья "prefab:*", которые
//! сами не рисуются, а размножаются `spawn()` под любым узлом.

use std::collections::{HashMap, HashSet};

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::{EulerRot, Mat3, Mat4, Quat, Vec3};
use alkash3d_rs::GPULight;

use crate::scene_file::{Keys, Kind, LightInfo, Material, SceneFile, Surface, TrackType};

#[derive(Clone, Copy, Debug)]
pub struct Trs {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}

impl Trs {
    pub fn from_mat(m: Mat4) -> Self {
        let (scale, rot, pos) = m.to_scale_rotation_translation();
        Trs { pos, rot, scale }
    }
    pub fn mat(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }
}

/// Godot `rotation` (радианы, порядок YXZ: R = Ry * Rx * Rz) -> кватернион.
pub fn euler_yxz(r: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, r.y, r.x, r.z)
}

/// Обратно: кватернион -> Godot `rotation` (x, y, z).
pub fn quat_to_yxz(q: Quat) -> Vec3 {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    Vec3::new(x, y, z)
}

pub struct GNode {
    pub name: String,
    pub parent: Option<usize>,
    pub kind: Kind,
    pub local: Trs,
    /// Последнее значение `rotation`, выставленное анимацией/кодом — нужно,
    /// чтобы интерполировать эйлеровы углы так же, как Godot (покомпонентно),
    /// а не через кватернион.
    pub euler: Vec3,
    pub visible: bool,
    pub top_level: bool,
    pub dynamic: bool,
    pub fov: f32,
    pub light: Option<LightInfo>,
    pub light_energy: f32,
    /// Множитель перевода энергии Godot в интенсивность движка (см. `convert_intensity`).
    pub light_calib: f32,
    pub light_id: Option<u32>,
    pub light_sent: Option<[f32; 11]>,
    /// Меши в локальной системе узла (только у динамических).
    pub meshes: Vec<usize>,
    /// Гильзы: варианты с цветом поверхности 1 — [боевая, холостая].
    pub variants: [Vec<usize>; 2],
    /// 0 — исходный материал, 1 — боевая, 2 — холостая.
    pub variant: u8,
    /// Не рисовать меши самого узла, но не трогать детей — аналог
    /// `set_layer_mask_value` в Godot (дилер меняет голову на раздавленную,
    /// а раздавленная — дочерний узел обычной).
    pub hide_self: bool,
    /// Множитель цвета/непрозрачность мешей узла (GeometryInstance3D.transparency,
    /// material_override:albedo_color из анимаций). [1,1,1,1] — как в материале.
    pub tint: [f32; 4],
    /// Радиус лампы, если его двигает анимация (omni_range); иначе из сцены.
    pub light_range: Option<f32>,
    /// Линейный albedo_color первого материала узла: анимация albedo_color в
    /// Godot ЗАМЕНЯЕТ цвет, а у нас цвет запечён в вершины — tint = новый / базовый.
    pub base_albedo: [f32; 4],
    /// Skeleton3D: кости и их исходные позы.
    pub bones: Vec<BoneRt>,
    pub bones_initial: Vec<BoneRt>,
    /// Скиннингованный меш: деформируется на CPU каждый кадр.
    pub skin: Option<SkinRt>,
    /// Label3D и текст, выставленный кодом игры (имя игрока, буквы бланка).
    pub label: Option<crate::scene_file::LabelInfo>,
    pub label_text: Option<String>,
    /// Поток AudioStreamPlayer (путь ресурса), если узел — проигрыватель звука.
    pub audio_path: String,
}

/// Кость скелета в рантайме: текущая локальная поза (её двигают анимации).
#[derive(Clone)]
pub struct BoneRt {
    pub name: String,
    pub parent: Option<usize>,
    pub pose: Trs,
}

/// Поверхность скиннингованного меша: исходные вершины (в системе узла, с
/// уже запечёнными цветом/UV), влияния костей и кольцо из трёх мешей
/// движка — на каждый кадр пишется следующий, пока GPU рисует предыдущие.
pub struct SkinSurface {
    pub base: Vec<Vertex>,
    pub bones: Vec<[u32; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub ring: [usize; 3],
}

pub struct SkinRt {
    pub skeleton: usize,
    /// Привязки: (индекс кости, bind pose).
    pub binds: Vec<(usize, Mat4)>,
    pub surfaces: Vec<SkinSurface>,
}

pub struct Spawned {
    pub template: usize,
    pub parent: usize,
    pub local: Trs,
    pub variant: u8,
    pub visible: bool,
}

pub struct Graph {
    pub nodes: Vec<GNode>,
    pub by_path: HashMap<String, usize>,
    pub global: Vec<Mat4>,
    pub vis: Vec<bool>,
    pub static_meshes: Vec<usize>,
    pub spawned: HashMap<u32, Spawned>,
    children: Vec<Vec<usize>>,
    template: Vec<bool>,
    next_spawn: u32,
    pub root_name: String,
    light_scale: f32,
    skin_frame: usize,
    /// Исходное состояние узлов для `reset` (локальный трансформ, эйлер, видимость, fov, энергия).
    initial: Vec<(Trs, Vec3, bool, f32, f32)>,
    initial_tint: Vec<[f32; 4]>,
}

/// Альфа материала узла, если он реально прозрачный (иначе 1): у динамических
/// узлов она живёт в tint, чтобы анимации могли её менять.
fn base_alpha(scene: &SceneFile, s: &crate::scene_file::Node) -> f32 {
    s.materials
        .iter()
        .filter(|&&m| m >= 0)
        .map(|&m| &scene.materials[m as usize])
        .find(|m| m.transparency != 0 && m.transparency != 2)
        .map(|m| m.albedo[3])
        .unwrap_or(1.0)
}

fn base_albedo(scene: &SceneFile, s: &crate::scene_file::Node) -> [f32; 4] {
    s.materials
        .iter()
        .find(|&&m| m >= 0)
        .map(|&m| {
            let a = scene.materials[m as usize].albedo;
            [srgb_to_linear(a[0]), srgb_to_linear(a[1]), srgb_to_linear(a[2]), a[3]]
        })
        .unwrap_or([1.0; 4])
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Перевод Godot omni light (затухание d^-attenuation, энергия в
/// "игровых" единицах) в физический точечный источник движка (1/d²):
/// подгоняем так, чтобы на расстоянии `ref_dist` освещённость совпадала.
/// Это калибровка единиц, а не подмена модели — в движке затухание
/// остаётся честным обратным квадратом.
fn convert_intensity(energy: f32, attenuation: f32, ref_dist: f32, scale: f32) -> f32 {
    energy * std::f32::consts::PI * ref_dist.powf(2.0 - attenuation) * scale
}

struct MeshMaker<'a> {
    scene: &'a SceneFile,
    tex_cache: HashMap<i32, Option<u32>>,
    /// Есть ли в текстуре заметная прозрачность (минимальная альфа < 250).
    tex_alpha: HashMap<i32, bool>,
    /// true — альфу материала кладём в вершины (запечённая статика); false —
    /// вершины непрозрачны, альфа идёт через tint узла (её могут менять анимации).
    alpha_in_vertex: bool,
    /// (godot mesh, материалы, вариант цвета) -> меши движка в локальной системе.
    local_cache: HashMap<(usize, Vec<i32>, u8), Vec<usize>>,
}

impl<'a> MeshMaker<'a> {
    fn texture_has_alpha(&mut self, index: i32) -> bool {
        if index < 0 {
            return false;
        }
        if let Some(&a) = self.tex_alpha.get(&index) {
            return a;
        }
        let a = self.scene.textures.get(index as usize).is_some_and(|t| t.rgba.chunks_exact(4).any(|p| p[3] < 250));
        self.tex_alpha.insert(index, a);
        a
    }

    fn texture(&mut self, engine: &mut AlkashEngine, index: i32) -> Option<u32> {
        if index < 0 {
            return None;
        }
        if let Some(t) = self.tex_cache.get(&index) {
            return *t;
        }
        let t = self.scene.textures.get(index as usize).and_then(|t| engine.create_texture_rgba(t.width, t.height, &t.rgba));
        self.tex_cache.insert(index, t);
        t
    }

    fn surface(&mut self, engine: &mut AlkashEngine, surface: &Surface, material: Option<&Material>, xf: Mat4, color_override: Option<[f32; 3]>) -> Option<usize> {
        self.surface_full(engine, surface, material, xf, color_override).map(|(m, _)| m)
    }

    /// То же, но отдаёт и вершины (нужны скиннингу на CPU).
    fn surface_full(&mut self, engine: &mut AlkashEngine, surface: &Surface, material: Option<&Material>, xf: Mat4, color_override: Option<[f32; 3]>) -> Option<(usize, Vec<Vertex>)> {
        if surface.vertices.is_empty() || surface.indices.is_empty() {
            return None;
        }
        // Прозрачные материалы (стекло, дым, лучи света, декали) движок
        // умеет рисовать только непрозрачными — честнее их не показывать,
        // чем закрыть ими сцену чёрными плоскостями.
        // BaseMaterial3D.transparency: 2 (alpha scissor) — отсечение по альфе,
        // остальное ненулевое — смешивание в прозрачном проходе движка.
        let cutout = material.is_some_and(|m| m.transparency == 2 && m.albedo_tex >= 0);
        // Прозрачность по-настоящему нужна, только если альфа < 1 или у текстуры
        // есть прозрачные пиксели: многие материалы помечены прозрачными "на
        // всякий случай" (гильзы, сегмент ружья) — их рисуем непрозрачными, иначе
        // они не пишут глубину и просвечивают.
        let blend = match material {
            Some(m) if m.transparency != 0 && m.transparency != 2 => {
                let tex_alpha = self.texture_has_alpha(m.albedo_tex);
                m.albedo[3] < 0.99 || tex_alpha
            }
            _ => false,
        };
        let default = Material {
            albedo: [0.6, 0.6, 0.6, 1.0],
            albedo_tex: -1,
            normal_tex: -1,
            roughness: 0.8,
            metallic: 0.0,
            emission: [0.0; 3],
            transparency: 0,
            unshaded: false,
            uv_scale: [1.0, 1.0],
            uv_offset: [0.0, 0.0],
        };
        let m = material.unwrap_or(&default);
        let albedo = color_override.unwrap_or([m.albedo[0], m.albedo[1], m.albedo[2]]);
        let color = [srgb_to_linear(albedo[0]), srgb_to_linear(albedo[1]), srgb_to_linear(albedo[2])];
        let alpha = if blend && self.alpha_in_vertex { m.albedo[3] } else { 1.0 };

        let basis = Mat3::from_mat4(xf);
        let normal_mat = basis.inverse().transpose();
        let flip = if basis.determinant() < 0.0 { -1.0 } else { 1.0 };
        let vertices: Vec<Vertex> = surface
            .vertices
            .iter()
            .map(|v| {
                let p = xf.transform_point3(Vec3::new(v[0], v[1], v[2]));
                let n = (normal_mat * Vec3::new(v[3], v[4], v[5])).normalize_or_zero();
                let t = (basis * Vec3::new(v[8], v[9], v[10])).normalize_or_zero();
                let u = v[6] * m.uv_scale[0] + m.uv_offset[0];
                let w = v[7] * m.uv_scale[1] + m.uv_offset[1];
                Vertex::with_normal_uv_tangent(
                    p.x, p.y, p.z, n.x, n.y, n.z, color[0], color[1], color[2], alpha, u, w,
                    [t.x, t.y, t.z, v[11] * flip],
                )
            })
            .collect();

        let mut mesh = Mesh::from_vertices_and_indices(&vertices, &surface.indices).ok()?;
        mesh.albedo_srv_index = self.texture(engine, m.albedo_tex);
        mesh.normal_srv_index = self.texture(engine, m.normal_tex);
        mesh.material_roughness = m.roughness.clamp(0.05, 1.0);
        mesh.material_metallic = m.metallic.clamp(0.0, 1.0);
        mesh.material_emissive = if m.unshaded { [1.0, 1.0, 1.0] } else { m.emission };
        if cutout {
            mesh.alpha_cutoff = 0.5;
        }
        mesh.blend = blend;
        // Гладкое прозрачное без текстуры — стекло: альфа = доля рассеяния,
        // отражение по Френелю от неё не зависит (`Mesh::glass`).
        mesh.glass = blend && m.albedo_tex < 0 && m.roughness < 0.1 && m.metallic < 0.01 && !m.unshaded;
        Some((engine.add_mesh(mesh), vertices))
    }

    /// Все поверхности узла, вершины умножены на `xf`.
    fn node_meshes(&mut self, engine: &mut AlkashEngine, node: &crate::scene_file::Node, xf: Mat4, variant: u8) -> Vec<usize> {
        let scene = self.scene;
        let Some(mi) = node.mesh else { return Vec::new() };
        let mut out = Vec::new();
        for (si, surface) in scene.meshes[mi].surfaces.iter().enumerate() {
            let mat = node.materials.get(si).copied().filter(|&m| m >= 0).map(|m| &scene.materials[m as usize]);
            // mat_shell live/blank.tres — красный и синий пластик гильзы.
            let over = match (variant, si) {
                (1, 1) => Some([1.0, 0.282, 0.298]),
                (2, 1) => Some([0.298, 0.51, 1.0]),
                _ => None,
            };
            if let Some(m) = self.surface(engine, surface, mat, xf, over) {
                out.push(m);
            }
        }
        out
    }

    fn local_meshes(&mut self, engine: &mut AlkashEngine, node: &crate::scene_file::Node, variant: u8) -> Vec<usize> {
        let Some(mi) = node.mesh else { return Vec::new() };
        let key = (mi, node.materials.clone(), variant);
        if let Some(v) = self.local_cache.get(&key) {
            return v.clone();
        }
        let v = self.node_meshes(engine, node, Mat4::IDENTITY, variant);
        self.local_cache.insert(key, v.clone());
        v
    }
}

impl Graph {
    /// `dynamic_roots` — пути (от корня сцены), которые игра двигает или
    /// прячет сама; всё под ними тоже динамическое.
    pub fn build(engine: &mut AlkashEngine, scene: &SceneFile, dynamic_roots: &[&str], light_scale: f32) -> Graph {
        let n = scene.nodes.len();
        let mut by_path = HashMap::with_capacity(n);
        for i in 0..n {
            by_path.insert(scene.path(i), i);
        }
        let root_name = scene.nodes.first().map(|n| n.name.clone()).unwrap_or_default();

        // Какие узлы двигают анимации.
        let mut animated: HashSet<usize> = HashSet::new();
        for p in &scene.players {
            for a in &p.animations {
                for t in &a.tracks {
                    let Some(target) = t.target else { continue };
                    let moves = matches!(t.kind, TrackType::Position | TrackType::Rotation | TrackType::Scale)
                        || matches!(
                            t.property.as_str(),
                            "position" | "rotation" | "rotation_degrees" | "quaternion" | "scale" | "transform" | "visible"
                        );
                    if moves && matches!(t.keys, Keys::Value(_)) {
                        animated.insert(target);
                    }
                }
            }
        }
        for r in dynamic_roots {
            if let Some(&i) = by_path.get(&format!("{}/{}", root_name, r)) {
                animated.insert(i);
            }
        }
        let mut dynamic = vec![false; n];
        for i in 0..n {
            let node = &scene.nodes[i];
            let prefab = node.name.starts_with("prefab_");
            dynamic[i] = prefab || animated.contains(&i) || node.parent.is_some_and(|p| dynamic[p]);
        }

        let mut nodes: Vec<GNode> = scene
            .nodes
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let local = Trs::from_mat(s.xf);
                GNode {
                    name: s.name.clone(),
                    parent: s.parent,
                    kind: s.kind,
                    local,
                    euler: quat_to_yxz(local.rot),
                    visible: s.visible,
                    top_level: s.top_level,
                    dynamic: dynamic[i],
                    fov: s.fov,
                    light: s.light,
                    light_energy: s.light.map(|l| l.energy).unwrap_or(0.0),
                    light_calib: 0.0,
                    light_id: None,
                    light_sent: None,
                    meshes: Vec::new(),
                    variants: [Vec::new(), Vec::new()],
                    variant: 0,
                    hide_self: false,
                    tint: [1.0, 1.0, 1.0, base_alpha(scene, s)],
                    light_range: None,
                    base_albedo: base_albedo(scene, s),
                    bones: s
                        .bones
                        .iter()
                        .map(|b| BoneRt { name: b.name.clone(), parent: b.parent, pose: Trs { pos: b.pos, rot: b.rot, scale: b.scale } })
                        .collect(),
                    bones_initial: Vec::new(),
                    skin: None,
                    label: s.label.clone(),
                    label_text: None,
                    audio_path: s.audio.clone(),
                }
            })
            .collect();

        let mut graph = Graph {
            global: vec![Mat4::IDENTITY; n],
            vis: vec![true; n],
            nodes: Vec::new(),
            by_path,
            static_meshes: Vec::new(),
            spawned: HashMap::new(),
            children: vec![Vec::new(); n],
            template: vec![false; n],
            next_spawn: 1,
            root_name,
            light_scale,
            skin_frame: 0,
            initial_tint: Vec::new(),
            initial: Vec::new(),
        };
        std::mem::swap(&mut graph.nodes, &mut nodes);
        graph.initial = graph.nodes.iter().map(|n| (n.local, n.euler, n.visible, n.fov, n.light_energy)).collect();
        graph.initial_tint = graph.nodes.iter().map(|n| n.tint).collect();
        for n in graph.nodes.iter_mut() {
            n.bones_initial = n.bones.clone();
        }
        for i in 0..n {
            if let Some(p) = graph.nodes[i].parent {
                graph.children[p].push(i);
            }
            graph.template[i] = graph.nodes[i].name.starts_with("prefab_") || graph.nodes[i].parent.is_some_and(|p| graph.template[p]);
        }
        graph.update_globals();

        let mut maker = MeshMaker { scene, tex_cache: HashMap::new(), tex_alpha: HashMap::new(), alpha_in_vertex: false, local_cache: HashMap::new() };
        let mut baked = 0;
        for i in 0..n {
            let s = &scene.nodes[i];
            if s.kind != Kind::Mesh {
                continue;
            }
            if graph.nodes[i].dynamic && s.skeleton.is_some() && scene.meshes[s.mesh.unwrap_or(0)].surfaces.iter().any(|x| x.skin.is_some()) {
                let mut surfaces = Vec::new();
                let mi = s.mesh.unwrap();
                for (si, surf) in scene.meshes[mi].surfaces.iter().enumerate() {
                    let Some((bones, weights)) = &surf.skin else { continue };
                    let mat = s.materials.get(si).copied().filter(|&m| m >= 0).map(|m| &scene.materials[m as usize]);
                    let mut ring = [0usize; 3];
                    let mut base = Vec::new();
                    let mut ok = true;
                    for slot in ring.iter_mut() {
                        match maker.surface_full(engine, surf, mat, Mat4::IDENTITY, None) {
                            Some((m, v)) => {
                                engine.meshes[m].cpu_writable = true;
                                *slot = m;
                                base = v;
                            }
                            None => ok = false,
                        }
                    }
                    if ok {
                        surfaces.push(SkinSurface { base, bones: bones.clone(), weights: weights.clone(), ring });
                    }
                }
                let binds = s.binds.iter().filter(|b| b.0 >= 0).map(|&(b, m)| (b as usize, m)).collect();
                graph.nodes[i].skin = Some(SkinRt { skeleton: s.skeleton.unwrap(), binds, surfaces });
            } else if graph.nodes[i].dynamic {
                graph.nodes[i].meshes = maker.local_meshes(engine, s, 0);
                if graph.under_prefab(i, "prefab_shell") || s.name.starts_with("shell parent") {
                    graph.nodes[i].variants = [maker.local_meshes(engine, s, 1), maker.local_meshes(engine, s, 2)];
                }
            } else if graph.vis[i] {
                maker.alpha_in_vertex = true;
                let m = maker.node_meshes(engine, s, graph.global[i], 0);
                maker.alpha_in_vertex = false;
                baked += m.len();
                graph.static_meshes.extend(m);
            }
        }

        // Фонари: все регистрируются сразу, видимость/энергия/позиция
        // обновляются в `sync_lights`.
        let mut light_count = 0;
        for i in 0..n {
            let Some(l) = graph.nodes[i].light else { continue };
            if engine.lights.is_none() {
                break;
            }
            let pos = graph.global[i].w_axis.truncate();
            // Высота лампы над полом/столом — опорное расстояние калибровки.
            let ref_dist = (pos.y + 0.6).clamp(2.0, 12.0);
            graph.nodes[i].light_calib = convert_intensity(1.0, l.attenuation, ref_dist, 1.0);
            let id = engine.lights.as_mut().unwrap().add_light(&GPULight {
                position: [pos.x, pos.y, pos.z, 0.0],
                color: [0.0, 0.0, 0.0, 0.0],
                direction: [0.0, -1.0, 0.0, l.range],
                params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
            });
            graph.nodes[i].light_id = Some(id);
            light_count += 1;
        }

        println!(
            "[BUCKSHOT] сцена '{}': узлов {}, динамических {}, запечено мешей {}, текстур {}, фонарей {}",
            graph.root_name,
            n,
            graph.nodes.iter().filter(|n| n.dynamic).count(),
            baked,
            maker.tex_cache.values().filter(|t| t.is_some()).count(),
            light_count
        );
        graph
    }

    fn pick(node: &GNode, variant: u8) -> &[usize] {
        match variant {
            1 | 2 if !node.variants[variant as usize - 1].is_empty() => &node.variants[variant as usize - 1],
            _ => &node.meshes,
        }
    }

    fn under_prefab(&self, mut i: usize, prefab: &str) -> bool {
        loop {
            if self.nodes[i].name == prefab {
                return true;
            }
            match self.nodes[i].parent {
                Some(p) => i = p,
                None => return false,
            }
        }
    }

    /// Индекс узла по пути от корня сцены ("tabletop parent/main tabletop").
    pub fn find(&self, path: &str) -> Option<usize> {
        if path.is_empty() {
            return Some(0);
        }
        self.by_path.get(&format!("{}/{}", self.root_name, path)).copied()
    }

    pub fn node(&self, path: &str) -> usize {
        self.find(path).unwrap_or_else(|| panic!("в сцене '{}' нет узла '{}'", self.root_name, path))
    }

    /// Label3D, которые живут в 3D (не внутри 2D-интерфейса Control).
    pub fn world_labels(&self) -> Vec<usize> {
        (0..self.nodes.len())
            .filter(|&i| self.nodes[i].label.is_some() && !self.template[i])
            .filter(|&i| {
                let mut p = self.nodes[i].parent;
                while let Some(j) = p {
                    if self.nodes[j].kind == Kind::Canvas {
                        return false;
                    }
                    p = self.nodes[j].parent;
                }
                true
            })
            .collect()
    }

    /// Текст Label3D (как `label.text = …` в скриптах).
    pub fn set_label(&mut self, path: &str, text: &str) {
        if let Some(i) = self.find(path) {
            self.nodes[i].label_text = Some(text.to_string());
        }
    }

    pub fn set_visible(&mut self, path: &str, v: bool) {
        if let Some(i) = self.find(path) {
            if !self.nodes[i].dynamic {
                eprintln!("[BUCKSHOT] set_visible('{}') у статического узла — добавь его в dynamic_roots", path);
            }
            self.nodes[i].visible = v;
        }
    }

    pub fn initial_pos(&self, i: usize) -> Vec3 {
        self.initial[i].0.pos
    }

    /// Вернуть сцену в состояние из файла (в оригинале — перезагрузка main.tscn).
    pub fn reset(&mut self) {
        for (n, &(local, euler, visible, fov, energy)) in self.nodes.iter_mut().zip(self.initial.iter()) {
            n.local = local;
            n.euler = euler;
            n.visible = visible;
            n.fov = fov;
            n.light_energy = energy;
            n.variant = 0;
            n.hide_self = false;
            n.light_range = None;
            n.label_text = None;
            if !n.bones_initial.is_empty() {
                n.bones = n.bones_initial.clone();
            }
        }
        self.spawned.clear();
        for (n, t) in self.nodes.iter_mut().zip(self.initial_tint.iter()) {
            n.tint = *t;
        }
    }

    pub fn set_rotation(&mut self, i: usize, euler: Vec3) {
        self.nodes[i].euler = euler;
        self.nodes[i].local.rot = euler_yxz(euler);
    }

    pub fn update_globals(&mut self) {
        for i in 0..self.nodes.len() {
            let node = &self.nodes[i];
            let local = node.local.mat();
            let (pg, pv) = match node.parent {
                // Node3D под обычным Node/Control — начало новой 3D-цепочки.
                Some(p) if !node.top_level && matches!(self.nodes[p].kind, Kind::Spatial | Kind::Mesh | Kind::Light | Kind::Camera | Kind::Skeleton | Kind::Label) => {
                    (self.global[p], self.vis[p])
                }
                Some(_) => (Mat4::IDENTITY, true),
                None => (Mat4::IDENTITY, true),
            };
            self.global[i] = pg * local;
            self.vis[i] = pv && node.visible;
        }
    }

    /// Мировой трансформ узла шаблона при размножении: корень префаба
    /// заменён на `root` (масштаб корня сохраняется, как в Godot при
    /// `transform.origin = …; rotation_degrees = …`).
    fn template_chain(&self, template: usize, node: usize) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut i = node;
        while i != template {
            m = self.nodes[i].local.mat() * m;
            i = self.nodes[i].parent.expect("узел вне шаблона");
        }
        Mat4::from_scale(self.nodes[template].local.scale) * m
    }

    pub fn spawn(&mut self, prefab: &str, parent: usize, local: Trs, variant: u8) -> u32 {
        let template = self.by_path.get(&format!("prefab_{}", prefab)).copied().unwrap_or_else(|| panic!("нет префаба {}", prefab));
        let id = self.next_spawn;
        self.next_spawn += 1;
        self.spawned.insert(id, Spawned { template, parent, local, variant, visible: true });
        id
    }

    /// Мировые ограничивающие сферы видимых мешей поддерева `root` — для
    /// выбора объекта мышью (в оригинале — коллайдеры InteractionBranch).
    pub fn spheres(&self, meshes: &[Mesh], root: usize, out: &mut Vec<(Vec3, f32)>) {
        if !self.vis[root] {
            return;
        }
        let mut stack = vec![root];
        while let Some(i) = stack.pop() {
            if !self.vis[i] {
                continue;
            }
            let g = self.global[i];
            let scale = g.to_scale_rotation_translation().0.abs().max_element();
            let own: &[usize] = if self.nodes[i].hide_self { &[] } else { Self::pick(&self.nodes[i], self.nodes[i].variant) };
            for &m in own {
                let c = Vec3::from(meshes[m].bounding_center);
                out.push((g.transform_point3(c), meshes[m].bounding_radius * scale));
            }
            stack.extend_from_slice(&self.children[i]);
        }
    }

    /// То же для размноженного префаба.
    pub fn spawn_spheres(&self, meshes: &[Mesh], id: u32, out: &mut Vec<(Vec3, f32)>) {
        let Some(sp) = self.spawned.get(&id) else { return };
        let base = self.global[sp.parent] * sp.local.mat();
        let mut stack = vec![sp.template];
        while let Some(i) = stack.pop() {
            let g = base * self.template_chain(sp.template, i);
            let scale = g.to_scale_rotation_translation().0.abs().max_element();
            for &m in Self::pick(&self.nodes[i], sp.variant) {
                out.push((g.transform_point3(Vec3::from(meshes[m].bounding_center)), meshes[m].bounding_radius * scale));
            }
            stack.extend_from_slice(&self.children[i]);
        }
    }

    pub fn despawn(&mut self, id: u32) {
        self.spawned.remove(&id);
    }

    /// Мировая (в системе скелета) матрица кости.
    fn bone_global(bones: &[BoneRt], i: usize, cache: &mut Vec<Option<Mat4>>) -> Mat4 {
        if let Some(m) = cache[i] {
            return m;
        }
        let local = bones[i].pose.mat();
        let m = match bones[i].parent {
            Some(p) if p < bones.len() => Self::bone_global(bones, p, cache) * local,
            _ => local,
        };
        cache[i] = Some(m);
        m
    }

    /// Деформирует скиннингованные меши по текущим позам костей (CPU) и
    /// пишет результат в очередной меш кольца. Вызывать раз в кадр до
    /// `push_instances`.
    pub fn update_skins(&mut self, engine: &mut AlkashEngine) {
        self.skin_frame = (self.skin_frame + 1) % 3;
        let slot = self.skin_frame;
        for i in 0..self.nodes.len() {
            let Some(skin) = &self.nodes[i].skin else { continue };
            if !self.vis[i] {
                continue;
            }
            let bones = &self.nodes[skin.skeleton].bones;
            let mut cache = vec![None; bones.len()];
            let mats: Vec<Mat4> = skin
                .binds
                .iter()
                .map(|&(b, bind)| if b < bones.len() { Self::bone_global(bones, b, &mut cache) * bind } else { Mat4::IDENTITY })
                .collect();
            for surf in &skin.surfaces {
                let mut out = surf.base.clone();
                for (v, (bi, w)) in out.iter_mut().zip(surf.bones.iter().zip(surf.weights.iter())) {
                    let mut m = Mat4::ZERO;
                    for k in 0..4 {
                        if w[k] > 0.0 {
                            if let Some(bm) = mats.get(bi[k] as usize) {
                                m += *bm * w[k];
                            }
                        }
                    }
                    if m == Mat4::ZERO {
                        continue;
                    }
                    let p = m.transform_point3(Vec3::new(v.position[0], v.position[1], v.position[2]));
                    let n = m.transform_vector3(Vec3::from(v.normal)).normalize_or_zero();
                    let t = m.transform_vector3(Vec3::new(v.tangent[0], v.tangent[1], v.tangent[2])).normalize_or_zero();
                    v.position = [p.x, p.y, p.z, 1.0];
                    v.normal = n.into();
                    v.tangent = [t.x, t.y, t.z, v.tangent[3]];
                }
                // Сфера для отсечения — по деформированным вершинам: у
                // исходных (bind pose) она в другом месте, и вблизи меш
                // целиком отсекался как невидимый.
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for v in &out {
                    let p = Vec3::new(v.position[0], v.position[1], v.position[2]);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                let mesh = &mut engine.meshes[surf.ring[slot]];
                if !out.is_empty() {
                    mesh.bounding_center = ((lo + hi) * 0.5).into();
                    mesh.bounding_radius = (hi - lo).length() * 0.5;
                }
                let _ = mesh.update_vertices(&out);
            }
        }
    }

    pub fn push_instances(&self, out: &mut Vec<MeshInstance>) {
        for &m in &self.static_meshes {
            out.push(MeshInstance::new(m));
        }
        let push = |out: &mut Vec<MeshInstance>, meshes: &[usize], g: Mat4, tint: [f32; 4]| {
            if tint[3] <= 0.001 {
                return;
            }
            let (s, r, t) = g.to_scale_rotation_translation();
            let (rz, ry, rx) = r.to_euler(EulerRot::ZYX);
            for &m in meshes {
                out.push(MeshInstance::new(m).at(t.x, t.y, t.z).rotated(rx, ry, rz).scaled(s.x, s.y, s.z).tinted(tint[0], tint[1], tint[2], tint[3]));
            }
        };
        for (i, node) in self.nodes.iter().enumerate() {
            if node.dynamic && self.vis[i] && !node.hide_self && !node.meshes.is_empty() && !self.template[i] {
                push(out, Self::pick(node, node.variant), self.global[i], node.tint);
            }
        }
        for (i, node) in self.nodes.iter().enumerate() {
            let Some(skin) = &node.skin else { continue };
            if !self.vis[i] || node.hide_self || self.template[i] {
                continue;
            }
            let meshes: Vec<usize> = skin.surfaces.iter().map(|s| s.ring[self.skin_frame]).collect();
            // Godot 4 рисует скиннингованный меш в пространстве скелета.
            push(out, &meshes, self.global[skin.skeleton], node.tint);
        }
        for sp in self.spawned.values() {
            if !sp.visible || !self.vis[sp.parent] {
                continue;
            }
            let base = self.global[sp.parent] * sp.local.mat();
            // Обход поддерева шаблона.
            let mut stack = vec![sp.template];
            while let Some(i) = stack.pop() {
                let node = &self.nodes[i];
                if !node.visible && i != sp.template {
                    continue;
                }
                let meshes = Self::pick(node, sp.variant);
                if !meshes.is_empty() {
                    push(out, meshes, base * self.template_chain(sp.template, i), [1.0; 4]);
                }
                stack.extend_from_slice(&self.children[i]);
            }
        }
    }

    /// Обновляет фонари в FirstFires: позиция (если узел движется),
    /// видимость, анимированная энергия. `active` — false у сцены, которая
    /// сейчас не показывается (её свет гасится).
    pub fn sync_lights(&mut self, engine: &mut AlkashEngine, active: bool) {
        let Some(lights) = engine.lights.as_mut() else { return };
        for i in 0..self.nodes.len() {
            let (Some(id), Some(l)) = (self.nodes[i].light_id, self.nodes[i].light) else { continue };
            let pos = self.global[i].w_axis.truncate();
            let dir = -self.global[i].z_axis.truncate().normalize_or_zero();
            let on = active && self.vis[i];
            let intensity = if on { (self.nodes[i].light_energy * self.nodes[i].light_calib * self.light_scale).max(0.0) } else { 0.0 };
            let range = self.nodes[i].light_range.unwrap_or(l.range);
            let key = [pos.x, pos.y, pos.z, intensity, dir.x, dir.y, dir.z, range, l.color[0], l.color[1], l.color[2]];
            if self.nodes[i].light_sent == Some(key) {
                continue;
            }
            self.nodes[i].light_sent = Some(key);
            lights.update_light(id, &GPULight {
                position: [pos.x, pos.y, pos.z, if l.spot { 1.0 } else { 0.0 }],
                color: [l.color[0], l.color[1], l.color[2], intensity],
                direction: [dir.x, dir.y, dir.z, range],
                params: [if l.spot { l.angle } else { std::f32::consts::PI }, 2.0, if l.spot { l.angle * 0.8 } else { 0.0 }, 0.0],
            });
        }
    }
}
