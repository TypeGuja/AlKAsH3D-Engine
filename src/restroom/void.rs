//! Пустота после падения (сцена `void.altex` + `void.alfar` от `void_scene`):
//! стол из оригинала вдали, над ним — оригинальные прожекторы на штативах;
//! слева от стола (если сидеть за ним) — пробковая доска в темноте.
//!
//! Что здесь происходит:
//! - ружьё стоит у стола, прислонённое к краю (на стол его потом положит
//!   пони-единорог — пока оно просто стоит);
//! - на столе лежит бланк отказа от претензий: посмотреть на него, E —
//!   машинка поднимается (камера — оригинальная анимация "pickup waiver"),
//!   имя набирается с клавиатуры, Enter — подпись; имя уходит на панель
//!   здоровья, и перед доской загораются два прожектора;
//! - панель здоровья и плашка с патронами — логика стола (`table.rs`); пока
//!   раунда нет, их показ — отладочная F5;
//! - пони — официальные паппеты DHX (пакет `official.mlpo` или `RESTROOM_PONIES`, `ponies/official.rs`)
//!   с idle-анимацией; кто и где стоит — файл `ponies.txt` рядом со сценой
//!   (строка `имя x z угол [масштаб]`: x, z — в единицах сцены, угол — куда
//!   смотрит, градусы: 0 — на +X, 90 — на +Z, 180 — на игрока со стороны
//!   дилера; масштаб — во сколько раз больше настоящего роста, по умолчанию
//!   `PONY_SCALE`: в настоящем росте пони едва видно из-за стола).
//!   Паппеты без освещения, поэтому их яркость идёт за прожекторами стола
//!   и за расстоянием до него. До начала партии пони нет: когда сюжет
//!   зовёт `start_ponies` (отладка — F9), она выходит из темноты из-за
//!   спины дилера к своему месту (`PONY_WALK_FROM` дальше по её взгляду назад);
//! - вещи (гильзы, предметы) — физика (`items.rs`, Inertial), кровь и раны
//!   пони — `blood.rs`. Пока игры нет, отладка: F6 — бросить предмет на
//!   стол, F7 — гильза из ружья, F8 — выстрел дробью в пони-дилера (брызги,
//!   пятна на столе и полу, пучок ран и рваная рана с подтёками; чем больше
//!   попаданий — тем потрёпаннее), F9 — пони выходят.
//!
//! Яркость ламп считает Lua через универсальный рантайм движка
//! (`alkash3d-luascript`, DLL): на каждую лампу своё Lua-состояние из одного
//! файла `buckshot-equestria/scripts/void_lamps.lua`. Протокол рантайма
//! числовой, используется так:
//!   `update(dt, x, y, z, номер, t, 0) -> (яркость, 0, 0, 1)` — x, y, z —
//!   где стоит лампа, номер — 1..N (1–4 — стол, 5–6 — доска), t — секунды
//!   с появления в пустоте;
//!   `on_event(тип, a, b, c, d)` — события игры (`event`), тип 0 = Custom:
//!   a = 1 — всплеск мерцания, a = 2 — бланк подписан (свет на доску).
//! Нет DLL или скрипта — лампы стола плавно загораются, доски — после подписи.

use alkash3d_rs::engine::{AlkashEngine, MeshInstance};
use alkash3d_rs::math::{Mat4, Quat, Vec3};
use alkash3d_rs::{ScriptConfig, ScriptContext, ScriptEvent, ScriptEventType, ScriptingPlugin};

use crate::anim::Anims;
use crate::blood::Blood;
use crate::items::{self, Items};
use crate::collide::Collider;
use crate::graph::{Graph, Trs};
use crate::scene_file::SceneFile;
use crate::sound::Sounds;
use crate::official::{self, Puppet, Wound};
use crate::table::{HealthCounter, ShellBox, Waiver, WaiverStage};

