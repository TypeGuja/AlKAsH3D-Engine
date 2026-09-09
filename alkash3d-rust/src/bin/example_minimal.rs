// src/bin/example_minimal.rs
//! Минимальный пример на движке "с нуля" — НЕ версия main_car.rs, отдельный
//! чистый файл, написанный заново специально для двух целей:
//!
//! 1. Показать минимальный рабочий каркас использования `AlkashEngine`
//!    (окно → устройство → один куб → рендер-цикл → чистое завершение) без
//!    единой лишней детали — годится как отправная точка для нового
//!    примера/демо.
//! 2. Бисекция воспроизведённого на `main_car` `DXGI_ERROR_DEVICE_HUNG`:
//!    сцена здесь настолько простая, насколько возможно (один куб, без
//!    физики, без кастомного освещения), а тяжёлые проходы рендера и
//!    отдельные фичи сцены включаются/выключаются НЕЗАВИСИМО друг от друга
//!    через переменные окружения — так один и тот же бинарник без
//!    пересборки проверяет любую комбинацию, вместо того чтобы писать
//!    отдельный файл на каждую гипотезу.
//!
//! РЕЗУЛЬТАТ БИСЕКЦИИ (2026-09-08): баг найден и исправлен. Оказалось, что
//! содержимое сцены вообще ни при чём — с текстурами, физикой, иерархией
//! колёс, day/night и 24 мешами всё проходило чисто на 800x600, а голая
//! сцена из двух мешей стабильно вешала GPU на 1366x768. Причина —
//! `handle_resize()` в engine/window.rs пересоздавал только `Renderer`, но
//! не depth SRV (он оставался указывать на уничтоженный depth stencil) и не
//! half-res таргеты bloom/volumetric (они навсегда оставались от исходного,
//! неподогнанного под рамку окна размера). Подробности — в комментарии
//! в `handle_resize`. Этот файл оставлен как регрессионный тест: разрешение
//! ниже специально то самое, на котором баг воспроизводился 100% раз.
//!
//! Управление бисекцией (все — булевы флаги, достаточно самого факта
//! `set`, значение не проверяется):
//!   `ALKASH3D_DIAG_NO_SHADOW=1`     — выключить shadow mapping
//!   `ALKASH3D_DIAG_NO_VOLUMETRIC=1` — выключить volumetric god rays
//!   `ALKASH3D_DIAG_NO_BLOOM=1`      — выключить bloom
//!
//! Пример: `ALKASH3D_DIAG_NO_SHADOW=1 ALKASH3D_DIAG_NO_VOLUMETRIC=1 cargo run --bin example_minimal`
//!
//! Кадровый бюджет (см. `FRAME_BUDGET` ниже) — self-terminating: скрипт
//! диагностики не должен гадать, сколько ждать реальное окно, пока оно
//! либо зависнет, либо честно доедет до конца бюджета и само выйдет с
//! понятным PASS/FAIL в последней строке лога.

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};
use alkash3d_rs::PhysicsConfig;

/// Путь к Inertial — тот же, что и в main_car.rs (соседняя директория).
const INERTIAL_DLL_PATH: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";

// СОВПАДАЕТ с main_car.rs (1366x768) — это и оказалось единственным
// значимым отличием: Windows отдаёт окну клиентскую область 1350x729
// (подгонка под рамку), и на устаревших после ресайза ресурсах GPU
// стабильно вис. НЕ МЕНЯТЬ на "круглое" разрешение: именно эта пара чисел
// делает файл регрессионным тестом того фикса.
const WINDOW_WIDTH: u32 = 1366;
const WINDOW_HEIGHT: u32 = 768;

