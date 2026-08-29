// src/texture.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT, DXGI_SAMPLE_DESC};
// ДОБАВЛЕНО (Задача #15 — исправление бага "всё чёрное после текстур"):
// нужны для синхронного ожидания GPU-копирования в `upload_and_copy` —
// короткоживущий fence + Win32 event (событийное ожидание, а не
// busy-wait спин-цикл на `GetCompletedValue()`, который используется для
// per-frame fence'ов в `engine/mod.rs` — здесь это разовая, редкая
// операция при загрузке текстуры, а не по кадру, поэтому чуть более
// тяжёлая настройка event-объекта оправдана экономией CPU на ожидании).
// ИЗМЕНЕНО (фикс "белого окна" + краша драйвера — см. подробный
// комментарий у `wait_for_fence` в engine/mod.rs про общий класс бага):
// `INFINITE` здесь раньше означало, что если GPU по любой причине
// перестаёт продвигать fence (TDR, зависший драйвер, device removed) во
// время загрузки ЛЮБОЙ PBR-текстуры (.altex с albedo/normal/metallic-
// roughness картами — Задача #15), поток, вызвавший `upload_and_copy`,
// блокируется здесь НАВСЕГДА. Если это главный поток (а `load_object_mesh`
// вызывается именно из него, синхронно, при загрузке .altex) — это тот же
// класс зависания message loop/окна, что был исправлен для per-frame
// fence'ов в engine/mod.rs. Заменяем `INFINITE` на ограниченный таймаут
// (`WAIT_TIMEOUT_MS`) — при его истечении возвращаем явную ошибку вместо
// вечной блокировки.
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::Foundation::{HANDLE, WAIT_TIMEOUT};
use crate::STATE;

// ДОБАВЛЕНО (оптимизация — жалоба пользователя на лаги/просадки FPS при
// загрузке новых чанков world streaming): `upload_and_copy` раньше
// создавала НОВЫЙ `CreateEventW` win32-объект (kernel syscall, дороже,
// чем кажется — переход в режим ядра, аллокация object header'а) и сразу
// же `CloseHandle` его на КАЖДЫЙ вызов — то есть на каждую загружаемую
// текстуру каждого материала каждого объекта каждого чанка. Загрузка
// объекта .altex с PBR-материалом (albedo+normal+metallic-roughness —
// Задача #15) создаёт до 3 текстур за раз, а `load_chunk` (engine/mod.rs)
// может грузить несколько таких объектов в одном кадре при входе камеры
// в новый чанк. Event-объект переиспользуем — создаём один раз на поток
// (загрузка ресурсов всегда идёт из главного потока движка, отдельного
// потока рендера или загрузки в этом движке нет) и просто сбрасываем его
// автосбросом (`bManualReset = false` в `CreateEventW` — событие само
// сбрасывается в non-signaled после того, как `WaitForSingleObject`
// однажды его дождался, так что переиспользование безопасно раз за
// разом). Семантика ожидания (fence + `SetEventOnCompletion` +
// `WaitForSingleObject` с тем же `WAIT_TIMEOUT_MS`) не меняется —
// убирается только стоимость create/close самого handle.
thread_local! {
    static UPLOAD_WAIT_EVENT: HANDLE = unsafe {
        CreateEventW(None, false, false, None)
            .expect("[TEXTURE] не удалось создать переиспользуемый event для ожидания GPU-загрузки текстур")
    };
}

/// См. комментарий у импорта `WaitForSingleObject` выше — максимальное
/// время ожидания однократного GPU-копирования текстуры при загрузке.
/// 5 секунд — тот же порог, что используется для per-frame ожиданий в
/// engine/mod.rs (`wait_for_fence`) и для `shutdown()`; загрузка одной
/// текстуры физически не может занимать дольше на живом GPU.
const WAIT_TIMEOUT_MS: u32 = 5000;

// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): этот файл существовал, но
// не был подключён к дереву модулей вообще (см. mod texture в lib.rs) —
// сиротский, но рабочий код. Расширяю его view-методами (RTV/SRV/UAV),
// нужными для HDR render target (compile_default_shaders рисует в него, а
// не в back buffer напрямую) и промежуточных bloom-текстур (compute-шейдер
// одновременно читает через SRV предыдущий mip/этап и пишет через UAV в
// следующий).

