// src/bin/main.rs
//! Alkash3D Engine — демонстрация: плоскость из множества кубов-плиток,
//! по которой можно ходить (не летать).
//!
//! Показывает:
//!  - ECS (`engine.scene`) для сотни+ объектов сетки пола (см. scene.rs);
//!  - новую систему ввода (`engine.input`, см. input.rs) вместо
//!    GetAsyncKeyState — движок сам ничего не решает про WASD/ESC, только
//!    отдаёт состояние клавиш, а что с ним делать, решает этот файл;
//!  - "ходьбу": движение заперто в горизонтальной плоскости (не зависит от
//!    того, куда камера смотрит по вертикали), высота глаз фиксирована.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use std::time::Instant;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

/// Сетка пола: (-GRID_HALF..=GRID_HALF) x (-GRID_HALF..=GRID_HALF) плиток.
const GRID_HALF: i32 = 6;
const TILE_SPACING: f32 = 1.0;
const TILE_HEIGHT: f32 = 0.2;
const GROUND_Y: f32 = 0.0;
const EYE_HEIGHT: f32 = 1.6;

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

    setup_scene(&mut engine);
    run_loop(&mut engine);

    engine.shutdown();
    println!("[MAIN] Goodbye!");
    Ok(())
}

fn setup_scene(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up scene...");

    // ===== ПЛОСКОСТЬ ИЗ МНОЖЕСТВА КУБОВ (не один большой quad) =====
    // Специально сделано именно кубами через ECS, а не одним Mesh::quad,
    // чтобы: (1) показать, что ECS нормально тянет сотни объектов, и
    // (2) чтобы пол реально состоял из отдельных "плиток" — как в
    // Minecraft-подобных играх, а не был одной цельной плоскостью.
    let tile_mesh = engine.add_cube(0.95); // чуть меньше TILE_SPACING — видны швы между плитками
    let mut tile_count = 0;
    for gx in -GRID_HALF..=GRID_HALF {
        for gz in -GRID_HALF..=GRID_HALF {
            let tile = engine.spawn_mesh_entity(tile_mesh);
            if let Some(t) = engine.scene.transform_mut(tile) {
                t.position = [gx as f32 * TILE_SPACING, GROUND_Y, gz as f32 * TILE_SPACING];
                t.scale = [1.0, TILE_HEIGHT, 1.0]; // приплюснутый куб — плитка пола
            }
            tile_count += 1;
        }
    }

    // Несколько высоких столбов по краям — просто ориентиры, чтобы во
    // время ходьбы было видно, что камера реально перемещается по сетке,
    // а не стоит на месте.
    let pillar_mesh = engine.add_cube(0.6);
    let half_extent = GRID_HALF as f32 * TILE_SPACING;
    let pillar_positions = [
        (half_extent, half_extent),
        (half_extent, -half_extent),
        (-half_extent, half_extent),
        (-half_extent, -half_extent),
    ];
    for (x, z) in pillar_positions {
        let pillar = engine.spawn_mesh_entity(pillar_mesh);
        if let Some(t) = engine.scene.transform_mut(pillar) {
            t.position = [x, GROUND_Y + 1.2, z];
            t.scale = [1.0, 4.0, 1.0];
        }
    }

    engine.set_clear_color(0.55, 0.7, 0.9, 1.0); // светлое небо, чтобы было видно горизонт

    println!(
        "✅ Scene ready: {} плиток пола + {} столбов ({} ECS-сущностей всего)",
        tile_count,
        pillar_positions.len(),
        engine.scene.len()
    );
}

fn run_loop(engine: &mut AlkashEngine) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    let mut frame_count: u64 = 0;
    let mut time = 0.0f32;
    let start = Instant::now();

    let mut fps_window_start = Instant::now();
    let mut fps_window_frames: u32 = 0;

    // Стартовая позиция — стоим на полу, смотрим чуть вперёд и вниз, на
    // сетку плиток.
    engine.camera.position = Vec3::new(0.0, EYE_HEIGHT, 4.0);
    engine.camera.target = Vec3::new(0.0, EYE_HEIGHT - 0.3, 0.0);

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

        // ===== РЕНДЕР =====
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
            println!("*** {} ECS entities rendering ***\n", engine.scene.len());
        }

        if fps_window_start.elapsed().as_secs_f32() >= 1.0 {
            let fps = fps_window_frames as f32 / fps_window_start.elapsed().as_secs_f32();
            println!("[INFO] Frame {} | FPS: {:.1}", frame_count, fps);
            fps_window_frames = 0;
            fps_window_start = Instant::now();
        }
    }

    println!("\n=== RENDER LOOP STOPPED (frames rendered: {}) ===", frame_count);
}
