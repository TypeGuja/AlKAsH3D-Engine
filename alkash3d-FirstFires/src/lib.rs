//! FirstFires - GPU Light Culling System
//! Динамическая библиотека для каллинга источников света

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use nalgebra::{Vector3, Matrix4};
use rayon::prelude::*;

// ===================================================================
// Внутренние модули
// ===================================================================

mod light;
mod culling;
mod grid;
mod stats;

use light::*;
use culling::*;
use grid::*;
use stats::*;

// ===================================================================
// Структуры для Plugin API (совместимые с движком)
// ===================================================================

pub const PLUGIN_API_VERSION: u32 = 1;
pub const PLUGIN_TYPE_LIGHT: u32 = 1;

/// Конфигурация света
#[repr(C)]
#[derive(Debug, Clone, Copy)]
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
    pub position: [f32; 4],  // xyz, type (0=point,1=spot,2=directional)
    pub color: [f32; 4],     // rgb, intensity
    pub direction: [f32; 4], // xyz, range
    pub params: [f32; 4],    // spot_angle, falloff, lod, padding
}

unsafe impl bytemuck::Zeroable for GPULight {}
unsafe impl bytemuck::Pod for GPULight {}

/// Ячейка световой сетки
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
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

/// Статистика каллинга
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LightStats {
    pub total_lights: u32,
    pub visible_lights: u32,
    pub culled_by_lod: u32,
    pub culled_by_distance: u32,
    pub culled_by_frustum: u32,
    pub culling_time_ms: f32,
}

/// API света для движка
#[repr(C)]
pub struct LightAPI {
    pub add_light: extern "C" fn(instance: *mut c_void, light: *const GPULight) -> u32,
    pub remove_light: extern "C" fn(instance: *mut c_void, id: u32),
    pub update_light: extern "C" fn(instance: *mut c_void, id: u32, light: *const GPULight),
    pub get_lights_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub cull: extern "C" fn(instance: *mut c_void, camera_pos: *const f32, view_proj: *const f32, dt: f32),
    pub get_gpu_lights: extern "C" fn(instance: *mut c_void) -> *const GPULight,
    pub get_gpu_lights_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_light_grid_cells: extern "C" fn(instance: *mut c_void) -> *const LightGridCell,
    pub get_light_grid_entries: extern "C" fn(instance: *mut c_void) -> *const LightGridEntry,
    pub get_grid_cells_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_grid_entries_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_stats: extern "C" fn(instance: *mut c_void) -> LightStats,
}

/// Базовый API плагина
#[repr(C)]
pub struct PluginAPI {
    pub version: u32,
    pub plugin_type: u32,
    pub name: *const std::os::raw::c_char,
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

// ===================================================================
// Внутреннее состояние
// ===================================================================

/// Внутренняя структура света
#[derive(Debug, Clone)]
struct InternalLight {
    id: u32,
    position: Vector3<f32>,
    color: Vector3<f32>,
    intensity: f32,
    range: f32,
    light_type: LightType,
    spot_angle: f32,
    spot_direction: Vector3<f32>,
    falloff: f32,
}

impl InternalLight {
    fn to_gpu(&self) -> GPULight {
        GPULight {
            position: [self.position.x, self.position.y, self.position.z, self.light_type as u32 as f32],
            color: [self.color.x, self.color.y, self.color.z, self.intensity],
            direction: [self.spot_direction.x, self.spot_direction.y, self.spot_direction.z, self.range],
            params: [self.spot_angle, self.falloff, 0.0, 0.0],
        }
    }
}

struct LightState {
    lights: Vec<InternalLight>,
    gpu_lights: Vec<GPULight>,
    grid_cells: Vec<LightGridCell>,
    grid_entries: Vec<LightGridEntry>,
    stats: LightStats,
    config: LightConfig,
    next_id: AtomicU32,
    grid_width: u32,
    grid_height: u32,
    grid_depth: u32,
    world_min: Vector3<f32>,
    world_max: Vector3<f32>,
    cell_size: f32,
}

impl LightState {
    fn new(config: &LightConfig) -> Self {
        let world_size = config.far_plane;
        let world_min = Vector3::new(-world_size, -world_size, -world_size);
        let world_max = Vector3::new(world_size, world_size, world_size);
        let cell_size = config.grid_cell_size;

        let size = world_max - world_min;
        let grid_width = (size.x / cell_size).ceil() as u32;
        let grid_height = (size.y / cell_size).ceil() as u32;
        let grid_depth = (size.z / cell_size).ceil() as u32;
        let total_cells = (grid_width * grid_height * grid_depth) as usize;

        Self {
            lights: Vec::with_capacity(config.max_lights as usize),
            gpu_lights: Vec::with_capacity(config.max_lights as usize),
            grid_cells: vec![LightGridCell::default(); total_cells],
            grid_entries: Vec::with_capacity(config.max_lights as usize * 4),
            stats: LightStats::default(),
            config: *config,
            next_id: AtomicU32::new(0),
            grid_width,
            grid_height,
            grid_depth,
            world_min,
            world_max,
            cell_size,
        }
    }

