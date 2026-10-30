//! ПОДЪЕЗД — ночь, лестничная клетка пятиэтажки. Пройти с первого этажа
//! наверх, домой. Свет только настоящий: лампы накаливания 60 Вт на
//! площадках (тени от каждой, отражённый от стен свет — DDGI лучами),
//! натриевый фонарь во дворе за окнами. Одна лампа с плохим контактом,
//! одна перегорела.
//!
//! Цель: квартира 14 на пятом этаже — подойти к двери и нажать E.
//!
//! Управление: ЛКМ — захватить мышь, WASD — идти, Shift — быстрее,
//! E — открыть дверь / щёлкнуть выключателем у входа на площадку,
//! Esc — отпустить мышь, Shift+Esc — выйти.
//! Отладка: PODEZD_FRAMES=N — выйти через N кадров; PODEZD_CAM=x,y,z,yaw°,pitch°
//! — поставить камеру (ходьба выключена); PODEZD_WINDOWED=1 — окном;
//! PODEZD_GI_DEBUG=gain — только непрямой свет.

#[path = "../msc/mouse.rs"]
mod mouse;
#[path = "../msc/hud.rs"]
#[allow(dead_code)]
mod hud;
mod build;
mod sound;
mod tex;

use alkash3d_rs::engine::{AlkashEngine, GiVolume, GraphicsSettings, Lens, Mesh, MeshInstance, Vertex};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::{GPULight, LightConfig};
use windows::core::Interface;

use build::{LampKind, Mat, MATS};

const FIRSTFIRES_DLL_PATH: &str = "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll";
/// Единиц силы света движка на 1 кд (та же шкала, что в CODEC).
const ENGINE_PER_CANDELA: f32 = 0.1;
const EYE: f32 = 1.62;
const RADIUS: f32 = 0.22;
const WALK: f32 = 1.3;
const RUN: f32 = 2.6;
const SENSITIVITY: f32 = 0.0022;

fn firstfires_dll() -> String {
    let name = "alkash3d_firstfires.dll";
    let mut tries: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
        tries.push(dir.join(name));
    }
    tries.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIRSTFIRES_DLL_PATH));
    tries.into_iter().find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| FIRSTFIRES_DLL_PATH.to_string())
}

fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

/// Цвет излучения чёрного тела, линейный, нормированный на единичную яркость.
fn kelvin_rgb(k: f32) -> [f32; 3] {
    let t = k / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.7 * (t - 60.0).powf(-0.1332) };
    let g = if t <= 66.0 { 99.47 * t.ln() - 161.12 } else { 288.12 * (t - 60.0).powf(-0.0755) };
    let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { 138.52 * (t - 10.0).ln() - 305.04 };
    let l = [r, g, b].map(|c: f32| (c.clamp(0.0, 255.0) / 255.0).powf(2.2));
    let y = (0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2]).max(1e-6);
    l.map(|c| c / y)
}

fn hash1(i: u32) -> f32 {
    let mut h = i.wrapping_mul(2654435761);
    h = (h ^ (h >> 15)).wrapping_mul(2246822519);
    h ^= h >> 13;
    h as f32 / u32::MAX as f32
}

fn sphere(rings: u32, segs: u32) -> Mesh {
    let mut v = Vec::new();
    let mut idx = Vec::new();
    for i in 0..=rings {
        let th = std::f32::consts::PI * i as f32 / rings as f32;
        for j in 0..=segs {
            let ph = std::f32::consts::TAU * j as f32 / segs as f32;
            let n = Vec3::new(th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin());
            v.push(Vertex::with_normal_uv(n.x, n.y, n.z, n.x, n.y, n.z, 1.0, 1.0, 1.0, 1.0, j as f32 / segs as f32, i as f32 / rings as f32));
        }
    }
    let row = segs + 1;
    for i in 0..rings {
        for j in 0..segs {
            let (a, b, c, d) = (i * row + j, i * row + j + 1, (i + 1) * row + j + 1, (i + 1) * row + j);
            idx.extend([a, c, b, a, d, c]);
        }
    }
    Mesh::from_vertices_and_indices(&v, &idx).expect("сфера")
}

/// Лампа в игре: что задано и что с ней сейчас.
struct LiveLamp {
    spec: build::Lamp,
    id: Option<u32>,
    /// Ток через нить: 1 — есть, 0 — нет (контакт).
    contact: f32,
    /// Накал нити 0..1 — отстаёт от тока (тепловая инерция ~0.07 с).
    glow: f32,
    sent: f32,
    /// Выключена выключателем на площадке.
    off: bool,
}

impl LiveLamp {
    fn send(&mut self, engine: &mut AlkashEngine) {
        let Some(id) = self.id else { return };
        // остывающая нить краснее: цветовая температура падает с накалом
        let k = if self.spec.kind == LampKind::Dead { self.spec.kelvin } else { 1500.0 + (self.spec.kelvin - 1500.0) * self.glow.sqrt() };
        let c = kelvin_rgb(k);
        // свет окон дома напротив (10–30 кд): до подъезда ~26 м, там он меньше
        // 0.3% лампы на площадке, а с дальностью 30 м попадал в сетку света
        // подъезда — ещё 4 источника в цикле каждого пикселя внутри. С 20 м
        // пятна на снегу под окнами (до ~5 м) темнее лишь на ~6%.
        let range = if self.spec.range > 0.0 { self.spec.range } else if self.spec.candela > 500.0 { 90.0 } else if self.spec.cone.is_some() { 20.0 } else { 30.0 };
        let i = self.intensity();
        if let Some(lights) = engine.lights.as_mut() {
            let p = self.spec.pos;
            // params.z точечного света = −радиус светящегося тела (мягкие тени)
            let light = match self.spec.cone {
                // прожектор: конус вниз (params.x — край, params.z — начало спада)
                Some((outer, inner)) => {
                    // окна дома напротив светят к нам (−Z) и чуть вниз, фонарь — вниз
                    let dir = if self.spec.candela < 100.0 { [0.0, -0.35, -1.0] } else { [0.0, -1.0, 0.0] };
                    GPULight { position: [p.x, p.y, p.z, 1.0], color: [c[0], c[1], c[2], i], direction: [dir[0], dir[1], dir[2], range], params: [outer, 2.0, inner, 0.0] }
                }
                None => GPULight { position: [p.x, p.y, p.z, 0.0], color: [c[0], c[1], c[2], i], direction: [0.0, -1.0, 0.0, range], params: [std::f32::consts::PI, 2.0, -self.spec.bulb_r, 0.0] },
            };
            lights.update_light(id, &light);
        }
        self.sent = i;
    }

    fn intensity(&self) -> f32 {
        if self.spec.kind == LampKind::Dead {
            0.0
        } else {
            // световой поток нити растёт с температурой круто — ~ накал^3
            self.spec.candela * ENGINE_PER_CANDELA * self.glow.powi(3)
        }
    }
}

/// Перекрывает ли плита отрезок a→b (для адаптации глаза: свет с другого
/// этажа сквозь перекрытие не виден). Плиты — площадки и промежуточные.
fn blocked(a: Vec3, b: Vec3) -> bool {
    use build::*;
    let mut planes: Vec<(f32, f32, f32)> = Vec::new();
    for n in 1..FLOORS {
        planes.push((n as f32 * H - SLAB * 0.5, 0.0, LAND_Z));
    }
    for n in 0..FLOORS - 1 {
        planes.push((n as f32 * H + HALF - SLAB * 0.5, MID_Z, D));
    }
    planes.push((ROOF, 0.0, D));
    for (y, z0, z1) in planes {
        if (a.y - y) * (b.y - y) < 0.0 {
            let t = (y - a.y) / (b.y - a.y);
            let p = a + (b - a) * t;
            if p.z >= z0 && p.z <= z1 && p.x >= 0.0 && p.x <= W {
                return true;
            }
        }
    }
    // марши тоже плиты: грубо — середина марша
    false
}

/// Снаружи свет попадает к камере только через окна клетки.
fn through_window(a: Vec3, b: Vec3) -> bool {
    use build::*;
    if (a.z - D) * (b.z - D) >= 0.0 {
        return false;
    }
    let t = (D - a.z) / (b.z - a.z);
    let p = a + (b - a) * t;
    (0..FLOORS - 1).any(|n| {
        let f = n as f32 * H + HALF;
        p.x > 0.55 && p.x < 1.95 && p.y > f + 0.85 && p.y < f + 2.3
    })
}

