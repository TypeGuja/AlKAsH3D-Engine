//! FFI модуль для динамической загрузки alkash3d_rs.dll

use std::ffi::c_void;
use std::sync::OnceLock;

static DLL_INSTANCE: OnceLock<AlkashDll> = OnceLock::new();

// Типы функций
type CreateDeviceFn = unsafe extern "C" fn() -> *mut c_void;
type CreateCommandQueueFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CreateSwapChainFn = unsafe extern "C" fn(*mut c_void, usize, u32, u32) -> *mut c_void;
type CreateDescriptorHeapFn = unsafe extern "C" fn(*mut c_void, u32, u32, bool) -> *mut c_void;
type CreateBufferFn = unsafe extern "C" fn(*mut c_void, usize, u32) -> *mut c_void;
type UpdateSubresourceFn = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool;
type ReleaseResourceFn = unsafe extern "C" fn(*mut c_void) -> bool;
type BeginFrameFn = unsafe extern "C" fn() -> bool;
type EndFrameFn = unsafe extern "C" fn() -> bool;
type PresentSwapChainFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type WaitForGpuFn = unsafe extern "C" fn() -> bool;
type ForceCleanupFn = unsafe extern "C" fn();
type GetBufferGpuAddressFn = unsafe extern "C" fn(*mut c_void) -> u64;
type SetVertexBufferFn = unsafe extern "C" fn(u64, u32, u32) -> bool;
type SetIndexBufferFn = unsafe extern "C" fn(u64, u32, u32) -> bool;
type SetGraphicsPipelineFn = unsafe extern "C" fn(*mut c_void) -> bool;
type SetRootSignatureFn = unsafe extern "C" fn(*mut c_void) -> bool;
type SetRootConstantBufferViewFn = unsafe extern "C" fn(u32, u64) -> bool;
type SetViewportFn = unsafe extern "C" fn(f32, f32, f32, f32, f32, f32) -> bool;
type SetScissorRectFn = unsafe extern "C" fn(i32, i32, i32, i32) -> bool;
type DrawIndexedInstancedFn = unsafe extern "C" fn(u32, u32, u32, i32, u32) -> bool;
type ClearRenderTargetFn = unsafe extern "C" fn(u64, *const f32) -> bool;
type ClearDepthStencilFn = unsafe extern "C" fn(u64, f32, u8) -> bool;
type SetRenderTargetsWithDepthFn = unsafe extern "C" fn(u64, u64, u32) -> bool;
type CreateRenderTargetViewFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool;
type CreateDepthStencilViewFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> bool;
type GetCPUDescriptorHandleForHeapStartFn = unsafe extern "C" fn(*mut c_void) -> u64;
type ResizeSwapChainFn = unsafe extern "C" fn(*mut c_void, u32, u32) -> bool;
type GetFrameIndexFn = unsafe extern "C" fn() -> u32;
type IsRealGpuFn = unsafe extern "C" fn() -> bool;
type GetGpuNameFn = unsafe extern "C" fn(*mut c_void) -> *const i8;
type CreateRootSignatureAdvancedFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CreateAdvancedPsoFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
type CreatePsoFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> *mut c_void;
type CreateCommandAllocatorsFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type CreateFenceFn = unsafe extern "C" fn(*mut c_void) -> bool;
type SetPrimitiveTopologyFn = unsafe extern "C" fn(u32) -> bool;

pub struct AlkashDll {
    pub lib: &'static libloading::Library,
    pub create_device: CreateDeviceFn,
    pub create_command_queue: CreateCommandQueueFn,
    pub create_swap_chain: CreateSwapChainFn,
    pub create_descriptor_heap: CreateDescriptorHeapFn,
    pub create_buffer: CreateBufferFn,
    pub update_subresource: UpdateSubresourceFn,
    pub release_resource: ReleaseResourceFn,
    pub begin_frame: BeginFrameFn,
    pub end_frame: EndFrameFn,
    pub present_swap_chain: PresentSwapChainFn,
    pub wait_for_gpu: WaitForGpuFn,
    pub force_cleanup: ForceCleanupFn,
    pub get_buffer_gpu_address: GetBufferGpuAddressFn,
    pub set_vertex_buffer: SetVertexBufferFn,
    pub set_index_buffer: SetIndexBufferFn,
    pub set_graphics_pipeline: SetGraphicsPipelineFn,
    pub set_root_signature: SetRootSignatureFn,
    pub set_root_constant_buffer_view: SetRootConstantBufferViewFn,
    pub set_viewport: SetViewportFn,
    pub set_scissor_rect: SetScissorRectFn,
    pub draw_indexed_instanced: DrawIndexedInstancedFn,
    pub clear_render_target: ClearRenderTargetFn,
    pub clear_depth_stencil: ClearDepthStencilFn,
    pub set_render_targets_with_depth: SetRenderTargetsWithDepthFn,
    pub create_render_target_view: CreateRenderTargetViewFn,
    pub create_depth_stencil_view: CreateDepthStencilViewFn,
    pub get_cpu_descriptor_handle_for_heap_start: GetCPUDescriptorHandleForHeapStartFn,
    pub resize_swap_chain: ResizeSwapChainFn,
    pub get_frame_index: GetFrameIndexFn,
    pub is_real_gpu: IsRealGpuFn,
    pub get_gpu_name: GetGpuNameFn,
    pub create_root_signature_advanced: CreateRootSignatureAdvancedFn,
    pub create_advanced_pso: CreateAdvancedPsoFn,
    pub create_pso: CreatePsoFn,
    pub create_command_allocators: CreateCommandAllocatorsFn,
    pub create_fence: CreateFenceFn,
    pub set_primitive_topology: SetPrimitiveTopologyFn,
}

