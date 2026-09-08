// inertial/examples/joint_test.rs
//! Функциональная проверка джойнтов/constraint API (разборка машины на
//! детали — см. `ConstraintDesc`/`PhysicsAPI::add_constraint` в lib.rs).
//!
//! В отличие от `perf_test.rs` (меряет ВРЕМЯ), этот пример проверяет
//! ПРАВИЛЬНОСТЬ поведения через `assert!` — гоняется через тот же самый
//! реальный ABI (`get_plugin_api()` → `add_body`/`add_constraint` →
//! `update` → `get_body`/`get_constraint`/`get_broken_constraints`),
//! которым пользуется движок, а не внутренние функции Rust напрямую.
//!
//! Запуск:
//!   cargo run --release --example joint_test

use inertial::{get_plugin_api, joint_type, ConstraintDesc, PhysicsAPI, PhysicsBody, PhysicsConfig};
use std::ffi::c_void;

fn static_body(x: f32, y: f32, z: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass: 0.0,
        inv_mass: 0.0,
        restitution: 0.0,
        friction: 0.5,
        linear_damping: 0.01,
        angular_damping: 0.01,
        is_static: 1,
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
    }
}

fn dynamic_body(x: f32, y: f32, z: f32, mass: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass,
        inv_mass: 1.0 / mass,
        restitution: 0.0,
        friction: 0.5,
        linear_damping: 0.01,
        angular_damping: 0.05,
        is_static: 0,
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
    }
}

struct Harness {
    plugin_api: inertial::PluginAPI,
    instance: *mut c_void,
    api: PhysicsAPI,
}

impl Harness {
    fn new(max_bodies: i32) -> Self {
        let plugin_api = get_plugin_api();
        let config = PhysicsConfig {
            max_bodies,
            world_size: 200.0,
            cell_size: 4.0,
            solver_iterations: 8,
            use_simd: 1,
        };
        let instance = (plugin_api.init)(std::ptr::null_mut(), &config as *const PhysicsConfig as *const c_void);
        assert!(!instance.is_null(), "init() вернул null");
        let api_ptr = (plugin_api.get_physics_api)(instance);
        assert!(!api_ptr.is_null(), "get_physics_api() вернул null");
        let api = unsafe { *(api_ptr as *const PhysicsAPI) };
        Self { plugin_api, instance, api }
    }

