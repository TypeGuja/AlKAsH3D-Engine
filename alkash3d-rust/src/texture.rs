//! Текстуры

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_core::Interface;
use crate::{debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_texture_from_memory(
    device_ptr: *mut c_void,
    _data_ptr: *mut c_void,
    width: u32,
    height: u32,
    _fmt: *const u8,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_texture_from_memory] {}x{}", width, height);

        if device_ptr.is_null() || width == 0 || height == 0 {
            return std::ptr::null_mut();
        }

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        };

        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let mut texture: Option<ID3D12Resource> = None;
        match device.CreateCommittedResource(&heap_props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut texture) {
            Ok(_) => {
                if let Some(tex) = texture {
                    let raw_ptr = tex.as_raw();
                    std::mem::forget(tex);
                    raw_ptr as *mut c_void
                } else {
                    std::ptr::null_mut()
                }
            }
            Err(e) => {
                debug_println!("[create_texture_from_memory] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn update_texture(
    texture_ptr: *mut c_void,
    data_ptr: *const c_void,
    width: u32,
    height: u32,
) -> bool {
    unsafe {
        if texture_ptr.is_null() || data_ptr.is_null() {
            return false;
        }

        let texture: ID3D12Resource = std::mem::transmute_copy(&texture_ptr);

        let mut mapped: *mut c_void = std::ptr::null_mut();
        let read_range = D3D12_RANGE { Begin: 0, End: 0 };

        match texture.Map(0, Some(&read_range), Some(&mut mapped)) {
            Ok(_) => {
                if mapped.is_null() {
                    texture.Unmap(0, None);
                    std::mem::forget(texture);
                    return false;
                }

                let row_pitch = width as usize * 4;
                let total_size = row_pitch * height as usize;

                std::ptr::copy_nonoverlapping(data_ptr as *const u8, mapped as *mut u8, total_size);

                let write_range = D3D12_RANGE { Begin: 0, End: total_size };
                texture.Unmap(0, Some(&write_range));

                std::mem::forget(texture);
                true
            }
            Err(e) => {
                debug_println!("[update_texture] Map failed: {:?}", e);
                std::mem::forget(texture);
                false
            }
        }
    }
}