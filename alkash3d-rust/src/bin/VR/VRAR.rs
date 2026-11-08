use std::error::Error;
use alkash3d_rs::*;
use std::time::*;
use windows::core::Interface;
use alkash3d_rs::engine::{AlkashEngine, GraphicsSettings};

const FIRSTFIRES_DLL_PATH: &str = "./alkash3d_firstfires.dll";
const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

const KEY_F: u32 = 0x46;
const EYE_HEIGHT: f32 = 1.6;

const SPAWN_X: f32 = 10.0;
const SPAWN_Y: f32 = 0.0;
const SPAWN_Z: f32 = 5.0;
const SPAWN_YAW_DEG: f32 = 45.0;

const ENABLE_MSAA: bool = false;
const ENABLE_SSAO: bool = false;
const ENABLE_BLOOM: bool = false;
const ENABLE_VOLUMETRIC: bool = false;
const ENABLE_SHADOWS: bool = true;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    alkash3d_rs::console_log::init_console_log_to_file("VR engine test log")

    let map = resolve_map_files();
    if !map.from_map_dir {
        if std::path::Path::new(TEST_WORLD_PATH).exists() {
            println!("[MAIN_TEST] {} уже существует — использую как есть (не перезаписываю)", TEST_WORLD_PATH);
        } else if let Err(e) = create_test_world(TEST_WORLD_PATH) {
            eprintln!("[MAIN_TEST] Не удалось создать {}: {:?}", TEST_WORLD_PATH, e);
            return Err(e.into());
        } else {
            println!("[MAIN_TEST] ✓ {} создан (синтетическая точка спавна: ({:.1}, {:.1}, {:.1}), yaw={:.1}°)", TEST_WORLD_PATH, SPAWN_X, SPAWN_Y, SPAWN_Z, SPAWN_YAW_DEG);
        }
    }

    let graphics_settings = GraphicsSettings {
        msaa: ENABLE_MSAA,
        ssao: ENABLE_SSAO,
        bloom: ENABLE_BLOOM,
        volumetric: ENABLE_VOLUMETRIC,
        shadows: ENABLE_SHADOWS,
        render_scale: 1.0,
    };
    println!(
        "[MAIN_TEST] GraphicsSettings: msaa={} ssao={} bloom={} volumetric={} shadows={} (правь константы ENABLE_* в начале файла)",
        graphics_settings.msaa, graphics_settings.ssao, graphics_settings.bloom,
        graphics_settings.volumetric, graphics_settings.shadows
    );

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    // ВАЖНО: ДО init() — настройки читаются один раз при построении
    // шейдеров/PSO/ресурсов, после init() эффекта уже не будет (см.
    // подробный комментарий у `set_graphics_settings` в lifecycle.rs).
    engine.set_graphics_settings(graphics_settings);
    if let Err(e) = engine.init() {
        eprintln!("[MAIN_TEST] Failed to initialize engine: {:?}", e);
        return Err(e.into());
    }

    engine.set_clear_color(0.05, 0.05, 0.12, 1.0);
    engine.camera.far = CAMERA_FAR_PLANE;

    engine.set_time_of_day(22.0);
    setup_lights_from_alfar(&mut engine, &map.alfars);

    let world_str = map.world.to_string_lossy().into_owned();
    let chunks_str = map.chunks.to_string_lossy().into_owned();
    match engine.load_world(&world_str, Some(&chunks_str)) {
        Ok(()) => println!("[MAIN_TEST] ✓ {} загружен (чанки из {})", world_str, chunks_str),
        Err(e) => eprintln!("[MAIN_TEST] ✗ Не удалось загрузить {}: {:?}", world_str, e),
    }

    match engine.world_spawn_point() {
        Some((pos, yaw)) => {
            println!(
                "[MAIN_TEST] ✓ Точка спавна прочитана: ({:.2}, {:.2}, {:.2}), yaw={:.1}°",
                pos.x, pos.y, pos.z, yaw.to_degrees()
            );
            let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());

            // куб-маркер нужен только синтетическому тесту спавна, не настоящей карте
            if !map.from_map_dir {
                let marker_pos = pos + forward * 3.0;
                let marker_mesh = engine.add_cube_colored(0.6, 0.9, 0.25, 0.2, 1.0);
                let marker = engine.spawn_mesh_entity(marker_mesh);
                if let Some(t) = engine.scene.transform_mut(marker) {
                    t.position = [marker_pos.x, marker_pos.y + 0.3, marker_pos.z];
                }
            }

            engine.camera.position = Vec3::new(pos.x, pos.y + EYE_HEIGHT, pos.z);
            engine.camera.target = engine.camera.position + forward;
        }
        None => {
            eprintln!("[MAIN_TEST] ✗ Точка спавна НЕ найдена в загруженном мире — тест провален (ожидалась одна в global_objects)");
            engine.camera.position = Vec3::new(0.0, EYE_HEIGHT, 0.0);
            engine.camera.target = Vec3::new(0.0, EYE_HEIGHT, 1.0);
        }
    }

    let walk = if map.from_map_dir { walk::WorldCollision::open(&map.world, &map.chunks) } else { None };
    run_loop(&mut engine, walk);

    engine.shutdown();
    println!("[MAIN_TEST] Goodbye!");
    Ok(())
}

struct MapFiles {
    world: std::path::PathBuf,
    chunks: std::path::PathBuf,
    alfars: Vec<std::path::PathBuf>,
    from_map_dir: bool,
}