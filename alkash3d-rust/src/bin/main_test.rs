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

use alkash3d_rs::engine::{AlkashEngine, GraphicsSettings};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::{AlworldFile, GlobalObject, GLOBAL_OBJECT_FLAG_SPAWN_POINT};
// ДОБАВЛЕНО (перенос света из эдитора в main_test.rs — по прямому запросу
// пользователя, баг: "накидывал свет в эдиторе, но он не переносился в
// main_test.rs"): `LightConfig` — тот же реэкспорт с корня крейта, что
// использует `main.rs::setup_lights` (модуль `plugin` приватный — см.
// комментарий там же про E0603). Сам `.alfar` читается не напрямую (тип
// `AlfarFile` тут не нужен), а через `AlkashEngine::load_lights_from_alfar`.
// `windows::core::Interface` нужен только ради `.as_raw()` на D3D12-device.
use alkash3d_rs::LightConfig;
use std::time::Instant;
use windows::core::Interface;

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 576;
const EYE_HEIGHT: f32 = 1.6;
const TEST_WORLD_PATH: &str = "test.alworld";
// ДОБАВЛЕНО (перенос света из эдитора в main_test.rs): свет — ОТДЕЛЬНЫЙ от
// .alworld формат (эдитор экспортирует его через "Lighting to .alfar..."
// в меню, не через "Scene to .alworld" — см. converters/alfar.rs и
// alworld.rs в alkash3d-editorapp), поэтому рядом с test.alworld ищем свой
// test.alfar. В отличие от test.alworld, синтетический test.alfar НЕ
// генерируется, если его нет — сцена спавна и без света валидна сама по
// себе, свет тут строго опционален.
const TEST_ALFAR_PATH: &str = "test.alfar";
/// Тот же путь к FirstFires, что и в `main.rs` (см. подробный комментарий
/// там про relative-путь и ловушку "firstfires.dll" без подчёркивания) —
/// оба bin-файла запускаются из одной и той же рабочей директории
/// (`alkash3d-rust/`), так что путь идентичен.
const FIRSTFIRES_DLL_PATH: &str = "./alkash3d_firstfires.dll";

// Тестовые координаты/угол точки спавна — специально не (0,0,0)/0°, чтобы
// проверка не могла случайно "пройти" из-за того, что 0 — это ещё и
// значение по умолчанию, если что-то не прочиталось.
const SPAWN_X: f32 = 10.0;
const SPAWN_Y: f32 = 0.0;
const SPAWN_Z: f32 = 5.0;
const SPAWN_YAW_DEG: f32 = 45.0;

// ДОБАВЛЕНО (переключаемые графические настройки — по прямому запросу
// пользователя: "а в коде можно задать типо true или false"): простые
// константы вместо переменных окружения — поменял значение здесь,
// пересобрал, готово. Читаются один раз в `main()` и передаются в
// `engine.set_graphics_settings()` ДО `engine.init()` (см. `GraphicsSettings`
// в engine/mod.rs).
const ENABLE_MSAA: bool = false;
const ENABLE_SSAO: bool = false;
const ENABLE_BLOOM: bool = false;
const ENABLE_VOLUMETRIC: bool = false;
const ENABLE_SHADOWS: bool = false;

// УВЕЛИЧЕНО (по прямому запросу пользователя: "дальность прорисовки мира
// больше"): `Camera::new` по умолчанию ставит `far = 100.0` (см.
// camera.rs), и main_test.rs эту переменную нигде не переопределял — всё,
// что дальше 100 юнитов от камеры, попросту отсекалось near/far-clipping'ом
// перспективной проекции (`perspective()` в math.rs), а не culling'ом или
// LOD-механизмом. 1000.0 даёт запас на порядок больше typичного размера
// одного чанка `.alworld` (по умолчанию 64м, см. alworld_format.rs).
const CAMERA_FAR_PLANE: f32 = 1000.0;

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

    // ДОБАВЛЕНО (переключаемые графические настройки — по прямому запросу
    // пользователя): значения берутся из констант ENABLE_* выше — поменяй
    // их там (true/false) и пересобери, никаких переменных окружения.
    let graphics_settings = GraphicsSettings {
        msaa: ENABLE_MSAA,
        ssao: ENABLE_SSAO,
        bloom: ENABLE_BLOOM,
        volumetric: ENABLE_VOLUMETRIC,
        shadows: ENABLE_SHADOWS,
    };
    println!(
        "[MAIN_TEST] GraphicsSettings: msaa={} ssao={} bloom={} volumetric={} shadows={} (правь константы ENABLE_* в начале файла)",
        graphics_settings.msaa, graphics_settings.ssao, graphics_settings.bloom,
        graphics_settings.volumetric, graphics_settings.shadows
    );

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    // ВАЖНО: ДО init() — настройки читаются один раз при построении
    // шейдеров/PSO/ресурсов, после init() эффекта уже не будет (см.
    // подробный комментарий у `set_graphics_settings` в lifecycle.rs).
    engine.set_graphics_settings(graphics_settings);
    if let Err(e) = engine.init() {
        eprintln!("[MAIN_TEST] Failed to initialize engine: {:?}", e);
        return Err(e.into());
    }

    engine.set_clear_color(0.05, 0.05, 0.12, 1.0);
    engine.camera.far = CAMERA_FAR_PLANE;

    // ДОБАВЛЕНО (по прямому запросу пользователя: "сделай ночь, я не
    // понимаю этот свет там есть или нету"): по умолчанию `time_of_day`
    // движка — 12.0 (полдень, см. `AlkashEngine::new`), и `update_day_night`
    // каждый кадр заливает сцену ярким дневным ambient+directional светом
    // (см. тот же комментарий в `main.rs::setup_lights`) — на этом фоне
    // маленькие точечные фонари из .alfar (если они вообще есть и загрузились)
    // визуально неотличимы от их отсутствия. Фиксируем ночь (22:00, тот же
    // час, что и `main.rs`) ДО загрузки мира — только тогда разница
    // "фонарь горит" vs "фонаря нет" станет видна глазом. `day_night_speed`
    // по умолчанию 0 (см. `AlkashEngine::new`), так что время суток само
    // не "уедет" обратно к дню за время теста.
    engine.set_time_of_day(22.0);

    setup_lights_from_alfar(&mut engine);

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

