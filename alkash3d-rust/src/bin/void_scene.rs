//! void_scene — собирает сцену "пустоты" (куда игрок попадает после падения
//! в restroom): стол из оригинального Buckshot, тёмный пол без стен и лампы
//! на штативах вокруг стола. Пишет `.altex` v3 + `.alfar` (формат движка).
//!
//! `void_scene build <мир.altex> <выход.altex>` — собрать сцену.
//! `void_scene list <мир.altex> [подстрока]` — узлы мира с мешами и светом.
//!
//! Из мира переносятся (`SUBTREES`, `SINGLES`) стол с панелью здоровья,
//! плашкой патронов и ружьём, корпус счётчика, машинка с бланком отказа,
//! камера и префаб гильзы — с анимациями, пути как в оригинале без "main/".
//! Скины (руки дилера), скелеты и свет мира не переносятся. Пол — большой
//! тёмный квадрат по низу стола. Прожекторы — те, что висели над столом в
//! оригинале, отодвинуты, подняты и наведены на стол, стоят на штативах со
//! стрелой (`lamps/lamp 1..4`); ещё два таких же светят на пробковую доску
//! слева от стола (`lamps/lamp 5..6`, `board`). Узлы лампы — `projector, stand,
//! glass, light`; когда что горит — скрипт игры.
//! Масштаб как у Buckshot: 1 единица ≈ 10 см.

#[path = "buckshot/scene_file.rs"]
#[allow(dead_code)]
mod scene_file;
#[path = "buckshot/altex_write.rs"]
#[allow(dead_code)]
mod altex_write;

use alkash3d_rs::math::{Mat4, Quat, Vec3, Vec4};
use scene_file::{Kind, LightInfo, Material, MeshData, Node, SceneFile, Surface, Texture};

const TABLE: &str = "main/tabletop parent";
/// Поддеревья мира, переносимые целиком: стол (с панелью здоровья, плашкой
/// патронов и ружьём), корпус счётчика с дефибриллятором, машинка с бланком
/// отказа от претензий, префабы гильзы (для плашки) и предметов (физика вещей).
const SUBTREES: [&str; 9] = [
    "main/tabletop parent",
    "main/health counter main1",
    "main/signature machine2",
    "prefab_shell",
    // Предметы — для физики вещей в пустоте.
    "prefab_item_handsaw",
    "prefab_item_magnifying glass",
    "prefab_item_beer can",
    "prefab_item_cigarettes",
    "prefab_item_handcuffs",
];
/// Отдельные узлы без детей: камера (её водит анимация "pickup waiver") и
/// плеер этой анимации.
const SINGLES: [&str; 2] = ["main/Camera", "main/standalone managers/signature manager/animator_pickup waiver"];
/// Прожекторы, висевшие над столом в оригинале (крепление + корпус + крышка).
const LAMPS: [&str; 4] = [
    "main/backroom visual parent/Cube_001",
    "main/backroom visual parent/Cube_003",
    "main/backroom visual parent/Cube_005",
    "main/backroom visual parent/Cube_043",
];
/// Насколько прожектор отодвинут от стола (штатив не должен в него упираться).
const SHIFT: f32 = 6.0;
/// Насколько прожектор поднят над своим местом в оригинале.
const LIFT: f32 = 5.0;
/// Куда смотрят прожекторы: центр стола, высота над полом (столешница ~6.9).
const AIM_Y: f32 = 7.0;
/// Вылет стрелы: стойка штатива стоит дальше прожектора от стола.
const BOOM: f32 = 3.5;
/// Стекло: расстояние от центра корпуса по оси (передняя крышка ~2.7) и радиус.
const GLASS_AT: f32 = 2.85;
const GLASS_R: f32 = 1.3;
/// Цвет света — как у ламп над столом в оригинале.
const LIGHT_COLOR: [f32; 3] = [0.960784, 0.501961, 0.227451];
/// Дальность света прожекторов стола и доски: друг до друга не достают.
const TABLE_LIGHT_RANGE: f32 = 70.0;
const BOARD_LIGHT_RANGE: f32 = 45.0;
/// Доска: расстояние от стола влево (для сидящего за столом — -Z), высота центра, размер.
const BOARD_DIST: f32 = 110.0;
const BOARD_Y: f32 = 17.0;
const BOARD_W: f32 = 14.0;
const BOARD_H: f32 = 10.0;
/// Прожекторы доски: на сколько перед ней, высота, разнос в стороны.
const BOARD_LAMP_DIST: f32 = 28.0;
const BOARD_LAMP_Y: f32 = 22.0;
const BOARD_LAMP_SIDE: f32 = 11.0;
/// Игрок появляется со своей стороны стола (-X), далеко.
const SPAWN_DIST: f32 = 220.0;
const FLOOR_HALF: f32 = 2500.0;

