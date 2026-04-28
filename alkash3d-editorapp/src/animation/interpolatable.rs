use crate::math::{Vec3, Quat};

pub trait Interpolatable {
    fn interpolate(&self, other: &Self, t: f32) -> Self;
}

impl Interpolatable for Vec3 {
    fn interpolate(&self, other: &Self, t: f32) -> Self { self.lerp(*other, t) }
}

impl Interpolatable for Quat {
    fn interpolate(&self, other: &Self, t: f32) -> Self { self.slerp(other, t) }
}

impl Interpolatable for f32 {
    fn interpolate(&self, other: &Self, t: f32) -> Self { self + (other - self) * t }
}