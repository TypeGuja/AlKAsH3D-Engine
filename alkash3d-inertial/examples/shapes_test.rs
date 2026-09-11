// inertial/examples/shapes_test.rs
//! Функциональная проверка физики, добавленной за эту сессию: box-vs-box,
//! box-vs-sphere/box-vs-plane (перенесённые в Fortran), полный набор
//! capsule-коллайдера (capsule-vs-sphere/box/capsule/plane) и raycast API
//! (сфера/коробка/капсула/плоскость + исключение собственного тела).
//!
//! Тот же паттерн, что и у `joint_test.rs`/`friction_test.rs` — гоняется
//! через РЕАЛЬНЫЙ ABI (`get_plugin_api()` → `add_body`/`add_plane` →
//! `update`/`raycast` → `get_body`), не внутренние функции напрямую.
//!
//! Запуск:
//!   cargo run --release --example shapes_test

use inertial::{get_plugin_api, shape_type, PhysicsAPI, PhysicsBody, PhysicsConfig, PlaneDesc};
use std::ffi::c_void;

fn sphere_body(x: f32, y: f32, z: f32, mass: f32, radius: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass,
        inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
        restitution: 0.0,
        friction: 0.5,
        linear_damping: 0.05,
        angular_damping: 0.05,
        is_static: if mass <= 0.0 { 1 } else { 0 },
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
        radius,
        shape_type: shape_type::SPHERE,
        half_extents: [0.0; 3],
    }
}

fn box_body(x: f32, y: f32, z: f32, mass: f32, half_extents: [f32; 3]) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass,
        inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
        restitution: 0.0,
        friction: 0.5,
        linear_damping: 0.05,
        angular_damping: 0.15,
        is_static: if mass <= 0.0 { 1 } else { 0 },
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
        radius: 0.0,
        shape_type: shape_type::BOX,
        half_extents,
    }
}

