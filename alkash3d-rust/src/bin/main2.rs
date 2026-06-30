// src/bin/main1.rs
//! 3D версия с камерой и трансформациями

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{} - 3D Mode", alkash3d_rs::VERSION);
    println!("==========================================");

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    engine.init()?;

    setup_3d_scene(&mut engine);
    run_3d_loop(&mut engine);
    engine.shutdown();
    println!("[MAIN] Goodbye!");

    Ok(())
}

fn setup_3d_scene(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up 3D scene...");

    // --- Куб в центре (главный объект) ---
    let cube_idx = engine.add_cube(1.2);
    engine.mesh_instances.push(
        MeshInstance::new(cube_idx)
            .at(0.0, 0.0, 0.0)
            .rotated(0.0, 0.5, 0.0)
    );

    // --- Куб справа (летающий) ---
    let cube2_idx = engine.add_cube(0.8);
    engine.mesh_instances.push(
        MeshInstance::new(cube2_idx)
            .at(2.5, 0.0, 0.0)
            .rotated(0.0, 0.3, 0.3)
            .scaled(1.5, 1.0, 0.5)
    );

    // --- Куб слева ---
    let cube3_idx = engine.add_cube(0.6);
    engine.mesh_instances.push(
        MeshInstance::new(cube3_idx)
            .at(-2.5, 0.5, 1.0)
            .rotated(0.7, 0.0, 0.0)
    );

    // --- Куб позади (высокий) ---
    let cube4_idx = engine.add_cube(0.5);
    engine.mesh_instances.push(
        MeshInstance::new(cube4_idx)
            .at(0.0, 0.0, -2.5)
            .rotated(0.2, 0.8, 0.1)
            .scaled(1.0, 2.0, 1.0)
    );

    // --- Ещё один куб (мелкий, прыгающий) ---
    let cube5_idx = engine.add_cube(0.4);
    engine.mesh_instances.push(
        MeshInstance::new(cube5_idx)
            .at(-1.5, 0.0, 1.5)
            .rotated(0.0, 0.0, 0.0)
    );

    // --- Пол (квадрат) ---
    let floor_idx = engine.add_quad(0.0, 0.0, 8.0, 8.0, [0.3, 0.3, 0.4, 1.0]);
    engine.mesh_instances.push(
        MeshInstance::new(floor_idx)
            .at(0.0, -0.6, 0.0)
            .rotated(-1.57, 0.0, 0.0)
            .scaled(1.0, 1.0, 1.0)
    );

    engine.set_clear_color(0.05, 0.05, 0.1, 1.0);

    println!("3D scene has {} meshes, {} instances",
             engine.meshes.len(), engine.mesh_instances.len());
}

fn run_3d_loop(engine: &mut AlkashEngine) {
    println!("\n=== 3D RENDER LOOP STARTING ===\n");
    use std::time::Instant;

    let mut frame_count = 0u32;
    let mut time = 0.0f32;
    let start = Instant::now();

    // ===== КАМЕРА СНАРУЖИ =====
    // Ставим камеру ДАЛЕКО, чтобы видеть всю сцену
    engine.camera.position = [0.0, 0.0, 15.0];  // Дальше и выше
    engine.camera.target = [0.0, 0.0, 0.0];

    while engine.is_running() {
        engine.process_messages();

        let dt = start.elapsed().as_secs_f32() - time;
        time = start.elapsed().as_secs_f32();

        // ===== КАМЕРА ВРАЩАЕТСЯ ВОКРУГ СЦЕНЫ =====
        /// Возможная проблема Pos точки камеры
        let radius = -1.0;
        let angle = time * 0.15;
        let height = 0.0 + (time * 0.2).sin() * 1.0;

        engine.camera.position = [
            radius * angle.sin(),
            height,
            radius * angle.cos(),
        ];
        engine.camera.target = [0.0, 0.0, 0.0];

        // === АНИМАЦИЯ ОБЪЕКТОВ ===
        // Главный куб - вращается
        if let Some(instance) = engine.mesh_instances.get_mut(0) {
            instance.rotation[1] = time * 0.7;
            instance.rotation[0] = (time * 0.3).sin() * 0.3;
        }

        // Правый куб - прыгает и вращается
        if let Some(instance) = engine.mesh_instances.get_mut(1) {
            instance.rotation[1] = time * 0.4;
            instance.position[1] = 0.5 + (time * 1.5).sin() * 0.5;
        }

        // Левый куб - вращается вокруг оси Z
        if let Some(instance) = engine.mesh_instances.get_mut(2) {
            instance.rotation[2] = time * 0.5;
        }

        // Задний куб - движется вперёд-назад
        if let Some(instance) = engine.mesh_instances.get_mut(3) {
            instance.rotation[1] = time * 0.6;
            instance.position[2] = -2.5 + (time * 0.8).sin() * 0.5;
        }

        // Мелкий куб - прыгает
        if let Some(instance) = engine.mesh_instances.get_mut(4) {
            instance.position[1] = 0.5 + (time * 2.0).sin().abs() * 0.8;
            instance.rotation[0] = time * 1.2;
            instance.rotation[1] = time * 0.9;
        }

        // === РЕНДЕР ===
        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN] Render error: {:?}", e);
            break;
        }

        frame_count += 1;

        if frame_count == 1 {
            println!("*** FIRST 3D FRAME COMPLETED ***");
            println!("*** Camera pos: ({:.2}, {:.2}, {:.2}) ***",
                     engine.camera.position[0],
                     engine.camera.position[1],
                     engine.camera.position[2]);
            println!("*** Rendering {} instances ***\n", engine.mesh_instances.len());
        }

        if frame_count % 60 == 0 {
            println!("[INFO] Frame {} rendered, {} instances",
                     frame_count, engine.mesh_instances.len());
        }
    }
}