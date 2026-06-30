// src/camera.rs
//! Камера для 3D рендеринга

use crate::math::{Mat4, Vec3, look_at, perspective, rotation_x, rotation_y};

pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Camera {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 5.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 100.0,
            aspect: width as f32 / height as f32,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        look_at(self.position, self.target, self.up)
    }

    pub fn projection_matrix(&self) -> Mat4 {
        perspective(self.fov, self.aspect, self.near, self.far)
    }

    pub fn set_aspect(&mut self, width: u32, height: u32) {
        self.aspect = width as f32 / height as f32;
    }

    pub fn move_forward(&mut self, distance: f32) {
        let dir = (self.target - self.position).normalize();
        self.position += dir * distance;
        self.target += dir * distance;
    }

    pub fn move_right(&mut self, distance: f32) {
        let forward = (self.target - self.position).normalize();
        let right = forward.cross(self.up).normalize();
        self.position += right * distance;
        self.target += right * distance;
    }

    pub fn rotate_yaw(&mut self, angle: f32) {
        self.yaw += angle;
        let dir = (self.target - self.position).normalize();
        let rot = rotation_y(angle);
        let new_dir = rot.transform_vector3(dir);
        self.target = self.position + new_dir * (self.target - self.position).length();
    }

    pub fn rotate_pitch(&mut self, angle: f32) {
        self.pitch += angle;
        let dir = (self.target - self.position).normalize();
        let rot = rotation_x(angle);
        let new_dir = rot.transform_vector3(dir);
        self.target = self.position + new_dir * (self.target - self.position).length();
    }

    // Для совместимости со старым кодом (работа с массивами)
    pub fn position_array(&self) -> [f32; 3] {
        [self.position.x, self.position.y, self.position.z]
    }

    pub fn target_array(&self) -> [f32; 3] {
        [self.target.x, self.target.y, self.target.z]
    }
}