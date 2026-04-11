// renderer.rs
use std::ffi::c_void;
use std::ptr;
use libloading::{Library, Symbol};

const FRAME_COUNT: u32 = 2;
const RTV_HEAP_SIZE: u32 = FRAME_COUNT;

// Типы функций
type CreateDeviceFn = unsafe fn() -> *mut c_void;
type CreateCommandQueueFn = unsafe fn(*mut c_void) -> *mut c_void;
type CreateSwapChainFn = unsafe fn(*mut c_void, usize, u32, u32) -> *mut c_void;
type CreateDescriptorHeapFn = unsafe fn(*mut c_void, u32, u32, bool) -> *mut c_void;
type GetCPUDescriptorHandleForHeapStartFn = unsafe fn(*mut c_void) -> u64;
type CreateRenderTargetViewFn = unsafe fn(*mut c_void, *mut c_void, u64) -> bool;
type SwapChainGetBufferFn = unsafe fn(*mut c_void, u32) -> *mut c_void;
type GetRTVDescriptorSizeFn = unsafe fn() -> u32;
type BeginFrameFn = unsafe fn() -> bool;
type EndFrameFn = unsafe fn() -> bool;
type PresentSwapChainFn = unsafe fn(*mut c_void, u32) -> bool;
type WaitForGPUFn = unsafe fn() -> bool;
type GetFrameIndexFn = unsafe fn() -> u32;
type ClearRenderTargetFn = unsafe fn(u64, *const f32) -> bool;
type SetViewportFn = unsafe fn(f32, f32, f32, f32, f32, f32) -> bool;
type SetScissorRectFn = unsafe fn(i32, i32, i32, i32) -> bool;
type ReleaseResourceFn = unsafe fn(*mut c_void);
type CreateCommandAllocatorsFn = unsafe fn(*mut c_void, u32) -> bool;
type CreateCommandListFn = unsafe fn(*mut c_void) -> *mut c_void;
type CreateFenceFn = unsafe fn(*mut c_void) -> bool;

// Структура для хранения загруженных функций
struct DllFunctions {
    create_device: CreateDeviceFn,
    create_command_queue: CreateCommandQueueFn,
    create_swap_chain: CreateSwapChainFn,
    create_descriptor_heap: CreateDescriptorHeapFn,
    get_cpu_descriptor_handle: GetCPUDescriptorHandleForHeapStartFn,
    create_render_target_view: CreateRenderTargetViewFn,
    swap_chain_get_buffer: SwapChainGetBufferFn,
    get_rtv_descriptor_size: GetRTVDescriptorSizeFn,
    begin_frame: BeginFrameFn,
    end_frame: EndFrameFn,
    present_swap_chain: PresentSwapChainFn,
    wait_for_gpu: WaitForGPUFn,
    get_frame_index: GetFrameIndexFn,
    clear_render_target: ClearRenderTargetFn,
    set_viewport: SetViewportFn,
    set_scissor_rect: SetScissorRectFn,
    release_resource: ReleaseResourceFn,
    create_command_allocators: CreateCommandAllocatorsFn,
    create_command_list: CreateCommandListFn,
    create_fence: CreateFenceFn,
    _lib: Library, // Библиотека должна жить вместе с функциями
}

pub struct Renderer {
    device: *mut c_void,
    queue: *mut c_void,
    swap_chain: *mut c_void,
    rtv_heap: *mut c_void,
    rtv_handles: Vec<u64>,
    rtv_descriptor_size: u32,
    width: u32,
    height: u32,
    hwnd: usize,
    funcs: DllFunctions,
}

