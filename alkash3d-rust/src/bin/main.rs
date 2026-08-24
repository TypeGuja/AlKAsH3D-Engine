// src/bin/main.rs
//! Alkash3D Engine — демонстрация: плоскость из множества кубов-плиток,
//! по которой можно ходить (не летать), освещённая ночным рядом уличных
//! фонарей.
//!
//! Показывает:
//!  - ECS (`engine.scene`) для сотни+ объектов сетки пола (см. scene.rs);
//!  - новую систему ввода (`engine.input`, см. input.rs) вместо
//!    GetAsyncKeyState — движок сам ничего не решает про WASD/ESC, только
//!    отдаёт состояние клавиш, а что с ним делать, решает этот файл;
//!  - "ходьбу": движение заперто в горизонтальной плоскости (не зависит от
//!    того, куда камера смотрит по вертикали), высота глаз фиксирована;
//!  - ОБНОВЛЕНО (после Фаз 0-5 плана по реализму/фонарям): реальные
//!    фонари FirstFires + .alfar, вместо светлого дневного неба без единого
//!    источника света. Раньше ни один demo-бинарник вообще не вызывал
//!    `init_lights`/`load_lights_from_alfar` — весь конвейер HDR/bloom/
//!    tonemap/spot-конусов/пространственной сетки каллинга, реализованный
//!    в движке, оставался невидимым (сцена рисовалась плоским
//!    ambient+directional светом с захардкоженными значениями по
//!    умолчанию, GPULight-буфер был всегда пуст). Теперь сцена — ночная
//!    улица с 20 мерцающими фонарями (`AlfarFile::create_night_city()`),
//!    ambient — тёмно-синий (ночное небо), а не светлый день.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
// ВАЖНО: `alfar_format`/`plugin` — ПРИВАТНЫЕ модули в lib.rs (`mod
// alfar_format;`, `mod plugin;`, без `pub`) — их содержимое доступно
// внешним крейтам (в т.ч. этому bin-файлу) ТОЛЬКО через реэкспорт с корня
// крейта (`pub use alfar_format::*;` / `pub use plugin::*;` в lib.rs), не
// через полный путь модуля. `use alkash3d_rs::alfar_format::AlfarFile`
// был бы E0603 ("module `alfar_format` is private").
use alkash3d_rs::AlfarFile;
use alkash3d_rs::LightConfig;
// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): `PhysicsConfig` — тот
// же класс реэкспорта, что и `LightConfig` выше (см. комментарий над
// `AlfarFile` про приватность модуля `plugin`).
use alkash3d_rs::PhysicsConfig;
use std::time::Instant;
use windows::core::Interface;

const WINDOW_WIDTH: u32 = 1366;
const WINDOW_HEIGHT: u32 = 768;

/// ИСПРАВЛЕНО (баг: "свет не на столбах, а на половине плоскости" +
/// "фонари резко гаснут/загораются при ходьбе"): раньше пол был крошечным
/// квадратом ±6м от центра, а 20 уличных фонарей из `create_night_city()`
/// стоят В РЯД вдоль оси X от -50 до +45 (шаг 5м) на z=0 — то есть пол
/// физически умещался под 1-3 ближайшими фонарями из целого ряда, отсюда
/// и ощущение "свет только на половине" (пол был меньше, чем область
/// действия даже одной лампы), и резкие перепады яркости при ходьбе
/// (соседние лампы то попадали, то не попадали в радиус действия/culling
/// от текущей позиции камеры — см. `first_fires.rs::cull`, distance >
/// light.range * 1.2). Теперь пол — вытянутая "улица" вдоль X ТЕХ ЖЕ
/// размеров, что и ряд фонарей (GRID_HALF_X=52 при TILE_SPACING=1 — с
/// небольшим запасом за крайние фонари на x=±47.5), узкая по Z (сама
/// "улица", а не квадратная площадь) — так под каждым фонарём реально
/// есть пол, и при ходьбе вдоль улицы всегда видно НЕСКОЛЬКО соседних
/// фонарей одновременно, без резких переключений.
const GRID_HALF_X: i32 = 52;
const GRID_HALF_Z: i32 = 4;
const TILE_SPACING: f32 = 1.0;
const TILE_HEIGHT: f32 = 0.2;
const GROUND_Y: f32 = 0.0;
const EYE_HEIGHT: f32 = 1.6;

/// Те же параметры ряда фонарей, что жёстко закодированы в
/// `AlfarFile::create_night_city()` (alfar_format.rs: `let x = (i as f32
/// - 10.0) * 5.0;`, `position: [x, 3.0, 0.0]`, `for i in 0..20`) —
/// вынесены сюда КАК КОНСТАНТЫ ЭТОГО ФАЙЛА, а не считаны из AlfarFile
/// программно, потому что create_night_city() не выставляет наружу свои
/// параметры (нет публичных геттеров позиций фонарей) — дублирование
/// именно этих трёх чисел единственный практичный способ разместить
/// столбы РОВНО там же, где реально стоят фонари, без изменения формата
/// .alfar. Если параметры create_night_city() когда-нибудь изменятся,
/// эти константы нужно обновить синхронно.
const STREET_LIGHT_COUNT: i32 = 20;
const STREET_LIGHT_SPACING: f32 = 5.0;
const STREET_LIGHT_X_OFFSET: f32 = -10.0; // x = (i - 10.0) * 5.0

/// ДОБАВЛЕНО: путь к плагину FirstFires (см. `AlkashEngine::init_lights`).
/// FirstFires — отдельный крейт `alkash3d-firstfires` (Cargo заменяет
/// дефис на подчёркивание в имени артефакта), собирается ОТДЕЛЬНО от
/// `alkash3d-rust` в СОСЕДНЕЙ папке репозитория (обе — подпапки одного
/// корня GitHub): `alkash3d-FirstFires/target/release/alkash3d_firstfires.dll`.
/// Путь относительный — верен при запуске через `cargo run`/`cargo run
/// --release` ИЗНУТРИ папки `alkash3d-rust` (её Cargo задаёт эту папку
/// как текущую рабочую директорию процесса), что и есть обычный способ
/// запуска. Если файла нет (FirstFires не собран или собран в debug, а не
/// release) — `init_lights` вернёт понятную ошибку, а не тихо промолчит.
///
/// ВАЖНО: НЕ переименовывай в "firstfires.dll" — рядом в
/// alkash3d-FirstFires/target/release/ лежит одноимённый файл-путаница
/// `firstfires.dll` (без подчёркивания), оставшийся от старой/другой
/// системы сборки — он НЕ экспортирует `get_plugin_api` в ожидаемом виде
/// (при попытке его загрузить engine.init_lights() вернёт "No light API").
/// Именно `alkash3d_firstfires.dll` (с подчёркиванием) — настоящий,
/// актуальный артефакт крейта `alkash3d-firstfires`.
const FIRSTFIRES_DLL_PATH: &str = "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll";

/// ДОБАВЛЕНО: путь, по которому сохраняется и откуда затем читается
/// сгенерированная ночная сцена — через реальный файл на диске, а не
/// напрямую в памяти, чтобы демонстрировать полный путь данных
/// `.alfar` (create_night_city -> save -> load_lights_from_alfar), тот
/// же самый, каким будет пользоваться редактор карт в будущем.
const NIGHT_CITY_ALFAR_PATH: &str = "night_city_demo.alfar";

/// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): путь к плагину
/// Inertial — отдельный крейт `alkash3d-inertial` в СОСЕДНЕЙ папке
/// репозитория, СОБСТВЕННОЕ имя пакета/артефакта внутри — просто
/// `inertial` (см. `[package] name = "inertial"` / `[lib] name =
/// "inertial"` в его Cargo.toml — НЕ "alkash3d-inertial", в отличие от
/// FirstFires, у которого имя пакета совпадает с именем папки), поэтому
/// артефакт называется РОВНО "inertial.dll", без каких-либо подчёркиваний.
///
/// ВАЖНО: этот плагин собирается Fortran-ядрами (.f90) через gfortran и
/// ОБЯЗАН собираться под GNU-таргет (см. его README.md/.cargo/config.toml
/// — `x86_64-pc-windows-gnu`, а не обычный MSVC-таргет этого движка) —
/// поэтому итоговый .dll лежит в `target/x86_64-pc-windows-gnu/release/`,
/// а НЕ в `target/release/` напрямую, как у FirstFires (тот собирается
/// обычным MSVC-таргетом). Загрузка в движок всё равно происходит через
/// `LoadLibrary`/`libloading` в рантайме — GNU-DLL и MSVC-.exe совместимы
/// на уровне `extern "C"` ABI, поэтому смешение таргетов между плагином и
/// движком не проблема.
const INERTIAL_DLL_PATH: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";

/// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): директория, куда
/// сохраняется демонстрационный `.alsnd` банк вместе со сгенерированными
/// `.wav` файлами — та же идея, что и `NIGHT_CITY_ALFAR_PATH` у света
/// (create -> save -> load через реальный файл на диске, не в памяти),
/// применённая к звуку.
const SOUND_DEMO_DIR: &str = "sound_demo";

/// ДОБАВЛЕНО (скриптинг — Python hot-reload, первое реальное подключение
/// в main): путь к эталонному Python-скрипту "bobber" — переиспользует
/// УЖЕ существующий `examples_scripts/bobber.py` (тот же контракт
/// update/on_event, что задокументирован и здесь, и в bobber.lua) вместо
/// создания нового дублирующего файла. Относительный путь верен при
/// запуске через `cargo run`/`cargo run --release` ИЗНУТРИ папки
/// alkash3d-rust, как и остальные пути выше (FIRSTFIRES_DLL_PATH и т.п.).
/// В отличие от Native/Lua, здесь НЕТ пути к .dll вообще — Python
/// исполняется встроенным интерпретатором прямо в движке (см.
/// engine::scripting_python), .py-файл читается движком напрямую.
const PYTHON_BOBBER_SCRIPT_PATH: &str = "examples_scripts/bobber.py";

/// ДОБАВЛЕНО (скриптинг — Native/Rust DLL-плагин, проверка вживую): путь
/// к собранному `alkash3d-examplescript` (см. cargo build --release в
/// той папке) — та же схема относительных путей, что и у
/// FIRSTFIRES_DLL_PATH/INERTIAL_DLL_PATH выше (СОСЕДНЯЯ папка репозитория
/// относительно alkash3d-rust). Если DLL не собрана — `load_native_script`
/// вернёт понятную ошибку, а не тихо промолчит (см. setup_scripting).
const NATIVE_EXAMPLE_DLL_PATH: &str = "../alkash3d-examplescript/target/release/alkash3d_examplescript.dll";

/// ДОБАВЛЕНО (скриптинг — Lua DLL-плагин, проверка вживую): путь к
/// собранному УНИВЕРСАЛЬНОМУ Lua-рантайм-плагину `alkash3d-luascript` —
/// одна и та же DLL грузит ЛЮБОЙ .lua-файл текстом (см.
/// LUA_BOBBER_SCRIPT_PATH ниже), поэтому здесь только путь к самой DLL,
/// а не к конкретному скрипту.
const LUA_RUNTIME_DLL_PATH: &str = "../alkash3d-luascript/target/release/alkash3d_luascript.dll";

/// Путь к .lua-исходнику — уже существующий эталонный пример (см.
/// alkash3d-luascript/examples/bobber.lua), тот же демо-сценарий, что и
/// у bobber.py.
const LUA_BOBBER_SCRIPT_PATH: &str = "../alkash3d-luascript/examples/bobber.lua";

// ВЫРЕЗАНО (по прямой просьбе пользователя — "вырежи C# из скриптинга,
// будет жить на том что есть"): C# как язык скриптинга удалён из движка
// целиком (alkash3d-csscript/alkash3d-csscript-managed убраны из
// репозитория) — не требовал бы .NET SDK для сборки/запуска. Скриптинг
// движка живёт на трёх языках: Python (hot-reload), Lua (DLL), Native/
// C++/Rust (DLL).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{}", alkash3d_rs::VERSION);
    println!("==========================================");
    println!();
    println!("🎮 УПРАВЛЕНИЕ:");
    println!("  WASD    - ходьба (только по горизонтали)");
    println!("  Стрелки - осмотреться (влево/вправо/вверх/вниз)");
    println!("  SHIFT   - ускорение (x2)");
    println!("  ESC     - выход");
    println!("==========================================");

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);

    if let Err(e) = engine.init() {
        eprintln!("[MAIN] Failed to initialize engine: {:?}", e);
        return Err(e.into());
    }

    setup_lights(&mut engine);
    setup_scene(&mut engine);
    let physics_debug_entities = setup_physics(&mut engine);
    setup_world_streaming(&mut engine);
    setup_audio(&mut engine);
    let scripting_handles = setup_scripting(&mut engine);
    run_loop(&mut engine, scripting_handles, physics_debug_entities);

    engine.shutdown();
    println!("[MAIN] Goodbye!");
    Ok(())
}

