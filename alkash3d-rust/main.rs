use std::ffi::c_void;
use std::ptr;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::LibraryLoader::*,
        UI::WindowsAndMessaging::*,
    },
};

// Типы для функций из DLL
type CreateDeviceFn = unsafe extern "C" fn() -> *mut c_void;
type CreateCommandQueueFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CreateCommandAllocatorsFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type CreateCommandListFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CreateFenceFn = unsafe extern "C" fn(*mut c_void) -> bool;
type CreateDescriptorHeapFn = unsafe extern "C" fn(*mut c_void, u32, u32, bool) -> *mut c_void;
type CreateBufferFn = unsafe extern "C" fn(*mut c_void, usize, *mut c_void) -> *mut c_void;
type UpdateSubresourceFn = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool;
type CreateSwapChainFn = unsafe extern "C" fn(*mut c_void, usize, u32, u32) -> *mut c_void;
type PresentSwapChainFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type SwapChainGetBufferFn = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
type BeginFrameFn = unsafe extern "C" fn() -> bool;
type EndFrameFn = unsafe extern "C" fn() -> bool;
type WaitForGpuFn = unsafe extern "C" fn() -> bool;
type SetRenderTargetFn = unsafe extern "C" fn(u64) -> bool;
type SetViewportFn = unsafe extern "C" fn(f32, f32, f32, f32, f32, f32) -> bool;
type SetScissorRectFn = unsafe extern "C" fn(i32, i32, i32, i32) -> bool;
type ClearRenderTargetFn = unsafe extern "C" fn(u64, f32, f32, f32, f32) -> bool;
type SetVertexBufferFn = unsafe extern "C" fn(u64, u32, u32) -> bool;
type DrawInstancedFn = unsafe extern "C" fn(u32, u32, u32, u32) -> bool;
type GetBufferGpuAddressFn = unsafe extern "C" fn(*mut c_void) -> u64;
type CreateRenderTargetViewFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool;
type GetRtvDescriptorSizeFn = unsafe extern "C" fn() -> u32;
type GetCpuHandleForHeapStartFn = unsafe extern "C" fn(*mut c_void) -> u64;
type ReleaseResourceFn = unsafe extern "C" fn(*mut c_void);

struct D3D12Functions {
    create_device: CreateDeviceFn,
    create_command_queue: CreateCommandQueueFn,
    create_command_allocators: CreateCommandAllocatorsFn,
    create_command_list: CreateCommandListFn,
    create_fence: CreateFenceFn,
    create_descriptor_heap: CreateDescriptorHeapFn,
    create_buffer: CreateBufferFn,
    update_subresource: UpdateSubresourceFn,
    create_swap_chain: CreateSwapChainFn,
    present_swap_chain: PresentSwapChainFn,
    swap_chain_get_buffer: SwapChainGetBufferFn,
    begin_frame: BeginFrameFn,
    end_frame: EndFrameFn,
    wait_for_gpu: WaitForGpuFn,
    set_render_target: SetRenderTargetFn,
    set_viewport: SetViewportFn,
    set_scissor_rect: SetScissorRectFn,
    clear_render_target: ClearRenderTargetFn,
    set_vertex_buffer: SetVertexBufferFn,
    draw_instanced: DrawInstancedFn,
    get_buffer_gpu_address: GetBufferGpuAddressFn,
    create_render_target_view: CreateRenderTargetViewFn,
    get_rtv_descriptor_size: GetRtvDescriptorSizeFn,
    get_cpu_handle_for_heap_start: GetCpuHandleForHeapStartFn,
    release_resource: ReleaseResourceFn,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: f32,
    y: f32,
    z: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

const WINDOW_WIDTH: i32 = 800;
const WINDOW_HEIGHT: i32 = 600;

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=".repeat(60));
    println!("D3D12 Triangle Test (Pure Rust)");
    println!("=".repeat(60));

    // 1. Загружаем DLL
    println!("\n1. Loading DLL...");
    let dll_path = r"C:\Users\user\Documents\GitHub\AlKAsH3D-Engine\alkash3d-rust\target\release\alkash3d_rs.dll";
    let dll = unsafe { LoadLibraryA(dll_path) }?;
    println!("   ✅ DLL loaded");

