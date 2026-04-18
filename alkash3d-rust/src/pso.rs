//! Pipeline State Objects с поддержкой разных конфигураций

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_core::{Interface, s};
use crate::{debug_println, utils::ptr_to_device};

#[repr(u32)]
pub enum PsoType {
    Simple = 0,      // Только позиция и цвет
    Advanced = 1,    // Полный вертекс с нормалями и UV
    Textured = 2,    // С текстурами
    Wireframe = 3,   // Wireframe режим
}

#[no_mangle]
pub extern "C" fn create_root_signature_simple(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_root_signature_simple] Creating...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        // Один CBV параметр
        let root_param = D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
        };

        let root_sig_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 1,
            pParameters: &root_param,
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        serialize_and_create_root_signature(&device, &root_sig_desc)
    }
}

#[no_mangle]
pub extern "C" fn create_root_signature_advanced(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_root_signature_advanced] Creating...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        // Один CBV параметр для расширенного шейдера
        let root_param = D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };

        let root_sig_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 1,
            pParameters: &root_param,
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        serialize_and_create_root_signature(&device, &root_sig_desc)
    }
}

#[no_mangle]
pub extern "C" fn create_root_signature(device_ptr: *mut c_void) -> *mut c_void {
    create_root_signature_simple(device_ptr)
}

#[no_mangle]
pub extern "C" fn create_simple_pso(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
) -> *mut c_void {
    unsafe {
        create_pso_internal(device_ptr, root_sig_ptr, PsoType::Simple as u32)
    }
}

#[no_mangle]
pub extern "C" fn create_advanced_pso(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
) -> *mut c_void {
    unsafe {
        create_pso_internal(device_ptr, root_sig_ptr, PsoType::Advanced as u32)
    }
}

#[no_mangle]
pub extern "C" fn create_pso(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
    pso_type: u32,
) -> *mut c_void {
    unsafe {
        create_pso_internal(device_ptr, root_sig_ptr, pso_type)
    }
}

unsafe fn create_pso_internal(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
    pso_type: u32,
) -> *mut c_void {
    debug_println!("\n[create_pso] Type: {}", pso_type);

    let device = match ptr_to_device(device_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    if root_sig_ptr.is_null() {
        debug_println!("[create_pso] Root signature is null!");
        std::mem::forget(device);
        return std::ptr::null_mut();
    }

    let root_sig: ID3D12RootSignature = std::mem::transmute_copy(&root_sig_ptr);

    // Получаем шейдеры в зависимости от типа
    let (vs_blob_ptr, ps_blob_ptr) = match pso_type {
        0 => (crate::shader::get_builtin_vs_blob(), crate::shader::get_builtin_ps_blob()),
        1 => (crate::shader::get_advanced_vs_blob(), crate::shader::get_advanced_ps_blob()),
        _ => (crate::shader::get_builtin_vs_blob(), crate::shader::get_builtin_ps_blob()),
    };

    if vs_blob_ptr.is_null() || ps_blob_ptr.is_null() {
        debug_println!("[create_pso] Failed to get shader blobs");
        std::mem::forget(root_sig);
        std::mem::forget(device);
        return std::ptr::null_mut();
    }

    let vs_blob: ID3DBlob = std::mem::transmute_copy(&vs_blob_ptr);
    let ps_blob: ID3DBlob = std::mem::transmute_copy(&ps_blob_ptr);

    let vs_data = vs_blob.GetBufferPointer();
    let vs_size = vs_blob.GetBufferSize();
    let ps_data = ps_blob.GetBufferPointer();
    let ps_size = ps_blob.GetBufferSize();

    if vs_data.is_null() || ps_data.is_null() {
        debug_println!("[create_pso] Shader data is null!");
        std::mem::forget(vs_blob);
        std::mem::forget(ps_blob);
        std::mem::forget(root_sig);
        std::mem::forget(device);
        return std::ptr::null_mut();
    }

    debug_println!("[create_pso] VS: {} bytes, PS: {} bytes", vs_size, ps_size);

    // Input layout в зависимости от типа
    let input_elements = match pso_type {
        0 => create_simple_input_layout(),
        1 => create_advanced_input_layout(),
        _ => create_simple_input_layout(),
    };

    // Rasterizer state
    let rasterizer = match pso_type {
        3 => D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_WIREFRAME,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: false.into(),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: true.into(),
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        },
        _ => D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: false.into(),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: true.into(),
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        },
    };

    let default_blend = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: false.into(),
        LogicOpEnable: false.into(),
        SrcBlend: D3D12_BLEND_ONE,
        DestBlend: D3D12_BLEND_ZERO,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_ZERO,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: 0x0F,
    };

    // Создаем массив из 8 одинаковых blend states
    let render_target_blends = [
        default_blend,
        default_blend,
        default_blend,
        default_blend,
        default_blend,
        default_blend,
        default_blend,
        default_blend,
    ];

    // Форматы RTV - все 8 должны быть валидными или UNKNOWN
    let rtv_formats = [
        DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_UNKNOWN,
        DXGI_FORMAT_UNKNOWN,
        DXGI_FORMAT_UNKNOWN,
        DXGI_FORMAT_UNKNOWN,
        DXGI_FORMAT_UNKNOWN,
        DXGI_FORMAT_UNKNOWN,
        DXGI_FORMAT_UNKNOWN,
    ];

    let pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(Some(root_sig.clone())),
        VS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: vs_data,
            BytecodeLength: vs_size,
        },
        PS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: ps_data,
            BytecodeLength: ps_size,
        },
        DS: D3D12_SHADER_BYTECODE::default(),
        HS: D3D12_SHADER_BYTECODE::default(),
        GS: D3D12_SHADER_BYTECODE::default(),
        StreamOutput: D3D12_STREAM_OUTPUT_DESC::default(),
        BlendState: D3D12_BLEND_DESC {
            AlphaToCoverageEnable: false.into(),
            IndependentBlendEnable: false.into(),
            RenderTarget: render_target_blends,
        },
        SampleMask: u32::MAX,
        RasterizerState: rasterizer,
        DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: false.into(),
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
            StencilEnable: false.into(),
            StencilReadMask: 0,
            StencilWriteMask: 0,
            FrontFace: D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
                StencilPassOp: D3D12_STENCIL_OP_KEEP,
                StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            },
            BackFace: D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
                StencilPassOp: D3D12_STENCIL_OP_KEEP,
                StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            },
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        },
        IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        RTVFormats: rtv_formats,
        DSVFormat: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        NodeMask: 0,
        CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
        Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
    };

    match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc) {
        Ok(pso) => {
            debug_println!("[create_pso] ✅ PSO created!");
            std::mem::forget(vs_blob);
            std::mem::forget(ps_blob);
            let ptr = pso.as_raw() as *mut c_void;
            std::mem::forget(pso);
            std::mem::forget(root_sig);
            std::mem::forget(device);
            ptr
        }
        Err(e) => {
            debug_println!("[create_pso] Failed with first config: {:?}", e);

            // Пробуем с другими настройками
            debug_println!("[create_pso] Trying alternative configuration...");

            let pso_desc_alt = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
                    DepthEnable: false.into(),
                    DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
                    DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
                    StencilEnable: false.into(),
                    ..Default::default()
                },
                RasterizerState: D3D12_RASTERIZER_DESC {
                    FillMode: D3D12_FILL_MODE_SOLID,
                    CullMode: D3D12_CULL_MODE_NONE,
                    FrontCounterClockwise: false.into(),
                    DepthBias: 0,
                    DepthBiasClamp: 0.0,
                    SlopeScaledDepthBias: 0.0,
                    DepthClipEnable: false.into(),
                    MultisampleEnable: false.into(),
                    AntialiasedLineEnable: false.into(),
                    ForcedSampleCount: 0,
                    ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
                },
                ..pso_desc
            };

            match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc_alt) {
                Ok(pso) => {
                    debug_println!("[create_pso] ✅ PSO created with alt config!");
                    std::mem::forget(vs_blob);
                    std::mem::forget(ps_blob);
                    let ptr = pso.as_raw() as *mut c_void;
                    std::mem::forget(pso);
                    std::mem::forget(root_sig);
                    std::mem::forget(device);
                    ptr
                }
                Err(e2) => {
                    debug_println!("[create_pso] Alt config also failed: {:?}", e2);
                    std::mem::forget(vs_blob);
                    std::mem::forget(ps_blob);
                    std::mem::forget(root_sig);
                    std::mem::forget(device);
                    std::ptr::null_mut()
                }
            }
        }
    }
}