/// ДОБАВЛЕНО: подключает FirstFires (плагин каллинга фонарей) и
/// загружает в него ночную уличную сцену — ДОЛЖНО вызываться ДО
/// setup_scene (не строго обязательно по коду, но по смыслу: сначала
/// свет, потом геометрия, которую он будет освещать) и ПОСЛЕ engine.init()
/// (плагину нужен реальный D3D12-device pointer, которого до init() ещё
/// не существует).
fn setup_lights(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Loading FirstFires light plugin...");

    // Реальный указатель на D3D12-устройство движка — FirstFires его не
    // использует напрямую под рендер (весь рендеринг остаётся на стороне
    // alkash3d_rs), но плагинный ABI принимает его как часть контракта
    // (см. LightAPI::load в plugin/light_api.rs) на случай будущих
    // плагинов, которым понадобится GPU-доступ (например для compute-
    // culling на самой GPU, а не на CPU, как сейчас).
    let device_ptr = match alkash3d_rs::get_device() {
        Ok(device) => device.as_raw(),
        Err(e) => {
            eprintln!("[MAIN] WARNING: не удалось получить D3D12 device для FirstFires: {:?} — фонари не будут загружены, сцена останется тёмной", e);
            return;
        }
    };

    // ИСПРАВЛЕНО (просадка FPS до ~20 после range 15->100, обнаружено
    // пользователем на реальной сборке): при far_plane=100/grid_cell_size=10
    // (старые значения) мир — куб [-100,100]^3 (сторона 200), а сфера
    // влияния фонаря с range=100 имеет ДИАМЕТР 200 — то есть РАВНА самому
    // миру по размеру. Каждый из 20 фонарей регистрировался буквально во
    // ВСЕ 8000 ячеек сетки сразу (см. get_cell_indices_for_sphere в
    // alkash3d-FirstFires/src/lib.rs) — пространственная сетка каллинга
    // переставала хоть что-то фильтровать, и пиксельный шейдер
    // (ComputePointLightContribution, engine/mod.rs) перебирал все 20
    // фонарей на КАЖДЫЙ пиксель КАЖДОЙ ячейки, а не 1-3 ближайших, как
    // задумано архитектурой сетки — отсюда и просадка.
    //
    // Фикс — не уменьшать range обратно (это вернуло бы уже исправленный
    // баг с обрывом света), а увеличить far_plane (размер мира) ВМЕСТЕ с
    // grid_cell_size, чтобы диаметр сферы фонаря (200) стал заметно
    // МЕНЬШЕ размера мира. far_plane=200 -> мир [-200,200]^3 (сторона
    // 400), grid_cell_size=20 -> сетка снова 20x20x20=8000 ячеек (ТА ЖЕ
    // память, что и раньше) — но теперь сфера фонаря покрывает ~17%
    // сетки вместо 100%. lod_distances максимум поднят с 100 до 200
    // синхронно с far_plane — иначе LOD-каллинг обрезал бы фонарей на
    // дистанции 100 ещё до того, как размер мира вообще стал бы иметь
    // значение.
    let config = LightConfig {
        max_lights: 64,
        tile_size: 16,
        far_plane: 200.0,
        lod_distances: [30.0, 60.0, 200.0],
        grid_cell_size: 20.0,
    };

    if let Err(e) = engine.init_lights(FIRSTFIRES_DLL_PATH, device_ptr, config) {
        eprintln!(
            "[MAIN] WARNING: не удалось загрузить FirstFires ({}): {:?} — сцена останется без фонарей. \
             Проверь, что alkash3d-FirstFires собран в release (cargo build --release из папки alkash3d-FirstFires).",
            FIRSTFIRES_DLL_PATH, e
        );
        return;
    }
    println!("[MAIN] ✓ FirstFires loaded");

    // Генерируем ночную сцену (20 мерцающих уличных фонарей вдоль улицы,
    // тёмный синий ambient, exposure/bloom уже настроены под ночь) и
    // сохраняем на диск — так демонстрируется ПОЛНЫЙ путь данных
    // .alfar (create -> save -> load), а не прямой обход файловой системы.
    let night_city = AlfarFile::create_night_city();
    if let Err(e) = night_city.save(NIGHT_CITY_ALFAR_PATH) {
        eprintln!("[MAIN] WARNING: не удалось сохранить {}: {:?} — фонари не будут загружены", NIGHT_CITY_ALFAR_PATH, e);
        return;
    }

    match engine.load_lights_from_alfar(NIGHT_CITY_ALFAR_PATH) {
        Ok(count) => println!("[MAIN] ✓ Загружено {} фонарей из {}", count, NIGHT_CITY_ALFAR_PATH),
        Err(e) => eprintln!("[MAIN] WARNING: не удалось загрузить {}: {:?}", NIGHT_CITY_ALFAR_PATH, e),
    }

    // ИСПРАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь, баг):
    // `AlkashEngine::time_of_day` по умолчанию 12.0 (полдень, см.
    // AlkashEngine::new) — `update_day_night` (вызывается КАЖДЫМ кадром
    // из engine.update() в run_loop) каждый кадр ПЕРЕЗАПИСЫВАЕТ
    // transform_constants.light_dir/light_color/ambient_color ярким
    // полуденным солнцем, полностью поверх тёмного ночного ambient и
    // отсутствия солнца, которые задавала загруженная выше night_city
    // сцена (AmbientLight/GlobalLightSettings из .alfar влияют на
    // ДРУГИЕ поля, не на day/night солнце — Фаза 7 добавлена ПОСЛЕ Фазы
    // 1 и ничего не знала об этой демо-сцене). Результат — весь экран
    // залит ярким прямым+ambient светом полудня, а маленькие мерцающие
    // уличные фонари теряются на этом фоне почти незаметно. Фиксируем
    // время суток на ночь (22:00, уже после заката в 18:00, см.
    // compute_sun_state) — солнца нет, только холодный лунный ambient,
    // и фонари наконец видны так, как задумано сценой.
    engine.set_time_of_day(22.0);
}

fn setup_scene(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up scene...");

    // ===== ПЛОСКОСТЬ ИЗ МНОЖЕСТВА КУБОВ (не один большой quad) =====
    // Специально сделано именно кубами через ECS, а не одним Mesh::quad,
    // чтобы: (1) показать, что ECS нормально тянет сотни объектов, и
    // (2) чтобы пол реально состоял из отдельных "плиток" — как в
    // Minecraft-подобных играх, а не был одной цельной плоскостью.
    //
    // ИСПРАВЛЕНО (баг: "пол выглядит одинаково залитым синим независимо
    // от близости к фонарю" — фонари как будто не освещают поверхность):
    // `add_cube()` красит каждую грань куба в свой ОТЛАДОЧНЫЙ цвет (top
    // face = чистый синий (0,0,1), см. подробный комментарий у
    // `Mesh::cube_colored` в engine/mod.rs). Пиксельный шейдер умножает
    // вершинный цвет на итоговую освещённость (`input.color.rgb *
    // brightness`), поэтому синий top face просто ОБНУЛЯЛ R/G каналы
    // тёплого света фонарей — сам расчёт освещения (attenuation/
    // culling/сетка) при этом работал правильно, проблема была ЧИСТО в
    // выборе тестовой геометрии. `add_cube_colored` с нейтральным
    // светло-серым (0.75,0.75,0.75) — теперь видно РЕАЛЬНЫЙ эффект
    // освещения: тёплый круг под каждым фонарём, темнее между ними.
    let tile_mesh = engine.add_cube_colored(0.95, 0.75, 0.75, 0.75, 1.0); // чуть меньше TILE_SPACING — видны швы между плитками
    let mut tile_count = 0;
    for gx in -GRID_HALF_X..=GRID_HALF_X {
        for gz in -GRID_HALF_Z..=GRID_HALF_Z {
            let tile = engine.spawn_mesh_entity(tile_mesh);
            if let Some(t) = engine.scene.transform_mut(tile) {
                t.position = [gx as f32 * TILE_SPACING, GROUND_Y, gz as f32 * TILE_SPACING];
                t.scale = [1.0, TILE_HEIGHT, 1.0]; // приплюснутый куб — плитка пола
            }
            tile_count += 1;
        }
    }

    // ИСПРАВЛЕНО (баг: "свет находится не на столбах, а на половине
    // плоскости"): раньше столбы стояли по 4 углам маленького квадратного
    // пола, а фонари — отдельным рядом вдоль X, никак с этими углами не
    // связанным (два независимых набора координат). Теперь столб стоит
    // РОВНО в той же точке XZ, где `create_night_city()` реально
    // размещает каждый из 20 фонарей — визуально "свет на столбах" стало
    // буквальной правдой, а не совпадением. Столб чуть тоньше и короче,
    // чем раньше (это не декоративный ориентир по краю площади, а
    // "столб фонаря", который своей геометрией не должен закрывать сам
    // источник света на высоте y=3.0 — см. position.y фонаря в
    // create_night_city()).
    let pillar_mesh = engine.add_cube_colored(0.4, 0.6, 0.6, 0.62, 1.0); // нейтральный серый — та же причина, см. комментарий у tile_mesh выше
    let mut pillar_count = 0;
    for i in 0..STREET_LIGHT_COUNT {
        let light_x = (i as f32 + STREET_LIGHT_X_OFFSET) * STREET_LIGHT_SPACING;
        let pillar = engine.spawn_mesh_entity(pillar_mesh);
        if let Some(t) = engine.scene.transform_mut(pillar) {
            // Столб тянется от земли (GROUND_Y) до чуть выше фонаря
            // (фонарь на y=3.0) — центр приплюснутого по XZ, вытянутого
            // по Y куба на полпути.
            t.position = [light_x, GROUND_Y + 1.6, 0.0];
            t.scale = [1.0, 3.2, 1.0];
        }
        pillar_count += 1;
    }

    // ИЗМЕНЕНО: было светлое дневное небо (0.55, 0.7, 0.9) — теперь тёмная
    // ночь, соответствующая ambient-цвету загруженной ночной сцены
    // (AmbientLight.color = [0.05, 0.05, 0.1] в create_night_city). Если
    // светлый clear_color оставить, а фонари погасить (например DLL не
    // нашёлся), результат выглядел бы как "дневная сцена без единой
    // тени" — что маскирует именно ту проблему, которую эта правка
    // призвана сделать видимой.
    engine.set_clear_color(0.02, 0.02, 0.04, 1.0);

    println!(
        "✅ Scene ready: {} плиток пола + {} столбов-фонарей ({} ECS-сущностей всего)",
        tile_count,
        pillar_count,
        engine.scene.len()
    );
}

/// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): подключает Inertial
/// (физический плагин) и наполняет сцену несколькими падающими сферами
/// над полом — простейшая, но реальная демонстрация всего конвейера
/// (интегрирование + широкая/узкая фаза + solver + sleep), а не просто
/// "плагин загрузился и ничего не делает". Пол демо-сцены — плитки на
/// GROUND_Y=0.0 (см. `setup_scene`) — физический пол под ним СОБИРАЕТСЯ
/// из ряда статических сфер (см. FLOOR_SPHERE_SPACING ниже — у Inertial
/// нет коллайдера-плоскости, только sphere-sphere), чтобы падающие сферы
/// приземлялись именно на видимый пол, а не проваливались сквозь него.
fn setup_physics(engine: &mut AlkashEngine) -> Vec<alkash3d_rs::scene::EntityId> {
    println!("\n[MAIN] Loading Inertial physics plugin...");

    // world_size/cell_size — тот же порядок величин, что far_plane/
    // grid_cell_size у LightConfig в setup_lights: сцена умещается в
    // куб [-100,100]^3, широкая фаза физики использует сетку с ячейкой
    // 4м (несколько сфер радиусом 0.5м на ячейку — разумный компромисс
    // для десятков тел на "минимальном железе 10-летней давности" из ТЗ).
    let config = PhysicsConfig {
        max_bodies: 256,
        world_size: 100.0,
        cell_size: 4.0,
        solver_iterations: 8,
        use_simd: 0,
    };

    if let Err(e) = engine.init_physics(INERTIAL_DLL_PATH, config) {
        eprintln!(
            "[MAIN] WARNING: не удалось загрузить Inertial ({}): {:?} — сцена останется без физики. \
             Проверь, что alkash3d-inertial собран в release (cargo build --release из папки alkash3d-inertial).",
            INERTIAL_DLL_PATH, e
        );
        return Vec::new();
    }
    println!("[MAIN] ✓ Inertial loaded");

    // ИСПРАВЛЕНО (баг: "все кубы вне дороги упали" — воспроизведён
    // пользователем ПОСЛЕ того, как Inertial реально собрался и физика
    // заработала первый раз): у Inertial narrow-phase — ЧЕСТНЫЙ
    // sphere-sphere тест (см. `narrow_phase_gjk` в alkash3d-inertial), у
    // `PhysicsBody` НЕТ поля `radius` вообще — каждое тело (в т.ч.
    // "статический пол") трактуется как сфера ФИКСИРОВАННОГО радиуса
    // `IMPLICIT_RADIUS = 0.5` (см. alkash3d-inertial/src/lib.rs). Один
    // `add_sphere_body(0.0, GROUND_Y, 0.0, 0.0)` — это НЕ плоскость-пол,
    // а один статический шарик диаметром 1м РОВНО в точке (0,0,0). Пока
    // Inertial был не собран (init_physics падал с WARNING), это не было
    // заметно — Transform падающих сфер просто не двигался вообще. Как
    // только физика реально заработала, гравитация потянула вниз все 5
    // сфер (стартующих на x = -8.0..-2.0, см. цикл ниже) — ни одна из них
    // не оказывается рядом с единственной точкой-полом на x=0, и все
    // проваливаются в бесконечность.
    //
    // Фикс — вместо одной точечной сферы-пола кладём РЯД статических
    // сфер вдоль всей ширины улицы (тот же диапазон X, что и у пола из
    // плиток, GRID_HALF_X=52, см. setup_scene), с шагом чуть меньше
    // диаметра сферы (IMPLICIT_RADIUS*2=1.0), чтобы соседние сферы-пол
    // перекрывались и не оставляли "щелей", в которые падающее тело
    // могло бы провалиться между двумя опорными точками.
    const FLOOR_SPHERE_SPACING: f32 = 0.9; // < 2*IMPLICIT_RADIUS (1.0) — с нахлёстом
    let floor_half_x = GRID_HALF_X as f32 * TILE_SPACING;
    let floor_sphere_count = (2.0 * floor_half_x / FLOOR_SPHERE_SPACING) as i32;
    let mut floor_spheres_created = 0;
    for i in 0..=floor_sphere_count {
        let x = -floor_half_x + i as f32 * FLOOR_SPHERE_SPACING;
        // `mass<=0.0` внутри `add_sphere_body` сам выставляет
        // `is_static=1` (не "невесомое тело", а явный маркер статики) —
        // z=1.5 совпадает с z падающих сфер ниже (см. цикл
        // spawn_physics_sphere), чтобы опора была ровно под ними, а не
        // сбоку.
        if engine.add_sphere_body(x, GROUND_Y, 1.5, 0.0).is_some() {
            floor_spheres_created += 1;
        }
    }
    if floor_spheres_created == 0 {
        eprintln!("[MAIN] WARNING: не удалось создать физический пол — сферы будут падать бесконечно");
    } else {
        println!("[MAIN] ✓ Физический пол: {} опорных сфер вдоль улицы", floor_spheres_created);
    }

    // Несколько падающих сфер над улицей, в зоне, где камера гарантированно
    // их увидит при старте (см. run_loop: старт камеры x=-8.0, z=1.0,
    // смотрит в сторону x=0.0).
    let sphere_mesh = engine.add_cube_colored(0.7, 0.9, 0.35, 0.25, 1.0); // тёплый зелёный — заметно отличается от серого пола/столбов
    let mut spawned = 0;
    // ДОБАВЛЕНО (диагностика бага "падают и пропадают под картой" — нужны
    // точные позиции по кадрам, а не гадание по коду): собираем EntityId
    // каждой заспавненной тестовой сферы, чтобы run_loop мог периодически
    // печатать их реальную Transform.position (та же позиция, что
    // sync_physics_transforms пишет туда каждый кадр из физики).
    let mut sphere_entities = Vec::new();
    for i in 0..5 {
        let x = -8.0 + i as f32 * 1.5;
        let y = 4.0 + i as f32 * 1.2; // разная высота старта — падают не синхронно, нагляднее видно физику
        if let Some((_body_id, entity)) = engine.spawn_physics_sphere(sphere_mesh, x, y, 1.5, 1.0) {
            spawned += 1;
            sphere_entities.push(entity);
        }
    }

    println!("[MAIN] ✓ Физика готова: пол + {} падающих сфер", spawned);

    // ИСПРАВЛЕНО (баг: "кубы-дома тоже [падают]" — тот же класс бага, что
    // и выше, но в ДРУГОМ месте): `AlworldFile::create_and_save_demo_world`
    // (alworld_format.rs) кладёт ровно ОДИН физический объект — в
    // центральном чанке demo_world (grid x=0,z=0), приподнятый на 5м,
    // mass=1.0 — падает при загрузке чанка через `load_chunk`.
    //
    // ИСПРАВЛЕНО (это было НЕВЕРНО в предыдущей версии этого фикса —
    // подтверждено логом пользователя: "81 чанков по 64м"): реальный
    // `chunk_size` demo_world — 64.0, а НЕ 0.6! `AlworldFile::new(0.6)`
    // передаёт 0.6 как `world_size_km` (используется только для
    // вычисления `chunks_per_axis`/границ мира), а `chunk_size` внутри
    // `AlworldFile::new()` ЖЁСТКО захардкожен как `64.0` — не зависит от
    // аргумента вообще (см. `alworld_format.rs::AlworldFile::new`,
    // `let chunk_size = 64.0;`). Прошлая версия этого фикса считала опору
    // по 0.6 и клала её в (0.3, *, 0.3) — почти в 100 раз мимо реальной
    // точки падения объекта (32.0, *, 32.0), что и объясняет, почему куб
    // всё ещё падал и не возвращался после первого фикса.
    const DEMO_WORLD_CHUNK_SIZE: f32 = 64.0; // должно совпадать с захардкоженным chunk_size в AlworldFile::new()
    let demo_world_floor_x = (0.0 + 0.5) * DEMO_WORLD_CHUNK_SIZE; // тот же center_x, что в create_and_save_demo_world для x=0
    let demo_world_floor_z = (0.0 + 0.5) * DEMO_WORLD_CHUNK_SIZE;
    if engine.add_sphere_body(demo_world_floor_x, GROUND_Y, demo_world_floor_z, 0.0).is_none() {
        eprintln!("[MAIN] WARNING: не удалось создать опору под физическим объектом demo_world");
    } else {
        println!("[MAIN] ✓ Опора под demo_world объектом создана ({:.1}, {:.1}, {:.1})", demo_world_floor_x, GROUND_Y, demo_world_floor_z);
    }

    sphere_entities
}

