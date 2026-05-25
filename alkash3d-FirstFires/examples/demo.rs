// examples/demo.rs - Максимально простая версия
use alkash3d_firstfires::{FirstFiresSystem, FirstFiresConfig};
use nalgebra::{Vector3, Matrix4, Point3};
use std::time::Instant;

fn main() {
    println!("==========================================");
    println!("FirstFires Light Culling System Demo");
    println!("Version: {}", alkash3d_firstfires::VERSION);
    println!("==========================================\n");

    // Конфигурация
    let config = FirstFiresConfig::default();
    let mut lighting = FirstFiresSystem::new(config);

    // Добавляем источники света
    println!("Adding lights...");

    // Сетка фонарей 20x20 = 400 источников
    lighting.add_street_lights_grid(-100.0, -100.0, 100.0, 100.0, 10.0, 4.0);

    // Круговая аллея
    lighting.add_street_lights_circle(Vector3::new(0.0, 0.0, 0.0), 50.0, 32, 4.0);

    let total_lights = lighting.get_lights().len();
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
    let (gpu_lights, grid) = lighting.cull(camera_pos, &view_proj, 0.016);
    let cull_time = start.elapsed();

    // Сохраняем данные
    let visible_count = gpu_lights.len();
    let grid_cells = grid.cells.len();
    let grid_entries = grid.entries.len();

    // Заканчиваем использовать gpu_lights и grid
    drop(gpu_lights);
    drop(grid);

    // Теперь можно взять stats
    let stats = lighting.get_stats();

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

    println!("\n🎉 FirstFires test complete!");
}