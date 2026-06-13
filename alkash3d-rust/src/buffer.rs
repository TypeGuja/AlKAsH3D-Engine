// src/buffer.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use crate::STATE;

#[derive(Clone)]
pub struct Buffer {
    pub resource: ID3D12Resource,
    pub size: u64,
    pub vertex_stride: u32,
}

impl Buffer {
    pub fn create_vertex_buffer(data: &[u8], stride: u32) -> Result<Self> {
        println!("[BUFFER] Creating vertex buffer, size: {} bytes, stride: {}", data.len(), stride);

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let size = data.len() as u64;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            let hr = device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            );
            if hr.is_err() {
                eprintln!("[BUFFER] CreateCommittedResource failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let resource = resource.ok_or_else(|| {
                eprintln!("[BUFFER] Resource is None");
                Error::from_hresult(HRESULT(1))
            })?;

            let mut mapped = std::ptr::null_mut();
            let hr = resource.Map(0, None, Some(&mut mapped));
            if hr.is_err() {
                eprintln!("[BUFFER] Map failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            if !mapped.is_null() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, data.len());
                println!("[BUFFER] Data copied successfully");
            } else {
                eprintln!("[BUFFER] Mapped pointer is null");
            }
            resource.Unmap(0, None);

            println!("[BUFFER] Vertex buffer created successfully");
            Ok(Self {
                resource,
                size,
                vertex_stride: stride,
            })
        }
    }

    pub fn create_index_buffer(data: &[u32]) -> Result<Self> {
        println!("[BUFFER] Creating index buffer, {} indices", data.len());
        let bytes: Vec<u8> = data.iter().flat_map(|&x| x.to_le_bytes()).collect();
        Self::create_vertex_buffer(&bytes, 4)
    }

    pub fn create_constant_buffer(size: u64) -> Result<Self> {
        println!("[BUFFER] Creating constant buffer, size: {} bytes", size);
        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let aligned_size = (size + 255) & !255;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: aligned_size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            let hr = device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            );
            if hr.is_err() {
                eprintln!("[BUFFER] CreateConstantBuffer failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let resource = resource.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

            println!("[BUFFER] Constant buffer created successfully");
            Ok(Self {
                resource,
                size: aligned_size,
                vertex_stride: 0,
            })
        }
    }

    pub fn update_constant_buffer(&self, data: &[u8]) -> Result<()> {
        unsafe {
            let mut mapped = std::ptr::null_mut();
            let _ = self.resource.Map(0, None, Some(&mut mapped));
            if !mapped.is_null() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, data.len().min(self.size as usize));
            }
            self.resource.Unmap(0, None);
        }
        Ok(())
    }
}