//! Функции рендеринга

use std::ffi::c_void;
use std::slice;
use windows::Win32::{
    Foundation::RECT,
    Graphics::{Direct3D12::*, Dxgi::Common::*},
};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows_core::Interface;
use crate::{STATE, debug_println, utils::{ptr_to_device, ptr_to_resource}};

// Вспомогательная функция для преобразования u64 в usize
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
pub extern "C" fn set_render_target(rtv_cpu_handle: u64) -> bool {
    unsafe {
        let state = STATE.lock().unwrap();
        let list = match state.command_list.clone() {
            Some(l) => l,
            None => return false,
        };
        drop(state);

        let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        list.OMSetRenderTargets(1, Some(&rtv), false, None);
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

// Остальные функции-заглушки для нереализованных функций
#[no_mangle]
pub extern "C" fn set_root_descriptor_table(_root_index: u32, _gpu_handle: u64) -> bool {
    debug_println!("[set_root_descriptor_table] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_descriptor_heaps(_count: usize, _heaps: *const *mut c_void) -> bool {
    debug_println!("[set_descriptor_heaps] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_root_constant_buffer_view(_root_index: u32, _gpu_address: u64) -> bool {
    debug_println!("[set_root_constant_buffer_view] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_render_targets(_count: usize, _rtvs: *const u64, _dsv_cpu_handle: u64) -> bool {
    debug_println!("[set_render_targets] stub");
    true
}

#[no_mangle]
pub extern "C" fn clear_depth_stencil(_dsv_cpu_handle: u64, _flags: u32, _depth: f32, _stencil: u8) -> bool {
    debug_println!("[clear_depth_stencil] stub");
    true
}

#[no_mangle]
pub extern "C" fn create_vertex_buffer_view(_buffer_ptr: *mut c_void, _stride: u32, _size: u32, _out_view: *mut c_void) -> bool {
    debug_println!("[create_vertex_buffer_view] stub");
    false
}

#[no_mangle]
pub extern "C" fn create_index_buffer_view(_buffer_ptr: *mut c_void, _size: u32, _format: u32, _out_view: *mut c_void) -> bool {
    debug_println!("[create_index_buffer_view] stub");
    false
}

#[no_mangle]
pub extern "C" fn create_shader_resource_view(_device_ptr: *mut c_void, _resource_ptr: *mut c_void, _cpu_handle: u64) -> bool {
    debug_println!("[create_shader_resource_view] stub");
    true
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

        let cpu_desc = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: cpu_handle as usize };
        device.CreateRenderTargetView(&resource, None, cpu_desc);

        std::mem::forget(resource);
        std::mem::forget(device);
        true
    }
}
#[no_mangle]
pub extern "C" fn create_constant_buffer_view(_device_ptr: *mut c_void, _resource_ptr: *mut c_void, _cpu_handle: u64, _size: u32) -> bool {
    debug_println!("[create_constant_buffer_view] stub");
    true
}

#[no_mangle]
pub extern "C" fn set_vsync(_enable: bool) {
    debug_println!("[set_vsync] stub");
}