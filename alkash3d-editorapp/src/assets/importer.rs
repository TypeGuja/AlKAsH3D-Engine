//! Импорт ассетов

use anyhow::Result;
use std::path::{Path, PathBuf};
use crate::converters;

pub struct AssetImporter {
    pub import_path: PathBuf,
    pub output_path: PathBuf,
    pub preserve_structure: bool,
    pub generate_mipmaps: bool,
    pub compress_textures: bool,
}

impl Default for AssetImporter {
    fn default() -> Self {
        Self {
            import_path: PathBuf::from("import"),
            output_path: PathBuf::from("assets"),
            preserve_structure: true,
            generate_mipmaps: true,
            compress_textures: true,
        }
    }
}

impl AssetImporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn import_file(&self, path: &str) -> Result<Vec<String>> {
        let input_path = Path::new(path);
        let extension = input_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let file_name = input_path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");

        let output_path = self.output_path.join("meshes")
            .join(format!("{}.altex", file_name));

        std::fs::create_dir_all(output_path.parent().unwrap())?;

        let output_str = output_path.to_str().unwrap();

        match extension.as_str() {
            "obj" => {
                converters::obj::convert(path, output_str)?;
                Ok(vec![output_str.to_string()])
            }
            "blend" => {
                converters::blend::convert(path, output_str)?;
                Ok(vec![output_str.to_string()])
            }
            "gltf" | "glb" => {
                converters::gltf::convert(path, output_str)?;
                Ok(vec![output_str.to_string()])
            }
            _ => Err(anyhow::anyhow!("Unsupported format: {}", extension)),
        }
    }

    pub fn import_directory(&self, path: &str) -> Result<Vec<String>> {
        let mut imported = Vec::new();

        for entry in walkdir::WalkDir::new(path) {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_str().unwrap().to_lowercase();
                    if matches!(ext_str.as_str(), "obj" | "blend" | "gltf" | "glb") {
                        if let Ok(files) = self.import_file(path.to_str().unwrap()) {
                            imported.extend(files);
                        }
                    }
                }
            }
        }

        Ok(imported)
    }
}