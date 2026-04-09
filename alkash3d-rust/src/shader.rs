//! Шейдеры

use std::ffi::c_void;
use crate::debug_println;

#[no_mangle]
pub extern "C" fn compile_shader(
    _file_path: *const u16,
    _entry_point: *const u8,
    _profile: *const u8,
    out_blob: *mut *mut c_void,
) -> i32 {
    debug_println!("\n[compile_shader] stub");

    if out_blob.is_null() {
        return -1;
    }

    unsafe {
        *out_blob = std::ptr::null_mut();
    }

    0
}