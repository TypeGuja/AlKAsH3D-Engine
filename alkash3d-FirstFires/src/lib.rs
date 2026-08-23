//! FirstFires - GPU Light Culling System
//! Динамическая библиотека для каллинга источников света

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use nalgebra::{Vector3, Matrix4};
use rayon::prelude::*;

// ===================================================================
// Внутренние модули
// ===================================================================

mod light;
mod culling;
mod grid;
mod stats;

use light::*;
use culling::*;
use grid::*;
use stats::*;

// ===================================================================
// Структуры для Plugin API (совместимые с движком)
// ===================================================================

pub const PLUGIN_API_VERSION: u32 = 1;
pub const PLUGIN_TYPE_LIGHT: u32 = 1;

/// Конфигурация света
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightConfig {
    pub max_lights: u32,
    pub tile_size: u32,
    pub far_plane: f32,
    pub lod_distances: [f32; 3],
    pub grid_cell_size: f32,
}

/// GPU-совместимая структура света
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GPULight {
    pub position: [f32; 4],  // xyz, type (0=point,1=spot,2=directional)
    pub color: [f32; 4],     // rgb, intensity
    pub direction: [f32; 4], // xyz, range
    pub params: [f32; 4],    // spot_angle, falloff, lod, padding
}

unsafe impl bytemuck::Zeroable for GPULight {}
unsafe impl bytemuck::Pod for GPULight {}

/// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): явный ABI-контракт для
/// параметров пространственной сетки, которую `LightState` уже строит
/// внутри `cull()` (см. поля `world_min`/`cell_size`/`grid_width` и т.д.
/// ниже), но раньше не отдавал наружу. ДОЛЖНА побайтово совпадать со
/// своей копией в alkash3d-rust/src/plugin/light_api.rs — это две стороны
/// одного и того же C-ABI, как и остальные структуры этого файла.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LightGridParams {
    pub world_min: [f32; 3],
    pub cell_size: f32,
    pub world_max: [f32; 3],
    pub grid_width: u32,
    pub grid_height: u32,
    pub grid_depth: u32,
    pub _padding: u32,
}

/// Ячейка световой сетки
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LightGridCell {
    pub offset: u32,
    pub count: u32,
}

/// Запись в сетке
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightGridEntry {
    pub light_index: u32,
    pub lod_level: u32,
    pub depth: f32,
    pub padding: u32,
}

/// Статистика каллинга
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LightStats {
    pub total_lights: u32,
    pub visible_lights: u32,
    pub culled_by_lod: u32,
    pub culled_by_distance: u32,
    pub culled_by_frustum: u32,
    pub culling_time_ms: f32,
}

/// API света для движка
#[repr(C)]
pub struct LightAPI {
    pub add_light: extern "C" fn(instance: *mut c_void, light: *const GPULight) -> u32,
    pub remove_light: extern "C" fn(instance: *mut c_void, id: u32),
    pub update_light: extern "C" fn(instance: *mut c_void, id: u32, light: *const GPULight),
    pub get_lights_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub cull: extern "C" fn(instance: *mut c_void, camera_pos: *const f32, view_proj: *const f32, dt: f32),
    pub get_gpu_lights: extern "C" fn(instance: *mut c_void) -> *const GPULight,
    pub get_gpu_lights_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_light_grid_cells: extern "C" fn(instance: *mut c_void) -> *const LightGridCell,
    pub get_light_grid_entries: extern "C" fn(instance: *mut c_void) -> *const LightGridEntry,
    pub get_grid_cells_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_grid_entries_count: extern "C" fn(instance: *mut c_void) -> u32,
    pub get_stats: extern "C" fn(instance: *mut c_void) -> LightStats,
    // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям) — см. LightGridParams.
    // В КОНЦЕ структуры, не в середине (см. подробное объяснение в
    // зеркальной копии LightAPI в alkash3d-rust/src/plugin/light_api.rs).
    pub get_grid_params: extern "C" fn(instance: *mut c_void) -> LightGridParams,
}

