// src/gpu/renderer.rs - ОСНОВНОЙ РЕНДЕРЕР С ОПТИМИЗАЦИЯМИ
use crate::math::Vec3;
use crate::mesh::Mesh;
use std::num::NonZeroU64;
use std::sync::Arc;
pub use egui_wgpu::wgpu;
use wgpu::*;
use winit::window::Window;
use std::collections::HashMap;
use lru::LruCache;
use std::num::NonZeroUsize;
use crate::memory::{FrameAllocator, ObjectPool};

pub struct GpuRenderer {
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub pipeline: RenderPipeline,
    pub camera_buffer: Buffer,
    pub camera_bind_group: BindGroup,
    pub light_buffer: Buffer,
    pub light_bind_group: BindGroup,
    pub model_buffer: Buffer,
    pub model_bind_group: BindGroup,
    pub material_bind_group_layout: BindGroupLayout,
    pub meshes: Vec<GpuMesh>,
    pub materials: Vec<GpuMaterial>,
    pub camera: CameraData,
    pub light: LightData,
    pub depth_texture: Texture,
    pub depth_view: TextureView,
    pub size: (u32, u32),
    pub surface: Option<Surface<'static>>,
    pub config: Option<SurfaceConfiguration>,
    pub cached_color_texture: Option<Texture>,
    pub cached_color_view: Option<TextureView>,

    // ========== ОПТИМИЗАЦИИ ==========
    // Инстансинг буфер
    pub instance_buffer: Buffer,
    pub instance_capacity: u32,

    // Кэши для минимизации переключений состояний
    pub pipeline_cache: LruCache<u64, RenderPipeline>,
    pub bind_group_cache: LruCache<u64, BindGroup>,

    // Статистика производительности
    pub draw_calls: u32,
    pub triangles_rendered: u32,
    pub frame_allocations: u32,

    // Память
    pub frame_allocator: FrameAllocator,
    pub vertex_pool: ObjectPool<Vec<Vertex3D>>,
}

// Остальные структуры оставляем как были
#[derive(Debug, Clone)]
pub struct CameraData {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Debug, Clone)]
pub struct LightData {
    pub position: Vec3,
    pub color: [f32; 3],
    pub intensity: f32,
}

pub struct GpuMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer: Buffer,
    pub index_count: u32,
    pub visible: bool,
    pub vertex_count: u32,
}

pub struct GpuMaterial {
    pub _buffer: Buffer,
    pub bind_group: BindGroup,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    view_position: [f32; 3],
    _padding: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    position: [f32; 3],
    intensity: f32,
    color: [f32; 3],
    _padding: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelUniform {
    model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    albedo: [f32; 4],
    metallic: f32,
    roughness: f32,
    ao: f32,
    _padding: f32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex3D {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

// Инстанс данные для батчинга
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceData {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 3]; 3],
    material_index: u32,
    _padding: u32,
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

impl LightData {
    pub fn new() -> Self {
        Self {
            position: Vec3::new(10.0, 15.0, 10.0),
            color: [1.0, 0.95, 0.8],
            intensity: 2.0,
        }
    }
}

impl GpuRenderer {
    pub fn with_device(
        device: Arc<Device>,
        queue: Arc<Queue>,
        format: TextureFormat,
        width: u32,
        height: u32
    ) -> Self {
        let shader_source = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view_position: vec3<f32>,
}

struct Light {
    position: vec3<f32>,
    intensity: f32,
    color: vec3<f32>,
}

struct Model {
    model: mat4x4<f32>,
}

struct Material {
    albedo: vec4<f32>,
    metallic: f32,
    roughness: f32,
    ao: f32,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> light: Light;
@group(2) @binding(0) var<uniform> model_uniform: Model;
@group(3) @binding(0) var<uniform> material: Material;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model_uniform.model * vec4<f32>(in.position, 1.0);
    out.world_pos = world_pos.xyz;
    out.clip_pos = camera.view_proj * world_pos;
    out.normal = normalize((model_uniform.model * vec4<f32>(in.normal, 0.0)).xyz);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let light_dir = normalize(light.position - in.world_pos);
    let distance = length(light.position - in.world_pos);
    let attenuation = light.intensity / (1.0 + distance * distance * 0.001);
    let radiance = light.color * attenuation;
    let albedo = material.albedo * vec4<f32>(in.color, 1.0);
    let n_dot_l = max(dot(n, light_dir), 0.0);
    let diffuse = albedo.rgb * n_dot_l;
    let ambient = vec3<f32>(0.05) * albedo.rgb;
    let color = ambient + diffuse * radiance;
    return vec4<f32>(pow(color, vec3<f32>(1.0/2.2)), albedo.a);
}
"#;

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("PBR Shader"),
            source: ShaderSource::Wgsl(shader_source.into()),
        });

        // Camera bind group
        let camera_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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
            layout: &camera_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // Light bind group
        let light_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Light Layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let light = LightData::new();
        let light_uniform = LightUniform {
            position: [light.position.x, light.position.y, light.position.z],
            intensity: light.intensity,
            color: light.color,
            _padding: 0.0,
        };

