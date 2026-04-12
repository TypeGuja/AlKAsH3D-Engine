// editor/src/ui.rs
pub struct EditorState {
    pub selected_object: Option<u32>,
    pub scene_name: String,
    pub camera_position: [f32; 3],
    pub camera_target: [f32; 3],
    pub play_mode: bool,
    pub fps: f32,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            selected_object: None,
            scene_name: "Untitled".to_string(),
            camera_position: [10.0, 10.0, 10.0],
            camera_target: [0.0, 0.0, 0.0],
            play_mode: false,
            fps: 0.0,
        }
    }
}