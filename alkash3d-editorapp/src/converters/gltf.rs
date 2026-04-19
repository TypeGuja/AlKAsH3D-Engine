use anyhow::{Result, anyhow};

pub fn convert(gltf_path: &str, output_path: &str) -> Result<()> {
    println!("[glTF] Converting: {}", gltf_path);
    println!("[glTF] Output: {}", output_path);

    // Заглушка - требует доработки для полной поддержки glTF
    Err(anyhow!("glTF support is not fully implemented yet. Please use OBJ format."))
}