fn node(name: &str, parent: Option<usize>, kind: Kind, xf: Mat4) -> Node {
    Node {
        name: name.into(),
        parent,
        kind,
        visible: true,
        top_level: false,
        xf,
        mesh: None,
        materials: Vec::new(),
        light: None,
        fov: 0.0,
        audio: String::new(),
        skeleton: None,
        binds: Vec::new(),
        bones: Vec::new(),
        label: None,
    }
}

fn material(albedo: [f32; 4], roughness: f32, metallic: f32, unshaded: bool) -> Material {
    Material {
        albedo,
        albedo_tex: -1,
        normal_tex: -1,
        roughness,
        metallic,
        emission: [0.0; 3],
        transparency: 0,
        unshaded,
        uv_scale: [1.0, 1.0],
        uv_offset: [0.0, 0.0],
    }
}

fn surface() -> Surface {
    Surface { vertices: Vec::new(), indices: Vec::new(), skin: None }
}

/// Ящик с полуразмерами `h` в системе `m` (локальные координаты узла).
fn add_box(s: &mut Surface, m: Mat4, h: Vec3) {
    let faces = [(Vec3::X, Vec3::Z), (-Vec3::X, -Vec3::Z), (Vec3::Y, Vec3::X), (-Vec3::Y, Vec3::X), (Vec3::Z, -Vec3::X), (-Vec3::Z, Vec3::X)];
    for (n, t) in faces {
        let b = n.cross(t);
        let base = s.vertices.len() as u32;
        let wn = m.transform_vector3(n).normalize();
        let wt = m.transform_vector3(t).normalize();
        for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = m.transform_point3((n + t * u + b * v) * h);
            s.vertices.push([p.x, p.y, p.z, wn.x, wn.y, wn.z, (u + 1.0) * 0.5, (v + 1.0) * 0.5, wt.x, wt.y, wt.z, 1.0]);
        }
        s.indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// Круглая пластина радиуса `r` и полутолщины `h` в плоскости XY системы `m`.
fn add_disc(s: &mut Surface, m: Mat4, r: f32, h: f32) {
    const SEG: usize = 32;
    let ang = |j: usize| j as f32 / SEG as f32 * std::f32::consts::TAU;
    let wn = |v: Vec3| m.transform_vector3(v).normalize();
    // Две крышки: центр + кольцо.
    for side in [1.0f32, -1.0] {
        let n = wn(Vec3::Z * side);
        let t = wn(Vec3::X);
        let base = s.vertices.len() as u32;
        let c = m.transform_point3(Vec3::Z * h * side);
        s.vertices.push([c.x, c.y, c.z, n.x, n.y, n.z, 0.5, 0.5, t.x, t.y, t.z, 1.0]);
        for j in 0..SEG {
            let (sn, cs) = ang(j).sin_cos();
            let p = m.transform_point3(Vec3::new(cs * r, sn * r, h * side));
            s.vertices.push([p.x, p.y, p.z, n.x, n.y, n.z, 0.5 + cs * 0.5, 0.5 + sn * 0.5, t.x, t.y, t.z, 1.0]);
        }
        for j in 0..SEG as u32 {
            let (a, b) = (base + 1 + j, base + 1 + (j + 1) % SEG as u32);
            // Лицевая сторона — по часовой, если смотреть снаружи (как у `add_box`).
            if side > 0.0 {
                s.indices.extend([base, b, a]);
            } else {
                s.indices.extend([base, a, b]);
            }
        }
    }
    // Ободок.
    let base = s.vertices.len() as u32;
    for j in 0..SEG {
        let (sn, cs) = ang(j).sin_cos();
        let n = wn(Vec3::new(cs, sn, 0.0));
        let t = wn(Vec3::Z);
        for z in [h, -h] {
            let p = m.transform_point3(Vec3::new(cs * r, sn * r, z));
            s.vertices.push([p.x, p.y, p.z, n.x, n.y, n.z, j as f32 / SEG as f32, (z > 0.0) as u32 as f32, t.x, t.y, t.z, 1.0]);
        }
    }
    for j in 0..SEG as u32 {
        let (a, b) = (base + j * 2, base + ((j + 1) % SEG as u32) * 2);
        s.indices.extend([a, b + 1, a + 1, a, b, b + 1]);
    }
}

/// Система координат с осью -Z на `dir` (как свет Godot), началом в `at`.
fn facing(at: Vec3, dir: Vec3) -> Mat4 {
    let z = -dir.normalize();
    let x = Vec3::Y.cross(z).normalize();
    let y = z.cross(x);
    Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), Vec4::new(at.x, at.y, at.z, 1.0))
}

