//! Стриминг открытого мира: рантайм-состояние чанков (`WorldStreamingState`/
//! `ChunkRuntimeState`), параллельная фоновая загрузка .alwchunk/.altex
//! (`ChunkLoadResult`, `load_chunk_data`) и методы движка — загрузка/
//! выгрузка мира, постановка чанков в очередь по дистанции от камеры,
//! размазанный по кадрам бюджетный ввод-вывод.
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись — видимость `WorldStreamingState`/`ChunkLoadResult`
//! поднята, т.к. на них ссылаются поля `AlkashEngine` и `AlkashEngine::new()`,
//! объявленные в родительском модуле `engine`.
//!
//! ИЗМЕНЕНО (масштабирование под открытый мир городского размера — раньше
//! ОДИН выделенный поток-загрузчик обрабатывал ВСЕ запросы строго
//! последовательно: если камера резким рывком (быстрая машина, телепорт)
//! заводила в `load_distance` сразу 20-30 чанков городской плотности, они
//! реально появлялись в сцене ОДИН ЗА ДРУГИМ, растянуто по времени на
//! сумму их индивидуальных чтений с диска — никакой параллельности между
//! НИМИ не было, только между ЭТИМ потоком и рендером): теперь каждый
//! запрос загрузки чанка — отдельная `TaskPriority::Low`-задача,
//! отправляемая в уже существовавший, но НИГДЕ не использовавшийся
//! `AlkashEngine::scheduler` (`EngineScheduler::execute` → `WorkerPool`,
//! `light_workers` — по одному воркеру на ядро, см. `scheduler/pool.rs`).
//! Несколько чанков теперь реально читаются и парсятся ОДНОВРЕМЕННО на
//! разных ядрах. `altex_parse_cache` стал полем `AlkashEngine`
//! (`Arc<Mutex<...>>`, а не локальный `HashMap` внутри одного потока) —
//! см. подробное обоснование гонки/её цены у `load_chunk_data`.

use std::sync::{Arc, Mutex};
use crate::math::Vec3;
use crate::plugin::GPULight;
use crate::scheduler::{Task, TaskPriority};
use super::AlkashEngine;

/// Рантайм-состояние ОДНОГО чанка — какие сущности Scene сейчас
/// представляют его содержимое (если он загружен). Отдельная от
/// `alworld_format::ChunkDescriptor` структура — та описывает чанк НА
/// ДИСКЕ (позиция, объём, где искать данные), эта — чанк В ПАМЯТИ ДВИЖКА
/// (что из него реально заспавнено в Scene прямо сейчас).
#[derive(Debug, Clone, Default)]
struct ChunkRuntimeState {
    /// Заспавненные `EntityId` объектов этого чанка — при выгрузке чанка
    /// каждый despawn'ится (см. `unload_chunk`). Пусто, пока чанк не
    /// загружен.
    spawned_entities: Vec<crate::scene::EntityId>,
    /// ДОБАВЛЕНО (объединённая сцена — физика из .alworld): id физических
    /// тел (`PhysicsPlugin::add_body`/`add_sphere_body`), созданных для
    /// объектов ЭТОГО чанка с флагом `CHUNK_OBJECT_FLAG_HAS_PHYSICS` (см.
    /// `load_chunk`). Отдельно от `spawned_entities` — физическое тело и
    /// визуальная ECS-сущность живут в РАЗНЫХ системах (плагин Inertial
    /// / `Scene`), и `unload_chunk` обязан почистить обе, иначе
    /// физическое тело осиротевшего объекта продолжало бы столкновения и
    /// тратить CPU-время в broad/narrow phase даже после того, как его
    /// чанк давно выгружен и визуально не существует — на большом
    /// открытом мире с активным стримингом это накапливалось бы без
    /// предела за время игровой сессии (утечка физических тел).
    spawned_physics_bodies: Vec<i32>,
    /// true, если чанк прямо сейчас считается загруженным (есть
    /// заспавненные сущности ИЛИ чанк пуст, но всё равно отмечен
    /// загруженным, чтобы не пытаться перезагружать его каждый кадр).
    loaded: bool,
    /// ИСПРАВЛЕНО (баг: "фризы как будто GC" при ходьбе — жалоба
    /// пользователя, см. `WorldStreamingState::pending_load`): true, если
    /// чанк уже поставлен в очередь `pending_load`/`pending_unload`, но
    /// ещё физически не обработан `drain_pending_chunk_io`. Нужно, чтобы
    /// `update_world_streaming` не добавил один и тот же чанк в очередь
    /// повторно при следующем пересчёте (каждые
    /// `WORLD_STREAMING_INTERVAL_FRAMES` кадров), пока он ещё ждёт своей
    /// очереди.
    queued: bool,
}

