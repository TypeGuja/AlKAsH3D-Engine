// src/world.rs
// ═══════════════════════════════════════════════════════════════════
// INERTIAL PHYSICS ENGINE - SAFE PARALLEL EDITION
// Без unsafe, с безопасным параллелизмом через разделение данных
// ═══════════════════════════════════════════════════════════════════

use crate::{RigidBody, Vector3};
use rayon::prelude::*;
use std::time::Instant;

// ──────────────────────────────────────────────────────────────────
// СТАТИСТИКА ПРОИЗВОДИТЕЛЬНОСТИ
// ──────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────
// ВЫРОВНЕННОЕ ТВЁРДОЕ ТЕЛО
// ──────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────
// ОСНОВНОЙ ФИЗИЧЕСКИЙ МИР
// ──────────────────────────────────────────────────────────────────

pub struct PhysicsWorld {
    bodies: Vec<AlignedRigidBody>,
    gravity: Vector3,
    stats: PhysicsStats,
    enable_collisions: bool,
    parallel_threshold: usize,
}

impl PhysicsWorld {
    // ──────────────────────────────────────────────────────────────
    // КОНСТРУКТОРЫ
    // ──────────────────────────────────────────────────────────────

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
        }
    }

    #[inline(always)]
    pub fn with_collisions(mut self, enabled: bool) -> Self {
        self.enable_collisions = enabled;
        self
    }

    // ──────────────────────────────────────────────────────────────
    // УПРАВЛЕНИЕ ТЕЛАМИ
    // ──────────────────────────────────────────────────────────────

    #[inline(always)]
    pub fn add_body(&mut self, body: RigidBody) -> u32 {
        let id = self.bodies.len() as u32;
        self.bodies.push(AlignedRigidBody::new(body));
        id
    }

    #[inline(always)]
    pub fn add_bodies(&mut self, bodies: impl IntoIterator<Item = RigidBody>) -> Vec<u32> {
        bodies.into_iter().map(|b| self.add_body(b)).collect()
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

    // ──────────────────────────────────────────────────────────────
    // БЕЗОПАСНОЕ ПАРАЛЛЕЛЬНОЕ ОБНОВЛЕНИЕ
    // ──────────────────────────────────────────────────────────────

    fn update_parallel(&mut self, dt: f32) {
        let gravity = self.gravity;

        // Безопасный способ: разбиваем на независимые чанки
        // Используем par_iter_mut напрямую (работает!)
        self.bodies.par_iter_mut().for_each(|body| {
            if !body.body.is_asleep && !body.body.is_static {
                body.body.velocity.y += gravity.y * dt;
                body.body.position.x += body.body.velocity.x * dt;
                body.body.position.y += body.body.velocity.y * dt;
                body.body.position.z += body.body.velocity.z * dt;
                body.body.force_accumulator = Vector3::zeros();
            }
        });

        // Коллизии (если включены) - последовательно для безопасности
        if self.enable_collisions {
            self.solve_collisions_sequential();
        }
    }

    // ──────────────────────────────────────────────────────────────
    // ОДНОПОТОЧНОЕ ОБНОВЛЕНИЕ
    // ──────────────────────────────────────────────────────────────

    fn update_single(&mut self, dt: f32) {
        let gravity = self.gravity;

        for body in &mut self.bodies {
            if !body.body.is_asleep && !body.body.is_static {
                body.body.velocity.y += gravity.y * dt;
                body.body.position.x += body.body.velocity.x * dt;
                body.body.position.y += body.body.velocity.y * dt;
                body.body.position.z += body.body.velocity.z * dt;
                body.body.force_accumulator = Vector3::zeros();
            }
        }

        if self.enable_collisions {
            self.solve_collisions_sequential();
        }
    }

    // ──────────────────────────────────────────────────────────────
    // ОСНОВНОЙ ЦИКЛ ОБНОВЛЕНИЯ
    // ──────────────────────────────────────────────────────────────

    #[inline(always)]
    pub fn update(&mut self, dt: f32) {
        let frame_start = Instant::now();

        if self.bodies.len() > self.parallel_threshold {
            self.update_parallel(dt);
        } else {
            self.update_single(dt);
        }

        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        self.stats.update_time_ms = frame_time;

        let memory_mb = (self.bodies.len() * std::mem::size_of::<RigidBody>()) as f32 / (1024.0 * 1024.0);
        self.stats.memory_bandwidth_gbps = if frame_time > 0.0 {
            (memory_mb / (frame_time / 1000.0)) / 1024.0
        } else {
            0.0
        };

        self.stats.bodies_count = self.bodies.len();
        self.stats.active_bodies = self.bodies.iter().filter(|b| !b.body.is_asleep).count();
    }

    // ──────────────────────────────────────────────────────────────
    // КОЛЛИЗИИ (ПОСЛЕДОВАТЕЛЬНЫЕ, НО ОПТИМИЗИРОВАННЫЕ)
    // ──────────────────────────────────────────────────────────────

    fn solve_collisions_sequential(&mut self) {
        let n = self.bodies.len();
        let mut collisions = 0;

        for i in 0..n {
            for j in i + 1..n {
                // Вычисляем дистанцию (без квадратного корня если можно)
                let dx = self.bodies[j].body.position.x - self.bodies[i].body.position.x;
                let dy = self.bodies[j].body.position.y - self.bodies[i].body.position.y;
                let dz = self.bodies[j].body.position.z - self.bodies[i].body.position.z;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                let min_dist = 1.0;
                let min_dist_sq = min_dist * min_dist;

                if dist_sq < min_dist_sq && dist_sq > 0.0001 {
                    let dist = dist_sq.sqrt();
                    let inv_dist = 1.0 / dist;
                    let nx = dx * inv_dist;
                    let ny = dy * inv_dist;
                    let nz = dz * inv_dist;
                    let penetration = min_dist - dist;

                    let restitution = (self.bodies[i].body.restitution + self.bodies[j].body.restitution) * 0.5;
                    let rel_vx = self.bodies[j].body.velocity.x - self.bodies[i].body.velocity.x;
                    let rel_vy = self.bodies[j].body.velocity.y - self.bodies[i].body.velocity.y;
                    let rel_vz = self.bodies[j].body.velocity.z - self.bodies[i].body.velocity.z;
                    let vel_along = rel_vx * nx + rel_vy * ny + rel_vz * nz;

                    if vel_along < 0.0 {
                        let impulse = -(1.0 + restitution) * vel_along;
                        let inv_mass_sum = self.bodies[i].body.inv_mass + self.bodies[j].body.inv_mass;
                        if inv_mass_sum > 0.0 {
                            let imp_mag = impulse / inv_mass_sum;

                            let imp_x = imp_mag * nx;
                            let imp_y = imp_mag * ny;
                            let imp_z = imp_mag * nz;

                            self.bodies[i].body.velocity.x -= imp_x * self.bodies[i].body.inv_mass;
                            self.bodies[i].body.velocity.y -= imp_y * self.bodies[i].body.inv_mass;
                            self.bodies[i].body.velocity.z -= imp_z * self.bodies[i].body.inv_mass;

                            self.bodies[j].body.velocity.x += imp_x * self.bodies[j].body.inv_mass;
                            self.bodies[j].body.velocity.y += imp_y * self.bodies[j].body.inv_mass;
                            self.bodies[j].body.velocity.z += imp_z * self.bodies[j].body.inv_mass;
                        }
                    }

                    // Коррекция позиций (разделяем)
                    let correction = penetration * 0.5;
                    self.bodies[i].body.position.x -= nx * correction;
                    self.bodies[i].body.position.y -= ny * correction;
                    self.bodies[i].body.position.z -= nz * correction;
                    self.bodies[j].body.position.x += nx * correction;
                    self.bodies[j].body.position.y += ny * correction;
                    self.bodies[j].body.position.z += nz * correction;

                    collisions += 1;
                }
            }
        }

        self.stats.collisions_detected = collisions;
    }

    // ──────────────────────────────────────────────────────────────
    // ПОЛУЧЕНИЕ СТАТИСТИКИ
    // ──────────────────────────────────────────────────────────────

    #[inline(always)]
    pub fn get_stats(&self) -> &PhysicsStats {
        &self.stats
    }

    #[inline(always)]
    pub fn reset_stats(&mut self) {
        self.stats = PhysicsStats::default();
    }

    // ──────────────────────────────────────────────────────────────
    // НАСТРОЙКИ
    // ──────────────────────────────────────────────────────────────

    #[inline(always)]
    pub fn set_gravity(&mut self, gravity: Vector3) {
        self.gravity = gravity;
    }

    #[inline(always)]
    pub fn set_parallel_threshold(&mut self, threshold: usize) {
        self.parallel_threshold = threshold;
    }

    // ──────────────────────────────────────────────────────────────
    // ОЧИСТКА
    // ──────────────────────────────────────────────────────────────

    #[inline(always)]
    pub fn clear(&mut self) {
        self.bodies.clear();
        self.stats = PhysicsStats::default();
    }

    #[inline(always)]
    pub fn reserve(&mut self, additional: usize) {
        self.bodies.reserve(additional);
    }

    // ──────────────────────────────────────────────────────────────
    // СНЯТИЕ СНА
    // ──────────────────────────────────────────────────────────────

    #[inline(always)]
    pub fn wake_all(&mut self) {
        for body in &mut self.bodies {
            body.body.is_asleep = false;
            body.body.sleep_timer = 0.0;
        }
    }
}

