//! Управление D3D12 устройством

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Dxgi::*;
use windows_core::Interface;
use crate::{STATE, debug_println};

static mut REAL_GPU_FOUND: bool = false;
static mut GPU_NAME_STR: [u8; 256] = [0; 256];

#[no_mangle]
pub extern "C" fn create_device() -> *mut c_void {
    debug_println!("\n[create_device] Called");

    unsafe {
        // Сброс флагов
        REAL_GPU_FOUND = false;

        // Получаем фабрику
        let factory: IDXGIFactory4 = match CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) {
            Ok(f) => f,
            Err(_) => return std::ptr::null_mut(),
        };

        let mut device: Option<ID3D12Device> = None;
        let mut adapter_index = 0;

        // Перебираем все адаптеры
        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                Ok(a) => a,
                Err(_) => break,
            };

            // Получаем описание адаптера
            let desc = match adapter.GetDesc1() {
                Ok(d) => d,
                Err(_) => {
                    adapter_index += 1;
                    continue;
                }
            };

            let name = String::from_utf16_lossy(&desc.Description);
            let vram_mb = desc.DedicatedVideoMemory / 1024 / 1024;

            debug_println!("[create_device] Adapter {}: {} (VRAM: {} MB)", adapter_index, name, vram_mb);

            // Определяем WARP (Microsoft Basic Render Driver)
            let is_warp = name.contains("Microsoft Basic Render Driver") ||
                name.contains("WARP") ||
                name.contains("Software") ||
                vram_mb == 0;

            if !is_warp && vram_mb > 0 {
                debug_println!("[create_device] Trying REAL GPU: {}", name);

                let mut temp_device: Option<ID3D12Device> = None;
                // Передаём адаптер напрямую, без Option и без ссылки
                let hr = D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut temp_device);

                if hr.is_ok() {
                    if let Some(d) = temp_device {
                        debug_println!("[create_device] ✅ REAL GPU FOUND: {}", name);
                        device = Some(d);
                        REAL_GPU_FOUND = true;

                        // Сохраняем имя GPU
                        let name_bytes = name.as_bytes();
                        for i in 0..name_bytes.len().min(255) {
                            GPU_NAME_STR[i] = name_bytes[i];
                        }
                        GPU_NAME_STR[name_bytes.len().min(255)] = 0;

                        break;
                    }
                }
            }

            adapter_index += 1;
        }

        // Если реального GPU нет - создаём WARP устройство
        if device.is_none() {
            debug_println!("[create_device] No real GPU found, creating WARP device...");

            // Создаём WARP адаптер
            let warp_adapter: IDXGIAdapter4 = match factory.EnumWarpAdapter() {
                Ok(a) => a,
                Err(_) => return std::ptr::null_mut(),
            };

            let mut temp_device: Option<ID3D12Device> = None;
            let hr = D3D12CreateDevice(&warp_adapter, D3D_FEATURE_LEVEL_11_0, &mut temp_device);

            if hr.is_ok() {
                if let Some(d) = temp_device {
                    debug_println!("[create_device] ⚠️ WARP device created (software rendering)");
                    device = Some(d);
                    REAL_GPU_FOUND = false;

                    let name = "WARP Software Renderer";
                    let name_bytes = name.as_bytes();
                    for i in 0..name_bytes.len().min(255) {
                        GPU_NAME_STR[i] = name_bytes[i];
                    }
                    GPU_NAME_STR[name_bytes.len().min(255)] = 0;
                }
            }
        }

        let device = match device {
            Some(d) => d,
            None => {
                debug_println!("[create_device] ❌ FAILED to create any device!");
                return std::ptr::null_mut();
            }
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

        debug_println!("[create_device] Done, REAL_GPU={}", REAL_GPU_FOUND);

        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn is_real_gpu() -> bool {
    unsafe { REAL_GPU_FOUND }
}

#[no_mangle]
pub extern "C" fn is_warp_mode() -> bool {
    unsafe { !REAL_GPU_FOUND }
}

#[no_mangle]
pub extern "C" fn check_warp_driver(_device_ptr: *mut c_void) -> bool {
    unsafe { !REAL_GPU_FOUND }
}

#[no_mangle]
pub extern "C" fn get_gpu_name(_device_ptr: *mut c_void) -> *const i8 {
    unsafe {
        GPU_NAME_STR.as_ptr() as *const i8
    }
}

#[no_mangle]
pub extern "C" fn get_gpu_vram_mb(device_ptr: *mut c_void) -> u32 {
    unsafe {
        if !REAL_GPU_FOUND || device_ptr.is_null() {
            return 0;
        }

        let device: ID3D12Device = std::mem::transmute_copy(&device_ptr);

        let mut adapter_ptr: *mut c_void = std::ptr::null_mut();
        let iid = &IDXGIAdapter::IID;

        let hr = device.query(iid, &mut adapter_ptr);
        std::mem::forget(device);

        if hr.is_ok() && !adapter_ptr.is_null() {
            let adapter = std::mem::transmute::<*mut c_void, IDXGIAdapter>(adapter_ptr);
            let desc = adapter.GetDesc();
            std::mem::forget(adapter);

            match desc {
                Ok(desc) => (desc.DedicatedVideoMemory / 1024 / 1024) as u32,
                Err(_) => 0
            }
        } else {
            0
        }
    }
}