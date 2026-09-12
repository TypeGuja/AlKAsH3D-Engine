// src/bin/main_test.rs
//! ДОБАВЛЕНО (проверка точки спавна игрока — по прямому запросу
//! пользователя): минимальный, специально урезанный демо-бинарник, чтобы
//! проверить путь `.alworld` -> `AlkashEngine::world_spawn_point()` ->
//! стартовая позиция камеры (см. `world_streaming.rs::world_spawn_point`
//! и правки в `main.rs`), не запуская тяжёлую сцену `main.rs` (FirstFires,
//! физика, скриптинг, 945 плиток пола) — та сцена уже не раз фризила
//! машину целиком (см. память проекта про фризы), а для проверки ОДНОГО
//! узкого механизма она не нужна вообще: чем меньше движущихся частей в
//! тестовом сценарии, тем быстрее и безопаснее проверка.
//!
//! Что делает:
//!   1. Создаёт (перезаписывает) `test.alworld` рядом с exe — БЕЗ единого
//!      чанка (в отличие от `AlworldFile::create_and_save_demo_world`,
//!      которая генерирует 9x9 чанков с .alwchunk-файлами на диске) — сама
//!      точка спавна хранится прямо в `AlworldFile::global_objects`,
//!      сериализуется вместе с заголовком в ОДНОМ файле, чанки ей не
//!      нужны (см. `alworld_format::GLOBAL_OBJECT_FLAG_SPAWN_POINT`).
//!   2. Ставит один красный куб-маркер в 3м впереди по направлению взгляда
//!      от точки спавна — если камера стартовала в нужном месте и смотрит
//!      в нужную сторону, маркер будет прямо по центру экрана на первом
//!      кадре.
//!   3. Загружает `test.alworld`, читает `world_spawn_point()`, ставит
//!      камеру туда (с тем же fallback-логированием, что и в `main.rs`).
//!   4. Простой цикл: WASD + стрелки, ESC — выход (тот же ввод, что в
//!      `main.rs`), рендерится ТОЛЬКО куб-маркер — никакого дополнительного
//!      контента.
//!
//! Проверка на глаз: если в консоли/`engine_test_log.txt` есть строка
//! "✓ Точка спавна прочитана: (10.00, 0.00, 5.00), yaw=45.0°" И красный
//! куб виден прямо перед камерой на первом кадре — весь путь работает.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::{AlworldFile, GlobalObject, GLOBAL_OBJECT_FLAG_SPAWN_POINT};
use std::time::Instant;

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 576;
const EYE_HEIGHT: f32 = 1.6;
const TEST_WORLD_PATH: &str = "test.alworld";

