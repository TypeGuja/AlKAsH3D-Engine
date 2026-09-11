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
use crate::STATE;
use crate::buffer::Buffer;
use crate::plugin::{GPULight, LightGridCell, LightGridEntry};
use crate::constant_buffer::TransformConstants;
use crate::command::CommandList;
use crate::math::{identity, Mat4, Vec3};
use super::{
    AlkashEngine, Vertex, NUM_CASCADES, CASCADE_SPLITS, SHADOW_MAP_RESOLUTION,
    OCCLUDER_MIN_WORLD_RADIUS, OCCLUDER_INSCRIBE_FACTOR,
    NEXT_FENCE_VALUE, wait_for_fence,
};

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
    fn wait_for_all_frames_idle_before_realloc(&self) {
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

        let new_capacity = needed_per_frame.max(64).next_power_of_two();
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

        let new_capacity = needed_per_frame.max(64).next_power_of_two();
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
    unsafe fn drop_transition_barrier(mut barrier: D3D12_RESOURCE_BARRIER) {
        unsafe {
            std::mem::ManuallyDrop::drop(&mut barrier.Anonymous.Transition);
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

    pub fn render_frame(&mut self) -> Result<bool> {
        let renderer = self.renderer.as_ref().ok_or_else(|| {
            eprintln!("[ENGINE] ERROR: render_frame() called but renderer is not initialized");
            Error::from_hresult(HRESULT(1))
        })?;

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

        let allocator = CommandList::get_allocator(frame_index)
            .ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

        unsafe {
            allocator.Reset()?;
        }

        let device = crate::get_device()?;

        let mut cmd_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?
        };

        let rtv_handle = renderer.hdr_rtv;
        let dsv_handle = renderer.depth_stencil_view;

        // ДОБАВЛЕНО (максимальная графика — LOD для мешей): считаем ОДИН
        // раз на кадр, используется и для shadow_jobs ниже, и для main
        // pass jobs дальше по функции — LOD выбирается по расстоянию до
        // КАМЕРЫ (не до источника света) в обоих проходах, чтобы тень
        // всегда соответствовала уровню детализации видимого объекта, а не
        // жила своей отдельной жизнью.
        let camera_pos_for_lod = self.camera.position;

        let shadow_jobs: Vec<(usize, Mat4)> = {
            let mut v: Vec<(usize, Mat4)> = Vec::new();
            if !self.mesh_instances.is_empty() {
                for instance in &self.mesh_instances {
                    if instance.mesh_index < self.meshes.len() {
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
                arr[i] = self.camera.far * CASCADE_SPLITS[i];
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

        if self.shadow_pipeline_state.is_some() && self.shadow_root_signature.is_some() {
            if let Err(e) = self.ensure_shadow_constant_buffer_capacity(shadow_jobs.len() * NUM_CASCADES) {
                eprintln!("[ENGINE] WARNING: не удалось выделить shadow_constant_buffer: {:?}", e);
            }

            unsafe {
                cmd_list.SetPipelineState(Some(self.shadow_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootSignature(Some(self.shadow_root_signature.as_ref().unwrap()));
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

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

                for cascade in 0..NUM_CASCADES {
                    let dsv = self.shadow_dsvs[cascade];
                    cmd_list.OMSetRenderTargets(0, None, false, Some(&dsv));
                    cmd_list.ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

                    let light_view_proj = cascade_view_projs[cascade];

                    if let Some(shadow_cb) = &self.shadow_constant_buffer {
                        for (i, (mesh_index, model)) in shadow_jobs.iter().enumerate() {
                            let mesh = &self.meshes[*mesh_index];
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

        cmd_list = self.maybe_flush_for_warm_up(cmd_list, &allocator, "shadow")?;

        unsafe {
            cmd_list.OMSetRenderTargets(1, Some(&rtv_handle), false, Some(&dsv_handle));
            cmd_list.ClearRenderTargetView(rtv_handle, &self.clear_color, None);
            cmd_list.ClearDepthStencilView(dsv_handle, D3D12_CLEAR_FLAG_DEPTH, 1.0, 0, None);

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
                    let gpu_lights = self.lights.as_ref().map(|l| l.get_gpu_lights()).unwrap_or(&[]);
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
                            });
                        }
                    }
                }
            } else if self.scene.is_empty() {
                for i in 0..self.meshes.len() {
                    jobs.push(DrawJob { mesh_index: i, transform: DrawTransform::RawIdentity });
                }
            }

            for (mesh_index, world) in self.scene.collect_renderables() {
                if mesh_index < self.meshes.len() {
                    if let Some(idx) = self.resolve_lod_mesh_index(mesh_index, world, camera_pos_for_lod) {
                        jobs.push(DrawJob { mesh_index: idx, transform: DrawTransform::Camera(world) });
                    }
                }
            }

            let view = self.camera.view_matrix();
            let proj = self.camera.projection_matrix();
            let id_matrix = identity();

            let frustum = crate::math::Frustum::from_view_proj(&(proj * view));
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
                }
                DrawTransform::RawIdentity => true,
            });

            self.poll_occluder_readback();
            let mut occluder_instance_data: Vec<f32> = Vec::new();
            for job in &jobs {
                let DrawTransform::Camera(model) = &job.transform else { continue };
                if job.mesh_index >= self.meshes.len() { continue; }
                let mesh = &self.meshes[job.mesh_index];
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

            for (i, job) in jobs.iter().enumerate() {
                let mesh = &self.meshes[job.mesh_index];

                if let Some(shadow_srv_heap) = self.shadow_srv_heap.as_ref() {
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
                }

                let has_mr_map = if mesh.mr_srv_index.is_some() { 1.0f32 } else { 0.0f32 };
                let mr_constants: [f32; 4] = [mesh.material_metallic, mesh.material_roughness, has_mr_map, 0.0];
                cmd_list.SetGraphicsRoot32BitConstants(8, 4, mr_constants.as_ptr() as *const _, 0);

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
                let to_resolve = [
                    Self::transition_barrier(
                        &renderer.hdr_target.resource,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        D3D12_RESOURCE_STATE_RESOLVE_SOURCE,
                    ),
                    Self::transition_barrier(
                        &renderer.hdr_resolved.resource,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_RESOLVE_DEST,
                    ),
                ];
                cmd_list.ResourceBarrier(&to_resolve);
                for b in to_resolve {
                    Self::drop_transition_barrier(b);
                }

                cmd_list.ResolveSubresource(
                    &renderer.hdr_resolved.resource,
                    0,
                    &renderer.hdr_target.resource,
                    0,
                    windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
                );

                let after_resolve = [
                    Self::transition_barrier(
                        &renderer.hdr_target.resource,
                        D3D12_RESOURCE_STATE_RESOLVE_SOURCE,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ),
                    Self::transition_barrier(
                        &renderer.hdr_resolved.resource,
                        D3D12_RESOURCE_STATE_RESOLVE_DEST,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ),
                ];
                cmd_list.ResourceBarrier(&after_resolve);
                for b in after_resolve {
                    Self::drop_transition_barrier(b);
                }
            }

            if let (Some(volumetric_texture), Some(volumetric_srv_heap), Some(volumetric_cb)) = (
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
            if let (Some(ssao_texture), Some(ssao_depth_srv_heap), Some(ssao_cb)) = (
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
                    radius: 0.5,
                    bias: 0.02,
                    strength: 1.2,
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
            }

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

            if let (Some(bloom_a), Some(bloom_b), Some(bloom_srv_heap)) =
                (&self.bloom_texture_a, &self.bloom_texture_b, &self.bloom_srv_heap)
            {
                let bloom_a_resource = &bloom_a.resource;
                let bloom_b_resource = &bloom_b.resource;
                let bloom_width = bloom_a.width;
                let bloom_height = bloom_a.height;

                let bloom_viewport = D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: bloom_width as f32,
                    Height: bloom_height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                };
                let bloom_scissor = RECT {
                    left: 0,
                    top: 0,
                    right: bloom_width as i32,
                    bottom: bloom_height as i32,
                };

                cmd_list.RSSetViewports(&[bloom_viewport]);
                cmd_list.RSSetScissorRects(&[bloom_scissor]);
                cmd_list.SetGraphicsRootSignature(Some(self.bloom_root_signature.as_ref().unwrap()));
                let heaps = [Some(bloom_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&heaps);
                cmd_list.IASetPrimitiveTopology(D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

                // ИЗМЕНЕНО (MSAA — см. resolve-шаг сразу после main pass
                // выше): раньше здесь был переход `hdr_target` RENDER_TARGET
                // -> PIXEL_SHADER_RESOURCE — теперь не нужен, `hdr_target`
                // уже вернулась в RENDER_TARGET сразу после resolve, а то,
                // что реально читает bloom-extract (`renderer.hdr_srv_gpu`),
                // указывает на `hdr_resolved`, УЖЕ находящуюся в
                // PIXEL_SHADER_RESOURCE к этому моменту.
                let a_before = if self.bloom_a_is_srv {
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                } else {
                    D3D12_RESOURCE_STATE_RENDER_TARGET
                };
                if a_before != D3D12_RESOURCE_STATE_RENDER_TARGET {
                    let barriers = [Self::transition_barrier(
                        bloom_a_resource,
                        a_before,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    )];
                    cmd_list.ResourceBarrier(&barriers);
                    for b in barriers {
                        Self::drop_transition_barrier(b);
                    }
                }
                self.bloom_a_is_srv = false;

                cmd_list.OMSetRenderTargets(1, Some(&self.bloom_rtv_a), false, None);
                cmd_list.SetPipelineState(Some(self.bloom_extract_pipeline_state.as_ref().unwrap()));
                let hdr_heap = [Some(renderer.srv_uav_heap.clone())];
                cmd_list.SetDescriptorHeaps(&hdr_heap);
                cmd_list.SetGraphicsRootDescriptorTable(0, renderer.hdr_srv_gpu);
                if let Some(params_cb) = &self.bloom_params_buffer {
                    let params: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
                    let bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 16);
                    let _ = params_cb.update_constant_buffer(bytes);
                    cmd_list.SetGraphicsRootConstantBufferView(1, params_cb.resource.GetGPUVirtualAddress());
                }
                cmd_list.DrawInstanced(3, 1, 0, 0);

                let bloom_heap_rebind = [Some(bloom_srv_heap.clone())];
                cmd_list.SetDescriptorHeaps(&bloom_heap_rebind);

                let a_to_srv = Self::transition_barrier(
                    bloom_a_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                let b_before = if self.bloom_b_is_srv {
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                } else {
                    D3D12_RESOURCE_STATE_RENDER_TARGET
                };
                let mut barriers = Vec::with_capacity(2);
                barriers.push(a_to_srv);
                if b_before != D3D12_RESOURCE_STATE_RENDER_TARGET {
                    barriers.push(Self::transition_barrier(
                        bloom_b_resource,
                        b_before,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ));
                }
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_a_is_srv = true;
                self.bloom_b_is_srv = false;

                cmd_list.OMSetRenderTargets(1, Some(&self.bloom_rtv_b), false, None);
                cmd_list.SetPipelineState(Some(self.bloom_blur_pipeline_state.as_ref().unwrap()));
                cmd_list.SetGraphicsRootDescriptorTable(0, self.bloom_srv_a_gpu);
                if let Some(params_cb) = &self.bloom_params_buffer {
                    let texel_x = 1.0 / bloom_width as f32;
                    let params: [f32; 4] = [1.0, texel_x, 0.0, 0.0];
                    let bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 16);
                    let _ = params_cb.update_constant_buffer(bytes);
                    cmd_list.SetGraphicsRootConstantBufferView(1, params_cb.resource.GetGPUVirtualAddress());
                }
                cmd_list.DrawInstanced(3, 1, 0, 0);

                let b_to_srv = Self::transition_barrier(
                    bloom_b_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                let a_back_to_rt = Self::transition_barrier(
                    bloom_a_resource,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                );
                let barriers = [b_to_srv, a_back_to_rt];
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
                    Self::drop_transition_barrier(b);
                }
                self.bloom_b_is_srv = true;
                self.bloom_a_is_srv = false;

                cmd_list.OMSetRenderTargets(1, Some(&self.bloom_rtv_a), false, None);
                cmd_list.SetGraphicsRootDescriptorTable(0, self.bloom_srv_b_gpu);
                if let Some(params_cb) = &self.bloom_params_buffer {
                    let texel_y = 1.0 / bloom_height as f32;
                    let params: [f32; 4] = [1.0, 0.0, texel_y, 0.0];
                    let bytes = std::slice::from_raw_parts(params.as_ptr() as *const u8, 16);
                    let _ = params_cb.update_constant_buffer(bytes);
                    cmd_list.SetGraphicsRootConstantBufferView(1, params_cb.resource.GetGPUVirtualAddress());
                }
                cmd_list.DrawInstanced(3, 1, 0, 0);

                let a_final_to_srv = Self::transition_barrier(
                    bloom_a_resource,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                );
                let barriers = [a_final_to_srv];
                cmd_list.ResourceBarrier(&barriers);
                for b in barriers {
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
                    let tonemap_data: [f32; 4] = [settings.exposure, settings.bloom_intensity, 0.0, 0.0];
                    let bytes = std::slice::from_raw_parts(tonemap_data.as_ptr() as *const u8, 16);
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
            cmd_list.ResourceBarrier(&barriers_after);
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

        let swap_chain = crate::get_swap_chain()?;

        unsafe {
            let hr = swap_chain.Present(1, DXGI_PRESENT(0));
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

        {
            let mut state = STATE.lock().unwrap();
            if let Some(swap_chain) = &state.swap_chain {
                state.frame_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
            }
        }

        Ok(true)
    }
}
