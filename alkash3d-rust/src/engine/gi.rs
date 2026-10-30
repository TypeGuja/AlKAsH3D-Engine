//! ДОБАВЛЕНО (непрямой свет лучами, DDGI): сетка проб освещённости,
//! обновляемая трассировкой лучей (DXR 1.1, inline RayQuery из compute) —
//! см. shaders/ddgi.hlsl и shaders/ddgi_common.hlsl.
//!
//! Каждый кадр, до основного прохода:
//!  1. BLAS — один раз на меш (при первом появлении меша в кадре); меши,
//!     которые CPU переписывает (скиннинг), остаются в позе первой сборки.
//!  2. TLAS — заново из инстансов кадра (непрозрачные, не unlit).
//!  3. Лучи из проб -> освещённость и расстояния проб (+ рамки плиток).
//! Основной проход берёт освещённость проб вместо равномерного ambient
//! (диффуз); отражения окружения пока остаются от ambient.
//!
//! Без DXR 1.1 или без DXC движок работает как раньше (`Gi::new` -> Err).

use std::mem::ManuallyDrop;
use windows::core::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::math::{Mat4, Quat, Vec3};
use crate::plugin::GPULight;
use super::Mesh;

/// Исходник лучевых проходов: общая часть + ядра.
pub(super) fn ddgi_source() -> String {
    format!("{}\n{}", include_str!("shaders/ddgi_common.hlsl"), include_str!("shaders/ddgi.hlsl"))
}

/// Исходник основного пиксельного шейдера: общая часть DDGI (выборка проб) + main_ps.
pub(super) fn main_ps_source() -> String {
    format!("{}\n{}", include_str!("shaders/ddgi_common.hlsl"), include_str!("shaders/main_ps.hlsl"))
}

/// Точки входа ddgi.hlsl (порядок = индексы `Gi::psos`).
pub(super) const DDGI_KERNELS: [&str; 8] = ["TraceRays", "UpdateIrradiance", "UpdateDistance", "BorderIrradiance", "BorderDistance", "ClearAtlases", "TraceReflections", "ResolveReflections"];
const K_TRACE: usize = 0;
const K_IRR: usize = 1;
const K_DIST: usize = 2;
const K_BORDER_IRR: usize = 3;
const K_BORDER_DIST: usize = 4;
const K_CLEAR: usize = 5;
const K_REFL: usize = 6;
const K_REFL_RESOLVE: usize = 7;
/// ДОБАВЛЕНО (отражения лучами): дескрипторы прохода отражений в конце
/// хипа — t6 вес/шероховатость, t7 нормаль/расстояние, t8 цвет,
/// t9 лучи половинного разрешения (SRV), u3 выход, u4 лучи (UAV).
const REFL_SLOT: u32 = HEAP_FIXED + 2 * MAX_RT_MESHES;

/// Как cbuffer ReflConstants (b1) в ddgi.hlsl.
#[repr(C)]
#[derive(Clone, Copy)]
struct ReflConstants {
    inv_view_proj: [f32; 16],
    cam_pos: [f32; 3],
    frame: u32,
    size: [f32; 2],
    max_roughness: f32,
    _pad: f32,
}

/// Внутренняя часть плитки пробы (как DDGI_IRR_N / DDGI_DIST_N в HLSL).
const IRR_N: u32 = 8;
const DIST_N: u32 = 16;
/// Кадров в полёте (как `frame_fence_values` движка).
const FRAMES: usize = 2;
/// Bindless-таблица буферов: на меш два дескриптора (вершины, индексы).
/// Первые HEAP_FIXED — атласы и данные лучей.
pub(super) const MAX_RT_MESHES: u32 = 16384;
const HEAP_FIXED: u32 = 6;
/// Пробы адресуются группами по X (лимит D3D12 — 65535 групп).
const MAX_PROBES: u32 = 65535;

/// Объём сетки проб: где, с каким шагом, сколько лучей на пробу.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GiVolume {
    pub min: Vec3,
    pub max: Vec3,
    /// Шаг сетки в единицах мира.
    pub spacing: f32,
    /// Лучей на пробу за кадр (DDGI: 128–256).
    pub rays_per_probe: u32,
    /// Доля прошлого значения при смешивании (0.9–0.98): больше —
    /// спокойнее, но медленнее отклик на изменения света.
    pub hysteresis: f32,
    /// Сколько проб обновлять за кадр (по кругу): цена кадра против
    /// задержки отклика (все пробы обходятся за ceil(всего / это) кадров).
    pub probes_per_frame: u32,
}

impl GiVolume {
    pub fn new(min: Vec3, max: Vec3, spacing: f32) -> Self {
        GiVolume { min, max, spacing, rays_per_probe: 128, hysteresis: 0.95, probes_per_frame: 1024 }
    }

    fn counts(&self) -> [u32; 3] {
        let n = |a: f32, b: f32| (((b - a) / self.spacing.max(1e-3)).ceil() as u32 + 1).max(2);
        [n(self.min.x, self.max.x), n(self.min.y, self.max.y), n(self.min.z, self.max.z)]
    }
}

