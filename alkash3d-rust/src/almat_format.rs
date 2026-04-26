// almat_format.rs - Material Acceleration System

use std::io::{Read, Write};

#[repr(C)]
pub struct AlmatHeader {
    pub magic: [u8; 8],           // "ALKALMAT"
    pub version: u32,
    pub total_materials: u32,
    pub material_buckets: u32,    // Группировка по типам (opaque, transparent, decal)
    pub string_table_offset: u64,
    pub material_table_offset: u64,
    pub texture_atlas_offset: u64,
    pub shader_cache_offset: u64, // Предкомпилированные варианты шейдеров
    pub created_at: u64,
}

#[repr(C)]
pub struct MaterialBucket {
    pub bucket_type: u32,         // 0=opaque, 1=alpha_test, 2=transparent, 3=decal
    pub material_start: u32,
    pub material_count: u32,
    pub sort_key: u32,            // Для сортировки при рендеринге
}

#[repr(C)]
pub struct AcceleratedMaterial {
    pub name_id: u32,
    pub shader_hash: u64,         // Хеш комбинации шейдеров для быстрого поиска
    pub texture_handles: [u64; 8], // Прямые GPU-дескрипторы текстур
    pub constant_buffer_data: [u32; 16], // Предзапечённые константы
    pub render_state_hash: u64,   // Хеш состояния рендера для батчинга
    pub batch_group: u32,         // Группа батчинга
    pub lod_material_id: u32,     // ID материала для LOD версий
    pub draw_calls_per_frame: u32, // Статистика использования
}

#[repr(C)]
pub struct TextureAtlasEntry {
    pub texture_name_id: u32,
    pub atlas_x: u16,
    pub atlas_y: u16,
    pub atlas_width: u16,
    pub atlas_height: u16,
    pub page_index: u16,          // Для больших атласов из нескольких страниц
}

pub struct AlmatFile {
    pub header: AlmatHeader,
    pub strings: Vec<String>,
    pub buckets: Vec<MaterialBucket>,
    pub materials: Vec<AcceleratedMaterial>,
    pub texture_atlas: Vec<TextureAtlasEntry>,
}

impl AlmatFile {
    pub fn new() -> Self {
        Self {
            header: AlmatHeader {
                magic: *b"ALKALMAT",
                version: 1,
                total_materials: 0,
                material_buckets: 4,
                string_table_offset: 0,
                material_table_offset: 0,
                texture_atlas_offset: 0,
                shader_cache_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            buckets: vec![
                MaterialBucket { bucket_type: 0, material_start: 0, material_count: 0, sort_key: 0 },
                MaterialBucket { bucket_type: 1, material_start: 0, material_count: 0, sort_key: 1 },
                MaterialBucket { bucket_type: 2, material_start: 0, material_count: 0, sort_key: 2 },
                MaterialBucket { bucket_type: 3, material_start: 0, material_count: 0, sort_key: 3 },
            ],
            materials: Vec::new(),
            texture_atlas: Vec::new(),
        }
    }

    pub fn create_optimized() -> Self {
        let mut mat = AlmatFile::new();

        // Настройка для максимального батчинга
        mat.materials.push(AcceleratedMaterial {
            name_id: 0,
            shader_hash: 0xDEADBEEF,
            texture_handles: [0; 8],
            constant_buffer_data: [0; 16],
            render_state_hash: 0,
            batch_group: 0,
            lod_material_id: 0xFFFFFFFF,
            draw_calls_per_frame: 0,
        });

        mat
    }
}