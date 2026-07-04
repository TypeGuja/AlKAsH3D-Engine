// src/plugin/manager.rs
//! Менеджер динамической загрузки плагинов

use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use super::abi::*;
use super::physics_api::*;
use super::light_api::*;

pub struct PluginManager {
    plugins: HashMap<String, LoadedPlugin>,
    physics_plugin: Option<String>,
    light_plugin: Option<String>,
}

struct LoadedPlugin {
    lib: Library,
    api: PluginAPI,
    instance: *mut c_void,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            physics_plugin: None,
            light_plugin: None,
        }
    }

    /// Загрузить плагин из DLL
    pub fn load_plugin(&mut self, path: &str, device_ptr: *mut c_void, config_ptr: *const c_void) -> Result<(), String> {
        let path = Path::new(path);
        if !path.exists() {
            return Err(format!("Plugin not found: {}", path.display()));
        }

        unsafe {
            let lib = Library::new(path).map_err(|e| format!("Failed to load {}: {}", path.display(), e))?;

            // Получаем функцию get_plugin_api
            let get_api: Symbol<extern "C" fn() -> PluginAPI> = lib
                .get(b"get_plugin_api")
                .map_err(|_| "get_plugin_api not found in DLL")?;

            let api = get_api();

            // Проверяем версию
            if api.version != PLUGIN_API_VERSION {
                return Err(format!("Plugin API version mismatch: {} != {}", api.version, PLUGIN_API_VERSION));
            }

            // Инициализируем плагин
            let instance = (api.init)(device_ptr, config_ptr);
            if instance.is_null() {
                return Err("Plugin init failed".into());
            }

            let name = match api.plugin_type {
                PluginType::Physics => {
                    self.physics_plugin = Some(path.file_stem().unwrap().to_str().unwrap().to_string());
                    "physics"
                }
                PluginType::LightCulling => {
                    self.light_plugin = Some(path.file_stem().unwrap().to_str().unwrap().to_string());
                    "light_culling"
                }
                _ => "unknown",
            };

            println!("✅ Loaded {} plugin: {}", name, path.display());

            self.plugins.insert(path.display().to_string(), LoadedPlugin {
                lib,
                api,
                instance,
            });

            Ok(())
        }
    }

    /// Получить API физики
    pub fn get_physics_api(&self) -> Option<&PhysicsAPI> {
        let plugin_name = self.physics_plugin.as_ref()?;
        let plugin = self.plugins.get(plugin_name)?;
        unsafe {
            let api_ptr = (plugin.api.get_physics_api)(plugin.instance);
            if api_ptr.is_null() {
                None
            } else {
                Some(&*(api_ptr as *const PhysicsAPI))
            }
        }
    }

    /// Получить instance физики
    pub fn get_physics_instance(&self) -> Option<*mut c_void> {
        let plugin_name = self.physics_plugin.as_ref()?;
        let plugin = self.plugins.get(plugin_name)?;
        Some(plugin.instance)
    }

    /// Получить API света
    pub fn get_light_api(&self) -> Option<&LightAPI> {
        let plugin_name = self.light_plugin.as_ref()?;
        let plugin = self.plugins.get(plugin_name)?;
        unsafe {
            let api_ptr = (plugin.api.get_light_api)(plugin.instance);
            if api_ptr.is_null() {
                None
            } else {
                Some(&*(api_ptr as *const LightAPI))
            }
        }
    }

    /// Получить instance света
    pub fn get_light_instance(&self) -> Option<*mut c_void> {
        let plugin_name = self.light_plugin.as_ref()?;
        let plugin = self.plugins.get(plugin_name)?;
        Some(plugin.instance)
    }

    /// Выгрузить все плагины
    pub fn unload_all(&mut self) {
        for (_, plugin) in self.plugins.drain() {
            (plugin.api.shutdown)(plugin.instance);
        }
        self.physics_plugin = None;
        self.light_plugin = None;
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        // ИСПРАВЛЕНО: `unload_all()` существовал и раньше, но никогда не
        // вызывался автоматически. При дропе `PluginManager` (например,
        // вместе с `PhysicsPlugin`/`LightPlugin` в plugin/mod.rs) `HashMap`
        // с плагинами просто освобождал `Library`, выгружая DLL из памяти,
        // а `(api.shutdown)(instance)` внутри неё так никогда и не
        // вызывался — плагин не успевал корректно освободить свои
        // внутренние ресурсы. Теперь это происходит гарантированно, и в
        // правильном порядке: сначала shutdown у плагина, потом (в конце
        // итерации цикла в unload_all) выгрузка самой Library.
        self.unload_all();
    }
}