//! ДОБАВЛЕНО (кривая силы света): как светильник распределяет свет по углу от
//! оси — вместо ровного конуса. Хранится таблицей по углу 0..180° (одна
//! вертикальная кривая: светильник считается осесимметричным, разница между
//! плоскостями файла IES усредняется). Прожектор ссылается на кривую через
//! `GPULight.params[1]` (см. `spot_params_y`).

/// Отсчётов на кривую, через равные углы от 0° (по оси) до 180°.
pub const PROFILE_SAMPLES: usize = 33;
/// Сколько кривых можно задать одновременно.
pub const MAX_PROFILES: usize = 4;
/// Строк float4 в константах шейдера: по 9 на кривую (33 отсчёта и запас).
pub(super) const PROFILE_ROWS: usize = 9 * MAX_PROFILES;

/// Таблица кривых, как `lightProfiles` в шейдерах.
pub(super) type ProfileTable = [[f32; 4]; PROFILE_ROWS];

/// Кривая силы света, максимум нормирован на 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightProfile {
    pub samples: [f32; PROFILE_SAMPLES],
}

impl LightProfile {
    /// Из точек (угол от оси в градусах по возрастанию, относительная сила).
    /// Между точками — линейно, за пределами — значение крайней точки.
    pub fn from_points(points: &[(f32, f32)]) -> Result<Self, String> {
        if points.is_empty() {
            return Err("кривая без точек".into());
        }
        if points.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err("углы кривой должны возрастать".into());
        }
        let peak = points.iter().map(|p| p.1).fold(0.0f32, f32::max);
        if !(peak > 0.0) || points.iter().any(|p| p.1 < 0.0 || !p.1.is_finite()) {
            return Err("сила света кривой должна быть неотрицательной и не вся нулевая".into());
        }
        let mut samples = [0.0; PROFILE_SAMPLES];
        for (k, out) in samples.iter_mut().enumerate() {
            let angle = k as f32 * 180.0 / (PROFILE_SAMPLES - 1) as f32;
            let i = points.partition_point(|p| p.0 <= angle);
            let value = if i == 0 {
                points[0].1
            } else if i == points.len() {
                points[i - 1].1
            } else {
                let (a, b) = (points[i - 1], points[i]);
                a.1 + (b.1 - a.1) * (angle - a.0) / (b.0 - a.0)
            };
            *out = value / peak;
        }
        Ok(LightProfile { samples })
    }

    /// Из текста файла IES (LM-63, фотометрия типа C): плоскости усредняются.
    pub fn from_ies(text: &str) -> Result<Self, String> {
        let tilt = text.find("TILT=").ok_or("IES: нет строки TILT")?;
        let line_end = text[tilt..].find('\n').map(|n| tilt + n).ok_or("IES: файл обрывается после TILT")?;
        if text[tilt + 5..line_end].trim() != "NONE" {
            return Err("IES: поддерживается только TILT=NONE".into());
        }
        let numbers = text[line_end..]
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f32>().map_err(|_| format!("IES: не число: {}", s)))
            .collect::<Result<Vec<f32>, String>>()?;
        if numbers.len() < 13 {
            return Err("IES: мало чисел в заголовке".into());
        }
        let (vertical, horizontal) = (numbers[3] as usize, numbers[4] as usize);
        if numbers[5] != 1.0 {
            return Err(format!("IES: поддерживается только фотометрия типа C, в файле {}", numbers[5]));
        }
        if vertical == 0 || horizontal == 0 {
            return Err("IES: нет углов".into());
        }
        let body = &numbers[13..];
        if body.len() < vertical + horizontal + vertical * horizontal {
            return Err("IES: чисел меньше, чем объявлено".into());
        }
        let angles = &body[..vertical];
        let candela = &body[vertical + horizontal..vertical + horizontal + vertical * horizontal];
        let points: Vec<(f32, f32)> = angles
            .iter()
            .enumerate()
            .map(|(v, &a)| (a, (0..horizontal).map(|h| candela[h * vertical + v]).sum::<f32>() / horizontal as f32))
            .collect();
        Self::from_points(&points)
    }
}

/// Записать кривую номер `id` (с 1) в таблицу шейдера.
pub(super) fn write_profile(table: &mut ProfileTable, id: usize, profile: &LightProfile) {
    let row = (id - 1) * PROFILE_ROWS / MAX_PROFILES;
    for (k, &v) in profile.samples.iter().enumerate() {
        table[row + k / 4][k % 4] = v;
    }
}

/// `GPULight.params[1]` прожектора: минус (номер кривой + радиус светящегося
/// тела в метрах). Номер 0 — без кривой; радиус меньше 1 м.
pub fn spot_params_y(profile_id: u32, emitter_radius: f32) -> f32 {
    -(profile_id as f32 + emitter_radius.clamp(0.0, 0.5))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_points_normalizes_and_interpolates() {
        let p = LightProfile::from_points(&[(0.0, 1.0), (90.0, 2.0), (180.0, 0.0)]).unwrap();
        assert!((p.samples[0] - 0.5).abs() < 1e-5);
        assert!((p.samples[16] - 1.0).abs() < 1e-5);
        assert!((p.samples[8] - 0.75).abs() < 1e-5);
        assert_eq!(p.samples[32], 0.0);
    }

    #[test]
    fn from_points_rejects_bad_input() {
        assert!(LightProfile::from_points(&[]).is_err());
        assert!(LightProfile::from_points(&[(10.0, 1.0), (5.0, 1.0)]).is_err());
        assert!(LightProfile::from_points(&[(0.0, 0.0), (5.0, 0.0)]).is_err());
    }

    #[test]
    fn from_ies_averages_planes() {
        let text = "IESNA:LM-63-2002\n[TEST] x\nTILT=NONE\n1 1000 1 3 2 1 2 0 0 0\n1 1 100\n0 45 90\n0 90\n100 200 0\n300 400 0\n";
        let p = LightProfile::from_ies(text).unwrap();
        // среднее плоскостей: 0° — 200, 45° — 300, 90° — 0
        assert!((p.samples[0] - 200.0 / 300.0).abs() < 1e-5);
        assert!((p.samples[8] - 1.0).abs() < 1e-5);
        assert_eq!(p.samples[16], 0.0);
    }

    #[test]
    fn from_ies_rejects_other_photometry_and_tilt() {
        assert!(LightProfile::from_ies("TILT=INCLUDE\n1 2 3").is_err());
        assert!(LightProfile::from_ies("TILT=NONE\n1 1000 1 1 1 2 2 0 0 0\n1 1 100\n0\n0\n100\n").is_err());
    }

    #[test]
    fn spot_params_round_trip() {
        let y = spot_params_y(2, 0.15);
        assert_eq!((-y).floor(), 2.0);
        assert!(((-y).fract() - 0.15).abs() < 1e-5);
        assert_eq!(spot_params_y(0, 0.0), 0.0);
    }
}
