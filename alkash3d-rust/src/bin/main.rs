// src/bin/main.rs
//! Alkash3D Engine - Main Entry Point

use alkash3d_rs::engine::*;
use alkash3d_rs::*;
use std::time::Instant;

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║              Alkash3D Engine v{}                          ║", VERSION);
    println!("║                    DirectX 12 + Rust                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ===================================================================
    // 1. Инициализация D3D12
    // ===================================================================
    println!("[1] Initializing D3D12...");
    let device = create_device();
    if device.is_null() {
        eprintln!("❌ Failed to create D3D12 device!");
        return;
    }

    println!("    GPU: {:?}", unsafe {
        let ptr = get_gpu_name(std::ptr::null_mut());
        if ptr.is_null() { "Unknown" } else {
            std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("Unknown")
        }
    });
    println!("    VRAM: {} MB", get_gpu_vram_mb());
    println!("    Real GPU: {}", is_real_gpu());

    // ===================================================================
    // 2. Создание движка
    // ===================================================================
    println!("\n[2] Creating Engine...");
    let mut engine = AlkashEngine::new(device);
    let scheduler = EngineScheduler::new();
    println!("    CPU cores: {}", num_cpus::get());
    println!("    Available cores (budget): {}", scheduler.cpu_budget.available_cores());

    // ===================================================================
    // 3. Загрузка плагинов
    // ===================================================================
    println!("\n[3] Loading Plugins...");

    let physics_config = PhysicsConfig {
        max_bodies: 10000,
        world_size: 1000.0,
        cell_size: 10.0,
        solver_iterations: 8,
        use_simd: 1,
    };

    match engine.init_physics(physics_config) {
        Ok(_) => println!("    ✅ Physics plugin loaded (inertial.dll)"),
        Err(e) => println!("    ⚠️ Physics plugin not loaded: {}", e),
    }

    // ===================================================================
    // 4. Добавление тестовых тел
    // ===================================================================
    println!("\n[4] Adding Test Bodies...");

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
    println!("    Added {} bodies", body_count);

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

    // ===================================================================
    // 5. Симуляция
    // ===================================================================
    println!("\n[5] Running Simulation (120 frames)...");

    let dt = 1.0 / 60.0;
    let gravity = -9.81;
    let mut frame_times = Vec::new();

    for frame in 0..120 {
        let frame_start = Instant::now();

        engine.update_physics(dt, gravity);

        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        frame_times.push(frame_time);

        if frame % 30 == 0 {
            let stats = engine.get_physics_stats();
            let contacts = engine.get_physics_contacts();
            println!("    Frame {}: {} bodies, {} contacts, time: {:.2}ms",
                     frame, stats.bodies_count, contacts.len(), frame_time);
        }
    }

    // ===================================================================
    // 6. Статистика
    // ===================================================================
    println!("\n[6] Statistics:");

    let stats = engine.get_physics_stats();
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
    println!("    Frame Times:");
    println!("      Average: {:.2}ms ({:.1} FPS)", avg_time, 1000.0 / avg_time);
    println!("      Min: {:.2}ms", min_time);
    println!("      Max: {:.2}ms", max_time);

    // ===================================================================
    // 7. Очистка
    // ===================================================================
    println!("\n[7] Cleanup...");
    force_cleanup();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    Engine Shutdown Complete!                  ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}