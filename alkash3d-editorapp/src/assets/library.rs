use std::collections::HashMap;
use std::path::Path;
use crate::mesh::Mesh;
use crate::material::Material;

pub struct AssetLibrary {
    pub meshes: HashMap<String, Mesh>,
    pub materials: HashMap<String, Material>,
}

impl AssetLibrary {
    pub fn new() -> Self {
        let mut meshes = HashMap::new();
        meshes.insert("cube".to_string(), Mesh::create_cube());
        meshes.insert("sphere".to_string(), Mesh::create_sphere());
        meshes.insert("plane".to_string(), Mesh::create_plane());
        meshes.insert("cylinder".to_string(), Mesh::create_cylinder());
        meshes.insert("cone".to_string(), Mesh::create_cone());
        meshes.insert("torus".to_string(), Mesh::create_torus());

        let mut materials = HashMap::new();
        materials.insert("default".to_string(), Material::default());
        materials.insert("red".to_string(), Material { color: [1.0, 0.2, 0.2, 1.0], ..Default::default() });
        materials.insert("green".to_string(), Material { color: [0.2, 1.0, 0.2, 1.0], ..Default::default() });
        materials.insert("blue".to_string(), Material { color: [0.2, 0.2, 1.0, 1.0], ..Default::default() });
        materials.insert("metal".to_string(), Material { metallic: 1.0, roughness: 0.3, ..Default::default() });

        Self { meshes, materials }
    }

    pub fn import_model(&mut self, path: &str) -> Result<Vec<String>, String> {
        let file_name = Path::new(path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let mesh = self.parse_obj(path)?;
        self.meshes.insert(file_name.clone(), mesh);
        Ok(vec![file_name])
    }

    fn parse_obj(&self, path: &str) -> Result<Mesh, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("v ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    vertices.push(crate::math::Vec3::new(
                        parts[1].parse().unwrap_or(0.0),
                        parts[2].parse().unwrap_or(0.0),
                        parts[3].parse().unwrap_or(0.0),
                    ));
                }
            } else if line.starts_with("f ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let mut face = Vec::new();
                    for i in 1..parts.len() {
                        let idx = parts[i].split('/').next().unwrap_or("0");
                        if let Ok(i) = idx.parse::<i32>() {
                            let idx = if i > 0 { i - 1 } else { vertices.len() as i32 + i };
                            if idx >= 0 && idx < vertices.len() as i32 {
                                face.push(idx as u32);
                            }
                        }
                    }
                    if face.len() >= 3 {
                        for j in 1..face.len()-1 {
                            indices.extend_from_slice(&[face[0], face[j], face[j+1]]);
                        }
                    }
                }
            }
        }

        if vertices.is_empty() { return Err("No vertices".to_string()); }
        Ok(Mesh::new(vertices, indices))
    }

    pub fn get_mesh(&self, name: &str) -> Option<&Mesh> {
        self.meshes.get(name)
    }

    pub fn list_meshes(&self) -> Vec<String> {
        self.meshes.keys().cloned().collect()
    }

    pub fn list_materials(&self) -> Vec<String> {
        self.materials.keys().cloned().collect()
    }
}