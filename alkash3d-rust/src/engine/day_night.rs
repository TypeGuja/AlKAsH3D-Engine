//! Цикл дня/ночи: положение и цвет "солнца" (directional-свет) плюс
//! обновление управляемых точечных/spot-источников из .alfar (мерцание,
//! включение/выключение по расписанию).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный — тела
//! методов не менялись, только `use`-импорты и видимость `ManagedLight`
//! (`pub(super)`, т.к. на неё ссылается поле `AlkashEngine::managed_lights`,
//! объявленное в родительском модуле `engine`).

use super::AlkashEngine;
use crate::plugin::GPULight;
use crate::math::Vec3;

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
pub(super) struct ManagedLight {
    /// id, под которым свет живёт внутри FirstFires (аргумент id для
    /// `LightPlugin::update_light`).
    pub(super) firstfires_id: u32,
    /// Статическая (не меняющаяся во время работы) часть GPULight —
    /// пересобирается каждый кадр из этих полей + промодулированного
    /// intensity/enabled.
    pub(super) position: [f32; 3],
    pub(super) light_type: f32,
    pub(super) color: [f32; 3],
    pub(super) base_intensity: f32,
    pub(super) direction: [f32; 3],
    pub(super) range: f32,
    pub(super) params: [f32; 4],
    pub(super) flicker_enabled: bool,
    pub(super) flicker_speed: f32,
    pub(super) flicker_intensity: f32,
    pub(super) active_from: f32,
    pub(super) active_to: f32,
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
            hour >= self.active_from || hour < self.active_to
        }
    }
}

impl AlkashEngine {
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
    pub(super) fn update_day_night(&mut self, dt: f32) {
        self.time_of_day = (self.time_of_day + self.day_night_speed * dt).rem_euclid(24.0);

        let sun = Self::compute_sun_state(self.time_of_day);
        self.transform_constants.light_dir = [sun.direction.x, sun.direction.y, sun.direction.z, 0.0];
        self.transform_constants.light_color = [sun.color[0], sun.color[1], sun.color[2], sun.intensity];
        self.transform_constants.ambient_color = [sun.ambient[0], sun.ambient[1], sun.ambient[2], 1.0];

        if self.managed_lights.is_empty() {
            return;
        }

        let hour = self.time_of_day;
        for (i, managed) in self.managed_lights.iter().enumerate() {
            let active = managed.is_active_at(hour);

            let mut intensity = if active { managed.base_intensity } else { 0.0 };

            if active && managed.flicker_enabled {
                let phase = self.flicker_phase[i];
                let noise = 0.6 * (phase).sin() + 0.4 * (phase * 2.7).sin();
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
            return SunState {
                direction: Vec3::new(0.0, -1.0, 0.0),
                color: [0.6, 0.7, 1.0],
                intensity: 0.0,
                ambient: [0.02, 0.02, 0.05],
            };
        }

        let day_t = (hour - SUNRISE) / (SUNSET - SUNRISE);
        let elevation = (day_t * std::f32::consts::PI).sin().max(0.0);
        let elevation_angle = elevation * std::f32::consts::FRAC_PI_2;

        let azimuth = day_t * std::f32::consts::PI;

        let sun_pos_dir = Vec3::new(
            -azimuth.cos(),
            elevation_angle.sin(),
            0.3,
        ).normalize();
        let direction = -sun_pos_dir;

        let horizon_color = Vec3::new(1.0, 0.45, 0.15);
        let noon_color = Vec3::new(1.0, 0.97, 0.92);
        let color_t = elevation;
        let color = horizon_color.lerp(noon_color, color_t);

        let intensity = 0.15 + 0.85 * elevation;

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
}
