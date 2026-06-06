// src/command.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ (убрана ошибка типа)

use std::ffi::c_void;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Graphics::Direct3D12::*,
    System::Threading::{CreateEventA, WaitForSingleObject, INFINITE},
};
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_device};

thread_local! {
    static FENCE_EVENT: RefCell<Option<HANDLE>> = RefCell::new(None);
}

const ALLOCATOR_POOL_SIZE: usize = 16;
static FRAME_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn with_command_list<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&ID3D12GraphicsCommandList) -> R,
{
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(e) => {
            debug_println!("[with_command_list] Failed to lock state: {:?}", e);
            return None;
        }
    };

    if !state.command_list_open {
        return None;
    }

    let list = match state.command_list.as_ref() {
        Some(l) => l,
        None => {
            debug_println!("[with_command_list] command_list is None but open=true");
            return None;
        }
    };

    Some(f(list))
}

#[no_mangle]
pub extern "C" fn create_fence(device_ptr: *mut c_void) -> bool {
    unsafe {
        debug_println!("\n[create_fence] Creating...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
            Ok(fence) => {
                {
                    let mut state = match STATE.lock() {
                        Ok(s) => s,
                        Err(_) => return false,
                    };
                    state.fence = Some(fence);
                    state.fence_values = vec![0; ALLOCATOR_POOL_SIZE];
                }

                match CreateEventA(None, false, false, None) {
                    Ok(event) => {
                        FENCE_EVENT.with(|cell| {
                            *cell.borrow_mut() = Some(event);
                        });
                    }
                    Err(_) => {}
                }

                debug_println!("[create_fence] ✅ Created");
                true
            }
            Err(e) => {
                debug_println!("[create_fence] Failed: {:?}", e);
                false
            }
        }
    }
}

unsafe fn wait_for_allocator(allocator_idx: usize) {
    let (fence, fence_value, event) = {
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let fence = match state.fence.as_ref() {
            Some(f) => f.clone(),
            None => return,
        };

        let fence_val = if allocator_idx < state.fence_values.len() {
            state.fence_values[allocator_idx]
        } else {
            0
        };

        (fence, fence_val, FENCE_EVENT.with(|cell| cell.borrow().clone()))
    };

    if fence_value == 0 {
        return;
    }

    let completed = fence.GetCompletedValue();
    if completed < fence_value {
        debug_println!("[wait] Allocator {} waiting for fence {}", allocator_idx, fence_value);
        if let Some(event) = event {
            let _ = fence.SetEventOnCompletion(fence_value, event);
            WaitForSingleObject(event, INFINITE);
        } else {
            while fence.GetCompletedValue() < fence_value {
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
        }
        debug_println!("[wait] Allocator {} ready", allocator_idx);
    }
}

#[no_mangle]
pub extern "C" fn begin_frame_ex(pso_ptr: *mut c_void) -> *mut c_void {
    let frame_id = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);
    let allocator_idx = 0;

    println!("\n[begin_frame] Frame {} (allocator {})", frame_id, allocator_idx);

    unsafe {
        if frame_id > 0 {
            println!("[begin_frame] Waiting for allocator...");
            wait_for_allocator(allocator_idx);
            println!("[begin_frame] Wait done");
        }

        println!("[begin_frame] Resetting state...");
        {
            let mut state = STATE.lock().unwrap();
            state.command_list_open = false;
            state.command_list = None;
        }
        println!("[begin_frame] State reset");

        let (device, allocator) = {
            println!("[begin_frame] Locking state for device/allocator...");
            let mut state = STATE.lock().unwrap();

            let device = state.device.as_ref().unwrap().clone();
            println!("[begin_frame] Device obtained");

            if state.command_allocators.is_empty() {
                state.command_allocators.push(None);
            }

            let allocator = if let Some(ref a) = state.command_allocators[0] {
                println!("[begin_frame] Using existing allocator");
                a.clone()
            } else {
                println!("[begin_frame] Creating new allocator...");
                match device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) {
                    Ok(a) => {
                        println!("[begin_frame] Allocator created");
                        state.command_allocators[0] = Some(a.clone());
                        a
                    }
                    Err(e) => {
                        println!("[begin_frame] Failed to create allocator: {:?}", e);
                        return std::ptr::null_mut();
                    }
                }
            };

            println!("[begin_frame] Allocator ready");
            (device, allocator)
        };

        println!("[begin_frame] Resetting allocator...");
        if let Err(e) = allocator.Reset() {
            println!("[begin_frame] Allocator reset failed: {:?}", e);
            return std::ptr::null_mut();
        }
        println!("[begin_frame] Allocator reset OK");

        println!("[begin_frame] Creating command list...");
        let list = match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None,
        ) {
            Ok(l) => {
                println!("[begin_frame] Command list created successfully");
                l
            },
            Err(e) => {
                println!("[begin_frame] CreateCommandList failed: {:?}", e);
                return std::ptr::null_mut();
            }
        };

        let list_ptr = list.as_raw();
        println!("[begin_frame] Command list raw ptr: {:p}", list_ptr);

        {
            println!("[begin_frame] Saving to state...");
            let mut state = STATE.lock().unwrap();
            state.command_list = Some(list);
            state.command_list_open = true;
            state.reset_bindings();
            // НЕ СОХРАНЯЕМ PSO ЗДЕСЬ!
            println!("[begin_frame] State saved");
        }

        println!("[begin_frame] Returning {:p}", list_ptr);
        list_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn begin_frame() -> *mut c_void {
    begin_frame_ex(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn end_frame() -> bool {
    let frame_id = FRAME_COUNTER.load(Ordering::Relaxed);
    if frame_id == 0 {
        return false;
    }

    let (queue, list, fence, fence_val) = {
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(e) => {
                debug_println!("[end_frame] Failed to lock state: {:?}", e);
                return false;
            }
        };

        if !state.command_list_open {
            debug_println!("[end_frame] Command list not open");
            return false;
        }

        let queue = match state.command_queue.as_ref() {
            Some(q) => q.clone(),
            None => {
                debug_println!("[end_frame] No command queue!");
                return false;
            }
        };

        let list = match state.command_list.take() {
            Some(l) => l,
            None => {
                debug_println!("[end_frame] No command list!");
                return false;
            }
        };

        let fence = match state.fence.as_ref() {
            Some(f) => f.clone(),
            None => {
                debug_println!("[end_frame] No fence!");
                return false;
            }
        };

        state.command_list_open = false;

        let val = state.fence_values[0] + 1;
        state.fence_values[0] = val;

        (queue, list, fence, val)
    };

    unsafe {
        // ЗАКРЫВАЕМ command list
        if let Err(e) = list.Close() {
            debug_println!("[end_frame] Close failed: {:?}", e);
            return false;
        }

        // ВЫПОЛНЯЕМ command list на GPU
        let cmd_list = ID3D12CommandList::from_raw(list.as_raw());
        let cmd_lists = [Some(cmd_list.clone())];
        queue.ExecuteCommandLists(&cmd_lists);
        std::mem::forget(cmd_list);

        // СИГНАЛИМ fence
        if let Err(e) = queue.Signal(&fence, fence_val) {
            debug_println!("[end_frame] Signal failed: {:?}", e);
            return false;
        }
    }

    true
}

#[no_mangle]
pub extern "C" fn wait_for_gpu() -> bool {
    unsafe {
        debug_println!("[wait_for_gpu] Waiting for all frames...");

        let fence_values = {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return false,
            };
            state.fence_values.clone()
        };

        for i in 0..ALLOCATOR_POOL_SIZE {
            if i < fence_values.len() && fence_values[i] > 0 {
                wait_for_allocator(i);
            }
        }

        debug_println!("[wait_for_gpu] ✅ All frames complete");
        true
    }
}

