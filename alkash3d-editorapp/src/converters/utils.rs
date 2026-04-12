use anyhow::Result;
use std::path::Path;

pub fn ensure_output_dir(path: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn replace_extension(file_path: &str, new_ext: &str) -> String {
    let path = Path::new(file_path);
    let stem = path.file_stem().unwrap().to_str().unwrap();
    format!("{}.{}", stem, new_ext)
}