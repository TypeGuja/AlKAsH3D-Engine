// src/queue.rs
use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use crate::STATE;

/// ДОБАВЛЕНО (фикс "белого окна" + краша драйвера — см. подробный
/// комментарий у `wait_for_fence` в engine/mod.rs про общий класс бага):
/// `flush()` ниже раньше ждал `0xFFFFFFFF` (то есть `INFINITE`) без
/// таймаута — тот же риск вечного зависания вызывающего потока при
/// потерянном/зависшем GPU, что уже был исправлен для per-frame
/// ожиданий в engine/mod.rs и для загрузки текстур в texture.rs.
const WAIT_TIMEOUT_MS: u32 = 5000;

pub struct CommandQueue;

impl CommandQueue {
    pub fn create() -> Result<(), windows::core::Error> {
        println!("[QUEUE] Creating command queue...");
        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap()`.
        let device = crate::get_device()?;
        let mut state = STATE.lock().unwrap();

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
                    // ИЗМЕНЕНО: было `WaitForSingleObject(event, 0xFFFFFFFF)`
                    // (INFINITE) — см. комментарий у `WAIT_TIMEOUT_MS` в
                    // начале файла. Ограниченный таймаут вместо вечного
                    // ожидания.
                    let wait_result = WaitForSingleObject(event, WAIT_TIMEOUT_MS);
                    let _ = CloseHandle(event);
                    if wait_result == WAIT_TIMEOUT {
                        eprintln!("[QUEUE] ERROR: timeout ({} ms) waiting for fence in flush()", WAIT_TIMEOUT_MS);
                        if let Some(reason) = crate::device_removed_reason() {
                            eprintln!("[QUEUE] Device removed, reason: {}", reason);
                        }
                        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(1)));
                    }
                    println!("[QUEUE] Fence completed");
                } else {
                    println!("[QUEUE] Fence already completed");
                }
            }
        }

        Ok(())
    }
}