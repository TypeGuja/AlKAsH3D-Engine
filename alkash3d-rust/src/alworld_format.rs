// alworld_format.rs - World Streaming & Map System

use std::io::{Read, Write};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AlworldHeader {
    pub magic: [u8; 8],           // "ALKWORLD"
    pub version: u32,
    pub flags: u32,               // Битовая маска: стриминг, LOD, коллизии
    pub chunk_size: f32,          // Размер чанка в метрах (по умолчанию 64.0)
    pub world_bounds_min: [f32; 3],
    pub world_bounds_max: [f32; 3],
    pub total_chunks: u32,
    pub active_chunks: u32,       // Максимум одновременно загруженных чанков
    pub chunk_table_offset: u64,
    pub string_table_offset: u64,
    pub global_objects_offset: u64, // Объекты, видимые из любой точки (небо, горы)
    pub streaming_config_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ChunkDescriptor {
    pub grid_x: i32,
    pub grid_y: i32,              // Для открытого мира; для подземелий - Z
    pub grid_z: i32,
    // ДОБАВЛЕНО (World Streaming — подключение к движку): раньше `state`
    // было частью сериализованного дескриптора, но состояние загрузки —
    // ЧИСТО рантайм-величина (зависит от текущей позиции камеры В ЭТОМ
    // запуске движка, а не от содержимого файла) — хранить её на диске
    // means каждое сохранение .alworld фиксировало бы случайный снимок
    // "что было загружено в момент save()", что бессмысленно для файла,
    // который открывают заново с чистого состояния. Оставлено полем ради
    // обратной совместимости layout (см. save()/load() ниже — сериализуется
    // как есть, но всегда сбрасывается в 0/unloaded при `load()`), реальное
    // состояние стриминга живёт в `AlkashEngine`-стороне (см. `ChunkRuntimeState`
    // в engine/mod.rs), не здесь.
    pub state: u32,               // 0=unloaded, 1=loading, 2=loaded, 3=unloading
    pub priority: f32,            // Динамический приоритет для стриминга
    pub data_offset: u64,         // Смещение в файле или внешний файл
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub objects_count: u32,
    pub lights_count: u32,
    pub occlusion_mesh_offset: u64, // Упрощённая геометрия для окклюзии
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StreamingConfig {
    pub load_distance: f32,        // Дистанция загрузки (200.0)
    pub unload_distance: f32,      // Дистанция выгрузки (250.0)
    pub high_priority_distance: f32, // Зона высокого приоритета (50.0)
    pub max_concurrent_loads: u32, // Максимум одновременных загрузок (3)
    pub load_timeout_ms: u32,      // Таймаут загрузки (5000)
    pub preload_budget_mb: u32,    // Бюджет памяти на предзагрузку (512)
    pub streaming_threads: u32,    // Количество потоков стриминга (2)
    pub use_async_io: u32,
    pub compression_type: u32,     // 0=нет, 1=zstd, 2=lz4
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GlobalObject {
    pub name_id: u32,
    pub altex_file_id: u32,        // ID в строковой таблице
    pub transform: [f32; 16],      // 4x4 матрица
    pub lod_distances: [f32; 4],   // Дистанции для LOD
    pub flags: u32,
}

pub struct AlworldFile {
    pub header: AlworldHeader,
    pub strings: Vec<String>,
    pub chunks: Vec<ChunkDescriptor>,
    pub streaming_config: StreamingConfig,
    pub global_objects: Vec<GlobalObject>,
}

impl AlworldFile {
    pub fn new(world_size_km: f32) -> Self {
        let chunk_size = 64.0;
        let half_world = world_size_km * 500.0; // В метрах
        let chunks_per_axis = ((world_size_km * 1000.0) / chunk_size).ceil() as u32;

        Self {
            header: AlworldHeader {
                magic: *b"ALKWORLD",
                version: 1,
                flags: 0x01 | 0x02, // Стриминг + LOD
                chunk_size,
                world_bounds_min: [-half_world, -100.0, -half_world],
                world_bounds_max: [half_world, 500.0, half_world],
                total_chunks: chunks_per_axis * chunks_per_axis,
                active_chunks: 64,
                chunk_table_offset: 0,
                string_table_offset: 0,
                global_objects_offset: 0,
                streaming_config_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            chunks: Vec::with_capacity(1024),
            streaming_config: StreamingConfig {
                load_distance: 200.0,
                unload_distance: 250.0,
                high_priority_distance: 50.0,
                max_concurrent_loads: 3,
                load_timeout_ms: 5000,
                preload_budget_mb: 512,
                streaming_threads: 2,
                use_async_io: 1,
                compression_type: 1, // zstd
            },
            global_objects: Vec::new(),
        }
    }

    pub fn create_open_world_demo() -> Self {
        let mut world = AlworldFile::new(4.0); // 4x4 км мир

        // Добавляем тестовые чанки
        for x in -16..16 {
            for z in -16..16 {
                world.chunks.push(ChunkDescriptor {
                    grid_x: x,
                    grid_y: 0,
                    grid_z: z,
                    state: 0,
                    priority: 0.0,
                    data_offset: 0,
                    compressed_size: 0,
                    uncompressed_size: 0,
                    objects_count: 0,
                    lights_count: 0,
                    occlusion_mesh_offset: 0,
                });
            }
        }

        world
    }

    /// ДОБАВЛЕНО (World Streaming — подключение к движку): создаёт
    /// небольшой демонстрационный мир (9x9 чанков по 64м = ~576x576м) И
    /// СРАЗУ сохраняет его на диск вместе с РЕАЛЬНЫМ содержимым нескольких
    /// центральных чанков (по одному объекту-"зданию" в каждом, размещённому
    /// в центре чанка) — в отличие от `create_open_world_demo` (которая
    /// только заполняет `self.chunks` дескрипторами в памяти, но ничего не
    /// пишет на диск и не создаёт ни одного файла содержимого чанка),
    /// эта функция даёт полностью рабочий, готовый к `AlkashEngine::load_world`
    /// пример на диске — удобно для первой проверки стриминга без
    /// необходимости вручную готовить .alworld + .alwchunk файлы.
    ///
    /// `dir` — папка, куда будет сохранён `world.alworld` и подпапка
    /// `chunks/` с файлами содержимого. Возвращает путь к созданному
    /// `.alworld` файлу.
    pub fn create_and_save_demo_world(dir: &str) -> std::io::Result<String> {
        let mut world = AlworldFile::new(0.6); // ~600x600м — компактный демо-мир
        world.chunks.clear(); // new() не создаёт чанков сама (в отличие от create_open_world_demo)

        std::fs::create_dir_all(dir)?;
        let chunks_dir = std::path::Path::new(dir).join("chunks");
        std::fs::create_dir_all(&chunks_dir)?;

        // 9x9 чанков вокруг начала координат — достаточно, чтобы
        // продемонстрировать и загрузку (центральные чанки рядом со
        // стартовой позицией камеры), и выгрузку (дальние чанки на краю
        // сетки), не создавая тысячи файлов для простого примера.
        for x in -4..=4 {
            for z in -4..=4 {
                world.chunks.push(ChunkDescriptor {
                    grid_x: x,
                    grid_y: 0,
                    grid_z: z,
                    state: 0,
                    priority: 0.0,
                    data_offset: 0,
                    compressed_size: 0,
                    uncompressed_size: 0,
                    objects_count: 1,
                    lights_count: 0,
                    occlusion_mesh_offset: 0,
                });

                // Один объект-плейсхолдер в центре каждого чанка — путь
                // "placeholder" не указывает на реальный существующий
                // .altex (загрузчик .altex -> Mesh ещё не реализован, см.
                // подробное объяснение у `AlkashEngine::load_chunk` в
                // engine/mod.rs) — движок использует его только как
                // непрозрачный идентификатор объекта, реальная geometry
                // подставляется fallback'ом на стороне движка.
                let cs = world.header.chunk_size;
                let center_x = (x as f32 + 0.5) * cs;
                let center_z = (z as f32 + 0.5) * cs;
                let mut transform = [0.0f32; 16];
                // Единичная 4x4 row-major матрица со смещением в
                // последней строке (индексы 12..15) — та же конвенция,
                // что `AlkashEngine::load_chunk` ожидает при разборе
                // ChunkObjectHeader::transform.
                transform[0] = 1.0;
                transform[5] = 1.0;
                transform[10] = 1.0;
                transform[15] = 1.0;
                transform[12] = center_x;
                transform[13] = 0.0;
                transform[14] = center_z;

                let mut content = ChunkContent::new();
                // ДОБАВЛЕНО (объединённая сцена — физика из .alworld):
                // ровно ОДИН чанк демо-мира (самый центральный, grid (0,0)
                // — гарантированно попадает в стартовую окрестность
                // загрузки камеры, см. `update_world_streaming`) получает
                // физический объект вместо обычного, приподнятый над
                // землёй (y=5.0) — при запуске он сразу начинает падать
                // под действием гравитации и наглядно демонстрирует, что
                // `.alworld` теперь сам создаёт физические тела через
                // `CHUNK_OBJECT_FLAG_HAS_PHYSICS`, без ручного вызова
                // `spawn_physics_sphere` в коде main.rs. Остальные 80
                // чанков демо-мира остаются как раньше — обычными
                // (`add_object`, без физики), чтобы не грузить движок
                // физикой ради чисто визуальной демонстрации стриминга.
                if x == 0 && z == 0 {
                    let mut physics_transform = transform;
                    physics_transform[13] = 5.0; // приподнят на 5м над "полом" чанка
                    content.add_object_with_physics("placeholder", physics_transform, 1.0);
                } else {
                    content.add_object("placeholder", transform);
                }
                let chunk_path = chunks_dir.join(format!("chunk_{}_{}_{}.alwchunk", x, 0, z));
                content.save_to_file(chunk_path.to_string_lossy().as_ref())?;
            }
        }

        let alworld_path = std::path::Path::new(dir).join("world.alworld");
        world.save(alworld_path.to_string_lossy().as_ref())?;

        Ok(alworld_path.to_string_lossy().into_owned())
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        // ДОБАВЛЕНО (World Streaming): переиспользуем уже добавленную
        // строку, если она уже есть — тот же приём, что в других форматах
        // (.alfar/.altex/.alcar), не даёт дублировать одинаковые пути к
        // .altex файлам чанков в строковой таблице.
        if let Some(pos) = self.strings.iter().position(|existing| existing == s) {
            return pos as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }

    pub fn get_string(&self, id: u32) -> &str {
        self.strings.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }

    /// Находит индекс (в `self.chunks`) дескриптора чанка по его сеточным
    /// координатам, если такой чанк существует в мире.
    pub fn find_chunk(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> Option<usize> {
        self.chunks.iter().position(|c| c.grid_x == grid_x && c.grid_y == grid_y && c.grid_z == grid_z)
    }

    /// Мировые координаты ЦЕНТРА чанка с заданными сеточными координатами
    /// — используется стриминг-логикой движка (`AlkashEngine::update_world_streaming`
    /// в engine/mod.rs) для вычисления дистанции от камеры до чанка.
    pub fn chunk_center_world(&self, chunk: &ChunkDescriptor) -> [f32; 3] {
        let cs = self.header.chunk_size;
        [
            (chunk.grid_x as f32 + 0.5) * cs,
            (chunk.grid_y as f32 + 0.5) * cs,
            (chunk.grid_z as f32 + 0.5) * cs,
        ]
    }

    // =====================================================================
    // ДОБАВЛЕНО (World Streaming — подключение к движку): save()/load() для
    // САМОГО .alworld (заголовок мира, таблица чанков-дескрипторов,
    // строки, streaming config, global objects). Содержимое КАЖДОГО чанка
    // (реальные объекты внутри него) сериализуется ОТДЕЛЬНО через
    // `ChunkContent::save_to_file`/`load_from_file` в СВОИ файлы (см.
    // подробное объяснение у `ChunkContent` ниже) — .alworld описывает
    // структуру и метаданные мира ("где какие чанки, какого они
    // размера"), а не хранит внутри себя тысячи объектов каждого чанка
    // целиком, что при большом открытом мире сделало бы сам .alworld
    // файлом на сотни мегабайт, читаемым целиком даже если игрок стоит в
    // одной точке и видит только несколько ближайших чанков — весь смысл
    // стриминга в том, чтобы читать с диска ТОЛЬКО то, что реально нужно
    // прямо сейчас.
    //
    // Формат совпадает по духу с `.alfar`/`.altex`: header -> string table
    // (count + null-terminated строки) -> streaming_config -> chunks
    // (count + POD-массив ChunkDescriptor) -> global_objects (count +
    // POD-массив GlobalObject).
    // =====================================================================
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(&(s.len() as u32).to_le_bytes());
            strings_data.extend_from_slice(s.as_bytes());
        }

        let streaming_data = unsafe {
            std::slice::from_raw_parts(
                &self.streaming_config as *const StreamingConfig as *const u8,
                std::mem::size_of::<StreamingConfig>(),
            )
        };

        let mut chunks_data = Vec::new();
        chunks_data.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        for chunk in &self.chunks {
            chunks_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(chunk as *const ChunkDescriptor as *const u8, std::mem::size_of::<ChunkDescriptor>())
            });
        }

        let mut globals_data = Vec::new();
        globals_data.extend_from_slice(&(self.global_objects.len() as u32).to_le_bytes());
        for obj in &self.global_objects {
            globals_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(obj as *const GlobalObject as *const u8, std::mem::size_of::<GlobalObject>())
            });
        }

        let header_size = std::mem::size_of::<AlworldHeader>() as u64;
        let string_table_offset = header_size;
        let streaming_config_offset = string_table_offset + strings_data.len() as u64;
        let chunk_table_offset = streaming_config_offset + streaming_data.len() as u64;
        let global_objects_offset = chunk_table_offset + chunks_data.len() as u64;

        let header = AlworldHeader {
            magic: self.header.magic,
            version: self.header.version,
            flags: self.header.flags,
            chunk_size: self.header.chunk_size,
            world_bounds_min: self.header.world_bounds_min,
            world_bounds_max: self.header.world_bounds_max,
            total_chunks: self.chunks.len() as u32,
            active_chunks: self.header.active_chunks,
            chunk_table_offset,
            string_table_offset,
            global_objects_offset,
            streaming_config_offset,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlworldHeader as *const u8, std::mem::size_of::<AlworldHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(streaming_data)?;
        file.write_all(&chunks_data)?;
        file.write_all(&globals_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let header_size = std::mem::size_of::<AlworldHeader>();
        if buf.len() < header_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alworld: файл короче заголовка AlworldHeader",
            ));
        }

        // SAFETY: AlworldHeader — #[repr(C)], POD (только числа и [u8;8]/
        // [f32;3]), длина буфера уже проверена выше.
        let header: AlworldHeader = unsafe {
            std::ptr::read_unaligned(buf.as_ptr() as *const AlworldHeader)
        };

        if &header.magic != b"ALKWORLD" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alworld: неверная сигнатура {:?}, ожидалось ALKWORLD", header.magic),
            ));
        }
        if header.version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alworld: неподдерживаемая версия формата {}", header.version),
            ));
        }

        let read_at = |offset: u64, size: usize, what: &str| -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.checked_add(size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alworld: переполнение при вычислении конца блока {}", what),
            ))?;
            buf.get(start..end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("alworld: блок {} выходит за пределы файла (offset={}, size={}, file_len={})", what, offset, size, buf.len()),
            ))
        };

        // Строковая таблица: count(u32) + N раз [len(u32) + байты строки
        // БЕЗ null-терминатора] — отличается от .alfar (там строки
        // null-terminated) сознательно: пути к .altex файлам чанков могут
        // быть произвольными ОС-путями, где null-терминация менее
        // естественна, чем явная длина, и явная длина исключает проблему
        // "а что если в пути встретится байт 0" (на практике невозможно на
        // большинстве ФС, но явная длина не полагается на это допущение).
        let strings_start = header.string_table_offset as usize;
        if strings_start > buf.len() || strings_start + 4 > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alworld: string_table_offset выходит за пределы файла",
            ));
        }
        let string_count = u32::from_le_bytes(buf[strings_start..strings_start + 4].try_into().unwrap()) as usize;
        let mut cursor = strings_start + 4;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            let len_bytes = read_at(cursor as u64, 4, "string length")?;
            let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            cursor += 4;
            let str_bytes = read_at(cursor as u64, len, "string data")?;
            let s = String::from_utf8_lossy(str_bytes).into_owned();
            strings.push(s);
            cursor += len;
        }

        let streaming_bytes = read_at(header.streaming_config_offset, std::mem::size_of::<StreamingConfig>(), "streaming_config")?;
        let streaming_config: StreamingConfig = unsafe { std::ptr::read_unaligned(streaming_bytes.as_ptr() as *const StreamingConfig) };

        let chunks_count_start = header.chunk_table_offset as usize;
        if chunks_count_start + 4 > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alworld: chunk_table_offset выходит за пределы файла",
            ));
        }
        let chunks_count = u32::from_le_bytes(buf[chunks_count_start..chunks_count_start + 4].try_into().unwrap()) as usize;
        let chunk_desc_size = std::mem::size_of::<ChunkDescriptor>();
        let chunks_bytes = read_at((chunks_count_start + 4) as u64, chunks_count * chunk_desc_size, "chunks table")?;
        let mut chunks = Vec::with_capacity(chunks_count);
        for i in 0..chunks_count {
            let start = i * chunk_desc_size;
            let mut chunk: ChunkDescriptor = unsafe {
                std::ptr::read_unaligned(chunks_bytes[start..start + chunk_desc_size].as_ptr() as *const ChunkDescriptor)
            };
            // Состояние загрузки — чисто рантайм-величина (см. комментарий
            // у поля `state` выше) — всегда стартуем с "unloaded", даже
            // если файл почему-то был сохранён с другим значением.
            chunk.state = 0;
            chunks.push(chunk);
        }

        let globals_count_start = header.global_objects_offset as usize;
        let mut global_objects = Vec::new();
        if globals_count_start + 4 <= buf.len() {
            let globals_count = u32::from_le_bytes(buf[globals_count_start..globals_count_start + 4].try_into().unwrap()) as usize;
            let global_obj_size = std::mem::size_of::<GlobalObject>();
            let globals_bytes = read_at((globals_count_start + 4) as u64, globals_count * global_obj_size, "global objects")?;
            for i in 0..globals_count {
                let start = i * global_obj_size;
                let obj: GlobalObject = unsafe {
                    std::ptr::read_unaligned(globals_bytes[start..start + global_obj_size].as_ptr() as *const GlobalObject)
                };
                global_objects.push(obj);
            }
        }

        Ok(Self {
            header,
            strings,
            chunks,
            streaming_config,
            global_objects,
        })
    }
}

