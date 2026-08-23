// src/lib.rs — alkash3d-examplescript ("Bobber")
//
// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust .alscript-плагины):
// эталонный пример нативного скрипта, доказывающий на практике, что вся
// цепочка "движок -> PluginManager -> ScriptingPlugin -> DLL" реально
// работает. Логика намеренно простая, но нарочно демонстрирует ОБЕ
// возможности, которые запросил пользователь ответом "1,2":
//
//   1) Покадровое обновление (update_script) — сущность плавно "покачивается"
//      вверх-вниз по синусоиде вокруг своей исходной позиции.
//   2) Событийные триггеры (dispatch_event) — получив ZoneEnter (или Custom)
//      событие, скрипт МЕНЯЕТ амплитуду покачивания (умножает её на data[0],
//      если это > 0), что видно в следующих кадрах update_script — то есть
//      событие реально влияет на дальнейшее поведение, а не просто
//      логируется.
//
// Как и alkash3d-inertial/alkash3d-FirstFires, этот крейт НЕ зависит от
// alkash3d-rust как библиотеки (нет общего crate с ABI-типами — так
// исторически устроен весь плагинный слой движка, см. подробный
// комментарий в scripting_api.rs про "синхронизируется вручную"). Поэтому
// ниже — СВОЯ ЛОКАЛЬНАЯ копия только тех ABI-структур, что реально нужны
// скриптовому плагину; порядок полей и типы обязаны байт-в-байт совпадать
// с alkash3d-rust/src/plugin/abi.rs и .../scripting_api.rs.

use std::ffi::{c_void, CString};
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
    // Этот плагин — PluginType::Scripting, поэтому именно это поле здесь
    // реально используется движком (см. abi.rs — оно добавлено ПОСЛЕДНИМ,
    // это важно для ABI-совместимости со старыми Physics/LightCulling DLL).
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
    #[allow(dead_code)]
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
    // ДОБАВЛЕНО (скриптинг, вторая волна — Lua как DLL-плагин): поле
    // введено в общий ABI (см. scripting_api.rs) для alkash3d-luascript,
    // которому нужно знать ПУТЬ к .lua-файлу при создании прикрепления
    // (одна DLL обслуживает много разных .lua). Этот Native-плагин
    // (alkash3d-examplescript, PluginType::Scripting, script_type==2)
    // такого мультиплексирования не делает — движок никогда не должен
    // вызывать это поле для него (см. комментарий у поля в
    // scripting_api.rs), но структура обязана содержать ВСЕ поля
    // ScriptingAPI байт-в-байт, иначе `static SCRIPTING_API` не
    // соберётся. Реализация — просто алиас на обычный create_script,
    // путь игнорируется.
    create_script_with_source: extern "C" fn(instance: *mut c_void, entity_id: u64, source_path: *const c_char) -> u32,
}

// ---------------------------------------------------------------------
// Состояние плагина
// ---------------------------------------------------------------------

/// Одно прикрепление скрипта к сущности — своя "исходная" позиция (точка
/// покоя, вокруг которой идёт покачивание — запоминается в момент
/// первого update_script, чтобы не зависеть от того, где именно сущность
/// заспавнена) и своя фаза/амплитуда, чтобы разные Bobber-ы на разных
/// сущностях не были синхронизированы друг с другом один-в-один.
struct BobberInstance {
    entity_id: u64,
    /// None до первого update_script — устанавливается один раз.
    base_position: Option<[f32; 3]>,
    time_accum: f32,
    amplitude: f32,
    alive: bool,
}

const DEFAULT_AMPLITUDE: f32 = 0.3; // метры
const BOB_SPEED: f32 = 2.0; // рад/сек

struct PluginState {
    scripts: Vec<BobberInstance>,
    max_scripts: u32,
}

impl PluginState {
    fn new(max_scripts: u32) -> Self {
        Self {
            scripts: Vec::with_capacity(max_scripts as usize),
            max_scripts,
        }
    }

    fn create(&mut self, entity_id: u64) -> u32 {
        // Ищем свободный (уже уничтоженный) слот, чтобы id были стабильно
        // переиспользуемы — тот же подход, что и в остальных плагинах
        // движка (add_body/add_light).
        if let Some((idx, slot)) = self
            .scripts
            .iter_mut()
            .enumerate()
            .find(|(_, s)| !s.alive)
        {
            *slot = BobberInstance {
                entity_id,
                base_position: None,
                time_accum: 0.0,
                amplitude: DEFAULT_AMPLITUDE,
                alive: true,
            };
            return idx as u32;
        }

        if self.scripts.len() as u32 >= self.max_scripts {
            return u32::MAX;
        }

        self.scripts.push(BobberInstance {
            entity_id,
            base_position: None,
            time_accum: 0.0,
            amplitude: DEFAULT_AMPLITUDE,
            alive: true,
        });
        (self.scripts.len() - 1) as u32
    }