/// Ходьба: ноги на самой высокой поверхности не выше колена, стены и
/// перила не пускают. ИЗМЕНЕНО (выход на улицу): из клетки в проём входной
/// двери — только в открытую дверь; в толще стены — только в пределах
/// проёма; снаружи — крыльцо, бордюры и препятствия двора (build::outside_*).
fn walk(pos: Vec3, feet: f32, step: Vec3, door_open: bool, obstacles: &[[f32; 4]]) -> (Vec3, f32) {
    use build::*;
    let mut p = pos + step;
    let in_doorway = |x: f32| x > DOOR_X0 + RADIUS && x < DOOR_X1 - RADIUS;
    if pos.z >= RADIUS - 1e-3 {
        // ---- в клетке ----
        p.x = p.x.clamp(RADIUS, W - RADIUS);
        let can_exit = door_open && feet < 0.3 && in_doorway(p.x);
        p.z = if can_exit { p.z.min(D - RADIUS) } else { p.z.clamp(RADIUS, D - RADIUS) };
        let well = (A_X + B_X) * 0.5;
        let in_flights = |z: f32| z > LAND_Z && z < MID_Z;
        // перила между маршами
        if in_flights(p.z) || in_flights(pos.z) {
            let right = pos.x >= well;
            if right {
                p.x = p.x.max(well + 0.2);
            } else {
                p.x = p.x.min(well - 0.2);
            }
        }
        // первый этаж: кладовка под маршем B и под промежуточной площадкой
        if feet < 1.0 && p.x < well && p.z > 1.6 - RADIUS {
            p.z = 1.6 - RADIUS;
        }
        // верхний этаж: ограждение над маршем A
        let top = (FLOORS - 1) as f32 * H;
        if feet > top - 0.3 && p.x > well - 0.2 && p.z > LAND_Z - 0.1 {
            if pos.z <= LAND_Z - 0.1 {
                p.z = LAND_Z - 0.1;
            } else {
                p.x = p.x.min(well - 0.2);
            }
        }
    } else if pos.z > FRONT_Z - RADIUS {
        // ---- в проёме (толща стены): только вдоль него ----
        p.x = p.x.clamp(DOOR_X0 + RADIUS, DOOR_X1 - RADIUS);
        if !door_open {
            // полотно закрылось: оно у внутренней грани стены
            p.z = p.z.min(-RADIUS);
        }
    } else {
        // ---- на улице ----
        if p.z > FRONT_Z - RADIUS && !(in_doorway(p.x) && door_open) {
            p.z = FRONT_Z - RADIUS;
        }
        if p.z > FRONT_Z - RADIUS && !door_open {
            p.z = p.z.min(-RADIUS);
        }
        let [bx0, bz0, bx1, _] = OUTSIDE_BOUNDS;
        p.x = p.x.clamp(bx0, bx1);
        p.z = p.z.max(bz0);
        // препятствия: вытолкнуть по оси наименьшего проникновения
        for o in obstacles {
            let (x0, z0, x1, z1) = (o[0] - RADIUS, o[1] - RADIUS, o[2] + RADIUS, o[3] + RADIUS);
            if p.x > x0 && p.x < x1 && p.z > z0 && p.z < z1 {
                let pen = [(p.x - x0, 0), (x1 - p.x, 1), (p.z - z0, 2), (z1 - p.z, 3)];
                let (_, side) = pen.into_iter().fold((f32::INFINITY, 0), |a, b| if b.0 < a.0 { b } else { a });
                match side {
                    0 => p.x = x0,
                    1 => p.x = x1,
                    2 => p.z = z0,
                    _ => p.z = z1,
                }
            }
        }
    }
    let floor = if p.z >= 0.0 {
        surfaces(p.x, p.z).into_iter().filter(|&h| h <= feet + 0.45).fold(f32::NEG_INFINITY, f32::max)
    } else {
        let f = outside_floor(p.x, p.z);
        if f <= feet + 0.45 { f } else { f32::NEG_INFINITY }
    };
    if floor.is_finite() {
        (p, floor)
    } else {
        (pos, feet)
    }
}

/// ДОБАВЛЕНО (дверь из отдельного меша): то, как материал выглядит —
/// текстуры и флаги. Вторая геометрия того же материала (полотно двери)
/// берёт их отсюда, не создавая текстуры заново.
#[derive(Clone, Copy)]
struct Look {
    albedo: Option<u32>,
    normal: Option<u32>,
    mr: Option<u32>,
    parallax: f32,
    blend: bool,
    glass: bool,
    alpha_cutoff: f32,
    unlit: bool,
    roughness: f32,
    metallic: f32,
}

impl Look {
    fn of(m: &Mesh) -> Look {
        Look {
            albedo: m.albedo_srv_index,
            normal: m.normal_srv_index,
            mr: m.mr_srv_index,
            parallax: m.parallax_depth,
            blend: m.blend,
            glass: m.glass,
            alpha_cutoff: m.alpha_cutoff,
            unlit: m.unlit,
            roughness: m.material_roughness,
            metallic: m.material_metallic,
        }
    }

    fn apply(&self, m: &mut Mesh) {
        m.albedo_srv_index = self.albedo;
        m.normal_srv_index = self.normal;
        m.mr_srv_index = self.mr;
        m.parallax_depth = self.parallax;
        m.blend = self.blend;
        m.glass = self.glass;
        m.alpha_cutoff = self.alpha_cutoff;
        m.unlit = self.unlit;
        m.material_roughness = self.roughness;
        m.material_metallic = self.metallic;
    }
}

struct Materials {
    meshes: Vec<(Mat, usize)>,
}

/// Скан настоящей поверхности под материал: id в assets/podezd/scan,
/// физический размер скана (м) и сила фактуры.
fn scan_for(m: Mat) -> Option<(&'static str, [f32; 2], f32)> {
    Some(match m {
        Mat::Paint => ("Paint004", [1.0, 1.0], 0.6),
        // побелка поверх штукатурки — фактура мягче, не «шуба»
        Mat::White => ("PaintedPlaster017", [2.0, 1.0], 0.4),
        Mat::Tread => ("Concrete034", [1.1, 0.55], 0.6),
        Mat::Concrete => ("Concrete034", [1.1, 0.55], 0.8),
        Mat::Metal => ("Paint004", [1.0, 1.0], 0.35),
        Mat::Wood => ("Paint004", [1.0, 1.0], 0.5),
        Mat::Door => ("Leather030", [0.45, 0.45], 0.7),
        // ИЗМЕНЕНО (текстуры): скан снега убран — его лунки давали рисунок
        // «песка» даже на 0.3; рельеф и блёстки теперь в tex::snow
        Mat::Plaster => ("Concrete046", [2.4, 2.4], 0.35),
        Mat::Plinth => ("Concrete046", [1.0, 1.0], 0.3),
        Mat::Facade => ("Concrete046", [2.4, 2.4], 0.7),
        _ => return None,
    })
}

/// Процедурный материал под `m` (None — у материала своей текстуры нет).
fn procedural(m: Mat) -> Option<tex::Material> {
    match m {
        Mat::Paint => Some(tex::wall_paint(1024)),
        Mat::White => Some(tex::whitewash(1024)),
        Mat::Tread => Some(tex::treads(2048)),
        Mat::Concrete => Some(tex::concrete(512)),
        Mat::Tiles => Some(tex::tiles(1024)),
        Mat::Metal => Some(tex::painted_metal(256)),
        Mat::Rail => Some(tex::handrail(128)),
        Mat::Door => Some(tex::leatherette(512)),
        Mat::Wood => Some(tex::wood_paint(256)),
        Mat::Snow => Some(tex::snow(512)),
        Mat::Facade => Some(tex::facade(512)),
        Mat::Plaster => Some(tex::plaster(2048)),
        Mat::Plinth => Some(tex::plinth(512)),
        Mat::Brick => Some(tex::brick(1024)),
        Mat::Sheet => Some(tex::corrugated(256)),
        Mat::Steel => Some(tex::rusty_steel(512)),
        Mat::Rug => Some(tex::rug(256)),
        Mat::Paper => Some(tex::paper(512)),
        _ => None,
    }
}

