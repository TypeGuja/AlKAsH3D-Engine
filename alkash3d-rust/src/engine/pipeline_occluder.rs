//! Occlusion culling на ВТОРОЙ видеокарте (explicit multi-adapter D3D12):
//! минимальные depth-таргет/буфер типы для secondary device
//! (`SecondaryDepthTarget`/`SecondaryBuffer` — не могут переиспользовать
//! `crate::render::RenderTexture`/`crate::buffer::Buffer`, те жёстко привязаны
//! к первой карте), depth-only проход instanced AABB-боксов, отправка без
//! ожидания (`submit_occluder_pass`) и неблокирующее чтение результата
//! (`poll_occluder_readback`).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_UINT, DXGI_FORMAT_UNKNOWN};
use crate::shader::ShaderBlob;
use crate::math::Mat4;
use super::{AlkashEngine, OCCLUDER_DEPTH_RESOLUTION, align_to_256};

/// Минимальный (persistent) depth-таргет на ВТОРОЙ карте — в отличие от
/// `crate::render::RenderTexture`, которая жёстко использует
/// `crate::get_device()` (первая карта, см. её методы `create_shadow_map`/
/// `create_dsv`), эти два метода — единственное, что реально нужно
/// occluder-проходу (SRV не нужен вообще — читаем depth только через
/// READBACK-копию, не через шейдер).
pub(super) struct SecondaryDepthTarget {
    pub(super) resource: ID3D12Resource,
    width: u32,
    height: u32,
}

impl SecondaryDepthTarget {
    /// ТОЧНАЯ копия паттерна `RenderTexture::create_shadow_map` (см.
    /// render.rs) — тот же TYPELESS-ресурс/DEPTH_WRITE/clear value, но
    /// созданный через `device`, переданный явно (а не всегда
    /// `crate::get_device()`), чтобы работать со ВТОРЫМ устройством.
    fn create(device: &ID3D12Device, resolution: u32) -> Result<Self> {
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_D32_FLOAT, DXGI_SAMPLE_DESC};
        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: resolution as u64,
            Height: resolution,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R32_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };
        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } },
        };
        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                Some(&clear_value),
                &mut resource,
            )?;
            let resource = resource.ok_or_else(|| {
                eprintln!("[ENGINE] ERROR: occluder depth target (secondary GPU) resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            Ok(Self { resource, width: resolution, height: resolution })
        }
    }

    fn create_dsv(&self, device: &ID3D12Device, handle: D3D12_CPU_DESCRIPTOR_HANDLE) {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT;
        let desc = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 },
            },
        };
        unsafe {
            device.CreateDepthStencilView(&self.resource, Some(&desc), handle);
        }
    }
}

/// Минимальный переиспользуемый GPU-буфер на ВТОРОЙ карте — аналог
/// `crate::buffer::Buffer`, но с явно переданным `device` вместо жёстко
/// зашитого `crate::get_device()`. Поддерживает только то, что реально
/// нужно occluder-проходу: UPLOAD-буфер (CPU пишет каждый кадр/один раз)
/// и READBACK-буфер (CPU читает после GPU-копирования).
pub(super) struct SecondaryBuffer {
    pub(super) resource: ID3D12Resource,
    pub(super) size: u64,
}

impl SecondaryBuffer {
    fn create_upload(device: &ID3D12Device, size_bytes: u64) -> Result<Self> {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
        let size = size_bytes.max(4);
        let heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };
        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            )?;
            let resource = resource.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;
            Ok(Self { resource, size })
        }
    }

    fn create_readback(device: &ID3D12Device, size_bytes: u64) -> Result<Self> {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
        let size = size_bytes.max(4);
        let heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };
        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut resource,
            )?;
            let resource = resource.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;
            Ok(Self { resource, size })
        }
    }

    fn update(&self, data: &[u8]) -> Result<()> {
        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.resource.Map(0, None, Some(&mut mapped))?;
            if !mapped.is_null() {
                let len = data.len().min(self.size as usize);
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, len);
            }
            self.resource.Unmap(0, None);
        }
        Ok(())
    }
}