/// Полное рантайм-состояние стриминга — хранится в
/// `AlkashEngine::world`. Содержит и метаданные (`AlworldFile`), и
/// рантайм-карту "чанк -> что из него сейчас в Scene" (`chunk_states`,
/// индексируется ТЕМ ЖЕ индексом, что и `world_file.chunks` — оба Vec
/// всегда одной длины, поддерживается инвариантом в `load_world`).
pub struct WorldStreamingState {
    world_file: crate::alworld_format::AlworldFile,
    /// Директория, где лежат файлы содержимого чанков (`chunk_X_Y_Z.alwchunk`)
    /// — вычисляется от пути к .alworld при `load_world` (тот же каталог,
    /// подпапка "chunks").
    chunks_dir: std::path::PathBuf,
    chunk_states: Vec<ChunkRuntimeState>,
    /// Мировая позиция, для которой стриминг считался В ПОСЛЕДНИЙ РАЗ —
    /// используется, чтобы не пересчитывать дистанции до ВСЕХ чанков
    /// каждый кадр без надобности (см. `update_world_streaming` —
    /// пересчёт происходит, только если камера сдвинулась заметно, либо
    /// раз в несколько кадров — компромисс между отзывчивостью стриминга
    /// и CPU-стоимостью обхода потенциально тысяч чанков каждый кадр).
    last_streaming_origin: Vec3,
    /// Счётчик кадров с последнего полного пересчёта стриминга — см.
    /// `WORLD_STREAMING_INTERVAL_FRAMES`.
    frames_since_streaming_update: u32,
    /// Сколько чанков реально загружено прямо сейчас — для диагностики/
    /// логов, не участвует в логике загрузки напрямую.
    loaded_chunk_count: usize,
    /// ИСПРАВЛЕНО (баг: "фризы как будто GC" при ходьбе — жалоба
    /// пользователя): `load_chunk`/`unload_chunk` синхронно читают файл
    /// чанка с диска и парсят его ПРЯМО в кадре рендера (см. `load_chunk`
    /// — `ChunkContent::load_from_file` блокирующий). Раньше
    /// `update_world_streaming` находил ВСЕ чанки, попавшие в
    /// load_distance за один пересчёт, и грузил их ВСЕ в одном кадре — при
    /// первом входе в мир или после резкого скачка позиции камеры (или
    /// после нескольких пропущенных пересчётов, см.
    /// `WORLD_STREAMING_INTERVAL_FRAMES`) это могло быть сразу 10-30+
    /// чанков разом, что и ощущается как долгий стоп-пауза ("будто GC"),
    /// хотя в Rust нет GC — тормозит именно синхронный дисковый I/O кучей
    /// файлов подряд на одном кадре. Фикс — очередь: `update_world_streaming`
    /// теперь только ОПРЕДЕЛЯЕТ, какие чанки нужно загрузить/выгрузить, и
    /// складывает их сюда, а реальная загрузка размазывается по
    /// нескольким последующим кадрам с бюджетом `CHUNK_LOAD_BUDGET_PER_FRAME`
    /// чанков за кадр (см. `drain_pending_chunk_io`, вызывается из
    /// `update()` каждый кадр, а не только когда истёк интервал пересчёта).
    pending_load: Vec<usize>,
    pending_unload: Vec<usize>,
}

/// ДОБАВЛЕНО (World Streaming): стриминг пересчитывается не КАЖДЫЙ кадр, а
/// раз в это число кадров — обход дескрипторов тысяч чанков (вычисление
/// дистанции камера-чанк для каждого) на каждый ЕДИНСТВЕННЫЙ кадр был бы
/// заметной и совершенно ненужной тратой CPU-бюджета кадра: чанки размером
/// в десятки метров не требуют реакции быстрее нескольких кадров — камера
/// физически не успевает пересечь границу load/unload-дистанции за 1/60
/// секунды при разумных скоростях перемещения.
const WORLD_STREAMING_INTERVAL_FRAMES: u32 = 15;

/// ИСПРАВЛЕНО (фризы при стриминге, см. комментарий у `pending_load`
/// выше): сколько чанков максимум грузится/выгружается за ОДИН кадр из
/// накопленной очереди. 2 — консервативный выбор специально под "минимум
/// 10-летнее железо" из ТЗ (медленный HDD/eMMC на такой машине делает
/// даже один синхронный файловый I/O заметным на кадре) — при обычной
/// ходьбе очередь почти всегда пуста или содержит 1 чанк за раз, бюджет
/// реально ограничивает только "взрывные" случаи (первый вход в мир,
/// резкий скачок позиции камеры, телепорт).
const CHUNK_LOAD_BUDGET_PER_FRAME: usize = 2;

/// Результат фоновой загрузки — ПОЛНОСТЬЮ CPU-данные. `ChunkContent`
/// (alworld_format.rs) и `AltexFile` (altex_format.rs) — оба обычные Rust-
/// структуры (Vec/String/POD-поля, без Rc/RefCell/сырых GPU-хендлов), а
/// их методы `load`/`load_from_file` — это только `std::fs::File` +
/// побайтовый парсинг (проверено построчно, ни один вызов не трогает
/// windows/Direct3D12 API), поэтому оба типа безопасно передаются между
/// потоками (`Send`), и весь этот путь физически не может случайно
/// затронуть D3D12 из фонового потока/потока пула планировщика.
pub(super) struct ChunkLoadResult {
    chunk_idx: usize,
    generation: u64,
    content: crate::alworld_format::ChunkContent,
    /// Распарсенные `.altex` каждого УНИКАЛЬНОГО пути среди объектов
    /// ЭТОГО чанка (`Arc`, чтобы несколько объектов одного чанка —
    /// например, фонарь + его же плафон одним файлом — не копировали
    /// геометрию, а разделяли один и тот же разбор). `Err(String)` — та
    /// же отказоустойчивость, что была в старом синхронном
    /// `load_object_mesh`: файл не найден/повреждён, главный поток
    /// заменит объект placeholder-кубом, а не уронит весь чанк или тем
    /// более поток пула целиком.
    parsed_altex: std::collections::HashMap<String, std::result::Result<Arc<crate::altex_format::AltexFile>, String>>,
}

