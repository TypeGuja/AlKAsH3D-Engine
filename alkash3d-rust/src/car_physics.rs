// src/car_physics.rs
//! Реальная физика вождения через Inertial (в отличие от `car_sim.rs` —
//! аркадной кинематики, оставленной НЕТРОНУТОЙ и по-прежнему доступной,
//! просто не используемой машиной игрока в `main_car.rs`).
//!
//! Кузов — настоящее твёрдое тело в Inertial (box-коллайдер, см.
//! `PhysicsBody::shape_type`/`AlkashEngine::add_box_body`). Ничего не
//! телепортируется и не пишется в позицию/скорость напрямую — ЕДИНСТВЕННЫЙ
//! канал влияния на машину — `apply_physics_force_at_point` (сила плюс
//! момент, см. его комментарий в `engine/physics_bridge.rs`), тот же
//! механизм, каким в реальности на кузов действуют настоящие пружины
//! подвески и трение шин о дорогу. Интегрирование, столкновения с бочками,
//! сон/пробуждение — всё это уже честно считает Fortran-солвер
//! (`solve_contacts_vectorized`/`batch_integrate`), эта функция лишь решает,
//! КАКИЕ силы приложить в этом кадре.
//!
//! Подвеска — по одному вертикальному raycast'у на колесо (см. упрощение в
//! комментарии внутри `step`) + классическая пружина-демпфер. Тяга/
//! торможение/руление — продольная и поперечная сила трения в точке
//! колеса, ограниченная кругом трения (friction circle) от текущей
//! нормальной силы подвески — то есть газ на разгруженном (после прыжка,
//! например) колесе честно даёт меньше тяги, а не одинаковую всегда.

use crate::engine::AlkashEngine;
use crate::math::{Quat, Vec3};

/// Настройки одной машины — вынесены в структуру по тому же принципу, что
/// и `car_sim::CarParams`, чтобы `main_car.rs` мог задать их один раз при
/// спавне.
#[derive(Debug, Clone, Copy)]
pub struct CarPhysicsParams {
    /// Длина подвески в состоянии покоя (метры) — расстояние от точки
    /// крепления колеса на кузове до земли, при котором пружина не сжата
    /// и не растянута.
    pub suspension_rest_length: f32,
    /// Максимальный ход подвески СВЕРХ состояния покоя (сжатие).
    pub suspension_max_travel: f32,
    /// Жёсткость пружины, Н/м.
    pub suspension_stiffness: f32,
    /// Демпфирование, Н·с/м.
    pub suspension_damping: f32,
    /// Коэффициент трения шины о землю (используется как радиус круга
    /// трения — `mu * нормальная_сила_подвески`).
    pub tire_friction: f32,
    /// Максимальная продольная сила тяги на ОДНО колесо, Н (двигатель).
    pub engine_force: f32,
    /// Максимальная тормозная сила на ОДНО колесо, Н (ручник/тормоз).
    pub brake_force: f32,
    /// "Жёсткость" бокового сцепления шины, Н/(м/с) — насколько сильно
    /// шина сопротивляется боковому проскальзыванию (упрощённая линейная
    /// модель вместо честной кривой Pacejka — см. комментарий в `step`).
    pub lateral_stiffness: f32,
    pub max_steer_angle: f32,
    pub steer_speed: f32,
}

impl Default for CarPhysicsParams {
    fn default() -> Self {
        Self {
            suspension_rest_length: 0.42,
            suspension_max_travel: 0.16,
            suspension_stiffness: 45000.0,
            suspension_damping: 4200.0,
            tire_friction: 1.2,
            engine_force: 3200.0,
            brake_force: 7000.0,
            lateral_stiffness: 14000.0,
            max_steer_angle: 32.0f32.to_radians(),
            steer_speed: 2.6,
        }
    }
}

/// Управляющий ввод текущего кадра — та же форма, что и `car_sim::CarInput`
/// (независимая копия, не общий тип: этот модуль сознательно не зависит от
/// `car_sim`, см. шапку файла).
#[derive(Debug, Clone, Copy, Default)]
pub struct CarInput {
    pub throttle: f32,
    pub steer: f32,
    pub handbrake: bool,
}

/// Единственное состояние, которое эта модель хранит САМА между кадрами —
/// угол руля (плавно едет к целевому, как и в `car_sim::CarState`). Всё
/// остальное (позиция/скорость/ориентация) — честно читается из
/// физического тела каждый кадр через `get_physics_body`, а не кэшируется
/// здесь, чтобы не могло разойтись с тем, что реально посчитал солвер.
#[derive(Debug, Clone, Copy, Default)]
pub struct CarPhysicsState {
    pub steer_angle: f32,
}

