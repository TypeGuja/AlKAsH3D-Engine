// src/gpu/camera.rs
use wgpu::*;
use crate::math::Vec3;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub view_position: [f32; 3],
    pub _padding: f32,
}

pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub buffer: Option<Buffer>,
    pub bind_group: Option<BindGroup>,
    pub uniform: CameraUniform,
}

impl Camera {
    pub fn new(aspect: f32) -> Self {
        let position = Vec3::new(5.0, 5.0, 10.0);
        let target = Vec3::ZERO;
        let view = Self::calculate_view_matrix(position, target, Vec3::UP);
        let proj = Self::calculate_projection_matrix(60.0_f32.to_radians(), aspect, 0.1, 1000.0);
        let view_proj = Self::multiply_matrices(proj, view);
        Self {
            position, target, up: Vec3::UP,
            fov: 60.0_f32.to_radians(), aspect, near: 0.1, far: 1000.0,
            buffer: None, bind_group: None,
            uniform: CameraUniform {
                view_proj,
                view_position: [position.x, position.y, position.z],
                _padding: 0.0,
            },
        }
    }

    pub fn update_view_matrix(&mut self) {
        let view = Self::calculate_view_matrix(self.position, self.target, self.up);
        let proj = Self::calculate_projection_matrix(self.fov, self.aspect, self.near, self.far);
        self.uniform.view_proj = Self::multiply_matrices(proj, view);
        self.uniform.view_position = [self.position.x, self.position.y, self.position.z];
    }

    pub fn update_bind_group(&mut self, device: &Device, queue: &Queue) {
        if self.buffer.is_none() {
            self.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Camera Buffer"),
                size: std::mem::size_of::<CameraUniform>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(ref buffer) = self.buffer {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[self.uniform]));
        }
    }

    fn calculate_view_matrix(pos: Vec3, target: Vec3, up: Vec3) -> [[f32; 4]; 4] {
        let f = (target - pos).normalize();
        let s = f.cross(up).normalize();
        let u = s.cross(f);
        [
            [s.x, u.x, -f.x, 0.0],
            [s.y, u.y, -f.y, 0.0],
            [s.z, u.z, -f.z, 0.0],
            [-s.dot(pos), -u.dot(pos), f.dot(pos), 1.0],
        ]
    }

    fn calculate_projection_matrix(fov: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
        let f = 1.0 / (fov / 2.0).tan();
        [
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, (far + near) / (near - far), -1.0],
            [0.0, 0.0, (2.0 * far * near) / (near - far), 0.0],
        ]
    }

    fn multiply_matrices(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
        let mut result = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    result[i][j] += a[i][k] * b[k][j];
                }
            }
        }
        result
    }
}