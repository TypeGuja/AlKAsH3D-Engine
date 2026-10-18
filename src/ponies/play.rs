//! Режим игры (`PONIES_PLAY=имя|номер`): играешь за одну официальную пони, остальные
//! стоят кругом и живут сами. Всё управление — из игры:
//!
//! - WASD — идти (относительно камеры), Shift — бежать, ПКМ + мышь — крутить камеру,
//!   колесо — ближе/дальше, Tab — играть за следующую пони, Esc — выход;
//! - 1–5 форма глаз, 6 настроение рта, B брови по кругу, H румянец, 0 — нейтрально;
//! - F — магия рога (единороги) / раскрытые крылья (пегасы, аликорны), если есть в пакете;
//! - G — спрятать/показать крылья; Space (с крыльями) — взлететь, держать — набирать
//!   высоту, отпустить — плавно планировать вниз; крылья машут (чаще при подъёме);
//! - Enter — чат: набрать фразу латиницей (буквы, пробел, Backspace) и Enter —
//!   пони проговаривает её ртом (фонемы DHX по буквам).
//!
//! NPC живут сами: бродят вокруг своего места, расходятся друг с другом, болтают
//! (рот по фонемам), меняют настроение, оборачиваются и улыбаются подошедшему игроку.
//!
//! Грива и хвост — пружины: отстают при разгоне и поворотах, раскачиваются при
//! остановке, подрагивают в такт шагу (`Pose::tail_extra`/`mane_extra`).

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};
use alkash3d_rs::input::keys;
use alkash3d_rs::math::Vec3;

use crate::official::{self, Expression, Eyes, Mood, Phoneme, Puppet};

const WALK_SPEED: f32 = 0.9;
const RUN_SPEED: f32 = 2.4;
/// Длина шага — доля роста: фаза шага из пройденного пути (ноги не скользят).
const STRIDE: f32 = 0.5;
const VK_TAB: u32 = 0x09;
const VK_ENTER: u32 = 0x0D;
const VK_BACK: u32 = 0x08;
/// Радиус «личного пространства» (доля ширины пони) — пони не проходят друг сквозь друга.
const PERSONAL: f32 = 0.3;
/// Как далеко NPC отходит от своего места.
const WANDER: f32 = 1.6;
/// Что NPC говорят сами (рот по буквам).
const PHRASES: &[&str] = &["hello", "nice day", "oh my", "what", "hmm", "yay", "really", "see you", "how are you", "wonderful", "привет", "ой", "ага"];

/// Пружина с затуханием: x'' = k (цель − x) − c x'.
#[derive(Clone, Copy, Default)]
struct Spring {
    x: f32,
    v: f32,
}

impl Spring {
    fn step(&mut self, target: f32, k: f32, c: f32, dt: f32) -> f32 {
        // Несколько подшагов — устойчиво при любом dt.
        let n = ((dt / 0.004).ceil() as usize).max(1);
        let h = dt / n as f32;
        for _ in 0..n {
            self.v += (k * (target - self.x) - c * self.v) * h;
            self.x += self.v * h;
        }
        self.x
    }
}

/// Одна пони в мире (и игрок, и остальные).
struct Pony {
    pup: usize,
    at: Vec3,
    yaw: f32,
    speed: f32,
    walk_phase: f32,
    seed: f32,
    tail: Spring,
    mane: Spring,
    prev_yaw: f32,
    prev_speed: f32,
    face: Expression,
    /// Речь: фонемы по очереди, время на фонему.
    speech: Vec<Phoneme>,
    speech_t: f32,
    /// NPC: когда сменить настроение.
    mood_t: f32,
    /// Вариант B: магия рога (единороги) или раскрытые крылья (пегасы, аликорны).
    alt: bool,
    /// NPC: своё место, куда идёт сейчас, сколько ещё стоять, когда заговорить.
    home: Vec3,
    goal: Option<Vec3>,
    idle_t: f32,
    chat_t: f32,
    /// Полёт: вертикальная скорость, фаза взмаха, крылья спрятаны.
    vy: f32,
    flap: f32,
    no_wings: bool,
}

