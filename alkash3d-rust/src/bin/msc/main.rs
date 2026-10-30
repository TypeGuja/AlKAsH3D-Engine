//! My Summer Car на AlKAsH3D.
//!
//! Мир Peräjärvi целиком (level2 оригинала: рельеф, дома, дороги, лес,
//! машины, лампы) вытащен из игры дампером `tools/msc/msc_dump.py` в
//! формат MSCW; данные лежат ВНЕ репозитория (`data/msc/scene`, в .gitignore; переменная
//! `MSC_DATA`) — это ассеты игры.
//!
//! Модули порта: `world_file` (чтение MSCW), `world` (дерево узлов, меши,
//! лампы), `ground` (земля и стены: видимая геометрия + коллайдеры Unity),
//! `physics` (предметы — твёрдые тела), `doors` (двери на шарнирах),
//! `assembly` (сборка, болты, двигатель Satsuma), `vehicle` (машины),
//! `sky` (солнце, небо, воздух, экспозиция из одной модели атмосферы),
//! `hud` (экранный текст), `mouse` (обзор мышью).
//!
//! Управление пешком: мышь — смотреть, WASD — идти, Shift — бегом, C —
//! присесть, Пробел — прыжок, F — сесть в машину рядом, N — полёт сквозь
//! стены (Пробел/Ctrl — вверх/вниз).
//! Руки: ЛКМ (держать) — нести предмет, колесо — ближе/дальше, ПКМ —
//! установить деталь (в зоне её места) или бросить; ЛКМ по двери —
//! открыть/закрыть; колесо на болте — затянуть/ослабить; ПКМ по
//! установленной детали с ослабленными болтами — снять; колесо на
//! карбюраторе/трамблёре/коромыслах — регулировка.
//! За рулём: W — газ, S — тормоз / задний ход, A/D — руль, Пробел — ручник,
//! мышь — оглядываться, V — вид из кабины / снаружи, F — выйти;
//! в Satsuma: I — завести (держать — стартер) / заглушить.
//! T — время ×60, H — справка, Esc — отпустить мышь (ещё раз — выход).
//! Переменные: `MSC_TIME` (час старта), `MSC_FRAMES`, `MSC_LIGHT_SCALE`,
//! `MSC_START="x,y,z,yaw,pitch"` (старт в полёте), `MSC_DRIVE=1` (за рулём
//! фургона), `MSC_AUTODRIVE=1` (газ и руль сами — проверка без человека).

mod assembly;
mod doors;
mod ground;
mod hud;
mod mouse;
mod physics;
mod sky;
mod vehicle;
mod world;
mod world_file;

use alkash3d_rs::engine::{AlkashEngine, GraphicsSettings, MeshInstance};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::LightConfig;
use windows::core::Interface;

use hud::{Hud, Text};
use vehicle::{Controls, Spec, Vehicle};
use world::World;
use world_file::WorldFile;

const FIRSTFIRES_DLL_PATH: &str = "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll";
const DEFAULT_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/msc/scene");
/// Двор дома героя (YARD) — у крыльца, в координатах оригинала (-15.5, -0.4, 11.2), Z зеркален.
const SPAWN: [f32; 3] = [-15.5, 2.0, -11.2];
const EYE: f32 = 1.72;
const EYE_CROUCH: f32 = 1.0;
const RADIUS: f32 = 0.3;
const SENSITIVITY: f32 = 0.0022;
/// Сутки в оригинале длятся около получаса реального времени.
const HOURS_PER_SECOND: f32 = 24.0 / 1800.0;

const VK_CONTROL: u32 = 0x11;
const VK_F: u32 = 0x46;
const VK_H: u32 = 0x48;
const VK_I: u32 = 0x49;
/// Досягаемость рук, м.
const REACH: f32 = 2.2;
const VK_N: u32 = 0x4E;
const VK_T: u32 = 0x54;
const VK_V: u32 = 0x56;

/// Машины оригинала. Satsuma в начале разобрана: едет, только когда
/// собрана и заведена (`assembly.rs`).
const VEHICLES: &[(Spec, f32)] = &[
    (
        Spec { root: "SATSUMA", label: "Satsuma", power: 45_000.0, v_max: 42.0, cd_a: 0.75, grip: 0.95, max_steer: 0.6, idle_sound: "idle_sisa4", start_sound: "motor_start_1", gears: &[2.0, 3.6, 5.3, 7.2], redline: 6500.0 },
        557.0,
    ),
    (
        Spec { root: "HAYOSIKO", label: "Hayosiko (фургон)", power: 55_000.0, v_max: 38.0, cd_a: 1.7, grip: 0.9, max_steer: 0.6, idle_sound: "idle_ulko_pako", start_sound: "van_start", gears: &[2.3, 4.1, 6.3, 8.6], redline: 5500.0 },
        1500.0,
    ),
    (
        Spec { root: "GIFU", label: "Gifu (ассенизатор)", power: 110_000.0, v_max: 25.0, cd_a: 6.0, grip: 0.8, max_steer: 0.55, idle_sound: "truck_diesel_idle2", start_sound: "truck_air", gears: &[1.2, 2.2, 3.4, 4.8, 6.5, 9.5], redline: 2600.0 },
        8000.0,
    ),
    (
        Spec { root: "KEKMET", label: "Kekmet (трактор)", power: 35_000.0, v_max: 9.0, cd_a: 3.0, grip: 0.8, max_steer: 0.7, idle_sound: "tractor_idle", start_sound: "tractor_start2", gears: &[0.8, 1.6, 2.6, 3.8], redline: 2400.0 },
        2500.0,
    ),
    (
        Spec { root: "FERNDALE", label: "Ferndale", power: 170_000.0, v_max: 55.0, cd_a: 0.85, grip: 1.0, max_steer: 0.55, idle_sound: "muscle_idle", start_sound: "muscle_start", gears: &[3.2, 5.4, 7.8, 10.5], redline: 5500.0 },
        1630.0,
    ),
];

