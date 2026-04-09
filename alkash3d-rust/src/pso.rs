//! Pipeline State Object

use std::ffi::c_void;
use crate::debug_println;

#[no_mangle]
pub extern "C" fn create_graphics_ps(
    _device_ptr: *mut c_void,
    _vs_blob_ptr: *mut c_void,
    _ps_blob_ptr: *mut c_void,
) -> *mut c_void {
    debug_println!("\n[create_graphics_ps] stub");
    std::ptr::null_mut()
}