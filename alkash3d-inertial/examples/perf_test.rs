// inertial/examples/perf_test.rs
//! Тест производительности физического плагина.
//!
//! Гоняет через РЕАЛЬНЫЙ ABI (`get_plugin_api()` → `init` → `add_body` →
//! `update` → `get_stats`), а не внутренние функции Rust напрямую — так
//! заодно проверяется, что весь путь плагина (тот, которым реально
//! пользуется движок) живой, а не только Fortran-математика в отрыве от
//! обвязки.
//!
//! Запуск:
//!   cargo run --release --example perf_test
//!
//! ВАЖНО: обязательно `--release` — без оптимизаций (-O3 у Fortran,
//! LTO у Rust) числа будут не показательны, разница может быть в разы.

use inertial::{get_plugin_api, PhysicsAPI, PhysicsBody, PhysicsConfig};
use std::ffi::c_void;
use std::time::Instant;

fn make_body(x: f32, y: f32, z: f32, mass: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass,
        inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
        restitution: 0.4,
        friction: 0.5,
        linear_damping: 0.01,
        angular_damping: 0.01,
        is_static: if mass <= 0.0 { 1 } else { 0 },
        is_asleep: 0,
    }
}

/// Сценарий: тела раскиданы по сетке РЕДКО (почти не сталкиваются).
/// Показывает "чистую" стоимость broad-phase + интеграции без нагрузки
/// на narrow-phase/солвер.
fn spawn_sparse(count: usize) -> Vec<PhysicsBody> {
    let side = (count as f32).sqrt().ceil() as i32;
    let spacing = 5.0; // достаточно, чтобы почти не пересекаться (радиус тел 0.5)
    (0..count)
        .map(|i| {
            let gx = (i as i32) % side;
            let gz = (i as i32) / side;
            make_body(gx as f32 * spacing, 5.0, gz as f32 * spacing, 1.0)
        })
        .collect()
}

/// Сценарий: плотная сетка вплотную друг к другу — как пол из кубов в
/// демо движка. Много контактов сразу, но статично (не падают) —
/// показывает стоимость narrow-phase/солвера на широком фронте контактов.
fn spawn_dense_grid(count: usize) -> Vec<PhysicsBody> {
    let side = (count as f32).sqrt().ceil() as i32;
    let spacing = 0.9; // тела радиуса 0.5 — соседи слегка пересекаются
    (0..count)
        .map(|i| {
            let gx = (i as i32) % side;
            let gz = (i as i32) / side;
            // Первый ряд — статичный "пол", остальное — динамические тела
            // на нём, чтобы сразу были и контакты, и интеграция.
            let mass = if gz == 0 { 0.0 } else { 1.0 };
            make_body(gx as f32 * spacing, gz as f32 * spacing * 0.01, gz as f32 * spacing, mass)
        })
        .collect()
}

/// Сценарий: куча тел, падающая с высоты в одну точку — самый тяжёлый
/// случай для солвера (много одновременных контактов на одних и тех же
/// телах), и заодно проверка, что система сна реально снижает стоимость
/// после того, как куча "уляжется" (см. update_sleep_state).
fn spawn_falling_pile(count: usize) -> Vec<PhysicsBody> {
    let side = (count as f32).sqrt().ceil() as i32;
    (0..count)
        .map(|i| {
            let gx = (i as i32) % side;
            let gy = (i as i32) / side;
            make_body(
                (gx as f32 - side as f32 / 2.0) * 0.6,
                10.0 + gy as f32 * 1.1,
                0.0,
                1.0,
            )
        })
        .collect()
}

struct FrameSample {
    broad_ms: f32,
    narrow_ms: f32,
    solver_ms: f32,
    active_bodies: u32,
    contacts: u32,
}