/// `geos` — по материалу MATS (b.geo или b.door); `looks` — уже созданные
/// материалы (общие для всех вызовов); `moving` — см. Mesh::moving.
fn upload(engine: &mut AlkashEngine, geos: &[build::Geo], looks: &mut std::collections::HashMap<Mat, Look>, moving: bool) -> Materials {
    let mut meshes = Vec::new();
    // PODEZD_NOSCAN=1 — чисто процедурные материалы (сравнение «до/после»)
    let use_scans = std::env::var("PODEZD_NOSCAN").is_err();
    let mut scans: std::collections::HashMap<&str, Option<tex::Scan>> = std::collections::HashMap::new();
    // ДОБАВЛЕНО (текстуры): процедурные материалы — все сразу по ядрам
    // (по одному загрузка росла на секунды с каждой новой текстурой)
    let built: std::collections::HashMap<Mat, tex::Material> = {
        use rayon::prelude::*;
        let todo: Vec<Mat> = MATS
            .iter()
            .enumerate()
            .filter(|&(k, m)| geos.get(k).is_some_and(|g| !g.i.is_empty()) && !looks.contains_key(m))
            .map(|(_, &m)| m)
            .collect();
        todo.par_iter().filter_map(|&m| procedural(m).map(|t| (m, t))).collect()
    };
    let mut built = built;
    for (k, &m) in MATS.iter().enumerate() {
        let Some(g) = geos.get(k) else { continue };
        if g.i.is_empty() {
            continue;
        }
        let mut mesh = Mesh::from_vertices_and_indices(&g.v, &g.i).expect("меш подъезда");
        mesh.moving = moving;
        if let Some(l) = looks.get(&m) {
            l.apply(&mut mesh);
            meshes.push((m, engine.add_mesh(mesh)));
            continue;
        }
        let mut mat: Option<tex::Material> = built.remove(&m);
        if let (Some(t), Some((id, world, k)), true) = (mat.as_mut(), scan_for(m), use_scans) {
            if let Some(s) = scans.entry(id).or_insert_with(|| tex::load_scan(id, world)) {
                tex::detail(t, s, k);
            }
        }
        // декали — альфа-смешивание поверх стены/потолка
        let decal = match m {
            Mat::Soot => Some(tex::soot(256)),
            Mat::Scribble => Some(tex::scribble(512)),
            Mat::Web => Some(tex::web(512)),
            Mat::Warning => Some(tex::warning(256)),
            Mat::Frost => Some(tex::frost(512)),
            Mat::Crack => Some(tex::cracks(1024)),
            _ => None,
        };
        if let Some(d) = decal {
            mesh.albedo_srv_index = engine.create_texture_rgba(d.size, d.size, &d.rgba);
            mesh.blend = true;
        }
        if m == Mat::Glass {
            let d = tex::glass_dirt(256);
            mesh.albedo_srv_index = engine.create_texture_rgba(d.size, d.size, &d.rgba);
        }
        if m == Mat::Wet {
            // вода на полу — как стекло: отражение по Френелю (лучами),
            // свой слой — мутная вода; край лужи отсечён по альфе, чтобы
            // G-буфер воды не залезал на сухую плитку
            let d = tex::puddles(512);
            mesh.albedo_srv_index = engine.create_texture_rgba(d.size, d.size, &d.rgba);
            mesh.blend = true;
            mesh.glass = true;
            mesh.alpha_cutoff = 0.05;
        }
        if m == Mat::WinLit {
            let (w, h, rgba) = tex::lit_window(128);
            mesh.albedo_srv_index = engine.create_texture_rgba(w, h, &rgba);
        }
        if m == Mat::Digits {
            let (w, h, rgba) = tex::digits();
            mesh.albedo_srv_index = engine.create_texture_rgba(w, h, &rgba);
            mesh.blend = true;
            mesh.material_roughness = 0.5;
        }
        if let Some(t) = mat {
            // настоящая глубина рельефа (швы, носки ступеней, стёжка) — в
            // долях тайла; у стен перепад десятые мм, там рельеф не нужен
            if t.relief > 0.001 && !matches!(m, Mat::Paint | Mat::White | Mat::Snow | Mat::Facade | Mat::Plaster | Mat::Plinth) {
                mesh.parallax_depth = t.relief / t.tile_u;
            }
            mesh.albedo_srv_index = engine.create_texture_rgba(t.size, t.size, &t.albedo);
            mesh.normal_srv_index = engine.create_texture_rgba(t.size, t.size, &t.normal);
            mesh.mr_srv_index = engine.create_texture_rgba(t.size, t.size, &t.mr);
        }
        mesh.material_metallic = 0.0;
        mesh.material_roughness = 0.8;
        match m {
            Mat::Glass => {
                mesh.blend = true;
                mesh.glass = true;
                mesh.material_roughness = 0.05;
            }
            Mat::Wet => mesh.material_roughness = 0.03,
            Mat::Brass => {
                mesh.material_metallic = 1.0;
                mesh.material_roughness = 0.35;
            }
            Mat::Black => mesh.material_roughness = 0.5,
            Mat::WinDark => mesh.material_roughness = 0.08,
            Mat::WinLit => mesh.unlit = true,
            _ => {}
        }
        looks.insert(m, Look::of(&mesh));
        meshes.push((m, engine.add_mesh(mesh)));
    }
    Materials { meshes }
}