/// Как DDGIVolume в ddgi_common.hlsl (64 байта, 16 root constants).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct VolumeGpu {
    pub origin: [f32; 3],
    pub spacing: f32,
    pub counts: [u32; 3],
    pub enabled: u32,
    pub irr_texel: [f32; 2],
    pub dist_texel: [f32; 2],
    pub normal_bias: f32,
    pub view_bias: f32,
    pub _pad: [f32; 2],
}

/// Как cbuffer GIConstants в ddgi.hlsl.
#[repr(C)]
#[derive(Clone, Copy)]
struct Constants {
    volume: VolumeGpu,
    ray_rotation: [f32; 16],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky: [f32; 4],
    rays: u32,
    light_count: u32,
    hysteresis: f32,
    max_distance: f32,
    probe_offset: u32,
    probe_budget: u32,
    _pad: [u32; 2],
}

/// Как RtInstance в ddgi.hlsl.
#[repr(C)]
#[derive(Clone, Copy)]
struct RtInstance {
    albedo: [f32; 3],
    vertex_buffer: u32,
    emissive: [f32; 3],
    index_buffer: u32,
    metallic: f32,
    _pad: [u32; 3],
}

/// Инстанс кадра для лучей.
pub(super) struct RtDraw {
    pub mesh: usize,
    pub world: Mat4,
    pub tint: [f32; 4],
    /// Средний линейный цвет меша (`AlkashEngine::mesh_albedo`).
    pub albedo: [f32; 3],
}

/// Освещение кадра (как TransformConstants: light_dir/light_color/ambient_color).
pub(super) struct RtLighting<'a> {
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub sky: [f32; 4],
    pub lights: &'a [GPULight],
}

/// Буфер, растущий по требованию.
#[derive(Default)]
struct Grow {
    resource: Option<ID3D12Resource>,
    size: u64,
}

#[derive(Default)]
struct Frame {
    instances: Grow,
    materials: Grow,
    constants: Grow,
    lights: Grow,
    tlas: Grow,
    tlas_scratch: Grow,
    /// Scratch-буферы BLAS этого кадра — живут до повторного использования слота.
    garbage: Vec<ID3D12Resource>,
}

struct Probes {
    volume: GiVolume,
    counts: [u32; 3],
    ray_data: ID3D12Resource,
    irr: ID3D12Resource,
    dist: ID3D12Resource,
    irr_size: [u32; 2],
    dist_size: [u32; 2],
    cleared: bool,
    /// Атласы сейчас в ALL_SHADER_RESOURCE (иначе UNORDERED_ACCESS).
    atlases_srv: bool,
}

pub(crate) struct Gi {
    device: ID3D12Device5,
    root: ID3D12RootSignature,
    psos: Vec<ID3D12PipelineState>,
    heap: ID3D12DescriptorHeap,
    inc: u32,
    blas: Vec<Option<ID3D12Resource>>,
    frames: [Frame; FRAMES],
    probes: Option<Probes>,
    frame_counter: u64,
    /// Первая проба следующей порции обновления.
    next_probe: u32,
    /// Отладка: показывать только рассеянный свет с этим усилением (0 — выкл).
    pub(super) debug_gain: f32,
    /// ДОБАВЛЕНО (отражения лучами): выход прохода (UAV) и его размер.
    refl_out: Option<(ID3D12Resource, u32, u32)>,
    /// Лучи отражений, по одному на блок 2x2 выхода (тот же размер, что у refl_out).
    refl_half: Option<ID3D12Resource>,
    /// Ресурсы, на которые записаны SRV t6..t8 (пересоздаются при ресайзе).
    refl_srcs: [usize; 3],
}

fn buffer(device: &ID3D12Device5, size: u64, heap: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource> {
    let props = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size.max(256),
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

fn texture(device: &ID3D12Device5, w: u32, h: u32, format: DXGI_FORMAT) -> Result<ID3D12Resource> {
    let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: w as u64,
        Height: h,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    unsafe { device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, None, &mut r)? };
    r.ok_or_else(|| Error::from_hresult(HRESULT(0x80004005u32 as i32)))
}

/// Upload-буфер слота кадра не меньше `size` (старый принадлежит только
/// этому слоту, а слот кадра уже дождался своего fence — пересоздавать можно).
fn ensure(device: &ID3D12Device5, g: &mut Grow, size: u64, heap: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource> {
    if g.resource.is_none() || g.size < size {
        let new_size = size.max(256).next_power_of_two();
        g.resource = Some(buffer(device, new_size, heap, flags, state)?);
        g.size = new_size;
    }
    Ok(g.resource.clone().unwrap())
}

fn write<T: Copy>(r: &ID3D12Resource, data: &[T]) -> Result<()> {
    unsafe {
        let mut p = std::ptr::null_mut();
        r.Map(0, None, Some(&mut p))?;
        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, p as *mut u8, std::mem::size_of_val(data));
        r.Unmap(0, None);
    }
    Ok(())
}

fn uav_barrier(list: &ID3D12GraphicsCommandList4, r: Option<&ID3D12Resource>) {
    let b = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 { UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER { pResource: ManuallyDrop::new(r.cloned()) }) },
    };
    unsafe {
        list.ResourceBarrier(std::slice::from_ref(&b));
        let mut b = b;
        ManuallyDrop::drop(&mut (*b.Anonymous.UAV).pResource);
    }
}

