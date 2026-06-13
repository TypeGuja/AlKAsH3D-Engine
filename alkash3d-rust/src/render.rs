// src/render.rs
use windows::core::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use crate::{STATE, CommandList};
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
    pub frame_index: u32,
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
            println!("[RENDERER] Creating RTV for back buffer {}", i);
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
            frame_index: 0,
        })
    }

    pub fn begin_frame(&mut self) -> Result<()> {
        {
            let mut state = STATE.lock().unwrap();
            self.frame_index = state.frame_index;
            println!("[RENDERER] Begin frame: frame_index={}", self.frame_index);
        }

        CommandList::reset_command_list()?;

        let cmd_list = {
            let state = STATE.lock().unwrap();
            state.command_list.as_ref().unwrap().clone()
        };
        println!("[RENDERER] Command list obtained");

        let rtv_handle = self.render_target_views[self.frame_index as usize];
        let dsv_handle = self.depth_stencil_view;
        println!("[RENDERER] RTV handle obtained, DSV handle obtained");

        unsafe {
            cmd_list.OMSetRenderTargets(1, Some(&rtv_handle), false, Some(&dsv_handle));
            println!("[RENDERER] Render targets set");
        }

        let viewport = D3D12_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.back_buffers[0].width as f32,
            Height: self.back_buffers[0].height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        CommandList::set_viewport(viewport);

        let scissor = RECT {
            left: 0,
            top: 0,
            right: self.back_buffers[0].width as i32,
            bottom: self.back_buffers[0].height as i32,
        };
        CommandList::set_scissor_rect(scissor);

        let clear_color = [0.1, 0.1, 0.2, 1.0];
        println!("[RENDERER] Clearing with color: [{}, {}, {}, {}]", clear_color[0], clear_color[1], clear_color[2], clear_color[3]);
        unsafe {
            cmd_list.ClearRenderTargetView(rtv_handle, &clear_color, None);
            cmd_list.ClearDepthStencilView(dsv_handle, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);
            println!("[RENDERER] Clear commands executed");
        }

        Ok(())
    }

    pub fn end_frame(&mut self) -> Result<()> {
        println!("[RENDERER] Ending frame...");
        CommandList::close_command_list()?;

        let cmd_list = {
            let state = STATE.lock().unwrap();
            state.command_list.as_ref().unwrap().clone()
        };
        println!("[RENDERER] Command list for execution obtained");

        let queue = {
            let state = STATE.lock().unwrap();
            state.command_queue.as_ref().unwrap().clone()
        };
        println!("[RENDERER] Queue obtained");

        let cmd_lists = [Some(cmd_list.into())];
        unsafe {
            queue.ExecuteCommandLists(&cmd_lists);
            println!("[RENDERER] Command list executed");
        }

        let swap_chain = {
            let state = STATE.lock().unwrap();
            state.swap_chain.as_ref().unwrap().clone()
        };
        println!("[RENDERER] Presenting...");
        unsafe {
            let _ = swap_chain.Present(1, DXGI_PRESENT(0));
            println!("[RENDERER] Present called");
        }

        Ok(())
    }
}