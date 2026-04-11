// main.rs
mod obj_loader;
mod camera;
mod math;
mod renderer;

use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, RegisterClassW,
    DestroyWindow, TranslateMessage, DispatchMessageW,
    PostQuitMessage, PeekMessageW, ShowWindow, UpdateWindow,
    CS_HREDRAW, CS_VREDRAW, WS_OVERLAPPEDWINDOW, CW_USEDEFAULT,
    WM_DESTROY, WM_QUIT, PM_REMOVE, SW_SHOW
};
use winapi::shared::minwindef::{UINT, WPARAM, LPARAM, LRESULT};
use winapi::shared::windef::HWND;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::MSG;

use crate::renderer::Renderer;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

fn main() {
    println!("=== Alkash3D OBJ Viewer ===");
    println!("Place alkash3d_rs.dll in the same folder as this executable");

    unsafe {
        let hwnd = create_window();
        if hwnd.is_null() {
            eprintln!("Failed to create window!");
            return;
        }

        // ПОКАЗЫВАЕМ ОКНО!
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        println!("Window created and shown!");

        let mut renderer = Renderer::new(hwnd as usize, WINDOW_WIDTH, WINDOW_HEIGHT);

        if !renderer.init() {
            eprintln!("Failed to initialize renderer!");
            DestroyWindow(hwnd);
            return;
        }

        println!("Starting render loop...");
        println!("Press ESC to exit");

        let mut running = true;
        let mut msg: MSG = std::mem::zeroed();

        while running {
            // Обработка сообщений Windows
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    running = false;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            renderer.begin_frame();
            renderer.end_frame();

            // Небольшая задержка для снижения нагрузки CPU
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        renderer.cleanup();
        DestroyWindow(hwnd);
        println!("Application closed.");
    }
}

unsafe fn create_window() -> HWND {
    let instance = GetModuleHandleW(std::ptr::null());
    let window_class: Vec<u16> = "Alkash3DWindow\0".encode_utf16().collect();

    let wc = winapi::um::winuser::WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: window_class.as_ptr(),
    };

    let atom = RegisterClassW(&wc);
    if atom == 0 {
        eprintln!("Failed to register window class!");
        return std::ptr::null_mut();
    }
    println!("Window class registered");

    let title: Vec<u16> = "Alkash3D OBJ Viewer\0".encode_utf16().collect();

    let hwnd = CreateWindowExW(
        0,
        window_class.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT,
        WINDOW_WIDTH as i32, WINDOW_HEIGHT as i32,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        instance,
        std::ptr::null_mut(),
    );

    if hwnd.is_null() {
        eprintln!("CreateWindowExW failed! Error: {}", GetLastError());
    } else {
        println!("Window created successfully");
    }

    hwnd
}

// Добавим функцию GetLastError для диагностики
unsafe fn GetLastError() -> u32 {
    winapi::um::errhandlingapi::GetLastError()
}

extern "system" fn wnd_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_DESTROY => {
                println!("WM_DESTROY received");
                PostQuitMessage(0);
                return 0;
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}