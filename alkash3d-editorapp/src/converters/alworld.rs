// src/converters/alworld.rs
//
// Экспорт/импорт сцены эдитора как настоящего открытого мира движка
// (`.alworld` + `chunks/*.alwchunk` + `objects/*.altex`) — использует
// реальные alkash3d_rs::AlworldFile/ChunkContent/ChunkDescriptor, так что
// результат грузится `AlkashEngine::load_world` без каких-либо
// дополнительных преобразований.
//
// Соглашение об именовании чанков (`chunk_{x}_{y}_{z}.alwchunk`) и месте
// поиска подпапки `chunks/` (рядом с .alworld, если явно не указано иное)
// — см. `engine/world_streaming.rs::chunk_file_path`/`load_world` в
// alkash3d-rust; здесь мы намеренно повторяем то же соглашение.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use uuid::Uuid;

use crate::math::Vec3;
use crate::scene::{GameObject, MeshComponent, ObjectType, Scene};

use alkash3d_rs::{AlworldFile, ChunkContent, ChunkDescriptor, GlobalObject};

use super::altex::build_altex;

/// ДОБАВЛЕНО (точка спавна игрока): движок сейчас `AlworldFile::
/// global_objects` не читает вообще (см. поиск по engine/*.rs в сессии) —
/// это единственное на сегодня полностью свободное, но уже полноценно
/// сериализуемое (`save`/`load` уже гоняют его туда-обратно) поле формата,
/// так что помечаем один `GlobalObject` этим битом вместо того, чтобы
/// менять сам бинарный layout `.alworld` (что потребовало бы версионирования
/// заголовка — см. как это решалось для `.altex` через `materials_offset`
/// и `version>=2`, гораздо более инвазивно, чем нужно здесь). Старший бит
/// `flags` выбран, чтобы не пересекаться с возможными будущими
/// битами-флагами обычных global-объектов, которые естественно начинали бы
/// нумероваться с 0.
const GLOBAL_OBJECT_FLAG_SPAWN_POINT: u32 = 0x8000_0000;

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "object".to_string()
    } else {
        cleaned
    }
}

/// `to_matrix()` эдитора (см. math/transform.rs) уже возвращает
/// column-major-для-WGSL массив `[[f32;4];4]`, где `array[3] =
/// [pos.x, pos.y, pos.z, 1.0]`. Простая построчная конкатенация даёт
/// плоский `[f32;16]` с переносом ИМЕННО в индексах 12..15 — ровно та
/// раскладка, которую `engine/world_streaming.rs` читает при спавне объекта
/// чанка (`let position = [m[12], m[13], m[14]];`), так что дополнительная
/// транспонизация здесь не нужна.
fn flatten_transform(m: &[[f32; 4]; 4]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for (i, row) in m.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(row);
    }
    out
}

