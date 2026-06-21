// src/plugin/mod.rs
mod abi;
mod physics_api;
mod light_api;
mod manager;

pub use abi::*;
pub use physics_api::*;
pub use light_api::*;
pub use manager::*;

// Вспомогательные структуры для плагинов
use std::ffi::c_void;
use crate::plugin::manager::PluginManager;

pub struct PhysicsPlugin {
    pub api: PhysicsAPI,
    pub instance: *mut c_void,
    manager: PluginManager,
}

impl PhysicsPlugin {
    pub fn load(path: &str, config: PhysicsConfig) -> Result<Self, String> {
        let mut manager = PluginManager::new();
        let config_ptr = &config as *const PhysicsConfig as *const c_void;
        manager.load_plugin(path, std::ptr::null_mut(), config_ptr)?;

        let api = manager.get_physics_api().ok_or("No physics API")?;

        // Получаем instance через PluginManager
        let instance = manager.get_physics_instance().ok_or("No physics instance")?;

        Ok(Self {
            api: *api,
            instance,
            manager,
        })
    }

    pub fn update(&mut self, dt: f32, gravity: f32) {
        (self.api.update)(self.instance, dt, gravity);
    }

    pub fn add_body(&mut self, body: &PhysicsBody) -> i32 {
        (self.api.add_body)(self.instance, body)
    }

    pub fn get_contacts(&self) -> &[PhysicsContact] {
        unsafe {
            let ptr = (self.api.get_contacts)(self.instance);
            let count = (self.api.get_contacts_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }
}

pub struct LightPlugin {
    pub api: LightAPI,
    pub instance: *mut c_void,
    manager: PluginManager,
}

impl LightPlugin {
    pub fn load(path: &str, device_ptr: *mut c_void, config: LightConfig) -> Result<Self, String> {
        let mut manager = PluginManager::new();
        let config_ptr = &config as *const LightConfig as *const c_void;
        manager.load_plugin(path, device_ptr, config_ptr)?;

        let api = manager.get_light_api().ok_or("No light API")?;
        let instance = manager.get_light_instance().ok_or("No light instance")?;

        Ok(Self {
            api: *api,
            instance,
            manager,
        })
    }

    pub fn add_light(&mut self, light: &GPULight) -> u32 {
        (self.api.add_light)(self.instance, light)
    }

    pub fn cull(&mut self, camera_pos: [f32; 3], view_proj: &[f32; 16], dt: f32) {
        (self.api.cull)(self.instance, camera_pos.as_ptr(), view_proj.as_ptr(), dt);
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        unsafe {
            let ptr = (self.api.get_gpu_lights)(self.instance);
            let count = (self.api.get_gpu_lights_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }
}