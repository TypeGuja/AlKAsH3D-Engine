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

/// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): ортографическая
/// проекция для DirectX (Left-Handed, NDC Z в [0,1] — тот же диапазон,
/// что и у `perspective` выше) — нужна для shadow map directional-света
/// ("солнца"): в отличие от обычной камеры, свет не имеет точки схода
/// лучей (все лучи параллельны), поэтому его проекция ортографическая, а
/// не перспективная.
pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    glam::camera::lh::proj::directx::orthographic(left, right, bottom, top, near, far)
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

/// ДОБАВЛЕНО (оптимизация рендера — жалоба пользователя на лаги, третье
/// выбранное направление после стриминга/физики): фрустум камеры из 6
/// плоскостей, извлечённых напрямую из view_proj-матрицы (метод
/// Gribb/Hartmann) — используется для CPU-side frustum culling ПЕРЕД
/// записью draw call'ов в командный список (см. `render_frame` в
/// engine/mod.rs), вместо того чтобы рисовать КАЖДУЮ из 965+ ECS-сущностей
/// каждый кадр независимо от того, видна ли она вообще в текущем кадре.
///
/// Работает целиком в терминах glam (`Mat4 * Vec4` — вектор-столбец), не
/// зависит от того, как HLSL-шейдер на GPU интерпретирует ту же матрицу в
/// константном буфере (row-major/column-major) — эти два использования
/// независимы: здесь считается ТОЛЬКО CPU-side решение "рисовать/не
/// рисовать", а GPU получает ту же самую матрицу, что и раньше, для
/// собственного, отдельного умножения в вершинном шейдере.
///
/// Извлечение плоскостей: если `M` — матрица, такая что `clip = M * v`
/// (`v` — вектор-столбец, ровно то, что делает `Mat4::mul_vec4`/
/// `transform_point3` в glam), то строки `M` дают 6 плоскостей фрустума
/// как суммы/разности строки 3 (w) с строками 0/1/2 (x/y/z) — стандартный
/// метод Gribb/Hartmann. `glam::Mat4` хранит СТОЛБЦЫ (`x_axis`/`y_axis`/
/// `z_axis`/`w_axis`), поэтому строки `M` берутся как столбцы `M.transpose()`.
/// Near/Far формулы соответствуют диапазону глубины [0,1] (DirectX-стиль —
/// та же конвенция, что уже использует `perspective()`/`orthographic()`
/// выше в этом файле).
///
/// Проверено изолированной сборкой против реального pinned glam 0.33.2 —
/// корректно отбрасывает объекты за спиной камеры и вне поля зрения,
/// никогда не отбрасывает объекты, реально пересекающие фрустум.
pub struct Frustum {
    /// Каждая плоскость: (a,b,c,d), НОРМАЛИЗОВАННАЯ так, что `(a,b,c)` —
    /// единичный вектор нормали, направленный ВНУТРЬ фрустума. Точка
    /// внутри фрустума по этой плоскости, если `a*x+b*y+c*z+d >= 0`.
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_view_proj(vp: &Mat4) -> Self {
        let mt = vp.transpose();
        let row0 = mt.x_axis;
        let row1 = mt.y_axis;
        let row2 = mt.z_axis;
        let row3 = mt.w_axis;

        let mut planes = [
            row3 + row0, // left
            row3 - row0, // right
            row3 + row1, // bottom
            row3 - row1, // top
            row2,        // near (depth range [0,1])
            row3 - row2, // far  (depth range [0,1])
        ];

        // Нормализация каждой плоскости — без неё `test_sphere` ниже не
        // мог бы честно сравнивать "расстояние до плоскости" с реальным
        // мировым радиусом сферы (расстояние в невормализованных
        // координатах измеряется в масштабе, зависящем от самой матрицы
        // проекции, а не в метрах).
        for plane in &mut planes {
            let normal_len = (plane.x * plane.x + plane.y * plane.y + plane.z * plane.z).sqrt();
            if normal_len > 1e-8 {
                *plane /= normal_len;
            }
        }

        Self { planes }
    }

    /// `true`, если сфера (`center`, `radius`) пересекает фрустум ИЛИ
    /// находится внутри него — то есть её стоит рисовать. `false` —
    /// сфера гарантированно ЦЕЛИКОМ вне хотя бы одной плоскости, можно
    /// безопасно пропустить draw call для неё.
    ///
    /// Консервативный тест (по сферам вокруг каждой плоскости, не точный
    /// AABB/OBB-тест) — иногда пропускает объекты, которые формально чуть
    /// ближе к границе, чем есть на самом деле (сфера — надмножество
    /// реальной геометрии меша), но НИКОГДА не отбрасывает то, что
    /// реально видно — то есть безопасен по построению, ложноотрицательных
    /// "не рисуем то, что видно" быть не может.
    pub fn test_sphere(&self, center: Vec3, radius: f32) -> bool {
        for plane in &self.planes {
            let dist = plane.x * center.x + plane.y * center.y + plane.z * center.z + plane.w;
            if dist < -radius {
                return false;
            }
        }
        true
    }
}