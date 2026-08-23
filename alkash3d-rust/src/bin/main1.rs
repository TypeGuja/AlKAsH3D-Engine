// src/bin/main.rs
//! Alkash3D Engine — полный пример использования.
//!
//! Показывает оба способа размещать объекты в сцене одновременно:
//!  1) СТАРЫЙ способ — `engine.mesh_instances` (плоский Vec, как было
//!     раньше) — используется здесь для пола, просто чтобы показать, что
//!     старый код продолжает работать без единого изменения.
//!  2) НОВЫЙ способ — ECS (`engine.scene`, см. scene.rs) — используется
//!     для "солнечной системы" из вращающихся друг вокруг друга объектов,
//!     чтобы наглядно показать, зачем нужна иерархия parent/child: чтобы
//!     заставить луну вращаться вокруг планеты, а планету — вокруг солнца,
//!     нам нужно каждый кадр менять всего два числа (углы пивотов), а не
//!     пересчитывать мировые позиции вручную.

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};
use alkash3d_rs::scene::EntityId;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::LightConfig;
use std::time::Instant;
use windows::core::Interface;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const WINDOW_WIDTH: u32 = 1366;
const WINDOW_HEIGHT: u32 = 720;

/// Путь к скомпилированному плагину FirstFires — соседняя директория
/// относительно alkash3d-rust (см. main.rs про то, откуда взялось это имя
/// файла: Cargo превращает дефис в имени пакета "alkash3d-firstfires" в
/// подчёркивание, так что итоговый файл — alkash3d_firstfires.dll). НЕ
/// "firstfires.dll" (без подчёркивания) — рядом лежит одноимённый файл от
/// другой/старой сборки, не экспортирующий ожидаемый API ("No light API").
const FIRSTFIRES_DLL_PATH: &str = "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll";

/// Держит ID сущностей "солнечной системы", которые нужно анимировать
/// каждый кадр (см. update_solar_system).
struct SolarSystem {
    /// Пивот планеты — пустая (без меша) сущность в центре солнца;
    /// вращение этого пивота и создаёт орбиту планеты.
    planet_pivot: EntityId,
    /// Пивот луны — то же самое, но в центре планеты.
    moon_pivot: EntityId,
    /// Сама планета вращается вокруг своей оси отдельно от орбиты.
    planet: EntityId,
    moon: EntityId,
    sun: EntityId,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{}", alkash3d_rs::VERSION);
    println!("==========================================");
    println!();
    println!("🎮 УПРАВЛЕНИЕ:");
    println!("  WASD  - движение камеры");
    println!("  Q/E   - подняться/опуститься");
    println!("  Стрелки - поворот камеры");
    println!("  SHIFT - ускорение (x2)");
    println!("  ESC   - выход");
    println!("==========================================");

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);

    if let Err(e) = engine.init() {
        eprintln!("[MAIN] Failed to initialize engine: {:?}", e);
        return Err(e.into());
    }

    setup_lights(&mut engine);
    let solar_system = setup_scene(&mut engine);
    run_loop(&mut engine, &solar_system);

    // ИСПРАВЛЕНО (не критично для корректности, но яснее): раньше здесь
    // ничего не вызывалось и расчёт шёл только на Drop. Drop теперь и сам
    // безопасно вызывает shutdown() при выходе из scope, но явный вызов
    // здесь — это просто хорошая практика: понятно из кода main(), что
    // именно и когда завершает работу движка, а не полагается неявно на
    // порядок разрушения полей структуры.
    engine.shutdown();

    println!("[MAIN] Goodbye!");
    Ok(())
}

/// Инициализирует плагин освещения (FirstFires) и ставит один простой
/// "уличный" источник света над сценой — в отличие от main.rs (который
/// грузит целую ночную сцену из .alfar-файла), здесь достаточно одного
/// источника, чтобы было видно, что HDR/bloom/tonemap-пайплайн из Фазы 5
/// реально получает данные об освещении, а не просто рисует в темноту.
/// Как и в main.rs, любая ошибка здесь не фатальна — сцена в худшем случае
/// останется без дополнительного света, но не упадёт.
fn setup_lights(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up lights...");

    let device_ptr = match alkash3d_rs::get_device() {
        Ok(device) => device.as_raw(),
        Err(e) => {
            eprintln!("[MAIN] Failed to get D3D12 device for lighting: {:?}", e);
            return;
        }
    };

    // ИСПРАВЛЕНО (та же просадка FPS, что и в main.rs — см. подробный
    // комментарий там): `add_street_light` ниже использует range=100.0,
    // при far_plane=100/grid_cell_size=10 сфера фонаря (диаметр 200)
    // покрывала бы весь мир [-100,100]^3 целиком, вырождая сетку
    // каллинга. far_plane=200/grid_cell_size=20 — та же память (8000
    // ячеек), но сфера снова покрывает лишь часть сетки.
    let config = LightConfig {
        max_lights: 64,
        tile_size: 16,
        far_plane: 200.0,
        lod_distances: [30.0, 60.0, 200.0],
        grid_cell_size: 20.0,
    };

    if let Err(e) = engine.init_lights(FIRSTFIRES_DLL_PATH, device_ptr, config) {
        eprintln!("[MAIN] Failed to init lights (scene will stay dark): {:?}", e);
        return;
    }

    engine.add_street_light(0.0, 3.0, 0.0);

    println!("✅ Lights ready (FirstFires plugin loaded, 1 street light)");
}

