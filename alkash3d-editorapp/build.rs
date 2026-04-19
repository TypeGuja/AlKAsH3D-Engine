//! Build script

fn main() {
    // Проверяем наличие DLL движка
    let dll_name = "alkash3d_rs.dll";
    let search_paths = [
        format!("./{}", dll_name),
        format!("../alkash3d-rust/target/release/{}", dll_name),
        format!("../target/release/{}", dll_name),
        format!("C:/Users/user/Documents/GitHub/AlKAsH3D-Engine/alkash3d-rust/target/release/{}", dll_name),
    ];

    let mut dll_found = false;
    for path in &search_paths {
        if std::path::Path::new(path).exists() {
            println!("cargo:warning=Found {} at: {}", dll_name, path);
            dll_found = true;
            break;
        }
    }

    if !dll_found {
        println!("cargo:warning={} not found. Please build alkash3d_rs first.", dll_name);
        println!("cargo:warning=Searched in: {:?}", search_paths);
    }

    println!("cargo:rerun-if-changed=build.rs");
}