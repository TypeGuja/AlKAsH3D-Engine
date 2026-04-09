//! Функции рендеринга

use std::ffi::c_void;
use windows::Win32::{
    Foundation::RECT,
    Graphics::{Direct3D12::*, Dxgi::Common::*},
};
use windows_core::Interface;
use crate::{STATE, debug_println};

#[no_mangle]
pub extern "C" fn set_graphics_pipeline(_pso_ptr: *mut c_void) -> bool {
    debug_println!("[set_graphics_pipeline] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_root_descriptor_table(_root_index: u32, _gpu_handle: u64) -> bool {
    debug_println!("[set_root_descriptor_table] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_descriptor_heaps(_count: usize, _heaps: *const *mut c_void) -> bool {
    debug_println!("[set_descriptor_heaps] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_render_target(_rtv: u64) -> bool {
    debug_println!("[set_render_target] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_render_targets(_count: usize, _rtvs: *const u64) -> bool {
    debug_println!("[set_render_targets] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_viewport(_x: i32, _y: i32, _w: i32, _h: i32, _min_d: f32, _max_d: f32) -> bool {
    debug_println!("[set_viewport] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_scissor_rect(_l: i32, _t: i32, _r: i32, _b: i32) -> bool {
    debug_println!("[set_scissor_rect] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_vertex_buffers(_vb: *mut c_void, _ib: *mut c_void) -> bool {
    debug_println!("[set_vertex_buffers] stub");
    true
}

#[no_mangle]
pub extern "C" fn draw_instanced(_vc: u32, _ic: u32, _sv: u32, _si: u32) -> bool {
    debug_println!("[draw_instanced] stub");
    true
}

#[no_mangle]
pub extern "C" fn draw_indexed_instanced(_ic: u32, _inst: u32, _si: u32, _bv: i32, _si2: u32) -> bool {
    debug_println!("[draw_indexed_instanced] stub");
    true
}

#[no_mangle]
pub extern "C" fn clear_render_target(_rtv: u64, _color: *const f32) -> bool {
    debug_println!("[clear_render_target] stub");
    true
}

#[no_mangle]
pub extern "C" fn create_shader_resource_view(_device_ptr: *mut c_void, _resource_ptr: *mut c_void, _cpu_handle: u64) -> bool {
    debug_println!("[create_shader_resource_view] stub");
    true
}

#[no_mangle]
pub extern "C" fn create_render_target_view(_device_ptr: *mut c_void, _resource_ptr: *mut c_void, _cpu_handle: u64) -> bool {
    debug_println!("[create_render_target_view] stub");
    true
}

#[no_mangle]
pub extern "C" fn create_constant_buffer_view(_device_ptr: *mut c_void, _resource_ptr: *mut c_void, _cpu_handle: u64) -> bool {
    debug_println!("[create_constant_buffer_view] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_vsync(_enable: bool) {
    debug_println!("[set_vsync] stub");
}