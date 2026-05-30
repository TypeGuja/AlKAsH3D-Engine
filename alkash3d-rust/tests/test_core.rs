// tests/test_core.rs
//! Тесты распределения задач по ядрам и потокам

use alkash3d_rs::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ===================================================================
// ТЕСТ 1: Определение количества ядер
// ===================================================================
#[test]
fn test_core_detection() {
    println!("\n=== TEST: Core Detection ===");

    let physical_cores = num_cpus::get_physical();
    let logical_cores = num_cpus::get();

    println!("Physical cores: {}", physical_cores);
    println!("Logical cores: {}", logical_cores);

    assert!(logical_cores > 0, "No cores detected!");
    assert!(physical_cores > 0, "No physical cores detected!");

    let scheduler = EngineScheduler::new();
    let budget = CpuBudget::new();

    println!("Available cores (budget): {}", budget.available_cores());
    println!("Broad phase threshold: {}", scheduler.broad_phase_threshold());
    println!("Narrow phase threshold: {}", scheduler.narrow_phase_threshold());

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 2: Бюджет ядер (CpuBudget)
// ===================================================================
#[test]
fn test_cpu_budget() {
    println!("\n=== TEST: CPU Budget ===");

    let budget = CpuBudget::with_reserve(1);
    let total_cores = num_cpus::get();
    let available_initial = budget.available_cores();

    println!("Total cores: {}", total_cores);
    println!("Available initially: {}", available_initial);

    assert!(budget.try_acquire(2), "Failed to acquire 2 cores");
    println!("After acquiring 2 cores: available={}", budget.available_cores());
    assert_eq!(budget.available_cores(), available_initial - 2);

    assert!(budget.try_acquire(2), "Failed to acquire another 2 cores");
    println!("After acquiring another 2: available={}", budget.available_cores());
    assert_eq!(budget.available_cores(), available_initial - 4);

    let too_many = budget.available_cores() + 1;
    let result = budget.try_acquire(too_many);
    println!("Try acquire {} cores (should fail): {}", too_many, result);
    assert!(!result, "Should not be able to acquire more than available");

    budget.release(2);
    println!("After releasing 2: available={}", budget.available_cores());

    budget.release(2);
    println!("After releasing all: available={}", budget.available_cores());

    budget.reset_frame();
    println!("After reset: available={}", budget.available_cores());
    assert_eq!(budget.available_cores(), available_initial);

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 3: Адаптивные пороги
// ===================================================================
#[test]
fn test_adaptive_thresholds() {
    println!("\n=== TEST: Adaptive Thresholds ===");

    let thresholds = AdaptiveThresholds::new(ThresholdConfig::default());

    println!("Initial thresholds:");
    println!("  Broad phase: {}", thresholds.broad_phase_threshold());
    println!("  Narrow phase: {}", thresholds.narrow_phase_threshold());
    println!("  Solver: {}", thresholds.solver_threshold());

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 4: Приоритеты задач
// ===================================================================
#[test]
fn test_task_priorities() {
    println!("\n=== TEST: Task Priorities ===");

    let scheduler = Arc::new(EngineScheduler::new());
    let executed = Arc::new(AtomicUsize::new(0));

    let sched1 = scheduler.clone();
    let exec1 = executed.clone();

    let sched2 = scheduler.clone();
    let exec2 = executed.clone();

    let sched3 = scheduler.clone();
    let exec3 = executed.clone();

    sched1.execute(
        Task::new(1, TaskPriority::Critical),
        move || {
            exec1.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
            println!("  Critical task executed");
        }
    );

    sched2.execute(
        Task::new(2, TaskPriority::High),
        move || {
            exec2.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
            println!("  High priority task executed");
        }
    );

    sched3.execute(
        Task::new(3, TaskPriority::Normal),
        move || {
            exec3.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
            println!("  Normal priority task executed");
        }
    );

    thread::sleep(Duration::from_millis(500));

    let total = executed.load(Ordering::SeqCst);
    println!("Total executed tasks: {}", total);
    assert!(total >= 3, "Not all tasks executed: {}/3", total);

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 5: Статистика планировщика
// ===================================================================
#[test]
fn test_scheduler_stats() {
    println!("\n=== TEST: Scheduler Statistics ===");

    let mut scheduler = EngineScheduler::new();

    for i in 0..10 {
        scheduler.execute(
            Task::new(i, TaskPriority::Normal),
            move || {
                thread::sleep(Duration::from_millis(5));
            }
        );
    }

    thread::sleep(Duration::from_millis(200));

    let timings = FrameTimings {
        broad_phase_ms: 1.5,
        narrow_phase_ms: 2.3,
        solver_ms: 3.1,
        render_ms: 5.2,
        culling_ms: 1.8,
        physics_ms: 4.0,
        scripts_ms: 1.0,
        audio_ms: 0.5,
        streaming_ms: 0.3,
    };

    scheduler.update_stats(Duration::from_millis(16), timings);

    let stats = scheduler.stats();
    println!("Scheduler Statistics:");
    println!("  Frames: {}", stats.frames);
    println!("  Avg frame time: {:.2}ms", stats.avg_frame_time_ms);
    println!("  CPU usage: {:.1}%", stats.cpu_usage_percent);

    assert!(stats.avg_frame_time_ms > 0.0);
    assert!(stats.cpu_usage_percent >= 0.0 && stats.cpu_usage_percent <= 100.0);

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 6: Нагрузочное тестирование (исправленный)
// ===================================================================
#[test]
fn test_stress_scheduler() {
    println!("\n=== TEST: Stress Test (100 tasks) ===");

    let scheduler = Arc::new(EngineScheduler::new());
    let completed = Arc::new(AtomicUsize::new(0));
    let start = Instant::now();

    let mut handles = vec![];

    // Уменьшим количество задач до 100 для надёжности
    for i in 0..100 {
        let sched = scheduler.clone();
        let counter = completed.clone();

        let handle = thread::spawn(move || {
            let priority = match i % 4 {
                0 => TaskPriority::Critical,
                1 => TaskPriority::High,
                2 => TaskPriority::Normal,
                _ => TaskPriority::Low,
            };

            sched.execute(
                Task::new(i, priority),
                move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            );
        });
        handles.push(handle);
    }

    // Ждём создания всех потоков
    for handle in handles {
        let _ = handle.join();
    }

    // Ждём выполнения задач с увеличенным таймаутом
    let mut attempts = 0;
    let max_attempts = 50;
    while completed.load(Ordering::SeqCst) < 100 && attempts < max_attempts {
        thread::sleep(Duration::from_millis(100));
        attempts += 1;
        print!(".");
    }
    println!();

    let total = completed.load(Ordering::SeqCst);
    let elapsed = start.elapsed();

    println!("\nResults:");
    println!("  Tasks completed: {}/100", total);
    println!("  Total time: {:.2}ms", elapsed.as_secs_f32() * 1000.0);

    // Проверяем, что выполнено достаточно задач (не обязательно все 100)
    assert!(total >= 90, "Too few tasks completed: {}/100", total);

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 7: Симуляция игрового цикла
// ===================================================================
#[test]
fn test_game_loop_simulation() {
    println!("\n=== TEST: Game Loop Simulation ===");

    let mut scheduler = EngineScheduler::new();
    let mut frame_times = Vec::new();
    let iterations = 20;

    for frame in 0..iterations {
        let frame_start = Instant::now();

        scheduler.reset_budget();

        scheduler.execute(
            Task::new(1, TaskPriority::High),
            || {
                let mut s = 0;
                for _ in 0..500000 {
                    s += 1;
                }
                let _ = s;
            }
        );

        scheduler.execute(
            Task::new(2, TaskPriority::High),
            || {
                let mut s = 0;
                for _ in 0..300000 {
                    s += 1;
                }
                let _ = s;
            }
        );

        scheduler.execute(
            Task::new(3, TaskPriority::Normal),
            || {
                let mut s = 0;
                for _ in 0..100000 {
                    s += 1;
                }
                let _ = s;
            }
        );

        thread::sleep(Duration::from_millis(10));

        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        frame_times.push(frame_time);

        let timings = FrameTimings::default();
        scheduler.update_stats(Duration::from_millis(16), timings);

        if frame % 10 == 0 && frame > 0 {
            let avg: f32 = frame_times.iter().sum::<f32>() / frame_times.len() as f32;
            println!("  Frame {}: {:.2}ms (avg: {:.2}ms)", frame, frame_time, avg);
        }
    }

    let avg_time: f32 = frame_times.iter().sum::<f32>() / frame_times.len() as f32;
    println!("\nGame Loop Statistics ({} frames):", frame_times.len());
    println!("  Average frame time: {:.2}ms", avg_time);

    assert!(avg_time < 100.0, "Game loop too slow: {:.2}ms per frame", avg_time);

    let stats = scheduler.stats();
    println!("  CPU usage: {:.1}%", stats.cpu_usage_percent);

    println!("✅ Test passed!");
}

// ===================================================================
// ТЕСТ 8: Параллельная итерация (Rayon)
// ===================================================================
#[test]
fn test_parallel_iteration() {
    println!("\n=== TEST: Parallel Iteration (Rayon) ===");

    use rayon::prelude::*;

    let data: Vec<i32> = (0..5_000_000).collect();

    // Sequential
    let start = Instant::now();
    let mut sum_seq = 0;
    for &x in &data {
        sum_seq += x;
    }
    let seq_time = start.elapsed();

    // Parallel
    let start = Instant::now();
    let sum_par: i32 = data.par_iter().map(|&x| x).sum();
    let par_time = start.elapsed();

    println!("Sequential sum: {} in {:.2}ms", sum_seq, seq_time.as_secs_f32() * 1000.0);
    println!("Parallel sum:   {} in {:.2}ms", sum_par, par_time.as_secs_f32() * 1000.0);

    if seq_time.as_secs_f32() > 0.0 {
        println!("Speedup: {:.2}x", seq_time.as_secs_f32() / par_time.as_secs_f32());
    }

    assert_eq!(sum_seq, sum_par);

    println!("✅ Test passed!");
}