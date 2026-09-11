// src/plugin/physics_api.rs
//! API для физического плагина

use std::ffi::c_void;

/// Конфигурация физики
#[repr(C)]
pub struct PhysicsConfig {
    pub max_bodies: i32,
    pub world_size: f32,
    pub cell_size: f32,
    pub solver_iterations: i32,
    pub use_simd: i32,
}

/// Структура тела (совместима с Fortran)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsBody {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angular_acceleration: [f32; 3],
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub is_static: i32,
    pub is_asleep: i32,
    /// ДОБАВЛЕНО (физика автомобиля — вращение кузова): кватернион
    /// ориентации (x, y, z, w) — СИММЕТРИЧНО зеркалит новое поле
    /// `PhysicsBody::orientation` в `alkash3d-inertial/src/lib.rs`
    /// (ABI-контракт между движком и `inertial.dll`). Единичный
    /// кватернион `[0,0,0,1]` = без поворота — см. `add_sphere_body`
    /// ниже в `engine/mod.rs`, где новые тела инициализируются им.
    /// добавлять новые поля можно только строго в
    /// конец, не переставляя существующие (иначе весь layout после
    /// изменённого поля разъедется между движком и плагином).
    pub orientation: [f32; 4],
    /// ДОБАВЛЕНО (код-ревью — per-body радиус столкновения вместо одного
    /// глобального `IMPLICIT_RADIUS=0.5` на все тела без исключения) —
    /// СИММЕТРИЧНО зеркалит новое поле `PhysicsBody::radius` в
    /// `alkash3d-inertial/src/lib.rs`.
    pub radius: f32,
    /// ДОБАВЛЕНО (реальная физика машины — box-коллайдер кузова):
    /// СИММЕТРИЧНО зеркалит `shape_type`/`half_extents` в
    /// `alkash3d-inertial/src/lib.rs` — см. подробный комментарий там.
    /// `0` (см. `shape_type::SPHERE`) = сфера (использует `radius` выше),
    /// `1` (`shape_type::BOX`) = коробка (`half_extents` — половинные
    /// размеры по локальным осям тела). ЭТИ ДВА ПОЛЯ — ПОСЛЕДНИЕ в
    /// структуре, та же конвенция "только в конец", что и у
    /// `orientation`/`radius` выше.
    pub shape_type: i32,
    pub half_extents: [f32; 3],
}

/// Дискриминанты `PhysicsBody::shape_type` — см. его комментарий.
pub mod shape_type {
    pub const SPHERE: i32 = 0;
    pub const BOX: i32 = 1;
}

/// Структура контакта
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
}

/// ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API): типы
/// соединений `ConstraintDesc::joint_type`/`ConstraintInfo::joint_type`.
/// ЗНАЧЕНИЯ ДОЛЖНЫ побайтово совпадать с `joint_type` в
/// `alkash3d-inertial/src/lib.rs` (и, транзитивно, с `JOINT_*` в
/// `alkash3d-inertial/src/kernels/rigid_body.f90`) — эта DLL не является
/// Cargo-зависимостью движка (см. комментарий в шапке `inertial/src/lib.rs`
/// про то, что ABI между ними — совпадение layout по соглашению, а не
/// общий Rust-тип), поэтому синхронизация констант — вручную.
pub mod joint_type {
    /// Шаровой шарнир — только точка крепления, вращение свободно.
    pub const BALL: i32 = 0;
    /// Петля — точка крепления + вращение только вокруг `axis_a` (дверь,
    /// капот, крышка багажника).
    pub const HINGE: i32 = 1;
    /// Жёсткая сварка/болтовое соединение — точка крепления + вращение
    /// полностью заперто (пока соединение не разрушено).
    pub const FIXED: i32 = 2;
    /// Ползун — свободное смещение вдоль `axis_a`, перпендикулярные оси
    /// заперты, вращение не ограничивается.
    pub const SLIDER: i32 = 3;
}

