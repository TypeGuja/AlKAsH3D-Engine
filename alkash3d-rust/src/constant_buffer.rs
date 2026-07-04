// src/constant_buffer.rs
//! Константный буфер для матриц трансформации

use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use crate::{Buffer, STATE};

/// ИСПРАВЛЕНО: добавлен `#[repr(C)]`. Эта структура копируется побайтово
/// в GPU constant buffer, а шейдер (`cbuffer TransformConstants : register(b0)`)
/// ожидает конкретный, стабильный порядок полей в памяти. Без `#[repr(C)]`
/// компилятор формально не обязан сохранять порядок полей структуры — на
/// практике для "плоских" POD-структур это обычно совпадает, но полагаться
/// на совпадение по умолчанию для чего-то, что мапится на GPU layout,
/// нельзя.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TransformConstants {
    pub model_view_proj: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub ambient_color: [f32; 4],
}

impl TransformConstants {
    pub fn new() -> Self {
        Self {
            model_view_proj: [[0.0; 4]; 4],
            model: [[0.0; 4]; 4],
            view: [[0.0; 4]; 4],
            proj: [[0.0; 4]; 4],
            camera_pos: [0.0, 0.0, 0.0, 0.0],
            light_dir: [0.0, -1.0, 0.0, 0.0],
            light_color: [1.0, 1.0, 1.0, 1.0],
            ambient_color: [0.1, 0.1, 0.15, 1.0],
        }
    }

    pub fn create_buffer() -> Result<Buffer> {
        let size = std::mem::size_of::<Self>() as u64;
        Buffer::create_constant_buffer(size)
    }

    pub fn update(&self, buffer: &Buffer) -> Result<()> {
        let data = unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        };
        buffer.update_constant_buffer(data)
    }
}