impl AlkashEngine {
    /// VS occluder-прохода — рисует ЕДИНСТВЕННЫЙ unit-cube instanced на
    /// произвольное число occluder'ов за один DrawIndexedInstanced. Слот 0
    /// (per-vertex) — угол unit-куба в диапазоне [-1;1] по каждой оси;
    /// слот 1 (per-instance) — world-space AABB min/max этого occluder'а.
    /// `worldPos = lerp(instanceMin, instanceMax, unitCube*0.5+0.5)`
    /// разворачивает единичный куб в конкретный мировой AABB БЕЗ отдельной
    /// per-instance world-матрицы — дешевле и проще, чем константный буфер
    /// на каждый occluder (как у shadow/основного прохода), и не нужен:
    /// AABB occluder'а осесимметричен по построению (см. `OCCLUDER_INSCRIBE_FACTOR`),
    /// поворот геометрии не имеет смысла для грубого прямоугольного occluder'а.
    pub(super) fn compile_occluder_shaders(&mut self) -> Result<()> {
        if crate::get_secondary_device().is_none() {
            return Ok(());
        }
        let vs_source = r#"
        cbuffer OccluderConstants : register(b0) {
            float4x4 viewProj;
        };

        struct VS_INPUT {
            float3 unitCubePos : POSITION;
            float3 instanceMin : INSTANCE_MIN;
            float3 instanceMax : INSTANCE_MAX;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            float3 t = input.unitCubePos * 0.5 + 0.5;
            float3 worldPos = lerp(input.instanceMin, input.instanceMax, t);
            output.pos = mul(viewProj, float4(worldPos, 1.0));
            return output;
        }
        "#;
        self.occluder_vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        println!("[ENGINE] ✓ Occluder shaders compiled (вторая карта, depth-only, instanced boxes)");
        Ok(())
    }

