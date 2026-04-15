use std::ffi::c_void;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_core::{Interface, PCSTR};
use crate::{STATE, debug_println, utils::ptr_to_device};

#[no_mangle]
pub extern "C" fn create_root_signature(
    device_ptr: *mut c_void,
    _num_params: u32,
    _param_types: *const u32,
    _param_visibility: *const u32,
    _num_descriptors: *const u32,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_root_signature] Creating root signature with 1 CBV parameter");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        // Параметр 0: Constant Buffer View (b0, space0)
        let root_parameters = [D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,  // b0
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
        }];

        // Root signature без статических сэмплеров
        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_parameters.len() as u32,
            pParameters: root_parameters.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut serialized: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3D12SerializeRootSignature(
            &root_desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut serialized,
            Some(&mut error_blob),
        );

        if hr.is_err() {
            if let Some(err) = error_blob {
                let ptr = err.GetBufferPointer();
                let size = err.GetBufferSize();
                let msg = std::slice::from_raw_parts(ptr as *const u8, size);
                debug_println!("RootSig error: {}", String::from_utf8_lossy(msg));
            }
            return std::ptr::null_mut();
        }

        if let Some(blob) = serialized {
            let data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize()
            );

            match device.CreateRootSignature::<ID3D12RootSignature>(0, data) {
                Ok(rs) => {
                    let ptr = rs.as_raw();

                    // Сохраняем в глобальное состояние
                    if let Ok(mut state) = STATE.lock() {
                        state.root_signature = Some(rs.clone());
                    }

                    std::mem::forget(rs);
                    debug_println!("[create_root_signature] ✅ OK");
                    return ptr as *mut c_void;
                }
                Err(e) => debug_println!("[create_root_signature] Failed: {:?}", e),
            }
        }
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub extern "C" fn create_simple_pso(
    device_ptr: *mut c_void,
    root_sig_ptr: *mut c_void,
) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_simple_pso] Compiling shaders from files...");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let root_sig: ID3D12RootSignature = std::mem::transmute_copy(&root_sig_ptr);

        let vs_path = std::ffi::CString::new("shaders/default_vs.hlsl").unwrap();
        let ps_path = std::ffi::CString::new("shaders/default_ps.hlsl").unwrap();
        let entry = std::ffi::CString::new("main").unwrap();
        let vs_profile = std::ffi::CString::new("vs_5_0").unwrap();
        let ps_profile = std::ffi::CString::new("ps_5_0").unwrap();

        let mut vs_blob_ptr: *mut c_void = std::ptr::null_mut();
        if !crate::shader::compile_shader_from_file(
            vs_path.as_ptr(),
            entry.as_ptr(),
            vs_profile.as_ptr(),
            &mut vs_blob_ptr,
        ) {
            return std::ptr::null_mut();
        }

        let mut ps_blob_ptr: *mut c_void = std::ptr::null_mut();
        if !crate::shader::compile_shader_from_file(
            ps_path.as_ptr(),
            entry.as_ptr(),
            ps_profile.as_ptr(),
            &mut ps_blob_ptr,
        ) {
            return std::ptr::null_mut();
        }

        let vs_blob: ID3DBlob = std::mem::transmute(vs_blob_ptr);
        let ps_blob: ID3DBlob = std::mem::transmute(ps_blob_ptr);

        let vs_data = vs_blob.GetBufferPointer();
        let vs_size = vs_blob.GetBufferSize();
        let ps_data = ps_blob.GetBufferPointer();
        let ps_size = ps_blob.GetBufferSize();

        debug_println!("[create_simple_pso] VS: {} bytes, PS: {} bytes", vs_size, ps_size);

        // ПОЛНЫЙ INPUT LAYOUT для Vertex (80 байт)
        // Порядок полей в Vertex:
        // position: [f32; 3]   - 12 байт, смещение 0
        // normal: [f32; 3]     - 12 байт, смещение 12
        // tangent: [f32; 3]    - 12 байт, смещение 24
        // bitangent: [f32; 3]  - 12 байт, смещение 36
        // uv: [f32; 2]         - 8 байт,  смещение 48
        // uv2: [f32; 2]        - 8 байт,  смещение 56
        // color: [f32; 4]      - 16 байт, смещение 64
        // Всего: 80 байт

        let input_elements = [
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"POSITION\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"NORMAL\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 12,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"TANGENT\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 24,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"BITANGENT\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 36,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"TEXCOORD\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 48,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"TEXCOORD\0".as_ptr()),
                SemanticIndex: 1,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 56,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(b"COLOR\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 64,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];

        // Проверяем общий размер
        let total_size = 12 + 12 + 12 + 12 + 8 + 8 + 16;
        debug_println!("[create_simple_pso] Input layout total size: {} bytes", total_size);
        debug_println!("[create_simple_pso] Vertex size should be: {} bytes", std::mem::size_of::<crate::Vertex>());

        // Создаём PSO
        let pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root_sig)),
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
                RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: false.into(),
                    LogicOpEnable: false.into(),
                    SrcBlend: D3D12_BLEND_ONE,
                    DestBlend: D3D12_BLEND_ZERO,
                    BlendOp: D3D12_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D12_BLEND_ONE,
                    DestBlendAlpha: D3D12_BLEND_ZERO,
                    BlendOpAlpha: D3D12_BLEND_OP_ADD,
                    LogicOp: D3D12_LOGIC_OP_NOOP,
                    RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
                }; 8],
            },
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_BACK,  // Включаем back-face culling
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
                DepthEnable: true.into(),  // Включаем depth test
                DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D12_COMPARISON_FUNC_LESS,
                StencilEnable: false.into(),
                StencilReadMask: 0xFF,
                StencilWriteMask: 0xFF,
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
            RTVFormats: [DXGI_FORMAT_R8G8B8A8_UNORM; 8],
            DSVFormat: DXGI_FORMAT_D32_FLOAT,  // Добавляем depth-stencil формат
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
        };

        match device.CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc) {
            Ok(pso) => {
                debug_println!("[create_simple_pso] ✅ PSO created successfully!");
                let ptr = pso.as_raw();
                std::mem::forget(pso);
                ptr as *mut c_void
            }
            Err(e) => {
                debug_println!("[create_simple_pso] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}