/// Звуки: клипы, вытащенные из игры в `<данные>/audio/<имя>.wav`.
struct Sound {
    dir: String,
    loaded: std::collections::HashSet<String>,
}

impl Sound {
    fn clip(&mut self, engine: &mut AlkashEngine, name: &str) -> bool {
        if self.loaded.contains(name) {
            return true;
        }
        let Some(audio) = engine.audio.as_mut() else { return false };
        match audio.load_clip(name, &format!("{}/audio/{}.wav", self.dir, name)) {
            Ok(()) => {
                self.loaded.insert(name.to_string());
                true
            }
            Err(e) => {
                eprintln!("[MSC] звук {}: {}", name, e);
                false
            }
        }
    }
    fn play(&mut self, engine: &mut AlkashEngine, name: &str, volume: f32) {
        if self.clip(engine, name) {
            if let Some(a) = engine.audio.as_mut() {
                let _ = a.play_sound_2d(name, volume, false);
            }
        }
    }
    fn looped(&mut self, engine: &mut AlkashEngine, name: &str, volume: f32) -> Option<alkash3d_rs::SoundHandle> {
        if !self.clip(engine, name) {
            return None;
        }
        engine.audio.as_mut().and_then(|a| a.play_sound_2d(name, volume, true).ok())
    }
    fn stop(engine: &mut AlkashEngine, h: Option<alkash3d_rs::SoundHandle>) {
        if let (Some(h), Some(a)) = (h, engine.audio.as_mut()) {
            a.stop_sound(h);
        }
    }
}

fn birds_for(hour: f32) -> &'static str {
    match hour as u32 {
        4..=9 => "birds_morning",
        10..=17 => "birds_day",
        18..=22 => "birds_evening",
        _ => "birds_night",
    }
}

/// Направление взгляда по рысканью/тангажу (yaw = 0 — на -Z).
fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

/// Плавное приближение `v` к `target` со скоростью `rate` (1/с), независимо от FPS.
fn approach(v: Vec3, target: Vec3, rate: f32, dt: f32) -> Vec3 {
    v + (target - v) * (1.0 - (-rate * dt).exp())
}

#[derive(PartialEq)]
enum Mode {
    Walk,
    Fly,
    Drive(usize),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("MSC_DATA").unwrap_or_else(|_| DEFAULT_DATA_DIR.to_string());
    let env_f32 = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let light_scale = env_f32("MSC_LIGHT_SCALE", 1.0);
    let mut hour = env_f32("MSC_TIME", 14.0).rem_euclid(24.0);
    let frame_budget: Option<u32> = std::env::var("MSC_FRAMES").ok().and_then(|v| v.parse().ok());
    let autodrive = std::env::var("MSC_AUTODRIVE").is_ok();
    println!("[MSC] данные: {}", data_dir);

    let mut engine = AlkashEngine::new(1600, 900);
    // Воздух считает своя модель рассеяния (sky.rs) в основном шейдере —
    // экранные "лучи" движка поверх неё дали бы свет дважды.
    engine.set_graphics_settings(GraphicsSettings { volumetric: false, ..GraphicsSettings::default() });
    engine.init()?;
    let device_ptr = alkash3d_rs::get_device()?.as_raw();
    // Карта ~4x4 км: сетка каллинга фонарей накрывает [-2500, 2500]^3.
    let config = LightConfig { max_lights: 512, tile_size: 16, far_plane: 2500.0, lod_distances: [80.0, 200.0, 400.0], grid_cell_size: 125.0 };
    if let Err(e) = engine.init_lights(FIRSTFIRES_DLL_PATH, device_ptr, config) {
        eprintln!("[MSC] FirstFires не загрузился ({:?}) — ламп не будет", e);
    }

