// src/swap_chain.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ (с правильными типами индексов)

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
    fn IsWindow(hwnd: HWND) -> i32;
}

// Храним back buffer'ы для каждого кадра
static mut BACK_BUFFERS: [*mut c_void; 2] = [ptr::null_mut(), ptr::null_mut()];
static mut SWAP_CHAIN_STORED: Option<IDXGISwapChain3> = None;

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

                // Сохраняем swap chain глобально
                SWAP_CHAIN_STORED = Some(swap_chain3.clone());

                // Получаем оба back buffer'а и сохраняем их указатели
                for i in 0..2 {
                    match swap_chain3.GetBuffer::<ID3D12Resource>(i) {
                        Ok(buffer) => {
                            let raw_ptr = buffer.as_raw() as *mut c_void;
                            BACK_BUFFERS[i as usize] = raw_ptr;
                            debug_println!("[create_swap_chain] Back buffer {} stored at {:p}", i, raw_ptr);
                            // Не забываем освободить COM объект, который мы получили
                            std::mem::forget(buffer);
                        }
                        Err(e) => {
                            debug_println!("Failed to get back buffer {}: {:?}", i, e);
                        }
                    }
                }

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
pub extern "C" fn get_current_back_buffer_resource() -> *mut c_void {
    unsafe {
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        let swap_chain = match state.swap_chain.as_ref() {
            Some(sc) => sc,
            None => return std::ptr::null_mut(),
        };

        let idx = swap_chain.GetCurrentBackBufferIndex() as usize;

        if idx < 2 {
            let ptr = BACK_BUFFERS[idx];
            debug_println!("[get_current_back_buffer_resource] Returning buffer {} at {:p}", idx, ptr);
            ptr
        } else {
            debug_println!("[get_current_back_buffer_resource] Invalid index: {}", idx);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn get_back_buffer() -> *mut c_void {
    get_current_back_buffer_resource()
}

#[no_mangle]
pub extern "C" fn destroy_swap_chain(swap_ptr: *mut c_void) -> bool {
    unsafe {
        if !swap_ptr.is_null() {
            let _swap = IDXGISwapChain3::from_raw(swap_ptr as *mut _);
            // Объект автоматически освободится через Drop
        }
        for i in 0..2 {
            BACK_BUFFERS[i] = ptr::null_mut();
        }
        SWAP_CHAIN_STORED = None;
    }
    debug_println!("[destroy_swap_chain] ✅ Swap chain cleaned");
    true
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) -> bool {
    unsafe {
        if swap_ptr.is_null() {
            debug_println!("[present_swap_chain] swap_ptr is null");
            return false;
        }

        let swap_chain = IDXGISwapChain3::from_raw(swap_ptr as *mut _);

        let flags = if sync_interval == 0 {
            DXGI_PRESENT_ALLOW_TEARING
        } else {
            DXGI_PRESENT(0)
        };

        let result = swap_chain.Present(sync_interval, flags).is_ok();
        debug_println!("[present_swap_chain] Present result: {}", result);

        // После презентации обновляем указатель на текущий back buffer
        if result {
            let new_idx = swap_chain.GetCurrentBackBufferIndex() as usize;
            if new_idx < 2 && BACK_BUFFERS[new_idx].is_null() {
                // Если почему-то указатель не сохранён, получаем заново
                match swap_chain.GetBuffer::<ID3D12Resource>(new_idx as u32) {
                    Ok(buffer) => {
                        BACK_BUFFERS[new_idx] = buffer.as_raw() as *mut c_void;
                        std::mem::forget(buffer);
                        debug_println!("[present_swap_chain] Updated back buffer {} at {:p}", new_idx, BACK_BUFFERS[new_idx]);
                    }
                    Err(e) => {
                        debug_println!("[present_swap_chain] Failed to get back buffer {}: {:?}", new_idx, e);
                    }
                }
            }
        }

        // Не забываем, что мы создали новый COM объект - нужно его освободить
        std::mem::forget(swap_chain);

        result
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(swap_ptr: *mut c_void, buffer_index: u32) -> *mut c_void {
    unsafe {
        let idx = buffer_index as usize;
        if idx < 2 && !BACK_BUFFERS[idx].is_null() {
            debug_println!("[swap_chain_get_buffer] Returning cached buffer {} at {:p}", idx, BACK_BUFFERS[idx]);
            return BACK_BUFFERS[idx];
        }

        // Fallback - получаем из swap chain
        let swap_chain = if !swap_ptr.is_null() {
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

        match swap_chain.GetBuffer::<ID3D12Resource>(buffer_index) {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw() as *mut c_void;
                if idx < 2 {
                    BACK_BUFFERS[idx] = raw_ptr;
                }
                std::mem::forget(buffer);
                debug_println!("[swap_chain_get_buffer] Returning buffer {} at {:p}", buffer_index, raw_ptr);
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
        // Очищаем кэш back buffer'ов
        for i in 0..2 {
            BACK_BUFFERS[i] = ptr::null_mut();
        }

        let swap_chain = if !swap_ptr.is_null() {
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

        let result = swap_chain.ResizeBuffers(
            2,
            width,
            height,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING
        ).is_ok();

        // После ресайза получаем новые back buffer'ы
        if result {
            for i in 0..2 {
                match swap_chain.GetBuffer::<ID3D12Resource>(i) {
                    Ok(buffer) => {
                        let idx = i as usize;
                        BACK_BUFFERS[idx] = buffer.as_raw() as *mut c_void;
                        std::mem::forget(buffer);
                        debug_println!("[resize_swap_chain] Back buffer {} stored at {:p}", i, BACK_BUFFERS[idx]);
                    }
                    Err(e) => {
                        debug_println!("[resize_swap_chain] Failed to get back buffer {}: {:?}", i, e);
                    }
                }
            }
        }

        result
    }
}

#[no_mangle]
pub extern "C" fn get_current_back_buffer_index(swap_ptr: *mut c_void) -> u32 {
    unsafe {
        let swap_chain = if !swap_ptr.is_null() {
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
        let idx = swap_chain.GetCurrentBackBufferIndex();
        std::mem::forget(swap_chain);
        idx
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer_current(swap_ptr: *mut c_void) -> *mut c_void {
    let idx = get_current_back_buffer_index(swap_ptr);
    swap_chain_get_buffer(swap_ptr, idx)
}

#[no_mangle]
pub extern "C" fn get_swap_chain(swap_ptr: *mut c_void) -> *mut c_void {
    if !swap_ptr.is_null() {
        return swap_ptr;
    }
    {
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