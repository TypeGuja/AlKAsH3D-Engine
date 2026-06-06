// src/pso.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_core::{s, Interface};
use crate::{debug_println, utils::ptr_to_device, STATE};

#[no_mangle]
pub extern "C" fn create_root_signature_simple(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_root_signature_simple] Creating...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return ptr::null_mut(),
        };

        // Параметр CBV для константного буфера
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
            pStaticSamplers: ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut signature_blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        #[link(name = "d3d12")]
        extern "system" {
            fn D3D12SerializeRootSignature(
                p_root_signature: *const D3D12_ROOT_SIGNATURE_DESC,
                version: u32,
                pp_blob: *mut Option<ID3DBlob>,
                pp_error_blob: *mut Option<ID3DBlob>,
            ) -> i32;
        }

        let hr = D3D12SerializeRootSignature(&root_sig_desc, 1, &mut signature_blob, &mut error_blob);

        if hr < 0 {
            if let Some(err) = error_blob {
                let err_ptr = err.GetBufferPointer();
                let err_size = err.GetBufferSize();
                if err_size > 0 && !err_ptr.is_null() {
                    let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                    debug_println!("Root signature error:\n{}", String::from_utf8_lossy(err_msg));
                }
            }
            return ptr::null_mut();
        }

        let blob = match signature_blob {
            Some(b) => b,
            None => return ptr::null_mut(),
        };

        let blob_data = blob.GetBufferPointer();
        let blob_size = blob.GetBufferSize();
        let blob_slice = std::slice::from_raw_parts(blob_data as *const u8, blob_size);

        match device.CreateRootSignature::<ID3D12RootSignature>(0, blob_slice) {
            Ok(sig) => {
                debug_println!("✅ Root signature created");
                // НЕ используем forget - сохраняем как Box
                let boxed = Box::new(sig);
                Box::into_raw(boxed) as *mut c_void
            }
            Err(e) => {
                debug_println!("CreateRootSignature failed: {:?}", e);
                ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn destroy_root_signature(root_sig_ptr: *mut c_void) -> bool {
    if root_sig_ptr.is_null() {
        return false;
    }
    unsafe {
        // Восстанавливаем Box, он автоматически вызовет Drop
        let _ = Box::from_raw(root_sig_ptr as *mut ID3D12RootSignature);
        debug_println!("[destroy_root_signature] ✅ Root signature destroyed");
        true
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

#[no_mangle]
pub extern "C" fn destroy_pso(pso_ptr: *mut c_void) -> bool {
    if pso_ptr.is_null() {
        return false;
    }
    unsafe {
        let _ = Box::from_raw(pso_ptr as *mut ID3D12PipelineState);
        debug_println!("[destroy_pso] ✅ PSO destroyed");
        true
    }
}

unsafe fn create_pso_internal(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
    _pso_type: u32,
) -> *mut c_void {
    debug_println!("\n[create_pso] Creating...");

    let device = match ptr_to_device(device_ptr) {
        Some(d) => d,
        None => {
            debug_println!("[create_pso] No device");
            return ptr::null_mut();
        }
    };

    if root_sig_ptr.is_null() {
        debug_println!("[create_pso] Root signature is null!");
        return ptr::null_mut();
    }

    let root_sig_ref = &*(root_sig_ptr as *const ID3D12RootSignature);
    let root_sig = root_sig_ref.clone();

    let vs_data = crate::shader::get_builtin_vs_data();
    let vs_size = crate::shader::get_builtin_vs_size();
    let ps_data = crate::shader::get_builtin_ps_data();
    let ps_size = crate::shader::get_builtin_ps_size();

    debug_println!("[create_pso] VS data: {:p}, size: {}", vs_data, vs_size);
    debug_println!("[create_pso] PS data: {:p}, size: {}", ps_data, ps_size);

    if vs_data.is_null() || ps_data.is_null() || vs_size == 0 || ps_size == 0 {
        debug_println!("[create_pso] Invalid shader data!");
        return ptr::null_mut();
    }

    // Input layout для Vertex { position (3 floats), color (4 floats) }
    let input_elements = vec![
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
            AlignedByteOffset: 12,  // 3 floats * 4 = 12
            InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ];

    debug_println!("[create_pso] Input layout: POSITION offset 0, COLOR offset 12");

    let rasterizer = D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: D3D12_CULL_MODE_NONE,  // Отключаем culling, чтобы видеть треугольник с обеих сторон
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        MultisampleEnable: false.into(),
        AntialiasedLineEnable: false.into(),
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
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

    let mut render_target_blends = [default_blend; 8];

    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    rtv_formats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;

    debug_println!("[create_pso] Creating PSO with:");
    debug_println!("  - Vertex shader size: {}", vs_size);
    debug_println!("  - Pixel shader size: {}", ps_size);
    debug_println!("  - RTV format: R8G8B8A8_UNORM");
    debug_println!("  - Cull mode: NONE");

    let pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(Some(root_sig)),
        VS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: vs_data as *const std::ffi::c_void,
            BytecodeLength: vs_size
        },
        PS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: ps_data as *const std::ffi::c_void,
            BytecodeLength: ps_size
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
            FrontFace: D3D12_DEPTH_STENCILOP_DESC::default(),
            BackFace: D3D12_DEPTH_STENCILOP_DESC::default(),
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
        CS: Default::default(),
    };

    debug_println!("[create_pso] Calling CreateGraphicsPipelineState...");

    match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc) {
        Ok(pso) => {
            debug_println!("[create_pso] ✅ PSO created successfully!");
            let boxed = Box::new(pso);
            let ptr = Box::into_raw(boxed) as *mut c_void;
            debug_println!("[create_pso] PSO pointer: {:p}", ptr);
            ptr
        }
        Err(e) => {
            debug_println!("[create_pso] Failed: {:?}", e);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn get_pso_from_state() -> *mut c_void {
    let state = match STATE.lock() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    match state.current_pso.as_ref() {
        Some(pso) => pso.as_raw() as *mut c_void,
        None => std::ptr::null_mut(),
    }
}