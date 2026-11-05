//! КРАШ-ТЕСТ — как машина сминается при ударе. Расчёт идёт заранее, мелким шагом
//! (`alkash3d_rs::softbody_car`), удар показывается записью и его можно разглядывать в
//! замедлении. Машина обобщённая, цифры — оценка для такого класса, а не прогноз
//! для конкретного автомобиля.
//!
//! Управление: 1 — стена, 2 — столб по центру, 3 — столб со смещением, 4 — стена с
//! перекрытием 40%; стрелки вверх/вниз — скорость; R — посчитать заново; пробел — пауза;
//! Z/X — медленнее/быстрее; стрелки влево/вправо — по кадрам; ЛКМ — вращать камеру,
//! W/S — ближе/дальше; Esc — отпустить мышь, Shift+Esc — выйти.
//! Отладка: CRASH_TARGET=wall|pole|poleside|offset, CRASH_SPEED=км/ч, CRASH_T=секунды
//! от удара — застыть на этом моменте; CRASH_CAM=рысканье°,наклон°,расстояние;
//! CRASH_FRAMES=N — выйти через N кадров; CRASH_WINDOWED=1 — окном.

#[path = "../msc/mouse.rs"]
mod mouse;
#[path = "../msc/hud.rs"]
#[allow(dead_code)]
mod hud;

use alkash3d_rs::engine::{AlkashEngine, GraphicsSettings, Lens, Mesh, MeshInstance, Vertex};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;
use alkash3d_rs::softbody::Obstacle;
use alkash3d_rs::softbody::StepKernel;
use alkash3d_rs::softbody_car::{simulate_with, CrashRecording, Face, Part, Target};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

/// Сколько физического времени считать после старта, с.
const DURATION: f64 = 0.7;
/// Частота записи: при замедлении в 10 раз хватает с запасом.
const FRAME_RATE: f64 = 500.0;
/// Сколько секунд перед ударом показывать подъезд к препятствию.
const RUN_UP: f32 = 0.15;

/// Вершины куба-параллелепипеда с внешними нормалями.
fn box_mesh(min: [f32; 3], max: [f32; 3], color: [f32; 3], tile: f32) -> Mesh {
    let faces: [([f32; 3], [f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [max[0], min[1], min[2]]),
        ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [min[0], min[1], min[2]]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [min[0], max[1], min[2]]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [min[0], min[1], min[2]]),
        ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [min[0], min[1], max[2]]),
        ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [min[0], min[1], min[2]]),
    ];
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let mut v = Vec::new();
    let mut idx = Vec::new();
    for (n, a, b, o) in faces {
        let base = v.len() as u32;
        let la = a[0] * size[0] + a[1] * size[1] + a[2] * size[2];
        let lb = b[0] * size[0] + b[1] * size[1] + b[2] * size[2];
        for (s, t) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            v.push(Vertex::with_normal_uv(
                o[0] + a[0] * la * s + b[0] * lb * t,
                o[1] + a[1] * la * s + b[1] * lb * t,
                o[2] + a[2] * la * s + b[2] * lb * t,
                n[0], n[1], n[2],
                color[0], color[1], color[2], 1.0,
                s * la / tile, t * lb / tile,
            ));
        }
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::from_vertices_and_indices(&v, &idx).expect("параллелепипед")
}