// =========================================================================
// ДОБАВЛЕНО (World Streaming — подключение к движку): содержимое ОДНОГО
// чанка — список объектов внутри него. Сознательно ОТДЕЛЬНЫЙ от
// `AlworldFile` формат/файл (см. подробное объяснение у save()/load()
// AlworldFile выше, почему): `AlworldFile` читается ЦЕЛИКОМ один раз при
// запуске мира (это лишь метаданные — где чанки, какого размера), а
// `ChunkContent` каждого отдельного чанка читается/выгружается ИЗ ДИСКА
// по требованию, когда камера приближается/удаляется — именно это и есть
// собственно "стриминг".
// =========================================================================

/// Один объект внутри чанка — ссылка на geometry-файл (.altex) с
/// трансформацией размещения в мире. Сознательно НЕ хранит саму
/// геометрию — `.altex` уже полноценный формат со своим набором
/// вершин/мешей (см. `altex_format.rs`), дублировать его здесь означало
/// бы либо повторно сериализовать геометрию на каждый чанк, где она
/// встречается (расточительно — один и тот же меш фонарного столба
/// используется в сотнях чанков), либо разделять geometry-данные между
/// чанками сложным механизмом — путь к файлу проще и уже используется
/// как паттерн ссылки в этом движке (см. `AlcarFile::set_mesh`).
/// ДОБАВЛЕНО (объединённая сцена — физика из .alworld): бит `flags`,
/// означающий "у этого объекта чанка есть физическое тело" — движок
/// (`AlkashEngine::load_chunk`) создаёт физическое тело через
/// `add_physics_body` ТОЛЬКО для объектов с этим битом, а не для всех
/// подряд: подавляющее большинство объектов открытого мира (деревья,
/// декор, дальний фон) не должны участвовать в физическом моделировании
/// вообще — лишние тела в broad/narrow phase плагина Inertial стоят
/// CPU-времени на каждый кадр без какой-либо пользы для объекта, который
/// и так никогда не двигается и ни с чем не должен физически
/// взаимодействовать.
pub const CHUNK_OBJECT_FLAG_HAS_PHYSICS: u32 = 0x01;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ChunkObjectHeader {
    pub altex_path_string_id: u32,
    pub transform: [f32; 16], // 4x4 матрица размещения объекта в мировых координатах
    pub flags: u32,
    // ИЗМЕНЕНО (объединённая сцена — физика из .alworld): было
    // `_padding: u32`, неиспользуемое поле-заполнитель. Раскладка
    // структуры (размер, офсеты) не меняется — это то же самое 4-байтное
    // поле, теперь несущее реальные данные вместо нулей. Валидно ТОЛЬКО
    // когда установлен `CHUNK_OBJECT_FLAG_HAS_PHYSICS` (см. `add_object`,
    // которая пишет туда 0.0 — та же семантика "нет физики", что была у
    // старого _padding=0). Масса — не булев признак "есть физика или
    // нет" (для этого есть сам флаг), потому что 0.0 — валидное
    // физическое значение "статическое тело" (см. `add_sphere_body` в
    // engine/mod.rs: `is_static = mass <= 0.0`) — статическое тело ВСЁ
    // РАВНО участвует в коллизиях (например, стена, о которую должны
    // спотыкаться другие тела), просто не двигается само, поэтому это
    // отдельный от "нет физики вообще" случай.
    pub mass: f32,
}

