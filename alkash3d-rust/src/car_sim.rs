// src/car_sim.rs
//! Аркадная симуляция вождения для My Summer Car-like демо (`main_car.rs`).
//!
//! ПОЧЕМУ не через `alkash3d-inertial` (физический плагин движка): у
//! `PhysicsBody`/`inertial.dll` в этой версии движка НЕТ понятия формы тела
//! вообще — узкая фаза всегда sphere-sphere с ОДНИМ фиксированным implicit-
//! радиусом на все тела (см. `IMPLICIT_RADIUS` в комментариях
//! `main_car.rs::setup_physics_and_car`), то есть кузов машины физически
//! вёл бы себя как шар ~0.5м радиуса, даже будучи нарисованным как
//! коробка 1.8×1.2×4м — заметно меньше видимого кузова, машина визуально
//! "проваливалась" бы в стены/предметы ДО того, как физика вообще
//! почувствовала бы контакт. Плюс `PhysicsAPI` (см. `plugin/physics_api.rs`)
//! не даёт способа ВЛИЯТЬ на уже созданное тело после `add_body` — нет ни
//! `apply_force`, ни `set_velocity`, только `get_body` на чтение — то есть
//! честное "нажал W — машина поехала" потребовало бы новой FFI-функции,
//! пересборки `inertial.dll` (Fortran, отдельный MinGW-тулчейн) и риска
//! сломать уже работающую физику падения/качения из задач #39-#40.
//!
//! Вместо этого — стандартный для аркадных гоночных игр подход: простая
//! "bicycle model" (руль поворачивает не колёса по отдельности, а
//! эффективный угол поворота всей оси) без переусложнения, полностью в
//! Rust, без завязки на Inertial — предсказуемая, отзывчивая на ввод,
//! легко настраиваемая (константы в `CarParams`). Столкновения со стенами
//! гаража/границами площадки — свой отдельный 2D circle-vs-AABB тест
//! (`resolve_wall_collisions`), а не через Inertial.
//!
//! `Inertial` при этом продолжает честно использоваться в сцене для ДРУГИХ
//! объектов (бочки/ящики — см. `spawn_physics_sphere` в `main_car.rs`),
//! которым sphere-физика подходит без компромиссов.

use crate::math::Vec3;

/// Прямоугольная зона столкновения в плоскости XZ (мировые координаты) —
/// используется для стен гаража и границ площадки. Не хранит высоту — вся
/// сцена демо плоская, коллизия по Y не нужна (см. `CarParams::ground_y`).
#[derive(Debug, Clone, Copy)]
pub struct WallAabb {
    pub min_x: f32,
    pub max_x: f32,
    pub min_z: f32,
    pub max_z: f32,
}

impl WallAabb {
    pub fn from_center_half_extents(center_x: f32, center_z: f32, half_x: f32, half_z: f32) -> Self {
        Self {
            min_x: center_x - half_x,
            max_x: center_x + half_x,
            min_z: center_z - half_z,
            max_z: center_z + half_z,
        }
    }
}

