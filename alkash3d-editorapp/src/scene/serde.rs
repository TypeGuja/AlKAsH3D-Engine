//! Сериализация и десериализация сцены

use super::{Scene, GameObject, SceneSettings, SceneMetadata};
use anyhow::Result;
use serde_derive::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableScene {
    pub name: String,
    pub objects: Vec<GameObject>,
    pub settings: SceneSettings,
    pub metadata: SceneMetadata,
}

pub fn serialize_scene(scene: &Scene) -> Result<String> {
    let serializable = SerializableScene {
        name: scene.name.clone(),
        objects: scene.objects.values().cloned().collect(),
        settings: scene.settings.clone(),
        metadata: scene.metadata.clone(),
    };
    serde_json::to_string_pretty(&serializable)
        .map_err(|e| anyhow::anyhow!("Failed to serialize scene: {}", e))
}

pub fn deserialize_scene(data: &str) -> Result<Scene> {
    let serializable: SerializableScene = serde_json::from_str(data)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize scene: {}", e))?;
    let mut scene = Scene::new(serializable.name);
    scene.settings = serializable.settings;
    scene.metadata = serializable.metadata;
    for obj in serializable.objects {
        scene.add_object(obj);
    }
    Ok(scene)
}

pub fn serialize_gameobject(obj: &GameObject) -> Result<String> {
    serde_json::to_string_pretty(obj)
        .map_err(|e| anyhow::anyhow!("Failed to serialize gameobject: {}", e))
}

pub fn deserialize_gameobject(data: &str) -> Result<GameObject> {
    serde_json::from_str(data)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize gameobject: {}", e))
}