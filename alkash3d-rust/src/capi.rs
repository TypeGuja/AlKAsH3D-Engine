// src/capi.rs
//! ВОССТАНОВЛЕНО (по просьбе пользователя — синхронизация `alkash3d-execfile`
//! с текущим движком): плоский C-ABI поверх глобального `crate::STATE`,
//! экспортирующий РОВНО тот набор функций, которые `alkash3d-execfile/
//! src/renderer.rs` уже умеет грузить через `libloading` (см. историю —
//! старая `alkash3d_rs.dll` от апреля экспортировала именно этот набор
//! функций/сигнатур; текущий движок с тех пор перешёл на плагинный ABI
//! `src/plugin/abi.rs` и этот плоский слой больше не существовал).
//!
//! Ключевая архитектурная причина, почему большинство функций ниже НЕ
//! принимают "контекстные" указатели (device/queue) для маршрутизации —
//! `begin_frame()`, `end_frame()`, `wait_for_gpu()`, `get_frame_index()`,
//! `clear_render_target()`, `set_viewport()`, `set_scissor_rect()`,
//! `get_rtv_descriptor_size()` вообще не принимают device/queue-параметр в
//! оригинальных сигнатурах execfile — движок и раньше, и сейчас держит РОВНО
//! ОДНО глобальное состояние (`crate::STATE`, см. `lib.rs::GlobalState`), а
//! не набор независимых "контекстов рендера". Передаваемые execfile
//! указатели device/queue у большинства вызовов приняты только ради
//! совместимости сигнатур и полностью игнорируются — реальная маршрутизация
//! всегда идёт через `crate::get_device()`/`crate::STATE`.
//!
//! Все указатели, которые эта C-ABI отдаёт наружу (device/queue/swapchain/
//! heap/resource/command_list), — это `Box<CapiHandle>` (см. ниже), никак не
//! связанный с конкретным вызывающим кодом: execfile просто хранит их и
//! передаёт обратно в следующие вызовы этого же API, никогда их не
//! разыменовывая сам — так что закодировать их как непрозрачные владеющие
//! обёртки безопасно и не меняет наблюдаемое поведение со стороны execfile.
//!
//! ВАЖНО (см. закреплённый в памяти инвариант про
//! `wait_for_all_frames_idle_before_realloc` в `engine/render_frame.rs`):
//! этот модуль НЕ переиспользует и не трогает `ensure_*_capacity`/GPU-буферы
//! основного движка — `begin_frame`/`end_frame` ниже сознательно, как и
//! оригинальная DLL (см. лог `[wait_for_gpu] Fence already completed`
//! КАЖДЫЙ кадр), делают ПОЛНУЮ синхронную остановку GPU в конце каждого
//! кадра вместо конвейерного double-buffering — то есть намеренно самый
//! медленный, но и самый безопасный вариант: аллокатор текущего кадра
//! никогда не может быть переиспользован, пока GPU ещё выполняет команды из
//! него, потому что предыдущий кадр уже гарантированно завершён к этому
//! моменту.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, RECT, WAIT_TIMEOUT};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::{DXGI_PRESENT, IDXGISwapChain3};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::{CommandList, CommandQueue, DescriptorHeap, STATE};

/// Владеющая обёртка над одним COM-объектом, которую эта C-ABI прячет за
/// непрозрачным `*mut c_void` — см. объяснение в шапке модуля про то, почему
/// это безопасно (execfile никогда не разыменовывает эти указатели сам).
enum CapiHandle {
    Device(ID3D12Device),
    Queue(ID3D12CommandQueue),
    SwapChain(IDXGISwapChain3),
    Heap(ID3D12DescriptorHeap),
    Resource(ID3D12Resource),
    CommandList(ID3D12GraphicsCommandList),
}

fn into_handle(h: CapiHandle) -> *mut c_void {
    Box::into_raw(Box::new(h)) as *mut c_void
}

