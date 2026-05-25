#[derive(Debug, Default, Clone, Copy)]
pub struct CullingStats {
    pub total_lights: u32,
    pub visible_lights: u32,
    pub culled_by_lod: u32,
    pub culled_by_distance: u32,
    pub culled_by_frustum: u32,
    pub culled_by_occlusion: u32,
    pub avg_lights_per_tile: f32,
    pub culling_time_ms: f32,
    pub grid_time_ms: f32,
}

impl CullingStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn culling_efficiency(&self) -> f32 {
        if self.total_lights == 0 {
            0.0
        } else {
            self.visible_lights as f32 / self.total_lights as f32
        }
    }
}