// src/solver.rs - оптимизированная версия
use crate::{Vector3, RigidBody, Contact};

pub struct SequentialImpulseSolver {
    pub iterations: u32,
    pub position_iterations: u32,
    pub use_warm_starting: bool,
}

impl SequentialImpulseSolver {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            iterations: 8,
            position_iterations: 4,
            use_warm_starting: true,
        }
    }

    #[inline(always)]
    pub fn solve_contacts(&mut self, bodies: &mut [RigidBody], contacts: &[Contact]) {
        let iterations = self.iterations as usize;
        let pos_iterations = self.position_iterations as usize;

        for iteration in 0..iterations + pos_iterations {
            let is_position = iteration >= iterations;
            let bias = if is_position { 0.2 } else { 0.0 };

            for contact in contacts {
                let idx_a = contact.body_a as usize;
                let idx_b = contact.body_b as usize;

                if idx_a >= bodies.len() || idx_b >= bodies.len() || idx_a == idx_b {
                    continue;
                }

                if idx_a < idx_b {
                    let (left, right) = bodies.split_at_mut(idx_b);
                    let body_a = &mut left[idx_a];
                    let body_b = &mut right[0];
                    Self::resolve_contact(body_a, body_b, contact, bias);
                } else {
                    let (left, right) = bodies.split_at_mut(idx_a);
                    let body_a = &mut right[0];
                    let body_b = &mut left[idx_b];
                    Self::resolve_contact(body_a, body_b, contact, bias);
                }
            }
        }
    }

    #[inline(always)]
    fn resolve_contact(body_a: &mut RigidBody, body_b: &mut RigidBody, contact: &Contact, bias: f32) {
        let nx = contact.normal.x;
        let ny = contact.normal.y;
        let nz = contact.normal.z;

        // Относительная скорость
        let rel_vx = body_b.velocity.x - body_a.velocity.x;
        let rel_vy = body_b.velocity.y - body_a.velocity.y;
        let rel_vz = body_b.velocity.z - body_a.velocity.z;
        let vel_along = rel_vx * nx + rel_vy * ny + rel_vz * nz;

        if vel_along < 0.0 {
            let restitution = (body_a.restitution + body_b.restitution) * 0.5;
            let impulse_magnitude = -(1.0 + restitution) * vel_along;
            let inv_mass_sum = body_a.inv_mass + body_b.inv_mass;

            if inv_mass_sum > 0.0 {
                let impulse = impulse_magnitude / inv_mass_sum;

                let imp_x = nx * impulse;
                let imp_y = ny * impulse;
                let imp_z = nz * impulse;

                body_a.velocity.x -= imp_x * body_a.inv_mass;
                body_a.velocity.y -= imp_y * body_a.inv_mass;
                body_a.velocity.z -= imp_z * body_a.inv_mass;

                body_b.velocity.x += imp_x * body_b.inv_mass;
                body_b.velocity.y += imp_y * body_b.inv_mass;
                body_b.velocity.z += imp_z * body_b.inv_mass;
            }
        }

        // Коррекция позиций
        if bias > 0.0 {
            let correction = contact.penetration * 0.5 * bias;
            let corr_x = nx * correction;
            let corr_y = ny * correction;
            let corr_z = nz * correction;

            body_a.position.x -= corr_x;
            body_a.position.y -= corr_y;
            body_a.position.z -= corr_z;

            body_b.position.x += corr_x;
            body_b.position.y += corr_y;
            body_b.position.z += corr_z;
        }
    }
}