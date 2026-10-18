//! Персонажи MLP на AlKAsH3D: анимированные 2D-фигуры (риг из частей) на траве.
//!
//! Риги читаются из пакета MLPR (по умолчанию `G:/mlp_characters/engine/
//! rigs.mlpr`, переменная `PONIES_RIG`) — его пишет `export_rig.py`; данные
//! лежат ВНЕ репозитория. Устройство рига и анимации — `rig.rs`. Рисуются
//! без освещения, цвета в пакете закодированы под тонмаппинг движка (`tint`
//! из заголовка) — на экране цвета рисунка один в один. Под ногами — мягкая
//! тень-пятно.
//!
//! Главные (шестёрка и принцессы) стоят передним рядом: дышат, водят
//! головой, машут хвостом. Фоновые гуляют рядами позади: ряд — лента, все в
//! ряду идут в одну сторону с одной скоростью (не сталкиваются), у края ряда
//! переходят на другой конец; смотрят по ходу, ноги шагают.
//!
//! Перед ними — шестёрка "со всех сторон" (`views.rs`, пакет `views.mlps`,
//! переменная `PONIES_VIEWS`): медленно поворачиваются на месте, рисунок
//! выбирается по углу к камере. V — остановить/продолжить поворот.
//!
//! Официальные паппеты DHX (`official.rs`, пакет `official.mlpo`, переменная
//! `PONIES_OFFICIAL`): если пакет есть, показываются они вместо ригов —
//! главные передним рядом, остальные рядами позади, все поворачиваются на
//! месте, рисунок — по углу к камере (16 ракурсов из исходных .fla).
//! `PONIES_OFFICIAL=-` — старые риги.
//!
//! Управление: ПКМ (держать) + мышь — смотреть, WASD — лететь, Пробел/C —
//! вверх/вниз, Shift — быстрее, N — главные шагают на месте / стоят,
//! M — развернуть главных, P — пауза анимации, Esc — выход.
//! Лицо официальных (пакет v2): 1–5 форма глаз, 6 настроение рта, T —
//! болтать, B — брови по кругу, H — румянец, 0 — нейтрально.
//! `PONIES_FRAMES` — самозавершение через N кадров, `PONIES_STILL=1` —
//! фоновые не гуляют, `PONIES_FACE_DEMO=1` — эмоции сами меняются раз в 2 с,
//! `PONIES_ONLY=имя[,имя]` — только эти официальные, `PONIES_BLACK=1` — чёрный фон,
//! `PONIES_PLAY=имя|номер` — играть за пони (управление в play.rs).

mod rig;
mod official;
mod play;
mod views;

use alkash3d_rs::engine::{AlkashEngine, GraphicsSettings, Mesh, MeshInstance};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;

use rig::Rig;

const DEFAULT_RIGS: &str = "G:/mlp_characters/engine/rigs.mlpr";
const DEFAULT_VIEWS: &str = "G:/mlp_characters/engine/views.mlps";
const DEFAULT_OFFICIAL: &str = "G:/mlp_characters/mlp_2d/engine/official.mlpo";
/// Главные в официальном пакете — первые по списку (шестёрка и принцессы).
const OFFICIAL_MAINS: usize = 8;
const GAP: f32 = 0.35;
const ROW_WIDTH: f32 = 14.0;
const ROW_STEP: f32 = 2.2;
const WALK_SPEED: f32 = 0.55;
/// Длина шага (доля роста) — частота шага из скорости, чтобы ноги не скользили.
const STRIDE: f32 = 0.55;
const SENSITIVITY: f32 = 2.2;

/// Круглое мягкое пятно (тень под ногами): чёрное, альфа спадает к краю.
fn blob_texture(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - r).clamp(0.0, 1.0).powf(1.5);
            px[((y * size + x) * 4 + 3) as usize] = (a * 255.0) as u8;
        }
    }
    px
}

