// src/bin/engine_test.rs
//! Консольный тест движка Alkash3D - проверка плагинов

use alkash3d_rs::engine::*;
use alkash3d_rs::*;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║              Alkash3D Engine Console Test                     ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ===================================================================
    // 1. Инициализация D3D12 (нужен для движка)
    // ===================================================================
    println!("[1] Initializing D3D12...");
    let device_ptr = create_device();
    if device_ptr.is_null() {
        eprintln!("❌ Failed to create D3D12 device!");
        return;
    }
    println!("    ✅ Device created at {:p}", device_ptr);

    // ===================================================================
    // 2. Создание движка
    // ===================================================================
    println!("\n[2] Creating Engine...");
    let mut engine = AlkashEngine::new(device_ptr);
    println!("    ✅ Engine created");

    // ===================================================================
    // 3. Загрузка плагина физики (если есть)
    // ===================================================================
    println!("\n[3] Loading Physics Plugin...");

    let physics_config = PhysicsConfig {
        max_bodies: 100,
        world_size: 100.0,
        cell_size: 10.0,
        solver_iterations: 8,
        use_simd: 1,
    };

    match engine.init_physics(physics_config) {
        Ok(_) => println!("    ✅ Physics plugin loaded"),
        Err(e) => println!("    ⚠️ Physics plugin not loaded: {}", e),
    }

    // ===================================================================
    // 4. Загрузка плагина света (если есть)
    // ===================================================================
    println!("\n[4] Loading Light Plugin...");

    let light_config = LightConfig {
        max_lights: 1000,
        tile_size: 16,
        far_plane: 1000.0,
        lod_distances: [50.0, 100.0, 200.0],
        grid_cell_size: 32.0,
    };

    match engine.init_lights(light_config) {
        Ok(_) => println!("    ✅ Light plugin loaded"),
        Err(e) => println!("    ⚠️ Light plugin not loaded: {}", e),
    }

    // ===================================================================
    // 5. Добавление тестовых тел
    // ===================================================================
    println!("\n[5] Adding Test Bodies...");

    let mut body_count = 0;
    for i in 0..10 {
        let x = (i as f32 - 5.0) * 2.0;
        let body = PhysicsBody {
            position: [x, 10.0 + (i % 3) as f32 * 2.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            acceleration: [0.0, 0.0, 0.0],
            angular_velocity: [0.0, 0.0, 0.0],
            angular_acceleration: [0.0, 0.0, 0.0],
            mass: 1.0,
            inv_mass: 1.0,
            restitution: 0.5,
            friction: 0.5,
            linear_damping: 0.01,
            angular_damping: 0.01,
            is_static: 0,
            is_asleep: 0,
        };
        let id = engine.add_physics_body(body);
        if id >= 0 {
            body_count += 1;
            println!("    Body {}: position ({:.1}, {:.1}, {:.1})",
                     id, body.position[0], body.position[1], body.position[2]);
        }
    }

    // Добавляем пол
    let ground = PhysicsBody {
        position: [0.0, -1.0, 0.0],
        velocity: [0.0, 0.0, 0.0],
        acceleration: [0.0, 0.0, 0.0],
        angular_velocity: [0.0, 0.0, 0.0],
        angular_acceleration: [0.0, 0.0, 0.0],
        mass: 0.0,
        inv_mass: 0.0,
        restitution: 0.5,
        friction: 0.8,
        linear_damping: 0.0,
        angular_damping: 0.0,
        is_static: 1,
        is_asleep: 1,
    };
    engine.add_physics_body(ground);
    println!("    Ground added");
    println!("    Total bodies: {}", body_count + 1);

    // ===================================================================
    // 6. Добавление тестовых источников света
    // ===================================================================
    println!("\n[6] Adding Test Lights...");

    let mut light_count = 0;
    for i in 0..5 {
        let x = (i as f32 - 2.0) * 5.0;
        let light = GPULight {
            position: [x, 5.0, 0.0, 0.0],
            color: [1.0, 0.8 + i as f32 * 0.05, 0.6, 2.0],
            direction: [0.0, -1.0, 0.0, 20.0],
            params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
        };
        let id = engine.add_light(light);
        if id != u32::MAX {
            light_count += 1;
            println!("    Light {}: position ({:.1}, {:.1}, {:.1})", id, light.position[0], light.position[1], light.position[2]);
        }
    }
    println!("    Total lights: {}", light_count);

    // ===================================================================
    // 7. Симуляция
    // ===================================================================
    println!("\n[7] Running Simulation (60 frames)...");

    let dt = 1.0 / 60.0;
    let gravity = -9.81;
    let camera_pos = [0.0, 5.0, 15.0];
    let view_proj = mat4_identity_for_test();

    let start = Instant::now();
    let mut frame_times = Vec::new();

    for frame in 0..60 {
        let frame_start = Instant::now();

        // ОБНОВЛЕНИЕ ФИЗИКИ (правильный метод)
        engine.update_physics(dt, gravity);

        // ОБНОВЛЕНИЕ СВЕТА (если есть)
        engine.update_lights(camera_pos, &view_proj, dt);

        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        frame_times.push(frame_time);

        if frame % 15 == 0 {
            let stats = engine.get_physics_stats();
            let contacts = engine.get_physics_contacts();
            let lights = engine.get_gpu_lights();
            println!("    Frame {}: {} bodies, {} contacts, {} lights, time: {:.2}ms",
                     frame, stats.bodies_count, contacts.len(), lights.len(), frame_time);
        }

        sleep(Duration::from_millis(16));
    }

    // ===================================================================
    // 8. Статистика
    // ===================================================================
    println!("\n[8] Statistics:");

    let stats = engine.get_physics_stats();
    let light_stats = engine.get_light_stats();
    let avg_time = frame_times.iter().sum::<f32>() / frame_times.len() as f32;
    let min_time = frame_times.iter().fold(f32::MAX, |a, &b| a.min(b));
    let max_time = frame_times.iter().fold(f32::MIN, |a, &b| a.max(b));

    println!("    Physics Stats:");
    println!("      Bodies: {}", stats.bodies_count);
    println!("      Active: {}", stats.active_bodies);
    println!("      Contacts: {}", stats.contacts_count);
    println!("      Pairs: {}", stats.pairs_count);
    println!("      Broad phase: {:.2}ms", stats.broad_phase_time_ms);
    println!("      Narrow phase: {:.2}ms", stats.narrow_phase_time_ms);
    println!("      Solver: {:.2}ms", stats.solver_time_ms);

    println!("    Light Stats:");
    println!("      Total lights: {}", light_stats.total_lights);
    println!("      Visible lights: {}", light_stats.visible_lights);
    println!("      Culled by frustum: {}", light_stats.culled_by_frustum);
    println!("      Culled by distance: {}", light_stats.culled_by_distance);
    println!("      Culling time: {:.2}ms", light_stats.culling_time_ms);

    println!("    Frame Times:");
    println!("      Average: {:.2}ms ({:.1} FPS)", avg_time, 1000.0 / avg_time);
    println!("      Min: {:.2}ms", min_time);
    println!("      Max: {:.2}ms", max_time);

    // ===================================================================
    // 9. Очистка
    // ===================================================================
    println!("\n[9] Cleanup...");
    force_cleanup();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    Test Complete!                             ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}

// Упрощённая единичная матрица для теста
fn mat4_identity_for_test() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]
}