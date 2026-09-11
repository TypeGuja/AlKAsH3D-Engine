// inertial/src/lib.rs
//! Физический плагин "inertial" для Alkash3D — ABI-совместимая обёртка
//! (PluginAPI/PhysicsAPI) поверх РЕАЛЬНОГО Fortran-солвера.
//!
//! ИСПРАВЛЕНО (полная история): раньше этот файл содержал полностью
//! самостоятельную, наивную O(N²) Rust-реализацию физики — не
//! использующую ни Fortran-ядра (broad_phase.f90/narrow_phase.f90/
//! solver.f90/kernels_optimized.f90), ни объявленный, но неиспользуемый
//! rayon. В ней же был реальный баг в `resolve_contact`: коррекция
//! проникновения брала только normal[0] (X-компоненту нормали) и
//! применяла её ко всем трём осям позиции — то есть тела расталкивались
//! не вдоль нормали контакта, а в произвольном направлении. Плюс
//! `remove_body` использовал `Vec::remove` — сдвигая ID всех тел после
//! удалённого, из-за чего сохранённые где-то ID тихо начинали указывать
//! не на то тело. Плюс `println!` на каждый вызов `update_physics`.
//!
//! Теперь: вся физика реально считается в Fortran (broad phase — uniform
//! grid O(N), narrow phase — честный sphere-sphere тест, солвер —
//! безопасно распараллеленный через atomic-update, интеграция — по-
//! настоящему многопоточная через std::thread::scope), а этот файл —
//! только мост между ABI-структурами движка (PhysicsBody и т.п., без
//! информации о форме/инерции) и Fortran-структурами (FortranRigidBody, с
//! тензором инерции). ID тел — стабильные handle'ы, не совпадающие с
//! текущей позицией в солвере, так что удаление тел больше не портит
//! чужие ID.

mod ffi;

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::os::raw::c_char;
use std::sync::Mutex;

use ffi::{FortranContact, FortranConstraint, FortranRigidBody, FortranPhysics};

// =====================================================================
// ABI СТРУКТУРЫ
//
// Определены здесь ЛОКАЛЬНО (эта DLL не зависит от крейта движка
// alkash3d_rs как от Cargo-зависимости) — контракт между движком и
// плагином это не общий Rust-тип, а совпадение #[repr(C)] layout'а.
// Должны побайтово совпадать с abi.rs/physics_api.rs движка.
// =====================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsConfig {
    pub max_bodies: i32,
    pub world_size: f32,
    pub cell_size: f32,
    pub solver_iterations: i32,
    pub use_simd: i32,
}

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
    // ДОБАВЛЕНО (физика автомобиля — вращение кузова): кватернион
    // ориентации (x, y, z, w) — теперь реально интегрируется в
    // rigid_body.f90/kernels_optimized.f90 (см. integrate_orientation).
    // ВАЖНО: этот тип — локальный ABI-контракт (не Cargo-зависимость от
    // движка, см. комментарий в шапке файла про #[repr(C)] layout) —
    // при обновлении этого поля здесь СИММЕТРИЧНО обновить
    // PhysicsBody/аналог в alkash3d-rust/src/plugin/{abi.rs,
    // physics_api.rs} (или там, где ABI физики реально объявлен в
    // движке), иначе поля после orientation в движковой копии будут
    // читаться со сдвигом.
    pub orientation: [f32; 4],
    // ДОБАВЛЕНО (код-ревью: раньше у ВСЕХ тел без исключения был один
    // захардкоженный радиус столкновения `IMPLICIT_RADIUS = 0.5` — кузов
    // машины физически вёл бы себя как шарик 0.5м, заметно меньше
    // видимого кузова, см. подробное обоснование в шапке
    // `alkash3d-rust/src/car_sim.rs`). Теперь радиус — per-body поле,
    // читаемое `narrow_phase.f90` (`radius_sum = body_a%radius +
    // body_b%radius`, было `BODY_RADIUS + BODY_RADIUS`) и моментом
    // инерции тела (см. `to_fortran_body` ниже: `0.4 * mass * radius²`).
    pub radius: f32,
    // ДОБАВЛЕНО (реальная физика машины — box-коллайдер кузова, см.
    // подробный комментарий у `shape_type`/`half_extents` в `rigid_body_c`,
    // kernels/rigid_body.f90): 0 = сфера (используй `radius` выше), 1 =
    // коробка (половинные размеры в `half_extents`, локальные оси). ЭТИ
    // ДВА ПОЛЯ — ПОСЛЕДНИЕ в структуре (та же конвенция "дописывать
    // только в конец", что и у `orientation`/`radius` выше) — СИММЕТРИЧНО
    // обнови `alkash3d-rust/src/plugin/physics_api.rs`.
    pub shape_type: i32,
    pub half_extents: [f32; 3],
}

/// Дискриминанты `PhysicsBody::shape_type` — см. его комментарий. Именованные
/// константы вместо "магических" 0/1 на местах вызова.
pub mod shape_type {
    pub const SPHERE: i32 = 0;
    pub const BOX: i32 = 1;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
}

/// ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API): см.
/// подробное обоснование каждого типа у `JOINT_*` в
/// `src/kernels/rigid_body.f90`. Реэкспорт (а не вторая копия тех же
/// констант) — движковый ABI (`ConstraintDesc::joint_type`,
/// `PhysicsAPI::add_constraint`) и внутренний Fortran-мост
/// (`ffi::FortranConstraint::joint_type`) должны совпадать по значению
/// один-в-один, а держать одно и то же число литералом в двух местах
/// этого же крейта — только повод рассинхронизировать их при следующей
/// правке. С Fortran-стороной (`rigid_body.f90`) синхронизация
/// по-прежнему вручную (bind(c)-параметры не экспортируются как
/// C-символы) — см. комментарий у самого `ffi::joint_type`.
pub use ffi::joint_type;

/// Описание нового соединения для `PhysicsAPI::add_constraint`.
/// `body_a`/`body_b` — СТАБИЛЬНЫЕ handle'ы, возвращённые `add_body`, а не
/// индексы в солвере (индексы двигаются при `remove_body`, см. подробное
/// объяснение в `PhysicsState::update`) — тот же контракт, что уже
/// использует весь остальной API (`get_body`/`remove_body` и т.п.).
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
    /// JOINT_FIXED. Ось — в мировых координатах (см. ограничение "ось не
    /// вращается вместе с телом" у `constraint_c` в rigid_body.f90).
    pub axis_a: [f32; 3],
    pub axis_b: [f32; 3],
    /// Коэффициент "жёсткости" соединения (доля ошибки положения/
    /// ориентации, устраняемая за одну итерацию солвера) — типичное
    /// значение 1.0, меньше — мягче/эластичнее, больше 1.0 может
    /// вызывать колебания.
    pub bias: f32,
    /// Порог суммарного линейного импульса ЗА ШАГ физики, после
    /// превышения которого соединение помечается сломанным. `<= 0` —
    /// неразрушимо.
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

/// ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость): раньше у
/// движка не было НИКАКОГО способа физически представить пол/землю,
/// кроме как заставлять игровой код вручную городить плотную сетку
/// статичных сфер (см. `alkash3d-rust/src/bin/main.rs`/`main_car.rs`,
/// `FLOOR_SPHERE_SPACING`/`BARREL_FLOOR_*`). Плоскость — бесконечный
/// полупространственный статичный коллайдер: `normal` (нормированный
/// вектор нормали, "наружу", в сторону, где разрешено находиться телам)
/// + `point` (любая точка на плоскости). Задаётся ОДИН раз при настройке
/// уровня — в отличие от `ConstraintDesc`, нет `remove_plane`/CRUD в этой
/// версии API (плоскости — статичная геометрия уровня, не игровые
/// объекты, которые появляются/исчезают в рантайме).
///
/// ВАЖНО (см. код-ревью план): бесконечная плоскость корректна для ПОЛА
/// (он действительно бесконечен), но НЕ подходит для стен ограниченного
/// размера — плоскость просто рассекает весь мир по своей нормали, без
/// понятия границ. Для стен ограниченной длины в этой версии движка
/// коллайдера нет вообще (нужен отдельный, более сложный тип
/// "ограниченная плоскость/OBB") — используй прежний кинематический
/// AABB-тест на стороне игрового кода (см. `alkash3d-rust/src/car_sim.rs`).
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

