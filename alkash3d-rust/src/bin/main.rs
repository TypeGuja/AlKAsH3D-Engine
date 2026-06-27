// src/bin/main.rs
//! Полная 3D демка с анимацией и управлением камерой

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::{Vec3, Camera};
use alkash3d_rs::Transform;
const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{} - 3D Demo", alkash3d_rs::VERSION);
    println!("==========================================");
    println!("\n🎮 CONTROLS:");
    println!("  WASD  - Move camera");
    println!("  Q/E   - Move up/down");
    println!("  Mouse Wheel - Zoom");
    println!("  ESC   - Exit");
    println!("==========================================\n");

    // Создаем движок
    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);

    // Настраиваем камеру
    let camera = Camera::new(
        Vec3::new(0.0, 2.0, 8.0),  // Позиция
        Vec3::new(0.0, 0.0, 0.0),  // Цель
        Vec3::UP,                   // Вектор вверх
    )
        .with_aspect(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32)
        .with_fov(60.0)
        .with_clip(0.1, 100.0);

    engine.set_camera(camera);

    // Инициализируем движок
    engine.init()?;

    // Настраиваем 3D сцену
    setup_scene(&mut engine);

    // Запускаем основной цикл
    run_loop(&mut engine);

    // Завершаем работу
    engine.shutdown();
    println!("\n[MAIN] Goodbye!");

    Ok(())
}

fn setup_scene(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up 3D scene...");
    engine.set_clear_color(0.08, 0.08, 0.15, 1.0);

    // 1. Куб слева - будет вращаться
    let mut cube = alkash3d_rs::engine::Mesh::cube(1.2).unwrap();
    cube.set_transform(
        Transform::new()
            .with_position(-3.0, 0.0, 0.0)
            .with_rotation(0.3, 0.5, 0.0)
    );
    engine.add_mesh(cube);
    println!("  ✓ Added spinning cube at (-3, 0, 0)");

    // 2. Сфера справа
    let mut sphere = alkash3d_rs::engine::Mesh::sphere(0.8, 32).unwrap();
    sphere.set_transform(
        Transform::new()
            .with_position(3.0, 0.0, 0.0)
    );
    engine.add_mesh(sphere);
    println!("  ✓ Added sphere at (3, 0, 0)");

    // 3. Куб в центре - будет летать вверх-вниз
    let mut center_cube = alkash3d_rs::engine::Mesh::cube(1.0).unwrap();
    center_cube.set_transform(
        Transform::new()
            .with_position(0.0, 1.5, -1.0)
            .with_rotation(0.0, 0.0, 0.0)
            .with_scale(0.7, 0.7, 0.7)
    );
    engine.add_mesh(center_cube);
    println!("  ✓ Added center cube at (0, 1.5, -1)");

    // 4. Пол - большой плоский куб
    let mut ground = alkash3d_rs::engine::Mesh::cube(8.0).unwrap();
    // Делаем его плоским, растягивая по Y
    ground.set_transform(
        Transform::new()
            .with_position(0.0, -1.0, 0.0)
            .with_scale(1.0, 0.1, 1.0)
    );
    engine.add_mesh(ground);
    println!("  ✓ Added ground");

    // 5. Маленькие кубики вокруг
    for i in -2..=2 {
        for j in -2..=2 {
            if i == 0 && j == 0 { continue; }
            let x = i as f32 * 1.5;
            let z = j as f32 * 1.5 - 3.0;

            // Пропускаем места, где уже есть объекты
            if x.abs() < 0.5 && z.abs() < 0.5 { continue; }

            let mut small_cube = alkash3d_rs::engine::Mesh::cube(0.3).unwrap();
            small_cube.set_transform(
                Transform::new()
                    .with_position(x, -0.5 + (i as f32 * 0.1), z)
                    .with_rotation(i as f32 * 0.2, j as f32 * 0.3, 0.0)
            );
            engine.add_mesh(small_cube);
        }
    }
    println!("  ✓ Added small cubes around");

    println!("\n✅ Scene ready! Total meshes: {}", engine.meshes.len());
}

fn run_loop(engine: &mut AlkashEngine) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    let mut frame_count = 0u32;
    let start_time = std::time::Instant::now();
    let mut float_time: f32 = 0.0;  // Явно указываем тип f32

    while engine.is_running() {
        // Обрабатываем сообщения Windows
        engine.process_messages();

        // === АНИМАЦИЯ ===
        let dt: f32 = 1.0 / 60.0; // Явно указываем тип f32
        float_time += dt;

        // Вращаем первый куб
        if let Some(mesh) = engine.get_mesh_mut(0) {
            mesh.transform.rotate(0.0, 0.02, 0.0);
        }

        // Летающий куб в центре
        if let Some(mesh) = engine.get_mesh_mut(2) {
            let y_offset = (float_time * 0.8).sin() * 0.3;
            mesh.transform.position.y = 1.5 + y_offset;
            mesh.transform.rotate(0.02, 0.01, 0.01);
        }

        // === РЕНДЕРИНГ ===
        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN] Render error: {:?}", e);
            break;
        }

        frame_count += 1;

        // Статистика
        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
            println!("*** Rendering {} meshes in 3D ***\n", engine.meshes.len());
        }

        if frame_count % 120 == 0 {
            let elapsed = start_time.elapsed().as_secs_f32();
            let pos = engine.camera.position;
            println!("[INFO] Frame {} | Time: {:.1}s | Camera: ({:.1}, {:.1}, {:.1}) | FPS: {:.1}",
                     frame_count,
                     elapsed,
                     pos.x, pos.y, pos.z,
                     frame_count as f32 / elapsed
            );
        }
    }
}