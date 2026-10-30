//! Логика стола Buckshot отдельно от режиссуры порта (`flow.rs`): панель
//! индикации здоровья (HealthCounter), плашка с патронами перед раундом
//! (ShellSpawner, показ) и машинка с бланком отказа от претензий
//! (Signature). Работает прямо с графом сцены, где стол, счётчик и машинка
//! лежат по оригинальным путям (`tabletop parent/...`, `signature machine2/...`),
//! — как в `world` порта, так и в пустоте restroom (`void_scene`).
//!
//! Тайминги, анимации и звуки — из оригинальных скриптов (те же, что в
//! `flow.rs`); вместо очередей реплик — небольшие автоматы со временем.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::math::{Mat4, Vec3};

use crate::anim::Anims;
use crate::graph::{euler_yxz, Graph, Trs};
use crate::sound::Sounds;

const COUNTER_UI: &str = "tabletop parent/main tabletop/health counter/health counter ui parent";
const SPK_BOOTUP: &str = "tabletop parent/main tabletop/health counter/speaker_health counter bootup";
const SPK_BEEP: &str = "tabletop parent/main tabletop/health counter/speaker_health counter beep";
const A_FLICKER_P: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/health UI_player side/symbol_player1/animator_flicker digit player";
const A_FLICKER_D: &str = "tabletop parent/main tabletop/health counter/health counter ui parent/health UI_dealer side/symbol_dealer1/animator_flicker digit enemy";

const SHELLBOX: &str = "tabletop parent/main tabletop/compartment_shells";
const A_SHELLBOX: &str = "tabletop parent/main tabletop/compartment_shells/animator_shell compartment";
/// Места гильз в плашке (ShellSpawner.spawnPositions).
const SHELL_SPOTS: [[f32; 3]; 8] = [
    [0.049, 0.197, 2.047], [0.045, 0.197, 1.472], [0.045, 0.197, 0.888], [0.045, 0.197, 0.332],
    [0.045, 0.197, -0.291], [0.045, 0.197, -0.863], [0.045, 0.197, -1.423], [0.045, 0.197, -2.023],
];

const A_WAIVER: &str = "standalone managers/signature manager/animator_pickup waiver";
const WAIVER_TABLE: &str = "signature machine2/release form on table";
const WAIVER_MACHINE: &str = "signature machine2/signature machine main parent";
const WAIVER_SCREEN: &str = "signature machine2/signature machine main parent/baseplate main/screen parent";
const WAIVER_JOINED: &str = "signature machine2/signature machine main parent/baseplate main/release form1_003";
const WAIVER_SIGNED: &str = "signature machine2/release form signed separate";
const SPK_SIGN_BOOT: &str = "signature machine2/speaker_signature bootup";
const SPK_SIGN_KEY: &str = "signature machine2/speaker_signature keypress";
const SPK_SIGN_PUNCH: &str = "signature machine2/speaker_signature punch";
const SPK_SIGN_OFF: &str = "signature machine2/speaker_signature shutdown";
const CAMERA: &str = "Camera";
/// Бланк на столе (SetupWaiver), в координатах `signature machine2`.
const FORM_ON_TABLE: Vec3 = Vec3::new(-29.613, -35.341, -6.696);
/// Длина имени на машинке.
const NAME_LEN: usize = 6;

/// Звук из узла-динамика сцены (AudioStreamPlayer оригинала).
fn speaker(g: &Graph, snd: &mut Sounds, eng: &mut AlkashEngine, node: &str) {
    match g.find(node) {
        Some(i) if !g.nodes[i].audio_path.is_empty() => snd.play(eng, &g.nodes[i].audio_path.clone(), 0.8),
        _ => eprintln!("[TABLE] нет звукового узла '{}'", node),
    }
}

/// Перешёл ли таймер отметку `at` за этот кадр.
fn crossed(t: f32, dt: f32, at: f32) -> bool {
    t >= at && t - dt < at
}

// ---------------------------------------------------------------------------
// Панель здоровья (HealthCounter)
// ---------------------------------------------------------------------------

pub struct HealthCounter {
    /// Включение: время с начала и что показать после писка.
    boot: Option<(f32, [i32; 2])>,
}

impl HealthCounter {
    pub fn new(g: &mut Graph) -> Self {
        let mut c = HealthCounter { boot: None };
        c.power(g, false);
        c.clear(g);
        c
    }

