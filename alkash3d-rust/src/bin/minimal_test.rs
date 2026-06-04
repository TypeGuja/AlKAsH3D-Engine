// src/bin/engine_test_3d.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ (без ограничения кадров)

use alkash3d_rs::*;
use std::ptr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows_core::PCSTR;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const TARGET_FPS: u32 = 60;
const FRAME_TIME_NS: u64 = 1_000_000_000 / TARGET_FPS as u64;

static mut RUNNING: bool = true;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    color: [f32; 4],
}

impl Vertex {
    const STRIDE: usize = 28;
}

const VERTICES: [Vertex; 36] = [
    // Front face (red)
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    // Back face (green)
    Vertex { pos: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    // Right face (blue)
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [0.0, 0.0, 1.0, 1.0] },
    // Left face (yellow)
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5, -0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 1.0, 0.0, 1.0] },
    // Top face (magenta)
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0, 1.0] },
    // Bottom face (cyan)
    Vertex { pos: [-0.5, -0.5,  0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [-0.5, -0.5,  0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0, 1.0] },
];

macro_rules! log {
    ($($arg:tt)*) => {
        println!($($arg)*);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    };
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            RUNNING = false;
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                RUNNING = false;
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_window() -> HWND {
    let instance = GetModuleHandleA(None).unwrap();
    let class_name = b"3DCubeTest\0";
    let class_ptr = PCSTR(class_name.as_ptr());

    let wc = WNDCLASSA {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: HINSTANCE::from(instance),
        hIcon: HICON::default(),
        hCursor: HCURSOR::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: PCSTR::null(),
        lpszClassName: class_ptr,
    };

    RegisterClassA(&wc);

    CreateWindowExA(
        WINDOW_EX_STYLE::default(),
        class_ptr,
        PCSTR(b"3D Cube - ESC to exit\0".as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        WIDTH as i32,
        HEIGHT as i32,
        None,
        None,
        Some(HINSTANCE::from(instance)),
        None,
    ).unwrap()
}

fn mat4_perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (far - near), 1.0],
        [0.0, 0.0, -far * near / (far - near), 0.0],
    ]
}

fn mat4_look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let f_len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
    let forward = [f[0] / f_len, f[1] / f_len, f[2] / f_len];

    let s = [
        forward[1] * up[2] - forward[2] * up[1],
        forward[2] * up[0] - forward[0] * up[2],
        forward[0] * up[1] - forward[1] * up[0],
    ];
    let s_len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
    let side = [s[0] / s_len, s[1] / s_len, s[2] / s_len];

    let u = [
        side[1] * forward[2] - side[2] * forward[1],
        side[2] * forward[0] - side[0] * forward[2],
        side[0] * forward[1] - side[1] * forward[0],
    ];

    [
        [side[0], u[0], -forward[0], 0.0],
        [side[1], u[1], -forward[1], 0.0],
        [side[2], u[2], -forward[2], 0.0],
        [
            -(side[0] * eye[0] + side[1] * eye[1] + side[2] * eye[2]),
            -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
            forward[0] * eye[0] + forward[1] * eye[1] + forward[2] * eye[2],
            1.0,
        ],
    ]
}

