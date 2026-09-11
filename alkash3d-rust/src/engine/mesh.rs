//! Геометрические примитивы движка: вершинный формат (`Vertex`), GPU-меш
//! (`Mesh` — vertex/index buffer + материал + bounding sphere для culling) и
//! `MeshInstance` (лёгкая транспозиция меша в мире — позиция/поворот/
//! масштаб).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы, см. план: движок готовится к
//! гораздо более требовательному проекту, чем текущая My Summer Car-подобная
//! демка). Перенос дословный — ни одна из структур этого файла не зависит от
//! `AlkashEngine`, поэтому это чистое перемещение кода без изменения логики.

use windows::core::*;
use crate::*;
use crate::math::{Mat4, Vec3};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 4],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    /// ДОБАВЛЕНО (Задача #15, продолжение — normal mapping): касательный
    /// вектор в object space, xyz + w=handedness ("рукость" — ±1.0,
    /// определяет знак `bitangent = cross(normal, tangent.xyz) * tangent.w`
    /// в шейдере). Стандартный компактный способ хранить TBN-базис без
    /// отдельного bitangent-поля (bitangent однозначно восстанавливается
    /// из normal+tangent+знака) — тот же приём, что в glTF/Assimp.
    /// В КОНЦЕ структуры (после uv, не между полями) — та же причина, что
    /// у добавления uv ранее в этой же Задаче: не сдвигает офсеты уже
    /// существующих полей ни здесь, ни в HLSL input layout (см.
    /// `PipelineState::create_graphics`/`create_shadow_pipeline_state` в
    /// pso.rs — оба теперь дополнительно объявляют TANGENT@52).
    pub tangent: [f32; 4],
}

impl Vertex {
    pub const STRIDE: u32 = std::mem::size_of::<Vertex>() as u32;

    pub fn new(x: f32, y: f32, z: f32, r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::with_normal(x, y, z, 0.0, 0.0, 1.0, r, g, b, a)
    }

    pub fn with_normal(x: f32, y: f32, z: f32, nx: f32, ny: f32, nz: f32, r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::with_normal_uv(x, y, z, nx, ny, nz, r, g, b, a, 0.0, 0.0)
    }

    /// ДОБАВЛЕНО (Задача #15): полная форма конструктора с явным UV —
    /// `with_normal`/`new` остаются как есть (UV=(0,0) по умолчанию,
    /// безвредно для меша без текстуры — см. `albedo_srv_index: None`
    /// в `Mesh`, при котором пиксельный шейдер вообще не сэмплирует
    /// текстуру, UV в таком случае не используется) — чтобы не переписывать
    /// десятки существующих вызовов `Vertex::new`/`with_normal` по всему
    /// движку (процедурные меши, отладочная геометрия) ради поля, которое
    /// им не нужно. `tangent` по умолчанию — произвольный, но детерминированный
    /// вектор, ортогональный оси Z (см. `default_tangent_for_normal`) —
    /// геометрия без честного tangent (процедурные кубы/плитки без normal
    /// map) никогда не сэмплирует NormalMap (см. `Mesh::normal_srv_index:
    /// None` → шейдер использует геометрическую нормаль напрямую), поэтому
    /// приблизительность этого дефолта безвредна — тот же принцип, что и у
    /// UV=(0,0) для меша без albedo-текстуры.
    pub fn with_normal_uv(x: f32, y: f32, z: f32, nx: f32, ny: f32, nz: f32, r: f32, g: f32, b: f32, a: f32, u: f32, v: f32) -> Self {
        let tangent = Self::default_tangent_for_normal([nx, ny, nz]);
        Self {
            position: [x, y, z, 1.0],
            normal: [nx, ny, nz],
            color: [r, g, b, a],
            uv: [u, v],
            tangent,
        }
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping): полная форма конструктора с
    /// явным tangent — используется реальной .altex-геометрией (см.
    /// `altex_vertex_to_engine_vertex`), у которой честный tangent уже
    /// запечён при экспорте (перпендикулярен UV-развёртке, а не выбран
    /// произвольно, как в `default_tangent_for_normal`).
    pub fn with_normal_uv_tangent(x: f32, y: f32, z: f32, nx: f32, ny: f32, nz: f32, r: f32, g: f32, b: f32, a: f32, u: f32, v: f32, tangent: [f32; 4]) -> Self {
        Self {
            position: [x, y, z, 1.0],
            normal: [nx, ny, nz],
            color: [r, g, b, a],
            uv: [u, v],
            tangent,
        }
    }

