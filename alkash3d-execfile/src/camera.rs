// camera.rs
use crate::math::Vec3;

pub struct Camera {
    pub position: Vec3,
}

impl Camera {
    pub fn new(position: Vec3) -> Self {
        Self { position }
    }
}

pub struct CameraController;