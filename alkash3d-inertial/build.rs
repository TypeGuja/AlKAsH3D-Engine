// build.rs
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/kernels/");

    // Проверяем папку с kernels
    if !std::path::Path::new("src/kernels/rigid_body.f90").exists() {
        println!("cargo:warning=⚠️ Fortran kernels not found");
        return;
    }

    match which::which("gfortran") {
        Ok(path) => println!("cargo:warning=✅ Found gfortran at: {}", path.display()),
        Err(_) => {
            println!("cargo:warning=⚠️ gfortran not found - building pure Rust");
            return;
        }
    }

    let mut build = cc::Build::new();
    build.compiler("gfortran");

    // Только существующие файлы
    for file in &["src/kernels/rigid_body.f90", "src/kernels/broad_phase.f90",
        "src/kernels/narrow_phase.f90", "src/kernels/solver.f90"] {
        if std::path::Path::new(file).exists() {
            build.file(file);
        }
    }

    // Агрессивная оптимизация
    build.flag("-O3")
        .flag("-march=native")
        .flag("-mtune=native")
        .flag("-ffast-math")
        .flag("-funroll-loops");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    build.flag(&format!("-J{}", out_dir.display()));

    build.compile("libinertial_fortran.a");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:warning=✅ Fortran compiled!");
}