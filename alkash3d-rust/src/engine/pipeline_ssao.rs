//! SSAO (Screen-Space Ambient Occlusion) — контактные тени в местах, где
//! геометрия сходится (углы, щели, основания объектов), которые directional-
//! свет + константный ambient-член основного пиксельного шейдера физически
//! не могут дать (ambient сейчас — плоская константа, одинаковая для всей
//! сцены, см. `ambient` в `compile_default_shaders`).
//!
//! Тот же паттерн, что и у volumetric god rays (`pipeline_volumetric.rs`):
//! half-res screen-space проход, честно документированное упрощение вместо
//! физически точного решения. Два ключевых отличия от "учебниковой" SSAO:
//!
//! 1. НЕТ отдельного normal G-buffer (движок — forward renderer, не
//!    deferred) — нормаль пикселя реконструируется из экранных производных
//!    (ddx/ddy) его мировой позиции, восстановленной из глубины (тот же
//!    приём реконструкции позиции, что уже использует volumetric).
//! 2. НЕТ отдельного blur-прохода — шум от малого числа сэмплов (12)
//!    сглаживается per-pixel хэш-варьируемым паттерном сэмплов (см.
//!    `hash1` в шейдере) + bilinear-апскейл при финальном чтении в
//!    tonemap (тот же trick, что уже используют bloom/volumetric).
//!
//! Результат — множитель [0,1] на итоговый цвет (не только на ambient —
//! честное разделение потребовало бы depth pre-pass'а, отдельная, более
//! крупная доработка), применяется в tonemap composite-проходе ДО суммы с
//! bloom/volumetric (те — источники света/атмосфера, не заслоняемая
//! геометрией поверхность).
//!
//! ВЫНЕСЕНО в отдельный файл (не в `pipeline_volumetric.rs`) — отдельный
//! логический проход с собственными root signature/PSO/ресурсами, тот же
//! принцип модульности, что уже применён к bloom/shadow/occluder/volumetric.

use windows::core::*;
use windows::Win32::Foundation::*;
use crate::STATE;
use crate::shader::ShaderBlob;
use crate::buffer::Buffer;
use super::AlkashEngine;

