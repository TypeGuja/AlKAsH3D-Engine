// src/bin/main.rs
#![allow(never_type_fallback_flowing_into_unsafe)]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_WINDOW, HBRUSH};
use alkash3d_rs::*;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

#[repr(C)]
#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 4],
    color: [f32; 4],
}

// Вершинный шейдер с отладочным выводом через системные значения
const VERTEX_SHADER_SOURCE: &str = r#"
struct VS_INPUT {
    float4 pos : POSITION;
    float4 color : COLOR;
};

struct VS_OUTPUT {
    float4 pos : SV_POSITION;
    float4 color : COLOR;
};

VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.pos = input.pos;
    output.color = input.color;
    return output;
}
"#;

// Пиксельный шейдер - всегда возвращает ярко-белый цвет для теста
const PIXEL_SHADER_SOURCE: &str = r#"
struct PS_INPUT {
    float4 pos : SV_POSITION;
    float4 color : COLOR;
};

float4 main(PS_INPUT input) : SV_TARGET {
    // Для отладки - возвращаем ярко-белый цвет
    return float4(1.0, 1.0, 1.0, 1.0);
}
"#;

macro_rules! debug_print {
    ($($arg:tt)*) => {
        println!("[DEBUG] {}", format!($($arg)*));
    };
}

