//! Управление D3D12 устройством с поддержкой WARP и реальных GPU

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Dxgi::*;
use windows_core::Interface;
use crate::{STATE, debug_println};

static mut GPU_INFO: GpuInfo = GpuInfo {
    is_real: false,
    name: [0; 256],
    vram_mb: 0,
    supports_raytracing: false,
};

struct GpuInfo {
    is_real: bool,
    name: [u8; 256],
    vram_mb: u32,
    supports_raytracing: bool,
}

#[no_mangle]
pub extern "C" fn create_device() -> *mut c_void {
    debug_println!("\n[create_device] Creating D3D12 device...");

    unsafe {
        // Сброс информации
        GPU_INFO = GpuInfo {
            is_real: false,
            name: [0; 256],
            vram_mb: 0,
            supports_raytracing: false,
        };

        let factory: IDXGIFactory4 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(e) => {
                debug_println!("[create_device] Failed to create DXGI factory: {:?}", e);
                return std::ptr::null_mut();
            }
        };

        let mut best_device: Option<ID3D12Device> = None;
        let mut best_adapter: Option<IDXGIAdapter1> = None;
        let mut max_vram = 0;
        let mut adapter_index = 0;

        // Перебираем все адаптеры, ищем лучший
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

            // Пропускаем WARP если есть реальные GPU
            let is_warp = name.contains("Microsoft Basic Render Driver") ||
                name.contains("WARP") ||
                name.contains("Software") ||
                vram_mb == 0;

            debug_println!("[create_device] Adapter {}: {} (VRAM: {} MB, WARP: {})",
                adapter_index, name, vram_mb, is_warp);

            if !is_warp && vram_mb > max_vram {
                // Проверяем, может ли адаптер создать устройство
                let mut temp_device: Option<ID3D12Device> = None;
                if D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut temp_device).is_ok() {
                    if temp_device.is_some() {
                        max_vram = vram_mb;
                        best_device = temp_device;
                        best_adapter = Some(adapter.clone());

                        // Сохраняем информацию
                        GPU_INFO.is_real = true;
                        GPU_INFO.vram_mb = vram_mb as u32;
                        let name_bytes = name.as_bytes();
                        for i in 0..name_bytes.len().min(255) {
                            GPU_INFO.name[i] = name_bytes[i];
                        }

                        debug_println!("[create_device] ✅ Selected: {} ({} MB VRAM)", name, vram_mb);
                    }
                }
            }

            adapter_index += 1;
        }

        // Если реальный GPU не найден, используем WARP
        if best_device.is_none() {
            debug_println!("[create_device] No real GPU found, using WARP...");

            let warp_adapter: IDXGIAdapter4 = match factory.EnumWarpAdapter() {
                Ok(a) => a,
                Err(e) => {
                    debug_println!("[create_device] Failed to get WARP adapter: {:?}", e);
                    return std::ptr::null_mut();
                }
            };

            let mut temp_device: Option<ID3D12Device> = None;
            if D3D12CreateDevice(&warp_adapter, D3D_FEATURE_LEVEL_11_0, &mut temp_device).is_err() {
                debug_println!("[create_device] Failed to create WARP device");
                return std::ptr::null_mut();
            }

            best_device = temp_device;
            GPU_INFO.is_real = false;
            let name = "WARP Software Renderer";
            let name_bytes = name.as_bytes();
            for i in 0..name_bytes.len().min(255) {
                GPU_INFO.name[i] = name_bytes[i];
            }
            debug_println!("[create_device] ⚠️ Using WARP software renderer");
        }

        let device = match best_device {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        // Получаем размеры дескрипторов
        let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        let dsv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
        let cbv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

        {
            let mut state = STATE.lock().unwrap();
            state.device = Some(device.clone());
            state.rtv_descriptor_size = rtv_size;
            state.dsv_descriptor_size = dsv_size;
            state.cbv_srv_uav_descriptor_size = cbv_size;
        }

        let raw_ptr = device.as_raw();
        std::mem::forget(device);

        debug_println!("[create_device] ✅ Device created successfully");
        raw_ptr as *mut c_void
    }
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