/// Базовый API плагина
#[repr(C)]
pub struct PluginAPI {
    pub version: u32,
    pub plugin_type: u32,
    pub name: *const std::os::raw::c_char,
    pub init: extern "C" fn(device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void,
    pub shutdown: extern "C" fn(instance: *mut c_void),
    pub update: extern "C" fn(instance: *mut c_void, dt: f32),
    pub get_physics_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
    pub get_light_api: extern "C" fn(instance: *mut c_void) -> *const c_void,
}

// ===================================================================
// Внутреннее состояние
// ===================================================================

/// Внутренняя структура света
#[derive(Debug, Clone)]
struct InternalLight {
    id: u32,
    position: Vector3<f32>,
    color: Vector3<f32>,
    intensity: f32,
    range: f32,
    light_type: LightType,
    spot_angle: f32,
    spot_direction: Vector3<f32>,
    falloff: f32,
    // ДОБАВЛЕНО (Фаза 4 плана по реализму/фонарям): раньше это поле
    // отсутствовало — `add_light()` получал GPULight.params[2]
    // (spot_inner_angle, см. движковую сторону в engine/mod.rs) на входе,
    // но НИКУДА его не сохранял, а `to_gpu()` жёстко писал 0.0 обратно.
    // То есть внутренний угол конуса молча ТЕРЯЛСЯ между add_light() и
    // следующим cull()/get_gpu_lights() — движок получал бы обратно
    // spot_inner_angle=0 независимо от того, что реально было передано.
    // Теперь сохраняется и возвращается как есть (round-trip).
    spot_inner_angle: f32,
}

impl InternalLight {
    fn to_gpu(&self) -> GPULight {
        GPULight {
            position: [self.position.x, self.position.y, self.position.z, self.light_type as u32 as f32],
            color: [self.color.x, self.color.y, self.color.z, self.intensity],
            direction: [self.spot_direction.x, self.spot_direction.y, self.spot_direction.z, self.range],
            params: [self.spot_angle, self.falloff, self.spot_inner_angle, 0.0],
        }
    }
}

struct LightState {
    lights: Vec<InternalLight>,
    gpu_lights: Vec<GPULight>,
    grid_cells: Vec<LightGridCell>,
    grid_entries: Vec<LightGridEntry>,
    stats: LightStats,
    config: LightConfig,
    next_id: AtomicU32,
    grid_width: u32,
    grid_height: u32,
    grid_depth: u32,
    world_min: Vector3<f32>,
    world_max: Vector3<f32>,
    cell_size: f32,
}

impl LightState {
    fn new(config: &LightConfig) -> Self {
        let world_size = config.far_plane;
        let world_min = Vector3::new(-world_size, -world_size, -world_size);
        let world_max = Vector3::new(world_size, world_size, world_size);
        let cell_size = config.grid_cell_size;

        let size = world_max - world_min;
        let grid_width = (size.x / cell_size).ceil() as u32;
        let grid_height = (size.y / cell_size).ceil() as u32;
        let grid_depth = (size.z / cell_size).ceil() as u32;
        let total_cells = (grid_width * grid_height * grid_depth) as usize;

        Self {
            lights: Vec::with_capacity(config.max_lights as usize),
            gpu_lights: Vec::with_capacity(config.max_lights as usize),
            grid_cells: vec![LightGridCell::default(); total_cells],
            grid_entries: Vec::with_capacity(config.max_lights as usize * 4),
            stats: LightStats::default(),
            config: *config,
            next_id: AtomicU32::new(0),
            grid_width,
            grid_height,
            grid_depth,
            world_min,
            world_max,
            cell_size,
        }
    }

    fn add_light(&mut self, light: &GPULight) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let light_type = match (light.position[3] as u32) {
            0 => LightType::Point,
            1 => LightType::Spot,
            2 => LightType::Directional,
            _ => LightType::Point,
        };

        let internal = InternalLight {
            id,
            position: Vector3::new(light.position[0], light.position[1], light.position[2]),
            color: Vector3::new(light.color[0], light.color[1], light.color[2]),
            intensity: light.color[3],
            range: light.direction[3],
            light_type,
            spot_angle: light.params[0],
            spot_direction: Vector3::new(light.direction[0], light.direction[1], light.direction[2]),
            falloff: light.params[1],
            // ДОБАВЛЕНО (Фаза 4 плана по реализму/фонарям) — см. комментарий
            // у поля `spot_inner_angle` в InternalLight выше.
            spot_inner_angle: light.params[2],
        };

        self.lights.push(internal);
        self.stats.total_lights = self.lights.len() as u32;
        id
    }