/// Отрезок a→b как вытянутый ящик толщины `r`.
fn add_rod(s: &mut Surface, a: Vec3, b: Vec3, r: f32) {
    let d = b - a;
    // Вертикальный отрезок: `facing` с осью вдоль Y вырождается.
    let m = if d.normalize().y.abs() > 0.99 {
        Mat4::from_translation((a + b) * 0.5) * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
    } else {
        facing((a + b) * 0.5, d)
    };
    add_box(s, m, Vec3::new(r, r, d.length() * 0.5));
}

/// Мировые матрицы узлов (top_level — без родителя, как в Godot).
fn globals(scene: &SceneFile) -> Vec<Mat4> {
    let mut g: Vec<Mat4> = Vec::with_capacity(scene.nodes.len());
    for n in &scene.nodes {
        let parent = n.parent.filter(|_| !n.top_level).map(|p| g[p]).unwrap_or(Mat4::IDENTITY);
        g.push(parent * n.xf);
    }
    g
}

/// Прожектор (оригинальный корпус) на штативе.
struct Lamp {
    name: String,
    /// Узлы прожектора в мире (крепление + корпус + крышка), родитель раньше детей.
    src: Vec<usize>,
    /// Перестановка прожектора в мире (сдвиг, подъём, наклон на цель).
    place: Mat4,
    /// Низ стойки штатива на полу (начало узла `lamps/<name>`).
    foot: Vec3,
    /// Центр корпуса и ось (куда светит), верх крепления — уже переставленные.
    head: Vec3,
    axis: Vec3,
    mount: Vec3,
    energy: f32,
    range: f32,
    angle: f32,
}

/// Прожектор `src` переставить: центр корпуса в `head`, ось на `target`;
/// стойка штатива — на `BOOM` дальше от цели, чем крепление.
fn plan_lamp(world: &SceneFile, global: &[Mat4], name: String, src: usize, head: Vec3, target: Vec3, floor_y: f32) -> Result<Lamp, String> {
    let n = world.nodes.len();
    let cyl = (0..n)
        .find(|&i| world.nodes[i].parent == Some(src) && world.nodes[i].name.starts_with("Cylinder"))
        .ok_or_else(|| format!("у прожектора {} нет корпуса", world.path(src)))?;
    let head_old = global[cyl].w_axis.truncate();
    let axis = (target - head).normalize();
    let turn = Quat::from_rotation_arc(global[cyl].y_axis.truncate().normalize(), axis);
    let place = Mat4::from_translation(head) * Mat4::from_quat(turn) * Mat4::from_translation(-head_old);
    let mount = place.transform_point3(global[src].w_axis.truncate());
    let away = Vec3::new(mount.x - target.x, 0.0, mount.z - target.z).normalize();
    let foot = Vec3::new(mount.x + away.x * BOOM, floor_y, mount.z + away.z * BOOM);
    let mut nodes = vec![src];
    for i in src + 1..n {
        if world.nodes[i].parent.is_some_and(|p| nodes.contains(&p)) {
            nodes.push(i);
        }
    }
    Ok(Lamp { name, src: nodes, place, foot, head, axis, mount, energy: 25.0, range: 90.0, angle: 0.6 })
}

