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
//!
//! ДОБАВЛЕНО (по прямому запросу пользователя: "сделай чтобы он читал все
//! нужные файлы для карты"): режим ПАПКИ КАРТЫ — той, что эдитор пишет через
//! File > Export > Scene to .alworld. Из неё сам берётся всё:
//!   * `world.alworld` + `chunks/` (геометрия, текстуры по ссылкам),
//!   * весь свет: `*.alfar` в корне папки и в `lights/`,
//!   * точка спавна — из самого `.alworld`.
//! Папку можно указать (по порядку): аргументом (`main_test <папка>` или
//! путь к .alworld), переменной окружения `ALKASH3D_MAP`, одной строкой в
//! `test_map.txt` (рядом с запуском или с exe), или просто запустить exe из
//! самой папки карты (рядом лежит `world.alworld`). Без всего этого —
//! прежний режим `test.alworld`/`test.alfar` в текущей папке.
//! Управление (ДОБАВЛЕНО — режим пешехода по прямому запросу пользователя:
//! "добавь колизию для карты, чтобы камера была как человек"): камера ходит
//! по земле с гравитацией на высоте глаз 1.7 м, поднимается на бордюры и
//! ступени до 45 см, не проходит сквозь стены/заборы/столбы. Коллизия
//! считается на CPU из тех же .altex чанков, что рисует движок (модуль
//! `walk` в конце файла, чанки вокруг игрока грузятся в фоне).
//!   WASD — ходьба, SHIFT — бег, SPACE — прыжок, стрелки — осмотреться,
//!   F — режим полёта сквозь всё (E/Q вверх/вниз, SPACE — быстро), ESC — выход.

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

const WINDOW_WIDTH: u32 = 1600;
/// VK_F — переключение полёт/пешком (в `input::keys` константы F нет).
const KEY_F: u32 = 0x46;
const WINDOW_HEIGHT: u32 = 900;
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
    let map = resolve_map_files();
    if !map.from_map_dir {
        if std::path::Path::new(TEST_WORLD_PATH).exists() {
            println!("[MAIN_TEST] {} уже существует — использую как есть (не перезаписываю)", TEST_WORLD_PATH);
        } else if let Err(e) = create_test_world(TEST_WORLD_PATH) {
            eprintln!("[MAIN_TEST] Не удалось создать {}: {:?}", TEST_WORLD_PATH, e);
            return Err(e.into());
        } else {
            println!("[MAIN_TEST] ✓ {} создан (синтетическая точка спавна: ({:.1}, {:.1}, {:.1}), yaw={:.1}°)", TEST_WORLD_PATH, SPAWN_X, SPAWN_Y, SPAWN_Z, SPAWN_YAW_DEG);
        }
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

    setup_lights_from_alfar(&mut engine, &map.alfars);

    let world_str = map.world.to_string_lossy().into_owned();
    let chunks_str = map.chunks.to_string_lossy().into_owned();
    match engine.load_world(&world_str, Some(&chunks_str)) {
        Ok(()) => println!("[MAIN_TEST] ✓ {} загружен (чанки из {})", world_str, chunks_str),
        Err(e) => eprintln!("[MAIN_TEST] ✗ Не удалось загрузить {}: {:?}", world_str, e),
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

            // куб-маркер нужен только синтетическому тесту спавна, не настоящей карте
            if !map.from_map_dir {
                let marker_pos = pos + forward * 3.0;
                let marker_mesh = engine.add_cube_colored(0.6, 0.9, 0.25, 0.2, 1.0);
                let marker = engine.spawn_mesh_entity(marker_mesh);
                if let Some(t) = engine.scene.transform_mut(marker) {
                    t.position = [marker_pos.x, marker_pos.y + 0.3, marker_pos.z];
                }
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

    let walk = if map.from_map_dir { walk::WorldCollision::open(&map.world, &map.chunks) } else { None };
    run_loop(&mut engine, walk);

    engine.shutdown();
    println!("[MAIN_TEST] Goodbye!");
    Ok(())
}

/// Какие файлы карты грузить (см. шапку файла про режим папки карты).
struct MapFiles {
    world: std::path::PathBuf,
    chunks: std::path::PathBuf,
    alfars: Vec<std::path::PathBuf>,
    /// true — это настоящая папка карты, а не синтетический test.alworld
    from_map_dir: bool,
}

fn resolve_map_files() -> MapFiles {
    use std::path::{Path, PathBuf};
    // test_map.txt — рядом с запуском или рядом с самим exe
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    let from_file = [Some(PathBuf::from("test_map.txt")), exe_dir.as_ref().map(|d| d.join("test_map.txt"))]
        .into_iter()
        .flatten()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|t| t.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string));
    // exe запущен прямо из папки карты (или лежит в ней) — это и есть карта
    let here = [Some(PathBuf::from(".")), exe_dir.clone()]
        .into_iter()
        .flatten()
        .find(|d| d.join("world.alworld").is_file())
        .map(|d| d.to_string_lossy().into_owned());
    let requested = std::env::args().nth(1).or_else(|| std::env::var("ALKASH3D_MAP").ok()).or(from_file).or(here);

    if let Some(req) = requested {
        let req = PathBuf::from(req.trim_matches('"'));
        let dir = if req.is_file() { req.parent().map(Path::to_path_buf).unwrap_or_default() } else { req.clone() };
        let world = if req.is_file() {
            Some(req.clone())
        } else if dir.join("world.alworld").is_file() {
            Some(dir.join("world.alworld"))
        } else {
            list_with_ext(&dir, "alworld").into_iter().next()
        };
        match world {
            Some(world) => {
                let mut alfars = list_with_ext(&dir, "alfar");
                alfars.extend(list_with_ext(&dir.join("lights"), "alfar"));
                println!("[MAIN_TEST] Карта: {} (мир {}, свет: {} файл(ов))", dir.display(), world.display(), alfars.len());
                for a in &alfars {
                    println!("[MAIN_TEST]   свет: {}", a.display());
                }
                return MapFiles { chunks: dir.join("chunks"), world, alfars, from_map_dir: true };
            }
            None => eprintln!("[MAIN_TEST] ✗ В '{}' нет .alworld — запускаю обычный test.alworld", req.display()),
        }
    }
    let alfars = if Path::new(TEST_ALFAR_PATH).exists() { vec![PathBuf::from(TEST_ALFAR_PATH)] } else { Vec::new() };
    MapFiles { world: PathBuf::from(TEST_WORLD_PATH), chunks: PathBuf::from("chunks"), alfars, from_map_dir: false }
}

