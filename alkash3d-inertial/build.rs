// inertial/build.rs

fn main() {
    println!("cargo:rerun-if-changed=src/kernels/");
    println!("cargo:rerun-if-changed=src/fortran/");

    // Проверяем наличие Fortran файлов
    if !std::path::Path::new("src/kernels/rigid_body.f90").exists() {
        println!("cargo:warning=⚠️ Fortran kernels not found, using pure Rust physics");
        return;
    }

    // Ищем gfortran
    let gfortran = match which::which("gfortran") {
        Ok(path) => {
            println!("cargo:warning=✅ Found gfortran at: {}", path.display());
            path
        }
        Err(_) => {
            println!("cargo:warning=⚠️ gfortran not found - building pure Rust physics");
            return;
        }
    };

    // Список Fortran файлов
    let fortran_files = [
        "src/kernels/rigid_body.f90",
        "src/kernels/broad_phase.f90",
        "src/kernels/narrow_phase.f90",
        "src/kernels/solver.f90",
        "src/kernels/kernels_optimized.f90",
    ];

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::PathBuf::from(&out_dir);
    let dll_path = out_path.join("inertial.dll");

    // Собираем только существующие файлы
    let mut existing_files = Vec::new();
    for file in &fortran_files {
        if std::path::Path::new(file).exists() {
            existing_files.push(*file);
        } else {
            println!("cargo:warning=⚠️ Missing: {}", file);
        }
    }

    if existing_files.is_empty() {
        println!("cargo:warning=⚠️ No Fortran files found");
        return;
    }

    // Компилируем в DLL
    let status = std::process::Command::new(&gfortran)
        .arg("-shared")
        .arg("-o")
        .arg(&dll_path)
        .args(&existing_files)
        .arg("-fopenmp")
        .arg("-O3")
        .arg("-march=native")
        .arg("-ffast-math")
        .arg("-static-libgcc")
        .arg("-static-libgfortran")
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=✅ Inertial DLL created at: {}", dll_path.display());

            // Копируем в target/release
            let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
            let target_dir = manifest_dir.join("target").join(std::env::var("PROFILE").unwrap());
            let target_dll = target_dir.join("inertial.dll");
            let _ = std::fs::copy(&dll_path, &target_dll);
            println!("cargo:warning=✅ DLL copied to: {}", target_dll.display());
        }
        Ok(s) => {
            println!("cargo:warning=❌ Failed to create DLL: exit code {}", s);
        }
        Err(e) => {
            println!("cargo:warning=❌ Failed to create DLL: {}", e);
        }
    }
}