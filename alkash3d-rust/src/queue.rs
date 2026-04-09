//! Командная очередь

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_command_queue(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_command_queue] Called");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };

        let queue: ID3D12CommandQueue = match device.CreateCommandQueue(&desc) {
            Ok(q) => q,
            Err(_) => return std::ptr::null_mut(),
        };

        {
            let mut state = STATE.lock().unwrap();
            state.command_queue = Some(queue.clone());
        }

        let raw_ptr = queue.as_raw();
        std::mem::forget(queue);
        raw_ptr as *mut c_void
    }
}