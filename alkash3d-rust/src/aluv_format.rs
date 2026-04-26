// aluv_format.rs - Cinematic Sequences (Ultimate Visuals)

use std::io::{Read, Write, Seek, SeekFrom};

#[repr(C)]
pub struct AluvHeader {
    pub magic: [u8; 8],          // "ALKALUV "
    pub version: u32,
    pub sequence_count: u32,
    pub total_duration_ms: u64,   // Общая длительность всех катсцен
    pub string_table_offset: u64,
    pub sequence_table_offset: u64,
    pub track_table_offset: u64,
    pub keyframe_data_offset: u64,
    pub camera_paths_offset: u64,
    pub event_triggers_offset: u64,
    pub subtitles_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
pub struct Sequence {
    pub name_id: u32,
    pub duration_ms: u32,
    pub fps: f32,                 // Обычно 24, 30, 60
    pub flags: u32,               // loop, hold_on_end, realtime
    pub track_count: u32,
    pub track_start: u32,
    pub audio_sequence_id: u32,   // Привязка к .alsnd
    pub camera_start_id: u32,
    pub camera_count: u32,
    pub subtitle_start_id: u32,
    pub subtitle_count: u32,
    pub transition_in_ms: u32,    // Длительность перехода
    pub transition_out_ms: u32,
    pub transition_type: u32,     // 0=fade, 1=wipe, 2=cut
}

#[repr(C)]
pub struct AnimationTrack {
    pub target_type: u32,         // 0=camera, 1=light, 2=object, 3=material, 4=postprocess
    pub target_id: u32,
    pub property: u32,            // 0=position, 1=rotation, 2=scale, 3=color, 4=FOV...
    pub interpolation: u32,       // 0=linear, 1=bezier, 2=step, 3=hermite
    pub keyframe_start: u32,
    pub keyframe_count: u32,
}

#[repr(C)]
#[derive(Clone)]
pub struct KeyframeBezier {
    pub time_ms: u32,
    pub value: [f32; 4],          // Может быть позиция, цвет, rotation as quat
    pub in_tangent: [f32; 4],
    pub out_tangent: [f32; 4],
}

#[repr(C)]
pub struct CameraPath {
    pub camera_type: u32,         // 0=perspective, 1=orthographic, 2=cinematic
    pub position_track_id: u32,
    pub lookat_track_id: u32,
    pub up_vector_track_id: u32,
    pub fov_track_id: u32,
    pub aperture_track_id: u32,   // Для глубины резкости
    pub focal_distance_track_id: u32,
    pub shake_track_id: u32,      // Тряска камеры
    pub shake_intensity: f32,
    pub shake_frequency: f32,
}

#[repr(C)]
pub struct EventTrigger {
    pub time_ms: u32,
    pub event_type: u32,          // 0=spawn, 1=despawn, 2=sound, 3=script, 4=effect
    pub event_data_id: u32,
    pub param1: f32,
    pub param2: f32,
    pub param3: f32,
    pub param4: f32,
}

#[repr(C)]
pub struct Subtitle {
    pub time_start_ms: u32,
    pub time_end_ms: u32,
    pub text_id: u32,
    pub speaker_id: u32,
    pub position: [f32; 2],       // Позиция на экране (0-1)
    pub color: [f32; 4],
    pub font_size: f32,
    pub alignment: u32,           // 0=left, 1=center, 2=right
}

#[repr(C)]
pub struct PostProcessEffect {
    pub effect_type: u32,         // 0=bloom, 1=tonemapping, 2=chromatic, 3=vignette
    pub intensity_track_id: u32,
    pub color_shift_track_id: u32,
}

pub struct AluvFile {
    pub header: AluvHeader,
    pub strings: Vec<String>,
    pub sequences: Vec<Sequence>,
    pub tracks: Vec<AnimationTrack>,
    pub keyframes: Vec<KeyframeBezier>,
    pub camera_paths: Vec<CameraPath>,
    pub events: Vec<EventTrigger>,
    pub subtitles: Vec<Subtitle>,
}

impl AluvFile {
    pub fn new() -> Self {
        Self {
            header: AluvHeader {
                magic: *b"ALKALUV ",
                version: 1,
                sequence_count: 0,
                total_duration_ms: 0,
                string_table_offset: 0,
                sequence_table_offset: 0,
                track_table_offset: 0,
                keyframe_data_offset: 0,
                camera_paths_offset: 0,
                event_triggers_offset: 0,
                subtitles_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            sequences: Vec::new(),
            tracks: Vec::new(),
            keyframes: Vec::new(),
            camera_paths: Vec::new(),
            events: Vec::new(),
            subtitles: Vec::new(),
        }
    }

    pub fn create_opening_cinematic() -> Self {
        let mut aluv = AluvFile::new();

        // Заранее получаем все нужные ID строк
        let seq_name_id = aluv.add_string("Opening_Cinematic");
        let subtitle_text_id = aluv.add_string("В мире, где скорость решает всё...");

        // Создаём камеру
        let cam_track = AnimationTrack {
            target_type: 0,
            target_id: 0,
            property: 0,
            interpolation: 2,
            keyframe_start: 0,
            keyframe_count: 4,
        };
        let track_start = aluv.tracks.len() as u32;
        aluv.tracks.push(cam_track);

        // Добавляем ключевые кадры
        aluv.keyframes.extend_from_slice(&[
            KeyframeBezier {
                time_ms: 0,
                value: [0.0, 5.0, 20.0, 0.0],
                in_tangent: [0.0, 0.0, -5.0, 0.0],
                out_tangent: [0.0, 2.0, -5.0, 0.0],
            },
            KeyframeBezier {
                time_ms: 3000,
                value: [10.0, 10.0, 5.0, 0.0],
                in_tangent: [5.0, 2.0, -10.0, 0.0],
                out_tangent: [5.0, -1.0, 5.0, 0.0],
            },
            KeyframeBezier {
                time_ms: 6000,
                value: [20.0, 3.0, 10.0, 0.0],
                in_tangent: [5.0, -2.0, 5.0, 0.0],
                out_tangent: [0.0, -1.0, 0.0, 0.0],
            },
            KeyframeBezier {
                time_ms: 8000,
                value: [20.0, 1.5, 3.0, 0.0],
                in_tangent: [0.0, -0.5, -2.0, 0.0],
                out_tangent: [0.0, 0.0, 0.0, 0.0],
            },
        ]);

        // Теперь используем заранее полученный ID
        aluv.subtitles.push(Subtitle {
            time_start_ms: 1000,
            time_end_ms: 5000,
            text_id: subtitle_text_id,  // Используем готовый ID
            speaker_id: 0xFFFFFFFF,
            position: [0.5, 0.9],
            color: [1.0, 1.0, 1.0, 1.0],
            font_size: 24.0,
            alignment: 1,
        });

        // Собираем секвенцию
        aluv.sequences.push(Sequence {
            name_id: seq_name_id,  // Используем готовый ID
            duration_ms: 8000,
            fps: 30.0,
            flags: 0,
            track_count: 1,
            track_start,
            audio_sequence_id: 0xFFFFFFFF,
            camera_start_id: 0,
            camera_count: 1,
            subtitle_start_id: 0,
            subtitle_count: 1,
            transition_in_ms: 500,
            transition_out_ms: 1000,
            transition_type: 0,
        });

        aluv.header.sequence_count = 1;
        aluv.header.total_duration_ms = 8000;

        aluv
    }

    fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }
}