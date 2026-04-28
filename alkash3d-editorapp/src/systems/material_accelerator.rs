use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MaterialAccelerator {
    pub materials: HashMap<String, AcceleratedMaterial>,
    pub draw_calls: u64,
}

#[derive(Debug, Clone)]
pub struct AcceleratedMaterial {
    pub name: String,
    pub shader_hash: u64,
    pub batch_group: u32,
    pub lod_material_id: Option<String>,
    pub draw_calls_count: u32,
    pub albedo: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

impl MaterialAccelerator {
    pub fn new() -> Self {
        let mut materials = HashMap::new();

        materials.insert("pbr_standard".to_string(), AcceleratedMaterial {
            name: "PBR Standard".to_string(),
            shader_hash: 0xDEADBEEF,
            batch_group: 0,
            lod_material_id: Some("pbr_lod1".to_string()),
            draw_calls_count: 0,
            albedo: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.5,
        });

        materials.insert("metal_rough".to_string(), AcceleratedMaterial {
            name: "Metal Rough".to_string(),
            shader_hash: 0xCAFEBABE,
            batch_group: 1,
            lod_material_id: None,
            draw_calls_count: 0,
            albedo: [0.7, 0.7, 0.7, 1.0],
            metallic: 1.0,
            roughness: 0.3,
        });

        Self { materials, draw_calls: 0 }
    }

    pub fn get_or_create(&mut self, name: &str) -> &AcceleratedMaterial {
        if !self.materials.contains_key(name) {
            self.materials.insert(name.to_string(), AcceleratedMaterial {
                name: name.to_string(),
                shader_hash: 0,
                batch_group: self.materials.len() as u32,
                lod_material_id: None,
                draw_calls_count: 0,
                albedo: [1.0, 1.0, 1.0, 1.0],
                metallic: 0.0,
                roughness: 0.5,
            });
        }
        self.materials.get(name).unwrap()
    }
}