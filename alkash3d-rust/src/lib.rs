// src/lib.rs
#![allow(unused)]
#![allow(non_snake_case)]

mod device;
mod queue;
mod swap_chain;
mod heap;
mod buffer;
mod command;
mod render;
mod utils;
// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): texture.rs существовал в
// репозитории (create_texture2d/create_render_target/create_depth_stencil,
// с bounds-checked CPU->GPU upload), но НЕ был подключён к дереву
// модулей — не компилировался и не был доступен вообще. Нужен для HDR
// render target (Renderer::hdr_target) и промежуточных bloom-текстур.
pub mod texture;
mod shader;
mod pso;
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
// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): XAudio2-плейбек +
// WAV-декодер + 3D-позиционирование, см. подробный комментарий в начале
// audio.rs про архитектурное решение (нет отдельного готового аудио-
// плагина на диске пользователя, в отличие от alkash3d-inertial/
// alkash3d-FirstFires — поэтому звук реализован напрямую здесь).
pub mod audio;

mod plugin;
mod scheduler;
pub mod engine;  // engine зависит от Plugin и Sheduler

/// 3D Modules
pub mod math;
pub mod camera;
mod constant_buffer;

/// ECS / сцена — см. подробное описание в scene.rs. Объявлен как
/// `pub mod`, чтобы можно было писать как `alkash3d_rs::scene::EntityId`
/// (явно), так и использовать реэкспорт `alkash3d_rs::EntityId` ниже.
pub mod scene;

/// Система ввода — см. input.rs.
pub mod input;

///  in pub
pub use device::*;
pub use queue::*;
pub use swap_chain::*;
pub use heap::*;
pub use buffer::*;
pub use command::*;
pub use render::*;
pub use utils::*;
pub use texture::*;
pub use plugin::*;
pub use shader::*;
pub use pso::*;
pub use scheduler::*;
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
pub use audio::{AttenuationParams, AudioEngine, Listener, SoundClip, SoundHandle};

// НОВЫЕ ЭКСПОРТЫ
pub use math::*;
pub use camera::*;
pub use constant_buffer::*;
pub use scene::*;
pub use input::*;

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
    /// ДОБАВЛЕНО (диагностика краша E_INVALIDARG): интерфейс D3D12 debug
    /// layer для чтения подробных валидационных сообщений (см. device.rs —
    /// заполняется там, если EnableDebugLayer()/QueryInterface прошли
    /// успешно). `None`, если debug layer недоступен на машине — в этом
    /// случае dump_d3d12_debug_messages() ниже просто ничего не печатает.
    pub info_queue: Option<ID3D12InfoQueue>,
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
            info_queue: None,
        }
    }
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// =====================================================================
// ПАНИКО-БЕЗОПАСНЫЕ ХЕЛПЕРЫ ДОСТУПА К GLOBALSTATE
//
// ИСПРАВЛЕНО (производственная надёжность): раньше по всему движку было
// разбросано `state.device.as_ref().unwrap()` / `.unwrap().clone()` —
// если функция вызывалась до инициализации устройства (или после его
// потери, например TDR/DXGI_ERROR_DEVICE_REMOVED), это была МГНОВЕННАЯ
// паника и падение всего процесса, без единого шанса на graceful
// восстановление. Теперь такие обращения возвращают обычный
// `windows::core::Result<T>` — вызывающий код (который и так почти везде
// уже возвращает `Result` и распространяет ошибку через `?`) просто
// получает явную ошибку вместо паники. Сигнатуры функций-потребителей не
// меняются (они и раньше возвращали Result), так что это чисто аддитивное
// исправление, ничего не ломающее.
// =====================================================================

/// Текущий D3D12 device, либо явная ошибка вместо паники, если устройство
/// ещё не создано (или уже сброшено).
pub fn get_device() -> windows::core::Result<ID3D12Device> {
    let state = STATE.lock().unwrap();
    match &state.device {
        Some(d) => Ok(d.clone()),
        None => {
            eprintln!("[STATE] ERROR: get_device() called before device was initialized (or after it was released)");
            Err(Error::from_hresult(HRESULT(1)))
        }
    }
}

/// Текущая command queue, либо явная ошибка вместо паники.
pub fn get_command_queue() -> windows::core::Result<ID3D12CommandQueue> {
    let state = STATE.lock().unwrap();
    match &state.command_queue {
        Some(q) => Ok(q.clone()),
        None => {
            eprintln!("[STATE] ERROR: get_command_queue() called before command queue was initialized");
            Err(Error::from_hresult(HRESULT(1)))
        }
    }
}

/// Текущий swap chain, либо явная ошибка вместо паники.
pub fn get_swap_chain() -> windows::core::Result<IDXGISwapChain3> {
    let state = STATE.lock().unwrap();
    match &state.swap_chain {
        Some(s) => Ok(s.clone()),
        None => {
            eprintln!("[STATE] ERROR: get_swap_chain() called before swap chain was initialized");
            Err(Error::from_hresult(HRESULT(1)))
        }
    }
}

