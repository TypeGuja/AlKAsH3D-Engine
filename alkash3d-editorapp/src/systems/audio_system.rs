use std::collections::HashMap;
use crate::math::Vec3;

#[derive(Debug, Clone)]
pub struct SoundAsset {
    pub name: String,
    pub format: SoundFormat,
    pub category: SoundCategory,
    pub duration: f32,
    pub default_volume: f32,
    pub default_pitch: f32,
    pub spatial_blend: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SoundFormat { Wav, Ogg, Mp3, Flac, Opus }

#[derive(Debug, Clone, PartialEq)]
pub enum SoundCategory { SFX, Music, Ambient, Voice, UI }

#[derive(Debug, Clone)]
pub struct ActiveSound {
    pub asset_name: String,
    pub position: Vec3,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Debug, Clone)]
pub struct AudioRoom {
    pub position: Vec3,
    pub radius: f32,
    pub reverb_preset: ReverbPreset,
}

#[derive(Debug, Clone)]
pub enum ReverbPreset { Generic, Hall, Room, Chamber, Custom }

#[derive(Debug, Clone)]
pub struct SpatialAudioSystem {
    pub sounds: HashMap<String, SoundAsset>,
    pub active_sounds: Vec<ActiveSound>,
    pub global_volume: f32,
    pub rooms: Vec<AudioRoom>,
    pub occlusion_enabled: bool,
}

impl SpatialAudioSystem {
    pub fn new() -> Self {
        let mut sounds = HashMap::new();

        sounds.insert("engine_start".to_string(), SoundAsset {
            name: "Engine Start".to_string(),
            format: SoundFormat::Ogg,
            category: SoundCategory::SFX,
            duration: 2.5,
            default_volume: 0.8,
            default_pitch: 1.0,
            spatial_blend: 1.0,
        });

        sounds.insert("city_ambient".to_string(), SoundAsset {
            name: "City Ambient".to_string(),
            format: SoundFormat::Ogg,
            category: SoundCategory::Ambient,
            duration: 60.0,
            default_volume: 0.4,
            default_pitch: 1.0,
            spatial_blend: 0.5,
        });

        Self {
            sounds,
            active_sounds: Vec::new(),
            global_volume: 1.0,
            rooms: Vec::new(),
            occlusion_enabled: true,
        }
    }

    pub fn play_sound_at(&mut self, name: &str, position: Vec3, volume: f32) {
        if let Some(sound) = self.sounds.get(name) {
            self.active_sounds.push(ActiveSound {
                asset_name: name.to_string(),
                position,
                volume: volume * sound.default_volume,
                pitch: sound.default_pitch,
            });
        }
    }

    pub fn add_audio_room(&mut self, position: Vec3, radius: f32, preset: ReverbPreset) {
        self.rooms.push(AudioRoom { position, radius, reverb_preset: preset });
    }

    pub fn update_listener(&mut self, listener_pos: Vec3) {
        for sound in &mut self.active_sounds {
            let dist = sound.position.distance(listener_pos);
            let attenuation = 1.0 / (1.0 + dist * 0.1);
            sound.volume *= attenuation;
        }
        self.active_sounds.retain(|s| s.volume > 0.01);
    }
}