//! Командные списки и синхронизация

use std::ffi::c_void;
use std::cell::RefCell;
use std::mem::ManuallyDrop;
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

const ALLOCATOR_POOL_SIZE: usize = 3;
static FRAME_COUNTER: AtomicU64 = AtomicU64::new(0);
static mut ALLOCATOR_FENCE_VALUES: [u64; ALLOCATOR_POOL_SIZE] = [0; ALLOCATOR_POOL_SIZE];

#[no_mangle]
pub extern "C" fn create_command_allocators(_device_ptr: *mut c_void, _count: u32) -> bool {
    true
}

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
        debug_println!("[with_command_list] Command list not open");
        return None;
    }

    let list = match state.command_list.as_ref() {
        Some(l) => l,
        None => {
            debug_println!("[with_command_list] Command list is None");
            return None;
        }
    };

    Some(f(list))
}

#[no_mangle]
pub extern "C" fn create_command_list(_device_ptr: *mut c_void) -> *mut c_void {
    std::ptr::null_mut()
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
                        Err(_) => {
                            std::mem::forget(device);
                            return false;
                        }
                    };
                    state.fence = Some(fence);
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
                std::mem::forget(device);
                true
            }
            Err(e) => {
                debug_println!("[create_fence] Failed: {:?}", e);
                std::mem::forget(device);
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

        (fence, ALLOCATOR_FENCE_VALUES[allocator_idx],
         FENCE_EVENT.with(|cell| cell.borrow().clone()))
    };

    if fence_value == 0 {
        return;
    }

    let completed = fence.GetCompletedValue();
    if completed < fence_value {
        debug_println!("[wait] Allocator {} waiting ({} < {})",
                      allocator_idx, completed, fence_value);
        if let Some(event) = event {
            let _ = fence.SetEventOnCompletion(fence_value, event);
            WaitForSingleObject(event, INFINITE);
        } else {
            while fence.GetCompletedValue() < fence_value {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        debug_println!("[wait] Allocator {} ready", allocator_idx);
    }
}

unsafe fn create_new_allocator(device: &ID3D12Device, idx: usize) -> Option<ID3D12CommandAllocator> {
    match device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) {
        Ok(a) => {
            debug_println!("[begin_frame] Created new allocator {}", idx);

            let mut state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return None,
            };

            while state.command_allocators.len() <= idx {
                state.command_allocators.push(None);
            }
            state.command_allocators[idx] = Some(a.clone());

            Some(a)
        }
        Err(e) => {
            debug_println!("[begin_frame] CreateCommandAllocator failed: {:?}", e);
            None
        }
    }
}

#[no_mangle]
pub extern "C" fn begin_frame() -> *mut c_void {
    let frame_id = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);
    let allocator_idx = (frame_id % ALLOCATOR_POOL_SIZE as u64) as usize;

    debug_println!("\n[begin_frame] Frame {} (allocator {})", frame_id, allocator_idx);

    unsafe {
        wait_for_allocator(allocator_idx);
    }

    // Получаем device и allocator
    let (device, allocator) = {
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        let device = match state.device.as_ref() {
            Some(d) => d.clone(),
            None => return std::ptr::null_mut(),
        };

        let allocator = if allocator_idx < state.command_allocators.len() {
            match &state.command_allocators[allocator_idx] {
                Some(a) => a.clone(),
                None => {
                    drop(state);
                    match unsafe { create_new_allocator(&device, allocator_idx) } {
                        Some(a) => a,
                        None => return std::ptr::null_mut(),
                    }
                }
            }
        } else {
            drop(state);
            match unsafe { create_new_allocator(&device, allocator_idx) } {
                Some(a) => a,
                None => return std::ptr::null_mut(),
            }
        };

        (device, allocator)
    };

    unsafe {
        let _ = allocator.Reset();

        match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None,
        ) {
            Ok(list) => {
                let list_ptr = list.as_raw();

                {
                    let mut state = match STATE.lock() {
                        Ok(s) => s,
                        Err(_) => return std::ptr::null_mut(),
                    };
                    state.command_list = Some(list);  // НЕ клонируем!
                    state.command_list_open = true;
                    state.reset_bindings();
                }

                debug_println!("[begin_frame] ✅ Ready");
                list_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[begin_frame] CreateCommandList failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn end_frame() -> bool {
    let frame_id = FRAME_COUNTER.load(Ordering::Relaxed) - 1;
    let allocator_idx = (frame_id % ALLOCATOR_POOL_SIZE as u64) as usize;

    debug_println!("\n[end_frame] Frame {} (allocator {}) START", frame_id, allocator_idx);

    // Забираем command list из состояния
    let (queue, list, fence) = {
        debug_println!("[end_frame] Locking STATE...");
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(e) => {
                debug_println!("[end_frame] Failed to lock state: {:?}", e);
                return false;
            }
        };
        debug_println!("[end_frame] STATE locked");

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

        // Забираем command list
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

        (queue, list, fence)
    };

    // Оборачиваем в ManuallyDrop чтобы предотвратить автоматический дроп
    let list = ManuallyDrop::new(list);

    unsafe {
        debug_println!("[end_frame] Closing command list...");
        if let Err(e) = list.Close() {
            debug_println!("[end_frame] Close failed: {:?}", e);
            return false;
        }
        debug_println!("[end_frame] Command list closed");

        debug_println!("[end_frame] Executing command list...");
        let cmd_list_raw = list.as_raw();
        let cmd_list: ID3D12CommandList = std::mem::transmute_copy(&cmd_list_raw);
        let cmd_lists = [Some(cmd_list)];
        queue.ExecuteCommandLists(&cmd_lists);
        debug_println!("[end_frame] Command list executed");

        let next_fence_value = {
            debug_println!("[end_frame] Updating fence value...");
            let mut state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return false,
            };
            let val = state.fence_values[0] + 1;
            state.fence_values[0] = val;
            val
        };
        debug_println!("[end_frame] Fence value: {}", next_fence_value);

        debug_println!("[end_frame] Signaling fence...");
        if let Err(e) = queue.Signal(&fence, next_fence_value) {
            debug_println!("[end_frame] Signal failed: {:?}", e);
            return false;
        }
        debug_println!("[end_frame] Fence signaled");

        ALLOCATOR_FENCE_VALUES[allocator_idx] = next_fence_value;

        debug_println!("[end_frame] ✅ Done (fence: {})", next_fence_value);
    }

    // ManuallyDrop предотвращает вызов деструктора
    // Command list будет освобожден когда allocator сбросится в begin_frame

    debug_println!("[end_frame] RETURN true");
    true
}

#[no_mangle]
pub extern "C" fn wait_for_gpu() -> bool {
    debug_println!("[wait_for_gpu] Waiting for all frames...");

    unsafe {
        for i in 0..ALLOCATOR_POOL_SIZE {
            if ALLOCATOR_FENCE_VALUES[i] > 0 {
                wait_for_allocator(i);
            }
        }
    }

    debug_println!("[wait_for_gpu] ✅ All frames complete");
    true
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