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

/// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): параметры пространственной
/// сетки, которую FirstFires уже строит внутри себя во время `cull()`
/// (см. `LightState` в alkash3d-FirstFires/src/lib.rs — поля `world_min`,
/// `cell_size`, `grid_width/height/depth`), но раньше НЕ отдавал наружу
/// через ABI. Без этих чисел движок не может сопоставить мировую позицию
/// пикселя (worldPos) с индексом ячейки `LightGridCells`/`LightGridEntries`
/// — то есть не может реально ВОСПОЛЬЗОВАТЬСЯ уже посчитанной сеткой в
/// шейдере, только читать её вслепую.
///
/// Технически эти параметры детерминированно выводятся из LightConfig
/// (far_plane/grid_cell_size), и движок мог бы просто продублировать ту
/// же формулу у себя — но это создало бы риск молчаливого рассинхрона,
/// если формула в FirstFires когда-нибудь изменится, а копия в движке
/// нет. Явный геттер — единственный источник истины.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightGridParams {
    pub world_min: [f32; 3],
    pub cell_size: f32,
    pub world_max: [f32; 3],
    pub grid_width: u32,
    pub grid_height: u32,
    pub grid_depth: u32,
    pub _padding: u32,
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

    // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): см. LightGridParams
    // выше. Поле добавлено В КОНЕЦ структуры (не в середину) — порядок
    // полей #[repr(C)] задаёт ABI layout, вставка в середину сдвинула бы
    // смещения всех последующих fn-указателей и молча сломала бы уже
    // скомпилированный firstfires.dll, если бы движок и плагин собрались
    // не одновременно.
    pub get_grid_params: extern "C" fn(instance: *mut c_void) -> LightGridParams,
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