    /// Произвольный, но детерминированный tangent, ортогональный данной
    /// нормали — НЕ соответствует реальной UV-развёртке (только честный
    /// tangent из .altex это гарантирует), но даёт математически корректный
    /// (единичный, перпендикулярный normal) базис для геометрии, у которой
    /// нет запечённого tangent и которая всё равно не использует normal map
    /// (см. комментарий у `with_normal_uv`). Используется gram-schmidt-подобный
    /// выбор опорной оси: world-up (0,1,0), либо world-right (1,0,0), если
    /// normal почти параллельна up (иначе cross() выродился бы в почти
    /// нулевой вектор).
    fn default_tangent_for_normal(normal: [f32; 3]) -> [f32; 4] {
        let n = normal;
        let up = if n[1].abs() < 0.99 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let t = [
            up[1] * n[2] - up[2] * n[1],
            up[2] * n[0] - up[0] * n[2],
            up[0] * n[1] - up[1] * n[0],
        ];
        let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-6);
        [t[0] / len, t[1] / len, t[2] / len, 1.0]
    }
}

pub struct Mesh {
    pub vertex_buffer: Buffer,
    pub vertex_count: u32,
    pub index_buffer: Option<Buffer>,
    pub index_count: u32,
    /// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): индекс SRV этого
    /// меша (albedo-текстура) в `AlkashEngine::shadow_srv_heap`
    /// (material-часть — см. `ensure_material_srv_capacity`) — `None`
    /// значит "текстуры нет, красить только вершинным цветом", как и
    /// работало ДО этой задачи (см. `ComputePointLightContribution`/main()
    /// в пиксельном шейдере — если albedo-текстуры нет, шейдер использует
    /// нейтральный белый (1,1,1,1), что эквивалентно полному отсутствию
    /// текстурного умножения). Не `Option<usize>` — индекс в дескрипторном
    /// хипе физически ограничен 32-битным пространством D3D12
    /// (`OffsetInDescriptorsFromTableStart: u32`), поэтому u32 честнее и
    /// не требует приведения типов на каждый вызов
    /// `DescriptorHeap::get_gpu_handle`.
    pub albedo_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): индекс SRV normal map этого
    /// меша в том же `shadow_srv_heap` (material-часть), что и
    /// `albedo_srv_index` — `None` значит "нет карты нормалей, использовать
    /// геометрическую нормаль как есть" (см. `flat_normal_srv_fallback` в
    /// render_frame — в отличие от albedo, где отсутствующая карта
    /// заменяется НЕЙТРАЛЬНОЙ текстурой (белой), здесь тоже используется
    /// нейтральная "плоская" normal map (128,128,255) — RGB, декодируемая в
    /// tangent-space (0,0,1) — а не пропуск сэмплирования в шейдере: та же
    /// причина, что и у albedo (безусловный путь без HLSL-ветвления)).
    pub normal_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): индекс SRV
    /// metallic-roughness текстуры (R=metallic, G=roughness — упаковка
    /// такая же, как в glTF, но БЕЗ канала occlusion — `ao` в .altex
    /// Material отдельный скаляр, не карта в этой версии) — `None` значит
    /// "нет карты, использовать скалярные `metallic`/`roughness` из
    /// материала напрямую" (см. `mesh_metallic_roughness` в render_frame).
    pub mr_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): скалярные PBR-параметры
    /// материала — используются В ДОПОЛНЕНИЕ к `mr_srv_index` (если карта
    /// есть, шейдер берёт значения из неё; если карты нет, использует ЭТИ
    /// скаляры напрямую — см. `PSRootConstants` в render_frame). Дефолты
    /// (metallic=0.0, roughness=0.8) — нейтральный "обычный диэлектрик,
    /// довольно шершавый" материал, БИТ В БИТ совпадающий с тем, что было
    /// у ВСЕЙ геометрии до этой задачи (полностью диффузная, без бликов) —
    /// см. `Material::default` в altex_format.rs, откуда эти же дефолты
    /// берёт `add_material`.
    pub material_metallic: f32,
    pub material_roughness: f32,
    /// ДОБАВЛЕНО (оптимизация рендера — CPU-side frustum culling, см.
    /// `crate::math::Frustum`): ограничивающая сфера меша В ЛОКАЛЬНЫХ
    /// (model-space, ДО умножения на world-матрицу) координатах —
    /// `bounding_center` = центр AABB меша, `bounding_radius` =
    /// максимальное расстояние от этого центра до любой вершины.
    /// Считается ОДИН РАЗ при создании меша (см. `from_vertices` ниже), не
    /// каждый кадр — рендер-цикл просто трансформирует готовый центр в
    /// мировые координаты через текущую model-матрицу объекта и сравнивает
    /// с фрустумом камеры.
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
}