    /// Стороны панели (игрок, дилер, разделитель) — вкл/выкл.
    pub fn power(&mut self, g: &mut Graph, on: bool) {
        for side in ["health UI_dealer side", "health UI_player side", "health UI_dividing line"] {
            g.set_visible(&format!("{}/{}", COUNTER_UI, side), on);
        }
    }

    /// Имя игрока на панели и надпись победы (HealthCounter.SetupHealth).
    pub fn set_name(&mut self, g: &mut Graph, name: &str, win_text: &str) {
        g.set_label(&format!("{}/health UI_player side/text_player", COUNTER_UI), name);
        g.set_label(&format!("{}/indicator_player win", COUNTER_UI), win_text);
    }

    /// Символы зарядов (UpdateDisplay): [игрок, дилер]; последний заряд мигает.
    pub fn show(&mut self, g: &mut Graph, anims: &mut Anims, hp: [i32; 2]) {
        for (side, who, h) in [("health UI_player side", "player", hp[0]), ("health UI_dealer side", "dealer", hp[1])] {
            g.set_visible(&format!("{}/{}/symbol_{} blank", COUNTER_UI, side, who), true);
            for i in 1..=6 {
                g.set_visible(&format!("{}/{}/symbol_{}{}", COUNTER_UI, side, who, i), i <= h);
            }
        }
        anims.play(g, A_FLICKER_P, if hp[0] == 1 { "flicker digit" } else { "RESET" });
        anims.play(g, A_FLICKER_D, if hp[1] == 1 { "flicker digit" } else { "RESET" });
    }

    pub fn clear(&mut self, g: &mut Graph) {
        for (side, who) in [("health UI_player side", "player"), ("health UI_dealer side", "dealer")] {
            g.set_visible(&format!("{}/{}/symbol_{} blank", COUNTER_UI, side, who), false);
            for i in 1..=6 {
                g.set_visible(&format!("{}/{}/symbol_{}{}", COUNTER_UI, side, who, i), false);
            }
        }
    }

    /// Включение перед первой зарядкой (bootingUpCounter): звук, пустая
    /// панель, через 0.85 с писк и заряды.
    pub fn boot(&mut self, g: &mut Graph, snd: &mut Sounds, eng: &mut AlkashEngine, hp: [i32; 2]) {
        speaker(g, snd, eng, SPK_BOOTUP);
        self.clear(g);
        self.power(g, true);
        self.boot = Some((0.0, hp));
    }

    pub fn update(&mut self, dt: f32, g: &mut Graph, anims: &mut Anims, snd: &mut Sounds, eng: &mut AlkashEngine) {
        let Some((t, hp)) = self.boot.as_mut() else { return };
        *t += dt;
        if *t >= 0.85 {
            let hp = *hp;
            self.boot = None;
            speaker(g, snd, eng, SPK_BEEP);
            self.show(g, anims, hp);
        }
    }
}

// ---------------------------------------------------------------------------
// Плашка с патронами (ShellSpawner: показ набора перед раундом)
// ---------------------------------------------------------------------------

pub struct ShellBox {
    t: f32,
    shells: Vec<bool>,
    spawned: Vec<u32>,
    text: String,
    active: bool,
    rng: u32,
}

/// Сколько плашка открыта после надписи (ShellSpawner: 2.5 с).
const SHOW_TEXT_FOR: f32 = 2.5;

