//! Система коллизий

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

/// Детектор коллизий
pub struct CollisionDetector {
    /// GJK кэш для оптимизации
    gjk_cache: Vec<GJKCache>,
}

struct GJKCache {
    simplex: Vec<Vector3>,
    last_support: Vector3,
}

impl CollisionDetector {
    pub fn new() -> Self {
        Self {
            gjk_cache: Vec::new(),
        }
    }

    /// Проверка коллизии между двумя сферами (быстро)
    pub fn sphere_sphere(
        pos_a: Vector3, radius_a: f32,
        pos_b: Vector3, radius_b: f32,
    ) -> Option<CollisionManifold> {
        let delta = pos_b - pos_a;
        let distance = delta.magnitude();
        let radius_sum = radius_a + radius_b;

        if distance < radius_sum {
            let normal = if distance > 0.0 {
                delta / distance
            } else {
                Vector3::new(1.0, 0.0, 0.0)
            };

            let penetration = radius_sum - distance;

            let mut manifold = CollisionManifold::new();
            manifold.normal = normal;
            manifold.penetration = penetration;
            manifold.contact_points[0] = pos_a + normal * radius_a;
            manifold.contact_count = 1;

            Some(manifold)
        } else {
            None
        }
    }

    /// Проверка коллизии сфера-бокс
    pub fn sphere_aabb(
        sphere_pos: Vector3, sphere_radius: f32,
        box_min: Vector3, box_max: Vector3,
    ) -> Option<CollisionManifold> {
        // Находим ближайшую точку на AABB к сфере
        let closest = Vector3::new(
            sphere_pos.x.max(box_min.x).min(box_max.x),
            sphere_pos.y.max(box_min.y).min(box_max.y),
            sphere_pos.z.max(box_min.z).min(box_max.z),
        );

        let delta = closest - sphere_pos;
        let distance_sq = delta.magnitude_squared();

        if distance_sq < sphere_radius * sphere_radius {
            let distance = distance_sq.sqrt();
            let normal = if distance > 0.0 {
                delta / distance
            } else {
                Vector3::new(1.0, 0.0, 0.0)
            };

            let mut manifold = CollisionManifold::new();
            manifold.normal = normal;
            manifold.penetration = sphere_radius - distance;
            manifold.contact_points[0] = closest;
            manifold.contact_count = 1;

            Some(manifold)
        } else {
            None
        }
    }

    /// Проверка AABB-AABB (быстро)
    pub fn aabb_aabb(
        min_a: Vector3, max_a: Vector3,
        min_b: Vector3, max_b: Vector3,
    ) -> Option<CollisionManifold> {
        if max_a.x < min_b.x || max_b.x < min_a.x { return None; }
        if max_a.y < min_b.y || max_b.y < min_a.y { return None; }
        if max_a.z < min_b.z || max_b.z < min_a.z { return None; }

        // Вычисляем глубину проникновения по каждой оси
        let dx = (max_a.x - min_b.x).min(max_b.x - min_a.x);
        let dy = (max_a.y - min_b.y).min(max_b.y - min_a.y);
        let dz = (max_a.z - min_b.z).min(max_b.z - min_a.z);

        // Находим минимальную ось для нормали
        let mut manifold = CollisionManifold::new();

        if dx < dy && dx < dz {
            let center_a = (min_a.x + max_a.x) * 0.5;
            let center_b = (min_b.x + max_b.x) * 0.5;
            manifold.normal = if center_a < center_b {
                Vector3::new(1.0, 0.0, 0.0)
            } else {
                Vector3::new(-1.0, 0.0, 0.0)
            };
            manifold.penetration = dx;
        } else if dy < dz {
            let center_a = (min_a.y + max_a.y) * 0.5;
            let center_b = (min_b.y + max_b.y) * 0.5;
            manifold.normal = if center_a < center_b {
                Vector3::new(0.0, 1.0, 0.0)
            } else {
                Vector3::new(0.0, -1.0, 0.0)
            };
            manifold.penetration = dy;
        } else {
            let center_a = (min_a.z + max_a.z) * 0.5;
            let center_b = (min_b.z + max_b.z) * 0.5;
            manifold.normal = if center_a < center_b {
                Vector3::new(0.0, 0.0, 1.0)
            } else {
                Vector3::new(0.0, 0.0, -1.0)
            };
            manifold.penetration = dz;
        }

        Some(manifold)
    }
}