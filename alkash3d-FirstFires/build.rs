// build.rs
use std::process::Command;
use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=shaders/");

    let shader_dir = Path::new("shaders");
    let compiled_dir = shader_dir.join("compiled");

    if !compiled_dir.exists() {
        std::fs::create_dir_all(&compiled_dir).unwrap();
    }

    // Компиляция compute шейдера для light culling
    compile_shader(
        shader_dir.join("light_culling.hlsl"),
        compiled_dir.join("light_culling.cso"),
        "cs_5_0",
        "CSMain",
    );
}

fn compile_shader(src: std::path::PathBuf, dst: std::path::PathBuf, target: &str, entry: &str) {
    println!("cargo:rerun-if-changed={}", src.display());

    let output = Command::new("fxc")
        .args(&[
            "/T", target,
            "/E", entry,
            "/O3",  // Максимальная оптимизация
            "/Fo", dst.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            println!("cargo:warning=Shader compiled: {}", src.display());
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            println!("cargo:warning=Shader compile failed: {}", err);
        }
        Err(e) => {
            println!("cargo:warning=Failed to run fxc: {}", e);
        }
    }
}