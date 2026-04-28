// Skybox Shader - Procedural Sky with Atmosphere Scattering

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) local_position: vec3<f32>,
}

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_position: vec3<f32>,
    _padding: f32,
}

struct SkyUniform {
    sun_direction: vec3<f32>,
    sun_color: vec3<f32>,
    sun_intensity: f32,
    rayleigh_color: vec3<f32>,
    mie_color: vec3<f32>,
    turbidity: f32,
    exposure: f32,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var<uniform> sky: SkyUniform;

@group(1) @binding(1)
var sky_texture: texture_cube<f32>;

@group(1) @binding(2)
var sky_sampler: sampler;

// Константы атмосферы Земли
const RAYLEIGH_HEIGHT: f32 = 8400.0;
const MIE_HEIGHT: f32 = 1200.0;
const PLANET_RADIUS: f32 = 6371000.0;
const ATMOSPHERE_RADIUS: f32 = 6471000.0;

// Солнечный диск
fn sun_disc(dir: vec3<f32>, sun_dir: vec3<f32>, angular_radius: f32) -> f32 {
    let cos_theta = dot(dir, sun_dir);
    let theta = acos(cos_theta);
    return smoothstep(angular_radius, angular_radius * 0.99, theta);
}

// Рэлеевское рассеивание
fn rayleigh_phase(cos_theta: f32) -> f32 {
    return (3.0 / (16.0 * 3.14159265359)) * (1.0 + cos_theta * cos_theta);
}

// Ми-рассеивание
fn mie_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5);
    return (3.0 * (1.0 - g2)) / (8.0 * 3.14159265359 * (2.0 + g2) * denom);
}

// Пересечение с атмосферой
fn intersect_atmosphere(origin: vec3<f32>, dir: vec3<f32>) -> vec2<f32> {
    let offset_origin = origin - vec3<f32>(0.0, -PLANET_RADIUS, 0.0);
    let a = dot(dir, dir);
    let b = 2.0 * dot(offset_origin, dir);
    let c = dot(offset_origin, offset_origin) - ATMOSPHERE_RADIUS * ATMOSPHERE_RADIUS;
    let discriminant = b * b - 4.0 * a * c;

    if discriminant < 0.0 {
        return vec2<f32>(-1.0);
    }

    let sqrt_disc = sqrt(discriminant);
    let t1 = (-b - sqrt_disc) / (2.0 * a);
    let t2 = (-b + sqrt_disc) / (2.0 * a);

    return vec2<f32>(max(0.0, t1), t2);
}

// Получение цвета неба
fn get_sky_color(dir: vec3<f32>, sun_dir: vec3<f32>) -> vec3<f32> {
    let cos_theta = dot(dir, sun_dir);

    // Упрощённая модель атмосферы
    let rayleigh_factor = 1.0 / (cos_theta * 0.5 + 0.8);
    let mie_factor = (1.0 + cos_theta * cos_theta) / pow(1.1 - 0.9 * cos_theta, 1.5);

    // Горизонт
    let horizon = 1.0 - abs(dir.y);
    let horizon_glow = exp(-horizon * 3.0);

    // Цвета неба
    let zenith = mix(
        vec3<f32>(0.1, 0.2, 0.5),  // Ночное небо (в зените)
        vec3<f32>(0.3, 0.5, 1.0),  // Дневное небо (в зените)
        saturate(sun_dir.y * 2.0 + 0.5)
    );

    let horizon_color = mix(
        vec3<f32>(0.6, 0.7, 1.0),  // Светлый горизонт
        vec3<f32>(1.0, 0.8, 0.5),  // Тёплый горизонт (закат)
        horizon_glow
    );

    // Собираем цвет неба
    let sky_color = mix(zenith, horizon_color, pow(horizon, 2.0));

    // Добавляем солнце
    let sun = sun_disc(dir, sun_dir, 0.01) * sky.sun_color * sky.sun_intensity;

    // Звёзды (только ночью)
    let stars = 0.0;
    if (sun_dir.y < -0.1) {
        // Простые звёзды на основе позиции
        let star_noise = fract(sin(dot(floor(dir * 1000.0), vec3<f32>(12.9898, 78.233, 45.5432))) * 43758.5453);
        let star = smoothstep(0.997, 1.0, star_noise) * (1.0 - abs(sun_dir.y));
        let stars_color = vec3<f32>(0.8, 0.9, 1.0) * star * 0.5;
        sky_color += stars_color;
    }

    return sky_color + sun;
}

// Процедурные облака
fn get_clouds(position: vec3<f32>, time: f32) -> f32 {
    let uv = position.xz * 0.001 + time * 0.1;

    // Многооктавный шум для облаков
    var cloud = 0.0;
    var amplitude = 1.0;
    var frequency = 1.0;
    var max_value = 0.0;

    for (var i = 0u; i < 4u; i++) {
        let sample_uv = uv * frequency;
        let noise = fract(sin(dot(vec2<f32>(sample_uv.x, sample_uv.y), vec2<f32>(12.9898, 78.233))) * 43758.5453);

        cloud += noise * amplitude;
        max_value += amplitude;

        amplitude *= 0.5;
        frequency *= 2.0;
    }

    cloud /= max_value;

    // Формируем облака
    cloud = smoothstep(0.4, 0.6, cloud);

    // Учитываем высоту (облака только на определённой высоте)
    let height_falloff = exp(-abs(position.y - 2000.0) / 1000.0);
    cloud *= height_falloff;

    return cloud;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    // Full-screen triangle для скайбокса
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );

    let pos = positions[vertex_index];

    var out: VertexOutput;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);

    // Получаем направление луча из позиции на экране
    let near_point = vec4<f32>(pos.x, pos.y, -1.0, 1.0);
    let far_point = vec4<f32>(pos.x, pos.y, 1.0, 1.0);

    let inv_proj = mat4x4<f32>(
        vec4<f32>(1.0 / camera.view_proj[0][0], 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0 / camera.view_proj[1][1], 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, -1.0),
        vec4<f32>(0.0, 0.0, 0.5, 0.5),
    );

    let near_world = inv_proj * near_point;
    let far_world = inv_proj * far_point;

    let near = near_world.xyz / near_world.w;
    let far = far_world.xyz / far_world.w;

    out.local_position = normalize(far - near);
    out.world_position = out.local_position;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.local_position);

    // Вычисляем цвет неба
    var color = get_sky_color(dir, sky.sun_direction);

    // Добавляем облака (только днём)
    if (sky.sun_direction.y > -0.2) {
        let cloud = get_clouds(dir * 10000.0, 0.0);
        color = mix(color, vec3<f32>(1.0), cloud * 0.3);
    }

    // HDR тональная коррекция
    color = 1.0 - exp(-color * sky.exposure);

    // Гамма-коррекция
    color = pow(color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(color, 1.0);
}