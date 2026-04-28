use std::collections::HashMap;
use uuid::Uuid;
use crate::math::{Vec3, Quat};

#[derive(Debug, Clone)]
pub struct CinematicManager {
    pub sequences: HashMap<String, CinematicSequence>,
    pub active_sequence: Option<String>,
    pub playback_time: f32,
    pub is_playing: bool,
}

#[derive(Debug, Clone)]
pub struct CinematicSequence {
    pub name: String,
    pub duration: f32,
    pub fps: f32,
    pub camera_paths: Vec<CameraPathData>,
    pub subtitles: Vec<Subtitle>,
    pub events: Vec<CinematicEvent>,
}

#[derive(Debug, Clone)]
pub struct CameraPathData {
    pub camera_id: Uuid,
    pub position_track: Vec<KeyframeData>,
    pub fov_track: Vec<(f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct KeyframeData {
    pub time: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

#[derive(Debug, Clone)]
pub struct CinematicEvent {
    pub time: f32,
    pub event_type: CinematicEventType,
    pub params: Vec<f32>,
}

#[derive(Debug, Clone)]
pub enum CinematicEventType {
    SpawnObject,
    DespawnObject,
    PlaySound,
    ExecuteScript,
    SpawnEffect,
}

#[derive(Debug, Clone)]
pub struct Subtitle {
    pub start_time: f32,
    pub end_time: f32,
    pub text: String,
    pub position: (f32, f32),
    pub color: [f32; 4],
}

impl CinematicManager {
    pub fn new() -> Self {
        Self {
            sequences: HashMap::new(),
            active_sequence: None,
            playback_time: 0.0,
            is_playing: false,
        }
    }

    pub fn create_opening_sequence(&mut self) {
        let mut sequence = CinematicSequence {
            name: "Opening Cinematic".to_string(),
            duration: 10.0,
            fps: 30.0,
            camera_paths: Vec::new(),
            subtitles: Vec::new(),
            events: Vec::new(),
        };

        sequence.camera_paths.push(CameraPathData {
            camera_id: Uuid::new_v4(),
            position_track: vec![
                KeyframeData { time: 0.0, position: Vec3::new(0.0, 10.0, 20.0), rotation: Quat::IDENTITY, scale: Vec3::ONE },
                KeyframeData { time: 5.0, position: Vec3::new(10.0, 5.0, 5.0), rotation: Quat::IDENTITY, scale: Vec3::ONE },
            ],
            fov_track: vec![(0.0, 60.0), (5.0, 45.0)],
        });

        sequence.subtitles.push(Subtitle {
            start_time: 1.0,
            end_time: 4.0,
            text: "В мире, где скорость решает всё...".to_string(),
            position: (0.5, 0.9),
            color: [1.0, 1.0, 1.0, 1.0],
        });

        sequence.events.push(CinematicEvent {
            time: 2.0,
            event_type: CinematicEventType::PlaySound,
            params: vec![1.0, 0.8, 0.0],
        });

        self.sequences.insert("opening".to_string(), sequence);
    }

    pub fn play_sequence(&mut self, name: &str) {
        if self.sequences.contains_key(name) {
            self.active_sequence = Some(name.to_string());
            self.playback_time = 0.0;
            self.is_playing = true;
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        if !self.is_playing { return; }
        self.playback_time += delta_time;
    }

    pub fn get_active_subtitles(&self) -> Vec<&Subtitle> {
        if let Some(seq_name) = &self.active_sequence {
            if let Some(seq) = self.sequences.get(seq_name) {
                return seq.subtitles.iter()
                    .filter(|s| self.playback_time >= s.start_time && self.playback_time <= s.end_time)
                    .collect();
            }
        }
        Vec::new()
    }
}