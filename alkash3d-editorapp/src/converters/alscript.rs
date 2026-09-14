// src/converters/alscript.rs
//
// Экспорт ScriptedEntity-объектов сцены в настоящий `.alscript` движка
// (alkash3d_rs::AlscriptFile). Тип скрипта (Python hot-reload / Native DLL)
// определяется по расширению `ScriptedEntityComponent::script_name` — сама
// сцена эдитора не хранит владеет ли объект отдельно путём к Lua-рантайм-
// плагину, так что `.lua` скрипты пока регистрируются как Native с явным
// предупреждением в лог, а не тихо (см. `AlscriptFile::register_lua_script`
// в движке — ему помимо пути к .lua нужен ЕЩЁ путь к самому рантайм-DLL,
// которого в `ScriptedEntityComponent` попросту нет).

use anyhow::{anyhow, Result};

use crate::scene::{GameObject, ObjectType, Scene, ScriptedEntityComponent};

use alkash3d_rs::{AlscriptFile, ScriptDescriptor};

// ДОБАВЛЕНО (по прямому запросу пользователя: "делай то же самое под
// другие форматы", см. `converters/alsnd.rs` про паттерн): автономная
// редактируемая модель реестра скриптов, не завязанная на объекты сцены —
// см. `ui/script_editor.rs`. Строится сама, а не через `register_*`
// (те заточены под КОНКРЕТНЫЙ сценарий вызова из кода, а не под "взять
// произвольные значения из формы и разложить по нужным полям дескриптора",
// см. подробности раскладки `source_offset`/`native_dll_path_id` в шапке
// `alscript_format.rs`).
#[derive(Debug, Clone)]
pub struct ScriptEdit {
    pub name: String,
    /// 0=Python(hot-reload), 1=Lua(DLL), 2=Native(DLL) — см. шапку
    /// `alscript_format.rs`. 3/4 зарезервированы и намеренно не
    /// предлагаются в UI (движок их не реализует).
    pub script_type: u32,
    /// Путь к .py/.lua исходнику (type 0/1) или к DLL (type 2).
    pub path: String,
    pub hot_reloadable: bool,
    pub priority: i32,
    /// `ScriptDescriptor::run_on_thread` — тип поля в движке `u32` (см.
    /// `alscript_format.rs`), несмотря на doc-комментарий там же "-1=main
    /// thread" (устаревший/неточный, физически невозможен для `u32` — само
    /// поле нигде в движке пока не читается, так что менять его тип не
    /// входит в охват этой правки); существующий код движка (например
    /// `register_native_script`) сам всегда пишет туда `0`, так что `0` и
    /// здесь используется как практический дефолт "основной поток".
    pub run_on_thread: u32,
}

impl Default for ScriptEdit {
    fn default() -> Self {
        Self { name: String::new(), script_type: 0, path: String::new(), hot_reloadable: true, priority: 0, run_on_thread: 0 }
    }
}

pub fn build_alscript_file(entries: &[ScriptEdit]) -> AlscriptFile {
    let mut file = AlscriptFile::new();
    for e in entries {
        let name = e.name.trim();
        if name.is_empty() {
            continue;
        }
        let name_id = file.add_string(name);
        let path_id = file.add_string(&e.path);
        // Python/Lua держат путь в `source_offset` (переиспользован как id
        // строки), Native — в `native_dll_path_id` — см. `register_*` в
        // `alscript_format.rs` и комментарий у `import_alscript_to_scene`
        // ниже про ту же раскладку в обратную сторону.
        let (source_offset, native_dll_path_id) = match e.script_type {
            0 | 1 => (path_id as u64, alkash3d_rs::NONE_ID),
            _ => (0, path_id),
        };
        file.scripts.push(ScriptDescriptor {
            name_id,
            script_type: e.script_type,
            compilation_mode: if e.script_type == 0 { 0 } else { 1 },
            bytecode_offset: 0,
            bytecode_size: 0,
            source_offset,
            source_size: 0,
            dependencies_count: 0,
            dependency_ids_offset: 0,
            hot_reloadable: if e.hot_reloadable { 1 } else { 0 },
            run_on_thread: e.run_on_thread,
            priority: e.priority,
            owner_entity_id: 0,
            native_dll_path_id,
        });
        file.header.script_count += 1;
    }
    file
}

