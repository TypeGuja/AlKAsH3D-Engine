//! Мир на движке: дерево узлов из `world.mscw`, меши и лампы.
//!
//! Неподвижная геометрия запекается в мировые координаты и СЛИВАЕТСЯ: всё
//! с одним материалом в одной клетке карты и одного класса размера — один
//! меш движка. В исходной сцене десятки тысяч мелких объектов (static
//! batching Unity), по одному draw-вызову на каждый движок не тянет; клетки
//! сохраняют отсечение по фрустуму, классы размера — дальность прорисовки
//! (мелочь дальше пары сотен метров меньше пикселя).
//!
//! Подвижные поддеревья (машины) остаются деревом: у каждого узла свой
//! локальный трансформ, меши — в системе узла, каждый кадр инстанс получает
//! мировую матрицу.

use std::collections::HashMap;

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::{EulerRot, Mat3, Mat4, Quat, Vec3};
use alkash3d_rs::GPULight;

use crate::world_file::{Collider, Hinge, Kind, Material, NodeKind, Rigidbody, Shape, WorldFile};

/// Корни, которые в оригинале только служебные (интерфейс, системы).
const SKIP_ROOTS: &[&str] = &["GUI", "Systems", "PLAYER", "NoClip"];
/// Корни, где предметы с Rigidbody становятся настоящими телами (их можно
/// брать, ронять, ставить детали). В остальных местах (ралли, работы,
/// люди, трафик — их логика не перенесена) тела остаются неподвижной
/// декорацией, чтобы не платить draw-вызовами за тысячи объектов.
const BODY_ROOTS: &[&str] = &["CARPARTS", "ITEMS", "STORE", "YARD", "REPAIRSHOP"];
/// Шарниры, которые не двери (оси, кузов самосвала, камера игрока).
fn is_door_hinge(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("door") || n.contains("bootlid")
}

/// Классы размера: (радиус до, дальность прорисовки, клетка слияния).
const SIZE_CLASSES: [(f32, f32, f32); 4] = [(1.0, 150.0, 64.0), (4.0, 400.0, 64.0), (15.0, 1000.0, 128.0), (f32::INFINITY, f32::INFINITY, 512.0)];

#[derive(Clone, Copy, Debug)]
pub struct Trs {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}

impl Trs {
    pub fn mat(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }
}

pub struct WNode {
    pub name: String,
    pub parent: Option<usize>,
    pub local: Trs,
    pub active: bool,
    pub dynamic: bool,
    /// Меши движка в системе узла (только у подвижных).
    pub meshes: Vec<usize>,
    pub colliders: Vec<Collider>,
    pub rigidbody: Option<Rigidbody>,
    pub hinge: Option<Hinge>,
    /// Узел — корень физического тела (`physics.rs`).
    pub body_root: bool,
    /// Узел — дверь/крышка на шарнире.
    pub door: bool,
    /// Имя корня мира ("CARPARTS", "STORE", ...).
    pub root: String,
}

struct WLight {
    node: usize,
    id: u32,
    spot: bool,
    color: [f32; 3],
    intensity: f32,
    range: f32,
    half_angle: f32,
    sent: Option<[f32; 7]>,
}

pub struct World {
    pub nodes: Vec<WNode>,
    pub global: Vec<Mat4>,
    pub visible: Vec<bool>,
    pub children: Vec<Vec<usize>>,
    static_meshes: Vec<usize>,
    lights: Vec<WLight>,
}

/// Треугольники неподвижной твёрдой геометрии в мировых координатах — для
/// ходьбы и столкновений (см. `ground.rs`), с узлом-владельцем (для отладки).
pub type Triangles = Vec<([Vec3; 3], u32)>;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Будет ли поверхность с этим материалом твёрдой для игрока.
fn solid(m: Option<&Material>) -> bool {
    m.is_none_or(|m| matches!(m.kind, Kind::Opaque | Kind::Glass | Kind::Emissive))
}

struct Bucket {
    material: i32,
    max_draw: f32,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

struct Builder<'a> {
    file: &'a WorldFile,
    textures: HashMap<i32, Option<u32>>,
}

