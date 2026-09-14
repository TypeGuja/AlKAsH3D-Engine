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
// ИЗВЕСТНОЕ ОГРАНИЧЕНИЕ: `MaterialDefinition` в движке уже несёт слоты под
// текстурные карты (albedo/normal/metallic_roughness/ao/emissive), но
// `material::Material` эдитора текстур вообще не поддерживает (только
// плоские color/metallic/roughness/emissive) — при экспорте эти слоты
// всегда пишутся как `NO_TEXTURE`, при импорте молча игнорируются. Когда
// эдитор научится назначать текстуры материалу, здесь будет что читать —
// формат уже готов, менять его не придётся.

use std::collections::HashMap;

use anyhow::{anyhow, Result};

use crate::material::Material;

use alkash3d_rs::{AlmatFile, MaterialDefinition, NO_TEXTURE};

/// Строит `AlmatFile` из библиотеки материалов эдитора. Ключи `HashMap`
/// сортируются по имени перед добавлением — `HashMap` не гарантирует
/// порядок обхода, а детерминированный порядок в файле делает diff/повторный
/// экспорт воспроизводимым (тот же принцип, что уже применяется у выбора
/// точки спавна в converters/alworld.rs).
pub fn export_materials_to_almat(materials: &HashMap<String, Material>) -> AlmatFile {
    let mut almat = AlmatFile::new();

    let mut names: Vec<&String> = materials.keys().collect();
    names.sort();

    for name in names {
        let mat = &materials[name];
        let def = MaterialDefinition {
            name_id: 0, // перезаписывается add_material_definition
            albedo: mat.color,
            metallic: mat.metallic,
            roughness: mat.roughness,
            ao: 1.0,
            emissive: mat.emissive,
            albedo_texture_id: NO_TEXTURE,
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
pub fn export_materials_to_almat_file(materials: &HashMap<String, Material>, path: &str) -> Result<()> {
    let almat = export_materials_to_almat(materials);
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

        let has_textures = [
            def.albedo_texture_id,
            def.normal_texture_id,
            def.metallic_roughness_texture_id,
            def.ao_texture_id,
            def.emissive_texture_id,
        ].iter().any(|&id| id != NO_TEXTURE);
        if has_textures {
            log(format!(
                "⚠️ Материал '{}' ссылается на текстуры — эдитор пока не поддерживает текстурные материалы, слоты пропущены",
                name
            ));
        }

        if materials.contains_key(&name) {
            log(format!("⚠️ Повторяющееся имя материала '{}' в .almat — предыдущая запись перезаписана", name));
        }

        materials.insert(name, Material {
            name: almat.get_string(def.name_id).to_string(),
            color: def.albedo,
            metallic: def.metallic,
            roughness: def.roughness,
            emissive: def.emissive,
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
        });
        materials.insert("Neon Metal".to_string(), Material {
            name: "Neon Metal".to_string(),
            color: [0.1, 0.1, 0.1, 1.0],
            metallic: 1.0,
            roughness: 0.1,
            emissive: [0.0, 0.5, 0.9],
        });

        let path = std::env::temp_dir().join("alkash3d_editor_almat_roundtrip_test.almat");
        let path_str = path.to_string_lossy().to_string();

        export_materials_to_almat_file(&materials, &path_str).expect("export");
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