/// ДОБАВЛЕНО (полноценная физика — запрос луча против сцены, см.
/// `kernels/raycast.f90`): результат `PhysicsAPI::raycast` — ближайшее
/// пересечение луча с ЛЮБЫМ живым телом (сфера/коробка, с учётом
/// ориентации) или статичной плоскостью. `hit == 0` означает "ничего не
/// найдено в пределах `max_dist`" — остальные поля в этом случае нулевые,
/// на них нельзя полагаться (тот же принцип, что у `ConstraintInfo` с
/// `is_broken=0`/`body_a=body_b=-1` при ненайденном handle'е).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RaycastHit {
    pub hit: i32,
    pub distance: f32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    /// Стабильный handle тела (см. `add_body`/`remove_body`) — ТОЛЬКО если
    /// `is_plane == 0`, иначе `-1`.
    pub body: i32,
    /// Порядковый номер плоскости (см. `PhysicsAPI::add_plane`) — ТОЛЬКО
    /// если `is_plane != 0`, иначе `-1`.
    pub plane_index: i32,
    pub is_plane: i32,
}

impl Default for RaycastHit {
    fn default() -> Self {
        Self {
            hit: 0,
            distance: 0.0,
            point: [0.0; 3],
            normal: [0.0; 3],
            body: -1,
            plane_index: -1,
            is_plane: 0,
        }
    }
}

/// Текущее состояние соединения, возвращаемое `PhysicsAPI::get_constraint`
/// — для отладочной визуализации и для опроса `is_broken` вручную (в
/// дополнение к событийному списку `get_broken_constraints`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConstraintInfo {
    pub body_a: i32,
    pub body_b: i32,
    pub joint_type: i32,
    pub is_broken: i32,
    /// Суммарный импульс ПОСЛЕДНЕГО решённого физического шага (см.
    /// обнуление в начале каждого `solve_constraints` в `solver.f90`) —
    /// удобно для HUD/отладки "насколько близко соединение к разрушению",
    /// не только для бинарного `is_broken`.
    pub linear_impulse: [f32; 3],
    pub angular_impulse: [f32; 3],
}

