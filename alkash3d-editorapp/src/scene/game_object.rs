use std::collections::HashMap;
use uuid::Uuid;
use crate::math::Transform;
use crate::animation::AnimationTrack;  // Исправлено
use super::object_type::ObjectType;

#[derive(Debug, Clone)]
pub struct GameObject {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub transform: Transform,
    pub object_type: ObjectType,
    pub animations: HashMap<String, Animation>,  // Нужно создать тип Animation
    pub shader_technique: String,
}

// Временно определим Animation здесь
#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub position_track: AnimationTrack<crate::math::Vec3>,
    pub rotation_track: AnimationTrack<crate::math::Quat>,
    pub scale_track: AnimationTrack<crate::math::Vec3>,
    pub duration: f32,
    pub playing: bool,
    pub current_time: f32,
}

impl Animation {
    pub fn new(name: String) -> Self {
        Self {
            name,
            position_track: AnimationTrack::new(),
            rotation_track: AnimationTrack::new(),
            scale_track: AnimationTrack::new(),
            duration: 0.0,
            playing: false,
            current_time: 0.0,
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        if self.playing {
            self.current_time += delta_time;
            if self.current_time > self.duration {
                self.current_time = 0.0;
            }
        }
    }

    pub fn get_transform(&self) -> Transform {
        Transform {
            position: self.position_track.evaluate(self.current_time).unwrap_or(crate::math::Vec3::ZERO),
            rotation: self.rotation_track.evaluate(self.current_time).unwrap_or(crate::math::Quat::IDENTITY),
            scale: self.scale_track.evaluate(self.current_time).unwrap_or(crate::math::Vec3::ONE),
        }
    }
}

impl GameObject {
    pub fn new(name: &str, object_type: ObjectType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            visible: true,
            locked: false,
            transform: Transform::default(),
            object_type,
            animations: HashMap::new(),
            shader_technique: "PBR_Standard".to_string(),
        }
    }
}