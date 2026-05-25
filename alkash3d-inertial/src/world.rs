// src/world.rs - упрощённая версия без FFI (для начала)
#![allow(dead_code)]

use crate::{RigidBody, Vector3, Contact, CollisionDetector, SequentialImpulseSolver};
use rayon::prelude::*;
use std::time::Instant;

#[derive(Default, Debug, Clone, Copy)]
pub struct PhysicsStats {
    pub bodies_count: usize,
    pub active_bodies: usize,
    pub collisions_detected: u32,
    pub solver_iterations: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
    pub update_time_ms: f32,
    pub memory_bandwidth_gbps: f32,
}

// Uniform Grid для broad phase
pub struct UniformGrid {
    cells: Vec<Vec<u32>>,
    cell_size: f32,
    grid_width: usize,
    grid_height: usize,
}

impl UniformGrid {
    pub fn new(world_size: f32, cell_size: f32) -> Self {
        let grid_width = (world_size / cell_size).ceil() as usize;
        let grid_height = grid_width;
        let total_cells = grid_width * grid_height;

        Self {
            cells: (0..total_cells).map(|_| Vec::with_capacity(64)).collect(),
            cell_size,
            grid_width,
            grid_height,
        }
    }

    #[inline(always)]
    pub fn find_pairs_parallel(&mut self, x: &[f32], z: &[f32], active: &[bool]) -> Vec<(u32, u32)> {
        use rayon::prelude::*;

        // Очистка ячеек
        for cell in &mut self.cells {
            cell.clear();
        }

        let cell_size = self.cell_size;
        let grid_width = self.grid_width as i32;
        let grid_height = self.grid_height as i32;

        // Сбор индексов
        let mut items: Vec<(usize, u32)> = (0..x.len())
            .filter(|&i| active[i])
            .map(|i| {
                let cx = ((x[i] / cell_size).floor() as i32)
                    .max(0)
                    .min(grid_width - 1);
                let cz = ((z[i] / cell_size).floor() as i32)
                    .max(0)
                    .min(grid_height - 1);
                let cell_idx = (cz * grid_width + cx) as usize;
                (cell_idx, i as u32)
            })
            .collect();

        // Сортировка
        items.sort_by_key(|(cell_idx, _)| *cell_idx);

        // Заполнение ячеек
        let mut current_cell = 0;
        let mut start = 0;
        for i in 0..items.len() {
            if items[i].0 != current_cell {
                if start < i {
                    self.cells[current_cell].extend(items[start..i].iter().map(|(_, id)| *id));
                }
                current_cell = items[i].0;
                start = i;
            }
        }
        if start < items.len() {
            self.cells[current_cell].extend(items[start..].iter().map(|(_, id)| *id));
        }

        // Поиск пар
        self.cells
            .par_iter()
            .flat_map(|cell| {
                let len = cell.len();
                if len < 2 {
                    return Vec::new();
                }
                let mut pairs = Vec::with_capacity(len * (len - 1) / 2);
                for i in 0..len {
                    for j in i + 1..len {
                        pairs.push((cell[i], cell[j]));
                    }
                }
                pairs
            })
            .collect()
    }
}

#[repr(C, align(64))]
pub struct AlignedRigidBody {
    pub body: RigidBody,
}

impl AlignedRigidBody {
    #[inline(always)]
    pub fn new(body: RigidBody) -> Self {
        Self { body }
    }
}

// ОСНОВНОЙ ФИЗИЧЕСКИЙ МИР
pub struct PhysicsWorld {
    bodies: Vec<AlignedRigidBody>,
    gravity: Vector3,
    stats: PhysicsStats,
    enable_collisions: bool,
    parallel_threshold: usize,
    positions_x: Vec<f32>,
    positions_y: Vec<f32>,
    positions_z: Vec<f32>,
    velocities_x: Vec<f32>,
    velocities_y: Vec<f32>,
    velocities_z: Vec<f32>,
    grid: UniformGrid,
    collision_radius: f32,
    gravity_y: f32,
}

impl PhysicsWorld {
    #[inline(always)]
    pub fn new() -> Self {
        Self::with_capacity(10000)
    }

