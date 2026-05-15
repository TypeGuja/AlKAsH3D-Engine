// src/simd_math.rs - AVX2/AVX-512 интринсики
#![cfg(target_arch = "x86_64")]

use std::arch::x86_64::*;

#[inline(always)]
pub unsafe fn update_bodies_simd(
    positions_x: &mut [f32],
    positions_y: &mut [f32],
    positions_z: &mut [f32],
    velocities_x: &mut [f32],
    velocities_y: &mut [f32],
    velocities_z: &mut [f32],
    dt: f32,
) {
    let dt_vec = _mm256_set1_ps(dt);
    let gravity = _mm256_set1_ps(-9.81);

    let chunks = positions_x.len() / 8;

    for i in 0..chunks {
        let idx = i * 8;

        // Загружаем скорости Y
        let vy = _mm256_loadu_ps(&velocities_y[idx]);
        // Добавляем гравитацию
        let new_vy = _mm256_fmadd_ps(gravity, dt_vec, vy);
        // Сохраняем
        _mm256_storeu_ps(&mut velocities_y[idx], new_vy);

        // Загружаем позиции
        let px = _mm256_loadu_ps(&positions_x[idx]);
        let py = _mm256_loadu_ps(&positions_y[idx]);
        let pz = _mm256_loadu_ps(&positions_z[idx]);

        // Загружаем скорости
        let vx = _mm256_loadu_ps(&velocities_x[idx]);
        let vz = _mm256_loadu_ps(&velocities_z[idx]);

        // Обновляем позиции
        let new_px = _mm256_fmadd_ps(vx, dt_vec, px);
        let new_py = _mm256_fmadd_ps(new_vy, dt_vec, py);
        let new_pz = _mm256_fmadd_ps(vz, dt_vec, pz);

        // Сохраняем
        _mm256_storeu_ps(&mut positions_x[idx], new_px);
        _mm256_storeu_ps(&mut positions_y[idx], new_py);
        _mm256_storeu_ps(&mut positions_z[idx], new_pz);
    }
}