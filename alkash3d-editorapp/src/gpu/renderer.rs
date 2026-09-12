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

    // Буфер модели.
    //
    // ИСПРАВЛЕНО (баг "объекты не рисуются" / все объекты схлопываются в
    // одну точку): раньше это был ОДИН uniform-буфер на одну ModelUniform
    // (64 байта), который в render() перезаписывался через
    // `self.queue.write_buffer(&self.model_buffer, 0, ..)` перед КАЖДЫМ
    // draw_indexed внутри одного и того же command encoder'а. Но
    // queue.write_buffer не встраивается в текущий encoder как команда —
    // это отдельная запись, которая (наравне с остальными такими же
    // записями) применяется к буферу до того, как GPU начнёт выполнять
    // команды основного encoder'а (тот сабмитится один раз, после того как
    // весь render pass уже записан). Поэтому к моменту фактического
    // исполнения draw-вызовов на GPU в буфере оставалась ТОЛЬКО последняя
    // записанная матрица — то есть ВСЕ объекты кадра рисовались с
    // трансформацией последнего обработанного объекта (порядок которого
    // к тому же был недетерминирован — материалы группировались через
    // HashMap). Теперь это буфер на `model_capacity` слотов с выравниванием
    // под `min_uniform_buffer_offset_alignment`, все матрицы кадра
    // записываются в него по своим уникальным офсетам ДО открытия render
    // pass, а каждый draw_indexed использует свой слот через dynamic offset
    // в set_bind_group — так буферы разных объектов больше не затирают
    // друг друга.
    pub model_buffer: wgpu::Buffer,
    pub model_bind_group: wgpu::BindGroup,
    model_bind_group_layout: wgpu::BindGroupLayout,
    model_stride: u64,
    model_capacity: u32,

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

    // ИСПРАВЛЕНО (ещё один wgpu validation panic — "Bytes per row does not
    // respect COPY_BYTES_PER_ROW_ALIGNMENT" в copy_texture_to_buffer): wgpu
    // требует, чтобы bytes_per_row в ImageDataLayout был кратен 256 байтам;
    // "естественный" bytes_per_row = width * 4 этому почти никогда не
    // удовлетворяет (кратно 256 только при width, кратной 64). Храним
    // реально используемый (дополненный до 256) bytes_per_row отдельно от
    // логической ширины изображения — see try_update_egui_texture, где по
    // этому значению построчно убирается паддинг перед тем, как отдать
    // тайтово упакованные пиксели в egui::ColorImage.
    padded_bytes_per_row: u32,

    // Egui текстура
    // ИСПРАВЛЕНО (панику "Tried setting texture Managed(N) which is not
    // allocated" в epaint): раньше здесь хранился голый `egui::TextureId`,
    // полученный из `handle.id()` сразу после `ctx.load_texture(...)`, а сам
    // `TextureHandle` отбрасывался. `TextureHandle` в egui — счётчик ссылок:
    // когда он дропается, texture manager на ближайшем кадре освобождает
    // текстуру по этому id. Следующий вызов `tex_manager().write().set(id, ..)`
    // с уже освобождённым id и приводил к панике. Нужно хранить сам handle,
    // пока текстура используется — тогда он живёт вместе с рендерером.
    pub egui_texture: Option<egui::TextureHandle>,
    pub texture_size: (u32, u32),

    // Статистика
    pub draw_calls: u32,
    pub triangles_rendered: u32,
    pub surface_format: wgpu::TextureFormat,

    // Флаг ожидания копирования
    pub copy_in_progress: bool,

    // ИСПРАВЛЕНО (баг отрисовки — см. подробности у try_update_egui_texture):
    // канал текущего незавершённого map_async, чтобы не запускать map_async
    // повторно на буфере, который уже находится в процессе маппинга.
    map_rx: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
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
    // ИСПРАВЛЕНО (ещё один wgpu validation panic: "Buffer is bound with size
    // 28 where the shader expects 32 in group[3]"): без этого поля структура
    // занимает 16+4+4+4=28 байт, а WGSL-структура `Material` (см. PBR_SHADER
    // выше) из-за vec4-поля albedo требует выравнивания размера на 16 байт
    // (std140-подобные правила uniform-буферов) — то есть реально 32 байта.
    // CameraUniform/LightUniform это поле уже имели, у MaterialUniform его
    // не хватало.
    _padding: f32,
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
            // ИСПРАВЛЕНО (баг: "большой импортированный OBJ-город не
            // виден"): far=1000 обрезал геометрию дальше 1000 единиц от
            // камеры — для сцены масштаба "город" этого может не хватать
            // даже после того, как исправлен потолок zoom_camera (см.
            // app.rs). near/far здесь не синхронизируются с App per-frame
            // (в отличие от position/target/fov/aspect в render_gpu_viewport)
            // — это просто статический дефолт, так что подняли его сразу
            // до безопасного запаса, согласованного с новым потолком zoom
            // (500 000).
            far: 500_000.0,
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

    // ИСПРАВЛЕНО (глубина клипа не подходила под wgpu): формула ниже —
    // классическая OpenGL-проекция, у которой NDC z лежит в [-1, 1]
    // (M[2][2]=(far+near)/(near-far), M[2][3]=2*far*near/(near-far)). Но
    // wgpu (как D3D/Vulkan/Metal) ожидает NDC z в [0, 1] — с OpenGL-формулой
    // clip.z получается отрицательным для всего, что ближе примерно
    // середины диапазона [near, far], и аппаратный клиппинг (который для
    // wgpu требует clip.z в [0, clip.w]) отбрасывает такие вершины как
    // "перед near plane", хотя они видимы. Заменил на стандартную формулу
    // для z в [0,1] (M[2][2]=far/(near-far), M[2][3]=far*near/(near-far)).
    fn proj_matrix(&self) -> [[f32; 4]; 4] {
        let f = 1.0 / (self.fov / 2.0).tan();

        [
            [f / self.aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, self.far / (self.near - self.far), -1.0],
            [0.0, 0.0, (self.far * self.near) / (self.near - self.far), 0.0],
        ]
    }

    /// ИСПРАВЛЕНО (главная причина "объекты не рисуются на видеокарте" —
    /// сильнее всех остальных найденных багов вместе взятых): перемножение
    /// `result[i][j] = sum_k proj[i][k] * view[k][j]` арифметически НЕ дает
    /// `proj_matrix * view_matrix` в том соглашении (внешний индекс массива
    /// = колонка, см. комментарий у Transform::to_matrix()), в котором сами
    /// view_matrix()/proj_matrix() написаны и корректно читаются WGSL по
    /// отдельности. Проверено численно: для камеры в (0,0,5), смотрящей на
    /// начало координат, старая формула давала для мировой точки (0,0,0)
    /// клип-координату с W=0 (!) — то есть после perspective-divide деление
    /// на ноль/неопределённость для КАЖДОЙ вершины прямо по центру экрана,
    /// а для остальных точек — грубо неверный клиппинг. Правильная формула
    /// (дающая ровно proj_matrix() ∘ view_matrix(), т.е. сперва view, потом
    /// proj, как и требуется) получается перемножением в обратном порядке
    /// операндов: `result[i][j] = sum_k view[i][k] * proj[k][j]`.
    pub fn view_proj_matrix(&self) -> [[f32; 4]; 4] {
        let view = self.view_matrix();
        let proj = self.proj_matrix();
        let mut result = [[0.0; 4]; 4];

        for i in 0..4 {
            for j in 0..4 {
                result[i][j] = 0.0;
                for k in 0..4 {
                    result[i][j] += view[i][k] * proj[k][j];
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

        // Model bind group. has_dynamic_offset: true — см. комментарий у
        // поля model_buffer выше: каждый объект кадра получает свой слот в
        // общем буфере вместо того, чтобы делить один слот на всех.
        let model_item_size = std::mem::size_of::<ModelUniform>() as u64;
        let model_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Model Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: std::num::NonZeroU64::new(model_item_size),
                },
                count: None,
            }],
        });

        let model_align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let model_stride = model_item_size.div_ceil(model_align) * model_align;
        let model_capacity: u32 = 128;

        let model_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Model Buffer"),
            size: model_stride * model_capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Model BG"),
            layout: &model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &model_buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(model_item_size),
                }),
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
                // ИСПРАВЛЕНО (главная причина крашей "GPU rendering" — wgpu
                // validation panic "Render pipeline targets are incompatible
                // with render pass ... RenderPass uses textures with formats
                // [Rgba8Unorm] but the RenderPipeline uses attachments with
                // formats [Bgra8Unorm]"): раньше здесь стоял `format`,
                // переданный снаружи как `render_state.target_format` — формат
                // ОКОННОЙ поверхности egui/wgpu. Но этот рендерер никогда не
                // рисует напрямую в окно — он всегда рендерит в offscreen
                // `output_texture`, который `ensure_output_texture()` ниже
                // всегда создаёт как `Rgba8Unorm` (годный для readback в
                // egui::ColorImage). Из-за рассинхронизации форматов первый же
                // вызов `render()` падал с hard validation panic и убивал весь
                // процесс. Пайплайн должен быть собран под формат ЦЕЛИ, в
                // которую он реально рисует, — то есть под Rgba8Unorm, а не
                // под формат окна.
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
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
            model_bind_group_layout,
            model_stride,
            model_capacity,
            material_bind_group_layout,
            materials: vec![default_material],
            meshes: Vec::new(),
            depth_texture,
            depth_view,
            output_texture: None,
            output_view: None,
            readback_buffer: None,
            buffer_size: 0,
            padded_bytes_per_row: 0,
            egui_texture: None,
            texture_size: (width, height),
            draw_calls: 0,
            triangles_rendered: 0,
            surface_format: format,
            copy_in_progress: false,
            map_rx: None,
        }
    }

    /// Гарантирует, что model_buffer вмещает `needed` слотов (по одному на
    /// объект кадра); при нехватке пересоздаёт буфер и bind group под новую
    /// вместимость (см. комментарий у поля model_buffer).
    fn ensure_model_capacity(&mut self, needed: u32) {
        if needed <= self.model_capacity {
            return;
        }

        let new_capacity = needed.max(self.model_capacity.saturating_mul(2)).max(16);
        let model_item_size = std::mem::size_of::<ModelUniform>() as u64;

        self.model_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Model Buffer"),
            size: self.model_stride * new_capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.model_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Model BG"),
            layout: &self.model_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.model_buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(model_item_size),
                }),
            }],
        });
        self.model_capacity = new_capacity;
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

            // Создаём readback буфер. bytes_per_row обязан быть кратен
            // wgpu::COPY_BYTES_PER_ROW_ALIGNMENT (256) — округляем вверх и
            // выделяем буфер уже под дополненный размер строки (см. пояснение
            // у поля padded_bytes_per_row выше).
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let unpadded_bytes_per_row = w * 4;
            let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
            let buffer_size = (padded_bytes_per_row as u64 * h as u64).max(4);
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
            self.padded_bytes_per_row = padded_bytes_per_row;
            self.texture_size = (w, h);
            self.copy_in_progress = false;
            // Старый readback-буфер (и любой незавершённый map_async на нём)
            // сейчас будет отброшен вместе с заменяемым Buffer — забываем и
            // его receiver, иначе try_update_egui_texture ниже мог бы позже
            // получить результат маппинга буфера, которого уже нет.
            self.map_rx = None;
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

        // ИСПРАВЛЕНО (баг отрисовки на видеокарте — см. подробности у
        // try_update_egui_texture): раньше render() безусловно писал новый
        // copy_texture_to_buffer в readback_buffer каждый UI-кадр, даже пока
        // буфер ещё был замаплен (map_async) под чтение предыдущего кадра.
        // wgpu запрещает использовать замапленный буфер как destination
        // копирования — это либо тихо проглатываемая validation-ошибка (и
        // вьюпорт замирал на первом же кадре), либо паника через
        // uncaptured-error handler в зависимости от бэкенда/сборки. Пока
        // предыдущий цикл чтения не завершён (см. copy_in_progress/map_rx),
        // просто пропускаем этот кадр рендера — предыдущая картинка в egui
        // остаётся видна, а как только буфер освободится, рендер продолжится
        // с актуальной камерой/сценой.
        if self.copy_in_progress {
            return;
        }

        // Гарантируем, что в model_buffer хватит слотов на все объекты
        // кадра, и сразу пишем каждую матрицу в свой уникальный офсет — до
        // открытия render pass, одним проходом, без разделения одного слота
        // между несколькими draw-вызовами (см. комментарий у поля
        // model_buffer выше).
        self.ensure_model_capacity(render_objects.len() as u32);
        for (i, obj) in render_objects.iter().enumerate() {
            let model_uniform = ModelUniform { model: obj.1 };
            let offset = i as u64 * self.model_stride;
            self.queue.write_buffer(&self.model_buffer, offset, bytemuck::cast_slice(&[model_uniform]));
        }

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

            // Группируем по материалам — индексы в render_objects, а не
            // сами кортежи, чтобы у каждого объекта остался его собственный
            // офсет (i * model_stride) в общем model_buffer.
            let mut material_groups: HashMap<usize, Vec<usize>> = HashMap::new();
            for (i, obj) in render_objects.iter().enumerate() {
                material_groups.entry(obj.2).or_default().push(i);
            }

            for (&material_idx, indices) in &material_groups {
                let mat_idx = if material_idx < self.materials.len() { material_idx } else { 0 };
                rp.set_bind_group(3, &self.materials[mat_idx].bind_group, &[]);

                for &i in indices {
                    let (mesh_idx, _model_matrix, _) = render_objects[i];
                    if mesh_idx >= self.meshes.len() { continue; }
                    let mesh = &self.meshes[mesh_idx];
                    if !mesh.visible { continue; }

                    // Матрица этого объекта уже записана в свой слот выше
                    // (до открытия render pass) — здесь только выбираем его
                    // через dynamic offset, ничего не перезаписывая.
                    let offset = i as u64 * self.model_stride;
                    rp.set_bind_group(2, &self.model_bind_group, &[offset as u32]);

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
                    // Дополненный до 256 байт bytes_per_row (см. поле
                    // padded_bytes_per_row) — try_update_egui_texture ниже
                    // убирает этот паддинг при чтении.
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        self.copy_in_progress = true;
    }

    /// Пытается обновить egui текстуру из readback буфера.
    ///
    /// ИСПРАВЛЕНО (баг отрисовки на видеокарте): раньше эта функция вызывала
    /// `slice.map_async(...)` заново на КАЖДЫЙ вызов, пока `copy_in_progress`
    /// было true — то есть на буфере с уже отправленным, но не завершённым
    /// маппингом запускался ещё один `map_async`, что wgpu не поддерживает
    /// (второй запрос либо отбрасывается с ошибкой в новый канал, либо
    /// конфликтует с первым). А после успешного чтения буфер НИКОГДА не
    /// разматировался обратно (`unmap()` не вызывался) — он оставался
    /// замапленным навсегда, и следующий `render()` пытался писать в него
    /// через `copy_texture_to_buffer`, что для замапленного буфера запрещено.
    /// Отсюда и наблюдавшиеся баги — вьюпорт замирал на первом кадре или
    /// падал. Теперь `map_async` запускается РОВНО один раз за цикл (receiver
    /// хранится в `self.map_rx` между вызовами), а по завершении буфер
    /// явно разматируется.
    pub fn try_update_egui_texture(&mut self, ctx: &egui::Context) -> bool {
        if !self.copy_in_progress {
            return false;
        }

        let readback_buffer = match &self.readback_buffer {
            Some(b) => b,
            None => return false,
        };

        // Запускаем map_async только один раз за цикл чтения.
        if self.map_rx.is_none() {
            let slice = readback_buffer.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
            self.map_rx = Some(rx);
        }

        // Не ждём — просто проверяем, готов ли маппинг.
        self.device.poll(wgpu::Maintain::Poll);

        let rx = self.map_rx.as_ref().unwrap();
        match rx.try_recv() {
            Ok(Ok(())) => {
                let w = self.texture_size.0 as usize;
                let h = self.texture_size.1 as usize;
                let padded_row = self.padded_bytes_per_row as usize;
                let unpadded_row = w * 4;
                let raw = readback_buffer.slice(..).get_mapped_range();

                // Строки в буфере дополнены до padded_bytes_per_row (см.
                // ensure_output_texture/render) — вырезаем только реальные
                // unpadded_row байт из каждой строки, иначе изображение
                // съезжает по диагонали (padding-байты сдвигают все строки,
                // кроме первой).
                let data: Vec<u8> = if padded_row == unpadded_row {
                    raw.to_vec()
                } else {
                    let mut tight = Vec::with_capacity(unpadded_row * h);
                    for row in 0..h {
                        let start = row * padded_row;
                        tight.extend_from_slice(&raw[start..start + unpadded_row]);
                    }
                    tight
                };
                drop(raw);

                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [w, h],
                    &data,
                );
                drop(data);

                // Буфер обязательно разматируем сразу после чтения — иначе
                // следующий render() не сможет писать в него copy_texture_to_buffer.
                readback_buffer.unmap();
                self.map_rx = None;
                self.copy_in_progress = false;

                // Обновляем существующую текстуру или создаём новую. Handle
                // хранится целиком в self.egui_texture (см. комментарий у
                // поля) — только так текстура переживает конец этого кадра.
                if let Some(handle) = &self.egui_texture {
                    ctx.tex_manager().write().set(
                        handle.id(),
                        egui::epaint::ImageDelta::full(color_image, egui::TextureOptions::LINEAR),
                    );
                } else {
                    let handle = ctx.load_texture(
                        "gpu-3d-output",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.egui_texture = Some(handle);
                }

                true
            }
            Ok(Err(e)) => {
                eprintln!("[GPU] try_update_egui_texture: map_async failed: {:?}", e);
                self.map_rx = None;
                self.copy_in_progress = false;
                false
            }
            Err(_) => false, // ещё не готово — ждём следующего кадра
        }
    }

    /// Возвращает текущую egui текстуру для отображения
    pub fn get_egui_texture(&self) -> Option<egui::TextureId> {
        self.egui_texture.as_ref().map(|h| h.id())
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
        let uniform = MaterialUniform { albedo, metallic, roughness, ao: 1.0, _padding: 0.0 };

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