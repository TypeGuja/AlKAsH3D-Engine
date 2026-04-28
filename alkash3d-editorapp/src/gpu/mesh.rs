// src/gpu/mesh.rs
use wgpu::*;
use crate::math::{Vec3, Transform};

pub struct GpuMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,
    pub index_count: u32,
    pub visible: bool,
    pub material_index: usize,
}

impl GpuMesh {
    pub fn new(device: &Device, queue: &Queue, vertices: &[Vec3], indices: &[u32], normals: &[Vec3], _transform: Transform, _has_normals: bool) -> Self {
        let n = if _has_normals { normals.to_vec() } else { vec![Vec3::UP; vertices.len()] };
        let mut vertex_data = Vec::new();
        for i in 0..vertices.len() {
            vertex_data.extend_from_slice(&[vertices[i].x, vertices[i].y, vertices[i].z, n[i].x, n[i].y, n[i].z, 0.0f32, 0.0]);
        }
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("VB"), size: (vertex_data.len() * 4) as u64, usage: BufferUsages::VERTEX, mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&vertex_data));
        let index_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("IB"), size: (indices.len() * 4) as u64, usage: BufferUsages::INDEX, mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(indices));
        Self { vertex_buffer, index_buffer, index_count: indices.len() as u32, visible: true, material_index: 0 }
    }
}