    fn destroy(&mut self, script_id: u32) {
        if let Some(slot) = self.scripts.get_mut(script_id as usize) {
            slot.alive = false;
        }
    }

    fn update(&mut self, script_id: u32, ctx: &mut ScriptContext) {
        let Some(slot) = self.scripts.get_mut(script_id as usize) else {
            return;
        };
        if !slot.alive {
            return;
        }

        // Точка покоя фиксируется один раз, при первом кадре — дальше
        // покачивание идёт вокруг НЕЁ, а не вокруг position каждого кадра
        // (иначе амплитуда накапливалась бы, а не колебалась).
        let base = *slot.base_position.get_or_insert(ctx.position);

        slot.time_accum += ctx.delta_time;
        let offset_y = (slot.time_accum * BOB_SPEED).sin() * slot.amplitude;

        ctx.out_position = [base[0], base[1] + offset_y, base[2]];
        ctx.out_rotation = ctx.rotation;
        ctx.position_changed = 1;
    }

    fn dispatch(&mut self, script_id: u32, event: &ScriptEvent) {
        let Some(slot) = self.scripts.get_mut(script_id as usize) else {
            return;
        };
        if !slot.alive {
            return;
        }

        match event.event_type {
            x if x == ScriptEventType::Spawned as u32 => {
                // Ничего специального — base_position и так лениво
                // выставляется в первом update_script.
            }
            x if x == ScriptEventType::ZoneEnter as u32 || x == ScriptEventType::Custom as u32 => {
                // Соглашение для этого демо-скрипта: data[0] > 0.0 —
                // множитель новой амплитуды покачивания относительно
                // DEFAULT_AMPLITUDE. Именно это и доказывает, что событие
                // реально меняет дальнейшее поведение, а не просто
                // логируется — следующий же update_script покажет другую
                // амплитуду покачивания.
                if event.data[0] > 0.0 {
                    slot.amplitude = DEFAULT_AMPLITUDE * event.data[0];
                }
            }
            _ => {}
        }
    }

    fn active_count(&self) -> u32 {
        self.scripts.iter().filter(|s| s.alive).count() as u32
    }
}

/// Инстанс плагина — весь DLL целиком, как и описано в scripting_api.rs:
/// один instance обслуживает МНОЖЕСТВО прикреплённых сущностей.
struct Instance {
    state: Mutex<PluginState>,
    scripting_api: ScriptingAPI,
}

// ---------------------------------------------------------------------
// extern "C" реализация ScriptingAPI
// ---------------------------------------------------------------------

extern "C" fn api_create_script(instance: *mut c_void, entity_id: u64) -> u32 {
    if instance.is_null() {
        return u32::MAX;
    }
    let inst = unsafe { &*(instance as *mut Instance) };
    let mut state = inst.state.lock().unwrap();
    state.create(entity_id)
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

/// См. комментарий у поля `create_script_with_source` в `ScriptingAPI`
/// выше — этот Native-плагин не мультиплексирует источники, поэтому
/// `source_path` полностью игнорируется, вызов — прямой алиас на
/// `api_create_script`.
extern "C" fn api_create_script_with_source(
    instance: *mut c_void,
    entity_id: u64,
    _source_path: *const c_char,
) -> u32 {
    api_create_script(instance, entity_id)
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
    // config_ptr указывает на ScriptConfig, переданный движком через
    // ScriptingPlugin::load (см. plugin/mod.rs) — как и у остальных
    // плагинов, указатель валиден только на время вызова init, поэтому
    // копируем нужное поле сразу, по значению.
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
    // Пер-кадровая логика этого плагина идёт через update_script
    // (вызывается движком отдельно на каждое прикрепление, см.
    // AlkashEngine::update_native_scripts в engine/mod.rs) — этот
    // общий PluginAPI::update здесь намеренно пустой, как и у
    // остальных non-Physics плагинов без глобального тика.
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

/// Имя плагина как statically-хранимая C-строка (живёт всё время жизни
/// DLL — тот же приём, что в alkash3d-inertial/alkash3d-FirstFires:
/// протекает намеренно, DLL выгружается целиком при завершении процесса).
fn plugin_name_cstr() -> *const c_char {
    use std::sync::OnceLock;
    static NAME: OnceLock<CString> = OnceLock::new();
    NAME.get_or_init(|| CString::new("alkash3d_examplescript_bobber").unwrap())
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