/// Описание нового соединения для `PhysicsAPI::add_constraint`.
/// `body_a`/`body_b` — СТАБИЛЬНЫЕ handle'ы, возвращённые `add_body` (тот
/// же контракт, что уже использует весь остальной API — `get_body`/
/// `remove_body`), а НЕ индексы во внутреннем солвере.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConstraintDesc {
    pub body_a: i32,
    pub body_b: i32,
    /// См. `joint_type` выше.
    pub joint_type: i32,
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
    /// Используется JOINT_HINGE/JOINT_SLIDER, игнорируется JOINT_BALL/
    /// JOINT_FIXED. Мировые координаты — ось НЕ поворачивается вместе с
    /// телом (см. то же ограничение у `anchor_a`/`anchor_b` в
    /// `rigid_body.f90`/`constraint_c`).
    pub axis_a: [f32; 3],
    pub axis_b: [f32; 3],
    /// "Жёсткость" соединения (доля ошибки, устраняемая за итерацию
    /// солвера). Типично 1.0; меньше — мягче, заметно больше 1.0может
    /// вызывать колебания.
    pub bias: f32,
    /// Порог суммарного линейного импульса ЗА ШАГ физики, после которого
    /// соединение ломается. `<= 0` — неразрушимо.
    pub break_impulse_linear: f32,
    /// То же для углового (скручивающего/изгибающего) импульса.
    pub break_impulse_angular: f32,
}

impl Default for ConstraintDesc {
    fn default() -> Self {
        Self {
            body_a: -1,
            body_b: -1,
            joint_type: joint_type::BALL,
            anchor_a: [0.0; 3],
            anchor_b: [0.0; 3],
            axis_a: [0.0, 1.0, 0.0],
            axis_b: [0.0, 1.0, 0.0],
            bias: 1.0,
            break_impulse_linear: 0.0,
            break_impulse_angular: 0.0,
        }
    }
}

/// ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость): зеркало
/// `PlaneDesc` в `alkash3d-inertial/src/lib.rs` — см. там подробное
/// обоснование (в т.ч. почему это ТОЛЬКО для пола, а не для стен
/// ограниченного размера). `normal` — нормаль полупространства (нормируется
/// на стороне плагина), `point` — любая точка на плоскости.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlaneDesc {
    pub normal: [f32; 3],
    pub point: [f32; 3],
    pub friction: f32,
    pub restitution: f32,
}

impl Default for PlaneDesc {
    fn default() -> Self {
        Self {
            normal: [0.0, 1.0, 0.0],
            point: [0.0, 0.0, 0.0],
            friction: 0.5,
            restitution: 0.0,
        }
    }
}

/// ДОБАВЛЕНО (полноценная физика — запрос луча против сцены): зеркало
/// `RaycastHit` в `alkash3d-inertial/src/lib.rs` — см. там подробное
/// обоснование. `hit == 0` означает "ничего не найдено в пределах
/// max_dist" — остальные поля в этом случае нулевые/недостоверны.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RaycastHit {
    pub hit: i32,
    pub distance: f32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    /// Стабильный handle тела — ТОЛЬКО если `is_plane == 0`, иначе `-1`.
    pub body: i32,
    /// Порядковый номер плоскости (см. `PhysicsAPI::add_plane`) — ТОЛЬКО
    /// если `is_plane != 0`, иначе `-1`.
    pub plane_index: i32,
    pub is_plane: i32,
}

/// Текущее состояние соединения — см. `PhysicsAPI::get_constraint`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConstraintInfo {
    pub body_a: i32,
    pub body_b: i32,
    pub joint_type: i32,
    pub is_broken: i32,
    /// Суммарный импульс ПОСЛЕДНЕГО решённого физического шага — удобно
    /// для HUD/отладки "насколько близко к разрушению", не только для
    /// бинарного `is_broken`.
    pub linear_impulse: [f32; 3],
    pub angular_impulse: [f32; 3],
}