fn parse_cam() -> Option<(Vec3, f32, f32)> {
    let s = std::env::var("PODEZD_CAM").ok()?;
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 5).then(|| (Vec3::new(v[0], v[1], v[2]), v[3].to_radians(), v[4].to_radians()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let frame_budget: Option<u32> = std::env::var("PODEZD_FRAMES").ok().and_then(|v| v.parse().ok());
    let perf_log = std::env::var("ALKASH3D_PERF").is_ok_and(|v| v != "0");
    let fixed_cam = parse_cam();
    // кадры — по развёртке монитора: без синхронизации ~70 кадр/с на 60 Гц
    // приходили неровно и рвались, плавный поворот головы дёргался
    // (PODEZD_VSYNC=0 — без синхронизации, для замеров FPS)
    if std::env::var("PODEZD_VSYNC").map(|v| v != "0").unwrap_or(true) {
        // SAFETY: в самом начале main, других потоков ещё нет
        unsafe { std::env::set_var("ALKASH3D_VSYNC", "1") };
    }

    let mut engine = AlkashEngine::new(1600, 900);
    // Воздух в подъезде чистый, лучей в нём не видно — экранные god rays были бы выдумкой.
    // MSAA при TAA лишний (края сглаживает TAA), а стоит ~1.5 мс на 1080p —
    // без него кадр влезает в 16.6 мс и vsync держит 60. PODEZD_MSAA=1 — включить.
    let taa = std::env::var("PODEZD_TAA").map(|v| v != "0").unwrap_or(true);
    let msaa = std::env::var("PODEZD_MSAA").map(|v| v != "0").unwrap_or(!taa);
    // доля разрешения, в которой рисуется кадр (TAA собирает полное):
    // 0.75 — кадр 1440x810 на 1080p, минус ~30% времени GPU; PODEZD_SCALE=1 — без досборки
    let render_scale = std::env::var("PODEZD_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.75);
    engine.set_graphics_settings(GraphicsSettings { volumetric: false, msaa, render_scale, ..GraphicsSettings::default() });
    engine.init()?;
    if std::env::var("PODEZD_WINDOWED").is_err() {
        engine.toggle_fullscreen();
    }
    // звук: только в обычной игре — замеры и снимки (PODEZD_CAM, скрытое
    // окно) идут молча; PODEZD_MUTE=1 — без звука
    // (PODEZD_AUDIO=1 — со звуком и при них, для проверки; PODEZD_VOLUME — общая громкость)
    let forced = std::env::var("PODEZD_AUDIO").is_ok_and(|v| v != "0");
    let muted = !forced && (fixed_cam.is_some() || std::env::var("ALKASH3D_HIDDEN").is_ok_and(|v| v != "0") || std::env::var("PODEZD_MUTE").is_ok_and(|v| v != "0"));
    if !muted {
        if let Err(e) = engine.init_audio() {
            eprintln!("[ПОДЪЕЗД] звук недоступен: {:?}", e);
        }
    }
    let mut sfx = sound::Sfx::new(&mut engine);
    let device_ptr = alkash3d_rs::get_device()?.as_raw();
    let config = LightConfig { max_lights: 64, tile_size: 16, far_plane: 500.0, lod_distances: [40.0, 100.0, 200.0], grid_cell_size: 25.0 };
    if let Err(e) = engine.init_lights(&firstfires_dll(), device_ptr, config) {
        eprintln!("[ПОДЪЕЗД] FirstFires не загрузился ({:?}) — света не будет", e);
    }

    let t0 = std::time::Instant::now();
    let building = build::build();
    let mut looks = std::collections::HashMap::new();
    let mats = upload(&mut engine, &building.geo, &mut looks, false);
    let back_mats = upload(&mut engine, &building.back, &mut looks, false);
    let front_mats = upload(&mut engine, &building.front, &mut looks, false);
    // полотно входной двери — своим мешем, поворачивается на петле
    let door_mats = upload(&mut engine, &building.door, &mut looks, true);
    let obstacles = build::outside_obstacles();
    // ДОБАВЛЕНО (forward-декали): атлас пятен двора и их места
    {
        let (n, atlas, rect) = tex::decal_atlas();
        let decals: Vec<alkash3d_rs::engine::Decal> = building
            .decals
            .iter()
            .map(|d| {
                let r = rect(d.cell);
                let (du, dv) = (r[2] - r[0], r[3] - r[1]);
                let uv = [r[0] + du * d.sub[0], r[1] + dv * d.sub[1], r[0] + du * d.sub[2], r[1] + dv * d.sub[3]];
                let mut dc = alkash3d_rs::engine::Decal::on_plane(d.center, d.normal, d.up, d.w, d.h, 0.25, uv, d.color);
                dc.roughness = d.roughness;
                dc
            })
            .collect();
        // отладка: PODEZD_NODECALS=1 — без декалей (сравнение)
        let decals = if std::env::var("PODEZD_NODECALS").is_ok() { Vec::new() } else { decals };
        if let Err(e) = engine.set_decals(&atlas, n, n, &decals) {
            eprintln!("[ПОДЪЕЗД] декали: {:?}", e);
        }
    }
    println!("[ПОДЪЕЗД] геометрия и материалы: {:.2} с", t0.elapsed().as_secs_f32());

    let mut bulb = sphere(12, 24);
    bulb.unlit = true;
    let bulb_lit = engine.add_mesh(bulb);
    let mut dead = sphere(12, 24);
    dead.material_roughness = 0.15;
    let bulb_dead = engine.add_mesh(dead);
    // тело игрока — только для тени: голова, торс, ноги (камера в голове)
    let mut body_mesh = sphere(8, 12);
    body_mesh.shadow_only = true;
    // движется каждый кадр — в тенях ламп рисуется поверх кэша статики
    body_mesh.moving = true;
    let body = engine.add_mesh(body_mesh);
    // прозрачная колба (лампы в подъездах — без матирования): стекло
    // с отражением по Френелю, внутри видна нить
    let mut glass = sphere(16, 32);
    glass.blend = true;
    glass.glass = true;
    glass.material_roughness = 0.02;
    let bulb_glass = engine.add_mesh(glass);
    // нить: светящееся тело, яркость задаётся на кадр
    let mut fil = sphere(4, 8);
    fil.unlit = true;
    let filament = engine.add_mesh(fil);
    // угловой размер пикселя по вертикали (окно 900 px)
    let px_angle = engine.camera.fov / 900.0;

    // ночное небо над городом: засветка снизу (натрий, окна) отражается от
    // облаков — у горизонта буро-оранжевое, к зениту темнее
    let sky = {
        let (rings, segs) = (12u32, 32u32);
        let mut v = Vec::new();
        let mut idx = Vec::new();
        for i in 0..=rings {
            let th = std::f32::consts::FRAC_PI_2 * i as f32 / rings as f32; // 0 — зенит
            let h = th.cos(); // высота над горизонтом
            let glow = (1.0 - h).powi(3);
            // яркость, кд/м² * 0.1: ~0.03 кд/м² у горизонта, ~0.004 в зените
            let lum = 0.0004 + 0.0028 * glow;
            let c = [lum * 1.25, lum * 0.92, lum * 0.62];
            for j in 0..=segs {
                let ph = std::f32::consts::TAU * j as f32 / segs as f32;
                let d = Vec3::new(th.sin() * ph.cos(), h, th.sin() * ph.sin());
                v.push(Vertex::with_normal_uv(d.x, d.y, d.z, -d.x, -d.y, -d.z, c[0], c[1], c[2], 1.0, 0.0, 0.0));
            }
        }
        let row = segs + 1;
        for i in 0..rings {
            for j in 0..segs {
                let (a, b, c, d) = (i * row + j, i * row + j + 1, (i + 1) * row + j + 1, (i + 1) * row + j);
                idx.extend([a, c, b, a, d, c]);
            }
        }
        let mut m = Mesh::from_vertices_and_indices(&v, &idx).expect("небо");
        m.unlit = true;
        engine.add_mesh(m)
    };

    // ДОБАВЛЕНО (следы на снегу): плоский квад отпечатка, носок к −Z (туда
    // же смотрит look_dir при повороте на курс); полупрозрачный — без теней
    let footprint = {
        let (l, w) = (0.15f32, 0.055f32);
        let v = [
            Vertex::with_normal_uv(-w, 0.0, -l, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0),
            Vertex::with_normal_uv(w, 0.0, -l, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0),
            Vertex::with_normal_uv(w, 0.0, l, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0),
            Vertex::with_normal_uv(-w, 0.0, l, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0),
        ];
        let mut m = Mesh::from_vertices_and_indices(&v, &[0, 2, 1, 0, 3, 2]).expect("след");
        let d = tex::footprint(256);
        m.albedo_srv_index = engine.create_texture_rgba(d.size, d.size, &d.rgba);
        m.blend = true;
        m.material_roughness = 0.85;
        engine.add_mesh(m)
    };
    let mut prints: std::collections::VecDeque<(Vec3, f32)> = std::collections::VecDeque::new();
    let mut left_foot = false;
    // отладка: PODEZD_TRAIL=1 — цепочка следов от старта вдоль проезда (снимок)
    if std::env::var("PODEZD_TRAIL").is_ok() {
        let yaw0 = -std::f32::consts::FRAC_PI_2;
        let side = look_dir(yaw0, 0.0).cross(Vec3::Y).normalize();
        for k in 0..16 {
            let x = -2.0 + k as f32 * 0.7;
            let z = -5.4 + 0.15 * (k as f32 * 0.7).sin();
            let s = if k % 2 == 0 { -0.11 } else { 0.11 };
            prints.push_back((Vec3::new(x + side.x * s, build::outside_floor(x, z) + 0.004, z + side.z * s), yaw0 + 0.05 * (k as f32).sin()));
        }
    }
    // снег: хлопья падают во дворе, видны в окна и в свете фонаря
    let mut flake_mesh = sphere(3, 6);
    flake_mesh.material_roughness = 0.9;
    let flake = engine.add_mesh(flake_mesh);
    let flake_n = 500u32;
    let mut flakes: Vec<[f32; 4]> = (0..flake_n).map(|i| [hash1(i * 3) * 18.0 - 8.0, hash1(i * 3 + 1) * 16.0, build::D + 1.0 + hash1(i * 3 + 2) * 13.0, hash1(i * 7 + 5)]).collect();
    // ДОБАВЛЕНО (выход на улицу): снег и перед домом — по всему двору,
    // рисуются только хлопья ближе 18 м (каждый — отдельный draw)
    flakes.extend((0..1400u32).map(|i| {
        let j = i + 10_000;
        [build::HOUSE_X0 - 4.0 + hash1(j * 3) * (build::HOUSE_X1 - build::HOUSE_X0 + 8.0), hash1(j * 3 + 1) * 16.0, -12.0 + hash1(j * 3 + 2) * 11.4, hash1(j * 7 + 5)]
    }));
    // угловой размер пикселя (рад) — по вертикальному углу обзора окна
    let pixel_angle = engine.camera.fov / 900.0;

    let mut lamps: Vec<LiveLamp> = building.lamps.into_iter().map(|spec| LiveLamp { spec, id: None, contact: 1.0, glow: 1.0, sent: -1.0, off: false }).collect();
    if let Some(lights) = engine.lights.as_mut() {
        for l in &mut lamps {
            l.id = Some(lights.add_light(&GPULight { position: [0.0; 4], color: [0.0; 4], direction: [0.0, -1.0, 0.0, 1.0], params: [std::f32::consts::PI, 2.0, 0.0, 0.0] }));
        }
    }
    for l in &mut lamps {
        if l.spec.kind == LampKind::Dead {
            l.glow = 0.0;
        }
        l.send(&mut engine);
    }

    // геометрия не меняется — ей место в видеопамяти: чтение через PCIe
    // стоило ~12 нс на треугольник, 5 мс на дворе в открытую дверь
    // (PODEZD_VRAM=0 — оставить в оперативной, для сравнения)
    if std::env::var("PODEZD_VRAM").map(|v| v != "0").unwrap_or(true) {
        match engine.meshes_to_video_memory() {
            Ok(bytes) => println!("[ПОДЪЕЗД] геометрия в видеопамяти: {:.0} МБ", bytes as f64 / 1e6),
            Err(e) => eprintln!("[ПОДЪЕЗД] геометрия осталась в оперативной памяти: {:?}", e),
        }
    }

    // непрямой свет — во всей клетке, шаг сетки проб 0.45 м
    // все пробы — каждый кадр, с меньшей инерцией: отражённый свет гаснет
    // вместе с мигающей лампой, а не догоняет её через секунду
    let mut volume = GiVolume::new(Vec3::new(-0.1, -0.3, -0.1), Vec3::new(build::W + 0.1, build::ROOF + 0.2, build::D + 0.1), 0.45);
    volume.probes_per_frame = std::env::var("PODEZD_GI_PROBES").ok().and_then(|v| v.parse().ok()).unwrap_or(2048);
    volume.hysteresis = std::env::var("PODEZD_GI_HYST").ok().and_then(|v| v.parse().ok()).unwrap_or(0.85);
    let gi_on = engine.set_gi_volume(Some(volume));
    println!("[ПОДЪЕЗД] непрямой свет: {}", if gi_on { "лучами (DDGI)" } else { "нет (DXR недоступен)" });
    if let Some(g) = std::env::var("PODEZD_GI_DEBUG").ok().and_then(|v| v.parse::<f32>().ok()) {
        engine.set_gi_debug(g);
    }

    engine.set_clear_color(0.004, 0.0036, 0.0042, 1.0);
    // как у настоящей камеры: края кадра темнее (cos^4), в темноте — шум
    // 1 единица яркости = 10 кд/м² (сила света 0.1 на кд, см. ENGINE_PER_CANDELA)
    // затенение рассеянного света в углах, стыках и под предметами —
    // сильнее, чем по умолчанию: свет в подъезде в основном отражённый
    engine.ssao_radius = 0.6;
    engine.ssao_strength = 2.5;
    // без шума сенсора: зерно делало картинку шершавой
    engine.lens = Lens { vignetting: true, sensor_noise: 0.0, aces_full: true, vision_cd_per_unit: 1.0 / ENGINE_PER_CANDELA, ..Default::default() };
    // объектив: лёгкая дисторсия (сильная «плывёт» по краям при повороте
    // головы) и цветная кайма к краям (PODEZD_CAMERA=0 — чистый кадр)
    if std::env::var("PODEZD_CAMERA").map(|v| v != "0").unwrap_or(true) {
        engine.lens.distortion = 0.03;
        engine.lens.chromatic_aberration = 0.0025;
    }
    // размытие в движении — выдержка как у кинокамеры (обтюратор 180°): при
    // повороте головы кадр смазан, как на видео; PODEZD_MBLUR=0 — выключить
    engine.lens.motion_blur = std::env::var("PODEZD_MBLUR").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
    // локальная экспозиция — как видит глаз: и колба, и тёмный угол разом
    // (PODEZD_LOCALEXP=сила, 0 — выключить)
    engine.lens.local_exposure = std::env::var("PODEZD_LOCALEXP").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
    // экспонометр: средняя яркость кадра (а не прикидка по лампам)
    engine.metering = true;
    // двор: идёт снег, воздух влажный — свет фонаря виден конусом
    // (рассеяние ~0.03 1/м — видимость около 100 м; снежинки рассеивают
    // вперёд, g = 0.6). В подъезде воздух чистый — среды нет.
    // отладка: PODEZD_NOMEDIUM=1 — без среды во дворе, PODEZD_NOSNOW=1 — без снега,
    // PODEZD_NOGLASS=1 — без стёкол
    let no_snow = std::env::var("PODEZD_NOSNOW").is_ok();
    let no_glass = std::env::var("PODEZD_NOGLASS").is_ok();
    if std::env::var("PODEZD_NOMEDIUM").is_err() {
    engine.medium = Some(alkash3d_rs::engine::ParticipatingMedium { min: [-60.0, -5.0, build::D + 0.4], max: [60.0, 40.0, build::D + 60.0], scattering: 0.03, anisotropy: 0.6 });
    }
    // сглаживание по времени: тонкие балясины и провода не мерцают
    engine.taa = taa;
    // отражения лучами (блестящая краска, плитка, стёкла дверей)
    engine.rt_reflections = std::env::var("PODEZD_RTREFL").map(|v| v != "0").unwrap_or(true);
    engine.set_day_night_speed(0.0);
    engine.camera.near = 0.03;
    engine.camera.far = 300.0;
    engine.camera.fov = 72f32.to_radians();
    if let Err(e) = engine.warm_up_pipelines() {
        eprintln!("[ПОДЪЕЗД] прогрев PSO: {:?}", e);
    }

    // надписи: шрифт CODEC (если нет — играем без подсказок)
    let mut hud = hud::Hud::load(&mut engine, concat!(env!("CARGO_MANIFEST_DIR"), "/../data/codec/font.bin")).map_err(|e| eprintln!("[ПОДЪЕЗД] шрифт: {}", e)).ok();
    // дверь квартиры 14: пятый этаж, правая стена
    let home = Vec3::new(build::W - 0.45, (build::FLOORS - 1) as f32 * build::H, 0.75);
    let mut ending: Option<f32> = None;
    let mut mouse = mouse::Mouse::new();
    mouse.set_captured(fixed_cam.is_none());
    // у входной двери, лицом к лестнице
    // ИЗМЕНЕНО (выход на улицу): вышел вынести мусор — стоишь на проезде у
    // своего подъезда, контейнеры — у гаражей направо
    let mut pos = Vec3::new(3.6, 0.0, -5.6);
    let mut feet = build::outside_floor(pos.x, pos.z);
    let mut eye_y = feet;
    let mut yaw = -std::f32::consts::FRAC_PI_2;
    // входная дверь: угол полотна (рад, наружу), куда идёт, когда открыли
    let mut door_angle = 0.0f32;
    let mut door_target = 0.0f32;
    let mut door_opened_at = -100.0f32;
    let mut has_bag = true;
    // путь с прошлого шага (звук шагов)
    let mut step_acc = 0.0f32;
    let mut pitch = 0.0f32;
    let mut stride = 0.0f32;
    let mut exposure = 1.0f32;
    let start = std::time::Instant::now();
    let mut last = 0.0f32;
    let mut frames = 0u32;
    // плохой контакт: когда начинается следующая серия миганий
    let mut burst_until = 0.0f32;
    let mut next_burst = 2.0f32;

    while engine.is_running() {
        engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).clamp(0.0001, 0.1);
        last = now;
        let input = &engine.input;
        if input.just_pressed(keys::ESCAPE) {
            if input.is_down(keys::SHIFT) || !mouse.captured {
                break;
            }
            mouse.set_captured(false);
        }
        if input.just_pressed(keys::MOUSE_LEFT) && !mouse.captured && fixed_cam.is_none() {
            mouse.set_captured(true);
        }

        // ---- взгляд и шаги ----
        let (mdx, mdy) = mouse.delta();
        yaw -= mdx * SENSITIVITY;
        pitch = (pitch - mdy * SENSITIVITY).clamp(-1.45, 1.45);
        let input = &engine.input;
        let fwd = look_dir(yaw, 0.0);
        let right = fwd.cross(Vec3::Y).normalize();
        let mut v = Vec3::ZERO;
        if input.is_down(keys::W) { v += fwd; }
        if input.is_down(keys::S) { v -= fwd; }
        if input.is_down(keys::D) { v += right; }
        if input.is_down(keys::A) { v -= right; }
        let speed = if input.is_down(keys::SHIFT) { RUN } else { WALK };
        let step = v.normalize_or_zero() * speed * dt;
        let door_open = door_angle > 1.2;
        let pos_before = pos;
        let (np, floor) = walk(Vec3::new(pos.x, 0.0, pos.z), feet, Vec3::new(step.x, 0.0, step.z), door_open, &obstacles);
        pos.x = np.x;
        pos.z = np.z;
        // ноги догоняют пол (для коллизий — сразу), а глаза — плавно: на
        // марше ступени по 15 см сливаются в ровный подъём, без толчка на
        // каждой; покачивания головы при шаге нет — оно читалось тряской
        feet += (floor - feet) * (1.0 - (-dt * 14.0).exp());
        eye_y += (feet - eye_y) * (1.0 - (-dt * 5.0).exp());
        stride += step.length();
        // ДОБАВЛЕНО (звук): шаг — каждые ~0.7 м (бегом длиннее); снег во
        // дворе, бетон крыльца и проёма, гулкий бетон клетки
        if fixed_cam.is_none() && ending.is_none() {
            step_acc += Vec3::new(np.x - pos_before.x, 0.0, np.z - pos_before.z).length();
            let run = engine.input.is_down(keys::SHIFT);
            if step_acc > if run { 0.85 } else { 0.7 } {
                step_acc = 0.0;
                let surface = if pos.z >= 0.0 {
                    sound::Surface::Stairwell
                } else if feet > build::GROUND_Y + 0.1 {
                    sound::Surface::Street
                } else {
                    sound::Surface::Snow
                };
                sfx.step(&mut engine, surface, run);
                // след на снегу: попеременно левой и правой, поперёк — на 11 см
                if surface == sound::Surface::Snow {
                    left_foot = !left_foot;
                    let side = look_dir(yaw, 0.0).cross(Vec3::Y).normalize() * if left_foot { -0.11 } else { 0.11 };
                    prints.push_back((Vec3::new(pos.x + side.x, feet + 0.004, pos.z + side.z), yaw));
                    if prints.len() > 300 {
                        prints.pop_front();
                    }
                }
            }
        }
        let (eye, yaw_v, pitch_v) = match fixed_cam {
            // PODEZD_SPIN=рад/с — камера поворачивается (проверка TAA в движении)
            Some((p, y, pt)) => (p, y + now * std::env::var("PODEZD_SPIN").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0), pt),
            None => (Vec3::new(pos.x, eye_y + EYE, pos.z), yaw, pitch),
        };
        engine.camera.position = eye;
        engine.camera.target = eye + look_dir(yaw_v, pitch_v);

        // ---- лампы ----
        if now > next_burst {
            burst_until = now + 0.4 + hash1(frames) * 1.6;
            next_burst = burst_until + 1.5 + hash1(frames + 77) * 7.0;
        }
        // отладка: PODEZD_OFF=номер этажа (0..4) — лампа площадки выключена
        if frames == 0 {
            if let Some(n) = std::env::var("PODEZD_OFF").ok().and_then(|v| v.parse::<usize>().ok()) {
                let (y0, y1) = (n as f32 * build::H, (n + 1) as f32 * build::H);
                if let Some(l) = lamps.iter_mut().find(|l| l.spec.pos.z > 0.0 && l.spec.pos.z < build::D && l.spec.pos.y > y0 && l.spec.pos.y < y1) {
                    l.off = true;
                }
            }
        }
        for l in &mut lamps {
            match l.spec.kind {
                LampKind::Flicker => {
                    l.contact = if now < burst_until { (hash1((now * 25.0) as u32 + 13) > 0.45) as u32 as f32 } else { 1.0 };
                }
                LampKind::Steady => l.contact = 1.0,
                LampKind::Dead => l.contact = 0.0,
            }
            if l.off {
                l.contact = 0.0;
            }
            // нить греется быстрее, чем остывает
            let tau = if l.contact > l.glow { 0.04 } else { 0.09 };
            l.glow += (l.contact - l.glow) * (1.0 - (-dt / tau).exp());
            if (l.intensity() - l.sent).abs() > 1e-3 * l.sent.max(0.01) {
                l.send(&mut engine);
            }
        }

        // ---- что рисовать ----
        let mut out: Vec<MeshInstance> = Vec::new();
        // ДОБАВЛЕНО (группы видимости): клетка, двор за домом, двор перед
        // домом. Изнутри передний двор виден только в открытую дверь, снаружи
        // клетка — тоже только в неё, задний двор с улицы не виден (дом).
        let door_ajar = door_angle > 0.02;
        let cam_inside = eye.z > build::FRONT_Z;
        // непрямой свет считается только в клетке — с улицы за закрытой
        // дверью её не видно, пробы не обновляем (−1.2…1.9 мс)
        engine.gi_paused = !cam_inside && !door_ajar;
        let groups: [(&Materials, bool); 3] = [(&mats, cam_inside || door_ajar), (&back_mats, cam_inside), (&front_mats, !cam_inside || door_ajar || eye.z < 0.0)];
        for (set, visible) in groups {
            if !visible {
                continue;
            }
            for &(m, mesh) in &set.meshes {
                if m == Mat::Glass && no_glass {
                    continue;
                }
                let inst = MeshInstance::new(mesh);
                out.push(match m {
                    // доля пыльного слоя: текстура (0.25..1) * 0.2
                    Mat::Glass => inst.tinted(0.9, 0.92, 0.9, 0.2),
                    Mat::Brass => inst.tinted(0.78, 0.6, 0.32, 1.0),
                    Mat::WinDark => inst.tinted(0.02, 0.022, 0.025, 1.0),
                    _ => inst,
                });
            }
        }
        if !cam_inside || door_ajar {
            for &(p, y) in &prints {
                if (p - eye).length() < 30.0 {
                    out.push(MeshInstance::new(footprint).at(p.x, p.y, p.z).rotated(0.0, y, 0.0).tinted(1.0, 1.0, 1.0, 0.99));
                }
            }
        }
        let h = build::DOOR_HINGE;
        for &(_, mesh) in &door_mats.meshes {
            out.push(MeshInstance::new(mesh).at(h.x, h.y, h.z).rotated(0.0, door_angle, 0.0));
        }
        for l in &lamps {
            let p = l.spec.pos;
            let r = l.spec.bulb_r;
            // свет окон напротив — без видимого тела (светится само окно)
            if r <= 0.0 {
                continue;
            }
            if l.spec.clear {
                // колба 55 мм: стекло почти прозрачно; пыль на нём ~0.3% —
                // у самой нити (74 тыс. лк) и этого хватает на лёгкое свечение
                out.push(MeshInstance::new(bulb_glass).at(p.x, p.y, p.z).scaled(0.0275, 0.034, 0.0275).tinted(0.95, 0.93, 0.88, 0.003));
                // нить — спираль ~20 x 1.5 мм: яркость L = I / видимая
                // площадь; меньше пикселя — растягиваем до пикселя, яркость
                // делим на ту же долю (энергия на экране та же)
                let (fl, fr): (f32, f32) = (0.01, 0.0012);
                let d = (p - eye).length().max(0.05);
                let pix = d * px_angle * 0.5;
                let (sl, sr) = (fl.max(pix), fr.max(pix));
                let area_k = (fl * fr) / (sl * sr);
                if l.spec.kind != LampKind::Dead && l.glow > 0.01 {
                    let k = kelvin_rgb(1500.0 + (l.spec.kelvin - 1500.0) * l.glow.sqrt());
                    let lum = l.intensity() / (std::f32::consts::PI * fl * fr) * area_k;
                    out.push(MeshInstance::new(filament).at(p.x, p.y, p.z).scaled(sl, sr, sr).tinted(k[0] * lum, k[1] * lum, k[2] * lum, 1.0));
                } else {
                    // перегоревшая: нить оборвана и потемнела — серый штрих
                    out.push(MeshInstance::new(bulb_dead).at(p.x - 0.004, p.y, p.z).scaled(fl * 0.5, fr, fr).tinted(0.08, 0.08, 0.08, 1.0));
                }
            } else if l.spec.kind == LampKind::Dead {
                out.push(MeshInstance::new(bulb_dead).at(p.x, p.y, p.z).scaled(r, r * 1.2, r).tinted(0.55, 0.55, 0.52, 1.0));
            } else {
                // яркость матовой колбы: L = I / (π r²) — сила света, делённая на видимую площадь
                let k = kelvin_rgb(1500.0 + (l.spec.kelvin - 1500.0) * l.glow.sqrt());
                let lum = l.intensity() / (std::f32::consts::PI * r * r);
                out.push(MeshInstance::new(bulb_lit).at(p.x, p.y, p.z).scaled(r, r * 1.2, r).tinted(k[0] * lum, k[1] * lum, k[2] * lum, 1.0));
            }
        }
        // тень игрока: эллипсоиды по росту ~1.75 м, повёрнуты по взгляду
        // отладка: PODEZD_BODY=x,ноги_y,z,курс° — тело стоит там (при PODEZD_CAM)
        let body_at = match fixed_cam {
            None => Some((Vec3::new(pos.x, feet, pos.z), yaw)),
            Some(_) => std::env::var("PODEZD_BODY").ok().and_then(|s| {
                let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                // PODEZD_BODY_SPIN=рад/с — тело поворачивается (тени ламп в движении)
                let spin = std::env::var("PODEZD_BODY_SPIN").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                (v.len() == 4).then(|| (Vec3::new(v[0], v[1], v[2]), v[3].to_radians() + now * spin))
            }),
        };
        if let Some((at, yaw)) = body_at {
            let f = look_dir(yaw, 0.0);
            let side = f.cross(Vec3::Y).normalize();
            let base = at - f * 0.05;
            let swing = (stride * std::f32::consts::PI / 0.75).sin() * 0.12 * (v.length() > 0.0) as u32 as f32;
            let parts: [(Vec3, Vec3); 5] = [
                (base + Vec3::Y * (EYE + 0.02) - f * 0.03, Vec3::new(0.09, 0.12, 0.1)),
                (base + Vec3::Y * 1.2, Vec3::new(0.19, 0.3, 0.12)),
                (base + Vec3::Y * 0.85, Vec3::new(0.16, 0.12, 0.11)),
                (base + Vec3::Y * 0.42 + side * 0.09 + f * swing, Vec3::new(0.07, 0.42, 0.07)),
                (base + Vec3::Y * 0.42 - side * 0.09 - f * swing, Vec3::new(0.07, 0.42, 0.07)),
            ];
            let yaw_r = yaw;
            for (c, r) in parts {
                out.push(MeshInstance::new(body).at(c.x, c.y, c.z).rotated(0.0, yaw_r, 0.0).scaled(r.x, r.y, r.z));
            }
        }
        // пыль в воздухе у ламп: пылинки ~0.3 мм медленно плавают в
        // конвекции над горячей колбой; видны только там, где их освещает
        // лампа (освещаются как обычные тела — без подделки свечения)
        for (li, l) in lamps.iter().enumerate() {
            if l.spec.pos.z > build::D || l.spec.pos.z < 0.0 || (l.spec.pos - eye).length() > 4.0 {
                continue;
            }
            for k in 0..60u32 {
                let h = |m: u32| hash1(li as u32 * 1000 + k * 7 + m);
                let ph = now * (0.05 + h(1) * 0.08) + h(2) * 50.0;
                let rr = 0.25 + h(3) * 0.9;
                let p = l.spec.pos + Vec3::new(ph.cos() * rr, -0.2 - h(4) * 1.2 + ((now * 0.12 + h(5) * 9.0).sin()) * 0.15, ph.sin() * rr);
                let r: f32 = 0.00015;
                let d = (p - eye).length();
                let rr2 = r.max(d * pixel_angle * 0.6);
                let k2 = (r / rr2) * (r / rr2);
                out.push(MeshInstance::new(flake).at(p.x, p.y, p.z).scaled(rr2, rr2, rr2).tinted(0.8 * k2, 0.78 * k2, 0.72 * k2, 0.99));
            }
        }
        // небо — купол вокруг камеры (бесконечно далеко)
        out.push(MeshInstance::new(sky).at(eye.x, eye.y - 5.0, eye.z).scaled(250.0, 250.0, 250.0));
        // снег: ~1 м/с вниз, лёгкий снос ветром и покачивание
        for f in flakes.iter().filter(|f| !no_snow && (f[0] - eye.x).abs() < 18.0 && (f[2] - eye.z).abs() < 18.0) {
            let fall = 0.9 + f[3] * 0.5;
            let y = build::GROUND_Y + (f[1] - now * fall).rem_euclid(16.0);
            let x = f[0] + (now * 0.6 + f[3] * 20.0).sin() * 0.25;
            let z = f[2] + (now * 0.45 + f[3] * 13.0).cos() * 0.2;
            let r = 0.004 + f[3] * 0.004;
            // меньше пикселя — рисуем в пиксель, но темнее во столько же раз
            // по площади: свет, который хлопья отражают в камеру, тот же
            let d = (Vec3::new(x, y, z) - eye).length();
            let rr = r.max(d * pixel_angle * 0.6);
            let k = (r / rr) * (r / rr);
            // альфа чуть меньше 1 — прозрачный проход: без теней и лучей
            // (иначе 500 хлопьев попадают в кубы теней всех ламп)
            out.push(MeshInstance::new(flake).at(x, y, z).scaled(rr, rr, rr).tinted(0.9 * k, 0.92 * k, 0.95 * k, 0.99));
        }
        // ---- цель: дойти до своей двери ----
        let at_home = fixed_cam.is_none() && (feet - home.y).abs() < 0.3 && Vec3::new(pos.x - home.x, 0.0, pos.z - home.z).length() < 0.75;
        if at_home && ending.is_none() && engine.input.just_pressed(keys::E) {
            ending = Some(now);
        }
        // ДОБАВЛЕНО (выход на улицу): входная дверь — изнутри ручкой, снаружи
        // ключом от домофона; доводчик закрывает её через 4 с, если в проёме
        // никого нет
        let in_doorway = pos.x > build::DOOR_X0 - 0.2 && pos.x < build::DOOR_X1 + 0.2 && pos.z > -1.0 && pos.z < 0.9;
        let door_inside = fixed_cam.is_none() && feet.abs() < 0.3 && pos.z > 0.0 && pos.z < 1.1 && pos.x > 0.85 && pos.x < 1.9;
        let door_outside = fixed_cam.is_none() && pos.z < build::FRONT_Z && pos.z > -1.7 && pos.x > 0.4 && pos.x < 2.2;
        let door_closed = door_target == 0.0;
        if (door_inside || door_outside) && door_closed && ending.is_none() && engine.input.just_pressed(keys::E) {
            door_target = 1.65;
            door_opened_at = now;
            let dc = Vec3::new((build::DOOR_X0 + build::DOOR_X1) * 0.5, 1.0, 0.0);
            if door_outside {
                sfx.at(&mut engine, "intercom", Vec3::new(build::W / 2.0 + 0.82, 1.37, build::FRONT_Z), 0.45, 12.0);
            }
            sfx.at(&mut engine, "door_creak", dc, 0.5, 15.0);
        }
        if door_target > 0.0 && now - door_opened_at > 4.0 && !in_doorway {
            door_target = 0.0;
        }
        // открывают рукой ~0.7 с, доводчик закрывает медленнее и в конце дожимает
        let rate = if door_target > door_angle { 2.4 } else { 1.4 + 2.0 * (1.0 - door_angle / 1.65).max(0.0) };
        let door_before = door_angle;
        door_angle += (door_target - door_angle).clamp(-rate * dt, rate * dt);
        if door_target == 0.0 && door_before > 0.0 && door_angle <= 0.0 {
            sfx.at(&mut engine, "door_slam", Vec3::new((build::DOOR_X0 + build::DOOR_X1) * 0.5, 1.0, 0.0), 0.75, 30.0);
        }
        // отладка: PODEZD_DOOR=угол° — дверь стоит открытой на этот угол
        if let Some(a) = std::env::var("PODEZD_DOOR").ok().and_then(|v| v.parse::<f32>().ok()) {
            door_angle = a.to_radians();
        }
        // мусор — в контейнер у гаражей
        let [tx0, tz0, tx1, tz1] = build::TRASH;
        let near_trash = fixed_cam.is_none() && has_bag && pos.x > tx0 - 0.3 && pos.x < tx1 + 0.3 && pos.z > tz1 - 0.2 && pos.z < tz1 + 1.6 && tz0 < tz1;
        if near_trash && engine.input.just_pressed(keys::E) {
            has_bag = false;
            sfx.at(&mut engine, "trash", Vec3::new((tx0 + tx1) * 0.5, build::GROUND_Y + 1.0, tz0 + 0.6), 0.6, 15.0);
        }
        // выключатель у входа на площадку
        let floor_n = ((feet + 0.3) / build::H).floor().max(0.0) as usize;
        let sw = Vec3::new(if floor_n == 0 { 0.55 } else { 0.62 }, floor_n as f32 * build::H + 1.44, 0.0);
        let near_switch = fixed_cam.is_none() && !at_home && !door_inside && pos.z >= 0.0 && floor_n < build::FLOORS && (feet - floor_n as f32 * build::H).abs() < 0.3 && Vec3::new(pos.x - sw.x, 0.0, pos.z - sw.z).length() < 0.7;
        if near_switch && ending.is_none() && engine.input.just_pressed(keys::E) {
            // лампа этой площадки — внутри клетки, под её потолком
            let (y0, y1) = (floor_n as f32 * build::H, (floor_n + 1) as f32 * build::H);
            if let Some(l) = lamps.iter_mut().find(|l| l.spec.pos.z > 0.0 && l.spec.pos.z < build::D && l.spec.pos.y > y0 && l.spec.pos.y < y1) {
                l.off = !l.off;
            }
            sfx.at(&mut engine, "switch", sw, 0.5, 10.0);
        }
        // гудение ламп в клетке — по накалу нити; ветер — снаружи в полную
        // силу, в клетке глухо, в открытую дверь — сильнее, ближе к ней — громче
        for (i, l) in lamps.iter().enumerate() {
            if l.spec.bulb_r > 0.0 && l.spec.pos.z > 0.0 && l.spec.pos.z < build::D && l.spec.kind != LampKind::Dead {
                sfx.hum(&mut engine, i, l.spec.pos, l.glow);
            }
        }
        let wind_level = if eye.z < build::FRONT_Z {
            0.22
        } else {
            let near_door = (1.0 - Vec3::new(eye.x - 1.25, eye.y - 1.0, eye.z).length() / 6.0).max(0.0);
            0.025 + 0.16 * (door_angle / 1.65).clamp(0.0, 1.0) * near_door
        };
        sfx.wind(&mut engine, wind_level);
        sfx.ambience(&mut engine, dt, if eye.z < build::FRONT_Z { 1.0 } else { 0.3 * (door_angle / 1.65).clamp(0.0, 1.0) });
        // концовка: свет в глазах гаснет (как зашёл в тёмную прихожую)
        let fade = ending.map(|t0| (1.0 - (now - t0) / 2.5).clamp(0.0, 1.0)).unwrap_or(1.0).max(0.002);
        if let Some(h) = hud.as_mut() {
            h.set_exposure(exposure * fade);
            // белый с поправкой на привыкание глаза к тёплому свету (иначе
            // после адаптации текст выглядит голубым)
            let w = kelvin_rgb(2900.0);
            let wm = w[0].max(w[1]).max(w[2]);
            let white = [w[0] / wm, w[1] / wm, w[2] / wm];
            // ИЗМЕНЕНО (двор): на ярком снегу под фонарём белый в 1x выходил
            // серым (локальная экспозиция тянет яркое вниз) — текст ярче, в
            // тонмаппере он всё равно белый, под ним тёмная подложка
            let line = |h: &hud::Hud, out: &mut Vec<MeshInstance>, text: &str, y: f32, size: f32, a: f32| {
                h.draw(&engine, out, hud::Text { text, x: 0.0, y, size, color: [white[0] * a * 3.0, white[1] * a * 3.0, white[2] * a * 3.0], align: 0.0, backing: 0.7 * a });
            };
            // отладка: PODEZD_HUD=1 — подсказки и при PODEZD_CAM (снимок текста)
            if fixed_cam.is_none() || std::env::var("PODEZD_HUD").is_ok() {
                let intro = 1.0 - ((now - 5.0) / 2.0).clamp(0.0, 1.0);
                if intro > 0.0 && ending.is_none() {
                    line(h, &mut out, "Вынести мусор — и домой: подъезд 2, квартира 14.", -0.62, 0.045, intro);
                }
                if near_trash {
                    line(h, &mut out, "E — выбросить мусор", -0.62, 0.04, 1.0);
                }
                if door_closed && door_inside && ending.is_none() {
                    line(h, &mut out, "E — открыть дверь", -0.62, 0.04, 1.0);
                }
                if door_closed && door_outside && ending.is_none() {
                    line(h, &mut out, "E — приложить ключ к домофону", -0.62, 0.04, 1.0);
                }
                if at_home && ending.is_none() {
                    line(h, &mut out, "E — открыть дверь", -0.62, 0.04, 1.0);
                }
                if near_switch && ending.is_none() {
                    line(h, &mut out, "E — выключатель", -0.62, 0.035, 0.8);
                }
                if let Some(t0) = ending {
                    line(h, &mut out, "Дома.", 0.0, 0.07, ((now - t0) / 1.5).clamp(0.0, 1.0));
                }
            }
        }
        engine.mesh_instances = out;

        // ---- глаз адаптируется к свету вокруг (приближение: прямой свет
        // видимых ламп в точке глаза, перекрытия загораживают) ----
        let key: f32 = lamps
            .iter()
            .filter(|l| {
                let outside = l.spec.pos.z > build::D;
                if outside { through_window(eye, l.spec.pos) } else { !blocked(eye, l.spec.pos) }
            })
            .map(|l| l.intensity() / (l.spec.pos - eye).length_squared().max(0.25))
            .sum::<f32>()
            + 0.004;
        // глаз в темноте раскрывается сильно (зрачок + палочки), но не
        // бесконечно: предел ~ в 8 раз выше, чем под лампой
        let target = match engine.scene_luminance() {
            // по замеру кадра: адаптация неполная (степень 0.85) — тёмное
            // остаётся темнее светлого, как у глаза, а не выравнивается в серое
            Some(lum) => (0.108 / lum.max(1e-4).powf(0.85)).clamp(0.08, 9.0),
            None => (0.35 / key).clamp(0.08, 9.0),
        };
        // к свету глаз привыкает быстро, к темноте — медленно
        let rate = if target < exposure { 2.5 } else { 0.35 };
        exposure *= (target / exposure).powf(1.0 - (-dt * rate).exp());

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let cp = engine.camera.position;
        engine.update(dt, -9.81, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        {
            let tc = &mut engine.transform_constants;
            // ночь, облачно: ни луны, ни солнца; рассеянный свет — засветка неба городом
            tc.light_dir = [0.3, -0.8, 0.5, 0.0];
            tc.light_color = [1.0, 1.0, 1.0, 0.0];
            tc.ambient_color = [0.9, 0.75, 0.6, 0.003];
            tc.fog_scattering = [0.0; 4];
        }
        let settings = engine.light_global_settings.get_or_insert(alkash3d_rs::GlobalLightSettings {
            master_intensity: 1.0,
            shadow_quality: 2,
            shadow_distance: 60.0,
            cascade_count: 3,
            volumetric_enabled: 0,
            volumetric_quality: 0,
            bloom_intensity: 0.04,
            exposure: 1.0,
            gamma: 2.2,
        });
        settings.exposure = exposure * fade;
        // глаз привыкает и к цвету: под лампой накаливания побелка кажется
        // почти белой, но не до конца (неполная адаптация, ~75%)
        let warm = kelvin_rgb(2700.0);
        engine.white_point = [1.0 + (warm[0] - 1.0) * 0.75, 1.0 + (warm[1] - 1.0) * 0.75, 1.0 + (warm[2] - 1.0) * 0.75];
        settings.bloom_intensity = 0.04;
        if let Err(e) = engine.render_frame() {
            eprintln!("[ПОДЪЕЗД] render_frame: {:?}", e);
            break;
        }
        if ending.is_some_and(|t0| now - t0 > 5.0) {
            break;
        }
        // замер (ALKASH3D_PERF=1): где камера раз в секунду — тяжёлый момент
        // игры повторяется через PODEZD_CAM с этими же числами
        if perf_log && now.floor() > (now - dt).floor() {
            let off = lamps.iter().filter(|l| l.off).count();
            println!("[ПОДЪЕЗД] PODEZD_CAM={:.2},{:.2},{:.2},{:.0},{:.0} | выключено ламп {} | экспозиция {:.2}{}", eye.x, eye.y, eye.z, yaw_v.to_degrees(), pitch_v.to_degrees(), off, exposure, if ending.is_some() { " | концовка" } else { "" });
        }
        frames += 1;
        if frame_budget.is_some_and(|b| frames >= b) {
            println!("[ПОДЪЕЗД] PASS: {} кадров, {:.1} FPS, экспозиция {:.2}, замер {:?}", frames, frames as f32 / now.max(0.001), exposure, engine.scene_luminance());
            break;
        }
    }
    Ok(())
}

/// ДОБАВЛЕНО (выход на улицу): ходьба по двору и через дверь — без окна и
/// видеокарты, только `walk()` и геометрия столкновений из build.rs.
#[cfg(test)]
mod walk_tests {
    use super::*;

    /// Идти к цели шагами по 1/60 с со скоростью ходьбы; вернуть, где оказались.
    fn go(mut pos: Vec3, mut feet: f32, to: Vec3, door_open: bool, obstacles: &[[f32; 4]]) -> (Vec3, f32) {
        for _ in 0..60 * 40 {
            let d = Vec3::new(to.x - pos.x, 0.0, to.z - pos.z);
            if d.length() < 0.05 {
                break;
            }
            let step = d.normalize() * (WALK / 60.0).min(d.length());
            let (np, floor) = walk(pos, feet, step, door_open, obstacles);
            pos = np;
            feet += (floor - feet) * 0.5;
        }
        (pos, feet)
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        Vec3::new(a.x - b.x, 0.0, a.z - b.z).length() < 0.15
    }

    #[test]
    fn route_from_start_to_trash_and_into_the_stairwell() {
        let obs = build::outside_obstacles();
        let start = Vec3::new(3.6, 0.0, -5.6);
        let feet = build::outside_floor(start.x, start.z);
        // к контейнерам по проезду
        let trash = Vec3::new(22.5, 0.0, build::TRASH[3] + 0.8);
        let (p, f) = go(start, feet, Vec3::new(22.5, 0.0, -6.0), false, &obs);
        assert!(near(p, Vec3::new(22.5, 0.0, -6.0)), "по проезду не дошёл: {:?}", p);
        let (p, f) = go(p, f, trash, false, &obs);
        assert!(near(p, trash), "к контейнерам не подойти: {:?}", p);
        // обратно к своему подъезду: проезд → дорожка → крыльцо
        let (p, f) = go(p, f, Vec3::new(1.25, 0.0, -6.0), false, &obs);
        let (p, f) = go(p, f, Vec3::new(1.25, 0.0, -2.5), false, &obs);
        assert!(near(p, Vec3::new(1.25, 0.0, -2.5)), "на дорожку к подъезду не выйти: {:?}", p);
        let (p, f) = go(p, f, Vec3::new(1.25, 0.0, -0.8), false, &obs);
        assert!(near(p, Vec3::new(1.25, 0.0, -0.8)) && f.abs() < 0.05, "на крыльцо не подняться: {:?} {}", p, f);
        // закрытая дверь не пускает
        let (shut, _) = go(p, f, Vec3::new(1.25, 0.0, 0.8), false, &obs);
        assert!(shut.z < 0.0, "прошёл сквозь закрытую дверь: {:?}", shut);
        // открытая — пускает, и внутри пол первого этажа
        let (inside, fi) = go(p, f, Vec3::new(1.25, 0.0, 0.8), true, &obs);
        assert!(near(inside, Vec3::new(1.25, 0.0, 0.8)) && fi.abs() < 0.05, "в открытую дверь не войти: {:?} {}", inside, fi);
    }

    #[test]
    fn walls_and_fences_hold() {
        let obs = build::outside_obstacles();
        // фасад мимо проёма не пускает в дом
        let (p, _) = go(Vec3::new(-6.0, 0.0, -2.0), build::GROUND_Y, Vec3::new(-6.0, 0.0, 1.0), true, &obs);
        assert!(p.z <= build::FRONT_Z, "прошёл сквозь фасад: {:?}", p);
        // из клетки в стену рядом с проёмом не выйти даже при открытой двери
        let (p, _) = go(Vec3::new(0.3, 0.0, 0.8), 0.0, Vec3::new(0.3, 0.0, -2.0), true, &obs);
        assert!(p.z > 0.0, "вышел сквозь стену рядом с дверью: {:?}", p);
        // за кирпичную ограду (к гаражам) не пройти
        let (p, _) = go(Vec3::new(-2.0, 0.0, -6.0), build::GROUND_Y, Vec3::new(-2.0, 0.0, -16.0), false, &obs);
        assert!(p.z > -11.8, "прошёл сквозь ограду: {:?}", p);
        // ограда палисадника: с проезда в палисадник не пройти
        let (p, _) = go(Vec3::new(-6.0, 0.0, -5.0), build::GROUND_Y, Vec3::new(-6.0, 0.0, -2.0), false, &obs);
        assert!(p.z < -3.2, "перелез ограду палисадника: {:?}", p);
    }

    #[test]
    fn stairs_still_lead_home() {
        // в клетке ничего не сломалось: с первого этажа по маршу A вверх
        let obs = build::outside_obstacles();
        let (p, f) = go(Vec3::new(1.25, 0.0, 0.8), 0.0, Vec3::new(1.9, 0.0, 0.8), false, &obs);
        let (_, f) = go(p, f, Vec3::new(1.9, 0.0, 4.6), false, &obs);
        assert!((f - build::HALF).abs() < 0.1, "по маршу не поднялся: {}", f);
    }
}
