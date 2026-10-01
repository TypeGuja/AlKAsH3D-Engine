//! Volumetric god-rays: SRV основного depth-таргета, half-res screen-space
//! raymarch-проход (реконструкция мировой позиции из глубины + шаги вдоль
//! луча камера->пиксель с PCF-опросом shadow map каждого шага), его
//! root signature/PSO/ресурсы.
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись.

use windows::core::*;
use windows::Win32::Foundation::*;
use crate::STATE;
use crate::shader::ShaderBlob;
use crate::buffer::Buffer;
use super::AlkashEngine;

impl AlkashEngine {
    /// Создаёт SRV основного depth-таргета (`renderer.depth_stencil`) — см.
    /// подробное обоснование TYPELESS-перевода этого ресурса у
    /// `RenderTexture::create_depth_stencil` в render.rs. Вызывается ПОСЛЕ
    /// того, как renderer уже создан (в отличие от shadow-ресурсов —
    /// depth-таргет живёт внутри `Renderer`, а не отдельно).
    pub(super) fn create_depth_srv_resources(&mut self) -> Result<()> {
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_depth_srv_resources() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;

        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(1)?;
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        renderer.depth_stencil.create_depth_srv(srv_cpu)?;

        self.depth_srv_heap = Some(srv_heap);
        self.depth_srv_gpu = srv_gpu;

        println!("[ENGINE] ✓ Depth SRV created (для volumetric raymarch)");
        Ok(())
    }

    /// Компилирует вершинный (переиспользует ту же fullscreen-triangle
    /// технику, что и tonemap/bloom — см. `self.tonemap_vs`) и пиксельный
    /// шейдеры volumetric raymarch-прохода.
    ///
    /// Идея: для каждого экранного пикселя восстанавливаем его мировую
    /// позицию из сохранённой глубины (через инвертированную view-proj
    /// матрицу камеры — тот же математический приём, что уже используется
    /// в `compute_cascade_view_proj` для обратного преобразования NDC-
    /// углов фрустума в мировые координаты), затем идём МАЛЫМИ шагами
    /// вдоль луча камера->этот пиксель и на каждом шаге спрашиваем shadow
    /// map: "виден ли этот участок воздуха солнцу, или он в тени?" —
    /// сумма "видимых" шагов (с весом, убывающим к краю дальности) даёт
    /// яркость god ray в этом пикселе. Классический screen-space
    /// volumetric lighting приём (та же идея, что "God Rays"/"Crepuscular
    /// Rays" в играх) — не физически точная симуляция рассеяния в
    /// атмосфере, а дешёвая, устойчивая по кадрам аппроксимация, которая
    /// на зафиксированном минимуме железа (RTX 3050 8GB) должна оставаться
    /// в разумном бюджете кадра благодаря half-res выполнению (см.
    /// `create_volumetric_resources`) и небольшому фиксированному числу
    /// шагов (NUM_STEPS ниже).
    pub(super) fn compile_volumetric_shaders(&mut self) -> Result<()> {
        let vs_source = include_str!("shaders/fullscreen_vs.hlsl");

        // ДОБАВЛЕНО (runtime-переключаемый MSAA — по прямому запросу
        // пользователя, см. GraphicsSettings в engine/mod.rs и тот же
        // приём в `compile_ssao_shaders`): при MSAA включён основной
        // depth-таргет многосэмпловый — SRV на него ОБЯЗАН быть
        // Texture2DMS, а не обычный Texture2D (иначе ошибка валидации
        // D3D12 при создании SRV, см. `RenderTexture::create_depth_srv`).
        // `.Load`/`.GetDimensions` имеют разную сигнатуру между ними —
        // изолируем разницу в двух HLSL-функциях (LoadDepth/GetDepthDims)
        // вместо дублирования всего raymarch-алгоритма в двух копиях. Выбор
        // делается через `#ifdef MSAA` в shaders/volumetric_ps.hlsl; `#line 1`
        // сохраняет номера строк в ошибках такими же, как в .hlsl-файле.
        let msaa_define = if self.msaa_samples > 1 { "#define MSAA 1\n#line 1\n" } else { "" };
        let ps_source = format!("{}{}", msaa_define, include_str!("shaders/volumetric_ps.hlsl"));

        self.volumetric_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.volumetric_ps = Some(ShaderBlob::compile(&ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Volumetric shaders compiled (screen-space raymarch, {} шагов, MSAA={}x)", 24, self.msaa_samples);
        Ok(())
    }

    /// Root signature volumetric raymarch-прохода: descriptor table с
    /// ДВУМЯ смежными SRV (t0 = depth, t1 = shadow map — оба должны быть
    /// смежными дескрипторами в одном heap, см. `create_volumetric_resources`),
    /// точечный сэмплер (s0, для depth) + comparison-сэмплер (s1, для
    /// shadow map — идентичен по параметрам сэмплеру s0 основного
    /// прохода, см. `create_root_signature`), и один CBV (b0) с
    /// параметрами (см. `VolumetricParams` в шейдере выше).
    pub(super) fn create_volumetric_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 2,
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

        let shadow_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            MipLODBias: 0.0,
            MaxAnisotropy: 0,
            ComparisonFunc: D3D12_COMPARISON_FUNC_LESS,
            BorderColor: D3D12_STATIC_BORDER_COLOR_OPAQUE_WHITE,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 1,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        let static_samplers = [point_sampler, shadow_sampler];

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
                    eprintln!("Volumetric root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.volumetric_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Volumetric root signature created (SRV table t0/t1 + point sampler s0 + comparison sampler s1 + CBV b0)");
        Ok(())
    }

    /// PSO volumetric-прохода — та же форма, что и bloom/tonemap (нет
    /// input layout, нет depth-теста, полноэкранный треугольник), RTV
    /// формат HDR (float, чтобы не терять яркость god rays до тонмаппинга,
    /// как и весь остальной свет в этом движке начиная с Фазы 5).
    pub(super) fn create_volumetric_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.volumetric_vs.as_ref().unwrap();
        let ps = self.volumetric_ps.as_ref().unwrap();
        let root_sig = self.volumetric_root_signature.as_ref().unwrap();
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
        self.volumetric_pipeline_state = Some(pso);

        println!("[ENGINE] ✓ Volumetric pipeline state created");
        Ok(())
    }

