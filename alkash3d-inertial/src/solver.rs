//! Sequential Impulse Solver
// src/solver.rs
use crate::{Vector3, RigidBody, Contact};

pub struct SequentialImpulseSolver {
    pub iterations: u32,
    pub position_iterations: u32,
    pub use_warm_starting: bool,
}

impl SequentialImpulseSolver {
    pub fn new() -> Self {
        Self {
            iterations: 8,
            position_iterations: 4,
            use_warm_starting: true,
        }
    }

    pub fn solve_contacts(&mut self, bodies: &mut [RigidBody], contacts: &[Contact]) {
        for _ in 0..self.iterations {
            for contact in contacts {
                // Упрощённое решение
                if contact.body_a < bodies.len() as u32 && contact.body_b < bodies.len() as u32 {
                    let idx_a = contact.body_a as usize;
                    let idx_b = contact.body_b as usize;

                    if idx_a == idx_b { continue; }

                    // Разделяем mutable ссылки
                    if idx_a < idx_b {
                        let (left, right) = bodies.split_at_mut(idx_b);
                        let body_a = &mut left[idx_a];
                        let body_b = &mut right[0];

                        let rel_vel = body_b.velocity - body_a.velocity;
                        let vel_along = rel_vel.dot(&contact.normal);

                        if vel_along < 0.0 {
                            let restitution = (body_a.restitution + body_b.restitution) * 0.5;
                            let impulse = -(1.0 + restitution) * vel_along;
                            let inv_mass_sum = body_a.inv_mass + body_b.inv_mass;
                            let impulse_magnitude = impulse / inv_mass_sum;

                            let impulse_vec = contact.normal * impulse_magnitude;

                            body_a.velocity -= impulse_vec * body_a.inv_mass;
                            body_b.velocity += impulse_vec * body_b.inv_mass;
                        }
                    } else {
                        let (left, right) = bodies.split_at_mut(idx_a);
                        let body_a = &mut left[idx_b];
                        let body_b = &mut right[0];

                        let rel_vel = body_b.velocity - body_a.velocity;
                        let vel_along = rel_vel.dot(&contact.normal);

                        if vel_along < 0.0 {
                            let restitution = (body_a.restitution + body_b.restitution) * 0.5;
                            let impulse = -(1.0 + restitution) * vel_along;
                            let inv_mass_sum = body_a.inv_mass + body_b.inv_mass;
                            let impulse_magnitude = impulse / inv_mass_sum;

                            let impulse_vec = contact.normal * impulse_magnitude;

                            body_a.velocity -= impulse_vec * body_a.inv_mass;
                            body_b.velocity += impulse_vec * body_b.inv_mass;
                        }
                    }
                }
            }
        }
    }
}