/// API физического плагина
#[repr(C)]
#[derive(Clone, Copy)]  // <-- Добавлено
pub struct PhysicsAPI {
    // Управление телами
    pub add_body: extern "C" fn(instance: *mut c_void, body: *const PhysicsBody) -> i32,
    pub remove_body: extern "C" fn(instance: *mut c_void, id: i32),
    pub get_body: extern "C" fn(instance: *mut c_void, id: i32) -> PhysicsBody,
    pub get_bodies_count: extern "C" fn(instance: *mut c_void) -> i32,

    // Обновление
    pub update: extern "C" fn(instance: *mut c_void, dt: f32, gravity: f32),

    // Получение результатов
    pub get_contacts: extern "C" fn(instance: *mut c_void) -> *const PhysicsContact,
    pub get_contacts_count: extern "C" fn(instance: *mut c_void) -> i32,

    // Broad phase пары
    pub get_pairs: extern "C" fn(instance: *mut c_void) -> *const i32,
    pub get_pairs_count: extern "C" fn(instance: *mut c_void) -> i32,

    // Статистика
    pub get_stats: extern "C" fn(instance: *mut c_void) -> PhysicsStats,

    // ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API):
    // добавлены В КОНЕЦ структуры — тот же принцип "новые поля только
    // дописываются", что уже применяется ко всем #[repr(C)] ABI-типам в
    // этом проекте (см. `orientation` в `PhysicsBody`), чтобы не сдвинуть
    // смещения уже существующих полей ни на движковой, ни на плагинной
    // стороне.
    /// Создаёт соединение между двумя УЖЕ существующими телами. `-1`,
    /// если `instance`/`desc` — null, либо `body_a`/`body_b` не найдены.
    pub add_constraint: extern "C" fn(instance: *mut c_void, desc: *const ConstraintDesc) -> i32,
    /// Удаляет соединение (в т.ч. уже сломанное) по его handle'у.
    pub remove_constraint: extern "C" fn(instance: *mut c_void, id: i32),
    /// Текущее состояние соединения — `is_broken=0`, `body_a=body_b=-1`,
    /// если handle не найден.
    pub get_constraint: extern "C" fn(instance: *mut c_void, id: i32) -> ConstraintInfo,
    /// Handle'ы соединений, СЕЙЧАС ЖЕ (на последнем `update`) впервые
    /// перешедших в `is_broken` — не весь список когда-либо сломанных,
    /// только НОВЫЕ события этого шага, чтобы движок мог один раз
    /// проиграть звук/заспавнить обломок на каждую поломку.
    ///
    /// ИСПРАВЛЕНО (код-ревью — гонка указатель/длина): `count_out`
    /// заполняется под тем же mutex-локом плагина, что и сам указатель —
    /// раньше указатель и количество читались двумя отдельными вызовами
    /// (`get_broken_constraints` + `get_broken_constraints_count`), и
    /// между ними `Vec` мог переаллоцироваться на другом потоке, оставляя
    /// указатель висячим при уже новом count. См. зеркальный комментарий
    /// в `alkash3d-inertial/src/lib.rs`.
    pub get_broken_constraints: extern "C" fn(instance: *mut c_void, count_out: *mut i32) -> *const i32,
    pub get_broken_constraints_count: extern "C" fn(instance: *mut c_void) -> i32,