fn default_constraint_info() -> ConstraintInfo {
    ConstraintInfo {
        body_a: -1,
        body_b: -1,
        joint_type: joint_type::BALL,
        is_broken: 0,
        linear_impulse: [0.0; 3],
        angular_impulse: [0.0; 3],
    }
}

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
    /// ДОБАВЛЕНО (джойнты/constraint API): живые (не обязательно
    /// решаемые — сломанные тоже считаются, пока их явно не удалили
    /// через `remove_constraint`) соединения плюс сколько из них СЕЙЧАС
    /// в состоянии `is_broken`.
    pub constraints_count: u32,
    pub broken_constraints_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicsAPI {
    pub add_body: extern "C" fn(instance: *mut c_void, body: *const PhysicsBody) -> i32,
    pub remove_body: extern "C" fn(instance: *mut c_void, id: i32),
    pub get_body: extern "C" fn(instance: *mut c_void, id: i32) -> PhysicsBody,
    pub get_bodies_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub update: extern "C" fn(instance: *mut c_void, dt: f32, gravity: f32),
    pub get_contacts: extern "C" fn(instance: *mut c_void) -> *const PhysicsContact,
    pub get_contacts_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_pairs: extern "C" fn(instance: *mut c_void) -> *const i32,
    pub get_pairs_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_stats: extern "C" fn(instance: *mut c_void) -> PhysicsStats,
    // ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API):
    // добавлены В КОНЕЦ структуры — тот же принцип "новые поля только
    // дописываются", что уже применяется ко всем #[repr(C)] ABI-типам в
    // этом проекте (см. комментарии у `orientation` в `PhysicsBody`),
    // чтобы не сдвинуть смещения уже существующих полей ни на
    // движковой, ни на плагинной стороне.
    /// Создаёт соединение между двумя УЖЕ существующими телами (по их
    /// handle'ам). Возвращает handle соединения (>= 0) либо `-1`, если
    /// `instance`/`desc` — null, либо один из `body_a`/`body_b` не
    /// существует.
    pub add_constraint: extern "C" fn(instance: *mut c_void, desc: *const ConstraintDesc) -> i32,
    /// Удаляет соединение (в т.ч. уже сломанное) по его handle'у. Не
    /// затрагивает тела — только сам констрейнт.
    pub remove_constraint: extern "C" fn(instance: *mut c_void, id: i32),
    /// Текущее состояние соединения — `is_broken=0`,
    /// `body_a=body_b=-1`, если handle не найден.
    pub get_constraint: extern "C" fn(instance: *mut c_void, id: i32) -> ConstraintInfo,
    /// Handle'ы соединений, СЕЙЧАС ЖЕ (на последнем вызове `update`)
    /// впервые перешедших в состояние `is_broken` — не весь список
    /// когда-либо сломанных, только НОВЫЕ события этого шага (см.
    /// `PhysicsState::update`), чтобы вызывающий код мог один раз
    /// проиграть звук/заспавнить обломок на каждую поломку, а не на
    /// каждый кадр, пока constraint остаётся сломанным.
    ///
    /// ИСПРАВЛЕНО (код-ревью — гонка указатель/длина): раньше указатель и
    /// количество читались ДВУМЯ отдельными вызовами
    /// (`get_broken_constraints` + `get_broken_constraints_count`), то
    /// есть двумя независимыми lock/unlock мьютекса. Если бы
    /// `PhysicsState::update` (который каждый кадр очищает и заново
    /// заполняет `broken_constraints_abi`) хоть раз выполнился на другом
    /// потоке МЕЖДУ этими двумя вызовами, `Vec` мог переаллоцироваться —
    /// указатель стал бы висячим, а count отражал бы уже новое состояние,
    /// и `slice::from_raw_parts` на стороне движка читал бы
    /// освобождённую/чужую память. Теперь `count_out` заполняется ПОД ТЕМ
    /// ЖЕ mutex-локом, что и получение указателя — один атомарный снимок
    /// вместо двух рассинхронизируемых чтений. `get_broken_constraints_count`
    /// оставлен в ABI как есть (в т.ч. для случая, если `count_out` —
    /// null), но новый комбинированный вызов — единственный, которым
    /// теперь пользуется `PhysicsPlugin::get_broken_constraints`.
    pub get_broken_constraints: extern "C" fn(instance: *mut c_void, count_out: *mut i32) -> *const i32,
    pub get_broken_constraints_count: extern "C" fn(instance: *mut c_void) -> i32,
    // ДОБАВЛЕНО (Фаза 1 реальной физики — см. план "фундамент реальной
    // физики"): до этого движок мог только ЧИТАТЬ состояние тела
    // (get_body) или удалить его — не было НИКАКОГО способа повлиять на
    // уже созданное тело, что делало невозможной любую реальную физику
    // вождения (газ/руль/тормоз через силы). Снова строго В КОНЕЦ
    // структуры, тот же append-only принцип, что и у constraint-полей
    // выше.
    /// Копит силу (Н, мировые координаты) в аккумулятор ДО следующего
    /// `update()` — вызывать КАЖДЫЙ кадр, пока сила должна действовать
    /// (аккумулятор обнуляется сразу после интеграции). `force` —
    /// указатель на 3 float. Будит тело. No-op для static/несуществующего
    /// id.
    pub apply_force: extern "C" fn(instance: *mut c_void, id: i32, force: *const f32),
    /// Мгновенно `v += impulse * inv_mass`, в отличие от `apply_force` не
    /// ждёт следующего `update()`. Будит тело. No-op для
    /// static/несуществующего id.
    pub apply_impulse: extern "C" fn(instance: *mut c_void, id: i32, impulse: *const f32),
    /// Прямая перезапись линейной/угловой скорости (телепорт скорости).
    /// Будит тело. No-op для static/несуществующего id.
    pub set_velocity: extern "C" fn(instance: *mut c_void, id: i32, linear: *const f32, angular: *const f32),
    /// Прямая перезапись позиции/ориентации (телепорт). Скорость НЕ
    /// трогает — зови `set_velocity` отдельно, если нужно ещё и
    /// погасить/задать скорость. Кватернион нормализуется защитно на
    /// стороне плагина. Будит тело. No-op для static/несуществующего id.
    pub set_transform: extern "C" fn(instance: *mut c_void, id: i32, position: *const f32, orientation: *const f32),
    // ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость, см.
    // `PlaneDesc`): строго В КОНЕЦ структуры, тот же append-only принцип.
    /// Добавляет статичный полупространственный коллайдер (пол и т.п.).
    /// Возвращает `>= 0` (порядковый номер, не handle для удаления — CRUD
    /// не поддерживается, см. `PlaneDesc`) либо `-1`, если `instance`/
    /// `desc` — null, либо `normal` — вырожденный (нулевой) вектор.
    pub add_plane: extern "C" fn(instance: *mut c_void, desc: *const PlaneDesc) -> i32,
    // ДОБАВЛЕНО (реальная физика машины — box-коллайдер + подвеска):
    // строго В КОНЕЦ структуры, тот же append-only принцип, что и у всех
    // полей выше.
    /// Копит момент силы (Н·м, мировые координаты) — та же семантика,
    /// что у `apply_force`, только для угловой составляющей. Будит тело.
    /// No-op для static/несуществующего id.
    pub apply_torque: extern "C" fn(instance: *mut c_void, id: i32, torque: *const f32),
    /// Прикладывает силу в точке `world_point`, а не через центр масс —
    /// рождает и линейное ускорение, и момент (см. `apply_torque` выше).
    /// Ключевая функция для честной подвески (сила пружины/демпфера на
    /// колесе применяется именно в точке колеса, не в центре кузова).
    /// Будит тело. No-op для static/несуществующего id.
    pub apply_force_at_point: extern "C" fn(instance: *mut c_void, id: i32, force: *const f32, world_point: *const f32),
    // ДОБАВЛЕНО (полноценная физика — запрос луча против сцены): строго в
    // конец, тот же append-only принцип, что и у всех предыдущих полей.
    // СИММЕТРИЧНО обнови `alkash3d-rust/src/plugin/physics_api.rs` (тем же
    // полем, в том же порядке) — см. комментарий в шапке того файла.
    /// Ближайшее пересечение луча `origin` + `t * normalize(direction)`,
    /// `t` в `[0, max_dist]`, со ВСЕМИ живыми телами (сфера/коробка) и
    /// статичными плоскостями сцены. `direction` НЕ обязан быть
    /// нормированным заранее — нормируется на стороне плагина.
    /// `exclude_body` — handle тела, которое нужно пропустить (`-1` — не
    /// исключать никого; см. подробное обоснование "почему это нужно" у
    /// `PhysicsState::raycast` в этом файле). См. `RaycastHit` за
    /// подробностями результата.
    pub raycast: extern "C" fn(instance: *mut c_void, origin: *const f32, direction: *const f32, max_dist: f32, exclude_body: i32) -> RaycastHit,
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum PluginType {
    Physics = 0,
    LightCulling = 1,
    Audio = 2,
    Scripting = 3,
}

#[repr(C)]
pub struct PluginAPI {
    pub version: u32,
    pub plugin_type: PluginType,
    pub name: *const c_char,
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

pub const PLUGIN_API_VERSION: u32 = 1;
static PLUGIN_NAME: &[u8] = b"inertial\0";

// =====================================================================
// Мостик PhysicsBody(ABI, без формы) <-> FortranRigidBody(с тензором инерции)
// =====================================================================

// ИСПРАВЛЕНО (код-ревью): раньше был ОДИН глобальный радиус для всех тел
// без исключения — `PhysicsBody::radius` (новое поле, см. его комментарий)
// заменил эту константу везде, где радиус реально на что-то влияет
// (момент инерции ниже, `narrow_phase.f90`). Оставлена как fallback ТОЛЬКО
// для `default_abi_body()` (синтетическое "тело не найдено" — там любое
// значение одинаково безобидно, реального столкновения не будет).
const IMPLICIT_RADIUS: f32 = 0.5;

// ИЗМЕНЕНО (полноценная физика — box-vs-sphere/box-vs-plane перенесены в
// Fortran, см. `narrow_phase_box_sphere`/`box_effective_radius` в
// narrow_phase.f90): раньше здесь жили Rust-реализации этих двух функций
// узкой фазы (плюс кватернионные хелперы `cross3`/`quat_conjugate`/
// `quat_rotate_vector`, нужные только им) — единственное место в крейте,
// где узкая фаза считалась НЕ в Fortran. Убраны целиком, не оставлены как
// мёртвый код — вызывающая сторона (`update`/`resolve_plane_contacts`
// ниже) теперь честно зовёт Fortran-версии через `ffi::`.

/// Диагональный тензор инерции (в ЛОКАЛЬНЫХ осях тела) для сферы или
/// коробки. ВАЖНО (упрощение, задокументированное честно): этот тензор
/// считается ОДИН РАЗ при создании тела (здесь) и никогда не
/// пересчитывается в мировые оси по мере вращения `orientation` —
/// `integrate_bodies`/`batch_integrate` применяют его "как есть" каждый
/// кадр. Для сферы это точно (изотропна — поворот не меняет тензор). Для
/// коробки это ТОЛЬКО приближение, верное в состоянии покоя/малых углов
/// (машина, стоящая или слегка накренившаяся на подвеске) — при большом
/// повороте (переворот на бок/крышу) реальный мировой тензор инерции
/// должен быть `R * I_local * R^T`, чего эта версия солвера не делает.
/// Полный честный учёт потребовал бы пересчёта `inv_inertia` каждый кадр
/// в Fortran-цикле интеграции — отдельная, более крупная доработка,
/// сознательно отложенная (см. `PhysicsBody::shape_type` — весь этот
/// box-коллайдер уже сам по себе большая доработка за один проход).
fn compute_local_inertia(b: &PhysicsBody) -> ([[f32; 3]; 3], [[f32; 3]; 3]) {
    if b.mass <= 0.0 {
        return ([[0.0; 3]; 3], [[0.0; 3]; 3]);
    }
    let diag = if b.shape_type == shape_type::BOX {
        // Коробка со сторонами (2*hx, 2*hy, 2*hz): Ixx=m/3*(hy²+hz²) и т.д.
        // (стандартная формула m/12*(полная_сторона²+полная_сторона²) с
        // полной стороной = 2*half_extent).
        let (hx, hy, hz) = (b.half_extents[0], b.half_extents[1], b.half_extents[2]);
        [
            (b.mass / 3.0) * (hy * hy + hz * hz),
            (b.mass / 3.0) * (hx * hx + hz * hz),
            (b.mass / 3.0) * (hx * hx + hy * hy),
        ]
    } else {
        // Сплошная сфера: I = 2/5 * m * r² по всем трём осям (изотропна).
        let i = 0.4 * b.mass * b.radius * b.radius;
        [i, i, i]
    };
    let inertia = [
        [diag[0], 0.0, 0.0],
        [0.0, diag[1], 0.0],
        [0.0, 0.0, diag[2]],
    ];
    let inv_inertia = [
        [if diag[0] > 0.0 { 1.0 / diag[0] } else { 0.0 }, 0.0, 0.0],
        [0.0, if diag[1] > 0.0 { 1.0 / diag[1] } else { 0.0 }, 0.0],
        [0.0, 0.0, if diag[2] > 0.0 { 1.0 / diag[2] } else { 0.0 }],
    ];
    (inertia, inv_inertia)
}

fn to_fortran_body(b: &PhysicsBody) -> FortranRigidBody {
    let (inertia, inv_inertia) = compute_local_inertia(b);

    FortranRigidBody {
        position: b.position,
        velocity: b.velocity,
        acceleration: b.acceleration,
        angular_velocity: b.angular_velocity,
        angular_acceleration: b.angular_acceleration,
        inertia,
        inv_inertia,
        mass: b.mass,
        inv_mass: b.inv_mass,
        restitution: b.restitution,
        friction: b.friction,
        linear_damping: b.linear_damping,
        angular_damping: b.angular_damping,
        is_static: b.is_static,
        is_asleep: b.is_asleep,
        orientation: b.orientation,
        radius: b.radius,
        shape_type: b.shape_type,
        half_extents: b.half_extents,
    }
}

fn to_abi_body(f: &FortranRigidBody) -> PhysicsBody {
    PhysicsBody {
        position: f.position,
        velocity: f.velocity,
        acceleration: f.acceleration,
        angular_velocity: f.angular_velocity,
        angular_acceleration: f.angular_acceleration,
        mass: f.mass,
        inv_mass: f.inv_mass,
        restitution: f.restitution,
        friction: f.friction,
        linear_damping: f.linear_damping,
        angular_damping: f.angular_damping,
        is_static: f.is_static,
        is_asleep: f.is_asleep,
        orientation: f.orientation,
        radius: f.radius,
        shape_type: f.shape_type,
        half_extents: f.half_extents,
    }
}

fn default_abi_body() -> PhysicsBody {
    PhysicsBody {
        position: [0.0; 3],
        velocity: [0.0; 3],
        acceleration: [0.0; 3],
        angular_velocity: [0.0; 3],
        angular_acceleration: [0.0; 3],
        mass: 0.0,
        inv_mass: 0.0,
        restitution: 0.0,
        friction: 0.0,
        linear_damping: 0.0,
        angular_damping: 0.0,
        is_static: 1,
        is_asleep: 1,
        // Единичный кватернион (0,0,0,1) — "без поворота".
        orientation: [0.0, 0.0, 0.0, 1.0],
        radius: IMPLICIT_RADIUS,
        shape_type: shape_type::SPHERE,
        half_extents: [0.0; 3],
    }
}

// =====================================================================
// PhysicsState — реальная логика поверх Fortran-солвера
// =====================================================================

/// ДОБАВЛЕНО (джойнты/constraint API): одна запись реестра соединений
/// движка. Хранит handle'ы ТЕЛ (не индексы — см. подробное объяснение в
/// `PhysicsState::update`) отдельно от `data` (само описание +
/// последний решённый результат), потому что `data.body_a`/`data.body_b`
/// каждый кадр перезаписываются ТЕКУЩИМИ индексами солвера перед
/// вызовом `solve_constraints` — если бы handle хранился только внутри
/// `data`, он бы каждый кадр затирался этой перезаписью.
struct ConstraintRecord {
    body_a_handle: i32,
    body_b_handle: i32,
    data: FortranConstraint,
}

pub struct PhysicsState {
    config: PhysicsConfig,
    solver: FortranPhysics,
    next_handle: i32,
    handle_to_index: HashMap<i32, usize>,
    index_to_handle: Vec<i32>,
    contacts_abi: Vec<PhysicsContact>,
    pairs_abi: Vec<i32>,
    stats: PhysicsStats,
    /// ДОБАВЛЕНО (джойнты/constraint API): та же схема handle<->index
    /// индирекции, что уже используется для тел выше (`next_handle`/
    /// `handle_to_index`/`index_to_handle`) — нужна по той же причине:
    /// `remove_constraint` использует `swap_remove` (дёшево, не требует
    /// сдвигать хвост), поэтому "текущий индекс в `constraints`" не
    /// может служить стабильным ID, который движок держит у себя между
    /// кадрами.
    constraints: Vec<ConstraintRecord>,
    next_constraint_handle: i32,
    constraint_handle_to_index: HashMap<i32, usize>,
    constraint_index_to_handle: Vec<i32>,
    /// Handle'ы соединений, впервые сломавшихся НА ЭТОМ вызове `update` —
    /// см. `PhysicsAPI::get_broken_constraints`. Перезаполняется с нуля
    /// в начале каждого `update`, не накапливается между кадрами.
    broken_constraints_abi: Vec<i32>,
    /// ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость): статичная
    /// геометрия уровня, задаётся один раз при инициализации (см.
    /// `PlaneDesc`) — никакого handle/индекса-для-удаления, поэтому
    /// просто растущий `Vec`, без схемы handle<->index, которая нужна
    /// телам/констрейнтам ТОЛЬКО из-за `remove_*`.
    planes: Vec<PlaneRecord>,
}

/// Внутреннее (не-ABI) представление одной плоскости — `normal` хранится
/// уже нормированным (валидируется/нормализуется в `add_plane`), чтобы
/// `resolve_plane_contacts` не пересчитывал длину на каждое тело каждый
/// кадр.
#[derive(Debug, Clone, Copy)]
struct PlaneRecord {
    normal: [f32; 3],
    point: [f32; 3],
    friction: f32,
    restitution: f32,
}

impl PhysicsState {
    fn new(config: PhysicsConfig) -> Self {
        let max_bodies = config.max_bodies.max(1) as usize;
        let world_size = if config.world_size > 0.0 { config.world_size } else { 100.0 };
        let cell_size = if config.cell_size > 0.0 { config.cell_size } else { 4.0 };

        Self {
            config,
            solver: FortranPhysics::new(max_bodies, world_size, cell_size),
            next_handle: 0,
            handle_to_index: HashMap::with_capacity(max_bodies),
            index_to_handle: Vec::with_capacity(max_bodies),
            contacts_abi: Vec::new(),
            pairs_abi: Vec::new(),
            stats: PhysicsStats::default(),
            constraints: Vec::new(),
            next_constraint_handle: 0,
            constraint_handle_to_index: HashMap::new(),
            constraint_index_to_handle: Vec::new(),
            broken_constraints_abi: Vec::new(),
            planes: Vec::new(),
        }
    }

    /// См. `PlaneDesc`/`PhysicsAPI::add_plane`. `-1` при вырожденной
    /// (нулевой длины) нормали — тот же принцип "отрицательный id —
    /// отказ", что и у `add_constraint`.
    fn add_plane(&mut self, desc: &PlaneDesc) -> i32 {
        let len_sq = desc.normal[0] * desc.normal[0]
            + desc.normal[1] * desc.normal[1]
            + desc.normal[2] * desc.normal[2];
        if len_sq < 1.0e-8 {
            return -1;
        }
        let inv_len = 1.0 / len_sq.sqrt();
        self.planes.push(PlaneRecord {
            normal: [desc.normal[0] * inv_len, desc.normal[1] * inv_len, desc.normal[2] * inv_len],
            point: desc.point,
            friction: desc.friction,
            restitution: desc.restitution,
        });
        (self.planes.len() - 1) as i32
    }

    /// Разрешает столкновение каждого нестатичного тела с каждой
    /// плоскостью — см. подробное обоснование "почему в Rust, а не в
    /// Fortran" у `PlaneDesc`/в плане код-ревью: плоскость всегда
    /// статична (бесконечная масса), поэтому это односторонняя коррекция
    /// ОДНОГО тела, а не парный impulse-солвер, которым Fortran честно
    /// разрешает sphere-sphere контакты. Тот же порядок вызова, что и
    /// сфера-сфера контакты — ПОСЛЕ constraint'ов, ДО `batch_integrate`
    /// (сначала скорректировать скорость, потом проинтегрировать позицию
    /// этой скорректированной скоростью).
    fn resolve_plane_contacts(&mut self) {
        if self.planes.is_empty() {
            return;
        }
        for body in self.solver.bodies.iter_mut() {
            if body.is_static != 0 {
                continue;
            }
            for plane in &self.planes {
                let rel = [
                    body.position[0] - plane.point[0],
                    body.position[1] - plane.point[1],
                    body.position[2] - plane.point[2],
                ];
                let dist = rel[0] * plane.normal[0] + rel[1] * plane.normal[1] + rel[2] * plane.normal[2];
                // ДОБАВЛЕНО (box-коллайдер кузова — например, машина легла
                // на бок/крышу): для сферы "насколько далеко тело
                // выступает в сторону плоскости" — просто `radius`,
                // одинаково по всем направлениям. Для коробки это ЗАВИСИТ
                // от того, как она повёрнута относительно нормали —
                // честная формула проекции полуразмеров OBB на
                // произвольное направление: сумма |half_extent_i * (мировая
                // ось_i · normal)| по трём локальным осям (стандартный
                // "support function" OBB, точный, а не приближение через
                // перебор 8 углов). При normal=(0,1,0) (плоский пол) и
                // машине строго вертикально это даёт ровно half_extents.y,
                // как и ожидается.
                // ИЗМЕНЕНО (полноценная физика — box-vs-plane перенесён в
                // Fortran, см. `box_effective_radius` в narrow_phase.f90):
                // та же формула ("сумма проекций полуразмеров OBB на
                // normal"), что раньше считалась здесь через
                // `quat_rotate_vector` на Rust-стороне.
                let effective_radius = if body.shape_type == shape_type::BOX {
                    unsafe {
                        ffi::box_effective_radius(body.orientation.as_ptr(), body.half_extents.as_ptr(), plane.normal.as_ptr())
                    }
                } else {
                    body.radius
                };
                let penetration = effective_radius - dist;
                if penetration <= 0.0 {
                    continue;
                }
                // Вытолкнуть вдоль нормали на глубину проникновения.
                body.position[0] += plane.normal[0] * penetration;
                body.position[1] += plane.normal[1] * penetration;
                body.position[2] += plane.normal[2] * penetration;

                let v_n = body.velocity[0] * plane.normal[0]
                    + body.velocity[1] * plane.normal[1]
                    + body.velocity[2] * plane.normal[2];
                if v_n < 0.0 {
                    // Гасим нормальную составляющую скорости по
                    // restitution (0 = прилипает, 1 = честный упругий
                    // отскок) — та же формула, что для sphere-sphere.
                    let combined_restitution = (body.restitution + plane.restitution) * 0.5;
                    let new_v_n = -v_n * combined_restitution;
                    let delta_v_n = new_v_n - v_n;
                    body.velocity[0] += plane.normal[0] * delta_v_n;
                    body.velocity[1] += plane.normal[1] * delta_v_n;
                    body.velocity[2] += plane.normal[2] * delta_v_n;

                    // Приближённое (не честный Кулон — см. общую
                    // оговорку о трении в комментариях этого крейта)
                    // затухание касательной составляющей — иначе тело,
                    // упавшее на пол с горизонтальной скоростью, катится
                    // по инерции почти бесконечно (тот же эффект уже
                    // описан у бочек в main_car.rs).
                    let combined_friction = ((body.friction + plane.friction) * 0.5).clamp(0.0, 1.0);
                    let v_tn = body.velocity[0] * plane.normal[0]
                        + body.velocity[1] * plane.normal[1]
                        + body.velocity[2] * plane.normal[2];
                    for i in 0..3 {
                        let v_tangent = body.velocity[i] - plane.normal[i] * v_tn;
                        body.velocity[i] -= v_tangent * combined_friction;
                    }
                }
            }
        }
    }

    /// ДОБАВЛЕНО (полноценная физика — запрос луча против сцены, см.
    /// `kernels/raycast.f90`): честный raycast против ВСЕХ живых тел
    /// (сфер и коробок, с учётом текущей ориентации) и статичных
    /// плоскостей. Нормирует `direction` здесь (не в Fortran), чтобы
    /// `hit.distance` была честной метрической длиной для любого
    /// ненулевого вектора направления, переданного вызывающей стороной —
    /// игровой код не обязан помнить о нормализации сам.
    ///
    /// `exclude_body` — стабильный handle тела, которое нужно пропустить
    /// (например собственный кузов машины при raycast'е подвески вниз —
    /// иначе точка крепления колеса, лежащая на границе/внутри box-
    /// коллайдера кузова, почти всегда даёт "попадание в самого себя" на
    /// нулевой/крошечной дистанции вместо честного попадания в землю) —
    /// `None` — не исключать никого.
    fn raycast(&self, origin: [f32; 3], direction: [f32; 3], max_dist: f32, exclude_body: Option<i32>) -> RaycastHit {
        let len_sq: f32 = direction.iter().map(|c| c * c).sum();
        let nothing_to_hit = self.solver.bodies.is_empty() && self.planes.is_empty();
        if len_sq < 1.0e-12 || max_dist <= 0.0 || nothing_to_hit {
            return RaycastHit::default();
        }
        let inv_len = len_sq.sqrt().recip();
        let dir = [direction[0] * inv_len, direction[1] * inv_len, direction[2] * inv_len];
        // Handle -> индекс в солвере (та же индирекция, что везде — индексы
        // двигаются при `remove_body`, handle'ы стабильны), `-1` если
        // handle не найден (тело уже удалено) ИЛИ исключение не запрошено.
        let exclude_index = exclude_body
            .and_then(|h| self.handle_to_index.get(&h))
            .map(|&idx| idx as i32)
            .unwrap_or(-1);

        // Fortran-сторона ожидает плоские массивы 3×n_planes (см. `plane_c`-
        // подобные параметры `raycast_query` в raycast.f90) — собираем их
        // здесь по требованию, а не храним `self.planes` в этом формате
        // постоянно (текущая форма — `PlaneRecord` — удобнее для
        // `resolve_plane_contacts` выше, которому плоские массивы не нужны).
        let mut plane_normals = Vec::with_capacity(self.planes.len() * 3);
        let mut plane_points = Vec::with_capacity(self.planes.len() * 3);
        for p in &self.planes {
            plane_normals.extend_from_slice(&p.normal);
            plane_points.extend_from_slice(&p.point);
        }

        let mut hit_found: i32 = 0;
        let mut hit_distance: f32 = 0.0;
        let mut hit_point = [0.0f32; 3];
        let mut hit_normal = [0.0f32; 3];
        let mut hit_index: i32 = -1;
        let mut hit_is_plane: i32 = 0;

        unsafe {
            ffi::raycast_query(
                self.solver.bodies.as_ptr(),
                self.solver.bodies.len() as i32,
                plane_normals.as_ptr(),
                plane_points.as_ptr(),
                self.planes.len() as i32,
                origin.as_ptr(),
                dir.as_ptr(),
                max_dist,
                exclude_index,
                &mut hit_found,
                &mut hit_distance,
                hit_point.as_mut_ptr(),
                hit_normal.as_mut_ptr(),
                &mut hit_index,
                &mut hit_is_plane,
            );
        }

        if hit_found == 0 {
            return RaycastHit::default();
        }

        RaycastHit {
            hit: 1,
            distance: hit_distance,
            point: hit_point,
            normal: hit_normal,
            // `hit_index` — 0-based ИНДЕКС В СОЛВЕРЕ (не handle) при
            // попадании в тело — переводим в стабильный handle тем же
            // способом, что `get_body`/`remove_body` (`index_to_handle`),
            // потому что вызывающая сторона (движок) знает тела только по
            // handle'ам, а индексы двигаются при `remove_body` (swap_remove).
            body: if hit_is_plane == 0 {
                self.index_to_handle.get(hit_index as usize).copied().unwrap_or(-1)
            } else {
                -1
            },
            plane_index: if hit_is_plane != 0 { hit_index } else { -1 },
            is_plane: hit_is_plane,
        }
    }

    fn add_body(&mut self, body: &PhysicsBody) -> i32 {
        // ИСПРАВЛЕНО (краш видеодрайвера/зависание, воспроизведено
        // пользователем — 2025 тел вместо запланированных ~150 при
        // `max_bodies: 256`): раньше эта функция НЕ проверяла лимит
        // вообще — `self.solver.bodies` (`Vec<FortranRigidBody>`) просто
        // рос без ограничений, при том что `FortranPhysics::new`
        // выделяет фиксированные Fortran-буферы РОВНО под
        // `config.max_bodies` (см. `cell_pairs: vec![0; max_bodies * 8]`
        // в ffi/mod.rs). `find_pairs_grid` (единственный broad-phase
        // путь, реально используемый из `update()` ниже) сам по себе
        // защищён — динамически ресайзит `cell_pairs`, если пар
        // оказывается больше вместимости — так что прямого
        // переполнения буфера через ЭТОТ путь не было. Но при
        // многократном превышении `max_bodies` (2025 вместо 256, почти
        // в 8 раз) broad-phase на плотной сетке тел даёт квадратично
        // больше пар/контактов КАЖДЫЙ кадр — кадр физики переставал
        // укладываться в разумное время, и Windows TDR (Timeout
        // Detection and Recovery) считал GPU зависшим и перезапускал
        // драйвер. Плюс `max_bodies` — явный, документированный
        // пользователем движка контракт (см. `PhysicsConfig`) — молчаливо
        // игнорировать его всё равно неверно, даже если бы конкретно
        // этот буфер не переполнялся.
        //
        // Теперь — честный отказ при превышении лимита: `-1` (тот же
        // код ошибки, что уже используют другие ветки `api_add_body`
        // ниже, например null instance/body), а не тихий безлимитный
        // рост.
        if self.solver.bodies.len() >= self.config.max_bodies.max(1) as usize {
            return -1;
        }

        let handle = self.next_handle;
        self.next_handle += 1;

        let idx = self.solver.bodies.len();
        self.solver.add_body(to_fortran_body(body));
        self.index_to_handle.push(handle);
        self.handle_to_index.insert(handle, idx);
        handle
    }

    fn remove_body(&mut self, handle: i32) {
        let Some(idx) = self.handle_to_index.remove(&handle) else {
            return;
        };
        if self.solver.bodies.is_empty() {
            return;
        }
        let last = self.solver.bodies.len() - 1;

        self.solver.bodies.swap_remove(idx);
        self.solver.sleep_timers.swap_remove(idx);
        self.solver.force_accum.swap_remove(idx);
        self.solver.torque_accum.swap_remove(idx);

        if idx != last {
            let moved_handle = self.index_to_handle[last];
            self.index_to_handle[idx] = moved_handle;
            self.handle_to_index.insert(moved_handle, idx);
        }
        self.index_to_handle.pop();
    }

    fn get_body(&self, handle: i32) -> Option<PhysicsBody> {
        let &idx = self.handle_to_index.get(&handle)?;
        Some(to_abi_body(&self.solver.bodies[idx]))
    }

    /// См. `PhysicsAPI::apply_force`. Тихо игнорирует несуществующий
    /// handle (тот же контракт, что `remove_constraint`) — `apply_force`
    /// на уже удалённое/никогда не существовавшее тело физически ничего
    /// не значит, а не ошибка, которую стоит как-то сигнализировать через
    /// этот ABI (в нём и так нет возврата ошибки у этой группы функций).
    fn apply_force(&mut self, handle: i32, force: [f32; 3]) {
        if let Some(&idx) = self.handle_to_index.get(&handle) {
            self.solver.apply_force(idx, force);
        }
    }

    fn apply_impulse(&mut self, handle: i32, impulse: [f32; 3]) {
        if let Some(&idx) = self.handle_to_index.get(&handle) {
            self.solver.apply_impulse(idx, impulse);
        }
    }

    fn apply_torque(&mut self, handle: i32, torque: [f32; 3]) {
        if let Some(&idx) = self.handle_to_index.get(&handle) {
            self.solver.apply_torque(idx, torque);
        }
    }

    fn apply_force_at_point(&mut self, handle: i32, force: [f32; 3], world_point: [f32; 3]) {
        if let Some(&idx) = self.handle_to_index.get(&handle) {
            self.solver.apply_force_at_point(idx, force, world_point);
        }
    }

    fn set_velocity(&mut self, handle: i32, linear: [f32; 3], angular: [f32; 3]) {
        if let Some(&idx) = self.handle_to_index.get(&handle) {
            self.solver.set_velocity(idx, linear, angular);
        }
    }

    fn set_transform(&mut self, handle: i32, position: [f32; 3], orientation: [f32; 4]) {
        if let Some(&idx) = self.handle_to_index.get(&handle) {
            self.solver.set_transform(idx, position, orientation);
        }
    }

    fn bodies_count(&self) -> i32 {
        self.solver.bodies.len() as i32
    }

    /// ДОБАВЛЕНО (джойнты/constraint API): см. `ConstraintRecord` за
    /// объяснением, почему handle'ы тел хранятся отдельно от `data`.
    /// Отказывает (`-1`), если любой из `body_a`/`body_b` не существует —
    /// соединение "в никуда" не может быть создано даже временно (в
    /// отличие от тел, которые могут исчезнуть уже ПОСЛЕ создания
    /// соединения — та ситуация обрабатывается в `update`, см. там).
    ///
    /// ИСПРАВЛЕНО (код-ревью): также отказывает при `body_a == body_b`.
    /// Раньше это не проверялось, а `solver.f90` (solve_point/
    /// solve_hinge_angular/solve_fixed_angular) принимает body_a и body_b
    /// как ДВА ОТДЕЛЬНЫХ `INTENT(INOUT)` фортрановских аргумента без
    /// защиты от алиасинга — если это один и тот же элемент массива тел,
    /// правила Фортрана для алиасированных INTENT(INOUT)-аргументов
    /// нарушаются, как только один из них записывается, и результат
    /// становится неопределённым (зависит от оптимизаций компилятора)
    /// вместо чистого отказа на границе API.
    fn add_constraint(&mut self, desc: &ConstraintDesc) -> i32 {
        if desc.body_a == desc.body_b
            || !self.handle_to_index.contains_key(&desc.body_a)
            || !self.handle_to_index.contains_key(&desc.body_b)
        {
            return -1;
        }

        let handle = self.next_constraint_handle;
        self.next_constraint_handle += 1;

        let idx = self.constraints.len();
        self.constraints.push(ConstraintRecord {
            body_a_handle: desc.body_a,
            body_b_handle: desc.body_b,
            data: FortranConstraint {
                // Перезаписываются реальными индексами перед каждым
                // `solve_constraints` в `update()` — значения здесь
                // никогда не читаются как индексы.
                body_a: 0,
                body_b: 0,
                joint_type: desc.joint_type,
                anchor_a: desc.anchor_a,
                anchor_b: desc.anchor_b,
                axis_a: desc.axis_a,
                axis_b: desc.axis_b,
                bias: desc.bias,
                break_impulse_linear: desc.break_impulse_linear,
                break_impulse_angular: desc.break_impulse_angular,
                linear_impulse: [0.0; 3],
                angular_impulse: [0.0; 3],
                is_broken: 0,
            },
        });
        self.constraint_index_to_handle.push(handle);
        self.constraint_handle_to_index.insert(handle, idx);
        handle
    }

    /// Тот же swap_remove-паттерн, что и `remove_body` выше — см. его
    /// комментарий про то, почему это не портит чужие handle'ы.
    fn remove_constraint(&mut self, handle: i32) {
        let Some(idx) = self.constraint_handle_to_index.remove(&handle) else {
            return;
        };
        if self.constraints.is_empty() {
            return;
        }
        let last = self.constraints.len() - 1;

        self.constraints.swap_remove(idx);

        if idx != last {
            let moved_handle = self.constraint_index_to_handle[last];
            self.constraint_index_to_handle[idx] = moved_handle;
            self.constraint_handle_to_index.insert(moved_handle, idx);
        }
        self.constraint_index_to_handle.pop();
    }

    fn get_constraint(&self, handle: i32) -> Option<ConstraintInfo> {
        let &idx = self.constraint_handle_to_index.get(&handle)?;
        let rec = &self.constraints[idx];
        Some(ConstraintInfo {
            body_a: rec.body_a_handle,
            body_b: rec.body_b_handle,
            joint_type: rec.data.joint_type,
            is_broken: rec.data.is_broken,
            linear_impulse: rec.data.linear_impulse,
            angular_impulse: rec.data.angular_impulse,
        })
    }

    /// Переводит handle'ы тел каждого ЕЩЁ НЕ сломанного соединения в
    /// текущие индексы солвера и решает их все разом через
    /// `FortranPhysics::solve_constraints`. Вызывается из `update()`
    /// СРАЗУ после решения контактов — см. вызов ниже.
    ///
    /// Соединение, чьё тело успело исчезнуть (`remove_body` был вызван
    /// без предварительного `remove_constraint` — например, движок
    /// выгрузил чанк, не думая о соединениях внутри него), физически не
    /// может продолжать существовать — удаляется здесь же, автоматически.
    /// Это НЕ добавляется в `broken_constraints_abi`: пропавшее тело —
    /// рутинная уборка после `remove_body`, а не событие "деталь
    /// оторвалась под нагрузкой", на которое игровой код должен был бы
    /// реагировать звуком/осколками.
    fn solve_and_update_constraints(&mut self) {
        self.broken_constraints_abi.clear();
        if self.constraints.is_empty() {
            return;
        }

        self.solver.clear_constraints();
        // Индекс в этом Vec == индекс соответствующей записи в буфере
        // `self.solver.constraints`, который сейчас будет решаться —
        // нужен, чтобы после решения скопировать результат обратно в
        // ПРАВИЛЬНУЮ запись `self.constraints` (порядок двух Vec иначе
        // мог бы разойтись, если бы часть констрейнтов пропускалась).
        let mut solved_record_indices: Vec<usize> = Vec::with_capacity(self.constraints.len());
        let mut stale_handles: Vec<i32> = Vec::new();

        for (record_idx, record) in self.constraints.iter().enumerate() {
            if record.data.is_broken != 0 {
                continue;
            }
            let ia = self.handle_to_index.get(&record.body_a_handle).copied();
            let ib = self.handle_to_index.get(&record.body_b_handle).copied();
            match (ia, ib) {
                (Some(ia), Some(ib)) => {
                    let mut c = record.data;
                    c.body_a = ia as i32;
                    c.body_b = ib as i32;
                    self.solver.push_constraint(c);
                    solved_record_indices.push(record_idx);
                }
                _ => stale_handles.push(self.constraint_index_to_handle[record_idx]),
            }
        }

        if !solved_record_indices.is_empty() {
            self.solver.solve_constraints(self.config.solver_iterations.max(1));
            for (buffer_idx, &record_idx) in solved_record_indices.iter().enumerate() {
                let solved = self.solver.constraints[buffer_idx];
                let was_broken = self.constraints[record_idx].data.is_broken != 0;
                self.constraints[record_idx].data.linear_impulse = solved.linear_impulse;
                self.constraints[record_idx].data.angular_impulse = solved.angular_impulse;
                self.constraints[record_idx].data.is_broken = solved.is_broken;
                if !was_broken && solved.is_broken != 0 {
                    self.broken_constraints_abi.push(self.constraint_index_to_handle[record_idx]);
                }
            }
        }

        for handle in stale_handles {
            self.remove_constraint(handle);
        }
    }

    fn update(&mut self, dt: f32, gravity: f32) {
        if self.solver.bodies.is_empty() {
            self.stats = PhysicsStats::default();
            self.contacts_abi.clear();
            self.pairs_abi.clear();
            // ДОБАВЛЕНО (джойнты/constraint API): без тел ни одно
            // соединение не может решаться (все ссылались бы на
            // несуществующие handle'ы) — очищаем список событий поломки
            // этого шага тем же способом, что и contacts_abi/pairs_abi
            // выше, вместо того чтобы оставлять в нём "протухший" список
            // с прошлого кадра, когда тела ещё существовали.
            self.broken_constraints_abi.clear();
            return;
        }

        let t0 = std::time::Instant::now();
        let raw_pairs: Vec<i32> = self.solver.find_pairs_grid().to_vec();
        let broad_phase_time_ms = t0.elapsed().as_secs_f32() * 1000.0;

        // ДОБАВЛЕНО (джойнты/constraint API, найдено
        // `examples/joint_test.rs::test_fixed_holds_against_gravity`):
        // тела, скреплённые constraint'ом, в реальных сценариях сборки
        // (болт держит деталь ВПЛОТНУЮ к кузову, колесо надето на ступицу
        // и т.п.) почти всегда физически перекрываются своими сферами
        // столкновений. Без этого исключения narrow phase честно находит
        // "проникновение" на КАЖДОМ кадре, а `resolve_contact_simple`
        // (ниже) яростно расталкивает их позиционной коррекцией — которая
        // напрямую воюет с constraint'ом, пытающимся удержать ИМЕННО эту
        // пару вместе в той же самой точке. На практике это проявлялось
        // как взрывной, нефизичный скачок положения в первые же кадры
        // после создания соединения. Стандартное решение (используется
        // практически во всех физических движках с констрейнтами) —
        // отключать контакты между телами одного и того же (не сломанного)
        // соединения; сам констрейнт — единственный источник истины об их
        // взаимном положении, пока он жив.
        //
        // Строится КАЖДЫЙ кадр (а не кэшируется) — множество активных
        // соединений обычно небольшое (десятки-сотни, не тысячи), а
        // индексы тел в солвере двигаются при `remove_body` (`swap_remove`),
        // так что кэш индексов от прошлого кадра всё равно нельзя было бы
        // использовать без пересчёта.
        let mut constrained_pairs: HashSet<(usize, usize)> = HashSet::with_capacity(self.constraints.len());
        for record in &self.constraints {
            if record.data.is_broken != 0 {
                // Сломанное соединение больше не должно подавлять контакты
                // между обломками — наоборот, именно после разрушения им
                // естественно физически столкнуться друг с другом.
                continue;
            }
            if let (Some(&ia), Some(&ib)) = (
                self.handle_to_index.get(&record.body_a_handle),
                self.handle_to_index.get(&record.body_b_handle),
            ) {
                constrained_pairs.insert((ia.min(ib), ia.max(ib)));
            }
        }

        let t1 = std::time::Instant::now();
        self.solver.clear_contacts();
        let mut internal_contacts: Vec<(usize, usize, FortranContact)> = Vec::new();
        for pair in raw_pairs.chunks_exact(2) {
            let ia = pair[0] as usize;
            let ib = pair[1] as usize;
            if ia >= self.solver.bodies.len() || ib >= self.solver.bodies.len() {
                continue;
            }
            if constrained_pairs.contains(&(ia.min(ib), ia.max(ib))) {
                continue;
            }
            // ИСПРАВЛЕНО (найдено по жалобе пользователя на просадки FPS
            // ПОСЛЕ того, как физика реально заработала): плотный опорный
            // "пол" из статических сфер (main.rs — 116 штук с нахлёстом,
            // чтобы не было щелей) взаимно ПЕРЕКРЫВАЕТСЯ соседями по
            // построению — каждая соседняя пара статичных опор физически
            // касается или проникает друг в друга. Broad phase честно
            // находит эти пары каждый кадр (они и правда близко), и без
            // этой проверки для КАЖДОЙ такой пары выполнялся полный GJK
            // (narrow_phase_gjk) и создавался контакт для solve_contacts —
            // то есть десятки-сотни contact-пар статика-статика
            // обрабатывались впустую каждый кадр: solve_contacts всё равно
            // ничего не может сделать с парой из двух тел с inv_mass=0
            // (resolve_contact_simple делит коррекцию по inv_mass, у обеих
            // total_inv_mass=0 — коррекция нулевая), но CPU на broad-phase-
            // совпадение + сам GJK-вызов + добавление в буфер контактов
            // тратится независимо от результата. Статика с статикой
            // физически никогда не должна порождать контакт для солвера —
            // пропускаем пару ДО дорогого narrow phase, а не после.
            if self.solver.bodies[ia].is_static != 0 && self.solver.bodies[ib].is_static != 0 {
                continue;
            }
            // ИСПРАВЛЕНО (полноценная физика — box-vs-box narrow phase, см.
            // `narrow_phase_box_box` в narrow_phase.f90): раньше пара из
            // ДВУХ box-тел не давала контакта вообще (единственное box-тело
            // в сцене было кузовом машины, который с другими box-телами не
            // сталкивался) — теперь честный 15-осевой SAT-тест в Fortran,
            // тот же уровень строгости, что уже есть у sphere-sphere.
            // ИСПРАВЛЕНО (полноценная физика — box-vs-sphere перенесён в
            // Fortran, см. `narrow_phase_box_sphere` в narrow_phase.f90):
            // раньше это (и box-vs-plane в `resolve_plane_contacts` ниже)
            // было единственным местом узкой фазы, считавшимся на
            // Rust-стороне — теперь вся узкая фаза (sphere-sphere/box-box/
            // box-sphere) честно в Fortran, эта функция только
            // диспетчеризует по паре shape_type.
            let mut contact = FortranContact::default();
            let hit = if self.solver.bodies[ia].shape_type == shape_type::SPHERE
                && self.solver.bodies[ib].shape_type == shape_type::SPHERE
            {
                unsafe { ffi::narrow_phase_gjk(&self.solver.bodies[ia], &self.solver.bodies[ib], &mut contact) }
            } else if self.solver.bodies[ia].shape_type == shape_type::BOX
                && self.solver.bodies[ib].shape_type == shape_type::BOX
            {
                unsafe { ffi::narrow_phase_box_box(&self.solver.bodies[ia], &self.solver.bodies[ib], &mut contact) }
            } else {
                unsafe { ffi::narrow_phase_box_sphere(&self.solver.bodies[ia], &self.solver.bodies[ib], &mut contact) }
            };
            if hit != 0 {
                contact.body_a = ia as i32;
                contact.body_b = ib as i32;
                internal_contacts.push((ia, ib, contact));
                self.solver.add_contact(contact);
            }
        }
        let narrow_phase_time_ms = t1.elapsed().as_secs_f32() * 1000.0;

        let t2 = std::time::Instant::now();
        if !self.solver.contacts.is_empty() {
            self.solver.solve_contacts_vectorized(self.config.solver_iterations.max(1), dt);
        }

        // ДОБАВЛЕНО (джойнты/constraint API): решается ПОСЛЕ контактов
        // (соединение не должно "спорить" с ещё не разрешённым
        // проникновением тел) и ДО batch_integrate — joints корректируют
        // линейную/угловую СКОРОСТЬ, а не позицию напрямую, поэтому
        // должны успеть отработать до того, как эта скорость будет
        // проинтегрирована в положение/ориентацию тела этим кадром (см.
        // solve_point/solve_hinge_angular/solve_fixed_angular в
        // `solver.f90` — все три пишут только `velocity`/
        // `angular_velocity`, а не `position`/`orientation` напрямую).
        self.solve_and_update_constraints();

        // ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость): та же
        // позиция в пайплайне, что и констрейнты выше — после контактов
        // тел друг с другом, до интеграции, чтобы `batch_integrate`
        // сразу использовал уже скорректированную (не "проваливающуюся"
        // сквозь пол) скорость/позицию.
        self.resolve_plane_contacts();

        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(8);
        self.solver.batch_integrate(dt, gravity, num_threads);

        self.solver.update_sleep_state(dt, 0.01, 0.5);
        let solver_time_ms = t2.elapsed().as_secs_f32() * 1000.0;

        self.contacts_abi.clear();
        for (ia, ib, c) in &internal_contacts {
            self.contacts_abi.push(PhysicsContact {
                body_a: self.index_to_handle.get(*ia).copied().unwrap_or(-1),
                body_b: self.index_to_handle.get(*ib).copied().unwrap_or(-1),
                normal: c.normal,
                penetration: c.penetration,
                point: c.point,
            });
        }

        self.pairs_abi.clear();
        for pair in raw_pairs.chunks_exact(2) {
            let ia = pair[0] as usize;
            let ib = pair[1] as usize;
            self.pairs_abi.push(self.index_to_handle.get(ia).copied().unwrap_or(-1));
            self.pairs_abi.push(self.index_to_handle.get(ib).copied().unwrap_or(-1));
        }

        let active = self
            .solver
            .bodies
            .iter()
            .filter(|b| b.is_asleep == 0 && b.is_static == 0)
            .count() as u32;

        let broken_count = self.constraints.iter().filter(|r| r.data.is_broken != 0).count() as u32;

        self.stats = PhysicsStats {
            bodies_count: self.solver.bodies.len() as u32,
            active_bodies: active,
            contacts_count: self.contacts_abi.len() as u32,
            pairs_count: (raw_pairs.len() / 2) as u32,
            broad_phase_time_ms,
            narrow_phase_time_ms,
            solver_time_ms,
            constraints_count: self.constraints.len() as u32,
            broken_constraints_count: broken_count,
        };
    }
}

// =====================================================================
// C ABI — экспортируемые функции плагина
// =====================================================================

struct PhysicsInstance {
    state: Mutex<PhysicsState>,
}

extern "C" fn physics_init(_device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void {
    let config = if config_ptr.is_null() {
        eprintln!("[INERTIAL] WARNING: init called with null config_ptr, using defaults");
        PhysicsConfig {
            max_bodies: 1000,
            world_size: 100.0,
            cell_size: 4.0,
            solver_iterations: 8,
            use_simd: 1,
        }
    } else {
        unsafe { *(config_ptr as *const PhysicsConfig) }
    };

    let instance = Box::new(PhysicsInstance {
        state: Mutex::new(PhysicsState::new(config)),
    });
    Box::into_raw(instance) as *mut c_void
}

extern "C" fn physics_shutdown(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(instance as *mut PhysicsInstance));
    }
}

extern "C" fn physics_plugin_update(instance: *mut c_void, dt: f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.update(dt, -9.81);
    }
}

