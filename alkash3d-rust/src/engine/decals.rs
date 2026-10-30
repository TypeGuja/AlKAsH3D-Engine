//! ДОБАВЛЕНО (forward-декали): надписи, трещины, копоть, пятна — без своей
//! геометрии. Движок прямой (forward), отложенного G-буфера нет: декаль
//! накладывается в основном пиксельном шейдере ДО освещения — меняет цвет
//! поверхности и шероховатость (как у Doom 2016). Каждая декаль — коробка в
//! мире (проекция вдоль её оси Z) и прямоугольник в общем атласе.
//!
//! Пиксель смотрит только декали своей ячейки мировой сетки (2 м): буфер
//! декалей (t15, в [0] — заголовок сетки), ячейки (t16, смещение+число) и
//! индексы (t17), атлас — t14. Всё в таблице теней (root-параметр 4) — по
//! лимиту корневой сигнатуры (64 слова) места больше нет. Без декалей
//! заголовок нулевой — шейдер выходит после одного чтения.

use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::math::{Mat4, Quat, Vec3};

/// Слоты в shadow_srv_heap (таблица root-параметра 4): атлас t14, буферы t15..t17.
pub const DECAL_ATLAS_SLOT: usize = super::NUM_CASCADES + 4;
pub const DECAL_BUFFER_SLOT: usize = super::NUM_CASCADES + 5;
pub const DECAL_CELLS_SLOT: usize = super::NUM_CASCADES + 6;
pub const DECAL_INDEX_SLOT: usize = super::NUM_CASCADES + 7;
const CELL: f32 = 2.0;
/// Предел сетки по каждой оси (ячеек): больше — сетка крупнее ячейки.
const MAX_DIM: u32 = 96;

/// Декаль: коробка `size` с центром `center`, повёрнутая `rotation`;
/// проецируется вдоль своей оси −Z (на поверхности, повёрнутые к ней).
#[derive(Clone, Copy, Debug)]
pub struct Decal {
    pub center: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
    /// u0, v0, u1, v1 — прямоугольник в атласе.
    pub uv: [f32; 4],
    /// Множитель цвета атласа и непрозрачность.
    pub color: [f32; 4],
    /// Шероховатость под декалью (−1 — не трогать): краска глаже бетона,
    /// масляное пятно на асфальте — почти зеркало.
    pub roughness: f32,
}

impl Decal {
    /// Декаль на плоскости с нормалью `normal`: `up` — верх картинки на
    /// плоскости, `w`×`h` — размер, `depth` — толщина коробки (запас на
    /// неровности поверхности).
    pub fn on_plane(center: Vec3, normal: Vec3, up: Vec3, w: f32, h: f32, depth: f32, uv: [f32; 4], color: [f32; 4]) -> Decal {
        let z = normal.normalize();
        let y = (up - z * up.dot(z)).normalize();
        let x = y.cross(z);
        let rotation = Quat::from_mat3(&crate::math::Mat3::from_cols(x, y, z));
        Decal { center, rotation, size: Vec3::new(w, h, depth), uv, color, roughness: -1.0 }
    }

    /// Мир → коробка (координаты в [−0.5, 0.5] внутри).
    fn world_to_box(&self) -> Mat4 {
        (Mat4::from_translation(self.center) * Mat4::from_quat(self.rotation) * Mat4::from_scale(self.size)).inverse()
    }

    fn corners(&self) -> [Vec3; 8] {
        let m = Mat4::from_translation(self.center) * Mat4::from_quat(self.rotation) * Mat4::from_scale(self.size);
        std::array::from_fn(|i| m.transform_point3(Vec3::new(if i & 1 == 0 { -0.5 } else { 0.5 }, if i & 2 == 0 { -0.5 } else { 0.5 }, if i & 4 == 0 { -0.5 } else { 0.5 })))
    }
}

/// Как struct DecalGpu в main_ps.hlsl (6 × float4 = 96 байт).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DecalGpu {
    r: [[f32; 4]; 3],
    uv: [f32; 4],
    color: [f32; 4],
    /// xyz — нормаль плоскости декали в мире (к ней повёрнутые поверхности
    /// её получают), w — шероховатость (<0 — не трогать).
    params: [f32; 4],
}

pub(super) struct DecalSet {
    atlas: crate::texture::Texture,
    decals: ID3D12Resource,
    cells: ID3D12Resource,
    indices: ID3D12Resource,
    counts: [u32; 3],
}

fn upload_buffer<T: Copy>(data: &[T]) -> Result<ID3D12Resource> {
    let device = crate::get_device()?;
    let size = (std::mem::size_of_val(data).max(16)) as u64;
    let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    unsafe {
        device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut r)?;
        let r = r.ok_or_else(|| Error::from_hresult(HRESULT(0x80004005u32 as i32)))?;
        let mut ptr = std::ptr::null_mut();
        r.Map(0, None, Some(&mut ptr))?;
        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, ptr as *mut u8, std::mem::size_of_val(data));
        r.Unmap(0, None);
        Ok(r)
    }
}

fn buffer_srv(device: &ID3D12Device, r: Option<&ID3D12Resource>, count: u32, stride: u32, handle: D3D12_CPU_DESCRIPTOR_HANDLE) {
    let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV { FirstElement: 0, NumElements: count.max(1), StructureByteStride: stride, Flags: D3D12_BUFFER_SRV_FLAG_NONE },
        },
    };
    unsafe { device.CreateShaderResourceView(r, Some(&desc), handle) };
}

