// editor/src/render.rs - ПОЛНОСТЬЮ ИСПРАВЛЕННАЯ ВЕРСИЯ

use anyhow::{Result, anyhow};
use alkash3d_rs::*;
use std::ffi::c_void;
use std::collections::HashMap;
use std::path::Path;

const DLL_PATHS: &[&str] = &[
    "./alkash3d_rs.dll",
    "../alkash3d-rust/target/release/alkash3d_rs.dll",
    "../../alkash3d-rust/target/release/alkash3d_rs.dll",
    "./target/release/alkash3d_rs.dll",
    "./target/debug/alkash3d_rs.dll",
];

pub struct EditorRenderer {
    device: *mut c_void,
    queue: *mut c_void,
    swap_chain: *mut c_void,
    rtv_heap: *mut c_void,
    dsv_heap: *mut c_void,
    depth_buffer: *mut c_void,
    rtv_handles: Vec<u64>,
    dsv_handle: u64,
    rtv_descriptor_size: u32,
    dsv_descriptor_size: u32,
    width: u32,
    height: u32,
    hwnd: usize,
    initialized: bool,
    dll_loaded: bool,
    headless_mode: bool,

    loaded_meshes: HashMap<String, MeshResource>,
    loaded_textures: HashMap<String, TextureResource>,
    loaded_altex_files: HashMap<String, AltexFile>,

    default_pso: *mut c_void,
    root_signature: *mut c_void,
    constant_buffer: *mut c_void,
    cb_gpu_addr: u64,
}

struct MeshResource {
    vb: *mut c_void,
    ib: *mut c_void,
    vb_gpu_addr: u64,
    ib_gpu_addr: u64,
    vb_size: u32,
    ib_size: u32,
    index_count: u32,
    vertex_stride: u32,
    material_id: u32,
}

struct TextureResource {
    texture: *mut c_void,
    srv_heap: *mut c_void,
    srv_handle: u64,
    width: u32,
    height: u32,
}

#[repr(C)]
struct ConstantBuffer {
    world: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    color: [f32; 4],
}

impl EditorRenderer {
    pub fn new(hwnd: usize, width: u32, height: u32) -> Result<Self> {
        let dll_loaded = Self::check_dll();
        let headless_mode = hwnd == 0;

        if headless_mode {
            println!("[Renderer] Running in headless mode (no window handle)");
        }

        Ok(Self {
            device: std::ptr::null_mut(),
            queue: std::ptr::null_mut(),
            swap_chain: std::ptr::null_mut(),
            rtv_heap: std::ptr::null_mut(),
            dsv_heap: std::ptr::null_mut(),
            depth_buffer: std::ptr::null_mut(),
            rtv_handles: Vec::new(),
            dsv_handle: 0,
            rtv_descriptor_size: 0,
            dsv_descriptor_size: 0,
            width, height, hwnd,
            initialized: false,
            dll_loaded,
            headless_mode,
            loaded_meshes: HashMap::new(),
            loaded_textures: HashMap::new(),
            loaded_altex_files: HashMap::new(),
            default_pso: std::ptr::null_mut(),
            root_signature: std::ptr::null_mut(),
            constant_buffer: std::ptr::null_mut(),
            cb_gpu_addr: 0,
        })
    }

    fn check_dll() -> bool {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();

        for path in DLL_PATHS {
            let full_path = if Path::new(path).is_absolute() {
                Path::new(path).to_path_buf()
            } else {
                exe_dir.join(path)
            };

            if full_path.exists() {
                println!("[Renderer] DLL found at: {:?}", full_path);
                return true;
            }
        }

        println!("[Renderer] DLL not found, will use fallback");
        false
    }

