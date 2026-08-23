// src/constant_buffer.rs
//! Константный буфер для матриц трансформации

use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use crate::{Buffer, STATE};
use crate::engine::NUM_CASCADES;

/// ИСПРАВЛЕНО: добавлен `#[repr(C)]`. Эта структура копируется побайтово
/// в GPU constant buffer, а шейдер (`cbuffer TransformConstants : register(b0)`)
/// ожидает конкретный, стабильный порядок полей в памяти. Без `#[repr(C)]`
/// компилятор формально не обязан сохранять порядок полей структуры — на
/// практике для "плоских" POD-структур это обычно совпадает, но полагаться
/// на совпадение по умолчанию для чего-то, что мапится на GPU layout,
/// нельзя.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TransformConstants {
    pub model_view_proj: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub ambient_color: [f32; 4],
    // ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): сколько реальных
    // элементов лежит в light_buffer (StructuredBuffer<GPULight> t0) в
    // этом кадре — без этого пиксельный шейдер не может знать, где
    // заканчивается настоящий список видимых фонарей и начинается
    // непроинициализированный/устаревший хвост буфера (light_buffer растёт
    // степенями двойки и обычно больше, чем реально видимых источников).
    // [u32;4] вместо одного u32: HLSL cbuffer пакует поля по 16-байтным
    // регистрам — одиночный скалярный u32 после четырёх float4-полей всё
    // равно занял бы целый регистр, так что лишний паддинг неизбежен в
    // любом случае; делаем это явно, а не полагаемся на то, что Rust и
    // HLSL совпадут в скрытом паддинге.
    pub light_count: u32,
    pub _light_count_padding: [u32; 3],

    // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): параметры
    // пространственной сетки FirstFires (см. LightGridParams в
    // plugin/light_api.rs) — без них пиксельный шейдер не может
    // сопоставить worldPos своей ячейке в grid_cells_buffer/
    // grid_entries_buffer. grid_world_min.w используется под cell_size
    // (экономит отдельный float4-регистр — тот же приём, что уже
    // применён для position.w/color.w в GPULight).
    pub grid_world_min: [f32; 4], // xyz = world_min, w = cell_size
    pub grid_dimensions: [u32; 4], // x = grid_width, y = grid_height, z = grid_depth, w = padding

    // ИЗМЕНЕНО (Cascaded Shadow Maps): раньше здесь была ОДНА view-proj
    // матрица directional-света. Каскадные тени используют NUM_CASCADES
    // независимых ортографических проекций — каждая покрывает свой
    // диапазон дистанций от камеры (ближний каскад — маленькая площадь,
    // высокая тексельная плотность; дальний — большая площадь, низкая
    // плотность). Пиксельный шейдер выбирает нужный каскад по
    // view-space глубине пикселя (см. cascade_split_distances ниже) и
    // умножает worldPos на СООТВЕТСТВУЮЩУЮ матрицу этого каскада (см.
    // `AlkashEngine::compute_cascade_view_proj` в engine/mod.rs).
    pub light_view_proj: [[[f32; 4]; 4]; NUM_CASCADES],
    // ДОБАВЛЕНО (Cascaded Shadow Maps): view-space дистанции ДАЛЬНИХ
    // границ каждого каскада (см. cascade_far_distances в render_frame) —
    // шейдер сравнивает view-space depth пикселя с этими порогами, чтобы
    // определить, в каком из NUM_CASCADES каскадов искать тень. [f32;4]
    // вместо [f32; NUM_CASCADES] — тот же приём паддинга под 16-байтные
    // HLSL-регистры, что и у light_count/grid_dimensions выше; реально
    // используются только первые NUM_CASCADES компонент.
    pub cascade_split_distances: [f32; 4],
    // shadow_bias — сдвиг вдоль нормали перед сравнением глубины
    // (борьба с "shadow acne" — самозатенением из-за ограниченной
    // точности shadow map); shadow_map_size — разрешение shadow map в
    // тексселях, нужно PS для расчёта шага PCF-сэмплирования (1/size).
    // shadows_enabled — 0/1: пока свет не загружен из .alfar (или сцена
    // вообще не использует тени), PS обязан безопасно пропускать
    // shadow-выборку, а не читать непроинициализированную shadow map.
    pub shadow_bias: f32,
    pub shadow_map_size: f32,
    pub shadows_enabled: u32,
    pub _shadow_padding: u32,
}

