// src/render.rs
//! Функции рендеринга - ИСПРАВЛЕННАЯ ВЕРСИЯ

use std::ffi::c_void;
use std::io::Write;
use windows::Win32::{
    Foundation::RECT,
    Graphics::{Direct3D12::*, Dxgi::Common::*},
};
use windows_core::Interface;
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

// ============================================================================
// УСТАНОВКА PSO И ROOT SIGNATURE
// ============================================================================

#[no_mangle]
pub extern "C" fn set_graphics_pipeline(pso_ptr: *mut c_void) -> bool {
    unsafe {
        println!("[set_graphics_pipeline] START, pso_ptr={:p}", pso_ptr);
        std::io::stdout().flush().unwrap();

        if pso_ptr.is_null() {
            println!("[set_graphics_pipeline] pso_ptr is null");
            return false;
        }

        let result = with_command_list(|list| {
            println!("[set_graphics_pipeline] inside with_command_list");
            let pso = &*(pso_ptr as *const ID3D12PipelineState);
            println!("[set_graphics_pipeline] calling SetPipelineState");
            list.SetPipelineState(pso);
            println!("[set_graphics_pipeline] SetPipelineState done");
            true
        });

        println!("[set_graphics_pipeline] result = {:?}", result);
        let final_result = result.unwrap_or(false);
        println!("[set_graphics_pipeline] final_result = {}", final_result);
        std::io::stdout().flush().unwrap();
        final_result
    }
}

#[no_mangle]
pub extern "C" fn set_pipeline(pso_ptr: *mut c_void) -> bool {
    set_graphics_pipeline(pso_ptr)
}

#[no_mangle]
pub extern "C" fn set_pipeline_state(pso_ptr: *mut c_void) -> bool {
    set_graphics_pipeline(pso_ptr)
}

#[no_mangle]
pub extern "C" fn set_root_signature(root_sig_ptr: *mut c_void) -> bool {
    unsafe {
        println!("[set_root_signature] START, ptr={:p}", root_sig_ptr);  // ← ДОБАВИТЬ

        if root_sig_ptr.is_null() {
            debug_println!("[set_root_signature] root_sig_ptr is null");
            return false;
        }

        let result = with_command_list(|list| {
            let root_sig = &*(root_sig_ptr as *const ID3D12RootSignature);
            list.SetGraphicsRootSignature(root_sig);
            debug_println!("[set_root_signature] ✅ Root signature set");
            true
        });

        println!("[set_root_signature] Result: {:?}", result);  // ← ДОБАВИТЬ
        result.unwrap_or(false)
    }
}
#[no_mangle]
pub extern "C" fn set_root_signature_state(root_sig_ptr: *mut c_void) -> bool {
    set_root_signature(root_sig_ptr)
}

// ============================================================================
// VIEWPORT И SCISSOR
// ============================================================================

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
        debug_println!("[set_viewport] ✅ Viewport set: {}x{}", width, height);
        true
    }).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn set_scissor_rect(left: i32, top: i32, right: i32, bottom: i32) -> bool {
    with_command_list(|list| {
        let rect = RECT { left, top, right, bottom };
        unsafe {
            list.RSSetScissorRects(&[rect]);
        }
        debug_println!("[set_scissor_rect] ✅ Scissor rect set: ({},{})-({},{})", left, top, right, bottom);
        true
    }).unwrap_or(false)
}

// ============================================================================
// VERTEX И INDEX BUFFERS
// ============================================================================

#[no_mangle]
pub extern "C" fn set_vertex_buffer(gpu_address: u64, size: u32, stride: u32) -> bool {
    debug_println!("[set_vertex_buffer] START: addr=0x{:X}, size={}, stride={}", gpu_address, size, stride);

    let result = with_command_list(|list| {
        debug_println!("[set_vertex_buffer] inside with_command_list");
        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            StrideInBytes: stride,
        };
        unsafe {
            list.IASetVertexBuffers(0, Some(&[view]));
        }
        debug_println!("[set_vertex_buffer] ✅ Vertex buffer set");
        true
    });

    debug_println!("[set_vertex_buffer] result = {:?}", result);
    result.unwrap_or(false)
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
        debug_println!("[set_index_buffer] ✅ Index buffer set: addr={:X}, size={}", gpu_address, size);
        true
    }).unwrap_or(false)
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
        debug_println!("[set_primitive_topology] ✅ Topology set: {}", topology);
        true
    }).unwrap_or(false)
}