extern "C" fn physics_get_physics_api(_instance: *mut c_void) -> *const c_void {
    &PHYSICS_API as *const PhysicsAPI as *const c_void
}

extern "C" fn physics_get_light_api(_instance: *mut c_void) -> *const c_void {
    std::ptr::null()
}

extern "C" fn api_add_body(instance: *mut c_void, body: *const PhysicsBody) -> i32 {
    if instance.is_null() || body.is_null() {
        return -1;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let body = unsafe { &*body };
    match inst.state.lock() {
        Ok(mut state) => state.add_body(body),
        Err(_) => -1,
    }
}

extern "C" fn api_remove_body(instance: *mut c_void, id: i32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.remove_body(id);
    }
}

extern "C" fn api_get_body(instance: *mut c_void, id: i32) -> PhysicsBody {
    if instance.is_null() {
        return default_abi_body();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.get_body(id).unwrap_or_else(default_abi_body),
        Err(_) => default_abi_body(),
    }
}

extern "C" fn api_get_bodies_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.bodies_count()).unwrap_or(0)
}

extern "C" fn api_update(instance: *mut c_void, dt: f32, gravity: f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.update(dt, gravity);
    }
}

extern "C" fn api_get_contacts(instance: *mut c_void) -> *const PhysicsContact {
    if instance.is_null() {
        return std::ptr::null();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.contacts_abi.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

extern "C" fn api_get_contacts_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.contacts_abi.len() as i32).unwrap_or(0)
}