impl TransformConstants {
    pub fn new() -> Self {
        Self {
            model_view_proj: [[0.0; 4]; 4],
            model: [[0.0; 4]; 4],
            view: [[0.0; 4]; 4],
            proj: [[0.0; 4]; 4],
            camera_pos: [0.0, 0.0, 0.0, 0.0],
            light_dir: [0.0, -1.0, 0.0, 0.0],
            light_color: [1.0, 1.0, 1.0, 1.0],
            ambient_color: [0.1, 0.1, 0.15, 1.0],
            light_count: 0,
            _light_count_padding: [0, 0, 0],
            grid_world_min: [0.0, 0.0, 0.0, 1.0],
            grid_dimensions: [0, 0, 0, 0],
            light_view_proj: [[[0.0; 4]; 4]; NUM_CASCADES],
            cascade_split_distances: [0.0, 0.0, 0.0, 0.0],
            shadow_bias: 0.0015,
            shadow_map_size: 2048.0,
            shadows_enabled: 0,
            _shadow_padding: 0,
        }
    }

    pub fn create_buffer() -> Result<Buffer> {
        let size = std::mem::size_of::<Self>() as u64;
        Buffer::create_constant_buffer(size)
    }

    /// Размер ОДНОГО слота в "массиве" константного буфера, выровненный
    /// на 256 байт — таково требование D3D12 к CBV (`D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT`).
    pub fn aligned_size() -> u64 {
        let raw = std::mem::size_of::<Self>() as u64;
        (raw + 255) & !255
    }

    /// Записывает данные ИМЕННО в слот `slot` внутри буфера, который
    /// должен вмещать как минимум `(slot + 1) * aligned_size()` байт (см.
    /// `Buffer::create_constant_buffer_array` и
    /// `AlkashEngine::ensure_constant_buffer_capacity`).
    ///
    /// ВАЖНО, почему это вообще нужно (а не просто писать в один и тот же
    /// адрес перед каждым Draw, как было раньше): GPU видит содержимое
    /// константного буфера НА МОМЕНТ, КОГДА РЕАЛЬНО ВЫПОЛНЯЕТ Draw — а не
    /// на момент, когда CPU туда что-то записал во время построения
    /// command list'а. CPU успевает выполнить ВСЕ свои записи (Map/Unmap)
    /// для целого кадра ещё ДО того, как ExecuteCommandLists вообще
    /// отправляет что-либо на GPU. Значит, если каждый объект кадра пишет
    /// свою трансформацию в ОДИН и тот же адрес, к моменту, когда GPU
    /// реально начнёт выполнять command list, в буфере будут лежать
    /// данные только ПОСЛЕДНЕГО записанного объекта — и КАЖДЫЙ Draw в
    /// кадре отрисуется с одной и той же (последней) трансформацией. Со
    /// стороны это выглядит так, будто все объекты "слиплись" в один и
    /// двигаются/вращаются синхронно — именно это и происходило с полом и
    /// кубом. Раздельные слоты решают проблему: у каждого Draw — свой
    /// адрес константного буфера с ЕГО собственными данными.
    pub fn write_at(&self, buffer: &Buffer, slot: usize) -> Result<()> {
        let offset = slot as u64 * Self::aligned_size();
        let data = unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        };
        buffer.update_constant_buffer_at(offset, data)
    }

    /// GPU-адрес для чтения слота `slot` — передаётся напрямую в
    /// `SetGraphicsRootConstantBufferView`.
    pub fn gpu_address_for_slot(buffer: &Buffer, slot: usize) -> u64 {
        unsafe { buffer.resource.GetGPUVirtualAddress() + slot as u64 * Self::aligned_size() }
    }

    pub fn update(&self, buffer: &Buffer) -> Result<()> {
        let data = unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        };
        buffer.update_constant_buffer(data)
    }
}

/// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): отдельный, НАМНОГО
/// более лёгкий константный буфер под shadow-проход (см.
/// `compile_shadow_shaders`/`create_shadow_root_signature` в
/// engine/mod.rs) — тому шейдеру нужна ровно ОДНА матрица
/// (model*light_view_proj), а не вся `TransformConstants` (камера/свет/
/// сетка каллинга ему не нужны вообще). Отдельная маленькая структура, а
/// не переиспользование `TransformConstants` с игнорированием лишних
/// полей — экономит и место в буфере (256-байтные слоты и так съедают
/// выравнивание, раздувать реальные данные до полного размера
/// TransformConstants было бы расточительно при сотнях объектов на кадр),
/// и предотвращает путаницу — какие поля этот проход реально читает.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShadowConstants {
    pub model_light_view_proj: [[f32; 4]; 4],
}

impl ShadowConstants {
    pub fn aligned_size() -> u64 {
        let raw = std::mem::size_of::<Self>() as u64;
        (raw + 255) & !255
    }

    pub fn write_at(&self, buffer: &Buffer, slot: usize) -> Result<()> {
        let offset = slot as u64 * Self::aligned_size();
        let data = unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        };
        buffer.update_constant_buffer_at(offset, data)
    }

    pub fn gpu_address_for_slot(buffer: &Buffer, slot: usize) -> u64 {
        unsafe { buffer.resource.GetGPUVirtualAddress() + slot as u64 * Self::aligned_size() }
    }
}