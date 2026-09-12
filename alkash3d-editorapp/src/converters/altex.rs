// src/converters/altex.rs
//
// Конвертация между внутренним представлением эдитора (crate::mesh::Mesh +
// crate::material::Material) и настоящим форматом движка `.altex`
// (alkash3d_rs::AltexFile — та же структура, что движок читает в рантайме,
// см. alkash3d-rust/src/altex_format.rs). Экспортированный отсюда файл
// побайтово совместим с тем, что понимает engine/world_streaming.rs, так
// как используется РЕАЛЬНЫЙ `AltexFile::save`/`load` движка, а не отдельная
// переизобретённая реализация формата.

use anyhow::{anyhow, Result};

use crate::material::Material as EditorMaterial;
use crate::math::Vec3;
use crate::mesh::Mesh as EditorMesh;

// NB: `Transform` не может быть импортирован по короткому имени —
// `alkash3d_rs::Transform` неоднозначен между `altex_format::Transform` и
// `math::Transform` (оба реэкспортированы в корень крейта движка), поэтому
// здесь и ниже используется полный путь через модуль (см. комментарий у
// `pub mod altex_format;` в alkash3d-rust/src/lib.rs).
use alkash3d_rs::altex_format::{
    AltexFile, Material as AltexMaterial, Transform as AltexTransform, Vertex as AltexVertex,
};

/// Строит `AltexFile` из одного меша + материала эдитора — единственный
/// объект в сцене файла, с единичной (identity) трансформацией: .altex сам
/// по себе — формат геометрии ОДНОГО объекта/типового меша (см. комментарий
/// у `alworld_format.rs::ChunkObjectHeader` — размещение в мире хранится
/// отдельно, в .alwchunk, объект .altex ссылается только по пути).
///
/// UV/тангенты у `crate::mesh::Mesh` эдитора нет (см. mesh/mesh.rs) —
/// пишем нейтральные заглушки (uv=(0,0), tangent/bitangent — оси X/Y),
/// геометрия и нормали передаются как есть.
pub fn build_altex(mesh: &EditorMesh, material: &EditorMaterial, name: &str) -> AltexFile {
    let mut altex = AltexFile::new();

    let vertices: Vec<AltexVertex> = (0..mesh.vertices.len())
        .map(|i| {
            let p = mesh.vertices[i];
            let n = mesh.normals.get(i).copied().unwrap_or(Vec3::UP);
            AltexVertex {
                position: [p.x, p.y, p.z],
                normal: [n.x, n.y, n.z],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
                uv: [0.0, 0.0],
                uv2: [0.0, 0.0],
                color: [1.0, 1.0, 1.0, 1.0],
            }
        })
        .collect();

    let mesh_id = altex.add_mesh(vertices, mesh.indices.clone(), name);

    // ИСПРАВЛЕНО (metallic/roughness/emissive терялись при экспорте):
    // `AltexFile::add_material()` в движке — узкий хелпер, он хардкодит
    // `metallic: 0.0, roughness: 0.8` независимо от переданных аргументов
    // (см. altex_format.rs::AltexFile::add_material — берёт только albedo и
    // albedo_map). Поля `materials`/`strings` у `AltexFile` публичные —
    // пишем полную `Material` напрямую, чтобы реальные значения материала
    // эдитора действительно попадали в файл.
    let name_id = altex.add_string(&material.name);
    let mat_id = altex.materials.len() as u32;
    altex.materials.push(AltexMaterial {
        name_id,
        shader_id: 0,
        albedo: material.color,
        metallic: material.metallic,
        roughness: material.roughness,
        ao: 1.0,
        emissive: material.emissive,
        albedo_map: 0xFFFF_FFFF,
        normal_map: 0xFFFF_FFFF,
        metallic_map: 0xFFFF_FFFF,
        roughness_map: 0xFFFF_FFFF,
        ao_map: 0xFFFF_FFFF,
        emissive_map: 0xFFFF_FFFF,
    });
    if let Some(m) = altex.meshes.get_mut(mesh_id as usize) {
        m.material_id = mat_id;
    }

    let transform = AltexTransform {
        position: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };
    altex.add_object(mesh_id, transform, name);

    altex
}

/// Сохраняет меш+материал эдитора как настоящий `.altex` на диске.
pub fn export_mesh_to_altex(
    mesh: &EditorMesh,
    material: &EditorMaterial,
    name: &str,
    path: &str,
) -> Result<()> {
    let altex = build_altex(mesh, material, name);
    altex
        .save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .altex '{}': {}", path, e))
}