fn main() -> Result<()> {
    println!("==========================================");
    println!("Alkash3D Engine v{} - Starting DirectX 12...", alkash3d_rs::VERSION);
    println!("==========================================");

    unsafe {
        debug_print!("Создание окна...");
        let hinstance = GetModuleHandleA(None)?;
        let window_class = "ALKASH3D_WINDOW";

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCSTR(window_class.as_ptr()),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as _),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };

        RegisterClassA(&wc);
        debug_print!("Класс окна зарегистрирован");

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            PCSTR(window_class.as_ptr()),
            PCSTR(b"Alkash3D Engine - DirectX 12\0".as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH as i32,
            WINDOW_HEIGHT as i32,
            None,
            None,
            Some(HINSTANCE::from(hinstance)),
            None,
        )?;

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        debug_print!("Окно создано, HWND: {:?}", hwnd);

        println!("\n=== INITIALIZING DIRECTX 12 ===\n");

        debug_print!("D3D12Device::create()...");
        D3D12Device::create()?;
        debug_print!("✓ Device created");

        debug_print!("CommandQueue::create()...");
        CommandQueue::create()?;
        debug_print!("✓ Command queue created");

        debug_print!("SwapChain::create()...");
        SwapChain::create(hwnd.0 as isize, WINDOW_WIDTH, WINDOW_HEIGHT, 2)?;
        debug_print!("✓ Swap chain created");

        debug_print!("CommandList::create_allocators(2)...");
        CommandList::create_allocators(2)?;
        debug_print!("✓ Allocators created");

        debug_print!("create_fence()...");
        let fence = create_fence()?;
        STATE.lock().unwrap().fence = Some(fence.clone());
        debug_print!("✓ Fence created");

        debug_print!("DescriptorHeap::create_rtv_heap(2)...");
        let rtv_heap = DescriptorHeap::create_rtv_heap(2)?;
        debug_print!("✓ RTV heap created");

        debug_print!("DescriptorHeap::create_dsv_heap(1)...");
        let dsv_heap = DescriptorHeap::create_dsv_heap(1)?;
        debug_print!("✓ DSV heap created");

        let rtv_size;
        let dsv_size;
        {
            let state = STATE.lock().unwrap();
            rtv_size = state.rtv_descriptor_size;
            dsv_size = state.dsv_descriptor_size;
            debug_print!("RTV size: {}, DSV size: {}", rtv_size, dsv_size);
        }

        debug_print!("Получение swap_chain и device...");
        let swap_chain = STATE.lock().unwrap().swap_chain.as_ref().unwrap().clone();
        let device = STATE.lock().unwrap().device.as_ref().unwrap().clone();
        debug_print!("✓ Swap chain и device получены");

        debug_print!("Создание RTV хендлов...");
        let mut rtv_handles = Vec::new();
        for i in 0..2 {
            let back_buffer: ID3D12Resource = swap_chain.GetBuffer(i)?;
            let rtv_handle = DescriptorHeap::get_cpu_handle(&rtv_heap, i, rtv_size);
            device.CreateRenderTargetView(&back_buffer, None, rtv_handle);
            rtv_handles.push(rtv_handle);
            debug_print!("  ✓ RTV {} создан", i);
        }
        debug_print!("✓ RTVs created");

        debug_print!("Создание depth stencil...");
        let depth_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: WINDOW_WIDTH as u64,
            Height: WINDOW_HEIGHT,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } },
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };

        let mut depth_stencil: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &depth_desc,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            Some(&clear_value),
            &mut depth_stencil,
        )?;
        let depth_stencil = depth_stencil.unwrap();
        debug_print!("✓ Depth stencil resource создан");

        let dsv_handle = DescriptorHeap::get_cpu_handle(&dsv_heap, 0, dsv_size);
        device.CreateDepthStencilView(&depth_stencil, None, dsv_handle);
        debug_print!("✓ Depth stencil view создан");

        debug_print!("Создание вершинного буфера...");
        // Используем треугольник, который гарантированно виден
        // Изменяем координаты: делаем треугольник больше и в центре
        let vertices = [
            Vertex { position: [-0.8, -0.8, 0.5, 1.0], color: [1.0, 0.0, 0.0, 1.0] },  // красный
            Vertex { position: [0.0, 0.8, 0.5, 1.0], color: [0.0, 1.0, 0.0, 1.0] },    // зелёный
            Vertex { position: [0.8, -0.8, 0.5, 1.0], color: [0.0, 0.0, 1.0, 1.0] },   // синий
        ];

        println!("[MAIN] Vertex data:");
        for (i, v) in vertices.iter().enumerate() {
            println!("  Vertex {}: pos=({:.1}, {:.1}, {:.1}), color=({:.1}, {:.1}, {:.1}, {:.1})",
                     i, v.position[0], v.position[1], v.position[2],
                     v.color[0], v.color[1], v.color[2], v.color[3]);
        }

        let vertex_buffer_size = (vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        let upload_heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            ..Default::default()
        };

        let buffer_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: vertex_buffer_size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let mut vertex_buffer: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(
            &upload_heap_props,
            D3D12_HEAP_FLAG_NONE,
            &buffer_desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut vertex_buffer,
        )?;
        let vertex_buffer = vertex_buffer.unwrap();
        println!("[MAIN] Vertex buffer created, size: {} bytes", vertex_buffer_size);

        let mut mapped = std::ptr::null_mut();
        let hr = vertex_buffer.Map(0, None, Some(&mut mapped));
        if hr.is_err() {
            println!("[MAIN] ERROR: Map failed with HRESULT: {:?}", hr);
        } else if !mapped.is_null() {
            std::ptr::copy_nonoverlapping(vertices.as_ptr() as *const u8, mapped as *mut u8, vertex_buffer_size as usize);
            println!("[MAIN] Vertex data copied to buffer at address: {:p}", mapped);
        } else {
            println!("[MAIN] ERROR: Mapped pointer is null!");
        }
        vertex_buffer.Unmap(0, None);

        let vertex_gpu_addr = vertex_buffer.GetGPUVirtualAddress();
        println!("[MAIN] Vertex buffer GPU address: {:?}", vertex_gpu_addr);

        debug_print!("Компиляция шейдеров...");
        let vs = ShaderBlob::compile(VERTEX_SHADER_SOURCE, "vs_5_0", "main")?;
        let ps = ShaderBlob::compile(PIXEL_SHADER_SOURCE, "ps_5_0", "main")?;
        debug_print!("✓ Шейдеры скомпилированы (VS: {} байт, PS: {} байт)", vs.size(), ps.size());

        debug_print!("Создание корневой сигнатуры...");
        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 0,
            pParameters: std::ptr::null(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut signature_serialized: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3D12SerializeRootSignature(
            &root_signature_desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut signature_serialized,
            Some(&mut error_blob),
        );

        if hr.is_err() {
            if let Some(error_blob) = error_blob {
                let error = std::slice::from_raw_parts(
                    error_blob.GetBufferPointer() as *const u8,
                    error_blob.GetBufferSize(),
                );
                let error_str = String::from_utf8_lossy(error);
                eprintln!("Root signature serialization error:\n{}", error_str);
            }
            debug_print!("Ошибка сериализации корневой сигнатуры: {:?}", hr);
            return Err(Error::from_hresult(HRESULT::from(hr)));
        }
        let signature_serialized = signature_serialized.unwrap();
        println!("[MAIN] Root signature serialized, size: {} bytes", signature_serialized.GetBufferSize());

        let blob_data = std::slice::from_raw_parts(
            signature_serialized.GetBufferPointer() as *const u8,
            signature_serialized.GetBufferSize(),
        );

        let root_signature: ID3D12RootSignature = device.CreateRootSignature(0, blob_data)?;
        debug_print!("✓ Корневая сигнатура создана");

        debug_print!("Создание PSO...");
        let pso: ID3D12PipelineState = PipelineState::create_graphics(
            &vs, &ps, &root_signature,
            std::mem::size_of::<Vertex>() as u32,
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_FORMAT_D32_FLOAT,
        )?;
        debug_print!("✓ PSO создан");

        println!("\n=== RENDER LOOP STARTING ===\n");

        let mut running = true;
        let mut frame_count = 0u32;
        let mut fence_value = 0u64;

        while running {
            let mut msg = MSG::default();
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    running = false;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if !running { break; }

            let frame_index = swap_chain.GetCurrentBackBufferIndex();

            if frame_count % 60 == 0 {
                println!("\n=== FRAME {} (BackBuffer {}) ===", frame_count, frame_index);
            }

            // Сброс allocator
            let allocator = {
                let state = STATE.lock().unwrap();
                state.command_allocators[frame_index as usize].as_ref().unwrap().clone()
            };
            allocator.Reset();

            // Создание command list
            let cmd_list: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;

            // Установка render targets
            let rtv = rtv_handles[frame_index as usize];
            cmd_list.OMSetRenderTargets(1, Some(&rtv), false, Some(&dsv_handle));

            // Очистка цветного буфера (ярко-красный фон для теста)
            let clear_color = [1.0, 0.0, 0.0, 1.0];  // Ярко-красный фон
            cmd_list.ClearRenderTargetView(rtv, &clear_color, None);
            cmd_list.ClearDepthStencilView(dsv_handle, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

            // Установка PSO и root signature
            cmd_list.SetPipelineState(Some(&pso));
            cmd_list.SetGraphicsRootSignature(Some(&root_signature));

            // Установка viewport и scissor rect
            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: WINDOW_WIDTH as f32,
                Height: WINDOW_HEIGHT as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);

            let scissor_rect = RECT {
                left: 0,
                top: 0,
                right: WINDOW_WIDTH as i32,
                bottom: WINDOW_HEIGHT as i32,
            };
            cmd_list.RSSetScissorRects(&[scissor_rect]);

            // Установка vertex buffer
            let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: vertex_gpu_addr,
                SizeInBytes: vertex_buffer_size as u32,
                StrideInBytes: std::mem::size_of::<Vertex>() as u32,
            };
            cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
            cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

            // Draw
            cmd_list.DrawInstanced(3, 1, 0, 0);

            if frame_count % 60 == 0 {
                println!("[FRAME] DrawInstanced(3) called - Triangle should be white on red background");
            }

            // Close и execute
            cmd_list.Close()?;

            let queue = STATE.lock().unwrap().command_queue.as_ref().unwrap().clone();
            let cmd_lists = [Some(cmd_list.into())];
            queue.ExecuteCommandLists(&cmd_lists);

            // Present
            let _ = swap_chain.Present(1, DXGI_PRESENT(0));

            // Fence synchronization
            fence_value += 1;
            queue.Signal(&fence, fence_value)?;

            while fence.GetCompletedValue() < fence_value {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }

            frame_count += 1;

            if frame_count == 1 {
                println!("\n*** FIRST FRAME COMPLETED ***");
                println!("*** Screen should be RED with a WHITE triangle ***");
                println!("*** If you see RED screen, triangle is not rendering ***");
                println!("*** If you see WHITE triangle, rendering works! ***\n");
            }

            if frame_count == 60 {
                println!("[INFO] 60 frames rendered - check if triangle appears");
            }
        }

        println!("\n=== ENGINE SHUTDOWN ===\n");
        println!("Engine shutdown complete");
    }

    Ok(())
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if wparam.0 == 0x1B {  // ESC key
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            _ => DefWindowProcA(hwnd, msg, wparam, lparam),
        }
    }
}