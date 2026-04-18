//! Дескрипторные кучи

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows_core::Interface;
use crate::{STATE, debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_descriptor_heap(
    device_ptr: *mut c_void,
    num_descriptors: u32,
    heap_type: u32,
    shader_visible: bool,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_descriptor_heap] num={}, type={}, shader_visible={}",
                       num_descriptors, heap_type, shader_visible);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let heap_ty = match heap_type {
            0 => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            1 => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            2 => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            3 => D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
            _ => return std::ptr::null_mut(),
        };

        let flags = if shader_visible && heap_ty == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV {
            D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
        } else {
            D3D12_DESCRIPTOR_HEAP_FLAG_NONE
        };

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: heap_ty,
            NumDescriptors: num_descriptors,
            Flags: flags,
            NodeMask: 0,
        };

        match device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) {
            Ok(heap) => {
                debug_println!("[heap] ✅ Created at {:p}", heap.as_raw());

                if let Ok(mut state) = STATE.lock() {
                    state.descriptor_heaps.push(heap.clone());
                }

                let raw_ptr = heap.as_raw();
                std::mem::forget(heap);
                std::mem::forget(device);
                raw_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[heap] Failed: {:?}", e);
                std::mem::forget(device);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn GetGPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> u64 {
    if heap_ptr.is_null() {
        return 0;
    }

    unsafe {
        let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
        let desc = heap.GetDesc();

        if desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE {
            std::mem::forget(heap);
            return 0;
        }

        let gpu_handle = heap.GetGPUDescriptorHandleForHeapStart();
        let result = gpu_handle.ptr as u64;
        std::mem::forget(heap);
        result
    }
}

#[no_mangle]
pub extern "C" fn GetCPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> u64 {
    if heap_ptr.is_null() {
        return 0;
    }
    unsafe {
        let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
        let handle = heap.GetCPUDescriptorHandleForHeapStart();
        let result = handle.ptr as u64;
        std::mem::forget(heap);
        result
    }
}

#[no_mangle]
pub extern "C" fn get_descriptor_handle_increment_size(
    device_ptr: *mut c_void,
    heap_type: u32
) -> u32 {
    unsafe {
        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return 0,
        };

        let ty = match heap_type {
            0 => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            1 => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            2 => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            3 => D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
            _ => return 0,
        };

        let size = device.GetDescriptorHandleIncrementSize(ty);
        std::mem::forget(device);
        size
    }
}