impl AlkashEngine {
    /// Компилирует вершинный (тот же fullscreen-triangle приём, что и у
    /// tonemap/bloom/volumetric) и пиксельный шейдеры SSAO-прохода.
    pub(super) fn compile_ssao_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };
        VS_OUTPUT main(uint id : SV_VertexID) {
            VS_OUTPUT output;
            output.uv = float2((id << 1) & 2, id & 2);
            output.pos = float4(output.uv.x * 2.0 - 1.0, 1.0 - output.uv.y * 2.0, 0.0, 1.0);
            return output;
        }
        "#;

        let ps_source = r#"
        // ИЗМЕНЕНО (максимальная графика — MSAA): основной depth-таргет
        // теперь многосэмпловый (см. MSAA_SAMPLES) — читаем сэмпл 0 через
        // Texture2DMS.Load, тот же приём и то же обоснование, что уже
        // применяется в volumetric (compile_volumetric_shaders).
        Texture2DMS<float> DepthBuffer : register(t0);
        SamplerState PointSampler : register(s0);

        cbuffer SSAOParams : register(b0) {
            float4x4 viewProj;
            float4x4 invViewProj;
            float3   cameraPos;
            float    radius;
            float    bias;
            float    strength;
            float2   _padding0;
        };

        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };

        static const int NUM_SAMPLES = 12;

        float hash1(float2 p, float2 seed) {
            return frac(sin(dot(p + seed, float2(12.9898, 78.233))) * 43758.5453);
        }

        // Та же формула восстановления мировой позиции по глубине, что уже
        // проверена в volumetric (compile_volumetric_shaders) — переиспользуем
        // намеренно один и тот же, уже подтверждённый вживую вывод, а не
        // выводим параллельную view-space версию с риском перепутать знак/
        // handedness там, где я не могу быстро увидеть результат глазами.
        float3 reconstructWorldPos(float2 uv, float depth) {
            float ndcX = uv.x * 2.0 - 1.0;
            float ndcY = 1.0 - uv.y * 2.0;
            float4 clip = float4(ndcX, ndcY, depth, 1.0);
            float4 world = mul(invViewProj, clip);
            return world.xyz / world.w;
        }

        float4 main(PS_INPUT input) : SV_TARGET {
            uint depthW, depthH, depthSamples;
            DepthBuffer.GetDimensions(depthW, depthH, depthSamples);
            int2 depthCoord = int2(input.uv * float2(depthW, depthH));
            float depth = DepthBuffer.Load(depthCoord, 0).r;

            // depth == 1.0 — небо/пустота, окклюзии в принципе нет.
            if (depth >= 0.9999) {
                return float4(1.0, 1.0, 1.0, 1.0);
            }

            float3 worldPos = reconstructWorldPos(input.uv, depth);

            // Нормаль из экранных производных мировой позиции — заменяет
            // normal G-buffer, которого у этого (forward) рендерера нет.
            // Знак выбирается так, чтобы нормаль ГАРАНТИРОВАННО смотрела на
            // камеру, независимо от знакового соглашения ddx/ddy — защита
            // от переворота окклюзии "наизнанку" на части экрана.
            float3 normal = normalize(cross(ddx(worldPos), ddy(worldPos)));
            if (dot(normal, cameraPos - worldPos) < 0.0) {
                normal = -normal;
            }

            float3 up = (abs(normal.y) < 0.99) ? float3(0.0, 1.0, 0.0) : float3(1.0, 0.0, 0.0);
            float3 tangent = normalize(cross(up, normal));
            float3 bitangent = cross(normal, tangent);

            float occlusion = 0.0;
            [unroll]
            for (int i = 0; i < NUM_SAMPLES; i++) {
                // Косинус-взвешенное распределение по полусфере в ЛОКАЛЬНОМ
                // (относительно нормали) пространстве — u1/u2 варьируются и
                // по пикселю (input.uv), и по индексу сэмпла (seed), поэтому
                // соседние пиксели получают РАЗНЫЕ паттерны сэмплов — тот же
                // приём, что убирает "полосы" у volumetric raymarch (см. его
                // jitter), только здесь заменяет собой отдельный blur-проход
                // целиком, а не только маскирует шаг вдоль луча.
                float2 seed = float2(float(i) * 0.13, float(i) * 0.71);
                float u1 = hash1(input.uv, seed);
                float u2 = hash1(input.uv, seed + float2(0.37, 0.91));
                float r = sqrt(u1);
                float theta = 6.28318530718 * u2;
                float lx = r * cos(theta);
                float ly = r * sin(theta);
                float lz = sqrt(max(0.0, 1.0 - u1));
                // Сэмплы ближе к центру полусферы весят больше — стандартный
                // приём SSAO-ядер (кластеризация сэмплов у начала координат),
                // даёт более выраженную окклюзию у самых близких преград.
                float t = (float(i) + 0.5) / float(NUM_SAMPLES);
                float scale = lerp(0.1, 1.0, t * t);
                float3 localDir = float3(lx, ly, lz) * scale;
                float3 worldDir = tangent * localDir.x + bitangent * localDir.y + normal * localDir.z;

                float3 samplePos = worldPos + worldDir * radius;

                float4 clip = mul(viewProj, float4(samplePos, 1.0));
                if (clip.w <= 0.0001) continue;
                float3 ndc = clip.xyz / clip.w;
                float2 sampleUV = float2(ndc.x * 0.5 + 0.5, 1.0 - (ndc.y * 0.5 + 0.5));
                if (sampleUV.x < 0.0 || sampleUV.x > 1.0 || sampleUV.y < 0.0 || sampleUV.y > 1.0) continue;

                int2 sampleDepthCoord = int2(sampleUV * float2(depthW, depthH));
                float sceneDepth = DepthBuffer.Load(sampleDepthCoord, 0).r;
                if (sceneDepth >= 0.9999) continue; // небо в этом направлении — окклюдировать нечем

                float3 sceneWorldPos = reconstructWorldPos(sampleUV, sceneDepth);

                float distSample = length(samplePos - cameraPos);
                float distScene = length(sceneWorldPos - cameraPos);

                // Затухание вклада при большой разнице глубин — стандартный
                // приём SSAO (Crysis-style range check), убирает широкие
                // "ореолы" окклюзии вокруг тонких/далёких объектов.
                float rangeCheck = saturate(radius / max(abs(distSample - distScene), 0.0001));
                occlusion += ((distScene <= distSample - bias) ? 1.0 : 0.0) * rangeCheck;
            }
            occlusion = occlusion / float(NUM_SAMPLES);

            float ao = 1.0 - saturate(occlusion * strength);
            return float4(ao, ao, ao, 1.0);
        }
        "#;

        self.ssao_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.ssao_ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ SSAO shaders compiled ({} сэмплов, без normal G-buffer/blur-прохода)", 12);
        Ok(())
    }

    /// Root signature SSAO-прохода — SRV table t0 (depth, MSAA) + point
    /// sampler s0 + CBV b0 (SSAOParams). Та же форма, что и у volumetric
    /// (`create_volumetric_root_signature`), только БЕЗ второго входа
    /// (shadow map SSAO не нужна) и без comparison-сэмплера.
    pub(super) fn create_ssao_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let point_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 0,
            ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
            BorderColor: D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 0,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        let static_samplers = [point_sampler];

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];

        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: static_samplers.len() as u32,
            pStaticSamplers: static_samplers.as_ptr(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
        };

        let device = crate::get_device()?;
        let mut signature_serialized = None;
        let mut error_blob = None;

        unsafe {
            let hr = D3D12SerializeRootSignature(
                &root_signature_desc,
                D3D_ROOT_SIGNATURE_VERSION_1,
                &mut signature_serialized,
                Some(&mut error_blob),
            );

            if hr.is_err() {
                if let Some(err) = error_blob {
                    let err_data = std::slice::from_raw_parts(
                        err.GetBufferPointer() as *const u8,
                        err.GetBufferSize(),
                    );
                    eprintln!("SSAO root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.ssao_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ SSAO root signature created (SRV table t0 + point sampler s0 + CBV b0)");
        Ok(())
    }

    /// PSO SSAO-прохода — та же форма, что у tonemap/bloom/volumetric (нет
    /// input layout, нет depth-теста, полноэкранный треугольник). RTV
    /// формат — тот же R16G16B16A16_FLOAT-хелпер, что и у остальных
    /// half-res таргетов (см. `create_ssao_resources`) — AO переиспользует
    /// его исключительно ради переиспользования уже отлаженного кода
    /// создания текстуры, реально используется только R-канал.
    pub(super) fn create_ssao_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.ssao_vs.as_ref().unwrap();
        let ps = self.ssao_ps.as_ref().unwrap();
        let root_sig = self.ssao_root_signature.as_ref().unwrap();
        let device = crate::get_device()?;

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

        let depth_stencil = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: FALSE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
            DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
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

        let mut pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::ManuallyDrop::new(Some(root_sig.clone())),
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
            InputLayout: D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: std::ptr::null(),
                NumElements: 0,
            },
            IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            RTVFormats: [DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN],
            DSVFormat: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
            CS: D3D12_SHADER_BYTECODE::default(),
        };

        let result = unsafe { device.CreateGraphicsPipelineState(&pso_desc) };
        unsafe {
            std::mem::ManuallyDrop::drop(&mut pso_desc.pRootSignature);
        }
        let pso = result?;
        self.ssao_pipeline_state = Some(pso);

        println!("[ENGINE] ✓ SSAO pipeline state created");
        Ok(())
    }

    /// Создаёт half-res AO render target + его RTV/SRV heap (depth @ t0,
    /// СВОЯ копия, не разделяемая с volumetric — тот же принцип, что уже
    /// применяет `create_volumetric_resources` для своего depth SRV: каждый
    /// потребитель делает свою собственную SRV-запись поверх
    /// `renderer.depth_stencil`, а не пытается шарить один центральный
    /// хип между проходами с разными root signature/descriptor table
    /// формами) + CBV buffer под SSAOParams. Вызывается ПОСЛЕ создания
    /// `renderer` (нужен `renderer.depth_stencil`).
    pub(super) fn create_ssao_resources(&mut self) -> Result<()> {
        let ao_width = (self.width / 2).max(1);
        let ao_height = (self.height / 2).max(1);

        let texture = crate::render::RenderTexture::create_hdr_target(ao_width, ao_height, 1, windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_RENDER_TARGET)?;

        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(1)?;
        let depth_srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(1)?;

        let rtv_size = {
            let state = STATE.lock().unwrap();
            state.rtv_descriptor_size
        };
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        let rtv = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, 0, rtv_size);
        texture.create_rtv(rtv)?;

        let depth_dst = crate::heap::DescriptorHeap::get_cpu_handle(&depth_srv_heap, 0, cbv_srv_uav_size);
        let depth_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&depth_srv_heap, 0, cbv_srv_uav_size);
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_ssao_resources() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        renderer.depth_stencil.create_depth_srv(depth_dst)?;

        self.ssao_texture = Some(texture);
        self.ssao_rtv = rtv;
        self.ssao_rtv_heap = Some(rtv_heap);
        self.ssao_depth_srv_heap = Some(depth_srv_heap);
        self.ssao_srv_gpu_depth = depth_gpu;

        if self.ssao_constant_buffer.is_none() {
            let params_cb = Buffer::create_constant_buffer(256)?;
            self.ssao_constant_buffer = Some(params_cb);
        }

        println!("[ENGINE] ✓ SSAO resources created: {}x{} half-res target", ao_width, ao_height);

        self.create_ssao_final_srv()?;

        Ok(())
    }

    /// Регистрирует SRV AO-таргета в индексе 3 `renderer.srv_uav_heap`
    /// (t3 в tonemap composite-шейдере, см. `create_tonemap_root_signature`
    /// в pipeline_post.rs) — тот же приём разделения "создание ресурса" /
    /// "регистрация в чужом хипе", что и `create_volumetric_final_srv`.
    fn create_ssao_final_srv(&mut self) -> Result<()> {
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_ssao_final_srv() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        let texture = self.ssao_texture.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_ssao_final_srv() called before ssao_texture created");
            Error::from_hresult(HRESULT(1))
        })?;
        let dst = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 3, cbv_srv_uav_size);
        texture.create_srv(dst)?;
        Ok(())
    }
}
