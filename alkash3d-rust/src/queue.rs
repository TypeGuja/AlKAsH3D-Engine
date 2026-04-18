//! Командная очередь с поддержкой разных типов

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_command_queue(device_ptr: *mut c_void) -> *mut c_void {
    create_command_queue_ex(device_ptr, 0) // 0 = D3D12_COMMAND_LIST_TYPE_DIRECT
}

#[no_mangle]
pub extern "C" fn create_command_queue_ex(device_ptr: *mut c_void, queue_type: u32) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_command_queue] Type: {}", queue_type);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let list_type = match queue_type {
            0 => D3D12_COMMAND_LIST_TYPE_DIRECT,
            1 => D3D12_COMMAND_LIST_TYPE_BUNDLE,
            2 => D3D12_COMMAND_LIST_TYPE_COMPUTE,
            3 => D3D12_COMMAND_LIST_TYPE_COPY,
            _ => D3D12_COMMAND_LIST_TYPE_DIRECT,
        };

        let desc = D3D12_COMMAND_QUEUE_DESC {
            Type: list_type,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };

        match device.CreateCommandQueue::<ID3D12CommandQueue>(&desc) {
            Ok(queue) => {
                if queue_type == 0 {
                    let mut state = STATE.lock().unwrap();
                    state.command_queue = Some(queue.clone());
                }

                let raw_ptr = queue.as_raw();
                std::mem::forget(queue);
                std::mem::forget(device);

                debug_println!("[create_command_queue] ✅ Created at {:p}", raw_ptr);
                raw_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[create_command_queue] Failed: {:?}", e);
                std::mem::forget(device);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn execute_command_lists(queue_ptr: *mut c_void, lists: *const *mut c_void, count: u32) -> bool {
    unsafe {
        if queue_ptr.is_null() || lists.is_null() || count == 0 {
            return false;
        }

        let queue: ID3D12CommandQueue = std::mem::transmute_copy(&queue_ptr);

        let mut cmd_lists = Vec::new();
        for i in 0..count as usize {
            let list_ptr = *lists.add(i);
            if !list_ptr.is_null() {
                let list: ID3D12CommandList = std::mem::transmute_copy(&list_ptr);
                cmd_lists.push(Some(list));
            }
        }

        queue.ExecuteCommandLists(&cmd_lists);

        std::mem::forget(queue);
        true
    }
}