/// Читает handle БЕЗ передачи владения (в отличие от `release_resource`,
/// который заберёт и уронит `Box`) — вызывающая сторона (execfile) обязана
/// вызвать `release_resource` сама, когда указатель больше не нужен.
unsafe fn handle_ref<'a>(ptr: *mut c_void) -> Option<&'a CapiHandle> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*(ptr as *const CapiHandle) })
    }
}

/// Ограниченное по времени (5с — тот же таймаут, что и у `CommandQueue::flush`
/// в `queue.rs`, НЕ `windows::core::Result` `INFINITE`-вариант из
/// `utils::wait_for_fence`) ожидание конкретного значения fence — потерянное/
/// зависшее устройство не вешает вызывающий поток навсегда.
fn wait_for_fence_bounded(fence: &ID3D12Fence, value: u64) -> bool {
    unsafe {
        if fence.GetCompletedValue() >= value {
            return true;
        }
        let event = match CreateEventW(None, true, false, None) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[CAPI] wait_for_gpu: CreateEventW failed: {:?}", e);
                return false;
            }
        };
        if let Err(e) = fence.SetEventOnCompletion(value, event) {
            eprintln!("[CAPI] wait_for_gpu: SetEventOnCompletion failed: {:?}", e);
            let _ = CloseHandle(event);
            return false;
        }
        let result = WaitForSingleObject(event, 5000);
        let _ = CloseHandle(event);
        if result == WAIT_TIMEOUT {
            eprintln!("[CAPI] wait_for_gpu: timeout (5000ms) waiting for fence value {}", value);
            if let Some(reason) = crate::device_removed_reason() {
                eprintln!("[CAPI] wait_for_gpu: device removed, reason: {}", reason);
            }
            return false;
        }
        true
    }
}

