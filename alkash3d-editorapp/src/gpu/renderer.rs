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

    // ДОБАВЛЕНО (рендер частиц — см. `ParticleInstance`/`render()` ниже):
    // отдельный alpha-blend пайплайн поверх основного (тот рисует
    // непрозрачные меши с BlendState::REPLACE, для частиц это не годится).
    // Динамический vertex-буфер растёт по тому же паттерну, что и
    // `model_buffer` выше (`ensure_particle_capacity`) — 6 вершин
    // (billboard-квад из 2 треугольников) на частицу.
    particle_pipeline: wgpu::RenderPipeline,
    particle_vertex_buffer: Option<wgpu::Buffer>,
    particle_capacity: u32,

    // Текстура глубины
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,

    // Выходная текстура
    pub output_texture: Option<wgpu::Texture>,
    pub output_view: Option<wgpu::TextureView>,

    // ИСПРАВЛЕНО (баг, найденный пользователем: "перемещение карты
    // лаганное" + "точки освещения и спавн немного смещаются при движении
    // карты"): раньше сюда рендерилось В ОТДЕЛЬНУЮ output-текстуру, потом
    // ЦЕЛИКОМ копировалось на CPU через `copy_texture_to_buffer` +
    // `map_async` (см. историю правок ниже — `readback_buffer`/
    // `copy_in_progress`/`map_rx`, ныне удалены), и только ПОТОМ заново
    // загружалось в egui как НОВАЯ CPU-текстура через `ctx.load_texture`.
    // У этого пути ДВЕ проблемы разом: (1) `map_async` — асинхронный,
    // раньше чем через кадр-другой результат не готов, а `render()` пока
    // это не завершится, СОВСЕМ пропускал кадр (`if copy_in_progress {
    // return; }`) — то есть картинка в вьюпорте реально обновлялась
    // заметно РЕЖЕ, чем рисовались остальные элементы UI, отсюда
    // "лаганность"; (2) оверлей (маркер спавна, лейблы, гизмо — см.
    // app.rs::render_gpu_viewport) считает свои экранные координаты через
    // `world_to_screen` от ТЕКУЩЕЙ камеры КАЖДЫЙ UI-кадр, а фон за ним
    // обновлялся с задержкой в кадр-другой — при движении камеры оверлей
    // и фон оказывались нарисованы для РАЗНЫХ положений камеры, отсюда и
    // "точки/спавн смещаются".
    //
    // Правильный (и заодно кратно более дешёвый) путь — тот, для которого
    // egui_wgpu вообще существует: регистрируем ЭТУ ЖЕ wgpu-текстуру
    // напрямую в `egui_wgpu::Renderer` (`register_native_texture`) один
    // раз (и заново при пересоздании текстуры/ресайзе) — egui рисует её
    // БЕЗ единого байта копирования через CPU, и в тот же кадр, в который
    // мы её отрендерили (никакого readback, никакой асинхронности,
    // никакой задержки между фоном и оверлеем).
    egui_renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    egui_texture_id: Option<egui::TextureId>,
    pub texture_size: (u32, u32),

    // Статистика
    pub draw_calls: u32,
    pub triangles_rendered: u32,
    pub surface_format: wgpu::TextureFormat,
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

/// Один частица-инстанс для `GpuRenderer::render()` — уже в мировых
/// координатах (см. `EditorApp::get_gpu_particle_instances`), рендерер сам
/// разворачивает её в billboard-квад лицом к камере.
pub struct ParticleInstance {
    pub position: [f32; 3],
    pub size: f32,
    pub color: [f32; 4],
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

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ParticleVertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl ParticleVertex {
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
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 12,
                    shader_location: 1,
                },
            ],
        }
    }
}

// ============================================================
// Шейдер WGSL
// ============================================================

