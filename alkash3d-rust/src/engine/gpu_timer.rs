//! ДОБАВЛЕНО (замер производительности по прямому запросу — 20 FPS на карте Самары):
//! время каждого прохода рендера НА ВИДЕОКАРТЕ (timestamp-запросы D3D12). Включается
//! вместе с `ALKASH3D_PERF=1` (см. render_frame.rs) и печатается в той же строке `[PERF]`.
//!
//! Метки ставятся на границах проходов (`mark`), в конце кадра пачка резолвится в
//! READBACK-буфер (`end`), а читается, когда этот же слот кадра снова начинается и fence
//! гарантирует, что GPU его закончил (`begin`) — без ожидания и без влияния на кадр.
//! Состояние — thread_local: рендер всегда идёт в одном потоке.

use std::cell::RefCell;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

/// Метки: начало кадра, после теней солнца, после теней фонарей, после основного
/// прохода, после объёмного света, после SSAO, после bloom, конец кадра.
pub const MARKS: usize = 8;
pub const LABELS: [&str; MARKS - 1] = ["тени солнца", "тени фонарей", "основной", "объёмный свет", "SSAO", "bloom", "tonemap"];
const SLOTS: usize = 4;

struct Timer {
    heap: ID3D12QueryHeap,
    readback: ID3D12Resource,
    freq: f64,
    pending: [bool; SLOTS],
    sum_ms: [f64; MARKS - 1],
    frames: u32,
}

thread_local! {
    static TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

fn create() -> Result<Timer> {
    let device = crate::get_device()?;
    let queue = crate::get_command_queue()?;
    unsafe {
        let desc = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: (MARKS * SLOTS) as u32, NodeMask: 0 };
        let mut heap: Option<ID3D12QueryHeap> = None;
        device.CreateQueryHeap(&desc, &mut heap)?;
        let heap = heap.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;
        let heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: (MARKS * SLOTS * 8) as u64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };
        let mut readback: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(&heap_properties, D3D12_HEAP_FLAG_NONE, &resource_desc,
                                       D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback)?;
        let readback = readback.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;
        let freq = queue.GetTimestampFrequency()? as f64;
        Ok(Timer { heap, readback, freq, pending: [false; SLOTS], sum_ms: [0.0; MARKS - 1], frames: 0 })
    }
}

/// Начало кадра (после ожидания fence этого слота): забрать результаты прошлого кадра слота.
pub fn begin(slot: usize) {
    TIMER.with(|t| {
        let mut t = t.borrow_mut();
        if t.is_none() {
            match create() {
                Ok(v) => *t = Some(v),
                Err(e) => {
                    eprintln!("[PERF] GPU-таймер недоступен: {:?}", e);
                    return;
                }
            }
        }
        let Some(tm) = t.as_mut() else { return };
        let slot = slot % SLOTS;
        if !tm.pending[slot] {
            return;
        }
        tm.pending[slot] = false;
        unsafe {
            let base = slot * MARKS * 8;
            let range = D3D12_RANGE { Begin: base, End: base + MARKS * 8 };
            let mut mapped = std::ptr::null_mut();
            if tm.readback.Map(0, Some(&range), Some(&mut mapped)).is_err() || mapped.is_null() {
                return;
            }
            let ts = std::slice::from_raw_parts((mapped as *const u8).add(base) as *const u64, MARKS);
            let ok = ts.windows(2).all(|w| w[1] >= w[0]) && ts[0] > 0;
            if ok {
                for i in 0..MARKS - 1 {
                    tm.sum_ms[i] += (ts[i + 1] - ts[i]) as f64 / tm.freq * 1000.0;
                }
                tm.frames += 1;
            }
            let none = D3D12_RANGE { Begin: 0, End: 0 };
            tm.readback.Unmap(0, Some(&none));
        }
    });
}

/// Метка i (0..MARKS) в текущем командном списке.
pub fn mark(cmd: &ID3D12GraphicsCommandList, slot: usize, i: usize) {
    TIMER.with(|t| {
        if let Some(tm) = t.borrow().as_ref() {
            unsafe { cmd.EndQuery(&tm.heap, D3D12_QUERY_TYPE_TIMESTAMP, ((slot % SLOTS) * MARKS + i) as u32) };
        }
    });
}

/// Конец кадра: резолв меток слота в READBACK-буфер (в том же командном списке).
pub fn end(cmd: &ID3D12GraphicsCommandList, slot: usize) {
    TIMER.with(|t| {
        if let Some(tm) = t.borrow_mut().as_mut() {
            let s = slot % SLOTS;
            unsafe {
                cmd.ResolveQueryData(&tm.heap, D3D12_QUERY_TYPE_TIMESTAMP, (s * MARKS) as u32, MARKS as u32,
                                     &tm.readback, (s * MARKS * 8) as u64);
            }
            tm.pending[s] = true;
        }
    });
}

/// Средние миллисекунды по проходам с прошлого вызова (и сброс), строкой для `[PERF]`.
pub fn report() -> Option<String> {
    TIMER.with(|t| {
        let mut t = t.borrow_mut();
        let tm = t.as_mut()?;
        if tm.frames == 0 {
            return None;
        }
        let n = tm.frames as f64;
        let total: f64 = tm.sum_ms.iter().sum::<f64>() / n;
        let parts: Vec<String> = LABELS.iter().zip(tm.sum_ms.iter()).map(|(l, s)| format!("{} {:.1}", l, s / n)).collect();
        tm.sum_ms = [0.0; MARKS - 1];
        tm.frames = 0;
        Some(format!("GPU {:.1} мс: {}", total, parts.join(", ")))
    })
}