    fn add_light(&mut self, light: &GPULight) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let light_type = match (light.position[3] as u32) {
            0 => LightType::Point,
            1 => LightType::Spot,
            2 => LightType::Directional,
            _ => LightType::Point,
        };

        let internal = InternalLight {
            id,
            position: Vector3::new(light.position[0], light.position[1], light.position[2]),
            color: Vector3::new(light.color[0], light.color[1], light.color[2]),
            intensity: light.color[3],
            range: light.direction[3],
            light_type,
            spot_angle: light.params[0],
            spot_direction: Vector3::new(light.direction[0], light.direction[1], light.direction[2]),
            falloff: light.params[1],
        };

        self.lights.push(internal);
        self.stats.total_lights = self.lights.len() as u32;
        id
    }

    fn get_cell_index(&self, pos: Vector3<f32>) -> Option<usize> {
        let local = pos - self.world_min;

        if local.x < 0.0 || local.y < 0.0 || local.z < 0.0 {
            return None;
        }

        let x = (local.x / self.cell_size) as u32;
        let y = (local.y / self.cell_size) as u32;
        let z = (local.z / self.cell_size) as u32;

        if x >= self.grid_width || y >= self.grid_height || z >= self.grid_depth {
            return None;
        }

        Some((z * self.grid_height * self.grid_width + y * self.grid_width + x) as usize)
    }

    fn cull(&mut self, camera_pos: [f32; 3], view_proj: &[f32; 16], _dt: f32) {
        let start = Instant::now();

        self.gpu_lights.clear();
        self.grid_entries.clear();

        // Очищаем сетку
        for cell in &mut self.grid_cells {
            cell.offset = 0;
            cell.count = 0;
        }

        let camera = Vector3::new(camera_pos[0], camera_pos[1], camera_pos[2]);

        // Создаём frustum из view_proj
        let view_proj_mat = Matrix4::new(
            view_proj[0], view_proj[1], view_proj[2], view_proj[3],
            view_proj[4], view_proj[5], view_proj[6], view_proj[7],
            view_proj[8], view_proj[9], view_proj[10], view_proj[11],
            view_proj[12], view_proj[13], view_proj[14], view_proj[15],
        );
        let frustum = Frustum::from_view_proj(&view_proj_mat);

        let mut culled_lod = 0;
        let mut culled_dist = 0;
        let mut culled_frustum = 0;

        // Параллельный каллинг
        let visible: Vec<(usize, u32, f32, Vector3<f32>)> = self.lights
            .par_iter()
            .enumerate()
            .filter_map(|(idx, light)| {
                let distance = (light.position - camera).magnitude();

                // LOD culling
                let lod = if distance < self.config.lod_distances[0] {
                    0
                } else if distance < self.config.lod_distances[1] {
                    1
                } else if distance < self.config.lod_distances[2] {
                    2
                } else {
                    return None;
                };

                // Distance culling
                if distance > light.range * 1.2 {
                    return None;
                }

                // Frustum culling
                if !frustum.test_sphere(light.position, light.range) {
                    return None;
                }

                Some((idx, lod, distance, light.position))
            })
            .collect();

        // Подсчёт статистики
        for light in &self.lights {
            let distance = (light.position - camera).magnitude();
            if distance >= self.config.lod_distances[2] {
                culled_lod += 1;
            } else if distance > light.range * 1.2 {
                culled_dist += 1;
            } else if !frustum.test_sphere(light.position, light.range) {
                culled_frustum += 1;
            }
        }

        self.stats.culled_by_lod = culled_lod;
        self.stats.culled_by_distance = culled_dist;
        self.stats.culled_by_frustum = culled_frustum;

        // Сортируем по глубине (для правильного освещения)
        let mut visible_sorted = visible;
        visible_sorted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());

        for (idx, lod, depth, position) in visible_sorted {
            let light = &self.lights[idx];
            let light_idx = self.gpu_lights.len() as u32;
            self.gpu_lights.push(light.to_gpu());

            if let Some(cell_idx) = self.get_cell_index(position) {
                let entry = LightGridEntry {
                    light_index: light_idx,
                    lod_level: lod,
                    depth,
                    padding: 0,
                };
                self.grid_entries.push(entry);

                let cell = &mut self.grid_cells[cell_idx];
                if cell.count == 0 {
                    cell.offset = (self.grid_entries.len() - 1) as u32;
                }
                cell.count += 1;
            }
        }