#[no_mangle]
pub extern "C" fn wait_for_frame(frame_id: u64) -> bool {
    unsafe {
        let allocator_idx = (frame_id % ALLOCATOR_POOL_SIZE as u64) as usize;

        let (fence, fence_value) = {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return false,
            };

            let fence = match state.fence.as_ref() {
                Some(f) => f.clone(),
                None => return false,
            };

            let fence_val = if allocator_idx < state.fence_values.len() {
                state.fence_values[allocator_idx]
            } else {
                0
            };

            (fence, fence_val)
        };

        if fence_value == 0 {
            return true;
        }

        let event = FENCE_EVENT.with(|cell| cell.borrow().clone());

        let completed = fence.GetCompletedValue();
        if completed < fence_value {
            if let Some(event) = event {
                let _ = fence.SetEventOnCompletion(fence_value, event);
                WaitForSingleObject(event, INFINITE);
            } else {
                while fence.GetCompletedValue() < fence_value {
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        }

        true
    }
}

#[no_mangle]
pub extern "C" fn get_frame_index() -> u32 {
    (FRAME_COUNTER.load(Ordering::Relaxed) % ALLOCATOR_POOL_SIZE as u64) as u32
}

#[no_mangle]
pub extern "C" fn force_cleanup() {
    debug_println!("[force_cleanup] Cleaning up...");

    wait_for_gpu();

    FENCE_EVENT.with(|cell| {
        if let Some(event) = cell.borrow_mut().take() {
            unsafe { let _ = CloseHandle(event); }
        }
    });

    let mut state = match STATE.try_lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    state.command_list_open = false;
    state.command_list = None;
    state.command_allocators.clear();
    state.swap_chain = None;
    state.command_queue = None;
    state.root_signature = None;
    state.fence = None;
    state.descriptor_heaps.clear();
    state.reset_bindings();

    debug_println!("[force_cleanup] ✅ Done");
}