        let light_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Light Buffer"),
            size: std::mem::size_of::<LightUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        light_buffer.slice(..).get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&[light_uniform]));
        light_buffer.unmap();

        let light_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Light BG"),
            layout: &light_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
        });

        // Model bind group
        let model_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Model Layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let model_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Model Buffer"),
            size: std::mem::size_of::<ModelUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let model_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Model BG"),
            layout: &model_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: model_buffer.as_entire_binding(),
            }],
        });

        // Material bind group layout
        let material_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Material Layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Pipeline
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &light_bind_group_layout,
                &model_bind_group_layout,
                &material_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

        let vertex_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex3D>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttribute { format: VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                VertexAttribute { format: VertexFormat::Float32x3, offset: 12, shader_location: 1 },
                VertexAttribute { format: VertexFormat::Float32x3, offset: 24, shader_location: 2 },
            ],
        };

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("PBR Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Option::from("vs_main"),
                compilation_options: Default::default(),
                buffers: &[vertex_layout],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Option::from("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Rgba8Unorm,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
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

        // Depth texture
        let depth_texture = device.create_texture(&TextureDescriptor {
            label: Some("Depth Texture"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&TextureViewDescriptor::default());

        // Инстанс буфер для батчинга
        let instance_capacity = 1024;
        let instance_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (std::mem::size_of::<InstanceData>() * instance_capacity as usize) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Default material
        let default_material = GpuMaterial::new(
            &device,
            &queue,
            &material_bind_group_layout,
            [0.8, 0.8, 0.8, 1.0],
            0.0,
            0.5,
        );

        Self {
            device,
            queue,
            pipeline,
            camera_buffer,
            camera_bind_group,
            light_buffer,
            light_bind_group,
            model_buffer,
            model_bind_group,
            material_bind_group_layout,
            meshes: Vec::new(),
            materials: vec![default_material],
            camera: CameraData::new(),
            light,
            depth_texture,
            depth_view,
            size: (width, height),
            surface: None,
            config: None,
            cached_color_texture: None,
            cached_color_view: None,

            // Оптимизации
            instance_buffer,
            instance_capacity,
            pipeline_cache: LruCache::new(NonZeroUsize::new(64).unwrap()),
            bind_group_cache: LruCache::new(NonZeroUsize::new(256).unwrap()),
            draw_calls: 0,
            triangles_rendered: 0,
            frame_allocations: 0,
            frame_allocator: FrameAllocator::new(64), // 64 MB для GPU данных
            vertex_pool: ObjectPool::new(
                || Vec::with_capacity(4096),
                128,
                1000,
            ),
        }
    }

    /// ОПТИМИЗИРОВАННЫЙ РЕНДЕРИНГ С ГРУППИРОВКОЙ ПО МАТЕРИАЛАМ
    pub fn render_optimized(
        &mut self,
        render_objects: &[(usize, [[f32; 4]; 4], usize)],
    ) -> Result<(), String> {
        self.draw_calls = 0;
        self.triangles_rendered = 0;

        let vp = self.camera.view_proj_matrix();
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[CameraUniform {
            view_proj: vp,
            view_position: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
            _padding: 0.0,
        }]));

        let (color_view, _) = if let (Some(ref tex), Some(ref view)) = (&self.cached_color_texture, &self.cached_color_view) {
            if tex.size().width == self.size.0 && tex.size().height == self.size.1 {
                (view, tex)
            } else {
                let (new_tex, new_view) = self.create_color_texture();
                self.cached_color_texture = Some(new_tex);
                self.cached_color_view = Some(new_view);
                (self.cached_color_view.as_ref().unwrap(), self.cached_color_texture.as_ref().unwrap())
            }
        } else {
            let (new_tex, new_view) = self.create_color_texture();
            self.cached_color_texture = Some(new_tex);
            self.cached_color_view = Some(new_view);
            (self.cached_color_view.as_ref().unwrap(), self.cached_color_texture.as_ref().unwrap())
        };

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("GPU Render Optimized")
        });

        // ГРУППИРУЕМ ПО МАТЕРИАЛАМ ДЛЯ МИНИМИЗАЦИИ ПЕРЕКЛЮЧЕНИЙ
        let mut material_groups: HashMap<usize, Vec<&(usize, [[f32; 4]; 4], usize)>> = HashMap::new();

        for obj in render_objects {
            material_groups
                .entry(obj.2)
                .or_insert_with(Vec::new)
                .push(obj);
        }

        {
            let mut rp = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("GPU Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.camera_bind_group, &[]);
            rp.set_bind_group(1, &self.light_bind_group, &[]);

            // РЕНДЕРИМ ГРУППАМИ ПО МАТЕРИАЛАМ
            for (&material_idx, objects) in &material_groups {
                // Устанавливаем материал ОДИН раз для всей группы
                let mat_idx = if material_idx < self.materials.len() { material_idx } else { 0 };
                rp.set_bind_group(3, &self.materials[mat_idx].bind_group, &[]);

                for &&(mesh_idx, model_matrix, _) in objects {
                    if mesh_idx >= self.meshes.len() { continue; }
                    let mesh = &self.meshes[mesh_idx];
                    if !mesh.visible { continue; }

                    // Обновляем матрицу модели
                    let model_uniform = ModelUniform { model: model_matrix };
                    self.queue.write_buffer(&self.model_buffer, 0, bytemuck::cast_slice(&[model_uniform]));
                    rp.set_bind_group(2, &self.model_bind_group, &[]);

                    rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    rp.set_index_buffer(mesh.index_buffer.slice(..), IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);

                    self.draw_calls += 1;
                    self.triangles_rendered += mesh.index_count / 3;
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        // Логируем статистику каждые 100 кадров
        if self.draw_calls > 100 {
            println!("🎨 GPU: {} draw calls, {} triangles",
                     self.draw_calls, self.triangles_rendered);
        }

        Ok(())
    }

    fn create_color_texture(&self) -> (Texture, TextureView) {
        let tex = self.device.create_texture(&TextureDescriptor {
            label: Some("Color Target"),
            size: Extent3d { width: self.size.0.max(1), height: self.size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&TextureViewDescriptor::default());
        (tex, view)
    }

    // Оригинальный метод для совместимости
    pub fn render_to_egui_texture(
        &mut self,
        render_objects: &[(usize, [[f32; 4]; 4], usize)],
        tex_id: egui::TextureId,
        ctx: &egui::Context,
        width: u32,
        height: u32,
    ) {
        let vp = self.camera.view_proj_matrix();
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[CameraUniform {
            view_proj: vp,
            view_position: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
            _padding: 0.0,
        }]));

        let color_tex = self.device.create_texture(&TextureDescriptor {
            label: Some("3D Output"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&TextureViewDescriptor::default());

        let buffer_size = (width * height * 4) as u64;
        let output_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Output"),
            size: buffer_size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("GPU") });

        {
            let mut rp = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("3D Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.camera_bind_group, &[]);
            rp.set_bind_group(1, &self.light_bind_group, &[]);

            for (mesh_idx, model_matrix, material_idx) in render_objects {
                if *mesh_idx >= self.meshes.len() { continue; }
                let mesh = &self.meshes[*mesh_idx];
                if !mesh.visible { continue; }

                self.queue.write_buffer(&self.model_buffer, 0, bytemuck::cast_slice(&[ModelUniform { model: *model_matrix }]));
                rp.set_bind_group(2, &self.model_bind_group, &[]);

                let mat_idx = if *material_idx < self.materials.len() { *material_idx } else { 0 };
                rp.set_bind_group(3, &self.materials[mat_idx].bind_group, &[]);

                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rp.set_index_buffer(mesh.index_buffer.slice(..), IndexFormat::Uint32);
                rp.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        encoder.copy_texture_to_buffer(
            ImageCopyTexture { texture: &color_tex, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
            ImageCopyBuffer { buffer: &output_buffer, layout: ImageDataLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) } },
            Extent3d { width, height, depth_or_array_layers: 1 },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = output_buffer.slice(..);
        slice.map_async(MapMode::Read, |_| {});
        self.device.poll(Maintain::Wait);

        let data = slice.get_mapped_range().to_vec();

        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            &data,
        );
        ctx.tex_manager().write().set(
            tex_id,
            egui::epaint::ImageDelta::full(color_image, egui::TextureOptions::LINEAR),
        );
    }

    pub fn add_mesh(&mut self, mesh: &crate::mesh::Mesh) -> usize {
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

        let idx = self.meshes.len();
        self.meshes.push(GpuMesh {
            vertex_buffer: vb,
            index_buffer: ib,
            index_count: mesh.indices.len() as u32,
            visible: true,
            vertex_count: vertices.len() as u32,
        });
        idx
    }

    pub fn add_material(&mut self, albedo: [f32; 4], metallic: f32, roughness: f32) -> usize {
        let mat = GpuMaterial::new(
            &self.device,
            &self.queue,
            &self.material_bind_group_layout,
            albedo,
            metallic,
            roughness
        );
        let idx = self.materials.len();
        self.materials.push(mat);
        idx
    }

    /// Очистка кэшей при изменении сцены
    pub fn invalidate_caches(&mut self) {
        self.pipeline_cache.clear();
        self.bind_group_cache.clear();
    }

    /// Получить статистику рендеринга
    pub fn get_stats(&self) -> (u32, u32, f64) {
        let usage = self.frame_allocator.usage_percent();
        (self.draw_calls, self.triangles_rendered, usage)
    }
}

impl GpuMaterial {
    pub fn new(
        device: &Device,
        queue: &Queue,
        layout: &BindGroupLayout,
        albedo: [f32; 4],
        metallic: f32,
        roughness: f32
    ) -> Self {
        let uniform = MaterialUniform { albedo, metallic, roughness, ao: 1.0, _padding: 0.0 };
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Mat"),
            size: std::mem::size_of::<MaterialUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        buffer.slice(..).get_mapped_range_mut().copy_from_slice(bytemuck::cast_slice(&[uniform]));
        buffer.unmap();

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Mat BG"),
            layout,
            entries: &[BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        Self { _buffer: buffer, bind_group }
    }
}