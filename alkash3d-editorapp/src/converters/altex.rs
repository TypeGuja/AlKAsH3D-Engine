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

use crate::material::{Material as EditorMaterial, TextureAsset};
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

/// Значение `Texture::format` у встроенной в `.altex` текстуры — движок
/// (`asset_loading.rs::register_material_texture`) это поле СЕЙЧАС вообще
/// не читает (всегда создаёт `DXGI_FORMAT_R8G8B8A8_UNORM` независимо от
/// него), так что для рантайма оно не имеет значения, но пишем настоящий
/// DXGI-код формата (28 = `DXGI_FORMAT_R8G8B8A8_UNORM`) для честности файла
/// на диске — `TextureAsset::pixels` действительно всегда RGBA8 (см.
/// `TextureAsset::load_from_file` — `image::DynamicImage::to_rgba8()`).
const DXGI_FORMAT_R8G8B8A8_UNORM: u32 = 28;

/// Строит `AltexFile` из одного меша + материала эдитора — единственный
/// объект в сцене файла, с единичной (identity) трансформацией: .altex сам
/// по себе — формат геометрии ОДНОГО объекта/типового меша (см. комментарий
/// у `alworld_format.rs::ChunkObjectHeader` — размещение в мире хранится
/// отдельно, в .alwchunk, объект .altex ссылается только по пути).
///
/// ОБНОВЛЕНО (текстуры материалов — по прямому запросу пользователя):
/// раньше UV писались нейтральной заглушкой `(0,0)` на каждую вершину
/// (`crate::mesh::Mesh` тогда вообще не хранил UV) — теперь берутся из
/// `mesh.uv` (см. `mesh/uv.rs`), реально посчитанного для каждого меша.
/// Тангенты по-прежнему заглушка (оси X/Y) — полноценный расчёт тангентов
/// по UV, как уже делает НЕиспользуемый `converters/obj.rs`, отдельная
/// доработка, не относящаяся напрямую к "назначить текстуру объекту".
pub fn build_altex(mesh: &EditorMesh, material: &EditorMaterial, name: &str) -> AltexFile {
    let mut altex = AltexFile::new();

    let vertices: Vec<AltexVertex> = (0..mesh.vertices.len())
        .map(|i| {
            let p = mesh.vertices[i];
            let n = mesh.normals.get(i).copied().unwrap_or(Vec3::UP);
            let uv = mesh.uv.get(i).copied().unwrap_or([0.0, 0.0]);
            AltexVertex {
                position: [p.x, p.y, p.z],
                normal: [n.x, n.y, n.z],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
                uv,
                uv2: [0.0, 0.0],
                color: [1.0, 1.0, 1.0, 1.0],
            }
        })
        .collect();

    let mesh_id = altex.add_mesh(vertices, mesh.indices.clone(), name);

    // ДОБАВЛЕНО (текстуры материалов): если материалу назначена картинка
    // (см. `Material::albedo_texture`), встраиваем её сырые RGBA8-пиксели
    // прямо в файл через `AltexFile::add_texture` — .altex самодостаточен
    // (геометрия+материал+текстуры одним файлом, см. комментарий у
    // `almat_format::MaterialDefinition` про разницу с .almat), поэтому
    // текстура ВСТРАИВАЕТСЯ, а не сохраняется по ссылке на путь.
    let albedo_map = match &material.albedo_texture {
        Some(tex) => altex.add_texture(
            tex.width,
            tex.height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            &tex.pixels,
            &format!("{}_albedo", material.name),
        ),
        None => 0xFFFF_FFFF,
    };

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
        albedo_map,
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

/// ДОБАВЛЕНО (текстуры материалов): распаковывает albedo-текстуру
/// материала (если она есть, `albedo_map != 0xFFFFFFFF`) из общего пула
/// `AltexFile::texture_data` в `TextureAsset`. Границы `data_offset`/
/// `data_size` проверяются явно (не просто индексируются с паникой при
/// повреждённом файле) — та же осторожность, что уже применена к
/// движковой стороне в `asset_loading.rs::load_altex_map_srv` для точно
/// такой же ситуации. Ошибка здесь НЕ обрывает импорт всего файла — только
/// пропускает текстуру (тот же принцип отказоустойчивости, что и у
/// движка): испорченная текстура не должна ронять геометрию/материал.
fn load_embedded_albedo(altex: &AltexFile, albedo_map: u32, path: &str) -> Option<TextureAsset> {
    if albedo_map == 0xFFFF_FFFF {
        return None;
    }
    let texture = altex.textures.get(albedo_map as usize)?;
    let start = texture.data_offset as usize;
    let end = start + texture.data_size as usize;
    let Some(pixels) = altex.texture_data.get(start..end) else {
        eprintln!(
            "[ALTEX] WARNING: '{}' содержит albedo-текстуру с некорректными data_offset/data_size — пропущена",
            path
        );
        return None;
    };
    if pixels.len() != texture.width as usize * texture.height as usize * 4 {
        eprintln!(
            "[ALTEX] WARNING: '{}' albedo-текстура {}x{} имеет {} байт вместо ожидаемых {} — пропущена",
            path, texture.width, texture.height, pixels.len(), texture.width as usize * texture.height as usize * 4
        );
        return None;
    }
    Some(TextureAsset::from_rgba(texture.width, texture.height, pixels.to_vec(), None))
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
        // ДОБАВЛЕНО (текстуры материалов): та же логика, что и у normals
        // выше — реальные UV из файла (если это не заглушка (0,0), с
        // которой раньше писали ВСЕ .altex до этой правки) точнее
        // автоматической планарной проекции `EditorMesh::new()`.
        editor_mesh.set_uv(verts.iter().map(|v| v.uv).collect());

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
                    // ДОБАВЛЕНО (текстуры материалов): если материал
                    // ссылается на встроенную albedo-текстуру, распаковываем
                    // её пиксели из общего пула `altex.texture_data` в
                    // `TextureAsset` — `source_path: None`, т.к. у этой
                    // текстуры нет файла на диске эдитора, она целиком
                    // пришла из байт `.altex` (см. комментарий у поля
                    // `TextureAsset::source_path`).
                    albedo_texture: load_embedded_albedo(&altex, mat.albedo_map, path),
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
            albedo_texture: None,
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
        assert!(imported_material.albedo_texture.is_none());
        // Куб больше не пишет UV-заглушку (0,0) на каждую вершину — у
        // каждой вершины должна быть своя, посчитанная `recalculate_uv()`
        // (планарная проекция по доминирующей оси нормали, см. mesh/uv.rs).
        assert_eq!(imported_mesh.uv.len(), mesh.uv.len());
        for (a, b) in imported_mesh.uv.iter().zip(mesh.uv.iter()) {
            assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn round_trip_preserves_embedded_albedo_texture() {
        let mesh = EditorMesh::create_cube();
        // 2x2 RGBA8 — маленькая, но не однопиксельная текстура: ловит и
        // размер, и порядок байт, а не только "хоть что-то не пусто".
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255, 0, 255, 0, 255,
            0, 0, 255, 255, 255, 255, 0, 255,
        ];
        let material = EditorMaterial {
            name: "Textured".to_string(),
            color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.8,
            emissive: [0.0, 0.0, 0.0],
            albedo_texture: Some(TextureAsset::from_rgba(2, 2, pixels.clone(), Some("C:/fake/brick.png".to_string()))),
        };

        let path = std::env::temp_dir().join("alkash3d_editor_altex_texture_roundtrip_test.altex");
        let path_str = path.to_string_lossy().to_string();

        export_mesh_to_altex(&mesh, &material, "TexturedCube", &path_str).expect("export");
        let imported = import_altex(&path_str).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.len(), 1);
        let (_, _, imported_material) = &imported[0];
        let tex = imported_material.albedo_texture.as_ref().expect("albedo texture must round-trip");
        assert_eq!(tex.width, 2);
        assert_eq!(tex.height, 2);
        assert_eq!(*tex.pixels, pixels);
        // Встроенная текстура пришла из байт файла, а не с диска эдитора —
        // путь заведомо не переносится (см. `TextureAsset::source_path`).
        assert!(tex.source_path.is_none());
    }
}
