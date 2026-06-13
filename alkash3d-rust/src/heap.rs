// src/heap.rs
use windows::Win32::Graphics::Direct3D12::*;
use crate::STATE;

pub struct DescriptorHeap;

impl DescriptorHeap {
    pub fn create_rtv_heap(count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        println!("[HEAP] Creating RTV heap with {} descriptors", count);
        let state = STATE.lock().unwrap();
        let device = state.device.as_ref().unwrap();

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };

        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ RTV heap created");
            Ok(heap)
        }
    }

    pub fn create_dsv_heap(count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        println!("[HEAP] Creating DSV heap with {} descriptors", count);
        let state = STATE.lock().unwrap();
        let device = state.device.as_ref().unwrap();

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };

        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ DSV heap created");
            Ok(heap)
        }
    }

    pub fn create_cbv_srv_uav_heap(count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        println!("[HEAP] Creating CBV/SRV/UAV heap with {} descriptors", count);
        let state = STATE.lock().unwrap();
        let device = state.device.as_ref().unwrap();

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };

        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ CBV/SRV/UAV heap created");
            Ok(heap)
        }
    }

    pub fn get_cpu_handle(heap: &ID3D12DescriptorHeap, index: u32, increment_size: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        unsafe {
            let handle = heap.GetCPUDescriptorHandleForHeapStart();
            let offset = (index as u64) * (increment_size as u64);
            let ptr = handle.ptr + offset as usize;
            println!("[HEAP] CPU handle: index={}, offset={}", index, offset);
            D3D12_CPU_DESCRIPTOR_HANDLE { ptr }
        }
    }

    pub fn get_gpu_handle(heap: &ID3D12DescriptorHeap, index: u32, increment_size: u32) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        unsafe {
            let handle = heap.GetGPUDescriptorHandleForHeapStart();
            let offset = (index as u64) * (increment_size as u64);
            let ptr = handle.ptr + offset;
            println!("[HEAP] GPU handle: index={}, offset={}", index, offset);
            D3D12_GPU_DESCRIPTOR_HANDLE { ptr }
        }
    }
}