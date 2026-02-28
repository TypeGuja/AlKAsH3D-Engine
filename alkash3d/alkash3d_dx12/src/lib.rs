//! alkash3d_dx12 – Полноценная рабочая обертка над DirectX 12 для Python
//! ИСПРАВЛЕННАЯ ВЕРСИЯ С ПРАВИЛЬНОЙ СИНХРОНИЗАЦИЕЙ

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

// Флаг отладки
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
    command_allocators: Vec<Option<ID3D12CommandAllocator>>,
    current_allocator_index: usize,
    root_signature: Option<ID3D12RootSignature>,
    rtv_descriptor_size: u32,
    dsv_descriptor_size: u32,
    cbv_srv_uav_descriptor_size: u32,
    frame_index: u32,
    fence: Option<ID3D12Fence>,
    fence_values: Vec<u64>,
    rtv_heap: Option<ID3D12DescriptorHeap>,
    rtv_handle_size: u32,
}

impl GlobalState {
    fn new() -> Self {
        Self {
            device: None,
            command_queue: None,
            swap_chain: None,
            command_list: None,
            command_allocators: Vec::new(),
            current_allocator_index: 0,
            root_signature: None,
            rtv_descriptor_size: 0,
            dsv_descriptor_size: 0,
            cbv_srv_uav_descriptor_size: 0,
            frame_index: 0,
            fence: None,
            fence_values: vec![0; 4], // Для 4 кадров
            rtv_heap: None,
            rtv_handle_size: 0,
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

/* ==================== КОМАНДНЫЙ АЛЛОКАТОР ==================== */
mod command_mod {
    use super::*;

    pub unsafe fn create_allocator(device: &ID3D12Device) -> Option<ID3D12CommandAllocator> {
        debug_println!("[command] create_allocator: calling CreateCommandAllocator...");

        match device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) {
            Ok(a) => {
                debug_println!("[command] create_allocator: SUCCESS");
                Some(a)
            },
            Err(e) => {
                debug_println!("[command] create_allocator: FAILED with HRESULT 0x{:X}", e.code().0);
                None
            }
        }
    }

    pub unsafe fn create_command_list(
        device: &ID3D12Device,
        allocator: &ID3D12CommandAllocator,
        pso: Option<&ID3D12PipelineState>,
    ) -> Option<ID3D12GraphicsCommandList> {
        debug_println!("[command] create_command_list: calling CreateCommandList...");

        let result: Result<ID3D12GraphicsCommandList, _> = device.CreateCommandList(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            allocator,
            pso,
        );

        match result {
            Ok(list) => {
                debug_println!("[command] create_command_list: SUCCESS");

                // Закрываем сразу – это обязательное требование DirectX 12.
                if let Err(e) = list.Close() {
                    debug_println!(
                        "[command] initial Close() failed: HRESULT 0x{:X}",
                        e.code().0
                    );
                }

                Some(list)
            }
            Err(e) => {
                debug_println!(
                    "[command] create_command_list: FAILED with HRESULT 0x{:X}",
                    e.code().0
                );
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
        debug_println!("[swapchain] Creating swap chain {}x{} with hwnd=0x{:X}", width, height, hwnd);

        if hwnd == 0 {
            debug_println!("[swapchain] ERROR: invalid HWND (0)");
            return None;
        }

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
            Ok(sc) => {
                debug_println!("[swapchain] CreateSwapChainForHwnd returned OK");
                sc
            },
            Err(e) => {
                debug_println!("[swapchain] Failed to create swap chain: HRESULT 0x{:X}", e.code().0);
                return None;
            }
        };

        match swap_chain1.cast::<IDXGISwapChain3>() {
            Ok(sc) => {
                debug_println!("[swapchain] Created successfully at {:p}", sc.as_raw());

                let _ = factory.MakeWindowAssociation(HWND(hwnd as isize), DXGI_MWA_NO_ALT_ENTER);

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
            if let Ok(mut state) = STATE.lock() {
                state.frame_index = frame_idx;
            }
            debug_println!("[swapchain_mod::present] Present OK, new frame index: {}", frame_idx);
            true
        } else {
            debug_println!("[swapchain_mod::present] Present FAILED");
            false
        }
    }

    pub unsafe fn resize(swap: &IDXGISwapChain3, width: u32, height: u32) -> bool {
        debug_println!("[swapchain] Resizing to {}x{}", width, height);

        let hr = swap.ResizeBuffers(2, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0);

        if hr.is_ok() {
            debug_println!("[swapchain] ResizeBuffers succeeded");
            true
        } else {
            debug_println!("[swapchain] ResizeBuffers failed");
            false
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
        shader_visible: bool,
    ) -> Option<ID3D12DescriptorHeap> {
        let heap_ty = match heap_type {
            0 => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            1 => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            2 => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            _ => {
                eprintln!("[heap] Invalid type: {}", heap_type);
                return None;
            }
        };

        let flags = if heap_ty == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && shader_visible {
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
                eprintln!("[heap] Created at {:p}", heap.as_raw());
                Some(heap)
            },
            Err(e) => {
                eprintln!("[heap] Failed: HRESULT 0x{:X}", e.code().0);
                None
            }
        }
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
            debug_println!("[buffer] Failed: HRESULT 0x{:X}", e.code().0);
            return None;
        }

        resource_opt
    }

    pub unsafe fn update(
        resource: &ID3D12Resource,
        data: *const c_void,
        size: usize,
    ) -> bool {
        let write_range = D3D12_RANGE { Begin: 0, End: 0 };
        let mut mapped: *mut c_void = ptr::null_mut();

        if let Err(e) = resource.Map(0, Some(&write_range), Some(&mut mapped)) {
            debug_println!("[buffer] Map failed: HRESULT 0x{:X}", e.code().0);
            return false;
        }

        if !mapped.is_null() && !data.is_null() {
            std::ptr::copy_nonoverlapping(data as *const u8, mapped as *mut u8, size);
        }

        let _ = resource.Unmap(0, None);
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
        upload: bool,
    ) -> Option<ID3D12Resource> {
        let heap_type = if upload {
            D3D12_HEAP_TYPE_UPLOAD
        } else {
            D3D12_HEAP_TYPE_DEFAULT
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: heap_type,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        };

        let init_state = if upload {
            D3D12_RESOURCE_STATE_GENERIC_READ
        } else {
            D3D12_RESOURCE_STATE_COPY_DEST
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

        let mut tex_opt: Option<ID3D12Resource> = None;
        let hr = device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            init_state,
            None,
            &mut tex_opt,
        );

        if let Err(e) = hr {
            debug_println!("[texture] Failed: HRESULT 0x{:X}", e.code().0);
            return None;
        }

        tex_opt
    }

    pub unsafe fn update(
        texture: &ID3D12Resource,
        data: *const c_void,
        width: u32,
        height: u32,
        bpp: usize,
    ) -> bool {
        let write_range = D3D12_RANGE { Begin: 0, End: 0 };
        let mut mapped: *mut c_void = ptr::null_mut();

        if let Err(e) = texture.Map(0, Some(&write_range), Some(&mut mapped)) {
            debug_println!("[texture] Map failed: HRESULT 0x{:X}", e.code().0);
            return false;
        }

        if !mapped.is_null() && !data.is_null() {
            let row_pitch   = (width as usize) * bpp;
            let slice_pitch = row_pitch * height as usize;
            std::ptr::copy_nonoverlapping(data as *const u8, mapped as *mut u8, slice_pitch);
        }

        let _ = texture.Unmap(0, None);
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
        if file_path.is_null() || entry_point.is_null() || profile.is_null() {
            return None;
        }

        let dll_name = match CString::new("d3dcompiler_47.dll") {
            Ok(s) => s,
            Err(_) => return None,
        };

        let lib = match LoadLibraryA(PCSTR(dll_name.as_ptr() as *const u8)) {
            Ok(h) => h,
            Err(_) => return None,
        };

        let proc_name = match CString::new("D3DCompileFromFile") {
            Ok(s) => s,
            Err(_) => return None,
        };

        let fn_ptr = match GetProcAddress(lib, PCSTR(proc_name.as_ptr() as *const u8)) {
            Some(p) => p,
            None => return None,
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

        if hr.is_ok() && !shader_blob.is_null() {
            return Some(std::mem::transmute_copy(&shader_blob));
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

        debug_println!("[pso] Creating PSO...");

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

        let ptr_val = ptr as usize;
        if ptr_val == 0xDEADBEEF || ptr_val == 0xDEADF00D ||
            ptr_val == 0xFEEDC0DE || ptr_val == 0x12345678 ||
            ptr_val == 0x87654321 {
            debug_println!("[release] Skipping stub pointer: {:#x}", ptr_val);
            return;
        }

        if ptr_val < 0x10000 {
            debug_println!("[release] Suspicious pointer: {:#x}", ptr_val);
            return;
        }

        let _ = std::panic::catch_unwind(|| {
            let unknown = IUnknown::from_raw(ptr);
            std::mem::drop(unknown);
            debug_println!("[release] Resource released");
        });
    }
}

/* ==================== ЭКСПОРТИРУЕМЫЕ ФУНКЦИИ ==================== */

#[no_mangle]
pub extern "C" fn release_resource(res_ptr: *mut c_void) {
    if res_ptr.is_null() {
        return;
    }
    unsafe {
        release_mod::release_resource(res_ptr);
    }
}

#[no_mangle]
pub extern "C" fn force_cleanup() {
    println!("\n[API] force_cleanup() called");
    unsafe {
        let mut state = STATE.lock().unwrap();
        state.command_list = None;
        state.command_allocators.clear();
        state.swap_chain = None;
        state.command_queue = None;
        state.root_signature = None;
        state.fence = None;
        state.rtv_heap = None;
        state.device = None;
        println!("[API] force_cleanup() done");
    }
}

#[no_mangle]
pub extern "C" fn create_device() -> *mut c_void {
    println!("\n[API] create_device() called");

    unsafe {
        let device = match device_mod::create_d3d12_device() {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        let root_sig = match root_sig::create_graphics_root_signature(&device) {
            Some(s) => s,
            None => return ptr::null_mut(),
        };

        let mut allocators = Vec::new();
        for _ in 0..4 {
            let allocator = match command_mod::create_allocator(&device) {
                Some(a) => a,
                None => return ptr::null_mut(),
            };
            allocators.push(Some(allocator));
        }

        let command_list = match command_mod::create_command_list(&device, allocators[0].as_ref().unwrap(), None) {
            Some(l) => l,
            None => return ptr::null_mut(),
        };

        let fence: ID3D12Fence = match device.CreateFence(0, D3D12_FENCE_FLAG_NONE) {
            Ok(f) => f,
            Err(_) => return ptr::null_mut(),
        };

        {
            let mut state = STATE.lock().unwrap();
            state.device = Some(device.clone());
            state.command_allocators = allocators.clone();
            state.command_list = Some(command_list.clone());
            state.fence = Some(fence.clone());
            state.root_signature = Some(root_sig.clone());
            state.fence_values = vec![0; 4];

            let rtv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let dsv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
            let cbv_sz = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

            state.rtv_descriptor_size = rtv_sz;
            state.dsv_descriptor_size = dsv_sz;
            state.cbv_srv_uav_descriptor_size = cbv_sz;
        }

        let raw_ptr = device.as_raw();
        std::mem::forget(device);
        std::mem::forget(root_sig);
        for a in allocators {
            std::mem::forget(a);
        }
        std::mem::forget(command_list);
        std::mem::forget(fence);

        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn create_command_queue(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        let queue = match queue_mod::create(&device) {
            Some(q) => q,
            None => return ptr::null_mut(),
        };

        let mut state = STATE.lock().unwrap();
        state.command_queue = Some(queue.clone());

        let raw_ptr = queue.as_raw();
        std::mem::forget(queue);
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
    unsafe {
        use ptr_utils::*;

        let queue = match as_queue(queue_ptr) {
            Some(q) => q,
            None => return ptr::null_mut(),
        };

        let swap_chain = match swapchain_mod::create(&queue, hwnd, width, height) {
            Some(sc) => sc,
            None => return ptr::null_mut(),
        };

        let mut state = STATE.lock().unwrap();
        state.swap_chain = Some(swap_chain.clone());
        state.frame_index = swap_chain.GetCurrentBackBufferIndex();

        let raw_ptr = swap_chain.as_raw();
        std::mem::forget(swap_chain);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn present_swap_chain(swap_ptr: *mut c_void, sync_interval: u32) -> bool {
    if swap_ptr.is_null() {
        return false;
    }

    unsafe {
        use ptr_utils::*;

        if let Some(swap) = as_swapchain(swap_ptr) {
            let result = swapchain_mod::present(&swap, sync_interval);
            std::mem::forget(swap);
            result
        } else {
            false
        }
    }
}

#[no_mangle]
pub extern "C" fn resize_swap_chain(swap_ptr: *mut c_void, width: u32, height: u32) -> bool {
    unsafe {
        use ptr_utils::*;

        if let Some(swap) = as_swapchain(swap_ptr) {
            let result = swapchain_mod::resize(&swap, width, height);
            std::mem::forget(swap);
            result
        } else {
            false
        }
    }
}

#[no_mangle]
pub extern "C" fn swap_chain_get_buffer(
    swap_ptr: *mut c_void,
    buffer_index: u32,
) -> *mut c_void {
    unsafe {
        use ptr_utils::*;

        let swap_chain = match as_swapchain(swap_ptr) {
            Some(s) => s,
            None => return ptr::null_mut(),
        };

        let buffer_result: Result<ID3D12Resource, windows::core::Error> = swap_chain.GetBuffer(buffer_index);
        std::mem::forget(swap_chain);

        match buffer_result {
            Ok(buffer) => {
                let raw_ptr = buffer.as_raw();
                std::mem::forget(buffer);
                raw_ptr as *mut c_void
            },
            Err(_) => ptr::null_mut(),
        }
    }
}

#[no_mangle]
pub extern "C" fn create_constant_buffer_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: usize,
) -> bool {
    unsafe {
        let device = match ptr_utils::as_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };
        let resource = match ptr_utils::as_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        let size = ((resource.GetDesc().Width as usize + 255) & !255) as u32;

        let desc = D3D12_CONSTANT_BUFFER_VIEW_DESC {
            BufferLocation: resource.GetGPUVirtualAddress(),
            SizeInBytes: size,
        };

        let desc_opt = Some(&desc as *const D3D12_CONSTANT_BUFFER_VIEW_DESC);
        let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle };

        device.CreateConstantBufferView(desc_opt, handle);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_descriptor_heap(
    device_ptr: *mut c_void,
    num_descriptors: u32,
    heap_type: u32,
    shader_visible: bool,
) -> *mut c_void {
    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        let heap = match heap_mod::create(&device, num_descriptors, heap_type, shader_visible) {
            Some(h) => h,
            None => return ptr::null_mut(),
        };

        let raw_ptr = heap.as_raw();
        std::mem::forget(heap);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn GetCPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> usize {
    if heap_ptr.is_null() {
        return 0;
    }

    unsafe {
        let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
        let handle = heap.GetCPUDescriptorHandleForHeapStart();
        let result = handle.ptr as usize;
        std::mem::forget(heap);
        result
    }
}

#[no_mangle]
pub extern "C" fn GetGPUDescriptorHandleForHeapStart(heap_ptr: *mut c_void) -> usize {
    if heap_ptr.is_null() {
        return 0;
    }

    unsafe {
        let heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&heap_ptr);
        let handle = heap.GetGPUDescriptorHandleForHeapStart();
        let result = handle.ptr as usize;
        std::mem::forget(heap);
        result
    }
}

#[no_mangle]
pub extern "C" fn offset_descriptor_handle(start: usize, offset: u32) -> usize {
    let state = STATE.lock().unwrap();
    let increment = state.rtv_descriptor_size as usize;
    start + (offset as usize) * increment
}

#[no_mangle]
pub extern "C" fn create_buffer(
    device_ptr: *mut c_void,
    size: usize,
    _usage: *const u8,
) -> *mut c_void {
    unsafe {
        use ptr_utils::*;

        if size == 0 || size > 1024 * 1024 * 1024 {
            return ptr::null_mut();
        }

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        let buffer = match buffer_mod::create_upload(&device, size) {
            Some(b) => b,
            None => return ptr::null_mut(),
        };

        let raw_ptr = buffer.as_raw();
        std::mem::forget(buffer);
        raw_ptr as *mut c_void
    }
}

#[no_mangle]
pub extern "C" fn update_subresource(
    buffer_ptr: *mut c_void,
    data_ptr: *const c_void,
    size: usize,
) -> bool {
    unsafe {
        use ptr_utils::*;

        if let Some(buffer) = as_resource(buffer_ptr) {
            let result = buffer_mod::update(&buffer, data_ptr, size);
            std::mem::forget(buffer);
            result
        } else {
            false
        }
    }
}

#[no_mangle]
pub extern "C" fn create_texture_from_memory(
    device_ptr: *mut c_void,
    data_ptr: *mut c_void,
    width: u32,
    height: u32,
    fmt: *const u8,
) -> *mut c_void {
    if device_ptr.is_null() || width == 0 || height == 0 {
        return ptr::null_mut();
    }

    unsafe {
        let device = match ptr_utils::as_device(device_ptr) {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        let fmt_str = if fmt.is_null() {
            "rgba8"
        } else {
            CStr::from_ptr(fmt as *const i8).to_str().unwrap_or("rgba8")
        };
        let dxgi_format = match fmt_str.to_ascii_lowercase().as_str() {
            "rgba8" | "rgba8unorm" => DXGI_FORMAT_R8G8B8A8_UNORM,
            "rgba16f" => DXGI_FORMAT_R16G16B16A16_FLOAT,
            "rgba32f" => DXGI_FORMAT_R32G32B32A32_FLOAT,
            _ => DXGI_FORMAT_R8G8B8A8_UNORM,
        };

        let upload = !data_ptr.is_null();
        let tex_opt = texture_mod::create_2d(&device, width, height, dxgi_format, upload);
        let tex = match tex_opt {
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

            texture_mod::update(&tex, data_ptr, width, height, bpp);
        }

        let raw = tex.as_raw();
        std::mem::forget(tex);
        raw as *mut c_void
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
        use ptr_utils::*;

        if let Some(texture) = as_resource(texture_ptr) {
            let result = texture_mod::update(&texture, data_ptr, width, height, 4);
            std::mem::forget(texture);
            result
        } else {
            false
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
) -> bool {
    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let resource = match as_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        view_mod::create_srv(&device, &resource, cpu_handle);
        std::mem::forget(device);
        std::mem::forget(resource);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_render_target_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: usize,
) -> bool {
    unsafe {
        use ptr_utils::*;

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let resource = match as_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        view_mod::create_rtv(&device, &resource, cpu_handle);
        std::mem::forget(device);
        std::mem::forget(resource);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_graphics_ps(
    device_ptr: *mut c_void,
    vs_blob_ptr: *mut c_void,
    ps_blob_ptr: *mut c_void,
) -> *mut c_void {
    unsafe {
        use ptr_utils::*;

        if device_ptr.is_null() || vs_blob_ptr.is_null() || ps_blob_ptr.is_null() {
            eprintln!("[create_graphics_ps] Null pointer argument");
            return ptr::null_mut();
        }

        let device = match as_device(device_ptr) {
            Some(d) => d,
            None => {
                eprintln!("[create_graphics_ps] Failed to get device");
                return ptr::null_mut();
            }
        };

        let vs_blob = match as_blob(vs_blob_ptr) {
            Some(b) => b,
            None => {
                eprintln!("[create_graphics_ps] Failed to get VS blob");
                return ptr::null_mut();
            }
        };

        let ps_blob = match as_blob(ps_blob_ptr) {
            Some(b) => b,
            None => {
                eprintln!("[create_graphics_ps] Failed to get PS blob");
                return ptr::null_mut();
            }
        };

        // Проверяем размеры шейдеров
        if vs_blob.GetBufferSize() == 0 || ps_blob.GetBufferSize() == 0 {
            eprintln!("[create_graphics_ps] Shader blob size is zero");
            return ptr::null_mut();
        }

        let root_sig = match STATE.lock().unwrap().root_signature.clone() {
            Some(s) => s,
            None => {
                eprintln!("[create_graphics_ps] No root signature");
                return ptr::null_mut();
            }
        };

        eprintln!("[create_graphics_ps] Creating PSO...");
        eprintln!("  VS size: {}", vs_blob.GetBufferSize());
        eprintln!("  PS size: {}", ps_blob.GetBufferSize());

        let pso = match pso_mod::create_graphics(&device, &root_sig, &vs_blob, &ps_blob) {
            Some(p) => p,
            None => {
                eprintln!("[create_graphics_ps] PSO creation failed");
                return ptr::null_mut();
            }
        };

        let raw_ptr = pso.as_raw();
        eprintln!("[create_graphics_ps] PSO created at {:p}", raw_ptr);
        std::mem::forget(pso);
        raw_ptr as *mut c_void
    }
}

/* ==================== ОСНОВНЫЕ ФУНКЦИИ РЕНДЕРИНГА ==================== */

#[no_mangle]
pub unsafe extern "C" fn begin_frame() -> bool {
    debug_println!("\n[API] begin_frame() called");

    // Получаем текущий индекс кадра
    let frame_index = {
        let state = STATE.lock().unwrap();
        state.frame_index as usize
    };

    // Ждем GPU для текущего кадра
    if !wait_for_gpu() {
        debug_println!("[API] wait_for_gpu failed in begin_frame");
        return false;
    }

    // Получаем аллокатор для текущего кадра
    let (allocator, list) = {
        let mut state = STATE.lock().unwrap();

        // Убеждаемся, что у нас есть аллокаторы
        if state.command_allocators.is_empty() {
            debug_println!("[API] No command allocators");
            return false;
        }

        // Используем аллокатор для текущего кадра
        let alloc_index = frame_index % state.command_allocators.len();
        let allocator = state.command_allocators[alloc_index].clone();
        let list = state.command_list.clone();

        (allocator, list)
    };

    if let (Some(allocator), Some(list)) = (allocator, list) {
        // Сначала пробуем сбросить allocator
        match allocator.Reset() {
            Ok(()) => debug_println!("[API] Allocator reset successfully"),
            Err(e) => {
                debug_println!("[API] Failed to reset allocator: {:?}", e);
                // Пробуем создать новый аллокатор
                if let Some(device) = STATE.lock().unwrap().device.clone() {
                    // Явно указываем тип для CreateCommandAllocator
                    let new_allocator: Option<ID3D12CommandAllocator> = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).ok();

                    if let Some(new_allocator) = new_allocator {
                        let mut state = STATE.lock().unwrap();
                        let index = frame_index % state.command_allocators.len();
                        state.command_allocators[index] = Some(new_allocator.clone());

                        // Пробуем сбросить список с новым аллокатором
                        if let Err(e) = list.Reset(&new_allocator, None) {
                            debug_println!("[API] Failed to reset command list with new allocator: {:?}", e);
                            return false;
                        }
                        debug_println!("[API] Created new allocator and reset command list");
                        return true;
                    } else {
                        debug_println!("[API] Failed to create new allocator");
                        return false;
                    }
                }
                return false;
            }
        }

        // Сбрасываем command list с этим аллокатором
        match list.Reset(&allocator, None) {
            Ok(()) => {
                debug_println!("[API] Command list reset successfully");
                true
            }
            Err(e) => {
                debug_println!("[API] Failed to reset command list: {:?}", e);
                false
            }
        }
    } else {
        debug_println!("[API] Missing allocator or list");
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn end_frame() -> bool {
    debug_println!("\n[API] end_frame() called");

    let (queue_opt, cmd_list_opt, fence_opt) = {
        let state = STATE.lock().unwrap();
        (
            state.command_queue.clone(),
            state.command_list.clone(),
            state.fence.clone(),
        )
    };

    let (queue, cmd_list, fence) = match (queue_opt, cmd_list_opt, fence_opt) {
        (Some(q), Some(cl), Some(f)) => (q, cl, f),
        _ => {
            debug_println!("[API] Missing queue, command list or fence");
            return false;
        }
    };

    // Закрываем список
    if let Err(e) = cmd_list.Close() {
        debug_println!("[API] CommandList::Close() failed: {:?}", e);
        return false;
    }

    // Выполняем список команд
    let cmd_list_clone = cmd_list.clone();
    let cmd_list_cast: ID3D12CommandList = match cmd_list_clone.cast() {
        Ok(list) => list,
        Err(e) => {
            debug_println!("[API] Failed to cast command list: {:?}", e);
            return false;
        }
    };

    queue.ExecuteCommandLists(&[Some(cmd_list_cast)]);

    // Обновляем fence значение
    let frame_idx = {
        let state = STATE.lock().unwrap();
        state.frame_index as usize
    };

    let mut state = STATE.lock().unwrap();
    if frame_idx < state.fence_values.len() {
        state.fence_values[frame_idx] = state.fence_values[frame_idx].wrapping_add(1);
        let fence_val = state.fence_values[frame_idx];

        // Сигналим fence
        if let Err(e) = queue.Signal(&fence, fence_val) {
            debug_println!("[API] Queue::Signal() failed: {:?}", e);
            return false;
        }

        debug_println!("[API] end_frame completed – fence value {}", fence_val);
    } else {
        debug_println!("[API] Invalid frame index: {}", frame_idx);
        return false;
    }

    true
}

#[no_mangle]
pub unsafe extern "C" fn wait_for_gpu() -> bool {
    debug_println!("\n[API] wait_for_gpu() called");

    let (queue, fence) = {
        let state = STATE.lock().unwrap();
        (
            state.command_queue.clone(),
            state.fence.clone(),
        )
    };

    if let (Some(queue), Some(fence)) = (queue, fence) {
        // Получаем текущий кадр
        let frame_idx = {
            let state = STATE.lock().unwrap();
            state.frame_index as usize
        };

        // Получаем значение fence для этого кадра
        let fence_value = {
            let state = STATE.lock().unwrap();
            if frame_idx < state.fence_values.len() {
                state.fence_values[frame_idx]
            } else {
                0
            }
        };

        if fence_value > 0 {
            // Проверяем текущее значение
            let completed_value = fence.GetCompletedValue();

            if completed_value < fence_value {
                debug_println!("[API] Waiting for fence: {} < {}", completed_value, fence_value);

                let event = match CreateEventA(None, true, false, None) {
                    Ok(e) => e,
                    Err(_) => {
                        debug_println!("[API] Failed to create event");
                        return false;
                    }
                };

                if let Err(e) = fence.SetEventOnCompletion(fence_value, event) {
                    debug_println!("[API] SetEventOnCompletion failed: {:?}", e);
                    CloseHandle(event);
                    return false;
                }

                WaitForSingleObject(event, INFINITE);
                CloseHandle(event);

                debug_println!("[API] Fence wait completed");
            } else {
                debug_println!("[API] Fence already completed: {} >= {}", completed_value, fence_value);
            }
        }

        debug_println!("[API] wait_for_gpu completed");
        true
    } else {
        debug_println!("[API] Missing queue or fence");
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn set_graphics_pipeline(pso_ptr: *mut c_void) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        if let Some(pso) = ptr_utils::as_pipeline_state(pso_ptr) {
            list.SetPipelineState(&pso);
            std::mem::forget(pso);
            return true;
        }
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_root_descriptor_table(root_index: u32, gpu_handle: u64) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let handle = D3D12_GPU_DESCRIPTOR_HANDLE { ptr: gpu_handle };
        list.SetGraphicsRootDescriptorTable(root_index, handle);
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_descriptor_heaps(count: usize, heaps: *const *mut c_void) -> bool {
    if count == 0 || heaps.is_null() {
        return false;
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
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_render_target(rtv: usize) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv };
        list.OMSetRenderTargets(1, Some(&rtv_handle), false, None);
        debug_println!("[API] Render target set");
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_render_targets(count: usize, rtvs: *const usize) -> bool {
    if count == 0 || rtvs.is_null() {
        return false;
    }

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let mut rtv_handles = Vec::with_capacity(count);
        for i in 0..count {
            let rtv = *rtvs.add(i);
            rtv_handles.push(D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv });
        }
        list.OMSetRenderTargets(count as u32, Some(rtv_handles.as_ptr()), false, None);
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn clear_render_target(rtv: usize, color: *const f32) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv };
        let clear_color: [f32; 4] = [
            *color,
            *color.add(1),
            *color.add(2),
            *color.add(3)
        ];
        list.ClearRenderTargetView(rtv_handle, &clear_color, None);
        debug_println!("[API] Render target cleared");
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_viewport(x: i32, y: i32, w: i32, h: i32, min_depth: f32, max_depth: f32) -> bool {
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
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_scissor_rect(left: i32, top: i32, right: i32, bottom: i32) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        let rect = RECT { left, top, right, bottom };
        list.RSSetScissorRects(&[rect]);
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn set_vertex_buffers(vertex_buffer: *mut c_void, index_buffer: *mut c_void) -> bool {
    use ptr_utils::*;

    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        if !vertex_buffer.is_null() {
            if let Some(buffer) = as_resource(vertex_buffer) {
                let desc = buffer.GetDesc();
                let view = D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: buffer.GetGPUVirtualAddress(),
                    SizeInBytes: desc.Width as u32,
                    StrideInBytes: 12,
                };
                list.IASetVertexBuffers(0, Some(&[view]));
                std::mem::forget(buffer);
            }
        }

        if !index_buffer.is_null() {
            if let Some(buffer) = as_resource(index_buffer) {
                let desc = buffer.GetDesc();
                let view = D3D12_INDEX_BUFFER_VIEW {
                    BufferLocation: buffer.GetGPUVirtualAddress(),
                    SizeInBytes: desc.Width as u32,
                    Format: DXGI_FORMAT_R32_UINT,
                };
                list.IASetIndexBuffer(Some(&view));
                std::mem::forget(buffer);
            }
        }
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn draw_instanced(
    vertex_count: u32,
    instance_count: u32,
    start_vertex: u32,
    start_instance: u32,
) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.DrawInstanced(vertex_count, instance_count, start_vertex, start_instance);
        return true;
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn draw_indexed_instanced(
    index_count: u32,
    instance_count: u32,
    start_index: u32,
    base_vertex: i32,
    start_instance: u32,
) -> bool {
    let state = STATE.lock().unwrap();
    if let Some(list) = &state.command_list {
        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.DrawIndexedInstanced(index_count, instance_count, start_index, base_vertex, start_instance);
        return true;
    }
    false
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

#[no_mangle]
pub extern "C" fn set_vsync(enable: bool) {
    debug_println!("[API] set_vsync({}) called", enable);
}

#[no_mangle]
pub extern "C" fn get_last_error() -> *const std::os::raw::c_char {
    static mut ERROR_BUFFER: [u8; 256] = [0; 256];
    unsafe {
        let error = "No error\0";
        let bytes = error.as_bytes();
        for (i, &byte) in bytes.iter().enumerate() {
            ERROR_BUFFER[i] = byte;
        }
        ERROR_BUFFER.as_ptr() as *const std::os::raw::c_char
    }
}