/// Копия узла (у переносимых узлов нет костей, скина и света).
fn copy_node(nd: &Node) -> Node {
    Node {
        name: nd.name.clone(),
        parent: nd.parent,
        kind: nd.kind,
        visible: nd.visible,
        top_level: false,
        xf: nd.xf,
        mesh: nd.mesh,
        materials: nd.materials.clone(),
        light: None,
        fov: nd.fov,
        audio: nd.audio.clone(),
        skeleton: None,
        binds: Vec::new(),
        bones: Vec::new(),
        label: nd.label.clone(),
    }
}

/// Пробка: тёплый коричневый с пятнами и порами (процедурно, 256×256).
fn cork_texture() -> Texture {
    let n = 256u32;
    let hash = |x: u32, y: u32| {
        let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
    };
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let big = hash(x / 6, y / 6) * 0.5 + hash(x / 3 + 1000, y / 3) * 0.3;
            let fine = hash(x, y);
            let mut k = 0.78 + (big - 0.4) * 0.35 + (fine - 0.5) * 0.18;
            if fine > 0.93 {
                k *= 0.55;
            }
            let c = |v: f32| (v * k * 255.0).clamp(0.0, 255.0) as u8;
            rgba.extend([c(0.66), c(0.47), c(0.29), 255]);
        }
    }
    Texture { width: n, height: n, rgba }
}

/// Узел-меш из одной поверхности.
fn mesh_node(out: &mut SceneFile, name: &str, parent: usize, xf: Mat4, s: Surface, mat: i32) {
    out.meshes.push(MeshData { surfaces: vec![s] });
    let mut nd = node(name, Some(parent), Kind::Mesh, xf);
    nd.mesh = Some(out.meshes.len() - 1);
    nd.materials = vec![mat];
    out.nodes.push(nd);
}