// ──────────────────────────────────────────────────────────────────
// DEFAULT IMPLEMENTATION
// ──────────────────────────────────────────────────────────────────

impl Default for PhysicsWorld {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────
// ТЕСТЫ
// ──────────────────────────────────────────────────────────────────

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
    fn test_update_parallel() {
        let mut world = PhysicsWorld::with_capacity(1000);
        for i in 0..1000 {
            let body = RigidBody::new(1.0, Vector3::new(i as f32, 10.0, 0.0));
            world.add_body(body);
        }

        world.update(1.0 / 60.0);
        let stats = world.get_stats();
        assert_eq!(stats.bodies_count, 1000);
    }

    #[test]
    fn test_collisions() {
        let mut world = PhysicsWorld::with_capacity(10).with_collisions(true);

        let body1 = RigidBody::new(1.0, Vector3::new(0.0, 0.0, 0.0));
        let body2 = RigidBody::new(1.0, Vector3::new(0.8, 0.0, 0.0));

        world.add_body(body1);
        world.add_body(body2);

        world.update(1.0 / 60.0);

        let stats = world.get_stats();
        assert!(stats.collisions_detected > 0);
    }

    #[test]
    fn test_gravity() {
        let mut world = PhysicsWorld::new();
        let mut body = RigidBody::new(1.0, Vector3::new(0.0, 10.0, 0.0));
        body.velocity = Vector3::zeros();
        world.add_body(body);

        world.update(1.0);

        let body = world.get_body(0).unwrap();
        assert!(body.position.y < 10.0);
    }
}