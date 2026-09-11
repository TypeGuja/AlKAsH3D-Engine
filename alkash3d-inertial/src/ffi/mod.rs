// src/ffi/mod.rs
//! Fortran FFI bindings - ПОЛНАЯ ВЕРСИЯ С ОПТИМИЗАЦИЯМИ

use std::ffi::c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranRigidBody {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angular_acceleration: [f32; 3],
    pub inertia: [[f32; 3]; 3],
    pub inv_inertia: [[f32; 3]; 3],
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub is_static: i32,
    pub is_asleep: i32,
    // ДОБАВЛЕНО (физика автомобиля — вращение кузова): кватернион
    // ориентации (x, y, z, w). layout #[repr(C)] обязан побайтово
    // совпадать с bind(c) типом rigid_body_c в rigid_body.f90.
    // integrate_orientation в rigid_body.f90 — формула интеграции.
    pub orientation: [f32; 4],
    // ДОБАВЛЕНО (код-ревью — per-body радиус вместо одного глобального
    // IMPLICIT_RADIUS на все тела): читается `narrow_phase.f90`
    // (`radius_sum = body_a%radius + body_b%radius`) и моментом инерции
    // в `to_fortran_body` (lib.rs).
    pub radius: f32,
    // ДОБАВЛЕНО (box-коллайдер кузова машины) — см. подробный комментарий
    // у `shape_type`/`half_extents` в `rigid_body_c` (kernels/rigid_body.f90,
    // ОБЯЗАН побайтово совпадать с этим layout). ПОСЛЕДНИЕ поля структуры.
    pub shape_type: i32,
    pub half_extents: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
    pub tangent1: [f32; 3],
    pub tangent2: [f32; 3],
    pub friction_impulse: [f32; 2],
}

/// ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API):
/// типы соединений `FortranConstraint::joint_type` — ЗНАЧЕНИЯ ДОЛЖНЫ
/// побайтово совпадать с константами `JOINT_*` в
/// `src/kernels/rigid_body.f90` (bind(c)-параметры Fortran не
/// экспортируются как C-символы, поэтому синхронизация — вручную, тот же
/// принцип, что уже применяется здесь для layout структур).
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

/// ИЗМЕНЕНО (разборка машины на детали — джойнты/constraint API): layout
/// расширен под универсальные соединения (шар/петля/сварка/ползун) с
/// разрушением по порогу нагрузки — см. подробное обоснование каждого
/// поля у синхронного `constraint_c` в `src/kernels/rigid_body.f90`.
/// Раньше это была структура ТОЛЬКО под шаровой шарнир со скалярным
/// `accumulated_impulse`, никогда не использовавшаяся снаружи этого
/// крейта (в `PhysicsAPI` плагина не было `add_constraint`) — сейчас
/// именно эта структура пересекает ABI-границу движка через
/// `alkash3d-inertial/src/lib.rs::ConstraintDesc`/`ConstraintInfo`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranConstraint {
    pub body_a: i32,
    pub body_b: i32,
    pub joint_type: i32,
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
    pub axis_a: [f32; 3],
    pub axis_b: [f32; 3],
    pub bias: f32,
    pub break_impulse_linear: f32,
    pub break_impulse_angular: f32,
    pub linear_impulse: [f32; 3],
    pub angular_impulse: [f32; 3],
    pub is_broken: i32,
}

impl Default for FortranConstraint {
    fn default() -> Self {
        Self {
            body_a: 0,
            body_b: 0,
            joint_type: joint_type::BALL,
            anchor_a: [0.0; 3],
            anchor_b: [0.0; 3],
            axis_a: [0.0, 1.0, 0.0],
            axis_b: [0.0, 1.0, 0.0],
            bias: 1.0,
            break_impulse_linear: 0.0,
            break_impulse_angular: 0.0,
            linear_impulse: [0.0; 3],
            angular_impulse: [0.0; 3],
            is_broken: 0,
        }
    }
}

