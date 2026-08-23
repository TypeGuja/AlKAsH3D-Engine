// src/engine/scripting.rs
//
// ДОБАВЛЕНО (рефакторинг по просьбе пользователя — engine/mod.rs разросся
// до ~8400 строк): все методы AlkashEngine, отвечающие за скриптинг
// (Native/C++/Rust-DLL, Lua-DLL, Python hot-reload), вынесены сюда из
// mod.rs — тот же тип `AlkashEngine`, просто impl-блок физически лежит в
// отдельном файле (в Rust это можно — методы остаются частью того же
// типа независимо от того, в каком файле находится `impl`). В mod.rs
// теперь только `mod scripting; pub use scripting::*;` и сами ВЫЗОВЫ этих
// методов (`self.update_native_scripts(dt)`/`self.update_python_scripts(dt)`
// в `AlkashEngine::update`) — реализация переехала целиком.
//
// НЕ перенесено намеренно: `deps_fallback_path` — используется не только
// скриптингом, но и `init_physics`/`init_lights` (см. mod.rs), поэтому
// осталась общей утилитой в главном файле; скриптинговые методы ниже
// по-прежнему вызывают её как `Self::deps_fallback_path(...)` — это
// работает независимо от того, в каком файле лежит конкретный impl-блок,
// пока оба относятся к одному и тому же типу `AlkashEngine`.
//
// `PythonScriptRuntime` — из engine/scripting_python.rs (независимый
// submodule, был вынесен ещё при первой реализации Python hot-reload, до
// этого рефакторинга).

use super::{AlkashEngine, PythonScriptRuntime};
use windows::core::{Result, Error, HRESULT};

// ===================================================================
// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины)
// ===================================================================

/// Одно живое прикрепление нативного скрипта к конкретной сущности —
/// возвращается `AlkashEngine::load_native_script`, используется для
/// `dispatch_script_event`/`unload_native_script`. Хранится в
/// `AlkashEngine::active_scripts` и обновляется каждый кадр в
/// `update_native_scripts` (вызывается из `AlkashEngine::update`).
///
/// Сам `EntityId` (index+generation, см. scene.rs) хранится ЗДЕСЬ в
/// исходном, неупакованном виде — упаковка в `u64` (см.
/// `pack_entity_id`) происходит только на границе с C-ABI
/// (`ScriptContext`/`ScriptEvent` в plugin/scripting_api.rs), где обычный
/// Rust-тип `EntityId` передать нельзя.
#[derive(Debug, Clone)]
pub struct ScriptHandle {
    /// Ключ в `AlkashEngine::native_scripts` — путь к DLL, реализующей
    /// логику этого скрипта.
    pub dll_path: String,
    /// id прикрепления ВНУТРИ этой DLL (см. `ScriptingAPI::create_script`)
    /// — уникален в пределах одной DLL, но НЕ глобально (два разных DLL
    /// могут независимо выдать одинаковый script_id).
    pub script_id: u32,
    pub entity: crate::scene::EntityId,
}

/// Упаковывает `EntityId` в `u64` для передачи через C-ABI (см.
/// `ScriptContext::entity_id`/`ScriptEvent::source_entity` в
/// plugin/scripting_api.rs) — старшие 32 бита индекс, младшие 32 бита
/// поколение, тот же формат, что уже задокументирован там.
#[inline]
pub fn pack_entity_id(id: crate::scene::EntityId) -> u64 {
    ((id.index() as u64) << 32) | (id.generation() as u64)
}


