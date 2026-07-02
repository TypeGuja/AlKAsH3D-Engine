// src/engine/mod.rs
//! Основной движок Alkash3D

use std::sync::Arc;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_UINT, DXGI_FORMAT_UNKNOWN};
use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_WINDOW, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::*;
use crate::plugin::{PhysicsPlugin, LightPlugin, PhysicsConfig, LightConfig, GPULight, PhysicsBody, PhysicsContact};
use crate::math::{Mat4, Vec3, identity, translation, rotation_x, rotation_y, rotation_z, scaling};
use crate::camera::Camera;
use crate::constant_buffer::TransformConstants;
use crate::shader::ShaderBlob;
use crate::pso::PipelineState;

// ===================================================================
// Vertex definition for rendering
// ===================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 4],
    pub color: [f32; 4],
}

impl Vertex {
    pub const STRIDE: u32 = std::mem::size_of::<Vertex>() as u32;

    pub fn new(x: f32, y: f32, z: f32, r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            position: [x, y, z, 1.0],
            color: [r, g, b, a],
        }
    }
}

// ===================================================================
// Mesh - хранит геометрию
// ===================================================================

pub struct Mesh {
    pub vertex_buffer: Buffer,
    pub vertex_count: u32,
    pub index_buffer: Option<Buffer>,
    pub index_count: u32,
}

impl Mesh {
    pub fn from_vertices(vertices: &[Vertex]) -> Result<Self> {
        let vertex_data: Vec<u8> = vertices
            .iter()
            .flat_map(|v| {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&v.position[0].to_le_bytes());
                bytes.extend_from_slice(&v.position[1].to_le_bytes());
                bytes.extend_from_slice(&v.position[2].to_le_bytes());
                bytes.extend_from_slice(&v.position[3].to_le_bytes());
                bytes.extend_from_slice(&v.color[0].to_le_bytes());
                bytes.extend_from_slice(&v.color[1].to_le_bytes());
                bytes.extend_from_slice(&v.color[2].to_le_bytes());
                bytes.extend_from_slice(&v.color[3].to_le_bytes());
                bytes
            })
            .collect();

        let buffer = Buffer::create_vertex_buffer(&vertex_data, Vertex::STRIDE)?;

