//! Туалет и коридор из Buckshot Roulette (вырезка до комнаты дилера) как
//! отдельная играбельная сцена: ходьба от первого лица, дверь туалета
//! выбивается, дверь в бэкрум заперта — после попытки её открыть курильщик
//! на мостике заговаривает, а на перилах можно "сброситься".
//!
//! Данные (ВНЕ репозитория, игру нужно купить): сцена `data/buckshot/restroom/
//! restroom.altex` + свет `restroom.alfar` (вырезана из main.tscn скриптом
//! `_export_restroom.gd`, выгружена `_dump_v2.gd` в restroom.bin и
//! переведена `brsc2altex`), звуки и шрифт — из `data/buckshot/scene` (как у
//! порта Buckshot). Пути меняются переменными `RESTROOM_SCENE` и `BUCKSHOT_DATA`.
//!
//! Загрузка сцены, граф, анимации, звук и текст — общие с портом Buckshot
//! (`src/buckshot/*`), обзор мышью — из порта My Summer Car.
//!
//! Управление: мышь — смотреть, WASD — идти, Shift — быстрее, E / ЛКМ —
//! дверь, R — заново, Esc — отпустить мышь / выйти (второе нажатие).
//! `RESTROOM_FRAMES=N` — выйти после N кадров (проверка), `RESTROOM_AUTO=1` —
//! самому дойти до двери и проиграть сценку.
//!
//! После падения и 10 с темноты — пустота (`void.rs`, сцена `void.altex`
//! от `void_scene`, путь — `RESTROOM_VOID`); `RESTROOM_START=void` — сразу туда,
//! `RESTROOM_VOID_POSE=x,y,z,yaw[,pitch]` — где там стоять.

#![allow(dead_code)]

#[path = "../buckshot/anim.rs"]
mod anim;
#[path = "../buckshot/graph.rs"]
mod graph;
#[path = "../buckshot/scene_file.rs"]
mod scene_file;
#[path = "../buckshot/sound.rs"]
mod sound;
#[path = "../buckshot/table.rs"]
mod table;
#[path = "../ponies/official.rs"]
mod official;
#[path = "../buckshot/text.rs"]
mod text;
#[path = "../../../alkash3d-rust/src/bin/msc/mouse.rs"]
mod mouse;

mod blood;
mod collide;
mod items;
mod story;
mod void;

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::{Quat, Vec3};
use alkash3d_rs::{GlobalLightSettings, LightConfig};
use windows::core::Interface;

use anim::Anims;
use collide::{Collider, Tri};
use graph::Graph;
use scene_file::SceneFile;
use sound::Sounds;
use story::Story;
use text::{Font, Rgb, TextLine};

const FIRSTFIRES_DLL_PATH: &str = "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll";

/// Подвижные узлы (остальное сливается в статику).
pub const BATH_DOOR: &str = "intro parent__bathroom door";
pub const BACK_DOOR: &str = "intro parent__backroom door";
pub const DUDE: &str = "smoking dude";
pub const DUDE_ANIM: &str = "smoking dude/AnimationPlayer";
const MUSIC: &str = "res://audio/music/music main_techno techno.ogg";
/// Стена туалета (дверь на x = -136): западнее — музыка глухая.
const RESTROOM_WALL_X: f32 = -136.0;
const PILL_ANIM: &str = "restroom_CLUB/pill bottle main/light_pill flicker/anim_pill flicker";

/// Масштаб сцены: 1 единица ≈ 10 см (дверь — 20.8).
pub const EYE: f32 = 15.5;
pub const RADIUS: f32 = 2.4;
const WALK: f32 = 17.0;
const RUN: f32 = 30.0;
const GRAVITY: f32 = 98.0;
const REACH: f32 = 24.0;
const FOV: f32 = 70.0;
const SPAWN: Vec3 = Vec3::new(-158.0, -6.0, -3.0);
const SPAWN_YAW: f32 = 0.0; // лицом к зеркалу (-Z)

