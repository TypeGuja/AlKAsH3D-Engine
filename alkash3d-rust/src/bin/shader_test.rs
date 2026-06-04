// src/bin/shader_test.rs - ТЕСТ ЗАГРУЗКИ ШЕЙДЕРОВ
use alkash3d_rs::*;

fn main() {
    println!("Testing shader loading from files...\n");

    unsafe {
        // Пробуем загрузить тестовые шейдеры
        let vs = get_test_vs_blob();
        let ps = get_builtin_ps_blob();

        if !vs.is_null() {
            let size = get_blob_size(vs);
            println!("✅ VS_TEST loaded: {} bytes", size);
        } else {
            println!("❌ VS_TEST failed to load");
        }

        if !ps.is_null() {
            let size = get_blob_size(ps);
            println!("✅ PS_TEST loaded: {} bytes", size);
        } else {
            println!("❌ PS_TEST failed to load");
        }
    }
}