    let t0 = std::time::Instant::now();
    let file = WorldFile::load(&format!("{}/world.mscw", data_dir))?;
    let roots: Vec<&str> = file
        .nodes
        .iter()
        .filter(|n| n.parent == Some(0) && VEHICLES.iter().any(|v| n.name.starts_with(v.0.root)))
        .map(|n| n.name.as_str())
        .collect();
    let (mut world, tris) = World::build(&mut engine, &file, &roots);
    drop(roots);
    // Бокс сетки-коллайдера (для формы тел).
    let mesh_box = |m: usize| -> Option<(Vec3, Vec3)> {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for s in file.meshes.get(m)? {
            for v in &s.vertices {
                let p = Vec3::new(v[0], v[1], v[2]);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        (lo.x <= hi.x).then_some((lo, hi))
    };
    let mut physics = physics::Physics::new(&world, mesh_box);
    drop(file);
    let ground = ground::Ground::new(tris);
    let satsuma_root = world.find_root("SATSUMA");
    let mut assembly = assembly::Assembly::new(&mut world, &mut physics, satsuma_root);
    let mut doors = doors::Doors::new(&world, &engine.meshes);
    // MSC_WAKEALL=1 — разбудить все тела (проверка, что ничего не проваливается).
    let wake_all = std::env::var("MSC_WAKEALL").is_ok();
    let start_y: Vec<f32> = physics.bodies.iter().map(|b| b.com.y).collect();
    if wake_all {
        for b in physics.bodies.iter_mut().filter(|b| !b.attached) {
            b.wake();
        }
    }
    // MSC_AUTOBUILD=1 — собрать всё, что подходит (проверка сборки без человека).
    if std::env::var("MSC_AUTOBUILD").is_ok() {
        let n = assembly.auto_build(&mut world, &mut physics);
        println!("[MSC] автосборка: поставлено {}, для запуска не хватает {:?}, для езды {:?}", n, assembly.missing(false), assembly.missing(true));
    }
    println!("[MSC] тел: {}", physics.bodies.len());

    let mut cars: Vec<Vehicle> = Vec::new();
    for &(spec, mass) in VEHICLES {
        let Some(root) = world.find_root(spec.root) else { continue };
        let car = Vehicle::new(&world, &ground, root, spec, mass);
        println!("[MSC] машина {} ({:.0} кг) в {:?}", world.nodes[root].name, car.mass, car.position);
        car.apply(&mut world);
        cars.push(car);
    }
    println!("[MSC] мир загружен за {:.1} c, треугольников земли {}, машин {}", t0.elapsed().as_secs_f32(), ground.triangles(), cars.len());

    let satsuma_car = cars.iter().position(|c| c.spec.root == "SATSUMA");
    let bolt_mesh = engine.add_cylinder_textured(0.009, 0.014, 6, 1.0, [0.55, 0.56, 0.58, 1.0], None, 0.35, 0.9);
    let mut held: Option<usize> = None;
    let mut hold_dist = 1.1f32;
    let mut starter_sound: Option<alkash3d_rs::SoundHandle> = None;
    let mut missing_note = String::new();

    let mut hud = Hud::load(&mut engine, &format!("{}/font.bin", data_dir))?;
    let mut sky = sky::Sky::new(&mut engine);
    let mut mouse = mouse::Mouse::new();
    // Автопрогон (MSC_FRAMES) курсор не захватывает — человеку за компьютером он нужен.
    if frame_budget.is_some() {
        mouse.set_captured(false);
    }

    engine.set_clear_color(0.0, 0.0, 0.0, 1.0);
    // Время ведёт порт (солнце — sky.rs); простое солнце движка стоит.
    engine.set_day_night_speed(0.0);
    engine.camera.near = 0.08;
    engine.camera.far = 6000.0;
    engine.camera.fov = 70f32.to_radians();

    // Геометрии место в видеопамяти (купол неба остаётся: его пишет CPU).
    match engine.meshes_to_video_memory() {
        Ok(bytes) => println!("[MSC] геометрия в видеопамяти: {:.0} МБ", bytes as f64 / 1e6),
        Err(e) => eprintln!("[MSC] геометрия осталась в оперативной памяти: {:?}", e),
    }

    if let Err(e) = engine.warm_up_pipelines() {
        eprintln!("[MSC] прогрев PSO: {:?}", e);
    }
    if let Err(e) = engine.init_audio() {
        eprintln!("[MSC] звук недоступен: {:?}", e);
    }
    let mut sound = Sound { dir: data_dir.clone(), loaded: Default::default() };
    let mut birds: Option<(&'static str, alkash3d_rs::SoundHandle)> = None;
    let mut engine_sound: Option<alkash3d_rs::SoundHandle> = None;

    // ---- игрок ----
    let mut pos = Vec3::from(SPAWN);
    if let Some(h) = ground.height_below(pos + Vec3::Y * 50.0) {
        pos.y = h;
    }
    let mut vel = Vec3::ZERO;
    let mut yaw = 0.0f32;
    let mut pitch = 0.0f32;
    let mut eye_h = EYE;
    let mut grounded = true;
    let mut step_phase = 0.0f32;
    let mut mode = Mode::Walk;
    // За рулём: взгляд относительно машины; вид снаружи.
    let mut look_yaw = 0.0f32;
    let mut look_pitch = -0.08f32;
    let mut outside = false;
    let mut throttle = 0.0f32;
    let mut show_help = true;

    if let Ok(s) = std::env::var("MSC_START") {
        let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if v.len() >= 5 {
            pos = Vec3::new(v[0], v[1], v[2]);
            yaw = v[3].to_radians();
            pitch = v[4].to_radians();
            mode = Mode::Fly;
        }
    }
    if std::env::var("MSC_DRIVE").is_ok() {
        if let Some(ci) = cars.iter().position(|c| c.spec.root == "HAYOSIKO") {
            mode = Mode::Drive(ci);
            engine_sound = sound.looped(&mut engine, cars[ci].spec.idle_sound, 0.5);
        }
    }

    let start = std::time::Instant::now();
    let mut last = 0.0f32;
    let mut frames = 0u32;
    let mut fps_avg = 60.0f32;

    while engine.is_running() {
        engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).clamp(0.0001, 0.1);
        last = now;
        fps_avg += (1.0 / dt - fps_avg) * 0.05;

        // ---- мышь и служебные клавиши ----
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
        if engine.input.just_pressed(VK_H) {
            show_help = !show_help;
        }
        let (mdx, mdy) = mouse.delta();
        let input = &engine.input;
        // MSC_WALKTEST=угол — идти вперёд сам под этим углом (проверка стен без человека).
        let walktest: Option<f32> = std::env::var("MSC_WALKTEST").ok().and_then(|v| v.parse().ok());
        let down = |k: u32| input.is_down(k) || (walktest.is_some() && k == keys::W);
        if let Some(a) = walktest {
            yaw = a.to_radians();
            if frames % 30 == 0 {
                let b = [0.65, 1.4].iter().find_map(|&h| ground.blocker(pos + Vec3::Y * h, RADIUS + 0.05));
                println!("[WALK] t={:.1} pos={:.2},{:.2},{:.2} упор: {:?}", now, pos.x, pos.y, pos.z, b.map(|i| world.path(i as usize)));
            }
        }
        let fast_time = down(VK_T);
        // Состояние кнопок — копией: дальше звуки меняют engine, пока ввод ещё нужен.
        let lmb_down = input.is_down(keys::MOUSE_LEFT) && mouse.captured;
        let lmb = input.just_pressed(keys::MOUSE_LEFT) && mouse.captured;
        let rmb = input.just_pressed(keys::MOUSE_RIGHT);
        let wheel_in = input.wheel;
        let i_down = down(VK_I);
        let i_pressed = input.just_pressed(VK_I);
        let press_f = input.just_pressed(VK_F) || input.just_pressed(keys::E);
        hour = (hour + dt * HOURS_PER_SECOND * if fast_time { 60.0 } else { 1.0 }).rem_euclid(24.0);

        let mut prompt = String::new();
        match mode {
            Mode::Drive(ci) => {
                // ---- за рулём ----
                let want = if down(keys::W) { 1.0 } else { 0.0 };
                // Педаль не "цифровая": нажимается за ~0.25 с, отпускается быстрее.
                throttle += (want - throttle).clamp(-dt * 6.0, dt * 4.0);
                let mut c = Controls {
                    throttle,
                    brake: if down(keys::S) { 1.0 } else { 0.0 },
                    steer: if down(keys::A) { -1.0 } else if down(keys::D) { 1.0 } else { 0.0 },
                    handbrake: down(keys::SPACE),
                };
                if autodrive {
                    c = Controls { throttle: 0.5, brake: 0.0, steer: (now * 0.3).sin() * 0.4, handbrake: false };
                }
                if input.just_pressed(VK_V) {
                    outside = !outside;
                }
                look_yaw = (look_yaw - mdx * SENSITIVITY).clamp(if outside { -10.0 } else { -2.6 }, if outside { 10.0 } else { 2.6 });
                look_pitch = (look_pitch - mdy * SENSITIVITY).clamp(-1.3, 1.2);
                let is_satsuma = Some(ci) == satsuma_car;
                if is_satsuma {
                    // Зажигание: держать I — стартер; заводится, если двигатель
                    // собран, затянут, есть заряд и регулировки не убиты.
                    if i_pressed && assembly.engine_on {
                        assembly.engine_on = false;
                        Sound::stop(&mut engine, engine_sound.take());
                        sound.play(&mut engine, "ignition_keys", 0.6);
                    } else if i_down && !assembly.engine_on {
                        if starter_sound.is_none() {
                            sound.play(&mut engine, "ignition_keys", 0.6);
                            starter_sound = if assembly.battery > 0.05 { sound.looped(&mut engine, "starter_grind", 0.6) } else { None };
                        }
                        assembly.crank += dt;
                        assembly.battery = (assembly.battery - dt * 0.01).max(0.0);
                        let missing = assembly.missing(false);
                        if assembly.crank > 1.2 {
                            if missing.is_empty() && assembly.battery > 0.1 && assembly.engine_quality() > 0.35 {
                                assembly.engine_on = true;
                                Sound::stop(&mut engine, starter_sound.take());
                                sound.play(&mut engine, "motor_start_1", 0.8);
                                engine_sound = sound.looped(&mut engine, "idle_sisa4", 0.5);
                            } else {
                                missing_note = if assembly.battery <= 0.1 {
                                    "аккумулятор сел".to_string()
                                } else if !missing.is_empty() {
                                    format!("не хватает: {}", missing.iter().take(4).copied().collect::<Vec<_>>().join(", "))
                                } else {
                                    "не схватывает — отрегулируй карбюратор, зажигание, клапаны".to_string()
                                };
                            }
                        }
                    } else {
                        assembly.crank = 0.0;
                        Sound::stop(&mut engine, starter_sound.take());
                    }
                    if assembly.engine_on && assembly.slots.iter().any(|s| s.key == "alternator" && s.fixed()) {
                        assembly.battery = (assembly.battery + dt * 0.002).min(1.0);
                    }
                    let drive_missing = assembly.missing(true);
                    cars[ci].power_scale = if assembly.engine_on { assembly.engine_quality() } else { 0.0 };
                    cars[ci].rolling = !drive_missing.iter().any(|k| ["wheel", "subframe", "halfshaft"].contains(k));
                }
                let car = &mut cars[ci];
                car.step(c, &ground, dt);
                car.apply(&mut world);
                let rot = car.rotation();
                if outside {
                    let target = car.position + Vec3::Y * 1.5;
                    let d = look_dir(car.yaw + look_yaw, look_pitch.min(0.2) - 0.25);
                    let mut eye = target - d * 7.0;
                    if let Some(h) = ground.height_below(eye + Vec3::Y * 3.0) {
                        eye.y = eye.y.max(h + 0.4);
                    }
                    engine.camera.position = eye;
                    engine.camera.target = target;
                    engine.camera.up = Vec3::Y;
                } else {
                    // Голова водителя; взгляд в системе машины (качается вместе с кузовом).
                    let eye = car.head(&world);
                    let d = rot * look_dir(look_yaw, look_pitch);
                    engine.camera.position = eye;
                    engine.camera.target = eye + d;
                    engine.camera.up = rot * Vec3::Y;
                }
                prompt = format!("{}   {:.0} км/ч   {} передача   {:.0} об/мин", car.spec.label, car.kmh(), car.gear + 1, car.rpm);
                if is_satsuma {
                    prompt = if assembly.engine_on {
                        format!("{}   АКБ {:.0}%   I — заглушить", prompt, assembly.battery * 100.0)
                    } else if missing_note.is_empty() {
                        format!("Satsuma заглушена   АКБ {:.0}%   I (держать) — завести", assembly.battery * 100.0)
                    } else {
                        format!("Не заводится: {}", missing_note)
                    };
                }
                if let (Some(h), Some(a)) = (engine_sound, engine.audio.as_mut()) {
                    let r = ((car.rpm - 800.0) / (car.spec.redline - 800.0)).clamp(0.0, 1.0);
                    a.set_sound_pitch(h, 0.85 + r * 1.1);
                    a.set_sound_volume(h, 0.35 + r * 0.35 + c.throttle * 0.15);
                }
                if press_f {
                    if car.speed.abs() < 1.0 {
                        let f = car.forward();
                        let left = Vec3::Y.cross(f).normalize();
                        pos = car.position + left * (car.half_width() + 0.3).min(1.6) + f * 0.3;
                        pos.y = ground.height_below(pos + Vec3::Y * 2.0).unwrap_or(pos.y);
                        // Не выходить в стену/дерево: вытолкнуть из крутых поверхностей.
                        pos += ground.push_out(pos + Vec3::Y * 0.9, RADIUS);
                        yaw = car.yaw + look_yaw;
                        pitch = 0.0;
                        vel = Vec3::ZERO;
                        mode = Mode::Walk;
                        if !is_satsuma {
                            Sound::stop(&mut engine, engine_sound.take());
                        }
                        Sound::stop(&mut engine, starter_sound.take());
                        missing_note.clear();
                        sound.play(&mut engine, "car_old_door_close", 0.7);
                    } else {
                        prompt = "Сначала остановись".to_string();
                    }
                }
            }
            Mode::Walk | Mode::Fly => {
                yaw -= mdx * SENSITIVITY;
                pitch = (pitch - mdy * SENSITIVITY).clamp(-1.5, 1.5);
                if input.just_pressed(VK_N) {
                    mode = if mode == Mode::Fly { Mode::Walk } else { Mode::Fly };
                    vel = Vec3::ZERO;
                }
                let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
                let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
                let mut wish = Vec3::ZERO;
                if down(keys::W) { wish += forward; }
                if down(keys::S) { wish -= forward; }
                if down(keys::D) { wish += right; }
                if down(keys::A) { wish -= right; }
                let wish = wish.normalize_or_zero();

                if mode == Mode::Fly {
                    let speed = if down(keys::SHIFT) { 60.0 } else { 12.0 };
                    let mut v = look_dir(yaw, pitch) * (wish.dot(forward)) + right * wish.dot(right);
                    if down(keys::SPACE) { v += Vec3::Y; }
                    if down(VK_CONTROL) { v -= Vec3::Y; }
                    vel = approach(vel, v * speed, 8.0, dt);
                    pos += vel * dt;
                    eye_h = 0.0;
                } else {
                    let crouch = down(keys::C);
                    let speed = if crouch { 1.4 } else if down(keys::SHIFT) { 6.0 } else { 2.8 };
                    // Разгон/торможение ногами — за доли секунды, в воздухе — почти никак.
                    let target = wish * speed;
                    let horiz = approach(Vec3::new(vel.x, 0.0, vel.z), target, if grounded { 12.0 } else { 0.8 }, dt);
                    vel.x = horiz.x;
                    vel.z = horiz.z;
                    if grounded && input.just_pressed(keys::SPACE) && !crouch {
                        vel.y = 4.2;
                        grounded = false;
                    }
                    vel.y -= 9.81 * dt;
                    let mut next = pos + vel * dt;
                    // Стены: сфера на уровне колен и груди (ступеньки до 0.45 м — не стена).
                    let top = if crouch { 0.8 } else { 1.4 };
                    for h in [0.65, top] {
                        next += ground.push_out(next + Vec3::Y * h, RADIUS);
                        let c = next + Vec3::Y * h;
                        next += physics.push_player(&world, c, RADIUS, vel);
                        next += doors.push_player(&world, c, RADIUS);
                        for car in &cars {
                            next += car.push_player(next + Vec3::Y * h, RADIUS);
                        }
                    }
                    // Пол: ступенька вверх до 0.45 м; при ходьбе вниз — прилипаем до 0.35 м.
                    // Пол — и земля, и верх предметов (ящики, детали).
                    let probe = next + Vec3::Y * 0.45;
                    let floor = match (ground.height_below(probe), physics.floor_below(&world, probe, RADIUS)) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        (a, b) => a.or(b),
                    };
                    match floor {
                        Some(h) if next.y <= h || (grounded && vel.y <= 0.0 && next.y - h < 0.35) => {
                            next.y = h;
                            vel.y = 0.0;
                            grounded = true;
                        }
                        _ => grounded = false,
                    }
                    pos = next;
                    if pos.y < -100.0 {
                        pos = Vec3::from(SPAWN);
                        vel = Vec3::ZERO;
                    }
                    let target_eye = if crouch { EYE_CROUCH } else { EYE };
                    eye_h += (target_eye - eye_h) * (1.0 - (-10.0 * dt).exp());
                    // Шаги: по пройденному пути.
                    let moved = Vec3::new(vel.x, 0.0, vel.z).length();
                    if grounded && moved > 0.5 {
                        step_phase += moved * dt;
                        let stride = if moved > 4.0 { 1.6 } else { 1.3 };
                        if step_phase > stride {
                            step_phase -= stride;
                            let clip = if frames % 2 == 0 { "footstep_forest1" } else { "footstep_forest2" };
                            sound.play(&mut engine, clip, 0.25 + moved * 0.03);
                        }
                    }
                    // Машина рядом — сесть.
                    if let Some(ci) = (0..cars.len()).min_by(|&a, &b| (cars[a].position - pos).length().total_cmp(&(cars[b].position - pos).length())) {
                        if (cars[ci].position - pos).length() < 3.5 {
                            prompt = format!("F — сесть в {}", cars[ci].spec.label);
                            if press_f {
                                mode = Mode::Drive(ci);
                                look_yaw = 0.0;
                                look_pitch = -0.08;
                                throttle = 0.0;
                                if let Some(bi) = held.take() {
                                    physics.bodies[bi].held = false;
                                }
                                sound.play(&mut engine, "car_old_door_open", 0.7);
                                // Satsuma заводят ключом (I), остальные — сразу.
                                if Some(ci) != satsuma_car {
                                    sound.play(&mut engine, cars[ci].spec.start_sound, 0.8);
                                    engine_sound = sound.looped(&mut engine, cars[ci].spec.idle_sound, 0.5);
                                }
                            }
                        }
                    }
                }
                let eye = pos + Vec3::Y * eye_h;
                engine.camera.position = eye;
                engine.camera.target = eye + look_dir(yaw, pitch);
                engine.camera.up = Vec3::Y;

                // ---- руки: предметы, двери, болты, детали ----
                if mode == Mode::Walk {
                    let d = look_dir(yaw, pitch);
                    let wheel = wheel_in;
                    let wall = ground.raycast(eye, d, REACH).unwrap_or(REACH) + 0.02;
                    if let Some(bi) = held {
                        if !lmb_down {
                            physics.bodies[bi].held = false;
                            held = None;
                        } else {
                            hold_dist = (hold_dist + wheel * 0.1).clamp(0.5, 2.0);
                            let slot = assembly.slot_for(&world, &physics, bi);
                            let name = physics.bodies[bi].name.replace("(Clone)", "");
                            prompt = match slot {
                                Some(_) => format!("{} — ПКМ: установить", name),
                                None => format!("{} ({:.0} кг) — ПКМ: бросить, колесо: ближе/дальше", name, physics.bodies[bi].mass),
                            };
                            if rmb {
                                held = None;
                                match slot {
                                    Some(si) => {
                                        assembly.install(&mut world, &mut physics, bi, si);
                                        sound.play(&mut engine, "bolt_screw", 0.5);
                                    }
                                    None => {
                                        let b = &mut physics.bodies[bi];
                                        b.held = false;
                                        b.vel += d * (60.0 / b.mass.max(1.0)).min(9.0);
                                    }
                                }
                            }
                        }
                    } else {
                        let bolt = assembly.pick_bolt(&world, eye, d, wall.min(1.8));
                        let part = assembly.pick_installed(&physics, eye, d, wall);
                        let body = physics.pick(&world, eye, d, wall);
                        let door = doors.pick(&world, eye, d, wall);
                        let far = f32::MAX;
                        let t_part = part.map(|p| p.1).unwrap_or(far);
                        let t_body = body.map(|p| p.1).unwrap_or(far);
                        let t_door = door.map(|p| p.1).unwrap_or(far);
                        if let Some((si, k, _)) = bolt {
                            let s = &mut assembly.slots[si];
                            let t = &mut s.tight[k];
                            if wheel != 0.0 {
                                let before = *t;
                                *t = (*t as i32 + wheel.signum() as i32).clamp(0, assembly::BOLT_STEPS as i32) as u8;
                                if *t != before {
                                    sound.play(&mut engine, "bolt_screw", 0.4);
                                }
                            }
                            let done = s.tight.iter().filter(|&&x| x >= assembly::BOLT_STEPS).count();
                            prompt = format!("Болт {}/{}   (колесо: затянуть/ослабить)   затянуто болтов {}/{}", s.tight[k], assembly::BOLT_STEPS, done, s.bolts.len());
                        } else if t_door < t_part.min(t_body) {
                            let (k, _) = door.unwrap();
                            prompt = "ЛКМ — открыть/закрыть".to_string();
                            if lmb {
                                doors.toggle(k);
                                let open = doors.list[k].open;
                                let car_door = world.path(doors.list[k].node).contains("Doors/door");
                                let clip = match (open, car_door) {
                                    (true, true) => "car_old_door_open",
                                    (false, true) => "car_old_door_close",
                                    (true, false) => "house_door_open",
                                    (false, false) => "house_door_close2",
                                };
                                sound.play(&mut engine, clip, 0.6);
                            }
                        } else if t_part < t_body {
                            let (si, _) = part.unwrap();
                            let key = assembly.slots[si].key.clone();
                            let bi = assembly.slots[si].installed.unwrap();
                            let name = physics.bodies[bi].name.replace("(Clone)", "");
                            if let Some(msg) = (wheel != 0.0).then(|| assembly.adjust(&key, wheel)).flatten() {
                                prompt = format!("{} — {}", name, msg);
                            } else {
                                let s = &assembly.slots[si];
                                let done = s.tight.iter().filter(|&&x| x >= assembly::BOLT_STEPS).count();
                                prompt = if s.loose() {
                                    format!("{} — ПКМ: снять", name)
                                } else {
                                    format!("{} — болты {}/{} (ослабь все, чтобы снять)", name, done, s.bolts.len())
                                };
                                if ["carburator", "distributor", "rockershaft"].iter().any(|k| key.starts_with(k)) {
                                    prompt.push_str("   колесо: регулировка");
                                }
                                if rmb && s.loose() {
                                    if let Some(bi) = assembly.remove(&mut world, &mut physics, si) {
                                        physics.bodies[bi].held = true;
                                        held = Some(bi);
                                        hold_dist = 1.0;
                                    } else {
                                        prompt = format!("{} — сначала сними то, что на ней стоит", name);
                                    }
                                }
                            }
                        } else if let Some((bi, t)) = body {
                            let b = &physics.bodies[bi];
                            prompt = format!("{} — ЛКМ: взять", b.name.replace("(Clone)", ""));
                            if lmb {
                                let b = &mut physics.bodies[bi];
                                b.held = true;
                                b.wake();
                                held = Some(bi);
                                hold_dist = t.clamp(0.7, 1.5);
                            }
                        }
                    }
                }
            }
        }

        // ---- физика мира: двери, предметы, установленные детали ----
        doors.update(&mut world, dt);
        let hold_target = held.map(|bi| (bi, engine.camera.position + (engine.camera.target - engine.camera.position).normalize() * hold_dist));
        physics.step(&ground, dt, hold_target, &world.visible);
        physics.apply(&mut world);
        assembly.follow(&mut world, &mut physics);

        // ---- небо, звук окружения ----
        sky.update(hour);
        hud.set_exposure(sky.exposure);
        let want = birds_for(hour);
        if birds.map(|b| b.0) != Some(want) {
            Sound::stop(&mut engine, birds.take().map(|b| b.1));
            birds = sound.looped(&mut engine, want, 0.3).map(|h| (want, h));
        }

        world.sync_lights(&mut engine, light_scale);

        let mut instances = Vec::new();
        world.push_instances(&mut instances);
        // Болты установленных деталей (в сцене оригинала у них нет своей модели).
        let cam = engine.camera.position;
        for s in &assembly.slots {
            if s.installed.is_none() {
                continue;
            }
            for (k, &b) in s.bolts.iter().enumerate() {
                let g = world.global[b];
                let p = g.w_axis.truncate();
                if (p - cam).length() > 8.0 || !world.visible[s.target] && s.mode == assembly::Mode::Copy {
                    continue;
                }
                let (_, r, _) = g.to_scale_rotation_translation();
                let (rz, ry, rx) = r.to_euler(alkash3d_rs::math::EulerRot::ZYX);
                // Незатянутый болт торчит и темнее (без смазки/краски).
                let t = s.tight[k] as f32 / assembly::BOLT_STEPS as f32;
                let shade = 0.55 + 0.45 * t;
                instances.push(MeshInstance::new(bolt_mesh).at(p.x, p.y, p.z).rotated(rx, ry, rz).tinted(shade, shade, shade, 1.0));
            }
        }
        sky.push(&mut engine, &mut instances);

        // ---- интерфейс ----
        let white = [0.95, 0.95, 0.9];
        let status = format!("{:02}:{:02}   {:.0} FPS{}", hour as u32, (hour.fract() * 60.0) as u32, fps_avg, if mode == Mode::Fly { "   ПОЛЁТ (N)" } else { "" });
        hud.draw(&engine, &mut instances, Text { text: &status, x: -0.97, y: 0.92, size: 0.035, color: white, align: -1.0, backing: 0.35 });
        if matches!(mode, Mode::Walk) || (matches!(mode, Mode::Drive(_)) && !outside) {
            hud.draw(&engine, &mut instances, Text { text: "+", x: 0.0, y: -0.015, size: 0.03, color: [0.9, 0.9, 0.9], align: 0.0, backing: 0.0 });
        }
        if !prompt.is_empty() {
            hud.draw(&engine, &mut instances, Text { text: &prompt, x: 0.0, y: -0.8, size: 0.04, color: [1.0, 0.85, 0.4], align: 0.0, backing: 0.45 });
        }
        if !mouse.captured {
            hud.draw(&engine, &mut instances, Text { text: "ПАУЗА — клик мышью: продолжить, Esc: выход", x: 0.0, y: 0.1, size: 0.05, color: white, align: 0.0, backing: 0.6 });
        }
        if show_help {
            let lines: &[&str] = if matches!(mode, Mode::Drive(_)) {
                &["W — газ   S — тормоз / назад   A/D — руль", "Пробел — ручник   V — вид снаружи   F — выйти", "I — зажигание (Satsuma)   мышь — оглядеться   H — скрыть"]
            } else {
                &["мышь — смотреть   WASD — идти   Shift — бегом", "C — присесть   Пробел — прыжок   F — сесть в машину", "ЛКМ — взять/дверь   ПКМ — установить/снять/бросить", "колесо — болты и регулировки   N — полёт   H — скрыть"]
            };
            for (k, l) in lines.iter().enumerate() {
                hud.draw(&engine, &mut instances, Text { text: l, x: 0.97, y: 0.92 - k as f32 * 0.055, size: 0.03, color: [0.85, 0.85, 0.8], align: 1.0, backing: 0.35 });
            }
        }
        engine.mesh_instances = instances;

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let cp = engine.camera.position;
        engine.update(dt, -9.81, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        sky.apply(&mut engine, dt);
        if let Err(e) = engine.render_frame() {
            eprintln!("[MSC] render_frame: {:?}", e);
            break;
        }

        frames += 1;
        if frames == 1 {
            println!("[MSC] первый кадр, позиция {:?}, солнце {:?} (I {:?}), небо {:?}", pos, sky.sun_dir, sky.sun, sky.ambient);
        }
        if wake_all && frame_budget.is_some_and(|b| frames >= b) {
            let vis: Vec<&physics::Body> = physics.bodies.iter().filter(|b| !b.attached && world.visible[b.node]).collect();
            let fell: Vec<String> = physics.bodies.iter().enumerate().filter(|(i, b)| !b.attached && world.visible[b.node] && b.com.y < start_y[*i] - 1.0).map(|(i, b)| format!("{} ({:.1} м)", b.name, start_y[i] - b.com.y)).collect();
            println!("[MSC] физика: тел {}, не спят {}, провалились {}: {:?}", vis.len(), vis.iter().filter(|b| b.awake).count(), fell.len(), fell.iter().take(12).collect::<Vec<_>>());
        }
        if frame_budget.is_some_and(|b| frames >= b) {
            println!("[MSC] PASS: {} кадров, {:.1} FPS, экспозиция {:.2}", frames, frames as f32 / now.max(0.001), sky.exposure);
            break;
        }
    }

    drop(mouse);
    engine.shutdown();
    Ok(())
}