    /// Root signature occluder-прохода (СЕКОНДАРНОЕ устройство!) — один
    /// CBV (b0), точная структурная копия `create_shadow_root_signature`,
    /// но `CreateRootSignature` вызывается через `crate::get_secondary_device()`,
    /// т.к. root signature — объект, привязанный к конкретному device.
    pub(super) fn create_occluder_root_signature(&mut self) -> Result<()> {
        let Some(device) = crate::get_secondary_device() else { return Ok(()); };

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: 0, RegisterSpace: 0 },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
            },
        ];
        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
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
                    let err_data = std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize());
                    eprintln!("Occluder root signature error: {}", String::from_utf8_lossy(err_data));
                }
                eprintln!("[ENGINE] WARNING: не удалось создать occluder root signature — occlusion culling на второй карте будет неактивен");
                return Ok(());
            }
            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
            match device.CreateRootSignature(0, blob_data) {
                Ok(root_sig) => {
                    self.occluder_root_signature = Some(root_sig);
                    println!("[ENGINE] ✓ Occluder root signature created (вторая карта, CBV b0)");
                }
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: CreateRootSignature (occluder, вторая карта) failed: {:?} — occlusion culling будет неактивен", e);
                }
            }
        }
        Ok(())
    }

    /// PSO occluder-прохода (СЕКОНДАРНОЕ устройство) — depth-only, БЕЗ PS,
    /// два input-слота (per-vertex unit cube corner + per-instance AABB
    /// min/max), см. `compile_occluder_shaders` про семантику полей.
    pub(super) fn create_occluder_pipeline_state(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_D32_FLOAT, DXGI_SAMPLE_DESC};
        let (Some(device), Some(vs), Some(root_sig)) = (
            crate::get_secondary_device(),
            self.occluder_vs.as_ref(),
            self.occluder_root_signature.as_ref(),
        ) else {
            return Ok(());
        };

        let input_elements = [
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("INSTANCE_MIN"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 1,
                AlignedByteOffset: 0,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D12_INPUT_ELEMENT_DESC {
                SemanticName: s!("INSTANCE_MAX"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 1,
                AlignedByteOffset: 12,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
        ];
        let input_layout = D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_elements.as_ptr(),
            NumElements: input_elements.len() as u32,
        };

        let rasterizer = D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: FALSE,
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: TRUE,
            MultisampleEnable: FALSE,
            AntialiasedLineEnable: FALSE,
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        };
        let blend_desc = D3D12_BLEND_DESC {
            AlphaToCoverageEnable: FALSE,
            IndependentBlendEnable: FALSE,
            RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC {
                BlendEnable: FALSE,
                LogicOpEnable: FALSE,
                SrcBlend: D3D12_BLEND_ONE,
                DestBlend: D3D12_BLEND_ZERO,
                BlendOp: D3D12_BLEND_OP_ADD,
                SrcBlendAlpha: D3D12_BLEND_ONE,
                DestBlendAlpha: D3D12_BLEND_ZERO,
                BlendOpAlpha: D3D12_BLEND_OP_ADD,
                LogicOp: D3D12_LOGIC_OP_NOOP,
                RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
            }; 8],
        };
        let depth_stencil = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: TRUE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
            StencilEnable: FALSE,
            StencilReadMask: D3D12_DEFAULT_STENCIL_READ_MASK as u8,
            StencilWriteMask: D3D12_DEFAULT_STENCIL_WRITE_MASK as u8,
            FrontFace: D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
                StencilPassOp: D3D12_STENCIL_OP_KEEP,
                StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            },
            BackFace: D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
                StencilPassOp: D3D12_STENCIL_OP_KEEP,
                StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            },
        };

        let mut pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::ManuallyDrop::new(Some(root_sig.clone())),
            VS: D3D12_SHADER_BYTECODE { pShaderBytecode: vs.as_ptr(), BytecodeLength: vs.size() },
            PS: D3D12_SHADER_BYTECODE::default(),
            DS: D3D12_SHADER_BYTECODE::default(),
            HS: D3D12_SHADER_BYTECODE::default(),
            GS: D3D12_SHADER_BYTECODE::default(),
            StreamOutput: D3D12_STREAM_OUTPUT_DESC::default(),
            BlendState: blend_desc,
            SampleMask: u32::MAX,
            RasterizerState: rasterizer,
            DepthStencilState: depth_stencil,
            InputLayout: input_layout,
            IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 0,
            RTVFormats: [DXGI_FORMAT_UNKNOWN; 8],
            DSVFormat: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE::default(),
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
            CS: D3D12_SHADER_BYTECODE::default(),
        };

        let result = unsafe { device.CreateGraphicsPipelineState(&pso_desc) };
        unsafe { std::mem::ManuallyDrop::drop(&mut pso_desc.pRootSignature); }

        match result {
            Ok(pso) => {
                self.occluder_pipeline_state = Some(pso);
                println!("[ENGINE] ✓ Occluder pipeline state created (вторая карта, depth-only, instanced)");
            }
            Err(e) => {
                eprintln!("[ENGINE] WARNING: CreateGraphicsPipelineState (occluder, вторая карта) failed: {:?} — occlusion culling будет неактивен", e);
            }
        }
        Ok(())
    }

    /// Depth-таргет + DSV heap + статический unit-cube VB/IB + постоянные
    /// command allocator/list + fence occluder-прохода — всё на ВТОРОЙ
    /// карте. Вызывается один раз в init(), симметрично `create_shadow_resources`.
    pub(super) fn create_occluder_resources(&mut self) -> Result<()> {
        let Some(device) = crate::get_secondary_device() else { return Ok(()); };

        let depth_target = match SecondaryDepthTarget::create(&device, OCCLUDER_DEPTH_RESOLUTION) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder depth target на второй карте: {:?} — occlusion culling будет неактивен", e);
                return Ok(());
            }
        };

        let dsv_heap = match crate::heap::DescriptorHeap::create_dsv_heap_on_device(&device, 1) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать DSV heap второй карты для occlusion culling: {:?}", e);
                return Ok(());
            }
        };
        let dsv_size = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) };
        let dsv_handle = crate::heap::DescriptorHeap::get_cpu_handle(&dsv_heap, 0, dsv_size);
        depth_target.create_dsv(&device, dsv_handle);

        #[rustfmt::skip]
        let cube_vertices: [f32; 24] = [
            -1.0, -1.0, -1.0,   1.0, -1.0, -1.0,   1.0,  1.0, -1.0,  -1.0,  1.0, -1.0,
            -1.0, -1.0,  1.0,   1.0, -1.0,  1.0,   1.0,  1.0,  1.0,  -1.0,  1.0,  1.0,
        ];
        #[rustfmt::skip]
        let cube_indices: [u32; 36] = [
            0,1,2, 0,2,3,
            4,6,5, 4,7,6,
            0,4,5, 0,5,1,
            3,2,6, 3,6,7,
            0,3,7, 0,7,4,
            1,5,6, 1,6,2,
        ];
        let vertex_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(cube_vertices.as_ptr() as *const u8, std::mem::size_of_val(&cube_vertices))
        };
        let index_bytes: Vec<u8> = cube_indices.iter().flat_map(|v| v.to_le_bytes()).collect();

        let cube_vb = match SecondaryBuffer::create_upload(&device, vertex_bytes.len() as u64) {
            Ok(b) => { let _ = b.update(vertex_bytes); b }
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder cube vertex buffer: {:?}", e);
                return Ok(());
            }
        };
        let cube_ib = match SecondaryBuffer::create_upload(&device, index_bytes.len() as u64) {
            Ok(b) => { let _ = b.update(&index_bytes); b }
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder cube index buffer: {:?}", e);
                return Ok(());
            }
        };

        let viewproj_cb = match SecondaryBuffer::create_upload(&device, 256) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder view-proj CBV: {:?}", e);
                return Ok(());
            }
        };

        let command_allocator: ID3D12CommandAllocator = match unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) } {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder command allocator (вторая карта): {:?}", e);
                return Ok(());
            }
        };
        let command_list: ID3D12GraphicsCommandList = match unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &command_allocator, None) } {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder command list (вторая карта): {:?}", e);
                return Ok(());
            }
        };
        if let Err(e) = unsafe { command_list.Close() } {
            eprintln!("[ENGINE] WARNING: не удалось закрыть occluder command list после создания: {:?}", e);
            return Ok(());
        }

        let fence: ID3D12Fence = match unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) } {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder fence (вторая карта): {:?}", e);
                return Ok(());
            }
        };

        let row_pitch = align_to_256(OCCLUDER_DEPTH_RESOLUTION as u64 * 4);
        let readback_size = row_pitch * OCCLUDER_DEPTH_RESOLUTION as u64;
        let readback_buffer = match SecondaryBuffer::create_readback(&device, readback_size) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[ENGINE] WARNING: не удалось создать occluder readback buffer: {:?}", e);
                return Ok(());
            }
        };

        self.occluder_depth_target = Some(depth_target);
        self.occluder_dsv_heap = Some(dsv_heap);
        self.occluder_dsv = dsv_handle;
        self.occluder_cube_vertex_buffer = Some(cube_vb);
        self.occluder_cube_index_buffer = Some(cube_ib);
        self.occluder_viewproj_buffer = Some(viewproj_cb);
        self.occluder_command_allocator = Some(command_allocator);
        self.occluder_command_list = Some(command_list);
        self.occluder_fence = Some(fence);
        self.occluder_readback_buffer = Some(readback_buffer);

        println!(
            "[ENGINE] ✓ Occluder resources created (вторая карта, {}x{} depth target)",
            OCCLUDER_DEPTH_RESOLUTION, OCCLUDER_DEPTH_RESOLUTION
        );
        Ok(())
    }

    /// Растит `occluder_instance_buffer` (вторая карта) до вмещения как
    /// минимум `needed` инстансов — ТОЧНО тот же паттерн степени двойки,
    /// что `ensure_light_buffer_capacity` (см. подробности там), только
    /// на secondary device и с размером слота = 2 x float3 (min+max).
    fn ensure_occluder_instance_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        let Some(device) = crate::get_secondary_device() else { return Ok(()); };
        if self.occluder_instance_buffer.is_some() && needed <= self.occluder_instance_capacity {
            return Ok(());
        }
        let new_capacity = needed.max(64).next_power_of_two();
        let size_bytes = new_capacity as u64 * (6 * std::mem::size_of::<f32>() as u64);
        let buffer = SecondaryBuffer::create_upload(&device, size_bytes)?;
        println!(
            "[ENGINE] Occluder instance buffer (вторая карта) (re)allocated: {} слотов ({} байт)",
            new_capacity, size_bytes
        );
        self.occluder_instance_buffer = Some(buffer);
        self.occluder_instance_capacity = new_capacity;
        Ok(())
    }

    /// Отправляет depth-only occluder-проход на ВТОРУЮ карту — БЕЗ
    /// ожидания результата (никакого `WaitForSingleObject` здесь). Список
    /// AABB окклюдеров (`instance_data`) собирается ВЫЗЫВАЮЩИМ кодом
    /// (`render_frame`, сразу после `jobs.retain(...)` frustum-теста) —
    /// см. doc-комментарий у сигнатуры чуть ниже про то, почему сбор не
    /// сделан прямо здесь. Если предыдущий проход ещё не прочитан (`occluder_pass_in_flight`)
    /// — пропускает отправку целиком в этом кадре, переиспользуя старый
    /// CPU depth-буфер ещё один кадр (одно-двух-кадровая задержка
    /// свежести occluder-данных совершенно не критична — сцена не меняет
    /// крупную геометрию настолько резко от кадра к кадру).
    /// `instance_data` — уже готовый плоский список `[minX,minY,minZ,maxX,maxY,maxZ, ...]`
    /// по одному occluder'у (6 float на инстанс). Собирается ВЫЗЫВАЮЩИМ
    /// кодом (`render_frame`) из уже отфильтрованного фрустумом списка
    /// задач отрисовки — сам `submit_occluder_pass` намеренно не знает
    /// про `DrawJob`/`DrawTransform` (те — локальные типы `render_frame`,
    /// невидимые на уровне impl-метода), поэтому принимает уже
    /// посчитанные AABB как самодостаточные данные.
    pub(super) fn submit_occluder_pass(&mut self, instance_data: &[f32], view: Mat4, proj: Mat4) {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_FLOAT;
        if self.occluder_pass_in_flight {
            return;
        }
        let instance_count = instance_data.len() / 6;
        if instance_count == 0 {
            return;
        }
        if crate::get_secondary_device().is_none() { return; }
        let Some(queue) = crate::get_secondary_command_queue() else { return };
        let (
            Some(pso),
            Some(root_sig),
            Some(depth_target_resource),
            Some(cube_vb_resource),
            Some(cube_vb_size),
            Some(cube_ib_resource),
            Some(cube_ib_size),
            Some(viewproj_cb_resource),
            Some(command_allocator),
            Some(command_list),
            Some(fence),
        ) = (
            self.occluder_pipeline_state.clone(),
            self.occluder_root_signature.clone(),
            self.occluder_depth_target.as_ref().map(|t| t.resource.clone()),
            self.occluder_cube_vertex_buffer.as_ref().map(|b| b.resource.clone()),
            self.occluder_cube_vertex_buffer.as_ref().map(|b| b.size),
            self.occluder_cube_index_buffer.as_ref().map(|b| b.resource.clone()),
            self.occluder_cube_index_buffer.as_ref().map(|b| b.size),
            self.occluder_viewproj_buffer.as_ref().map(|b| b.resource.clone()),
            self.occluder_command_allocator.clone(),
            self.occluder_command_list.clone(),
            self.occluder_fence.clone(),
        )
        else {
            return;
        };

        if let Err(e) = self.ensure_occluder_instance_buffer_capacity(instance_count) {
            eprintln!("[ENGINE] WARNING: не удалось вырастить occluder instance buffer: {:?}", e);
            return;
        }
        let Some(instance_buffer_resource) = self.occluder_instance_buffer.as_ref().map(|b| b.resource.clone()) else { return };
        let Some(instance_buffer) = self.occluder_instance_buffer.as_ref() else { return };
        let instance_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(instance_data.as_ptr() as *const u8, instance_data.len() * 4)
        };
        if instance_buffer.update(instance_bytes).is_err() {
            return;
        }

        let Some(viewproj_cb) = self.occluder_viewproj_buffer.as_ref() else { return };
        let view_proj = proj * view;
        let vp_bytes: [f32; 16] = view_proj.to_cols_array();
        let vp_u8: &[u8] = unsafe { std::slice::from_raw_parts(vp_bytes.as_ptr() as *const u8, 64) };
        if viewproj_cb.update(vp_u8).is_err() {
            return;
        }

        let occluder_dsv = self.occluder_dsv;
        let Some(readback_buffer_resource) = self.occluder_readback_buffer.as_ref().map(|b| b.resource.clone()) else { return };

        unsafe {
            if command_allocator.Reset().is_err() { return; }
            if command_list.Reset(&command_allocator, None).is_err() { return; }

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0, TopLeftY: 0.0,
                Width: OCCLUDER_DEPTH_RESOLUTION as f32, Height: OCCLUDER_DEPTH_RESOLUTION as f32,
                MinDepth: 0.0, MaxDepth: 1.0,
            };
            let scissor = RECT { left: 0, top: 0, right: OCCLUDER_DEPTH_RESOLUTION as i32, bottom: OCCLUDER_DEPTH_RESOLUTION as i32 };
            command_list.RSSetViewports(&[viewport]);
            command_list.RSSetScissorRects(&[scissor]);
            command_list.OMSetRenderTargets(0, None, false, Some(&occluder_dsv));
            command_list.ClearDepthStencilView(occluder_dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

            command_list.SetPipelineState(&pso);
            command_list.SetGraphicsRootSignature(&root_sig);
            command_list.SetGraphicsRootConstantBufferView(0, viewproj_cb_resource.GetGPUVirtualAddress());
            command_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

            let cube_vbv = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: cube_vb_resource.GetGPUVirtualAddress(),
                SizeInBytes: cube_vb_size as u32,
                StrideInBytes: 12,
            };
            let instance_vbv = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: instance_buffer_resource.GetGPUVirtualAddress(),
                SizeInBytes: (instance_count * 24) as u32,
                StrideInBytes: 24,
            };
            command_list.IASetVertexBuffers(0, Some(&[cube_vbv, instance_vbv]));
            let index_view = D3D12_INDEX_BUFFER_VIEW {
                BufferLocation: cube_ib_resource.GetGPUVirtualAddress(),
                SizeInBytes: cube_ib_size as u32,
                Format: DXGI_FORMAT_R32_UINT,
            };
            command_list.IASetIndexBuffer(Some(&index_view));
            command_list.DrawIndexedInstanced(36, instance_count as u32, 0, 0, 0);

            let mut barrier = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: std::mem::ManuallyDrop::new(Some(depth_target_resource.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        StateAfter: D3D12_RESOURCE_STATE_COPY_SOURCE,
                    }),
                },
            };
            command_list.ResourceBarrier(&[barrier.clone()]);
            std::mem::ManuallyDrop::drop(&mut barrier.Anonymous.Transition);

            let row_pitch = align_to_256(OCCLUDER_DEPTH_RESOLUTION as u64 * 4);
            let src_location = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(depth_target_resource.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            let dst_location = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(readback_buffer_resource.clone())),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: DXGI_FORMAT_R32_FLOAT,
                            Width: OCCLUDER_DEPTH_RESOLUTION,
                            Height: OCCLUDER_DEPTH_RESOLUTION,
                            Depth: 1,
                            RowPitch: row_pitch as u32,
                        },
                    },
                },
            };
            command_list.CopyTextureRegion(&dst_location, 0, 0, 0, &src_location, None);

            let mut barrier_back = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: std::mem::ManuallyDrop::new(Some(depth_target_resource.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_COPY_SOURCE,
                        StateAfter: D3D12_RESOURCE_STATE_DEPTH_WRITE,
                    }),
                },
            };
            command_list.ResourceBarrier(&[barrier_back.clone()]);
            std::mem::ManuallyDrop::drop(&mut barrier_back.Anonymous.Transition);

            if command_list.Close().is_err() { return; }
            let cmd_lists: [Option<ID3D12CommandList>; 1] = [Some(command_list.clone().into())];
            queue.ExecuteCommandLists(&cmd_lists);

            self.occluder_fence_value += 1;
            if queue.Signal(&fence, self.occluder_fence_value).is_err() { return; }
            self.occluder_pass_in_flight = true;
        }
    }

    /// Опрашивает (НЕ ждёт) fence occluder-прохода в начале кадра — если
    /// GPU второй карты уже закончил (Part 2a проход отправляется не
    /// каждый кадр, так что обычно готово задолго до опроса), читает
    /// READBACK-буфер и обновляет `self.occluder_depth_cpu`. Если ещё не
    /// готово — просто выходит, оставляя `occluder_pass_in_flight = true`
    /// и старый (если был) `occluder_depth_cpu` как есть: НИКОГДА не
    /// блокирует основной кадр ожиданием второй карты.
    pub(super) fn poll_occluder_readback(&mut self) {
        if !self.occluder_pass_in_flight {
            return;
        }
        let Some(fence) = self.occluder_fence.as_ref() else {
            self.occluder_pass_in_flight = false;
            return;
        };
        let completed = unsafe { fence.GetCompletedValue() };
        if completed < self.occluder_fence_value {
            return;
        }
        self.occluder_pass_in_flight = false;

        let Some(readback_buffer) = self.occluder_readback_buffer.as_ref() else { return };
        let row_pitch = align_to_256(OCCLUDER_DEPTH_RESOLUTION as u64 * 4) as usize;
        let total_size = row_pitch * OCCLUDER_DEPTH_RESOLUTION as usize;
        unsafe {
            let mut mapped = std::ptr::null_mut();
            let read_range = D3D12_RANGE { Begin: 0, End: total_size as u64 as usize };
            if readback_buffer.resource.Map(0, Some(&read_range), Some(&mut mapped)).is_err() {
                return;
            }
            if mapped.is_null() {
                readback_buffer.resource.Unmap(0, None);
                return;
            }
            let mut dense = vec![0.0f32; (OCCLUDER_DEPTH_RESOLUTION * OCCLUDER_DEPTH_RESOLUTION) as usize];
            let row_floats = OCCLUDER_DEPTH_RESOLUTION as usize;
            for y in 0..OCCLUDER_DEPTH_RESOLUTION as usize {
                let row_src = (mapped as *const u8).add(y * row_pitch) as *const f32;
                let row_slice = std::slice::from_raw_parts(row_src, row_floats);
                dense[y * row_floats..(y + 1) * row_floats].copy_from_slice(row_slice);
            }
            let written_range = D3D12_RANGE { Begin: 0, End: 0 };
            readback_buffer.resource.Unmap(0, Some(&written_range));

            let covered = dense.iter().filter(|&&d| d < 1.0).count();
            let total = dense.len();
            println!(
                "[ENGINE] [OCCLUSION] Occluder depth readback готов: {}/{} texel'ей с геометрией ({:.1}%)",
                covered, total, 100.0 * covered as f32 / total.max(1) as f32
            );

            self.occluder_depth_cpu = Some(dense);
        }
    }
}
