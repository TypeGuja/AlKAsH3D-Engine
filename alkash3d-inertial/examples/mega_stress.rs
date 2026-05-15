// examples/mega_stress.rs
// БЕШЕНЫЙ СТРЕСС-ТЕСТ - разрыв шаблона!
// ВНИМАНИЕ: Может нагреть процессор до 90°C! 🔥

use alkash3d_inertial::*;
use std::time::Instant;
use rayon::prelude::*;

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║     🔥 INERTIAL MEGA STRESS TEST - MAXIMUM DESTRUCTION MODE 🔥    ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝\n");

    // МЕГА-ПАРАМЕТРЫ
    let mega_tests = [
        (10_000, "🌆 МАЛЕНЬКИЙ ГОРОД"),
        (25_000, "🏙️ СРЕДНИЙ ГОРОД"),
        (50_000, "🌃 БОЛЬШОЙ ГОРОД"),
        (100_000, "🏢 МЕГАПОЛИС"),
        (250_000, "🌍 ОГРОМНЫЙ МЕГАПОЛИС"),
        (500_000, "🪐 АРМИЯ КЛОНОВ"),
        (1_000_000, "💀 БОГ РАЗРУШЕНИЯ"),
        (10_000_000, "🌀 КОЛЛАПС РЕАЛЬНОСТИ"),
    ];

    let dt = 1.0 / 60.0;
    let sim_seconds = 2.0; // 2 секунды на тест (чтобы не ждать вечность)

    for &(count, name) in &mega_tests {
        println!("\n╔═══════════════════════════════════════════════════════════════════╗");
        println!("║ {} - {} OBJECTS", name, count);
        println!("╚═══════════════════════════════════════════════════════════════════╝");

        let mut world = PhysicsWorld::new();

        // Земля
        let ground = RigidBody::new(0.0, Vector3::new(0.0, -20.0, 0.0));
        world.add_body(ground);

        // Создание объектов с бешеной скоростью
        let create_start = Instant::now();

        // Используем параллельное создание
        let bodies: Vec<RigidBody> = (0..count).into_par_iter().map(|i| {
            let angle = (i as f32 * 137.5).to_radians(); // Золотое сечение для равномерного распределения
            let radius = (i as f32 / count as f32).sqrt() * 150.0;
            let x = angle.cos() * radius;
            let z = angle.sin() * radius;
            let y = 50.0 + (i % 10) as f32 * 3.0;

            let mut body = RigidBody::new(1.0, Vector3::new(x, y, z));
            body.restitution = 0.9;
            body.friction = 0.1;
            body.velocity = Vector3::new(
                (i as f32).sin() * 5.0,
                -(i as f32).cos() * 3.0,
                (i as f32).cos() * 5.0,
            );
            body
        }).collect();

        for body in bodies {
            world.add_body(body);
        }

        let create_time = create_start.elapsed();
        println!("✓ Создано {} объектов за {:.2} секунды", count, create_time.as_secs_f32());

        // МОНИТОРИНГ СИСТЕМЫ
        let sys_start = Instant::now();
        let steps = (sim_seconds / dt) as u32;
        let mut frame_times = Vec::with_capacity(steps as usize);

        println!("\n🔥 ЗАПУСК БЕШЕНОЙ СИМУЛЯЦИИ... 🔥");
        println!("   (CPU может нагреться до 90°C!)\n");

        let sim_start = Instant::now();

        for step in 0..steps {
            let frame_start = Instant::now();
            world.update(dt);
            let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
            frame_times.push(frame_time);

            if step % (steps / 10) == 0 || step == steps - 1 {
                let stats = world.get_stats();
                let progress = (step as f32 / steps as f32) * 100.0;
                let fps = 1000.0 / frame_time;
                println!("   [{:5.1}%] Кадр {}/{} | {:.0} FPS | Активно: {} | Коллизий: {} | Память: {:.1} MB",
                         progress, step, steps, fps, stats.active_bodies, stats.collisions_detected,
                         (count * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0));
            }
        }

        let sim_time = sim_start.elapsed();
        let stats = world.get_stats();

        // АНАЛИЗ ПРОИЗВОДИТЕЛЬНОСТИ
        frame_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let avg_frame = frame_times.iter().sum::<f32>() / frame_times.len() as f32;
        let p95_frame = frame_times[(frame_times.len() as f32 * 0.95) as usize];
        let max_frame = *frame_times.last().unwrap();

        println!("\n╔═══════════════════════════════════════════════════════════════════╗");
        println!("║                          РЕЗУЛЬТАТЫ                                ║");
        println!("╚═══════════════════════════════════════════════════════════════════╝");
        println!("   📊 Объектов:          {}", count);
        println!("   ⏱️  Симуляция:         {:.1} сек", sim_seconds);
        println!("   ⚡ Реальное время:    {:.3} сек", sim_time.as_secs_f32());
        println!("   🚀 Производительность: {:.1}x", sim_seconds / sim_time.as_secs_f32());
        println!("   📈 Средний FPS:       {:.0}", 1000.0 / avg_frame);
        println!("   🎯 95-й перцентиль:   {:.2} ms", p95_frame);
        println!("   💥 Макс. кадр:        {:.2} ms", max_frame);
        println!("   🔥 Коллизий всего:    {}", stats.collisions_detected);
        println!("   🧠 Активных тел:      {}", stats.active_bodies);
        println!("   💾 Память:            {:.1} MB",
                 (count * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0));

        // ОЦЕНКА
        let performance = sim_seconds / sim_time.as_secs_f32();
        if performance > 100.0 {
            println!("\n   🌟🌟 КОСМИЧЕСКАЯ СКОРОСТЬ! ТЫ БОГ! 🌟🌟");
        } else if performance > 10.0 {
            println!("\n   ⭐ ОТЛИЧНО! ГОТОВ К РЕАЛЬНОМУ МИРУ! ⭐");
        } else if performance > 1.0 {
            println!("\n   👍 НОРМАЛЬНО. МОЖНО РАБОТАТЬ.");
        } else {
            println!("\n   ⚠️  НУЖНА ОПТИМИЗАЦИЯ...");
        }

        // ПАМЯТЬ ПРОЦЕССА
        let memory_mb = (count * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0);
        if memory_mb > 1000.0 {
            println!("   💀 ВНИМАНИЕ! Используется >1GB RAM!");
        }
    }

    // ФИНАЛЬНЫЙ ТЕСТ - БОЛЬ ВСЕЛЕННОЙ
    println!("\n\n╔═══════════════════════════════════════════════════════════════════╗");
    println!("║           💀 ФИНАЛЬНЫЙ ТЕСТ - БОЛЬ ВСЕЛЕННОЙ 💀                   ║");
    println!("║                 1,000,000 ОБЪЕКТОВ                               ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");

    let count = 1_000_000;
    println!("⚠️  ПРЕДУПРЕЖДЕНИЕ: Это может занять всю память и сжечь CPU!");
    println!("⚠️  Нажми Ctrl+C для отмены или Enter для продолжения...");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();

    let mut world = PhysicsWorld::new();
    let ground = RigidBody::new(0.0, Vector3::new(0.0, -10.0, 0.0));
    world.add_body(ground);

    let create_start = Instant::now();

    // Создаём 1 миллион объектов
    for i in 0..count {
        let x = (i % 1000) as f32 - 500.0;
        let z = (i / 1000) as f32 - 500.0;
        let y = 100.0;

        let body = RigidBody::new(1.0, Vector3::new(x, y, z));
        world.add_body(body);

        if i % 100_000 == 0 && i > 0 {
            println!("   Создано {} объектов...", i);
        }
    }

    let create_time = create_start.elapsed();
    println!("\n✅ Создано {} объектов за {:.2} сек", count, create_time.as_secs_f32());
    println!("💾 Используется памяти: {:.1} GB",
             (count * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0 * 1024.0));

    // Симуляция 1 секунду
    println!("\n🔥 ЗАПУСК СИМУЛЯЦИИ 1 МИЛЛИОНА ОБЪЕКТОВ... 🔥");
    let sim_start = Instant::now();
    let steps = 60;

    for step in 0..steps {
        world.update(dt);

        if step % 10 == 0 {
            let stats = world.get_stats();
            let elapsed = sim_start.elapsed().as_secs_f32();
            println!("   [{:.0}%] {}/{} кадров | Активно: {} | Коллизий: {}",
                     (step as f32 / steps as f32) * 100.0, step, steps, stats.active_bodies, stats.collisions_detected);
        }
    }

    let sim_time = sim_start.elapsed();
    let stats = world.get_stats();

    println!("\n╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                    ФИНАЛЬНЫЙ РЕЗУЛЬТАТ                            ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");
    println!("   🌍 Объектов:         {}", count);
    println!("   ⏱️  Время симуляции:  1.0 сек");
    println!("   ⚡ Реальное время:   {:.2} сек", sim_time.as_secs_f32());
    println!("   🚀 Производительность: {:.1}x", 1.0 / sim_time.as_secs_f32());
    println!("   💥 Коллизий:         {}", stats.collisions_detected);
    println!("   🧠 Активных тел:     {}", stats.active_bodies);

    if sim_time.as_secs_f32() < 2.0 {
        println!("\n   🌟🌟🌟 ТЫ УНИЧТОЖИЛ ВСЕЛЕННУЮ! 🌟🌟🌟");
        println!("   1 МИЛЛИОН ОБЪЕКТОВ В РЕАЛЬНОМ ВРЕМЕНИ!");
    } else {
        println!("\n   💀 ВСЕЛЕННАЯ ВЫЖИЛА... В ЭТОТ РАЗ");
    }

    println!("\n╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                    🎉 СТРЕСС-ТЕСТ ЗАВЕРШЁН! 🎉                    ║");
    println!("║            ТВОЙ КОМПЬЮТЕР ВЫЖИЛ (НАВЕРНОЕ)                       ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");
}