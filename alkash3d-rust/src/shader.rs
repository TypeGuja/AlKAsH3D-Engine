//! Шейдеры

use std::ffi::c_void;
use crate::debug_println;

#[no_mangle]
pub extern "C" fn compile_shader(
    _file_path: *const u16,
    _entry_point: *const i8,
    _profile: *const i8,
    out_blob: *mut *mut c_void,
) -> i32 {
    debug_println!("\n[compile_shader] Warning: Shader compilation not yet implemented");
    debug_println!("  Please use pre-compiled shaders (cso files)");

    if out_blob.is_null() {
        return -1;
    }

    unsafe {
        *out_blob = std::ptr::null_mut();
    }

    -1
}

#[no_mangle]
pub extern "C" fn load_precompiled_shader(
    data_ptr: *const u8,
    data_size: usize,
    out_blob: *mut *mut c_void,
) -> i32 {
    debug_println!("\n[load_precompiled_shader] Loading precompiled shader (size: {})", data_size);

    if data_ptr.is_null() || out_blob.is_null() || data_size == 0 {
        return -1;
    }

    unsafe {
        // Создаём имитацию blob (просто указатель на данные)
        // В реальном коде здесь нужно создать ID3DBlob
        *out_blob = data_ptr as *mut c_void;
    }

    0
}

#[no_mangle]
pub extern "C" fn get_shader_data(shader_blob: *mut c_void, out_data: *mut *mut c_void, out_size: *mut usize) -> bool {
    unsafe {
        if shader_blob.is_null() || out_data.is_null() || out_size.is_null() {
            return false;
        }

        // В реальном коде здесь нужно получить данные из ID3DBlob
        *out_data = shader_blob;
        *out_size = 0;
        true
    }
}