    #[inline(always)]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bodies: Vec::with_capacity(capacity),
            gravity: Vector3::new(0.0, -9.81, 0.0),
            stats: PhysicsStats::default(),
            enable_collisions: false,
            parallel_threshold: 500,
            positions_x: Vec::with_capacity(capacity),
            positions_y: Vec::with_capacity(capacity),
            positions_z: Vec::with_capacity(capacity),
            velocities_x: Vec::with_capacity(capacity),
            velocities_y: Vec::with_capacity(capacity),
            velocities_z: Vec::with_capacity(capacity),
            grid: UniformGrid::new(1000.0, 10.0),
            collision_radius: 0.5,
            gravity_y: -9.81,
        }
    }

    #[inline(always)]
    pub fn with_collisions(mut self, enabled: bool) -> Self {
        self.enable_collisions = enabled;
        self
    }

    #[inline(always)]
    pub fn with_collision_radius(mut self, radius: f32) -> Self {
        self.collision_radius = radius;
        self
    }

    #[inline(always)]
    pub fn add_body(&mut self, mut body: RigidBody) -> u32 {
        let id = self.bodies.len() as u32;
        body.id = id;

        self.positions_x.push(body.position.x);
        self.positions_y.push(body.position.y);
        self.positions_z.push(body.position.z);
        self.velocities_x.push(body.velocity.x);
        self.velocities_y.push(body.velocity.y);
        self.velocities_z.push(body.velocity.z);

        self.bodies.push(AlignedRigidBody::new(body));
        id
    }

    #[inline(always)]
    pub fn get_body(&self, id: u32) -> Option<&RigidBody> {
        self.bodies.get(id as usize).map(|b| &b.body)
    }

    #[inline(always)]
    pub fn get_body_mut(&mut self, id: u32) -> Option<&mut RigidBody> {
        self.bodies.get_mut(id as usize).map(|b| &mut b.body)
    }

    #[inline(always)]
    pub fn bodies(&self) -> &[AlignedRigidBody] {
        &self.bodies
    }

    #[inline(always)]
    pub fn get_position(&self, id: u32) -> Vector3 {
        let i = id as usize;
        Vector3::new(self.positions_x[i], self.positions_y[i], self.positions_z[i])
    }

    // ГЛАВНЫЙ ЦИКЛ ОБНОВЛЕНИЯ
    #[inline(always)]
    pub fn update(&mut self, dt: f32) {
        if self.bodies.is_empty() {
            return;
        }

        let frame_start = Instant::now();

        // Синхронизация
        self.sync_simd_arrays();

        // Маска активных тел
        let active_mask: Vec<bool> = self.bodies
            .iter()
            .map(|b| !b.body.is_asleep && !b.body.is_static)
            .collect();

        let active_count = active_mask.iter().filter(|&&x| x).count();
        self.stats.active_bodies = active_count;

        // BROAD PHASE
        let broad_start = Instant::now();
        let pairs = if self.enable_collisions && active_count > 1 {
            self.grid.find_pairs_parallel(&self.positions_x, &self.positions_z, &active_mask)
        } else {
            Vec::new()
        };
        self.stats.broad_phase_time_ms = broad_start.elapsed().as_secs_f32() * 1000.0;

        // NARROW PHASE
        let narrow_start = Instant::now();
        let mut contacts = Vec::with_capacity(pairs.len());

        if self.enable_collisions && !pairs.is_empty() {
            let radius = self.collision_radius;
            let radius_sum = radius + radius;
            let radius_sum_sq = radius_sum * radius_sum;

            for (a, b) in pairs {
                let a_usize = a as usize;
                let b_usize = b as usize;

                if a_usize < self.bodies.len() && b_usize < self.bodies.len() && active_mask[a_usize] && active_mask[b_usize] {
                    let dx = self.positions_x[b_usize] - self.positions_x[a_usize];
                    let dy = self.positions_y[b_usize] - self.positions_y[a_usize];
                    let dz = self.positions_z[b_usize] - self.positions_z[a_usize];
                    let dist_sq = dx * dx + dy * dy + dz * dz;

                    if dist_sq < radius_sum_sq {
                        let distance = if dist_sq > 0.0 { dist_sq.sqrt() } else { 0.001 };
                        let inv_dist = 1.0 / distance;
                        let nx = dx * inv_dist;
                        let ny = dy * inv_dist;
                        let nz = dz * inv_dist;
                        let penetration = radius_sum - distance;

                        contacts.push(Contact {
                            body_a: a,
                            body_b: b,
                            point: Vector3::new(
                                self.positions_x[a_usize] + nx * radius,
                                self.positions_y[a_usize] + ny * radius,
                                self.positions_z[a_usize] + nz * radius,
                            ),
                            normal: Vector3::new(nx, ny, nz),
                            penetration,
                        });
                    }
                }
            }
        }
        self.stats.narrow_phase_time_ms = narrow_start.elapsed().as_secs_f32() * 1000.0;
        self.stats.collisions_detected = contacts.len() as u32;

        // SOLVER
        let solver_start = Instant::now();
        if self.enable_collisions && !contacts.is_empty() {
            let mut bodies_mut: Vec<RigidBody> = self.bodies.iter().map(|b| b.body.clone()).collect();
            let mut solver = SequentialImpulseSolver::new();
            solver.solve_contacts(&mut bodies_mut, &contacts);

            for (i, body) in bodies_mut.into_iter().enumerate() {
                self.positions_x[i] = body.position.x;
                self.positions_y[i] = body.position.y;
                self.positions_z[i] = body.position.z;
                self.velocities_x[i] = body.velocity.x;
                self.velocities_y[i] = body.velocity.y;
                self.velocities_z[i] = body.velocity.z;
                self.bodies[i].body = body;
            }
        }
        self.stats.solver_time_ms = solver_start.elapsed().as_secs_f32() * 1000.0;

        // ИНТЕГРАЦИЯ (SIMD)
        #[cfg(target_arch = "x86_64")]
        {
            unsafe {
                crate::simd_math::update_bodies_simd(
                    &mut self.positions_x, &mut self.positions_y, &mut self.positions_z,
                    &mut self.velocities_x, &mut self.velocities_y, &mut self.velocities_z,
                    dt, self.gravity_y,
                );
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            for i in 0..self.bodies.len() {
                if !self.bodies[i].body.is_asleep && !self.bodies[i].body.is_static {
                    self.velocities_y[i] += self.gravity_y * dt;
                    self.positions_x[i] += self.velocities_x[i] * dt;
                    self.positions_y[i] += self.velocities_y[i] * dt;
                    self.positions_z[i] += self.velocities_z[i] * dt;
                }
            }
        }

        // Обновление тел
        for i in 0..self.bodies.len() {
            let body = &mut self.bodies[i];
            if !body.body.is_asleep && !body.body.is_static {
                body.body.position.x = self.positions_x[i];
                body.body.position.y = self.positions_y[i];
                body.body.position.z = self.positions_z[i];
                body.body.velocity.x = self.velocities_x[i];
                body.body.velocity.y = self.velocities_y[i];
                body.body.velocity.z = self.velocities_z[i];

                // Система сна
                let linear_speed = body.body.velocity.magnitude();
                let angular_speed = body.body.angular_velocity.magnitude();

                if linear_speed < body.body.linear_sleep_threshold &&
                    angular_speed < body.body.angular_sleep_threshold {
                    body.body.sleep_timer += dt;
                    if body.body.sleep_timer > 2.0 {
                        body.body.is_asleep = true;
                    }
                } else {
                    body.body.sleep_timer = 0.0;
                }
            }
        }

        self.stats.update_time_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        self.stats.bodies_count = self.bodies.len();
    }

    fn sync_simd_arrays(&mut self) {
        for (i, body) in self.bodies.iter().enumerate() {
            self.positions_x[i] = body.body.position.x;
            self.positions_y[i] = body.body.position.y;
            self.positions_z[i] = body.body.position.z;
            self.velocities_x[i] = body.body.velocity.x;
            self.velocities_y[i] = body.body.velocity.y;
            self.velocities_z[i] = body.body.velocity.z;
        }
    }

    #[inline(always)]
    pub fn get_stats(&self) -> &PhysicsStats {
        &self.stats
    }

    #[inline(always)]
    pub fn reset_stats(&mut self) {
        self.stats = PhysicsStats::default();
    }

    #[inline(always)]
    pub fn set_gravity(&mut self, gravity: Vector3) {
        self.gravity = gravity;
        self.gravity_y = gravity.y;
    }

    #[inline(always)]
    pub fn set_collision_radius(&mut self, radius: f32) {
        self.collision_radius = radius;
    }

    #[inline(always)]
    pub fn set_parallel_threshold(&mut self, threshold: usize) {
        self.parallel_threshold = threshold;
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.bodies.clear();
        self.positions_x.clear();
        self.positions_y.clear();
        self.positions_z.clear();
        self.velocities_x.clear();
        self.velocities_y.clear();
        self.velocities_z.clear();
        self.stats = PhysicsStats::default();
    }

    #[inline(always)]
    pub fn reserve(&mut self, additional: usize) {
        self.bodies.reserve(additional);
        self.positions_x.reserve(additional);
        self.positions_y.reserve(additional);
        self.positions_z.reserve(additional);
        self.velocities_x.reserve(additional);
        self.velocities_y.reserve(additional);
        self.velocities_z.reserve(additional);
    }

    #[inline(always)]
    pub fn wake_all(&mut self) {
        for body in &mut self.bodies {
            body.body.is_asleep = false;
            body.body.sleep_timer = 0.0;
        }
    }
}

impl Default for PhysicsWorld {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_body() {
        let mut world = PhysicsWorld::new();
        let body = RigidBody::new(1.0, Vector3::zeros());
        let id = world.add_body(body);
        assert_eq!(id, 0);
        assert_eq!(world.bodies().len(), 1);
    }

    #[test]
    fn test_gravity() {
        let mut world = PhysicsWorld::new();
        let body = RigidBody::new(1.0, Vector3::new(0.0, 10.0, 0.0));
        world.add_body(body);
        world.update(1.0);
        let body = world.get_body(0).unwrap();
        assert!(body.position.y < 10.0);
    }
}