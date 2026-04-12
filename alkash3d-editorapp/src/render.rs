// editor/src/render.rs
use alkash3d_rs::*;

pub struct EditorRenderer {
    device: *mut std::ffi::c_void,
    queue: *mut std::ffi::c_void,
    swap_chain: *mut std::ffi::c_void,
    rtv_heap: *mut std::ffi::c_void,
    rtv_handles: Vec<u64>,
    rtv_descriptor_size: u32,
    width: u32,
    height: u32,
    hwnd: usize,
    initialized: bool,
}

impl EditorRenderer {
    pub fn new(hwnd: usize, width: u32, height: u32) -> anyhow::Result<Self> {
        Ok(Self {
            device: std::ptr::null_mut(),
            queue: std::ptr::null_mut(),
            swap_chain: std::ptr::null_mut(),
            rtv_heap: std::ptr::null_mut(),
            rtv_handles: Vec::new(),
            rtv_descriptor_size: 0,
            width,
            height,
            hwnd,
            initialized: false,
        })
    }

    pub fn init(&mut self) -> bool {
        println!("[Renderer] Initializing DirectX 12...");

        self.device = unsafe { create_device() };
        if self.device.is_null() {
            eprintln!("Failed to create device");
            return false;
        }

        self.queue = unsafe { create_command_queue(self.device) };
        if self.queue.is_null() {
            eprintln!("Failed to create command queue");
            return false;
        }

        self.swap_chain = unsafe { create_swap_chain(self.queue, self.hwnd, self.width, self.height) };
        if self.swap_chain.is_null() {
            eprintln!("Failed to create swap chain");
            return false;
        }

        self.rtv_descriptor_size = unsafe { get_rtv_descriptor_size() };
        self.rtv_heap = unsafe { create_descriptor_heap(self.device, 2, 0, false) };
        if self.rtv_heap.is_null() {
            eprintln!("Failed to create RTV heap");
            return false;
        }

        let rtv_start = unsafe { GetCPUDescriptorHandleForHeapStart(self.rtv_heap) };
        for i in 0..2 {
            let buffer = unsafe { swap_chain_get_buffer(self.swap_chain, i) };
            if !buffer.is_null() {
                let rtv_handle = rtv_start + (i as u64 * self.rtv_descriptor_size as u64);
                unsafe { create_render_target_view(self.device, buffer, rtv_handle) };
                self.rtv_handles.push(rtv_handle);
                unsafe { release_resource(buffer) };
            }
        }

        unsafe { create_command_allocators(self.device, 2) };
        let cmd_list = unsafe { create_command_list(self.device) };
        if !cmd_list.is_null() {
            unsafe { release_resource(cmd_list) };
        }
        unsafe { create_fence(self.device) };

        println!("[Renderer] DirectX 12 Ready");
        self.initialized = true;
        true
    }

    pub fn begin_frame(&mut self) {
        if !self.initialized { return; }
        unsafe {
            begin_frame();
            let frame_idx = get_frame_index() as usize;
            if frame_idx < self.rtv_handles.len() {
                let clear_color = [0.1f32, 0.1, 0.15, 1.0];
                clear_render_target(self.rtv_handles[frame_idx], clear_color.as_ptr());
                set_render_target(self.rtv_handles[frame_idx]);
            }
            set_viewport(0.0, 0.0, self.width as f32, self.height as f32, 0.0, 1.0);
            set_scissor_rect(0, 0, self.width as i32, self.height as i32);
        }
    }

    pub fn end_frame(&mut self) {
        if !self.initialized { return; }
        unsafe {
            end_frame();
            present_swap_chain(self.swap_chain, 1);
            wait_for_gpu();
        }
    }

    pub fn cleanup(&mut self) {
        if !self.initialized { return; }
        unsafe {
            if !self.rtv_heap.is_null() {
                release_resource(self.rtv_heap);
            }
            if !self.swap_chain.is_null() {
                release_resource(self.swap_chain);
            }
            if !self.queue.is_null() {
                release_resource(self.queue);
            }
            if !self.device.is_null() {
                release_resource(self.device);
            }
        }
        self.initialized = false;
        println!("[Renderer] Cleanup done");
    }
}