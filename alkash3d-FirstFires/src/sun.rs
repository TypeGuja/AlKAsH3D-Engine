//! Параметры направленного солнечного света для хоста FirstFires.
//! Атмосфера задаёт спектральную освещённость; плагин готовит её для
//! PBR-шейдера.

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SunFrame {
    /// Направление распространения лучей, xyz.
    pub incoming: [f32; 4],
    /// Нормированный цвет и интенсивность прямого света.
    pub direct: [f32; 4],
    /// Рассеянное освещение неба и земли.
    pub ambient: [f32; 4],
}

/// Стабильная C-точка входа DLL для хостов без Rust-зависимости.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SunInput {
    pub toward_sun: [f32; 3],
    pub radiance: [f32; 3],
    pub ambient: [f32; 3],
}

#[no_mangle]
pub extern "C" fn firstfires_prepare_sun(input: SunInput) -> SunFrame {
    SunFrame::from_atmosphere(input.toward_sun, input.radiance, input.ambient)
}

impl SunFrame {
    pub fn from_atmosphere(
        toward_sun: [f32; 3],
        radiance: [f32; 3],
        ambient: [f32; 3],
    ) -> Self {
        let len = toward_sun.iter().map(|v| v * v).sum::<f32>().sqrt();
        let direction = if len > 1e-6 {
            toward_sun.map(|v| -v / len)
        } else {
            [0.0, -1.0, 0.0]
        };
        let sun = radiance.map(|v| v.max(0.0));
        let peak = sun.iter().copied().fold(0.0, f32::max);
        let direct = if peak > 1e-6 {
            [sun[0] / peak, sun[1] / peak, sun[2] / peak, peak]
        } else {
            [1.0, 1.0, 1.0, 0.0]
        };
        Self {
            incoming: [direction[0], direction[1], direction[2], 0.0],
            direct,
            ambient: [ambient[0].max(0.0), ambient[1].max(0.0), ambient[2].max(0.0), 1.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SunFrame;

    #[test]
    fn preserves_solar_spectrum_and_direction() {
        let frame = SunFrame::from_atmosphere([0.0, 2.0, 0.0], [2.0, 1.5, 1.0], [0.2; 3]);
        assert_eq!(frame.incoming, [0.0, -1.0, 0.0, 0.0]);
        assert_eq!(frame.direct, [1.0, 0.75, 0.5, 2.0]);
    }

    #[test]
    fn night_has_no_direct_sun() {
        let frame = SunFrame::from_atmosphere([0.0, -1.0, 0.0], [0.0; 3], [0.01; 3]);
        assert_eq!(frame.direct[3], 0.0);
        assert_eq!(frame.ambient[3], 1.0);
    }
}
