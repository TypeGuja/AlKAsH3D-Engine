//! Главный проход рендера кадра: shadow-проход (cascaded shadow maps),
//! основной 3D draw pass (frustum culling + occlusion culling + материалы +
//! constant buffer per-object), volumetric god-rays, bloom (extract+blur
//! ping-pong), composite/tonemap в back buffer, Present. Плюс рост GPU-
//! буферов по требованию (`ensure_*_capacity`) и хелперы transition-барьеров.
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись — видимость `ensure_constant_buffer_capacity` поднята
//! до `pub(super)`, т.к. её вызывает `init()`, оставшийся в mod.rs.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;
use std::sync::atomic::Ordering;
use std::time::Instant;
use crate::STATE;
use crate::buffer::Buffer;
use crate::plugin::{GPULight, LightGridCell, LightGridEntry};
use crate::constant_buffer::TransformConstants;
use crate::command::CommandList;
use crate::math::{identity, Mat4, Vec3};
use super::{
    AlkashEngine, Vertex, NUM_CASCADES, CASCADE_SPLITS, SHADOW_MAP_RESOLUTION,
    MAX_SPOT_SHADOWS, SPOT_SHADOW_TILES_PER_ROW, SPOT_SHADOW_TILE_RESOLUTION,
    MAX_POINT_SHADOWS, POINT_SHADOW_TILES_PER_ROW, POINT_SHADOW_TILE_RESOLUTION,
    OCCLUDER_MIN_WORLD_RADIUS, OCCLUDER_INSCRIBE_FACTOR,
    NEXT_FENCE_VALUE, wait_for_fence,
};


// ДОБАВЛЕНО (замер производительности по прямому запросу — 20 FPS на карте Самары):
// ALKASH3D_PERF=1 — раз в секунду печатает FPS, время CPU на запись кадра, ожидание
// GPU (fence + Present) и число draw-вызовов/треугольников по проходам. Если почти всё
// время уходит в ожидание GPU — упираемся в видеокарту, если в запись — в процессор.
// ИЗМЕНЕНО: по умолчанию Present без вертикальной синхронизации (FPS не упирается в
// частоту монитора и не скачет ступеньками 60/30/20). ALKASH3D_VSYNC=1 — включить vsync.
#[derive(Default)]
struct PerfAcc {
    since: Option<Instant>,
    frames: u32,
    wait_ms: f64,
    record_ms: f64,
    present_ms: f64,
    late_frames: u32,
    main_draws: u64,
    main_tris: u64,
    csm_draws: u64,
    local_shadow_draws: u64,
}

static PERF: std::sync::Mutex<Option<PerfAcc>> = std::sync::Mutex::new(None);

// ДОБАВЛЕНО (производительность — тени фонарей): кэш плиток атласов локальных теней.
// Фонари и город неподвижны, а тени для 8 point-фонарей × 6 граней (+ spot) перерисовывались
// каждый кадр — десятки проходов по окрестной геометрии. Теперь у плитки есть отпечаток
// (матрица view-proj + список попавших в неё мешей с их матрицами); совпал с прошлым кадром —
// глубина в плитке уже верная, плитка не перерисовывается и не очищается. Въехала машина,
// подгрузился чанк, сменился фонарь — отпечаток другой, перерисовывается только эта плитка.
// Чтобы фонарь не скакал по плиткам при пересортировке по расстоянию, слоты стабильные
// (`stable_slots`): оставшийся в выборке фонарь держит свой слот.
#[derive(Default)]
struct LocalShadowCache {
    atlas_ids: (usize, usize),
    spot_keys: Vec<(i64, i64, i64)>,
    point_keys: Vec<(i64, i64, i64)>,
    tile_hash: std::collections::HashMap<(usize, u32), u64>,
    /// Отпечатки плиток без движущихся мешей — для атласов в режиме кэша статики.
    static_hash: std::collections::HashMap<(usize, u32), u64>,
    /// Атлас [spot, point] перешёл в режим кэша статики (в нём бывал `Mesh::moving`).
    moving_mode: [bool; 2],
    /// Плитки, где в атласе сейчас нарисовано движущееся поверх статики.
    has_moving: std::collections::HashSet<(usize, u32)>,
}

thread_local! {
    static LOCAL_SHADOW_CACHE: std::cell::RefCell<LocalShadowCache> = std::cell::RefCell::new(LocalShadowCache::default());
}

fn light_key(lights: &[GPULight], index: usize) -> (i64, i64, i64) {
    let p = lights.get(index).map(|l| l.position).unwrap_or([0.0; 4]);
    ((p[0] * 10.0).round() as i64, (p[1] * 10.0).round() as i64, (p[2] * 10.0).round() as i64)
}

/// Переставляет выбранные фонари так, чтобы оставшиеся с прошлого кадра сохранили позицию
/// (= слот плитки атласа), а новые заняли освободившиеся. `prev` обновляется.
fn stable_slots<T: Clone>(sel: Vec<(usize, T)>, lights: &[GPULight], prev: &mut Vec<(i64, i64, i64)>) -> Vec<(usize, T)> {
    let keys: Vec<_> = sel.iter().map(|(i, _)| light_key(lights, *i)).collect();
    let mut out: Vec<Option<(usize, T)>> = vec![None; sel.len()];
    let mut placed = vec![false; sel.len()];
    for (slot, k) in prev.iter().enumerate() {
        if slot >= out.len() {
            break;
        }
        if let Some(j) = keys.iter().position(|kk| kk == k) {
            if !placed[j] {
                out[slot] = Some(sel[j].clone());
                placed[j] = true;
            }
        }
    }
    let mut rest = (0..sel.len()).filter(|j| !placed[*j]);
    for slot in out.iter_mut() {
        if slot.is_none() {
            if let Some(j) = rest.next() {
                *slot = Some(sel[j].clone());
            }
        }
    }
    let out: Vec<(usize, T)> = out.into_iter().flatten().collect();
    *prev = out.iter().map(|(i, _)| light_key(lights, *i)).collect();
    out
}

fn tile_fingerprint(vp: &Mat4, jobs: &[usize], shadow_jobs: &[(usize, Mat4)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in vp.to_cols_array() {
        v.to_bits().hash(&mut h);
    }
    for &j in jobs {
        let (mi, m) = &shadow_jobs[j];
        mi.hash(&mut h);
        for v in m.to_cols_array() {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).map(|v| v != "0" && !v.is_empty()).unwrap_or(false)
}

fn perf_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| env_flag("ALKASH3D_PERF"))
}

fn no_vsync() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !env_flag("ALKASH3D_VSYNC"))
}

/// ДОБАВЛЕНО (адаптивный vsync — подъезд на 1080p: кадр GPU ~15 мс, часть
/// кадров всё же опаздывает к обновлению экрана, и vsync держит их ещё
/// период — рывок до 30 FPS): период обновления основного монитора, с.
/// `None` — частоту не узнать (тогда обычный vsync).
fn refresh_period() -> Option<f64> {
    static P: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *P.get_or_init(|| {
        use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
        let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        let ok = unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm) }.as_bool();
        // 0 и 1 — «частота по умолчанию железа», не число герц
        (ok && dm.dmDisplayFrequency > 1).then(|| 1.0 / dm.dmDisplayFrequency as f64)
    })
}