pub struct ChunkContent {
    pub strings: Vec<String>,
    pub objects: Vec<ChunkObjectHeader>,
}

impl ChunkContent {
    pub fn new() -> Self {
        Self { strings: Vec::new(), objects: Vec::new() }
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.strings.iter().position(|existing| existing == s) {
            return pos as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }

    pub fn get_string(&self, id: u32) -> &str {
        self.strings.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }

    /// Добавляет объект (путь к .altex + трансформация) в чанк БЕЗ
    /// физики — сохраняет прежнее поведение для всего существующего кода,
    /// который уже вызывает `add_object` (декор, статичная геометрия без
    /// коллизий). Возвращает индекс объекта в `self.objects`.
    pub fn add_object(&mut self, altex_path: &str, transform: [f32; 16]) -> usize {
        let path_id = self.add_string(altex_path);
        self.objects.push(ChunkObjectHeader {
            altex_path_string_id: path_id,
            transform,
            flags: 0,
            mass: 0.0,
        });
        self.objects.len() - 1
    }

    /// ДОБАВЛЕНО (объединённая сцена — физика из .alworld): та же
    /// логика, что `add_object`, но помечает объект флагом
    /// `CHUNK_OBJECT_FLAG_HAS_PHYSICS` и сохраняет `mass` — при загрузке
    /// чанка (`AlkashEngine::load_chunk`) движок создаст для этого
    /// объекта физическое тело-сферу (текущая единственная форма
    /// коллайдера, поддерживаемая плагином Inertial на сегодня — см.
    /// `add_sphere_body`) в дополнение к визуальному мешу, без ручной
    /// привязки в коде каждого конкретного объекта. `mass <= 0.0` создаёт
    /// СТАТИЧЕСКОЕ тело (не двигается, но участвует в коллизиях — та же
    /// семантика, что и у прямого вызова `add_sphere_body`).
    pub fn add_object_with_physics(&mut self, altex_path: &str, transform: [f32; 16], mass: f32) -> usize {
        let path_id = self.add_string(altex_path);
        self.objects.push(ChunkObjectHeader {
            altex_path_string_id: path_id,
            transform,
            flags: CHUNK_OBJECT_FLAG_HAS_PHYSICS,
            mass,
        });
        self.objects.len() - 1
    }

    /// Формат файла: magic "ALKCHNK" + version(u32) + string table (count +
    /// [len+bytes]) + objects (count + POD-массив ChunkObjectHeader) —
    /// тот же общий стиль, что у `AlworldFile::save`/`load`, минимальный и
    /// самодостаточный (не имеет заголовка со смещениями, т.к. блоки
    /// всего два и идут строго последовательно — чанк-файлы по
    /// определению маленькие и многочисленные, экономия на сложности
    /// важнее гибкости произвольного порядка блоков).
    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        file.write_all(b"ALKCHNK\0")?;
        file.write_all(&1u32.to_le_bytes())?;

        file.write_all(&(self.strings.len() as u32).to_le_bytes())?;
        for s in &self.strings {
            file.write_all(&(s.len() as u32).to_le_bytes())?;
            file.write_all(s.as_bytes())?;
        }

        file.write_all(&(self.objects.len() as u32).to_le_bytes())?;
        for obj in &self.objects {
            file.write_all(unsafe {
                std::slice::from_raw_parts(obj as *const ChunkObjectHeader as *const u8, std::mem::size_of::<ChunkObjectHeader>())
            })?;
        }

        Ok(())
    }