/// Настройки конкретной машины — вынесены из констант в структуру, чтобы
/// `main_car.rs` мог задать их один раз при спавне и не размазывать по
/// коду вызовов `step`.
#[derive(Debug, Clone, Copy)]
pub struct CarParams {
    pub max_speed_forward: f32,
    pub max_speed_reverse: f32,
    pub acceleration: f32,
    pub reverse_acceleration: f32,
    pub brake_decel: f32,
    pub coast_decel: f32,
    pub max_steer_angle: f32,
    pub steer_speed: f32,
    pub wheelbase: f32,
    pub wheel_radius: f32,
    /// Радиус условной "окружности столкновения" машины (см. шапку файла —
    /// почему не честный OBB) — берётся чуть меньше половины диагонали
    /// кузова, чтобы машина могла заезжать в проём гаража, а не застревать
    /// на его углах.
    pub collision_radius: f32,
    /// Y кузова над землёй в состоянии покоя (используется как константа —
    /// сцена плоская, полноценной подвески/раскачивания на кочках нет,
    /// см. шапку файла).
    pub ground_y: f32,
    /// Насколько сильно нос кузова визуально "клюёт" при торможении и
    /// "приседает" при разгоне (перенос веса) — чисто косметический эффект
    /// (не влияет на физику/позицию), подмешивается плавно через
    /// `pitch_smoothing`. ПОЧЕМУ именно тангаж (pitch), а не крен (roll) в
    /// повороте, как в большинстве аркадных гонок: `Transform::local_matrix`
    /// (`scene.rs`) считает мировую матрицу как `Rz*Ry*Rx` — вращение
    /// вокруг X (тангаж) применяется К СЫРОЙ локальной геометрии ПЕРВЫМ,
    /// то есть корректно "относительно курса" независимо от текущего yaw.
    /// А вот вращение вокруг Z (крен) применяется ПОСЛЕДНИМ, вокруг
    /// МИРОВОЙ оси Z — то есть при курсе, отличном от исходного (yaw≠0),
    /// такой "крен" клонил бы машину в фиксированном мировом направлении,
    /// а не относительно её реального борта — заметно неправильно выглядело
    /// бы на поворотах после того, как машина уже развернулась. Эйлеровы
    /// углы этого Transform просто не дают чистого слота "вращение вокруг
    /// ЛОКАЛЬНОЙ оси Z после yaw" без перехода на кватернионы, поэтому
    /// используется тангаж — единственная ось из трёх, для которой этот
    /// порядок композиции даёт физически осмысленный (relative to heading)
    /// результат.
    pub pitch_factor: f32,
    pub pitch_smoothing: f32,
}

impl Default for CarParams {
    fn default() -> Self {
        Self {
            max_speed_forward: 22.0,   // ~79 км/ч
            max_speed_reverse: 6.0,
            acceleration: 6.0,
            reverse_acceleration: 3.0,
            brake_decel: 14.0,
            coast_decel: 2.5,
            max_steer_angle: 32.0f32.to_radians(),
            steer_speed: 2.6,
            wheelbase: 2.6,
            wheel_radius: 0.35,
            collision_radius: 1.05,
            ground_y: 0.0,
            pitch_factor: 0.028,
            pitch_smoothing: 7.0,
        }
    }
}

/// Управляющий ввод текущего кадра — заполняется `main_car.rs` из
/// `engine.input`, сама симуляция ничего не знает про клавиши/винды.
#[derive(Debug, Clone, Copy, Default)]
pub struct CarInput {
    /// -1.0 (полный газ назад/тормоз) .. 1.0 (полный газ вперёд).
    pub throttle: f32,
    /// -1.0 (руль влево до упора) .. 1.0 (вправо).
    pub steer: f32,
    pub handbrake: bool,
}

/// Полное состояние одной машины между кадрами.
#[derive(Debug, Clone, Copy)]
pub struct CarState {
    /// Положение опорной точки кузова (центр, на высоте `ground_y`) в
    /// мировых координатах.
    pub position: Vec3,
    /// Курс машины (радианы, вращение вокруг Y). Направление "вперёд" —
    /// `(sin(yaw), 0, cos(yaw))`, то же соглашение, что и `car_forward()`.
    pub yaw: f32,
    /// Скорость вдоль курса — знаковая (положительная = вперёд,
    /// отрицательная = назад), метры в секунду.
    pub speed: f32,
    /// Текущий (сглаженный к целевому) угол поворота руля, радианы.
    pub steer_angle: f32,
    /// Накопленный угол вращения колёс — для визуального "качения",
    /// не нормализован (просто растёт/убывает, рендер сам возьмёт по mod
    /// 2π через тригонометрию).
    pub wheel_spin: f32,
    /// Сглаженный визуальный тангаж (нос вниз при торможении/вверх при
    /// разгоне) — косметика, не влияет на симуляцию. См. подробное
    /// объяснение выбора именно этой оси у `CarParams::pitch_factor`.
    pub visual_pitch: f32,
}

