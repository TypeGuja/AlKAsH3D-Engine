// src/gpu/renderer.rs - GPU РЕНДЕРЕР С ПРЯМОЙ ИНТЕГРАЦИЕЙ В EGUI
use crate::math::Vec3;
use crate::mesh::Mesh;
use std::sync::Arc;
use std::collections::HashMap;

use egui_wgpu::wgpu;
use wgpu::*;

// ============================================================
// Структуры данных
// ============================================================

pub struct GpuRenderer {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub pipeline: wgpu::RenderPipeline,
    pub camera: CameraData,
    pub light: LightData,

    // Буферы для камеры и света
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    pub light_buffer: wgpu::Buffer,
    pub light_bind_group: wgpu::BindGroup,

    // Буфер модели
    pub model_buffer: wgpu::Buffer,
    pub model_bind_group: wgpu::BindGroup,

    // Материалы
    pub material_bind_group_layout: wgpu::BindGroupLayout,
    pub materials: Vec<GpuMaterial>,

    // Меши
    pub meshes: Vec<GpuMesh>,

    // Текстура глубины
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,

    // Выходная текстура
    pub output_texture: Option<wgpu::Texture>,
    pub output_view: Option<wgpu::TextureView>,

    // Буфер для копирования
    pub readback_buffer: Option<wgpu::Buffer>,
    pub buffer_size: u64,

    // Egui текстура
    pub egui_texture: Option<egui::TextureId>,
    pub texture_size: (u32, u32),

    // Статистика
    pub draw_calls: u32,
    pub triangles_rendered: u32,
    pub surface_format: wgpu::TextureFormat,

    // Флаг ожидания копирования
    pub copy_in_progress: bool,
}

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
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub visible: bool,
}

pub struct GpuMaterial {
    pub buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

// ============================================================
// Uniform структуры
// ============================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
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
struct ModelUniform {
    model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    albedo: [f32; 4],
    metallic: f32,
    roughness: f32,
    ao: f32,
}

// ============================================================
// Вершинные данные
// ============================================================

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex3D {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

impl Vertex3D {
    fn vertex_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 24,
                    shader_location: 2,
                },
            ],
        }
    }
}

// ============================================================
// Шейдер WGSL
// ============================================================

const PBR_SHADER: &str = r#"
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
    let normal_matrix = mat3x3<f32>(
        model_uniform.model[0].xyz,
        model_uniform.model[1].xyz,
        model_uniform.model[2].xyz,
    );
    out.normal = normalize(normal_matrix * in.normal);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let N = normalize(in.normal);
    let V = normalize(camera.view_position - in.world_pos);
    let L = normalize(light.position - in.world_pos);
    let H = normalize(L + V);

    let distance = length(light.position - in.world_pos);
    let attenuation = light.intensity / (1.0 + distance * distance * 0.001);
    let radiance = light.color * attenuation;

    let albedo = material.albedo.rgb * in.color;
    let metallic = material.metallic;
    let roughness = material.roughness;

    let F0 = mix(vec3<f32>(0.04), albedo, metallic);
    let cos_theta = max(dot(H, V), 0.0);
    let F = F0 + (1.0 - F0) * pow(1.0 - cos_theta, 5.0);

    let alpha = roughness * roughness;
    let alpha2 = alpha * alpha;
    let NdotH = max(dot(N, H), 0.0001);
    let denom = NdotH * NdotH * (alpha2 - 1.0) + 1.0;
    let D = alpha2 / (3.14159265 * denom * denom);

    let NdotV = max(dot(N, V), 0.0001);
    let NdotL = max(dot(N, L), 0.0001);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let G1V = NdotV / (NdotV * (1.0 - k) + k);
    let G1L = NdotL / (NdotL * (1.0 - k) + k);
    let G = G1V * G1L;

    let specular = (D * G * F) / max(4.0 * NdotV * NdotL, 0.0001);
    let kD = (1.0 - F) * (1.0 - metallic);
    let diffuse = kD * albedo / 3.14159265;

    let ambient = vec3<f32>(0.03) * albedo * material.ao;
    var color = ambient + (diffuse + specular) * radiance * NdotL;

    color = color / (color + 1.0);
    color = pow(color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(color, material.albedo.a);
}
"#;

