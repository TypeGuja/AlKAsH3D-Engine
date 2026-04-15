// examples/test_altex.rs - ИСПРАВЛЕННАЯ ВЕРСИЯ БЕЗ ОШИБОК
use alkash3d_rs::*;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HINSTANCE};
use windows::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_core::*;

// Используем Vertex из библиотеки
use alkash3d_rs::Vertex;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MVP {
    mvp: [[f32; 4]; 4],
}

// Глобальный флаг для выхода
static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║         AlKAsH3D Engine - .altex Render Test v2.0           ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // 1. Создаём устройство
    println!("[1/10] Creating D3D12 Device...");
    let device = create_device();
    if device.is_null() {
        eprintln!("❌ Failed to create device");
        return;
    }
    println!("      ✓ Device created (GPU: {})", get_gpu_name_safe(device));

    // 2. Создаём очередь команд
    println!("[2/10] Creating Command Queue...");
    let queue = create_command_queue(device);
    if queue.is_null() {
        eprintln!("❌ Failed to create command queue");
        return;
    }
    println!("      ✓ Command queue created");

    // 3. Создаём окно
    println!("[3/10] Creating Window...");
    let hwnd = create_window(800, 600, "AlKAsH3D Engine - .altex Render Test");
    if hwnd == 0 {
        eprintln!("❌ Failed to create window");
        return;
    }
    println!("      ✓ Window created (800x600)");

    // 4. Создаём Swap Chain
    println!("[4/10] Creating Swap Chain...");
    let swap = create_swap_chain(queue, hwnd, 800, 600);
    if swap.is_null() {
        eprintln!("❌ Failed to create swap chain");
        return;
    }
    println!("      ✓ Swap chain created");

    // 5. Создаём RTV heap и views
    println!("[5/10] Creating Render Target Views...");
    let rtv_heap = create_descriptor_heap(device, 2, 0, false);
    let rtv_start = GetCPUDescriptorHandleForHeapStart(rtv_heap);
    let rtv_size = get_rtv_descriptor_size();

    let mut rtvs = [0u64; 2];
    for i in 0..2 {
        let buf = swap_chain_get_buffer(swap, i as u32);
        let rtv = rtv_start + (i as u64 * rtv_size as u64);
        create_render_target_view(device, buf, rtv);
        rtvs[i as usize] = rtv;
    }
    println!("      ✓ {} RTVs created", rtvs.len());

    // 6. Командные списки
    println!("[6/10] Creating Command System...");
    if !create_command_allocators(device, 3) {
        eprintln!("❌ Failed to create command allocators");
        return;
    }
    let cmd_list = create_command_list(device);
    if cmd_list.is_null() {
        eprintln!("❌ Failed to create command list");
        return;
    }
    if !create_fence(device) {
        eprintln!("❌ Failed to create fence");
        return;
    }
    println!("      ✓ Command system ready");

    // 7. Загружаем или создаём тестовый .altex
    println!("[7/10] Loading 3D Model...");
    let test_model_path = "test_model.altex";
    let altex = match AltexFile::load(test_model_path) {
        Ok(a) => {
            println!("      ✓ Loaded .altex: {} meshes, {} vertices, {} indices",
                     a.meshes.len(), a.vertices.len(), a.indices.len());
            a
        }
        Err(_) => {
            println!("      ⚠️ No .altex found, creating test cube...");
            create_test_cube_model(test_model_path)
        }
    };

    // 8. Создаём буферы
    println!("[8/10] Creating GPU Buffers...");

    // Вершинный буфер
    let vertices_size = altex.vertices.len() * std::mem::size_of::<Vertex>();
    let vbuf = create_buffer(device, vertices_size, std::ptr::null());
    if vbuf.is_null() {
        eprintln!("❌ Failed to create vertex buffer");
        return;
    }
    let vertices_ptr = altex.vertices.as_ptr() as *const std::ffi::c_void;
    update_subresource(vbuf, vertices_ptr, vertices_size);

    // Индексный буфер
    let indices_size = altex.indices.len() * 4;
    let ibuf = create_buffer(device, indices_size, std::ptr::null());
    if ibuf.is_null() {
        eprintln!("❌ Failed to create index buffer");
        return;
    }
    let indices_ptr = altex.indices.as_ptr() as *const std::ffi::c_void;
    update_subresource(ibuf, indices_ptr, indices_size);

    println!("      ✓ Buffers created (VB: {} bytes, IB: {} bytes)", vertices_size, indices_size);

    // 9. Константный буфер
    let cb_size = std::mem::size_of::<MVP>();
    let cb = create_buffer(device, cb_size, std::ptr::null());
    if cb.is_null() {
        eprintln!("❌ Failed to create constant buffer");
        return;
    }
    let mvp = create_perspective_mvp(800.0 / 600.0);
    update_subresource(cb, &mvp as *const _ as _, cb_size);
    println!("      ✓ Constant buffer created");

    // 10. Создаём PSO
    println!("[9/10] Creating Pipeline State Object...");

    let root_sig = create_root_signature(
        device,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null()
    );

    if root_sig.is_null() {
        eprintln!("❌ Failed to create root signature");
        return;
    }

    let pso = create_simple_pso(device, root_sig);
    if pso.is_null() {
        eprintln!("❌ Failed to create PSO");
        return;
    }
    println!("      ✓ PSO created successfully");

    // 11. Основной цикл рендеринга
    println!("[10/10] Starting Render Loop...");
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Controls: ESC or Close window to exit");
    println!("═══════════════════════════════════════════════════════════════\n");

    let mut frame_count = 0u64;
    let start_time = std::time::Instant::now();
    let mut last_fps_update = start_time;
    let mut angle = 0.0f32;

    'render_loop: while RUNNING.load(Ordering::Relaxed) {
        // Обработка сообщений окна
        if !process_window_messages() {
            break 'render_loop;
        }

        // Начинаем кадр
        if !begin_frame() {
            eprintln!("❌ begin_frame failed");
            break 'render_loop;
        }

        let frame_idx = get_frame_index() as usize;
        let rtv = rtvs[frame_idx % 2];

        // Очищаем экран (тёмно-синий фон)
        let clear_color = [0.1f32, 0.15, 0.25, 1.0];
        clear_render_target(rtv, clear_color.as_ptr());

        // Настраиваем вьюпорт и scissors
        set_viewport(0.0, 0.0, 800.0, 600.0, 0.0, 1.0);
        set_scissor_rect(0, 0, 800, 600);

        // Устанавливаем PSO и ресурсы
        set_graphics_pipeline(pso);
        set_root_signature(root_sig);

        // Устанавливаем буферы
        let vbuf_gpu = get_buffer_gpu_address(vbuf);
        let ibuf_gpu = get_buffer_gpu_address(ibuf);

        set_vertex_buffer(vbuf_gpu, vertices_size as u32, std::mem::size_of::<Vertex>() as u32);
        set_index_buffer(ibuf_gpu, indices_size as u32, 4); // 32-bit индексы

        // Устанавливаем константный буфер
        set_root_constant_buffer_view(0, get_buffer_gpu_address(cb));

        // Рендерим все меши
        for mesh in &altex.meshes {
            draw_indexed_instanced(
                mesh.index_count,
                1,
                mesh.index_offset,
                mesh.vertex_offset as i32,
                0
            );
        }

        // Завершаем кадр
        if !end_frame() {
            eprintln!("❌ end_frame failed");
            break 'render_loop;
        }

        wait_for_gpu();
        present_swap_chain(swap, 1);

        frame_count += 1;

        // Вращаем модель
        angle += 0.02;
        let mvp = create_rotated_perspective_mvp(800.0 / 600.0, angle);
        update_subresource(cb, &mvp as *const _ as _, cb_size);

        // Статистика FPS
        let now = std::time::Instant::now();
        if (now - last_fps_update) >= Duration::from_secs(1) {
            let elapsed = (now - start_time).as_secs_f32();
            let fps = frame_count as f32 / elapsed;
            print!("\r📊 Frame: {} | FPS: {:.1} | Angle: {:.1}°    ", frame_count, fps, angle.to_degrees());
            use std::io::Write;
            std::io::stdout().flush().unwrap();
            last_fps_update = now;
        }

        // Небольшая задержка для стабильности
        thread::sleep(Duration::from_millis(1));
    }

    println!("\n\n✅ Render test completed successfully!");
    println!("   Total frames: {}", frame_count);
    println!("   Total time: {:.1}s", start_time.elapsed().as_secs_f32());
}