impl DecalSet {
    fn build(atlas_rgba: &[u8], atlas_w: u32, atlas_h: u32, decals: &[Decal]) -> Result<DecalSet> {
        let atlas = crate::texture::Texture::create_texture2d(atlas_w, atlas_h, DXGI_FORMAT_R8G8B8A8_UNORM, Some(atlas_rgba))?;
        // сетка по общей коробке всех декалей
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        let boxes: Vec<(Vec3, Vec3)> = decals
            .iter()
            .map(|d| {
                let c = d.corners();
                let (a, b) = c.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
                lo = lo.min(a);
                hi = hi.max(b);
                (a, b)
            })
            .collect();
        let cell = if decals.is_empty() { CELL } else { CELL.max(((hi - lo).max_element() / MAX_DIM as f32) * 1.001) };
        let dims = if decals.is_empty() { [0u32; 3] } else { std::array::from_fn(|k| (((hi[k] - lo[k]) / cell).floor() as u32 + 1).min(MAX_DIM)) };
        let n_cells = (dims[0] * dims[1] * dims[2]) as usize;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); n_cells];
        for (i, (a, b)) in boxes.iter().enumerate() {
            let c0: [u32; 3] = std::array::from_fn(|k| (((a[k] - lo[k]) / cell).floor().max(0.0) as u32).min(dims[k].saturating_sub(1)));
            let c1: [u32; 3] = std::array::from_fn(|k| (((b[k] - lo[k]) / cell).floor().max(0.0) as u32).min(dims[k].saturating_sub(1)));
            for z in c0[2]..=c1[2] {
                for y in c0[1]..=c1[1] {
                    for x in c0[0]..=c1[0] {
                        lists[((z * dims[1] + y) * dims[0] + x) as usize].push(i as u32 + 1);
                    }
                }
            }
        }
        let mut cells: Vec<[u32; 2]> = Vec::with_capacity(n_cells.max(1));
        let mut indices: Vec<u32> = Vec::new();
        for l in &lists {
            cells.push([indices.len() as u32, l.len() as u32]);
            indices.extend(l);
        }
        // [0] — заголовок: начало сетки и ячейка, число ячеек по осям
        let mut gpu = vec![DecalGpu {
            r: [[lo.x, lo.y, lo.z, cell], [dims[0] as f32, dims[1] as f32, dims[2] as f32, decals.len() as f32], [0.0; 4]],
            ..Default::default()
        }];
        for d in decals {
            let m = d.world_to_box().to_cols_array_2d();
            // строки аффинной матрицы мир → коробка
            let row = |k: usize| [m[0][k], m[1][k], m[2][k], m[3][k]];
            let axis = d.rotation * Vec3::Z;
            gpu.push(DecalGpu { r: [row(0), row(1), row(2)], uv: d.uv, color: d.color, params: [axis.x, axis.y, axis.z, d.roughness] });
        }
        if cells.is_empty() {
            cells.push([0, 0]);
        }
        if indices.is_empty() {
            indices.push(0);
        }
        Ok(DecalSet {
            atlas,
            decals: upload_buffer(&gpu)?,
            cells: upload_buffer(&cells)?,
            indices: upload_buffer(&indices)?,
            counts: [gpu.len() as u32, cells.len() as u32, indices.len() as u32],
        })
    }
}

impl super::AlkashEngine {
    /// Задать все декали сцены и их атлас (RGBA8, альфа — маска). Пустой
    /// список — декалей нет. Дорого (атлас, сетка) — один раз при загрузке.
    pub fn set_decals(&mut self, atlas_rgba: &[u8], atlas_w: u32, atlas_h: u32, decals: &[Decal]) -> Result<()> {
        // старые буферы может читать кадр в полёте
        self.wait_for_all_frames_idle_before_realloc();
        self.decal_set = Some(DecalSet::build(atlas_rgba, atlas_w, atlas_h, decals)?);
        self.write_decal_srvs();
        println!("[ENGINE] ✓ декали: {} шт., атлас {}x{}", decals.len(), atlas_w, atlas_h);
        Ok(())
    }

    /// SRV декалей в таблице теней (слоты DECAL_*); без декалей — пустые
    /// (нули: заголовок сетки с нулевыми размерами — шейдер ничего не делает).
    pub(super) fn write_decal_srvs(&self) {
        let (Some(heap), Ok(device)) = (&self.shadow_srv_heap, crate::get_device()) else { return };
        let size = crate::STATE.lock().unwrap().cbv_srv_uav_descriptor_size;
        let slot = |s: usize| crate::heap::DescriptorHeap::get_cpu_handle(heap, s as u32, size);
        match &self.decal_set {
            Some(d) => {
                let _ = d.atlas.create_srv(slot(DECAL_ATLAS_SLOT));
                buffer_srv(&device, Some(&d.decals), d.counts[0], std::mem::size_of::<DecalGpu>() as u32, slot(DECAL_BUFFER_SLOT));
                buffer_srv(&device, Some(&d.cells), d.counts[1], 8, slot(DECAL_CELLS_SLOT));
                buffer_srv(&device, Some(&d.indices), d.counts[2], 4, slot(DECAL_INDEX_SLOT));
            }
            None => {
                let tex = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
                };
                unsafe { device.CreateShaderResourceView(None, Some(&tex), slot(DECAL_ATLAS_SLOT)) };
                buffer_srv(&device, None, 1, std::mem::size_of::<DecalGpu>() as u32, slot(DECAL_BUFFER_SLOT));
                buffer_srv(&device, None, 1, 8, slot(DECAL_CELLS_SLOT));
                buffer_srv(&device, None, 1, 4, slot(DECAL_INDEX_SLOT));
            }
        }
    }
}
