// src/plugin/manager.rs
//! Менеджер динамической загрузки плагинов

use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use super::abi::*;
use super::physics_api::*;
use super::light_api::*;
use super::scripting_api::*;

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

            // ИСПРАВЛЕНО (найдено при диагностике "No light API" на реальной
            // машине пользователя — плагин грузился успешно, лог печатал
            // "✅ Loaded light_culling plugin: ...", но get_light_api() всё
            // равно возвращал None): ключ, под которым плагин кладётся в
            // `self.plugins` HashMap — `path.display().to_string()` (ПОЛНЫЙ
            // путь, например "../alkash3d-FirstFires/target/release/
            // alkash3d_firstfires.dll"), а `self.physics_plugin`/
            // `self.light_plugin` раньше сохраняли `path.file_stem()` —
            // только имя файла БЕЗ пути и расширения (например
            // "alkash3d_firstfires"). `get_light_api()`/`get_physics_api()`/
            // `get_light_instance()`/`get_physics_instance()` ниже делают
            // `self.plugins.get(plugin_name)` с этим сохранённым именем —
            // но раз ключи не совпадают (полный путь vs просто stem),
            // `.get()` всегда возвращал `None`, даже когда плагин на самом
            // деле был успешно загружен и лежал в HashMap. Теперь сохраняем
            // ТОТ ЖЕ `path.display().to_string()`, что используется как
            // ключ при `insert` — единственный источник истины для этого
            // ключа, без риска рассинхронизации в будущем.
            let key = path.display().to_string();
            let name = match api.plugin_type {
                PluginType::Physics => {
                    self.physics_plugin = Some(key.clone());
                    "physics"
                }
                PluginType::LightCulling => {
                    self.light_plugin = Some(key.clone());
                    "light_culling"
                }
                // ДОБАВЛЕНО (скриптинг — нативные C++/Rust плагины): в
                // отличие от Physics/LightCulling (singleton — ОДИН плагин
                // такого типа на весь движок, отсюда self.physics_plugin/
                // self.light_plugin как Option<String>), скриптовых DLL
                // может быть загружено НЕСКОЛЬКО одновременно (разная
                // игровая логика в разных .dll). Поэтому здесь намеренно
                // НЕТ singleton-поля вроде self.scripting_plugin — плагин
                // просто остаётся в `self.plugins` под своим ключом (путь
                // к DLL), и `get_scripting_api`/`get_scripting_instance`
                // ниже находят его по этому же ключу, который вызывающая
                // сторона (ScriptingPlugin::load в plugin/mod.rs) и так уже
                // знает — тот самый путь, что был передан в load_plugin().
                PluginType::Scripting => "scripting",
                _ => "unknown",
            };

            println!("✅ Loaded {} plugin: {}", name, path.display());

            self.plugins.insert(key, LoadedPlugin {
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

    /// ДОБАВЛЕНО (скриптинг — нативные C++/Rust плагины): в отличие от
    /// get_physics_api/get_light_api (без параметров — ищут ЕДИНСТВЕННый
    /// загруженный плагин своего типа через singleton-поле), здесь нужен
    /// явный `key` — тот же путь к DLL, что был передан в `load_plugin()`
    /// — потому что скриптовых плагинов может быть загружено несколько
    /// одновременно (см. комментарий у `PluginType::Scripting` выше в
    /// `load_plugin`).
    pub fn get_scripting_api(&self, key: &str) -> Option<&ScriptingAPI> {
        let plugin = self.plugins.get(key)?;
        unsafe {
            let api_ptr = (plugin.api.get_scripting_api)(plugin.instance);
            if api_ptr.is_null() {
                None
            } else {
                Some(&*(api_ptr as *const ScriptingAPI))
            }
        }
    }

    /// Получить instance скриптового плагина по тому же ключу.
    pub fn get_scripting_instance(&self, key: &str) -> Option<*mut c_void> {
        let plugin = self.plugins.get(key)?;
        Some(plugin.instance)
    }

    /// Загружен ли уже плагин с таким ключом (путём к DLL) — используется
    /// `ScriptingPlugin::load`, чтобы НЕ грузить одну и ту же DLL повторно,
    /// если она уже нужна другому скрипту/сущности (см. подробный
    /// комментарий в scripting_api.rs про "одна DLL — много прикреплений").
    pub fn is_loaded(&self, key: &str) -> bool {
        self.plugins.contains_key(key)
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