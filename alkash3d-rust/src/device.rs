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
            // ДОБАВЛЕНО (диагностика реального краша E_INVALIDARG на живой
            // машине, который не удавалось точно локализовать по одному
            // только коду по инструментам этой песочницы): включаем D3D12
            // debug layer ДО создания устройства. Она не меняет поведение
            // рендеринга — только добавляет валидацию API-вызовов и копит
            // подробные текстовые сообщения (какой именно вызов, с каким
            // параметром, нарушил какое именно правило) в ID3D12InfoQueue,
            // которые мы теперь читаем и печатаем в render_frame() при
            // ошибке (см. device_removed_reason()/dump_d3d12_debug_messages()
            // в lib.rs). Без него мы видим только голый HRESULT без деталей
            // — с ним рантайм называет ИМЕННО тот вызов и параметр, что
            // сломался. Не фатально, если недоступно (например на машине
            // не установлены Windows SDK "Graphics Tools" — опциональный
            // компонент Windows, без которого DXGIGetDebugInterface1
            // /D3D12GetDebugInterface вернёт ошибку) — тогда просто
            // продолжаем без валидации, как раньше.
            let mut debug: Option<ID3D12Debug> = None;
            match D3D12GetDebugInterface(&mut debug) {
                Ok(()) => {
                    if let Some(debug) = debug {
                        debug.EnableDebugLayer();
                        println!("[DEVICE] ✓ D3D12 debug layer enabled");
                    }
                }
                Err(e) => {
                    println!("[DEVICE] Debug layer unavailable ({:?}) — продолжаем без неё (не критично)", e);
                }
            }

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

            // ДОБАВЛЕНО: если debug layer выше реально включился, у самого
            // устройства (не у ID3D12Debug) можно запросить дополнительный
            // интерфейс ID3D12InfoQueue через QueryInterface (`.cast()`) —
            // именно через него читаются накопленные валидационные
            // сообщения. Если debug layer не был включён, `.cast()` здесь
            // тоже вернёт ошибку (интерфейс просто не реализован устройством
            // без debug layer) — это ожидаемо и не критично.
            match device.cast::<ID3D12InfoQueue>() {
                Ok(info_queue) => {
                    state.info_queue = Some(info_queue);
                    println!("[DEVICE] ✓ D3D12 info queue obtained (подробные сообщения об ошибках доступны)");
                }
                Err(_) => {
                    // Тихо — это ожидаемый случай, если debug layer не
                    // включился (см. предупреждение выше), отдельное
                    // сообщение об этом было бы шумом.
                }
            }

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