/// Тип кэша "путь `.altex` -> уже распарсенный файл", РАЗДЕЛЯЕМЫЙ между
/// ВСЕМИ параллельными загрузками чанков (см. развёрнутое обоснование
/// перехода на пул планировщика в шапке модуля). Псевдоним — чтобы не
/// повторять этот громоздкий тип в каждой сигнатуре ниже и в поле
/// `AlkashEngine::altex_parse_cache`.
pub(super) type AltexParseCache = Arc<Mutex<std::collections::HashMap<String, Arc<crate::altex_format::AltexFile>>>>;

/// Читает и парсит ОДИН чанк мира (.alwchunk) и ВСЕ `.altex`-файлы, на
/// которые ссылаются его объекты — выполняется на воркер-потоке
/// `EngineScheduler` (`TaskPriority::Low`, см. `request_chunk_load`), НЕ
/// на главном потоке.
///
/// `altex_cache` теперь РАЗДЕЛЯЕТСЯ между всеми одновременно выполняющимися
/// загрузками (раньше был локальным `HashMap` внутри единственного
/// выделенного потока-загрузчика, где гонки не могло быть по построению —
/// см. историю в шапке модуля). Гонка "два воркера одновременно не находят
/// путь в кэше и оба парсят один и тот же файл" возможна и НАМЕРЕННО не
/// устраняется блокировкой на всё время парсинга: держать мьютекс на
/// время файлового I/O одного чанка означало бы сериализовать ВСЕ
/// параллельные загрузки друг за другом — то есть свести на нет саму
/// причину перехода на пул воркеров. Редкий дублирующий разбор одного и
/// того же типового `.altex` (последняя запись в кэш просто перезатирает
/// предыдущую — оба Arc валидны, ничего не портится) на практике намного
/// дешевле этой сериализации.
fn load_chunk_data(
    chunk_path: &std::path::Path,
    altex_cache: &AltexParseCache,
) -> (
    crate::alworld_format::ChunkContent,
    std::collections::HashMap<String, std::result::Result<Arc<crate::altex_format::AltexFile>, String>>,
) {
    let content = match crate::alworld_format::ChunkContent::load_from_file(chunk_path.to_string_lossy().as_ref()) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[CHUNK-LOADER] WARNING: не удалось прочитать чанк {:?}: {:?}", chunk_path, e);
            }
            crate::alworld_format::ChunkContent::new()
        }
    };

    let mut parsed_altex = std::collections::HashMap::new();
    for obj in &content.objects {
        let altex_path = content.get_string(obj.altex_path_string_id).to_string();
        if altex_path.is_empty() || altex_path == "placeholder" || parsed_altex.contains_key(&altex_path) {
            continue;
        }

        let cached = altex_cache.lock().unwrap().get(&altex_path).cloned();
        let parsed = if let Some(arc) = cached {
            Ok(arc)
        } else {
            match crate::altex_format::AltexFile::load(&altex_path) {
                Ok(file) => {
                    let arc = Arc::new(file);
                    altex_cache.lock().unwrap().insert(altex_path.clone(), Arc::clone(&arc));
                    Ok(arc)
                }
                Err(e) => Err(format!("{:?}", e)),
            }
        };
        parsed_altex.insert(altex_path, parsed);
    }

    (content, parsed_altex)
}

