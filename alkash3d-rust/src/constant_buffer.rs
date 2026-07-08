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

    /// Размер ОДНОГО слота в "массиве" константного буфера, выровненный
    /// на 256 байт — таково требование D3D12 к CBV (`D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT`).
    pub fn aligned_size() -> u64 {
        let raw = std::mem::size_of::<Self>() as u64;
        (raw + 255) & !255
    }

    /// Записывает данные ИМЕННО в слот `slot` внутри буфера, который
    /// должен вмещать как минимум `(slot + 1) * aligned_size()` байт (см.
    /// `Buffer::create_constant_buffer_array` и
    /// `AlkashEngine::ensure_constant_buffer_capacity`).
    ///
    /// ВАЖНО, почему это вообще нужно (а не просто писать в один и тот же
    /// адрес перед каждым Draw, как было раньше): GPU видит содержимое
    /// константного буфера НА МОМЕНТ, КОГДА РЕАЛЬНО ВЫПОЛНЯЕТ Draw — а не
    /// на момент, когда CPU туда что-то записал во время построения
    /// command list'а. CPU успевает выполнить ВСЕ свои записи (Map/Unmap)
    /// для целого кадра ещё ДО того, как ExecuteCommandLists вообще
    /// отправляет что-либо на GPU. Значит, если каждый объект кадра пишет
    /// свою трансформацию в ОДИН и тот же адрес, к моменту, когда GPU
    /// реально начнёт выполнять command list, в буфере будут лежать
    /// данные только ПОСЛЕДНЕГО записанного объекта — и КАЖДЫЙ Draw в
    /// кадре отрисуется с одной и той же (последней) трансформацией. Со
    /// стороны это выглядит так, будто все объекты "слиплись" в один и
    /// двигаются/вращаются синхронно — именно это и происходило с полом и
    /// кубом. Раздельные слоты решают проблему: у каждого Draw — свой
    /// адрес константного буфера с ЕГО собственными данными.
    pub fn write_at(&self, buffer: &Buffer, slot: usize) -> Result<()> {
        let offset = slot as u64 * Self::aligned_size();
        let data = unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        };
        buffer.update_constant_buffer_at(offset, data)
    }

    /// GPU-адрес для чтения слота `slot` — передаётся напрямую в
    /// `SetGraphicsRootConstantBufferView`.
    pub fn gpu_address_for_slot(buffer: &Buffer, slot: usize) -> u64 {
        unsafe { buffer.resource.GetGPUVirtualAddress() + slot as u64 * Self::aligned_size() }
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