        self.stats.visible_lights = self.gpu_lights.len() as u32;
        self.stats.culling_time_ms = start.elapsed().as_secs_f32() * 1000.0;
    }
}

// ===================================================================
// Глобальное состояние
// ===================================================================

thread_local! {
    static LIGHT_STATE: std::cell::RefCell<Option<LightState>> = std::cell::RefCell::new(None);
}

// ===================================================================
// Экспортируемые функции для LightAPI
// ===================================================================

extern "C" fn add_light(instance: *mut c_void, light: *const GPULight) -> u32 {
    if instance.is_null() || light.is_null() { return u32::MAX; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.add_light(&*light)
    }
}

extern "C" fn remove_light(instance: *mut c_void, _id: u32) {
    if instance.is_null() { return; }
    // Для простоты не реализуем удаление
}

extern "C" fn update_light(_instance: *mut c_void, _id: u32, _light: *const GPULight) {
    // Для простоты не реализуем обновление
}

extern "C" fn get_lights_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.lights.len() as u32
    }
}

extern "C" fn cull_lights(instance: *mut c_void, camera_pos: *const f32, view_proj: *const f32, dt: f32) {
    if instance.is_null() || camera_pos.is_null() || view_proj.is_null() { return; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        let pos = [*camera_pos, *camera_pos.add(1), *camera_pos.add(2)];
        let vp = std::slice::from_raw_parts(view_proj, 16);
        state.cull(pos, vp.try_into().unwrap(), dt);
    }
}

extern "C" fn get_gpu_lights(instance: *mut c_void) -> *const GPULight {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        if state.gpu_lights.is_empty() { ptr::null() } else { state.gpu_lights.as_ptr() }
    }
}

extern "C" fn get_gpu_lights_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.gpu_lights.len() as u32
    }
}

extern "C" fn get_light_grid_cells(instance: *mut c_void) -> *const LightGridCell {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        if state.grid_cells.is_empty() { ptr::null() } else { state.grid_cells.as_ptr() }
    }
}

extern "C" fn get_light_grid_entries(instance: *mut c_void) -> *const LightGridEntry {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        if state.grid_entries.is_empty() { ptr::null() } else { state.grid_entries.as_ptr() }
    }
}

extern "C" fn get_grid_cells_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.grid_cells.len() as u32
    }
}

extern "C" fn get_grid_entries_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.grid_entries.len() as u32
    }
}

extern "C" fn get_stats(instance: *mut c_void) -> LightStats {
    if instance.is_null() { return LightStats::default(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.stats
    }
}

static LIGHT_API: LightAPI = LightAPI {
    add_light,
    remove_light,
    update_light,
    get_lights_count,
    cull: cull_lights,
    get_gpu_lights,
    get_gpu_lights_count,
    get_light_grid_cells,
    get_light_grid_entries,
    get_grid_cells_count,
    get_grid_entries_count,
    get_stats,
};

// ===================================================================
// Экспортируемые функции для PluginAPI
// ===================================================================

extern "C" fn init_light(_device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void {
    if config_ptr.is_null() { return ptr::null_mut(); }
    unsafe {
        let config = &*(config_ptr as *const LightConfig);
        let state = Box::new(LightState::new(config));
        Box::into_raw(state) as *mut c_void
    }
}

extern "C" fn shutdown_light(instance: *mut c_void) {
    if !instance.is_null() {
        unsafe { let _ = Box::from_raw(instance as *mut LightState); }
    }
}

extern "C" fn update_plugin(_instance: *mut c_void, _dt: f32) {}

extern "C" fn get_physics_api(_instance: *mut c_void) -> *const c_void {
    ptr::null()
}

extern "C" fn get_light_api(instance: *mut c_void) -> *const c_void {
    &LIGHT_API as *const _ as *const c_void
}

// ===================================================================
// Главная экспортируемая функция
// ===================================================================

#[no_mangle]
pub extern "C" fn get_plugin_api() -> PluginAPI {
    PluginAPI {
        version: PLUGIN_API_VERSION,
        plugin_type: PLUGIN_TYPE_LIGHT,
        name: b"FirstFires Light Culling\0".as_ptr() as *const std::os::raw::c_char,
        init: init_light,
        shutdown: shutdown_light,
        update: update_plugin,
        get_physics_api,
        get_light_api,
    }
}