// Безосвещённый alpha-blend шейдер для частиц — billboard-квады уже
// развёрнуты на CPU (см. `GpuRenderer::render`), здесь только проекция и
// вывод цвета/альфы как есть. `Camera`-структура продублирована из
// PBR_SHADER ниже (не импортируется — отдельный shader module), но layout
// идентичен: переиспользуется тот же `camera_bind_group`.
const PARTICLE_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view_position: vec3<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_particle(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_pos = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_particle(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

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

    /// Единичные right/up векторы камеры в мировых координатах — та же
    /// пара (s, u), что строит `view_matrix()` ниже, вынесена отдельно для
    /// CPU-billboard'а частиц (`GpuRenderer::render`): каждый частица-квад
    /// разворачивается лицом к камере этими же осями.
    pub fn right_up(&self) -> (Vec3, Vec3) {
        let f = (self.target - self.position).normalize();
        let s = f.cross(self.up).normalize();
        let u = s.cross(f);
        (s, u)
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
        egui_renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
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

        // Particle pipeline — отдельный от основного `pipeline` выше:
        // alpha blend вместо REPLACE, без записи в depth (частицы не должны
        // затенять друг друга по глубине), но с depth-тестом против уже
        // отрисованных мешей (та же `depth_view`, см. `render()`). Только
        // camera bind group — шейдер не использует свет/модель/материал.
        let particle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Particle Shader"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_SHADER.into()),
        });
        let particle_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Particle Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });
        let particle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Particle Pipeline"),
            layout: Some(&particle_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &particle_shader,
                entry_point: Some("vs_particle"),
                compilation_options: Default::default(),
                buffers: &[ParticleVertex::vertex_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &particle_shader,
                entry_point: Some("fs_particle"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
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
            particle_pipeline,
            particle_vertex_buffer: None,
            particle_capacity: 0,
            depth_texture,
            depth_view,
            output_texture: None,
            output_view: None,
            egui_renderer,
            egui_texture_id: None,
            texture_size: (width, height),
            draw_calls: 0,
            triangles_rendered: 0,
            surface_format: format,
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

    /// Гарантирует, что `particle_vertex_buffer` вмещает `needed` частиц
    /// (6 вершин каждая) — тот же паттерн роста, что `ensure_model_capacity`
    /// выше, буфер целиком перезаписывается каждый кадр в `render()`, так
    /// что сохранять его прошлое содержимое при пересоздании не нужно.
    fn ensure_particle_capacity(&mut self, needed: u32) {
        if needed <= self.particle_capacity && self.particle_vertex_buffer.is_some() {
            return;
        }
        let new_capacity = needed.max(self.particle_capacity.saturating_mul(2)).max(64);
        self.particle_vertex_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle Vertex Buffer"),
            size: new_capacity as u64 * 6 * std::mem::size_of::<ParticleVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.particle_capacity = new_capacity;
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

            // ИЗМЕНЕНО (см. подробный комментарий у полей `egui_renderer`/
            // `egui_texture_id` в определении struct — устранение
            // лаганности/рассинхронизации оверлея с фоном): `COPY_SRC`
            // больше не нужен (никто больше не копирует эту текстуру в CPU
            // буфер) — вместо него `TEXTURE_BINDING`, чтобы egui_wgpu мог
            // сэмплировать её напрямую как обычную текстуру в своём
            // собственном шейдере отрисовки UI.
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Output Texture"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Регистрируем ЭТУ САМУЮ view напрямую в egui_wgpu — начиная с
            // этого момента `egui_texture_id` показывает ЛЮБОЙ будущий
            // рендер в неё БЕЗ дополнительных вызовов (никакого аналога
            // "update" не нужно — egui каждый кадр сэмплирует ровно тот же
            // GPU-ресурс). Старый id (если был — например, после ресайза
            // окна) сначала освобождаем, иначе texture manager egui копит
            // дескрипторы на уже ненужные view.
            {
                let mut renderer = self.egui_renderer.write();
                if let Some(old_id) = self.egui_texture_id.take() {
                    renderer.free_texture(&old_id);
                }
                let id = renderer.register_native_texture(&self.device, &view, wgpu::FilterMode::Linear);
                self.egui_texture_id = Some(id);
            }

            self.output_texture = Some(texture);
            self.output_view = Some(view);
            self.texture_size = (w, h);
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
        particles: &[ParticleInstance],
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

        // Billboard-развёртка частиц в вершины и запись в GPU-буфер — ДО
        // заимствования `self.output_view` ниже (не после): `ensure_particle_
        // capacity` требует `&mut self`, а `output_view` ниже держит
        // заимствование `self.output_view` до конца функции, с которым
        // любой последующий вызов `&mut self`-метода конфликтовал бы.
        let particle_vertex_count: u32 = if particles.is_empty() {
            0
        } else {
            let (right, up) = self.camera.right_up();
            let mut particle_vertices: Vec<ParticleVertex> = Vec::with_capacity(particles.len() * 6);
            for p in particles {
                let center = Vec3::new(p.position[0], p.position[1], p.position[2]);
                let half = p.size * 0.5;
                let r = right * half;
                let u = up * half;
                let corners = [center - r - u, center + r - u, center + r + u, center - r + u];
                let vert = |c: Vec3| ParticleVertex { position: [c.x, c.y, c.z], color: p.color };
                particle_vertices.push(vert(corners[0]));
                particle_vertices.push(vert(corners[1]));
                particle_vertices.push(vert(corners[2]));
                particle_vertices.push(vert(corners[0]));
                particle_vertices.push(vert(corners[2]));
                particle_vertices.push(vert(corners[3]));
            }
            self.ensure_particle_capacity(particles.len() as u32);
            if let Some(buf) = &self.particle_vertex_buffer {
                self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&particle_vertices));
            }
            particle_vertices.len() as u32
        };

        let output_view = match &self.output_view {
            Some(v) => v,
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

        // Второй проход — частицы, поверх уже нарисованных мешей в ТОМ ЖЕ
        // encoder'е (LoadOp::Load сохраняет содержимое цвета/глубины из
        // прохода выше вместо очистки). Billboard-развёртка (4 угла на
        // частицу из right/up камеры) считается на CPU — при типичных для
        // редактора количествах частиц (сотни-тысячи) это на порядки
        // дешевле кадра, чем сама отрисовка, и не требует geometry/compute
        // шейдеров.
        if particle_vertex_count > 0 {
            let mut particle_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Particle Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            particle_pass.set_pipeline(&self.particle_pipeline);
            particle_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            if let Some(buf) = &self.particle_vertex_buffer {
                particle_pass.set_vertex_buffer(0, buf.slice(..));
                particle_pass.draw(0..particle_vertex_count, 0..1);
            }
            self.draw_calls += 1;
        }

        // ИЗМЕНЕНО: раньше здесь был `copy_texture_to_buffer` в readback-буфер
        // + `copy_in_progress = true` (весь механизм async-чтения с CPU, см.
        // подробный комментарий у полей `egui_renderer`/`egui_texture_id`
        // выше) — теперь `output_view` уже зарегистрирована напрямую в egui
        // (см. `ensure_output_texture`), никакого копирования не нужно:
        // просто отправляем кадр на GPU, и он тут же виден там же, где
        // зарегистрирован `egui_texture_id`.
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Текстура для отображения в egui — зарегистрирована напрямую на GPU
    /// (см. `ensure_output_texture`), обновляется автоматически каждым
    /// вызовом `render()` без какого-либо дополнительного шага.
    pub fn get_egui_texture(&self) -> Option<egui::TextureId> {
        self.egui_texture_id
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

    /// ДОБАВЛЕНО (редактор вершин — по прямому запросу пользователя):
    /// дешёвое обновление УЖЕ существующего GPU-меша на месте
    /// (`queue.write_buffer`, без пересоздания буфера) — используется во
    /// время перетаскивания вершин gizmo, когда КОЛИЧЕСТВО вершин/индексов
    /// не меняется каждый кадр, только их позиции/нормали. `add_mesh` (новый
    /// буфер на каждый вызов) для этого пути слишком дорог — при
    /// перетаскивании вызывается потенциально десятки раз в секунду, и
    /// каждый вызов `add_mesh` не освобождает старый буфер (см. комментарий
    /// у `GpuRenderer::meshes` — растущий `Vec`, индексы должны оставаться
    /// стабильными), так что бездумный вызов `add_mesh` каждый кадр драга
    /// быстро набрал бы сотни мегабайт мусора за одну сессию правки.
    ///
    /// Возвращает `false` (ничего не меняя), если число вершин/индексов
    /// разошлось с уже выделенным буфером — тогда вызывающий код (см.
    /// `EditorApp::refresh_gpu_mesh_after_structural_edit`) обязан вместо
    /// этого вызвать `add_mesh` и обновить свою карту `id -> mesh_idx`
    /// (структурные правки — экструзия/удаление — меняют количество вершин
    /// и происходят РЕДКО, по одной операции за раз, не каждый кадр, так что
    /// цена нового буфера там уже не проблема).
    pub fn update_mesh_vertices(&mut self, mesh_idx: usize, mesh: &Mesh) -> bool {
        let Some(gpu_mesh) = self.meshes.get(mesh_idx) else { return false; };

        let expected_vertex_bytes = (mesh.vertices.len() * std::mem::size_of::<Vertex3D>()) as u64;
        let expected_index_bytes = (mesh.indices.len() * 4) as u64;
        if gpu_mesh.vertex_buffer.size() != expected_vertex_bytes || gpu_mesh.index_buffer.size() != expected_index_bytes {
            return false;
        }

        let mut vertices = Vec::with_capacity(mesh.vertices.len());
        for i in 0..mesh.vertices.len() {
            let normal = if i < mesh.normals.len() { mesh.normals[i] } else { Vec3::UP };
            vertices.push(Vertex3D {
                position: [mesh.vertices[i].x, mesh.vertices[i].y, mesh.vertices[i].z],
                normal: [normal.x, normal.y, normal.z],
                color: [0.7, 0.7, 0.7],
            });
        }

        self.queue.write_buffer(&gpu_mesh.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        // Индексы при простом перемещении вершин не меняются, но пишем их
        // тоже — эта функция вызывается и после операций, где порядок
        // индексов мог быть переставлен без изменения ИХ ЧИСЛА (сейчас
        // таких нет, но дешёвая защита на будущее не помешает).
        self.queue.write_buffer(&gpu_mesh.index_buffer, 0, bytemuck::cast_slice(&mesh.indices));

        true
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