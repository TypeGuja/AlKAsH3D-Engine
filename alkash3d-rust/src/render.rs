//! Функции рендеринга

use std::ffi::c_void;
use windows::Win32::{
    Foundation::RECT,
    Graphics::{Direct3D12::*, Dxgi::Common::*},
};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows_core::Interface;
use crate::utils::{ptr_to_device, ptr_to_resource};
use crate::command::with_command_list;
use crate::{STATE, debug_println};

#[inline]
fn to_usize(ptr: u64) -> usize {
    ptr as usize
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScissorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[no_mangle]
pub extern "C" fn set_graphics_pipeline(pso_ptr: *mut c_void) -> bool {
    unsafe {
        if pso_ptr.is_null() {
            return false;
        }

        with_command_list(|list| {
            let pso: ID3D12PipelineState = std::mem::transmute_copy(&pso_ptr);
            list.SetPipelineState(&pso);

            if let Ok(mut state) = STATE.lock() {
                state.current_pso = Some(pso.clone());
            }

            std::mem::forget(pso);
        }).is_some()
    }
}

#[no_mangle]
pub extern "C" fn set_root_signature(root_sig_ptr: *mut c_void) -> bool {
    unsafe {
        if root_sig_ptr.is_null() {
            return false;
        }

        with_command_list(|list| {
            let root_sig: ID3D12RootSignature = std::mem::transmute_copy(&root_sig_ptr);
            list.SetGraphicsRootSignature(&root_sig);
            std::mem::forget(root_sig);
        }).is_some()
    }
}

#[no_mangle]
pub extern "C" fn set_viewport(x: f32, y: f32, width: f32, height: f32, min_depth: f32, max_depth: f32) -> bool {
    with_command_list(|list| {
        let viewport = D3D12_VIEWPORT {
            TopLeftX: x,
            TopLeftY: y,
            Width: width,
            Height: height,
            MinDepth: min_depth,
            MaxDepth: max_depth,
        };
        unsafe {
            list.RSSetViewports(&[viewport]);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_scissor_rect(left: i32, top: i32, right: i32, bottom: i32) -> bool {
    with_command_list(|list| {
        let rect = RECT { left, top, right, bottom };
        unsafe {
            list.RSSetScissorRects(&[rect]);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_vertex_buffer(gpu_address: u64, size: u32, stride: u32) -> bool {
    with_command_list(|list| {
        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            StrideInBytes: stride,
        };
        unsafe {
            list.IASetVertexBuffers(0, Some(&[view]));
        }

        if let Ok(mut state) = STATE.lock() {
            state.bound_vertex_buffers.clear();
            state.bound_vertex_buffers.push(gpu_address);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_index_buffer(gpu_address: u64, size: u32, format: u32) -> bool {
    with_command_list(|list| {
        let view = D3D12_INDEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            Format: if format == 4 { DXGI_FORMAT_R32_UINT } else { DXGI_FORMAT_R16_UINT },
        };
        unsafe {
            list.IASetIndexBuffer(Some(&view));
        }

        if let Ok(mut state) = STATE.lock() {
            state.bound_index_buffer = Some(gpu_address);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_primitive_topology(topology: u32) -> bool {
    use windows::Win32::Graphics::Direct3D::*;

    with_command_list(|list| {
        let topo = match topology {
            1 => D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
            2 => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
            3 => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
            4 => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            5 => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            _ => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        };
        unsafe {
            list.IASetPrimitiveTopology(topo);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn draw_indexed(index_count: u32, start_index: u32, base_vertex: i32) -> bool {
    with_command_list(|list| {
        unsafe {
            list.DrawIndexedInstanced(index_count, 1, start_index, base_vertex, 0);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn draw_indexed_instanced(
    index_count: u32,
    instance_count: u32,
    start_index: u32,
    base_vertex: i32,
    start_instance: u32
) -> bool {
    with_command_list(|list| {
        unsafe {
            list.DrawIndexedInstanced(index_count, instance_count, start_index, base_vertex, start_instance);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn draw_instanced(
    vertex_count: u32,
    instance_count: u32,
    start_vertex: u32,
    start_instance: u32
) -> bool {
    with_command_list(|list| {
        unsafe {
            list.DrawInstanced(vertex_count, instance_count, start_vertex, start_instance);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn clear_render_target(rtv_cpu_handle: u64, color: *const f32) -> bool {
    if color.is_null() {
        debug_println!("[clear_render_target] color is null");
        return false;
    }

    debug_println!("[clear_render_target] handle=0x{:X}", rtv_cpu_handle);

    unsafe {
        with_command_list(|list: &ID3D12GraphicsCommandList| {
            let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
            let clear_color = [*color, *color.add(1), *color.add(2), *color.add(3)];
            list.ClearRenderTargetView(rtv, &clear_color, Some(&[]));  // &[] вместо Some(&[])
            debug_println!("[clear_render_target] OK");
        }).is_some()
    }
}

#[no_mangle]
pub extern "C" fn set_render_targets(rtv_cpu_handle: u64, num_rtvs: u32) -> bool {
    debug_println!("[set_render_targets] handle=0x{:X}, count={}", rtv_cpu_handle, num_rtvs);

    with_command_list(|list: &ID3D12GraphicsCommandList| {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        unsafe {
            list.OMSetRenderTargets(num_rtvs, Some(&rtv_handle), false, None);
        }
        debug_println!("[set_render_targets] OK");
    }).is_some()
}

#[no_mangle]
pub extern "C" fn clear_depth_stencil(dsv_cpu_handle: u64, depth: f32, stencil: u8) -> bool {
    with_command_list(|list| {
        let dsv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(dsv_cpu_handle) };
        unsafe {
            list.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, depth, stencil, &[]);
        }
    }).is_some()
}
#[no_mangle]
pub extern "C" fn set_render_targets_with_depth(
    rtv_cpu_handle: u64,
    dsv_cpu_handle: u64,
    num_rtvs: u32
) -> bool {
    with_command_list(|list| {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        let dsv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(dsv_cpu_handle) };
        unsafe {
            list.OMSetRenderTargets(num_rtvs, Some(&rtv_handle), false, Some(&dsv_handle));
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn get_buffer_gpu_address(buffer_ptr: *mut c_void) -> u64 {
    unsafe {
        if buffer_ptr.is_null() {
            return 0;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let address = buffer.GetGPUVirtualAddress();
        std::mem::forget(buffer);
        address
    }
}

#[no_mangle]
pub extern "C" fn set_root_constant_buffer_view(root_index: u32, gpu_address: u64) -> bool {
    with_command_list(|list| {
        unsafe {
            list.SetGraphicsRootConstantBufferView(root_index, gpu_address);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_root_descriptor_table(root_index: u32, gpu_handle: u64) -> bool {
    with_command_list(|list| {
        unsafe {
            list.SetGraphicsRootDescriptorTable(root_index, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: gpu_handle });
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_root_32bit_constants(
    root_index: u32,
    num_constants: u32,
    data: *const u32,
    dest_offset: u32
) -> bool {
    unsafe {
        if data.is_null() {
            return false;
        }

        with_command_list(|list| {
            // Преобразуем *const u32 в *const c_void
            list.SetGraphicsRoot32BitConstants(
                root_index,
                num_constants,
                data as *const std::ffi::c_void,
                dest_offset
            );
        }).is_some()
    }
}

#[no_mangle]
pub extern "C" fn create_render_target_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: u64
) -> bool {
    unsafe {
        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let resource = match ptr_to_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(cpu_handle) };
        device.CreateRenderTargetView(&resource, None, cpu_desc);

        std::mem::forget(resource);
        std::mem::forget(device);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_depth_stencil_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: u64
) -> bool {
    unsafe {
        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let resource = match ptr_to_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(cpu_handle) };

        let dsv_desc = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 },
            },
        };

        device.CreateDepthStencilView(&resource, Some(&dsv_desc), cpu_desc);

        std::mem::forget(resource);
        std::mem::forget(device);
        true
    }
}

#[no_mangle]
pub extern "C" fn transition_resource(
    resource_ptr: *mut c_void,
    state_before: u32,
    state_after: u32,
) -> bool {
    unsafe {
        if resource_ptr.is_null() {
            debug_println!("[transition_resource] resource_ptr is null");
            return false;
        }

        with_command_list(|list: &ID3D12GraphicsCommandList| {
            let resource: ID3D12Resource = match std::mem::transmute_copy(&resource_ptr) {
                r => r,
            };

            let barrier = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATES(state_before as i32),
                        StateAfter: D3D12_RESOURCE_STATES(state_after as i32),
                    }),
                },
            };

            list.ResourceBarrier(&[barrier]);
        }).is_some()
    }
}

#[no_mangle]
pub extern "C" fn set_render_target(rtv_cpu_handle: u64) -> bool {
    set_render_targets(rtv_cpu_handle, 1)
}