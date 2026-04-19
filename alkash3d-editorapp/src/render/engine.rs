//! Высокоуровневая обертка движка alkash3d_rs через FFI

use anyhow::{Result, anyhow};
use std::ffi::c_void;
use std::collections::HashMap;
use crate::math::{Mat4, Vec3, Vec4};
use crate::ffi::AlkashDll;

pub struct RenderEngine {
    dll: &'static AlkashDll,
    device: *mut c_void,
    queue: *mut c_void,
    swap_chain: *mut c_void,

    // Дескрипторы
    rtv_heap: *mut c_void,
    dsv_heap: *mut c_void,

    // Буферы
    depth_buffer: *mut c_void,
    constant_buffer: *mut c_void,

    // Кэш ресурсов
    meshes: HashMap<String, MeshData>,

    // Pipeline states
    default_pso: *mut c_void,
    wireframe_pso: *mut c_void,
    root_signature: *mut c_void,

    // Состояние
    width: u32,
    height: u32,
    initialized: bool,
    headless: bool,
}

#[derive(Clone)]
struct MeshData {
    vb: *mut c_void,
    ib: *mut c_void,
    vb_gpu_addr: u64,
    ib_gpu_addr: u64,
    vb_size: u32,
    ib_size: u32,
    index_count: u32,
    vertex_stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct FrameConstants {
    view_proj: Mat4,
    model: Mat4,
    camera_pos: Vec4,
    light_dir: Vec4,
    light_color: Vec4,
    ambient_color: Vec4,
    time: f32,
    _pad: [f32; 3],
}

impl RenderEngine {
    pub fn new(width: u32, height: u32, headless: bool) -> Self {
        Self {
            dll: AlkashDll::load(),
            device: std::ptr::null_mut(),
            queue: std::ptr::null_mut(),
            swap_chain: std::ptr::null_mut(),
            rtv_heap: std::ptr::null_mut(),
            dsv_heap: std::ptr::null_mut(),
            depth_buffer: std::ptr::null_mut(),
            constant_buffer: std::ptr::null_mut(),
            meshes: HashMap::new(),
            default_pso: std::ptr::null_mut(),
            wireframe_pso: std::ptr::null_mut(),
            root_signature: std::ptr::null_mut(),
            width,
            height,
            initialized: false,
            headless,
        }
    }

    pub fn init(&mut self, hwnd: usize) -> Result<()> {
        println!("[RenderEngine] Initializing with hwnd: 0x{:X}", hwnd);

        unsafe {
            self.device = (self.dll.create_device)();
            if self.device.is_null() {
                return Err(anyhow!("Failed to create D3D12 device"));
            }

            self.queue = (self.dll.create_command_queue)(self.device);
            if self.queue.is_null() {
                return Err(anyhow!("Failed to create command queue"));
            }

            if !self.headless && hwnd != 0 {
                self.swap_chain = (self.dll.create_swap_chain)(self.queue, hwnd, self.width, self.height);
                if self.swap_chain.is_null() {
                    println!("[RenderEngine] Warning: Failed to create swap chain, running headless");
                    self.headless = true;
                }
            }

            if !self.headless && !self.swap_chain.is_null() {
                self.rtv_heap = (self.dll.create_descriptor_heap)(self.device, 3, 0, false);
                self.dsv_heap = (self.dll.create_descriptor_heap)(self.device, 1, 1, false);
            }

            self.root_signature = (self.dll.create_root_signature_advanced)(self.device);
            self.default_pso = (self.dll.create_advanced_pso)(self.device, self.root_signature);
            self.wireframe_pso = (self.dll.create_pso)(self.device, self.root_signature, 3);

            self.constant_buffer = (self.dll.create_buffer)(
                self.device,
                std::mem::size_of::<FrameConstants>(),
                0,
            );

            (self.dll.create_command_allocators)(self.device, 3);
            (self.dll.create_fence)(self.device);
        }

        self.initialized = true;
        println!("[RenderEngine] Initialization complete");
        Ok(())
    }

