// src/bin/main_car.rs
//! Alkash3D Engine — "гараж-симулятор" в духе My Summer Car (СВОЯ игра,
//! не копия её кода/ассетов): грунтовый двор с текстурами + гараж-зона
//! (стены буквой "П" + крыша) + одна РЕАЛЬНО УПРАВЛЯЕМАЯ машина, в которую
//! можно сесть (E рядом с ней) и поехать (WASD = газ/тормоз/руль, R —
//! ручник), плюс несколько бочек, физически падающих и катающихся по двору
//! через Inertial (показывает ту же физику падения/вращения, что и в
//! задачах #39-#40, только теперь не на самой машине — см. ниже).
//!
//! ВАЖНОЕ АРХИТЕКТУРНОЕ РЕШЕНИЕ (после задач #39/#40 — машина падала и
//! вращалась через Inertial, но НЕ ехала): реальное вождение реализовано
//! СВОЕЙ аркадной симуляцией (`alkash3d_rs::car_sim`), а не через
//! `PhysicsAPI`/`inertial.dll`. Причина подробно объяснена в шапке
//! `car_sim.rs` — коротко: у Inertial все тела физически ведут себя как
//! сферы ОДНОГО фиксированного радиуса (кузов машины физически был бы
//! шаром ~0.5м, заметно меньше видимого кузова), и у `PhysicsAPI` в принципе
//! нет способа управлять уже созданным телом (`add_body`/`get_body`/
//! `remove_body`, никакого `apply_force`/`set_velocity`) — честное
//! вождение потребовало бы новой FFI-функции и пересборки Fortran-плагина
//! ради физики, которая всё равно осталась бы "машина-шар". Инерциальная
//! физика по-прежнему честно используется в сцене — просто для бочек, а не
//! для машины.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::scene::EntityId;
use alkash3d_rs::car_sim::{CarInput, CarParams, CarState, WallAabb};
use alkash3d_rs::{proc_textures, PhysicsBody, PhysicsConfig};
use std::f32::consts::FRAC_PI_2;
use std::time::Instant;

const WINDOW_WIDTH: u32 = 1366;
const WINDOW_HEIGHT: u32 = 768;
const EYE_HEIGHT: f32 = 1.6;

// Половина размера площадки (метров от центра) — площадка теперь ОДНА
// большая текстурированная плоскость (см. `setup_ground`), а не сетка
// кубов-плиток, как раньше (441 плитка × 4 draw call/кадр — см.
// исторический комментарий про TDR-краш от похожей проблемы, полностью
// снятый переходом на один меш).
const WORLD_HALF: f32 = 18.0;
const GROUND_Y: f32 = 0.0;

const GARAGE_CENTER_X: f32 = 12.0;
const GARAGE_CENTER_Z: f32 = 0.0;
const GARAGE_WIDTH: f32 = 8.0; // по Z (ширина проёма)
const GARAGE_DEPTH: f32 = 6.0; // по X (глубина гаража)
const GARAGE_WALL_HEIGHT: f32 = 3.0;
const GARAGE_WALL_THICKNESS: f32 = 0.3;
const GARAGE_ROOF_THICKNESS: f32 = 0.15;
const GARAGE_ROOF_OVERHANG: f32 = 0.4;

const CAR_START_X: f32 = 0.0;
const CAR_START_Z: f32 = -1.0;
/// Машина стартует лицом к гаражу (гараж стоит по +X от площадки, проём —
/// со стороны -X гаража, то есть смотрит на площадку) — `forward =
/// (sin(yaw), 0, cos(yaw))` (см. `CarState::forward`), при `yaw = FRAC_PI_2`
/// это `(1, 0, 0)` = +X, ровно в сторону гаража.
const CAR_START_YAW: f32 = FRAC_PI_2;

/// Дистанция (метры, по XZ) от игрока до центра машины, ближе которой
/// разрешено сесть по E.
const ENTER_CAR_DISTANCE: f32 = 2.6;

const INERTIAL_DLL_PATH: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";

/// Текущий режим игрока — определяет, что делают WASD/стрелки и откуда
/// берётся позиция камеры в `run_loop`.
#[derive(PartialEq, Eq, Clone, Copy)]
enum PlayerMode {
    Walking,
    Driving,
}