        Ok(Self {
            vertex_buffer: buffer,
            vertex_count: vertices.len() as u32,
            index_buffer: None,
            index_count: 0,
        })
    }

    pub fn from_vertices_and_indices(vertices: &[Vertex], indices: &[u32]) -> Result<Self> {
        let mut mesh = Self::from_vertices(vertices)?;
        mesh.index_buffer = Some(Buffer::create_index_buffer(indices)?);
        mesh.index_count = indices.len() as u32;
        Ok(mesh)
    }

    pub fn triangle() -> Result<Self> {
        let vertices = [
            Vertex::new(-0.8, -0.8, 0.5, 1.0, 0.0, 0.0, 1.0),
            Vertex::new(0.0, 0.8, 0.5, 0.0, 1.0, 0.0, 1.0),
            Vertex::new(0.8, -0.8, 0.5, 0.0, 0.0, 1.0, 1.0),
        ];
        Self::from_vertices(&vertices)
    }

    pub fn quad(x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Result<Self> {
        let half_w = width / 2.0;
        let half_h = height / 2.0;
        let left = x - half_w;
        let right = x + half_w;
        let top = y + half_h;
        let bottom = y - half_h;

        let vertices = [
            Vertex::new(left, bottom, 0.0, color[0], color[1], color[2], color[3]),
            Vertex::new(right, bottom, 0.0, color[0], color[1], color[2], color[3]),
            Vertex::new(left, top, 0.0, color[0], color[1], color[2], color[3]),
            Vertex::new(right, top, 0.0, color[0], color[1], color[2], color[3]),
        ];

        let indices = [0, 1, 2, 1, 3, 2];
        Self::from_vertices_and_indices(&vertices, &indices)
    }

    pub fn cube(size: f32) -> Result<Self> {
        let half = size / 2.0;

        let vertices = [
            // Front face
            Vertex::new(-half, -half, half, 1.0, 0.0, 0.0, 1.0),
            Vertex::new( half, -half, half, 1.0, 0.0, 0.0, 1.0),
            Vertex::new(-half,  half, half, 1.0, 0.0, 0.0, 1.0),
            Vertex::new( half,  half, half, 1.0, 0.0, 0.0, 1.0),
            // Back face
            Vertex::new(-half, -half, -half, 0.0, 1.0, 0.0, 1.0),
            Vertex::new( half, -half, -half, 0.0, 1.0, 0.0, 1.0),
            Vertex::new(-half,  half, -half, 0.0, 1.0, 0.0, 1.0),
            Vertex::new( half,  half, -half, 0.0, 1.0, 0.0, 1.0),
            // Top face
            Vertex::new(-half,  half, -half, 0.0, 0.0, 1.0, 1.0),
            Vertex::new( half,  half, -half, 0.0, 0.0, 1.0, 1.0),
            Vertex::new(-half,  half,  half, 0.0, 0.0, 1.0, 1.0),
            Vertex::new( half,  half,  half, 0.0, 0.0, 1.0, 1.0),
            // Bottom face
            Vertex::new(-half, -half, -half, 1.0, 1.0, 0.0, 1.0),
            Vertex::new( half, -half, -half, 1.0, 1.0, 0.0, 1.0),
            Vertex::new(-half, -half,  half, 1.0, 1.0, 0.0, 1.0),
            Vertex::new( half, -half,  half, 1.0, 1.0, 0.0, 1.0),
            // Right face
            Vertex::new( half, -half, -half, 1.0, 0.0, 1.0, 1.0),
            Vertex::new( half,  half, -half, 1.0, 0.0, 1.0, 1.0),
            Vertex::new( half, -half,  half, 1.0, 0.0, 1.0, 1.0),
            Vertex::new( half,  half,  half, 1.0, 0.0, 1.0, 1.0),
            // Left face
            Vertex::new(-half, -half, -half, 1.0, 0.5, 0.0, 1.0),
            Vertex::new(-half,  half, -half, 1.0, 0.5, 0.0, 1.0),
            Vertex::new(-half, -half,  half, 1.0, 0.5, 0.0, 1.0),
            Vertex::new(-half,  half,  half, 1.0, 0.5, 0.0, 1.0),
        ];

        let indices = [
            0,1,2, 1,3,2,  // front
            4,6,5, 5,6,7,  // back
            8,10,9, 9,10,11, // top
            12,13,14, 13,15,14, // bottom
            16,18,17, 17,18,19, // right
            20,21,22, 21,23,22, // left
        ];

        Self::from_vertices_and_indices(&vertices, &indices)
    }
}

// ===================================================================
// MeshInstance - экземпляр меша с трансформацией
// ===================================================================

