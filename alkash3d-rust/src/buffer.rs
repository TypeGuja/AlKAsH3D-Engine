//! Буферы (vertex, index, constant)

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows_core::Interface;
use crate::{debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_buffer(device_ptr: *mut c_void, size: usize, _usage: *const u8) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_buffer] Creating buffer of size {}", size);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        };

        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: size as u64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let mut buffer: Option<ID3D12Resource> = None;
        match device.CreateCommittedResource(&heap_props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut buffer) {
            Ok(_) => {
                if let Some(b) = buffer {
                    let raw_ptr = b.as_raw();
                    debug_println!("[create_buffer] Created at {:p}", raw_ptr);
                    std::mem::forget(b);
                    raw_ptr as *mut c_void
                } else {
                    std::ptr::null_mut()
                }
            }
            Err(e) => {
                debug_println!("[create_buffer] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn update_subresource(buffer_ptr: *mut c_void, data_ptr: *const c_void, size: usize) -> bool {
    unsafe {
        if buffer_ptr.is_null() || data_ptr.is_null() || size == 0 {
            return false;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let desc = buffer.GetDesc();

        if (desc.Width as usize) < size {
            std::mem::forget(buffer);
            return false;
        }

        let mut mapped: *mut c_void = std::ptr::null_mut();
        let read_range = D3D12_RANGE { Begin: 0, End: 0 };

        match buffer.Map(0, Some(&read_range), Some(&mut mapped)) {
            Ok(_) => {
                if mapped.is_null() {
                    buffer.Unmap(0, None);
                    std::mem::forget(buffer);
                    return false;
                }

                std::ptr::copy_nonoverlapping(data_ptr as *const u8, mapped as *mut u8, size);

                let write_range = D3D12_RANGE { Begin: 0, End: size };
                buffer.Unmap(0, Some(&write_range));

                std::mem::forget(buffer);
                true
            }
            Err(e) => {
                debug_println!("[update_subresource] Map failed: {:?}", e);
                std::mem::forget(buffer);
                false
            }
        }
    }
}