impl Pony {
    fn new(pup: usize, at: Vec3, yaw: f32, seed: f32) -> Self {
        Pony { pup, at, yaw, speed: 0.0, walk_phase: 0.0, seed, tail: Spring::default(), mane: Spring::default(), prev_yaw: yaw, prev_speed: 0.0, face: Expression::default(), speech: Vec::new(), speech_t: 0.0, mood_t: 3.0 + seed, alt: false, home: at, goal: None, idle_t: 2.0 + 4.0 * hash(seed + 0.3), chat_t: 5.0 + 10.0 * hash(seed + 0.7), vy: 0.0, flap: 0.0, no_wings: false }
    }

    fn facing(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }

    /// Физика гривы/хвоста за кадр: разгон, поворот, шаг.
    fn sway(&mut self, dt: f32, height: f32) {
        let yaw_rate = wrap(self.yaw - self.prev_yaw) / dt.max(1e-4);
        let accel = (self.speed - self.prev_speed) / dt.max(1e-4);
        self.prev_yaw = self.yaw;
        self.prev_speed = self.speed;
        let run = (self.speed / RUN_SPEED).clamp(0.0, 1.0);
        let bounce = if self.speed > 0.05 { (2.0 * self.walk_phase).sin() * (0.04 + 0.08 * run) } else { 0.0 };
        // Хвост: на ходу отходит назад-вверх, при разгоне — сильнее, в поворот — вбок (в плоскости — тот же угол).
        let tail_target = 0.25 * run + 0.06 * (accel / 3.0).clamp(-1.0, 1.0) + 0.05 * yaw_rate.clamp(-3.0, 3.0) + bounce;
        self.tail.step(tail_target, 60.0, 5.5, dt);
        let mane_target = 0.12 * run + 0.04 * (accel / 3.0).clamp(-1.0, 1.0) + 0.6 * bounce;
        self.mane.step(mane_target, 90.0, 7.0, dt);
        let _ = height;
    }

    fn say(&mut self, text: &str) {
        self.speech = text.chars().flat_map(phonemes).collect();
        self.speech.push(Phoneme::Rest);
        self.speech_t = 0.0;
    }

    /// Текущая фонема речи (≈11 звуков в секунду); None — молчит.
    fn speaking(&mut self, dt: f32) -> Option<Phoneme> {
        if self.speech.is_empty() {
            return None;
        }
        self.speech_t += dt;
        let i = (self.speech_t * 11.0) as usize;
        if i >= self.speech.len() {
            self.speech.clear();
            return None;
        }
        Some(self.speech[i])
    }
}

