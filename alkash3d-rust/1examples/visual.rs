// 1examples/visual_test.rs
use alkash3d_rs::*;
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::time::Instant;
use windows_core::Event;
use winit::{
    event::{Event, WindowEvent, ElementState, KeyEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::WindowBuilder,
    raw_window_handle::{HasRawWindowHandle, RawWindowHandle},
};

fn main() {
    println!("🚀 Alkash3D Visual Test");
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("Alkash3D Visual Test")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .unwrap();

    // Получаем HWND
    #[cfg(target_os = "windows")]
    let hwnd = match window.raw_window_handle() {
        RawWindowHandle::Win32(handle) => handle.hwnd as usize,
        _ => panic!("Expected Win32 window handle"),
    };

    // Инициализация D3D12
    unsafe {
        let device = create_device();
        assert!(!device.is_null(), "Failed to create device");
        println!("✅ Device created");

        let queue = create_command_queue(device);
        assert!(!queue.is_null(), "Failed to create queue");
        println!("✅ Command queue created");

        let swap_chain = create_swap_chain(queue, hwnd, 800, 600);
        assert!(!swap_chain.is_null(), "Failed to create swap chain");
        println!("✅ Swap chain created");

        // RTV heap
        let rtv_heap = create_descriptor_heap(device, 2, 0, false);
        let rtv_handle = GetCPUDescriptorHandleForHeapStart(rtv_heap);
        let rtv_size = get_rtv_descriptor_size();

        // Создаём RTV для буферов swap chain
        for i in 0..2 {
            let buffer = swap_chain_get_buffer(swap_chain, i);
            let handle = rtv_handle + (i as u64 * rtv_size as u64);
            create_render_target_view(device, buffer, handle);
            println!("✅ RTV {} created", i);
        }

        // Root signature и PSO
        let root_sig = create_root_signature(device);
        let pso = create_pso(device, root_sig, 0);
        println!("✅ Root signature and PSO created");

        // Fence
        create_fence(device);
        println!("✅ Fence created");

        // Вершинный буфер (треугольник)
        #[repr(C)]
        struct SimpleVertex {
            pos: [f32; 3],
            color: [f32; 4],
        }

        let vertices = vec![
            SimpleVertex { pos: [0.0, 0.5, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
            SimpleVertex { pos: [0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
            SimpleVertex { pos: [-0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
        ];

        let vb_size = std::mem::size_of_val(&vertices[..]);
        let vb = create_buffer(device, vb_size, 1);
        let vb_upload = create_buffer(device, vb_size, 0);
        update_subresource(vb_upload, vertices.as_ptr() as *const c_void, vb_size);
        println!("✅ Vertex buffer created ({} bytes)", vb_size);

        // Constant buffer (MVP матрица)
        #[repr(C)]
        struct MVP {
            mvp: [[f32; 4]; 4],
        }

        let mvp = MVP {
            mvp: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };

        let cb = create_buffer(device, std::mem::size_of::<MVP>(), 0);
        update_subresource(cb, &mvp as *const MVP as *const c_void, std::mem::size_of::<MVP>());
        let cb_gpu_addr = get_buffer_gpu_address(cb);
        println!("✅ Constant buffer created");

        // Основной цикл
        let mut frame_count = 0u64;
        let start_time = Instant::now();

        event_loop.run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);

            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => elwt.exit(),
                    WindowEvent::KeyboardInput {
                        event: KeyEvent {
                            logical_key: Key::Named(NamedKey::Escape),
                            state: ElementState::Pressed,
                            ..
                        },
                        ..
                    } => elwt.exit(),
                    WindowEvent::RedrawRequested => {
                        // ========== РЕНДЕРИНГ ==========
                        let cmd_list = begin_frame();

                        if !cmd_list.is_null() {
                            let back_buffer_idx = get_current_back_buffer_index(swap_chain) as u64;
                            let current_rtv = rtv_handle + (back_buffer_idx * rtv_size as u64);

                            // Очистка (тёмно-синий фон)
                            let clear_color = [0.1f32, 0.2f32, 0.4f32, 1.0f32];
                            clear_render_target(current_rtv, clear_color.as_ptr());

                            // Устанавливаем RTV
                            set_render_targets(current_rtv, 1);

                            // Pipeline
                            set_root_signature(root_sig);
                            set_graphics_pipeline(pso);

                            // Viewport + Scissor
                            set_viewport(0.0, 0.0, 800.0, 600.0, 0.0, 1.0);
                            set_scissor_rect(0, 0, 800, 600);

                            // Vertex buffer
                            let vb_gpu_addr = get_buffer_gpu_address(vb);
                            set_vertex_buffer(vb_gpu_addr, vb_size as u32, 28); // 28 = sizeof(SimpleVertex)

                            // Constant buffer
                            set_root_constant_buffer_view(0, cb_gpu_addr);

                            // Рисуем
                            set_primitive_topology(4); // TriangleList
                            draw_instanced(3, 1, 0, 0);

                            // Завершаем кадр
                            if end_frame() {
                                present_swap_chain(swap_chain, 1);

                                frame_count += 1;
                                if frame_count % 60 == 0 {
                                    let elapsed = start_time.elapsed().as_secs_f64();
                                    if elapsed > 0.0 {
                                        println!("📊 Frame: {}, Avg FPS: {:.1}",
                                                 frame_count, frame_count as f64 / elapsed);
                                    }
                                }
                            } else {
                                eprintln!("❌ Frame {} failed!", frame_count);
                            }
                        }
                    }
                    _ => {}
                },
                Event::AboutToWait => {
                    window.request_redraw();
                }
                _ => {}
            }
        }).unwrap();
    }
}