pub struct Texture {
    pub resource: ID3D12Resource,
    pub width: u32,
    pub height: u32,
    pub format: DXGI_FORMAT,
    pub mip_levels: u32,
}

impl Texture {
    /// ИСПРАВЛЕНО (баг "после подключения текстур всё чёрное на экране",
    /// найдено и подтверждено на реальной машине пользователя): раньше при
    /// `data.is_some()` (единственный путь, которым реально пользуется
    /// Задача #15 — текстуры и PBR-материалы, ДО неё эта ветка не была
    /// нигде задействована) текстура целиком создавалась в UPLOAD heap
    /// (`D3D12_HEAP_TYPE_UPLOAD`) с состоянием `GENERIC_READ`, и именно
    /// этот же ресурс потом напрямую использовался для `CreateShaderResourceView`
    /// (см. `create_srv` ниже) — то есть шейдер должен был сэмплировать
    /// Texture2D ПРЯМО ИЗ upload heap. Формально `GENERIC_READ` включает
    /// `PIXEL_SHADER_RESOURCE`, но UPLOAD heap физически не предназначен
    /// для чтения через сэмплер текстурным блоком GPU (в отличие от
    /// табличного/буферного чтения) — на реальном железе это либо не
    /// поддерживается вовсе, либо даёт непредсказуемый результат
    /// (типично — сплошной чёрный/нулевой цвет, т.к. текстурный юнит не
    /// может корректно адресовать row-major/linear-layout память upload
    /// heap так, как ожидает swizzled/opaque layout обычной текстуры).
    /// Стандартный и единственный физически корректный путь: текстура
    /// живёт в DEFAULT heap (GPU-локальная память, опаковый layout), а
    /// исходные байты сначала загружаются во ВРЕМЕННЫЙ upload-буфер и
    /// копируются в неё командой `CopyTextureRegion` — этот метод теперь
    /// делает ровно это, синхронно (создаёт короткоживущие command
    /// allocator/list/fence, ждёт завершения копирования перед возвратом),
    /// чтобы вызывающий код (`register_material_texture` и т.п.) мог
    /// использовать результат сразу же, как и раньше.
    pub fn create_texture2d(width: u32, height: u32, format: DXGI_FORMAT, data: Option<&[u8]>) -> Result<Self> {
        // УБРАНО (оптимизация — см. подробный комментарий в
        // create_vertex_buffer в buffer.rs про ту же причину): success-
        // логи убраны из hot path загрузки материалов чанков. Ошибочные
        // пути (`eprintln!`) оставлены.
        let device = crate::get_device()?;

        // ДОБАВЛЕНО (мипмапы — фикс алиасинга текстур материалов на
        // удалении, см. подробный комментарий у `generate_mip_chain`
        // ниже): полная цепочка мипов нужна ТОЛЬКО когда реально есть
        // исходные пиксели, которые можно продаунсемплить (загруженная
        // текстура материала). Ветка `data.is_none()` — это render-target-
        // текстуры (см. комментарий выше про "будущие render-target-
        // текстуры") — для них поведение НЕ меняется, остаётся ровно
        // `MipLevels: 1`, как было всегда.
        let mip_levels = if data.is_some() {
            Self::compute_mip_levels(width, height)
        } else {
            1
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height as u32,
            DepthOrArraySize: 1,
            MipLevels: mip_levels as u16,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        // Текстура ВСЕГДА в DEFAULT heap теперь (независимо от того, есть
        // ли исходные данные) — с данными она первично создаётся в
        // COPY_DEST (получатель `CopyTextureRegion` ниже), затем
        // переводится барьером в PIXEL_SHADER_RESOURCE перед возвратом;
        // без данных (например под будущие render-target-текстуры,
        // которые этот метод тоже обслуживает) — сразу в COMMON, как и
        // раньше.
        let heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let initial_state = if data.is_some() {
            D3D12_RESOURCE_STATE_COPY_DEST
        } else {
            D3D12_RESOURCE_STATE_COMMON
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                initial_state,
                None,
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: Resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;

            if let Some(bytes) = data {
                // ИСПРАВЛЕНО (защита от паники): раньше здесь было
                // `bytes[(y * row_pitch)..]` без проверки, что во входных
                // данных реально хватает байт на все строки — при
                // несоответствии размера это была паника на срезе.
                // Теперь размер проверяется заранее и функция возвращает
                // явную ошибку.
                let row_pitch = (width * 4) as usize;
                let required_len = row_pitch * height as usize;
                if bytes.len() < required_len {
                    eprintln!(
                        "[TEXTURE] ERROR: input data too small: got {} bytes, need at least {} ({}x{}x4)",
                        bytes.len(), required_len, width, height
                    );
                    return Err(Error::from_hresult(HRESULT(1)));
                }

                let mip_chain = Self::generate_mip_chain(width, height, bytes);
                Self::upload_and_copy_mips(&device, &resource, format, &mip_chain)?;
            }

            Ok(Self {
                resource,
                width,
                height,
                format,
                mip_levels,
            })
        }
    }

    /// ДОБАВЛЕНО (мипмапы — фикс алиасинга/мерцания текстур материалов на
    /// удалении от камеры): раньше ВСЕ текстуры создавались с
    /// `MipLevels: 1` — то есть материалов-сэмплер (`material_sampler` в
    /// engine/mod.rs, `D3D12_FILTER_MIN_MAG_MIP_LINEAR` — УЖЕ настроен на
    /// трилинейную фильтрацию с мипами) физически нечего было выбирать
    /// между уровнями, всегда сэмплировался только mip 0 в полном
    /// разрешении — классическая причина муара/мерцания текстур на
    /// удалении. Стандартная формула количества уровней: 1 +
    /// floor(log2(max(width, height))), т.е. цепочка идёт до 1×1
    /// включительно.
    fn compute_mip_levels(width: u32, height: u32) -> u32 {
        let max_dim = width.max(height).max(1);
        32 - max_dim.leading_zeros()
    }

    /// Генерирует полную цепочку мипов из исходного RGBA8-изображения
    /// простым box-фильтром 2×2 (усреднение 4 соседних текселей
    /// предыдущего уровня — стандартный, самый распространённый способ).
    /// Каждый следующий уровень вдвое меньше по стороне (округление вниз,
    /// не меньше 1), пока не дойдёт до 1×1. Уровень 0 — копия исходных
    /// данных без изменений. Индексы результата совпадают с индексами
    /// subresource в D3D12 (mip 0 — элемент 0, и т.д.).
    ///
    /// Считается на CPU, а не GPU-проходом (в отличие от bloom/tonemap в
    /// движке) — сознательно: текстуры материалов подгружаются постепенно,
    /// по мере стриминга чанков мира, а не каждый кадр, поэтому здесь не
    /// нужны ни новый шейдер, ни новая root signature/PSO, ни
    /// динамическое выделение RTV/SRV на каждую загружаемую текстуру —
    /// заметно меньше нового кода для одной и той же цели. Стоимость на
    /// CPU для одной текстуры материала пренебрежимо мала (полная
    /// цепочка добавляет всего ~33% данных сверх одного mip 0).
    fn generate_mip_chain(width: u32, height: u32, base: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
        let mut levels: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        levels.push((width.max(1), height.max(1), base.to_vec()));

        loop {
            let (prev_w, prev_h, prev_data) = levels.last().unwrap();
            let (prev_w, prev_h) = (*prev_w, *prev_h);
            if prev_w == 1 && prev_h == 1 {
                break;
            }
            let next_w = (prev_w / 2).max(1);
            let next_h = (prev_h / 2).max(1);

            let mut next_data = vec![0u8; (next_w * next_h * 4) as usize];
            for y in 0..next_h {
                for x in 0..next_w {
                    // Клэмп к границам предыдущего уровня — корректно
                    // обрабатывает НЕ-степени-двойки (например 5×3), где
                    // последний ряд/столбец 2×2-блока может выходить за
                    // пределы исходного изображения.
                    let sx0 = (x * 2).min(prev_w - 1);
                    let sy0 = (y * 2).min(prev_h - 1);
                    let sx1 = (x * 2 + 1).min(prev_w - 1);
                    let sy1 = (y * 2 + 1).min(prev_h - 1);

                    let mut sum = [0u32; 4];
                    for (sx, sy) in [(sx0, sy0), (sx1, sy0), (sx0, sy1), (sx1, sy1)] {
                        let idx = ((sy * prev_w + sx) * 4) as usize;
                        for c in 0..4 {
                            sum[c] += prev_data[idx + c] as u32;
                        }
                    }
                    let out_idx = ((y * next_w + x) * 4) as usize;
                    for c in 0..4 {
                        next_data[out_idx + c] = (sum[c] / 4) as u8;
                    }
                }
            }

            levels.push((next_w, next_h, next_data));
        }

        levels
    }

    /// Обобщение `upload_and_copy` на ПОЛНУЮ цепочку мипов вместо одного
    /// уровня — тот же проверенный паттерн (upload heap на каждый уровень
    /// → `CopyTextureRegion` в соответствующий subresource → один барьер
    /// COPY_DEST->PIXEL_SHADER_RESOURCE на ВСЕ subresource сразу → одно
    /// исполнение command list → один fence-wait), просто в цикле по
    /// уровням внутри ОДНОГО command list вместо одного вызова на
    /// уровень — так GPU выполняет все копирования одним batch'ем, а не
    /// N отдельными synchronous round-trip'ами.
    unsafe fn upload_and_copy_mips(
        device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
        dst_resource: &ID3D12Resource,
        format: DXGI_FORMAT,
        mips: &[(u32, u32, Vec<u8>)],
    ) -> Result<()> {
        const D3D12_TEXTURE_DATA_PITCH_ALIGNMENT: usize = 256;

        let command_allocator: ID3D12CommandAllocator =
            device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
        let command_list: ID3D12GraphicsCommandList =
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &command_allocator, None)?;

        // ВАЖНО: держим все upload-буферы живыми до самого fence-wait —
        // `CopyTextureRegion`, записанный в command list, ссылается на них
        // по GPU-адресу; если буфер освободится раньше, чем GPU реально
        // выполнит копирование, это use-after-free видеопамяти.
        let mut upload_resources: Vec<ID3D12Resource> = Vec::with_capacity(mips.len());

        for (mip_index, (mip_w, mip_h, pixels)) in mips.iter().enumerate() {
            let row_pitch = (*mip_w * 4) as usize;
            let aligned_row_pitch = (row_pitch + D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1)
                & !(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1);
            let upload_size = aligned_row_pitch * (*mip_h as usize);

            let upload_heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
            let upload_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Alignment: 0,
                Width: upload_size as u64,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT(0), // DXGI_FORMAT_UNKNOWN — буфер, не текстура
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Flags: D3D12_RESOURCE_FLAG_NONE,
            };

            let mut upload_resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &upload_heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &upload_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut upload_resource,
            )?;
            let upload_resource = upload_resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: upload staging resource is None (mip {})!", mip_index);
                Error::from_hresult(HRESULT(1))
            })?;

            let mut mapped = std::ptr::null_mut();
            upload_resource.Map(0, None, Some(&mut mapped))?;
            if mapped.is_null() {
                eprintln!("[TEXTURE] ERROR: staging buffer mapped pointer is null (mip {})!", mip_index);
                return Err(Error::from_hresult(HRESULT(1)));
            }
            for y in 0..*mip_h as usize {
                let src = &pixels[(y * row_pitch)..(y * row_pitch + row_pitch)];
                let dst = (mapped as *mut u8).add(y * aligned_row_pitch);
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst, row_pitch);
            }
            upload_resource.Unmap(0, None);

            let footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: 0,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: format,
                    Width: *mip_w,
                    Height: *mip_h,
                    Depth: 1,
                    RowPitch: aligned_row_pitch as u32,
                },
            };

            let src_location = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(upload_resource.clone())),
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint },
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            };
            let dst_location = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::ManuallyDrop::new(Some(dst_resource.clone())),
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: mip_index as u32 },
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            };

            command_list.CopyTextureRegion(&dst_location, 0, 0, 0, &src_location, None);

            {
                let mut src_location = src_location;
                let mut dst_location = dst_location;
                std::mem::ManuallyDrop::drop(&mut src_location.pResource);
                std::mem::ManuallyDrop::drop(&mut dst_location.pResource);
            }

            upload_resources.push(upload_resource);
        }

        // Один барьер на ВСЕ subresource сразу (D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES) —
        // после того, как все CopyTextureRegion для всех уровней уже
        // записаны в command list выше.
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: std::mem::ManuallyDrop::new(Some(dst_resource.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                    StateAfter: D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                }),
            },
        };
        let mut barrier = barrier;
        command_list.ResourceBarrier(&[barrier.clone()]);
        std::mem::ManuallyDrop::drop(&mut barrier.Anonymous.Transition);

        command_list.Close()?;

        let command_queue = crate::get_command_queue()?;
        let cmd_lists: [Option<ID3D12CommandList>; 1] = [Some(command_list.into())];
        command_queue.ExecuteCommandLists(&cmd_lists);

        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
        let fence_value = 1u64;
        command_queue.Signal(&fence, fence_value)?;
        if fence.GetCompletedValue() < fence_value {
            let wait_result = UPLOAD_WAIT_EVENT.with(|&event| -> Result<u32> {
                fence.SetEventOnCompletion(fence_value, event)?;
                Ok(WaitForSingleObject(event, WAIT_TIMEOUT_MS).0)
            })?;
            if wait_result == WAIT_TIMEOUT.0 {
                eprintln!("[TEXTURE] ERROR: timeout ({} ms) waiting for mip-chain GPU copy to complete", WAIT_TIMEOUT_MS);
                if let Some(reason) = crate::device_removed_reason() {
                    eprintln!("[TEXTURE] Device removed, reason: {}", reason);
                }
                return Err(Error::from_hresult(HRESULT(1)));
            }
        }

        // upload_resources держались живыми до этой точки — GPU уже
        // подтвердил (fence) завершение всех копирований, дальше их можно
        // безопасно уронить.
        drop(upload_resources);

        Ok(())
    }

    /// ДОБАВЛЕНО (см. подробное объяснение бага у `create_texture2d`
    /// выше). Загружает `pixels` (row_pitch байт на строку, `height`
    /// строк) во временный UPLOAD-буфер, копирует его в текстуру
    /// `dst_resource` (уже созданную в DEFAULT heap, состояние
    /// COPY_DEST) командой `CopyTextureRegion`, переводит её в
    /// PIXEL_SHADER_RESOURCE и синхронно дожидается завершения на GPU
    /// перед возвратом (короткоживущие command allocator/list/fence —
    /// ИЗОЛИРОВАННЫЙ от общего `crate::get_fence()`/frame_fence_values
    /// движка, чтобы загрузка текстуры НЕ вмешивалась в подсчёт кадровых
    /// fence-значений основного render loop, см. `AlkashEngine::frame_fence_values`).
    unsafe fn upload_and_copy(
        device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
        dst_resource: &ID3D12Resource,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        pixels: &[u8],
        row_pitch: usize,
    ) -> Result<()> {
        // D3D12 требует, чтобы row pitch промежуточного upload-буфера был
        // выровнен на D3D12_TEXTURE_DATA_PITCH_ALIGNMENT (256 байт) — это
        // ОТДЕЛЬНОЕ требование от исходных данных (`pixels`, обычный
        // плотный RGBA8 без выравнивания), поэтому строки копируются по
        // отдельности в промежуточный буфер с этим выравниванием, а не
        // заливаются одним memcpy.
        const D3D12_TEXTURE_DATA_PITCH_ALIGNMENT: usize = 256;
        let aligned_row_pitch = (row_pitch + D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1)
            & !(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1);
        let upload_size = aligned_row_pitch * height as usize;

        let upload_heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
        let upload_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: upload_size as u64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT(0), // DXGI_FORMAT_UNKNOWN — буфер, не текстура
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let mut upload_resource: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(
            &upload_heap_properties,
            D3D12_HEAP_FLAG_NONE,
            &upload_desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut upload_resource,
        )?;
        let upload_resource = upload_resource.ok_or_else(|| {
            eprintln!("[TEXTURE] ERROR: upload staging resource is None!");
            Error::from_hresult(HRESULT(1))
        })?;

        let mut mapped = std::ptr::null_mut();
        upload_resource.Map(0, None, Some(&mut mapped))?;
        if mapped.is_null() {
            eprintln!("[TEXTURE] ERROR: staging buffer mapped pointer is null!");
            return Err(Error::from_hresult(HRESULT(1)));
        }
        for y in 0..height as usize {
            let src = &pixels[(y * row_pitch)..(y * row_pitch + row_pitch)];
            let dst = (mapped as *mut u8).add(y * aligned_row_pitch);
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst, row_pitch);
        }
        upload_resource.Unmap(0, None);

        // Короткоживущие command allocator/list — ИЗОЛИРОВАННЫЕ от
        // command allocator'ов основного render loop (те привязаны к
        // конкретному back buffer'у/кадру, см. `AlkashEngine::render_frame`),
        // т.к. загрузка текстуры может происходить в произвольный момент
        // (например посреди world streaming в `update()`, не обязательно
        // между BeginFrame/EndFrame основного прохода).
        let command_allocator: ID3D12CommandAllocator =
            device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
        let command_list: ID3D12GraphicsCommandList =
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &command_allocator, None)?;

        let footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
            Offset: 0,
            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                Format: format,
                Width: width,
                Height: height,
                Depth: 1,
                RowPitch: aligned_row_pitch as u32,
            },
        };

        let src_location = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(upload_resource.clone())),
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprint },
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        };
        let dst_location = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(dst_resource.clone())),
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        };

        command_list.CopyTextureRegion(&dst_location, 0, 0, 0, &src_location, None);

        // ManuallyDrop выше держал by-value клоны resource-ссылок только
        // на время жизни src_location/dst_location — освобождаем их сразу
        // после CopyTextureRegion, тем же паттерном, что уже используется
        // для transition_barrier/pRootSignature в engine/mod.rs (иначе
        // одна лишняя ссылка на COM-объект утекает на каждый вызов).
        {
            let mut src_location = src_location;
            let mut dst_location = dst_location;
            std::mem::ManuallyDrop::drop(&mut src_location.pResource);
            std::mem::ManuallyDrop::drop(&mut dst_location.pResource);
        }

        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: std::mem::ManuallyDrop::new(Some(dst_resource.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                    StateAfter: D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                }),
            },
        };
        let mut barrier = barrier;
        command_list.ResourceBarrier(&[barrier.clone()]);
        std::mem::ManuallyDrop::drop(&mut barrier.Anonymous.Transition);

        command_list.Close()?;

        let command_queue = crate::get_command_queue()?;
        let cmd_lists: [Option<ID3D12CommandList>; 1] = [Some(command_list.into())];
        command_queue.ExecuteCommandLists(&cmd_lists);

        // Собственный, изолированный fence — синхронно дожидаемся, чтобы
        // upload_resource/command_allocator/command_list можно было
        // безопасно уронить сразу после возврата из этой функции (их
        // время жизни не выходит за пределы этого вызова).
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
        let fence_value = 1u64;
        command_queue.Signal(&fence, fence_value)?;
        if fence.GetCompletedValue() < fence_value {
            // ИЗМЕНЕНО (оптимизация — см. комментарий у `UPLOAD_WAIT_EVENT`
            // в начале файла): переиспользуемый event вместо
            // create/close на каждый вызов.
            let wait_result = UPLOAD_WAIT_EVENT.with(|&event| -> Result<u32> {
                fence.SetEventOnCompletion(fence_value, event)?;
                // ИЗМЕНЕНО: было `WaitForSingleObject(event, INFINITE)` —
                // см. подробный комментарий у импорта `WaitForSingleObject`
                // в начале файла. Ограниченный таймаут вместо вечного
                // ожидания: при истечении логируем причину (если
                // устройство реально потеряно) и возвращаем явную ошибку
                // вместо того, чтобы молча повиснуть здесь навсегда.
                Ok(WaitForSingleObject(event, WAIT_TIMEOUT_MS).0)
            })?;
            if wait_result == WAIT_TIMEOUT.0 {
                eprintln!("[TEXTURE] ERROR: timeout ({} ms) waiting for texture upload GPU copy to complete", WAIT_TIMEOUT_MS);
                if let Some(reason) = crate::device_removed_reason() {
                    eprintln!("[TEXTURE] Device removed, reason: {}", reason);
                }
                return Err(Error::from_hresult(HRESULT(1)));
            }
        }

        Ok(())
    }

    pub fn create_render_target(width: u32, height: u32, format: DXGI_FORMAT) -> Result<Self> {
        println!("[TEXTURE] Creating render target: {}x{}, format={:?}", width, height, format);

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()`.
        let device = crate::get_device()?;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: format,
            Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.1, 0.1, 0.2, 1.0] },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            println!("[TEXTURE] Creating render target resource...");
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: Render target resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[TEXTURE] ✓ Render target created");

            Ok(Self {
                resource,
                width,
                height,
                format,
                mip_levels: 1,
            })
        }
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): текстура с
    /// UNORDERED_ACCESS-флагом — нужна промежуточным bloom-буферам, в
    /// которые compute-шейдер пишет напрямую (extract/downsample/blur), в
    /// отличие от `create_render_target`, куда пишет растровый пайплайн
    /// через RTV. НЕ добавляет ALLOW_RENDER_TARGET — эта текстура не
    /// участвует в растровом OMSetRenderTargets, только в compute-проходах
    /// через UAV (запись) и SRV (последующее чтение соседним/следующим
    /// проходом).
    pub fn create_uav_texture(width: u32, height: u32, format: DXGI_FORMAT) -> Result<Self> {
        println!("[TEXTURE] Creating UAV texture: {}x{}, format={:?}", width, height, format);

        let device = crate::get_device()?;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                None,
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: UAV texture resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[TEXTURE] ✓ UAV texture created");

            Ok(Self {
                resource,
                width,
                height,
                format,
                mip_levels: 1,
            })
        }
    }

    /// ДОБАВЛЕНО (Фаза 5): создаёт SRV для этой текстуры по указанному
    /// CPU-дескриптору (из CBV/SRV/UAV хипа, см. heap.rs
    /// `create_cbv_srv_uav_heap` — существовал, но не использовался нигде
    /// в движке до этой фазы).
    pub fn create_srv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: self.format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: self.mip_levels,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        };
        unsafe {
            device.CreateShaderResourceView(&self.resource, Some(&desc), handle);
        }
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5): создаёт UAV для этой текстуры — нужен
    /// compute-шейдерам bloom-прохода, которые пишут в текстуру напрямую
    /// (не через растровый OMSetRenderTargets).
    pub fn create_uav(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        let desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: self.format,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_UAV {
                    MipSlice: 0,
                    PlaneSlice: 0,
                },
            },
        };
        unsafe {
            device.CreateUnorderedAccessView(&self.resource, None, Some(&desc), handle);
        }
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5): создаёт RTV для этой текстуры — нужен HDR
    /// render target'у, в который основной draw pass рисует сцену вместо
    /// прямой записи в 8-битный back buffer.
    pub fn create_rtv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        unsafe {
            device.CreateRenderTargetView(&self.resource, None, handle);
        }
        Ok(())
    }

    pub fn create_depth_stencil(width: u32, height: u32) -> Result<Self> {
        println!("[TEXTURE] Creating depth stencil: {}x{}", width, height);

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()`.
        let device = crate::get_device()?;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            println!("[TEXTURE] Creating depth stencil resource...");
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[TEXTURE] ERROR: Depth stencil resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[TEXTURE] ✓ Depth stencil created");

            Ok(Self {
                resource,
                width,
                height,
                format: DXGI_FORMAT_D32_FLOAT,
                mip_levels: 1,
            })
        }
    }
}