fn setup_scene(engine: &mut AlkashEngine) -> SolarSystem {
    println!("\n[MAIN] Setting up scene...");

    // ===== СТАРЫЙ СПОСОБ: пол через mesh_instances =====
    // Специально оставлен на mesh_instances, а не переведён на ECS —
    // показывает, что старый API никуда не делся и работает как раньше.
    let floor_idx = engine.add_quad(0.0, 0.0, 10.0, 10.0, [0.25, 0.25, 0.32, 1.0]);
    engine.mesh_instances.push(
        MeshInstance::new(floor_idx)
            .at(0.0, -1.2, 0.0)
            .rotated(-1.57, 0.0, 0.0),
    );

    // ===== НОВЫЙ СПОСОБ: солнечная система через ECS (scene.rs) =====
    let sun_mesh = engine.add_cube(1.0);
    let planet_mesh = engine.add_cube(0.45);
    let moon_mesh = engine.add_cube(0.2);

    // Солнце — корневая сущность в центре сцены.
    let sun = engine.spawn_mesh_entity(sun_mesh);

    // Пивот планеты — сущность БЕЗ меша, только Transform. Её вращение
    // вокруг оси Y и создаёт орбиту: планета сидит на фиксированном
    // расстоянии от пивота, а мы каждый кадр крутим сам пивот.
    let planet_pivot = engine.scene.spawn();
    engine.scene.set_parent(planet_pivot, Some(sun));

    let planet = engine.spawn_mesh_entity(planet_mesh);
    engine.scene.set_parent(planet, Some(planet_pivot));
    if let Some(t) = engine.scene.transform_mut(planet) {
        t.position = [2.8, 0.0, 0.0]; // расстояние от солнца
    }

    // Пивот луны — та же идея, но в системе координат планеты: когда
    // планета вращается вокруг солнца (через planet_pivot), пивот луны
    // (являясь потомком planet) автоматически летит вместе с планетой —
    // считать это вручную не нужно, иерархия делает это сама.
    let moon_pivot = engine.scene.spawn();
    engine.scene.set_parent(moon_pivot, Some(planet));

    let moon = engine.spawn_mesh_entity(moon_mesh);
    engine.scene.set_parent(moon, Some(moon_pivot));
    if let Some(t) = engine.scene.transform_mut(moon) {
        t.position = [0.8, 0.0, 0.0];
    }

    engine.scene.set_name(sun, "Sun");
    engine.scene.set_name(planet, "Planet");
    engine.scene.set_name(moon, "Moon");

    engine.set_clear_color(0.03, 0.03, 0.07, 1.0);

    println!(
        "✅ Scene ready: {} mesh_instances (старый API) + {} entities (ECS)",
        engine.mesh_instances.len(),
        engine.scene.len()
    );

    SolarSystem {
        planet_pivot,
        moon_pivot,
        planet,
        moon,
        sun,
    }
}

/// Обновляет углы пивотов и собственное вращение тел каждый кадр.
fn update_solar_system(engine: &mut AlkashEngine, solar: &SolarSystem, time: f32) {
    // Орбита планеты вокруг солнца.
    if let Some(t) = engine.scene.transform_mut(solar.planet_pivot) {
        t.rotation[1] = time * 0.4;
    }
    // Орбита луны вокруг планеты (быстрее, чтобы было заметно отдельно от
    // орбиты планеты).
    if let Some(t) = engine.scene.transform_mut(solar.moon_pivot) {
        t.rotation[1] = time * 1.8;
    }
    // Собственное вращение (спин) планеты и солнца — не влияет на орбиты,
    // просто визуальное разнообразие.
    if let Some(t) = engine.scene.transform_mut(solar.planet) {
        t.rotation[1] = time * 2.0;
    }
    if let Some(t) = engine.scene.transform_mut(solar.sun) {
        t.rotation[1] = time * 0.15;
    }
    let _ = solar.moon; // luna не вращается вокруг своей оси в этом демо
}