#[derive(Debug, Clone)]
pub struct MeshInstance {
    pub mesh_index: usize,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl MeshInstance {
    pub fn new(mesh_index: usize) -> Self {
        Self {
            mesh_index,
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    pub fn at(mut self, x: f32, y: f32, z: f32) -> Self {
        self.position = [x, y, z];
        self
    }

    pub fn rotated(mut self, x: f32, y: f32, z: f32) -> Self {
        self.rotation = [x, y, z];
        self
    }

    pub fn scaled(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = [x, y, z];
        self
    }

    pub fn transform_matrix(&self) -> Mat4 {
        let translation = Mat4::from_translation(Vec3::new(
            self.position[0],
            self.position[1],
            self.position[2],
        ));

        // Поворот в порядке ZYX (как в старом коде)
        let rot_z = Mat4::from_rotation_z(self.rotation[2]);
        let rot_y = Mat4::from_rotation_y(self.rotation[1]);
        let rot_x = Mat4::from_rotation_x(self.rotation[0]);
        let rotation = rot_z * rot_y * rot_x;

        let scale = Mat4::from_scale(Vec3::new(
            self.scale[0],
            self.scale[1],
            self.scale[2],
        ));

        translation * rotation * scale
    }
}

// ===================================================================
// Main Engine - С ВСТРОЕННЫМ ОКНОМ
// ===================================================================

pub struct AlkashEngine {
    // Рендеринг
    pub renderer: Option<Renderer>,
    pub meshes: Vec<Mesh>,
    pub mesh_instances: Vec<MeshInstance>,
    pub root_signature: Option<ID3D12RootSignature>,
    pub pipeline_state: Option<ID3D12PipelineState>,
    pub vs: Option<ShaderBlob>,
    pub ps: Option<ShaderBlob>,

    // 3D рендеринг
    pub camera: Camera,
    pub constant_buffer: Option<Buffer>,
    pub transform_constants: TransformConstants,

    // Планировщик
    pub scheduler: Arc<EngineScheduler>,

    // Плагины
    pub physics: Option<PhysicsPlugin>,
    pub lights: Option<LightPlugin>,

    // Окно
    hwnd: Option<HWND>,
    running: bool,

    // Настройки
    width: u32,
    height: u32,
    clear_color: [f32; 4],

    shutdown_in_progress: bool,
}

impl AlkashEngine {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            scheduler: Arc::new(EngineScheduler::new()),
            renderer: None,
            meshes: Vec::new(),
            mesh_instances: Vec::new(),
            root_signature: None,
            pipeline_state: None,
            vs: None,
            ps: None,
            camera: Camera::new(width, height),
            constant_buffer: None,
            transform_constants: TransformConstants::new(),
            physics: None,
            lights: None,
            hwnd: None,
            running: false,
            width,
            height,
            clear_color: [0.05, 0.05, 0.1, 1.0],
            shutdown_in_progress: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = [r, g, b, a];
    }

    pub fn init(&mut self) -> Result<()> {
        println!("[ENGINE] Initializing Alkash3D Engine v{}...", VERSION);

        // 1. Создаем окно
        self.create_window()?;
        println!("[ENGINE] ✓ Window created");

        // 2. Инициализируем DirectX 12
        unsafe {
            D3D12Device::create()?;
            println!("[ENGINE] ✓ Device created");

            CommandQueue::create()?;
            println!("[ENGINE] ✓ Command queue created");

            let hwnd = self.hwnd.unwrap();
            SwapChain::create(hwnd.0 as isize, self.width, self.height, 2)?;
            println!("[ENGINE] ✓ Swap chain created");

            CommandList::create_allocators(2)?;
            println!("[ENGINE] ✓ Command allocators created");

            let fence = create_fence()?;
            {
                let mut state = STATE.lock().unwrap();
                state.fence = Some(fence);
                state.fence_values = vec![0, 0];
            }
            println!("[ENGINE] ✓ Fence created");

            let renderer = Renderer::new(self.width, self.height, 2)?;
            self.renderer = Some(renderer);
            println!("[ENGINE] ✓ Renderer created");
        }

        // 3. Компилируем шейдеры
        self.compile_default_shaders()?;

        // 4. Создаём корневую сигнатуру (с константным буфером)
        self.create_root_signature()?;

        // 5. Создаём PSO
        self.create_pipeline_state()?;

        // 6. Создаём константный буфер
        self.constant_buffer = Some(TransformConstants::create_buffer()?);
        println!("[ENGINE] ✓ Constant buffer created");

        // Показываем окно
        unsafe {
            ShowWindow(self.hwnd.unwrap(), SW_SHOW);
            UpdateWindow(self.hwnd.unwrap());
        }

        self.running = true;
        println!("[ENGINE] ✓ Initialization complete");
        Ok(())
    }

    fn create_window(&mut self) -> Result<()> {
        unsafe {
            let hinstance = GetModuleHandleA(None)?;
            let window_class = "ALKASH3D_WINDOW\0".as_ptr();

            let wc = WNDCLASSA {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(Self::wndproc_static),
                hInstance: hinstance.into(),
                lpszClassName: PCSTR(window_class),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as _),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };

            RegisterClassA(&wc);

            let hwnd = CreateWindowExA(
                WINDOW_EX_STYLE::default(),
                PCSTR(window_class),
                PCSTR(b"Alkash3D Engine - DirectX 12\0".as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                self.width as i32,
                self.height as i32,
                None,
                None,
                Some(HINSTANCE::from(hinstance)),
                Some(self as *mut Self as _),
            )?;

            self.hwnd = Some(hwnd);
            println!("[ENGINE] Window created: HWND=0x{:X}", hwnd.0 as usize);
        }

        Ok(())
    }

    extern "system" fn wndproc_static(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let cs = lparam.0 as *const CREATESTRUCTA;
                let engine = (*cs).lpCreateParams as *mut AlkashEngine;
                SetWindowLongPtrA(hwnd, GWLP_USERDATA, engine as isize);
            }

            let engine = GetWindowLongPtrA(hwnd, GWLP_USERDATA) as *mut AlkashEngine;
            if !engine.is_null() {
                let engine_ref = &mut *engine;
                return engine_ref.wndproc(hwnd, msg, wparam, lparam);
            }

            DefWindowProcA(hwnd, msg, wparam, lparam)
        }
    }

