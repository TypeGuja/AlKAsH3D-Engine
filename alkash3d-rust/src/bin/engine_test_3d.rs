// src/bin/engine_test_3d.rs
//! 3D тест - вращающийся куб с использованием движка

use alkash3d_rs::*;
use std::ptr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16_UINT;
use windows::Win32::System::Threading::{CreateEventA, WaitForSingleObject, INFINITE};
use windows_core::{BOOL, PCSTR};

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

static mut RUNNING: bool = true;
static mut ANGLE: f32 = 0.0;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    color: [f32; 4],
}

impl Vertex {
    const STRIDE: usize = 28;
}

// Вершины куба (36 вершин для 12 треугольников)
const VERTICES: [Vertex; 36] = [
    // Передняя грань (красная)
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 0.0, 1.0] },
    // Задняя грань (зелёная)
    Vertex { pos: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [0.0, 1.0, 0.0, 1.0] },
    // Правая грань (синяя)
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [0.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [0.0, 0.0, 1.0, 1.0] },
    // Левая грань (жёлтая)
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5, -0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5, -0.5,  0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [1.0, 1.0, 0.0, 1.0] },
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 1.0, 0.0, 1.0] },
    // Верхняя грань (пурпурная)
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [-0.5,  0.5,  0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0, 1.0] },
    Vertex { pos: [-0.5,  0.5, -0.5], color: [1.0, 0.0, 1.0, 1.0] },
    // Нижняя грань (циан)
    Vertex { pos: [-0.5, -0.5,  0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5,  0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [-0.5, -0.5,  0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [ 0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0, 1.0] },
    Vertex { pos: [-0.5, -0.5, -0.5], color: [0.0, 1.0, 1.0, 1.0] },
];

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

#[link(name = "user32")]
extern "system" {
    fn IsWindow(hWnd: HWND) -> BOOL;
}

unsafe fn create_window() -> HWND {
    let instance = GetModuleHandleA(None).unwrap();
    let class_name = b"Simple3DTest\0";
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

    let hwnd = CreateWindowExA(
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
    ).unwrap();

    hwnd
}

fn mat4_identity() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
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
    let f = [
        target[0] - eye[0],
        target[1] - eye[1],
        target[2] - eye[2],
    ];
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
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║                    3D ROTATING CUBE                             ║");
    println!("║                         ESC - exit                              ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    unsafe {
        // ====================================================================
        // 1. СОЗДАНИЕ ОКНА
        // ====================================================================
        let hwnd = create_window();
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        // Проверка HWND
        println!("[1] Window created: {:?}", hwnd);
        println!("    HWND raw: {:p}", hwnd.0);
        println!("    IsWindow: {}", IsWindow(hwnd).as_bool());

        // ====================================================================
        // 2. СОЗДАНИЕ DEVICE
        // ====================================================================
        let device_ptr = create_device();
        if device_ptr.is_null() {
            eprintln!("❌ Failed to create device");
            return;
        }
        println!("[2] Device created");

        // ====================================================================
        // 3. СОЗДАНИЕ COMMAND QUEUE
        // ====================================================================
        let queue_ptr = create_command_queue(std::ptr::null_mut());
        if queue_ptr.is_null() {
            eprintln!("❌ Failed to create command queue");
            return;
        }
        println!("[3] Command queue created");

        // ====================================================================
        // 4. СОЗДАНИЕ SWAP CHAIN
        // ====================================================================
        let swap_ptr = create_swap_chain(queue_ptr, hwnd.0, WIDTH, HEIGHT);
        if swap_ptr.is_null() {
            eprintln!("❌ Failed to create swap chain");
            return;
        }
        println!("[4] Swap chain created");

        // ====================================================================
        // 5. RENDER TARGET VIEW
        // ====================================================================
        let rtv_heap_ptr = create_descriptor_heap(device_ptr, 2, 0, false);
        if rtv_heap_ptr.is_null() {
            eprintln!("❌ Failed to create RTV heap");
            return;
        }
        let rtv_handle = GetCPUDescriptorHandleForHeapStart(rtv_heap_ptr);
        println!("[5] RTV heap created");

        let back_buffer_ptr = swap_chain_get_buffer(swap_ptr, 0);
        if back_buffer_ptr.is_null() {
            eprintln!("❌ Failed to get back buffer");
            return;
        }

        if !create_render_target_view(device_ptr, back_buffer_ptr, rtv_handle) {
            eprintln!("❌ Failed to create RTV");
            return;
        }
        println!("[6] RTV created");

        // ====================================================================
        // 6. ROOT SIGNATURE И PSO
        // ====================================================================
        let root_sig_ptr = create_root_signature_simple(device_ptr);
        if root_sig_ptr.is_null() {
            eprintln!("❌ Failed to create root signature");
            return;
        }

        let pso_ptr = create_simple_pso(device_ptr, root_sig_ptr);
        if pso_ptr.is_null() {
            eprintln!("❌ Failed to create PSO");
            return;
        }
        println!("[7] Root signature and PSO created");

        // ====================================================================
        // 7. ВЕРТЕКСНЫЙ БУФЕР
        // ====================================================================
        let vb = create_buffer(device_ptr, VERTICES.len() * Vertex::STRIDE, 0);
        if vb.is_null() {
            eprintln!("❌ Failed to create vertex buffer");
            return;
        }
        update_subresource(vb, VERTICES.as_ptr() as *const _, VERTICES.len() * Vertex::STRIDE);
        let vb_gpu = get_buffer_gpu_address(vb);
        println!("[8] Vertex buffer created");

        let vb_view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: vb_gpu,
            SizeInBytes: (VERTICES.len() * Vertex::STRIDE) as u32,
            StrideInBytes: Vertex::STRIDE as u32,
        };

        // ====================================================================
        // 8. CONSTANT BUFFER
        // ====================================================================
        let const_buf = create_buffer(device_ptr, 256, 0);
        if const_buf.is_null() {
            eprintln!("❌ Failed to create constant buffer");
            return;
        }
        let const_gpu = get_buffer_gpu_address(const_buf);
        let const_map = map_buffer(const_buf);
        if const_map.is_null() {
            eprintln!("❌ Failed to map constant buffer");
            return;
        }
        println!("[9] Constant buffer created and mapped");

        // ====================================================================
        // 9. FENCE И ALLOCATOR
        // ====================================================================
        if !create_fence(device_ptr) {
            eprintln!("❌ Failed to create fence");
            return;
        }
        println!("[10] Fence created");

        // ====================================================================
        // 10. ОСНОВНОЙ ЦИКЛ
        // ====================================================================
        println!("\n[11] Starting main loop...\n");

        let mut msg = std::mem::zeroed();
        let start = Instant::now();
        let mut frame = 0;
        let mut last_fps = Instant::now();
        let mut fps_counter = 0;

        while RUNNING {
            let elapsed = start.elapsed().as_secs_f32();
            ANGLE = elapsed * 1.5;

            // Вычисляем MVP матрицу
            let aspect = WIDTH as f32 / HEIGHT as f32;
            let proj = mat4_perspective(60.0_f32.to_radians(), aspect, 0.1, 100.0);
            let view = mat4_look_at([3.0, 2.0, 4.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
            let model = mat4_rotate_y(ANGLE);
            let mvp = mat4_mul(mat4_mul(model, view), proj);

            // Обновляем constant buffer
            ptr::copy_nonoverlapping(&mvp as *const _ as *const u8, const_map as *mut u8, 64);

            // Обработка сообщений
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    RUNNING = false;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            // Начинаем кадр
            let cmd_list = begin_frame_ex(pso_ptr);
            if cmd_list.is_null() {
                continue;
            }

            // Устанавливаем PSO и root signature
            set_pipeline(pso_ptr);
            set_root_signature(root_sig_ptr);

            // Переход ресурса в RENDER_TARGET
            transition_resource(back_buffer_ptr, 0, 1); // PRESENT -> RENDER_TARGET

            // Устанавливаем render target
            set_render_target(rtv_handle);

            // Очищаем
            let clear_color = [0.1f32, 0.1f32, 0.2f32, 1.0f32];
            clear_render_target(rtv_handle, clear_color.as_ptr());

            // Viewport и Scissor
            set_viewport(0.0, 0.0, WIDTH as f32, HEIGHT as f32, 0.0, 1.0);
            set_scissor_rect(0, 0, WIDTH as i32, HEIGHT as i32);

            // Устанавливаем буферы
            set_vertex_buffer(vb_gpu, vb_view.SizeInBytes, vb_view.StrideInBytes);
            set_primitive_topology(4); // D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST

            // Устанавливаем constant buffer
            set_root_constant_buffer_view(0, const_gpu);

            // Рисуем куб (36 вершин)
            draw_instanced(36, 1, 0, 0);

            // Переход обратно в PRESENT
            transition_resource(back_buffer_ptr, 1, 0); // RENDER_TARGET -> PRESENT

            // Завершаем кадр
            end_frame();

            // Презентуем
            present_swap_chain(swap_ptr, 1);

            frame += 1;
            fps_counter += 1;

            if last_fps.elapsed().as_secs_f32() >= 1.0 {
                println!("Frame {} | FPS: {} | Angle: {:.1}°", frame, fps_counter, ANGLE.to_degrees());
                fps_counter = 0;
                last_fps = Instant::now();
            }

            sleep(Duration::from_millis(16));
        }

        // ====================================================================
        // 11. ЗАВЕРШЕНИЕ
        // ====================================================================
        println!("\n[12] Shutting down...");
        println!("    Total frames: {}", frame);

        wait_for_gpu();
        force_cleanup();

        println!("    ✅ Done!\n");
    }
}