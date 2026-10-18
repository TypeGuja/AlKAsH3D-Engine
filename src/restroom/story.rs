//! Сценарий: дверь туалета выбивается; дверь в бэкрум (комнату дилера)
//! заперта — игрок дёргает её. Курильщик на мостике просто стоит и курит;
//! после попытки открыть дверь он заговаривает (субтитры). На перилах
//! мостика — "[E] Сброситься": игрок забирается на перила спиной к
//! пропасти и валится назад, падает лицом вверх; при ударе — мгновенно чёрный экран и звук удара, через 10 с — пустота.
//!
//! Реплики курильщика — субтитры из текстового файла рядом со сценой
//! (`restroom_lines.txt`, перечитывается при сбросе на R): строка
//! `talk начало конец текст`, время в секундах от начала разговора (он
//! начинается, когда игрок отпускает запертую дверь). Нет файла — реплики
//! по умолчанию. Печатаются по буквам со щелчками, как у дилера.

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::math::Vec3;

use crate::anim::Anims;
use crate::collide::Collider;
use crate::graph::Graph;
use crate::sound::Sounds;
use crate::{Door, Player, EYE};

/// Перила мостика: линия z, высота верхней трубы, мостик по x.
const RAIL_Z: f32 = 11.3;
const RAIL_TOP: f32 = 1.9;
const BRIDGE_X: (f32, f32) = (-134.0, -38.0);
/// Насколько близко к перилам надо подойти, чтобы сброситься.
const RAIL_REACH: f32 = 8.0;
/// Перелезть через перила (с).
const CLIMB_TIME: f32 = 1.1;
/// Длительность падения до удара (с).
const FALL_TIME: f32 = 1.5;
/// Темнота после удара перед пустотой (с).
const VOID_DELAY: f32 = 10.0;
/// Печать реплики по буквам — как у дилера (DialogueManager: incrementDelay
/// 0.02, на букву щелчок "dialogue click4" с pitch 0.5–0.6).
const LETTER_DELAY: f32 = 0.02;
const CLICK: &str = "res://audio/dialogue click4.ogg";
const DEFAULT_LINES: &str = "talk 0.4 3.2 Ты пришёл снова играть?
talk 3.6 5.8 Сегодня тебе не туда.
talk 6.2 8.0 Вниз.
";

struct Line {
    from: f32,
    to: f32,
    text: String,
}

