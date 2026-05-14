// build.rs
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/kernels/");

    // Проверяем, есть ли gfortran в PATH
    let gfortran_found = match which::which("gfortran") {
        Ok(path) => {
            println!("cargo:warning=Found gfortran at: {}", path.display());
            true
        }
        Err(_) => {
            println!("cargo:warning=gfortran not found in PATH");
            println!("cargo:warning=Building without Fortran optimizations");
            println!("cargo:warning=To enable Fortran, add to PATH: C:\\msys64\\mingw64\\bin");
            false
        }
    };

    if !gfortran_found {
        // Просто выходим, не паникуем
        return;
    }

    let mut build = cc::Build::new();
    build.compiler("gfortran");

    // Порядок файлов важен!
    build.files([
        "src/kernels/rigid_body.f90",
        "src/kernels/broad_phase.f90",
        "src/kernels/narrow_phase.f90",
        "src/kernels/solver.f90",
    ]);

    build.flag("-O2")
        .flag("-ffast-math");

    // Указываем путь для .mod файлов
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    build.flag(&format!("-J{}", out_dir.display()));

    build.compile("libinertial_fortran.a");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:warning=Fortran compilation completed!");
}