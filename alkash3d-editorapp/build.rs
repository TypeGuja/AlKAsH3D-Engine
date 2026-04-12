// editor/build.rs
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir).ancestors().nth(3).unwrap();
    let assets_dir = target_dir.join("assets");

    if Path::new("assets").exists() {
        let _ = fs::create_dir_all(&assets_dir);
        copy_dir("assets", &assets_dir);
    }
}

fn copy_dir(from: &str, to: &Path) {
    if let Ok(entries) = fs::read_dir(from) {
        for entry in entries.flatten() {
            let path = entry.path();
            let dest = to.join(entry.file_name());
            if path.is_dir() {
                let _ = fs::create_dir_all(&dest);
                copy_dir(path.to_str().unwrap(), &dest);
            } else {
                let _ = fs::copy(&path, &dest);
            }
        }
    }
}