//! Библиотека ассетов

use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;
use crate::render::RenderEngine;

pub struct AssetLibrary {
    pub meshes: HashMap<String, MeshInfo>,
    pub textures: HashMap<String, TextureInfo>,
    pub materials: HashMap<String, MaterialInfo>,
    pub scenes: HashMap<String, SceneInfo>,
    root_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MeshInfo {
    pub path: PathBuf,
    pub vertex_count: u32,
    pub index_count: u32,
    pub submesh_count: u32,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub struct TextureInfo {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub struct MaterialInfo {
    pub name: String,
    pub shader: String,
    pub properties: HashMap<String, MaterialProperty>,
    pub textures: HashMap<String, String>,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub struct SceneInfo {
    pub path: PathBuf,
    pub object_count: u32,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub enum MaterialProperty {
    Float(f32),
    Float2([f32; 2]),
    Float3([f32; 3]),
    Float4([f32; 4]),
    Int(i32),
    Bool(bool),
    Color([f32; 4]),
}

impl AssetLibrary {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            meshes: HashMap::new(),
            textures: HashMap::new(),
            materials: HashMap::new(),
            scenes: HashMap::new(),
            root_path: root_path.into(),
        }
    }

    pub fn scan(&mut self) -> Result<()> {
        self.scan_meshes()?;
        self.scan_textures()?;
        self.scan_materials()?;
        self.scan_scenes()?;
        Ok(())
    }

    fn scan_meshes(&mut self) -> Result<()> {
        let mesh_dir = self.root_path.join("meshes");
        if !mesh_dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&mesh_dir) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |e| e == "altex") {
                let name = path.file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                self.meshes.insert(name.clone(), MeshInfo {
                    path: path.to_path_buf(),
                    vertex_count: 0,
                    index_count: 0,
                    submesh_count: 0,
                    loaded: false,
                });
            }
        }

        Ok(())
    }

    fn scan_textures(&mut self) -> Result<()> {
        let tex_dir = self.root_path.join("textures");
        if !tex_dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&tex_dir) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let ext = path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "dds" | "hdr" | "altex") {
                    let name = path.file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();

                    self.textures.insert(name, TextureInfo {
                        path: path.to_path_buf(),
                        width: 0,
                        height: 0,
                        format: ext,
                        loaded: false,
                    });
                }
            }
        }

        Ok(())
    }

    fn scan_materials(&mut self) -> Result<()> {
        let mat_dir = self.root_path.join("materials");
        if !mat_dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&mat_dir) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |e| e == "almat") {
                let name = path.file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                // Пытаемся загрузить материал
                if let Ok(data) = std::fs::read_to_string(path) {
                    if let Ok(mat) = serde_json::from_str::<serde_json::Value>(&data) {
                        let mut properties = HashMap::new();

                        if let Some(props) = mat.get("properties") {
                            if let Some(obj) = props.as_object() {
                                for (key, value) in obj {
                                    if let Some(num) = value.as_f64() {
                                        properties.insert(key.clone(),
                                                          MaterialProperty::Float(num as f32));
                                    } else if let Some(arr) = value.as_array() {
                                        match arr.len() {
                                            2 => {
                                                properties.insert(key.clone(),
                                                                  MaterialProperty::Float2([
                                                                      arr[0].as_f64().unwrap_or(0.0) as f32,
                                                                      arr[1].as_f64().unwrap_or(0.0) as f32,
                                                                  ]));
                                            }
                                            3 => {
                                                properties.insert(key.clone(),
                                                                  MaterialProperty::Float3([
                                                                      arr[0].as_f64().unwrap_or(0.0) as f32,
                                                                      arr[1].as_f64().unwrap_or(0.0) as f32,
                                                                      arr[2].as_f64().unwrap_or(0.0) as f32,
                                                                  ]));
                                            }
                                            4 => {
                                                properties.insert(key.clone(),
                                                                  MaterialProperty::Float4([
                                                                      arr[0].as_f64().unwrap_or(0.0) as f32,
                                                                      arr[1].as_f64().unwrap_or(0.0) as f32,
                                                                      arr[2].as_f64().unwrap_or(0.0) as f32,
                                                                      arr[3].as_f64().unwrap_or(0.0) as f32,
                                                                  ]));
                                            }
                                            _ => {}
                                        }
                                    } else if let Some(b) = value.as_bool() {
                                        properties.insert(key.clone(), MaterialProperty::Bool(b));
                                    }
                                }
                            }
                        }

                        self.materials.insert(name.clone(), MaterialInfo {
                            name,
                            shader: mat.get("shader")
                                .and_then(|s| s.as_str())
                                .unwrap_or("standard")
                                .to_string(),
                            properties,
                            textures: HashMap::new(),
                            loaded: false,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn scan_scenes(&mut self) -> Result<()> {
        let scene_dir = self.root_path.join("scenes");
        if !scene_dir.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&scene_dir) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |e| e == "alscene") {
                let name = path.file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();

                self.scenes.insert(name, SceneInfo {
                    path: path.to_path_buf(),
                    object_count: 0,
                    loaded: false,
                });
            }
        }

        Ok(())
    }

    pub fn load_mesh(&mut self, name: &str, renderer: &mut RenderEngine) -> Result<()> {
        if let Some(info) = self.meshes.get_mut(name) {
            if !info.loaded {
                let path_str = info.path.to_str().unwrap();
                renderer.load_altex(path_str)?;
                info.loaded = true;
            }
        }
        Ok(())
    }

    pub fn get_mesh_names(&self) -> Vec<String> {
        self.meshes.keys().cloned().collect()
    }

    pub fn get_texture_names(&self) -> Vec<String> {
        self.textures.keys().cloned().collect()
    }

    pub fn get_material_names(&self) -> Vec<String> {
        self.materials.keys().cloned().collect()
    }

    pub fn get_scene_names(&self) -> Vec<String> {
        self.scenes.keys().cloned().collect()
    }
}