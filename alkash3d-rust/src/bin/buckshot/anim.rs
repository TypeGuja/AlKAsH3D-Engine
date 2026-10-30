//! Проигрыватель Godot-анимаций (AnimationPlayer) поверх `Graph`:
//! value-дорожки (position / rotation / scale / visible / fov /
//! light_energy …), 3D-дорожки позиции/поворота/масштаба, вызовы методов
//! и звуковые дорожки. Вызовы методов и звуки не исполняются здесь, а
//! возвращаются наружу — их смысл знает только порт скриптов игры.

use std::collections::HashMap;

use crate::graph::{euler_yxz, Graph};
use crate::scene_file::{Animation, Keys, SceneFile, TrackType, Value};

pub struct PlayerRt {
    pub animations: Vec<Animation>,
    by_name: HashMap<String, usize>,
    current: Option<usize>,
    time: f32,
    /// true — следующий update должен сработать и на ключах с t = 0.
    fresh: bool,
    pub speed: f32,
}

pub enum Fired {
    Method { node: usize, name: String },
    Sound(String),
}

pub struct Anims {
    pub players: Vec<PlayerRt>,
    by_path: HashMap<String, usize>,
}

/// Godot `ease(x, curve)`.
pub fn ease(x: f32, c: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if c > 0.0 {
        if c < 1.0 { 1.0 - (1.0 - x).powf(1.0 / c) } else { x.powf(c) }
    } else if c < 0.0 {
        if x < 0.5 {
            (x * 2.0).powf(-c) * 0.5
        } else {
            (1.0 - (1.0 - (x - 0.5) * 2.0).powf(-c)) * 0.5 + 0.5
        }
    } else {
        0.0
    }
}

fn lerp_value(a: &Value, b: &Value, t: f32) -> Value {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => Value::Float(x + (y - x) * t),
        (Value::Vec3(x), Value::Vec3(y)) => Value::Vec3(x.lerp(*y, t)),
        (Value::Quat(x), Value::Quat(y)) => Value::Quat(x.slerp(*y, t)),
        (Value::Color(x), Value::Color(y)) => Value::Color([0, 1, 2, 3].map(|i| x[i] + (y[i] - x[i]) * t)),
        _ => if t < 1.0 { a.clone() } else { b.clone() },
    }
}

impl Anims {
    pub fn new(scene: &SceneFile, graph: &Graph) -> Self {
        let mut players = Vec::new();
        let mut by_path = HashMap::new();
        for p in &scene.players {
            let by_name = p.animations.iter().enumerate().map(|(i, a)| (a.name.clone(), i)).collect();
            let path = scene.path(p.node);
            let rel = path.split_once('/').map(|(_, r)| r.to_string()).unwrap_or_default();
            by_path.insert(rel, players.len());
            players.push(PlayerRt { animations: p.animations.clone(), by_name, current: None, time: 0.0, fresh: false, speed: p.speed });
        }
        let _ = graph;
        Anims { players, by_path }
    }

    pub fn player(&self, path: &str) -> Option<usize> {
        self.by_path.get(path).copied()
    }

    pub fn length(&self, player: &str, anim: &str) -> f32 {
        self.player(player)
            .and_then(|p| self.players[p].by_name.get(anim).map(|&a| self.players[p].animations[a].length))
            .unwrap_or(0.0)
    }

    /// AnimationPlayer.play(name). "RESET" тоже обычная анимация (кадр 0).
    pub fn play(&mut self, graph: &mut Graph, player: &str, anim: &str) {
        let Some(p) = self.player(player) else {
            eprintln!("[ANIM] нет AnimationPlayer '{}'", player);
            return;
        };
        let Some(&a) = self.players[p].by_name.get(anim) else {
            eprintln!("[ANIM] у '{}' нет анимации '{}'", player, anim);
            return;
        };
        let pl = &mut self.players[p];
        pl.current = Some(a);
        pl.time = 0.0;
        pl.fresh = true;
        apply(graph, &pl.animations[a], 0.0);
    }

    /// Играет ли сейчас что-нибудь у этого AnimationPlayer.
    pub fn is_playing(&self, player: &str) -> bool {
        self.player(player).is_some_and(|p| self.players[p].current.is_some())
    }

    pub fn stop_all(&mut self) {
        for p in &mut self.players {
            p.current = None;
        }
    }

    pub fn stop(&mut self, player: &str) {
        if let Some(p) = self.player(player) {
            self.players[p].current = None;
        }
    }

    pub fn update(&mut self, graph: &mut Graph, dt: f32) -> Vec<Fired> {
        let mut fired = Vec::new();
        for pl in &mut self.players {
            let Some(a) = pl.current else { continue };
            let anim = &pl.animations[a];
            let prev = pl.time;
            let mut now = prev + dt * pl.speed;
            let mut finished = false;
            if now >= anim.length {
                if anim.looped && anim.length > 0.0 {
                    collect_events(anim, prev, anim.length, pl.fresh, &mut fired);
                    pl.fresh = true;
                    now = now.rem_euclid(anim.length);
                    collect_events(anim, 0.0, now, true, &mut fired);
                    pl.fresh = false;
                    pl.time = now;
                    apply(graph, anim, now);
                    continue;
                }
                now = anim.length;
                finished = true;
            }
            collect_events(anim, prev, now, pl.fresh, &mut fired);
            pl.fresh = false;
            pl.time = now;
            apply(graph, anim, now);
            if finished {
                pl.current = None;
            }
        }
        fired
    }
}

