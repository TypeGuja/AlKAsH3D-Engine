use std::collections::HashMap;
use uuid::Uuid;
use crate::math::Vec3;
use super::game_object::GameObject;

pub struct Scene {
    pub name: String,
    pub objects: HashMap<Uuid, GameObject>,
    pub selected_ids: Vec<Uuid>,
    pub ambient_color: [f32; 3],
    pub grid_enabled: bool,
    pub playing: bool,
    pub animation_time: f32,
    pub dirty: bool,
}

impl Scene {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            objects: HashMap::new(),
            selected_ids: Vec::new(),
            ambient_color: [0.2, 0.2, 0.25],
            grid_enabled: true,
            playing: false,
            animation_time: 0.0,
            dirty: false,
        }
    }

    pub fn add_object(&mut self, obj: GameObject) -> Uuid {
        let id = obj.id;
        self.objects.insert(id, obj);
        self.dirty = true;
        id
    }

    pub fn remove_object(&mut self, id: Uuid) -> Option<GameObject> {
        self.selected_ids.retain(|&sid| sid != id);
        self.dirty = true;
        self.objects.remove(&id)
    }

    pub fn get_object(&self, id: Uuid) -> Option<&GameObject> {
        self.objects.get(&id)
    }

    pub fn get_object_mut(&mut self, id: Uuid) -> Option<&mut GameObject> {
        self.dirty = true;
        self.objects.get_mut(&id)
    }

    pub fn selected_objects(&self) -> Vec<&GameObject> {
        self.selected_ids.iter()
            .filter_map(|id| self.objects.get(id))
            .collect()
    }

    pub fn select(&mut self, id: Uuid, add: bool) {
        if add {
            if !self.selected_ids.contains(&id) {
                self.selected_ids.push(id);
            }
        } else {
            self.selected_ids.clear();
            self.selected_ids.push(id);
        }
    }

    pub fn delete_selected(&mut self) {
        let ids: Vec<Uuid> = self.selected_ids.drain(..).collect();
        for id in ids {
            self.objects.remove(&id);
        }
        self.dirty = true;
    }

    pub fn get_world_transform(&self, id: Uuid) -> crate::math::Transform {
        self.objects.get(&id)
            .map(|obj| obj.transform)
            .unwrap_or_default()
    }

    pub fn update(&mut self, delta_time: f32) {
        if self.playing {
            self.animation_time += delta_time;
        }
    }
}