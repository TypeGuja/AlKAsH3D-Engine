// src/math/transform.rs

use super::vec3::Vec3;
use super::quat::Quat;

/// Трансформация в 3D пространстве (позиция, вращение, масштаб)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    /// Создаёт единичную трансформацию (без изменений)
    pub const fn identity() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Создаёт трансформацию только с позицией
    #[inline]
    pub fn from_position(position: Vec3) -> Self {
        Self {
            position,
            ..Self::identity()
        }
    }

    /// Создаёт трансформацию только с вращением
    #[inline]
    pub fn from_rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            ..Self::identity()
        }
    }

    /// Создаёт трансформацию только с масштабом
    #[inline]
    pub fn from_scale(scale: Vec3) -> Self {
        Self {
            scale,
            ..Self::identity()
        }
    }

    /// Преобразует в матрицу 4x4
    #[inline]
    pub fn to_matrix(&self) -> [[f32; 4]; 4] {
        let rot_mat = self.rotation.to_mat3();
        [
            [rot_mat[0][0] * self.scale.x, rot_mat[0][1] * self.scale.x, rot_mat[0][2] * self.scale.x, self.position.x],
            [rot_mat[1][0] * self.scale.y, rot_mat[1][1] * self.scale.y, rot_mat[1][2] * self.scale.y, self.position.y],
            [rot_mat[2][0] * self.scale.z, rot_mat[2][1] * self.scale.z, rot_mat[2][2] * self.scale.z, self.position.z],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    /// Направление вперёд
    #[inline]
    pub fn forward(&self) -> Vec3 {
        self.rotation.forward()
    }

    /// Направление вправо
    #[inline]
    pub fn right(&self) -> Vec3 {
        self.rotation.right()
    }

    /// Направление вверх
    #[inline]
    pub fn up(&self) -> Vec3 {
        self.rotation.up()
    }

    /// Применяет трансформацию к точке
    #[inline]
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        let scaled = Vec3::new(
            point.x * self.scale.x,
            point.y * self.scale.y,
            point.z * self.scale.z,
        );
        self.rotation.rotate(scaled) + self.position
    }

    /// Применяет трансформацию к направлению (без позиции)
    #[inline]
    pub fn transform_direction(&self, dir: Vec3) -> Vec3 {
        self.rotation.rotate(dir)
    }

    /// Композиция двух трансформаций
    #[inline]
    pub fn compose(&self, other: &Transform) -> Transform {
        // Масштабируем позицию другой трансформации
        let scaled_pos = Vec3::new(
            other.position.x * self.scale.x,
            other.position.y * self.scale.y,
            other.position.z * self.scale.z,
        );

        // Вращаем и добавляем к своей позиции
        let rotated_pos = self.rotation.rotate(scaled_pos);

        Transform {
            position: Vec3::new(
                self.position.x + rotated_pos.x,
                self.position.y + rotated_pos.y,
                self.position.z + rotated_pos.z,
            ),
            rotation: self.rotation.mul(&other.rotation),
            scale: Vec3::new(
                self.scale.x * other.scale.x,
                self.scale.y * other.scale.y,
                self.scale.z * other.scale.z,
            ),
        }
    }
    /// Обратная трансформация
    #[inline]
    pub fn inverse(&self) -> Transform {
        let inv_rot = self.rotation.inverse();
        let inv_scale = Vec3::new(
            1.0 / self.scale.x,
            1.0 / self.scale.y,
            1.0 / self.scale.z,
        );

        // Инвертированная позиция: -inv_rot(pos / scale)
        let neg_pos = Vec3::new(-self.position.x, -self.position.y, -self.position.z);
        let inv_pos = inv_rot.rotate(neg_pos);

        Transform {
            position: Vec3::new(
                inv_pos.x * inv_scale.x,
                inv_pos.y * inv_scale.y,
                inv_pos.z * inv_scale.z,
            ),
            rotation: inv_rot,
            scale: inv_scale,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

// ============================================================
// Форматирование
// ============================================================

impl std::fmt::Display for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "T(pos={}, rot={}, scale={})", self.position, self.rotation, self.scale)
    }
}

// ============================================================
// Тесты
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity() {
        let t = Transform::identity();
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(t.transform_point(p), p);
    }

    #[test]
    fn test_position() {
        let t = Transform::from_position(Vec3::new(10.0, 0.0, 0.0));
        let p = Vec3::ZERO;
        assert_eq!(t.transform_point(p), Vec3::new(10.0, 0.0, 0.0));
    }

    #[test]
    fn test_rotation() {
        let t = Transform::from_rotation(
            Quat::from_axis_angle(Vec3::UP, std::f32::consts::FRAC_PI_2)
        );
        let fwd = t.forward();
        assert!((fwd.x - 1.0).abs() < 0.001);
        assert!((fwd.z).abs() < 0.001);
    }

    #[test]
    fn test_compose() {
        let t1 = Transform::from_position(Vec3::new(1.0, 0.0, 0.0));
        let t2 = Transform::from_position(Vec3::new(0.0, 1.0, 0.0));
        let composed = t1.compose(&t2);
        assert_eq!(composed.position, Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn test_inverse() {
        let t = Transform::from_position(Vec3::new(5.0, 0.0, 0.0));
        let inv = t.inverse();
        let p = Vec3::new(1.0, 2.0, 3.0);
        let transformed = t.transform_point(p);
        let back = inv.transform_point(transformed);
        assert!((back.x - p.x).abs() < 0.001);
        assert!((back.y - p.y).abs() < 0.001);
        assert!((back.z - p.z).abs() < 0.001);
    }
}