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
    /// ДОБАВЛЕНО (скриптинг — нативные C++/Rust плагины): та же схема, что
    /// и у get_physics_api/get_light_api — поле добавлено В КОНЕЦ структуры
    /// (не в середину), чтобы не сдвинуть смещения полей, которые уже
    /// умеют читать существующие alkash3d_firstfires.dll/inertial.dll (сами
    /// они это поле не пишут — у каждого плагина СВОЯ копия `PluginAPI`,
    /// синхронизируемая вручную, см. комментарий про ABI-контракт у
    /// LightAPI в light_api.rs).
    ///
    /// ВАЖНО: `PluginAPI` возвращается ИЗ DLL ПО ЗНАЧЕНИЮ (не по
    /// указателю) — если уже собранный (старый) плагин физики/света
    /// вернёт структуру БЕЗ этого поля, движковая сторона всё равно
    /// прочитает 11 полей из места возврата, и `get_scripting_api`
    /// окажется мусором (что лежало в памяти до вызова). Это БЕЗОПАСНО
    /// ровно потому, что это поле вызывается ТОЛЬКО когда
    /// `api.plugin_type == PluginType::Scripting` (см.
    /// `PluginManager::load_plugin`/`get_scripting_api` в manager.rs) — а
    /// существующие FirstFires/Inertial всегда возвращают
    /// LightCulling/Physics и это поле у них никогда не читается и не
    /// вызывается. Если когда-нибудь понадобится вызывать
    /// `get_scripting_api` независимо от `plugin_type` — сначала поднять
    /// `PLUGIN_API_VERSION` и заставить все плагины пересобраться.
    pub get_scripting_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}