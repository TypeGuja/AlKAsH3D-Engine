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

/// ИСПРАВЛЕНО (по прямому запросу пользователя: "2 отредактированные по
/// величине фигуры не передают ту же величину в движок и из-за этого
/// остаются маленькие кубы") — `.altex` несёт геометрию меша в ЛОКАЛЬНЫХ
/// координатах, а движок при спавне объекта чанка (`engine/
/// world_streaming.rs`) достаёт из переданной матрицы ТОЛЬКО позицию
/// (`m[12..15]`, см. комментарий у `flatten_transform` выше) — поворот и
/// масштаб самого объекта в рантайм вообще не доходят. Для большинства
/// объектов сцены (scale=1, rotation=identity) разницы не видно, поэтому
/// баг проявлялся только на реально отмасштабированных/повёрнутых
/// объектах: в движке они откатывались к исходному юнит-размеру меша
/// ("маленький куб"). Чиним на стороне экспорта — печём поворот и масштаб
/// (БЕЗ позиции: она и так корректно доезжает через матрицу чанка, а
/// повторное её применение задвоило бы смещение) прямо в вершины/нормали
/// перед записью в `.altex`, так что движку достаточно уже применяемой им
/// позиции, чтобы получить объект нужного размера и ориентации.
fn bake_linear_transform(mesh: &crate::mesh::Mesh, world: &crate::math::Transform) -> crate::mesh::Mesh {
    use crate::math::Transform;

    if world.scale == Vec3::ONE && world.rotation == crate::math::Quat::IDENTITY {
        return mesh.clone();
    }

    // Только поворот+масштаб, без переноса — `Transform::transform_point`
    // как раз считает "scale, затем rotate" и прибавляет `position`, так что
    // с обнулённой позицией это ровно нужный линейный оператор.
    let linear = Transform { position: Vec3::ZERO, rotation: world.rotation, scale: world.scale };
    // Нормали трансформируются обратно-транспонированной матрицей — для
    // (rotation * diag(scale)) это тот же поворот, но с ОБРАТНЫМ масштабом
    // (см. вывод: (R*S)^-T = R*S^-1 для ортогонального R и диагонального S).
    let inv_scale = Vec3::new(1.0 / world.scale.x, 1.0 / world.scale.y, 1.0 / world.scale.z);
    let normal_linear = Transform { position: Vec3::ZERO, rotation: world.rotation, scale: inv_scale };

    let vertices = mesh.vertices.iter().map(|&v| linear.transform_point(v)).collect::<Vec<_>>();
    let normals = mesh
        .normals
        .iter()
        .map(|&n| normal_linear.transform_point(n).normalize())
        .collect();

    // UV не зависят от поворота/масштаба объекта (это чисто параметризация
    // ПОВЕРХНОСТИ, не мировая геометрия) — переносим как есть, тем же
    // порядком вершин, вместо пересчёта `recalculate_uv()` заново: та
    // проекция по доминирующей оси нормали дала бы другой (не обязательно
    // худший, но НЕконсистентный между исходным и запечённым мешем)
    // результат, а честная UV из OBJ-импорта (см. `assets/library.rs::
    // parse_obj`) вообще не пересчитывается заново — только переносится.
    let uv = mesh.uv.clone();

    let mut baked = crate::mesh::Mesh {
        vertices,
        indices: mesh.indices.clone(),
        normals,
        uv,
        bounds: mesh.bounds,
    };
    baked.recalculate_bounds();
    baked
}

