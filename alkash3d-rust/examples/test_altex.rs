// examples/test_simple.rs
use alkash3d_rs::*;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcA, PostQuitMessage};

fn main() {
    println!("=== D3D12 Test ===");

    let device = create_device();
    if device.is_null() { eprintln!("No device"); return; }
    println!("✓ Device");

    let queue = create_command_queue(device);
    if queue.is_null() { eprintln!("No queue"); return; }
    println!("✓ Queue");

    let hwnd = create_window();
    if hwnd == 0 { eprintln!("No window"); return; }
    println!("✓ Window");

    let swap = create_swap_chain(queue, hwnd, 800, 600);
    if swap.is_null() { eprintln!("No swap"); return; }
    println!("✓ Swap chain");

    // RTV heap
    let rtv_heap = create_descriptor_heap(device, 2, 0, false);
    let rtv_start = GetCPUDescriptorHandleForHeapStart(rtv_heap);
    let rtv_size = get_rtv_descriptor_size();

    let mut rtvs = [0u64; 2];
    for i in 0..2 {
        let buf = swap_chain_get_buffer(swap, i);
        let rtv = rtv_start + (i as u64 * rtv_size as u64);
        create_render_target_view(device, buf, rtv);
        rtvs[i as usize] = rtv;
    }
    println!("✓ RTVs");

    // Commands
    create_command_allocators(device, 2);
    create_command_list(device);
    create_fence(device);
    println!("✓ Commands");

    // Buffers
    let vertices = create_triangle_vertices();
    let vbuf = create_buffer(device, vertices.len() * 28, b"V\0".as_ptr());
    update_subresource(vbuf, vertices.as_ptr() as *const _, vertices.len() * 28);

    let indices: [u32; 3] = [0, 1, 2];
    let ibuf = create_buffer(device, 12, b"I\0".as_ptr());
    update_subresource(ibuf, indices.as_ptr() as *const _, 12);
    println!("✓ Buffers");

    // PSO
    let root_sig = create_root_signature(device, 0, std::ptr::null(), std::ptr::null(), std::ptr::null());
    let pso = create_simple_pso(device, root_sig);
    if pso.is_null() {
        eprintln!("No PSO - using null pipeline");
    } else {
        println!("✓ PSO created");
    }

    // Render loop
    println!("\nRendering 100 frames...");

    for frame in 0..100 {
        begin_frame();

        let idx = get_frame_index() as usize;
        let clear = [0.1, 0.2, 0.3, 1.0];
        clear_render_target(rtvs[idx % 2], clear.as_ptr());

        set_viewport(0.0, 0.0, 800.0, 600.0, 0.0, 1.0);
        set_scissor_rect(0, 0, 800, 600);

        if !pso.is_null() {
            set_graphics_pipeline(pso);
            set_root_signature(root_sig);

            set_vertex_buffer(get_buffer_gpu_address(vbuf), 3 * 28, 28);
            set_index_buffer(get_buffer_gpu_address(ibuf), 12, 4);

            draw_indexed_instanced(3, 1, 0, 0, 0);
        }

        end_frame();
        wait_for_gpu();
        present_swap_chain(swap, 1);

        if frame % 30 == 0 {
            println!("Frame {}", frame);
        }

        thread::sleep(Duration::from_millis(16));
    }

    println!("Done!");
}

fn create_window() -> usize {
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows_core::*;

    unsafe {
        let inst = GetModuleHandleA(None).unwrap();
        let class = s!("TestWindow");

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassA(&wc);

        let h = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            class,
            s!("D3D12 Test"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, 800, 600,
            None, None, inst, None,
        ).unwrap();

        ShowWindow(h, SW_SHOW);
        h.0 as usize
    }
}

extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            0x0002 => { PostQuitMessage(0); LRESULT(0) }
            _ => DefWindowProcA(h, msg, w, l),
        }
    }
}

#[repr(C)]
struct Vertex {
    pos: [f32; 3],
    color: [f32; 4],
}

fn create_triangle_vertices() -> Vec<Vertex> {
    vec![
        Vertex { pos: [0.0, 0.5, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
        Vertex { pos: [0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
        Vertex { pos: [-0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
    ]
}