    /// ИСПРАВЛЕНО (баг: мерцание/день-ночь фонарей молча не работали):
    /// раньше `extern "C" fn update_light` ниже был пустой заглушкой
    /// ("для простоты не реализуем обновление") — движковая сторона
    /// (`AlkashEngine::update_day_night` в engine/mod.rs) каждый кадр
    /// честно считала новую intensity (мерцание по flicker_speed/
    /// flicker_intensity, включение/выключение по active_from/active_to)
    /// и вызывала `LightPlugin::update_light`, но результат никуда не
    /// попадал: свет, добавленный через `add_light()`, так и оставался с
    /// ПЕРВОНАЧАЛЬНЫМИ параметрами навсегда — визуально это выглядело как
    /// полностью статичные фонари без единого намёка на мерцание или
    /// суточный цикл, хотя вся математика для этого была на месте.
    ///
    /// Ищем свет по `id` (линейный проход — при `max_lights` в единицы
    /// десятков это пренебрежимо дёшево, а `remove_light()` не реализован
    /// намеренно, см. комментарий там, так что `id` не гарантированно
    /// совпадает с индексом после возможных будущих удалений — искать по
    /// `id`, а не считать `id == index`, единственный надёжный вариант) и
    /// перезаписываем ВСЕ поля, которые может передать `GPULight` — та же
    /// раскладка полей, что и в `add_light()` выше, чтобы обновление вело
    /// себя идентично повторному добавлению того же света.
    fn update_light(&mut self, id: u32, light: &GPULight) {
        if let Some(internal) = self.lights.iter_mut().find(|l| l.id == id) {
            let light_type = match light.position[3] as u32 {
                0 => LightType::Point,
                1 => LightType::Spot,
                2 => LightType::Directional,
                _ => LightType::Point,
            };
            internal.position = Vector3::new(light.position[0], light.position[1], light.position[2]);
            internal.color = Vector3::new(light.color[0], light.color[1], light.color[2]);
            internal.intensity = light.color[3];
            internal.range = light.direction[3];
            internal.light_type = light_type;
            internal.spot_angle = light.params[0];
            internal.spot_direction = Vector3::new(light.direction[0], light.direction[1], light.direction[2]);
            internal.falloff = light.params[1];
            internal.spot_inner_angle = light.params[2];
        }
        // Свет с таким id не найден (например id из другого, уже
        // выгруженного instance плагина) — тихо игнорируем, как и
        // остальные функции этого ABI обходятся с невалидными id
        // (см. `remove_light`), а не паникуют/логируют на каждый кадр.
    }

    // ИСПРАВЛЕНО: больше не используется в `cull()` — заменён на
    // `get_cell_indices_for_sphere` (см. подробный комментарий там).
    // Оставлен как есть (не удалён) — тривиальная и потенциально полезная
    // утилита "какой ячейке принадлежит точка", `#[allow(dead_code)]`,
    // чтобы не засорять сборку предупреждением.
    #[allow(dead_code)]
    fn get_cell_index(&self, pos: Vector3<f32>) -> Option<usize> {
        let local = pos - self.world_min;

        if local.x < 0.0 || local.y < 0.0 || local.z < 0.0 {
            return None;
        }

        let x = (local.x / self.cell_size) as u32;
        let y = (local.y / self.cell_size) as u32;
        let z = (local.z / self.cell_size) as u32;

        if x >= self.grid_width || y >= self.grid_height || z >= self.grid_depth {
            return None;
        }

        Some((z * self.grid_height * self.grid_width + y * self.grid_width + x) as usize)
    }

