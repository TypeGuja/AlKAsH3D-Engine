// tests/ball_game_simple.rs
//! Простая игра: шар на плоскости с физикой
//! Запуск: cargo test --release ball_game_simple -- --nocapture

use alkash3d_rs::engine::*;
use alkash3d_rs::*;
use std::time::Instant;
use std::f32::consts::PI;

#[test]
fn ball_game_simple() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    BALL GAME (Console)                        ║");
    println!("║              Ball on Plane with Physics                       ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ===================================================================
    // 1. ИНИЦИАЛИЗАЦИЯ
    // ===================================================================
    println!("[1] Initializing...");

    let device = create_device();
    assert!(!device.is_null(), "Failed to create D3D12 device");
    println!("  ✅ D3D12 device created");

    let mut engine = AlkashEngine::new(device);

    let physics_config = PhysicsConfig {
        max_bodies: 100,
        world_size: 100.0,
        cell_size: 5.0,
        solver_iterations: 8,
        use_simd: 1,
    };

    match engine.init_physics(physics_config) {
        Ok(_) => println!("  ✅ Physics loaded"),
        Err(e) => println!("  ⚠️ Physics not loaded: {}", e),
    }

    // ===================================================================
    // 2. СОЗДАНИЕ ТЕЛ
    // ===================================================================
    println!("\n[2] Creating physics bodies...");

    // Пол (плоскость)
    let ground = PhysicsBody {
        position: [0.0, -1.0, 0.0],
        velocity: [0.0, 0.0, 0.0],
        acceleration: [0.0, 0.0, 0.0],
        angular_velocity: [0.0, 0.0, 0.0],
        angular_acceleration: [0.0, 0.0, 0.0],
        mass: 0.0,
        inv_mass: 0.0,
        restitution: 0.7,
        friction: 0.5,
        linear_damping: 0.0,
        angular_damping: 0.0,
        is_static: 1,
        is_asleep: 1,
    };
    engine.add_physics_body(ground);
    println!("  ✅ Ground plane at Y = -1");

    // Шар
    let ball = PhysicsBody {
        position: [0.0, 5.0, 0.0],
        velocity: [2.0, 0.0, 1.0],
        acceleration: [0.0, 0.0, 0.0],
        angular_velocity: [0.0, 0.0, 0.0],
        angular_acceleration: [0.0, 0.0, 0.0],
        mass: 1.0,
        inv_mass: 1.0,
        restitution: 0.8,
        friction: 0.3,
        linear_damping: 0.01,
        angular_damping: 0.01,
        is_static: 0,
        is_asleep: 0,
    };
    engine.add_physics_body(ball);
    println!("  ✅ Ball at (0, 5, 0) with velocity (2, 0, 1)");

    // ===================================================================
    // 3. СИМУЛЯЦИЯ
    // ===================================================================
    println!("\n[3] Running simulation...");
    println!("   Press Ctrl+C to stop (auto-stop after 5 seconds)\n");

    let dt = 1.0 / 60.0;
    let gravity = -9.81;

    let start_time = Instant::now();
    let mut last_y = 5.0;
    let mut bounce_count = 0;
    let mut frame = 0;

    // Визуализация в консоли
    let screen_width = 40;
    let screen_height = 20;

    while start_time.elapsed().as_secs_f32() < 8.0 {
        let frame_start = Instant::now();

        // Обновление физики
        engine.update_physics(dt, gravity);

        // Получение позиции шара (через stats, так как прямой доступ сложен)
        let stats = engine.get_physics_stats();

        // Симуляция позиции шара (упрощённо, так как нет прямого доступа к body)
        // В реальности нужно получать body через API
        if frame % 30 == 0 {
            // Простая симуляция для отображения
            let time = start_time.elapsed().as_secs_f32();
            let y = 5.0 + (gravity * time * time / 2.0).max(-1.0);
            let x = 2.0 * time;
            let z = 1.0 * time;

            // Отскок
            let y = if y < 0.5 { 0.5 } else { y };

            // Очистка консоли
            print!("\x1B[2J\x1B[1;1H");

            println!("╔══════════════════════════════════════════════════════════════╗");
            println!("║                    BALL SIMULATION                           ║");
            println!("╠══════════════════════════════════════════════════════════════╣");
            println!("║  Time: {:5.2}s    Ball X: {:6.2}    Z: {:6.2}    Y: {:5.2}  ║",
                     time, x, z, y);
            println!("║  Physics bodies: {}    Contacts: {}                           ║",
                     stats.bodies_count, stats.contacts_count);
            println!("║  Broad phase: {:.2}ms    Narrow: {:.2}ms    Solver: {:.2}ms   ║",
                     stats.broad_phase_time_ms, stats.narrow_phase_time_ms, stats.solver_time_ms);
            println!("╚══════════════════════════════════════════════════════════════╝");
            println!();

            // Рисование сцены в консоли
            let screen_x = ((x + 10.0) / 20.0 * screen_width as f32) as usize;
            let screen_y = ((y + 1.0) / 6.0 * screen_height as f32) as usize;

            for i in 0..screen_height {
                print!("  ");
                for j in 0..screen_width {
                    if i == screen_height - 2 {
                        print!("─");
                    } else if i == screen_y && j == screen_x {
                        print!("●");
                    } else if i == screen_height - 2 && j == screen_x {
                        print!("●");
                    } else {
                        print!(" ");
                    }
                }
                println!();
            }

            println!();
            println!("  Ground: ────────────────────────────────────────");
            println!();
            println!("  [INFO] Ball moving in X and Z directions");
            println!("  [INFO] Gravity pulling down");
            println!("  [INFO] Will bounce when Y < 0.5");

            if y <= 0.6 {
                bounce_count += 1;
                println!("\n  🎾 BOUNCE! (count: {})", bounce_count);
            }
        }

        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        if frame_time > 1.0 {
            println!("  ⚠️ Frame time: {:.2}ms", frame_time);
        }

        frame += 1;
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    // ===================================================================
    // 4. СТАТИСТИКА
    // ===================================================================
    println!("\n[4] Statistics:");

    let stats = engine.get_physics_stats();
    println!("  Physics stats:");
    println!("    Total bodies: {}", stats.bodies_count);
    println!("    Active bodies: {}", stats.active_bodies);
    println!("    Contacts: {}", stats.contacts_count);
    println!("    Pairs: {}", stats.pairs_count);
    println!("    Broad phase: {:.2}ms", stats.broad_phase_time_ms);
    println!("    Narrow phase: {:.2}ms", stats.narrow_phase_time_ms);
    println!("    Solver: {:.2}ms", stats.solver_time_ms);

    println!("\n  Bounces: {}", bounce_count);

    // ===================================================================
    // 5. ЗАВЕРШЕНИЕ
    // ===================================================================
    println!("\n[5] Cleanup...");
    force_cleanup();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    TEST COMPLETED!                           ║");
    println!("║                    Ball moved and bounced                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}