// src/lib.rs - добавьте в начало файла
#![allow(unused)]
#![allow(non_snake_case)]

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
mod alworld_format;
mod almat_format;
mod alps_format;
mod alsnd_format;
mod alscript_format;
mod aluv_format;
mod scheduler;
pub mod engine;

pub use device::*;
pub use queue::*;
pub use swap_chain::*;
pub use heap::*;
pub use buffer::*;
pub use texture::Texture;
pub use shader::*;
pub use pso::*;
pub use command::*;
pub use render::*;
pub use utils::*;
pub use altex_format::*;
pub use alfar_format::*;
pub use alcar_format::*;
pub use alroute_format::*;
pub use alworld_format::*;
pub use almat_format::*;
pub use alps_format::*;
pub use alsnd_format::*;
pub use alscript_format::*;
pub use aluv_format::*;
pub use scheduler::*;

use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;
use windows_core::{Error, HRESULT};

pub static STATE: std::sync::LazyLock<Mutex<GlobalState>> =
    std::sync::LazyLock::new(|| Mutex::new(GlobalState::new()));

pub struct GlobalState {
    pub device: Option<ID3D12Device>,
    pub command_queue: Option<ID3D12CommandQueue>,
    pub swap_chain: Option<IDXGISwapChain3>,
    pub command_list: Option<ID3D12GraphicsCommandList>,
    pub command_allocators: Vec<Option<ID3D12CommandAllocator>>,
    pub root_signature: Option<ID3D12RootSignature>,
    pub rtv_descriptor_size: u32,
    pub dsv_descriptor_size: u32,
    pub cbv_srv_uav_descriptor_size: u32,
    pub frame_index: u32,
    pub fence: Option<ID3D12Fence>,
    pub fence_values: Vec<u64>,
    pub descriptor_heaps: Vec<ID3D12DescriptorHeap>,
    pub command_list_open: bool,
    pub current_pso: Option<ID3D12PipelineState>,
    pub bound_vertex_buffers: Vec<u64>,
    pub bound_index_buffer: Option<u64>,
    pub scheduler: Option<EngineScheduler>,
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
            current_pso: None,
            bound_vertex_buffers: Vec::new(),
            bound_index_buffer: None,
            scheduler: None,
        }
    }
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");