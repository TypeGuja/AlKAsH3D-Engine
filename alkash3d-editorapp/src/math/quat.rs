// src/math/quat.rs

use super::vec3::Vec3;

/// Кватернион для представления вращения в 3D пространстве
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    /// Единичный кватернион (без вращения)
    pub const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    /// Создаёт кватернион из оси вращения и угла
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let half = angle * 0.5;
        let s = half.sin();
        Self {
            x: axis.x * s,
            y: axis.y * s,
            z: axis.z * s,
            w: half.cos(),
        }
    }

    /// Создаёт кватернион из углов Эйлера (XYZ)
    #[inline]
    pub fn from_euler(x: f32, y: f32, z: f32) -> Self {
        let (sx, cx) = (x * 0.5).sin_cos();
        let (sy, cy) = (y * 0.5).sin_cos();
        let (sz, cz) = (z * 0.5).sin_cos();

        Self {
            x: sx * cy * cz - cx * sy * sz,
            y: cx * sy * cz + sx * cy * sz,
            z: cx * cy * sz - sx * sy * cz,
            w: cx * cy * cz + sx * sy * sz,
        }
    }

    /// Умножение кватернионов (композиция вращений)
    #[inline]
    pub fn mul(&self, other: &Quat) -> Quat {
        Quat {
            x: self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            y: self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            z: self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            w: self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        }
    }

    /// Сферическая линейная интерполяция (slerp)
    #[inline]
    pub fn slerp(&self, other: &Quat, t: f32) -> Quat {
        let cos_half_theta = self.dot(other).clamp(-1.0, 1.0);

        if cos_half_theta.abs() > 0.9995 {
            // Линейная интерполяция для малых углов
            return Quat {
                x: self.x + (other.x - self.x) * t,
                y: self.y + (other.y - self.y) * t,
                z: self.z + (other.z - self.z) * t,
                w: self.w + (other.w - self.w) * t,
            }.normalize();
        }

        let half_theta = cos_half_theta.acos();
        let sin_half_theta = (1.0 - cos_half_theta * cos_half_theta).sqrt();

        let a = ((1.0 - t) * half_theta).sin() / sin_half_theta;
        let b = (t * half_theta).sin() / sin_half_theta;

        Quat {
            x: self.x * a + other.x * b,
            y: self.y * a + other.y * b,
            z: self.z * a + other.z * b,
            w: self.w * a + other.w * b,
        }
    }

    /// Скалярное произведение кватернионов
    #[inline]
    pub fn dot(&self, other: &Quat) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Нормализация кватерниона
    #[inline]
    pub fn normalize(&self) -> Quat {
        let len = self.length();
        if len > 0.0001 {
            Quat {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
                w: self.w / len,
            }
        } else {
            Quat::IDENTITY
        }
    }

    /// Длина кватерниона
    #[inline]
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    /// Применяет вращение к вектору
    #[inline]
    pub fn rotate(&self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let s = self.w;

        // Оптимизированная формула: v' = 2(u·v)u + (s² - u·u)v + 2s(u × v)
        let dot_uv = u.dot(v);
        let dot_uu = u.dot(u);

        u * (2.0 * dot_uv) + v * (s * s - dot_uu) + u.cross(v) * (2.0 * s)
    }

    /// Преобразует кватернион в матрицу 3x3
    #[inline]
    pub fn to_mat3(&self) -> [[f32; 3]; 3] {
        let x = self.x;
        let y = self.y;
        let z = self.z;
        let w = self.w;

        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        [
            [1.0 - (yy + zz), xy + wz, xz - wy],
            [xy - wz, 1.0 - (xx + zz), yz + wx],
            [xz + wy, yz - wx, 1.0 - (xx + yy)],
        ]
    }

    /// Преобразует в углы Эйлера (XYZ)
    #[inline]
    pub fn to_euler(&self) -> Vec3 {
        let sin_pitch = 2.0 * (self.w * self.x - self.y * self.z);
        let pitch = if sin_pitch.abs() > 0.999 {
            std::f32::consts::FRAC_PI_2 * sin_pitch.signum()
        } else {
            sin_pitch.asin()
        };

        let yaw = (2.0 * (self.w * self.y + self.z * self.x))
            .atan2(1.0 - 2.0 * (self.x * self.x + self.y * self.y));

        let roll = (2.0 * (self.w * self.z + self.x * self.y))
            .atan2(1.0 - 2.0 * (self.z * self.z + self.x * self.x));

        Vec3::new(pitch, yaw, roll)
    }

    /// Сопряжённый кватернион (инверсное вращение)
    #[inline]
    pub fn conjugate(&self) -> Quat {
        Quat {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    /// Обратный кватернион
    #[inline]
    pub fn inverse(&self) -> Quat {
        let len_sq = self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w;
        if len_sq < 0.0001 {
            Quat::IDENTITY
        } else {
            let inv_len_sq = 1.0 / len_sq;
            Quat {
                x: -self.x * inv_len_sq,
                y: -self.y * inv_len_sq,
                z: -self.z * inv_len_sq,
                w: self.w * inv_len_sq,
            }
        }
    }

    /// Направление вперёд относительно вращения
    #[inline]
    pub fn forward(&self) -> Vec3 {
        self.rotate(Vec3::FORWARD)
    }

    /// Направление вверх относительно вращения
    #[inline]
    pub fn up(&self) -> Vec3 {
        self.rotate(Vec3::UP)
    }

    /// Направление вправо относительно вращения
    #[inline]
    pub fn right(&self) -> Vec3 {
        self.rotate(Vec3::RIGHT)
    }
}

// ============================================================
// Форматирование
// ============================================================

impl std::fmt::Display for Quat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Quat({:.3}, {:.3}, {:.3}, {:.3})", self.x, self.y, self.z, self.w)
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
        let q = Quat::IDENTITY;
        let v = Vec3::new(1.0, 0.0, 0.0);
        assert_eq!(q.rotate(v), v);
    }

    #[test]
    fn test_axis_angle() {
        let q = Quat::from_axis_angle(Vec3::UP, std::f32::consts::FRAC_PI_2);
        let v = Vec3::RIGHT;
        let rotated = q.rotate(v);
        assert!((rotated.x - 0.0).abs() < 0.001);
        assert!((rotated.z - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_mul_identity() {
        let q = Quat::from_axis_angle(Vec3::UP, 1.0);
        let result = q.mul(&Quat::IDENTITY);
        assert_eq!(result, q);
    }

    #[test]
    fn test_slerp() {
        let q1 = Quat::IDENTITY;
        let q2 = Quat::from_axis_angle(Vec3::UP, std::f32::consts::PI);
        let mid = q1.slerp(&q2, 0.5);
        assert!((mid.w - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_forward() {
        let q = Quat::IDENTITY;
        assert_eq!(q.forward(), Vec3::FORWARD);
    }

    #[test]
    fn test_conjugate() {
        let q = Quat::from_axis_angle(Vec3::UP, 1.0);
        let conj = q.conjugate();
        let v = Vec3::RIGHT;
        let rotated_back = conj.rotate(q.rotate(v));
        assert!((rotated_back.x - v.x).abs() < 0.001);
    }
}