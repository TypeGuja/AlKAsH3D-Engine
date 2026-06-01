// src/bin/ball_game.rs
//! Квадрат с пульсирующим цветом (без ошибок)

use alkash3d_rs::*;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain3, DXGI_PRESENT};
use windows::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows::Win32::System::Threading::{CreateEventA, WaitForSingleObject, INFINITE};
use windows_core::PCSTR;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SimpleVertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl SimpleVertex {
    const STRIDE: usize = 28;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ConstantBuffer {
    color_r: f32,
    color_g: f32,
    color_b: f32,
    _padding: f32,
}

static mut RUNNING: bool = true;

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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
    let class_name = b"BallGameClass\0";
    let class_name_ptr = PCSTR(class_name.as_ptr());

    let wc = WNDCLASSA {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: HINSTANCE::from(instance),
        hIcon: HICON::default(),
        hCursor: HCURSOR::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: PCSTR::null(),
        lpszClassName: class_name_ptr,
    };

    RegisterClassA(&wc);

    CreateWindowExA(
        WINDOW_EX_STYLE::default(),
        class_name_ptr,
        PCSTR(b"Alkash3D - Ball Game\0".as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        1024,
        768,
        None,
        None,
        Some(HINSTANCE::from(instance)),
        None,
    ).unwrap()
}

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           BALL GAME - PULSATING COLOR                        ║");
    println!("║                    ESC - exit                                ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    unsafe {
        let hwnd = create_window();
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        println!("✅ Window created");

        let device_ptr = create_device();
        if device_ptr.is_null() {
            eprintln!("❌ Failed to create D3D12 device");
            return;
        }
        let device: ID3D12Device = std::mem::transmute_copy(&device_ptr);
        println!("✅ D3D12 device created");

        let queue_ptr = create_command_queue(device_ptr);
        let queue: ID3D12CommandQueue = std::mem::transmute_copy(&queue_ptr);

        let swap_chain_ptr = create_swap_chain(queue_ptr, hwnd.0 as usize, 1024, 768);
        let swap_chain: IDXGISwapChain3 = std::mem::transmute_copy(&swap_chain_ptr);
        println!("✅ Swap chain created");

        let allocator: ID3D12CommandAllocator = device
            .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
            .expect("Failed to create command allocator");

        let cmd_list: ID3D12GraphicsCommandList = device
            .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
            .expect("Failed to create command list");
        println!("✅ Command list created");

        let fence: ID3D12Fence = device
            .CreateFence(0, D3D12_FENCE_FLAG_NONE)
            .expect("Failed to create fence");
        let fence_event = CreateEventA(None, false, false, None).unwrap();
        let mut fence_value = 0u64;

        let rtv_heap_desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: 2,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };
        let rtv_heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&rtv_heap_desc).unwrap();
        let rtv_handle = rtv_heap.GetCPUDescriptorHandleForHeapStart();

        let back_buffer: ID3D12Resource = swap_chain.GetBuffer(0).expect("Failed to get back buffer");
        device.CreateRenderTargetView(&back_buffer, None, rtv_handle);
        println!("✅ RTV created");

        let root_sig = create_root_signature_simple(device_ptr);
        let pso = create_simple_pso(device_ptr, root_sig);
        let pso_com: ID3D12PipelineState = std::mem::transmute_copy(&pso);
        let root_sig_com: ID3D12RootSignature = std::mem::transmute_copy(&root_sig);
        println!("✅ PSO created");

        // Квадрат из 6 вершин (2 треугольника)
        let vertices = [
            SimpleVertex { position: [-0.2, -0.2, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            SimpleVertex { position: [ 0.2, -0.2, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            SimpleVertex { position: [ 0.2,  0.2, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            SimpleVertex { position: [-0.2, -0.2, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            SimpleVertex { position: [ 0.2,  0.2, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            SimpleVertex { position: [-0.2,  0.2, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        ];

        let vertex_size = vertices.len() * SimpleVertex::STRIDE;
        let vertex_buffer = create_buffer(device_ptr, vertex_size, 0);
        update_subresource(vertex_buffer, vertices.as_ptr() as *const c_void, vertex_size);
        let vertex_buffer_gpu = get_buffer_gpu_address(vertex_buffer);

        let vertex_view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: vertex_buffer_gpu,
            SizeInBytes: vertex_size as u32,
            StrideInBytes: SimpleVertex::STRIDE as u32,
        };
        println!("✅ Vertex buffer created");

        // Constant buffer для цвета (UPLOAD)
        let const_buffer = create_buffer(device_ptr, 64, 0);
        let const_buffer_gpu = get_buffer_gpu_address(const_buffer);
        println!("✅ Constant buffer created");

        println!("\n🎮 GAME STARTED! Watch the color pulse!\n");

        let mut frame = 0;
        let start_time = Instant::now();
        let mut msg = MSG::default();
        let mut time = 0.0f32;

        while RUNNING {
            let elapsed = start_time.elapsed().as_secs_f32();
            time = elapsed;

            // Пульсирующий цвет от красного к синему
            let r = (time.sin() * 0.5 + 0.5) * 0.8 + 0.2;
            let g = ((time + 2.0).sin() * 0.5 + 0.5) * 0.8 + 0.2;
            let b = ((time + 4.0).sin() * 0.5 + 0.5) * 0.8 + 0.2;

            // Обновляем constant buffer с новым цветом
            let cb_data = ConstantBuffer { color_r: r, color_g: g, color_b: b, _padding: 0.0 };
            update_subresource(const_buffer, &cb_data as *const _ as *const c_void, 64);

            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    RUNNING = false;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            allocator.Reset();
            cmd_list.Reset(&allocator, &pso_com);

            let barrier = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: ManuallyDrop::new(Some(back_buffer.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_PRESENT,
                        StateAfter: D3D12_RESOURCE_STATE_RENDER_TARGET,
                    }),
                },
            };
            cmd_list.ResourceBarrier(&[barrier]);

            cmd_list.OMSetRenderTargets(1, Some(&rtv_handle), false, None);

            let clear_color = [0.05f32, 0.05f32, 0.1f32, 1.0f32];
            cmd_list.ClearRenderTargetView(rtv_handle, &clear_color, None);

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: 1024.0,
                Height: 768.0,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);

            let scissor = RECT { left: 0, top: 0, right: 1024, bottom: 768 };
            cmd_list.RSSetScissorRects(&[scissor]);

            cmd_list.SetPipelineState(&pso_com);
            cmd_list.SetGraphicsRootSignature(&root_sig_com);
            cmd_list.IASetVertexBuffers(0, Some(&[vertex_view]));
            cmd_list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            cmd_list.SetGraphicsRootConstantBufferView(0, const_buffer_gpu);

            cmd_list.DrawInstanced(6, 1, 0, 0);

            let barrier_present = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: ManuallyDrop::new(Some(back_buffer.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_RENDER_TARGET,
                        StateAfter: D3D12_RESOURCE_STATE_PRESENT,
                    }),
                },
            };
            cmd_list.ResourceBarrier(&[barrier_present]);

            cmd_list.Close().unwrap();

            let cmd_list_base: ID3D12CommandList = cmd_list.clone().into();
            let cmd_lists = &[Some(cmd_list_base)];
            queue.ExecuteCommandLists(cmd_lists);

            fence_value += 1;
            queue.Signal(&fence, fence_value).unwrap();
            if fence.GetCompletedValue() < fence_value {
                fence.SetEventOnCompletion(fence_value, fence_event).unwrap();
                WaitForSingleObject(fence_event, INFINITE);
            }

            swap_chain.Present(1, DXGI_PRESENT(0)).ok();

            frame += 1;

            if frame % 60 == 0 {
                println!("Frame {}: {:.1}s, Color: RGB({:.2}, {:.2}, {:.2})",
                         frame, elapsed, r, g, b);
            }

            sleep(Duration::from_millis(16));
        }

        println!("\n📊 Total frames: {}", frame);
        println!("🛑 Shutting down...");

        wait_for_gpu();
        force_cleanup();

        println!("✅ Done!");
    }
}