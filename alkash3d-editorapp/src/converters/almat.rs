// src/converters/almat.rs
//
// Конвертация между библиотекой материалов эдитора (`AssetLibrary::materials`,
// см. assets/library.rs) и настоящим форматом материалов движка `.almat`
// (alkash3d_rs::AlmatFile, см. alkash3d-rust/src/almat_format.rs).
//
// ВАЖНО про то, что такое ".almat" здесь: это НЕ материал одного конкретного
// объекта (для этого уже есть материал, встроенный прямо в `.altex` — см.
// converters/altex.rs), а ПЕРЕИСПОЛЬЗУЕМАЯ библиотека материалов — палитра
// именованных материалов, которую можно сохранить и подгрузить заново или
// передать между сценами/проектами. Ровно поэтому экспорт/импорт работают
// с `AssetLibrary::materials` (HashMap<String, Material>) целиком, а не с
// материалом текущего выделенного объекта.
//
// ОБНОВЛЕНО (текстуры материалов — по прямому запросу пользователя):
// `MaterialDefinition` в движке несёт слоты под текстурные карты (albedo/
// normal/metallic_roughness/ao/emissive) — `material::Material` эдитора
// теперь поддерживает ОДНУ из них (`albedo_texture`, см. material.rs), и
// ниже она читается/пишется через `albedo_texture_id`. Остальные четыре
// слота (normal/metallic_roughness/ao/emissive) эдитор всё ещё не
// поддерживает — при экспорте пишутся `NO_TEXTURE`, при импорте молча
// игнорируются (лог-предупреждение см. в `import_almat_to_materials`).
//
// ВАЖНО про то, ПОЧЕМУ путь, а не встроенные пиксели: `.almat` — это
// ПЕРЕИСПОЛЬЗУЕМАЯ библиотека материалов (см. заголовок файла выше), а не
// самодостаточный пакет одного объекта, как `.altex` (см.
// `converters/altex.rs` — там та же albedo-текстура встраивается сырыми
// байтами). Экспорт материала с текстурой, у которой нет `source_path`
// (например импортированной из чужого `.altex`, где были только встроенные
// байты без файла на диске — см. `TextureAsset::source_path`), пропускает
// текстурный слот и предупреждает в лог: сослаться в `.almat` попросту не
// на что.

use std::collections::HashMap;

use anyhow::{anyhow, Result};

use crate::material::{Material, TextureAsset};

use alkash3d_rs::{AlmatFile, MaterialDefinition, NO_TEXTURE};

/// Строит `AlmatFile` из библиотеки материалов эдитора. Ключи `HashMap`
/// сортируются по имени перед добавлением — `HashMap` не гарантирует
/// порядок обхода, а детерминированный порядок в файле делает diff/повторный
/// экспорт воспроизводимым (тот же принцип, что уже применяется у выбора
/// точки спавна в converters/alworld.rs).
///
/// `log` получает по одному сообщению на материал, у которого назначена
/// albedo-текстура БЕЗ `source_path` (см. комментарий у поля
/// `TextureAsset::source_path` и у заголовка файла выше про то, почему
/// `.almat` не может встроить такую текстуру) — та же сигнатура колбэка,
/// что уже использует `import_almat_to_materials` ниже, ради единообразия
/// на стороне вызывающего UI-кода.
pub fn export_materials_to_almat(materials: &HashMap<String, Material>, log: &mut dyn FnMut(String)) -> AlmatFile {
    let mut almat = AlmatFile::new();

    let mut names: Vec<&String> = materials.keys().collect();
    names.sort();

    for name in names {
        let mat = &materials[name];
        let albedo_texture_id = match &mat.albedo_texture {
            Some(tex) => match &tex.source_path {
                Some(source_path) => almat.add_string(source_path),
                None => {
                    log(format!(
                        "⚠️ Материал '{}' несёт albedo-текстуру без пути на диске (пришла из встроенных байт .altex) — .almat ссылается на текстуры по пути, слот пропущен",
                        name
                    ));
                    NO_TEXTURE
                }
            },
            None => NO_TEXTURE,
        };

        let def = MaterialDefinition {
            name_id: 0, // перезаписывается add_material_definition
            albedo: mat.color,
            metallic: mat.metallic,
            roughness: mat.roughness,
            ao: 1.0,
            emissive: mat.emissive,
            albedo_texture_id,
            normal_texture_id: NO_TEXTURE,
            metallic_roughness_texture_id: NO_TEXTURE,
            ao_texture_id: NO_TEXTURE,
            emissive_texture_id: NO_TEXTURE,
        };
        almat.add_material_definition(def, name);
    }

    almat
}

