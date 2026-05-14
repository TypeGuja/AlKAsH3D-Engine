// examples/stress_test.rs
use alkash3d_inertial::*;
use std::time::Instant;

fn main() {
    println!("═══════════════════════════════════════════════════════");
    println!("           INERTIAL PHYSICS ENGINE - STRESS TEST       ");
    println!("═══════════════════════════════════════════════════════\n");

    // Параметры теста
    let object_counts = [100, 500, 1000, 2000, 5000];
    let simulation_time = 5.0; // секунд
    let dt = 1.0 / 60.0;

    for &count in &object_counts {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🔥 TESTING WITH {} OBJECTS", count);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let mut world = PhysicsWorld::new();

        // Добавляем статическую землю (большую платформу)
        let ground = RigidBody::new(0.0, Vector3::new(0.0, -10.0, 0.0));
        world.add_body(ground);

        // Создаём N падающих объектов
        let start_time = Instant::now();

        for i in 0..count {
            let x = (i as f32 % 20.0) - 10.0;
            let z = (i as f32 / 20.0).floor() - 10.0;
            let y = 20.0 + (i % 5) as f32 * 2.0;

            let mut body = RigidBody::new(1.0, Vector3::new(x, y, z));
            body.restitution = 0.7;   // Упругость
            body.friction = 0.3;      // Трение
            world.add_body(body);
        }

        let creation_time = start_time.elapsed();
        println!("✓ Created {} objects in {:.2}ms", count, creation_time.as_secs_f32() * 1000.0);

        // Симуляция
        let sim_start = Instant::now();
        let steps = (simulation_time / dt) as u32;
        let mut collisions_total = 0;

        for step in 0..steps {
            world.update(dt);

            if step % 60 == 0 {
                let stats = world.get_stats();
                collisions_total += stats.collisions_detected;

                print!("\r  Frame {}/{} | Collisions: {} | Active: {}",
                       step, steps, stats.collisions_detected, stats.active_bodies);
            }
        }

        let sim_time = sim_start.elapsed();
        let stats = world.get_stats();

        println!("\n\n📊 RESULTS for {} objects:", count);
        println!("   ├─ Simulation time: {:.2}s", simulation_time);
        println!("   ├─ Actual time: {:.2}s", sim_time.as_secs_f32());
        println!("   ├─ Performance: {:.1}x realtime", simulation_time / sim_time.as_secs_f32());
        println!("   ├─ Total collisions: {}", collisions_total);
        println!("   ├─ Active bodies: {}", stats.active_bodies);
        println!("   └─ Bodies count: {}", stats.bodies_count);

        // Использование памяти (примерно)
        let memory_mb = (count * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0);
        println!("   └─ Memory: {:.2} MB", memory_mb);
    }

    // Экстремальный тест с 10000 объектов
    println!("\n\n═══════════════════════════════════════════════════════");
    println!("              EXTREME TEST: 10,000 OBJECTS");
    println!("═══════════════════════════════════════════════════════");

    let mut world = PhysicsWorld::new();

    // Земля
    let ground = RigidBody::new(0.0, Vector3::new(0.0, -10.0, 0.0));
    world.add_body(ground);

    // Кубическая сетка объектов
    let grid_size = 22; // 22x22x22 = 10648 объектов
    let spacing = 2.0;
    let offset = (grid_size as f32 * spacing) / 2.0;

    let start_time = Instant::now();
    let mut count = 0;

    for x in 0..grid_size {
        for y in 0..grid_size/2 {
            for z in 0..grid_size {
                let pos = Vector3::new(
                    x as f32 * spacing - offset,
                    y as f32 * spacing + 50.0,
                    z as f32 * spacing - offset,
                );

                let mut body = RigidBody::new(1.0, pos);
                body.restitution = 0.8;
                body.friction = 0.2;
                world.add_body(body);
                count += 1;

                if count >= 10000 { break; }
            }
            if count >= 10000 { break; }
        }
        if count >= 10000 { break; }
    }

    let creation_time = start_time.elapsed();
    println!("✓ Created {} objects in {:.2}s", count, creation_time.as_secs_f32());

    // Симуляция
    let sim_start = Instant::now();
    let dt = 1.0 / 60.0;
    let steps = 300; // 5 секунд

    for step in 0..steps {
        world.update(dt);

        if step % 30 == 0 {
            let stats = world.get_stats();
            print!("\r  Frame {}/{} | Collisions: {} | FPS: {:.0}",
                   step, steps, stats.collisions_detected,
                   1.0 / (sim_start.elapsed().as_secs_f32() / (step + 1) as f32));
        }
    }

    let sim_time = sim_start.elapsed();
    let stats = world.get_stats();

    println!("\n\n🎯 EXTREME TEST RESULTS:");
    println!("   ├─ Objects: {}", count);
    println!("   ├─ Simulation time: 5.00s");
    println!("   ├─ Actual time: {:.2}s", sim_time.as_secs_f32());
    println!("   ├─ Performance: {:.1}x realtime", 5.0 / sim_time.as_secs_f32());
    println!("   ├─ Total collisions: {}", stats.collisions_detected);
    println!("   ├─ Active bodies: {}", stats.active_bodies);
    println!("   └─ Memory: {:.2} MB", (count * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0));

    println!("\n═══════════════════════════════════════════════════════");
    println!("✅ STRESS TEST COMPLETED!");
    println!("═══════════════════════════════════════════════════════");
}