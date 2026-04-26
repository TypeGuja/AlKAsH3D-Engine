// alworld_format.rs - World Streaming & Map System

use std::io::{Read, Write, Seek, SeekFrom};
use std::collections::HashMap;

#[repr(C)]
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
pub struct ChunkDescriptor {
    pub grid_x: i32,
    pub grid_y: i32,              // Для открытого мира; для подземелий - Z
    pub grid_z: i32,
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
}