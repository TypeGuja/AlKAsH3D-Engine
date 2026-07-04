// src/queue.rs
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use crate::STATE;

pub struct CommandQueue;

impl CommandQueue {
    pub fn create() -> Result<(), windows::core::Error> {
        println!("[QUEUE] Creating command queue...");
        let mut state = STATE.lock().unwrap();
        let device = state.device.as_ref().unwrap();

        let desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0 as i32,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        };
        println!("[QUEUE] Queue type: DIRECT, priority: NORMAL");

        unsafe {
            let queue = device.CreateCommandQueue(&desc)?;
            state.command_queue = Some(queue);
            println!("[QUEUE] ✓ Command queue created");
        }

        Ok(())
    }

    pub fn execute_command_lists(&self, lists: &[ID3D12GraphicsCommandList]) {
        let state = STATE.lock().unwrap();
        if let Some(queue) = &state.command_queue {
            let cmd_lists: Vec<Option<ID3D12CommandList>> = lists.iter()
                .map(|cmd| Some(cmd.clone().into()))
                .collect();
            println!("[QUEUE] Executing {} command lists", cmd_lists.len());
            unsafe {
                queue.ExecuteCommandLists(&cmd_lists);
            }
            println!("[QUEUE] Command lists executed");
        }
    }

    pub fn signal_fence(&self, fence_value: u64) -> Result<(), windows::core::Error> {
        let state = STATE.lock().unwrap();
        if let (Some(queue), Some(fence)) = (&state.command_queue, &state.fence) {
            println!("[QUEUE] Signaling fence with value {}", fence_value);
            unsafe { queue.Signal(fence, fence_value) }?;
            println!("[QUEUE] Fence signaled");
        }
        Ok(())
    }

    /// Блокирующе дожидается завершения всех команд, отправленных в очередь
    /// до этого момента.
    ///
    /// ИСПРАВЛЕНО: раньше эта функция брала `STATE.lock()` и держала его,
    /// одновременно вызывая `self.signal_fence(...)`, которая пытается
    /// взять тот же `Mutex` повторно. `std::sync::Mutex` не реентерабельный,
    /// поэтому это гарантированно вешало поток при первом же вызове
    /// `flush()`. Теперь лок берётся отдельными короткими секциями и
    /// не удерживается поперёк вызова `signal_fence`.
    pub fn flush(&self) -> Result<(), windows::core::Error> {
        println!("[QUEUE] Flushing command queue...");

        let fence_value = {
            let mut state = STATE.lock().unwrap();
            state.fence_values[0] += 1;
            state.fence_values[0]
        };
        println!("[QUEUE] New fence value: {}", fence_value);

        self.signal_fence(fence_value)?;

        let fence = {
            let state = STATE.lock().unwrap();
            state.fence.clone()
        };

        if let Some(fence) = fence {
            unsafe {
                let completed = fence.GetCompletedValue();
                if completed < fence_value {
                    println!("[QUEUE] Waiting for fence (completed={}, target={})", completed, fence_value);
                    let event = CreateEventW(None, true, false, None)?;
                    fence.SetEventOnCompletion(fence_value, event)?;
                    WaitForSingleObject(event, 0xFFFFFFFF);
                    CloseHandle(event);
                    println!("[QUEUE] Fence completed");
                } else {
                    println!("[QUEUE] Fence already completed");
                }
            }
        }

        Ok(())
    }
}