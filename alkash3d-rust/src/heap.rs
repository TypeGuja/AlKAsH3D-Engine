// src/heap.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ

use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;
use windows_core::Interface;
use crate::{debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_descriptor_heap(
    device_ptr: *mut c_void,
    num_descriptors: u32,
    heap_type: u32,
    shader_visible: bool,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_descriptor_heap] START: num={}, type={}", num_descriptors, heap_type);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[create_descriptor_heap] No device!");
                return ptr::null_mut();
            }
        };

        let heap_ty = match heap_type {
            0 => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            1 => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            2 => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            3 => D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
            _ => {
                debug_println!("[create_descriptor_heap] Invalid heap type!");
                return ptr::null_mut();
            }
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
                let raw_ptr = heap.as_raw();
                debug_println!("[create_descriptor_heap] ✅ Heap created at {:p}", raw_ptr);

                // Сохраняем в Box
                let boxed = Box::new(heap);
                let result = Box::into_raw(boxed) as *mut c_void;
                debug_println!("[create_descriptor_heap] Returning pointer: {:p}", result);
                result
            }
            Err(e) => {
                debug_println!("[create_descriptor_heap] Failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn GetCPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> u64 {
    debug_println!("\n[GetCPUDescriptorHandleForHeapStart] heap_ptr={:p}", heap_ptr);

    if heap_ptr.is_null() {
        debug_println!("[GetCPUDescriptorHandleForHeapStart] heap_ptr is NULL!");
        return 0;
    }

    unsafe {
        // Восстанавливаем Box, но не забираем владение (используем &)
        let heap = &*(heap_ptr as *const ID3D12DescriptorHeap);
        let handle = heap.GetCPUDescriptorHandleForHeapStart();
        let ptr_value = handle.ptr as u64;
        debug_println!("[GetCPUDescriptorHandleForHeapStart] handle ptr: 0x{:X}", ptr_value);
        ptr_value
    }
}

#[no_mangle]
pub extern "C" fn get_descriptor_handle_increment_size(
    device_ptr: *mut c_void,
    heap_type: u32
) -> u32 {
    unsafe {
        debug_println!("\n[get_descriptor_handle_increment_size] heap_type={}", heap_type);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("  No device!");
                return 0;
            }
        };

        let ty = match heap_type {
            0 => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            1 => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            2 => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            3 => D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
            _ => {
                debug_println!("  Invalid heap type!");
                return 0;
            }
        };

        let size = device.GetDescriptorHandleIncrementSize(ty);
        debug_println!("  Increment size: {} bytes", size);
        size
    }
}

#[no_mangle]
pub extern "C" fn destroy_descriptor_heap(heap_ptr: *mut c_void) -> bool {
    if heap_ptr.is_null() {
        debug_println!("[destroy_descriptor_heap] heap_ptr is NULL");
        return false;
    }
    unsafe {
        debug_println!("[destroy_descriptor_heap] Destroying heap at {:p}", heap_ptr);
        let _ = Box::from_raw(heap_ptr as *mut ID3D12DescriptorHeap);
        debug_println!("[destroy_descriptor_heap] ✅ Heap destroyed");
        true
    }
}