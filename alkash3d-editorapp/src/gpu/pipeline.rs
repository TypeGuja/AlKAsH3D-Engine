use wgpu::*;

pub struct PipelineManager {
    pub pbr_pipeline: RenderPipeline,
    pub wireframe_pipeline: RenderPipeline,
    pub skybox_pipeline: RenderPipeline,
    pub pbr_bind_group_layout: BindGroupLayout,
    pub material_bind_group_layout: BindGroupLayout,
    pub light_bind_group_layout: BindGroupLayout,
    pub sky_bind_group_layout: BindGroupLayout,
}

impl PipelineManager {
    pub fn new(device: &Device, surface_format: TextureFormat) -> Self {
        let pbr_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("PBR Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/pbr.wgsl").into()),
        });

        let skybox_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Skybox Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/skybox.wgsl").into()),
        });

        // Bind Group Layouts
        let pbr_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("PBR Camera"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let material_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Material"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let light_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Light"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let sky_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Sky"),
            entries: &[
                BindGroupLayoutEntry { binding: 0, visibility: ShaderStages::FRAGMENT, ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                BindGroupLayoutEntry { binding: 1, visibility: ShaderStages::FRAGMENT, ty: BindingType::Texture { sample_type: TextureSampleType::Float { filterable: true }, view_dimension: TextureViewDimension::Cube, multisampled: false }, count: None },
                BindGroupLayoutEntry { binding: 2, visibility: ShaderStages::FRAGMENT, ty: BindingType::Sampler(SamplerBindingType::Filtering), count: None },
            ],
        });

        let pbr_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("PBR Layout"),
            bind_group_layouts: &[&pbr_bind_group_layout, &material_bind_group_layout, &light_bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_attrs = [
            VertexAttribute { format: VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            VertexAttribute { format: VertexFormat::Float32x3, offset: 12, shader_location: 1 },
            VertexAttribute { format: VertexFormat::Float32x2, offset: 24, shader_location: 2 },
        ];

        let vb_layout = VertexBufferLayout { array_stride: 32, step_mode: VertexStepMode::Vertex, attributes: &vertex_attrs };
        let vb_layout2 = VertexBufferLayout { array_stride: 32, step_mode: VertexStepMode::Vertex, attributes: &vertex_attrs };

        // PBR Pipeline
        let pbr_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("PBR"),
            layout: Some(&pbr_layout),
            vertex: VertexState { module: &pbr_shader, entry_point: Option::from("vs_main"), buffers: &[vb_layout], compilation_options: Default::default() },
            fragment: Some(FragmentState { module: &pbr_shader, entry_point: Option::from("fs_main"), targets: &[Some(ColorTargetState { format: surface_format, blend: Some(BlendState::REPLACE), write_mask: ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: PrimitiveState { topology: PrimitiveTopology::TriangleList, front_face: FrontFace::Ccw, cull_mode: Some(Face::Back), polygon_mode: PolygonMode::Fill, ..Default::default() },
            depth_stencil: Some(DepthStencilState { format: TextureFormat::Depth32Float, depth_write_enabled: true, depth_compare: CompareFunction::Less, stencil: StencilState::default(), bias: DepthBiasState::default() }),
            multisample: MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
            multiview: None,
            cache: None,
        });

        // Wireframe
        let wireframe_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Wireframe"),
            layout: Some(&pbr_layout),
            vertex: VertexState { module: &pbr_shader, entry_point: Option::from("vs_main"), buffers: &[vb_layout2], compilation_options: Default::default() },
            fragment: Some(FragmentState { module: &pbr_shader, entry_point: Option::from("fs_main"), targets: &[Some(ColorTargetState { format: surface_format, blend: Some(BlendState::REPLACE), write_mask: ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: PrimitiveState { polygon_mode: PolygonMode::Line, ..PrimitiveState::default() },
            depth_stencil: Some(DepthStencilState { format: TextureFormat::Depth32Float, depth_write_enabled: true, depth_compare: CompareFunction::Less, stencil: StencilState::default(), bias: DepthBiasState::default() }),
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sky_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Sky Layout"),
            bind_group_layouts: &[&pbr_bind_group_layout, &sky_bind_group_layout],
            push_constant_ranges: &[],
        });

        let skybox_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Skybox"),
            layout: Some(&sky_layout),
            vertex: VertexState { module: &skybox_shader, entry_point: Option::from("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(FragmentState { module: &skybox_shader, entry_point: Option::from("fs_main"), targets: &[Some(ColorTargetState { format: surface_format, blend: Some(BlendState::REPLACE), write_mask: ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: PrimitiveState { topology: PrimitiveTopology::TriangleList, front_face: FrontFace::Cw, cull_mode: None, ..Default::default() },
            depth_stencil: Some(DepthStencilState { format: TextureFormat::Depth32Float, depth_write_enabled: false, depth_compare: CompareFunction::LessEqual, stencil: StencilState::default(), bias: DepthBiasState::default() }),
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self { pbr_pipeline, wireframe_pipeline, skybox_pipeline, pbr_bind_group_layout, material_bind_group_layout, light_bind_group_layout, sky_bind_group_layout }
    }
}