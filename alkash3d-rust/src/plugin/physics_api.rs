// src/plugin/physics_api.rs
//! API для физического плагина

use std::ffi::c_void;

/// Конфигурация физики
#[repr(C)]
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

/// API физического плагина
#[repr(C)]
pub struct PhysicsAPI {
    // Управление телами
    pub add_body: extern "C" fn(instance: *mut c_void, body: *const PhysicsBody) -> i32,
    pub remove_body: extern "C" fn(instance: *mut c_void, id: i32),
    pub get_body: extern "C" fn(instance: *mut c_void, id: i32) -> PhysicsBody,
    pub get_bodies_count: extern "C" fn(instance: *mut c_void) -> i32,

    // Обновление
    pub update: extern "C" fn(instance: *mut c_void, dt: f32, gravity: f32),

    // Получение результатов
    pub get_contacts: extern "C" fn(instance: *mut c_void) -> *const PhysicsContact,
    pub get_contacts_count: extern "C" fn(instance: *mut c_void) -> i32,

    // Broad phase пары
    pub get_pairs: extern "C" fn(instance: *mut c_void) -> *const i32,
    pub get_pairs_count: extern "C" fn(instance: *mut c_void) -> i32,

    // Статистика
    pub get_stats: extern "C" fn(instance: *mut c_void) -> PhysicsStats,
}

/// Статистика физики
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicsStats {
    pub bodies_count: u32,
    pub active_bodies: u32,
    pub contacts_count: u32,
    pub pairs_count: u32,
    pub broad_phase_time_ms: f32,
    pub narrow_phase_time_ms: f32,
    pub solver_time_ms: f32,
}