pub fn save_scripts(entries: &[ScriptEdit], path: &str) -> Result<usize> {
    let file = build_alscript_file(entries);
    let count = file.scripts.len();
    if count == 0 {
        return Err(anyhow!("Нет ни одного скрипта с именем — сохранять нечего"));
    }
    file.save(path).map_err(|e| anyhow!("Не удалось сохранить .alscript '{}': {}", path, e))?;
    Ok(count)
}

pub fn load_scripts(path: &str, log: &mut dyn FnMut(String)) -> Result<Vec<ScriptEdit>> {
    let file = AlscriptFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alscript '{}': {}", path, e))?;
    let mut out = Vec::with_capacity(file.scripts.len());
    for s in &file.scripts {
        let name = file.get_string(s.name_id).to_string();
        let script_path = match s.script_type {
            0 | 1 => file.get_string(s.source_offset as u32).to_string(),
            2 => file.get_string(s.native_dll_path_id).to_string(),
            other => {
                log(format!("⚠️ '{}': script_type={} не поддерживается движком (зарезервирован) — пропущен", name, other));
                continue;
            }
        };
        out.push(ScriptEdit {
            name,
            script_type: s.script_type,
            path: script_path,
            hot_reloadable: s.hot_reloadable != 0,
            priority: s.priority,
            run_on_thread: s.run_on_thread,
        });
    }
    Ok(out)
}

pub fn export_scene_to_alscript(scene: &Scene, log: &mut dyn FnMut(String)) -> AlscriptFile {
    let mut file = AlscriptFile::new();

    for obj in scene.objects.values() {
        let ObjectType::ScriptedEntity(s) = &obj.object_type else { continue };
        if !s.enabled {
            continue;
        }

        let ext = std::path::Path::new(&s.script_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "py" => {
                file.register_python_script_path(&obj.name, &s.script_name, 0);
            }
            "dll" => {
                file.register_native_script(&obj.name, &s.script_name, 0);
            }
            "lua" => {
                log(format!(
                    "⚠️ '{}': Lua-скрипт '{}' зарегистрирован как Native (движку для Lua также нужен путь к рантайм-DLL alkash3d-luascript, которого нет в объекте сцены) — поправьте .alscript вручную при необходимости",
                    obj.name, s.script_name
                ));
                file.register_native_script(&obj.name, &s.script_name, 0);
            }
            _ => {
                log(format!(
                    "⚠️ '{}': не удалось определить тип скрипта по расширению '{}' — зарегистрирован как Native as-is",
                    obj.name, s.script_name
                ));
                file.register_native_script(&obj.name, &s.script_name, 0);
            }
        }
    }

    file
}

pub fn export_scene_to_alscript_file(scene: &Scene, path: &str, log: &mut dyn FnMut(String)) -> Result<usize> {
    let file = export_scene_to_alscript(scene, log);
    let count = file.header.script_count as usize;
    if count == 0 {
        return Err(anyhow!("В сцене нет включённых ScriptedEntity-объектов — экспортировать нечего"));
    }
    file.save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alscript '{}': {}", path, e))?;
    Ok(count)
}

