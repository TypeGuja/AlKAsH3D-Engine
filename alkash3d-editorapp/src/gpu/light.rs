// src/gpu/light.rs
use wgpu::*;
use crate::math::Vec3;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightUniform {
    pub position: [f32; 3],
    pub intensity: f32,
    pub color: [f32; 3],
    pub range: f32,
}

pub struct GpuLight {
    pub uniform: LightUniform,
    pub buffer: Buffer,
}

impl GpuLight {
    pub fn new(device: &Device, queue: &Queue, position: Vec3, color: [f32; 3], intensity: f32, range: f32) -> Self {
        let uniform = LightUniform { position: [position.x, position.y, position.z], intensity, color, range };
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("LightBuf"), size: std::mem::size_of::<LightUniform>() as u64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&[uniform]));
        Self { uniform, buffer }
    }
}