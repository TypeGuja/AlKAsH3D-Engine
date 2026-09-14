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

use crate::math::Vec3;
use crate::material::Material;
use crate::mesh::Mesh as EditorMesh;
use crate::scene::{GameObject, MeshComponent, ObjectType, Scene};

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

// ДОБАВЛЕНО (по прямому запросу пользователя: "делай то же самое под
// другие форматы", см. `converters/alsnd.rs` про паттерн): автономная
// редактируемая модель дерева деталей, НЕ завязанная на объекты сцены (в
// отличие от `export_selected_to_alasm` выше, которая экспортирует РОВНО
// один выделенный меш как вырожденную однодетальную сборку) — см.
// `ui/assembly_editor.rs`. `parent` — индекс РОДИТЕЛЬСКОЙ записи В ЭТОМ ЖЕ
// `Vec<PartEdit>` (UI-уровня ссылка, конвертируется в `PartRecord::parent_index`
// при сохранении); ровно одна деталь без родителя (`parent: None`) — корень.
#[derive(Debug, Clone)]
pub struct PartEdit {
    pub name: String,
    pub parent: Option<usize>,
    /// Путь к `.altex`-геометрии; пусто = куб-заглушка при спавне/импорте.
    pub mesh_path: String,
    pub local_position: [f32; 3],
    pub local_rotation: [f32; 3],
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    /// 0=Ball, 1=Hinge, 2=Fixed, 3=Slider — см. `crate::plugin::joint_type` в движке.
    pub joint_type: i32,
    pub break_impulse_linear: f32,
    pub break_impulse_angular: f32,
}

impl Default for PartEdit {
    fn default() -> Self {
        Self {
            name: String::new(),
            parent: None,
            mesh_path: String::new(),
            local_position: [0.0, 0.0, 0.0],
            local_rotation: [0.0, 0.0, 0.0],
            mass: 1.0,
            friction: 0.5,
            restitution: 0.1,
            joint_type: 2, // Fixed
            break_impulse_linear: 0.0,
            break_impulse_angular: 0.0,
        }
    }
}

pub fn build_alasm_file(name: &str, category: AssemblyCategory, parts: &[PartEdit]) -> AlasmFile {
    let mut file = AlasmFile::new(name, category);
    for p in parts {
        let name_id = if p.name.trim().is_empty() { alkash3d_rs::NONE_ID } else { file.add_string(p.name.trim()) };
        let mesh_path_id = if p.mesh_path.trim().is_empty() { alkash3d_rs::NONE_ID } else { file.add_string(p.mesh_path.trim()) };
        let record = PartRecord {
            parent_index: p.parent.map(|i| i as i32).unwrap_or(-1),
            joint_type: p.joint_type,
            local_position: p.local_position,
            local_rotation: p.local_rotation,
            mass: p.mass,
            friction: p.friction,
            restitution: p.restitution,
            mesh_path_id,
            name_id,
            ..Default::default()
        };
        if p.parent.is_none() {
            file.add_root_part(record);
        } else {
            // `add_child_part` берёт ИНДЕКС родителя В УЖЕ ДОБАВЛЕННОМ
            // `parts` файла — совпадает с индексом в исходном `Vec<PartEdit>`
            // 1-в-1, ПОКА детали добавляются строго по порядку исходного
            // списка (гарантируется этим самым циклом `for p in parts`).
            let parent_idx = p.parent.unwrap();
            file.add_child_part(parent_idx, record);
        }
    }
    file
}

pub fn save_assembly(name: &str, category: AssemblyCategory, parts: &[PartEdit], path: &str) -> Result<usize> {
    if parts.is_empty() {
        return Err(anyhow!("Нет ни одной детали — сохранять нечего"));
    }
    if parts.iter().filter(|p| p.parent.is_none()).count() != 1 {
        return Err(anyhow!("Должна быть ровно одна корневая деталь (без родителя) — сейчас {}", parts.iter().filter(|p| p.parent.is_none()).count()));
    }
    let file = build_alasm_file(name, category, parts);
    file.save(path).map_err(|e| anyhow!("Не удалось сохранить .alasm '{}': {}", path, e))?;
    Ok(file.parts.len())
}

