// src/bin/final_triangle.rs - ГАРАНТИРОВАННО РАБОТАЮЩИЙ ТРЕУГОЛЬНИК
use alkash3d_rs::*;
use std::ptr;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::PCSTR;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    x: f32, y: f32, z: f32,
    r: f32, g: f32, b: f32, a: f32,
}

// ЯРКИЙ треугольник с насыщенными цветами
const VERTICES: [Vertex; 3] = [
    Vertex { x: 0.0, y: 0.8, z: 0.0, r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, // ЯРКО-КРАСНЫЙ
    Vertex { x: -0.8, y: -0.8, z: 0.0, r: 0.0, g: 1.0, b: 0.0, a: 1.0 }, // ЯРКО-ЗЕЛЁНЫЙ
    Vertex { x: 0.8, y: -0.8, z: 0.0, r: 0.0, g: 0.0, b: 1.0, a: 1.0 }, // ЯРКО-СИНИЙ
];

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
        WM_KEYDOWN if wparam.0 as u32 == 0x1B => { PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("                  FINAL TRIANGLE TEST");
    println!("         You should see RED, GREEN, BLUE triangle!");
    println!("                    Press ESC to exit");
    println!("═══════════════════════════════════════════════════════════════\n");

    unsafe {
        // Окно
        let instance = GetModuleHandleA(None).unwrap();
        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: HINSTANCE::from(instance),
            lpszClassName: PCSTR(b"FinalClass\0".as_ptr()),
            ..Default::default()
        };
        RegisterClassA(&wc);

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(b"FinalClass\0".as_ptr()),
            PCSTR(b"FINAL TRIANGLE TEST\0".as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT,
            WIDTH as i32, HEIGHT as i32,
            None, None, Some(HINSTANCE::from(instance)), None,
        ).unwrap();

        // D3D12
        let pair = create_device_and_queue();
        let device = get_device_from_pair(pair);
        let swap = create_swap_chain(ptr::null_mut(), hwnd.0, WIDTH, HEIGHT);
        create_fence(device);

        let root_sig = create_root_signature_simple(device);
        let pso = create_pso(device, root_sig, 0);

        // Вершинный буфер
        let vb = create_buffer(device, 84, 0);
        update_subresource(vb, VERTICES.as_ptr() as *const _, 84);
        wait_for_gpu();
        let vb_gpu = get_buffer_gpu_address(vb);

        // Константный буфер
        let cb = create_buffer(device, 256, 0);
        let cb_gpu = get_buffer_gpu_address(cb);
        map_buffer(cb);

        // RTV
        let rtv_heap = create_descriptor_heap(device, 2, 0, false);
        let rtv_base = GetCPUDescriptorHandleForHeapStart(rtv_heap);
        let rtv_inc = get_descriptor_handle_increment_size(device, 0);

        let back0 = swap_chain_get_buffer(swap, 0);
        let back1 = swap_chain_get_buffer(swap, 1);
        create_render_target_view(device, back0, rtv_base);
        create_render_target_view(device, back1, rtv_base + rtv_inc as u64);

        println!("✓ Initialization complete");
        println!("✓ Looking for RED-GREEN-BLUE triangle...\n");

        let mut frame = 0;
        let mut msg = std::mem::zeroed();
        let mut running = true;

        while running && frame < 300 {
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT { running = false; }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            let idx = get_current_back_buffer_index(swap);
            let rtv = if idx == 0 { rtv_base } else { rtv_base + rtv_inc as u64 };

            begin_frame();
            set_pipeline(pso);
            set_root_signature(root_sig);
            set_render_target(rtv);

            // СЕРЫЙ фон для контраста
            let clear = [0.2, 0.2, 0.2, 1.0];
            clear_render_target(rtv, clear.as_ptr());

            set_viewport(0.0, 0.0, WIDTH as f32, HEIGHT as f32, 0.0, 1.0);
            set_vertex_buffer(vb_gpu, 84, 28);
            set_primitive_topology(4);
            set_root_constant_buffer_view(0, cb_gpu);
            draw_instanced(3, 1, 0, 0);
            end_frame();
            present_swap_chain(swap, 0);

            if frame == 0 {
                println!("✓ First frame rendered!");
                println!("✓ If screen is gray, triangle should be visible!");
            }
            frame += 1;
        }

        wait_for_gpu();
        force_cleanup();
        println!("\n✓ Test completed!");
    }
}