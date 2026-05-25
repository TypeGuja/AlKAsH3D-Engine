// src/collision.rs - оптимизированная версия
#![allow(dead_code)]

use crate::{Vector3, RigidBody};

#[derive(Debug, Clone, Copy)]
pub struct CollisionManifold {
    pub body_a: u32,
    pub body_b: u32,
    pub normal: Vector3,
    pub penetration: f32,
    pub contact_points: [Vector3; 4],
    pub contact_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub body_a: u32,
    pub body_b: u32,
    pub point: Vector3,
    pub normal: Vector3,
    pub penetration: f32,
}

impl CollisionManifold {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            body_a: 0,
            body_b: 0,
            normal: Vector3::zeros(),
            penetration: 0.0,
            contact_points: [Vector3::zeros(); 4],
            contact_count: 0,
        }
    }
}

pub struct CollisionDetector;

impl CollisionDetector {
    #[inline(always)]
    pub fn new() -> Self {
        Self
    }

    /// Быстрая проверка коллизии сфер (без квадратного корня если возможно)
    #[inline(always)]
    pub fn sphere_sphere_fast(
        pos_a: Vector3, radius_a: f32,
        pos_b: Vector3, radius_b: f32,
    ) -> Option<CollisionManifold> {
        let dx = pos_b.x - pos_a.x;
        let dy = pos_b.y - pos_a.y;
        let dz = pos_b.z - pos_a.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        let radius_sum = radius_a + radius_b;

        if dist_sq < radius_sum * radius_sum {
            let distance = if dist_sq > 0.0 { dist_sq.sqrt() } else { 0.001 };
            let inv_dist = 1.0 / distance;
            let nx = dx * inv_dist;
            let ny = dy * inv_dist;
            let nz = dz * inv_dist;
            let penetration = radius_sum - distance;

            Some(CollisionManifold {
                body_a: 0,
                body_b: 0,
                normal: Vector3::new(nx, ny, nz),
                penetration,
                contact_points: [Vector3::new(pos_a.x + nx * radius_a, pos_a.y + ny * radius_a, pos_a.z + nz * radius_a), Vector3::zeros(), Vector3::zeros(), Vector3::zeros()],
                contact_count: 1,
            })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn sphere_sphere_with_radii(
        body_a: &RigidBody, radius_a: f32,
        body_b: &RigidBody, radius_b: f32,
    ) -> Option<Contact> {
        let dx = body_b.position.x - body_a.position.x;
        let dy = body_b.position.y - body_a.position.y;
        let dz = body_b.position.z - body_a.position.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        let radius_sum = radius_a + radius_b;

        if dist_sq < radius_sum * radius_sum {
            let distance = if dist_sq > 0.0 { dist_sq.sqrt() } else { 0.001 };
            let inv_dist = 1.0 / distance;
            let nx = dx * inv_dist;
            let ny = dy * inv_dist;
            let nz = dz * inv_dist;
            let penetration = radius_sum - distance;

            Some(Contact {
                body_a: body_a.id,
                body_b: body_b.id,
                point: Vector3::new(body_a.position.x + nx * radius_a, body_a.position.y + ny * radius_a, body_a.position.z + nz * radius_a),
                normal: Vector3::new(nx, ny, nz),
                penetration,
            })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn sphere_aabb(
        sphere_pos: Vector3, sphere_radius: f32,
        box_min: Vector3, box_max: Vector3,
    ) -> Option<CollisionManifold> {
        let closest_x = sphere_pos.x.max(box_min.x).min(box_max.x);
        let closest_y = sphere_pos.y.max(box_min.y).min(box_max.y);
        let closest_z = sphere_pos.z.max(box_min.z).min(box_max.z);

        let dx = closest_x - sphere_pos.x;
        let dy = closest_y - sphere_pos.y;
        let dz = closest_z - sphere_pos.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;

        if dist_sq < sphere_radius * sphere_radius {
            let distance = dist_sq.sqrt();
            let inv_dist = 1.0 / distance;
            let nx = dx * inv_dist;
            let ny = dy * inv_dist;
            let nz = dz * inv_dist;

            Some(CollisionManifold {
                body_a: 0,
                body_b: 0,
                normal: Vector3::new(nx, ny, nz),
                penetration: sphere_radius - distance,
                contact_points: [Vector3::new(closest_x, closest_y, closest_z), Vector3::zeros(), Vector3::zeros(), Vector3::zeros()],
                contact_count: 1,
            })
        } else {
            None
        }
    }
}