// Тестовые координаты/угол точки спавна — специально не (0,0,0)/0°, чтобы
// проверка не могла случайно "пройти" из-за того, что 0 — это ещё и
// значение по умолчанию, если что-то не прочиталось.
const SPAWN_X: f32 = 10.0;
const SPAWN_Y: f32 = 0.0;
const SPAWN_Z: f32 = 5.0;
const SPAWN_YAW_DEG: f32 = 45.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    alkash3d_rs::console_log::init_console_log_to_file("engine_test_log.txt");

    println!("==========================================");
    println!("Alkash3D Engine v{} — MAIN_TEST", alkash3d_rs::VERSION);
    println!("Проверка: точка спавна из .alworld -> позиция камеры");
    println!("==========================================");
    println!("WASD — ходьба, стрелки — осмотреться, ESC — выход\n");

    // ИСПРАВЛЕНО (баг: "я же вроде положил рядом test.alworld, а вижу
    // только 1 куб-маркер"): раньше `create_test_world` вызывался БЕЗУСЛОВНО
    // и перезаписывал `test.alworld` своим синтетическим содержимым КАЖДЫЙ
    // раз — если пользователь клал туда СВОЙ файл (например экспорт из
    // эдитора с реальной сценой), он затирался ДО того, как успевал
    // загрузиться, и запускался всегда один и тот же синтетический тест.
    // Теперь генерируем `test.alworld` только если его там ещё нет — если
    // файл уже существует, используем его как есть (это и есть весь смысл
    // "положить своё test.alworld рядом").
    if std::path::Path::new(TEST_WORLD_PATH).exists() {
        println!("[MAIN_TEST] {} уже существует — использую как есть (не перезаписываю)", TEST_WORLD_PATH);
    } else if let Err(e) = create_test_world(TEST_WORLD_PATH) {
        eprintln!("[MAIN_TEST] Не удалось создать {}: {:?}", TEST_WORLD_PATH, e);
        return Err(e.into());
    } else {
        println!("[MAIN_TEST] ✓ {} создан (синтетическая точка спавна: ({:.1}, {:.1}, {:.1}), yaw={:.1}°)", TEST_WORLD_PATH, SPAWN_X, SPAWN_Y, SPAWN_Z, SPAWN_YAW_DEG);
    }

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    if let Err(e) = engine.init() {
        eprintln!("[MAIN_TEST] Failed to initialize engine: {:?}", e);
        return Err(e.into());
    }

    engine.set_clear_color(0.05, 0.05, 0.12, 1.0);

    match engine.load_world(TEST_WORLD_PATH, None) {
        Ok(()) => println!("[MAIN_TEST] ✓ {} загружен", TEST_WORLD_PATH),
        Err(e) => eprintln!("[MAIN_TEST] ✗ Не удалось загрузить {}: {:?}", TEST_WORLD_PATH, e),
    }

    // ИЗМЕНЕНО: куб-маркер теперь ставится по РЕАЛЬНО прочитанной точке
    // спавна (а не по захардкоженным SPAWN_X/Y/Z/YAW) — если рядом лежит
    // СВОЙ test.alworld с другой точкой спавна (или без неё вовсе), маркер
    // (если он есть) честно отражает то, что реально загрузилось, а не
    // синтетические координаты, которые в этом случае никак не участвуют.
    match engine.world_spawn_point() {
        Some((pos, yaw)) => {
            println!(
                "[MAIN_TEST] ✓ Точка спавна прочитана: ({:.2}, {:.2}, {:.2}), yaw={:.1}°",
                pos.x, pos.y, pos.z, yaw.to_degrees()
            );
            let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());

            let marker_pos = pos + forward * 3.0;
            let marker_mesh = engine.add_cube_colored(0.6, 0.9, 0.25, 0.2, 1.0);
            let marker = engine.spawn_mesh_entity(marker_mesh);
            if let Some(t) = engine.scene.transform_mut(marker) {
                t.position = [marker_pos.x, marker_pos.y + 0.3, marker_pos.z];
            }

            engine.camera.position = Vec3::new(pos.x, pos.y + EYE_HEIGHT, pos.z);
            engine.camera.target = engine.camera.position + forward;
        }
        None => {
            eprintln!("[MAIN_TEST] ✗ Точка спавна НЕ найдена в загруженном мире — тест провален (ожидалась одна в global_objects)");
            engine.camera.position = Vec3::new(0.0, EYE_HEIGHT, 0.0);
            engine.camera.target = Vec3::new(0.0, EYE_HEIGHT, 1.0);
        }
    }

    run_loop(&mut engine);

    engine.shutdown();
    println!("[MAIN_TEST] Goodbye!");
    Ok(())
}

/// Пишет минимальный `.alworld` без единого чанка — только точка спавна
/// в `global_objects` (см. пояснение в шапке файла). Тот же приём кодинга
/// yaw в `lod_distances[0]`, что и `converters/alworld.rs::
/// export_scene_to_alworld` в эдиторе — намеренно, чтобы тестовый файл
/// был читаем тем же путём, каким эдитор реально пишет точки спавна.
fn create_test_world(path: &str) -> std::io::Result<()> {
    let mut world = AlworldFile::new(0.1);
    world.chunks.clear();

    let name_id = world.add_string("TestSpawn");

    let mut transform = [0.0f32; 16];
    transform[0] = 1.0;
    transform[5] = 1.0;
    transform[10] = 1.0;
    transform[15] = 1.0;
    transform[12] = SPAWN_X;
    transform[13] = SPAWN_Y;
    transform[14] = SPAWN_Z;

    world.global_objects.push(GlobalObject {
        name_id,
        altex_file_id: 0xFFFF_FFFF,
        transform,
        lod_distances: [SPAWN_YAW_DEG.to_radians(), 0.0, 0.0, 0.0],
        flags: GLOBAL_OBJECT_FLAG_SPAWN_POINT,
    });

    world.save(path)
}

/// Тот же ввод (WASD/стрелки/ESC), что и в `main.rs::run_loop` — скопирован
/// намеренно один в один, чтобы поведение камеры при проверке ощущалось
/// так же, а не как отдельная, по-новому написанная реализация.
fn run_loop(engine: &mut AlkashEngine) {
    let start = Instant::now();
    let mut time = 0.0f32;
    let mut frame_count = 0u32;
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
            println!("[MAIN_TEST] ESC pressed - exiting");
            engine.request_exit();
            continue;
        }

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

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        engine.update(
            dt,
            -9.8,
            [engine.camera.position.x, engine.camera.position.y, engine.camera.position.z],
            view_proj.to_cols_array(),
        );

        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN_TEST] Render error: {:?}", e);
            break;
        }

        frame_count += 1;
        if frame_count == 1 {
            println!(
                "[MAIN_TEST] Первый кадр отрендерен. Camera pos=({:.2},{:.2},{:.2}) target=({:.2},{:.2},{:.2})",
                engine.camera.position.x, engine.camera.position.y, engine.camera.position.z,
                engine.camera.target.x, engine.camera.target.y, engine.camera.target.z,
            );
        }
    }
}