/// Текущий fence, либо явная ошибка вместо паники.
pub fn get_fence() -> windows::core::Result<ID3D12Fence> {
    let state = STATE.lock().unwrap();
    match &state.fence {
        Some(f) => Ok(f.clone()),
        None => {
            eprintln!("[STATE] ERROR: get_fence() called before fence was created");
            Err(Error::from_hresult(HRESULT(1)))
        }
    }
}

/// Проверяет, не потеряно ли устройство (TDR, обновление драйвера, GPU
/// hang и т.п.), и если да — возвращает человекочитаемую причину.
/// Полезно вызывать сразу после неудачного `Present()`/
/// `ExecuteCommandLists()`, чтобы не просто молча всё сломать, а понимать,
/// что именно произошло, вместо тихого `let _ = ...`, которое раньше было
/// в `render_frame()`.
pub fn device_removed_reason() -> Option<String> {
    let state = STATE.lock().unwrap();
    let device = state.device.as_ref()?;
    let reason = unsafe { device.GetDeviceRemovedReason() };
    match reason {
        Ok(()) => None, // устройство в порядке
        Err(e) => Some(format!("{:?}", e)),
    }
}

/// ДОБАВЛЕНО (диагностика реального краша E_INVALIDARG на живой машине):
/// печатает в stderr ВСЕ сообщения, накопленные D3D12 debug layer'ом (см.
/// `device.rs::D3D12Device::create()` — там он включается и туда же
/// сохраняется `ID3D12InfoQueue`). В отличие от голого HRESULT
/// (`Error { code: HRESULT(0x80070057), message: "Параметр задан
/// неверно." }` — не говорит НИЧЕГО о том, какой именно вызов и какой
/// именно параметр не так), debug layer называет ИМЕННО нарушенное
/// правило текстом (например "ID3D12CommandList::OMSetRenderTargets: ...
/// descriptor handle ... resides in a heap that ... is not currently
/// set..." — конкретный пример, реальный текст будет другим, но так же
/// точным). Ничего не делает (тихо), если debug layer недоступен —
/// `state.info_queue` в этом случае `None` (см. device.rs).
///
/// D3D12_MESSAGE — переменной длины структура: `GetMessage` вызывается
/// ДВАЖДЫ на каждое сообщение — первый раз с `pmessage=None`, чтобы
/// узнать нужный размер буфера в байтах, второй раз с реально выделенным
/// буфером этого размера, интерпретируемым как `D3D12_MESSAGE` (первые
/// байты буфера — сама структура, `pDescription` внутри неё указывает на
/// текст, лежащий следом, в том же буфере, который заполняет сам API) —
/// таков контракт этого COM-метода, тот же паттерн, что и в C++.
pub fn dump_d3d12_debug_messages() {
    let state = STATE.lock().unwrap();
    let Some(info_queue) = &state.info_queue else {
        eprintln!("[DEBUG-LAYER] Недоступен (debug layer не был включён при создании устройства) — подробностей об ошибке нет, только HRESULT выше.");
        return;
    };

    unsafe {
        let num = info_queue.GetNumStoredMessages();
        if num == 0 {
            eprintln!("[DEBUG-LAYER] Debug layer включён, но не накопил ни одного сообщения об этой ошибке (валидация могла не поймать её структурно) — HRESULT выше остаётся единственной подсказкой.");
            return;
        }
        eprintln!("[DEBUG-LAYER] ===== {} сообщени(е/й) от D3D12 debug layer =====", num);
        for i in 0..num {
            let mut len: usize = 0;
            if info_queue.GetMessage(i, None, &mut len).is_err() || len == 0 {
                continue;
            }
            let mut buffer = vec![0u8; len];
            let msg_ptr = buffer.as_mut_ptr() as *mut D3D12_MESSAGE;
            if info_queue.GetMessage(i, Some(msg_ptr), &mut len).is_ok() {
                let msg = &*msg_ptr;
                if !msg.pDescription.is_null() && msg.DescriptionByteLength > 0 {
                    let desc_bytes = std::slice::from_raw_parts(msg.pDescription, msg.DescriptionByteLength);
                    let desc = String::from_utf8_lossy(desc_bytes);
                    let desc = desc.trim_end_matches('\0');
                    eprintln!("[DEBUG-LAYER] [{:?} / {:?}] {}", msg.Severity, msg.Category, desc);
                } else {
                    eprintln!("[DEBUG-LAYER] (сообщение #{} без текста, ID={:?})", i, msg.ID);
                }
            }
        }
        eprintln!("[DEBUG-LAYER] ===== конец сообщений =====");
        info_queue.ClearStoredMessages();
    }
}