    pub fn load_from_file(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        if buf.len() < 12 || &buf[0..8] != b"ALKCHNK\0" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "alworld chunk: неверная сигнатура, ожидалось ALKCHNK",
            ));
        }
        let version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        if version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alworld chunk: неподдерживаемая версия формата {}", version),
            ));
        }

        let read_at = |buf: &[u8], offset: usize, size: usize, what: &str| -> std::io::Result<std::ops::Range<usize>> {
            let end = offset.checked_add(size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alworld chunk: переполнение при вычислении конца блока {}", what),
            ))?;
            if end > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("alworld chunk: блок {} выходит за пределы файла (offset={}, size={}, file_len={})", what, offset, size, buf.len()),
                ));
            }
            Ok(offset..end)
        };

        let mut cursor = 12usize;
        let range = read_at(&buf, cursor, 4, "string count")?;
        let string_count = u32::from_le_bytes(buf[range].try_into().unwrap()) as usize;
        cursor += 4;

        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            let range = read_at(&buf, cursor, 4, "string length")?;
            let len = u32::from_le_bytes(buf[range].try_into().unwrap()) as usize;
            cursor += 4;
            let range = read_at(&buf, cursor, len, "string data")?;
            strings.push(String::from_utf8_lossy(&buf[range]).into_owned());
            cursor += len;
        }

        let range = read_at(&buf, cursor, 4, "object count")?;
        let object_count = u32::from_le_bytes(buf[range].try_into().unwrap()) as usize;
        cursor += 4;

        let obj_size = std::mem::size_of::<ChunkObjectHeader>();
        let range = read_at(&buf, cursor, object_count * obj_size, "objects")?;
        let objects_bytes = &buf[range];
        let mut objects = Vec::with_capacity(object_count);
        for i in 0..object_count {
            let start = i * obj_size;
            let obj: ChunkObjectHeader = unsafe {
                std::ptr::read_unaligned(objects_bytes[start..start + obj_size].as_ptr() as *const ChunkObjectHeader)
            };
            objects.push(obj);
        }

        Ok(Self { strings, objects })
    }
}

impl Default for ChunkContent {
    fn default() -> Self {
        Self::new()
    }
}