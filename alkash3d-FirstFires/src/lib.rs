#![allow(dead_code)]
#![allow(unused_imports)]

mod first_fires;
mod culling;
mod light;
mod grid;
mod stats;

pub use first_fires::*;
pub use light::*;
pub use grid::*;
pub use stats::*;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GPULight {
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub direction: [f32; 4],
    pub params: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightGridCell {
    pub offset: u32,
    pub count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightGridEntry {
    pub light_index: u32,
    pub lod_level: u32,
    pub depth: f32,
    pub padding: u32,
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");