/// Не успевает ли игра к обновлению экрана: средний интервал между Present
/// больше периода монитора. Тогда vsync держал бы каждый кадр ещё период
/// (16.6 мс на 60 Гц) — показываем сразу, с разрывом, как адаптивный vsync
/// у NVIDIA. Успевающая игра идёт с vsync как раньше.
/// ALKASH3D_VSYNC_ADAPTIVE=0 — выкл.
/// ИСПРАВЛЕНО: решение по одному интервалу срабатывало ложно — интервал на
/// CPU дрожит на ±1 мс и при ровных 60 Гц, и половина кадров рвалась зря.
/// Среднее (экспоненциальное, ~10 кадров) дрожание гасит: при ровных 60 Гц
/// оно ровно период, а с разрывами — время кадра GPU, так что с быстрой
/// сцены игра сама возвращается к vsync.
fn present_late(now: Instant) -> bool {
    thread_local! {
        static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
        static AVG: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let last = LAST.with(|l| l.replace(Some(now)));
    if !*ON.get_or_init(|| std::env::var("ALKASH3D_VSYNC_ADAPTIVE").map(|v| v != "0").unwrap_or(true)) || !crate::swap_chain::tearing_supported() {
        return false;
    }
    let (Some(last), Some(period)) = (last, refresh_period()) else { return false };
    // паузы (загрузка, сворачивание окна) в среднее не берём
    let dt = (now - last).as_secs_f64().min(period * 3.0);
    let avg = AVG.with(|a| {
        let v = if a.get() == 0.0 { dt } else { a.get() + (dt - a.get()) * 0.1 };
        a.set(v);
        v
    });
    avg > period * 1.02
}

fn perf_frame(wait_ms: f64, record_ms: f64, present_ms: f64, late: bool, main_draws: usize, main_tris: u64, csm_draws: usize, local_draws: usize) {
    let mut g = PERF.lock().unwrap();
    let a = g.get_or_insert_with(PerfAcc::default);
    let now = Instant::now();
    let since = *a.since.get_or_insert(now);
    a.frames += 1;
    a.wait_ms += wait_ms;
    a.record_ms += record_ms;
    a.present_ms += present_ms;
    a.late_frames += late as u32;
    a.main_draws += main_draws as u64;
    a.main_tris += main_tris;
    a.csm_draws += csm_draws as u64;
    a.local_shadow_draws += local_draws as u64;
    let el = (now - since).as_secs_f64();
    if el >= 1.0 {
        let n = a.frames as f64;
        println!(
            "[PERF] {:.1} FPS | кадр {:.1} мс: запись CPU {:.1}, ожидание GPU {:.1} (fence) + {:.1} (Present), опоздавших к vsync {} | основной: {:.0} draw, {:.2} млн тр. | тени солнца: {:.0} draw | тени фонарей: {:.0} draw",
            n / el, el * 1000.0 / n, a.record_ms / n, a.wait_ms / n, a.present_ms / n, a.late_frames,
            a.main_draws as f64 / n, a.main_tris as f64 / n / 1e6, a.csm_draws as f64 / n, a.local_shadow_draws as f64 / n
        );
        if let Some(gpu) = super::gpu_timer::report() {
            println!("[PERF]   {}", gpu);
        }
        *a = PerfAcc { since: Some(now), ..Default::default() };
    }
}

impl AlkashEngine {

    /// ИСПРАВЛЕНО (краш exit code 2173 / 0x87d после ~200 кадров, ловилось
    /// только БЕЗ GPU-Based Validation): все `ensure_*_capacity` ниже при
    /// пересоздании буфера делали `self.some_buffer = Some(new_buffer)`,
    /// что дропает старый `ID3D12Resource` НЕМЕДЛЕННО, отдавая его память
    /// обратно. `render_frame` ждёт fence только ТЕКУЩЕГО frame_index
    /// (двойная буферизация слотов ВНУТРИ буфера) — но пересоздание рвёт
    /// буфер целиком, под ОБОИМИ слотами сразу, включая слот "чужого"
    /// frame_index, чей command list мог быть отправлен на GPU в прошлом
    /// кадре и ещё не завершиться (fence для него не проверялся в этом
    /// вызове). Если GPU в этот момент ещё читает старый буфер —
    /// classic use-after-free на GPU-стороне: без diagnostic-сообщений,
    /// просто обрыв процесса (структурное исключение из драйвера/рантайма,
    /// не наш Rust-код). GPU-Based Validation резко замедляет GPU-команды
    /// и почти всегда даёт GPU закончить читать старый буфер раньше, чем
    /// CPU успевает его пересоздать — маскирует гонку, а не убирает её.
    ///
    /// Фикс: перед ЛЮБЫМ пересозданием (не первым выделением с нуля —
    /// тогда старого буфера просто нет) дожидаемся МАКСИМАЛЬНОГО из двух
    /// `frame_fence_values` — то есть гарантируем, что GPU закончил ОБА
    /// in-flight кадра, а не только текущий, прежде чем отдать память
    /// старого буфера.
    ///
    /// ВЕРИФИЦИРОВАНО (2026-09-11): 5 прогонов `main_car.exe` подряд (без
    /// GBV, живое управление, форсированный рост буфера — стартовая ёмкость
    /// `ensure_constant_buffer_capacity` была временно занижена на время
    /// теста до `max(4)` вместо `max(64)`, чтобы гарантировать пересоздание
    /// буфера уже на первом кадре вместо ожидания естественного роста
    /// сцены), все 5 — чистый `[ENGINE] Shutdown complete` без единого
    /// краша/DRED-сообщения. Краш exit code 2173/0x87D, ради которого этот
    /// вызов был добавлен, не воспроизвёлся — баг считается закрытым.
    pub(super) fn wait_for_all_frames_idle_before_realloc(&self) {
        if let Ok(fence) = crate::get_fence() {
            let target = self.frame_fence_values.iter().copied().max().unwrap_or(0);
            if let Err(reason) = wait_for_fence(&fence, target, std::time::Duration::from_secs(5)) {
                eprintln!("[ENGINE] wait_for_all_frames_idle_before_realloc: {} — продолжаем пересоздание буфера рискованно", reason);
            }
        }
    }

    /// Гарантирует, что константный буфер вмещает как минимум
    /// `needed_per_frame` слотов трансформаций НА КАЖДЫЙ из двух back
    /// buffer'ов (итого выделяется `needed_per_frame * 2` слотов).
    /// Пересоздаёт буфер, если текущей ёмкости не хватает (например,
    /// сцена выросла — добавили ещё кубов в сетку пола).
    ///
    /// Буфер удваивается на оба back buffer'а по той же причине, по
    /// которой у нас уже два `command allocator`'а: пока GPU дорисовывает
    /// кадр N (frame_index k), CPU уже готовит кадр N+1 (frame_index
    /// 1-k). Если бы оба кадра писали в одни и те же слоты одного и того
    /// же буфера — это была бы гонка данных между CPU, пишущим новый
    /// кадр, и GPU, всё ещё читающим предыдущий. Слот для конкретного
    /// кадра выбирается как `frame_index * capacity + i` — см.
    /// `render_frame`.
    pub(super) fn ensure_constant_buffer_capacity(&mut self, needed_per_frame: usize) -> Result<()> {
        if self.constant_buffer.is_some() && needed_per_frame <= self.constant_buffer_capacity {
            return Ok(());
        }
        if self.constant_buffer.is_some() {
            self.wait_for_all_frames_idle_before_realloc();
        }

        let new_capacity = needed_per_frame.max(1024).next_power_of_two();
        let total_slots = new_capacity * 2;
        let buffer = Buffer::create_constant_buffer_array(TransformConstants::aligned_size(), total_slots)?;
        println!(
            "[ENGINE] Constant buffer (re)allocated: {} slots/кадр x2 = {} слотов",
            new_capacity, total_slots
        );
        self.constant_buffer = Some(buffer);
        self.constant_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): тот же
    /// паттерн роста, что и `ensure_constant_buffer_capacity` выше, но для
    /// отдельного `shadow_constant_buffer` (см. `constant_buffer::ShadowConstants`)
    /// — shadow-проход рисует ТЕ ЖЕ объекты кадра, поэтому нуждается в
    /// ровно таком же количестве слотов, просто в СВОЁМ буфере (другой
    /// layout данных, другая root signature).
    ///
    /// ИСПРАВЛЕНО (Cascaded Shadow Maps — переполнение буфера): `caller`
    /// (render_frame) передаёт `needed_per_frame = shadow_jobs.len() *
    /// NUM_CASCADES` — то есть `shadow_constant_buffer_capacity` ниже
    /// хранит ёмкость на ОДИН ПОЛНЫЙ кадр (все каскады сразу), а формула
    /// слота в render_frame — `(frame_index * NUM_CASCADES + cascade) *
    /// shadow_constant_buffer_capacity + i` — умножает `capacity` НА
    /// (frame_index * NUM_CASCADES + cascade), а НЕ просто на frame_index.
    /// Раньше (до CSM, один каскад) буфер выделялся как `capacity * 2`
    /// (x2 только на frame_index) — теперь этого катастрофически не
    /// хватает: слот для cascade=2 при frame_index=1 обращается далеко ЗА
    /// пределы буфера (undefined behaviour/GPU crash). Нужно выделять
    /// `capacity`, умноженную на ПОЛНОЕ число независимых блоков —
    /// `2 (frame_index) * NUM_CASCADES` — а не на 2.
    fn ensure_shadow_constant_buffer_capacity(&mut self, needed_per_frame: usize) -> Result<()> {
        if self.shadow_constant_buffer.is_some() && needed_per_frame <= self.shadow_constant_buffer_capacity {
            return Ok(());
        }
        if self.shadow_constant_buffer.is_some() {
            self.wait_for_all_frames_idle_before_realloc();
        }

        let new_capacity = needed_per_frame.max(1024).next_power_of_two();
        let total_slots = new_capacity * 2 * NUM_CASCADES;
        let buffer = Buffer::create_constant_buffer_array(crate::constant_buffer::ShadowConstants::aligned_size(), total_slots)?;
        println!(
            "[ENGINE] Shadow constant buffer (re)allocated: {} slots/(кадр*каскад) x2 x{} каскада = {} слотов",
            new_capacity, NUM_CASCADES, total_slots
        );
        self.shadow_constant_buffer = Some(buffer);
        self.shadow_constant_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (тени фонарей): слоты ShadowConstants для отрисовки
    /// геометрии в плитки spot-атласа. `needed_per_frame` — суммарное
    /// число draw'ов по всем плиткам кадра; слот = frame_index * capacity + k.
    /// Как и все остальные ensure_*_capacity, ОБЯЗАН дождаться простоя
    /// GPU перед пересозданием (см. комментарий в начале impl — краш 0x87D).
    fn ensure_spot_shadow_constant_buffer_capacity(&mut self, needed_per_frame: usize) -> Result<()> {
        if self.spot_shadow_constant_buffer.is_some() && needed_per_frame <= self.spot_shadow_constant_buffer_capacity {
            return Ok(());
        }
        if self.spot_shadow_constant_buffer.is_some() {
            self.wait_for_all_frames_idle_before_realloc();
        }

        let new_capacity = needed_per_frame.max(4096).next_power_of_two();
        let total_slots = new_capacity * 2;
        let buffer = Buffer::create_constant_buffer_array(crate::constant_buffer::ShadowConstants::aligned_size(), total_slots)?;
        println!(
            "[ENGINE] Spot shadow constant buffer (re)allocated: {} slots/кадр x2 = {} слотов",
            new_capacity, total_slots
        );
        self.spot_shadow_constant_buffer = Some(buffer);
        self.spot_shadow_constant_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (тени фонарей): записывает в cmd_list depth-only отрисовку
    /// одного меша (VB/IB + Draw) — общая часть для плиток spot-атласа.
    /// ДОБАВЛЕНО (тени листвы): нужен ли мешу теневой PSO с альфа-вырезом.
    fn shadow_needs_cutout(&self, mesh: &super::Mesh) -> bool {
        mesh.alpha_cutoff > 0.0 && mesh.albedo_srv_index.is_some() && self.shadow_srv_heap.is_some()
    }

    /// ДОБАВЛЕНО (тени листвы): переключает теневой PSO под меш — с альфа-вырезом
    /// (PS отбрасывает прозрачные пиксели текстуры) или прежний depth-only.
    unsafe fn shadow_bind_material(
        &self,
        cmd_list: &ID3D12GraphicsCommandList,
        mesh: &super::Mesh,
        base: &ID3D12PipelineState,
        cutout: Option<&ID3D12PipelineState>,
        cutout_on: &mut bool,
        srv_descriptor_size: u32,
    ) {
        let want = cutout.is_some() && self.shadow_needs_cutout(mesh);
        unsafe {
            if want != *cutout_on {
                cmd_list.SetPipelineState(if want { cutout } else { Some(base) });
                *cutout_on = want;
            }
            if want {
                let heap = self.shadow_srv_heap.as_ref().unwrap();
                let handle = crate::heap::DescriptorHeap::get_gpu_handle(heap, mesh.albedo_srv_index.unwrap(), srv_descriptor_size);
                cmd_list.SetGraphicsRootDescriptorTable(1, handle);
                cmd_list.SetGraphicsRoot32BitConstants(2, 1, &mesh.alpha_cutoff as *const f32 as *const _, 0);
            }
        }
    }

    unsafe fn record_depth_only_draw(cmd_list: &ID3D12GraphicsCommandList, mesh: &super::Mesh) {
        unsafe {
            let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                SizeInBytes: mesh.vertex_buffer.size as u32,
                StrideInBytes: Vertex::STRIDE,
            };
            cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
            if let Some(index_buffer) = &mesh.index_buffer {
                let index_view = D3D12_INDEX_BUFFER_VIEW {
                    BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                    SizeInBytes: index_buffer.size as u32,
                    Format: DXGI_FORMAT_R32_UINT,
                };
                cmd_list.IASetIndexBuffer(Some(&index_view));
                cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
            } else {
                cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
            }
        }
    }

    /// ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): гарантирует, что
    /// `light_buffer` вмещает как минимум `needed` элементов `GPULight`.
    /// Тот же паттерн роста, что и у `ensure_constant_buffer_capacity`
    /// (степень двойки, минимум разумного стартового размера) — растёт по
    /// требованию, а не выделяется на весь возможный максимум сразу,
    /// потому что реальное число видимых после каллинга фонарей в кадре
    /// обычно НАМНОГО меньше total_lights (это и есть весь смысл каллинга).
    fn ensure_light_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        if self.light_buffer.is_some() && needed <= self.light_buffer_capacity {
            return Ok(());
        }
        if self.light_buffer.is_some() {
            self.wait_for_all_frames_idle_before_realloc();
        }

        let new_capacity = needed.max(64).next_power_of_two();
        let size_bytes = new_capacity as u64 * std::mem::size_of::<GPULight>() as u64;
        let buffer = Buffer::create_structured_buffer(size_bytes)?;
        println!(
            "[ENGINE] Light buffer (re)allocated: {} GPULight слотов ({} байт)",
            new_capacity, size_bytes
        );
        self.light_buffer = Some(buffer);
        self.light_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): гарантирует, что
    /// `grid_cells_buffer` вмещает РОВНО `needed` ячеек. В отличие от
    /// `ensure_light_buffer_capacity`/`ensure_grid_entries_buffer_capacity`,
    /// здесь НЕТ роста "с запасом" (`next_power_of_two`) — общее число
    /// ячеек сетки в FirstFires фиксировано на весь срок жизни плагина
    /// (задаётся один раз в LightConfig), пересоздание буфера при этом
    /// размере НЕ происходит на каждый кадр (проверка `needed ==
    /// capacity`, а не `needed <= capacity`, чтобы не тратить память под
    /// "запас", который никогда не понадобится для этого буфера).
    fn ensure_grid_cells_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        if self.grid_cells_buffer.is_some() && needed == self.grid_cells_buffer_capacity {
            return Ok(());
        }
        if self.grid_cells_buffer.is_some() {
            self.wait_for_all_frames_idle_before_realloc();
        }
        let size_bytes = needed.max(1) as u64 * std::mem::size_of::<LightGridCell>() as u64;
        let buffer = Buffer::create_structured_buffer(size_bytes)?;
        println!(
            "[ENGINE] Grid cells buffer (re)allocated: {} ячеек ({} байт)",
            needed, size_bytes
        );
        self.grid_cells_buffer = Some(buffer);
        self.grid_cells_buffer_capacity = needed;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): то же самое, что
    /// `ensure_light_buffer_capacity`, но для `grid_entries_buffer` —
    /// число entries меняется каждый кадр (зависит от того, сколько
    /// фонарей реально видимо), поэтому растёт степенями двойки, а не
    /// фиксировано, как `grid_cells_buffer`.
    fn ensure_grid_entries_buffer_capacity(&mut self, needed: usize) -> Result<()> {
        if self.grid_entries_buffer.is_some() && needed <= self.grid_entries_buffer_capacity {
            return Ok(());
        }
        if self.grid_entries_buffer.is_some() {
            self.wait_for_all_frames_idle_before_realloc();
        }
        let new_capacity = needed.max(64).next_power_of_two();
        let size_bytes = new_capacity as u64 * std::mem::size_of::<LightGridEntry>() as u64;
        let buffer = Buffer::create_structured_buffer(size_bytes)?;
        println!(
            "[ENGINE] Grid entries buffer (re)allocated: {} слотов ({} байт)",
            new_capacity, size_bytes
        );
        self.grid_entries_buffer = Some(buffer);
        self.grid_entries_buffer_capacity = new_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): вспомогательная
    /// функция для transition-барьера — до этой фазы движок вообще не
    /// вызывал `ResourceBarrier` (рисовал прямо в back buffer без явных
    /// переходов состояния, что формально некорректно по спецификации
    /// D3D12, хоть и "работало" на многих драйверах). Теперь, когда
    /// появился HDR render target с полноценным циклом состояний
    /// (RENDER_TARGET во время draw pass -> PIXEL_SHADER_RESOURCE во время
    /// чтения в composite pass -> обратно в RENDER_TARGET для следующего
    /// кадра), обойтись без барьеров уже не получится — без них GPU не
    /// гарантированно видит корректные данные (кэши/порядок записи-чтения
    /// не синхронизированы).
    ///
    /// `pResource: ManuallyDrop<Option<ID3D12Resource>>` (см.
    /// `D3D12_RESOURCE_TRANSITION_BARRIER` в windows-крейте) — тот же COM
    /// refcounting паттерн, что уже встречался в `pso.rs` для
    /// `pRootSignature`: клонируем ресурс (это увеличивает refcount на 1),
    /// поэтому обязаны сами явно уменьшить его обратно после того, как
    /// барьер отработал — см. `ManuallyDrop::drop` сразу после
    /// `ResourceBarrier` в местах вызова.
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

    /// Освобождает лишнюю ссылку на ресурс внутри барьера, добавленную
    /// клонированием в `transition_barrier` — см. объяснение там же.
    ///
    /// ИСПРАВЛЕНО (ResizeBuffers -> DXGI_ERROR_INVALID_CALL после первого
    /// же кадра, окно не переживало разворачивание): раньше здесь
    /// дропался только внешний `ManuallyDrop<D3D12_RESOURCE_TRANSITION_BARRIER>`
    /// — но поле `pResource` внутри САМО обёрнуто в `ManuallyDrop`, и
    /// drop структуры его не трогает. Ссылка от `resource.clone()` не
    /// освобождалась никогда: каждый кадр back buffer'ы копили +1 к
    /// refcount, и swap chain отказывался ресайзиться, пока на буферы
    /// есть ссылки. Дропаем именно `pResource`.
    unsafe fn drop_transition_barrier(mut barrier: D3D12_RESOURCE_BARRIER) {
        unsafe {
            let transition = &mut *barrier.Anonymous.Transition;
            std::mem::ManuallyDrop::drop(&mut transition.pResource);
        }
    }

    /// ДОБАВЛЕНО (максимальная графика — LOD для мешей): если `mesh_index`
    /// — ключ зарегистрированной LOD-группы (см. `AlkashEngine::add_lod_group`
    /// в mesh_api.rs), возвращает индекс уровня детализации, подходящего
    /// для расстояния от `camera_pos` до мировой позиции объекта (мировая
    /// origin-точка модельной матрицы — не требует одинаковых
    /// bounding-сфер у разных уровней, "достаточно точно" для порогов
    /// LOD), либо `None`, если объект дальше самого дальнего уровня (не
    /// рисуем вовсе в этом кадре — ни в main pass, ни в shadow pass).
    /// Для НЕ-LOD мешей (подавляющее большинство — обычный случай) —
    /// честный no-op, `Some(mesh_index)` без изменений.
    fn resolve_lod_mesh_index(&self, mesh_index: usize, world: Mat4, camera_pos: Vec3) -> Option<usize> {
        match self.lod_groups.get(&mesh_index) {
            Some(group) => {
                let world_pos = world.transform_point3(Vec3::ZERO);
                group.resolve((world_pos - camera_pos).length())
            }
            None => Some(mesh_index),
        }
    }

    /// ДОБАВЛЕНО как попытка фикса `DXGI_ERROR_DEVICE_HUNG` на первом кадре
    /// (см. `warm_up_pipelines` ниже) — НЕ оказалось причиной зависания, но
    /// оставлено как полезная само по себе вещь. Замер показывал, что первый
    /// кадр считается на GPU ~2.5с (вплотную к таймауту TDR ~2с), и гипотеза
    /// была: ШЕСТЬ разных PSO впервые используются все сразу в одном
    /// `ExecuteCommandLists`, драйвер компилирует их машинный код по факту
    /// первого использования. Разнесение shadow/main по отдельным
    /// submission'ам зависание НЕ убрало (настоящая причина — устаревшие
    /// depth SRV / bloom / volumetric после ресайза окна, см. `handle_resize`
    /// в window.rs), однако сам по себе разогрев остаётся разумным: он
    /// действительно снимает первый, самый дорогой кадр с игрового цикла и
    /// печатает его реальную стоимость.
    ///
    /// В обычном кадре (`self.warm_up_mode == false`) — no-op, возвращает
    /// тот же `cmd_list` без единого лишнего вызова: ноль влияния на
    /// обычную производительность. В режиме прогрева (только ОДИН раз,
    /// внутри `warm_up_pipelines`) — закрывает и отправляет накопленный
    /// `cmd_list` на GPU, дожидается реального завершения (щедрый
    /// таймаут — сама суть проблемы в том, что первый раз это может
    /// занять заметно больше обычного кадрового бюджета) и возвращает
    /// свежий command list на том же аллокаторе, чтобы вызывающий код мог
    /// продолжить запись следующего прохода как ни в чём не бывало.
    fn maybe_flush_for_warm_up(
        &self,
        cmd_list: ID3D12GraphicsCommandList,
        allocator: &ID3D12CommandAllocator,
        label: &str,
    ) -> Result<ID3D12GraphicsCommandList> {
        if !self.warm_up_mode {
            return Ok(cmd_list);
        }
        unsafe {
            cmd_list.Close()?;
            let queue = crate::get_command_queue()?;
            let cmd_lists: &[Option<ID3D12CommandList>] = &[Some(cmd_list.into())];
            queue.ExecuteCommandLists(cmd_lists);
            let fence = crate::get_fence()?;
            let fence_value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
            queue.Signal(&fence, fence_value)?;
            let t0 = std::time::Instant::now();
            if let Err(reason) = wait_for_fence(&fence, fence_value, std::time::Duration::from_secs(20)) {
                eprintln!("[WARMUP] '{}' не прогрелся за 20с: {} (elapsed {:?})", label, reason, t0.elapsed());
                crate::dump_d3d12_debug_messages();
                crate::dump_dred_report();
                return Err(Error::from_hresult(HRESULT(1)));
            }
            println!("[WARMUP] '{}' прогрет за {:?}", label, t0.elapsed());
            allocator.Reset()?;
            let device = crate::get_device()?;
            let new_list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, None)?;
            Ok(new_list)
        }
    }

    /// Разогрев конвейеров (НЕ фикс DEVICE_HUNG — тот оказался в
    /// `handle_resize`, см. `maybe_flush_for_warm_up` выше): вызвать РОВНО
    /// ОДИН раз, после
    /// того как сцена уже содержит хотя бы один видимый объект (иначе
    /// main/shadow PSO не получат ни одного реального Draw-вызова и не
    /// прогреются), и ДО первого обычного вызова `render_frame()` /
    /// входа в игровой цикл. Технически это просто ОДИН настоящий кадр
    /// (расходует один `NEXT_FENCE_VALUE`/frame_index как обычно — не
    /// "лишний" холостой кадр), просто с принудительными точками сброса
    /// внутри.
    pub fn warm_up_pipelines(&mut self) -> Result<bool> {
        println!("[WARMUP] Прогрев PSO перед входом в render loop (первый кадр может занять заметно дольше обычного)...");
        self.warm_up_mode = true;
        let result = self.render_frame();
        self.warm_up_mode = false;
        result
    }

    /// ДОБАВЛЕНО: ближайшая точка сферы меша — не дальше его `max_draw_distance` от камеры.
    fn within_draw_distance(&self, mesh_index: usize, model: &Mat4, camera_pos: Vec3) -> bool {
        let mesh = &self.meshes[mesh_index];
        if !mesh.max_draw_distance.is_finite() {
            return true;
        }
        let (scale, _r, _t) = model.to_scale_rotation_translation();
        let max_scale = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
        let c = model.transform_point3(Vec3::new(mesh.bounding_center[0], mesh.bounding_center[1], mesh.bounding_center[2]));
        (c - camera_pos).length() - mesh.bounding_radius * max_scale <= mesh.max_draw_distance
    }

    pub fn render_frame(&mut self) -> Result<bool> {
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: render_frame() called but renderer is not initialized");
            Error::from_hresult(HRESULT(1))
        })?;
        // ДОБАВЛЕНО (TAAU): размер кадра и экрана — копии, без заимствования renderer
        let (render_w, render_h) = (renderer.render_width, renderer.render_height);
        let upscaling = renderer.hdr_upscaled.is_some();

        let real_back_buffer_index = {
            let state = STATE.lock().unwrap();
            state.frame_index as usize
        };
        // ИСТОРИЯ (2026-09-08, диагностика DXGI_ERROR_DEVICE_HUNG на main_car):
        // здесь временно форсировался `frame_index = 0`, чтобы проверить
        // гипотезу "баг в первом использовании ВТОРОГО double-buffering
        // слота" — зависание воспроизвелось и так, гипотеза опровергнута.
        // Настоящая причина найдена позже бисекцией через
        // `src/bin/example_minimal.rs` и оказалась вообще не здесь:
        // `handle_resize()` в window.rs пересоздавал только `Renderer`, но не
        // depth SRV / bloom / volumetric — см. подробный комментарий там.
        // `real_back_buffer_index` оставлен отдельным именем намеренно: у
        // back buffer'а и у CPU-side слотов (allocator/constant buffer) РАЗНАЯ
        // семантика, даже когда индекс численно совпадает.
        let frame_index = real_back_buffer_index;

        let perf_t0 = Instant::now();
        if let Some(&target) = self.frame_fence_values.get(frame_index) {
            if target > 0 {
                let fence = crate::get_fence()?;
                if let Err(reason) = wait_for_fence(&fence, target, std::time::Duration::from_secs(5)) {
                    eprintln!("[ENGINE] render_frame: {} — прерываем кадр", reason);
                    crate::dump_d3d12_debug_messages();
                    crate::dump_dred_report();
                    return Err(Error::from_hresult(HRESULT(1)));
                }
            }
        }

        let perf_wait_ms = perf_t0.elapsed().as_secs_f64() * 1000.0;
        let perf_t_record = Instant::now();
        let mut perf_csm_draws = 0usize;
        let mut perf_main_draws = 0usize;
        let mut perf_main_tris = 0u64;
        let allocator = CommandList::get_allocator(frame_index)
            .ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

        unsafe {
            allocator.Reset()?;
        }

        let device = crate::get_device()?;

        let mut cmd_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?
        };

        if perf_enabled() {
            super::gpu_timer::begin(frame_index);
            super::gpu_timer::mark(&cmd_list, frame_index, 0);
        }

        let rtv_handle = renderer.hdr_rtv;
        let ambient_rtv = renderer.ambient_rtv;
        let rtv_size = renderer.rtv_size;
        let dsv_handle = renderer.depth_stencil_view;

        // ДОБАВЛЕНО (максимальная графика — LOD для мешей): считаем ОДИН
        // раз на кадр, используется и для shadow_jobs ниже, и для main
        // pass jobs дальше по функции — LOD выбирается по расстоянию до
        // КАМЕРЫ (не до источника света) в обоих проходах, чтобы тень
        // всегда соответствовала уровню детализации видимого объекта, а не
        // жила своей отдельной жизнью.
        let camera_pos_for_lod = self.camera.position;

        let mut shadow_jobs: Vec<(usize, Mat4)> = {
            let mut v: Vec<(usize, Mat4)> = Vec::new();
            if !self.mesh_instances.is_empty() {
                for instance in &self.mesh_instances {
                    if instance.mesh_index < self.meshes.len() {
                        // ДОБАВЛЕНО (прозрачность): полупрозрачное (стекло, дым,
                        // затухающие объекты) тень не отбрасывает.
                        let m = &self.meshes[instance.mesh_index];
                        if instance.tint[3] < 0.999 || m.blend || m.unlit {
                            continue;
                        }
                        let world = instance.transform_matrix();
                        if let Some(idx) = self.resolve_lod_mesh_index(instance.mesh_index, world, camera_pos_for_lod) {
                            v.push((idx, world));
                        }
                    }
                }
            }
            for (mesh_index, world) in self.scene.collect_renderables() {
                if mesh_index < self.meshes.len() {
                    if let Some(idx) = self.resolve_lod_mesh_index(mesh_index, world, camera_pos_for_lod) {
                        v.push((idx, world));
                    }
                }
            }
            // ДОБАВЛЕНО: мелочь дальше своей дистанции отрисовки (Mesh::max_draw_distance) тень не даёт
            v.retain(|(mesh_index, model)| self.within_draw_distance(*mesh_index, model, camera_pos_for_lod));
            v
        };

        let light_dir_vec = Vec3::new(
            self.transform_constants.light_dir[0],
            self.transform_constants.light_dir[1],
            self.transform_constants.light_dir[2],
        );

        let cascade_far_distances: [f32; NUM_CASCADES] = {
            let mut arr = [0.0f32; NUM_CASCADES];
            for i in 0..NUM_CASCADES {
                arr[i] = self.shadow_far.unwrap_or(self.camera.far).min(self.camera.far) * CASCADE_SPLITS[i];
            }
            arr
        };
        let cascade_view_projs: [Mat4; NUM_CASCADES] = {
            let mut arr = [Mat4::IDENTITY; NUM_CASCADES];
            let mut near_dist = self.camera.near;
            for i in 0..NUM_CASCADES {
                let far_dist = cascade_far_distances[i];
                arr[i] = self.compute_cascade_view_proj(light_dir_vec, near_dist, far_dist);
                near_dist = far_dist;
            }
            arr
        };

        // ДОБАВЛЕНО (тени листвы): меши с альфа-вырезом — в конец (стабильно),
        // чтобы PSO переключался на них один раз за проход.
        shadow_jobs.sort_by_key(|(m, _)| self.shadow_needs_cutout(&self.meshes[*m]));
        let srv_descriptor_size = STATE.lock().unwrap().cbv_srv_uav_descriptor_size;

        if self.shadow_pipeline_state.is_some() && self.shadow_root_signature.is_some() {
            if let Err(e) = self.ensure_shadow_constant_buffer_capacity(shadow_jobs.len() * NUM_CASCADES) {
                eprintln!("[ENGINE] WARNING: не удалось выделить shadow_constant_buffer: {:?}", e);
            }

            unsafe {
                cmd_list.SetPipelineState(Some(self.shadow_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.shadow_root_signature.as_ref().unwrap()));
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                if let Some(heap) = &self.shadow_srv_heap {
                    cmd_list.SetDescriptorHeaps(&[Some(heap.clone())]);
                }

                let shadow_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: SHADOW_MAP_RESOLUTION as f32,
                    Height: SHADOW_MAP_RESOLUTION as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                cmd_list.RSSetViewports(&[shadow_viewport]);
                let shadow_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: SHADOW_MAP_RESOLUTION as i32,
                    bottom: SHADOW_MAP_RESOLUTION as i32,
                };
                cmd_list.RSSetScissorRects(&[shadow_scissor]);

                // ДОБАВЛЕНО (20 FPS на карте Самары): раньше КАЖДЫЙ каскад рисовал ВСЕ
                // загруженные меши — весь город вокруг трижды за кадр, без отсечения
                // (основной проход отсекает, а теневой — нет). Теперь меш пропускается,
                // если его сфера целиком вне ортопроекции каскада. Картинка та же:
                // DepthClipEnable в shadow PSO = TRUE (pipeline_shadow.rs), так что
                // отброшенное и раньше обрезалось растеризатором — просто после
                // вершинного шейдера всего города.
                let shadow_spheres: Vec<(Vec3, f32)> = shadow_jobs
                    .iter()
                    .map(|(mesh_index, model)| {
                        let mesh = &self.meshes[*mesh_index];
                        let (scale, _r, _t) = model.to_scale_rotation_translation();
                        let max_scale = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
                        let c = Vec3::new(mesh.bounding_center[0], mesh.bounding_center[1], mesh.bounding_center[2]);
                        (model.transform_point3(c), mesh.bounding_radius * max_scale)
                    })
                    .collect();

                for cascade in 0..NUM_CASCADES {
                    let dsv = self.shadow_dsvs[cascade];
                    cmd_list.OMSetRenderTargets(0, None, false, Some(&dsv));
                    cmd_list.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

                    let light_view_proj = cascade_view_projs[cascade];
                    let cascade_frustum = crate::math::Frustum::from_view_proj(&light_view_proj);

                    // ДОБАВЛЕНО (FPS бакшота): солнце с нулевой яркостью ничего не
                    // освещает, а его каскады всё равно рисовали ~500 draw за кадр.
                    // Карта только очищается (тень = 1) — вклад солнца и так 0.
                    let sun_on = self.transform_constants.light_color[3] > 0.0;
                    let mut cutout_on = false;
                    if let Some(shadow_cb) = self.shadow_constant_buffer.as_ref().filter(|_| sun_on) {
                        for (i, (mesh_index, model)) in shadow_jobs.iter().enumerate() {
                            let (sc, sr) = shadow_spheres[i];
                            if !cascade_frustum.test_sphere(sc, sr) {
                                continue;
                            }
                            perf_csm_draws += 1;
                            let mesh = &self.meshes[*mesh_index];
                            self.shadow_bind_material(&cmd_list, mesh, self.shadow_pipeline_state.as_ref().unwrap(), self.shadow_cutout_pipeline_state.as_ref(), &mut cutout_on, srv_descriptor_size);
                            let mlvp = light_view_proj * (*model);
                            let shadow_constants = crate::constant_buffer::ShadowConstants {
                                model_light_view_proj: mlvp.to_cols_array_2d(),
                            };
                            let slot = (frame_index * NUM_CASCADES + cascade) * self.shadow_constant_buffer_capacity + i;
                            if let Err(e) = shadow_constants.write_at(shadow_cb, slot) {
                                eprintln!("[ENGINE] WARNING: failed to write shadow constant buffer slot {}: {:?}", slot, e);
                                continue;
                            }
                            let gpu_addr = crate::constant_buffer::ShadowConstants::gpu_address_for_slot(shadow_cb, slot);
                            cmd_list.SetGraphicsRootConstantBufferView(0, gpu_addr);

                            let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                                BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                                SizeInBytes: mesh.vertex_buffer.size as u32,
                                StrideInBytes: Vertex::STRIDE,
                            };
                            cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));

                            if let Some(index_buffer) = &mesh.index_buffer {
                                let index_view = D3D12_INDEX_BUFFER_VIEW {
                                    BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                                    SizeInBytes: index_buffer.size as u32,
                                    Format: DXGI_FORMAT_R32_UINT,
                                };
                                cmd_list.IASetIndexBuffer(Some(&index_view));
                                cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
                            } else {
                                cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
                            }
                        }
                    }
                    if cutout_on {
                        cmd_list.SetPipelineState(Some(self.shadow_pipeline_state.as_ref().unwrap()));
                    }
                    if let Some(shadow_map) = &self.shadow_maps[cascade] {
                        if !self.shadow_maps_are_srv[cascade] {
                            let barrier = Self::transition_barrier(
                                &shadow_map.resource,
                                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                            );
                            let barriers = [barrier];
                            cmd_list.ResourceBarrier(&barriers);
                            for b in barriers {
                                Self::drop_transition_barrier(b);
                            }
                            self.shadow_maps_are_srv[cascade] = true;
                        }
                    }
                }
            }
        }

        // ДОБАВЛЕНО (тени фонарей): shadow map для ближайших spot-фонарей
        // (плитка на фонарь) и point-фонарей (6 плиток-граней куба на
        // фонарь) — каждая плитка рисуется из позиции своего фонаря, только
        // геометрия, попавшая в её frustum. Оба атласа очищаются и
        // переводятся в SRV КАЖДЫЙ кадр, даже без фонарей: они всегда
        // забинжены в таблице теней основного прохода и должны находиться в
        // корректном состоянии ресурса.
        if perf_enabled() {
            super::gpu_timer::mark(&cmd_list, frame_index, 1);
        }
        let spot_shadow_lights: Vec<(usize, Mat4)> = self.select_spot_shadow_lights();
        let point_shadow_lights: Vec<(usize, [Mat4; 6])> = self.select_point_shadow_lights();
        let (spot_shadow_lights, point_shadow_lights) = {
            let lights = self.get_gpu_lights();
            LOCAL_SHADOW_CACHE.with(|c| {
                let mut c = c.borrow_mut();
                let s = stable_slots(spot_shadow_lights, lights, &mut c.spot_keys);
                let p = stable_slots(point_shadow_lights, lights, &mut c.point_keys);
                (s, p)
            })
        };

        // (атлас: 0 = spot, 1 = point; x; y; размер плитки; view-proj)
        let mut shadow_tiles: Vec<(usize, u32, u32, u32, Mat4)> = Vec::new();
        for (tile, (_, vp)) in spot_shadow_lights.iter().enumerate() {
            let t = tile as u32;
            let r = SPOT_SHADOW_TILE_RESOLUTION;
            shadow_tiles.push((0, (t % SPOT_SHADOW_TILES_PER_ROW) * r, (t / SPOT_SHADOW_TILES_PER_ROW) * r, r, *vp));
        }
        for (slot, (_, face_vps)) in point_shadow_lights.iter().enumerate() {
            for (face, vp) in face_vps.iter().enumerate() {
                let t = (slot * 6 + face) as u32;
                let r = POINT_SHADOW_TILE_RESOLUTION;
                shadow_tiles.push((1, (t % POINT_SHADOW_TILES_PER_ROW) * r, (t / POINT_SHADOW_TILES_PER_ROW) * r, r, *vp));
            }
        }

        let tile_jobs: Vec<Vec<usize>> = shadow_tiles
            .iter()
            .map(|(_, _, _, _, vp)| {
                let tile_frustum = crate::math::Frustum::from_view_proj(vp);
                shadow_jobs
                    .iter()
                    .enumerate()
                    .filter(|(_, (mesh_index, model))| {
                        let mesh = &self.meshes[*mesh_index];
                        let (scale, _r, _t) = model.to_scale_rotation_translation();
                        let max_scale = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
                        let center = model.transform_point3(Vec3::new(
                            mesh.bounding_center[0],
                            mesh.bounding_center[1],
                            mesh.bounding_center[2],
                        ));
                        tile_frustum.test_sphere(center, mesh.bounding_radius * max_scale)
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .collect();
        // кэш: атлас пересоздан (ресайз) — всё заново; иначе перерисовываются только плитки
        // с изменившимся отпечатком
        // ДОБАВЛЕНО (25 FPS в подъезде: тело игрока попадало в кубы теней ламп,
        // и плитка перерисовывалась целиком, со всем подъездом, каждый кадр —
        // 7.5 мс): движущиеся меши (`Mesh::moving`) рисуются поверх кэша статики.
        // У атласа, где они появились, есть копия «только статика»
        // (`local_shadow_static`). Плитка, где сменилось только движущееся,
        // восстанавливается из неё (shadow_restore.rs — переносится одна
        // плитка, а не атлас целиком) и движущееся дорисовывается; где
        // сменилась статика — статика перерисовывается, и копия обновляется
        // (тогда же восстанавливаются плитки с движущимся, чтобы оно не попало
        // в копию). Атлас, где движущегося не бывало, — как раньше.
        if self.shadow_restore.is_none() {
            self.shadow_restore = Some(super::shadow_restore::ShadowRestore::new().map_err(|e| eprintln!("[ENGINE] WARNING: перенос кэша теней недоступен: {:?}", e)));
        }
        let restore_ok = matches!(self.shadow_restore, Some(Ok(_)));
        let is_moving = |job: usize| self.meshes[shadow_jobs[job].0].moving;
        let tile_moving: Vec<bool> = tile_jobs.iter().map(|jobs| restore_ok && jobs.iter().any(|&j| is_moving(j))).collect();
        let (tile_dirty, tile_static_dirty, moving_mode): (Vec<bool>, Vec<bool>, [bool; 2]) = {
            use windows::core::Interface;
            let ids = (
                self.spot_shadow_atlas.as_ref().map(|a| a.resource.as_raw() as usize).unwrap_or(0),
                self.point_shadow_atlas.as_ref().map(|a| a.resource.as_raw() as usize).unwrap_or(0),
            );
            LOCAL_SHADOW_CACHE.with(|c| {
                let mut c = c.borrow_mut();
                if c.atlas_ids != ids {
                    c.atlas_ids = ids;
                    c.tile_hash.clear();
                    c.static_hash.clear();
                    c.has_moving.clear();
                }
                for kind in 0..2usize {
                    if !c.moving_mode[kind] && shadow_tiles.iter().zip(&tile_moving).any(|((k, ..), m)| *k == kind && *m) {
                        // копия статики ещё пуста — первый раз статика рисуется во всех плитках
                        c.moving_mode[kind] = true;
                        c.static_hash.retain(|(k, _), _| *k != kind);
                        c.tile_hash.retain(|(k, _), _| *k != kind);
                        c.has_moving.retain(|(k, _)| *k != kind);
                    }
                }
                let mut dirty = Vec::with_capacity(shadow_tiles.len());
                let mut static_dirty = Vec::with_capacity(shadow_tiles.len());
                for ((kind, tx, ty, _r, vp), jobs) in shadow_tiles.iter().zip(tile_jobs.iter()) {
                    let key = (*kind, (tx << 16) | ty);
                    let fp = tile_fingerprint(vp, jobs, &shadow_jobs);
                    dirty.push(c.tile_hash.insert(key, fp) != Some(fp));
                    static_dirty.push(c.moving_mode[*kind] && {
                        let statics: Vec<usize> = jobs.iter().copied().filter(|&j| !is_moving(j)).collect();
                        let sfp = tile_fingerprint(vp, &statics, &shadow_jobs);
                        c.static_hash.insert(key, sfp) != Some(sfp)
                    });
                }
                (dirty, static_dirty, c.moving_mode)
            })
        };
        // по атласу: обновить копию статики (в атласе сменилась статика)
        let copy_out: [bool; 2] = std::array::from_fn(|kind| moving_mode[kind] && shadow_tiles.iter().zip(&tile_static_dirty).any(|((k, ..), d)| *k == kind && *d));
        // по плитке: (восстановить из копии; очистить и нарисовать статику до
        // обновления копии; дорисовать движущееся после)
        let tile_draws: Vec<(bool, Option<Vec<usize>>, Vec<usize>)> = LOCAL_SHADOW_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            shadow_tiles
                .iter()
                .enumerate()
                .map(|(t, (kind, tx, ty, ..))| {
                    let jobs = &tile_jobs[t];
                    if !moving_mode[*kind] {
                        return (false, tile_dirty[t].then(|| jobs.clone()), Vec::new());
                    }
                    let key = (*kind, (tx << 16) | ty);
                    let restore = !tile_static_dirty[t] && (tile_dirty[t] || (copy_out[*kind] && c.has_moving.contains(&key)));
                    let statics = tile_static_dirty[t].then(|| jobs.iter().copied().filter(|&j| !is_moving(j)).collect());
                    let moving: Vec<usize> = if restore || tile_static_dirty[t] { jobs.iter().copied().filter(|&j| is_moving(j)).collect() } else { Vec::new() };
                    if restore || tile_static_dirty[t] {
                        if moving.is_empty() {
                            c.has_moving.remove(&key);
                        } else {
                            c.has_moving.insert(key);
                        }
                    }
                    (restore, statics, moving)
                })
                .collect()
        });
        for kind in 0..2usize {
            if moving_mode[kind] && self.local_shadow_static[kind].is_none() {
                let res = if kind == 0 { super::SPOT_SHADOW_ATLAS_RESOLUTION } else { super::POINT_SHADOW_ATLAS_RESOLUTION };
                match crate::render::RenderTexture::create_shadow_map(res) {
                    Ok(t) => unsafe {
                        // между кадрами — в PIXEL_SHADER_RESOURCE: её читает перенос плиток
                        let barriers = [Self::transition_barrier(&t.resource, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)];
                        cmd_list.ResourceBarrier(&barriers);
                        for b in barriers {
                            Self::drop_transition_barrier(b);
                        }
                        self.local_shadow_static[kind] = Some(t.resource.clone());
                    },
                    Err(e) => eprintln!("[ENGINE] WARNING: кэш статики теней фонарей не создан: {:?}", e),
                }
            }
        }
        let total_local_shadow_draws: usize = tile_draws.iter().map(|(_, a, b)| a.as_ref().map_or(0, |a| a.len()) + b.len()).sum();
        let can_draw_local_shadows = total_local_shadow_draws > 0
            && self.spot_shadow_pipeline_state.is_some()
            && self.shadow_root_signature.is_some()
            && match self.ensure_spot_shadow_constant_buffer_capacity(total_local_shadow_draws) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось выделить spot_shadow_constant_buffer: {:?}", e);
                    false
                }
            };

        unsafe {
            let mut draw_k = 0usize;
            // копия глубины целиком: dst <- src, оба возвращаются в свои состояния
            let copy = |dst: &ID3D12Resource, dst_state: D3D12_RESOURCE_STATES, src: &ID3D12Resource, src_state: D3D12_RESOURCE_STATES| {
                let mut to = vec![Self::transition_barrier(dst, dst_state, D3D12_RESOURCE_STATE_COPY_DEST)];
                if src_state != D3D12_RESOURCE_STATE_COPY_SOURCE {
                    to.push(Self::transition_barrier(src, src_state, D3D12_RESOURCE_STATE_COPY_SOURCE));
                }
                cmd_list.ResourceBarrier(&to);
                for b in to {
                    Self::drop_transition_barrier(b);
                }
                cmd_list.CopyResource(dst, src);
                let mut back = vec![Self::transition_barrier(dst, D3D12_RESOURCE_STATE_COPY_DEST, dst_state)];
                if src_state != D3D12_RESOURCE_STATE_COPY_SOURCE {
                    back.push(Self::transition_barrier(src, D3D12_RESOURCE_STATE_COPY_SOURCE, src_state));
                }
                cmd_list.ResourceBarrier(&back);
                for b in back {
                    Self::drop_transition_barrier(b);
                }
            };
            for atlas_kind in 0..2usize {
                let (dsv, atlas_res) = if atlas_kind == 0 {
                    (self.spot_shadow_dsv, self.spot_shadow_atlas.as_ref().map(|a| a.resource.clone()))
                } else {
                    (self.point_shadow_dsv, self.point_shadow_atlas.as_ref().map(|a| a.resource.clone()))
                };
                let Some(atlas_res) = atlas_res else { continue };
                let static_res = self.local_shadow_static[atlas_kind].clone();
                let use_cache = moving_mode[atlas_kind] && can_draw_local_shadows && static_res.is_some();
                cmd_list.OMSetRenderTargets(0, None, false, Some(&dsv));
                if use_cache {
                    let restore: Vec<RECT> = shadow_tiles
                        .iter()
                        .zip(tile_draws.iter())
                        .filter(|((kind, ..), (r, ..))| *kind == atlas_kind && *r)
                        .map(|((_, tx, ty, r, _), _)| RECT { left: *tx as i32, top: *ty as i32, right: (*tx + *r) as i32, bottom: (*ty + *r) as i32 })
                        .collect();
                    if let Some(Ok(sr)) = self.shadow_restore.as_mut() {
                        sr.record(&cmd_list, atlas_kind, static_res.as_ref().unwrap(), &restore);
                    }
                }
                // очищаем только перерисовываемые плитки — остальные хранят верную глубину
                let rects: Vec<RECT> = shadow_tiles
                    .iter()
                    .zip(tile_draws.iter())
                    .filter(|((kind, ..), (_, before, _))| *kind == atlas_kind && before.is_some())
                    .map(|((_, tx, ty, r, _), _)| RECT { left: *tx as i32, top: *ty as i32, right: (*tx + *r) as i32, bottom: (*ty + *r) as i32 })
                    .collect();
                if !rects.is_empty() {
                    cmd_list.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, Some(&rects));
                }

                if !can_draw_local_shadows {
                    continue;
                }
                let Some(local_cb) = &self.spot_shadow_constant_buffer else { continue };
                cmd_list.SetPipelineState(Some(self.spot_shadow_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.shadow_root_signature.as_ref().unwrap()));
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                if let Some(heap) = &self.shadow_srv_heap {
                    cmd_list.SetDescriptorHeaps(&[Some(heap.clone())]);
                }
                let mut cutout_on = false;

                let mut draw_tile = |(_, tx, ty, tile_res, tile_vp): &(usize, u32, u32, u32, Mat4), jobs: &[usize], cutout_on: &mut bool| {
                    cmd_list.RSSetViewports(&[D3D12_VIEWPORT {
                        TopLeftX: *tx as f32,
                        TopLeftY: *ty as f32,
                        Width: *tile_res as f32,
                        Height: *tile_res as f32,
                        MinDepth: 0.0,
                        MaxDepth: 1.0,
                    }]);
                    cmd_list.RSSetScissorRects(&[RECT {
                        left: *tx as i32,
                        top: *ty as i32,
                        right: (*tx + *tile_res) as i32,
                        bottom: (*ty + *tile_res) as i32,
                    }]);

                    for &job_index in jobs {
                        let (mesh_index, model) = &shadow_jobs[job_index];
                        let shadow_constants = crate::constant_buffer::ShadowConstants {
                            model_light_view_proj: ((*tile_vp) * (*model)).to_cols_array_2d(),
                        };
                        let slot = frame_index * self.spot_shadow_constant_buffer_capacity + draw_k;
                        draw_k += 1;
                        if let Err(e) = shadow_constants.write_at(local_cb, slot) {
                            eprintln!("[ENGINE] WARNING: failed to write local shadow constant buffer slot {}: {:?}", slot, e);
                            continue;
                        }
                        let gpu_addr = crate::constant_buffer::ShadowConstants::gpu_address_for_slot(local_cb, slot);
                        cmd_list.SetGraphicsRootConstantBufferView(0, gpu_addr);
                        self.shadow_bind_material(&cmd_list, &self.meshes[*mesh_index], self.spot_shadow_pipeline_state.as_ref().unwrap(), self.spot_shadow_cutout_pipeline_state.as_ref(), cutout_on, srv_descriptor_size);
                        Self::record_depth_only_draw(&cmd_list, &self.meshes[*mesh_index]);
                    }
                };

                for (tile, (_, before, _)) in shadow_tiles.iter().zip(tile_draws.iter()) {
                    if let (Some(jobs), true) = (before, tile.0 == atlas_kind) {
                        draw_tile(tile, jobs, &mut cutout_on);
                    }
                }
                if use_cache && copy_out[atlas_kind] {
                    copy(static_res.as_ref().unwrap(), D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, &atlas_res, D3D12_RESOURCE_STATE_DEPTH_WRITE);
                }
                for (tile, (_, _, after)) in shadow_tiles.iter().zip(tile_draws.iter()) {
                    if tile.0 == atlas_kind && !after.is_empty() {
                        draw_tile(tile, after, &mut cutout_on);
                    }
                }
            }

            let mut to_srv = Vec::new();
            if let Some(atlas) = &self.spot_shadow_atlas {
                if !self.spot_shadow_atlas_is_srv {
                    to_srv.push(Self::transition_barrier(
                        &atlas.resource,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ));
                    self.spot_shadow_atlas_is_srv = true;
                }
            }
            if let Some(atlas) = &self.point_shadow_atlas {
                if !self.point_shadow_atlas_is_srv {
                    to_srv.push(Self::transition_barrier(
                        &atlas.resource,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ));
                    self.point_shadow_atlas_is_srv = true;
                }
            }
            if !to_srv.is_empty() {
                cmd_list.ResourceBarrier(&to_srv);
                for b in to_srv {
                    Self::drop_transition_barrier(b);
                }
            }
        }

        cmd_list = self.maybe_flush_for_warm_up(cmd_list, &allocator, "shadow")?;
        if perf_enabled() {
            super::gpu_timer::mark(&cmd_list, frame_index, 2);
        }

        // ДОБАВЛЕНО (непрямой свет лучами, DDGI): пробы обновляются до
        // основного прохода — он читает их освещённость вместо ambient.
        if !self.gi_paused && self.gi.as_ref().is_some_and(|g| g.atlases().is_some()) {
            let mut draws = Vec::with_capacity(self.mesh_instances.len());
            // ИСПРАВЛЕНО: как и в тенях, лучи не видят светящееся (unlit) и
            // прозрачное — иначе колба вокруг лампы заслоняет её от всех
            // точек, и непрямой свет от ламп выходит нулевым.
            let blocks = |m: &super::Mesh, tint: [f32; 4]| !m.unlit && !m.blend && !m.shadow_only && tint[3] >= 0.999;
            for instance in &self.mesh_instances {
                if instance.mesh_index < self.meshes.len() && blocks(&self.meshes[instance.mesh_index], instance.tint) {
                    let albedo = self.mesh_albedo(&self.meshes[instance.mesh_index]);
                    draws.push(super::gi::RtDraw { mesh: instance.mesh_index, world: instance.transform_matrix(), tint: instance.tint, albedo });
                }
            }
            for (mesh_index, world) in self.scene.collect_renderables() {
                if mesh_index < self.meshes.len() && blocks(&self.meshes[mesh_index], [1.0; 4]) {
                    let albedo = self.mesh_albedo(&self.meshes[mesh_index]);
                    draws.push(super::gi::RtDraw { mesh: mesh_index, world, tint: [1.0; 4], albedo });
                }
            }
            let lights = self.get_gpu_lights().to_vec();
            let lighting = super::gi::RtLighting {
                sun_dir: self.transform_constants.light_dir,
                sun_color: self.transform_constants.light_color,
                sky: self.transform_constants.ambient_color,
                lights: &lights,
            };
            if let Err(e) = self.gi.as_mut().unwrap().record(&cmd_list, frame_index, &self.meshes, &draws, &lighting) {
                eprintln!("[GI] обновление проб не записано: {:?}", e);
            }
        }

        unsafe {
            // ИЗМЕНЕНО (честный SSAO): два RT — HDR-цвет и ambient
            // (renderer.ambient_rtv лежит в том же RTV-хипе сразу за
            // hdr_rtv, поэтому RTsSingleHandleToDescriptorRange = true).
            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 3);
            }
            // ИЗМЕНЕНО (отражения лучами): + G-буфер (слоты 2, 3 того же хипа).
            cmd_list.OMSetRenderTargets(4, Some(&rtv_handle), true, Some(&dsv_handle));
            cmd_list.ClearRenderTargetView(rtv_handle, &self.clear_color, None);
            cmd_list.ClearRenderTargetView(ambient_rtv, &[0.0, 0.0, 0.0, 0.0], None);
            for k in 2..4 {
                let h = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: rtv_handle.ptr + (k * rtv_size) as usize };
                cmd_list.ClearRenderTargetView(h, &[0.0, 0.0, 0.0, 0.0], None);
            }
            cmd_list.ClearDepthStencilView(dsv_handle, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: render_w as f32,
                Height: render_h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);

            let scissor = RECT {
                left: 0,
                top: 0,
                right: render_w as i32,
                bottom: render_h as i32,
            };
            cmd_list.RSSetScissorRects(&[scissor]);

            cmd_list.SetPipelineState(Some(self.pipeline_state.as_ref().unwrap()));
            cmd_list.SetGraphicsRootSignature(Some(self.root_signature.as_ref().unwrap()));

            let white_texture_srv_fallback = match self.ensure_white_texture() {
                Ok(index) => Some(index),
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось создать белую fallback-текстуру: {:?} — material-биндинг пропущен в этом кадре", e);
                    None
                }
            };
            let flat_normal_srv_fallback = match self.ensure_flat_normal_texture() {
                Ok(index) => Some(index),
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось создать flat-normal fallback-текстуру: {:?} — normal mapping пропущен в этом кадре", e);
                    None
                }
            };
            let neutral_mr_srv_fallback = match self.ensure_neutral_mr_texture() {
                Ok(index) => Some(index),
                Err(e) => {
                    eprintln!("[ENGINE] WARNING: не удалось создать нейтральную MR fallback-текстуру: {:?}", e);
                    None
                }
            };
            let cbv_srv_uav_size_materials = {
                let state = STATE.lock().unwrap();
                state.cbv_srv_uav_descriptor_size
            };

            if let Some(shadow_srv_heap) = &self.shadow_srv_heap {
                let heaps = [Some(shadow_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.SetGraphicsRootDescriptorTable(4, self.shadow_srv_gpu);
            }
            // ДОБАВЛЕНО (DDGI): объём проб (root-параметр 11), enabled = 0 — без GI.
            let gi_volume = self.gi.as_ref().map(|g| g.volume_gpu()).unwrap_or_default();
            cmd_list.SetGraphicsRoot32BitConstants(11, 16, &gi_volume as *const _ as *const _, 0);
            // ДОБАВЛЕНО (рассеяние света фонарей в среде): объём среды (параметр 12).
            let m = self.medium.unwrap_or_default();
            let medium: [f32; 8] = [m.min[0], m.min[1], m.min[2], m.scattering, m.max[0], m.max[1], m.max[2], m.anisotropy];
            cmd_list.SetGraphicsRoot32BitConstants(12, 8, medium.as_ptr() as *const _, 0);

            // ДОБАВЛЕНО (тени фонарей): матрицы плиток spot-атласа (b2).
            // Биндится всегда (root CBV должен быть валиден при любом
            // обращении); матрицы неиспользуемых плиток — нули.
            if let Some(matrices_buffer) = &self.spot_shadow_matrices_buffer {
                let mut spot_constants = crate::constant_buffer::SpotShadowConstants {
                    view_proj: [[[0.0; 4]; 4]; MAX_SPOT_SHADOWS],
                    params: [
                        SPOT_SHADOW_TILES_PER_ROW as f32,
                        1.0 / SPOT_SHADOW_TILES_PER_ROW as f32,
                        SPOT_SHADOW_TILE_RESOLUTION as f32,
                        0.0,
                    ],
                    point_view_proj: [[[0.0; 4]; 4]; MAX_POINT_SHADOWS * 6],
                    point_params: [
                        POINT_SHADOW_TILES_PER_ROW as f32,
                        1.0 / POINT_SHADOW_TILES_PER_ROW as f32,
                        POINT_SHADOW_TILE_RESOLUTION as f32,
                        super::point_shadow_face_tan_half_fov(),
                    ],
                };
                for (tile, (_, vp)) in spot_shadow_lights.iter().enumerate() {
                    spot_constants.view_proj[tile] = vp.to_cols_array_2d();
                }
                for (slot, (_, face_vps)) in point_shadow_lights.iter().enumerate() {
                    for (face, vp) in face_vps.iter().enumerate() {
                        spot_constants.point_view_proj[slot * 6 + face] = vp.to_cols_array_2d();
                    }
                }
                if let Err(e) = spot_constants.write_at(matrices_buffer, frame_index) {
                    eprintln!("[ENGINE] WARNING: не удалось записать матрицы spot-теней: {:?}", e);
                }
                let addr = crate::constant_buffer::SpotShadowConstants::gpu_address_for_slot(matrices_buffer, frame_index);
                cmd_list.SetGraphicsRootConstantBufferView(9, addr);
            }

            for cascade in 0..NUM_CASCADES {
                self.transform_constants.light_view_proj[cascade] = cascade_view_projs[cascade].to_cols_array_2d();
            }
            self.transform_constants.cascade_split_distances = [
                cascade_far_distances[0],
                cascade_far_distances[1],
                cascade_far_distances[2],
                0.0,
            ];
            self.transform_constants.shadow_map_size = SHADOW_MAP_RESOLUTION as f32;
            self.transform_constants.shadows_enabled =
                if self.shadow_pipeline_state.is_some() && self.shadow_root_signature.is_some() { 1 } else { 0 };

            let light_count = self.get_gpu_lights().len();
            if let Err(e) = self.ensure_light_buffer_capacity(light_count) {
                eprintln!("[ENGINE] WARNING: не удалось выделить light_buffer: {:?}", e);
            }
            if let Some(light_buffer) = &self.light_buffer {
                if light_count > 0 {
                    // ИЗМЕНЕНО (тени фонарей): копия списка, чтобы проставить
                    // тенеобразующим фонарям номер плитки атласа в params.w
                    // (1-based; FirstFires всегда пишет туда 0). Данные
                    // плагина не трогаем.
                    let mut gpu_lights: Vec<GPULight> = self.get_gpu_lights().to_vec();
                    for (tile, (light_index, _)) in spot_shadow_lights.iter().enumerate() {
                        if let Some(light) = gpu_lights.get_mut(*light_index) {
                            light.params[3] = (tile + 1) as f32;
                        }
                    }
                    // Point-фонари: отрицательный номер -(слот+1), см. HLSL.
                    for (slot, (light_index, _)) in point_shadow_lights.iter().enumerate() {
                        if let Some(light) = gpu_lights.get_mut(*light_index) {
                            light.params[3] = -((slot + 1) as f32);
                        }
                    }
                    let bytes = std::slice::from_raw_parts(
                        gpu_lights.as_ptr() as *const u8,
                        light_count * std::mem::size_of::<GPULight>(),
                    );
                    if let Err(e) = light_buffer.update_structured_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить light_buffer: {:?}", e);
                    }
                }
                let light_gpu_addr = light_buffer.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootShaderResourceView(1, light_gpu_addr);
            }
            self.transform_constants.light_count = light_count as u32;

            let grid_params = self.lights.as_ref().map(|l| l.get_grid_params());
            let grid_cells_count = self.lights.as_ref().map(|l| l.get_grid_cells().len()).unwrap_or(0);
            let grid_entries_count = self.lights.as_ref().map(|l| l.get_grid_entries().len()).unwrap_or(0);

            if let Err(e) = self.ensure_grid_cells_buffer_capacity(grid_cells_count) {
                eprintln!("[ENGINE] WARNING: не удалось выделить grid_cells_buffer: {:?}", e);
            }
            if let Err(e) = self.ensure_grid_entries_buffer_capacity(grid_entries_count) {
                eprintln!("[ENGINE] WARNING: не удалось выделить grid_entries_buffer: {:?}", e);
            }

            if let Some(grid_cells_buffer) = &self.grid_cells_buffer {
                if grid_cells_count > 0 {
                    let cells = self.lights.as_ref().map(|l| l.get_grid_cells()).unwrap_or(&[]);
                    let bytes = std::slice::from_raw_parts(
                        cells.as_ptr() as *const u8,
                        grid_cells_count * std::mem::size_of::<LightGridCell>(),
                    );
                    if let Err(e) = grid_cells_buffer.update_structured_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить grid_cells_buffer: {:?}", e);
                    }
                }
                let addr = grid_cells_buffer.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootShaderResourceView(2, addr);
            }
            if let Some(grid_entries_buffer) = &self.grid_entries_buffer {
                if grid_entries_count > 0 {
                    let entries = self.lights.as_ref().map(|l| l.get_grid_entries()).unwrap_or(&[]);
                    let bytes = std::slice::from_raw_parts(
                        entries.as_ptr() as *const u8,
                        grid_entries_count * std::mem::size_of::<LightGridEntry>(),
                    );
                    if let Err(e) = grid_entries_buffer.update_structured_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить grid_entries_buffer: {:?}", e);
                    }
                }
                let addr = grid_entries_buffer.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootShaderResourceView(3, addr);
            }

            match grid_params {
                Some(p) => {
                    self.transform_constants.grid_world_min = [p.world_min[0], p.world_min[1], p.world_min[2], p.cell_size];
                    self.transform_constants.grid_dimensions = [p.grid_width, p.grid_height, p.grid_depth, 0];
                }
                None => {
                    self.transform_constants.grid_world_min = [0.0, 0.0, 0.0, 1.0];
                    self.transform_constants.grid_dimensions = [0, 0, 0, 0];
                }
            }

            enum DrawTransform {
                /// Обычный 3D-объект: своя model-матрица, view/proj берутся
                /// из камеры один раз на весь кадр.
                Camera(Mat4),
                /// Старый 2D-режим (mesh_instances пуст) — все 4 матрицы
                /// константного буфера были identity, без камеры. Сохраняем
                /// это поведение один в один, чтобы не сломать main1.rs.
                RawIdentity,
            }
            struct DrawJob {
                mesh_index: usize,
                transform: DrawTransform,
                /// ДОБАВЛЕНО (прозрачность): множитель цвета инстанса, см. MeshInstance::tint.
                tint: [f32; 4],
            }

            let mut jobs: Vec<DrawJob> = Vec::new();

            if !self.mesh_instances.is_empty() {
                for instance in &self.mesh_instances {
                    if instance.mesh_index < self.meshes.len() {
                        let world = instance.transform_matrix();
                        if let Some(idx) = self.resolve_lod_mesh_index(instance.mesh_index, world, camera_pos_for_lod) {
                            jobs.push(DrawJob {
                                mesh_index: idx,
                                transform: DrawTransform::Camera(world),
                                tint: instance.tint,
                            });
                        }
                    }
                }
            } else if self.scene.is_empty() {
                for i in 0..self.meshes.len() {
                    jobs.push(DrawJob { mesh_index: i, transform: DrawTransform::RawIdentity, tint: [1.0; 4] });
                }
            }

            for (mesh_index, world) in self.scene.collect_renderables() {
                if mesh_index < self.meshes.len() {
                    if let Some(idx) = self.resolve_lod_mesh_index(mesh_index, world, camera_pos_for_lod) {
                        jobs.push(DrawJob { mesh_index: idx, transform: DrawTransform::Camera(world), tint: [1.0; 4] });
                    }
                }
            }

            let view = self.camera.view_matrix();
            let mut proj = self.camera.projection_matrix();
            // ДОБАВЛЕНО (TAA): проекция со сдвигом этого кадра на долю пикселя.
            // ИЗМЕНЕНО (TAAU): кадр меньше экрана собирает только TAA — тогда он
            // работает и при выключенном `self.taa`.
            if (self.taa || upscaling) && !self.taa_unavailable {
                let msaa = self.msaa_samples > 1;
                if !self.taa_state.as_ref().is_some_and(|t| t.matches(render_w, render_h, self.width, self.height, msaa)) {
                    // старую историю может ещё читать кадр в полёте (ресайз окна)
                    if self.taa_state.is_some() {
                        self.wait_for_all_frames_idle_before_realloc();
                    }
                    self.taa_state = None;
                    match super::taa::Taa::new(render_w, render_h, self.width, self.height, msaa) {
                        Ok(t) => self.taa_state = Some(t),
                        Err(e) => {
                            eprintln!("[TAA] недоступно: {:?}", e);
                            self.taa_unavailable = true;
                        }
                    }
                }
                if let Some(t) = &self.taa_state {
                    proj = t.jittered(proj);
                }
            }
            self.taa_view_proj = proj * view;
            // ДОБАВЛЕНО (отражения лучами): только при работающем DDGI (его TLAS).
            let rt_reflections_active = self.rt_reflections && self.gi.as_ref().is_some_and(|g| g.atlases().is_some());
            let id_matrix = identity();

            let frustum = crate::math::Frustum::from_view_proj(&(proj * view));
            // ДОБАВЛЕНО (тень игрока): только-тень меши в основной проход не идут.
            jobs.retain(|job| !self.meshes[job.mesh_index].shadow_only);
            jobs.retain(|job| match &job.transform {
                DrawTransform::Camera(model) => {
                    let mesh = &self.meshes[job.mesh_index];
                    let (scale, _rotation, _translation) = model.to_scale_rotation_translation();
                    let max_scale = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
                    let local_center = Vec3::new(
                        mesh.bounding_center[0],
                        mesh.bounding_center[1],
                        mesh.bounding_center[2],
                    );
                    let world_center = model.transform_point3(local_center);
                    let world_radius = mesh.bounding_radius * max_scale;
                    frustum.test_sphere(world_center, world_radius)
                        && (world_center - camera_pos_for_lod).length() - world_radius <= mesh.max_draw_distance
                }
                DrawTransform::RawIdentity => true,
            });
            // ДОБАВЛЕНО (прозрачность): сначала непрозрачное, затем
            // полупрозрачное от дальнего к ближнему (смешивание без записи
            // глубины корректно только в таком порядке).
            let is_transparent = |job: &DrawJob, meshes: &Vec<crate::engine::Mesh>| job.tint[3] < 0.999 || meshes[job.mesh_index].blend;
            let (mut opaque_jobs, mut transparent_jobs): (Vec<DrawJob>, Vec<DrawJob>) =
                jobs.into_iter().partition(|j| !is_transparent(j, &self.meshes));
            let depth_of = |job: &DrawJob, meshes: &Vec<crate::engine::Mesh>| match &job.transform {
                DrawTransform::Camera(model) => {
                    let m = &meshes[job.mesh_index];
                    let c = model.transform_point3(Vec3::new(m.bounding_center[0], m.bounding_center[1], m.bounding_center[2]));
                    (c - camera_pos_for_lod).length_squared()
                }
                DrawTransform::RawIdentity => 0.0,
            };
            transparent_jobs.sort_by(|a, b| depth_of(b, &self.meshes).partial_cmp(&depth_of(a, &self.meshes)).unwrap_or(std::cmp::Ordering::Equal));
            let first_transparent = opaque_jobs.len();
            opaque_jobs.append(&mut transparent_jobs);
            let jobs = opaque_jobs;
            perf_main_draws = jobs.len();
            perf_main_tris = jobs.iter().map(|j| self.meshes[j.mesh_index].index_count as u64 / 3).sum();

            self.poll_occluder_readback();
            let mut occluder_instance_data: Vec<f32> = Vec::new();
            for job in jobs.iter().take(first_transparent) {
                let DrawTransform::Camera(model) = &job.transform else { continue };
                if job.mesh_index >= self.meshes.len() { continue; }
                let mesh = &self.meshes[job.mesh_index];
                // Купол неба окружает камеру — как заслоняющий он "закрыл" бы всё.
                if mesh.unlit { continue; }
                let (scale, _rotation, _translation) = model.to_scale_rotation_translation();
                let max_scale = scale.x.abs().max(scale.y.abs()).max(scale.z.abs());
                let world_radius = mesh.bounding_radius * max_scale;
                if world_radius < OCCLUDER_MIN_WORLD_RADIUS {
                    continue;
                }
                let local_center = Vec3::new(mesh.bounding_center[0], mesh.bounding_center[1], mesh.bounding_center[2]);
                let world_center = model.transform_point3(local_center);
                let half_extent = world_radius * OCCLUDER_INSCRIBE_FACTOR;
                occluder_instance_data.extend_from_slice(&[
                    world_center.x - half_extent, world_center.y - half_extent, world_center.z - half_extent,
                    world_center.x + half_extent, world_center.y + half_extent, world_center.z + half_extent,
                ]);
            }
            self.submit_occluder_pass(&occluder_instance_data, view, proj);

            self.ensure_constant_buffer_capacity(jobs.len())?;

            // ИЗМЕНЕНО (отражения лучами в стекле): в прозрачной части PSO
            // выбирается по мешу — стекло пишет G-буфер, остальное нет.
            let mut glass_bound: Option<bool> = None;
            // ДОБАВЛЕНО (предпроход глубины): проход 0 — только глубина
            // непрозрачного (без альфа-выреза: без PS его не отбросить), проход
            // 1 — как раньше, непрозрачное из предпрохода — поверх готовой
            // глубины, дорогой шейдер один раз на пиксель. Константы
            // преобразования пишутся в тот же слот, что и в проходе 1.
            let prepass_on = self.depth_prepass && self.prepass_pipeline_state.is_some() && self.after_prepass_pipeline_state.is_some();
            let prepassed = |mesh: &super::Mesh| mesh.alpha_cutoff <= 0.0;
            for pass in 0..2 {
            if pass == 0 && !prepass_on {
                continue;
            }
            // какой PSO непрозрачного сейчас стоит: 0 — предпроход, 1 — поверх него, 2 — обычный
            let mut opaque_bound: Option<u8> = None;
            for (i, job) in jobs.iter().enumerate() {
                let mesh = &self.meshes[job.mesh_index];
                if pass == 0 {
                    if i >= first_transparent || !prepassed(mesh) {
                        continue;
                    }
                    if opaque_bound != Some(0) {
                        cmd_list.SetPipelineState(self.prepass_pipeline_state.as_ref());
                        opaque_bound = Some(0);
                    }
                } else if i < first_transparent && prepass_on {
                    let want = if prepassed(mesh) { 1 } else { 2 };
                    if opaque_bound != Some(want) {
                        let pso = if want == 1 { self.after_prepass_pipeline_state.as_ref() } else { self.pipeline_state.as_ref() };
                        cmd_list.SetPipelineState(pso);
                        opaque_bound = Some(want);
                    }
                }
                if pass == 1 && i == first_transparent && perf_enabled() {
                    super::gpu_timer::mark(&cmd_list, frame_index, 4);
                }
                if pass == 1 && i >= first_transparent {
                    let want_glass = mesh.glass && rt_reflections_active && self.glass_pipeline_state.is_some();
                    if glass_bound != Some(want_glass) {
                        let pso = if want_glass { self.glass_pipeline_state.as_ref() } else { self.transparent_pipeline_state.as_ref() };
                        if let Some(pso) = pso {
                            cmd_list.SetPipelineState(Some(pso));
                        }
                        glass_bound = Some(want_glass);
                    }
                }

                if let (1, Some(shadow_srv_heap)) = (pass, self.shadow_srv_heap.as_ref()) {
                    let albedo_slot = mesh.albedo_srv_index.or(white_texture_srv_fallback);
                    if let Some(albedo_slot) = albedo_slot {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, albedo_slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(5, gpu_handle);
                    }

                    let normal_slot = mesh.normal_srv_index.or(flat_normal_srv_fallback);
                    if let Some(normal_slot) = normal_slot {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, normal_slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(6, gpu_handle);
                    }

                    let mr_slot = mesh.mr_srv_index.or(neutral_mr_srv_fallback);
                    if let Some(mr_slot) = mr_slot {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, mr_slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(7, gpu_handle);
                    }

                    // ДОБАВЛЕНО (детальная текстура): без неё — белая заглушка, шейдер её не читает.
                    if let Some(slot) = mesh.detail_srv_index.or(white_texture_srv_fallback) {
                        let gpu_handle = crate::heap::DescriptorHeap::get_gpu_handle(shadow_srv_heap, slot, cbv_srv_uav_size_materials);
                        cmd_list.SetGraphicsRootDescriptorTable(10, gpu_handle);
                    }
                }

                let has_mr_map = if mesh.mr_srv_index.is_some() { 1.0f32 } else { 0.0f32 };
                // Раскладка ОБЯЗАНА совпадать с cbuffer MaterialConstants
                // (b1) в main_ps.hlsl: metallic, roughness, hasMrMap,
                // lightEmitterArea, emissive.rgb, alphaCutoff, tint.rgba, isBlend, glass, unlit + резерв.
                let e = mesh.material_emissive;
                let t = job.tint;
                let blended = if i >= first_transparent { 1.0f32 } else { 0.0 };
                let flag = |b: bool| if b { 1.0f32 } else { 0.0 };
                let material_constants: [f32; 20] = [
                    mesh.material_metallic, mesh.material_roughness, has_mr_map, mesh.light_emitter_area,
                    e[0], e[1], e[2], mesh.alpha_cutoff,
                    t[0], t[1], t[2], t[3],
                    blended, flag(mesh.glass && blended > 0.5), flag(mesh.unlit), flag(rt_reflections_active),
                    mesh.detail_uv_scale[0], mesh.detail_uv_scale[1], flag(mesh.detail_srv_index.is_some()), if mesh.normal_srv_index.is_some() { mesh.parallax_depth } else { 0.0 },
                ];
                if pass == 1 {
                    cmd_list.SetGraphicsRoot32BitConstants(8, 20, material_constants.as_ptr() as *const _, 0);
                }

                match &job.transform {
                    DrawTransform::Camera(model) => {
                        let model = *model;
                        let model_view_proj = proj * view * model;
                        self.transform_constants.model_view_proj = model_view_proj.to_cols_array_2d();
                        self.transform_constants.model = model.to_cols_array_2d();
                        self.transform_constants.view = view.to_cols_array_2d();
                        self.transform_constants.proj = proj.to_cols_array_2d();
                        self.transform_constants.camera_pos = [
                            self.camera.position.x,
                            self.camera.position.y,
                            self.camera.position.z,
                            1.0,
                        ];
                    }
                    DrawTransform::RawIdentity => {
                        self.transform_constants.model_view_proj = id_matrix.to_cols_array_2d();
                        self.transform_constants.model = id_matrix.to_cols_array_2d();
                        self.transform_constants.view = id_matrix.to_cols_array_2d();
                        self.transform_constants.proj = id_matrix.to_cols_array_2d();
                    }
                }

                let slot = frame_index * self.constant_buffer_capacity + i;
                let Some(cb) = self.constant_buffer.as_ref() else {
                    eprintln!("[ENGINE] WARNING: no constant buffer available, skipping draw");
                    continue;
                };
                if let Err(e) = self.transform_constants.write_at(cb, slot) {
                    eprintln!("[ENGINE] WARNING: failed to write constant buffer slot {}: {:?}", slot, e);
                    continue;
                }
                let gpu_addr = TransformConstants::gpu_address_for_slot(cb, slot);
                cmd_list.SetGraphicsRootConstantBufferView(0, gpu_addr);

                let vertex_buffer_view = D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: mesh.vertex_buffer.resource.GetGPUVirtualAddress(),
                    SizeInBytes: mesh.vertex_buffer.size as u32,
                    StrideInBytes: Vertex::STRIDE,
                };
                cmd_list.IASetVertexBuffers(0, Some(&[vertex_buffer_view]));
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                if let Some(index_buffer) = &mesh.index_buffer {
                    let index_view = D3D12_INDEX_BUFFER_VIEW {
                        BufferLocation: index_buffer.resource.GetGPUVirtualAddress(),
                        SizeInBytes: index_buffer.size as u32,
                        Format: DXGI_FORMAT_R32_UINT,
                    };
                    cmd_list.IASetIndexBuffer(Some(&index_view));
                    cmd_list.DrawIndexedInstanced(mesh.index_count, 1, 0, 0, 0);
                } else {
                    cmd_list.DrawInstanced(mesh.vertex_count, 1, 0, 0);
                }
            }
            }
            // прозрачного в кадре нет — метка «конец непрозрачного» всё равно нужна
            if first_transparent == jobs.len() && perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 4);
            }

            cmd_list = self.maybe_flush_for_warm_up(cmd_list, &allocator, "main")?;

            let renderer = self.renderer.as_ref().ok_or_else(|| {
                eprintln!("[ENGINE] ERROR: render_frame() lost renderer mid-frame (unexpected)");
                Error::from_hresult(HRESULT(1))
            })?;

            // ДОБАВЛЕНО (максимальная графика — MSAA): основной цветовой
            // проход выше рисовал в `renderer.hdr_target` — многосэмпловый
            // (см. MSAA_SAMPLES) render target. Ни один downstream-проход
            // (bloom/volumetric-composite/tonemap) не умеет читать
            // многосэмпловый Texture2D напрямую (они написаны под обычный
            // Texture2D, см. их шейдеры) — разрешаем ОДИН РАЗ здесь, сразу
            // после main pass, в одноимпловую `renderer.hdr_resolved`,
            // чью SRV (`renderer.hdr_srv_gpu`) все downstream-проходы и
            // читают дальше НИЧЕГО не зная про MSAA — тот же путь, что и
            // раньше, только источник теперь разрешённая копия, а не
            // `hdr_target` напрямую.
            //
            // Состояния: `hdr_target` каждый кадр приходит сюда в
            // RENDER_TARGET (main pass только что в неё рисовал) и
            // ОБЯЗАН уйти отсюда обратно в RENDER_TARGET — следующий кадр
            // начинает с `ClearRenderTargetView`/`OMSetRenderTargets` на
            // неё же, ожидая именно этого состояния (см. выше по кадру).
            // `hdr_resolved` создаётся сразу в PIXEL_SHADER_RESOURCE (см.
            // `RenderTexture::create_hdr_target` в render.rs) и НИГДЕ,
            // кроме как здесь, не меняет состояние — то есть гарантированно
            // приходит сюда в PIXEL_SHADER_RESOURCE на КАЖДОМ кадре,
            // включая первый, без отдельной ветки под "первый кадр".
            {
                // ДОБАВЛЕНО (runtime-переключаемый MSAA — по прямому
                // запросу пользователя): при MSAA=off `hdr_target` создаётся
                // ОДНОИМПЛОВЫМ (см. `Renderer::new`, msaa_samples=1) — то
                // есть уже РОВНО того же формата/размера/сэмплинга, что и
                // `hdr_resolved`. `ResolveSubresource` требует именно
                // многосэмпловый источник (иначе ошибка валидации D3D12),
                // поэтому здесь простой `CopyResource` вместо resolve — обе
                // ветки решают одну и ту же задачу ("перенести то, что
                // нарисовал main pass, в текстуру, которую дальше читают
                // bloom/volumetric/tonemap"), отличается только GPU-команда
                // и связанные с ней состояния ресурсов (COPY_* вместо
                // RESOLVE_*).
                let msaa_on = self.msaa_samples > 1;
                let (src_state_before, dst_state_before) = if msaa_on {
                    (D3D12_RESOURCE_STATE_RESOLVE_SOURCE, D3D12_RESOURCE_STATE_RESOLVE_DEST)
                } else {
                    (D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST)
                };

                // ИЗМЕНЕНО (честный SSAO): ambient-таргет резолвится тем же
                // путём, что и HDR — его читает composite в тонмапе.
                let to_resolve = [
                    Self::transition_barrier(
                        &renderer.hdr_target.resource,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        src_state_before,
                    ),
                    Self::transition_barrier(
                        &renderer.hdr_resolved.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        dst_state_before,
                    ),
                    Self::transition_barrier(
                        &renderer.ambient_target.resource,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        src_state_before,
                    ),
                    Self::transition_barrier(
                        &renderer.ambient_resolved.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        dst_state_before,
                    ),
                ];
                cmd_list.ResourceBarrier(&to_resolve);
                for b in to_resolve {
                    Self::drop_transition_barrier(b);
                }

                if msaa_on {
                    cmd_list.ResolveSubresource(
                        &renderer.hdr_resolved.resource,
                        0,
                        &renderer.hdr_target.resource,
                        0,
                        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
                    );
                    cmd_list.ResolveSubresource(
                        &renderer.ambient_resolved.resource,
                        0,
                        &renderer.ambient_target.resource,
                        0,
                        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
                    );
                } else {
                    cmd_list.CopyResource(&renderer.hdr_resolved.resource, &renderer.hdr_target.resource);
                    cmd_list.CopyResource(&renderer.ambient_resolved.resource, &renderer.ambient_target.resource);
                }

                let after_resolve = [
                    Self::transition_barrier(
                        &renderer.hdr_target.resource,
                        src_state_before,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ),
                    Self::transition_barrier(
                        &renderer.hdr_resolved.resource,
                        dst_state_before,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ),
                    Self::transition_barrier(
                        &renderer.ambient_target.resource,
                        src_state_before,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ),
                    Self::transition_barrier(
                        &renderer.ambient_resolved.resource,
                        dst_state_before,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ),
                ];
                cmd_list.ResourceBarrier(&after_resolve);
                for b in after_resolve {
                    Self::drop_transition_barrier(b);
                }

                // ДОБАВЛЕНО (отражения лучами): G-буфер — тем же путём.
                let gbuf = [(&renderer.spec_target, &renderer.spec_resolved), (&renderer.normal_target, &renderer.normal_resolved)];
                let mut before = Vec::new();
                for (t, r) in gbuf {
                    before.push(Self::transition_barrier(&t.resource, D3D12_RESOURCE_STATE_RENDER_TARGET, src_state_before));
                    before.push(Self::transition_barrier(&r.resource, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, dst_state_before));
                }
                cmd_list.ResourceBarrier(&before);
                for b in before {
                    Self::drop_transition_barrier(b);
                }
                for (t, r) in gbuf {
                    if msaa_on {
                        cmd_list.ResolveSubresource(&r.resource, 0, &t.resource, 0, windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT);
                    } else {
                        cmd_list.CopyResource(&r.resource, &t.resource);
                    }
                }
                let mut after = Vec::new();
                for (t, r) in gbuf {
                    after.push(Self::transition_barrier(&t.resource, src_state_before, D3D12_RESOURCE_STATE_RENDER_TARGET));
                    after.push(Self::transition_barrier(&r.resource, dst_state_before, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE));
                }
                cmd_list.ResourceBarrier(&after);
                for b in after {
                    Self::drop_transition_barrier(b);
                }
            }

            // ДОБАВЛЕНО (по прямому запросу пользователя — переключаемые
            // графические настройки): `graphics_settings.volumetric`
            // проверяется ЗДЕСЬ, а не через обнуление `volumetric_texture`
            // (как делает `disable_volumetric_for_diagnostics` для разовой
            // ручной диагностики) — иначе SRV этой текстуры в
            // `renderer.srv_uav_heap`, которую БЕЗУСЛОВНО каждый кадр
            // читает tonemap composite, указывала бы на уже уничтоженный
            // ресурс. Ресурс остаётся живым, пропускается только сам
            // проход (реальная экономия GPU-времени всё равно есть).
            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 5);
            }
            if self.graphics_settings.volumetric { if let (Some(volumetric_texture), Some(volumetric_srv_heap), Some(volumetric_cb)) = (
                &self.volumetric_texture,
                &self.volumetric_srv_heap,
                &self.volumetric_constant_buffer,
            ) {
                let vol_width = volumetric_texture.width;
                let vol_height = volumetric_texture.height;

                let mut barriers = Vec::with_capacity(2);
                if !self.depth_stencil_is_srv {
                    barriers.push(Self::transition_barrier(
                        &renderer.depth_stencil.resource,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ));
                }
                if self.volumetric_is_srv {
                    barriers.push(Self::transition_barrier(
                        &volumetric_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ));
                }
                if !barriers.is_empty() {
                    cmd_list.ResourceBarrier(&barriers);
                    for b in barriers {
                        Self::drop_transition_barrier(b);
                    }
                }
                self.depth_stencil_is_srv = true;
                self.volumetric_is_srv = false;

                let vol_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: vol_width as f32,
                    Height: vol_height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                let vol_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: vol_width as i32,
                    bottom: vol_height as i32,
                };
                cmd_list.RSSetViewports(&[vol_viewport]);
                cmd_list.RSSetScissorRects(&[vol_scissor]);

                cmd_list.OMSetRenderTargets(1, Some(&self.volumetric_rtv), false, None);
                cmd_list.SetPipelineState(Some(self.volumetric_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.volumetric_root_signature.as_ref().unwrap()));
                let heaps = [Some(volumetric_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.SetGraphicsRootDescriptorTable(0, self.volumetric_srv_gpu_raymarch);
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                let inv_view_proj = (self.camera.projection_matrix() * self.camera.view_matrix()).inverse();
                let sun_color = [
                    self.transform_constants.light_color[0],
                    self.transform_constants.light_color[1],
                    self.transform_constants.light_color[2],
                ];
                let sun_intensity = self.transform_constants.light_color[3];
                // ИСПРАВЛЕНО (жалоба "свет стал выцветшим, пропала сочность"):
                // этот коэффициент технически существовал и раньше, но
                // НИКОГДА не был по-настоящему проверен глазами — volumetric-
                // проход сэмплировал `depth_srv_heap`, который до фикса
                // ресайза в window.rs указывал на давно уничтоженный depth
                // stencil (см. handle_resize) и практически не мог влиять на
                // картинку осмысленно. Как только depth SRV стал указывать на
                // РЕАЛЬНЫЙ буфер глубины, шейдер (см. compile_volumetric_shaders)
                // начал честно считать густой воздух — а он аддитивный и
                // ПОЛНОСТЬЮ не зависит от пройденной дистанции/оптической
                // плотности (нет экстинкции), только от видимости солнца
                // вдоль луча. Это значит, что почти ВЕСЬ экран (небо и земля
                // одинаково) получает примерно одинаковую добавку — при
                // 0.15 она была достаточно большой, чтобы поднять тени/
                // полутона к белому ДО ACES-тонмаппинга, визуально смывая
                // контраст и насыщенность через всю сцену. 0.04 — та же
                // самая формула и тот же множитель по sunFacing/accumulated
                // (эффект остаётся, слегка ярче по направлению к солнцу),
                // просто в разумных пределах для аддитивного тумана без
                // экстинкции.
                let vol_intensity = 0.04 * sun_intensity;

                #[repr(C)]
                struct VolumetricParamsGpu {
                    inv_view_proj: [[f32; 4]; 4],
                    light_view_proj: [[f32; 4]; 4],
                    camera_pos: [f32; 3],
                    intensity: f32,
                    light_dir: [f32; 3],
                    _padding0: f32,
                    light_color: [f32; 3],
                    max_distance: f32,
                }
                let params = VolumetricParamsGpu {
                    inv_view_proj: inv_view_proj.to_cols_array_2d(),
                    light_view_proj: cascade_view_projs[0].to_cols_array_2d(),
                    camera_pos: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
                    intensity: vol_intensity,
                    light_dir: [light_dir_vec.x, light_dir_vec.y, light_dir_vec.z],
                    _padding0: 0.0,
                    light_color: sun_color,
                    max_distance: self.camera.far.min(150.0),
                };
                let bytes = std::slice::from_raw_parts(
                    &params as *const VolumetricParamsGpu as *const u8,
                    std::mem::size_of::<VolumetricParamsGpu>(),
                );
                let _ = volumetric_cb.update_constant_buffer(bytes);
                cmd_list.SetGraphicsRootConstantBufferView(1, volumetric_cb.resource.GetGPUVirtualAddress());

                cmd_list.DrawInstanced(3, 1, 0, 0);

                // ИЗМЕНЕНО (максимальная графика — SSAO): раньше здесь
                // сразу же возвращали `depth_stencil` обратно в
                // DEPTH_WRITE — теперь этот переход отложен ДО ПОСЛЕ
                // SSAO-прохода ниже (он делит с volumetric ОДНО и то же
                // "depth уже SRV" окно, см. комментарий там), чтобы не
                // переключать состояние depth_stencil туда-обратно дважды
                // за кадр. Здесь возвращаем в PSR только сам
                // `volumetric_texture` — он больше никому в этом кадре не
                // нужен как RTV.
                let vol_to_srv = [Self::transition_barrier(
                    &volumetric_texture.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                )];
                cmd_list.ResourceBarrier(&vol_to_srv);
                for b in vol_to_srv {
                    Self::drop_transition_barrier(b);
                }
                self.volumetric_is_srv = true;
            } } else if let Some(volumetric_texture) = &self.volumetric_texture {
                // ИСПРАВЛЕНО (та же причина, что и у SSAO ниже — см.
                // подробный комментарий там): volumetric-свет АДДИТИВНЫЙ
                // (`combined = hdrColor + ... + volumetricColor`, см.
                // pipeline_post.rs), поэтому непроинициализированное
                // содержимое здесь менее разрушительно, чем у
                // мультипликативного SSAO (не может занулить всю картинку),
                // но всё равно может добавить случайный паразитный оттенок —
                // на всякий случай чистим в (0,0,0,0), нейтральный "нет
                // god rays" для аддитивного канала.
                if self.volumetric_is_srv {
                    let to_rt = [Self::transition_barrier(
                        &volumetric_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    )];
                    cmd_list.ResourceBarrier(&to_rt);
                    for b in to_rt {
                        Self::drop_transition_barrier(b);
                    }
                }
                cmd_list.ClearRenderTargetView(self.volumetric_rtv, &[0.0, 0.0, 0.0, 0.0], None);
                let to_srv = [Self::transition_barrier(
                    &volumetric_texture.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                )];
                cmd_list.ResourceBarrier(&to_srv);
                for b in to_srv {
                    Self::drop_transition_barrier(b);
                }
                self.volumetric_is_srv = true;
            }

            // ДОБАВЛЕНО (максимальная графика — SSAO, см.
            // engine/pipeline_ssao.rs): делит с volumetric-блоком выше ОДНО
            // и то же окно "depth_stencil уже PIXEL_SHADER_RESOURCE" —
            // depth_stencil_is_srv к этому моменту либо УЖЕ true (если
            // volumetric-блок выше реально выполнился), либо всё ещё false
            // (если volumetric отключён диагностикой, см.
            // disable_volumetric_for_diagnostics) — в последнем случае SSAO
            // сам переводит depth в SRV, как раньше это делал volumetric в
            // одиночку.
            // ДОБАВЛЕНО (по прямому запросу пользователя — переключаемые
            // графические настройки): та же причина, что и у volumetric
            // чуть выше — `graphics_settings.ssao` проверяется здесь, а не
            // через обнуление `ssao_texture` (см. `disable_ssao_for_diagnostics`,
            // предназначенный для разовой ручной диагностики, а не для
            // штатного переключения) — иначе SRV этой текстуры в t3
            // tonemap-composite указывала бы на уничтоженный ресурс.
            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 6);
            }
            if self.graphics_settings.ssao { if let (Some(ssao_texture), Some(ssao_depth_srv_heap), Some(ssao_cb)) = (
                &self.ssao_texture,
                &self.ssao_depth_srv_heap,
                &self.ssao_constant_buffer,
            ) {
                let ao_width = ssao_texture.width;
                let ao_height = ssao_texture.height;

                let mut barriers = Vec::with_capacity(2);
                if !self.depth_stencil_is_srv {
                    barriers.push(Self::transition_barrier(
                        &renderer.depth_stencil.resource,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ));
                }
                if self.ssao_is_srv {
                    barriers.push(Self::transition_barrier(
                        &ssao_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ));
                }
                if !barriers.is_empty() {
                    cmd_list.ResourceBarrier(&barriers);
                    for b in barriers {
                        Self::drop_transition_barrier(b);
                    }
                }
                self.depth_stencil_is_srv = true;
                self.ssao_is_srv = false;

                let ao_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: ao_width as f32,
                    Height: ao_height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                let ao_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: ao_width as i32,
                    bottom: ao_height as i32,
                };
                cmd_list.RSSetViewports(&[ao_viewport]);
                cmd_list.RSSetScissorRects(&[ao_scissor]);

                cmd_list.OMSetRenderTargets(1, Some(&self.ssao_rtv), false, None);
                cmd_list.SetPipelineState(Some(self.ssao_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.ssao_root_signature.as_ref().unwrap()));
                let heaps = [Some(ssao_depth_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.SetGraphicsRootDescriptorTable(0, self.ssao_srv_gpu_depth);
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                let view_proj = self.camera.projection_matrix() * self.camera.view_matrix();
                let inv_view_proj = view_proj.inverse();

                #[repr(C)]
                struct SSAOParamsGpu {
                    view_proj: [[f32; 4]; 4],
                    inv_view_proj: [[f32; 4]; 4],
                    camera_pos: [f32; 3],
                    radius: f32,
                    bias: f32,
                    strength: f32,
                    _padding0: [f32; 2],
                }
                let params = SSAOParamsGpu {
                    view_proj: view_proj.to_cols_array_2d(),
                    inv_view_proj: inv_view_proj.to_cols_array_2d(),
                    camera_pos: [self.camera.position.x, self.camera.position.y, self.camera.position.z],
                    radius: self.ssao_radius,
                    bias: 0.02,
                    strength: self.ssao_strength,
                    _padding0: [0.0, 0.0],
                };
                let bytes = std::slice::from_raw_parts(
                    &params as *const SSAOParamsGpu as *const u8,
                    std::mem::size_of::<SSAOParamsGpu>(),
                );
                let _ = ssao_cb.update_constant_buffer(bytes);
                cmd_list.SetGraphicsRootConstantBufferView(1, ssao_cb.resource.GetGPUVirtualAddress());

                cmd_list.DrawInstanced(3, 1, 0, 0);

                let ssao_to_srv = [Self::transition_barrier(
                    &ssao_texture.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                )];
                cmd_list.ResourceBarrier(&ssao_to_srv);
                for b in ssao_to_srv {
                    Self::drop_transition_barrier(b);
                }
                self.ssao_is_srv = true;

                // ДОБАВЛЕНО (по прямому запросу пользователя — устранение
                // видимой зернистости SSAO, см. подробный комментарий у
                // полей `ssao_blur_*` в engine/mod.rs): два fullscreen-
                // прохода separable-блюра СРАЗУ после сырого AO выше, пока
                // `ssao_texture` ещё PIXEL_SHADER_RESOURCE (барьер только
                // что это обеспечил). Переиспользует УЖЕ существующие
                // `bloom_blur_pipeline_state`/`bloom_root_signature`
                // (идентичны по форме — тот же RTV-формат, та же root
                // signature SRV t0 + CBV b0) вместо отдельного шейдера/PSO.
                if let (
                    Some(blur_texture),
                    Some(blur_srv_heap),
                    Some(cb_x),
                    Some(cb_y),
                    Some(blur_pso),
                    Some(blur_root_sig),
                ) = (
                    &self.ssao_blur_texture,
                    &self.ssao_blur_srv_heap,
                    &self.ssao_blur_cb_x,
                    &self.ssao_blur_cb_y,
                    &self.bloom_blur_pipeline_state,
                    &self.bloom_root_signature,
                ) {
                    let blur_width = blur_texture.width;
                    let blur_height = blur_texture.height;

                    let blur_viewport = D3D12_VIEWPORT {
                        TopLeftX: 0.0,
                        TopLeftY: 0.0,
                        Width: blur_width as f32,
                        Height: blur_height as f32,
                        MinDepth: 0.0,
                        MaxDepth: 1.0,
                    };
                    let blur_scissor = RECT {
                        left: 0,
                        top: 0,
                        right: blur_width as i32,
                        bottom: blur_height as i32,
                    };
                    cmd_list.RSSetViewports(&[blur_viewport]);
                    cmd_list.RSSetScissorRects(&[blur_scissor]);
                    cmd_list.SetGraphicsRootSignature(Some(blur_root_sig));
                    cmd_list.SetPipelineState(Some(blur_pso));
                    let blur_heaps = [Some(blur_srv_heap.clone())];
                    cmd_list.SetDescriptorHeaps(&blur_heaps);
                    cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                    // Проход 1 (горизонталь): ssao_texture (уже SRV) ->
                    // ssao_blur_texture.
                    cmd_list.OMSetRenderTargets(1, Some(&self.ssao_blur_rtv), false, None);
                    cmd_list.SetGraphicsRootDescriptorTable(0, self.ssao_blur_srv_raw_gpu);
                    cmd_list.SetGraphicsRootConstantBufferView(1, cb_x.resource.GetGPUVirtualAddress());
                    cmd_list.DrawInstanced(3, 1, 0, 0);

                    // ssao_texture больше не читается в этом кадре — сразу
                    // возвращаем в RENDER_TARGET под финальную запись
                    // прохода 2, а только что записанную ssao_blur_texture
                    // переводим в SRV — теперь она вход прохода 2.
                    let blur_to_srv = Self::transition_barrier(
                        &blur_texture.resource,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    );
                    let raw_back_to_rt = Self::transition_barrier(
                        &ssao_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    );
                    let barriers = [blur_to_srv, raw_back_to_rt];
                    cmd_list.ResourceBarrier(&barriers);
                    for b in barriers {
                        Self::drop_transition_barrier(b);
                    }

                    // Проход 2 (вертикаль): ssao_blur_texture -> ssao_texture
                    // (финальный результат — тот же слот, что уже
                    // зарегистрирован в t3 renderer.srv_uav_heap для
                    // tonemap, см. create_ssao_final_srv, повторная
                    // регистрация не нужна).
                    cmd_list.OMSetRenderTargets(1, Some(&self.ssao_rtv), false, None);
                    cmd_list.SetGraphicsRootDescriptorTable(0, self.ssao_blur_srv_mid_gpu);
                    cmd_list.SetGraphicsRootConstantBufferView(1, cb_y.resource.GetGPUVirtualAddress());
                    cmd_list.DrawInstanced(3, 1, 0, 0);

                    let ssao_final_to_srv = Self::transition_barrier(
                        &ssao_texture.resource,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    );
                    // ВАЖНО: blur_texture тоже возвращаем в RENDER_TARGET
                    // ЗДЕСЬ же, а не оставляем в PIXEL_SHADER_RESOURCE — она
                    // используется ТОЛЬКО внутри этого блока (никто больше
                    // её не читает), поэтому вместо отдельного персистентного
                    // трекера состояния (как `bloom_a_is_srv`/`ssao_is_srv`
                    // для ресурсов, живущих дольше одного блока) проще и
                    // надёжнее гарантировать, что она ВСЕГДА заканчивает
                    // кадр в том же состоянии, в котором была создана
                    // (RENDER_TARGET, см. `create_hdr_target`) — тогда
                    // следующий кадр может безусловно писать в неё Проходом
                    // 1 без разбора "а в каком она была состоянии в конце
                    // прошлого кадра". Без этого второй и все последующие
                    // кадры писали бы в неё как в RTV, когда GPU считает её
                    // PIXEL_SHADER_RESOURCE — ровно тот класс "no-op барьер
                    // не на первом кадре" бага, что уже объяснён у
                    // `bloom_a_is_srv`/`shadow_maps_are_srv` в engine/mod.rs.
                    let blur_back_to_rt = Self::transition_barrier(
                        &blur_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    );
                    let barriers = [ssao_final_to_srv, blur_back_to_rt];
                    cmd_list.ResourceBarrier(&barriers);
                    for b in barriers {
                        Self::drop_transition_barrier(b);
                    }
                    // self.ssao_is_srv остаётся true — итоговое состояние
                    // ssao_texture то же самое (PIXEL_SHADER_RESOURCE), что
                    // и без блюра, только что записанное чуть выше.
                }
            } } else if let Some(ssao_texture) = &self.ssao_texture {
                // ИСПРАВЛЕНО (баг, найденный пользователем: "при all false
                // нету изображения"): SSAO выключен — сам проход НИКОГДА не
                // рисует в `ssao_texture`, а D3D12 НЕ гарантирует
                // зануление/какое-либо конкретное содержимое свежесозданного
                // ресурса. tonemap composite БЕЗУСЛОВНО каждый кадр делает
                // `hdrColor *= aoColor` (см. pipeline_post.rs) — раз AO
                // МУЛЬТИПЛИКАТИВНЫЙ, а не аддитивный (в отличие от bloom/
                // volumetric), любое содержимое текстуры, отличное от
                // (1,1,1), портит ВСЮ картинку, а не только AO-эффект; на
                // практике Windows часто (не гарантированно) зануляет новую
                // VRAM-страницу из соображений безопасности — умножение на 0
                // даёт ПОЛНОСТЬЮ чёрный экран, что пользователь и увидел.
                // Чиним дешёвым `ClearRenderTargetView` в белый (1,1,1,1) —
                // нейтральный множитель "нет затемнения" — вместо полного
                // SSAO-прохода.
                if self.ssao_is_srv {
                    let to_rt = [Self::transition_barrier(
                        &ssao_texture.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    )];
                    cmd_list.ResourceBarrier(&to_rt);
                    for b in to_rt {
                        Self::drop_transition_barrier(b);
                    }
                }
                cmd_list.ClearRenderTargetView(self.ssao_rtv, &[1.0, 1.0, 1.0, 1.0], None);
                let to_srv = [Self::transition_barrier(
                    &ssao_texture.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                )];
                cmd_list.ResourceBarrier(&to_srv);
                for b in to_srv {
                    Self::drop_transition_barrier(b);
                }
                self.ssao_is_srv = true;
            }

            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 7);
            }
            // ДОБАВЛЕНО (отражения лучами): до TAA — он и усредняет один луч на пиксель.
            if self.rt_reflections {
                if let Some(gi) = self.gi.as_mut() {
                    let size = [renderer.hdr_resolved.width, renderer.hdr_resolved.height];
                    let cam = self.camera.position;
                    if let Err(e) = gi.record_reflections(&cmd_list, frame_index, &renderer.spec_resolved.resource, &renderer.normal_resolved.resource, &renderer.hdr_resolved.resource, size, self.taa_view_proj.inverse(), cam, 0.55) {
                        eprintln!("[GI] отражения не записаны: {:?}", e);
                    }
                }
            }

            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 8);
            }
            // ДОБАВЛЕНО (TAA): после SSAO/volumetric (они читают глубину и
            // кадр этого же кадра), до bloom/tonemap — те берут уже сглаженный.
            // ИЗМЕНЕНО (TAAU): SSAO вычитается в TAA, в разрешении кадра (см.
            // taa_cs.hlsl) — tonemap тогда его не трогает (ao_in_taa).
            let mut ao_in_taa = false;
            if self.taa || upscaling {
                if let Some(t) = self.taa_state.as_mut() {
                    let depth_state = if self.depth_stencil_is_srv { D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE } else { D3D12_RESOURCE_STATE_DEPTH_WRITE };
                    let ao = match (&self.ssao_texture, self.graphics_settings.ssao && self.ssao_is_srv) {
                        (Some(ssao), true) => Some((&renderer.ambient_resolved.resource, &ssao.resource)),
                        _ => None,
                    };
                    ao_in_taa = ao.is_some();
                    t.record(&cmd_list, &renderer.hdr_resolved.resource, &renderer.depth_stencil, depth_state, self.taa_view_proj, ao, &renderer.post_hdr().resource);
                }
            } else if let Some(t) = self.taa_state.as_mut() {
                t.reset();
            }
            // ДОБАВЛЕНО (замер экспозиции): по готовому (сглаженному) HDR-кадру.
            if self.metering {
                if self.metering_state.is_none() {
                    match super::metering::Metering::new() {
                        Ok(m) => self.metering_state = Some(m),
                        Err(e) => {
                            eprintln!("[METER] недоступен: {:?}", e);
                            self.metering = false;
                        }
                    }
                }
                if let Some(m) = self.metering_state.as_mut() {
                    let post = renderer.post_hdr();
                    m.record(&cmd_list, frame_index, &post.resource, post.width, post.height);
                }
            }
            // ДОБАВЛЕНО (локальная экспозиция): сетка яркости по готовому кадру.
            if self.lens.local_exposure > 0.0 && !self.local_exposure_unavailable {
                if self.local_exposure_state.is_none() {
                    match super::local_exposure::LocalExposure::new() {
                        Ok(l) => self.local_exposure_state = Some(l),
                        Err(e) => {
                            eprintln!("[LOCALEXP] недоступна: {:?}", e);
                            self.local_exposure_unavailable = true;
                        }
                    }
                }
                if let Some(l) = self.local_exposure_state.as_mut() {
                    let post = renderer.post_hdr();
                    l.record(&cmd_list, &post.resource, post.width, post.height, &renderer.srv_uav_heap);
                }
            }


            // ДОБАВЛЕНО (по прямому запросу пользователя — переключаемые
            // графические настройки): та же причина, что у SSAO/volumetric
            // выше — `graphics_settings.bloom` проверяется здесь, а не
            // через обнуление `bloom_texture_a` (см.
            // `disable_bloom_for_diagnostics`, для разовой ручной
            // диагностики) — иначе SRV `bloom_texture_a`, которую
            // БЕЗУСЛОВНО каждый кадр читает tonemap composite (t1),
            // указывала бы на уничтоженный ресурс.
            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 9);
            }
            // ИЗМЕНЕНО (bloom цепочкой уровней, Jimenez 2014): яркое
            // выделяется сразу с понижением до half-res (уровень 0), затем
            // понижается до мелких уровней и поднимается обратно тент-фильтром
            // со сложением. Было: один гауссов блюр 9 выборок point-сэмплером,
            // и все проходы брали параметры из ОДНОГО constant buffer, который
            // к исполнению списка содержал последнее значение — оба "разделимых"
            // блюра шли по вертикали (вертикальные полосы у ярких точек).
            // Параметры теперь — root constants (снимок на каждый draw).
            if self.graphics_settings.bloom { if let (Some(bloom_a), Some(bloom_srv_heap)) =
                (&self.bloom_texture_a, &self.bloom_srv_heap)
            {
                // Уровни сверху вниз: (ресурс, RTV, SRV, ширина, высота) и их состояния.
                let mut levels = vec![(bloom_a.resource.clone(), self.bloom_rtv_a, self.bloom_srv_a_gpu, bloom_a.width, bloom_a.height)];
                levels.extend(self.bloom_mips.iter().map(|m| (m.texture.resource.clone(), m.rtv, m.srv_gpu, m.texture.width, m.texture.height)));
                let mut is_srv: Vec<bool> = std::iter::once(self.bloom_a_is_srv).chain(self.bloom_mips.iter().map(|m| m.is_srv)).collect();
                let set_state = |is_srv: &mut Vec<bool>, k: usize, srv: bool| {
                    if is_srv[k] != srv {
                        let (before, after) = if srv {
                            (D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)
                        } else {
                            (D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET)
                        };
                        let barriers = [Self::transition_barrier(&levels[k].0, before, after)];
                        cmd_list.ResourceBarrier(&barriers);
                        for b in barriers {
                            Self::drop_transition_barrier(b);
                        }
                        is_srv[k] = srv;
                    }
                };
                // Рисовать в уровень `k`: состояние, вьюпорт, таргет.
                let target = |is_srv: &mut Vec<bool>, k: usize| {
                    set_state(is_srv, k, false);
                    let (_, rtv, _, w, h) = levels[k];
                    cmd_list.RSSetViewports(&[D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w as f32, Height: h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]);
                    cmd_list.RSSetScissorRects(&[RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 }]);
                    cmd_list.OMSetRenderTargets(1, Some(&rtv), false, None);
                };
                let constants = |params: [f32; 4]| {
                    cmd_list.SetGraphicsRoot32BitConstants(1, 4, params.as_ptr() as *const _, 0);
                };
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                // Выделение яркого с понижением полноразмерного HDR до уровня 0.
                // Каждый уровень несёт ту же энергию, при сложении их
                // `levels.len()` — поэтому доля на уровень 1/len.
                // ИЗМЕНЕНО (MSAA): читается `hdr_resolved` (renderer.hdr_srv_gpu),
                // уже в PIXEL_SHADER_RESOURCE после resolve основного прохода.
                target(&mut is_srv, 0);
                cmd_list.SetGraphicsRootSignature(Some(self.bloom_extract_root_signature.as_ref().unwrap()));
                cmd_list.SetPipelineState(Some(self.bloom_extract_pipeline_state.as_ref().unwrap()));
                let hdr_heap = [Some(renderer.srv_uav_heap.clone())];
                cmd_list.SetDescriptorHeaps(&hdr_heap);
                cmd_list.SetGraphicsRootDescriptorTable(0, renderer.hdr_srv_gpu);
                constants([1.0 / self.width.max(1) as f32, 1.0 / self.height.max(1) as f32, 1.0, 1.0 / levels.len() as f32]);
                cmd_list.DrawInstanced(3, 1, 0, 0);
                set_state(&mut is_srv, 0, true);

                cmd_list.SetGraphicsRootSignature(Some(self.bloom_chain_root_signature.as_ref().unwrap()));
                let bloom_heap = [Some(bloom_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&bloom_heap);
                // Вниз: k -> k+1.
                cmd_list.SetPipelineState(Some(self.bloom_down_pipeline_state.as_ref().unwrap()));
                for k in 0..levels.len() - 1 {
                    target(&mut is_srv, k + 1);
                    let (_, _, src, w, h) = levels[k];
                    cmd_list.SetGraphicsRootDescriptorTable(0, src);
                    constants([1.0 / w as f32, 1.0 / h as f32, 0.0, 0.0]);
                    cmd_list.DrawInstanced(3, 1, 0, 0);
                    set_state(&mut is_srv, k + 1, true);
                }
                // Вверх: k+1 прибавляется к k.
                cmd_list.SetPipelineState(Some(self.bloom_up_pipeline_state.as_ref().unwrap()));
                for k in (0..levels.len() - 1).rev() {
                    target(&mut is_srv, k);
                    let (_, _, src, w, h) = levels[k + 1];
                    cmd_list.SetGraphicsRootDescriptorTable(0, src);
                    constants([1.0 / w as f32, 1.0 / h as f32, 0.0, 0.0]);
                    cmd_list.DrawInstanced(3, 1, 0, 0);
                    set_state(&mut is_srv, k, true);
                }

                self.bloom_a_is_srv = is_srv[0];
                for (m, s) in self.bloom_mips.iter_mut().zip(&is_srv[1..]) {
                    m.is_srv = *s;
                }
            } } else if let Some(bloom_a) = &self.bloom_texture_a {
                // ИСПРАВЛЕНО (та же причина, что у SSAO/volumetric выше):
                // bloom тоже АДДИТИВНЫЙ (`bloomColor * bloomIntensity`,
                // складывается, не умножается) — чистим только
                // `bloom_texture_a` (единственная, что реально читает
                // tonemap через уже зарегистрированный `bloom_final_srv_cpu`,
                // см. `create_bloom_resources`) в (0,0,0,0), `bloom_texture_b`
                // — чисто внутренний scratch этого прохода, никем больше не
                // читается, трогать не нужно.
                if self.bloom_a_is_srv {
                    let to_rt = [Self::transition_barrier(
                        &bloom_a.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    )];
                    cmd_list.ResourceBarrier(&to_rt);
                    for b in to_rt {
                        Self::drop_transition_barrier(b);
                    }
                }
                cmd_list.ClearRenderTargetView(self.bloom_rtv_a, &[0.0, 0.0, 0.0, 0.0], None);
                let to_srv = [Self::transition_barrier(
                    &bloom_a.resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                )];
                cmd_list.ResourceBarrier(&to_srv);
                for b in to_srv {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_a_is_srv = true;
            }

            // ИЗМЕНЕНО (MSAA): раньше здесь условно (если bloom не рисовал)
            // переводили `hdr_target` в PIXEL_SHADER_RESOURCE для tonemap-
            // прохода — не нужно, tonemap читает `hdr_resolved`
            // (`renderer.hdr_srv_gpu`), которая уже в PIXEL_SHADER_RESOURCE
            // после resolve-шага в начале кадра, независимо от того, бежал
            // ли bloom.
            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 10);
            }
            let back_buffer_resource = &renderer.back_buffers[real_back_buffer_index].resource;

            let barriers_before = [Self::transition_barrier(
                back_buffer_resource,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )];
            cmd_list.ResourceBarrier(&barriers_before);
            for b in barriers_before {
                Self::drop_transition_barrier(b);
            }

            let back_buffer_rtv = renderer.render_target_views[real_back_buffer_index];
            cmd_list.OMSetRenderTargets(1, Some(&back_buffer_rtv), false, None);

            let viewport = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            cmd_list.RSSetViewports(&[viewport]);
            let scissor = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            cmd_list.RSSetScissorRects(&[scissor]);

            cmd_list.SetPipelineState(Some(self.tonemap_pipeline_state.as_ref().unwrap()));
            cmd_list.SetGraphicsRootSignature(Some(self.tonemap_root_signature.as_ref().unwrap()));

            let srv_heaps = [Some(renderer.srv_uav_heap.clone())];
            cmd_list.SetDescriptorHeaps(&srv_heaps);
            cmd_list.SetGraphicsRootDescriptorTable(0, renderer.hdr_srv_gpu);

            if let Some(settings) = &self.light_global_settings {
                if let Some(cb) = &self.tonemap_constant_buffer {
                    // ДОБАВЛЕНО (хроматическая адаптация): строки матрицы 3x3 в b0 после exposure/bloom.
                    self.lens_frame = self.lens_frame.wrapping_add(1);
                    let a = chromatic_adaptation(self.white_point);
                    let (vignette, aspect) = if self.lens.vignetting { ((self.camera.fov * 0.5).tan(), self.camera.aspect) } else { (0.0, 1.0) };
                    // локальная экспозиция: к чему подтягивать — средняя яркость
                    // кадра после экспозиции (по замеру), без замера — средне-серый
                    let local_on = self.lens.local_exposure > 0.0 && self.local_exposure_state.is_some();
                    let local_key = self.metering_state.as_ref().and_then(|m| m.last).map(|l| l * settings.exposure).unwrap_or(0.18);
                    // размытие в движении: матрицы кадра и прошлого, обе без сдвига TAA
                    let mb_vp = self.camera.projection_matrix() * self.camera.view_matrix();
                    let mb_prev = self.motion_prev_view_proj.unwrap_or(mb_vp);
                    self.motion_prev_view_proj = Some(mb_vp);
                    let mb_on = self.lens.motion_blur > 0.0 && renderer.depth_stencil.sample_count == 1 && self.depth_stencil_is_srv;
                    let tonemap_data: [f32; 60] = [
                        settings.exposure, settings.bloom_intensity, vignette, aspect,
                        a[0][0], a[0][1], a[0][2], self.lens.sensor_noise,
                        a[1][0], a[1][1], a[1][2], (self.lens_frame % 4096) as f32,
                        a[2][0], a[2][1], a[2][2], if self.lens.aces_full { 1.0 } else { 0.0 },
                        self.lens.vision_cd_per_unit, self.lens.distortion, self.lens.chromatic_aberration, 0.0,
                        if local_on { self.lens.local_exposure } else { 0.0 }, local_key, if ao_in_taa { 1.0 } else { 0.0 }, 0.0,
                        // mbInvViewProj, mbPrevViewProj (по столбцам — как у остальных
                        // матриц движка), mbParams
                        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        if mb_on { self.lens.motion_blur } else { 0.0 }, 0.05, 0.0, 0.0,
                    ];
                    let mut tonemap_data = tonemap_data;
                    tonemap_data[24..40].copy_from_slice(&mb_vp.inverse().to_cols_array());
                    tonemap_data[40..56].copy_from_slice(&mb_prev.to_cols_array());
                    let bytes = std::slice::from_raw_parts(tonemap_data.as_ptr() as *const u8, 240);
                    if let Err(e) = cb.update_constant_buffer(bytes) {
                        eprintln!("[ENGINE] WARNING: не удалось обновить tonemap_constant_buffer: {:?}", e);
                    }
                }
            }
            if let Some(cb) = &self.tonemap_constant_buffer {
                let gpu_addr = cb.resource.GetGPUVirtualAddress();
                cmd_list.SetGraphicsRootConstantBufferView(1, gpu_addr);
            }

            cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            cmd_list.DrawInstanced(3, 1, 0, 0);

            // ИЗМЕНЕНО (MSAA): `hdr_target` больше не транзитится здесь —
            // она уже вернулась в RENDER_TARGET сразу после resolve-шага
            // в начале кадра (см. его комментарий) и с тех пор не
            // трогалась вообще; `hdr_resolved` НИКОГДА не покидает
            // PIXEL_SHADER_RESOURCE после resolve-шага (и не должна —
            // именно в этом состоянии её ожидает начало СЛЕДУЮЩЕГО
            // кадра).
            // ИЗМЕНЕНО (размытие в движении): возврат перенесён в конец кадра —
            // глубину читает и tonemap.
            // Оба прохода выше (volumetric/SSAO) закончили читать depth —
            // возвращаем его в DEPTH_WRITE ОДИН раз (а не дважды за кадр),
            // если он реально был переведён в SRV хотя бы одним из них.
            if self.depth_stencil_is_srv {
                let depth_back = [Self::transition_barrier(
                    &renderer.depth_stencil.resource,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_DEPTH_WRITE,
                )];
                cmd_list.ResourceBarrier(&depth_back);
                for b in depth_back {
                    Self::drop_transition_barrier(b);
                }
                self.depth_stencil_is_srv = false;
            }
            let mut barriers_after = vec![
                Self::transition_barrier(
                    back_buffer_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PRESENT,
                ),
            ];
            for cascade in 0..NUM_CASCADES {
                if let Some(shadow_map) = &self.shadow_maps[cascade] {
                    if self.shadow_maps_are_srv[cascade] {
                        barriers_after.push(Self::transition_barrier(
                            &shadow_map.resource,
                            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                            D3D12_RESOURCE_STATE_DEPTH_WRITE,
                        ));
                        self.shadow_maps_are_srv[cascade] = false;
                    }
                }
            }
            if let Some(atlas) = &self.spot_shadow_atlas {
                if self.spot_shadow_atlas_is_srv {
                    barriers_after.push(Self::transition_barrier(
                        &atlas.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                    ));
                    self.spot_shadow_atlas_is_srv = false;
                }
            }
            if let Some(atlas) = &self.point_shadow_atlas {
                if self.point_shadow_atlas_is_srv {
                    barriers_after.push(Self::transition_barrier(
                        &atlas.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_DEPTH_WRITE,
                    ));
                    self.point_shadow_atlas_is_srv = false;
                }
            }
            cmd_list.ResourceBarrier(&barriers_after);
            super::screenshot::record(&cmd_list, back_buffer_resource);
            if perf_enabled() {
                super::gpu_timer::mark(&cmd_list, frame_index, 11);
                super::gpu_timer::end(&cmd_list, frame_index);
            }
            for b in barriers_after {
                Self::drop_transition_barrier(b);
            }

            if let Err(e) = cmd_list.Close() {
                eprintln!("[ENGINE] cmd_list.Close() failed: {:?}", e);
                crate::dump_d3d12_debug_messages();
                return Err(e);
            }
        }

        let queue = crate::get_command_queue()?;

        let cmd_lists: &[Option<ID3D12CommandList>] = &[Some(cmd_list.into())];
        unsafe {
            queue.ExecuteCommandLists(cmd_lists);
        }
        let perf_record_ms = perf_t_record.elapsed().as_secs_f64() * 1000.0;

        let swap_chain = crate::get_swap_chain()?;

        let perf_t_present = Instant::now();
        let late = !no_vsync() && present_late(perf_t_present);
        unsafe {
            // без vsync и с поддержкой tearing — Present не ждёт монитор (см. swap_chain.rs)
            let hr = if no_vsync() {
                let flags = if crate::swap_chain::tearing_supported() { windows::Win32::Graphics::Dxgi::DXGI_PRESENT_ALLOW_TEARING } else { DXGI_PRESENT(0) };
                swap_chain.Present(0, flags)
            } else if late {
                swap_chain.Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT_ALLOW_TEARING)
            } else {
                swap_chain.Present(1, DXGI_PRESENT(0))
            };
            if hr.is_err() {
                eprintln!("[ENGINE] Present failed: {:?}", hr);
                if let Some(reason) = crate::device_removed_reason() {
                    eprintln!("[ENGINE] Device removed, reason: {}", reason);
                }
                crate::dump_d3d12_debug_messages();
                // ДОБАВЛЕНО (диагностика "кадр 2" DXGI_ERROR_DEVICE_HUNG,
                // см. комментарий у `dump_dred_report`): обычный debug
                // layer выше не называет причину зависания, только сам
                // факт — DRED называет конкретную GPU-команду/адрес.
                crate::dump_dred_report();
                return Err(Error::from_hresult(hr));
            }
        }

        let fence = crate::get_fence()?;
        let fence_value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
        unsafe {
            if let Err(e) = queue.Signal(&fence, fence_value) {
                // ДОБАВЛЕНО (закрывает пробел в диагностике: до этого
                // ошибка ЗДЕСЬ пропагировалась голым `?` без единого
                // eprintln/dump'а — при живой отладке main_car один из
                // прогонов упал именно тут, тихо, без Present-failed и
                // без DRED в логе, что затруднило диагностику). Present()
                // асинхронен и мог УСПЕШНО вернуться, даже если устройство
                // потерялось буквально сразу после — этот `Signal` часто
                // первое место, где это станет заметно.
                eprintln!("[ENGINE] queue.Signal() failed: {:?}", e);
                if let Some(reason) = crate::device_removed_reason() {
                    eprintln!("[ENGINE] Device removed, reason: {}", reason);
                }
                crate::dump_d3d12_debug_messages();
                crate::dump_dred_report();
                return Err(e);
            }
        }
        if frame_index < self.frame_fence_values.len() {
            self.frame_fence_values[frame_index] = fence_value;
        }
        super::screenshot::save(&fence, fence_value);

        {
            let mut state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
            }
        }

        if perf_enabled() {
            perf_frame(perf_wait_ms, perf_record_ms, perf_t_present.elapsed().as_secs_f64() * 1000.0, late,
                       perf_main_draws, perf_main_tris, perf_csm_draws, total_local_shadow_draws);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod local_shadow_cache_tests {
    use super::*;

    fn light(x: f32) -> GPULight {
        GPULight { position: [x, 5.0, 0.0, 0.0], color: [1.0; 4], direction: [0.0, -1.0, 0.0, 20.0], params: [0.0; 4] }
    }

    #[test]
    fn stable_slots_keep_lights_in_place() {
        // кадр 1: фонари A(10) B(20) C(30) — слоты 0,1,2
        let mut prev = Vec::new();
        let l1 = vec![light(10.0), light(20.0), light(30.0)];
        let s1 = stable_slots(vec![(0, ()), (1, ()), (2, ())], &l1, &mut prev);
        assert_eq!(s1.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 1, 2]);
        // кадр 2: другой порядок в списке видимых и B выпал, пришёл D(40): A и C держат слоты 0 и 2, D — в слот 1
        let l2 = vec![light(40.0), light(30.0), light(10.0)];
        let s2 = stable_slots(vec![(1, ()), (2, ()), (0, ())], &l2, &mut prev);
        let xs: Vec<f32> = s2.iter().map(|(i, _)| l2[*i].position[0]).collect();
        assert_eq!(xs, vec![10.0, 40.0, 30.0]);
    }

    #[test]
    fn fingerprint_changes_only_when_content_changes() {
        let vp = Mat4::IDENTITY;
        let jobs = vec![(3usize, Mat4::IDENTITY), (5usize, Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)))];
        let a = tile_fingerprint(&vp, &[0, 1], &jobs);
        assert_eq!(a, tile_fingerprint(&vp, &[0, 1], &jobs));
        let moved = vec![(3usize, Mat4::IDENTITY), (5usize, Mat4::from_translation(Vec3::new(1.5, 0.0, 0.0)))];
        assert_ne!(a, tile_fingerprint(&vp, &[0, 1], &moved));
        assert_ne!(a, tile_fingerprint(&vp, &[0], &jobs));
    }
}