// ============================================================================
// DRAW CALLS
// ============================================================================

#[no_mangle]
pub extern "C" fn draw_indexed(index_count: u32, start_index: u32, base_vertex: i32) -> bool {
    with_command_list(|list| {
        unsafe {
            list.DrawIndexedInstanced(index_count, 1, start_index, base_vertex, 0);
        }
        debug_println!("[draw_indexed] ✅ Draw {} indices", index_count);
        true
    }).unwrap_or(false)
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
        debug_println!("[draw_indexed_instanced] ✅ Draw {} indices, {} instances", index_count, instance_count);
        true
    }).unwrap_or(false)
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
        debug_println!("[draw_instanced] ✅ Draw {} vertices, {} instances", vertex_count, instance_count);
        true
    }).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn draw(start_vertex: u32, vertex_count: u32) -> bool {
    draw_instanced(vertex_count, 1, start_vertex, 0)
}

// ============================================================================
// RENDER TARGETS
// ============================================================================

#[no_mangle]
pub extern "C" fn set_render_targets(rtv_cpu_handle: u64, num_rtvs: u32) -> bool {
    debug_println!("[set_render_targets] handle=0x{:X}, count={}", rtv_cpu_handle, num_rtvs);

    with_command_list(|list: &ID3D12GraphicsCommandList| {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        unsafe {
            list.OMSetRenderTargets(num_rtvs, Some(&rtv_handle), false, None);
        }
        debug_println!("[set_render_targets] ✅ Render targets set");
        true
    }).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn set_render_target(rtv_cpu_handle: u64) -> bool {
    set_render_targets(rtv_cpu_handle, 1)
}

#[no_mangle]
pub extern "C" fn set_render_targets_with_depth(
    rtv_cpu_handle: u64,
    dsv_cpu_handle: u64,
    num_rtvs: u32
) -> bool {
    debug_println!("[set_render_targets_with_depth] rtv=0x{:X}, dsv=0x{:X}, count={}",
                   rtv_cpu_handle, dsv_cpu_handle, num_rtvs);

    with_command_list(|list| {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        let dsv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(dsv_cpu_handle) };
        unsafe {
            list.OMSetRenderTargets(num_rtvs, Some(&rtv_handle), false, Some(&dsv_handle));
        }
        debug_println!("[set_render_targets_with_depth] ✅ Render targets with depth set");
        true
    }).unwrap_or(false)
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
            list.ClearRenderTargetView(rtv, &clear_color, Some(&[]));
            debug_println!("[clear_render_target] ✅ Clear color: ({:.2},{:.2},{:.2},{:.2})",
                          clear_color[0], clear_color[1], clear_color[2], clear_color[3]);
            true
        }).unwrap_or(false)
    }
}

#[no_mangle]
pub extern "C" fn clear_depth_stencil(dsv_cpu_handle: u64, depth: f32, stencil: u8) -> bool {
    debug_println!("[clear_depth_stencil] handle=0x{:X}, depth={}, stencil={}", dsv_cpu_handle, depth, stencil);

    with_command_list(|list| {
        let dsv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(dsv_cpu_handle) };
        unsafe {
            list.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, depth, stencil, Some(&[]));
        }
        debug_println!("[clear_depth_stencil] ✅ Depth stencil cleared");
        true
    }).unwrap_or(false)
}

// ============================================================================
// CREATE VIEWS
// ============================================================================

