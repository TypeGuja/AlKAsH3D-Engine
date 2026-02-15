//! alkash3d_dx12 – Полноценная рабочая обертка над DirectX 12 для Python

#![allow(non_snake_case)]
#![allow(dead_code)]

use std::{
    ffi::{c_void, CStr, CString},
    mem::ManuallyDrop,
    ptr,
    sync::{LazyLock, Mutex},
};

use windows::{
    core::{PCSTR, PCWSTR},
    Win32::{
        Foundation::{HWND, FALSE, TRUE, RECT, CloseHandle},
        Graphics::{
            Direct3D::*,
            Direct3D12::*,
            Dxgi::*,
            Dxgi::Common::*,
        },
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryA},
            Threading::{CreateEventA, WaitForSingleObject, INFINITE},
        },
    },
};
use windows_core::{ComInterface, Interface, IUnknown};

// Флаг отладки - установите true для включения отладочного вывода
const DEBUG: bool = true;

macro_rules! debug_println {
    ($($arg:tt)*) => {
        if DEBUG {
            eprintln!($($arg)*);
        }
    };
}

/* ==================== ГЛОБАЛЬНОЕ СОСТОЯНИЕ ==================== */
struct GlobalState {
    device: Option<ID3D12Device>,
    command_queue: Option<ID3D12CommandQueue>,
    swap_chain: Option<IDXGISwapChain3>,
    command_list: Option<ID3D12GraphicsCommandList>,
    command_allocator: Option<ID3D12CommandAllocator>,
    root_signature: Option<ID3D12RootSignature>,
    rtv_descriptor_size: u32,
    dsv_descriptor_size: u32,
    cbv_srv_uav_descriptor_size: u32,
    frame_index: u32,
    fence: Option<ID3D12Fence>,
    fence_value: u64,
}

impl GlobalState {
    fn new() -> Self {
        Self {
            device: None,
            command_queue: None,
            swap_chain: None,
            command_list: None,
            command_allocator: None,
            root_signature: None,
            rtv_descriptor_size: 0,
            dsv_descriptor_size: 0,
            cbv_srv_uav_descriptor_size: 0,
            frame_index: 0,
            fence: None,
            fence_value: 0,
        }
    }
}

static STATE: LazyLock<Mutex<GlobalState>> = LazyLock::new(|| Mutex::new(GlobalState::new()));

/* ==================== УТИЛИТЫ ДЛЯ РАБОТЫ С УКАЗАТЕЛЯМИ ==================== */
mod ptr_utils {
    use super::*;

    pub unsafe fn as_device(ptr: *mut c_void) -> Option<ID3D12Device> {
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }

    pub unsafe fn as_queue(ptr: *mut c_void) -> Option<ID3D12CommandQueue> {
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }

    pub unsafe fn as_swapchain(ptr: *mut c_void) -> Option<IDXGISwapChain3> {
        if ptr.is_null() {
            return None;
        }

        let ptr_val = ptr as usize;
        if ptr_val == 0xDEADBEEF || ptr_val == 0xDEADF00D ||
            ptr_val == 0xFEEDC0DE || ptr_val == 0x12345678 ||
            ptr_val == 0x87654321 {
            return None;
        }

        let swap: IDXGISwapChain3 = std::mem::transmute_copy(&ptr);

        if swap.as_raw().is_null() {
            return None;
        }

        Some(swap)
    }

    pub unsafe fn as_resource(ptr: *mut c_void) -> Option<ID3D12Resource> {
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }

    pub unsafe fn as_blob(ptr: *mut c_void) -> Option<ID3DBlob> {
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }

    pub unsafe fn as_heap(ptr: *mut c_void) -> Option<ID3D12DescriptorHeap> {
        if ptr.is_null() {
            None
        } else {
            let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&ptr);
            Some(heap)
        }
    }

    pub unsafe fn as_pipeline_state(ptr: *mut c_void) -> Option<ID3D12PipelineState> {
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }
}

/* ==================== ROOT SIGNATURE ==================== */
mod root_sig {
    use super::*;

