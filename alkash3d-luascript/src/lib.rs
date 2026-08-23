// src/lib.rs — alkash3d-luascript
//
// ДОБАВЛЕНО (скриптинг, вторая волна — Lua как DLL-плагин): универсальный
// Lua-рантайм плагин. По решению пользователя Lua компилируется в DLL так
// же, как C++/Rust (тот же ScriptingAPI C-ABI), а НЕ как отдельная
// hot-reload архитектура (та осталась только за Python, см.
// engine::scripting_python в alkash3d-rust/src/engine/mod.rs). C#
// изначально тоже планировался как DLL-плагин, но был вырезан из
// скриптинга целиком по просьбе пользователя (не требуется .NET SDK).
//
// Отличие от alkash3d-examplescript (Native): там вся логика зашита в
// саму DLL — один плагин, одно поведение. Здесь DLL — универсальный
// исполнитель: каждое прикрепление (create_script_with_source) грузит
// СВОЙ .lua-файл текстом и держит СВОЁ независимое Lua-состояние
// (mlua::Lua). Соглашение для .lua-файла (см. example_bobber.lua рядом):
//   - глобальная функция `update(dt, x, y, z, rx, ry, rz) -> (x, y, z, changed)`
//     вызывается каждый кадр (аналог update_script/ScriptContext).
//   - глобальная функция `on_event(event_type, data0, data1, data2, data3)`
//     вызывается по событию (аналог dispatch_event/ScriptEvent) — опциональна.
// Числовой протокол (а не userdata-объекты) выбран намеренно: он проще,
// не требует регистрировать сложные метатаблицы в первой версии, и
// прямо соответствует тому же плоскому C-ABI, что уже использует Native.

use mlua::{Lua, Function};
use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

// ---------------------------------------------------------------------
// Локальная копия единого ABI плагинов (см. alkash3d-rust/src/plugin/abi.rs)
// ---------------------------------------------------------------------

const PLUGIN_API_VERSION: u32 = 1;

#[repr(u32)]
#[allow(dead_code)]
enum PluginType {
    Physics = 0,
    LightCulling = 1,
    Audio = 2,
    Scripting = 3,
}

#[repr(C)]
struct PluginAPI {
    version: u32,
    plugin_type: PluginType,
    name: *const c_char,
    init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    shutdown: extern "C" fn(instance: *mut c_void),
    update: extern "C" fn(instance: *mut c_void, dt: f32),
    get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    get_scripting_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

// ---------------------------------------------------------------------
// Локальная копия scripting_api.rs (см. alkash3d-rust/src/plugin/scripting_api.rs)
// ---------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct ScriptConfig {
    max_scripts: u32,
}

#[repr(u32)]
#[allow(dead_code)]
enum ScriptEventType {
    Custom = 0,
    CollisionEnter = 1,
    CollisionExit = 2,
    ZoneEnter = 3,
    ZoneExit = 4,
    Spawned = 5,
    Despawned = 6,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ScriptEvent {
    event_type: u32,
    #[allow(dead_code)]
    source_entity: u64,
    #[allow(dead_code)]
    target_entity: u64,
    data: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ScriptContext {
    #[allow(dead_code)]
    entity_id: u64,
    delta_time: f32,
    #[allow(dead_code)]
    frame_number: u64,
    position: [f32; 3],
    rotation: [f32; 3],
    out_position: [f32; 3],
    out_rotation: [f32; 3],
    position_changed: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ScriptingAPI {
    create_script: extern "C" fn(instance: *mut c_void, entity_id: u64) -> u32,
    destroy_script: extern "C" fn(instance: *mut c_void, script_id: u32),
    update_script: extern "C" fn(instance: *mut c_void, script_id: u32, ctx: *mut ScriptContext),
    dispatch_event: extern "C" fn(instance: *mut c_void, script_id: u32, event: *const ScriptEvent),
    get_active_scripts_count: extern "C" fn(instance: *mut c_void) -> u32,
    create_script_with_source: extern "C" fn(instance: *mut c_void, entity_id: u64, source_path: *const c_char) -> u32,
}

// ---------------------------------------------------------------------
// Состояние плагина
// ---------------------------------------------------------------------

/// Одно прикрепление — своё независимое Lua-состояние (осознанно: два
/// разных .lua-скрипта, даже если это один и тот же файл на двух разных
/// сущностях, НЕ делят глобальные переменные друг с другом — иначе
/// например счётчик времени одного "боббера" утекал бы в другой).
struct LuaInstance {
    #[allow(dead_code)]
    entity_id: u64,
    lua: Lua,
    has_update: bool,
    has_on_event: bool,
    alive: bool,
}

struct PluginState {
    scripts: Vec<Option<LuaInstance>>,
    max_scripts: u32,
}

impl PluginState {
    fn new(max_scripts: u32) -> Self {
        Self {
            scripts: Vec::with_capacity(max_scripts as usize),
            max_scripts,
        }
    }

    fn create(&mut self, entity_id: u64, source_path: &str) -> u32 {
        let source = match std::fs::read_to_string(source_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[alkash3d-luascript] не удалось прочитать '{}': {}",
                    source_path, e
                );
                return u32::MAX;
            }
        };

        let lua = Lua::new();
        if let Err(e) = lua.load(&source).set_name(source_path).exec() {
            eprintln!(
                "[alkash3d-luascript] ошибка выполнения '{}': {}",
                source_path, e
            );
            return u32::MAX;
        }

        // Заимствование `globals`/`get::<_, Function>` должно закончиться
        // ДО перемещения `lua` в LuaInstance ниже — отдельный блок scope
        // гарантирует это явно, не полагаясь на порядок дропа временных.
        let (has_update, has_on_event) = {
            let globals = lua.globals();
            (
                globals.get::<_, Function>("update").is_ok(),
                globals.get::<_, Function>("on_event").is_ok(),
            )
        };

        let instance = LuaInstance {
            entity_id,
            lua,
            has_update,
            has_on_event,
            alive: true,
        };

        // Ищем свободный слот (уже уничтоженный) — тот же приём id
        // переиспользования, что и в alkash3d-examplescript.
        if let Some((idx, slot)) = self
            .scripts
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.is_none())
        {
            *slot = Some(instance);
            return idx as u32;
        }

        if self.scripts.len() as u32 >= self.max_scripts {
            return u32::MAX;
        }

        self.scripts.push(Some(instance));
        (self.scripts.len() - 1) as u32
    }

