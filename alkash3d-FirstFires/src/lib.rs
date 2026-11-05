//! FirstFires - отсечение источников света (на CPU)
//! Динамическая библиотека для каллинга источников света

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use nalgebra::Vector3;
use rayon::prelude::*;

// ===================================================================
// Внутренние модули
// ===================================================================

mod light;
mod culling;

use light::*;
use culling::*;

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
    /// ДОБАВЛЕНО (remove_light): слот свободен, `add_light` займёт его снова.
    /// Слоты не сдвигаются: `id` остаётся индексом в `lights`.
    removed: bool,
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

/// Заявка лампы на ячейку сетки; `weight` — её свет на ближней точке ячейки.
#[derive(Clone, Copy)]
struct PendingEntry {
    cell_idx: u32,
    weight: f32,
    entry: LightGridEntry,
}

struct LightState {
    lights: Vec<InternalLight>,
    free_ids: Vec<u32>,
    pending: Vec<PendingEntry>,
    grouped: Vec<PendingEntry>,
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

/// ДОБАВЛЕНО (лаги на карте Самары с реальными фонарями): сетка каллинга
/// больше не стоит в мире неподвижно кубом ±far_plane вокруг начала
/// координат, а каждый кадр пересобирается вокруг камеры (см. `recenter`
/// в `cull`). По вертикали ей не нужен весь far_plane — город лежит в слое
/// высотой в сотни метров, поэтому половина высоты ограничена этим
/// значением: при мелких ячейках (25 м) число ячеек остаётся постоянным и
/// умеренным (~64 тыс. при far_plane=1000), буферы движка не перевыделяются.
const GRID_HALF_HEIGHT: f32 = 128.0;

/// ДОБАВЛЕНО: предел источников на ячейку. Пиксель перебирает ВСЕ
/// источники своей ячейки; в центре города их в одной ячейке набиралось
/// по 50+ (а светят на пиксель 3–4). Записи ячейки уже идут по удалённости
/// от камеры (visible_sorted), так что отбрасываются самые дальние.
const MAX_LIGHTS_PER_CELL: u32 = 32;

/// ДОБАВЛЕНО (непрямой свет за кадром): сколько ламп вне кадра идёт в список
/// после видимых. Сетка на них не ссылается, основной шейдер их не перебирает;
/// они нужны лучам DDGI и отражений: лампа за спиной светит на стену, которая
/// видна в кадре.
const MAX_OFFSCREEN_LIGHTS: usize = 64;

/// Предел ячеек одной лампы: защита от патологических настроек (огромный range
/// при крошечной ячейке), в обычной работе не срабатывает.
const MAX_CELLS_PER_LIGHT: usize = 20000;

impl LightState {
    fn new(config: &LightConfig) -> Self {
        let world_size = config.far_plane;
        let half_h = world_size.min(GRID_HALF_HEIGHT);
        let world_min = Vector3::new(-world_size, -half_h, -world_size);
        let world_max = Vector3::new(world_size, half_h, world_size);
        let cell_size = config.grid_cell_size;

        let size = world_max - world_min;
        let grid_width = (size.x / cell_size).ceil() as u32;
        let grid_height = (size.y / cell_size).ceil() as u32;
        let grid_depth = (size.z / cell_size).ceil() as u32;
        let total_cells = (grid_width * grid_height * grid_depth) as usize;

        Self {
            lights: Vec::with_capacity(config.max_lights as usize),
            free_ids: Vec::new(),
            pending: Vec::new(),
            grouped: Vec::new(),
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
        let reused = self.free_ids.pop();
        let id = reused.unwrap_or_else(|| self.next_id.fetch_add(1, Ordering::Relaxed));

        let light_type = match light.position[3] as u32 {
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
            removed: false,
        };

        match reused {
            Some(slot) => self.lights[slot as usize] = internal,
            None => self.lights.push(internal),
        }
        self.stats.total_lights = self.live_lights();
        id
    }

    fn live_lights(&self) -> u32 {
        (self.lights.len() - self.free_ids.len()) as u32
    }

    /// ДОБАВЛЕНО: раньше заглушка, и игры гасили лампы вместо удаления.
    /// Занятый слот освобождается и достаётся следующему `add_light`.
    fn remove_light(&mut self, id: u32) {
        if let Some(light) = self.lights.get_mut(id as usize) {
            if !light.removed {
                light.removed = true;
                self.free_ids.push(id);
                self.stats.total_lights = self.live_lights();
            }
        }
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
    ///
    /// ИСПРАВЛЕНО (perf: 57 728 фонарей Самары): линейный `find` по id на
    /// каждый вызов давал O(N²) за кадр (~1.7 млрд сравнений). `id` выдаётся
    /// по порядку из `next_id`, а `lights` только растёт через `push` (удаления
    /// нет), поэтому сначала пробуем `lights[id]` напрямую — O(1); линейный
    /// поиск остаётся запасным путём, если это равенство когда-нибудь нарушится.
    fn update_light(&mut self, id: u32, light: &GPULight) {
        let direct = self.lights.get(id as usize).map_or(false, |l| l.id == id);
        let slot = if direct {
            self.lights.get_mut(id as usize)
        } else {
            self.lights.iter_mut().find(|l| l.id == id)
        };
        if let Some(internal) = slot.filter(|l| !l.removed) {
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
    /// уступами"): лампа регистрируется во всех ячейках, которых касается её
    /// сфера, а не в одной своей: шейдер смотрит только ячейку пикселя, и
    /// иначе свет обрывался бы на границе ячейки, а не на своём радиусе.
    ///
    /// ИЗМЕНЕНО: ячейки проверяются по самой сфере, а не по её габаритному
    /// кубу (углы куба — около половины его объёма), прожектор — ещё и по
    /// конусу; у каждой заявки считается свет лампы на ближней точке ячейки.
    fn push_cells(&self, light: &InternalLight, entry: LightGridEntry, out: &mut Vec<PendingEntry>) {
        let radius = light.range;
        let local = light.position - self.world_min;
        let cell = self.cell_size;

        let cell_range = |v: f32, dim: u32| -> Option<(u32, u32)> {
            if v + radius < 0.0 || v - radius >= dim as f32 * cell {
                return None;
            }
            let lo = ((v - radius) / cell).floor().max(0.0) as u32;
            let hi = (((v + radius) / cell).floor() as i64).clamp(0, dim as i64 - 1) as u32;
            (lo <= hi).then_some((lo, hi))
        };
        let (Some((x_lo, x_hi)), Some((y_lo, y_hi)), Some((z_lo, z_hi))) = (
            cell_range(local.x, self.grid_width),
            cell_range(local.y, self.grid_height),
            cell_range(local.z, self.grid_depth),
        ) else {
            return;
        };

        let directional = matches!(light.light_type, LightType::Directional);
        let cone = match light.light_type {
            LightType::Spot if light.spot_angle > 0.0 && light.spot_angle < std::f32::consts::FRAC_PI_2 => {
                light.spot_direction.try_normalize(1e-6).map(|d| (d, light.spot_angle.sin(), light.spot_angle.cos()))
            }
            _ => None,
        };
        let half_diagonal = cell * 0.5 * 3f32.sqrt();
        let strength = light.intensity * light.color.max();

        let mut added = 0;
        for z in z_lo..=z_hi {
            for y in y_lo..=y_hi {
                for x in x_lo..=x_hi {
                    let cell_min = Vector3::new(x as f32, y as f32, z as f32) * cell;
                    let closest = local.zip_zip_map(&cell_min, &(cell_min + Vector3::repeat(cell)), |v, lo, hi| v.clamp(lo, hi));
                    let dist2 = (closest - local).norm_squared();
                    if !directional && dist2 > radius * radius {
                        continue;
                    }
                    if let Some((dir, sin_a, cos_a)) = cone {
                        // сфера вокруг ячейки против конуса
                        let to_cell = cell_min + Vector3::repeat(cell * 0.5) - local;
                        let along = to_cell.dot(&dir);
                        let across = (to_cell.norm_squared() - along * along).max(0.0).sqrt();
                        if along < -half_diagonal || across * cos_a - along * sin_a > half_diagonal {
                            continue;
                        }
                    }
                    out.push(PendingEntry {
                        cell_idx: z * self.grid_height * self.grid_width + y * self.grid_width + x,
                        weight: strength / dist2.max(0.01),
                        entry,
                    });
                    added += 1;
                    if added >= MAX_CELLS_PER_LIGHT {
                        return;
                    }
                }
            }
        }
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

        // Сетка — вокруг камеры (с привязкой к шагу ячейки, чтобы границы
        // ячеек не «плыли» за камерой и освещение не дрожало), размеры те же.
        let half = Vector3::new(
            self.grid_width as f32 * self.cell_size * 0.5,
            self.grid_height as f32 * self.cell_size * 0.5,
            self.grid_depth as f32 * self.cell_size * 0.5,
        );
        let snap = |v: f32| (v / self.cell_size).floor() * self.cell_size;
        self.world_min = Vector3::new(snap(camera.x - half.x), snap(camera.y - half.y), snap(camera.z - half.z));
        self.world_max = self.world_min + half * 2.0;

        // Создаём frustum из view_proj
        // view_proj от движка — по столбцам (см. `Frustum::from_column_major`)
        let frustum = Frustum::from_column_major(view_proj);

        let culled_dist = 0;

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

        // ИЗМЕНЕНО (perf — см. `examples/perf_test.rs`, добавленный по
        // прямому запросу пользователя на замер производительности:
        // culling света оказался ощутимо дороже per-item, чем физика
        // Inertial на сопоставимом числе объектов): раньше здесь было ДВА
        // прохода по всем светам за кадр — этот, параллельный (реальный
        // результат), и отдельный ПОСЛЕДОВАТЕЛЬНЫЙ ниже, который заново
        // считал distance/frustum test ТОЛЬКО ради статистики
        // (`culled_lod`/`culled_frustum`) — то есть самый дорогой тест
        // (`frustum.test_sphere`) выполнялся для каждого света дважды за
        // кадр. Теперь причина отбраковки (LOD/frustum) возвращается прямо
        // из ТОГО ЖЕ параллельного прохода через `LightCullOutcome`, и
        // счётчики статистики считаются во время группировки результата —
        // второй проход убран целиком.
        enum LightCullOutcome {
            Visible(usize, u32, f32),
            OffScreen(usize, f32),
            CulledLod,
        }

        let outcomes: Vec<LightCullOutcome> = self.lights
            .par_iter()
            .enumerate()
            .map(|(idx, light)| {
                // ДОБАВЛЕНО (perf): выключенный фонарь (день/ночь выставил
                // intensity = 0) ничего не освещает — не тратим на него сетку и
                // шейдер. Лампа с NaN в координатах не должна ломать кадр.
                if light.removed || light.intensity <= 0.0 || !light.position.iter().all(|v| v.is_finite()) {
                    return LightCullOutcome::CulledLod;
                }
                let distance = (light.position - camera).magnitude();

                // ИСПРАВЛЕНО (лампа гасла разом): порог LOD сравнивался с
                // расстоянием до самой лампы, и лампа с большим радиусом
                // пропадала, хотя ещё светила рядом с камерой. Теперь — до
                // края её сферы.
                let reach = (distance - light.range).max(0.0);
                let lod = if reach < self.config.lod_distances[0] {
                    0
                } else if reach < self.config.lod_distances[1] {
                    1
                } else if reach < self.config.lod_distances[2] {
                    2
                } else {
                    return LightCullOutcome::CulledLod;
                };

                // Frustum culling
                if !frustum.test_sphere(light.position, light.range) {
                    return LightCullOutcome::OffScreen(idx, reach);
                }

                LightCullOutcome::Visible(idx, lod, distance)
            })
            .collect();

        let mut visible: Vec<(usize, u32, f32)> = Vec::with_capacity(outcomes.len());
        let mut offscreen: Vec<(usize, f32)> = Vec::new();
        let mut culled_lod = 0u32;
        for outcome in outcomes {
            match outcome {
                LightCullOutcome::Visible(idx, lod, distance) => visible.push((idx, lod, distance)),
                LightCullOutcome::OffScreen(idx, reach) => offscreen.push((idx, reach)),
                LightCullOutcome::CulledLod => culled_lod += 1,
            }
        }
        let culled_frustum = offscreen.len() as u32;

        self.stats.culled_by_lod = culled_lod;
        // ИСПРАВЛЕНО: `culled_by_distance` больше не считается отдельно от
        // LOD (см. подробный комментарий выше у убранного теста `distance
        // > light.range * 1.2`) — всегда 0. Поле оставлено в
        // `LightStats` ради стабильности публичного ABI плагина
        // (`get_stats`/`LightStats` в этом же файле), а не удалено.
        self.stats.culled_by_distance = culled_dist;
        self.stats.culled_by_frustum = culled_frustum;

        // Ближние — первыми: первые 32 лампы списка дают свечение воздуха
        visible.sort_by(|a, b| a.2.total_cmp(&b.2));

        // Запись ячейки должна лежать в `grid_entries` подряд (offset + count),
        // а лампа попадает во много ячеек, поэтому сначала собираем все заявки.
        let mut pending = std::mem::take(&mut self.pending);
        pending.clear();
        for &(idx, lod, depth) in &visible {
            let light = &self.lights[idx];
            let entry = LightGridEntry {
                light_index: self.gpu_lights.len() as u32,
                lod_level: lod,
                depth,
                padding: 0,
            };
            self.gpu_lights.push(light.to_gpu());
            self.push_cells(light, entry, &mut pending);
        }
        self.fill_grid(&pending);
        self.pending = pending;
        let visible_count = self.gpu_lights.len() as u32;

        // Лампы вне кадра: ближайшие к камере по краю сферы, после видимых
        offscreen.sort_by(|a, b| a.1.total_cmp(&b.1));
        for &(idx, _) in offscreen.iter().take(MAX_OFFSCREEN_LIGHTS) {
            self.gpu_lights.push(self.lights[idx].to_gpu());
        }

        self.stats.visible_lights = visible_count;
        self.stats.culling_time_ms = start.elapsed().as_secs_f32() * 1000.0;
    }

    /// Раскладывает заявки по ячейкам сортировкой подсчётом, за один проход.
    ///
    /// ИСПРАВЛЕНО (лимит ячейки): сверх `MAX_LIGHTS_PER_CELL` отбрасывались
    /// самые дальние от камеры, и ячейка могла потерять лампу, которая висит
    /// прямо в ней. Теперь остаются те, что светят в ячейку сильнее.
    fn fill_grid(&mut self, pending: &[PendingEntry]) {
        for p in pending {
            self.grid_cells[p.cell_idx as usize].count += 1;
        }
        let mut next = 0u32;
        for cell in &mut self.grid_cells {
            cell.offset = next;
            next += cell.count;
        }
        let Some(&first) = pending.first() else { return };
        self.grouped.clear();
        self.grouped.resize(pending.len(), first);
        for p in pending {
            let cell = &mut self.grid_cells[p.cell_idx as usize];
            self.grouped[cell.offset as usize] = *p;
            cell.offset += 1;
        }
        // offset теперь указывает на конец группы ячейки
        for cell in &mut self.grid_cells {
            if cell.count == 0 {
                cell.offset = 0;
                continue;
            }
            let end = cell.offset as usize;
            let group = &mut self.grouped[end - cell.count as usize..end];
            let keep = cell.count.min(MAX_LIGHTS_PER_CELL) as usize;
            if group.len() > keep {
                group.select_nth_unstable_by(keep - 1, |a, b| b.weight.total_cmp(&a.weight));
            }
            cell.offset = self.grid_entries.len() as u32;
            cell.count = keep as u32;
            self.grid_entries.extend(group[..keep].iter().map(|p| p.entry));
        }
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

extern "C" fn remove_light(instance: *mut c_void, id: u32) {
    if instance.is_null() { return; }
    unsafe {
        let state = &mut *(instance as *mut LightState);
        state.remove_light(id);
    }
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
        state.live_lights()
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

extern "C" fn get_light_api(_instance: *mut c_void) -> *const c_void {
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
#[cfg(test)]
mod grid_tests {
    use super::*;
    use nalgebra::{Matrix4, Point3, Vector3};

    #[test]
    fn grid_follows_camera_and_caps_cells() {
        let config = LightConfig { max_lights: 64, tile_size: 16, far_plane: 1000.0, lod_distances: [30.0, 60.0, 1000.0], grid_cell_size: 25.0 };
        let mut st = LightState::new(&config);
        let cells_before = st.grid_cells.len();
        // 200 фонарей кучей в 5 км от начала координат (раньше — за пределами сетки)
        for i in 0..200 {
            let x = 5000.0 + (i % 20) as f32 * 3.0;
            let z = 5040.0 + (i / 20) as f32 * 3.0;
            st.add_light(&GPULight { position: [x, 36.0, z, 0.0], color: [1.0, 0.8, 0.6, 5.0], direction: [0.0, -1.0, 0.0, 20.0], params: [0.0; 4] });
        }
        let cam = Point3::new(5000.0, 30.0, 5000.0);
        let view = Matrix4::look_at_rh(&cam, &Point3::new(5000.0, 30.0, 5100.0), &Vector3::y());
        let proj = Matrix4::new_perspective(16.0 / 9.0, 60f32.to_radians(), 0.1, 1000.0);
        let vp = proj * view;
        let mut vps = [0f32; 16];
        vps.copy_from_slice(vp.as_slice());
        st.cull([cam.x, cam.y, cam.z], &vps, 0.016);
        assert_eq!(st.grid_cells.len(), cells_before, "число ячеек постоянно");
        assert!((st.world_min.x - (5000.0 - 1000.0)).abs() <= 25.0, "сетка вокруг камеры: {:?}", st.world_min);
        assert!(st.gpu_lights.len() > 0, "фонари видны");
        let max = st.grid_cells.iter().map(|c| c.count).max().unwrap();
        assert!(max > 0 && max <= MAX_LIGHTS_PER_CELL, "в ячейке {}", max);
    }

    fn lamp(x: f32, y: f32, z: f32, intensity: f32, range: f32) -> GPULight {
        GPULight { position: [x, y, z, 0.0], color: [1.0, 1.0, 1.0, intensity], direction: [0.0, -1.0, 0.0, range], params: [0.0; 4] }
    }

    /// Камера в начале координат смотрит вдоль +Z.
    fn state(cell: f32, lod: [f32; 3]) -> (LightState, [f32; 16]) {
        let config = LightConfig { max_lights: 64, tile_size: 16, far_plane: 100.0, lod_distances: lod, grid_cell_size: cell };
        let view = Matrix4::look_at_rh(&Point3::origin(), &Point3::new(0.0, 0.0, 1.0), &Vector3::y());
        let proj = Matrix4::new_perspective(16.0 / 9.0, 60f32.to_radians(), 0.1, 1000.0);
        let mut vp = [0f32; 16];
        vp.copy_from_slice((proj * view).as_slice());
        (LightState::new(&config), vp)
    }

    #[test]
    fn lamp_behind_camera_stays_after_visible_and_out_of_grid() {
        let (mut st, vp) = state(4.0, [30.0, 60.0, 100.0]);
        st.add_light(&lamp(0.0, 0.0, 10.0, 5.0, 3.0));
        st.add_light(&lamp(0.0, 0.0, -10.0, 7.0, 3.0));
        st.cull([0.0; 3], &vp, 0.016);
        assert_eq!(st.gpu_lights.len(), 2);
        assert_eq!(st.gpu_lights[0].color[3], 5.0, "видимая первой");
        assert_eq!(st.gpu_lights[1].color[3], 7.0, "за спиной после неё");
        assert_eq!(st.stats.visible_lights, 1);
        assert_eq!(st.stats.culled_by_frustum, 1);
        assert!(st.grid_entries.iter().all(|e| e.light_index == 0), "сетка только на видимые");
    }

    #[test]
    fn far_lamp_with_big_range_is_not_cut_off() {
        let (mut st, vp) = state(25.0, [10.0, 20.0, 30.0]);
        st.add_light(&lamp(0.0, 0.0, 60.0, 5.0, 50.0));
        st.add_light(&lamp(0.0, 0.0, 60.0, 5.0, 5.0));
        st.cull([0.0; 3], &vp, 0.016);
        assert_eq!(st.stats.visible_lights, 1, "большой радиус достаёт до камеры, малый нет");
        assert_eq!(st.gpu_lights[0].direction[3], 50.0);
    }

    #[test]
    fn full_cell_keeps_lamps_that_light_it() {
        let (mut st, vp) = state(50.0, [100.0, 200.0, 300.0]);
        // ближе к камере, но намного слабее
        for i in 0..MAX_LIGHTS_PER_CELL {
            st.add_light(&lamp(i as f32 * 0.01, 0.0, 5.0, 0.001, 200.0));
        }
        st.add_light(&lamp(0.0, 0.0, 40.0, 1000.0, 200.0));
        st.cull([0.0; 3], &vp, 0.016);
        let strong = st.gpu_lights.iter().position(|l| l.color[3] == 1000.0).unwrap() as u32;
        assert!(st.grid_entries.iter().any(|e| e.light_index == strong), "сильная лампа в сетке");
        assert!(st.grid_cells.iter().all(|c| c.count <= MAX_LIGHTS_PER_CELL));
    }

    #[test]
    fn entries_of_cell_are_contiguous_and_belong_to_it() {
        let (mut st, vp) = state(4.0, [100.0, 200.0, 300.0]);
        for i in 0..40 {
            st.add_light(&lamp(i as f32 - 20.0, (i % 5) as f32, 20.0 + (i % 7) as f32, 5.0, 6.0));
        }
        st.cull([0.0; 3], &vp, 0.016);
        let (w, h) = (st.grid_width as usize, st.grid_height as usize);
        for (i, c) in st.grid_cells.iter().enumerate() {
            let (x, y, z) = (i % w, (i / w) % h, i / w / h);
            let lo = st.world_min + Vector3::new(x as f32, y as f32, z as f32) * st.cell_size;
            for e in &st.grid_entries[c.offset as usize..(c.offset + c.count) as usize] {
                let l = &st.gpu_lights[e.light_index as usize];
                let p = Vector3::new(l.position[0], l.position[1], l.position[2]);
                let near = p.zip_zip_map(&lo, &(lo + Vector3::repeat(st.cell_size)), |v, a, b| v.clamp(a, b));
                assert!((near - p).norm() <= l.direction[3] + 1e-3, "сфера лампы касается ячейки");
            }
        }
    }

    #[test]
    fn sphere_skips_corners_of_its_box() {
        let (mut st, vp) = state(4.0, [100.0, 200.0, 300.0]);
        st.add_light(&lamp(2.0, 2.0, 22.0, 5.0, 12.0));
        st.cull([0.0; 3], &vp, 0.016);
        // габаритный куб 24 м — 7 ячеек на ось, 343 ячейки; шар заметно меньше
        let used = st.grid_cells.iter().filter(|c| c.count > 0).count();
        assert!(used > 100 && used < 300, "ячеек {}", used);
    }

    #[test]
    fn spot_uses_only_cells_of_its_cone() {
        let (mut st, vp) = state(4.0, [100.0, 200.0, 300.0]);
        let down = GPULight { position: [2.0, 22.0, 22.0, 1.0], color: [1.0, 1.0, 1.0, 5.0], direction: [0.0, -1.0, 0.0, 20.0], params: [0.3, 2.0, 0.2, 0.0] };
        st.add_light(&down);
        st.cull([0.0; 3], &vp, 0.016);
        let (w, h) = (st.grid_width as usize, st.grid_height as usize);
        let lamp_y = 22.0;
        for (i, c) in st.grid_cells.iter().enumerate() {
            if c.count > 0 {
                let y = (i / w) % h;
                let top = st.world_min.y + (y + 1) as f32 * st.cell_size;
                assert!(top > lamp_y - 20.5 && st.world_min.y + y as f32 * st.cell_size <= lamp_y + 3.5, "ячейка вне конуса: y {}", y);
            }
        }
        assert!(st.grid_cells.iter().filter(|c| c.count > 0).count() < 120);
    }

    #[test]
    fn removed_slot_is_reused_and_old_id_goes_dark() {
        let (mut st, vp) = state(4.0, [100.0, 200.0, 300.0]);
        let a = st.add_light(&lamp(0.0, 0.0, 10.0, 5.0, 3.0));
        let b = st.add_light(&lamp(1.0, 0.0, 10.0, 6.0, 3.0));
        st.remove_light(a);
        st.remove_light(a);
        assert_eq!(st.live_lights(), 1);
        st.update_light(a, &lamp(0.0, 0.0, 10.0, 9.0, 3.0));
        st.cull([0.0; 3], &vp, 0.016);
        assert_eq!(st.gpu_lights.len(), 1);
        assert_eq!(st.gpu_lights[0].color[3], 6.0);
        let c = st.add_light(&lamp(2.0, 0.0, 10.0, 8.0, 3.0));
        assert_eq!(c, a, "слот занят снова");
        assert_ne!(c, b);
        assert_eq!(st.live_lights(), 2);
        st.update_light(c, &lamp(2.0, 0.0, 10.0, 4.0, 3.0));
        st.cull([0.0; 3], &vp, 0.016);
        assert!(st.gpu_lights.iter().any(|l| l.color[3] == 4.0));
    }

    #[test]
    fn nan_lamp_does_not_panic() {
        let (mut st, vp) = state(4.0, [100.0, 200.0, 300.0]);
        st.add_light(&lamp(f32::NAN, 0.0, 10.0, 5.0, 3.0));
        st.add_light(&lamp(0.0, 0.0, 10.0, 5.0, 3.0));
        st.cull([0.0; 3], &vp, 0.016);
        assert_eq!(st.gpu_lights.len(), 1);
    }
}
