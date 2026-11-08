//! Универсальный локальный источник света для ламп, сигналов и приборов.

use crate::GPULight;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PointLamp {
    pub position: [f32; 3],
    pub color: [f32; 3],
    /// Интенсивность в единицах хоста (для маршрута: кд · π/128000).
    pub intensity: f32,
    pub range: f32,
    pub cast_shadows: u32,
}

#[no_mangle]
pub extern "C" fn firstfires_point_lamp(lamp: PointLamp) -> GPULight {
    GPULight {
        position: [lamp.position[0], lamp.position[1], lamp.position[2], 0.0],
        color: [lamp.color[0], lamp.color[1], lamp.color[2], lamp.intensity.max(0.0)],
        direction: [0.0, -1.0, 0.0, lamp.range.max(0.0)],
        params: [std::f32::consts::PI, 2.0, 0.0,
            if lamp.cast_shadows != 0 { 0.0 } else { -0.25 }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_lamp_can_skip_shadow_map() {
        let lamp = firstfires_point_lamp(PointLamp {
            position: [1.0, 2.0, 3.0], color: [1.0, 0.0, 0.0],
            intensity: 4.0, range: 12.0, cast_shadows: 0,
        });
        assert_eq!(lamp.position, [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(lamp.params[3], -0.25);
        assert_eq!(lamp.direction[3], 12.0);
    }
}