/// Дверь на петле: вращается вокруг вертикали через начало своего узла.
pub struct Door {
    pub node: usize,
    pub hinge: Vec3,
    base: Quat,
    /// Треугольники в мировых координатах при закрытой двери.
    rest: Vec<Tri>,
    pub angle: f32,
    pub target: f32,
    pub speed: f32,
    pub open: bool,
}

impl Door {
    fn new(scene: &SceneFile, g: &Graph, path: &str) -> Door {
        let node = g.node(path);
        let (_, base, hinge) = g.global[node].to_scale_rotation_translation();
        let mut rest = Vec::new();
        collect_tris(scene, g, node, &mut rest, &|_| true);
        Door { node, hinge, base, rest, angle: 0.0, target: 0.0, speed: 6.0, open: false }
    }

    pub fn tris(&self) -> Vec<Tri> {
        let r = Quat::from_rotation_y(self.angle);
        let h = self.hinge;
        self.rest.iter().filter_map(|t| Tri::new(h + r * (t.a - h), h + r * (t.b - h), h + r * (t.c - h))).collect()
    }

    fn update(&mut self, g: &mut Graph, dt: f32) {
        let k = 1.0 - (-self.speed * dt).exp();
        self.angle += (self.target - self.angle) * k;
        g.nodes[self.node].local.rot = Quat::from_rotation_y(self.angle) * self.base;
    }
}

/// Треугольники мешей поддерева `root` в мировых координатах.
fn collect_tris(scene: &SceneFile, g: &Graph, root: usize, out: &mut Vec<Tri>, keep: &dyn Fn(usize) -> bool) {
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        if !keep(i) {
            continue;
        }
        if let Some(m) = scene.nodes[i].mesh {
            if g.vis[i] {
                let xf = g.global[i];
                for s in &scene.meshes[m].surfaces {
                    let p = |k: u32| {
                        let v = s.vertices[k as usize];
                        xf.transform_point3(Vec3::new(v[0], v[1], v[2]))
                    };
                    for f in s.indices.chunks_exact(3) {
                        if let Some(t) = Tri::new(p(f[0]), p(f[1]), p(f[2])) {
                            out.push(t);
                        }
                    }
                }
            }
        }
        for (j, n) in scene.nodes.iter().enumerate() {
            if n.parent == Some(i) {
                stack.push(j);
            }
        }
    }
}

pub struct Player {
    /// Ноги.
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    vel_y: f32,
    pub grounded: bool,
    /// Камера ведётся сценарием, а не игроком.
    pub scripted: bool,
    /// Движение заблокировано (сценарий), обзор мышью — по `look_locked`.
    pub move_locked: bool,
    pub look_locked: bool,
}

