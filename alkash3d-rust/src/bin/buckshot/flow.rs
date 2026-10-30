//! Порт скриптов-"режиссёров" Buckshot Roulette (IntroManager,
//! RoundManager, ShellSpawner, ShellLoader, ShotgunShooting,
//! DealerIntelligence, HandManager, ItemManager, ItemInteraction,
//! HealthCounter, DeathManager, Signature, BriefcaseMachine,
//! EndingManager, HeavenManager, MenuManager).
//!
//! GDScript-корутины (`await get_tree().create_timer(t).timeout`) здесь —
//! очереди "реплик" (`Cue`): подождать, сделать что-то (и, возможно,
//! вставить новые реплики), запустить параллельную ветку (то, что в
//! оригинале вызывается без await), ждать ввода игрока. Тайминги и имена
//! анимаций взяты из оригинальных скриптов один в один.

use std::collections::{HashMap, VecDeque};

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::math::Vec3;

use crate::anim::{ease, Anims, Fired};
use crate::game::{Action, Game, Item, Side, SLOTS};
use crate::graph::{euler_yxz, Graph, Trs};
use crate::sound::Sounds;

pub type Step = Box<dyn FnOnce(&mut Ctx) -> Vec<Cue>>;

pub enum Cue {
    Wait(f32),
    Do(Step),
    Fork(Vec<Cue>),
    Input(Mode),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Menu,
    BathroomDoor,
    BackroomDoor,
    Waiver,
    GrabItem,
    PlaceItem,
    Turn,
    Holding,
    BriefcaseLatches,
    AnyKeyToMenu,
    Heaven,
}

/// То, что можно нажать мышью.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Start,
    Quit,
    BathroomDoor,
    BackroomDoor,
    Waiver,
    Key(char),
    KeyEnter,
    KeyBack,
    Briefcase,
    Slot(usize),
    Shotgun,
    Item(usize),
    ShootDealer,
    ShootSelf,
    LatchL,
    LatchR,
    Lid,
    Anywhere,
    Retry,
    ToMenu,
}

/// Рамки "bracket selection" вокруг интерактивных объектов — в оригинале
/// видны только под курсором.
const BRACKETS: [&str; 6] = [
    "intro parent/bracket selection 3d_door",
    "intro parent/bracket selection 3d_door backroom",
    "intro parent/bracket selection 3d_crt screen",
    "intro parent/bracket selection 3d_pill bottle",
    "bracket selection 3d_waiver",
    "tabletop parent/main tabletop/compartments parent/compartment user_briefcase/tabletop briefcase player/Cube_004/3d ui_briefcase intake",
];

pub const MENU: usize = 0;
pub const MAIN: usize = 1;
pub const HEAVEN: usize = 2;

pub struct SceneRt {
    pub graph: Graph,
    pub anims: Anims,
    pub camera: usize,
}

// ---- пути AnimationPlayer'ов main.tscn ----
const A_SHOOT_TEXT: &str = "ui parent_shooting decision/animator_shooting text";
const A_SHOTGUN: &str = "tabletop parent/shotgun main parent/anim_shotgun main parent";
const A_DEALER: &str = "dealer model parent/animator_dealer parent";
const A_HANDS: &str = "dealer hands main1/animator_dealer hands";
const A_HEADLOOK: &str = "dealer model parent/animator_dealer head look";
const A_PITEMS: &str = "player item interaction parent/animator_player items";
const A_SHELLBOX: &str = "tabletop parent/main tabletop/compartment_shells/animator_shell compartment";
const A_COMPARTMENTS: &str = "tabletop parent/main tabletop/compartments parent/animator_compartments";
const A_BRIEFCASE: &str = "tabletop parent/main tabletop/compartments parent/animator_briefcase compartments";
const A_LOAD_HAND: &str = "tabletop parent/shotgun main parent/shotgun parent new/dealer hand_shotgun right/animator_dealer hand shotgun right";
const A_MUZZLE: &str = "tabletop parent/shotgun main parent/animator_muzzle flash";
const A_MUZZLE_MODEL: &str = "tabletop parent/shotgun main parent/shotgun parent new/muzzle flash test1/Cylinder/animator muzzle flash model";
const A_EJECT_PLAYER: &str = "shell parent_ejecting player side/animator_ejecting shell player";
const A_EJECT_DEALER: &str = "shell parent_ejecting enemy side/animator_eject shell enemy";
const A_INTRO: &str = "intro parent/animator_intro";
const A_SMOKER: &str = "intro parent/smoker dude revive player1/animator_smoker dude revival";
const A_WAIVER: &str = "standalone managers/signature manager/animator_pickup waiver";
const A_ROUND_BLINK: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/round indicator parent/animator_round blinker";
const A_DEFIB: &str = "player defib device parent/animator_player defib device";
const A_CUFFBAR: &str = "player cuff bar/animator_handcuffs player bar";
const A_FLICKER_P: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/health UI_player side/symbol_player1/animator_flicker digit player";
const A_FLICKER_D: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/health UI_dealer side/symbol_dealer1/animator_flicker digit enemy";
const A_MACHINE: &str = "briefcase machine parent/briefcase machine animator";
const A_LATCH_L: &str = "briefcase machine parent/Plane_005/briefcase parent in vehicle/briefcase hinge/briefcase latch_left/animator_latch left";
const A_LATCH_R: &str = "briefcase machine parent/Plane_005/briefcase parent in vehicle/briefcase hinge/briefcase latch_right/animator_latch right";
const A_LID: &str = "briefcase machine parent/Plane_005/briefcase parent in vehicle/briefcase hinge/animator_lid";
const A_PAN_BRIEF: &str = "standalone managers/ending manager/animator_camera briefcase pan";
const A_STREETLIGHTS: &str = "player vehicle parent/animator_street lights";
const A_BACKDROP: &str = "player vehicle backdrop parent/animator_vehicle backdrop";
const A_CAR_CAM: &str = "player vehicle parent/animator_camera in car";

// ---- узлы ----
const N_COUNTER_UI: &str = "tabletop parent/main tabletop/health counter/health counter ui parent";
const N_SHELLBOX: &str = "tabletop parent/main tabletop/compartment_shells";
const N_INTAKE: &str = "tabletop parent/main tabletop/compartments parent/player grid intake colliders";
const N_SHOTGUN: &str = "tabletop parent/shotgun main parent";
const N_HAND_L: &str = "tabletop parent/shotgun main parent/shotgun parent new/shotgun-forearm_003/dealer hand shotgun_left";
const N_HAND_R: &str = "tabletop parent/shotgun main parent/shotgun parent new/dealer hand_shotgun right";
const N_SEGMENT: &str = "tabletop parent/shotgun main parent/shotgun parent new/shotgun cut segment";
const N_EXAMINE_SHELL: &str = "tabletop parent/shotgun main parent/shotgun parent new/shotgun-forearm_003/shotgun_bolt carrier_003/shotgun_pos shell_003/shell parent";
const N_EJECT_PLAYER: &str = "shell parent_ejecting player side";
const N_EJECT_DEALER: &str = "shell parent_ejecting enemy side";
const N_DEALER_MESH: &str = "dealer model parent/dealer head rigged1/Armature/Skeleton3D/dealer head mesh";
const N_DEALER_CRUSHED: &str = "dealer model parent/dealer head rigged1/Armature/Skeleton3D/dealer head mesh/dealer head crushed";
const N_DEFIB: &str = "player defib device parent";
const N_PILLS: &str = "restroom_CLUB/pill bottle main";
const N_SMOKER: &str = "intro parent/smoker dude revive player1";
const N_WAIVER_TABLE: &str = "signature machine2/release form on table";
const N_WAIVER_MACHINE: &str = "signature machine2/signature machine main parent";
const N_WAIVER_KEYS: &str = "signature machine2/signature machine main parent/baseplate main/button colliders parent/button colliders sub-parent";
const N_WAIVER_SCREEN: &str = "signature machine2/signature machine main parent/baseplate main/screen parent";
const N_WAIVER_JOINED: &str = "signature machine2/signature machine main parent/baseplate main/release form1_003";
const N_WAIVER_SIGNED: &str = "signature machine2/release form signed separate";
const N_ROUND_IND_PARENT: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/round indicator parent";
const N_ROUND_IND: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/round indicator parent/round indicator";
const N_PLAYER_WIN: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/indicator_player win";
const N_MACHINE_EYES: &str = "briefcase machine eyes";
const N_MACHINE: &str = "briefcase machine parent";

/// Узлы, которые игра двигает или прячет сама (не только анимации).
pub const MAIN_DYNAMIC: &[&str] = &[
    "tabletop parent",
    "dealer model parent",
    "dealer hands main1",
    "player item interaction parent",
    "player defib device parent",
    "player cuff bar",
    "shell parent_ejecting player side",
    "shell parent_ejecting enemy side",
    "restroom_CLUB/pill bottle main",
    "intro parent",
    "signature machine2",
    "briefcase machine eyes",
    "briefcase machine parent",
    "Camera",
    "bracket selection 3d_waiver",
];

/// CameraSocket'ы из main.tscn: имя, позиция, rotation_degrees, fov.
const SOCKETS: &[(&str, [f32; 3], [f32; 3], f32)] = &[
    ("home", [-13.59, 7.323, 0.024], [-49.0, -90.0, 0.0], 65.5),
    ("shell compartment", [-4.45, 3.104, 2.081], [-49.0, -128.0, 0.0], 65.5),
    ("enemy", [-13.59, 7.323, 0.024], [-14.5, -90.0, 0.0], 65.5),
    ("health counter", [-3.40373, 1.595, 7.00212], [-33.1, -149.5, 0.0], 65.5),
    ("briefcase", [-12.928, 7.284, 0.024], [-64.0, -90.0, 0.0], 61.5),
    ("dealer cut shotgun", [-0.092, 3.031, -1.253], [-35.0, -112.3, 0.0], 65.5),
    ("check handcuffs", [-13.59, 7.323, 0.024], [-52.0, -43.9, 0.0], 65.5),
    ("dealer handcuffs", [8.253, 5.794, -2.916], [-36.7, -109.1, -6.0], 57.5),
    ("enemy cuffs close", [-13.59, 7.323, 0.024], [-14.5, -90.0, 0.0], 33.5),
    ("grow barrel", [0.042, 2.743, 2.502], [-43.1, -76.6, 0.0], 65.5),
    ("cash briefcase", [-4.784, 7.323, 0.024], [-19.2, -90.0, 0.0], 65.5),
    ("enemy items", [-2.378, 7.323, 0.024], [-49.0, -90.0, 0.0], 65.5),
    ("player item grid left", [-13.59, 7.323, 0.024], [-44.1, -61.5, 0.0], 65.5),
    ("player item grid right", [-13.59, 7.323, 0.024], [-44.1, -116.7, 0.0], 65.5),
];

/// ItemResource из main.tscn: префаб, pos/rot в чемодане, в руке, смещение в ячейке — [игрок, дилер].
struct ItemRes {
    prefab: &'static str,
    in_briefcase: ([f32; 3], [f32; 3]),
    in_hand: ([f32; 3], [f32; 3]),
    offset: [([f32; 3], [f32; 3]); 2],
}

fn item_res(item: Item) -> ItemRes {
    match item {
        Item::Handsaw => ItemRes {
            prefab: "item_handsaw",
            in_briefcase: ([8.635, -0.204, 0.033], [65.9, -56.3, 29.8]),
            in_hand: ([8.794, 3.145, -0.081], [44.4, -166.5, -118.5]),
            offset: [([0.0, 0.149, 0.143], [0.0, -31.5, 0.0]), ([0.085, 0.153, -0.206], [0.0, 150.5, 0.0])],
        },
        Item::Magnifier => ItemRes {
            prefab: "item_magnifying glass",
            in_briefcase: ([8.253, -0.054, 0.142], [-2.0, -89.4, 80.1]),
            in_hand: ([9.254, 3.138, -0.182], [0.0, -28.5, 45.6]),
            offset: [([0.309, 0.0, 0.356], [0.0, -28.5, 90.0]), ([-0.249, 0.0, -0.227], [0.0, 152.7, 90.0])],
        },
        Item::Beer => ItemRes {
            prefab: "item_beer can",
            in_briefcase: ([8.28, -0.197, 1.305], [47.7, -83.6, 93.5]),
            in_hand: ([9.71, 2.233, -0.021], [16.6, -45.5, 23.6]),
            offset: [([1.048, 0.0, -0.437], [0.0, 16.2, 0.0]), ([-1.031, -0.006, 0.439], [0.0, 34.9, 0.0])],
        },
        Item::Cigarettes => ItemRes {
            prefab: "item_cigarettes",
            in_briefcase: ([8.673, -0.076, 0.317], [-3.2, 4.1, 10.2]),
            in_hand: ([9.331, 3.288, -0.127], [60.4, 37.1, 6.6]),
            offset: [([0.477, 0.0, 0.019], [0.0, 66.8, 0.0]), ([-0.418, -0.007, 0.086], [0.0, -71.2, 0.0])],
        },
        Item::Handcuffs => ItemRes {
            prefab: "item_handcuffs",
            in_briefcase: ([8.044, 0.108, -0.183], [58.3, 80.4, -10.3]),
            in_hand: ([8.811, 3.43, -0.021], [52.7, 117.2, -16.0]),
            offset: [([-0.128, 0.054, 0.629], [0.0, 180.0, 0.0]), ([0.032, 0.01, 0.508], [0.0, -166.9, 0.0])],
        },
    }
}