impl CarState {
    pub fn new(position: Vec3, yaw: f32) -> Self {
        Self {
            position,
            yaw,
            speed: 0.0,
            steer_angle: 0.0,
            wheel_spin: 0.0,
            visual_pitch: 0.0,
        }
    }

    /// Единичный вектор "вперёд" машины в мировых координатах.
    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    /// Один шаг симуляции: обновляет руль/скорость/курс/позицию по вводу
    /// этого кадра, затем разрешает столкновения со стенами `colliders` и
    /// границами площадки. `dt` — в секундах, ожидается уже "зажатый"
    /// вызывающим кодом (см. `run_loop` в `main_car.rs` — `dt.min(0.05)`),
    /// эта функция сама dt не ограничивает.
    pub fn step(&mut self, input: CarInput, params: &CarParams, dt: f32, colliders: &[WallAabb], bounds: WallAabb) {
        let prev_speed = self.speed;

        // --- Руль: плавно едет к целевому углу, не телепортируется ---
        let target_steer = input.steer.clamp(-1.0, 1.0) * params.max_steer_angle;
        let max_delta = params.steer_speed * dt;
        let diff = (target_steer - self.steer_angle).clamp(-max_delta, max_delta);
        self.steer_angle += diff;

        // --- Газ/тормоз/накат ---
        if input.handbrake {
            let brake = params.brake_decel * 1.6 * dt;
            self.speed = move_toward(self.speed, 0.0, brake);
        } else if input.throttle > 0.01 {
            if self.speed < 0.0 {
                // едем назад, а газ дан вперёд — сперва честно тормозим,
                // не "телепортируя" скорость через ноль.
                self.speed = move_toward(self.speed, 0.0, params.brake_decel * dt);
            } else {
                self.speed = (self.speed + params.acceleration * input.throttle * dt)
                    .min(params.max_speed_forward);
            }
        } else if input.throttle < -0.01 {
            if self.speed > 0.0 {
                self.speed = move_toward(self.speed, 0.0, params.brake_decel * (-input.throttle) * dt);
            } else {
                self.speed = (self.speed + params.reverse_acceleration * input.throttle * dt)
                    .max(-params.max_speed_reverse);
            }
        } else {
            // Ни газа, ни тормоза — естественное замедление (сопротивление
            // качению/двигателя), как и у реальной машины на нейтрали.
            self.speed = move_toward(self.speed, 0.0, params.coast_decel * dt);
        }

        // --- Курс: bicycle model — скорость поворота руля пропорциональна
        // скорости и tan(угол руля), делённым на колёсную базу. На месте
        // (speed≈0) машина не крутится вокруг своей оси, как и в жизни. ---
        let yaw_rate = if self.speed.abs() > 0.02 {
            (self.speed / params.wheelbase) * self.steer_angle.tan()
        } else {
            0.0
        };
        self.yaw += yaw_rate * dt;

        // --- Позиция ---
        let heading = self.forward();
        let mut new_pos = self.position + heading * (self.speed * dt);

        resolve_wall_collisions(&mut new_pos, &mut self.speed, params.collision_radius, colliders);
        clamp_to_bounds(&mut new_pos, &mut self.speed, bounds);

        self.position = new_pos;
        self.position.y = params.ground_y;

        // --- Визуал: качение колёс + тангаж от продольного ускорения (см.
        // подробное обоснование выбора оси у `CarParams::pitch_factor`) ---
        if params.wheel_radius > 1e-4 {
            self.wheel_spin += (self.speed / params.wheel_radius) * dt;
        }
        let longitudinal_accel = if dt > 1e-5 { (self.speed - prev_speed) / dt } else { 0.0 };
        let target_pitch = (-longitudinal_accel * params.pitch_factor).clamp(-0.12, 0.12);
        let pitch_t = (params.pitch_smoothing * dt).clamp(0.0, 1.0);
        self.visual_pitch += (target_pitch - self.visual_pitch) * pitch_t;
    }
}

