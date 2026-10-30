//! ДОБАВЛЕНО (временное сглаживание, TAA): см. shaders/taa_cs.hlsl.
//!
//! Проход compute сразу после resolve основного HDR и SSAO/volumetric (они
//! читают глубину того же кадра): результат пишется в историю и копируется
//! в `output` — `hdr_resolved` или, при досборке разрешения (TAAU), в
//! `Renderer::hdr_upscaled` размера экрана; bloom и tonemap дальше
//! работают как раньше, только с уже сглаженным кадром. Камера в основном
//! проходе сдвигается на `jitter()` (доли пикселя) — без этого сглаживать
//! нечего.
//!
//! Свой дескрипторный хип (как у DDGI): 0 — цвет, 1 — глубина, 2 —
//! рассеянный свет, 3 — SSAO, 4/5 — история 0/1 (SRV), 6/7 — история 0/1 (UAV).

use std::mem::ManuallyDrop;
use windows::core::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::math::{Mat4, Vec2};

const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;
const JITTER_PHASES: u32 = 8;
/// При досборке пиксель экрана покрывают сэмплы нескольких пикселей кадра —
/// нужно больше разных точек.
const JITTER_PHASES_UPSCALE: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy)]
struct Constants {
    inv_view_proj: [f32; 16],
    prev_view_proj: [f32; 16],
    render_size: [f32; 2],
    out_size: [f32; 2],
    jitter: [f32; 2],
    history_valid: f32,
    feedback: f32,
    apply_ao: f32,
    _pad: [f32; 3],
}

pub(super) struct Taa {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    inc: u32,
    history: [ID3D12Resource; 2],
    /// Пишется в этом кадре; другая — прошлый кадр.
    cur: usize,
    /// Размер кадра (рисуется) и экрана (история, выход).
    width: u32,
    height: u32,
    out_width: u32,
    out_height: u32,
    msaa: bool,
    /// Ресурсы, на которые уже записаны SRV (пересоздаются при ресайзе).
    color_ptr: usize,
    depth_ptr: usize,
    ambient_ptr: usize,
    ssao_ptr: usize,
    prev_view_proj: Mat4,
    valid: bool,
    frame: u32,
}

fn halton(mut i: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0f32, 0.0f32);
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
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
    if barriers.is_empty() {
        return;
    }
    unsafe { list.ResourceBarrier(&barriers) };
    for b in barriers {
        let mut t = ManuallyDrop::into_inner(unsafe { b.Anonymous.Transition });
        unsafe { ManuallyDrop::drop(&mut t.pResource) };
    }
}

const SRV_ALL: D3D12_RESOURCE_STATES = D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0);