fn transitions(list: &ID3D12GraphicsCommandList4, items: &[(&ID3D12Resource, D3D12_RESOURCE_STATES, D3D12_RESOURCE_STATES)]) {
    let barriers: Vec<D3D12_RESOURCE_BARRIER> = items
        .iter()
        .map(|(r, before, after)| D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: ManuallyDrop::new(Some((*r).clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: *before,
                    StateAfter: *after,
                }),
            },
        })
        .collect();
    unsafe {
        list.ResourceBarrier(&barriers);
        for mut b in barriers {
            ManuallyDrop::drop(&mut (*b.Anonymous.Transition).pResource);
        }
    }
}

/// Сырой (ByteAddressBuffer) SRV буфера.
fn raw_srv(device: &ID3D12Device5, r: &ID3D12Resource, size: u64, handle: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV { FirstElement: 0, NumElements: (size / 4) as u32, StructureByteStride: 0, Flags: D3D12_BUFFER_SRV_FLAG_RAW },
        },
    };
    unsafe { device.CreateShaderResourceView(r, Some(&desc), handle) };
}

pub(super) fn texture_srv(device: &ID3D12Device, r: Option<&ID3D12Resource>, format: DXGI_FORMAT, handle: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
    };
    unsafe { device.CreateShaderResourceView(r, Some(&desc), handle) };
}

fn texture_uav(device: &ID3D12Device5, r: &ID3D12Resource, format: DXGI_FORMAT, handle: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV::default() },
    };
    unsafe { device.CreateUnorderedAccessView(r, None, Some(&desc), handle) };
}

const RAY_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R32G32B32A32_FLOAT;
pub(super) const IRR_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;
pub(super) const DIST_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;

