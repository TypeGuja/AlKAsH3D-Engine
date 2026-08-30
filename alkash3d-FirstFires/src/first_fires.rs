// ============================================================================
// АРХИВ / НЕ ИСПОЛЬЗУЕТСЯ (найдено при аудите кодовой базы)
//
// Этот файл НЕ подключён к сборке крейта — в src/lib.rs объявлены только
// `mod light; mod culling; mod grid; mod stats;`, `mod first_fires;`
// отсутствует. cargo build его не видит и не компилирует вообще.
//
// ВАЖНО: код ниже содержит УСТАРЕВШИЙ, уже исправленный в другом месте
// баг — метод `cull()` в этом файле регистрирует источник света только в
// ОДНОЙ ячейке сетки по его позиции (`self.light_grid.get_cell_index(
// light.position)`), что при большом радиусе действия света даёт резкие
// уступы освещённости на границах ячеек. В реально используемом коде
// (src/lib.rs, LightState::cull) этот баг уже исправлен —
// get_cell_indices_for_sphere регистрирует свет во ВСЕХ ячейках, которые
// пересекает его bounding sphere, а не только в одной.
//
// Если этот файл когда-нибудь понадобится как основа для нового кода —
// НЕ копируйте culling-логику отсюда напрямую, возьмите её из
// src/lib.rs (актуальная, исправленная версия). Безопаснее всего —
// удалить этот файл полностью, если он больше не нужен как референс.
// ============================================================================

use nalgebra::{Vector3, Matrix4};
use rayon::prelude::*;
use std::time::Instant;

use crate::{
    Light, LightType, LightGrid, CullingStats,
    GPULight,
};
use crate::culling::{Frustum, Culler};

#[derive(Debug, Clone, Copy)]
pub struct FirstFiresConfig {
    pub max_lights: u32,
    pub max_lights_per_tile: u32,
    pub tile_size: u32,
    pub far_plane: f32,
    pub lod_distances: [f32; 3],
    pub cell_size: f32,
}

impl Default for FirstFiresConfig {
    fn default() -> Self {
        Self {
            max_lights: 4096,
            max_lights_per_tile: 64,
            tile_size: 16,
            far_plane: 1000.0,
            lod_distances: [50.0, 150.0, 300.0],
            cell_size: 32.0,
        }
    }
}

pub struct FirstFiresSystem {
    config: FirstFiresConfig,
    lights: Vec<Light>,
    gpu_lights: Vec<GPULight>,
    light_grid: LightGrid,
    culler: Culler,
    stats: CullingStats,
    frame_counter: u64,
}

impl FirstFiresSystem {
    pub fn new(config: FirstFiresConfig) -> Self {
        let world_size = config.far_plane;
        let world_min = Vector3::new(-world_size, -world_size, -world_size);
        let world_max = Vector3::new(world_size, world_size, world_size);

        Self {
            config,
            lights: Vec::with_capacity(config.max_lights as usize),
            gpu_lights: Vec::with_capacity(config.max_lights as usize),
            light_grid: LightGrid::new(world_min, world_max, config.cell_size),
            culler: Culler::new(config.lod_distances),
            stats: CullingStats::new(),
            frame_counter: 0,
        }
    }

    pub fn add_light(&mut self, light: Light) -> u32 {
        let id = self.lights.len() as u32;
        let mut light = light;
        light.id = id;
        self.lights.push(light);
        id
    }

    pub fn add_street_light(&mut self, x: f32, y: f32, z: f32) -> u32 {
        self.add_light(Light::point(
            Vector3::new(x, y, z),
            Vector3::new(1.0, 0.85, 0.6),
            2.5,
            25.0,
        ))
    }

    pub fn add_street_lights_circle(
        &mut self,
        center: Vector3<f32>,
        radius: f32,
        count: usize,
        height: f32,
    ) -> Vec<u32> {
        let mut ids = Vec::with_capacity(count);
        for i in 0..count {
            let angle = 2.0 * std::f32::consts::PI * (i as f32) / (count as f32);
            let x = center.x + radius * angle.cos();
            let z = center.z + radius * angle.sin();
            ids.push(self.add_street_light(x, height, z));
        }
        ids
    }

    pub fn add_street_lights_grid(
        &mut self,
        min_x: f32, min_z: f32,
        max_x: f32, max_z: f32,
        spacing: f32,
        height: f32,
    ) -> Vec<u32> {
        let mut ids = Vec::new();
        let mut x = min_x;
        while x <= max_x {
            let mut z = min_z;
            while z <= max_z {
                ids.push(self.add_street_light(x, height, z));
                z += spacing;
            }
            x += spacing;
        }
        ids
    }

