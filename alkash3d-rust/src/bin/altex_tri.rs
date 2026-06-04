use std::ffi::c_void;
// src/bin/altex_triangle.rs - Тест с загрузкой из ALTeX файла
use alkash3d_rs::*;
use std::ptr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows_core::PCSTR;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;

static mut RUNNING: bool = true;

// Вершина для ALTeX формата
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl Vertex {
    const STRIDE: usize = 28;
}

const VERTICES: [Vertex; 3] = [
    Vertex { position: [0.0, 0.5, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
    Vertex { position: [-0.5, -0.5, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
    Vertex { position: [0.5, -0.5, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
];

// Простой вершинный шейдер для ALTeX формата
const VS_ALTEX: &str = r#"
struct VSInput {
    float3 position : POSITION;
    float4 color : COLOR;
};

struct VSOutput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

VSOutput main(VSInput input) {
    VSOutput output;
    output.position = float4(input.position, 1.0);
    output.color = input.color;
    return output;
}
"#;

// Простой пиксельный шейдер
const PS_ALTEX: &str = r#"
struct PSInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

float4 main(PSInput input) : SV_TARGET {
    return input.color;
}
"#;

macro_rules! log {
    ($($arg:tt)*) => {
        println!($($arg)*);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    };
}

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

unsafe fn create_window() -> HWND {
    let instance = GetModuleHandleA(None).unwrap();
    let class_name = b"AltexTriangleTest\0";
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

    CreateWindowExA(
        WINDOW_EX_STYLE::default(),
        class_ptr,
        PCSTR(b"ALTeX Triangle Test - ESC to exit\0".as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        WIDTH as i32,
        HEIGHT as i32,
        None,
        None,
        Some(HINSTANCE::from(instance)),
        None,
    ).unwrap()
}

// Функция компиляции шейдера
unsafe fn compile_shader(source: &str, target: &str, entry: &str) -> *mut c_void {
    use windows::Win32::Graphics::Direct3D::ID3DBlob;

    #[link(name = "d3dcompiler")]
    extern "system" {
        fn D3DCompile(
            pSrcData: *const core::ffi::c_void,
            SrcDataSize: usize,
            pSourceName: PCSTR,
            pDefines: *const core::ffi::c_void,
            pInclude: *const core::ffi::c_void,
            pEntrypoint: PCSTR,
            pTarget: PCSTR,
            Flags1: u32,
            Flags2: u32,
            ppCode: *mut *mut c_void,
            ppErrorMsgs: *mut *mut c_void,
        ) -> windows_core::HRESULT;
    }

    let entry_cstr = std::ffi::CString::new(entry).unwrap();
    let target_cstr = std::ffi::CString::new(target).unwrap();

    let mut code_blob: *mut c_void = std::ptr::null_mut();
    let mut error_blob: *mut c_void = std::ptr::null_mut();

    let hr = D3DCompile(
        source.as_ptr() as *const _,
        source.len(),
        PCSTR(b"shader.hlsl\0".as_ptr()),
        std::ptr::null(),
        std::ptr::null(),
        PCSTR(entry_cstr.as_ptr() as *const u8),
        PCSTR(target_cstr.as_ptr() as *const u8),
        0,
        0,
        &mut code_blob,
        &mut error_blob,
    );

    if hr.is_err() {
        if !error_blob.is_null() {
            let error_interface = &*(error_blob as *const ID3DBlob);
            let err_ptr = error_interface.GetBufferPointer();
            let err_size = error_interface.GetBufferSize();
            if err_size > 0 && !err_ptr.is_null() {
                let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                eprintln!("Shader error:\n{}", String::from_utf8_lossy(err_msg));
            }
        }
        return std::ptr::null_mut();
    }

    code_blob
}

fn main() {
    log!("\n╔════════════════════════════════════════════════════════════════╗");
    log!("║                  ALTeX TRIANGLE TEST                            ║");
    log!("║                         ESC - exit                              ║");
    log!("╚════════════════════════════════════════════════════════════════╝\n");

    unsafe {
        // Включаем Debug Layer
        enable_debug_layer();

        // Создаём окно
        let hwnd = create_window();
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        log!("[1] Window created");

        // Создаём Device и Queue
        let pair_ptr = create_device_and_queue();
        if pair_ptr.is_null() {
            eprintln!("❌ Failed to create device and queue");
            return;
        }
        let device_ptr = get_device_from_pair(pair_ptr);
        log!("[2] Device ready");

        // Создаём Swap Chain
        let swap_ptr = create_swap_chain(std::ptr::null_mut(), hwnd.0, WIDTH, HEIGHT);
        if swap_ptr.is_null() {
            eprintln!("❌ Failed to create swap chain");
            return;
        }
        log!("[3] Swap chain ready");

        // Создаём Fence
        if !create_fence(device_ptr) {
            eprintln!("❌ Failed to create fence");
            return;
        }
        log!("[4] Fence ready");

        // Компилируем шейдеры
        log!("[5] Compiling shaders...");
        let vs_blob = compile_shader(VS_ALTEX, "vs_5_0", "main");
        let ps_blob = compile_shader(PS_ALTEX, "ps_5_0", "main");

        if vs_blob.is_null() || ps_blob.is_null() {
            eprintln!("❌ Failed to compile shaders");
            return;
        }
        log!("[5] Shaders compiled");

        // Создаём простую root signature
        log!("[5] Creating root signature...");
        let root_sig = create_root_signature_simple(device_ptr);
        if root_sig.is_null() {
            eprintln!("❌ Failed to create root signature");
            return;
        }
        log!("[5] Root signature created");

        // Создаём PSO
        log!("[5] Creating PSO...");
        let pso = create_pso(device_ptr, root_sig, 2); // Test shader
        if pso.is_null() {
            eprintln!("❌ Failed to create PSO");
            return;
        }
        log!("[5] PSO ready");

        // Создаём вершинный буфер
        log!("[6] Creating vertex buffer...");
        let vb = create_buffer(device_ptr, VERTICES.len() * Vertex::STRIDE, 0);
        if vb.is_null() {
            eprintln!("❌ Failed to create vertex buffer");
            return;
        }

        log!("[6] Uploading vertex data...");
        if !update_subresource(vb, VERTICES.as_ptr() as *const _, VERTICES.len() * Vertex::STRIDE) {
            eprintln!("❌ Failed to update vertex buffer");
            return;
        }

        // Ждём, чтобы GPU увидел данные
        wait_for_gpu();

        let vb_gpu = get_buffer_gpu_address(vb);
        log!("[6] Vertex buffer ready, GPU addr: 0x{:X}, size: {} bytes",
             vb_gpu, VERTICES.len() * Vertex::STRIDE);

        // Выводим вершины для проверки
        log!("  Vertex 0: pos=({:.2},{:.2},{:.2}) color=({:.2},{:.2},{:.2},{:.2})",
             VERTICES[0].position[0], VERTICES[0].position[1], VERTICES[0].position[2],
             VERTICES[0].color[0], VERTICES[0].color[1], VERTICES[0].color[2], VERTICES[0].color[3]);
        log!("  Vertex 1: pos=({:.2},{:.2},{:.2}) color=({:.2},{:.2},{:.2},{:.2})",
             VERTICES[1].position[0], VERTICES[1].position[1], VERTICES[1].position[2],
             VERTICES[1].color[0], VERTICES[1].color[1], VERTICES[1].color[2], VERTICES[1].color[3]);
        log!("  Vertex 2: pos=({:.2},{:.2},{:.2}) color=({:.2},{:.2},{:.2},{:.2})",
             VERTICES[2].position[0], VERTICES[2].position[1], VERTICES[2].position[2],
             VERTICES[2].color[0], VERTICES[2].color[1], VERTICES[2].color[2], VERTICES[2].color[3]);

        // Создаём constant buffer
        let cb = create_buffer(device_ptr, 256, 0);
        let cb_gpu = get_buffer_gpu_address(cb);
        let cb_map = map_buffer(cb);
        log!("[7] Constant buffer ready");

        // Создаём RTV heap
        let rtv_heap = create_descriptor_heap(device_ptr, 2, 0, false);
        let rtv_base = GetCPUDescriptorHandleForHeapStart(rtv_heap);
        let rtv_inc = get_descriptor_handle_increment_size(device_ptr, 0);
        log!("[8] RTV heap ready");

        // Получаем back buffers и создаём RTV
        let back0 = swap_chain_get_buffer(swap_ptr, 0);
        let back1 = swap_chain_get_buffer(swap_ptr, 1);
        create_render_target_view(device_ptr, back0, rtv_base);
        create_render_target_view(device_ptr, back1, rtv_base + rtv_inc as u64);
        log!("[9] RTVs created");

        log!("\n[10] START RENDERING\n");
        log!("     You should see a RED-GREEN-BLUE triangle on BLACK background\n");

        let mut frame = 0;
        let mut msg = std::mem::zeroed();

        while RUNNING && frame < 300 {
            // Обработка сообщений
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    RUNNING = false;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }

            if !RUNNING {
                break;
            }

            let idx = get_current_back_buffer_index(swap_ptr);
            let current_rtv = if idx == 0 { rtv_base } else { rtv_base + rtv_inc as u64 };

            // Начинаем кадр
            let cmd_list = begin_frame();
            if cmd_list.is_null() {
                log!("❌ begin_frame failed");
                break;
            }

            // Устанавливаем PSO
            if !set_pipeline(pso) {
                log!("❌ set_pipeline failed");
                break;
            }

            // Устанавливаем root signature
            if !set_root_signature(root_sig) {
                log!("❌ set_root_signature failed");
                break;
            }

            // Устанавливаем render target
            if !set_render_target(current_rtv) {
                log!("❌ set_render_target failed");
                break;
            }

            // Очищаем экран (ЧЁРНЫЙ фон)
            let clear_color = [0.0f32, 0.0f32, 0.0f32, 1.0f32];
            if !clear_render_target(current_rtv, clear_color.as_ptr()) {
                log!("❌ clear_render_target failed");
                break;
            }

            // Устанавливаем viewport
            if !set_viewport(0.0, 0.0, WIDTH as f32, HEIGHT as f32, 0.0, 1.0) {
                log!("❌ set_viewport failed");
                break;
            }

            // Устанавливаем vertex buffer
            if !set_vertex_buffer(vb_gpu, (VERTICES.len() * Vertex::STRIDE) as u32, Vertex::STRIDE as u32) {
                log!("❌ set_vertex_buffer failed");
                break;
            }

            // Устанавливаем топологию
            if !set_primitive_topology(4) { // D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
                log!("❌ set_primitive_topology failed");
                break;
            }

            // Устанавливаем constant buffer
            if !set_root_constant_buffer_view(0, cb_gpu) {
                log!("❌ set_root_constant_buffer_view failed");
                break;
            }

            // Рисуем треугольник
            if !draw_instanced(3, 1, 0, 0) {
                log!("❌ draw_instanced failed");
                break;
            }

            // Завершаем кадр
            if !end_frame() {
                log!("❌ end_frame failed");
                break;
            }

            // Презентуем
            if !present_swap_chain(swap_ptr, 0) {
                log!("❌ present_swap_chain failed");
                break;
            }

            frame += 1;
            if frame == 1 {
                log!("✓ First frame rendered successfully!");
                log!("✓ If you see a black screen, the triangle is not rendering.");
                log!("✓ Check that the vertex colors are not black.");
            }

            if frame % 60 == 0 {
                log!("Frame {} rendered", frame);
            }
        }

        log!("\n[11] Shutdown, {} frames rendered", frame);

        wait_for_gpu();
        force_cleanup();

        log!("✅ Done!\n");
    }
}