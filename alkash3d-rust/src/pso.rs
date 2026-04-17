// pso.rs - Минимальная рабочая версия
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_core::{Interface, s};
use crate::{debug_println, utils::ptr_to_device, STATE};

#[no_mangle]
pub extern "C" fn create_root_signature(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_root_signature] Creating root signature...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        // Один root parameter как constant buffer view
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

        let mut signature_blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3D12SerializeRootSignature(
            &root_sig_desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut signature_blob,
            Some(&mut error_blob),
        );

        if hr.is_err() {
            if let Some(err) = error_blob {
                let err_ptr = err.GetBufferPointer();
                let err_size = err.GetBufferSize();
                if err_size > 0 {
                    let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                    debug_println!("[create_root_signature] ERROR:\n{}", String::from_utf8_lossy(err_msg));
                }
            }
            return std::ptr::null_mut();
        }

        let blob = match signature_blob {
            Some(b) => b,
            None => return std::ptr::null_mut(),
        };

        let blob_slice = std::slice::from_raw_parts(
            blob.GetBufferPointer() as *const u8,
            blob.GetBufferSize()
        );

        match device.CreateRootSignature::<ID3D12RootSignature>(0, blob_slice) {
            Ok(sig) => {
                debug_println!("[create_root_signature] ✅ Created");

                if let Ok(mut state) = STATE.lock() {
                    state.root_signature = Some(sig.clone());
                }

                let raw_ptr = sig.as_raw();
                std::mem::forget(sig);
                raw_ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[create_root_signature] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn create_root_signature_with_texture(device_ptr: *mut c_void) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_root_signature_with_texture] Creating...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let descriptor_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 8,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
        };

        let sampler_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
        };

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &descriptor_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &sampler_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];

        let static_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 16,
            ComparisonFunc: D3D12_COMPARISON_FUNC_LESS_EQUAL,
            BorderColor: D3D12_STATIC_BORDER_COLOR_OPAQUE_WHITE,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 0,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        let root_sig_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 3,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &static_sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut signature_blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3D12SerializeRootSignature(
            &root_sig_desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut signature_blob,
            Some(&mut error_blob),
        );

        if hr.is_err() {
            return std::ptr::null_mut();
        }

        let blob = match signature_blob {
            Some(b) => b,
            None => return std::ptr::null_mut(),
        };

        let blob_slice = std::slice::from_raw_parts(
            blob.GetBufferPointer() as *const u8,
            blob.GetBufferSize()
        );

        match device.CreateRootSignature::<ID3D12RootSignature>(0, blob_slice) {
            Ok(sig) => {
                debug_println!("[create_root_signature_with_texture] ✅ Created");

                if let Ok(mut state) = STATE.lock() {
                    state.root_signature = Some(sig.clone());
                }

                let raw_ptr = sig.as_raw();
                std::mem::forget(sig);
                raw_ptr as *mut c_void
            }
            Err(_) => std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn create_simple_pso(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_simple_pso] Creating PSO...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        if root_sig_ptr.is_null() {
            debug_println!("[create_simple_pso] Root signature is null!");
            return std::ptr::null_mut();
        }

        let root_sig: ID3D12RootSignature = std::mem::transmute_copy(&root_sig_ptr);

        let vs_blob_ptr = crate::shader::get_builtin_vs_blob();
        let ps_blob_ptr = crate::shader::get_builtin_ps_blob();

        if vs_blob_ptr.is_null() || ps_blob_ptr.is_null() {
            debug_println!("[create_simple_pso] Failed to compile shaders");
            std::mem::forget(root_sig);
            return std::ptr::null_mut();
        }

        let vs_blob: ID3DBlob = std::mem::transmute(vs_blob_ptr);
        let ps_blob: ID3DBlob = std::mem::transmute(ps_blob_ptr);

        let vs_data = vs_blob.GetBufferPointer();
        let vs_size = vs_blob.GetBufferSize();
        let ps_data = ps_blob.GetBufferPointer();
        let ps_size = ps_blob.GetBufferSize();

        debug_println!("[create_simple_pso] VS: {} bytes, PS: {} bytes", vs_size, ps_size);

        let input_elements = [
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
        ];

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

        let pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root_sig.clone())),
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: vs_data,
                BytecodeLength: vs_size
            },
            PS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: ps_data,
                BytecodeLength: ps_size
            },
            DS: D3D12_SHADER_BYTECODE::default(),
            HS: D3D12_SHADER_BYTECODE::default(),
            GS: D3D12_SHADER_BYTECODE::default(),
            StreamOutput: D3D12_STREAM_OUTPUT_DESC::default(),
            BlendState: D3D12_BLEND_DESC {
                AlphaToCoverageEnable: false.into(),
                IndependentBlendEnable: false.into(),
                RenderTarget: [
                    default_blend, default_blend, default_blend, default_blend,
                    default_blend, default_blend, default_blend, default_blend,
                ],
            },
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC {
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
            DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
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
            RTVFormats: [
                DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_FORMAT_UNKNOWN,
                DXGI_FORMAT_UNKNOWN,
                DXGI_FORMAT_UNKNOWN,
                DXGI_FORMAT_UNKNOWN,
                DXGI_FORMAT_UNKNOWN,
                DXGI_FORMAT_UNKNOWN,
                DXGI_FORMAT_UNKNOWN
            ],
            DSVFormat: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
        };

        match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc) {
            Ok(pso) => {
                debug_println!("[create_simple_pso] ✅ PSO created!");

                crate::shader::free_blob(vs_blob_ptr);
                crate::shader::free_blob(ps_blob_ptr);

                let ptr = pso.as_raw();
                std::mem::forget(pso);
                std::mem::forget(root_sig);
                ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[create_simple_pso] Failed: {:?}", e);
                crate::shader::free_blob(vs_blob_ptr);
                crate::shader::free_blob(ps_blob_ptr);
                std::mem::forget(root_sig);
                std::ptr::null_mut()
            }
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