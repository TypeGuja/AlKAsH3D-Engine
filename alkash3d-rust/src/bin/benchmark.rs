// src/bin/benchmark.rs
//! Бенчмарк движка — фиксированная нагрузка (по умолчанию) или АВТОНАГРУЗКА
//! (`ALKASH3D_BENCH_AUTO=1`), которая сама поэтапно растит число объектов на
//! сцене, пока не найдёт "точку излома" (FPS падает ниже целевого) — печатает
//! отчёт по времени кадра (avg/min/max FPS, 1% low) на каждом шаге.
//!
//! ПОЧЕМУ отдельный бинарник, а не флаг `main.rs`/`main_car.rs`: тот же
//! принцип, что и у `example_minimal.rs` — самозавершающийся кадровый
//! бюджет (не бесконечный `while engine.is_running()`, как в реальных демо),
//! чтобы бенчмарк сам чисто выходил с готовым числом, а не требовал руками
//! закрывать окно и смотреть на консоль. Сцена — своя (не переиспользует
//! `main_car.rs::setup_*`), т.к. там сцена фиксированного размера, а тут
//! нужна МАСШТАБИРУЕМАЯ нагрузка.
//!
//! БЕЗОПАСНОСТЬ ЗАПУСКА (см. `ALKASH3D_GBV` в device.rs и историю крашей
//! всей машины на тяжёлых сценах в этом репозитории — "GPU-Based Validation
//! + сотни сущностей" уже вешало компьютер намертво): GPU-Based Validation
//! по умолчанию ВЫКЛЮЧЕНА и этот бинарник её не включает — не выставляй
//! `ALKASH3D_GBV=1` вместе с автонагрузкой или большим `ALKASH3D_BENCH_*`,
//! это ровно тот сценарий. Кроме того, автонагрузка сама следит за временем
//! КАЖДОГО кадра (см. `HARD_STOP_FRAME_MS`) — если хоть один кадр займёт
//! подозрительно долго (признак того, что GPU уже начинает "тормозить" перед
//! настоящим зависанием), бенчмарк немедленно останавливается и НЕ растит
//! нагрузку дальше, вместо того чтобы слепо долбить ещё более тяжёлый шаг.
//! Это снижает риск, но не убирает его полностью — реальный GPU-хендж (см.
//! `TdrLevel`/фикс "frame 2" в истории репозитория) может произойти и без
//! предупреждающих медленных кадров, поэтому автонагрузку всё равно стоит
//! в первый раз наблюдать вживую, а не запускать и уходить.
//!
//! Переменные окружения — фиксированный режим (по умолчанию):
//!   ALKASH3D_BENCH_ENTITIES=<n>   — число вращающихся кубов-инстансов (по
//!                                    умолчанию 64, без физики)
//!   ALKASH3D_BENCH_PHYSICS=<n>    — число падающих физических сфер через
//!                                    Inertial (по умолчанию 0 — физика не
//!                                    грузится вообще)
//!   ALKASH3D_BENCH_FRAMES=<n>     — сколько кадров ЗАСЧИТЫВАТЬ в отчёт
//!                                    (после разогрева), по умолчанию 600
//!   ALKASH3D_BENCH_WARMUP=<n>     — кадров разогрева ДО начала замера, по
//!                                    умолчанию 60
//!
//! Переменные окружения — автонагрузка (`ALKASH3D_BENCH_AUTO=1`):
//!   ALKASH3D_BENCH_AUTO_START=<n>       — с чего начать (по умолчанию 50)
//!   ALKASH3D_BENCH_AUTO_STEP=<n>        — сколько кубов добавлять за шаг
//!                                          (по умолчанию 50)
//!   ALKASH3D_BENCH_AUTO_STEP_PHYSICS=<n> — сколько физ. сфер добавлять за
//!                                          шаг (по умолчанию 0 — растим
//!                                          только рендер-нагрузку)
//!   ALKASH3D_BENCH_AUTO_TARGET_FPS=<n>  — остановиться, когда средний FPS
//!                                          шага упадёт ниже (по умолчанию 30)
//!   ALKASH3D_BENCH_AUTO_MAX_OBJECTS=<n> — жёсткий потолок суммы объектов,
//!                                          дальше которого не растим, даже
//!                                          если FPS всё ещё в норме (по
//!                                          умолчанию 800)
//!   ALKASH3D_BENCH_AUTO_STAGE_FRAMES=<n> — кадров замера на шаг (по
//!                                          умолчанию 120, ~2с на 60 FPS)
//!
//! Общие для обоих режимов:
//!   ALKASH3D_BENCH_NO_SHADOW=1     — выключить shadow mapping
//!   ALKASH3D_BENCH_NO_VOLUMETRIC=1 — выключить volumetric god rays
//!   ALKASH3D_BENCH_NO_BLOOM=1      — выключить bloom
//!
//! Примеры:
//!   `cargo run --release --bin benchmark` — фиксированная лёгкая сцена.
//!   `ALKASH3D_BENCH_AUTO=1 cargo run --release --bin benchmark` — найти
//!   точку излома по FPS, начиная с 50 кубов и добавляя по 50 за шаг.

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};
use alkash3d_rs::input::keys;
use alkash3d_rs::PhysicsConfig;
use std::time::Instant;