#[no_mangle]
pub extern "C" fn create_render_target_view(
    _device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: u64,
) -> bool {
    unsafe {
        debug_println!("[create_render_target_view] START");

        if resource_ptr.is_null() {
            debug_println!("[create_render_target_view] resource_ptr is null");
            return false;
        }

        if cpu_handle == 0 {
            debug_println!("[create_render_target_view] cpu_handle is 0");
            return false;
        }

        // Получаем device из STATE
        let device_ptr_from_state = {
            let state = match STATE.lock() {
                Ok(s) => s,
                Err(e) => {
                    debug_println!("[create_render_target_view] Failed to lock STATE: {:?}", e);
                    return false;
                }
            };
            match state.device.as_ref() {
                Some(d) => d.as_raw(),
                None => {
                    debug_println!("[create_render_target_view] No device in STATE");
                    return false;
                }
            }
        };

        if device_ptr_from_state.is_null() {
            debug_println!("[create_render_target_view] device_ptr_from_state is null");
            return false;
        }

        // Проверяем, что resource_ptr указывает на валидный объект
        // Пробуем получить IUnknown для проверки
        let resource_test = ID3D12Resource::from_raw(resource_ptr as *mut _);
        if resource_test.as_raw().is_null() {
            debug_println!("[create_render_target_view] resource raw pointer is null!");
            return false;
        }

        debug_println!("[create_render_target_view] device raw: {:p}, resource raw: {:p}, cpu_handle: 0x{:X}",
                      device_ptr_from_state, resource_ptr, cpu_handle);

        let device = ID3D12Device::from_raw(device_ptr_from_state as *mut _);
        let resource = ID3D12Resource::from_raw(resource_ptr as *mut _);
        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle as usize };

        // Создаём Render Target View
        device.CreateRenderTargetView(&resource, None, cpu_desc);

        debug_println!("[create_render_target_view] ✅ RTV created");
        true
    }
}

#[no_mangle]
pub extern "C" fn create_depth_stencil_view(
    device_ptr: *mut c_void,
    resource_ptr: *mut c_void,
    cpu_handle: u64,
) -> bool {
    unsafe {
        if device_ptr.is_null() || resource_ptr.is_null() || cpu_handle == 0 {
            debug_println!("[create_depth_stencil_view] Invalid parameters");
            return false;
        }

        let device = ID3D12Device::from_raw(device_ptr as *mut _);
        let resource = ID3D12Resource::from_raw(resource_ptr as *mut _);
        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle as usize };

        let dsv_desc = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 },
            },
        };

        device.CreateDepthStencilView(&resource, Some(&dsv_desc), cpu_desc);
        debug_println!("[create_depth_stencil_view] ✅ DSV created");
        true
    }
}

// ============================================================================
// RESOURCE MANAGEMENT - ИСПРАВЛЕННАЯ ФУНКЦИЯ
// ============================================================================


#[no_mangle]
pub extern "C" fn transition_resource(
    resource_ptr: *mut c_void,
    state_before: u32,
    state_after: u32,
) -> bool {
    unsafe {
        debug_println!("[transition_resource] START, ptr={:p}, before={}, after={}",
                       resource_ptr, state_before, state_after);

        if resource_ptr.is_null() {
            debug_println!("[transition_resource] resource_ptr is null");
            return false;
        }

        // Получаем command list напрямую из STATE
        let state = match STATE.lock() {
            Ok(s) => s,
            Err(e) => {
                debug_println!("[transition_resource] Failed to lock STATE: {:?}", e);
                return false;
            }
        };

        if !state.command_list_open {
            debug_println!("[transition_resource] command list not open");
            return false;
        }

        let list = match state.command_list.as_ref() {
            Some(l) => l,
            None => {
                debug_println!("[transition_resource] command list is None");
                return false;
            }
        };

        let resource = &*(resource_ptr as *const ID3D12Resource);

        // Создаём барьер напрямую, используя сырой указатель
        let transition = D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            StateBefore: D3D12_RESOURCE_STATES(state_before as i32),
            StateAfter: D3D12_RESOURCE_STATES(state_after as i32),
        };

        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(transition),
            },
        };

        list.ResourceBarrier(&[barrier]);
        debug_println!("[transition_resource] ✅ Transition done");
        true
    }
}