impl CarPhysicsState {
    /// Один шаг: обновляет угол руля, затем считает и прикладывает силы
    /// подвески + тяги/торможения/поворота на все 4 колеса. Вызывать ДО
    /// `engine.update()` этого же кадра (там реально происходит
    /// интегрирование — эта функция только копит силы в аккумуляторе
    /// физического плагина).
    ///
    /// `wheel_local_positions` — [FL, FR, RL, RR] в ЛОКАЛЬНЫХ осях кузова
    /// (см. `CarHandle::wheel_local_positions`/`spawn_physics_car`) —
    /// индексы 0,1 (FL/FR) считаются передними (получают угол руля и НЕ
    /// получают тягу — переднеприводность здесь сознательно не нужна,
    /// см. ниже), индексы 2,3 (RL/RR) — задними (тяга/торможение).
    pub fn step(
        &mut self,
        engine: &mut AlkashEngine,
        body_id: i32,
        wheel_local_positions: &[[f32; 3]; 4],
        input: CarInput,
        params: &CarPhysicsParams,
        ground_y: f32,
        dt: f32,
    ) {
        let target_steer = input.steer.clamp(-1.0, 1.0) * params.max_steer_angle;
        let max_delta = params.steer_speed * dt;
        let diff = (target_steer - self.steer_angle).clamp(-max_delta, max_delta);
        self.steer_angle += diff;

        let Some(body) = engine.get_physics_body(body_id) else { return };
        let orientation = Quat::from_xyzw(
            body.orientation[0],
            body.orientation[1],
            body.orientation[2],
            body.orientation[3],
        );
        let com = Vec3::from(body.position);
        let lin_vel = Vec3::from(body.velocity);
        let ang_vel = Vec3::from(body.angular_velocity);

        for (i, local_pos) in wheel_local_positions.iter().enumerate() {
            let local_pos = Vec3::from(*local_pos);
            // Мировое смещение точки крепления колеса от центра масс —
            // ЭТО и есть плечо, которым сила подвески/тяги рождает момент
            // (см. `apply_physics_force_at_point`), не только линейное
            // ускорение.
            let r = orientation * local_pos;
            let world_attach = com + r;

            // ВАЖНО (сознательное упрощение): raycast строго вниз в
            // МИРОВЫХ координатах (не вдоль локальной оси стойки
            // подвески, повёрнутой вместе с кузовом) — корректно для
            // плоской земли и машины без экстремальных кренов/тангажей
            // (чего в этой демо-сцене и не происходит — сцена плоская).
            // Честный raycast против реальной геометрии земли потребовал
            // бы, чтобы земля вообще была физическим телом (сейчас это
            // просто визуальный меш, см. `main_car.rs::setup_ground`) —
            // не стоит того усложнения ради плоской площадки.
            let ground_dist = world_attach.y - ground_y;
            let max_reach = params.suspension_rest_length + params.suspension_max_travel;
            if ground_dist > max_reach {
                continue; // колесо в воздухе — подвеска этого колеса ничего не прикладывает
            }
            let compression = (params.suspension_rest_length - ground_dist)
                .clamp(0.0, params.suspension_max_travel);

            // Скорость ИМЕННО этой точки кузова (не центра масс!) —
            // стандартная формула твёрдого тела v_point = v_com + ω×r.
            let point_vel = lin_vel + ang_vel.cross(r);

            // compression = rest_length - ground_dist, значит
            // d(compression)/dt = -d(ground_dist)/dt = -point_vel.y.
            let compression_rate = -point_vel.y;
            let suspension_force =
                (params.suspension_stiffness * compression + params.suspension_damping * compression_rate)
                    .max(0.0); // пружина может только ТОЛКАТЬ, не тянуть

            let is_front = i < 2;
            // ИСПРАВЛЕНО (баг "руль поворачивается, машина не поворачивает"):
            // `local_right` ОБЯЗАН поворачиваться вместе с `local_forward` на
            // тот же `steer_angle` (остаётся перпендикулярен ему) — иначе
            // боковая сила увода шины (`lat_force` ниже, см. её комментарий)
            // считалась бы вдоль оси КУЗОВА, как будто руль всегда прямо, и
            // передние колёса физически вели бы себя неотличимо от задних. А
            // ведь именно эта сила (реакция шины на то, что повёрнутое
            // колесо "смотрит" не туда, куда реально едет кузов) и создаёт
            // основной момент поворота у настоящей машины — без неё
            // оставалась лишь слабая добавка от вектора тяги.
            let (local_forward, local_right) = if is_front {
                (
                    Vec3::new(self.steer_angle.sin(), 0.0, self.steer_angle.cos()),
                    Vec3::new(self.steer_angle.cos(), 0.0, -self.steer_angle.sin()),
                )
            } else {
                (Vec3::Z, Vec3::X)
            };
            let forward_world = (orientation * local_forward).normalize_or_zero();
            let right_world = (orientation * local_right).normalize_or_zero();

            let v_long = point_vel.dot(forward_world);
            let v_lat = point_vel.dot(right_world);

            let max_friction = params.tire_friction * suspension_force;

            // Продольная сила: газ — постоянная (до предела трения) сила
            // тяги; ручник/тормоз — сильный демпфер, гасящий продольную
            // скорость (реалистично приходит к остановке без раскачки
            // благодаря клэмпу по `brake_force`, вместо мгновенного
            // обнуления скорости).
            // ИСПРАВЛЕНО (см. коммент в шапке файла — "передние НЕ получают
            // тягу, переднеприводность здесь сознательно не нужна"): этой
            // проверки раньше не было — газ прикладывался ко всем 4 колёсам
            // одинаково (фактический полный привод вместо документированного
            // заднего), из-за чего передние колёса ещё и толкали кузов вбок
            // при повороте руля, маскируя основной баг выше. Ручник же
            // должен тормозить ВСЕ колёса, как в жизни — его не трогаем.
            let mut long_force = if input.handbrake {
                (-v_long * 6000.0).clamp(-params.brake_force, params.brake_force)
            } else if is_front {
                0.0
            } else {
                input.throttle.clamp(-1.0, 1.0) * params.engine_force
            };
            long_force = long_force.clamp(-max_friction, max_friction);

            // Круг трения: то, что уже "потрачено" на продольную силу,
            // недоступно боковому сцеплению — иначе газ+руль одновременно
            // давали бы физически невозможное суммарное сцепление больше
            // максимума одной шины.
            let remaining_sq = (max_friction * max_friction - long_force * long_force).max(0.0);
            let remaining = remaining_sq.sqrt();
            // Упрощённая (линейная, не честная кривая Pacejka — см. шапку
            // файла) модель бокового сцепления: сила пропорциональна
            // боковой скорости проскальзывания, ограничена оставшимся
            // кругом трения. Даёт реалистичное "плывущее" поведение на
            // пределе сцепления (занос), не только жёсткое прилипание.
            //
            // ИСПРАВЛЕНО (баг "дрифт при движении и повороте назад" после
            // фикса руления выше): `-lateral_stiffness * v_lat` — пружина-
            // демпфер, которую солвер интегрирует ЯВНО (explicit Euler,
            // см. `batch_integrate` в alkash3d-inertial). У такой схемы
            // есть порог устойчивости: если сила за один кадр гасит боковую
            // скорость КОЛЕСА больше, чем нужно для её обнуления, знак
            // v_lat переворачивается — и на следующем кадре сила снова
            // толкает мимо нуля, в другую сторону, с ещё большей
            // амплитудой (классическая раскачка демпфера с explicit-Euler,
            // а не честный физический занос). Раньше это было незаметно,
            // потому что у передних колёс `right_world` не поворачивался
            // вместе с рулём (см. фикс выше) — боковая сила там была
            // слабее и по факту эквивалентна задним колёсам. С честной
            // боковой силой раскачка стала заметна — особенно на заднем
            // ходу, где неуправляемая (задняя) ось оказывается ВПЕРЕДИ по
            // направлению движения, а это пассивно неустойчивая
            // конфигурация (как толкать тележку назад — она вихляет), и
            // любая численная раскачка там физически усиливается. Фикс:
            // не позволяем силе ОДНОГО колеса за один кадр перегасить его
            // боковую скорость мимо нуля — берём консервативную (полная
            // масса кузова, не доля на колесо, то есть реальный физический
            // предел мягче этого) верхнюю границу, всё ещё честную для
            // explicit-Euler шага любой длины (включая просевшие кадры,
            // dt до 0.05с — см. `main_car.rs::run_loop`).
            let max_stopping_force = v_lat.abs() * body.mass / dt.max(1.0e-4);
            let lat_force = (-params.lateral_stiffness * v_lat)
                .clamp(-remaining, remaining)
                .clamp(-max_stopping_force, max_stopping_force);

            let total_force = Vec3::new(0.0, suspension_force, 0.0)
                + forward_world * long_force
                + right_world * lat_force;

            engine.apply_physics_force_at_point(body_id, total_force.to_array(), world_attach.to_array());
        }
    }
}
