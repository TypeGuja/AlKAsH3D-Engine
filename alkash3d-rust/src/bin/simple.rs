// src/bin/simple.rs - ПОЛНОСТЬЮ ИСПРАВЛЕННАЯ ВЕРСИЯ
use alkash3d_rs::*;
use std::ptr;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::PCSTR;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

#[repr(C)]
struct Vertex {
    x: f32, y: f32, z: f32,
    r: f32, g: f32, b: f32, a: f32,
}

// Треугольник в пределах видимого экрана (-1..1)
const VERTICES: [Vertex; 3] = [
    Vertex { x: -0.5, y: -0.5, z: 0.0, r: 1.0, g: 0.0, b: 0.0, a: 1.0 },  // красный - левый нижний
    Vertex { x:  0.5, y: -0.5, z: 0.0, r: 0.0, g: 1.0, b: 0.0, a: 1.0 },  // зелёный - правый нижний
    Vertex { x:  0.0, y:  0.5, z: 0.0, r: 0.0, g: 0.0, b: 1.0, a: 1.0 },  // синий - верхний центр
];

fn main() {
    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    ULTIMATE TRIANGLE TEST                      ║");
    println!("║              You should see a COLORFUL triangle!               ║");
    println!("║                        Press ESC to exit                       ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    unsafe {
        // Создаём окно
        let instance = GetModuleHandleA(None).unwrap();
        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE::from(instance),
            lpszClassName: PCSTR(b"UltimateTest\0".as_ptr()),
            ..Default::default()
        };
        RegisterClassA(&wc);

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(b"UltimateTest\0".as_ptr()),
            PCSTR(b"ALKASH3D - COLORFUL TRIANGLE\0".as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT,
            WIDTH as i32, HEIGHT as i32,
            None, None, Some(HINSTANCE::from(instance)), None,
        ).unwrap();

        println!("[1] Creating D3D12 device...");
        let pair = create_device_and_queue();
        let device = get_device_from_pair(pair);
        println!("[2] Device created: {:p}", device);

        println!("[3] Creating swap chain...");
        let swap = create_swap_chain(ptr::null_mut(), hwnd.0, WIDTH, HEIGHT);
        if swap.is_null() {
            println!("❌ Failed to create swap chain!");
            return;
        }
        println!("[4] Swap chain created: {:p}", swap);

        println!("[5] Creating fence...");
        create_fence(device);
        println!("[6] Fence created");

        println!("[6.5] Initializing builtin shaders...");
        init_builtin_shaders();

        println!("[7] Creating root signature...");
        let root_sig = create_root_signature_simple(device);

        if root_sig.is_null() {
            println!("❌ Failed to create root signature!");
            return;
        }
        println!("[8] Root signature created");

        println!("[9] Creating PSO with test shader...");
        let pso = create_pso(device, root_sig, 0);
        if pso.is_null() {
            println!("❌ Failed to create PSO!");
            return;
        }
        println!("[10] PSO created");

        println!("[11] Creating vertex buffer...");
        let vb = create_buffer(device, std::mem::size_of_val(&VERTICES), 0);
        if vb.is_null() {
            println!("❌ Failed to create vertex buffer!");
            return;
        }

        // Ждём GPU перед обновлением буфера

        update_subresource(vb, VERTICES.as_ptr() as *const _, std::mem::size_of_val(&VERTICES));

        let vb_gpu = get_buffer_gpu_address(vb);
        println!("[12] Vertex buffer created: GPU addr 0x{:X}", vb_gpu);

        println!("[13] Creating constant buffer...");
        let cb = create_buffer(device, 256, 0);
        let cb_gpu = get_buffer_gpu_address(cb);
        println!("[14] Constant buffer created: GPU addr 0x{:X}", cb_gpu);

        wait_for_gpu();

        println!("[15] Creating RTV heap...");
        let rtv_heap = create_descriptor_heap(device, 2, 0, false);
        let rtv_base = GetCPUDescriptorHandleForHeapStart(rtv_heap);
        let rtv_inc = get_descriptor_handle_increment_size(device, 0);
        println!("[16] RTV heap created: base=0x{:X}, inc={}", rtv_base, rtv_inc);

        println!("[17] Getting back buffers...");
        let back0 = swap_chain_get_buffer(swap, 0);
        let back1 = swap_chain_get_buffer(swap, 1);
        if back0.is_null() || back1.is_null() {
            println!("❌ Failed to get back buffers!");
            return;
        }
        println!("[18] Back buffers: 0={:p}, 1={:p}", back0, back1);

        create_render_target_view(device, back0, rtv_base);
        create_render_target_view(device, back1, rtv_base + rtv_inc as u64);
        println!("[19] RTVs created");

        println!("\n✓ ALL INITIALIZED!");
        println!("✓ Looking for triangle...\n");

        let mut frame = 0;
        let mut msg = std::mem::zeroed();
        let mut running = true;

        // Отрисовываем 100 кадров или пока не нажмут ESC
        while running && frame < 100 {
            // Обработка сообщений окна
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    running = false;
                }
                if msg.message == WM_KEYDOWN && msg.wParam.0 as u32 == 0x1B {
                    running = false;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            let idx = get_current_back_buffer_index(swap);
            let rtv = if idx == 0 { rtv_base } else { rtv_base + rtv_inc as u64 };

            // Начинаем кадр
            begin_frame();

            // Устанавливаем PSO и root signature
            set_pipeline(pso);
            set_root_signature(root_sig);

            // Устанавливаем render target и очищаем его (чёрный фон)
            set_render_target(rtv);
            let clear_color = [0.0, 0.0, 0.0, 1.0];
            clear_render_target(rtv, clear_color.as_ptr());

            // Устанавливаем viewport
            set_viewport(0.0, 0.0, WIDTH as f32, HEIGHT as f32, 0.0, 1.0);

            // Устанавливаем вершинный буфер
            set_vertex_buffer(vb_gpu, std::mem::size_of_val(&VERTICES) as u32, 28);

            // Устанавливаем топологию треугольника
            set_primitive_topology(4);  // D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST

            // Устанавливаем константный буфер
            set_root_constant_buffer_view(0, cb_gpu);

            // Рисуем 3 вершины
            draw_instanced(3, 1, 0, 0);

            // Завершаем кадр и презентуем
            end_frame();
            present_swap_chain(swap, 0);

            if frame == 0 {
                println!("✓ Frame 0 rendered successfully!");
                println!("✓ You should see a COLORFUL triangle on BLACK background!");
                println!("✓ Colors: RED (bottom-left), GREEN (bottom-right), BLUE (top)\n");
            }

            frame += 1;

            // Небольшая задержка для плавности
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        println!("\n[20] Cleaning up...");

        // Ждём завершения всех GPU операций
        wait_for_gpu();

        // Очищаем ресурсы
        force_cleanup();
        destroy_buffer(vb);
        destroy_buffer(cb);
        destroy_pso(pso);
        destroy_root_signature(root_sig);
        destroy_descriptor_heap(rtv_heap);
        destroy_swap_chain(swap);
        destroy_device_queue_pair(pair);

        println!("✓ Done!");
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u32 == 0x1B => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}