// src/engine/mod.rs
//! Интеграция планировщика и DLL-плагинов

mod core;

pub use core::AlkashEngine;

use libloading::{Library, Symbol};
use crate::scheduler::*;

// ===================================================================
// Send-безопасный указатель для FFI
// ===================================================================

#[derive(Debug, Clone, Copy)]
pub struct SendPtr(pub *mut std::ffi::c_void);

// Говорим компилятору, что указатель можно передавать между потоками
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

impl SendPtr {
    pub fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.0
    }
}

// ===================================================================
// PHYSICS PLUGIN (Inertial.dll)
// ===================================================================

#[repr(C)]
pub struct PhysicsConfig {
    pub max_bodies: i32,
    pub world_size: f32,
    pub cell_size: f32,
    pub solver_iterations: i32,
    pub use_simd: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranRigidBody {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angular_acceleration: [f32; 3],
    pub inertia: [[f32; 3]; 3],
    pub inv_inertia: [[f32; 3]; 3],
    pub mass: f32,
    pub inv_mass: f32,
    pub restitution: f32,
    pub friction: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub is_static: i32,
    pub is_asleep: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FortranContact {
    pub body_a: i32,
    pub body_b: i32,
    pub normal: [f32; 3],
    pub penetration: f32,
    pub point: [f32; 3],
    pub tangent1: [f32; 3],
    pub tangent2: [f32; 3],
    pub friction_impulse: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsStats {
    pub bodies_count: u32,
    pub active_bodies: u32,
    pub contacts_count: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicsAPI {
    pub init: extern "C" fn(config: *const PhysicsConfig) -> *mut std::ffi::c_void,
    pub shutdown: extern "C" fn(instance: *mut std::ffi::c_void),
    pub update: extern "C" fn(instance: *mut std::ffi::c_void, dt: f32, gravity: f32),
    pub add_body: extern "C" fn(instance: *mut std::ffi::c_void, body: *const FortranRigidBody) -> i32,
    pub get_body: extern "C" fn(instance: *mut std::ffi::c_void, id: i32) -> FortranRigidBody,
    pub get_contacts: extern "C" fn(instance: *mut std::ffi::c_void) -> *const FortranContact,
    pub get_contacts_count: extern "C" fn(instance: *mut std::ffi::c_void) -> i32,
    pub get_stats: extern "C" fn(instance: *mut std::ffi::c_void) -> PhysicsStats,
}

// Safety: PhysicsAPI содержит только функции, которые можно вызывать из любого потока
unsafe impl Send for PhysicsAPI {}
unsafe impl Sync for PhysicsAPI {}

pub struct PhysicsPlugin {
    pub _lib: Library,
    pub api: PhysicsAPI,
    pub instance: SendPtr,
}

impl PhysicsPlugin {
    pub fn load(path: &str, config: PhysicsConfig) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path).map_err(|e| format!("Failed to load {}: {}", path, e))?;

            let get_api: Symbol<extern "C" fn() -> PhysicsAPI> = lib
                .get(b"get_physics_api")
                .map_err(|_| "get_physics_api not found in DLL")?;

            let api = get_api();
            let instance = (api.init)(&config);

            if instance.is_null() {
                return Err("Physics plugin init failed".into());
            }

            Ok(Self { _lib: lib, api, instance: SendPtr(instance) })
        }
    }

    pub fn update(&mut self, dt: f32, gravity: f32) {
        (self.api.update)(self.instance.as_ptr(), dt, gravity);
    }

    pub fn add_body(&mut self, body: &FortranRigidBody) -> i32 {
        (self.api.add_body)(self.instance.as_ptr(), body)
    }

    pub fn get_contacts(&self) -> &[FortranContact] {
        unsafe {
            let ptr = (self.api.get_contacts)(self.instance.as_ptr());
            let count = (self.api.get_contacts_count)(self.instance.as_ptr()) as usize;
            if count == 0 || ptr.is_null() { &[] } else { std::slice::from_raw_parts(ptr, count) }
        }
    }

    // Для асинхронного вызова — возвращаем API и указатель, которые можно передать в поток
    pub fn into_async_call(self) -> (PhysicsAPI, SendPtr) {
        (self.api, self.instance)
    }
}

impl Drop for PhysicsPlugin {
    fn drop(&mut self) {
        (self.api.shutdown)(self.instance.as_ptr());
    }
}

// ===================================================================
// LIGHT PLUGIN (FirstFires.dll)
// ===================================================================

#[repr(C)]
pub struct LightConfig {
    pub max_lights: u32,
    pub tile_size: u32,
    pub far_plane: f32,
    pub lod_distances: [f32; 3],
    pub grid_cell_size: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GPULight {
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub direction: [f32; 4],
    pub params: [f32; 4],
}

// Safety: GPULight используется только как POD для GPU буферов
unsafe impl bytemuck::Zeroable for GPULight {}
unsafe impl bytemuck::Pod for GPULight {}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightGridCell {
    pub offset: u32,
    pub count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightGridEntry {
    pub light_index: u32,
    pub lod_level: u32,
    pub depth: f32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LightAPI {
    pub init: extern "C" fn(device_ptr: *mut std::ffi::c_void, config: *const LightConfig) -> *mut std::ffi::c_void,
    pub shutdown: extern "C" fn(instance: *mut std::ffi::c_void),
    pub cull: extern "C" fn(instance: *mut std::ffi::c_void, camera_pos: *const f32, view_proj: *const f32, dt: f32),
    pub add_light: extern "C" fn(instance: *mut std::ffi::c_void, light: *const GPULight) -> u32,
    pub get_gpu_lights: extern "C" fn(instance: *mut std::ffi::c_void) -> *const GPULight,
    pub get_gpu_lights_count: extern "C" fn(instance: *mut std::ffi::c_void) -> u32,
    pub get_light_grid_cells: extern "C" fn(instance: *mut std::ffi::c_void) -> *const LightGridCell,
    pub get_light_grid_entries: extern "C" fn(instance: *mut std::ffi::c_void) -> *const LightGridEntry,
    pub get_visible_count: extern "C" fn(instance: *mut std::ffi::c_void) -> u32,
}

// Safety: LightAPI содержит только функции, которые можно вызывать из любого потока
unsafe impl Send for LightAPI {}
unsafe impl Sync for LightAPI {}

pub struct LightPlugin {
    pub _lib: Library,
    pub api: LightAPI,
    pub instance: SendPtr,
}

impl LightPlugin {
    pub fn load(path: &str, device_ptr: *mut std::ffi::c_void, config: LightConfig) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path).map_err(|e| format!("Failed to load {}: {}", path, e))?;

            let get_api: Symbol<extern "C" fn() -> LightAPI> = lib
                .get(b"get_light_api")
                .map_err(|_| "get_light_api not found in DLL")?;

            let api = get_api();
            let instance = (api.init)(device_ptr, &config);

            if instance.is_null() {
                return Err("Light plugin init failed".into());
            }

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

    // Для асинхронного вызова — возвращаем API и указатель, которые можно передать в поток
    pub fn into_async_call(self) -> (LightAPI, SendPtr) {
        (self.api, self.instance)
    }
}

impl Drop for LightPlugin {
    fn drop(&mut self) {
        (self.api.shutdown)(self.instance.as_ptr());
    }
}