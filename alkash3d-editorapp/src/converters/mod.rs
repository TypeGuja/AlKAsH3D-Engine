//! Конвертеры форматов

pub mod obj;
pub mod blend;
pub mod fbx;
pub mod gltf;

// ДОБАВЛЕНО (полноценный эдитор — реальные форматы движка): конвертеры в
// настоящие бинарные форматы alkash3d-rust (alkash3d_rs::AltexFile/
// AlworldFile/AlfarFile/... — та же зависимость, что использует движок в
// рантайме, см. Cargo.toml). obj/blend/fbx/gltf выше — конвертеры МЕЖДУ
// сторонними форматами (для превью/промежуточного OBJ), эти — экспорт/
// импорт РОДНЫХ форматов движка, которые эдитор реально сохраняет и грузит.
pub mod altex;
pub mod alworld;
pub mod alfar;
pub mod alsnd;
pub mod alcar;
pub mod alroute;
pub mod alscript;
pub mod alasm;

use anyhow::Result;

/// Конвертирует в OBJ с выбором качества (0.05-1.0)
pub fn convert_to_obj(input_path: &str, output_path: &str, quality: f32) -> Result<()> {
    let path = std::path::Path::new(input_path);
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "obj" => {
            std::fs::copy(input_path, output_path)?;
            Ok(())
        }
        "blend" => blend::convert_with_quality(input_path, output_path, quality),
        "fbx" => fbx::convert_with_quality(input_path, output_path, quality),
        "gltf" | "glb" => gltf::convert_with_quality(input_path, output_path, quality),
        _ => Err(anyhow::anyhow!("Unsupported format: {}", extension)),
    }
}