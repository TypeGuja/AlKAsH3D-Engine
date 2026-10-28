//! Пост-обработка HDR-кадра: composite/tonemap-проход (ACES filmic +
//! экспозиция + гамма, читает HDR + bloom + volumetric источники в одном
//! fullscreen-треугольнике) и bloom (extract по порогу яркости с понижением
//! до half-res + цепочка уровней вниз/вверх, Jimenez 2014; separable
//! Gaussian blur остался только для SSAO).
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

/// ДОБАВЛЕНО (bloom цепочкой уровней): сколько уровней (half-res, 1/4,
/// 1/8 ...) и до какого размера (меньшая сторона, пиксели) их понижать.
/// Каждый уровень — вдвое шире ореол; 6 уровней от half-res дают радиус
/// порядка 1/10 экрана, сумма — плавный спад яркости от источника.
pub(super) const BLOOM_LEVELS: usize = 6;
const BLOOM_MIN_SIZE: u32 = 8;

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
        let vs_source = include_str!("shaders/fullscreen_vs.hlsl");

        let ps_source = include_str!("shaders/tonemap_ps.hlsl");

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

        // ИЗМЕНЕНО (максимальная графика — SSAO): 3 -> 4 (t0 HDR, t1 Bloom,
        // t2 Volumetric, t3 SSAO — см. `renderer.srv_uav_heap`, у него уже
        // 4 слота, `create_ssao_final_srv` в pipeline_ssao.rs пишет в
        // слот 3).
        // ИЗМЕНЕНО (честный SSAO): +t4 — ambient-вклад основного прохода
        // (renderer::AMBIENT_SRV_SLOT).
        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: crate::render::AMBIENT_SRV_SLOT + 1,
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

        // ИЗМЕНЕНО (bloom цепочкой уровней): билинейный — half-res bloom и
        // volumetric растягиваются на экран гладко, а не квадратами 2x2.
        // Полноразмерные источники (HDR, SSAO, ambient) читаются ровно в
        // центрах своих пикселей, для них билинейный = point.
        let static_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
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
        let extract_source = include_str!("shaders/bloom_extract_ps.hlsl");

        let blur_source = include_str!("shaders/bloom_blur_ps.hlsl");

        self.bloom_extract_ps = Some(ShaderBlob::compile(extract_source, "ps_5_0", "main")?);
        self.bloom_blur_ps = Some(ShaderBlob::compile(blur_source, "ps_5_0", "main")?);
        self.bloom_down_ps = Some(ShaderBlob::compile(include_str!("shaders/bloom_down_ps.hlsl"), "ps_5_0", "main")?);
        self.bloom_up_ps = Some(ShaderBlob::compile(include_str!("shaders/bloom_up_ps.hlsl"), "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Bloom shaders compiled (extract + down/up chain + separable blur for SSAO)");
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
        // Блюр SSAO: point-сэмплер, параметры в своих CBV (см. ssao_blur_*).
        self.bloom_root_signature = Some(Self::bloom_root_signature_with(1, false)?);
        // ИЗМЕНЕНО (bloom цепочкой уровней): у проходов bloom параметры —
        // root constants (снимок на каждый draw; общий CBV, переписанный
        // несколькими проходами до исполнения списка, отдавал GPU только
        // последнее значение) и билинейный сэмплер.
        // Extract читает ещё метку unlit из ambient-таргета (t4): таблица —
        // с hdr_srv_gpu, как у tonemap, до AMBIENT_SRV_SLOT включительно.
        self.bloom_extract_root_signature = Some(Self::bloom_root_signature_with(crate::render::AMBIENT_SRV_SLOT + 1, true)?);
        self.bloom_chain_root_signature = Some(Self::bloom_root_signature_with(1, true)?);
        println!("[ENGINE] ✓ Bloom root signatures created (SRV table + static sampler s0 + b0)");
        Ok(())
    }

    /// `root_constants`: b0 — 4 root constants и билинейный сэмплер (проходы
    /// bloom); иначе b0 — CBV и point-сэмплер (блюр SSAO).
    fn bloom_root_signature_with(num_srv: u32, root_constants: bool) -> Result<windows::Win32::Graphics::Direct3D12::ID3D12RootSignature> {
        use windows::Win32::Graphics::Direct3D12::*;

        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: num_srv,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let static_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: if root_constants { D3D12_FILTER_MIN_MAG_MIP_LINEAR } else { D3D12_FILTER_MIN_MAG_MIP_POINT },
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

        let params_b0 = if root_constants {
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: 4,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            }
        } else {
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            }
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
            params_b0,
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

            Ok(device.CreateRootSignature(0, blob_data)?)
        }
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

        // Повышение уровня прибавляется к содержимому таргета (ONE/ONE).
        let mut additive = blend_desc;
        additive.RenderTarget[0].BlendEnable = TRUE;
        additive.RenderTarget[0].DestBlend = D3D12_BLEND_ONE;
        additive.RenderTarget[0].DestBlendAlpha = D3D12_BLEND_ONE;
        let extract_sig = self.bloom_extract_root_signature.as_ref().unwrap();
        let blur_sig = self.bloom_root_signature.as_ref().unwrap();
        let chain_sig = self.bloom_chain_root_signature.as_ref().unwrap();
        let passes = [
            (self.bloom_extract_ps.as_ref().unwrap(), extract_sig, blend_desc),
            (self.bloom_blur_ps.as_ref().unwrap(), blur_sig, blend_desc),
            (self.bloom_down_ps.as_ref().unwrap(), chain_sig, blend_desc),
            (self.bloom_up_ps.as_ref().unwrap(), chain_sig, additive),
        ];
        let mut psos = Vec::with_capacity(passes.len());
        for (target_ps, sig, blend) in passes {
            let mut pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::ManuallyDrop::new(Some(sig.clone())),
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
                BlendState: blend,
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
            psos.push(result?);
        }
        let mut psos = psos.into_iter();
        self.bloom_extract_pipeline_state = psos.next();
        self.bloom_blur_pipeline_state = psos.next();
        self.bloom_down_pipeline_state = psos.next();
        self.bloom_up_pipeline_state = psos.next();

        println!("[ENGINE] ✓ Bloom pipeline states created (extract + blur + down + up)");
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

        // ИЗМЕНЕНО (bloom цепочкой уровней): уровень 0 — half-res (его
        // читает tonemap), дальше каждый вдвое меньше, пока не станет мельче
        // `BLOOM_MIN_SIZE` или не наберётся `BLOOM_LEVELS`.
        let mut sizes = vec![((self.width / 2).max(1), (self.height / 2).max(1))];
        while sizes.len() < BLOOM_LEVELS {
            let (w, h) = *sizes.last().unwrap();
            if w / 2 < BLOOM_MIN_SIZE || h / 2 < BLOOM_MIN_SIZE {
                break;
            }
            sizes.push((w / 2, h / 2));
        }
        let (bloom_width, bloom_height) = sizes[0];

        // ИЗМЕНЕНО (максимальная графика — MSAA, см. `create_hdr_target` в
        // render.rs): bloom-таргеты — ВСЕГДА одноимпловые обычные
        // render target'ы (RENDER_TARGET initial state, как и раньше) —
        // ЭТОТ вызов не имеет отношения к MSAA основного цветового
        // прохода, просто переиспользует тот же формат-хелпер (R16G16B16A16_FLOAT).
        let textures = sizes
            .iter()
            .map(|&(w, h)| crate::render::RenderTexture::create_hdr_target(w, h, 1, D3D12_RESOURCE_STATE_RENDER_TARGET))
            .collect::<Result<Vec<_>>>()?;

        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(textures.len() as u32)?;
        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(textures.len() as u32)?;

        let rtv_size = {
            let state = STATE.lock().unwrap();
            state.rtv_descriptor_size
        };
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        let mut levels = Vec::with_capacity(textures.len());
        for (i, texture) in textures.into_iter().enumerate() {
            let rtv = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, i as u32, rtv_size);
            texture.create_rtv(rtv)?;
            texture.create_srv(crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, i as u32, cbv_srv_uav_size))?;
            let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, i as u32, cbv_srv_uav_size);
            levels.push(super::BloomMip { texture, rtv, srv_gpu, is_srv: false });
        }

        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_bloom_resources() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        let bloom_final_srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 1, cbv_srv_uav_size);
        levels[0].texture.create_srv(bloom_final_srv_cpu)?;

        let mut levels = levels.into_iter();
        let top = levels.next().unwrap();
        self.bloom_texture_a = Some(top.texture);
        self.bloom_rtv_a = top.rtv;
        self.bloom_srv_a_gpu = top.srv_gpu;
        self.bloom_a_is_srv = false;
        self.bloom_mips = levels.collect();
        self.bloom_rtv_heap = Some(rtv_heap);
        self.bloom_srv_heap = Some(srv_heap);

        println!(
            "[ENGINE] ✓ Bloom resources created: {}x{} half-res + {} smaller levels",
            bloom_width, bloom_height, self.bloom_mips.len()
        );
        Ok(())
    }
}
