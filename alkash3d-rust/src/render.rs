// src/render.rs
use windows::core::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use crate::{STATE};
use crate::texture::Texture;

pub struct Renderer {
    pub back_buffers: Vec<Texture>,
    pub render_target_views: Vec<D3D12_CPU_DESCRIPTOR_HANDLE>,
    pub rtv_heap: ID3D12DescriptorHeap,
    pub dsv_heap: ID3D12DescriptorHeap,
    pub depth_stencil: Texture,
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
            let resource = unsafe { swap_chain.GetBuffer(i)? };
            let texture = Texture {
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
        let depth_stencil = crate::texture::Texture::create_depth_stencil(width, height)?;
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