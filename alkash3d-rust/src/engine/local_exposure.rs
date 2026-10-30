//! ДОБАВЛЕНО (локальная экспозиция): сетка средней логарифмической яркости
//! HDR-кадра (см. shaders/local_exposure_cs.hlsl). Tonemap читает её из
//! слота `render::LOCAL_EXPOSURE_SRV_SLOT` и выравнивает яркость по кадру
//! (`Lens::local_exposure`). Сетка фиксированного размера — от размера окна
//! не зависит; после пересоздания кучи рендерера (ресайз) SRV в ней
//! записывается заново сам (`record` сравнивает кучу).

use std::mem::ManuallyDrop;
use windows::core::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

pub(super) const GRID_W: u32 = 64;
pub(super) const GRID_H: u32 = 36;

pub(super) struct LocalExposure {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    inc: u32,
    grid: ID3D12Resource,
    hdr_ptr: usize,
    /// Куча рендерера, в которую уже записан SRV сетки.
    bound_heap: usize,
}

fn barrier(r: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(r.clone())),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

fn submit(list: &ID3D12GraphicsCommandList, barriers: Vec<D3D12_RESOURCE_BARRIER>) {
    unsafe { list.ResourceBarrier(&barriers) };
    for b in barriers {
        let mut t = ManuallyDrop::into_inner(unsafe { b.Anonymous.Transition });
        unsafe { ManuallyDrop::drop(&mut t.pResource) };
    }
}

const SRV_ALL: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0);

impl LocalExposure {
    pub(super) fn new() -> Result<LocalExposure> {
        let device = crate::get_device()?;
        let cs = crate::shader::ShaderBlob::compile(include_str!("shaders/local_exposure_cs.hlsl"), "cs_5_0", "main")?;
        let srv = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 1, BaseShaderRegister: 0, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let uav = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 1, BaseShaderRegister: 0, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 1 };
        let ranges = [srv, uav];
        let params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 2, pDescriptorRanges: ranges.as_ptr() } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 4 } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let desc = D3D12_ROOT_SIGNATURE_DESC { NumParameters: params.len() as u32, pParameters: params.as_ptr(), NumStaticSamplers: 0, pStaticSamplers: std::ptr::null(), Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE };
        let root: ID3D12RootSignature = unsafe {
            let mut blob: Option<ID3DBlob> = None;
            let mut error: Option<ID3DBlob> = None;
            if let Err(e) = D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)) {
                if let Some(err) = error {
                    eprintln!("[LOCALEXP] root signature: {}", String::from_utf8_lossy(std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize())));
                }
                return Err(e);
            }
            let blob = blob.unwrap();
            device.CreateRootSignature(0, std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()))?
        };
        let mut pdesc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root.clone())),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: cs.as_ptr(), BytecodeLength: cs.size() },
            ..Default::default()
        };
        let pso: Result<ID3D12PipelineState> = unsafe { device.CreateComputePipelineState(&pdesc) };
        unsafe { ManuallyDrop::drop(&mut pdesc.pRootSignature) };
        let pso = pso?;
        let heap: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 2, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, NodeMask: 0 })?
        };
        let inc = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        let grid: ID3D12Resource = unsafe {
            let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: GRID_W as u64,
                Height: GRID_H,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_R32_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                ..Default::default()
            };
            let mut r: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, SRV_ALL, None, &mut r)?;
            r.ok_or_else(|| Error::from_hresult(HRESULT(0x80004005u32 as i32)))?
        };
        let le = LocalExposure { device, root, pso, heap, inc, grid, hdr_ptr: 0, bound_heap: 0 };
        unsafe {
            le.device.CreateUnorderedAccessView(
                &le.grid,
                None,
                Some(&D3D12_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_FLOAT,
                    ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV { MipSlice: 0, PlaneSlice: 0 } },
                }),
                le.cpu(1),
            );
        }
        println!("[LOCALEXP] ✓ локальная экспозиция ({}x{})", GRID_W, GRID_H);
        Ok(le)
    }

    fn cpu(&self, slot: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let base = unsafe { self.heap.GetCPUDescriptorHandleForHeapStart() };
        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (slot * self.inc) as usize }
    }

    /// Посчитать сетку по `hdr` (сейчас PIXEL_SHADER_RESOURCE, туда и
    /// вернётся) и записать её SRV в слот tonemap кучи `target_heap`.
    pub(super) fn record(&mut self, list: &ID3D12GraphicsCommandList, hdr: &ID3D12Resource, w: u32, h: u32, target_heap: &ID3D12DescriptorHeap) {
        let heap_ptr = target_heap.as_raw() as usize;
        if heap_ptr != self.bound_heap {
            let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_FLOAT,
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
            };
            let base = unsafe { target_heap.GetCPUDescriptorHandleForHeapStart() };
            let dst = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (crate::render::LOCAL_EXPOSURE_SRV_SLOT * self.inc) as usize };
            unsafe { self.device.CreateShaderResourceView(&self.grid, Some(&desc), dst) };
            self.bound_heap = heap_ptr;
        }
        let ptr = hdr.as_raw() as usize;
        if ptr != self.hdr_ptr {
            let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
            };
            unsafe { self.device.CreateShaderResourceView(hdr, Some(&desc), self.cpu(0)) };
            self.hdr_ptr = ptr;
        }
        let pixel = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        submit(list, vec![barrier(hdr, pixel, SRV_ALL), barrier(&self.grid, SRV_ALL, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)]);
        let c = [w as f32, h as f32, GRID_W as f32, GRID_H as f32];
        unsafe {
            list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            list.SetComputeRootSignature(&self.root);
            list.SetPipelineState(&self.pso);
            list.SetComputeRootDescriptorTable(0, self.heap.GetGPUDescriptorHandleForHeapStart());
            list.SetComputeRoot32BitConstants(1, 4, c.as_ptr() as *const _, 0);
            list.Dispatch(GRID_W.div_ceil(8), GRID_H.div_ceil(8), 1);
        }
        submit(list, vec![barrier(&self.grid, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, SRV_ALL), barrier(hdr, SRV_ALL, pixel)]);
    }
}
