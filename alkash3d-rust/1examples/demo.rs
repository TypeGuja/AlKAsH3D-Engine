// 1examples/demo.rs
//! Демонстрационная программа для Alkash3D Engine

use alkash3d_rs::*;
use std::time::Instant;
use std::thread;
use std::sync::Arc;

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                 Alkash3D Engine Demo                          ║");
    println!("║              DirectX 12 + Rust Game Engine                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ===================================================================
    // 1. Инициализация D3D12
    // ===================================================================
    println!("[1] Initializing D3D12...");
    let device = create_device();
    if device.is_null() {
        eprintln!("Failed to create D3D12 device!");
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
    // 2. Создание планировщика
    // ===================================================================
    println!("\n[2] Initializing Scheduler...");
    let scheduler = Arc::new(EngineScheduler::new());
    println!("    CPU cores: {}", num_cpus::get());
    println!("    Broad phase threshold: {}", scheduler.broad_phase_threshold());
    println!("    Narrow phase threshold: {}", scheduler.narrow_phase_threshold());

    // ===================================================================
    // 3. Создание движка
    // ===================================================================
    println!("\n[3] Creating Engine...");
    let mut engine = AlkashEngine::new();

    // ===================================================================
    // 4. Загрузка плагинов (если есть)
    // ===================================================================
    println!("\n[4] Loading Plugins...");

    let physics_config = PhysicsConfig {
        max_bodies: 10000,
        world_size: 1000.0,
        cell_size: 10.0,
        solver_iterations: 8,
        use_simd: 1,
    };

    match engine.init_physics(physics_config) {
        Ok(_) => println!("    ✅ Physics plugin loaded (Inertial.dll)"),
        Err(e) => println!("    ⚠️ Physics plugin not loaded: {}", e),
    }

    let light_config = LightConfig {
        max_lights: 4096,
        tile_size: 16,
        far_plane: 1000.0,
        lod_distances: [50.0, 150.0, 300.0],
        grid_cell_size: 32.0,
    };

    match engine.init_lights(device, light_config) {
        Ok(_) => println!("    ✅ Light plugin loaded (FirstFires.dll)"),
        Err(e) => println!("    ⚠️ Light plugin not loaded: {}", e),
    }

    // ===================================================================
    // 5. Создание тестовых объектов
    // ===================================================================
    println!("\n[5] Creating Test Objects...");

    // Добавляем сферы в сетку 10x10
    let mut body_count = 0;
    for x in -5..5 {
        for z in -5..5 {
            let x_pos = x as f32 * 2.0;
            let z_pos = z as f32 * 2.0;
            if let Some(id) = engine.add_sphere_body(x_pos, 5.0, z_pos, 1.0) {
                body_count += 1;
            }
        }
    }
    println!("    Added {} physics bodies", body_count);

    // Добавляем уличные фонари по кругу
    let mut light_count = 0;
    for i in 0..20 {
        let angle = 2.0 * std::f32::consts::PI * (i as f32) / 20.0;
        let x = angle.cos() * 15.0;
        let z = angle.sin() * 15.0;
        if let Some(id) = engine.add_street_light(x, 3.0, z) {
            light_count += 1;
        }
    }
    println!("    Added {} street lights", light_count);

    // ===================================================================
    // 6. Тестирование форматов файлов
    // ===================================================================
    println!("\n[6] Testing File Formats...");

    // Тест .altex
    let mut altex = AltexFile::new();
    let vertices = vec![
        Vertex { position: [-1.0, -1.0, -1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        Vertex { position: [ 1.0, -1.0, -1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        Vertex { position: [ 1.0, -1.0,  1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        Vertex { position: [-1.0, -1.0,  1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
    ];
    let indices = vec![0,1,2, 0,2,3];
    altex.add_mesh(vertices, indices, "Quad");
    let _ = altex.save("test_quad.altex");
    println!("    ✅ Created test_quad.altex");

    // Тест .alcar
    let sports_car = AlcarFile::create_sports_car();
    let _ = sports_car.save("test_sports_car.alcar");
    println!("    ✅ Created test_sports_car.alcar ({} HP)", sports_car.physics.engine_power);

    // Тест .aluv
    let cinematic = AluvFile::create_opening_cinematic();
    let _ = cinematic.save("test_cinematic.aluv");
    println!("    ✅ Created test_cinematic.aluv ({} ms)", cinematic.header.total_duration_ms);

    // ===================================================================
    // 7. Симуляционный цикл
    // ===================================================================
    println!("\n[7] Running Simulation (100 frames)...");

    let mut last_time = Instant::now();
    let mut frame_times = Vec::with_capacity(100);

    for frame in 0..100 {
        let dt = last_time.elapsed().as_secs_f32().min(0.033);
        last_time = Instant::now();

        let frame_start = Instant::now();

        // Камера движется по кругу
        let angle = frame as f32 * 0.05;
        let cam_x = angle.sin() * 30.0;
        let cam_z = angle.cos() * 30.0;
        let camera = [cam_x, 15.0, cam_z];

        // Простая projection matrix (perspective)
        let fov = 3.14159 / 3.0; // 60 degrees
        let aspect = 16.0 / 9.0;
        let near = 0.1;
        let far = 1000.0;

        let f = 1.0 / (fov / 2.0).tan();
        let view_proj = [
            f / aspect, 0.0, 0.0, 0.0,
            0.0, f, 0.0, 0.0,
            0.0, 0.0, far / (far - near), 1.0,
            0.0, 0.0, -far * near / (far - near), 0.0,
        ];

        // Обновление
        engine.update(0.016, -9.81, camera, view_proj);

        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        frame_times.push(frame_time);

        if frame % 20 == 0 {
            println!("    Frame {}: {:.2} ms", frame, frame_time);
        }

        // Небольшая задержка для имитации реального времени
        thread::sleep(std::time::Duration::from_millis(16));
    }

    // ===================================================================
    // 8. Статистика
    // ===================================================================
    println!("\n[8] Statistics:");

    let avg_time = frame_times.iter().sum::<f32>() / frame_times.len() as f32;
    let min_time = frame_times.iter().fold(f32::MAX, |a, &b| a.min(b));
    let max_time = frame_times.iter().fold(f32::MIN, |a, &b| a.max(b));

    println!("    Average frame time: {:.2} ms ({:.1} FPS)", avg_time, 1000.0 / avg_time);
    println!("    Min frame time: {:.2} ms", min_time);
    println!("    Max frame time: {:.2} ms", max_time);
    println!("    Frames: {}", frame_times.len());

    // ===================================================================
    // 9. Очистка
    // ===================================================================
    println!("\n[9] Cleanup...");

    force_cleanup();

    // Удаляем тестовые файлы
    let _ = std::fs::remove_file("test_quad.altex");
    let _ = std::fs::remove_file("test_sports_car.alcar");
    let _ = std::fs::remove_file("test_cinematic.aluv");

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    Demo Completed!                           ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}