// src/queue.rs
//! Командная очередь

use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;
use windows_core::Interface;
use crate::{STATE, debug_println};

#[no_mangle]
pub extern "C" fn create_command_queue(_device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_command_queue] Creating command queue...");

        // Игнорируем переданный указатель, берём устройство из STATE
        let device = {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            match state.device.as_ref() {
                Some(d) => d.clone(),
                None => {
                    debug_println!("[create_command_queue] No device in STATE!");
                    return ptr::null_mut();
                }
            }
        };

        debug_println!("[create_command_queue] Device from STATE: {:p}", &device);

        let desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };

        match device.CreateCommandQueue::<ID3D12CommandQueue>(&desc) {
            Ok(queue) => {
                debug_println!("[create_command_queue] Queue created successfully");

                {
                    let mut state = match STATE.lock() {
                        Ok(s) => s,
                        Err(_) => return ptr::null_mut(),
                    };
                    state.command_queue = Some(queue.clone());
                }

                let raw_ptr = queue.as_raw() as *mut c_void;
                debug_println!("[create_command_queue] Raw pointer: {:p}", raw_ptr);
                raw_ptr
            }
            Err(e) => {
                debug_println!("[create_command_queue] Failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn create_command_queue_ex(device_ptr: *mut c_void, queue_type: u32) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_command_queue_ex] Type: {}", queue_type);

        let device = if device_ptr.is_null() {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            match state.device.as_ref() {
                Some(d) => d.clone(),
                None => return ptr::null_mut(),
            }
        } else {
            (&*(device_ptr as *const ID3D12Device)).clone()
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
                    let mut state = match STATE.lock() {
                        Ok(s) => s,
                        Err(_) => return ptr::null_mut(),
                    };
                    state.command_queue = Some(queue.clone());
                }
                queue.as_raw() as *mut c_void
            }
            Err(e) => {
                debug_println!("[create_command_queue_ex] Failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn destroy_command_queue(_queue_ptr: *mut c_void) -> bool {
    debug_println!("[destroy_command_queue] Queue will be cleaned by STATE");
    true
}

#[no_mangle]
pub extern "C" fn execute_command_lists(queue_ptr: *mut c_void, lists: *const *mut c_void, count: u32) -> bool {
    unsafe {
        if queue_ptr.is_null() || lists.is_null() || count == 0 {
            return false;
        }

        let queue = &*(queue_ptr as *const ID3D12CommandQueue);
        let mut cmd_lists = Vec::with_capacity(count as usize);

        for i in 0..count as usize {
            let list_ptr = *lists.add(i);
            if !list_ptr.is_null() {
                let list = &*(list_ptr as *const ID3D12CommandList);
                cmd_lists.push(Some(list.clone()));
            }
        }

        queue.ExecuteCommandLists(&cmd_lists);
        true
    }
}

#[no_mangle]
pub extern "C" fn get_command_queue(queue_ptr: *mut c_void) -> *mut c_void {
    queue_ptr
}