impl Builder<'_> {
    fn texture(&mut self, engine: &mut AlkashEngine, i: i32) -> Option<u32> {
        if i < 0 {
            return None;
        }
        *self.textures.entry(i).or_insert_with(|| {
            let t = self.file.textures.get(i as usize)?;
            engine.create_texture_rgba(t.width, t.height, &t.rgba)
        })
    }

    /// Вершины поверхности, умноженные на `xf`, с цветом материала.
    fn vertices(&self, surface: &crate::world_file::Surface, m: Option<&Material>, xf: Mat4) -> Vec<Vertex> {
        let (color, uv_scale, uv_offset) = match m {
            Some(m) => {
                let c = m.color;
                // Альфа цвета в вершине: у Fade/Glass она — доля непрозрачного слоя.
                let a = if matches!(m.kind, Kind::Fade | Kind::Glass) { c[3].clamp(0.0, 1.0) } else { 1.0 };
                ([srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), a], m.uv_scale, m.uv_offset)
            }
            None => ([0.5, 0.5, 0.5, 1.0], [1.0, 1.0], [0.0, 0.0]),
        };
        let basis = Mat3::from_mat4(xf);
        let normal_mat = basis.inverse().transpose();
        let flip = if basis.determinant() < 0.0 { -1.0 } else { 1.0 };
        surface
            .vertices
            .iter()
            .map(|v| {
                let p = xf.transform_point3(Vec3::new(v[0], v[1], v[2]));
                let n = (normal_mat * Vec3::new(v[3], v[4], v[5])).normalize_or_zero();
                let t = (basis * Vec3::new(v[8], v[9], v[10])).normalize_or_zero();
                Vertex::with_normal_uv_tangent(
                    p.x, p.y, p.z, n.x, n.y, n.z, color[0], color[1], color[2], color[3],
                    v[6] * uv_scale[0] + uv_offset[0],
                    v[7] * uv_scale[1] + uv_offset[1],
                    [t.x, t.y, t.z, v[11] * flip],
                )
            })
            .collect()
    }

    /// Меш движка с параметрами материала.
    fn mesh(&mut self, engine: &mut AlkashEngine, vertices: &[Vertex], indices: &[u32], material: i32) -> Option<usize> {
        if vertices.is_empty() || indices.is_empty() {
            return None;
        }
        let m = self.file.material(material);
        let mut mesh = Mesh::from_vertices_and_indices(vertices, indices).ok()?;
        mesh.material_roughness = 0.9;
        if let Some(m) = m {
            mesh.albedo_srv_index = self.texture(engine, m.albedo_tex);
            mesh.normal_srv_index = self.texture(engine, m.normal_tex);
            // В вершинах UV уже умножены на масштаб основной текстуры, а
            // детальная в Unity считается от исходных UV.
            if let Some(t) = self.texture(engine, m.detail_tex) {
                mesh.detail_srv_index = Some(t);
                mesh.detail_uv_scale = [m.detail_scale[0] / m.uv_scale[0].max(1e-6), m.detail_scale[1] / m.uv_scale[1].max(1e-6)];
            }
            mesh.material_roughness = m.roughness.clamp(0.04, 1.0);
            mesh.material_metallic = m.metallic.clamp(0.0, 1.0);
            mesh.material_emissive = m.emission;
            match m.kind {
                Kind::Cutout if m.albedo_tex >= 0 => mesh.alpha_cutoff = m.cutoff,
                Kind::Fade => mesh.blend = true,
                Kind::Glass => {
                    mesh.blend = true;
                    mesh.glass = true;
                }
                _ => {}
            }
        }
        Some(engine.add_mesh(mesh))
    }
}