const INERTIAL_DLL_PATH: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";

const WINDOW_WIDTH: u32 = 1366;
const WINDOW_HEIGHT: u32 = 768;

const DEFAULT_ENTITIES: u32 = 64;
const DEFAULT_PHYSICS: u32 = 0;
const DEFAULT_FRAMES: u32 = 600;
const DEFAULT_WARMUP: u32 = 60;

const DEFAULT_AUTO_START: u32 = 50;
const DEFAULT_AUTO_STEP: u32 = 50;
const DEFAULT_AUTO_STEP_PHYSICS: u32 = 0;
const DEFAULT_AUTO_TARGET_FPS: u32 = 30;
const DEFAULT_AUTO_MAX_OBJECTS: u32 = 800;
const DEFAULT_AUTO_STAGE_FRAMES: u32 = 120;
/// Кадров разогрева ПОСЛЕ добавления новой партии объектов на сцену, ДО
/// начала замера этого шага — только что заспавненные меши/тела иначе
/// внесли бы одноразовый выброс (первая загрузка instance-буфера/пробуждение
/// тел в Inertial) в статистику шага.
const AUTO_STAGE_WARMUP_FRAMES: u32 = 10;
/// Ограничитель числа шагов автонагрузки — независимая от FPS/MAX_OBJECTS
/// защита от зацикливания, если что-то пошло не так с условиями останова.
const AUTO_MAX_STAGES: u32 = 60;
/// "Предохранитель": если ОДИН кадр (в любом режиме) займёт дольше этого —
/// немедленно останавливаемся, не растим нагрузку дальше и не ждём
/// следующих кадров. См. подробное обоснование в шапке файла — реальный
/// GPU-хендж на этой машине начинался с кадров, раздувающихся на порядки, и
/// лучше остановиться на первом же подозрительно медленном кадре, чем
/// продолжать грузить систему дальше.
const HARD_STOP_FRAME_MS: f32 = 2000.0;

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auto_mode = std::env::var("ALKASH3D_BENCH_AUTO").is_ok();

    println!("==========================================");
    println!("Alkash3D Engine v{} — Benchmark", alkash3d_rs::VERSION);
    println!("==========================================");

    let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    if let Err(e) = engine.init() {
        eprintln!("[BENCH] FAIL: engine.init() вернул ошибку: {:?}", e);
        return Err(e.into());
    }

    if std::env::var("ALKASH3D_BENCH_NO_SHADOW").is_ok() {
        engine.disable_shadows_for_diagnostics();
    }
    if std::env::var("ALKASH3D_BENCH_NO_VOLUMETRIC").is_ok() {
        engine.disable_volumetric_for_diagnostics();
    }
    if std::env::var("ALKASH3D_BENCH_NO_BLOOM").is_ok() {
        engine.disable_bloom_for_diagnostics();
    }

    let exit_code = if auto_mode {
        let cfg = AutoConfig {
            start_entities: env_u32("ALKASH3D_BENCH_AUTO_START", DEFAULT_AUTO_START),
            step_entities: env_u32("ALKASH3D_BENCH_AUTO_STEP", DEFAULT_AUTO_STEP).max(1),
            step_physics: env_u32("ALKASH3D_BENCH_AUTO_STEP_PHYSICS", DEFAULT_AUTO_STEP_PHYSICS),
            target_fps: env_u32("ALKASH3D_BENCH_AUTO_TARGET_FPS", DEFAULT_AUTO_TARGET_FPS) as f32,
            max_objects: env_u32("ALKASH3D_BENCH_AUTO_MAX_OBJECTS", DEFAULT_AUTO_MAX_OBJECTS),
            stage_frames: env_u32("ALKASH3D_BENCH_AUTO_STAGE_FRAMES", DEFAULT_AUTO_STAGE_FRAMES).max(1),
        };
        println!(
            "[BENCH-AUTO] start={} step_entities={} step_physics={} target_fps={:.0} max_objects={} stage_frames={}",
            cfg.start_entities, cfg.step_entities, cfg.step_physics, cfg.target_fps, cfg.max_objects, cfg.stage_frames
        );
        println!(
            "[BENCH-AUTO] ⚠ Автонагрузка сама доведёт сцену до {} объектов (или до просадки FPS/подозрительно медленного \
             кадра, смотря что раньше) — см. предохранитель HARD_STOP_FRAME_MS={:.0}ms в шапке файла.",
            cfg.max_objects, HARD_STOP_FRAME_MS
        );

        let cube_mesh = setup_scene_base(&mut engine);
        if let Err(e) = engine.warm_up_pipelines() {
            eprintln!("[BENCH] WARNING: прогрев PSO не завершился штатно: {:?}", e);
        }
        run_auto_load(&mut engine, cube_mesh, cfg)
    } else {
        let entities = env_u32("ALKASH3D_BENCH_ENTITIES", DEFAULT_ENTITIES);
        let physics_bodies = env_u32("ALKASH3D_BENCH_PHYSICS", DEFAULT_PHYSICS);
        let bench_frames = env_u32("ALKASH3D_BENCH_FRAMES", DEFAULT_FRAMES).max(1);
        let warmup_frames = env_u32("ALKASH3D_BENCH_WARMUP", DEFAULT_WARMUP);
        println!(
            "[BENCH] entities={} physics_bodies={} warmup_frames={} bench_frames={}",
            entities, physics_bodies, warmup_frames, bench_frames
        );
        if entities + physics_bodies > 200 {
            println!(
                "[BENCH] ⚠ Суммарная нагрузка ({} объектов) заметно тяжелее дефолтной — \
                 убедись, что ALKASH3D_GBV НЕ установлена (см. шапку файла).",
                entities + physics_bodies
            );
        }

        let cube_mesh = setup_scene_base(&mut engine);
        spawn_cube_batch(&mut engine, cube_mesh, entities, 0, entities);
        if physics_bodies > 0 {
            setup_physics(&mut engine, physics_bodies);
        }
        println!(
            "[BENCH] Scene ready: {} meshes, {} instances, {} entities",
            engine.meshes.len(),
            engine.mesh_instances.len(),
            engine.scene_entity_count(),
        );

        if let Err(e) = engine.warm_up_pipelines() {
            eprintln!("[BENCH] WARNING: прогрев PSO не завершился штатно: {:?}", e);
        }
        run_loop(&mut engine, warmup_frames, bench_frames)
    };

    engine.shutdown();
    println!("[BENCH] Goodbye!");

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