/// Цилиндр с осью `axis` (0 — x, 1 — y, 2 — z), основание в начале координат по оси.
fn cylinder_mesh(radius: f32, length: f32, axis: usize, centered: bool, color: [f32; 3]) -> Mesh {
    const SEGS: u32 = 24;
    let mut v = Vec::new();
    let mut idx = Vec::new();
    let place = |along: f32, c: f32, s: f32| -> [f32; 3] {
        match axis {
            0 => [along, c, s],
            1 => [c, along, s],
            _ => [c, s, along],
        }
    };
    let offset = if centered { -length * 0.5 } else { 0.0 };
    for i in 0..=SEGS {
        let a = i as f32 / SEGS as f32 * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        for along in [offset, offset + length] {
            let p = place(along, c * radius, s * radius);
            let n = place(0.0, c, s);
            v.push(Vertex::with_normal_uv(p[0], p[1], p[2], n[0], n[1], n[2], color[0], color[1], color[2], 1.0, 0.0, 0.0));
        }
    }
    for i in 0..SEGS {
        let b = i * 2;
        idx.extend([b, b + 1, b + 3, b, b + 3, b + 2, b, b + 3, b + 1, b, b + 2, b + 3]);
    }
    // торцы
    for (along, sign) in [(offset, -1.0f32), (offset + length, 1.0)] {
        let center = v.len() as u32;
        let n = place(sign, 0.0, 0.0);
        let p = place(along, 0.0, 0.0);
        v.push(Vertex::with_normal_uv(p[0], p[1], p[2], n[0], n[1], n[2], color[0], color[1], color[2], 1.0, 0.0, 0.0));
        for i in 0..=SEGS {
            let a = i as f32 / SEGS as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            let q = place(along, c * radius, s * radius);
            v.push(Vertex::with_normal_uv(q[0], q[1], q[2], n[0], n[1], n[2], color[0], color[1], color[2], 1.0, 0.0, 0.0));
        }
        for i in 0..SEGS {
            idx.extend([center, center + 1 + i, center + 2 + i, center, center + 2 + i, center + 1 + i]);
        }
    }
    Mesh::from_vertices_and_indices(&v, &idx).expect("цилиндр")
}

/// Обшивка одной группы граней: вершины каждый кадр считаются из узлов записи.
struct Skin {
    mesh: usize,
    faces: Vec<Face>,
    color: [f32; 3],
    alpha: f32,
    vertices: Vec<Vertex>,
}

impl Skin {
    fn new(engine: &mut AlkashEngine, faces: Vec<Face>, color: [f32; 3], alpha: f32, roughness: f32, metallic: f32) -> Skin {
        let mut vertices = Vec::with_capacity(faces.len() * 4);
        let mut indices = Vec::with_capacity(faces.len() * 6);
        for (n, _) in faces.iter().enumerate() {
            let b = n as u32 * 4;
            indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
            for _ in 0..4 {
                vertices.push(Vertex::with_normal_uv(0.0, 0.0, 0.0, 0.0, 1.0, 0.0, color[0], color[1], color[2], alpha, 0.0, 0.0));
            }
        }
        // пустую группу (например, нет стёкол) рисовать незачем, но меш нужен непустой
        if vertices.is_empty() {
            for _ in 0..3 {
                vertices.push(Vertex::with_normal_uv(0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            }
            indices.extend([0, 1, 2]);
        }
        let mut mesh = Mesh::from_vertices_and_indices(&vertices, &indices).expect("обшивка");
        // меняется каждый кадр
        mesh.cpu_writable = true;
        // вершины меняются, а границы движок считает при создании: машина ездит по
        // сцене, поэтому границы берутся с запасом на всю площадку
        mesh.bounding_center = [2.0, 1.0, 0.0];
        mesh.bounding_radius = 30.0;
        mesh.material_roughness = roughness;
        mesh.material_metallic = metallic;
        if alpha < 1.0 {
            mesh.blend = true;
        }
        let mesh = engine.add_mesh(mesh);
        Skin { mesh, faces, color, alpha, vertices }
    }

    fn update(&mut self, engine: &AlkashEngine, frame: &[f32], broken: &[u32], frame_index: u32, normals: &mut Vec<[f32; 3]>) {
        if self.faces.is_empty() {
            return;
        }
        let node = |i: u32| -> [f32; 3] {
            let i = i as usize * 3;
            [frame[i], frame[i + 1], frame[i + 2]]
        };
        // гладкие нормали по узлам: складки читаются по свету, но без граней сетки
        normals.clear();
        normals.resize(frame.len() / 3, [0.0; 3]);
        let visible = |f: &Face| f.beams.iter().all(|&b| broken[b as usize] > frame_index);
        for f in self.faces.iter().filter(|f| visible(f)) {
            let (p0, p1, p2, p3) = (node(f.nodes[0]), node(f.nodes[1]), node(f.nodes[2]), node(f.nodes[3]));
            let (u, w) = ([p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]], [p3[0] - p1[0], p3[1] - p1[1], p3[2] - p1[2]]);
            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            for &i in &f.nodes {
                let a = &mut normals[i as usize];
                a[0] += n[0];
                a[1] += n[1];
                a[2] += n[2];
            }
        }
        for (k, f) in self.faces.iter().enumerate() {
            let show = visible(f);
            for c in 0..4 {
                let i = f.nodes[c];
                let p = if show { node(i) } else { node(f.nodes[0]) };
                let n = normals[i as usize];
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                self.vertices[k * 4 + c] = Vertex::with_normal_uv(p[0], p[1], p[2], n[0] / l, n[1] / l, n[2] / l, self.color[0], self.color[1], self.color[2], self.alpha, 0.0, 0.0);
            }
        }
        if let Some(m) = engine.meshes.get(self.mesh) {
            let _ = m.update_vertices(&self.vertices);
        }
    }
}

fn target_name(t: Target) -> &'static str {
    match t {
        Target::Wall => "жёсткая стена",
        Target::OffsetWall { .. } => "стена, перекрытие 40%",
        Target::Pole { offset } if offset == 0.0 => "столб по центру",
        Target::Pole { .. } => "столб со смещением",
    }
}