fn collect_events(anim: &Animation, from: f32, to: f32, inclusive_start: bool, out: &mut Vec<Fired>) {
    let hit = |t: f32| (t > from || (inclusive_start && t >= from)) && t <= to;
    for track in &anim.tracks {
        match &track.keys {
            Keys::Method(keys) => {
                for (t, name, _) in keys {
                    if hit(*t) {
                        if let Some(node) = track.target {
                            out.push(Fired::Method { node, name: name.clone() });
                        }
                    }
                }
            }
            Keys::Audio(keys) => {
                for (t, path) in keys {
                    if hit(*t) && !path.is_empty() {
                        out.push(Fired::Sound(path.clone()));
                    }
                }
            }
            Keys::Value(_) => {}
        }
    }
}

fn sample(keys: &[(f32, f32, Value)], t: f32, interp: u32, discrete: bool) -> Option<Value> {
    if keys.is_empty() {
        return None;
    }
    let idx = keys.iter().rposition(|k| k.0 <= t);
    let Some(i) = idx else { return Some(keys[0].2.clone()) };
    if i + 1 >= keys.len() || discrete || interp == 0 {
        return Some(keys[i].2.clone());
    }
    let (t0, e, ref a) = keys[i];
    let (t1, _, ref b) = keys[i + 1];
    let mut k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
    if (e - 1.0).abs() > 1e-4 {
        k = ease(k, e);
    }
    Some(lerp_value(a, b, k))
}

fn apply(graph: &mut Graph, anim: &Animation, t: f32) {
    for track in &anim.tracks {
        let (Some(target), Keys::Value(keys)) = (track.target, &track.keys) else { continue };
        let discrete = track.property == "visible";
        let Some(v) = sample(keys, t, track.interp, discrete) else { continue };
        let node = &mut graph.nodes[target];
        // Дорожка кости: "Skeleton3D:имя_кости" (3D-дорожки позиции/поворота/масштаба).
        if matches!(track.kind, TrackType::Position | TrackType::Rotation | TrackType::Scale) && !track.property.is_empty() {
            if let Some(b) = node.bones.iter_mut().find(|b| b.name == track.property) {
                match (track.kind, v) {
                    (TrackType::Position, Value::Vec3(p)) => b.pose.pos = p,
                    (TrackType::Rotation, Value::Quat(q)) => b.pose.rot = q,
                    (TrackType::Scale, Value::Vec3(s)) => b.pose.scale = s,
                    _ => {}
                }
            }
            continue;
        }
        match (track.kind, track.property.as_str(), v) {
            (TrackType::Position, _, Value::Vec3(p)) | (TrackType::Value, "position", Value::Vec3(p)) => node.local.pos = p,
            (TrackType::Rotation, _, Value::Quat(q)) | (TrackType::Value, "quaternion", Value::Quat(q)) => node.local.rot = q,
            (TrackType::Scale, _, Value::Vec3(s)) | (TrackType::Value, "scale", Value::Vec3(s)) => node.local.scale = s,
            (TrackType::Value, "rotation", Value::Vec3(r)) => {
                node.euler = r;
                node.local.rot = euler_yxz(r);
            }
            (TrackType::Value, "rotation_degrees", Value::Vec3(r)) => {
                let r = r * (std::f32::consts::PI / 180.0);
                node.euler = r;
                node.local.rot = euler_yxz(r);
            }
            (TrackType::Value, "transform", Value::Xf(m)) => {
                let (s, r, p) = m.to_scale_rotation_translation();
                node.local.pos = p;
                node.local.rot = r;
                node.local.scale = s;
            }
            (TrackType::Value, "visible", Value::Float(f)) => node.visible = f > 0.5,
            (TrackType::Value, "fov", Value::Float(f)) => node.fov = f,
            (TrackType::Value, "light_energy", Value::Float(f)) => node.light_energy = f,
            (TrackType::Value, "omni_range", Value::Float(f)) | (TrackType::Value, "spot_range", Value::Float(f)) => node.light_range = Some(f),
            // GeometryInstance3D.transparency: 0 — непрозрачно, 1 — невидимо.
            (TrackType::Value, "transparency", Value::Float(f)) => node.tint[3] = (1.0 - f).clamp(0.0, 1.0),
            // modulate у Label3D/Sprite3D — множитель цвета и альфа.
            (TrackType::Value, "modulate", Value::Color(c)) => node.tint = c,
            (TrackType::Value, "material_override:albedo_color", Value::Color(c))
            | (TrackType::Value, "mesh:surface_0/material:albedo_color", Value::Color(c)) => {
                let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
                let b = node.base_albedo;
                let ratio = |v: f32, base: f32| if base > 1e-4 { v / base } else { v };
                node.tint = [ratio(lin(c[0]), b[0]), ratio(lin(c[1]), b[1]), ratio(lin(c[2]), b[2]), c[3]];
            }
            _ => {}
        }
    }
}