// Улучшенное создание окна
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

        if RegisterClassA(&wc) == 0 {
            return 0;
        }

        let title_cstr = std::ffi::CString::new(title).unwrap();

        let hwnd = CreateWindowExA(
            WINDOW_EX_STYLE::default(),
            class_name,
            PCSTR(title_cstr.as_ptr() as *const u8),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            None,
            None,
            HINSTANCE(inst.0),
            None,
        );

        match hwnd {
            Ok(h) => {
                ShowWindow(h, SW_SHOW);
                UpdateWindow(h);
                h.0 as usize
            }
            Err(_) => 0
        }
    }
}

// Улучшенная обработка сообщений окна
extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CLOSE => {
                RUNNING.store(false, Ordering::Relaxed);
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_DESTROY => {
                RUNNING.store(false, Ordering::Relaxed);
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if wparam.0 == 27 { // ESC key
                    RUNNING.store(false, Ordering::Relaxed);
                    PostQuitMessage(0);
                }
                DefWindowProcA(hwnd, msg, wparam, lparam)
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
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageA(&msg);
        }
    }
    true
}

fn get_gpu_name_safe(device: *mut std::ffi::c_void) -> String {
    let name_ptr = get_gpu_name(device);
    if name_ptr.is_null() {
        return "Unknown".to_string();
    }
    unsafe {
        std::ffi::CStr::from_ptr(name_ptr)
            .to_string_lossy()
            .to_string()
    }
}

