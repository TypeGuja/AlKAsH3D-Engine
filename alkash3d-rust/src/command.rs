//! Командные списки и синхронизация

use std::ffi::c_void;
use std::cell::RefCell;
use windows::Win32::{
    Foundation::CloseHandle,
    Graphics::Direct3D12::*,
    System::Threading::{CreateEventA, WaitForSingleObject, INFINITE},
};
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_device};

thread_local! {
    static CURRENT_COMMAND_LIST: RefCell<Option<ID3D12GraphicsCommandList>> = RefCell::new(None);
    static CURRENT_ALLOCATOR_INDEX: RefCell<usize> = RefCell::new(0);
}

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
                    debug_println!("[create_command_allocators] Failed: {:?}", e);
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

        match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            &allocator,
            None,
        ) {
            Ok(command_list) => {
                let _ = command_list.Close();

                let mut state = STATE.lock().unwrap();
                state.command_list = Some(command_list.clone());
                state.command_list_open = false;

                CURRENT_COMMAND_LIST.with(|cell| {
                    *cell.borrow_mut() = Some(command_list.clone());
                });

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
                debug_println!("[create_fence] Success");
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
        debug_println!("[begin_frame] No command queue");
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
    let allocator_idx = frame_idx % state.command_allocators.len();
    let allocator = match state.command_allocators[allocator_idx].clone() {
        Some(a) => a,
        None => {
            debug_println!("[begin_frame] Allocator {} is None", allocator_idx);
            return false;
        }
    };

    let list = match state.command_list.clone() {
        Some(l) => l,
        None => return false,
    };

    drop(state);

    unsafe {
        if let Err(e) = allocator.Reset() {
            debug_println!("[begin_frame] Allocator reset failed: {:?}", e);
            return false;
        }

        if let Err(e) = list.Reset(&allocator, None) {
            debug_println!("[begin_frame] Command list reset failed: {:?}", e);
            return false;
        }
    }

    let mut state = STATE.lock().unwrap();
    state.command_list_open = true;
    CURRENT_ALLOCATOR_INDEX.with(|cell| *cell.borrow_mut() = allocator_idx);

    if !state.descriptor_heaps.is_empty() {
        if let Some(list) = state.command_list.clone() {
            let mut heaps: Vec<Option<ID3D12DescriptorHeap>> = Vec::new();
            for heap in &state.descriptor_heaps {
                unsafe {
                    if heap.GetDesc().Flags == D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE {
                        heaps.push(Some(heap.clone()));
                    }
                }
            }

            if !heaps.is_empty() {
                unsafe {
                    list.SetDescriptorHeaps(&heaps);
                }
            }
        }
    }

    debug_println!("[begin_frame] Success");
    true
}

#[no_mangle]
pub extern "C" fn end_frame() -> bool {
    debug_println!("\n[end_frame] Called");

    let (queue, list, fence, frame_idx) = {
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

        (queue, list, fence, state.frame_index as usize)
    };

    let next_fence_value = {
        let mut state = STATE.lock().unwrap();
        let val = state.fence_values[frame_idx] + 1;
        state.fence_values[frame_idx] = val;
        val
    };

    unsafe {
        if let Err(e) = list.Close() {
            debug_println!("[end_frame] Close failed: {:?}", e);
            return false;
        }

        let cmd_list: ID3D12CommandList = match list.cast() {
            Ok(l) => l,
            Err(e) => {
                debug_println!("[end_frame] Cast failed: {:?}", e);
                return false;
            }
        };

        queue.ExecuteCommandLists(&[Some(cmd_list)]);

        if let Err(e) = queue.Signal(&fence, next_fence_value) {
            debug_println!("[end_frame] Signal failed: {:?}", e);
            return false;
        }
    }

    {
        let mut state = STATE.lock().unwrap();
        state.command_list_open = false;
        state.frame_index = (state.frame_index + 1) % (state.fence_values.len() as u32);
    }

    debug_println!("[end_frame] Success (fence: {})", next_fence_value);
    true
}

#[no_mangle]
pub extern "C" fn wait_for_gpu() -> bool {
    debug_println!("\n[wait_for_gpu] Called");

    let (fence, fence_value) = {
        let state = STATE.lock().unwrap();
        let fence = match state.fence.clone() {
            Some(f) => f,
            None => {
                debug_println!("[wait_for_gpu] No fence");
                return false;
            }
        };

        let fence_value = state.fence_values[state.frame_index as usize];
        (fence, fence_value)
    };

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
                    let _ = CloseHandle(event);
                    return false;
                }

                WaitForSingleObject(event, INFINITE);
                let _ = CloseHandle(event);
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
            debug_println!("[force_cleanup] Could not lock state");
            return;
        }
    };

    CURRENT_COMMAND_LIST.with(|cell| {
        *cell.borrow_mut() = None;
    });

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

pub fn with_command_list<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&ID3D12GraphicsCommandList) -> R
{
    CURRENT_COMMAND_LIST.with(|cell| {
        let list = cell.borrow();
        list.as_ref().map(|l| f(l))
    })
}

#[no_mangle]
pub extern "C" fn transition_resource(
    resource_ptr: *mut c_void,
    state_before: u32,
    state_after: u32,
) -> bool {
    unsafe {
        if resource_ptr.is_null() {
            return false;
        }

        let result = with_command_list(|list| {
            let resource: ID3D12Resource = std::mem::transmute_copy(&resource_ptr);

            let barrier = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATES(state_before as i32),
                        StateAfter: D3D12_RESOURCE_STATES(state_after as i32),
                    }),
                },
            };

            list.ResourceBarrier(&[barrier]);
            std::mem::forget(resource);
        });

        result.is_some()
    }
}