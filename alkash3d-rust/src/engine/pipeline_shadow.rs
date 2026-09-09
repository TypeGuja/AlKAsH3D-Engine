//! Cascaded shadow maps: depth-таргеты каскадов + их DSV/SRV, depth-only
//! shadow-проход (шейдер/root signature/PSO) и подгонка ортографического
//! объёма света под видимый frustum камеры на каждый каскад
//! (`compute_cascade_view_proj`, со стабилизацией "shadow swimming" —
//! фиксированный радиус + texel-snapping).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D12::*;
use crate::STATE;
use crate::shader::ShaderBlob;
use crate::math::{Vec3, Mat4};
use super::{AlkashEngine, NUM_CASCADES, SHADOW_MAP_RESOLUTION};

impl AlkashEngine {
    /// ОБНОВЛЕНО (каскадные тени / CSM — расширение Фазы 6): создаёт
    /// `NUM_CASCADES` depth-таргетов shadow map (по одному на каскад), их
    /// DSV (для shadow-прохода — запись глубины) и SRV, все SМЕЖНЫЕ в
    /// одном shader-visible heap (для основного 3D-прохода — чтение/
    /// сравнение глубины через PCF, см. подробности у поля
    /// `shadow_srv_heap`). Вызывается один раз в init(), НЕ пересоздаётся
    /// при resize (см. подробное объяснение у полей `shadow_maps` и
    /// `SHADOW_MAP_RESOLUTION` — разрешение shadow map не зависит от
    /// размера окна).
    pub(super) fn create_shadow_resources(&mut self) -> Result<()> {
        let dsv_heap = crate::heap::DescriptorHeap::create_dsv_heap(NUM_CASCADES as u32)?;
        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(NUM_CASCADES as u32)?;

        let dsv_size = {
            let state = STATE.lock().unwrap();
            state.dsv_descriptor_size
        };
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        for cascade in 0..NUM_CASCADES {
            let shadow_map = crate::render::RenderTexture::create_shadow_map(SHADOW_MAP_RESOLUTION)?;

            let dsv = crate::heap::DescriptorHeap::get_cpu_handle(&dsv_heap, cascade as u32, dsv_size);
            shadow_map.create_dsv(dsv)?;

            let srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, cascade as u32, cbv_srv_uav_size);
            shadow_map.create_shadow_srv(srv_cpu)?;

            self.shadow_maps[cascade] = Some(shadow_map);
            self.shadow_dsvs[cascade] = dsv;
        }

        let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        self.shadow_dsv_heap = Some(dsv_heap);
        self.shadow_srv_heap = Some(srv_heap);
        self.shadow_srv_gpu = srv_gpu;

