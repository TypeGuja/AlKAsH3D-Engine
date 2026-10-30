//! ДОБАВЛЕНО (кэш теней фонарей без полного копирования): переносит глубину
//! копии «только статика» (`AlkashEngine::local_shadow_static`) в плитку
//! атласа теней. Раньше перед дорисовкой движущегося (`Mesh::moving`) атлас
//! восстанавливался копированием целиком — D3D12 копирует глубину только
//! всем ресурсом, а это 64 МБ на атлас 4096², ~0.7 мс каждый кадр, пока тело
//! игрока в кадре. Здесь — треугольник на весь экран во вьюпорте плитки:
//! PS читает копию в том же пикселе атласа и пишет её в SV_Depth (тест
//! глубины ALWAYS) — переносится только нужное.
//!
//! Свой хип: 0 — копия spot-атласа, 1 — копия point-атласа (SRV R32_FLOAT).

use std::mem::ManuallyDrop;
use windows::core::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const PS: &str = r#"
Texture2D<float> StaticDepth : register(t0);
float main(float4 pos : SV_Position) : SV_Depth {
    return StaticDepth.Load(int3(pos.xy, 0));
}
"#;

pub(super) struct ShadowRestore {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    inc: u32,
    /// Ресурсы, на которые записаны SRV слотов 0/1.
    srcs: [usize; 2],
}

impl ShadowRestore {
    pub(super) fn new() -> Result<ShadowRestore> {
        let device = crate::get_device()?;
        let vs = crate::shader::ShaderBlob::compile(include_str!("shaders/fullscreen_vs.hlsl"), "vs_5_0", "main")?;
        let ps = crate::shader::ShaderBlob::compile(PS, "ps_5_0", "main")?;
        let root = Self::root_signature(&device)?;
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root.clone())),
            VS: D3D12_SHADER_BYTECODE { pShaderBytecode: vs.as_ptr(), BytecodeLength: vs.size() },
            PS: D3D12_SHADER_BYTECODE { pShaderBytecode: ps.as_ptr(), BytecodeLength: ps.size() },
            BlendState: D3D12_BLEND_DESC {
                RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC { RenderTargetWriteMask: 0, ..Default::default() }; 8],
                ..Default::default()
            },
            SampleMask: u32::MAX,
            RasterizerState: D3D12_RASTERIZER_DESC { FillMode: D3D12_FILL_MODE_SOLID, CullMode: D3D12_CULL_MODE_NONE, DepthClipEnable: false.into(), ..Default::default() },
            DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
                ..Default::default()
            },
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 0,
            DSVFormat: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let pso: Result<ID3D12PipelineState> = unsafe { device.CreateGraphicsPipelineState(&desc) };
        unsafe { ManuallyDrop::drop(&mut desc.pRootSignature) };
        let pso = pso?;
        let heap: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 2,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })?
        };
        let inc = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        Ok(ShadowRestore { root, pso, heap, inc, srcs: [0; 2] })
    }

    fn root_signature(device: &ID3D12Device) -> Result<ID3D12RootSignature> {
        let range = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 1, BaseShaderRegister: 0, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let params = [D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: &range } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        }];
        let desc = D3D12_ROOT_SIGNATURE_DESC { NumParameters: 1, pParameters: params.as_ptr(), NumStaticSamplers: 0, pStaticSamplers: std::ptr::null(), Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE };
        unsafe {
            let mut blob: Option<ID3DBlob> = None;
            let mut error: Option<ID3DBlob> = None;
            if let Err(e) = D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)) {
                if let Some(err) = error {
                    eprintln!("[ENGINE] shadow restore root signature: {}", String::from_utf8_lossy(std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize())));
                }
                return Err(e);
            }
            let blob = blob.unwrap();
            device.CreateRootSignature(0, std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()))
        }
    }

    /// Перенести глубину копии `src` (PIXEL_SHADER_RESOURCE) в плитки `tiles`
    /// атласа, чья DSV сейчас стоит целью (OMSetRenderTargets — снаружи).
    /// `slot` — 0 spot / 1 point. Меняет PSO, корневую сигнатуру, хип,
    /// вьюпорт и scissor — вызывающий выставляет свои заново.
    pub(super) fn record(&mut self, list: &ID3D12GraphicsCommandList, slot: usize, src: &ID3D12Resource, tiles: &[RECT]) {
        if tiles.is_empty() {
            return;
        }
        let ptr = src.as_raw() as usize;
        let base_cpu = unsafe { self.heap.GetCPUDescriptorHandleForHeapStart() };
        if self.srcs[slot] != ptr {
            let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_FLOAT,
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
            };
            let cpu = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base_cpu.ptr + slot * self.inc as usize };
            if let Ok(device) = crate::get_device() {
                unsafe { device.CreateShaderResourceView(src, Some(&desc), cpu) };
            }
            self.srcs[slot] = ptr;
        }
        let base_gpu = unsafe { self.heap.GetGPUDescriptorHandleForHeapStart() };
        unsafe {
            list.SetPipelineState(&self.pso);
            list.SetGraphicsRootSignature(&self.root);
            list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            list.SetGraphicsRootDescriptorTable(0, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: base_gpu.ptr + (slot as u32 * self.inc) as u64 });
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            for r in tiles {
                list.RSSetViewports(&[D3D12_VIEWPORT {
                    TopLeftX: r.left as f32,
                    TopLeftY: r.top as f32,
                    Width: (r.right - r.left) as f32,
                    Height: (r.bottom - r.top) as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                }]);
                list.RSSetScissorRects(&[*r]);
                list.DrawInstanced(3, 1, 0, 0);
            }
        }
    }
}
