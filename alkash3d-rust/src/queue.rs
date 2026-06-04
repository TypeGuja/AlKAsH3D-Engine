// src/queue.rs

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;

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