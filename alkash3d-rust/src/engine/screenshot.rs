//! ДОБАВЛЕНО (снимок кадра без снимка экрана): `ALKASH3D_SHOT=путь.png`
//! сохраняет кадр `ALKASH3D_SHOT_FRAME` (по умолчанию 120 — после прогрева
//! TAA и адаптации глаза) из back buffer — ровно то, что ушло бы на экран.
//! Снимок экрана ловит то окно, что сверху, а не игру; с `ALKASH3D_HIDDEN=1`
//! окно и вовсе не показывается (см. lifecycle.rs), и прогон не мешает
//! работать за компьютером.
//!
//! Копия back buffer в READBACK-буфер записывается в конец списка команд
//! кадра (`record`), после Signal кадра ждём его fence и пишем PNG (`save`) —
//! кадр со снимком один, ожидание GPU в нём не важно.

use std::cell::{Cell, RefCell};
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

struct Pending {
    readback: ID3D12Resource,
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    path: String,
}

thread_local! {
    static FRAME: Cell<u64> = const { Cell::new(0) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

fn request() -> Option<(String, u64)> {
    static REQ: std::sync::OnceLock<Option<(String, u64)>> = std::sync::OnceLock::new();
    REQ.get_or_init(|| {
        let path = std::env::var("ALKASH3D_SHOT").ok().filter(|p| !p.is_empty())?;
        let frame = std::env::var("ALKASH3D_SHOT_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
        Some((path, frame))
    })
    .clone()
}

/// Скопировать back buffer (в состоянии PRESENT) в READBACK-буфер, если этот
/// кадр — снимок. Вызывать в конце списка команд кадра, раз за кадр.
pub(super) unsafe fn record(cmd: &ID3D12GraphicsCommandList, back_buffer: &ID3D12Resource) {
    let frame = FRAME.with(|f| {
        let v = f.get();
        f.set(v + 1);
        v
    });
    let Some((path, at)) = request() else { return };
    if frame != at {
        return;
    }
    unsafe {
        let Ok(device) = crate::get_device() else { return };
        let desc = back_buffer.GetDesc();
        let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let mut total = 0u64;
        device.GetCopyableFootprints(&desc, 0, 1, 0, Some(&mut footprint), None, None, Some(&mut total));
        let props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
        let buf_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: total,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut readback: Option<ID3D12Resource> = None;
        if let Err(e) = device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &buf_desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback) {
            eprintln!("[ENGINE] снимок: буфер не создан: {:?}", e);
            return;
        }
        let readback = readback.unwrap();
        let barrier = |before, after| D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: std::mem::ManuallyDrop::new(Some(back_buffer.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
        };
        let drop_barrier = |b: D3D12_RESOURCE_BARRIER| {
            let mut t = std::mem::ManuallyDrop::into_inner(b.Anonymous.Transition);
            std::mem::ManuallyDrop::drop(&mut t.pResource);
        };
        let to = barrier(D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
        cmd.ResourceBarrier(std::slice::from_ref(&to));
        drop_barrier(to);
        let dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(readback.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint },
        };
        let src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(back_buffer.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        cmd.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
        let (mut dst, mut src) = (dst, src);
        std::mem::ManuallyDrop::drop(&mut dst.pResource);
        std::mem::ManuallyDrop::drop(&mut src.pResource);
        let back = barrier(D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
        cmd.ResourceBarrier(std::slice::from_ref(&back));
        drop_barrier(back);
        PENDING.with(|p| *p.borrow_mut() = Some(Pending { readback, footprint, path }));
    }
}

/// Если в этом кадре был снимок — дождаться его fence и записать PNG.
pub(super) fn save(fence: &ID3D12Fence, fence_value: u64) {
    let Some(p) = PENDING.with(|p| p.borrow_mut().take()) else { return };
    if let Err(e) = super::wait_for_fence(fence, fence_value, std::time::Duration::from_secs(5)) {
        eprintln!("[ENGINE] снимок: {}", e);
        return;
    }
    let f = p.footprint.Footprint;
    let (w, h, pitch) = (f.Width as usize, f.Height as usize, f.RowPitch as usize);
    // back buffer — R8G8B8A8 или B8G8R8A8 (UNORM / SRGB): байты уже в гамме экрана
    let bgra = matches!(f.Format, DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB);
    let mut rgb = Vec::with_capacity(w * h * 3);
    unsafe {
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        if let Err(e) = p.readback.Map(0, None, Some(&mut ptr)) {
            eprintln!("[ENGINE] снимок: Map: {:?}", e);
            return;
        }
        let base = (ptr as *const u8).add(p.footprint.Offset as usize);
        for y in 0..h {
            let row = std::slice::from_raw_parts(base.add(y * pitch), w * 4);
            for px in row.chunks_exact(4) {
                if bgra {
                    rgb.extend([px[2], px[1], px[0]]);
                } else {
                    rgb.extend([px[0], px[1], px[2]]);
                }
            }
        }
        p.readback.Unmap(0, None);
    }
    let write = || -> std::result::Result<(), Box<dyn std::error::Error>> {
        let file = std::io::BufWriter::new(std::fs::File::create(&p.path)?);
        let mut enc = png::Encoder::new(file, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&rgb)?;
        Ok(())
    };
    match write() {
        Ok(()) => println!("[ENGINE] снимок кадра: {} ({}x{})", p.path, w, h),
        Err(e) => eprintln!("[ENGINE] снимок: запись {}: {}", p.path, e),
    }
}