fn list_with_ext(dir: &std::path::Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case(ext)))
        .collect();
    out.sort();
    out
}

/// FirstFires DLL: рядом с запуском (как раньше), рядом с exe, или прямо из
/// сборки соседнего проекта alkash3d-FirstFires — чтобы свет работал без
/// ручного копирования DLL.
fn find_firstfires_dll() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let name = "alkash3d_firstfires.dll";
    let mut candidates = vec![PathBuf::from(FIRSTFIRES_DLL_PATH)];
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        // exe лежит в alkash3d-rust/target/<profile>/ — поднимаемся до корня репозитория
        for _ in 0..5 {
            let Some(d) = dir else { break };
            candidates.push(d.join(name));
            for profile in ["release", "debug"] {
                candidates.push(d.join("alkash3d-FirstFires").join("target").join(profile).join(name));
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    for profile in ["release", "debug"] {
        candidates.push(PathBuf::from("..").join("alkash3d-FirstFires").join("target").join(profile).join(name));
    }
    candidates.into_iter().find(|p| p.is_file())
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
fn setup_lights_from_alfar(engine: &mut AlkashEngine, alfars: &[std::path::PathBuf]) {
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
    //
    // ОБНОВЛЕНО: порог 1км больше не действует — сетка FirstFires теперь
    // едет за камерой (far_plane = её полуразмер вокруг камеры, а не вокруг
    // начала координат), а lod_distances больше не отсекают свет, только
    // метят LOD-уровень. Фонари работают по всей карте; отсекает только
    // frustum камеры (включая её дальнюю плоскость).
    let config = LightConfig {
        max_lights: 64,
        tile_size: 16,
        far_plane: 1000.0,
        lod_distances: [30.0, 60.0, 1000.0],
        grid_cell_size: 100.0,
    };

    if alfars.is_empty() {
        println!("[MAIN_TEST] .alfar не найден — сцена без света (это нормально, если ты его не экспортировал)");
        return;
    }
    let Some(dll) = find_firstfires_dll() else {
        eprintln!("[MAIN_TEST] ✗ {} не найден (ни рядом с запуском, ни в alkash3d-FirstFires/target) — свет пропущен. Собери его: cd alkash3d-FirstFires && cargo build --release", FIRSTFIRES_DLL_PATH);
        return;
    };
    let dll_str = dll.to_string_lossy().into_owned();
    if let Err(e) = engine.init_lights(&dll_str, device_ptr, config) {
        eprintln!("[MAIN_TEST] FirstFires не загружен ({}): {:?} — свет пропущен", dll_str, e);
        return;
    }
    println!("[MAIN_TEST] ✓ FirstFires: {}", dll_str);

    for alfar in alfars {
        let path = alfar.to_string_lossy();
        match engine.load_lights_from_alfar(&path) {
            Ok(count) => println!("[MAIN_TEST] ✓ Загружено {} источников света из {}", count, path),
            Err(e) => eprintln!("[MAIN_TEST] ✗ Не удалось загрузить {}: {:?}", path, e),
        }
    }
}

/// Тот же ввод (WASD/стрелки/ESC), что и в `main.rs::run_loop` — скопирован
/// намеренно один в один, чтобы поведение камеры при проверке ощущалось
/// так же, а не как отдельная, по-новому написанная реализация.
fn run_loop(engine: &mut AlkashEngine, mut collision: Option<walk::WorldCollision>) {
    let start = Instant::now();
    let mut time = 0.0f32;
    let mut frame_count = 0u32;
    let rot_speed = 2.0;
    // режим пешехода: ноги (камера на EYE_HEIGHT выше), вертикальная скорость
    let spawn_feet = engine.camera.position - Vec3::Y * EYE_HEIGHT;
    let mut feet = spawn_feet;
    let mut vel_y = 0.0f32;
    let mut on_ground = false;
    let mut fly = collision.is_none();
    if collision.is_some() {
        println!("[MAIN_TEST] Режим пешехода: гравитация + коллизия с картой (F — полёт)");
    }

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
        if collision.is_some() && engine.input.just_pressed(KEY_F) {
            fly = !fly;
            vel_y = 0.0;
            feet = engine.camera.position - Vec3::Y * EYE_HEIGHT;
            println!("[MAIN_TEST] {}", if fly { "Полёт (сквозь стены)" } else { "Пешком (гравитация + коллизия)" });
        }

        if fly || collision.is_none() {
            // SPACE — полёт над городом (5 м/с пешком по карте в десятки км — слишком медленно)
            let move_speed = if engine.input.is_down(keys::SPACE) { 60.0 } else if shift { 10.0 } else { 5.0 };
            let move_amount = move_speed * dt;

            let mut delta = Vec3::ZERO;
            if engine.input.is_down(keys::W) { delta += forward * move_amount; }
            if engine.input.is_down(keys::S) { delta -= forward * move_amount; }
            if engine.input.is_down(keys::A) { delta -= right * move_amount; }
            if engine.input.is_down(keys::D) { delta += right * move_amount; }
            if engine.input.is_down(keys::E) { delta += Vec3::Y * move_amount; }
            if engine.input.is_down(keys::Q) { delta -= Vec3::Y * move_amount; }

            engine.camera.position += delta;
            engine.camera.target += delta;
            if let Some(c) = collision.as_mut() {
                c.update(engine.camera.position);
            }
        } else if let Some(c) = collision.as_mut() {
            c.update(feet);
            let speed = if shift { walk::RUN_SPEED } else { walk::WALK_SPEED };
            let mut wish = Vec3::ZERO;
            if engine.input.is_down(keys::W) { wish += forward; }
            if engine.input.is_down(keys::S) { wish -= forward; }
            if engine.input.is_down(keys::A) { wish -= right; }
            if engine.input.is_down(keys::D) { wish += right; }
            if wish.length_squared() > 1e-6 {
                wish = wish.normalize() * speed * dt;
            }
            if on_ground && engine.input.just_pressed(keys::SPACE) {
                vel_y = walk::JUMP_SPEED;
                on_ground = false;
            }
            let (new_feet, new_vel, grounded) = c.step(feet, wish, vel_y, dt);
            feet = new_feet;
            vel_y = new_vel;
            on_ground = grounded;
            if feet.y < spawn_feet.y - 500.0 {
                println!("[MAIN_TEST] Провалился под карту — возврат на точку спавна");
                feet = spawn_feet;
                vel_y = 0.0;
            }
            let look = engine.camera.target - engine.camera.position;
            engine.camera.position = feet + Vec3::Y * EYE_HEIGHT;
            engine.camera.target = engine.camera.position + look;
        }

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

/// Коллизия камеры-пешехода с картой (режим папки карты). Геометрия — те же
/// `.altex`, что рисует движок: чанки вокруг игрока грузятся фоновым
/// потоком, треугольники раскладываются по сетке 4x4 м для быстрого поиска.
/// Пол — треугольники с нормалью круче `WALKABLE_NY` вверх (земля, дороги,
/// крыши, настил мостов); остальное — стены (дома, заборы, столбы).
mod walk {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Arc;

    use alkash3d_rs::altex_format::AltexFile;
    use alkash3d_rs::math::Vec3;
    use alkash3d_rs::{AlworldFile, ChunkContent};

    pub const WALK_SPEED: f32 = 4.0;
    pub const RUN_SPEED: f32 = 8.0;
    pub const JUMP_SPEED: f32 = 4.5;
    const GRAVITY: f32 = 9.8;
    const STEP_HEIGHT: f32 = 0.45;
    const BODY_RADIUS: f32 = 0.3;
    const WALKABLE_NY: f32 = 0.6;
    const CELL: f32 = 4.0;
    /// Сколько чанков вокруг игрока держать загруженными (1 = 3x3).
    const RADIUS_CHUNKS: i32 = 1;

    struct ChunkTris {
        tris: Vec<[Vec3; 3]>,
        normals: Vec<Vec3>,
        grid: HashMap<(i32, i32), Vec<u32>>,
    }

    pub struct WorldCollision {
        chunk_size: f32,
        existing: HashSet<(i32, i32)>,
        loaded: HashMap<(i32, i32), Arc<ChunkTris>>,
        /// чанки, файл которых не прочитался — считаются пустыми, чтобы не ждать их вечно
        failed: HashSet<(i32, i32)>,
        requested: HashSet<(i32, i32)>,
        tx: Sender<(i32, i32)>,
        rx: Receiver<((i32, i32), Option<ChunkTris>)>,
    }

    impl WorldCollision {
        pub fn open(world_path: &Path, chunks_dir: &Path) -> Option<Self> {
            let world = match AlworldFile::load(&world_path.to_string_lossy()) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("[WALK] не удалось прочитать {} для коллизии: {:?} — только полёт", world_path.display(), e);
                    return None;
                }
            };
            let existing = world.chunks.iter().map(|c| (c.grid_x, c.grid_z)).collect();
            let (tx, req_rx) = channel::<(i32, i32)>();
            let (res_tx, rx) = channel();
            let dir: PathBuf = chunks_dir.to_path_buf();
            std::thread::spawn(move || {
                for key in req_rx {
                    let tris = load_chunk(&dir, key);
                    if res_tx.send((key, tris)).is_err() {
                        break;
                    }
                }
            });
            Some(Self { chunk_size: world.header.chunk_size, existing, loaded: HashMap::new(), failed: HashSet::new(), requested: HashSet::new(), tx, rx })
        }

        fn chunk_of(&self, p: Vec3) -> (i32, i32) {
            ((p.x / self.chunk_size).floor() as i32, (p.z / self.chunk_size).floor() as i32)
        }

        /// Подгрузить/выгрузить чанки вокруг позиции и забрать готовые из фона.
        pub fn update(&mut self, p: Vec3) {
            for (key, tris) in self.rx.try_iter() {
                match tris {
                    Some(t) => {
                        self.loaded.insert(key, Arc::new(t));
                    }
                    None => {
                        eprintln!("[WALK] чанк {:?}: файл не прочитан — без коллизии", key);
                        self.failed.insert(key);
                    }
                }
            }
            let (cx, cz) = self.chunk_of(p);
            for dx in -RADIUS_CHUNKS..=RADIUS_CHUNKS {
                for dz in -RADIUS_CHUNKS..=RADIUS_CHUNKS {
                    let key = (cx + dx, cz + dz);
                    if self.existing.contains(&key) && self.requested.insert(key) {
                        let _ = self.tx.send(key);
                    }
                }
            }
            let keep = RADIUS_CHUNKS + 2;
            self.loaded.retain(|&(x, z), _| (x - cx).abs() <= keep && (z - cz).abs() <= keep);
            let loaded = &self.loaded;
            self.requested.retain(|&(x, z)| (x - cx).abs() <= keep && (z - cz).abs() <= keep || loaded.contains_key(&(x, z)));
        }

        /// Геометрия под ногами уже загружена? (пока нет — не роняем игрока в пустоту)
        fn ready_at(&self, p: Vec3) -> bool {
            let key = self.chunk_of(p);
            !self.existing.contains(&key) || self.loaded.contains_key(&key) || self.failed.contains(&key)
        }

        fn cells_around(&self, x: f32, z: f32, r: f32, mut f: impl FnMut(&ChunkTris, u32)) {
            let mut seen: HashSet<(usize, u32)> = HashSet::new();
            let (x0, x1) = (((x - r) / CELL).floor() as i32, ((x + r) / CELL).floor() as i32);
            let (z0, z1) = (((z - r) / CELL).floor() as i32, ((z + r) / CELL).floor() as i32);
            let mut chunks: Vec<&Arc<ChunkTris>> = Vec::new();
            let (ca, cb) = (((x - r) / self.chunk_size).floor() as i32, ((x + r) / self.chunk_size).floor() as i32);
            let (cc, cd) = (((z - r) / self.chunk_size).floor() as i32, ((z + r) / self.chunk_size).floor() as i32);
            for kx in ca..=cb {
                for kz in cc..=cd {
                    if let Some(c) = self.loaded.get(&(kx, kz)) {
                        chunks.push(c);
                    }
                }
            }
            for (ci, chunk) in chunks.iter().enumerate() {
                for ix in x0..=x1 {
                    for iz in z0..=z1 {
                        if let Some(list) = chunk.grid.get(&(ix, iz)) {
                            for &t in list {
                                if seen.insert((ci, t)) {
                                    f(chunk, t);
                                }
                            }
                        }
                    }
                }
            }
        }

        /// Самая высокая "пол"-поверхность в точке (x, z) не выше `max_y`.
        fn ground_at(&self, x: f32, z: f32, max_y: f32) -> Option<f32> {
            let mut best: Option<f32> = None;
            self.cells_around(x, z, 0.0, |c, t| {
                if c.normals[t as usize].y < WALKABLE_NY {
                    return;
                }
                if let Some(y) = height_in_tri(&c.tris[t as usize], x, z) {
                    if y <= max_y + 1e-3 && best.map_or(true, |b| y > b) {
                        best = Some(y);
                    }
                }
            });
            best
        }

        /// Выталкивание тела (две сферы: у колен и у груди) из стен по горизонтали.
        fn push_out(&self, feet: Vec3) -> Vec3 {
            let mut p = feet;
            for _ in 0..3 {
                let mut shift = Vec3::ZERO;
                for h in [STEP_HEIGHT + BODY_RADIUS + 0.05, 1.3] {
                    let c = p + Vec3::Y * h;
                    self.cells_around(c.x, c.z, BODY_RADIUS, |ch, t| {
                        if ch.normals[t as usize].y >= WALKABLE_NY {
                            return;
                        }
                        let q = closest_point_on_tri(c, &ch.tris[t as usize]);
                        let mut d = c - q;
                        d.y = 0.0;
                        let dist = d.length();
                        if dist < BODY_RADIUS && (q.y - c.y).abs() < BODY_RADIUS {
                            let dir = if dist > 1e-5 { d / dist } else {
                                let n = ch.normals[t as usize];
                                Vec3::new(n.x, 0.0, n.z).normalize_or_zero()
                            };
                            let push = dir * (BODY_RADIUS - dist);
                            if push.length_squared() > shift.length_squared() {
                                shift = push;
                            }
                        }
                    });
                }
                if shift.length_squared() < 1e-8 {
                    break;
                }
                p += shift;
            }
            p
        }

        /// Один шаг пешехода: горизонтальное перемещение со стенами, гравитация, пол.
        pub fn step(&self, feet: Vec3, wish: Vec3, vel_y: f32, dt: f32) -> (Vec3, f32, bool) {
            if !self.ready_at(feet) {
                return (feet, 0.0, true); // чанк под ногами ещё грузится — ждём на месте
            }
            let mut p = self.push_out(feet + Vec3::new(wish.x, 0.0, wish.z));
            let vel_y = vel_y - GRAVITY * dt;
            let next_y = p.y + vel_y * dt;
            match self.ground_at(p.x, p.z, p.y + STEP_HEIGHT) {
                Some(g) if next_y <= g => {
                    p.y = g;
                    (p, 0.0, true)
                }
                _ => {
                    p.y = next_y;
                    (p, vel_y, false)
                }
            }
        }
    }

    fn load_chunk(dir: &Path, (gx, gz): (i32, i32)) -> Option<ChunkTris> {
        let path = dir.join(format!("chunk_{}_0_{}.alwchunk", gx, gz));
        let content = ChunkContent::load_from_file(&path.to_string_lossy()).ok()?;
        let mut tris = Vec::new();
        for obj in &content.objects {
            let altex_path = content.get_string(obj.altex_path_string_id).to_string();
            if altex_path.is_empty() || altex_path == "placeholder" {
                continue;
            }
            let Ok(altex) = AltexFile::load(&altex_path) else { continue };
            // движок ставит объект чанка только переносом (m[12..14]) — так же и здесь
            let off = Vec3::new(obj.transform[12], obj.transform[13], obj.transform[14]);
            for m in &altex.meshes {
                let (i0, n) = (m.index_offset as usize, m.index_count as usize);
                let Some(idx) = altex.indices.get(i0..i0 + n) else { continue };
                for t in idx.chunks_exact(3) {
                    let v = |i: u32| altex.vertices.get(i as usize).map(|v| Vec3::from(v.position) + off);
                    if let (Some(a), Some(b), Some(c)) = (v(t[0]), v(t[1]), v(t[2])) {
                        tris.push([a, b, c]);
                    }
                }
            }
        }
        let mut normals = Vec::with_capacity(tris.len());
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, [a, b, c]) in tris.iter().enumerate() {
            let mut n = (*b - *a).cross(*c - *a).normalize_or_zero();
            if n.y < 0.0 {
                n = -n; // ориентация обхода не важна: пол/стена по модулю наклона
            }
            normals.push(n);
            let (x0, x1) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
            let (z0, z1) = (a.z.min(b.z).min(c.z), a.z.max(b.z).max(c.z));
            for ix in (x0 / CELL).floor() as i32..=(x1 / CELL).floor() as i32 {
                for iz in (z0 / CELL).floor() as i32..=(z1 / CELL).floor() as i32 {
                    grid.entry((ix, iz)).or_default().push(i as u32);
                }
            }
        }
        Some(ChunkTris { tris, normals, grid })
    }

    /// Высота плоскости треугольника в (x, z), если точка внутри его проекции на XZ.
    fn height_in_tri(t: &[Vec3; 3], x: f32, z: f32) -> Option<f32> {
        let [a, b, c] = *t;
        let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
        if d.abs() < 1e-9 {
            return None;
        }
        let w0 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / d;
        let w1 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / d;
        let w2 = 1.0 - w0 - w1;
        let eps = -1e-4;
        (w0 >= eps && w1 >= eps && w2 >= eps).then(|| w0 * a.y + w1 * b.y + w2 * c.y)
    }

    /// Ближайшая точка треугольника к точке p (Ericson, Real-Time Collision Detection 5.1.5).
    fn closest_point_on_tri(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
        let [a, b, c] = *t;
        let (ab, ac, ap) = (b - a, c - a, p - a);
        let (d1, d2) = (ab.dot(ap), ac.dot(ap));
        if d1 <= 0.0 && d2 <= 0.0 { return a; }
        let bp = p - b;
        let (d3, d4) = (ab.dot(bp), ac.dot(bp));
        if d3 >= 0.0 && d4 <= d3 { return b; }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 { return a + ab * (d1 / (d1 - d3)); }
        let cp = p - c;
        let (d5, d6) = (ab.dot(cp), ac.dot(cp));
        if d6 >= 0.0 && d5 <= d6 { return c; }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 { return a + ac * (d2 / (d2 - d6)); }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
        }
        let denom = 1.0 / (va + vb + vc);
        a + ab * (vb * denom) + ac * (vc * denom)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn height_and_closest_point() {
            let t = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(10.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 10.0)];
            assert_eq!(height_in_tri(&t, 2.0, 2.0), Some(1.0));
            assert_eq!(height_in_tri(&t, 9.0, 9.0), None);
            let wall = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 0.0, 5.0)];
            let q = closest_point_on_tri(Vec3::new(0.2, 1.0, 1.0), &wall);
            assert!((q - Vec3::new(0.0, 1.0, 1.0)).length() < 1e-5);
        }

        /// На реальной экспортированной карте (папка из ALKASH3D_MAP, без
        /// окна/GPU): пешеход с точки спавна встаёт на землю и может пройти
        /// квартал, ни разу не провалившись. `cargo test --bin main_test -- --ignored --nocapture`
        #[test]
        #[ignore]
        fn walks_on_real_map() {
            let Ok(dir) = std::env::var("ALKASH3D_MAP") else { return };
            let dir = PathBuf::from(dir);
            let world = AlworldFile::load(&dir.join("world.alworld").to_string_lossy()).unwrap();
            let spawn = world.global_objects.iter().find(|g| g.flags & alkash3d_rs::GLOBAL_OBJECT_FLAG_SPAWN_POINT != 0).unwrap();
            let mut feet = Vec3::new(spawn.transform[12], spawn.transform[13], spawn.transform[14]);
            let mut w = WorldCollision::open(&dir.join("world.alworld"), &dir.join("chunks")).unwrap();
            let t0 = std::time::Instant::now();
            while !w.ready_at(feet) && t0.elapsed().as_secs() < 60 {
                w.update(feet);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            println!("чанки у спавна загружены за {:.1} c, треугольников: {}", t0.elapsed().as_secs_f32(),
                w.loaded.values().map(|c| c.tris.len()).sum::<usize>());
            let dt = 1.0 / 60.0;
            let mut vy = 0.0;
            let mut grounded = false;
            for _ in 0..180 {
                w.update(feet);
                let r = w.step(feet, Vec3::ZERO, vy, dt);
                feet = r.0; vy = r.1; grounded = r.2;
            }
            println!("стоит на земле: {} на высоте {:.2} (спавн {:.2})", grounded, feet.y, spawn.transform[13]);
            assert!(grounded, "пешеход не встал на землю");
            assert!(spawn.transform[13] - feet.y < 3.0, "упал слишком далеко от спавна");
            // идём в 8 разных сторон по 60 с — не проваливаемся, где-то упрёмся в стену
            let start = feet;
            let mut min_y = feet.y;
            let mut blocked = 0;
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let dir = Vec3::new(a.cos(), 0.0, a.sin());
                feet = start;
                vy = 0.0;
                let mut moved = 0.0;
                for _ in 0..(60 * 60) {
                    w.update(feet);
                    while !w.ready_at(feet) {
                        w.update(feet);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    let before = feet;
                    let r = w.step(feet, dir * WALK_SPEED * dt, vy, dt);
                    feet = r.0; vy = r.1;
                    moved += Vec3::new(feet.x - before.x, 0.0, feet.z - before.z).length();
                    min_y = min_y.min(feet.y);
                }
                let straight = Vec3::new(feet.x - start.x, 0.0, feet.z - start.z).length();
                if straight < 60.0 * WALK_SPEED * 0.9 {
                    blocked += 1;
                }
                println!("направление {}: прошёл {:.0} м (по прямой {:.0} м), высота {:.1}", k, moved, straight, feet.y);
            }
            assert!(min_y > spawn.transform[13] - 60.0, "провалился под карту: {}", min_y);
            println!("упёрлись во что-то в {} направлениях из 8", blocked);
        }
    }
}
