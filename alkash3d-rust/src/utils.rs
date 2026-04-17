//! Вспомогательные утилиты

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::Win32::Graphics::Direct3D12::ID3D12DescriptorHeap;
use windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;
use windows_core::IUnknown;
use windows_core::Interface;

pub const DEBUG: bool = true;

#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::utils::DEBUG {
            eprintln!($($arg)*);
        }
    };
}

pub unsafe fn ptr_to_device(ptr: *mut c_void) -> Option<ID3D12Device> {
    if ptr.is_null() { return None; }
    Some(std::mem::transmute_copy(&ptr))
}

pub unsafe fn ptr_to_queue(ptr: *mut c_void) -> Option<ID3D12CommandQueue> {
    if ptr.is_null() { return None; }
    Some(std::mem::transmute_copy(&ptr))
}

pub unsafe fn ptr_to_swapchain(ptr: *mut c_void) -> Option<IDXGISwapChain3> {
    if ptr.is_null() { return None; }
    let ptr_val = ptr as usize;
    if ptr_val < 0x10000 { return None; }
    Some(std::mem::transmute_copy(&ptr))
}

pub unsafe fn ptr_to_resource(ptr: *mut c_void) -> Option<ID3D12Resource> {
    if ptr.is_null() { return None; }
    let ptr_val = ptr as usize;
    if ptr_val < 0x10000 { return None; }
    Some(std::mem::transmute_copy(&ptr))
}

#[allow(dead_code)]
pub unsafe fn ptr_to_heap(ptr: *mut c_void) -> Option<ID3D12DescriptorHeap> {
    if ptr.is_null() { return None; }
    Some(std::mem::transmute_copy(&ptr))
}

#[no_mangle]
pub extern "C" fn release_resource(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }

    unsafe {
        let _unknown: IUnknown = std::mem::transmute_copy(&ptr);
        true
    }
}

#[allow(dead_code)]
pub unsafe fn into_raw_interface<T: Interface + Clone>(obj: &T) -> *mut c_void {
    let cloned = obj.clone();
    let raw = cloned.as_raw();
    std::mem::forget(cloned);
    raw as *mut c_void
}

pub const BROKEN_HANDLES: [u64; 4] = [
    0x15678A00110000, 0x25678A00120000,
    0x35678A00130000, 0x45678A00140000,
];

pub fn is_gpu_handle_valid(handle: u64) -> bool {
    if handle == 0 { return false; }
    if handle < 0x10000 { return false; }
    !BROKEN_HANDLES.contains(&handle)
}