/// Все размеры кузова/колёс машины в одном месте — используются и при
/// постройке геометрии (`spawn_player_car`), и при расчёте позиций колёс/
/// клиренса, поэтому нельзя развести константы по разным функциям, не
/// рискуя их рассинхронизировать.
#[derive(Clone, Copy)]
struct CarDimensions {
    chassis_half: [f32; 3],
    cabin_half: [f32; 3],
    wheel_radius: f32,
    wheel_width: f32,
    wheel_x: f32,
    wheel_y_local: f32,
    wheel_z: f32,
    /// Y корневой сущности машины над `GROUND_Y`, при котором низ колеса
    /// ровно касается земли — считается ИЗ остальных полей (см.
    /// `CarDimensions::new`), а не подбирается на глаз.
    root_y: f32,
}

impl CarDimensions {
    fn new() -> Self {
        let chassis_half = [0.85, 0.50, 1.9];
        let cabin_half = [0.72, 0.35, 0.95];
        let wheel_radius = 0.33;
        let wheel_width = 0.28;
        let wheel_x = chassis_half[0] + wheel_width * 0.35;
        let wheel_y_local = -chassis_half[1] + wheel_radius * 0.25;
        let wheel_z = chassis_half[2] * 0.62;
        // Хотим: root_y + wheel_y_local - wheel_radius == GROUND_Y.
        let root_y = GROUND_Y - wheel_y_local + wheel_radius;
        Self {
            chassis_half,
            cabin_half,
            wheel_radius,
            wheel_width,
            wheel_x,
            wheel_y_local,
            wheel_z,
            root_y,
        }
    }
}

/// FL/FR/RL/RR — порядок, в котором заведены колёса машины (совпадает с
/// порядком в `spawn_player_car`), используется, чтобы знать, каким
/// колёсам применять угол руля (передним), а каким — только качение.
struct PlayerCar {
    entity: EntityId,
    wheel_entities: [EntityId; 4],
    dims: CarDimensions,
    state: CarState,
    params: CarParams,
}

impl PlayerCar {
    fn distance_xz(&self, pos: Vec3) -> f32 {
        let dx = self.state.position.x - pos.x;
        let dz = self.state.position.z - pos.z;
        (dx * dx + dz * dz).sqrt()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    alkash3d_rs::console_log::init_console_log_to_file("engine_car_log.txt");

    println!("==========================================");
    println!("Alkash3D Engine v{} — Car Demo", alkash3d_rs::VERSION);
    println!("==========================================");
    println!();
    println!("🎮 УПРАВЛЕНИЕ:");
    println!("  WASD    - ходьба пешком / газ+тормоз+руль за рулём");
    println!("  Стрелки - осмотреться пешком");
    println!("  E       - сесть в машину (рядом с ней) / выйти из машины");
    println!("  R       - ручной тормоз (за рулём)");
    println!("  SHIFT   - ускорение ходьбы (x2)");
    println!("  ESC     - выход");
    println!("==========================================");

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);

    if let Err(e) = engine.init() {
        eprintln!("[MAIN_CAR] Failed to initialize engine: {:?}", e);
        return Err(e.into());
    }

    setup_ground(&mut engine);
    let garage_walls = setup_garage(&mut engine);

    // Физика (Inertial) — теперь нужна ТОЛЬКО бочкам во дворе (см. шапку
    // файла про то, почему сама машина больше не физическое тело). Можно
    // отключить для диагностики: `ALKASH_NO_PHYSICS=1`.
    if std::env::var("ALKASH_NO_PHYSICS").is_err() {
        setup_barrel_physics(&mut engine);
    } else {
        println!("[MAIN_CAR] ALKASH_NO_PHYSICS=1 — физика бочек отключена (диагностика)");
    }

    let car = spawn_player_car(&mut engine, Vec3::new(CAR_START_X, 0.0, CAR_START_Z), CAR_START_YAW);
    setup_yard_props(&mut engine, &car);

    let world_bounds = WallAabb {
        min_x: -WORLD_HALF + 1.5,
        max_x: WORLD_HALF - 1.5,
        min_z: -WORLD_HALF + 1.5,
        max_z: WORLD_HALF - 1.5,
    };

    run_loop(&mut engine, car, garage_walls, world_bounds);

    engine.shutdown();
    println!("[MAIN_CAR] Goodbye!");
    Ok(())
}

