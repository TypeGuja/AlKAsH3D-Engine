// src/swap_chain.rs
use windows::core::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_UNSPECIFIED, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use crate::STATE;

pub struct SwapChain;

impl SwapChain {
    pub fn create(hwnd: isize, width: u32, height: u32, buffer_count: u32) -> Result<()> {
        println!("[SWAPCHAIN] ========== CREATING SWAP CHAIN ==========");
        println!("[SWAPCHAIN] HWND: 0x{:X}, Width: {}, Height: {}, Buffer count: {}", hwnd, width, height, buffer_count);

        let queue = {
            let state = STATE.lock().unwrap();
            match &state.command_queue {
                Some(q) => {
                    println!("[SWAPCHAIN] Command queue obtained");
                    q.clone()
                },
                None => {
                    eprintln!("[SWAPCHAIN] ERROR: Command queue is None!");
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        unsafe {
            println!("[SWAPCHAIN] Creating DXGI factory...");
            let dxgi_factory = CreateDXGIFactory1::<IDXGIFactory4>()?;
            println!("[SWAPCHAIN] DXGI factory created");

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: buffer_count,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_UNSPECIFIED,
                Flags: 0,
            };
            println!("[SWAPCHAIN] Swap chain description created");

            println!("[SWAPCHAIN] Creating swap chain...");
            let swap_chain = dxgi_factory.CreateSwapChainForHwnd(&queue, HWND(hwnd as _), &desc, None, None)?;
            println!("[SWAPCHAIN] ✓ Swap chain created");

            let mut state = STATE.lock().unwrap();
            state.swap_chain = Some(swap_chain.cast::<IDXGISwapChain3>()?);
            state.frame_index = state.swap_chain.as_ref().unwrap().GetCurrentBackBufferIndex();
            println!("[SWAPCHAIN] ✓ Swap chain stored, initial frame index: {}", state.frame_index);
        }

        Ok(())
    }

    pub fn present(&self, sync_interval: u32, flags: DXGI_PRESENT) -> windows::core::Result<()> {
        let state = STATE.lock().unwrap();
        if let Some(swap_chain) = &state.swap_chain {
            println!("[SWAPCHAIN] Present: sync_interval={}", sync_interval);
            unsafe { swap_chain.Present(sync_interval, flags) };
            println!("[SWAPCHAIN] Present completed");
        } else {
            eprintln!("[SWAPCHAIN] WARNING: Swap chain is None!");
        }
        Ok(())
    }

    pub unsafe fn resize(&self, width: u32, height: u32) -> Result<()> {
        println!("[SWAPCHAIN] Resizing: {}x{}", width, height);
        let mut state = STATE.lock().unwrap();
        if let Some(swap_chain) = &state.swap_chain {
            swap_chain.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
            state.frame_index = swap_chain.GetCurrentBackBufferIndex();
            println!("[SWAPCHAIN] Resize completed, new frame index: {}", state.frame_index);
        }
        Ok(())
    }

    pub fn get_back_buffer(&self, index: u32) -> Result<ID3D12Resource> {
        let state = STATE.lock().unwrap();
        if let Some(swap_chain) = &state.swap_chain {
            println!("[SWAPCHAIN] Getting back buffer {}", index);
            let buffer = unsafe { swap_chain.GetBuffer(index)? };
            println!("[SWAPCHAIN] ✓ Back buffer {} obtained", index);
            return Ok(buffer);
        }
        eprintln!("[SWAPCHAIN] ERROR: Swap chain is None!");
        Err(Error::from_hresult(HRESULT(1)))
    }
}