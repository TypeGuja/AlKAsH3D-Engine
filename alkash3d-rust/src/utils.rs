// src/utils.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use crate::STATE;

pub fn create_fence() -> Result<ID3D12Fence> {
    println!("[UTILS] Creating fence...");
    let state = STATE.lock().unwrap();
    let device = state.device.as_ref().unwrap();
    unsafe {
        let fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
        println!("[UTILS] ✓ Fence created");
        Ok(fence)
    }
}

pub fn create_event() -> windows::Win32::Foundation::HANDLE {
    unsafe {
        let event = windows::Win32::System::Threading::CreateEventW(None, true, false, None)
            .expect("Failed to create event");
        println!("[UTILS] Event created");
        event
    }
}

pub fn wait_for_fence(fence: &ID3D12Fence, value: u64) -> Result<()> {
    println!("[UTILS] Waiting for fence value {}", value);
    unsafe {
        if fence.GetCompletedValue() < value {
            let event = create_event();
            fence.SetEventOnCompletion(value, event)?;
            windows::Win32::System::Threading::WaitForSingleObject(event, 0xFFFFFFFF);
            println!("[UTILS] Wait completed");
        } else {
            println!("[UTILS] Fence already at or above target value");
        }
    }
    Ok(())
}

pub fn align_up(value: u64, alignment: u64) -> u64 {
    let result = (value + alignment - 1) & !(alignment - 1);
    println!("[UTILS] align_up({}, {}) = {}", value, alignment, result);
    result
}