impl World {
    /// `dynamic_roots` — имена корневых объектов, которые двигает игра.
    /// Возвращает мир и твёрдые треугольники статики.
    pub fn build(engine: &mut AlkashEngine, file: &WorldFile, dynamic_roots: &[&str]) -> (World, Triangles) {
        let n = file.nodes.len();
        let mut nodes: Vec<WNode> = Vec::with_capacity(n);
        let mut children = vec![Vec::new(); n];
        let mut root_of = vec![0usize; n];
        for (i, s) in file.nodes.iter().enumerate() {
            let top = s.parent == Some(0);
            root_of[i] = if top { i } else { s.parent.map(|p| root_of[p]).unwrap_or(0) };
            let root_name = file.nodes[root_of[i]].name.as_str();
            let skip = top && SKIP_ROOTS.contains(&s.name.as_str());
            let parent_dynamic = s.parent.is_some_and(|p| nodes[p].dynamic);
            let vehicle = dynamic_roots.iter().any(|r| *r == root_name);
            // Тело — верхний узел с Rigidbody (детали внутри тел — части тела).
            let door = s.hinge.is_some() && is_door_hinge(&s.name) && root_name != "PLAYER";
            // Двери на шарнирах (ворота гаража) — не предметы, их открывают.
            let body_root = !vehicle
                && !door
                && BODY_ROOTS.contains(&root_name)
                && s.rigidbody.is_some()
                && !parent_dynamic
                && !s.name.starts_with("Bolt");
            nodes.push(WNode {
                name: s.name.clone(),
                parent: s.parent,
                local: Trs { pos: s.pos, rot: s.rot, scale: s.scale },
                active: s.active && !skip,
                dynamic: parent_dynamic || (top && vehicle) || body_root || door,
                meshes: Vec::new(),
                colliders: s.colliders.clone(),
                rigidbody: s.rigidbody,
                hinge: s.hinge,
                body_root,
                door,
                root: root_name.to_string(),
            });
            if let Some(p) = s.parent {
                children[p].push(i);
            }
        }
        let mut world = World { nodes, global: vec![Mat4::IDENTITY; n], visible: vec![true; n], children, static_meshes: Vec::new(), lights: Vec::new() };
        world.update_globals();

        let mut b = Builder { file, textures: HashMap::new() };
        let mut buckets: HashMap<(i32, i32, i32, usize), Bucket> = HashMap::new();
        let mut tris: Triangles = Vec::new();
        let mut local_cache: HashMap<(usize, Vec<i32>), Vec<usize>> = HashMap::new();
        for (i, s) in file.nodes.iter().enumerate() {
            // Твёрдые коллайдеры неподвижного мира (Unity: стволы деревьев,
            // заборы, невидимые стены, сетки-коллайдеры зданий и рельефа).
            if !world.nodes[i].dynamic && world.visible[i] {
                for c in &s.colliders {
                    if !c.trigger {
                        collider_triangles(file, c.shape, world.global[i], i as u32, &mut tris);
                    }
                }
            }
            // Подвижным узлам меши строим и выключенным: их включает игра
            // (установленные копии деталей, двери).
            let dyn_hidden_ok = world.nodes[i].dynamic && !SKIP_ROOTS.contains(&file.nodes[root_of[i]].name.as_str());
            if s.kind != NodeKind::Mesh || !(world.visible[i] || dyn_hidden_ok) {
                continue;
            }
            let Some(mi) = s.mesh else { continue };
            let surfaces = &file.meshes[mi];
            if world.nodes[i].dynamic {
                let key = (mi, s.materials.clone());
                if let Some(v) = local_cache.get(&key) {
                    world.nodes[i].meshes = v.clone();
                    continue;
                }
                let mut out = Vec::new();
                for (si, surf) in surfaces.iter().enumerate() {
                    let mat = s.materials.get(si).copied().unwrap_or(-1);
                    let m = file.material(mat);
                    if m.is_some_and(|m| m.kind == Kind::Hidden) {
                        continue;
                    }
                    let v = b.vertices(surf, m, Mat4::IDENTITY);
                    out.extend(b.mesh(engine, &v, &surf.indices, mat));
                }
                local_cache.insert(key, out.clone());
                world.nodes[i].meshes = out;
                continue;
            }
            let xf = world.global[i];
            for (si, surf) in surfaces.iter().enumerate() {
                let mat = s.materials.get(si).copied().unwrap_or(-1);
                let m = file.material(mat);
                if m.is_some_and(|m| m.kind == Kind::Hidden) || surf.vertices.is_empty() {
                    continue;
                }
                let v = b.vertices(surf, m, xf);
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for p in &v {
                    let p = Vec3::new(p.position[0], p.position[1], p.position[2]);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                let radius = (hi - lo).length() * 0.5;
                let center = (lo + hi) * 0.5;
                let class = SIZE_CLASSES.iter().position(|c| radius < c.0).unwrap_or(3);
                let (_, max_draw, cell) = SIZE_CLASSES[class];
                let key = (mat, (center.x / cell).floor() as i32, (center.z / cell).floor() as i32, class);
                let bucket = buckets.entry(key).or_insert_with(|| Bucket { material: mat, max_draw, vertices: Vec::new(), indices: Vec::new() });
                let base = bucket.vertices.len() as u32;
                bucket.indices.extend(surf.indices.iter().map(|&x| x + base));
                if solid(m) && (hi - lo).max_element() >= 0.4 {
                    for t in surf.indices.chunks_exact(3) {
                        let p = |k: u32| {
                            let q = v[k as usize].position;
                            Vec3::new(q[0], q[1], q[2])
                        };
                        tris.push(([p(t[0]), p(t[1]), p(t[2])], i as u32));
                    }
                }
                bucket.vertices.extend(v);
            }
        }
        let bucket_count = buckets.len();
        for (_, bk) in buckets {
            if let Some(m) = b.mesh(engine, &bk.vertices, &bk.indices, bk.material) {
                engine.meshes[m].max_draw_distance = bk.max_draw;
                world.static_meshes.push(m);
            }
        }

        if let Some(lights) = engine.lights.as_mut() {
            for (i, s) in file.nodes.iter().enumerate() {
                let Some(l) = s.light else { continue };
                if !world.visible[i] {
                    continue;
                }
                let id = lights.add_light(&GPULight {
                    position: [0.0; 4],
                    color: [0.0; 4],
                    direction: [0.0, -1.0, 0.0, l.range],
                    params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
                });
                world.lights.push(WLight {
                    node: i,
                    id,
                    spot: l.spot,
                    color: l.color,
                    intensity: unity_light_intensity(l.intensity, l.range),
                    range: l.range,
                    half_angle: l.half_angle,
                    sent: None,
                });
            }
        }

        println!(
            "[MSC] мир: узлов {}, подвижных {}, статических мешей {} (из {} объектов-поверхностей), текстур {}, ламп {}, твёрдых треугольников {}",
            n,
            world.nodes.iter().filter(|x| x.dynamic).count(),
            world.static_meshes.len(),
            bucket_count,
            b.textures.values().filter(|t| t.is_some()).count(),
            world.lights.len(),
            tris.len()
        );
        (world, tris)
    }

    /// Путь узла от корня мира ("YARD/Building/...").
    pub fn path(&self, mut i: usize) -> String {
        let mut parts = Vec::new();
        while let Some(p) = self.nodes[i].parent {
            parts.push(self.nodes[i].name.as_str());
            i = p;
        }
        parts.reverse();
        parts.join("/")
    }

    pub fn find_root(&self, prefix: &str) -> Option<usize> {
        (0..self.nodes.len()).find(|&i| self.nodes[i].parent == Some(0) && self.nodes[i].name.starts_with(prefix))
    }

    /// Первый потомок `root` с именем `name` (обход в глубину).
    pub fn find_in(&self, root: usize, name: &str) -> Option<usize> {
        let mut stack = vec![root];
        while let Some(i) = stack.pop() {
            if self.nodes[i].name == name && i != root {
                return Some(i);
            }
            stack.extend(self.children[i].iter().rev());
        }
        None
    }

    pub fn update_globals(&mut self) {
        for i in 0..self.nodes.len() {
            let local = self.nodes[i].local.mat();
            let (g, v) = match self.nodes[i].parent {
                Some(p) => (self.global[p] * local, self.visible[p] && self.nodes[i].active),
                None => (local, self.nodes[i].active),
            };
            self.global[i] = g;
            self.visible[i] = v;
        }
    }

    /// Пересчёт только поддерева (машина сдвинулась — остальной мир тот же).
    pub fn update_subtree(&mut self, root: usize) {
        let mut stack = vec![root];
        while let Some(i) = stack.pop() {
            let local = self.nodes[i].local.mat();
            let (g, v) = match self.nodes[i].parent {
                Some(p) => (self.global[p] * local, self.visible[p] && self.nodes[i].active),
                None => (local, self.nodes[i].active),
            };
            self.global[i] = g;
            self.visible[i] = v;
            stack.extend_from_slice(&self.children[i]);
        }
    }

    /// Включить/выключить узел (видимость поддерева пересчитывается сразу).
    pub fn set_active(&mut self, i: usize, active: bool) {
        if self.nodes[i].active != active {
            self.nodes[i].active = active;
            self.update_subtree(i);
        }
    }

    /// Поставить узел в мировую позу `m` (локальная = родитель⁻¹ · m).
    pub fn set_world_matrix(&mut self, i: usize, m: Mat4) {
        let local = match self.nodes[i].parent {
            Some(p) => self.global[p].inverse() * m,
            None => m,
        };
        let (scale, rot, pos) = local.to_scale_rotation_translation();
        self.nodes[i].local = Trs { pos, rot, scale };
        self.update_subtree(i);
    }

    /// Мировая матрица узла по текущим локальным (без пересчёта всего мира).
    pub fn compute_global(&self, i: usize) -> Mat4 {
        let local = self.nodes[i].local.mat();
        match self.nodes[i].parent {
            Some(p) => self.global[p] * local,
            None => local,
        }
    }

    pub fn push_instances(&self, out: &mut Vec<MeshInstance>) {
        out.extend(self.static_meshes.iter().map(|&m| MeshInstance::new(m)));
        for (i, node) in self.nodes.iter().enumerate() {
            if !node.dynamic || node.meshes.is_empty() || !self.visible[i] {
                continue;
            }
            let (s, r, t) = self.global[i].to_scale_rotation_translation();
            let (rz, ry, rx) = r.to_euler(EulerRot::ZYX);
            for &m in &node.meshes {
                out.push(MeshInstance::new(m).at(t.x, t.y, t.z).rotated(rx, ry, rz).scaled(s.x, s.y, s.z));
            }
        }
    }

    /// Лампы мира в FirstFires: позиция за узлом, включена — если узел виден.
    /// `scale` — общий множитель яркости (MSC_LIGHT_SCALE).
    pub fn sync_lights(&mut self, engine: &mut AlkashEngine, scale: f32) {
        let Some(lights) = engine.lights.as_mut() else { return };
        for l in &mut self.lights {
            let g = self.global[l.node];
            let pos = g.w_axis.truncate();
            // Unity: луч прожектора — +Z узла, после зеркалирования Z — -Z.
            let dir = -g.z_axis.truncate().normalize_or_zero();
            let intensity = if self.visible[l.node] { l.intensity * scale } else { 0.0 };
            let key = [pos.x, pos.y, pos.z, intensity, dir.x, dir.y, dir.z];
            if l.sent == Some(key) {
                continue;
            }
            l.sent = Some(key);
            lights.update_light(l.id, &GPULight {
                position: [pos.x, pos.y, pos.z, if l.spot { 1.0 } else { 0.0 }],
                color: [l.color[0], l.color[1], l.color[2], intensity],
                direction: [dir.x, dir.y, dir.z, l.range],
                params: [if l.spot { l.half_angle } else { std::f32::consts::PI }, 2.0, if l.spot { l.half_angle * 0.8 } else { 0.0 }, 0.0],
            });
        }
    }
}

/// Треугольники коллайдера в мировых координатах (сфера и капсула —
/// многогранники в 8 сегментов: для ходьбы и столкновений достаточно).
pub fn collider_triangles(file: &WorldFile, shape: Shape, m: Mat4, owner: u32, out: &mut Triangles) {
    let mut push = |a: Vec3, b: Vec3, c: Vec3| out.push(([m.transform_point3(a), m.transform_point3(b), m.transform_point3(c)], owner));
    match shape {
        Shape::Box { center, size } => {
            let h = size * 0.5;
            let p = |x: f32, y: f32, z: f32| center + Vec3::new(x * h.x, y * h.y, z * h.z);
            let faces = [
                [p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.)],
                [p(-1., -1., 1.), p(-1., 1., 1.), p(1., 1., 1.), p(1., -1., 1.)],
                [p(-1., -1., -1.), p(-1., 1., -1.), p(-1., 1., 1.), p(-1., -1., 1.)],
                [p(1., -1., -1.), p(1., -1., 1.), p(1., 1., 1.), p(1., 1., -1.)],
                [p(-1., -1., -1.), p(-1., -1., 1.), p(1., -1., 1.), p(1., -1., -1.)],
                [p(-1., 1., -1.), p(1., 1., -1.), p(1., 1., 1.), p(-1., 1., 1.)],
            ];
            for f in faces {
                push(f[0], f[1], f[2]);
                push(f[0], f[2], f[3]);
            }
        }
        Shape::Sphere { center, radius } => capsule_tris(center, radius, 0.0, 1, &mut push),
        Shape::Capsule { center, radius, height, axis } => capsule_tris(center, radius, (height * 0.5 - radius).max(0.0), axis, &mut push),
        Shape::Mesh { mesh, .. } => {
            let Some(surfaces) = file.meshes.get(mesh) else { return };
            for s in surfaces {
                for t in s.indices.chunks_exact(3) {
                    let v = |k: u32| {
                        let q = s.vertices[k as usize];
                        Vec3::new(q[0], q[1], q[2])
                    };
                    push(v(t[0]), v(t[1]), v(t[2]));
                }
            }
        }
    }
}

