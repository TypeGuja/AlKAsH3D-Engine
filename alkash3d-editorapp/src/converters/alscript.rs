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

use crate::scene::{ObjectType, Scene};

use alkash3d_rs::AlscriptFile;

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
}