/// Экспортирует всю сцену как открытый мир движка в папку `dir`:
/// `dir/world.alworld`, `dir/chunks/chunk_{x}_0_{z}.alwchunk`,
/// `dir/objects/<name>_<id8>.altex` (по одному .altex на меш-объект сцены).
///
/// Меш-объекты группируются по чанку (`chunk_size` из `AlworldFile`, по
/// умолчанию 64м) на основе их мировой позиции — объект без чанка не
/// попадёт в мир вообще, каждый чанк содержит объекты только своей ячейки.
///
/// ВАЖНО: пути к `.altex` в `ChunkContent` записываются АБСОЛЮТНЫМИ. Это
/// сознательный выбор — `AltexFile::load()` в движке открывает путь как
/// есть (`std::fs::File::open`), БЕЗ разрешения относительно .alworld/
/// .alwchunk (в отличие от самой папки `chunks/`, которую `load_world`
/// действительно резолвит относительно .alworld) — относительный путь типа
/// "objects/foo.altex" загрузился бы только если процесс движка запущен
/// именно из `dir`, что хрупко и неочевидно. Абсолютный путь всегда
/// корректен ценой непереносимости на другую машину/папку — если это когда-
/// нибудь станет проблемой, чинить нужно в первую очередь резолвинг путей в
/// `engine/world_streaming.rs`, а не подстраиваться под его текущее
/// ограничение здесь.
pub fn export_scene_to_alworld(scene: &Scene, dir: &str) -> Result<String> {
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir)
        .map_err(|e| anyhow!("Не удалось создать папку '{}': {}", dir.display(), e))?;
    let objects_dir = dir.join("objects");
    std::fs::create_dir_all(&objects_dir)?;
    let chunks_dir = dir.join("chunks");
    std::fs::create_dir_all(&chunks_dir)?;

    let mut world = AlworldFile::new(1.0);
    world.chunks.clear();
    let chunk_size = world.header.chunk_size;

    // Группируем меш-объекты по (grid_x, grid_z); grid_y всегда 0 — тот же
    // плоский мир, что у демо-миров движка (AlworldFile::create_open_world_demo).
    let mut per_chunk: BTreeMap<(i32, i32), Vec<Uuid>> = BTreeMap::new();
    for (&id, obj) in &scene.objects {
        if let ObjectType::Mesh(_) = obj.object_type {
            let p = scene.get_world_transform(id).position;
            let gx = (p.x / chunk_size).floor() as i32;
            let gz = (p.z / chunk_size).floor() as i32;
            per_chunk.entry((gx, gz)).or_default().push(id);
        }
    }

    if per_chunk.is_empty() {
        return Err(anyhow!("В сцене нет ни одного видимого меш-объекта — экспортировать в .alworld нечего"));
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for ((gx, gz), ids) in &per_chunk {
        let mut content = ChunkContent::new();

        for &id in ids {
            let obj = &scene.objects[&id];
            let (mesh, material) = match &obj.object_type {
                ObjectType::Mesh(m) => (&m.mesh, &m.material),
                _ => unreachable!("per_chunk строится только из ObjectType::Mesh"),
            };

            let altex = build_altex(mesh, material, &obj.name);

            let short_id = id.simple().to_string();
            let mut file_stem = format!("{}_{}", sanitize_filename(&obj.name), &short_id[..8]);
            while !used_names.insert(file_stem.clone()) {
                file_stem.push('_');
            }
            let file_name = format!("{}.altex", file_stem);
            let altex_path = objects_dir.join(&file_name);
            altex
                .save(altex_path.to_string_lossy().as_ref())
                .map_err(|e| anyhow!("Не удалось сохранить {}: {}", file_name, e))?;

            let absolute_altex_path = std::fs::canonicalize(&altex_path)
                .unwrap_or(altex_path)
                .to_string_lossy()
                .into_owned();

            let world_transform = scene.get_world_transform(id);
            let flat = flatten_transform(&world_transform.to_matrix());
            content.add_object(&absolute_altex_path, flat);

            let p = world_transform.position;
            for (axis, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                if v < min[axis] { min[axis] = v; }
                if v > max[axis] { max[axis] = v; }
            }
        }

        let chunk_path = chunks_dir.join(format!("chunk_{}_0_{}.alwchunk", gx, gz));
        content
            .save_to_file(chunk_path.to_string_lossy().as_ref())
            .map_err(|e| anyhow!("Не удалось сохранить чанк {}: {}", chunk_path.display(), e))?;

        world.chunks.push(ChunkDescriptor {
            grid_x: *gx,
            grid_y: 0,
            grid_z: *gz,
            state: 0,
            priority: 0.0,
            data_offset: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            objects_count: ids.len() as u32,
            lights_count: 0,
            occlusion_mesh_offset: 0,
        });
    }

    world.header.total_chunks = world.chunks.len() as u32;
    world.header.world_bounds_min = min;
    world.header.world_bounds_max = max;

    // Точка спавна — см. комментарий у GLOBAL_OBJECT_FLAG_SPAWN_POINT.
    // Детерминированный выбор "первой" при нескольких (сортировка по имени
    // затем id) — HashMap-порядок scene.objects иначе менялся бы между
    // запусками, а экспорт должен быть воспроизводим.
    let mut spawn_ids: Vec<Uuid> = scene
        .objects
        .iter()
        .filter(|(_, o)| matches!(o.object_type, ObjectType::SpawnPoint))
        .map(|(&id, _)| id)
        .collect();
    spawn_ids.sort_by(|a, b| {
        let na = &scene.objects[a].name;
        let nb = &scene.objects[b].name;
        na.cmp(nb).then(a.cmp(b))
    });
    if let Some(&spawn_id) = spawn_ids.first() {
        let obj = &scene.objects[&spawn_id];
        let world_transform = scene.get_world_transform(spawn_id);
        let name_id = world.add_string(&obj.name);
        let yaw = world_transform.rotation.to_euler().y;
        world.global_objects.push(GlobalObject {
            name_id,
            altex_file_id: 0xFFFF_FFFF,
            transform: flatten_transform(&world_transform.to_matrix()),
            // lod_distances переиспользован под yaw (см. комментарий у
            // GLOBAL_OBJECT_FLAG_SPAWN_POINT — это не LOD-объект вообще,
            // полю всё равно иначе нечем было бы быть занятым).
            lod_distances: [yaw, 0.0, 0.0, 0.0],
            flags: GLOBAL_OBJECT_FLAG_SPAWN_POINT,
        });
    }

    let world_path = dir.join("world.alworld");
    world
        .save(world_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow!("Не удалось сохранить world.alworld: {}", e))?;

    Ok(world_path.to_string_lossy().into_owned())
}