/// Импорт `.alscript` — по одному `ScriptedEntity` на каждый
/// `ScriptDescriptor` (зеркально `export_scene_to_alscript` выше). Путь
/// скрипта восстанавливается по `script_type` (см. подробное описание
/// раскладки полей в шапке `alscript_format.rs`): Python/Lua держат путь в
/// `source_offset` (переиспользован как id строки), Native — в
/// `native_dll_path_id`. `script_type` 3/4 зарезервированы и движком не
/// реализованы (см. шапку файла) — пропускаются с предупреждением, а не
/// падением, тем же принципом, что и остальные импортёры этого модуля.
pub fn import_alscript_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let file = AlscriptFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alscript '{}': {}", path, e))?;
    if file.scripts.is_empty() {
        return Err(anyhow!("В '{}' нет зарегистрированных скриптов", path));
    }

    let mut scene = Scene::new("ImportedScripts");
    for s in &file.scripts {
        let name = file.get_string(s.name_id);
        let script_path = match s.script_type {
            0 | 1 => file.get_string(s.source_offset as u32), // Python / Lua
            2 => file.get_string(s.native_dll_path_id),       // Native
            other => {
                log(format!("⚠️ '{}': script_type={} не поддерживается движком (зарезервирован) — пропущен", name, other));
                continue;
            }
        };
        if script_path.is_empty() {
            log(format!("⚠️ '{}': путь к скрипту пуст — пропущен", name));
            continue;
        }
        scene.add_object(GameObject::new(
            name,
            ObjectType::ScriptedEntity(ScriptedEntityComponent {
                script_name: script_path.to_string(),
                enabled: true,
            }),
        ));
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{GameObject, ScriptedEntityComponent};

    #[test]
    fn export_then_load_preserves_python_script() {
        let mut scene = Scene::new("TestScripts");
        scene.add_object(GameObject::new(
            "AI",
            ObjectType::ScriptedEntity(ScriptedEntityComponent {
                script_name: "ai_controller.py".to_string(),
                enabled: true,
            }),
        ));

        let path = std::env::temp_dir().join("alkash3d_editor_alscript_test.alscript");
        let path_str = path.to_string_lossy().to_string();

        let mut logs = Vec::new();
        let count = export_scene_to_alscript_file(&scene, &path_str, &mut |m| logs.push(m)).expect("export");
        assert_eq!(count, 1, "logs: {:?}", logs);

        let loaded = AlscriptFile::load(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.scripts.len(), 1);
        assert_eq!(loaded.scripts[0].script_type, 0); // Python
    }

    #[test]
    fn import_recreates_scripted_entity_objects() {
        let mut scene = Scene::new("TestScripts");
        scene.add_object(GameObject::new(
            "AI",
            ObjectType::ScriptedEntity(ScriptedEntityComponent {
                script_name: "ai_controller.py".to_string(),
                enabled: true,
            }),
        ));

        let path = std::env::temp_dir().join("alkash3d_editor_alscript_import_test.alscript");
        let path_str = path.to_string_lossy().to_string();
        let mut export_logs = Vec::new();
        export_scene_to_alscript_file(&scene, &path_str, &mut |m| export_logs.push(m)).expect("export");

        let mut logs = Vec::new();
        let imported = import_alscript_to_scene(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.objects.len(), 1, "logs: {:?}", logs);
        let obj = imported.objects.values().next().unwrap();
        let ObjectType::ScriptedEntity(s) = &obj.object_type else { panic!("expected ScriptedEntity") };
        assert_eq!(s.script_name, "ai_controller.py");
    }

    #[test]
    fn script_editor_round_trip() {
        let entries = vec![
            ScriptEdit { name: "AI".to_string(), script_type: 0, path: "ai.py".to_string(), hot_reloadable: true, priority: 5, run_on_thread: 0 },
            ScriptEdit { name: "PhysicsPlugin".to_string(), script_type: 2, path: "physics.dll".to_string(), hot_reloadable: false, priority: -1, run_on_thread: 1 },
        ];

        let path = std::env::temp_dir().join("alkash3d_editor_script_editor_test.alscript");
        let path_str = path.to_string_lossy().to_string();
        let count = save_scripts(&entries, &path_str).expect("save");
        assert_eq!(count, 2);

        let mut logs = Vec::new();
        let loaded = load_scripts(&path_str, &mut |m| logs.push(m)).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.len(), 2, "logs: {:?}", logs);
        assert_eq!(loaded[0].path, "ai.py");
        assert!(loaded[0].hot_reloadable);
        assert_eq!(loaded[1].script_type, 2);
        assert_eq!(loaded[1].path, "physics.dll");
        assert!(!loaded[1].hot_reloadable);
    }
}
