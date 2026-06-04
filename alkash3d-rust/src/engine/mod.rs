// src/engine/mod.rs
//! Основной движок Alkash3D с поддержкой плагинов

use std::ffi::c_void;
use std::sync::Arc;
use libloading::{Library, Symbol};

use crate::scheduler::*;

// ===================================================================
// Plugin ABI - общий для всех плагинов
// ===================================================================

/// Версия API плагинов
pub const PLUGIN_API_VERSION: u32 = 1;

/// Тип плагина
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginType {
    Physics = 0,
    LightCulling = 1,
    Audio = 2,
    Scripting = 3,
}

/// Базовый ABI для всех плагинов
#[repr(C)]
pub struct PluginAPI {
    pub version: u32,
    pub plugin_type: PluginType,
    pub name: *const std::os::raw::c_char,

    // Жизненный цикл
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),

    // Получение специфических API
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

// ===================================================================
// Send-безопасный указатель для FFI
// ===================================================================

#[derive(Debug, Clone, Copy)]
pub struct SendPtr(pub *mut c_void);

unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

impl SendPtr {
    pub fn as_ptr(&self) -> *mut c_void {
        self.0
    }
}

// ===================================================================
// Physics API
// ===================================================================

/// Конфигурация физики
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsConfig {
    pub max_bodies: i32,
    pub world_size: f32,
    pub cell_size: f32,
    pub solver_iterations: i32,
    pub use_simd: i32,
}

/// Структура тела (совместима с Fortran)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsBody {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angular_acceleration: [f32; 3],
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub is_static: i32,
    pub is_asleep: i32,
}

/// Структура контакта
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
}

/// Статистика физики
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicsStats {
    pub bodies_count: u32,
    pub active_bodies: u32,
    pub contacts_count: u32,
    pub pairs_count: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
}

/// API физического плагина
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicsAPI {
    pub add_body: extern "C" fn(instance: *mut c_void, body: *const PhysicsBody) -> i32,
    pub remove_body: extern "C" fn(instance: *mut c_void, id: i32),
    pub get_body: extern "C" fn(instance: *mut c_void, id: i32) -> PhysicsBody,
    pub get_bodies_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub update: extern "C" fn(instance: *mut c_void, dt: f32, gravity: f32),
    pub get_contacts: extern "C" fn(instance: *mut c_void) -> *const PhysicsContact,
    pub get_contacts_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_pairs: extern "C" fn(instance: *mut c_void) -> *const i32,
    pub get_pairs_count: extern "C" fn(instance: *mut c_void) -> i32,
    pub get_stats: extern "C" fn(instance: *mut c_void) -> PhysicsStats,
}

// Safety: PhysicsAPI содержит только функции, которые можно вызывать из любого потока
unsafe impl Send for PhysicsAPI {}
unsafe impl Sync for PhysicsAPI {}

/// Плагин физики
pub struct PhysicsPlugin {
    pub _lib: Library,
    pub api: PhysicsAPI,
    pub instance: SendPtr,
}

impl PhysicsPlugin {
    pub fn load(path: &str, config: PhysicsConfig) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path).map_err(|e| format!("Failed to load {}: {}", path, e))?;

            let get_api: Symbol<extern "C" fn() -> PluginAPI> = lib
                .get(b"get_plugin_api")
                .map_err(|_| "get_plugin_api not found in DLL")?;

            let plugin_api = get_api();

            if plugin_api.version != PLUGIN_API_VERSION {
                return Err(format!("API version mismatch: {} != {}", plugin_api.version, PLUGIN_API_VERSION));
            }

            if plugin_api.plugin_type != PluginType::Physics {
                return Err(format!("Invalid plugin type: expected Physics, got {:?}", plugin_api.plugin_type));
            }

            let instance = (plugin_api.init)(std::ptr::null_mut(), &config as *const _ as *const c_void);
            if instance.is_null() {
                return Err("Physics plugin init failed".into());
            }

            let physics_api_ptr = (plugin_api.get_physics_api)(instance);
            if physics_api_ptr.is_null() {
                (plugin_api.shutdown)(instance);
                return Err("Physics plugin has no PhysicsAPI".into());
            }

            let api = *(physics_api_ptr as *const PhysicsAPI);

            Ok(Self { _lib: lib, api, instance: SendPtr(instance) })
        }
    }

    pub fn update(&mut self, dt: f32, gravity: f32) {
        (self.api.update)(self.instance.as_ptr(), dt, gravity);
    }

    pub fn add_body(&mut self, body: &PhysicsBody) -> i32 {
        (self.api.add_body)(self.instance.as_ptr(), body)
    }

    pub fn get_contacts(&self) -> &[PhysicsContact] {
        unsafe {
            let ptr = (self.api.get_contacts)(self.instance.as_ptr());
            let count = (self.api.get_contacts_count)(self.instance.as_ptr()) as usize;
            if count == 0 || ptr.is_null() { &[] } else { std::slice::from_raw_parts(ptr, count) }
        }
    }

    pub fn get_stats(&self) -> PhysicsStats {
        (self.api.get_stats)(self.instance.as_ptr())
    }
}

