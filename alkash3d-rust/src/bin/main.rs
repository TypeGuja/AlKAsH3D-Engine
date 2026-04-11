//! Тестовая программа для отрисовки треугольника через D3D12

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
use windows::Win32::Graphics::Gdi::HBRUSH;

// Типы для функций из нашего движка
type CreateDeviceFn = unsafe extern "C" fn() -> *mut c_void;
type CreateCommandQueueFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CreateCommandAllocatorsFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type CreateCommandListFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CreateFenceFn = unsafe extern "C" fn(*mut c_void) -> bool;
type CreateDescriptorHeapFn = unsafe extern "C" fn(*mut c_void, u32, u32, bool) -> *mut c_void;
type CreateBufferFn = unsafe extern "C" fn(*mut c_void, usize, *const u8) -> *mut c_void;
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
type ClearRenderTargetFn = unsafe extern "C" fn(u64, *const f32) -> bool;
type SetVertexBufferFn = unsafe extern "C" fn(u64, u32, u32) -> bool;
type DrawInstancedFn = unsafe extern "C" fn(u32, u32, u32, u32) -> bool;
type GetBufferGpuAddressFn = unsafe extern "C" fn(*mut c_void) -> u64;
type CreateRenderTargetViewFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool;
type GetRtvDescriptorSizeFn = unsafe extern "C" fn() -> u32;
type GetCpuHandleForHeapStartFn = unsafe extern "C" fn(*mut c_void) -> u64;
type ReleaseResourceFn = unsafe extern "C" fn(*mut c_void);
type CreateRootSignatureFn = unsafe extern "C" fn(*mut c_void, u32, *const u32, *const u32, *const u32) -> *mut c_void;
type SetRootSignatureFn = unsafe extern "C" fn(*mut c_void) -> bool;
type SetGraphicsPipelineFn = unsafe extern "C" fn(*mut c_void) -> bool;

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
    create_root_signature: CreateRootSignatureFn,
    set_root_signature: SetRootSignatureFn,
    set_graphics_pipeline: SetGraphicsPipelineFn,
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