fn run_loop(engine: &mut AlkashEngine, solar: &SolarSystem) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    let mut frame_count: u64 = 0;
    let mut time = 0.0f32;
    let start = Instant::now();

    // Для подсчёта FPS за скользящее окно (печатаем раз в секунду).
    let mut fps_window_start = Instant::now();
    let mut fps_window_frames: u32 = 0;

    engine.camera.position = Vec3::new(0.0, 3.5, 9.0);
    engine.camera.target = Vec3::new(0.0, 0.0, 0.0);

    let rot_speed = 2.0;

    while engine.is_running() {
        engine.process_messages();
        if !engine.is_running() {
            // Окно закрыли (WM_CLOSE) во время process_messages() — не
            // рендерим лишний кадр, выходим сразу.
            break;
        }

        let dt = {
            let now = start.elapsed().as_secs_f32();
            let dt = (now - time).min(0.05); // защита от скачка dt после лагов/паузы
            time = now;
            dt
        };

        // ===== ВВОД: камера =====
        unsafe {
            let w = (GetAsyncKeyState(0x57) as i16) < 0;
            let s = (GetAsyncKeyState(0x53) as i16) < 0;
            let a = (GetAsyncKeyState(0x41) as i16) < 0;
            let d = (GetAsyncKeyState(0x44) as i16) < 0;
            let q = (GetAsyncKeyState(0x51) as i16) < 0;
            let e = (GetAsyncKeyState(0x45) as i16) < 0;

            let up = (GetAsyncKeyState(0x26) as i16) < 0;
            let down = (GetAsyncKeyState(0x28) as i16) < 0;
            let left = (GetAsyncKeyState(0x25) as i16) < 0;
            let right = (GetAsyncKeyState(0x27) as i16) < 0;

            let shift = (GetAsyncKeyState(0x10) as i16) < 0;
            let move_speed = if shift { 10.0 } else { 5.0 };

            let rot_amount = rot_speed * dt;
            if left { engine.camera.rotate_yaw(rot_amount); }
            if right { engine.camera.rotate_yaw(-rot_amount); }
            if up { engine.camera.rotate_pitch(-rot_amount); }
            if down { engine.camera.rotate_pitch(rot_amount); }

            let move_amount = move_speed * dt;
            if w { engine.camera.move_forward(move_amount); }
            if s { engine.camera.move_forward(-move_amount); }
            if a { engine.camera.move_right(-move_amount); }
            if d { engine.camera.move_right(move_amount); }
            if q {
                engine.camera.position.y += move_amount;
                engine.camera.target.y += move_amount;
            }
            if e {
                engine.camera.position.y -= move_amount;
                engine.camera.target.y -= move_amount;
            }
        }

        // ===== АНИМАЦИЯ: солнечная система (ECS) =====
        update_solar_system(engine, solar, time);

        // ===== ОБНОВЛЕНИЕ ДВИЖКА (свет и т.п.) =====
        // ИСПРАВЛЕНО: раньше engine.update() здесь вообще не вызывался,
        // из-за чего плагин освещения (FirstFires) никогда не получал
        // текущую позицию камеры/view-proj и не выполнял per-frame culling
        // источников света после самого первого кадра.
        {
            let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
            let camera_pos = [
                engine.camera.position.x,
                engine.camera.position.y,
                engine.camera.position.z,
            ];
            engine.update(dt, -9.8, camera_pos, view_proj.to_cols_array());
        }

        // ===== РЕНДЕР =====
        // ИСПРАВЛЕНО (см. предыдущие правки engine.rs): render_frame()
        // теперь честно возвращает Err при реальной проблеме (например,
        // потеря устройства/TDR), вместо того чтобы молча проглатывать
        // ошибку и продолжать рендерить в мёртвое устройство. Причина уже
        // залогирована внутри render_frame(); здесь мы просто прекращаем
        // цикл, чтобы дойти до engine.shutdown() и корректно всё
        // освободить.
        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN] Render error, stopping: {:?}", e);
            break;
        }

        frame_count += 1;
        fps_window_frames += 1;

        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
            println!(
                "*** Camera pos: ({:.2}, {:.2}, {:.2}) ***",
                engine.camera.position.x, engine.camera.position.y, engine.camera.position.z
            );
            println!(
                "*** {} mesh_instances + {} ECS entities rendering ***\n",
                engine.mesh_instances.len(),
                engine.scene.len()
            );
        }

        // Печатаем реальный FPS раз в секунду (а не "раз в N кадров", как
        // было раньше, — так число не зависит от того, насколько быстрый
        // сейчас кадр).
        if fps_window_start.elapsed().as_secs_f32() >= 1.0 {
            let fps = fps_window_frames as f32 / fps_window_start.elapsed().as_secs_f32();
            println!("[INFO] Frame {} | FPS: {:.1}", frame_count, fps);
            fps_window_frames = 0;
            fps_window_start = Instant::now();
        }
    }

    println!("\n=== RENDER LOOP STOPPED (frames rendered: {}) ===", frame_count);
}