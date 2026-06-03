// src/swap_chain.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ (без forget)

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

#[link(name = "user32")]
extern "system" {
    fn IsWindow(hWnd: HWND) -> i32;
}

// Глобальное хранение back buffer
static mut BACK_BUFFER: *mut c_void = std::ptr::null_mut();

#[no_mangle]
pub extern "C" fn create_swap_chain(queue_ptr: *mut c_void, hwnd_ptr: *mut c_void, width: u32, height: u32) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_swap_chain] {}x{}, hwnd_ptr: {:p}", width, height, hwnd_ptr);

        if hwnd_ptr.is_null() {
            debug_println!("HWND pointer is null!");
            return ptr::null_mut();
        }

        let hwnd = HWND(hwnd_ptr);
        debug_println!("HWND: {:?}", hwnd);

        if IsWindow(hwnd) == 0 {
            debug_println!("HWND is not a valid window!");
            return ptr::null_mut();
        }

        let queue = if queue_ptr.is_null() {
            debug_println!("[create_swap_chain] queue_ptr is null, taking from STATE");
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            match state.command_queue.as_ref() {
                Some(q) => q.clone(),
                None => {
                    debug_println!("[create_swap_chain] No command queue in STATE!");
                    return ptr::null_mut();
                }
            }
        } else {
            debug_println!("[create_swap_chain] Using queue from pointer: {:p}", queue_ptr);
            ID3D12CommandQueue::from_raw(queue_ptr as *mut _)
        };

        debug_println!("[create_swap_chain] Queue obtained");

        let factory: IDXGIFactory2 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(e) => {
                debug_println!("Failed to create factory: {:?}", e);
                return ptr::null_mut();
            }
        };
        debug_println!("Factory created");

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
        debug_println!("Swap descriptor created");

        debug_println!("Calling CreateSwapChainForHwnd...");

        match factory.CreateSwapChainForHwnd(&queue, hwnd, &desc, None, None) {
            Ok(swap_chain) => {
                debug_println!("Swap chain created successfully!");

                let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);

                let swap_chain3: IDXGISwapChain3 = match swap_chain.cast() {
                    Ok(sc) => {
                        debug_println!("Cast to IDXGISwapChain3 successful");
                        sc
                    }
                    Err(e) => {
                        debug_println!("Failed to cast: {:?}", e);
                        return ptr::null_mut();
                    }
                };

                // Сохраняем swap chain в STATE
                {
                    let mut state = match STATE.lock() {
                        Ok(s) => s,
                        Err(_) => return ptr::null_mut(),
                    };
                    state.swap_chain = Some(swap_chain3.clone());
                    state.frame_index = 0;
                }

                // Получаем back buffer - НЕ ИСПОЛЬЗУЕМ forget!
                match swap_chain3.GetBuffer::<ID3D12Resource>(0) {
                    Ok(buffer) => {
                        BACK_BUFFER = buffer.as_raw() as *mut c_void;
                        debug_println!("[create_swap_chain] Back buffer: {:p}", BACK_BUFFER);
                        // НЕ вызываем forget! buffer умрёт когда умрёт swap chain
                    }
                    Err(e) => {
                        debug_println!("Failed to get back buffer: {:?}", e);
                        return ptr::null_mut();
                    }
                };

                let raw_ptr = swap_chain3.as_raw() as *mut c_void;
                debug_println!("[create_swap_chain] ✅ Success, returning {:p}", raw_ptr);
                raw_ptr
            }
            Err(e) => {
                debug_println!("Failed to create swap chain: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn get_back_buffer() -> *mut c_void {
    unsafe {
        if BACK_BUFFER.is_null() {
            debug_println!("[get_back_buffer] Back buffer is null!");
            return ptr::null_mut();
        }
        debug_println!("[get_back_buffer] Returning {:p}", BACK_BUFFER);
        BACK_BUFFER
    }
}

#[no_mangle]
pub extern "C" fn destroy_swap_chain(_swap_ptr: *mut c_void) -> bool {
    unsafe {
        BACK_BUFFER = std::ptr::null_mut();
    }
    debug_println!("[destroy_swap_chain] Swap chain will be cleaned by STATE");
    true
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) -> bool {
    unsafe {
        let swap_chain_clone = if !swap_ptr.is_null() {
            IDXGISwapChain3::from_raw(swap_ptr as *mut _)
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

        let result = swap_chain_clone.Present(sync_interval, flags).is_ok();

        // После презентации обновляем back buffer
        if result {
            let idx = swap_chain_clone.GetCurrentBackBufferIndex();
            match swap_chain_clone.GetBuffer::<ID3D12Resource>(idx) {
                Ok(buffer) => {
                    BACK_BUFFER = buffer.as_raw() as *mut c_void;
                    // НЕ вызываем forget!
                }
                Err(_) => {}
            }
        }

        result
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(swap_ptr: *mut c_void, buffer_index: u32) -> *mut c_void {
    unsafe {
        let swap_chain_clone = if !swap_ptr.is_null() {
            IDXGISwapChain3::from_raw(swap_ptr as *mut _)
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

        match swap_chain_clone.GetBuffer::<ID3D12Resource>(buffer_index) {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw() as *mut c_void;
                raw_ptr
            }
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
        let swap_chain_clone = if !swap_ptr.is_null() {
            IDXGISwapChain3::from_raw(swap_ptr as *mut _)
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

        swap_chain_clone.ResizeBuffers(
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
        let swap_chain_clone = if !swap_ptr.is_null() {
            IDXGISwapChain3::from_raw(swap_ptr as *mut _)
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
        swap_chain_clone.GetCurrentBackBufferIndex()
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer_current(swap_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        let swap_chain_clone = if !swap_ptr.is_null() {
            IDXGISwapChain3::from_raw(swap_ptr as *mut _)
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

        let idx = swap_chain_clone.GetCurrentBackBufferIndex();
        match swap_chain_clone.GetBuffer::<ID3D12Resource>(idx) {
            Ok(buffer) => {
                buffer.as_raw() as *mut c_void
            }
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