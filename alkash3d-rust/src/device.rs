// src/device.rs
//! Управление D3D12 устройством

use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Dxgi::*;
use windows_core::Interface;
use crate::{STATE, debug_println};

static mut GPU_INFO: GpuInfo = GpuInfo {
    is_real: false,
    name: [0; 256],
    vram_mb: 0,
};

struct GpuInfo {
    is_real: bool,
    name: [u8; 256],
    vram_mb: u32,
}

#[repr(C)]
struct DeviceQueuePair {
    device: *mut c_void,
    queue: *mut c_void,
}

#[no_mangle]
pub extern "C" fn create_device_and_queue() -> *mut c_void {
    unsafe {
        debug_println!("\n[create_device_and_queue] Creating D3D12 device and queue...");

        let factory: IDXGIFactory4 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(e) => {
                debug_println!("Failed to create factory: {:?}", e);
                return ptr::null_mut();
            }
        };

        let mut best_device: Option<ID3D12Device> = None;
        let mut max_vram = 0;
        let mut adapter_index = 0;

        // Ищем лучший адаптер
        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                Ok(a) => a,
                Err(_) => break,
            };

            let desc = match adapter.GetDesc1() {
                Ok(d) => d,
                Err(_) => {
                    adapter_index += 1;
                    continue;
                }
            };

            let name = String::from_utf16_lossy(&desc.Description);
            let vram_mb = desc.DedicatedVideoMemory / 1024 / 1024;
            let is_warp = name.contains("Microsoft Basic Render Driver") || name.contains("WARP") || vram_mb == 0;

            debug_println!("[create_device_and_queue] Adapter {}: {} (VRAM: {} MB)", adapter_index, name, vram_mb);

            if !is_warp && vram_mb > max_vram {
                let mut temp_device: Option<ID3D12Device> = None;
                if D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut temp_device).is_ok() {
                    if let Some(device) = temp_device {
                        max_vram = vram_mb;
                        best_device = Some(device);

                        GPU_INFO.is_real = true;
                        GPU_INFO.vram_mb = vram_mb as u32;
                        let name_bytes = name.as_bytes();
                        for i in 0..name_bytes.len().min(255) {
                            GPU_INFO.name[i] = name_bytes[i];
                        }
                        debug_println!("[create_device_and_queue] ✅ Selected: {} ({} MB VRAM)", name, vram_mb);
                    }
                }
            }
            adapter_index += 1;
        }

        // Если не нашли реальный GPU, используем WARP
        if best_device.is_none() {
            debug_println!("[create_device_and_queue] No real GPU found, using WARP...");
            let warp_adapter: IDXGIAdapter4 = match factory.EnumWarpAdapter() {
                Ok(a) => a,
                Err(e) => {
                    debug_println!("[create_device_and_queue] Failed to get WARP adapter: {:?}", e);
                    return ptr::null_mut();
                }
            };

            let mut temp_device: Option<ID3D12Device> = None;
            if D3D12CreateDevice(&warp_adapter, D3D_FEATURE_LEVEL_11_0, &mut temp_device).is_err() {
                debug_println!("[create_device_and_queue] Failed to create WARP device");
                return ptr::null_mut();
            }
            best_device = temp_device;

            let name = "WARP Software Renderer";
            let name_bytes = name.as_bytes();
            for i in 0..name_bytes.len().min(255) {
                GPU_INFO.name[i] = name_bytes[i];
            }
            GPU_INFO.is_real = false;
            GPU_INFO.vram_mb = 0;
            debug_println!("[create_device_and_queue] ⚠️ Using WARP software renderer");
        }

        let device = best_device.unwrap();
        let device_raw = device.as_raw() as *mut c_void;
        debug_println!("[create_device_and_queue] Device raw pointer: {:p}", device_raw);

        // Создаём очередь
        let queue_desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };

        let queue = match device.CreateCommandQueue::<ID3D12CommandQueue>(&queue_desc) {
            Ok(q) => q,
            Err(e) => {
                debug_println!("[create_device_and_queue] Failed to create queue: {:?}", e);
                return ptr::null_mut();
            }
        };
        let queue_raw = queue.as_raw() as *mut c_void;
        debug_println!("[create_device_and_queue] Queue raw pointer: {:p}", queue_raw);

        // Получаем размеры дескрипторов
        let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        let dsv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
        let cbv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

        {
            let mut state = match STATE.lock() {
                Ok(s) => s,
                Err(_) => return ptr::null_mut(),
            };
            state.device = Some(device.clone());
            state.command_queue = Some(queue.clone());
            state.rtv_descriptor_size = rtv_size;
            state.dsv_descriptor_size = dsv_size;
            state.cbv_srv_uav_descriptor_size = cbv_size;
        }

        let pair = Box::new(DeviceQueuePair {
            device: device_raw,
            queue: queue_raw,
        });

        let raw_ptr = Box::into_raw(pair) as *mut c_void;
        debug_println!("[create_device_and_queue] ✅ Created pair at {:p}", raw_ptr);
        raw_ptr
    }
}