/// Голая сцена — только пол + освещение + меш куба (без единого инстанса) —
/// общая для обоих режимов: фиксированный режим сразу добавляет все
/// инстансы разом (`spawn_cube_batch`), автонагрузка добавляет их
/// поэтапно из `run_auto_load`.
fn setup_scene_base(engine: &mut AlkashEngine) -> usize {
    println!("\n[BENCH] Setting up base scene...");

    let floor_idx = engine.add_quad(0.0, 0.0, 60.0, 60.0, [0.25, 0.25, 0.3, 1.0]);
    engine.mesh_instances.push(
        MeshInstance::new(floor_idx)
            .at(0.0, 0.0, 0.0)
            .rotated(-1.5708, 0.0, 0.0),
    );

    engine.set_clear_color(0.05, 0.05, 0.1, 1.0);
    engine.set_time_of_day(13.0);

    engine.add_cube(0.5)
}

/// Добавляет `count` кубов-инстансов, начиная с глобального индекса
/// `global_start` — сетка в плоскости XZ считается от `grid_side_basis`
/// (фиксированное число, НЕ пересчитывается на каждый вызов), чтобы ранее
/// заспавненные кубы не сдвигались, когда автонагрузка досыпает новую
/// партию: позиция зависит только от глобального индекса конкретного куба,
/// а не от текущего общего количества.
fn spawn_cube_batch(engine: &mut AlkashEngine, cube_mesh: usize, grid_side_basis: u32, global_start: u32, count: u32) {
    let side = (grid_side_basis as f32).sqrt().ceil().max(1.0) as i32;
    for i in global_start..global_start + count {
        let ix = i as i32 % side;
        let iz = i as i32 / side;
        let x = (ix as f32 - side as f32 * 0.5) * 1.5;
        let z = (iz as f32 - side as f32 * 0.5) * 1.5;
        engine.mesh_instances.push(MeshInstance::new(cube_mesh).at(x, 0.75, z));
    }
}

