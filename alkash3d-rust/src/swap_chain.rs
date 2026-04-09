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
        debug_println!("\n[create_swap_chain] {}x{}", width, height);

        let queue = match ptr_to_queue(queue_ptr) {
            Some(q) => q,
            None => return std::ptr::null_mut(),
        };

        let factory: IDXGIFactory4 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(_) => return std::ptr::null_mut(),
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

        let swap_chain1 = match factory.CreateSwapChainForHwnd(&queue, HWND(hwnd as *mut std::ffi::c_void), &swap_desc, None, None) {
            Ok(sc) => sc,
            Err(_) => return std::ptr::null_mut(),
        };

        let swap_chain3: IDXGISwapChain3 = match swap_chain1.cast() {
            Ok(sc) => sc,
            Err(_) => return std::ptr::null_mut(),
        };

        {
            let mut state = STATE.lock().unwrap();
            state.swap_chain = Some(swap_chain3.clone());
            state.frame_index = swap_chain3.GetCurrentBackBufferIndex();
        }

        let raw_ptr = swap_chain3.as_raw();
        std::mem::forget(swap_chain3);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) -> bool {
    unsafe {
        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => return false,
        };

        let result = swap.Present(sync_interval, DXGI_PRESENT(0)).is_ok();

        if result {
            let mut state = STATE.lock().unwrap();
            state.frame_index = swap.GetCurrentBackBufferIndex();
        }

        std::mem::forget(swap);
        result
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(swap_ptr: *mut c_void, buffer_index: u32) -> *mut c_void {
    unsafe {
        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        match swap.GetBuffer::<ID3D12Resource>(buffer_index) {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw();
                std::mem::forget(buffer);
                raw_ptr as *mut c_void
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}

#[no_mangle]
pub extern "C" fn resize_swap_chain(swap_ptr: *mut c_void, width: u32, height: u32) -> bool {
    unsafe {
        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => return false,
        };

        let result = swap.ResizeBuffers(2, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SWAP_CHAIN_FLAG(0)).is_ok();
        std::mem::forget(swap);
        result
    }
}