//! Конвертер FBX -> Altex (заглушка)

use anyhow::{Result, anyhow};

pub fn convert(fbx_path: &str, _output_path: &str) -> Result<()> {
    println!("[FBX] Converting: {}", fbx_path);
    Err(anyhow!("FBX support requires external converter. Please convert to OBJ or glTF first."))
}