pub fn forward(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

impl Player {
    fn new() -> Player {
        Player { pos: SPAWN, yaw: SPAWN_YAW, pitch: 0.0, roll: 0.0, vel_y: 0.0, grounded: false, scripted: false, move_locked: false, look_locked: false }
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::Y * EYE
    }

    fn step(&mut self, eng: &AlkashEngine, col: &Collider, extra: &[Tri], dt: f32) {
        if self.scripted {
            return;
        }
        let mut wish = Vec3::ZERO;
        if !self.move_locked {
            let f = Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos());
            let r = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
            let i = &eng.input;
            if i.is_down(keys::W) {
                wish += f;
            }
            if i.is_down(keys::S) {
                wish -= f;
            }
            if i.is_down(keys::D) {
                wish += r;
            }
            if i.is_down(keys::A) {
                wish -= r;
            }
            let speed = if i.is_down(keys::SHIFT) { RUN } else { WALK };
            wish = wish.normalize_or_zero() * speed;
        }
        self.vel_y -= GRAVITY * dt;
        let mut p = self.pos + wish * dt + Vec3::Y * (self.vel_y * dt);
        // Капсула = три сферы; опора — нормаль, смотрящая вверх.
        self.grounded = false;
        for h in [RADIUS, 8.0, 13.0] {
            let (d, ground) = col.push_sphere(p + Vec3::Y * h, RADIUS, extra);
            if ground.is_some() {
                self.grounded = true;
                // На полу не сползаем по склонам: убираем только вертикаль.
                p += Vec3::new(d.x, d.y.max(0.0), d.z);
            } else {
                p += d;
            }
        }
        if self.grounded && self.vel_y < 0.0 {
            self.vel_y = 0.0;
        }
        self.pos = p;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("BUCKSHOT_DATA").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../data/buckshot/scene").to_string());
    let scene_path = std::env::var("RESTROOM_SCENE").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../data/buckshot/restroom/restroom.altex").to_string());
    let env_f32 = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let light_scale = env_f32("RESTROOM_LIGHT_SCALE", 1.0);
    let exposure = env_f32("RESTROOM_EXPOSURE", 1.0);
    let frame_budget: Option<u32> = std::env::var("RESTROOM_FRAMES").ok().and_then(|v| v.parse().ok());
    let auto = std::env::var("RESTROOM_AUTO").is_ok();
    let void_path = std::env::var("RESTROOM_VOID").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../data/buckshot/void/void.altex").to_string());
    let debug = std::env::var("RESTROOM_DEBUG").is_ok();

    let mut font = Font::load(&format!("{}/font.bin", data_dir))?;
    // Переводы надписей стола (как у порта Buckshot): ключ<TAB>текст.
    let strings: std::collections::HashMap<String, String> = std::fs::read_to_string(format!("{}/strings_ru.tsv", data_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, v)| (k.to_string(), v.replace("\\n", "\n")))
        .collect();
    let mut engine = AlkashEngine::new(1600, 900);
    engine.init()?;
    engine.set_window_title("Risk pony life.");
    let device_ptr = alkash3d_rs::get_device()?.as_raw();
    let config = LightConfig { max_lights: 128, tile_size: 16, far_plane: 300.0, lod_distances: [60.0, 150.0, 300.0], grid_cell_size: 30.0 };
    if let Err(e) = engine.init_lights(FIRSTFIRES_DLL_PATH, device_ptr, config) {
        eprintln!("[RESTROOM] FirstFires не загрузился ({:?}) — без фонарей будет темно.", e);
    }
    if let Err(e) = engine.init_audio() {
        eprintln!("[RESTROOM] звук недоступен: {:?}", e);
    }

    let scene = SceneFile::load(&scene_path)?;
    if debug {
        for (i, n) in scene.nodes.iter().enumerate() {
            let path = scene.path(i);
            if !path.contains(DUDE) || n.mesh.is_none() {
                continue;
            }
            for (si, surf) in scene.meshes[n.mesh.unwrap()].surfaces.iter().enumerate() {
                let m = n.materials.get(si).copied().unwrap_or(-1);
                let mat = (m >= 0).then(|| &scene.materials[m as usize]);
                println!(
                    "[DBG] {} s{} verts={} skin={} skel={:?} mat={} albedo={:?} tex={:?} transp={:?} unshaded={:?} vis={}",
                    path, si, surf.vertices.len(), surf.skin.is_some(), n.skeleton, m,
                    mat.map(|m| m.albedo), mat.map(|m| (m.albedo_tex, m.albedo_tex.max(0) as usize).1).and_then(|t| scene.textures.get(t).map(|x| (t, x.width, x.height))),
                    mat.map(|m| m.transparency), mat.map(|m| m.unshaded), n.visible
                );
            }
        }
    }
    let mut graph = Graph::build(&mut engine, &scene, &[BATH_DOOR, BACK_DOOR, DUDE], light_scale);
    let mut anims = Anims::new(&scene, &graph);
    graph.update_globals();

    // Статическая геометрия для столкновений: всё, кроме подвижных узлов.
    let skip: Vec<usize> = [BATH_DOOR, BACK_DOOR, DUDE].iter().map(|p| graph.node(p)).collect();
    let mut tris = Vec::new();
    collect_tris(&scene, &graph, 0, &mut tris, &|i| !skip.contains(&i));
    let collider = Collider::new(tris);
    println!("[RESTROOM] треугольников для столкновений: {}", collider.len());

    let mut doors = [Door::new(&scene, &graph, BATH_DOOR), Door::new(&scene, &graph, BACK_DOOR)];
    // Дверь туалета распахивается в коридор (+X), выбивается ногой — быстро.
    doors[0].speed = 9.0;
    let lines_path = std::path::Path::new(&scene_path).with_file_name("restroom_lines.txt");
    let mut story = Story::new(&graph, &lines_path);
    drop(scene);
    // Пустота грузится сразу: при переходе не будет паузы на загрузку.
    let mut void = match void::Void::load(&mut engine, &void_path, light_scale) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[RESTROOM] пустота не загрузилась ({}) — после падения останется темнота", e);
            None
        }
    };
    let mut in_void = false;
    if std::env::var("RESTROOM_START").is_ok_and(|v| v == "void") {
        story.enter_void();
    }

    let black_quad = engine.add_mesh(Mesh::from_vertices_and_indices(
        &[
            Vertex::with_normal(-1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(-1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
        ],
        &[0, 1, 2, 0, 2, 3],
    )?);

    engine.set_clear_color(0.0, 0.0, 0.0, 1.0);
    engine.set_time_of_day(0.0);
    engine.set_day_night_speed(0.0);
    engine.camera.near = 0.3;
    engine.camera.far = 1200.0;
    engine.camera.fov = FOV.to_radians();

    let mut sounds = Sounds::new(&data_dir);
    // Музыка клуба: в туалете глухая (через стену), в коридоре чистая.
    sounds.start_music(&mut engine, MUSIC, true, false);
    sounds.play_loop(&mut engine, "lamp", "res://audio/ambience_fluorescent light.ogg", 0.25);
    anims.play(&mut graph, DUDE_ANIM, "idle smoking");
    anims.play(&mut graph, PILL_ANIM, "flicker pill");

    let mut player = Player::new();
    // Отладка: `RESTROOM_POSE=x,y,z,yaw[,pitch]` — стартовая поза игрока.
    let pose: Vec<f32> = std::env::var("RESTROOM_POSE").unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect();
    if pose.len() >= 4 {
        player.pos = Vec3::new(pose[0], pose[1], pose[2]);
        player.yaw = pose[3];
        player.pitch = pose.get(4).copied().unwrap_or(0.0);
    }
    let mut mouse = mouse::Mouse::new();
    if let Err(e) = engine.warm_up_pipelines() {
        eprintln!("[RESTROOM] прогрев PSO: {:?}", e);
    }

    let start = std::time::Instant::now();
    let mut last = 0.0f32;
    let mut frames = 0u32;
    // Отладка: худший кадр за 60 кадров по частям (ходьба, пустота, рендер, всего), мс.
    let mut worst = [0.0f32; 4];
    let mut auto_t = 0.0f32;
    while engine.is_running() {
        engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).min(0.05);
        last = now;

        if engine.input.just_pressed(keys::ESCAPE) {
            if mouse.captured {
                mouse.set_captured(false);
            } else {
                break;
            }
        }
        if !mouse.captured && engine.input.just_pressed(keys::MOUSE_LEFT) {
            mouse.set_captured(true);
        }
        if engine.input.just_pressed(keys::R) {
            graph.reset();
            for d in &mut doors {
                d.angle = 0.0;
                d.target = 0.0;
                d.open = false;
            }
            player = Player::new();
            story = Story::new(&graph, &lines_path);
            if let Some(v) = void.as_mut() {
                v.reset();
            }
            in_void = false;
            anims.play(&mut graph, DUDE_ANIM, "idle smoking");
            sounds.start_music(&mut engine, MUSIC, true, false);
        }

        // Обзор мышью.
        let (dx, dy) = mouse.delta();
        if !player.scripted && !player.look_locked {
            player.yaw += dx * 0.0025;
            player.pitch = (player.pitch - dy * 0.0025).clamp(-1.45, 1.45);
        }
        // Переход в пустоту: игрок стоит на своём краю, лицом к столу.
        if story.in_void() && !in_void {
            if let Some(v) = &void {
                in_void = true;
                player = Player::new();
                player.pos = v.spawn;
                player.yaw = v.spawn_yaw;
                // Отладка: `RESTROOM_VOID_POSE=x,y,z,yaw[,pitch]` — поза в пустоте.
                let pose: Vec<f32> = std::env::var("RESTROOM_VOID_POSE").unwrap_or_default().split(',').filter_map(|v| v.trim().parse().ok()).collect();
                if pose.len() >= 4 {
                    player.pos = Vec3::new(pose[0], pose[1], pose[2]);
                    player.yaw = pose[3];
                    player.pitch = pose.get(4).copied().unwrap_or(0.0);
                }
                // Автопрогон мог оставить клавишу зажатой.
                for key in [keys::W, keys::A, keys::S, keys::D] {
                    engine.input.on_key_up(key);
                }
            }
        }
        if auto && !in_void {
            auto_walk(&mut player, &mut engine, &mut auto_t, dt, &story);
        }

        // Двери — подвижные треугольники.
        let mut extra: Vec<Tri> = Vec::new();
        match void.as_ref().filter(|_| in_void) {
            Some(v) => {
                let t0 = std::time::Instant::now();
                player.step(&engine, &v.collider, &extra, dt);
                worst[0] = worst[0].max(t0.elapsed().as_secs_f32() * 1000.0);
            }
            None => {
                for d in &doors {
                    extra.extend(d.tris());
                }
                player.step(&engine, &collider, &extra, dt);
            }
        }

        // На что смотрим: ближайшая дверь в пределах досягаемости.
        let eye = player.eye();
        let dir = forward(player.yaw, player.pitch);
        let world_hit = collider.ray(eye, dir, REACH, &[]).unwrap_or(f32::MAX);
        let mut look_door = None;
        // Только треугольники самой двери: `collider.ray` с `extra` бьёт ещё
        // и по статике, и любая стена рядом засчитывалась бы как дверь.
        let mut nearest = f32::MAX;
        for (k, d) in doors.iter().enumerate().filter(|_| !in_void) {
            let t = d.tris().iter().filter_map(|tri| tri.ray(eye, dir)).fold(f32::MAX, f32::min);
            if t <= REACH && t <= world_hit + 0.5 && t < nearest && !d.open {
                nearest = t;
                look_door = Some(k);
            }
        }
        let use_pressed = engine.input.just_pressed(keys::E) || (mouse.captured && engine.input.just_pressed(keys::MOUSE_LEFT));
        let use_door = if auto { story.auto_use(&player, look_door) } else { use_pressed.then_some(look_door).flatten() };
        if let Some(v) = void.as_mut().filter(|_| in_void) {
            v.input(&mut engine, &mut sounds, use_pressed, eye, dir, &strings);
            // Пока бланк в руках — ни ходить, ни смотреть по сторонам.
            player.scripted = v.busy();
            // С отладочной позой игрок остаётся, где поставлен (снимки).
            if let Some((eye_at, yaw, pitch)) = v.take_seat().filter(|_| std::env::var("RESTROOM_VOID_POSE").is_err()) {
                player.pos = eye_at - Vec3::Y * EYE;
                player.yaw = yaw;
                player.pitch = pitch;
            }
        }

        // Перила мостика: "[E] Сброситься" (после того, как курильщик заговорил).
        let can_jump = !in_void && story.can_jump(&player, dir);
        let jump = if auto { story.auto_jump() } else { use_pressed && can_jump && look_door.is_none() };
        story.update(dt, use_door, jump, &mut player, &mut doors, &mut graph, &mut anims, &mut sounds, &mut engine, &collider);
        for d in &mut doors {
            d.update(&mut graph, dt);
        }
        anims.update(&mut graph, dt);
        // "idle smoking" может быть не зациклена в файле — перезапускаем.
        if story.dude_idle && !anims.is_playing(DUDE_ANIM) {
            anims.play(&mut graph, DUDE_ANIM, "idle smoking");
        }
        sounds.update_music(&mut engine, dt, player.pos.x < RESTROOM_WALL_X);

        graph.update_globals();
        graph.update_skins(&mut engine);
        graph.sync_lights(&mut engine, !in_void);
        if let Some(v) = void.as_mut() {
            let t0 = std::time::Instant::now();
            v.update(&mut engine, &mut sounds, dt, in_void, &strings);
            worst[1] = worst[1].max(t0.elapsed().as_secs_f32() * 1000.0);
        }

        // Камера.
        let eye = player.eye();
        let f = forward(player.yaw, player.pitch);
        let up0 = Vec3::Y;
        let right = f.cross(up0).normalize_or_zero();
        let up = (Quat::from_axis_angle(f, player.roll) * right.cross(f)).normalize_or_zero();
        let cam = &mut engine.camera;
        cam.position = eye;
        cam.target = eye + f;
        cam.up = if up.length_squared() > 0.5 { up } else { Vec3::Y };
        cam.fov = FOV.to_radians();
        // В пустоте с бланком в руках вид ведёт камера бланка.
        if let Some(c) = void.as_ref().filter(|_| in_void).and_then(|v| v.camera(eye, f, FOV.to_radians())) {
            cam.position = c.pos;
            cam.target = c.pos + c.forward;
            cam.up = c.up;
            cam.fov = c.fov;
        }

        let mut instances = Vec::new();
        match void.as_mut().filter(|_| in_void) {
            Some(v) => {
                let facing = text::facing_camera(&engine);
                v.push_instances(&mut instances, engine.camera.position, facing);
                draw_labels(&v.graph, &strings, &mut font, &mut engine, &mut instances);
            }
            None => graph.push_instances(&mut instances),
        }
        // Подсказки и прицел.
        let mut lines: Vec<(String, f32, f32, f32, Rgb)> = Vec::new();
        if story.show_crosshair(&player) && !(in_void && player.scripted) {
            lines.push(("+".into(), 0.0, -0.012, 0.035, Rgb(0.9, 0.9, 0.85)));
        }
        if can_jump && look_door.is_none() {
            lines.push(("[E] Сброситься".into(), 0.0, -0.12, 0.045, Rgb(1.0, 0.85, 0.4)));
        }
        if let Some(k) = look_door {
            if story.can_use(k) {
                lines.push(("[E] Открыть".into(), 0.0, -0.12, 0.045, Rgb(1.0, 0.85, 0.4)));
            }
        }
        if let Some(t) = story.caption() {
            lines.push((t, 0.0, -0.8, 0.05, Rgb(0.92, 0.92, 0.88)));
        }
        if let Some(v) = void.as_ref().filter(|_| in_void) {
            if let Some(h) = v.hint(eye, forward(player.yaw, player.pitch)) {
                lines.push((h.into(), 0.0, -0.12, 0.045, Rgb(1.0, 0.85, 0.4)));
            }
            if let Some(t) = v.caption() {
                lines.push((t, 0.0, -0.8, 0.05, Rgb(0.92, 0.92, 0.88)));
            }
        }
        if !mouse.captured && !story.ended() {
            lines.push(("Esc — выход, клик — вернуть мышь".into(), 0.0, 0.85, 0.035, Rgb(0.6, 0.6, 0.55)));
        }
        for (t, x, y, size, color) in &lines {
            let line = TextLine { text: t, x: *x, y: *y, size: *size, color: *color, align: 0.0 };
            // Прицел — без подложки, иначе под ним чёрный квадрат.
            if t != "+" {
                font.draw_backing(&engine, &mut instances, &line, black_quad);
            }
            font.draw(&mut engine, &mut instances, &line);
        }
        if story.black() {
            let (fw, _, _) = text::camera_basis(&engine);
            let p = engine.camera.position + fw * (engine.camera.near * 2.0);
            let (rx, ry, rz) = text::facing_camera(&engine);
            // После падения — только чёрный экран и звук удара, без текста.
            instances.push(MeshInstance::new(black_quad).at(p.x, p.y, p.z).rotated(rx, ry, rz).scaled(5.0, 5.0, 5.0));
        }
        engine.mesh_instances = instances;

        engine.light_global_settings = Some(GlobalLightSettings {
            master_intensity: 1.0,
            shadow_quality: 2,
            shadow_distance: 60.0,
            cascade_count: 3,
            volumetric_enabled: 0,
            volumetric_quality: 0,
            bloom_intensity: 0.5,
            exposure: (exposure * story.brightness()).max(0.001),
            gamma: 2.2,
        });
        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let cp = engine.camera.position;
        // Гравитация — для физики вещей в пустоте (1 единица ≈ 10 см).
        engine.update(dt, items::GRAVITY, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        engine.transform_constants.light_color[3] = 0.0;
        engine.transform_constants.ambient_color = [0.0, 0.0, 0.0, 1.0];
        let t0 = std::time::Instant::now();
        let rendered = engine.render_frame();
        worst[2] = worst[2].max(t0.elapsed().as_secs_f32() * 1000.0);
        worst[3] = worst[3].max(dt * 1000.0);
        if let Err(e) = rendered {
            eprintln!("[RESTROOM] render_frame: {:?}", e);
            break;
        }

        frames += 1;
        if debug && frames % 60 == 0 {
            println!("[DBG] худший кадр, мс: ходьба {:.2} пустота {:.2} рендер {:.2} всего {:.2}", worst[0], worst[1], worst[2], worst[3]);
            worst = [0.0; 4];
            println!("[DBG] t={:.1} pos={:.1?} yaw={:.2} grounded={} stage={:?} leg={} look={:?}", now, player.pos, player.yaw, player.grounded, story.stage_name(), story.auto_leg(), look_door);
        }
        if frames == 1 {
            println!("[RESTROOM] первый кадр за {:.1} c", now);
        }
        if frame_budget.is_some_and(|b| frames >= b) {
            println!("[RESTROOM] PASS: {} кадров, {:.1} FPS, стадия {:?}, игрок {:?}", frames, frames as f32 / now.max(0.001), story.stage_name(), player.pos);
            break;
        }
    }
    sounds.stop_all_loops(&mut engine);
    engine.shutdown();
    Ok(())
}

/// Label3D сцены (буквы на машинке, имя на панели здоровья) — как у порта Buckshot.
fn draw_labels(g: &Graph, strings: &std::collections::HashMap<String, String>, font: &mut Font, engine: &mut AlkashEngine, out: &mut Vec<MeshInstance>) {
    for i in g.world_labels() {
        if !g.vis[i] {
            continue;
        }
        let node = &g.nodes[i];
        let Some(l) = node.label.clone() else { continue };
        let tint = node.tint;
        if tint[3] * l.color[3] < 0.05 {
            continue;
        }
        let raw = node.label_text.clone().unwrap_or_else(|| l.text.clone());
        if raw.trim().is_empty() {
            continue;
        }
        let text = strings.get(raw.trim()).cloned().unwrap_or(raw);
        let m = g.global[i];
        let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        let color = Rgb(lin(l.color[0]) * tint[0], lin(l.color[1]) * tint[1], lin(l.color[2]) * tint[2]);
        for (k, line) in text.lines().enumerate() {
            let h = l.font_size * l.pixel_size;
            let shifted = m * alkash3d_rs::math::Mat4::from_translation(Vec3::new(0.0, -(k as f32) * h * 1.2, 0.0));
            font.draw_world(engine, out, line, shifted, h, color, l.align, l.billboard != 0);
        }
    }
}

/// `RESTROOM_AUTO`: дойти до двери туалета, выбить её, пройти мостик к
/// двери в бэкрум — дальше сценарий сам.
fn auto_walk(p: &mut Player, eng: &mut AlkashEngine, t: &mut f32, dt: f32, story: &Story) {
    *t += dt;
    let route = [Vec3::new(-142.0, 0.0, -3.0), Vec3::new(-120.0, 0.0, -2.0), Vec3::new(-44.0, 0.0, 0.0)];
    let k = story.auto_leg();
    for key in [keys::W, keys::A, keys::S, keys::D] {
        eng.input.on_key_up(key);
    }
    if p.scripted || p.move_locked || k >= route.len() {
        return;
    }
    let to = route[k] - Vec3::new(p.pos.x, 0.0, p.pos.z);
    let want = to.x.atan2(-to.z);
    let mut d = want - p.yaw;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    p.yaw += d * (dt * 6.0).min(1.0);
    p.pitch *= 0.9;
    if to.length() > 3.0 {
        eng.input.on_key_down(keys::W);
    }
}