unsafe fn create_simple_input_layout() -> Vec<D3D12_INPUT_ELEMENT_DESC> {
    vec![
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("COLOR"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 12,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ]
}

unsafe fn create_advanced_input_layout() -> Vec<D3D12_INPUT_ELEMENT_DESC> {
    vec![
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("NORMAL"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 12,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("TANGENT"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 24,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("BITANGENT"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 36,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXCOORD"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 48,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXCOORD"),
            SemanticIndex: 1,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 56,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("COLOR"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 64,
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ]
}

unsafe fn serialize_and_create_root_signature(
    device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
    desc: &D3D12_ROOT_SIGNATURE_DESC,
) -> *mut c_void {
    let mut signature_blob: Option<ID3DBlob> = None;
    let mut error_blob: Option<ID3DBlob> = None;

    #[link(name = "d3d12")]
    extern "system" {
        fn D3D12SerializeRootSignature(
            pRootSignature: *const D3D12_ROOT_SIGNATURE_DESC,
            Version: u32,
            ppBlob: *mut Option<ID3DBlob>,
            ppErrorBlob: *mut Option<ID3DBlob>,
        ) -> i32;
    }

    let hr = D3D12SerializeRootSignature(desc, 1, &mut signature_blob, &mut error_blob);

    if hr < 0 {
        if let Some(err) = error_blob {
            let err_ptr = err.GetBufferPointer();
            let err_size = err.GetBufferSize();
            if err_size > 0 && !err_ptr.is_null() {
                let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                debug_println!("Root signature error:\n{}", String::from_utf8_lossy(err_msg));
            }
        }
        return std::ptr::null_mut();
    }

    let blob = match signature_blob {
        Some(b) => b,
        None => return std::ptr::null_mut(),
    };

    let blob_data = blob.GetBufferPointer();
    let blob_size = blob.GetBufferSize();
    let blob_slice = std::slice::from_raw_parts(blob_data as *const u8, blob_size);

    match device.CreateRootSignature::<ID3D12RootSignature>(0, blob_slice) {
        Ok(sig) => {
            let ptr = sig.as_raw() as *mut c_void;
            std::mem::forget(sig);
            debug_println!("✅ Root signature created");
            ptr
        }
        Err(e) => {
            debug_println!("CreateRootSignature failed: {:?}", e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn destroy_pso(pso_ptr: *mut c_void) {
    if !pso_ptr.is_null() {
        unsafe {
            let _pso: ID3D12PipelineState = std::mem::transmute_copy(&pso_ptr);
        }
    }
}

#[no_mangle]
pub extern "C" fn destroy_root_signature(root_sig_ptr: *mut c_void) {
    if !root_sig_ptr.is_null() {
        unsafe {
            let _sig: ID3D12RootSignature = std::mem::transmute_copy(&root_sig_ptr);
        }
    }
}