fn player_use_anim(item: Item) -> &'static str {
    match item {
        Item::Handsaw => "player use handsaw",
        Item::Magnifier => "player use magnifier",
        Item::Beer => "player use beer",
        Item::Cigarettes => "player use cigarettes",
        Item::Handcuffs => "",
    }
}

const SHELL_SPOTS: [[f32; 3]; 8] = [
    [0.049, 0.197, 2.047], [0.045, 0.197, 1.472], [0.045, 0.197, 0.888], [0.045, 0.197, 0.332],
    [0.045, 0.197, -0.291], [0.045, 0.197, -0.863], [0.045, 0.197, -1.423], [0.045, 0.197, -2.023],
];

/// Позиции индикатора раунда на счётчике (roundIndicatorPositions).
const ROUND_IND_POS: [[f32; 3]; 4] = [[0.0, 0.0, 0.0], [-0.616, -0.018, 0.438], [0.004, -0.018, 0.438], [0.636, -0.018, 0.438]];

/// Музыка по наборам (MusicManager.trackArray): файл, приглушённая версия есть рядом (.lp).
const TRACKS: [(&str, &str); 3] = [
    ("res://audio/music/music main_techno techno.ogg", "res://audio/music/music main second loop_techno techno.ogg"),
    ("res://audio/music/music main_error.ogg", "res://audio/music/music main second loop_error.ogg"),
    ("res://audio/music/music main_room.ogg", "res://audio/music/music main second loop_room.ogg"),
];

struct CamLerp {
    moving: bool,
    from: (Vec3, Vec3, f32),
    to: (Vec3, Vec3, f32),
    elapsed: f32,
}

struct ItemTween {
    spawn: u32,
    from: (Vec3, Vec3),
    to: (Vec3, Vec3),
    t: f32,
    dur: f32,
    curve: f32,
}

pub struct Dialogue {
    pub text: String,
    pub started: f32,
    pub until: Option<f32>,
    pub delay: f32,
    shown: usize,
}

#[derive(Default)]
pub struct Stats {
    pub shots: u32,
    pub shells_ejected: u32,
    pub doors_kicked: u32,
    pub cigs: u32,
    pub beer_ml: u32,
}

pub struct Ctx {
    pub engine: AlkashEngine,
    pub scenes: Vec<SceneRt>,
    pub active: usize,
    pub game: Game,
    pub sounds: Sounds,
    pub strings: HashMap<String, String>,
    queues: Vec<(VecDeque<Cue>, f32)>,
    cam: CamLerp,
    pub shake: f32,
    pub time: f32,
    pub dialogue: Option<Dialogue>,
    pub hint: Option<String>,
    /// Цель под курсором (подпись предмета, курсор-рука).
    pub hover: Option<Target>,
    pub black: bool,
    /// Яркость кадра 0..1 (animator_pp "brightness fade in/out").
    pub brightness: f32,
    brightness_target: f32,
    pub name: String,
    pub typed: String,
    pub stats: Stats,
    pub ending_lines: Vec<String>,
    pub exit: bool,
    // Состояние партии.
    signed_waiver: bool,
    dealer_at_table: bool,
    entering_from_death: bool,
    dealer_holding_shotgun: bool,
    dealer_greeted: bool,
    true_death_ready: bool,
    player_can_go_again: bool,
    shells: Vec<u32>,
    items: [[Option<u32>; SLOTS]; 2],
    grab: Option<(Item, u32)>,
    grabbed: usize,
    tweens: Vec<ItemTween>,
    latches: [bool; 2],
    music_muffled: bool,
    last_ejected: bool,
    active_socket: &'static str,
    barrel_sawed: bool,
    lerping: bool,
    waiting_for_dealer_return: bool,
    careful_shown: [bool; 2],
    dialogue_reads: u32,
    read_intro: bool,
    read_item_swap: bool,
    read_item_distribution: bool,
    read_item_intro2: bool,
    entering_from_waiver: bool,
    next_load_first: bool,
    new_batch_begun: bool,
    drill_shown: bool,
    player_shot_self: bool,
    shell_insert_index: usize,
    /// Дилер вернулся к столу в наручниках (RoundManager.waitingForReturn).
    waiting_for_return: bool,
    /// Монолог перед финалом уже прочитан (playerData.cutterDialogueRead).
    cutter_dialogue_read: bool,
    /// DialogueManager.dealerLowPitched: в финале дилер говорит ниже.
    dealer_low_pitched: bool,
    /// MusicManager.trackset: вторая версия трека — только один раз после смерти.
    trackset: bool,
}

// ---- конструкторы реплик ----

fn wait(t: f32) -> Cue {
    Cue::Wait(t)
}

fn act(f: impl FnOnce(&mut Ctx) + 'static) -> Cue {
    Cue::Do(Box::new(move |c| {
        f(c);
        Vec::new()
    }))
}

fn then(f: impl FnOnce(&mut Ctx) -> Vec<Cue> + 'static) -> Cue {
    Cue::Do(Box::new(f))
}

fn anim(player: &'static str, name: &'static str) -> Cue {
    act(move |c| c.play(player, name))
}

fn cam(socket: &'static str) -> Cue {
    act(move |c| c.begin_lerp(socket))
}

fn speaker(node: &'static str) -> Cue {
    act(move |c| c.play_node(node))
}

fn sfx(path: &'static str) -> Cue {
    act(move |c| c.sounds.play(&mut c.engine, path, 0.8))
}

fn say(key: &'static str) -> Cue {
    act(move |c| {
        let t = c.tr(key);
        c.show_text(t, None)
    })
}

fn say_for(key: &'static str, dur: f32) -> Cue {
    act(move |c| {
        let t = c.tr(key);
        c.show_text(t, Some(dur))
    })
}

fn hide_text() -> Cue {
    act(|c| c.dialogue = None)
}

fn vis(node: &'static str, v: bool) -> Cue {
    act(move |c| c.main().graph.set_visible(node, v))
}

impl Ctx {
    pub fn new(engine: AlkashEngine, scenes: Vec<SceneRt>, sounds: Sounds, strings: HashMap<String, String>, seed: u64) -> Self {
        Ctx {
            engine,
            scenes,
            active: MENU,
            game: Game::new(seed),
            sounds,
            strings,
            queues: vec![(VecDeque::new(), 0.0)],
            cam: CamLerp { moving: false, from: (Vec3::ZERO, Vec3::ZERO, 65.5), to: (Vec3::ZERO, Vec3::ZERO, 65.5), elapsed: 0.0 },
            shake: 0.0,
            time: 0.0,
            dialogue: None,
            hint: None,
            hover: None,
            black: false,
            brightness: 1.0,
            brightness_target: 1.0,
            name: String::new(),
            typed: String::new(),
            stats: Stats::default(),
            ending_lines: Vec::new(),
            exit: false,
            signed_waiver: false,
            dealer_at_table: false,
            entering_from_death: false,
            dealer_holding_shotgun: false,
            dealer_greeted: false,
            true_death_ready: false,
            player_can_go_again: false,
            shells: Vec::new(),
            items: [[None; SLOTS]; 2],
            grab: None,
            grabbed: 0,
            tweens: Vec::new(),
            latches: [false; 2],
            music_muffled: true,
            last_ejected: false,
            active_socket: "home",
            barrel_sawed: false,
            lerping: false,
            waiting_for_dealer_return: false,
            careful_shown: [false; 2],
            dialogue_reads: 0,
            read_intro: false,
            read_item_swap: false,
            read_item_distribution: false,
            read_item_intro2: false,
            entering_from_waiver: false,
            next_load_first: true,
            new_batch_begun: false,
            drill_shown: false,
            player_shot_self: false,
            shell_insert_index: 0,
            waiting_for_return: false,
            cutter_dialogue_read: false,
            dealer_low_pitched: false,
            trackset: false,
        }
    }

    pub fn main(&mut self) -> &mut SceneRt {
        &mut self.scenes[MAIN]
    }

    pub fn tr(&self, key: &str) -> String {
        self.strings.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    fn play(&mut self, player: &str, name: &str) {
        let s = &mut self.scenes[self.active];
        s.anims.play(&mut s.graph, player, name);
    }

    fn anim_len(&self, player: &str, name: &str) -> f32 {
        self.scenes[self.active].anims.length(player, name)
    }

    /// AudioStreamPlayer по пути узла: его поток знает граф.
    fn play_node(&mut self, node: &str) {
        let s = &self.scenes[self.active];
        if let Some(i) = s.graph.find(node) {
            let path = s.graph.nodes[i].audio_path.clone();
            if !path.is_empty() {
                self.sounds.play(&mut self.engine, &path, 0.8);
            }
        } else {
            eprintln!("[BUCKSHOT] нет звукового узла '{}'", node);
        }
    }

    fn show_text(&mut self, text: String, dur: Option<f32>) {
        let until = dur.map(|d| self.time + d);
        self.dialogue = Some(Dialogue { text, started: self.time, until, delay: 0.02, shown: 0 });
    }

    /// CameraManager.BeginLerp: 0.6 с, ease(0.2), покомпонентно по rotation_degrees.
    fn begin_lerp(&mut self, socket: &str) {
        let Some(&(_, p, r, f)) = SOCKETS.iter().find(|s| s.0 == socket) else {
            eprintln!("[BUCKSHOT] нет сокета камеры '{}'", socket);
            return;
        };
        let s = &self.scenes[MAIN];
        let n = &s.graph.nodes[s.camera];
        self.cam.from = (n.local.pos, n.euler * (180.0 / std::f32::consts::PI), n.fov);
        self.cam.to = (Vec3::from(p), Vec3::from(r), f);
        self.cam.elapsed = 0.0;
        self.cam.moving = true;
        self.active_socket = SOCKETS.iter().find(|s| s.0 == socket).map(|s| s.0).unwrap_or("home");
        // CameraManager.PanSound: pitch_scale = randf_range(0.5, 1.0).
        let pitch = 0.5 + 0.5 * self.game.rng.f32();
        let s = &self.scenes[MAIN];
        if let Some(i) = s.graph.find("speaker parent/camera pan noise") {
            let path = s.graph.nodes[i].audio_path.clone();
            self.sounds.play_pitched(&mut self.engine, &path, 0.8, pitch);
        }
    }

    fn shake_camera(&mut self) {
        self.shake = 1.0;
    }

    fn fork(&mut self, cues: Vec<Cue>) {
        self.queues.push((cues.into_iter().collect(), 0.0));
    }

    pub fn start(&mut self, cues: Vec<Cue>) {
        self.queues[0].0.extend(cues);
    }

    pub fn waiting_for(&self) -> Option<Mode> {
        self.queues.iter().find_map(|q| match q.0.front() {
            Some(Cue::Input(m)) => Some(*m),
            _ => None,
        })
    }

    // ================= кадр =================

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.run_queues(dt);

        // Анимации + вызовы методов/звуки из их дорожек.
        let active = self.active;
        let fired = {
            let s = &mut self.scenes[active];
            s.anims.update(&mut s.graph, dt)
        };
        for f in fired {
            match f {
                Fired::Sound(path) => self.sounds.play(&mut self.engine, &path, 0.8),
                Fired::Method { node, name } => {
                    let cues = self.on_method(node, &name);
                    if !cues.is_empty() {
                        self.fork(cues);
                    }
                }
            }
        }

        if self.cam.moving && self.active == MAIN {
            self.cam.elapsed += dt;
            let c = ease((self.cam.elapsed / 0.6).clamp(0.0, 1.0), 0.2);
            let (p0, r0, f0) = self.cam.from;
            let (p1, r1, f1) = self.cam.to;
            let cam_node = self.scenes[MAIN].camera;
            let g = &mut self.scenes[MAIN].graph;
            g.nodes[cam_node].local.pos = p0.lerp(p1, c);
            g.set_rotation(cam_node, r0.lerp(r1, c) * (std::f32::consts::PI / 180.0));
            g.nodes[cam_node].fov = f0 + (f1 - f0) * c;
        }

        for tw in &mut self.tweens {
            tw.t = (tw.t + dt / tw.dur).min(1.0);
            let k = if tw.curve == 1.0 { tw.t } else { ease(tw.t, tw.curve) };
            if let Some(sp) = self.scenes[MAIN].graph.spawned.get_mut(&tw.spawn) {
                sp.local.pos = tw.from.0.lerp(tw.to.0, k);
                sp.local.rot = euler_yxz(tw.from.1.lerp(tw.to.1, k) * (std::f32::consts::PI / 180.0));
            }
        }
        self.tweens.retain(|t| t.t < 1.0);

        // Яркость (animator_pp): плавно к цели за ~2 с, как анимации fade in/out.
        let step = dt / 2.05;
        if self.brightness < self.brightness_target {
            self.brightness = (self.brightness + step).min(self.brightness_target);
        } else {
            self.brightness = (self.brightness - step).max(self.brightness_target);
        }

        if let Some(d) = &mut self.dialogue {
            if d.until.is_some_and(|u| self.time >= u) {
                self.dialogue = None;
            } else {
                let n = (((self.time - d.started) / d.delay) as usize).min(d.text.chars().count());
                if n > d.shown {
                    // DialogueManager: на каждую букву щелчок "dialogue click4" с
                    // pitch 0.5–0.6 (в финале — 0.2–0.3), это и есть "голос" дилера.
                    let new_chars: Vec<char> = d.text.chars().skip(d.shown).take(n - d.shown).collect();
                    d.shown = n;
                    for ch in new_chars {
                        if ch.is_whitespace() {
                            continue;
                        }
                        let (lo, hi) = if self.dealer_low_pitched { (0.2, 0.3) } else { (0.5, 0.6) };
                        let pitch = lo + (hi - lo) * self.game.rng.f32();
                        self.sounds.play_pitched(&mut self.engine, "res://audio/dialogue click4.ogg", 1.0, pitch);
                    }
                }
            }
        }

        self.sounds.update_music(&mut self.engine, dt, self.music_muffled);
        self.shake = (self.shake - dt * 2.0).max(0.0);
    }

