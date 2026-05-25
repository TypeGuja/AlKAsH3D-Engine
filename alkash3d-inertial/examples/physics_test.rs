// examples/physics_test.rs
// Демонстрация физического движка Inertial

use alkash3d_inertial::*;
use std::time::Instant;
use std::thread;
use std::time::Duration;

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║         INERTIAL PHYSICS ENGINE - DEMONSTRATION              ║");
    println!("║         Оптимизирован для больших городов и машин            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Тест 1: Базовая физика
    test_basic_physics();

    // Тест 2: Коллизии
    test_collisions();

    // Тест 3: Производительность
    test_performance();

    // Тест 4: Система сна
    test_sleep_system();

    // Тест 5: Массовый тест (если нужно)
    if std::env::args().any(|arg| arg == "--massive") {
        test_massive_objects();
    }

    println!("\n✅ Все тесты завершены!");
}

fn test_basic_physics() {
    println!("\n📊 ТЕСТ 1: Базовая физика");
    println!("────────────────────────────────────────");

    let mut world = PhysicsWorld::new();

    // Создаём падающий объект
    let body = RigidBody::new(1.0, Vector3::new(0.0, 10.0, 0.0));
    let id = world.add_body(body);

    println!("Создано тело #{} на высоте 10 метров", id);

    // Симулируем падение
    let dt = 1.0 / 60.0;
    let mut time = 0.0;

    for step in 0..120 {
        world.update(dt);
        time += dt;

        if step % 30 == 0 {
            let body = world.get_body(0).unwrap();
            println!("  t={:.2}s: высота={:.2}m, скорость={:.2}m/s",
                     time, body.position.y, -body.velocity.y);
        }

        if let Some(body) = world.get_body(0) {
            if body.position.y <= 0.0 {
                println!("  💥 Приземление! t={:.2}s", time);
                break;
            }
        }
    }

    let stats = world.get_stats();
    println!("Статистика: {} тел, {} мс/кадр",
             stats.bodies_count, stats.update_time_ms);
}

fn test_collisions() {
    println!("\n📊 ТЕСТ 2: Система коллизий");
    println!("────────────────────────────────────────");

    let mut world = PhysicsWorld::with_capacity(10)
        .with_collisions(true);

    // Два сталкивающихся шара
    let body1 = RigidBody::new(1.0, Vector3::new(-1.0, 0.0, 0.0));
    let body2 = RigidBody::new(1.0, Vector3::new(1.0, 0.0, 0.0));

    // Придаём скорость первому
    let mut body1 = body1;
    body1.velocity = Vector3::new(5.0, 0.0, 0.0);

    world.add_body(body1);
    world.add_body(body2);

    println!("Два шара движутся навстречу друг другу");

    let dt = 1.0 / 60.0;

    for step in 0..120 {
        world.update(dt);

        if step % 30 == 0 {
            let b1 = world.get_body(0).unwrap();
            let b2 = world.get_body(1).unwrap();
            let dist = (b2.position.x - b1.position.x).abs();
            println!("  t={:.2}s: расстояние={:.2}m, v1={:.2}m/s, v2={:.2}m/s",
                     step as f32 * dt, dist, b1.velocity.x, b2.velocity.x);
        }
    }

    let stats = world.get_stats();
    println!("Коллизий обнаружено: {}", stats.collisions_detected);
}

fn test_performance() {
    println!("\n📊 ТЕСТ 3: Производительность");
    println!("────────────────────────────────────────");

    let counts = [100, 500, 1000, 5000, 10000];

    println!("\n{:<10} {:<15} {:<15} {:<15}",
             "Объектов", "Broad phase(ms)", "Narrow(ms)", "Total(ms)");
    println!("{}", "-".repeat(60));

    for &count in &counts {
        let mut world = PhysicsWorld::with_capacity(count);

        // Создаём случайно распределённые объекты
        for i in 0..count {
            let x = (i % 100) as f32 * 2.0;
            let z = (i / 100) as f32 * 2.0;
            let body = RigidBody::new(1.0, Vector3::new(x, 0.0, z));
            world.add_body(body);
        }

        // Прогрев
        world.update(1.0 / 60.0);

        // Измерение
        let start = Instant::now();
        world.update(1.0 / 60.0);
        let elapsed = start.elapsed().as_secs_f32() * 1000.0;

        let stats = world.get_stats();

        println!("{:<10} {:<15.2} {:<15.2} {:<15.2}",
                 count,
                 stats.broad_phase_time_ms,
                 stats.narrow_phase_time_ms,
                 elapsed);
    }

    // Оценка производительности
    let mut world_10k = PhysicsWorld::with_capacity(10000);
    for i in 0..10000 {
        let body = RigidBody::new(1.0, Vector3::new((i % 100) as f32, 0.0, (i / 100) as f32));
        world_10k.add_body(body);
    }

    world_10k.update(1.0 / 60.0);
    let stats = world_10k.get_stats();

    println!("\n📈 Оценка для 1,000,000 объектов:");
    println!("  - Память: ~{} MB", 1_000_000 * std::mem::size_of::<RigidBody>() / 1024 / 1024);
    println!("  - Broad phase: ~{} ms (теоретически)", stats.broad_phase_time_ms * 100.0);
    println!("  - Narrow phase: ~{} ms (теоретически)", stats.narrow_phase_time_ms * 100.0);
    println!("  - FPS: ~{} (оптимизированно)", 1000.0 / (stats.update_time_ms * 100.0));
}