#[no_mangle]
pub extern "C" fn get_device_from_pair(pair_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        if pair_ptr.is_null() {
            return ptr::null_mut();
        }
        let pair = &*(pair_ptr as *const DeviceQueuePair);
        pair.device
    }
}

#[no_mangle]
pub extern "C" fn get_device_from_state() -> *mut c_void {
    unsafe {
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        };
        match state.device.as_ref() {
            Some(d) => d.as_raw() as *mut c_void,
            None => ptr::null_mut(),
        }
    }
}

#[no_mangle]
pub extern "C" fn get_queue_from_pair(pair_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        if pair_ptr.is_null() {
            return ptr::null_mut();
        }
        let pair = &*(pair_ptr as *const DeviceQueuePair);
        pair.queue
    }
}

#[no_mangle]
pub extern "C" fn destroy_device_queue_pair(pair_ptr: *mut c_void) -> bool {
    if pair_ptr.is_null() {
        return false;
    }
    unsafe {
        let _ = Box::from_raw(pair_ptr as *mut DeviceQueuePair);
        debug_println!("[destroy_device_queue_pair] ✅ Pair destroyed");
        true
    }
}

#[no_mangle]
pub extern "C" fn create_device() -> *mut c_void {
    let pair = create_device_and_queue();
    get_device_from_pair(pair)
}

#[no_mangle]
pub extern "C" fn create_command_queue(_device_ptr: *mut c_void) -> *mut c_void {
    let pair = create_device_and_queue();
    get_queue_from_pair(pair)
}

#[no_mangle]
pub extern "C" fn destroy_device(_device_ptr: *mut c_void) -> bool {
    debug_println!("[destroy_device] Device will be cleaned by STATE");
    true
}

#[no_mangle]
pub extern "C" fn is_real_gpu() -> bool {
    unsafe { GPU_INFO.is_real }
}

#[no_mangle]
pub extern "C" fn get_gpu_name(_device_ptr: *mut c_void) -> *const i8 {
    unsafe { GPU_INFO.name.as_ptr() as *const i8 }
}

#[no_mangle]
pub extern "C" fn get_gpu_vram_mb() -> u32 {
    unsafe { GPU_INFO.vram_mb }
}

#[no_mangle]
pub extern "C" fn get_rtv_descriptor_size() -> u32 {
    STATE.lock().unwrap().rtv_descriptor_size
}

#[no_mangle]
pub extern "C" fn get_dsv_descriptor_size() -> u32 {
    STATE.lock().unwrap().dsv_descriptor_size
}

#[no_mangle]
pub extern "C" fn get_cbv_srv_uav_descriptor_size() -> u32 {
    STATE.lock().unwrap().cbv_srv_uav_descriptor_size
}

#[no_mangle]
pub extern "C" fn is_valid_device_ptr(ptr: *mut c_void) -> bool {
    if ptr.is_null() {
        debug_println!("[is_valid_device_ptr] ptr is null");
        return false;
    }
    unsafe {
        // Проверяем, можно ли создать COM объект из указателя
        let device = ID3D12Device::from_raw(ptr as *mut _);
        let raw = device.as_raw();
        let result = !raw.is_null();
        debug_println!("[is_valid_device_ptr] ptr={:p}, raw={:p}, valid={}", ptr, raw, result);
        result
    }
}