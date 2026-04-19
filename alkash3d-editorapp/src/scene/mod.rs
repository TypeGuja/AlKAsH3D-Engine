//! Модуль управления сценой

pub mod object;
pub mod serde;

pub use object::*;

use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use crate::math::Vec3;
use serde_derive::{Serialize, Deserialize};

// ============================================================
// Scene
// ============================================================

#[derive(Debug, Clone)]
pub struct Scene {
    pub name: String,
    pub path: Option<String>,
    pub objects: HashMap<Uuid, GameObject>,
    pub root_objects: Vec<Uuid>,
    pub selection: HashSet<Uuid>,
    pub main_camera: Option<Uuid>,
    pub settings: SceneSettings,
    pub metadata: SceneMetadata,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSettings {
    pub ambient_color: Vec3,
    pub ambient_intensity: f32,
    pub skybox_asset: Option<String>,
    pub fog_enabled: bool,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub gravity: Vec3,
    pub time_scale: f32,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            ambient_color: Vec3::new(0.2, 0.25, 0.3),
            ambient_intensity: 0.5,
            skybox_asset: None,
            fog_enabled: false,
            fog_color: Vec3::new(0.5, 0.6, 0.7),
            fog_density: 0.01,
            fog_start: 50.0,
            fog_end: 200.0,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            time_scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub author: String,
    pub version: String,
    pub created_at: u64,
    pub modified_at: u64,
    pub description: String,
}

impl Scene {
    pub fn new(name: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            name: name.into(),
            path: None,
            objects: HashMap::new(),
            root_objects: Vec::new(),
            selection: HashSet::new(),
            main_camera: None,
            settings: SceneSettings::default(),
            metadata: SceneMetadata {
                created_at: now,
                modified_at: now,
                ..Default::default()
            },
            dirty: false,
        }
    }

    pub fn add_object(&mut self, object: GameObject) -> Uuid {
        let id = object.id;
        if object.parent.is_none() {
            self.root_objects.push(id);
        }
        self.objects.insert(id, object);
        self.dirty = true;
        id
    }

    pub fn remove_object(&mut self, id: Uuid) -> Option<GameObject> {
        self.selection.remove(&id);
        self.root_objects.retain(|&root_id| root_id != id);
        if self.main_camera == Some(id) {
            self.main_camera = None;
        }
        let removed = self.objects.remove(&id);
        self.dirty = true;
        removed
    }

    pub fn get_object(&self, id: Uuid) -> Option<&GameObject> {
        self.objects.get(&id)
    }

    pub fn get_object_mut(&mut self, id: Uuid) -> Option<&mut GameObject> {
        self.objects.get_mut(&id)
    }

    pub fn get_selected(&self) -> Vec<&GameObject> {
        self.selection.iter()
            .filter_map(|id| self.objects.get(id))
            .collect()
    }

    pub fn get_selected_mut(&mut self) -> Vec<&mut GameObject> {
        let ids: Vec<Uuid> = self.selection.iter().copied().collect();
        let objects_ptr = &mut self.objects as *mut HashMap<Uuid, GameObject>;
        ids.into_iter()
            .filter_map(move |id| unsafe { (*objects_ptr).get_mut(&id) })
            .collect()
    }

    pub fn select(&mut self, id: Uuid, add: bool) {
        if add {
            self.selection.insert(id);
        } else {
            self.selection.clear();
            self.selection.insert(id);
        }
    }

    pub fn deselect(&mut self, id: Uuid) {
        self.selection.remove(&id);
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    pub fn select_all(&mut self) {
        self.selection.extend(self.objects.keys().copied());
    }

    pub fn duplicate_selected(&mut self) -> Vec<Uuid> {
        let to_duplicate: Vec<GameObject> = self.get_selected()
            .into_iter()
            .cloned()
            .collect();
        let mut new_ids = Vec::new();
        for mut obj in to_duplicate {
            obj.id = Uuid::new_v4();
            obj.name = format!("{} (Copy)", obj.name);
            obj.transform.translation += Vec3::new(1.0, 0.0, 1.0);
            let new_id = obj.id;
            self.add_object(obj);
            new_ids.push(new_id);
        }
        self.selection.clear();
        self.selection.extend(new_ids.iter().copied());
        new_ids
    }

    pub fn delete_selected(&mut self) {
        let to_delete: Vec<Uuid> = self.selection.iter().copied().collect();
        for id in to_delete {
            self.remove_object(id);
        }
    }

    pub fn focus_on_selection(&self, camera: &mut crate::math::Camera) {
        if let Some(first) = self.selection.iter().next() {
            if let Some(obj) = self.objects.get(first) {
                camera.focus_on(obj.world_position());
            }
        } else {
            camera.focus_on(Vec3::ZERO);
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.metadata.modified_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
}

// ============================================================
// Scene Manager
// ============================================================

pub struct SceneManager {
    pub current_scene: Option<Scene>,
    pub recent_scenes: Vec<String>,
}

impl SceneManager {
    pub fn new() -> Self {
        Self {
            current_scene: None,
            recent_scenes: Vec::new(),
        }
    }

    pub fn new_scene(&mut self, name: impl Into<String>) {
        self.current_scene = Some(Scene::new(name));
    }

    pub fn save_scene(&mut self, path: &str) -> anyhow::Result<()> {
        if let Some(scene) = &mut self.current_scene {
            let serialized = serde::serialize_scene(scene)?;
            std::fs::write(path, serialized)?;
            scene.path = Some(path.to_string());
            scene.dirty = false;
            if !self.recent_scenes.contains(&path.to_string()) {
                self.recent_scenes.insert(0, path.to_string());
                self.recent_scenes.truncate(10);
            }
        }
        Ok(())
    }

    pub fn load_scene(&mut self, path: &str) -> anyhow::Result<()> {
        let data = std::fs::read_to_string(path)?;
        let scene = serde::deserialize_scene(&data)?;
        let mut scene = scene;
        scene.path = Some(path.to_string());
        self.current_scene = Some(scene);
        if !self.recent_scenes.contains(&path.to_string()) {
            self.recent_scenes.insert(0, path.to_string());
            self.recent_scenes.truncate(10);
        }
        Ok(())
    }
}

impl Default for SceneManager {
    fn default() -> Self {
        Self::new()
    }
}