    pub fn begin_frame(&mut self, _camera: &crate::math::Camera) {
        if !self.initialized || self.headless {
            return;
        }

        unsafe {
            (self.dll.begin_frame)();

            let clear_color = [0.1f32, 0.1, 0.15, 1.0];
            let frame_idx = (self.dll.get_frame_index)() as usize;

            let rtv_start = (self.dll.get_cpu_descriptor_handle_for_heap_start)(self.rtv_heap);
            let rtv_handle = rtv_start + (frame_idx as u64 * 32); // RTV descriptor size

            (self.dll.clear_render_target)(rtv_handle, clear_color.as_ptr());

            let dsv_start = (self.dll.get_cpu_descriptor_handle_for_heap_start)(self.dsv_heap);
            (self.dll.clear_depth_stencil)(dsv_start, 1.0, 0);

            (self.dll.set_render_targets_with_depth)(rtv_handle, dsv_start, 1);
            (self.dll.set_viewport)(0.0, 0.0, self.width as f32, self.height as f32, 0.0, 1.0);
            (self.dll.set_scissor_rect)(0, 0, self.width as i32, self.height as i32);

            (self.dll.set_graphics_pipeline)(self.default_pso);
            (self.dll.set_root_signature)(self.root_signature);
            (self.dll.set_primitive_topology)(4);
        }
    }

    pub fn end_frame(&mut self) {
        if !self.initialized || self.headless {
            return;
        }

        unsafe {
            (self.dll.end_frame)();
            (self.dll.present_swap_chain)(self.swap_chain, 1);
        }
    }

    pub fn render_mesh(
        &mut self,
        mesh_name: &str,
        transform: &Mat4,
        camera: &crate::math::Camera,
        wireframe: bool,
    ) {
        if !self.initialized {
            return;
        }

        if let Some(mesh) = self.meshes.get(mesh_name) {
            unsafe {
                if wireframe && !self.wireframe_pso.is_null() {
                    (self.dll.set_graphics_pipeline)(self.wireframe_pso);
                } else {
                    (self.dll.set_graphics_pipeline)(self.default_pso);
                }

                let constants = FrameConstants {
                    view_proj: camera.view_projection_matrix(),
                    model: *transform,
                    camera_pos: camera.transform.translation.extend(1.0),
                    light_dir: Vec3::new(-0.5, -1.0, -0.5).normalize().extend(0.0),
                    light_color: Vec4::new(1.0, 0.95, 0.9, 1.0),
                    ambient_color: Vec4::new(0.2, 0.25, 0.3, 1.0),
                    time: 0.0,
                    _pad: [0.0; 3],
                };

                (self.dll.update_subresource)(
                    self.constant_buffer,
                    &constants as *const _ as *const c_void,
                    std::mem::size_of::<FrameConstants>(),
                );

                let cb_gpu_addr = (self.dll.get_buffer_gpu_address)(self.constant_buffer);
                (self.dll.set_root_constant_buffer_view)(0, cb_gpu_addr);

                (self.dll.set_vertex_buffer)(mesh.vb_gpu_addr, mesh.vb_size, mesh.vertex_stride);
                (self.dll.set_index_buffer)(mesh.ib_gpu_addr, mesh.ib_size, 4);

                (self.dll.draw_indexed_instanced)(mesh.index_count, 1, 0, 0, 0);
            }
        }
    }

    pub fn load_altex(&mut self, path: &str) -> Result<Vec<String>> {
        // Заглушка для загрузки Altex через FFI
        println!("[RenderEngine] Loading Altex: {}", path);
        Ok(Vec::new())
    }

    pub fn cleanup(&mut self) {
        if !self.initialized {
            return;
        }

        unsafe {
            (self.dll.wait_for_gpu)();

            for mesh in self.meshes.values() {
                if !mesh.vb.is_null() { (self.dll.release_resource)(mesh.vb); }
                if !mesh.ib.is_null() { (self.dll.release_resource)(mesh.ib); }
            }
            self.meshes.clear();

            if !self.constant_buffer.is_null() { (self.dll.release_resource)(self.constant_buffer); }
            if !self.default_pso.is_null() { (self.dll.release_resource)(self.default_pso); }
            if !self.wireframe_pso.is_null() { (self.dll.release_resource)(self.wireframe_pso); }
            if !self.root_signature.is_null() { (self.dll.release_resource)(self.root_signature); }
            if !self.depth_buffer.is_null() { (self.dll.release_resource)(self.depth_buffer); }
            if !self.dsv_heap.is_null() { (self.dll.release_resource)(self.dsv_heap); }
            if !self.rtv_heap.is_null() { (self.dll.release_resource)(self.rtv_heap); }
            if !self.swap_chain.is_null() { (self.dll.release_resource)(self.swap_chain); }
            if !self.queue.is_null() { (self.dll.release_resource)(self.queue); }
            if !self.device.is_null() { (self.dll.release_resource)(self.device); }

            (self.dll.force_cleanup)();
        }

        self.initialized = false;
    }
}

impl Drop for RenderEngine {
    fn drop(&mut self) {
        self.cleanup();
    }
}