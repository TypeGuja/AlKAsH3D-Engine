// src/converters/alasm.rs
//
// Экспорт выделенного объекта сцены в настоящий `.alasm` движка
// (alkash3d_rs::AlasmFile) — граф разбираемых физических деталей, см.
// подробное описание формата в alkash3d-rust/src/alasm_format.rs.
//
// ИЗВЕСТНОЕ ОГРАНИЧЕНИЕ: `Scene` эдитора плоская (`HashMap<Uuid,
// GameObject>`, без parent/child — см. scene/scene.rs) — у неё физически
// нет данных, чтобы построить дерево деталей с несколькими уровнями
// вложенности и joint-соединениями между ними. Экспорт честно отражает
// это: получившийся `.alasm` содержит РОВНО ОДНУ корневую деталь
// (`parent_index == -1`), без потомков — валидная, но вырожденная сборка
// ("одна деталь = вся сборка"). Настоящее дерево потребует сначала
// добавить иерархию объектов в саму сцену эдитора — отдельная, более
// крупная задача, не входящая в охват "реальный экспорт в форматы".

use anyhow::{anyhow, Result};
use std::path::Path;

use crate::scene::{ObjectType, Scene};

use alkash3d_rs::{AlasmFile, AssemblyCategory, PartRecord};

use super::altex::build_altex;

/// Экспортирует выделенный меш-объект как однодетальную сборку: сохраняет
/// его геометрию рядом как `<name>.altex` и `.alasm`, ссылающийся на неё
/// через единственную корневую деталь.
pub fn export_selected_to_alasm(scene: &Scene, category: AssemblyCategory, alasm_path: &str) -> Result<()> {
    let &id = scene
        .selected_ids
        .first()
        .ok_or_else(|| anyhow!("Выделите меш-объект для экспорта в .alasm"))?;
    let obj = scene
        .get_object(id)
        .ok_or_else(|| anyhow!("Выделенный объект не найден в сцене"))?;
    let ObjectType::Mesh(m) = &obj.object_type else {
        return Err(anyhow!("Выделенный объект не является мешем"));
    };

    let alasm_path = Path::new(alasm_path);
    let altex_path = alasm_path.with_extension("altex");

    let altex = build_altex(&m.mesh, &m.material, &obj.name);
    altex
        .save(altex_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow!("Не удалось сохранить геометрию '{}': {}", altex_path.display(), e))?;

    let mut asm = AlasmFile::new(&obj.name, category);
    let mesh_path_id = asm.add_string(&altex_path.to_string_lossy());
    asm.add_root_part(PartRecord {
        mass: 1.0,
        mesh_path_id,
        ..Default::default()
    });

    asm.save(alasm_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow!("Не удалось сохранить .alasm '{}': {}", alasm_path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::Material;
    use crate::mesh::Mesh as EditorMesh;
    use crate::scene::{GameObject, MeshComponent};

    #[test]
    fn export_then_load_has_single_root_part() {
        let mut scene = Scene::new("TestAssembly");
        let obj = GameObject::new(
            "EngineBlock",
            ObjectType::Mesh(MeshComponent {
                mesh: EditorMesh::create_cube(),
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        );
        let id = scene.add_object(obj);
        scene.select(id, false);

        let dir = std::env::temp_dir().join("alkash3d_editor_alasm_test");
        let _ = std::fs::create_dir_all(&dir);
        let alasm_path = dir.join("assembly.alasm");
        let alasm_path_str = alasm_path.to_string_lossy().to_string();

        export_selected_to_alasm(&scene, AssemblyCategory::Engine, &alasm_path_str).expect("export");
        let loaded = AlasmFile::load(&alasm_path_str).expect("load");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(loaded.parts.len(), 1);
        assert_eq!(loaded.root_index(), Some(0));
        assert_eq!(loaded.metadata.category(), AssemblyCategory::Engine);
    }
}
