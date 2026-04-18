//! Управление swap chain

use std::ffi::c_void;
use windows::Win32::{
    Foundation::HWND,
    Graphics::{Dxgi::*, Dxgi::Common::*},
};
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_queue};

#[no_mangle]
pub extern "C" fn create_swap_chain(queue_ptr: *mut c_void, hwnd: usize, width: u32, height: u32) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_swap_chain] {}x{}, hwnd: 0x{:X}", width, height, hwnd);

        let queue = match ptr_to_queue(queue_ptr) {
            Some(q) => q,
            None => return std::ptr::null_mut(),
        };

        let factory: IDXGIFactory4 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(e) => {
                debug_println!("[create_swap_chain] Failed to create DXGI factory: {:?}", e);
                std::mem::forget(queue);
                return std::ptr::null_mut();
            }
        };

        let swap_desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
            Flags: 0,
        };

        let swap_chain1 = match factory.CreateSwapChainForHwnd(
            &queue,
            HWND(hwnd as *mut std::ffi::c_void),
            &swap_desc,
            None,
            None
        ) {
            Ok(sc) => sc,
            Err(e) => {
                debug_println!("[create_swap_chain] Failed to create swap chain: {:?}", e);
                std::mem::forget(queue);
                return std::ptr::null_mut();
            }
        };

        let swap_chain3: IDXGISwapChain3 = match swap_chain1.cast() {
            Ok(sc) => sc,
            Err(e) => {
                debug_println!("[create_swap_chain] Failed to cast to IDXGISwapChain3: {:?}", e);
                std::mem::forget(queue);
                return std::ptr::null_mut();
            }
        };

        {
            let mut state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => {
                    std::mem::forget(swap_chain3);
                    std::mem::forget(queue);
                    return std::ptr::null_mut();
                }
            };
            state.swap_chain = Some(swap_chain3.clone());
        }

        let raw_ptr = swap_chain3.as_raw();
        debug_println!("[create_swap_chain] ✅ Created at {:p}", raw_ptr);
        std::mem::forget(swap_chain3);
        std::mem::forget(queue);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) -> bool {
    unsafe {
        if swap_ptr.is_null() {
            debug_println!("[present_swap_chain] swap_ptr is null");
            return false;
        }

        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => {
                debug_println!("[present_swap_chain] Invalid swap chain pointer");
                return false;
            }
        };

        let flags = if sync_interval == 0 {
            DXGI_PRESENT_ALLOW_TEARING
        } else {
            DXGI_PRESENT(0)
        };

        let hr = swap.Present(sync_interval, flags);

        if hr.is_ok() {
            std::mem::forget(swap);
            true
        } else {
            debug_println!("[present_swap_chain] Present failed: {:?}", hr);
            std::mem::forget(swap);
            false
        }
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(swap_ptr: *mut c_void, buffer_index: u32) -> *mut c_void {
    unsafe {
        if swap_ptr.is_null() {
            debug_println!("[swap_chain_get_buffer] swap_ptr is null");
            return std::ptr::null_mut();
        }

        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => {
                debug_println!("[swap_chain_get_buffer] Invalid swap chain pointer");
                return std::ptr::null_mut();
            }
        };

        match swap.GetBuffer::<ID3D12Resource>(buffer_index) {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw();
                std::mem::forget(buffer);
                raw_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[swap_chain_get_buffer] Failed for index {}: {:?}", buffer_index, e);
                std::mem::forget(swap);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn resize_swap_chain(swap_ptr: *mut c_void, width: u32, height: u32) -> bool {
    unsafe {
        if swap_ptr.is_null() {
            return false;
        }

        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => return false,
        };

        let result = swap.ResizeBuffers(
            2,
            width,
            height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING
        ).is_ok();

        std::mem::forget(swap);

        if result {
            debug_println!("[resize_swap_chain] ✅ Resized to {}x{}", width, height);
        }
        result
    }
}

#[no_mangle]
pub extern "C" fn get_current_back_buffer_index(_swap_ptr: *mut c_void) -> u32 {
    // Игнорируем swap_ptr, используем глобальное состояние
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => {
            debug_println!("[get_current_back_buffer_index] Failed to lock state");
            return 0;
        }
    };

    unsafe {
        match state.swap_chain.as_ref() {
            Some(swap) => swap.GetCurrentBackBufferIndex(),
            None => {
                debug_println!("[get_current_back_buffer_index] No swap chain in state");
                0
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer_current(_swap_ptr: *mut c_void) -> *mut c_void {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let swap = match state.swap_chain.as_ref() {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let idx = unsafe { swap.GetCurrentBackBufferIndex() };

    unsafe {
        match swap.GetBuffer::<ID3D12Resource>(idx) {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw();
                std::mem::forget(buffer);
                raw_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[swap_chain_get_buffer_current] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}