/// Грузит Inertial (если ещё не загружен) с ёмкостью `max_bodies_hint`
/// (нужна ЗАРАНЕЕ — плагин не умеет расти после инициализации) и спавнит
/// `count` падающих сфер слоями друг над другом (сразу начинают
/// сталкиваться при падении — честная нагрузка на broad/narrow phase, а не
/// просто N невзаимодействующих тел).
fn setup_physics(engine: &mut AlkashEngine, count: u32) -> u32 {
    if engine.physics_stats().is_none() {
        println!("[BENCH] Загружаю Inertial...");
        let config = PhysicsConfig {
            max_bodies: (count + 8) as i32,
            world_size: 100.0,
            cell_size: 4.0,
            solver_iterations: 8,
            use_simd: 0,
        };
        if let Err(e) = engine.init_physics(INERTIAL_DLL_PATH, config) {
            eprintln!(
                "[BENCH] WARNING: не удалось загрузить Inertial ({}): {:?} — физическая часть нагрузки пропущена",
                INERTIAL_DLL_PATH, e
            );
            return 0;
        }
        println!("[BENCH] ✓ Inertial loaded");
    }

    let ball_mesh = engine.add_cube(0.4);
    let side = (count as f32).sqrt().ceil().max(1.0) as i32;
    let mut spawned = 0u32;
    for i in 0..count as i32 {
        let ix = i % side;
        let iy = i / side;
        let x = (ix as f32 - side as f32 * 0.5) * 0.9;
        let z = (iy as f32 - side as f32 * 0.5) * 0.9;
        let y = 15.0 + (iy as f32) * 0.9;
        if engine.spawn_physics_sphere(ball_mesh, x, y, z, 1.0).is_some() {
            spawned += 1;
        }
    }
    println!("[BENCH] ✓ {} физических сфер заспавнено", spawned);
    spawned
}