impl Renderer {
    pub fn new(hwnd: usize, width: u32, height: u32) -> Self {
        // Загружаем DLL и функции сразу
        let lib = match unsafe { Library::new("alkash3d_rs.dll") } {
            Ok(lib) => lib,
            Err(e) => {
                eprintln!("Failed to load alkash3d_rs.dll: {}", e);
                std::process::exit(1);
            }
        };

        unsafe {
            let create_device = match lib.get(b"create_device") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_device: {}", e); std::process::exit(1); }
            };
            let create_command_queue = match lib.get(b"create_command_queue") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_command_queue: {}", e); std::process::exit(1); }
            };
            let create_swap_chain = match lib.get(b"create_swap_chain") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_swap_chain: {}", e); std::process::exit(1); }
            };
            let create_descriptor_heap = match lib.get(b"create_descriptor_heap") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_descriptor_heap: {}", e); std::process::exit(1); }
            };
            let get_cpu_descriptor_handle = match lib.get(b"GetCPUDescriptorHandleForHeapStart") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load GetCPUDescriptorHandleForHeapStart: {}", e); std::process::exit(1); }
            };
            let create_render_target_view = match lib.get(b"create_render_target_view") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_render_target_view: {}", e); std::process::exit(1); }
            };
            let swap_chain_get_buffer = match lib.get(b"swap_chain_get_buffer") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load swap_chain_get_buffer: {}", e); std::process::exit(1); }
            };
            let get_rtv_descriptor_size = match lib.get(b"get_rtv_descriptor_size") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load get_rtv_descriptor_size: {}", e); std::process::exit(1); }
            };
            let begin_frame = match lib.get(b"begin_frame") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load begin_frame: {}", e); std::process::exit(1); }
            };
            let end_frame = match lib.get(b"end_frame") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load end_frame: {}", e); std::process::exit(1); }
            };
            let present_swap_chain = match lib.get(b"present_swap_chain") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load present_swap_chain: {}", e); std::process::exit(1); }
            };
            let wait_for_gpu = match lib.get(b"wait_for_gpu") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load wait_for_gpu: {}", e); std::process::exit(1); }
            };
            let get_frame_index = match lib.get(b"get_frame_index") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load get_frame_index: {}", e); std::process::exit(1); }
            };
            let clear_render_target = match lib.get(b"clear_render_target") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load clear_render_target: {}", e); std::process::exit(1); }
            };
            let set_viewport = match lib.get(b"set_viewport") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load set_viewport: {}", e); std::process::exit(1); }
            };
            let set_scissor_rect = match lib.get(b"set_scissor_rect") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load set_scissor_rect: {}", e); std::process::exit(1); }
            };
            let release_resource = match lib.get(b"release_resource") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load release_resource: {}", e); std::process::exit(1); }
            };
            let create_command_allocators = match lib.get(b"create_command_allocators") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_command_allocators: {}", e); std::process::exit(1); }
            };
            let create_command_list = match lib.get(b"create_command_list") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_command_list: {}", e); std::process::exit(1); }
            };
            let create_fence = match lib.get(b"create_fence") {
                Ok(f) => *f,
                Err(e) => { eprintln!("Failed to load create_fence: {}", e); std::process::exit(1); }
            };

            Self {
                device: ptr::null_mut(),
                queue: ptr::null_mut(),
                swap_chain: ptr::null_mut(),
                rtv_heap: ptr::null_mut(),
                rtv_handles: Vec::new(),
                rtv_descriptor_size: 0,
                width,
                height,
                hwnd,
                funcs: DllFunctions {
                    create_device,
                    create_command_queue,
                    create_swap_chain,
                    create_descriptor_heap,
                    get_cpu_descriptor_handle,
                    create_render_target_view,
                    swap_chain_get_buffer,
                    get_rtv_descriptor_size,
                    begin_frame,
                    end_frame,
                    present_swap_chain,
                    wait_for_gpu,
                    get_frame_index,
                    clear_render_target,
                    set_viewport,
                    set_scissor_rect,
                    release_resource,
                    create_command_allocators,
                    create_command_list,
                    create_fence,
                    _lib: lib,
                },
            }
        }
    }

    pub fn init(&mut self) -> bool {
        println!("Initializing renderer...");

        unsafe {
            self.device = (self.funcs.create_device)();
            if self.device.is_null() {
                eprintln!("Failed to create device!");
                return false;
            }
            println!("✓ Device created");

            self.queue = (self.funcs.create_command_queue)(self.device);
            if self.queue.is_null() {
                eprintln!("Failed to create command queue!");
                return false;
            }
            println!("✓ Command queue created");

            self.swap_chain = (self.funcs.create_swap_chain)(self.queue, self.hwnd, self.width, self.height);
            if self.swap_chain.is_null() {
                eprintln!("Failed to create swap chain!");
                return false;
            }
            println!("✓ Swap chain created");

            self.rtv_descriptor_size = (self.funcs.get_rtv_descriptor_size)();
            println!("✓ RTV descriptor size: {}", self.rtv_descriptor_size);

            self.rtv_heap = (self.funcs.create_descriptor_heap)(self.device, RTV_HEAP_SIZE, 0, false);
            if self.rtv_heap.is_null() {
                eprintln!("Failed to create RTV heap!");
                return false;
            }
            println!("✓ RTV heap created");

            let rtv_start = (self.funcs.get_cpu_descriptor_handle)(self.rtv_heap);
            for i in 0..FRAME_COUNT {
                let buffer = (self.funcs.swap_chain_get_buffer)(self.swap_chain, i);
                if !buffer.is_null() {
                    let rtv_handle = rtv_start + (i as u64 * self.rtv_descriptor_size as u64);
                    (self.funcs.create_render_target_view)(self.device, buffer, rtv_handle);
                    self.rtv_handles.push(rtv_handle);
                    (self.funcs.release_resource)(buffer);
                }
            }
            println!("✓ Created {} RTVs", self.rtv_handles.len());

            if !(self.funcs.create_command_allocators)(self.device, FRAME_COUNT) {
                eprintln!("Failed to create command allocators!");
                return false;
            }
            println!("✓ Command allocators created");

            let cmd_list = (self.funcs.create_command_list)(self.device);
            if cmd_list.is_null() {
                eprintln!("Failed to create command list!");
                return false;
            }
            (self.funcs.release_resource)(cmd_list);
            println!("✓ Command list created");

            if !(self.funcs.create_fence)(self.device) {
                eprintln!("Failed to create fence!");
                return false;
            }
            println!("✓ Fence created");

            println!("Renderer initialized successfully!");
            true
        }
    }

    pub fn begin_frame(&mut self) {
        unsafe {
            (self.funcs.begin_frame)();
            let frame_idx = (self.funcs.get_frame_index)() as usize;
            if frame_idx < self.rtv_handles.len() {
                let clear_color = [0.1f32, 0.1, 0.15, 1.0];
                (self.funcs.clear_render_target)(self.rtv_handles[frame_idx], clear_color.as_ptr());
            }
            (self.funcs.set_viewport)(0.0, 0.0, self.width as f32, self.height as f32, 0.0, 1.0);
            (self.funcs.set_scissor_rect)(0, 0, self.width as i32, self.height as i32);
        }
    }

    pub fn end_frame(&mut self) {
        unsafe {
            (self.funcs.end_frame)();
            (self.funcs.present_swap_chain)(self.swap_chain, 1);
            (self.funcs.wait_for_gpu)();
        }
    }

    pub fn cleanup(&mut self) {
        unsafe {
            if !self.rtv_heap.is_null() {
                (self.funcs.release_resource)(self.rtv_heap);
            }
            if !self.swap_chain.is_null() {
                (self.funcs.release_resource)(self.swap_chain);
            }
            if !self.queue.is_null() {
                (self.funcs.release_resource)(self.queue);
            }
            if !self.device.is_null() {
                (self.funcs.release_resource)(self.device);
            }
        }
    }
}