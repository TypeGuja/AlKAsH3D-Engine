// src/math.rs
//! Математика для 3D рендеринга (на базе glam 0.33.2)

pub use glam::*;

// Переэкспортируем нужные типы для совместимости
pub type Mat4 = glam::Mat4;
pub type Vec3 = glam::Vec3;
pub type Vec4 = glam::Vec4;
pub type Quat = glam::Quat;

// Вспомогательные функции для работы с массивами
pub fn mat4_to_array(m: &Mat4) -> [[f32; 4]; 4] {
    m.to_cols_array_2d()
}

pub fn array_to_mat4(arr: &[[f32; 4]; 4]) -> Mat4 {
    Mat4::from_cols_array_2d(arr)
}

/// Перспективная проекция для DirectX (Left-Handed)
/// Используем новый API из glam::camera
pub fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    // Используем правильный модуль для DirectX
    glam::camera::lh::proj::directx::perspective(fov, aspect, near, far)
}

/// View матрица для DirectX (Left-Handed)
pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    glam::camera::lh::view::look_at_mat4(eye, target, up)
}

/// Матрица трансляции
pub fn translation(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

/// Матрица поворота вокруг X
pub fn rotation_x(angle: f32) -> Mat4 {
    Mat4::from_rotation_x(angle)
}

/// Матрица поворота вокруг Y
pub fn rotation_y(angle: f32) -> Mat4 {
    Mat4::from_rotation_y(angle)
}

/// Матрица поворота вокруг Z
pub fn rotation_z(angle: f32) -> Mat4 {
    Mat4::from_rotation_z(angle)
}

/// Матрица масштабирования
pub fn scaling(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_scale(Vec3::new(x, y, z))
}

/// Единичная матрица (Identity)
pub fn identity() -> Mat4 {
    Mat4::IDENTITY
}

/// Преобразование точки через матрицу
pub fn transform_point(mat: &Mat4, point: &[f32; 3]) -> [f32; 4] {
    let p = mat.transform_point3(Vec3::new(point[0], point[1], point[2]));
    [p.x, p.y, p.z, 1.0]
}

/// Преобразование вектора через матрицу
pub fn transform_vector(mat: &Mat4, vec: &[f32; 3]) -> [f32; 3] {
    let v = mat.transform_vector3(Vec3::new(vec[0], vec[1], vec[2]));
    [v.x, v.y, v.z]
}