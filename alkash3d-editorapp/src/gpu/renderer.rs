// src/gpu/renderer.rs
use wgpu::*;
use winit::window::Window;
use std::sync::Arc;
use super::camera::Camera;
use super::mesh::GpuMesh;
use super::material::GpuMaterial;
use super::light::GpuLight;
use super::pipeline::PipelineManager;

pub struct Renderer {
    pub device: Device,
    pub queue: Queue,
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
    pub size: (u32, u32),
    pub depth_view: TextureView,
    pub _depth_texture: Texture,
    pub camera: Camera,
    pub pipelines: PipelineManager,
    pub meshes: Vec<GpuMesh>,
    pub materials: Vec<GpuMaterial>,
    pub lights: Vec<GpuLight>,
    pub sky_bind_group: BindGroup,
    pub light_bind_group: BindGroup,
    pub _window: Arc<Window>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, Box<dyn std::error::Error>> {
        let size: (u32, u32) = window.inner_size().into();
        let instance = Instance::new(&InstanceDescriptor { backends: Backends::all(), ..Default::default() });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance.request_adapter(&RequestAdapterOptions { power_preference: PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false }).await.unwrap();
        let (device, queue) = adapter.request_device(&DeviceDescriptor { required_features: Features::empty(), required_limits: Limits::default(), ..Default::default() }).await?;
        let caps = surface.get_capabilities(&adapter);
        let fmt = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = SurfaceConfiguration { usage: TextureUsages::RENDER_ATTACHMENT, format: fmt, width: size.0, height: size.1, present_mode: PresentMode::AutoVsync, alpha_mode: caps.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        let (dt, dv) = Self::depth(&device, &config);
        let pipelines = PipelineManager::new(&device, fmt);
        let mut camera = Camera::new(size.0 as f32 / size.1 as f32);
        camera.update_bind_group(&device, &queue);
        let cam_bg = Self::make_bind_group(&device, &pipelines.pbr_bind_group_layout, camera.buffer.as_ref().unwrap());
        let (sb, sky_bg, lb, light_bg) = Self::make_sky_light_bindings(&device, &pipelines);
        camera.bind_group = Some(cam_bg);
        Ok(Self { device, queue, surface, config, size, depth_view: dv, _depth_texture: dt, camera, pipelines, meshes: vec![], materials: vec![], lights: vec![], sky_bind_group: sky_bg, light_bind_group: light_bg, _window: window })
    }

    fn depth(device: &Device, config: &SurfaceConfiguration) -> (Texture, TextureView) {
        let t = device.create_texture(&TextureDescriptor { label: Some("D"), size: Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Depth32Float, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
        let v = t.create_view(&TextureViewDescriptor::default());
        (t, v)
    }

    fn make_bind_group(device: &Device, layout: &BindGroupLayout, buffer: &Buffer) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor { label: Some("CamBG"), layout, entries: &[BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }] })
    }

    fn make_sky_light_bindings(device: &Device, p: &PipelineManager) -> (Buffer, BindGroup, Buffer, BindGroup) {
        let sb = device.create_buffer(&BufferDescriptor { label: Some("SkyBuf"), size: 64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
        let lb = device.create_buffer(&BufferDescriptor { label: Some("LightBuf"), size: 512, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
        let ct = device.create_texture(&TextureDescriptor { label: Some("Cube"), size: Extent3d { width: 1, height: 1, depth_or_array_layers: 6 }, mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2, format: TextureFormat::Rgba8UnormSrgb, usage: TextureUsages::TEXTURE_BINDING, view_formats: &[] });
        let cv = ct.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..Default::default() });
        let sm = device.create_sampler(&SamplerDescriptor { label: Some("Smp"), ..Default::default() });
        let sky_bg = device.create_bind_group(&BindGroupDescriptor { label: Some("SkyBG"), layout: &p.sky_bind_group_layout, entries: &[BindGroupEntry { binding: 0, resource: sb.as_entire_binding() }, BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&cv) }, BindGroupEntry { binding: 2, resource: BindingResource::Sampler(&sm) }] });
        let light_bg = device.create_bind_group(&BindGroupDescriptor { label: Some("LightBG"), layout: &p.light_bind_group_layout, entries: &[BindGroupEntry { binding: 0, resource: lb.as_entire_binding() }] });
        (sb, sky_bg, lb, light_bg)
    }

    pub fn render(&mut self) -> Result<(), SurfaceError> {
        self.camera.update_view_matrix();
        if let Some(ref buf) = self.camera.buffer { self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&[self.camera.uniform])); }
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&TextureViewDescriptor::default());
        let mut enc = self.device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut rp = enc.begin_render_pass(&RenderPassDescriptor { label: None, color_attachments: &[Some(RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }), store: StoreOp::Store } })], depth_stencil_attachment: Some(RenderPassDepthStencilAttachment { view: &self.depth_view, depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }), stencil_ops: None }), timestamp_writes: None, occlusion_query_set: None });
            rp.set_pipeline(&self.pipelines.skybox_pipeline);
            if let Some(ref bg) = self.camera.bind_group { rp.set_bind_group(0, bg, &[]); }
            rp.set_bind_group(1, &self.sky_bind_group, &[]);
            rp.draw(0..3, 0..1);
            for mesh in &self.meshes {
                if !mesh.visible { continue; }
                let mat = &self.materials[mesh.material_index.min(self.materials.len().saturating_sub(1))];
                rp.set_pipeline(&self.pipelines.pbr_pipeline);
                if let Some(ref bg) = self.camera.bind_group { rp.set_bind_group(0, bg, &[]); }
                rp.set_bind_group(1, &mat.bind_group, &[]);
                rp.set_bind_group(2, &self.light_bind_group, &[]);
                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rp.set_index_buffer(mesh.index_buffer.slice(..), IndexFormat::Uint32);
                rp.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }
        self.queue.submit(std::iter::once(enc.finish()));
        output.present();
        Ok(())
    }
}