// src/world.rs
use crate::{RigidBody, Vector3, AABB, Contact};

pub struct PhysicsWorld {
    bodies: Vec<RigidBody>,
    static_bodies: Vec<RigidBody>,
    gravity: Vector3,
    dt_fixed: f32,
    accumulator: f32,
    iterations: u32,
    max_bodies: usize,
    stats: PhysicsStats,
}

#[derive(Default)]
pub struct PhysicsStats {
    pub bodies_count: usize,
    pub active_bodies: usize,
    pub collisions_detected: u32,
    pub solver_iterations: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            bodies: Vec::new(),
            static_bodies: Vec::new(),
            gravity: Vector3::new(0.0, -9.81, 0.0),
            dt_fixed: 1.0 / 60.0,
            accumulator: 0.0,
            iterations: 8,
            max_bodies: 10000,
            stats: PhysicsStats::default(),
        }
    }

    pub fn add_body(&mut self, body: RigidBody) -> u32 {
        self.bodies.push(body);
        (self.bodies.len() - 1) as u32
    }

    pub fn update(&mut self, dt: f32) {
        self.step(dt);
    }

    fn step(&mut self, dt: f32) {
        // Применяем силы
        for body in &mut self.bodies {
            if !body.is_asleep {
                body.apply_force_center(self.gravity * body.mass);
            }
        }

        // Решаем коллизии (упрощённо)
        self.solve_collisions();

        // Интеграция
        for body in &mut self.bodies {
            if !body.is_asleep && !body.is_static {
                body.velocity += body.acceleration * dt;
                body.position += body.velocity * dt;
                body.force_accumulator = Vector3::zeros();
                body.torque_accumulator = Vector3::zeros();
            }
        }

        self.stats.bodies_count = self.bodies.len();
        self.stats.active_bodies = self.bodies.iter().filter(|b| !b.is_asleep).count();
    }

    fn solve_collisions(&mut self) {
        // Простая проверка коллизий
        for i in 0..self.bodies.len() {
            for j in i+1..self.bodies.len() {
                let delta = self.bodies[j].position - self.bodies[i].position;
                let distance = delta.magnitude();
                let min_distance = 1.0; // Примерный радиус

                if distance < min_distance {
                    let normal = delta.normalize();
                    let penetration = min_distance - distance;

                    // Раздвигаем тела
                    let correction = normal * (penetration * 0.5);
                    self.bodies[i].position -= correction;
                    self.bodies[j].position += correction;

                    self.stats.collisions_detected += 1;
                }
            }
        }
    }

    pub fn get_stats(&self) -> &PhysicsStats {
        &self.stats
    }
}