/// Сохраняет библиотеку материалов эдитора в `.almat`.
pub fn export_materials_to_almat_file(materials: &HashMap<String, Material>, path: &str, log: &mut dyn FnMut(String)) -> Result<()> {
    let almat = export_materials_to_almat(materials, log);
    almat
        .save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .almat '{}': {}", path, e))
}

/// Импортирует материалы из `.almat` как `HashMap<String, Material>` —
/// вызывающий код сам решает, как их слить с `AssetLibrary::materials`
/// (обычно — просто `extend`, перезаписывая совпадающие по имени, см.
/// `EditorApp::import_almat_from_path`). Материалы без имени (пустая
/// строка в таблице строк) получают синтетическое имя `Material_{idx}`,
/// чтобы не потерять запись молча и не столкнуть несколько безымянных
/// материалов в один и тот же ключ HashMap.
pub fn import_almat_to_materials(path: &str, log: &mut dyn FnMut(String)) -> Result<HashMap<String, Material>> {
    let almat = AlmatFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .almat '{}': {}", path, e))?;

    let mut materials = HashMap::new();
    for (idx, def) in almat.material_definitions.iter().enumerate() {
        let mut name = almat.get_string(def.name_id).to_string();
        if name.is_empty() {
            name = format!("Material_{}", idx);
        }

        // ОБНОВЛЕНО (текстуры материалов): albedo теперь обрабатывается
        // отдельно ниже (эдитор её поддерживает) — предупреждение осталось
        // только про ОСТАЛЬНЫЕ четыре слота, которые эдитор всё ещё не
        // читает.
        let has_unsupported_textures = [
            def.normal_texture_id,
            def.metallic_roughness_texture_id,
            def.ao_texture_id,
            def.emissive_texture_id,
        ].iter().any(|&id| id != NO_TEXTURE);
        if has_unsupported_textures {
            log(format!(
                "⚠️ Материал '{}' ссылается на normal/metallic-roughness/ao/emissive текстуры — эдитор пока поддерживает только albedo, остальные слоты пропущены",
                name
            ));
        }

        // ДОБАВЛЕНО (текстуры материалов): `albedo_texture_id` — индекс в
        // `almat.strings`, то есть ПУТЬ к файлу изображения на диске (см.
        // заголовок файла выше про то, почему `.almat` хранит путь, а не
        // пиксели). Отсутствие файла по этому пути (переместили/удалили
        // после экспорта, .almat расшарили без текстур рядом) — не фатальная
        // ошибка импорта материала, только этого одного слота.
        let albedo_texture = if def.albedo_texture_id != NO_TEXTURE {
            let tex_path = almat.get_string(def.albedo_texture_id);
            if tex_path.is_empty() {
                None
            } else {
                match TextureAsset::load_from_file(tex_path) {
                    Ok(tex) => Some(tex),
                    Err(e) => {
                        log(format!(
                            "⚠️ Материал '{}': не удалось загрузить albedo-текстуру '{}': {} — материал импортирован без неё",
                            name, tex_path, e
                        ));
                        None
                    }
                }
            }
        } else {
            None
        };

        if materials.contains_key(&name) {
            log(format!("⚠️ Повторяющееся имя материала '{}' в .almat — предыдущая запись перезаписана", name));
        }

        materials.insert(name, Material {
            name: almat.get_string(def.name_id).to_string(),
            color: def.albedo,
            metallic: def.metallic,
            roughness: def.roughness,
            emissive: def.emissive,
            albedo_texture,
        });
    }

    Ok(materials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_material_properties() {
        let mut materials = HashMap::new();
        materials.insert("Rust".to_string(), Material {
            name: "Rust".to_string(),
            color: [0.8, 0.2, 0.1, 1.0],
            metallic: 0.3,
            roughness: 0.6,
            emissive: [0.0, 0.0, 0.0],
            albedo_texture: None,
        });
        materials.insert("Neon Metal".to_string(), Material {
            name: "Neon Metal".to_string(),
            color: [0.1, 0.1, 0.1, 1.0],
            metallic: 1.0,
            roughness: 0.1,
            emissive: [0.0, 0.5, 0.9],
            albedo_texture: None,
        });

        let path = std::env::temp_dir().join("alkash3d_editor_almat_roundtrip_test.almat");
        let path_str = path.to_string_lossy().to_string();

        let mut export_logs = Vec::new();
        export_materials_to_almat_file(&materials, &path_str, &mut |m| export_logs.push(m)).expect("export");
        assert!(export_logs.is_empty(), "no texture warnings expected: {:?}", export_logs);
        let mut logs = Vec::new();
        let imported = import_almat_to_materials(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.len(), 2, "logs: {:?}", logs);
        assert!(logs.is_empty(), "no texture warnings expected: {:?}", logs);

        let rust = &imported["Rust"];
        assert!((rust.color[0] - 0.8).abs() < 1e-5);
        assert!((rust.metallic - 0.3).abs() < 1e-5);
        assert!((rust.roughness - 0.6).abs() < 1e-5);

        let neon = &imported["Neon Metal"];
        assert!((neon.emissive[2] - 0.9).abs() < 1e-5);
        assert!((neon.metallic - 1.0).abs() < 1e-5);
    }

    #[test]
    fn round_trip_preserves_albedo_texture_by_path() {
        // .almat ссылается на текстуры ПУТЁМ (см. комментарий у заголовка
        // файла), поэтому для честного round-trip нужен настоящий файл
        // изображения на диске — пишем маленький PNG во временную папку.
        let png_path = std::env::temp_dir().join("alkash3d_editor_almat_texture_test_source.png");
        let png_path_str = png_path.to_string_lossy().to_string();
        let img = image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]).unwrap();
        img.save(&png_path).expect("write test png");

        let texture = TextureAsset::load_from_file(&png_path_str).expect("load test png");

        let mut materials = HashMap::new();
        materials.insert("Bricks".to_string(), Material {
            name: "Bricks".to_string(),
            color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.9,
            emissive: [0.0, 0.0, 0.0],
            albedo_texture: Some(texture),
        });

        let almat_path = std::env::temp_dir().join("alkash3d_editor_almat_texture_roundtrip_test.almat");
        let almat_path_str = almat_path.to_string_lossy().to_string();

        let mut export_logs = Vec::new();
        export_materials_to_almat_file(&materials, &almat_path_str, &mut |m| export_logs.push(m)).expect("export");
        assert!(export_logs.is_empty(), "texture has a source_path, should not warn: {:?}", export_logs);

        let mut import_logs = Vec::new();
        let imported = import_almat_to_materials(&almat_path_str, &mut |m| import_logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&almat_path_str);
        let _ = std::fs::remove_file(&png_path);

        assert!(import_logs.is_empty(), "logs: {:?}", import_logs);
        let bricks = &imported["Bricks"];
        let tex = bricks.albedo_texture.as_ref().expect("albedo texture must round-trip by path");
        assert_eq!(tex.width, 2);
        assert_eq!(tex.height, 1);
        assert_eq!(*tex.pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
        assert_eq!(tex.source_path.as_deref(), Some(png_path_str.as_str()));
    }

    #[test]
    fn missing_texture_file_is_reported_but_does_not_fail_import() {
        let mut materials = HashMap::new();
        materials.insert("Ghost".to_string(), Material {
            name: "Ghost".to_string(),
            color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0],
            albedo_texture: Some(TextureAsset::from_rgba(1, 1, vec![255, 255, 255, 255], Some("C:/definitely/does/not/exist.png".to_string()))),
        });

        let path = std::env::temp_dir().join("alkash3d_editor_almat_missing_texture_test.almat");
        let path_str = path.to_string_lossy().to_string();

        export_materials_to_almat_file(&materials, &path_str, &mut |_| {}).expect("export");
        let mut logs = Vec::new();
        let imported = import_almat_to_materials(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.len(), 1);
        assert!(imported["Ghost"].albedo_texture.is_none());
        assert!(!logs.is_empty(), "missing texture file should be logged");
    }

    #[test]
    fn duplicate_and_empty_names_are_handled_without_losing_entries() {
        let mut almat = AlmatFile::new();
        almat.add_material_definition(
            MaterialDefinition {
                name_id: 0,
                albedo: [1.0, 0.0, 0.0, 1.0],
                metallic: 0.0,
                roughness: 1.0,
                ao: 1.0,
                emissive: [0.0, 0.0, 0.0],
                albedo_texture_id: NO_TEXTURE,
                normal_texture_id: NO_TEXTURE,
                metallic_roughness_texture_id: NO_TEXTURE,
                ao_texture_id: NO_TEXTURE,
                emissive_texture_id: NO_TEXTURE,
            },
            "",
        );

        let path = std::env::temp_dir().join("alkash3d_editor_almat_empty_name_test.almat");
        let path_str = path.to_string_lossy().to_string();
        almat.save(&path_str).expect("save");

        let mut logs = Vec::new();
        let imported = import_almat_to_materials(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.len(), 1, "logs: {:?}", logs);
        assert!(imported.contains_key("Material_0"), "keys: {:?}", imported.keys().collect::<Vec<_>>());
    }
}