/// Земля двора — одна большая плоскость (грязь/трава, процедурная текстура
/// `proc_textures::dirt_grass`) + гравийная "подъездная площадка" перед
/// гаражом чуть выше (Y=0.01, чтобы не z-fight'ить с основной землёй)
/// текстурой `proc_textures::gravel` — чисто визуальное разнообразие,
/// коллизии у обеих плоскостей нет (обходится через `car_sim::WallAabb`
/// стен/границ, не через геометрию земли).
fn setup_ground(engine: &mut AlkashEngine) {
    println!("\n[MAIN_CAR] Setting up ground...");

    let (gw, gh, gpix) = proc_textures::dirt_grass(256);
    let ground_srv = engine.create_texture_rgba(gw, gh, &gpix);
    let ground_mesh = engine.add_plane_textured(
        WORLD_HALF * 2.0,
        WORLD_HALF * 2.0,
        0.35,
        [1.0, 1.0, 1.0, 1.0],
        ground_srv,
        0.95,
        0.0,
    );
    engine.spawn_static_mesh(ground_mesh, [0.0, GROUND_Y, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);

    let (yw, yh, ypix) = proc_textures::gravel(256);
    let yard_srv = engine.create_texture_rgba(yw, yh, &ypix);
    let driveway_width = GARAGE_WIDTH * 1.1;
    let driveway_depth = 9.0;
    let yard_mesh = engine.add_plane_textured(driveway_depth, driveway_width, 0.6, [1.0, 1.0, 1.0, 1.0], yard_srv, 0.9, 0.0);
    engine.spawn_static_mesh(
        yard_mesh,
        [GARAGE_CENTER_X - GARAGE_DEPTH * 0.5 - driveway_depth * 0.5 + 1.0, GROUND_Y + 0.01, GARAGE_CENTER_Z],
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    );

    engine.set_clear_color(0.55, 0.7, 0.9, 1.0);
    engine.set_time_of_day(13.0);

    println!("✅ Ground ready: 1 плоскость двора + 1 гравийная площадка (вместо сетки плиток)");
}

/// Гараж — три бетонные стены буквой "П" (проём со стороны двора, откуда
/// заезжает машина) + гофрированная металлическая крыша сверху (визуальная
/// доработка — раньше гараж был без крыши). Возвращает `WallAabb` стен для
/// коллизии машины в `car_sim` (проём НЕ входит в список — там разрешено
/// проезжать).
fn setup_garage(engine: &mut AlkashEngine) -> Vec<WallAabb> {
    println!("\n[MAIN_CAR] Setting up garage...");

    let (cw, ch, cpix) = proc_textures::concrete_wall(256);
    let wall_srv = engine.create_texture_rgba(cw, ch, &cpix);

    let (rw, rh, rpix) = proc_textures::corrugated_metal(256, [0.5, 0.14, 0.10]);
    let roof_srv = engine.create_texture_rgba(rw, rh, &rpix);

    let half_h = GARAGE_WALL_HEIGHT * 0.5;
    let half_th = GARAGE_WALL_THICKNESS * 0.5;

    // Задняя стена (дальний край гаража по X, closes off the "П").
    let back_wall_mesh = engine.add_box_textured(
        [half_th, half_h, GARAGE_WIDTH * 0.5],
        0.6,
        [1.0, 1.0, 1.0, 1.0],
        wall_srv,
        0.9,
        0.0,
    );
    let back_wall_center = [GARAGE_CENTER_X + GARAGE_DEPTH * 0.5, GROUND_Y + half_h, GARAGE_CENTER_Z];
    engine.spawn_static_mesh(back_wall_mesh, back_wall_center, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);

    let mut walls = vec![WallAabb::from_center_half_extents(back_wall_center[0], back_wall_center[2], half_th, GARAGE_WIDTH * 0.5)];

    // Боковые стены.
    let side_wall_mesh = engine.add_box_textured(
        [GARAGE_DEPTH * 0.5, half_h, half_th],
        0.6,
        [1.0, 1.0, 1.0, 1.0],
        wall_srv,
        0.9,
        0.0,
    );
    for side in [-1.0f32, 1.0f32] {
        let center = [GARAGE_CENTER_X, GROUND_Y + half_h, GARAGE_CENTER_Z + side * (GARAGE_WIDTH * 0.5)];
        engine.spawn_static_mesh(side_wall_mesh, center, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        walls.push(WallAabb::from_center_half_extents(center[0], center[2], GARAGE_DEPTH * 0.5, half_th));
    }

    // Крыша — плоская коробка чуть шире стен по периметру (карниз), гофро-
    // металл. Раньше гараж крыши не имел вообще ("минимальный вертикальный
    // срез" — см. историю задачи #39); теперь это полноценное строение.
    let roof_half = [
        GARAGE_DEPTH * 0.5 + GARAGE_ROOF_OVERHANG,
        GARAGE_ROOF_THICKNESS * 0.5,
        GARAGE_WIDTH * 0.5 + GARAGE_ROOF_OVERHANG,
    ];
    let roof_mesh = engine.add_box_textured(roof_half, 0.5, [1.0, 1.0, 1.0, 1.0], roof_srv, 0.6, 0.15);
    engine.spawn_static_mesh(
        roof_mesh,
        [GARAGE_CENTER_X, GROUND_Y + GARAGE_WALL_HEIGHT + roof_half[1], GARAGE_CENTER_Z],
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    );

    println!("✅ Garage ready: 3 стены + крыша (открытый проём со стороны площадки)");
    walls
}

/// Плоский физический "пол" — теперь нужен ТОЛЬКО под зоной, где падают и
/// катаются бочки (см. `setup_yard_props`), не под всей площадкой и не под
/// машиной (машина больше не физическое тело, см. шапку файла) — гораздо
/// меньшая зона, чем в исторической версии этого файла.
const BARREL_FLOOR_HALF: f32 = 4.0;
const BARREL_FLOOR_CENTER_X: f32 = 4.0;
const BARREL_FLOOR_CENTER_Z: f32 = 5.0;

fn setup_barrel_physics(engine: &mut AlkashEngine) {
    println!("\n[MAIN_CAR] Loading Inertial physics plugin (для бочек)...");

    let config = PhysicsConfig {
        max_bodies: 256,
        world_size: 100.0,
        cell_size: 4.0,
        solver_iterations: 8,
        use_simd: 0,
    };

    if let Err(e) = engine.init_physics(INERTIAL_DLL_PATH, config) {
        eprintln!(
            "[MAIN_CAR] WARNING: не удалось загрузить Inertial ({}): {:?} — бочки не будут падать. \
             Проверь, что alkash3d-inertial собран в release (cargo build --release из папки alkash3d-inertial).",
            INERTIAL_DLL_PATH, e
        );
        return;
    }
    println!("[MAIN_CAR] ✓ Inertial loaded");

    const SPACING: f32 = 0.9; // < 2*IMPLICIT_RADIUS(0.5) — с нахлёстом, без щелей
    let steps = (2.0 * BARREL_FLOOR_HALF / SPACING) as i32;
    let mut created = 0;
    for ix in 0..=steps {
        let x = BARREL_FLOOR_CENTER_X - BARREL_FLOOR_HALF + ix as f32 * SPACING;
        for iz in 0..=steps {
            let z = BARREL_FLOOR_CENTER_Z - BARREL_FLOOR_HALF + iz as f32 * SPACING;
            if engine.add_sphere_body(x, GROUND_Y, z, 0.0).is_some() {
                created += 1;
            }
        }
    }
    println!("[MAIN_CAR] ✓ Физический пол под бочками: {} опорных сфер", created);
}

/// ДОБАВЛЕНО (после первого тестового запуска — одна из бочек, созданных
/// через `spawn_physics_sphere`, укатилась через весь двор и оказалась
/// прямо у стартовой точки игрока): `resolve_contact_simple` в
/// `alkash3d-inertial/src/kernels/rigid_body.f90` разрешает контакт ТОЛЬКО
/// нормальным импульсом (restitution) + позиционной коррекцией — никакого
/// тангенциального (Кулоновского) трения там нет вообще, несмотря на поле
/// `friction` в `PhysicsBody`/`FortranRigidBody`. Значит любая сфера,
/// коснувшаяся земли с ненулевой горизонтальной скоростью (после падения и
/// отскока — почти гарантированно), катится по инерции и практически НЕ
/// останавливается: `add_sphere_body`/`spawn_physics_sphere` используют
/// фиксированный `linear_damping: 0.01` — при 0.01/кадр скорость затухает
/// до 1/e только за ~100 секунд. Вместо `spawn_physics_sphere` — свой
/// вызов `add_physics_body` с НАМНОГО большим `linear_damping` (реальное
/// трение качения бочки о грунт, а не honest sphere-physics трение,
/// которого тут просто нет) и меньшей `restitution` (бочка не мячик) — так
/// бочка падает, пару раз подскакивает/чуть катится и реально
/// останавливается за разумное время, а не разъезжается по всей площадке.
fn spawn_barrel(engine: &mut AlkashEngine, mesh_index: usize, x: f32, y: f32, z: f32) {
    let body = PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass: 25.0,
        inv_mass: 1.0 / 25.0,
        restitution: 0.15,
        friction: 0.5,
        linear_damping: 0.85,
        angular_damping: 0.85,
        is_static: 0,
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
    };
    let Some(body_id) = engine.add_physics_body(body) else { return };
    let entity = engine.spawn_mesh_entity(mesh_index);
    engine.set_entity_transform(entity, [x, y, z], [0.0, 0.0, 0.0]);
    engine.physics_links.push((body_id, entity));
}

/// Бочки (падают и катаются через Inertial — та же физика вращения тела,
/// что задачи #39-#40 отрабатывали на кузове машины) + декоративные
/// деревянные ящики у гаража (статичные, без физики — чистая массовка).
fn setup_yard_props(engine: &mut AlkashEngine, car: &PlayerCar) {
    println!("\n[MAIN_CAR] Setting up yard props...");

    if engine.physics_stats().is_some() {
        let (mw, mh, mpix) = proc_textures::corrugated_metal(128, [0.55, 0.28, 0.05]);
        let barrel_srv = engine.create_texture_rgba(mw, mh, &mpix);
        let barrel_mesh = engine.add_cylinder_textured(0.38, 0.85, 14, 1.4, [1.0, 1.0, 1.0, 1.0], barrel_srv, 0.7, 0.15);

        let barrel_positions = [
            (BARREL_FLOOR_CENTER_X - 1.0, 2.6, BARREL_FLOOR_CENTER_Z - 1.0),
            (BARREL_FLOOR_CENTER_X + 0.6, 3.4, BARREL_FLOOR_CENTER_Z + 0.4),
            (BARREL_FLOOR_CENTER_X - 0.4, 4.2, BARREL_FLOOR_CENTER_Z + 1.2),
            (BARREL_FLOOR_CENTER_X + 1.2, 3.0, BARREL_FLOOR_CENTER_Z - 0.6),
        ];
        for (x, y, z) in barrel_positions {
            spawn_barrel(engine, barrel_mesh, x, y, z);
        }
        println!("[MAIN_CAR] ✓ {} бочек (физика Inertial)", barrel_positions.len());
    }

    let (ww, wh, wpix) = proc_textures::wood_plank(256, [0.45, 0.30, 0.16]);
    let wood_srv = engine.create_texture_rgba(ww, wh, &wpix);
    let crate_mesh = engine.add_box_textured([0.5, 0.5, 0.5], 1.2, [1.0, 1.0, 1.0, 1.0], wood_srv, 0.85, 0.0);

    // Пара ящиков у стены гаража, подальше от того места, где стоит машина
    // (чтобы не мешали въезду) — но в пределах видимости с точки старта
    // игрока.
    let crate_positions = [
        (GARAGE_CENTER_X - GARAGE_DEPTH * 0.5 + 0.6, GROUND_Y + 0.5, GARAGE_CENTER_Z + GARAGE_WIDTH * 0.5 - 0.7),
        (GARAGE_CENTER_X - GARAGE_DEPTH * 0.5 + 0.6, GROUND_Y + 1.5, GARAGE_CENTER_Z + GARAGE_WIDTH * 0.5 - 0.7),
        (GARAGE_CENTER_X - GARAGE_DEPTH * 0.5 + 1.7, GROUND_Y + 0.5, GARAGE_CENTER_Z + GARAGE_WIDTH * 0.5 - 0.7),
    ];
    for (x, y, z) in crate_positions {
        engine.spawn_static_mesh(crate_mesh, [x, y, z], [0.0, 0.05, 0.0], [1.0, 1.0, 1.0]);
    }

    let _ = car; // зарезервировано (пока не используется — без ящиков "под машиной")
    println!("✅ Yard props ready");
}

/// Строит машину: два бокса кузова (нижний корпус + кабина сверху — для
/// более узнаваемого силуэта, чем один сплошной параллелепипед), 4 честных
/// цилиндрических колеса (см. `Mesh::cylinder_textured` — раньше колёса
/// были приплюснутыми кубами, "цилиндра в движке нет"), простые фары/
/// бампер. Возвращает `PlayerCar` с УЖЕ готовой аркадной симуляцией
/// (`car_sim::CarState`) — машина НЕ физическое тело Inertial, см. шапку
/// файла.
fn spawn_player_car(engine: &mut AlkashEngine, start_pos: Vec3, start_yaw: f32) -> PlayerCar {
    println!("\n[MAIN_CAR] Spawning player car...");

    let dims = CarDimensions::new();

    let car_color = [0.62, 0.08, 0.06]; // тёмно-красный
    let (pw, ph, ppix) = proc_textures::car_paint(128, car_color);
    let paint_srv = engine.create_texture_rgba(pw, ph, &ppix);

    let (tw, th, tpix) = proc_textures::tire_rubber(128);
    let tire_srv = engine.create_texture_rgba(tw, th, &tpix);

    let (chw, chh, chpix) = proc_textures::chrome(64);
    let chrome_srv = engine.create_texture_rgba(chw, chh, &chpix);

    let chassis_mesh = engine.add_box_textured(dims.chassis_half, 0.8, [1.0, 1.0, 1.0, 1.0], paint_srv, 0.35, 0.25);
    let cabin_mesh = engine.add_box_textured(dims.cabin_half, 0.8, [1.0, 1.0, 1.0, 1.0], paint_srv, 0.35, 0.25);
    let wheel_mesh = engine.add_cylinder_textured(dims.wheel_radius, dims.wheel_width, 16, 1.6, [1.0, 1.0, 1.0, 1.0], tire_srv, 0.85, 0.0);
    let bumper_mesh = engine.add_box_textured([dims.chassis_half[0] * 0.95, 0.10, 0.12], 0.5, [1.0, 1.0, 1.0, 1.0], chrome_srv, 0.25, 0.8);
    let headlight_mesh = engine.add_cube_colored(0.18, 1.0, 0.95, 0.75, 1.0);
    let taillight_mesh = engine.add_cube_colored(0.16, 0.75, 0.05, 0.03, 1.0);

    let entity = engine.spawn_static_mesh(
        chassis_mesh,
        [start_pos.x, dims.root_y, start_pos.z],
        [0.0, start_yaw, 0.0],
        [1.0, 1.0, 1.0],
    );

    engine.spawn_child_mesh(
        cabin_mesh,
        entity,
        [0.0, dims.chassis_half[1] + dims.cabin_half[1], -dims.chassis_half[2] * 0.12],
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    );

    // Бамперы спереди/сзади.
    for &z_sign in &[1.0f32, -1.0f32] {
        engine.spawn_child_mesh(
            bumper_mesh,
            entity,
            [0.0, -dims.chassis_half[1] * 0.3, z_sign * (dims.chassis_half[2] + 0.10)],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
    }

    // Фары (передние, жёлто-белые) и стоп-сигналы (задние, красные).
    for &x_sign in &[-1.0f32, 1.0f32] {
        engine.spawn_child_mesh(
            headlight_mesh,
            entity,
            [x_sign * dims.chassis_half[0] * 0.6, dims.chassis_half[1] * 0.15, dims.chassis_half[2] + 0.02],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
        engine.spawn_child_mesh(
            taillight_mesh,
            entity,
            [x_sign * dims.chassis_half[0] * 0.6, dims.chassis_half[1] * 0.15, -dims.chassis_half[2] - 0.02],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        );
    }

    // Колёса: [0]=FL, [1]=FR, [2]=RL, [3]=RR — X<0 = лево, Z>0 = перед
    // (совпадает со знаком `wheel_z`/направлением "вперёд" в `car_sim`).
    let wheel_local_positions: [[f32; 3]; 4] = [
        [-dims.wheel_x, dims.wheel_y_local, dims.wheel_z],
        [dims.wheel_x, dims.wheel_y_local, dims.wheel_z],
        [-dims.wheel_x, dims.wheel_y_local, -dims.wheel_z],
        [dims.wheel_x, dims.wheel_y_local, -dims.wheel_z],
    ];
    let mut wheel_entities = [EntityId::INVALID; 4];
    for (i, pos) in wheel_local_positions.iter().enumerate() {
        wheel_entities[i] = engine.spawn_child_mesh(wheel_mesh, entity, *pos, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    }

    println!("✅ Car ready at ({:.1}, {:.1}, {:.1}), yaw={:.2}", start_pos.x, dims.root_y, start_pos.z, start_yaw);

    PlayerCar {
        entity,
        wheel_entities,
        dims,
        state: CarState::new(Vec3::new(start_pos.x, dims.root_y, start_pos.z), start_yaw),
        params: CarParams {
            ground_y: dims.root_y,
            ..CarParams::default()
        },
    }
}

fn run_loop(engine: &mut AlkashEngine, mut car: PlayerCar, garage_walls: Vec<WallAabb>, world_bounds: WallAabb) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    // Кэшируем локальные позиции колёс ОДИН раз (они не меняются) — честная
    // замена `entity_local_pos_unused` выше, см. её комментарий.
    let wheel_local_positions: [[f32; 3]; 4] = [
        [-car.dims.wheel_x, car.dims.wheel_y_local, car.dims.wheel_z],
        [car.dims.wheel_x, car.dims.wheel_y_local, car.dims.wheel_z],
        [-car.dims.wheel_x, car.dims.wheel_y_local, -car.dims.wheel_z],
        [car.dims.wheel_x, car.dims.wheel_y_local, -car.dims.wheel_z],
    ];

    let mut frame_count: u64 = 0;
    let mut time = 0.0f32;
    let start = Instant::now();

    let mut fps_window_start = Instant::now();
    let mut fps_window_frames: u32 = 0;

    let mut mode = PlayerMode::Walking;
    let mut near_car_prompted = false;
    let mut chase_look_offset = 0.0f32;

    // Игрок стартует рядом с машиной, лицом к ней — на расстоянии, с
    // которого видно её целиком (первая версия ставила камеру всего в
    // ~3м от 3.8-метрового кузова, что при обычном FOV означало "стоять
    // почти уткнувшись носом в дверь", см. первый тестовый скриншот).
    engine.camera.position = Vec3::new(CAR_START_X - 5.0, EYE_HEIGHT, CAR_START_Z - 4.5);
    engine.camera.target = Vec3::new(CAR_START_X, EYE_HEIGHT - 0.3, CAR_START_Z);

    let rot_speed = 2.0;

    while engine.is_running() {
        engine.process_messages();
        if !engine.is_running() {
            break;
        }

        let dt = {
            let now = start.elapsed().as_secs_f32();
            let dt = (now - time).min(0.05);
            time = now;
            dt
        };

        if engine.input.just_pressed(keys::ESCAPE) {
            println!("[MAIN_CAR] ESC pressed - exiting");
            engine.request_exit();
            continue;
        }

        // --- Сесть/выйти ---
        if engine.input.just_pressed(keys::E) {
            match mode {
                PlayerMode::Walking => {
                    if car.distance_xz(engine.camera.position) <= ENTER_CAR_DISTANCE {
                        mode = PlayerMode::Driving;
                        println!("[MAIN_CAR] 🚗 Сел в машину — WASD: газ/тормоз/руль, R: ручник, E: выйти");
                    }
                }
                PlayerMode::Driving => {
                    mode = PlayerMode::Walking;
                    let right = car.state.forward().cross(Vec3::Y).normalize();
                    let exit_pos = car.state.position + right * 2.2 + Vec3::new(0.0, EYE_HEIGHT - car.dims.root_y, 0.0);
                    engine.camera.position = exit_pos;
                    engine.camera.target = exit_pos + car.state.forward();
                    println!("[MAIN_CAR] 🚶 Вышел из машины");
                }
            }
        }

        match mode {
            PlayerMode::Walking => {
                let rot_amount = rot_speed * dt;
                if engine.input.is_down(keys::ARROW_LEFT) { engine.camera.rotate_yaw(rot_amount); }
                if engine.input.is_down(keys::ARROW_RIGHT) { engine.camera.rotate_yaw(-rot_amount); }
                if engine.input.is_down(keys::ARROW_UP) { engine.camera.rotate_pitch(-rot_amount); }
                if engine.input.is_down(keys::ARROW_DOWN) { engine.camera.rotate_pitch(rot_amount); }

                let forward = {
                    let dir = engine.camera.target - engine.camera.position;
                    let flat = Vec3::new(dir.x, 0.0, dir.z);
                    if flat.length_squared() > 1e-6 { flat.normalize() } else { Vec3::new(0.0, 0.0, 1.0) }
                };
                let right = forward.cross(Vec3::Y).normalize();

                let shift = engine.input.is_down(keys::SHIFT);
                let move_speed = if shift { 10.0 } else { 5.0 };
                let move_amount = move_speed * dt;

                let mut delta = Vec3::ZERO;
                if engine.input.is_down(keys::W) { delta += forward * move_amount; }
                if engine.input.is_down(keys::S) { delta -= forward * move_amount; }
                if engine.input.is_down(keys::A) { delta -= right * move_amount; }
                if engine.input.is_down(keys::D) { delta += right * move_amount; }

                engine.camera.position += delta;
                engine.camera.target += delta;

                let dy = EYE_HEIGHT - engine.camera.position.y;
                engine.camera.position.y += dy;
                engine.camera.target.y += dy;

                let near = car.distance_xz(engine.camera.position) <= ENTER_CAR_DISTANCE;
                if near && !near_car_prompted {
                    println!("[MAIN_CAR] 🔑 Рядом с машиной — E, чтобы сесть за руль");
                    near_car_prompted = true;
                } else if !near {
                    near_car_prompted = false;
                }
            }
            PlayerMode::Driving => {
                let throttle = if engine.input.is_down(keys::W) { 1.0 }
                    else if engine.input.is_down(keys::S) { -1.0 }
                    else { 0.0 };
                let steer = if engine.input.is_down(keys::A) { -1.0 }
                    else if engine.input.is_down(keys::D) { 1.0 }
                    else { 0.0 };
                let handbrake = engine.input.is_down(keys::R);

                car.state.step(
                    CarInput { throttle, steer, handbrake },
                    &car.params,
                    dt,
                    &garage_walls,
                    world_bounds,
                );

                engine.set_entity_transform(
                    car.entity,
                    [car.state.position.x, car.state.position.y, car.state.position.z],
                    [car.state.visual_pitch, car.state.yaw, 0.0],
                );

                for (i, &wheel) in car.wheel_entities.iter().enumerate() {
                    let steer_here = if i < 2 { car.state.steer_angle } else { 0.0 };
                    engine.set_entity_transform(wheel, wheel_local_positions[i], [car.state.wheel_spin, steer_here, 0.0]);
                }

                // Камера "от третьего лица" за машиной — стрелки влево/
                // вправо чуть довора́чивают взгляд вокруг машины, не
                // трогая саму симуляцию руления.
                let look_amount = rot_speed * dt;
                if engine.input.is_down(keys::ARROW_LEFT) { chase_look_offset -= look_amount; }
                if engine.input.is_down(keys::ARROW_RIGHT) { chase_look_offset += look_amount; }
                chase_look_offset = chase_look_offset.clamp(-2.2, 2.2);

                let chase_yaw = car.state.yaw + chase_look_offset;
                let chase_dir = Vec3::new(chase_yaw.sin(), 0.0, chase_yaw.cos());
                let chase_distance = 6.0;
                let chase_height = 2.6;
                engine.camera.position = car.state.position - chase_dir * chase_distance + Vec3::new(0.0, chase_height, 0.0);
                engine.camera.target = car.state.position + Vec3::new(0.0, 0.7, 0.0);
            }
        }

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        engine.update(
            dt,
            -9.8,
            [engine.camera.position.x, engine.camera.position.y, engine.camera.position.z],
            view_proj.to_cols_array(),
        );

        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN_CAR] Render error, stopping: {:?}", e);
            break;
        }

        frame_count += 1;
        fps_window_frames += 1;

        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
            println!("*** {} ECS entities rendering ***\n", engine.scene_entity_count());
        }

        if fps_window_start.elapsed().as_secs_f32() >= 1.0 {
            let fps = fps_window_frames as f32 / fps_window_start.elapsed().as_secs_f32();
            let mode_str = match mode { PlayerMode::Walking => "walking", PlayerMode::Driving => "driving" };
            println!(
                "[INFO] Frame {} | FPS: {:.1} | mode={} | speed={:.1} m/s | entities: {}",
                frame_count, fps, mode_str, car.state.speed, engine.scene_entity_count(),
            );
            if let Some(stats) = engine.physics_stats() {
                println!(
                    "[PHYS-STATS] bodies={} active={} contacts={} pairs={}",
                    stats.bodies_count, stats.active_bodies, stats.contacts_count, stats.pairs_count
                );
            }
            fps_window_frames = 0;
            fps_window_start = Instant::now();
        }
    }
}
