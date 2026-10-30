//! ДОБАВЛЕНО (замер экспозиции по кадру): средняя логарифмическая яркость
//! HDR-кадра до экспозиции, с весом к центру — то, что меряет экспонометр
//! камеры (см. shaders/metering_cs.hlsl). Результат читается на CPU через
//! readback-буфер своего слота кадра — с задержкой в несколько кадров, без
//! ожидания GPU. Игра по нему ведёт адаптацию глаза/камеры
//! (`AlkashEngine::scene_luminance`).

use std::mem::ManuallyDrop;
use windows::core::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SLOTS: usize = 3;

pub(super) struct Metering {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    inc: u32,
    result: ID3D12Resource,
    readback: Vec<ID3D12Resource>,
    written: [bool; SLOTS],
    hdr_ptr: usize,
    /// Последний прочитанный замер (средняя яркость, единицы HDR-кадра).
    pub(super) last: Option<f32>,
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

fn buffer(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource> {
    let props = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: 256,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: flags,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    unsafe { device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut r)? };
    r.ok_or_else(|| Error::from_hresult(HRESULT(0x80004005u32 as i32)))
}

const SRV_ALL: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0);

impl Metering {
    pub(super) fn new() -> Result<Metering> {
        let device = crate::get_device()?;
        let cs = crate::shader::ShaderBlob::compile(include_str!("shaders/metering_cs.hlsl"), "cs_5_0", "main")?;
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
                    eprintln!("[METER] root signature: {}", String::from_utf8_lossy(std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize())));
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
        let result = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)?;
        let mut readback = Vec::new();
        for _ in 0..SLOTS {
            readback.push(buffer(&device, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST)?);
        }
        let m = Metering { device, root, pso, heap, inc, result, readback, written: [false; SLOTS], hdr_ptr: 0, last: None };
        unsafe {
            m.device.CreateUnorderedAccessView(
                &m.result,
                None,
                Some(&D3D12_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Buffer: D3D12_BUFFER_UAV { FirstElement: 0, NumElements: 64, StructureByteStride: 0, CounterOffsetInBytes: 0, Flags: D3D12_BUFFER_UAV_FLAG_RAW } },
                }),
                m.cpu(1),
            );
        }
        println!("[METER] ✓ замер экспозиции по кадру");
        Ok(m)
    }

    fn cpu(&self, slot: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let base = unsafe { self.heap.GetCPUDescriptorHandleForHeapStart() };
        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (slot * self.inc) as usize }
    }

    /// Записать замер `hdr` (сейчас PIXEL_SHADER_RESOURCE, туда и вернётся)
    /// в слот `frame`; заодно прочитать то, что этот слот намерил раньше
    /// (его кадр уже дождался своего fence — данные готовы).
    pub(super) fn record(&mut self, list: &ID3D12GraphicsCommandList, frame: usize, hdr: &ID3D12Resource, w: u32, h: u32) {
        let slot = frame % SLOTS;
        if self.written[slot] {
            unsafe {
                let mut p = std::ptr::null_mut();
                if self.readback[slot].Map(0, Some(&D3D12_RANGE { Begin: 0, End: 4 }), Some(&mut p)).is_ok() {
                    let v = *(p as *const f32);
                    self.readback[slot].Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
                    if v.is_finite() && v > 0.0 {
                        self.last = Some(v);
                    }
                }
            }
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
        submit(list, vec![barrier(hdr, pixel, SRV_ALL)]);
        let c = [w as f32, h as f32, 0.0, 0.0];
        unsafe {
            list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            list.SetComputeRootSignature(&self.root);
            list.SetPipelineState(&self.pso);
            let base = self.heap.GetGPUDescriptorHandleForHeapStart();
            list.SetComputeRootDescriptorTable(0, base);
            list.SetComputeRoot32BitConstants(1, 4, c.as_ptr() as *const _, 0);
            list.Dispatch(1, 1, 1);
        }
        submit(list, vec![barrier(&self.result, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE), barrier(hdr, SRV_ALL, pixel)]);
        unsafe { list.CopyBufferRegion(&self.readback[slot], 0, &self.result, 0, 4) };
        submit(list, vec![barrier(&self.result, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)]);
        self.written[slot] = true;
    }
}