const LUA_DLL: &str = "../alkash3d-luascript/target/release/alkash3d_luascript.dll";
const LAMPS_LUA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../buckshot-equestria/scripts/void_lamps.lua");
const OFFICIAL_PACK: &str = "G:/mlp_characters/mlp_2d/engine/official.mlpo";
/// Единиц сцены в метре (Buckshot: 1 единица ≈ 10 см).
const UNITS_PER_M: f32 = 10.0;
/// Нет `ponies.txt` — Твайлайт на месте дилера, лицом к игроку.
const DEFAULT_PONIES: &str = "twilight_sparkle 21 0 180
";
/// Пони крупнее настоящего роста: Твайлайт ~1.8 м, как дилер оригинала.
const PONY_SCALE: f32 = 1.7;
/// Раны на одной пони (старые уходят).
const MAX_WOUNDS: usize = 90;
/// Выход пони к столу: откуда (единиц за её местом), с какой скоростью (единиц/с).
const PONY_WALK_FROM: f32 = 60.0;
const PONY_WALK_SPEED: f32 = 11.0;
/// Свет стола на пони: полный ближе `LIT_NEAR` от центра стола, к `LIT_FAR` — темнота.
const LIT_NEAR: f32 = 28.0;
const LIT_FAR: f32 = 70.0;
const LAMPS: &str = "lamps";
/// Лампы с этого номера светят на доску (до подписи бланка погашены).
const FIRST_BOARD_LAMP: usize = 5;
const SHOTGUN: &str = "tabletop parent/shotgun main parent";
/// Ружьё у стола: приклад на полу у края стола со стороны игрока, ствол
/// вверх с наклоном на столешницу (градусы от вертикали).
const GUN_FOOT: Vec3 = Vec3::new(-15.6, 0.0, -6.5);
const GUN_LEAN: f32 = 20.0;
/// Приклад и дуло ружья на столе (мир оригинала) — по ним ось ружья.
const GUN_STOCK: Vec3 = Vec3::new(-8.3, -0.3, -1.35);
const GUN_MUZZLE: Vec3 = Vec3::new(4.6, -0.35, 1.6);
/// До бланка дотянуться: расстояние от глаз и радиус "попадания" взгляда.
const REACH: f32 = 26.0;
const FORM_RADIUS: f32 = 3.5;
/// Переход камеры от глаз игрока к камере бланка и обратно (с).
const BLEND: f32 = 0.6;

struct Lamp {
    pos: Vec3,
    light: usize,
    glass: usize,
    energy: f32,
    script: Option<u32>,
    power: f32,
}

