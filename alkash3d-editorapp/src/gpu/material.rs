// src/gpu/material.rs
use wgpu::*;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaterialUniform {
    pub albedo: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub ao: f32,
}

pub struct GpuMaterial {
    pub buffer: Buffer,
    pub bind_group: BindGroup,
}

impl GpuMaterial {
    pub fn new(device: &Device, queue: &Queue, color: [f32; 4], metallic: f32, roughness: f32) -> Self {
        let uniform = MaterialUniform { albedo: color, metallic, roughness, ao: 1.0 };
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("MatBuf"), size: std::mem::size_of::<MaterialUniform>() as u64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&[uniform]));
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("MatLayout"), entries: &[BindGroupLayoutEntry { binding: 0, visibility: ShaderStages::FRAGMENT, ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("MatBG"), layout: &layout, entries: &[BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        Self { buffer, bind_group }
    }
}