/// Сколько кадров прогнать перед чистым самозавершением, если ничего не
/// зависло. 300 кадров — с запасом покрывает интересующее нас окно (баг
/// стабильно ловился на кадре 2-3), но не настолько много, чтобы держать
/// живой D3D12-процесс дольше необходимого при батч-бисекции.
const FRAME_BUDGET: u32 = 300;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{} — Minimal Example", alkash3d_rs::VERSION);
    println!("==========================================");
    println!(
        "[DIAG] Флаги бисекции: NO_SHADOW={} NO_VOLUMETRIC={} NO_BLOOM={}",
        std::env::var("ALKASH3D_DIAG_NO_SHADOW").is_ok(),
        std::env::var("ALKASH3D_DIAG_NO_VOLUMETRIC").is_ok(),
        std::env::var("ALKASH3D_DIAG_NO_BLOOM").is_ok(),
    );

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);

    if let Err(e) = engine.init() {
        eprintln!("[EXAMPLE] FAIL: engine.init() вернул ошибку: {:?}", e);
        return Err(e.into());
    }

    // Применяем флаги бисекции СРАЗУ после init() — все три ресурса уже
    // созданы к этому моменту, но ни один кадр ещё не отрендерен.
    if std::env::var("ALKASH3D_DIAG_NO_SHADOW").is_ok() {
        engine.disable_shadows_for_diagnostics();
    }
    if std::env::var("ALKASH3D_DIAG_NO_VOLUMETRIC").is_ok() {
        engine.disable_volumetric_for_diagnostics();
    }
    if std::env::var("ALKASH3D_DIAG_NO_BLOOM").is_ok() {
        engine.disable_bloom_for_diagnostics();
    }

    setup_scene(&mut engine);

    // Прогрев PSO (см. engine/render_frame.rs::warm_up_pipelines) — сам по
    // себе не "чинит" известное зависание (см. историю отладки main_car),
    // но и не мешает, а для минимального примера как отправной точки
    // правильно делать то же, что и настоящий пример движка.
    if let Err(e) = engine.warm_up_pipelines() {
        eprintln!("[EXAMPLE] WARNING: прогрев PSO не завершился штатно: {:?}", e);
    }

    let exit_code = run_loop(&mut engine);
    engine.shutdown();
    println!("[EXAMPLE] Goodbye!");

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