    pub fn add_street_lights_line(
        &mut self,
        start: Vector3<f32>,
        end: Vector3<f32>,
        spacing: f32,
    ) -> Vec<u32> {
        let mut ids = Vec::new();
        let dir = end - start;
        let len = dir.magnitude();
        let steps = (len / spacing) as i32;

        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let pos = start + dir * t;
            ids.push(self.add_street_light(pos.x, start.y, pos.z));
        }

        ids
    }

    pub fn cull(
        &mut self,
        camera_pos: Vector3<f32>,
        view_proj: &Matrix4<f32>,
        _dt: f32,
    ) -> (&[GPULight], &LightGrid) {
        let start = Instant::now();
        self.frame_counter += 1;
        self.stats.reset();
        self.stats.total_lights = self.lights.len() as u32;

        self.gpu_lights.clear();
        self.light_grid.clear();

        let frustum = Frustum::from_view_proj(view_proj);

        let mut culled_lod = 0u32;
        let mut culled_dist = 0u32;
        let mut culled_frustum = 0u32;

        // ИСПРАВЛЕНО (баг: фонари резко гаснут/загораются при ходьбе):
        // раньше здесь был ДОПОЛНИТЕЛЬНЫЙ тест `distance > light.range *
        // 1.2`, отдельный от LOD-дистанции. `light.range` — радиус
        // ВЛИЯНИЯ СВЕТА НА ОСВЕЩАЕМУЮ ИМ ПОВЕРХНОСТЬ (уже корректно учтён
        // в пиксельном шейдере через windowFalloff, см.
        // ComputePointLightContribution в engine/mod.rs) — он НЕ имеет
        // отношения к тому, виден ли сам ИСТОЧНИК СВЕТА камерой. Смешивать
        // эти два понятия было ошибкой: для типичного уличного фонаря
        // (range=15м) порог `range*1.2`=18м оказывался ГОРАЗДО жёстче,
        // чем LOD/frustum-дистанции (обычно 30-100+м) — фонарь, который
        // физически ещё освещает видимую в кадре геометрию (стены, пол
        // рядом с камерой), резко пропадал из списка видимых источников
        // уже на 18 метрах, что и ощущалось как "свет то есть, то резко
        // гаснет" при обычной ходьбе вдоль улицы. LOD-дистанция
        // (`lod_distances`, настраивается через LightConfig, см.
        // `AlkashEngine::init_lights`) и frustum test — уже достаточные и
        // физически осмысленные критерии "не считать вклад этого света в
        // данном кадре"; отдельный жёсткий cutoff по `range` только
        // дублировал их менее подходящим порогом.
        let visible: Vec<(usize, i32, f32)> = self.lights
            .par_iter()
            .enumerate()
            .filter_map(|(idx, light)| {
                let distance = (light.position - camera_pos).magnitude();

                let lod = self.culler.get_lod_level(distance);
                if lod < 0 {
                    return None;
                }

                if !frustum.test_sphere(light.position, light.range) {
                    return None;
                }

                Some((idx, lod, distance))
            })
            .collect();

        // Подсчёт статистики
        for light in &self.lights {
            let distance = (light.position - camera_pos).magnitude();
            if self.culler.get_lod_level(distance) < 0 {
                culled_lod += 1;
            } else if !frustum.test_sphere(light.position, light.range) {
                culled_frustum += 1;
            }
        }

        self.stats.culled_by_lod = culled_lod;
        // ИСПРАВЛЕНО: `culled_by_distance` больше не считается отдельно от
        // LOD (см. подробный комментарий выше у убранного теста `distance
        // > light.range * 1.2`) — всегда 0. Поле оставлено в
        // `CullingStats` ради стабильности публичного ABI плагина
        // (`get_stats`/`CullingStats` в lib.rs), а не удалено.
        self.stats.culled_by_distance = culled_dist;
        self.stats.culled_by_frustum = culled_frustum;

        let mut visible_sorted = visible;
        visible_sorted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());

        for (idx, lod, depth) in visible_sorted {
            let light = &self.lights[idx];
            let light_idx = self.gpu_lights.len() as u32;
            self.gpu_lights.push(light.gpu_pack());

            if let Some(cell_idx) = self.light_grid.get_cell_index(light.position) {
                self.light_grid.add_light(light_idx, lod as u32, depth, cell_idx);
            }
        }

        self.stats.visible_lights = self.gpu_lights.len() as u32;

        let non_empty = self.light_grid.cells.iter().filter(|c| c.count > 0).count();
        if non_empty > 0 {
            self.stats.avg_lights_per_tile = self.light_grid.entries.len() as f32 / non_empty as f32;
        }

        self.stats.culling_time_ms = start.elapsed().as_secs_f32() * 1000.0;

        (&self.gpu_lights, &self.light_grid)
    }

    pub fn get_stats(&self) -> &CullingStats {
        &self.stats
    }

    pub fn get_lights(&self) -> &[Light] {
        &self.lights
    }
}