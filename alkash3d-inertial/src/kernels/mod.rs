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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranConstraint {
    pub body_a: i32,
    pub body_b: i32,
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
    pub bias: f32,
    pub accumulated_impulse: f32,
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
    );

    // ИСПРАВЛЕНО: раньше тут отсутствовал параметр `n` — реальная
    // Fortran-сигнатура в broad_phase.f90:
    // (bodies, n, active_indices, active_count, pairs, pair_count).
    // Без этого параметра вызов сдвинул бы все последующие аргументы на
    // одну позицию (то, что должно было быть active_indices, реально
    // читалось бы как n, и т.д.) — неопределённое поведение при первом
    // же вызове. Функция сейчас нигде не вызывается из FortranPhysics,
    // но объявление обязано совпадать с реальной сигнатурой на будущее.
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

    // ИСПРАВЛЕНО: добавлен параметр sleep_timers — без него у функции не
    // было персистентного состояния между кадрами, поэтому она не могла
    // реализовать гистерезис засыпания и была пустышкой (см. kernels_optimized.f90).
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
    /// ДОБАВЛЕНО: таймер сна на каждое тело (см. update_sleep_state).
    /// Должен оставаться СИНХРОННЫМ по длине и порядку с `bodies` —
    /// если когда-нибудь добавите удаление тел из этой обёртки, не
    /// забудьте убрать соответствующий элемент и здесь тоже (в lib.rs,
    /// который реально используется как inertial.dll, это учтено через
    /// общий механизм handle-индирекции — см. PhysicsState).
    pub sleep_timers: Vec<f32>,
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
            grid_width: grid_size,
            grid_height: grid_size,
            cell_size,
        }
    }

    pub fn add_body(&mut self, body: FortranRigidBody) {
        self.bodies.push(body);
        self.sleep_timers.push(0.0);
    }

    pub fn add_contact(&mut self, contact: FortranContact) {
        self.contacts.push(contact);
    }

    pub fn clear_contacts(&mut self) {
        self.contacts.clear();
    }

    /// Broad phase с uniform grid - O(N)
    pub fn find_pairs_grid(&mut self) -> &[i32] {
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
            );
        }

        &self.cell_pairs[..(pair_count as usize * 2)]
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

    /// ДОБАВЛЕНО: SAP broad phase по активному списку — раньше эта
    /// Fortran-функция была объявлена с несовпадающей сигнатурой и нигде
    /// не вызывалась; теперь сигнатура верна, и вот рабочий вызов.
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

    /// ИСПРАВЛЕНО: раньше этот метод называл себя "batch_integrate(...,
    /// num_threads)", но внутри просто вызывал Fortran ПОСЛЕДОВАТЕЛЬНО в
    /// Rust-цикле `for thread in 0..num_threads` — никакие реальные
    /// потоки/rayon не создавались, несмотря на название параметра.
    /// Теперь это по-настоящему многопоточно: каждый чанк тел — это
    /// НЕПЕРЕСЕКАЮЩИЙСЯ мутабельный срез (через `split_at_mut`), поэтому
    /// параллельный доступ безопасен и доказуем компилятором Rust, без
    /// unsafe-жонглирования сырыми указателями с ручным расчётом смещений.
    pub fn batch_integrate(&mut self, dt: f32, gravity: f32, num_threads: usize) {
        if self.bodies.is_empty() {
            return;
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

    /// ИСПРАВЛЕНО: теперь передаёт `sleep_time` и `sleep_timers` — без
    /// них Fortran-сторона не могла ничего реально усыпить (см. комментарий
    /// у update_sleep_state в kernels_optimized.f90).
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

    /// Обновление AABB для всех тел
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
