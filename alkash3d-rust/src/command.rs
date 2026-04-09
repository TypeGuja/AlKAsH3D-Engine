//! Командные списки и синхронизация

use std::ffi::c_void;
use windows::Win32::{
    Foundation::CloseHandle,
    Graphics::Direct3D12::*,
    System::Threading::{CreateEventA, WaitForSingleObject, INFINITE},
};
use windows_core::Interface;
use crate::{STATE, debug_println};

#[no_mangle]
pub extern "C" fn begin_frame() -> bool {
    debug_println!("\n[begin_frame] Called");

    let state = STATE.lock().unwrap();

    // Проверяем наличие необходимых компонентов
    if state.command_queue.is_none() {
        debug_println!("[begin_frame] No command queue available");
        return false;
    }

    if state.command_allocators.is_empty() {
        debug_println!("[begin_frame] No command allocators");
        return false;
    }

    if state.command_list.is_none() {
        debug_println!("[begin_frame] No command list");
        return false;
    }

    let frame_idx = state.frame_index as usize;
    let idx = frame_idx % state.command_allocators.len();
    let allocator = match state.command_allocators[idx].clone() {
        Some(a) => a,
        None => return false,
    };

    let list = match state.command_list.clone() {
        Some(l) => l,
        None => return false,
    };

    drop(state);

    unsafe {
        // Сброс аллокатора
        if let Err(e) = allocator.Reset() {
            debug_println!("[begin_frame] Allocator reset failed: {:?}", e);
            return false;
        }

        // Сброс командного листа
        if let Err(e) = list.Reset(&allocator, None) {
            debug_println!("[begin_frame] Command list reset failed: {:?}", e);
            return false;
        }
    }

    {
        let mut state = STATE.lock().unwrap();
        state.command_list_open = true;
    }

    debug_println!("[begin_frame] Success");
    true
}

#[no_mangle]
pub extern "C" fn end_frame() -> bool {
    debug_println!("\n[end_frame] Called");

    let state = STATE.lock().unwrap();

    if !state.command_list_open {
        debug_println!("[end_frame] Command list not open");
        return false;
    }

    let queue = match state.command_queue.clone() {
        Some(q) => q,
        None => return false,
    };

    let list = match state.command_list.clone() {
        Some(l) => l,
        None => return false,
    };

    let fence = match state.fence.clone() {
        Some(f) => f,
        None => return false,
    };

    let frame_idx = state.frame_index as usize;
    drop(state);

    unsafe {
        // Закрываем командный лист
        if let Err(e) = list.Close() {
            debug_println!("[end_frame] Close failed: {:?}", e);
            return false;
        }

        // Кастуем к ID3D12CommandList
        let cmd_list: ID3D12CommandList = match list.cast() {
            Ok(l) => l,
            Err(e) => {
                debug_println!("[end_frame] Cast failed: {:?}", e);
                return false;
            }
        };

        // Выполняем команды
        queue.ExecuteCommandLists(&[Some(cmd_list)]);

        // Сигналим fence
        let fence_val = if frame_idx < 4 { frame_idx as u64 + 1 } else { 1 };

        if let Err(e) = queue.Signal(&fence, fence_val) {
            debug_println!("[end_frame] Signal failed: {:?}", e);
            return false;
        }
    }

    {
        let mut state = STATE.lock().unwrap();
        state.command_list_open = false;
        if frame_idx < state.fence_values.len() {
            state.fence_values[frame_idx] = frame_idx as u64 + 1;
        }
        state.frame_index = (state.frame_index + 1) % 4;
    }

    debug_println!("[end_frame] Success");
    true
}

#[no_mangle]
pub extern "C" fn wait_for_gpu() -> bool {
    debug_println!("\n[wait_for_gpu] Called");

    let state = STATE.lock().unwrap();
    let fence = match state.fence.clone() {
        Some(f) => f,
        None => {
            debug_println!("[wait_for_gpu] No fence");
            return false;
        }
    };

    let frame_idx = state.frame_index as usize;
    let fence_value = if frame_idx < state.fence_values.len() {
        state.fence_values[frame_idx]
    } else {
        0
    };
    drop(state);

    if fence_value > 0 {
        unsafe {
            let completed = fence.GetCompletedValue();
            if completed < fence_value {
                debug_println!("[wait_for_gpu] Waiting for fence {} < {}", completed, fence_value);

                let event = match CreateEventA(None, true, false, None) {
                    Ok(e) => e,
                    Err(_) => return false,
                };

                if let Err(_e) = fence.SetEventOnCompletion(fence_value, event) {
                    CloseHandle(event);
                    return false;
                }

                WaitForSingleObject(event, INFINITE);
                CloseHandle(event);
                debug_println!("[wait_for_gpu] Wait completed");
            }
        }
    }

    true
}

#[no_mangle]
pub extern "C" fn get_frame_index() -> u32 {
    STATE.lock().unwrap().frame_index
}

#[no_mangle]
pub extern "C" fn force_cleanup() {
    debug_println!("[force_cleanup] Called");

    let mut state = match STATE.try_lock() {
        Ok(s) => s,
        Err(_) => {
            debug_println!("[force_cleanup] Could not lock state, skipping");
            return;
        }
    };

    state.command_list_open = false;
    state.command_list = None;
    state.command_allocators.clear();
    state.swap_chain = None;
    state.command_queue = None;
    state.root_signature = None;
    state.fence = None;
    state.descriptor_heaps.clear();

    debug_println!("[force_cleanup] Done");
}

#[no_mangle]
pub extern "C" fn release_resource(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    debug_println!("[release_resource] Releasing resource at {:p}", ptr);
}