    /// ИСПРАВЛЕНО (баг: "свет резко появляется/пропадает при ходьбе,
    /// уступами" — фонарь физически ещё должен освещать пиксель, но пол
    /// под ним резко темнеет): раньше свет регистрировался ТОЛЬКО в ОДНОЙ
    /// ячейке сетки — той, где находится сам источник (`get_cell_index`
    /// по позиции фонаря). Но пиксельный шейдер ищет фонари ТОЛЬКО в
    /// ячейке, которой принадлежит ОСВЕЩАЕМЫЙ ПИКСЕЛЬ (см. `main()` в
    /// `compile_default_shaders`, engine/mod.rs) — если `light.range`
    /// (радиус физического действия света) больше `cell_size` (типичная
    /// ситуация: уличный фонарь range=15м при cell_size=10м, см.
    /// LightConfig в main.rs), сфера освещения фонаря выходит ЗА
    /// пределы его собственной ячейки в соседние — но шейдер эти соседние
    /// ячейки никогда не проверял, поэтому свет резко обрывался РОВНО НА
    /// ГРАНИЦЕ ячейки сетки, а не на границе своего реального радиуса
    /// действия. Это и создавало резкие "уступы" освещённости при ходьбе
    /// — переход в соседнюю ячейку сетки (не самого фонаря!) выглядел как
    /// внезапное гашение/разгорание света.
    ///
    /// Стандартное решение для clustered/tiled lighting: регистрировать
    /// источник во ВСЕХ ячейках, которые пересекает его bounding sphere
    /// (позиция + range), а не только в одной. Возвращает индексы всех
    /// таких ячеек (может быть пусто, если сфера целиком вне границ
    /// мира).
    fn get_cell_indices_for_sphere(&self, pos: Vector3<f32>, radius: f32) -> Vec<usize> {
        let min_local = pos - self.world_min - Vector3::new(radius, radius, radius);
        let max_local = pos - self.world_min + Vector3::new(radius, radius, radius);

        // Переводим в целочисленный диапазон индексов ячеек по каждой оси,
        // clamped к границам сетки — сфера может частично выходить за
        // world_min/world_max (например фонарь у самого края мира), в
        // этом случае просто не регистрируем те ячейки, которых нет.
        let cell_range = |min_v: f32, max_v: f32, dim: u32| -> Option<(u32, u32)> {
            if max_v < 0.0 || min_v >= dim as f32 * self.cell_size {
                return None; // сфера целиком вне диапазона этой оси
            }
            let lo = (min_v / self.cell_size).floor().max(0.0) as u32;
            let hi = ((max_v / self.cell_size).floor() as i64).clamp(0, dim as i64 - 1) as u32;
            if lo > hi { None } else { Some((lo, hi)) }
        };

        let (x_lo, x_hi) = match cell_range(min_local.x, max_local.x, self.grid_width) { Some(r) => r, None => return Vec::new() };
        let (y_lo, y_hi) = match cell_range(min_local.y, max_local.y, self.grid_height) { Some(r) => r, None => return Vec::new() };
        let (z_lo, z_hi) = match cell_range(min_local.z, max_local.z, self.grid_depth) { Some(r) => r, None => return Vec::new() };

        // Небольшой защитный предел на число ячеек одной сферы — при
        // разумных grid_cell_size/range (десятки метров) сфера пересекает
        // единицы-десятки ячеек; предел ловит патологический случай
        // (огромный range при крошечном cell_size), а не обычную работу.
        //
        // ИЗМЕНЕНО (по просьбе — дальность уличных фонарей увеличена с 15
        // до 100, см. create_night_city() в alfar_format.rs), затем
        // ОБНОВЛЕНО (просадка FPS до ~20, обнаруженная пользователем на
        // реальной сборке): при СТАРОЙ конфигурации (far_plane=100 -> мир
        // [-100,100]^3, grid_cell_size=10 -> сетка 20x20x20=8000 ячеек)
        // фонарь с range=100 пересекал почти ВЕСЬ объём сетки — сфера
        // диаметром 200 при мире со стороной тоже 200 покрывала все 8000
        // ячеек сразу, что и обваливало FPS (сетка каллинга переставала
        // хоть что-то фильтровать — на каждый пиксель перебирались ВСЕ
        // фонари сцены). Исправлено на стороне вызывающего кода (см.
        // main.rs/main1.rs/main2.rs): far_plane поднят до 200,
        // grid_cell_size — до 20 (та же память, 8000 ячеек), из-за чего
        // сфера фонаря range=100 покрывает теперь лишь ~1300 ячеек
        // (~17%) вместо всех 8000. MAX_CELLS_PER_LIGHT=20000 здесь
        // по-прежнему актуален (с запасом больше, чем реально нужно и при
        // старой, и при новой конфигурации) и НЕ является причиной
        // просадки — сам предел никогда не срабатывал (8000 < 20000), это
        // сетка была вырождена, а не переполнен предел. Значение
        // оставлено как есть — настоящая защита только от патологических
        // конфигураций (например cell_size около 0), не активное
        // ограничение при разумных far_plane/grid_cell_size.
        const MAX_CELLS_PER_LIGHT: usize = 20000;
        let mut result = Vec::new();
        'outer: for z in z_lo..=z_hi {
            for y in y_lo..=y_hi {
                for x in x_lo..=x_hi {
                    let idx = (z * self.grid_height * self.grid_width + y * self.grid_width + x) as usize;
                    result.push(idx);
                    if result.len() >= MAX_CELLS_PER_LIGHT {
                        break 'outer;
                    }
                }
            }
        }
        result
    }

    fn cull(&mut self, camera_pos: [f32; 3], view_proj: &[f32; 16], _dt: f32) {
        let start = Instant::now();

        self.gpu_lights.clear();
        self.grid_entries.clear();

        // Очищаем сетку
        for cell in &mut self.grid_cells {
            cell.offset = 0;
            cell.count = 0;
        }

        let camera = Vector3::new(camera_pos[0], camera_pos[1], camera_pos[2]);

        // Создаём frustum из view_proj
        let view_proj_mat = Matrix4::new(
            view_proj[0], view_proj[1], view_proj[2], view_proj[3],
            view_proj[4], view_proj[5], view_proj[6], view_proj[7],
            view_proj[8], view_proj[9], view_proj[10], view_proj[11],
            view_proj[12], view_proj[13], view_proj[14], view_proj[15],
        );
        let frustum = Frustum::from_view_proj(&view_proj_mat);

        let mut culled_lod = 0;
        let mut culled_dist = 0;
        let mut culled_frustum = 0;

        // ИСПРАВЛЕНО (баг: уличные фонари резко гаснут/загораются при
        // ходьбе игрока): раньше здесь был ДОПОЛНИТЕЛЬНЫЙ тест `distance >
        // light.range * 1.2`, отдельный от LOD-дистанции. `light.range` —
        // радиус ВЛИЯНИЯ СВЕТА НА ОСВЕЩАЕМУЮ ИМ ПОВЕРХНОСТЬ (уже корректно
        // учтён в пиксельном шейдере через windowFalloff, см.
        // ComputePointLightContribution в alkash3d-rust/src/engine/mod.rs)
        // — он НЕ имеет отношения к тому, виден ли сам ИСТОЧНИК СВЕТА
        // камерой. Смешивать эти два понятия было ошибкой: для типичного
        // уличного фонаря (range=15м, см. create_night_city() в
        // alfar_format.rs) порог `range*1.2`=18м оказывался ГОРАЗДО
        // жёстче, чем LOD-дистанции (обычно 30-100+м, см. LightConfig в
        // main.rs) — фонарь, который физически ещё освещает видимую в
        // кадре геометрию (стены, пол рядом с камерой), резко пропадал из
        // списка видимых источников уже на 18 метрах, что и ощущалось как
        // "свет то есть, то резко гаснет" при обычной ходьбе вдоль улицы.
        // LOD-дистанция и frustum test — уже достаточные и физически
        // осмысленные критерии "не считать вклад этого света в данном
        // кадре"; отдельный жёсткий cutoff по `range` только дублировал их
        // менее подходящим порогом.

        // Параллельный каллинг
        let visible: Vec<(usize, u32, f32, Vector3<f32>)> = self.lights
            .par_iter()
            .enumerate()
            .filter_map(|(idx, light)| {
                let distance = (light.position - camera).magnitude();

                // LOD culling
                let lod = if distance < self.config.lod_distances[0] {
                    0
                } else if distance < self.config.lod_distances[1] {
                    1
                } else if distance < self.config.lod_distances[2] {
                    2
                } else {
                    return None;
                };

                // Frustum culling
                if !frustum.test_sphere(light.position, light.range) {
                    return None;
                }

                Some((idx, lod, distance, light.position))
            })
            .collect();

        // Подсчёт статистики
        for light in &self.lights {
            let distance = (light.position - camera).magnitude();
            if distance >= self.config.lod_distances[2] {
                culled_lod += 1;
            } else if !frustum.test_sphere(light.position, light.range) {
                culled_frustum += 1;
            }
        }

        self.stats.culled_by_lod = culled_lod;
        // ИСПРАВЛЕНО: `culled_by_distance` больше не считается отдельно от
        // LOD (см. подробный комментарий выше у убранного теста `distance
        // > light.range * 1.2`) — всегда 0. Поле оставлено в
        // `LightStats` ради стабильности публичного ABI плагина
        // (`get_stats`/`LightStats` в этом же файле), а не удалено.
        self.stats.culled_by_distance = culled_dist;
        self.stats.culled_by_frustum = culled_frustum;

        // Сортируем по глубине (для правильного освещения)
        let mut visible_sorted = visible;
        visible_sorted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());

        // ИСПРАВЛЕНО (продолжение фикса "свет резко появляется/пропадает
        // уступами", см. подробный комментарий у
        // `get_cell_indices_for_sphere` выше): раньше каждый свет сразу
        // писался В ОДНУ ячейку прямо в этом цикле — `grid_cells[i].offset/
        // count` предполагает, что все записи ОДНОЙ ячейки лежат ПОДРЯД в
        // `grid_entries` (offset = начало диапазона, count = длина), что
        // легко гарантировать, когда на ячейку приходится максимум одна
        // запись за раз в порядке обхода. Теперь один свет может попасть
        // в НЕСКОЛЬКО ячеек (пересечение bounding sphere с сеткой) — если
        // писать записи сразу в порядке "по свету", записи одной и той же
        // ячейки окажутся раскиданы по `grid_entries`, а не подряд, и
        // offset/count перестанут быть валидным диапазоном. Поэтому
        // сначала собираем ВСЕ пары (cell_idx, entry) для всех светов,
        // затем группируем по cell_idx (stable sort по ключу ячейки
        // сохраняет исходный порядок по глубине внутри каждой ячейки —
        // важно для LOD/прозрачности), и только потом одним проходом
        // заполняем `grid_entries`/`grid_cells`, зная, что все записи
        // одной ячейки идут подряд.
        struct PendingEntry {
            cell_idx: usize,
            entry: LightGridEntry,
        }
        let mut pending: Vec<PendingEntry> = Vec::new();

        for (idx, lod, depth, position) in visible_sorted {
            let light = &self.lights[idx];
            let light_idx = self.gpu_lights.len() as u32;
            self.gpu_lights.push(light.to_gpu());

            let cell_indices = self.get_cell_indices_for_sphere(position, light.range);
            for cell_idx in cell_indices {
                pending.push(PendingEntry {
                    cell_idx,
                    entry: LightGridEntry {
                        light_index: light_idx,
                        lod_level: lod,
                        depth,
                        padding: 0,
                    },
                });
            }
        }

        // Стабильная сортировка по cell_idx — группирует все записи одной
        // ячейки подряд, не переставляя записи РАЗНЫХ светов внутри одной
        // ячейки местами (сохраняется относительный порядок вставки —
        // порядок по глубине, заданный visible_sorted выше).
        pending.sort_by_key(|p| p.cell_idx);

        for p in pending {
            self.grid_entries.push(p.entry);
            let cell = &mut self.grid_cells[p.cell_idx];
            if cell.count == 0 {
                cell.offset = (self.grid_entries.len() - 1) as u32;
            }
            cell.count += 1;
        }

        self.stats.visible_lights = self.gpu_lights.len() as u32;
        self.stats.culling_time_ms = start.elapsed().as_secs_f32() * 1000.0;
    }
}

