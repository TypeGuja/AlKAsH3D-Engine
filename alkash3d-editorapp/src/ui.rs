// editor/src/ui.rs

#[derive(Clone)]
pub struct SceneObject {
    pub id: u32,
    pub name: String,
    pub object_type: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

pub struct EditorState {
    pub selected_object: Option<u32>,
    pub objects: Vec<SceneObject>,
    pub scene_name: String,
    pub camera_position: [f32; 3],
    pub camera_target: [f32; 3],
    pub play_mode: bool,
    pub fps: f32,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            selected_object: Some(0),
            objects: vec![
                SceneObject {
                    id: 0,
                    name: "Main Camera".to_string(),
                    object_type: "camera".to_string(),
                    position: [0.0, 5.0, 10.0],
                    rotation: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                },
                SceneObject {
                    id: 1,
                    name: "Directional Light".to_string(),
                    object_type: "light".to_string(),
                    position: [5.0, 10.0, 5.0],
                    rotation: [45.0, 30.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                },
                SceneObject {
                    id: 2,
                    name: "Ground".to_string(),
                    object_type: "mesh".to_string(),
                    position: [0.0, -1.0, 0.0],
                    rotation: [0.0, 0.0, 0.0],
                    scale: [10.0, 1.0, 10.0],
                },
                SceneObject {
                    id: 3,
                    name: "Player Spawn".to_string(),
                    object_type: "spawn".to_string(),
                    position: [0.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                },
            ],
            scene_name: "Untitled".to_string(),
            camera_position: [10.0, 10.0, 10.0],
            camera_target: [0.0, 0.0, 0.0],
            play_mode: false,
            fps: 0.0,
        }
    }
}