// alps_format.rs - Programmable Shader System

use std::io::{Read, Write, Seek, SeekFrom};
use std::collections::HashMap;

#[repr(C)]
pub struct AlpsHeader {
    pub magic: [u8; 8],           // "ALKALPS "
    pub version: u32,
    pub technique_count: u32,
    pub permutation_count: u32,   // Всего возможных вариантов
    pub string_table_offset: u64,
    pub technique_table_offset: u64,
    pub permutation_table_offset: u64,
    pub bytecode_offset: u64,
    pub metadata_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
pub struct ShaderTechnique {
    pub name_id: u32,
    pub vertex_shader_id: u32,
    pub pixel_shader_id: u32,
    pub geometry_shader_id: u32,
    pub hull_shader_id: u32,
    pub domain_shader_id: u32,
    pub compute_shader_id: u32,
    pub permutation_start: u32,
    pub permutation_count: u32,
    pub default_permutation: u32,
    pub flags: u32,               // Поддержка тесселяции, рейтрейсинга
}

#[repr(C)]
pub struct ShaderPermutation {
    pub technique_id: u32,
    pub define_hash: u64,         // Хеш комбинации дефайнов
    pub vs_bytecode_offset: u64,
    pub ps_bytecode_offset: u64,
    pub vertex_stride: u32,
    pub input_layout_hash: u64,
    pub pso_cache_key: u64,       // Ключ для кеширования PSO
    pub compile_time_ms: u32,     // Время компиляции для профилирования
    pub rating: u16,              // Оценка производительности (1-100)
    pub fallback_permutation: u32, // На случай отсутствия поддержки
}

#[repr(C)]
pub struct ShaderMetadata {
    pub author_id: u32,
    pub description_id: u32,
    pub version_major: u16,
    pub version_minor: u16,
    pub min_feature_level: u32,   // D3D_FEATURE_LEVEL
    pub required_extensions: [u32; 8], // Битовые флаги расширений
    pub estimated_instruction_count: u32,
    pub register_pressure: u32,   // Давление на регистры
    pub memory_usage_bytes: u32,
    pub compile_date: u64,
}

pub struct AlpsFile {
    pub header: AlpsHeader,
    pub strings: Vec<String>,
    pub techniques: Vec<ShaderTechnique>,
    pub permutations: Vec<ShaderPermutation>,
    pub bytecode: Vec<u8>,
    pub metadata: Vec<ShaderMetadata>,
}

#[repr(u32)]
pub enum ShaderFeature {
    Standard = 0,
    Instanced = 1,
    Skinned = 2,
    Tessellated = 3,
    Raytraced = 4,
    ComputeDriven = 5,
    Terrain = 6,
    Water = 7,
    ParticleSystem = 8,
    PostProcess = 9,
    CustomUser0 = 1000,
}

impl AlpsFile {
    pub fn new() -> Self {
        Self {
            header: AlpsHeader {
                magic: *b"ALKALPS ",
                version: 1,
                technique_count: 0,
                permutation_count: 0,
                string_table_offset: 0,
                technique_table_offset: 0,
                permutation_table_offset: 0,
                bytecode_offset: 0,
                metadata_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            techniques: Vec::new(),
            permutations: Vec::new(),
            bytecode: Vec::new(),
            metadata: Vec::new(),
        }
    }

    pub fn create_standard_library() -> Self {
        let mut alps = AlpsFile::new();

        // Добавляем технику PBR (Physically Based Rendering)
        let tech_name_id = alps.add_string("PBR_Standard");
        let vs_name_id = alps.add_string("PBR_VS_Main");
        let ps_name_id = alps.add_string("PBR_PS_Main");

        alps.techniques.push(ShaderTechnique {
            name_id: tech_name_id,
            vertex_shader_id: vs_name_id,
            pixel_shader_id: ps_name_id,
            geometry_shader_id: 0xFFFFFFFF,
            hull_shader_id: 0xFFFFFFFF,
            domain_shader_id: 0xFFFFFFFF,
            compute_shader_id: 0xFFFFFFFF,
            permutation_start: 0,
            permutation_count: 8,  // Базовые перестановки
            default_permutation: 0,
            flags: 0x01,           // Поддержка рейтрейсинга
        });

        alps.header.technique_count = 1;
        alps
    }

    fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }
}