/// ДОБАВЛЕНО (хроматическая адаптация): матрица (строки), переводящая цвет
/// при освещении белым `white` (линейный sRGB) в то, каким его видит
/// привыкший к этому свету глаз: sRGB -> XYZ -> LMS по Брэдфорду, деление
/// на отклик колбочек на `white`, умножение на отклик на D65, обратно.
pub(crate) fn chromatic_adaptation(white: [f32; 3]) -> [[f32; 3]; 3] {
    use crate::math::{Mat3, Vec3};
    // столбцами: glam хранит матрицы по столбцам
    let srgb_to_xyz = Mat3::from_cols(
        Vec3::new(0.4124564, 0.2126729, 0.0193339),
        Vec3::new(0.3575761, 0.7151522, 0.1191920),
        Vec3::new(0.1804375, 0.0721750, 0.9503041),
    );
    let bradford = Mat3::from_cols(
        Vec3::new(0.8951, -0.7502, 0.0389),
        Vec3::new(0.2664, 1.7135, -0.0685),
        Vec3::new(-0.1614, 0.0367, 1.0296),
    );
    let m = bradford * srgb_to_xyz;
    let w = Vec3::from(white).max(Vec3::splat(1e-4));
    let gain = (m * Vec3::ONE) / (m * w);
    let a = m.inverse() * Mat3::from_diagonal(gain) * m;
    let r = a.transpose();
    [r.x_axis.to_array(), r.y_axis.to_array(), r.z_axis.to_array()]
}