fn run_scenario(name: &str, bodies: Vec<PhysicsBody>, frames: u32) {
    let count = bodies.len();
    let plugin_api = get_plugin_api();

    let config = PhysicsConfig {
        max_bodies: count as i32 + 16,
        world_size: ((count as f32).sqrt() * 6.0 + 50.0).max(50.0),
        cell_size: 4.0,
        solver_iterations: 8,
        use_simd: 1,
    };

    let instance = (plugin_api.init)(
        std::ptr::null_mut(),
        &config as *const PhysicsConfig as *const c_void,
    );
    if instance.is_null() {
        eprintln!("[{}] init() вернул null — пропускаю сценарий", name);
        return;
    }

    let physics_api_ptr = (plugin_api.get_physics_api)(instance);
    if physics_api_ptr.is_null() {
        eprintln!("[{}] get_physics_api() вернул null — пропускаю сценарий", name);
        (plugin_api.shutdown)(instance);
        return;
    }
    let physics_api = unsafe { &*(physics_api_ptr as *const PhysicsAPI) };

    for body in &bodies {
        (physics_api.add_body)(instance, body as *const PhysicsBody);
    }

    // Прогрев — первые кадры часто медленнее (аллокации внутренних
    // буферов, холодный кэш), не должны искажать итоговое среднее.
    for _ in 0..10 {
        (physics_api.update)(instance, 1.0 / 60.0, -9.81);
    }

    let mut samples: Vec<FrameSample> = Vec::with_capacity(frames as usize);
    let wall_start = Instant::now();

    for _ in 0..frames {
        (physics_api.update)(instance, 1.0 / 60.0, -9.81);
        let stats = (physics_api.get_stats)(instance);
        samples.push(FrameSample {
            broad_ms: stats.broad_phase_time_ms,
            narrow_ms: stats.narrow_phase_time_ms,
            solver_ms: stats.solver_time_ms,
            active_bodies: stats.active_bodies,
            contacts: stats.contacts_count,
        });
    }

    let wall_elapsed = wall_start.elapsed();

    // Разбиваем на первую и вторую половину кадров — чтобы увидеть, дают
    // ли что-то засыпающие тела (падающая куча должна успокоиться и
    // подешеветь; разрежённая сцена почти не изменится).
    let half = samples.len() / 2;
    let (first_half, second_half) = samples.split_at(half.max(1));

    let avg = |xs: &[FrameSample], f: fn(&FrameSample) -> f32| {
        if xs.is_empty() {
            0.0
        } else {
            xs.iter().map(f).sum::<f32>() / xs.len() as f32
        }
    };
    let avg_u32 = |xs: &[FrameSample], f: fn(&FrameSample) -> u32| {
        if xs.is_empty() {
            0.0
        } else {
            xs.iter().map(|s| f(s) as f32).sum::<f32>() / xs.len() as f32
        }
    };

    let total_avg_frame_ms = wall_elapsed.as_secs_f64() * 1000.0 / frames as f64;
    let fps_equivalent = 1000.0 / total_avg_frame_ms;

    println!("=== {} (тел: {}) ===", name, count);
    println!(
        "  Итого: {:.3} мс/кадр  (~{:.0} FPS эквивалент), {} кадров за {:.2?}",
        total_avg_frame_ms, fps_equivalent, frames, wall_elapsed
    );
    println!(
        "  Первая половина  — broad: {:.3}мс  narrow: {:.3}мс  solver: {:.3}мс  active: {:.0}  contacts: {:.0}",
        avg(first_half, |s| s.broad_ms),
        avg(first_half, |s| s.narrow_ms),
        avg(first_half, |s| s.solver_ms),
        avg_u32(first_half, |s| s.active_bodies),
        avg_u32(first_half, |s| s.contacts),
    );
    println!(
        "  Вторая половина  — broad: {:.3}мс  narrow: {:.3}мс  solver: {:.3}мс  active: {:.0}  contacts: {:.0}",
        avg(second_half, |s| s.broad_ms),
        avg(second_half, |s| s.narrow_ms),
        avg(second_half, |s| s.solver_ms),
        avg_u32(second_half, |s| s.active_bodies),
        avg_u32(second_half, |s| s.contacts),
    );
    println!();

    (plugin_api.shutdown)(instance);
}

fn main() {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    println!("Доступно ядер (used by batch_integrate): {}\n", cores);

    let body_counts = [100usize, 1_000, 5_000, 10_000];
    let frames = 300u32; // 5 секунд симуляции при 60 Гц

    for &count in &body_counts {
        run_scenario("Разрежённая сцена", spawn_sparse(count), frames);
    }

    for &count in &body_counts {
        run_scenario("Плотная сетка (пол из кубов)", spawn_dense_grid(count), frames);
    }

    // Падающая куча — тяжелее для солвера, поэтому на меньших размерах,
    // иначе тест будет идти неоправданно долго.
    for &count in &[100usize, 500, 1_000] {
        run_scenario("Падающая куча", spawn_falling_pile(count), frames);
    }
}