impl Gi {
    /// Есть ли DXR 1.1 и DXC; Err — почему нет.
    pub(super) fn new() -> std::result::Result<Gi, String> {
        let device: ID3D12Device5 = crate::get_device().and_then(|d| d.cast()).map_err(|e| format!("ID3D12Device5: {}", e))?;
        unsafe {
            let mut o5 = D3D12_FEATURE_DATA_D3D12_OPTIONS5::default();
            device
                .CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS5, &mut o5 as *mut _ as *mut _, std::mem::size_of_val(&o5) as u32)
                .map_err(|e| format!("OPTIONS5: {}", e))?;
            if o5.RaytracingTier.0 < D3D12_RAYTRACING_TIER_1_1.0 {
                return Err(format!("нет DXR 1.1 (tier {:?})", o5.RaytracingTier));
            }
        }
        let source = ddgi_source();
        let mut blobs = Vec::new();
        for entry in DDGI_KERNELS {
            blobs.push(crate::dxc::compile(&source, "cs_6_5", entry).map_err(|e| format!("DXC {}: {}", entry, e))?);
        }
        let root = Self::root_signature(&device).map_err(|e| format!("root signature: {}", e))?;
        let mut psos = Vec::new();
        for blob in &blobs {
            let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: ManuallyDrop::new(Some(root.clone())),
                CS: D3D12_SHADER_BYTECODE { pShaderBytecode: blob.data.as_ptr() as *const _, BytecodeLength: blob.data.len() },
                ..Default::default()
            };
            let pso: Result<ID3D12PipelineState> = unsafe { device.CreateComputePipelineState(&desc) };
            unsafe { ManuallyDrop::drop(&mut desc.pRootSignature) };
            psos.push(pso.map_err(|e| format!("compute PSO: {}", e))?);
        }
        let heap: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: HEAP_FIXED + 2 * MAX_RT_MESHES + 6,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                NodeMask: 0,
            })
        }
        .map_err(|e| format!("descriptor heap: {}", e))?;
        let inc = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        println!("[GI] ✓ DDGI готов (DXR 1.1, {} ядер)", psos.len());
        Ok(Gi { device, root, psos, heap, inc, blas: Vec::new(), frames: Default::default(), probes: None, frame_counter: 0, next_probe: 0, debug_gain: 0.0, refl_out: None, refl_half: None, refl_srcs: [0; 3] })
    }

    fn root_signature(device: &ID3D12Device5) -> Result<ID3D12RootSignature> {
        let srv_atlases = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 3, BaseShaderRegister: 3, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let uavs = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 3, BaseShaderRegister: 0, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 };
        let buffers = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 2 * MAX_RT_MESHES, BaseShaderRegister: 0, RegisterSpace: 1, OffsetInDescriptorsFromTableStart: 0 };
        let refl = [
            D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 4, BaseShaderRegister: 6, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 0 },
            D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 2, BaseShaderRegister: 3, RegisterSpace: 0, OffsetInDescriptorsFromTableStart: 4 },
        ];
        let root_desc = |t: D3D12_ROOT_PARAMETER_TYPE, reg: u32| D3D12_ROOT_PARAMETER {
            ParameterType: t,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: reg, RegisterSpace: 0 } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let table = |r: &D3D12_DESCRIPTOR_RANGE| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: r } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        // 0 — b0 константы, 1..3 — t0 TLAS, t1 инстансы, t2 фонари,
        // 4 — t3..t5 атласы (SRV), 5 — u0..u2, 6 — bindless буферы (space1).
        let params = [
            root_desc(D3D12_ROOT_PARAMETER_TYPE_CBV, 0),
            root_desc(D3D12_ROOT_PARAMETER_TYPE_SRV, 0),
            root_desc(D3D12_ROOT_PARAMETER_TYPE_SRV, 1),
            root_desc(D3D12_ROOT_PARAMETER_TYPE_SRV, 2),
            table(&srv_atlases),
            table(&uavs),
            table(&buffers),
            // 7 — отражения: t6..t9 + u3..u4; 8 — их константы b1
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 2, pDescriptorRanges: refl.as_ptr() } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 1, RegisterSpace: 0, Num32BitValues: (std::mem::size_of::<ReflConstants>() / 4) as u32 } },
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
                    eprintln!("[GI] root signature: {}", String::from_utf8_lossy(std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize())));
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

    /// Задать объём (None — выключить). Атласы пересоздаются, если поменялась сетка.
    /// Вызывать между кадрами при простое GPU (атласы читает кадр в полёте).
    pub(super) fn set_volume(&mut self, volume: Option<GiVolume>) -> Result<()> {
        let Some(v) = volume else {
            self.probes = None;
            return Ok(());
        };
        let counts = v.counts();
        if let Some(p) = self.probes.as_mut() {
            if p.counts == counts {
                p.volume = v;
                return Ok(());
            }
        }
        let probes = counts[0] * counts[1] * counts[2];
        if probes > MAX_PROBES {
            eprintln!("[GI] слишком много проб ({}), максимум {} — увеличь шаг сетки", probes, MAX_PROBES);
            return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
        }
        let tiles = [counts[0] * counts[1], counts[2]];
        let irr_size = [tiles[0] * (IRR_N + 2), tiles[1] * (IRR_N + 2)];
        let dist_size = [tiles[0] * (DIST_N + 2), tiles[1] * (DIST_N + 2)];
        let rays = v.rays_per_probe.clamp(32, 512);
        let ray_data = texture(&self.device, rays, probes, RAY_FORMAT)?;
        let irr = texture(&self.device, irr_size[0], irr_size[1], IRR_FORMAT)?;
        let dist = texture(&self.device, dist_size[0], dist_size[1], DIST_FORMAT)?;
        // Таблица: 0..2 — UAV лучей/освещённости/расстояний, 3..5 — SRV освещённости/расстояний/лучей.
        texture_uav(&self.device, &ray_data, RAY_FORMAT, self.cpu(0));
        texture_uav(&self.device, &irr, IRR_FORMAT, self.cpu(1));
        texture_uav(&self.device, &dist, DIST_FORMAT, self.cpu(2));
        let device: ID3D12Device = self.device.cast()?;
        texture_srv(&device, Some(&irr), IRR_FORMAT, self.cpu(3));
        texture_srv(&device, Some(&dist), DIST_FORMAT, self.cpu(4));
        texture_srv(&device, Some(&ray_data), RAY_FORMAT, self.cpu(5));
        println!(
            "[GI] сетка {}x{}x{} = {} проб, шаг {}, {} лучей на пробу, атласы {}x{} и {}x{}",
            counts[0], counts[1], counts[2], probes, v.spacing, rays, irr_size[0], irr_size[1], dist_size[0], dist_size[1]
        );
        self.probes = Some(Probes { volume: GiVolume { rays_per_probe: rays, ..v }, counts, ray_data, irr, dist, irr_size, dist_size, cleared: false, atlases_srv: false });
        Ok(())
    }

    /// Атласы для основного прохода (None — объёма нет).
    pub(super) fn atlases(&self) -> Option<(&ID3D12Resource, &ID3D12Resource)> {
        self.probes.as_ref().map(|p| (&p.irr, &p.dist))
    }

    /// Параметры объёма для основного прохода (enabled = 0, если проб нет
    /// или атласы ещё не готовы к чтению).
    pub(super) fn volume_gpu(&self) -> VolumeGpu {
        match &self.probes {
            Some(p) if p.atlases_srv => VolumeGpu { _pad: [(self.debug_gain > 0.0) as u32 as f32, self.debug_gain], ..Self::volume_of(p) },
            _ => VolumeGpu::default(),
        }
    }

    fn volume_of(p: &Probes) -> VolumeGpu {
        VolumeGpu {
            origin: p.volume.min.to_array(),
            spacing: p.volume.spacing,
            counts: p.counts,
            enabled: 1,
            irr_texel: [1.0 / p.irr_size[0] as f32, 1.0 / p.irr_size[1] as f32],
            dist_texel: [1.0 / p.dist_size[0] as f32, 1.0 / p.dist_size[1] as f32],
            // Смещения точки выборки — в долях шага сетки (как в RTXGI).
            normal_bias: 0.1,
            view_bias: 0.3,
            _pad: [0.0; 2],
        }
    }

    /// BLAS меша `index` (собирается в `list` при первом обращении).
    fn ensure_blas(&mut self, list: &ID3D12GraphicsCommandList4, frame: usize, meshes: &[Mesh], index: usize) -> Result<bool> {
        if index >= MAX_RT_MESHES as usize {
            return Ok(false);
        }
        if self.blas.len() <= index {
            self.blas.resize(index + 1, None);
        }
        if self.blas[index].is_some() {
            return Ok(true);
        }
        let mesh = &meshes[index];
        let vb = &mesh.vertex_buffer;
        let (index_count, ib_addr) = match &mesh.index_buffer {
            Some(ib) if mesh.index_count >= 3 => (mesh.index_count, unsafe { ib.resource.GetGPUVirtualAddress() }),
            _ => (0, 0),
        };
        if (index_count == 0 && mesh.vertex_count < 3) || vb.vertex_stride == 0 {
            return Ok(false);
        }
        let geom = D3D12_RAYTRACING_GEOMETRY_DESC {
            Type: D3D12_RAYTRACING_GEOMETRY_TYPE_TRIANGLES,
            Flags: D3D12_RAYTRACING_GEOMETRY_FLAG_OPAQUE,
            Anonymous: D3D12_RAYTRACING_GEOMETRY_DESC_0 {
                Triangles: D3D12_RAYTRACING_GEOMETRY_TRIANGLES_DESC {
                    Transform3x4: 0,
                    IndexFormat: if index_count > 0 { DXGI_FORMAT_R32_UINT } else { DXGI_FORMAT_UNKNOWN },
                    VertexFormat: DXGI_FORMAT_R32G32B32_FLOAT,
                    IndexCount: index_count,
                    VertexCount: mesh.vertex_count,
                    IndexBuffer: ib_addr,
                    VertexBuffer: D3D12_GPU_VIRTUAL_ADDRESS_AND_STRIDE { StartAddress: unsafe { vb.resource.GetGPUVirtualAddress() }, StrideInBytes: vb.vertex_stride as u64 },
                },
            },
        };
        let inputs = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
            Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_BOTTOM_LEVEL,
            Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
            NumDescs: 1,
            DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
            Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 { pGeometryDescs: &geom },
        };
        let mut info = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
        unsafe { self.device.GetRaytracingAccelerationStructurePrebuildInfo(&inputs, &mut info) };
        let result = buffer(&self.device, info.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE)?;
        let scratch = buffer(&self.device, info.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)?;
        unsafe {
            list.BuildRaytracingAccelerationStructure(
                &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC {
                    DestAccelerationStructureData: result.GetGPUVirtualAddress(),
                    Inputs: inputs,
                    SourceAccelerationStructureData: 0,
                    ScratchAccelerationStructureData: scratch.GetGPUVirtualAddress(),
                },
                None,
            );
        }
        // Буферы вершин/индексов меша — в bindless-таблицу (для нормалей в точке попадания).
        let slot = HEAP_FIXED + 2 * index as u32;
        raw_srv(&self.device, &vb.resource, vb.size, self.cpu(slot));
        if let Some(ib) = &mesh.index_buffer {
            raw_srv(&self.device, &ib.resource, ib.size, self.cpu(slot + 1));
        }
        self.frames[frame].garbage.push(scratch);
        self.blas[index] = Some(result);
        Ok(true)
    }

    /// Записать обновление проб в `list` (кадр `frame`). Ничего не делает,
    /// если объём не задан.
    pub(super) fn record(&mut self, list: &ID3D12GraphicsCommandList, frame: usize, meshes: &[Mesh], draws: &[RtDraw], lighting: &RtLighting) -> Result<()> {
        self.frames[frame].garbage.clear();
        if self.probes.is_none() {
            return Ok(());
        }
        let list: ID3D12GraphicsCommandList4 = list.cast()?;

        // BLAS и описания инстансов.
        let mut descs: Vec<D3D12_RAYTRACING_INSTANCE_DESC> = Vec::with_capacity(draws.len());
        let mut materials: Vec<RtInstance> = Vec::with_capacity(draws.len());
        let mut built = 0;
        for d in draws {
            let mesh = &meshes[d.mesh];
            if mesh.blend || mesh.unlit || d.tint[3] < 0.999 {
                continue;
            }
            let had = self.blas.get(d.mesh).is_some_and(|b| b.is_some());
            if !self.ensure_blas(&list, frame, meshes, d.mesh)? {
                continue;
            }
            built += (!had) as usize;
            let m = d.world;
            let row = |r: usize| [m.x_axis[r], m.y_axis[r], m.z_axis[r], m.w_axis[r]];
            let (r0, r1, r2) = (row(0), row(1), row(2));
            let mut transform = [0.0f32; 12];
            transform[0..4].copy_from_slice(&r0);
            transform[4..8].copy_from_slice(&r1);
            transform[8..12].copy_from_slice(&r2);
            descs.push(D3D12_RAYTRACING_INSTANCE_DESC {
                Transform: transform,
                _bitfield1: (materials.len() as u32 & 0x00FF_FFFF) | (0xFF << 24),
                // Двусторонние треугольники (как растеризация: CullMode NONE), все непрозрачные.
                _bitfield2: ((D3D12_RAYTRACING_INSTANCE_FLAG_TRIANGLE_CULL_DISABLE.0 | D3D12_RAYTRACING_INSTANCE_FLAG_FORCE_OPAQUE.0) as u32) << 24,
                AccelerationStructure: unsafe { self.blas[d.mesh].as_ref().unwrap().GetGPUVirtualAddress() },
            });
            let base = d.albedo;
            let albedo = [base[0] * d.tint[0], base[1] * d.tint[1], base[2] * d.tint[2]];
            let e = mesh.material_emissive;
            // ИСПРАВЛЕНО: индекс внутри таблицы Buffers[] (она начинается с
            // HEAP_FIXED, см. SetComputeRootDescriptorTable(6, ...)), а не
            // номер дескриптора в хипе — раньше луч читал нормали и индексы
            // меша на HEAP_FIXED/2 дальше: ложные «изнанки» гасили пробы.
            let slot = 2 * d.mesh as u32;
            materials.push(RtInstance {
                albedo: [albedo[0].min(1.0), albedo[1].min(1.0), albedo[2].min(1.0)],
                vertex_buffer: slot,
                emissive: [e[0] * albedo[0], e[1] * albedo[1], e[2] * albedo[2]],
                index_buffer: if mesh.index_buffer.is_some() && mesh.index_count >= 3 { slot + 1 } else { 0xFFFF_FFFF },
                metallic: mesh.material_metallic,
                _pad: [0; 3],
            });
        }
        if built > 0 {
            uav_barrier(&list, None);
        }
        if descs.is_empty() {
            return Ok(());
        }

        let device = self.device.clone();
        let upload = D3D12_HEAP_TYPE_UPLOAD;
        let none = D3D12_RESOURCE_FLAG_NONE;
        let read = D3D12_RESOURCE_STATE_GENERIC_READ;
        let f = &mut self.frames[frame];
        let instances = ensure(&device, &mut f.instances, std::mem::size_of_val(descs.as_slice()) as u64, upload, none, read)?;
        write(&instances, &descs)?;
        let material_buf = ensure(&device, &mut f.materials, std::mem::size_of_val(materials.as_slice()) as u64, upload, none, read)?;
        write(&material_buf, &materials)?;
        let lights_buf = ensure(&device, &mut f.lights, (std::mem::size_of_val(lighting.lights) as u64).max(64), upload, none, read)?;
        if !lighting.lights.is_empty() {
            write(&lights_buf, lighting.lights)?;
        }

        // TLAS.
        let inputs = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
            Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL,
            Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
            NumDescs: descs.len() as u32,
            DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
            Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 { InstanceDescs: unsafe { instances.GetGPUVirtualAddress() } },
        };
        let mut info = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
        unsafe { device.GetRaytracingAccelerationStructurePrebuildInfo(&inputs, &mut info) };
        let uav = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        let tlas = ensure(&device, &mut f.tlas, info.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT, uav, D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE)?;
        let scratch = ensure(&device, &mut f.tlas_scratch, info.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT, uav, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)?;
        unsafe {
            list.BuildRaytracingAccelerationStructure(
                &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC {
                    DestAccelerationStructureData: tlas.GetGPUVirtualAddress(),
                    Inputs: inputs,
                    SourceAccelerationStructureData: 0,
                    ScratchAccelerationStructureData: scratch.GetGPUVirtualAddress(),
                },
                None,
            );
        }
        uav_barrier(&list, Some(&tlas));

        // Константы кадра: случайный поворот сферы лучей (разные направления
        // каждый кадр — со временем пробы видят всю сферу, а не 128 точек).
        self.frame_counter += 1;
        let p = self.probes.as_ref().unwrap();
        let total = p.counts[0] * p.counts[1] * p.counts[2];
        let budget = p.volume.probes_per_frame.clamp(1, total);
        let offset = self.next_probe % total;
        self.next_probe = (offset + budget) % total;
        let rotation = random_rotation(self.frame_counter);
        let extent = (p.volume.max - p.volume.min).length();
        let constants = Constants {
            volume: Self::volume_of(p),
            ray_rotation: Mat4::from_quat(rotation).to_cols_array(),
            sun_dir: lighting.sun_dir,
            sun_color: lighting.sun_color,
            sky: lighting.sky,
            rays: p.volume.rays_per_probe,
            light_count: lighting.lights.len() as u32,
            hysteresis: p.volume.hysteresis.clamp(0.0, 0.999),
            // Лучи дальше объёма с запасом — до дальних стен/потолка.
            max_distance: extent * 2.0 + p.volume.spacing * 4.0,
            probe_offset: offset,
            probe_budget: budget,
            _pad: [0; 2],
        };
        let f = &mut self.frames[frame];
        let cb = ensure(&device, &mut f.constants, 256, upload, none, read)?;
        write(&cb, std::slice::from_ref(&constants))?;

        // Порция проб этого кадра.
        let probe_count = budget;
        let rays = p.volume.rays_per_probe;
        let (ray_data, irr, dist) = (p.ray_data.clone(), p.irr.clone(), p.dist.clone());
        let (irr_size, dist_size) = (p.irr_size, p.dist_size);
        let (cleared, atlases_srv) = (p.cleared, p.atlases_srv);

        unsafe {
            list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            list.SetComputeRootSignature(&self.root);
            list.SetComputeRootConstantBufferView(0, cb.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(1, tlas.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(2, material_buf.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(3, lights_buf.GetGPUVirtualAddress());
            list.SetComputeRootDescriptorTable(4, self.gpu(3));
            list.SetComputeRootDescriptorTable(5, self.gpu(0));
            list.SetComputeRootDescriptorTable(6, self.gpu(HEAP_FIXED));

            let all_srv = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
            let ua = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
            if !cleared {
                // Атласы ещё в UNORDERED_ACCESS (как созданы).
                list.SetPipelineState(&self.psos[K_CLEAR]);
                let w = irr_size[0].max(dist_size[0]);
                let h = irr_size[1].max(dist_size[1]);
                list.Dispatch(w.div_ceil(8), h.div_ceil(8), 1);
                uav_barrier(&list, None);
            }
            // Трассировка читает атласы прошлого кадра.
            if !atlases_srv {
                transitions(&list, &[(&irr, ua, all_srv), (&dist, ua, all_srv)]);
            }
            list.SetPipelineState(&self.psos[K_TRACE]);
            list.Dispatch(rays.div_ceil(32), probe_count, 1);

            transitions(&list, &[(&ray_data, ua, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE), (&irr, all_srv, ua), (&dist, all_srv, ua)]);
            list.SetPipelineState(&self.psos[K_IRR]);
            list.Dispatch(probe_count, 1, 1);
            list.SetPipelineState(&self.psos[K_DIST]);
            list.Dispatch(probe_count, 1, 1);
            uav_barrier(&list, None);
            list.SetPipelineState(&self.psos[K_BORDER_IRR]);
            list.Dispatch(probe_count, 1, 1);
            list.SetPipelineState(&self.psos[K_BORDER_DIST]);
            list.Dispatch(probe_count, 1, 1);
            uav_barrier(&list, None);
            transitions(&list, &[(&ray_data, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, ua), (&irr, ua, all_srv), (&dist, ua, all_srv)]);
        }
        let p = self.probes.as_mut().unwrap();
        p.cleared = true;
        p.atlases_srv = true;
        Ok(())
    }
}

impl Gi {
    /// ДОБАВЛЕНО (отражения лучами): один отражённый луч на пиксель по
    /// G-буферу (`spec`, `normal` — разрешённые, PIXEL_SHADER_RESOURCE),
    /// результат «цвет + вес * отражение» копируется обратно в `color`
    /// (hdr_resolved). Нужны TLAS и константы, записанные `record` этого
    /// же кадра; без них — false (кадр не тронут).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_reflections(&mut self, list: &ID3D12GraphicsCommandList, frame: usize, spec: &ID3D12Resource, normal: &ID3D12Resource, color: &ID3D12Resource, size: [u32; 2], inv_view_proj: Mat4, cam_pos: Vec3, max_roughness: f32) -> Result<bool> {
        if self.probes.is_none() {
            return Ok(false);
        }
        let f = &self.frames[frame];
        let (Some(cb), Some(tlas), Some(materials), Some(lights)) = (f.constants.resource.clone(), f.tlas.resource.clone(), f.materials.resource.clone(), f.lights.resource.clone()) else {
            return Ok(false);
        };
        let list4: ID3D12GraphicsCommandList4 = list.cast()?;
        let [w, h] = size;
        if !self.refl_out.as_ref().is_some_and(|(_, ow, oh)| *ow == w && *oh == h) {
            let t = texture(&self.device, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT)?;
            texture_uav(&self.device, &t, DXGI_FORMAT_R16G16B16A16_FLOAT, self.cpu(REFL_SLOT + 4));
            let half = texture(&self.device, w.div_ceil(2), h.div_ceil(2), DXGI_FORMAT_R16G16B16A16_FLOAT)?;
            texture_srv(&self.device, Some(&half), DXGI_FORMAT_R16G16B16A16_FLOAT, self.cpu(REFL_SLOT + 3));
            texture_uav(&self.device, &half, DXGI_FORMAT_R16G16B16A16_FLOAT, self.cpu(REFL_SLOT + 5));
            // прежний выход может читать кадр в полёте — освободится вместе с
            // мусором этого слота кадра (после его fence)
            if let Some((old, _, _)) = self.refl_out.replace((t, w, h)) {
                self.frames[frame].garbage.push(old);
            }
            if let Some(old) = self.refl_half.replace(half) {
                self.frames[frame].garbage.push(old);
            }
        }
        for (k, r) in [spec, normal, color].into_iter().enumerate() {
            let ptr = r.as_raw() as usize;
            if self.refl_srcs[k] != ptr {
                texture_srv(&self.device, Some(r), DXGI_FORMAT_R16G16B16A16_FLOAT, self.cpu(REFL_SLOT + k as u32));
                self.refl_srcs[k] = ptr;
            }
        }
        let out = self.refl_out.as_ref().unwrap().0.clone();
        let half = self.refl_half.clone().unwrap();
        let pixel = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        let all_srv = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        let ua = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
        let c = ReflConstants {
            inv_view_proj: inv_view_proj.to_cols_array(),
            cam_pos: cam_pos.to_array(),
            frame: self.frame_counter as u32,
            size: [w as f32, h as f32],
            max_roughness,
            _pad: 0.0,
        };
        transitions(&list4, &[(spec, pixel, all_srv), (normal, pixel, all_srv), (color, pixel, all_srv)]);
        unsafe {
            list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            list.SetComputeRootSignature(&self.root);
            list.SetComputeRootConstantBufferView(0, cb.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(1, tlas.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(2, materials.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(3, lights.GetGPUVirtualAddress());
            list.SetComputeRootDescriptorTable(4, self.gpu(3));
            list.SetComputeRootDescriptorTable(5, self.gpu(0));
            list.SetComputeRootDescriptorTable(6, self.gpu(HEAP_FIXED));
            list.SetComputeRootDescriptorTable(7, self.gpu(REFL_SLOT));
            list.SetComputeRoot32BitConstants(8, (std::mem::size_of::<ReflConstants>() / 4) as u32, &c as *const _ as *const _, 0);
            list.SetPipelineState(&self.psos[K_REFL]);
            list.Dispatch(w.div_ceil(2).div_ceil(8), h.div_ceil(2).div_ceil(8), 1);
        }
        transitions(&list4, &[(&half, ua, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)]);
        unsafe {
            list.SetPipelineState(&self.psos[K_REFL_RESOLVE]);
            list.Dispatch(w.div_ceil(8), h.div_ceil(8), 1);
        }
        transitions(&list4, &[(&half, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, ua)]);
        transitions(&list4, &[(&out, ua, D3D12_RESOURCE_STATE_COPY_SOURCE), (color, all_srv, D3D12_RESOURCE_STATE_COPY_DEST), (spec, all_srv, pixel), (normal, all_srv, pixel)]);
        unsafe { list.CopyResource(color, &out) };
        transitions(&list4, &[(&out, D3D12_RESOURCE_STATE_COPY_SOURCE, ua), (color, D3D12_RESOURCE_STATE_COPY_DEST, pixel)]);
        Ok(true)
    }
}

/// Равномерно случайный поворот (кватернион из трёх равномерных чисел, Shoemake).
fn random_rotation(seed: u64) -> Quat {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f32 / (1u64 << 53) as f32
    };
    let (u1, u2, u3) = (next(), next(), next());
    let tau = std::f32::consts::TAU;
    let (a, b) = ((1.0 - u1).sqrt(), u1.sqrt());
    Quat::from_xyzw(a * (tau * u2).sin(), a * (tau * u2).cos(), b * (tau * u3).sin(), b * (tau * u3).cos()).normalize()
}

impl super::AlkashEngine {
    /// ДОБАВЛЕНО (непрямой свет лучами, DDGI): включить непрямой свет в
    /// объёме `volume` (None — выключить, вернуть равномерный ambient).
    /// Нужны DXR 1.1 и DXC (см. dxc.rs); без них — false и всё как раньше.
    /// Пробы стоят сеткой с шагом `spacing` от `min` до `max`: объём должен
    /// охватывать место действия (за его пределами — прежний ambient).
    pub fn set_gi_volume(&mut self, volume: Option<GiVolume>) -> bool {
        if volume.is_some() && self.gi.is_none() && !self.gi_unavailable {
            match Gi::new() {
                Ok(g) => self.gi = Some(g),
                Err(e) => {
                    eprintln!("[GI] непрямой свет недоступен: {}", e);
                    self.gi_unavailable = true;
                }
            }
        }
        let Some(gi) = self.gi.as_mut() else { return false };
        // Атласы читает кадр в полёте — сначала дождаться GPU.
        self.wait_for_all_frames_idle_before_realloc();
        let ok = self.gi.as_mut().unwrap().set_volume(volume).is_ok();
        self.write_gi_srvs();
        ok && volume.is_some()
    }

    /// SRV атласов проб в таблице основного прохода (слоты GI_*_SLOT
    /// shadow_srv_heap); без объёма — пустые SRV (шейдер их не читает,
    /// но таблица должна быть целиком валидной).
    pub(super) fn write_gi_srvs(&self) {
        let (Some(heap), Ok(device)) = (&self.shadow_srv_heap, crate::get_device()) else { return };
        let size = crate::STATE.lock().unwrap().cbv_srv_uav_descriptor_size;
        let atlases = self.gi.as_ref().and_then(|g| g.atlases());
        let slot = |s: usize| crate::heap::DescriptorHeap::get_cpu_handle(heap, s as u32, size);
        texture_srv(&device, atlases.map(|a| a.0), IRR_FORMAT, slot(super::GI_IRRADIANCE_SLOT));
        texture_srv(&device, atlases.map(|a| a.1), DIST_FORMAT, slot(super::GI_DISTANCE_SLOT));
        // ДОБАВЛЕНО (forward-декали): их слоты в той же таблице — после
        // перевыделения хипа пишутся заново вместе с атласами проб
        self.write_decal_srvs();
    }

    /// Отладка непрямого света: вместо кадра — только рассеянный свет в
    /// каждой точке (без альбедо), умноженный на `gain`; 0 — обычный кадр.
    pub fn set_gi_debug(&mut self, gain: f32) {
        if let Some(gi) = self.gi.as_mut() {
            gi.debug_gain = gain.max(0.0);
        }
    }

    /// Средний линейный цвет поверхности меша: вершины * текстура альбедо.
    pub(super) fn mesh_albedo(&self, mesh: &Mesh) -> [f32; 3] {
        let tex = mesh
            .albedo_srv_index
            .and_then(|slot| slot.checked_sub(super::SHADOW_TABLE_SLOTS as u32))
            .and_then(|i| self.material_texture_avg.get(i as usize))
            .copied()
            .unwrap_or([1.0; 3]);
        [mesh.avg_color[0] * tex[0], mesh.avg_color[1] * tex[1], mesh.avg_color[2] * tex[2]]
    }
}