    fn step(&self, n: u32) {
        for _ in 0..n {
            (self.api.update)(self.instance, 1.0 / 60.0, -9.81);
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        (self.plugin_api.shutdown)(self.instance);
    }
}

/// JOINT_FIXED (жёсткая сварка/болт) должно держать динамическое тело на
/// месте против гравитации — ровно то поведение, которое нужно для
/// "детали, прикрученной к машине, пока её не открутили".
fn test_fixed_holds_against_gravity() {
    let h = Harness::new(8);
    let anchor = (h.api.add_body)(h.instance, &static_body(0.0, 5.0, 0.0));
    let part = (h.api.add_body)(h.instance, &dynamic_body(0.0, 5.0, 0.0, 1.0));
    assert!(anchor >= 0 && part >= 0);

    let desc = ConstraintDesc {
        body_a: anchor,
        body_b: part,
        joint_type: joint_type::FIXED,
        bias: 1.0,
        break_impulse_linear: 0.0,  // неразрушимо
        break_impulse_angular: 0.0,
        ..Default::default()
    };
    let c = (h.api.add_constraint)(h.instance, &desc as *const ConstraintDesc);
    assert!(c >= 0, "add_constraint вернул ошибку");

    h.step(120); // 2 игровые секунды

    let body = (h.api.get_body)(h.instance, part);
    assert!(
        body.position[1] > 4.5,
        "FIXED joint не удержал деталь против гравитации: y={}",
        body.position[1]
    );
    let info = (h.api.get_constraint)(h.instance, c);
    assert_eq!(info.is_broken, 0, "неразрушимый joint сломался сам собой");
    println!("[OK] JOINT_FIXED держит деталь: y={:.4} (ожидалось ~5.0)", body.position[1]);
}

/// JOINT_HINGE должно гасить угловую скорость ВОКРУГ перпендикулярных
/// оси петли направлений (ось не "уезжает"), но НЕ гасить вращение
/// вокруг самой оси (дверь должна свободно открываться).
fn test_hinge_locks_perpendicular_axis_only() {
    let h = Harness::new(8);
    let frame = (h.api.add_body)(h.instance, &static_body(0.0, 0.0, 0.0));
    let mut door = dynamic_body(1.0, 0.0, 0.0, 1.0);
    // Закручиваем ОДНОВРЕМЕННО вокруг оси петли (Y, должно остаться) и
    // перпендикулярно ей (X, должно быть погашено).
    door.angular_velocity = [2.0, 3.0, 0.0];
    let door_id = (h.api.add_body)(h.instance, &door);

    let desc = ConstraintDesc {
        body_a: frame,
        body_b: door_id,
        joint_type: joint_type::HINGE,
        anchor_a: [1.0, 0.0, 0.0],
        anchor_b: [0.0, 0.0, 0.0],
        axis_a: [0.0, 1.0, 0.0],
        axis_b: [0.0, 1.0, 0.0],
        bias: 1.0,
        break_impulse_linear: 0.0,
        break_impulse_angular: 0.0,
        ..Default::default()
    };
    let c = (h.api.add_constraint)(h.instance, &desc as *const ConstraintDesc);
    assert!(c >= 0);

    h.step(30);

    let body = (h.api.get_body)(h.instance, door_id);
    println!(
        "[OK] JOINT_HINGE: angular_velocity после 30 шагов = {:?} (ожидание: X~0, Y осталось ненулевым)",
        body.angular_velocity
    );
    assert!(
        body.angular_velocity[0].abs() < 0.3,
        "перпендикулярная оси петли угловая скорость не погашена: wx={}",
        body.angular_velocity[0]
    );
    assert!(
        body.angular_velocity[1].abs() > 0.3,
        "вращение ВОКРУГ оси петли не должно гаситься: wy={}",
        body.angular_velocity[1]
    );
}

/// Соединение с низким порогом разрушения обязано сломаться под
/// достаточной нагрузкой и ОДИН раз попасть в `get_broken_constraints` —
/// это и есть механика "деталь оторвалась", на которую завязана
/// разборка машины на компоненты.
fn test_break_under_load() {
    let h = Harness::new(8);
    let anchor = (h.api.add_body)(h.instance, &static_body(0.0, 5.0, 0.0));
    // Тяжёлое тело — большой вес быстро разгоняется гравитацией и создаёт
    // impulse на joint, кратно превышающий низкий порог ниже.
    let part = (h.api.add_body)(h.instance, &dynamic_body(0.0, 5.0, 0.0, 50.0));

    let desc = ConstraintDesc {
        body_a: anchor,
        body_b: part,
        joint_type: joint_type::FIXED,
        bias: 1.0,
        break_impulse_linear: 0.5, // намеренно низкий порог
        break_impulse_angular: 0.0,
        ..Default::default()
    };
    let c = (h.api.add_constraint)(h.instance, &desc as *const ConstraintDesc);
    assert!(c >= 0);

    let mut saw_break_event = false;
    let mut break_event_count = 0;
    for _ in 0..120 {
        (h.api.update)(h.instance, 1.0 / 60.0, -9.81);
        let count = (h.api.get_broken_constraints_count)(h.instance);
        if count > 0 {
            saw_break_event = true;
            break_event_count += count;
            let ptr = (h.api.get_broken_constraints)(h.instance);
            let handles = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
            assert_eq!(handles, &[c], "сломавшийся handle не совпадает с ожидаемым");
        }
    }

    assert!(saw_break_event, "joint под явно чрезмерной нагрузкой не сломался");
    assert_eq!(break_event_count, 1, "событие поломки должно попасть в get_broken_constraints РОВНО один раз");

    let info = (h.api.get_constraint)(h.instance, c);
    assert_eq!(info.is_broken, 1);

    // После разрушения деталь должна снова свободно падать под гравитацией
    // (joint больше не держит её) — проверяем скорость падения на
    // последнем шаге.
    let body_before = (h.api.get_body)(h.instance, part);
    (h.api.update)(h.instance, 1.0 / 60.0, -9.81);
    let body_after = (h.api.get_body)(h.instance, part);
    assert!(
        body_after.position[1] < body_before.position[1],
        "деталь должна свободно падать после разрушения joint'а"
    );

    println!("[OK] JOINT_FIXED сломался под нагрузкой ровно один раз, деталь освободилась");
}

fn main() {
    test_fixed_holds_against_gravity();
    test_hinge_locks_perpendicular_axis_only();
    test_break_under_load();
    println!("\nВсе проверки джойнтов/constraint API пройдены.");
}