struct Placed<'a> {
    rig: &'a Rig,
    home: Vec3,
    /// Гуляет: (направление, левый край ленты, длина ленты).
    walk: Option<(f32, f32, f32)>,
    seed: f32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("PONIES_RIG").unwrap_or_else(|_| DEFAULT_RIGS.to_string());
    let frame_budget: Option<u32> = std::env::var("PONIES_FRAMES").ok().and_then(|s| s.parse().ok());
    let still = std::env::var("PONIES_STILL").is_ok();

    let mut engine = AlkashEngine::new(1600, 900);
    // Цвета закодированы яркостями выше 1 (см. шапку) — bloom принял бы их
    // за источники света и обвёл ореолом. Только до init(): ресурсы bloom
    // остаются, проход пропускается.
    engine.set_graphics_settings(GraphicsSettings { bloom: false, ..GraphicsSettings::default() });
    engine.init()?;

    let official_path = std::env::var("PONIES_OFFICIAL").unwrap_or_else(|_| DEFAULT_OFFICIAL.to_string());
    let (official_tint, puppets) = if official_path == "-" {
        (1.0, Vec::new())
    } else {
        // PONIES_ONLY=имя[,имя] — загрузить только этих.
        let only_env = std::env::var("PONIES_ONLY").ok();
        let only: Option<Vec<&str>> = only_env.as_deref().map(|s| s.split(',').collect());
        match official::read_pack(&mut engine, &official_path, only.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[PONIES] официальные паппеты не загружены: {}", e);
                (1.0, Vec::new())
            }
        }
    };
    for p in &puppets {
        println!("[PONIES] {} — {:.2} x {:.2} м (официальный)", p.name, p.width, p.height);
    }
    let official_mode = !puppets.is_empty();

    let (tint, rigs) = if official_mode { (1.0, Vec::new()) } else { rig::read_pack(&mut engine, &path)? };
    for r in &rigs {
        println!("[PONIES] {} — {:.2} x {:.2} м, частей {}", r.name, r.width, r.height, r.parts.len());
    }
    if rigs.is_empty() && !official_mode {
        return Err(format!("{}: нет персонажей", path).into());
    }
    let views_path = std::env::var("PONIES_VIEWS").unwrap_or_else(|_| DEFAULT_VIEWS.to_string());
    let (views_tint, turnables) = if official_mode {
        (1.0, Vec::new())
    } else {
        match views::read_pack(&mut engine, &views_path) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[PONIES] ракурсы не загружены: {}", e);
                (1.0, Vec::new())
            }
        }
    };
    // Официальные: главные передним рядом, остальные рядами не шире ROW_WIDTH.
    let mut official_spots: Vec<Vec3> = Vec::new();
    let mut official_front = 0.0f32;
    {
        let (mains, others) = puppets.split_at(OFFICIAL_MAINS.min(puppets.len()));
        let mut z = 0.0f32;
        for (gi, group) in [mains, others].into_iter().enumerate() {
            let mut rows: Vec<Vec<&official::Puppet>> = vec![Vec::new()];
            let mut w = 0.0;
            for p in group {
                // Главные — всегда одним рядом, как у ригов.
                if gi > 0 && w + p.width > ROW_WIDTH && !rows.last().unwrap().is_empty() {
                    rows.push(Vec::new());
                    w = 0.0;
                }
                w += p.width + GAP;
                rows.last_mut().unwrap().push(p);
            }
            for row in rows.into_iter().filter(|r| !r.is_empty()) {
                let total: f32 = row.iter().map(|p| p.width).sum::<f32>() + GAP * (row.len() as f32 - 1.0);
                if gi == 0 {
                    official_front = official_front.max(total);
                }
                let mut x = -total * 0.5;
                for p in row {
                    official_spots.push(Vec3::new(x + p.width * 0.5, 0.0, z));
                    x += p.width + GAP;
                }
                z -= ROW_STEP;
            }
        }
    }

    let (gw, gh, grass) = alkash3d_rs::proc_textures::dirt_grass(512);
    let grass_tex = engine.create_texture_rgba(gw, gh, &grass);
    let ground = engine.add_plane_textured(80.0, 80.0, 16.0, [1.0, 1.0, 1.0, 1.0], grass_tex, 0.95, 0.0);
    let blob_tex = engine.create_texture_rgba(64, 64, &blob_texture(64));
    let mut blob = Mesh::plane_textured(1.0, 1.0, 1.0, [0.0, 0.0, 0.0, 1.0]).map_err(|e| format!("{:?}", e))?;
    blob.albedo_srv_index = blob_tex;
    blob.unlit = true;
    blob.blend = true;
    let blob = engine.add_mesh(blob);

    // PONIES_BLACK=1 — чёрный фон, без земли и пятен-теней.
    let black = std::env::var("PONIES_BLACK").is_ok();
    if black {
        engine.set_clear_color(0.0, 0.0, 0.0, 1.0);
    } else {
        engine.set_clear_color(0.45, 0.65, 0.9, 1.0);
    }
    engine.set_day_night_speed(0.0);
    engine.set_time_of_day(13.0);
    engine.camera.near = 0.05;
    engine.camera.fov = 55f32.to_radians();
    if let Err(e) = engine.warm_up_pipelines() {
        eprintln!("[PONIES] прогрев PSO: {:?}", e);
    }
    // PONIES_PLAY=имя|номер — режим игры за пони (см. play.rs).
    if let Ok(who) = std::env::var("PONIES_PLAY") {
        return play::run(&mut engine, &puppets, official_tint, if black { None } else { Some(ground) }, blob, &who, frame_budget);
    }

    // Главные — одним рядом впереди, фоновые — рядами не шире ROW_WIDTH позади.
    let mains: Vec<&Rig> = rigs.iter().filter(|r| r.main).collect();
    let others: Vec<&Rig> = rigs.iter().filter(|r| !r.main).collect();
    let mut placed: Vec<Placed> = Vec::new();
    let mut z = 0.0f32;
    let mut front_width = 0.0f32;
    let mut row_index = 0;
    for (gi, group) in [mains, others].into_iter().enumerate() {
        let mut rows: Vec<Vec<&Rig>> = vec![Vec::new()];
        let mut w = 0.0;
        for r in group {
            if gi > 0 && w + r.width > ROW_WIDTH && !rows.last().unwrap().is_empty() {
                rows.push(Vec::new());
                w = 0.0;
            }
            w += r.width + GAP;
            rows.last_mut().unwrap().push(r);
        }
        for row in rows {
            if row.is_empty() {
                continue;
            }
            let total: f32 = row.iter().map(|r| r.width).sum::<f32>() + GAP * (row.len() as f32 - 1.0);
            if gi == 0 {
                front_width = total;
            }
            // Ряды через один — в разные стороны; лента чуть длиннее ряда,
            // чтобы на стыке был промежуток.
            let walk = (gi > 0 && !still).then(|| (if row_index % 2 == 0 { 1.0 } else { -1.0 }, -total * 0.5 - GAP, total + 2.0 * GAP));
            let mut x = -total * 0.5;
            for r in row {
                placed.push(Placed { rig: r, home: Vec3::new(x + r.width * 0.5, 0.0, z), walk, seed: placed.len() as f32 * 1.7 });
                x += r.width + GAP;
            }
            z -= ROW_STEP;
            row_index += 1;
        }
    }
    // Сверху-спереди: видны и передний ряд, и задние за ним.
    let mut pos = Vec3::new(0.0, 3.2, front_width.max(official_front).max(ROW_WIDTH) * 0.6 + 2.6);
    let mut yaw = std::f32::consts::PI;
    let mut pitch = -0.28f32;
    // Отладка: PONIES_FOCUS=N — камера прямо перед N-м официальным паппетом.
    if let Some(at) = std::env::var("PONIES_FOCUS").ok().and_then(|v| v.parse::<usize>().ok()).and_then(|i| official_spots.get(i)) {
        pos = *at + Vec3::new(0.0, 0.6, 2.2);
        pitch = -0.08;
    }
    let mut last_mouse: Option<[f32; 2]> = None;
    let mut front_walk = std::env::var("PONIES_WALK").is_ok();
    let mut front_right = false;
    let mut paused = false;
    // PONIES_STILL — и официальные паппеты не крутятся (смотрят в камеру).
    let mut spin = !still;
    let mut spin_t = 0.0f32;
    // Ракурсы — рядом перед главными.
    let turn_total: f32 = turnables.iter().map(|t| t.width).sum::<f32>() + GAP * 2.0 * turnables.len().saturating_sub(1) as f32;
    let turn_row: Vec<Vec3> = {
        let mut x = -turn_total * 0.5;
        turnables.iter().map(|t| { let c = x + t.width * 0.5; x += t.width + GAP * 2.0; Vec3::new(c, 0.0, 2.6) }).collect()
    };
    let mut anim_t = 0.0f32;
    let mut face = official::Expression::default();
    let mut talk = false;
    let face_demo = std::env::var("PONIES_FACE_DEMO").is_ok();

    let start = std::time::Instant::now();
    let mut last = 0.0f32;
    let mut frames = 0u32;
    while engine.is_running() {
        engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).clamp(0.0001, 0.1);
        last = now;

        let input = &engine.input;
        if input.just_pressed(keys::ESCAPE) {
            break;
        }
        if input.just_pressed(0x4E) {
            front_walk = !front_walk;
        }
        if input.just_pressed(0x4D) {
            front_right = !front_right;
        }
        if input.just_pressed(0x50) {
            paused = !paused;
        }
        if input.just_pressed(0x56) {
            spin = !spin;
        }
        // Эмоции официальных паппетов (пакет v2): 1–5 глаза, 6 настроение рта,
        // T болтать (липсинк по кругу), B брови по кругу, H румянец, 0 — нейтрально.
        let face_before = face;
        for (k, eyes) in official::Eyes::ALL.iter().enumerate() {
            if input.just_pressed(0x31 + k as u32) {
                face.eyes = *eyes;
            }
        }
        if input.just_pressed(0x36) {
            face.mood = if face.mood == official::Mood::Happy { official::Mood::Sad } else { official::Mood::Happy };
        }
        if input.just_pressed(0x54) {
            talk = !talk;
        }
        if input.just_pressed(0x42) {
            face.brow = match face.brow { None => Some(0), Some(b) if b < 12 => Some(b + 1), _ => None };
        }
        if input.just_pressed(0x48) {
            face.blush = if face.blush.is_some() { None } else { Some(1) };
        }
        if input.just_pressed(0x30) {
            face = official::Expression::default();
            talk = false;
        }
        if face_demo {
            let k = (now / 2.0) as usize;
            face = official::Expression {
                eyes: official::Eyes::ALL[k % 5],
                mood: if k % 2 == 0 { official::Mood::Happy } else { official::Mood::Sad },
                brow: if k % 3 == 0 { None } else { Some((k % 12) as u32) },
                blush: if k % 4 == 1 { Some(1) } else { None },
                ..official::Expression::default()
            };
            talk = k % 2 == 1;
        }
        if face != face_before {
            println!("[PONIES] лицо: {:?}", face);
        }
        if !paused {
            anim_t += dt;
            if spin {
                spin_t += dt;
            }
        }
        // Обзор: ПКМ держать и вести мышь.
        if input.is_down(keys::MOUSE_RIGHT) {
            let m = input.mouse_ndc;
            if let Some(p) = last_mouse {
                yaw -= (m[0] - p[0]) * SENSITIVITY;
                pitch = (pitch + (m[1] - p[1]) * SENSITIVITY * 0.6).clamp(-1.5, 1.5);
            }
            last_mouse = Some(m);
        } else {
            last_mouse = None;
        }
        let forward = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
        let flat = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let right = flat.cross(Vec3::Y).normalize();
        let mut wish = Vec3::ZERO;
        if input.is_down(keys::W) { wish += forward; }
        if input.is_down(keys::S) { wish -= forward; }
        if input.is_down(keys::D) { wish += right; }
        if input.is_down(keys::A) { wish -= right; }
        if input.is_down(keys::SPACE) { wish += Vec3::Y; }
        if input.is_down(keys::C) { wish -= Vec3::Y; }
        let speed = if input.is_down(keys::SHIFT) { 8.0 } else { 2.5 };
        if wish.length_squared() > 0.0 {
            pos += wish.normalize() * speed * dt;
        }
        pos.y = pos.y.max(0.15);
        engine.camera.position = pos;
        engine.camera.target = pos + forward;
        engine.camera.up = Vec3::Y;

        let mut instances = if black { Vec::new() } else { vec![MeshInstance::new(ground)] };
        for pl in &placed {
            let r = pl.rig;
            let mut p = pl.home;
            let step_phase = |t: f32| t * WALK_SPEED / (STRIDE * r.height) * std::f32::consts::TAU + pl.seed;
            let (pose, face_right) = match pl.walk {
                Some((dir, left, len)) => {
                    p.x = left + (pl.home.x - left + dir * WALK_SPEED * anim_t).rem_euclid(len);
                    (r.walk(step_phase(anim_t)), dir > 0.0)
                }
                None if front_walk => (r.walk(step_phase(anim_t)), (r.face > 0) != front_right),
                None => (r.idle(anim_t, pl.seed), (r.face > 0) != front_right),
            };
            // К камере только поворотом вокруг вертикали — фигура стоит прямо.
            let d = pos - p;
            let turn = d.x.atan2(d.z);
            instances.push(MeshInstance::new(blob).at(p.x, 0.01, p.z).scaled(r.width * 0.75, 1.0, r.width * 0.3).tinted(1.0, 1.0, 1.0, 0.45));
            r.draw(&mut instances, &pose, p, turn, face_right, tint);
        }
        for (i, (t, &p)) in turnables.iter().zip(&turn_row).enumerate() {
            let a = spin_t * 0.6 + i as f32 * 1.05;
            let facing = Vec3::new(a.sin(), 0.0, a.cos());
            instances.push(MeshInstance::new(blob).at(p.x, 0.01, p.z).scaled(t.width * 0.6, 1.0, t.width * 0.6).tinted(1.0, 1.0, 1.0, 0.45));
            t.draw(&mut instances, p, facing, pos, views_tint);
        }
        for (i, (p, &at)) in puppets.iter().zip(&official_spots).enumerate() {
            let a = if spin { spin_t * 0.6 + i as f32 * 0.4 } else { 0.0 };
            let d = pos - at;
            // Без поворота — лицом к камере.
            let base = if spin { 0.0 } else { d.x.atan2(d.z) };
            let facing = Vec3::new((a + base).sin(), 0.0, (a + base).cos());
            if !black {
                instances.push(MeshInstance::new(blob).at(at.x, 0.01, at.z).scaled(p.width * 0.6, 1.0, p.width * 0.35).tinted(1.0, 1.0, 1.0, 0.45));
            }
            let seed = i as f32 * 2.399;
            if let Some(mut pose) = p.pose(at, facing, pos, now, seed) {
                // Автоморгание из pose() сохраняется, остальное — из демо.
                pose.expr = official::Expression { blink: pose.expr.blink, ..face };
                // N — шаг на месте (~1.6 шага в секунду).
                if front_walk {
                    pose.walk = Some(anim_t * std::f32::consts::TAU * 1.6 + seed);
                }
                if talk {
                    let k = ((now * 9.0 + seed) as usize) % official::Phoneme::ALL.len();
                    pose.expr.phoneme = official::Phoneme::ALL[k];
                }
                p.draw_pose_ex(&mut engine, &mut instances, &pose, at, official_tint, 1.0);
            }
        }
        engine.mesh_instances = instances;

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let cp = engine.camera.position;
        engine.update(dt, -9.81, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        if let Err(e) = engine.render_frame() {
            eprintln!("[PONIES] render_frame: {:?}", e);
            break;
        }
        frames += 1;
        if frames == 1 {
            println!("[PONIES] первый кадр: {} персонажей, {} инстансов", placed.len(), engine.mesh_instances.len());
        }
        if frame_budget.is_some_and(|b| frames >= b) {
            println!("[PONIES] PASS: {} кадров, {:.1} FPS", frames, frames as f32 / now.max(0.001));
            break;
        }
    }
    engine.shutdown();
    Ok(())
}