pub fn load_assembly(path: &str) -> Result<(String, AssemblyCategory, Vec<PartEdit>)> {
    let file = AlasmFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alasm '{}': {}", path, e))?;
    let name = file.get_string(file.metadata.name_id).unwrap_or("").to_string();
    let category = file.metadata.category();
    let parts = file.parts.iter().map(|p| PartEdit {
        name: file.get_string(p.name_id).unwrap_or("").to_string(),
        parent: if p.parent_index < 0 { None } else { Some(p.parent_index as usize) },
        mesh_path: file.get_string(p.mesh_path_id).unwrap_or("").to_string(),
        local_position: p.local_position,
        local_rotation: p.local_rotation,
        mass: p.mass,
        friction: p.friction,
        restitution: p.restitution,
        joint_type: p.joint_type,
        break_impulse_linear: p.break_impulse_linear,
        break_impulse_angular: p.break_impulse_angular,
    }).collect();
    Ok((name, category, parts))
}

/// Импорт `.alasm` — по одной детали (`PartRecord`) на объект сцены, с
/// иерархией `parent_index` -> `Scene::set_parent` (та же parent/child
/// модель, что уже использует `.alworld`/`.alroute`). Геометрия детали
/// (`mesh_path_id`) грузится через `import_altex`, если ссылка резолвится;
/// если геометрии нет (`sub_assembly_path_id` — ссылка на другой `.alasm`,
/// рекурсивная загрузка суб-сборок эдитором пока не поддержана; либо файл
/// геометрии не нашёлся) — куб-заглушка с предупреждением в лог, тем же
/// принципом "нет геометрии — не крах", что и у самого движка (см.
/// `world_streaming.rs::load_placeholder_mesh` в alkash3d-rust).
pub fn import_alasm_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let file = AlasmFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alasm '{}': {}", path, e))?;
    if file.parts.is_empty() {
        return Err(anyhow!("В '{}' нет деталей", path));
    }

    let mut scene = Scene::new("ImportedAssembly");
    let mut ids = Vec::with_capacity(file.parts.len());

    for (i, part) in file.parts.iter().enumerate() {
        // ИСПРАВЛЕНО (тест `import_recreates_root_part_with_mesh` показал
        // "Part0" вместо "EngineBlock"): у детали своего имени чаще всего
        // нет — `export_selected_to_alasm` кладёт имя объекта сцены в имя
        // ВСЕЙ СБОРКИ (`AlasmFile::new(&obj.name, ...)` -> `metadata.name_id`),
        // а не в `PartRecord::name_id` конкретной детали (он остаётся
        // `NONE_ID`, см. `PartRecord::default()`). Для корневой детали без
        // своего имени логично унаследовать имя сборки — это и есть тот
        // самый объект, что был экспортирован.
        let name = file.get_string(part.name_id).map(|s| s.to_string())
            .or_else(|| {
                if part.parent_index < 0 {
                    file.get_string(file.metadata.name_id).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| format!("Part{}", i));

        let (mesh, material) = if part.sub_assembly_path_id != alkash3d_rs::NONE_ID {
            log(format!("⚠️ '{}': ссылка на суб-сборку (.alasm) не поддержана импортом эдитора — заглушка-куб", name));
            (EditorMesh::create_cube(), Material::default())
        } else if part.mesh_path_id != alkash3d_rs::NONE_ID {
            let mesh_path = file.get_string(part.mesh_path_id).unwrap_or("");
            match super::altex::import_altex(mesh_path) {
                Ok(mut meshes) if !meshes.is_empty() => {
                    let (_, m, mat) = meshes.remove(0);
                    (m, mat)
                }
                Ok(_) => {
                    log(format!("⚠️ '{}': '{}' не содержит мешей — заглушка-куб", name, mesh_path));
                    (EditorMesh::create_cube(), Material::default())
                }
                Err(e) => {
                    log(format!("⚠️ '{}': не удалось загрузить геометрию '{}' ({}) — заглушка-куб", name, mesh_path, e));
                    (EditorMesh::create_cube(), Material::default())
                }
            }
        } else {
            (EditorMesh::create_cube(), Material::default())
        };

        let mut obj = GameObject::new(&name, ObjectType::Mesh(MeshComponent {
            mesh,
            material,
            visible: true,
            wireframe: false,
            solid: true,
            double_sided: false,
        }));
        obj.transform.position = Vec3::new(part.local_position[0], part.local_position[1], part.local_position[2]);
        obj.transform.rotation = crate::math::Quat::from_euler(part.local_rotation[0], part.local_rotation[1], part.local_rotation[2]);
        ids.push(scene.add_object(obj));
    }

    for (i, part) in file.parts.iter().enumerate() {
        if part.parent_index >= 0 {
            let parent_idx = part.parent_index as usize;
            if let Some(&parent_id) = ids.get(parent_idx) {
                if let Err(e) = scene.set_parent(ids[i], Some(parent_id)) {
                    log(format!("⚠️ Не удалось прикрепить деталь #{} к родителю: {}", i, e));
                }
            } else {
                log(format!("⚠️ Деталь #{}: parent_index={} вне диапазона — оставлена без родителя", i, part.parent_index));
            }
        }
    }

    Ok(scene)
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

    #[test]
    fn import_recreates_root_part_with_mesh() {
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

        let dir = std::env::temp_dir().join("alkash3d_editor_alasm_import_test");
        let _ = std::fs::create_dir_all(&dir);
        let alasm_path = dir.join("assembly.alasm");
        let alasm_path_str = alasm_path.to_string_lossy().to_string();
        export_selected_to_alasm(&scene, AssemblyCategory::Engine, &alasm_path_str).expect("export");

        let mut logs = Vec::new();
        let imported = import_alasm_to_scene(&alasm_path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(imported.objects.len(), 1, "logs: {:?}", logs);
        let obj = imported.objects.values().next().unwrap();
        assert_eq!(obj.name, "EngineBlock");
        assert_eq!(obj.parent, None);
        let ObjectType::Mesh(m) = &obj.object_type else { panic!("expected Mesh") };
        assert_eq!(m.mesh.vertices.len(), EditorMesh::create_cube().vertices.len());
    }

    #[test]
    fn assembly_editor_round_trip_with_hierarchy() {
        let parts = vec![
            PartEdit { name: "Body".to_string(), mass: 800.0, ..Default::default() },
            PartEdit { name: "Wheel_FL".to_string(), parent: Some(0), mass: 15.0, joint_type: 1, local_position: [1.0, 0.0, 2.0], ..Default::default() },
        ];

        let dir = std::env::temp_dir().join("alkash3d_editor_assembly_editor_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("car.alasm");
        let path_str = path.to_string_lossy().to_string();
        let count = save_assembly("TestCar", AssemblyCategory::Vehicle, &parts, &path_str).expect("save");
        assert_eq!(count, 2);

        let (name, category, loaded) = load_assembly(&path_str).expect("load");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(name, "TestCar");
        assert_eq!(category, AssemblyCategory::Vehicle);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Body");
        assert_eq!(loaded[0].parent, None);
        assert_eq!(loaded[1].name, "Wheel_FL");
        assert_eq!(loaded[1].parent, Some(0));
        assert!((loaded[1].local_position[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn save_assembly_rejects_missing_or_multiple_roots() {
        let path = std::env::temp_dir().join("alkash3d_editor_assembly_invalid_test.alasm");
        let path_str = path.to_string_lossy().to_string();

        assert!(save_assembly("Empty", AssemblyCategory::Generic, &[], &path_str).is_err());

        let two_roots = vec![PartEdit::default(), PartEdit::default()];
        assert!(save_assembly("TwoRoots", AssemblyCategory::Generic, &two_roots, &path_str).is_err());
    }
}