impl AlkashEngine {
    /// ДОБАВЛЕНО (Фаза 1 плана по реализму/фонарям): загружает .alfar с
    /// диска и превращает каждый `IndividualLight` в `GPULight`, добавляя
    /// его в уже инициализированный `LightPlugin` (FirstFires) — то есть в
    /// ТОТ ЖЕ путь данных, которым раньше пользовался только
    /// `add_street_light`.
    ///
    /// До этого метода .alfar использовался ТОЛЬКО на запись (`save()`) —
    /// у движка не было пути "прочитать файл со светом обратно". Теперь
    /// есть: `.alfar` можно готовить заранее (в т.ч. через
    /// `AlfarFile::create_night_city()` или редактором в будущем) и
    /// загружать в сцену одним вызовом.
    ///
    /// ВАЖНО про формат конвертации `IndividualLight` -> `GPULight` (см.
    /// `plugin/light_api.rs`, layout подтверждён по `light_culling.hlsl` и
    /// по demo.rs FirstFires):
    ///   position = [x, y, z, light_type]   (w = тип: 0=point,1=spot,2=dir)
    ///   color    = [r, g, b, intensity]
    ///   direction= [dx, dy, dz, range]
    ///   params   = [spot_outer_angle, falloff_type, spot_inner_angle, padding(0)]
    ///
    /// ОБНОВЛЕНО (Фаза 4 плана по реализму/фонарям): params.z раньше был
    /// зарезервирован под "lod" (см. комментарий в GPULight в
    /// alkash3d-FirstFires/src/lib.rs), но реально нигде в цепочке
    /// FirstFires -> движок не читался и не записывался как LOD — сам LOD
    /// вычисляется отдельно внутри `LightState::cull()` и хранится в
    /// `LightGridEntry.lod_level`, а не в GPULight.params.z. Поэтому это
    /// поле было мёртвым — переиспользовано под `spot_inner_angle`, чтобы
    /// не расширять GPULight ещё одним полем (и не трогать ABI FirstFires
    /// повторно вдобавок к Фазе 3). Если это поле когда-нибудь понадобится
    /// именно под LOD — потребуется либо снова его переиспользовать другим
    /// способом, либо добавить пятое поле в GPULight.
    ///
    /// Не переносится в этой фазе (сознательно отложено, а не забыто —
    /// сохранённые вместе поля будут нужны в следующих фазах, а не сейчас):
    /// casts_shadows/shadow_bias/shadow_resolution (эти три — часть Фазы 6,
    /// уже сделанной для одного directional-света "солнца", но per-light
    /// точечные/spot тени в отдельные shadow map всё ещё не реализованы —
    /// остаются на будущее улучшение), falloff_custom (полноценный
    /// IES-профиль, возможное будущее улучшение Фазы 4 для топового
    /// железа).
    ///
    /// ОБНОВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
    /// мерцание): flicker_enabled/flicker_speed/flicker_intensity и
    /// active_from/active_to теперь СОХРАНЯЮТСЯ (см. `ManagedLight` и
    /// `self.managed_lights` выше) — `update_day_night` каждый кадр
    /// использует их, чтобы промодулировать intensity уже добавленного в
    /// FirstFires источника через `LightPlugin::update_light`.
    ///
    /// `enabled == 0` источники пропускаются — нет смысла тратить слот в
    /// FirstFires на свет, который автор сцены явно выключил.
    pub fn load_lights_from_alfar(&mut self, path: &str) -> std::io::Result<u32> {
        let alfar = crate::alfar_format::AlfarFile::load(path)?;

        let mut added = 0u32;
        for light in &alfar.lights {
            if light.enabled == 0 {
                continue;
            }

            let light_type = light.light_type as f32;
            let gpu_light = GPULight {
                position: [light.position[0], light.position[1], light.position[2], light_type],
                color: [light.color[0], light.color[1], light.color[2], light.intensity],
                direction: [light.direction[0], light.direction[1], light.direction[2], light.range],
                params: [light.spot_outer_angle, light.falloff_type as f32, light.spot_inner_angle, 0.0],
            };

            if let Some(firstfires_id) = self.lights.as_mut().map(|l| l.add_light(&gpu_light)) {
                added += 1;

                if light.light_type != crate::alfar_format::LightType::Directional as u32 {
                    self.managed_lights.push(super::ManagedLight {
                        firstfires_id,
                        position: light.position,
                        light_type,
                        color: light.color,
                        base_intensity: light.intensity,
                        direction: light.direction,
                        range: light.range,
                        params: [light.spot_outer_angle, light.falloff_type as f32, light.spot_inner_angle, 0.0],
                        flicker_enabled: light.flicker_enabled != 0,
                        flicker_speed: light.flicker_speed,
                        flicker_intensity: light.flicker_intensity,
                        active_from: light.active_from,
                        active_to: light.active_to,
                    });
                    self.flicker_phase.push(0.0);
                }
            } else {
                eprintln!("[ENGINE] load_lights_from_alfar: LightPlugin не инициализирован (вызови init_lights() раньше) — свет '{}' пропущен", light.id);
                break;
            }
        }

        println!(
            "[ENGINE] ✓ .alfar загружен: '{}' — {} из {} источников добавлено в LightPlugin ({} под управлением день/ночь+мерцание)",
            path, added, alfar.lights.len(), self.managed_lights.len()
        );

        self.light_ambient = Some(alfar.ambient);
        self.light_global_settings = Some(alfar.global_settings);

        Ok(added)
    }

    /// Загружает .alworld файл (метаданные мира — где какие чанки, размер
    /// чанка, streaming config) и переводит движок в режим стриминга этого
    /// мира. НЕ загружает содержимое ни одного чанка сразу — это делает
    /// `update_world_streaming`, вызываемый каждый кадр из `update()`, по
    /// мере приближения камеры (тот же принцип "загружаем только то, что
    /// реально нужно прямо сейчас", ради которого стриминг вообще
    /// существует).
    ///
    /// `chunks_dir` — папка, где лежат файлы содержимого чанков
    /// (`chunk_{x}_{y}_{z}.alwchunk`, см. `chunk_file_path`). Если `None`,
    /// используется подпапка `chunks` рядом с самим .alworld файлом —
    /// разумный дефолт, соответствующий тому, как `create_open_world_demo`-
    /// подобные инструменты обычно раскладывают файлы на диске.
    pub fn load_world(&mut self, alworld_path: &str, chunks_dir: Option<&str>) -> std::io::Result<()> {
        let world_file = crate::alworld_format::AlworldFile::load(alworld_path)?;

        let chunks_dir = match chunks_dir {
            Some(dir) => std::path::PathBuf::from(dir),
            None => {
                let mut dir = std::path::PathBuf::from(alworld_path);
                dir.pop();
                dir.push("chunks");
                dir
            }
        };

        let chunk_count = world_file.chunks.len();
        println!(
            "[ENGINE] ✓ Мир загружен: {} ({} чанков по {}м, дальность загрузки {}м/выгрузки {}м)",
            alworld_path, chunk_count, world_file.header.chunk_size,
            world_file.streaming_config.load_distance, world_file.streaming_config.unload_distance,
        );

        self.world_generation = self.world_generation.wrapping_add(1);

        self.world = Some(WorldStreamingState {
            world_file,
            chunks_dir,
            chunk_states: vec![ChunkRuntimeState::default(); chunk_count],
            last_streaming_origin: Vec3::new(f32::MAX, f32::MAX, f32::MAX),
            frames_since_streaming_update: WORLD_STREAMING_INTERVAL_FRAMES,
            loaded_chunk_count: 0,
            pending_load: Vec::new(),
            pending_unload: Vec::new(),
        });

        Ok(())
    }

