// src/plugin/mod.rs
mod abi;
mod physics_api;
mod light_api;
mod scripting_api;
mod manager;

pub use abi::*;
pub use physics_api::*;
pub use light_api::*;
pub use scripting_api::*;
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

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): обёртка над
    /// `PhysicsAPI::get_body`, уже присутствовавшим в ABI (см.
    /// physics_api.rs) с самой первой версии, но раньше не имевшим
    /// соответствующего метода в безопасной обёртке — ничего в движке его
    /// ни разу не вызывало. Нужен для синхронизации видимой геометрии с
    /// результатом текущего кадра физики (см.
    /// `AlkashEngine::sync_physics_transforms` в engine/mod.rs) — только
    /// через `get_body` движок узнаёт, куда РЕАЛЬНО переместил тело
    /// физический солвер (интегрирование + разрешение столкновений), а не
    /// куда оно было бы без учёта коллизий.
    pub fn get_body(&self, id: i32) -> PhysicsBody {
        (self.api.get_body)(self.instance, id)
    }

    /// ДОБАВЛЕНО (объединённая сцена — физика из .alworld): обёртка над
    /// `PhysicsAPI::remove_body`, тоже присутствовавшим в ABI с самой
    /// первой версии (см. `plugin/physics_api.rs`), но раньше без
    /// безопасной обёртки — до этого момента ни один физический объект,
    /// однажды созданный через `add_body`, не мог быть удалён из плагина
    /// иначе как полной перезагрузкой DLL. Нужен, чтобы
    /// `AlkashEngine::unload_chunk` мог убрать физическое тело объекта
    /// при выгрузке его чанка (см. `ChunkRuntimeState::spawned_physics_bodies`)
    /// — без этого метода тела выгруженных чанков продолжали бы жить в
    /// плагине навсегда, накапливаясь при активном стриминге открытого
    /// мира.
    pub fn remove_body(&mut self, id: i32) {
        (self.api.remove_body)(self.instance, id);
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

    /// ДОБАВЛЕНО (диагностика — жалоба пользователя "всё равно ФПС не
    /// радует" ПОСЛЕ фиксов стриминга/hot-reload/culling): обёртка над
    /// `PhysicsAPI::get_stats`, присутствовавшим в ABI плагина с самого
    /// начала (см. `plugin/physics_api.rs`), но раньше без безопасной
    /// обёртки — ничего в движке его ни разу не вызывало, статистика
    /// (broad/narrow phase/solver время, число тел/контактов/пар)
    /// реально считалась внутри `alkash3d-inertial` каждый кадр, но
    /// никогда не покидала плагин. Нужна, чтобы РЕАЛЬНО измерить, где
    /// именно уходит время в кадре, вместо дальнейших догадок по коду.
    pub fn get_stats(&self) -> PhysicsStats {
        (self.api.get_stats)(self.instance)
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

    /// ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
    /// мерцание): обёртка над `LightAPI::update_light`, которая уже была
    /// в ABI плагина (см. `plugin/light_api.rs`) с самой первой версии, но
    /// раньше не имела соответствующего метода в безопасной обёртке
    /// `LightPlugin` — ничего в движке её ни разу не вызывало, потому что
    /// раньше свет один раз добавлялся (`add_light`) и больше никогда не
    /// менялся. Мерцание и включение/выключение по времени суток (см.
    /// `AlkashEngine::update_day_night` в engine/mod.rs) требуют менять
    /// intensity/enabled уже добавленного света КАЖДЫЙ кадр — без этого
    /// метода это было бы невозможно без изменения ABI плагина.
    pub fn update_light(&mut self, id: u32, light: &GPULight) {
        (self.api.update_light)(self.instance, id, light);
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

    // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): доступ к
    // пространственной сетке, которую FirstFires уже строит внутри
    // `cull()` (см. LightState::cull в alkash3d-FirstFires/src/lib.rs) —
    // используется, чтобы пиксельный шейдер проверял только фонари своей
    // ячейки, а не перебирал ВЕСЬ видимый список на каждый пиксель (см.
    // render_frame/compile_default_shaders в engine/mod.rs).

    pub fn get_grid_cells(&self) -> &[LightGridCell] {
        unsafe {
            let ptr = (self.api.get_light_grid_cells)(self.instance);
            let count = (self.api.get_grid_cells_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    pub fn get_grid_entries(&self) -> &[LightGridEntry] {
        unsafe {
            let ptr = (self.api.get_light_grid_entries)(self.instance);
            let count = (self.api.get_grid_entries_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    pub fn get_grid_params(&self) -> LightGridParams {
        (self.api.get_grid_params)(self.instance)
    }
}

/// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины): безопасная
/// обёртка над одной загруженной скриптовой DLL — тот же паттерн, что и
/// `PhysicsPlugin`/`LightPlugin` выше (собственный `PluginManager`,
/// `api`/`instance`, кэшированные из первого `get_scripting_api`/
/// `get_scripting_instance` после загрузки).
///
/// Отличие от Physics/Light: `AlkashEngine` держит НЕСКОЛЬКО
/// `ScriptingPlugin` одновременно (по одному на каждую РАЗНУЮ загруженную
/// DLL — см. `native_script_plugins: HashMap<String, ScriptingPlugin>` в
/// engine/mod.rs), а не один статический экземпляр. Одна и та же
/// `ScriptingPlugin` (одна DLL) может при этом обслуживать НЕСКОЛЬКО
/// прикреплённых сущностей через `create_script`/`script_id`.
pub struct ScriptingPlugin {
    pub api: ScriptingAPI,
    pub instance: *mut c_void,
    manager: PluginManager,
}

impl ScriptingPlugin {
    /// Грузит DLL по `path` и конфигурирует её через `config`. В отличие
    /// от Physics/Light, `device_ptr` скриптам на этом этапе не передаётся
    /// (`std::ptr::null_mut()`) — нативным скриптам первого этапа (движение
    /// сущности + события) прямой доступ к D3D12-устройству не нужен;
    /// расширить сигнатуру, если будущий скрипт всё же захочет рисовать
    /// сам (например debug-визуализация) — тогда это будет ломающее
    /// изменение ABI, требующее поднять PLUGIN_API_VERSION.
    pub fn load(path: &str, config: ScriptConfig) -> Result<Self, String> {
        let mut manager = PluginManager::new();
        let config_ptr = &config as *const ScriptConfig as *const c_void;
        manager.load_plugin(path, std::ptr::null_mut(), config_ptr)?;

        let api = manager.get_scripting_api(path).ok_or("No scripting API")?;
        let instance = manager.get_scripting_instance(path).ok_or("No scripting instance")?;

        Ok(Self {
            api: *api,
            instance,
            manager,
        })
    }

    /// Прикрепляет логику этой DLL к сущности `entity_id` (уже
    /// упакованный, см. `ScriptEvent` в scripting_api.rs) — возвращает
    /// script_id для последующих `update_script`/`dispatch_event`/
    /// `destroy_script`, либо `None`, если плагин отказал (например
    /// `u32::MAX` — превышен `max_scripts` из `ScriptConfig`).
    pub fn create_script(&mut self, entity_id: u64) -> Option<u32> {
        let id = (self.api.create_script)(self.instance, entity_id);
        if id == u32::MAX { None } else { Some(id) }
    }

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Lua как универсальный
    /// DLL-плагин): вариант `create_script` с указанием пути к конкретному
    /// .lua-файлу (см. `ScriptingAPI::create_script_with_source` в
    /// scripting_api.rs) — нужен, потому что одна alkash3d-luascript.dll
    /// обслуживает МНОГО разных .lua-скриптов, в отличие от Native
    /// (alkash3d-examplescript), где вся логика зашита в саму DLL и
    /// обычного `create_script` достаточно. `source_path` конвертируется
    /// в null-terminated C-строку здесь же — плагин не обязан удерживать
    /// указатель дольше самого вызова.
    pub fn create_script_with_source(&mut self, entity_id: u64, source_path: &str) -> Option<u32> {
        let c_path = match std::ffi::CString::new(source_path) {
            Ok(s) => s,
            Err(_) => return None, // путь содержит NUL-байт — некорректные данные
        };
        let id = (self.api.create_script_with_source)(self.instance, entity_id, c_path.as_ptr());
        if id == u32::MAX { None } else { Some(id) }
    }

    pub fn destroy_script(&mut self, script_id: u32) {
        (self.api.destroy_script)(self.instance, script_id);
    }

    /// Заполненный движком `ctx` передаётся по `&mut` — плагин пишет
    /// результат обратно в те же поля (`out_position`/`out_rotation`/
    /// `position_changed`), вызывающая сторона (`AlkashEngine::update`)
    /// сама решает, применять ли их к `Transform`.
    pub fn update_script(&mut self, script_id: u32, ctx: &mut ScriptContext) {
        (self.api.update_script)(self.instance, script_id, ctx as *mut ScriptContext);
    }

    pub fn dispatch_event(&mut self, script_id: u32, event: &ScriptEvent) {
        (self.api.dispatch_event)(self.instance, script_id, event as *const ScriptEvent);
    }

    pub fn get_active_scripts_count(&self) -> u32 {
        (self.api.get_active_scripts_count)(self.instance)
    }
}