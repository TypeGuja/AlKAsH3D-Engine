//! Буферы (vertex, index, constant)

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
            None => return std::ptr::null_mut(),
        };

        let heap_type = match buffer_type {
            0 => D3D12_HEAP_TYPE_UPLOAD,      // Upload heap (CPU write)
            1 => D3D12_HEAP_TYPE_DEFAULT,     // Default heap (GPU only)
            2 => D3D12_HEAP_TYPE_READBACK,    // Readback heap (CPU read)
            _ => D3D12_HEAP_TYPE_UPLOAD,
        };

        let resource_state = match buffer_type {
            0 => D3D12_RESOURCE_STATE_GENERIC_READ,
            1 => D3D12_RESOURCE_STATE_COMMON,
            2 => D3D12_RESOURCE_STATE_COPY_DEST,
            _ => D3D12_RESOURCE_STATE_GENERIC_READ,
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
                    std::mem::forget(b);
                    std::mem::forget(device);
                    raw_ptr as *mut c_void
                } else {
                    std::mem::forget(device);
                    std::ptr::null_mut()
                }
            }
            Err(e) => {
                debug_println!("[create_buffer] Failed: {:?}", e);
                std::mem::forget(device);
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
        if buffer_ptr.is_null() || data_ptr.is_null() || size == 0 {
            return false;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let desc = buffer.GetDesc();

        if (desc.Width as usize) < offset + size {
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

                let dst = (mapped as *mut u8).add(offset);
                std::ptr::copy_nonoverlapping(data_ptr as *const u8, dst, size);

                let write_range = D3D12_RANGE {
                    Begin: offset,
                    End: offset + size
                };
                buffer.Unmap(0, Some(&write_range));

                std::mem::forget(buffer);
                true
            }
            Err(e) => {
                debug_println!("[update_buffer] Map failed: {:?}", e);
                std::mem::forget(buffer);
                false
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn update_buffer_persistent(
    buffer_ptr: *mut c_void,
    data_ptr: *const c_void,
    size: usize,
) -> bool {
    unsafe {
        if buffer_ptr.is_null() || data_ptr.is_null() || size == 0 {
            return false;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);

        // Для UPLOAD буферов используем persistent mapping
        let mut mapped: *mut c_void = std::ptr::null_mut();

        // Мапим без read range
        match buffer.Map(0, None, Some(&mut mapped)) {
            Ok(_) => {
                if mapped.is_null() {
                    buffer.Unmap(0, None);
                    std::mem::forget(buffer);
                    return false;
                }

                // Копируем данные
                std::ptr::copy_nonoverlapping(data_ptr as *const u8, mapped as *mut u8, size);

                // Unmap с записью всех изменений
                let write_range = D3D12_RANGE {
                    Begin: 0,
                    End: size,
                };
                buffer.Unmap(0, Some(&write_range));

                std::mem::forget(buffer);
                true
            }
            Err(e) => {
                debug_println!("[update_buffer_persistent] Map failed: {:?}", e);
                std::mem::forget(buffer);
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
    update_buffer(buffer_ptr, data_ptr, size, 0)
}

#[no_mangle]
pub extern "C" fn map_buffer(buffer_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        if buffer_ptr.is_null() {
            return std::ptr::null_mut();
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let mut mapped: *mut c_void = std::ptr::null_mut();
        let read_range = D3D12_RANGE { Begin: 0, End: 0 };

        match buffer.Map(0, Some(&read_range), Some(&mut mapped)) {
            Ok(_) => {
                std::mem::forget(buffer);
                mapped
            }
            Err(e) => {
                debug_println!("[map_buffer] Failed: {:?}", e);
                std::mem::forget(buffer);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn unmap_buffer(buffer_ptr: *mut c_void, written_range_start: usize, written_range_end: usize) {
    unsafe {
        if buffer_ptr.is_null() {
            return;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let write_range = D3D12_RANGE {
            Begin: written_range_start,
            End: written_range_end
        };
        buffer.Unmap(0, Some(&write_range));
        std::mem::forget(buffer);
    }
}

#[no_mangle]
pub extern "C" fn get_buffer_size(buffer_ptr: *mut c_void) -> u64 {
    unsafe {
        if buffer_ptr.is_null() {
            return 0;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let desc = buffer.GetDesc();
        let size = desc.Width;
        std::mem::forget(buffer);
        size
    }
}