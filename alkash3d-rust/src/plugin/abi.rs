// src/plugin/abi.rs
//! Единый ABI для всех плагинов

use std::ffi::c_void;

/// Версия API плагинов
pub const PLUGIN_API_VERSION: u32 = 1;

/// Тип плагина
#[repr(u32)]
pub enum PluginType {
    Physics = 0,
    LightCulling = 1,
    Audio = 2,
    Scripting = 3,
}

/// Базовый ABI для всех плагинов
#[repr(C)]
pub struct PluginAPI {
    /// Версия API
    pub version: u32,
    /// Тип плагина
    pub plugin_type: PluginType,
    /// Имя плагина
    pub name: *const std::os::raw::c_char,

    // Жизненный цикл
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),

    // Получение указателей на специфические API
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}