impl Drop for PhysicsPlugin {
    fn drop(&mut self) {
        unsafe {
            let get_api: Symbol<extern "C" fn() -> PluginAPI> = match self._lib.get(b"get_plugin_api") {
                Ok(f) => f,
                Err(_) => return,
            };
            let plugin_api = get_api();
            (plugin_api.shutdown)(self.instance.as_ptr());
        }
    }
}

// ===================================================================
// Light API
// ===================================================================

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
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub direction: [f32; 4],
    pub params: [f32; 4],
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

/// API плагина Light Culling
#[repr(C)]
#[derive(Clone, Copy)]
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

// Safety: LightAPI содержит только функции, которые можно вызывать из любого потока
unsafe impl Send for LightAPI {}
unsafe impl Sync for LightAPI {}

/// Плагин света
pub struct LightPlugin {
    pub _lib: Library,
    pub api: LightAPI,
    pub instance: SendPtr,
}

impl LightPlugin {
    pub fn load(path: &str, device_ptr: *mut c_void, config: LightConfig) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path).map_err(|e| format!("Failed to load {}: {}", path, e))?;

            let get_api: Symbol<extern "C" fn() -> PluginAPI> = lib
                .get(b"get_plugin_api")
                .map_err(|_| "get_plugin_api not found in DLL")?;

            let plugin_api = get_api();

            if plugin_api.version != PLUGIN_API_VERSION {
                return Err(format!("API version mismatch: {} != {}", plugin_api.version, PLUGIN_API_VERSION));
            }

            if plugin_api.plugin_type != PluginType::LightCulling {
                return Err(format!("Invalid plugin type: expected LightCulling, got {:?}", plugin_api.plugin_type));
            }

            let instance = (plugin_api.init)(device_ptr, &config as *const _ as *const c_void);
            if instance.is_null() {
                return Err("Light plugin init failed".into());
            }

            let light_api_ptr = (plugin_api.get_light_api)(instance);
            if light_api_ptr.is_null() {
                (plugin_api.shutdown)(instance);
                return Err("Light plugin has no LightAPI".into());
            }

            let api = *(light_api_ptr as *const LightAPI);

            Ok(Self { _lib: lib, api, instance: SendPtr(instance) })
        }
    }

    pub fn cull(&mut self, camera_pos: [f32; 3], view_proj: &[f32; 16], dt: f32) {
        (self.api.cull)(self.instance.as_ptr(), camera_pos.as_ptr(), view_proj.as_ptr(), dt);
    }

    pub fn add_light(&mut self, light: &GPULight) -> u32 {
        (self.api.add_light)(self.instance.as_ptr(), light)
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        unsafe {
            let ptr = (self.api.get_gpu_lights)(self.instance.as_ptr());
            let count = (self.api.get_gpu_lights_count)(self.instance.as_ptr()) as usize;
            if count == 0 || ptr.is_null() { &[] } else { std::slice::from_raw_parts(ptr, count) }
        }
    }

    pub fn get_stats(&self) -> LightStats {
        (self.api.get_stats)(self.instance.as_ptr())
    }
}

impl Drop for LightPlugin {
    fn drop(&mut self) {
        unsafe {
            let get_api: Symbol<extern "C" fn() -> PluginAPI> = match self._lib.get(b"get_plugin_api") {
                Ok(f) => f,
                Err(_) => return,
            };
            let plugin_api = get_api();
            (plugin_api.shutdown)(self.instance.as_ptr());
        }
    }
}

// ===================================================================
// Plugin Manager
// ===================================================================

pub struct LoadedPlugin {
    pub lib: Library,
    pub api: PluginAPI,
    pub instance: SendPtr,
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if !self.instance.as_ptr().is_null() {
            (self.api.shutdown)(self.instance.as_ptr());
        }
    }
}

pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
    physics_plugin_idx: Option<usize>,
    light_plugin_idx: Option<usize>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            physics_plugin_idx: None,
            light_plugin_idx: None,
        }
    }

    pub fn load_physics(&mut self, path: &str, device_ptr: *mut c_void, config: PhysicsConfig) -> Result<(), String> {
        let idx = self.load_plugin(path, device_ptr, &config as *const _ as *const c_void)?;
        self.physics_plugin_idx = Some(idx);
        Ok(())
    }

    pub fn load_lights(&mut self, path: &str, device_ptr: *mut c_void, config: LightConfig) -> Result<(), String> {
        let idx = self.load_plugin(path, device_ptr, &config as *const _ as *const c_void)?;
        self.light_plugin_idx = Some(idx);
        Ok(())
    }

    fn load_plugin(&mut self, path: &str, device_ptr: *mut c_void, config_ptr: *const c_void) -> Result<usize, String> {
        unsafe {
            let lib = Library::new(path)
                .map_err(|e| format!("Failed to load {}: {}", path, e))?;

            let get_api: Symbol<extern "C" fn() -> PluginAPI> = lib
                .get(b"get_plugin_api")
                .map_err(|_| "get_plugin_api not found in DLL")?;

            let api = get_api();

            if api.version != PLUGIN_API_VERSION {
                return Err(format!("API version mismatch: {} != {}", api.version, PLUGIN_API_VERSION));
            }

            let instance = (api.init)(device_ptr, config_ptr);
            if instance.is_null() {
                return Err("Plugin init failed".into());
            }

            let plugin_type = match api.plugin_type {
                PluginType::Physics => "Physics",
                PluginType::LightCulling => "LightCulling",
                _ => "Unknown",
            };

            println!("✅ Loaded {} plugin: {}", plugin_type, path);

            self.plugins.push(LoadedPlugin {
                lib,
                api,
                instance: SendPtr(instance),
            });
            Ok(self.plugins.len() - 1)
        }
    }

    pub fn get_physics_api(&self) -> Option<PhysicsAPI> {
        let idx = self.physics_plugin_idx?;
        let plugin = &self.plugins[idx];
        unsafe {
            let ptr = (plugin.api.get_physics_api)(plugin.instance.as_ptr());
            if ptr.is_null() { None } else { Some(*(ptr as *const PhysicsAPI)) }
        }
    }

    pub fn get_physics_instance(&self) -> Option<SendPtr> {
        let idx = self.physics_plugin_idx?;
        Some(self.plugins[idx].instance)
    }

    pub fn get_light_api(&self) -> Option<LightAPI> {
        let idx = self.light_plugin_idx?;
        let plugin = &self.plugins[idx];
        unsafe {
            let ptr = (plugin.api.get_light_api)(plugin.instance.as_ptr());
            if ptr.is_null() { None } else { Some(*(ptr as *const LightAPI)) }
        }
    }

    pub fn get_light_instance(&self) -> Option<SendPtr> {
        let idx = self.light_plugin_idx?;
        Some(self.plugins[idx].instance)
    }

    pub fn unload_all(&mut self) {
        self.plugins.clear();
        self.physics_plugin_idx = None;
        self.light_plugin_idx = None;
    }
}

// ===================================================================
// Основной движок
// ===================================================================

pub struct AlkashEngine {
    pub scheduler: Arc<EngineScheduler>,
    pub plugin_manager: PluginManager,
    pub physics: Option<PhysicsPlugin>,
    pub lights: Option<LightPlugin>,
    device_ptr: *mut c_void,
}

impl AlkashEngine {
    pub fn new(device_ptr: *mut c_void) -> Self {
        Self {
            scheduler: Arc::new(EngineScheduler::new()),
            plugin_manager: PluginManager::new(),
            physics: None,
            lights: None,
            device_ptr,
        }
    }

    pub fn init_physics(&mut self, config: PhysicsConfig) -> Result<(), String> {
        // Загружаем через PluginManager
        self.plugin_manager.load_physics("plugins/inertial.dll", self.device_ptr, config)?;

        // Создаём PhysicsPlugin для прямого доступа (используем копию config)
        let plugin = PhysicsPlugin::load("plugins/inertial.dll", config)?;
        self.physics = Some(plugin);

        Ok(())
    }

    pub fn init_lights(&mut self, config: LightConfig) -> Result<(), String> {
        // Загружаем через PluginManager
        self.plugin_manager.load_lights("plugins/firstfires.dll", self.device_ptr, config)?;

        // Создаём LightPlugin для прямого доступа (используем копию config)
        let plugin = LightPlugin::load("plugins/firstfires.dll", self.device_ptr, config)?;
        self.lights = Some(plugin);

        Ok(())
    }

    pub fn update_physics(&self, dt: f32, gravity: f32) {
        if let Some(physics) = &self.physics {
            let scheduler = self.scheduler.clone();
            let physics_ptr = physics.instance;
            let api = physics.api;

            // Запускаем физику через планировщик
            scheduler.execute(
                Task::new(1, TaskPriority::High),
                move || {
                    (api.update)(physics_ptr.as_ptr(), dt, gravity);
                }
            );
        }
    }

    pub fn update_lights(&mut self, camera_pos: [f32; 3], view_proj: &[f32; 16], dt: f32) {
        if let Some(lights) = &mut self.lights {
            lights.cull(camera_pos, view_proj, dt);
        }
    }

    pub fn add_physics_body(&mut self, body: PhysicsBody) -> i32 {
        if let Some(physics) = &mut self.physics {
            return physics.add_body(&body);
        }
        -1
    }

    pub fn add_light(&mut self, light: GPULight) -> u32 {
        if let Some(lights) = &mut self.lights {
            return lights.add_light(&light);
        }
        u32::MAX
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        if let Some(lights) = &self.lights {
            lights.get_gpu_lights()
        } else {
            &[]
        }
    }

    pub fn get_physics_contacts(&self) -> &[PhysicsContact] {
        if let Some(physics) = &self.physics {
            physics.get_contacts()
        } else {
            &[]
        }
    }

    pub fn get_physics_stats(&self) -> PhysicsStats {
        if let Some(physics) = &self.physics {
            physics.get_stats()
        } else {
            PhysicsStats::default()
        }
    }

    pub fn get_light_stats(&self) -> LightStats {
        if let Some(lights) = &self.lights {
            lights.get_stats()
        } else {
            LightStats::default()
        }
    }
}