/// Тот же плейсхолдер, что рантайм движка подставляет вместо
/// отсутствующей/нечитаемой геометрии — см. `engine/asset_loading.rs::
/// load_placeholder_mesh` (`add_cube(1.0)`, единичный куб). Раньше
/// импортёр молча ПРОПУСКАЛ такие объекты (см. историю правки) — из-за
/// этого `.alworld`, где ВСЕ объекты ссылаются на служебный путь
/// "placeholder" (ровно так устроен `demo_world/world.alworld`, см.
/// `AlworldFile::create_and_save_demo_world`), импортировался как
/// полностью пустая сцена, хотя сам движок в рантайме рисует на этом месте
/// кубы. Теперь эдитор показывает то же самое, что покажет движок.
fn placeholder_mesh_object(name: &str, position: Vec3) -> GameObject {
    let mut obj = GameObject::new(
        name,
        ObjectType::Mesh(MeshComponent {
            mesh: crate::mesh::Mesh::create_cube(),
            material: crate::material::Material::default(),
            visible: true,
            wireframe: false,
            solid: true,
            double_sided: false,
        }),
    );
    obj.transform.position = position;
    obj
}

/// Импортирует `.alworld` (и его `chunks/*.alwchunk` + все ссылки на
/// `.altex`) обратно в сцену эдитора — по одному `GameObject::Mesh` на
/// каждый объект каждого загруженного чанка. Объекты, ссылающиеся на
/// служебный путь "placeholder" или на `.altex`, который не удалось
/// прочитать, получают куб-плейсхолдер (см. `placeholder_mesh_object`) —
/// ровно то же поведение, что у самого движка, а не пропуск объекта.
pub fn import_alworld_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let world = AlworldFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alworld '{}': {}", path, e))?;

    let mut chunks_dir = std::path::PathBuf::from(path);
    chunks_dir.pop();
    chunks_dir.push("chunks");

    let name = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "World".to_string());
    let mut scene = Scene::new(&name);

    let mut objects_loaded = 0usize;
    for chunk in &world.chunks {
        let chunk_path = chunks_dir.join(format!("chunk_{}_{}_{}.alwchunk", chunk.grid_x, chunk.grid_y, chunk.grid_z));
        let content = match ChunkContent::load_from_file(chunk_path.to_string_lossy().as_ref()) {
            Ok(c) => c,
            Err(e) => {
                log(format!("⚠️ Пропущен чанк ({}, {}, {}): {} ({})", chunk.grid_x, chunk.grid_y, chunk.grid_z, e, chunk_path.display()));
                continue;
            }
        };

        for (obj_idx, obj) in content.objects.iter().enumerate() {
            let altex_path = content.get_string(obj.altex_path_string_id);
            let m = &obj.transform;
            let position = Vec3::new(m[12], m[13], m[14]);

            if altex_path.is_empty() || altex_path == "placeholder" {
                let name = format!("Placeholder_{}_{}_{}_{}", chunk.grid_x, chunk.grid_y, chunk.grid_z, obj_idx);
                scene.add_object(placeholder_mesh_object(&name, position));
                objects_loaded += 1;
                continue;
            }

            let meshes = match super::altex::import_altex(altex_path) {
                Ok(m) => m,
                Err(e) => {
                    log(format!("⚠️ '{}' недоступен, подставлен куб-плейсхолдер (как в движке): {}", altex_path, e));
                    let name = format!("Placeholder_{}_{}_{}_{}", chunk.grid_x, chunk.grid_y, chunk.grid_z, obj_idx);
                    scene.add_object(placeholder_mesh_object(&name, position));
                    objects_loaded += 1;
                    continue;
                }
            };

            for (mesh_name, mesh, material) in meshes {
                let mut game_obj = GameObject::new(
                    &mesh_name,
                    ObjectType::Mesh(MeshComponent {
                        mesh,
                        material,
                        visible: true,
                        wireframe: false,
                        solid: true,
                        double_sided: false,
                    }),
                );
                game_obj.transform.position = position;
                scene.add_object(game_obj);
                objects_loaded += 1;
            }
        }
    }

    // Точка спавна — см. комментарий у GLOBAL_OBJECT_FLAG_SPAWN_POINT и у
    // экспорта выше. Позиция — из transform[12..15] (та же раскладка, что
    // у объектов чанков), направление взгляда — только по горизонтали
    // (yaw), взятое из lod_distances[0] (см. export для объяснения, почему
    // именно это поле).
    for global_obj in &world.global_objects {
        if global_obj.flags & GLOBAL_OBJECT_FLAG_SPAWN_POINT == 0 {
            continue;
        }
        let m = &global_obj.transform;
        let position = Vec3::new(m[12], m[13], m[14]);
        let yaw = global_obj.lod_distances[0];
        let name = world.strings.get(global_obj.name_id as usize).cloned().unwrap_or_else(|| "Spawn Point".to_string());

        let mut spawn_obj = GameObject::new(&name, ObjectType::SpawnPoint);
        spawn_obj.transform.position = position;
        spawn_obj.transform.rotation = crate::math::Quat::from_euler(0.0, yaw, 0.0);
        scene.add_object(spawn_obj);
    }

    if objects_loaded == 0 {
        log("⚠️ .alworld загружен, но ни один объект не был импортирован (пустой мир или все ссылки на .altex недоступны)".to_string());
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
    fn round_trip_preserves_object_count_and_position() {
        let mut scene = Scene::new("TestWorld");
        let mut obj = GameObject::new(
            "Cube1",
            ObjectType::Mesh(MeshComponent {
                mesh: EditorMesh::create_cube(),
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        );
        obj.transform.position = Vec3::new(100.0, 0.0, -40.0);
        scene.add_object(obj);

        let dir = std::env::temp_dir().join("alkash3d_editor_alworld_roundtrip_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_string_lossy().to_string();

        let world_path = export_scene_to_alworld(&scene, &dir_str).expect("export");
        assert!(Path::new(&world_path).exists());

        let mut logs = Vec::new();
        let imported = import_alworld_to_scene(&world_path, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(imported.objects.len(), 1, "logs: {:?}", logs);
        let imported_obj = imported.objects.values().next().unwrap();
        assert!((imported_obj.transform.position.x - 100.0).abs() < 1e-3);
        assert!((imported_obj.transform.position.z - (-40.0)).abs() < 1e-3);
    }

    #[test]
    fn round_trip_preserves_spawn_point() {
        let mut scene = Scene::new("TestWorld");
        scene.add_object(GameObject::new(
            "Cube1",
            ObjectType::Mesh(MeshComponent {
                mesh: EditorMesh::create_cube(),
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        ));

        let mut spawn = GameObject::new("PlayerStart", ObjectType::SpawnPoint);
        spawn.transform.position = Vec3::new(5.0, 1.0, -3.0);
        spawn.transform.rotation = crate::math::Quat::from_euler(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        scene.add_object(spawn);

        let dir = std::env::temp_dir().join("alkash3d_editor_alworld_spawn_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_string_lossy().to_string();

        let world_path = export_scene_to_alworld(&scene, &dir_str).expect("export");
        let mut logs = Vec::new();
        let imported = import_alworld_to_scene(&world_path, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_dir_all(&dir);

        let spawn_objects: Vec<_> = imported
            .objects
            .values()
            .filter(|o| matches!(o.object_type, ObjectType::SpawnPoint))
            .collect();
        assert_eq!(spawn_objects.len(), 1, "logs: {:?}", logs);
        let s = spawn_objects[0];
        assert!((s.transform.position.x - 5.0).abs() < 1e-3);
        assert!((s.transform.position.y - 1.0).abs() < 1e-3);
        assert!((s.transform.position.z - (-3.0)).abs() < 1e-3);
        let facing = s.transform.rotation.forward();
        // Поворот на 90° по Y от "смотрю вдоль +Z" даёт направление вдоль +X.
        assert!(facing.x > 0.9, "facing={:?}", facing);
    }

    /// ИСПРАВЛЕНО (баг: "импортировал alworld который сейчас есть, но там
    /// пусто"): `demo_world/world.alworld`, который реально лежит в
    /// репозитории (`AlworldFile::create_and_save_demo_world`), — ВСЕ его
    /// объекты ссылаются на служебный путь "placeholder". Раньше импортёр
    /// такие объекты пропускал целиком — сцена оставалась пустой. Тест
    /// гоняет ИМЕННО этот файл (не синтетический), чтобы поймать регрессию
    /// на будущее.
    #[test]
    fn demo_world_placeholder_objects_import_as_cubes() {
        let demo_world_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("alkash3d-rust")
            .join("demo_world")
            .join("world.alworld");
        if !demo_world_path.exists() {
            // demo_world не сгенерирован в этом окружении — не проваливаем сборку.
            return;
        }

        let mut logs = Vec::new();
        let scene = import_alworld_to_scene(&demo_world_path.to_string_lossy(), &mut |m| logs.push(m))
            .expect("import demo_world/world.alworld");

        assert!(!scene.objects.is_empty(), "demo_world импортировался пустым; logs: {:?}", logs);
        // ИЗМЕНЕНО: раньше этот файл был чисто автогенерируемым демо-миром
        // движка (только Mesh-плейсхолдеры) — тест ассертил, что КАЖДЫЙ
        // объект Mesh. С тех пор пользователь реально пользовался эдитором
        // и перезаписал именно этот файл своим экспортом сцены с точкой
        // спавна (см. git-историю demo_world/world.alworld — строка "Spawn
        // Point" внутри) — живое доказательство, что реальный round-trip
        // "эдитор -> .alworld -> точка спавна" действительно работает не
        // только в синтетическом тесте выше. Смысл ЭТОГО теста
        // (плейсхолдер вместо отсутствующей геометрии не пропускается импортом)
        // по-прежнему проверяем — просто больше не требуем, чтобы В ФАЙЛЕ
        // не было ничего, кроме Mesh (валидный .alworld теперь может нести
        // и SpawnPoint).
        for obj in scene.objects.values() {
            assert!(
                matches!(obj.object_type, ObjectType::Mesh(_) | ObjectType::SpawnPoint),
                "неожиданный тип объекта в demo_world: {:?}", obj.object_type
            );
        }
    }
}