/// Ровно один куб и пол — минимум, достаточный чтобы shadow/main-проходы
/// реально что-то рисовали (иначе PSO не получат ни одного Draw-вызова, и
/// прогрев/бисекция ничего не проверят).
///
/// `ALKASH3D_DIAG_TEXTURED_FLOOR=1` — бисекция: baseline (без этого флага)
/// прошёл 300 кадров БЕЗ единой ошибки со всеми проходами включёнными
/// (shadow+volumetric+bloom) — то есть сам пайплайн рендера исправен.
/// `main_car.rs`, в отличие от baseline, грузит процедурные текстуры через
/// `create_texture_rgba` ДО входа в render loop — а у загрузки текстуры в
/// `texture.rs` СВОЙ ОТДЕЛЬНЫЙ временный command allocator/list/fence, не
/// связанный с double-buffering в `render_frame()`. Этот флаг воспроизводит
/// РОВНО этот один шаг (одна процедурная текстура на полу) — если после
/// него зависание появится, причина локализована именно в пути загрузки
/// текстур, а не в самом рендер-пайплайне.
fn setup_scene(engine: &mut AlkashEngine) {
    println!("\n[EXAMPLE] Setting up minimal scene...");

    let cube_idx = engine.add_cube(1.0);
    engine.mesh_instances.push(MeshInstance::new(cube_idx).at(0.0, 0.5, 0.0));

    if std::env::var("ALKASH3D_DIAG_TEXTURED_FLOOR").is_ok() {
        println!("[DIAG] Пол — с процедурной текстурой (add_plane_textured), как в main_car");
        let (w, h, pixels) = alkash3d_rs::proc_textures::dirt_grass(256);
        let floor_srv = engine.create_texture_rgba(w, h, &pixels);
        let floor_idx = engine.add_plane_textured(10.0, 10.0, 0.35, [1.0, 1.0, 1.0, 1.0], floor_srv, 0.95, 0.0);
        engine.spawn_static_mesh(floor_idx, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    } else {
        let floor_idx = engine.add_quad(0.0, 0.0, 10.0, 10.0, [0.3, 0.3, 0.35, 1.0]);
        engine.mesh_instances.push(
            MeshInstance::new(floor_idx)
                .at(0.0, 0.0, 0.0)
                .rotated(-1.5708, 0.0, 0.0),
        );
    }

    engine.set_clear_color(0.05, 0.05, 0.1, 1.0);

    if std::env::var("ALKASH3D_DIAG_TIME_OF_DAY").is_ok() {
        println!("[DIAG] set_time_of_day(13.0), как в main_car (меняет light_dir/тени)");
        engine.set_time_of_day(13.0);
    }

    let need_physics = std::env::var("ALKASH3D_DIAG_PHYSICS").is_ok()
        || std::env::var("ALKASH3D_DIAG_CAR").is_ok();
    if need_physics {
        println!("[DIAG] Загружаю Inertial (физика), как в main_car");
        let config = PhysicsConfig {
            max_bodies: 256,
            world_size: 100.0,
            cell_size: 4.0,
            solver_iterations: 8,
            use_simd: 0,
        };
        match engine.init_physics(INERTIAL_DLL_PATH, config) {
            Ok(()) => {
                println!("[DIAG] ✓ Inertial loaded");
                if std::env::var("ALKASH3D_DIAG_PHYSICS").is_ok() {
                    let ball_mesh = engine.add_cube(0.4);
                    if engine.spawn_physics_sphere(ball_mesh, 0.0, 3.0, 0.0, 2.0).is_some() {
                        println!("[DIAG] ✓ Физический шар заспавнен (упадёт на пол за счёт add_sphere_body(..., 0.0) ниже)");
                    }
                    // Пол как один статический физический шар — иначе падающему
                    // шару не на что опереться (то же ограничение sphere-sphere
                    // narrow phase, что и в main.rs/main_car.rs).
                    engine.add_sphere_body(0.0, 0.0, 0.0, 0.0);
                }
                if std::env::var("ALKASH3D_DIAG_CAR").is_ok() {
                    // ЕДИНСТВЕННОЕ, что реально уникально для main_car и ещё не
                    // проверено этим примером: родитель-потомок трансформы
                    // (4 колеса, дочерние сущности кузова через scene.set_parent
                    // внутри spawn_physics_car).
                    println!("[DIAG] spawn_physics_car — иерархия кузов+4 колеса (parent-child transforms)");
                    let (chassis_mesh, wheel_mesh) = engine.add_car_demo_meshes();
                    if engine
                        .spawn_physics_car(chassis_mesh, wheel_mesh, 0.0, 2.0, 5.0, 50.0, [0.9, 0.4, 1.8], 0.35, 0.25)
                        .is_some()
                    {
                        println!("[DIAG] ✓ Физическая машина заспавнена");
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[DIAG] WARNING: не удалось загрузить Inertial ({}): {:?} — физика пропущена",
                    INERTIAL_DLL_PATH, e
                );
            }
        }
    }

    if std::env::var("ALKASH3D_DIAG_MANY_OBJECTS").is_ok() {
        println!("[DIAG] Добавляю ~20 доп. кубов (масштаб, близкий к main_car: 13-25 мешей)");
        for i in 0..20 {
            let x = (i % 5) as f32 * 1.5 - 3.0;
            let z = (i / 5) as f32 * 1.5 - 3.0;
            let idx = engine.add_cube(0.5);
            engine.mesh_instances.push(MeshInstance::new(idx).at(x, 0.25, z + 3.0));
        }
    }

    println!(
        "[EXAMPLE] Scene ready: {} meshes, {} instances",
        engine.meshes.len(),
        engine.mesh_instances.len()
    );
}

/// Возвращает 0 при чистом завершении (весь `FRAME_BUDGET` отрендерен без
/// ошибок ИЛИ окно закрыто пользователем/ESC), 1 — если `render_frame()`
/// вернул ошибку (в т.ч. воспроизведённый DEVICE_HUNG).
fn run_loop(engine: &mut AlkashEngine) -> i32 {
    println!("\n=== RENDER LOOP STARTING (бюджет {} кадров) ===\n", FRAME_BUDGET);

    let mut frame_count = 0u32;
    let start = std::time::Instant::now();
    let mut time = 0.0f32;

    while engine.is_running() {
        engine.process_messages();

        let now = start.elapsed().as_secs_f32();
        let dt = now - time;
        time = now;

        if let Some(instance) = engine.mesh_instances.get_mut(0) {
            instance.rotation[1] = time * 0.7;
        }

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let camera_pos = [
            engine.camera.position[0],
            engine.camera.position[1],
            engine.camera.position[2],
        ];
        engine.update(dt, -9.8, camera_pos, view_proj.to_cols_array());

        if let Err(e) = engine.render_frame() {
            eprintln!("[EXAMPLE] FAIL: render_frame() вернул ошибку на кадре {}: {:?}", frame_count, e);
            return 1;
        }

        frame_count += 1;
        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
        }
        if frame_count % 60 == 0 {
            println!("[INFO] Frame {} / {}", frame_count, FRAME_BUDGET);
        }
        if frame_count >= FRAME_BUDGET {
            println!("[EXAMPLE] PASS: {} кадров отрендерено без ошибок", frame_count);
            return 0;
        }
    }

    println!("[EXAMPLE] Окно закрыто пользователем на кадре {} — считаем PASS (ошибок рендера не было)", frame_count);
    0
}