impl ShellBox {
    pub fn new() -> Self {
        ShellBox { t: 0.0, shells: Vec::new(), spawned: Vec::new(), text: String::new(), active: false, rng: 0x2545_f491 }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Показать набор: `shells` — true боевой, false холостой (до 8);
    /// `text` — надпись ("2 БОЕВЫХ. 1 ХОЛОСТОЙ.").
    pub fn show(&mut self, g: &mut Graph, anims: &mut Anims, snd: &mut Sounds, eng: &mut AlkashEngine, shells: &[bool], text: String) {
        self.despawn(g);
        let parent = g.node(SHELLBOX);
        for (i, live) in shells.iter().enumerate().take(SHELL_SPOTS.len()) {
            let local = Trs {
                pos: Vec3::from(SHELL_SPOTS[i]),
                rot: euler_yxz(Vec3::new(-90.0, -90.0, 180.0) * (std::f32::consts::PI / 180.0)),
                scale: Vec3::ONE,
            };
            self.spawned.push(g.spawn("shell", parent, local, if *live { 1 } else { 2 }));
        }
        self.shells = shells.to_vec();
        self.text = text;
        self.t = 0.0;
        self.active = true;
        anims.play(g, A_SHELLBOX, "show shells");
        self.latch(snd, eng);
    }

    fn latch(&mut self, snd: &mut Sounds, eng: &mut AlkashEngine) {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        snd.play(eng, &format!("res://audio/shell latch{}.ogg", 1 + self.rng % 2), 0.8);
    }

    fn despawn(&mut self, g: &mut Graph) {
        for id in self.spawned.drain(..) {
            g.despawn(id);
        }
    }

    /// Надпись под плашкой, пока она открыта.
    pub fn caption(&self) -> Option<&str> {
        (self.active && self.t >= 1.0 && self.t < 1.0 + SHOW_TEXT_FOR).then_some(self.text.as_str())
    }

    pub fn update(&mut self, dt: f32, g: &mut Graph, anims: &mut Anims, snd: &mut Sounds, eng: &mut AlkashEngine) {
        if !self.active {
            return;
        }
        self.t += dt;
        // PlayAudioIndicators: через 0.37 с короткий сигнал на каждый патрон.
        for (k, live) in self.shells.clone().iter().enumerate() {
            if crossed(self.t, dt, 0.37 + k as f32 * 0.07) {
                snd.play(eng, if *live { "res://audio/shell indicator_live.ogg" } else { "res://audio/shell indicator_blank.ogg" }, 0.6);
            }
        }
        let hide = 1.0 + SHOW_TEXT_FOR;
        if crossed(self.t, dt, hide) {
            anims.play(g, A_SHELLBOX, "hide shells");
            self.latch(snd, eng);
        }
        // "hide shells" длится 1 с — потом гильзы из плашки убираются.
        if self.t >= hide + 1.0 {
            self.despawn(g);
            self.active = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Бланк отказа от претензий (Signature)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WaiverStage {
    /// Бланк лежит на столе.
    Idle,
    /// Игрок взял бланк, машинка поднимается (2.77 с).
    Pickup,
    /// Набор имени.
    Typing,
    /// Буквы впечатываются по одной.
    Punch,
    /// Машинка убирается, бланк подписан.
    PutAway,
    Signed,
}

pub struct Waiver {
    pub stage: WaiverStage,
    t: f32,
    typed: String,
    name: String,
    /// Подписали в этом кадре.
    just_signed: bool,
}

impl Waiver {
    /// Бланк на столе, машинка спрятана (Signature/SetupWaiver). RESET
    /// анимации не играем: он уносит бланк за кадр (на стол его в оригинале
    /// кладёт SetupWaiver), а исходное состояние узлов и так даёт `Graph::reset`.
    pub fn new(g: &mut Graph, _anims: &mut Anims) -> Self {
        let form = g.node(WAIVER_TABLE);
        g.nodes[form].local.pos = FORM_ON_TABLE;
        g.set_visible(WAIVER_MACHINE, false);
        g.set_visible(WAIVER_SIGNED, false);
        g.set_visible(WAIVER_TABLE, true);
        let w = Waiver { stage: WaiverStage::Idle, t: 0.0, typed: String::new(), name: String::new(), just_signed: false };
        w.clear_signature(g);
        w
    }

    /// Строка подписи в бланке пустая: в оригинальных Label3D стоят
    /// буквы-заглушки, иначе короткое имя дописалось бы ими.
    fn clear_signature(&self, g: &mut Graph) {
        for i in 0..NAME_LEN {
            g.set_label(&format!("{}/letter_signed_joined_{}", WAIVER_JOINED, i), "");
            g.set_label(&format!("{}/letter_signed_separate_{}", WAIVER_SIGNED, i), "");
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn signed(&self) -> bool {
        self.stage == WaiverStage::Signed
    }

    /// Бланк подписан раньше (`name`): на столе его уже нет.
    pub fn signed_before(&mut self, g: &mut Graph, name: &str) {
        self.name = name.to_string();
        g.set_visible(WAIVER_TABLE, false);
        self.stage = WaiverStage::Signed;
    }

    /// Подписан именно в этом кадре (для реакции сцены).
    pub fn take_just_signed(&mut self) -> bool {
        std::mem::take(&mut self.just_signed)
    }

    /// Где лежит бланк (для "посмотреть и нажать").
    pub fn form_pos(&self, g: &Graph) -> Vec3 {
        g.global[g.node(WAIVER_TABLE)].w_axis.truncate()
    }

    /// Камера, пока игрок занят бланком: (мировая матрица, fov в градусах).
    pub fn camera(&self, g: &Graph) -> Option<(Mat4, f32)> {
        if matches!(self.stage, WaiverStage::Idle | WaiverStage::Signed) {
            return None;
        }
        let i = g.find(CAMERA)?;
        Some((g.global[i], g.nodes[i].fov))
    }

    /// Взять бланк со стола.
    pub fn pick_up(&mut self, g: &mut Graph, anims: &mut Anims, snd: &mut Sounds, eng: &mut AlkashEngine) {
        if self.stage != WaiverStage::Idle {
            return;
        }
        self.typed.clear();
        self.letters(g);
        self.clear_signature(g);
        speaker(g, snd, eng, SPK_SIGN_BOOT);
        g.set_visible(WAIVER_MACHINE, true);
        anims.play(g, A_WAIVER, "pickup waiver");
        self.stage = WaiverStage::Pickup;
        self.t = 0.0;
    }

    /// Буквы набранного имени на экране машинки.
    fn letters(&self, g: &mut Graph) {
        let typed: Vec<char> = self.typed.to_uppercase().chars().collect();
        for i in 0..NAME_LEN {
            let t = typed.get(i).map(|c| c.to_string()).unwrap_or_default();
            g.set_label(&format!("{}/letter_{}", WAIVER_SCREEN, i), &t);
        }
        g.set_visible(&format!("{}/entry marker", WAIVER_SCREEN), typed.len() < NAME_LEN);
    }

    pub fn key(&mut self, g: &mut Graph, snd: &mut Sounds, eng: &mut AlkashEngine, ch: char) {
        if self.stage != WaiverStage::Typing {
            return;
        }
        if self.typed.chars().count() < NAME_LEN {
            self.typed.push(ch.to_ascii_lowercase());
        }
        self.letters(g);
        speaker(g, snd, eng, SPK_SIGN_KEY);
    }

    pub fn backspace(&mut self, g: &mut Graph, snd: &mut Sounds, eng: &mut AlkashEngine) {
        if self.stage != WaiverStage::Typing {
            return;
        }
        self.typed.pop();
        self.letters(g);
        speaker(g, snd, eng, SPK_SIGN_KEY);
    }

    /// Enter: пустое имя и запрещённые ("dealer", "god") не принимаются.
    pub fn enter(&mut self, g: &mut Graph, snd: &mut Sounds, eng: &mut AlkashEngine) {
        if self.stage != WaiverStage::Typing {
            return;
        }
        speaker(g, snd, eng, SPK_SIGN_KEY);
        if self.typed.is_empty() || self.typed == "dealer" || self.typed == "god" {
            return;
        }
        self.name = self.typed.to_uppercase();
        self.stage = WaiverStage::Punch;
        self.t = 0.0;
    }

    pub fn update(&mut self, dt: f32, g: &mut Graph, anims: &mut Anims, snd: &mut Sounds, eng: &mut AlkashEngine) {
        self.t += dt;
        match self.stage {
            WaiverStage::Pickup if self.t >= 2.77 => {
                self.stage = WaiverStage::Typing;
            }
            WaiverStage::Punch => {
                // Signature.Input_Enter: через 0.25 с буквы по одной каждые 0.17 с.
                let n = self.name.chars().count();
                for (i, ch) in self.name.clone().chars().enumerate() {
                    if crossed(self.t, dt, 0.25 + i as f32 * 0.17) {
                        let ch = ch.to_string();
                        g.set_label(&format!("{}/letter_signed_joined_{}", WAIVER_JOINED, i), &ch);
                        g.set_label(&format!("{}/letter_signed_separate_{}", WAIVER_SIGNED, i), &ch);
                        speaker(g, snd, eng, SPK_SIGN_PUNCH);
                    }
                }
                if self.t >= 0.25 + n as f32 * 0.17 + 0.17 {
                    anims.play(g, A_WAIVER, "put away waiver");
                    speaker(g, snd, eng, SPK_SIGN_OFF);
                    self.stage = WaiverStage::PutAway;
                    self.t = 0.0;
                }
            }
            WaiverStage::PutAway => {
                if crossed(self.t, dt, 1.72) {
                    g.set_visible(WAIVER_MACHINE, false);
                    g.set_visible(WAIVER_SIGNED, false);
                }
                if self.t >= 2.12 {
                    g.set_visible(WAIVER_TABLE, false);
                    self.stage = WaiverStage::Signed;
                    self.just_signed = true;
                }
            }
            _ => {}
        }
    }
}
