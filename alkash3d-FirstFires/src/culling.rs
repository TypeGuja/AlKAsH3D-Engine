use nalgebra::{Vector3, Matrix4};

#[derive(Debug, Clone)]
pub struct Frustum {
    pub planes: [Vector3<f32>; 6],
    pub distances: [f32; 6],
}

impl Frustum {
    pub fn from_view_proj(view_proj: &Matrix4<f32>) -> Self {
        let mut planes = [Vector3::zeros(); 6];
        let mut distances = [0.0; 6];

        let m = view_proj.as_slice();

        // Left
        planes[0] = Vector3::new(m[3] + m[0], m[7] + m[4], m[11] + m[8]);
        distances[0] = m[15] + m[12];

        // Right
        planes[1] = Vector3::new(m[3] - m[0], m[7] - m[4], m[11] - m[8]);
        distances[1] = m[15] - m[12];

        // Bottom
        planes[2] = Vector3::new(m[3] + m[1], m[7] + m[5], m[11] + m[9]);
        distances[2] = m[15] + m[13];

        // Top
        planes[3] = Vector3::new(m[3] - m[1], m[7] - m[5], m[11] - m[9]);
        distances[3] = m[15] - m[13];

        // Near
        planes[4] = Vector3::new(m[3] + m[2], m[7] + m[6], m[11] + m[10]);
        distances[4] = m[15] + m[14];

        // Far
        planes[5] = Vector3::new(m[3] - m[2], m[7] - m[6], m[11] - m[10]);
        distances[5] = m[15] - m[14];

        // Normalize
        for i in 0..6 {
            let len = planes[i].norm();
            if len > 0.0 {
                planes[i] /= len;
                distances[i] /= len;
            }
        }

        Self { planes, distances }
    }

    /// Frustum из плоского массива 16 float в порядке ПО СТОЛБЦАМ — так
    /// матрицу отдаёт движок (`glam::Mat4::to_cols_array`, см.
    /// alkash3d-rust/src/bin/*.rs -> `engine.update(.., view_proj)`).
    /// ИСПРАВЛЕНО (баг: "фонари горят только под определённым углом
    /// камеры"): C-ABI путь (`lib.rs::cull`) раньше собирал матрицу через
    /// `Matrix4::new(...)`, а он принимает элементы ПОСТРОЧНО — матрица
    /// выходила транспонированной, плоскости frustum получались неверными,
    /// и `test_sphere` выбрасывал видимые фонари в зависимости от поворота
    /// камеры.
    pub fn from_column_major(view_proj: &[f32; 16]) -> Self {
        Self::from_view_proj(&Matrix4::from_column_slice(view_proj))
    }

    #[inline]
    pub fn test_sphere(&self, center: Vector3<f32>, radius: f32) -> bool {
        for i in 0..6 {
            let dist = self.planes[i].dot(&center) + self.distances[i];
            if dist < -radius {
                return false;
            }
        }
        true
    }
}

pub struct Culler {
    pub lod_distances: [f32; 3],
}

impl Culler {
    pub fn new(lod_distances: [f32; 3]) -> Self {
        Self { lod_distances }
    }

    #[inline]
    pub fn get_lod_level(&self, distance: f32) -> i32 {
        if distance < self.lod_distances[0] {
            0
        } else if distance < self.lod_distances[1] {
            1
        } else if distance < self.lod_distances[2] {
            2
        } else {
            -1
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Point3;

    /// Камера в начале координат смотрит в разные стороны — сфера перед
    /// камерой должна проходить тест, сфера за спиной/сбоку — нет, при
    /// ЛЮБОМ направлении взгляда (раньше из-за транспонированной матрицы
    /// результат зависел от угла камеры).
    #[test]
    fn column_major_frustum_follows_camera_direction() {
        let proj = Matrix4::new_perspective(16.0 / 9.0, 60f32.to_radians(), 0.1, 1000.0);
        for (dir, side) in [
            (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            (Vector3::new(-1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)),
            (Vector3::new(0.0, 0.0, -1.0), Vector3::new(1.0, 0.0, 0.0)),
            (Vector3::new(0.7071, 0.0, 0.7071), Vector3::new(0.7071, 0.0, -0.7071)),
        ] {
            let view = Matrix4::look_at_rh(&Point3::origin(), &Point3::from(dir), &Vector3::y());
            let vp = proj * view;
            let mut arr = [0.0f32; 16];
            arr.copy_from_slice(vp.as_slice()); // nalgebra хранит по столбцам — как glam::to_cols_array
            let f = Frustum::from_column_major(&arr);
            assert!(f.test_sphere(dir * 30.0 + Vector3::new(0.0, 9.0, 0.0), 1.0), "фонарь впереди при взгляде {:?}", dir);
            assert!(!f.test_sphere(-dir * 30.0, 1.0), "сфера за спиной при взгляде {:?}", dir);
            assert!(!f.test_sphere(side * 200.0, 1.0), "сфера далеко сбоку при взгляде {:?}", dir);
        }
    }
}
