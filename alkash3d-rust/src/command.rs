// src/command.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;
use windows::Win32::Foundation::RECT;
use crate::STATE;

pub struct CommandList;

impl CommandList {
    pub fn create_allocators(count: u32) -> Result<()> {
        println!("[COMMAND] ========== CREATING ALLOCATORS ==========");
        println!("[COMMAND] Requested allocator count: {}", count);

        let device = {
            let state = STATE.lock().unwrap();
            match &state.device {
                Some(d) => d.clone(),
                None => {
                    eprintln!("[COMMAND] ERROR: Device is None!");
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        };

        let mut state = STATE.lock().unwrap();
        state.command_allocators.clear();

        for i in 0..count {
            unsafe {
                println!("[COMMAND] Creating allocator {}...", i);
                let allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
                state.command_allocators.push(Some(allocator));
                println!("[COMMAND] Allocator {} created", i);
            }
        }
        println!("[COMMAND] ✓ {} allocators created", count);
        Ok(())
    }

    pub fn get_allocator(index: usize) -> Option<ID3D12CommandAllocator> {
        let state = STATE.lock().unwrap();
        state.command_allocators.get(index)?.as_ref().cloned()
    }
}