/// ДОБАВЛЕНО (перенос света из эдитора в main_test.rs): подключает
/// FirstFires и, если рядом с exe лежит `test.alfar` (эдитор: File >
/// "Lighting to .alfar..."), загружает из него источники света — тот же
/// путь данных, что `main.rs::setup_lights` использует для
/// `night_city_demo.alfar`, только БЕЗ генерации синтетической сцены:
/// этот бинарник проверяет ИМЕННО перенос СВОЕГО света пользователя, а не
/// демонстрирует захардкоженную демку.
///
/// Ни отсутствие FirstFires.dll, ни отсутствие `test.alfar` не считаются
/// ошибкой теста — оба этапа опциональны, тест точки спавна (ради которого
/// этот бинарник изначально существует) должен продолжать работать даже
/// без единого источника света рядом.
fn setup_lights_from_alfar(engine: &mut AlkashEngine) {
    let device_ptr = match alkash3d_rs::get_device() {
        Ok(device) => device.as_raw(),
        Err(e) => {
            eprintln!("[MAIN_TEST] Не удалось получить D3D12 device для FirstFires: {:?} — свет не будет загружен", e);
            return;
        }
    };

    // ИСПРАВЛЕНО (баг: "граница света рвётся резкой линией ровно там, где
    // раньше пропадала карта" — обнаружено после включения чанковой
    // стриминга и подъёма CAMERA_FAR_PLANE до 1000.0 выше): сетка каллинга
    // FirstFires — это ФИКСИРОВАННЫЙ куб [-far_plane, far_plane]^3 вокруг
    // МИРОВОГО НАЧАЛА КООРДИНАТ (не вокруг камеры, см. `LightState::new` в
    // alkash3d-FirstFires/src/lib.rs), заведённый ОДИН раз при
    // `init_lights()` и никогда не пересчитываемый. far_plane=200.0 здесь
    // оставался с тех времён, когда `camera.far` тоже было 100-200 —
    // теперь `camera.far` (и, соответственно, реально стримящаяся и
    // видимая часть мира) уходит до 1000.0, но сетка каллинга по-прежнему
    // обрывается на 200 от начала координат. Пиксельный шейдер
    // (`pipeline_main.rs`) при выходе мировой позиции пикселя за пределы
    // `gridDimensions` просто не добавляет ни одного источника (проверка
    // `(uint)cell.x < gridDimensions.x` и т.д.) — получается не плавное
    // затухание, а резкий обрыв освещения РОВНО НА ГРАНИЦЕ КУБА СЕТКИ,
    // тогда как геометрия за этой границей теперь честно рендерится.
    //
    // Фикс — тот же приём, что уже применялся раньше при подъёме дальности
    // (см. историю в LightConfig в main.rs): far_plane поднят до 1000.0
    // вместе с grid_cell_size (20.0 -> 100.0), чтобы сетка по-прежнему
    // состояла из тех же 20x20x20=8000 ячеек (та же память), но теперь
    // накрывала весь куб [-1000,1000]^3 — с запасом, покрывающим
    // CAMERA_FAR_PLANE. lod_distances[2] поднят синхронно с far_plane по
    // той же причине, что и раньше — иначе LOD-каллинг обрезал бы фонари
    // на дистанции 200 ещё до того, как размер сетки стал бы иметь
    // значение.
    let config = LightConfig {
        max_lights: 64,
        tile_size: 16,
        far_plane: 1000.0,
        lod_distances: [30.0, 60.0, 1000.0],
        grid_cell_size: 100.0,
    };

    if let Err(e) = engine.init_lights(FIRSTFIRES_DLL_PATH, device_ptr, config) {
        eprintln!(
            "[MAIN_TEST] FirstFires не загружен ({}): {:?} — свет из {} (если он есть) пропущен",
            FIRSTFIRES_DLL_PATH, e, TEST_ALFAR_PATH
        );
        return;
    }

    if !std::path::Path::new(TEST_ALFAR_PATH).exists() {
        println!("[MAIN_TEST] {} не найден рядом с exe — сцена без света (это нормально, если ты его не экспортировал)", TEST_ALFAR_PATH);
        return;
    }

    match engine.load_lights_from_alfar(TEST_ALFAR_PATH) {
        Ok(count) => println!("[MAIN_TEST] ✓ Загружено {} источников света из {}", count, TEST_ALFAR_PATH),
        Err(e) => eprintln!("[MAIN_TEST] ✗ Не удалось загрузить {}: {:?}", TEST_ALFAR_PATH, e),
    }
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
