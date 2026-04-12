//! alkash3d_rs - DirectX 12 рендерер на Rust

mod device;
mod queue;
mod swap_chain;
mod heap;
mod buffer;
mod texture;
mod shader;
mod pso;
mod command;
mod render;
mod utils;
mod altex_format;
mod alfar_format;
mod alcar_format;
mod alroute_format;

pub use device::*;
pub use queue::*;
pub use swap_chain::*;
pub use heap::*;
pub use buffer::*;
pub use texture::*;
pub use shader::*;
pub use pso::*;
pub use command::*;
pub use render::*;
pub use altex_format::*;
pub use alfar_format::*;
pub use alcar_format::*;
pub use alroute_format::*;

// Глобальное состояние
static STATE: std::sync::LazyLock<std::sync::Mutex<GlobalState>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(GlobalState::new()));

pub struct GlobalState {
    pub device: Option<windows::Win32::Graphics::Direct3D12::ID3D12Device>,
    pub command_queue: Option<windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue>,
    pub swap_chain: Option<windows::Win32::Graphics::Dxgi::IDXGISwapChain3>,
    pub command_list: Option<windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList>,
    pub command_allocators: Vec<Option<windows::Win32::Graphics::Direct3D12::ID3D12CommandAllocator>>,
    pub root_signature: Option<windows::Win32::Graphics::Direct3D12::ID3D12RootSignature>,
    pub rtv_descriptor_size: u32,
    pub dsv_descriptor_size: u32,
    pub cbv_srv_uav_descriptor_size: u32,
    pub frame_index: u32,
    pub fence: Option<windows::Win32::Graphics::Direct3D12::ID3D12Fence>,
    pub fence_values: Vec<u64>,
    pub descriptor_heaps: Vec<windows::Win32::Graphics::Direct3D12::ID3D12DescriptorHeap>,
    pub command_list_open: bool,
}

impl GlobalState {
    fn new() -> Self {
        Self {
            device: None,
            command_queue: None,
            swap_chain: None,
            command_list: None,
            command_allocators: Vec::new(),
            root_signature: None,
            rtv_descriptor_size: 0,
            dsv_descriptor_size: 0,
            cbv_srv_uav_descriptor_size: 0,
            frame_index: 0,
            fence: None,
            fence_values: vec![0; 4],
            descriptor_heaps: Vec::new(),
            command_list_open: false,
        }
    }
}