// ============================================================
// Реализация CameraData
// ============================================================

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
                result[i][j] = 0.0;
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

// ============================================================
// Реализация GpuRenderer
// ============================================================

impl GpuRenderer {
    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PBR Shader"),
            source: wgpu::ShaderSource::Wgsl(PBR_SHADER.into()),
        });

        // Camera bind group
        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera BG"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // Light bind group
        let light_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Light Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
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

        let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Light Buffer"),
            size: std::mem::size_of::<LightUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        light_buffer.slice(..).get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&[light_uniform]));
        light_buffer.unmap();

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Light BG"),
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
        });

        // Model bind group
        let model_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Model Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let model_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Model Buffer"),
            size: std::mem::size_of::<ModelUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Model BG"),
            layout: &model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: model_buffer.as_entire_binding(),
            }],
        });

        // Material bind group layout
        let material_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Material Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &light_bind_group_layout,
                &model_bind_group_layout,
                &material_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("PBR Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Vertex3D::vertex_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Depth texture
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

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
            camera: CameraData::new(),
            light,
            camera_buffer,
            camera_bind_group,
            light_buffer,
            light_bind_group,
            model_buffer,
            model_bind_group,
            material_bind_group_layout,
            materials: vec![default_material],
            meshes: Vec::new(),
            depth_texture,
            depth_view,
            output_texture: None,
            output_view: None,
            readback_buffer: None,
            buffer_size: 0,
            egui_texture: None,
            texture_size: (width, height),
            draw_calls: 0,
            triangles_rendered: 0,
            surface_format: format,
            copy_in_progress: false,
        }
    }

    /// Проверяет, нужно ли пересоздавать output текстуру
    fn ensure_output_texture(&mut self, width: u32, height: u32) {
        let needs_new = match &self.output_texture {
            None => true,
            Some(tex) => tex.width() != width || tex.height() != height,
        };

        if needs_new {
            let w = width.max(1);
            let h = height.max(1);

            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Output Texture"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Создаём readback буфер
            let buffer_size = (w as u64 * h as u64 * 4).max(4);
            let readback_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Readback Buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            self.output_texture = Some(texture);
            self.output_view = Some(view);
            self.readback_buffer = Some(readback_buffer);
            self.buffer_size = buffer_size;
            self.texture_size = (w, h);
            self.copy_in_progress = false;
        }

        // Пересоздаём depth если нужно
        if self.depth_texture.width() != width || self.depth_texture.height() != height {
            let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Depth Texture"),
                size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.depth_texture = depth_texture;
            self.depth_view = depth_view;
        }
    }

    /// Рендерит сцену и копирует результат в readback буфер
    pub fn render(
        &mut self,
        render_objects: &[(usize, [[f32; 4]; 4], usize)],
        width: u32,
        height: u32,
    ) {
        self.draw_calls = 0;
        self.triangles_rendered = 0;

        self.ensure_output_texture(width, height);

        // Обновляем камеру
        let vp = self.camera.view_proj_matrix();
        let camera_uniform = CameraUniform {
            view_proj: vp,
            view_position: [
                self.camera.position.x,
                self.camera.position.y,
                self.camera.position.z,
            ],
            _padding: 0.0,
        };
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[camera_uniform]));

        let output_view = match &self.output_view {
            Some(v) => v,
            None => return,
        };
        let readback_buffer = match &self.readback_buffer {
            Some(b) => b,
            None => return,
        };

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GPU Render Encoder"),
        });

        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GPU Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.camera_bind_group, &[]);
            rp.set_bind_group(1, &self.light_bind_group, &[]);

            // Группируем по материалам
            let mut material_groups: HashMap<usize, Vec<&(usize, [[f32; 4]; 4], usize)>> = HashMap::new();
            for obj in render_objects {
                material_groups.entry(obj.2).or_default().push(obj);
            }

            for (&material_idx, objects) in &material_groups {
                let mat_idx = if material_idx < self.materials.len() { material_idx } else { 0 };
                rp.set_bind_group(3, &self.materials[mat_idx].bind_group, &[]);

                for &&(mesh_idx, model_matrix, _) in objects {
                    if mesh_idx >= self.meshes.len() { continue; }
                    let mesh = &self.meshes[mesh_idx];
                    if !mesh.visible { continue; }

                    let model_uniform = ModelUniform { model: model_matrix };
                    self.queue.write_buffer(&self.model_buffer, 0, bytemuck::cast_slice(&[model_uniform]));
                    rp.set_bind_group(2, &self.model_bind_group, &[]);

                    rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    if mesh.index_count > 0 {
                        rp.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                    }

                    self.draw_calls += 1;
                    self.triangles_rendered += mesh.index_count / 3;
                }
            }
        }

        // Копируем в readback буфер
        let output_texture = self.output_texture.as_ref().unwrap();
        let w = self.texture_size.0;
        let h = self.texture_size.1;

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: readback_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        self.copy_in_progress = true;
    }

    /// Пытается обновить egui текстуру из readback буфера
    pub fn try_update_egui_texture(&mut self, ctx: &egui::Context) -> bool {
        if !self.copy_in_progress {
            return false;
        }

        let readback_buffer = match &self.readback_buffer {
            Some(b) => b,
            None => return false,
        };

        let w = self.texture_size.0 as usize;
        let h = self.texture_size.1 as usize;

        // Пробуем прочитать без блокировки
        let slice = readback_buffer.slice(..);

        // Используем канал для асинхронного чтения
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        // Не ждём — просто проверяем
        self.device.poll(wgpu::Maintain::Poll);

        match rx.try_recv() {
            Ok(Ok(())) => {
                let data = slice.get_mapped_range().to_vec();

                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [w, h],
                    &data,
                );

                // Обновляем существующую текстуру или создаём новую
                if let Some(tex_id) = self.egui_texture {
                    ctx.tex_manager().write().set(
                        tex_id,
                        egui::epaint::ImageDelta::full(color_image, egui::TextureOptions::LINEAR),
                    );
                } else {
                    let handle = ctx.load_texture(
                        "gpu-3d-output",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.egui_texture = Some(handle.id());
                }

                drop(data);
                self.copy_in_progress = false;
                true
            }
            _ => false,
        }
    }

    /// Возвращает текущую egui текстуру для отображения
    pub fn get_egui_texture(&self) -> Option<egui::TextureId> {
        self.egui_texture
    }

    pub fn add_mesh(&mut self, mesh: &Mesh) -> usize {
        let mut vertices = Vec::with_capacity(mesh.vertices.len());

        for i in 0..mesh.vertices.len() {
            let normal = if i < mesh.normals.len() { mesh.normals[i] } else { Vec3::UP };

            vertices.push(Vertex3D {
                position: [mesh.vertices[i].x, mesh.vertices[i].y, mesh.vertices[i].z],
                normal: [normal.x, normal.y, normal.z],
                color: [0.7, 0.7, 0.7],
            });
        }

        let vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (vertices.len() * std::mem::size_of::<Vertex3D>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        vertex_buffer.slice(..).get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&vertices));
        vertex_buffer.unmap();

        let index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Index Buffer"),
            size: (mesh.indices.len() * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        index_buffer.slice(..).get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&mesh.indices));
        index_buffer.unmap();

        let idx = self.meshes.len();
        self.meshes.push(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
            visible: true,
        });

        idx
    }

    pub fn add_material(&mut self, albedo: [f32; 4], metallic: f32, roughness: f32) -> usize {
        let mat = GpuMaterial::new(&self.device, &self.queue, &self.material_bind_group_layout, albedo, metallic, roughness);
        let idx = self.materials.len();
        self.materials.push(mat);
        idx
    }
}

// ============================================================
// Реализация GpuMaterial
// ============================================================

impl GpuMaterial {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        albedo: [f32; 4],
        metallic: f32,
        roughness: f32,
    ) -> Self {
        let uniform = MaterialUniform { albedo, metallic, roughness, ao: 1.0 };

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Material Buffer"),
            size: std::mem::size_of::<MaterialUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        buffer.slice(..).get_mapped_range_mut()
            .copy_from_slice(bytemuck::cast_slice(&[uniform]));
        buffer.unmap();

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Material BG"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self { buffer, bind_group }
    }
}