// src/render.rs
use windows::core::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use crate::STATE;
use crate::buffer::Buffer;

// Простая структура текстуры для рендера
pub struct RenderTexture {
    pub resource: ID3D12Resource,
    pub width: u32,
    pub height: u32,
    pub format: DXGI_FORMAT,
    pub mip_levels: u32,
}

impl RenderTexture {
    pub fn create_depth_stencil(width: u32, height: u32) -> Result<Self> {
        println!("[RENDERER] Creating depth stencil: {}x{}", width, height);

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

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
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
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
                eprintln!("[RENDERER] ERROR: Depth stencil resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[RENDERER] ✓ Depth stencil created");

            Ok(Self {
                resource,
                width,
                height,
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
                mip_levels: 1,
            })
        }
    }
}

pub struct Renderer {
    pub back_buffers: Vec<RenderTexture>,
    pub render_target_views: Vec<D3D12_CPU_DESCRIPTOR_HANDLE>,
    pub rtv_heap: ID3D12DescriptorHeap,
    pub dsv_heap: ID3D12DescriptorHeap,
    pub depth_stencil: RenderTexture,
    pub depth_stencil_view: D3D12_CPU_DESCRIPTOR_HANDLE,
    pub rtv_size: u32,
    pub dsv_size: u32,
    pub width: u32,
    pub height: u32,
}

impl Renderer {
    pub fn new(width: u32, height: u32, buffer_count: u32) -> Result<Self> {
        println!("[RENDERER] ========== CREATING RENDERER ==========");
        println!("[RENDERER] Width: {}, Height: {}, Buffer count: {}", width, height, buffer_count);

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };
        println!("[RENDERER] Device obtained");

        let swap_chain = {
            let state = STATE.lock().unwrap();
            state.swap_chain.as_ref().unwrap().clone()
        };
        println!("[RENDERER] Swap chain obtained");

        let rtv_size = {
            let state = STATE.lock().unwrap();
            state.rtv_descriptor_size
        };
        let dsv_size = {
            let state = STATE.lock().unwrap();
            state.dsv_descriptor_size
        };
        println!("[RENDERER] RTV size: {}, DSV size: {}", rtv_size, dsv_size);

        println!("[RENDERER] Creating RTV heap...");
        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(buffer_count)?;
        println!("[RENDERER] Creating DSV heap...");
        let dsv_heap = crate::heap::DescriptorHeap::create_dsv_heap(1)?;

        let mut back_buffers = Vec::new();
        let mut render_target_views = Vec::new();

        for i in 0..buffer_count {
            println!("[RENDERER] Getting back buffer {}", i);
            let resource: ID3D12Resource = unsafe { swap_chain.GetBuffer(i)? };
            let texture = RenderTexture {
                resource,
                width,
                height,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                mip_levels: 1,
            };
            println!("[RENDERER] Back buffer {} resource obtained", i);

            let handle = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, i, rtv_size);
            unsafe {
                device.CreateRenderTargetView(&texture.resource, None, handle);
            }

            back_buffers.push(texture);
            render_target_views.push(handle);
            println!("[RENDERER] ✓ RTV {} created", i);
        }

        println!("[RENDERER] Creating depth stencil...");
        let depth_stencil = RenderTexture::create_depth_stencil(width, height)?;
        let depth_stencil_view = crate::heap::DescriptorHeap::get_cpu_handle(&dsv_heap, 0, dsv_size);
        unsafe {
            device.CreateDepthStencilView(&depth_stencil.resource, None, depth_stencil_view);
        }
        println!("[RENDERER] ✓ Depth stencil created");

        println!("[RENDERER] ✓ Renderer created successfully");
        Ok(Self {
            back_buffers,
            render_target_views,
            rtv_heap,
            dsv_heap,
            depth_stencil,
            depth_stencil_view,
            rtv_size,
            dsv_size,
            width,
            height,
        })
    }
}