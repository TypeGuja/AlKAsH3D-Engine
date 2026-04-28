#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    pub color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0],
        }
    }
}