/// Буква -> фонемы рта DHX (латиница и кириллица).
fn phonemes(c: char) -> Vec<Phoneme> {
    let c = c.to_lowercase().next().unwrap_or(' ');
    let p = match c {
        'a' | 'e' | 'i' | 'y' | 'а' | 'е' | 'и' | 'ы' | 'э' | 'я' | 'ё' | 'й' => Phoneme::Aei,
        'o' | 'u' | 'w' | 'q' | 'о' | 'у' | 'ю' => Phoneme::Rou,
        's' | 'z' | 'c' | 'x' | 'j' | 'с' | 'з' | 'ц' | 'ж' | 'ш' | 'щ' | 'ч' => Phoneme::S,
        'l' | 'r' | 'л' | 'р' => Phoneme::L,
        'f' | 'v' | 'ф' | 'в' => Phoneme::F,
        't' | 'd' | 'n' | 'k' | 'g' | 'h' | 'т' | 'д' | 'н' | 'к' | 'г' | 'х' => Phoneme::Th,
        'm' | 'b' | 'p' | 'м' | 'б' | 'п' => Phoneme::Mbp,
        _ => Phoneme::Rest,
    };
    vec![p]
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Простой генератор (без зависимостей) для настроений NPC.
fn hash(x: f32) -> f32 {
    ((x * 12.9898).sin() * 43758.547).fract().abs()
}

pub fn run(engine: &mut AlkashEngine, puppets: &[Puppet], tint: f32, ground: Option<usize>, blob: usize, who: &str, frame_budget: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    if puppets.is_empty() {
        return Err("PONIES_PLAY: нет официальных паппетов".into());
    }
    let first = who.parse::<usize>().ok().filter(|&i| i < puppets.len()).or_else(|| puppets.iter().position(|p| p.name == who)).unwrap_or(0);
    // Игрок в центре, остальные — кругом лицом к центру.
    let others: Vec<usize> = (0..puppets.len()).filter(|&i| i != first).collect();
    let radius = (2.5f32).max(others.len() as f32 * 0.55);
    let mut ponies = vec![Pony::new(first, Vec3::ZERO, 0.0, 0.0)];
    for (k, &i) in others.iter().enumerate() {
        let a = k as f32 / others.len() as f32 * std::f32::consts::TAU;
        let at = Vec3::new(a.sin() * radius, 0.0, a.cos() * radius);
        ponies.push(Pony::new(i, at, a + std::f32::consts::PI, k as f32 * 2.399 + 1.0));
    }
    let mut player = 0usize;
    println!("[PLAY] играешь за {} ({} пони вокруг)", puppets[first].name, others.len());

    let (mut cam_yaw, mut cam_pitch, mut cam_dist) = (std::f32::consts::PI, 0.25f32, 3.0f32);
    let mut last_mouse: Option<[f32; 2]> = None;
    let mut typing: Option<String> = None;
    let start = std::time::Instant::now();
    let mut last = 0.0f32;
    let mut frames = 0u32;
    while engine.is_running() {
        engine.process_messages();
        let now = start.elapsed().as_secs_f32();
        let dt = (now - last).clamp(0.0001, 0.1);
        last = now;
        let input = &engine.input;
        if input.just_pressed(keys::ESCAPE) {
            break;
        }

        // --- чат
        if let Some(text) = typing.as_mut() {
            for vk in 0x41..=0x5A {
                if input.just_pressed(vk) {
                    text.push(char::from_u32(vk + 0x20).unwrap());
                }
            }
            if input.just_pressed(keys::SPACE) {
                text.push(' ');
            }
            if input.just_pressed(VK_BACK) {
                text.pop();
            }
            if input.just_pressed(VK_ENTER) {
                let t = typing.take().unwrap_or_default();
                if !t.trim().is_empty() {
                    println!("[PLAY] {}: {}", puppets[ponies[player].pup].name, t);
                    ponies[player].say(&t);
                }
            }
        } else {
            if input.just_pressed(VK_ENTER) {
                typing = Some(String::new());
                println!("[PLAY] чат: печатай, Enter — сказать");
            }
            // --- лицо игрока
            let face = &mut ponies[player].face;
            for (k, e) in Eyes::ALL.iter().enumerate() {
                if input.just_pressed(0x31 + k as u32) {
                    face.eyes = *e;
                }
            }
            if input.just_pressed(0x36) {
                face.mood = if face.mood == Mood::Happy { Mood::Sad } else { Mood::Happy };
            }
            if input.just_pressed(0x42) {
                face.brow = match face.brow { None => Some(0), Some(b) if b < 12 => Some(b + 1), _ => None };
            }
            if input.just_pressed(0x48) {
                face.blush = if face.blush.is_some() { None } else { Some(1) };
            }
            if input.just_pressed(0x30) {
                *face = Expression::default();
            }
            if input.just_pressed(0x46) {
                let p = &mut ponies[player];
                if puppets[p.pup].has_variant_b() {
                    p.alt = !p.alt;
                    println!("[PLAY] F: {}", if p.alt { "магия / крылья — вкл" } else { "выкл" });
                } else {
                    println!("[PLAY] F: у {} нет магии/крыльев в пакете", puppets[p.pup].name);
                }
            }
            if input.just_pressed(0x47) {
                let p = &mut ponies[player];
                p.no_wings = !p.no_wings;
                println!("[PLAY] G: крылья {}", if p.no_wings { "спрятаны" } else { "видны" });
            }
            if input.just_pressed(VK_TAB) {
                player = (player + 1) % ponies.len();
                println!("[PLAY] теперь играешь за {}", puppets[ponies[player].pup].name);
            }
        }

        // --- камера
        if input.is_down(keys::MOUSE_RIGHT) {
            let m = input.mouse_ndc;
            if let Some(p) = last_mouse {
                cam_yaw -= (m[0] - p[0]) * 2.2;
                cam_pitch = (cam_pitch - (m[1] - p[1]) * 1.4).clamp(-0.2, 1.2);
            }
            last_mouse = Some(m);
        } else {
            last_mouse = None;
        }
        cam_dist = (cam_dist - input.wheel * 0.25).clamp(1.2, 9.0);

        // --- движение игрока
        let fwd = Vec3::new(-cam_yaw.sin(), 0.0, -cam_yaw.cos());
        let right = Vec3::new(fwd.z, 0.0, -fwd.x) * -1.0;
        let mut wish = Vec3::ZERO;
        if typing.is_none() {
            if input.is_down(keys::W) { wish += fwd; }
            if input.is_down(keys::S) { wish -= fwd; }
            if input.is_down(keys::D) { wish += right; }
            if input.is_down(keys::A) { wish -= right; }
        }
        let run = input.is_down(keys::SHIFT);
        {
            let p = &mut ponies[player];
            let target_speed = if wish.length_squared() > 0.0 { if run { RUN_SPEED } else { WALK_SPEED } } else { 0.0 };
            p.speed += (target_speed - p.speed) * (1.0 - (-8.0 * dt).exp());
            if wish.length_squared() > 0.0 {
                let want = wish.x.atan2(wish.z);
                p.yaw += wrap(want - p.yaw) * (1.0 - (-10.0 * dt).exp());
            }
            let step = p.facing() * p.speed * dt;
            p.at += step;
            // Полёт: только с видимыми крыльями.
            let flying = typing.is_none() && input.is_down(keys::SPACE) && !p.no_wings && puppets[p.pup].has_wings();
            if flying {
                p.vy += (1.6 - p.vy) * (1.0 - (-4.0 * dt).exp());
            } else if p.at.y > 0.0 {
                // Планирование: падает медленно, если крылья есть.
                let glide = if !p.no_wings && puppets[p.pup].has_wings() { -0.7 } else { -6.0 };
                p.vy += (glide - p.vy) * (1.0 - (-2.5 * dt).exp());
            }
            p.at.y = (p.at.y + p.vy * dt).min(6.0);
            if p.at.y <= 0.0 {
                p.at.y = 0.0;
                p.vy = 0.0;
            }
        }

        // --- NPC: бродят у своего места, смотрят на подошедшего игрока, болтают, меняют настроение
        let pat = ponies[player].at;
        for (k, p) in ponies.iter_mut().enumerate() {
            if k == player {
                continue;
            }
            let d = pat - p.at;
            let near = d.length() < 2.2;
            if near {
                // Игрок рядом: остановиться и повернуться к нему.
                p.goal = None;
                p.idle_t = p.idle_t.max(1.5);
                let want = d.x.atan2(d.z);
                p.yaw += wrap(want - p.yaw) * (1.0 - (-3.0 * dt).exp());
            }
            let mut target_speed = 0.0;
            if let Some(g) = p.goal {
                let to = g - p.at;
                if to.length() < 0.1 {
                    p.goal = None;
                    p.idle_t = 3.0 + 7.0 * hash(now + p.seed * 3.1);
                } else {
                    let want = to.x.atan2(to.z);
                    p.yaw += wrap(want - p.yaw) * (1.0 - (-4.0 * dt).exp());
                    // Сначала развернуться, потом идти.
                    target_speed = WALK_SPEED * 0.6 * wrap(want - p.yaw).cos().max(0.0);
                }
            } else if !near {
                p.idle_t -= dt;
                if p.idle_t <= 0.0 {
                    let a = hash(now * 1.7 + p.seed) * std::f32::consts::TAU;
                    let r = WANDER * hash(now * 2.3 + p.seed * 5.0).sqrt();
                    p.goal = Some(p.home + Vec3::new(a.sin() * r, 0.0, a.cos() * r));
                }
            }
            p.speed += (target_speed - p.speed) * (1.0 - (-6.0 * dt).exp());
            p.at += p.facing() * p.speed * dt;
            p.chat_t -= dt;
            if p.chat_t <= 0.0 {
                p.say(PHRASES[(hash(now + p.seed * 7.0) * PHRASES.len() as f32) as usize % PHRASES.len()]);
                p.chat_t = 8.0 + 14.0 * hash(now + p.seed);
            }
            if near && p.face.mood == Mood::Sad && hash(now.floor() + p.seed) < 0.02 {
                p.face.mood = Mood::Happy;
            }
            p.mood_t -= dt;
            if p.mood_t <= 0.0 {
                let r = hash(now + p.seed);
                p.face = if r < 0.55 {
                    Expression::default()
                } else {
                    Expression { eyes: Eyes::ALL[(r * 97.0) as usize % 5], mood: if hash(r) < 0.7 { Mood::Happy } else { Mood::Sad }, ..Expression::default() }
                };
                p.mood_t = 4.0 + 8.0 * hash(r + 1.0);
            }
        }

        // --- пони не проходят друг сквозь друга (расталкиваем попарно)
        for a in 0..ponies.len() {
            for b in a + 1..ponies.len() {
                let ra = puppets[ponies[a].pup].width * PERSONAL;
                let rb = puppets[ponies[b].pup].width * PERSONAL;
                let mut d = ponies[b].at - ponies[a].at;
                d.y = 0.0;
                let l = d.length();
                if l < ra + rb && l > 1e-4 {
                    let push = d * ((ra + rb - l) / l * 0.5);
                    ponies[a].at -= push;
                    ponies[b].at += push;
                }
            }
        }

        // --- камера за игроком
        let target = ponies[player].at + Vec3::new(0.0, puppets[ponies[player].pup].height * 0.7, 0.0);
        let cam = target + Vec3::new(cam_yaw.sin() * cam_pitch.cos(), cam_pitch.sin(), cam_yaw.cos() * cam_pitch.cos()) * cam_dist;
        engine.camera.position = Vec3::new(cam.x, cam.y.max(0.15), cam.z);
        engine.camera.target = target;
        engine.camera.up = Vec3::Y;

        // --- кадр
        let mut instances = match ground { Some(g) => vec![MeshInstance::new(g)], None => Vec::new() };
        let cam_pos = engine.camera.position;
        for p in ponies.iter_mut() {
            let pup = &puppets[p.pup];
            let airborne = p.at.y > 0.01;
            if airborne {
                // Взмахи: 2.5 Гц, при подъёме 4 Гц.
                p.flap += dt * std::f32::consts::TAU * if p.vy > 0.2 { 4.0 } else { 2.5 };
            }
            if p.speed > 0.02 && !airborne {
                p.walk_phase += p.speed * dt / (STRIDE * pup.height) * std::f32::consts::PI;
            }
            p.sway(dt, pup.height);
            let speech = p.speaking(dt);
            // Тень на земле: в полёте меньше и бледнее.
            let fade = 1.0 / (1.0 + p.at.y);
            instances.push(MeshInstance::new(blob).at(p.at.x, 0.01, p.at.z).scaled(pup.width * 0.6 * fade.sqrt(), 1.0, pup.width * 0.35 * fade.sqrt()).tinted(1.0, 1.0, 1.0, 0.45 * fade));
            if let Some(mut pose) = pup.pose_v(p.at, p.facing(), cam_pos, now, p.seed, p.alt) {
                pose.expr = Expression { blink: pose.expr.blink, ..p.face };
                if let Some(ph) = speech {
                    pose.expr.phoneme = ph;
                }
                if p.speed > 0.05 && !airborne {
                    pose.walk = Some(p.walk_phase);
                }
                pose.no_wings = p.no_wings;
                if airborne {
                    pose.wing_extra = 0.35 * (0.5 + 0.5 * p.flap.sin());
                }
                pose.tail_extra = p.tail.x;
                pose.mane_extra = p.mane.x;
                pup.draw_pose_ex(engine, &mut instances, &pose, p.at, tint, 1.0);
            }
        }
        engine.mesh_instances = instances;
        let view_proj = engine.camera.projection_matrix() * engine.camera.view_matrix();
        let cp = engine.camera.position;
        engine.update(dt, -9.81, [cp.x, cp.y, cp.z], view_proj.to_cols_array());
        if let Err(e) = engine.render_frame() {
            eprintln!("[PLAY] render_frame: {:?}", e);
            break;
        }
        frames += 1;
        if frame_budget.is_some_and(|b| frames >= b) {
            println!("[PLAY] PASS: {} кадров, {:.1} FPS", frames, frames as f32 / now.max(1e-3));
            break;
        }
    }
    let _ = official::FEATURE_BLINK;
    Ok(())
}
