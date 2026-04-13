//! Функции рендеринга

use std::ffi::c_void;
use windows::Win32::{
    Foundation::RECT,
    Graphics::{Direct3D12::*, Dxgi::Common::*},
};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows_core::Interface;
use crate::{STATE, debug_println, utils::{ptr_to_device, ptr_to_resource, ptr_to_heap}};

#[inline]
fn to_usize(ptr: u64) -> usize {
    ptr as usize
}

#[no_mangle]
pub extern "C" fn set_graphics_pipeline(pso_ptr: *mut c_void) -> bool {
    unsafe {
        debug_println!("[set_graphics_pipeline] Setting PSO");

        if pso_ptr.is_null() {
            return false;
        }

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let pso: ID3D12PipelineState = std::mem::transmute_copy(&pso_ptr);
        list.SetPipelineState(&pso);
        std::mem::forget(pso);
        true
    }
}

#[no_mangle]
pub extern "C" fn set_root_signature(root_sig_ptr: *mut c_void) -> bool {
    unsafe {
        debug_println!("[set_root_signature] Setting root signature");

        if root_sig_ptr.is_null() {
            return false;
        }

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let root_sig: ID3D12RootSignature = std::mem::transmute_copy(&root_sig_ptr);
        list.SetGraphicsRootSignature(&root_sig);
        std::mem::forget(root_sig);
        true
    }
}

#[no_mangle]
pub extern "C" fn set_render_targets(count: usize, rtvs: *const u64, dsv_cpu_handle: u64) -> bool {
    unsafe {
        debug_println!("[set_render_targets] count={}", count);

        if rtvs.is_null() && count == 0 {
            return false;
        }

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let mut rtv_handles = Vec::with_capacity(count);
        for i in 0..count {
            let handle_ptr = rtvs.add(i);
            if !handle_ptr.is_null() {
                let handle_val = *handle_ptr;
                rtv_handles.push(D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(handle_val) });
            }
        }

        let dsv = if dsv_cpu_handle != 0 {
            Some(D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(dsv_cpu_handle) })
        } else {
            None
        };

        // Исправлено: передаём Option<&D3D12_CPU_DESCRIPTOR_HANDLE> как Option<*const>
        let dsv_ptr = dsv.as_ref().map(|h| h as *const D3D12_CPU_DESCRIPTOR_HANDLE);

        list.OMSetRenderTargets(
            rtv_handles.len() as u32,
            Some(rtv_handles.as_ptr()),
            false,
            dsv_ptr
        );
        true
    }
}

#[no_mangle]
pub extern "C" fn set_viewport(x: f32, y: f32, width: f32, height: f32, min_depth: f32, max_depth: f32) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let viewport = D3D12_VIEWPORT {
            TopLeftX: x,
            TopLeftY: y,
            Width: width,
            Height: height,
            MinDepth: min_depth,
            MaxDepth: max_depth,
        };
        list.RSSetViewports(&[viewport]);
        true
    }
}

#[no_mangle]
pub extern "C" fn set_scissor_rect(left: i32, top: i32, right: i32, bottom: i32) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let rect = RECT { left, top, right, bottom };
        // RSSetScissorRects ожидает &[RECT], передаём срез из одного элемента
        list.RSSetScissorRects(&[rect]);
        true
    }
}

#[no_mangle]
pub extern "C" fn set_vertex_buffer(gpu_address: u64, size: u32, stride: u32) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            StrideInBytes: stride,
        };

        list.IASetVertexBuffers(0, Some(&[view]));
        true
    }
}

#[no_mangle]
pub extern "C" fn set_index_buffer(gpu_address: u64, size: u32, format: u32) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let view = D3D12_INDEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            Format: if format == 4 { DXGI_FORMAT_R32_UINT } else { DXGI_FORMAT_R16_UINT },
        };

        list.IASetIndexBuffer(Some(&view));
        true
    }
}

#[no_mangle]
pub extern "C" fn draw_instanced(vertex_count: u32, instance_count: u32, start_vertex: u32, start_instance: u32) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.DrawInstanced(vertex_count, instance_count, start_vertex, start_instance);
        true
    }
}

#[no_mangle]
pub extern "C" fn draw_indexed_instanced(index_count: u32, instance_count: u32, start_index: u32, base_vertex: i32, start_instance: u32) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        list.DrawIndexedInstanced(index_count, instance_count, start_index, base_vertex, start_instance);
        true
    }
}

#[no_mangle]
pub extern "C" fn clear_render_target(rtv_cpu_handle: u64, color: *const f32) -> bool {
    unsafe {
        if color.is_null() {
            return false;
        }

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        let clear_color = [*color, *color.add(1), *color.add(2), *color.add(3)];
        list.ClearRenderTargetView(rtv, &clear_color, None);
        true
    }
}

