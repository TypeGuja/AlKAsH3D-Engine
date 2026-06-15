// src/bin/main.rs
#![allow(never_type_fallback_flowing_into_unsafe)]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_WINDOW, HBRUSH};
use alkash3d_rs::engine::AlkashEngine;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

fn main() -> Result<()> {
    println!("==========================================");
    println!("Alkash3D Engine v{} - Starting...", alkash3d_rs::VERSION);
    println!("==========================================");

    unsafe {
        println!("[MAIN] Creating window...");
        let hinstance = GetModuleHandleA(None)?;
        let window_class = "ALKASH3D_WINDOW";

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCSTR(window_class.as_ptr()),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as _),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };

        RegisterClassA(&wc);

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(window_class.as_ptr()),
            PCSTR(b"Alkash3D Engine - DirectX 12\0".as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH as i32,
            WINDOW_HEIGHT as i32,
            None,
            None,
            Some(HINSTANCE::from(hinstance)),
            None,
        )?;

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        println!("[MAIN] Window created");

        let mut engine = AlkashEngine::new(WINDOW_WIDTH, WINDOW_HEIGHT);

        if let Err(e) = engine.init(hwnd.0 as isize) {
            eprintln!("[MAIN] Engine init failed: {:?}", e);
            return Err(e);
        }

        println!("\n[MAIN] Adding objects to scene...");

        engine.add_triangle();
        engine.add_quad(0.5, -0.5, 0.4, 0.4, [0.0, 1.0, 1.0, 1.0]);
        engine.add_cube(0.5);
        engine.set_clear_color(0.05, 0.05, 0.1, 1.0);

        println!("\n=== RENDER LOOP STARTING ===\n");
        println!("Objects in scene: {} meshes\n", engine.meshes.len());

        let mut running = true;
        let mut frame_count = 0u32;

        while running {
            let mut msg = MSG::default();
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    running = false;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if !running { break; }

            if let Err(e) = engine.render_frame() {
                eprintln!("[MAIN] Render error: {:?}", e);
                break;
            }

            frame_count += 1;

            if frame_count == 1 {
                println!("\n*** FIRST FRAME COMPLETED ***");
                println!("*** Engine is rendering {} meshes ***\n", engine.meshes.len());
            }

            if frame_count % 60 == 0 {
                println!("[INFO] Frame {} rendered, {} meshes on screen",
                         frame_count, engine.meshes.len());
            }
        }

        println!("\n=== ENGINE SHUTDOWN ===\n");
        engine.shutdown();
    }

    Ok(())
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if wparam.0 == 0x1B {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            _ => DefWindowProcA(hwnd, msg, wparam, lparam),
        }
    }
}