fn capsule_body(x: f32, y: f32, z: f32, mass: f32, radius: f32, half_height: f32) -> PhysicsBody {
    PhysicsBody {
        position: [x, y, z],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass,
        inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
        restitution: 0.0,
        friction: 0.5,
        linear_damping: 0.05,
        angular_damping: 0.15,
        is_static: if mass <= 0.0 { 1 } else { 0 },
        is_asleep: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
        radius,
        shape_type: shape_type::CAPSULE,
        half_extents: [half_height, 0.0, 0.0],
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

/// Два box-тела, одно падает точно на другое сверху — раньше (до
/// `narrow_phase_box_box`) они проходили бы друг сквозь друга. Проверяем,
/// что падающая коробка ОСТАНАВЛИВАЕТСЯ примерно на сумме половин высот, а
/// не проваливается сквозь.
fn test_box_vs_box_stops_falling_box() {
    let h = Harness::new(4);
    let ground = (h.api.add_body)(h.instance, &box_body(0.0, 0.0, 0.0, 0.0, [2.0, 0.5, 2.0]));
    let mut falling = box_body(0.0, 3.0, 0.0, 1.0, [0.5, 0.5, 0.5]);
    falling.linear_damping = 0.3; // быстрее гасит остаточные колебания для стабильной проверки
    let falling_id = (h.api.add_body)(h.instance, &falling);
    assert!(ground >= 0 && falling_id >= 0);

    h.step(180); // 3 секунды — достаточно упасть и устаканиться

    let body = (h.api.get_body)(h.instance, falling_id);
    // Ожидаемая высота покоя: ground top (0.5) + half_extents падающей (0.5) = 1.0.
    assert!(
        body.position[1] > 0.7 && body.position[1] < 1.3,
        "box-vs-box не остановил падающую коробку на ожидаемой высоте: y={} (ожидалось ~1.0)",
        body.position[1]
    );
    println!("[OK] box-vs-box: падающая коробка остановилась на y={:.4} (ожидалось ~1.0)", body.position[1]);
}

/// Сфера падает на коробку — box-vs-sphere теперь в Fortran, проверяем,
/// что поведение то же самое (сфера останавливается на поверхности).
fn test_box_vs_sphere_stops_falling_sphere() {
    let h = Harness::new(4);
    let ground = (h.api.add_body)(h.instance, &box_body(0.0, 0.0, 0.0, 0.0, [2.0, 0.5, 2.0]));
    let mut ball = sphere_body(0.0, 3.0, 0.0, 1.0, 0.4);
    ball.linear_damping = 0.3;
    let ball_id = (h.api.add_body)(h.instance, &ball);
    assert!(ground >= 0 && ball_id >= 0);

    h.step(180);

    let body = (h.api.get_body)(h.instance, ball_id);
    // Ожидаемая высота покоя: ground top (0.5) + радиус сферы (0.4) = 0.9.
    assert!(
        body.position[1] > 0.6 && body.position[1] < 1.2,
        "box-vs-sphere не остановил падающую сферу на ожидаемой высоте: y={} (ожидалось ~0.9)",
        body.position[1]
    );
    println!("[OK] box-vs-sphere: сфера остановилась на y={:.4} (ожидалось ~0.9)", body.position[1]);
}

/// box-vs-plane (через `resolve_plane_contacts`/`box_effective_radius`,
/// перенесённый в Fortran) — коробка, слегка повёрнутая, не должна
/// провалиться под пол.
fn test_box_vs_plane_holds_rotated_box() {
    let h = Harness::new(4);
    let plane = PlaneDesc {
        normal: [0.0, 1.0, 0.0],
        point: [0.0, 0.0, 0.0],
        friction: 0.5,
        restitution: 0.0,
    };
    let plane_idx = (h.api.add_plane)(h.instance, &plane as *const PlaneDesc);
    assert!(plane_idx >= 0, "add_plane вернул ошибку");

    // Кватернион поворота ~30° вокруг оси Z — коробка садится на угол.
    let half_angle = (30.0f32).to_radians() * 0.5;
    let mut b = box_body(0.0, 3.0, 0.0, 1.0, [0.5, 0.5, 0.5]);
    b.orientation = [0.0, 0.0, half_angle.sin(), half_angle.cos()];
    b.linear_damping = 0.3;
    b.angular_damping = 0.5;
    let box_id = (h.api.add_body)(h.instance, &b);
    assert!(box_id >= 0);

    h.step(240); // 4 секунды — дать улечься на бок/угол

    let body = (h.api.get_body)(h.instance, box_id);
    assert!(
        body.position[1] > -0.1,
        "box-vs-plane не удержал повёрнутую коробку — провалилась под пол: y={}",
        body.position[1]
    );
    assert!(
        body.position[1] < 1.0,
        "box-vs-plane: коробка висит подозрительно высоко (не осела вообще): y={}",
        body.position[1]
    );
    println!("[OK] box-vs-plane: повёрнутая коробка осела на y={:.4} (в разумных пределах)", body.position[1]);
}

/// Капсула падает на статичную сферу — capsule-vs-sphere.
fn test_capsule_vs_sphere() {
    let h = Harness::new(4);
    let ground = (h.api.add_body)(h.instance, &sphere_body(0.0, 0.0, 0.0, 0.0, 1.0));
    let mut cap = capsule_body(0.0, 3.5, 0.0, 1.0, 0.3, 0.5); // радиус 0.3, полувысота 0.5 -> полная высота капсулы 1.6
    cap.linear_damping = 0.3;
    cap.angular_damping = 0.8; // не даём капсуле завалиться набок — проверяем чистую высоту покоя
    let cap_id = (h.api.add_body)(h.instance, &cap);
    assert!(ground >= 0 && cap_id >= 0);

    h.step(180);

    let body = (h.api.get_body)(h.instance, cap_id);
    // Капсула стоит вертикально своим нижним концом (половина капсулы, y=-0.5
    // от центра, плюс радиус 0.3) на сфере радиуса 1.0: y_rest ~= 1.0 + 0.5 + 0.3 = 1.8.
    assert!(
        body.position[1] > 1.4 && body.position[1] < 2.2,
        "capsule-vs-sphere не остановил падающую капсулу на ожидаемой высоте: y={} (ожидалось ~1.8)",
        body.position[1]
    );
    println!("[OK] capsule-vs-sphere: капсула остановилась на y={:.4} (ожидалось ~1.8)", body.position[1]);
}

/// Капсула падает на статичную коробку — capsule-vs-box (тернарный поиск).
fn test_capsule_vs_box() {
    let h = Harness::new(4);
    let ground = (h.api.add_body)(h.instance, &box_body(0.0, 0.0, 0.0, 0.0, [2.0, 0.5, 2.0]));
    let mut cap = capsule_body(0.0, 3.5, 0.0, 1.0, 0.3, 0.5);
    cap.linear_damping = 0.3;
    cap.angular_damping = 0.8;
    let cap_id = (h.api.add_body)(h.instance, &cap);
    assert!(ground >= 0 && cap_id >= 0);

    h.step(180);

    let body = (h.api.get_body)(h.instance, cap_id);
    // ground top (0.5) + капсула нижним концом (0.5 полувысота + 0.3 радиус) = 1.3.
    assert!(
        body.position[1] > 0.9 && body.position[1] < 1.7,
        "capsule-vs-box не остановил падающую капсулу на ожидаемой высоте: y={} (ожидалось ~1.3)",
        body.position[1]
    );
    println!("[OK] capsule-vs-box: капсула остановилась на y={:.4} (ожидалось ~1.3)", body.position[1]);
}

/// Две капсулы, одна падает на другую — capsule-vs-capsule
/// (closest-point-between-segments).
fn test_capsule_vs_capsule() {
    let h = Harness::new(4);
    let ground = (h.api.add_body)(h.instance, &capsule_body(0.0, 0.0, 0.0, 0.0, 0.3, 0.5));
    let mut cap = capsule_body(0.0, 3.0, 0.0, 1.0, 0.3, 0.5);
    cap.linear_damping = 0.3;
    cap.angular_damping = 0.8;
    let cap_id = (h.api.add_body)(h.instance, &cap);
    assert!(ground >= 0 && cap_id >= 0);

    h.step(180);

    let body = (h.api.get_body)(h.instance, cap_id);
    // Нижняя капсула стоит вертикально: её верхний конец на y = 0.5+0.3 = 0.8.
    // Верхняя капсула садится на неё нижним концом: y_rest ~= 0.8 + 0.3 + 0.5 = 1.6.
    assert!(
        body.position[1] > 1.2 && body.position[1] < 2.0,
        "capsule-vs-capsule не остановил падающую капсулу на ожидаемой высоте: y={} (ожидалось ~1.6)",
        body.position[1]
    );
    println!("[OK] capsule-vs-capsule: капсула остановилась на y={:.4} (ожидалось ~1.6)", body.position[1]);
}

/// capsule-vs-plane (`capsule_effective_radius`) — капсула, лежащая НА
/// БОКУ (ось капсулы горизонтальна), не должна провалиться под пол.
fn test_capsule_vs_plane() {
    let h = Harness::new(4);
    let plane = PlaneDesc {
        normal: [0.0, 1.0, 0.0],
        point: [0.0, 0.0, 0.0],
        friction: 0.5,
        restitution: 0.0,
    };
    assert!((h.api.add_plane)(h.instance, &plane as *const PlaneDesc) >= 0);

    // Капсула повёрнута на 90° вокруг оси Z — её локальная ось Y (вдоль
    // которой идёт сегмент) теперь смотрит вдоль мировой оси X, капсула
    // "лежит на боку".
    let mut cap = capsule_body(0.0, 3.0, 0.0, 1.0, 0.3, 0.5);
    let half_angle = (90.0f32).to_radians() * 0.5;
    cap.orientation = [0.0, 0.0, half_angle.sin(), half_angle.cos()];
    cap.linear_damping = 0.3;
    cap.angular_damping = 0.8;
    let cap_id = (h.api.add_body)(h.instance, &cap);
    assert!(cap_id >= 0);

    h.step(180);

    let body = (h.api.get_body)(h.instance, cap_id);
    // Лежащая на боку капсула касается пола только радиусом: y_rest ~= 0.3.
    assert!(
        body.position[1] > 0.0 && body.position[1] < 0.8,
        "capsule-vs-plane не удержал лежащую капсулу на ожидаемой высоте: y={} (ожидалось ~0.3)",
        body.position[1]
    );
    println!("[OK] capsule-vs-plane: лежащая капсула осела на y={:.4} (ожидалось ~0.3)", body.position[1]);
}

/// Raycast против сферы/коробки/капсулы/плоскости + исключение
/// собственного тела (`exclude_body`).
fn test_raycast() {
    let h = Harness::new(8);

    let sphere_id = (h.api.add_body)(h.instance, &sphere_body(0.0, 5.0, 0.0, 0.0, 1.0));
    let box_id = (h.api.add_body)(h.instance, &box_body(5.0, 5.0, 0.0, 0.0, [1.0, 1.0, 1.0]));
    let cap_id = (h.api.add_body)(h.instance, &capsule_body(10.0, 5.0, 0.0, 0.0, 0.5, 1.0));
    assert!(sphere_id >= 0 && box_id >= 0 && cap_id >= 0);

    let plane = PlaneDesc { normal: [0.0, 1.0, 0.0], point: [0.0, 0.0, 0.0], friction: 0.5, restitution: 0.0 };
    assert!((h.api.add_plane)(h.instance, &plane as *const PlaneDesc) >= 0);

    // Луч сверху вниз через центр сферы.
    let origin = [0.0f32, 10.0, 0.0];
    let dir = [0.0f32, -1.0, 0.0];
    let hit = (h.api.raycast)(h.instance, origin.as_ptr(), dir.as_ptr(), 100.0, -1);
    assert_eq!(hit.hit, 1, "raycast не попал ни во что, хотя должен был попасть в сферу");
    assert_eq!(hit.is_plane, 0);
    assert_eq!(hit.body, sphere_id, "raycast попал не в ту сферу");
    assert!((hit.distance - 4.0).abs() < 0.05, "неверная дистанция до сферы: {}", hit.distance);
    println!("[OK] raycast vs sphere: distance={:.3} (ожидалось ~4.0), body={}", hit.distance, hit.body);

    // Луч сверху вниз через центр коробки.
    let origin_box = [5.0f32, 10.0, 0.0];
    let hit = (h.api.raycast)(h.instance, origin_box.as_ptr(), dir.as_ptr(), 100.0, -1);
    assert_eq!(hit.hit, 1);
    assert_eq!(hit.body, box_id, "raycast попал не в ту коробку");
    assert!((hit.distance - 4.0).abs() < 0.05, "неверная дистанция до коробки: {}", hit.distance);
    println!("[OK] raycast vs box: distance={:.3} (ожидалось ~4.0), body={}", hit.distance, hit.body);

    // Луч сверху вниз через центр капсулы (полная высота 3.0, радиус 0.5 —
    // должен попасть в верхнюю полусферу-крышку на y=6.0+0.5=6.5, т.е. на
    // дистанции 10.0-6.5=3.5 от origin y=10).
    let origin_cap = [10.0f32, 10.0, 0.0];
    let hit = (h.api.raycast)(h.instance, origin_cap.as_ptr(), dir.as_ptr(), 100.0, -1);
    assert_eq!(hit.hit, 1);
    assert_eq!(hit.body, cap_id, "raycast попал не в ту капсулу");
    assert!((hit.distance - 3.5).abs() < 0.05, "неверная дистанция до капсулы: {}", hit.distance);
    println!("[OK] raycast vs capsule: distance={:.3} (ожидалось ~3.5), body={}", hit.distance, hit.body);

    // Луч мимо всех тел, вниз — должен попасть в плоскость.
    let origin_plane = [50.0f32, 10.0, 50.0];
    let hit = (h.api.raycast)(h.instance, origin_plane.as_ptr(), dir.as_ptr(), 100.0, -1);
    assert_eq!(hit.hit, 1);
    assert_eq!(hit.is_plane, 1, "raycast должен был попасть в плоскость");
    assert!((hit.distance - 10.0).abs() < 0.05, "неверная дистанция до плоскости: {}", hit.distance);
    println!("[OK] raycast vs plane: distance={:.3} (ожидалось ~10.0)", hit.distance);

    // exclude_body: тот же луч, что и в тест сферы, но с исключённой
    // сферой — должен пролететь мимо неё и попасть в плоскость под ней.
    let hit = (h.api.raycast)(h.instance, origin.as_ptr(), dir.as_ptr(), 100.0, sphere_id);
    assert_eq!(hit.hit, 1);
    assert_eq!(hit.is_plane, 1, "exclude_body не исключил сферу — луч всё ещё попадает в неё");
    println!("[OK] raycast exclude_body: сфера исключена, луч прошёл сквозь неё в плоскость");

    // Луч в пустоту (высоко над всем, вбок) — не должен попасть никуда.
    let origin_miss = [0.0f32, 10.0, 0.0];
    let dir_up = [0.0f32, 1.0, 0.0];
    let hit = (h.api.raycast)(h.instance, origin_miss.as_ptr(), dir_up.as_ptr(), 5.0, -1);
    assert_eq!(hit.hit, 0, "raycast нашёл попадание там, где ничего быть не должно");
    println!("[OK] raycast miss: честно ничего не нашёл");
}

fn main() {
    test_box_vs_box_stops_falling_box();
    test_box_vs_sphere_stops_falling_sphere();
    test_box_vs_plane_holds_rotated_box();
    test_capsule_vs_sphere();
    test_capsule_vs_box();
    test_capsule_vs_capsule();
    test_capsule_vs_plane();
    test_raycast();
    println!("\nВсе проверки новой физики (box-box, capsule, raycast) пройдены.");
}
