//! Звук по путям ресурсов Godot ("res://audio/x.ogg"): движок играет
//! только PCM WAV, поэтому вся папка audio/ заранее сконвертирована в
//! `<данные>/res/audio/x.ogg.wav` (тот же относительный путь + ".wav").
//! Клип грузится при первом проигрывании.
//!
//! Музыка — два синхронно запущенных цикла одного трека: чистый и
//! приглушённый (в оригинале на шину музыки повешен AudioEffectLowPassFilter,
//! который FilterController плавно двигает, когда игрок выбивает двери;
//! у нас фильтр заранее посчитан в `.lp.wav`, а "движение" фильтра —
//! кроссфейд между версиями).

use std::collections::{HashMap, HashSet};

use alkash3d_rs::engine::AlkashEngine;
use alkash3d_rs::SoundHandle;

pub struct Music {
    clear: Option<SoundHandle>,
    muffled: Option<SoundHandle>,
    /// 0 — приглушённая, 1 — чистая.
    mix: f32,
    volume: f32,
    target_volume: f32,
    /// Трек без приглушённой версии (меню, рай, концовка) — фильтр к нему не применяется.
    plain: bool,
}

pub struct Sounds {
    dir: String,
    loaded: HashSet<String>,
    failed: HashSet<String>,
    loops: HashMap<String, SoundHandle>,
    music: Option<Music>,
}

impl Sounds {
    pub fn new(data_dir: &str) -> Self {
        Self { dir: data_dir.to_string(), loaded: HashSet::new(), failed: HashSet::new(), loops: HashMap::new(), music: None }
    }

    fn ensure(&mut self, engine: &mut AlkashEngine, res_path: &str, suffix: &str) -> Option<String> {
        let name = format!("{}{}", res_path, suffix);
        if self.loaded.contains(&name) {
            return Some(name);
        }
        if self.failed.contains(&name) {
            return None;
        }
        let audio = engine.audio.as_mut()?;
        let rel = res_path.trim_start_matches("res://");
        let file = format!("{}/res/{}{}.wav", self.dir, rel, suffix);
        match audio.load_clip(&name, &file) {
            Ok(()) => {
                self.loaded.insert(name.clone());
                Some(name)
            }
            Err(e) => {
                eprintln!("[BUCKSHOT] звук {}: {}", file, e);
                self.failed.insert(name);
                None
            }
        }
    }

    pub fn play(&mut self, engine: &mut AlkashEngine, res_path: &str, volume: f32) {
        self.play_pitched(engine, res_path, volume, 1.0);
    }

    /// Как AudioStreamPlayer с pitch_scale: высота тона (XAudio2 frequency ratio).
    pub fn play_pitched(&mut self, engine: &mut AlkashEngine, res_path: &str, volume: f32, pitch: f32) {
        if let Some(name) = self.ensure(engine, res_path, "") {
            if let Some(audio) = engine.audio.as_mut() {
                if let Ok(h) = audio.play_sound_2d(&name, volume, false) {
                    if (pitch - 1.0).abs() > 1e-3 {
                        audio.set_sound_pitch(h, pitch);
                    }
                }
            }
        }
    }

    /// Зацикленный звук под именем `key` (повторный вызов не дублирует).
    pub fn play_loop(&mut self, engine: &mut AlkashEngine, key: &str, res_path: &str, volume: f32) {
        if self.loops.contains_key(key) {
            return;
        }
        if let Some(name) = self.ensure(engine, res_path, "") {
            if let Some(h) = engine.audio.as_mut().and_then(|a| a.play_sound_2d(&name, volume, true).ok()) {
                self.loops.insert(key.to_string(), h);
            }
        }
    }

    pub fn stop_loop(&mut self, engine: &mut AlkashEngine, key: &str) {
        if let (Some(h), Some(audio)) = (self.loops.remove(key), engine.audio.as_mut()) {
            audio.stop_sound(h);
        }
    }

    pub fn stop_all_loops(&mut self, engine: &mut AlkashEngine) {
        let keys: Vec<String> = self.loops.keys().cloned().collect();
        for k in keys {
            self.stop_loop(engine, &k);
        }
    }

    /// MusicManager.LoadTrack: оба варианта трека с нуля.
    pub fn start_music(&mut self, engine: &mut AlkashEngine, res_path: &str, muffled: bool, fade_in: bool) {
        self.stop_music(engine);
        let clear = self.ensure(engine, res_path, "");
        let lp = self.ensure(engine, res_path, ".lp");
        let Some(audio) = engine.audio.as_mut() else { return };
        let mix = if muffled { 0.0 } else { 1.0 };
        let volume = if fade_in { 0.0 } else { 1.0 };
        let clear = clear.and_then(|n| audio.play_sound_2d(&n, 0.0, true).ok());
        let muffled_h = lp.and_then(|n| audio.play_sound_2d(&n, 0.0, true).ok());
        self.music = Some(Music { clear, muffled: muffled_h, mix, volume, target_volume: 1.0, plain: false });
    }

    /// Обычный (не музыкальный) трек без приглушённой версии.
    pub fn start_plain_music(&mut self, engine: &mut AlkashEngine, res_path: &str) {
        self.stop_music(engine);
        let clear = self.ensure(engine, res_path, "");
        let Some(audio) = engine.audio.as_mut() else { return };
        let clear = clear.and_then(|n| audio.play_sound_2d(&n, 0.0, true).ok());
        self.music = Some(Music { clear, muffled: None, mix: 1.0, volume: 1.0, target_volume: 1.0, plain: true });
    }

    pub fn stop_music(&mut self, engine: &mut AlkashEngine) {
        if let (Some(m), Some(audio)) = (self.music.take(), engine.audio.as_mut()) {
            for h in [m.clear, m.muffled].into_iter().flatten() {
                audio.stop_sound(h);
            }
        }
    }

    pub fn fade_music(&mut self, to: f32) {
        if let Some(m) = &mut self.music {
            m.target_volume = to;
        }
    }

    pub fn update_music(&mut self, engine: &mut AlkashEngine, dt: f32, muffled: bool) {
        let Some(m) = &mut self.music else { return };
        let target_mix = if muffled && !m.plain { 0.0 } else { 1.0 };
        // FilterController двигает фильтр примерно за секунду.
        m.mix += (target_mix - m.mix).clamp(-dt, dt);
        m.volume += (m.target_volume - m.volume).clamp(-dt * 0.5, dt * 0.5);
        let Some(audio) = engine.audio.as_mut() else { return };
        const MUSIC: f32 = 0.55;
        if let Some(h) = m.clear {
            audio.set_sound_volume(h, MUSIC * m.volume * m.mix);
        }
        if let Some(h) = m.muffled {
            audio.set_sound_volume(h, MUSIC * m.volume * (1.0 - m.mix) * 1.6);
        }
    }
}