    /// ДОБАВЛЕНО (точка спавна игрока — по прямому запросу пользователя):
    /// позиция и угол поворота по Y (yaw, радианы), если в загруженном
    /// мире есть отмеченный `GlobalObject` — см. подробное объяснение у
    /// `alworld_format::GLOBAL_OBJECT_FLAG_SPAWN_POINT`. `None`, если мир
    /// не загружен вообще, или загружен, но без точки спавна (например,
    /// `create_and_save_demo_world` её не кладёт) — вызывающий код сам
    /// решает, каким запасным значением пользоваться в этом случае (см.
    /// `main.rs`/`main_car.rs`, где раньше начальная позиция камеры была
    /// жёстко захардкожена, а теперь используется как fallback).
    pub fn world_spawn_point(&self) -> Option<(crate::math::Vec3, f32)> {
        let world = self.world.as_ref()?;
        let spawn = world.world_file.global_objects.iter()
            .find(|g| g.flags & crate::alworld_format::GLOBAL_OBJECT_FLAG_SPAWN_POINT != 0)?;
        let m = &spawn.transform;
        Some((crate::math::Vec3::new(m[12], m[13], m[14]), spawn.lod_distances[0]))
    }

    /// ДОБАВЛЕНО (World Streaming — подключение к движку): создаёт
    /// небольшой демонстрационный мир на диске (см.
    /// `AlworldFile::create_and_save_demo_world`) и сразу загружает его
    /// через `load_world` — удобный способ проверить стриминг за один
    /// вызов, без ручной подготовки .alworld/.alwchunk файлов. `dir` —
    /// папка, куда будет сохранён демо-мир (например, рядом с exe).
    pub fn load_demo_world(&mut self, dir: &str) -> std::io::Result<()> {
        let alworld_path = crate::alworld_format::AlworldFile::create_and_save_demo_world(dir)?;
        self.load_world(&alworld_path, None)
    }

    /// Выгружает ВСЕ загруженные чанки (despawn всех их сущностей из
    /// Scene) и сбрасывает состояние стриминга — используется, например,
    /// при переходе на другой уровень/мир, чтобы не оставлять "осиротевшие"
    /// сущности предыдущего мира в Scene.
    ///
    /// ИЗМЕНЕНО (фоновая загрузка чанков): раньше `self.world.take()` сам
    /// по себе гарантированно уничтожал ЛЮБОЕ незавершённое состояние
    /// загрузки — `pending_load`/`pending_unload` жили ВНУТРИ
    /// `WorldStreamingState`, забираемой здесь. Но фоновая задача пула
    /// планировщика (см. `request_chunk_load`) может в этот самый момент
    /// ещё выполняться, читая чанк ЭТОГО мира — её результат придёт в
    /// канал уже ПОСЛЕ того, как `self.world`
    /// станет `None` (или будет заменён совсем другим миром через
    /// повторный `load_world`). Без явного bump'а `world_generation` этот
    /// "осиротевший" результат прошёл бы проверку в
    /// `drain_pending_chunk_io` (её generation ещё совпадала бы) и
    /// заспавнил бы сущности ВЫГРУЖЕННОГО мира — утечка сущностей/
    /// физических тел, которые никто и никогда не отследит и не удалит.
    /// Тот же bump, что и в `load_world`, закрывает эту дыру.
    pub fn unload_world(&mut self) {
        self.world_generation = self.world_generation.wrapping_add(1);

        if let Some(mut world) = self.world.take() {
            let mut all_physics_bodies = Vec::new();
            for state in &mut world.chunk_states {
                for entity in state.spawned_entities.drain(..) {
                    self.scene.despawn(entity);
                }
                all_physics_bodies.extend(state.spawned_physics_bodies.drain(..));
                state.loaded = false;
            }
            if !all_physics_bodies.is_empty() {
                if let Some(physics) = self.physics.as_mut() {
                    for body_id in &all_physics_bodies {
                        physics.remove_body(*body_id);
                    }
                }
                self.physics_links.retain(|(id, _)| !all_physics_bodies.contains(id));
            }
            println!("[ENGINE] Мир выгружен, все чанки despawn'нуты");
        }
    }

    /// Путь к файлу содержимого чанка с заданными сеточными координатами —
    /// единая точка формирования имени файла, используется и при загрузке
    /// (`load_chunk`), и внешними инструментами экспорта мира должны
    /// придерживаться того же соглашения об именовании
    /// (`chunk_{x}_{y}_{z}.alwchunk`), чтобы `load_chunk` их находил.
    fn chunk_file_path(chunks_dir: &std::path::Path, chunk: &crate::alworld_format::ChunkDescriptor) -> std::path::PathBuf {
        chunks_dir.join(format!("chunk_{}_{}_{}.alwchunk", chunk.grid_x, chunk.grid_y, chunk.grid_z))
    }