impl Taa {
    pub(super) fn new(width: u32, height: u32, out_width: u32, out_height: u32, msaa: bool) -> Result<Taa> {
        let device = crate::get_device()?;
        let define = if msaa { "#define MSAA 1\n#line 1\n" } else { "" };
        let cs = crate::shader::ShaderBlob::compile(&format!("{}{}", define, include_str!("shaders/taa_cs.hlsl")), "cs_5_0", "main")?;
        let root = Self::root_signature(&device)?;
        let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root.clone())),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: cs.as_ptr(), BytecodeLength: cs.size() },
            ..Default::default()
        };
        let pso: Result<ID3D12PipelineState> = unsafe { device.CreateComputePipelineState(&desc) };
        unsafe { ManuallyDrop::drop(&mut desc.pRootSignature) };
        let pso = pso?;
        let heap: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: 8,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })?
        };
        let inc = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        let history = [Self::texture(&device, out_width, out_height)?, Self::texture(&device, out_width, out_height)?];
        let taa = Taa {
            device, root, pso, heap, inc, history, cur: 0, width, height, out_width, out_height, msaa,
            color_ptr: 0, depth_ptr: 0, ambient_ptr: 0, ssao_ptr: 0, prev_view_proj: Mat4::IDENTITY, valid: false, frame: 0,
        };
        for k in 0..2 {
            taa.srv(&taa.history[k], 4 + k as u32);
            unsafe {
                taa.device.CreateUnorderedAccessView(
                    &taa.history[k],
                    None,
                    Some(&D3D12_UNORDERED_ACCESS_VIEW_DESC {
                        Format: FORMAT,
                        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV { MipSlice: 0, PlaneSlice: 0 } },
                    }),
                    taa.cpu(6 + k as u32),
                );
            }
        }
        if (width, height) == (out_width, out_height) {
            println!("[TAA] ✓ временное сглаживание {}x{} (MSAA {})", width, height, msaa);
        } else {
            println!("[TAA] ✓ временное сглаживание с досборкой {}x{} -> {}x{} (MSAA {})", width, height, out_width, out_height, msaa);
        }
        Ok(taa)
    }

    /// Обе истории живут в UAV, пока в них пишут, и в SRV_ALL, пока их читают.
    fn texture(device: &ID3D12Device, w: u32, h: u32) -> Result<ID3D12Resource> {
        let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: w as u64,
            Height: h,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let mut r: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, SRV_ALL, None, &mut r)? };
        r.ok_or_else(|| Error::from_hresult(HRESULT(0x80004005u32 as i32)))
    }

    fn root_signature(device: &ID3D12Device) -> Result<ID3D12RootSignature> {
        // t0 цвет, t1 глубина, t2 рассеянный свет, t3 SSAO; t4 история; u0 выход
        let cur = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 4, BaseShaderRegister: 0, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let hist = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 1, BaseShaderRegister: 4, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let out = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 1, BaseShaderRegister: 0, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let table = |r: &D3D12_DESCRIPTOR_RANGE| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: r } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let params = [
            table(&cur),
            table(&hist),
            table(&out),
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: (std::mem::size_of::<Constants>() / 4) as u32 } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            ..Default::default()
        };
        let desc = D3D12_ROOT_SIGNATURE_DESC { NumParameters: params.len() as u32, pParameters: params.as_ptr(), NumStaticSamplers: 1, pStaticSamplers: &sampler, Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE };
        unsafe {
            let mut blob: Option<ID3DBlob> = None;
            let mut error: Option<ID3DBlob> = None;
            if let Err(e) = D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)) {
                if let Some(err) = error {
                    eprintln!("[TAA] root signature: {}", String::from_utf8_lossy(std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize())));
                }
                return Err(e);
            }
            let blob = blob.unwrap();
            device.CreateRootSignature(0, std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()))
        }
    }

    fn cpu(&self, slot: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let base = unsafe { self.heap.GetCPUDescriptorHandleForHeapStart() };
        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (slot * self.inc) as usize }
    }

    fn gpu(&self, slot: u32) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        let base = unsafe { self.heap.GetGPUDescriptorHandleForHeapStart() };
        D3D12_GPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (slot * self.inc) as u64 }
    }

    fn srv(&self, r: &ID3D12Resource, slot: u32) {
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: FORMAT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
        };
        unsafe { self.device.CreateShaderResourceView(r, Some(&desc), self.cpu(slot)) };
    }

    pub(super) fn matches(&self, width: u32, height: u32, out_width: u32, out_height: u32, msaa: bool) -> bool {
        (self.width, self.height, self.out_width, self.out_height, self.msaa) == (width, height, out_width, out_height, msaa)
    }

    /// Сдвиг камеры этого кадра в пикселях кадра (-0.5..0.5).
    pub(super) fn jitter(&self) -> Vec2 {
        let phases = if (self.width, self.height) == (self.out_width, self.out_height) { JITTER_PHASES } else { JITTER_PHASES_UPSCALE };
        let i = self.frame % phases + 1;
        Vec2::new(halton(i, 2) - 0.5, halton(i, 3) - 0.5)
    }

    /// Матрица проекции со сдвигом `jitter()` (доли пикселя в NDC).
    pub(super) fn jittered(&self, proj: Mat4) -> Mat4 {
        self.jitter_ndc(1.0) * proj
    }

    /// Сдвиг этого кадра в NDC (`sign` = -1 — снять его).
    fn jitter_ndc(&self, sign: f32) -> Mat4 {
        let j = self.jitter() * sign;
        Mat4::from_translation(crate::math::Vec3::new(j.x * 2.0 / self.width as f32, -j.y * 2.0 / self.height as f32, 0.0))
    }

    /// Забыть историю (телепорт камеры, смена сцены).
    pub(super) fn reset(&mut self) {
        self.valid = false;
    }

    /// Записать проход. `color` — hdr_resolved (сейчас PIXEL_SHADER_RESOURCE),
    /// `depth` с его SRV-описанием и текущим состоянием `depth_state`
    /// (возвращается в него же). `view_proj` — та же (со сдвигом), что
    /// у основного прохода этого кадра. `ao` — рассеянный свет и SSAO (оба
    /// PIXEL_SHADER_RESOURCE): затенённое вычитается здесь, в разрешении
    /// кадра, а не в tonemap. `output` — куда результат (PIXEL_SHADER_RESOURCE):
    /// сам `color` или, при досборке, текстура размера экрана.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &mut self,
        list: &ID3D12GraphicsCommandList,
        color: &ID3D12Resource,
        depth: &crate::render::RenderTexture,
        depth_state: D3D12_RESOURCE_STATES,
        view_proj: Mat4,
        ao: Option<(&ID3D12Resource, &ID3D12Resource)>,
        output: &ID3D12Resource,
    ) {
        let color_ptr = color.as_raw() as usize;
        if color_ptr != self.color_ptr {
            self.srv(color, 0);
            self.color_ptr = color_ptr;
        }
        let depth_ptr = depth.resource.as_raw() as usize;
        if depth_ptr != self.depth_ptr {
            let _ = depth.create_depth_srv(self.cpu(1));
            self.depth_ptr = depth_ptr;
        }
        // без SSAO на их месте — любое валидное описание (цвет), шейдер не читает
        let (ambient, ssao) = ao.unwrap_or((color, color));
        let ambient_ptr = ambient.as_raw() as usize;
        if ambient_ptr != self.ambient_ptr {
            self.srv(ambient, 2);
            self.ambient_ptr = ambient_ptr;
        }
        let ssao_ptr = ssao.as_raw() as usize;
        if ssao_ptr != self.ssao_ptr {
            self.srv(ssao, 3);
            self.ssao_ptr = ssao_ptr;
        }
        let (cur, prev) = (self.cur, 1 - self.cur);
        let pixel = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        let mut to = vec![
            barrier(color, pixel, SRV_ALL),
            barrier(&depth.resource, depth_state, SRV_ALL),
            barrier(&self.history[cur], SRV_ALL, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
        ];
        if let Some((a, s)) = ao {
            to.push(barrier(a, pixel, SRV_ALL));
            to.push(barrier(s, pixel, SRV_ALL));
        }
        submit(list, to);
        let j = self.jitter();
        // ИСПРАВЛЕНО (дрожь кадра): выходной пиксель — в пространстве без
        // сдвига (как и история), поэтому и мир из глубины — обратной
        // матрицей без сдвига; со сдвигом история читалась смещённой на
        // текущий сдвиг, и неподвижный кадр дрожал. Сам цвет кадра остаётся
        // сдвинутым — так TAA и собирает разные точки внутри пикселя.
        let unjittered = self.jitter_ndc(-1.0) * view_proj;
        let c = Constants {
            inv_view_proj: unjittered.inverse().to_cols_array(),
            prev_view_proj: self.prev_view_proj.to_cols_array(),
            render_size: [self.width as f32, self.height as f32],
            out_size: [self.out_width as f32, self.out_height as f32],
            jitter: [j.x, j.y],
            history_valid: if self.valid { 1.0 } else { 0.0 },
            feedback: 0.9,
            apply_ao: if ao.is_some() { 1.0 } else { 0.0 },
            _pad: [0.0; 3],
        };
        unsafe {
            list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            list.SetComputeRootSignature(&self.root);
            list.SetPipelineState(&self.pso);
            list.SetComputeRootDescriptorTable(0, self.gpu(0));
            list.SetComputeRootDescriptorTable(1, self.gpu(4 + prev as u32));
            list.SetComputeRootDescriptorTable(2, self.gpu(6 + cur as u32));
            list.SetComputeRoot32BitConstants(3, (std::mem::size_of::<Constants>() / 4) as u32, &c as *const _ as *const _, 0);
            list.Dispatch(self.out_width.div_ceil(8), self.out_height.div_ceil(8), 1);
        }
        // результат — в output для bloom/tonemap
        let same = output.as_raw() == color.as_raw();
        let mut back = vec![
            barrier(&self.history[cur], D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE),
            barrier(&depth.resource, SRV_ALL, depth_state),
        ];
        if same {
            back.push(barrier(color, SRV_ALL, D3D12_RESOURCE_STATE_COPY_DEST));
        } else {
            back.push(barrier(color, SRV_ALL, pixel));
            back.push(barrier(output, pixel, D3D12_RESOURCE_STATE_COPY_DEST));
        }
        if let Some((a, s)) = ao {
            back.push(barrier(a, SRV_ALL, pixel));
            back.push(barrier(s, SRV_ALL, pixel));
        }
        submit(list, back);
        unsafe { list.CopyResource(output, &self.history[cur]) };
        submit(list, vec![
            barrier(&self.history[cur], D3D12_RESOURCE_STATE_COPY_SOURCE, SRV_ALL),
            barrier(output, D3D12_RESOURCE_STATE_COPY_DEST, pixel),
        ]);
        // ИСПРАВЛЕНО (дрожь кадра): история накоплена без сдвига — и
        // репроецировать в неё надо матрицей БЕЗ сдвига прошлого кадра. Со
        // сдвигом история каждый кадр читалась смещённой на разницу сдвигов
        // (до ~0.85 пикселя): неподвижная картинка дрожала вслед за ними.
        self.prev_view_proj = unjittered;
        self.valid = true;
        self.cur = prev;
        self.frame = self.frame.wrapping_add(1);
    }
}
