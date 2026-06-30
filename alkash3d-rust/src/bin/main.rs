// src/bin/main.rs
//! 3D версия с камерой и трансформациями

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};
use alkash3d_rs::math::Vec3;
use std::time::Instant;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Alkash3D Engine v{} - 3D Mode", alkash3d_rs::VERSION);
    println!("==========================================");
    println!();
    println!("🎮 УПРАВЛЕНИЕ:");
    println!("  WASD - движение камеры");
    println!("  Q/E - подняться/опуститься");
    println!("  Стрелки - поворот камеры");
    println!("  SHIFT - ускорение (x2)");
    println!("  ESC - выход");
    println!("==========================================");

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    engine.init()?;

    setup_scene(&mut engine);
    run_loop(&mut engine);
    engine.shutdown();
    println!("[MAIN] Goodbye!");

    Ok(())
}

fn setup_scene(engine: &mut AlkashEngine) {
    println!("\n[MAIN] Setting up scene...");

    // ===== ВАЖНО: СНАЧАЛА СОЗДАЕМ ВСЕ МЕШИ =====
    let cube1_idx = engine.add_cube(1.2);
    let cube2_idx = engine.add_cube(0.8);
    let cube3_idx = engine.add_cube(0.6);
    let cube4_idx = engine.add_cube(0.5);
    let cube5_idx = engine.add_cube(0.4);
    let floor_idx = engine.add_quad(0.0, 0.0, 8.0, 8.0, [0.3, 0.3, 0.4, 1.0]);

    // ===== ПОТОМ СОЗДАЕМ ЭКЗЕМПЛЯРЫ =====
    // Куб в центре
    engine.mesh_instances.push(
        MeshInstance::new(cube1_idx)
            .at(0.0, 0.0, 0.0)
            .rotated(0.0, 0.5, 0.0)
    );

    // Куб справа
    engine.mesh_instances.push(
        MeshInstance::new(cube2_idx)
            .at(2.5, 0.0, 0.0)
            .rotated(0.0, 0.3, 0.3)
            .scaled(1.5, 1.0, 0.5)
    );

    // Куб слева
    engine.mesh_instances.push(
        MeshInstance::new(cube3_idx)
            .at(-2.5, 0.5, 1.0)
            .rotated(0.7, 0.0, 0.0)
    );

    // Куб позади
    engine.mesh_instances.push(
        MeshInstance::new(cube4_idx)
            .at(0.0, 0.0, -2.5)
            .rotated(0.2, 0.8, 0.1)
            .scaled(1.0, 2.0, 1.0)
    );

    // Мелкий куб
    engine.mesh_instances.push(
        MeshInstance::new(cube5_idx)
            .at(-1.5, 0.0, 1.5)
            .rotated(0.0, 0.0, 0.0)
    );

    // Пол
    engine.mesh_instances.push(
        MeshInstance::new(floor_idx)
            .at(0.0, -0.6, 0.0)
            .rotated(-1.57, 0.0, 0.0)
    );

    engine.set_clear_color(0.05, 0.05, 0.1, 1.0);

    println!("✅ Scene has {} meshes, {} instances",
             engine.meshes.len(), engine.mesh_instances.len());
}

fn run_loop(engine: &mut AlkashEngine) {
    println!("\n=== RENDER LOOP STARTING ===\n");

    let mut frame_count = 0u32;
    let mut time = 0.0f32;
    let start = Instant::now();

    // Начальная позиция камеры
    engine.camera.position = Vec3::new(0.0, 4.0, 10.0);
    engine.camera.target = Vec3::new(0.0, 0.0, 0.0);

    let mut move_speed = 5.0;
    let rot_speed = 2.0;

    while engine.is_running() {
        engine.process_messages();

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
            move_speed = if shift { 10.0 } else { 5.0 };

            let dt = start.elapsed().as_secs_f32() - time;
            time = start.elapsed().as_secs_f32();
            let dt = dt.min(0.05);

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

        // ===== АНИМАЦИЯ (опционально, закомментируй если не нужно) =====
        // Главный куб - вращается
        if let Some(instance) = engine.mesh_instances.get_mut(0) {
            instance.rotation[1] = time * 0.7;
            instance.rotation[0] = (time * 0.3).sin() * 0.3;
        }

        // Правый куб - прыгает
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

        if let Err(e) = engine.render_frame() {
            eprintln!("[MAIN] Render error: {:?}", e);
            break;
        }

        frame_count += 1;

        if frame_count == 1 {
            println!("*** FIRST FRAME COMPLETED ***");
            println!("*** Camera pos: ({:.2}, {:.2}, {:.2}) ***",
                     engine.camera.position.x,
                     engine.camera.position.y,
                     engine.camera.position.z);
            println!("*** Rendering {} instances ***\n", engine.mesh_instances.len());
        }

        if frame_count % 60 == 0 {
            println!("[INFO] Frame {} rendered, {} instances",
                     frame_count, engine.mesh_instances.len());
        }
    }
}