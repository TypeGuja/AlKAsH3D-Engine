// src/plugin/scripting_api.rs
//! API для нативных (C++/Rust) скриптовых плагинов.
//!
//! ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust скрипты): первая
//! реализация языков скриптинга движка (см. `alscript_format.rs`:
//! `script_type` 0=Python,1=Lua,2=Native,4=Hybrid — 3 зарезервирован, был
//! C#, вырезан по просьбе пользователя, см. комментарий в шапке
//! alscript_format.rs). Нативные языки (C++/Rust) компилируются в
//! отдельную DLL заранее — тот же механизм, что уже используется для
//! `alkash3d-FirstFires`/`alkash3d-inertial` (общий `PluginAPI`/
//! `PluginManager`, см. abi.rs/manager.rs). Python/Lua вместо этого
//! используют встроенный интерпретатор с hot-reload (Python) или
//! универсальный DLL-рантайм (Lua) — не C-ABI плагин со скрипт-
//! специфичной логикой каждый, и сюда НЕ входят.
//!
//! Архитектурное отличие от PhysicsPlugin/LightPlugin: там один DLL-плагин
//! управляет ОДНИМ большим набором однородных объектов (все тела/все
//! источники света). Скриптов, наоборот, может быть НЕСКОЛЬКО РАЗНЫХ DLL
//! одновременно (например "vehicle_ai.dll" и "door_trigger.dll"), и каждая
//! DLL может обслуживать НЕСКОЛЬКО сущностей сразу (одна и та же логика
//! ИИ на нескольких машинах). Поэтому `instance` (из `PluginAPI::init`) —
//! это состояние ВСЕЙ DLL целиком, а `create_script`/`destroy_script`
//! ниже управляют отдельными "прикреплениями" этой логики к конкретным
//! entity внутри одного instance — тот же паттерн, что `add_light`/
//! `update_light`/id в LightAPI, только для скриптов.

use std::ffi::c_void;

/// Конфигурация скриптового плагина — передаётся в `PluginAPI::init` как
/// `config_ptr`, как и `PhysicsConfig`/`LightConfig` у остальных плагинов.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScriptConfig {
    /// Максимум одновременных прикреплений скрипта (create_script) для
    /// этого instance — плагин волен интерпретировать это как подсказку
    /// для предвыделения памяти, а не как жёсткий лимит.
    pub max_scripts: u32,
}

/// Тип события — минимальный набор для первого этапа (движение по кадру +
/// триггеры). Расширяется по мере необходимости; НЕ убирать/менять
/// порядок существующих вариантов — значения зашиты в C-ABI на обеих
/// сторонах (движок и плагин).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptEventType {
    /// Произвольное игровое событие, различаемое только по `data`/по
    /// смыслу, согласованному между движком и конкретным скриптом.
    Custom = 0,
    /// Сущность-владелец скрипта вошла в контакт с другой (physics
    /// contact, см. PhysicsContact в physics_api.rs).
    CollisionEnter = 1,
    /// Симметрично CollisionEnter — контакт прекратился.
    CollisionExit = 2,
    /// Сущность вошла в именованную триггерную зону (данные события — см.
    /// `ScriptEvent::data`, интерпретация зоны на совести вызывающего
    /// движкового кода, привязку зона->сущности этот API не хранит).
    ZoneEnter = 3,
    ZoneExit = 4,
    /// Скрипт только что создан (`create_script`) — присылается ПОСЛЕ
    /// создания как первый event, до первого `update_script`, чтобы
    /// плагин мог сделать инициализацию, требующую доступа к движковому
    /// контексту (в отличие от `create_script`, у которого такого доступа
    /// нет — только entity_id).
    Spawned = 5,
    /// Скрипт вот-вот будет уничтожен (`destroy_script`) — присылается
    /// ПЕРЕД самим destroy, чтобы плагин успел освободить свои ресурсы
    /// корректно, а не только через Drop/освобождение памяти инстанса.
    Despawned = 6,
}

/// Одно событие, доставляемое конкретному прикреплению скрипта.
///
/// `source_entity`/`target_entity` — упакованные `EntityId` (см.
/// `scene::EntityId` в scene.rs: `index: u32, generation: u32`), в виде
/// `(index as u64) << 32 | generation as u64` — тот же формат, что и
/// `owner_entity_id` в `ScriptDescriptor` (alscript_format.rs). Плагин НЕ
/// обязан их разыменовывать сам (у него и нет доступа к Scene) — это
/// просто непрозрачный идентификатор, который можно вернуть обратно
/// движку через будущие callback-и (пока не реализованы, см. план ниже).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScriptEvent {
    pub event_type: u32, // ScriptEventType as u32
    pub source_entity: u64,
    pub target_entity: u64,
    /// Произвольные числовые данные события — например точка контакта
    /// (xyz) + сила удара (w) для CollisionEnter, либо специфичные для
    /// Custom-события числа по соглашению между конкретной игрой и
    /// конкретным скриптом.
    pub data: [f32; 4],
}

