// render.rs - Добавлены функции для инициализации рендера
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

// Структура для MVP матрицы
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MVPConstantBuffer {
    pub mvp: [[f32; 4]; 4],
}

impl MVPConstantBuffer {
    pub fn identity() -> Self {
        Self {
            mvp: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn ortho_2d(width: f32, height: f32) -> Self {
        Self {
            mvp: [
                [2.0 / width, 0.0, 0.0, 0.0],
                [0.0, -2.0 / height, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0, 1.0],
            ],
        }
    }
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
    }).is_some()
}

#[no_mangle]
pub extern "C" fn draw_indexed_instanced(index_count: u32, instance_count: u32, start_index: u32, base_vertex: i32, start_instance: u32) -> bool {
    with_command_list(|list| {
        unsafe {
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.DrawIndexedInstanced(index_count, instance_count, start_index, base_vertex, start_instance);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn draw_instanced(vertex_count: u32, instance_count: u32, start_vertex: u32, start_instance: u32) -> bool {
    with_command_list(|list| {
        unsafe {
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.DrawInstanced(vertex_count, instance_count, start_vertex, start_instance);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn clear_render_target(rtv_cpu_handle: u64, color: *const f32) -> bool {
    unsafe {
        if color.is_null() {
            return false;
        }

        with_command_list(|list| {
            let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
            let clear_color = [*color, *color.add(1), *color.add(2), *color.add(3)];
            list.ClearRenderTargetView(rtv, &clear_color, None);
        }).is_some()
    }
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
pub extern "C" fn set_render_targets(rtv_cpu_handle: u64, num_rtvs: u32) -> bool {
    with_command_list(|list| {
        let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: to_usize(rtv_cpu_handle) };
        unsafe {
            list.OMSetRenderTargets(num_rtvs, Some(&rtv_handle), false, None);
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_render_targets_with_depth(rtv_cpu_handle: u64, dsv_cpu_handle: u64, num_rtvs: u32) -> bool {
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
pub extern "C" fn create_render_target_view(device_ptr: *mut c_void, resource_ptr: *mut c_void, cpu_handle: u64) -> bool {
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
pub extern "C" fn create_depth_stencil_view(device_ptr: *mut c_void, resource_ptr: *mut c_void, cpu_handle: u64) -> bool {
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
        device.CreateDepthStencilView(&resource, None, cpu_desc);

        std::mem::forget(resource);
        std::mem::forget(device);
        true
    }
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
pub extern "C" fn set_root_descriptor_table(root_index: u32, gpu_handle: u64) -> bool {
    with_command_list(|list| {
        unsafe {
            list.SetGraphicsRootDescriptorTable(root_index, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: gpu_handle });
        }
    }).is_some()
}

#[no_mangle]
pub extern "C" fn set_render_target(rtv_cpu_handle: u64) -> bool {
    set_render_targets(rtv_cpu_handle, 1)
}

// Новая функция для создания depth buffer
#[no_mangle]
pub extern "C" fn create_depth_buffer(device_ptr: *mut c_void, width: u32, height: u32) -> *mut c_void {
    unsafe {
        debug_println!("\n[create_depth_buffer] Creating {}x{} depth buffer", width, height);

        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        };

        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 {
                DepthStencil: D3D12_DEPTH_STENCIL_VALUE {
                    Depth: 1.0,
                    Stencil: 0,
                },
            },
        };

        let mut buffer: Option<ID3D12Resource> = None;
        match device.CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_DEPTH_WRITE,
            Some(&clear_value),
            &mut buffer,
        ) {
            Ok(_) => {
                if let Some(buf) = buffer {
                    let raw_ptr = buf.as_raw();
                    std::mem::forget(buf);
                    debug_println!("[create_depth_buffer] ✅ Created");
                    raw_ptr as *mut c_void
                } else {
                    std::ptr::null_mut()
                }
            }
            Err(e) => {
                debug_println!("[create_depth_buffer] Failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

// Функция для получения RTV дескриптора из swap chain
#[no_mangle]
pub extern "C" fn create_rtv_for_swapchain_buffer(
    device_ptr: *mut c_void,
    swap_ptr: *mut c_void,
    rtv_heap_ptr: *mut c_void,
    buffer_index: u32,
) -> u64 {
    unsafe {
        let device = match ptr_to_device(device_ptr) {
            Some(d) => d,
            None => return 0,
        };

        let swap = match crate::utils::ptr_to_swapchain(swap_ptr) {
            Some(s) => s,
            None => return 0,
        };

        let rtv_heap: ID3D12DescriptorHeap = std::mem::transmute_copy(&rtv_heap_ptr);

        let buffer = match swap.GetBuffer::<ID3D12Resource>(buffer_index) {
            Ok(b) => b,
            Err(_) => return 0,
        };

        let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        let mut cpu_handle = rtv_heap.GetCPUDescriptorHandleForHeapStart();
        cpu_handle.ptr += (buffer_index as usize) * (rtv_size as usize);

        device.CreateRenderTargetView(&buffer, None, cpu_handle);

        std::mem::forget(buffer);
        std::mem::forget(rtv_heap);
        std::mem::forget(swap);
        std::mem::forget(device);

        cpu_handle.ptr as u64
    }
}