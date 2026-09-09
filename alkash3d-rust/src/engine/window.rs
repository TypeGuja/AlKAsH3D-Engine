//! Окно Win32 и обработка сообщений: создание окна/класса окна, WNDPROC
//! (клавиатура, resize, закрытие), переключение borderless-fullscreen (F11),
//! корректный resize (дождаться GPU idle -> пересоздать Renderer -> ResizeBuffers),
//! выкачка очереди сообщений (`process_messages`).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись.

use std::sync::atomic::Ordering;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_FLAG;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Gdi::{
    UpdateWindow, COLOR_WINDOW, HBRUSH,
    MonitorFromWindow, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::STATE;
use crate::render::Renderer;
use super::{AlkashEngine, NEXT_FENCE_VALUE, wait_for_fence};

impl AlkashEngine {
    pub(super) fn create_window(&mut self) -> Result<()> {
        unsafe {
            let hinstance = GetModuleHandleA(None)?;
            let window_class = "ALKASH3D_WINDOW\0".as_ptr();

            let wc = WNDCLASSA {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(Self::wndproc_static),
                hInstance: hinstance.into(),
                lpszClassName: PCSTR(window_class),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as _),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };

            RegisterClassA(&wc);

            let hwnd = CreateWindowExA(
                WINDOW_EX_STYLE::default(),
                PCSTR(window_class),
                PCSTR(b"Alkash3D Engine - DirectX 12\0".as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                self.width as i32,
                self.height as i32,
                None,
                None,
                Some(HINSTANCE::from(hinstance)),
                Some(self as *mut Self as _),
            )?;

            self.hwnd = Some(hwnd);
            println!("[ENGINE] Window created: HWND=0x{:X}", hwnd.0 as usize);
        }

        Ok(())
    }

    extern "system" fn wndproc_static(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let cs = lparam.0 as *const CREATESTRUCTA;
                let engine = (*cs).lpCreateParams as *mut AlkashEngine;
                SetWindowLongPtrA(hwnd, GWLP_USERDATA, engine as isize);
            }

            let engine = GetWindowLongPtrA(hwnd, GWLP_USERDATA) as *mut AlkashEngine;
            if !engine.is_null() {
                let engine_ref = &mut *engine;
                return engine_ref.wndproc(hwnd, msg, wparam, lparam);
            }

            DefWindowProcA(hwnd, msg, wparam, lparam)
        }
    }

    fn wndproc(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_CLOSE => {
                    println!("[ENGINE] WM_CLOSE received - stopping engine loop");
                    self.running = false;
                    ShowWindow(hwnd, SW_HIDE);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    println!("[ENGINE] WM_DESTROY received - window being destroyed");
                    self.running = false;
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                WM_KEYDOWN => {
                    self.input.on_key_down(wparam.0 as u32);
                    LRESULT(0)
                }
                WM_KEYUP => {
                    self.input.on_key_up(wparam.0 as u32);
                    LRESULT(0)
                }
                WM_SYSKEYDOWN => {
                    let vk = wparam.0 as u32;
                    self.input.on_key_down(vk);
                    if vk == crate::input::keys::F11 {
                        self.toggle_fullscreen();
                        return LRESULT(0);
                    }
                    DefWindowProcA(hwnd, msg, wparam, lparam)
                }
                WM_SYSKEYUP => {
                    let vk = wparam.0 as u32;
                    self.input.on_key_up(vk);
                    if vk == crate::input::keys::F11 {
                        return LRESULT(0);
                    }
                    DefWindowProcA(hwnd, msg, wparam, lparam)
                }
                WM_SIZE => {
                    let width = (lparam.0 & 0xFFFF) as u32;
                    let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                    if width > 0 && height > 0 && (width != self.width || height != self.height) {
                        if self.resizing_live {
                            self.pending_resize = Some((width, height));
                        } else {
                            self.width = width;
                            self.height = height;
                            self.camera.set_aspect(width, height);
                            self.handle_resize(width, height);
                        }
                    }
                    LRESULT(0)
                }
                WM_ENTERSIZEMOVE => {
                    self.resizing_live = true;
                    self.pending_resize = None;
                    LRESULT(0)
                }
                WM_EXITSIZEMOVE => {
                    self.resizing_live = false;
                    if let Some((width, height)) = self.pending_resize.take() {
                        if width > 0 && height > 0 && (width != self.width || height != self.height) {
                            self.width = width;
                            self.height = height;
                            self.camera.set_aspect(width, height);
                            self.handle_resize(width, height);
                        }
                    }
                    LRESULT(0)
                }
                _ => DefWindowProcA(hwnd, msg, wparam, lparam),
            }
        }
    }

    /// ДОБАВЛЕНО (F11 — переключение полноэкранного режима): классический
    /// Win32-паттерн "borderless fullscreen" — НЕ эксклюзивный DXGI-
    /// fullscreen (тот требует отдельного `IDXGISwapChain::SetFullscreenState`
    /// и пересоздания swap chain под EXCLUSIVE-режим, конфликтует с Alt+Tab
    /// и оверлеями), а обычное окно без рамки/заголовка, растянутое на весь
    /// монитор. Перед входом сохраняем текущие позицию/размер (`GetWindowRect`)
    /// и стиль (`GetWindowLongPtrA(GWL_STYLE)`) окна, чтобы точно вернуть их
    /// при выходе — а не гадать разрешение константами.
    ///
    /// Границы монитора берём через `MonitorFromWindow` + `GetMonitorInfoW`
    /// (монитор, на котором СЕЙЧАС физически находится окно), а не хардкодим
    /// разрешение — иначе на второй видеокарте/мониторе с другим
    /// разрешением или DPI окно растянулось бы неверно (чёрные полосы или
    /// уехало бы за пределы экрана).
    ///
    /// Отдельно вызывать `handle_resize()` отсюда НЕ нужно: `SetWindowPos`
    /// ниже сам пришлёт окну WM_SIZE с новым размером, а обработчик WM_SIZE
    /// (см. выше) сам вызовет `handle_resize()`, т.к. `self.resizing_live`
    /// в этот момент `false` — это не перетаскивание рамки мышью, а
    /// программная смена размера, для которой WM_SIZE применяет ресайз
    /// немедленно, одним вызовом.
    fn toggle_fullscreen(&mut self) {
        let Some(hwnd) = self.hwnd else {
            return;
        };

        unsafe {
            if !self.is_fullscreen {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_err() {
                    println!("[ENGINE] toggle_fullscreen: GetWindowRect не удался, отмена");
                    return;
                }
                let style = WINDOW_STYLE(GetWindowLongPtrA(hwnd, GWL_STYLE) as u32);

                self.saved_window_rect = Some(rect);
                self.saved_window_style = Some(style);

                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
                    println!("[ENGINE] toggle_fullscreen: GetMonitorInfoW не удался, отмена");
                    return;
                }
                let mr = mi.rcMonitor;

                let new_style = WINDOW_STYLE((style.0 & !WS_OVERLAPPEDWINDOW.0) | WS_POPUP.0);
                SetWindowLongPtrA(hwnd, GWL_STYLE, new_style.0 as isize);

                let _ = SetWindowPos(
                    hwnd,
                    None,
                    mr.left,
                    mr.top,
                    mr.right - mr.left,
                    mr.bottom - mr.top,
                    SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
                );

                self.is_fullscreen = true;
                println!(
                    "[ENGINE] Fullscreen: ON ({}x{})",
                    mr.right - mr.left,
                    mr.bottom - mr.top
                );
            } else {
                if let Some(style) = self.saved_window_style.take() {
                    SetWindowLongPtrA(hwnd, GWL_STYLE, style.0 as isize);
                }

                if let Some(rect) = self.saved_window_rect.take() {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }

                self.is_fullscreen = false;
                println!("[ENGINE] Fullscreen: OFF");
            }
        }
    }

    /// Корректная обработка ресайза окна: дожидается GPU idle, освобождает
    /// Renderer (владеющий back buffer'ами/RTV/DSV), ресайзит swap chain и
    /// пересоздаёт Renderer под новый размер.
    fn handle_resize(&mut self, width: u32, height: u32) {
        println!("[ENGINE] Handling resize: {}x{}", width, height);

        let (queue_opt, fence_opt) = {
            let state = STATE.lock().unwrap();
            (state.command_queue.clone(), state.fence.clone())
        };
        if let (Some(queue), Some(fence)) = (queue_opt, fence_opt) {
            let value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
            unsafe {
                if queue.Signal(&fence, value).is_ok() {
                    if let Err(reason) = wait_for_fence(&fence, value, std::time::Duration::from_secs(5)) {
                        eprintln!("[ENGINE] handle_resize: {} — resize прерван", reason);
                        crate::dump_d3d12_debug_messages();
                        return;
                    }
                }
            }
        } else {
            return;
        }

        self.renderer = None;

        let resize_ok = {
            let state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                let hr = unsafe {
                    swap_chain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
                };
                match hr {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("[ENGINE] ResizeBuffers failed: {:?}", e);
                        false
                    }
                }
            } else {
                false
            }
        };

        if !resize_ok {
            return;
        }

        match Renderer::new(width, height, 2) {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                println!("[ENGINE] ✓ Renderer recreated after resize: {}x{}", width, height);
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to recreate renderer after resize: {:?}", e);
            }
        }

        // ИСПРАВЛЕНО (воспроизведённый DXGI_ERROR_DEVICE_HUNG на первых кадрах,
        // локализован бисекцией в src/bin/example_minimal.rs — см. его шапку):
        // `Renderer::new` выше пересоздаёт back buffer'ы, depth stencil и HDR
        // target, но ВСЁ ОСТАЛЬНОЕ, что зависит от размера окна ИЛИ ссылается
        // на пересозданные ресурсы, раньше оставалось от старого размера:
        //
        //  * `depth_srv_heap` — SRV, указывающий на depth stencil, который
        //    только что был УНИЧТОЖЕН и создан заново по другому адресу.
        //    То есть после любого ресайза volumetric-проход сэмплил висячий
        //    дескриптор освобождённого ресурса — классическая причина
        //    зависания GPU без единого сообщения валидации (обычный debug
        //    layer такое не ловит, он проверяет только корректность вызовов,
        //    а не содержимое дескрипторов; GBV — ловит/маскирует, чем и
        //    объяснялось, почему под GBV баг "исчезал").
        //  * `bloom_texture_a/b` и `volumetric_texture` — half-res таргеты,
        //    посчитанные как `self.width/2 x self.height/2` в момент init().
        //    Окно почти всегда получает WM_SIZE с ДРУГИМ (подогнанным под
        //    рамку) клиентским размером сразу после создания — например
        //    запрошенные 1366x768 превращаются в 1350x729 — и half-res
        //    таргеты навсегда оставались от исходного размера, не совпадая
        //    с реальным кадром.
        //
        // Порядок важен: `create_volumetric_resources` копирует дескрипторы
        // из `depth_srv_heap` и `shadow_maps` в свой смежный heap (см. его
        // комментарий), поэтому depth SRV должен быть пересоздан ДО него.
        // Shadow maps фиксированного размера (2048x2048) и от размера окна
        // не зависят — их пересоздавать не нужно. Occluder-ресурсы живут на
        // второй видеокарте с собственным фиксированным разрешением — тоже
        // не зависят от размера окна.
        //
        // GPU здесь уже гарантированно простаивает (fence дождались в начале
        // handle_resize), поэтому освобождение старых ресурсов при перезаписи
        // `self.*` безопасно.
        if self.renderer.is_some() {
            if let Err(e) = self.create_depth_srv_resources() {
                eprintln!("[ENGINE] WARNING: не удалось пересоздать depth SRV после ресайза: {:?} — volumetric-проход может сэмплить устаревший дескриптор", e);
            }
            if let Err(e) = self.create_bloom_resources() {
                eprintln!("[ENGINE] WARNING: не удалось пересоздать bloom-ресурсы после ресайза: {:?}", e);
            }
            if let Err(e) = self.create_volumetric_resources() {
                eprintln!("[ENGINE] WARNING: не удалось пересоздать volumetric-ресурсы после ресайза: {:?}", e);
            }
            println!("[ENGINE] ✓ Size-dependent resources recreated after resize (depth SRV + bloom + volumetric)");
        }

        // Регистрация bloom-таргета в heap'е рендерера (слот 1, откуда его
        // читает tonemap-проход) — ОБЯЗАТЕЛЬНО после пересоздания bloom выше,
        // иначе сюда попал бы дескриптор уже освобождённой старой текстуры.
        if let (Some(renderer), Some(bloom_a)) = (&self.renderer, &self.bloom_texture_a) {
            let cbv_srv_uav_size = {
                let state = STATE.lock().unwrap();
                state.cbv_srv_uav_descriptor_size
            };
            let bloom_final_srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&renderer.srv_uav_heap, 1, cbv_srv_uav_size);
            match bloom_a.create_srv(bloom_final_srv_cpu) {
                Ok(()) => println!("[ENGINE] ✓ Bloom SRV re-registered in new srv_uav_heap after resize"),
                Err(e) => eprintln!("[ENGINE] WARNING: failed to re-register bloom SRV after resize: {:?}", e),
            }
        }

        for v in &mut self.frame_fence_values {
            *v = 0;
        }

        let mut state = STATE.lock().unwrap();
        if let Some(swap_chain) = &state.swap_chain {
            state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
        }
    }

    pub fn process_messages(&mut self) {
        self.input.end_frame();

        unsafe {
            let mut msg = MSG::default();
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    println!("[ENGINE] WM_QUIT received - exiting message loop");
                    self.running = false;
                    break;
                }
                if msg.message == WM_DESTROY {
                    println!("[ENGINE] WM_DESTROY received in message loop");
                    self.running = false;
                    let _ = DefWindowProcA(msg.hwnd, msg.message, msg.wParam, msg.lParam);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }
        }
    }
}