/// Читает `.altex` с диска и возвращает (имя, меш, материал) для КАЖДОГО
/// меша файла — один .altex может нести несколько мешей (см.
/// `AltexFile::meshes`), хотя `build_altex` выше всегда пишет ровно один.
/// Материал берётся из `Mesh::material_id`, если он задан (не `0xFFFFFFFF`)
/// и существует в `altex.materials`, иначе — материал эдитора по умолчанию.
pub fn import_altex(path: &str) -> Result<Vec<(String, EditorMesh, EditorMaterial)>> {
    let altex =
        AltexFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .altex '{}': {}", path, e))?;

    let get_string = |id: u32| -> String {
        altex
            .strings
            .get(id as usize)
            .cloned()
            .unwrap_or_default()
    };

    let mut out = Vec::with_capacity(altex.meshes.len());
    for m in &altex.meshes {
        let vstart = m.vertex_offset as usize;
        let vcount = m.vertex_count as usize;
        let istart = m.index_offset as usize;
        let icount = m.index_count as usize;

        let verts = altex
            .vertices
            .get(vstart..vstart + vcount)
            .ok_or_else(|| anyhow!(".altex '{}': меш '{}' ссылается на вершины за пределами файла", path, get_string(m.name_id)))?;
        let idx = altex
            .indices
            .get(istart..istart + icount)
            .ok_or_else(|| anyhow!(".altex '{}': меш '{}' ссылается на индексы за пределами файла", path, get_string(m.name_id)))?;

        let positions: Vec<Vec3> = verts
            .iter()
            .map(|v| Vec3::new(v.position[0], v.position[1], v.position[2]))
            .collect();
        // Индексы в .altex абсолютные (считают от начала общего пула
        // вершин файла, см. AltexFile::add_mesh) — перебазируем в
        // локальные для editor::Mesh, у которого свой собственный vertices[].
        let rebased_indices: Vec<u32> = idx.iter().map(|&i| i - m.vertex_offset).collect();

        let mut editor_mesh = EditorMesh::new(positions, rebased_indices);
        // EditorMesh::new() пересчитывает сглаженные нормали сама — если в
        // .altex уже были осмысленные нормали (не с фолбэком Vec3::UP,
        // выставленным при экспорте только для отсутствующих), они точнее
        // пересчитанных (могут быть жёсткими/по граням), поэтому берём их.
        editor_mesh.normals = verts
            .iter()
            .map(|v| Vec3::new(v.normal[0], v.normal[1], v.normal[2]))
            .collect();

        let name = get_string(m.name_id);
        let material_id = m.material_id;
        let material = if material_id != 0xFFFF_FFFF {
            altex
                .materials
                .get(material_id as usize)
                .map(|mat| EditorMaterial {
                    name: get_string(mat.name_id),
                    color: mat.albedo,
                    metallic: mat.metallic,
                    roughness: mat.roughness,
                    emissive: mat.emissive,
                })
                .unwrap_or_default()
        } else {
            EditorMaterial::default()
        };

        out.push((name, editor_mesh, material));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_geometry_and_material() {
        let mesh = EditorMesh::create_cube();
        let material = EditorMaterial {
            name: "TestMat".to_string(),
            color: [0.1, 0.2, 0.3, 1.0],
            metallic: 0.4,
            roughness: 0.6,
            emissive: [0.0, 0.0, 0.0],
        };

        let path = std::env::temp_dir().join("alkash3d_editor_altex_roundtrip_test.altex");
        let path_str = path.to_string_lossy().to_string();

        export_mesh_to_altex(&mesh, &material, "TestCube", &path_str).expect("export");
        let imported = import_altex(&path_str).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.len(), 1);
        let (name, imported_mesh, imported_material) = &imported[0];
        assert_eq!(name, "TestCube");
        assert_eq!(imported_mesh.vertices.len(), mesh.vertices.len());
        assert_eq!(imported_mesh.indices.len(), mesh.indices.len());
        assert_eq!(imported_mesh.indices, mesh.indices);
        for (a, b) in imported_mesh.vertices.iter().zip(mesh.vertices.iter()) {
            assert!((a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5);
        }
        assert!((imported_material.color[0] - 0.1).abs() < 1e-5);
        assert!((imported_material.metallic - 0.4).abs() < 1e-5);
        assert!((imported_material.roughness - 0.6).abs() < 1e-5);
    }
}