    fn wndproc(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_CLOSE => {
                    println!("[ENGINE] WM_CLOSE received - window close button clicked");
                    self.running = false;
                    // Уничтожаем окно
                    DestroyWindow(hwnd);
                    // Отправляем WM_QUIT для выхода из цикла сообщений
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    println!("[ENGINE] WM_DESTROY received - window being destroyed");
                    self.running = false;
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                WM_KEYDOWN => {
                    if wparam.0 == 0x1B { // ESC
                        println!("[ENGINE] ESC pressed - closing window");
                        self.running = false;
                        // Отправляем WM_CLOSE для корректного закрытия
                        PostMessageA(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    // WASD управление камерой
                    let speed = 0.1;
                    if wparam.0 == 0x57 { // W
                        self.camera.move_forward(speed);
                    }
                    if wparam.0 == 0x53 { // S
                        self.camera.move_forward(-speed);
                    }
                    if wparam.0 == 0x41 { // A
                        self.camera.move_right(-speed);
                    }
                    if wparam.0 == 0x44 { // D
                        self.camera.move_right(speed);
                    }
                    LRESULT(0)
                }
                WM_SIZE => {
                    let width = (lparam.0 & 0xFFFF) as u32;
                    let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                    if width > 0 && height > 0 {
                        self.width = width;
                        self.height = height;
                        self.camera.set_aspect(width, height);
                        let state = STATE.lock().unwrap();
                        if let Some(swap_chain) = &state.swap_chain {
                            let _ = unsafe { swap_chain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)) };
                        }
                    }
                    LRESULT(0)
                }
                _ => DefWindowProcA(hwnd, msg, wparam, lparam),
            }
        }
    }

    pub fn process_messages(&mut self) {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageA(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    println!("[ENGINE] WM_QUIT received - exiting message loop");
                    self.running = false;
                    break;
                }
                // Обработка сообщения WM_DESTROY через окно
                if msg.message == WM_DESTROY {
                    println!("[ENGINE] WM_DESTROY received in message loop");
                    self.running = false;
                    // Позволяем DefWindowProc обработать сообщение
                    let _ = DefWindowProcA(msg.hwnd, msg.message, msg.wParam, msg.lParam);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageA(&msg);
            }
        }
    }

    fn compile_default_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        cbuffer TransformConstants : register(b0) {
            float4x4 modelViewProj;
            float4x4 model;
            float4x4 view;
            float4x4 proj;
            float4 cameraPos;
            float4 lightDir;
            float4 lightColor;
            float4 ambientColor;
        };

        struct VS_INPUT {
            float4 pos : POSITION;
            float4 color : COLOR;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
            float3 worldPos : TEXCOORD0;
            float3 normal : TEXCOORD1;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            output.pos = mul(modelViewProj, input.pos);
            output.color = input.color;
            output.worldPos = mul(model, input.pos).xyz;
            output.normal = float3(0.0, 0.0, 1.0);
            return output;
        }
        "#;

        let ps_source = r#"
        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
            float3 worldPos : TEXCOORD0;
            float3 normal : TEXCOORD1;
        };
        float4 main(PS_INPUT input) : SV_TARGET {
            float3 lightDir = normalize(float3(0.0, -1.0, 0.0));
            float3 normal = normalize(input.normal);
            float diff = max(dot(normal, -lightDir), 0.0);
            float ambient = 0.2;
            float brightness = ambient + diff * 0.8;
            return float4(input.color.rgb * brightness, input.color.a);
        }
        "#;

        self.vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Default shaders compiled");
        Ok(())
    }

    fn create_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];

        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let mut signature_serialized = None;
        let mut error_blob = None;

        unsafe {
            let hr = D3D12SerializeRootSignature(
                &root_signature_desc,
                D3D_ROOT_SIGNATURE_VERSION_1,
                &mut signature_serialized,
                Some(&mut error_blob),
            );

            if hr.is_err() {
                if let Some(err) = error_blob {
                    let err_data = std::slice::from_raw_parts(
                        err.GetBufferPointer() as *const u8,
                        err.GetBufferSize(),
                    );
                    eprintln!("Root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Root signature created (with CBV)");
        Ok(())
    }

    fn create_pipeline_state(&mut self) -> Result<()> {
        let vs = self.vs.as_ref().unwrap();
        let ps = self.ps.as_ref().unwrap();
        let root_sig = self.root_signature.as_ref().unwrap();

        let pso = PipelineState::create_graphics(
            vs, ps, root_sig,
            Vertex::STRIDE,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
        )?;

        self.pipeline_state = Some(pso);
        println!("[ENGINE] ✓ Pipeline state created");
        Ok(())
    }

    pub fn add_mesh(&mut self, mesh: Mesh) -> usize {
        self.meshes.push(mesh);
        println!("[ENGINE] Mesh added, total meshes: {}", self.meshes.len());
        self.meshes.len() - 1
    }

    pub fn add_triangle(&mut self) -> usize {
        let mesh = Mesh::triangle().unwrap();
        self.add_mesh(mesh)
    }

    pub fn add_quad(&mut self, x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> usize {
        let mesh = Mesh::quad(x, y, width, height, color).unwrap();
        self.add_mesh(mesh)
    }

    pub fn add_cube(&mut self, size: f32) -> usize {
        let mesh = Mesh::cube(size).unwrap();
        self.add_mesh(mesh)
    }

    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.mesh_instances.clear();
        println!("[ENGINE] All meshes and instances cleared");
    }

    pub fn render_frame(&mut self) -> Result<bool> {
        let renderer = self.renderer.as_ref().unwrap();

        let frame_index = {
            let state = STATE.lock().unwrap();
            state.frame_index as usize
        };

        let allocator = CommandList::get_allocator(frame_index)
            .ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

        unsafe {
            allocator.Reset();
        }

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let cmd_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?
        };

        let rtv_handle = renderer.render_target_views[frame_index];
        let dsv_handle = renderer.depth_stencil_view;

        unsafe {
            cmd_list.OMSetRenderTargets(1, Some(&rtv_handle), false, Some(&dsv_handle));
            cmd_list.ClearRenderTargetView(rtv_handle, &self.clear_color, None);
            cmd_list.ClearDepthStencilView(dsv_handle, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);

            let scissor = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            cmd_list.RSSetScissorRects(&[scissor]);

            cmd_list.SetPipelineState(Some(self.pipeline_state.as_ref().unwrap()));
            cmd_list.SetGraphicsRootSignature(Some(self.root_signature.as_ref().unwrap()));

            if let Some(cb) = &self.constant_buffer {
                let gpu_handle = cb.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootConstantBufferView(0, gpu_handle);
            }

            // ========== 3D РЕНДЕР С ТРАНСФОРМАЦИЯМИ ==========
            if !self.mesh_instances.is_empty() {
                let view = self.camera.view_matrix();
                let proj = self.camera.projection_matrix();

                for instance in &self.mesh_instances {
                    if instance.mesh_index >= self.meshes.len() {
                        continue;
                    }

                    let mesh = &self.meshes[instance.mesh_index];
                    let model = instance.transform_matrix();

                    // Правильный порядок для DirectX: proj * view * model
                    let model_view_proj = proj * view * model;

                    // Преобразуем матрицы в массивы для константного буфера
                    self.transform_constants.model_view_proj = model_view_proj.to_cols_array_2d();
                    self.transform_constants.model = model.to_cols_array_2d();
                    self.transform_constants.view = view.to_cols_array_2d();
                    self.transform_constants.proj = proj.to_cols_array_2d();
                    self.transform_constants.camera_pos = [
                        self.camera.position.x,
                        self.camera.position.y,
                        self.camera.position.z,
                        1.0,
                    ];

                    if let Some(cb) = &self.constant_buffer {
                        let _ = self.transform_constants.update(cb);
                    }

                    let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                        BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                        SizeInBytes: mesh.vertex_buffer.size as u32,
                        StrideInBytes: Vertex::STRIDE,
                    };
                    cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
                    cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                    if let Some(index_buffer) = &mesh.index_buffer {
                        let index_view = D3D12_INDEX_BUFFER_VIEW {
                            BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                            SizeInBytes: index_buffer.size as u32,
                            Format: DXGI_FORMAT_R32_UINT,
                        };
                        cmd_list.IASetIndexBuffer(Some(&index_view));
                        cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
                    } else {
                        cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
                    }
                }
            } else {
                // 2D режим - без трансформаций
                let identity = identity();

                for mesh in &self.meshes {
                    self.transform_constants.model_view_proj = identity.to_cols_array_2d();
                    self.transform_constants.model = identity.to_cols_array_2d();
                    self.transform_constants.view = identity.to_cols_array_2d();
                    self.transform_constants.proj = identity.to_cols_array_2d();

                    if let Some(cb) = &self.constant_buffer {
                        let _ = self.transform_constants.update(cb);
                    }

                    let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                        BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                        SizeInBytes: mesh.vertex_buffer.size as u32,
                        StrideInBytes: Vertex::STRIDE,
                    };
                    cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
                    cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                    if let Some(index_buffer) = &mesh.index_buffer {
                        let index_view = D3D12_INDEX_BUFFER_VIEW {
                            BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                            SizeInBytes: index_buffer.size as u32,
                            Format: DXGI_FORMAT_R32_UINT,
                        };
                        cmd_list.IASetIndexBuffer(Some(&index_view));
                        cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
                    } else {
                        cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
                    }
                }
            }

            cmd_list.Close()?;
        }

        let queue = {
            let state = STATE.lock().unwrap();
            state.command_queue.as_ref().unwrap().clone()
        };

        let cmd_lists: &[Option<ID3D12CommandList>] = &[Some(cmd_list.into())];
        unsafe {
            queue.ExecuteCommandLists(cmd_lists);
        }

        let swap_chain = {
            let state = STATE.lock().unwrap();
            state.swap_chain.as_ref().unwrap().clone()
        };
        unsafe {
            let _ = swap_chain.Present(1, DXGI_PRESENT(0));
        }

        let fence = {
            let state = STATE.lock().unwrap();
            state.fence.as_ref().unwrap().clone()
        };

        let fence_value = {
            let mut state = STATE.lock().unwrap();
            state.fence_values[frame_index] += 1;
            state.fence_values[frame_index]
        };

        unsafe {
            queue.Signal(&fence, fence_value)?;
            while fence.GetCompletedValue() < fence_value {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        {
            let mut state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
            }
        }

        Ok(true)
    }

    // ================================================================
    // МЕТОДЫ ПЛАГИНОВ
    // ================================================================

    pub fn init_physics(&mut self, config: PhysicsConfig) -> Result<()> {
        match PhysicsPlugin::load("plugins/inertial.dll", config) {
            Ok(plugin) => {
                self.physics = Some(plugin);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to load physics plugin: {}", e);
                Err(Error::from_hresult(HRESULT(1)))
            }
        }
    }

    pub fn init_lights(&mut self, device_ptr: *mut std::ffi::c_void, config: LightConfig) -> Result<()> {
        match LightPlugin::load("plugins/firstfires.dll", device_ptr, config) {
            Ok(plugin) => {
                self.lights = Some(plugin);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to load light plugin: {}", e);
                Err(Error::from_hresult(HRESULT(1)))
            }
        }
    }

    pub fn add_physics_body(&mut self, body: PhysicsBody) -> Option<i32> {
        self.physics.as_mut().map(|p| p.add_body(&body))
    }

    pub fn add_sphere_body(&mut self, x: f32, y: f32, z: f32, mass: f32) -> Option<i32> {
        let body = PhysicsBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.5,
            friction: 0.5,
            linear_damping: 0.01,
            angular_damping: 0.01,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
        };
        self.physics.as_mut().map(|p| p.add_body(&body))
    }

    pub fn add_street_light(&mut self, x: f32, y: f32, z: f32) -> Option<u32> {
        let light = GPULight {
            position: [x, y, z, 0.0],
            color: [1.0, 0.85, 0.6, 2.5],
            direction: [0.0, -1.0, 0.0, 25.0],
            params: [std::f32::consts::PI, 2.0, 0.0, 0.0],
        };
        self.lights.as_mut().map(|l| l.add_light(&light))
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        self.lights.as_ref().map(|l| l.get_gpu_lights()).unwrap_or(&[])
    }

    pub fn get_contacts(&self) -> &[PhysicsContact] {
        self.physics.as_ref().map(|p| p.get_contacts()).unwrap_or(&[])
    }

    pub fn update(&mut self, dt: f32, gravity: f32, camera_pos: [f32; 3], view_proj: [f32; 16]) {
        self.scheduler.reset_budget();

        if let Some(physics) = &mut self.physics {
            physics.update(dt, gravity);
        }

        if let Some(lights) = &mut self.lights {
            lights.cull(camera_pos, &view_proj, dt);
        }
    }

    pub fn shutdown(&mut self) {
        // Защита от двойного вызова
        if self.shutdown_in_progress {
            println!("[ENGINE] Shutdown already in progress");
            return;
        }
        self.shutdown_in_progress = true;

        println!("[ENGINE] Shutting down...");
        self.running = false;

        // ===== 1. Ждем завершения всех GPU операций =====
        {
            let state = STATE.lock().unwrap();
            if let (Some(queue), Some(fence)) = (&state.command_queue, &state.fence) {
                let fence_value = 100;
                unsafe {
                    println!("[ENGINE] Signaling fence...");
                    let _ = queue.Signal(fence, fence_value);
                    println!("[ENGINE] Waiting for GPU to finish...");

                    let start = std::time::Instant::now();
                    while fence.GetCompletedValue() < fence_value {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        if start.elapsed() > std::time::Duration::from_secs(5) {
                            println!("[ENGINE] WARNING: GPU timeout, forcing shutdown");
                            break;
                        }
                    }
                    println!("[ENGINE] GPU idle");
                }
            }
        }

        // ===== 2. Очищаем меши и экземпляры =====
        println!("[ENGINE] Clearing meshes...");
        self.meshes.clear();
        self.mesh_instances.clear();

        // ===== 3. Явно сбрасываем ресурсы в правильном порядке =====
        println!("[ENGINE] Releasing resources...");

        self.constant_buffer = None;
        self.vs = None;
        self.ps = None;
        self.pipeline_state = None;
        self.root_signature = None;
        self.renderer = None;

        // ===== 4. Сбрасываем глобальное состояние =====
        println!("[ENGINE] Resetting global state...");
        {
            let mut state = STATE.lock().unwrap();

            state.fence = None;
            state.fence_values.clear();
            state.command_allocators.clear();
            state.command_list = None;

            if let Some(swap_chain) = &state.swap_chain {
                unsafe {
                    let _ = swap_chain.SetFullscreenState(false, None);
                }
            }
            state.swap_chain = None;
            state.command_queue = None;
            state.device = None;
            state.descriptor_heaps.clear();
            state.root_signature = None;
            state.current_pso = None;
            state.bound_vertex_buffers.clear();
            state.bound_index_buffer = None;
            state.scheduler = None;
        }

        // ===== 5. Уничтожаем окно (если еще не уничтожено) =====
        unsafe {
            if let Some(hwnd) = self.hwnd {
                println!("[ENGINE] Destroying window...");
                // Проверяем, существует ли еще окно
                if IsWindow(Some(hwnd)).as_bool() {
                    DestroyWindow(hwnd);
                }
                self.hwnd = None;
            }
        }

        self.shutdown_in_progress = false;
        println!("[ENGINE] Shutdown complete");
    }
}

impl Drop for AlkashEngine {
    fn drop(&mut self) {
        // Если движок еще не был завершен, завершаем его
        if self.running {
            self.shutdown();
        }
    }
}