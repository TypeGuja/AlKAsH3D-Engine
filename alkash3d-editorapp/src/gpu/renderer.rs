// src/gpu/renderer.rs
use std::sync::Arc;
use wgpu::*;
use winit::window::Window;
use crate::math::Vec3;
use crate::mesh::Mesh;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    view_position: [f32; 3],
    _padding: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex3D {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

pub struct GpuMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,
    pub index_count: u32,
    pub visible: bool,
}

pub struct GpuRenderer {
    pub device: Device,
    pub queue: Queue,
    surface: Surface<'static>,
    config: SurfaceConfiguration,
    pipeline: RenderPipeline,
    camera_buffer: Buffer,
    camera_bind_group: BindGroup,
    depth_texture: Texture,
    depth_view: TextureView,
    pub meshes: Vec<GpuMesh>,
    pub camera: CameraData,
    size: (u32, u32),
    _window: Arc<Window>,
}

pub struct CameraData {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl CameraData {
    pub fn new() -> Self {
        Self {
            position: Vec3::new(5.0, 5.0, 10.0),
            target: Vec3::ZERO,
            up: Vec3::UP,
            fov: 60.0_f32.to_radians(),
            aspect: 16.0 / 9.0,
            near: 0.1,
            far: 1000.0,
        }
    }

    fn view_matrix(&self) -> [[f32; 4]; 4] {
        let f = (self.target - self.position).normalize();
        let s = f.cross(self.up).normalize();
        let u = s.cross(f);
        [
            [s.x, u.x, -f.x, 0.0],
            [s.y, u.y, -f.y, 0.0],
            [s.z, u.z, -f.z, 0.0],
            [-s.dot(self.position), -u.dot(self.position), f.dot(self.position), 1.0],
        ]
    }

    fn proj_matrix(&self) -> [[f32; 4]; 4] {
        let f = 1.0 / (self.fov / 2.0).tan();
        [
            [f / self.aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, (self.far + self.near) / (self.near - self.far), -1.0],
            [0.0, 0.0, (2.0 * self.far * self.near) / (self.near - self.far), 0.0],
        ]
    }

    pub fn view_proj_matrix(&self) -> [[f32; 4]; 4] {
        let view = self.view_matrix();
        let proj = self.proj_matrix();
        let mut result = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    result[i][j] += proj[i][k] * view[k][j];
                }
            }
        }
        result
    }
}

impl GpuRenderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).map_err(|e| format!("Surface error: {}", e))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("No adapter found");

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("GPU Device"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    ..Default::default()
                },
            )
            .await
            .expect("Failed to create device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Shader
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Shader"),
            source: ShaderSource::Wgsl(
                r#"
                struct Camera {
                    view_proj: mat4x4<f32>,
                    view_position: vec3<f32>,
                }
                @group(0) @binding(0) var<uniform> camera: Camera;

                struct VertexOutput {
                    @builtin(position) clip_pos: vec4<f32>,
                    @location(0) world_pos: vec3<f32>,
                    @location(1) normal: vec3<f32>,
                    @location(2) color: vec3<f32>,
                }

                @vertex
                fn vs_main(
                    @location(0) pos: vec3<f32>,
                    @location(1) norm: vec3<f32>,
                    @location(2) col: vec3<f32>,
                ) -> VertexOutput {
                    var out: VertexOutput;
                    out.world_pos = pos;
                    out.normal = norm;
                    out.color = col;
                    out.clip_pos = camera.view_proj * vec4<f32>(pos, 1.0);
                    return out;
                }

                @fragment
                fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
                    let light_dir = normalize(vec3<f32>(-0.5, -1.0, -0.5));
                    let n = normalize(in.normal);
                    let diff = max(dot(n, -light_dir), 0.0) * 0.7 + 0.3;
                    return vec4<f32>(in.color * diff, 1.0);
                }
                "#.into()
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Camera Layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Camera Buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Camera BG"),
            layout: &bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex3D>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &[
                        VertexAttribute { format: VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                        VertexAttribute { format: VertexFormat::Float32x3, offset: 12, shader_location: 1 },
                        VertexAttribute { format: VertexFormat::Float32x3, offset: 24, shader_location: 2 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
            multiview: None,
            cache: None,
        });

        let depth_texture = device.create_texture(&TextureDescriptor {
            label: Some("Depth"),
            size: Extent3d { width: size.width, height: size.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&TextureViewDescriptor::default());

        Ok(Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            camera_buffer,
            camera_bind_group,
            depth_texture,
            depth_view,
            meshes: Vec::new(),
            camera: CameraData::new(),
            size: (size.width, size.height),
            _window: window,
        })
    }

    pub fn add_mesh(&mut self, mesh: &Mesh) {
        let mut vertices = Vec::new();
        for (i, v) in mesh.vertices.iter().enumerate() {
            let n = if i < mesh.normals.len() { mesh.normals[i] } else { Vec3::UP };
            vertices.push(Vertex3D {
                position: [v.x, v.y, v.z],
                normal: [n.x, n.y, n.z],
                color: [0.7, 0.7, 0.7],
            });
        }

        let vb = self.device.create_buffer(&BufferDescriptor {
            label: Some("VB"),
            size: (vertices.len() * std::mem::size_of::<Vertex3D>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        vb.slice(..).get_mapped_range_mut().copy_from_slice(bytemuck::cast_slice(&vertices));
        vb.unmap();

        let ib = self.device.create_buffer(&BufferDescriptor {
            label: Some("IB"),
            size: (mesh.indices.len() * 4) as u64,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        ib.slice(..).get_mapped_range_mut().copy_from_slice(bytemuck::cast_slice(&mesh.indices));
        ib.unmap();

        self.meshes.push(GpuMesh {
            vertex_buffer: vb,
            index_buffer: ib,
            index_count: mesh.indices.len() as u32,
            visible: true,
        });
    }

    pub fn render(&mut self) -> Result<(), SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&TextureViewDescriptor::default());

        let vp = self.camera.view_proj_matrix();
        let uniform = CameraUniform {
            view_proj: vp,
            view_position: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
            _padding: 0.0,
        };
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[uniform]));

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("Encoder") });
        {
            let mut rp = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations { load: LoadOp::Clear(Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }), store: StoreOp::Store },
                })],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.camera_bind_group, &[]);
            for mesh in &self.meshes {
                if !mesh.visible { continue; }
                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rp.set_index_buffer(mesh.index_buffer.slice(..), IndexFormat::Uint32);
                rp.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 { return; }
        self.size = (width, height);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_texture = self.device.create_texture(&TextureDescriptor {
            label: Some("Depth"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
        });
        self.depth_view = self.depth_texture.create_view(&TextureViewDescriptor::default());
    }
}