// ===================================================================
// Глобальное состояние
// ===================================================================

thread_local! {
    static LIGHT_STATE: std::cell::RefCell<Option<LightState>> = std::cell::RefCell::new(None);
}

// ===================================================================
// Экспортируемые функции для LightAPI
// ===================================================================

extern "C" fn add_light(instance: *mut c_void, light: *const GPULight) -> u32 {
    if instance.is_null() || light.is_null() { return u32::MAX; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.add_light(&*light)
    }
}

extern "C" fn remove_light(instance: *mut c_void, _id: u32) {
    if instance.is_null() { return; }
    // Для простоты не реализуем удаление
}

/// ИСПРАВЛЕНО (баг: мерцание/день-ночь фонарей молча не работали) — см.
/// подробный комментарий у `LightState::update_light` выше. Раньше эта
/// функция была пустой заглушкой и молча отбрасывала ЛЮБОЕ обновление
/// уже добавленного света.
extern "C" fn update_light(instance: *mut c_void, id: u32, light: *const GPULight) {
    if instance.is_null() || light.is_null() { return; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.update_light(id, &*light);
    }
}

extern "C" fn get_lights_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.lights.len() as u32
    }
}

extern "C" fn cull_lights(instance: *mut c_void, camera_pos: *const f32, view_proj: *const f32, dt: f32) {
    if instance.is_null() || camera_pos.is_null() || view_proj.is_null() { return; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        let pos = [*camera_pos, *camera_pos.add(1), *camera_pos.add(2)];
        let vp = std::slice::from_raw_parts(view_proj, 16);
        state.cull(pos, vp.try_into().unwrap(), dt);
    }
}