    // ДОБАВЛЕНО (Фаза 1 реальной физики): ЗЕРКАЛЬНО тем же 4 полям, в
    // ТОМ ЖЕ порядке, что дописаны в конец `PhysicsAPI` в
    // `alkash3d-inertial/src/lib.rs` — этот файл и тот НЕ используют общий
    // Rust-тип (см. комментарий у структуры выше), совпадение layout
    // держится вручную. Если меняешь одну копию — обязательно меняй и
    // вторую, в том же порядке, иначе движок будет звать чужие указатели
    // на функции чужого плагина (тихая порча ABI, не ошибка компиляции).
    /// Копит силу (Н, мировые координаты) в аккумулятор ДО следующего
    /// `update()` — вызывать КАЖДЫЙ кадр, пока сила должна действовать.
    /// Будит тело. No-op для static/несуществующего id.
    pub apply_force: extern "C" fn(instance: *mut c_void, id: i32, force: *const f32),
    /// Мгновенно `v += impulse * inv_mass`. Будит тело. No-op для
    /// static/несуществующего id.
    pub apply_impulse: extern "C" fn(instance: *mut c_void, id: i32, impulse: *const f32),
    /// Прямая перезапись линейной/угловой скорости (телепорт скорости).
    /// Будит тело. No-op для static/несуществующего id.
    pub set_velocity: extern "C" fn(instance: *mut c_void, id: i32, linear: *const f32, angular: *const f32),
    /// Прямая перезапись позиции/ориентации (телепорт), скорость НЕ
    /// трогает. Будит тело. No-op для static/несуществующего id.
    pub set_transform: extern "C" fn(instance: *mut c_void, id: i32, position: *const f32, orientation: *const f32),
    // ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость): строго в
    // конец, тот же append-only принцип, что и выше.
    /// Добавляет статичный полупространственный коллайдер (пол и т.п.).
    /// `>= 0` (порядковый номер, не handle для удаления — CRUD не
    /// поддерживается, см. `PlaneDesc`) либо `-1` при null/вырожденной
    /// нормали.
    pub add_plane: extern "C" fn(instance: *mut c_void, desc: *const PlaneDesc) -> i32,
    // ДОБАВЛЕНО (реальная физика машины — box-коллайдер + подвеска):
    // строго в конец, ЗЕРКАЛЬНО тем же двум полям (в том же порядке), что
    // дописаны в конец `PhysicsAPI` в `alkash3d-inertial/src/lib.rs`.
    /// Копит момент силы (Н·м, мировые координаты). Будит тело. No-op
    /// для static/несуществующего id.
    pub apply_torque: extern "C" fn(instance: *mut c_void, id: i32, torque: *const f32),
    /// Прикладывает силу в точке `world_point` (не через центр масс) —
    /// рождает и линейное ускорение, и момент. Ключевая функция для
    /// честной подвески. Будит тело. No-op для static/несуществующего id.
    pub apply_force_at_point: extern "C" fn(instance: *mut c_void, id: i32, force: *const f32, world_point: *const f32),
    // ДОБАВЛЕНО (полноценная физика — запрос луча против сцены): строго в
    // конец, ЗЕРКАЛЬНО тому же полю (в том же порядке), что дописано в
    // конец `PhysicsAPI` в `alkash3d-inertial/src/lib.rs`.
    /// Ближайшее пересечение луча `origin` + `t * normalize(direction)`,
    /// `t` в `[0, max_dist]`, со ВСЕМИ живыми телами и статичными
    /// плоскостями сцены. `direction` не обязан быть нормированным заранее.
    /// `exclude_body` — handle тела, которое нужно пропустить (`-1` — не
    /// исключать никого).
    pub raycast: extern "C" fn(instance: *mut c_void, origin: *const f32, direction: *const f32, max_dist: f32, exclude_body: i32) -> RaycastHit,
}

/// Статистика физики
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicsStats {
    pub bodies_count: u32,
    pub active_bodies: u32,
    pub contacts_count: u32,
    pub pairs_count: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
    /// ДОБАВЛЕНО (джойнты/constraint API): живые (сломанные тоже
    /// считаются, пока их явно не удалили через `remove_constraint`)
    /// соединения плюс сколько из них СЕЙЧАС в состоянии `is_broken`.
    pub constraints_count: u32,
    pub broken_constraints_count: u32,
}