/// Капсула (сфера при half = 0) вдоль оси `axis` узла: 8 сегментов по кругу, 3 кольца на полусферу.
fn capsule_tris(center: Vec3, r: f32, half: f32, axis: u32, push: &mut impl FnMut(Vec3, Vec3, Vec3)) {
    const SEG: usize = 8;
    let (ax, u, w) = match axis {
        0 => (Vec3::X, Vec3::Y, Vec3::Z),
        2 => (Vec3::Z, Vec3::X, Vec3::Y),
        _ => (Vec3::Y, Vec3::Z, Vec3::X),
    };
    // Кольца от низа к верху: (смещение по оси, радиус кольца).
    let mut rings: Vec<(f32, f32)> = vec![(-half - r, 0.0)];
    for k in 1..=3 {
        let a = std::f32::consts::FRAC_PI_2 * (1.0 - k as f32 / 3.0);
        rings.push((-half - r * a.sin(), r * a.cos()));
    }
    for k in 0..=2 {
        let a = std::f32::consts::FRAC_PI_2 * (k as f32 / 3.0);
        rings.push((half + r * a.sin(), r * a.cos()));
    }
    rings.push((half + r, 0.0));
    let pt = |ring: (f32, f32), s: usize| {
        let a = s as f32 / SEG as f32 * std::f32::consts::TAU;
        center + ax * ring.0 + (u * a.cos() + w * a.sin()) * ring.1
    };
    for k in 0..rings.len() - 1 {
        for s in 0..SEG {
            let (a, b) = (pt(rings[k], s), pt(rings[k], s + 1));
            let (c, d) = (pt(rings[k + 1], s), pt(rings[k + 1], s + 1));
            push(a, c, b);
            push(b, c, d);
        }
    }
}

/// Unity 5 (forward) гасит точечный свет как 1 / (1 + 25 (d/r)²) и к краю
/// радиуса обнуляет; в движке — честный обратный квадрат. Подбираем силу
/// источника так, чтобы на 2 м освещённость совпала: это перевод единиц, а
/// не подмена модели затухания.
fn unity_light_intensity(intensity: f32, range: f32) -> f32 {
    let d = 2.0f32.min(range * 0.5).max(0.3);
    intensity * d * d / (1.0 + 25.0 * (d / range.max(0.01)).powi(2))
}
