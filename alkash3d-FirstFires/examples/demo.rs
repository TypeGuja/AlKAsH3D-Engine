// examples/demo.rs - Максимально простая версия
//
// ИСПРАВЛЕНО (ошибка компиляции E0432/E0425): демка была написана под
// высокоуровневый API (`FirstFiresSystem`, `FirstFiresConfig::default()`,
// `lighting.add_street_lights_grid(...)`, `alkash3d_firstfires::VERSION`
// и т.п.), которого в крейте НЕТ и никогда не было — `src/lib.rs`
// экспортирует только сырой C-ABI плагина (`get_plugin_api()` →
// `init`/`get_light_api`/`add_light`/`cull`/`get_stats`, как и у
// `inertial`/`PhysicsAPI`). Ниже — та же демонстрация (сетка фонарей +
// круговая аллея, каллинг, вывод статистики), но через РЕАЛЬНЫЙ
// экспортируемый ABI, тем же путём, которым его использует движок.
// examples/demo.rs - Максимально простая версия
//
// ИСПРАВЛЕНО (ошибка компиляции E0432/E0425): демка была написана под
// высокоуровневый API (`FirstFiresSystem`, `FirstFiresConfig::default()`,
// `lighting.add_street_lights_grid(...)`, `alkash3d_firstfires::VERSION`
// и т.п.), которого в крейте НЕТ и никогда не было — `src/lib.rs`
// экспортирует только сырой C-ABI плагина (`get_plugin_api()` →
// `init`/`get_light_api`/`add_light`/`cull`/`get_stats`, как и у
// `inertial`/`PhysicsAPI`). Ниже — та же демонстрация (сетка фонарей +
// круговая аллея, каллинг, вывод статистики), но через РЕАЛЬНЫЙ
// экспортируемый ABI, тем же путём, которым его использует движок.
use alkash3d_firstfires::{get_plugin_api, GPULight, LightAPI, LightConfig};
use nalgebra::{Matrix4, Point3, Vector3};
use std::ffi::c_void;
use std::time::Instant;

fn point_light(pos: Vector3<f32>, color: Vector3<f32>, intensity: f32, range: f32) -> GPULight {
    GPULight {
        position: [pos.x, pos.y, pos.z, 0.0], // w=0.0 -> LightType::Point
        color: [color.x, color.y, color.z, intensity],
        direction: [0.0, 0.0, 0.0, range],
        params: [0.0, 0.0, 0.0, 0.0],
    }
}

/// Сетка уличных фонарей (аналог add_street_lights_grid из старой демки).
fn street_lights_grid(min_x: f32, min_z: f32, max_x: f32, max_z: f32, spacing: f32, height: f32) -> Vec<GPULight> {
    let mut lights = Vec::new();
    let mut x = min_x;
    while x <= max_x {
        let mut z = min_z;
        while z <= max_z {
            lights.push(point_light(
                Vector3::new(x, height, z),
                Vector3::new(1.0, 0.9, 0.7),
                2.0,
                // Радиус подобран так, чтобы демо реально показывало и
                // видимые, и отсечённые источники (при камере на z=-50,
                // глядящей вдоль +z) — с исходным 15.0 отсекались вообще
                // все, что доказывало только "не падает", но не то, что
                // каллинг по дальности/фрustum'у действительно работает.
                60.0,
            ));
            z += spacing;
        }
        x += spacing;
    }
    lights
}

/// Круговая аллея фонарей (аналог add_street_lights_circle).
fn street_lights_circle(center: Vector3<f32>, radius: f32, count: u32, height: f32) -> Vec<GPULight> {
    (0..count)
        .map(|i| {
            let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
            let pos = center + Vector3::new(angle.cos() * radius, height, angle.sin() * radius);
            point_light(pos, Vector3::new(0.9, 0.9, 1.0), 1.5, 60.0)
        })
        .collect()
}

fn main() {
    println!("==========================================");
    println!("FirstFires Light Culling System Demo");
    println!("Plugin API version: {}", alkash3d_firstfires::PLUGIN_API_VERSION);
    println!("==========================================\n");

    // Конфигурация
    let config = LightConfig {
        max_lights: 1024,
        tile_size: 16,
        far_plane: 300.0,
        lod_distances: [50.0, 150.0, 300.0],
        grid_cell_size: 10.0,
    };

    let plugin_api = get_plugin_api();
    let instance = (plugin_api.init)(
        std::ptr::null_mut(),
        &config as *const LightConfig as *const c_void,
    );
    if instance.is_null() {
        eprintln!("init() вернул null — не удалось создать инстанс");
        return;
    }

    let light_api_ptr = (plugin_api.get_light_api)(instance);
    if light_api_ptr.is_null() {
        eprintln!("get_light_api() вернул null");
        (plugin_api.shutdown)(instance);
        return;
    }
    let light_api = unsafe { &*(light_api_ptr as *const LightAPI) };

    // Добавляем источники света
    println!("Adding lights...");

    // Сетка фонарей 20x20 = 400 источников
    let grid_lights = street_lights_grid(-100.0, -100.0, 100.0, 100.0, 10.0, 4.0);
    // Круговая аллея
    let circle_lights = street_lights_circle(Vector3::new(0.0, 0.0, 0.0), 50.0, 32, 4.0);

    let mut total_lights = 0u32;
    for light in grid_lights.iter().chain(circle_lights.iter()) {
        (light_api.add_light)(instance, light as *const GPULight);
        total_lights += 1;
    }
    println!("Generated {} lights\n", total_lights);

    // Камера
    let camera_pos = Vector3::new(0.0, 10.0, -50.0);
    let look_at = Vector3::new(0.0, 10.0, 50.0);

    let view = Matrix4::look_at_rh(
        &Point3::from(camera_pos),
        &Point3::from(look_at),
        &Vector3::y_axis(),
    );

    let proj = Matrix4::new_perspective(16.0 / 9.0, 90.0_f32.to_radians(), 0.1, 300.0);
    let view_proj = proj * view;

    // Тест
    println!("Running culling test...\n");

    let start = Instant::now();
    (light_api.cull)(
        instance,
        camera_pos.as_slice().as_ptr(),
        view_proj.as_slice().as_ptr(),
        0.016,
    );
    let cull_time = start.elapsed();

    let visible_count = (light_api.get_gpu_lights_count)(instance);
    let grid_cells = (light_api.get_grid_cells_count)(instance);
    let grid_entries = (light_api.get_grid_entries_count)(instance);
    let stats = (light_api.get_stats)(instance);

    println!("=== RESULTS ===");
    println!("Total lights:      {}", stats.total_lights);
    println!("Visible lights:    {}", stats.visible_lights);
    println!("Culled by LOD:     {}", stats.culled_by_lod);
    println!("Culled by dist:    {}", stats.culled_by_distance);
    println!("Culled by frustum: {}", stats.culled_by_frustum);
    println!("Culling time:      {:.2} ms", cull_time.as_secs_f32() * 1000.0);
    println!("Grid cells:        {}", grid_cells);
    println!("Grid entries:      {}", grid_entries);
    println!("GPU lights:        {}", visible_count);

    let efficiency = if stats.total_lights > 0 {
        (stats.visible_lights as f32 / stats.total_lights as f32) * 100.0
    } else {
        0.0
    };
    println!("Efficiency:        {:.1}%", efficiency);

    if stats.visible_lights > 0 {
        println!("\n✅ SUCCESS: {} lights are visible!", stats.visible_lights);
    } else {
        println!("\n⚠️ No visible lights detected");
    }

    (plugin_api.shutdown)(instance);
    println!("\n🎉 FirstFires test complete!");
}