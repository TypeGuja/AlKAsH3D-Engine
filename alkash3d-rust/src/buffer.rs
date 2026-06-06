// src/buffer.rs - ПОЛНОСТЬЮ ИСПРАВЛЕННАЯ ВЕРСИЯ

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows_core::Interface;
use crate::{debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_buffer(
    device_ptr: *mut c_void,
    size: usize,
    buffer_type: u32,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_buffer] size={}, type={}", size, buffer_type);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[create_buffer] No device!");
                return std::ptr::null_mut();
            }
        };

        let (heap_type, resource_state) = match buffer_type {
            0 => (D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ),
            1 => (D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COMMON),
            2 => (D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST),
            _ => (D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COMMON),
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: heap_type,
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
        match device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            resource_state,
            None,
            &mut buffer
        ) {
            Ok(_) => {
                if let Some(b) = buffer {
                    let raw_ptr = b.as_raw();
                    debug_println!("[create_buffer] ✅ Created at {:p}", raw_ptr);
                    let boxed = Box::new(b);
                    Box::into_raw(boxed) as *mut c_void
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
pub extern "C" fn update_buffer(
    buffer_ptr: *mut c_void,
    data_ptr: *const c_void,
    size: usize,
    offset: usize,
) -> bool {
    unsafe {
        debug_println!("\n[update_buffer] START: buffer_ptr={:p}, data_ptr={:p}, size={}, offset={}",
                      buffer_ptr, data_ptr, size, offset);

        if buffer_ptr.is_null() {
            debug_println!("[update_buffer] buffer_ptr is NULL!");
            return false;
        }

        if data_ptr.is_null() {
            debug_println!("[update_buffer] data_ptr is NULL!");
            return false;
        }

        if size == 0 {
            debug_println!("[update_buffer] size is 0!");
            return false;
        }

        // Восстанавливаем буфер из Box
        let buffer_ref = &*(buffer_ptr as *const ID3D12Resource);

        // Проверяем, что указатель валидный, пытаясь получить описание
        let desc = buffer_ref.GetDesc();
        debug_println!("[update_buffer] Buffer desc: Width={}", desc.Width);

        if (desc.Width as usize) < offset + size {
            debug_println!("[update_buffer] Buffer too small: {} < {}", desc.Width, offset + size);
            return false;
        }

        let mut mapped: *mut c_void = std::ptr::null_mut();

        match buffer_ref.Map(0, None, Some(&mut mapped)) {
            Ok(_) => {
                if mapped.is_null() {
                    debug_println!("[update_buffer] Map returned NULL!");
                    buffer_ref.Unmap(0, None);
                    return false;
                }

                debug_println!("[update_buffer] Mapped at {:p}", mapped);

                let dst = (mapped as *mut u8).add(offset);
                std::ptr::copy_nonoverlapping(data_ptr as *const u8, dst, size);

                let write_range = D3D12_RANGE {
                    Begin: offset,
                    End: offset + size
                };
                buffer_ref.Unmap(0, Some(&write_range));
                debug_println!("[update_buffer] ✅ Data copied successfully");
                true
            }
            Err(e) => {
                debug_println!("[update_buffer] Map failed: {:?}", e);
                false
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn update_subresource(
    buffer_ptr: *mut c_void,
    data_ptr: *const c_void,
    size: usize,
) -> bool {
    debug_println!("\n[update_subresource] Calling update_buffer with offset 0");
    update_buffer(buffer_ptr, data_ptr, size, 0)
}

#[no_mangle]
pub extern "C" fn destroy_buffer(buffer_ptr: *mut c_void) -> bool {
    if buffer_ptr.is_null() {
        debug_println!("[destroy_buffer] buffer_ptr is NULL");
        return false;
    }
    unsafe {
        debug_println!("[destroy_buffer] Destroying buffer at {:p}", buffer_ptr);
        let _ = Box::from_raw(buffer_ptr as *mut ID3D12Resource);
        debug_println!("[destroy_buffer] ✅ Buffer destroyed");
        true
    }
}

#[no_mangle]
pub extern "C" fn get_buffer_gpu_address(buffer_ptr: *mut c_void) -> u64 {
    unsafe {
        if buffer_ptr.is_null() {
            debug_println!("[get_buffer_gpu_address] buffer_ptr is NULL");
            return 0;
        }
        let buffer = &*(buffer_ptr as *const ID3D12Resource);
        let addr = buffer.GetGPUVirtualAddress();
        debug_println!("[get_buffer_gpu_address] GPU addr: 0x{:X}", addr);
        addr
    }
}

#[no_mangle]
pub extern "C" fn map_buffer (buffer_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        if buffer_ptr.is_null() {
            debug_println!("[ma_buffer] buffer_ptr is NULL");
            return std::ptr::null_mut();

        }
        let buffer = &*(buffer_ptr as *const ID3D12Resource);
        let mut mapped: *mut c_void = std::ptr::null_mut();

        match buffer.Map(0, None, Some(&mut mapped)) {
            Ok(_) => {
                debug_println!("[ma_buffer] Mapped at {:p}", mapped);
                mapped
            }
            Err(e) => {
                debug_println!("[ma_buffer] Map failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn unmap_buffer(buffer_ptr: *mut c_void) -> bool {
    unsafe {
        if buffer_ptr.is_null() {
            return false;
        }
        let buffer = &*(buffer_ptr as *const ID3D12Resource);
        buffer.Unmap(0, None);
        debug_println!("[unmap_buffer] Unmapped");
        true
    }
}
