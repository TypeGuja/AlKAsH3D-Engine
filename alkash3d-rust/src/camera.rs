// src/camera.rs
//! Камера для 3D рендеринга

use crate::math::Mat4;

pub struct Camera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
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
            position: [0.0, 0.0, 5.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 100.0,
            aspect: width as f32 / height as f32,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at(self.position, self.target, self.up)
    }

    pub fn projection_matrix(&self) -> Mat4 {
        Mat4::perspective(self.fov, self.aspect, self.near, self.far)
    }

    pub fn set_aspect(&mut self, width: u32, height: u32) {
        self.aspect = width as f32 / height as f32;
    }

    pub fn move_forward(&mut self, distance: f32) {
        let dir = [
            self.target[0] - self.position[0],
            self.target[1] - self.position[1],
            self.target[2] - self.position[2],
        ];
        let len = (dir[0]*dir[0] + dir[1]*dir[1] + dir[2]*dir[2]).sqrt();
        if len > 0.0 {
            self.position[0] += dir[0] / len * distance;
            self.position[1] += dir[1] / len * distance;
            self.position[2] += dir[2] / len * distance;
            self.target[0] += dir[0] / len * distance;
            self.target[1] += dir[1] / len * distance;
            self.target[2] += dir[2] / len * distance;
        }
    }

    pub fn move_right(&mut self, distance: f32) {
        let forward = [
            self.target[0] - self.position[0],
            self.target[1] - self.position[1],
            self.target[2] - self.position[2],
        ];
        let f_len = (forward[0]*forward[0] + forward[1]*forward[1] + forward[2]*forward[2]).sqrt();
        let right = [
            forward[1]*self.up[2] - forward[2]*self.up[1],
            forward[2]*self.up[0] - forward[0]*self.up[2],
            forward[0]*self.up[1] - forward[1]*self.up[0],
        ];
        let r_len = (right[0]*right[0] + right[1]*right[1] + right[2]*right[2]).sqrt();
        if r_len > 0.0 {
            self.position[0] += right[0] / r_len * distance;
            self.position[1] += right[1] / r_len * distance;
            self.position[2] += right[2] / r_len * distance;
            self.target[0] += right[0] / r_len * distance;
            self.target[1] += right[1] / r_len * distance;
            self.target[2] += right[2] / r_len * distance;
        }
    }

    pub fn rotate_yaw(&mut self, angle: f32) {
        self.yaw += angle;
        let dir = [
            self.target[0] - self.position[0],
            self.target[1] - self.position[1],
            self.target[2] - self.position[2],
        ];
        let len = (dir[0]*dir[0] + dir[1]*dir[1] + dir[2]*dir[2]).sqrt();
        let rot = Mat4::rotation_y(angle);
        let new_dir = rot.transform_point(&[dir[0]/len, dir[1]/len, dir[2]/len]);
        self.target[0] = self.position[0] + new_dir[0] * len;
        self.target[1] = self.position[1] + new_dir[1] * len;
        self.target[2] = self.position[2] + new_dir[2] * len;
    }

    pub fn rotate_pitch(&mut self, angle: f32) {
        self.pitch += angle;
        let dir = [
            self.target[0] - self.position[0],
            self.target[1] - self.position[1],
            self.target[2] - self.position[2],
        ];
        let len = (dir[0]*dir[0] + dir[1]*dir[1] + dir[2]*dir[2]).sqrt();
        let rot = Mat4::rotation_x(angle);
        let new_dir = rot.transform_point(&[dir[0]/len, dir[1]/len, dir[2]/len]);
        self.target[0] = self.position[0] + new_dir[0] * len;
        self.target[1] = self.position[1] + new_dir[1] * len;
        self.target[2] = self.position[2] + new_dir[2] * len;
    }
}