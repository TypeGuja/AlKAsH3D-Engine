// examples/perf_test.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя: "примерная производительность
// ... светового плагина") — у alkash3d-inertial (физика) уже был встроенный
// `cargo run --release --example perf_test`, у FirstFires (свет) такого не
// было, хотя ABI (`cull()`/`get_stats()`) для этого достаточен — см.
// `examples/demo.rs` за тем же паттерном инициализации через сырой C-ABI
// плагина. Прогоняет `cull()` много кадров подряд на разном числе
// источников света (сетка фонарей, статичная камера) и печатает среднее
// время каллинга + статистику видимости/сетки, тем же стилем вывода, что у
// inertial/perf_test.rs.

use alkash3d_firstfires::{get_plugin_api, GPULight, LightAPI, LightConfig};
use nalgebra::{Matrix4, Point3, Vector3};
use std::ffi::c_void;
use std::time::Instant;

fn point_light(pos: Vector3<f32>, range: f32) -> GPULight {
    GPULight {
        position: [pos.x, pos.y, pos.z, 0.0], // w=0.0 -> LightType::Point
        color: [1.0, 0.9, 0.7, 2.0],
        direction: [0.0, 0.0, 0.0, range],
        params: [0.0, 0.0, 0.0, 0.0],
    }
}

/// Квадратная сетка фонарей нужного размера — сторона сетки подбирается из
/// `count` (ceil(sqrt(count))), спейсинг фиксирован (10м, как в demo.rs), так
/// что сцена растёт вширь, а не становится плотнее — ближе к тому, как
/// реально выглядит уличное освещение большого уровня.
fn light_grid(count: u32, spacing: f32, range: f32) -> Vec<GPULight> {
    let side = (count as f32).sqrt().ceil() as u32;
    let mut lights = Vec::with_capacity(count as usize);
    'outer: for xi in 0..side {
        for zi in 0..side {
            if lights.len() as u32 >= count { break 'outer; }
            let x = (xi as f32 - side as f32 / 2.0) * spacing;
            let z = (zi as f32 - side as f32 / 2.0) * spacing;
            lights.push(point_light(Vector3::new(x, 4.0, z), range));
        }
    }
    lights
}

fn run_scenario(label: &str, light_count: u32, frames: u32) {
    // ИСПРАВЛЕНО: изначально здесь стоял конфиг, скопированный из
    // demo.rs (far_plane=300/grid_cell_size=10) — это УСТАРЕВШИЕ значения,
    // которые сам же движок (см. `alkash3d-rust/src/bin/main.rs` — там же
    // подробный комментарий про то же самое) заменил на far_plane=200/
    // grid_cell_size=20 именно из-за того, что мелкие ячейки при большом
    // far_plane давали сетку 60×60×60=216000 ячеек, и каждый фонарь с
    // range=60 (обычный уличный) пересекал тысячи из них — тестировать на
    // demo.rs-конфиге означало бы мерить давно пофикшенную патологию, а не
    // реальную производительность движка. Теперь конфиг 1:1 как в
    // `main.rs::setup_lights` — сетка 20×20×20=8000 ячеек.
    let config = LightConfig {
        max_lights: light_count.max(64),
        tile_size: 16,
        far_plane: 200.0,
        lod_distances: [30.0, 60.0, 200.0],
        grid_cell_size: 20.0,
    };

    let plugin_api = get_plugin_api();
    let instance = (plugin_api.init)(std::ptr::null_mut(), &config as *const LightConfig as *const c_void);
    if instance.is_null() {
        eprintln!("[{label}] init() вернул null — пропускаю сценарий");
        return;
    }
    let light_api_ptr = (plugin_api.get_light_api)(instance);
    if light_api_ptr.is_null() {
        eprintln!("[{label}] get_light_api() вернул null");
        (plugin_api.shutdown)(instance);
        return;
    }
    let light_api = unsafe { &*(light_api_ptr as *const LightAPI) };

    // Радиус 60 (как в demo.rs) — при спейсинге 10м это даёт реалистичное
    // перекрытие соседних фонарей (несколько ячеек сетки на свет), а не
    // вырожденный случай "каждый свет ровно в одной ячейке".
    let lights = light_grid(light_count, 10.0, 60.0);
    for light in &lights {
        (light_api.add_light)(instance, light as *const GPULight);
    }

    let camera_pos = Vector3::new(0.0, 10.0, -50.0);
    let look_at = Vector3::new(0.0, 10.0, 50.0);
    let view = Matrix4::look_at_rh(&Point3::from(camera_pos), &Point3::from(look_at), &Vector3::y_axis());
    let proj = Matrix4::new_perspective(16.0 / 9.0, 90.0_f32.to_radians(), 0.1, 300.0);
    let view_proj = proj * view;
    let view_proj_arr: [f32; 16] = view_proj.as_slice().try_into().unwrap();

    // Прогрев (первый вызов часто дороже — аллокации внутренних Vec).
    (light_api.cull)(instance, camera_pos.as_slice().as_ptr(), view_proj_arr.as_ptr(), 0.016);

    let start = Instant::now();
    for _ in 0..frames {
        (light_api.cull)(instance, camera_pos.as_slice().as_ptr(), view_proj_arr.as_ptr(), 0.016);
    }
    let elapsed = start.elapsed();

    let stats = (light_api.get_stats)(instance);
    let grid_cells = (light_api.get_grid_cells_count)(instance);
    let grid_entries = (light_api.get_grid_entries_count)(instance);
    let ms_per_frame = elapsed.as_secs_f64() * 1000.0 / frames as f64;
    let fps_equiv = if ms_per_frame > 0.0 { 1000.0 / ms_per_frame } else { f64::INFINITY };

    println!("=== {label} (источников: {light_count}) ===");
    println!(
        "  cull(): {:.4} мс/кадр (~{:.0} FPS эквивалент), {frames} кадров за {:.2}мс",
        ms_per_frame, fps_equiv, elapsed.as_secs_f64() * 1000.0
    );
    println!(
        "  Видимо: {} / {}  culled_lod: {}  culled_frustum: {}  grid_cells: {}  grid_entries: {}",
        stats.visible_lights, stats.total_lights, stats.culled_by_lod, stats.culled_by_frustum,
        grid_cells, grid_entries
    );

    (plugin_api.shutdown)(instance);
    println!();
}

fn main() {
    println!("==========================================");
    println!("FirstFires Light Culling — Performance Test");
    println!("Plugin API version: {}", alkash3d_firstfires::PLUGIN_API_VERSION);
    println!("==========================================\n");

    for &count in &[100u32, 500, 1000, 2000, 5000, 10000] {
        run_scenario("Сетка фонарей", count, 300);
    }
}