/// Замеряет `update()`+`render_frame()` отдельно (полезно, чтобы понять,
/// упирается кадр в CPU-логику/физику или в GPU/рендер), плюс общий frame
/// time. Кадры разогрева (`warmup_frames`) не попадают в собранные времена —
/// первые кадры содержат одноразовые расходы (аллокации, ленивая
/// инициализация ресурсов), которые исказили бы устойчивую картину.
fn run_loop(engine: &mut AlkashEngine, warmup_frames: u32, bench_frames: u32) -> i32 {
    println!(
        "\n=== BENCHMARK RUNNING (разогрев {} кадров, замер {} кадров) ===\n",
        warmup_frames, bench_frames
    );

    let mut frame_times_ms: Vec<f32> = Vec::with_capacity(bench_frames as usize);
    let mut update_times_ms: Vec<f32> = Vec::with_capacity(bench_frames as usize);
    let mut render_times_ms: Vec<f32> = Vec::with_capacity(bench_frames as usize);

    let mut frame_count = 0u32;
    let start = Instant::now();
    let mut time = 0.0f32;

    while engine.is_running() {
        engine.process_messages();
        if !engine.is_running() {
            break;
        }
        if engine.input.just_pressed(keys::ESCAPE) {
            println!("[BENCH] ESC — прерываю замер досрочно на кадре {}", frame_count);
            break;
        }

        let now = start.elapsed().as_secs_f32();
        let dt = (now - time).min(0.05);
        time = now;

        for instance in engine.mesh_instances.iter_mut().skip(1) {
            instance.rotation[1] += dt * 0.6;
        }

        let frame_start = Instant::now();

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let camera_pos = [engine.camera.position[0], engine.camera.position[1], engine.camera.position[2]];

        let update_start = Instant::now();
        engine.update(dt, -9.8, camera_pos, view_proj.to_cols_array());
        let update_ms = update_start.elapsed().as_secs_f32() * 1000.0;

        let render_start = Instant::now();
        if let Err(e) = engine.render_frame() {
            eprintln!("[BENCH] FAIL: render_frame() вернул ошибку на кадре {}: {:?}", frame_count, e);
            return 1;
        }
        let render_ms = render_start.elapsed().as_secs_f32() * 1000.0;

        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;

        if frame_ms > HARD_STOP_FRAME_MS {
            eprintln!(
                "[BENCH] ⚠ ПРЕДОХРАНИТЕЛЬ: кадр {} занял {:.0}ms (> {:.0}ms) — останавливаюсь немедленно, не жду больше кадров.",
                frame_count, frame_ms, HARD_STOP_FRAME_MS
            );
            break;
        }

        frame_count += 1;
        if frame_count > warmup_frames {
            frame_times_ms.push(frame_ms);
            update_times_ms.push(update_ms);
            render_times_ms.push(render_ms);
        }

        if frame_count % 60 == 0 {
            let phase = if frame_count <= warmup_frames { "warmup" } else { "measuring" };
            println!("[BENCH] frame {} ({}) — {:.2}ms ({:.0} FPS)", frame_count, phase, frame_ms, 1000.0 / frame_ms.max(0.001));
        }

        if frame_count >= warmup_frames + bench_frames {
            break;
        }
    }

    if frame_times_ms.is_empty() {
        println!("[BENCH] FAIL: ни одного замеренного кадра (окно закрыли/предохранитель сработал до конца разогрева?)");
        return 1;
    }

    print_report("Frame (update+render)", &frame_times_ms);
    print_report("  ├─ update()", &update_times_ms);
    print_report("  └─ render_frame()", &render_times_ms);

    if let Some(stats) = engine.physics_stats() {
        println!(
            "[BENCH] Physics: bodies={} active={} contacts={} pairs={}",
            stats.bodies_count, stats.active_bodies, stats.contacts_count, stats.pairs_count
        );
    }

    println!("[BENCH] PASS: {} кадров замерено без ошибок рендера", frame_times_ms.len());
    0
}

struct AutoConfig {
    start_entities: u32,
    step_entities: u32,
    step_physics: u32,
    target_fps: f32,
    max_objects: u32,
    stage_frames: u32,
}

/// Итог одного шага автонагрузки — используется и для построчного вывода, и
/// для финальной сводной таблицы.
struct StageResult {
    stage: u32,
    entities: u32,
    physics: u32,
    avg_fps: f32,
    low1_fps: f32,
}