#[inline]
fn move_toward(value: f32, target: f32, max_delta: f32) -> f32 {
    if (target - value).abs() <= max_delta {
        target
    } else {
        value + max_delta * (target - value).signum()
    }
}

/// Разрешает столкновения машины (представленной кругом радиуса `radius`)
/// со списком прямоугольных стен: находит ближайшую точку каждого AABB к
/// центру машины, и если она ближе `radius` — выталкивает машину наружу
/// вдоль нормали и гасит скорость (простое "бап" вместо честного скольжения
/// вдоль стены — см. подробное обоснование этого упрощения в шапке файла:
/// `speed` тут скаляр вдоль курса, а не полный 2D-вектор, поэтому разложить
/// его на "вдоль стены"/"в стену" отдельно от курса некорректно).
fn resolve_wall_collisions(pos: &mut Vec3, speed: &mut f32, radius: f32, colliders: &[WallAabb]) {
    for wall in colliders {
        let closest_x = pos.x.clamp(wall.min_x, wall.max_x);
        let closest_z = pos.z.clamp(wall.min_z, wall.max_z);
        let dx = pos.x - closest_x;
        let dz = pos.z - closest_z;
        let dist_sq = dx * dx + dz * dz;
        if dist_sq >= radius * radius {
            continue;
        }
        let dist = dist_sq.sqrt();
        let (nx, nz) = if dist > 1e-4 {
            (dx / dist, dz / dist)
        } else {
            // Центр машины уже внутри AABB (за один кадр её туда занесло,
            // например при спавне, или машина протаранила тонкую стену на
            // высокой скорости) — выталкиваем по кратчайшей оси, чтобы не
            // оставить деление на ноль.
            //
            // ИСПРАВЛЕНО (код-ревью): раньше знак направления был всегда
            // +1.0 на выбранной оси, независимо от того, к какому краю
            // (min или max) центр был БЛИЖЕ — то есть если центр оказался
            // ближе к min_x, машину всё равно толкало в сторону +x, то
            // есть ГЛУБЖЕ в стену вместо наружу через ближний край, где
            // она реально в неё вошла. Теперь для каждой оси явно
            // сравниваются расстояния до min- и max-края, и знак толчка
            // выбирается в сторону БЛИЖНЕГО края.
            let dist_to_min_x = pos.x - wall.min_x;
            let dist_to_max_x = wall.max_x - pos.x;
            let dist_to_min_z = pos.z - wall.min_z;
            let dist_to_max_z = wall.max_z - pos.z;
            let push_x = dist_to_min_x.min(dist_to_max_x);
            let push_z = dist_to_min_z.min(dist_to_max_z);
            if push_x < push_z {
                if dist_to_min_x < dist_to_max_x { (-1.0, 0.0) } else { (1.0, 0.0) }
            } else {
                if dist_to_min_z < dist_to_max_z { (0.0, -1.0) } else { (0.0, 1.0) }
            }
        };
        let penetration = radius - dist;
        pos.x += nx * penetration;
        pos.z += nz * penetration;
        *speed *= 0.25;
    }
}

/// Держит машину внутри границ площадки `bounds` (мягкий "невидимый забор"
/// по краю площадки) — выталкивает внутрь и гасит скорость так же, как
/// `resolve_wall_collisions` гасит удар о стену, чтобы игрок не укатил в
/// пустоту за пределами сгенерированной земли.
pub fn clamp_to_bounds(pos: &mut Vec3, speed: &mut f32, bounds: WallAabb) {
    let mut hit = false;
    if pos.x < bounds.min_x {
        pos.x = bounds.min_x;
        hit = true;
    } else if pos.x > bounds.max_x {
        pos.x = bounds.max_x;
        hit = true;
    }
    if pos.z < bounds.min_z {
        pos.z = bounds.min_z;
        hit = true;
    } else if pos.z > bounds.max_z {
        pos.z = bounds.max_z;
        hit = true;
    }
    if hit {
        *speed *= 0.25;
    }
}
