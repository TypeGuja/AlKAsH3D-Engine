// src/command.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;
use windows::Win32::Foundation::RECT;
use crate::STATE;

pub struct CommandList;

impl CommandList {
    pub fn create_allocators(count: u32) -> Result<()> {
        println!("[COMMAND] ========== CREATING ALLOCATORS ==========");
        println!("[COMMAND] Requested allocator count: {}", count);

        let device = {
            let state = STATE.lock().unwrap();
            match &state.device {
                Some(d) => {
                    println!("[COMMAND] Device obtained");
                    d.clone()
                },
                None => {
                    eprintln!("[COMMAND] ERROR: Device is None!");
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        let mut state = STATE.lock().unwrap();
        state.command_allocators.clear();

        for i in 0..count {
            unsafe {
                println!("[COMMAND] Creating allocator {}...", i);
                let allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
                state.command_allocators.push(Some(allocator));
                println!("[COMMAND] Allocator {} created", i);
            }
        }
        println!("[COMMAND] ✓ {} allocators created", count);
        Ok(())
    }

    pub fn create_command_list() -> Result<()> {
        println!("[COMMAND] ========== CREATING COMMAND LIST ==========");

        let device = {
            let state = STATE.lock().unwrap();
            match &state.device {
                Some(d) => {
                    println!("[COMMAND] Device obtained");
                    d.clone()
                },
                None => {
                    eprintln!("[COMMAND] ERROR: Device is None!");
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        let allocator = {
            let state = STATE.lock().unwrap();
            match state.command_allocators.get(0) {
                Some(Some(a)) => {
                    println!("[COMMAND] Using allocator 0");
                    a.clone()
                },
                _ => {
                    eprintln!("[COMMAND] ERROR: No allocator found!");
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        unsafe {
            println!("[COMMAND] Creating command list...");
            let cmd_list = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            println!("[COMMAND] Command list created");

            let mut state = STATE.lock().unwrap();
            state.command_list = Some(cmd_list);
            println!("[COMMAND] ✓ Command list stored in global state");
        }

        Ok(())
    }

    pub fn reset_command_list() -> Result<()> {
        let frame_index = {
            let state = STATE.lock().unwrap();
            let idx = state.frame_index as usize;
            println!("[COMMAND] Current frame index: {}", idx);
            idx
        };

        println!("[COMMAND] Resetting command list for frame {}", frame_index);

        let allocator = {
            let state = STATE.lock().unwrap();
            match state.command_allocators.get(frame_index) {
                Some(Some(a)) => {
                    println!("[COMMAND] Got allocator {}: {:p}", frame_index, a);
                    a.clone()
                },
                _ => {
                    eprintln!("[COMMAND] ERROR: Allocator {} not found!", frame_index);
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        unsafe {
            println!("[COMMAND] Resetting allocator...");
            allocator.Reset();
            println!("[COMMAND] Allocator reset complete");

            let mut state = STATE.lock().unwrap();
            if let Some(cmd_list) = &state.command_list {
                println!("[COMMAND] Resetting command list...");
                cmd_list.Reset(&allocator, None)?;
                state.command_list_open = true;
                println!("[COMMAND] ✓ Command list reset successfully, open={}", state.command_list_open);
            } else {
                eprintln!("[COMMAND] ERROR: Command list is None!");
                return Err(Error::from_hresult(HRESULT(1)));
            }
        }

        Ok(())
    }

    pub fn close_command_list() -> Result<()> {
        println!("[COMMAND] Closing command list...");
        let mut state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe {
                cmd_list.Close()?;
                println!("[COMMAND] Command list closed successfully");
            }
            state.command_list_open = false;
        } else {
            eprintln!("[COMMAND] WARNING: Command list is None, nothing to close");
        }
        Ok(())
    }

    pub fn set_pipeline_state(pso: &ID3D12PipelineState) {
        println!("[COMMAND] Setting pipeline state: {:p}", pso);
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe { cmd_list.SetPipelineState(pso) };
            println!("[COMMAND] ✓ Pipeline state set");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn set_graphics_root_signature(signature: &ID3D12RootSignature) {
        println!("[COMMAND] Setting graphics root signature: {:p}", signature);
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe { cmd_list.SetGraphicsRootSignature(signature) };
            println!("[COMMAND] ✓ Root signature set");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn set_viewport(viewport: D3D12_VIEWPORT) {
        println!("[COMMAND] Setting viewport: x={}, y={}, w={}, h={}, minZ={}, maxZ={}",
                 viewport.TopLeftX, viewport.TopLeftY, viewport.Width, viewport.Height,
                 viewport.MinDepth, viewport.MaxDepth);
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe { cmd_list.RSSetViewports(&[viewport]) };
            println!("[COMMAND] ✓ Viewport set");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn set_scissor_rect(rect: RECT) {
        println!("[COMMAND] Setting scissor rect: l={}, t={}, r={}, b={}",
                 rect.left, rect.top, rect.right, rect.bottom);
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe { cmd_list.RSSetScissorRects(&[rect]) };
            println!("[COMMAND] ✓ Scissor rect set");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn set_vertex_buffer(buffer: &crate::Buffer, slot: u32) {
        println!("[COMMAND] Setting vertex buffer: slot={}, size={}, stride={}, addr={:?}",
                 slot, buffer.size, buffer.vertex_stride, unsafe { buffer.resource.GetGPUVirtualAddress() });
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            let view = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: unsafe { buffer.resource.GetGPUVirtualAddress() },
                SizeInBytes: buffer.size as u32,
                StrideInBytes: buffer.vertex_stride,
            };
            unsafe { cmd_list.IASetVertexBuffers(slot, Some(&[view])) };
            println!("[COMMAND] ✓ Vertex buffer set");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn set_index_buffer(buffer: &crate::Buffer) {
        println!("[COMMAND] Setting index buffer: size={}, addr={:?}",
                 buffer.size, unsafe { buffer.resource.GetGPUVirtualAddress() });
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            let view = D3D12_INDEX_BUFFER_VIEW {
                BufferLocation: unsafe { buffer.resource.GetGPUVirtualAddress() },
                SizeInBytes: buffer.size as u32,
                Format: DXGI_FORMAT_R32_UINT,
            };
            unsafe { cmd_list.IASetIndexBuffer(Some(&view)) };
            println!("[COMMAND] ✓ Index buffer set");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn draw_indexed(index_count: u32, start_index: u32, base_vertex: i32) {
        println!("[COMMAND] DrawIndexedInstanced: indexCount={}, startIndex={}, baseVertex={}",
                 index_count, start_index, base_vertex);
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe { cmd_list.DrawIndexedInstanced(index_count, 1, start_index, base_vertex, 0) };
            println!("[COMMAND] ✓ DrawIndexedInstanced called");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }

    pub fn draw(vertex_count: u32, start_vertex: u32) {
        println!("[COMMAND] DrawInstanced: vertexCount={}, startVertex={}", vertex_count, start_vertex);
        let state = STATE.lock().unwrap();
        if let Some(cmd_list) = &state.command_list {
            unsafe { cmd_list.DrawInstanced(vertex_count, 1, start_vertex, 0) };
            println!("[COMMAND] ✓ DrawInstanced called");
        } else {
            eprintln!("[COMMAND] ERROR: Command list is None!");
        }
    }
}