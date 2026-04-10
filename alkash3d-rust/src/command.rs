//! Командные списки и синхронизация

use std::ffi::c_void;
use windows::Win32::{
    Foundation::CloseHandle,
    Graphics::Direct3D12::*,
    System::Threading::{CreateEventA, WaitForSingleObject, INFINITE},
};
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_command_allocators(device_ptr: *mut c_void, count: u32) -> bool {
    unsafe {
        debug_println!("\n[create_command_allocators] Creating {} allocators", count);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let mut state = STATE.lock().unwrap();
        state.command_allocators.clear();

        for i in 0..count {
            match device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) {
                Ok(allocator) => {
                    state.command_allocators.push(Some(allocator));
                    debug_println!("[create_command_allocators] Allocator {} created", i);
                }
                Err(e) => {
                    debug_println!("[create_command_allocators] Failed to create allocator {}: {:?}", i, e);
                    return false;
                }
            }
        }

        true
    }
}

#[no_mangle]
pub extern "C" fn create_command_list(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_command_list] Called");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let state = STATE.lock().unwrap();
        let allocator = match state.command_allocators.first() {
            Some(Some(a)) => a.clone(),
            _ => {
                debug_println!("[create_command_list] No allocator available");
                return std::ptr::null_mut();
            }
        };
        drop(state);

        // Правильная сигнатура: CreateCommandList(nodeMask, Type, pCommandAllocator, pInitialState, ppCommandList)
        // pInitialState ожидает Option<&ID3D12PipelineState>, передаём None
        match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,                                      // nodeMask
            D3D12_COMMAND_LIST_TYPE_DIRECT,        // Type
            &allocator,                            // pCommandAllocator
            None,                                  // pInitialState (нет начального PSO)
        ) {
            Ok(command_list) => {
                // Закрываем начальный список (он создаётся в открытом состоянии)
                let _ = command_list.Close();

                let mut state = STATE.lock().unwrap();
                state.command_list = Some(command_list.clone());
                state.command_list_open = false;

                let raw_ptr = command_list.as_raw();
                std::mem::forget(command_list);
                debug_println!("[create_command_list] Created at {:p}", raw_ptr);
                raw_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[create_command_list] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn create_fence(device_ptr: *mut c_void) -> bool {
    unsafe {
        debug_println!("\n[create_fence] Called");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        match device.CreateFence(0, D3D12_FENCE_FLAG_NONE) {
            Ok(fence) => {
                let mut state = STATE.lock().unwrap();
                state.fence = Some(fence);
                debug_println!("[create_fence] Fence created successfully");
                true
            }
            Err(e) => {
                debug_println!("[create_fence] Failed: {:?}", e);
                false
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn begin_frame() -> bool {
    debug_println!("\n[begin_frame] Called");

    let state = STATE.lock().unwrap();

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
        None => {
            debug_println!("[begin_frame] Allocator {} is None", idx);
            return false;
        }
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
        None => {
            debug_println!("[end_frame] No command queue");
            return false;
        }
    };

    let list = match state.command_list.clone() {
        Some(l) => l,
        None => {
            debug_println!("[end_frame] No command list");
            return false;
        }
    };

    let fence = match state.fence.clone() {
        Some(f) => f,
        None => {
            debug_println!("[end_frame] No fence");
            return false;
        }
    };

    let frame_idx = state.frame_index as usize;
    let fence_value = (frame_idx + 1) as u64;
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
        if let Err(e) = queue.Signal(&fence, fence_value) {
            debug_println!("[end_frame] Signal failed: {:?}", e);
            return false;
        }
    }

    {
        let mut state = STATE.lock().unwrap();
        state.command_list_open = false;
        if frame_idx < state.fence_values.len() {
            state.fence_values[frame_idx] = fence_value;
        }
        state.frame_index = (state.frame_index + 1) % 4;
    }

    debug_println!("[end_frame] Success (fence value: {})", fence_value);
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
    // Ждём предыдущий кадр
    let fence_value = if frame_idx == 0 {
        if state.fence_values.len() > 3 {
            state.fence_values[3]
        } else {
            0
        }
    } else {
        if frame_idx - 1 < state.fence_values.len() {
            state.fence_values[frame_idx - 1]
        } else {
            0
        }
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
            } else {
                debug_println!("[wait_for_gpu] Fence already completed: {} >= {}", completed, fence_value);
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