// src/device.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use crate::STATE;

pub struct D3D12Device;

impl D3D12Device {
    pub fn create() -> Result<()> {
        println!("[DEVICE] ========== CREATING D3D12 DEVICE ==========");
        let mut state = STATE.lock().unwrap();

        unsafe {
            println!("[DEVICE] Creating DXGI factory...");
            let dxgi_factory = CreateDXGIFactory1::<IDXGIFactory4>()?;
            println!("[DEVICE] DXGI factory created");

            println!("[DEVICE] Enumerating adapters...");
            let mut adapter: Option<IDXGIAdapter1> = None;
            for i in 0.. {
                match dxgi_factory.EnumAdapters1(i) {
                    Ok(adap) => {
                        let desc = adap.GetDesc1()?;
                        let is_software = (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0;
                        println!("[DEVICE] Adapter {}: {} - Software: {}", i,
                                 String::from_utf16_lossy(&desc.Description), is_software);
                        if !is_software {
                            adapter = Some(adap);
                            println!("[DEVICE] Selected hardware adapter");
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            let adapter = adapter.ok_or_else(|| {
                eprintln!("[DEVICE] ERROR: No suitable adapter found!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[DEVICE] Adapter selected");

            println!("[DEVICE] Creating D3D12 device...");
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &mut device)?;
            let device = device.ok_or_else(|| {
                eprintln!("[DEVICE] ERROR: Failed to create device!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[DEVICE] D3D12 device created");

            println!("[DEVICE] Getting descriptor sizes...");
            let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let dsv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
            let cbv_srv_uav_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            println!("[DEVICE] RTV size: {}, DSV size: {}, CBV/SRV/UAV size: {}",
                     rtv_size, dsv_size, cbv_srv_uav_size);

            state.device = Some(device);
            state.rtv_descriptor_size = rtv_size;
            state.dsv_descriptor_size = dsv_size;
            state.cbv_srv_uav_descriptor_size = cbv_srv_uav_size;
            println!("[DEVICE] ✓ D3D12 device initialized successfully");
        }

        Ok(())
    }

    pub fn get() -> Option<ID3D12Device> {
        STATE.lock().unwrap().device.clone()
    }
}