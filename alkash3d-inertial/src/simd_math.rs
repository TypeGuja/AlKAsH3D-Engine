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
    gravity: f32,
) {
    let dt_vec = _mm256_set1_ps(dt);
    let gravity_vec = _mm256_set1_ps(gravity);

    let chunks = positions_x.len() / 8;

    for i in 0..chunks {
        let idx = i * 8;

        let vy = _mm256_loadu_ps(&velocities_y[idx]);
        let new_vy = _mm256_fmadd_ps(gravity_vec, dt_vec, vy);
        _mm256_storeu_ps(&mut velocities_y[idx], new_vy);

        let px = _mm256_loadu_ps(&positions_x[idx]);
        let py = _mm256_loadu_ps(&positions_y[idx]);
        let pz = _mm256_loadu_ps(&positions_z[idx]);
        let vx = _mm256_loadu_ps(&velocities_x[idx]);
        let vz = _mm256_loadu_ps(&velocities_z[idx]);

        let new_px = _mm256_fmadd_ps(vx, dt_vec, px);
        let new_py = _mm256_fmadd_ps(new_vy, dt_vec, py);
        let new_pz = _mm256_fmadd_ps(vz, dt_vec, pz);

        _mm256_storeu_ps(&mut positions_x[idx], new_px);
        _mm256_storeu_ps(&mut positions_y[idx], new_py);
        _mm256_storeu_ps(&mut positions_z[idx], new_pz);
    }

    let start = chunks * 8;
    for i in start..positions_x.len() {
        velocities_y[i] += gravity * dt;
        positions_x[i] += velocities_x[i] * dt;
        positions_y[i] += velocities_y[i] * dt;
        positions_z[i] += velocities_z[i] * dt;
    }
}

// AVX-512 версия - всегда компилируется, но использует соответствующие инструкции
#[inline(always)]
pub unsafe fn update_bodies_avx512(
    positions_x: &mut [f32],
    positions_y: &mut [f32],
    positions_z: &mut [f32],
    velocities_x: &mut [f32],
    velocities_y: &mut [f32],
    velocities_z: &mut [f32],
    dt: f32,
    gravity: f32,
) {
    #[cfg(target_feature = "avx512f")]
    {
        let dt_vec = _mm512_set1_ps(dt);
        let gravity_vec = _mm512_set1_ps(gravity);

        let chunks = positions_x.len() / 16;

        for i in 0..chunks {
            let idx = i * 16;

            let vy = _mm512_loadu_ps(&velocities_y[idx]);
            let new_vy = _mm512_fmadd_ps(gravity_vec, dt_vec, vy);
            _mm512_storeu_ps(&mut velocities_y[idx], new_vy);

            let px = _mm512_loadu_ps(&positions_x[idx]);
            let py = _mm512_loadu_ps(&positions_y[idx]);
            let pz = _mm512_loadu_ps(&positions_z[idx]);
            let vx = _mm512_loadu_ps(&velocities_x[idx]);
            let vz = _mm512_loadu_ps(&velocities_z[idx]);

            let new_px = _mm512_fmadd_ps(vx, dt_vec, px);
            let new_py = _mm512_fmadd_ps(new_vy, dt_vec, py);
            let new_pz = _mm512_fmadd_ps(vz, dt_vec, pz);

            _mm512_storeu_ps(&mut positions_x[idx], new_px);
            _mm512_storeu_ps(&mut positions_y[idx], new_py);
            _mm512_storeu_ps(&mut positions_z[idx], new_pz);
        }

        let start = chunks * 16;
        for i in start..positions_x.len() {
            velocities_y[i] += gravity * dt;
            positions_x[i] += velocities_x[i] * dt;
            positions_y[i] += velocities_y[i] * dt;
            positions_z[i] += velocities_z[i] * dt;
        }
    }

    #[cfg(not(target_feature = "avx512f"))]
    {
        // Fallback to AVX2
        update_bodies_simd(positions_x, positions_y, positions_z,
                           velocities_x, velocities_y, velocities_z, dt, gravity);
    }
}