    /// Создаёт half-res volumetric render target + его RTV/SRV + SRV heap
    /// с depth (t0) и shadow map (t1) СМЕЖНЫМИ дескрипторами (обязательное
    /// требование D3D12 для одной descriptor table с одним диапазоном на
    /// несколько ресурсов, см. `create_volumetric_root_signature`) + CBV
    /// buffer под VolumetricParams. Вызывается ПОСЛЕ `create_shadow_resources`
    /// и `create_depth_srv_resources` (нужны их результаты — self.shadow_maps,
    /// self.depth_srv_heap — чтобы скопировать соответствующие дескрипторы
    /// в новый смежный heap).
    /// ВРЕМЕННО (бисекция DXGI_ERROR_DEVICE_HUNG — см. пару
    /// `disable_shadows_for_diagnostics` в pipeline_shadow.rs, тот же
    /// приём): `render_frame()` пропускает весь volumetric-проход, если
    /// `volumetric_texture.is_none()`. Вызвать сразу после `init()`.
    pub fn disable_volumetric_for_diagnostics(&mut self) {
        println!("[DIAG] Volumetric-проход принудительно отключён для диагностики зависания");
        self.volumetric_texture = None;
    }

    pub(super) fn create_volumetric_resources(&mut self) -> Result<()> {
        let vol_width = (self.width / 2).max(1);
        let vol_height = (self.height / 2).max(1);

        // ИЗМЕНЕНО (максимальная графика — MSAA, см. `create_hdr_target` в
        // render.rs): volumetric-таргет — одноимпловый обычный render
        // target, как и раньше; этот вызов не связан с MSAA основного
        // цветового прохода.
        let texture = crate::render::RenderTexture::create_hdr_target(vol_width, vol_height, 1, windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_RENDER_TARGET)?;

        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(1)?;
        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(3)?;

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

        let cascade_for_volumetric: usize = 0;
        let dst0 = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        let dst1 = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 1, cbv_srv_uav_size);
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_resources() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        renderer.depth_stencil.create_depth_srv(dst0)?;
        let shadow_map = self.shadow_maps[cascade_for_volumetric].as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_resources() called before create_shadow_resources()");
            Error::from_hresult(HRESULT(1))
        })?;
        shadow_map.create_shadow_srv(dst1)?;

        let srv2_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 2, cbv_srv_uav_size);
        let srv2_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 2, cbv_srv_uav_size);
        texture.create_srv(srv2_cpu)?;

        let raymarch_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);

        self.volumetric_texture = Some(texture);
        self.volumetric_rtv = rtv;
        self.volumetric_srv_gpu_final = srv2_gpu;
        self.volumetric_rtv_heap = Some(rtv_heap);
        self.volumetric_srv_heap = Some(srv_heap);
        self.volumetric_srv_gpu_raymarch = raymarch_gpu;

        let params_cb = Buffer::create_constant_buffer(256)?;
        self.volumetric_constant_buffer = Some(params_cb);

        println!(
            "[ENGINE] ✓ Volumetric resources created: {}x{} half-res target",
            vol_width, vol_height
        );

        self.create_volumetric_final_srv()?;

        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
    /// подсветка): регистрирует SRV volumetric-таргета в индексе 2
    /// `renderer.srv_uav_heap` — вынесено в отдельный метод (а не
    /// встроено прямо в `create_volumetric_resources`), т.к. требует
    /// одновременного заимствования `self.renderer` (по ссылке) и
    /// `self.volumetric_texture` (тоже по ссылке) — оба READ-ONLY на этом
    /// шаге, так что заимствование безопасно; вынесено отдельным методом
    /// ради явного разделения "создание ресурса" / "регистрация в чужом
    /// хипе" (`create_bloom_resources` делает то же самое инлайн, без
    /// разделения — здесь тот же эффект достигается отдельным вызовом).
    fn create_volumetric_final_srv(&mut self) -> Result<()> {
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_final_srv() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        let texture = self.volumetric_texture.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_final_srv() called before volumetric_texture created");
            Error::from_hresult(HRESULT(1))
        })?;
        let dst = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 2, cbv_srv_uav_size);
        texture.create_srv(dst)?;
        Ok(())
    }
}
