//! Управление ассетами

mod importer;
mod library;

use std::collections::HashMap;
use std::path::{PathBuf};
use anyhow::Result;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone)]
pub struct AssetLibrary {
    pub root_path: PathBuf,
    pub meshes: HashMap<String, MeshAsset>,
    pub textures: HashMap<String, TextureAsset>,
    pub materials: HashMap<String, MaterialAsset>,
    pub sounds: HashMap<String, SoundAsset>,
}

#[derive(Debug, Clone)]
pub struct MeshAsset {
    pub path: PathBuf,
    pub vertex_count: u32,
    pub index_count: u32,
    pub bounds: (Vec3, Vec3),
}

#[derive(Debug, Clone)]
pub struct TextureAsset {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
}

#[derive(Debug, Clone)]
pub enum TextureFormat {
    RGBA8,
    RGBA32F,
    BC1,
    BC3,
    BC5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialAsset {
    pub name: String,
    pub albedo: Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub ao: f32,
    pub emissive: Vec3,
    pub albedo_map: Option<String>,
    pub normal_map: Option<String>,
    pub metallic_map: Option<String>,
    pub roughness_map: Option<String>,
    pub ao_map: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SoundAsset {
    pub path: PathBuf,
    pub duration: f32,
    pub channels: u32,
    pub sample_rate: u32,
}

impl AssetLibrary {
    pub fn new() -> Self {
        Self {
            root_path: PathBuf::from("assets"),
            meshes: HashMap::new(),
            textures: HashMap::new(),
            materials: HashMap::new(),
            sounds: HashMap::new(),
        }
    }

    pub fn set_root_path(&mut self, path: impl Into<PathBuf>) {
        self.root_path = path.into();
    }

    pub fn scan(&mut self) -> Result<()> {
        self.scan_meshes()?;
        self.scan_textures()?;
        self.scan_materials()?;
        self.scan_sounds()?;
        Ok(())
    }

    fn scan_meshes(&mut self) -> Result<()> {
        let mesh_path = self.root_path.join("meshes");
        if !mesh_path.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&mesh_path) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "altex" || ext == "obj" || ext == "gltf" {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();

                        self.meshes.insert(name, MeshAsset {
                            path: path.to_path_buf(),
                            vertex_count: 0,
                            index_count: 0,
                            bounds: (Vec3::ZERO, Vec3::ONE),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn scan_textures(&mut self) -> Result<()> {
        let texture_path = self.root_path.join("textures");
        if !texture_path.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&texture_path) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if matches!(ext_str.as_str(), "png" | "jpg" | "jpeg" | "dds" | "hdr") {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();

                        self.textures.insert(name, TextureAsset {
                            path: path.to_path_buf(),
                            width: 0,
                            height: 0,
                            format: TextureFormat::RGBA8,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn scan_materials(&mut self) -> Result<()> {
        let material_path = self.root_path.join("materials");
        if !material_path.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&material_path) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |e| e == "almat") {
                if let Ok(data) = std::fs::read_to_string(path) {
                    if let Ok(mat) = serde_json::from_str(&data) {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();
                        self.materials.insert(name, mat);
                    }
                }
            }
        }

        Ok(())
    }

    fn scan_sounds(&mut self) -> Result<()> {
        let sound_path = self.root_path.join("sounds");
        if !sound_path.exists() {
            return Ok(());
        }

        for entry in walkdir::WalkDir::new(&sound_path) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if matches!(ext_str.as_str(), "wav" | "mp3" | "ogg" | "flac") {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();

                        self.sounds.insert(name, SoundAsset {
                            path: path.to_path_buf(),
                            duration: 0.0,
                            channels: 0,
                            sample_rate: 0,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    pub fn get_mesh(&self, name: &str) -> Option<&MeshAsset> {
        self.meshes.get(name)
    }

    pub fn get_texture(&self, name: &str) -> Option<&TextureAsset> {
        self.textures.get(name)
    }

    pub fn get_material(&self, name: &str) -> Option<&MaterialAsset> {
        self.materials.get(name)
    }

    pub fn get_sound(&self, name: &str) -> Option<&SoundAsset> {
        self.sounds.get(name)
    }
}

impl Default for AssetLibrary {
    fn default() -> Self {
        Self::new()
    }
}

use crate::math::{Vec3, Vec4};