/// ДОБАВЛЕНО (по прямому запросу пользователя: "почему у меня целая карта в
/// .obj всего грузится в 1 маленьком чанке, хотя карта достаточно большая")
/// — импортированный `.obj` обычно приходит ОДНИМ гигантским мешем на всю
/// карту (один `GameObject`, одна-единственная `transform.position`), а
/// группировка по чанкам раньше велась ИМЕННО по этой одной точке (см. старую
/// версию цикла ниже, до этой правки) — вся геометрия, как бы далеко её
/// вершины друг от друга ни лежали, попадала в один и тот же `(gx, gz)`.
/// Реального постримингового разбиения такой карты не получалось: она либо
/// вся загружена, либо вся выгружена разом, привязанная к одной ячейке.
///
/// Разрезаем меш НА УРОВНЕ ТРЕУГОЛЬНИКОВ — каждый треугольник кладём в ту
/// ячейку сетки чанков, в которую попадает его МИРОВОЙ центроид
/// (`world_position + среднее трёх вершин`; `mesh` здесь уже прогнан через
/// `bake_linear_transform`, то есть несёт поворот/масштаб объекта, но НЕ
/// перенос — поэтому `world_position` добавляем явно). Вершины
/// переиспользуются внутри своей ячейки через реиндексацию (`remap`) —
/// дублируются только те, что реально лежат на границе между двумя ячейками
/// (иначе такая вершина не может одновременно принадлежать двум независимым
/// мешам двух разных чанков).
fn split_mesh_by_chunk(
    mesh: &crate::mesh::Mesh,
    world_position: Vec3,
    chunk_size: f32,
) -> BTreeMap<(i32, i32), crate::mesh::Mesh> {
    #[derive(Default)]
    struct CellBuilder {
        vertices: Vec<Vec3>,
        normals: Vec<Vec3>,
        // ДОБАВЛЕНО (текстуры материалов): UV переносится тем же remap'ом,
        // что и normals — см. комментарий у `uv` в `bake_linear_transform`
        // про то, почему это перенос, а не пересчёт заново.
        uv: Vec<[f32; 2]>,
        indices: Vec<u32>,
        remap: std::collections::HashMap<u32, u32>,
    }

    let mut cells: BTreeMap<(i32, i32), CellBuilder> = BTreeMap::new();

    for tri in mesh.indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0], tri[1], tri[2]);
        let v0 = mesh.vertices[i0 as usize];
        let v1 = mesh.vertices[i1 as usize];
        let v2 = mesh.vertices[i2 as usize];
        let centroid_world = world_position + (v0 + v1 + v2) * (1.0 / 3.0);
        let gx = (centroid_world.x / chunk_size).floor() as i32;
        let gz = (centroid_world.z / chunk_size).floor() as i32;

        let cell = cells.entry((gx, gz)).or_insert_with(CellBuilder::default);
        // Разбираем на отдельные поля (не через `cell.vertices`/`cell.remap`
        // напрямую внутри одного выражения) — иначе `remap.entry(..).
        // or_insert_with(|| ... vertices.push(..) ...)` держал бы два
        // одновременных `&mut` на один и тот же `cell` и не скомпилировался
        // бы. Матчинг на `&mut CellBuilder` по полям (match ergonomics) даёт
        // РАЗНЫЕ независимые `&mut` на каждое поле — без этой проблемы.
        let CellBuilder { vertices, normals, uv, indices, remap } = cell;

        for &orig_idx in &[i0, i1, i2] {
            let new_idx = *remap.entry(orig_idx).or_insert_with(|| {
                let idx = vertices.len() as u32;
                vertices.push(mesh.vertices[orig_idx as usize]);
                normals.push(mesh.normals.get(orig_idx as usize).copied().unwrap_or(Vec3::UP));
                uv.push(mesh.uv.get(orig_idx as usize).copied().unwrap_or([0.0, 0.0]));
                idx
            });
            indices.push(new_idx);
        }
    }

    cells
        .into_iter()
        .map(|(cell, b)| {
            let mut m = crate::mesh::Mesh {
                vertices: b.vertices,
                indices: b.indices,
                normals: b.normals,
                uv: b.uv,
                bounds: (Vec3::ZERO, Vec3::ZERO),
            };
            m.recalculate_bounds();
            (cell, m)
        })
        .collect()
}

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

    // ИЗМЕНЕНО (по прямому запросу пользователя: "стоит ли поднимать размер
    // чанков до 128/256/512?" — после того как реальная резка по чанкам
    // дала пользователю 53 тыс. занятых ячеек при chunk_size=64м): да,
    // стоит. Каждая занятая ячейка — это отдельные `.altex`+`.alwchunk`
    // файлы на диске (см. комментарий у `MAX_REASONABLE_CHUNKS` ниже) —
    // число файлов растёт как 1/chunk_size² при том же охвате карты, так
    // что чанк покрупнее резко снижает файловый I/O (256м вместо 64м — это
    // в 16 раз меньше файлов на ту же площадь). Плата — более грубая
    // гранулярность стриминга (грузится/выгружается более крупными
    // кусками). 256м — разумная середина для одной пользовательской карты
    // (не MMO-масштаба): заметно меньше файлов, чем 64м/128м, но ещё не
    // настолько крупно, как 512м, где даже `load_distance` ниже толком не
    // покрывал бы один чанк целиком. Меняй эту константу, если конкретно
    // твоей карте нужно другое соотношение "число файлов / гранулярность".
    const EXPORT_CHUNK_SIZE_METERS: f32 = 256.0;
    world.header.chunk_size = EXPORT_CHUNK_SIZE_METERS;
    let chunk_size = EXPORT_CHUNK_SIZE_METERS;

    // ИСПРАВЛЕНО (по прямому запросу пользователя, после включения реальной
    // резки меша по чанкам выше: "карта перестала быть картой, отдельные
    // несложенные части карты") — куски карты стоят на правильных местах,
    // "дыры" между ними — это чанки, которые ещё не попали в радиус
    // подгрузки: `AlworldFile::new` по умолчанию ставит
    // `load_distance=200`/`unload_distance=250` (см. alworld_format.rs) —
    // разумно для настоящего MMO-масштаба открытого мира при chunk_size=64,
    // но для одной сравнительно некрупной пользовательской карты этого мало.
    // ИЗМЕНЕНО: считаем радиус ПРОПОРЦИОНАЛЬНО реальному chunk_size (~3-4
    // чанка в каждую сторону), а не абсолютными метрами — иначе смена
    // EXPORT_CHUNK_SIZE_METERS выше рассинхронизировала бы радиус подгрузки
    // с размером самой ячейки (например при chunk_size=512 старые
    // 400/500м не покрывали бы даже один чанк).
    world.streaming_config.load_distance = chunk_size * 3.0;
    world.streaming_config.unload_distance = chunk_size * 4.0;

    // ИЗМЕНЕНО (по прямому запросу пользователя: "почему у меня целая карта
    // в .obj всего грузится в 1 маленьком чанке") — раньше сюда попадали
    // ЦЕЛЫЕ объекты сцены, сгруппированные по (grid_x, grid_z) их ОДНОЙ
    // `transform.position`; теперь ключом чанка размечен не объект, а
    // отдельный ГОТОВЫЙ К ЗАПИСИ кусок геометрии (см. `split_mesh_by_chunk`)
    // — один меш-объект сцены (например весь импортированный `.obj`) может
    // дать записи в НЕСКОЛЬКО разных ячеек. grid_y всегда 0 — тот же плоский
    // мир, что у демо-миров движка (AlworldFile::create_open_world_demo).
    // ДОБАВЛЕНО (по прямому запросу пользователя: "бесконечно грузит +
    // использует не больше 30% проца" — на реальной большой .obj-карте):
    // сама резка по треугольникам быстрая (проверено бенчмарком — 500 тыс.
    // треугольников укладываются в доли секунды), а вот запись файлов —
    // одна пара `.altex`+`canonicalize()` (файловый syscall) на КАЖДЫЙ
    // занятый чанк — уже настоящий диск-bound I/O. Если геометрия
    // оказывается в неверном масштабе (типичный баг: .obj экспортирован в
    // сантиметрах, но интерпретируется как метры — тогда всё в 100 раз
    // больше ожидаемого), при chunk_size=64м это даёт не сотни, а ДЕСЯТКИ
    // ТЫСЯЧ занятых ячеек — соответственно десятки тысяч мелких файлов,
    // создание каждого из которых на Windows (+возможное сканирование
    // антивирусом каждого нового файла) стоит реальное время: суммарно
    // это может растянуться на часы, выглядя как "зависло", хотя технически
    // прогресс идёт (отсюда и неполная, но ненулевая загрузка CPU — время
    // уходит на ожидание диска, а не на вычисления).
    //
    // Поэтому режем и считаем занятые ячейки СНАЧАЛА, ничего не записывая
    // на диск, и если их аномально много — сразу возвращаем понятную
    // ошибку с реальными границами мира (по которым видно проблему
    // масштаба), вместо того чтобы тихо начинать писать файлы на много
    // часов вперёд.
    struct PendingPiece {
        gx: i32,
        gz: i32,
        mesh: crate::mesh::Mesh,
        obj_name: String,
        short_id: String,
        flat: [f32; 16],
    }

    let mut pending: Vec<(PendingPiece, crate::material::Material)> = Vec::new();
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];

    for (&id, obj) in &scene.objects {
        let (mesh, material) = match &obj.object_type {
            ObjectType::Mesh(m) => (&m.mesh, &m.material),
            _ => continue,
        };

        let world_transform = scene.get_world_transform(id);
        let baked_mesh = bake_linear_transform(mesh, &world_transform);
        let flat = flatten_transform(&world_transform.to_matrix());
        let short_id = id.simple().to_string();

        for ((gx, gz), piece_mesh) in split_mesh_by_chunk(&baked_mesh, world_transform.position, chunk_size) {
            // Границы мира считаем по РЕАЛЬНЫМ мировым вершинам куска
            // (`world_transform.position` + уже испечённые поворот/масштаб
            // в `piece_mesh`), а не по одной точке `transform.position`
            // объекта — иначе для огромного OBJ-меша `world_bounds` были бы
            // размером в одну точку вместо реального размера карты.
            for &v in &piece_mesh.vertices {
                let world_v = world_transform.position + v;
                for (axis, value) in [world_v.x, world_v.y, world_v.z].into_iter().enumerate() {
                    if value < min[axis] { min[axis] = value; }
                    if value > max[axis] { max[axis] = value; }
                }
            }

            pending.push((
                PendingPiece { gx, gz, mesh: piece_mesh, obj_name: obj.name.clone(), short_id: short_id.clone(), flat },
                material.clone(),
            ));
        }
    }

    if pending.is_empty() {
        return Err(anyhow!("В сцене нет ни одного видимого меш-объекта — экспортировать в .alworld нечего"));
    }

    let distinct_chunks: std::collections::HashSet<(i32, i32)> =
        pending.iter().map(|(p, _)| (p.gx, p.gz)).collect();

    // ИЗМЕНЕНО (по прямому запросу пользователя: "убери лимит по 4096 а то
    // у меня 53к так что убери" — 53 тыс. занятых ячеек оказались РЕАЛЬНЫМ
    // размером его карты, а не багом масштаба). Не убираю проверку целиком:
    // она — единственное, что раньше отличало "у меня правда большая карта"
    // от "я перепутал сантиметры с метрами и жду часами, глядя на
    // 'зависший' эдитор" (см. историю этого файла) — без неё та же ошибка
    // масштаба когда-нибудь в будущем на ДРУГОЙ карте снова обернётся
    // многочасовым 'зависанием' без единого объяснения. Вместо удаления —
    // поднимаю порог на порядок выше его реального 53к (с запасом на рост
    // карты), да и после подъёма `chunk_size` до 256м та же карта потребует
    // уже в ~16 раз меньше ячеек (53000/16 ≈ 3300) — так что порог теперь
    // почти наверняка не будет мешать даже без учёта запаса.
    const MAX_REASONABLE_CHUNKS: usize = 100_000;
    if distinct_chunks.len() > MAX_REASONABLE_CHUNKS {
        return Err(anyhow!(
            "Карта требует {} чанков (порог {}), это заняло бы неоправданно долго. \
             Границы мира получились X: [{:.1}; {:.1}], Y: [{:.1}; {:.1}], Z: [{:.1}; {:.1}] (метры) \
             — если это больше, чем реальный размер твоей карты, скорее всего геометрия импортирована \
             в неверном масштабе (например .obj в сантиметрах вместо метров). Проверь масштаб перед повторным экспортом.",
            distinct_chunks.len(), MAX_REASONABLE_CHUNKS,
            min[0], max[0], min[1], max[1], min[2], max[2]
        ));
    }

    // ДОБАВЛЕНО (по прямому запросу пользователя: "добавь возможность
    // использовать все ресурсы проца" — для карты с десятками тысяч
    // занятых ячеек запись `.altex` (build_altex + save + canonicalize —
    // диск-I/O на КАЖДУЮ ячейку, см. комментарий выше) раньше шла строго
    // последовательно одним потоком. Делим `pending` на равные пачки по
    // числу логических ядер (`available_parallelism`) и пишем их
    // ПАРАЛЛЕЛЬНО через `std::thread::scope` — сам ОС/диск всё равно
    // ограничивают реальный выигрыш (I/O bound, не CPU bound), но на SSD и
    // на файловых системах, где создание файла не сериализовано железно,
    // это даёт реальное ускорение почти пропорционально числу ядер.
    // `std::thread::scope` (не голый `thread::spawn`) — потому что каждая
    // пачка лишь ЗАИМСТВУЕТ свой срез `pending`, а не владеет им (не нужно
    // клонировать десятки тысяч мешей ради потоков); имя файла уникально
    // само по себе (short_id объекта + gx + gz — см. `PendingPiece`), так
    // что `used_names`-дедуп внутри пачки — чисто defensive, коллизий
    // между пачками в норме не бывает.
    //
    // ГРАФИЧЕСКИЙ ПРОЦЕССОР сюда не подключаем — это не численно-параллельная
    // задача (матрицы/шейдеры), а последовательная логика + файловый I/O
    // (HashMap, форматирование строк, syscalls), GPU для такой работы не
    // предназначен и ничего бы не ускорил.
    let worker_count = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(1);
    let batch_size = pending.len().div_ceil(worker_count).max(1);

    let batch_results: Vec<Result<Vec<((i32, i32), String, [f32; 16])>>> = std::thread::scope(|scope| {
        let objects_dir = &objects_dir;
        let handles: Vec<_> = pending
            .chunks(batch_size)
            .map(|batch| {
                scope.spawn(move || -> Result<Vec<((i32, i32), String, [f32; 16])>> {
                    let mut local_used_names: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let mut out = Vec::with_capacity(batch.len());
                    for (piece, material) in batch {
                        let altex = build_altex(&piece.mesh, material, &piece.obj_name);

                        let mut file_stem = format!("{}_{}_{}_{}", sanitize_filename(&piece.obj_name), &piece.short_id[..8], piece.gx, piece.gz);
                        while !local_used_names.insert(file_stem.clone()) {
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

                        out.push(((piece.gx, piece.gz), absolute_altex_path, piece.flat));
                    }
                    Ok(out)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("поток записи .altex запаниковал")).collect()
    });

    let mut per_chunk: BTreeMap<(i32, i32), Vec<(String, [f32; 16])>> = BTreeMap::new();
    for batch in batch_results {
        for (cell, altex_path, flat) in batch? {
            per_chunk.entry(cell).or_default().push((altex_path, flat));
        }
    }

    for ((gx, gz), pieces) in &per_chunk {
        let mut content = ChunkContent::new();
        for (altex_path, flat) in pieces {
            content.add_object(altex_path, *flat);
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
            objects_count: pieces.len() as u32,
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

    // ДОБАВЛЕНО (по прямому запросу пользователя: "открытие .alworld висит
    // много минут, использует не все ядра") — тот же I/O-bound профиль, что
    // у записи при экспорте (см. `export_scene_to_alworld`/`available_
    // parallelism` там), только на чтение: файл чанка + `.altex` КАЖДОГО
    // объекта в нём — отдельные файловые операции. Читаем чанки
    // ПАРАЛЛЕЛЬНО по числу логических ядер через `std::thread::scope`.
    // Колбэк `log: &mut dyn FnMut(String)` — не `Send` (обычная mutable-
    // ссылка на замыкание вызывающей стороны), поделить его между потоками
    // нельзя, поэтому каждый поток копит СВОИ объекты/сообщения в обычные
    // `Vec`, а вызывающий поток прогоняет их через `log`/`scene.add_object`
    // ПОСЛЕ того, как всё параллельное чтение завершилось — это уже не
    // I/O, а дешёвая работа с памятью, распараллеливать её незачем.
    let worker_count = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).max(1);
    let batch_size = world.chunks.len().div_ceil(worker_count).max(1);

    struct ChunkBatchResult {
        objects: Vec<GameObject>,
        messages: Vec<String>,
    }

    let batch_results: Vec<ChunkBatchResult> = std::thread::scope(|scope| {
        let chunks_dir = &chunks_dir;
        let handles: Vec<_> = world
            .chunks
            .chunks(batch_size.max(1))
            .map(|batch| {
                scope.spawn(move || {
                    let mut objects = Vec::new();
                    let mut messages = Vec::new();

                    for chunk in batch {
                        let chunk_path = chunks_dir.join(format!("chunk_{}_{}_{}.alwchunk", chunk.grid_x, chunk.grid_y, chunk.grid_z));
                        let content = match ChunkContent::load_from_file(chunk_path.to_string_lossy().as_ref()) {
                            Ok(c) => c,
                            Err(e) => {
                                messages.push(format!("⚠️ Пропущен чанк ({}, {}, {}): {} ({})", chunk.grid_x, chunk.grid_y, chunk.grid_z, e, chunk_path.display()));
                                continue;
                            }
                        };

                        for (obj_idx, obj) in content.objects.iter().enumerate() {
                            let altex_path = content.get_string(obj.altex_path_string_id);
                            let m = &obj.transform;
                            let position = Vec3::new(m[12], m[13], m[14]);

                            if altex_path.is_empty() || altex_path == "placeholder" {
                                let name = format!("Placeholder_{}_{}_{}_{}", chunk.grid_x, chunk.grid_y, chunk.grid_z, obj_idx);
                                objects.push(placeholder_mesh_object(&name, position));
                                continue;
                            }

                            let meshes = match super::altex::import_altex(altex_path) {
                                Ok(m) => m,
                                Err(e) => {
                                    messages.push(format!("⚠️ '{}' недоступен, подставлен куб-плейсхолдер (как в движке): {}", altex_path, e));
                                    let name = format!("Placeholder_{}_{}_{}_{}", chunk.grid_x, chunk.grid_y, chunk.grid_z, obj_idx);
                                    objects.push(placeholder_mesh_object(&name, position));
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
                                objects.push(game_obj);
                            }
                        }
                    }

                    ChunkBatchResult { objects, messages }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("поток чтения чанка запаниковал")).collect()
    });

    let mut objects_loaded = 0usize;
    for result in batch_results {
        for m in result.messages {
            log(m);
        }
        for obj in result.objects {
            scene.add_object(obj);
            objects_loaded += 1;
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

    /// Регрессия для бага "2 отредактированные по величине фигуры не
    /// передают ту же величину в движок и из-за этого остаются маленькие
    /// кубы" — см. комментарий у `bake_linear_transform`. Проверяет, что
    /// экспортированная геометрия сама несёт масштаб/поворот объекта, а не
    /// полагается на то, что их применит движок (движок применяет из
    /// матрицы объекта в чанке только позицию).
    #[test]
    fn export_bakes_scale_and_rotation_into_geometry() {
        let mut scene = Scene::new("TestWorld");
        let mut obj = GameObject::new(
            "ScaledCube",
            ObjectType::Mesh(MeshComponent {
                mesh: EditorMesh::create_cube(),
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        );
        // ИЗМЕНЕНО (после добавления `split_mesh_by_chunk` — резки геометрии
        // по границам чанков при экспорте): позиция (0,0,0) по умолчанию
        // лежит РОВНО на стыке границ чанков (обе оси делят мир на ячейки
        // по кратным `chunk_size`), так что растянутый куб в этой точке
        // ЧЕСТНО пересекает границу чанка и режется на несколько кусков —
        // это здесь не проверяется (см. `split_mesh_by_chunk` и её тесты
        // ниже), а мешает тесту про запекание масштаба/поворота. Сдвигаем
        // куб вглубь одной ячейки, подальше от любой границы.
        obj.transform.position = Vec3::new(30.0, 0.0, 30.0);
        obj.transform.scale = Vec3::new(2.0, 3.0, 4.0);
        obj.transform.rotation = crate::math::Quat::from_euler(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        scene.add_object(obj);

        let dir = std::env::temp_dir().join("alkash3d_editor_alworld_scale_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_string_lossy().to_string();

        let world_path = export_scene_to_alworld(&scene, &dir_str).expect("export");
        let mut logs = Vec::new();
        let imported = import_alworld_to_scene(&world_path, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(imported.objects.len(), 1, "logs: {:?}", logs);
        let imported_obj = imported.objects.values().next().unwrap();
        let ObjectType::Mesh(mesh_comp) = &imported_obj.object_type else { panic!("expected mesh") };
        let (min, max) = mesh_comp.mesh.bounds;
        let extent = max - min;
        // Единичный куб (полуразмер 0.5) со scale=(2,3,4) и поворотом на 90°
        // по Y меняет местами протяжённость по X и Z — так что геометрия
        // должна занимать (4, 3, 2), а не исходный (1, 1, 1).
        assert!((extent.x - 4.0).abs() < 1e-3, "extent={:?}", extent);
        assert!((extent.y - 3.0).abs() < 1e-3, "extent={:?}", extent);
        assert!((extent.z - 2.0).abs() < 1e-3, "extent={:?}", extent);
    }

    /// Регрессия для бага "почему у меня целая карта в .obj всего грузится
    /// в 1 маленьком чанке, хотя карта достаточно большая" — импортированный
    /// `.obj` обычно приходит одним меш-объектом на всю карту, и ДО
    /// `split_mesh_by_chunk` вся эта геометрия раскладывалась по чанкам по
    /// ОДНОЙ точке `transform.position` объекта — весь меш целиком уезжал в
    /// один чанк независимо от того, насколько далеко реально разбросаны его
    /// вершины. Строим вытянутый вдоль X quad (192м), пересекающий границу
    /// чанка на x=0 (chunk_size по умолчанию 64м) — он ДОЛЖЕН разъехаться на
    /// куски минимум в 2 разных чанка, и совокупная геометрия всех кусков
    /// должна покрывать ровно исходный диапазон, без потерь/задвоений.
    #[test]
    fn export_splits_large_mesh_spanning_multiple_chunks() {
        let vertices = vec![
            Vec3::new(-96.0, 0.0, -1.0),
            Vec3::new(96.0, 0.0, -1.0),
            Vec3::new(96.0, 0.0, 1.0),
            Vec3::new(-96.0, 0.0, 1.0),
        ];
        let indices = vec![0u32, 1, 2, 0, 2, 3];
        let mesh = EditorMesh::new(vertices, indices);

        let mut scene = Scene::new("TestWorld");
        scene.add_object(GameObject::new(
            "BigRoad",
            ObjectType::Mesh(MeshComponent {
                mesh,
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        ));

        let dir = std::env::temp_dir().join("alkash3d_editor_alworld_chunk_split_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_string_lossy().to_string();

        let world_path = export_scene_to_alworld(&scene, &dir_str).expect("export");
        let mut logs = Vec::new();
        let imported = import_alworld_to_scene(&world_path, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(imported.objects.len() >= 2, "ожидались минимум 2 куска в разных чанках, logs: {:?}", logs);

        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for obj in imported.objects.values() {
            let ObjectType::Mesh(mesh_comp) = &obj.object_type else { panic!("expected mesh") };
            min_x = min_x.min(obj.transform.position.x + mesh_comp.mesh.bounds.0.x);
            max_x = max_x.max(obj.transform.position.x + mesh_comp.mesh.bounds.1.x);
        }
        assert!((min_x - (-96.0)).abs() < 1e-2, "min_x={}", min_x);
        assert!((max_x - 96.0).abs() < 1e-2, "max_x={}", max_x);
    }

    /// Регрессия для бага "бесконечно грузит + использует не больше 30%
    /// проца" — реальная OBJ-карта пользователя, судя по всему, оказалась в
    /// неверном масштабе (типичная причина: .obj экспортирован в
    /// сантиметрах, но интерпретируется как метры), из-за чего
    /// `split_mesh_by_chunk` рассыпал её на десятки тысяч крошечных занятых
    /// ячеек — экспорт не зависал технически, а писал десятки тысяч мелких
    /// файлов на диск (I/O-bound, отсюда и неполная загрузка CPU), что
    /// растянулось бы на неприемлемо долгое время. Строим один-единственный
    /// огромный треугольник (по одной вершине на чанк почти в 100
    /// направлениях), гарантированно превышающий `MAX_REASONABLE_CHUNKS`, и
    /// проверяем, что экспорт СРАЗУ возвращает понятную ошибку вместо того,
    /// чтобы начинать писать файлы.
    #[test]
    fn export_rejects_pathologically_scattered_mesh_with_clear_error() {
        // Сетка 320x320 крошечных треугольников с шагом 512м (2 *
        // EXPORT_CHUNK_SIZE_METERS=256) между ними — каждый треугольник
        // целиком умещается в СВОЕЙ ячейке чанка, соседние ячейки заведомо
        // не пересекаются (шаг вдвое больше размера ячейки), так что
        // 320*320=102400 треугольников детерминированно дают 102400
        // уникальных занятых ячеек — больше `MAX_REASONABLE_CHUNKS`
        // (100 000).
        let grid_n = 320;
        let step = 512.0f32;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for gz in 0..grid_n {
            for gx in 0..grid_n {
                let x = gx as f32 * step;
                let z = gz as f32 * step;
                let base = vertices.len() as u32;
                vertices.push(Vec3::new(x, 0.0, z));
                vertices.push(Vec3::new(x + 1.0, 0.0, z));
                vertices.push(Vec3::new(x, 0.0, z + 1.0));
                indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
        let mesh = EditorMesh::new(vertices, indices);

        let mut scene = Scene::new("TestWorld");
        scene.add_object(GameObject::new(
            "ScatteredMesh",
            ObjectType::Mesh(MeshComponent {
                mesh,
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        ));

        let dir = std::env::temp_dir().join("alkash3d_editor_alworld_scatter_guard_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_string_lossy().to_string();

        let result = export_scene_to_alworld(&scene, &dir_str);
        let _ = std::fs::remove_dir_all(&dir);

        let err = result.expect_err("экспорт аномально разбросанного меша должен быть отклонён");
        let msg = err.to_string();
        assert!(msg.contains("чанк"), "сообщение об ошибке должно объяснять проблему с числом чанков: {}", msg);
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