impl Default for FortranContact {
    fn default() -> Self {
        Self {
            body_a: 0,
            body_b: 0,
            normal: [0.0; 3],
            penetration: 0.0,
            point: [0.0; 3],
            tangent1: [0.0; 3],
            tangent2: [0.0; 3],
            friction_impulse: [0.0; 2],
        }
    }
}

// Внешние Fortran функции
extern "C" {
    // ===================================================================
    // BROAD PHASE
    // ===================================================================

    // ИСПРАВЛЕНО: добавлен параметр `max_pairs` — реальная ёмкость
    // `cell_pairs` В ПАРАХ. Раньше Fortran-сторона писала в этот буфер
    // без всякой проверки границ (и вдобавок изначально читала и писала
    // ОДИН И ТОТ ЖЕ массив для входных и выходных данных — см. фикс в
    // broad_phase.f90), что при достаточно плотной сцене приводило к
    // записи ЗА ПРЕДЕЛАМИ выделенного буфера и падению с
    // STATUS_ACCESS_VIOLATION. Теперь Fortran останавливает запись на
    // max_pairs, но честно возвращает РЕАЛЬНОЕ количество найденных пар в
    // pair_count — если оно больше max_pairs, вызывающая сторона обязана
    // перевызвать с бОльшим буфером (см. find_pairs_grid ниже).
    pub fn broad_phase_grid(
        bodies: *const FortranRigidBody,
        n: i32,
        cell_size: f32,
        grid_width: i32,
        grid_height: i32,
        cell_starts: *mut i32,
        cell_counts: *mut i32,
        cell_pairs: *mut i32,
        pair_count: *mut i32,
        max_pairs: i32,
    );

    pub fn broad_phase_sap_optimized(
        bodies: *const FortranRigidBody,
        n: i32,
        active_indices: *const i32,
        active_count: i32,
        pairs: *mut i32,
        pair_count: *mut i32,
    );

    pub fn broad_phase_temporal(
        bodies: *const FortranRigidBody,
        n: i32,
        active_list: *const i32,
        active_count: i32,
        pairs: *mut i32,
        pair_count: *mut i32,
        radius: f32,
        time_step: f32,
    );

    // ===================================================================
    // NARROW PHASE
    // ===================================================================
    pub fn narrow_phase_gjk(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    // ДОБАВЛЕНО (полноценная физика — box-vs-box narrow phase, см.
    // narrow_phase.f90): 15-осевой SAT-тест. Требует ОБА тела shape_type
    // == BOX (как и narrow_phase_gjk требует обе SPHERE) — иначе честно
    // возвращает 0, не пытаясь угадать.
    pub fn narrow_phase_box_box(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    // ДОБАВЛЕНО (полноценная физика — box-vs-sphere/box-vs-plane перенесены
    // в Fortran, см. narrow_phase.f90): те же алгоритмы, что раньше жили в
    // Rust (lib.rs), теперь честно в Fortran-ядре, как и остальная узкая
    // фаза. Требует ровно ОДНО из тел BOX (иначе честно возвращает 0).
    pub fn narrow_phase_box_sphere(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    // "Насколько далеко коробка выступает в сторону `normal`" — support-
    // функция OBB, нужная `resolve_plane_contacts` в lib.rs. `orientation`/
    // `half_extents`/`normal` — по 3-4 float'а каждый, передаются по
    // ссылке (без `value`) как везде в этом ABI для массивов.
    pub fn box_effective_radius(orientation: *const f32, half_extents: *const f32, normal: *const f32) -> f32;

    // ДОБАВЛЕНО (полноценная физика — capsule-коллайдер, см. narrow_phase.f90):
    // требует ровно нужную комбинацию shape_type у обоих тел (иначе честно
    // возвращает 0) — тот же контракт, что у box_box/box_sphere выше.
    pub fn narrow_phase_capsule_sphere(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    pub fn narrow_phase_capsule_capsule(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    pub fn narrow_phase_capsule_box(
        body_a: *const FortranRigidBody,
        body_b: *const FortranRigidBody,
        contact: *mut FortranContact,
    ) -> i32;

    // support-функция капсулы для `resolve_plane_contacts`, тот же принцип,
    // что у `box_effective_radius` выше. `half_height`/`radius` — по
    // значению (`value` на Fortran-стороне), `orientation`/`normal` — по
    // ссылке (массивы).
    pub fn capsule_effective_radius(orientation: *const f32, half_height: f32, radius: f32, normal: *const f32) -> f32;

    pub fn generate_collision_pairs(
        bodies: *const FortranRigidBody,
        n: i32,
        pairs: *mut i32,
        pair_count: *mut i32,
        radius: f32,
    );

    // ===================================================================
    // SOLVER
    // ===================================================================
    pub fn integrate_bodies(
        bodies: *mut FortranRigidBody,
        n: i32,
        dt: f32,
    );

    pub fn batch_integrate(
        bodies: *mut FortranRigidBody,
        n: i32,
        dt: f32,
        gravity: f32,
        start_idx: i32,
        end_idx: i32,
    );

    pub fn solve_contacts(
        bodies: *mut FortranRigidBody,
        contacts: *mut FortranContact,
        n_contacts: i32,
        iterations: i32,
    );

    pub fn solve_contacts_vectorized(
        bodies: *mut FortranRigidBody,
        contacts: *mut FortranContact,
        n_contacts: i32,
        iterations: i32,
        dt: f32,
    );

    pub fn solve_constraints(
        bodies: *mut FortranRigidBody,
        constraints: *mut FortranConstraint,
        n_constraints: i32,
        iterations: i32,
    );

    pub fn resolve_penetration_batch(
        bodies: *mut FortranRigidBody,
        contacts: *mut FortranContact,
        n_contacts: i32,
    );

    // ===================================================================
    // AABB и вспомогательные функции
    // ===================================================================
    pub fn update_aabb(
        bodies: *const FortranRigidBody,
        n: i32,
        min_bounds: *mut f32,
        max_bounds: *mut f32,
        radius: f32,
    );

    pub fn update_aabb_vectorized(
        bodies: *const FortranRigidBody,
        n: i32,
        min_bounds: *mut f32,
        max_bounds: *mut f32,
        radius: f32,
    );

    pub fn update_sleep_state(
        bodies: *mut FortranRigidBody,
        n: i32,
        dt: f32,
        sleep_threshold: f32,
        sleep_time: f32,
        sleep_timers: *mut f32,
    );

    pub fn compute_center_of_mass(
        bodies: *const FortranRigidBody,
        n: i32,
        center: *mut f32,
        total_mass: *mut f32,
    );

    // ===================================================================
    // RAYCAST (запрос луча против сцены — см. kernels/raycast.f90)
    // ===================================================================
    // ДОБАВЛЕНО (полноценная физика): честный raycast против сфер, коробок
    // (с учётом ориентации) и статичных полуплоскостей. `plane_normals`/
    // `plane_points` — плоские массивы 3×n_planes (см. `plane_c` в
    // raycast.f90 — там это `real(c_float) :: plane_normals(3, *)`,
    // Fortran допускает assumed-size массив с явной ведущей размерностью).
    // `direction` ОБЯЗАН быть нормированным — вызывающая (Rust) сторона
    // отвечает за это (см. `PhysicsState::raycast` в lib.rs). `exclude_index`
    // — 0-based индекс тела, которое надо пропустить (`-1` — никого не
    // исключать), см. подробное обоснование в raycast.f90.
    pub fn raycast_query(
        bodies: *const FortranRigidBody,
        n_bodies: i32,
        plane_normals: *const f32,
        plane_points: *const f32,
        n_planes: i32,
        origin: *const f32,
        direction: *const f32,
        max_dist: f32,
        exclude_index: i32,
        hit_found: *mut i32,
        hit_distance: *mut f32,
        hit_point: *mut f32,
        hit_normal: *mut f32,
        hit_index: *mut i32,
        hit_is_plane: *mut i32,
    );
}

/// Обёртка для безопасного вызова Fortran
pub struct FortranPhysics {
    pub bodies: Vec<FortranRigidBody>,
    pub contacts: Vec<FortranContact>,
    pub constraints: Vec<FortranConstraint>,
    pub cell_starts: Vec<i32>,
    pub cell_counts: Vec<i32>,
    pub cell_pairs: Vec<i32>,
    pub active_indices: Vec<i32>,
    pub sleep_timers: Vec<f32>,
    /// ДОБАВЛЕНО (apply_force — см. `PhysicsAPI::apply_force` в lib.rs):
    /// аккумулятор силы (Н, мировые координаты) на тело, копится между
    /// вызовами `apply_force` и переносится в `bodies[i].acceleration`
    /// (через `inv_mass`) прямо перед `batch_integrate`, обнуляется сразу
    /// после — тот же жизненный цикл push/swap_remove, что у
    /// `sleep_timers` выше.
    pub force_accum: Vec<[f32; 3]>,
    /// ДОБАВЛЕНО (реальная физика машины — подвеска прикладывает силу НЕ
    /// через центр масс, а в точке колеса, что физически обязано рождать
    /// момент, а не только линейное ускорение): аккумулятор момента силы
    /// (Н·м, мировые координаты), тот же жизненный цикл, что у
    /// `force_accum` выше — копится между вызовами `apply_torque`/
    /// `apply_force_at_point`, переносится в `bodies[i].angular_acceleration`
    /// (через `inv_inertia`, полную матрицу 3×3, не скаляр) прямо перед
    /// `batch_integrate`, обнуляется сразу после.
    pub torque_accum: Vec<[f32; 3]>,
    pub grid_width: i32,
    pub grid_height: i32,
    pub cell_size: f32,
}

impl FortranPhysics {
    pub fn new(max_bodies: usize, world_size: f32, cell_size: f32) -> Self {
        let grid_size = (world_size / cell_size).ceil() as i32;
        let grid_cells = (grid_size * grid_size) as usize;

        Self {
            bodies: Vec::with_capacity(max_bodies),
            contacts: Vec::with_capacity(max_bodies * 2),
            constraints: Vec::with_capacity(max_bodies),
            cell_starts: vec![0; grid_cells],
            cell_counts: vec![0; grid_cells],
            cell_pairs: vec![0; max_bodies * 8],
            active_indices: Vec::with_capacity(max_bodies),
            sleep_timers: Vec::with_capacity(max_bodies),
            force_accum: Vec::with_capacity(max_bodies),
            torque_accum: Vec::with_capacity(max_bodies),
            grid_width: grid_size,
            grid_height: grid_size,
            cell_size,
        }
    }

    pub fn add_body(&mut self, body: FortranRigidBody) {
        self.bodies.push(body);
        self.sleep_timers.push(0.0);
        self.force_accum.push([0.0; 3]);
        self.torque_accum.push([0.0; 3]);
    }

    /// Копит силу (Н) в аккумулятор ДО следующего `batch_integrate` — см.
    /// комментарий у `force_accum`. Будит тело (иначе `batch_integrate`
    /// его просто пропустит целиком, см. `is_asleep` в
    /// kernels_optimized.f90). No-op для статичных тел — им сила не
    /// нужна, они всё равно не интегрируются.
    pub fn apply_force(&mut self, idx: usize, force: [f32; 3]) {
        if self.bodies[idx].is_static != 0 {
            return;
        }
        for k in 0..3 {
            self.force_accum[idx][k] += force[k];
        }
        self.bodies[idx].is_asleep = 0;
    }

    /// Копит момент силы (Н·м) в `torque_accum` — та же семантика, что у
    /// `apply_force` выше, только для угловой составляющей.
    pub fn apply_torque(&mut self, idx: usize, torque: [f32; 3]) {
        if self.bodies[idx].is_static != 0 {
            return;
        }
        for k in 0..3 {
            self.torque_accum[idx][k] += torque[k];
        }
        self.bodies[idx].is_asleep = 0;
    }

    /// Прикладывает силу В ТОЧКЕ `world_point` (мировые координаты), а не
    /// через центр масс — реалистично для подвески (сила пружины на
    /// колесе рождает и линейное ускорение кузова, и вращение, если точка
    /// приложения не совпадает с центром масс). Раскладывается на
    /// `apply_force` (линейная часть) + `apply_torque` c моментом
    /// `torque = (world_point - position) × force` (стандартное
    /// определение момента силы).
    pub fn apply_force_at_point(&mut self, idx: usize, force: [f32; 3], world_point: [f32; 3]) {
        if self.bodies[idx].is_static != 0 {
            return;
        }
        let r = [
            world_point[0] - self.bodies[idx].position[0],
            world_point[1] - self.bodies[idx].position[1],
            world_point[2] - self.bodies[idx].position[2],
        ];
        let torque = [
            r[1] * force[2] - r[2] * force[1],
            r[2] * force[0] - r[0] * force[2],
            r[0] * force[1] - r[1] * force[0],
        ];
        self.apply_force(idx, force);
        self.apply_torque(idx, torque);
    }

    /// Мгновенно `v += impulse * inv_mass` — в отличие от `apply_force`,
    /// не ждёт следующего `batch_integrate`.
    pub fn apply_impulse(&mut self, idx: usize, impulse: [f32; 3]) {
        if self.bodies[idx].is_static != 0 {
            return;
        }
        let inv_mass = self.bodies[idx].inv_mass;
        for k in 0..3 {
            self.bodies[idx].velocity[k] += impulse[k] * inv_mass;
        }
        self.bodies[idx].is_asleep = 0;
    }

    /// Прямая перезапись линейной/угловой скорости (телепорт скорости).
    pub fn set_velocity(&mut self, idx: usize, linear: [f32; 3], angular: [f32; 3]) {
        if self.bodies[idx].is_static != 0 {
            return;
        }
        self.bodies[idx].velocity = linear;
        self.bodies[idx].angular_velocity = angular;
        self.bodies[idx].is_asleep = 0;
    }

    /// Прямая перезапись позиции/ориентации (телепорт). Скорость НЕ
    /// трогает — вызывающая сторона зовёт `set_velocity` отдельно, если
    /// нужно ещё и погасить/задать скорость при телепорте. Кватернион
    /// нормализуется защитно — вызывающая сторона может передать
    /// ненормализованный (например накопленную ошибку из другого места).
    pub fn set_transform(&mut self, idx: usize, position: [f32; 3], orientation: [f32; 4]) {
        if self.bodies[idx].is_static != 0 {
            return;
        }
        self.bodies[idx].position = position;
        let len_sq: f32 = orientation.iter().map(|c| c * c).sum();
        self.bodies[idx].orientation = if len_sq > 1.0e-12 {
            let inv_len = len_sq.sqrt().recip();
            [orientation[0] * inv_len, orientation[1] * inv_len, orientation[2] * inv_len, orientation[3] * inv_len]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        self.bodies[idx].is_asleep = 0;
    }

    pub fn add_contact(&mut self, contact: FortranContact) {
        self.contacts.push(contact);
    }

    pub fn clear_contacts(&mut self) {
        self.contacts.clear();
    }

    /// ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API): в
    /// отличие от контактов (пересчитываются заново каждый кадр из
    /// broad+narrow phase, см. `add_contact`/`clear_contacts` выше),
    /// констрейнты — ДОЛГОЖИВУЩИЕ игровые объекты (болт остаётся болтом,
    /// пока его явно не открутили или не сломали), поэтому у них
    /// раздельные push/clear/get-по-индексу — вызывающая сторона
    /// (`PhysicsState` в `lib.rs`) сама решает, когда пересобирать этот
    /// буфер (при добавлении/удалении соединения или сдвиге индексов тел
    /// после `remove_body`), а не обязана делать это каждый кадр заново.
    pub fn clear_constraints(&mut self) {
        self.constraints.clear();
    }

    pub fn push_constraint(&mut self, constraint: FortranConstraint) {
        self.constraints.push(constraint);
    }

    /// Решает ВСЕ констрейнты, сейчас лежащие в `self.constraints`, читая/
    /// записывая `self.bodies` напрямую по индексам `body_a`/`body_b`
    /// каждого констрейнта (индексы, НЕ стабильные handle'ы — см.
    /// подробное объяснение разницы в `PhysicsState::update` в `lib.rs`,
    /// где handle'ы переводятся в текущие индексы перед КАЖДЫМ вызовом
    /// этого метода, потому что `remove_body` двигает индексы через
    /// `swap_remove`). Ничего не делает при пустом буфере — тот же
    /// принцип раннего выхода, что уже применяется для контактов в
    /// `PhysicsState::update`.
    pub fn solve_constraints(&mut self, iterations: i32) {
        if self.constraints.is_empty() {
            return;
        }
        unsafe {
            solve_constraints(
                self.bodies.as_mut_ptr(),
                self.constraints.as_mut_ptr(),
                self.constraints.len() as i32,
                iterations,
            );
        }
    }

    /// Broad phase с uniform grid - O(N).
    ///
    /// ИСПРАВЛЕНО: раньше вызов не сообщал Fortran-стороне реальную
    /// ёмкость `cell_pairs`, из-за чего запись могла уйти за пределы
    /// буфера (см. подробности в объявлении `broad_phase_grid` выше и в
    /// broad_phase.f90). Теперь передаём ёмкость явно и, если Fortran
    /// сообщает, что реальных пар оказалось больше — увеличиваем буфер и
    /// перевызываем, вместо того чтобы рисковать переполнением.
    pub fn find_pairs_grid(&mut self) -> &[i32] {
        loop {
            let capacity_pairs = (self.cell_pairs.len() / 2) as i32;
            let mut pair_count = 0;

            unsafe {
                broad_phase_grid(
                    self.bodies.as_ptr(),
                    self.bodies.len() as i32,
                    self.cell_size,
                    self.grid_width,
                    self.grid_height,
                    self.cell_starts.as_mut_ptr(),
                    self.cell_counts.as_mut_ptr(),
                    self.cell_pairs.as_mut_ptr(),
                    &mut pair_count,
                    capacity_pairs,
                );
            }

            if pair_count > capacity_pairs {
                // Буфера не хватило — Fortran честно сообщил истинное
                // количество пар, но записал только первые capacity_pairs
                // из них. Увеличиваем буфер с запасом и пробуем снова.
                let new_len = ((pair_count as usize) * 2 + 16) * 2;
                self.cell_pairs.resize(new_len, 0);
                continue;
            }

            return &self.cell_pairs[..(pair_count.max(0) as usize * 2)];
        }
    }

    /// Быстрая генерация пар коллизий
    pub fn generate_pairs_fast(&mut self, radius: f32) -> &[i32] {
        let mut pair_count = 0;

        unsafe {
            generate_collision_pairs(
                self.bodies.as_ptr(),
                self.bodies.len() as i32,
                self.cell_pairs.as_mut_ptr(),
                &mut pair_count,
                radius,
            );
        }

        &self.cell_pairs[..(pair_count as usize * 2)]
    }

    /// Temporal broad phase
    pub fn find_pairs_temporal(&mut self, dt: f32, radius: f32) -> &[i32] {
        self.active_indices.clear();
        for (i, body) in self.bodies.iter().enumerate() {
            if body.is_asleep == 0 && body.is_static == 0 {
                self.active_indices.push(i as i32);
            }
        }

        let mut pair_count = 0;

        unsafe {
            broad_phase_temporal(
                self.bodies.as_ptr(),
                self.bodies.len() as i32,
                self.active_indices.as_ptr(),
                self.active_indices.len() as i32,
                self.cell_pairs.as_mut_ptr(),
                &mut pair_count,
                radius,
                dt,
            );
        }

        &self.cell_pairs[..(pair_count as usize * 2)]
    }

    pub fn find_pairs_sap_optimized(&mut self) -> &[i32] {
        self.active_indices.clear();
        for (i, body) in self.bodies.iter().enumerate() {
            if body.is_asleep == 0 && body.is_static == 0 {
                self.active_indices.push(i as i32);
            }
        }

        let mut pair_count = 0;
        unsafe {
            broad_phase_sap_optimized(
                self.bodies.as_ptr(),
                self.bodies.len() as i32,
                self.active_indices.as_ptr(),
                self.active_indices.len() as i32,
                self.cell_pairs.as_mut_ptr(),
                &mut pair_count,
            );
        }

        &self.cell_pairs[..(pair_count as usize * 2)]
    }

    pub fn integrate(&mut self, dt: f32) {
        unsafe {
            integrate_bodies(
                self.bodies.as_mut_ptr(),
                self.bodies.len() as i32,
                dt,
            );
        }
    }

    pub fn batch_integrate(&mut self, dt: f32, gravity: f32, num_threads: usize) {
        if self.bodies.is_empty() {
            return;
        }

        // ДОБАВЛЕНО (apply_force): переносим накопленный за кадр(ы)
        // аккумулятор силы в `acceleration` (F=ma => a=F*inv_mass) прямо
        // перед интеграцией — та самая строка в kernels_optimized.f90/
        // batch_integrate теперь читает именно это поле. Статичные тела
        // сюда не попадают в `apply_force` (no-op), но на всякий случай
        // не трогаем их acceleration и здесь тоже.
        for i in 0..self.bodies.len() {
            if self.bodies[i].is_static == 0 {
                let inv_mass = self.bodies[i].inv_mass;
                let f = self.force_accum[i];
                self.bodies[i].acceleration = [f[0] * inv_mass, f[1] * inv_mass, f[2] * inv_mass];

                // ДОБАВЛЕНО (apply_torque/apply_force_at_point — подвеска
                // машины): та же идея, что и у линейного ускорения выше
                // (F*inv_mass), но `angular_acceleration = inv_inertia *
                // torque` — inv_inertia ПОЛНАЯ матрица 3×3 (см.
                // `compute_local_inertia` в lib.rs), а не скаляр, поэтому
                // умножение матрица-на-вектор явно построчно, а не F*inv_mass
                // покомпонентно.
                let t = self.torque_accum[i];
                let ii = self.bodies[i].inv_inertia;
                self.bodies[i].angular_acceleration = [
                    ii[0][0] * t[0] + ii[0][1] * t[1] + ii[0][2] * t[2],
                    ii[1][0] * t[0] + ii[1][1] * t[1] + ii[1][2] * t[2],
                    ii[2][0] * t[0] + ii[2][1] * t[1] + ii[2][2] * t[2],
                ];
            }
        }

        let num_threads = num_threads.max(1).min(self.bodies.len());
        let chunk_size = (self.bodies.len() + num_threads - 1) / num_threads;

        std::thread::scope(|scope| {
            let mut rest = self.bodies.as_mut_slice();
            while !rest.is_empty() {
                let take = chunk_size.min(rest.len());
                let (chunk, remainder) = rest.split_at_mut(take);
                rest = remainder;
                let n = chunk.len() as i32;
                scope.spawn(move || unsafe {
                    batch_integrate(chunk.as_mut_ptr(), n, dt, gravity, 1, n);
                });
            }
        });

        // Аккумулятор — за ОДИН кадр, не накапливается дальше (тот же
        // контракт, что у Bullet/Box2D: вызывающая сторона обязана звать
        // apply_force КАЖДЫЙ кадр, пока сила должна действовать).
        for f in self.force_accum.iter_mut() {
            *f = [0.0; 3];
        }
        for t in self.torque_accum.iter_mut() {
            *t = [0.0; 3];
        }
    }

    pub fn solve_contacts(&mut self, iterations: i32) {
        unsafe {
            solve_contacts(
                self.bodies.as_mut_ptr(),
                self.contacts.as_mut_ptr(),
                self.contacts.len() as i32,
                iterations,
            );
        }
    }

    pub fn solve_contacts_vectorized(&mut self, iterations: i32, dt: f32) {
        unsafe {
            solve_contacts_vectorized(
                self.bodies.as_mut_ptr(),
                self.contacts.as_mut_ptr(),
                self.contacts.len() as i32,
                iterations,
                dt,
            );
        }
    }

    pub fn resolve_penetration_batch(&mut self) {
        unsafe {
            resolve_penetration_batch(
                self.bodies.as_mut_ptr(),
                self.contacts.as_mut_ptr(),
                self.contacts.len() as i32,
            );
        }
    }

    pub fn update_sleep_state(&mut self, dt: f32, sleep_threshold: f32, sleep_time: f32) {
        debug_assert_eq!(self.sleep_timers.len(), self.bodies.len());
        unsafe {
            update_sleep_state(
                self.bodies.as_mut_ptr(),
                self.bodies.len() as i32,
                dt,
                sleep_threshold,
                sleep_time,
                self.sleep_timers.as_mut_ptr(),
            );
        }
    }

    pub fn update_bounds(&mut self, radius: f32) -> (Vec<f32>, Vec<f32>) {
        let mut min_bounds = vec![0.0f32; self.bodies.len() * 3];
        let mut max_bounds = vec![0.0f32; self.bodies.len() * 3];

        unsafe {
            update_aabb(
                self.bodies.as_ptr(),
                self.bodies.len() as i32,
                min_bounds.as_mut_ptr(),
                max_bounds.as_mut_ptr(),
                radius,
            );
        }

        (min_bounds, max_bounds)
    }

    pub fn update_bounds_vectorized(&mut self, radius: f32) -> (Vec<f32>, Vec<f32>) {
        let mut min_bounds = vec![0.0f32; self.bodies.len() * 3];
        let mut max_bounds = vec![0.0f32; self.bodies.len() * 3];

        unsafe {
            update_aabb_vectorized(
                self.bodies.as_ptr(),
                self.bodies.len() as i32,
                min_bounds.as_mut_ptr(),
                max_bounds.as_mut_ptr(),
                radius,
            );
        }

        (min_bounds, max_bounds)
    }

    pub fn active_indices(&self) -> Vec<u32> {
        self.bodies
            .iter()
            .enumerate()
            .filter(|(_, b)| b.is_asleep == 0 && b.is_static == 0)
            .map(|(i, _)| i as u32)
            .collect()
    }

    pub fn get_center_of_mass(&self) -> (f32, f32, f32, f32) {
        let mut center = [0.0f32; 3];
        let mut total_mass = 0.0f32;

        unsafe {
            compute_center_of_mass(
                self.bodies.as_ptr(),
                self.bodies.len() as i32,
                center.as_mut_ptr(),
                &mut total_mass,
            );
        }

        (center[0], center[1], center[2], total_mass)
    }
}

impl Default for FortranPhysics {
    fn default() -> Self {
        Self::new(10000, 1000.0, 10.0)
    }
}