impl Mesh {
    pub fn from_vertices(vertices: &[Vertex]) -> Result<Self> {
        let vertex_data: Vec<u8> = vertices
            .iter()
            .flat_map(|v| {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&v.position[0].to_le_bytes());
                bytes.extend_from_slice(&v.position[1].to_le_bytes());
                bytes.extend_from_slice(&v.position[2].to_le_bytes());
                bytes.extend_from_slice(&v.position[3].to_le_bytes());
                bytes.extend_from_slice(&v.normal[0].to_le_bytes());
                bytes.extend_from_slice(&v.normal[1].to_le_bytes());
                bytes.extend_from_slice(&v.normal[2].to_le_bytes());
                bytes.extend_from_slice(&v.color[0].to_le_bytes());
                bytes.extend_from_slice(&v.color[1].to_le_bytes());
                bytes.extend_from_slice(&v.color[2].to_le_bytes());
                bytes.extend_from_slice(&v.color[3].to_le_bytes());
                bytes.extend_from_slice(&v.uv[0].to_le_bytes());
                bytes.extend_from_slice(&v.uv[1].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[0].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[1].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[2].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[3].to_le_bytes());
                bytes
            })
            .collect();

        let buffer = Buffer::create_vertex_buffer(&vertex_data, Vertex::STRIDE)?;

        let (bounding_center, bounding_radius) = if vertices.is_empty() {
            ([0.0, 0.0, 0.0], 0.0)
        } else {
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            for v in vertices {
                for axis in 0..3 {
                    let p = v.position[axis];
                    if p < min[axis] { min[axis] = p; }
                    if p > max[axis] { max[axis] = p; }
                }
            }
            let center = [
                (min[0] + max[0]) * 0.5,
                (min[1] + max[1]) * 0.5,
                (min[2] + max[2]) * 0.5,
            ];
            let mut radius_sq = 0.0f32;
            for v in vertices {
                let dx = v.position[0] - center[0];
                let dy = v.position[1] - center[1];
                let dz = v.position[2] - center[2];
                let d_sq = dx * dx + dy * dy + dz * dz;
                if d_sq > radius_sq { radius_sq = d_sq; }
            }
            (center, radius_sq.sqrt())
        };