fn mat4_rotate_y(angle: f32) -> [[f32; 4]; 4] {
    let c = angle.cos();
    let s = angle.sin();
    [
        [c, 0.0, s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [-s, 0.0, c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            result[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
        }
    }
    result
}

fn main() {
    log!("\n╔════════════════════════════════════════════════════════════════╗");
    log!("║                    3D ROTATING CUBE                             ║");
    log!("║                         ESC - exit                              ║");
    log!("╚════════════════════════════════════════════════════════════════╝\n");

    unsafe {
        // ===================================================================
        // 1. СОЗДАНИЕ ОКНА
        // ===================================================================
        let hwnd = create_window();
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        log!("[1] Window created");

        // ===================================================================
        // 2. СОЗДАНИЕ DEVICE И QUEUE
        // ===================================================================
        let pair_ptr = create_device_and_queue();
        if pair_ptr.is_null() {
            eprintln!("❌ Failed to create device and queue");
            return;
        }
        let device_ptr = get_device_from_pair(pair_ptr);
        log!("[2] Device ready");

        // ===================================================================
        // 3. СОЗДАНИЕ SWAP CHAIN
        // ===================================================================
        let swap_ptr = create_swap_chain(std::ptr::null_mut(), hwnd.0, WIDTH, HEIGHT);
        if swap_ptr.is_null() {
            eprintln!("❌ Failed to create swap chain");
            return;
        }
        log!("[3] Swap chain ready");

        // ===================================================================
        // 4. СОЗДАНИЕ FENCE
        // ===================================================================
        if !create_fence(device_ptr) {
            eprintln!("❌ Failed to create fence");
            return;
        }
        log!("[4] Fence ready");

        // ===================================================================
        // 5. СОЗДАНИЕ ROOT SIGNATURE И PSO
        // ===================================================================
        let root_sig = create_root_signature_simple(device_ptr);
        if root_sig.is_null() {
            eprintln!("❌ Failed to create root signature");
            return;
        }
        log!("[5] Root signature created");

        let pso = create_simple_pso(device_ptr, root_sig);
        if pso.is_null() {
            eprintln!("❌ Failed to create PSO");
            return;
        }
        log!("[5] PSO ready");

        // ===================================================================
        // 6. СОЗДАНИЕ VERTEX BUFFER
        // ===================================================================
        let vb = create_buffer(device_ptr, VERTICES.len() * Vertex::STRIDE, 0);
        if vb.is_null() {
            eprintln!("❌ Failed to create vertex buffer");
            return;
        }

        if !update_subresource(vb, VERTICES.as_ptr() as *const _, VERTICES.len() * Vertex::STRIDE) {
            eprintln!("❌ Failed to update vertex buffer");
            return;
        }

        let vb_gpu = get_buffer_gpu_address(vb);
        log!("[6] Vertex buffer ready, GPU addr: 0x{:X}", vb_gpu);

        // ===================================================================
        // 7. СОЗДАНИЕ CONSTANT BUFFER
        // ===================================================================
        let cb = create_buffer(device_ptr, 256, 0);
        if cb.is_null() {
            eprintln!("❌ Failed to create constant buffer");
            return;
        }

        let cb_gpu = get_buffer_gpu_address(cb);
        let cb_map = map_buffer(cb);
        if cb_map.is_null() {
            eprintln!("❌ Failed to map constant buffer");
            return;
        }
        log!("[7] Constant buffer ready, GPU addr: 0x{:X}", cb_gpu);

        // ===================================================================
        // 8. СОЗДАНИЕ RTV HEAP (ДЛЯ 2 BACK BUFFER'ОВ!)
        // ===================================================================
        let rtv_heap = create_descriptor_heap(device_ptr, 2, 0, false);
        if rtv_heap.is_null() {
            eprintln!("❌ Failed to create RTV heap");
            return;
        }

        let rtv_base_handle = GetCPUDescriptorHandleForHeapStart(rtv_heap);
        let rtv_increment = get_descriptor_handle_increment_size(device_ptr, 0);

        log!("[8] RTV base handle: 0x{:X}, increment: {}", rtv_base_handle, rtv_increment);

        // ===================================================================
        // 9. СОЗДАНИЕ RTV ДЛЯ ОБОИХ BACK BUFFER'ОВ (ОДИН РАЗ!)
        // ===================================================================
        let back_buffer0 = swap_chain_get_buffer(swap_ptr, 0);
        let back_buffer1 = swap_chain_get_buffer(swap_ptr, 1);

        let rtv_handle0 = rtv_base_handle;
        let rtv_handle1 = rtv_base_handle + rtv_increment as u64;

        if !create_render_target_view(device_ptr, back_buffer0, rtv_handle0) {
            eprintln!("❌ Failed to create RTV for buffer 0");
            return;
        }

        if !create_render_target_view(device_ptr, back_buffer1, rtv_handle1) {
            eprintln!("❌ Failed to create RTV for buffer 1");
            return;
        }

        log!("[9] RTV handles created: 0x{:X} and 0x{:X}", rtv_handle0, rtv_handle1);

        log!("\n[10] START RENDERING (press ESC to exit)\n");

        // ===================================================================
        // 10. ГЛАВНЫЙ РЕНДЕР-ЦИКЛ (БЕЗ ОГРАНИЧЕНИЯ КАДРОВ)
        // ===================================================================
        let mut msg = std::mem::zeroed();
        let start = Instant::now();
        let mut frame = 0;
        let mut frame_timer = Instant::now();

        while RUNNING {
            // Обработка сообщений Windows
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    RUNNING = false;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if !RUNNING {
                break;
            }

            // FPS лимитер
            let elapsed = frame_timer.elapsed();
            if elapsed.as_nanos() < FRAME_TIME_NS as u128 {
                sleep(Duration::from_nanos(FRAME_TIME_NS - elapsed.as_nanos() as u64));
            }
            frame_timer = Instant::now();

            // Вычисление матрицы MVP - ПРАВИЛЬНЫЙ ПОРЯДОК: proj * view * model
            let angle = start.elapsed().as_secs_f32() * 1.5;
            let proj = mat4_perspective(60.0_f32.to_radians(), WIDTH as f32 / HEIGHT as f32, 0.1, 100.0);
            let view = mat4_look_at([3.0, 2.0, 4.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
            let model = mat4_rotate_y(angle);
            let mvp = mat4_mul(proj, mat4_mul(view, model));  // proj * view * model

            // Обновление constant buffer
            ptr::copy_nonoverlapping(&mvp as *const _ as *const u8, cb_map as *mut u8, 64);

            // Получаем текущий back buffer index и выбираем соответствующий RTV
            let back_buffer_idx = get_current_back_buffer_index(swap_ptr);
            let current_rtv_handle = if back_buffer_idx == 0 { rtv_handle0 } else { rtv_handle1 };

            // Начало кадра
            let cmd_list = begin_frame();
            if cmd_list.is_null() {
                log!("❌ ERROR: begin_frame failed");
                break;
            }

            // Установка PSO
            if !set_pipeline(pso) {
                log!("❌ ERROR: set_pipeline failed");
                break;
            }

            // Установка root signature
            if !set_root_signature(root_sig) {
                log!("❌ ERROR: set_root_signature failed");
                break;
            }

            // Установка render target
            if !set_render_target(current_rtv_handle) {
                log!("❌ ERROR: set_render_target failed");
                break;
            }

            // Очистка экрана
            let clear_color = [0.1f32, 0.1f32, 0.2f32, 1.0f32];
            if !clear_render_target(current_rtv_handle, clear_color.as_ptr()) {
                log!("❌ ERROR: clear_render_target failed");
                break;
            }

            // Установка viewport
            if !set_viewport(0.0, 0.0, WIDTH as f32, HEIGHT as f32, 0.0, 1.0) {
                log!("❌ ERROR: set_viewport failed");
                break;
            }

            // Установка vertex buffer
            if !set_vertex_buffer(vb_gpu, (VERTICES.len() * Vertex::STRIDE) as u32, Vertex::STRIDE as u32) {
                log!("❌ ERROR: set_vertex_buffer failed");
                break;
            }

            // Установка топологии
            if !set_primitive_topology(4) {
                log!("❌ ERROR: set_primitive_topology failed");
                break;
            }

            // Установка constant buffer
            if !set_root_constant_buffer_view(0, cb_gpu) {
                log!("❌ ERROR: set_root_constant_buffer_view failed");
                break;
            }

            // Отрисовка
            if !draw_instanced(36, 1, 0, 0) {
                log!("❌ ERROR: draw_instanced failed");
                break;
            }

            // Завершение кадра
            if !end_frame() {
                log!("❌ ERROR: end_frame failed");
                break;
            }

            // Презентация
            if !present_swap_chain(swap_ptr, 0) {
                log!("❌ ERROR: present failed");
                break;
            }

            frame += 1;

            if frame % 60 == 0 {
                log!("Frame {} | Angle: {:.1}°", frame, angle.to_degrees());
            }
        }

        log!("\n[11] Shutdown, {} frames rendered", frame);

        // ===================================================================
        // 11. ОЧИСТКА
        // ===================================================================
        wait_for_gpu();
        force_cleanup();

        destroy_buffer(vb);
        destroy_buffer(cb);
        destroy_pso(pso);
        destroy_root_signature(root_sig);
        destroy_descriptor_heap(rtv_heap);
        destroy_swap_chain(swap_ptr);
        destroy_device_queue_pair(pair_ptr);

        log!("✅ Done!\n");
    }
}