    pub fn init(&mut self) -> Result<()> {
        println!("[Renderer] Initializing...");

        unsafe {
            self.device = create_device();
            if self.device.is_null() {
                return Err(anyhow!("Failed to create D3D12 device"));
            }

            if !is_real_gpu() {
                println!("[Renderer] Running in WARP mode (software)");
            } else {
                let gpu_name_ptr = get_gpu_name(self.device);
                if !gpu_name_ptr.is_null() {
                    let gpu_name = std::ffi::CStr::from_ptr(gpu_name_ptr).to_string_lossy();
                    println!("[Renderer] GPU: {}", gpu_name);
                }
            }

            self.queue = create_command_queue(self.device);
            if self.queue.is_null() {
                return Err(anyhow!("Failed to create command queue"));
            }

            if !self.headless_mode {
                self.swap_chain = create_swap_chain(self.queue, self.hwnd, self.width, self.height);
                if self.swap_chain.is_null() {
                    println!("[Renderer] Failed to create swap chain, switching to headless mode");
                    self.headless_mode = true;
                }
            }

            if !self.headless_mode {
                self.rtv_descriptor_size = get_rtv_descriptor_size();
                self.dsv_descriptor_size = get_dsv_descriptor_size();

                self.rtv_heap = create_descriptor_heap(self.device, 2, 0, false);
                self.dsv_heap = create_descriptor_heap(self.device, 1, 1, false);

                if self.rtv_heap.is_null() || self.dsv_heap.is_null() {
                    return Err(anyhow!("Failed to create descriptor heaps"));
                }

                self.create_depth_buffer()?;
                self.create_render_target_views()?;
            }

            self.create_constant_buffer()?;

            if !create_command_allocators(self.device, 2) {
                return Err(anyhow!("Failed to create command allocators"));
            }

            let cmd_list = create_command_list(self.device);
            if cmd_list.is_null() {
                return Err(anyhow!("Failed to create command list"));
            }
            release_resource(cmd_list);

            if !create_fence(self.device) {
                return Err(anyhow!("Failed to create fence"));
            }

            // Создаем Root Signature
            self.root_signature = self.create_root_signature_internal();
            println!("[Renderer] Root signature: {}", if self.root_signature.is_null() { "FAILED" } else { "OK" });

            // Создаем PSO с исправленным input layout
            self.default_pso = create_simple_pso(self.device, self.root_signature);
            println!("[Renderer] PSO: {}", if self.default_pso.is_null() { "FAILED" } else { "OK" });
        }

        self.initialized = true;
        println!("[Renderer] Ready ({}x{}, headless={})", self.width, self.height, self.headless_mode);
        Ok(())
    }

    unsafe fn create_root_signature_internal(&mut self) -> *mut c_void {
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Direct3D::ID3DBlob;
        use windows_core::Interface;

        let device: ID3D12Device = std::mem::transmute_copy(&self.device);

        let descriptor_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
            NumDescriptors: 1,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
        };