extern "C" fn get_gpu_lights(instance: *mut c_void) -> *const GPULight {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        if state.gpu_lights.is_empty() { ptr::null() } else { state.gpu_lights.as_ptr() }
    }
}

extern "C" fn get_gpu_lights_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.gpu_lights.len() as u32
    }
}

extern "C" fn get_light_grid_cells(instance: *mut c_void) -> *const LightGridCell {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        if state.grid_cells.is_empty() { ptr::null() } else { state.grid_cells.as_ptr() }
    }
}

extern "C" fn get_light_grid_entries(instance: *mut c_void) -> *const LightGridEntry {
    if instance.is_null() { return ptr::null(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        if state.grid_entries.is_empty() { ptr::null() } else { state.grid_entries.as_ptr() }
    }
}

extern "C" fn get_grid_cells_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.grid_cells.len() as u32
    }
}

extern "C" fn get_grid_entries_count(instance: *mut c_void) -> u32 {
    if instance.is_null() { return 0; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.grid_entries.len() as u32
    }
}

extern "C" fn get_stats(instance: *mut c_void) -> LightStats {
    if instance.is_null() { return LightStats::default(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.stats
    }
}

/// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): отдаёт наружу параметры
/// сетки, которые `LightState::new()` вычисляет из LightConfig при
/// инициализации и хранит в приватных полях `world_min`/`cell_size`/
/// `grid_width`/... — до этой функции движок не мог сопоставить мировую
/// позицию пикселя с индексом ячейки `grid_cells`/`grid_entries`.
extern "C" fn get_grid_params(instance: *mut c_void) -> LightGridParams {
    if instance.is_null() { return LightGridParams::default(); }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        LightGridParams {
            world_min: [state.world_min.x, state.world_min.y, state.world_min.z],
            cell_size: state.cell_size,
            world_max: [state.world_max.x, state.world_max.y, state.world_max.z],
            grid_width: state.grid_width,
            grid_height: state.grid_height,
            grid_depth: state.grid_depth,
            _padding: 0,
        }
    }
}