    pub unsafe fn create_graphics_root_signature(device: &ID3D12Device) -> Option<ID3D12RootSignature> {
        debug_println!("[root_sig] Creating graphics root signature...");

        // Диапазоны для таблицы дескрипторов
        let ranges = [
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
                NumDescriptors: 1,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
            },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 1,
                BaseShaderRegister: 0,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
            },
        ];

        // Статический сэмплер
        let samplers = [
            D3D12_STATIC_SAMPLER_DESC {
                Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
                AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
                AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
                MipLODBias: 0.0,
                MaxAnisotropy: 0,
                ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
                BorderColor: D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
                MinLOD: 0.0,
                MaxLOD: D3D12_FLOAT32_MAX,
                ShaderRegister: 0,
                RegisterSpace: 0,
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: ranges.len() as u32,
                        pDescriptorRanges: ranges.as_ptr(),
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
                DescriptorTable: Default::default(),
                Constants: Default::default(),
                Descriptor: Default::default(),
            },
        ];

        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: samplers.len() as u32,
            pStaticSamplers: samplers.as_ptr(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut blob: Option<ID3DBlob> = None;
        let mut err_blob: Option<ID3DBlob> = None;

        if let Err(e) = D3D12SerializeRootSignature(
            &root_desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut blob,
            Some(&mut err_blob),
        ) {
            debug_println!("[root_sig] Failed to serialize: HRESULT 0x{:X}", e.code().0);
            return None;
        }

        let root_blob = match blob {
            Some(b) => b,
            None => {
                debug_println!("[root_sig] Blob is None");
                return None;
            }
        };

        match device.CreateRootSignature(
            0,
            std::slice::from_raw_parts(
                root_blob.GetBufferPointer() as *const u8,
                root_blob.GetBufferSize(),
            ),
        ) {
            Ok(s) => {
                debug_println!("[root_sig] Created successfully");
                Some(s)
            },
            Err(e) => {
                debug_println!("[root_sig] Failed: HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }
}

/* ==================== УСТРОЙСТВО ==================== */
mod device_mod {
    use super::*;

    pub unsafe fn create_d3d12_device() -> Option<ID3D12Device> {
        debug_println!("[device] Creating D3D12 device...");

        let mut device_opt: Option<ID3D12Device> = None;

        let feature_levels = [
            D3D_FEATURE_LEVEL_12_0,
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
        ];

        for &level in &feature_levels {
            let hr = D3D12CreateDevice(None, level, &mut device_opt);
            if hr.is_ok() && device_opt.is_some() {
                debug_println!("[device] Created successfully with level {:?}", level);
                return device_opt;
            }
        }

        debug_println!("[device] Failed to create device");
        None
    }

    pub unsafe fn save_device_state(device: &ID3D12Device, root_sig: ID3D12RootSignature) {
        let rtv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        let dsv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
        let cbv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

        let mut state = STATE.lock().unwrap();
        state.device = Some(device.clone());
        state.root_signature = Some(root_sig.clone()); // Используем clone
        state.rtv_descriptor_size = rtv_sz;
        state.dsv_descriptor_size = dsv_sz;
        state.cbv_srv_uav_descriptor_size = cbv_sz;

        println!("[device] State saved - RTV: {}, DSV: {}, CBV: {}", rtv_sz, dsv_sz, cbv_sz);
    }
}

/* ==================== КОМАНДНАЯ ОЧЕРЕДЬ ==================== */
mod queue_mod {
    use super::*;

    pub unsafe fn create(device: &ID3D12Device) -> Option<ID3D12CommandQueue> {
        let desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };

        match device.CreateCommandQueue(&desc) {
            Ok(q) => {
                debug_println!("[queue] Created successfully");
                Some(q)
            },
            Err(e) => {
                debug_println!("[queue] Failed: HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }
}

/* ==================== КОМАНДНЫЙ АЛЛОКАТОР И СПИСОК ==================== */
mod command_mod {
    use super::*;

    pub unsafe fn create_allocator(device: &ID3D12Device) -> Option<ID3D12CommandAllocator> {
        println!("[command] create_allocator: calling CreateCommandAllocator...");

        match device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) {
            Ok(a) => {
                println!("[command] create_allocator: SUCCESS");
                Some(a)
            },
            Err(e) => {
                println!("[command] create_allocator: FAILED with HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }

    pub unsafe fn create_command_list(
        device: &ID3D12Device,
        allocator: &ID3D12CommandAllocator,
        pso: Option<&ID3D12PipelineState>,
    ) -> Option<ID3D12GraphicsCommandList> {
        println!("[command] create_command_list: calling CreateCommandList...");

        let result: Result<ID3D12GraphicsCommandList, _> = device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            allocator,
            pso,
        );

        match result {
            Ok(list) => {
                println!("[command] create_command_list: SUCCESS");

                println!("[command] create_command_list: closing list...");
                if let Err(e) = list.Close() {
                    println!("[command] create_command_list: close FAILED: {:?}", e);
                } else {
                    println!("[command] create_command_list: closed successfully");
                }
                Some(list)
            },
            Err(e) => {
                println!("[command] create_command_list: FAILED with HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }
}

/* ==================== СВОП ЧЕЙН ==================== */
mod swapchain_mod {
    use super::*;

    pub unsafe fn create(
        queue: &ID3D12CommandQueue,
        hwnd: usize,
        width: u32,
        height: u32,
    ) -> Option<IDXGISwapChain3> {
        debug_println!("[swapchain] Creating swap chain {}x{}", width, height);

        let factory: IDXGIFactory4 = match CreateDXGIFactory2(0) {
            Ok(f) => f,
            Err(e) => {
                debug_println!("[swapchain] Failed to create factory: HRESULT 0x{:X}", e.code().0);
                return None;
            }
        };

        let swap_desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
            Flags: 0,
        };

        let swap_chain1: IDXGISwapChain1 = match factory.CreateSwapChainForHwnd(
            queue,
            HWND(hwnd as isize),
            &swap_desc,
            None,
            None,
        ) {
            Ok(sc) => sc,
            Err(e) => {
                debug_println!("[swapchain] Failed to create: HRESULT 0x{:X}", e.code().0);
                return None;
            }
        };

        match swap_chain1.cast::<IDXGISwapChain3>() {
            Ok(sc) => {
                debug_println!("[swapchain] Created successfully");
                Some(sc)
            },
            Err(e) => {
                debug_println!("[swapchain] Failed to cast: HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }

    pub unsafe fn present(swap: &IDXGISwapChain3, sync_interval: u32) -> bool {
        debug_println!("[swapchain_mod::present] Calling Present({}, 0)", sync_interval);

        let hr = swap.Present(sync_interval, 0);

        if hr.is_ok() {
            let frame_idx = swap.GetCurrentBackBufferIndex();
            let mut state = STATE.lock().unwrap();
            state.frame_index = frame_idx;
            debug_println!("[swapchain_mod::present] Success, new frame index: {}", frame_idx);
            true
        } else {
            debug_println!("[swapchain_mod::present] Failed with HRESULT error");
            false
        }
    }

    pub unsafe fn resize(swap: &IDXGISwapChain3, width: u32, height: u32) -> bool {
        let hr = swap.ResizeBuffers(2, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0);
        if hr.is_err() {
            debug_println!("[swapchain] Resize failed");
            false
        } else {
            let mut state = STATE.lock().unwrap();
            state.frame_index = swap.GetCurrentBackBufferIndex();
            true
        }
    }
}

/* ==================== ДЕСКРИПТОРНЫЕ ХИПЫ ==================== */
mod heap_mod {
    use super::*;

    pub unsafe fn create(
        device: &ID3D12Device,
        num_descriptors: u32,
        heap_type: u32,
    ) -> Option<ID3D12DescriptorHeap> {
        let heap_ty = match heap_type {
            0 => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            1 => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            2 => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            _ => {
                debug_println!("[heap] Invalid type: {}", heap_type);
                return None;
            }
        };

        let flags = if heap_ty == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV {
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

        debug_println!("[heap] Creating with {} descriptors, type={}, flags={:?}",
                  num_descriptors, heap_type, flags);

        let result: Result<ID3D12DescriptorHeap, _> = device.CreateDescriptorHeap(&desc);

        match result {
            Ok(heap) => {
                debug_println!("[heap] Created successfully");
                Some(heap)
            },
            Err(e) => {
                debug_println!("[heap] Failed: HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn GetCPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> usize {
    debug_println!("\n[API] GetCPUDescriptorHandleForHeapStart({:p})", heap_ptr);

    if heap_ptr.is_null() {
        return 0;
    }

    unsafe {
        let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
        let handle = heap.GetCPUDescriptorHandleForHeapStart();
        let result = handle.ptr as usize;
        debug_println!("[API] GetCPUDescriptorHandleForHeapStart returning: {:#x}", result);
        std::mem::forget(heap);
        result
    }
}

#[no_mangle]
pub extern "C" fn GetGPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> usize {
    debug_println!("\n[API] GetGPUDescriptorHandleForHeapStart({:p})", heap_ptr);

    if heap_ptr.is_null() {
        return 0;
    }

    unsafe {
        let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
        let handle = heap.GetGPUDescriptorHandleForHeapStart();
        let result = handle.ptr as usize;
        debug_println!("[API] GetGPUDescriptorHandleForHeapStart returning: {:#x}", result);
        std::mem::forget(heap);
        result
    }
}

/* ==================== RENDER TARGET ==================== */
#[no_mangle]
pub unsafe extern "C" fn set_render_target(rtv: usize) {
    debug_println!("\n[API] set_render_target({:#x})", rtv);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv };
        list.OMSetRenderTargets(1, Some(&rtv_handle), false, None);
    }
}

#[no_mangle]
pub unsafe extern "C" fn clear_render_target(rtv: usize, color: *const f32) {
    debug_println!("\n[API] clear_render_target({:#x})", rtv);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv };
        let color_array: [f32; 4] = if color.is_null() {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            let slice = std::slice::from_raw_parts(color, 4);
            [slice[0], slice[1], slice[2], slice[3]]
        };
        list.ClearRenderTargetView(rtv_handle, &color_array, None);
    }
}

/* ==================== БУФЕРЫ ==================== */
mod buffer_mod {
    use super::*;

    pub unsafe fn create_upload(
        device: &ID3D12Device,
        size: usize,
    ) -> Option<ID3D12Resource> {
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
            height: 0,
        };

        let mut resource_opt: Option<ID3D12Resource> = None;
        let hr = device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut resource_opt,
        );

        if let Err(e) = hr {
            debug_println!("[buffer] Failed to create: HRESULT 0x{:X}", e.code().0);
            return None;
        }

        resource_opt
    }

    pub unsafe fn update(
        resource: &ID3D12Resource,
        data: *const c_void,
        size: usize,
    ) -> bool {
        let mut mapped: *mut c_void = ptr::null_mut();
        if let Err(e) = resource.Map(0, None, Some(&mut mapped)) {
            debug_println!("[buffer] Failed to map: HRESULT 0x{:X}", e.code().0);
            return false;
        }

        if !mapped.is_null() && !data.is_null() {
            std::ptr::copy_nonoverlapping(data as *const u8, mapped as *mut u8, size);
        }

        resource.Unmap(0, None);
        true
    }
}

/* ==================== ТЕКСТУРЫ ==================== */
mod texture_mod {
    use super::*;

    pub unsafe fn create_2d(
        device: &ID3D12Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Option<ID3D12Resource> {
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
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_NONE,
            height: 0,
        };

        let mut texture_opt: Option<ID3D12Resource> = None;
        let hr = device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut texture_opt,
        );

        if let Err(e) = hr {
            debug_println!("[texture] Failed to create: HRESULT 0x{:X}", e.code().0);
            return None;
        }

        texture_opt
    }

    pub unsafe fn update(
        texture: &ID3D12Resource,
        data: *const c_void,
        width: u32,
        height: u32,
        bpp: usize,
    ) -> bool {
        let mut mapped: *mut c_void = ptr::null_mut();
        if let Err(e) = texture.Map(0, None, Some(&mut mapped)) {
            debug_println!("[texture] Failed to map: HRESULT 0x{:X}", e.code().0);
            return false;
        }

        if !mapped.is_null() && !data.is_null() {
            let row_pitch = (width as usize) * bpp;
            let slice_pitch = row_pitch * height as usize;
            std::ptr::copy_nonoverlapping(data as *const u8, mapped as *mut u8, slice_pitch);
        }

        texture.Unmap(0, None);
        true
    }
}

/* ==================== ШЕЙДЕРЫ ==================== */
mod shader_mod {
    use super::*;

    pub unsafe fn compile_from_file(
        file_path: *const u16,
        entry_point: *const u8,
        profile: *const u8,
    ) -> Option<ID3DBlob> {
        debug_println!("\n[shader] Compiling from file...");

        if file_path.is_null() {
            debug_println!("[shader] ERROR: file_path is null");
            return None;
        }
        if entry_point.is_null() {
            debug_println!("[shader] ERROR: entry_point is null");
            return None;
        }
        if profile.is_null() {
            debug_println!("[shader] ERROR: profile is null");
            return None;
        }

        let dll_name = match CString::new("d3dcompiler_47.dll") {
            Ok(s) => s,
            Err(e) => {
                debug_println!("[shader] Failed to create dll name: {:?}", e);
                return None;
            }
        };

        let lib = match LoadLibraryA(PCSTR(dll_name.as_ptr() as *const u8)) {
            Ok(h) => h,
            Err(e) => {
                debug_println!("[shader] Failed to load d3dcompiler_47.dll: {:?}", e);
                return None;
            }
        };

        let proc_name = match CString::new("D3DCompileFromFile") {
            Ok(s) => s,
            Err(e) => {
                debug_println!("[shader] Failed to create proc name: {:?}", e);
                return None;
            }
        };

        let fn_ptr = match GetProcAddress(lib, PCSTR(proc_name.as_ptr() as *const u8)) {
            Some(p) => p,
            None => {
                debug_println!("[shader] Failed to get D3DCompileFromFile address");
                return None;
            }
        };

        type D3DCompileFromFileFn = unsafe extern "system" fn(
            PCWSTR,
            *const std::ffi::c_void,
            *mut std::ffi::c_void,
            PCSTR,
            PCSTR,
            u32,
            u32,
            *mut *mut ID3DBlob,
            *mut *mut ID3DBlob,
        ) -> windows::core::HRESULT;

        let compile: D3DCompileFromFileFn = std::mem::transmute(fn_ptr);

        let mut shader_blob: *mut ID3DBlob = std::ptr::null_mut();
        let mut err_blob: *mut ID3DBlob = std::ptr::null_mut();

        let flags1 = 0x0001;
        let flags2 = 0;

        let hr = compile(
            PCWSTR(file_path),
            std::ptr::null(),
            std::ptr::null_mut(),
            PCSTR(entry_point),
            PCSTR(profile),
            flags1,
            flags2,
            &mut shader_blob,
            &mut err_blob,
        );

        if hr.is_ok() {
            if !shader_blob.is_null() {
                return Some(std::mem::transmute_copy(&shader_blob));
            }
        }

        if !err_blob.is_null() {
            let _ = IUnknown::from_raw(err_blob as *mut c_void);
        }

        None
    }
}

/* ==================== PSO ==================== */
mod pso_mod {
    use super::*;

    pub unsafe fn create_graphics(
        device: &ID3D12Device,
        root_sig: &ID3D12RootSignature,
        vs_blob: &ID3DBlob,
        ps_blob: &ID3DBlob,
    ) -> Option<ID3D12PipelineState> {
        let vs_size = vs_blob.GetBufferSize();
        let ps_size = ps_blob.GetBufferSize();

        debug_println!("[pso] Creating PSO with VS size: {}, PS size: {}", vs_size, ps_size);

        let input_elements = [
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR("POSITION\0".as_ptr() as *const u8),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR("NORMAL\0".as_ptr() as *const u8),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR("TEXCOORD\0".as_ptr() as *const u8),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];

        let input_layout = D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        };

        let vs_bc = D3D12_SHADER_BYTECODE {
            pShaderBytecode: vs_blob.GetBufferPointer(),
            BytecodeLength: vs_size,
        };

        let ps_bc = D3D12_SHADER_BYTECODE {
            pShaderBytecode: ps_blob.GetBufferPointer(),
            BytecodeLength: ps_size,
        };

        let mut pso_desc = std::mem::zeroed::<D3D12_GRAPHICS_PIPELINE_STATE_DESC>();

        pso_desc.pRootSignature = ManuallyDrop::new(Some(root_sig.clone()));
        pso_desc.VS = vs_bc;
        pso_desc.PS = ps_bc;
        pso_desc.BlendState = D3D12_BLEND_DESC {
            AlphaToCoverageEnable: FALSE,
            IndependentBlendEnable: FALSE,
            RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC {
                BlendEnable: FALSE,
                LogicOpEnable: FALSE,
                SrcBlend: D3D12_BLEND_ONE,
                DestBlend: D3D12_BLEND_ZERO,
                BlendOp: D3D12_BLEND_OP_ADD,
                SrcBlendAlpha: D3D12_BLEND_ONE,
                DestBlendAlpha: D3D12_BLEND_ZERO,
                BlendOpAlpha: D3D12_BLEND_OP_ADD,
                LogicOp: D3D12_LOGIC_OP_NOOP,
                RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
            }; 8],
        };
        pso_desc.SampleMask = u32::MAX;
        pso_desc.RasterizerState = D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_BACK,
            FrontCounterClockwise: FALSE,
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: TRUE,
            MultisampleEnable: FALSE,
            AntialiasedLineEnable: FALSE,
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        };
        pso_desc.DepthStencilState = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: FALSE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
            StencilEnable: FALSE,
            StencilReadMask: D3D12_DEFAULT_STENCIL_READ_MASK as u8,
            StencilWriteMask: D3D12_DEFAULT_STENCIL_WRITE_MASK as u8,
            FrontFace: D3D12_DEPTH_STENCILOP_DESC::default(),
            BackFace: D3D12_DEPTH_STENCILOP_DESC::default(),
        };
        pso_desc.InputLayout = input_layout;
        pso_desc.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
        pso_desc.NumRenderTargets = 1;
        pso_desc.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
        pso_desc.SampleDesc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
        pso_desc.Flags = D3D12_PIPELINE_STATE_FLAG_NONE;

        match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc) {
            Ok(pso) => {
                debug_println!("[pso] Created successfully");
                Some(pso)
            },
            Err(e) => {
                debug_println!("[pso] Failed: HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }
}

/* ==================== VIEWS ==================== */
mod view_mod {
    use super::*;

    pub unsafe fn create_srv(
        device: &ID3D12Device,
        resource: &ID3D12Resource,
        cpu_handle: usize,
    ) {
        let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle };
        device.CreateShaderResourceView(resource, None, handle);
        debug_println!("[view] SRV created at {:#x}", cpu_handle);
    }

    pub unsafe fn create_rtv(
        device: &ID3D12Device,
        resource: &ID3D12Resource,
        cpu_handle: usize,
    ) {
        let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle };
        device.CreateRenderTargetView(resource, None, handle);
        debug_println!("[view] RTV created at {:#x}", cpu_handle);
    }
}

/* ==================== ОСВОБОЖДЕНИЕ ==================== */
mod release_mod {
    use super::*;

    pub unsafe fn release_resource(ptr: *mut c_void) {
        if ptr.is_null() {
            return;
        }

        debug_println!("[release] Releasing resource at {:p}", ptr);

        let ptr_val = ptr as usize;
        if ptr_val == 0xDEADBEEF || ptr_val == 0xDEADF00D ||
            ptr_val == 0xFEEDC0DE || ptr_val == 0x12345678 ||
            ptr_val == 0x87654321 {
            debug_println!("[release] Skipping stub pointer: {:#x}", ptr_val);
            return;
        }

        let _ = std::panic::catch_unwind(|| {
            let unknown = IUnknown::from_raw(ptr);
            std::mem::drop(unknown);
        });
    }
}

/* ==================== ЭКСПОРТИРУЕМЫЕ ФУНКЦИИ ==================== */

#[no_mangle]
pub extern "C" fn release_resource(res_ptr: *mut c_void) {
    debug_println!("\n[API] release_resource({:p})", res_ptr);

    if res_ptr.is_null() {
        return;
    }

    let ptr_val = res_ptr as usize;
    if ptr_val == 0xDEADBEEF || ptr_val == 0xDEADF00D ||
        ptr_val == 0xFEEDC0DE || ptr_val == 0x12345678 ||
        ptr_val == 0x87654321 {
        debug_println!("[release] Skipping stub pointer: {:#x}", ptr_val);
        return;
    }

    unsafe {
        release_mod::release_resource(res_ptr);
    }
}

#[no_mangle]
pub extern "C" fn create_device() -> *mut c_void {
    println!("\n[API] create_device() called - НАЧАЛО");

    unsafe {
        println!("[API] Creating D3D12 device...");
        let device = match device_mod::create_d3d12_device() {
            Some(d) => {
                println!("[API] Device created successfully");
                d
            },
            None => {
                println!("[API] Failed to create device");
                return ptr::null_mut();
            }
        };

        println!("[API] Creating root signature...");
        let root_sig = match root_sig::create_graphics_root_signature(&device) {
            Some(s) => {
                println!("[API] Root signature created successfully");
                s
            },
            None => {
                println!("[API] Failed to create root signature");
                return ptr::null_mut();
            }
        };

        println!("[API] Creating command allocator...");
        let allocator = match command_mod::create_allocator(&device) {
            Some(a) => {
                println!("[API] Command allocator created successfully");
                a
            },
            None => {
                println!("[API] Failed to create command allocator");
                return ptr::null_mut();
            }
        };

        println!("[API] Creating command list...");
        let command_list = match command_mod::create_command_list(&device, &allocator, None) {
            Some(l) => {
                println!("[API] Command list created successfully");
                l
            },
            None => {
                println!("[API] Failed to create command list");
                return ptr::null_mut();
            }
        };

        println!("[API] Creating fence...");
        let fence: ID3D12Fence = match device.CreateFence(0, D3D12_FENCE_FLAG_NONE) {
            Ok(f) => {
                println!("[API] Fence created successfully");
                f
            },
            Err(e) => {
                println!("[API] Failed to create fence: HRESULT 0x{:X}", e.code().0);
                return ptr::null_mut();
            }
        };

        println!("[API] Saving state...");
        {
            let mut state = STATE.lock().unwrap();
            state.device = Some(device.clone());
            state.command_allocator = Some(allocator.clone());
            state.command_list = Some(command_list.clone());
            state.fence = Some(fence.clone());
            state.root_signature = Some(root_sig.clone());

            println!("[API] State saved, getting descriptor sizes...");
            let rtv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let dsv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
            let cbv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

            state.rtv_descriptor_size = rtv_sz;
            state.dsv_descriptor_size = dsv_sz;
            state.cbv_srv_uav_descriptor_size = cbv_sz;

            println!("[API] Descriptor sizes: RTV={}, DSV={}, CBV={}", rtv_sz, dsv_sz, cbv_sz);
        }

        let raw_ptr = device.as_raw();
        println!("[API] Device raw pointer: {:p}", raw_ptr);

        std::mem::forget(device);
        std::mem::forget(root_sig);
        std::mem::forget(allocator);
        std::mem::forget(command_list);
        std::mem::forget(fence);

        println!("[API] All objects forgotten, returning pointer");
        println!("[API] create_device() - КОНЕЦ");

        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn create_command_queue(device_ptr: *mut c_void) -> *mut c_void {
    debug_println!("\n[API] create_command_queue() called");

    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[API] Invalid device");
                return ptr::null_mut();
            }
        };

        let queue = match queue_mod::create(&device) {
            Some(q) => q,
            None => return ptr::null_mut(),
        };

        let mut state = STATE.lock().unwrap();
        state.command_queue = Some(queue.clone());

        let raw_ptr = queue.as_raw();
        std::mem::forget(queue);

        debug_println!("[API] Queue created at {:p}", raw_ptr);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn create_swap_chain(
    queue_ptr: *mut c_void,
    hwnd: usize,
    width: u32,
    height: u32,
) -> *mut c_void {
    debug_println!("\n[API] create_swap_chain({}x{})", width, height);

    unsafe {
        use ptr_utils::*;

        let queue = match as_queue(queue_ptr) {
            Some(q) => q,
            None => {
                debug_println!("[API] Invalid queue");
                return ptr::null_mut();
            }
        };

        let swap = match swapchain_mod::create(&queue, hwnd, width, height) {
            Some(s) => s,
            None => return ptr::null_mut(),
        };

        let mut state = STATE.lock().unwrap();
        state.swap_chain = Some(swap.clone());
        state.frame_index = swap.GetCurrentBackBufferIndex();

        let raw_ptr = swap.as_raw();
        std::mem::forget(swap);

        debug_println!("[API] Swap chain created at {:p}", raw_ptr);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) {
    debug_println!("\n[API] present_swap_chain({:p}, {})", swap_ptr, sync_interval);

    if swap_ptr.is_null() {
        return;
    }

    let ptr_val = swap_ptr as usize;
    if ptr_val == 0xDEADBEEF || ptr_val == 0xDEADF00D ||
        ptr_val == 0xFEEDC0DE || ptr_val == 0x12345678 ||
        ptr_val == 0x87654321 {
        return;
    }

    unsafe {
        use ptr_utils::*;

        if let Some(swap) = as_swapchain(swap_ptr) {
            swapchain_mod::present(&swap, sync_interval);
            std::mem::forget(swap);
        }
    }
}

#[no_mangle]
pub extern "C" fn resize_swap_chain(swap_ptr: *mut c_void, width: u32, height: u32) {
    debug_println!("\n[API] resize_swap_chain({}x{})", width, height);

    unsafe {
        use ptr_utils::*;

        if let Some(swap) = as_swapchain(swap_ptr) {
            swapchain_mod::resize(&swap, width, height);
            std::mem::forget(swap);
        }
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(
    swap_ptr: *mut c_void,
    buffer_index: u32,
) -> *mut c_void {
    debug_println!("\n[API] swap_chain_get_buffer({})", buffer_index);

    unsafe {
        use ptr_utils::*;

        let swap_chain = match as_swapchain(swap_ptr) {
            Some(s) => s,
            None => {
                debug_println!("[API] Invalid swap chain");
                return ptr::null_mut();
            }
        };

        let buffer_result: Result<ID3D12Resource, windows::core::Error> = swap_chain.GetBuffer(buffer_index);

        std::mem::forget(swap_chain);

        match buffer_result {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw();
                std::mem::forget(buffer);
                debug_println!("[API] Buffer {} created at {:p}", buffer_index, raw_ptr);
                raw_ptr as *mut c_void
            },
            Err(e) => {
                debug_println!("[API] Failed to get buffer: HRESULT 0x{:X}", e.code().0);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn create_descriptor_heap(
    device_ptr: *mut c_void,
    num_descriptors: u32,
    heap_type: u32,
) -> *mut c_void {
    debug_println!("\n[API] create_descriptor_heap({}, type={})", num_descriptors, heap_type);

    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[API] Invalid device");
                return ptr::null_mut();
            }
        };

        if num_descriptors == 0 || num_descriptors > 1000000 {
            debug_println!("[API] Invalid number of descriptors: {}", num_descriptors);
            return ptr::null_mut();
        }

        let heap = match heap_mod::create(&device, num_descriptors, heap_type) {
            Some(h) => h,
            None => return ptr::null_mut(),
        };

        let raw_ptr = heap.as_raw();
        std::mem::forget(heap);

        debug_println!("[API] Heap created at {:p}", raw_ptr);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn offset_descriptor_handle(start: usize, offset: u32) -> usize {
    let state = STATE.lock().unwrap();
    let increment = state.rtv_descriptor_size as usize;
    let result = start + (offset as usize) * increment;
    debug_println!("[API] offset_descriptor_handle({:#x}, {}) -> {:#x} (increment: {})",
              start, offset, result, increment);
    result
}

#[no_mangle]
pub extern "C" fn create_buffer(
    device_ptr: *mut c_void,
    size: usize,
    _usage: *const u8,
) -> *mut c_void {
    debug_println!("\n[API] create_buffer({})", size);

    unsafe {
        use ptr_utils::*;

        if size == 0 || size > 1024 * 1024 * 1024 {
            debug_println!("[API] Invalid buffer size: {}", size);
            return ptr::null_mut();
        }

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[API] Invalid device");
                return ptr::null_mut();
            }
        };

        let buffer = match buffer_mod::create_upload(&device, size) {
            Some(b) => b,
            None => return ptr::null_mut(),
        };

        let raw_ptr = buffer.as_raw();
        std::mem::forget(buffer);

        debug_println!("[API] Buffer created at {:p}", raw_ptr);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn update_subresource(
    buffer_ptr: *mut c_void,
    data_ptr: *const c_void,
    size: usize,
) {
    debug_println!("\n[API] update_subresource({})", size);

    unsafe {
        use ptr_utils::*;

        if let Some(buffer) = as_resource(buffer_ptr) {
            buffer_mod::update(&buffer, data_ptr, size);
            std::mem::forget(buffer);
        }
    }
}

#[no_mangle]
pub extern "C" fn create_texture_from_memory(
    device_ptr: *mut c_void,
    data_ptr: *const c_void,
    width: u32,
    height: u32,
    format: *const u8,
) -> *mut c_void {
    debug_println!("\n[API] create_texture_from_memory({}x{})", width, height);

    unsafe {
        use ptr_utils::*;

        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            debug_println!("[API] Invalid texture dimensions: {}x{}", width, height);
            return ptr::null_mut();
        }

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[API] Invalid device");
                return ptr::null_mut();
            }
        };

        let fmt_str = if format.is_null() {
            "rgba8"
        } else {
            CStr::from_ptr(format as *const i8).to_str().unwrap_or("rgba8")
        };

        let dxgi_format = match fmt_str.to_ascii_lowercase().as_str() {
            "rgba8" | "rgba8unorm" => DXGI_FORMAT_R8G8B8A8_UNORM,
            "rgba16f" => DXGI_FORMAT_R16G16B16A16_FLOAT,
            "rgba32f" => DXGI_FORMAT_R32G32B32A32_FLOAT,
            _ => DXGI_FORMAT_R8G8B8A8_UNORM,
        };

        let texture = match texture_mod::create_2d(&device, width, height, dxgi_format) {
            Some(t) => t,
            None => return ptr::null_mut(),
        };

        if !data_ptr.is_null() {
            let bpp = match dxgi_format {
                DXGI_FORMAT_R8G8B8A8_UNORM => 4,
                DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
                DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
                _ => 4,
            };
            texture_mod::update(&texture, data_ptr, width, height, bpp);
        }

        let raw_ptr = texture.as_raw();
        std::mem::forget(texture);

        debug_println!("[API] Texture created at {:p}", raw_ptr);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn update_texture(
    texture_ptr: *mut c_void,
    data_ptr: *const c_void,
    width: u32,
    height: u32,
) {
    debug_println!("\n[API] update_texture({}x{})", width, height);

    unsafe {
        use ptr_utils::*;

        if let Some(texture) = as_resource(texture_ptr) {
            texture_mod::update(&texture, data_ptr, width, height, 4);
            std::mem::forget(texture);
        }
    }
}

#[no_mangle]
pub extern "C" fn compile_shader(
    file_path: *const u16,
    entry_point: *const u8,
    profile: *const u8,
    out_blob: *mut *mut c_void,
) -> i32 {
    if file_path.is_null() || entry_point.is_null() || profile.is_null() || out_blob.is_null() {
        return -1;
    }

    unsafe {
        ptr::write(out_blob, ptr::null_mut());

        let result = shader_mod::compile_from_file(file_path, entry_point, profile);

        match result {
            Some(blob) => {
                let raw_ptr = blob.as_raw();
                std::mem::forget(blob);
                ptr::write(out_blob, raw_ptr as *mut c_void);
                0
            },
            None => -1
        }
    }
}

#[no_mangle]
pub extern "C" fn create_shader_resource_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: usize,
) {
    debug_println!("\n[API] create_shader_resource_view({:#x})", cpu_handle);

    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[API] Invalid device");
                return;
            }
        };

        let resource = match as_resource(resource_ptr) {
            Some(r) => r,
            None => {
                debug_println!("[API] Invalid resource");
                return;
            }
        };

        view_mod::create_srv(&device, &resource, cpu_handle);
        std::mem::forget(device);
        std::mem::forget(resource);
    }
}

#[no_mangle]
pub extern "C" fn create_render_target_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: usize,
) {
    debug_println!("\n[API] create_render_target_view({:#x})", cpu_handle);

    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                debug_println!("[API] Invalid device");
                return;
            }
        };

        let resource = match as_resource(resource_ptr) {
            Some(r) => r,
            None => {
                debug_println!("[API] Invalid resource");
                return;
            }
        };

        view_mod::create_rtv(&device, &resource, cpu_handle);
        std::mem::forget(device);
        std::mem::forget(resource);
    }
}

#[no_mangle]
pub extern "C" fn create_graphics_ps(
    device_ptr: *mut c_void,
    vs_blob_ptr: *mut c_void,
    ps_blob_ptr: *mut c_void,
) -> *mut c_void {
    println!("\n[API] create_graphics_ps() called");
    println!("  device_ptr: {:p}", device_ptr);
    println!("  vs_blob_ptr: {:p}", vs_blob_ptr);
    println!("  ps_blob_ptr: {:p}", ps_blob_ptr);

    unsafe {
        use ptr_utils::*;

        if device_ptr.is_null() {
            println!("[API] ERROR: device_ptr is null");
            return ptr::null_mut();
        }

        let device = match as_device(device_ptr) {
            Some(d) => {
                println!("[API] Device OK");
                d
            },
            None => {
                println!("[API] ERROR: Failed to convert device");
                return ptr::null_mut();
            }
        };

        if vs_blob_ptr.is_null() {
            println!("[API] ERROR: vs_blob_ptr is null");
            return ptr::null_mut();
        }

        let vs_blob = match as_blob(vs_blob_ptr) {
            Some(b) => {
                let size = b.GetBufferSize();
                println!("[API] VS blob OK, size: {} bytes", size);
                b
            },
            None => {
                println!("[API] ERROR: Failed to convert VS blob");
                return ptr::null_mut();
            }
        };

        if ps_blob_ptr.is_null() {
            println!("[API] ERROR: ps_blob_ptr is null");
            return ptr::null_mut();
        }

        let ps_blob = match as_blob(ps_blob_ptr) {
            Some(b) => {
                let size = b.GetBufferSize();
                println!("[API] PS blob OK, size: {} bytes", size);
                b
            },
            None => {
                println!("[API] ERROR: Failed to convert PS blob");
                return ptr::null_mut();
            }
        };

        let root_sig = match STATE.lock().unwrap().root_signature.clone() {
            Some(s) => {
                println!("[API] Root signature OK");
                s
            },
            None => {
                println!("[API] ERROR: No root signature");
                return ptr::null_mut();
            }
        };

        let input_elements = [
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR("POSITION\0".as_ptr() as *const u8),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR("NORMAL\0".as_ptr() as *const u8),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR("TEXCOORD\0".as_ptr() as *const u8),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];

        let input_layout = D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        };
        println!("[API] Input layout created with {} elements", input_elements.len());

        let mut pso_desc = std::mem::zeroed::<D3D12_GRAPHICS_PIPELINE_STATE_DESC>();

        pso_desc.pRootSignature = ManuallyDrop::new(Some(root_sig));
        pso_desc.VS = D3D12_SHADER_BYTECODE {
            pShaderBytecode: vs_blob.GetBufferPointer(),
            BytecodeLength: vs_blob.GetBufferSize(),
        };
        pso_desc.PS = D3D12_SHADER_BYTECODE {
            pShaderBytecode: ps_blob.GetBufferPointer(),
            BytecodeLength: ps_blob.GetBufferSize(),
        };
        pso_desc.BlendState = D3D12_BLEND_DESC {
            AlphaToCoverageEnable: FALSE,
            IndependentBlendEnable: FALSE,
            RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC {
                BlendEnable: FALSE,
                LogicOpEnable: FALSE,
                SrcBlend: D3D12_BLEND_ONE,
                DestBlend: D3D12_BLEND_ZERO,
                BlendOp: D3D12_BLEND_OP_ADD,
                SrcBlendAlpha: D3D12_BLEND_ONE,
                DestBlendAlpha: D3D12_BLEND_ZERO,
                BlendOpAlpha: D3D12_BLEND_OP_ADD,
                LogicOp: D3D12_LOGIC_OP_NOOP,
                RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
            }; 8],
        };
        pso_desc.SampleMask = u32::MAX;
        pso_desc.RasterizerState = D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_BACK,
            FrontCounterClockwise: FALSE,
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: TRUE,
            MultisampleEnable: FALSE,
            AntialiasedLineEnable: FALSE,
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        };
        pso_desc.DepthStencilState = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: FALSE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
            StencilEnable: FALSE,
            StencilReadMask: D3D12_DEFAULT_STENCIL_READ_MASK as u8,
            StencilWriteMask: D3D12_DEFAULT_STENCIL_WRITE_MASK as u8,
            FrontFace: D3D12_DEPTH_STENCILOP_DESC::default(),
            BackFace: D3D12_DEPTH_STENCILOP_DESC::default(),
        };
        pso_desc.InputLayout = input_layout;
        pso_desc.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
        pso_desc.NumRenderTargets = 1;
        pso_desc.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
        pso_desc.SampleDesc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
        pso_desc.Flags = D3D12_PIPELINE_STATE_FLAG_NONE;

        match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc) {
            Ok(pso) => {
                let raw_ptr = pso.as_raw();
                std::mem::forget(pso);
                println!("[API] PSO created successfully at {:p}", raw_ptr);
                raw_ptr as *mut c_void
            },
            Err(e) => {
                println!("[API] Failed to create PSO: HRESULT 0x{:X}", e.code().0);
                ptr::null_mut()
            }
        }
    }
}

/* ==================== НОВЫЕ ФУНКЦИИ ==================== */

#[no_mangle]
pub unsafe extern "C" fn begin_frame() {
    debug_println!("\n[API] begin_frame() called");

    let (allocator, list) = {
        let state = STATE.lock().unwrap();
        (
            state.command_allocator.clone(),
            state.command_list.clone(),
        )
    };

    if let Some(ref allocator) = allocator {
        let _ = allocator.Reset();
    }

    if let (Some(ref allocator), Some(ref list)) = (allocator.as_ref(), list.as_ref()) {
        let _ = list.Reset(*allocator, None);
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_graphics_pipeline(pso_ptr: *mut c_void) {
    debug_println!("\n[API] set_graphics_pipeline({:p})", pso_ptr);

    use ptr_utils::*;

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        if let Some(pso) = as_pipeline_state(pso_ptr) {
            list.SetPipelineState(&pso);
            std::mem::forget(pso);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_root_descriptor_table(root_index: u32, gpu_handle: u64) {
    debug_println!("\n[API] set_root_descriptor_table({}, {:#x})", root_index, gpu_handle);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let handle = D3D12_GPU_DESCRIPTOR_HANDLE { ptr: gpu_handle };
        list.SetGraphicsRootDescriptorTable(root_index, handle);
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_descriptor_heaps(count: usize, heaps: *const *mut c_void) {
    debug_println!("\n[API] set_descriptor_heaps({})", count);

    if count == 0 || heaps.is_null() {
        return;
    }

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let mut heap_ptrs = Vec::with_capacity(count);
        for i in 0..count {
            let heap_ptr = *heaps.add(i);
            if !heap_ptr.is_null() {
                let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
                heap_ptrs.push(Some(heap));
            }
        }
        list.SetDescriptorHeaps(&heap_ptrs);
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_render_targets(count: usize, rtvs: *const usize) {
    debug_println!("\n[API] set_render_targets({})", count);

    if count == 0 || rtvs.is_null() {
        return;
    }

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let mut rtv_handles = Vec::with_capacity(count);
        for i in 0..count {
            let rtv = *rtvs.add(i);
            rtv_handles.push(D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv });
        }
        list.OMSetRenderTargets(count as u32, Some(rtv_handles.as_ptr()), false, None);
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_viewport(x: i32, y: i32, w: i32, h: i32, min_depth: f32, max_depth: f32) {
    debug_println!("\n[API] set_viewport({}, {}, {}, {}, {}, {})", x, y, w, h, min_depth, max_depth);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let viewport = D3D12_VIEWPORT {
            TopLeftX: x as f32,
            TopLeftY: y as f32,
            Width: w as f32,
            Height: h as f32,
            MinDepth: min_depth,
            MaxDepth: max_depth,
        };
        list.RSSetViewports(&[viewport]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_scissor_rect(left: i32, top: i32, right: i32, bottom: i32) {
    debug_println!("\n[API] set_scissor_rect({}, {}, {}, {})", left, top, right, bottom);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let rect = RECT { left, top, right, bottom };
        list.RSSetScissorRects(&[rect]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_vertex_buffers(vertex_buffer: *mut c_void, index_buffer: *mut c_void) {
    debug_println!("\n[API] set_vertex_buffers({:p}, {:p})", vertex_buffer, index_buffer);

    use ptr_utils::*;

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        if !vertex_buffer.is_null() {
            if let Some(buffer) = as_resource(vertex_buffer) {
                let view = D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: buffer.GetGPUVirtualAddress(),
                    SizeInBytes: 1024 * 1024,
                    StrideInBytes: 32,
                };
                list.IASetVertexBuffers(0, Some(&[view]));
                std::mem::forget(buffer);
            }
        }

        if !index_buffer.is_null() {
            if let Some(buffer) = as_resource(index_buffer) {
                let view = D3D12_INDEX_BUFFER_VIEW {
                    BufferLocation: buffer.GetGPUVirtualAddress(),
                    SizeInBytes: 1024 * 1024,
                    Format: DXGI_FORMAT_R32_UINT,
                };
                list.IASetIndexBuffer(Some(&view));
                std::mem::forget(buffer);
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn draw_instanced(
    vertex_count: u32,
    instance_count: u32,
    start_vertex: u32,
    start_instance: u32,
) {
    debug_println!("\n[API] draw_instanced({}, {}, {}, {})", vertex_count, instance_count, start_vertex, start_instance);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.DrawInstanced(vertex_count, instance_count, start_vertex, start_instance);
    }
}

#[no_mangle]
pub unsafe extern "C" fn draw_indexed_instanced(
    index_count: u32,
    instance_count: u32,
    start_index: u32,
    base_vertex: i32,
    start_instance: u32,
) {
    debug_println!("\n[API] draw_indexed_instanced({}, {}, {}, {}, {})",
                   index_count, instance_count, start_index, base_vertex, start_instance);

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.DrawIndexedInstanced(index_count, instance_count, start_index, base_vertex, start_instance);
    }
}

#[no_mangle]
pub unsafe extern "C" fn wait_for_gpu() {
    debug_println!("\n[API] wait_for_gpu() called");

    let (queue, fence, fence_value) = {
        let state = STATE.lock().unwrap();
        (
            state.command_queue.clone(),
            state.fence.clone(),
            state.fence_value,
        )
    };

    if let (Some(queue), Some(fence)) = (queue, fence) {
        let new_fence_value = fence_value + 1;

        {
            let mut state = STATE.lock().unwrap();
            state.fence_value = new_fence_value;
        }

        let _ = queue.Signal(&fence, new_fence_value);

        if fence.GetCompletedValue() < new_fence_value {
            let event = CreateEventA(None, true, false, None).expect("Failed to create event");
            let _ = fence.SetEventOnCompletion(new_fence_value, event);
            WaitForSingleObject(event, INFINITE);
            CloseHandle(event);
        }
    }
}

#[no_mangle]
pub extern "C" fn get_frame_index() -> u32 {
    STATE.lock().unwrap().frame_index
}

#[no_mangle]
pub extern "C" fn get_rtv_descriptor_size() -> u32 {
    STATE.lock().unwrap().rtv_descriptor_size
}

#[no_mangle]
pub extern "C" fn get_dsv_descriptor_size() -> u32 {
    STATE.lock().unwrap().dsv_descriptor_size
}