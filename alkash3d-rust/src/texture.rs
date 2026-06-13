// src/texture.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT, DXGI_SAMPLE_DESC};
use crate::STATE;

pub struct Texture {
    pub resource: ID3D12Resource,
    pub width: u32,
    pub height: u32,
    pub format: DXGI_FORMAT,
    pub mip_levels: u32,
}

impl Texture {
    pub fn create_texture2d(width: u32, height: u32, format: DXGI_FORMAT, data: Option<&[u8]>) -> Result<Self> {
        println!("[TEXTURE] Creating 2D texture: {}x{}, format={:?}, data={}", width, height, format, data.is_some());

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let heap_properties = if data.is_some() {
            D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() }
        } else {
            D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() }
        };
        println!("[TEXTURE] Heap type: {:?}", heap_properties.Type);

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height as u32,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let initial_state = if data.is_some() {
            D3D12_RESOURCE_STATE_GENERIC_READ
        } else {
            D3D12_RESOURCE_STATE_COMMON
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            println!("[TEXTURE] Creating committed resource...");
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                initial_state,
                None,
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: Resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[TEXTURE] ✓ Resource created");

            if let Some(bytes) = data {
                println!("[TEXTURE] Uploading texture data...");
                let mut mapped = std::ptr::null_mut();
                let _ = resource.Map(0, None, Some(&mut mapped));
                if !mapped.is_null() {
                    let row_pitch = width * 4;
                    for y in 0..height {
                        let src = &bytes[(y * row_pitch) as usize..];
                        let dst = (mapped as *mut u8).add((y * row_pitch) as usize);
                        std::ptr::copy_nonoverlapping(src.as_ptr(), dst, row_pitch as usize);
                    }
                    println!("[TEXTURE] Data uploaded");
                } else {
                    eprintln!("[TEXTURE] ERROR: Mapped pointer is null!");
                }
                resource.Unmap(0, None);
            }

            Ok(Self {
                resource,
                width,
                height,
                format,
                mip_levels: 1,
            })
        }
    }

    pub fn create_render_target(width: u32, height: u32, format: DXGI_FORMAT) -> Result<Self> {
        println!("[TEXTURE] Creating render target: {}x{}, format={:?}", width, height, format);

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: format,
            Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.1, 0.1, 0.2, 1.0] },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            println!("[TEXTURE] Creating render target resource...");
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: Render target resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[TEXTURE] ✓ Render target created");

            Ok(Self {
                resource,
                width,
                height,
                format,
                mip_levels: 1,
            })
        }
    }

    pub fn create_depth_stencil(width: u32, height: u32) -> Result<Self> {
        println!("[TEXTURE] Creating depth stencil: {}x{}", width, height);

        let device = {
            let state = STATE.lock().unwrap();
            state.device.as_ref().unwrap().clone()
        };

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };

        let resource_desc = D3D12_RESOURCE_DESC {
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
            Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            println!("[TEXTURE] Creating depth stencil resource...");
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: Depth stencil resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[TEXTURE] ✓ Depth stencil created");

            Ok(Self {
                resource,
                width,
                height,
                format: DXGI_FORMAT_D32_FLOAT,
                mip_levels: 1,
            })
        }
    }
}