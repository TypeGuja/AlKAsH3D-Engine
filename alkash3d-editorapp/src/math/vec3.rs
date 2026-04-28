// src/math/vec3.rs

use std::ops::{Add, Sub, Mul, AddAssign};

/// Трёхмерный вектор с основными математическими операциями
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    /// Нулевой вектор (0, 0, 0)
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };

    /// Единичный вектор (1, 1, 1)
    pub const ONE: Self = Self { x: 1.0, y: 1.0, z: 1.0 };

    /// Вектор вверх (0, 1, 0)
    pub const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };

    /// Вектор вправо (1, 0, 0)
    pub const RIGHT: Self = Self { x: 1.0, y: 0.0, z: 0.0 };

    /// Вектор вперёд (0, 0, 1)
    pub const FORWARD: Self = Self { x: 0.0, y: 0.0, z: 1.0 };

    /// Создаёт новый вектор
    #[inline]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Создаёт вектор со всеми одинаковыми компонентами
    #[inline]
    pub fn splat(value: f32) -> Self {
        Self { x: value, y: value, z: value }
    }

    /// Длина вектора (магнитуда)
    #[inline]
    pub fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Квадрат длины (быстрее, чем length)
    #[inline]
    pub fn length_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Нормализованный вектор (единичной длины)
    #[inline]
    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 0.0001 {
            Self {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
            }
        } else {
            *self
        }
    }

    /// Безопасная нормализация (возвращает None если длина ~0)
    #[inline]
    pub fn try_normalize(&self) -> Option<Self> {
        let len = self.length();
        if len > 0.0001 {
            Some(Self {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
            })
        } else {
            None
        }
    }

    /// Векторное произведение
    #[inline]
    pub fn cross(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Скалярное произведение
    #[inline]
    pub fn dot(&self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Линейная интерполяция между двумя векторами
    #[inline]
    pub fn lerp(&self, other: Vec3, t: f32) -> Vec3 {
        Vec3 {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            z: self.z + (other.z - self.z) * t,
        }
    }

    /// Покомпонентный минимум
    #[inline]
    pub fn min(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
            z: self.z.min(other.z),
        }
    }

    /// Покомпонентный максимум
    #[inline]
    pub fn max(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x.max(other.x),
            y: self.y.max(other.y),
            z: self.z.max(other.z),
        }
    }

    /// Покомпонентное ограничение значения
    #[inline]
    pub fn clamp(&self, min: Vec3, max: Vec3) -> Vec3 {
        Vec3 {
            x: self.x.clamp(min.x, max.x),
            y: self.y.clamp(min.y, max.y),
            z: self.z.clamp(min.z, max.z),
        }
    }

    /// Расстояние между двумя точками
    #[inline]
    pub fn distance(&self, other: Vec3) -> f32 {
        (*self - other).length()
    }

    /// Квадрат расстояния между точками
    #[inline]
    pub fn distance_squared(&self, other: Vec3) -> f32 {
        (*self - other).length_squared()
    }

    /// Угол между двумя векторами в радианах
    #[inline]
    pub fn angle_between(&self, other: Vec3) -> f32 {
        let dot = self.dot(other);
        let len_product = self.length() * other.length();
        if len_product < 0.0001 {
            0.0
        } else {
            (dot / len_product).clamp(-1.0, 1.0).acos()
        }
    }

    /// Отражает вектор относительно нормали
    #[inline]
    pub fn reflect(&self, normal: Vec3) -> Vec3 {
        *self - normal * 2.0 * self.dot(normal)
    }

    /// Проецирует вектор на другой вектор
    #[inline]
    pub fn project_onto(&self, other: Vec3) -> Vec3 {
        let other_len_sq = other.length_squared();
        if other_len_sq < 0.0001 {
            Vec3::ZERO
        } else {
            other * (self.dot(other) / other_len_sq)
        }
    }

    /// Покомпонентное умножение
    #[inline]
    pub fn mul_component(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x * other.x,
            y: self.y * other.y,
            z: self.z * other.z,
        }
    }

    /// Покомпонентное деление
    #[inline]
    pub fn div_component(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x / other.x,
            y: self.y / other.y,
            z: self.z / other.z,
        }
    }

    /// Проверяет, является ли вектор нулевым
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.length_squared() < 0.0001
    }

    /// Преобразует в массив [f32; 3]
    #[inline]
    pub fn to_array(&self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Создаёт из массива [f32; 3]
    #[inline]
    pub fn from_array(arr: [f32; 3]) -> Self {
        Self { x: arr[0], y: arr[1], z: arr[2] }
    }

    /// Преобразует в кортеж (f32, f32, f32)
    #[inline]
    pub fn to_tuple(&self) -> (f32, f32, f32) {
        (self.x, self.y, self.z)
    }
}

// ============================================================
// Арифметические операции
// ============================================================

impl Add for Vec3 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

impl Sub for Vec3 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z }
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs }
    }
}

impl Mul<Vec3> for f32 {
    type Output = Vec3;
    #[inline]
    fn mul(self, rhs: Vec3) -> Vec3 {
        Vec3 { x: self * rhs.x, y: self * rhs.y, z: self * rhs.z }
    }
}

impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

// ============================================================
// Оператор отрицания
// ============================================================

impl std::ops::Neg for Vec3 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self { x: -self.x, y: -self.y, z: -self.z }
    }
}

// ============================================================
// Форматирование
// ============================================================

impl std::fmt::Display for Vec3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.3}, {:.3}, {:.3})", self.x, self.y, self.z)
    }
}

// ============================================================
// Тесты
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn test_constants() {
        assert_eq!(Vec3::ZERO, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(Vec3::ONE, Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(Vec3::UP, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn test_length() {
        let v = Vec3::new(3.0, 4.0, 0.0);
        assert_eq!(v.length(), 5.0);
    }

    #[test]
    fn test_normalize() {
        let v = Vec3::new(3.0, 4.0, 0.0);
        let n = v.normalize();
        assert!((n.length() - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_dot() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(a.dot(b), 0.0);
        assert_eq!(a.dot(a), 1.0);
    }

    #[test]
    fn test_cross() {
        let a = Vec3::RIGHT;
        let b = Vec3::UP;
        let c = a.cross(b);
        assert_eq!(c, Vec3::FORWARD);
    }

    #[test]
    fn test_add() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
    }

    #[test]
    fn test_sub() {
        let a = Vec3::new(5.0, 7.0, 9.0);
        let b = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(a - b, Vec3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn test_mul() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v * 2.0, Vec3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn test_distance() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(3.0, 4.0, 0.0);
        assert_eq!(a.distance(b), 5.0);
    }

    #[test]
    fn test_lerp() {
        let a = Vec3::ZERO;
        let b = Vec3::ONE;
        let mid = a.lerp(b, 0.5);
        assert_eq!(mid, Vec3::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn test_clamp() {
        let v = Vec3::new(5.0, -2.0, 0.5);
        let clamped = v.clamp(Vec3::ZERO, Vec3::ONE);
        assert_eq!(clamped, Vec3::new(1.0, 0.0, 0.5));
    }

    #[test]
    fn test_angle_between() {
        let a = Vec3::RIGHT;
        let b = Vec3::UP;
        let angle = a.angle_between(b);
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn test_reflect() {
        let v = Vec3::new(1.0, -1.0, 0.0);
        let n = Vec3::UP;
        let reflected = v.reflect(n);
        assert_eq!(reflected, Vec3::new(1.0, 1.0, 0.0));
    }
}