fn start_job(kernel: Option<StepKernel>, target: Target, speed: f64, progress: &Arc<AtomicU32>) -> mpsc::Receiver<CrashRecording> {
    let (tx, rx) = mpsc::channel();
    let progress = progress.clone();
    progress.store(0, Ordering::Relaxed);
    std::thread::spawn(move || {
        let rec = simulate_with(kernel, target, speed, DURATION, FRAME_RATE, |p| progress.store((p * 1000.0) as u32, Ordering::Relaxed));
        let _ = tx.send(rec);
    });
    rx
}

fn parse_list(name: &str) -> Option<Vec<f32>> {
    let s = std::env::var(name).ok()?;
    Some(s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let frame_budget: Option<u32> = std::env::var("CRASH_FRAMES").ok().and_then(|v| v.parse().ok());
    let fixed_t: Option<f32> = std::env::var("CRASH_T").ok().and_then(|v| v.parse().ok());
    let fixed_cam = parse_list("CRASH_CAM").filter(|v| v.len() == 3);
    let mut target = match std::env::var("CRASH_TARGET").as_deref() {
        Ok("pole") => Target::Pole { offset: 0.0 },
        Ok("poleside") => Target::Pole { offset: 0.45 },
        Ok("offset") => Target::OffsetWall { overlap: 0.4 },
        _ => Target::Wall,
    };
    let mut speed: f64 = std::env::var("CRASH_SPEED").ok().and_then(|v| v.parse().ok()).unwrap_or(56.0);

    let mut engine = AlkashEngine::new(1600, 900);
    engine.set_graphics_settings(GraphicsSettings { volumetric: false, msaa: false, render_scale: 0.75, ..GraphicsSettings::default() });
    engine.init()?;
    if std::env::var("CRASH_WINDOWED").is_err() {
        engine.toggle_fullscreen();
    }
    engine.taa = true;
    engine.lens = Lens { vignetting: true, aces_full: true, ..Default::default() };
    engine.metering = true;
    engine.set_day_night_speed(0.0);
    engine.set_time_of_day(11.0);
    engine.camera.near = 0.05;
    engine.camera.far = 400.0;
    engine.camera.fov = 55f32.to_radians();
    engine.set_clear_color(0.42, 0.55, 0.72, 1.0);

    // сцена: асфальт и препятствия
    let mut ground = box_mesh([-100.0, -0.5, -100.0], [100.0, 0.0, 100.0], [0.09, 0.09, 0.095], 4.0);
    ground.material_roughness = 0.9;
    let ground = engine.add_mesh(ground);
    let mut wall = box_mesh([0.0, 0.0, -3.5], [1.0, 2.6, 3.5], [0.42, 0.41, 0.38], 1.0);
    wall.material_roughness = 0.85;
    let wall = engine.add_mesh(wall);
    let mut half_wall = box_mesh([0.0, 0.0, 0.0], [1.0, 2.6, 3.5], [0.42, 0.41, 0.38], 1.0);
    half_wall.material_roughness = 0.85;
    let half_wall = engine.add_mesh(half_wall);
    let mut pole = cylinder_mesh(0.15, 5.0, 1, false, [0.3, 0.3, 0.3]);
    pole.material_roughness = 0.7;
    let pole = engine.add_mesh(pole);
    let mut wheel = cylinder_mesh(0.3, 0.2, 2, true, [0.015, 0.015, 0.015]);
    wheel.material_roughness = 0.8;
    let wheel = engine.add_mesh(wheel);

    let mut hud = hud::Hud::load(&mut engine, concat!(env!("CARGO_MANIFEST_DIR"), "/../data/codec/font.bin")).map_err(|e| eprintln!("[КРАШ] шрифт: {}", e)).ok();

    if let Err(e) = engine.warm_up_pipelines() {
        eprintln!("[КРАШ] прогрев PSO: {:?}", e);
    }

    // считает ядро на Fortran из inertial.dll; без неё — эталон на Rust с тем же результатом
    let kernel = match alkash3d_rs::softbody_kernels::step_kernel() {
        Ok(k) => {
            println!("[КРАШ] расчёт: Fortran (inertial.dll)");
            Some(k)
        }
        Err(e) => {
            eprintln!("[КРАШ] {} — считаю на Rust", e);
            None
        }
    };
    let progress = Arc::new(AtomicU32::new(0));
    let mut job = None;
    let mut recording: Option<CrashRecording> = None;
    // снимок из командной строки — ждём расчёт сразу, кадры считать не с чего
    if fixed_t.is_some() {
        let t0 = std::time::Instant::now();
        recording = Some(simulate_with(kernel, target, speed, DURATION, FRAME_RATE, |_| {}));
        println!("[КРАШ] расчёт {:.1} с", t0.elapsed().as_secs_f32());
    } else {
        job = Some(start_job(kernel, target, speed, &progress));
    }

    // обшивка по первому расчёту: её узлы от настроек удара не зависят
    let template = alkash3d_rs::softbody_car::build_car(5e-5);
    // все грани — наружу: нормаль считается по порядку узлов
    let outward = |mut f: Face| -> Face {
        let p = |i: usize| template.body.pos[f.nodes[i] as usize];
        let n = (p(2) - p(0)).cross(p(3) - p(1));
        let center = (p(0) + p(1) + p(2) + p(3)) * 0.25;
        if n.dot(center - alkash3d_rs::softbody::v3(2.1, 0.65, 0.0)) < 0.0 {
            f.nodes.swap(1, 3);
        }
        f
    };
    let pick = |part: Part| -> Vec<Face> { template.faces.iter().copied().filter(|f| f.part == part).map(outward).collect() };
    let mut skins = vec![
        Skin::new(&mut engine, pick(Part::Paint), [0.32, 0.03, 0.035], 1.0, 0.32, 0.0),
        Skin::new(&mut engine, pick(Part::Underbody), [0.03, 0.03, 0.03], 1.0, 0.7, 0.0),
        Skin::new(&mut engine, pick(Part::Glass), [0.02, 0.03, 0.035], 0.55, 0.05, 0.0),
    ];
    drop(template);

    let mut mouse = mouse::Mouse::new();
    let (mut yaw, mut pitch, mut dist) = (1.25f32, 0.3f32, 7.5f32);
    if let Some(c) = &fixed_cam {
        yaw = c[0].to_radians();
        pitch = c[1].to_radians();
        dist = c[2];
    }
    let mut slow = 0.1f32;
    let mut paused = false;
    let mut play_t = -RUN_UP;
    let mut normals = Vec::new();
    let mut frames = 0u32;
    let start = std::time::Instant::now();
    let mut last = 0.0f32;

    while engine.is_running() {
        engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).min(0.1);
        last = now;

        // ---- управление ----
        let shift = engine.input.is_down(keys::SHIFT);
        if engine.input.just_pressed(keys::ESCAPE) {
            if shift || !mouse.captured {
                break;
            }
            mouse.set_captured(false);
        }
        if engine.input.just_pressed(keys::MOUSE_LEFT) && !mouse.captured {
            mouse.set_captured(true);
        }
        let (mx, my) = mouse.delta();
        if mouse.captured {
            yaw -= mx * 0.003;
            pitch = (pitch + my * 0.003).clamp(-0.05, 1.4);
        }
        if engine.input.is_down(keys::W) {
            dist = (dist - dt * 4.0).max(2.5);
        }
        if engine.input.is_down(keys::S) {
            dist = (dist + dt * 4.0).min(30.0);
        }
        let mut rerun = engine.input.just_pressed(keys::R);
        for (key, t) in [(0x31u32, Target::Wall), (0x32, Target::Pole { offset: 0.0 }), (0x33, Target::Pole { offset: 0.45 }), (0x34, Target::OffsetWall { overlap: 0.4 })] {
            if engine.input.just_pressed(key) {
                target = t;
                rerun = true;
            }
        }
        if engine.input.just_pressed(keys::ARROW_UP) {
            speed = (speed + 5.0).min(120.0);
            rerun = true;
        }
        if engine.input.just_pressed(keys::ARROW_DOWN) {
            speed = (speed - 5.0).max(5.0);
            rerun = true;
        }
        if rerun && job.is_none() && fixed_t.is_none() {
            job = Some(start_job(kernel, target, speed, &progress));
        }
        if engine.input.just_pressed(keys::SPACE) {
            paused = !paused;
        }
        if engine.input.just_pressed(0x5A) {
            slow = (slow * 0.5).max(0.0125);
        }
        if engine.input.just_pressed(0x58) {
            slow = (slow * 2.0).min(1.0);
        }
        let step = 1.0 / FRAME_RATE as f32;
        if engine.input.is_down(keys::ARROW_LEFT) {
            play_t -= step;
            paused = true;
        }
        if engine.input.is_down(keys::ARROW_RIGHT) {
            play_t += step;
            paused = true;
        }

        if let Some(rx) = &job {
            if let Ok(rec) = rx.try_recv() {
                recording = Some(rec);
                job = None;
                play_t = -RUN_UP;
                paused = false;
            }
        }

        // ---- кадр записи ----
        let mut out: Vec<MeshInstance> = Vec::new();
        out.push(MeshInstance::new(ground));
        let mut center = Vec3::new(2.1, 0.7, 0.0);
        let mut lines: Vec<String> = Vec::new();
        if let Some(rec) = &recording {
            if let Some(t) = fixed_t {
                play_t = t;
            } else if !paused {
                play_t += dt * slow;
            }
            let hold = 1.5 * slow;
            let end = DURATION as f32;
            if play_t > end + hold && job.is_none() && fixed_t.is_none() {
                play_t = -RUN_UP;
            }
            let index = ((play_t.max(0.0) * FRAME_RATE as f32) as usize).min(rec.frames.len() - 1);
            let frame = &rec.frames[index];
            for s in skins.iter_mut() {
                s.update(&engine, frame, &rec.beam_break_frame, index as u32, &mut normals);
                out.push(MeshInstance::new(s.mesh));
            }
            // до удара — та же первая поза, сдвинутая назад по ходу
            let run_up = play_t.min(0.0) * (rec.report.speed_kmh / 3.6) as f32;
            for m in out.iter_mut().skip(1) {
                m.position[0] += run_up;
            }
            let node = |i: u32| -> Vec3 {
                let i = i as usize * 3;
                Vec3::new(frame[i], frame[i + 1], frame[i + 2])
            };
            let row = rec.sensor.len() / 5;
            let avg = |ids: &[u32]| -> Vec3 { ids.iter().map(|&i| node(i)).fold(Vec3::ZERO, |a, b| a + b) / ids.len().max(1) as f32 };
            let (rear, front) = (avg(&rec.sensor[..row]), avg(&rec.sensor[rec.sensor.len() - row..]));
            let forward = (front - rear).normalize_or_zero();
            let heading = (-forward.z).atan2(forward.x);
            for w in &rec.wheels {
                let p = node(w.node);
                out.push(MeshInstance::new(wheel).at(p.x + run_up, p.y, p.z).rotated(0.0, heading, 0.0));
            }
            center = (rear + front) * 0.5 + Vec3::new(run_up + 0.9, 0.5, 0.0);
            match rec.obstacle {
                Obstacle::Block { min, .. } => {
                    if min.z > -3.0 {
                        out.push(MeshInstance::new(half_wall).at(min.x as f32, 0.0, min.z as f32));
                    } else {
                        out.push(MeshInstance::new(wall).at(min.x as f32, 0.0, 0.0));
                    }
                }
                Obstacle::Pole { x, z, .. } => out.push(MeshInstance::new(pole).at(x as f32, 0.0, z as f32)),
            }
            let r = &rec.report;
            let g = rec.g_by_frame.get(index.saturating_sub(1)).copied().unwrap_or(0.0).max(0.0);
            let v = rec.speed_by_frame.get(index.saturating_sub(1)).copied().unwrap_or(r.speed_kmh as f32 / 3.6);
            lines.push(format!("{} · {:.0} км/ч · {:.0} кг · замедление показа ×{:.0}", target_name(target), r.speed_kmh, r.mass, 1.0 / slow));
            lines.push(format!("t {:+.0} мс   скорость салона {:.0} км/ч   замедление {:.0} g", play_t * 1000.0, v * 3.6, g));
            lines.push(format!("итог: ход салона после удара {:.2} м, щит вдавило на {:.2} м, пик {:.0} g, среднее {:.0} g, порвано балок {}, разбито стёкол {}", r.dynamic_crush, r.firewall_intrusion.max(0.0), r.peak_g, r.mean_g, r.broken_beams, r.broken_glass_faces));
            lines.push(format!("энергия {:.0} кДж: деформация {:.0}%, тепло и трение {:.0}%; баланс сходится до {:.1}%", r.kinetic_in / 1e3, r.plastic / r.kinetic_in * 100.0, (r.damping + r.friction + r.contact_damping + r.rupture) / r.kinetic_in * 100.0, r.balance_error * 100.0));
        }
        if job.is_some() {
            lines.push(format!("считаю удар: {} · {:.0} км/ч — {}%", target_name(target), speed, progress.load(Ordering::Relaxed) / 10));
        }
        lines.push("1 стена · 2 столб · 3 столб сбоку · 4 смещение · ↑↓ скорость · пробел пауза · Z/X замедление · ←→ кадр · ЛКМ камера · W/S ближе".to_string());

        // ---- камера ----
        let eye = center + Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos()) * dist;
        engine.camera.position = eye;
        engine.camera.target = center;

        // ---- экспозиция и надписи ----
        let exposure = (0.1 / engine.scene_luminance().unwrap_or(0.5).max(0.01)).clamp(0.05, 6.0);
        let settings = engine.light_global_settings.get_or_insert(alkash3d_rs::GlobalLightSettings {
            master_intensity: 1.0,
            shadow_quality: 2,
            shadow_distance: 40.0,
            cascade_count: 3,
            volumetric_enabled: 0,
            volumetric_quality: 0,
            bloom_intensity: 0.02,
            exposure: 1.0,
            gamma: 2.2,
        });
        settings.exposure = exposure;
        if let Some(h) = hud.as_mut() {
            h.set_exposure(exposure);
        }
        engine.mesh_instances = out;
        if let Some(h) = &hud {
            let mut out = std::mem::take(&mut engine.mesh_instances);
            for (i, line) in lines.iter().enumerate() {
                h.draw(&engine, &mut out, hud::Text { text: line, x: -0.97, y: 0.93 - i as f32 * 0.045, size: 0.024, color: [1.0, 1.0, 1.0], align: -1.0, backing: 0.45 });
            }
            engine.mesh_instances = out;
        }

        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let cp = engine.camera.position;
        engine.update(dt, -9.81, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        // день, ясно: солнце с небесным рассеянным светом, отношение примерно 5:1
        let tc = &mut engine.transform_constants;
        tc.light_dir = [0.35, -0.8, 0.5, 0.0];
        tc.light_color = [1.0, 0.96, 0.9, 3.0];
        tc.ambient_color = [0.55, 0.7, 1.0, 0.6];
        if let Err(e) = engine.render_frame() {
            eprintln!("[КРАШ] render_frame: {:?}", e);
            break;
        }
        frames += 1;
        if frame_budget.is_some_and(|n| frames >= n) {
            break;
        }
    }
    engine.shutdown();
    Ok(())
}
