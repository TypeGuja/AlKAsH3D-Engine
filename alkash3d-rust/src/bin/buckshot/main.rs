//! Buckshot Roulette на AlKAsH3D — порт игры целиком: меню, заставка в
//! клубе, подпись отказа, три набора раундов, дилер, предметы, смерть и
//! оживление, финальная дуэль, концовка с чемоданом, "рай".
//!
//! Сцены, модели, текстуры, анимации, звук и тексты — из распакованной
//! игры (её нужно купить; данные лежат ВНЕ репозитория, по умолчанию в
//! `data/buckshot/scene` в корне AlKAsH3D-Engine, в .gitignore; путь меняется переменной `BUCKSHOT_DATA`).
//! Как они туда попадают — см. шапку `scene_file.rs`. Код написан заново:
//! правила и ИИ — `game.rs`, граф сцены — `graph.rs`, анимации — `anim.rs`,
//! порт скриптов-"режиссёров" — `flow.rs`, звук — `sound.rs`, текст — `text.rs`.
//!
//! Управление — мышью, как в оригинале: клики по объектам на столе и
//! экранным кнопкам; Esc — в меню.
//! Переменные окружения: `BUCKSHOT_LIGHT_SCALE` (яркость фонарей, 1.0),
//! `BUCKSHOT_EXPOSURE` (экспозиция, 1.0), `BUCKSHOT_FRAMES` (выйти после N
//! кадров — для автопроверки).

mod anim;
mod flow;
mod game;
mod graph;
mod scene_file;
mod sound;
mod text;

use std::collections::HashMap;

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::{GlobalLightSettings, LightConfig};
use windows::core::Interface;

use anim::Anims;
use flow::{Ctx, Mode, SceneRt, Target};
use graph::Graph;
use scene_file::{Kind, SceneFile};
use sound::Sounds;
use text::{Font, Rgb, TextLine};

const FIRSTFIRES_DLL_PATH: &str = "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll";
const DEFAULT_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/buckshot/scene");

fn load_scene(engine: &mut AlkashEngine, path: &str, dynamic: &[&str], light_scale: f32) -> Result<SceneRt, String> {
    let scene = SceneFile::load(path)?;
    let graph = Graph::build(engine, &scene, dynamic, light_scale);
    let anims = Anims::new(&scene, &graph);
    let camera = scene.nodes.iter().position(|n| n.kind == Kind::Camera).ok_or_else(|| format!("{}: нет камеры", path))?;
    Ok(SceneRt { graph, anims, camera })
}