    // 2. Получаем функции
    println!("\n2. Getting functions...");
    let functions = D3D12Functions {
        create_device: unsafe { GetProcAddress(dll, s!("create_device")).map(|f| std::mem::transmute(f)).unwrap() },
        create_command_queue: unsafe { GetProcAddress(dll, s!("create_command_queue")).map(|f| std::mem::transmute(f)).unwrap() },
        create_command_allocators: unsafe { GetProcAddress(dll, s!("create_command_allocators")).map(|f| std::mem::transmute(f)).unwrap() },
        create_command_list: unsafe { GetProcAddress(dll, s!("create_command_list")).map(|f| std::mem::transmute(f)).unwrap() },
        create_fence: unsafe { GetProcAddress(dll, s!("create_fence")).map(|f| std::mem::transmute(f)).unwrap() },
        create_descriptor_heap: unsafe { GetProcAddress(dll, s!("create_descriptor_heap")).map(|f| std::mem::transmute(f)).unwrap() },
        create_buffer: unsafe { GetProcAddress(dll, s!("create_buffer")).map(|f| std::mem::transmute(f)).unwrap() },
        update_subresource: unsafe { GetProcAddress(dll, s!("update_subresource")).map(|f| std::mem::transmute(f)).unwrap() },
        create_swap_chain: unsafe { GetProcAddress(dll, s!("create_swap_chain")).map(|f| std::mem::transmute(f)).unwrap() },
        present_swap_chain: unsafe { GetProcAddress(dll, s!("present_swap_chain")).map(|f| std::mem::transmute(f)).unwrap() },
        swap_chain_get_buffer: unsafe { GetProcAddress(dll, s!("swap_chain_get_buffer")).map(|f| std::mem::transmute(f)).unwrap() },
        begin_frame: unsafe { GetProcAddress(dll, s!("begin_frame")).map(|f| std::mem::transmute(f)).unwrap() },
        end_frame: unsafe { GetProcAddress(dll, s!("end_frame")).map(|f| std::mem::transmute(f)).unwrap() },
        wait_for_gpu: unsafe { GetProcAddress(dll, s!("wait_for_gpu")).map(|f| std::mem::transmute(f)).unwrap() },
        set_render_target: unsafe { GetProcAddress(dll, s!("set_render_target")).map(|f| std::mem::transmute(f)).unwrap() },
        set_viewport: unsafe { GetProcAddress(dll, s!("set_viewport")).map(|f| std::mem::transmute(f)).unwrap() },
        set_scissor_rect: unsafe { GetProcAddress(dll, s!("set_scissor_rect")).map(|f| std::mem::transmute(f)).unwrap() },
        clear_render_target: unsafe { GetProcAddress(dll, s!("clear_render_target")).map(|f| std::mem::transmute(f)).unwrap() },
        set_vertex_buffer: unsafe { GetProcAddress(dll, s!("set_vertex_buffer")).map(|f| std::mem::transmute(f)).unwrap() },
        draw_instanced: unsafe { GetProcAddress(dll, s!("draw_instanced")).map(|f| std::mem::transmute(f)).unwrap() },
        get_buffer_gpu_address: unsafe { GetProcAddress(dll, s!("get_buffer_gpu_address")).map(|f| std::mem::transmute(f)).unwrap() },
        create_render_target_view: unsafe { GetProcAddress(dll, s!("create_render_target_view")).map(|f| std::mem::transmute(f)).unwrap() },
        get_rtv_descriptor_size: unsafe { GetProcAddress(dll, s!("get_rtv_descriptor_size")).map(|f| std::mem::transmute(f)).unwrap() },
        get_cpu_handle_for_heap_start: unsafe { GetProcAddress(dll, s!("GetCPUDescriptorHandleForHeapStart")).map(|f| std::mem::transmute(f)).unwrap() },
        release_resource: unsafe { GetProcAddress(dll, s!("release_resource")).map(|f| std::mem::transmute(f)).unwrap() },
    };
    println!("   ✅ Functions loaded");

    // 3. Создаём окно
    println!("\n3. Creating window...");
    let instance = unsafe { GetModuleHandleW(None)? };