/// ДОБАВЛЕНО (World Streaming — проверка стриминга .alworld вживую):
/// создаёт демонстрационный мир (9x9 чанков по 64м, см.
/// `AlworldFile::create_and_save_demo_world`) прямо рядом с exe и сразу
/// загружает его в движок. Дальше `engine.update()` внутри `run_loop`
/// сам подгружает/выгружает чанки по мере ходьбы камеры — здесь только
/// однократная инициализация.
fn setup_world_streaming(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up world streaming demo...");
    match engine.load_demo_world("demo_world") {
        Ok(()) => println!("[MAIN] ✓ Демо-мир создан и загружен (demo_world/world.alworld)"),
        Err(e) => eprintln!("[MAIN] WARNING: не удалось создать/загрузить демо-мир: {:?} — стриминг не будет активен", e),
    }
}

/// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): подключает
/// `AudioEngine` (реальный XAudio2-плейбек, см. audio.rs) и наполняет его
/// демонстрационным `.alsnd` банком — та же структура, что у
/// `setup_lights`/`setup_physics`: реальный внешний движок/подсистема +
/// демо-контент, который проходит через полный путь сохранения/загрузки
/// формата (`AlsndFile::save`/`load`), а не только через память процесса.
///
/// На реальном диске пользователя нет готовых `.wav`-ассетов для движка
/// (это НЕ то же самое, что отсутствие аудио-плагина — см. подробный
/// комментарий про архитектурное решение в начале audio.rs) — поэтому,
/// как и `tile_mesh`/`pillar_mesh` в `setup_scene` используют простые
/// процедурные кубы вместо настоящих .altex-моделей, здесь звуки
/// генерируются процедурно (чистые синусоиды на разных частотах) прямо в
/// этой функции, а не загружаются с диска как готовые файлы. Это честная
/// демонстрация: реальный XAudio2-плейбек и реальный .alsnd round-trip,
/// со звуковым содержимым-плейсхолдером.
fn setup_audio(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up audio engine (XAudio2)...");

    if let Err(e) = engine.init_audio() {
        eprintln!("[MAIN] WARNING: не удалось создать звуковой движок: {:?} — сцена останется без звука", e);
        return;
    }
    println!("[MAIN] ✓ AudioEngine создан");

    std::fs::create_dir_all(SOUND_DEMO_DIR).ok();

    // Три процедурных звука: короткий "клик" шага (UI/SFX-подобный, 2D),
    // гудение уличного фонаря (тихий протяжный тон, 3D — привязан к
    // позиции первого фонаря ряда) и общий городской эмбиент (2D, фоновая
    // музыка/атмосфера). Частоты и длительности подобраны так, чтобы звуки
    // были явно РАЗЛИЧИМЫ на слух, а не просто "тишина/непонятный писк".
    let footstep_path = format!("{}/footstep.wav", SOUND_DEMO_DIR);
    let hum_path = format!("{}/street_hum.wav", SOUND_DEMO_DIR);
    let ambient_path = format!("{}/city_ambient.wav", SOUND_DEMO_DIR);

    write_tone_wav(&footstep_path, 220.0, 0.12, 0.5).ok();
    write_tone_wav(&hum_path, 110.0, 1.5, 0.25).ok();
    write_tone_wav(&ambient_path, 55.0, 2.0, 0.15).ok();

    let mut bank = alkash3d_rs::AlsndFile::new(1, 44100); // моно 44.1кГц — минимально достаточно для процедурных тонов-плейсхолдеров

    let footstep_name = bank.add_string("footstep");
    bank.sounds.push(alkash3d_rs::SoundDescriptor {
        name_id: footstep_name,
        format: 0, // WAV
        category: 0, // SFX
        data_offset: 0,
        size_compressed: 0,
        size_uncompressed: 0,
        duration_ms: 120,
        loop_start_ms: 0,
        loop_end_ms: 0, // не зациклен
        default_volume: 0.6,
        default_pitch: 1.0,
        priority: 150,
        max_instances: 4,
        spatial_blend: 0.0, // 2D — звук шагов игрока, не привязан к точке в мире
    });

    let hum_name = bank.add_string("street_hum");
    bank.sounds.push(alkash3d_rs::SoundDescriptor {
        name_id: hum_name,
        format: 0,
        category: 2, // Ambient
        data_offset: 0,
        size_compressed: 0,
        size_uncompressed: 0,
        duration_ms: 1500,
        loop_start_ms: 0,
        loop_end_ms: 1500, // зациклен — фонарь гудит непрерывно, пока слушатель рядом
        default_volume: 0.35,
        default_pitch: 1.0,
        priority: 80,
        max_instances: 20, // по одному на каждый из 20 фонарей потенциально
        spatial_blend: 1.0, // полностью 3D — источник должен звучать ИМЕННО из точки фонаря
    });

    let ambient_name = bank.add_string("city_ambient");
    bank.sounds.push(alkash3d_rs::SoundDescriptor {
        name_id: ambient_name,
        format: 0,
        category: 2,
        data_offset: 0,
        size_compressed: 0,
        size_uncompressed: 0,
        duration_ms: 2000,
        loop_start_ms: 0,
        loop_end_ms: 2000,
        default_volume: 0.2,
        default_pitch: 1.0,
        priority: 60,
        max_instances: 1, // фоновая атмосфера — играет только одна копия сразу
        spatial_blend: 0.0, // 2D — фоновая атмосфера города, не привязана к точке
    });

    let alsnd_path = format!("{}/demo.alsnd", SOUND_DEMO_DIR);
    if let Err(e) = bank.save(&alsnd_path) {
        eprintln!("[MAIN] WARNING: не удалось сохранить {}: {:?} — банк звуков не будет загружен", alsnd_path, e);
        return;
    }

    match engine.load_sound_bank(&alsnd_path, SOUND_DEMO_DIR) {
        Ok(count) => println!("[MAIN] ✓ Звуковой банк загружен: {} звук(ов) из {}", count, alsnd_path),
        Err(e) => {
            eprintln!("[MAIN] WARNING: не удалось загрузить звуковой банк {}: {:?}", alsnd_path, e);
            return;
        }
    }

    // Городской эмбиент запускается сразу и зациклен — играет фоном всю
    // демо-сессию (как музыка/атмосфера в setup_lights создаёт визуальную
    // ночную сцену, здесь — её звуковой эквивалент).
    if engine.play_sound("city_ambient", Vec3::ZERO).is_some() {
        println!("[MAIN] ✓ Городской эмбиент запущен");
    }

    // Гудение первого уличного фонаря ряда (x = -50.0, см. константы
    // STREET_LIGHT_* выше) — демонстрирует именно 3D-звук: при ходьбе
    // вдоль улицы (WASD) громкость и панорама этого источника должны
    // ощутимо меняться в зависимости от положения камеры относительно
    // фонаря.
    let first_light_x = STREET_LIGHT_X_OFFSET * STREET_LIGHT_SPACING;
    let hum_position = Vec3::new(first_light_x, 3.0, 0.0);
    if engine.play_sound("street_hum", hum_position).is_some() {
        println!("[MAIN] ✓ 3D-гудение фонаря запущено (позиция x={:.1})", first_light_x);
    }
}

