// src/pso.rs
use windows::core::*;
use windows::Win32::Foundation::{FALSE, TRUE};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use crate::{STATE, ShaderBlob};

pub struct PipelineState;

impl PipelineState {
    pub fn create_graphics(
        vs: &ShaderBlob,
        ps: &ShaderBlob,
        root_signature: &ID3D12RootSignature,
        _vertex_stride: u32,
        render_target_format: DXGI_FORMAT,
        depth_format: DXGI_FORMAT,
    ) -> Result<ID3D12PipelineState> {
        println!("[PSO] ========== CREATING GRAPHICS PIPELINE STATE ==========");
        println!("[PSO] VS size: {} bytes", vs.size());
        println!("[PSO] PS size: {} bytes", ps.size());
        println!("[PSO] RTV format: {:?}", render_target_format);
        println!("[PSO] DSV format: {:?}", depth_format);

        let device = {
            let state = STATE.lock().unwrap();
            match &state.device {
                Some(d) => {
                    println!("[PSO] Device obtained successfully");
                    d.clone()
                },
                None => {
                    eprintln!("[PSO] ERROR: Device is None!");
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        println!("[PSO] Creating input layout...");
        let input_elements = [
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
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
                AlignedByteOffset: 16,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];

        let input_layout = D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        };
        println!("[PSO] Input layout created: {} elements", input_elements.len());

        println!("[PSO] Creating rasterizer state...");
        let rasterizer = D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
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
        println!("[PSO] Rasterizer: FillMode=Solid, CullMode=None");

        println!("[PSO] Creating blend state...");
        let blend_desc = D3D12_BLEND_DESC {
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
        println!("[PSO] Blend state created");

        println!("[PSO] Creating depth stencil state (ENABLED)...");
        let depth_stencil = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: TRUE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
            StencilEnable: FALSE,
            StencilReadMask: D3D12_DEFAULT_STENCIL_READ_MASK as u8,
            StencilWriteMask: D3D12_DEFAULT_STENCIL_WRITE_MASK as u8,
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
        };
        println!("[PSO] Depth stencil: DepthEnable=TRUE, DepthWriteMask=ALL");

        println!("[PSO] Root signature: {:p}", root_signature);

        // ИСПРАВЛЕНО: pRootSignature в D3D12_GRAPHICS_PIPELINE_STATE_DESC
        // имеет тип ManuallyDrop<Option<ID3D12RootSignature>>. Строкой ниже
        // мы клонируем root_signature (это увеличивает COM refcount на 1),
        // поэтому обязаны сами явно уменьшить его обратно после того, как
        // pso_desc отработал — раньше этого не делалось, и на каждый вызов
        // create_graphics() (например, при hot-reload шейдеров) утекала
        // одна лишняя ссылка на root signature.
        let mut pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::ManuallyDrop::new(Some(root_signature.clone())),
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: vs.as_ptr(),
                BytecodeLength: vs.size(),
            },
            PS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: ps.as_ptr(),
                BytecodeLength: ps.size(),
            },
            DS: D3D12_SHADER_BYTECODE::default(),
            HS: D3D12_SHADER_BYTECODE::default(),
            GS: D3D12_SHADER_BYTECODE::default(),
            StreamOutput: D3D12_STREAM_OUTPUT_DESC::default(),
            BlendState: blend_desc,
            SampleMask: u32::MAX,
            RasterizerState: rasterizer,
            DepthStencilState: depth_stencil,
            InputLayout: input_layout,
            IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            RTVFormats: [render_target_format, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN],
            DSVFormat: depth_format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
            CS: Default::default(),
        };
        println!("[PSO] Pipeline state descriptor created");

        println!("[PSO] Calling CreateGraphicsPipelineState...");
        let result = unsafe { device.CreateGraphicsPipelineState(&pso_desc) };

        // Освобождаем нашу дополнительную ссылку на root signature — она
        // была нужна только на время жизни pso_desc выше.
        unsafe {
            std::mem::ManuallyDrop::drop(&mut pso_desc.pRootSignature);
        }

        match &result {
            Ok(pso) => {
                println!("[PSO] ✓ PSO created successfully! PSO: {:p}", pso);
            },
            Err(e) => {
                eprintln!("[PSO] ✗ Failed to create PSO: {:?}", e);
            }
        }
        result
    }
}