    fn destroy(&mut self, script_id: u32) {
        if let Some(slot) = self.scripts.get_mut(script_id as usize) {
            *slot = None; // Lua-состояние (и вся его память) освобождается здесь
        }
    }

    fn update(&mut self, script_id: u32, ctx: &mut ScriptContext) {
        let Some(Some(inst)) = self.scripts.get_mut(script_id as usize) else {
            return;
        };
        if !inst.alive || !inst.has_update {
            return;
        }

        let globals = inst.lua.globals();
        let func: Function = match globals.get("update") {
            Ok(f) => f,
            Err(_) => return,
        };

        // Числовой протокол: update(dt, x,y,z, rx,ry,rz) -> (x,y,z,changed)
        let result: mlua::Result<(f32, f32, f32, u32)> = func.call((
            ctx.delta_time,
            ctx.position[0], ctx.position[1], ctx.position[2],
            ctx.rotation[0], ctx.rotation[1], ctx.rotation[2],
        ));

        match result {
            Ok((x, y, z, changed)) => {
                if changed != 0 {
                    ctx.out_position = [x, y, z];
                    ctx.out_rotation = ctx.rotation;
                    ctx.position_changed = 1;
                }
            }
            Err(e) => {
                eprintln!("[alkash3d-luascript] ошибка в update(): {}", e);
            }
        }
    }

    fn dispatch(&mut self, script_id: u32, event: &ScriptEvent) {
        let Some(Some(inst)) = self.scripts.get_mut(script_id as usize) else {
            return;
        };
        if !inst.alive || !inst.has_on_event {
            return;
        }

        let globals = inst.lua.globals();
        let func: Function = match globals.get("on_event") {
            Ok(f) => f,
            Err(_) => return,
        };

        let result: mlua::Result<()> = func.call((
            event.event_type,
            event.data[0], event.data[1], event.data[2], event.data[3],
        ));

        if let Err(e) = result {
            eprintln!("[alkash3d-luascript] ошибка в on_event(): {}", e);
        }
    }

    fn active_count(&self) -> u32 {
        self.scripts.iter().filter(|s| s.is_some()).count() as u32
    }
}

/// Инстанс плагина — вся DLL целиком. `paths` не хранится отдельно —
/// используется только на момент create (см. PluginState::create).
struct Instance {
    state: Mutex<PluginState>,
    scripting_api: ScriptingAPI,
}

// ---------------------------------------------------------------------
// extern "C" реализация ScriptingAPI
// ---------------------------------------------------------------------