    /// Загружает содержимое ОДНОГО чанка (`ChunkContent` с диска, см.
    /// `alworld_format.rs`) и спавнит каждый его объект как отдельную
    /// сущность Scene (`Transform` из объектной 4x4-матрицы + `MeshRenderer`,
    /// ссылающийся на mesh_index geometry этого объекта). Если файл
    /// содержимого чанка отсутствует на диске (например, чанк объявлен в
    /// .alworld, но экспортёр мира ещё не сгенерировал для него данные) —
    /// НЕ ошибка, чанк просто помечается загруженным без объектов (пустой
    /// чанк, например открытое поле/вода без построек, вполне легитимен).
    ///
    /// ДОБАВЛЕНО (Задача #14): геометрия объектов чанка теперь загружается
    /// из реального `.altex` файла по пути объекта (см. `load_object_mesh_sync`)
    /// вместо всегда-плейсхолдера. Fallback на единичный куб
    /// (`load_placeholder_mesh`) остаётся ТОЛЬКО для случаев отсутствующего/
    /// повреждённого файла или служебного пути "placeholder" (см.
    /// `AlworldFile::create_and_save_demo_world`) — не блокирует стриминг
    /// целиком из-за одного плохого ассета.
    ///
    /// ПЕРЕИМЕНОВАНО (фоновая загрузка чанков): это ПОЛНОСТЬЮ синхронный
    /// путь (сама читает файл чанка И парсит .altex каждого объекта прямо
    /// здесь, в кадре рендера) — раньше был единственным способом загрузки
    /// чанка (`load_chunk`), теперь используется ТОЛЬКО как fallback на
    /// случай, если пул планировщика занят/CPU-бюджет исчерпан (см.
    /// `request_chunk_load`) — штатный путь теперь асинхронный
    /// (`request_chunk_load` -> задача `EngineScheduler` ->
    /// `integrate_loaded_chunk`). Тело функции НЕ менялось, только имя и
    /// единственный внутренний вызов `load_object_mesh` ->
    /// `load_object_mesh_sync` (переименован туда же, см. его комментарий).
    fn load_chunk_sync_fallback(&mut self, chunk_idx: usize) {
        let (chunk_path, chunk_desc) = {
            let world = match &self.world {
                Some(w) => w,
                None => return,
            };
            let chunk = &world.world_file.chunks[chunk_idx];
            (Self::chunk_file_path(&world.chunks_dir, chunk), *chunk)
        };

        let content = match crate::alworld_format::ChunkContent::load_from_file(chunk_path.to_string_lossy().as_ref()) {
            Ok(c) => c,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "[ENGINE] WARNING: не удалось прочитать чанк {:?} ({},{},{}): {:?}",
                        chunk_path, chunk_desc.grid_x, chunk_desc.grid_y, chunk_desc.grid_z, e
                    );
                }
                crate::alworld_format::ChunkContent::new()
            }
        };

        let mut spawned = Vec::with_capacity(content.objects.len());
        let mut spawned_bodies = Vec::new();
        for obj in &content.objects {
            let m = &obj.transform;
            let position = [m[12], m[13], m[14]];

            let altex_path = content.get_string(obj.altex_path_string_id).to_string();
            let mesh_indices = self.load_object_mesh_sync(&altex_path);

            let physics_body_id = if obj.flags & crate::alworld_format::CHUNK_OBJECT_FLAG_HAS_PHYSICS != 0 {
                self.add_sphere_body(position[0], position[1], position[2], obj.mass)
            } else {
                None
            };

            let mut first_entity_of_object: Option<crate::scene::EntityId> = None;
            for mesh_index in mesh_indices {
                let entity = self.scene.spawn();
                if let Some(transform) = self.scene.transform_mut(entity) {
                    transform.position = position;
                }
                self.scene.add_mesh_renderer(entity, mesh_index);
                if first_entity_of_object.is_none() {
                    first_entity_of_object = Some(entity);
                }
                spawned.push(entity);
            }

            if let (Some(body_id), Some(entity)) = (physics_body_id, first_entity_of_object) {
                self.physics_links.push((body_id, entity));
                spawned_bodies.push(body_id);
            }
        }

        if let Some(world) = &mut self.world {
            let state = &mut world.chunk_states[chunk_idx];
            state.spawned_entities = spawned;
            state.spawned_physics_bodies = spawned_bodies;
            state.loaded = true;
            world.loaded_chunk_count += 1;
        }
    }

    /// ДОБАВЛЕНО (фоновая загрузка чанков — теперь через
    /// `AlkashEngine::scheduler`, см. развёрнутое обоснование в шапке
    /// модуля): отправляет чтение+разбор чанка `chunk_idx` как отдельную
    /// `TaskPriority::Low`-задачу в `EngineScheduler` (реально выполнится
    /// на одном из `light_workers` пула — по потоку на ядро, см.
    /// `scheduler/pool.rs`). НЕ блокирует главный поток — `execute` лишь
    /// проверяет CPU-бюджет и кладёт замыкание в очередь воркера, сам
    /// дисковый I/O произойдёт позже, параллельно с рендером текущего и
    /// следующих кадров (и параллельно с ЛЮБЫМИ другими одновременно
    /// запрошенными чанками — в отличие от одного выделенного потока
    /// раньше). Готовый результат будет подобран и интегрирован в
    /// GPU/Scene позже, в `drain_pending_chunk_io` (через
    /// `integrate_loaded_chunk`), когда придёт по `chunk_loader_rx`.
    ///
    /// `EngineScheduler::execute` возвращает `false`, если CPU-бюджет
    /// исчерпан (все ядра заняты другими задачами планировщика прямо
    /// сейчас) — в этом случае, как и раньше при недоступном канале,
    /// честный синхронный fallback на главном потоке вместо потери чанка.
    fn request_chunk_load(&mut self, chunk_idx: usize) {
        let chunk_path = {
            let Some(world) = &self.world else { return };
            let chunk = &world.world_file.chunks[chunk_idx];
            Self::chunk_file_path(&world.chunks_dir, chunk)
        };
        let generation = self.world_generation;
        let result_tx = self.chunk_loader_result_tx.clone();
        let altex_cache = Arc::clone(&self.altex_parse_cache);

        let dispatched = self.scheduler.execute(
            Task::new(chunk_idx as u32, TaskPriority::Low),
            move || {
                let (content, parsed_altex) = load_chunk_data(&chunk_path, &altex_cache);
                let _ = result_tx.send(ChunkLoadResult { chunk_idx, generation, content, parsed_altex });
            },
        );

        if !dispatched {
            eprintln!(
                "[ENGINE] WARNING: пул фоновой загрузки занят (CPU-бюджет исчерпан) — чанк {} загружается синхронно",
                chunk_idx
            );
            self.load_chunk_sync_fallback(chunk_idx);
            if let Some(world) = &mut self.world {
                if chunk_idx < world.chunk_states.len() {
                    world.chunk_states[chunk_idx].queued = false;
                }
            }
        }
    }

    /// ДОБАВЛЕНО (фоновая загрузка чанков): забирает результат, уже
    /// полностью прочитанный и распарсенный фоновым потоком (см.
    /// `ChunkLoadResult`), и делает единственную оставшуюся часть работы,
    /// которая ОБЯЗАНА выполняться на главном потоке — создание GPU-
    /// ресурсов (буферы мешей через `load_object_mesh_from_parsed`) и
    /// spawn сущностей Scene. Логика spawn/физики здесь ДОСЛОВНО совпадает
    /// с тем, что раньше делал `load_chunk_sync_fallback` (ныне
    /// `load_chunk_sync_fallback`) ПОСЛЕ чтения файла — единственное
    /// отличие в том, откуда берутся `content`/распарсенные `.altex`.
    fn integrate_loaded_chunk(&mut self, result: ChunkLoadResult) {
        let chunk_idx = result.chunk_idx;
        let content = result.content;
        let parsed_altex = result.parsed_altex;

        let mut spawned = Vec::with_capacity(content.objects.len());
        let mut spawned_bodies = Vec::new();
        for obj in &content.objects {
            let m = &obj.transform;
            let position = [m[12], m[13], m[14]];

            let altex_path = content.get_string(obj.altex_path_string_id).to_string();
            let mesh_indices = self.load_object_mesh_from_parsed(&altex_path, &parsed_altex);

            let physics_body_id = if obj.flags & crate::alworld_format::CHUNK_OBJECT_FLAG_HAS_PHYSICS != 0 {
                self.add_sphere_body(position[0], position[1], position[2], obj.mass)
            } else {
                None
            };

            let mut first_entity_of_object: Option<crate::scene::EntityId> = None;
            for mesh_index in mesh_indices {
                let entity = self.scene.spawn();
                if let Some(transform) = self.scene.transform_mut(entity) {
                    transform.position = position;
                }
                self.scene.add_mesh_renderer(entity, mesh_index);
                if first_entity_of_object.is_none() {
                    first_entity_of_object = Some(entity);
                }
                spawned.push(entity);
            }

            if let (Some(body_id), Some(entity)) = (physics_body_id, first_entity_of_object) {
                self.physics_links.push((body_id, entity));
                spawned_bodies.push(body_id);
            }
        }

        if let Some(world) = &mut self.world {
            if chunk_idx < world.chunk_states.len() {
                let state = &mut world.chunk_states[chunk_idx];
                state.spawned_entities = spawned;
                state.spawned_physics_bodies = spawned_bodies;
                state.loaded = true;
                state.queued = false;
                world.loaded_chunk_count += 1;
            }
        }
    }

    /// Выгружает содержимое ОДНОГО чанка — despawn всех его сущностей из
    /// Scene (см. `load_chunk`). Геометрия (placeholder-меш) НЕ удаляется —
    /// она общая и переиспользуется другими чанками/будущими загрузками
    /// того же чанка.
    fn unload_chunk(&mut self, chunk_idx: usize) {
        let (entities, physics_bodies) = if let Some(world) = &mut self.world {
            (
                std::mem::take(&mut world.chunk_states[chunk_idx].spawned_entities),
                std::mem::take(&mut world.chunk_states[chunk_idx].spawned_physics_bodies),
            )
        } else {
            return;
        };
        for entity in entities {
            self.scene.despawn(entity);
        }
        if !physics_bodies.is_empty() {
            if let Some(physics) = self.physics.as_mut() {
                for body_id in &physics_bodies {
                    physics.remove_body(*body_id);
                }
            }
            self.physics_links.retain(|(id, _)| !physics_bodies.contains(id));
        }
        if let Some(world) = &mut self.world {
            world.chunk_states[chunk_idx].loaded = false;
            world.loaded_chunk_count = world.loaded_chunk_count.saturating_sub(1);
        }
    }

    /// Вызывается каждый кадр из `update()` (см. ниже) — сравнивает
    /// текущую позицию камеры с позицией на момент последнего пересчёта
    /// стриминга (`last_streaming_origin`) и, если камера сдвинулась
    /// заметно ИЛИ прошло достаточно кадров (`WORLD_STREAMING_INTERVAL_FRAMES`
    /// — защита от "стоим на месте, но пересчитываем каждый кадр
    /// впустую"), обходит ВСЕ чанки мира и ОПРЕДЕЛЯЕТ, какие нужно
    /// загрузить (ближе `load_distance`) или выгрузить (дальше
    /// `unload_distance`, намеренно РАЗНЫЙ порог — гистерезис, см.
    /// подробное объяснение ниже, почему не один общий порог с загрузкой).
    ///
    /// ИСПРАВЛЕНО (баг: "фризы как будто GC" — см. подробности у поля
    /// `pending_load` в `WorldStreamingState`): эта функция теперь ТОЛЬКО
    /// складывает решения в очередь `pending_load`/`pending_unload` —
    /// реальный синхронный дисковый I/O (`load_chunk`/`unload_chunk`)
    /// вынесен в `drain_pending_chunk_io`, которая тратит на него
    /// ограниченный бюджет КАЖДЫЙ кадр (не только когда истёк интервал
    /// пересчёта), размазывая потенциально большую пачку чанков по многим
    /// кадрам вместо одного длинного застоя.
    pub(super) fn update_world_streaming(&mut self, camera_pos: Vec3) {
        let world = match &self.world {
            Some(w) => w,
            None => return,
        };

        let moved_far_enough = (camera_pos - world.last_streaming_origin).length_squared() > 1.0;
        let interval_elapsed = world.frames_since_streaming_update >= WORLD_STREAMING_INTERVAL_FRAMES;
        if !moved_far_enough && !interval_elapsed {
            if let Some(world) = &mut self.world {
                world.frames_since_streaming_update += 1;
            }
            return;
        }

        let load_distance_sq = world.world_file.streaming_config.load_distance * world.world_file.streaming_config.load_distance;
        let unload_distance_sq = world.world_file.streaming_config.unload_distance * world.world_file.streaming_config.unload_distance;

        let mut newly_queued_load = 0;
        let mut newly_queued_unload = 0;
        {
            let world = self.world.as_mut().unwrap();
            for i in 0..world.world_file.chunks.len() {
                let chunk = &world.world_file.chunks[i];
                let center = world.world_file.chunk_center_world(chunk);
                let center = Vec3::new(center[0], center[1], center[2]);
                let dist_sq = (center - camera_pos).length_squared();
                let state = &world.chunk_states[i];

                if !state.loaded && !state.queued && dist_sq <= load_distance_sq {
                    world.pending_load.push(i);
                    world.chunk_states[i].queued = true;
                    newly_queued_load += 1;
                } else if state.loaded && !state.queued && dist_sq > unload_distance_sq {
                    world.pending_unload.push(i);
                    world.chunk_states[i].queued = true;
                    newly_queued_unload += 1;
                }
            }
        }

        if let Some(world) = &mut self.world {
            world.last_streaming_origin = camera_pos;
            world.frames_since_streaming_update = 0;
        }

        if newly_queued_load > 0 || newly_queued_unload > 0 {
            println!(
                "[ENGINE] World streaming: +{} чанков поставлено в очередь загрузки, +{} в очередь выгрузки",
                newly_queued_load, newly_queued_unload
            );
        }
    }

    /// ДОБАВЛЕНО (фризы стриминга, см. `pending_load` у `WorldStreamingState`):
    /// вызывается КАЖДЫЙ кадр из `update()` (в отличие от
    /// `update_world_streaming`, которая пересчитывает окрестность лишь
    /// изредка).
    ///
    /// ИЗМЕНЕНО (фоновая загрузка чанков — см. подробное обоснование в
    /// шапке модуля): раньше эта функция САМА выполняла
    /// синхронную загрузку (диск + парсинг + GPU) не более
    /// `CHUNK_LOAD_BUDGET_PER_FRAME` чанков за кадр. Теперь три отдельных
    /// шага:
    ///  1. Забирает ГОТОВЫЕ результаты фоновой загрузки из канала
    ///     (`try_recv`, не блокируясь) и интегрирует их в GPU/Scene —
    ///     именно ЭТА часть (создание GPU-буферов/текстур) остаётся
    ///     потенциально заметной по времени, поэтому бюджет
    ///     (`CHUNK_LOAD_BUDGET_PER_FRAME`) применяется здесь.
    ///  2. Отправляет фоновому потоку запросы на ВСЕ чанки из
    ///     `pending_load` — сама отправка (`Sender::send`) не читает диск
    ///     и не блокирует, поэтому бюджет ей не нужен (диск/парсинг
    ///     происходят позже, уже в фоновом потоке, параллельно с кадрами).
    ///  3. Выгрузка — как и раньше, дешёвая (despawn без файлового I/O),
    ///     с тем же бюджетом, чтобы massed unload (например после
    ///     телепорта камеры далеко в сторону) не давал свой всплеск на
    ///     одном кадре.
    pub(super) fn drain_pending_chunk_io(&mut self) {
        let mut integrate_budget = CHUNK_LOAD_BUDGET_PER_FRAME;
        while integrate_budget > 0 {
            let Ok(result) = self.chunk_loader_rx.try_recv() else { break };

            let current_generation = self.world_generation;
            if result.generation != current_generation {
                continue;
            }

            self.integrate_loaded_chunk(result);
            integrate_budget -= 1;
        }

        loop {
            let next = match &mut self.world {
                Some(world) => world.pending_load.pop(),
                None => None,
            };
            let Some(chunk_idx) = next else { break };
            self.request_chunk_load(chunk_idx);
        }

        let mut unload_budget = CHUNK_LOAD_BUDGET_PER_FRAME;
        while unload_budget > 0 {
            let next = match &mut self.world {
                Some(world) => world.pending_unload.pop(),
                None => None,
            };
            let Some(chunk_idx) = next else { break };
            self.unload_chunk(chunk_idx);
            if let Some(world) = &mut self.world {
                world.chunk_states[chunk_idx].queued = false;
            }
            unload_budget -= 1;
        }
    }

    pub fn add_street_light(&mut self, x: f32, y: f32, z: f32) -> Option<u32> {
        let light = GPULight {
            position: [x, y, z, 0.0],
            color: [1.0, 0.85, 0.6, 2.5],
            direction: [0.0, -1.0, 0.0, 100.0],
            params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
        };
        self.lights.as_mut().map(|l| l.add_light(&light))
    }
}
