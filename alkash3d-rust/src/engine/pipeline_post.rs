//! Пост-обработка HDR-кадра: composite/tonemap-проход (ACES filmic +
//! экспозиция + гамма, читает HDR + bloom + volumetric источники в одном
//! fullscreen-треугольнике) и bloom (extract по порогу яркости + separable
//! Gaussian blur на half-res ping-pong таргетах).
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
    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): шейдеры
    /// composite/tonemap-прохода. Вершинный шейдер рисует ОДИН
    /// fullscreen-треугольник без единого байта вершинных данных —
    /// координаты (uv, clip-space позиция) вычисляются прямо из
    /// SV_VertexID (0,1,2) арифметикой; такой треугольник целиком
    /// перекрывает экран, а видимая часть (за пределами [-1,1]) отсекается
    /// растеризатором как обычно — это стандартный, самый дешёвый способ
    /// нарисовать fullscreen-эффект в DX12, не требующий отдельного
    /// вершинного/индексного буфера ради двух треугольников квада.
    ///
    /// Пиксельный шейдер делает ровно две вещи: (1) экспозицию — умножает
    /// HDR-цвет на `exposure` (из `.alfar` GlobalLightSettings, если сцена
    /// загружена, иначе 1.0 по умолчанию) и (2) ACES filmic tonemap —
    /// стандартная, широко используемая аппроксимация (Narkowicz 2015),
    /// сжимающая произвольно яркий HDR-диапазон в [0,1] MUCH мягче, чем
    /// голое обрезание (clamp), сохраняя видимые детали в ярких участках
    /// (например прямо под фонарём) вместо однородного белого пятна.
    pub(super) fn compile_tonemap_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };
        VS_OUTPUT main(uint vertexId : SV_VertexID) {
            VS_OUTPUT output;
            // Классический fullscreen-triangle трюк: 3 вершины покрывают
            // весь [-1,1]x[-1,1] экран одним треугольником (с запасом за
            // пределами экрана, что нормально — растеризатор отсекает
            // невидимую часть). UV идёт от (0,0) в левом верхнем углу до
            // (2,2) в "запасной" вершине, но реально используемая часть —
            // [0,1]x[0,1], как у обычного квада.
            float2 uv = float2((vertexId << 1) & 2, vertexId & 2);
            output.uv = uv;
            output.pos = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
            return output;
        }
        "#;

        let ps_source = r#"
        Texture2D HDRSource : register(t0);
        // ДОБАВЛЕНО (bloom): результат extract+blur-прохода (half-res,
        // уже билинейно "размазанное" свечение ярких источников) —
        // складывается с основным HDR-цветом ДО тонмаппинга, что даёт
        // физически правдоподобный эффект "пересвета" вокруг фонарей
        // вместо плоских ярких пятен без ореола.
        Texture2D BloomSource : register(t1);
        // ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
        // подсветка): результат screen-space raymarch-прохода (half-res,
        // см. compile_volumetric_shaders) — аддитивно складывается с
        // остальным HDR-цветом ДО тонмаппинга, как и bloom, чтобы god rays
        // тоже проходили через один и тот же ACES-тонмаппинг, а не
        // накладывались поверх уже сжатого LDR-изображения (что выглядело
        // бы плоско и не сочеталось по яркости с остальной сценой).
        Texture2D VolumetricSource : register(t2);
        SamplerState PointSampler : register(s0);

        cbuffer TonemapConstants : register(b0) {
            float exposure;
            float bloomIntensity;
            float2 _padding;
        };

        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };

        // ACES filmic tonemap, аппроксимация Krzysztof Narkowicz (2015) —
        // стандартная в игровой индустрии формула, недорогая (никаких
        // циклов/textur-выборок сверх одной), даёт кинематографичную
        // компрессию яркости с мягким "плечом" у самых ярких значений
        // вместо жёсткого обрезания.
        float3 ACESFilm(float3 x) {
            float a = 2.51;
            float b = 0.03;
            float c = 2.43;
            float d = 0.59;
            float e = 0.14;
            return saturate((x * (a * x + b)) / (x * (c * x + d) + e));
        }

        float4 main(PS_INPUT input) : SV_TARGET {
            float3 hdrColor = HDRSource.Sample(PointSampler, input.uv).rgb;
            // BloomSource — half-res текстура, PointSampler здесь всё
            // равно даёт визуально мягкий результат, т.к. само свечение
            // уже размыто предыдущим Gaussian-blur проходом (см.
            // compile_bloom_shaders) — отдельный билинейный сэмплер под
            // апскейл bloom не заводим, чтобы не плодить лишний статический
            // сэмплер только ради этого.
            float3 bloomColor = BloomSource.Sample(PointSampler, input.uv).rgb;
            // ДОБАВЛЕНО (Фаза 8): volumetric-свет — тоже half-res, тот же
            // point-сэмплер + upscale "как есть" (raymarch сам по себе уже
            // достаточно гладкий по построению, см. jitter в
            // compile_volumetric_shaders — дополнительный билинейный
            // сэмплер здесь не добавляет заметного качества).
            float3 volumetricColor = VolumetricSource.Sample(PointSampler, input.uv).rgb;
            float3 combined = hdrColor + bloomColor * bloomIntensity + volumetricColor;
            float3 exposed = combined * exposure;
            float3 tonemapped = ACESFilm(exposed);
            // Гамма-коррекция: back buffer — R8G8B8A8_UNORM без sRGB-вьюхи
            // (см. Renderer::back_buffers/create_pipeline_state — формат
            // тот же, что был и до Фазы 5), поэтому применяем гамму 1/2.2
            // здесь явно, а не полагаемся на автоматическую sRGB-конверсию
            // GPU, которой при этом формате RTV попросту нет.
            float3 gammaCorrected = pow(max(tonemapped, 0.0), 1.0 / 2.2);
            return float4(gammaCorrected, 1.0);
        }
        "#;

        self.tonemap_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.tonemap_ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Tonemap shaders compiled (ACES + экспозиция)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): отдельная root
    /// signature для composite/tonemap-прохода — ОТЛИЧАЕТСЯ от основной
    /// (`create_root_signature`) тем, что вместо root-descriptor SRV (как
    /// у GPULight/сетки, register(t0..t2) в основном пиксельном шейдере)
    /// здесь нужна дескрипторная ТАБЛИЦА (register(t0) HDRSource) плюс
    /// сэмплер (register(s0) PointSampler). Root-descriptor SRV годится
    /// только для StructuredBuffer — Texture2D, читаемая через
    /// SamplerState, обязана идти через descriptor table (это требование
    /// D3D12: `Texture2D.Sample()` не работает с "голым" root SRV без
    /// связанного сэмплера). Сэмплер объявлен как STATIC (часть самой root
    /// signature) — предпочтительно перед per-frame sampler heap'ом, когда
    /// нужен один и тот же неизменный point-sampler, это не тратит слот в
    /// каком-либо динамическом хипе и не требует отдельного SAMPLER heap
    /// вообще.
    pub(super) fn create_tonemap_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 3,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let root_params = [D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &srv_range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        }];

        let static_sampler = D3D12_STATIC_SAMPLER_DESC {
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

        let root_params = [
            root_params[0],
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
            NumStaticSamplers: 1,
            pStaticSamplers: &static_sampler,
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
                    eprintln!("Tonemap root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.tonemap_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Tonemap root signature created (SRV table t0 + static sampler s0 + CBV b0)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): PSO для
    /// composite/tonemap-прохода. Отличия от основного PSO
    /// (`create_pipeline_state`): (1) нет input layout — вершины
    /// генерируются в VS через SV_VertexID, вершинного буфера физически
    /// нет; (2) нет depth test/write — это чисто 2D fullscreen-проход
    /// поверх уже готового изображения, глубина ему не нужна и не имеет
    /// смысла; (3) целевой формат RTV — формат back buffer'а
    /// (R8G8B8A8_UNORM), а не HDR-формат, так как это ФИНАЛЬНАЯ запись
    /// после тонмаппинга.
    pub(super) fn create_tonemap_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.tonemap_vs.as_ref().unwrap();
        let ps = self.tonemap_ps.as_ref().unwrap();
        let root_sig = self.tonemap_root_signature.as_ref().unwrap();
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
            RTVFormats: [DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN],
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
        self.tonemap_pipeline_state = Some(pso);
        println!("[ENGINE] ✓ Tonemap pipeline state created");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, bloom): компилирует
    /// пиксельные шейдеры bloom-прохода. Переиспользует ТОТ ЖЕ
    /// fullscreen-triangle вершинный шейдер, что и tonemap
    /// (`self.tonemap_vs` — уже скомпилирован к моменту вызова этой
    /// функции, см. порядок вызовов в `init()`), так как геометрия
    /// абсолютно одинаковая (весь экран одним треугольником) — компилировать
    /// идентичный VS ещё раз не имеет смысла.
    pub(super) fn compile_bloom_shaders(&mut self) -> Result<()> {
        let extract_source = r#"
        Texture2D HDRSource : register(t0);
        SamplerState PointSampler : register(s0);

        cbuffer BloomParams : register(b0) {
            float threshold;
            float2 texel_size; // 1/width, 1/height ИСТОЧНИКА (для blur-прохода; extract его не использует)
            float _unused;
        };

        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };

        float4 main(PS_INPUT input) : SV_TARGET {
            float3 color = HDRSource.Sample(PointSampler, input.uv).rgb;
            float brightness = max(color.r, max(color.g, color.b));
            // smoothstep(threshold, threshold*2, brightness) — мягкий, а не
            // жёсткий порог: пиксели чуть ниже threshold не пропадают резко
            // в 0, а плавно затухают, что убирает "рваный" край вокруг
            // светящихся объектов после последующего блюра.
            float contribution = smoothstep(threshold, threshold * 2.0, brightness);
            return float4(color * contribution, 1.0);
        }
        "#;

        let blur_source = r#"
        Texture2D BloomSource : register(t0);
        SamplerState PointSampler : register(s0);

        cbuffer BloomParams : register(b0) {
            float threshold; // не используется в blur-проходе
            float2 texel_size;
            float _unused;
        };

        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };

        float4 main(PS_INPUT input) : SV_TARGET {
            // Веса 9-тапового биномиального гаусса (сумма = 1.0), центр —
            // самый большой вес, симметрично убывает к краям.
            float weights[5] = { 0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216 };
            float3 result = BloomSource.Sample(PointSampler, input.uv).rgb * weights[0];
            for (int i = 1; i < 5; i++) {
                float2 offset = texel_size * float(i);
                result += BloomSource.Sample(PointSampler, input.uv + offset).rgb * weights[i];
                result += BloomSource.Sample(PointSampler, input.uv - offset).rgb * weights[i];
            }
            return float4(result, 1.0);
        }
        "#;

        self.bloom_extract_ps = Some(ShaderBlob::compile(extract_source, "ps_5_0", "main")?);
        self.bloom_blur_ps = Some(ShaderBlob::compile(blur_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Bloom shaders compiled (extract + separable Gaussian blur)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, bloom): общая root
    /// signature для extract/blur-проходов — по форме идентична
    /// tonemap-root-signature (SRV-таблица t0 + статический point-сэмплер
    /// s0 + CBV b0), поэтому переиспользовать `tonemap_root_signature`
    /// было бы возможно, НО осознанно заведена отдельная — размер и
    /// содержимое CBV b0 у bloom (`threshold`+`texel_size`) отличаются от
    /// tonemap (`exposure`), и смешивание двух разных смыслов "b0" под
    /// одной root signature было бы источником трудноуловимых ошибок при
    /// будущих правках (например если один из двух проходов расширят
    /// дополнительными параметрами).
    pub(super) fn create_bloom_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let static_sampler = D3D12_STATIC_SAMPLER_DESC {
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
            NumStaticSamplers: 1,
            pStaticSamplers: &static_sampler,
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
                    eprintln!("Bloom root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.bloom_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Bloom root signature created (SRV table t0 + static sampler s0 + CBV b0)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, bloom): создаёт ОБА
    /// PSO bloom-прохода (extract и blur) — общая форма (нет input layout,
    /// нет depth, RTV формат = HDR-формат таргетов A/B, т.к. bloom
    /// накапливается в float, а не в LDR) полностью идентична
    /// `create_tonemap_pipeline_state`, отличается только PS и RTV-формат.
    pub(super) fn create_bloom_pipeline_states(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.tonemap_vs.as_ref().unwrap();
        let root_sig = self.bloom_root_signature.as_ref().unwrap();
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

        for (target_ps, target_field_is_extract) in [(self.bloom_extract_ps.as_ref().unwrap(), true), (self.bloom_blur_ps.as_ref().unwrap(), false)] {
            let mut pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::ManuallyDrop::new(Some(root_sig.clone())),
                VS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: vs.as_ptr(),
                    BytecodeLength: vs.size(),
                },
                PS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: target_ps.as_ptr(),
                    BytecodeLength: target_ps.size(),
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

            if target_field_is_extract {
                self.bloom_extract_pipeline_state = Some(pso);
            } else {
                self.bloom_blur_pipeline_state = Some(pso);
            }
        }

        println!("[ENGINE] ✓ Bloom pipeline states created (extract + blur)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, bloom): создаёт
    /// half-res (половина ширины/высоты основного разрешения, минимум
    /// 1x1 — на случай экстремально маленького окна) ping-pong таргеты A/B
    /// + их RTV/SRV дескрипторы + буфер параметров bloom-прохода.
    /// ВРЕМЕННО (бисекция DXGI_ERROR_DEVICE_HUNG — см. пару
    /// `disable_shadows_for_diagnostics` в pipeline_shadow.rs, тот же
    /// приём): `render_frame()` пропускает весь bloom-проход, если
    /// `bloom_texture_a.is_none()` (проверяется через `if let (Some(bloom_a),
    /// Some(bloom_b), Some(bloom_srv_heap)) = ...`, обнуления одного
    /// `bloom_texture_a` достаточно). Вызвать сразу после `init()`.
    pub fn disable_bloom_for_diagnostics(&mut self) {
        println!("[DIAG] Bloom-проход принудительно отключён для диагностики зависания");
        self.bloom_texture_a = None;
    }

    pub(super) fn create_bloom_resources(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let bloom_width = (self.width / 2).max(1);
        let bloom_height = (self.height / 2).max(1);

        // ИЗМЕНЕНО (максимальная графика — MSAA, см. `create_hdr_target` в
        // render.rs): bloom-таргеты — ВСЕГДА одноимпловые обычные
        // render target'ы (RENDER_TARGET initial state, как и раньше) —
        // ЭТОТ вызов не имеет отношения к MSAA основного цветового
        // прохода, просто переиспользует тот же формат-хелпер (R16G16B16A16_FLOAT).
        let texture_a = crate::render::RenderTexture::create_hdr_target(bloom_width, bloom_height, 1, windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_RENDER_TARGET)?;
        let texture_b = crate::render::RenderTexture::create_hdr_target(bloom_width, bloom_height, 1, windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_RENDER_TARGET)?;

        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(2)?;
        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(2)?;

        let rtv_size = {
            let state = STATE.lock().unwrap();
            state.rtv_descriptor_size
        };
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        let rtv_a = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, 0, rtv_size);
        let rtv_b = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, 1, rtv_size);
        texture_a.create_rtv(rtv_a)?;
        texture_b.create_rtv(rtv_b)?;

        let srv_a_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        let srv_a_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        let srv_b_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 1, cbv_srv_uav_size);
        let srv_b_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 1, cbv_srv_uav_size);
        texture_a.create_srv(srv_a_cpu)?;
        texture_b.create_srv(srv_b_cpu)?;

        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_bloom_resources() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        let bloom_final_srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 1, cbv_srv_uav_size);
        texture_a.create_srv(bloom_final_srv_cpu)?;

        self.bloom_texture_a = Some(texture_a);
        self.bloom_rtv_a = rtv_a;
        self.bloom_srv_a_gpu = srv_a_gpu;
        self.bloom_texture_b = Some(texture_b);
        self.bloom_rtv_b = rtv_b;
        self.bloom_srv_b_gpu = srv_b_gpu;
        self.bloom_rtv_heap = Some(rtv_heap);
        self.bloom_srv_heap = Some(srv_heap);

        let params_cb = Buffer::create_constant_buffer(256)?;
        let default_params: [f32; 4] = [1.0, 1.0 / bloom_width as f32, 0.0, 0.0];
        let bytes = unsafe {
            std::slice::from_raw_parts(default_params.as_ptr() as *const u8, 16)
        };
        params_cb.update_constant_buffer(bytes)?;
        self.bloom_params_buffer = Some(params_cb);

        println!(
            "[ENGINE] ✓ Bloom resources created: {}x{} half-res ping-pong targets",
            bloom_width, bloom_height
        );
        Ok(())
    }
}
