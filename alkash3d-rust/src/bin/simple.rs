// src/bin/simple.rs - ПОЛНОСТЬЮ ИСПРАВЛЕННАЯ РАБОЧАЯ ВЕРСИЯ
use alkash3d_rs::*;
use std::ptr;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::PCSTR;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: f32, y: f32, z: f32,
    r: f32, g: f32, b: f32, a: f32,
}

// ОДИН БОЛЬШОЙ ТРЕУГОЛЬНИК НА ВЕСЬ ЭКРАН
const VERTICES: [Vertex; 3] = [
    Vertex { x: -1.0, y: -1.0, z: 0.0, r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
    Vertex { x:  1.0, y: -1.0, z: 0.0, r: 0.0, g: 1.0, b: 0.0, a: 1.0 },
    Vertex { x:  0.0, y:  1.0, z: 0.0, r: 0.0, g: 0.0, b: 1.0, a: 1.0 },
];

// Матрица трансформации (identity для NDC координат)
const IDENTITY_MATRIX: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

fn main() {
    println!("\n╔═══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                         SIMPLE TRIANGLE TEST                                    ║");
    println!("║                    You should see ONE COLORFUL triangle!                       ║");
    println!("║                           Press ESC to exit                                    ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════╝\n");

    unsafe {
        let instance = GetModuleHandleA(None).unwrap();
        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE::from(instance),
            lpszClassName: PCSTR(b"SimpleTriangle\0".as_ptr()),
            ..Default::default()
        };
        RegisterClassA(&wc);

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(b"SimpleTriangle\0".as_ptr()),
            PCSTR(b"ALKASH3D - SIMPLE TRIANGLE\0".as_ptr()),
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

        println!("[9] Creating PSO...");
        let pso = create_pso(device, root_sig, 0);
        if pso.is_null() {
            println!("❌ Failed to create PSO!");
            return;
        }
        println!("[10] PSO created");

        println!("[11] Creating vertex buffer...");
        let vb = create_buffer(device, std::mem::size_of_val(&VERTICES), 0); // UPLOAD heap
        if vb.is_null() {
            println!("❌ Failed to create vertex buffer!");
            return;
        }

        // Заполняем буфер данными
        if !update_subresource(vb, VERTICES.as_ptr() as *const _, std::mem::size_of_val(&VERTICES)) {
            println!("❌ Failed to update vertex buffer!");
            return;
        }
        println!("[12] Vertex buffer filled");

        let vb_gpu = get_buffer_gpu_address(vb);
        println!("[12] Vertex buffer GPU addr: 0x{:X}", vb_gpu);

        // Проверяем данные в буфере
        let verify_ptr = map_buffer(vb);
        if !verify_ptr.is_null() {
            let verts = std::slice::from_raw_parts(verify_ptr as *const Vertex, 3);
            println!("[DEBUG] Vertex 0: ({:.2},{:.2},{:.2}) RGB({:.2},{:.2},{:.2})",
                     verts[0].x, verts[0].y, verts[0].z, verts[0].r, verts[0].g, verts[0].b);
            unmap_buffer(vb);
        }

        // Переводим вершинный буфер в состояние VERTEX_AND_CONSTANT_BUFFER
        transition_resource(vb, 0, 0x8); // D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER = 0x8

        println!("[12.5] Creating constant buffer...");
        let cbv_buffer = create_buffer(device, 256, 0); // UPLOAD heap
        if cbv_buffer.is_null() {
            println!("❌ Failed to create constant buffer!");
            return;
        }

        if !update_subresource(cbv_buffer, IDENTITY_MATRIX.as_ptr() as *const _, 64) {
            println!("❌ Failed to update constant buffer!");
            return;
        }

        let cbv_gpu = get_buffer_gpu_address(cbv_buffer);
        println!("[12.6] Constant buffer GPU addr: 0x{:X}", cbv_gpu);

        // Переводим constant buffer в состояние VERTEX_AND_CONSTANT_BUFFER
        transition_resource(cbv_buffer, 0, 0x8);

        wait_for_gpu();

        println!("[13] Creating RTV heap...");
        let rtv_heap = create_descriptor_heap(device, 2, 0, false);
        let rtv_base = GetCPUDescriptorHandleForHeapStart(rtv_heap);
        let rtv_inc = get_descriptor_handle_increment_size(device, 0);
        println!("[14] RTV heap created");

        println!("[15] Getting back buffers...");
        let back0 = swap_chain_get_buffer(swap, 0);
        let back1 = swap_chain_get_buffer(swap, 1);
        if back0.is_null() || back1.is_null() {
            println!("❌ Failed to get back buffers!");
            return;
        }

        create_render_target_view(device, back0, rtv_base);
        create_render_target_view(device, back1, rtv_base + rtv_inc as u64);
        println!("[16] RTVs created");

        println!("\n✓ ALL INITIALIZED! Starting render loop...\n");

        let mut frame = 0;
        let mut msg = std::mem::zeroed();
        let mut running = true;

        while running && frame < 200 {
            // Обработка сообщений
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT || (msg.message == WM_KEYDOWN && msg.wParam.0 as u32 == 0x1B) {
                    running = false;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if !running {
                break;
            }

            // Получаем текущий back buffer
            let idx = get_current_back_buffer_index(swap);
            let back_buffer = swap_chain_get_buffer(swap, idx);
            let rtv = if idx == 0 { rtv_base } else { rtv_base + rtv_inc as u64 };

            // Переводим back buffer в состояние RENDER_TARGET
            transition_resource(back_buffer, 0x0, 0x4); // PRESENT -> RENDER_TARGET

            // Начинаем кадр
            let cmd_list = begin_frame();
            if cmd_list.is_null() {
                println!("❌ Failed to begin frame!");
                break;
            }

            // Устанавливаем PSO и root signature
            set_pipeline_state(pso);
            set_root_signature(root_sig);

            // Устанавливаем constant buffer
            set_root_constant_buffer_view(0, cbv_gpu);

            // Устанавливаем render target
            set_render_target(rtv);

            // Очищаем экран синим цветом для теста
            let clear_color = [0.0, 0.0, 1.0, 1.0]; // Синий фон
            clear_render_target(rtv, clear_color.as_ptr());

            // Устанавливаем viewport
            set_viewport(0.0, 0.0, WIDTH as f32, HEIGHT as f32, 0.0, 1.0);

            // Устанавливаем вершинный буфер (stride = 28 байт: 3*4 + 4*4 = 12 + 16 = 28)
            set_vertex_buffer(vb_gpu, std::mem::size_of_val(&VERTICES) as u32, 28);

            // Устанавливаем топологию треугольников
            set_primitive_topology(4); // D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST

            // Рисуем!
            draw_instanced(3, 1, 0, 0);

            // Заканчиваем кадр
            end_frame();

            // Переводим back buffer обратно в состояние PRESENT
            transition_resource(back_buffer, 0x4, 0x0); // RENDER_TARGET -> PRESENT

            // Отображаем
            present_swap_chain(swap, 1);

            // Ждём GPU для первого кадра
            if frame == 0 {
                wait_for_gpu();
                println!("✓ First frame rendered! You should see a BLUE background with a COLORFUL triangle!");
            }

            frame += 1;

            // Маленькая задержка для стабильности
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        println!("\n[17] Cleaning up...");
        wait_for_gpu();
        force_cleanup();

        destroy_buffer(vb);
        destroy_buffer(cbv_buffer);
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
        WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
        WM_KEYDOWN if wparam.0 as u32 == 0x1B => { PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}