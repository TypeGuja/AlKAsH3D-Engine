// examples/basic_simulator.rs
// Вместо use inertial::*; используем правильное имя крейта

use alkash3d_inertial::*;

fn main() {
    println!("=== Inertial Physics Demo ===\n");

    // Создаём физический мир
    let mut world = PhysicsWorld::new();

    // Добавляем падающие сферы
    for i in 0..10 {
        let ball = RigidBody::new(
            1.0,
            Vector3::new(i as f32 - 5.0, 10.0, 0.0),
        );
        let id = world.add_body(ball);
        println!("Added ball {} with id {}", i, id);
    }

    // Симуляция 3 секунд
    let dt = 1.0 / 60.0;

    for step in 0..180 {
        world.update(dt);

        if step % 60 == 0 {
            let stats = world.get_stats();
            println!("Frame {}: {} bodies, {} active, {} collisions",
                     step, stats.bodies_count, stats.active_bodies, stats.collisions_detected);
        }
    }

    let stats = world.get_stats();
    println!("\n=== Final Stats ===");
    println!("Total bodies: {}", stats.bodies_count);
    println!("Active: {}", stats.active_bodies);
    println!("Total collisions detected: {}", stats.collisions_detected);
}