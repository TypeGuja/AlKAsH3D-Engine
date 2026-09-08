// src/bin/physics_api_smoke.rs
//! Сквозной smoke-тест физического ABI через РЕАЛЬНЫЙ `libloading` (а не
//! через `rlib`, как `alkash3d-inertial/examples/*.rs`) — единственная
//! проверка, способная поймать рассинхронизацию layout `PhysicsAPI` между
//! `alkash3d-inertial` и `alkash3d-rust` (см. план Фазы 1 — оба крейта
//! дублируют одну и ту же `#[repr(C)]` структуру вручную, без общего типа).
//!
//! Не требует D3D12-устройства/окна — `PhysicsPlugin::load` грузит плагин
//! с `device_ptr = null`, физика от GPU не зависит.
//!
//! Запуск (из папки alkash3d-rust):
//!   cargo run --bin physics_api_smoke

use alkash3d_rs::{PhysicsConfig, PhysicsPlugin};

const INERTIAL_DLL_PATH: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";

fn main() {
    println!("[SMOKE] Загружаю {}...", INERTIAL_DLL_PATH);
    let config = PhysicsConfig {
        max_bodies: 8,
        world_size: 200.0,
        cell_size: 4.0,
        solver_iterations: 8,
        use_simd: 1,
    };

    let plugin = match PhysicsPlugin::load(INERTIAL_DLL_PATH, config) {
        Ok(p) => {
            println!("[SMOKE] ✓ Плагин загружен");
            p
        }
        Err(e) => {
            eprintln!("[SMOKE] ✗ ЗАГРУЗКА ПЛАГИНА ПРОВАЛИЛАСЬ: {}", e);
            std::process::exit(1);
        }
    };

    // Простейшая проверка, что API реально рабочий, а не просто "не упал
    // при загрузке": добавить тело, прогнать несколько шагов, прочитать
    // его обратно — если ABI между крейтами разъехался, это либо
    // упадёт/зависнет здесь (вызов через чужой указатель на функцию),
    // либо вернёт заведомо мусорные значения.
    let mut plugin = plugin;
    let body = alkash3d_rs::PhysicsBody {
        position: [0.0, 10.0, 0.0],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass: 1.0,
        inv_mass: 1.0,
        restitution: 0.0,
        friction: 0.5,
        linear_damping: 0.01,
        angular_damping: 0.01,
        is_static: 0,
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
    };
    let id = plugin.add_body(&body);
    assert!(id >= 0, "add_body вернул ошибку");

    for _ in 0..30 {
        plugin.update(1.0 / 60.0, -9.81);
    }

    let after = plugin.get_body(id);
    println!("[SMOKE] Тело после 30 шагов свободного падения: y={:.4}", after.position[1]);
    assert!(
        after.position[1] < 9.5,
        "тело должно было упасть под гравитацией, но y={}",
        after.position[1]
    );

    // ДОБАВЛЕНО (Фаза 1 — apply_force/apply_impulse/set_velocity/
    // set_transform): единственная проверка, способная поймать
    // рассинхронизацию layout PhysicsAPI между двумя крейтами — примеры в
    // alkash3d-inertial линкуются как rlib и этого не видят (см. план).
    // Тело БЕЗ гравитационного дрейфа (используем gravity=0.0 ниже),
    // чтобы сдвиг по X был однозначно от apply_force, а не от побочных
    // эффектов падения/контактов.
    let mut probe_body = body;
    probe_body.position = [0.0, 100.0, 0.0]; // подальше от первого тела и от пола
    let id2 = plugin.add_body(&probe_body);
    assert!(id2 >= 0, "add_body для второго тела вернул ошибку");

    let x_before = plugin.get_body(id2).position[0];
    for _ in 0..10 {
        plugin.apply_force(id2, [50.0, 0.0, 0.0]);
        plugin.update(1.0 / 60.0, 0.0);
    }
    let x_after = plugin.get_body(id2).position[0];
    println!("[SMOKE] apply_force: x до={:.4} после={:.4}", x_before, x_after);
    assert!(x_after > x_before + 0.01, "apply_force не сдвинул тело: x={}", x_after);

    let impulse_body = alkash3d_rs::PhysicsBody { position: [10.0, 100.0, 0.0], ..probe_body };
    let id3 = plugin.add_body(&impulse_body);
    plugin.apply_impulse(id3, [0.0, 5.0, 0.0]);
    let vy_after_impulse = plugin.get_body(id3).velocity[1];
    println!("[SMOKE] apply_impulse: vy после импульса = {:.4} (ожидалось ~5.0, mass=1)", vy_after_impulse);
    assert!((vy_after_impulse - 5.0).abs() < 0.01, "apply_impulse дал неверный Δv: vy={}", vy_after_impulse);

    plugin.set_velocity(id3, [1.0, 2.0, 3.0], [0.0; 3]);
    let v_after_set = plugin.get_body(id3).velocity;
    println!("[SMOKE] set_velocity: v = {:?} (ожидалось [1,2,3])", v_after_set);
    assert_eq!(v_after_set, [1.0, 2.0, 3.0], "set_velocity не перезаписал скорость");

    plugin.set_transform(id3, [42.0, 7.0, -3.0], [0.0, 0.0, 0.0, 1.0]);
    let after_teleport = plugin.get_body(id3);
    println!("[SMOKE] set_transform: pos = {:?}, v (должна остаться) = {:?}", after_teleport.position, after_teleport.velocity);
    assert_eq!(after_teleport.position, [42.0, 7.0, -3.0], "set_transform не переместил тело");
    assert_eq!(after_teleport.velocity, [1.0, 2.0, 3.0], "set_transform не должен был трогать скорость");

    println!("[SMOKE] ✓ Всё в порядке — ABI между alkash3d-inertial и alkash3d-rust совпадает.");
}