/// Поэтапно растит сцену (кубы, опционально физические сферы) и на каждом
/// шаге замеряет `stage_frames` кадров (после короткого пошагового
/// разогрева `AUTO_STAGE_WARMUP_FRAMES`, см. его комментарий). Останавливает
/// рост, как только либо средний FPS шага падает ниже `target_fps`, либо
/// упирается в `max_objects`, либо срабатывает `HARD_STOP_FRAME_MS`
/// (см. шапку файла), либо превышен `AUTO_MAX_STAGES`. В конце печатает
/// таблицу всех пройденных шагов и итог — последнюю нагрузку, на которой FPS
/// был ещё в норме.
fn run_auto_load(engine: &mut AlkashEngine, cube_mesh: usize, cfg: AutoConfig) -> i32 {
    println!("\n=== AUTO-LOAD BENCHMARK RUNNING ===\n");

    let mut entities = 0u32;
    let mut physics = 0u32;
    let mut results: Vec<StageResult> = Vec::new();
    let mut hard_stopped = false;

    for stage in 1..=AUTO_MAX_STAGES {
        let add_entities = cfg.step_entities.min(cfg.max_objects.saturating_sub(entities + physics));
        let want_entities = if stage == 1 { cfg.start_entities.min(cfg.max_objects) } else { add_entities };
        if want_entities > 0 {
            spawn_cube_batch(engine, cube_mesh, cfg.max_objects, entities, want_entities);
            entities += want_entities;
        }
        if cfg.step_physics > 0 && entities + physics < cfg.max_objects {
            let want_physics = cfg.step_physics.min(cfg.max_objects - entities - physics);
            physics += setup_physics(engine, want_physics);
        }

        println!(
            "[BENCH-AUTO] stage {}: entities={} physics={} (total={}) — measuring {} frames...",
            stage, entities, physics, entities + physics, cfg.stage_frames
        );

        match measure_stage(engine, cfg.stage_frames) {
            StageOutcome::HardStop(frame_ms) => {
                eprintln!(
                    "[BENCH-AUTO] ⚠ ПРЕДОХРАНИТЕЛЬ: кадр занял {:.0}ms (> {:.0}ms) на stage {} (total={} объектов) — \
                     останавливаю автонагрузку немедленно, дальше НЕ расту.",
                    frame_ms, HARD_STOP_FRAME_MS, stage, entities + physics
                );
                hard_stopped = true;
                break;
            }
            StageOutcome::RenderError => {
                eprintln!("[BENCH-AUTO] FAIL: render_frame() вернул ошибку на stage {}", stage);
                return 1;
            }
            StageOutcome::WindowClosed => {
                println!("[BENCH-AUTO] окно закрыто/ESC — прерываю автонагрузку на stage {}", stage);
                break;
            }
            StageOutcome::Measured { avg_fps, low1_fps } => {
                println!(
                    "[BENCH-AUTO] stage {}: total={} objects — avg {:.0} FPS | 1% low {:.0} FPS{}",
                    stage,
                    entities + physics,
                    avg_fps,
                    low1_fps,
                    if avg_fps < cfg.target_fps { "  ← ниже target_fps, останавливаюсь" } else { "" }
                );
                results.push(StageResult { stage, entities, physics, avg_fps, low1_fps });

                if avg_fps < cfg.target_fps {
                    break;
                }
                if entities + physics >= cfg.max_objects {
                    println!("[BENCH-AUTO] достигнут ALKASH3D_BENCH_AUTO_MAX_OBJECTS={} при ещё здоровом FPS — останавливаюсь", cfg.max_objects);
                    break;
                }
            }
        }

        if stage == AUTO_MAX_STAGES {
            println!("[BENCH-AUTO] достигнут AUTO_MAX_STAGES={} — останавливаюсь (safety cap)", AUTO_MAX_STAGES);
        }
    }

    println!("\n[BENCH-AUTO] === Сводка по шагам ===");
    println!("[BENCH-AUTO] {:>5} {:>10} {:>10} {:>10} {:>12}", "stage", "entities", "physics", "avg FPS", "1% low FPS");
    for r in &results {
        println!("[BENCH-AUTO] {:>5} {:>10} {:>10} {:>10.0} {:>12.0}", r.stage, r.entities, r.physics, r.avg_fps, r.low1_fps);
    }

    match results.last() {
        Some(last) => {
            println!(
                "\n[BENCH-AUTO] Точка излома: {} объектов ({} кубов + {} физ. тел) — avg {:.0} FPS, 1% low {:.0} FPS",
                last.entities + last.physics, last.entities, last.physics, last.avg_fps, last.low1_fps
            );
        }
        None => {
            println!("\n[BENCH-AUTO] Ни один шаг не был замерен успешно.");
        }
    }

    if hard_stopped { 1 } else { 0 }
}