pub struct Void {
    pub graph: Graph,
    pub anims: Anims,
    pub collider: Collider,
    pub spawn: Vec3,
    pub spawn_yaw: f32,
    lamps: Vec<Lamp>,
    lua: Option<ScriptingPlugin>,
    script_path: String,
    t: f32,
    frame: u64,
    counter: HealthCounter,
    shells: ShellBox,
    waiver: Waiver,
    /// Сколько уже идёт переход камеры (к бланку и обратно).
    blend: f32,
    /// Когда подписали бланк (для запасного света доски без скрипта).
    signed_at: Option<f32>,
    seat: Option<(Vec3, f32, f32)>,
    rng: u32,
    puppets: Vec<Puppet>,
    /// Раны каждой пони (по индексу в `ponies`) и сколько в неё попали.
    wounds: Vec<Vec<(Wound, f32)>>,
    hits: Vec<u32>,
    items: Items,
    blood: Option<Blood>,
    /// Где камера (последний кадр) — ракурс пони при попадании.
    cam: Vec3,
    /// Когда пони начинают выходить к столу (None — партия не началась).
    pony_enter: Option<f32>,
    table_center: Vec3,
    /// Пони: какой паппет, где стоит (ноги), куда смотрит, во сколько раз крупнее.
    ponies: Vec<(usize, Vec3, Vec3, f32)>,
    pony_tint: f32,
    /// Отладка: `RESTROOM_VOID_SIGN=ИМЯ` — бланк подписывается сам,
    /// `RESTROOM_VOID_ROUND=1` — после подписи сам показ плашки и панели.
    auto_sign: Option<String>,
    auto_round: bool,
    /// Отладка: `RESTROOM_VOID_DEMO=маска` — сам бросает вещи и стреляет в
    /// пони: 2 — предмет, 4 — гильза, 8 — выстрел (1 или пусто — всё).
    demo: Option<f32>,
    demo_mask: u32,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Камера, которую пустота навязывает вместо глаз игрока.
pub struct CameraOverride {
    pub pos: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub fov: f32,
}

impl Void {
    pub fn load(engine: &mut AlkashEngine, path: &str, light_scale: f32) -> Result<Void, String> {
        let scene = SceneFile::load(path)?;
        let mut graph = Graph::build(engine, &scene, &[LAMPS, SHOTGUN], light_scale);
        let mut anims = Anims::new(&scene, &graph);
        graph.update_globals();
        let mut tris = Vec::new();
        crate::collect_tris(&scene, &graph, 0, &mut tris, &|_| true);
        let collider = Collider::new(tris);

        let sp = graph.global[graph.node("spawn")];
        let fwd = -sp.z_axis.truncate();
        let spawn = sp.w_axis.truncate();
        let spawn_yaw = fwd.x.atan2(-fwd.z);

        let mut lamps = Vec::new();
        for k in 1.. {
            let Some(light) = graph.find(&format!("{}/lamp {}/light", LAMPS, k)) else { break };
            let glass = graph.node(&format!("{}/lamp {}/glass", LAMPS, k));
            let energy = graph.nodes[light].light_energy;
            let pos = graph.global[graph.node(&format!("{}/lamp {}", LAMPS, k))].w_axis.truncate();
            lamps.push(Lamp { pos, light, glass, energy, script: None, power: 0.0 });
        }
        let script_path = LAMPS_LUA.to_string();
        let lua = match ScriptingPlugin::load(LUA_DLL, ScriptConfig { max_scripts: 64 }) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("[VOID] Lua-рантайм не загрузился ({}) — лампы без скрипта", e);
                None
            }
        };
        // Пони: кто и где — из ponies.txt (иначе Твайлайт за столом дилера).
        let floor = spawn.y;
        let list = std::fs::read_to_string(std::path::Path::new(path).with_file_name("ponies.txt")).unwrap_or_else(|_| DEFAULT_PONIES.to_string());
        let mut places: Vec<(String, Vec3, Vec3, f32)> = Vec::new();
        for l in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let f: Vec<&str> = l.split_whitespace().collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok());
            match (f.first(), num(1), num(2), num(3)) {
                (Some(name), Some(x), Some(z), Some(deg)) => {
                    let a = deg.to_radians();
                    places.push((name.to_string(), Vec3::new(x, floor, z), Vec3::new(a.cos(), 0.0, a.sin()), num(4).unwrap_or(PONY_SCALE)));
                }
                _ => eprintln!("[VOID] ponies.txt: не разобрано (нужно `имя x z угол [масштаб]`): {}", l),
            }
        }
        let names: Vec<&str> = places.iter().map(|p| p.0.as_str()).collect();
        let (pony_tint, puppets) = if names.is_empty() {
            (1.0, Vec::new())
        } else {
            let pack = std::env::var("RESTROOM_PONIES").unwrap_or_else(|_| OFFICIAL_PACK.to_string());
            official::read_pack(engine, &pack, Some(&names)).unwrap_or_else(|e| {
                eprintln!("[VOID] пони не загружены: {}", e);
                (1.0, Vec::new())
            })
        };
        let mut ponies = Vec::new();
        for (name, at, facing, scale) in places {
            match puppets.iter().position(|p| p.name == name) {
                Some(i) => ponies.push((i, at, facing, scale)),
                None => eprintln!("[VOID] в пакете нет пони '{}'", name),
            }
        }
        let counter = HealthCounter::new(&mut graph);
        let items = Items::new(engine);
        let blood = Blood::new(engine);
        if blood.is_none() {
            eprintln!("[VOID] текстуры крови не создались — без крови");
        }
        let waiver = Waiver::new(&mut graph, &mut anims);
        println!("[VOID] {}: ламп {}, треугольников {}", path, lamps.len(), collider.len());
        let mut v = Void {
            graph,
            anims,
            collider,
            spawn,
            spawn_yaw,
            lamps,
            lua,
            script_path,
            t: 0.0,
            frame: 0,
            counter,
            shells: ShellBox::new(),
            waiver,
            blend: 0.0,
            signed_at: None,
            seat: None,
            rng: 0x1234_5679,
            puppets,
            wounds: vec![Vec::new(); ponies.len()],
            hits: vec![0; ponies.len()],
            items,
            blood,
            cam: Vec3::ZERO,
            pony_enter: None,
            table_center: Vec3::new(1.25, spawn.y, 0.0),
            ponies,
            pony_tint,
            auto_sign: std::env::var("RESTROOM_VOID_SIGN").ok().filter(|s| !s.is_empty()),
            auto_round: std::env::var("RESTROOM_VOID_ROUND").is_ok(),
            demo: std::env::var("RESTROOM_VOID_DEMO").is_ok().then_some(0.0),
            demo_mask: std::env::var("RESTROOM_VOID_DEMO").ok().and_then(|v| v.parse().ok()).filter(|&m: &u32| m > 1).unwrap_or(14),
        };
        v.reset();
        Ok(v)
    }

    /// Всё как при первом появлении: свет погашен, скрипты ламп заново
    /// (файл перечитывается), бланк на столе, ружьё у стола.
    pub fn reset(&mut self) {
        self.t = 0.0;
        self.blend = 0.0;
        self.signed_at = None;
        self.seat = None;
        self.graph.reset();
        self.graph.spawned.clear();
        self.graph.update_globals();
        self.counter = HealthCounter::new(&mut self.graph);
        self.shells = ShellBox::new();
        self.waiver = Waiver::new(&mut self.graph, &mut self.anims);
        self.lean_shotgun();
        for w in &mut self.wounds {
            w.clear();
        }
        self.hits.iter_mut().for_each(|h| *h = 0);
        // Демо сразу выпускает пони — иначе стрелять не в кого.
        self.pony_enter = self.demo.map(|_| 1.0);
        if let Some(b) = self.blood.as_mut() {
            b.clear();
        }
        let Some(lua) = self.lua.as_mut() else { return };
        for (k, l) in self.lamps.iter_mut().enumerate() {
            if let Some(id) = l.script.take() {
                lua.destroy_script(id);
            }
            l.script = lua.create_script_with_source(k as u64 + 1, &self.script_path);
            l.power = 0.0;
        }
    }

    /// Ружьё со стола — к его краю, прикладом на пол.
    fn lean_shotgun(&mut self) {
        let g = &mut self.graph;
        let i = g.node(SHOTGUN);
        let floor = self.spawn.y;
        let along = (GUN_MUZZLE - GUN_STOCK).normalize();
        let lean = GUN_LEAN.to_radians();
        let up = Vec3::new(lean.sin(), lean.cos(), 0.0);
        let turn = Quat::from_rotation_arc(along, up);
        let foot = Vec3::new(GUN_FOOT.x, floor + 0.3, GUN_FOOT.z);
        let world = Mat4::from_translation(foot) * Mat4::from_quat(turn) * Mat4::from_translation(-GUN_STOCK) * g.global[i];
        let parent = g.nodes[i].parent.map(|p| g.global[p]).unwrap_or(Mat4::IDENTITY);
        g.nodes[i].local = Trs::from_mat(parent.inverse() * world);
    }

    /// Событие игры всем лампам (тип 0 = Custom, данные — по соглашению скрипта).
    pub fn event(&mut self, data: [f32; 4]) {
        let Some(lua) = self.lua.as_mut() else { return };
        for (k, l) in self.lamps.iter().enumerate() {
            if let Some(id) = l.script {
                let e = ScriptEvent { event_type: ScriptEventType::Custom as u32, source_entity: 0, target_entity: k as u64 + 1, data };
                lua.dispatch_event(id, &e);
            }
        }
    }

    /// Игрок занят бланком — ходить и крутить головой нельзя.
    pub fn busy(&self) -> bool {
        self.waiver.camera(&self.graph).is_some() || self.blend > 0.0
    }

    /// Смотрит ли игрок на бланк (на столе, в пределах досягаемости).
    pub fn looking_at_form(&self, eye: Vec3, dir: Vec3) -> bool {
        if self.waiver.stage != WaiverStage::Idle {
            return false;
        }
        let to = self.waiver.form_pos(&self.graph) - eye;
        let along = to.dot(dir);
        along > 0.0 && along < REACH && (to - dir * along).length() < FORM_RADIUS
    }

    /// Подсказка внизу экрана.
    pub fn hint(&self, eye: Vec3, dir: Vec3) -> Option<&'static str> {
        if self.looking_at_form(eye, dir) {
            return Some("[E] Подписать отказ от претензий");
        }
        (self.waiver.stage == WaiverStage::Typing).then_some("Введите имя — Enter, стереть — Backspace")
    }

    /// Надпись по центру снизу (плашка с патронами).
    pub fn caption(&self) -> Option<String> {
        self.shells.caption().map(str::to_string)
    }

    /// Камера бланка (с плавным переходом от глаз игрока `eye`/`fwd`).
    pub fn camera(&self, eye: Vec3, fwd: Vec3, fov: f32) -> Option<CameraOverride> {
        let k = smooth(self.blend / BLEND);
        if k <= 0.0 {
            return None;
        }
        let (m, cam_fov) = self.waiver.camera(&self.graph).unwrap_or_else(|| {
            // Бланк подписан, камера возвращается к глазам игрока.
            (crate::graph::Trs { pos: eye, rot: Quat::from_mat3(&alkash3d_rs::math::Mat3::from_cols(fwd.cross(Vec3::Y).normalize(), fwd.cross(Vec3::Y).normalize().cross(fwd), -fwd)), scale: Vec3::ONE }.mat(), fov.to_degrees())
        });
        let pos = m.w_axis.truncate();
        let f = -m.z_axis.truncate().normalize();
        Some(CameraOverride {
            pos: eye.lerp(pos, k),
            forward: fwd.lerp(f, k).normalize(),
            up: Vec3::Y.lerp(m.y_axis.truncate().normalize(), k).normalize(),
            fov: fov + (cam_fov.to_radians() - fov) * k,
        })
    }

    /// Поза камеры бланка: (глаза, yaw, pitch).
    fn seat_of_camera(&self) -> Option<(Vec3, f32, f32)> {
        let i = self.graph.find("Camera")?;
        let m = self.graph.global[i];
        let f = -m.z_axis.truncate().normalize();
        Some((m.w_axis.truncate(), f.x.atan2(-f.z), f.y.asin()))
    }

    /// Бланк только что подписан: куда встать игроку (где была камера
    /// бланка — дальше обзор возвращается плавно с того же места).
    pub fn take_seat(&mut self) -> Option<(Vec3, f32, f32)> {
        self.seat.take()
    }

    /// Ввод в пустоте: E на бланке, набор имени, F5 — показ плашки и панели.
    pub fn input(&mut self, engine: &mut AlkashEngine, snd: &mut Sounds, use_pressed: bool, eye: Vec3, dir: Vec3, strings: &std::collections::HashMap<String, String>) {
        if use_pressed && self.looking_at_form(eye, dir) {
            self.waiver.pick_up(&mut self.graph, &mut self.anims, snd, engine);
        }
        let g = &mut self.graph;
        if self.waiver.stage == WaiverStage::Typing {
            for vk in 0x41..=0x5Au32 {
                if engine.input.just_pressed(vk) {
                    self.waiver.key(g, snd, engine, vk as u8 as char);
                }
            }
            if engine.input.just_pressed(0x08) {
                self.waiver.backspace(g, snd, engine);
            }
            if engine.input.just_pressed(0x0D) {
                self.waiver.enter(g, snd, engine);
            }
        }
        if let Some(name) = self.auto_sign.clone() {
            let g = &mut self.graph;
            match self.waiver.stage {
                WaiverStage::Idle if self.t > 2.0 => self.waiver.pick_up(g, &mut self.anims, snd, engine),
                WaiverStage::Typing => {
                    for ch in name.chars() {
                        self.waiver.key(g, snd, engine, ch);
                    }
                    self.waiver.enter(g, snd, engine);
                }
                _ => {}
            }
        }
        if self.auto_round && self.signed_at.is_some_and(|s| self.t - s > 8.0) && !self.shells.active() {
            self.auto_round = false;
            self.debug_round(engine, snd, strings);
        }
        // Отладка (раунда ещё нет): включить панель и показать набор патронов.
        if engine.input.just_pressed(0x74) && !self.shells.active() {
            self.debug_round(engine, snd, strings);
        }
        // F6 — предмет на стол, F7 — гильза из ружья, F8 — выстрел в пони-дилера.
        if engine.input.just_pressed(0x75) {
            self.throw_item(engine);
        }
        if engine.input.just_pressed(0x76) {
            self.eject_shell(engine, snd);
        }
        if engine.input.just_pressed(0x77) {
            self.shoot_pony(0, eye, snd, engine);
        }
        // F9 — пони выходят (пока сюжетного вступления нет).
        if engine.input.just_pressed(0x78) {
            self.start_ponies();
        }
        if let Some(next) = self.demo {
            if self.t > 4.0 && self.t > next {
                self.demo = Some(self.t + 1.5);
                if self.demo_mask & 2 != 0 {
                    self.throw_item(engine);
                }
                if self.demo_mask & 4 != 0 {
                    self.eject_shell(engine, snd);
                }
                if self.demo_mask & 8 != 0 {
                    self.shoot_pony(0, eye, snd, engine);
                }
            }
        }
    }

    fn frand(&mut self) -> f32 {
        (self.rand() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Предмет падает на стол сверху со стороны игрока, кувыркаясь.
    fn throw_item(&mut self, engine: &mut AlkashEngine) {
        let kind = items::ITEMS[(self.rand() % items::ITEMS.len() as u32) as usize];
        let pos = Vec3::new(-8.0 + self.frand() * 10.0, 8.0, -6.0 + self.frand() * 12.0);
        let rot = Quat::from_euler(alkash3d_rs::math::EulerRot::YXZ, self.frand() * 6.3, self.frand() * 1.5, self.frand() * 1.5);
        let vel = Vec3::new(4.0 + self.frand() * 6.0, 2.0, self.frand() * 6.0 - 3.0);
        let spin = Vec3::new(self.frand() - 0.5, self.frand() - 0.5, self.frand() - 0.5) * 10.0;
        self.items.throw(engine, &mut self.graph, kind, pos, rot, vel, spin);
    }

    /// Гильза вылетает вбок из окна ружья (передёрнули затвор).
    fn eject_shell(&mut self, engine: &mut AlkashEngine, snd: &mut Sounds) {
        let g = &self.graph;
        let gun = g.global[g.node(SHOTGUN)];
        let port = gun.transform_point3(Vec3::new(-1.5, 0.3, 0.0));
        let side = gun.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
        let vel = side * (10.0 + self.frand() * 6.0) + Vec3::Y * (8.0 + self.frand() * 5.0);
        let spin = Vec3::new(self.frand() - 0.5, self.frand() - 0.5, self.frand() - 0.5) * 30.0;
        snd.play(engine, "res://audio/rack shotgun.ogg", 0.8);
        self.items.throw(engine, &mut self.graph, items::SHELL, port, Quat::from_mat4(&gun), vel, spin);
    }

    /// Попадание в пони `k`: рана на случайной части (с подтёком), брызги
    /// от игрока прочь, со временем — всё потрёпаннее.
    /// Где пони `k` сейчас и видна ли (до выхода — нет). Пока идёт — чуть
    /// покачивается в шаге.
    fn pony_at(&self, k: usize) -> Option<Vec3> {
        let (_, spot, facing, _) = self.ponies[k];
        let since = self.t - self.pony_enter?;
        if since < 0.0 {
            return None;
        }
        let left = (PONY_WALK_FROM - since * PONY_WALK_SPEED).max(0.0);
        let bob = if left > 0.0 { (since * 9.0).sin().abs() * 0.25 } else { 0.0 };
        Some(spot - facing * left + Vec3::Y * bob)
    }

    /// Свет на пони в `at`: прожекторы стола, к краю их света — темнота.
    fn light_at(&self, at: Vec3) -> f32 {
        let table: Vec<f32> = self.lamps.iter().take(FIRST_BOARD_LAMP - 1).map(|l| l.power).collect();
        let lamps = table.iter().sum::<f32>() / table.len().max(1) as f32;
        let d = Vec3::new(at.x - self.table_center.x, 0.0, at.z - self.table_center.z).length();
        let k = ((LIT_FAR - d) / (LIT_FAR - LIT_NEAR)).clamp(0.0, 1.0);
        lamps * k * k * (3.0 - 2.0 * k)
    }

    /// Выстрел дробью в пони `k`: пучок из 8–12 дробин ложится кучно вокруг
    /// точки попадания (часть цепляет соседние части тела), в центре —
    /// крупная рваная рана с длинным подтёком; брызги от каждой дробины.
    fn shoot_pony(&mut self, k: usize, eye: Vec3, snd: &mut Sounds, engine: &mut AlkashEngine) {
        use alkash3d_rs::math::Vec2;
        let Some(&(i, _, facing, scale)) = self.ponies.get(k) else { return };
        let Some(at) = self.pony_at(k) else { return };
        let Some(blood) = self.blood.as_ref() else { return };
        let (wound_meshes, drip_mesh) = (blood.wound_meshes, blood.drip_mesh);
        // Куда пришёлся центр выстрела: чаще торс и голова.
        let roles = [official::ROLE_HEAD, official::ROLE_NECK, 1, 1, official::ROLE_HEAD, official::ROLE_FRONT_LEG, 1];
        let role = roles[(self.rand() % roles.len() as u32) as usize];
        // Соседи по телу: дробины, ушедшие в сторону, цепляют их.
        let near: &[u32] = match role {
            official::ROLE_HEAD => &[official::ROLE_HEAD, official::ROLE_MANE, official::ROLE_NECK],
            official::ROLE_NECK => &[official::ROLE_NECK, official::ROLE_HEAD, 1],
            official::ROLE_FRONT_LEG => &[official::ROLE_FRONT_LEG, 1],
            _ => &[1, official::ROLE_NECK, official::ROLE_FRONT_LEG, official::ROLE_HIND_LEG],
        };
        let center = Vec2::new(0.3 + self.frand() * 0.4, 0.3 + self.frand() * 0.4);
        let mut shot: Vec<(Wound, f32)> = Vec::new();
        // Крупная рваная рана в центре.
        shot.push((
            Wound { role, uv: center, size: 0.2 + self.frand() * 0.08, mesh: wound_meshes[3], drip: Some((drip_mesh, 0.0)), angle: self.frand() * 6.3 },
            0.45 + self.frand() * 0.25,
        ));
        let pellets = 8 + (self.rand() % 5) as usize;
        for _ in 0..pellets {
            // Кучность: большинство рядом с центром, часть — дальше и на соседних частях.
            let (a, r) = (self.frand() * std::f32::consts::TAU, self.frand().powf(0.7) * 0.32);
            let mut uv = center + Vec2::new(a.cos(), a.sin()) * r;
            let mut role_p = role;
            if !(0.05..=0.95).contains(&uv.x) || !(0.05..=0.95).contains(&uv.y) || self.frand() < 0.15 {
                role_p = near[(self.rand() % near.len() as u32) as usize];
                uv = Vec2::new(0.2 + self.frand() * 0.6, 0.2 + self.frand() * 0.6);
            }
            let kind = if self.frand() < 0.8 { 0 } else { 1 };
            let drip = (self.frand() < 0.5).then_some((drip_mesh, 0.0));
            shot.push((
                Wound { role: role_p, uv, size: 0.035 + self.frand() * 0.035, mesh: wound_meshes[kind], drip, angle: self.frand() * 6.3 },
                0.1 + self.frand() * 0.25,
            ));
        }
        // Синяк вокруг — от удара всей осыпи.
        shot.push((Wound { role, uv: center, size: 0.34, mesh: wound_meshes[2], drip: None, angle: self.frand() * 6.3 }, 0.0));

        snd.play(engine, "res://audio/temp gunshot_live.wav", 1.0);
        snd.play(engine, "res://audio/splatter1.ogg", 0.9);
        let s = UNITS_PER_M * scale;
        let seed = k as f32 * 2.399;
        let p = &self.puppets[i];
        let pose = p.pose(at, facing, self.cam, self.t, seed);
        let points: Vec<Vec3> = shot.iter().filter_map(|(w, _)| pose.as_ref().and_then(|pose| p.wound_world(pose, at, w, s))).collect();
        if let Some(b) = self.blood.as_mut() {
            for (n, from) in points.iter().enumerate() {
                let away = Vec3::new(from.x - eye.x, 0.0, from.z - eye.z).normalize_or(Vec3::X) + Vec3::Y * 0.35;
                // Из центральной раны — больше всего.
                let count = if n == 0 { 120 } else { 18 };
                b.spray(*from, away, count);
                b.spray(*from, -away + Vec3::Y * 0.6, count / 5);
            }
        }
        // Синяк — под ранами: первым в списке (рисуется раньше).
        let bruise = shot.pop().unwrap();
        let ws = &mut self.wounds[k];
        ws.insert(0, bruise);
        ws.extend(shot);
        while ws.len() > MAX_WOUNDS {
            ws.remove(0);
        }
        self.hits[k] += 1;
    }

    /// Начало партии: пони выходят из темноты к своим местам. Зови из
    /// сюжета (вступление); для отладки — F9.
    pub fn start_ponies(&mut self) {
        self.pony_enter.get_or_insert(self.t);
    }

    fn rand(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    /// Случайный набор (2–8 патронов, боевых не меньше одного) и заряды 2–4.
    fn debug_round(&mut self, engine: &mut AlkashEngine, snd: &mut Sounds, strings: &std::collections::HashMap<String, String>) {
        let total = 2 + self.rand() % 7;
        let live = (1 + self.rand() % (total / 2 + 1)).min(total - 1).max(1);
        let mut shells: Vec<bool> = (0..total).map(|i| i < live).collect();
        for i in (1..shells.len()).rev() {
            let j = (self.rand() as usize) % (i + 1);
            shells.swap(i, j);
        }
        let blank = total - live;
        let tr = |k: &str| strings.get(k).cloned().unwrap_or_else(|| k.to_string());
        let text = format!(
            "{} {}",
            tr(if live == 1 { "LIVEROUND" } else { "LIVEROUNDS" }).replace("%s", &live.to_string()),
            tr(if blank == 1 { "BLANKROUND" } else { "BLANKROUNDS" }).replace("%s", &blank.to_string())
        );
        let hp = 2 + (self.rand() % 3) as i32;
        let name = if self.waiver.name().is_empty() { "PONY".to_string() } else { self.waiver.name().to_string() };
        self.counter.set_name(&mut self.graph, &name, &tr("PLAYERWIN").replace("%s", &name));
        self.counter.boot(&mut self.graph, snd, engine, [hp, hp]);
        self.shells.show(&mut self.graph, &mut self.anims, snd, engine, &shells, text);
    }

    /// `active` — игрок уже здесь: всё считается, лампы светят; иначе погашены.
    pub fn update(&mut self, engine: &mut AlkashEngine, snd: &mut Sounds, dt: f32, active: bool, strings: &std::collections::HashMap<String, String>) {
        if active {
            self.t += dt;
            self.frame += 1;
            let g = &mut self.graph;
            self.waiver.update(dt, g, &mut self.anims, snd, engine);
            self.counter.update(dt, g, &mut self.anims, snd, engine);
            self.shells.update(dt, g, &mut self.anims, snd, engine);
            // Переход камеры: к бланку, пока он в руках; обратно — после подписи.
            let target = if self.waiver.camera(&self.graph).is_some() { BLEND } else { 0.0 };
            self.blend = if target > self.blend { (self.blend + dt).min(target) } else { (self.blend - dt).max(target) };
            if self.waiver.take_just_signed() {
                self.signed_at = Some(self.t);
                self.seat = self.seat_of_camera();
                let name = self.waiver.name().to_string();
                let win = strings.get("PLAYERWIN").cloned().unwrap_or_else(|| "%s".into()).replace("%s", &name);
                self.counter.set_name(&mut self.graph, &name, &win);
                self.event([2.0, 0.0, 0.0, 0.0]);
            }
            self.anims.update(&mut self.graph, dt);
            self.items.update(engine, &mut self.graph, &self.collider);
            if let Some(b) = self.blood.as_mut() {
                b.update(dt, &self.collider);
            }
            // Подтёки медленно растут (~5 см/с).
            for ws in &mut self.wounds {
                for (w, to) in ws.iter_mut() {
                    if let Some((_, len)) = w.drip.as_mut() {
                        *len = (*len + dt * 0.05).min(*to);
                    }
                }
            }
            for (k, l) in self.lamps.iter_mut().enumerate() {
                l.power = match (self.lua.as_mut(), l.script) {
                    (Some(lua), Some(id)) => {
                        let mut ctx = ScriptContext {
                            entity_id: k as u64 + 1,
                            delta_time: dt,
                            frame_number: self.frame,
                            position: l.pos.to_array(),
                            rotation: [(k + 1) as f32, self.t, 0.0],
                            ..Default::default()
                        };
                        lua.update_script(id, &mut ctx);
                        if ctx.position_changed != 0 { ctx.out_position[0].max(0.0) } else { l.power }
                    }
                    // Без скрипта: стол загорается по очереди, доска — после подписи.
                    _ if k + 1 >= FIRST_BOARD_LAMP => self.signed_at.map_or(0.0, |s| ((self.t - s - 1.0) / 4.0).clamp(0.0, 1.0)),
                    _ => ((self.t - 0.6 - k as f32 * 0.9) / 3.0).clamp(0.0, 1.0),
                };
            }
        }
        for l in &self.lamps {
            let p = if active { l.power } else { 0.0 };
            self.graph.nodes[l.light].light_energy = l.energy * p;
            // Стекло светится само (unshaded): яркость через tint, погасшее — тёмное.
            let g = 0.04 + p;
            self.graph.nodes[l.glass].tint = [g, g, g, 1.0];
        }
        self.graph.update_globals();
        self.graph.update_skins(engine);
        self.graph.sync_lights(engine, active);
    }

    /// `cam` — где камера (по ней выбирается ракурс пони).
    pub fn push_instances(&mut self, out: &mut Vec<MeshInstance>, cam: Vec3, facing_cam: (f32, f32, f32)) {
        self.cam = cam;
        self.graph.push_instances(out);
        // Пони без освещения: яркость — за прожекторами стола (появляются и
        // мерцают вместе со светом).
        let table: Vec<f32> = self.lamps.iter().take(FIRST_BOARD_LAMP - 1).map(|l| l.power).collect();
        let light = table.iter().sum::<f32>() / table.len().max(1) as f32;
        if let Some(b) = &self.blood {
            b.push_instances(out, facing_cam, light);
        }
        if self.pony_tint * light <= 0.001 {
            return;
        }
        for (k, &(i, _, facing, scale)) in self.ponies.iter().enumerate() {
            let Some(at) = self.pony_at(k) else { continue };
            let light = self.light_at(at);
            if light <= 0.001 {
                continue;
            }
            // Чем больше попаданий — тем темнее и грязнее шерсть.
            let worn = 1.0 - 0.035 * self.hits[k].min(8) as f32;
            let p = &self.puppets[i];
            let Some(pose) = p.pose(at, facing, cam, self.t, k as f32 * 2.399) else { continue };
            p.draw_pose(out, &pose, at, self.pony_tint * light * worn, UNITS_PER_M * scale);
            let wounds: Vec<Wound> = self.wounds[k].iter().map(|w| w.0).collect();
            p.draw_wounds(out, &pose, at, &wounds, light, UNITS_PER_M * scale);
        }
    }
}