fn test_sleep_system() {
    println!("\n📊 ТЕСТ 4: Система сна");
    println!("────────────────────────────────────────");

    let mut world = PhysicsWorld::with_capacity(1000);

    // Создаём статические объекты (здания, дороги)
    for i in 0..500 {
        let static_body = RigidBody::new(0.0, Vector3::new((i % 50) as f32, 0.0, (i / 50) as f32));
        world.add_body(static_body);
    }

    // Создаём динамические объекты (машины)
    for i in 0..500 {
        let mut dynamic_body = RigidBody::new(1.0, Vector3::new((i % 50) as f32, 1.0, (i / 50) as f32));
        dynamic_body.velocity = Vector3::new(10.0, 0.0, 0.0);
        world.add_body(dynamic_body);
    }

    println!("Создано 500 статических и 500 динамических объектов");

    let dt = 1.0 / 60.0;

    for step in 0..300 {
        world.update(dt);

        if step % 60 == 0 {
            let stats = world.get_stats();
            let sleeping = stats.bodies_count - stats.active_bodies;
            println!("  t={}s: активных={}, спящих={}, коллизий={}",
                     step / 60, stats.active_bodies, sleeping, stats.collisions_detected);
        }
    }

    // Пробуждаем все тела
    world.wake_all();
    let stats = world.get_stats();
    println!("  После пробуждения: активных={}, спящих={}",
             stats.active_bodies, stats.bodies_count - stats.active_bodies);
}

fn test_massive_objects() {
    println!("\n📊 ТЕСТ 5: МАССОВЫЙ ТЕСТ (100,000 объектов)");
    println!("────────────────────────────────────────");
    println!("⚠️  Это может занять некоторое время...\n");

    let mut world = PhysicsWorld::with_capacity(100000);

    println!("Создание 100,000 объектов...");
    let start = Instant::now();

    for i in 0..100000 {
        let x = (i % 316) as f32;
        let z = (i / 316) as f32;

        if i % 1000 == 0 {
            // Статические объекты (здания)
            let body = RigidBody::new(0.0, Vector3::new(x, 0.0, z));
            world.add_body(body);
        } else {
            // Динамические объекты
            let mut body = RigidBody::new(1.0, Vector3::new(x, 10.0, z));
            body.velocity = Vector3::new(1.0, 0.0, 0.0);
            world.add_body(body);
        }

        if (i + 1) % 20000 == 0 {
            println!("  Создано {} объектов...", i + 1);
        }
    }

    let create_time = start.elapsed();
    println!("Создание завершено за {:.2} сек", create_time.as_secs_f32());

    println!("Симуляция 60 кадров...");
    let sim_start = Instant::now();

    for frame in 0..60 {
        world.update(1.0 / 60.0);

        if (frame + 1) % 15 == 0 {
            let stats = world.get_stats();
            println!("  Кадр {}: {:.2} мс, активных={}",
                     frame + 1, stats.update_time_ms, stats.active_bodies);
        }
    }

    let sim_time = sim_start.elapsed();
    let avg_frame_time = sim_time.as_secs_f32() * 1000.0 / 60.0;

    println!("\n📊 Результаты для 100,000 объектов:");
    println!("  Среднее время кадра: {:.2} мс", avg_frame_time);
    println!("  Средний FPS: {:.1}", 1000.0 / avg_frame_time);

    let stats = world.get_stats();
    println!("  Коллизий за кадр: {}", stats.collisions_detected);
    println!("  Broad phase: {:.2} мс", stats.broad_phase_time_ms);
    println!("  Narrow phase: {:.2} мс", stats.narrow_phase_time_ms);
    println!("  Solver: {:.2} мс", stats.solver_time_ms);

    if avg_frame_time < 33.33 {
        println!("\n  ✅ Движок готов к 100,000 объектов (30+ FPS)!");
    } else if avg_frame_time < 50.0 {
        println!("\n  ⚠️  Движок работает стабильно, но есть запас для оптимизации");
    } else {
        println!("\n  ❌ Требуется оптимизация для 100,000 объектов");
    }
}

// Вспомогательная функция для вывода прогресса
#[allow(dead_code)]
fn print_progress(current: usize, total: usize, message: &str) {
    if current % (total / 10) == 0 || current == total {
        let percent = (current as f32 / total as f32) * 100.0;
        println!("  {}: {:.0}%", message, percent);
    }
}