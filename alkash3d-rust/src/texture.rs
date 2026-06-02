// src/texture.rs
//! Текстуры - ИСПРАВЛЕННАЯ ВЕРСИЯ

use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_core::Interface;
use crate::{debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_texture_2d(
    device_ptr: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    mip_levels: u32,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_texture_2d] {}x{}, mips={}", width, height, mip_levels);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        let dxgi_format = match format {
            0 => DXGI_FORMAT_R8G8B8A8_UNORM,
            1 => DXGI_FORMAT_R32G32B32A32_FLOAT,
            2 => DXGI_FORMAT_BC1_UNORM,
            3 => DXGI_FORMAT_BC3_UNORM,
            4 => DXGI_FORMAT_BC5_UNORM,
            _ => DXGI_FORMAT_R8G8B8A8_UNORM,
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
            MipLevels: mip_levels as u16,
            Format: dxgi_format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let mut texture: Option<ID3D12Resource> = None;
        match device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut texture
        ) {
            Ok(_) => {
                if let Some(tex) = texture {
                    let raw_ptr = tex.as_raw();
                    debug_println!("[create_texture_2d] ✅ Created at {:p}", raw_ptr);
                    // ИСПРАВЛЕНИЕ: используем Box вместо forget
                    Box::into_raw(Box::new(tex)) as *mut c_void
                } else {
                    ptr::null_mut()
                }
            }
            Err(e) => {
                debug_println!("[create_texture_2d] Failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn destroy_texture_2d(texture_ptr: *mut c_void) -> bool {
    if texture_ptr.is_null() {
        return false;
    }
    unsafe {
        let _ = Box::from_raw(texture_ptr as *mut ID3D12Resource);
        debug_println!("[destroy_texture_2d] ✅ Texture destroyed");
        true
    }
}

#[no_mangle]
pub extern "C" fn create_texture_from_memory(
    device_ptr: *mut c_void,
    data_ptr: *mut c_void,
    width: u32,
    height: u32,
    _fmt: *const u8,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_texture_from_memory] {}x{}", width, height);

        let texture = create_texture_2d(device_ptr, width, height, 0, 1);
        if texture.is_null() {
            return ptr::null_mut();
        }

        if !data_ptr.is_null() {
            update_texture(texture, data_ptr, width, height);
        }

        texture
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

        let texture = &*(texture_ptr as *const ID3D12Resource);
        let _desc = texture.GetDesc();

        let row_pitch = (width * 4) as usize;
        let total_size = row_pitch * height as usize;

        let device = match get_device_from_resource(texture) {
            Some(d) => d,
            None => return false,
        };

        let upload_buffer = create_buffer(device.as_raw() as *mut c_void, total_size, 0);

        if upload_buffer.is_null() {
            return false;
        }

        if !update_subresource(upload_buffer, data_ptr, total_size) {
            destroy_buffer(upload_buffer);
            return false;
        }

        destroy_buffer(upload_buffer);
        true
    }
}

unsafe fn get_device_from_resource(resource: &ID3D12Resource) -> Option<ID3D12Device> {
    let mut device_ptr: *mut c_void = ptr::null_mut();
    let iid = &ID3D12Device::IID;

    if resource.query(iid, &mut device_ptr).is_ok() && !device_ptr.is_null() {
        // ИСПРАВЛЕНИЕ: читаем указатель и клонируем COM объект
        let device = &*(device_ptr as *const ID3D12Device);
        Some(device.clone())
    } else {
        None
    }
}

#[no_mangle]
pub extern "C" fn create_srv(
    device_ptr: *mut c_void,
    texture_ptr: *mut c_void,
    cpu_handle: u64,
) -> bool {
    unsafe {
        if device_ptr.is_null() || texture_ptr.is_null() || cpu_handle == 0 {
            return false;
        }

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let texture = &*(texture_ptr as *const ID3D12Resource);
        let desc = texture.GetDesc();

        let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: desc.Format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: desc.MipLevels as u32,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        };

        let cpu_handle_struct = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle as usize };
        device.CreateShaderResourceView(texture, Some(&srv_desc), cpu_handle_struct);

        true
    }
}

// Вспомогательные функции
extern "C" {
    fn create_buffer(device_ptr: *mut c_void, size: usize, buffer_type: u32) -> *mut c_void;
    fn update_subresource(buffer_ptr: *mut c_void, data_ptr: *const c_void, size: usize) -> bool;
    fn destroy_buffer(buffer_ptr: *mut c_void) -> bool;
}