fn parse_lines(src: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for l in src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut it = l.splitn(4, char::is_whitespace);
        if it.next() != Some("talk") {
            eprintln!("[RESTROOM] реплика не разобрана (нужно `talk начало конец текст`): {}", l);
            continue;
        }
        let (Some(from), Some(to), Some(text)) = (it.next().and_then(|v| v.parse().ok()), it.next().and_then(|v| v.parse().ok()), it.next()) else {
            eprintln!("[RESTROOM] реплика не разобрана: {}", l);
            continue;
        };
        out.push(Line { from, to, text: text.trim().to_string() });
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stage {
    Free,
    /// Дёргает запертую дверь.
    Rattle,
    /// Перелезает через перила.
    Climb,
    Fall,
    End,
    /// Пустота: стол вдали, лампы на штативах (`void.rs`).
    Void,
}

pub struct Story {
    stage: Stage,
    t: f32,
    /// Курильщик стоит (анимация "idle smoking" перезапускается).
    pub dude_idle: bool,
    leg: usize,
    /// Время с начала разговора курильщика (None — ещё не заговорил).
    talk: Option<f32>,
    from_pos: Vec3,
    from_yaw: f32,
    from_pitch: f32,
    vel: Vec3,
    impact: bool,
    lines: Vec<Line>,
    /// Текущая реплика и сколько её букв уже напечатано (для щелчков).
    line_shown: Option<(usize, usize)>,
    rng: u32,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn wrap(a: f32) -> f32 {
    let mut a = a;
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

impl Story {
    pub fn new(_g: &Graph, lines_path: &std::path::Path) -> Story {
        let lines = parse_lines(&std::fs::read_to_string(lines_path).unwrap_or_else(|_| DEFAULT_LINES.to_string()));
        Story {
            stage: Stage::Free,
            t: 0.0,
            dude_idle: true,
            leg: 0,
            talk: None,
            from_pos: Vec3::ZERO,
            from_yaw: 0.0,
            from_pitch: 0.0,
            vel: Vec3::ZERO,
            impact: false,
            lines,
            line_shown: None,
            rng: 0x9e37_79b9,
        }
    }

    pub fn stage_name(&self) -> Stage {
        self.stage
    }

    pub fn ended(&self) -> bool {
        self.stage == Stage::End
    }

    pub fn black(&self) -> bool {
        self.stage == Stage::End
    }

    /// Отладка (`RESTROOM_START=void`): сразу в пустоту.
    pub fn enter_void(&mut self) {
        self.stage = Stage::Void;
        self.t = 0.0;
    }

    pub fn in_void(&self) -> bool {
        self.stage == Stage::Void
    }

    pub fn brightness(&self) -> f32 {
        // После удара — полная темнота (синий свет снизу иначе подкрашивает чёрный экран).
        if self.stage == Stage::End { 0.0 } else { 1.0 }
    }

    pub fn can_use(&self, door: usize) -> bool {
        self.stage == Stage::Free && (door == 1 || door == 0)
    }

    pub fn show_crosshair(&self, p: &Player) -> bool {
        !p.scripted && matches!(self.stage, Stage::Free | Stage::Void)
    }

    /// Можно сброситься: курильщик уже заговорил, игрок на мостике у перил
    /// и смотрит за них.
    pub fn can_jump(&self, p: &Player, dir: Vec3) -> bool {
        self.stage == Stage::Free
            && self.talk.is_some()
            && (BRIDGE_X.0..BRIDGE_X.1).contains(&p.pos.x)
            && RAIL_Z - p.pos.z < RAIL_REACH
            && dir.z > 0.35
    }

    pub fn caption(&self) -> Option<String> {
        match self.stage {
            Stage::Rattle if self.t > 0.5 => Some("Заперто.".into()),
            Stage::Free => self.active_line().map(|(i, n)| self.lines[i].text.chars().take(n).collect()),
            _ => None,
        }
    }

    /// Реплика на экране сейчас и сколько её букв уже видно.
    fn active_line(&self) -> Option<(usize, usize)> {
        let t = self.talk?;
        let i = self.lines.iter().position(|l| (l.from..l.to).contains(&t))?;
        let n = ((t - self.lines[i].from) / LETTER_DELAY) as usize + 1;
        Some((i, n.min(self.lines[i].text.chars().count())))
    }

    /// Щелчок на каждую новую букву реплики.
    fn tick_line(&mut self, snd: &mut Sounds, eng: &mut AlkashEngine) {
        let now = if self.stage == Stage::Free { self.active_line() } else { None };
        let from = match (self.line_shown, now) {
            (Some((a, k)), Some((b, _))) if a == b => k,
            _ => 0,
        };
        if let Some((i, n)) = now {
            for ch in self.lines[i].text.chars().skip(from).take(n.saturating_sub(from)) {
                if ch.is_whitespace() {
                    continue;
                }
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 17;
                self.rng ^= self.rng << 5;
                let pitch = 0.5 + 0.1 * (self.rng >> 8) as f32 / (1u32 << 24) as f32;
                snd.play_pitched(eng, CLICK, 1.0, pitch);
            }
        }
        self.line_shown = now;
    }

    pub fn auto_leg(&self) -> usize {
        self.leg
    }

    pub fn auto_use(&self, p: &Player, look: Option<usize>) -> Option<usize> {
        if self.stage != Stage::Free || self.talk.is_some() {
            return None;
        }
        match (self.leg, look) {
            (0, Some(0)) => Some(0),
            (2, Some(1)) if p.pos.x > -50.0 => Some(1),
            _ => None,
        }
    }

    /// Автопрогон: дослушать курильщика и сброситься.
    pub fn auto_jump(&self) -> bool {
        self.stage == Stage::Free && self.talk.is_some_and(|t| t > self.lines.iter().map(|l| l.to).fold(0.0, f32::max) + 0.5)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f32,
        use_door: Option<usize>,
        jump: bool,
        p: &mut Player,
        doors: &mut [Door; 2],
        _g: &mut Graph,
        _anims: &mut Anims,
        snd: &mut Sounds,
        eng: &mut AlkashEngine,
        _col: &Collider,
    ) {
        self.t += dt;
        if let Some(t) = self.talk.as_mut() {
            *t += dt;
        }
        // Маршрут автопрогона.
        self.leg = if !doors[0].open { 0 } else if p.pos.x < -122.0 { 1 } else { 2 };
        match self.stage {
            Stage::Free => {
                match use_door {
                    Some(0) if !doors[0].open => {
                        doors[0].open = true;
                        doors[0].target = 1.65;
                        snd.play(eng, "res://audio/kick door enter lobby.ogg", 0.9);
                    }
                    Some(1) => {
                        self.stage = Stage::Rattle;
                        self.t = 0.0;
                        p.move_locked = true;
                        doors[1].speed = 30.0;
                        snd.play(eng, "res://audio/player check handcuffs.ogg", 0.9);
                    }
                    _ => {}
                }
                if jump && self.talk.is_some() {
                    self.stage = Stage::Climb;
                    self.t = 0.0;
                    p.scripted = true;
                    self.from_pos = p.pos;
                    self.from_yaw = p.yaw;
                    self.from_pitch = p.pitch;
                }
            }
            Stage::Rattle => {
                // Ручку дёргают дважды — дверь чуть ходит на петлях.
                let k = (1.0 - self.t / 1.5).max(0.0);
                doors[1].target = 0.025 * (self.t * 38.0).sin() * k;
                if self.t > 0.7 && self.t - dt <= 0.7 {
                    snd.play(eng, "res://audio/dealer check handcuffs.ogg", 0.7);
                }
                if self.t > 1.6 {
                    doors[1].target = 0.0;
                    p.move_locked = false;
                    self.stage = Stage::Free;
                    self.t = 0.0;
                    // Курильщик заговаривает (повторная попытка — разговор не сначала).
                    self.talk.get_or_insert(0.0);
                }
            }
            Stage::Climb => {
                // Забраться на перила, развернувшись спиной к пропасти, и
                // завалиться назад через верхнюю трубу — взгляд вверх.
                let k = smooth(self.t / CLIMB_TIME);
                let rail = Vec3::new(self.from_pos.x, self.from_pos.y, RAIL_Z - 1.5);
                let over = Vec3::new(self.from_pos.x, RAIL_TOP - EYE * 0.55, RAIL_Z + 3.0);
                p.pos = if k < 0.5 {
                    self.from_pos.lerp(rail, k * 2.0) + Vec3::Y * ((k * 2.0) * 4.0)
                } else {
                    (rail + Vec3::Y * 4.0).lerp(over, (k - 0.5) * 2.0)
                };
                // Лицом к мостику (-Z), спиной к пропасти; взгляд уходит вверх.
                let want_yaw = 0.0;
                p.yaw = self.from_yaw + wrap(want_yaw - self.from_yaw) * smooth(self.t / (CLIMB_TIME * 0.6));
                p.pitch = self.from_pitch + (0.7 - self.from_pitch) * k;
                p.roll = 0.05 * (self.t * 9.0).sin() * (1.0 - k);
                if self.t > CLIMB_TIME {
                    self.stage = Stage::Fall;
                    self.t = 0.0;
                    self.vel = Vec3::new(0.0, 1.0, 5.0);
                    self.from_pitch = p.pitch;
                    p.roll = 0.0;
                }
            }
            Stage::Fall => {
                // На спине, лицом вверх: мостик уходит вверх и пропадает.
                self.vel.y -= 98.0 * dt;
                p.pos += self.vel * dt;
                p.pitch = (self.from_pitch + self.t * 0.5).min(1.5);
                p.roll += dt * 0.15;
                // Удар о землю: изображение пропадает сразу, остаётся только звук.
                if self.t > FALL_TIME && !self.impact {
                    self.impact = true;
                    snd.stop_all_loops(eng);
                    snd.stop_music(eng);
                    snd.play(eng, "res://audio/dealer fly away crash.ogg", 1.0);
                    self.stage = Stage::End;
                    self.t = 0.0;
                }
            }
            Stage::End => {
                if self.t > VOID_DELAY {
                    self.stage = Stage::Void;
                    self.t = 0.0;
                }
            }
            Stage::Void => {}
        }
        self.tick_line(snd, eng);
    }
}