extern "C" fn api_get_pairs(instance: *mut c_void) -> *const i32 {
    if instance.is_null() {
        return std::ptr::null();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.pairs_abi.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

extern "C" fn api_get_pairs_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| (s.pairs_abi.len() / 2) as i32).unwrap_or(0)
}

extern "C" fn api_get_stats(instance: *mut c_void) -> PhysicsStats {
    if instance.is_null() {
        return PhysicsStats::default();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.stats).unwrap_or_default()
}

// ДОБАВЛЕНО (джойнты/constraint API): те же null-проверки и
// lock()-паттерн, что и у всех остальных `api_*` выше.

extern "C" fn api_add_constraint(instance: *mut c_void, desc: *const ConstraintDesc) -> i32 {
    if instance.is_null() || desc.is_null() {
        return -1;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let desc = unsafe { &*desc };
    match inst.state.lock() {
        Ok(mut state) => state.add_constraint(desc),
        Err(_) => -1,
    }
}

extern "C" fn api_remove_constraint(instance: *mut c_void, id: i32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    if let Ok(mut state) = inst.state.lock() {
        state.remove_constraint(id);
    }
}

extern "C" fn api_get_constraint(instance: *mut c_void, id: i32) -> ConstraintInfo {
    if instance.is_null() {
        return default_constraint_info();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        Ok(state) => state.get_constraint(id).unwrap_or_else(default_constraint_info),
        Err(_) => default_constraint_info(),
    }
}

extern "C" fn api_get_broken_constraints(instance: *mut c_void, count_out: *mut i32) -> *const i32 {
    if instance.is_null() {
        if !count_out.is_null() {
            unsafe { *count_out = 0 };
        }
        return std::ptr::null();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    match inst.state.lock() {
        // ИСПРАВЛЕНО (код-ревью): count читается из ТОГО ЖЕ `state`, под
        // тем же локом, что и указатель — см. комментарий у поля
        // `get_broken_constraints` в `PhysicsAPI` про гонку, которую это
        // устраняет.
        Ok(state) => {
            if !count_out.is_null() {
                unsafe { *count_out = state.broken_constraints_abi.len() as i32 };
            }
            state.broken_constraints_abi.as_ptr()
        }
        Err(_) => {
            if !count_out.is_null() {
                unsafe { *count_out = 0 };
            }
            std::ptr::null()
        }
    }
}

extern "C" fn api_get_broken_constraints_count(instance: *mut c_void) -> i32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    inst.state.lock().map(|s| s.broken_constraints_abi.len() as i32).unwrap_or(0)
}

/// Читает 3 float из сырого указателя — тот же паттерн разыменования
/// массива через границу ABI, что уже используют `LightAPI::cull`
/// (`camera_pos: *const f32`) и `ConstraintDesc`/`add_constraint` для
/// `anchor_a`/`axis_a` и т.п. Null-указатель — no-op (нулевой вектор),
/// вызывающая сторона в этом случае ничего не поменяет, но и не крашнёт
/// процесс.
unsafe fn read_vec3(p: *const f32) -> [f32; 3] {
    if p.is_null() {
        return [0.0; 3];
    }
    unsafe { [*p, *p.add(1), *p.add(2)] }
}

unsafe fn read_vec4(p: *const f32) -> [f32; 4] {
    if p.is_null() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    unsafe { [*p, *p.add(1), *p.add(2), *p.add(3)] }
}

extern "C" fn api_apply_force(instance: *mut c_void, id: i32, force: *const f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let force = unsafe { read_vec3(force) };
    if let Ok(mut state) = inst.state.lock() {
        state.apply_force(id, force);
    }
}

extern "C" fn api_apply_impulse(instance: *mut c_void, id: i32, impulse: *const f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let impulse = unsafe { read_vec3(impulse) };
    if let Ok(mut state) = inst.state.lock() {
        state.apply_impulse(id, impulse);
    }
}

extern "C" fn api_apply_torque(instance: *mut c_void, id: i32, torque: *const f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let torque = unsafe { read_vec3(torque) };
    if let Ok(mut state) = inst.state.lock() {
        state.apply_torque(id, torque);
    }
}

extern "C" fn api_apply_force_at_point(instance: *mut c_void, id: i32, force: *const f32, world_point: *const f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let force = unsafe { read_vec3(force) };
    let world_point = unsafe { read_vec3(world_point) };
    if let Ok(mut state) = inst.state.lock() {
        state.apply_force_at_point(id, force, world_point);
    }
}

extern "C" fn api_set_velocity(instance: *mut c_void, id: i32, linear: *const f32, angular: *const f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let linear = unsafe { read_vec3(linear) };
    let angular = unsafe { read_vec3(angular) };
    if let Ok(mut state) = inst.state.lock() {
        state.set_velocity(id, linear, angular);
    }
}

extern "C" fn api_set_transform(instance: *mut c_void, id: i32, position: *const f32, orientation: *const f32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let position = unsafe { read_vec3(position) };
    let orientation = unsafe { read_vec4(orientation) };
    if let Ok(mut state) = inst.state.lock() {
        state.set_transform(id, position, orientation);
    }
}

extern "C" fn api_add_plane(instance: *mut c_void, desc: *const PlaneDesc) -> i32 {
    if instance.is_null() || desc.is_null() {
        return -1;
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let desc = unsafe { &*desc };
    match inst.state.lock() {
        Ok(mut state) => state.add_plane(desc),
        Err(_) => -1,
    }
}

extern "C" fn api_raycast(instance: *mut c_void, origin: *const f32, direction: *const f32, max_dist: f32, exclude_body: i32) -> RaycastHit {
    if instance.is_null() || origin.is_null() || direction.is_null() {
        return RaycastHit::default();
    }
    let inst = unsafe { &*(instance as *const PhysicsInstance) };
    let origin = unsafe { read_vec3(origin) };
    let direction = unsafe { read_vec3(direction) };
    let exclude_body = if exclude_body >= 0 { Some(exclude_body) } else { None };
    match inst.state.lock() {
        Ok(state) => state.raycast(origin, direction, max_dist, exclude_body),
        Err(_) => RaycastHit::default(),
    }
}

static PHYSICS_API: PhysicsAPI = PhysicsAPI {
    add_body: api_add_body,
    remove_body: api_remove_body,
    get_body: api_get_body,
    get_bodies_count: api_get_bodies_count,
    update: api_update,
    get_contacts: api_get_contacts,
    get_contacts_count: api_get_contacts_count,
    get_pairs: api_get_pairs,
    get_pairs_count: api_get_pairs_count,
    get_stats: api_get_stats,
    add_constraint: api_add_constraint,
    remove_constraint: api_remove_constraint,
    get_constraint: api_get_constraint,
    get_broken_constraints: api_get_broken_constraints,
    get_broken_constraints_count: api_get_broken_constraints_count,
    apply_force: api_apply_force,
    apply_impulse: api_apply_impulse,
    set_velocity: api_set_velocity,
    set_transform: api_set_transform,
    add_plane: api_add_plane,
    apply_torque: api_apply_torque,
    apply_force_at_point: api_apply_force_at_point,
    raycast: api_raycast,
};

#[no_mangle]
pub extern "C" fn get_plugin_api() -> PluginAPI {
    PluginAPI {
        version: PLUGIN_API_VERSION,
        plugin_type: PluginType::Physics,
        name: PLUGIN_NAME.as_ptr() as *const c_char,
        init: physics_init,
        shutdown: physics_shutdown,
        update: physics_plugin_update,
        get_physics_api: physics_get_physics_api,
        get_light_api: physics_get_light_api,
    }
}
