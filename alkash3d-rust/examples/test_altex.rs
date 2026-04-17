// examples/test_altex_load.rs - Финальная рабочая версия
use alkash3d_rs::*;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HINSTANCE};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_D32_FLOAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows_core::*;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SimpleVertex {
    position: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MVP {
    mvp: [[f32; 4]; 4],
}

static RUNNING: AtomicBool = AtomicBool::new(true);

fn create_root_signature_local(device_ptr: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
    unsafe {
        println!("      Creating root signature...");
        let device: ID3D12Device = std::mem::transmute_copy(&device_ptr);

        let root_parameters = [D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
        }];

        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 1,
            pParameters: root_parameters.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut serialized: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3D12SerializeRootSignature(
            &root_desc,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut serialized,
            Some(&mut error_blob),
        );

        if hr.is_err() {
            return std::ptr::null_mut();
        }

        if let Some(blob) = serialized {
            let data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize()
            );

            match device.CreateRootSignature::<ID3D12RootSignature>(0, data) {
                Ok(rs) => {
                    let ptr = rs.as_raw();
                    std::mem::forget(rs);
                    ptr as *mut std::ffi::c_void
                }
                Err(_) => std::ptr::null_mut(),
            }
        } else {
            std::ptr::null_mut()
        }
    }
}