impl AlkashDll {
    pub fn load() -> &'static Self {
        DLL_INSTANCE.get_or_init(|| {
            let dll_path = find_dll().expect("Failed to find DLL");
            println!("[FFI] Loading DLL from: {}", dll_path.display());

            let lib = Box::leak(Box::new(unsafe {
                libloading::Library::new(&dll_path)
                    .expect("Failed to load DLL")
            }));

            unsafe {
                AlkashDll {
                    lib,
                    create_device: load_fn(lib, "create_device"),
                    create_command_queue: load_fn(lib, "create_command_queue"),
                    create_swap_chain: load_fn(lib, "create_swap_chain"),
                    create_descriptor_heap: load_fn(lib, "create_descriptor_heap"),
                    create_buffer: load_fn(lib, "create_buffer"),
                    update_subresource: load_fn(lib, "update_subresource"),
                    release_resource: load_fn(lib, "release_resource"),
                    begin_frame: load_fn(lib, "begin_frame"),
                    end_frame: load_fn(lib, "end_frame"),
                    present_swap_chain: load_fn(lib, "present_swap_chain"),
                    wait_for_gpu: load_fn(lib, "wait_for_gpu"),
                    force_cleanup: load_fn(lib, "force_cleanup"),
                    get_buffer_gpu_address: load_fn(lib, "get_buffer_gpu_address"),
                    set_vertex_buffer: load_fn(lib, "set_vertex_buffer"),
                    set_index_buffer: load_fn(lib, "set_index_buffer"),
                    set_graphics_pipeline: load_fn(lib, "set_graphics_pipeline"),
                    set_root_signature: load_fn(lib, "set_root_signature"),
                    set_root_constant_buffer_view: load_fn(lib, "set_root_constant_buffer_view"),
                    set_viewport: load_fn(lib, "set_viewport"),
                    set_scissor_rect: load_fn(lib, "set_scissor_rect"),
                    draw_indexed_instanced: load_fn(lib, "draw_indexed_instanced"),
                    clear_render_target: load_fn(lib, "clear_render_target"),
                    clear_depth_stencil: load_fn(lib, "clear_depth_stencil"),
                    set_render_targets_with_depth: load_fn(lib, "set_render_targets_with_depth"),
                    create_render_target_view: load_fn(lib, "create_render_target_view"),
                    create_depth_stencil_view: load_fn(lib, "create_depth_stencil_view"),
                    get_cpu_descriptor_handle_for_heap_start: load_fn(lib, "GetCPUDescriptorHandleForHeapStart"),
                    resize_swap_chain: load_fn(lib, "resize_swap_chain"),
                    get_frame_index: load_fn(lib, "get_frame_index"),
                    is_real_gpu: load_fn(lib, "is_real_gpu"),
                    get_gpu_name: load_fn(lib, "get_gpu_name"),
                    create_root_signature_advanced: load_fn(lib, "create_root_signature_advanced"),
                    create_advanced_pso: load_fn(lib, "create_advanced_pso"),
                    create_pso: load_fn(lib, "create_pso"),
                    create_command_allocators: load_fn(lib, "create_command_allocators"),
                    create_fence: load_fn(lib, "create_fence"),
                    set_primitive_topology: load_fn(lib, "set_primitive_topology"),
                }
            }
        })
    }
}

unsafe fn load_fn<T: Copy>(lib: &libloading::Library, name: &str) -> T {
    *lib.get::<T>(name.as_bytes()).unwrap_or_else(|_| panic!("Failed to load {}", name))
}

fn find_dll() -> anyhow::Result<std::path::PathBuf> {
    let dll_name = "alkash3d_rs.dll";

    let search_paths = [
        std::env::current_dir()?.join(dll_name),
        std::env::current_dir()?.join("..").join("alkash3d-rust").join("target").join("release").join(dll_name),
        std::env::current_dir()?.join("..").join("target").join("release").join(dll_name),
        std::path::PathBuf::from("C:/Users/user/Documents/GitHub/AlKAsH3D-Engine/alkash3d-rust/target/release").join(dll_name),
    ];

    for path in &search_paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    anyhow::bail!("Could not find {}. Searched in: {:?}", dll_name, search_paths)
}