/// ДОБАВЛЕНО (скриптинг — три языка сразу для наглядной сравнительной
/// проверки; C# как язык скриптинга вырезан из движка по просьбе
/// пользователя, см. комментарий у `fn main` выше): три одинаковых
/// "bobber"-куба РЯДОМ друг с другом, каждый на своём языке скриптинга —
/// Python (hot-reload, встроенный интерпретатор), Lua (DLL, универсальный
/// рантайм alkash3d-luascript), Native/Rust (DLL, alkash3d-examplescript).
/// Все три — НЕ физические тела (физика в `update()` каждый кадр
/// перезаписывает Transform, конфликтовало бы со скриптом). Каждый язык
/// загружается НЕЗАВИСИМО — ошибка в одном (например Native/Lua не
/// собраны) не мешает остальным, только печатает WARNING и оставляет
/// свой куб неподвижным.
///
/// Возвращает `ScriptingHandles` — нужен `run_loop`, чтобы периодически
/// слать `on_event`/`dispatch_script_event` каждому языку (демонстрация
/// не только per-frame `update`, но и событий).
struct ScriptingHandles {
    python_entity: Option<alkash3d_rs::scene::EntityId>,
    // ВАЖНО: `ScriptHandle` реэкспортирован из `engine::mod.rs`
    // (`pub use scripting::{ScriptHandle, pack_entity_id};`), а НЕ из
    // корня крейта — в отличие от `plugin::*` (см. `use
    // alkash3d_rs::PhysicsConfig` выше), `alkash3d_rs::ScriptHandle` был
    // бы E0433, нужен полный путь `alkash3d_rs::engine::ScriptHandle`.
    native_handle: Option<alkash3d_rs::engine::ScriptHandle>,
    lua_handle: Option<alkash3d_rs::engine::ScriptHandle>,
}

fn setup_scripting(engine: &mut AlkashEngine) -> ScriptingHandles {
    println!("\n[MAIN] Setting up scripting (Python/Lua/Native)...");

    // ===== PYTHON (hot-reload, встроенный интерпретатор) =====
    let bobber_mesh = engine.add_cube_colored(0.6, 0.95, 0.55, 0.2, 1.0); // жёлтый
    let python_entity_raw = engine.spawn_mesh_entity(bobber_mesh);
    if let Some(t) = engine.scene.transform_mut(python_entity_raw) {
        t.position = [-4.0, GROUND_Y + 1.0, -1.5];
        t.scale = [0.4, 0.4, 0.4];
    }
    let python_entity = match engine.load_python_script(PYTHON_BOBBER_SCRIPT_PATH, python_entity_raw) {
        Ok(()) => {
            println!("[MAIN] ✓ Python-скрипт '{}' загружен (жёлтый куб)", PYTHON_BOBBER_SCRIPT_PATH);
            Some(python_entity_raw)
        }
        Err(e) => {
            eprintln!("[MAIN] WARNING: Python-скрипт '{}' не загружен: {:?}", PYTHON_BOBBER_SCRIPT_PATH, e);
            None
        }
    };

    // ===== LUA (DLL, универсальный рантайм) =====
    let lua_mesh = engine.add_cube_colored(0.3, 0.5, 0.85, 0.2, 1.0); // синий
    let lua_entity = engine.spawn_mesh_entity(lua_mesh);
    if let Some(t) = engine.scene.transform_mut(lua_entity) {
        t.position = [-3.0, GROUND_Y + 1.0, -1.5];
        t.scale = [0.4, 0.4, 0.4];
    }
    let lua_handle = match engine.load_lua_script(LUA_RUNTIME_DLL_PATH, LUA_BOBBER_SCRIPT_PATH, lua_entity) {
        Ok(handle) => {
            println!("[MAIN] ✓ Lua-скрипт '{}' загружен (синий куб)", LUA_BOBBER_SCRIPT_PATH);
            Some(handle)
        }
        Err(e) => {
            eprintln!(
                "[MAIN] WARNING: Lua-плагин '{}' не загружен: {:?} — проверь, что alkash3d-luascript собран \
                 (cargo build --release из папки alkash3d-luascript)",
                LUA_RUNTIME_DLL_PATH, e
            );
            None
        }
    };

    // ===== NATIVE / Rust (DLL, alkash3d-examplescript) =====
    let native_mesh = engine.add_cube_colored(0.85, 0.35, 0.3, 0.2, 1.0); // красный
    let native_entity = engine.spawn_mesh_entity(native_mesh);
    if let Some(t) = engine.scene.transform_mut(native_entity) {
        t.position = [-2.0, GROUND_Y + 1.0, -1.5];
        t.scale = [0.4, 0.4, 0.4];
    }
    let native_handle = match engine.load_native_script(NATIVE_EXAMPLE_DLL_PATH, native_entity) {
        Ok(handle) => {
            println!("[MAIN] ✓ Native-скрипт '{}' загружен (красный куб)", NATIVE_EXAMPLE_DLL_PATH);
            Some(handle)
        }
        Err(e) => {
            eprintln!(
                "[MAIN] WARNING: Native-плагин '{}' не загружен: {:?} — проверь, что alkash3d-examplescript собран \
                 (cargo build --release из папки alkash3d-examplescript)",
                NATIVE_EXAMPLE_DLL_PATH, e
            );
            None
        }
    };

    ScriptingHandles {
        python_entity,
        native_handle,
        lua_handle,
    }
}

/// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): генерирует
/// простой несжатый PCM WAV-файл с чистым синусоидальным тоном заданной
/// частоты/длительности — единственный способ получить РЕАЛЬНЫЙ,
/// проигрываемый .wav без готовых звуковых ассетов на диске (см.
/// подробный комментарий у `setup_audio` выше). 16-бит моно 44100 Гц —
/// формат, гарантированно поддерживаемый WAV-декодером движка
/// (`AudioEngine::decode_wav` в audio.rs требует WAVE_FORMAT_PCM).
///
/// Огибающая (fade-in/fade-out по 10мс на границах) — без неё каждый
/// цикл зацикленного тона (street_hum/city_ambient, `loop_end_ms` в
/// `setup_audio`) давал бы слышимый щелчок на стыке конца/начала буфера
/// (резкий скачок амплитуды с ненулевого значения до 0 и обратно) —
/// стандартная практика при генерации/нарезке зацикленного аудио.
fn write_tone_wav(path: &str, frequency_hz: f32, duration_secs: f32, amplitude: f32) -> std::io::Result<()> {
    use std::io::Write;

    const SAMPLE_RATE: u32 = 44100;
    let sample_count = (SAMPLE_RATE as f32 * duration_secs) as u32;
    let fade_samples = (SAMPLE_RATE as f32 * 0.01) as u32; // 10мс

    let mut samples: Vec<i16> = Vec::with_capacity(sample_count as usize);
    for i in 0..sample_count {
        let t = i as f32 / SAMPLE_RATE as f32;
        let mut env = 1.0f32;
        if i < fade_samples {
            env = i as f32 / fade_samples.max(1) as f32;
        } else if i > sample_count.saturating_sub(fade_samples) {
            env = (sample_count - i) as f32 / fade_samples.max(1) as f32;
        }
        let value = (t * frequency_hz * std::f32::consts::TAU).sin() * amplitude * env;
        samples.push((value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16);
    }

    let data_bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();

    let byte_rate = SAMPLE_RATE * 1 * 2; // моно, 16 бит = 2 байта/сэмпл
    let block_align: u16 = 2;

    let mut file = std::fs::File::create(path)?;
    // RIFF-заголовок
    file.write_all(b"RIFF")?;
    file.write_all(&((36 + data_bytes.len()) as u32).to_le_bytes())?;
    file.write_all(b"WAVE")?;
    // Чанк fmt
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // размер чанка fmt
    file.write_all(&1u16.to_le_bytes())?; // wFormatTag = WAVE_FORMAT_PCM
    file.write_all(&1u16.to_le_bytes())?; // nChannels = моно
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?; // wBitsPerSample
    // Чанк data
    file.write_all(b"data")?;
    file.write_all(&(data_bytes.len() as u32).to_le_bytes())?;
    file.write_all(&data_bytes)?;

    Ok(())
}

fn run_loop(engine: &mut AlkashEngine, scripting_handles: ScriptingHandles, physics_debug_entities: Vec<alkash3d_rs::scene::EntityId>) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    let mut frame_count: u64 = 0;
    let mut time = 0.0f32;
    let start = Instant::now();

    let mut fps_window_start = Instant::now();
    let mut fps_window_frames: u32 = 0;

    // ДОБАВЛЕНО (диагностика — жалоба пользователя "всё равно ФПС не
    // радует" ПОСЛЕ фиксов стриминга/hot-reload/culling): вместо
    // дальнейших догадок по коду измеряем РЕАЛЬНОЕ время двух главных фаз
    // кадра — `engine.update()` (физика + скрипты + world streaming +
    // day/night + culling фонарей + аудио) и `engine.render_frame()` (весь
    // D3D12: shadow pass + main pass + bloom/tonemap + Present) — отдельно
    // друг от друга. Фиксируем ХУДШИЙ (не средний) кадр за последнее
    // 1-секундное окно каждой фазы — усреднённый FPS размазывает один
    // дорогой кадр на весь вывод (см. подробный комментарий у
    // `fps_window_start` выше), а нас интересуют именно СПАЙКИ, а не
    // средняя стоимость. Печатается вместе с FPS раз в секунду ниже —
    // сразу видно, физика ли стоит дорого, рендер, или что-то третье
    // (например Present/wait_for_fence, попадающее в render_frame).
    let mut max_update_ms: f32 = 0.0;
    let mut max_render_ms: f32 = 0.0;

    // ДОБАВЛЕНО (скриптинг — демонстрация on_event/dispatch_script_event
    // для ВСЕХ 3 языков сразу, не только update): раз в
    // BOBBER_EVENT_PERIOD_SECS секунд каждому из трёх bobber-кубов шлём
    // событие Custom (0) со случайно-циклической амплитудой (1x/2x/3x от
    // базовой 0.3м, одинаковое соглашение во всех языках — см.
    // bobber.py/bobber.lua/alkash3d-examplescript) — так видно, что и
    // `dispatch_python_event`, и `dispatch_script_event` (Native/Lua
    // используют один и тот же метод, см. engine/scripting.rs) реально
    // доходят до скрипта и меняют его поведение в рантайме, а не только
    // что update() вызывается каждый кадр.
    const BOBBER_EVENT_PERIOD_SECS: f32 = 5.0;
    let mut bobber_event_timer = 0.0f32;
    let mut bobber_amplitude_step: u32 = 1;

    // ИЗМЕНЕНО: старое z=4.0 оказалось РОВНО на краю нового узкого пола
    // улицы (GRID_HALF_Z=4, см. константы выше) — сдвинуто ближе к
    // центру по Z (z=1.0), чтобы гарантированно стоять НА полу, а не на
    // самой кромке. x=-8.0 остаётся почти в центре ряда из 20 фонарей
    // (фонари на x = -50..45 с шагом 5), рядом сразу с несколькими
    // фонарями по обе стороны.
    engine.camera.position = Vec3::new(-8.0, EYE_HEIGHT, 1.0);
    engine.camera.target = Vec3::new(0.0, EYE_HEIGHT - 0.2, 0.0);

    let rot_speed = 2.0;

    while engine.is_running() {
        engine.process_messages();
        if !engine.is_running() {
            break; // окно закрыли во время process_messages() — не рендерим лишний кадр
        }

        let dt = {
            let now = start.elapsed().as_secs_f32();
            let dt = (now - time).min(0.05);
            time = now;
            dt
        };

        // ===== ВЫХОД ПО ESC =====
        // ИСПРАВЛЕНО: раньше ESC обрабатывался прямо внутри движка
        // (wndproc) — теперь это решение приложения, движок только даёт
        // состояние клавиши.
        if engine.input.just_pressed(keys::ESCAPE) {
            println!("[MAIN] ESC pressed - exiting");
            engine.request_exit();
            continue;
        }

        // ===== ОСМОТРЕТЬСЯ (стрелки) =====
        let rot_amount = rot_speed * dt;
        if engine.input.is_down(keys::ARROW_LEFT) { engine.camera.rotate_yaw(rot_amount); }
        if engine.input.is_down(keys::ARROW_RIGHT) { engine.camera.rotate_yaw(-rot_amount); }
        if engine.input.is_down(keys::ARROW_UP) { engine.camera.rotate_pitch(-rot_amount); }
        if engine.input.is_down(keys::ARROW_DOWN) { engine.camera.rotate_pitch(rot_amount); }

        // ===== ХОДЬБА (WASD) =====
        // Движение специально ЗАПЕРТО в горизонтальной плоскости (Y не
        // меняется от WASD) — это и отличает "ходьбу" от "полёта": куда
        // бы камера ни смотрела по вертикали (вверх/вниз стрелками),
        // персонаж всё равно идёт вперёд/назад/вбок вдоль пола, а не
        // взлетает или зарывается в землю.
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

        // Фиксируем высоту глаз над полом (сдвигаем и position, и target
        // на ОДИНАКОВУЮ величину по Y — так текущий угол наклона взгляда,
        // выставленный стрелками, не сбивается этим сдвигом).
        let dy = EYE_HEIGHT - engine.camera.position.y;
        engine.camera.position.y += dy;
        engine.camera.target.y += dy;

        // ===== ФОНАРИ: каллинг по текущей позиции камеры =====
        // ДОБАВЛЕНО: FirstFires должен знать, где сейчас камера, чтобы
        // отобрать (culling) видимые/близкие фонари для этого кадра — без
        // этого вызова `engine.update()` список видимых фонарей никогда
        // не обновится после первого кадра (LightPlugin::cull принимает
        // camera_pos явным параметром, см. AlkashEngine::update).
        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let update_start = Instant::now();
        engine.update(
            dt,
            -9.8,
            [engine.camera.position.x, engine.camera.position.y, engine.camera.position.z],
            view_proj.to_cols_array(),
        );
        let update_ms = update_start.elapsed().as_secs_f32() * 1000.0;
        if update_ms > max_update_ms { max_update_ms = update_ms; }

        // ===== СКРИПТИНГ: периодическое событие ВСЕМ 3 bobber'ам =====
        {
            bobber_event_timer += dt;
            if bobber_event_timer >= BOBBER_EVENT_PERIOD_SECS {
                bobber_event_timer = 0.0;
                bobber_amplitude_step = bobber_amplitude_step % 3 + 1; // цикл 1 -> 2 -> 3 -> 1 ...
                let amplitude_data = [bobber_amplitude_step as f32, 0.0, 0.0, 0.0];

                // Python — отдельный метод (нет ScriptHandle/DLL, ключ —
                // сама сущность, см. dispatch_python_event в
                // engine/scripting.rs).
                if let Some(entity) = scripting_handles.python_entity {
                    engine.dispatch_python_event(entity, 0, amplitude_data);
                }

                // Native/Lua — ОДИН И ТОТ ЖЕ метод dispatch_script_event
                // для обоих (оба — DLL-плагины с одинаковым ScriptingAPI
                // C-ABI, см. engine/scripting.rs — разница между ними уже
                // "спрятана" внутри загруженной DLL).
                let script_event = alkash3d_rs::ScriptEvent {
                    event_type: 0, // Custom
                    source_entity: 0,
                    target_entity: 0,
                    data: amplitude_data,
                };
                if let Some(handle) = &scripting_handles.lua_handle {
                    engine.dispatch_script_event(handle, &script_event);
                }
                if let Some(handle) = &scripting_handles.native_handle {
                    engine.dispatch_script_event(handle, &script_event);
                }
            }
        }

        // ===== РЕНДЕР =====
        let render_start = Instant::now();
        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN] Render error, stopping: {:?}", e);
            break;
        }
        let render_ms = render_start.elapsed().as_secs_f32() * 1000.0;
        if render_ms > max_render_ms { max_render_ms = render_ms; }

        frame_count += 1;
        fps_window_frames += 1;

        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
            println!("*** Camera pos: ({:.2}, {:.2}, {:.2}) ***",
                engine.camera.position.x, engine.camera.position.y, engine.camera.position.z
            );
            println!("*** {} ECS entities rendering ***\n", engine.scene.len());
        }

        if fps_window_start.elapsed().as_secs_f32() >= 1.0 {
            let fps = fps_window_frames as f32 / fps_window_start.elapsed().as_secs_f32();
            // ДОБАВЛЕНО (World Streaming — проверка стриминга вживую):
            // печатаем текущее число ECS-сущностей КАЖДУЮ секунду вместе
            // с FPS — при ходьбе через `demo_world` (см.
            // setup_world_streaming) это число должно расти, когда камера
            // подходит к новым чанкам, и падать, когда отходит от старых
            // (сама загрузка/выгрузка отдельно логируется движком, см.
            // "[ENGINE] World streaming: ..." в update_world_streaming,
            // engine/mod.rs) — простой способ убедиться, что стриминг
            // реально работает, даже не разглядывая объекты на экране.
            println!(
                "[INFO] Frame {} | FPS: {:.1} | ECS entities: {} | cam=({:.1},{:.1},{:.1})",
                frame_count, fps, engine.scene.len(),
                engine.camera.position.x, engine.camera.position.y, engine.camera.position.z
            );
            // ДОБАВЛЕНО (диагностика — см. подробный комментарий у
            // `max_update_ms`/`max_render_ms` выше): худший кадр за это же
            // окно по каждой из двух главных фаз — сразу видно, физика ли
            // (`update`) или рендер (`render_frame`, включая Present/
            // ожидание fence) даёт конкретный спайк.
            println!(
                "[TIMING] worst update()={:.1}ms | worst render_frame()={:.1}ms",
                max_update_ms, max_render_ms
            );
            // ДОБАВЛЕНО (диагностика — та же жалоба): реальная статистика
            // физики этого кадра из плагина Inertial (см.
            // `AlkashEngine::physics_stats`) — тела/активные тела (не
            // спящие и не статичные)/контакты/пары + время каждой фазы
            // солвера ВНУТРИ update(). Если active_bodies НЕ падает до 0
            // (падающие тестовые сферы должны заснуть после стабилизации),
            // значит физика продолжает нагружать CPU каждый кадр даже
            // когда на экране всё выглядит неподвижным.
            if let Some(stats) = engine.physics_stats() {
                println!(
                    "[PHYS-STATS] bodies={} active={} contacts={} pairs={} | broad={:.2}ms narrow={:.2}ms solver={:.2}ms",
                    stats.bodies_count, stats.active_bodies, stats.contacts_count, stats.pairs_count,
                    stats.broad_phase_time_ms, stats.narrow_phase_time_ms, stats.solver_time_ms
                );
            }
            // ДОБАВЛЕНО (диагностика, следующий шаг после [PHYS-STATS]
            // подтвердил, что физика во время спайков спокойна): разбивка
            // ХУДШЕГО случая каждой под-фазы update() за это же окно (см.
            // `AlkashEngine::take_update_breakdown`/`UpdateBreakdownMs`) —
            // теперь видно НЕ только "update() иногда занимает 160мс", а
            // ТОЧНО какая под-фаза внутри него виновата (скрипты/каллинг
            // света/день-ночь/стриминг/аудио), без дальнейших догадок.
            // `take_update_breakdown` сама сбрасывает счётчики в 0 — здесь
            // отдельный ручной сброс не нужен (в отличие от
            // max_update_ms/max_render_ms ниже).
            let breakdown = engine.take_update_breakdown();
            println!(
                "[UPDATE-BREAKDOWN] physics={:.1}ms sync_physics={:.1}ms native_scripts={:.1}ms python_scripts={:.1}ms day_night={:.1}ms world_streaming={:.1}ms chunk_io={:.1}ms light_cull={:.1}ms audio={:.1}ms",
                breakdown.physics_ms, breakdown.sync_physics_ms, breakdown.native_scripts_ms,
                breakdown.python_scripts_ms, breakdown.day_night_ms, breakdown.world_streaming_ms,
                breakdown.chunk_io_ms, breakdown.light_cull_ms, breakdown.audio_ms
            );
            // ДОБАВЛЕНО (диагностика бага "падают и пропадают под картой"):
            // печатаем РЕАЛЬНУЮ Transform.position каждой тестовой
            // физической сферы раз в секунду — та же позиция, которую
            // sync_physics_transforms пишет туда каждый кадр из физики
            // (engine::mod.rs). Если сферы реально проваливаются сквозь
            // пол, здесь будет видно постоянно убывающий Y без остановки;
            // если они на самом деле стоят на месте, а пропадает только
            // ВИДИМОСТЬ — Y здесь остановится около 1.0, и проблема не в
            // физике, а в рендере/culling.
            for (i, entity) in physics_debug_entities.iter().enumerate() {
                if let Some(t) = engine.scene.transform(*entity) {
                    println!(
                        "[PHYS-DEBUG] sphere[{}] pos=({:.2},{:.2},{:.2})",
                        i, t.position[0], t.position[1], t.position[2]
                    );
                }
            }
            fps_window_frames = 0;
            fps_window_start = Instant::now();
            max_update_ms = 0.0;
            max_render_ms = 0.0;
        }
    }

    println!("\n=== RENDER LOOP STOPPED (frames rendered: {}) ===", frame_count);
}