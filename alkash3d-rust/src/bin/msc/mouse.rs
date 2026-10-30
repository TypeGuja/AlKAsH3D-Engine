//! Обзор мышью: курсор прячется и каждый кадр возвращается в центр окна,
//! смещение от центра — поворот головы. Пока окно не активно (Alt+Tab) или
//! захват снят (Esc), курсор свободен.

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::{ClipCursor, GetClientRect, GetCursorPos, SetCursorPos, ShowCursor};

pub struct Mouse {
    pub captured: bool,
    hidden: bool,
    /// Пропустить следующий кадр (после захвата курсор ещё не в центре).
    skip: bool,
    /// Где курсор был в прошлом кадре — смещение считается от него.
    last: POINT,
}

impl Mouse {
    pub fn new() -> Mouse {
        Mouse { captured: true, hidden: false, skip: true, last: POINT::default() }
    }

    fn show(&mut self, visible: bool) {
        if self.hidden == !visible {
            return;
        }
        self.hidden = !visible;
        unsafe {
            ShowCursor(visible);
            if visible {
                let _ = ClipCursor(None);
            }
        }
    }

    pub fn set_captured(&mut self, c: bool) {
        self.captured = c;
        self.skip = true;
        if !c {
            self.show(true);
        }
    }

    /// Смещение мыши за кадр в пикселях (x вправо, y вниз).
    pub fn delta(&mut self) -> (f32, f32) {
        unsafe {
            let hwnd = GetActiveWindow();
            if !self.captured || hwnd.0.is_null() {
                self.show(true);
                self.skip = true;
                return (0.0, 0.0);
            }
            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect).is_err() {
                return (0.0, 0.0);
            }
            let mut tl = POINT { x: rect.left, y: rect.top };
            let mut br = POINT { x: rect.right, y: rect.bottom };
            let _ = ClientToScreen(hwnd, &mut tl);
            let _ = ClientToScreen(hwnd, &mut br);
            let center = POINT { x: (tl.x + br.x) / 2, y: (tl.y + br.y) / 2 };
            self.show(false);
            let clip = RECT { left: tl.x, top: tl.y, right: br.x, bottom: br.y };
            let _ = ClipCursor(Some(&clip));
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            if self.skip {
                self.skip = false;
                let _ = SetCursorPos(center.x, center.y);
                self.last = center;
                return (0.0, 0.0);
            }
            let d = ((p.x - self.last.x) as f32, (p.y - self.last.y) as f32);
            // в центр — только когда курсор ушёл далеко: перенос каждый кадр
            // терял движение между чтением и переносом, поворот шёл рывками
            if (p.x - center.x).abs() > (br.x - tl.x) / 4 || (p.y - center.y).abs() > (br.y - tl.y) / 4 {
                let _ = SetCursorPos(center.x, center.y);
                self.last = center;
            } else {
                self.last = p;
            }
            d
        }
    }
}

impl Drop for Mouse {
    fn drop(&mut self) {
        self.show(true);
    }
}
