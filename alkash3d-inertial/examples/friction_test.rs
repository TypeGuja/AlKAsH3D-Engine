// inertial/examples/friction_test.rs
//! Функциональная проверка Coulomb-трения в контактном решателе (см.
//! `solve_contacts_vectorized` в `src/kernels/kernels_optimized.f90`).
//! Поля `friction`/`tangent1`/`tangent2`/`friction_impulse` существовали в
//! ABI и раньше, но ни один solve-путь их не читал — тела скользили без
//! сопротивления. Этот пример проверяет ПОВЕДЕНИЕ через реальный ABI
//! (`get_plugin_api()` → `add_body` → `update` → `get_body`), тем же
//! способом, что и `joint_test.rs`.
//!
//! Геометрия намеренно простая: обе сферы фиксированного радиуса 0.5
//! (см. `IMPLICIT_RADIUS` в lib.rs — в этом движке ВСЕ тела одного
//! размера, плоской "земли" как отдельного примитива не существует).
//! Динамическая сфера стартует с небольшим перекрытием со статичной
//! (0.95 < 1.0 = сумма радиусов) и небольшой горизонтальной скоростью —
//! за 5 шагов (~0.08м смещения) контакт гарантированно не успевает
//! разъехаться, но трение успевает несколько раз сработать (по 8
//! solver-итераций на шаг).
//!
//! Запуск:
//!   cargo run --release --example friction_test

use inertial::{get_plugin_api, PhysicsAPI, PhysicsBody, PhysicsConfig};
use std::ffi::c_void;

fn static_body(x: f32, y: f32, z: f32, friction: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass: 0.0,
        inv_mass: 0.0,
        restitution: 0.0,
        friction,
        linear_damping: 0.0,
        angular_damping: 0.0,
        is_static: 1,
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
    }
}

fn dynamic_body(x: f32, y: f32, z: f32, mass: f32, friction: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass,
        inv_mass: 1.0 / mass,
        restitution: 0.0,
        friction,
        linear_damping: 0.0,
        angular_damping: 0.0,
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

/// friction=0 у обеих сфер — клэмп трения `mu * |impulse|` при `mu=0`
/// всегда даёт `[0,0]`, независимо от того, сколько раз сработал контакт,
/// поэтому САМО трение не должно менять тангенциальную скорость вообще.
/// Допуск НЕ равен нулю: мяч, скользя вбок, постепенно смещается от
/// точки прямо над опорой, и контактная нормаль (всегда вдоль линии
/// центров двух сфер) чуть отклоняется от вертикали — импульс вдоль
/// НАКЛОНЁННОЙ нормали сам по себе слегка меняет vx, это геометрический
/// эффект контакта двух сфер, а не трение. Порог ниже — заведомо
/// маленький относительно эффекта трения (см. следующий тест), чтобы
/// чётко отличать "трения нет" от "трение есть".
fn test_zero_friction_preserves_tangential_velocity() {
    let h = Harness::new(4);
    let _floor = (h.api.add_body)(h.instance, &static_body(0.0, 0.0, 0.0, 0.0));
    let mut ball = dynamic_body(0.0, 0.95, 0.0, 1.0, 0.0);
    ball.velocity = [1.0, 0.0, 0.0];
    let ball_id = (h.api.add_body)(h.instance, &ball);
    assert!(ball_id >= 0);

    h.step(5);

    let body = (h.api.get_body)(h.instance, ball_id);
    assert!(
        (body.velocity[0] - 1.0).abs() < 0.05,
        "friction=0 не должно заметно менять тангенциальную скорость: vx={} (ожидалось ~1.0 ± геометрический шум)",
        body.velocity[0]
    );
    println!("[OK] friction=0: vx после 5 шагов = {:.5} (ожидалось ~1.0)", body.velocity[0]);
}

/// friction=1.0 у обеих сфер, та же геометрия и стартовая скорость — за
/// те же 5 шагов тангенциальная скорость должна заметно просесть
/// относительно недемпфированного случая выше (сравнительная, не точная
/// проверка — per-iteration клэмп без warm-starting, см. комментарий в
/// kernels_optimized.f90).
fn test_high_friction_damps_tangential_velocity() {
    let h = Harness::new(4);
    let _floor = (h.api.add_body)(h.instance, &static_body(0.0, 0.0, 0.0, 1.0));
    let mut ball = dynamic_body(0.0, 0.95, 0.0, 1.0, 1.0);
    ball.velocity = [1.0, 0.0, 0.0];
    let ball_id = (h.api.add_body)(h.instance, &ball);
    assert!(ball_id >= 0);

    h.step(5);

    let body = (h.api.get_body)(h.instance, ball_id);
    assert!(
        body.velocity[0] < 0.9,
        "высокое трение должно заметно погасить тангенциальную скорость: vx={} (ожидалось заметно < 1.0)",
        body.velocity[0]
    );
    println!(
        "[OK] friction=1: vx после 5 шагов = {:.5} (ожидалось заметно < 1.0, было 1.0 без трения)",
        body.velocity[0]
    );
}

fn main() {
    test_zero_friction_preserves_tangential_velocity();
    test_high_friction_damps_tangential_velocity();
    println!("\nВсе проверки трения пройдены.");
}
