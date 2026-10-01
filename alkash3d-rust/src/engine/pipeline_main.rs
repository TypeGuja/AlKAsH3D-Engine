//! Основной 3D draw pass: HLSL-шейдеры (вершинный трансформ + пиксельный —
//! сетка каллинга фонарей, cascaded shadow maps, normal mapping, Cook-Torrance
//! PBR-специуляр), корневая сигнатура и pipeline state объекта прохода.
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись. В оригинальном файле `create_root_signature`/
//! `create_pipeline_state` физически находились ПОСЛЕ всех
//! tonemap/bloom/shadow/occluder/volumetric методов (см. остальные
//! `engine/pipeline_*.rs`) — здесь они объединены с `compile_default_shaders`
//! логически, т.к. все три относятся к одному и тому же основному проходу.

use windows::core::*;
use windows::Win32::Foundation::*;
use crate::STATE;
use crate::shader::ShaderBlob;
use crate::pso::PipelineState;
use super::{AlkashEngine, Vertex, NUM_CASCADES, SPOT_SHADOW_ATLAS_SLOT, POINT_SHADOW_ATLAS_SLOT};

impl AlkashEngine {
    pub(super) fn compile_default_shaders(&mut self) -> Result<()> {
        let vs_source = include_str!("shaders/main_vs.hlsl");

        let ps_source = include_str!("shaders/main_ps.hlsl");

        self.vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Default shaders compiled (нормали + сетка каллинга + spot-конус фонарей)");
        Ok(())
    }

    pub(super) fn create_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        // ИЗМЕНЕНО (тени фонарей): таблица теней теперь из ДВУХ диапазонов
        // — каскады солнца t3..t5 (дескрипторы 0..3) и spot-атлас t9
        // (дескриптор SPOT_SHADOW_ATLAS_SLOT). t6..t8 уже заняты
        // материалами, поэтому атлас не может идти подряд за каскадами.
        let shadow_srv_ranges = [
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: NUM_CASCADES as u32,
                BaseShaderRegister: 3,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: 0,
            },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 1,
                BaseShaderRegister: 9,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: SPOT_SHADOW_ATLAS_SLOT as u32,
            },
            // ДОБАВЛЕНО (тени point-фонарей): атлас cube-граней, t10.
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: 1,
                BaseShaderRegister: 10,
                RegisterSpace: 0,
                OffsetInDescriptorsFromTableStart: POINT_SHADOW_ATLAS_SLOT as u32,
            },
        ];

        let material_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 6,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let normal_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 7,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };
        let mr_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 8,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
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
            ShaderRegister: 0,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        // ИЗМЕНЕНО (максимальная графика — анизотропная фильтрация):
        // раньше был обычный трилинейный фильтр — текстуры земли/дороги,
        // видимые под острым углом (главный случай, где это вообще
        // заметно — плоскости пола/двора, уходящие к горизонту), мылились
        // сильнее, чем нужно. 16x — максимум, который гарантированно
        // поддерживает любое D3D12-совместимое железо (FEATURE_LEVEL_11_0+
        // требует минимум 16x у ANISOTROPIC), дороже линейной фильтрации
        // не по числу текселей на пиксель, а по числу samples ПРИ
        // сэмплировании под углом — на прямой взгляд сверху вниз (как у
        // MinLOD/MaxLOD выше) практически бесплатно.
        let material_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_ANISOTROPIC,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 16,
            ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
            BorderColor: D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 1,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        let static_samplers = [shadow_sampler, material_sampler];

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
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 1,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 2,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: shadow_srv_ranges.len() as u32,
                        pDescriptorRanges: shadow_srv_ranges.as_ptr(),
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &material_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &normal_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &mr_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 1,
                        RegisterSpace: 0,
                        Num32BitValues: 4,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            // ДОБАВЛЕНО (тени фонарей): root-параметр 9 — cbuffer
            // SpotShadowConstants (b2), матрицы тенеобразующих фонарей.
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 2,
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
                    eprintln!("Root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Root signature created (CBV b0 + SRV t0 фонари + SRV t1/t2 сетка каллинга + SRV table t3..t5 shadow map + t9 spot-атлас + t10 point-атлас + SRV table t6 albedo + CBV b2 spot-матрицы + comparison sampler s0 + linear sampler s1)");
        Ok(())
    }

    pub(super) fn create_pipeline_state(&mut self) -> Result<()> {
        let vs = self.vs.as_ref().unwrap();
        let ps = self.ps.as_ref().unwrap();
        let root_sig = self.root_signature.as_ref().unwrap();

        let pso = PipelineState::create_graphics(
            vs, ps, root_sig,
            Vertex::STRIDE,
            // HDR-цвет + ambient (см. Renderer::ambient_target) — оба RGBA16F.
            &[
                windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
                windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
            ],
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
            self.msaa_samples,
        )?;

        self.pipeline_state = Some(pso);
        println!("[ENGINE] ✓ Pipeline state created (RTV format = R16G16B16A16_FLOAT, matches HDR target, {}x MSAA)", self.msaa_samples);
        Ok(())
    }
}