        let root_parameter = D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &descriptor_range,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };

        let root_sig_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 1,
            pParameters: &root_parameter,
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let mut signature_blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        #[link(name = "d3d12")]
        extern "system" {
            fn D3D12SerializeRootSignature(
                pRootSignature: *const D3D12_ROOT_SIGNATURE_DESC,
                Version: u32,
                ppBlob: *mut Option<ID3DBlob>,
                ppErrorBlob: *mut Option<ID3DBlob>,
            ) -> i32;
        }

        let hr = D3D12SerializeRootSignature(&root_sig_desc, 1, &mut signature_blob, &mut error_blob);

        if hr < 0 {
            if let Some(err) = error_blob {
                let err_ptr = err.GetBufferPointer();
                let err_size = err.GetBufferSize();
                let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                println!("[Renderer] Root signature error: {}", String::from_utf8_lossy(err_msg));
            }
            std::mem::forget(device);
            return std::ptr::null_mut();
        }

        let blob = match signature_blob {
            Some(b) => b,
            None => {
                std::mem::forget(device);
                return std::ptr::null_mut();
            }
        };

        let blob_data = blob.GetBufferPointer();
        let blob_size = blob.GetBufferSize();
        let blob_slice = std::slice::from_raw_parts(blob_data as *const u8, blob_size);

        let root_signature_result: windows_core::Result<ID3D12RootSignature> =
            device.CreateRootSignature(0, blob_slice);

        std::mem::forget(device);

        match root_signature_result {
            Ok(sig) => {
                let ptr = sig.as_raw() as *mut c_void;
                std::mem::forget(sig);
                ptr
            }
            Err(e) => {
                println!("[Renderer] CreateRootSignature failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }

    unsafe fn create_constant_buffer(&mut self) -> Result<()> {
        let buffer_size = std::mem::size_of::<ConstantBuffer>();
        self.constant_buffer = create_buffer(self.device, buffer_size, std::ptr::null());

        if self.constant_buffer.is_null() {
            return Err(anyhow!("Failed to create constant buffer"));
        }

        self.cb_gpu_addr = get_buffer_gpu_address(self.constant_buffer);

        let cb = ConstantBuffer {
            world: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            view: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            color: [1.0, 1.0, 1.0, 1.0],
        };

        update_subresource(self.constant_buffer, &cb as *const _ as *const c_void, buffer_size);
        Ok(())
    }

    unsafe fn create_depth_buffer(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::*;
        use windows_core::Interface;

        let device: ID3D12Device = std::mem::transmute_copy(&self.device);

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        };

        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: self.width as u64,
            Height: self.height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let depth_stencil_value = D3D12_DEPTH_STENCIL_VALUE {
            Depth: 1.0,
            Stencil: 0,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 {
                DepthStencil: depth_stencil_value,
            },
        };

        let mut depth_buffer: Option<ID3D12Resource> = None;

        match device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            Some(&clear_value),
            &mut depth_buffer,
        ) {
            Ok(_) => {
                if let Some(buffer) = depth_buffer {
                    self.depth_buffer = buffer.as_raw() as *mut c_void;
                    let dsv_start = GetCPUDescriptorHandleForHeapStart(self.dsv_heap);

                    if !create_depth_stencil_view(self.device, self.depth_buffer, dsv_start) {
                        std::mem::forget(buffer);
                        std::mem::forget(device);
                        return Err(anyhow!("Failed to create DSV"));
                    }

                    self.dsv_handle = dsv_start;
                    std::mem::forget(buffer);
                    std::mem::forget(device);
                    Ok(())
                } else {
                    std::mem::forget(device);
                    Err(anyhow!("Depth buffer is None"))
                }
            }
            Err(e) => {
                std::mem::forget(device);
                Err(anyhow!("Failed to create depth buffer: {:?}", e))
            }
        }
    }

    unsafe fn create_render_target_views(&mut self) -> Result<()> {
        let rtv_start = GetCPUDescriptorHandleForHeapStart(self.rtv_heap);

        for i in 0..2 {
            let buffer = swap_chain_get_buffer(self.swap_chain, i);
            if buffer.is_null() {
                return Err(anyhow!("Failed to get swap chain buffer {}", i));
            }

            let rtv_handle = rtv_start + (i as u64 * self.rtv_descriptor_size as u64);
            if !create_render_target_view(self.device, buffer, rtv_handle) {
                release_resource(buffer);
                return Err(anyhow!("Failed to create RTV for buffer {}", i));
            }

            self.rtv_handles.push(rtv_handle);
            release_resource(buffer);
        }

        Ok(())
    }

    pub fn begin_frame(&mut self) {
        if !self.initialized || self.headless_mode {
            return;
        }

        unsafe {
            if !begin_frame() {
                eprintln!("[Renderer] begin_frame failed");
                return;
            }

            let frame_idx = get_frame_index() as usize;
            if frame_idx < self.rtv_handles.len() {
                let clear_color = [0.1f32, 0.1, 0.15, 1.0];
                clear_render_target(self.rtv_handles[frame_idx], clear_color.as_ptr());
                clear_depth_stencil(self.dsv_handle, 1.0, 0);
                set_render_targets_with_depth(self.rtv_handles[frame_idx], self.dsv_handle, 1);
            }

            set_viewport(0.0, 0.0, self.width as f32, self.height as f32, 0.0, 1.0);
            set_scissor_rect(0, 0, self.width as i32, self.height as i32);

            if !self.default_pso.is_null() {
                set_graphics_pipeline(self.default_pso);
                if !self.root_signature.is_null() {
                    set_root_signature(self.root_signature);
                }
            }

            // Устанавливаем топологию
            set_primitive_topology(4);
        }
    }

    pub fn end_frame(&mut self) {
        if !self.initialized || self.headless_mode {
            return;
        }

        unsafe {
            end_frame();
            present_swap_chain(self.swap_chain, 1);
            wait_for_gpu();
        }
    }

    pub fn load_altex(&mut self, path: &str) -> Result<Vec<String>> {
        println!("[Renderer] Loading Altex: {}", path);

        unsafe {
            let altex = AltexFile::load(path)
                .map_err(|e| anyhow!("Failed to load Altex: {}", e))?;

            let mut mesh_names = Vec::new();

            for mesh in &altex.meshes {
                let mesh_name = altex.get_string(mesh.name_id).to_string();
                println!("[Renderer] Processing mesh: {} (offset: {}, count: {}, indices: {})",
                         mesh_name, mesh.vertex_offset, mesh.vertex_count, mesh.index_count);

                let start = mesh.vertex_offset as usize;
                let end = start + mesh.vertex_count as usize;

                if start >= altex.vertices.len() || end > altex.vertices.len() {
                    println!("[Renderer] ERROR: Vertex out of bounds!");
                    continue;
                }

                let vertex_data = &altex.vertices[start..end];
                let vb_size = vertex_data.len() * std::mem::size_of::<Vertex>();

                let vb = create_buffer(self.device, vb_size, std::ptr::null());
                if vb.is_null() {
                    println!("[Renderer] ERROR: Failed to create vertex buffer");
                    continue;
                }

                update_subresource(vb, vertex_data.as_ptr() as *const c_void, vb_size);

                let idx_start = mesh.index_offset as usize;
                let idx_end = idx_start + mesh.index_count as usize;

                if idx_start >= altex.indices.len() || idx_end > altex.indices.len() {
                    println!("[Renderer] ERROR: Index out of bounds!");
                    release_resource(vb);
                    continue;
                }

                let index_data = &altex.indices[idx_start..idx_end];
                let ib_size = index_data.len() * 4;

                let ib = create_buffer(self.device, ib_size, std::ptr::null());
                if ib.is_null() {
                    println!("[Renderer] ERROR: Failed to create index buffer");
                    release_resource(vb);
                    continue;
                }

                update_subresource(ib, index_data.as_ptr() as *const c_void, ib_size);

                let vb_gpu_addr = get_buffer_gpu_address(vb);
                let ib_gpu_addr = get_buffer_gpu_address(ib);

                self.loaded_meshes.insert(mesh_name.clone(), MeshResource {
                    vb, ib, vb_gpu_addr, ib_gpu_addr,
                    vb_size: vb_size as u32,
                    ib_size: ib_size as u32,
                    index_count: mesh.index_count,
                    vertex_stride: std::mem::size_of::<Vertex>() as u32,
                    material_id: mesh.material_id,
                });

                mesh_names.push(mesh_name.clone());
                println!("[Renderer] ✅ Loaded mesh: {} ({} verts, {} indices, stride={})",
                         mesh_name, mesh.vertex_count, mesh.index_count,
                         std::mem::size_of::<Vertex>());
            }

            self.loaded_altex_files.insert(path.to_string(), altex);
            Ok(mesh_names)
        }
    }

    pub fn render_mesh(&self, mesh_name: &str, transform: &Transform) {
        if !self.initialized {
            return;
        }

        if let Some(mesh) = self.loaded_meshes.get(mesh_name) {
            unsafe {
                // Обновляем constant buffer
                if !self.constant_buffer.is_null() && self.cb_gpu_addr != 0 {
                    let aspect = self.width as f32 / self.height as f32;

                    // Матрица проекции (перспективная)
                    let fov = 60.0_f32.to_radians();
                    let tan_half_fov = (fov * 0.5).tan();
                    let y_scale = 1.0 / tan_half_fov;
                    let x_scale = y_scale / aspect;
                    let z_near = 0.1;
                    let z_far = 1000.0;
                    let z_range = z_far - z_near;

                    let proj = [
                        [x_scale, 0.0, 0.0, 0.0],
                        [0.0, y_scale, 0.0, 0.0],
                        [0.0, 0.0, z_far / z_range, 1.0],
                        [0.0, 0.0, -z_near * z_far / z_range, 0.0],
                    ];

                    // Матрица вида (камера смотрит на начало координат)
                    let eye = [5.0_f32, 5.0, 10.0];
                    let target = [0.0_f32, 0.0, 0.0];
                    let up = [0.0_f32, 1.0, 0.0];

                    let forward = [
                        target[0] - eye[0],
                        target[1] - eye[1],
                        target[2] - eye[2],
                    ];
                    let len = (forward[0]*forward[0] + forward[1]*forward[1] + forward[2]*forward[2]).sqrt();
                    let forward = [forward[0]/len, forward[1]/len, forward[2]/len];

                    let right = [
                        forward[1] * up[2] - forward[2] * up[1],
                        forward[2] * up[0] - forward[0] * up[2],
                        forward[0] * up[1] - forward[1] * up[0],
                    ];
                    let len = (right[0]*right[0] + right[1]*right[1] + right[2]*right[2]).sqrt();
                    let right = [right[0]/len, right[1]/len, right[2]/len];

                    let new_up = [
                        right[1] * forward[2] - right[2] * forward[1],
                        right[2] * forward[0] - right[0] * forward[2],
                        right[0] * forward[1] - right[1] * forward[0],
                    ];

                    let view = [
                        [right[0], new_up[0], -forward[0], 0.0],
                        [right[1], new_up[1], -forward[1], 0.0],
                        [right[2], new_up[2], -forward[2], 0.0],
                        [
                            -right[0]*eye[0] - right[1]*eye[1] - right[2]*eye[2],
                            -new_up[0]*eye[0] - new_up[1]*eye[1] - new_up[2]*eye[2],
                            forward[0]*eye[0] + forward[1]*eye[1] + forward[2]*eye[2],
                            1.0
                        ],
                    ];

                    let cb = ConstantBuffer {
                        world: transform.to_matrix(),
                        view,
                        proj,
                        color: [1.0, 1.0, 1.0, 1.0],
                    };

                    update_subresource(
                        self.constant_buffer,
                        &cb as *const _ as *const c_void,
                        std::mem::size_of::<ConstantBuffer>()
                    );

                    set_root_constant_buffer_view(0, self.cb_gpu_addr);
                }

                // Устанавливаем вершинный и индексный буферы
                set_vertex_buffer(mesh.vb_gpu_addr, mesh.vb_size, mesh.vertex_stride);
                set_index_buffer(mesh.ib_gpu_addr, mesh.ib_size, 4);

                // Рисуем
                draw_indexed_instanced(mesh.index_count, 1, 0, 0, 0);
            }
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if !self.initialized || self.headless_mode || (self.width == width && self.height == height) {
            return Ok(());
        }

        println!("[Renderer] Resizing to {}x{}", width, height);

        unsafe {
            wait_for_gpu();
            self.rtv_handles.clear();

            if !self.depth_buffer.is_null() {
                release_resource(self.depth_buffer);
                self.depth_buffer = std::ptr::null_mut();
            }

            if resize_swap_chain(self.swap_chain, width, height) {
                self.width = width;
                self.height = height;
                self.create_render_target_views()?;
                self.create_depth_buffer()?;
            } else {
                return Err(anyhow!("Failed to resize swap chain"));
            }
        }

        Ok(())
    }

    pub fn cleanup(&mut self) {
        if !self.initialized { return; }

        unsafe {
            if !self.headless_mode {
                wait_for_gpu();
            }

            for (_, mesh) in self.loaded_meshes.drain() {
                if !mesh.vb.is_null() { release_resource(mesh.vb); }
                if !mesh.ib.is_null() { release_resource(mesh.ib); }
            }

            for (_, tex) in self.loaded_textures.drain() {
                if !tex.texture.is_null() { release_resource(tex.texture); }
                if !tex.srv_heap.is_null() { release_resource(tex.srv_heap); }
            }

            self.loaded_altex_files.clear();

            if !self.constant_buffer.is_null() { release_resource(self.constant_buffer); }
            if !self.default_pso.is_null() { release_resource(self.default_pso); }
            if !self.root_signature.is_null() { release_resource(self.root_signature); }
            if !self.depth_buffer.is_null() { release_resource(self.depth_buffer); }
            if !self.dsv_heap.is_null() { release_resource(self.dsv_heap); }
            if !self.rtv_heap.is_null() { release_resource(self.rtv_heap); }
            if !self.swap_chain.is_null() { release_resource(self.swap_chain); }
            if !self.queue.is_null() { release_resource(self.queue); }
            if !self.device.is_null() { release_resource(self.device); }

            force_cleanup();
        }

        self.initialized = false;
        println!("[Renderer] Cleanup done");
    }

    pub fn get_width(&self) -> u32 { self.width }
    pub fn get_height(&self) -> u32 { self.height }
    pub fn is_initialized(&self) -> bool { self.initialized }
    pub fn get_loaded_meshes(&self) -> Vec<String> {
        self.loaded_meshes.keys().cloned().collect()
    }
}

impl Drop for EditorRenderer {
    fn drop(&mut self) { self.cleanup(); }
}

#[derive(Clone)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl Transform {
    pub fn from_position(x: f32, y: f32, z: f32) -> Self {
        Self { position: [x, y, z], ..Default::default() }
    }

    pub fn from_position_array(pos: [f32; 3]) -> Self {
        Self { position: pos, ..Default::default() }
    }

    pub fn to_matrix(&self) -> [[f32; 4]; 4] {
        // Простая матрица трансформации (позиция + масштаб)
        [
            [self.scale[0], 0.0, 0.0, 0.0],
            [0.0, self.scale[1], 0.0, 0.0],
            [0.0, 0.0, self.scale[2], 0.0],
            [self.position[0], self.position[1], self.position[2], 1.0],
        ]
    }
}

// Импорт функции set_primitive_topology из alkash3d_rs
extern "C" {
    fn set_primitive_topology(topology: u32) -> bool;
}