static LIGHT_API: LightAPI = LightAPI {
    add_light,
    remove_light,
    update_light,
    get_lights_count,
    cull: cull_lights,
    get_gpu_lights,
    get_gpu_lights_count,
    get_light_grid_cells,
    get_light_grid_entries,
    get_grid_cells_count,
    get_grid_entries_count,
    get_stats,
    get_grid_params,
};

// ===================================================================
// Экспортируемые функции для PluginAPI
// ===================================================================

extern "C" fn init_light(_device_ptr: *mut c_void, config_ptr: *const c_void) -> *mut c_void {
    if config_ptr.is_null() { return ptr::null_mut(); }
    unsafe {
        let config = &*(config_ptr as *const LightConfig);
        let state = Box::new(LightState::new(config));
        Box::into_raw(state) as *mut c_void
    }
}

extern "C" fn shutdown_light(instance: *mut c_void) {
    if !instance.is_null() {
        unsafe { let _ = Box::from_raw(instance as *mut LightState); }
    }
}

extern "C" fn update_plugin(_instance: *mut c_void, _dt: f32) {}

extern "C" fn get_physics_api(_instance: *mut c_void) -> *const c_void {
    ptr::null()
}

extern "C" fn get_light_api(instance: *mut c_void) -> *const c_void {
    &LIGHT_API as *const _ as *const c_void
}

// ===================================================================
// Главная экспортируемая функция
// ===================================================================

#[no_mangle]
pub extern "C" fn get_plugin_api() -> PluginAPI {
    PluginAPI {
        version: PLUGIN_API_VERSION,
        plugin_type: PLUGIN_TYPE_LIGHT,
        name: b"FirstFires Light Culling\0".as_ptr() as *const std::os::raw::c_char,
        init: init_light,
        shutdown: shutdown_light,
        update: update_plugin,
        get_physics_api,
        get_light_api,
    }
}