enum StageOutcome {
    Measured { avg_fps: f32, low1_fps: f32 },
    HardStop(f32),
    RenderError,
    WindowClosed,
}

/// Один шаг автонагрузки: сперва `AUTO_STAGE_WARMUP_FRAMES` кадров без
/// записи в статистику (даём вращающимся инстансам/только что заспавненным
/// физическим телам "устаканиться"), затем `stage_frames` замеряемых
/// кадров. Прерывается немедленно на первом кадре, превысившем
/// `HARD_STOP_FRAME_MS` — не дожидаясь конца шага.
fn measure_stage(engine: &mut AlkashEngine, stage_frames: u32) -> StageOutcome {
    let mut samples_ms: Vec<f32> = Vec::with_capacity(stage_frames as usize);
    let total_frames = AUTO_STAGE_WARMUP_FRAMES + stage_frames;
    let start = Instant::now();
    let mut time = 0.0f32;

    for i in 0..total_frames {
        engine.process_messages();
        if !engine.is_running() {
            return StageOutcome::WindowClosed;
        }
        if engine.input.just_pressed(keys::ESCAPE) {
            return StageOutcome::WindowClosed;
        }

        let now = start.elapsed().as_secs_f32();
        let dt = (now - time).min(0.05);
        time = now;

        for instance in engine.mesh_instances.iter_mut().skip(1) {
            instance.rotation[1] += dt * 0.6;
        }

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let camera_pos = [engine.camera.position[0], engine.camera.position[1], engine.camera.position[2]];

        let frame_start = Instant::now();
        engine.update(dt, -9.8, camera_pos, view_proj.to_cols_array());
        if let Err(_e) = engine.render_frame() {
            return StageOutcome::RenderError;
        }
        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;

        if frame_ms > HARD_STOP_FRAME_MS {
            return StageOutcome::HardStop(frame_ms);
        }

        if i >= AUTO_STAGE_WARMUP_FRAMES {
            samples_ms.push(frame_ms);
        }
    }

    let n = samples_ms.len().max(1);
    let mut sorted = samples_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg_ms: f32 = sorted.iter().sum::<f32>() / n as f32;
    let low1_count = (n as f32 * 0.01).ceil().max(1.0) as usize;
    let low1_avg_ms: f32 = sorted[n.saturating_sub(low1_count)..].iter().sum::<f32>() / low1_count as f32;

    StageOutcome::Measured {
        avg_fps: 1000.0 / avg_ms.max(0.001),
        low1_fps: 1000.0 / low1_avg_ms.max(0.001),
    }
}

/// Печатает avg/min/max FPS + "1% low" (средний FPS по худшим 1% кадров —
/// стандартная метрика "не врёт ли средний FPS о микрофризах", которые
/// плоское среднее легко прячет).
fn print_report(label: &str, samples_ms: &[f32]) {
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = sorted.len();
    let sum: f32 = sorted.iter().sum();
    let avg_ms = sum / n as f32;
    let min_ms = sorted[0];
    let max_ms = sorted[n - 1];

    let low1_count = (n as f32 * 0.01).ceil().max(1.0) as usize;
    let low1_avg_ms: f32 = sorted[n - low1_count..].iter().sum::<f32>() / low1_count as f32;

    println!(
        "[BENCH] {}: avg {:.2}ms ({:.0} FPS) | min {:.2}ms ({:.0} FPS) | max {:.2}ms ({:.0} FPS) | 1% low {:.2}ms ({:.0} FPS) | n={}",
        label,
        avg_ms, 1000.0 / avg_ms.max(0.001),
        min_ms, 1000.0 / min_ms.max(0.001),
        max_ms, 1000.0 / max_ms.max(0.001),
        low1_avg_ms, 1000.0 / low1_avg_ms.max(0.001),
        n,
    );
}
