// src/gpu/renderer.rs - ИСПРАВЛЕННЫЙ
use wgpu::*;
use winit::window::Window;
use std::sync::Arc;
use crate::math::{Vec3, Transform};
use super::camera::Camera;
use super::pipeline::PipelineManager;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct InstanceData {
    model_matrix: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
    material_id: u32,
    _padding: [u32; 3],
}

pub struct RenderMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,
    pub index_count: u32,
    pub instance_buffer: Buffer,
    pub instance_count: u32,
    pub material_id: u32,
    pub visible: bool,
}

pub struct Renderer {
    pub device: Device,
    pub queue: Queue,
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
    pub size: (u32, u32),
    pub depth_texture: Texture,
    pub depth_view: TextureView,
    pub camera: Camera,
    pub pipelines: PipelineManager,
    pub meshes: Vec<RenderMesh>,
    pub camera_bind_group: BindGroup,
    pub light_bind_group: BindGroup,
    pub material_bind_groups: Vec<BindGroup>,
    pub _window: Arc<Window>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, Box<dyn std::error::Error>> {
        let size = window.inner_size();
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("Failed to request adapter: {}", e))?;
        
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("AlKAsH3D Device"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    experimental_features: Default::default(),
                    memory_hints: Default::default(),
                    trace: Default::default(),
                },
            )
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

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

        let (depth_texture, depth_view) = Self::create_depth_texture(&device, &config);

        let pipelines = PipelineManager::new(&device, format);
        let mut camera = Camera::new(size.width as f32 / size.height as f32);

        let camera_bind_group = Self::create_camera_bind_group(
            &device,
            &queue,
            &pipelines.camera_bind_group_layout,
            &mut camera,
        );

        let light_bind_group = Self::create_light_bind_group(
            &device,
            &pipelines.light_bind_group_layout,
        );

        Ok(Self {
            device,
            queue,
            surface,
            config,
            size: (size.width, size.height),
            depth_texture,
            depth_view,
            camera,
            pipelines,
            meshes: Vec::new(),
            camera_bind_group,
            light_bind_group,
            material_bind_groups: Vec::new(),
            _window: window,
        })
    }

    fn create_depth_texture(device: &Device, config: &SurfaceConfiguration) -> (Texture, TextureView) {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Depth Texture"),
            size: Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&TextureViewDescriptor::default());
        (texture, view)
    }

    fn create_camera_bind_group(
        device: &Device,
        queue: &Queue,
        layout: &BindGroupLayout,
        camera: &mut Camera,
    ) -> BindGroup {
        if camera.buffer.is_none() {
            camera.buffer = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Camera Buffer"),
                size: std::mem::size_of::<super::camera::CameraUniform>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        if let Some(ref buffer) = camera.buffer {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[camera.uniform]));
        }

        device.create_bind_group(&BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: camera.buffer.as_ref().unwrap().as_entire_binding(),
                },
            ],
        })
    }

    fn create_light_bind_group(device: &Device, layout: &BindGroupLayout) -> BindGroup {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Light Buffer"),
            size: 64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        device.create_bind_group(&BindGroupDescriptor {
            label: Some("Light Bind Group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
            ],
        })
    }

    pub fn add_mesh(
        &mut self,
        vertices: &[Vec3],
        indices: &[u32],
        normals: &[Vec3],
        material_id: u32,
    ) -> usize {
        let vertex_data = Self::pack_vertices(vertices, normals);

        let vertex_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (vertex_data.len() * std::mem::size_of::<f32>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });

        let index_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Index Buffer"),
            size: (indices.len() * std::mem::size_of::<u32>()) as u64,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });

        let instance_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Instance Buffer"),
            size: std::mem::size_of::<InstanceData>() as u64 * 1024,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Записываем данные в буферы через mapped_at_creation
        vertex_buffer.slice(..).get_mapped_range_mut().copy_from_slice(bytemuck::cast_slice(&vertex_data));
        vertex_buffer.unmap();

        index_buffer.slice(..).get_mapped_range_mut().copy_from_slice(bytemuck::cast_slice(indices));
        index_buffer.unmap();

        let mesh = RenderMesh {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            instance_buffer,
            instance_count: 0,
            material_id,
            visible: true,
        };

        let id = self.meshes.len();
        self.meshes.push(mesh);
        id
    }

    fn pack_vertices(vertices: &[Vec3], normals: &[Vec3]) -> Vec<f32> {
        let mut data = Vec::with_capacity(vertices.len() * 8);
        for (i, vertex) in vertices.iter().enumerate() {
            data.push(vertex.x);
            data.push(vertex.y);
            data.push(vertex.z);
            if i < normals.len() {
                data.push(normals[i].x);
                data.push(normals[i].y);
                data.push(normals[i].z);
            } else {
                data.push(0.0);
                data.push(1.0);
                data.push(0.0);
            }
            data.push(0.0); // UV u
            data.push(0.0); // UV v
        }
        data
    }

    pub fn update_mesh_instances(&mut self, mesh_id: usize, instances: &[Transform]) {
        if mesh_id >= self.meshes.len() { return; }

        let mesh = &mut self.meshes[mesh_id];
        let instance_data: Vec<InstanceData> = instances
            .iter()
            .map(|transform| {
                let model_matrix = transform.to_matrix();
                let normal_matrix = Self::calculate_normal_matrix(&model_matrix);
                InstanceData {
                    model_matrix,
                    normal_matrix,
                    material_id: mesh.material_id,
                    _padding: [0; 3],
                }
            })
            .collect();

        mesh.instance_count = instance_data.len() as u32;
        if mesh.instance_count > 0 {
            self.queue.write_buffer(
                &mesh.instance_buffer,
                0,
                bytemuck::cast_slice(&instance_data),
            );
        }
    }

    fn calculate_normal_matrix(model_matrix: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
        let mut normal_matrix = [[0.0f32; 4]; 4];
        for i in 0..3 {
            for j in 0..3 {
                normal_matrix[i][j] = model_matrix[j][i];
            }
        }
        normal_matrix[3][3] = 1.0;
        normal_matrix
    }

    pub fn resize(&mut self, new_size: (u32, u32)) {
        if new_size.0 == 0 || new_size.1 == 0 { return; }
        self.size = new_size;
        self.config.width = new_size.0;
        self.config.height = new_size.1;
        self.surface.configure(&self.device, &self.config);
        let (depth_texture, depth_view) = Self::create_depth_texture(&self.device, &self.config);
        self.depth_texture = depth_texture;
        self.depth_view = depth_view;
    }

    pub fn render(&mut self) -> Result<(), SurfaceError> {
        self.camera.update_view_matrix();

        if let Some(ref buffer) = self.camera.buffer {
            self.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[self.camera.uniform]));
        }

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Main Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            for mesh in &self.meshes {
                if !mesh.visible || mesh.instance_count == 0 { continue; }

                render_pass.set_pipeline(&self.pipelines.pbr_pipeline);
                render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, mesh.instance_buffer.slice(..));
                render_pass.set_index_buffer(mesh.index_buffer.slice(..), IndexFormat::Uint32);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_bind_group(1, &self.light_bind_group, &[]);

                if (mesh.material_id as usize) < self.material_bind_groups.len() {
                    render_pass.set_bind_group(
                        2,
                        &self.material_bind_groups[mesh.material_id as usize],
                        &[],
                    );
                }

                render_pass.draw_indexed(0..mesh.index_count, 0, 0..mesh.instance_count);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}