#[no_mangle]
pub extern "C" fn copy_buffer(
    cmd_list_ptr: *mut c_void,
    src_buffer_ptr: *mut c_void,
    dst_buffer_ptr: *mut c_void,
    size: u64,
) -> bool {
    unsafe {
        println!("[copy_buffer] START: src={:p}, dst={:p}, size={}", src_buffer_ptr, dst_buffer_ptr, size);

        if cmd_list_ptr.is_null() || src_buffer_ptr.is_null() || dst_buffer_ptr.is_null() {
            println!("[copy_buffer] Null pointer detected!");
            return false;
        }

        let cmd_list = &*(cmd_list_ptr as *const ID3D12GraphicsCommandList);
        let src_buffer = &*(src_buffer_ptr as *const ID3D12Resource);
        let dst_buffer = &*(dst_buffer_ptr as *const ID3D12Resource);

        // Переводим dst в COPY_DEST состояние
        let transition_dst = D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: std::mem::ManuallyDrop::new(Some(dst_buffer.clone())),
            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            StateBefore: D3D12_RESOURCE_STATE_COMMON,
            StateAfter: D3D12_RESOURCE_STATE_COPY_DEST,
        };

        let barrier_dst = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(transition_dst),
            },
        };

        cmd_list.ResourceBarrier(&[barrier_dst]);

        // Копируем данные
        cmd_list.CopyBufferRegion(dst_buffer, 0, src_buffer, 0, size);

        // Переводим dst обратно в COMMON
        let transition_back = D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: std::mem::ManuallyDrop::new(Some(dst_buffer.clone())),
            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
            StateAfter: D3D12_RESOURCE_STATE_COMMON,
        };

        let barrier_back = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(transition_back),
            },
        };

        cmd_list.ResourceBarrier(&[barrier_back]);

        println!("[copy_buffer] ✅ Copy completed!");
        true
    }
}

// ============================================================================
// ROOT SIGNATURE PARAMETERS
// ============================================================================

#[no_mangle]
pub extern "C" fn set_root_constant_buffer_view(root_index: u32, gpu_address: u64) -> bool {
    with_command_list(|list| {
        unsafe {
            list.SetGraphicsRootConstantBufferView(root_index, gpu_address);
        }
        debug_println!("[set_root_constant_buffer_view] ✅ Root {} CBV set to 0x{:X}", root_index, gpu_address);
        true
    }).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn set_root_descriptor_table(root_index: u32, gpu_handle: u64) -> bool {
    with_command_list(|list| {
        unsafe {
            list.SetGraphicsRootDescriptorTable(root_index, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: gpu_handle });
        }
        debug_println!("[set_root_descriptor_table] ✅ Root {} descriptor table set to 0x{:X}", root_index, gpu_handle);
        true
    }).unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn set_root_32bit_constants(
    root_index: u32,
    num_constants: u32,
    data: *const u32,
    dest_offset: u32,
) -> bool {
    unsafe {
        if data.is_null() {
            debug_println!("[set_root_32bit_constants] data is null");
            return false;
        }

        with_command_list(|list| {
            list.SetGraphicsRoot32BitConstants(
                root_index,
                num_constants,
                data as *const std::ffi::c_void,
                dest_offset
            );
            debug_println!("[set_root_32bit_constants] ✅ Root {} constants set, count={}, offset={}",
                          root_index, num_constants, dest_offset);
            true
        }).unwrap_or(false)
    }
}

// ============================================================================
// RESOURCE CLEANUP
// ============================================================================

#[no_mangle]
pub extern "C" fn destroy_render_target_view(_device_ptr: *mut c_void, cpu_handle: u64) -> bool {
    debug_println!("[destroy_render_target_view] RTV at handle 0x{:X} will be freed with heap", cpu_handle);
    true
}

#[no_mangle]
pub extern "C" fn destroy_depth_stencil_view(_device_ptr: *mut c_void, cpu_handle: u64) -> bool {
    debug_println!("[destroy_depth_stencil_view] DSV at handle 0x{:X} will be freed with heap", cpu_handle);
    true
}