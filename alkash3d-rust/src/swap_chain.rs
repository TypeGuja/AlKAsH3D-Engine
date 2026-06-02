// src/swap_chain.rs
//! Управление swap chain

use std::ffi::c_void;
use std::ptr;
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dxgi::*,
    Graphics::Dxgi::Common::*,
};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Resource};
use windows_core::Interface;
use crate::{STATE, debug_println};

#[no_mangle]
pub extern "C" fn create_swap_chain(queue_ptr: *mut c_void, hwnd_ptr: *mut c_void, width: u32, height: u32) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_swap_chain] {}x{}, hwnd: {:p}", width, height, hwnd_ptr);

        if queue_ptr.is_null() {
            debug_println!("Queue pointer is null!");
            return ptr::null_mut();
        }

        if hwnd_ptr.is_null() {
            debug_println!("HWND is null!");
            return ptr::null_mut();
        }

        let queue = &*(queue_ptr as *const ID3D12CommandQueue);

        let factory: IDXGIFactory4 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(e) => {
                debug_println!("Failed to create factory: {:?}", e);
                return ptr::null_mut();
            }
        };

        let desc = DXGI_SWAP_CHAIN_DESC1 {
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
            Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32,
        };

        let hwnd_obj = HWND(hwnd_ptr);
        debug_println!("[create_swap_chain] HWND object: {:?}", hwnd_obj);

        let swap_chain = match factory.CreateSwapChainForHwnd(queue, hwnd_obj, &desc, None, None) {
            Ok(sc) => sc,
            Err(e) => {
                debug_println!("Failed to create swap chain: {:?}", e);
                debug_println!("Error code: 0x{:08X}", e.code().0);
                return ptr::null_mut();
            }
        };

        debug_println!("Swap chain created successfully!");

        let _ = factory.MakeWindowAssociation(hwnd_obj, DXGI_MWA_NO_ALT_ENTER);

        let swap_chain3: IDXGISwapChain3 = match swap_chain.cast() {
            Ok(sc) => sc,
            Err(e) => {
                debug_println!("Failed to cast: {:?}", e);
                return ptr::null_mut();
            }
        };

        {
            let mut state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            state.swap_chain = Some(swap_chain3.clone());
            state.frame_index = 0;
        }

        swap_chain3.as_raw() as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn destroy_swap_chain(_swap_ptr: *mut c_void) -> bool {
    debug_println!("[destroy_swap_chain] Swap chain will be cleaned by STATE");
    true
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) -> bool {
    unsafe {
        let swap_chain = if !swap_ptr.is_null() {
            (&*(swap_ptr as *const IDXGISwapChain3)).clone()
        } else {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return false,
            };
            match state.swap_chain.as_ref() {
                Some(s) => s.clone(),
                None => return false,
            }
        };

        let flags = if sync_interval == 0 {
            DXGI_PRESENT_ALLOW_TEARING
        } else {
            DXGI_PRESENT(0)
        };

        swap_chain.Present(sync_interval, flags).is_ok()
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(swap_ptr: *mut c_void, buffer_index: u32) -> *mut c_void {
    unsafe {
        let swap_chain = if !swap_ptr.is_null() {
            (&*(swap_ptr as *const IDXGISwapChain3)).clone()
        } else {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            match state.swap_chain.as_ref() {
                Some(s) => s.clone(),
                None => return ptr::null_mut(),
            }
        };

        match swap_chain.GetBuffer::<ID3D12Resource>(buffer_index) {
            Ok(buffer) => buffer.as_raw() as *mut c_void,
            Err(e) => {
                debug_println!("[swap_chain_get_buffer] Failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn resize_swap_chain(swap_ptr: *mut c_void, width: u32, height: u32) -> bool {
    unsafe {
        let swap_chain = if !swap_ptr.is_null() {
            (&*(swap_ptr as *const IDXGISwapChain3)).clone()
        } else {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return false,
            };
            match state.swap_chain.as_ref() {
                Some(s) => s.clone(),
                None => return false,
            }
        };

        swap_chain.ResizeBuffers(
            2,
            width,
            height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING
        ).is_ok()
    }
}

#[no_mangle]
pub extern "C" fn get_current_back_buffer_index(swap_ptr: *mut c_void) -> u32 {
    unsafe {
        let swap_chain = if !swap_ptr.is_null() {
            (&*(swap_ptr as *const IDXGISwapChain3)).clone()
        } else {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return 0,
            };
            match state.swap_chain.as_ref() {
                Some(s) => s.clone(),
                None => return 0,
            }
        };
        swap_chain.GetCurrentBackBufferIndex()
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer_current(swap_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        let swap_chain = if !swap_ptr.is_null() {
            (&*(swap_ptr as *const IDXGISwapChain3)).clone()
        } else {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            match state.swap_chain.as_ref() {
                Some(s) => s.clone(),
                None => return ptr::null_mut(),
            }
        };

        let idx = swap_chain.GetCurrentBackBufferIndex();
        match swap_chain.GetBuffer::<ID3D12Resource>(idx) {
            Ok(buffer) => buffer.as_raw() as *mut c_void,
            Err(e) => {
                debug_println!("[swap_chain_get_buffer_current] Failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn get_swap_chain(swap_ptr: *mut c_void) -> *mut c_void {
    if !swap_ptr.is_null() {
        return swap_ptr;
    }
    unsafe {
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        };
        match state.swap_chain.as_ref() {
            Some(s) => s.as_raw() as *mut c_void,
            None => ptr::null_mut(),
        }
    }
}