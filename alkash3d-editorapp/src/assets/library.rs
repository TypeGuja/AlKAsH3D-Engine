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

        println!("[ASSET] Importing model: {} (name: {})", path, file_name);

        let mesh = self.parse_obj(path)?;
        self.meshes.insert(file_name.clone(), mesh);
        Ok(vec![file_name])
    }

    fn parse_obj(&self, path: &str) -> Result<Mesh, String> {
        println!("[ASSET] Parsing OBJ: {}", path);

        // Проверяем существование файла
        if !Path::new(path).exists() {
            return Err(format!("File not found: {}", path));
        }

        let (models, materials) = tobj::load_obj(
            path,
            &tobj::LoadOptions {
                single_index: true,
                triangulate: true,
                ignore_points: true,
                ignore_lines: true,
            },
        ).map_err(|e| {
            println!("[ASSET] tobj error: {}", e);
            format!("Failed to load OBJ: {}", e)
        })?;

        let _ = materials; // Не используем пока

        println!("[ASSET] Loaded {} models", models.len());

        if models.is_empty() {
            return Err("No models found in OBJ file".to_string());
        }

        // Объединяем все модели в один меш (или берем первую)
        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();
        let mut vertex_offset = 0u32;

        for (idx, model) in models.iter().enumerate() {
            let mesh = &model.mesh;
            let vertex_count = mesh.positions.len() / 3;

            println!("[ASSET] Model {}: {} vertices, {} indices",
                     idx, vertex_count, mesh.indices.len());

            if vertex_count == 0 {
                continue;
            }

            // Добавляем вершины
            for i in 0..vertex_count {
                all_vertices.push(crate::math::Vec3::new(
                    mesh.positions[i * 3],
                    mesh.positions[i * 3 + 1],
                    mesh.positions[i * 3 + 2],
                ));
            }

            // Добавляем индексы со смещением
            for &idx in &mesh.indices {
                all_indices.push(idx as u32 + vertex_offset);
            }

            vertex_offset += vertex_count as u32;
        }

        if all_vertices.is_empty() {
            return Err("No vertices in model".to_string());
        }

        println!("[ASSET] Total: {} vertices, {} indices",
                 all_vertices.len(), all_indices.len());

        Ok(Mesh::new(all_vertices, all_indices))
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