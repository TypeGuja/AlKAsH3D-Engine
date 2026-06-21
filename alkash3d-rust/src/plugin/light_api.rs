// src/plugin/light_api.rs
//! API для плагина Light Culling

use std::ffi::c_void;

/// Конфигурация света
#[repr(C)]
pub struct LightConfig {
    pub max_lights: u32,
    pub tile_size: u32,
    pub far_plane: f32,
    pub lod_distances: [f32; 3],
    pub grid_cell_size: f32,
}

/// GPU-совместимая структура света
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GPULight {
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub direction: [f32; 4],
    pub params: [f32; 4],
}

/// Ячейка световой сетки
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightGridCell {
    pub offset: u32,
    pub count: u32,
}

/// Запись в сетке
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightGridEntry {
    pub light_index: u32,
    pub lod_level: u32,
    pub depth: f32,
    pub padding: u32,
}

/// API плагина Light Culling
#[repr(C)]
#[derive(Clone, Copy)]  // <-- Добавлено
pub struct LightAPI {
    // Управление источниками
    pub add_light: extern "C" fn(instance: *mut c_void, light: *const GPULight) -> u32,
    pub remove_light: extern "C" fn(instance: *mut c_void, id: u32),
    pub update_light: extern "C" fn(instance: *mut c_void, id: u32, light: *const GPULight),
    pub get_lights_count: extern "C" fn(instance: *mut c_void) -> u32,

    // Culling
    pub cull: extern "C" fn(instance: *mut c_void, camera_pos: *const f32, view_proj: *const f32, dt: f32),

    // Получение результатов для GPU
    pub get_gpu_lights: extern "C" fn(instance: *mut c_void) -> *const GPULight,
    pub get_gpu_lights_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_light_grid_cells: extern "C" fn(instance: *mut c_void) -> *const LightGridCell,
    pub get_light_grid_entries: extern "C" fn(instance: *mut c_void) -> *const LightGridEntry,
    pub get_grid_cells_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_grid_entries_count: extern "C" fn(instance: *mut c_void) -> u32,

    // Статистика
    pub get_stats: extern "C" fn(instance: *mut c_void) -> LightStats,
}

/// Статистика каллинга
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightStats {
    pub total_lights: u32,
    pub visible_lights: u32,
    pub culled_by_lod: u32,
    pub culled_by_distance: u32,
    pub culled_by_frustum: u32,
    pub culling_time_ms: f32,
}