    fn run_queues(&mut self, dt: f32) {
        let count = self.queues.len();
        for qi in 0..count {
            self.queues[qi].1 -= dt;
        }
        let mut qi = 0;
        while qi < self.queues.len() {
            loop {
                if self.queues[qi].1 > 0.0 {
                    break;
                }
                let Some(cue) = self.queues[qi].0.pop_front() else { break };
                match cue {
                    Cue::Wait(t) => self.queues[qi].1 += t,
                    Cue::Do(f) => {
                        let more = f(self);
                        // Реплика могла оборвать все сценарии (смерть игрока, выход
                        // в меню) — тогда этой ветки уже нет, продолжение идёт
                        // в главную очередь.
                        if qi >= self.queues.len() {
                            for c in more.into_iter().rev() {
                                self.queues[0].0.push_front(c);
                            }
                            return;
                        }
                        for c in more.into_iter().rev() {
                            self.queues[qi].0.push_front(c);
                        }
                    }
                    Cue::Fork(cues) => self.fork(cues),
                    Cue::Input(m) => {
                        self.queues[qi].0.push_front(Cue::Input(m));
                        self.queues[qi].1 = 0.0;
                        break;
                    }
                }
            }
            qi += 1;
        }
        // Пустые параллельные ветки больше не нужны.
        let mut i = 1;
        while i < self.queues.len() {
            if self.queues[i].0.is_empty() && self.queues[i].1 <= 0.0 {
                self.queues.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Ответ на ввод: убираем `Input` из главной очереди и вставляем реплики.
    fn resolve(&mut self, cues: Vec<Cue>) {
        let qi = self.queues.iter().position(|q| matches!(q.0.front(), Some(Cue::Input(_)))).unwrap_or(0);
        if matches!(self.queues[qi].0.front(), Some(Cue::Input(_))) {
            self.queues[qi].0.pop_front();
        }
        self.hint = None;
        for c in cues.into_iter().rev() {
            self.queues[qi].0.push_front(c);
        }
    }

    // ================= вызовы методов из анимаций =================

    fn on_method(&mut self, _node: usize, name: &str) -> Vec<Cue> {
        match name {
            "KickDoorLobby" => {
                self.stats.doors_kicked += 1;
                vec![wait(0.25), act(|c| {
                    c.music_muffled = false;
                    c.sounds.stop_loop(&mut c.engine, "restroom");
                })]
            }
            "PanFilter" => {
                self.music_muffled = true;
                vec![]
            }
            "KickDoorBackroom" => {
                self.stats.doors_kicked += 1;
                vec![]
            }
            "BeginGame" => self.main_batch_setup(true),
            "SetupShell" => {
                let live = self.game.shells.first().copied().unwrap_or(false);
                let g = &mut self.main().graph;
                let i = g.node(N_EXAMINE_SHELL);
                g.nodes[i].variant = if live { 1 } else { 2 };
                g.nodes[i].visible = true;
                vec![]
            }
            "RevertShell" => vec![vis(N_EXAMINE_SHELL, false)],
            "UpdateDisplayRoutineCigarette_Player" => self.cigarette_display(Side::Player),
            "UpdateDisplayRoutineCigarette_Enemy" => self.cigarette_display(Side::Dealer),
            "BeerEjection_player" => self.eject_shell(N_EJECT_PLAYER, A_EJECT_PLAYER, "ejecting shell_player1"),
            "BeerEjection_dealer" => self.eject_shell(N_EJECT_DEALER, A_EJECT_DEALER, "eject shell beer"),
            "HideSegment" => vec![vis(N_SEGMENT, false)],
            "BeginLerpCutSegment" => vec![cam("dealer cut shotgun")],
            "BeginLerpEnemy" => vec![cam("enemy")],
            "ShowDealerInspectionText" => vec![say("INTERESTING")],
            "HideText" => vec![hide_text()],
            "Speaker_HandCrack" => vec![act(|c| c.hand_crack())],
            "Animator_GiveHandcuffs" => vec![speaker("speaker parent/player give handcuffs")],
            "Animator_CheckHandcuffs" => vec![speaker("speaker parent/dealer check handcuffs")],
            "CheckPlayerHandCuffs_Animation" => vec![anim(A_CUFFBAR, "check handcuffs")],
            "Speaker_DealerArrive" | "Speaker_DealerArrive_Cuffed" => vec![speaker("speaker parent/dealer hands on table")],
            // ShellEjectManager.EjectShell / DeathEjection: гильза вылетает со стороны
            // стрелявшего, цвет — того патрона, что был в патроннике.
            "EjectShell" | "DeathEjection" => {
                let player_side = self.scenes[MAIN].graph.nodes[_node].parent.map(|p| self.scenes[MAIN].graph.nodes[p].name.contains("player side")).unwrap_or(false)
                    || self.scenes[MAIN].graph.nodes[_node].name.contains("player side");
                if player_side {
                    self.eject_shell(N_EJECT_PLAYER, A_EJECT_PLAYER, "ejecting shell_player1")
                } else {
                    self.eject_shell(N_EJECT_DEALER, A_EJECT_DEALER, "eject shell")
                }
            }
            "PlayShellDropSound" => {
                let n = 1 + (self.game.rng.next_u64() % 7) as usize;
                let path = format!("res://audio/shelldrop{}.ogg", n);
                self.sounds.play(&mut self.engine, &path, 0.8);
                vec![]
            }
            // CompartmentManager: звуки ящиков стола и чемодана.
            "PlaySound_ShowBriefcase" => vec![sfx("res://audio/briefcase compartment show.ogg")],
            "PlaySound_HideBriefcase" => vec![sfx("res://audio/briefcase compartment hide.ogg")],
            "PlaySound_ClearItems" => vec![sfx("res://audio/main compartment clear.ogg")],
            "PlaySound_ShowItems" => vec![sfx("res://audio/main compartment show items.ogg")],
            "PlaySound_HideItems" => vec![sfx("res://audio/main compartment hide items.ogg")],
            // HandcuffManager.AttachHandCuffs: дилер надел наручники на игрока.
            "AttachHandCuffs" => vec![anim(A_CUFFBAR, "RESET"), wait(0.1), vis("player cuff bar/handcuffs player bar", true)],
            "PlaySound_BreakHandcuffs" => vec![speaker("speaker parent/player break cuffs")],
            _ => vec![],
        }
    }

    fn hand_crack(&mut self) {
        let n = 1 + (self.game.rng.next_u64() % 6) as usize;
        let path = format!("res://audio/crack{}.ogg", n);
        self.sounds.play(&mut self.engine, &path, 0.7);
    }

    fn eject_shell(&mut self, node: &'static str, player: &'static str, name: &'static str) -> Vec<Cue> {
        // Выброшенный пивом патрон уже снят с очереди логикой — цвет запомнен.
        let live = self.last_ejected;
        let g = &mut self.main().graph;
        let i = g.node(node);
        g.nodes[i].variant = if live { 1 } else { 2 };
        g.nodes[i].visible = true;
        self.stats.shells_ejected += 1;
        vec![anim(player, name)]
    }
}


// ================= сценарии =================

impl Ctx {
    // ---------- меню (MenuManager) ----------

    pub fn enter_menu(&mut self) -> Vec<Cue> {
        self.ending_lines.clear();
        self.music_muffled = false;
        self.sounds.stop_music(&mut self.engine);
        self.sounds.stop_all_loops(&mut self.engine);
        self.active = MENU;
        self.black = false;
        self.brightness = 1.0;
        self.brightness_target = 1.0;
        self.dialogue = None;
        let s = &mut self.scenes[MENU];
        s.graph.reset();
        s.anims.stop_all();
        for p in ["shell waterfall2/AnimationPlayer", "shell waterfall4/AnimationPlayer"] {
            s.anims.play(&mut s.graph, p, "main action");
        }
        if let Some(i) = s.graph.find("speaker_music") {
            let path = s.graph.nodes[i].audio_path.clone();
            self.sounds.start_plain_music(&mut self.engine, &path);
        }
        vec![Cue::Input(Mode::Menu)]
    }

    fn start_new_game(&mut self) -> Vec<Cue> {
        self.sounds.stop_music(&mut self.engine);
        let seed = self.game.rng.next_u64();
        self.game = Game::new(seed);
        self.signed_waiver = false;
        self.name.clear();
        self.stats = Stats::default();
        self.read_intro = false;
        self.read_item_swap = false;
        self.read_item_distribution = false;
        self.read_item_intro2 = false;
        self.dialogue_reads = 0;
        self.drill_shown = false;
        self.entering_from_death = false;
        self.trackset = false;
        self.cutter_dialogue_read = false;
        self.reset_main();
        self.intro(false)
    }

    /// Сцена main.tscn с нуля (в оригинале — change_scene_to_file).
    fn reset_main(&mut self) {
        self.sounds.stop_music(&mut self.engine);
        self.sounds.stop_all_loops(&mut self.engine);
        self.active = MAIN;
        let s = &mut self.scenes[MAIN];
        s.graph.reset();
        s.anims.stop_all();
        self.cam.moving = false;
        self.dialogue = None;
        self.hint = None;
        self.black = true;
        self.brightness = 1.0;
        self.brightness_target = 1.0;
        self.shake = 0.0;
        self.tweens.clear();
        self.shells.clear();
        self.items = [[None; SLOTS]; 2];
        self.grab = None;
        self.dealer_at_table = false;
        self.dealer_holding_shotgun = false;
        // Сцена перезагружается: дилер снова здоровается ("С ВОЗВРАЩЕНИЕМ" после смерти).
        self.dealer_greeted = false;
        self.true_death_ready = false;
        self.ending_lines.clear();
        self.typed.clear();
        self.active_socket = "home";
        self.shell_insert_index = 0;
        self.waiting_for_return = false;
        self.dealer_low_pitched = false;
        for b in BRACKETS {
            self.scenes[MAIN].graph.set_visible(b, false);
        }
        self.barrel_sawed = false;
        self.lerping = false;
        self.waiting_for_dealer_return = false;
        self.careful_shown = [false; 2];
        self.entering_from_waiver = false;
        self.new_batch_begun = false;
        self.music_muffled = true;
        self.latches = [false; 2];
        // IntroManager._ready / Signature._ready / DeathManager._ready.
        let g = &mut self.scenes[MAIN].graph;
        for (node, v) in [(N_PILLS, false), (N_WAIVER_MACHINE, false), (N_DEFIB, false), (N_EXAMINE_SHELL, false), (N_SMOKER, false), ("Camera/blood splatter plane", false), ("player cuff bar/handcuffs player bar", false)] {
            g.set_visible(node, v);
        }
        // Надписи выбора цели появляются только с ружьём в руках.
        {
            let sc = &mut self.scenes[MAIN];
            sc.anims.play(&mut sc.graph, A_SHOOT_TEXT, "RESET");
        }
        let g = &mut self.scenes[MAIN].graph;
        // Раздавленная голова дилера в оригинале скрыта маской слоя камеры.
        let crushed = g.node(N_DEALER_CRUSHED);
        g.nodes[crushed].hide_self = true;
        if !self.signed_waiver {
            // SetupWaiver: ружьё уезжает под стол, бланк — на стол.
            let sg = g.node(N_SHOTGUN);
            g.nodes[sg].local.pos = Vec3::new(-1.738, -5.0, -0.041);
            let w = g.node(N_WAIVER_TABLE);
            g.nodes[w].local.pos = Vec3::new(-29.613, -35.341, -6.696);
        } else {
            g.set_visible(N_WAIVER_TABLE, false);
            g.set_visible(N_WAIVER_SIGNED, false);
        }
        self.update_counter(false);
    }

    // ---------- туалет (IntroManager) ----------

    fn load_track(&mut self) {
        let (main, second) = TRACKS[self.game.batch.min(2)];
        let path = if self.entering_from_death && !self.trackset {
            self.trackset = true;
            second
        } else {
            main
        };
        self.sounds.start_music(&mut self.engine, path, self.music_muffled, false);
        self.sounds.play_loop(&mut self.engine, "restroom", "res://audio/ambience1.ogg", 0.5);
    }

    pub fn intro(&mut self, revival: bool) -> Vec<Cue> {
        if !revival {
            return vec![
                anim(A_INTRO, "camera idle bathroom"),
                wait(0.5),
                act(|c| {
                    c.black = false;
                    c.load_track();
                }),
                wait(3.0),
                Cue::Input(Mode::BathroomDoor),
            ];
        }
        // RevivalBathroomStart: курильщик откачивает игрока на полу.
        vec![
            act(|c| {
                let cam = c.scenes[MAIN].camera;
                let g = &mut c.scenes[MAIN].graph;
                g.nodes[cam].local.pos = Vec3::new(-168.471, -1.749, 9.269);
                g.set_rotation(cam, Vec3::new(39.1, -53.5, -0.4) * (std::f32::consts::PI / 180.0));
                g.set_visible(N_SMOKER, true);
            }),
            wait(1.0),
            speaker("speaker parent/defib device_player"),
            wait(0.85),
            anim(A_SMOKER, "revive player"),
            act(|c| {
                c.shake_camera();
                c.black = false;
                c.load_track();
            }),
            wait(0.5),
            anim(A_SMOKER, "revive player"),
            say("YOURE LUCKY"),
            wait(4.0),
            act(|c| {
                let name = c.name.clone();
                let t = format!("{}\n{}", c.tr("GET UP").replace("%s", &name), c.tr("THE NIGHT"));
                c.show_text(t, None);
            }),
            wait(4.0),
            hide_text(),
            act(|c| c.brightness_target = 0.0),
            wait(2.05),
            vis(N_SMOKER, false),
            anim(A_INTRO, "camera idle bathroom"),
            act(|c| c.brightness_target = 1.0),
            wait(3.0),
            Cue::Input(Mode::BathroomDoor),
        ]
    }

    // ---------- начало набора (RoundManager.MainBatchSetup) ----------

    fn main_batch_setup(&mut self, dealer_enter: bool) -> Vec<Cue> {
        let mut v = Vec::new();
        if !self.entering_from_waiver {
            if self.lerping {
                v.push(cam("enemy"));
            }
            if !self.dealer_at_table && dealer_enter {
                v.push(wait(0.5));
                v.push(anim(A_HANDS, "dealer hands on table"));
                v.push(anim(A_DEALER, "dealer return to table"));
                v.push(wait(2.0));
                v.push(then(|c| {
                    if !c.signed_waiver {
                        return vec![say("WAIVER"), wait(2.3), hide_text(), cam("home"), wait(0.6), Cue::Input(Mode::Waiver)];
                    }
                    let mut v = Vec::new();
                    if !c.dealer_greeted {
                        let text = if c.entering_from_death { c.tr("WELCOME") } else { "...".to_string() };
                        v.push(act(move |c| c.show_text(text, None)));
                        v.push(wait(2.3));
                        v.push(hide_text());
                        v.push(act(|c| c.dealer_greeted = true));
                    }
                    v.push(act(|c| c.dealer_at_table = true));
                    v.push(then(|c| c.batch_continue()));
                    v
                }));
                return v;
            }
        }
        v.push(then(|c| c.batch_continue()));
        v
    }

    fn batch_continue(&mut self) -> Vec<Cue> {
        self.entering_from_waiver = false;
        let batch = self.game.batch;
        self.game.start_batch(batch);
        self.next_load_first = true;
        self.update_counter(false);
        let mut v = self.round_indicator();
        v.push(act(|c| c.lerping = true));
        v.push(then(|c| c.start_round(false)));
        v
    }

    fn update_counter(&mut self, on: bool) {
        let g = &mut self.scenes[MAIN].graph;
        for side in ["health UI_dealer side", "health UI_player side", "health UI_dividing line"] {
            g.set_visible(&format!("{}/{}", N_COUNTER_UI, side), on);
        }
    }

    /// HealthCounter.UpdateDisplay: символы зарядов.
    fn update_display(&mut self) {
        let hp = self.game.hp;
        let g = &mut self.scenes[MAIN].graph;
        for (side, who, h) in [("health UI_player side", "player", hp[0]), ("health UI_dealer side", "dealer", hp[1])] {
            g.set_visible(&format!("{}/{}/symbol_{} blank", N_COUNTER_UI, side, who), true);
            for i in 1..=6 {
                g.set_visible(&format!("{}/{}/symbol_{}{}", N_COUNTER_UI, side, who, i), i as i32 <= h);
            }
        }
        let flicker = |c: &mut Ctx, p: &'static str, on: bool| c.play(p, if on { "flicker digit" } else { "RESET" });
        flicker(self, A_FLICKER_P, hp[0] == 1);
        flicker(self, A_FLICKER_D, hp[1] == 1);
    }

    fn clear_display(&mut self) {
        let g = &mut self.scenes[MAIN].graph;
        for (side, who) in [("health UI_player side", "player"), ("health UI_dealer side", "dealer")] {
            g.set_visible(&format!("{}/{}/symbol_{} blank", N_COUNTER_UI, side, who), false);
            for i in 1..=6 {
                g.set_visible(&format!("{}/{}/symbol_{}{}", N_COUNTER_UI, side, who, i), false);
            }
        }
    }

    fn round_indicator(&mut self) -> Vec<Cue> {
        let pos = Vec3::from(ROUND_IND_POS[self.game.batch + 1]);
        vec![
            vis(N_ROUND_IND, false),
            anim(A_ROUND_BLINK, "RESET"),
            cam("health counter"),
            wait(0.8),
            act(move |c| {
                let g = &mut c.scenes[MAIN].graph;
                let i = g.node(N_ROUND_IND);
                g.nodes[i].local.pos = pos;
                g.set_visible(N_ROUND_IND_PARENT, true);
            }),
            speaker("tabletop parent/main tabletop/health counter/speaker_round indicator hum"),
            wait(0.8),
            vis(N_ROUND_IND, true),
            anim(A_ROUND_BLINK, "round blinking"),
            wait(2.0),
            vis(N_ROUND_IND_PARENT, false),
            speaker("tabletop parent/main tabletop/health counter/speaker_round indicator shutdown"),
            anim(A_ROUND_BLINK, "RESET"),
        ]
    }

    // ---------- зарядка (RoundManager.StartRound) ----------

    fn start_round(&mut self, getting_next: bool) -> Vec<Cue> {
        self.next_load_first = !getting_next && self.next_load_first;
        let mut v = Vec::new();
        // RemoveAllCuffsRoutine: дилер и игрок рвут наручники перед раздачей.
        if self.game.cuffed[Side::Dealer.idx()] {
            v.extend([
                cam("enemy cuffs close"),
                wait(0.3),
                anim(A_HANDS, "dealer break cuffs"),
                speaker("speaker parent/dealer break handcuffs"),
                wait(0.47),
            ]);
        }
        if self.game.cuffed[Side::Player.idx()] {
            v.extend([
                cam("check handcuffs"),
                wait(0.6),
                anim(A_CUFFBAR, "player break handcuffs"),
                wait(0.8),
            ]);
        }
        v.push(act(|c| {
            c.game.cuffed = [false; 2];
            c.game.about_to_break_free = [false; 2];
            c.waiting_for_return = false;
            c.scenes[MAIN].graph.set_visible("player cuff bar/handcuffs player bar", false);
        }));
        if self.game.final_showdown() && !self.true_death_ready {
            v.extend([act(|c| c.dealer_low_pitched = true), cam("enemy"), wait(0.6)]);
            if !self.cutter_dialogue_read {
                v.extend([
                    say("FINAL SHOW1"),
                    wait(4.0),
                    say("FINAL SHOW2"),
                    wait(4.0),
                    say("FINAL SHOW3"),
                    wait(4.8),
                    act(|c| c.cutter_dialogue_read = true),
                ]);
            } else {
                v.extend([say("BETTER NOT"), wait(3.0)]);
            }
            v.extend([
                hide_text(),
                anim("health counter main1/main counter/animator_defib cutter", "initial setup"),
                speaker("health counter main1/defib_bootup"),
                wait(3.1),
                act(|c| c.true_death_ready = true),
            ]);
        }
        v.push(then(|c| if c.game.items_per_load() > 0 { c.begin_item_grabbing() } else { c.main_shell_routine() }));
        v
    }

    // ---------- предметы (ItemManager) ----------

    fn begin_item_grabbing(&mut self) -> Vec<Cue> {
        self.grabbed = 0;
        let mut v = vec![cam(if self.read_item_swap { "home" } else { "enemy" })];
        if self.new_batch_begun {
            if self.items.iter().flatten().any(|s| s.is_some()) {
                v.extend([wait(0.8), anim(A_COMPARTMENTS, "clear items"), wait(1.4), act(|c| c.clear_items())]);
            }
            self.new_batch_begun = false;
        }
        v.push(wait(0.8));
        if !self.read_item_swap {
            v.extend([say_for("MORE INTERESTING", 3.0), wait(3.0), cam("home"), wait(0.8), act(|c| c.read_item_swap = true)]);
        }
        v.extend([
            anim(A_BRIEFCASE, "show briefcase"),
            wait(0.8),
            anim(A_COMPARTMENTS, "show items"),
            wait(0.8),
            cam("briefcase"),
            wait(0.8),
        ]);
        let n = self.game.items_per_load();
        let each = self.tr("ITEMS EACH").replace("%s", &n.to_string());
        if !self.read_item_distribution {
            let each = each.clone();
            v.extend([
                act(move |c| c.show_text(each, None)),
                wait(2.5),
                say("MORE ITEMS"),
                wait(2.5),
                hide_text(),
                act(|c| c.read_item_distribution = true),
            ]);
        } else if self.game.final_showdown() && !self.read_item_intro2 {
            v.extend([act(move |c| c.show_text(each, None)), wait(2.5), hide_text(), act(|c| c.read_item_intro2 = true)]);
        }
        v.push(Cue::Input(Mode::GrabItem));
        v
    }

    fn clear_items(&mut self) {
        for side in 0..2 {
            for slot in 0..SLOTS {
                if let Some(id) = self.items[side][slot].take() {
                    self.scenes[MAIN].graph.despawn(id);
                }
            }
        }
        self.game.items = [[None; SLOTS]; 2];
    }

    fn intake_local(&self, pos: [f32; 3], rot: [f32; 3]) -> (Vec3, Vec3) {
        (Vec3::from(pos), Vec3::from(rot))
    }

    fn grid_local(&self, side: Side, slot: usize, item: Item) -> (Vec3, Vec3) {
        let g = &self.scenes[MAIN].graph;
        let name = if side == Side::Player { format!("grid collider{}", slot) } else { format!("grid collider{}{}", slot, slot) };
        let i = g.node(&format!("{}/{}", N_INTAKE, name));
        let (op, or) = item_res(item).offset[side.idx()];
        (g.nodes[i].local.pos + Vec3::from(op), g.nodes[i].euler * (180.0 / std::f32::consts::PI) + Vec3::from(or))
    }

    fn spawn_item(&mut self, item: Item, at: (Vec3, Vec3)) -> u32 {
        let g = &mut self.scenes[MAIN].graph;
        let parent = g.node(N_INTAKE);
        let local = Trs { pos: at.0, rot: euler_yxz(at.1 * (std::f32::consts::PI / 180.0)), scale: Vec3::ONE };
        g.spawn(item_res(item).prefab, parent, local, 0)
    }

    fn tween(&mut self, spawn: u32, from: (Vec3, Vec3), to: (Vec3, Vec3), dur: f32, curve: f32) {
        self.tweens.retain(|t| t.spawn != spawn);
        self.tweens.push(ItemTween { spawn, from, to, t: 0.0, dur, curve });
    }

    /// ItemManager.GrabItem: следующий предмет из чемодана — в руку.
    fn grab_item(&mut self) -> Vec<Cue> {
        let Some(item) = self.game.draw_item(Side::Player) else {
            // Сетка полна — оригинал кладёт предмет обратно и заканчивает раздачу.
            return self.end_item_grabbing();
        };
        self.grabbed += 1;
        let r = item_res(item);
        let from = self.intake_local(r.in_briefcase.0, r.in_briefcase.1);
        let to = self.intake_local(r.in_hand.0, r.in_hand.1);
        let id = self.spawn_item(item, from);
        self.tween(id, from, to, 0.45, 0.4);
        self.grab = Some((item, id));
        let n = 1 + (self.game.rng.next_u64() % 5) as usize;
        let path = format!("res://audio/grab item{}.ogg", n);
        self.sounds.play(&mut self.engine, &path, 0.8);
        vec![
            wait(0.25),
            Cue::Input(Mode::PlaceItem),
        ]
    }

    fn place_item(&mut self, slot: usize) -> Vec<Cue> {
        let Some((item, id)) = self.grab.take() else { return vec![] };
        let r = item_res(item);
        let from = self.intake_local(r.in_hand.0, r.in_hand.1);
        let to = self.grid_local(Side::Player, slot, item);
        self.tween(id, from, to, 0.45, 0.4);
        self.game.place_item(Side::Player, slot, item);
        self.items[0][slot] = Some(id);
        let path = format!("res://audio/place down {}.ogg", match item {
            Item::Handsaw => "handsaw",
            Item::Magnifier => "magnifier",
            Item::Beer => "beer",
            Item::Cigarettes => "cigarettes",
            Item::Handcuffs => "handcuffs",
        });
        self.sounds.play(&mut self.engine, &path, 0.8);
        if self.grabbed >= self.game.items_per_load() {
            let mut v = vec![wait(0.45)];
            v.extend(self.end_item_grabbing());
            return v;
        }
        vec![wait(0.45), Cue::Input(Mode::GrabItem)]
    }

    fn end_item_grabbing(&mut self) -> Vec<Cue> {
        // GrabItems_Enemy: предметы дилера сразу в его ячейки.
        for (slot, item) in self.game.deal_dealer_items() {
            let at = self.grid_local(Side::Dealer, slot, item);
            let id = self.spawn_item(item, at);
            self.items[1][slot] = Some(id);
        }
        vec![
            wait(0.45),
            anim(A_BRIEFCASE, "hide briefcase"),
            wait(1.0),
            cam("home"),
            wait(0.9),
            then(|c| c.main_shell_routine()),
        ]
    }

    // ---------- показ и зарядка патронов (ShellSpawner / ShellLoader) ----------

    fn main_shell_routine(&mut self) -> Vec<Cue> {
        let sped_up = self.game.batch != 0;
        let mut v = Vec::new();
        if self.next_load_first {
            // bootingUpCounter: первая зарядка набора включает счётчик.
            v.extend([
                cam("health counter"),
                wait(0.5),
                speaker("tabletop parent/main tabletop/health counter/speaker_health counter bootup"),
                act(|c| {
                    c.clear_display();
                    c.update_counter(true);
                    c.update_name_labels();
                }),
                wait(0.85),
                speaker("tabletop parent/main tabletop/health counter/speaker_health counter beep"),
                act(|c| c.update_display()),
                wait(1.4),
            ]);
        }
        v.push(cam("shell compartment"));
        v.push(wait(0.5));
        v.push(then(move |c| {
            let first = c.next_load_first;
            c.next_load_first = false;
            let shown = c.game.next_load(first);
            for id in c.shells.drain(..) {
                c.scenes[MAIN].graph.despawn(id);
            }
            let g = &mut c.scenes[MAIN].graph;
            let parent = g.node(N_SHELLBOX);
            for (i, live) in shown.iter().enumerate().take(SHELL_SPOTS.len()) {
                let local = Trs {
                    pos: Vec3::from(SHELL_SPOTS[i]),
                    rot: euler_yxz(Vec3::new(-90.0, -90.0, 180.0) * (std::f32::consts::PI / 180.0)),
                    scale: Vec3::ONE,
                };
                let id = g.spawn("shell", parent, local, if *live { 1 } else { 2 });
                c.shells.push(id);
            }
            let live = c.game.live_left();
            let blank = c.game.blank_left();
            // PlayAudioIndicators: короткие сигналы по каждому патрону.
            let mut beeps = vec![wait(0.37)];
            for s in &shown {
                beeps.push(speaker_static(if *s { "res://audio/shell indicator_live.ogg" } else { "res://audio/shell indicator_blank.ogg" }));
                beeps.push(wait(0.07));
            }
            let lives = c.tr(if live == 1 { "LIVEROUND" } else { "LIVEROUNDS" }).replace("%s", &live.to_string());
            let blanks = c.tr(if blank == 1 { "BLANKROUND" } else { "BLANKROUNDS" }).replace("%s", &blank.to_string());
            let mut v = vec![anim(A_SHELLBOX, "show shells"), act(|c| c.latch_sound()), Cue::Fork(beeps), wait(1.0)];
            let skipping = c.game.final_showdown();
            let mut dur = 2.5;
            if !skipping {
                let text = format!("{} {}", lives, blanks);
                v.push(act(move |c| c.show_text(text, None)));
            } else if !c.drill_shown {
                v.push(say("DRILL"));
                c.drill_shown = true;
            } else {
                dur = 1.3;
            }
            v.extend([
                wait(dur),
                hide_text(),
                anim(A_SHELLBOX, "hide shells"),
                act(|c| c.latch_sound()),
                wait(if sped_up { 0.2 } else { 0.5 }),
                then(move |c| c.load_shells(sped_up)),
            ]);
            v
        }));
        v
    }

    fn latch_sound(&mut self) {
        let n = 1 + (self.game.rng.next_u64() % 2) as usize;
        let path = format!("res://audio/shell latch{}.ogg", n);
        self.sounds.play(&mut self.engine, &path, 0.8);
    }

    fn dealer_hands_grab_shotgun(&mut self) -> Vec<Cue> {
        vec![
            anim(A_HANDS, "dealer grab shotgun"),
            wait(0.45),
            act(|c| {
                c.hand_crack();
                c.scenes[MAIN].graph.set_visible(N_HAND_L, true);
                c.scenes[MAIN].graph.set_visible(N_HAND_R, true);
            }),
        ]
    }

    fn dealer_hands_drop_shotgun(&mut self) {
        self.scenes[MAIN].graph.set_visible(N_HAND_L, false);
        self.scenes[MAIN].graph.set_visible(N_HAND_R, false);
        self.play(A_HANDS, "RESET");
    }

    fn load_shells(&mut self, sped_up: bool) -> Vec<Cue> {
        let mut v = vec![cam("enemy")];
        if !sped_up {
            v.push(wait(0.8));
        }
        v.extend(self.dealer_hands_grab_shotgun());
        v.extend([wait(0.2), anim(A_SHOTGUN, "grab shotgun_pointing enemy"), wait(0.45)]);
        if self.dialogue_reads < 3 {
            let key = if self.shell_insert_index % 2 == 0 { "SHELL INSERT1" } else { "SHELL INSERT2" };
            self.shell_insert_index += 1;
            v.extend([say_for(key, 3.0), wait(3.0), act(|c| c.dialogue_reads += 1)]);
        }
        let n = self.game.shells.len();
        for _ in 0..n {
            v.extend([speaker("speaker parent/load shell"), anim(A_LOAD_HAND, "load single shell"), wait(if sped_up { 0.17 } else { 0.32 })]);
        }
        v.extend([anim(A_LOAD_HAND, "RESET"), act(|c| c.hand_crack()), wait(if sped_up { 0.17 } else { 0.42 })]);
        if !self.read_intro {
            // introductionDialogues из main.tscn (в оригинале не переведены).
            const INTRO: [(&str, f32); 9] = [
                ("ПРАВИЛА ПРОСТЫ:", 1.9),
                ("ВЫСТРЕЛ В СЕБЯ ХОЛОСТЫМ\nПРОПУСКАЕТ МОЙ ХОД.", 3.0),
                ("ВЫСТРЕЛ В ДИЛЕРА БОЕВЫМ\nЗАБИРАЕТ ЕГО ЖИЗНЬ.", 3.0),
                ("МЫ СЫГРАЕМ НЕСКОЛЬКО\nРАУНДОВ ...", 3.0),
                ("ПОКА НЕ ДОЙДЁМ\nДО ФИНАЛЬНОЙ ДУЭЛИ.", 3.0),
                ("ТАМ МЫ ПОТАНЦУЕМ\nНА ГРАНИ ЖИЗНИ И СМЕРТИ.", 3.7),
                ("ПОБЕДИШЬ — УЙДЁШЬ\nС 400 000$ НАЛИЧНЫМИ.", 3.7),
                ("ПРОИГРАЕШЬ — МЫ ПРОДАДИМ\nТВОЙ ИЗУРОДОВАННЫЙ ТРУП.", 3.7),
                ("НАЧНЁМ?", 2.5),
            ];
            for (text, t) in INTRO {
                v.push(act(move |c| c.show_text(text.to_string(), None)));
                v.push(wait(t));
            }
            v.extend([act(|c| c.read_intro = true), hide_text()]);
        }
        v.extend([
            anim(A_SHOTGUN, "enemy rack shotgun start"),
            wait(0.8),
            anim(A_SHOTGUN, "enemy put down shotgun"),
            act(|c| c.dealer_hands_drop_shotgun()),
            cam("home"),
            wait(0.6),
        ]);
        v.push(then(|c| c.player_turn_ready()));
        v
    }

    // ---------- ход игрока ----------

    fn player_turn_ready(&mut self) -> Vec<Cue> {
        vec![Cue::Input(Mode::Turn)]
    }

    /// RoundManager.BeginPlayerTurn.
    fn begin_player_turn(&mut self) -> Vec<Cue> {
        let p = Side::Player.idx();
        if self.game.cuffed[p] {
            if !self.game.about_to_break_free[p] {
                self.game.about_to_break_free[p] = true;
                return vec![
                    cam("check handcuffs"),
                    anim(A_CUFFBAR, "check handcuffs"),
                    wait(1.4),
                    cam("enemy"),
                    then(|c| c.dealer_turn()),
                ];
            }
            self.game.cuffed[p] = false;
            self.game.about_to_break_free[p] = false;
            return vec![
                anim(A_CUFFBAR, "player break handcuffs"),
                wait(1.4),
                cam("home"),
                vis("player cuff bar/handcuffs player bar", false),
                wait(0.6),
                then(|c| c.player_turn_ready()),
            ];
        }
        vec![wait(0.6), then(|c| c.player_turn_ready())]
    }

    fn player_grab_shotgun(&mut self) -> Vec<Cue> {
        vec![
            anim(A_SHOTGUN, "player grab shotgun"),
            anim(A_SHOOT_TEXT, "show text"),
            wait(0.5),
            Cue::Input(Mode::Holding),
        ]
    }

    fn play_shooting_sound(&mut self, live: bool) {
        if live {
            self.stats.shots += 1;
            self.play_node("speaker parent/gunshot_live");
            self.play(A_MUZZLE, "muzzle flash fire");
            self.play(A_MUZZLE_MODEL, "fire");
            self.shake_camera();
        } else {
            self.play_node("speaker parent/gunshot_blank");
        }
    }

    /// ShotgunShooting.Shoot.
    fn player_shoot(&mut self, target: Side) -> Vec<Cue> {
        let anim_name = if target == Side::Player { "player shoot self" } else { "player shoot dealer" };
        vec![
            wait(0.25),
            anim(A_SHOTGUN, anim_name),
            wait(0.5),
            cam("enemy"),
            wait(2.0),
            then(move |c| {
                let shot = c.game.shoot(Side::Player, target);
                c.last_ejected = shot.live;
                c.play_shooting_sound(shot.live);
                if shot.live && target == Side::Dealer {
                    return c.kill_dealer(false);
                }
                if shot.live && target == Side::Player {
                    c.player_shot_self = true;
                    return c.kill_player(true);
                }
                c.player_can_go_again = shot.again;
                if shot.again {
                    vec![
                        wait(0.3),
                        anim(A_SHOTGUN, "player eject shell"),
                        wait(0.85),
                        cam("home"),
                        wait(1.35),
                        then(|c| c.finalize_shooting(true, true, false, false)),
                    ]
                } else {
                    vec![wait(0.3), then(|c| c.shooting_dealer_ejection(false))]
                }
            }),
        ]
    }

    fn shooting_dealer_ejection(&mut self, wait_revival: bool) -> Vec<Cue> {
        vec![anim(A_SHOTGUN, "player eject shell2"), wait(2.0), then(move |c| c.finalize_shooting(false, true, wait_revival, false))]
    }

    fn finalize_shooting(&mut self, again: bool, place: bool, wait_revival: bool, add_delay: bool) -> Vec<Cue> {
        let mut v = Vec::new();
        if place {
            v.push(anim(A_SHOTGUN, "player place on table after eject"));
        }
        v.push(wait(0.6));
        if wait_revival {
            v.push(wait(2.0));
        }
        if add_delay {
            v.push(wait(3.0));
        }
        v.push(then(move |c| if c.game.hp[1] != 0 { c.end_turn(again) } else { vec![] }));
        v
    }

    /// RoundManager.EndTurn.
    fn end_turn(&mut self, player_again: bool) -> Vec<Cue> {
        let mut v = Vec::new();
        if self.barrel_sawed {
            self.barrel_sawed = false;
            v.extend([wait(0.6), cam("grow barrel"), wait(0.6), vis(N_SEGMENT, true), sfx("res://audio/grow barrel.ogg"), wait(1.0), cam("home"), wait(0.4)]);
        }
        v.push(then(move |c| {
            if c.game.shells.is_empty() {
                return c.start_round(true);
            }
            if player_again {
                return c.begin_player_turn();
            }
            if !c.game.cuffed[Side::Dealer.idx()] {
                return c.dealer_turn();
            }
            if c.waiting_for_return {
                c.waiting_for_return = false;
                let mut v = vec![wait(1.4)];
                v.extend(c.dealer_check_handcuffs());
                return v;
            }
            c.dealer_check_handcuffs()
        }));
        v
    }

    // ---------- предметы игрока (ItemInteraction) ----------

    fn player_use_item(&mut self, slot: usize) -> Vec<Cue> {
        let Some(id) = self.items[0][slot].take() else { return vec![] };
        let Some((item, revealed)) = self.game.use_item(Side::Player, slot) else {
            self.items[0][slot] = Some(id);
            return vec![];
        };
        let from = self.scenes[MAIN].graph.spawned.get(&id).map(|s| (s.local.pos, quat_deg(s.local.rot))).unwrap_or((Vec3::ZERO, Vec3::ZERO));
        self.tween(id, from, (Vec3::new(12.8262, 1.46311, -0.159848), Vec3::new(0.0, 90.0, -90.0)), 0.5, 1.0);
        let pickup = match item {
            Item::Handsaw => "res://audio/pick up metal.ogg",
            Item::Magnifier => "res://audio/pick up magnifier.ogg",
            Item::Beer => "res://audio/pick up beer.ogg",
            Item::Cigarettes => "res://audio/pick up cigarettes.ogg",
            Item::Handcuffs => "res://audio/pick up handcuffs.ogg",
        };
        self.sounds.play(&mut self.engine, pickup, 0.8);
        let mut v = vec![wait(0.4), act(move |c| c.scenes[MAIN].graph.despawn(id))];
        match item {
            Item::Handcuffs => v.extend([
                anim(A_HANDS, "dealer get handcuffed"),
                wait(1.0),
                cam("enemy"),
                wait(1.3),
                cam("dealer handcuffs"),
                wait(1.3),
                cam("home"),
                wait(0.6),
                then(|c| c.player_turn_ready()),
            ]),
            Item::Beer => {
                self.stats.beer_ml += 330;
                self.last_ejected = revealed.unwrap_or(false);
                let last = self.game.shells.is_empty();
                v.extend([anim(A_PITEMS, player_use_anim(item)), wait(5.6)]);
                v.push(then(move |c| if last { c.start_round(true) } else { c.player_turn_ready() }));
            }
            Item::Magnifier => {
                let len = self.anim_len(A_PITEMS, "player use magnifier");
                v.extend([anim(A_PITEMS, player_use_anim(item)), wait(len + 0.2), then(|c| c.player_turn_ready())]);
            }
            Item::Cigarettes => {
                self.stats.cigs += 1;
                v.extend([anim(A_PITEMS, player_use_anim(item)), wait(5.0), anim(A_PITEMS, "player revert cigarette"), then(|c| c.player_turn_ready())]);
            }
            Item::Handsaw => {
                self.barrel_sawed = true;
                v.extend([anim(A_PITEMS, player_use_anim(item)), wait(4.48), then(|c| c.player_turn_ready())]);
            }
        }
        v
    }

    fn cigarette_display(&mut self, side: Side) -> Vec<Cue> {
        let changed = self.game.heal(side);
        let back = self.active_socket;
        vec![
            cam("health counter"),
            wait(0.8),
            act(move |c| {
                c.update_display();
                if changed {
                    c.play_node("tabletop parent/main tabletop/health counter/speaker_health counter beep");
                }
            }),
            wait(0.7),
            cam(back),
        ]
    }

    // ---------- дилер (DealerIntelligence / HandManager) ----------

    fn dealer_check_handcuffs(&mut self) -> Vec<Cue> {
        let d = Side::Dealer.idx();
        let mut v = vec![cam("enemy cuffs close"), wait(0.3)];
        if !self.game.about_to_break_free[d] {
            self.game.about_to_break_free[d] = true;
            v.extend([speaker("speaker parent/dealer check handcuffs"), anim(A_HANDS, "dealer check handcuffs"), wait(0.7), cam("home"), then(|c| c.begin_player_turn())]);
        } else {
            self.game.cuffed[d] = false;
            self.game.about_to_break_free[d] = false;
            v.extend([anim(A_HANDS, "dealer break cuffs"), speaker("speaker parent/dealer break handcuffs"), wait(0.8), cam("enemy"), then(|c| c.dealer_turn())]);
        }
        v
    }

    fn dealer_turn(&mut self) -> Vec<Cue> {
        if self.game.shells.is_empty() {
            return self.start_round(true);
        }
        let action = self.game.dealer_decide();
        let mut v = Vec::new();
        match action {
            Action::UseItem(slot) => {
                if self.dealer_holding_shotgun {
                    self.dealer_holding_shotgun = false;
                    v.extend([anim(A_SHOTGUN, "enemy put down shotgun"), act(|c| c.dealer_hands_drop_shotgun()), wait(0.45)]);
                }
                if self.waiting_for_dealer_return {
                    self.waiting_for_dealer_return = false;
                    v.push(wait(1.8));
                }
                v.push(then(move |c| c.dealer_pickup_item(slot)));
            }
            Action::Shoot(target) => {
                if self.waiting_for_dealer_return {
                    self.waiting_for_dealer_return = false;
                    v.push(wait(1.8));
                }
                if !self.dealer_holding_shotgun {
                    v.extend(self.dealer_hands_grab_shotgun());
                    v.extend([wait(0.2), anim(A_SHOTGUN, "grab shotgun_pointing enemy"), act(|c| c.dealer_holding_shotgun = true), wait(0.9)]);
                }
                v.push(wait(1.0));
                v.push(then(move |c| c.dealer_shoot(target)));
            }
        }
        v
    }

    fn dealer_pickup_item(&mut self, slot: usize) -> Vec<Cue> {
        let Some(id) = self.items[1][slot].take() else { return self.dealer_turn() };
        let right = self.scenes[MAIN].graph.spawned.get(&id).is_some_and(|s| s.local.pos.z > 0.0);
        let Some((item, revealed)) = self.game.use_item(Side::Dealer, slot) else {
            self.items[1][slot] = Some(id);
            return vec![wait(0.5), then(|c| c.dealer_turn())];
        };
        if item == Item::Beer {
            self.last_ejected = revealed.unwrap_or(false);
        }
        if item == Item::Handsaw {
            self.barrel_sawed = true;
        }
        let use_anim: &'static str = match item {
            Item::Handsaw => "dealer use handsaw",
            Item::Magnifier => "dealer use magnifying glass",
            Item::Beer => "dealer use beer",
            Item::Cigarettes => "dealer use cigarettes",
            Item::Handcuffs => "dealer use handcuffs",
        };
        let len = self.anim_len(A_HANDS, use_anim);
        let mut v = vec![
            act(|c| c.hand_crack()),
            anim(A_HANDS, "RESET"),
            anim(A_HEADLOOK, if right { "dealer look right" } else { "dealer look left" }),
            wait(0.3),
            act(move |c| {
                c.hand_crack();
                c.scenes[MAIN].graph.despawn(id);
            }),
            wait(0.2),
            anim(A_HEADLOOK, if right { "dealer look forward from right" } else { "dealer look forward from left" }),
            wait(0.71),
            anim(A_HANDS, use_anim),
            wait(len),
        ];
        if item == Item::Handcuffs {
            v.push(act(|c| c.game.cuffed[Side::Player.idx()] = true));
        }
        if item == Item::Cigarettes {
            v.push(wait(1.1));
        }
        v.push(then(|c| c.dealer_turn()));
        v
    }

    fn dealer_shoot(&mut self, target: Side) -> Vec<Cue> {
        let mut v = Vec::new();
        if target == Side::Dealer {
            v.extend([wait(0.2), anim(A_SHOTGUN, "enemy shoot self")]);
        } else {
            v.push(anim(A_SHOTGUN, "enemy shoot player"));
        }
        v.push(wait(2.0));
        v.push(then(move |c| {
            let shot = c.game.shoot(Side::Dealer, target);
            c.last_ejected = shot.live;
            c.play_shooting_sound(shot.live);
            if shot.live && target == Side::Dealer {
                return c.kill_dealer(true);
            }
            let mut v = Vec::new();
            if shot.live && target == Side::Player {
                v.extend(c.kill_player(false));
                v.push(then(|c| if c.game.hp[0] == 0 { vec![Cue::Wait(1e9)] } else { vec![] }));
            }
            let again = shot.again;
            v.extend([
                wait(0.4),
                anim(A_SHOTGUN, if target == Side::Player { "enemy eject shell_from player" } else { "enemy eject shell_from self" }),
                wait(1.7),
                then(move |c| c.end_dealer_turn(again)),
            ]);
            v
        }));
        v
    }

    fn end_dealer_turn(&mut self, again: bool) -> Vec<Cue> {
        if self.game.hp[1] == 0 {
            return self.out_of_health(Side::Dealer);
        }
        if again && !self.game.shells.is_empty() {
            return self.dealer_turn();
        }
        let mut v = vec![wait(0.5), cam("home")];
        if self.dealer_holding_shotgun {
            self.dealer_holding_shotgun = false;
            v.extend([anim(A_SHOTGUN, "enemy put down shotgun"), act(|c| c.dealer_hands_drop_shotgun())]);
        }
        v.push(then(|c| c.end_turn(true)));
        v
    }

    // ---------- смерть (DeathManager) ----------

    fn kill_player(&mut self, returning_shotgun: bool) -> Vec<Cue> {
        let mut v = vec![wait(0.08), act(|c| c.black = true)];
        if returning_shotgun {
            // DeathManager.Kill -> ejectManager_player.DeathEjection.
            v.extend([anim(A_SHOTGUN, "RESET"), then(|c| c.eject_shell(N_EJECT_PLAYER, A_EJECT_PLAYER, "ejecting shell_player1")), wait(2.0)]);
            v.push(then(|c| {
                let add = c.game.hp[1] == 1 || c.game.hp[0] == 1;
                if c.game.hp[0] != 0 {
                    vec![Cue::Fork(c.finalize_shooting(false, false, true, add))]
                } else {
                    vec![]
                }
            }));
        }
        v.push(then(|c| {
            if c.game.hp[0] == 0 {
                c.sounds.fade_music(0.0);
                return c.out_of_health(Side::Player);
            }
            vec![
                wait(0.4),
                speaker("speaker parent/defib device_player"),
                wait(0.85),
                speaker("speaker parent/heartbeat effect"),
                act(|c| {
                    let g = &mut c.scenes[MAIN].graph;
                    g.set_visible(N_DEFIB, true);
                    c.play(A_DEFIB, "RESET");
                    c.black = false;
                    c.shake_camera();
                }),
                wait(0.6),
                anim(A_DEFIB, "remove defib device"),
                wait(0.4),
                then(|c| c.health_update(Side::Player, false)),
                vis(N_DEFIB, false),
            ]
        }));
        v
    }

    fn kill_dealer(&mut self, returning_shotgun: bool) -> Vec<Cue> {
        let mut v = Vec::new();
        if returning_shotgun {
            self.dealer_holding_shotgun = false;
            v.push(anim(A_SHOTGUN, "enemy return shotgun self"));
            v.push(then(|c| c.eject_shell(N_EJECT_DEALER, A_EJECT_DEALER, "eject shell")));
        }
        v.extend([
            anim(A_DEALER, "dealer fly away"),
            anim(A_HANDS, "hide hands"),
            act(|c| {
                c.dealer_at_table = false;
                let g = &mut c.scenes[MAIN].graph;
                // DealerIntelligence.SwapDealerMesh: после первой "смерти" у дилера
                // навсегда (до перезагрузки сцены) раздавленная голова.
                let normal = g.node(N_DEALER_MESH);
                let crushed = g.node(N_DEALER_CRUSHED);
                g.nodes[normal].hide_self = true;
                g.nodes[crushed].hide_self = false;
                g.set_visible(N_HAND_L, false);
                g.set_visible(N_HAND_R, false);
            }),
            wait(0.4),
            speaker("speaker parent/speaker_dealer crash"),
        ]);
        if !returning_shotgun {
            v.push(then(|c| vec![Cue::Fork(c.shooting_dealer_ejection(true))]));
        }
        v.push(wait(1.8));
        v.push(then(move |c| {
            if c.game.hp[1] == 0 {
                let mut v = vec![cam("health counter"), wait(0.8), act(|c| c.update_display()), speaker("tabletop parent/main tabletop/health counter/speaker_health counter noise")];
                v.extend(c.out_of_health(Side::Dealer));
                return v;
            }
            let mut v = vec![then(|c| c.health_update(Side::Dealer, true))];
            let cuffed = c.game.cuffed[Side::Dealer.idx()];
            if cuffed {
                c.waiting_for_return = true;
            } else {
                c.waiting_for_dealer_return = true;
            }
            v.extend([
                anim(A_HANDS, if cuffed { "dealer hands on table cuffed" } else { "dealer hands on table" }),
                anim(A_DEALER, "dealer return to table"),
                act(|c| {
                    c.dealer_at_table = true;
                }),
                wait(2.0),
            ]);
            if returning_shotgun {
                v.push(then(|c| c.end_dealer_turn(false)));
            }
            v
        }));
        v
    }

    /// HealthCounter.UpdateDisplayRoutine (+ обрез провода в финале).
    fn health_update(&mut self, checking: Side, going_prev: bool) -> Vec<Cue> {
        let prev = self.active_socket;
        vec![
            cam("health counter"),
            wait(0.8),
            then(move |c| {
                let cut = c.game.check_wire();
                c.update_display();
                c.play_node("tabletop parent/main tabletop/health counter/speaker_health counter noise");
                let mut v = Vec::new();
                if !cut.is_empty() {
                    // DefibCutter.CutWire: лезвие режет провод — лечения больше нет.
                    v.extend([
                        speaker("health counter main1/defib_blade move"),
                        wait(0.6),
                        sfx("res://audio/blade cut.ogg"),
                        act(|c| c.update_display()),
                        wait(0.6),
                    ]);
                }
                v.push(wait(0.7));
                let i = checking.idx();
                if c.game.hp[i] == 1 && checking == Side::Player && !c.careful_shown[i] && !c.game.wire_cut[i] {
                    c.careful_shown[i] = true;
                    v.extend([say_for("CAREFUL", 3.0), wait(3.0)]);
                }
                v.push(cam(if going_prev { prev } else { "home" }));
                v.push(wait(0.4));
                v
            }),
        ]
    }

    fn out_of_health(&mut self, side: Side) -> Vec<Cue> {
        if side == Side::Player {
            return self.main_death_routine();
        }
        vec![wait(1.0), then(|c| c.end_main_batch())]
    }

    fn main_death_routine(&mut self) -> Vec<Cue> {
        let true_death = self.game.wire_cut[Side::Player.idx()];
        vec![wait(0.5), then(move |c| {
            // Как change_scene_to_file в оригинале: все сценарии стола обрываются.
            c.queues.truncate(1);
            c.queues[0].0.clear();
            c.queues[0].1 = 0.0;
            if true_death {
                return c.enter_heaven();
            }
            // death.tscn сразу перезагружает main.tscn: оживление в туалете, тот же набор.
            c.entering_from_death = true;
            c.reset_main();
            c.intro(true)
        })]
    }

    fn end_main_batch(&mut self) -> Vec<Cue> {
        self.game.batch += 1;
        self.new_batch_begun = true;
        let mut v = vec![wait(0.8)];
        if self.game.batch == 3 {
            v.extend([act(|c| c.update_counter(false)), wait(0.4), cam("enemy"), wait(0.7), then(|c| c.briefcase_machine())]);
            return v;
        }
        v.extend([
            act(|c| c.update_counter(false)),
            speaker("tabletop parent/main tabletop/health counter/speaker_round indicator shutdown"),
            wait(1.0),
            speaker("tabletop parent/main tabletop/health counter/speaker_player win"),
            vis(N_PLAYER_WIN, true),
            wait(2.33),
            speaker("tabletop parent/main tabletop/health counter/speaker_round indicator shutdown"),
            act(|c| c.sounds.fade_music(0.0)),
            vis(N_PLAYER_WIN, false),
        ]);
        if self.barrel_sawed {
            self.barrel_sawed = false;
            v.extend([wait(0.6), cam("grow barrel"), wait(0.6), vis(N_SEGMENT, true), sfx("res://audio/grow barrel.ogg"), wait(1.0)]);
        }
        v.push(then(|c| {
            let mut v = c.main_batch_setup(false);
            if !c.dealer_at_table {
                let cuffed = c.game.cuffed[Side::Dealer.idx()];
                v.insert(0, anim(A_HANDS, if cuffed { "dealer hands on table cuffed" } else { "dealer hands on table" }));
                v.insert(1, anim(A_DEALER, "dealer return to table"));
                v.insert(2, act(|c| c.dealer_at_table = true));
            }
            v.insert(0, act(|c| {
                let path = TRACKS[c.game.batch.min(2)].0;
                c.sounds.start_music(&mut c.engine, path, true, true);
            }));
            v
        }));
        v
    }

    // ---------- концовка (BriefcaseMachine / EndingManager) ----------

    fn briefcase_machine(&mut self) -> Vec<Cue> {
        vec![
            speaker("speaker parent/briefcase machine_machine main"),
            wait(0.42),
            vis(N_MACHINE_EYES, true),
            wait(1.28),
            vis(N_MACHINE_EYES, false),
            anim(A_MACHINE, "move to table"),
            vis(N_MACHINE, true),
            wait(5.14),
            cam("cash briefcase"),
            wait(0.8),
            Cue::Input(Mode::BriefcaseLatches),
        ]
    }

    fn open_latch(&mut self, right: bool) -> Vec<Cue> {
        let i = right as usize;
        if self.latches[i] {
            return vec![Cue::Input(Mode::BriefcaseLatches)];
        }
        self.latches[i] = true;
        let (node, player) = if right {
            ("speaker parent/briefcase machine_latch right", A_LATCH_R)
        } else {
            ("speaker parent/briefcase machine_latch left", A_LATCH_L)
        };
        self.play_node(node);
        self.play(player, "raise");
        if self.latches == [true, true] {
            return vec![wait(0.3), Cue::Input(Mode::BriefcaseLatches)];
        }
        vec![Cue::Input(Mode::BriefcaseLatches)]
    }

    fn begin_ending(&mut self) -> Vec<Cue> {
        self.cam.moving = false;
        self.play(A_LID, "open");
        self.play_node("speaker parent/briefcase machine_lid");
        self.play(A_PAN_BRIEF, "pan to brief");
        self.sounds.stop_music(&mut self.engine);
        if let Some(i) = self.scenes[MAIN].graph.find("speaker parent/speaker_main music ending") {
            let path = self.scenes[MAIN].graph.nodes[i].audio_path.clone();
            self.sounds.start_plain_music(&mut self.engine, &path);
        }
        let s = &self.stats;
        let total = (70_000i64 - s.cigs as i64 * 220 - (s.beer_ml as f64 * 1.5) as i64 - if s.doors_kicked > 2 { s.doors_kicked as i64 * 1000 } else { 0 }).max(0);
        let dots = "........................";
        let lines = vec![
            format!("{} {} {}", self.tr("SHOTS FIRED"), dots, s.shots),
            format!("{} {} {}", self.tr("SHELLS EJECTED"), dots, s.shells_ejected),
            format!("{} {} {}", self.tr("DOORS KICKED"), dots, s.doors_kicked),
            format!("{} {} {}", self.tr("CIGS SMOKED"), dots, s.cigs),
            format!("{} {} {}", self.tr("ML DRANK"), dots, s.beer_ml),
            format!("{} {} $", self.tr("TOTAL CASH"), total),
        ];
        let congrats = self.tr("CONGRATULATIONS").replace("%s", &self.name);
        vec![
            wait(10.67),
            act(move |c| {
                c.black = true;
                c.anims_stop(A_PAN_BRIEF);
                let cam = c.scenes[MAIN].camera;
                c.scenes[MAIN].graph.nodes[cam].fov = 60.0;
                c.play(A_STREETLIGHTS, "looping");
                c.play(A_BACKDROP, "loop exterior");
                c.play(A_CAR_CAM, "camera loop car");
                c.ending_lines = lines;
            }),
            wait(1.2),
            act(|c| c.black = false),
            wait(6.0),
            act(move |c| c.show_text(congrats, None)),
            wait(2.0),
            Cue::Input(Mode::AnyKeyToMenu),
        ]
    }

    fn anims_stop(&mut self, player: &str) {
        let a = self.active;
        self.scenes[a].anims.stop(player);
    }

    // ---------- рай (HeavenManager) ----------

    fn enter_heaven(&mut self) -> Vec<Cue> {
        self.sounds.stop_music(&mut self.engine);
        self.sounds.stop_all_loops(&mut self.engine);
        self.active = HEAVEN;
        self.black = false;
        self.dialogue = None;
        let s = &mut self.scenes[HEAVEN];
        s.graph.reset();
        s.anims.stop_all();
        s.anims.play(&mut s.graph, "animation parent/animator_camera", "camera pan down");
        s.anims.play(&mut s.graph, "animation parent/animator_birds", "bird fly");
        for i in 1..=9 {
            let p = if i == 1 { "main bird parent/bird parent/bird copy1/AnimationPlayer".to_string() } else { format!("main bird parent/bird parent{}/bird copy1/AnimationPlayer", i) };
            s.anims.play(&mut s.graph, &p, "flying loop");
        }
        if let Some(i) = s.graph.find("speaker_music").or_else(|| s.graph.find("speaker_music")) {
            let path = s.graph.nodes[i].audio_path.clone();
            self.sounds.start_plain_music(&mut self.engine, &path);
        } else {
            self.sounds.start_plain_music(&mut self.engine, "res://audio/music_true death vol2.ogg");
        }
        vec![
            wait(1.0),
            Cue::Input(Mode::Heaven),
        ]
    }

    // ================= ввод мышью =================
    // В оригинале всё управляется мышью: клики по коллайдерам
    // InteractionBranch (двери, бланк, клавиши машинки, чемодан, ячейки,
    // ружьё, предметы) и экранные кнопки (меню, "ДИЛЕР" / "ВЫ").

    /// Что сейчас можно нажать.
    pub fn targets(&self) -> Vec<Target> {
        let Some(mode) = self.waiting_for() else { return Vec::new() };
        match mode {
            Mode::Menu => vec![Target::Start, Target::Quit],
            Mode::BathroomDoor => vec![Target::BathroomDoor],
            Mode::BackroomDoor => vec![Target::BackroomDoor],
            Mode::Waiver => {
                let g = &self.scenes[MAIN].graph;
                if !g.nodes[g.node(N_WAIVER_MACHINE)].visible {
                    vec![Target::Waiver]
                } else {
                    let mut v: Vec<Target> = ('a'..='z').map(Target::Key).collect();
                    v.extend([Target::KeyEnter, Target::KeyBack]);
                    v
                }
            }
            Mode::GrabItem => vec![Target::Briefcase],
            Mode::PlaceItem => self.game.free_slots(Side::Player).into_iter().map(Target::Slot).collect(),
            Mode::Turn => {
                let mut v = vec![Target::Shotgun];
                v.extend((0..SLOTS).filter(|&s| self.game.can_use(Side::Player, s)).map(Target::Item));
                v
            }
            Mode::Holding => vec![Target::ShootDealer, Target::ShootSelf],
            Mode::BriefcaseLatches => {
                if self.latches == [true, true] {
                    vec![Target::Lid]
                } else {
                    let mut v = Vec::new();
                    if !self.latches[0] {
                        v.push(Target::LatchL);
                    }
                    if !self.latches[1] {
                        v.push(Target::LatchR);
                    }
                    v
                }
            }
            Mode::AnyKeyToMenu => vec![Target::Anywhere],
            Mode::Heaven => vec![Target::Retry, Target::ToMenu],
        }
    }

    /// Экранные кнопки текущего режима: текст, x, y, размер (как у TextLine, по центру).
    pub fn buttons(&self) -> Vec<(String, f32, f32, f32, Target)> {
        match self.waiting_for() {
            Some(Mode::Menu) => vec![("НАЧАТЬ".into(), 0.0, -0.3, 0.06, Target::Start), ("ВЫХОД".into(), 0.0, -0.42, 0.05, Target::Quit)],
            Some(Mode::Heaven) => vec![(self.tr("RETRY"), -0.25, -0.6, 0.06, Target::Retry), (self.tr("EXIT"), 0.25, -0.6, 0.06, Target::ToMenu)],
            _ => Vec::new(),
        }
    }

    fn target_spheres(&self, t: Target, out: &mut Vec<(Vec3, f32)>) {
        let g = &self.scenes[MAIN].graph;
        let meshes = &self.engine.meshes;
        let node = |path: &str, out: &mut Vec<(Vec3, f32)>| {
            if let Some(i) = g.find(path) {
                g.spheres(meshes, i, out);
            }
        };
        match t {
            Target::BathroomDoor => node("intro parent/bathroom door", out),
            Target::BackroomDoor => node("intro parent/backroom door", out),
            Target::Waiver => node(N_WAIVER_TABLE, out),
            Target::Key(c) => node(&format!("{}/button_{}", N_WAIVER_KEYS, c), out),
            Target::KeyEnter => node(&format!("{}/button_enter", N_WAIVER_KEYS), out),
            Target::KeyBack => node(&format!("{}/button_backspace", N_WAIVER_KEYS), out),
            Target::Briefcase => {
                node("tabletop parent/main tabletop/compartments parent/compartment user_briefcase", out);
                if let Some(i) = g.find(N_INTAKE) {
                    // Сам чемодан у края стола со стороны игрока (там же pos_inBriefcase предметов).
                    out.push((g.global[i].transform_point3(Vec3::new(8.6, 0.0, 0.0)), 2.0));
                }
            }
            Target::Slot(n) => {
                if let Some(i) = g.find(&format!("{}/grid collider{}", N_INTAKE, n)) {
                    out.push((g.global[i].w_axis.truncate(), 1.3));
                }
            }
            Target::Shotgun => node(N_SHOTGUN, out),
            // ShotgunShooting: надписи "ДИЛЕР" / "ВЫ" на столе (Label3D).
            Target::ShootDealer | Target::ShootSelf => {
                let path = if t == Target::ShootDealer { "ui parent_shooting decision/text dealer" } else { "ui parent_shooting decision/text you" };
                if let Some(i) = g.find(path) {
                    let h = g.nodes[i].label.as_ref().map(|l| l.font_size * l.pixel_size).unwrap_or(0.5);
                    out.push((g.global[i].w_axis.truncate(), (h * 2.0).max(0.8)));
                }
            }
            Target::Item(slot) => {
                if let Some(id) = self.items[0][slot] {
                    g.spawn_spheres(meshes, id, out);
                }
            }
            Target::LatchL => node(&A_LATCH_L[..A_LATCH_L.rfind('/').unwrap()], out),
            Target::LatchR => node(&A_LATCH_R[..A_LATCH_R.rfind('/').unwrap()], out),
            Target::Lid => node(&A_LID[..A_LID.rfind('/').unwrap()], out),
            _ => {}
        }
    }

    /// Куда на экране (NDC) навести курсор, чтобы попасть в цель — для автоигры.
    pub fn target_ndc(&self, t: Target) -> Option<[f32; 2]> {
        if let Some(b) = self.buttons().into_iter().find(|b| b.4 == t) {
            return Some([b.1, b.2 + b.3 * 0.5]);
        }
        if t == Target::Anywhere {
            return Some([0.0, 0.0]);
        }
        let mut spheres = Vec::new();
        self.target_spheres(t, &mut spheres);
        // Самая крупная сфера цели — её центр почти наверняка внутри объекта.
        let (c, _) = spheres.into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
        let cam = &self.engine.camera;
        let p = (cam.projection_matrix() * cam.view_matrix()).project_point3(c);
        (p.x.abs() <= 1.0 && p.y.abs() <= 1.0).then_some([p.x, p.y])
    }

    /// Объект сцены под курсором (`ndc` — позиция мыши), ближайший по лучу.
    pub fn pick(&self, ndc: [f32; 2]) -> Option<Target> {
        if self.active != MAIN {
            return None;
        }
        let cam = &self.engine.camera;
        let inv = (cam.projection_matrix() * cam.view_matrix()).inverse();
        let a = inv.project_point3(Vec3::new(ndc[0], ndc[1], 0.0));
        let b = inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.0));
        let dir = (b - a).normalize_or_zero();
        let mut best: Option<(f32, Target)> = None;
        let mut spheres = Vec::new();
        for t in self.targets() {
            spheres.clear();
            self.target_spheres(t, &mut spheres);
            for &(c, r) in &spheres {
                let oc = a - c;
                let bq = oc.dot(dir);
                let disc = bq * bq - (oc.length_squared() - r * r);
                if disc < 0.0 {
                    continue;
                }
                if -bq + disc.sqrt() <= 0.0 {
                    continue; // сфера позади камеры
                }
                // Сферы соседних мелких объектов (клавиши машинки) перекрываются,
                // поэтому побеждает та, к центру которой луч проходит ближе всего
                // относительно её радиуса, а не первая по глубине.
                let perp = (oc - dir * bq).length() / r.max(1e-4);
                let depth = (-bq - disc.sqrt()).max(0.0);
                let score = perp + depth * 0.01;
                if best.is_none_or(|(d, _)| score < d) {
                    best = Some((score, t));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// Signature: буквы набранного имени на экране машинки.
    fn update_waiver_letters(&mut self) {
        let typed: Vec<char> = self.typed.to_uppercase().chars().collect();
        let g = &mut self.scenes[MAIN].graph;
        for i in 0..6 {
            let t = typed.get(i).map(|c| c.to_string()).unwrap_or_default();
            g.set_label(&format!("{}/letter_{}", N_WAIVER_SCREEN, i), &t);
        }
        g.set_visible(&format!("{}/entry marker", N_WAIVER_SCREEN), typed.len() < 6);
    }

    /// Имя игрока на счётчике (HealthCounter.SetupHealth).
    fn update_name_labels(&mut self) {
        let name = self.name.clone();
        let win = self.tr("PLAYERWIN").replace("%s", &name);
        let g = &mut self.scenes[MAIN].graph;
        g.set_label(&format!("{}/health UI_player side/text_player", N_COUNTER_UI), &name);
        g.set_label(&format!("{}/indicator_player win", N_COUNTER_UI), &win);
    }

    /// Цель под курсором: запомнить и показать рамку вокруг неё.
    pub fn set_hover(&mut self, hover: Option<Target>) {
        self.hover = hover;
        let lit = match hover {
            Some(Target::BathroomDoor) => Some(BRACKETS[0]),
            Some(Target::BackroomDoor) => Some(BRACKETS[1]),
            Some(Target::Waiver) => Some(BRACKETS[4]),
            Some(Target::Briefcase) => Some(BRACKETS[5]),
            _ => None,
        };
        if self.active == MAIN {
            for b in BRACKETS {
                self.scenes[MAIN].graph.set_visible(b, Some(b) == lit);
            }
        }
    }

    /// Подпись под курсором (как описание предмета в оригинале).
    pub fn hover_text(&self) -> Option<(String, String)> {
        let key = match self.hover? {
            Target::Shotgun => "SHOTGUN",
            Target::Item(slot) => match self.game.items[0][slot]? {
                Item::Handsaw => "HANDSAW",
                Item::Magnifier => "MAGNIFIER",
                Item::Beer => "BEER",
                Item::Cigarettes => "CIGARETTE",
                Item::Handcuffs => "HANDCUFFS",
            },
            _ => return None,
        };
        Some((self.tr(key), self.tr(&format!("{} DESC", key))))
    }

    /// Нажатие на цель — то, что в оригинале делает InteractionBranch.
    pub fn click(&mut self, t: Target) {
        let Some(mode) = self.waiting_for() else { return };
        if !self.targets().contains(&t) {
            return;
        }
        match (mode, t) {
            (Mode::Menu, Target::Start) => {
                let cues = vec![act(|c| c.play_node("speaker_start main")), then(|c| c.start_new_game())];
                self.resolve(cues);
            }
            (Mode::Menu, Target::Quit) => self.exit = true,
            (Mode::BathroomDoor, _) => self.resolve(vec![anim(A_INTRO, "camera exit bathroom"), wait(5.0), Cue::Input(Mode::BackroomDoor)]),
            (Mode::BackroomDoor, _) => self.resolve(vec![anim(A_INTRO, "camera enter backroom")]),
            (Mode::Waiver, Target::Waiver) => {
                self.typed.clear();
                self.update_waiver_letters();
                self.resolve(vec![
                    speaker("signature machine2/speaker_signature bootup"),
                    vis(N_WAIVER_MACHINE, true),
                    anim(A_WAIVER, "pickup waiver"),
                    wait(2.77),
                    Cue::Input(Mode::Waiver),
                ]);
            }
            (Mode::Waiver, Target::Key(ch)) => {
                if self.typed.len() < 6 {
                    self.typed.push(ch);
                }
                self.update_waiver_letters();
                self.play_node("signature machine2/speaker_signature keypress");
            }
            (Mode::Waiver, Target::KeyBack) => {
                self.typed.pop();
                self.update_waiver_letters();
                self.play_node("signature machine2/speaker_signature keypress");
            }
            (Mode::Waiver, Target::KeyEnter) => {
                self.play_node("signature machine2/speaker_signature keypress");
                if self.typed.is_empty() || self.typed == "dealer" || self.typed == "god" {
                    return;
                }
                self.name = self.typed.to_uppercase();
                let n = self.typed.len();
                let mut cues = vec![wait(0.25)];
                // Signature.Input_Enter: буквы по одной "впечатываются" в бланк.
                for (i, ch) in self.name.chars().enumerate() {
                    let ch = ch.to_string();
                    cues.extend([
                        act(move |c| {
                            let g = &mut c.scenes[MAIN].graph;
                            g.set_label(&format!("{}/letter_signed_joined_{}", N_WAIVER_JOINED, i), &ch);
                            g.set_label(&format!("{}/letter_signed_separate_{}", N_WAIVER_SIGNED, i), &ch);
                        }),
                        wait(0.17),
                        speaker("signature machine2/speaker_signature punch"),
                    ]);
                }
                let _ = n;
                cues.extend([
                    wait(0.17),
                    act(|c| {
                        let g = &mut c.scenes[MAIN].graph;
                        let sg = g.node(N_SHOTGUN);
                        g.nodes[sg].local.pos = g.initial_pos(sg);
                        c.signed_waiver = true;
                    }),
                    anim(A_WAIVER, "put away waiver"),
                    speaker("signature machine2/speaker_signature shutdown"),
                    Cue::Fork(vec![wait(1.27), act(|c| c.entering_from_waiver = true), then(|c| c.main_batch_setup(false))]),
                    wait(1.72),
                    vis(N_WAIVER_MACHINE, false),
                    vis(N_WAIVER_SIGNED, false),
                    wait(0.4),
                    vis(N_WAIVER_TABLE, false),
                ]);
                self.resolve(cues);
            }
            (Mode::GrabItem, _) => {
                let cues = self.grab_item();
                self.resolve(cues);
            }
            (Mode::PlaceItem, Target::Slot(slot)) => {
                let cues = self.place_item(slot);
                self.resolve(cues);
            }
            (Mode::Turn, Target::Shotgun) => self.resolve(vec![then(|c| c.player_grab_shotgun())]),
            (Mode::Turn, Target::Item(slot)) => {
                let cues = self.player_use_item(slot);
                self.resolve(cues);
            }
            (Mode::Holding, Target::ShootDealer) => self.resolve(vec![anim(A_SHOOT_TEXT, "hide text"), then(|c| c.player_shoot(Side::Dealer))]),
            (Mode::Holding, Target::ShootSelf) => self.resolve(vec![anim(A_SHOOT_TEXT, "hide text"), then(|c| c.player_shoot(Side::Player))]),
            (Mode::BriefcaseLatches, Target::LatchL) => {
                let cues = self.open_latch(false);
                self.resolve(cues);
            }
            (Mode::BriefcaseLatches, Target::LatchR) => {
                let cues = self.open_latch(true);
                self.resolve(cues);
            }
            (Mode::BriefcaseLatches, Target::Lid) => {
                let cues = self.begin_ending();
                self.resolve(cues);
            }
            (Mode::AnyKeyToMenu, _) => self.resolve(vec![act(|c| c.black = true), wait(1.0), then(|c| c.enter_menu())]),
            (Mode::Heaven, Target::Retry) => self.resolve(vec![then(|c| {
                // Как death.tscn -> main.tscn: тот же набор, бланк уже подписан.
                c.entering_from_death = true;
                c.reset_main();
                c.intro(true)
            })]),
            (Mode::Heaven, Target::ToMenu) => self.resolve(vec![then(|c| c.enter_menu())]),
            _ => {}
        }
    }
}

fn speaker_static(path: &'static str) -> Cue {
    act(move |c| c.sounds.play(&mut c.engine, path, 0.6))
}

fn quat_deg(q: alkash3d_rs::math::Quat) -> Vec3 {
    crate::graph::quat_to_yxz(q) * (180.0 / std::f32::consts::PI)
}

impl Ctx {
    /// Esc посреди игры: в главное меню (все сценарии обрываются).
    pub fn abort_to_menu(&mut self) {
        self.queues.truncate(1);
        self.queues[0].0.clear();
        self.queues[0].1 = 0.0;
        self.hint = None;
        let cues = self.enter_menu();
        self.start(cues);
    }
}