impl AlkashEngine {
    /// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины):
    /// прикрепляет логику из `dll_path` к сущности `entity`. Если эта DLL
    /// уже загружена (например уже обслуживает другую сущность — типичный
    /// случай, одна "vehicle_ai.dll" на много машин), НЕ грузит её
    /// повторно — просто создаёт новое прикрепление (`create_script`)
    /// внутри уже загруженного instance. Тот же fallback на `deps/`, что и
    /// `init_physics`/`init_lights` (см. `deps_fallback_path`) — тоже
    /// частый случай, когда Cargo не копирует готовую .dll из
    /// `target/<profile>/deps/` в верхний уровень.
    ///
    /// Ключ в `self.native_scripts` — ИСХОДНЫЙ `dll_path`, переданный
    /// вызывающей стороной (а не фактически использованный путь после
    /// fallback) — так последующие вызовы с тем же `dll_path` всегда
    /// находят уже загруженный плагин независимо от того, был ли применён
    /// fallback при первой загрузке.
    pub fn load_native_script(&mut self, dll_path: &str, entity: crate::scene::EntityId) -> Result<ScriptHandle> {
        if !self.native_scripts.contains_key(dll_path) {
            let fallback_path = Self::deps_fallback_path(dll_path);
            let primary_exists = std::path::Path::new(dll_path).exists();
            let config = crate::plugin::ScriptConfig { max_scripts: 256 };

            let (used_path, load_result) = if primary_exists {
                (dll_path.to_string(), crate::plugin::ScriptingPlugin::load(dll_path, config))
            } else if let Some(fallback) = &fallback_path {
                eprintln!(
                    "[ENGINE] '{}' не найден, пробую запасной путь '{}' (Cargo не скопировал .dll из deps/)",
                    dll_path, fallback
                );
                (fallback.clone(), crate::plugin::ScriptingPlugin::load(fallback, config))
            } else {
                (dll_path.to_string(), crate::plugin::ScriptingPlugin::load(dll_path, config))
            };

            match load_result {
                Ok(plugin) => {
                    println!("[ENGINE] ✓ Script plugin loaded from '{}'", used_path);
                    self.native_scripts.insert(dll_path.to_string(), plugin);
                }
                Err(e) => {
                    eprintln!("[ENGINE] Failed to load script plugin '{}': {}", used_path, e);
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        }

        let plugin = self.native_scripts.get_mut(dll_path).unwrap();
        let script_id = plugin.create_script(pack_entity_id(entity)).ok_or_else(|| {
            eprintln!("[ENGINE] load_native_script('{}'): create_script отказал (превышен max_scripts?)", dll_path);
            Error::from_hresult(HRESULT(1))
        })?;

        let handle = ScriptHandle {
            dll_path: dll_path.to_string(),
            script_id,
            entity,
        };

        // Событие Spawned — ДО первого update_script, как задокументировано
        // у ScriptEventType::Spawned (scripting_api.rs): плагин может
        // сделать инициализацию, недоступную из create_script (у которого
        // нет доступа ни к чему, кроме entity_id).
        plugin.dispatch_event(script_id, &crate::plugin::ScriptEvent {
            event_type: crate::plugin::ScriptEventType::Spawned as u32,
            source_entity: pack_entity_id(entity),
            target_entity: pack_entity_id(entity),
            data: [0.0; 4],
        });

        self.active_scripts.push(handle.clone());
        Ok(handle)
    }

    /// Отправляет одно событие конкретному прикреплению скрипта (см.
    /// `ScriptEventType` в plugin/scripting_api.rs) — для триггеров/
    /// столкновений/входа в зону и т.п., НЕ для per-frame апдейта (тот
    /// делает `update_native_scripts` автоматически каждый кадр).
    pub fn dispatch_script_event(&mut self, handle: &ScriptHandle, event: &crate::plugin::ScriptEvent) {
        if let Some(plugin) = self.native_scripts.get_mut(&handle.dll_path) {
            plugin.dispatch_event(handle.script_id, event);
        }
    }

    /// Открепляет скрипт от сущности и уничтожает его прикрепление внутри
    /// DLL (сама DLL остаётся загруженной — на ней могут висеть другие
    /// прикрепления). Присылает `Despawned` ПЕРЕД самим destroy (см.
    /// `ScriptEventType::Despawned`), чтобы плагин успел освободить свои
    /// внутренние ресурсы.
    pub fn unload_native_script(&mut self, handle: &ScriptHandle) {
        if let Some(plugin) = self.native_scripts.get_mut(&handle.dll_path) {
            plugin.dispatch_event(handle.script_id, &crate::plugin::ScriptEvent {
                event_type: crate::plugin::ScriptEventType::Despawned as u32,
                source_entity: pack_entity_id(handle.entity),
                target_entity: pack_entity_id(handle.entity),
                data: [0.0; 4],
            });
            plugin.destroy_script(handle.script_id);
        }
        self.active_scripts.retain(|h| !(h.dll_path == handle.dll_path && h.script_id == handle.script_id));
    }

    /// Вызывается КАЖДЫЙ кадр из `update()` — для каждого живого
    /// прикрепления (`active_scripts`) собирает `ScriptContext` из
    /// текущего `Transform` сущности, вызывает `update_script`, и если
    /// плагин выставил `position_changed`, применяет `out_position`/
    /// `out_rotation` обратно в `Transform`.
    ///
    /// Сущность, уже уничтоженная (`Scene::transform` вернул `None`,
    /// например despawn на предыдущем кадре без явного
    /// `unload_native_script`), тихо пропускается на этот кадр — не
    /// паникует и не убирает прикрепление сама (явная очистка "осиротевших"
    /// прикреплений — ответственность вызывающего игрового кода, как и
    /// синхронный `unload_native_script` при despawn'е, см. `unload_chunk`
    /// для аналогичного паттерна с физическими телами).
    pub(crate) fn update_native_scripts(&mut self, dt: f32) {
        self.script_frame_counter += 1;
        let frame_number = self.script_frame_counter;

        for handle in &self.active_scripts {
            let (position, rotation) = match self.scene.transform(handle.entity) {
                Some(t) => (t.position, t.rotation),
                None => continue,
            };

            let mut ctx = crate::plugin::ScriptContext {
                entity_id: pack_entity_id(handle.entity),
                delta_time: dt,
                frame_number,
                position,
                rotation,
                out_position: position,
                out_rotation: rotation,
                position_changed: 0,
            };

            if let Some(plugin) = self.native_scripts.get_mut(&handle.dll_path) {
                plugin.update_script(handle.script_id, &mut ctx);
            }

            if ctx.position_changed != 0 {
                if let Some(t) = self.scene.transform_mut(handle.entity) {
                    t.position = ctx.out_position;
                    t.rotation = ctx.out_rotation;
                }
            }
        }
    }

    // ===================================================================
    // ДОБАВЛЕНО (скриптинг, вторая волна — Lua как DLL-плагин)
    // ===================================================================

    /// Прикрепляет Lua-скрипт из `script_source_path` к сущности `entity`,
    /// исполняемый через универсальный Lua-рантайм-плагин `dll_path`
    /// (обычно один и тот же путь на весь проект — см.
    /// alkash3d-luascript). В отличие от `load_native_script`, ЗДЕСЬ
    /// путь к DLL и путь к исполняемому .lua-файлу — РАЗНЫЕ параметры:
    /// DLL грузится (и переиспользуется между прикреплениями) обычным
    /// путём через `self.native_scripts` — Lua ничем не отличается от
    /// Native на этом уровне, разница только в том, что вызывается
    /// `create_script_with_source` вместо `create_script`.
    pub fn load_lua_script(
        &mut self,
        dll_path: &str,
        script_source_path: &str,
        entity: crate::scene::EntityId,
    ) -> Result<ScriptHandle> {
        if !self.native_scripts.contains_key(dll_path) {
            let fallback_path = Self::deps_fallback_path(dll_path);
            let primary_exists = std::path::Path::new(dll_path).exists();
            let config = crate::plugin::ScriptConfig { max_scripts: 256 };

            let (used_path, load_result) = if primary_exists {
                (dll_path.to_string(), crate::plugin::ScriptingPlugin::load(dll_path, config))
            } else if let Some(fallback) = &fallback_path {
                eprintln!(
                    "[ENGINE] '{}' не найден, пробую запасной путь '{}' (Cargo не скопировал .dll из deps/)",
                    dll_path, fallback
                );
                (fallback.clone(), crate::plugin::ScriptingPlugin::load(fallback, config))
            } else {
                (dll_path.to_string(), crate::plugin::ScriptingPlugin::load(dll_path, config))
            };

            match load_result {
                Ok(plugin) => {
                    println!("[ENGINE] ✓ Lua script plugin loaded from '{}'", used_path);
                    self.native_scripts.insert(dll_path.to_string(), plugin);
                }
                Err(e) => {
                    eprintln!("[ENGINE] Failed to load Lua script plugin '{}': {}", used_path, e);
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        }

        let plugin = self.native_scripts.get_mut(dll_path).unwrap();
        let script_id = plugin
            .create_script_with_source(pack_entity_id(entity), script_source_path)
            .ok_or_else(|| {
                eprintln!(
                    "[ENGINE] load_lua_script('{}', '{}'): create_script_with_source отказал",
                    dll_path, script_source_path
                );
                Error::from_hresult(HRESULT(1))
            })?;

        let handle = ScriptHandle {
            dll_path: dll_path.to_string(),
            script_id,
            entity,
        };

        plugin.dispatch_event(script_id, &crate::plugin::ScriptEvent {
            event_type: crate::plugin::ScriptEventType::Spawned as u32,
            source_entity: pack_entity_id(entity),
            target_entity: pack_entity_id(entity),
            data: [0.0; 4],
        });

        self.active_scripts.push(handle.clone());
        Ok(handle)
    }
    // ПРИМЕЧАНИЕ: dispatch_script_event/unload_native_script/
    // update_native_scripts выше уже полностью годятся и для Lua-скриптов
    // без изменений — `ScriptHandle` не различает происхождение
    // (Native/Lua), обе разновидности живут в одном и том же
    // `self.native_scripts`/`self.active_scripts` и используют идентичный
    // ScriptingAPI C-ABI (см. plugin/scripting_api.rs) для update/event/
    // destroy — отдельного `unload_lua_script` не требуется.

    // ===================================================================
    // ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload)
    // ===================================================================

    /// Прикрепляет Python-скрипт из `script_source_path` к сущности
    /// `entity` — В ОТЛИЧИЕ от Native/Lua здесь нет никакой DLL: движок
    /// сам, через встроенный интерпретатор (см. engine/scripting_python.rs),
    /// загружает и исполняет файл напрямую. Ключ в `self.python_scripts` —
    /// сама `entity` (см. комментарий у поля выше про упрощение "одна
    /// сущность — одно Python-прикрепление").
    pub fn load_python_script(
        &mut self,
        script_source_path: &str,
        entity: crate::scene::EntityId,
    ) -> Result<()> {
        let runtime = PythonScriptRuntime::new(entity, script_source_path);
        if runtime.has_error() {
            eprintln!(
                "[ENGINE] load_python_script('{}'): ошибка загрузки: {}",
                script_source_path,
                runtime.last_error().unwrap_or("неизвестная ошибка")
            );
            // Загрузка с ошибкой НЕ считается фатальной для самого
            // `load_python_script` (в отличие от Native/Lua, где
            // отсутствие DLL — Err) — тот же принцип hot-reload:
            // синтаксическая ошибка в файле не должна ронять игру, файл
            // можно исправить прямо во время работы, и hot-reload
            // подхватит исправление на следующем кадре сам. Прикрепление
            // всё равно регистрируется (бездействующим), чтобы hot-reload
            // впоследствии заработал без повторного вызова load_python_script.
        }
        self.python_scripts.insert(entity, runtime);
        Ok(())
    }

    /// Открепляет Python-скрипт от сущности — в отличие от
    /// `unload_native_script`, нет отдельного "уничтожения прикрепления
    /// внутри DLL" (никакой DLL нет) — просто убирает `PythonScriptRuntime`
    /// из `self.python_scripts`, что дропает его Python-состояние.
    pub fn unload_python_script(&mut self, entity: crate::scene::EntityId) {
        self.python_scripts.remove(&entity);
    }

    /// Отправляет событие Python-скрипту сущности (см. `on_event` в
    /// bobber.py) — прямой аналог `dispatch_script_event` для Native/Lua,
    /// но без промежуточного `ScriptHandle` (ключ — сама сущность).
    pub fn dispatch_python_event(&mut self, entity: crate::scene::EntityId, event_type: u32, data: [f32; 4]) {
        if let Some(runtime) = self.python_scripts.get_mut(&entity) {
            runtime.call_on_event(event_type, data);
        }
    }

    /// Вызывается КАЖДЫЙ кадр из `update()` — симметрично
    /// `update_native_scripts`, но проще: нет отдельной `ScriptContext`
    /// C-структуры (Python вызывается напрямую как Rust-функция, без
    /// FFI-границы) — `PythonScriptRuntime::call_update` сама делает
    /// проверку hot-reload и возвращает новую позицию, если скрипт её
    /// изменил.
    pub(crate) fn update_python_scripts(&mut self, dt: f32) {
        // Собираем список сущностей заранее — избегаем одновременного
        // `&mut self.python_scripts` (цикл) и `&mut self.scene`
        // (Transform) заимствований одного и того же `&mut self`.
        let entities: Vec<crate::scene::EntityId> = self.python_scripts.keys().copied().collect();

        for entity in entities {
            let (position, rotation) = match self.scene.transform(entity) {
                Some(t) => (t.position, t.rotation),
                None => continue, // сущность despawn'нута — тихо пропускаем, как и у Native
            };

            let new_position = {
                let Some(runtime) = self.python_scripts.get_mut(&entity) else { continue };
                runtime.call_update(dt, position, rotation)
            };

            if let Some(new_position) = new_position {
                if let Some(t) = self.scene.transform_mut(entity) {
                    t.position = new_position;
                }
            }
        }
    }
}