    let window_class = "D3D12TriangleWindow";
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: LoadIconW(instance, IDI_APPLICATION)?,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) },
        lpszMenuName: PCWSTR::null(),
        lpszClassName: PCWSTR::from_raw(window_class.encode_utf16().collect::<Vec<_>>().as_ptr()),
    };

    let atom = unsafe { RegisterClassW(&wc) };
    if atom == 0 {
        panic!("Failed to register window class");
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR::from_raw(window_class.encode_utf16().collect::<Vec<_>>().as_ptr()),
            "D3D12 Triangle Test",
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            None,
            None,
            instance,
            None,
        )
    }?;

    println!("   ✅ Window created: {:?}", hwnd);

    // 4. Создаём D3D12 устройства
    println!("\n4. Creating D3D12 device...");
    let device = unsafe { (functions.create_device)() };
    if device.is_null() {
        panic!("Failed to create device");
    }
    println!("   ✅ Device: {:p}", device);

    // 5. Создаём command queue
    println!("\n5. Creating command queue...");
    let queue = unsafe { (functions.create_command_queue)(device) };
    if queue.is_null() {
        panic!("Failed to create queue");
    }
    println!("   ✅ Queue: {:p}", queue);

    // 6. Создаём command allocators
    println!("\n6. Creating command allocators...");
    if !unsafe { (functions.create_command_allocators)(device, 4) } {
        panic!("Failed to create allocators");
    }
    println!("   ✅ Created 4 allocators");

    // 7. Создаём command list
    println!("\n7. Creating command list...");
    let cmd_list = unsafe { (functions.create_command_list)(device) };
    if cmd_list.is_null() {
        panic!("Failed to create command list");
    }
    println!("   ✅ Command list: {:p}", cmd_list);

    // 8. Создаём fence
    println!("\n8. Creating fence...");
    if !unsafe { (functions.create_fence)(device) } {
        panic!("Failed to create fence");
    }
    println!("   ✅ Fence created");

    // 9. Создаём swap chain
    println!("\n9. Creating swap chain...");
    let swap_chain = unsafe { (functions.create_swap_chain)(queue, hwnd.0 as usize, WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32) };
    if swap_chain.is_null() {
        panic!("Failed to create swap chain");
    }
    println!("   ✅ Swap chain: {:p}", swap_chain);

    // 10. Создаём RTV heap
    println!("\n10. Creating RTV descriptor heap...");
    let rtv_heap = unsafe { (functions.create_descriptor_heap)(device, 2, 0, false) };
    if rtv_heap.is_null() {
        panic!("Failed to create RTV heap");
    }
    println!("   ✅ RTV heap: {:p}", rtv_heap);

    // 11. Создаём RTVs
    println!("\n11. Creating RTVs...");
    let rtv_size = unsafe { (functions.get_rtv_descriptor_size)() };
    let rtv_base = unsafe { (functions.get_cpu_handle_for_heap_start)(rtv_heap) };

    let mut rtv_handles = Vec::new();
    for i in 0..2 {
        let rtv_handle = rtv_base + (i as u64 * rtv_size as u64);
        let buffer = unsafe { (functions.swap_chain_get_buffer)(swap_chain, i) };
        if !buffer.is_null() {
            unsafe { (functions.create_render_target_view)(device, buffer, rtv_handle) };
            rtv_handles.push(rtv_handle);
            println!("   RTV {}: {:#x}", i, rtv_handle);
        }
    }

    // 12. Создаём vertex buffer
    println!("\n12. Creating vertex buffer...");
    let vertices = [
        Vertex { x: 0.0, y: 0.5, z: 0.0, r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
        Vertex { x: -0.5, y: -0.5, z: 0.0, r: 0.0, g: 1.0, b: 0.0, a: 1.0 },
        Vertex { x: 0.5, y: -0.5, z: 0.0, r: 0.0, g: 0.0, b: 1.0, a: 1.0 },
    ];

    let vertex_size = std::mem::size_of_val(&vertices);
    let vb = unsafe { (functions.create_buffer)(device, vertex_size, ptr::null_mut()) };
    if vb.is_null() {
        panic!("Failed to create vertex buffer");
    }
    println!("   ✅ Vertex buffer: {:p}", vb);

    if !unsafe { (functions.update_subresource)(vb, vertices.as_ptr() as *const c_void, vertex_size) } {
        panic!("Failed to update buffer");
    }
    println!("   ✅ Vertex data uploaded");

    let vb_gpu_address = unsafe { (functions.get_buffer_gpu_address)(vb) };
    println!("   GPU address: {:#x}", vb_gpu_address);

    // 13. Render loop
    println!("\n13. Starting render loop...");
    println!("    Close the window to exit");

    let mut frame = 0;
    let mut fps_counter = 0;
    let mut last_time = std::time::Instant::now();

    let mut msg = MSG::default();

    while unsafe { GetMessageW(&mut msg, None, 0, 0).into() } {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Render
        unsafe {
            if !(functions.begin_frame)() {
                break;
            }

            let current_rtv = rtv_handles[frame as usize % 2];
            (functions.set_render_target)(current_rtv);
            (functions.clear_render_target)(current_rtv, 0.1, 0.1, 0.2, 1.0);
            (functions.set_viewport)(0.0, 0.0, WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32, 0.0, 1.0);
            (functions.set_scissor_rect)(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT);
            (functions.set_vertex_buffer)(vb_gpu_address, vertex_size as u32, std::mem::size_of::<Vertex>() as u32);
            (functions.draw_instanced)(3, 1, 0, 0);

            if !(functions.end_frame)() {
                break;
            }

            (functions.present_swap_chain)(swap_chain, 1);
            (functions.wait_for_gpu)();
        }

        frame += 1;
        fps_counter += 1;

        if last_time.elapsed().as_secs() >= 1 {
            println!("   FPS: {}", fps_counter);
            fps_counter = 0;
            last_time = std::time::Instant::now();
        }
    }

    // 14. Cleanup
    println!("\n14. Cleaning up...");
    unsafe {
        (functions.release_resource)(vb);
        (functions.release_resource)(rtv_heap);
        (functions.release_resource)(swap_chain);
        (functions.release_resource)(cmd_list);
        (functions.release_resource)(queue);
        (functions.release_resource)(device);
    }

    println!("\n✅ Test completed!");
    Ok(())
}