fn build(world: SceneFile) -> Result<SceneFile, String> {
    let n = world.nodes.len();
    let find = |p: &str| (0..n).find(|&i| world.path(i) == p).ok_or_else(|| format!("в мире нет узла '{}'", p));
    let global = globals(&world);
    let table = find(TABLE)?;

    // Что берём из мира: поддеревья целиком (без скинов, скелетов и света —
    // в пустоте светят только прожекторы) и отдельные узлы без детей.
    let subtrees: Vec<usize> = SUBTREES.iter().map(|p| find(p)).collect::<Result<_, _>>()?;
    let singles: Vec<usize> = SINGLES.iter().map(|p| find(p)).collect::<Result<_, _>>()?;
    let usable = |i: usize| world.nodes[i].skeleton.is_none() && !matches!(world.nodes[i].kind, Kind::Skeleton | Kind::Light);
    let mut keep = vec![false; n];
    for i in 0..n {
        let from_parent = world.nodes[i].parent.is_some_and(|p| keep[p] && !singles.contains(&p));
        keep[i] = usable(i) && (subtrees.contains(&i) || singles.contains(&i) || from_parent);
    }

    // Габариты стола (видимое): по низу стоит пол, на центр смотрят прожекторы.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut in_table = vec![false; n];
    for i in 0..n {
        in_table[i] = keep[i] && world.nodes[i].visible && (i == table || world.nodes[i].parent.is_some_and(|p| in_table[p]));
        if let (true, Some(m)) = (in_table[i], world.nodes[i].mesh) {
            for v in world.meshes[m].surfaces.iter().flat_map(|s| &s.vertices) {
                let p = global[i].transform_point3(Vec3::new(v[0], v[1], v[2]));
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    let center = (lo + hi) * 0.5;
    let floor_y = lo.y;
    println!("стол: габариты {:.1?} .. {:.1?}, пол на y = {:.2}", lo, hi, floor_y);

    // Прожекторы над столом: отодвинуты, подняты, смотрят в центр стола.
    let table_aim = Vec3::new(center.x, floor_y + AIM_Y, center.z);
    let mut lamps = Vec::new();
    for (k, p) in LAMPS.iter().enumerate() {
        let src = find(p)?;
        let mount = global[src].w_axis.truncate();
        let out_dir = Vec3::new(mount.x - center.x, 0.0, mount.z - center.z).normalize();
        let cyl = (0..n).find(|&i| world.nodes[i].parent == Some(src) && world.nodes[i].name.starts_with("Cylinder")).ok_or("нет корпуса")?;
        let head = global[cyl].w_axis.truncate() + out_dir * SHIFT + Vec3::Y * LIFT;
        let mut l = plan_lamp(&world, &global, format!("lamp {}", k + 1), src, head, table_aim, floor_y)?;
        l.range = TABLE_LIGHT_RANGE;
        lamps.push(l);
    }
    // Доска — слева от стола (игрок за столом смотрит на +X, слева -Z),
    // лицом к столу; перед ней два
    // таких же прожектора. Дальности света подобраны так, чтобы свет стола
    // не доставал до доски, а свет доски — до стола.
    let board = Vec3::new(center.x, floor_y + BOARD_Y, center.z - BOARD_DIST);
    let src0 = find(LAMPS[0])?;
    for (k, side) in [-1.0f32, 1.0].iter().enumerate() {
        let head = Vec3::new(board.x + side * BOARD_LAMP_SIDE, floor_y + BOARD_LAMP_Y, board.z + BOARD_LAMP_DIST);
        let mut l = plan_lamp(&world, &global, format!("lamp {}", LAMPS.len() + k + 1), src0, head, board, floor_y)?;
        l.range = BOARD_LIGHT_RANGE;
        l.angle = 0.45;
        lamps.push(l);
    }
    // Пути корней (для узлов-одиночек глубже корня мира).
    let root_paths: Vec<(usize, String)> = subtrees.iter().chain(&singles).map(|&i| (i, world.path(i))).collect();

    let SceneFile { textures: w_tex, materials: w_mat, meshes: w_mesh, nodes: w_nodes, players: w_players } = world;
    let mut w_tex: Vec<Option<Texture>> = w_tex.into_iter().map(Some).collect();
    let mut w_mat: Vec<Option<Material>> = w_mat.into_iter().map(Some).collect();
    let mut w_mesh: Vec<Option<MeshData>> = w_mesh.into_iter().map(Some).collect();
    let (mut tex_map, mut mat_map, mut mesh_map) = (vec![-1i32; w_tex.len()], vec![-1i32; w_mat.len()], vec![-1i32; w_mesh.len()]);
    let mut out = SceneFile { textures: Vec::new(), materials: Vec::new(), meshes: Vec::new(), nodes: Vec::new(), players: Vec::new() };

    // Меши, материалы и текстуры узла — в новую сцену (каждый ресурс один раз).
    let mut adopt = |nd: &mut Node, out: &mut SceneFile| -> Result<(), String> {
        if let Some(m) = nd.mesh {
            if mesh_map[m] < 0 {
                mesh_map[m] = out.meshes.len() as i32;
                let mut md = w_mesh[m].take().ok_or("меш уже взят")?;
                for s in &mut md.surfaces {
                    s.skin = None;
                }
                out.meshes.push(md);
            }
            nd.mesh = Some(mesh_map[m] as usize);
        }
        for mi in nd.materials.iter_mut().filter(|m| **m >= 0) {
            let k = *mi as usize;
            if mat_map[k] < 0 {
                mat_map[k] = out.materials.len() as i32;
                let mut mat = w_mat[k].take().ok_or("материал уже взят")?;
                for t in [&mut mat.albedo_tex, &mut mat.normal_tex] {
                    if *t >= 0 {
                        let ti = *t as usize;
                        if tex_map[ti] < 0 {
                            tex_map[ti] = out.textures.len() as i32;
                            out.textures.push(w_tex[ti].take().ok_or("текстура уже взята")?);
                        }
                        *t = tex_map[ti];
                    }
                }
                out.materials.push(mat);
            }
            *mi = mat_map[k];
        }
        Ok(())
    };

    out.nodes.push(node("void", None, Kind::Spatial, Mat4::IDENTITY));
    // Пути в новой сцене — как в оригинале без "main/" (константы порта
    // Buckshot подходят как есть); промежуточные узлы одиночек — пустые.
    let mut chain: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut new_idx = vec![usize::MAX; n];
    for i in 0..n {
        if !keep[i] {
            continue;
        }
        let mut nd = copy_node(&w_nodes[i]);
        if let Some((_, path)) = root_paths.iter().find(|(r, _)| *r == i) {
            nd.xf = global[i];
            nd.parent = if w_nodes[i].parent.is_none() {
                // Префабы — свои корни, как в мире (Graph::spawn ищет их по имени).
                None
            } else {
                let parts: Vec<&str> = path.trim_start_matches("main/").split('/').collect();
                let mut parent = 0;
                for d in 1..parts.len() {
                    let key = parts[..d].join("/");
                    parent = *chain.entry(key).or_insert_with(|| {
                        out.nodes.push(node(parts[d - 1], Some(parent), Kind::Spatial, Mat4::IDENTITY));
                        out.nodes.len() - 1
                    });
                }
                Some(parent)
            };
        } else {
            nd.parent = w_nodes[i].parent.map(|p| new_idx[p]);
        }
        adopt(&mut nd, &mut out)?;
        new_idx[i] = out.nodes.len();
        out.nodes.push(nd);
    }
    // Анимации перенесённых узлов; дорожки на то, чего здесь нет, — выкинуть.
    for p in w_players {
        if new_idx[p.node] == usize::MAX {
            continue;
        }
        let animations = p
            .animations
            .into_iter()
            .map(|mut a| {
                a.tracks.retain(|t| t.target.is_none_or(|t| new_idx[t] != usize::MAX));
                for t in &mut a.tracks {
                    t.target = t.target.map(|t| new_idx[t]);
                }
                a
            })
            .collect();
        out.players.push(scene_file::Player { node: new_idx[p.node], speed: p.speed, animations });
    }
    println!("из мира: {} узлов, {} анимационных плееров", out.nodes.len(), out.players.len());

    // Пол.
    let m_floor = out.materials.len() as i32;
    out.materials.push(material([0.06, 0.06, 0.065, 1.0], 0.85, 0.0, false));
    let mut s = surface();
    add_box(&mut s, Mat4::from_translation(Vec3::new(center.x, floor_y - 0.5, center.z)), Vec3::new(FLOOR_HALF, 0.5, FLOOR_HALF));
    mesh_node(&mut out, "floor", 0, Mat4::IDENTITY, s, m_floor);

    // Пробковая доска на мольберте, лицом к столу (-X).
    let m_wood = out.materials.len() as i32;
    out.materials.push(material([0.32, 0.2, 0.11, 1.0], 0.7, 0.0, false));
    out.textures.push(cork_texture());
    let m_cork = out.materials.len() as i32;
    let mut cork = material([1.0, 1.0, 1.0, 1.0], 0.95, 0.0, false);
    cork.albedo_tex = out.textures.len() as i32 - 1;
    cork.uv_scale = [2.0, 1.5];
    out.materials.push(cork);
    let board_node = out.nodes.len();
    // Ось X узла доски — вдоль её ширины, -Z — смотрит на стол.
    out.nodes.push(node("board", Some(0), Kind::Spatial, facing(board, Vec3::Z)));
    let (hw, hh) = (BOARD_W * 0.5, BOARD_H * 0.5);
    let mut s = surface();
    add_box(&mut s, Mat4::IDENTITY, Vec3::new(hw, hh, 0.25));
    mesh_node(&mut out, "cork", board_node, Mat4::IDENTITY, s, m_cork);
    let mut s = surface();
    let f = 0.5;
    for (c, h) in [
        (Vec3::new(0.0, hh + f * 0.5, 0.0), Vec3::new(hw + f, f * 0.5, 0.45)),
        (Vec3::new(0.0, -hh - f * 0.5, 0.0), Vec3::new(hw + f, f * 0.5, 0.45)),
        (Vec3::new(hw + f * 0.5, 0.0, 0.0), Vec3::new(f * 0.5, hh, 0.45)),
        (Vec3::new(-hw - f * 0.5, 0.0, 0.0), Vec3::new(f * 0.5, hh, 0.45)),
    ] {
        add_box(&mut s, Mat4::from_translation(c), h);
    }
    // Мольберт: две передние ноги, задняя, полка под доской.
    let ground = floor_y - board.y;
    for x in [-hw * 0.6, hw * 0.6] {
        add_rod(&mut s, Vec3::new(x * 0.8, hh + 1.5, 0.6), Vec3::new(x, ground, -1.2), 0.3);
    }
    add_rod(&mut s, Vec3::new(0.0, hh + 1.5, 0.7), Vec3::new(0.0, ground, 7.0), 0.3);
    add_box(&mut s, Mat4::from_translation(Vec3::new(0.0, -hh - f - 0.4, -0.6)), Vec3::new(hw * 0.75, 0.25, 0.9));
    mesh_node(&mut out, "easel", board_node, Mat4::IDENTITY, s, m_wood);

    // Прожекторы на штативах (корпуса — копии оригинальных), стекло, свет.
    let m_metal = out.materials.len() as i32;
    out.materials.push(material([0.09, 0.09, 0.1, 1.0], 0.45, 0.8, false));
    let m_glass = out.materials.len() as i32;
    out.materials.push(material([1.0, 0.8, 0.6, 1.0], 0.2, 0.0, true));
    let lamps_node = out.nodes.len();
    out.nodes.push(node("lamps", Some(0), Kind::Spatial, Mat4::IDENTITY));
    for l in &lamps {
        let ln = out.nodes.len();
        out.nodes.push(node(&l.name, Some(lamps_node), Kind::Spatial, Mat4::from_translation(l.foot)));
        let mut local = std::collections::HashMap::new();
        for (k, &i) in l.src.iter().enumerate() {
            let mut nd = copy_node(&w_nodes[i]);
            if k == 0 {
                nd.parent = Some(ln);
                nd.xf = Mat4::from_translation(-l.foot) * l.place * global[i];
                nd.name = "projector".into();
            } else {
                nd.parent = w_nodes[i].parent.map(|p| local[&p]);
            }
            adopt(&mut nd, &mut out)?;
            local.insert(i, out.nodes.len());
            out.nodes.push(nd);
        }
        // Штатив со стрелой, в координатах лампы (начало — низ стойки на полу).
        let top = l.mount - l.foot + Vec3::Y * 0.3;
        let mut s = surface();
        let hub = Vec3::Y * (top.y * 0.4);
        let out_dir = Vec3::new(l.foot.x - l.mount.x, 0.0, l.foot.z - l.mount.z).normalize();
        let a = out_dir.z.atan2(out_dir.x);
        for j in 0..3 {
            let b = a + j as f32 * std::f32::consts::TAU / 3.0;
            add_rod(&mut s, hub, Vec3::new(b.cos() * 5.5, 0.15, b.sin() * 5.5), 0.22);
        }
        add_rod(&mut s, Vec3::Y * 0.1, Vec3::Y * (top.y + 0.3), 0.3);
        add_rod(&mut s, hub - Vec3::Y * 0.6, hub + Vec3::Y * 0.6, 0.55);
        add_rod(&mut s, Vec3::Y * top.y, Vec3::new(top.x, top.y, top.z), 0.25);
        mesh_node(&mut out, "stand", ln, Mat4::IDENTITY, s, m_metal);
        // Стекло — круглый диск на передней крышке, светится само; яркость — tint из скрипта.
        let face = l.head + l.axis * GLASS_AT - l.foot;
        let mut s = surface();
        add_disc(&mut s, facing(face, l.axis), GLASS_R, 0.05);
        mesh_node(&mut out, "glass", ln, Mat4::IDENTITY, s, m_glass);
        let mut light = node("light", Some(ln), Kind::Light, facing(face + l.axis * 0.6, l.axis));
        light.light = Some(LightInfo { spot: true, color: LIGHT_COLOR, energy: l.energy, range: l.range, angle: l.angle, attenuation: 1.0, shadow: true });
        out.nodes.push(light);
    }
    // Точка появления игрока: ноги на полу, взгляд (-Z узла) — на стол.
    let spawn = Vec3::new(center.x - SPAWN_DIST, floor_y, center.z);
    out.nodes.push(node("spawn", Some(0), Kind::Spatial, facing(spawn, Vec3::X)));
    Ok(out)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("list") if args.len() >= 3 => {
            let scene = SceneFile::load(&args[2]).unwrap_or_else(|e| panic!("{}", e));
            let pat = args.get(3).map(|s| s.to_lowercase()).unwrap_or_default();
            let mut g: Vec<Mat4> = Vec::with_capacity(scene.nodes.len());
            for n in &scene.nodes {
                let parent = n.parent.filter(|_| !n.top_level).map(|p| g[p]).unwrap_or(Mat4::IDENTITY);
                g.push(parent * n.xf);
            }
            for i in 0..scene.nodes.len() {
                let p = scene.path(i);
                let n = &scene.nodes[i];
                if p.to_lowercase().contains(&pat) && (n.mesh.is_some() || n.light.is_some()) {
                    let t = g[i].w_axis;
                    let light = n.light.map(|l| format!(" LIGHT spot={} color={:?} energy={} range={} angle={:.2} att={}", l.spot, l.color, l.energy, l.range, l.angle, l.attenuation)).unwrap_or_default();
                    // Габариты меша в мире.
                    let mut bb = String::new();
                    if let Some(m) = n.mesh {
                        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                        for s in &scene.meshes[m].surfaces {
                            for v in &s.vertices {
                                let q = g[i].transform_point3(Vec3::new(v[0], v[1], v[2]));
                                lo = lo.min(q);
                                hi = hi.max(q);
                            }
                        }
                        let tris: usize = scene.meshes[m].surfaces.iter().map(|s| s.indices.len() / 3).sum();
                        bb = format!(" box {:.1?}..{:.1?} mat={:?} tris={}", lo, hi, n.materials, tris);
                        if std::env::var("VOID_XF").is_ok() {
                            let c = |v: Vec4| format!("({:.2} {:.2} {:.2})", v.x, v.y, v.z);
                            bb += &format!("
        X{} Y{} Z{}", c(g[i].x_axis), c(g[i].y_axis), c(g[i].z_axis));
                        }
                    }
                    println!("{:5} vis={} [{:.1} {:.1} {:.1}] {}{}{}", i, n.visible as u8, t.x, t.y, t.z, p, light, bb);
                }
            }
        }
        Some("build") if args.len() >= 4 => {
            let run = || -> Result<(), String> {
                let out = build(SceneFile::load(&args[2])?)?;
                let path = &args[3];
                let alfar = std::path::Path::new(path).with_extension("alfar");
                let (altex, lights) = altex_write::convert(&out);
                altex.save(path).map_err(|e| format!("{}: {}", path, e))?;
                lights.save(&alfar.to_string_lossy()).map_err(|e| format!("{}: {}", alfar.display(), e))?;
                println!("{}: {} узлов, {} мешей, {} текстур, {} источников света", path, out.nodes.len(), out.meshes.len(), out.textures.len(), lights.lights.len());
                altex_write::compare(&out, &SceneFile::load(path)?)?;
                println!("проверка: перечитанная сцена совпадает");
                Ok(())
            };
            if let Err(e) = run() {
                eprintln!("ошибка: {}", e);
                std::process::exit(1);
            }
        }
        Some("anims") if args.len() >= 3 => {
            let scene = SceneFile::load(&args[2]).unwrap_or_else(|e| panic!("{}", e));
            let pat = args.get(3).map(|s| s.to_lowercase()).unwrap_or_default();
            for p in &scene.players {
                let path = scene.path(p.node);
                if !path.to_lowercase().contains(&pat) {
                    continue;
                }
                println!("{}", path);
                for a in &p.animations {
                    println!("  [{}] {:.2}s loop={}", a.name, a.length, a.looped);
                    for t in &a.tracks {
                        let tgt = t.target.map(|i| scene.path(i)).unwrap_or_else(|| "?".into());
                        let n = match &t.keys {
                            scene_file::Keys::Value(k) => k.len(),
                            scene_file::Keys::Method(k) => k.len(),
                            scene_file::Keys::Audio(k) => k.len(),
                        };
                        println!("      {} :{} ({} ключей)", tgt, t.property, n);
                        if std::env::var("VOID_KEYS").is_ok() {
                            if let scene_file::Keys::Value(k) = &t.keys {
                                for (time, _, v) in k {
                                    println!("          {:.2}: {:?}", time, v);
                                }
                            }
                        }
                    }
                }
            }
        }
        Some("mat") if args.len() >= 4 => {
            let scene = SceneFile::load(&args[2]).unwrap_or_else(|e| panic!("{}", e));
            let m = &scene.materials[args[3].parse::<usize>().unwrap()];
            let tex = (m.albedo_tex >= 0).then(|| &scene.textures[m.albedo_tex as usize]).map(|t| (t.width, t.height));
            println!("albedo={:?} tex={} {:?} normal={} rough={} metal={} emission={:?} transp={} unshaded={}", m.albedo, m.albedo_tex, tex, m.normal_tex, m.roughness, m.metallic, m.emission, m.transparency, m.unshaded);
        }
        _ => eprintln!("usage: void_scene build <мир.altex> <выход.altex> | list <мир.altex> [подстрока]"),
    }
}