        println!(
            "[ENGINE] ✓ Shadow map resources created: {} каскадов по {}x{}",
            NUM_CASCADES, SHADOW_MAP_RESOLUTION, SHADOW_MAP_RESOLUTION
        );
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): вершинный
    /// шейдер shadow-прохода. Единственная задача — записать глубину
    /// объекта С ТОЧКИ ЗРЕНИЯ СВЕТА в shadow map; пиксельного шейдера НЕТ
    /// ВООБЩЕ (D3D12 разрешает PSO без PS для depth-only рендеринга — GPU
    /// сам записывает глубину в DSV по растеризованным треугольникам, а
    /// цвет никуда не пишется, т.к. NumRenderTargets=0, см.
    /// `create_shadow_pipeline_state`). Раздельный CBV (не переиспользует
    /// TransformConstants основного прохода) — здесь нужна только ОДНА
    /// матрица (model * light_view_proj), без камеры/света/сетки каллинга,
    /// которые этому проходу не нужны вообще.
    pub(super) fn compile_shadow_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        cbuffer ShadowConstants : register(b0) {
            float4x4 modelLightViewProj;
        };

        struct VS_INPUT {
            float4 pos : POSITION;
            float3 normal : NORMAL;
            float4 color : COLOR;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            output.pos = mul(modelLightViewProj, input.pos);
            return output;
        }
        "#;

        self.shadow_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        println!("[ENGINE] ✓ Shadow shaders compiled (depth-only, без PS)");
        Ok(())
    }

    /// Отдельная root signature shadow-прохода: ОДИН CBV (b0) — матрица
    /// model*light_view_proj, ничего больше (нет SRV фонарей/сетки, нет
    /// сэмплеров — этому проходу они не нужны).
    pub(super) fn create_shadow_root_signature(&mut self) -> Result<()> {
        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
            },
        ];

        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
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
                    eprintln!("Shadow root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.shadow_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Shadow root signature created (CBV b0, только матрица)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): PSO
    /// shadow-прохода — depth-only, без PS, без RTV вообще
    /// (NumRenderTargets=0). Input layout ОБЯЗАН совпадать с основным 3D
    /// PSO (POSITION/NORMAL/COLOR) — рисуется ТА ЖЕ геометрия (те же
    /// вершинные/индексные буферы), просто другим шейдером/матрицей.
    ///
    /// DepthBias/SlopeScaledDepthBias вместо (или в дополнение к)
    /// шейдерного shadow_bias (см. TransformConstants) — растеризатор
    /// сдвигает записываемую глубину аппаратно, что везде считается
    /// стандартной практикой против "shadow acne" (самозатенение
    /// поверхности из-за конечной точности глубины). Используем оба
    /// механизма вместе: аппаратный bias здесь — общий, грубый сдвиг,
    /// шейдерный bias в основном PS — по нормали, точнее компенсирует
    /// наклонные поверхности (аппаратный slope-scaled bias частично
    /// решает то же самое, но нормаль-based добавка в PS даёт больше
    /// контроля на пологих углах).
    /// ВРЕМЕННО (диагностика воспроизведённого DXGI_ERROR_DEVICE_HUNG,
    /// который стабильно ловится именно на ВТОРОМ реальном обращении к
    /// shadow-проходу — см. DRED-отчёты в истории отладки main_car):
    /// вызвать сразу после `init()`, чтобы полностью выключить shadow
    /// mapping (`render_frame()` пропускает весь shadow-блок, если
    /// `shadow_pipeline_state.is_none()`) и проверить, исчезает ли
    /// зависание — это либо локализует баг внутри shadow-кода, либо
    /// опровергнет и эту гипотезу. Удалить вызов (и, по желанию, этот
    /// метод) после диагностики.
    pub fn disable_shadows_for_diagnostics(&mut self) {
        println!("[DIAG] Shadow mapping принудительно отключён для диагностики зависания");
        self.shadow_pipeline_state = None;
    }

    pub(super) fn create_shadow_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.shadow_vs.as_ref().unwrap();
        let root_sig = self.shadow_root_signature.as_ref().unwrap();
        let device = crate::get_device()?;

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
                SemanticName: s!("NORMAL"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 16,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 28,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];
        let input_layout = D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        };

        let rasterizer = D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: FALSE,
            DepthBias: 5000,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 2.0,
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

        let mut pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::ManuallyDrop::new(Some(root_sig.clone())),
            VS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: vs.as_ptr(),
                BytecodeLength: vs.size(),
            },
            PS: D3D12_SHADER_BYTECODE::default(),
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
            NumRenderTargets: 0,
            RTVFormats: [DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN],
            DSVFormat: DXGI_FORMAT_D32_FLOAT,
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

        match result {
            Ok(pso) => {
                self.shadow_pipeline_state = Some(pso);
                println!("[ENGINE] ✓ Shadow pipeline state created (depth-only, DSVFormat=D32_FLOAT)");
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] ✗ Failed to create shadow PSO: {:?}", e);
                Err(e)
            }
        }
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): view-proj
    /// матрица directional-света ("солнца"), с точки зрения которой
    /// рисуется shadow map.
    ///
    /// Наивный подход — просто взять фиксированный огромный ортографический
    /// объём вокруг всей сцены — тратит разрешение shadow map (2048x2048)
    /// на площадь, часть которой камера может вообще не видеть, отчего
    /// тени рядом с камерой становятся грубыми/ступенчатыми. Вместо этого
    /// объём подгоняется под ВИДИМЫЙ frustum камеры на каждый кадр:
    /// 1. Берём 8 углов усечённой пирамиды камеры в мировых координатах
    ///    (near/far плоскости camera.near/camera.far).
    /// 2. Строим view-матрицу света: смотрим ИЗ направления, обратного
    ///    lightDir (свет "приходит" по lightDir, значит наблюдатель стоит
    ///    в направлении -lightDir), В центр frustum'а камеры.
    /// 3. Переводим все 8 углов в это light-space и берём их AABB — это
    ///    даёт МИНИМАЛЬНЫЙ ортографический объём, который гарантированно
    ///    покрывает весь видимый frustum, не тратя разрешение впустую.
    ///
    /// ВАЖНО про стабильность (требование проекта — БЕЗ видимых "попов"):
    /// при повороте/движении камеры этот объём меняет размер и позицию
    /// каждый кадр — из-за этого текселы shadow map "плавают" относительно
    /// мира (текселы не выровнены на пиксельной сетке между кадрами), что
    /// на глаз выглядит как мерцание/дрожание края тени ("shadow swimming"),
    /// даже когда камера и объекты неподвижны, а меняется только её угол
    /// обзора. Полное решение (округление центра объёма до шага текселя
    /// shadow map) — известное дальнейшее улучшение этой же Фазы 6,
    /// сознательно отложенное: минимальный рабочий вариант должен сначала
    /// давать корректные тени в принципе, стабилизация — следующий шаг той
    /// же фазы, а не блокер для первого прохода.
    /// ИСПРАВЛЕНО (стабилизация теней — устранение "shadow swimming",
    /// отложенное улучшение, изначально анонсированное как будущий шаг
    /// ещё в Фазе 6): раньше ортографический объём света ЗАНОВО строился
    /// каждый кадр как ПЛОТНЫЙ AABB 8 углов camera frustum'а в
    /// light-space. Из-за этого при повороте камеры (не только сдвиге!)
    /// сам РАЗМЕР этого AABB менял форму от кадра к кадру — диагональ
    /// frustum'а, спроецированная на плоскость света, "дышит" при
    /// повороте камеры, даже если сама камера физически стоит на месте.
    /// Изменение размера объёма означает изменение масштаба (texels per
    /// world unit) shadow map КАЖДЫЙ кадр — тень одного и того же
    /// неподвижного объекта из-за этого чуть смещается на доли текселя от
    /// кадра к кадру, что глаз считывает как заметное дрожание/"плавание"
    /// края тени, особенно при вращении камеры на месте.
    ///
    /// Стандартное решение (см. например GPU Gems 3, "Common Techniques
    /// to Improve Shadow Depth Maps" — там же описан этот же класс
    /// артефакта): (1) зафиксировать РАЗМЕР ортографического объёма как
    /// константу для текущего frustum'а (не зависящую от его ориентации —
    /// берём максимальный радиус СРЕДИ всех 8 углов от центра, тогда
    /// объём одного размера гарантированно вмещает frustum при ЛЮБОЙ его
    /// ориентации), и (2) "защёлкивать" (snap) центр этого объёма в
    /// light-space X/Y на шаги размером РОВНО в один тексель shadow map —
    /// тогда объём может двигаться только целыми текселями, а не плавно,
    /// и один и тот же мировой объект всегда попадает в один и тот же
    /// тексель (с точностью до целого текселя) независимо от того, куда
    /// именно между кадрами сдвинулась/повернулась камера.
    /// ОБНОВЛЕНО (каскадные тени / CSM — расширение Фазы 6): раньше
    /// (`compute_shadow_view_proj`, единственный каскад) NDC Z всегда был
    /// 0.0/1.0 — весь camera.near..camera.far. Теперь принимает СВОЙ
    /// диапазон дистанций для конкретного каскада (`near_dist`/`far_dist`,
    /// в метрах от камеры вдоль её взгляда) и переводит его в
    /// соответствующие NDC Z через ПРОЕКЦИОННУЮ (не полную view-proj)
    /// матрицу камеры — стандартный способ найти NDC Z для произвольной
    /// view-space глубины при перспективной проекции. Только 8 углов
    /// POD-frustum'а для ЭТОГО каскада строятся из ndc_z_near/ndc_z_far
    /// вместо фиксированных 0.0/1.0 — остальная логика (радиус, snap,
    /// ортопроекция) идентична однокаскадной версии и переиспользуется
    /// БЕЗ ИЗМЕНЕНИЙ для каждого каскада.
    pub(super) fn compute_cascade_view_proj(&self, light_dir: Vec3, near_dist: f32, far_dist: f32) -> Mat4 {
        let cam = &self.camera;
        let proj = cam.projection_matrix();
        let inv_view_proj = (proj * cam.view_matrix()).inverse();

        let ndc_z_for_view_dist = |dist: f32| -> f32 {
            let clip_z = proj.z_axis.z * dist + proj.w_axis.z;
            let clip_w = proj.z_axis.w * dist + proj.w_axis.w;
            if clip_w.abs() > 1e-6 { clip_z / clip_w } else { 0.0 }
        };
        let ndc_z_near = ndc_z_for_view_dist(near_dist);
        let ndc_z_far = ndc_z_for_view_dist(far_dist);

        let ndc_corners: [Vec3; 8] = [
            Vec3::new(-1.0, -1.0, ndc_z_near), Vec3::new(1.0, -1.0, ndc_z_near),
            Vec3::new(-1.0, 1.0, ndc_z_near), Vec3::new(1.0, 1.0, ndc_z_near),
            Vec3::new(-1.0, -1.0, ndc_z_far), Vec3::new(1.0, -1.0, ndc_z_far),
            Vec3::new(-1.0, 1.0, ndc_z_far), Vec3::new(1.0, 1.0, ndc_z_far),
        ];
        let mut world_corners = [Vec3::ZERO; 8];
        let mut center = Vec3::ZERO;
        for (i, ndc) in ndc_corners.iter().enumerate() {
            let clip = inv_view_proj * glam::Vec4::new(ndc.x, ndc.y, ndc.z, 1.0);
            let w = if clip.w.abs() > 1e-6 { clip.w } else { 1.0 };
            let world = Vec3::new(clip.x / w, clip.y / w, clip.z / w);
            world_corners[i] = world;
            center += world;
        }
        center /= 8.0;

        let mut radius = 0.0_f32;
        for corner in &world_corners {
            radius = radius.max((*corner - center).length());
        }
        radius = radius.max(1.0);

        let light_dir = if light_dir.length_squared() > 1e-6 { light_dir.normalize() } else { Vec3::new(0.0, -1.0, 0.0) };
        let light_eye = center - light_dir * cam.far.max(50.0);
        let up = if light_dir.x.abs() < 0.001 && light_dir.z.abs() < 0.001 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let light_view = crate::math::look_at(light_eye, center, up);

        let texel_size = (radius * 2.0) / SHADOW_MAP_RESOLUTION as f32;
        let center_light = light_view.transform_point3(center);
        let snapped_x = (center_light.x / texel_size).floor() * texel_size;
        let snapped_y = (center_light.y / texel_size).floor() * texel_size;

        let z_padding = radius * 0.5 + 10.0;

        crate::math::orthographic(
            snapped_x - radius, snapped_x + radius,
            snapped_y - radius, snapped_y + radius,
            center_light.z - radius - z_padding, center_light.z + radius + z_padding,
        ) * light_view
    }
}