fn create_depth_buffer(device_ptr: *mut std::ffi::c_void, width: u32, height: u32) -> *mut std::ffi::c_void {
    unsafe {
        let device: ID3D12Device = std::mem::transmute_copy(&device_ptr);

        let depth_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 {
                DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 }
            },
        };

        let mut depth_buffer: Option<ID3D12Resource> = None;
        match device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &depth_desc,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            Some(&clear_value),
            &mut depth_buffer,
        ) {
            Ok(_) => {
                if let Some(buf) = depth_buffer {
                    let ptr = buf.as_raw();
                    std::mem::forget(buf);
                    ptr as *mut std::ffi::c_void
                } else {
                    std::ptr::null_mut()
                }
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}

fn main() {
    println!("AlKAsH3D Engine - Loading Cube from .altex\n");

    // Загружаем .altex файл
    println!("Loading cube.altex...");
    let altex = match AltexFile::load("cube.altex") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to load cube.altex: {}", e);
            eprintln!("Please run create_cube_altex example first!");
            return;
        }
    };

    println!("Loaded: {} objects, {} meshes, {} vertices, {} indices",
             altex.objects.len(), altex.meshes.len(), altex.vertices.len(), altex.indices.len());

    let mesh = &altex.meshes[0];
    let mesh_name = altex.get_string(mesh.name_id);
    println!("First mesh: '{}' - {} vertices, {} indices",
             mesh_name, mesh.vertex_count, mesh.index_count);

    let vertices: Vec<SimpleVertex> = altex.vertices[mesh.vertex_offset as usize..(mesh.vertex_offset + mesh.vertex_count) as usize]
        .iter()
        .map(|v| SimpleVertex {
            position: v.position,
            color: v.color,
        })
        .collect();

    let indices: Vec<u32> = altex.indices[mesh.index_offset as usize..(mesh.index_offset + mesh.index_count) as usize]
        .to_vec();

    println!("Extracted {} vertices, {} indices", vertices.len(), indices.len());

    // Device
    let device = create_device();
    if device.is_null() { eprintln!("Failed to create device"); return; }
    println!("Device created");

    // Command Queue
    let queue = create_command_queue(device);
    if queue.is_null() { eprintln!("Failed to create command queue"); return; }
    println!("Command queue created");

    // Window
    let hwnd = create_window(800, 600, "Altex Cube Test");
    if hwnd == 0 { eprintln!("Failed to create window"); return; }
    println!("Window created");

    // Swap Chain
    let swap = create_swap_chain(queue, hwnd, 800, 600);
    if swap.is_null() { eprintln!("Failed to create swap chain"); return; }
    println!("Swap chain created");

    // RTV
    let rtv_heap = create_descriptor_heap(device, 2, 0, false);
    let rtv_start = GetCPUDescriptorHandleForHeapStart(rtv_heap);
    let rtv_size = get_rtv_descriptor_size();

    let mut rtvs = [0u64; 2];
    let mut back_buffers = [std::ptr::null_mut(); 2];

    for i in 0..2 {
        let buf = swap_chain_get_buffer(swap, i as u32);
        back_buffers[i] = buf;
        let rtv = rtv_start + (i as u64 * rtv_size as u64);
        create_render_target_view(device, buf, rtv);
        rtvs[i as usize] = rtv;
    }
    println!("RTVs created");

    // Depth Buffer
    let depth_heap = create_descriptor_heap(device, 1, 1, false);
    let depth_dsv = GetCPUDescriptorHandleForHeapStart(depth_heap);
    let depth_buffer = create_depth_buffer(device, 800, 600);
    if depth_buffer.is_null() { eprintln!("Failed to create depth buffer"); return; }
    create_depth_stencil_view(device, depth_buffer, depth_dsv);
    println!("Depth buffer created");

    // Command System
    create_command_allocators(device, 3);
    create_command_list(device);
    create_fence(device);
    println!("Command system ready");

    // Geometry buffers
    let vertices_size = vertices.len() * std::mem::size_of::<SimpleVertex>();
    let vbuf = create_buffer(device, vertices_size, std::ptr::null());
    update_subresource(vbuf, vertices.as_ptr() as _, vertices_size);

    let indices_size = indices.len() * 4;
    let ibuf = create_buffer(device, indices_size, std::ptr::null());
    update_subresource(ibuf, indices.as_ptr() as _, indices_size);

    let cb_size = std::mem::size_of::<MVP>();
    let cb = create_buffer(device, cb_size, std::ptr::null());
    let mvp = MVP { mvp: [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ] };
    update_subresource(cb, &mvp as *const _ as _, cb_size);
    println!("Geometry buffers created");

    // PSO
    let root_sig = create_root_signature_local(device);
    let pso = create_simple_pso(device, root_sig);
    if pso.is_null() { eprintln!("Failed to create PSO"); return; }
    println!("PSO created");

    println!("\nRENDERING - Press ESC to exit\n");

    let mut frame_count = 0u64;
    let start_time = std::time::Instant::now();

    // Настройки камеры - ПОДБЕРИ ЭТИ ЗНАЧЕНИЯ
    let scale = 0.3;      // Размер куба (0.2 - 0.5)
    let distance = 1.5;   // Расстояние от камеры (1.0 - 3.0)

    while RUNNING.load(Ordering::Relaxed) {
        if !process_window_messages() {
            break;
        }

        let elapsed = start_time.elapsed().as_secs_f32();
        let angle_y = elapsed * 1.5;
        let angle_x = elapsed * 0.7;

        let mut rotated_vertices = vertices.clone();
        for v in &mut rotated_vertices {
            let x = v.position[0];
            let y = v.position[1];
            let z = v.position[2];

            // Вращение по Y
            let cos_y = angle_y.cos();
            let sin_y = angle_y.sin();
            let x1 = x * cos_y + z * sin_y;
            let z1 = z * cos_y - x * sin_y;

            // Вращение по X
            let cos_x = angle_x.cos();
            let sin_x = angle_x.sin();
            let y2 = y * cos_x - z1 * sin_x;
            let z2 = z1 * cos_x + y * sin_x;

            // НЕ ДОБАВЛЯЕМ НИКАКИХ scale И distance
            v.position = [x1, y2, z2];
        }

        update_subresource(vbuf, rotated_vertices.as_ptr() as _, vertices_size);

        begin_frame();

        let frame_idx = get_frame_index() as usize;
        let rtv = rtvs[frame_idx % 2];
        let back_buffer = back_buffers[frame_idx % 2];

        transition_resource(back_buffer, 0, 4);

        let clear_color = [0.1f32, 0.15, 0.25, 1.0];
        clear_render_target(rtv, clear_color.as_ptr());
        clear_depth_stencil(depth_dsv, 1.0, 0);
        set_render_targets_with_depth(rtv, depth_dsv, 1);

        set_viewport(0.0, 0.0, 800.0, 600.0, 0.0, 1.0);
        set_scissor_rect(0, 0, 800, 600);

        set_graphics_pipeline(pso);
        set_root_signature(root_sig);

        set_vertex_buffer(get_buffer_gpu_address(vbuf), vertices_size as u32, std::mem::size_of::<SimpleVertex>() as u32);
        set_index_buffer(get_buffer_gpu_address(ibuf), indices_size as u32, 4);
        set_root_constant_buffer_view(0, get_buffer_gpu_address(cb));

        draw_indexed_instanced(indices.len() as u32, 1, 0, 0, 0);

        transition_resource(back_buffer, 4, 0);

        end_frame();
        wait_for_gpu();
        present_swap_chain(swap, 1);

        frame_count += 1;

        if frame_count % 60 == 0 {
            println!("Frame: {}", frame_count);
        }

        thread::sleep(Duration::from_millis(16));
    }

    println!("\nDone. Total frames: {}", frame_count);
}

fn create_window(width: i32, height: i32, title: &str) -> usize {
    unsafe {
        let inst = GetModuleHandleA(None).unwrap();
        let class_name = s!("AlKAsH3DWindowClass");

        let wc = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(inst.0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: HBRUSH::default(),
            lpszClassName: class_name,
            ..Default::default()
        };

        RegisterClassA(&wc);

        let title_cstr = std::ffi::CString::new(title).unwrap();

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            class_name,
            PCSTR(title_cstr.as_ptr() as *const u8),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT, CW_USEDEFAULT, width, height,
            None, None, HINSTANCE(inst.0), None,
        ).unwrap();

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        hwnd.0 as usize
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CLOSE | WM_DESTROY => {
                RUNNING.store(false, Ordering::Relaxed);
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == 27 => {
                RUNNING.store(false, Ordering::Relaxed);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcA(hwnd, msg, wparam, lparam)
        }
    }
}

fn process_window_messages() -> bool {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
    }
    true
}