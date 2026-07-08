// src/input.rs
//! Система ввода.
//!
//! ИСПРАВЛЕНО (архитектурная проблема): раньше WASD-движение камеры было
//! зашито ПРЯМО в `wndproc` движка (`engine_mod.rs`, обработчик
//! `WM_KEYDOWN`), а `main.rs` ОТДЕЛЬНО опрашивал `GetAsyncKeyState` каждый
//! кадр и тоже двигал камеру. Это означало, что при удержании клавиши
//! камера двигалась ДВАЖДЫ за кадр из двух независимых источников — один
//! раз на каждое `WM_KEYDOWN` (которых Windows шлёт с auto-repeat, пока
//! клавиша зажата), и ещё раз в игровом цикле. Плюс "какая клавиша что
//! делает" была решением ДВИЖКА, а не приложения — то есть ты не мог
//! написать свою логику ввода, не залезая в engine_mod.rs.
//!
//! Теперь: движок только ЗАПИСЫВАЕТ состояние клавиш (через оконные
//! сообщения — это надёжнее по времени отклика, чем поллинг), а что с ним
//! делать — решает уже `main.rs`/игровой код через `engine.input`.

use std::collections::HashSet;

/// Состояние клавиатуры на текущий кадр.
#[derive(Default)]
pub struct InputState {
    down: HashSet<u32>,
    pressed_this_frame: HashSet<u32>,
    released_this_frame: HashSet<u32>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Вызывается движком из wndproc при WM_KEYDOWN. Не срабатывает
    /// повторно на Windows-овский auto-repeat (пока клавиша просто
    /// зажата) — `just_pressed` останется true только на первый реальный
    /// нажим.
    pub fn on_key_down(&mut self, vk: u32) {
        if self.down.insert(vk) {
            self.pressed_this_frame.insert(vk);
        }
    }

    /// Вызывается движком из wndproc при WM_KEYUP.
    pub fn on_key_up(&mut self, vk: u32) {
        if self.down.remove(&vk) {
            self.released_this_frame.insert(vk);
        }
    }

    /// Вызывается движком РОВНО один раз за кадр (в начале
    /// `process_messages()`) — очищает "мгновенные" флаги
    /// (just_pressed/just_released) с ПРЕДЫДУЩЕГО кадра, оставляя
    /// `is_down` нетронутым. Игровому коду вызывать это не нужно.
    pub fn end_frame(&mut self) {
        self.pressed_this_frame.clear();
        self.released_this_frame.clear();
    }

    /// Зажата ли клавиша прямо сейчас (можно опрашивать каждый кадр для
    /// плавного движения, зависящего от dt).
    pub fn is_down(&self, vk: u32) -> bool {
        self.down.contains(&vk)
    }

    /// Была ли клавиша нажата ИМЕННО в этом кадре (для разовых действий —
    /// прыжок, открыть меню, и т.п. — а не для непрерывного движения).
    pub fn just_pressed(&self, vk: u32) -> bool {
        self.pressed_this_frame.contains(&vk)
    }

    /// Была ли клавиша отпущена именно в этом кадре.
    pub fn just_released(&self, vk: u32) -> bool {
        self.released_this_frame.contains(&vk)
    }
}

/// Именованные виртуальные коды клавиш (Win32 VK_*), чтобы не
/// разбрасывать магические числа `0x57`/`0x41`/... по игровому коду.
pub mod keys {
    pub const W: u32 = 0x57;
    pub const A: u32 = 0x41;
    pub const S: u32 = 0x53;
    pub const D: u32 = 0x44;
    pub const Q: u32 = 0x51;
    pub const E: u32 = 0x45;
    pub const SHIFT: u32 = 0x10;
    pub const SPACE: u32 = 0x20;
    pub const ESCAPE: u32 = 0x1B;
    pub const ARROW_LEFT: u32 = 0x25;
    pub const ARROW_UP: u32 = 0x26;
    pub const ARROW_RIGHT: u32 = 0x27;
    pub const ARROW_DOWN: u32 = 0x28;
}
