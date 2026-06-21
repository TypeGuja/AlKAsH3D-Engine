// src/bin/main1.rs
//! Альтернативная версия с настройками

use alkash3d_rs::engine::AlkashEngine;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{} - Starting...", alkash3d_rs::VERSION);
    println!("==========================================");

    // Создаем и инициализируем движок
    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    engine.init()?;

    // Настраиваем сцену
    setup_scene(&mut engine);

    // Запускаем основной цикл
    run_loop(&mut engine);

    // Завершаем работу
    engine.shutdown();
    println!("[MAIN] Goodbye!");

    Ok(())
}

fn setup_scene(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up scene...");

    engine.add_triangle();
    engine.add_quad(0.5, -0.5, 0.4, 0.4, [0.0, 1.0, 1.0, 1.0]);
    engine.add_cube(0.5);
    engine.set_clear_color(0.05, 0.05, 0.1, 1.0);

    println!("Scene has {} meshes", engine.meshes.len());
}

fn run_loop(engine: &mut AlkashEngine) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    let mut frame_count = 0u32;

    while engine.is_running() {
        engine.process_messages();

        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN] Render error: {:?}", e);
            break;
        }

        frame_count += 1;

        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
            println!("*** Rendering {} meshes ***\n", engine.meshes.len());
        }

        if frame_count % 60 == 0 {
            println!("[INFO] Frame {} rendered, {} meshes",
                     frame_count, engine.meshes.len());
        }
    }
}