extern "C" fn api_create_script(_instance: *mut c_void, _entity_id: u64) -> u32 {
    // Обычный create_script (без пути к .lua) для этого плагина
    // осмысленным быть не может — универсальному Lua-рантайму НЕЧЕГО
    // исполнять без указания файла. Возвращаем ошибку, а не паникуем —
    // движок обязан использовать create_script_with_source для Lua (см.
    // AlkashEngine::load_native_script в engine/mod.rs — там уже есть
    // ветвление по script_type).
    eprintln!("[alkash3d-luascript] create_script() без пути к .lua не поддерживается — используйте create_script_with_source()");
    u32::MAX
}

extern "C" fn api_create_script_with_source(
    instance: *mut c_void,
    entity_id: u64,
    source_path: *const c_char,
) -> u32 {
    if instance.is_null() || source_path.is_null() {
        return u32::MAX;
    }
    let path = unsafe { CStr::from_ptr(source_path) };
    let path = match path.to_str() {
        Ok(s) => s,
        Err(_) => return u32::MAX, // не валидный UTF-8
    };

    let inst = unsafe { &*(instance as *mut Instance) };
    let mut state = inst.state.lock().unwrap();
    state.create(entity_id, path)
}

extern "C" fn api_destroy_script(instance: *mut c_void, script_id: u32) {
    if instance.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *mut Instance) };
    let mut state = inst.state.lock().unwrap();
    state.destroy(script_id);
}

extern "C" fn api_update_script(instance: *mut c_void, script_id: u32, ctx: *mut ScriptContext) {
    if instance.is_null() || ctx.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *mut Instance) };
    let ctx_ref = unsafe { &mut *ctx };
    let mut state = inst.state.lock().unwrap();
    state.update(script_id, ctx_ref);
}

extern "C" fn api_dispatch_event(instance: *mut c_void, script_id: u32, event: *const ScriptEvent) {
    if instance.is_null() || event.is_null() {
        return;
    }
    let inst = unsafe { &*(instance as *mut Instance) };
    let event_ref = unsafe { &*event };
    let mut state = inst.state.lock().unwrap();
    state.dispatch(script_id, event_ref);
}

extern "C" fn api_get_active_scripts_count(instance: *mut c_void) -> u32 {
    if instance.is_null() {
        return 0;
    }
    let inst = unsafe { &*(instance as *mut Instance) };
    let state = inst.state.lock().unwrap();
    state.active_count()
}

static SCRIPTING_API: ScriptingAPI = ScriptingAPI {
    create_script: api_create_script,
    destroy_script: api_destroy_script,
    update_script: api_update_script,
    dispatch_event: api_dispatch_event,
    get_active_scripts_count: api_get_active_scripts_count,
    create_script_with_source: api_create_script_with_source,
};

// ---------------------------------------------------------------------
// extern "C" реализация PluginAPI (жизненный цикл)
// ---------------------------------------------------------------------

extern "C" fn plugin_init(_device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void {
    let max_scripts = if config_ptr.is_null() {
        64
    } else {
        unsafe { (*(config_ptr as *const ScriptConfig)).max_scripts }
    };

    let instance = Box::new(Instance {
        state: Mutex::new(PluginState::new(max_scripts)),
        scripting_api: SCRIPTING_API,
    });

    Box::into_raw(instance) as *mut c_void
}

extern "C" fn plugin_shutdown(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(instance as *mut Instance));
    }
}

extern "C" fn plugin_update(_instance: *mut c_void, _dt: f32) {
    // Как и у Native — покадровая логика идёт через update_script на
    // каждое прикрепление отдельно, а не через этот общий тик.
}

extern "C" fn plugin_get_physics_api(_instance: *mut c_void) -> *const c_void {
    std::ptr::null()
}

extern "C" fn plugin_get_light_api(_instance: *mut c_void) -> *const c_void {
    std::ptr::null()
}

extern "C" fn plugin_get_scripting_api(instance: *mut c_void) -> *const c_void {
    if instance.is_null() {
        return std::ptr::null();
    }
    let inst = unsafe { &*(instance as *mut Instance) };
    &inst.scripting_api as *const ScriptingAPI as *const c_void
}

fn plugin_name_cstr() -> *const c_char {
    use std::sync::OnceLock;
    static NAME: OnceLock<CString> = OnceLock::new();
    NAME.get_or_init(|| CString::new("alkash3d_luascript").unwrap())
        .as_ptr()
}

#[no_mangle]
pub extern "C" fn get_plugin_api() -> PluginAPI {
    PluginAPI {
        version: PLUGIN_API_VERSION,
        plugin_type: PluginType::Scripting,
        name: plugin_name_cstr(),
        init: plugin_init,
        shutdown: plugin_shutdown,
        update: plugin_update,
        get_physics_api: plugin_get_physics_api,
        get_light_api: plugin_get_light_api,
        get_scripting_api: plugin_get_scripting_api,
    }
}