        Ok(Self {
            vertex_buffer: buffer,
            vertex_count: vertices.len() as u32,
            index_buffer: None,
            index_count: 0,
            albedo_srv_index: None,
            normal_srv_index: None,
            mr_srv_index: None,
            material_metallic: 0.0,
            material_roughness: 0.8,
            bounding_center,
            bounding_radius,
        })
    }

    pub fn from_vertices_and_indices(vertices: &[Vertex], indices: &[u32]) -> Result<Self> {
        let mut mesh = Self::from_vertices(vertices)?;
        mesh.index_buffer = Some(Buffer::create_index_buffer(indices)?);
        mesh.index_count = indices.len() as u32;
        Ok(mesh)
    }

    pub fn triangle() -> Result<Self> {
        let vertices = [
            Vertex::new(-0.8, -0.8, 0.5, 1.0, 0.0, 0.0, 1.0),
            Vertex::new(0.0, 0.8, 0.5, 0.0, 1.0, 0.0, 1.0),
            Vertex::new(0.8, -0.8, 0.5, 0.0, 0.0, 1.0, 1.0),
        ];
        Self::from_vertices(&vertices)
    }

    pub fn quad(x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Result<Self> {
        let half_w = width / 2.0;
        let half_h = height / 2.0;
        let left = x - half_w;
        let right = x + half_w;
        let top = y + half_h;
        let bottom = y - half_h;

        let vertices = [
            Vertex::new(left, bottom, 0.0, color[0], color[1], color[2], color[3]),
            Vertex::new(right, bottom, 0.0, color[0], color[1], color[2], color[3]),
            Vertex::new(left, top, 0.0, color[0], color[1], color[2], color[3]),
            Vertex::new(right, top, 0.0, color[0], color[1], color[2], color[3]),
        ];

        let indices = [0, 1, 2, 1, 3, 2];
        Self::from_vertices_and_indices(&vertices, &indices)
    }

    pub fn cube(size: f32) -> Result<Self> {
        let half = size / 2.0;

        let vertices = [
            Vertex::with_normal(-half, -half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(-half,  half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal( half,  half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(-half, -half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal(-half,  half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half,  half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal(-half,  half, -half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half, -half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal(-half,  half,  half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half,  half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal(-half, -half, -half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, -half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal(-half, -half,  half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half,  half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, -half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half, -half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half, -half,  half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half,  half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal(-half, -half, -half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
            Vertex::with_normal(-half,  half, -half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
            Vertex::with_normal(-half, -half,  half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
            Vertex::with_normal(-half,  half,  half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
        ];

        let indices = [
            0,1,2, 1,3,2,
            4,6,5, 5,6,7,
            8,10,9, 9,10,11,
            12,13,14, 13,15,14,
            16,18,17, 17,18,19,
            20,21,22, 21,23,22,
        ];

        Self::from_vertices_and_indices(&vertices, &indices)
    }

    /// ДОБАВЛЕНО (фикс бага "точечные фонари как будто не освещают
    /// поверхность/пол остаётся одного цвета независимо от близости к
    /// фонарю"): `cube()` выше красит КАЖДУЮ грань в свой отладочный цвет
    /// (front=красный, top=синий, bottom=жёлтый и т.д. — удобно для
    /// визуальной проверки нормалей/ориентации граней, поэтому оставлен
    /// как есть, используется в main1.rs/main2.rs). Проблема в том, что
    /// основной пиксельный шейдер финально умножает `input.color.rgb *
    /// brightness` (см. main() в compile_default_shaders) — то есть
    /// ВЕРШИННЫЙ цвет работает как альбедо/маска поверхности, а не просто
    /// декорация поверх освещения. Top face `cube()` имеет цвет (0,0,1)
    /// — ЧИСТО синий, у которого R и G каналы РОВНО НОЛЬ — из-за этого
    /// тёплый жёлто-белый свет фонаря (высокие R/G, средний B) после
    /// умножения на (0,0,1) визуально ПОЛНОСТЬЮ теряет свой тёплый
    /// оттенок и яркость по R/G, остаётся только синий канал ambient —
    /// именно поэтому пол выглядел одинаково холодно-синим независимо от
    /// близости к фонарю, хотя сам расчёт освещения (attenuation/culling)
    /// работал корректно. `cube_colored` — тот же куб, но с ОДНИМ,
    /// заданным вызывающим кодом цветом на все 6 граней (белый/серый —
    /// нейтральное альбедо, реально показывающее, как выглядит объект
    /// под настоящим освещением, а не отладочную раскраску граней).
    /// ДОБАВЛЕНО (максимальная графика — LOD): "плоская плитка" — РОВНО
    /// верхняя грань `cube_colored` ниже (те же 4 вершины/нормаль (0,1,0)/
    /// порядок индексов — скопировано буквально из её вершин 8,9,10,11 и
    /// индексов 8,10,9,9,10,11, просто без остальных 5 граней), 2
    /// треугольника вместо 12. Нужна для объектов, у которых снизу/сбоку
    /// почти никогда не видно (например плитки плоского пола вдалеке — см.
    /// `add_lod_group` в main.rs) — LOD-уровень со СТРОГО такой же мировой
    /// высотой верхней грани, что и у полного куба того же `size` при
    /// одинаковом масштабе инстанса (та же формула `half = size/2`,
    /// поэтому переключение LOD не даёт видимого "скачка"/шва по высоте).
    /// Тот же принцип, что и у остального LOD в этом движке — упрощённая
    /// геометрия готовится заранее (здесь процедурно), не runtime-decimation.
    pub fn tile(size: f32, r: f32, g: f32, b: f32, a: f32) -> Result<Self> {
        let half = size / 2.0;
        let vertices = [
            Vertex::with_normal_uv(-half, half, -half, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, half, -half, 0.0, 1.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half, half,  half, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half, half,  half, 0.0, 1.0, 0.0, r, g, b, a, 1.0, 0.0),
        ];
        let indices = [0, 2, 1, 1, 2, 3];
        Self::from_vertices_and_indices(&vertices, &indices)
    }

    pub fn cube_colored(size: f32, r: f32, g: f32, b: f32, a: f32) -> Result<Self> {
        let half = size / 2.0;
        let vertices = [
            Vertex::with_normal_uv(-half, -half, half, 0.0, 0.0, 1.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, -half, half, 0.0, 0.0, 1.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half,  half, half, 0.0, 0.0, 1.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half, half, 0.0, 0.0, 1.0, r, g, b, a, 1.0, 0.0),
            Vertex::with_normal_uv(-half, -half, -half, 0.0, 0.0, -1.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, -half, -half, 0.0, 0.0, -1.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half,  half, -half, 0.0, 0.0, -1.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half, -half, 0.0, 0.0, -1.0, r, g, b, a, 1.0, 0.0),
            Vertex::with_normal_uv(-half,  half, -half, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half,  half, -half, 0.0, 1.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half,  half,  half, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half,  half, 0.0, 1.0, 0.0, r, g, b, a, 1.0, 0.0),
            Vertex::with_normal_uv(-half, -half, -half, 0.0, -1.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, -half, -half, 0.0, -1.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half, -half,  half, 0.0, -1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half, -half,  half, 0.0, -1.0, 0.0, r, g, b, a, 1.0, 0.0),
            Vertex::with_normal_uv( half, -half, -half, 1.0, 0.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half,  half, -half, 1.0, 0.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv( half, -half,  half, 1.0, 0.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half,  half, 1.0, 0.0, 0.0, r, g, b, a, 1.0, 0.0),
            Vertex::with_normal_uv(-half, -half, -half, -1.0, 0.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv(-half,  half, -half, -1.0, 0.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half, -half,  half, -1.0, 0.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(-half,  half,  half, -1.0, 0.0, 0.0, r, g, b, a, 1.0, 0.0),
        ];

        let indices = [
            0,1,2, 1,3,2,
            4,6,5, 5,6,7,
            8,10,9, 9,10,11,
            12,13,14, 13,15,14,
            16,18,17, 17,18,19,
            20,21,22, 21,23,22,
        ];

        Self::from_vertices_and_indices(&vertices, &indices)
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — текстуры вместо плоской
    /// заливки, см. `AlkashEngine::create_texture_rgba`/
    /// `src/proc_textures.rs`): текстурированный параллелепипед, где UV
    /// считается по МИРОВЫМ единицам (`uv_scale` = сколько повторов
    /// текстуры укладывается на один метр), а не фиксированным 0..1 на
    /// грань, как у `cube_colored` выше — иначе вытянутый объект (стена,
    /// длинный борт кузова) растягивал бы текстуру на всю грань целиком,
    /// без повторов, что на глаз выглядит как размытое пятно, а не
    /// материал. Работает благодаря WRAP-адресации `material_sampler` в
    /// `create_root_signature` — UV здесь осознанно выходит за [0,1] на
    /// крупных гранях. `half_extents` — половины размеров по X/Y/Z в
    /// метрах, запечённые прямо в геометрию (в отличие от `cube_colored`,
    /// которому размер задаётся снаружи через `Transform.scale`) — так
    /// UV на каждой грани считается из ЧЕСТНОГО размера этой грани, без
    /// отдельного world-scale параметра, который легко забыть согласовать.
    pub fn box_textured(half_extents: [f32; 3], uv_scale: f32, tint: [f32; 4]) -> Result<Self> {
        let (hx, hy, hz) = (half_extents[0], half_extents[1], half_extents[2]);
        let [r, g, b, a] = tint;
        let uw = (2.0 * hx * uv_scale).max(0.01);
        let uh = (2.0 * hy * uv_scale).max(0.01);
        let ud = (2.0 * hz * uv_scale).max(0.01);

        let vertices = [
            // +Z (перед): ширина=uw(X), высота=uh(Y)
            Vertex::with_normal_uv(-hx, -hy, hz, 0.0, 0.0, 1.0, r, g, b, a, 0.0, uh),
            Vertex::with_normal_uv(hx, -hy, hz, 0.0, 0.0, 1.0, r, g, b, a, uw, uh),
            Vertex::with_normal_uv(-hx, hy, hz, 0.0, 0.0, 1.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(hx, hy, hz, 0.0, 0.0, 1.0, r, g, b, a, uw, 0.0),
            // -Z (зад)
            Vertex::with_normal_uv(-hx, -hy, -hz, 0.0, 0.0, -1.0, r, g, b, a, 0.0, uh),
            Vertex::with_normal_uv(hx, -hy, -hz, 0.0, 0.0, -1.0, r, g, b, a, uw, uh),
            Vertex::with_normal_uv(-hx, hy, -hz, 0.0, 0.0, -1.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(hx, hy, -hz, 0.0, 0.0, -1.0, r, g, b, a, uw, 0.0),
            // +Y (верх): ширина=uw(X), глубина=ud(Z)
            Vertex::with_normal_uv(-hx, hy, -hz, 0.0, 1.0, 0.0, r, g, b, a, 0.0, ud),
            Vertex::with_normal_uv(hx, hy, -hz, 0.0, 1.0, 0.0, r, g, b, a, uw, ud),
            Vertex::with_normal_uv(-hx, hy, hz, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(hx, hy, hz, 0.0, 1.0, 0.0, r, g, b, a, uw, 0.0),
            // -Y (низ)
            Vertex::with_normal_uv(-hx, -hy, -hz, 0.0, -1.0, 0.0, r, g, b, a, 0.0, ud),
            Vertex::with_normal_uv(hx, -hy, -hz, 0.0, -1.0, 0.0, r, g, b, a, uw, ud),
            Vertex::with_normal_uv(-hx, -hy, hz, 0.0, -1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(hx, -hy, hz, 0.0, -1.0, 0.0, r, g, b, a, uw, 0.0),
            // +X (право): ширина=ud(Z), высота=uh(Y)
            Vertex::with_normal_uv(hx, -hy, -hz, 1.0, 0.0, 0.0, r, g, b, a, 0.0, uh),
            Vertex::with_normal_uv(hx, hy, -hz, 1.0, 0.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(hx, -hy, hz, 1.0, 0.0, 0.0, r, g, b, a, ud, uh),
            Vertex::with_normal_uv(hx, hy, hz, 1.0, 0.0, 0.0, r, g, b, a, ud, 0.0),
            // -X (лево)
            Vertex::with_normal_uv(-hx, -hy, -hz, -1.0, 0.0, 0.0, r, g, b, a, ud, uh),
            Vertex::with_normal_uv(-hx, hy, -hz, -1.0, 0.0, 0.0, r, g, b, a, ud, 0.0),
            Vertex::with_normal_uv(-hx, -hy, hz, -1.0, 0.0, 0.0, r, g, b, a, 0.0, uh),
            Vertex::with_normal_uv(-hx, hy, hz, -1.0, 0.0, 0.0, r, g, b, a, 0.0, 0.0),
        ];

        // Тот же индексный паттерн (по граням), что и у `cube_colored` —
        // порядок/позиции вершин идентичны, изменились только UV.
        let indices = [
            0, 1, 2, 1, 3, 2,
            4, 6, 5, 5, 6, 7,
            8, 10, 9, 9, 10, 11,
            12, 13, 14, 13, 15, 14,
            16, 18, 17, 17, 18, 19,
            20, 21, 22, 21, 23, 22,
        ];

        Self::from_vertices_and_indices(&vertices, &indices)
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — текстуры): одна большая
    /// плоскость (XZ, нормаль +Y) с тайлящимся UV — компактная замена
    /// "сетке кубов-плиток" (см. исторический `setup_scene` в
    /// main_car.rs, GROUND_HALF=10 → 441 плитка → 441*4 draw call'а за
    /// кадр, см. подробный комментарий там же про TDR-краш от похожей
    /// проблемы) — один draw call вместо сотен при том же визуальном
    /// результате, благодаря WRAP-сэмплеру материала (см.
    /// `box_textured` выше про тот же приём).
    pub fn plane_textured(width: f32, depth: f32, uv_scale: f32, tint: [f32; 4]) -> Result<Self> {
        let hw = width * 0.5;
        let hd = depth * 0.5;
        let [r, g, b, a] = tint;
        let uw = (width * uv_scale).max(0.01);
        let ud = (depth * uv_scale).max(0.01);

        let vertices = [
            Vertex::with_normal_uv(-hw, 0.0, -hd, 0.0, 1.0, 0.0, r, g, b, a, 0.0, ud),
            Vertex::with_normal_uv(hw, 0.0, -hd, 0.0, 1.0, 0.0, r, g, b, a, uw, ud),
            Vertex::with_normal_uv(-hw, 0.0, hd, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(hw, 0.0, hd, 0.0, 1.0, 0.0, r, g, b, a, uw, 0.0),
        ];
        // Тот же локальный паттерн, что и верхняя грань `box_textured`
        // (0,2,1, 1,2,3) — совпадающая ориентация нормали +Y.
        let indices = [0, 2, 1, 1, 2, 3];

        Self::from_vertices_and_indices(&vertices, &indices)
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — настоящие колёса): цилиндр с
    /// осью вдоль локального X (совпадает со старым соглашением
    /// `spawn_physics_car` — там колесо-куб масштабировалось как
    /// `[wheel_width, wheel_radius*2, wheel_radius*2]`, то есть X = ось
    /// вращения) — в движке НЕТ примитива "цилиндр" (см. исторический
    /// комментарий у `spawn_physics_car`: "цилиндра в движке нет"), это
    /// первая процедурная реализация. Боковая поверхность — два кольца
    /// вершин (`segments+1` каждое — последняя вершина ДУБЛИРУЕТ первую с
    /// u=side_u_total вместо 0, иначе UV-шов "прыгал" бы с
    /// `side_u_total` обратно на 0 в одном треугольнике). Торцы — по
    /// одному treugольному вееру на каждый конец, с простой полярной UV-
    /// разверткой (центр = (0.5,0.5)) — для однотонной резиновой текстуры
    /// шва почти не видно, честная развёртка тут не нужна.
    pub fn cylinder_textured(radius: f32, width: f32, segments: u32, uv_scale: f32, tint: [f32; 4]) -> Result<Self> {
        let segments = segments.max(3);
        let hw = width * 0.5;
        let [r, g, b, a] = tint;
        let circumference = std::f32::consts::TAU * radius;
        let side_u_total = (circumference * uv_scale).max(0.01);
        let side_v = (width * uv_scale).max(0.01);

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let angle = t * std::f32::consts::TAU;
            let (cy, cz) = (angle.cos(), angle.sin());
            let (py, pz) = (cy * radius, cz * radius);
            let u = t * side_u_total;
            vertices.push(Vertex::with_normal_uv(-hw, py, pz, 0.0, cy, cz, r, g, b, a, u, side_v));
            vertices.push(Vertex::with_normal_uv(hw, py, pz, 0.0, cy, cz, r, g, b, a, u, 0.0));
        }
        for i in 0..segments {
            let base = i * 2;
            let (a0, b0, a1, b1) = (base, base + 1, base + 2, base + 3);
            indices.extend_from_slice(&[a0, a1, b0, b0, a1, b1]);
        }

        for &(x, normal_x, flip) in &[(-hw, -1.0f32, true), (hw, 1.0f32, false)] {
            let center_index = vertices.len() as u32;
            vertices.push(Vertex::with_normal_uv(x, 0.0, 0.0, normal_x, 0.0, 0.0, r, g, b, a, 0.5, 0.5));
            let rim_start = vertices.len() as u32;
            for i in 0..=segments {
                let t = i as f32 / segments as f32;
                let angle = t * std::f32::consts::TAU;
                let (cy, cz) = (angle.cos(), angle.sin());
                let (py, pz) = (cy * radius, cz * radius);
                let (u, v) = (0.5 + 0.5 * cy, 0.5 + 0.5 * cz);
                vertices.push(Vertex::with_normal_uv(x, py, pz, normal_x, 0.0, 0.0, r, g, b, a, u, v));
            }
            for i in 0..segments {
                let (ra, rb) = (rim_start + i, rim_start + i + 1);
                if flip {
                    indices.extend_from_slice(&[center_index, rb, ra]);
                } else {
                    indices.extend_from_slice(&[center_index, ra, rb]);
                }
            }
        }

        Self::from_vertices_and_indices(&vertices, &indices)
    }
}

#[derive(Debug, Clone)]
pub struct MeshInstance {
    pub mesh_index: usize,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl MeshInstance {
    pub fn new(mesh_index: usize) -> Self {
        Self {
            mesh_index,
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    pub fn at(mut self, x: f32, y: f32, z: f32) -> Self {
        self.position = [x, y, z];
        self
    }

    pub fn rotated(mut self, x: f32, y: f32, z: f32) -> Self {
        self.rotation = [x, y, z];
        self
    }

    pub fn scaled(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = [x, y, z];
        self
    }

    pub fn transform_matrix(&self) -> Mat4 {
        let translation = Mat4::from_translation(Vec3::new(
            self.position[0],
            self.position[1],
            self.position[2],
        ));

        let rot_z = Mat4::from_rotation_z(self.rotation[2]);
        let rot_y = Mat4::from_rotation_y(self.rotation[1]);
        let rot_x = Mat4::from_rotation_x(self.rotation[0]);
        let rotation = rot_z * rot_y * rot_x;

        let scale = Mat4::from_scale(Vec3::new(
            self.scale[0],
            self.scale[1],
            self.scale[2],
        ));

        translation * rotation * scale
    }
}