// Встроенные шейдеры (минимальные)
const VS_CODE: &[u8] = &[
    0x44, 0x58, 0x42, 0x43, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const PS_CODE: &[u8] = VS_CODE;

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let separator = "=".repeat(60);
    println!("{}", separator);
    println!("D3D12 Triangle Demo - DirectX 12 Test");
    println!("{}", separator);

    println!("\n1. Getting D3D12 functions from alkash3d_rs...");

    let functions = D3D12Functions {
        create_device: alkash3d_rs::create_device,
        create_command_queue: alkash3d_rs::create_command_queue,
        create_command_allocators: alkash3d_rs::create_command_allocators,
        create_command_list: alkash3d_rs::create_command_list,
        create_fence: alkash3d_rs::create_fence,
        create_descriptor_heap: alkash3d_rs::create_descriptor_heap,
        create_buffer: alkash3d_rs::create_buffer,
        update_subresource: alkash3d_rs::update_subresource,
        create_swap_chain: alkash3d_rs::create_swap_chain,
        present_swap_chain: alkash3d_rs::present_swap_chain,
        swap_chain_get_buffer: alkash3d_rs::swap_chain_get_buffer,
        begin_frame: alkash3d_rs::begin_frame,
        end_frame: alkash3d_rs::end_frame,
        wait_for_gpu: alkash3d_rs::wait_for_gpu,
        set_render_target: alkash3d_rs::set_render_target,
        set_viewport: alkash3d_rs::set_viewport,
        set_scissor_rect: alkash3d_rs::set_scissor_rect,
        clear_render_target: alkash3d_rs::clear_render_target,
        set_vertex_buffer: alkash3d_rs::set_vertex_buffer,
        draw_instanced: alkash3d_rs::draw_instanced,
        get_buffer_gpu_address: alkash3d_rs::get_buffer_gpu_address,
        create_render_target_view: alkash3d_rs::create_render_target_view,
        get_rtv_descriptor_size: alkash3d_rs::get_rtv_descriptor_size,
        get_cpu_handle_for_heap_start: alkash3d_rs::GetCPUDescriptorHandleForHeapStart,
        release_resource: alkash3d_rs::release_resource,
        create_root_signature: alkash3d_rs::create_root_signature,
        set_root_signature: alkash3d_rs::set_root_signature,
        set_graphics_pipeline: alkash3d_rs::set_graphics_pipeline,
    };
    println!("   ✅ Functions loaded");

    // 2. Создаём окно
    println!("\n2. Creating window...");
    let instance = unsafe { GetModuleHandleW(None)? };

    let window_class = "D3D12TriangleWindow";
    let class_name_wide: Vec<u16> = window_class.encode_utf16().chain(Some(0)).collect();

    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance.into(),
        hIcon: HICON::default(),
        hCursor: HCURSOR::default(),
        hbrBackground: HBRUSH(std::ptr::null_mut()),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: PCWSTR(class_name_wide.as_ptr()),
    };

    let atom = unsafe { RegisterClassW(&wc) };
    if atom == 0 {
        panic!("Failed to register window class");
    }

    let window_title: Vec<u16> = "D3D12 Triangle Test".encode_utf16().chain(Some(0)).collect();

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name_wide.as_ptr()),
            PCWSTR(window_title.as_ptr()),
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

    // 3. Создаём D3D12 устройство
    println!("\n3. Creating D3D12 device...");
    let device = unsafe { (functions.create_device)() };
    if device.is_null() {
        panic!("Failed to create device");
    }
    println!("   ✅ Device: {:p}", device);

    // 4. Создаём command queue
    println!("\n4. Creating command queue...");
    let queue = unsafe { (functions.create_command_queue)(device) };
    if queue.is_null() {
        panic!("Failed to create queue");
    }
    println!("   ✅ Queue: {:p}", queue);

    // 5. Создаём command allocators
    println!("\n5. Creating command allocators...");
    if !unsafe { (functions.create_command_allocators)(device, 4) } {
        panic!("Failed to create allocators");
    }
    println!("   ✅ Created 4 allocators");

    // 6. Создаём command list
    println!("\n6. Creating command list...");
    let cmd_list = unsafe { (functions.create_command_list)(device) };
    if cmd_list.is_null() {
        panic!("Failed to create command list");
    }
    println!("   ✅ Command list: {:p}", cmd_list);

    // 7. Создаём fence
    println!("\n7. Creating fence...");
    if !unsafe { (functions.create_fence)(device) } {
        panic!("Failed to create fence");
    }
    println!("   ✅ Fence created");

    // 8. Создаём swap chain
    println!("\n8. Creating swap chain...");
    let swap_chain = unsafe { (functions.create_swap_chain)(queue, hwnd.0 as usize, WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32) };
    if swap_chain.is_null() {
        panic!("Failed to create swap chain");
    }
    println!("   ✅ Swap chain: {:p}", swap_chain);

    // 9. Создаём RTV heap
    println!("\n9. Creating RTV descriptor heap...");
    let rtv_heap = unsafe { (functions.create_descriptor_heap)(device, 2, 0, false) };
    if rtv_heap.is_null() {
        panic!("Failed to create RTV heap");
    }
    println!("   ✅ RTV heap: {:p}", rtv_heap);

    // 10. Создаём RTVs
    println!("\n10. Creating RTVs...");
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

    // 11. Создаём vertex buffer
    println!("\n11. Creating vertex buffer...");
    let vertices = [
        Vertex { x: 0.0, y: 0.5, z: 0.0, r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
        Vertex { x: -0.5, y: -0.5, z: 0.0, r: 0.0, g: 1.0, b: 0.0, a: 1.0 },
        Vertex { x: 0.5, y: -0.5, z: 0.0, r: 0.0, g: 0.0, b: 1.0, a: 1.0 },
    ];

    let vertex_size = std::mem::size_of_val(&vertices);
    let vb = unsafe { (functions.create_buffer)(device, vertex_size, ptr::null()) };
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

    // 12. Создаём root signature
    println!("\n12. Creating root signature...");
    let root_signature = unsafe { (functions.create_root_signature)(device, 0, ptr::null(), ptr::null(), ptr::null()) };
    if root_signature.is_null() {
        panic!("Failed to create root signature");
    }
    println!("   ✅ Root signature: {:p}", root_signature);

    // 13. Создаём PSO (Pipeline State Object)
    println!("\n13. Creating PSO...");

    // Создаём временные blob для шейдеров (имитация)
    let vs_blob = VS_CODE.as_ptr() as *mut c_void;
    let ps_blob = PS_CODE.as_ptr() as *mut c_void;

    let pso = unsafe { alkash3d_rs::create_graphics_pso(device, vs_blob, VS_CODE.len(), ps_blob, PS_CODE.len(), root_signature) };
    if pso.is_null() {
        panic!("Failed to create PSO");
    }
    println!("   ✅ PSO: {:p}", pso);

    // 14. Render loop
    println!("\n14. Starting render loop...");
    println!("    Close the window to exit");

    let mut frame = 0;
    let mut fps_counter = 0;
    let mut last_time = std::time::Instant::now();

    let mut msg = MSG::default();

    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);

            if !(functions.begin_frame)() {
                break;
            }

            let current_rtv = rtv_handles[frame as usize % 2];
            (functions.set_render_target)(current_rtv);
            let clear_color = [0.1f32, 0.1, 0.2, 1.0];
            (functions.clear_render_target)(current_rtv, clear_color.as_ptr());
            (functions.set_viewport)(0.0, 0.0, WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32, 0.0, 1.0);
            (functions.set_scissor_rect)(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT);

            // Устанавливаем root signature и PSO
            (functions.set_root_signature)(root_signature);
            (functions.set_graphics_pipeline)(pso);

            (functions.set_vertex_buffer)(vb_gpu_address, vertex_size as u32, std::mem::size_of::<Vertex>() as u32);
            (functions.draw_instanced)(3, 1, 0, 0);

            if !(functions.end_frame)() {
                break;
            }

            (functions.present_swap_chain)(swap_chain, 1);
            (functions.wait_for_gpu)();

            frame += 1;
            fps_counter += 1;

            if last_time.elapsed().as_secs() >= 1 {
                println!("   FPS: {}", fps_counter);
                fps_counter = 0;
                last_time = std::time::Instant::now();
            }
        }
    }

    // 15. Cleanup
    println!("\n15. Cleaning up...");
    unsafe {
        (functions.release_resource)(pso as *mut c_void);
        (functions.release_resource)(root_signature);
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