#[no_mangle]
pub extern "C" fn clear_depth_stencil(dsv_cpu_handle: u64, flags: u32, depth: f32, stencil: u8) -> bool {
    unsafe {
        debug_println!("[clear_depth_stencil] flags={}, depth={}", flags, depth);

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let dsv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(dsv_cpu_handle) };
        let clear_flags = match flags {
            0 => D3D12_CLEAR_FLAG_DEPTH,
            1 => D3D12_CLEAR_FLAG_STENCIL,
            _ => D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL,
        };

        // Передаём пустой срез для очистки всей поверхности
        list.ClearDepthStencilView(dsv, clear_flags, depth, stencil, &[]);
        true
    }
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
pub extern "C" fn set_root_descriptor_table(root_index: u32, gpu_handle: u64) -> bool {
    unsafe {
        debug_println!("[set_root_descriptor_table] root_index={}, gpu_handle=0x{:X}", root_index, gpu_handle);

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let handle = D3D12_GPU_DESCRIPTOR_HANDLE { ptr: to_usize(gpu_handle) as u64 };
        list.SetGraphicsRootDescriptorTable(root_index, handle);
        true
    }
}

#[no_mangle]
pub extern "C" fn set_descriptor_heaps(count: usize, heaps: *const *mut c_void) -> bool {
    unsafe {
        debug_println!("[set_descriptor_heaps] count={}", count);

        if heaps.is_null() || count == 0 {
            return false;
        }

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let mut heap_options: Vec<Option<ID3D12DescriptorHeap>> = Vec::with_capacity(count);
        for i in 0..count {
            let heap_ptr = *heaps.add(i);
            if !heap_ptr.is_null() {
                if let Some(heap) = ptr_to_heap(heap_ptr) {
                    heap_options.push(Some(heap));
                } else {
                    heap_options.push(None);
                }
            } else {
                heap_options.push(None);
            }
        }

        if !heap_options.is_empty() {
            // SetDescriptorHeaps ожидает &[Option<ID3D12DescriptorHeap>]
            list.SetDescriptorHeaps(&heap_options);
        }
        true
    }
}

#[no_mangle]
pub extern "C" fn set_root_constant_buffer_view(root_index: u32, gpu_address: u64) -> bool {
    unsafe {
        debug_println!("[set_root_constant_buffer_view] root_index={}, address=0x{:X}", root_index, gpu_address);

        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        list.SetGraphicsRootConstantBufferView(root_index, gpu_address);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_vertex_buffer_view(buffer_ptr: *mut c_void, stride: u32, size: u32, out_view: *mut c_void) -> bool {
    unsafe {
        debug_println!("[create_vertex_buffer_view] stride={}, size={}", stride, size);

        if buffer_ptr.is_null() || out_view.is_null() {
            return false;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let gpu_address = buffer.GetGPUVirtualAddress();
        std::mem::forget(buffer);

        let view = D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            StrideInBytes: stride,
        };

        std::ptr::write(out_view as *mut D3D12_VERTEX_BUFFER_VIEW, view);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_index_buffer_view(buffer_ptr: *mut c_void, size: u32, format: u32, out_view: *mut c_void) -> bool {
    unsafe {
        debug_println!("[create_index_buffer_view] size={}, format={}", size, format);

        if buffer_ptr.is_null() || out_view.is_null() {
            return false;
        }

        let buffer: ID3D12Resource = std::mem::transmute_copy(&buffer_ptr);
        let gpu_address = buffer.GetGPUVirtualAddress();
        std::mem::forget(buffer);

        let view = D3D12_INDEX_BUFFER_VIEW {
            BufferLocation: gpu_address,
            SizeInBytes: size,
            Format: if format == 4 { DXGI_FORMAT_R32_UINT } else { DXGI_FORMAT_R16_UINT },
        };

        std::ptr::write(out_view as *mut D3D12_INDEX_BUFFER_VIEW, view);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_shader_resource_view(device_ptr: *mut c_void, resource_ptr: *mut c_void, cpu_handle: u64) -> bool {
    unsafe {
        debug_println!("[create_shader_resource_view] Creating SRV");

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let resource = match ptr_to_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(cpu_handle) };

        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        };

        device.CreateShaderResourceView(&resource, Some(&desc), cpu_desc);

        std::mem::forget(resource);
        std::mem::forget(device);
        true
    }
}

#[no_mangle]
pub extern "C" fn create_render_target_view(device_ptr: *mut c_void, resource_ptr: *mut c_void, cpu_handle: u64) -> bool {
    unsafe {
        debug_println!("[create_render_target_view] Creating RTV");

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
pub extern "C" fn create_constant_buffer_view(device_ptr: *mut c_void, resource_ptr: *mut c_void, cpu_handle: u64, size: u32) -> bool {
    unsafe {
        debug_println!("[create_constant_buffer_view] size={}", size);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return false,
        };

        let resource = match ptr_to_resource(resource_ptr) {
            Some(r) => r,
            None => return false,
        };

        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(cpu_handle) };

        let desc = D3D12_CONSTANT_BUFFER_VIEW_DESC {
            BufferLocation: resource.GetGPUVirtualAddress(),
            SizeInBytes: size,
        };

        device.CreateConstantBufferView(Some(&desc), cpu_desc);

        std::mem::forget(resource);
        std::mem::forget(device);
        true
    }
}

#[no_mangle]
pub extern "C" fn set_vsync(enable: bool) {
    debug_println!("[set_vsync] enable={}", enable);
    // VSync управляется через Present параметрами, эта функция для будущего использования
}