use alkash3d_rs::*;
use std::ffi::CString;
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::core::PCSTR;
use windows_core::Interface;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SimpleVertex {
    position: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ConstantBuffer {
    mvp: [[f32; 4]; 4],
}

fn main() {
    println!("=== AlKAsH3D Engine - Full Test with Debug ===\n");

    unsafe {
        // Создаем окно
        let hinstance = GetModuleHandleA(None).unwrap();
        let class_name = CString::new("AlKAsH3DWindow").unwrap();
        let window_name = CString::new("AlKAsH3D - Triangle Demo (Debug)").unwrap();

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE::from(hinstance),
            lpszClassName: PCSTR(class_name.as_ptr() as *const u8),
            ..Default::default()
        };

        RegisterClassA(&wc);

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(class_name.as_ptr() as *const u8),
            PCSTR(window_name.as_ptr() as *const u8),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, 800, 600,
            None, None, hinstance, None,
        ).unwrap();

        println!("[DEBUG] Window created: 0x{:X}", hwnd.0 as usize);
        ShowWindow(hwnd, SW_SHOW);

        // D3D12 init
        println!("\n[DEBUG] --- Initializing D3D12 ---");

        let device = create_device();
        println!("[DEBUG] Device: {:p}", device);

        let queue = create_command_queue(device);
        println!("[DEBUG] Queue: {:p}", queue);

        let swap_chain = create_swap_chain(queue, hwnd.0 as usize, 800, 600);
        println!("[DEBUG] SwapChain: {:p}", swap_chain);

        // Root signature и PSO
        let root_sig = create_root_signature_simple(device);
        println!("[DEBUG] RootSig: {:p}", root_sig);

        let pso = create_pso(device, root_sig, 0);
        println!("[DEBUG] PSO: {:p}", pso);

        // Vertex buffer
        let vertices: [SimpleVertex; 3] = [
            SimpleVertex { position: [0.0, 0.5, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
            SimpleVertex { position: [0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
            SimpleVertex { position: [-0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
        ];

        let vb_size = std::mem::size_of_val(&vertices);
        let vb = create_buffer(device, vb_size, 0);
        update_buffer(vb, vertices.as_ptr() as *const _, vb_size, 0);
        let vb_gpu = get_buffer_gpu_address(vb);
        println!("[DEBUG] VB size: {}, GPU: 0x{:X}", vb_size, vb_gpu);

        // Constant buffer
        let cb_size = std::mem::size_of::<ConstantBuffer>();
        let cb = create_buffer(device, cb_size, 0);
        let cb_gpu = get_buffer_gpu_address(cb);
        println!("[DEBUG] CB size: {}, GPU: 0x{:X}", cb_size, cb_gpu);

        // Fence
        create_fence(device);
        println!("[DEBUG] Fence created");

        // RTV heap
        let rtv_heap = create_descriptor_heap(device, 2, 0, false);
        let rtv_size = get_rtv_descriptor_size() as usize;
        let rtv_start = GetCPUDescriptorHandleForHeapStart(rtv_heap) as usize;
        println!("[DEBUG] RTV heap: size={}, start=0x{:X}", rtv_size, rtv_start);

        // Минимальный тестовый цикл в game.rs

        println!("\n[DEBUG] === Entering render loop ===\n");

        let start_time = Instant::now();
        let mut msg = MSG::default();
        let mut frame_count = 0u64;
        let mut running = true;

        while running {
            // Обработка сообщений
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    println!("[DEBUG] WM_QUIT received");
                    running = false;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if !running { break; }

            frame_count += 1;

            println!("\n[DEBUG] === Frame {} START ===", frame_count);

            // begin_frame
            println!("[DEBUG] Calling begin_frame...");
            let cmd_list = begin_frame();
            if cmd_list.is_null() {
                println!("[DEBUG] begin_frame failed!");
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            println!("[DEBUG] begin_frame OK");

            // Получаем back buffer
            println!("[DEBUG] Getting back buffer...");
            let (back_buffer_idx, back_buffer) = {
                let state = match STATE.lock() {
                    Ok(s) => s,
                    Err(_) => {
                        println!("[DEBUG] Failed to lock STATE");
                        end_frame();
                        continue;
                    }
                };

                let swap = match state.swap_chain.as_ref() {
                    Some(s) => s,
                    None => {
                        println!("[DEBUG] No swap chain");
                        end_frame();
                        continue;
                    }
                };

                let idx = swap.GetCurrentBackBufferIndex();
                println!("[DEBUG] Back buffer index: {}", idx);

                match swap.GetBuffer::<ID3D12Resource>(idx) {
                    Ok(b) => (idx, b),
                    Err(e) => {
                        println!("[DEBUG] GetBuffer failed: {:?}", e);
                        end_frame();
                        continue;
                    }
                }
            };
            println!("[DEBUG] Got back buffer {}", back_buffer_idx);

            // RTV handle
            let rtv_handle = (rtv_start + (back_buffer_idx as usize) * rtv_size) as u64;
            println!("[DEBUG] RTV handle: 0x{:X}", rtv_handle);

            create_render_target_view(device, back_buffer.as_raw() as *mut _, rtv_handle);
            println!("[DEBUG] RTV created");

            // Очистка
            let clear_color = [0.1f32, 0.15f32, 0.25f32, 1.0f32];
            println!("[DEBUG] Clearing render target...");
            if !clear_render_target(rtv_handle, clear_color.as_ptr()) {
                println!("[DEBUG] clear_render_target failed!");
            }

            println!("[DEBUG] Setting render targets...");
            if !set_render_targets(rtv_handle, 1) {
                println!("[DEBUG] set_render_targets failed!");
            }

            // Viewport
            set_viewport(0.0, 0.0, 800.0, 600.0, 0.0, 1.0);
            set_scissor_rect(0, 0, 800, 600);
            println!("[DEBUG] Viewport set");

            // Pipeline (ПРОПУСКАЕМ для теста)
            /*
            set_graphics_pipeline(pso);
            set_root_signature(root_sig);
            // ... и т.д.
            */

            println!("[DEBUG] Calling end_frame...");
            if !end_frame() {
                println!("[DEBUG] end_frame failed!");
                continue;
            }
            println!("[DEBUG] end_frame OK");

            println!("[DEBUG] Calling present...");
            if !present_swap_chain(swap_chain, 1) {
                println!("[DEBUG] present failed!");
            }
            println!("[DEBUG] present OK");

            println!("[DEBUG] === Frame {} END ===\n", frame_count);

            if frame_count >= 200 {
                println!("[DEBUG] Test completed 5 frames, exiting...");
                break;
            }
        }
    }
    println!("✅ Test completed successfully!");
}

unsafe extern "system" fn wndproc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            println!("[DEBUG] WM_DESTROY");
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 27 { // ESC
                println!("[DEBUG] ESC pressed");
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}