/// См. `AlkashEngine::transition_barrier` в `engine/render_frame.rs` — тот же
/// ManuallyDrop-паттерн (клонируем COM-ссылку под барьер, обязаны сами
/// уменьшить refcount после `ResourceBarrier`, см. `drop_barrier` ниже), но
/// продублирован здесь как отдельная маленькая функция, а не вынесен в общий
/// код — тот метод приватный внутри `impl AlkashEngine`, и `capi.rs`
/// сознательно не заводит зависимость на структуру основного движка
/// (`AlkashEngine`), с которой у execfile нет и не должно быть ничего общего.
fn transition_barrier(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

unsafe fn drop_barrier(mut barrier: D3D12_RESOURCE_BARRIER) {
    unsafe {
        std::mem::ManuallyDrop::drop(&mut barrier.Anonymous.Transition);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_device() -> *mut c_void {
    match crate::D3D12Device::create() {
        Ok(()) => match crate::D3D12Device::get() {
            Some(d) => into_handle(CapiHandle::Device(d)),
            None => std::ptr::null_mut(),
        },
        Err(e) => {
            eprintln!("[CAPI] create_device failed: {:?}", e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_command_queue(_device: *mut c_void) -> *mut c_void {
    match CommandQueue::create() {
        Ok(()) => {
            let q = STATE.lock().unwrap().command_queue.clone();
            match q {
                Some(q) => into_handle(CapiHandle::Queue(q)),
                None => std::ptr::null_mut(),
            }
        }
        Err(e) => {
            eprintln!("[CAPI] create_command_queue failed: {:?}", e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_swap_chain(_queue: *mut c_void, hwnd: usize, width: u32, height: u32) -> *mut c_void {
    match crate::SwapChain::create(hwnd as isize, width, height, 2) {
        Ok(()) => {
            let sc = STATE.lock().unwrap().swap_chain.clone();
            match sc {
                Some(sc) => into_handle(CapiHandle::SwapChain(sc)),
                None => std::ptr::null_mut(),
            }
        }
        Err(e) => {
            eprintln!("[CAPI] create_swap_chain failed: {:?}", e);
            std::ptr::null_mut()
        }
    }
}

/// `heap_type`: 0=RTV, 1=DSV, 2=CBV/SRV/UAV — собственная условность этого
/// C-ABI (СОВПАДАЕТ с тем, что execfile уже передаёт — `0` для RTV heap —
/// но НЕ является значением нативного `D3D12_DESCRIPTOR_HEAP_TYPE`, не
/// путать одно с другим). `shader_visible` не читается: видимость для
/// шейдера у RTV/DSV хипов запрещена самим D3D12 (они и не бывают
/// shader-visible), а CBV/SRV/UAV хип ниже создаётся всегда shader-visible
/// через уже существующий `DescriptorHeap::create_cbv_srv_uav_heap`.
#[unsafe(no_mangle)]
pub extern "C" fn create_descriptor_heap(
    _device: *mut c_void,
    num_descriptors: u32,
    heap_type: u32,
    _shader_visible: bool,
) -> *mut c_void {
    let result = match heap_type {
        0 => DescriptorHeap::create_rtv_heap(num_descriptors),
        1 => DescriptorHeap::create_dsv_heap(num_descriptors),
        2 => DescriptorHeap::create_cbv_srv_uav_heap(num_descriptors),
        other => {
            eprintln!("[CAPI] create_descriptor_heap: неизвестный heap_type {} (ожидается 0=RTV/1=DSV/2=CBV_SRV_UAV)", other);
            return std::ptr::null_mut();
        }
    };
    match result {
        Ok(h) => into_handle(CapiHandle::Heap(h)),
        Err(e) => {
            eprintln!("[CAPI] create_descriptor_heap failed: {:?}", e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn GetCPUDescriptorHandleForHeapStart(heap: *mut c_void) -> u64 {
    match unsafe { handle_ref(heap) } {
        Some(CapiHandle::Heap(h)) => unsafe { h.GetCPUDescriptorHandleForHeapStart().ptr as u64 },
        _ => {
            eprintln!("[CAPI] GetCPUDescriptorHandleForHeapStart: указатель не является heap-хендлом");
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_render_target_view(_device: *mut c_void, resource: *mut c_void, handle: u64) -> bool {
    let Some(CapiHandle::Resource(r)) = (unsafe { handle_ref(resource) }) else {
        eprintln!("[CAPI] create_render_target_view: указатель не является resource-хендлом");
        return false;
    };
    match crate::get_device() {
        Ok(d) => {
            unsafe { d.CreateRenderTargetView(r, None, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: handle as usize }) };
            true
        }
        Err(_) => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn swap_chain_get_buffer(swap_chain: *mut c_void, index: u32) -> *mut c_void {
    let Some(CapiHandle::SwapChain(sc)) = (unsafe { handle_ref(swap_chain) }) else {
        eprintln!("[CAPI] swap_chain_get_buffer: указатель не является swapchain-хендлом");
        return std::ptr::null_mut();
    };
    match unsafe { sc.GetBuffer::<ID3D12Resource>(index) } {
        Ok(r) => into_handle(CapiHandle::Resource(r)),
        Err(e) => {
            eprintln!("[CAPI] swap_chain_get_buffer failed: {:?}", e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn get_rtv_descriptor_size() -> u32 {
    STATE.lock().unwrap().rtv_descriptor_size
}

#[unsafe(no_mangle)]
pub extern "C" fn create_command_allocators(_device: *mut c_void, count: u32) -> bool {
    CommandList::create_allocators(count).is_ok()
}

/// Соответствует одноразовому вызову в `execfile::Renderer::init()` — тот
/// сразу же вызывает `release_resource` на результате, не используя его
/// дальше (реальные, используемые per-frame командные списки создаются
/// заново на каждый кадр внутри `begin_frame()`, точно так же, как это
/// делает основной движок в `engine/render_frame.rs::render_frame()` — там
/// список тоже создаётся заново каждый кадр, а не переиспользуется через
/// `Reset()`). Единственная цель этого вызова — убедиться, что аллокатор[0]
/// уже существует и способен создать список (т.е. что `create_command_allocators`
/// был вызван раньше), прежде чем execfile перейдёт к рендер-циклу.
#[unsafe(no_mangle)]
pub extern "C" fn create_command_list(_device: *mut c_void) -> *mut c_void {
    let device = match crate::get_device() {
        Ok(d) => d,
        Err(_) => return std::ptr::null_mut(),
    };
    let Some(allocator) = CommandList::get_allocator(0) else {
        eprintln!("[CAPI] create_command_list: аллокатор[0] не найден — вызови create_command_allocators раньше");
        return std::ptr::null_mut();
    };
    unsafe {
        let result: windows::core::Result<ID3D12GraphicsCommandList> =
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None);
        match result {
            Ok(list) => {
                // Список создаётся уже в открытом (recording) состоянии —
                // закрываем сразу, раз он не будет использован для записи
                // команд (см. комментарий выше про doc-comment функции).
                if let Err(e) = list.Close() {
                    eprintln!("[CAPI] create_command_list: list.Close() failed: {:?}", e);
                }
                into_handle(CapiHandle::CommandList(list))
            }
            Err(e) => {
                eprintln!("[CAPI] create_command_list failed: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn create_fence(_device: *mut c_void) -> bool {
    match crate::create_fence() {
        Ok(fence) => {
            STATE.lock().unwrap().fence = Some(fence);
            true
        }
        Err(e) => {
            eprintln!("[CAPI] create_fence failed: {:?}", e);
            false
        }
    }
}

/// Начало кадра: сбрасывает аллокатор ТЕКУЩЕГО back buffer индекса (уже
/// гарантированно свободен — `wait_for_gpu()` предыдущего кадра дождался
/// его полного освобождения GPU, см. комментарий в шапке модуля), создаёт
/// НОВЫЙ командный список поверх него (тот же паттерн, что и
/// `engine/render_frame.rs::render_frame()`) и переводит текущий back
/// buffer из PRESENT в RENDER_TARGET.
#[unsafe(no_mangle)]
pub extern "C" fn begin_frame() -> bool {
    let frame_index = STATE.lock().unwrap().frame_index as usize;

    let Some(allocator) = CommandList::get_allocator(frame_index) else {
        eprintln!("[CAPI] begin_frame: аллокатор[{}] не найден", frame_index);
        return false;
    };
    let device = match crate::get_device() {
        Ok(d) => d,
        Err(_) => return false,
    };
    let swap_chain = match crate::get_swap_chain() {
        Ok(sc) => sc,
        Err(_) => return false,
    };

    unsafe {
        if let Err(e) = allocator.Reset() {
            eprintln!("[CAPI] begin_frame: allocator.Reset() failed: {:?}", e);
            return false;
        }

        let list: ID3D12GraphicsCommandList =
            match device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[CAPI] begin_frame: CreateCommandList failed: {:?}", e);
                    return false;
                }
            };

        let back_buffer = match swap_chain.GetBuffer::<ID3D12Resource>(frame_index as u32) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[CAPI] begin_frame: GetBuffer failed: {:?}", e);
                return false;
            }
        };
        let barrier = transition_barrier(&back_buffer, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        list.ResourceBarrier(&[barrier.clone()]);
        drop_barrier(barrier);

        STATE.lock().unwrap().command_list = Some(list);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn clear_render_target(rtv_handle: u64, color: *const f32) -> bool {
    if color.is_null() {
        eprintln!("[CAPI] clear_render_target: color == null");
        return false;
    }
    let Some(list) = STATE.lock().unwrap().command_list.clone() else {
        eprintln!("[CAPI] clear_render_target: нет активного командного списка — вызови begin_frame() раньше");
        return false;
    };
    let color: [f32; 4] = unsafe { std::slice::from_raw_parts(color, 4) }.try_into().unwrap();
    unsafe {
        list.ClearRenderTargetView(D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv_handle as usize }, &color, None);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn set_viewport(x: f32, y: f32, width: f32, height: f32, min_depth: f32, max_depth: f32) -> bool {
    let Some(list) = STATE.lock().unwrap().command_list.clone() else {
        return false;
    };
    let viewport = D3D12_VIEWPORT {
        TopLeftX: x,
        TopLeftY: y,
        Width: width,
        Height: height,
        MinDepth: min_depth,
        MaxDepth: max_depth,
    };
    unsafe {
        list.RSSetViewports(&[viewport]);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn set_scissor_rect(left: i32, top: i32, right: i32, bottom: i32) -> bool {
    let Some(list) = STATE.lock().unwrap().command_list.clone() else {
        return false;
    };
    let rect = RECT { left, top, right, bottom };
    unsafe {
        list.RSSetScissorRects(&[rect]);
    }
    true
}

/// Конец кадра: переводит back buffer обратно в PRESENT, закрывает и
/// отправляет командный список в очередь, сигналит fence. НЕ дожидается его
/// сам — это делает отдельный `wait_for_gpu()` ниже (в точности повторяя
/// порядок вызовов `execfile::Renderer::end_frame()`:
/// `end_frame()` -> `present_swap_chain()` -> `wait_for_gpu()`).
#[unsafe(no_mangle)]
pub extern "C" fn end_frame() -> bool {
    let frame_index = STATE.lock().unwrap().frame_index as usize;

    let Some(list) = STATE.lock().unwrap().command_list.clone() else {
        eprintln!("[CAPI] end_frame: нет активного командного списка — вызови begin_frame() раньше");
        return false;
    };
    let swap_chain = match crate::get_swap_chain() {
        Ok(sc) => sc,
        Err(_) => return false,
    };
    let queue = match crate::get_command_queue() {
        Ok(q) => q,
        Err(_) => return false,
    };

    unsafe {
        let back_buffer = match swap_chain.GetBuffer::<ID3D12Resource>(frame_index as u32) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[CAPI] end_frame: GetBuffer failed: {:?}", e);
                return false;
            }
        };
        let barrier = transition_barrier(&back_buffer, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
        list.ResourceBarrier(&[barrier.clone()]);
        drop_barrier(barrier);

        if let Err(e) = list.Close() {
            eprintln!("[CAPI] end_frame: list.Close() failed: {:?}", e);
            return false;
        }
        let cmd_lists: [Option<ID3D12CommandList>; 1] = [Some(list.into())];
        queue.ExecuteCommandLists(&cmd_lists);

        let fence_value = {
            let mut state = STATE.lock().unwrap();
            state.fence_values[0] += 1;
            state.fence_values[0]
        };
        let fence = STATE.lock().unwrap().fence.clone();
        let Some(fence) = fence else {
            eprintln!("[CAPI] end_frame: fence отсутствует — вызови create_fence() раньше");
            return false;
        };
        if let Err(e) = queue.Signal(&fence, fence_value) {
            eprintln!("[CAPI] end_frame: queue.Signal() failed: {:?}", e);
            return false;
        }
        println!("[CAPI] end_frame: успех (fence value: {})", fence_value);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn present_swap_chain(_swap_chain: *mut c_void, sync_interval: u32) -> bool {
    crate::SwapChain.present(sync_interval, DXGI_PRESENT(0)).is_ok()
}

/// Дожидается (с ограниченным таймаутом, см. `wait_for_fence_bounded`)
/// последнего засигналенного в `end_frame()` значения fence и обновляет
/// `frame_index` на следующий back buffer — см. комментарий в шапке модуля
/// про то, почему это ПОЛНАЯ синхронная остановка на каждый кадр (а не
/// конвейерный double-buffering), и почему это осознанно самый безопасный
/// вариант именно для этого простого C-ABI слоя.
#[unsafe(no_mangle)]
pub extern "C" fn wait_for_gpu() -> bool {
    let (fence, target) = {
        let state = STATE.lock().unwrap();
        (state.fence.clone(), state.fence_values[0])
    };
    let Some(fence) = fence else {
        eprintln!("[CAPI] wait_for_gpu: fence отсутствует");
        return false;
    };
    if !wait_for_fence_bounded(&fence, target) {
        return false;
    }
    let swap_chain = match crate::get_swap_chain() {
        Ok(sc) => sc,
        Err(_) => return false,
    };
    let idx = unsafe { swap_chain.GetCurrentBackBufferIndex() };
    STATE.lock().unwrap().frame_index = idx;
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn get_frame_index() -> u32 {
    STATE.lock().unwrap().frame_index
}

#[unsafe(no_mangle)]
pub extern "C" fn release_resource(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(ptr as *mut CapiHandle));
    }
}
