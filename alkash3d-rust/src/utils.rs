//! Вспомогательные утилиты

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;
use windows_core::Interface;

pub const DEBUG: bool = cfg!(debug_assertions);

#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::utils::DEBUG {
            eprintln!($($arg)*);
        }
    };
}

pub unsafe fn ptr_to_device(ptr: *mut c_void) -> Option<ID3D12Device> {
    if ptr.is_null() {
        return None;
    }
    Some(std::mem::transmute_copy(&ptr))
}

pub unsafe fn ptr_to_queue(ptr: *mut c_void) -> Option<ID3D12CommandQueue> {
    if ptr.is_null() {
        return None;
    }
    Some(std::mem::transmute_copy(&ptr))
}

pub unsafe fn ptr_to_swapchain(ptr: *mut c_void) -> Option<IDXGISwapChain3> {
    if ptr.is_null() {
        return None;
    }
    Some(std::mem::transmute_copy(&ptr))
}

pub unsafe fn ptr_to_resource(ptr: *mut c_void) -> Option<ID3D12Resource> {
    if ptr.is_null() {
        return None;
    }
    Some(std::mem::transmute_copy(&ptr))
}

#[no_mangle]
pub extern "C" fn release_resource(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }

    unsafe {
        let _unknown: windows_core::IUnknown = std::mem::transmute_copy(&ptr);
        true
    }
}

#[no_mangle]
pub extern "C" fn get_debug_mode() -> bool {
    DEBUG
}

#[no_mangle]
pub extern "C" fn enable_debug_layer() -> bool {
    #[cfg(debug_assertions)]
    {
        unsafe {
            use windows::Win32::Graphics::Direct3D12::*;
            let mut debug: Option<ID3D12Debug> = None;
            if D3D12GetDebugInterface(&mut debug).is_ok() {
                if let Some(d) = debug {
                    d.EnableDebugLayer();
                    return true;
                }
            }
        }
    }
    false
}

// Функции для безопасного освобождения COM объектов
#[no_mangle]
pub extern "C" fn release_com_object(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        return false;
    }
    unsafe {
        let _ = Box::from_raw(ptr);
        true
    }
}

// Получить raw указатель из Box
pub fn com_ptr_to_raw<T>(obj: T) -> *mut c_void {
    Box::into_raw(Box::new(obj)) as *mut c_void
}

// Восстановить Box из raw указателя
pub unsafe fn raw_to_com_ptr<T>(ptr: *mut c_void) -> Box<T> {
    Box::from_raw(ptr as *mut T)
}