fn load_strings(path: &str) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, v)| (k.to_string(), v.replace("\\n", "\n")))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("BUCKSHOT_DATA").unwrap_or_else(|_| DEFAULT_DATA_DIR.to_string());
    let env_f32 = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let light_scale = env_f32("BUCKSHOT_LIGHT_SCALE", 1.0);
    let exposure = env_f32("BUCKSHOT_EXPOSURE", 1.0);
    let frame_budget: Option<u32> = std::env::var("BUCKSHOT_FRAMES").ok().and_then(|v| v.parse().ok());
    println!("[BUCKSHOT] данные: {}", data_dir);

    let mut font = Font::load(&format!("{}/font.bin", data_dir))?;
    let strings = load_strings(&format!("{}/strings_ru.tsv", data_dir));

    let mut engine = AlkashEngine::new(1600, 900);
    engine.init()?;
    let device_ptr = alkash3d_rs::get_device()?.as_raw();
    let config = LightConfig { max_lights: 128, tile_size: 16, far_plane: 300.0, lod_distances: [60.0, 150.0, 300.0], grid_cell_size: 30.0 };
    if let Err(e) = engine.init_lights(FIRSTFIRES_DLL_PATH, device_ptr, config) {
        eprintln!("[BUCKSHOT] FirstFires не загрузился ({:?}) — без фонарей будет темно. Собери alkash3d-FirstFires в release.", e);
    }
    if let Err(e) = engine.init_audio() {
        eprintln!("[BUCKSHOT] звук недоступен: {:?}", e);
    }

    let menu = load_scene(&mut engine, &format!("{}/menu.altex", data_dir), &["Camera"], light_scale)?;
    let main_scene = load_scene(&mut engine, &format!("{}/world.altex", data_dir), flow::MAIN_DYNAMIC, light_scale)?;
    let heaven = load_scene(&mut engine, &format!("{}/heaven.altex", data_dir), &["Camera"], light_scale)?;

    let black_quad = engine.add_mesh(Mesh::from_vertices_and_indices(
        &[
            Vertex::with_normal(-1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Vertex::with_normal(-1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
        ],
        &[0, 1, 2, 0, 2, 3],
    )?);

    engine.set_clear_color(0.0, 0.0, 0.0, 1.0);
    engine.set_time_of_day(0.0);
    engine.set_day_night_speed(0.0);
    engine.camera.near = 0.1;
    engine.camera.far = 600.0;

    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let sounds = Sounds::new(&data_dir);
    let mut ctx = Ctx::new(engine, vec![menu, main_scene, heaven], sounds, strings, seed);
    let cues = ctx.enter_menu();
    ctx.start(cues);

    // Геометрии место в видеопамяти (кольца скиннинга остаются: их пишет CPU).
    match ctx.engine.meshes_to_video_memory() {
        Ok(bytes) => println!("[BUCKSHOT] геометрия в видеопамяти: {:.0} МБ", bytes as f64 / 1e6),
        Err(e) => eprintln!("[BUCKSHOT] геометрия осталась в оперативной памяти: {:?}", e),
    }

    if let Err(e) = ctx.engine.warm_up_pipelines() {
        eprintln!("[BUCKSHOT] прогрев PSO: {:?}", e);
    }

    let start = std::time::Instant::now();
    let mut last = 0.0f32;
    let mut frames = 0u32;
    let mut shake_rng = game::Rng::new(7);
    let autoplay = std::env::var("BUCKSHOT_AUTOPLAY").is_ok();
    let mut auto = AutoPlay { wait: 0.0, rng: game::Rng::new(seed), held: false, expect: None, self_chance: env_f32("BUCKSHOT_AUTOPLAY_SELF", 0.25) };

    while ctx.engine.is_running() && !ctx.exit {
        ctx.engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).min(0.1);
        last = now;
        if autoplay {
            auto.step(&mut ctx, dt);
        }

        if ctx.engine.input.just_pressed(keys::ESCAPE) && !matches!(ctx.waiting_for(), Some(Mode::Menu) | Some(Mode::Heaven)) {
            ctx.abort_to_menu();
        }

        // Мышь: сначала экранные кнопки, потом объекты сцены под курсором.
        let mouse = ctx.engine.input.mouse_ndc;
        let aspect = ctx.engine.camera.aspect;
        let mut hover = ctx.buttons().into_iter().find(|(text, x, y, size, _)| {
            let w = font.ndc_width(text, *size, aspect);
            (mouse[0] - x).abs() <= w * 0.5 + 0.02 && mouse[1] >= y - 0.01 && mouse[1] <= y + size * 2.0
        }).map(|b| b.4);
        if hover.is_none() {
            hover = if ctx.waiting_for() == Some(Mode::AnyKeyToMenu) { Some(Target::Anywhere) } else { ctx.pick(mouse) };
        }
        ctx.set_hover(hover);
        if ctx.engine.input.just_pressed(keys::MOUSE_LEFT) {
            if let Some(t) = hover {
                ctx.click(t);
            }
        }
        if autoplay {
            auto.after(&mut ctx, hover);
        }

        ctx.update(dt);

        let active = ctx.active;
        for (i, s) in ctx.scenes.iter_mut().enumerate() {
            if i == active {
                s.graph.update_globals();
                s.graph.update_skins(&mut ctx.engine);
            }
            s.graph.sync_lights(&mut ctx.engine, i == active);
        }

        // Камера — узел Camera активной сцены (+ тряска CameraShaker).
        {
            let s = &ctx.scenes[active];
            let g = s.graph.global[s.camera];
            let (_, rot, mut pos) = g.to_scale_rotation_translation();
            if ctx.shake > 0.0 {
                let r = |rng: &mut game::Rng| rng.f32() * 2.0 - 1.0;
                pos += Vec3::new(r(&mut shake_rng), r(&mut shake_rng), r(&mut shake_rng)) * (ctx.shake * 0.12);
            }
            let fov = s.graph.nodes[s.camera].fov;
            let cam = &mut ctx.engine.camera;
            cam.position = pos;
            cam.target = pos + rot * Vec3::NEG_Z;
            cam.up = rot * Vec3::Y;
            cam.fov = if fov > 1.0 { fov.to_radians() } else { 65.5f32.to_radians() };
        }

        let mut instances = Vec::new();
        ctx.scenes[active].graph.push_instances(&mut instances);
        // Label3D сцены (имена на счётчике, "ДИЛЕР"/"ВЫ", бланк, консоль дефибриллятора).
        for i in ctx.scenes[active].graph.world_labels() {
            let g = &ctx.scenes[active].graph;
            if !g.vis[i] {
                continue;
            }
            let node = &g.nodes[i];
            let Some(l) = node.label.clone() else { continue };
            let tint = node.tint;
            if tint[3] * l.color[3] < 0.05 {
                continue;
            }
            let raw = node.label_text.clone().unwrap_or_else(|| l.text.clone());
            if raw.trim().is_empty() {
                continue;
            }
            // Label3D в Godot сам переводит текст-ключ (auto translate).
            let text = ctx.strings.get(raw.trim()).cloned().unwrap_or(raw);
            let m = g.global[i];
            let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            let color = Rgb(lin(l.color[0]) * tint[0], lin(l.color[1]) * tint[1], lin(l.color[2]) * tint[2]);
            for (k, line) in text.lines().enumerate() {
                let h = l.font_size * l.pixel_size;
                let shifted = m * alkash3d_rs::math::Mat4::from_translation(alkash3d_rs::math::Vec3::new(0.0, -(k as f32) * h * 1.2, 0.0));
                font.draw_world(&mut ctx.engine, &mut instances, line, shifted, h, color, l.align, l.billboard != 0);
            }
        }
        draw_hud(&mut font, &mut ctx, &mut instances, black_quad);
        if ctx.black {
            let (forward, _, _) = text::camera_basis(&ctx.engine);
            let p = ctx.engine.camera.position + forward * (ctx.engine.camera.near * 2.0);
            let (rx, ry, rz) = text::facing_camera(&ctx.engine);
            instances.push(MeshInstance::new(black_quad).at(p.x, p.y, p.z).rotated(rx, ry, rz).scaled(5.0, 5.0, 5.0));
        }
        ctx.engine.mesh_instances = instances;

        ctx.engine.light_global_settings = Some(GlobalLightSettings {
            master_intensity: 1.0,
            shadow_quality: 2,
            shadow_distance: 60.0,
            cascade_count: 3,
            volumetric_enabled: 0,
            volumetric_quality: 0,
            bloom_intensity: 0.5,
            exposure: (exposure * ctx.brightness).max(0.001),
            gamma: 2.2,
        });

        let view_proj = ctx.engine.camera.projection_matrix() * ctx.engine.camera.view_matrix();
        let cp = ctx.engine.camera.position;
        ctx.engine.update(dt, -9.8, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        // Солнца и неба в этих сценах нет (в Godot-окружении ambient = 0).
        ctx.engine.transform_constants.light_color[3] = 0.0;
        ctx.engine.transform_constants.ambient_color = [0.0, 0.0, 0.0, 1.0];

        if let Err(e) = ctx.engine.render_frame() {
            eprintln!("[BUCKSHOT] render_frame: {:?}", e);
            break;
        }

        frames += 1;
        if frames == 1 {
            println!("[BUCKSHOT] первый кадр за {:.1} c", now);
        }
        if frame_budget.is_some_and(|b| frames >= b) {
            println!("[BUCKSHOT] PASS: {} кадров, {:.1} FPS", frames, frames as f32 / now.max(0.001));
            break;
        }
    }

    ctx.engine.shutdown();
    Ok(())
}

const WHITE: Rgb = Rgb(0.92, 0.92, 0.88);
const DIM: Rgb = Rgb(0.6, 0.6, 0.55);
const GOLD: Rgb = Rgb(1.0, 0.8, 0.35);

fn draw_hud(font: &mut Font, ctx: &mut Ctx, out: &mut Vec<MeshInstance>, backing: usize) {
    let mut lines: Vec<(String, f32, f32, f32, Rgb, f32)> = Vec::new();

    for (text, x, y, size, target) in ctx.buttons() {
        let color = if ctx.hover == Some(target) { GOLD } else { WHITE };
        lines.push((text, x, y, size, color, 0.0));
    }
    if let Some((title, desc)) = ctx.hover_text() {
        lines.push((title, 0.0, -0.78, 0.05, GOLD, 0.0));
        for (i, l) in desc.lines().enumerate() {
            lines.push((l.to_string(), 0.0, -0.85 - i as f32 * 0.06, 0.04, WHITE, 0.0));
        }
    }

    if let Some(d) = &ctx.dialogue {
        let shown: String = d.text.chars().take(((ctx.time - d.started) / d.delay).max(0.0) as usize).collect();
        for (i, l) in shown.lines().enumerate() {
            lines.push((l.to_string(), 0.0, -0.55 - i as f32 * 0.09, 0.06, WHITE, 0.0));
        }
    }


    for (i, l) in ctx.ending_lines.iter().enumerate() {
        lines.push((l.clone(), -0.9, 0.6 - i as f32 * 0.09, 0.05, WHITE, -1.0));
    }

    if let Some(h) = &ctx.hint {
        lines.push((h.clone(), 0.0, -0.9, 0.04, DIM, 0.0));
    }

    for (text, x, y, size, color, align) in lines {
        if text.trim().is_empty() {
            continue;
        }
        let line = TextLine { text: &text, x, y, size, color, align };
        font.draw_backing(&ctx.engine, out, &line, backing);
        font.draw(&mut ctx.engine, out, &line);
    }
}

/// `BUCKSHOT_AUTOPLAY=1`: сам "кликает", когда игра ждёт ввода —
/// прогон всей партии без человека (для проверки и скриншотов).
struct AutoPlay {
    wait: f32,
    rng: game::Rng,
    held: bool,
    expect: Option<Target>,
    /// Доля выстрелов в себя (`BUCKSHOT_AUTOPLAY_SELF`, по умолчанию 0.25).
    self_chance: f32,
}

impl AutoPlay {
    fn step(&mut self, ctx: &mut Ctx, dt: f32) {
        let Some(mode) = ctx.waiting_for() else {
            self.wait = 0.0;
            return;
        };
        self.wait += dt;
        if self.wait < 1.2 {
            return;
        }
        self.wait = 0.0;
        let targets = ctx.targets();
        let pick = match mode {
            Mode::Waiver => {
                if targets.contains(&Target::Waiver) {
                    Some(Target::Waiver)
                } else if ctx.typed.len() < 6 {
                    Some(Target::Key(b"claude"[ctx.typed.len()] as char))
                } else {
                    Some(Target::KeyEnter)
                }
            }
            Mode::Menu => Some(Target::Start),
            Mode::Turn => {
                let items: Vec<Target> = targets.iter().copied().filter(|t| matches!(t, Target::Item(_))).collect();
                if !items.is_empty() && self.rng.coin() { Some(items[0]) } else { Some(Target::Shotgun) }
            }
            Mode::Holding => Some(if self.rng.f32() < self.self_chance { Target::ShootSelf } else { Target::ShootDealer }),
            Mode::Heaven => Some(Target::ToMenu),
            _ => targets.first().copied(),
        };
        // Как живой игрок: навести курсор на цель и нажать левую кнопку —
        // так проверяется и выбор лучом, и экранные кнопки.
        if let Some(t) = pick {
            match ctx.target_ndc(t) {
                Some(ndc) => {
                    ctx.engine.input.mouse_ndc = ndc;
                    ctx.engine.input.on_key_down(keys::MOUSE_LEFT);
                    self.held = true;
                    self.expect = Some(t);
                }
                None => println!("[AUTOPLAY] цель {:?} вне экрана", t),
            }
        }
    }

    /// После кадра: отпустить кнопку и сверить, что под курсором была нужная цель.
    fn after(&mut self, ctx: &mut Ctx, hovered: Option<Target>) {
        if self.held {
            ctx.engine.input.on_key_up(keys::MOUSE_LEFT);
            self.held = false;
        }
        if let Some(t) = self.expect.take() {
            if hovered != Some(t) {
                println!("[AUTOPLAY] промах: целился в {:?}, под курсором {:?}", t, hovered);
            }
        }
    }
}
