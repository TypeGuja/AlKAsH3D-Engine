// alsnd_format.rs - Spatial Sound System

use std::io::{Read, Write, Seek, SeekFrom};

#[repr(C)]
pub struct AlsndHeader {
    pub magic: [u8; 8],           // "ALKALSND"
    pub version: u32,
    pub audio_engine: u32,        // 0=XAudio2, 1=WASAPI, 2=OpenAL, 3=Custom
    pub channels: u32,            // 2, 5.1, 7.1
    pub sample_rate: u32,         // 44100, 48000, 96000
    pub bits_per_sample: u32,     // 16, 24, 32
    pub sound_count: u32,
    pub sound_bank_count: u32,
    pub max_concurrent_sounds: u32, // 128
    pub string_table_offset: u64,
    pub sound_table_offset: u64,
    pub bank_table_offset: u64,
    pub preset_table_offset: u64,
    pub reverb_zones_offset: u64,
    pub occlusion_data_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
pub struct SoundDescriptor {
    pub name_id: u32,
    pub format: u32,              // 0=WAV, 1=OGG, 2=MP3, 3=FLAC, 4=OPUS
    pub category: u32,            // 0=SFX, 1=Music, 2=Ambient, 3=Voice, 4=UI
    pub data_offset: u64,
    pub size_compressed: u64,
    pub size_uncompressed: u64,
    pub duration_ms: u32,
    pub loop_start_ms: u32,
    pub loop_end_ms: u32,
    pub default_volume: f32,
    pub default_pitch: f32,
    pub priority: u32,            // 0 (низкий) - 255 (критический)
    pub max_instances: u32,       // Максимум одновременных проигрываний
    pub spatial_blend: f32,       // 0.0 (2D) - 1.0 (полностью 3D)
}

#[repr(C)]
pub struct SoundBank {
    pub name_id: u32,
    pub sounds_start: u32,
    pub sounds_count: u32,
    pub preload_all: u32,
    pub keep_in_memory: u32,
    pub memory_budget_mb: u32,
}

#[repr(C)]
pub struct SoundPreset {
    pub name_id: u32,
    pub sounds: [u32; 8],         // До 8 звуков на пресет
    pub weights: [f32; 8],        // Веса для случайного выбора
    pub sound_count: u32,
    pub randomize_pitch: [f32; 2], // min, max
    pub randomize_volume: [f32; 2],
    pub attenuation_model: u32,   // 0=linear, 1=log, 2=custom
    pub min_distance: f32,
    pub max_distance: f32,
    pub doppler_factor: f32,
    pub cone_inner_angle: f32,
    pub cone_outer_angle: f32,
    pub cone_outer_gain: f32,
}

#[repr(C)]
pub struct ReverbZone {
    pub position: [f32; 3],
    pub radius: f32,
    pub room_size: f32,
    pub damping: f32,
    pub wet_level: f32,
    pub dry_level: f32,
    pub width: f32,
    pub preset: u32,              // 0=generic, 1=hall, 2=room, 3=chamber, 4=custom
}

#[repr(C)]
pub struct AudioOcclusion {
    pub source_position: [f32; 3],
    pub listener_position: [f32; 3],
    pub direct_occlusion: f32,    // 0.0 (полностью перекрыто) - 1.0 (открыто)
    pub reverb_occlusion: f32,
    pub material_id: u32,         // Материал препятствия для фильтрации
    pub frequency_attenuation: [f32; 8], // Аттенюация по октавам
}

pub struct AlsndFile {
    pub header: AlsndHeader,
    pub strings: Vec<String>,
    pub sounds: Vec<SoundDescriptor>,
    pub banks: Vec<SoundBank>,
    pub presets: Vec<SoundPreset>,
    pub reverb_zones: Vec<ReverbZone>,
}

impl AlsndFile {
    pub fn new(channels: u32, sample_rate: u32) -> Self {
        Self {
            header: AlsndHeader {
                magic: *b"ALKALSND",
                version: 1,
                audio_engine: 0,      // XAudio2 по умолчанию
                channels,
                sample_rate,
                bits_per_sample: 16,
                sound_count: 0,
                sound_bank_count: 0,
                max_concurrent_sounds: 128,
                string_table_offset: 0,
                sound_table_offset: 0,
                bank_table_offset: 0,
                preset_table_offset: 0,
                reverb_zones_offset: 0,
                occlusion_data_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            sounds: Vec::new(),
            banks: Vec::new(),
            presets: Vec::new(),
            reverb_zones: Vec::new(),
        }
    }

    pub fn create_city_ambient() -> Self {
        let mut snd = AlsndFile::new(2, 48000);

        // Сначала получаем ID строки
        let name_id = snd.add_string("City_Ambient_Day");

        // Потом используем его
        snd.presets.push(SoundPreset {
            name_id,
            sounds: [0, 1, 2, 3, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF],
            weights: [0.3, 0.3, 0.2, 0.2, 0.0, 0.0, 0.0, 0.0],
            sound_count: 4,
            randomize_pitch: [0.95, 1.05],
            randomize_volume: [0.8, 1.0],
            attenuation_model: 1,
            min_distance: 10.0,
            max_distance: 200.0,
            doppler_factor: 0.5,
            cone_inner_angle: 360.0,
            cone_outer_angle: 360.0,
            cone_outer_gain: 0.0,
        });

        snd
    }

    fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }
}