/// Контекст одного вызова `update_script` — движок заполняет ВХОДНЫЕ поля
/// перед вызовом (entity_id/delta_time/frame_number/position/rotation),
/// плагин может ЗАПИСАТЬ obновлённые position/rotation в out_position/
/// out_rotation и выставить `position_changed=1`, если хочет подвинуть
/// свою сущность — движок применяет изменение к Transform ПОСЛЕ
/// update_script, только если этот флаг установлен (иначе оставляет
/// Transform как есть — большинство скриптов на первом этапе, например
/// триггеры дверей, физическую позицию не двигают вообще).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScriptContext {
    pub entity_id: u64,
    pub delta_time: f32,
    pub frame_number: u64,
    /// Текущая позиция/поворот сущности-владельца на момент вызова —
    /// снимок `Transform` (см. scene.rs), read-only вход.
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    /// Выход: см. описание структуры выше.
    pub out_position: [f32; 3],
    pub out_rotation: [f32; 3],
    pub position_changed: u32,
}

impl Default for ScriptContext {
    fn default() -> Self {
        Self {
            entity_id: 0,
            delta_time: 0.0,
            frame_number: 0,
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            out_position: [0.0, 0.0, 0.0],
            out_rotation: [0.0, 0.0, 0.0],
            position_changed: 0,
        }
    }
}

/// API нативного скриптового плагина.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScriptingAPI {
    /// Создаёт новое прикрепление скриптовой логики этой DLL к сущности
    /// `entity_id` (упакованный EntityId, см. `ScriptEvent` выше) —
    /// возвращает id этого прикрепления, уникальный В ПРЕДЕЛАХ данного
    /// `instance` (как `add_light`/`add_body` у остальных плагинов).
    /// `u32::MAX` — ошибка (например превышен `max_scripts`).
    pub create_script: extern "C" fn(instance: *mut c_void, entity_id: u64) -> u32,
    /// Уничтожает прикрепление — id может быть переиспользован для
    /// следующего `create_script` (как и в остальных плагинах, engine
    /// обязан не использовать старый id после этого вызова).
    pub destroy_script: extern "C" fn(instance: *mut c_void, script_id: u32),
    /// Вызывается КАЖДЫЙ кадр для каждого живого прикрепления — `ctx`
    /// передаётся движком по `*mut` (не `*const`), потому что плагин
    /// пишет в него результат (`out_position`/`out_rotation`/
    /// `position_changed`).
    pub update_script: extern "C" fn(instance: *mut c_void, script_id: u32, ctx: *mut ScriptContext),
    /// Доставляет одно событие конкретному прикреплению (см.
    /// `ScriptEventType`) — вызывается движком по факту события
    /// (столкновение, вход в зону и т.п.), НЕ каждый кадр.
    pub dispatch_event: extern "C" fn(instance: *mut c_void, script_id: u32, event: *const ScriptEvent),
    /// Статистика — сколько прикреплений живо прямо сейчас в этом
    /// instance (для отладочного HUD/логов, как `get_stats` у
    /// физики/света).
    pub get_active_scripts_count: extern "C" fn(instance: *mut c_void) -> u32,

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Lua как универсальный
    /// DLL-плагин): в отличие от `create_script` (для C++/Rust, где
    /// КАЖДАЯ DLL реализует ровно одну, зашитую в код логику — путь к
    /// скрипту ей просто не нужен), одна и та же alkash3d-luascript DLL
    /// обслуживает МНОЖЕСТВО разных .lua-файлов одновременно — поэтому ей
    /// нужно знать, какой именно файл исполнять для данного прикрепления.
    /// `source_path` — null-terminated C-строка (UTF-8), путь к .lua
    /// файлу; плагин копирует её содержимое сам (движок не обязан
    /// удерживать указатель валидным дольше самого вызова).
    ///
    /// ДОБАВЛЕНО В КОНЕЦ структуры — тот же приём ABI-совместимости, что
    /// и `PluginAPI::get_scripting_api` в abi.rs: уже собранные
    /// alkash3d-examplescript.dll (Native, `PluginType::Scripting`,
    /// реализует ScriptingAPI ИЗ 5 ПОЛЕЙ, без этого) продолжают работать
    /// — движковая сторона просто никогда не вызывает
    /// `create_script_with_source` для DLL, зарегистрированных как
    /// `script_type == 2` (Native, см. alscript_format.rs) — только для
    /// `script_type == 1` (Lua). Как и там, если понадобится вызывать это
    /// поле независимо от script_type — сначала поднять
    /// `PLUGIN_API_VERSION` и пересобрать все плагины.
    pub create_script_with_source: extern "C" fn(instance: *mut c_void, entity_id: u64, source_path: *const std::os::raw::c_char) -> u32,
}
