//! Конвертеры форматов

pub mod obj;
pub mod blend;
pub mod fbx;
pub mod gltf;

use anyhow::Result;
use std::path::Path;

/// Определяет тип файла и выбирает подходящий конвертер
pub fn convert_to_altex(input_path: &str, output_path: &str) -> Result<()> {
    let path = Path::new(input_path);
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "obj" => obj::convert(input_path, output_path),
        "blend" => blend::convert(input_path, output_path),
        "fbx" => fbx::convert(input_path, output_path),
        "gltf" | "glb" => gltf::convert(input_path, output_path),
        _ => Err(anyhow::anyhow!("Unsupported format: {}", extension)),
    }
}

/// Пакетная конвертация
pub fn batch_convert(
    input_dir: &str,
    output_dir: &str,
    format: &str,
) -> Result<Vec<String>> {
    let mut converted = Vec::new();

    let input_path = Path::new(input_dir);
    let output_path = Path::new(output_dir);

    std::fs::create_dir_all(output_path)?;

    for entry in walkdir::WalkDir::new(input_path) {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext.to_str().unwrap().to_lowercase() == format {
                    let rel_path = path.strip_prefix(input_path)?;
                    let out_path = output_path.join(rel_path).with_extension("altex");

                    std::fs::create_dir_all(out_path.parent().unwrap())?;

                    convert_to_altex(
                        path.to_str().unwrap(),
                        out_path.to_str().unwrap(),
                    )?;

                    converted.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(converted)
}