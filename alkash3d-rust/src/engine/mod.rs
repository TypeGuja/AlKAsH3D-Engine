// src/engine/mod.rs
//! Основной движок Alkash3D

// ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload, см.
// подробности в шапке самого файла): единственный submodule engine/ —
// раньше вся логика жила прямо в этом mod.rs одним файлом.
mod scripting_python;
pub use scripting_python::PythonScriptRuntime;

// ДОБАВЛЕНО (рефакторинг по просьбе пользователя — mod.rs разросся до
// ~8400 строк): все impl AlkashEngine методы скриптинга (Native/Lua-DLL,
// Python hot-reload — load/dispatch/unload/update для всех трёх) и
// вспомогательные ScriptHandle/pack_entity_id вынесены в scripting.rs.
// mod.rs теперь только объявляет вызовы этих методов внутри
// AlkashEngine::update, сама реализация переехала целиком — см. подробный
// комментарий в шапке scripting.rs.
mod scripting;
pub use scripting::{ScriptHandle, pack_entity_id};

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};     // проблема по свету при повороте камеры
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::{DXGI_PRESENT, DXGI_SWAP_CHAIN_FLAG};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_UINT, DXGI_FORMAT_UNKNOWN};
use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_WINDOW, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::*;
use crate::plugin::{PhysicsPlugin, LightPlugin, PhysicsConfig, LightConfig, GPULight, PhysicsBody, PhysicsContact, PhysicsStats, LightGridCell, LightGridEntry, LightGridParams};
use crate::math::{Mat4, Vec3, identity, translation, rotation_x, rotation_y, rotation_z, scaling};
use crate::camera::Camera;
use crate::constant_buffer::TransformConstants;
use crate::shader::ShaderBlob;
use crate::pso::PipelineState;
use crate::input::InputState;

// ===================================================================
// Общий монотонно возрастающий счётчик значений fence для всего движка.
//
// ИСПРАВЛЕНО: раньше в `shutdown()` использовалось захардкоженное число
// `100` в качестве fence-значения для финальной синхронизации с GPU —
// ничем не гарантировано, что оно больше всех значений, уже отправленных
// в очередь к этому моменту. Теперь и `render_frame`, и `handle_resize`,
// и `shutdown` берут значения из одного общего счётчика, поэтому каждое
// следующее Signal() гарантированно больше предыдущих.
// ===================================================================
static NEXT_FENCE_VALUE: AtomicU64 = AtomicU64::new(1);

// ===================================================================
// ДОБАВЛЕНО (фикс "белого окна" + краша драйвера, реальный баг с живой
// машины пользователя): по всему движку было ТРИ места, ждущих GPU через
// `while fence.GetCompletedValue() < target { sleep(1ms) }` СОВСЕМ БЕЗ
// таймаута и без проверки, не потеряно ли устройство — в `render_frame()`
// (вызывается КАЖДЫЙ кадр, самое горячее место), в `handle_resize()` и
// (раньше) в `shutdown()`. Если GPU по любой причине перестаёт продвигать
// fence — реальный TDR, зависший драйвер, device removed — это условие
// `GetCompletedValue() < target` остаётся истинным НАВСЕГДА: значение
// физически больше никогда не изменится, GPU, который должен был бы его
// просигналить, уже не отвечает. Цикл крутится бесконечно на главном
// потоке — а именно в нём же крутится message loop (`process_messages()`
// в main.rs вызывается ИЗ ТОГО ЖЕ потока, ПЕРЕД `render_frame()`) — то
// есть окно перестаёт разбирать оконные сообщения совсем. Для Windows это
// выглядит как "приложение не отвечает": DWM рисует поверх окна белую
// заглушку, а любое взаимодействие пользователя (клик, попытка
// перетащить) только добавляет новые сообщения в очередь, которую никто
// не читает, и провоцирует систему считать процесс окончательно
// зависшим — вплоть до дополнительного давления на уже нестабильный
// видеодрайвер и полного его сброса.
//
// `wait_for_fence` — общая замена всем трём местам: ждёт максимум
// `timeout`, и на каждой итерации проверяет `device_removed_reason()` —
// если устройство уже потеряно, GPU просто никогда не досчитает fence
// (ждать дальше бессмысленно), поэтому выходим сразу с этой причиной,
// вместо того чтобы досиживать полный таймаут вслепую. Возвращает `Ok(())`
// как только `target` достигнут, `Err(String)` с причиной при таймауте или
// обнаруженной потере устройства — вызывающий код решает, что делать
// дальше (в `render_frame()`/`handle_resize()` это должно останавливать
// текущую операцию и давать `main.rs` шанс завершить цикл вместо
// зависания, а не паниковать и не пытаться рендерить дальше в мёртвое
// устройство).
fn wait_for_fence(fence: &ID3D12Fence, target: u64, timeout: std::time::Duration) -> std::result::Result<(), String> {
    if target == 0 {
        return Ok(());
    }
    let start = std::time::Instant::now();
    loop {
        let completed = unsafe { fence.GetCompletedValue() };
        if completed >= target {
            return Ok(());
        }
        if let Some(reason) = crate::device_removed_reason() {
            return Err(format!("device removed while waiting for fence (target={}, completed={}): {}", target, completed, reason));
        }
        if start.elapsed() > timeout {
            return Err(format!("timeout waiting for fence (target={}, completed={}, waited={:?})", target, completed, start.elapsed()));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

// ===================================================================
// Vertex definition for rendering
// ===================================================================

// ИСПРАВЛЕНО (Фаза 0 плана по реализму/фонарям): у Vertex раньше не было
// поля normal вообще — вершинный шейдер писал в output.normal константу
// float3(0,0,1) для АБСОЛЮТНО ЛЮБОЙ геометрии под любым углом. Это делало
// корректное освещение (в т.ч. от фонарей FirstFires) физически
// невозможным: dot(normal, lightDir) был одинаков везде, независимо от
// реальной ориентации поверхности. Теперь normal — часть вершинных данных,
// прогоняется через vertex buffer как есть.
// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы) — поле `uv`. Раньше
// вершина вообще не несла текстурных координат: любая закраска
// поверхности могла быть только однородным (per-face/per-vertex) цветом
// (см. `Mesh::cube`/`Mesh::cube_colored`), что делает невозможным нанести
// на геометрию реальную текстуру (кирпич, асфальт, металл фонаря и т.п.).
// `uv` идёт ПОСЛЕДНИМ полем (после position/normal/color) — так расширение
// стрйда не сдвигает офсеты уже существующих полей ни в Rust, ни в HLSL
// input layout (см. `PipelineState::create_graphics`/
// `create_shadow_pipeline_state` в engine/mod.rs — оба места объявляют
// POSITION@0/NORMAL@16/COLOR@28 и теперь дополнительно TEXCOORD0@44,
// используя ТОТ ЖЕ вершинный буфер с новым, увеличенным `Vertex::STRIDE`).
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
        // t = normalize(cross(up, n))
        let t = [
            up[1] * n[2] - up[2] * n[1],
            up[2] * n[0] - up[0] * n[2],
            up[0] * n[1] - up[1] * n[0],
        ];
        let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-6);
        [t[0] / len, t[1] / len, t[2] / len, 1.0]
    }
}

// ===================================================================
// Mesh - хранит геометрию
// ===================================================================

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
                // ДОБАВЛЕНО (Задача #15): UV — 8 байт вершины, офсет 44
                // (СРАЗУ после color@28+16=44), см. соответствующий
                // TEXCOORD0-элемент input layout в pso.rs::create_graphics/
                // create_shadow_pipeline_state.
                bytes.extend_from_slice(&v.uv[0].to_le_bytes());
                bytes.extend_from_slice(&v.uv[1].to_le_bytes());
                // ДОБАВЛЕНО (Задача #15, normal mapping): tangent — 16 байт,
                // офсет 52 (СРАЗУ после uv@44+8=52), см. TANGENT-элемент
                // input layout в pso.rs.
                bytes.extend_from_slice(&v.tangent[0].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[1].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[2].to_le_bytes());
                bytes.extend_from_slice(&v.tangent[3].to_le_bytes());
                bytes
            })
            .collect();

        let buffer = Buffer::create_vertex_buffer(&vertex_data, Vertex::STRIDE)?;

        // ДОБАВЛЕНО (оптимизация рендера — CPU-side frustum culling):
        // ограничивающая сфера меша в локальных координатах — считается
        // ОДИН РАЗ здесь (при создании меша), а не каждый кадр. AABB по
        // всем вершинам -> центр = середина AABB, радиус = максимальное
        // расстояние от центра до любой вершины (гарантированно содержит
        // ВСЕ вершины меша — консервативная, но корректная граница).
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
            // ДОБАВЛЕНО (Задача #15): по умолчанию без текстуры — вызывающий
            // код (например `AlkashEngine::load_object_mesh`) явно
            // проставляет реальный индекс ПОСЛЕ создания меша, если у
            // объекта есть albedo-текстура (см. `add_mesh`/`load_object_mesh`).
            albedo_srv_index: None,
            // ДОБАВЛЕНО (Задача #15, normal mapping): по умолчанию без карт
            // — та же логика, что у albedo_srv_index выше.
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

        // ИСПРАВЛЕНО (Фаза 0): у каждой грани куба теперь её РЕАЛЬНАЯ
        // внешняя нормаль (было: нормаль вообще не хранилась и в шейдере
        // подменялась на константу). Дублирование вершин по граням (24
        // вершины вместо 8) сохранено намеренно — это стандартный приём
        // именно для того, чтобы у каждой грани была своя normal
        // (у настоящего угла куба не может быть одной корректной нормали
        // сразу для трёх граней, поэтому общие вершины на гранях не
        // используются).
        let vertices = [
            // Front face (+Z)
            Vertex::with_normal(-half, -half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(-half,  half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            Vertex::with_normal( half,  half, half, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0),
            // Back face (-Z)
            Vertex::with_normal(-half, -half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal(-half,  half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half,  half, -half, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0),
            // Top face (+Y)
            Vertex::with_normal(-half,  half, -half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half, -half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal(-half,  half,  half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half,  half, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0),
            // Bottom face (-Y)
            Vertex::with_normal(-half, -half, -half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half, -half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal(-half, -half,  half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            Vertex::with_normal( half, -half,  half, 0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
            // Right face (+X)
            Vertex::with_normal( half, -half, -half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half, -half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half, -half,  half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            Vertex::with_normal( half,  half,  half, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0),
            // Left face (-X)
            Vertex::with_normal(-half, -half, -half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
            Vertex::with_normal(-half,  half, -half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
            Vertex::with_normal(-half, -half,  half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
            Vertex::with_normal(-half,  half,  half, -1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 1.0),
        ];

        let indices = [
            0,1,2, 1,3,2,  // front
            4,6,5, 5,6,7,  // back
            8,10,9, 9,10,11, // top
            12,13,14, 13,15,14, // bottom
            16,18,17, 17,18,19, // right
            20,21,22, 21,23,22, // left
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
    pub fn cube_colored(size: f32, r: f32, g: f32, b: f32, a: f32) -> Result<Self> {
        let half = size / 2.0;
        // ДОБАВЛЕНО (Задача #15): каждая грань теперь несёт стандартную
        // 0..1 UV-развёртку квада (тот же порядок вершин, что и раньше —
        // bottom-left, bottom-right, top-left, top-right — просто с
        // добавленными текстурными координатами) — нужна, чтобы
        // `cube_colored` (используется для тайлов пола/столбов в main.rs и
        // как placeholder-геометрия .altex-объектов в
        // `AlkashEngine::load_placeholder_mesh`) могла реально показать
        // albedo-текстуру, а не только сплошной цвет. `cube()` (отдельная,
        // отладочная функция с per-face цветами для main1.rs/main2.rs) UV
        // сознательно не получает — её роль — визуальная проверка нормалей,
        // не текстурирование.
        let vertices = [
            // Front face (+Z)
            Vertex::with_normal_uv(-half, -half, half, 0.0, 0.0, 1.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, -half, half, 0.0, 0.0, 1.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half,  half, half, 0.0, 0.0, 1.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half, half, 0.0, 0.0, 1.0, r, g, b, a, 1.0, 0.0),
            // Back face (-Z)
            Vertex::with_normal_uv(-half, -half, -half, 0.0, 0.0, -1.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, -half, -half, 0.0, 0.0, -1.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half,  half, -half, 0.0, 0.0, -1.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half, -half, 0.0, 0.0, -1.0, r, g, b, a, 1.0, 0.0),
            // Top face (+Y)
            Vertex::with_normal_uv(-half,  half, -half, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half,  half, -half, 0.0, 1.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half,  half,  half, 0.0, 1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half,  half, 0.0, 1.0, 0.0, r, g, b, a, 1.0, 0.0),
            // Bottom face (-Y)
            Vertex::with_normal_uv(-half, -half, -half, 0.0, -1.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half, -half, -half, 0.0, -1.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half, -half,  half, 0.0, -1.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half, -half,  half, 0.0, -1.0, 0.0, r, g, b, a, 1.0, 0.0),
            // Right face (+X)
            Vertex::with_normal_uv( half, -half, -half, 1.0, 0.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv( half,  half, -half, 1.0, 0.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv( half, -half,  half, 1.0, 0.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv( half,  half,  half, 1.0, 0.0, 0.0, r, g, b, a, 1.0, 0.0),
            // Left face (-X)
            Vertex::with_normal_uv(-half, -half, -half, -1.0, 0.0, 0.0, r, g, b, a, 0.0, 1.0),
            Vertex::with_normal_uv(-half,  half, -half, -1.0, 0.0, 0.0, r, g, b, a, 1.0, 1.0),
            Vertex::with_normal_uv(-half, -half,  half, -1.0, 0.0, 0.0, r, g, b, a, 0.0, 0.0),
            Vertex::with_normal_uv(-half,  half,  half, -1.0, 0.0, 0.0, r, g, b, a, 1.0, 0.0),
        ];

        let indices = [
            0,1,2, 1,3,2,  // front
            4,6,5, 5,6,7,  // back
            8,10,9, 9,10,11, // top
            12,13,14, 13,15,14, // bottom
            16,18,17, 17,18,19, // right
            20,21,22, 21,23,22, // left
        ];

        Self::from_vertices_and_indices(&vertices, &indices)
    }
}

// ===================================================================
// MeshInstance - экземпляр меша с трансформацией
// ===================================================================

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

        // Поворот в порядке ZYX (как в старом коде)
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

/// ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит на
/// месте" ПОСЛЕ фиксов стриминга/hot-reload/culling/физики): разбивка
/// времени `AlkashEngine::update()` по под-фазам — каждое поле хранит
/// ХУДШИЙ случай этой конкретной под-фазы за текущее 1-секундное окно
/// измерения (тот же принцип, что уже `max_update_ms`/`max_render_ms` в
/// bin/main.rs, но детальнее). `[PHYS-STATS]` уже показал, что сам
/// физический солвер спокоен во время спайков — то есть общий
/// `physics_ms` здесь тоже должен остаться низким, а виновная под-фаза
/// (скрипты/день-ночь/стриминг/каллинг света/аудио) станет видна прямым
/// сравнением чисел в логе, без дальнейших догадок по коду.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateBreakdownMs {
    pub physics_ms: f32,
    pub sync_physics_ms: f32,
    pub native_scripts_ms: f32,
    pub python_scripts_ms: f32,
    pub day_night_ms: f32,
    pub world_streaming_ms: f32,
    pub chunk_io_ms: f32,
    pub light_cull_ms: f32,
    pub audio_ms: f32,
}

// ===================================================================
// Main Engine - С ВСТРОЕННЫМ ОКНОМ
// ===================================================================

pub struct AlkashEngine {
    // Рендеринг
    pub renderer: Option<Renderer>,
    pub meshes: Vec<Mesh>,
    pub mesh_instances: Vec<MeshInstance>,
    pub root_signature: Option<ID3D12RootSignature>,
    pub pipeline_state: Option<ID3D12PipelineState>,
    pub vs: Option<ShaderBlob>,
    pub ps: Option<ShaderBlob>,

    // 3D рендеринг
    pub camera: Camera,
    pub constant_buffer: Option<Buffer>,
    pub transform_constants: TransformConstants,
    /// ДОБАВЛЕНО: сколько СЛОТОВ трансформаций (на один back buffer)
    /// вмещает текущий `constant_buffer`. Буфер реально в 2 раза больше
    /// (по одному набору слотов на каждый из двух back buffer'ов) — см.
    /// `ensure_constant_buffer_capacity`.
    constant_buffer_capacity: usize,

    // Планировщик
    pub scheduler: Arc<EngineScheduler>,

    /// ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит на
    /// месте"): худшее время каждой под-фазы `update()` за текущее
    /// 1-секундное окно измерения — см. макрос `timed!` внутри `update()`
    /// и `AlkashEngine::take_update_breakdown` (публичный геттер+сброс
    /// для bin/main.rs).
    update_breakdown_ms: UpdateBreakdownMs,

    // Плагины
    pub physics: Option<PhysicsPlugin>,
    pub lights: Option<LightPlugin>,

    /// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): в отличие от
    /// `physics`/`lights`, звук — НЕ внешний plugin DLL (на диске
    /// пользователя нет отдельного готового аудио-плагина, см. подробный
    /// комментарий в начале audio.rs) — `AudioEngine` создаётся напрямую
    /// через системный XAudio2. `None` до вызова `init_audio()`, тот же
    /// принцип "подсистема опциональна, инициализация явная", что и у
    /// physics/lights — приложение (main.rs) решает, нужен ли звук в
    /// конкретном запуске, движок не навязывает его.
    pub audio: Option<AudioEngine>,

    /// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины): в
    /// отличие от `physics`/`lights` (singleton — один плагин своего типа
    /// на весь движок), скриптовых DLL может быть загружено НЕСКОЛЬКО
    /// одновременно, и одна и та же DLL может обслуживать несколько
    /// сущностей (см. подробный комментарий в
    /// `plugin/scripting_api.rs`/`plugin/mod.rs::ScriptingPlugin`). Ключ —
    /// путь к DLL, тот же, что использовался при `load_native_script`
    /// (позволяет узнать, уже ли загружена конкретная DLL, не грузя её
    /// повторно).
    pub native_scripts: std::collections::HashMap<String, crate::plugin::ScriptingPlugin>,

    /// Все текущие живые прикрепления скриптов к сущностям — обновляются
    /// КАЖДЫЙ кадр в `update()` (см. `update_native_scripts`). Хранится
    /// отдельно от `native_scripts` (которое хранит САМИ DLL, а не их
    /// прикрепления к конкретным entity) по той же причине, по которой
    /// `physics_links` хранится отдельно от `physics`.
    pub active_scripts: Vec<ScriptHandle>,

    /// Монотонный счётчик кадров для `ScriptContext::frame_number` —
    /// нужен скриптам, которым важно различать "первый кадр после
    /// create_script" от последующих, или делать что-то раз в N кадров
    /// без своего собственного таймера на стороне DLL.
    ///
    /// ИЗМЕНЕНО (рефакторинг — вынос скриптинга в engine/scripting.rs):
    /// `pub(super)` вместо приватного — `update_native_scripts` (теперь в
    /// scripting.rs, отдельном подмодуле) читает и увеличивает это поле;
    /// см. подробное объяснение `pub(super)` у `deps_fallback_path` выше.
    pub(super) script_frame_counter: u64,

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload): в
    /// отличие от `native_scripts`/`active_scripts` (Native/Lua — общий
    /// DLL-плагинный путь через `ScriptingPlugin`/`ScriptHandle`), Python
    /// работает БЕЗ DLL — каждое прикрепление это просто
    /// `PythonScriptRuntime` (собственный Python-scope + путь к .py-файлу
    /// + mtime для hot-reload), живущий прямо здесь. Ключ — `EntityId`
    /// владельца: в отличие от Native/Lua, где несколько разных сущностей
    /// МОГУТ делить одну DLL, каждой Python-сущности всегда соответствует
    /// РОВНО одно прикрепление (упрощение первой версии — если понадобится
    /// несколько .py-скриптов на одной сущности одновременно, ключ надо
    /// будет расширить до (EntityId, script_slot)).
    pub python_scripts: std::collections::HashMap<crate::scene::EntityId, PythonScriptRuntime>,

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): связь физического
    /// тела (id, присвоенный плагином Inertial через `add_body`/
    /// `add_sphere_body`) с визуальной ECS-сущностью, чью `Transform`
    /// нужно КАЖДЫЙ кадр обновлять её текущей позицией — см.
    /// `sync_physics_transforms()`, вызывается из `update()` сразу после
    /// `physics.update(dt, gravity)`. Без этой связи физика считалась бы
    /// "в вакууме" (тела падают/сталкиваются внутри плагина), но экран
    /// показывал бы неподвижную геометрию — ровно тот же класс разрыва
    /// между "модель" и "представление", которого не было бы, храни
    /// движок позицию ТОЛЬКО в Transform или ТОЛЬКО в PhysicsBody, но не в
    /// обоих местах сразу. `Vec`, а не `HashMap<i32, EntityId>` — типичное
    /// число физических тел в сцене (десятки-сотни, не миллионы), линейный
    /// проход по нему каждый кадр дешевле, чем поддержка хэш-карты, и
    /// проще для отладки (порядок вставки сохраняется).
    pub physics_links: Vec<(i32, crate::scene::EntityId)>,

    // Окно
    hwnd: Option<HWND>,
    running: bool,

    // Настройки
    width: u32,
    height: u32,
    clear_color: [f32; 4],

    shutdown_in_progress: bool,

    /// ДОБАВЛЕНО (фикс краша видеодрайвера при смене разрешения окна):
    /// `true` между WM_ENTERSIZEMOVE и WM_EXITSIZEMOVE — то есть пока
    /// пользователь реально держит зажатой мышь на рамке/заголовке окна
    /// (перетаскивание для ресайза ИЛИ перемещения). Windows шлёт WM_SIZE
    /// на КАЖДЫЙ промежуточный кадр перетаскивания рамки (не один раз в
    /// конце) — а `handle_resize()` это тяжёлая операция: ждёт GPU idle,
    /// дропает весь Renderer (RTV/DSV/HDR/SRV-хипы), вызывает
    /// `ResizeBuffers`, пересоздаёт Renderer заново. Без throttling'а это
    /// повторялось бы ДЕСЯТКИ раз в секунду, пока пользователь тянет
    /// границу окна — на слабом железе (10-летний минимум, под который
    /// целится движок) это регулярно превышало таймаут Timeout Detection
    /// & Recovery видеодрайвера (обычно ~2 секунды непрерывной занятости
    /// GPU без Present) и вызывало сброс драйвера/перезагрузку. Пока
    /// `resizing_live == true`, WM_SIZE только запоминает целевой размер в
    /// `pending_resize`, реальный `handle_resize()` откладывается до
    /// WM_EXITSIZEMOVE (окно отпущено) — ОДИН тяжёлый ресайз на весь жест
    /// перетаскивания вместо одного на каждый промежуточный пиксель.
    resizing_live: bool,
    /// Последний размер, полученный через WM_SIZE во время live-resize
    /// (`resizing_live == true`), которому ещё не соответствовал реальный
    /// `handle_resize()` — применяется одним вызовом в WM_EXITSIZEMOVE.
    /// `None`, если во время текущего жеста перетаскивания размер ни разу
    /// не менялся (например, пользователь просто перемещал окно, а не
    /// тянул за рамку) — тогда WM_EXITSIZEMOVE ничего не делает.
    pending_resize: Option<(u32, u32)>,

    // ИСПРАВЛЕНО: значение fence, при достижении которого GPU закончил
    // последнее использование ресурсов, привязанных к данному back
    // buffer'у (индекс = индекс back buffer'а). 0 означает "ещё не
    // использовался". Используется, чтобы не блокировать CPU сразу после
    // каждого Present (см. комментарий в render_frame).
    frame_fence_values: Vec<u64>,

    /// ДОБАВЛЕНО: см. input.rs. Заполняется движком из оконных сообщений
    /// (wndproc), читается игровым кодом. Движок сам решений о том, что
    /// делать с вводом (двигать камеру, закрывать окно и т.п.), НЕ
    /// принимает — это отдано на откуп приложению (main.rs).
    pub input: InputState,

    /// ДОБАВЛЕНО: ECS-ядро сцены (см. scene.rs). Работает ПАРАЛЛЕЛЬНО со
    /// старым `mesh_instances` — ничего не меняет в поведении существующих
    /// main.rs/main1.rs/main2.rs, которые продолжают использовать
    /// `mesh_instances` напрямую. `render_frame()` рендерит содержимое
    /// `scene` ДОПОЛНИТЕЛЬНО к `mesh_instances`, если в сцене есть живые
    /// сущности.
    pub scene: crate::scene::Scene,

    /// ДОБАВЛЕНО (Фаза 1 плана по реализму/фонарям): глобальные настройки
    /// освещения из последнего загруженного .alfar (ambient-цвет/яркость,
    /// shadow_quality, bloom_intensity, exposure, gamma и т.п. — см.
    /// alfar_format::GlobalLightSettings/AmbientLight). Пока ничего в
    /// рендере эти поля ещё не читает (это будущие Фазы 5/6/8 плана —
    /// HDR/bloom, тени, volumetrics) — сохраняются здесь уже сейчас, чтобы
    /// `load_lights_from_alfar` не приходилось переписывать при подключении
    /// каждой следующей фазы.
    pub light_ambient: Option<crate::alfar_format::AmbientLight>,
    pub light_global_settings: Option<crate::alfar_format::GlobalLightSettings>,

    /// ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): GPU-буфер
    /// (StructuredBuffer<GPULight>, register t0), в который каждый кадр
    /// копируется список УЖЕ ОТКУЛЛЕННЫХ (видимых после LOD/дистанции/
    /// фрустума) фонарей от FirstFires (`LightPlugin::get_gpu_lights()`).
    /// До этой фазы список фонарей существовал только на CPU-стороне —
    /// пиксельный шейдер вообще не имел к нему доступа.
    light_buffer: Option<Buffer>,
    /// Сколько GPULight-слотов сейчас реально вмещает `light_buffer` (в
    /// элементах, не в байтах) — растёт по требованию, аналогично
    /// `constant_buffer_capacity`.
    light_buffer_capacity: usize,

    /// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): GPU-буферы под
    /// пространственную сетку FirstFires (StructuredBuffer<LightGridCell>
    /// t1 и StructuredBuffer<LightGridEntry> t2) — позволяют пиксельному
    /// шейдеру находить фонари СВОЕЙ ячейки вместо перебора всего видимого
    /// списка на каждый пиксель (см. render_frame). Число ячеек в сетке
    /// FirstFires (`grid_width*height*depth`) фиксировано на весь срок
    /// жизни LightPlugin (задаётся один раз в LightConfig при
    /// `init_lights`), поэтому размер этого буфера НЕ растёт по кадрам, в
    /// отличие от `light_buffer`/`grid_entries_buffer` — выделяется один
    /// раз при первом кадре после инициализации света.
    grid_cells_buffer: Option<Buffer>,
    grid_cells_buffer_capacity: usize,
    /// Число entries растёт по кадрам (зависит от того, сколько фонарей
    /// реально видимо и в каких ячейках) — тот же паттерн роста, что и у
    /// light_buffer.
    grid_entries_buffer: Option<Buffer>,
    grid_entries_buffer_capacity: usize,

    // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): отдельный, второй PSO
    // и root signature для composite/tonemap-прохода — рисует один
    // fullscreen-треугольник (без вершинного/индексного буфера, координаты
    // генерируются прямо в вершинном шейдере по SV_VertexID — стандартный
    // приём для fullscreen-проходов, не требующий отдельной геометрии),
    // читает HDR render target через SRV и пишет tonemapped-результат в
    // реальный back buffer. Полностью отдельная пара PSO/root signature от
    // основного 3D-прохода (`pipeline_state`/`root_signature` выше) —
    // разные шейдеры, разный input layout (никакого), разные ресурсы.
    tonemap_vs: Option<ShaderBlob>,
    tonemap_ps: Option<ShaderBlob>,
    tonemap_root_signature: Option<ID3D12RootSignature>,
    tonemap_pipeline_state: Option<ID3D12PipelineState>,
    /// Константы экспозиции для tonemap-прохода — по умолчанию exposure=1.0,
    /// обновляется из `.alfar` GlobalLightSettings.exposure при загрузке
    /// сцены (см. `load_lights_from_alfar` — light_global_settings).
    tonemap_constant_buffer: Option<Buffer>,

    // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, продолжение — bloom):
    // "Bright-pass extract" + разделяемый (separable) Gaussian blur в
    // half-resolution (по ширине/высоте вдвое меньше основного разрешения
    // — обычная практика: свечение фонарей по природе низкочастотное, блюр
    // на полном разрешении был бы заметно дороже без видимой разницы в
    // качестве). Полностью через ГРАФИЧЕСКИЙ (не compute) пайплайн — те же
    // fullscreen-triangle PSO/root-signature паттерны, что уже введены для
    // tonemap-прохода выше, а не отдельная compute-инфраструктура (root
    // signature типа COMPUTE, UAV-байндинг, D3D12_COMPUTE_PIPELINE_STATE_DESC
    // — которой в движке пока нет вообще). Три логических шейдера
    // (extract/blur используют РАЗНЫЕ PSO с одной и той же root signature —
    // у всех них одинаковый вход "1 текстура + сэмплер", отличается только
    // пиксельный шейдер):
    //  1. bloom_extract: HDR target -> яркие пиксели (порог) в bloom_a (half-res)
    //  2. bloom_blur_h:   bloom_a -> bloom_b (горизонтальный проход)
    //  3. bloom_blur_v:   bloom_b -> bloom_a (вертикальный проход, результат
    //     остаётся в bloom_a — его читает финальный tonemap composite)
    bloom_extract_ps: Option<ShaderBlob>,
    bloom_blur_ps: Option<ShaderBlob>,
    /// Общая root signature для extract/blur — один SRV (t0) + сэмплер +
    /// один CBV (b0) с параметрами (порог для extract, направление блюра
    /// для blur — оба варианта используют одну и ту же по форме структуру
    /// параметров, чтобы не плодить лишние root signatures).
    bloom_root_signature: Option<ID3D12RootSignature>,
    bloom_extract_pipeline_state: Option<ID3D12PipelineState>,
    bloom_blur_pipeline_state: Option<ID3D12PipelineState>,
    bloom_params_buffer: Option<Buffer>,
    /// Half-res ping-pong render target A — хранится вне `Renderer`
    /// (в отличие от `hdr_target`), т.к. это внутренняя деталь именно
    /// bloom-прохода, а не что-то, что нужно основному 3D draw pass'у.
    bloom_texture_a: Option<crate::render::RenderTexture>,
    bloom_rtv_a: D3D12_CPU_DESCRIPTOR_HANDLE,
    bloom_srv_a_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    bloom_texture_b: Option<crate::render::RenderTexture>,
    bloom_rtv_b: D3D12_CPU_DESCRIPTOR_HANDLE,
    bloom_srv_b_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// Отдельные RTV heap (2 дескриптора: A, B) и SRV heap (2 дескриптора:
    /// A, B) под bloom-таргеты — тот же паттерн, что и `hdr_rtv_heap`/
    /// `srv_uav_heap` в Renderer, но здесь, а не там, т.к. размер
    /// bloom-таргетов (half-res) отличается от размера HDR-таргета и
    /// пересоздаётся вместе с ним при ресайзе окна в будущем.
    bloom_rtv_heap: Option<ID3D12DescriptorHeap>,
    bloom_srv_heap: Option<ID3D12DescriptorHeap>,
    /// ВАЖНО: реальное текущее состояние bloom_texture_a/b НАЧИНАЕТСЯ как
    /// RENDER_TARGET (см. `create_hdr_target` — все render targets в этом
    /// движке создаются именно в этом состоянии) и МЕНЯЕТСЯ каждый кадр
    /// bloom-проходом в `render_frame`. Без явного трекера пришлось бы
    /// либо угадывать состояние по номеру кадра (хрупко), либо каждый раз
    /// вставлять "safety"-барьеры с неверным StateBefore на первом кадре
    /// (что валидатор D3D12 debug layer справедливо считает ошибкой).
    /// `true` = сейчас PIXEL_SHADER_RESOURCE, `false` = сейчас RENDER_TARGET.
    bloom_a_is_srv: bool,
    bloom_b_is_srv: bool,

    // =====================================================================
    // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени)
    // ОБНОВЛЕНО (каскадные тени / CSM — расширение Фазы 6, отложенное
    // улучшение, изначально анонсированное там же): раньше был ОДИН
    // shadow map, ортопроекция которого подгонялась под ВЕСЬ видимый
    // camera frustum сразу (near..far целиком). Один каскад на весь
    // диапазон обзора — компромисс: чтобы покрыть большую дальность
    // (нужно для открытого города), приходится либо снижать плотность
    // текселей на единицу площади (тени вблизи камеры становятся грубыми,
    // "лестничными"), либо повышать разрешение самой карты сверх разумного
    // (не помещается в бюджет памяти/производительности на зафиксированном
    // минимуме железа). Cascaded Shadow Maps (CSM) — стандартное решение:
    // НЕСКОЛЬКО shadow map, каждая покрывает свой диапазон дистанций от
    // камеры (ближний — маленький объём высокой чёткости, дальний —
    // большой объём низкой чёткости), пиксельный шейдер выбирает нужный
    // каскад по глубине пикселя от камеры. См. `NUM_CASCADES`/
    // `CASCADE_SPLITS` ниже и `compute_cascade_view_proj`.
    // =====================================================================
    /// Массив из `NUM_CASCADES` depth-таргетов (по одному на каскад) —
    /// см. подробное объяснение у `RenderTexture::create_shadow_map` в
    /// render.rs про TYPELESS-паттерн (одновременно DSV и SRV поверх
    /// одной и той же памяти). Хранятся здесь, а не в `Renderer`, по той
    /// же причине, что и bloom-текстуры выше: разрешение shadow map
    /// (фиксированное — SHADOW_MAP_RESOLUTION) НЕ зависит от размера
    /// окна, поэтому НЕ должны пересоздаваться при каждом resize (в
    /// отличие от hdr_target/depth_stencil внутри Renderer).
    ///
    /// Массив фиксированного размера (`[Option<T>; NUM_CASCADES]`), а не
    /// `Vec` — число каскадов известно на этапе компиляции (константа
    /// `NUM_CASCADES`) и не меняется в рантайме, `Vec` добавил бы только
    /// лишнее косвенное обращение к куче без реальной гибкости.
    shadow_maps: [Option<crate::render::RenderTexture>; NUM_CASCADES],
    shadow_dsv_heap: Option<ID3D12DescriptorHeap>,
    shadow_dsvs: [D3D12_CPU_DESCRIPTOR_HANDLE; NUM_CASCADES],
    /// Один SHADER_VISIBLE-хип на `NUM_CASCADES` СМЕЖНЫХ дескрипторов — SRV
    /// каждого каскада для сэмплирования в основном пиксельном шейдере (см.
    /// корневой параметр 4 = descriptor table SRV t3..t3+NUM_CASCADES-1 в
    /// `create_root_signature`). Отдельный от `renderer.srv_uav_heap` (тот
    /// используется tonemap-проходом, а shadow map читается ОСНОВНЫМ 3D
    /// draw pass'ом — разные корневые сигнатуры, разные точки бинда в
    /// кадре). Смежность ОБЯЗАТЕЛЬНА — тот же принцип, что и у HDR+bloom
    /// SRV в `renderer.srv_uav_heap` (см. `create_tonemap_root_signature`):
    /// одна descriptor table с одним диапазоном на NUM_CASCADES дескрипторов
    /// требует, чтобы все они лежали подряд в одном heap.
    shadow_srv_heap: Option<ID3D12DescriptorHeap>,
    /// GPU-адрес НАЧАЛА (индекс 0) descriptor table в `shadow_srv_heap` —
    /// шейдер видит t3=каскад0, t4=каскад1, t5=каскад2 (смежные
    /// дескрипторы, начиная с этого адреса).
    shadow_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// Отдельные root signature/PSO/шейдеры под shadow pass — ОБЩИЕ для
    /// ВСЕХ каскадов (проход рисует ТОЛЬКО глубину — нет PS вообще, нет
    /// UV/normal/color на выходе VS — идентичен для любого каскада,
    /// отличается только view-proj матрица и целевой DSV), поэтому не
    /// дублируются на каждый каскад отдельно. Не может переиспользовать
    /// `self.root_signature`/`self.pipeline_state` основного 3D-прохода
    /// (тот ожидает PS и output-цель RTVFormat = R16G16B16A16_FLOAT, а не
    /// депф-онли DSV).
    shadow_vs: Option<ShaderBlob>,
    shadow_root_signature: Option<ID3D12RootSignature>,
    shadow_pipeline_state: Option<ID3D12PipelineState>,
    /// ВАЖНО (как и bloom_a_is_srv/bloom_b_is_srv выше): явный трекер
    /// текущего состояния КАЖДОГО каскада по отдельности — избегаем no-op
    /// ResourceBarrier на первом кадре (создаются уже в DEPTH_WRITE, см.
    /// `create_shadow_map`) точно так же, как раньше пришлось чинить для
    /// bloom-таргетов (см. подробный комментарий про этот класс бага у
    /// bloom_a_is_srv). `true` = сейчас PIXEL_SHADER_RESOURCE (можно
    /// читать в основном PS), `false` = сейчас DEPTH_WRITE (можно писать
    /// shadow-проходом).
    shadow_maps_are_srv: [bool; NUM_CASCADES],
    /// Отдельный константный буфер под shadow-проход — ОБЩИЙ для всех
    /// каскадов (см. `constant_buffer::ShadowConstants` — одна матрица на
    /// слот, а не вся TransformConstants) и
    /// `ensure_shadow_constant_buffer_capacity`. Ёмкость теперь считается
    /// на `NUM_CASCADES` полных проходов по сцене за кадр (каждый
    /// объект рисуется в КАЖДЫЙ каскад отдельно — см. render_frame), а не
    /// один — иначе слотов не хватило бы уже на втором каскаде того же
    /// кадра. Тот же паттерн роста/удвоения на 2 back buffer'а, что и у
    /// `constant_buffer` основного прохода.
    shadow_constant_buffer: Option<Buffer>,
    shadow_constant_buffer_capacity: usize,
    /// ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-подсветка):
    /// SRV основного depth-таргета (`renderer.depth_stencil`) — нужен
    /// volumetric raymarch-проходу, чтобы восстанавливать мировую позицию
    /// каждого экранного пикселя (см. подробное обоснование у
    /// `RenderTexture::create_depth_stencil` в render.rs). Отдельный
    /// SHADER_VISIBLE-хип на 1 дескриптор, аналогично `shadow_srv_heap` —
    /// не добавляется в `renderer.srv_uav_heap`, так как читается ДРУГИМ
    /// проходом (volumetric, не tonemap composite) с другой root signature.
    depth_srv_heap: Option<ID3D12DescriptorHeap>,
    depth_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,

    // =====================================================================
    // ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и мерцание)
    // =====================================================================
    /// Текущее игровое время суток в часах, [0.0, 24.0) — 0 = полночь,
    /// 12 = полдень. Продвигается в `update_day_night` со скоростью
    /// `day_night_speed` часов игрового времени за одну РЕАЛЬНУЮ секунду.
    pub time_of_day: f32,
    /// Скорость течения времени суток (игровых часов в секунду). 1.0
    /// означает "полные сутки за 24 реальные секунды" — удобно для
    /// отладки/демонстрации; для обычной игры это будет намного меньше
    /// (например, 24.0/1200.0 — полные сутки за 20 реальных минут).
    /// Настраивается через `set_day_night_speed`, по умолчанию — 0 (время
    /// стоит на месте, пока приложение явно не включит смену дня/ночи —
    /// см. подробности выбора дефолта в `AlkashEngine::new`).
    pub day_night_speed: f32,
    /// ДОБАВЛЕНО: сохранённые исходные записи `IndividualLight` вместе с
    /// их id в FirstFires (возвращённым `add_light` в
    /// `load_lights_from_alfar`) — раньше `IndividualLight` конвертировался
    /// в `GPULight` и сразу выбрасывался, поэтому move/flicker/
    /// active_from/active_to были недостижимы после загрузки .alfar (эти
    /// поля попросту нигде не сохранялись). Без этого списка
    /// `update_day_night` не смог бы ни промодулировать мерцание, ни
    /// найти, какой GPULight в FirstFires нужно обновить через
    /// `LightPlugin::update_light`.
    managed_lights: Vec<ManagedLight>,
    /// Накопленная фаза шума мерцания на каждый управляемый источник —
    /// хранится отдельно от `managed_lights` (а не как поле в
    /// ManagedLight), чтобы не путать "статические данные из .alfar" с
    /// "runtime-состоянием, которое движок сам меняет каждый кадр".
    /// Индекс совпадает с индексом в `managed_lights`.
    flicker_phase: Vec<f32>,

    // =====================================================================
    // ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-подсветка)
    // =====================================================================
    /// Half-res (та же половина ширины/высоты, что и bloom-таргеты — свет,
    /// рассеянный в воздухе, по природе низкочастотный, полное разрешение
    /// не даёт заметной разницы в качестве, но заметно дороже) render
    /// target, в который raymarch-шейдер аккумулирует видимую вдоль луча
    /// камера->пиксель долю "солнечного" света (проверяя каждый шаг луча
    /// через shadow map — освещён ли он, или загорожен геометрией). Не
    /// ping-pong (в отличие от bloom_texture_a/b) — здесь нет отдельного
    /// blur-прохода, raymarch сам по себе уже даёт достаточно гладкий
    /// результат при разумном числе шагов, а half-res + bilinear-апскейл
    /// при финальном чтении в tonemap composite дополнительно сглаживает.
    volumetric_texture: Option<crate::render::RenderTexture>,
    volumetric_rtv: D3D12_CPU_DESCRIPTOR_HANDLE,
    volumetric_srv_gpu_final: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// Отдельные RTV heap (1 дескриптор — сам volumetric-таргет) и SRV heap
    /// (3 дескриптора: 0=depth, 1=shadow map, 2=volumetric-таргет для
    /// финального чтения tonemap-проходом) — volumetric raymarch-шейдеру
    /// нужны ОБА входа (depth + shadow map) одновременно в одной
    /// descriptor table, поэтому они должны быть смежными дескрипторами в
    /// ОДНОМ heap (то же требование D3D12, что уже объяснено у
    /// `create_bloom_resources` про смежность HDR/bloom SRV).
    volumetric_rtv_heap: Option<ID3D12DescriptorHeap>,
    volumetric_srv_heap: Option<ID3D12DescriptorHeap>,
    /// GPU-адрес НАЧАЛА descriptor table (depth, индекс 0) внутри
    /// `volumetric_srv_heap` — передаётся в SetGraphicsRootDescriptorTable
    /// для raymarch-прохода; шейдер видит depth как t0, shadow map как t1
    /// (смежный следующий дескриптор в том же heap).
    volumetric_srv_gpu_raymarch: D3D12_GPU_DESCRIPTOR_HANDLE,
    volumetric_vs: Option<ShaderBlob>,
    volumetric_ps: Option<ShaderBlob>,
    volumetric_root_signature: Option<ID3D12RootSignature>,
    volumetric_pipeline_state: Option<ID3D12PipelineState>,
    volumetric_constant_buffer: Option<Buffer>,
    /// Как и у bloom/shadow: явный трекер текущего resource state — та же
    /// защита от no-op ResourceBarrier на первом кадре (создаётся в
    /// RENDER_TARGET, см. `create_hdr_target`, которым переиспользуется
    /// реализация под volumetric-таргет). `true` = сейчас
    /// PIXEL_SHADER_RESOURCE, `false` = сейчас RENDER_TARGET.
    volumetric_is_srv: bool,
    /// ДОБАВЛЕНО (Фаза 8): раньше `renderer.depth_stencil` НИКОГДА не
    /// покидал DEPTH_WRITE за весь срок жизни движка (только писался и
    /// тестировался основным 3D-проходом, никогда не читался как SRV) —
    /// поэтому явного трекера состояния не требовалось. Volumetric
    /// raymarch-проходу нужно ПРОЧИТАТЬ его как SRV (см.
    /// `create_depth_srv_resources`/render_frame) — то же самое отслеживание
    /// состояния, что уже применяется к shadow_map/bloom_a/bloom_b выше,
    /// требуется и здесь: без него второй и последующие кадры не знали бы,
    /// что depth_stencil уже был возвращён в DEPTH_WRITE предыдущим кадром
    /// (тот же принцип, что и у `shadow_map_is_srv`).
    depth_stencil_is_srv: bool,

    /// ДОБАВЛЕНО (World Streaming — подключение .alworld к движку):
    /// текущий загруженный мир (метаданные — где какие чанки, размер
    /// чанка, streaming config) + рантайм-состояние стриминга (какие
    /// чанки сейчас реально загружены и какие сущности Scene им
    /// принадлежат). `None`, пока `load_world()` ни разу не вызывался —
    /// `update_world_streaming()` в этом случае безопасно ничего не
    /// делает (см. её реализацию), это НЕ ошибка — многие сцены
    /// (main.rs/main1.rs/main2.rs с одиночными кубами) вообще не
    /// используют мировой стриминг.
    pub world: Option<WorldStreamingState>,
    /// ДОБАВЛЕНО (World Streaming): кэш mesh_index placeholder-геометрии
    /// (единичный куб), используемой ТОЛЬКО как fallback, когда реальный
    /// `.altex` объекта чанка не удалось загрузить (файл отсутствует,
    /// повреждён, путь "placeholder" — см. `load_object_mesh`/
    /// `load_placeholder_mesh`).
    /// `None`, пока фолбэк ни разу не понадобился.
    world_chunk_placeholder_mesh: Option<usize>,
    /// ДОБАВЛЕНО (загрузчик .altex -> GPU Mesh): кэш "путь к .altex файлу
    /// -> список mesh_index уже загруженных GPU-мешей из него" — один и
    /// тот же .altex (например меш фонарного столба или типового здания)
    /// обычно используется МНОГИМИ объектами МНОГИХ чанков; без кэша
    /// каждое появление объекта в новом чанке заново парсило бы файл с
    /// диска и заново создавало бы идентичный GPU vertex/index buffer —
    /// расточительно и по CPU (парсинг), и по GPU-памяти (дублирующиеся
    /// буферы одной и той же геометрии). Список (а не один mesh_index),
    /// т.к. один .altex может содержать НЕСКОЛЬКО мешей (см.
    /// `AltexFile::meshes`) — здание может состоять из нескольких
    /// отдельных частей с разными материалами.
    altex_mesh_cache: std::collections::HashMap<String, Vec<usize>>,

    // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). albedo-текстуры
    // хранятся В ТОМ ЖЕ дескрипторном хипе, что и shadow map каскады
    // (`self.shadow_srv_heap`), НЕ в отдельном хипе — это НЕ опциональная
    // оптимизация, а требование самого D3D12: аппаратно можно
    // одновременно забиндить (`SetDescriptorHeaps`) не более ОДНОГО
    // shader-visible хипа типа CBV_SRV_UAV за раз. Главный 3D-проход уже
    // биндит `shadow_srv_heap` перед своим draw loop'ом (см. render_frame)
    // — если бы material-текстуры жили в СВОЁМ отдельном хипе, второй
    // вызов `SetDescriptorHeaps` внутри того же прохода молча заменил бы
    // привязку первого хипа, и shadow-семплирование сломалось бы (или
    // наоборот). Поэтому: индексы `[0..NUM_CASCADES)` внутри
    // `shadow_srv_heap` — shadow-каскады (без изменений), индексы
    // `[NUM_CASCADES..)` — material-текстуры (см. `ensure_material_srv_capacity`,
    // которая при росте хипа перерегистрирует И каскады, И все текстуры).
    /// Сколько СВЕРХ NUM_CASCADES material-слотов сейчас реально вмещает
    /// `shadow_srv_heap` (растёт степенями двойки, как и
    /// `light_buffer_capacity`, — не пересоздаётся на каждую новую
    /// текстуру).
    material_srv_capacity: u32,
    /// Сколько material-слотов реально ЗАНЯТО (следующий свободный —
    /// `NUM_CASCADES + material_texture_count`).
    material_texture_count: u32,
    /// Живые GPU-ресурсы текстур — должны жить как минимум столько же,
    /// сколько дескрипторы, на них ссылающиеся, поэтому хранятся здесь
    /// (в `AlkashEngine`), а не как временные локальные переменные в
    /// месте загрузки.
    material_textures: Vec<crate::texture::Texture>,
    /// Кэш "путь к текстуре -> её SRV-индекс (уже с учётом NUM_CASCADES-
    /// смещения)" — та же идея, что и `altex_mesh_cache` выше: одна и та
    /// же текстура (например обычный кирпич/асфальт) типично
    /// переиспользуется МНОГИМИ разными .altex-мешами/материалами,
    /// повторная загрузка и повторный SRV на каждое использование были
    /// бы расточительны.
    texture_cache: std::collections::HashMap<String, u32>,
    /// ДОБАВЛЕНО: индекс SRV (с учётом NUM_CASCADES-смещения) нейтральной
    /// белой 1x1 текстуры (albedo (1,1,1,1)) — создаётся ОДИН раз лениво
    /// при первом обращении (см. `ensure_white_texture`). Используется как
    /// fallback ВЕЗДЕ, где меш не имеет собственной albedo-текстуры
    /// (`Mesh::albedo_srv_index == None`) — избавляет пиксельный шейдер от
    /// отдельной HLSL-ветки "текстуры нет вообще" (см. main() в
    /// compile_default_shaders): умножение на (1,1,1,1) не меняет
    /// освещённый вершинный цвет, что в точности воспроизводит поведение
    /// движка ДО этой задачи (когда albedo-текстур не существовало
    /// вообще, работал только вершинный цвет).
    white_texture_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): индекс SRV нейтральной
    /// "плоской" normal map (128,128,255,255 — RGB-кодировка tangent-space
    /// вектора (0,0,1), т.е. "нормаль совпадает с геометрической, карта
    /// ничего не меняет") — тот же fallback-принцип, что и
    /// `white_texture_srv_index`, но для register t7 вместо t6.
    flat_normal_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): индекс SRV нейтральной
    /// metallic-roughness текстуры (dummy — реальные значения в этом
    /// случае приходят из root constants `Mesh::material_metallic`/
    /// `material_roughness`, см. `create_root_signature`; эта текстура
    /// нужна только чтобы register t8 указывал на ВАЛИДНЫЙ SRV, а не на
    /// неинициализированный слот кучи, когда у меша нет собственной MR-карты).
    neutral_mr_srv_index: Option<u32>,
}

/// ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и мерцание):
/// то подмножество полей `alfar_format::IndividualLight`, которое
/// `update_day_night` реально использует каждый кадр, плюс id,
/// присвоенный FirstFires при добавлении. Отдельная структура, а не
/// хранение самого `IndividualLight` — тот содержит поля (`custom_data_offset`,
/// `has_physics`, `breakable`, `health`, `name_id`), не имеющие отношения к
/// день/ночь и мерцанию, и не является Copy (хотя в данном случае это не
/// критично) — явный список нужных полей делает понятным, что именно эта
/// фаза реально использует.
#[derive(Debug, Clone, Copy)]
struct ManagedLight {
    /// id, под которым свет живёт внутри FirstFires (аргумент id для
    /// `LightPlugin::update_light`).
    firstfires_id: u32,
    /// Статическая (не меняющаяся во время работы) часть GPULight —
    /// пересобирается каждый кадр из этих полей + промодулированного
    /// intensity/enabled.
    position: [f32; 3],
    light_type: f32,
    color: [f32; 3],
    base_intensity: f32,
    direction: [f32; 3],
    range: f32,
    params: [f32; 4],
    // Мерцание (Фаза 7)
    flicker_enabled: bool,
    flicker_speed: f32,
    flicker_intensity: f32,
    // Активность по времени суток (Фаза 7). active_from/active_to — часы
    // [0,24). Если active_from > active_to, диапазон считается
    // "оборачивающимся через полночь" (например, 18.0..6.0 — с 18:00 до
    // 06:00 следующих суток), см. `ManagedLight::is_active_at`.
    active_from: f32,
    active_to: f32,
}

/// ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и мерцание):
/// результат `AlkashEngine::compute_sun_state` — направление/цвет/
/// интенсивность/ambient directional-света ("солнца") для заданного часа
/// суток. Отдельная небольшая структура, а не кортеж — имена полей вместо
/// позиционных .0/.1/.2/.3 делают вызывающий код (`update_day_night`)
/// читаемым.
struct SunState {
    /// Направление, КУДА летит свет (как `TransformConstants.light_dir`) —
    /// НЕ позиция солнца на небе, а противоположность ей.
    direction: Vec3,
    color: [f32; 3],
    intensity: f32,
    ambient: [f32; 3],
}

impl ManagedLight {
    /// true, если источник должен быть включён в момент времени `hour`
    /// (часы, [0,24)). active_from == active_to трактуется как "всегда
    /// включён" (полный диапазон в 24 часа) — иначе диапазон нулевой
    /// длины никогда не был бы активен, что почти наверняка не то, что
    /// имел в виду автор сцены, оставивший оба поля равными (например,
    /// 0.0/0.0 — частый дефолт "не задано").
    fn is_active_at(&self, hour: f32) -> bool {
        if self.active_from == self.active_to {
            return true;
        }
        if self.active_from < self.active_to {
            hour >= self.active_from && hour < self.active_to
        } else {
            // Оборачивается через полночь: активен либо после active_from,
            // либо до active_to.
            hour >= self.active_from || hour < self.active_to
        }
    }
}

/// Разрешение shadow map directional-света в тексселях на сторону — см.
/// подробное обоснование выбора в `RenderTexture::create_shadow_map`
/// (render.rs). Константа, а не поле — единственный источник истины,
/// используемый и при создании ресурса, и при записи
/// `TransformConstants::shadow_map_size` (шейдеру нужно знать точный
/// размер текселя для шага PCF-сэмплирования).
pub const SHADOW_MAP_RESOLUTION: u32 = 2048;

/// ДОБАВЛЕНО (каскадные тени / CSM — расширение Фазы 6): число каскадов.
/// 3 — стандартный практический компромисс между качеством и стоимостью
/// (больше каскадов даёт более плавный переход плотности текселей с
/// дистанцией, но линейно увеличивает стоимость shadow-прохода — сцена
/// рисуется в КАЖДЫЙ каскад отдельно, см. render_frame). На
/// зафиксированном минимуме железа (i3-12100F/RTX 3050 8GB) 3 полных
/// depth-only прохода по сцене за кадр — разумный бюджет, 4+ уже
/// заметно дороже без пропорционального выигрыша в качестве для города
/// умеренной плотности застройки.
pub const NUM_CASCADES: usize = 3;

/// ДОБАВЛЕНО (каскадные тени / CSM): границы каскадов в ЕДИНИЦАХ ДОЛИ
/// camera.far (не абсолютные метры — далность обзора камеры может
/// меняться, доля пересчитывается в метры на лету в
/// `compute_cascade_view_proj`). Не равномерное деление (0.33/0.66/1.0),
/// а логарифмически-смещённое к камере распределение — воспринимаемая
/// плотность текселей падает с дистанцией нелинейно (объекты вблизи
/// камеры занимают на экране НАМНОГО больше пикселей на единицу мировой
/// длины, чем далёкие), поэтому ближний каскад сознательно ýже (покрывает
/// меньшую долю дальности), а дальний — шире. Стандартная практика CSM
/// (см. например Microsoft DirectX SDK "Cascaded Shadow Maps" sample).
pub const CASCADE_SPLITS: [f32; NUM_CASCADES] = [0.08, 0.25, 1.0];

// =============================================================================
// ДОБАВЛЕНО (World Streaming — подключение .alworld к движку): рантайм-
// состояние стриминга открытого мира.
//
// Идея (стандартный подход открытых миров — Cities: Skylines, GTA-подобные
// движки): мир разбит на СЕТКУ ЧАНКОВ фиксированного размера
// (`AlworldFile::header.chunk_size` метров). На диске лежит только
// МЕТАДАННЫЕ мира (`AlworldFile` — где какие чанки, что в них ПРИМЕРНО
// есть по счётчикам objects_count/lights_count) — содержимое каждого
// чанка (реальные объекты, см. `ChunkContent` в alworld_format.rs) хранится
// в ОТДЕЛЬНОМ файле НА ДИСКЕ, читается только когда камера подходит
// достаточно близко (`load_distance`), и выгружается из памяти обратно,
// когда камера отходит достаточно далеко (`unload_distance`). Это и
// позволяет городу быть СКОЛЬ УГОДНО большим — в памяти в любой момент
// держится только окрестность игрока, а не весь мир целиком.
// =============================================================================

/// Рантайм-состояние ОДНОГО чанка — какие сущности Scene сейчас
/// представляют его содержимое (если он загружен). Отдельная от
/// `alworld_format::ChunkDescriptor` структура — та описывает чанк НА
/// ДИСКЕ (позиция, объём, где искать данные), эта — чанк В ПАМЯТИ ДВИЖКА
/// (что из него реально заспавнено в Scene прямо сейчас).
#[derive(Debug, Clone, Default)]
struct ChunkRuntimeState {
    /// Заспавненные `EntityId` объектов этого чанка — при выгрузке чанка
    /// каждый despawn'ится (см. `unload_chunk`). Пусто, пока чанк не
    /// загружен.
    spawned_entities: Vec<crate::scene::EntityId>,
    /// ДОБАВЛЕНО (объединённая сцена — физика из .alworld): id физических
    /// тел (`PhysicsPlugin::add_body`/`add_sphere_body`), созданных для
    /// объектов ЭТОГО чанка с флагом `CHUNK_OBJECT_FLAG_HAS_PHYSICS` (см.
    /// `load_chunk`). Отдельно от `spawned_entities` — физическое тело и
    /// визуальная ECS-сущность живут в РАЗНЫХ системах (плагин Inertial
    /// / `Scene`), и `unload_chunk` обязан почистить обе, иначе
    /// физическое тело осиротевшего объекта продолжало бы столкновения и
    /// тратить CPU-время в broad/narrow phase даже после того, как его
    /// чанк давно выгружен и визуально не существует — на большом
    /// открытом мире с активным стримингом это накапливалось бы без
    /// предела за время игровой сессии (утечка физических тел).
    spawned_physics_bodies: Vec<i32>,
    /// true, если чанк прямо сейчас считается загруженным (есть
    /// заспавненные сущности ИЛИ чанк пуст, но всё равно отмечен
    /// загруженным, чтобы не пытаться перезагружать его каждый кадр).
    loaded: bool,
    /// ИСПРАВЛЕНО (баг: "фризы как будто GC" при ходьбе — жалоба
    /// пользователя, см. `WorldStreamingState::pending_load`): true, если
    /// чанк уже поставлен в очередь `pending_load`/`pending_unload`, но
    /// ещё физически не обработан `drain_pending_chunk_io`. Нужно, чтобы
    /// `update_world_streaming` не добавил один и тот же чанк в очередь
    /// повторно при следующем пересчёте (каждые
    /// `WORLD_STREAMING_INTERVAL_FRAMES` кадров), пока он ещё ждёт своей
    /// очереди.
    queued: bool,
}

/// Полное рантайм-состояние стриминга — хранится в
/// `AlkashEngine::world`. Содержит и метаданные (`AlworldFile`), и
/// рантайм-карту "чанк -> что из него сейчас в Scene" (`chunk_states`,
/// индексируется ТЕМ ЖЕ индексом, что и `world_file.chunks` — оба Vec
/// всегда одной длины, поддерживается инвариантом в `load_world`).
pub struct WorldStreamingState {
    world_file: crate::alworld_format::AlworldFile,
    /// Директория, где лежат файлы содержимого чанков (`chunk_X_Y_Z.alwchunk`)
    /// — вычисляется от пути к .alworld при `load_world` (тот же каталог,
    /// подпапка "chunks").
    chunks_dir: std::path::PathBuf,
    chunk_states: Vec<ChunkRuntimeState>,
    /// Мировая позиция, для которой стриминг считался В ПОСЛЕДНИЙ РАЗ —
    /// используется, чтобы не пересчитывать дистанции до ВСЕХ чанков
    /// каждый кадр без надобности (см. `update_world_streaming` —
    /// пересчёт происходит, только если камера сдвинулась заметно, либо
    /// раз в несколько кадров — компромисс между отзывчивостью стриминга
    /// и CPU-стоимостью обхода потенциально тысяч чанков каждый кадр).
    last_streaming_origin: Vec3,
    /// Счётчик кадров с последнего полного пересчёта стриминга — см.
    /// `WORLD_STREAMING_INTERVAL_FRAMES`.
    frames_since_streaming_update: u32,
    /// Сколько чанков реально загружено прямо сейчас — для диагностики/
    /// логов, не участвует в логике загрузки напрямую.
    loaded_chunk_count: usize,
    /// ИСПРАВЛЕНО (баг: "фризы как будто GC" при ходьбе — жалоба
    /// пользователя): `load_chunk`/`unload_chunk` синхронно читают файл
    /// чанка с диска и парсят его ПРЯМО в кадре рендера (см. `load_chunk`
    /// — `ChunkContent::load_from_file` блокирующий). Раньше
    /// `update_world_streaming` находил ВСЕ чанки, попавшие в
    /// load_distance за один пересчёт, и грузил их ВСЕ в одном кадре — при
    /// первом входе в мир или после резкого скачка позиции камеры (или
    /// после нескольких пропущенных пересчётов, см.
    /// `WORLD_STREAMING_INTERVAL_FRAMES`) это могло быть сразу 10-30+
    /// чанков разом, что и ощущается как долгий стоп-пауза ("будто GC"),
    /// хотя в Rust нет GC — тормозит именно синхронный дисковый I/O кучей
    /// файлов подряд на одном кадре. Фикс — очередь: `update_world_streaming`
    /// теперь только ОПРЕДЕЛЯЕТ, какие чанки нужно загрузить/выгрузить, и
    /// складывает их сюда, а реальная загрузка размазывается по
    /// нескольким последующим кадрам с бюджетом `CHUNK_LOAD_BUDGET_PER_FRAME`
    /// чанков за кадр (см. `drain_pending_chunk_io`, вызывается из
    /// `update()` каждый кадр, а не только когда истёк интервал пересчёта).
    pending_load: Vec<usize>,
    pending_unload: Vec<usize>,
}

/// ДОБАВЛЕНО (World Streaming): стриминг пересчитывается не КАЖДЫЙ кадр, а
/// раз в это число кадров — обход дескрипторов тысяч чанков (вычисление
/// дистанции камера-чанк для каждого) на каждый ЕДИНСТВЕННЫЙ кадр был бы
/// заметной и совершенно ненужной тратой CPU-бюджета кадра: чанки размером
/// в десятки метров не требуют реакции быстрее нескольких кадров — камера
/// физически не успевает пересечь границу load/unload-дистанции за 1/60
/// секунды при разумных скоростях перемещения.
const WORLD_STREAMING_INTERVAL_FRAMES: u32 = 15;

/// ИСПРАВЛЕНО (фризы при стриминге, см. комментарий у `pending_load`
/// выше): сколько чанков максимум грузится/выгружается за ОДИН кадр из
/// накопленной очереди. 2 — консервативный выбор специально под "минимум
/// 10-летнее железо" из ТЗ (медленный HDD/eMMC на такой машине делает
/// даже один синхронный файловый I/O заметным на кадре) — при обычной
/// ходьбе очередь почти всегда пуста или содержит 1 чанк за раз, бюджет
/// реально ограничивает только "взрывные" случаи (первый вход в мир,
/// резкий скачок позиции камеры, телепорт).
const CHUNK_LOAD_BUDGET_PER_FRAME: usize = 2;

impl AlkashEngine {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            scheduler: Arc::new(EngineScheduler::new()),
            update_breakdown_ms: UpdateBreakdownMs::default(),
            renderer: None,
            meshes: Vec::new(),
            mesh_instances: Vec::new(),
            root_signature: None,
            pipeline_state: None,
            vs: None,
            ps: None,
            camera: Camera::new(width, height),
            constant_buffer: None,
            transform_constants: TransformConstants::new(),
            constant_buffer_capacity: 0,
            physics: None,
            lights: None,
            audio: None,
            native_scripts: std::collections::HashMap::new(),
            active_scripts: Vec::new(),
            script_frame_counter: 0,
            python_scripts: std::collections::HashMap::new(),
            physics_links: Vec::new(),
            hwnd: None,
            running: false,
            width,
            height,
            clear_color: [0.05, 0.05, 0.1, 1.0],
            shutdown_in_progress: false,
            resizing_live: false,
            pending_resize: None,
            frame_fence_values: vec![0, 0],
            scene: crate::scene::Scene::new(),
            input: InputState::new(),
            light_ambient: None,
            light_global_settings: None,
            light_buffer: None,
            light_buffer_capacity: 0,
            grid_cells_buffer: None,
            grid_cells_buffer_capacity: 0,
            grid_entries_buffer: None,
            grid_entries_buffer_capacity: 0,
            tonemap_vs: None,
            tonemap_ps: None,
            tonemap_root_signature: None,
            tonemap_pipeline_state: None,
            tonemap_constant_buffer: None,
            bloom_extract_ps: None,
            bloom_blur_ps: None,
            bloom_root_signature: None,
            bloom_extract_pipeline_state: None,
            bloom_blur_pipeline_state: None,
            bloom_params_buffer: None,
            bloom_texture_a: None,
            bloom_rtv_a: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            bloom_srv_a_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            bloom_texture_b: None,
            bloom_rtv_b: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            bloom_srv_b_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            bloom_rtv_heap: None,
            bloom_srv_heap: None,
            // Обе текстуры создаются в RENDER_TARGET (см. create_hdr_target) —
            // is_srv = false до самого первого bloom-прохода.
            bloom_a_is_srv: false,
            bloom_b_is_srv: false,

            // [None; NUM_CASCADES] здесь недоступен — RenderTexture не Copy
            // (владеет ID3D12Resource), поэтому массив собирается явным
            // литералом. ВАЖНО: длина литерала должна вручную совпадать с
            // NUM_CASCADES — при изменении константы компилятор поймает
            // рассинхронизацию сам (E0308, ожидаемый размер массива), так
            // что молчаливого расхождения быть не может.
            shadow_maps: [None, None, None],
            shadow_dsv_heap: None,
            shadow_dsvs: [D3D12_CPU_DESCRIPTOR_HANDLE::default(); NUM_CASCADES],
            shadow_srv_heap: None,
            shadow_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            shadow_vs: None,
            shadow_root_signature: None,
            shadow_pipeline_state: None,
            // Каждый каскад создаётся уже в DEPTH_WRITE (см.
            // create_shadow_map) — is_srv = false для всех до самого
            // первого shadow-прохода.
            shadow_maps_are_srv: [false; NUM_CASCADES],
            shadow_constant_buffer: None,
            shadow_constant_buffer_capacity: 0,

            // Полдень по умолчанию — нейтральная стартовая точка, не
            // требующая от приложения сразу же вызывать set_time_of_day,
            // чтобы получить разумное освещение на первом кадре.
            time_of_day: 12.0,
            // 0 = время стоит на месте, пока приложение явно не вызовет
            // set_day_night_speed. Дефолт "время идёт само" удивил бы
            // существующие сцены/демки, которые ничего не знают про
            // Фазу 7 и не ожидают, что солнце будет двигаться само по
            // себе без явного запроса.
            day_night_speed: 0.0,
            managed_lights: Vec::new(),
            flicker_phase: Vec::new(),

            volumetric_texture: None,
            volumetric_rtv: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            volumetric_srv_gpu_final: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            volumetric_rtv_heap: None,
            volumetric_srv_heap: None,
            volumetric_srv_gpu_raymarch: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            volumetric_vs: None,
            volumetric_ps: None,
            volumetric_root_signature: None,
            volumetric_pipeline_state: None,
            volumetric_constant_buffer: None,
            // Создаётся в RENDER_TARGET (переиспользует create_hdr_target) —
            // is_srv = false до самого первого volumetric-прохода.
            volumetric_is_srv: false,
            // depth_stencil создаётся в DEPTH_WRITE (см. create_depth_stencil)
            // и ВСЕГДА возвращается туда до конца кадра (см. render_frame) —
            // is_srv = false изначально, как и у остальных трекеров.
            depth_stencil_is_srv: false,

            depth_srv_heap: None,
            depth_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),

            world: None,
            world_chunk_placeholder_mesh: None,
            altex_mesh_cache: std::collections::HashMap::new(),

            material_srv_capacity: 0,
            material_texture_count: 0,
            material_textures: Vec::new(),
            texture_cache: std::collections::HashMap::new(),
            white_texture_srv_index: None,
            flat_normal_srv_index: None,
            neutral_mr_srv_index: None,
        }
    }

    /// Останавливает игровой цикл (эквивалент нажатия ESC/закрытия окна),
    /// но БЕЗ закрытия окна напрямую — реальное освобождение ресурсов и
    /// закрытие окна происходит в `shutdown()`, как и при обычном
    /// закрытии через крестик. Используй это вместо того, чтобы решать
    /// "когда выходить" внутри самого движка — это дело приложения.
    pub fn request_exit(&mut self) {
        self.running = false;
    }

    /// Удобный конструктор: создаёт сущность в ECS-сцене и сразу вешает на
    /// неё `MeshRenderer`, ссылающийся на уже загруженный меш (индекс из
    /// `add_cube`/`add_quad`/`add_mesh`/... — то же самое хранилище, что
    /// используется старым `MeshInstance`-путём).
    pub fn spawn_mesh_entity(&mut self, mesh_index: usize) -> crate::scene::EntityId {
        let id = self.scene.spawn();
        self.scene.add_mesh_renderer(id, mesh_index);
        id
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = [r, g, b, a];
    }

    pub fn init(&mut self) -> Result<()> {
        println!("[ENGINE] Initializing Alkash3D Engine v{}...", VERSION);

        // 1. Создаем окно
        self.create_window()?;
        println!("[ENGINE] ✓ Window created");

        // 2. Инициализируем DirectX 12
        unsafe {
            D3D12Device::create()?;
            println!("[ENGINE] ✓ Device created");

            CommandQueue::create()?;
            println!("[ENGINE] ✓ Command queue created");

            let hwnd = self.hwnd.unwrap();
            SwapChain::create(hwnd.0 as isize, self.width, self.height, 2)?;
            println!("[ENGINE] ✓ Swap chain created");

            CommandList::create_allocators(2)?;
            println!("[ENGINE] ✓ Command allocators created");

            let fence = create_fence()?;
            {
                let mut state = STATE.lock().unwrap();
                state.fence = Some(fence);
                state.fence_values = vec![0, 0];
            }
            println!("[ENGINE] ✓ Fence created");

            let renderer = Renderer::new(self.width, self.height, 2)?;
            self.renderer = Some(renderer);
            println!("[ENGINE] ✓ Renderer created");
        }

        // 3. Компилируем шейдеры
        self.compile_default_shaders()?;

        // 4. Создаём корневую сигнатуру (с константным буфером)
        self.create_root_signature()?;

        // 5. Создаём PSO
        self.create_pipeline_state()?;

        // 6. Создаём константный буфер
        // ИСПРАВЛЕНО: раньше создавался ОДИН слот на весь константный
        // буфер, используемый для ВСЕХ объектов кадра подряд — из-за чего
        // все объекты в итоге отрисовывались с трансформацией последнего
        // записанного (см. подробности в TransformConstants::write_at и
        // ensure_constant_buffer_capacity). Теперь буфер заранее вмещает
        // много слотов и растёт по мере необходимости.
        self.ensure_constant_buffer_capacity(128)?;
        println!("[ENGINE] ✓ Constant buffer created");

        // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): compile+создание
        // всего, что нужно для composite/tonemap-прохода — отдельные
        // шейдеры, отдельная root signature (SRV-таблица + сэмплер вместо
        // root-descriptor SRV), отдельный PSO (без input layout/depth) и
        // константный буфер под exposure. Порядок важен: шейдеры/root
        // signature/PSO — та же зависимость, что и у основного 3D-прохода
        // (create_pipeline_state требует уже созданных self.vs/self.ps/
        // self.root_signature).
        self.compile_tonemap_shaders()?;
        self.create_tonemap_root_signature()?;
        self.create_tonemap_pipeline_state()?;

        // exposure=1.0, bloomIntensity=1.0 по умолчанию (сцена без .alfar
        // ещё не загружена — load_lights_from_alfar перезапишет
        // light_global_settings ПОЗЖЕ, но tonemap_constant_buffer нужно
        // создать уже сейчас, чтобы composite-проход в render_frame имел
        // что биндить с первого кадра). Реальные значения записываются
        // заново на каждый кадр в render_frame из self.light_global_settings,
        // если сцена к тому моменту уже загружена — см. там же.
        let tonemap_cb = Buffer::create_constant_buffer(256)?;
        let default_tonemap_params: [f32; 4] = [1.0, 1.0, 0.0, 0.0];
        let bytes = unsafe {
            std::slice::from_raw_parts(default_tonemap_params.as_ptr() as *const u8, 16)
        };
        tonemap_cb.update_constant_buffer(bytes)?;
        self.tonemap_constant_buffer = Some(tonemap_cb);
        println!("[ENGINE] ✓ Tonemap constant buffer created (exposure=1.0, bloomIntensity=1.0 по умолчанию)");

        // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, bloom): шейдеры,
        // root signature, PSO и ресурсы bloom-прохода. compile/create
        // функции должны идти ПОСЛЕ compile_tonemap_shaders (переиспользуют
        // self.tonemap_vs) и ПОСЛЕ создания рендерера (нужны реальные
        // self.width/self.height для half-res таргетов).
        self.compile_bloom_shaders()?;
        self.create_bloom_root_signature()?;
        self.create_bloom_pipeline_states()?;
        self.create_bloom_resources()?;

        // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): шейдеры,
        // root signature, PSO и ресурсы shadow-прохода. Порядок —
        // независим от bloom (свой полностью отдельный набор ресурсов),
        // но должен идти ПОСЛЕ создания устройства (уже есть — Renderer
        // выше) — create_shadow_resources() создаёт GPU-ресурс.
        self.compile_shadow_shaders()?;
        self.create_shadow_root_signature()?;
        self.create_shadow_pipeline_state()?;
        self.create_shadow_resources()?;

        // ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
        // подсветка): шейдеры, root signature, PSO и ресурсы volumetric
        // raymarch-прохода. Порядок ВАЖЕН: create_depth_srv_resources()
        // требует уже созданного renderer (для renderer.depth_stencil) —
        // есть с самого начала init(); create_volumetric_resources()
        // требует уже созданных self.depth_srv_heap (сразу выше) И
        // self.shadow_srv_heap (создан в create_shadow_resources() чуть
        // выше — оба SRV копируются в общий смежный heap raymarch-прохода,
        // см. подробности там).
        self.create_depth_srv_resources()?;
        self.compile_volumetric_shaders()?;
        self.create_volumetric_root_signature()?;
        self.create_volumetric_pipeline_state()?;
        self.create_volumetric_resources()?;

        // Показываем окно
        unsafe {
            ShowWindow(self.hwnd.unwrap(), SW_SHOW);
            UpdateWindow(self.hwnd.unwrap());
        }

        self.running = true;
        println!("[ENGINE] ✓ Initialization complete");
        Ok(())
    }

    fn create_window(&mut self) -> Result<()> {
        unsafe {
            let hinstance = GetModuleHandleA(None)?;
            let window_class = "ALKASH3D_WINDOW\0".as_ptr();

            let wc = WNDCLASSA {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(Self::wndproc_static),
                hInstance: hinstance.into(),
                lpszClassName: PCSTR(window_class),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as _),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };

            RegisterClassA(&wc);

            let hwnd = CreateWindowExA(
                WINDOW_EX_STYLE::default(),
                PCSTR(window_class),
                PCSTR(b"Alkash3D Engine - DirectX 12\0".as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                self.width as i32,
                self.height as i32,
                None,
                None,
                Some(HINSTANCE::from(hinstance)),
                Some(self as *mut Self as _),
            )?;

            self.hwnd = Some(hwnd);
            println!("[ENGINE] Window created: HWND=0x{:X}", hwnd.0 as usize);
        }

        Ok(())
    }

    extern "system" fn wndproc_static(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let cs = lparam.0 as *const CREATESTRUCTA;
                let engine = (*cs).lpCreateParams as *mut AlkashEngine;
                SetWindowLongPtrA(hwnd, GWLP_USERDATA, engine as isize);
            }

            let engine = GetWindowLongPtrA(hwnd, GWLP_USERDATA) as *mut AlkashEngine;
            if !engine.is_null() {
                let engine_ref = &mut *engine;
                return engine_ref.wndproc(hwnd, msg, wparam, lparam);
            }

            DefWindowProcA(hwnd, msg, wparam, lparam)
        }
    }

    fn wndproc(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_CLOSE => {
                    println!("[ENGINE] WM_CLOSE received - stopping engine loop");
                    // ИСПРАВЛЕНО: раньше здесь сразу и синхронно вызывался
                    // DestroyWindow(hwnd). Если после этого главный цикл
                    // движка успевал вызвать ещё хотя бы один render_frame()
                    // до проверки is_running() — Present() уходил в свап-чейн,
                    // привязанный к уже уничтоженному HWND. Для
                    // DXGI_SWAP_EFFECT_FLIP_DISCARD это может спровоцировать
                    // реальный hang GPU / TDR (сброс видеодрайвера,
                    // подвисание изображения на всех мониторах, кратковременное
                    // пропадание звука — драйверный стек перезапускается целиком).
                    //
                    // Теперь мы только останавливаем цикл движка (running = false)
                    // и скрываем окно, ничего не разрушая физически. Реальный
                    // DestroyWindow происходит в конце shutdown() — уже ПОСЛЕ
                    // того как swap chain / device / fence корректно
                    // освобождены, а не до этого.
                    self.running = false;
                    ShowWindow(hwnd, SW_HIDE);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    println!("[ENGINE] WM_DESTROY received - window being destroyed");
                    self.running = false;
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                WM_KEYDOWN => {
                    // ИСПРАВЛЕНО: раньше здесь были зашиты и закрытие по
                    // ESC, и WASD-движение камеры — НАПРЯМУЮ внутри
                    // движка, независимо от того, что `main.rs` делал со
                    // своим собственным опросом `GetAsyncKeyState`. Из-за
                    // этого при удержании клавиши камера двигалась
                    // ДВАЖДЫ за кадр (один раз тут на каждый WM_KEYDOWN,
                    // включая Windows-овский auto-repeat, и ещё раз в
                    // игровом цикле main.rs). Теперь движок только
                    // записывает состояние клавиши в `InputState` — что́ с
                    // этим делать (двигать камеру, закрывать окно и т.п.)
                    // решает уже приложение через `engine.input`.
                    self.input.on_key_down(wparam.0 as u32);
                    LRESULT(0)
                }
                WM_KEYUP => {
                    self.input.on_key_up(wparam.0 as u32);
                    LRESULT(0)
                }
                WM_SIZE => {
                    let width = (lparam.0 & 0xFFFF) as u32;
                    let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                    // ИСПРАВЛЕНО: раньше здесь напрямую вызывался
                    // swap_chain.ResizeBuffers(...) с игнорированием ошибки
                    // (`let _ = ...`). ResizeBuffers требует, чтобы ВСЕ
                    // внешние ссылки на back buffer'ы swap chain'а были
                    // освобождены до вызова — а `Renderer` держит их в
                    // `back_buffers`/RTV heap, которые никогда не
                    // трогались. Поэтому ResizeBuffers гарантированно
                    // проваливался с DXGI_ERROR_INVALID_CALL, ошибка
                    // молча проглатывалась, а окно продолжало рендериться
                    // в старом разрешении. Теперь используется
                    // handle_resize(), который сначала ждёт GPU idle,
                    // отпускает Renderer, ресайзит буферы и пересоздаёт
                    // Renderer под новый размер.
                    //
                    // ИСПРАВЛЕНО (краш видеодрайвера при смене разрешения —
                    // реальный баг, найденный на живой машине пользователя):
                    // раньше `handle_resize()` вызывался ЗДЕСЬ безусловно на
                    // КАЖДОЕ WM_SIZE. Windows шлёт WM_SIZE на каждый
                    // промежуточный кадр перетаскивания рамки окна мышью
                    // (live-resize) — не один раз в конце жеста, а
                    // непрерывно, пока зажата кнопка мыши. Каждый вызов
                    // `handle_resize()` — это полное ожидание GPU idle +
                    // уничтожение всего Renderer (back buffers/RTV/DSV/HDR/
                    // SRV) + `ResizeBuffers` + пересоздание всего заново.
                    // Повторение этой тяжёлой цепочки десятки раз в секунду
                    // на слабом железе (10-летний минимум — целевая планка
                    // движка) регулярно превышало таймаут Timeout Detection
                    // & Recovery видеодрайвера, из-за чего сам драйвер
                    // считал GPU "зависшим" и сбрасывался — что на практике
                    // выглядело как крах/перезагрузка ПК. Теперь: пока идёт
                    // live-resize (`self.resizing_live`, выставляется между
                    // WM_ENTERSIZEMOVE/WM_EXITSIZEMOVE ниже), WM_SIZE только
                    // запоминает целевой размер в `pending_resize` — ни один
                    // тяжёлый вызов не делается, пока пользователь реально
                    // тянет рамку. Реальный `handle_resize()` для этого
                    // случая происходит ОДИН раз, в WM_EXITSIZEMOVE. Если же
                    // WM_SIZE пришёл БЕЗ активного live-resize (например,
                    // программная смена разрешения через меню/настройки
                    // движка, которая не идёт через перетаскивание рамки
                    // мышью) — ведём себя как раньше и ресайзим немедленно,
                    // одним вызовом, т.к. серии промежуточных WM_SIZE в этом
                    // случае не возникает.
                    if width > 0 && height > 0 && (width != self.width || height != self.height) {
                        if self.resizing_live {
                            self.pending_resize = Some((width, height));
                        } else {
                            self.width = width;
                            self.height = height;
                            self.camera.set_aspect(width, height);
                            self.handle_resize(width, height);
                        }
                    }
                    LRESULT(0)
                }
                // ДОБАВЛЕНО (фикс краша видеодрайвера при смене разрешения):
                // начало live-resize/перемещения окна мышью — см. подробный
                // комментарий у WM_SIZE выше. С этого момента и до
                // WM_EXITSIZEMOVE реальный ресайз GPU-ресурсов
                // откладывается.
                WM_ENTERSIZEMOVE => {
                    self.resizing_live = true;
                    self.pending_resize = None;
                    LRESULT(0)
                }
                // ДОБАВЛЕНО (фикс краша видеодрайвера при смене разрешения):
                // пользователь отпустил рамку/заголовок окна — если за время
                // перетаскивания реально накопился новый размер
                // (`pending_resize`, т.е. это было именно изменение
                // размера, а не просто перемещение окна), применяем его
                // ОДНИМ вызовом `handle_resize()` здесь. Если размер не
                // менялся (чистое перемещение) — `pending_resize` остаётся
                // `None`, и ничего тяжёлого не происходит.
                WM_EXITSIZEMOVE => {
                    self.resizing_live = false;
                    if let Some((width, height)) = self.pending_resize.take() {
                        if width > 0 && height > 0 && (width != self.width || height != self.height) {
                            self.width = width;
                            self.height = height;
                            self.camera.set_aspect(width, height);
                            self.handle_resize(width, height);
                        }
                    }
                    LRESULT(0)
                }
                _ => DefWindowProcA(hwnd, msg, wparam, lparam),
            }
        }
    }

    /// Корректная обработка ресайза окна: дожидается GPU idle, освобождает
    /// Renderer (владеющий back buffer'ами/RTV/DSV), ресайзит swap chain и
    /// пересоздаёт Renderer под новый размер.
    fn handle_resize(&mut self, width: u32, height: u32) {
        println!("[ENGINE] Handling resize: {}x{}", width, height);

        // 1. Дожидаемся, что GPU закончил использовать текущие back
        //    buffer'ы — иначе ResizeBuffers ниже гарантированно провалится.
        //
        // ИСПРАВЛЕНО ("белое окно" + краш драйвера при смене разрешения —
        // см. подробный комментарий у `wait_for_fence` в начале файла):
        // раньше ожидание здесь было БЕЗ таймаута — если GPU по любой
        // причине не может просигналить это значение (TDR, зависший
        // драйвер), `handle_resize()` (вызывается прямо из `wndproc`, то
        // есть с главного потока, где крутится message loop) зависал
        // навсегда, замораживая окно. Теперь ждём не дольше 5 секунд и
        // прерываем ресайз с диагностикой при таймауте/потере устройства,
        // вместо вечного зависания.
        let (queue_opt, fence_opt) = {
            let state = STATE.lock().unwrap();
            (state.command_queue.clone(), state.fence.clone())
        };
        if let (Some(queue), Some(fence)) = (queue_opt, fence_opt) {
            let value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
            unsafe {
                if queue.Signal(&fence, value).is_ok() {
                    if let Err(reason) = wait_for_fence(&fence, value, std::time::Duration::from_secs(5)) {
                        eprintln!("[ENGINE] handle_resize: {} — resize прерван", reason);
                        crate::dump_d3d12_debug_messages();
                        return;
                    }
                }
            }
        } else {
            // Устройство ещё не инициализировано (ресайз до init()) —
            // делать нечего.
            return;
        }

        // 2. Освобождаем renderer: он держит back buffers/RTV/DSV. Без
        //    этого ResizeBuffers ниже вернёт DXGI_ERROR_INVALID_CALL.
        self.renderer = None;

        // 3. Ресайзим сами буферы swap chain.
        let resize_ok = {
            let state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                let hr = unsafe {
                    swap_chain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
                };
                match hr {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("[ENGINE] ResizeBuffers failed: {:?}", e);
                        false
                    }
                }
            } else {
                false
            }
        };

        if !resize_ok {
            return;
        }

        // 4. Пересоздаём renderer (RTV/DSV/back buffers) под новый размер.
        match Renderer::new(width, height, 2) {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                println!("[ENGINE] ✓ Renderer recreated after resize: {}x{}", width, height);
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to recreate renderer after resize: {:?}", e);
            }
        }

        // ИСПРАВЛЕНО (реальный краш на живой машине: HRESULT(0x80070057) на
        // самом первом кадре ВСЕГДА, даже после фикса bloom-барьера):
        // `Renderer::new()` выше создаёт СОВЕРШЕННО НОВЫЙ
        // `renderer.srv_uav_heap` (новый COM-объект дескрипторного хипа) —
        // а `create_bloom_resources()` (вызывается ОДИН раз в init(), ДО
        // первого resize) записывает SRV на bloom_texture_a во ВТОРОЙ слот
        // (индекс 1) именно ТОГДАШНЕГО `renderer.srv_uav_heap`, который
        // Renderer::new() внутри Self::init() создал изначально (индекс 0
        // там сам Renderer::new() занимает под HDR SRV). После resize этот
        // старый хип уничтожается вместе со старым `self.renderer`, а
        // новый хип получает свежий HDR SRV в индексе 0 (это делает сам
        // Renderer::new()), но НИКТО не записывает bloom SRV в индекс 1
        // НОВОГО хипа — там остаётся неинициализированная память.
        // Composite/tonemap-проход в render_frame() биндит 2-дескрипторную
        // таблицу (t0=HDR, t1=Bloom) из этого хипа — с валидным HDR в
        // индексе 0, но мусором в индексе 1 — GPU получает недействительный
        // дескриптор и Close()/ExecuteCommandLists() отвергает список
        // команд как невалидный параметр. Поскольку `handle_resize()`
        // ВСЕГДА выполняется хотя бы раз сразу после init() (см. лог —
        // окно подгоняется под фактический клиентский размер сразу после
        // создания), это не редкий edge-case, а гарантированный краш на
        // первом же кадре КАЖДОГО запуска. Исправление: если bloom-ресурсы
        // уже существуют (т.е. resize происходит ПОСЛЕ init(), не до
        // него), заново регистрируем SRV bloom_texture_a в индексе 1
        // НОВОГО renderer.srv_uav_heap — точно та же операция, что и в
        // create_bloom_resources(), но применённая к новому хипу.
        if let (Some(renderer), Some(bloom_a)) = (&self.renderer, &self.bloom_texture_a) {
            let cbv_srv_uav_size = {
                let state = STATE.lock().unwrap();
                state.cbv_srv_uav_descriptor_size
            };
            let bloom_final_srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 1, cbv_srv_uav_size);
            match bloom_a.create_srv(bloom_final_srv_cpu) {
                Ok(()) => println!("[ENGINE] ✓ Bloom SRV re-registered in new srv_uav_heap after resize"),
                Err(e) => eprintln!("[ENGINE] WARNING: failed to re-register bloom SRV after resize: {:?}", e),
            }
        }

        // 5. Сбрасываем накопленные fence-значения по индексам back
        //    buffer'ов — старые ссылались на уже не существующие ресурсы.
        for v in &mut self.frame_fence_values {
            *v = 0;
        }

        // 6. Синхронизируем текущий индекс back buffer'а.
        let mut state = STATE.lock().unwrap();
        if let Some(swap_chain) = &state.swap_chain {
            state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
        }
    }

    pub fn process_messages(&mut self) {
        // ДОБАВЛЕНО: очищаем "мгновенные" флаги ввода (just_pressed/
        // just_released) с ПРЕДЫДУЩЕГО кадра перед тем, как разбирать
        // новые оконные сообщения этого кадра. `is_down` не трогается —
        // только разовые флаги.
        self.input.end_frame();

        unsafe {
            let mut msg = MSG::default();
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    println!("[ENGINE] WM_QUIT received - exiting message loop");
                    self.running = false;
                    break;
                }
                // Обработка сообщения WM_DESTROY через окно
                if msg.message == WM_DESTROY {
                    println!("[ENGINE] WM_DESTROY received in message loop");
                    self.running = false;
                    // Позволяем DefWindowProc обработать сообщение
                    let _ = DefWindowProcA(msg.hwnd, msg.message, msg.wParam, msg.lParam);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }
        }
    }

    fn compile_default_shaders(&mut self) -> Result<()> {
        // ИСПРАВЛЕНО (Фаза 0 плана по реализму/фонарям):
        // 1) Вершинный шейдер раньше писал в output.normal константу
        //    float3(0,0,1) для ЛЮБОЙ геометрии — теперь читает реальную
        //    нормаль из вершинного буфера (см. новое поле NORMAL в
        //    engine::Vertex / pso.rs input layout) и переводит её в мировое
        //    пространство через верхний 3x3-блок матрицы `model` (без
        //    учёта translation — как и положено для направлений).
        //    Внимание на будущее (Фаза 4+): при неравномерном масштабе
        //    (scale.x != scale.y != scale.z) для абсолютно корректного
        //    преобразования нормалей нужна inverse-transpose матрицы
        //    модели, а не сама model — пока движок использует равномерный
        //    scale в большинстве мест, поэтому это сознательно отложено, а
        //    не пропущено по незнанию.
        // 2) Пиксельный шейдер раньше игнорировал `lightDir`/`lightColor`/
        //    `ambientColor` из TransformConstants и хардкодил свой
        //    собственный источник света прямо в шейдере — теперь читает
        //    те значения, что уже лежат в константном буфере (см.
        //    constant_buffer.rs), что даёт единую точку управления светом
        //    из Rust-кода вместо двух рассинхронизированных копий.
        //    Список фонарей FirstFires (GPULight) в этот шейдер ЕЩЁ НЕ
        //    подключён — это отдельная, более крупная Фаза 2 плана
        //    (нужен StructuredBuffer с видимыми фонарями + проход по
        //    списку в цикле), в рамках Фазы 0 меняется только то, что уже
        //    было в constant-buffer, но не читалось.
        // ВАЖНО: cbuffer-декларация в вершинном и пиксельном шейдерах
        // обязана иметь ОДИНАКОВЫЙ layout (порядок и типы полей), потому
        // что оба читают его из ОДНОГО И ТОГО ЖЕ root CBV (b0), который
        // заполняется из ОДНОЙ Rust-структуры TransformConstants (см.
        // constant_buffer.rs). lightCount добавлен в конец, вслед за
        // ambientColor — ровно там же, где он лежит в Rust-структуре.
        let vs_source = r#"
        cbuffer TransformConstants : register(b0) {
            float4x4 modelViewProj;
            float4x4 model;
            float4x4 view;
            float4x4 proj;
            float4 cameraPos;
            float4 lightDir;
            float4 lightColor;
            float4 ambientColor;
            uint lightCount;
            uint3 _lightCountPadding;
            float4 gridWorldMin; // xyz = world_min, w = cell_size
            uint4 gridDimensions; // x,y,z = grid_width/height/depth
        };

        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): поле `uv` в
        // VS_INPUT/VS_OUTPUT — зеркалит новое поле `uv: [f32;2]` в
        // `engine::Vertex` (TEXCOORD0 элемент input layout, см. pso.rs).
        // TEXCOORD2 у VS_OUTPUT.uv (не TEXCOORD0!) — TEXCOORD0/1 у
        // VS_OUTPUT уже заняты worldPos/normal, а входной семантический
        // индекс input layout (TEXCOORD0 у VS_INPUT.uv) — ОТДЕЛЬНОЕ
        // пространство имён от выходных семантик VS_OUTPUT, переиспользовать
        // индексы между входом/выходом вершинного шейдера можно без
        // конфликта, но здесь сознательно выбран следующий свободный
        // (TEXCOORD2), чтобы не запутывать чтение кода.
        // ДОБАВЛЕНО (Задача #15, normal mapping): TANGENT — зеркалит новое
        // поле `tangent: [f32;4]` в `engine::Vertex` (xyz + w=handedness,
        // см. подробный комментарий там). VS_OUTPUT.tangent — TEXCOORD3
        // (следующий свободный после worldPos@0/normal@1/uv@2).
        struct VS_INPUT {
            float4 pos : POSITION;
            float3 normal : NORMAL;
            float4 color : COLOR;
            float2 uv : TEXCOORD0;
            float4 tangent : TANGENT;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
            float3 worldPos : TEXCOORD0;
            float3 normal : TEXCOORD1;
            float2 uv : TEXCOORD2;
            float4 tangent : TEXCOORD3;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            output.pos = mul(modelViewProj, input.pos);
            output.color = input.color;
            output.worldPos = mul(model, input.pos).xyz;
            // Верхний 3x3 блок model — вращение/масштаб, без переноса;
            // этого достаточно для направлений при равномерном scale.
            float3x3 normalMatrix = (float3x3)model;
            output.normal = normalize(mul(normalMatrix, input.normal));
            output.uv = input.uv;
            // ДОБАВЛЕНО (Задача #15, normal mapping): tangent преобразуется
            // ТОЙ ЖЕ normalMatrix, что и normal (оба — направления, не
            // точки, w-компонента handedness переносится как есть — знак
            // не зависит от вращения/равномерного масштаба).
            output.tangent = float4(normalize(mul(normalMatrix, input.tangent.xyz)), input.tangent.w);
            return output;
        }
        "#;

        // ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): GPULight — точная
        // HLSL-копия Rust-структуры plugin::light_api::GPULight (см. её
        // подробное описание там же): position.w = тип света
        // (0=Point,1=Spot,2=Directional), color.w = intensity,
        // direction.w = range, params.x = spot_outer_angle, params.y =
        // falloff_type. Читается через StructuredBuffer из root SRV (t0),
        // который каждый кадр обновляется из уже отфильтрованного
        // FirstFires списка (см. render_frame в engine/mod.rs).
        //
        // Освещение здесь — БАЗОВАЯ physically-plausible модель (честное
        // inverse-square затухание с плавным обнулением к границе range,
        // Lambertian-диффуз), достаточная, чтобы фонари реально стали
        // видны и физически осмысленны. Это НЕ финальная модель из плана:
        // профиль формы фонаря (IES-подобная "бабочка", spot inner/outer
        // cone), тени и volumetric-подсветка — отдельные, более крупные
        // Фазы 4/6/8, которые расширят именно этот цикл, а не переписывают
        // его с нуля.
        let ps_source = r#"
        struct GPULight {
            float4 position;
            float4 color;
            float4 direction;
            float4 params;
        };
        StructuredBuffer<GPULight> Lights : register(t0);

        // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): пространственная
        // сетка FirstFires — LightGridCell/LightGridEntry зеркалят layout
        // Rust-структур в plugin/light_api.rs. GridCells[cellIndex] даёт
        // offset+count в GridEntries, GridEntries[offset..offset+count]
        // даёт индексы в Lights[] — именно эти фонари реально пересекают
        // ДАННУЮ ячейку мира, а не весь видимый список кадра. Раньше (Фаза
        // 2) шейдер перебирал ВСЕ lightCount видимых фонарей на КАЖДЫЙ
        // пиксель — корректно, но не масштабируется на город с сотнями
        // источников. Теперь пиксель проверяет только фонари своей ячейки.
        struct LightGridCell {
            uint offset;
            uint count;
        };
        struct LightGridEntry {
            uint lightIndex;
            uint lodLevel;
            float depth;
            uint padding;
        };
        StructuredBuffer<LightGridCell> GridCells : register(t1);
        StructuredBuffer<LightGridEntry> GridEntries : register(t2);

        // ОБНОВЛЕНО (Cascaded Shadow Maps): раньше здесь была ОДНА shadow
        // map. Теперь NUM_CASCADES (=3) отдельных текстур t3, t4, t5 —
        // HLSL требует объявлять Texture2D массив как ФИКСИРОВАННОЕ число
        // именованных регистров (Texture2D ShadowMap[3] тоже возможен
        // синтаксически, но неоднородный размер дескрипторной таблицы и
        // индексация по переменной внутри массива текстур не везде
        // одинаково поддерживаются старым SM 5.0 без динамической
        // индексации ресурсов — явные 3 регистра надёжнее и проще). t3
        // идёт следом за t0..t2 фонарей/сетки выше, s0 свободен (у этой
        // root signature раньше не было ни одного статического сэмплера
        // вообще). Порядок регистров ОБЯЗАН совпадать с NumDescriptors/
        // BaseShaderRegister в create_root_signature (engine/mod.rs).
        Texture2D ShadowMapCascade0 : register(t3);
        Texture2D ShadowMapCascade1 : register(t4);
        Texture2D ShadowMapCascade2 : register(t5);
        SamplerComparisonState ShadowSampler : register(s0);

        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): albedo-текстура
        // ТЕКУЩЕГО рисуемого меша — register t6 (следующий свободный после
        // t3..t5 shadow-каскадов), root-параметр 5, отдельная descriptor
        // table, перебиндивается на каждый Draw (см. render_frame). Читается
        // через MaterialSampler (register s1 — обычный линейный WRAP-сэмплер,
        // ОТДЕЛЬНЫЙ от compare-сэмплера ShadowSampler s0: `Texture2D.Sample`
        // с SamplerComparisonState недопустим в HLSL). Меши без собственной
        // текстуры получают белую (1,1,1,1) fallback-текстуру в ЭТОМ ЖЕ
        // регистре (см. `white_texture_srv_fallback` в render_frame) — PS
        // ниже поэтому МОЖЕТ безусловно сэмплировать AlbedoMap для КАЖДОГО
        // меша без отдельной HLSL-ветки "текстуры нет вообще".
        Texture2D AlbedoMap : register(t6);
        SamplerState MaterialSampler : register(s1);

        // ДОБАВЛЕНО (Задача #15, normal mapping): normal map (t7) и
        // metallic-roughness map (t8) ТЕКУЩЕГО рисуемого меша — root-
        // параметры 6 и 7 (отдельные descriptor table, см.
        // create_root_signature), тот же MaterialSampler (s1), что и у
        // AlbedoMap. Меши без своей карты получают нейтральные fallback-
        // текстуры (flat normal (128,128,255) / dummy MR) в ЭТИХ ЖЕ
        // регистрах — тот же принцип "безусловное чтение без HLSL-ветки",
        // что и у AlbedoMap выше.
        Texture2D NormalMap : register(t7);
        Texture2D MetallicRoughnessMap : register(t8);

        // ДОБАВЛЕНО (Задача #15, normal mapping): root constants (root-
        // параметр 8, register b1). `rootMetallic`/`rootRoughness` —
        // скалярные PBR-параметры ЭТОГО меша (см. `Mesh::material_metallic`/
        // `material_roughness`). `hasMrMap` — явный флаг (1.0/0.0): 1.0
        // значит "у меша есть собственная MetallicRoughnessMap, читать её",
        // 0.0 значит "карты нет (или fallback-текстура), использовать
        // rootMetallic/rootRoughness напрямую". Флаг обязателен: SRV сам по
        // себе не несёт признака "это fallback или реальные данные" — это
        // знает только Rust-сторона (`Mesh::mr_srv_index.is_some()`, см.
        // render_frame), поэтому передаётся явным third root constant'ом,
        // а не выводится внутри HLSL.
        cbuffer MaterialConstants : register(b1) {
            float rootMetallic;
            float rootRoughness;
            float hasMrMap;
            float _materialConstantsPadding;
        };

        cbuffer TransformConstants : register(b0) {
            float4x4 modelViewProj;
            float4x4 model;
            float4x4 view;
            float4x4 proj;
            float4 cameraPos;
            float4 lightDir;
            float4 lightColor;
            float4 ambientColor;
            uint lightCount;
            uint3 _lightCountPadding;
            float4 gridWorldMin; // xyz = world_min, w = cell_size
            uint4 gridDimensions; // x,y,z = grid_width/height/depth
            // ОБНОВЛЕНО (Cascaded Shadow Maps): порядок ОБЯЗАН совпадать с
            // Rust-структурой TransformConstants (constant_buffer.rs) —
            // массив light_view_proj[NUM_CASCADES] идёт СРАЗУ за
            // gridDimensions, как и там, затем cascadeSplitDistances.
            float4x4 lightViewProj[3];
            float4 cascadeSplitDistances; // x,y,z = дальние границы каскадов 0,1,2 (view-space, метры); w не используется
            float shadowBias;
            float shadowMapSize;
            uint shadowsEnabled;
            uint _shadowPadding;
        };

        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
            float3 worldPos : TEXCOORD0;
            float3 normal : TEXCOORD1;
            // ДОБАВЛЕНО (Задача #15): см. VS_OUTPUT.uv в вершинном шейдере
            // выше — те же TEXCOORD2, интерполируется растеризатором
            // между вершинами треугольника как обычно.
            float2 uv : TEXCOORD2;
            // ДОБАВЛЕНО (Задача #15, normal mapping): см. VS_OUTPUT.tangent
            // выше — TEXCOORD3.
            float4 tangent : TEXCOORD3;
        };

        // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): 3x3 PCF
        // (Percentage-Closer Filtering) — вместо ОДНОГО сравнения глубины
        // (что дало бы резкий, "лестничный" край тени — ступеньки шириной
        // в один тексель shadow map, ЗАМЕТНЫЕ "попы"/дрожание при движении
        // камеры, что явно запрещено требованиями проекта) берём 9
        // сравнений в радиусе одного текселя вокруг искомой точки и
        // усредняем результат — край тени становится плавным градиентом,
        // а не жёсткой границей. `SampleCmpLevelZero` — аппаратная
        // инструкция сравнения (сравнивает ЗАПИСАННУЮ в shadow map глубину
        // с переданной `compareDepth` и возвращает 0..1 результат
        // билинейной интерполяции 2x2 соседних сравнений одним вызовом) —
        // используем её как строительный блок 3x3 сетки вместо ручной
        // проверки каждого текселя по отдельности (что потребовало бы
        // Texture2D::Load вместо Sample и было бы медленнее).
        // ОБНОВЛЕНО (Cascaded Shadow Maps): PCF теперь принимает индекс
        // каскада и сэмплирует СООТВЕТСТВУЮЩУЮ текстуру — статическая
        // ветка по cascadeIndex (0/1/2), а не динамическая индексация
        // массива текстур (см. комментарий у ShadowMapCascade0..2 выше,
        // почему регистры именованные, а не Texture2D[3]).
        float SampleShadowPCF(int cascadeIndex, float3 shadowCoord) {
            float texelSize = 1.0 / max(shadowMapSize, 1.0);
            float sum = 0.0;
            [unroll]
            for (int y = -1; y <= 1; y++) {
                [unroll]
                for (int x = -1; x <= 1; x++) {
                    float2 offset = float2(x, y) * texelSize;
                    float2 uv = shadowCoord.xy + offset;
                    if (cascadeIndex == 0) {
                        sum += ShadowMapCascade0.SampleCmpLevelZero(ShadowSampler, uv, shadowCoord.z);
                    } else if (cascadeIndex == 1) {
                        sum += ShadowMapCascade1.SampleCmpLevelZero(ShadowSampler, uv, shadowCoord.z);
                    } else {
                        sum += ShadowMapCascade2.SampleCmpLevelZero(ShadowSampler, uv, shadowCoord.z);
                    }
                }
            }
            return sum / 9.0;
        }

        // ДОБАВЛЕНО (Cascaded Shadow Maps): выбирает индекс каскада по
        // view-space глубине пикселя (расстояние вдоль оси взгляда камеры,
        // НЕ euclidean-дистанция до камеры — то же соглашение, что и у
        // cascade_far_distances в render_frame/engine/mod.rs, которые
        // считаются как camera.far * CASCADE_SPLITS). Берём САМЫЙ БЛИЖНИЙ
        // каскад, чья дальняя граница ещё не меньше viewDepth — то есть
        // первый каскад, который "накрывает" эту глубину; если пиксель
        // дальше самого дальнего каскада (viewDepth > cascadeSplitDistances.z),
        // всё равно используем последний каскад, а не отключаем тени
        // резко на границе — плавнее деградирует на самом краю дальности
        // теней, чем полное отсутствие тени.
        int SelectCascade(float viewDepth) {
            if (viewDepth <= cascadeSplitDistances.x) {
                return 0;
            } else if (viewDepth <= cascadeSplitDistances.y) {
                return 1;
            }
            return 2;
        }

        // Возвращает множитель освещённости directional-света от теней:
        // 1.0 = полностью освещён, 0.0 = полностью в тени. Безопасные
        // fallback'и на 1.0 (не в тени) — если тени выключены глобально
        // (shadowsEnabled==0, например .alfar сцена не загружена) ИЛИ
        // пиксель вне ортографического объёма shadow map (координаты shadow
        // space вне [0,1] по X/Y или вне [0,1] по Z — за near/far
        // light-проекции); последнее НЕ должно происходить для видимой
        // геометрии (объём подгоняется под camera frustum, см.
        // compute_cascade_view_proj в engine/mod.rs), но защищает от чтения
        // границы карты при численных краевых случаях.
        //
        // ОБНОВЛЕНО (Cascaded Shadow Maps): принимает уже готовый viewDepth
        // (view-space Z пикселя, см. вызов в main() ниже) — используется
        // ДВАЖДЫ: чтобы выбрать каскад (SelectCascade) И чтобы выбрать
        // соответствующую lightViewProj[cascadeIndex] для проекции worldPos
        // в пространство ИМЕННО этого каскада.
        float ComputeShadowFactor(float3 worldPos, float3 normal, float viewDepth) {
            if (shadowsEnabled == 0) {
                return 1.0;
            }
            int cascadeIndex = SelectCascade(viewDepth);
            float4 lightSpacePos = mul(lightViewProj[cascadeIndex], float4(worldPos, 1.0));
            if (lightSpacePos.w <= 0.0001) {
                return 1.0;
            }
            float3 ndc = lightSpacePos.xyz / lightSpacePos.w;
            // NDC X/Y в [-1,1] (DirectX-конвенция) -> UV [0,1] с переворотом
            // Y (текстурные координаты растут ВНИЗ, NDC Y растёт ВВЕРХ) —
            // тот же переворот, что неявно делает растеризатор для
            // обычного экрана, но здесь нужен вручную, т.к. мы читаем
            // shadow map как обычную текстуру, а не через SV_POSITION.
            float2 shadowUV = float2(ndc.x * 0.5 + 0.5, 1.0 - (ndc.y * 0.5 + 0.5));
            float shadowDepth = ndc.z;
            if (shadowUV.x < 0.0 || shadowUV.x > 1.0 || shadowUV.y < 0.0 || shadowUV.y > 1.0 || shadowDepth < 0.0 || shadowDepth > 1.0) {
                return 1.0;
            }
            // Нормаль-based bias: пологие поверхности (свет скользит почти
            // параллельно нормали) страдают от acne сильнее, чем
            // перпендикулярные — компенсируем sqrt(1-NdotL^2) (~tan угла
            // падения), плюс базовый shadowBias для перпендикулярного
            // случая. Оба фактора работают ВМЕСТЕ с аппаратным
            // DepthBias/SlopeScaledDepthBias из shadow PSO (см.
            // create_shadow_pipeline_state) — не дублируют, а
            // подстраховывают друг друга на разных углах.
            float3 toLight = normalize(-lightDir.xyz);
            float ndotl = saturate(dot(normal, toLight));
            float slopeBias = shadowBias * sqrt(saturate(1.0 - ndotl * ndotl)) * 4.0 + shadowBias;
            float biasedDepth = saturate(shadowDepth - slopeBias);
            return SampleShadowPCF(cascadeIndex, float3(shadowUV, biasedDepth));
        }

        // ОБНОВЛЕНО (Фаза 4 плана по реализму/фонарям): раньше Point (0) и
        // Spot (1) обрабатывались АБСОЛЮТНО одинаково — условие `lightType
        // > 1.5` отделяло только Directional (2) от всех остальных, то
        // есть уличный фонарь, помеченный как Spot, светил равномерно во
        // все стороны как голая лампочка, что и есть главная претензия
        // пользователя ("фонари надо рисовать как в реальности"). Теперь
        // Spot получает настоящий конус: params.x = spot_outer_angle,
        // params.z = spot_inner_angle (радианы, угол от оси direction).
        // Между inner и outer — плавный smoothstep-переход ("мягкое
        // пятно", похожее по ощущению на реальный отражатель фонаря, без
        // полноценного IES-профиля, который остаётся отдельным будущим
        // улучшением для топового железа, а не обязательным минимумом).
        // Вынесено в отдельную функцию, чтобы grid-путь (основной) и
        // fallback-путь (полный перебор, см. ниже) считали освещение
        // ИДЕНТИЧНО, не двумя разными формулами, которые могли бы
        // незаметно разойтись при будущих правках.
        float3 ComputePointLightContribution(GPULight light, float3 worldPos, float3 normal) {
            float lightType = light.position.w; // 0=Point,1=Spot,2=Directional
            float3 toL;
            float attenuation;
            if (lightType > 1.5) {
                // Directional-фонарь из списка FirstFires (редкий случай —
                // обычно directional задаётся отдельно через
                // lightDir/lightColor выше в main(), но поддерживаем и
                // здесь для полноты, если такой свет добавят через .alfar).
                toL = normalize(-light.direction.xyz);
                attenuation = 1.0;
            } else {
                // Point и Spot: общее для обоих — честное inverse-square
                // затухание по расстоянию с плавным обнулением к границе
                // range (window function — без неё виден резкий обрыв
                // освещённости ровно на границе радиуса действия фонаря).
                float3 toLightVec = light.position.xyz - worldPos;
                float dist = length(toLightVec);
                toL = dist > 0.0001 ? (toLightVec / dist) : float3(0.0, 1.0, 0.0);
                float range = max(light.direction.w, 0.001);
                float distRatio = saturate(dist / range);
                float windowFalloff = (1.0 - distRatio * distRatio);
                windowFalloff = windowFalloff * windowFalloff;
                float invSquare = 1.0 / max(dist * dist, 0.01);
                attenuation = invSquare * windowFalloff;

                if (lightType > 0.5) {
                    // Spot: дополнительный конусный множитель. direction.xyz
                    // — направление, КУДА светит фонарь (не "к фонарю", а
                    // "от фонаря") — поэтому сравниваем с -toL (вектор ОТ
                    // фонаря К пикселю), а не с toL (вектор К фонарю).
                    float3 spotDir = normalize(light.direction.xyz);
                    float cosAngle = dot(spotDir, -toL);
                    float cosOuter = cos(max(light.params.x, 0.001));
                    float cosInner = cos(max(min(light.params.z, light.params.x), 0.0));
                    // saturate: если inner>=outer (некорректные/нулевые
                    // данные — например params.z не задан для старой сцены,
                    // добавленной через add_street_light без spot-полей),
                    // smoothstep(cosOuter, cosInner, x) с cosInner<=cosOuter
                    // даёт корректный резкий, но не мусорный переход, а не
                    // NaN/деление на 0.
                    float coneFactor = smoothstep(cosOuter, max(cosInner, cosOuter + 0.0001), cosAngle);
                    attenuation *= coneFactor;
                }
            }
            float lightDiff = max(dot(normal, toL), 0.0);
            float intensity = light.color.w;
            return light.color.rgb * intensity * lightDiff * attenuation;
        }

        // ДОБАВЛЕНО (Задача #15, normal mapping — PBR-специуляр):
        // Cook-Torrance микрофасетная модель с GGX/Trowbridge-Reitz
        // распределением нормалей (D), Smith-геометрией с
        // Schlick-GGX-аппроксимацией (G) и Schlick-аппроксимацией Френеля
        // (F) — стандартная тройка функций physically-based specular,
        // применяется здесь ТОЛЬКО к directional-свету (солнце/луна,
        // единственный источник, отбрасывающий тени и визуально
        // доминирующий в кадре) — это сознательно консервативный первый
        // шаг PBR-specular: point/spot-фонари FirstFires остаются на
        // прежней Lambertian-модели (ComputePointLightContribution выше),
        // не переписанной в этом шаге, чтобы не рисковать регрессией уже
        // работающего городского освещения ради специуляра, который на
        // маленьких point-источниках визуально менее заметен, чем на ярком
        // направленном свете.
        //
        // Диэлектрики (metallic=0) используют F0=0.04 (стандартное
        // приближение для большинства неметаллов — стекло, пластик,
        // камень), металлы (metallic=1) используют сам albedo как F0
        // (металлы отражают тем же цветом, каким выглядит их диффуз) —
        // линейная интерполяция между ними по metallic, тот же приём, что
        // в стандартном glTF/Disney PBR.
        float DistributionGGX(float3 N, float3 H, float roughness) {
            float a = roughness * roughness;
            float a2 = a * a;
            float NdotH = max(dot(N, H), 0.0);
            float NdotH2 = NdotH * NdotH;
            float denom = (NdotH2 * (a2 - 1.0) + 1.0);
            denom = 3.14159265 * denom * denom;
            return a2 / max(denom, 0.0001);
        }
        float GeometrySchlickGGX(float NdotV, float roughness) {
            float r = roughness + 1.0;
            float k = (r * r) / 8.0;
            return NdotV / max(NdotV * (1.0 - k) + k, 0.0001);
        }
        float GeometrySmith(float3 N, float3 V, float3 L, float roughness) {
            float NdotV = max(dot(N, V), 0.0);
            float NdotL = max(dot(N, L), 0.0);
            return GeometrySchlickGGX(NdotV, roughness) * GeometrySchlickGGX(NdotL, roughness);
        }
        float3 FresnelSchlick(float cosTheta, float3 F0) {
            return F0 + (1.0 - F0) * pow(saturate(1.0 - cosTheta), 5.0);
        }
        // Возвращает СПЕЦИУЛЯРНУЮ (не диффузную — та считается снаружи, как
        // и раньше) добавку Cook-Torrance для directional-света. `radiance`
        // — уже посчитанный вклад источника (цвет * intensity * NdotL *
        // shadowFactor) — та же величина, что раньше целиком уходила в
        // diffuse; здесь распределяется между диффузом (снаружи, домножен
        // на (1-metallic) — металлы не имеют диффузного отклика) и этим
        // specular-членом.
        float3 ComputeSpecularGGX(float3 N, float3 V, float3 L, float3 albedo, float metallic, float roughness, float3 radiancePerNdotL) {
            float3 H = normalize(V + L);
            float NdotL = max(dot(N, L), 0.0);
            float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
            float NDF = DistributionGGX(N, H, roughness);
            float G = GeometrySmith(N, V, L, roughness);
            float3 F = FresnelSchlick(max(dot(H, V), 0.0), F0);
            float3 numerator = NDF * G * F;
            float denom = 4.0 * max(dot(N, V), 0.0) * NdotL + 0.0001;
            float3 specular = numerator / denom;
            return specular * radiancePerNdotL * NdotL;
        }

        float4 main(PS_INPUT input) : SV_TARGET {
            float3 geomNormal = normalize(input.normal);

            // ДОБАВЛЕНО (Задача #15, normal mapping): построение TBN-базиса
            // и трансформация сэмплированной normal map (tangent-space) в
            // world-space. Gram-Schmidt пере-ортогонализация tangent
            // относительно normal — интерполяция по треугольнику (и
            // неравномерный масштаб модели) может немного разбалансировать
            // строгую перпендикулярность, накопленную на этапе экспорта.
            float3 T = normalize(input.tangent.xyz - geomNormal * dot(geomNormal, input.tangent.xyz));
            float3 B = cross(geomNormal, T) * input.tangent.w;
            float3x3 TBN = float3x3(T, B, geomNormal);
            // NormalMap.rgb в [0,1] — декодируем в tangent-space вектор
            // [-1,1]. Fallback-текстура (128,128,255) декодируется РОВНО в
            // (0,0,1) — "нормаль не меняется", см. `ensure_flat_normal_texture`.
            float3 tangentNormal = NormalMap.Sample(MaterialSampler, input.uv).rgb * 2.0 - 1.0;
            float3 normal = normalize(mul(tangentNormal, TBN));

            // lightDir хранится как направление "куда светит" (см.
            // TransformConstants::new(): [0,-1,0,0]) — свет приходит с
            // противоположной стороны, поэтому -lightDir.xyz.
            float3 toLight = normalize(-lightDir.xyz);
            float diff = max(dot(normal, toLight), 0.0);
            // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): тени
            // применяются ТОЛЬКО к directional-свету (солнце/луна) — это
            // единственный источник, для которого в этой фазе строится
            // shadow map (см. compute_cascade_view_proj). Point/spot-фонари
            // FirstFires теней пока не отбрасывают — сознательно
            // отложенное расширение (потребовало бы отдельных
            // point/spot shadow map на каждый тенеобразующий фонарь, что
            // на порядок дороже одного directional-прохода) для будущего
            // шага этой же Фазы 6, а не часть минимального рабочего
            // варианта. Ambient-составляющая НЕ затеняется — она по
            // определению не направленная (рассеянный свет неба),
            // затенять её было бы физически неверно (тень стала бы чёрной
            // дырой вместо мягкого рассеянного полумрака).
            //
            // ДОБАВЛЕНО (Cascaded Shadow Maps): view-space Z пикселя — то
            // же соглашение, что и cascade_far_distances в render_frame
            // (engine/mod.rs): расстояние вдоль оси взгляда камеры, не
            // euclidean-дистанция. `view` — матрица камеры (не света),
            // уже доступна в TransformConstants.
            float pixelViewDepth = mul(view, float4(input.worldPos, 1.0)).z;
            float shadowFactor = ComputeShadowFactor(input.worldPos, normal, pixelViewDepth);

            // ДОБАВЛЕНО (Задача #15, normal mapping): metallic/roughness
            // ЭТОГО меша — из MetallicRoughnessMap (R=metallic, G=roughness),
            // если у меша есть собственная карта (hasMrMap>0.5, см.
            // MaterialConstants), иначе из root constants напрямую.
            float metallic = rootMetallic;
            float roughness = rootRoughness;
            if (hasMrMap > 0.5) {
                float2 mr = MetallicRoughnessMap.Sample(MaterialSampler, input.uv).rg;
                metallic = mr.r;
                roughness = mr.g;
            }
            roughness = clamp(roughness, 0.045, 1.0); // 0 даёт NDF-деление на ~0 (зеркальная точка) — числовая защита

            float3 albedoRaw = input.color.rgb * AlbedoMap.Sample(MaterialSampler, input.uv).rgb;
            float3 viewDir = normalize(cameraPos.xyz - input.worldPos);

            // Directional-свет: диффуз (домножен на (1-metallic) —
            // металлы физически не имеют диффузного отклика, вся энергия
            // уходит в specular) + Cook-Torrance GGX-специуляр (см. функции
            // выше). radiancePerNdotL — тот же множитель, что раньше шёл
            // целиком в diffuse, БЕЗ повторного домножения на diff здесь
            // (ComputeSpecularGGX сам домножает на NdotL внутри).
            float3 sunRadiancePerNdotL = lightColor.rgb * lightColor.a * shadowFactor;
            float3 diffuse = albedoRaw * (1.0 - metallic) * sunRadiancePerNdotL * diff;
            float3 specular = ComputeSpecularGGX(normal, viewDir, toLight, albedoRaw, metallic, roughness, sunRadiancePerNdotL);
            float3 ambient = ambientColor.rgb * ambientColor.a * albedoRaw;
            float3 brightness = ambient + diffuse + specular;

            // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): находим ячейку
            // сетки, которой принадлежит этот пиксель, и проверяем ТОЛЬКО
            // фонари этой ячейки — вместо перебора всего видимого списка
            // (что делала Фаза 2). Если пиксель вне границ сетки
            // (gridDimensions == 0, т.е. свет ещё не инициализирован, или
            // worldPos буквально за пределами world_min/world_max) —
            // тихий fallback на 0 дополнительных фонарей, directional-свет
            // выше по-прежнему работает.
            if (gridDimensions.x > 0 && gridDimensions.y > 0 && gridDimensions.z > 0) {
                float cellSize = max(gridWorldMin.w, 0.001);
                float3 localPos = input.worldPos - gridWorldMin.xyz;
                int3 cell = int3(floor(localPos / cellSize));

                if (cell.x >= 0 && cell.y >= 0 && cell.z >= 0 &&
                    (uint)cell.x < gridDimensions.x && (uint)cell.y < gridDimensions.y && (uint)cell.z < gridDimensions.z) {
                    uint cellIndex = (uint)cell.z * gridDimensions.y * gridDimensions.x +
                                      (uint)cell.y * gridDimensions.x +
                                      (uint)cell.x;
                    LightGridCell gridCell = GridCells[cellIndex];

                    for (uint e = 0; e < gridCell.count; e++) {
                        LightGridEntry entry = GridEntries[gridCell.offset + e];
                        GPULight light = Lights[entry.lightIndex];
                        // ИЗМЕНЕНО (Задача #15, normal mapping): вклад
                        // point/spot-фонаря теперь домножается на albedoRaw
                        // ЗДЕСЬ, а не единым умножением в конце функции —
                        // раньше albedo применялось ко ВСЕМУ brightness
                        // разом (ambient+directional+point) одним
                        // умножением в самой последней строке функции; та
                        // схема перестала подходить, когда добавился
                        // GGX-специуляр (specular НЕ должен домножаться на
                        // albedo для диэлектриков — Cook-Torrance уже сам
                        // корректно взвешивает albedo через F0 только для
                        // металлов, см. ComputeSpecularGGX). Результат для
                        // point-фонарей бит-в-бит идентичен старой схеме
                        // (то же самое умножение, просто раньше — в конце).
                        brightness += ComputePointLightContribution(light, input.worldPos, normal) * albedoRaw;
                    }
                }
                // Пиксель вне границ сетки (например очень далёкий объект
                // за world_max) — намеренно 0 фонарных вкладов, а не
                // fallback на полный перебор: за пределами сетки FirstFires
                // всё равно ничего не закуллено в эти координаты.
            } else {
                // Сетка ещё не инициализирована (init_lights() не
                // вызывался, либо LightConfig ещё не применился) — честный
                // fallback: перебор lightCount видимых фонарей напрямую из
                // Lights[], без сетки. Не должен срабатывать в обычном
                // режиме работы движка, но не даёт кадру остаться совсем
                // без фонарей, если сетка почему-то недоступна.
                for (uint i = 0; i < lightCount; i++) {
                    brightness += ComputePointLightContribution(Lights[i], input.worldPos, normal) * albedoRaw;
                }
            }

            // ИЗМЕНЕНО (Задача #15, normal mapping): albedo уже применено
            // выше — к ambient/diffuse явно (см. `ambient`/`diffuse` строки
            // выше, обе домножены на `albedoRaw`) и к каждому point/spot
            // вкладу в циклах выше. `specular` НЕ домножается на albedo
            // (физически корректно — Cook-Torrance сам взвешивает через
            // F0=lerp(0.04,albedo,metallic), см. ComputeSpecularGGX).
            // Вершинный цвет (`input.color`) остаётся отдельным независимым
            // множителем поверх ВСЕГО результата, как и было исторически —
            // единственное отличие от версии до normal mapping: albedo
            // раньше умножался на brightness ЦЕЛИКОМ одной строкой здесь, а
            // теперь распределён по компонентам выше (для diffuse-
            // диэлектриков результат идентичен; отличие только в появлении
            // specular и metallic-взвешивания, которых раньше не было).
            return float4(input.color.rgb * brightness, input.color.a);
        }
        "#;

        self.vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Default shaders compiled (нормали + сетка каллинга + spot-конус фонарей)");
        Ok(())
    }

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
    fn compile_tonemap_shaders(&mut self) -> Result<()> {
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
    fn create_tonemap_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        // ОБНОВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
        // подсветка): теперь 3 смежных SRV в ОДНОЙ таблице — t0
        // (HDRSource, index 0 в renderer.srv_uav_heap), t1 (BloomSource,
        // index 1, зарегистрирован в create_bloom_resources) и t2
        // (VolumetricSource, index 2, зарегистрирован в
        // create_volumetric_final_srv). NumDescriptors=3 означает, что GPU
        // трактует индексы [base..base+3) дескрипторного хипа, начиная с
        // адреса, забинженного через SetGraphicsRootDescriptorTable, как
        // единый смежный диапазон — именно поэтому все три SRV ОБЯЗАНЫ
        // лежать в одном и том же хипе подряд, не в разных хипах.
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

        // Статический point-сэмплер — тонмаппинг читает HDR-таргет 1:1 по
        // пикселю (composite-проход рисует fullscreen-triangle в целевое
        // разрешение back buffer'а, которое СОВПАДАЕТ с разрешением HDR
        // target'а), билинейная/анизотропная фильтрация здесь не нужна и
        // добавила бы только лишнюю смазанность на границах.
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

        // ДОБАВЛЕНО: CBV b0 (TonemapConstants — только float exposure) —
        // как root descriptor, тем же паттерном, что и основной CBV b0 в
        // create_root_signature. Индекс 1 в этом массиве (после
        // descriptor table индекса 0) — см. SetGraphicsRootConstantBufferView
        // в tonemap-проходе ниже.
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
            // Fullscreen-triangle не использует input assembler (нет
            // вершинного/индексного буфера, геометрия генерируется в VS по
            // SV_VertexID) — поэтому, в отличие от основной root signature,
            // здесь НЕ ставим ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT.
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
    fn create_tonemap_pipeline_state(&mut self) -> Result<()> {
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

        // ИСПРАВЛЕНО (тот же паттерн, что и в pso.rs::create_graphics):
        // pRootSignature — ManuallyDrop<Option<ID3D12RootSignature>>,
        // клонирование увеличивает refcount, обязаны сами вручную его
        // опустить после CreateGraphicsPipelineState.
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
            // Нет вершинного буфера/input layout — fullscreen-triangle
            // генерируется целиком внутри VS по SV_VertexID.
            InputLayout: D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: std::ptr::null(),
                NumElements: 0,
            },
            IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            RTVFormats: [DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN],
            // ДОБАВЛЕНО: DSVFormat = UNKNOWN — этот PSO не пишет и не
            // читает глубину вообще (DepthEnable=FALSE выше), не привязан
            // ни к какому DSV во время composite-прохода.
            DSVFormat: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
            // ИСПРАВЛЕНО (моя ошибка в предыдущей правке этой сессии): я
            // был неправ, когда убирал поле `CS` из этого литерала — на
            // реальной машине пользователя компилятор чётко требует его
            // (E0063 "missing field CS"), то есть в фактически
            // используемой версии windows-крейта у
            // D3D12_GRAPHICS_PIPELINE_STATE_DESC ЕСТЬ поле `CS`
            // (bytecode компьют-шейдера — для графического PSO всегда
            // D3D12_SHADER_BYTECODE::default(), т.е. "нет"). См. тот же
            // паттерн в pso.rs::create_graphics(), где это поле всегда
            // присутствовало и никогда не убиралось.
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
    fn compile_bloom_shaders(&mut self) -> Result<()> {
        // "Bright-pass extract": оставляет только то, что ЯРЧЕ порога
        // (после экспозиции) — это то, что физически "светится" (сами
        // источники света, яркие блики), а не вся сцена целиком. Плавный
        // переход через smoothstep вокруг порога — резкий if/else дал бы
        // видимую границу (aliasing) вокруг ярких объектов.
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

        // Разделяемый (separable) Gaussian blur — 9 тапов, стандартные
        // веса биномиального приближения гауссианы. Направление блюра
        // (горизонталь/вертикаль) передаётся через `texel_size`: для
        // горизонтального прохода texel_size.y == 0, для вертикального
        // texel_size.x == 0 — один и тот же шейдер обслуживает оба прохода,
        // не нужно компилировать два почти идентичных варианта.
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
    fn create_bloom_root_signature(&mut self) -> Result<()> {
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
    fn create_bloom_pipeline_states(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.tonemap_vs.as_ref().unwrap(); // общий fullscreen-triangle VS
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

        // Общий шаблон PSO-описания — отличие между extract/blur только в
        // поле PS, поэтому строим один раз и клонируем структуру дважды
        // (D3D12_GRAPHICS_PIPELINE_STATE_DESC не Clone из-за ManuallyDrop
        // полей — поэтому здесь буквально два отдельных литерала, не
        // клонирование, но с одинаковыми остальными полями).
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
                // См. комментарий у первого PSO-литерала выше
                // (create_tonemap_pipeline_state) — поле CS реально нужно.
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
    fn create_bloom_resources(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let bloom_width = (self.width / 2).max(1);
        let bloom_height = (self.height / 2).max(1);

        let texture_a = crate::render::RenderTexture::create_hdr_target(bloom_width, bloom_height)?;
        let texture_b = crate::render::RenderTexture::create_hdr_target(bloom_width, bloom_height)?;

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

        // ДОБАВЛЕНО: SRV на bloom_texture_a ТАКЖЕ регистрируется в
        // `renderer.srv_uav_heap` (индекс 1 — индекс 0 там уже занят HDR
        // SRV, см. Renderer::new) — ФИНАЛЬНЫЙ tonemap-проход читает и HDR,
        // и результат блюра ОДНОЙ дескрипторной таблицей (t0=HDR, t1=Bloom,
        // оба должны быть смежными дескрипторами в ОДНОМ heap — таково
        // требование D3D12 для descriptor table с несколькими диапазонами
        // внутри одного root-параметра). `bloom_srv_heap` (созданный выше,
        // отдельный) используется только МЕЖДУ extract/blur-проходами,
        // где каждый раз биндится ровно одна текстура через свою
        // отдельную таблицу — там смежность с HDR SRV не нужна.
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

        // BloomParams: [threshold, texel_size.x, texel_size.y, unused] —
        // порог по умолчанию 1.0 (в единицах ЭКСПОНИРОВАННОГО HDR-цвета:
        // так как extract-проход читает `HDRSource` ДО тонмаппинга, но
        // после того как основной draw pass уже записал реальные яркости,
        // 1.0 — это "на грани стандартного динамического диапазона", что
        // на практике означает "светятся только сами источники света и
        // явные блики", не вся ярко освещённая геометрия целиком.
        // texel_size пересчитывается перед каждым конкретным проходом в
        // render_frame (разный для extract/blur-H/blur-V — там разные
        // исходные текстуры), здесь только начальное значение-заглушка.
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

    /// ОБНОВЛЕНО (каскадные тени / CSM — расширение Фазы 6): создаёт
    /// `NUM_CASCADES` depth-таргетов shadow map (по одному на каскад), их
    /// DSV (для shadow-прохода — запись глубины) и SRV, все SМЕЖНЫЕ в
    /// одном shader-visible heap (для основного 3D-прохода — чтение/
    /// сравнение глубины через PCF, см. подробности у поля
    /// `shadow_srv_heap`). Вызывается один раз в init(), НЕ пересоздаётся
    /// при resize (см. подробное объяснение у полей `shadow_maps` и
    /// `SHADOW_MAP_RESOLUTION` — разрешение shadow map не зависит от
    /// размера окна).
    fn create_shadow_resources(&mut self) -> Result<()> {
        let dsv_heap = crate::heap::DescriptorHeap::create_dsv_heap(NUM_CASCADES as u32)?;
        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(NUM_CASCADES as u32)?;

        let dsv_size = {
            let state = STATE.lock().unwrap();
            state.dsv_descriptor_size
        };
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        for cascade in 0..NUM_CASCADES {
            let shadow_map = crate::render::RenderTexture::create_shadow_map(SHADOW_MAP_RESOLUTION)?;

            let dsv = crate::heap::DescriptorHeap::get_cpu_handle(&dsv_heap, cascade as u32, dsv_size);
            shadow_map.create_dsv(dsv)?;

            let srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, cascade as u32, cbv_srv_uav_size);
            shadow_map.create_shadow_srv(srv_cpu)?;

            self.shadow_maps[cascade] = Some(shadow_map);
            self.shadow_dsvs[cascade] = dsv;
        }

        // GPU-адрес НАЧАЛА (индекс 0) descriptor table — шейдер видит
        // t3=каскад0, t4=каскад1, t5=каскад2 как смежные дескрипторы,
        // начиная с этого адреса (см. подробности у поля shadow_srv_gpu).
        let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        self.shadow_dsv_heap = Some(dsv_heap);
        self.shadow_srv_heap = Some(srv_heap);
        self.shadow_srv_gpu = srv_gpu;

        println!(
            "[ENGINE] ✓ Shadow map resources created: {} каскадов по {}x{}",
            NUM_CASCADES, SHADOW_MAP_RESOLUTION, SHADOW_MAP_RESOLUTION
        );
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): вершинный
    /// шейдер shadow-прохода. Единственная задача — записать глубину
    /// объекта С ТОЧКИ ЗРЕНИЯ СВЕТА в shadow map; пиксельного шейдера НЕТ
    /// ВООБЩЕ (D3D12 разрешает PSO без PS для depth-only рендеринга — GPU
    /// сам записывает глубину в DSV по растеризованным треугольникам, а
    /// цвет никуда не пишется, т.к. NumRenderTargets=0, см.
    /// `create_shadow_pipeline_state`). Раздельный CBV (не переиспользует
    /// TransformConstants основного прохода) — здесь нужна только ОДНА
    /// матрица (model * light_view_proj), без камеры/света/сетки каллинга,
    /// которые этому проходу не нужны вообще.
    fn compile_shadow_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        cbuffer ShadowConstants : register(b0) {
            float4x4 modelLightViewProj;
        };

        struct VS_INPUT {
            float4 pos : POSITION;
            float3 normal : NORMAL;
            float4 color : COLOR;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            output.pos = mul(modelLightViewProj, input.pos);
            return output;
        }
        "#;

        self.shadow_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        println!("[ENGINE] ✓ Shadow shaders compiled (depth-only, без PS)");
        Ok(())
    }

    /// Отдельная root signature shadow-прохода: ОДИН CBV (b0) — матрица
    /// model*light_view_proj, ничего больше (нет SRV фонарей/сетки, нет
    /// сэмплеров — этому проходу они не нужны).
    fn create_shadow_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
            },
        ];

        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
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
                    eprintln!("Shadow root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.shadow_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Shadow root signature created (CBV b0, только матрица)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): PSO
    /// shadow-прохода — depth-only, без PS, без RTV вообще
    /// (NumRenderTargets=0). Input layout ОБЯЗАН совпадать с основным 3D
    /// PSO (POSITION/NORMAL/COLOR) — рисуется ТА ЖЕ геометрия (те же
    /// вершинные/индексные буферы), просто другим шейдером/матрицей.
    ///
    /// DepthBias/SlopeScaledDepthBias вместо (или в дополнение к)
    /// шейдерного shadow_bias (см. TransformConstants) — растеризатор
    /// сдвигает записываемую глубину аппаратно, что везде считается
    /// стандартной практикой против "shadow acne" (самозатенение
    /// поверхности из-за конечной точности глубины). Используем оба
    /// механизма вместе: аппаратный bias здесь — общий, грубый сдвиг,
    /// шейдерный bias в основном PS — по нормали, точнее компенсирует
    /// наклонные поверхности (аппаратный slope-scaled bias частично
    /// решает то же самое, но нормаль-based добавка в PS даёт больше
    /// контроля на пологих углах).
    fn create_shadow_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.shadow_vs.as_ref().unwrap();
        let root_sig = self.shadow_root_signature.as_ref().unwrap();
        let device = crate::get_device()?;

        // ТОЧНАЯ копия input layout из pso.rs::create_graphics — тот же
        // Vertex-формат (POSITION float4@0, NORMAL float3@16, COLOR
        // float4@28), т.к. рисуется одна и та же геометрия.
        //
        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): shadow VS
        // (compile_shadow_shaders) читает только POSITION — TEXCOORD0@44
        // (новое поле `uv` в `engine::Vertex`) здесь СОЗНАТЕЛЬНО не
        // объявлен. Это корректно и безопасно: D3D12 input layout обязан
        // описывать только элементы, которые реально ЧИТАЕТ вершинный
        // шейдер этого PSO — размер шага между вершинами (`StrideInBytes`
        // в D3D12_VERTEX_BUFFER_VIEW, см. shadow_jobs draw loop в
        // render_frame) берётся из VBV независимо от количества элементов
        // layout, поэтому увеличенный `Vertex::STRIDE` (теперь включает uv)
        // корректно применяется к ТОЙ ЖЕ геометрии в обоих проходах, даже
        // когда shadow-проходу часть полей не нужна.
        let input_elements = [
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("NORMAL"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 16,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 28,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];
        let input_layout = D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        };

        let rasterizer = D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: FALSE,
            // Аппаратный depth bias — см. комментарий у функции. Значения
            // подобраны консервативно (небольшой сдвиг): слишком большой
            // bias отрывает тень от объекта ("peter panning"), слишком
            // маленький не убирает acne.
            DepthBias: 5000,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 2.0,
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
            DepthEnable: TRUE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
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
            // Намеренно ПУСТОЙ PS — depth-only проход, D3D12 явно
            // разрешает PSO без пиксельного шейдера, когда
            // NumRenderTargets=0 (только запись в DSV).
            PS: D3D12_SHADER_BYTECODE::default(),
            DS: D3D12_SHADER_BYTECODE::default(),
            HS: D3D12_SHADER_BYTECODE::default(),
            GS: D3D12_SHADER_BYTECODE::default(),
            StreamOutput: D3D12_STREAM_OUTPUT_DESC::default(),
            BlendState: blend_desc,
            SampleMask: u32::MAX,
            RasterizerState: rasterizer,
            DepthStencilState: depth_stencil,
            InputLayout: input_layout,
            IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 0,
            RTVFormats: [DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN, DXGI_FORMAT_UNKNOWN],
            DSVFormat: DXGI_FORMAT_D32_FLOAT,
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

        match result {
            Ok(pso) => {
                self.shadow_pipeline_state = Some(pso);
                println!("[ENGINE] ✓ Shadow pipeline state created (depth-only, DSVFormat=D32_FLOAT)");
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] ✗ Failed to create shadow PSO: {:?}", e);
                Err(e)
            }
        }
    }

    // =========================================================================
    // ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-подсветка)
    // =========================================================================

    /// Создаёт SRV основного depth-таргета (`renderer.depth_stencil`) — см.
    /// подробное обоснование TYPELESS-перевода этого ресурса у
    /// `RenderTexture::create_depth_stencil` в render.rs. Вызывается ПОСЛЕ
    /// того, как renderer уже создан (в отличие от shadow-ресурсов —
    /// depth-таргет живёт внутри `Renderer`, а не отдельно).
    fn create_depth_srv_resources(&mut self) -> Result<()> {
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_depth_srv_resources() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;

        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(1)?;
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        renderer.depth_stencil.create_depth_srv(srv_cpu)?;

        self.depth_srv_heap = Some(srv_heap);
        self.depth_srv_gpu = srv_gpu;

        println!("[ENGINE] ✓ Depth SRV created (для volumetric raymarch)");
        Ok(())
    }

    /// Компилирует вершинный (переиспользует ту же fullscreen-triangle
    /// технику, что и tonemap/bloom — см. `self.tonemap_vs`) и пиксельный
    /// шейдеры volumetric raymarch-прохода.
    ///
    /// Идея: для каждого экранного пикселя восстанавливаем его мировую
    /// позицию из сохранённой глубины (через инвертированную view-proj
    /// матрицу камеры — тот же математический приём, что уже используется
    /// в `compute_cascade_view_proj` для обратного преобразования NDC-
    /// углов фрустума в мировые координаты), затем идём МАЛЫМИ шагами
    /// вдоль луча камера->этот пиксель и на каждом шаге спрашиваем shadow
    /// map: "виден ли этот участок воздуха солнцу, или он в тени?" —
    /// сумма "видимых" шагов (с весом, убывающим к краю дальности) даёт
    /// яркость god ray в этом пикселе. Классический screen-space
    /// volumetric lighting приём (та же идея, что "God Rays"/"Crepuscular
    /// Rays" в играх) — не физически точная симуляция рассеяния в
    /// атмосфере, а дешёвая, устойчивая по кадрам аппроксимация, которая
    /// на зафиксированном минимуме железа (RTX 3050 8GB) должна оставаться
    /// в разумном бюджете кадра благодаря half-res выполнению (см.
    /// `create_volumetric_resources`) и небольшому фиксированному числу
    /// шагов (NUM_STEPS ниже).
    fn compile_volumetric_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };
        // Стандартный fullscreen-triangle без вершинного/индексного буфера
        // — та же техника, что уже используется tonemap/bloom-проходами
        // (см. compile_tonemap_shaders).
        VS_OUTPUT main(uint id : SV_VertexID) {
            VS_OUTPUT output;
            output.uv = float2((id << 1) & 2, id & 2);
            output.pos = float4(output.uv.x * 2.0 - 1.0, 1.0 - output.uv.y * 2.0, 0.0, 1.0);
            return output;
        }
        "#;

        let ps_source = r#"
        Texture2D DepthBuffer : register(t0);
        Texture2D ShadowMap : register(t1);
        SamplerState PointSampler : register(s0);
        SamplerComparisonState ShadowSampler : register(s1);

        cbuffer VolumetricParams : register(b0) {
            float4x4 invViewProj;
            float4x4 lightViewProj;
            float3   cameraPos;
            float    intensity;
            float3   lightDir;   // направление, КУДА летит свет (как TransformConstants.light_dir)
            float    _padding0;
            float3   lightColor;
            float    maxDistance; // дальше этого расстояния от камеры raymarch не идёт
        };

        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float2 uv : TEXCOORD0;
        };

        // 3x3 PCF, идентичный основному пиксельному шейдеру (см.
        // SampleShadowPCF в compile_default_shaders) — используем тот же
        // приём, чтобы шаги raymarch'а не давали "рваный" резкий край
        // между освещённым и затенённым воздухом.
        float SampleShadowPCF(float3 shadowCoord) {
            float shadow = 0.0;
            float texelSize = 1.0 / 2048.0; // SHADOW_MAP_RESOLUTION — см. engine/mod.rs
            [unroll]
            for (int x = -1; x <= 1; x++) {
                [unroll]
                for (int y = -1; y <= 1; y++) {
                    float2 offset = float2(x, y) * texelSize;
                    shadow += ShadowMap.SampleCmpLevelZero(ShadowSampler, shadowCoord.xy + offset, shadowCoord.z);
                }
            }
            return shadow / 9.0;
        }

        static const int NUM_STEPS = 24;

        float4 main(PS_INPUT input) : SV_TARGET {
            float depth = DepthBuffer.Sample(PointSampler, input.uv).r;

            // depth == 1.0 (дальняя плоскость очистки, см.
            // create_depth_stencil::clear_value) означает "нет геометрии в
            // этом пикселе — небо/пустота". Raymarch в этом случае идёт до
            // maxDistance вдоль луча вместо до реальной геометрии — иначе
            // god rays никогда бы не были видны на фоне неба, что не
            // соответствует тому, как они выглядят в реальности (свет,
            // рассеянный в воздухе МЕЖДУ камерой и любой преградой,
            // включая "нет преграды вообще").
            float ndcX = input.uv.x * 2.0 - 1.0;
            float ndcY = 1.0 - input.uv.y * 2.0;

            float3 rayEnd;
            if (depth >= 0.9999) {
                // Точка на дальней плоскости отсечения в направлении этого
                // пикселя — используем как временную "конечную точку" луча,
                // затем всё равно ограничиваем маршем через maxDistance
                // ниже.
                float4 farClip = mul(invViewProj, float4(ndcX, ndcY, 1.0, 1.0));
                rayEnd = farClip.xyz / farClip.w;
            } else {
                float4 worldPos = mul(invViewProj, float4(ndcX, ndcY, depth, 1.0));
                rayEnd = worldPos.xyz / worldPos.w;
            }

            float3 rayDir = rayEnd - cameraPos;
            float rayLength = length(rayDir);
            rayDir /= max(rayLength, 0.0001);
            rayLength = min(rayLength, maxDistance);

            float stepSize = rayLength / float(NUM_STEPS);
            // Небольшой случайный сдвиг стартовой точки шага (по
            // экранным координатам, детерминированный — не зависит от
            // кадра) убирает видимые полосы-артефакты (banding) от
            // слишком малого числа шагов, "размазывая" их в шум, который
            // визуально гораздо менее заметен, чем регулярные полосы.
            float jitter = frac(sin(dot(input.uv, float2(12.9898, 78.233))) * 43758.5453);

            float accumulated = 0.0;
            for (int i = 0; i < NUM_STEPS; i++) {
                float t = (float(i) + jitter) * stepSize;
                float3 samplePos = cameraPos + rayDir * t;

                float4 lightSpacePos = mul(lightViewProj, float4(samplePos, 1.0));
                if (lightSpacePos.w > 0.0001) {
                    float3 shadowCoord = lightSpacePos.xyz / lightSpacePos.w;
                    float2 shadowUV = float2(shadowCoord.x * 0.5 + 0.5, 1.0 - (shadowCoord.y * 0.5 + 0.5));
                    if (shadowUV.x >= 0.0 && shadowUV.x <= 1.0 && shadowUV.y >= 0.0 && shadowUV.y <= 1.0 && shadowCoord.z >= 0.0 && shadowCoord.z <= 1.0) {
                        accumulated += SampleShadowPCF(float3(shadowUV, shadowCoord.z));
                    } else {
                        // Вне shadow map (например, очень далеко от камеры,
                        // за пределами frustum-fitted ортопроекции) — по
                        // умолчанию считаем ОСВЕЩЁННЫМ (тот же safe fallback,
                        // что и border-цвет compare-сэмплера в основном
                        // пиксельном шейдере), чтобы god rays не обрывались
                        // резкой чёрной границей на краю shadow-объёма.
                        accumulated += 1.0;
                    }
                }
            }
            accumulated /= float(NUM_STEPS);

            // Дополнительно взвешиваем по тому, насколько луч вообще
            // направлен "к камере" от солнца (сильнее god rays видны,
            // когда смотришь примерно НА солнце, а не спиной к нему) —
            // стандартный приём, делающий эффект направленным, а не
            // равномерным туманом.
            float sunFacing = saturate(dot(-rayDir, normalize(lightDir)) * 0.5 + 0.5);

            float3 result = lightColor * accumulated * intensity * (0.3 + 0.7 * sunFacing);
            return float4(result, 1.0);
        }
        "#;

        self.volumetric_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.volumetric_ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Volumetric shaders compiled (screen-space raymarch, {} шагов)", 24);
        Ok(())
    }

    /// Root signature volumetric raymarch-прохода: descriptor table с
    /// ДВУМЯ смежными SRV (t0 = depth, t1 = shadow map — оба должны быть
    /// смежными дескрипторами в одном heap, см. `create_volumetric_resources`),
    /// точечный сэмплер (s0, для depth) + comparison-сэмплер (s1, для
    /// shadow map — идентичен по параметрам сэмплеру s0 основного
    /// прохода, см. `create_root_signature`), и один CBV (b0) с
    /// параметрами (см. `VolumetricParams` в шейдере выше).
    fn create_volumetric_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 2, // t0 = depth, t1 = shadow map (смежные)
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let point_sampler = D3D12_STATIC_SAMPLER_DESC {
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

        // Идентичен сэмплеру s0 основного 3D-прохода (см.
        // create_root_signature) — тот же comparison-сэмплер для
        // shadow map, но под другим регистром (s1, не s0 — этот root
        // signature уже занял s0 под point_sampler выше).
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
            ShaderRegister: 1,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        let static_samplers = [point_sampler, shadow_sampler];

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
            NumStaticSamplers: static_samplers.len() as u32,
            pStaticSamplers: static_samplers.as_ptr(),
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
                    eprintln!("Volumetric root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.volumetric_root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Volumetric root signature created (SRV table t0/t1 + point sampler s0 + comparison sampler s1 + CBV b0)");
        Ok(())
    }

    /// PSO volumetric-прохода — та же форма, что и bloom/tonemap (нет
    /// input layout, нет depth-теста, полноэкранный треугольник), RTV
    /// формат HDR (float, чтобы не терять яркость god rays до тонмаппинга,
    /// как и весь остальной свет в этом движке начиная с Фазы 5).
    fn create_volumetric_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let vs = self.volumetric_vs.as_ref().unwrap();
        let ps = self.volumetric_ps.as_ref().unwrap();
        let root_sig = self.volumetric_root_signature.as_ref().unwrap();
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
        self.volumetric_pipeline_state = Some(pso);

        println!("[ENGINE] ✓ Volumetric pipeline state created");
        Ok(())
    }

    /// Создаёт half-res volumetric render target + его RTV/SRV + SRV heap
    /// с depth (t0) и shadow map (t1) СМЕЖНЫМИ дескрипторами (обязательное
    /// требование D3D12 для одной descriptor table с одним диапазоном на
    /// несколько ресурсов, см. `create_volumetric_root_signature`) + CBV
    /// buffer под VolumetricParams. Вызывается ПОСЛЕ `create_shadow_resources`
    /// и `create_depth_srv_resources` (нужны их результаты — self.shadow_maps,
    /// self.depth_srv_heap — чтобы скопировать соответствующие дескрипторы
    /// в новый смежный heap).
    fn create_volumetric_resources(&mut self) -> Result<()> {
        let device = crate::get_device()?;

        let vol_width = (self.width / 2).max(1);
        let vol_height = (self.height / 2).max(1);

        let texture = crate::render::RenderTexture::create_hdr_target(vol_width, vol_height)?;

        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(1)?;
        // 3 слота: 0 = depth (для raymarch-прохода), 1 = shadow map (для
        // raymarch-прохода, смежный с 0), 2 = финальный volumetric-таргет
        // (для tonemap composite-прохода — читается ОТДЕЛЬНО, другой root
        // signature, смежность с 0/1 ему не нужна, но переиспользуем один
        // heap ради простоты, т.к. дескрипторы всё равно копируются явно).
        let srv_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(3)?;

        let rtv_size = {
            let state = STATE.lock().unwrap();
            state.rtv_descriptor_size
        };
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        let rtv = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, 0, rtv_size);
        texture.create_rtv(rtv)?;

        // Копируем depth SRV (уже созданный в create_depth_srv_resources,
        // отдельный heap) и shadow map SRV (уже созданный в
        // create_shadow_resources) в СМЕЖНЫЕ слоты 0/1 этого нового heap —
        // CopyDescriptorsSimple, а не пересоздание вида с нуля, экономит
        // необходимость дублировать D3D12_SHADER_RESOURCE_VIEW_DESC здесь.
        //
        // ИЗМЕНЕНО (Cascaded Shadow Maps): `shadow_srv_heap` теперь хранит
        // NUM_CASCADES смежных SRV (индексы 0..NUM_CASCADES-1, по одному на
        // каскад). Volumetric raymarch — экранный полноэкранный проход БЕЗ
        // per-пиксельного выбора каскада (в отличие от основного
        // пиксельного шейдера, см. compile_default_shaders), поэтому
        // сознательно берём только каскад 0 (ближний/самый плотный) —
        // индекс 0 в src_shadow_heap. God rays физически наиболее заметны
        // и наиболее ценны именно вблизи камеры, где каскад 0 даёт лучшее
        // качество; расхождение с дальними каскадами на большом удалении
        // для screen-space god ray приближения незначительно.
        let cascade_for_volumetric: u32 = 0;
        let dst0 = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 0, cbv_srv_uav_size);
        let dst1 = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 1, cbv_srv_uav_size);
        let src_depth_heap = self.depth_srv_heap.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_resources() called before create_depth_srv_resources()");
            Error::from_hresult(HRESULT(1))
        })?;
        let src_shadow_heap = self.shadow_srv_heap.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_resources() called before create_shadow_resources()");
            Error::from_hresult(HRESULT(1))
        })?;
        let src_depth_cpu = crate::heap::DescriptorHeap::get_cpu_handle(src_depth_heap, 0, cbv_srv_uav_size);
        let src_shadow_cpu = crate::heap::DescriptorHeap::get_cpu_handle(src_shadow_heap, cascade_for_volumetric, cbv_srv_uav_size);
        unsafe {
            device.CopyDescriptorsSimple(1, dst0, src_depth_cpu, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            device.CopyDescriptorsSimple(1, dst1, src_shadow_cpu, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        }

        let srv2_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_heap, 2, cbv_srv_uav_size);
        let srv2_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 2, cbv_srv_uav_size);
        texture.create_srv(srv2_cpu)?;

        let raymarch_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_heap, 0, cbv_srv_uav_size);

        self.volumetric_texture = Some(texture);
        self.volumetric_rtv = rtv;
        self.volumetric_srv_gpu_final = srv2_gpu;
        self.volumetric_rtv_heap = Some(rtv_heap);
        self.volumetric_srv_heap = Some(srv_heap);
        self.volumetric_srv_gpu_raymarch = raymarch_gpu;

        // VolumetricParams — размер должен быть кратен 256 (CBV alignment).
        // mat4x4 (64) + mat4x4 (64) + vec3+f32 (16) + vec3+f32 (16) = 160
        // байт "полезных" данных — создаём буфер на 256 (один слот,
        // проход не требует нескольких кадров в полёте одновременно, т.к.
        // не участвует в constant-buffer-per-draw паттерне остальных
        // проходов — здесь ровно один draw на кадр).
        let params_cb = Buffer::create_constant_buffer(256)?;
        self.volumetric_constant_buffer = Some(params_cb);

        println!(
            "[ENGINE] ✓ Volumetric resources created: {}x{} half-res target",
            vol_width, vol_height
        );

        // ДОБАВЛЕНО: регистрируем финальный volumetric SRV ТАКЖЕ в
        // `renderer.srv_uav_heap` (индекс 2 — 0=HDR, 1=bloom_a, см.
        // create_bloom_resources) — по той же причине, что и bloom SRV
        // там же: финальный tonemap composite-проход читает HDR/bloom/
        // volumetric ОДНОЙ смежной descriptor table (t0/t1/t2, см.
        // create_tonemap_root_signature), значит все три обязаны лежать
        // подряд в одном and том же shader-visible heap.
        self.create_volumetric_final_srv()?;

        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
    /// подсветка): регистрирует SRV volumetric-таргета в индексе 2
    /// `renderer.srv_uav_heap` — вынесено в отдельный метод (а не
    /// встроено прямо в `create_volumetric_resources`), т.к. требует
    /// одновременного заимствования `self.renderer` (по ссылке) и
    /// `self.volumetric_texture` (тоже по ссылке) — оба READ-ONLY на этом
    /// шаге, так что заимствование безопасно; вынесено отдельным методом
    /// ради явного разделения "создание ресурса" / "регистрация в чужом
    /// хипе" (`create_bloom_resources` делает то же самое инлайн, без
    /// разделения — здесь тот же эффект достигается отдельным вызовом).
    fn create_volumetric_final_srv(&mut self) -> Result<()> {
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_final_srv() called before renderer initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        let texture = self.volumetric_texture.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: create_volumetric_final_srv() called before volumetric_texture created");
            Error::from_hresult(HRESULT(1))
        })?;
        let dst = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 2, cbv_srv_uav_size);
        texture.create_srv(dst)?;
        Ok(())
    }

    fn create_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        // ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): второй root-параметр
        // — SRV (register t0), root descriptor (БЕЗ дескрипторной таблицы/
        // хипа — как и у существующего CBV b0). Через него пиксельный
        // шейдер получает StructuredBuffer<GPULight> со списком фонарей,
        // уже отфильтрованным FirstFires (LightPlugin::cull). Порядок
        // важен: индекс 0 = CBV (см. SetGraphicsRootConstantBufferView(0,
        // ...) в render_frame), индекс 1 = SRV t0 (список фонарей, см.
        // SetGraphicsRootShaderResourceView(1, ...) там же). Индексы 2 и 3
        // ДОБАВЛЕНЫ в Фазе 3 плана по реализму/фонарям — SRV t1
        // (GridCells) и t2 (GridEntries), пространственная сетка
        // FirstFires. Порядок в этом массиве обязан совпадать с индексами
        // в SetGraphicsRootShaderResourceView(N, ...) в render_frame — все
        // 4 места (root signature, HLSL register(tN), Rust root index,
        // сама структура данных) должны быть синхронны вручную, ABI между
        // ними не проверяется компилятором.
        //
        // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): индекс 4 —
        // ЕДИНСТВЕННЫЙ root-параметр этого сигнатуры, который является
        // ДЕСКРИПТОРНОЙ ТАБЛИЦЕЙ (не root-дескриптором, как остальные) —
        // root SRV/CBV поддерживают только "сырые"/структурированные
        // буферы, но НЕ полноценные текстуры с фильтрацией (D3D12 не
        // разрешает Texture2D через root descriptor). Shadow map — именно
        // текстура (см. `RenderTexture::create_shadow_map`), поэтому ей
        // обязательно нужна дескрипторная таблица, указывающая в
        // shader-visible heap (см. `shadow_srv_heap`/`SetDescriptorHeaps`
        // в render_frame перед основным draw pass'ом). Статический
        // сэмплер сравнения (comparison sampler, s0) — отдельно ниже,
        // используется HLSL-функцией SampleCmpLevelZero для аппаратного
        // PCF (несколько сравнений глубины в радиусе фильтра ОДНИМ
        // вызовом, вместо ручного цикла по тексклям).
        //
        // ИЗМЕНЕНО (Cascaded Shadow Maps): NumDescriptors 1 -> NUM_CASCADES
        // — вместо одной shadow map теперь t3..t(3+NUM_CASCADES-1)
        // покрывают NUM_CASCADES смежных SRV в ОДНОЙ таблице (они уже
        // выложены подряд в `shadow_srv_heap`, см. `create_shadow_resources`
        // — `shadow_srv_gpu` указывает на дескриптор каскада 0, дальше
        // driver сам считает смещения по OffsetInDescriptorsFromTableStart
        // для каскадов 1..NUM_CASCADES).
        let shadow_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: NUM_CASCADES as u32,
            BaseShaderRegister: 3,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): ВТОРАЯ
        // дескрипторная таблица root signature — ОДИН SRV, register t6
        // (следующий свободный после t3..t5 — NUM_CASCADES=3 shadow-каскадов
        // выше). Это albedo-текстура ТЕКУЩЕГО рисуемого меша — в отличие от
        // shadow-таблицы (общая на весь кадр, биндится один раз), эта
        // таблица перебиндивается ПЕРЕД КАЖДЫМ Draw в основном цикле
        // рендера (см. render_frame) на GPU-адрес КОНКРЕТНОГО слота
        // ЭТОГО меша внутри `AlkashEngine::shadow_srv_heap` (material-часть
        // хипа, слоты NUM_CASCADES.. — см. `ensure_material_srv_capacity`;
        // ОБЯЗАНА жить в ТОМ ЖЕ хипе, что и shadow-каскады — аппаратно
        // одновременно можно забиндить не более ОДНОГО shader-visible
        // CBV_SRV_UAV хипа) — то есть указывает КУДА в heap смотреть, а
        // не хранит саму текстуру. Ровно ОДИН SRV в таблице (не N по числу
        // мешей) — учитывая, что таблица перебиндивается на каждый Draw, а
        // не один раз, множественные
        // диапазоны здесь не нужны и не помогли бы (D3D12 всё равно требует
        // отдельного SetGraphicsRootDescriptorTable на каждое изменение
        // смещения).
        let material_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 6,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        // ДОБАВЛЕНО (Задача #15, normal mapping): ТРЕТЬЯ и ЧЕТВЁРТАЯ
        // дескрипторные таблицы — normal map (t7) и metallic-roughness map
        // (t8). ОТДЕЛЬНЫЕ таблицы (не один диапазон NumDescriptors=3 на
        // t6..t8) — намеренно: слоты трёх карт ОДНОГО меша в
        // `shadow_srv_heap` НЕ обязаны лежать подряд (см.
        // `register_material_texture`/`load_or_get_texture_srv` — каждая
        // карта регистрируется и кэшируется независимо, разные меши могут
        // делить, например, одну и ту же normal map, но разные albedo), а
        // непрерывный диапазон дескрипторов в root signature ТРЕБУЕТ, чтобы
        // соответствующие им дескрипторы в куче тоже шли подряд в ТОМ ЖЕ
        // порядке. Три независимые таблицы (как и albedo выше) снимают это
        // требование — каждая просто указывает на свой, отдельно вычисленный
        // GPU-адрес перед Draw (см. render_frame).
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

        // ComparisonFunc: LESS — "тексель shadow map (глубина от источника
        // света) МЕНЬШЕ, чем сравниваемое значение (глубина текущего
        // пикселя от источника света)" => пиксель БЛИЖЕ к свету, чем то,
        // что shadow map видела на этом месте => пиксель НЕ в тени.
        // Filter MIN_MAG_LINEAR_MIP_POINT + comparison — аппаратный PCF на
        // соседних 2x2 текселях ОДНИМ сэмплом (билинейная интерполяция
        // РЕЗУЛЬТАТОВ сравнения, а не самих глубин — единственный
        // корректный способ линейно фильтровать сравнение глубины).
        let shadow_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            MipLODBias: 0.0,
            MaxAnisotropy: 0,
            ComparisonFunc: D3D12_COMPARISON_FUNC_LESS,
            // Border = белый (1.0 = максимальная глубина) — координаты ВНЕ
            // shadow map (пиксель за пределами ортографического объёма
            // света) сэмплируют "максимально далеко", что при сравнении
            // LESS даёт "не в тени" — безопасный fallback вместо
            // произвольного тайлинга/клэмпинга по краю карты.
            BorderColor: D3D12_STATIC_BORDER_COLOR_OPAQUE_WHITE,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 0,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        // ДОБАВЛЕНО (Задача #15): линейный (билинейный + трилинейный по
        // мипам, WRAP по обеим осям — стандартное поведение для albedo-
        // текстур на геометрии произвольного размера, тайлящихся по
        // поверхности) статический sampler, register s1 — ОТДЕЛЬНЫЙ от
        // shadow-компарисон-сэмплера (s0, ComparisonFunc задействован
        // только у него; обычный `Texture2D.Sample` с compare-сэмплером
        // недопустим в HLSL). WRAP (не CLAMP, как у tonemap-прохода выше) —
        // albedo-текстуры типично тайлятся по UV > 1.0 (например бесшовная
        // текстура асфальта на большом полу), CLAMP растянул бы крайний
        // тексель, а не повторил текстуру.
        let material_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
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
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &shadow_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            // ДОБАВЛЕНО (Задача #15): root-параметр 5 — albedo-текстура
            // текущего меша (register t6, см. material_srv_range выше).
            // Порядок в этом массиве обязан совпадать с индексом,
            // передаваемым в SetGraphicsRootDescriptorTable(5, ...) в
            // render_frame (ТОЧНО так же, как для индекса 4/shadow выше).
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
            // ДОБАВЛЕНО (Задача #15, normal mapping): root-параметр 6 —
            // normal map текущего меша (register t7, см. normal_srv_range
            // выше). Индекс в этом массиве ОБЯЗАН совпадать с индексом,
            // передаваемым в SetGraphicsRootDescriptorTable(6, ...) в
            // render_frame.
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
            // ДОБАВЛЕНО (Задача #15, normal mapping): root-параметр 7 —
            // metallic-roughness map текущего меша (register t8, см.
            // mr_srv_range выше). Индекс 7 = SetGraphicsRootDescriptorTable(7, ...).
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
            // ДОБАВЛЕНО (Задача #15, normal mapping): root-параметр 8 —
            // 32-битные root constants (НЕ дескрипторная таблица, значения
            // лежат прямо в root arguments команды Draw — самый дешёвый
            // способ передать несколько float, дешевле отдельного CBV для
            // такого маленького объёма данных). 4 значения (см.
            // MaterialConstants в HLSL): metallic, roughness — скаляры
            // материала текущего меша (см. `Mesh::material_metallic`/
            // `material_roughness`); hasMrMap — явный флаг "есть ли у
            // этого меша собственная MetallicRoughnessMap" (SRV сам по себе
            // не несёт этого признака — см. подробный комментарий у
            // MaterialConstants в HLSL); последнее значение — padding до
            // кратности 4 (не обязателен для D3D12 per se, но избегает
            // потенциальных проблем с выравниванием, наблюдаемых на part
            // некоторых драйверов при нечётном числе root constants).
            // register b1 — b0 уже занят TransformConstants (root-параметр 0).
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
        ];

        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: static_samplers.len() as u32,
            pStaticSamplers: static_samplers.as_ptr(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()`.
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

        println!("[ENGINE] ✓ Root signature created (CBV b0 + SRV t0 фонари + SRV t1/t2 сетка каллинга + SRV table t3..t5 shadow map + SRV table t6 albedo + comparison sampler s0 + linear sampler s1)");
        Ok(())
    }

    fn create_pipeline_state(&mut self) -> Result<()> {
        let vs = self.vs.as_ref().unwrap();
        let ps = self.ps.as_ref().unwrap();
        let root_sig = self.root_signature.as_ref().unwrap();

        // ИСПРАВЛЕНО (найдено через D3D12 debug layer на реальной машине
        // пользователя — точный текст ошибки: "DrawIndexedInstanced: The
        // render target format in slot 0 does not match that specified by
        // the current pipeline state. (pipeline state = R8G8B8A8_UNORM,
        // RTV = ...)"). Раньше здесь было DXGI_FORMAT_R8G8B8A8_UNORM —
        // формат back buffer'а, актуальный ДО Фазы 5. Но начиная с Фазы 5
        // (HDR/bloom/tonemap) основной проход рендерит не прямо в back
        // buffer, а в renderer.hdr_target (R16G16B16A16_FLOAT) — тонемаппинг
        // потом сводит его к back buffer'у отдельным проходом. PSO для
        // основного прохода обязан объявлять ТОТ ЖЕ формат, что и RTV, в
        // который реально идёт отрисовка — иначе D3D12 валидация (и на
        // некоторых драйверах реальное поведение) считает это ошибкой.
        // Формат должен совпадать с DXGI_FORMAT_R16G16B16A16_FLOAT,
        // используемым в create_hdr_target()/Renderer::new() для
        // hdr_target.
        let pso = PipelineState::create_graphics(
            vs, ps, root_sig,
            Vertex::STRIDE,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
        )?;

        self.pipeline_state = Some(pso);
        println!("[ENGINE] ✓ Pipeline state created (RTV format = R16G16B16A16_FLOAT, matches HDR target)");
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): view-proj
    /// матрица directional-света ("солнца"), с точки зрения которой
    /// рисуется shadow map.
    ///
    /// Наивный подход — просто взять фиксированный огромный ортографический
    /// объём вокруг всей сцены — тратит разрешение shadow map (2048x2048)
    /// на площадь, часть которой камера может вообще не видеть, отчего
    /// тени рядом с камерой становятся грубыми/ступенчатыми. Вместо этого
    /// объём подгоняется под ВИДИМЫЙ frustum камеры на каждый кадр:
    /// 1. Берём 8 углов усечённой пирамиды камеры в мировых координатах
    ///    (near/far плоскости camera.near/camera.far).
    /// 2. Строим view-матрицу света: смотрим ИЗ направления, обратного
    ///    lightDir (свет "приходит" по lightDir, значит наблюдатель стоит
    ///    в направлении -lightDir), В центр frustum'а камеры.
    /// 3. Переводим все 8 углов в это light-space и берём их AABB — это
    ///    даёт МИНИМАЛЬНЫЙ ортографический объём, который гарантированно
    ///    покрывает весь видимый frustum, не тратя разрешение впустую.
    ///
    /// ВАЖНО про стабильность (требование проекта — БЕЗ видимых "попов"):
    /// при повороте/движении камеры этот объём меняет размер и позицию
    /// каждый кадр — из-за этого текселы shadow map "плавают" относительно
    /// мира (текселы не выровнены на пиксельной сетке между кадрами), что
    /// на глаз выглядит как мерцание/дрожание края тени ("shadow swimming"),
    /// даже когда камера и объекты неподвижны, а меняется только её угол
    /// обзора. Полное решение (округление центра объёма до шага текселя
    /// shadow map) — известное дальнейшее улучшение этой же Фазы 6,
    /// сознательно отложенное: минимальный рабочий вариант должен сначала
    /// давать корректные тени в принципе, стабилизация — следующий шаг той
    /// же фазы, а не блокер для первого прохода.
    /// ИСПРАВЛЕНО (стабилизация теней — устранение "shadow swimming",
    /// отложенное улучшение, изначально анонсированное как будущий шаг
    /// ещё в Фазе 6): раньше ортографический объём света ЗАНОВО строился
    /// каждый кадр как ПЛОТНЫЙ AABB 8 углов camera frustum'а в
    /// light-space. Из-за этого при повороте камеры (не только сдвиге!)
    /// сам РАЗМЕР этого AABB менял форму от кадра к кадру — диагональ
    /// frustum'а, спроецированная на плоскость света, "дышит" при
    /// повороте камеры, даже если сама камера физически стоит на месте.
    /// Изменение размера объёма означает изменение масштаба (texels per
    /// world unit) shadow map КАЖДЫЙ кадр — тень одного и того же
    /// неподвижного объекта из-за этого чуть смещается на доли текселя от
    /// кадра к кадру, что глаз считывает как заметное дрожание/"плавание"
    /// края тени, особенно при вращении камеры на месте.
    ///
    /// Стандартное решение (см. например GPU Gems 3, "Common Techniques
    /// to Improve Shadow Depth Maps" — там же описан этот же класс
    /// артефакта): (1) зафиксировать РАЗМЕР ортографического объёма как
    /// константу для текущего frustum'а (не зависящую от его ориентации —
    /// берём максимальный радиус СРЕДИ всех 8 углов от центра, тогда
    /// объём одного размера гарантированно вмещает frustum при ЛЮБОЙ его
    /// ориентации), и (2) "защёлкивать" (snap) центр этого объёма в
    /// light-space X/Y на шаги размером РОВНО в один тексель shadow map —
    /// тогда объём может двигаться только целыми текселями, а не плавно,
    /// и один и тот же мировой объект всегда попадает в один и тот же
    /// тексель (с точностью до целого текселя) независимо от того, куда
    /// именно между кадрами сдвинулась/повернулась камера.
    /// ОБНОВЛЕНО (каскадные тени / CSM — расширение Фазы 6): раньше
    /// (`compute_shadow_view_proj`, единственный каскад) NDC Z всегда был
    /// 0.0/1.0 — весь camera.near..camera.far. Теперь принимает СВОЙ
    /// диапазон дистанций для конкретного каскада (`near_dist`/`far_dist`,
    /// в метрах от камеры вдоль её взгляда) и переводит его в
    /// соответствующие NDC Z через ПРОЕКЦИОННУЮ (не полную view-proj)
    /// матрицу камеры — стандартный способ найти NDC Z для произвольной
    /// view-space глубины при перспективной проекции. Только 8 углов
    /// POD-frustum'а для ЭТОГО каскада строятся из ndc_z_near/ndc_z_far
    /// вместо фиксированных 0.0/1.0 — остальная логика (радиус, snap,
    /// ортопроекция) идентична однокаскадной версии и переиспользуется
    /// БЕЗ ИЗМЕНЕНИЙ для каждого каскада.
    fn compute_cascade_view_proj(&self, light_dir: Vec3, near_dist: f32, far_dist: f32) -> Mat4 {
        let cam = &self.camera;
        let proj = cam.projection_matrix();
        let inv_view_proj = (proj * cam.view_matrix()).inverse();

        // NDC Z для near_dist/far_dist: подставляем view-space точку
        // (0, 0, dist) в проекционную матрицу и берём z/w результата —
        // тот же способ, которым сама проекционная матрица переводит
        // произвольную view-space глубину в NDC Z. DirectX-конвенция
        // (glam::camera::lh::proj::directx) кладёт эти коэффициенты в
        // столбцы 2 (индекс Z) и 3 (индекс W) матрицы.
        let ndc_z_for_view_dist = |dist: f32| -> f32 {
            let clip_z = proj.z_axis.z * dist + proj.w_axis.z;
            let clip_w = proj.z_axis.w * dist + proj.w_axis.w;
            if clip_w.abs() > 1e-6 { clip_z / clip_w } else { 0.0 }
        };
        let ndc_z_near = ndc_z_for_view_dist(near_dist);
        let ndc_z_far = ndc_z_for_view_dist(far_dist);

        // 8 углов NDC-объёма ЭТОГО каскада (DirectX: Z в [0,1], X/Y в
        // [-1,1]) — переводим каждый через инверсию view*proj в мировые
        // координаты, получая 8 углов под-frustum'а камеры между
        // near_dist и far_dist (а не всего camera frustum'а целиком, как
        // раньше в однокаскадной версии).
        let ndc_corners: [Vec3; 8] = [
            Vec3::new(-1.0, -1.0, ndc_z_near), Vec3::new(1.0, -1.0, ndc_z_near),
            Vec3::new(-1.0, 1.0, ndc_z_near), Vec3::new(1.0, 1.0, ndc_z_near),
            Vec3::new(-1.0, -1.0, ndc_z_far), Vec3::new(1.0, -1.0, ndc_z_far),
            Vec3::new(-1.0, 1.0, ndc_z_far), Vec3::new(1.0, 1.0, ndc_z_far),
        ];
        let mut world_corners = [Vec3::ZERO; 8];
        let mut center = Vec3::ZERO;
        for (i, ndc) in ndc_corners.iter().enumerate() {
            let clip = inv_view_proj * glam::Vec4::new(ndc.x, ndc.y, ndc.z, 1.0);
            // Перспективное деление — clip.w != 1 после умножения на
            // инверсию проекции с перспективой.
            let w = if clip.w.abs() > 1e-6 { clip.w } else { 1.0 };
            let world = Vec3::new(clip.x / w, clip.y / w, clip.z / w);
            world_corners[i] = world;
            center += world;
        }
        center /= 8.0;

        // ДОБАВЛЕНО: фиксированный радиус объёма — максимальное расстояние
        // от центра frustum'а до любого из его 8 углов. Использование
        // ОДНОГО числа (радиуса сферы, описывающей frustum), а не
        // AABB-размера по каждой оси в отдельности, — ключевой момент: у
        // сферы нет "ориентации", её радиус не меняется при повороте
        // camera frustum'а вокруг своего центра, поэтому объём стабилен
        // при любом развороте камеры (только при изменении FOV/near/far,
        // которое и должно менять зону покрытия shadow map).
        let mut radius = 0.0_f32;
        for corner in &world_corners {
            radius = radius.max((*corner - center).length());
        }
        radius = radius.max(1.0); // защита от вырожденного (нулевого) frustum'а

        let light_dir = if light_dir.length_squared() > 1e-6 { light_dir.normalize() } else { Vec3::new(0.0, -1.0, 0.0) };
        // Наблюдатель стоит "позади" сцены относительно направления света
        // (свет идёт ВДОЛЬ light_dir, значит источник находится в
        // противоположном направлении от центра сцены) — дистанция взята с
        // запасом (camera.far), чтобы весь frustum гарантированно оказался
        // ПЕРЕД этой точкой обзора света (иначе часть объёма попала бы за
        // near-плоскость light-view и обрезалась бы).
        let light_eye = center - light_dir * cam.far.max(50.0);
        // up для look_at: если light_dir почти совпадает с мировым up
        // (взгляд светила прямо вниз/вверх), обычный Vec3::Y даёт
        // вырожденную (нулевую) правую ось — подстраховка запасным up.
        let up = if light_dir.x.abs() < 0.001 && light_dir.z.abs() < 0.001 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let light_view = crate::math::look_at(light_eye, center, up);

        // ДОБАВЛЕНО: центр объёма В ПРОСТРАНСТВЕ СВЕТА, "защёлкнутый" на
        // сетку с шагом в один тексель shadow map. texel_size — размер
        // одного текселя в МИРОВЫХ единицах (объём по X/Y имеет ширину
        // 2*radius, покрывается SHADOW_MAP_RESOLUTION текселями).
        // Округление center_light.x/y (не z — вдоль направления света
        // защёлкивать не нужно, там нет текселей, только near/far) до
        // ближайшего кратного texel_size гарантирует, что объём при
        // сдвиге камеры двигается только ЦЕЛЫМИ текселями — суб-текселные
        // сдвиги (источник дрожания) невозможны по построению.
        let texel_size = (radius * 2.0) / SHADOW_MAP_RESOLUTION as f32;
        let center_light = light_view.transform_point3(center);
        let snapped_x = (center_light.x / texel_size).floor() * texel_size;
        let snapped_y = (center_light.y / texel_size).floor() * texel_size;

        // Небольшой запас (padding) по глубине (Z в light-space — "вдоль"
        // направления света), чтобы отбрасывающие тень объекты чуть ЗА
        // пределами видимого frustum (но всё ещё способные закрыть свет
        // для видимой геометрии) не обрезались near/far плоскостями
        // light-проекции. Z (глубина) НЕ защёлкивается на тексели — она не
        // определяет позицию текселя в самой карте, только диапазон
        // сравнения глубины, поэтому не участвует в "swimming"-артефакте.
        let z_padding = radius * 0.5 + 10.0;

        crate::math::orthographic(
            snapped_x - radius, snapped_x + radius,
            snapped_y - radius, snapped_y + radius,
            center_light.z - radius - z_padding, center_light.z + radius + z_padding,
        ) * light_view
    }

    /// Гарантирует, что константный буфер вмещает как минимум
    /// `needed_per_frame` слотов трансформаций НА КАЖДЫЙ из двух back
    /// buffer'ов (итого выделяется `needed_per_frame * 2` слотов).
    /// Пересоздаёт буфер, если текущей ёмкости не хватает (например,
    /// сцена выросла — добавили ещё кубов в сетку пола).
    ///
    /// Буфер удваивается на оба back buffer'а по той же причине, по
    /// которой у нас уже два `command allocator`'а: пока GPU дорисовывает
    /// кадр N (frame_index k), CPU уже готовит кадр N+1 (frame_index
    /// 1-k). Если бы оба кадра писали в одни и те же слоты одного и того
    /// же буфера — это была бы гонка данных между CPU, пишущим новый
    /// кадр, и GPU, всё ещё читающим предыдущий. Слот для конкретного
    /// кадра выбирается как `frame_index * capacity + i` — см.
    /// `render_frame`.
    fn ensure_constant_buffer_capacity(&mut self, needed_per_frame: usize) -> Result<()> {
        if self.constant_buffer.is_some() && needed_per_frame <= self.constant_buffer_capacity {
            return Ok(());
        }

        let new_capacity = needed_per_frame.max(64).next_power_of_two();
        let total_slots = new_capacity * 2; // x2 — по набору слотов на каждый back buffer
        let buffer = Buffer::create_constant_buffer_array(TransformConstants::aligned_size(), total_slots)?;
        println!(
            "[ENGINE] Constant buffer (re)allocated: {} slots/кадр x2 = {} слотов",
            new_capacity, total_slots
        );
        self.constant_buffer = Some(buffer);
        self.constant_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): тот же
    /// паттерн роста, что и `ensure_constant_buffer_capacity` выше, но для
    /// отдельного `shadow_constant_buffer` (см. `constant_buffer::ShadowConstants`)
    /// — shadow-проход рисует ТЕ ЖЕ объекты кадра, поэтому нуждается в
    /// ровно таком же количестве слотов, просто в СВОЁМ буфере (другой
    /// layout данных, другая root signature).
    ///
    /// ИСПРАВЛЕНО (Cascaded Shadow Maps — переполнение буфера): `caller`
    /// (render_frame) передаёт `needed_per_frame = shadow_jobs.len() *
    /// NUM_CASCADES` — то есть `shadow_constant_buffer_capacity` ниже
    /// хранит ёмкость на ОДИН ПОЛНЫЙ кадр (все каскады сразу), а формула
    /// слота в render_frame — `(frame_index * NUM_CASCADES + cascade) *
    /// shadow_constant_buffer_capacity + i` — умножает `capacity` НА
    /// (frame_index * NUM_CASCADES + cascade), а НЕ просто на frame_index.
    /// Раньше (до CSM, один каскад) буфер выделялся как `capacity * 2`
    /// (x2 только на frame_index) — теперь этого катастрофически не
    /// хватает: слот для cascade=2 при frame_index=1 обращается далеко ЗА
    /// пределы буфера (undefined behaviour/GPU crash). Нужно выделять
    /// `capacity`, умноженную на ПОЛНОЕ число независимых блоков —
    /// `2 (frame_index) * NUM_CASCADES` — а не на 2.
    fn ensure_shadow_constant_buffer_capacity(&mut self, needed_per_frame: usize) -> Result<()> {
        if self.shadow_constant_buffer.is_some() && needed_per_frame <= self.shadow_constant_buffer_capacity {
            return Ok(());
        }

        let new_capacity = needed_per_frame.max(64).next_power_of_two();
        let total_slots = new_capacity * 2 * NUM_CASCADES;
        let buffer = Buffer::create_constant_buffer_array(crate::constant_buffer::ShadowConstants::aligned_size(), total_slots)?;
        println!(
            "[ENGINE] Shadow constant buffer (re)allocated: {} slots/(кадр*каскад) x2 x{} каскада = {} слотов",
            new_capacity, NUM_CASCADES, total_slots
        );
        self.shadow_constant_buffer = Some(buffer);
        self.shadow_constant_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): гарантирует, что
    /// `light_buffer` вмещает как минимум `needed` элементов `GPULight`.
    /// Тот же паттерн роста, что и у `ensure_constant_buffer_capacity`
    /// (степень двойки, минимум разумного стартового размера) — растёт по
    /// требованию, а не выделяется на весь возможный максимум сразу,
    /// потому что реальное число видимых после каллинга фонарей в кадре
    /// обычно НАМНОГО меньше total_lights (это и есть весь смысл каллинга).
    fn ensure_light_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        if self.light_buffer.is_some() && needed <= self.light_buffer_capacity {
            return Ok(());
        }

        let new_capacity = needed.max(64).next_power_of_two();
        let size_bytes = new_capacity as u64 * std::mem::size_of::<GPULight>() as u64;
        let buffer = Buffer::create_structured_buffer(size_bytes)?;
        println!(
            "[ENGINE] Light buffer (re)allocated: {} GPULight слотов ({} байт)",
            new_capacity, size_bytes
        );
        self.light_buffer = Some(buffer);
        self.light_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): гарантирует, что
    /// `grid_cells_buffer` вмещает РОВНО `needed` ячеек. В отличие от
    /// `ensure_light_buffer_capacity`/`ensure_grid_entries_buffer_capacity`,
    /// здесь НЕТ роста "с запасом" (`next_power_of_two`) — общее число
    /// ячеек сетки в FirstFires фиксировано на весь срок жизни плагина
    /// (задаётся один раз в LightConfig), пересоздание буфера при этом
    /// размере НЕ происходит на каждый кадр (проверка `needed ==
    /// capacity`, а не `needed <= capacity`, чтобы не тратить память под
    /// "запас", который никогда не понадобится для этого буфера).
    fn ensure_grid_cells_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        if self.grid_cells_buffer.is_some() && needed == self.grid_cells_buffer_capacity {
            return Ok(());
        }
        let size_bytes = needed.max(1) as u64 * std::mem::size_of::<LightGridCell>() as u64;
        let buffer = Buffer::create_structured_buffer(size_bytes)?;
        println!(
            "[ENGINE] Grid cells buffer (re)allocated: {} ячеек ({} байт)",
            needed, size_bytes
        );
        self.grid_cells_buffer = Some(buffer);
        self.grid_cells_buffer_capacity = needed;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): то же самое, что
    /// `ensure_light_buffer_capacity`, но для `grid_entries_buffer` —
    /// число entries меняется каждый кадр (зависит от того, сколько
    /// фонарей реально видимо), поэтому растёт степенями двойки, а не
    /// фиксировано, как `grid_cells_buffer`.
    fn ensure_grid_entries_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        if self.grid_entries_buffer.is_some() && needed <= self.grid_entries_buffer_capacity {
            return Ok(());
        }
        let new_capacity = needed.max(64).next_power_of_two();
        let size_bytes = new_capacity as u64 * std::mem::size_of::<LightGridEntry>() as u64;
        let buffer = Buffer::create_structured_buffer(size_bytes)?;
        println!(
            "[ENGINE] Grid entries buffer (re)allocated: {} слотов ({} байт)",
            new_capacity, size_bytes
        );
        self.grid_entries_buffer = Some(buffer);
        self.grid_entries_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): вспомогательная
    /// функция для transition-барьера — до этой фазы движок вообще не
    /// вызывал `ResourceBarrier` (рисовал прямо в back buffer без явных
    /// переходов состояния, что формально некорректно по спецификации
    /// D3D12, хоть и "работало" на многих драйверах). Теперь, когда
    /// появился HDR render target с полноценным циклом состояний
    /// (RENDER_TARGET во время draw pass -> PIXEL_SHADER_RESOURCE во время
    /// чтения в composite pass -> обратно в RENDER_TARGET для следующего
    /// кадра), обойтись без барьеров уже не получится — без них GPU не
    /// гарантированно видит корректные данные (кэши/порядок записи-чтения
    /// не синхронизированы).
    ///
    /// `pResource: ManuallyDrop<Option<ID3D12Resource>>` (см.
    /// `D3D12_RESOURCE_TRANSITION_BARRIER` в windows-крейте) — тот же COM
    /// refcounting паттерн, что уже встречался в `pso.rs` для
    /// `pRootSignature`: клонируем ресурс (это увеличивает refcount на 1),
    /// поэтому обязаны сами явно уменьшить его обратно после того, как
    /// барьер отработал — см. `ManuallyDrop::drop` сразу после
    /// `ResourceBarrier` в местах вызова.
    fn transition_barrier(
        resource: &ID3D12Resource,
        before: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) -> D3D12_RESOURCE_BARRIER {
        D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
        }
    }

    /// Освобождает лишнюю ссылку на ресурс внутри барьера, добавленную
    /// клонированием в `transition_barrier` — см. объяснение там же.
    unsafe fn drop_transition_barrier(mut barrier: D3D12_RESOURCE_BARRIER) {
        unsafe {
            std::mem::ManuallyDrop::drop(&mut barrier.Anonymous.Transition);
        }
    }

    pub fn add_mesh(&mut self, mesh: Mesh) -> usize {
        self.meshes.push(mesh);
        println!("[ENGINE] Mesh added, total meshes: {}", self.meshes.len());
        self.meshes.len() - 1
    }

    pub fn add_triangle(&mut self) -> usize {
        let mesh = Mesh::triangle().unwrap();
        self.add_mesh(mesh)
    }

    pub fn add_quad(&mut self, x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> usize {
        let mesh = Mesh::quad(x, y, width, height, color).unwrap();
        self.add_mesh(mesh)
    }

    pub fn add_cube(&mut self, size: f32) -> usize {
        let mesh = Mesh::cube(size).unwrap();
        self.add_mesh(mesh)
    }

    /// См. подробное объяснение у `Mesh::cube_colored` — куб с ОДНИМ
    /// нейтральным цветом на всех гранях (в отличие от `add_cube`,
    /// который красит каждую грань в отдельный отладочный цвет), нужен
    /// для геометрии, где важно реально увидеть результат освещения
    /// (point/spot-фонари, тени), а не отладочную раскраску нормалей.
    pub fn add_cube_colored(&mut self, size: f32, r: f32, g: f32, b: f32, a: f32) -> usize {
        let mesh = Mesh::cube_colored(size, r, g, b, a).unwrap();
        self.add_mesh(mesh)
    }

    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.mesh_instances.clear();
        println!("[ENGINE] All meshes and instances cleared");
    }

    pub fn render_frame(&mut self) -> Result<bool> {
        // ИСПРАВЛЕНО: было `self.renderer.as_ref().unwrap()` — паниковало,
        // если render_frame() вызван до init() либо после потери
        // устройства (renderer сброшен в None). Теперь явная ошибка.
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: render_frame() called but renderer is not initialized");
            Error::from_hresult(HRESULT(1))
        })?;

        let frame_index = {
            let state = STATE.lock().unwrap();
            state.frame_index as usize
        };

        // ИСПРАВЛЕНО: раньше здесь не ждали ничего перед Reset() аллокатора,
        // а полная синхронизация происходила уже ПОСЛЕ Present каждого
        // кадра (см. ниже) — это сводило на нет весь смысл двойной
        // буферизации, так как CPU и GPU работали строго последовательно.
        // Теперь мы ждём (если нужно) именно перед повторным использованием
        // ресурсов данного frame_index — то есть перед тем, как их
        // действительно нужно тронуть, а не сразу после отправки в очередь.
        //
        // ИСПРАВЛЕНО ("белое окно" + краш драйвера, см. подробный
        // комментарий у `wait_for_fence` в начале файла): раньше это был
        // `while fence.GetCompletedValue() < target { sleep(1ms) }` БЕЗ
        // таймаута — самое горячее место во всём движке (выполняется
        // КАЖДЫЙ кадр), поэтому именно оно раньше всего замораживало окно
        // намертво при малейшем сбое GPU. `wait_for_fence` ждёт не дольше
        // 5 секунд и досрочно выходит с диагностикой, если устройство уже
        // потеряно — вместо вечного зависания превращаем это в обычную
        // ошибку кадра.
        if let Some(&target) = self.frame_fence_values.get(frame_index) {
            if target > 0 {
                let fence = crate::get_fence()?;
                if let Err(reason) = wait_for_fence(&fence, target, std::time::Duration::from_secs(5)) {
                    eprintln!("[ENGINE] render_frame: {} — прерываем кадр", reason);
                    crate::dump_d3d12_debug_messages();
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        }

        let allocator = CommandList::get_allocator(frame_index)
            .ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

        unsafe {
            // ИСПРАВЛЕНО: раньше результат Reset() отбрасывался
            // (`allocator.Reset();` без `?`) — если аллокатор был всё ещё
            // "в полёте" на GPU (ошибка где-то в логике синхронизации),
            // Reset() тихо проваливался и рендер продолжал бы работать в
            // испорченном состоянии. Теперь ошибка сразу распространяется.
            allocator.Reset()?;
        }

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()`.
        let device = crate::get_device()?;

        let cmd_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?
        };

        // ИЗМЕНЕНО (Фаза 5 плана по реализму/фонарям): основной draw pass
        // теперь рисует в HDR render target (см. подробное объяснение у
        // `Renderer::hdr_target` в render.rs), а НЕ напрямую в 8-битный
        // back buffer, как было раньше. Реальная запись в back buffer
        // (после которого можно звать Present) происходит позже, в
        // отдельном composite/tonemap-проходе — см. ниже, после основного
        // цикла отрисовки объектов.
        //
        // ВАЖНО (borrow checker, Фаза 6 плана по реализму/фонарям — тени):
        // эти два handle'а копируются из `renderer` ЗДЕСЬ, СРАЗУ после
        // создания cmd_list — то есть ДО shadow pass'а ниже, который
        // вызывает `&mut self`-методы (`ensure_shadow_constant_buffer_capacity`).
        // Раньше (до Фазы 6) `renderer` использовался непрерывно от
        // объявления в начале функции до этой самой точки, без единого
        // `&mut self`-вызова между ними — компилировалось без проблем.
        // Теперь между началом функции и этой точкой вклинился shadow
        // pass, которому нужен `&mut self` (см. также E0502-комментарий у
        // повторного захвата `renderer` дальше в этой функции — та же
        // проблема, тот же класс фикса: завершить заём ДО, а не ПОСЛЕ
        // `&mut self`-вызовов).
        let rtv_handle = renderer.hdr_rtv;
        let dsv_handle = renderer.depth_stencil_view;

        // =====================================================================
        // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): SHADOW PASS.
        // Выполняется ПЕРВЫМ в кадре (до основного draw pass'а в HDR
        // target) — рисует сцену с точки зрения directional-света в
        // depth-only shadow map, которую основной пиксельный шейдер потом
        // читает через SampleCmpLevelZero (см. ComputeShadowFactor в
        // compile_default_shaders).
        //
        // Собираем ОТДЕЛЬНЫЙ (упрощённый) список задач — только объекты с
        // реальной model-матрицей (mesh_instances + ECS-сцена); старый 2D
        // RawIdentity-режим (main1.rs без камеры) в тенях не участвует —
        // эти объекты рисуются identity-матрицей прямо в clip-space, у них
        // физически нет осмысленной позиции в мировом пространстве, чтобы
        // отбрасывать тень.
        let shadow_jobs: Vec<(usize, Mat4)> = {
            let mut v: Vec<(usize, Mat4)> = Vec::new();
            if !self.mesh_instances.is_empty() {
                for instance in &self.mesh_instances {
                    if instance.mesh_index < self.meshes.len() {
                        v.push((instance.mesh_index, instance.transform_matrix()));
                    }
                }
            }
            for (mesh_index, world) in self.scene.collect_renderables() {
                if mesh_index < self.meshes.len() {
                    v.push((mesh_index, world));
                }
            }
            v
        };

        // Направление света берётся из уже существующего
        // self.transform_constants.light_dir (тот же источник, что читает
        // и основной PS для directional-освещения — единая точка истины,
        // не рассинхронизируется с тем, что реально освещает сцену).
        let light_dir_vec = Vec3::new(
            self.transform_constants.light_dir[0],
            self.transform_constants.light_dir[1],
            self.transform_constants.light_dir[2],
        );

        // ОБНОВЛЕНО (каскадные тени / CSM): дистанции сплитов в МЕТРАХ —
        // CASCADE_SPLITS хранит доли camera.far, пересчитываем один раз
        // здесь, а не в компонент каждого каскада отдельно.
        let cascade_far_distances: [f32; NUM_CASCADES] = {
            let mut arr = [0.0f32; NUM_CASCADES];
            for i in 0..NUM_CASCADES {
                arr[i] = self.camera.far * CASCADE_SPLITS[i];
            }
            arr
        };
        // view_proj для КАЖДОГО каскада — вычисляем все сразу (нужны и
        // здесь для shadow-прохода, и позже для основного 3D-прохода и
        // volumetric-прохода — единая точка истины, не пересчитываем
        // несколько раз за кадр).
        let cascade_view_projs: [Mat4; NUM_CASCADES] = {
            let mut arr = [Mat4::IDENTITY; NUM_CASCADES];
            let mut near_dist = self.camera.near;
            for i in 0..NUM_CASCADES {
                let far_dist = cascade_far_distances[i];
                arr[i] = self.compute_cascade_view_proj(light_dir_vec, near_dist, far_dist);
                near_dist = far_dist;
            }
            arr
        };

        if self.shadow_pipeline_state.is_some() && self.shadow_root_signature.is_some() {
            // ОБНОВЛЕНО (каскадные тени / CSM): ёмкость буфера теперь
            // считается на NUM_CASCADES полных проходов по сцене за кадр
            // (каждый объект рисуется в КАЖДЫЙ каскад отдельно ниже) — без
            // этого множителя слотов не хватило бы уже на втором каскаде.
            if let Err(e) = self.ensure_shadow_constant_buffer_capacity(shadow_jobs.len() * NUM_CASCADES) {
                eprintln!("[ENGINE] WARNING: не удалось выделить shadow_constant_buffer: {:?}", e);
            }

            unsafe {
                cmd_list.SetPipelineState(Some(self.shadow_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.shadow_root_signature.as_ref().unwrap()));
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                let shadow_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: SHADOW_MAP_RESOLUTION as f32,
                    Height: SHADOW_MAP_RESOLUTION as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                cmd_list.RSSetViewports(&[shadow_viewport]);
                let shadow_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: SHADOW_MAP_RESOLUTION as i32,
                    bottom: SHADOW_MAP_RESOLUTION as i32,
                };
                cmd_list.RSSetScissorRects(&[shadow_scissor]);

                // ДОБАВЛЕНО (каскадные тени / CSM): рисуем сцену В КАЖДЫЙ
                // каскад отдельно — viewport/scissor/PSO/root signature
                // одинаковы для всех (задаются один раз выше), меняется
                // только целевой DSV (OMSetRenderTargets) и view-proj
                // матрица (через ShadowConstants). Дороже линейно с числом
                // каскадов (полный проход по sceneой геометрии на каждый),
                // но это стандартная и неизбежная цена CSM — единственная
                // альтернатива (один общий проход с geometry shader instancing
                // по каскадам) требовала бы GS, которого в этом движке нет.
                for cascade in 0..NUM_CASCADES {
                    let dsv = self.shadow_dsvs[cascade];
                    cmd_list.OMSetRenderTargets(0, None, false, Some(&dsv));
                    cmd_list.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

                    let light_view_proj = cascade_view_projs[cascade];

                    if let Some(shadow_cb) = &self.shadow_constant_buffer {
                        for (i, (mesh_index, model)) in shadow_jobs.iter().enumerate() {
                            let mesh = &self.meshes[*mesh_index];
                            let mlvp = light_view_proj * (*model);
                            let shadow_constants = crate::constant_buffer::ShadowConstants {
                                model_light_view_proj: mlvp.to_cols_array_2d(),
                            };
                            // Слот учитывает И frame_index (двойная
                            // буферизация), И номер каскада — иначе кадры
                            // (или каскады одного кадра) писали бы поверх
                            // друг друга ДО того, как GPU реально
                            // отрисовал предыдущий (тот же класс бага, что
                            // и описан в TransformConstants::write_at).
                            let slot = (frame_index * NUM_CASCADES + cascade) * self.shadow_constant_buffer_capacity + i;
                            if let Err(e) = shadow_constants.write_at(shadow_cb, slot) {
                                eprintln!("[ENGINE] WARNING: failed to write shadow constant buffer slot {}: {:?}", slot, e);
                                continue;
                            }
                            let gpu_addr = crate::constant_buffer::ShadowConstants::gpu_address_for_slot(shadow_cb, slot);
                            cmd_list.SetGraphicsRootConstantBufferView(0, gpu_addr);

                            let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                                BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                                SizeInBytes: mesh.vertex_buffer.size as u32,
                                StrideInBytes: Vertex::STRIDE,
                            };
                            cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));

                            if let Some(index_buffer) = &mesh.index_buffer {
                                let index_view = D3D12_INDEX_BUFFER_VIEW {
                                    BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                                    SizeInBytes: index_buffer.size as u32,
                                    Format: DXGI_FORMAT_R32_UINT,
                                };
                                cmd_list.IASetIndexBuffer(Some(&index_view));
                                cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
                            } else {
                                cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
                            }
                        }
                    }

                    // Каскад записан — переводим в PIXEL_SHADER_RESOURCE,
                    // чтобы основной draw pass ниже мог читать его через
                    // SampleCmpLevelZero. Явный трекер (per-каскад в
                    // shadow_maps_are_srv), а не хардкод состояния — та же
                    // защита от no-op барьера, что уже пришлось чинить для
                    // bloom-таргетов (см. подробный комментарий у
                    // bloom_a_is_srv) — здесь она не даёт себя проявиться
                    // СРАЗУ (это первый барьер за кадр для каждого
                    // каскада, до записи всегда DEPTH_WRITE), но защищает
                    // на случай будущих правок порядка проходов.
                    if let Some(shadow_map) = &self.shadow_maps[cascade] {
                        if !self.shadow_maps_are_srv[cascade] {
                            let barrier = Self::transition_barrier(
                                &shadow_map.resource,
                                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                            );
                            let barriers = [barrier];
                            cmd_list.ResourceBarrier(&barriers);
                            for b in barriers {
                                Self::drop_transition_barrier(b);
                            }
                            self.shadow_maps_are_srv[cascade] = true;
                        }
                    }
                }
            }
        }
        // =====================================================================

        unsafe {
            cmd_list.OMSetRenderTargets(1, Some(&rtv_handle), false, Some(&dsv_handle));
            cmd_list.ClearRenderTargetView(rtv_handle, &self.clear_color, None);
            cmd_list.ClearDepthStencilView(dsv_handle, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);

            let scissor = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            cmd_list.RSSetScissorRects(&[scissor]);

            cmd_list.SetPipelineState(Some(self.pipeline_state.as_ref().unwrap()));
            cmd_list.SetGraphicsRootSignature(Some(self.root_signature.as_ref().unwrap()));

            // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Белая
            // fallback-текстура — ГАРАНТИРОВАННО создаётся здесь, ДО
            // SetDescriptorHeaps ниже, а не лениво внутри цикла по jobs. Это
            // важно: `ensure_white_texture`/`register_material_texture`
            // могут ПЕРЕСОЗДАТЬ `self.shadow_srv_heap` (если это первая
            // текстура вообще и его material-часть ещё нулевого размера, см.
            // `ensure_material_srv_capacity`) — новый COM-объект хипа делает
            // недействительным любой ПРЕДЫДУЩИЙ вызов SetDescriptorHeaps на
            // GPU. Если бы этот вызов происходил ПОСЛЕ SetDescriptorHeaps
            // (как было до этой задачи), command list остался бы привязан к
            // уже вытесненному старому хипу — GPU читал бы дескрипторы не
            // из того хипа, что реально хранит актуальные SRV. Поэтому все
            // возможные пересоздания хипа обязаны завершиться ДО
            // единственного SetDescriptorHeaps на кадр.
            let white_texture_srv_fallback = match self.ensure_white_texture() {
                Ok(index) => Some(index),
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось создать белую fallback-текстуру: {:?} — material-биндинг пропущен в этом кадре", e);
                    None
                }
            };
            // ДОБАВЛЕНО (Задача #15, normal mapping): та же логика "создать
            // fallback ДО SetDescriptorHeaps", что и у белой albedo-текстуры
            // выше — см. подробный комментарий там (пересоздание хипа
            // обязано завершиться до его единственного биндинга на кадр).
            let flat_normal_srv_fallback = match self.ensure_flat_normal_texture() {
                Ok(index) => Some(index),
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось создать flat-normal fallback-текстуру: {:?} — normal mapping пропущен в этом кадре", e);
                    None
                }
            };
            let neutral_mr_srv_fallback = match self.ensure_neutral_mr_texture() {
                Ok(index) => Some(index),
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось создать нейтральную MR fallback-текстуру: {:?}", e);
                    None
                }
            };
            let cbv_srv_uav_size_materials = {
                let state = STATE.lock().unwrap();
                state.cbv_srv_uav_descriptor_size
            };

            // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): биндим
            // shadow_srv_heap (единственный SHADER_VISIBLE-хип, нужный
            // основному 3D-проходу — сам по себе он раньше НЕ вызывал
            // SetDescriptorHeaps вообще, т.к. все его SRV/CBV были root-
            // дескрипторами без таблиц) и записываем GPU-адрес таблицы
            // (root-параметр 4, см. create_root_signature) в дескриптор
            // shadow map. Должно идти ДО SetGraphicsRootDescriptorTable,
            // иначе GPU не знает, из какого хипа читать смещение таблицы.
            // ОБНОВЛЕНО (Задача #15): этот же хип теперь ЕДИНСТВЕННЫЙ,
            // содержащий И shadow-каскады, И material-текстуры (см.
            // белую fallback-текстуру выше, гарантированно созданную/
            // перерегистрированную ДО этой точки) — SetDescriptorHeaps
            // остаётся ОДИН на кадр, root-параметр 5 (material) ниже в
            // цикле jobs только меняет СМЕЩЕНИЕ таблицы внутри уже
            // забинженного хипа, не сам хип.
            if let Some(shadow_srv_heap) = &self.shadow_srv_heap {
                let heaps = [Some(shadow_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.SetGraphicsRootDescriptorTable(4, self.shadow_srv_gpu);
            }

            // Параметры тени, общие для ВСЕХ объектов кадра (записываются
            // один раз, а не в цикле по jobs ниже — свет один на сцену).
            // shadows_enabled=1, только если shadow PSO/root signature
            // реально созданы (см. create_shadow_pipeline_state в init()) —
            // без этого основной PS корректно пропускает shadow-выборку
            // (см. ComputeShadowFactor: shadowsEnabled==0 -> return 1.0),
            // вместо чтения непроинициализированной/неверно
            // спроецированной shadow map.
            // ОБНОВЛЕНО (каскадные тени / CSM): раньше здесь писалась ОДНА
            // матрица light_view_proj. Теперь пишем все NUM_CASCADES матриц
            // (уже вычислены выше, в cascade_view_projs, ДО shadow-прохода
            // — единая точка истины, тот же массив, что использовался для
            // рендера самих shadow map) и view-space дистанции границ
            // каскадов, по которым основной пиксельный шейдер выбирает
            // нужный каскад (см. SelectCascade в compile_default_shaders).
            for cascade in 0..NUM_CASCADES {
                self.transform_constants.light_view_proj[cascade] = cascade_view_projs[cascade].to_cols_array_2d();
            }
            self.transform_constants.cascade_split_distances = [
                cascade_far_distances[0],
                cascade_far_distances[1],
                cascade_far_distances[2],
                0.0,
            ];
            self.transform_constants.shadow_map_size = SHADOW_MAP_RESOLUTION as f32;
            self.transform_constants.shadows_enabled =
                if self.shadow_pipeline_state.is_some() && self.shadow_root_signature.is_some() { 1 } else { 0 };

            // ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): список
            // видимых после каллинга фонарей (FirstFires уже отфильтровал
            // по LOD/дистанции/фрустуму в AlkashEngine::update()) —
            // загружается в GPU-буфер и биндится ОДИН РАЗ на кадр (в
            // отличие от CBV с трансформацией, свет одинаков для всех
            // объектов кадра, per-object бинда не нужно).
            //
            // Если LightPlugin не инициализирован (init_lights() не
            // вызывался) — список пуст, пиксельный шейдер получит 0
            // элементов и просто не добавит ни одного фонарного вклада;
            // это НЕ ошибка (например main1.rs может не использовать
            // фонари вообще), поэтому здесь нет early-return.
            // ВАЖНО (borrow checker): `self.get_gpu_lights()` заимствует
            // `self` неизменяемо (через `self.lights`), а
            // `ensure_light_buffer_capacity` требует `&mut self`. Если бы
            // мы держали результат `get_gpu_lights()` живым через вызов
            // `ensure_light_buffer_capacity`, компилятор отказался бы
            // собирать код (E0502). Поэтому сначала считаем ТОЛЬКО длину
            // (заимствование сразу заканчивается), гарантируем ёмкость
            // буфера, и только ПОСЛЕ этого заново берём срез — к этому
            // моменту `ensure_light_buffer_capacity` уже закончила менять
            // self, конфликтующих заимствований одновременно не остаётся.
            let light_count = self.get_gpu_lights().len();
            if let Err(e) = self.ensure_light_buffer_capacity(light_count) {
                eprintln!("[ENGINE] WARNING: не удалось выделить light_buffer: {:?}", e);
            }
            if let Some(light_buffer) = &self.light_buffer {
                if light_count > 0 {
                    let gpu_lights = self.lights.as_ref().map(|l| l.get_gpu_lights()).unwrap_or(&[]);
                    let bytes = std::slice::from_raw_parts(
                        gpu_lights.as_ptr() as *const u8,
                        light_count * std::mem::size_of::<GPULight>(),
                    );
                    if let Err(e) = light_buffer.update_structured_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить light_buffer: {:?}", e);
                    }
                }
                let light_gpu_addr = light_buffer.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootShaderResourceView(1, light_gpu_addr);
            }
            // Реальное число элементов в буфере (может быть меньше, чем
            // ёмкость light_buffer после её роста) — шейдер должен знать
            // именно это значение, а не размер буфера, иначе прочитает
            // мусор/нули из ещё не заполненного хвоста. Пишется в
            // TransformConstants ниже, вместе с остальными per-draw
            // данными, т.к. отдельного константного буфера под "просто одно
            // число" заводить не стали — не usize, а u32: HLSL cbuffer не
            // работает с 64-битными int по умолчанию, а количество фонарей
            // гарантированно помещается в u32.
            self.transform_constants.light_count = light_count as u32;

            // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): пространственная
            // сетка каллинга FirstFires — GridCells (SRV t1) и GridEntries
            // (SRV t2). Тот же паттерн заимствования, что и у light_buffer
            // выше (сначала длины, потом мутация self через
            // ensure_*_capacity, потом заново — срез для копирования), по
            // той же причине (E0502, см. комментарий выше).
            //
            // grid_cells присутствует не каждый кадр — он валиден, только
            // если LightPlugin реально инициализирован (get_grid_params()
            // на пустом instance возвращает нулевые размеры сетки, что
            // безопасно интерпретируется шейдером как "сетка отсутствует",
            // см. fallback-ветку `gridDimensions.x > 0 && ...` в
            // pixel-шейдере выше).
            let grid_params = self.lights.as_ref().map(|l| l.get_grid_params());
            let grid_cells_count = self.lights.as_ref().map(|l| l.get_grid_cells().len()).unwrap_or(0);
            let grid_entries_count = self.lights.as_ref().map(|l| l.get_grid_entries().len()).unwrap_or(0);

            if grid_cells_count > 0 {
                if let Err(e) = self.ensure_grid_cells_buffer_capacity(grid_cells_count) {
                    eprintln!("[ENGINE] WARNING: не удалось выделить grid_cells_buffer: {:?}", e);
                }
            }
            if let Err(e) = self.ensure_grid_entries_buffer_capacity(grid_entries_count) {
                eprintln!("[ENGINE] WARNING: не удалось выделить grid_entries_buffer: {:?}", e);
            }

            if let Some(grid_cells_buffer) = &self.grid_cells_buffer {
                if grid_cells_count > 0 {
                    let cells = self.lights.as_ref().map(|l| l.get_grid_cells()).unwrap_or(&[]);
                    let bytes = std::slice::from_raw_parts(
                        cells.as_ptr() as *const u8,
                        grid_cells_count * std::mem::size_of::<LightGridCell>(),
                    );
                    if let Err(e) = grid_cells_buffer.update_structured_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить grid_cells_buffer: {:?}", e);
                    }
                }
                let addr = grid_cells_buffer.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootShaderResourceView(2, addr);
            }
            if let Some(grid_entries_buffer) = &self.grid_entries_buffer {
                if grid_entries_count > 0 {
                    let entries = self.lights.as_ref().map(|l| l.get_grid_entries()).unwrap_or(&[]);
                    let bytes = std::slice::from_raw_parts(
                        entries.as_ptr() as *const u8,
                        grid_entries_count * std::mem::size_of::<LightGridEntry>(),
                    );
                    if let Err(e) = grid_entries_buffer.update_structured_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить grid_entries_buffer: {:?}", e);
                    }
                }
                let addr = grid_entries_buffer.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootShaderResourceView(3, addr);
            }

            // Параметры сетки в constant buffer — если LightPlugin не
            // инициализирован, grid_params == None и мы пишем нулевую
            // сетку (gridDimensions = [0,0,0,0]), что pixel-шейдер уже
            // трактует как "сетки нет, fallback на линейный перебор".
            match grid_params {
                Some(p) => {
                    self.transform_constants.grid_world_min = [p.world_min[0], p.world_min[1], p.world_min[2], p.cell_size];
                    self.transform_constants.grid_dimensions = [p.grid_width, p.grid_height, p.grid_depth, 0];
                }
                None => {
                    self.transform_constants.grid_world_min = [0.0, 0.0, 0.0, 1.0];
                    self.transform_constants.grid_dimensions = [0, 0, 0, 0];
                }
            }

            // ИСПРАВЛЕНО (главная причина "платформа вращается вместе с
            // кубом"): раньше константный буфер был ОДИН на весь кадр, и
            // ROOT CBV указывал на один и тот же адрес для ВСЕХ объектов —
            // пол, солнце, планету, луну. GPU видит содержимое буфера НА
            // МОМЕНТ РЕАЛЬНОГО ВЫПОЛНЕНИЯ Draw, а не на момент записи с
            // CPU. Поскольку CPU успевает записать данные ВСЕХ объектов
            // кадра ещё до ExecuteCommandLists, к моменту, когда GPU
            // реально начинал выполнять command list, в буфере уже лежали
            // данные ТОЛЬКО последнего записанного объекта — и КАЖДЫЙ Draw
            // рисовался с этой же (последней) трансформацией. Отсюда и
            // эффект "всё вращается вместе, как один объект".
            //
            // Теперь: сначала собираем ВСЕ объекты кадра (старый
            // mesh_instances/2D-путь + ECS-сцена) в единый список задач,
            // выделяем под них достаточно слотов константного буфера, и
            // КАЖДЫЙ Draw получает СВОЙ собственный адрес (см.
            // TransformConstants::write_at).
            enum DrawTransform {
                /// Обычный 3D-объект: своя model-матрица, view/proj берутся
                /// из камеры один раз на весь кадр.
                Camera(Mat4),
                /// Старый 2D-режим (mesh_instances пуст) — все 4 матрицы
                /// константного буфера были identity, без камеры. Сохраняем
                /// это поведение один в один, чтобы не сломать main1.rs.
                RawIdentity,
            }
            struct DrawJob {
                mesh_index: usize,
                transform: DrawTransform,
            }

            let mut jobs: Vec<DrawJob> = Vec::new();

            if !self.mesh_instances.is_empty() {
                for instance in &self.mesh_instances {
                    if instance.mesh_index < self.meshes.len() {
                        jobs.push(DrawJob {
                            mesh_index: instance.mesh_index,
                            transform: DrawTransform::Camera(instance.transform_matrix()),
                        });
                    }
                }
            } else if self.scene.is_empty() {
                // ИСПРАВЛЕНО (баг "куб посередине экрана"): раньше это
                // условие было просто `else` — то есть срабатывало ВСЕГДА,
                // когда mesh_instances пуст, даже если объекты размещены
                // через ECS-сцену (scene.rs). В результате КАЖДЫЙ
                // добавленный меш (включая тайлы пола и столбы) рисовался
                // ЕЩЁ РАЗ отдельно, с model=view=proj=identity — то есть
                // его вершины интерпретировались напрямую как clip-space
                // координаты, без всякой проекции камеры. Именно это и
                // выглядело как "куб посередине экрана". Теперь этот
                // истинный 2D-fallback (совместимость с main1.rs, где
                // меши не размещены НИКАК — ни через mesh_instances, ни
                // через scene) срабатывает, только если сцена тоже пуста.
                for i in 0..self.meshes.len() {
                    jobs.push(DrawJob { mesh_index: i, transform: DrawTransform::RawIdentity });
                }
            }

            for (mesh_index, world) in self.scene.collect_renderables() {
                if mesh_index < self.meshes.len() {
                    jobs.push(DrawJob { mesh_index, transform: DrawTransform::Camera(world) });
                }
            }

            let view = self.camera.view_matrix();
            let proj = self.camera.projection_matrix();
            let id_matrix = identity();

            // ДОБАВЛЕНО (оптимизация рендера — CPU-side frustum culling,
            // жалоба пользователя на лаги, третье выбранное направление):
            // раньше здесь рисовались АБСОЛЮТНО ВСЕ объекты сцены (965+
            // ECS-сущностей — тайлы пола, столбы фонарей и т.д.) каждый
            // кадр, независимо от того, видны ли они в текущем кадре камеры
            // — отдельный `IASetVertexBuffers`/`IASetIndexBuffer`/
            // `SetGraphicsRootConstantBufferView`/`DrawIndexedInstanced` на
            // КАЖДЫЙ, даже если объект давно за спиной камеры или далеко за
            // пределами FOV. Теперь перед основным циклом отрисовки
            // (`jobs.iter()` ниже) объекты с `DrawTransform::Camera`
            // (реальная world-позиция — то есть подавляющее большинство
            // сцены) фильтруются тестом сферы-в-фрустуме
            // (`crate::math::Frustum::test_sphere`, метод Gribb/Hartmann) —
            // мировой центр/радиус ограничивающей сферы меша (см.
            // `Mesh::bounding_center`/`bounding_radius`, посчитанные ОДИН
            // РАЗ при создании меша) трансформируются текущей model-
            // матрицей объекта и сравниваются с фрустумом камеры этого
            // кадра. `DrawTransform::RawIdentity` (старый 2D-fallback без
            // камеры, main1.rs) НЕ фильтруется — у таких объектов нет
            // осмысленной мировой позиции для теста, и их обычно единицы.
            //
            // Тест по сфере — консервативный (может изредка оставить
            // объект чуть дальше от границы, чем требуется), но НИКОГДА не
            // отбрасывает то, что реально попадает в кадр — сфера строго
            // содержит всю геометрию меша (см. комментарий у
            // `bounding_center`/`bounding_radius`).
            let frustum = crate::math::Frustum::from_view_proj(&(proj * view));
            jobs.retain(|job| match &job.transform {
                DrawTransform::Camera(model) => {
                    let mesh = &self.meshes[job.mesh_index];
                    let (scale, _rotation, _translation) = model.to_scale_rotation_translation();
                    let max_scale = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
                    let local_center = Vec3::new(
                        mesh.bounding_center[0],
                        mesh.bounding_center[1],
                        mesh.bounding_center[2],
                    );
                    let world_center = model.transform_point3(local_center);
                    let world_radius = mesh.bounding_radius * max_scale;
                    frustum.test_sphere(world_center, world_radius)
                }
                DrawTransform::RawIdentity => true,
            });

            self.ensure_constant_buffer_capacity(jobs.len())?;

            for (i, job) in jobs.iter().enumerate() {
                let mesh = &self.meshes[job.mesh_index];

                // ДОБАВЛЕНО (Задача #15): биндим albedo-текстуру ИМЕННО
                // ЭТОГО меша (root-параметр 5, register t6) ПЕРЕД Draw —
                // либо её собственный SRV-слот (`mesh.albedo_srv_index`),
                // либо белую fallback-текстуру, если у меша своей текстуры
                // нет. `shadow_srv_heap` уже забинжен через
                // SetDescriptorHeaps выше (ОДИН раз на кадр, до этого
                // цикла) — здесь только меняем СМЕЩЕНИЕ таблицы
                // (SetGraphicsRootDescriptorTable), сам хип не трогаем.
                if let Some(shadow_srv_heap) = self.shadow_srv_heap.as_ref() {
                    let albedo_slot = mesh.albedo_srv_index.or(white_texture_srv_fallback);
                    if let Some(albedo_slot) = albedo_slot {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, albedo_slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(5, gpu_handle);
                    }

                    // ДОБАВЛЕНО (Задача #15, normal mapping): normal map
                    // (root-параметр 6, register t7) — тот же fallback-принцип,
                    // что у albedo чуть выше.
                    let normal_slot = mesh.normal_srv_index.or(flat_normal_srv_fallback);
                    if let Some(normal_slot) = normal_slot {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, normal_slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(6, gpu_handle);
                    }

                    // ДОБАВЛЕНО (Задача #15, normal mapping): metallic-roughness
                    // map (root-параметр 7, register t8).
                    let mr_slot = mesh.mr_srv_index.or(neutral_mr_srv_fallback);
                    if let Some(mr_slot) = mr_slot {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, mr_slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(7, gpu_handle);
                    }
                }

                // ДОБАВЛЕНО (Задача #15, normal mapping): root constants
                // (root-параметр 8, register b1) — скалярные metallic/
                // roughness ЭТОГО меша + явный флаг hasMrMap (1.0 если у
                // меша есть собственная MR-карта, иначе 0.0 — см. подробный
                // комментарий у MaterialConstants в HLSL). Последнее
                // значение — padding (0.0, не читается шейдером).
                let has_mr_map = if mesh.mr_srv_index.is_some() { 1.0f32 } else { 0.0f32 };
                let mr_constants: [f32; 4] = [mesh.material_metallic, mesh.material_roughness, has_mr_map, 0.0];
                cmd_list.SetGraphicsRoot32BitConstants(8, 4, mr_constants.as_ptr() as *const _, 0);

                match &job.transform {
                    DrawTransform::Camera(model) => {
                        let model = *model;
                        let model_view_proj = proj * view * model;
                        self.transform_constants.model_view_proj = model_view_proj.to_cols_array_2d();
                        self.transform_constants.model = model.to_cols_array_2d();
                        self.transform_constants.view = view.to_cols_array_2d();
                        self.transform_constants.proj = proj.to_cols_array_2d();
                        self.transform_constants.camera_pos = [
                            self.camera.position.x,
                            self.camera.position.y,
                            self.camera.position.z,
                            1.0,
                        ];
                    }
                    DrawTransform::RawIdentity => {
                        self.transform_constants.model_view_proj = id_matrix.to_cols_array_2d();
                        self.transform_constants.model = id_matrix.to_cols_array_2d();
                        self.transform_constants.view = id_matrix.to_cols_array_2d();
                        self.transform_constants.proj = id_matrix.to_cols_array_2d();
                    }
                }

                // Свой слот константного буфера на КАЖДЫЙ Draw — вот что
                // на самом деле чинит баг. Плюс сдвиг на
                // `frame_index * capacity`, чтобы не было гонки между CPU,
                // готовящим кадр N+1, и GPU, ещё дорисовывающим кадр N (по
                // той же причине, по которой у нас уже два command
                // allocator'а — см. ensure_constant_buffer_capacity).
                let slot = frame_index * self.constant_buffer_capacity + i;
                let Some(cb) = self.constant_buffer.as_ref() else {
                    eprintln!("[ENGINE] WARNING: no constant buffer available, skipping draw");
                    continue;
                };
                if let Err(e) = self.transform_constants.write_at(cb, slot) {
                    eprintln!("[ENGINE] WARNING: failed to write constant buffer slot {}: {:?}", slot, e);
                    continue;
                }
                let gpu_addr = TransformConstants::gpu_address_for_slot(cb, slot);
                cmd_list.SetGraphicsRootConstantBufferView(0, gpu_addr);

                let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                    SizeInBytes: mesh.vertex_buffer.size as u32,
                    StrideInBytes: Vertex::STRIDE,
                };
                cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                if let Some(index_buffer) = &mesh.index_buffer {
                    let index_view = D3D12_INDEX_BUFFER_VIEW {
                        BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                        SizeInBytes: index_buffer.size as u32,
                        Format: DXGI_FORMAT_R32_UINT,
                    };
                    cmd_list.IASetIndexBuffer(Some(&index_view));
                    cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
                } else {
                    cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
                }
            }

            // ИСПРАВЛЕНО (E0502, реальная ошибка компиляции — найдена
            // пользователем при сборке на Windows): старая переменная
            // `renderer` (взятая в самом начале функции, см. `let renderer
            // = self.renderer.as_ref()...` выше) держала immutable-заём
            // `self.renderer` живым от начала функции ДО этой точки — а
            // между ними лежат несколько вызовов `&mut self`-методов
            // (`self.ensure_light_buffer_capacity`,
            // `self.ensure_grid_cells_buffer_capacity`,
            // `self.ensure_grid_entries_buffer_capacity`,
            // `self.ensure_constant_buffer_capacity` — все выше, в теле
            // основного draw pass'а). Пока `renderer` использовался только
            // ДО первого такого вызова (как было в коде ДО Фазы 5), это
            // компилировалось: NLL завершает заём в точке последнего
            // использования. Но bloom/tonemap-проход ниже читает
            // `renderer` СНОВА (hdr_target/srv_uav_heap/hdr_srv_gpu) — то
            // есть его "последнее использование" сдвинулось далеко за все
            // эти `&mut self`-вызовы, из-за чего заём оказывается живым
            // ЧЕРЕЗ них — E0502 ("cannot borrow `*self` as mutable because
            // it is also borrowed as immutable"). Исправление: заново
            // перевзять `renderer` здесь, ПОСЛЕ того как все
            // `&mut self`-вызовы этого кадра уже отработали — новый заём
            // начинается только с этой точки и никак не пересекается с
            // ними по времени жизни.
            let renderer = self.renderer.as_ref().ok_or_else(|| {
                eprintln!("[ENGINE] ERROR: render_frame() lost renderer mid-frame (unexpected)");
                Error::from_hresult(HRESULT(1))
            })?;

            // =============================================================
            // ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
            // подсветка): VOLUMETRIC RAYMARCH ПРОХОД, выполняется ПОСЛЕ
            // основного draw pass'а (нужна уже дописанная глубина сцены —
            // depth_stencil сейчас в DEPTH_WRITE, только что заполнен
            // основным 3D-проходом выше) и ПОСЛЕ shadow-прохода (нужна уже
            // готовая shadow map — та переведена в PIXEL_SHADER_RESOURCE
            // ещё до основного 3D-прохода, см. блок Фазы 6 выше), но ДО
            // bloom-блока — bloom реагирует на итоговую яркость сцены,
            // volumetric-свет добавляется в HDR-сумму уже ПОСЛЕ bloom (в
            // tonemap composite-проходе, см. ниже) как отдельное аддитивное
            // слагаемое, а не как то, что само может "цвести" через bloom
            // (god rays физически — рассеянный в воздухе свет, не яркий
            // самосветящийся объект, дополнительное свечение вокруг них
            // было бы визуально неверным).
            if let (Some(volumetric_texture), Some(volumetric_srv_heap), Some(volumetric_cb)) = (
                &self.volumetric_texture,
                &self.volumetric_srv_heap,
                &self.volumetric_constant_buffer,
            ) {
                let vol_width = volumetric_texture.width;
                let vol_height = volumetric_texture.height;

                // depth_stencil: DEPTH_WRITE -> PIXEL_SHADER_RESOURCE.
                // volumetric_texture: (SRV с прошлого кадра, если он
                // выполнялся) -> RENDER_TARGET, чтобы можно было писать в
                // неё этим кадром — на первом кадре она ещё в RENDER_TARGET
                // (создана такой, см. create_hdr_target), поэтому переход
                // добавляется УСЛОВНО, аналогично depth_stencil выше. Оба
                // трекера (depth_stencil_is_srv/volumetric_is_srv), а не
                // хардкод — тот же принцип, что и у shadow_map_is_srv/
                // bloom_a_is_srv (см. подробные комментарии там про класс
                // бага "no-op барьер на первом кадре").
                let mut barriers = Vec::with_capacity(2);
                if !self.depth_stencil_is_srv {
                    barriers.push(Self::transition_barrier(
                        &renderer.depth_stencil.resource,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ));
                }
                if self.volumetric_is_srv {
                    barriers.push(Self::transition_barrier(
                        &volumetric_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ));
                }
                if !barriers.is_empty() {
                    cmd_list.ResourceBarrier(&barriers);
                    for b in barriers {
                        Self::drop_transition_barrier(b);
                    }
                }
                self.depth_stencil_is_srv = true;
                self.volumetric_is_srv = false;

                let vol_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: vol_width as f32,
                    Height: vol_height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                let vol_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: vol_width as i32,
                    bottom: vol_height as i32,
                };
                cmd_list.RSSetViewports(&[vol_viewport]);
                cmd_list.RSSetScissorRects(&[vol_scissor]);

                cmd_list.OMSetRenderTargets(1, Some(&self.volumetric_rtv), false, None);
                cmd_list.SetPipelineState(Some(self.volumetric_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.volumetric_root_signature.as_ref().unwrap()));
                let heaps = [Some(volumetric_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.SetGraphicsRootDescriptorTable(0, self.volumetric_srv_gpu_raymarch);
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                let inv_view_proj = (self.camera.projection_matrix() * self.camera.view_matrix()).inverse();
                let sun_color = [
                    self.transform_constants.light_color[0],
                    self.transform_constants.light_color[1],
                    self.transform_constants.light_color[2],
                ];
                let sun_intensity = self.transform_constants.light_color[3];
                // Итоговая яркость god ray масштабируется ЯРКОСТЬЮ солнца
                // в этот момент (sun_intensity, см. compute_sun_state) — на
                // закате/восходе и ночью эффект сам по себе гаснет вместе
                // с солнцем, отдельно ничего выключать не нужно. Множитель
                // 0.15 — консервативная базовая интенсивность эффекта,
                // подобранная, чтобы god rays были ЗАМЕТНЫ, но не
                // доминировали над освещением сцены целиком.
                let vol_intensity = 0.15 * sun_intensity;

                #[repr(C)]
                struct VolumetricParamsGpu {
                    inv_view_proj: [[f32; 4]; 4],
                    light_view_proj: [[f32; 4]; 4],
                    camera_pos: [f32; 3],
                    intensity: f32,
                    light_dir: [f32; 3],
                    _padding0: f32,
                    light_color: [f32; 3],
                    max_distance: f32,
                }
                // ОБНОВЛЕНО (каскадные тени / CSM): volumetric raymarch —
                // полноэкранный проход без per-пиксельного выбора каскада
                // (см. подробный комментарий у cascade_for_volumetric в
                // create_volumetric_resources) — сознательно берём матрицу
                // ТОЛЬКО каскада 0 (тот же каскад, чей SRV скопирован в
                // volumetric_srv_heap).
                let params = VolumetricParamsGpu {
                    inv_view_proj: inv_view_proj.to_cols_array_2d(),
                    light_view_proj: cascade_view_projs[0].to_cols_array_2d(),
                    camera_pos: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
                    intensity: vol_intensity,
                    light_dir: [light_dir_vec.x, light_dir_vec.y, light_dir_vec.z],
                    _padding0: 0.0,
                    light_color: sun_color,
                    // Дальше этого расстояния от камеры raymarch не идёт —
                    // ограничивает стоимость прохода на открытых
                    // пространствах (город без конца видимости) и не даёт
                    // эффекту "включаться" за пределами frustum-fitted
                    // shadow-объёма (см. compute_cascade_view_proj), где
                    // сравнение с shadow map всё равно теряет смысл.
                    max_distance: self.camera.far.min(150.0),
                };
                let bytes = std::slice::from_raw_parts(
                    &params as *const VolumetricParamsGpu as *const u8,
                    std::mem::size_of::<VolumetricParamsGpu>(),
                );
                let _ = volumetric_cb.update_constant_buffer(bytes);
                cmd_list.SetGraphicsRootConstantBufferView(1, volumetric_cb.resource.GetGPUVirtualAddress());

                cmd_list.DrawInstanced(3, 1, 0, 0);

                // Возвращаем depth_stencil обратно в DEPTH_WRITE — нужен
                // основному 3D-проходу СЛЕДУЮЩЕГО кадра для записи новой
                // глубины (см. ClearDepthStencilView в начале основного
                // прохода выше).
                let depth_back = Self::transition_barrier(
                    &renderer.depth_stencil.resource,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_DEPTH_WRITE,
                );
                // volumetric-таргет: RENDER_TARGET -> PIXEL_SHADER_RESOURCE,
                // читается tonemap composite-проходом ниже (t2).
                let vol_to_srv = Self::transition_barrier(
                    &volumetric_texture.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                let barriers = [depth_back, vol_to_srv];
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.depth_stencil_is_srv = false;
                self.volumetric_is_srv = true;
            }
            // =============================================================

            // =============================================================
            // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям, bloom): BLOOM
            // ПРОХОД (extract -> blur H -> blur V), выполняется ПОСЛЕ
            // основного draw pass'а (HDR target уже полностью готов, всё
            // ещё в состоянии RENDER_TARGET — draw pass выше его никуда не
            // переводил), но ДО tonemap composite-прохода (которому нужен
            // готовый bloom_texture_a с финальным размытым свечением).
            //
            // Bloom-дескрипторный хип (`bloom_srv_heap`, 2 слота: индекс 0
            // = bloom_a SRV, индекс 1 = bloom_b SRV) биндится ОДИН раз на
            // весь bloom-проход — в отличие от HDR SRV (который читается
            // только в extract-проходе из renderer.srv_uav_heap), extract
            // и оба blur-прохода читают ТОЛЬКО текстуры из bloom_srv_heap,
            // поэтому SetDescriptorHeaps для него достаточно вызвать
            // единожды перед extract-проходом — переключать хип между
            // под-проходами не нужно, только сам GPU-адрес таблицы (через
            // SetGraphicsRootDescriptorTable) меняется.
            //
            // Отслеживание состояний по кадрам: bloom_a/bloom_b всегда
            // ЗАКАНЧИВАЮТ кадр в PIXEL_SHADER_RESOURCE (см. финальный
            // барьер в конце этого блока) и создаются изначально в
            // RENDER_TARGET (см. create_hdr_target) — то есть на первом
            // кадре они в RENDER_TARGET, на всех последующих в
            // PIXEL_SHADER_RESOURCE. Чтобы не завязываться на "какой это
            // кадр по счёту", extract-проход НЕ трогает bloom_a барьером
            // до записи в неё (OMSetRenderTargets работает с RTV
            // независимо от текущего resource state — некорректное
            // состояние обнаружится debug-layer'ом, а не тихим багом), а
            // явно переводит её в RENDER_TARGET непосредственно перед
            // записью, аналогично остальным под-проходам ниже — это
            // единообразно и не зависит от истории предыдущих кадров.
            // ИСПРАВЛЕНО (реальный краш на живой машине: render_frame()
            // падал с HRESULT(0x80070057) "Параметр задан неверно" на
            // Close() самого первого кадра): раньше `bloom_ran` не
            // существовало, а composite-проход ниже БЕЗУСЛОВНО переводил
            // `hdr_resource` из RENDER_TARGET в PIXEL_SHADER_RESOURCE —
            // как будто bloom-блок никогда не трогал HDR target. Но
            // extract-под-проход bloom-блока (см. `hdr_to_srv` ниже) уже
            // переводит HDR target в PIXEL_SHADER_RESOURCE ЗАРАНЕЕ (это
            // нужно, чтобы extract-шейдер вообще мог его читать как SRV) и
            // НИКОГДА не переводит обратно в RENDER_TARGET — он ему больше
            // не нужен, дальше bloom работает только со своими
            // bloom_a/bloom_b. В результате composite-барьер заявлял
            // StateBefore=RENDER_TARGET для ресурса, реально уже
            // находящегося в PIXEL_SHADER_RESOURCE — рассинхронизация
            // между заявленным и истинным состоянием ресурса, которую
            // Direct3D12 обнаруживает и отвергает как невалидный параметр
            // (именно так проявляется этот класс багов — не всегда сразу
            // на самом ResourceBarrier(), т.к. он void и ничего не
            // возвращает, а позже — на Close()/ExecuteCommandLists(),
            // когда рантайм проверяет накопленный список команд целиком).
            // Теперь запоминаем, реально ли выполнялся bloom-блок в этом
            // кадре (он пропускается, если bloom-ресурсы почему-то не
            // созданы — см. `if let` ниже), и используем это, чтобы
            // composite-проход НЕ переводил HDR target второй раз, если
            // bloom уже сделал это сам.
            let mut bloom_ran = false;
            if let (Some(bloom_a), Some(bloom_b), Some(bloom_srv_heap)) =
                (&self.bloom_texture_a, &self.bloom_texture_b, &self.bloom_srv_heap)
            {
                bloom_ran = true;
                let bloom_a_resource = &bloom_a.resource;
                let bloom_b_resource = &bloom_b.resource;
                let bloom_width = bloom_a.width;
                let bloom_height = bloom_a.height;

                let bloom_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: bloom_width as f32,
                    Height: bloom_height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                let bloom_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: bloom_width as i32,
                    bottom: bloom_height as i32,
                };

                cmd_list.RSSetViewports(&[bloom_viewport]);
                cmd_list.RSSetScissorRects(&[bloom_scissor]);
                cmd_list.SetGraphicsRootSignature(Some(self.bloom_root_signature.as_ref().unwrap()));
                let heaps = [Some(bloom_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                // --- Под-проход 1: EXTRACT (HDR target -> bloom_a) ---
                // Источник — HDR target, только что дописанный основным
                // draw pass'ом (всё ещё RENDER_TARGET) — переводим в
                // PIXEL_SHADER_RESOURCE здесь; composite-проход ниже уже
                // застаёт его в этом состоянии и не переводит второй раз.
                let hdr_to_srv = Self::transition_barrier(
                    &renderer.hdr_target.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                // StateBefore для bloom_a берём из реального отслеживаемого
                // состояния (`bloom_a_is_srv`), а не из захардкоженного
                // предположения — на первом кадре текстура ещё в
                // RENDER_TARGET (создана такой в create_hdr_target), на всех
                // последующих уже в PIXEL_SHADER_RESOURCE (см. барьер в конце
                // этого блока прошлого кадра). Хардкод StateBefore =
                // PIXEL_SHADER_RESOURCE был бы формально неверным на первом
                // кадре — debug-layer D3D12 обнаружил бы рассинхронизацию
                // между заявленным и реальным состоянием ресурса.
                //
                // ИСПРАВЛЕНО (найдено через D3D12 debug layer на реальной
                // машине пользователя — точный текст: "ResourceBarrier:
                // Before and after states must be different", дважды за
                // кадр, ИМЕННО на первом кадре каждого запуска): раньше этот
                // барьер добавлялся В КОМАНДНЫЙ СПИСОК БЕЗУСЛОВНО, даже
                // когда `a_before == RENDER_TARGET` (что и есть на первом
                // кадре — bloom_a только что создана в create_hdr_target
                // именно в RENDER_TARGET) — то есть барьер "переводил"
                // ресурс из RENDER_TARGET в RENDER_TARGET, не меняя
                // состояние вообще. D3D12 явно запрещает такие no-op
                // барьеры (StateBefore == StateAfter). Тот же баг — для
                // bloom_b ниже (`b_before`). Исправление: собираем барьеры
                // в динамический `Vec` и добавляем переход bloom_a, только
                // если состояние действительно меняется.
                let a_before = if self.bloom_a_is_srv {
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                } else {
                    D3D12_RESOURCE_STATE_RENDER_TARGET
                };
                let mut barriers = Vec::with_capacity(2);
                barriers.push(hdr_to_srv);
                if a_before != D3D12_RESOURCE_STATE_RENDER_TARGET {
                    barriers.push(Self::transition_barrier(
                        bloom_a_resource,
                        a_before,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ));
                }
                // ИСПРАВЛЕНО (E0382 "use of moved value" — реальная ошибка
                // компиляции, найдена пользователем): `D3D12_RESOURCE_BARRIER`
                // НЕ реализует Copy (внутри ManuallyDrop<Option<ID3D12Resource>>
                // — COM-типы не Copy), только Clone. `cmd_list.ResourceBarrier(
                // &[hdr_to_srv, a_to_rt])` создаёт ВРЕМЕННЫЙ массив-литерал —
                // оба элемента ПЕРЕМЕЩАЮТСЯ в него, поэтому последующие
                // `Self::drop_transition_barrier(hdr_to_srv)` — обращение к уже
                // перемещённому значению. Исправление: сначала создаём ИМЕНОВАННЫЙ
                // (владеющий) массив/Vec, передаём в ResourceBarrier ссылку на
                // НЕГО (не перемещая элементы — `&barriers`, а не `&[a, b]`), а
                // затем забираем элементы обратно через `for b in barriers`
                // (перемещение ИЗ коллекции, которая после этого больше не
                // используется) — тот же паттерн, что уже проверен в барьере
                // одиночного HDR-таргета в основном 3D draw pass'е выше в этой
                // же функции.
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_a_is_srv = false; // теперь RENDER_TARGET

                cmd_list.OMSetRenderTargets(1, Some(&self.bloom_rtv_a), false, None);
                cmd_list.SetPipelineState(Some(self.bloom_extract_pipeline_state.as_ref().unwrap()));
                // HDR SRV живёt в renderer.srv_uav_heap, а не в
                // bloom_srv_heap — но GPU-дескрипторный АДРЕС, который мы
                // передаём в SetGraphicsRootDescriptorTable, самодостаточен
                // (он указывает на конкретный дескриптор внутри уже
                // забинженного shader-visible хипа набора). Поскольку прямо
                // сейчас забинжен bloom_srv_heap (см. SetDescriptorHeaps
                // выше), а не renderer.srv_uav_heap, ЭТОТ вызов был бы
                // некорректен, если бы renderer.hdr_srv_gpu указывал в
                // другой хип. Поэтому extract-проход временно перебинживает
                // srv_uav_heap на время своего единственного DrawInstanced,
                // а затем возвращает bloom_srv_heap для blur-проходов ниже.
                let hdr_heap = [Some(renderer.srv_uav_heap.clone())];
                cmd_list.SetDescriptorHeaps(&hdr_heap);
                cmd_list.SetGraphicsRootDescriptorTable(0, renderer.hdr_srv_gpu);
                if let Some(params_cb) = &self.bloom_params_buffer {
                    // threshold=1.0 (см. create_bloom_resources), texel_size не
                    // используется в extract-шейдере.
                    let params: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
                    let bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 16);
                    let _ = params_cb.update_constant_buffer(bytes);
                    cmd_list.SetGraphicsRootConstantBufferView(1, params_cb.resource.GetGPUVirtualAddress());
                }
                cmd_list.DrawInstanced(3, 1, 0, 0);

                // Возвращаем bloom_srv_heap — оба blur-прохода читают
                // ТОЛЬКО из него (bloom_a/bloom_b SRV, индексы 0/1).
                let bloom_heap_rebind = [Some(bloom_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&bloom_heap_rebind);

                // --- Под-проход 2: BLUR ГОРИЗОНТАЛЬНО (bloom_a -> bloom_b) ---
                // bloom_a только что была переведена в RENDER_TARGET и
                // дописана extract-проходом выше — её состояние сейчас
                // достоверно известно без обращения к трекеру.
                let a_to_srv = Self::transition_barrier(
                    bloom_a_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                // ИСПРАВЛЕНО (тот же баг, что и у bloom_a выше — см.
                // подробный комментарий там): на первом кадре bloom_b тоже
                // ещё в RENDER_TARGET (создана такой в create_hdr_target),
                // `b_before` в этом случае равен RENDER_TARGET — безусловный
                // барьер "переводил" бы её из RENDER_TARGET в RENDER_TARGET,
                // что D3D12 отвергает как невалидный параметр. Добавляем
                // переход, только если состояние действительно меняется.
                let b_before = if self.bloom_b_is_srv {
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                } else {
                    D3D12_RESOURCE_STATE_RENDER_TARGET
                };
                let mut barriers = Vec::with_capacity(2);
                barriers.push(a_to_srv);
                if b_before != D3D12_RESOURCE_STATE_RENDER_TARGET {
                    barriers.push(Self::transition_barrier(
                        bloom_b_resource,
                        b_before,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ));
                }
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_a_is_srv = true;  // теперь PIXEL_SHADER_RESOURCE
                self.bloom_b_is_srv = false; // теперь RENDER_TARGET

                cmd_list.OMSetRenderTargets(1, Some(&self.bloom_rtv_b), false, None);
                cmd_list.SetPipelineState(Some(self.bloom_blur_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootDescriptorTable(0, self.bloom_srv_a_gpu);
                if let Some(params_cb) = &self.bloom_params_buffer {
                    let texel_x = 1.0 / bloom_width as f32;
                    let params: [f32; 4] = [1.0, texel_x, 0.0, 0.0]; // горизонталь: (texel_x, 0)
                    let bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 16);
                    let _ = params_cb.update_constant_buffer(bytes);
                    cmd_list.SetGraphicsRootConstantBufferView(1, params_cb.resource.GetGPUVirtualAddress());
                }
                cmd_list.DrawInstanced(3, 1, 0, 0);

                // --- Под-проход 3: BLUR ВЕРТИКАЛЬНО (bloom_b -> bloom_a) ---
                // Результат остаётся в bloom_a — именно её финальный SRV
                // (зарегистрированный в renderer.srv_uav_heap индекс 1, см.
                // create_bloom_resources) читает tonemap composite-проход
                // ниже.
                let b_to_srv = Self::transition_barrier(
                    bloom_b_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                let a_back_to_rt = Self::transition_barrier(
                    bloom_a_resource,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                );
                let barriers = [b_to_srv, a_back_to_rt];
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_b_is_srv = true;  // теперь PIXEL_SHADER_RESOURCE — остаётся так и в конце кадра
                self.bloom_a_is_srv = false; // теперь RENDER_TARGET (временно, до финального барьера ниже)

                cmd_list.OMSetRenderTargets(1, Some(&self.bloom_rtv_a), false, None);
                cmd_list.SetGraphicsRootDescriptorTable(0, self.bloom_srv_b_gpu);
                if let Some(params_cb) = &self.bloom_params_buffer {
                    let texel_y = 1.0 / bloom_height as f32;
                    let params: [f32; 4] = [1.0, 0.0, texel_y, 0.0]; // вертикаль: (0, texel_y)
                    let bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 16);
                    let _ = params_cb.update_constant_buffer(bytes);
                    cmd_list.SetGraphicsRootConstantBufferView(1, params_cb.resource.GetGPUVirtualAddress());
                }
                cmd_list.DrawInstanced(3, 1, 0, 0);

                // Переводим bloom_a (финальный результат) в
                // PIXEL_SHADER_RESOURCE — его читает tonemap composite-проход
                // ниже через renderer.srv_uav_heap индекс 1. bloom_b
                // остаётся в PIXEL_SHADER_RESOURCE (уже переведена барьером
                // b_to_srv выше) — оба bloom-таргета корректно заканчивают
                // кадр в PIXEL_SHADER_RESOURCE, как и ожидается в начале
                // СЛЕДУЮЩЕГО кадра (см. комментарий про отслеживание
                // состояний в начале этого блока).
                let a_final_to_srv = Self::transition_barrier(
                    bloom_a_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                let barriers = [a_final_to_srv];
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_a_is_srv = true; // теперь PIXEL_SHADER_RESOURCE — остаётся так и в конце кадра
            }
            // =============================================================

            // =============================================================
            // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): COMPOSITE/
            // TONEMAP ПРОХОД.
            //
            // К этому моменту весь основной draw pass закончил писать в
            // `renderer.hdr_target` (R16G16B16A16_FLOAT, состояние
            // RENDER_TARGET). Нужно: (1) перевести HDR target в состояние
            // PIXEL_SHADER_RESOURCE, чтобы его можно было читать через SRV;
            // (2) перевести back buffer текущего кадра из PRESENT (в этом
            // состоянии он передаётся движку драйвером после предыдущего
            // Present) в RENDER_TARGET; (3) нарисовать fullscreen-triangle
            // тонмаппинг-шейдером, который читает HDR target и пишет
            // финальный LDR-цвет в back buffer; (4) перевести back buffer
            // обратно в PRESENT (обязательное состояние для Present()); (5)
            // перевести HDR target обратно в RENDER_TARGET, чтобы он был
            // готов для записи основным draw pass'ом уже СЛЕДУЮЩЕГО кадра.
            //
            // Это первое место во всём движке, где вообще вызывается
            // ResourceBarrier — раньше (до Фазы 5) движок писал прямо в
            // back buffer без единого явного перехода состояния (см.
            // комментарий у `transition_barrier` выше).
            let hdr_resource = &renderer.hdr_target.resource;
            let back_buffer_resource = &renderer.back_buffers[frame_index].resource;

            // ИСПРАВЛЕНО (см. подробный комментарий у `bloom_ran` выше):
            // если bloom-блок в этом кадре реально выполнялся, он уже сам
            // перевёл `hdr_resource` в PIXEL_SHADER_RESOURCE (нужно было
            // extract-под-проходу, чтобы прочитать HDR target как SRV) —
            // переводить его туда ВТОРОЙ раз здесь означало бы заявить
            // StateBefore=RENDER_TARGET для ресурса, который на самом деле
            // уже PIXEL_SHADER_RESOURCE. Поэтому барьер для HDR target
            // добавляется в этот список, ТОЛЬКО если bloom не выполнялся
            // (например bloom-ресурсы почему-то не были созданы в init())
            // — тогда HDR target всё ещё в RENDER_TARGET после основного
            // draw pass'а и барьер настоящий.
            let mut barriers_before = Vec::with_capacity(2);
            if !bloom_ran {
                barriers_before.push(Self::transition_barrier(
                    hdr_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ));
            }
            barriers_before.push(Self::transition_barrier(
                back_buffer_resource,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            ));
            cmd_list.ResourceBarrier(&barriers_before);
            for b in barriers_before {
                Self::drop_transition_barrier(b);
            }

            let back_buffer_rtv = renderer.render_target_views[frame_index];
            cmd_list.OMSetRenderTargets(1, Some(&back_buffer_rtv), false, None);
            // Не очищаем back buffer перед tonemap-проходом: fullscreen-
            // triangle покрывает ВЕСЬ экран каждый пиксель ровно один раз,
            // ClearRenderTargetView здесь была бы бесполезной лишней
            // записью в ту же память, которую немедленно перезапишет
            // DrawInstanced ниже.

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);
            let scissor = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            cmd_list.RSSetScissorRects(&[scissor]);

            cmd_list.SetPipelineState(Some(self.tonemap_pipeline_state.as_ref().unwrap()));
            cmd_list.SetGraphicsRootSignature(Some(self.tonemap_root_signature.as_ref().unwrap()));

            // Дескрипторный хип с HDR SRV должен быть забинжен ПЕРЕД
            // SetGraphicsRootDescriptorTable — иначе GPU не знает, откуда
            // читать таблицу дескрипторов (это отдельный
            // SHADER_VISIBLE-хип, не тот же, что RTV/DSV-хипы выше).
            let srv_heaps = [Some(renderer.srv_uav_heap.clone())];
            cmd_list.SetDescriptorHeaps(&srv_heaps);
            cmd_list.SetGraphicsRootDescriptorTable(0, renderer.hdr_srv_gpu);

            // Экспозиция и интенсивность bloom — берём из .alfar, если
            // сцена загружена и её прочитал load_lights_from_alfar, иначе
            // оставляем то, что уже лежит в tonemap_constant_buffer
            // (значения по умолчанию exposure=1.0/bloomIntensity=1.0,
            // записанные один раз в init()).
            if let Some(settings) = &self.light_global_settings {
                if let Some(cb) = &self.tonemap_constant_buffer {
                    let tonemap_data: [f32; 4] = [settings.exposure, settings.bloom_intensity, 0.0, 0.0];
                    let bytes = std::slice::from_raw_parts(tonemap_data.as_ptr() as *const u8, 16);
                    if let Err(e) = cb.update_constant_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить tonemap_constant_buffer: {:?}", e);
                    }
                }
            }
            if let Some(cb) = &self.tonemap_constant_buffer {
                let gpu_addr = cb.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootConstantBufferView(1, gpu_addr);
            }

            cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            // Fullscreen-triangle: 3 вершины, ни вершинного, ни индексного
            // буфера не привязано (VS сам генерирует позиции/UV по
            // SV_VertexID) — сознательно не вызываем IASetVertexBuffers.
            cmd_list.DrawInstanced(3, 1, 0, 0);

            let mut barriers_after = vec![
                Self::transition_barrier(
                    back_buffer_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PRESENT,
                ),
                Self::transition_barrier(
                    hdr_resource,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                ),
            ];
            // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени),
            // ИЗМЕНЕНО (Cascaded Shadow Maps): возвращаем ВСЕ NUM_CASCADES
            // shadow map обратно в DEPTH_WRITE — они понадобятся ЗАПИСЬЮ на
            // САМОМ первом шаге СЛЕДУЮЩЕГО кадра (см. shadow pass в начале
            // render_frame). Условно на `shadow_maps_are_srv[cascade]` (та
            // же защита от no-op барьера, что и у bloom-таргетов) — если
            // shadow pass в этом кадре почему-то не выполнялся (PSO не
            // создан), барьера тоже быть не должно.
            for cascade in 0..NUM_CASCADES {
                if let Some(shadow_map) = &self.shadow_maps[cascade] {
                    if self.shadow_maps_are_srv[cascade] {
                        barriers_after.push(Self::transition_barrier(
                            &shadow_map.resource,
                            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                            D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        ));
                        self.shadow_maps_are_srv[cascade] = false;
                    }
                }
            }
            cmd_list.ResourceBarrier(&barriers_after);
            for b in barriers_after {
                Self::drop_transition_barrier(b);
            }
            // =============================================================

            // ИСПРАВЛЕНО (диагностика реального краша E_INVALIDARG на живой
            // машине): раньше `cmd_list.Close()?;` сразу пробрасывал голый
            // HRESULT дальше через `?`, ничего не объясняя. Close() — самое
            // вероятное место, где рантайм D3D12 обнаруживает накопленные
            // за кадр структурные нарушения (несовпадение состояний
            // ресурсов, недопустимые дескрипторы и т.п.) и возвращает
            // ошибку. Теперь при неудаче печатаем ВСЕ сообщения debug
            // layer'а (см. `dump_d3d12_debug_messages()` в lib.rs) ПЕРЕД
            // тем, как вернуть ошибку — они называют ИМЕННО нарушенный
            // вызов и правило, а не просто код ошибки.
            if let Err(e) = cmd_list.Close() {
                eprintln!("[ENGINE] cmd_list.Close() failed: {:?}", e);
                crate::dump_d3d12_debug_messages();
                return Err(e);
            }
        }

        // ИСПРАВЛЕНО: было `state.command_queue.as_ref().unwrap().clone()`.
        let queue = crate::get_command_queue()?;

        let cmd_lists: &[Option<ID3D12CommandList>] = &[Some(cmd_list.into())];
        unsafe {
            queue.ExecuteCommandLists(cmd_lists);
        }

        // ИСПРАВЛЕНО: было `state.swap_chain.as_ref().unwrap().clone()`.
        let swap_chain = crate::get_swap_chain()?;

        // ИСПРАВЛЕНО (главная правка для продакшн-надёжности): раньше
        // ошибка Present() тихо проглатывалась (`let _ = swap_chain.Present(...)`),
        // из-за чего движок мог продолжать рендерить кадр за кадром в уже
        // потерянное устройство (TDR, DXGI_ERROR_DEVICE_REMOVED и т.п.),
        // ничего об этом не зная. Теперь ошибка проверяется явно: если
        // устройство реально потеряно, мы это логируем с точной причиной
        // через `device_removed_reason()` и возвращаем Err — вызывающий
        // код (main.rs) уже и так корректно останавливает цикл при Err из
        // render_frame().
        unsafe {
            let hr = swap_chain.Present(1, DXGI_PRESENT(0));
            if hr.is_err() {
                eprintln!("[ENGINE] Present failed: {:?}", hr);
                if let Some(reason) = crate::device_removed_reason() {
                    eprintln!("[ENGINE] Device removed, reason: {}", reason);
                }
                crate::dump_d3d12_debug_messages();
                return Err(Error::from_hresult(hr));
            }
        }

        // ИСПРАВЛЕНО: раньше сразу здесь стоял busy-wait на fence.GetCompletedValue(),
        // блокирующий CPU до полного завершения GPU-работы этого кадра —
        // то есть render_frame() не возвращал управление, пока GPU
        // реально не дорисует кадр. Теперь мы просто сигналим новое
        // значение и ЗАПОМИНАЕМ его для этого frame_index — реальное
        // ожидание произойдёт только тогда, когда этот же frame_index
        // понадобится снова (см. начало функции). Это позволяет CPU и
        // GPU работать конвейерно, как и предполагает двойная буферизация.
        let fence = crate::get_fence()?;
        let fence_value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
        unsafe {
            queue.Signal(&fence, fence_value)?;
        }
        if frame_index < self.frame_fence_values.len() {
            self.frame_fence_values[frame_index] = fence_value;
        }

        {
            let mut state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
            }
        }

        Ok(true)
    }

    // ================================================================
    // МЕТОДЫ ПЛАГИНОВ
    // ================================================================

    /// ИСПРАВЛЕНО (Задача #16 плана — физика и коллизии): раньше путь к
    /// плагину был захардкожен как `"plugins/inertial.dll"` внутри этого
    /// метода — ни такой папки, ни файла с таким именем реально не
    /// существовало (физика ни разу не вызывалась ни одним bin/*.rs,
    /// ровно та же ситуация, что была с `init_lights`/FirstFires до
    /// соответствующего фикса, см. её комментарий чуть ниже). Теперь путь
    /// передаётся параметром — тем же способом и с тем же fallback на
    /// `deps/` (см. `deps_fallback_path`), что и `init_lights`, вместо
    /// того чтобы изобретать второй, чуть отличающийся механизм для
    /// второго плагина.
    pub fn init_physics(&mut self, dll_path: &str, config: PhysicsConfig) -> Result<()> {
        let fallback_path = Self::deps_fallback_path(dll_path);
        let primary_exists = std::path::Path::new(dll_path).exists();

        let (used_path, load_result) = if primary_exists {
            (dll_path.to_string(), PhysicsPlugin::load(dll_path, config))
        } else if let Some(fallback) = &fallback_path {
            eprintln!(
                "[ENGINE] '{}' не найден, пробую запасной путь '{}' (Cargo не скопировал .dll из deps/)",
                dll_path, fallback
            );
            (fallback.clone(), PhysicsPlugin::load(fallback, config))
        } else {
            (dll_path.to_string(), PhysicsPlugin::load(dll_path, config))
        };

        match load_result {
            Ok(plugin) => {
                self.physics = Some(plugin);
                println!("[ENGINE] ✓ Physics plugin loaded from '{}'", used_path);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to load physics plugin '{}': {}", used_path, e);
                Err(Error::from_hresult(HRESULT(1)))
            }
        }
    }

    /// ДОБАВЛЕНО (диагностика — жалоба пользователя "всё равно ФПС не
    /// радует" ПОСЛЕ фиксов стриминга/hot-reload/culling): публичный
    /// доступ к статистике физики этого кадра (см. `PhysicsPlugin::get_stats`)
    /// — `None`, если физика не инициализирована. Позволяет вызывающему
    /// коду (см. `run_loop` в bin/main.rs) реально ИЗМЕРИТЬ время
    /// broad/narrow phase и солвера, число тел/активных тел/контактов/пар,
    /// вместо того чтобы гадать по одной лишь позиции тестовых сфер.
    pub fn physics_stats(&self) -> Option<PhysicsStats> {
        self.physics.as_ref().map(|p| p.get_stats())
    }

    /// ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит на
    /// месте" ПОСЛЕ фиксов стриминга/hot-reload/culling/физики): отдаёт
    /// накопленную за текущее окно разбивку `update()` по под-фазам (см.
    /// `UpdateBreakdownMs`/макрос `timed!` внутри `update()`) и СРАЗУ
    /// сбрасывает её в нули — тот же паттерн, что уже `max_update_ms`/
    /// `max_render_ms` в bin/main.rs (там ручной сброс раз в секунду
    /// снаружи; здесь сброс происходит прямо тут, чтобы вызывающий код в
    /// bin/main.rs не мог забыть его сделать и не удвоил логику двух
    /// разных мест сброса).
    pub fn take_update_breakdown(&mut self) -> UpdateBreakdownMs {
        std::mem::take(&mut self.update_breakdown_ms)
    }

    /// ИСПРАВЛЕНО (обновление bin/*.rs под текущий движок): раньше путь к
    /// плагину был захардкожен как `"plugins/firstfires.dll"` — ни такой
    /// папки, ни файла с таким именем реально не существует. FirstFires —
    /// отдельный крейт `alkash3d-firstfires` (см. его Cargo.toml), Cargo
    /// заменяет дефисы на подчёркивания в имени артефакта, поэтому
    /// настоящий файл называется `alkash3d_firstfires.dll` и лежит в
    /// `alkash3d-FirstFires/target/<profile>/` — СОСЕДНЕЙ папке относительно
    /// `alkash3d-rust` (обе — подпапки одного репозитория), а не в
    /// подпапке `plugins/` внутри `alkash3d-rust`. Раньше это не
    /// проявлялось как ошибка ТОЛЬКО потому, что `init_lights()` нигде не
    /// вызывался — ни один bin/*.rs не подключал фонари. Теперь путь
    /// передаётся параметром (а не хардкодится здесь) — вызывающий код
    /// (main.rs) сам знает свою рабочую директорию относительно репозитория.
    ///
    /// ДОБАВЛЕНО: на практике на реальной машине Cargo иногда не копирует
    /// готовую `.dll` из `target/<profile>/deps/` в верхний уровень
    /// `target/<profile>/` (там остаются только `.dll.exp`/`.dll.lib`/
    /// `.pdb`, а сама `.dll` — только в `deps/`) — похоже на гонку с файлом,
    /// занятым предыдущим запущенным процессом движка на Windows во время
    /// пересборки. Поэтому если `dll_path` не найден, пробуем тот же файл
    /// внутри соседней папки `deps/` (тот же каталог + "deps/" + имя файла)
    /// как запасной вариант, прежде чем сдаваться.
    pub fn init_lights(&mut self, dll_path: &str, device_ptr: *mut std::ffi::c_void, config: LightConfig) -> Result<()> {
        let fallback_path = Self::deps_fallback_path(dll_path);
        let primary_exists = std::path::Path::new(dll_path).exists();

        let (used_path, load_result) = if primary_exists {
            (dll_path.to_string(), LightPlugin::load(dll_path, device_ptr, config))
        } else if let Some(fallback) = &fallback_path {
            eprintln!(
                "[ENGINE] '{}' не найден, пробую запасной путь '{}' (Cargo не скопировал .dll из deps/)",
                dll_path, fallback
            );
            (fallback.clone(), LightPlugin::load(fallback, device_ptr, config))
        } else {
            (dll_path.to_string(), LightPlugin::load(dll_path, device_ptr, config))
        };

        match load_result {
            Ok(plugin) => {
                self.lights = Some(plugin);
                println!("[ENGINE] ✓ Light plugin loaded from '{}'", used_path);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to load light plugin '{}': {}", used_path, e);
                Err(Error::from_hresult(HRESULT(1)))
            }
        }
    }

    /// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): создаёт
    /// `AudioEngine` (XAudio2 + mastering voice) — в отличие от
    /// `init_physics`/`init_lights`, не грузит внешний DLL (нет пути/
    /// device_ptr параметров) — сам XAudio2 является системным API, не
    /// плагином движка. Безопасно вызывать даже на системах без звукового
    /// устройства вообще (XAudio2 создаёт "null" render endpoint в этом
    /// случае, а не возвращает ошибку) — так что `init_audio` не должен
    /// проваливаться на "железе 10-летней давности" из ТЗ движка, даже
    /// если у конкретной машины отключена/не установлена звуковая карта.
    pub fn init_audio(&mut self) -> Result<()> {
        match AudioEngine::new() {
            Ok(engine) => {
                self.audio = Some(engine);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to initialize audio engine: {}", e);
                Err(Error::from_hresult(HRESULT(1)))
            }
        }
    }

    /// Загружает `.alsnd` банк звуков + связанные `.wav` файлы (см.
    /// `AudioEngine::load_bank`) — не делает ничего (тихо возвращает 0),
    /// если `init_audio()` ещё не вызывался, тот же принцип "деградируем
    /// молча, не паникуем", что и у остальных опциональных плагинов
    /// движка при обращении к ним до инициализации.
    pub fn load_sound_bank(&mut self, alsnd_path: &str, base_dir: &str) -> Result<usize> {
        match &mut self.audio {
            Some(audio) => audio.load_bank(alsnd_path, base_dir).map_err(|e| {
                eprintln!("[ENGINE] Failed to load sound bank '{}': {}", alsnd_path, e);
                Error::from_hresult(HRESULT(1))
            }),
            None => {
                eprintln!("[ENGINE] load_sound_bank called before init_audio() — банк не загружен");
                Ok(0)
            }
        }
    }

    /// Проигрывает звук по имени из загруженного банка (см.
    /// `AudioEngine::play_sound_by_name`) — `None`, если аудио-движок не
    /// инициализирован, звук не найден, или достигнут лимит
    /// `max_instances` (диагностика печатается в stderr самим
    /// `AudioEngine`, чтобы вызывающий код мог не проверять возврат на
    /// каждый выстрел/шаг, если ему не критично знать об отказе).
    pub fn play_sound(&mut self, name: &str, position: Vec3) -> Option<crate::audio::SoundHandle> {
        let audio = self.audio.as_mut()?;
        match audio.play_sound_by_name(name, position) {
            Ok(handle) => Some(handle),
            Err(e) => {
                eprintln!("[ENGINE] play_sound('{}') failed: {}", name, e);
                None
            }
        }
    }

    /// Строит запасной путь вида `.../target/<profile>/deps/<файл>.dll` из
    /// исходного `.../target/<profile>/<файл>.dll`, вставляя "deps" перед
    /// именем файла. Возвращает `None`, если у пути нет родительской папки
    /// (не должно происходить для реальных путей вида
    /// "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll").
    ///
    /// ИЗМЕНЕНО (рефакторинг — вынос скриптинга в engine/scripting.rs):
    /// было `fn` (приватная в пределах mod.rs) — `pub(super)` вместо
    /// `fn`/`pub`, потому что приватность в Rust действует по МОДУЛЯМ, а
    /// не по типу: `impl AlkashEngine` в scripting.rs (отдельный
    /// подмодуль `engine::scripting`) вызывает `Self::deps_fallback_path`
    /// и без `pub(super)` не увидел бы приватный метод соседнего модуля,
    /// даже у того же самого типа. `pub(super)` — “видно предку модуля
    /// engine и его подмодулям”, не публичный API крейта наружу.
    pub(super) fn deps_fallback_path(dll_path: &str) -> Option<String> {
        let path = std::path::Path::new(dll_path);
        let file_name = path.file_name()?;
        let parent = path.parent()?;
        Some(parent.join("deps").join(file_name).to_string_lossy().into_owned())
    }

    pub fn add_physics_body(&mut self, body: PhysicsBody) -> Option<i32> {
        self.physics.as_mut().map(|p| p.add_body(&body))
    }

    pub fn add_sphere_body(&mut self, x: f32, y: f32, z: f32, mass: f32) -> Option<i32> {
        let body = PhysicsBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.5,
            friction: 0.5,
            linear_damping: 0.01,
            angular_damping: 0.01,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
        };
        self.physics.as_mut().map(|p| p.add_body(&body))
    }

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): удобный
    /// "всё-в-одном" хелпер для демо/игрового кода — создаёт физическое
    /// тело-сферу (через `add_sphere_body`, та же формула inv_mass/
    /// is_static), СРАЗУ создаёт для него видимый меш-инстанс
    /// (`spawn_mesh_entity`) и регистрирует связь в `physics_links`, чтобы
    /// `sync_physics_transforms()` начал обновлять его `Transform` со
    /// следующего кадра. Без этого хелпера пришлось бы вручную
    /// синхронизировать три вызова (`add_sphere_body` +
    /// `spawn_mesh_entity` + `physics_links.push`) в каждом месте
    /// демо-кода, что легко забыть или рассинхронизировать.
    ///
    /// Возвращает `None`, если физический плагин не загружен
    /// (`init_physics` не вызывался или провалился) — в этом случае НЕ
    /// создаёт и визуальную сущность тоже, чтобы не оставлять "мёртвую"
    /// геометрию без физики за спиной у вызывающего кода.
    pub fn spawn_physics_sphere(&mut self, mesh_index: usize, x: f32, y: f32, z: f32, mass: f32) -> Option<(i32, crate::scene::EntityId)> {
        let body_id = self.add_sphere_body(x, y, z, mass)?;
        let entity = self.spawn_mesh_entity(mesh_index);
        if let Some(t) = self.scene.transform_mut(entity) {
            t.position = [x, y, z];
        }
        self.physics_links.push((body_id, entity));
        Some((body_id, entity))
    }

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): проецирует
    /// текущее состояние каждого связанного физического тела
    /// (`PhysicsPlugin::get_body`) на позицию его визуальной ECS-сущности.
    /// Вызывается из `update()` СРАЗУ ПОСЛЕ `physics.update(dt, gravity)`
    /// — то есть уже после того, как плагин посчитал интегрирование и
    /// разрешил столкновения этого кадра, но ДО render_frame(), которая
    /// читает `Transform` для построения матриц мира (см. render_frame,
    /// проход по `mesh_instances`/сцене).
    ///
    /// Ничего не делает (тихо), если физика не инициализирована — в этом
    /// случае `physics_links` попросту пуст (см. `spawn_physics_sphere`,
    /// единственная точка добавления записей в него).
    fn sync_physics_transforms(&mut self) {
        if self.physics.is_none() || self.physics_links.is_empty() {
            return;
        }
        for i in 0..self.physics_links.len() {
            let (body_id, entity) = self.physics_links[i];
            let Some(physics) = self.physics.as_ref() else { break };
            let body = physics.get_body(body_id);
            if let Some(t) = self.scene.transform_mut(entity) {
                t.position = body.position;
            }
        }
    }

    /// ДОБАВЛЕНО (Фаза 1 плана по реализму/фонарям): загружает .alfar с
    /// диска и превращает каждый `IndividualLight` в `GPULight`, добавляя
    /// его в уже инициализированный `LightPlugin` (FirstFires) — то есть в
    /// ТОТ ЖЕ путь данных, которым раньше пользовался только
    /// `add_street_light`.
    ///
    /// До этого метода .alfar использовался ТОЛЬКО на запись (`save()`) —
    /// у движка не было пути "прочитать файл со светом обратно". Теперь
    /// есть: `.alfar` можно готовить заранее (в т.ч. через
    /// `AlfarFile::create_night_city()` или редактором в будущем) и
    /// загружать в сцену одним вызовом.
    ///
    /// ВАЖНО про формат конвертации `IndividualLight` -> `GPULight` (см.
    /// `plugin/light_api.rs`, layout подтверждён по `light_culling.hlsl` и
    /// по demo.rs FirstFires):
    ///   position = [x, y, z, light_type]   (w = тип: 0=point,1=spot,2=dir)
    ///   color    = [r, g, b, intensity]
    ///   direction= [dx, dy, dz, range]
    ///   params   = [spot_outer_angle, falloff_type, spot_inner_angle, padding(0)]
    ///
    /// ОБНОВЛЕНО (Фаза 4 плана по реализму/фонарям): params.z раньше был
    /// зарезервирован под "lod" (см. комментарий в GPULight в
    /// alkash3d-FirstFires/src/lib.rs), но реально нигде в цепочке
    /// FirstFires -> движок не читался и не записывался как LOD — сам LOD
    /// вычисляется отдельно внутри `LightState::cull()` и хранится в
    /// `LightGridEntry.lod_level`, а не в GPULight.params.z. Поэтому это
    /// поле было мёртвым — переиспользовано под `spot_inner_angle`, чтобы
    /// не расширять GPULight ещё одним полем (и не трогать ABI FirstFires
    /// повторно вдобавок к Фазе 3). Если это поле когда-нибудь понадобится
    /// именно под LOD — потребуется либо снова его переиспользовать другим
    /// способом, либо добавить пятое поле в GPULight.
    ///
    /// Не переносится в этой фазе (сознательно отложено, а не забыто —
    /// сохранённые вместе поля будут нужны в следующих фазах, а не сейчас):
    /// casts_shadows/shadow_bias/shadow_resolution (эти три — часть Фазы 6,
    /// уже сделанной для одного directional-света "солнца", но per-light
    /// точечные/spot тени в отдельные shadow map всё ещё не реализованы —
    /// остаются на будущее улучшение), falloff_custom (полноценный
    /// IES-профиль, возможное будущее улучшение Фазы 4 для топового
    /// железа).
    ///
    /// ОБНОВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
    /// мерцание): flicker_enabled/flicker_speed/flicker_intensity и
    /// active_from/active_to теперь СОХРАНЯЮТСЯ (см. `ManagedLight` и
    /// `self.managed_lights` выше) — `update_day_night` каждый кадр
    /// использует их, чтобы промодулировать intensity уже добавленного в
    /// FirstFires источника через `LightPlugin::update_light`.
    ///
    /// `enabled == 0` источники пропускаются — нет смысла тратить слот в
    /// FirstFires на свет, который автор сцены явно выключил.
    pub fn load_lights_from_alfar(&mut self, path: &str) -> std::io::Result<u32> {
        let alfar = crate::alfar_format::AlfarFile::load(path)?;

        let mut added = 0u32;
        for light in &alfar.lights {
            if light.enabled == 0 {
                continue;
            }

            let light_type = light.light_type as f32; // 0=Point,1=Spot,2=Directional,3=Area (см. LightType)
            let gpu_light = GPULight {
                position: [light.position[0], light.position[1], light.position[2], light_type],
                color: [light.color[0], light.color[1], light.color[2], light.intensity],
                direction: [light.direction[0], light.direction[1], light.direction[2], light.range],
                params: [light.spot_outer_angle, light.falloff_type as f32, light.spot_inner_angle, 0.0],
            };

            if let Some(firstfires_id) = self.lights.as_mut().map(|l| l.add_light(&gpu_light)) {
                added += 1;

                // ДОБАВЛЕНО (Фаза 7): запоминаем всё, что нужно
                // update_day_night, чтобы этот источник мог мерцать и
                // включаться/выключаться по времени суток. Directional-свет
                // (light_type == 2, "солнце") сюда сознательно НЕ
                // попадает — его цвет/направление/интенсивность целиком
                // управляются кривой день/ночь напрямую через
                // transform_constants (см. compute_sun_state), а не через
                // FirstFires/GPULight (directional-свет не участвует в
                // per-pixel light culling FirstFires, он всегда один и
                // читается шейдером напрямую из TransformConstants.light_dir/
                // light_color).
                if light.light_type != crate::alfar_format::LightType::Directional as u32 {
                    self.managed_lights.push(ManagedLight {
                        firstfires_id,
                        position: light.position,
                        light_type,
                        color: light.color,
                        base_intensity: light.intensity,
                        direction: light.direction,
                        range: light.range,
                        params: [light.spot_outer_angle, light.falloff_type as f32, light.spot_inner_angle, 0.0],
                        flicker_enabled: light.flicker_enabled != 0,
                        flicker_speed: light.flicker_speed,
                        flicker_intensity: light.flicker_intensity,
                        active_from: light.active_from,
                        active_to: light.active_to,
                    });
                    self.flicker_phase.push(0.0);
                }
            } else {
                eprintln!("[ENGINE] load_lights_from_alfar: LightPlugin не инициализирован (вызови init_lights() раньше) — свет '{}' пропущен", light.id);
                break;
            }
        }

        println!(
            "[ENGINE] ✓ .alfar загружен: '{}' — {} из {} источников добавлено в LightPlugin ({} под управлением день/ночь+мерцание)",
            path, added, alfar.lights.len(), self.managed_lights.len()
        );

        self.light_ambient = Some(alfar.ambient);
        self.light_global_settings = Some(alfar.global_settings);

        Ok(added)
    }

    // =========================================================================
    // ДОБАВЛЕНО (World Streaming — подключение .alworld к движку)
    // =========================================================================

    /// Загружает .alworld файл (метаданные мира — где какие чанки, размер
    /// чанка, streaming config) и переводит движок в режим стриминга этого
    /// мира. НЕ загружает содержимое ни одного чанка сразу — это делает
    /// `update_world_streaming`, вызываемый каждый кадр из `update()`, по
    /// мере приближения камеры (тот же принцип "загружаем только то, что
    /// реально нужно прямо сейчас", ради которого стриминг вообще
    /// существует).
    ///
    /// `chunks_dir` — папка, где лежат файлы содержимого чанков
    /// (`chunk_{x}_{y}_{z}.alwchunk`, см. `chunk_file_path`). Если `None`,
    /// используется подпапка `chunks` рядом с самим .alworld файлом —
    /// разумный дефолт, соответствующий тому, как `create_open_world_demo`-
    /// подобные инструменты обычно раскладывают файлы на диске.
    pub fn load_world(&mut self, alworld_path: &str, chunks_dir: Option<&str>) -> std::io::Result<()> {
        let world_file = crate::alworld_format::AlworldFile::load(alworld_path)?;

        let chunks_dir = match chunks_dir {
            Some(dir) => std::path::PathBuf::from(dir),
            None => {
                let mut dir = std::path::PathBuf::from(alworld_path);
                dir.pop(); // убираем имя файла, оставляем родительскую папку
                dir.push("chunks");
                dir
            }
        };

        let chunk_count = world_file.chunks.len();
        println!(
            "[ENGINE] ✓ Мир загружен: {} ({} чанков по {}м, дальность загрузки {}м/выгрузки {}м)",
            alworld_path, chunk_count, world_file.header.chunk_size,
            world_file.streaming_config.load_distance, world_file.streaming_config.unload_distance,
        );

        self.world = Some(WorldStreamingState {
            world_file,
            chunks_dir,
            chunk_states: vec![ChunkRuntimeState::default(); chunk_count],
            // Заведомо "далеко" от любой разумной позиции камеры —
            // гарантирует, что ПЕРВЫЙ вызов update_world_streaming() после
            // load_world() реально выполнит полный пересчёт (а не решит,
            // что камера "не сдвинулась" от дефолтного Vec3::ZERO, если
            // тот случайно совпадёт со стартовой позицией камеры).
            last_streaming_origin: Vec3::new(f32::MAX, f32::MAX, f32::MAX),
            frames_since_streaming_update: WORLD_STREAMING_INTERVAL_FRAMES,
            loaded_chunk_count: 0,
            pending_load: Vec::new(),
            pending_unload: Vec::new(),
        });

        Ok(())
    }

    /// ДОБАВЛЕНО (World Streaming — подключение к движку): создаёт
    /// небольшой демонстрационный мир на диске (см.
    /// `AlworldFile::create_and_save_demo_world`) и сразу загружает его
    /// через `load_world` — удобный способ проверить стриминг за один
    /// вызов, без ручной подготовки .alworld/.alwchunk файлов. `dir` —
    /// папка, куда будет сохранён демо-мир (например, рядом с exe).
    pub fn load_demo_world(&mut self, dir: &str) -> std::io::Result<()> {
        let alworld_path = crate::alworld_format::AlworldFile::create_and_save_demo_world(dir)?;
        self.load_world(&alworld_path, None)
    }

    /// Выгружает ВСЕ загруженные чанки (despawn всех их сущностей из
    /// Scene) и сбрасывает состояние стриминга — используется, например,
    /// при переходе на другой уровень/мир, чтобы не оставлять "осиротевшие"
    /// сущности предыдущего мира в Scene.
    pub fn unload_world(&mut self) {
        if let Some(mut world) = self.world.take() {
            // ДОБАВЛЕНО (объединённая сцена — физика из .alworld): та же
            // причина, что и в `unload_chunk` — тела, созданные для
            // объектов чанков этого мира, должны быть удалены из плагина
            // Inertial здесь тоже, иначе они бы продолжали существовать (и
            // тратить CPU в солвере) даже после того, как весь мир,
            // которому они принадлежали, выгружен целиком.
            let mut all_physics_bodies = Vec::new();
            for state in &mut world.chunk_states {
                for entity in state.spawned_entities.drain(..) {
                    self.scene.despawn(entity);
                }
                all_physics_bodies.extend(state.spawned_physics_bodies.drain(..));
                state.loaded = false;
            }
            if !all_physics_bodies.is_empty() {
                if let Some(physics) = self.physics.as_mut() {
                    for body_id in &all_physics_bodies {
                        physics.remove_body(*body_id);
                    }
                }
                self.physics_links.retain(|(id, _)| !all_physics_bodies.contains(id));
            }
            println!("[ENGINE] Мир выгружен, все чанки despawn'нуты");
        }
    }

    /// Путь к файлу содержимого чанка с заданными сеточными координатами —
    /// единая точка формирования имени файла, используется и при загрузке
    /// (`load_chunk`), и внешними инструментами экспорта мира должны
    /// придерживаться того же соглашения об именовании
    /// (`chunk_{x}_{y}_{z}.alwchunk`), чтобы `load_chunk` их находил.
    fn chunk_file_path(chunks_dir: &std::path::Path, chunk: &crate::alworld_format::ChunkDescriptor) -> std::path::PathBuf {
        chunks_dir.join(format!("chunk_{}_{}_{}.alwchunk", chunk.grid_x, chunk.grid_y, chunk.grid_z))
    }

    /// Загружает содержимое ОДНОГО чанка (`ChunkContent` с диска, см.
    /// `alworld_format.rs`) и спавнит каждый его объект как отдельную
    /// сущность Scene (`Transform` из объектной 4x4-матрицы + `MeshRenderer`,
    /// ссылающийся на mesh_index geometry этого объекта). Если файл
    /// содержимого чанка отсутствует на диске (например, чанк объявлен в
    /// .alworld, но экспортёр мира ещё не сгенерировал для него данные) —
    /// НЕ ошибка, чанк просто помечается загруженным без объектов (пустой
    /// чанк, например открытое поле/вода без построек, вполне легитимен).
    ///
    /// ДОБАВЛЕНО (Задача #14): геометрия объектов чанка теперь загружается
    /// из реального `.altex` файла по пути объекта (см. `load_object_mesh`)
    /// вместо всегда-плейсхолдера. Fallback на единичный куб
    /// (`load_placeholder_mesh`) остаётся ТОЛЬКО для случаев отсутствующего/
    /// повреждённого файла или служебного пути "placeholder" (см.
    /// `AlworldFile::create_and_save_demo_world`) — не блокирует стриминг
    /// целиком из-за одного плохого ассета.
    fn load_chunk(&mut self, chunk_idx: usize) {
        let (chunk_path, chunk_desc) = {
            let world = match &self.world {
                Some(w) => w,
                None => return,
            };
            let chunk = &world.world_file.chunks[chunk_idx];
            (Self::chunk_file_path(&world.chunks_dir, chunk), *chunk)
        };

        let content = match crate::alworld_format::ChunkContent::load_from_file(chunk_path.to_string_lossy().as_ref()) {
            Ok(c) => c,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "[ENGINE] WARNING: не удалось прочитать чанк {:?} ({},{},{}): {:?}",
                        chunk_path, chunk_desc.grid_x, chunk_desc.grid_y, chunk_desc.grid_z, e
                    );
                }
                // Файла нет (или он повреждён) — считаем чанк пустым, а не
                // блокируем стриминг целиком из-за одного отсутствующего
                // файла; помечаем загруженным ниже, чтобы не пытаться
                // читать этот же несуществующий файл КАЖДЫЙ раз, когда
                // update_world_streaming() пересчитывает окрестность
                // камеры (см. WORLD_STREAMING_INTERVAL_FRAMES).
                crate::alworld_format::ChunkContent::new()
            }
        };

        let mut spawned = Vec::with_capacity(content.objects.len());
        // ДОБАВЛЕНО (объединённая сцена — физика из .alworld): id
        // физических тел, созданных для объектов ЭТОГО чанка — собираются
        // отдельно от `spawned` (визуальных сущностей), т.к. это разные
        // системы (см. `ChunkRuntimeState::spawned_physics_bodies`).
        let mut spawned_bodies = Vec::new();
        for obj in &content.objects {
            let m = &obj.transform;
            // Транспонируем [f32;16] (row-major, как в .alworld/.altex
            // форматах — та же конвенция, что уже используют
            // MeshInstance::transform_matrix и остальные загрузчики этого
            // движка) в позицию — для Transform-компонента Scene нужна
            // ТОЛЬКО позиция/поворот/масштаб (а не полная матрица), берём
            // позицию из последней строки row-major 4x4 (индексы 12,13,14
            // — стандартное расположение трансляции в row-major affine
            // матрице).
            let position = [m[12], m[13], m[14]];

            // ДОБАВЛЕНО (загрузчик .altex): реальный путь к geometry-файлу
            // объекта вместо игнорировавшегося раньше поля. Один объект
            // чанка может ссылаться на .altex с НЕСКОЛЬКИМИ мешами (см.
            // `AltexFile::meshes`) — в этом случае размещаем все меши
            // объекта в одной мировой позиции (типичный случай — составной
            // меш с несколькими материалами/подобъектами одной модели,
            // например фонарный столб = "столб" + "плафон" в одном файле).
            let altex_path = content.get_string(obj.altex_path_string_id).to_string();
            let mesh_indices = self.load_object_mesh(&altex_path);

            // ДОБАВЛЕНО (объединённая сцена — физика из .alworld): ОДНО
            // физическое тело на объект чанка (не на под-меш) — создаётся
            // ДО цикла по mesh_indices ниже и привязывается к ПЕРВОЙ
            // заспавненной сущности объекта. Составной меш с несколькими
            // под-мешами физически двигается и вращается как единое целое
            // (реальный физический смысл — это одна твёрдая модель типа
            // "столб + плафон", а не несколько независимо падающих
            // частей), поэтому создание тела на каждый под-меш было бы
            // не только лишней нагрузкой на солвер, но и физически неверно
            // (несколько несвязанных тел проваливались бы друг сквозь
            // друга/разъезжались вместо движения как один объект).
            // Известное ограничение этой версии: `sync_physics_transforms`
            // обновляет `Transform` только ПЕРВОЙ сущности объекта — для
            // составных физических объектов (больше одного mesh_index)
            // остальные под-меши останутся в исходной позиции, пока не
            // будет реализована групповая привязка нескольких сущностей к
            // одному телу. Для подавляющего большинства физических
            // объектов (ящики, бочки, обломки — как правило один
            // mesh_index на .altex) это ограничение не проявляется.
            let physics_body_id = if obj.flags & crate::alworld_format::CHUNK_OBJECT_FLAG_HAS_PHYSICS != 0 {
                self.add_sphere_body(position[0], position[1], position[2], obj.mass)
            } else {
                None
            };

            let mut first_entity_of_object: Option<crate::scene::EntityId> = None;
            for mesh_index in mesh_indices {
                let entity = self.scene.spawn();
                if let Some(transform) = self.scene.transform_mut(entity) {
                    transform.position = position;
                }
                self.scene.add_mesh_renderer(entity, mesh_index);
                if first_entity_of_object.is_none() {
                    first_entity_of_object = Some(entity);
                }
                spawned.push(entity);
            }

            if let (Some(body_id), Some(entity)) = (physics_body_id, first_entity_of_object) {
                self.physics_links.push((body_id, entity));
                spawned_bodies.push(body_id);
            }
        }

        if let Some(world) = &mut self.world {
            let state = &mut world.chunk_states[chunk_idx];
            state.spawned_entities = spawned;
            state.spawned_physics_bodies = spawned_bodies;
            state.loaded = true;
            world.loaded_chunk_count += 1;
        }
    }

    /// ДОБАВЛЕНО (Задача #14: загрузчик .altex -> GPU Mesh). Возвращает
    /// список mesh_index (по одному на каждый `altex_format::Mesh` внутри
    /// файла) для объекта чанка, ссылающегося на geometry-файл по пути
    /// `altex_path`.
    ///
    /// Поведение:
    /// - `altex_path == "placeholder"` (используется текущим демо-генератором
    ///   мира, см. `AlworldFile::create_and_save_demo_world`) — сразу
    ///   fallback на единичный куб, реального файла не существует и не
    ///   должно быть попытки его открыть.
    /// - Путь уже встречался и успешно распарсен раньше — отдаём
    ///   закэшированный `Vec<usize>` из `self.altex_mesh_cache` без
    ///   повторного чтения диска/пересоздания GPU-ресурсов (один и тот же
    ///   файл обычно используется тысячами объектов открытого мира —
    ///   фонарные столбы, деревья и т.д.).
    /// - Файл не найден / повреждён / не парсится — WARNING в лог и
    ///   fallback на placeholder-куб (та же логика отказоустойчивости, что
    ///   раньше была всегда включена, см. комментарий у `load_chunk`) —
    ///   один плохой ассет не должен останавливать стриминг всего мира.
    /// - Успешный парсинг — конвертируем каждый `altex_format::Vertex` в
    ///   `engine::Vertex` (см. `altex_vertex_to_engine_vertex`), строим по
    ///   одному `engine::Mesh` на каждый `altex_format::Mesh` через
    ///   `Mesh::from_vertices_and_indices` (индексы уже глобальные для
    ///   всего файла в `altex_format::Vertex`/`indices`, поэтому берём
    ///   срез vertices per-mesh и переиндексируем index_offset/count
    ///   относительно НАЧАЛА этого среза — см. ниже), регистрируем через
    ///   `self.add_mesh`, кэшируем и возвращаем результат.
    fn load_object_mesh(&mut self, altex_path: &str) -> Vec<usize> {
        if altex_path.is_empty() || altex_path == "placeholder" {
            return vec![self.load_placeholder_mesh()];
        }

        if let Some(cached) = self.altex_mesh_cache.get(altex_path) {
            return cached.clone();
        }

        let altex = match crate::altex_format::AltexFile::load(altex_path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!(
                    "[ENGINE] WARNING: не удалось загрузить .altex '{}': {:?} — используется placeholder-куб",
                    altex_path, e
                );
                let fallback = vec![self.load_placeholder_mesh()];
                self.altex_mesh_cache.insert(altex_path.to_string(), fallback.clone());
                return fallback;
            }
        };

        let mut mesh_indices = Vec::with_capacity(altex.meshes.len());
        for altex_mesh in &altex.meshes {
            let v_start = altex_mesh.vertex_offset as usize;
            let v_end = v_start + altex_mesh.vertex_count as usize;
            let i_start = altex_mesh.index_offset as usize;
            let i_end = i_start + altex_mesh.index_count as usize;

            if v_end > altex.vertices.len() || i_end > altex.indices.len() {
                eprintln!(
                    "[ENGINE] WARNING: .altex '{}' содержит меш с некорректными offset/count (вне границ vertices/indices) — меш пропущен",
                    altex_path
                );
                continue;
            }

            let engine_vertices: Vec<Vertex> = altex.vertices[v_start..v_end]
                .iter()
                .map(Self::altex_vertex_to_engine_vertex)
                .collect();

            // Индексы .altex глобальные (относительно ВСЕГО vertices файла),
            // а `Mesh::from_vertices_and_indices` ожидает индексы
            // относительно НАЧАЛА переданного среза — пересчитываем
            // вычитанием vertex_offset этого меша.
            let engine_indices: Vec<u32> = altex.indices[i_start..i_end]
                .iter()
                .map(|idx| idx - altex_mesh.vertex_offset)
                .collect();

            match Mesh::from_vertices_and_indices(&engine_vertices, &engine_indices) {
                Ok(mut mesh) => {
                    // ДОБАВЛЕНО (Задача #15): если у этого под-меша есть
                    // материал (material_id != sentinel 0xFFFFFFFF, см.
                    // `AltexFile::add_mesh`/`add_material`) и у материала
                    // задана albedo-карта (albedo_map != тот же sentinel,
                    // см. `add_material`), пытаемся загрузить и забиндить
                    // текстуру. Любая неудача (некорректный material_id/
                    // albedo_map вне границ, ошибка создания GPU-текстуры)
                    // — не фатальна для меша: он просто остаётся без
                    // текстуры (albedo_srv_index = None), закрашивается
                    // только вершинным цветом — ТО ЖЕ поведение, что было
                    // у всех мешей ДО этой задачи.
                    if let Some(material) = altex.materials.get(altex_mesh.material_id as usize) {
                        if material.albedo_map != 0xFFFFFFFF {
                            mesh.albedo_srv_index = self.load_altex_map_srv(&altex, altex_path, material.albedo_map, "albedo");
                        }

                        // ДОБАВЛЕНО (Задача #15, normal mapping): normal
                        // map — тот же паттерн загрузки, что albedo выше
                        // (см. `load_altex_map_srv`), в отдельный
                        // `mesh.normal_srv_index`. Отсутствие карты
                        // (sentinel 0xFFFFFFFF, см. `AltexFile::add_material`)
                        // — не ошибка, а нормальный случай "материал без
                        // normal map", остаётся `None` (см. fallback на
                        // плоскую normal map в render_frame).
                        if material.normal_map != 0xFFFFFFFF {
                            mesh.normal_srv_index = self.load_altex_map_srv(&altex, altex_path, material.normal_map, "normal");
                        }

                        // ДОБАВЛЕНО (Задача #15, normal mapping):
                        // metallic-roughness. `.altex Material` хранит
                        // metallic_map/roughness_map РАЗДЕЛЬНЫМИ индексами
                        // (см. Material в altex_format.rs) — в этой версии
                        // движок поддерживает объединённую ORM-подобную
                        // карту (R=metallic, G=roughness) ТОЛЬКО если ОБЕ
                        // ссылаются на ОДНУ И ТУ ЖЕ текстуру (типичный
                        // экспорт из большинства DCC-инструментов — они
                        // пишут metallic и roughness в разные каналы ОДНОЙ
                        // текстуры, а не в два отдельных файла). Если
                        // индексы различаются (раздельные текстуры) —
                        // сознательно не поддерживается в этой версии
                        // (потребовало бы CPU-стороннего слияния двух
                        // изображений в одно при загрузке — оставлено как
                        // следующий шаг, см. предупреждение ниже), меш в
                        // этом случае использует скалярные
                        // material_metallic/material_roughness напрямую.
                        if material.metallic_map != 0xFFFFFFFF && material.metallic_map == material.roughness_map {
                            mesh.mr_srv_index = self.load_altex_map_srv(&altex, altex_path, material.metallic_map, "metallic-roughness");
                        } else if material.metallic_map != 0xFFFFFFFF || material.roughness_map != 0xFFFFFFFF {
                            eprintln!(
                                "[ENGINE] WARNING: .altex '{}' содержит РАЗДЕЛЬНЫЕ metallic_map/roughness_map (индексы {} и {}) — объединение раздельных текстур в одну ORM-карту пока не реализовано, используются скалярные metallic={}/roughness={} материала",
                                altex_path, material.metallic_map, material.roughness_map, material.metallic, material.roughness
                            );
                        }

                        // Скаляры материала — используются шейдером ВСЕГДА,
                        // когда у меша нет `mr_srv_index` (см. root
                        // constants в render_frame), независимо от того,
                        // есть ли albedo/normal-карты у этого же материала.
                        mesh.material_metallic = material.metallic;
                        mesh.material_roughness = material.roughness;
                    }

                    let index = self.add_mesh(mesh);
                    mesh_indices.push(index);
                }
                Err(e) => {
                    eprintln!(
                        "[ENGINE] WARNING: не удалось создать GPU Mesh из .altex '{}': {:?} — меш пропущен",
                        altex_path, e
                    );
                }
            }
        }

        if mesh_indices.is_empty() {
            // Файл прочитан, но ни один меш не удалось создать (пустой файл
            // или все меши битые) — fallback, чтобы объект хотя бы был
            // виден как placeholder, а не пропадал из мира молча.
            mesh_indices.push(self.load_placeholder_mesh());
        }

        self.altex_mesh_cache.insert(altex_path.to_string(), mesh_indices.clone());
        mesh_indices
    }

    /// Конвертирует вершину формата `.altex` (position/normal/tangent/
    /// bitangent/uv/uv2/color — 7 полей) в вершину движка (position/normal/
    /// color/uv/tangent). ОБНОВЛЕНО (Задача #15, normal mapping): `tangent`
    /// теперь тоже переносится (был отброшен на предыдущем шаге этой же
    /// задачи, см. историю в git/предыдущих правках) — конвертируется из
    /// `.altex`-представления (tangent xyz + отдельный bitangent xyz) в
    /// компактный движковый формат (tangent xyz + w=handedness), см.
    /// `compute_tangent_handedness`. `uv2` (вторая UV-развёртка, обычно под
    /// lightmap/AO-запечёнку) по-прежнему отбрасывается — движок пока не
    /// поддерживает lightmap-запекание, это отдельное, не относящееся к
    /// normal mapping расширение.
    fn altex_vertex_to_engine_vertex(v: &crate::altex_format::Vertex) -> Vertex {
        let handedness = Self::compute_tangent_handedness(v.normal, v.tangent, v.bitangent);
        Vertex {
            position: [v.position[0], v.position[1], v.position[2], 1.0],
            normal: v.normal,
            color: v.color,
            uv: v.uv,
            tangent: [v.tangent[0], v.tangent[1], v.tangent[2], handedness],
        }
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping): вычисляет знак ("рукость")
    /// касательного базиса — ±1.0 — из явного normal/tangent/bitangent
    /// `.altex`-файла: `sign(dot(cross(normal, tangent), bitangent))`.
    /// Нужен, потому что движок хранит bitangent НЕ явным полем, а
    /// восстанавливает его в шейдере как `cross(normal, tangent.xyz) *
    /// tangent.w` (см. `Vertex::tangent` в engine/mod.rs) — этот знак
    /// компенсирует случаи, когда UV-развёртка отражена (мировая ось UV
    /// отзеркалена относительно объекта, частый случай для симметричной
    /// геометрии типа персонажей) и честный запечённый bitangent НЕ
    /// совпадает по направлению с `cross(normal, tangent)` "как есть".
    fn compute_tangent_handedness(normal: [f32; 3], tangent: [f32; 3], bitangent: [f32; 3]) -> f32 {
        let cross = [
            normal[1] * tangent[2] - normal[2] * tangent[1],
            normal[2] * tangent[0] - normal[0] * tangent[2],
            normal[0] * tangent[1] - normal[1] * tangent[0],
        ];
        let dot = cross[0] * bitangent[0] + cross[1] * bitangent[1] + cross[2] * bitangent[2];
        if dot < 0.0 { -1.0 } else { 1.0 }
    }

    /// Возвращает mesh_index единичного placeholder-куба, используемого,
    /// когда реальный `.altex` объекта недоступен (см. `load_object_mesh`).
    /// Переиспользует ОДИН созданный меш-куб для ВСЕХ таких случаев
    /// (кэшируется в `self.world_chunk_placeholder_mesh`) вместо создания
    /// нового меша на каждый объект — тысячи объектов открытого мира не
    /// должны означать тысячи идентичных GPU-мешей одного и того же куба.
    fn load_placeholder_mesh(&mut self) -> usize {
        if let Some(index) = self.world_chunk_placeholder_mesh {
            return index;
        }
        let index = self.add_cube(1.0);
        self.world_chunk_placeholder_mesh = Some(index);
        index
    }

    /// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Гарантирует, что
    /// `shadow_srv_heap` вмещает как минимум `NUM_CASCADES + needed`
    /// SRV-слотов — растёт степенями двойки (в material-части), как и
    /// `light_buffer_capacity`. Material-текстуры ОБЯЗАНЫ жить в ТОМ ЖЕ
    /// хипе, что и shadow-каскады (см. подробное объяснение у полей
    /// `material_srv_capacity`/`material_textures` — аппаратное
    /// ограничение D3D12: не более одного shader-visible CBV_SRV_UAV хипа
    /// забинжено одновременно). Дескрипторный хип НЕЛЬЗЯ просто
    /// пересоздать "пустым": каждый раз, когда хип пересоздаётся (новый
    /// COM-объект), ВСЕ ранее записанные в него SRV нужно записать ЗАНОВО
    /// в новый хип — старые GPU-адреса автоматически становятся
    /// недействительными вместе со старым хипом (тот же класс проблемы,
    /// что уже был решён для `renderer.srv_uav_heap` при resize окна, см.
    /// подробный комментарий в `on_resize`). Поэтому здесь ЗАНОВО
    /// создаются SRV И для NUM_CASCADES shadow-каскадов (из
    /// `self.shadow_maps`), И для ВСЕХ уже загруженных
    /// `self.material_textures` — не только для новых.
    ///
    /// ВАЖНО: вызывается ТОЛЬКО когда `shadow_srv_heap` уже существует
    /// (создаётся в `create_shadow_resources`, вызывается раньше первой
    /// загрузки любой текстуры в нормальном порядке инициализации
    /// движка) — если его почему-то ещё нет, рост material-части
    /// откладывается безопасно (see match ниже), а не паникует.
    fn ensure_material_srv_capacity(&mut self, needed: u32) -> Result<()> {
        if self.shadow_srv_heap.is_some() && needed <= self.material_srv_capacity {
            return Ok(());
        }

        let Some(_) = &self.shadow_srv_heap else {
            eprintln!("[ENGINE] WARNING: ensure_material_srv_capacity вызван до create_shadow_resources — текстура не зарегистрирована");
            return Err(Error::from_hresult(HRESULT(1)));
        };

        let new_material_capacity = needed.max(16).next_power_of_two();
        let total_slots = NUM_CASCADES as u32 + new_material_capacity;
        let heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(total_slots)?;

        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        // Шаг 1: shadow-каскады — ПЕРВЫЕ NUM_CASCADES слотов, тот же
        // порядок, что и в `create_shadow_resources`.
        for cascade in 0..NUM_CASCADES {
            if let Some(shadow_map) = &self.shadow_maps[cascade] {
                let cpu_handle = crate::heap::DescriptorHeap::get_cpu_handle(&heap, cascade as u32, cbv_srv_uav_size);
                if let Err(e) = shadow_map.create_shadow_srv(cpu_handle) {
                    eprintln!(
                        "[ENGINE] WARNING: не удалось повторно создать SRV shadow-каскада {} при росте shadow_srv_heap: {:?}",
                        cascade, e
                    );
                }
            }
        }

        // Шаг 2: material-текстуры — начиная со слота NUM_CASCADES.
        for (index, texture) in self.material_textures.iter().enumerate() {
            let slot = NUM_CASCADES as u32 + index as u32;
            let cpu_handle = crate::heap::DescriptorHeap::get_cpu_handle(&heap, slot, cbv_srv_uav_size);
            if let Err(e) = texture.create_srv(cpu_handle) {
                eprintln!(
                    "[ENGINE] WARNING: не удалось повторно создать SRV текстуры {} при росте shadow_srv_heap: {:?}",
                    index, e
                );
            }
        }

        // GPU-адрес НАЧАЛА shadow-таблицы (индекс 0) не меняется по
        // смыслу — он всегда указывает на каскад 0, ровно как и раньше в
        // create_shadow_resources.
        let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&heap, 0, cbv_srv_uav_size);

        println!(
            "[ENGINE] shadow_srv_heap перевыделен под материалы: {} слотов всего ({} каскадов + {} материалов, {} текстур перерегистрировано)",
            total_slots, NUM_CASCADES, new_material_capacity, self.material_textures.len()
        );

        self.shadow_srv_heap = Some(heap);
        self.shadow_srv_gpu = srv_gpu;
        self.material_srv_capacity = new_material_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Задача #15). Регистрирует новую GPU-текстуру (RGBA8,
    /// `pixels.len()` обязан быть РОВНО `width*height*4` — см. проверку в
    /// `Texture::create_texture2d`) в `shadow_srv_heap` (material-часть,
    /// см. `ensure_material_srv_capacity`) и возвращает её SRV-индекс
    /// (УЖЕ со смещением на NUM_CASCADES — то есть готовый
    /// `OffsetInDescriptorsFromTableStart` для root-параметра 5, register
    /// t6, см. render_frame). НЕ проверяет кэш сама — вызывающий код
    /// (`load_or_get_texture_srv`) отвечает за дедупликацию по ключу
    /// кэша, эта функция всегда создаёт РОВНО одну новую GPU-текстуру.
    fn register_material_texture(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<u32> {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

        let texture = crate::texture::Texture::create_texture2d(width, height, DXGI_FORMAT_R8G8B8A8_UNORM, Some(pixels))?;

        let local_index = self.material_texture_count;
        self.ensure_material_srv_capacity(local_index + 1)?;

        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let slot = NUM_CASCADES as u32 + local_index;
        let heap = self.shadow_srv_heap.as_ref().unwrap();
        let cpu_handle = crate::heap::DescriptorHeap::get_cpu_handle(heap, slot, cbv_srv_uav_size);
        texture.create_srv(cpu_handle)?;

        self.material_textures.push(texture);
        self.material_texture_count += 1;
        Ok(slot)
    }

    /// ДОБАВЛЕНО (Задача #15). Нейтральная белая 1x1 RGBA8(255,255,255,255)
    /// текстура — fallback SRV для мешей без собственной albedo-текстуры
    /// (см. `Mesh::albedo_srv_index` и биндинг в render_frame). Создаётся
    /// ЛЕНИВО (только при первом реальном обращении — не в `Self::new()`),
    /// т.к. большинство существующих сцен (main1.rs/main2.rs, демо-мир до
    /// появления настоящих .altex с материалами) вообще не используют
    /// текстуры и не должны платить даже за одну лишнюю GPU-текстуру.
    fn ensure_white_texture(&mut self) -> Result<u32> {
        if let Some(index) = self.white_texture_srv_index {
            return Ok(index);
        }
        let index = self.register_material_texture(1, 1, &[255, 255, 255, 255])?;
        self.white_texture_srv_index = Some(index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping). См. поле
    /// `flat_normal_srv_index` — та же ленивая lazy-init схема, что и
    /// `ensure_white_texture`.
    fn ensure_flat_normal_texture(&mut self) -> Result<u32> {
        if let Some(index) = self.flat_normal_srv_index {
            return Ok(index);
        }
        let index = self.register_material_texture(1, 1, &[128, 128, 255, 255])?;
        self.flat_normal_srv_index = Some(index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping). См. поле
    /// `neutral_mr_srv_index` — та же ленивая lazy-init схема. Значения
    /// пикселей (0,128,0,255) сами по себе не используются шейдером в
    /// fallback-случае (root constants приоритетнее — см. HLSL PS main()),
    /// выбраны просто как безобидные валидные байты на случай будущего
    /// расширения, где эта текстура станет реально читаемой.
    fn ensure_neutral_mr_texture(&mut self) -> Result<u32> {
        if let Some(index) = self.neutral_mr_srv_index {
            return Ok(index);
        }
        let index = self.register_material_texture(1, 1, &[0, 128, 0, 255])?;
        self.neutral_mr_srv_index = Some(index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15). Загружает (или берёт из кэша) SRV-индекс
    /// albedo-текстуры конкретного материала конкретного .altex-файла.
    /// Ключ кэша — `"{altex_path}#{albedo_map_index}"`, а НЕ просто
    /// `altex_path`: один .altex файл может содержать НЕСКОЛЬКО разных
    /// текстур (см. `AltexFile::textures`), поэтому индекс текстуры
    /// внутри файла обязателен, иначе разные текстуры одного файла
    /// схлопнулись бы в один и тот же кэшированный SRV.
    fn load_or_get_texture_srv(&mut self, altex_path: &str, albedo_map_index: u32, width: u32, height: u32, pixels: &[u8]) -> Result<u32> {
        let cache_key = format!("{}#{}", altex_path, albedo_map_index);
        if let Some(&index) = self.texture_cache.get(&cache_key) {
            return Ok(index);
        }
        let index = self.register_material_texture(width, height, pixels)?;
        self.texture_cache.insert(cache_key, index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping): общий хелпер поверх
    /// `load_or_get_texture_srv`, вынесенный из `load_object_mesh` — та же
    /// логика (границы `texture_data`, кэш по `"{altex_path}#{map_index}"`,
    /// нефатальная ошибка → `eprintln!` + `None`) нужна ТРИЖДЫ на меш
    /// (albedo/normal/metallic-roughness), раньше существовала только
    /// заинлайненной под albedo. `altex` — отдельный параметр (не поле
    /// `self`), поэтому одновременное заимствование `&AltexFile` и
    /// `&mut self` (для `self.load_or_get_texture_srv` внутри) не
    /// конфликтует с borrow checker'ом.
    fn load_altex_map_srv(&mut self, altex: &crate::altex_format::AltexFile, altex_path: &str, map_index: u32, map_kind: &str) -> Option<u32> {
        let texture = altex.textures.get(map_index as usize)?;
        let tex_start = texture.data_offset as usize;
        let tex_end = tex_start + texture.data_size as usize;
        if tex_end > altex.texture_data.len() {
            eprintln!(
                "[ENGINE] WARNING: .altex '{}' содержит {}-текстуру с некорректными data_offset/data_size (вне границ texture_data) — меш без этой карты",
                altex_path, map_kind
            );
            return None;
        }
        let pixels = &altex.texture_data[tex_start..tex_end];
        match self.load_or_get_texture_srv(altex_path, map_index, texture.width, texture.height, pixels) {
            Ok(srv_index) => Some(srv_index),
            Err(e) => {
                eprintln!(
                    "[ENGINE] WARNING: не удалось создать SRV {}-текстуры для .altex '{}': {:?} — меш без этой карты",
                    map_kind, altex_path, e
                );
                None
            }
        }
    }

    /// Выгружает содержимое ОДНОГО чанка — despawn всех его сущностей из
    /// Scene (см. `load_chunk`). Геометрия (placeholder-меш) НЕ удаляется —
    /// она общая и переиспользуется другими чанками/будущими загрузками
    /// того же чанка.
    fn unload_chunk(&mut self, chunk_idx: usize) {
        let (entities, physics_bodies) = if let Some(world) = &mut self.world {
            (
                std::mem::take(&mut world.chunk_states[chunk_idx].spawned_entities),
                std::mem::take(&mut world.chunk_states[chunk_idx].spawned_physics_bodies),
            )
        } else {
            return;
        };
        for entity in entities {
            self.scene.despawn(entity);
        }
        // ДОБАВЛЕНО (объединённая сцена — физика из .alworld): удаляем из
        // плагина Inertial физические тела, созданные для объектов этого
        // чанка (см. `load_chunk`), и вычищаем соответствующие записи из
        // `physics_links` — иначе (а) тело продолжало бы участвовать в
        // broad/narrow phase и тратить CPU-время ради объекта, которого
        // визуально уже нет, и (б) `sync_physics_transforms` на следующем
        // кадре попытался бы читать `get_body(id)` для уже despawn'нутой
        // ECS-сущности, а если плагин переиспользует освободившиеся id
        // под НОВЫЕ тела (см. README плагина `alkash3d-inertial` про
        // "unstable body IDs after removal", уже однажды чинившийся баг
        // самого плагина) — писал бы позицию чужого, не связанного с этим
        // чанком тела в Transform давно удалённой сущности, если бы запись
        // из `physics_links` осталась висеть.
        if !physics_bodies.is_empty() {
            if let Some(physics) = self.physics.as_mut() {
                for body_id in &physics_bodies {
                    physics.remove_body(*body_id);
                }
            }
            self.physics_links.retain(|(id, _)| !physics_bodies.contains(id));
        }
        if let Some(world) = &mut self.world {
            world.chunk_states[chunk_idx].loaded = false;
            world.loaded_chunk_count = world.loaded_chunk_count.saturating_sub(1);
        }
    }

    /// Вызывается каждый кадр из `update()` (см. ниже) — сравнивает
    /// текущую позицию камеры с позицией на момент последнего пересчёта
    /// стриминга (`last_streaming_origin`) и, если камера сдвинулась
    /// заметно ИЛИ прошло достаточно кадров (`WORLD_STREAMING_INTERVAL_FRAMES`
    /// — защита от "стоим на месте, но пересчитываем каждый кадр
    /// впустую"), обходит ВСЕ чанки мира и ОПРЕДЕЛЯЕТ, какие нужно
    /// загрузить (ближе `load_distance`) или выгрузить (дальше
    /// `unload_distance`, намеренно РАЗНЫЙ порог — гистерезис, см.
    /// подробное объяснение ниже, почему не один общий порог с загрузкой).
    ///
    /// ИСПРАВЛЕНО (баг: "фризы как будто GC" — см. подробности у поля
    /// `pending_load` в `WorldStreamingState`): эта функция теперь ТОЛЬКО
    /// складывает решения в очередь `pending_load`/`pending_unload` —
    /// реальный синхронный дисковый I/O (`load_chunk`/`unload_chunk`)
    /// вынесен в `drain_pending_chunk_io`, которая тратит на него
    /// ограниченный бюджет КАЖДЫЙ кадр (не только когда истёк интервал
    /// пересчёта), размазывая потенциально большую пачку чанков по многим
    /// кадрам вместо одного длинного застоя.
    fn update_world_streaming(&mut self, camera_pos: Vec3) {
        let world = match &self.world {
            Some(w) => w,
            None => return, // Мир не загружен — стриминг просто не участвует в кадре.
        };

        // Гистерезис (два разных порога вместо одного): если бы порог
        // загрузки/выгрузки совпадал, объект РОВНО на границе дистанции
        // при мельчайших колебаниях позиции камеры (или просто числовой
        // погрешности float) мог бы загружаться и выгружаться КАЖДЫЙ
        // пересчёт подряд — постоянные spawn/despawn одного и того же
        // чанка ("flickering"-стриминг), дорого и визуально заметно
        // (объекты то появляются, то исчезают). unload_distance ЗАВЕДОМО
        // больше load_distance в `AlworldFile::new()` (200.0/250.0) —
        // это и есть зона "нейтральной полосы", где чанк остаётся в том
        // состоянии, в котором уже был.
        let moved_far_enough = (camera_pos - world.last_streaming_origin).length_squared() > 1.0;
        let interval_elapsed = world.frames_since_streaming_update >= WORLD_STREAMING_INTERVAL_FRAMES;
        if !moved_far_enough && !interval_elapsed {
            if let Some(world) = &mut self.world {
                world.frames_since_streaming_update += 1;
            }
            return;
        }

        let load_distance_sq = world.world_file.streaming_config.load_distance * world.world_file.streaming_config.load_distance;
        let unload_distance_sq = world.world_file.streaming_config.unload_distance * world.world_file.streaming_config.unload_distance;

        // Собираем решения ОТДЕЛЬНО от их применения — `load_chunk`/
        // `unload_chunk` берут `&mut self` целиком (нужен доступ и к
        // `self.scene`, и к `self.world`, и к `self.meshes` через
        // `load_object_mesh`/`add_cube`), поэтому нельзя вызывать их
        // прямо во время итерации по `&self.world.world_file.chunks` —
        // классический паттерн "собрать индексы -> отпустить заём ->
        // применить", уже использованный в этом файле для shadow_jobs и
        // подобного.
        let mut newly_queued_load = 0;
        let mut newly_queued_unload = 0;
        {
            let world = self.world.as_mut().unwrap();
            for i in 0..world.world_file.chunks.len() {
                let chunk = &world.world_file.chunks[i];
                let center = world.world_file.chunk_center_world(chunk);
                let center = Vec3::new(center[0], center[1], center[2]);
                let dist_sq = (center - camera_pos).length_squared();
                let state = &world.chunk_states[i];

                // `queued` — уже в очереди с прошлого пересчёта, не
                // добавляем повторно (см. комментарий у поля `queued`).
                if !state.loaded && !state.queued && dist_sq <= load_distance_sq {
                    world.pending_load.push(i);
                    world.chunk_states[i].queued = true;
                    newly_queued_load += 1;
                } else if state.loaded && !state.queued && dist_sq > unload_distance_sq {
                    world.pending_unload.push(i);
                    world.chunk_states[i].queued = true;
                    newly_queued_unload += 1;
                }
            }
        }

        if let Some(world) = &mut self.world {
            world.last_streaming_origin = camera_pos;
            world.frames_since_streaming_update = 0;
        }

        if newly_queued_load > 0 || newly_queued_unload > 0 {
            println!(
                "[ENGINE] World streaming: +{} чанков поставлено в очередь загрузки, +{} в очередь выгрузки",
                newly_queued_load, newly_queued_unload
            );
        }
    }

    /// ДОБАВЛЕНО (фризы стриминга, см. `pending_load` у `WorldStreamingState`):
    /// вызывается КАЖДЫЙ кадр из `update()` (в отличие от
    /// `update_world_streaming`, которая пересчитывает окрестность лишь
    /// изредка) — обрабатывает не более `CHUNK_LOAD_BUDGET_PER_FRAME`
    /// чанков из накопленных очередей `pending_load`/`pending_unload`.
    /// Выгрузка обрабатывается тем же бюджетом (хоть и дешевле загрузки —
    /// без файлового I/O, только despawn), чтобы massed unload (например
    /// после телепорта камеры далеко в сторону) не давал свой всплеск на
    /// одном кадре.
    fn drain_pending_chunk_io(&mut self) {
        let mut budget = CHUNK_LOAD_BUDGET_PER_FRAME;

        while budget > 0 {
            let next = match &mut self.world {
                Some(world) => world.pending_load.pop(),
                None => None,
            };
            let Some(chunk_idx) = next else { break };
            self.load_chunk(chunk_idx);
            if let Some(world) = &mut self.world {
                world.chunk_states[chunk_idx].queued = false;
            }
            budget -= 1;
        }

        while budget > 0 {
            let next = match &mut self.world {
                Some(world) => world.pending_unload.pop(),
                None => None,
            };
            let Some(chunk_idx) = next else { break };
            self.unload_chunk(chunk_idx);
            if let Some(world) = &mut self.world {
                world.chunk_states[chunk_idx].queued = false;
            }
            budget -= 1;
        }
    }

    pub fn add_street_light(&mut self, x: f32, y: f32, z: f32) -> Option<u32> {
        // ИЗМЕНЕНО (по просьбе): дальность действия — с 25.0 до 100.0, тот
        // же range, что и у уличных фонарей create_night_city() в
        // alfar_format.rs, чтобы оба способа добавления уличных фонарей
        // (через .alfar и напрямую через этот метод, см. main1.rs) вели
        // себя одинаково.
        let light = GPULight {
            position: [x, y, z, 0.0],
            color: [1.0, 0.85, 0.6, 2.5],
            direction: [0.0, -1.0, 0.0, 100.0],
            params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
        };
        self.lights.as_mut().map(|l| l.add_light(&light))
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        self.lights.as_ref().map(|l| l.get_gpu_lights()).unwrap_or(&[])
    }

    pub fn get_contacts(&self) -> &[PhysicsContact] {
        self.physics.as_ref().map(|p| p.get_contacts()).unwrap_or(&[])
    }

    pub fn update(&mut self, dt: f32, gravity: f32, camera_pos: [f32; 3], view_proj: [f32; 16]) {
        self.scheduler.reset_budget();

        // ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит
        // на месте" ПОСЛЕ фиксов стриминга/hot-reload/culling/физики):
        // `[TIMING] worst update()=...` в bin/main.rs показывал общее
        // время update() целиком, но не показывал, какая именно под-фаза
        // внутри него периодически спайкает — а лог с [PHYS-STATS]
        // показал, что solver физики при этом остаётся спокойным (то
        // есть спайк НЕ в физике). Разбиваем update() на измеряемые
        // под-фазы, копим худший случай каждой в self.update_breakdown_ms
        // (сбрасывается в bin/main.rs раз в секунду вместе с остальными
        // счётчиками) — следующий лог покажет ТОЧНО виновную под-фазу
        // вместо очередной догадки.
        macro_rules! timed {
            ($field:ident, $body:expr) => {{
                let __start = std::time::Instant::now();
                let __result = $body;
                let __ms = __start.elapsed().as_secs_f32() * 1000.0;
                if __ms > self.update_breakdown_ms.$field {
                    self.update_breakdown_ms.$field = __ms;
                }
                __result
            }};
        }

        timed!(physics_ms, {
            if let Some(physics) = &mut self.physics {
                physics.update(dt, gravity);
            }
        });
        // ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): см. подробный
        // комментарий у самого метода — проецирует результат этого шага
        // физики на видимую геометрию, ДО render_frame() этого же кадра.
        timed!(sync_physics_ms, self.sync_physics_transforms());

        // ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины): ПОСЛЕ
        // sync_physics_transforms (чтобы скрипт видел уже свежую позицию
        // из физики в этом же кадре, а не устаревшую с прошлого) и ДО
        // update_day_night/cull — движение, которое задаёт скрипт, должно
        // успеть попасть в тот же кадр рендера, а не только в следующий.
        // Если скрипт сам двигает физическое тело — конфликт с
        // sync_physics_transforms исключён: тот применяется РАНЬШЕ и
        // только один раз за кадр, update_native_scripts может лишь
        // перезаписать Transform уже ПОСЛЕ него — скрипт имеет право
        // переопределить результат физики в тот же кадр.
        timed!(native_scripts_ms, self.update_native_scripts(dt));

        // ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload): та
        // же позиция в кадре, что и update_native_scripts выше (сразу
        // после неё) — оба вида скриптов (Native/Lua через
        // update_native_scripts, Python через update_python_scripts)
        // равноправны и оба успевают попасть в этот же кадр рендера;
        // порядок между ними самими не важен, т.к. они работают с РАЗНЫМИ
        // прикреплениями (Native/Lua используют active_scripts, Python —
        // python_scripts) и друг друга не перезаписывают.
        timed!(python_scripts_ms, self.update_python_scripts(dt));

        // ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
        // мерцание): ДО cull() — так свежий свет (обновлённый intensity от
        // мерцания/day-night) успевает попасть в FirstFires прежде, чем
        // тот в этом же кадре посчитает culling/LOD по текущему списку
        // источников.
        timed!(day_night_ms, self.update_day_night(dt));

        // ДОБАВЛЕНО (World Streaming): загрузка/выгрузка чанков мира по
        // дистанции от камеры — см. `update_world_streaming` выше. Место
        // вызова (после update_day_night, до lights.cull) не критично для
        // корректности (стриминг и день/ночь независимы), но ДО cull()
        // важно: если этот кадр что-то загрузил (заспавнил новые сущности
        // с MeshRenderer), они должны успеть попасть в тот же кадр
        // рендера, а не только в следующий — cull() ниже работает со
        // светом, не с mesh_instances/Scene, так что порядок здесь не
        // влияет на видимость новых объектов, но сохраняет естественный
        // порядок "сначала обновили мир, потом посчитали, что из него
        // видно".
        timed!(world_streaming_ms, self.update_world_streaming(Vec3::new(camera_pos[0], camera_pos[1], camera_pos[2])));
        // ИСПРАВЛЕНО (фризы стриминга): фактический I/O — каждый кадр, с
        // ограниченным бюджетом, а не одним махом внутри
        // update_world_streaming (см. drain_pending_chunk_io).
        timed!(chunk_io_ms, self.drain_pending_chunk_io());

        timed!(light_cull_ms, {
            if let Some(lights) = &mut self.lights {
                lights.cull(camera_pos, &view_proj, dt);
            }
        });

        // ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): слушатель
        // синхронизируется с текущей позицией/направлением камеры КАЖДЫЙ
        // кадр — без этого 3D-звуки навсегда считали бы дистанцию/панораму
        // от позиции слушателя на момент последнего явного `set_listener`
        // (которого извне может вообще не быть, если приложение не знает
        // об этом требовании) вместо реальной текущей позиции игрока.
        // `forward` берём из camera_pos/view_proj недоступно напрямую
        // здесь (у `update()` нет прямого доступа к `self.camera` как
        // параметру — но `self.camera` то же самое поле, что используется
        // для рендера, читаем его напрямую).
        timed!(audio_ms, {
            if let Some(audio) = &mut self.audio {
                let forward = self.camera.target - self.camera.position;
                audio.set_listener(crate::audio::Listener {
                    position: Vec3::new(camera_pos[0], camera_pos[1], camera_pos[2]),
                    forward: if forward.length_squared() > 1e-6 { forward.normalize() } else { Vec3::new(0.0, 0.0, 1.0) },
                    up: self.camera.up,
                    velocity: Vec3::ZERO,
                });
                audio.update(dt);
            }
        });
    }

    /// ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
    /// мерцание): продвигает время суток, пересчитывает "солнце"
    /// (directional-свет — напрямую в transform_constants, см.
    /// `compute_sun_state`) и обновляет каждый управляемый точечный/spot
    /// источник (`self.managed_lights`) — мерцание (шум по
    /// flicker_speed/flicker_intensity) и включение/выключение по
    /// active_from/active_to (см. `ManagedLight::is_active_at`).
    ///
    /// Вызывается из `update()` каждый кадр — не зависит от того, был ли
    /// вообще загружен .alfar: если `managed_lights` пуст (сцена без
    /// .alfar или без point/spot света), цикл по нему просто не делает
    /// ничего, а солнце всё равно пересчитывается (у него разумные
    /// дефолты даже без .alfar — см. `compute_sun_state`).
    fn update_day_night(&mut self, dt: f32) {
        // Продвигаем время суток. Оборачиваем в [0,24) через rem_euclid —
        // обычный `%` в Rust для f32 может вернуть отрицательный остаток
        // при отрицательной day_night_speed (например, если приложение
        // захочет отмотать время назад), а часы суток должны оставаться
        // неотрицательными.
        self.time_of_day = (self.time_of_day + self.day_night_speed * dt).rem_euclid(24.0);

        let sun = Self::compute_sun_state(self.time_of_day);
        self.transform_constants.light_dir = [sun.direction.x, sun.direction.y, sun.direction.z, 0.0];
        self.transform_constants.light_color = [sun.color[0], sun.color[1], sun.color[2], sun.intensity];
        self.transform_constants.ambient_color = [sun.ambient[0], sun.ambient[1], sun.ambient[2], 1.0];

        if self.managed_lights.is_empty() {
            return;
        }

        // Собираем обновления заранее (без заимствования self.lights) —
        // borrow checker: нельзя одновременно держать &mut self.lights
        // (внутри цикла ниже) и читать self.managed_lights/self.flicker_phase
        // по &self, если бы мы одалживали их через один и тот же self.
        // Здесь это не требуется (managed_lights и lights — разные поля,
        // Rust умеет их различать при прямом доступе к полям), но цикл
        // всё равно проще и понятнее с одним проходом.
        let hour = self.time_of_day;
        for (i, managed) in self.managed_lights.iter().enumerate() {
            let active = managed.is_active_at(hour);

            let mut intensity = if active { managed.base_intensity } else { 0.0 };

            if active && managed.flicker_enabled {
                // Простой, но не периодически-заметный шум: сумма двух
                // синусоид с разными (взаимно иррациональными по
                // отношению друг к другу) частотами. Чистая одна синусоида
                // мерцала бы слишком регулярно/предсказуемо ("дышащая"
                // лампа, а не реалистичное потрескивание); две с разным
                // периодом визуально гораздо ближе к настоящему мерцанию
                // лампы, оставаясь при этом дешёвым и полностью
                // детерминированным (без ГПСЧ и его состояния).
                let phase = self.flicker_phase[i];
                let noise = 0.6 * (phase).sin() + 0.4 * (phase * 2.7).sin();
                // noise примерно в [-1,1] (сумма амплитуд 0.6+0.4=1.0) —
                // модулируем intensity вокруг базового значения на
                // ±flicker_intensity долю.
                intensity *= (1.0 + noise * managed.flicker_intensity).max(0.0);
            }

            let gpu_light = GPULight {
                position: [managed.position[0], managed.position[1], managed.position[2], managed.light_type],
                color: [managed.color[0], managed.color[1], managed.color[2], intensity],
                direction: [managed.direction[0], managed.direction[1], managed.direction[2], managed.range],
                params: managed.params,
            };

            if let Some(lights) = &mut self.lights {
                lights.update_light(managed.firstfires_id, &gpu_light);
            }
        }

        // Фазу мерцания продвигаем ПОСЛЕ того, как её прочитали выше —
        // отдельный проход, а не в том же цикле, чтобы не занимать
        // &mut self.flicker_phase одновременно с &self.managed_lights
        // (оба — поля одного self, но заимствование ЦЕЛОГО self на чтение
        // выше через `self.managed_lights.iter()` уже держит &self
        // активным на всю длительность цикла).
        for (i, managed) in self.managed_lights.iter().enumerate() {
            if managed.flicker_enabled {
                self.flicker_phase[i] += dt * managed.flicker_speed;
            }
        }
    }

    /// ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
    /// мерцание): чистая функция часы-суток -> состояние солнца
    /// (направление/цвет/интенсивность/ambient). Не метод `&self` — не
    /// использует ничего из AlkashEngine, что упрощает модульное
    /// тестирование и делает явным, что результат детерминирован ТОЛЬКО
    /// временем суток.
    ///
    /// Модель нарочно простая (не астрономически точная — без широты/
    /// долготы/дня года): солнце восходит в 6:00, садится в 18:00, идёт
    /// по дуге высотой до 90° в зените (полдень) через азимут, зафиксированный
    /// в плоскости X (направление на восток/запад — вращать по азимуту
    /// приложение может отдельно, если нужно, повернув всю сцену).
    /// Ночью (после заката/до рассвета) прямого солнечного света нет
    /// вообще (intensity=0), но остаётся холодный лунный ambient — иначе
    /// сцена ночью была бы полностью чёрной там, куда не достаёт свет
    /// точечных источников.
    fn compute_sun_state(hour: f32) -> SunState {
        const SUNRISE: f32 = 6.0;
        const SUNSET: f32 = 18.0;

        if hour < SUNRISE || hour > SUNSET {
            // Ночь: солнце направлено строго вниз (значение направления в
            // этом случае не влияет на видимый результат, т.к.
            // intensity=0, но остаётся конечным и валидным для
            // compute_cascade_view_proj), только лунный ambient.
            return SunState {
                direction: Vec3::new(0.0, -1.0, 0.0),
                color: [0.6, 0.7, 1.0],
                intensity: 0.0,
                ambient: [0.02, 0.02, 0.05],
            };
        }

        // day_t: 0.0 на восходе, 1.0 на закате.
        let day_t = (hour - SUNRISE) / (SUNSET - SUNRISE);
        // Высота солнца над горизонтом: 0 на восходе/закате, максимум (90°,
        // в зените) в middle_t=0.5 (полдень) — простая синусоида по day_t.
        let elevation = (day_t * std::f32::consts::PI).sin().max(0.0);
        let elevation_angle = elevation * std::f32::consts::FRAC_PI_2;

        // Азимут: солнце идёт с востока (day_t=0) на запад (day_t=1) по
        // дуге в плоскости XY (Y — направление вверх в этом движке, см.
        // camera.rs/math.rs) — движение вдоль X, высота вдоль Y.
        let azimuth = day_t * std::f32::consts::PI; // 0..PI, восток->запад

        // Направление, ОТКУДА исходит свет (позиция солнца на небе).
        let sun_pos_dir = Vec3::new(
            -azimuth.cos(),
            elevation_angle.sin(),
            0.3, // небольшой сдвиг по Z, чтобы тени не были идеально плоскими в этой оси
        ).normalize();
        // TransformConstants.light_dir — это направление, КУДА летит свет
        // (см. shader main(): `-normalize(lightDir)` используется как
        // направление НА источник) — то есть противоположность позиции
        // солнца на небе.
        let direction = -sun_pos_dir;

        // Цвет: у горизонта (восход/закат, elevation~0) — тёплый
        // оранжевый (длинный путь через атмосферу рассеивает синий
        // сильнее); в зените — нейтрально-белый. Линейная интерполяция по
        // elevation между "закатным" и "полуденным" цветом — простая, но
        // визуально убедительная аппроксимация Релеевского рассеяния без
        // реальной симуляции атмосферы.
        let horizon_color = Vec3::new(1.0, 0.45, 0.15);
        let noon_color = Vec3::new(1.0, 0.97, 0.92);
        let color_t = elevation; // 0 у горизонта, 1 в зените
        let color = horizon_color.lerp(noon_color, color_t);

        // Интенсивность: слабая у самого горизонта (восход/закат),
        // максимальная в зените — та же elevation-кривая, что и выше,
        // с минимальным полом 0.15 у самого горизонта (не 0), чтобы
        // переход рассвет/закат не был резким скачком в момент hour==SUNRISE.
        let intensity = 0.15 + 0.85 * elevation;

        // Ambient следует за тем же дневным/сумеречным переходом, но
        // остаётся заметно темнее прямого света и синее (небо, а не
        // солнце, — рассеянный синий свет неба, а не прямой тёплый).
        let ambient = Vec3::new(0.05, 0.06, 0.09).lerp(Vec3::new(0.25, 0.28, 0.32), elevation);

        SunState {
            direction,
            color: [color.x, color.y, color.z],
            intensity,
            ambient: [ambient.x, ambient.y, ambient.z],
        }
    }

    /// Устанавливает время суток напрямую (часы, [0,24) — выходящие за
    /// диапазон значения оборачиваются через `rem_euclid`, как и в
    /// `update_day_night`). Полезно для мгновенных переходов (катсцены,
    /// быстрая перемотка через UI редактора) в отличие от плавного
    /// течения через `day_night_speed`.
    pub fn set_time_of_day(&mut self, hour: f32) {
        self.time_of_day = hour.rem_euclid(24.0);
    }

    /// Устанавливает скорость течения времени суток (игровых часов в
    /// реальную секунду). 0.0 останавливает смену дня/ночи (значение по
    /// умолчанию — см. `AlkashEngine::new`).
    pub fn set_day_night_speed(&mut self, speed: f32) {
        self.day_night_speed = speed;
    }

    pub fn get_time_of_day(&self) -> f32 {
        self.time_of_day
    }

    pub fn shutdown(&mut self) {
        // Защита от двойного вызова
        if self.shutdown_in_progress {
            println!("[ENGINE] Shutdown already in progress");
            return;
        }
        self.shutdown_in_progress = true;

        println!("[ENGINE] Shutting down...");
        self.running = false;

        // ===== 1. Ждем завершения всех GPU операций =====
        {
            let state = STATE.lock().unwrap();
            if let (Some(queue), Some(fence)) = (&state.command_queue, &state.fence) {
                // ИСПРАВЛЕНО: было захардкожено `let fence_value = 100;` —
                // ничем не гарантировано, что это значение больше всех
                // уже отправленных в очередь Signal(). Теперь берём
                // значение из общего монотонного счётчика движка.
                let fence_value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
                unsafe {
                    println!("[ENGINE] Signaling fence (value={})...", fence_value);
                    let _ = queue.Signal(fence, fence_value);
                    println!("[ENGINE] Waiting for GPU to finish...");

                    let start = std::time::Instant::now();
                    while fence.GetCompletedValue() < fence_value {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        if start.elapsed() > std::time::Duration::from_secs(5) {
                            println!("[ENGINE] WARNING: GPU timeout, forcing shutdown");
                            break;
                        }
                    }
                    println!("[ENGINE] GPU idle");
                }
            }
        }

        // ===== 2. Очищаем меши и экземпляры =====
        println!("[ENGINE] Clearing meshes...");
        self.meshes.clear();
        self.mesh_instances.clear();

        // ===== 3. Явно сбрасываем ВСЕ GPU-ресурсы движка, пока device/
        //    command_queue ещё живы =====
        //
        // ИСПРАВЛЕНО (краш видеодрайвера при открытии/закрытии окна после
        // смены разрешения — реальный баг, найденный на живой машине):
        // здесь раньше обнулялись ТОЛЬКО constant_buffer/vs/ps/
        // pipeline_state/root_signature/renderer. Но у `AlkashEngine` есть
        // ДЕСЯТКИ других полей, тоже хранящих реальные COM-объекты D3D12
        // (текстуры, дескрипторные хипы, PSO, root signatures, буферы) —
        // bloom-таргеты и их RTV/SRV-хипы, все NUM_CASCADES shadow map и их
        // DSV/SRV-хипы, весь shadow-PSO/root-signature, material_textures
        // (albedo/normal/metallic-roughness всех загруженных .altex,
        // задача #15), light_buffer/grid_cells_buffer/grid_entries_buffer,
        // весь tonemap-проход (PSO/root signature/шейдеры/CBV). Ни одно из
        // них НЕ обнулялось явно — они оставались жить как поля `self` и
        // дропались бы обычным полевым Drop только ПОСЛЕ выхода из этой
        // функции. Но чуть ниже (шаг 4) `state.device = None` уничтожает
        // ЕДИНСТВЕННУЮ ссылку на ID3D12Device, а `state.command_queue =
        // None` — на очередь команд: с этого момента ни у одного из
        // оставшихся GPU-ресурсов уже нет гарантированно живого устройства/
        // очереди, к которым они логически привязаны, а порядок их
        // финального высвобождения через COM Release() внутри Drop поля за
        // полем НИЧЕМ не гарантирован относительно порядка, в котором
        // реально освобождается сам device — с включённым (см. device.rs)
        // D3D12 debug layer это НАДЁЖНО отлавливается рантаймом как грубое
        // нарушение протокола владения ресурсами, а на некоторых
        // видеодрайверах (что и наблюдалось на реальной машине пользователя,
        // особенно после смены разрешения — другой набор живых ресурсов на
        // момент закрытия) это приводит не к аккуратной ошибке API, а к
        // зависанию GPU и TDR (сброс драйвера, перезагрузка ПК). Явно
        // обнуляем ВСЕ ресурсные поля здесь, СТРОГО после ожидания GPU idle
        // (шаг 1) и СТРОГО до обнуления device/command_queue/swap_chain
        // (шаг 4) — тогда порядок гарантированно правильный: сначала все
        // дочерние ресурсы, потом сама очередь и устройство.
        println!("[ENGINE] Releasing resources...");

        self.constant_buffer = None;
        self.constant_buffer_capacity = 0;
        self.vs = None;
        self.ps = None;
        self.pipeline_state = None;
        self.root_signature = None;
        self.renderer = None;

        // Bloom (Фаза 5): текстуры, их RTV/SRV-хипы, PSO/root signature/
        // шейдеры/CBV прохода.
        self.bloom_texture_a = None;
        self.bloom_texture_b = None;
        self.bloom_rtv_heap = None;
        self.bloom_srv_heap = None;
        self.bloom_extract_ps = None;
        self.bloom_blur_ps = None;
        self.bloom_root_signature = None;
        self.bloom_extract_pipeline_state = None;
        self.bloom_blur_pipeline_state = None;
        self.bloom_params_buffer = None;

        // Tonemap/composite-проход (Фаза 5): отдельная пара PSO/root
        // signature от основного 3D-прохода, свой CBV.
        self.tonemap_vs = None;
        self.tonemap_ps = None;
        self.tonemap_root_signature = None;
        self.tonemap_pipeline_state = None;
        self.tonemap_constant_buffer = None;

        // Cascaded Shadow Maps (Фаза 6): все NUM_CASCADES depth-таргетов,
        // их общий DSV-хип, отдельный SHADER_VISIBLE SRV-хип (тот же, что
        // держит material_textures задачи #15 — см. shadow_srv_heap ниже),
        // PSO/root signature/шейдер прохода, CBV.
        for slot in self.shadow_maps.iter_mut() {
            *slot = None;
        }
        self.shadow_dsv_heap = None;
        self.shadow_srv_heap = None;
        self.shadow_vs = None;
        self.shadow_root_signature = None;
        self.shadow_pipeline_state = None;

        // Освещение FirstFires (Фазы 2-3): GPU-буфер видимых фонарей и
        // буферы пространственной сетки.
        self.light_buffer = None;
        self.light_buffer_capacity = 0;
        self.grid_cells_buffer = None;
        self.grid_cells_buffer_capacity = 0;
        self.grid_entries_buffer = None;
        self.grid_entries_buffer_capacity = 0;

        // PBR-материалы (Задача #15): все загруженные albedo/normal/
        // metallic-roughness текстуры .altex-объектов и кэш их SRV-слотов
        // (сами слоты жили в уже обнулённом выше shadow_srv_heap — кэш
        // индексов без хипа бессмыслен и тоже должен быть сброшен).
        self.material_textures.clear();
        self.texture_cache.clear();

        // ДОБАВЛЕНО: ID3D12InfoQueue — тоже держит ссылку на device (см.
        // `device.cast::<ID3D12InfoQueue>()` в device.rs), хоть и не
        // ресурс рендеринга — обнуляем вместе со всем остальным, до
        // обнуления самого device чуть ниже.
        {
            let mut state = STATE.lock().unwrap();
            state.info_queue = None;
        }

        // ===== 4. Сбрасываем глобальное состояние =====
        println!("[ENGINE] Resetting global state...");
        {
            let mut state = STATE.lock().unwrap();

            state.fence = None;
            state.fence_values.clear();
            state.command_allocators.clear();
            state.command_list = None;

            if let Some(swap_chain) = &state.swap_chain {
                unsafe {
                    let _ = swap_chain.SetFullscreenState(false, None);
                }
            }
            state.swap_chain = None;
            state.command_queue = None;
            state.device = None;
            state.descriptor_heaps.clear();
            state.root_signature = None;
            state.current_pso = None;
            state.bound_vertex_buffers.clear();
            state.bound_index_buffer = None;
            state.scheduler = None;
        }

        // ===== 5. Уничтожаем окно (если еще не уничтожено) =====
        unsafe {
            if let Some(hwnd) = self.hwnd {
                println!("[ENGINE] Destroying window...");
                // Проверяем, существует ли еще окно
                if IsWindow(Some(hwnd)).as_bool() {
                    DestroyWindow(hwnd);
                }
                self.hwnd = None;
            }
        }

        self.shutdown_in_progress = false;
        println!("[ENGINE] Shutdown complete");
    }
}

impl Drop for AlkashEngine {
    fn drop(&mut self) {
        // ИСПРАВЛЕНО (главная причина падения драйвера при закрытии):
        // раньше здесь стояла проверка `if self.running`. Но `self.running`
        // выставляется в `false` уже в обработчике WM_CLOSE/WM_DESTROY
        // (см. wndproc) — то есть ЕЩЁ ДО того, как AlkashEngine реально
        // выходит из scope и срабатывает Drop. Из-за этого при закрытии
        // окна крестиком (main.rs, где shutdown() не вызывается явно, а
        // расчёт идёт именно на этот Drop) shutdown() НИКОГДА не
        // вызывался: Rust просто дропал все поля AlkashEngine в порядке
        // объявления (renderer → meshes → root_signature → ...) без
        // единого ожидания GPU. Если GPU в этот момент ещё не закончил
        // читать/писать в какой-то из этих ресурсов — это гарантированный
        // "ресурс уничтожен, пока GPU его использует", что и приводит к
        // зависанию GPU / TDR (сброс видеодрайвера, лаги на всех
        // мониторах, кратковременное пропадание звука).
        //
        // shutdown() безопасно вызывать повторно (каждый шаг там защищён
        // через `if let Some(...)` / `.clear()`), поэтому теперь Drop
        // всегда вызывает shutdown() — если он уже был вызван явно
        // (как в main1.rs/main2.rs), повторный вызов будет просто no-op.
        self.shutdown();
    }
}