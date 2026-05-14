// src/math.rs
use nalgebra as na;
use crate::{Vector3, Point3, Matrix4};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub position: Vector3,
    pub rotation: na::UnitQuaternion<f32>,
    pub scale: Vector3,
}

impl Transform {
    pub fn new(position: Vector3, rotation: na::UnitQuaternion<f32>, scale: Vector3) -> Self {
        Self { position, rotation, scale }
    }

    pub fn identity() -> Self {
        Self {
            position: Vector3::zeros(),
            rotation: na::UnitQuaternion::identity(),
            scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn to_matrix(&self) -> Matrix4 {
        let rot = self.rotation.to_rotation_matrix();
        let mut mat = Matrix4::identity();

        mat[(0,0)] = rot[(0,0)] * self.scale.x;
        mat[(0,1)] = rot[(0,1)] * self.scale.y;
        mat[(0,2)] = rot[(0,2)] * self.scale.z;
        mat[(0,3)] = self.position.x;

        mat[(1,0)] = rot[(1,0)] * self.scale.x;
        mat[(1,1)] = rot[(1,1)] * self.scale.y;
        mat[(1,2)] = rot[(1,2)] * self.scale.z;
        mat[(1,3)] = self.position.y;

        mat[(2,0)] = rot[(2,0)] * self.scale.x;
        mat[(2,1)] = rot[(2,1)] * self.scale.y;
        mat[(2,2)] = rot[(2,2)] * self.scale.z;
        mat[(2,3)] = self.position.z;

        mat
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AABB {
    pub min: Point3,
    pub max: Point3,
}

impl AABB {
    pub fn new(min: Point3, max: Point3) -> Self {
        Self { min, max }
    }

    pub fn from_points(points: &[Point3]) -> Self {
        let mut min = Point3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Point3::new(f32::MIN, f32::MIN, f32::MIN);

        for p in points {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
        }

        Self { min, max }
    }

    pub fn intersects(&self, other: &AABB) -> bool {
        self.min.x <= other.max.x && self.max.x >= other.min.x &&
            self.min.y <= other.max.y && self.max.y >= other.min.y &&
            self.min.z <= other.max.z && self.max.z >= other.min.z
    }
}