// Вспомогательная функция для создания Vertex
fn create_vertex(pos: [f32; 3], normal: [f32; 3], uv: [f32; 2], color: [f32; 4]) -> Vertex {
    Vertex {
        position: pos,
        normal,
        tangent: [1.0, 0.0, 0.0],
        bitangent: [0.0, 1.0, 0.0],
        uv,
        uv2: [0.0, 0.0],
        color,
    }
}

// Создание тестового куба
fn create_test_cube_model(path: &str) -> AltexFile {
    println!("      Creating test cube model...");

    let mut altex = AltexFile::new();

    // Простой куб из 8 вершин и 36 индексов (12 треугольников)
    let vertices = vec![
        create_vertex([-0.5, -0.5,  0.5], [0.0, 0.0, 1.0], [0.0, 0.0], [1.0, 0.0, 0.0, 1.0]),
        create_vertex([ 0.5, -0.5,  0.5], [0.0, 0.0, 1.0], [1.0, 0.0], [0.0, 1.0, 0.0, 1.0]),
        create_vertex([ 0.5,  0.5,  0.5], [0.0, 0.0, 1.0], [1.0, 1.0], [0.0, 0.0, 1.0, 1.0]),
        create_vertex([-0.5,  0.5,  0.5], [0.0, 0.0, 1.0], [0.0, 1.0], [1.0, 1.0, 0.0, 1.0]),

        create_vertex([ 0.5, -0.5, -0.5], [0.0, 0.0, -1.0], [0.0, 0.0], [0.0, 1.0, 1.0, 1.0]),
        create_vertex([-0.5, -0.5, -0.5], [0.0, 0.0, -1.0], [1.0, 0.0], [1.0, 0.5, 0.0, 1.0]),
        create_vertex([-0.5,  0.5, -0.5], [0.0, 0.0, -1.0], [1.0, 1.0], [1.0, 1.0, 1.0, 1.0]),
        create_vertex([ 0.5,  0.5, -0.5], [0.0, 0.0, -1.0], [0.0, 1.0], [0.5, 0.5, 0.5, 1.0]),
    ];

    let indices = vec![
        // Передняя грань
        0, 1, 2, 2, 3, 0,
        // Задняя грань
        4, 5, 6, 6, 7, 4,
        // Левая грань
        5, 0, 3, 3, 6, 5,
        // Правая грань
        1, 4, 7, 7, 2, 1,
        // Верхняя грань
        3, 2, 7, 7, 6, 3,
        // Нижняя грань
        5, 4, 1, 1, 0, 5,
    ];

    altex.add_mesh(vertices, indices, "TestCube");

    match altex.save(path) {
        Ok(_) => println!("      ✓ Test model saved to {}", path),
        Err(e) => println!("      ⚠️ Failed to save test model: {}", e),
    }

    altex
}

// Матрицы для рендеринга
fn create_perspective_mvp(aspect: f32) -> MVP {
    let fov = 60.0f32.to_radians();
    let near = 0.1;
    let far = 100.0;
    let tan_half_fov = (fov / 2.0).tan();

    MVP {
        mvp: [
            [1.0 / (aspect * tan_half_fov), 0.0, 0.0, 0.0],
            [0.0, 1.0 / tan_half_fov, 0.0, 0.0],
            [0.0, 0.0, far / (far - near), 1.0],
            [0.0, 0.0, -(far * near) / (far - near), 0.0],
        ]
    }
}

fn create_rotated_perspective_mvp(aspect: f32, angle: f32) -> MVP {
    let cos = angle.cos();
    let sin = angle.sin();

    let fov = 60.0f32.to_radians();
    let near = 0.1;
    let far = 100.0;
    let tan_half_fov = (fov / 2.0).tan();

    // Матрица вида (камера смотрит на объект)
    let view = [
        [cos, 0.0, -sin, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [sin, 0.0, cos, 0.0],
        [0.0, 0.0, -3.0, 1.0], // Отодвигаем камеру
    ];

    // Матрица проекции
    let proj = [
        [1.0 / (aspect * tan_half_fov), 0.0, 0.0, 0.0],
        [0.0, 1.0 / tan_half_fov, 0.0, 0.0],
        [0.0, 0.0, far / (far - near), 1.0],
        [0.0, 0.0, -(far * near) / (far - near), 0.0],
    ];

    // Умножаем view * proj
    MVP {
        mvp: multiply_matrices(view, proj)
    }
}

fn multiply_matrices(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            result[i][j] = a[i][0] * b[0][j] +
                a[i][1] * b[1][j] +
                a[i][2] * b[2][j] +
                a[i][3] * b[3][j];
        }
    }
    result
}