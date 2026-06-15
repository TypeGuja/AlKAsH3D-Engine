// src/engine/mod.rs
//! Основной движок Alkash3D

use std::sync::Arc;
use windows::core::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;

use crate::*;

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
// Main Engine
// ===================================================================

pub struct AlkashEngine {
    pub scheduler: Arc<EngineScheduler>,
    pub renderer: Option<Renderer>,
    pub meshes: Vec<Mesh>,
    pub root_signature: Option<ID3D12RootSignature>,
    pub pipeline_state: Option<ID3D12PipelineState>,
    pub vs: Option<ShaderBlob>,
    pub ps: Option<ShaderBlob>,
    width: u32,
    height: u32,
    clear_color: [f32; 4],
}

impl AlkashEngine {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            scheduler: Arc::new(EngineScheduler::new()),
            renderer: None,
            meshes: Vec::new(),
            root_signature: None,
            pipeline_state: None,
            vs: None,
            ps: None,
            width,
            height,
            clear_color: [0.05, 0.05, 0.1, 1.0],
        }
    }

    pub fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = [r, g, b, a];
    }

    pub fn init(&mut self, hwnd: isize) -> Result<()> {
        println!("[ENGINE] Initializing Alkash3D Engine v{}...", VERSION);

        D3D12Device::create()?;
        println!("[ENGINE] ✓ Device created");

        CommandQueue::create()?;
        println!("[ENGINE] ✓ Command queue created");

        SwapChain::create(hwnd, self.width, self.height, 2)?;
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

        self.compile_default_shaders()?;
        self.create_root_signature()?;
        self.create_pipeline_state()?;

        println!("[ENGINE] ✓ Initialization complete");
        Ok(())
    }

    fn compile_default_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
        struct VS_INPUT {
            float4 pos : POSITION;
            float4 color : COLOR;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            output.pos = input.pos;
            output.color = input.color;
            return output;
        }
        "#;

        let ps_source = r#"
        struct PS_INPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
        };
        float4 main(PS_INPUT input) : SV_TARGET {
            return input.color;
        }
        "#;

        self.vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Default shaders compiled");
        Ok(())
    }

    fn create_root_signature(&mut self) -> Result<()> {
        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 0,
            pParameters: std::ptr::null(),
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

        println!("[ENGINE] ✓ Root signature created");
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
        println!("[ENGINE] All meshes cleared");
    }

    pub fn render_frame(&mut self) -> Result<bool> {
        let renderer = self.renderer.as_ref().unwrap();

        let frame_index = {
            let state = STATE.lock().unwrap();
            state.frame_index as usize
        };

        // Получаем аллокатор для текущего кадра
        let allocator = CommandList::get_allocator(frame_index)
            .ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

        unsafe {
            allocator.Reset();
        }

        // Создаём command list с указанием типа
        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        // Явно указываем тип для cmd_list
        let cmd_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?
        };

        // Очищаем render target
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

            // Рисуем все меши
            for mesh in &self.meshes {
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

            cmd_list.Close()?;
        }

        // Выполняем command list
        let queue = {
            let state = STATE.lock().unwrap();
            state.command_queue.as_ref().unwrap().clone()
        };

        let cmd_lists: &[Option<ID3D12CommandList>] = &[Some(cmd_list.into())];
        unsafe {
            queue.ExecuteCommandLists(cmd_lists);
        }

        // Present
        let swap_chain = {
            let state = STATE.lock().unwrap();
            state.swap_chain.as_ref().unwrap().clone()
        };
        unsafe {
            let _ = swap_chain.Present(1, DXGI_PRESENT(0));
        }

        // Ждём fence
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

        // Обновляем frame index
        {
            let mut state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
            }
        }

        Ok(true)
    }

    pub fn shutdown(&mut self) {
        println!("[ENGINE] Shutting down...");

        let state = STATE.lock().unwrap();
        if let Some(queue) = state.command_queue.as_ref() {
            if let Some(fence) = state.fence.as_ref() {
                unsafe {
                    let _ = queue.Signal(fence, 100);
                }
            }
        }
        drop(state);

        self.meshes.clear();
        self.renderer = None;
        self.pipeline_state = None;
        self.root_signature = None;

        println!("[ENGINE] Shutdown complete");
    }
}