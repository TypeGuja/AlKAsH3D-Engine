//! Основной движок Alkash3D

mod scripting_python;
pub use scripting_python::PythonScriptRuntime;

mod scripting;
pub use scripting::{ScriptHandle, pack_entity_id};

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга — разбивка монолита
// `engine/mod.rs` на подсистемы): геометрические примитивы (Vertex/Mesh/
// MeshInstance), не зависящие от `AlkashEngine` — см. engine/mesh.rs.
mod mesh;
pub use mesh::{Vertex, Mesh, MeshInstance};

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): цикл дня/ночи — см.
// engine/day_night.rs. `ManagedLight` нужен здесь как тип поля
// `AlkashEngine::managed_lights` ниже, поэтому он `pub(super)` в
// day_night.rs, а не полностью приватный.
mod day_night;
use day_night::ManagedLight;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): API добавления мешей и
// спавна ECS-сущностей — см. engine/mesh_api.rs.
mod mesh_api;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): мост к плагинам физики
// (Inertial)/света (FirstFires) и встроенному звуку — см.
// engine/physics_bridge.rs.
mod physics_bridge;

// ДОБАВЛЕНО (разборка машины/двигателя/коробки на детали): спавн
// `.alasm`-сборок (граф деталей + joints поверх `physics_bridge`) в
// реальные физические тела/joints/ECS-сущности — см. engine/assembly.rs.
mod assembly;
pub use assembly::{AssemblyHandle, AssemblyPart};

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): загрузка .altex-геометрии/
// текстур в GPU-ресурсы, материальный SRV-хип — см. engine/asset_loading.rs.
mod asset_loading;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): стриминг открытого мира
// (рантайм-состояние чанков, параллельная фоновая загрузка через
// EngineScheduler, загрузка/выгрузка мира) — см. engine/world_streaming.rs.
// `WorldStreamingState` нужен здесь как тип поля `AlkashEngine::world`,
// поэтому реэкспортирован.
mod world_streaming;
pub use world_streaming::WorldStreamingState;
use world_streaming::{AltexParseCache, ChunkLoadResult};

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): окно Win32/WNDPROC/resize/
// fullscreen — см. engine/window.rs.
mod window;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): основной 3D draw pass —
// шейдеры/root signature/PSO — см. engine/pipeline_main.rs.
mod pipeline_main;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): пост-обработка HDR-кадра
// (tonemap + bloom) — см. engine/pipeline_post.rs.
mod pipeline_post;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): cascaded shadow maps —
// см. engine/pipeline_shadow.rs.
mod pipeline_shadow;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): occlusion culling на
// второй видеокарте — см. engine/pipeline_occluder.rs.
mod pipeline_occluder;
use pipeline_occluder::{SecondaryDepthTarget, SecondaryBuffer};

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): volumetric god-rays —
// см. engine/pipeline_volumetric.rs.
mod pipeline_volumetric;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга): главный проход рендера
// кадра (`render_frame`) + рост GPU-буферов по требованию — см.
// engine/render_frame.rs.
mod render_frame;

// ВЫНЕСЕНО (Фаза 1 архитектурного рефакторинга, финальный шаг): жизненный
// цикл движка (new/init/update/shutdown/Drop) — см. engine/lifecycle.rs.
mod lifecycle;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::{DXGI_PRESENT, DXGI_SWAP_CHAIN_FLAG};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_UINT, DXGI_FORMAT_UNKNOWN};
use windows::Win32::Graphics::Gdi::{
    UpdateWindow, COLOR_WINDOW, HBRUSH,
    MonitorFromWindow, GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::*;
use crate::plugin::{PhysicsPlugin, LightPlugin, PhysicsConfig, LightConfig, GPULight, PhysicsBody, PhysicsContact, PhysicsStats, LightGridCell, LightGridEntry, LightGridParams};
use crate::math::{Mat4, Vec3, identity, translation, rotation_x, rotation_y, rotation_z, scaling};
use crate::camera::Camera;
use crate::constant_buffer::TransformConstants;
use crate::shader::ShaderBlob;
use crate::pso::PipelineState;
use crate::input::InputState;

static NEXT_FENCE_VALUE: AtomicU64 = AtomicU64::new(1);

fn wait_for_fence(fence: &ID3D12Fence, target: u64, timeout: std::time::Duration) -> std::result::Result<(), String> {
    if target == 0 {
        return Ok(());
    }
    let start = std::time::Instant::now();
    loop {
        let completed = unsafe { fence.GetCompletedValue() };
        if completed >= target {
            return Ok(());
        }
        if let Some(reason) = crate::device_removed_reason() {
            return Err(format!("device removed while waiting for fence (target={}, completed={}): {}", target, completed, reason));
        }
        if start.elapsed() > timeout {
            return Err(format!("timeout waiting for fence (target={}, completed={}, waited={:?})", target, completed, start.elapsed()));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит на
/// месте" ПОСЛЕ фиксов стриминга/hot-reload/culling/физики): разбивка
/// времени `AlkashEngine::update()` по под-фазам — каждое поле хранит
/// ХУДШИЙ случай этой конкретной под-фазы за текущее 1-секундное окно
/// измерения (тот же принцип, что уже `max_update_ms`/`max_render_ms` в
/// bin/main.rs, но детальнее). `[PHYS-STATS]` уже показал, что сам
/// физический солвер спокоен во время спайков — то есть общий
/// `physics_ms` здесь тоже должен остаться низким, а виновная под-фаза
/// (скрипты/день-ночь/стриминг/каллинг света/аудио) станет видна прямым
/// сравнением чисел в логе, без дальнейших догадок по коду.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateBreakdownMs {
    pub physics_ms: f32,
    pub sync_physics_ms: f32,
    pub native_scripts_ms: f32,
    pub python_scripts_ms: f32,
    pub day_night_ms: f32,
    pub world_streaming_ms: f32,
    pub chunk_io_ms: f32,
    pub light_cull_ms: f32,
    pub audio_ms: f32,
}

/// ДОБАВЛЕНО (задача #39 плана — модель машины): хендл на все сущности,
/// созданные `AlkashEngine::spawn_physics_car` — физическое тело кузова
/// (`body_id`, для будущих задач #41/#42: приложение сил подвески/
/// управления через `PhysicsAPI`) и ECS-сущности кузова/4 колёс (для
/// прямого чтения/правки `Transform`, если понадобится, например,
/// визуально прокручивать колёса отдельно от кузова — сама физика колёс
/// пока не отдельные rigid body, см. подробный комментарий у
/// `spawn_physics_car`).
#[derive(Debug, Clone, Copy)]
pub struct CarHandle {
    pub body_id: i32,
    pub chassis_entity: crate::scene::EntityId,
    /// Порядок: [перед-лево, перед-право, зад-лево, зад-право] — тот же,
    /// что и в `wheel_local_positions` внутри `spawn_physics_car`.
    pub wheel_entities: [crate::scene::EntityId; 4],
    /// ДОБАВЛЕНО (реальная физика машины — подвеска): позиции точек
    /// крепления колёс в ЛОКАЛЬНЫХ осях кузова (те же значения, что уже
    /// вычислялись внутри `spawn_physics_car` для расстановки визуальных
    /// колёс-детей) — вызывающий игровой код (подвеска, см. main_car.rs)
    /// поворачивает их текущей ориентацией тела `get_body(body_id)` для
    /// raycast'а вниз в мировых координатах на каждом кадре, вместо того
    /// чтобы дублировать формулу расчёта позиций колёс во второй раз.
    pub wheel_local_positions: [[f32; 3]; 4],
    pub wheel_radius: f32,
}

pub struct AlkashEngine {
    pub renderer: Option<Renderer>,
    pub meshes: Vec<Mesh>,
    pub mesh_instances: Vec<MeshInstance>,
    pub root_signature: Option<ID3D12RootSignature>,
    pub pipeline_state: Option<ID3D12PipelineState>,
    pub vs: Option<ShaderBlob>,
    pub ps: Option<ShaderBlob>,

    pub camera: Camera,
    pub constant_buffer: Option<Buffer>,
    pub transform_constants: TransformConstants,
    /// ДОБАВЛЕНО: сколько СЛОТОВ трансформаций (на один back buffer)
    /// вмещает текущий `constant_buffer`. Буфер реально в 2 раза больше
    /// (по одному набору слотов на каждый из двух back buffer'ов) — см.
    /// `ensure_constant_buffer_capacity`.
    constant_buffer_capacity: usize,

    pub scheduler: Arc<EngineScheduler>,

    /// ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит на
    /// месте"): худшее время каждой под-фазы `update()` за текущее
    /// 1-секундное окно измерения — см. макрос `timed!` внутри `update()`
    /// и `AlkashEngine::take_update_breakdown` (публичный геттер+сброс
    /// для bin/main.rs).
    update_breakdown_ms: UpdateBreakdownMs,

    pub physics: Option<PhysicsPlugin>,
    pub lights: Option<LightPlugin>,

    /// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): в отличие от
    /// `physics`/`lights`, звук — НЕ внешний plugin DLL (на диске
    /// пользователя нет отдельного готового аудио-плагина, см. подробный
    /// комментарий в начале audio.rs) — `AudioEngine` создаётся напрямую
    /// через системный XAudio2. `None` до вызова `init_audio()`, тот же
    /// принцип "подсистема опциональна, инициализация явная", что и у
    /// physics/lights — приложение (main.rs) решает, нужен ли звук в
    /// конкретном запуске, движок не навязывает его.
    pub audio: Option<AudioEngine>,

    /// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины): в
    /// отличие от `physics`/`lights` (singleton — один плагин своего типа
    /// на весь движок), скриптовых DLL может быть загружено НЕСКОЛЬКО
    /// одновременно, и одна и та же DLL может обслуживать несколько
    /// сущностей (см. подробный комментарий в
    /// `plugin/scripting_api.rs`/`plugin/mod.rs::ScriptingPlugin`). Ключ —
    /// путь к DLL, тот же, что использовался при `load_native_script`
    /// (позволяет узнать, уже ли загружена конкретная DLL, не грузя её
    /// повторно).
    pub native_scripts: std::collections::HashMap<String, crate::plugin::ScriptingPlugin>,

    /// Все текущие живые прикрепления скриптов к сущностям — обновляются
    /// КАЖДЫЙ кадр в `update()` (см. `update_native_scripts`). Хранится
    /// отдельно от `native_scripts` (которое хранит САМИ DLL, а не их
    /// прикрепления к конкретным entity) по той же причине, по которой
    /// `physics_links` хранится отдельно от `physics`.
    pub active_scripts: Vec<ScriptHandle>,

    /// Монотонный счётчик кадров для `ScriptContext::frame_number` —
    /// нужен скриптам, которым важно различать "первый кадр после
    /// create_script" от последующих, или делать что-то раз в N кадров
    /// без своего собственного таймера на стороне DLL.
    ///
    /// ИЗМЕНЕНО (рефакторинг — вынос скриптинга в engine/scripting.rs):
    /// `pub(super)` вместо приватного — `update_native_scripts` (теперь в
    /// scripting.rs, отдельном подмодуле) читает и увеличивает это поле;
    /// см. подробное объяснение `pub(super)` у `deps_fallback_path` выше.
    pub(super) script_frame_counter: u64,

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload): в
    /// отличие от `native_scripts`/`active_scripts` (Native/Lua — общий
    /// DLL-плагинный путь через `ScriptingPlugin`/`ScriptHandle`), Python
    /// работает БЕЗ DLL — каждое прикрепление это просто
    /// `PythonScriptRuntime` (собственный Python-scope + путь к .py-файлу
    /// + mtime для hot-reload), живущий прямо здесь. Ключ — `EntityId`
    /// владельца: в отличие от Native/Lua, где несколько разных сущностей
    /// МОГУТ делить одну DLL, каждой Python-сущности всегда соответствует
    /// РОВНО одно прикрепление (упрощение первой версии — если понадобится
    /// несколько .py-скриптов на одной сущности одновременно, ключ надо
    /// будет расширить до (EntityId, script_slot)).
    pub python_scripts: std::collections::HashMap<crate::scene::EntityId, PythonScriptRuntime>,

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): связь физического
    /// тела (id, присвоенный плагином Inertial через `add_body`/
    /// `add_sphere_body`) с визуальной ECS-сущностью, чью `Transform`
    /// нужно КАЖДЫЙ кадр обновлять её текущей позицией — см.
    /// `sync_physics_transforms()`, вызывается из `update()` сразу после
    /// `physics.update(dt, gravity)`. Без этой связи физика считалась бы
    /// "в вакууме" (тела падают/сталкиваются внутри плагина), но экран
    /// показывал бы неподвижную геометрию — ровно тот же класс разрыва
    /// между "модель" и "представление", которого не было бы, храни
    /// движок позицию ТОЛЬКО в Transform или ТОЛЬКО в PhysicsBody, но не в
    /// обоих местах сразу. `Vec`, а не `HashMap<i32, EntityId>` — типичное
    /// число физических тел в сцене (десятки-сотни, не миллионы), линейный
    /// проход по нему каждый кадр дешевле, чем поддержка хэш-карты, и
    /// проще для отладки (порядок вставки сохраняется).
    pub physics_links: Vec<(i32, crate::scene::EntityId)>,

    hwnd: Option<HWND>,
    running: bool,

    /// ДОБАВЛЕНО (F11 — переключение полноэкранного режима):
    /// `is_fullscreen` — текущее состояние; `saved_window_rect`/
    /// `saved_window_style` — позиция/размер и стиль окна ДО входа в
    /// полноэкранный режим, чтобы корректно восстановить их при выходе
    /// (см. `toggle_fullscreen`).
    is_fullscreen: bool,
    saved_window_rect: Option<RECT>,
    saved_window_style: Option<WINDOW_STYLE>,

    width: u32,
    height: u32,
    clear_color: [f32; 4],

    shutdown_in_progress: bool,

    /// ДОБАВЛЕНО (фикс краша видеодрайвера при смене разрешения окна):
    /// `true` между WM_ENTERSIZEMOVE и WM_EXITSIZEMOVE — то есть пока
    /// пользователь реально держит зажатой мышь на рамке/заголовке окна
    /// (перетаскивание для ресайза ИЛИ перемещения). Windows шлёт WM_SIZE
    /// на КАЖДЫЙ промежуточный кадр перетаскивания рамки (не один раз в
    /// конце) — а `handle_resize()` это тяжёлая операция: ждёт GPU idle,
    /// дропает весь Renderer (RTV/DSV/HDR/SRV-хипы), вызывает
    /// `ResizeBuffers`, пересоздаёт Renderer заново. Без throttling'а это
    /// повторялось бы ДЕСЯТКИ раз в секунду, пока пользователь тянет
    /// границу окна — на слабом железе (10-летний минимум, под который
    /// целится движок) это регулярно превышало таймаут Timeout Detection
    /// & Recovery видеодрайвера (обычно ~2 секунды непрерывной занятости
    /// GPU без Present) и вызывало сброс драйвера/перезагрузку. Пока
    /// `resizing_live == true`, WM_SIZE только запоминает целевой размер в
    /// `pending_resize`, реальный `handle_resize()` откладывается до
    /// WM_EXITSIZEMOVE (окно отпущено) — ОДИН тяжёлый ресайз на весь жест
    /// перетаскивания вместо одного на каждый промежуточный пиксель.
    resizing_live: bool,
    /// Последний размер, полученный через WM_SIZE во время live-resize
    /// (`resizing_live == true`), которому ещё не соответствовал реальный
    /// `handle_resize()` — применяется одним вызовом в WM_EXITSIZEMOVE.
    /// `None`, если во время текущего жеста перетаскивания размер ни разу
    /// не менялся (например, пользователь просто перемещал окно, а не
    /// тянул за рамку) — тогда WM_EXITSIZEMOVE ничего не делает.
    pending_resize: Option<(u32, u32)>,

    frame_fence_values: Vec<u64>,

    /// ДОБАВЛЕНО (фикс DXGI_ERROR_DEVICE_HUNG на первом кадре — см.
    /// `warm_up_pipelines`/`maybe_flush_for_warm_up` в render_frame.rs):
    /// пока `true`, `render_frame()` досрочно закрывает и отправляет на
    /// GPU командный список между "тяжёлыми" проходами (shadow/main),
    /// вместо того чтобы копить их все в одном submission — иначе
    /// суммарная JIT-компиляция ШЕСТИ PSO драйвером при первом
    /// использовании каждого (измерено: ~2.5с на первый кадр) упирается в
    /// таймаут TDR (~2с). Выставляется в `true` только внутри
    /// `warm_up_pipelines()` на время ОДНОГО вызова `render_frame()`,
    /// вызываемого один раз перед входом в игровой цикл — обычные кадры
    /// это поле не трогают, `false` по умолчанию не меняет их поведение.
    warm_up_mode: bool,

    /// ДОБАВЛЕНО: см. input.rs. Заполняется движком из оконных сообщений
    /// (wndproc), читается игровым кодом. Движок сам решений о том, что
    /// делать с вводом (двигать камеру, закрывать окно и т.п.), НЕ
    /// принимает — это отдано на откуп приложению (main.rs).
    pub input: InputState,

    /// ДОБАВЛЕНО: ECS-ядро сцены (см. scene.rs). Работает ПАРАЛЛЕЛЬНО со
    /// старым `mesh_instances` — ничего не меняет в поведении существующих
    /// main.rs/main1.rs/main2.rs, которые продолжают использовать
    /// `mesh_instances` напрямую. `render_frame()` рендерит содержимое
    /// `scene` ДОПОЛНИТЕЛЬНО к `mesh_instances`, если в сцене есть живые
    /// сущности.
    pub scene: crate::scene::Scene,

    /// ДОБАВЛЕНО (Фаза 1 плана по реализму/фонарям): глобальные настройки
    /// освещения из последнего загруженного .alfar (ambient-цвет/яркость,
    /// shadow_quality, bloom_intensity, exposure, gamma и т.п. — см.
    /// alfar_format::GlobalLightSettings/AmbientLight). Пока ничего в
    /// рендере эти поля ещё не читает (это будущие Фазы 5/6/8 плана —
    /// HDR/bloom, тени, volumetrics) — сохраняются здесь уже сейчас, чтобы
    /// `load_lights_from_alfar` не приходилось переписывать при подключении
    /// каждой следующей фазы.
    pub light_ambient: Option<crate::alfar_format::AmbientLight>,
    pub light_global_settings: Option<crate::alfar_format::GlobalLightSettings>,

    /// ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): GPU-буфер
    /// (StructuredBuffer<GPULight>, register t0), в который каждый кадр
    /// копируется список УЖЕ ОТКУЛЛЕННЫХ (видимых после LOD/дистанции/
    /// фрустума) фонарей от FirstFires (`LightPlugin::get_gpu_lights()`).
    /// До этой фазы список фонарей существовал только на CPU-стороне —
    /// пиксельный шейдер вообще не имел к нему доступа.
    light_buffer: Option<Buffer>,
    /// Сколько GPULight-слотов сейчас реально вмещает `light_buffer` (в
    /// элементах, не в байтах) — растёт по требованию, аналогично
    /// `constant_buffer_capacity`.
    light_buffer_capacity: usize,

    /// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): GPU-буферы под
    /// пространственную сетку FirstFires (StructuredBuffer<LightGridCell>
    /// t1 и StructuredBuffer<LightGridEntry> t2) — позволяют пиксельному
    /// шейдеру находить фонари СВОЕЙ ячейки вместо перебора всего видимого
    /// списка на каждый пиксель (см. render_frame). Число ячеек в сетке
    /// FirstFires (`grid_width*height*depth`) фиксировано на весь срок
    /// жизни LightPlugin (задаётся один раз в LightConfig при
    /// `init_lights`), поэтому размер этого буфера НЕ растёт по кадрам, в
    /// отличие от `light_buffer`/`grid_entries_buffer` — выделяется один
    /// раз при первом кадре после инициализации света.
    grid_cells_buffer: Option<Buffer>,
    grid_cells_buffer_capacity: usize,
    /// Число entries растёт по кадрам (зависит от того, сколько фонарей
    /// реально видимо и в каких ячейках) — тот же паттерн роста, что и у
    /// light_buffer.
    grid_entries_buffer: Option<Buffer>,
    grid_entries_buffer_capacity: usize,

    tonemap_vs: Option<ShaderBlob>,
    tonemap_ps: Option<ShaderBlob>,
    tonemap_root_signature: Option<ID3D12RootSignature>,
    tonemap_pipeline_state: Option<ID3D12PipelineState>,
    /// Константы экспозиции для tonemap-прохода — по умолчанию exposure=1.0,
    /// обновляется из `.alfar` GlobalLightSettings.exposure при загрузке
    /// сцены (см. `load_lights_from_alfar` — light_global_settings).
    tonemap_constant_buffer: Option<Buffer>,

    bloom_extract_ps: Option<ShaderBlob>,
    bloom_blur_ps: Option<ShaderBlob>,
    /// Общая root signature для extract/blur — один SRV (t0) + сэмплер +
    /// один CBV (b0) с параметрами (порог для extract, направление блюра
    /// для blur — оба варианта используют одну и ту же по форме структуру
    /// параметров, чтобы не плодить лишние root signatures).
    bloom_root_signature: Option<ID3D12RootSignature>,
    bloom_extract_pipeline_state: Option<ID3D12PipelineState>,
    bloom_blur_pipeline_state: Option<ID3D12PipelineState>,
    bloom_params_buffer: Option<Buffer>,
    /// Half-res ping-pong render target A — хранится вне `Renderer`
    /// (в отличие от `hdr_target`), т.к. это внутренняя деталь именно
    /// bloom-прохода, а не что-то, что нужно основному 3D draw pass'у.
    bloom_texture_a: Option<crate::render::RenderTexture>,
    bloom_rtv_a: D3D12_CPU_DESCRIPTOR_HANDLE,
    bloom_srv_a_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    bloom_texture_b: Option<crate::render::RenderTexture>,
    bloom_rtv_b: D3D12_CPU_DESCRIPTOR_HANDLE,
    bloom_srv_b_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// Отдельные RTV heap (2 дескриптора: A, B) и SRV heap (2 дескриптора:
    /// A, B) под bloom-таргеты — тот же паттерн, что и `hdr_rtv_heap`/
    /// `srv_uav_heap` в Renderer, но здесь, а не там, т.к. размер
    /// bloom-таргетов (half-res) отличается от размера HDR-таргета и
    /// пересоздаётся вместе с ним при ресайзе окна в будущем.
    bloom_rtv_heap: Option<ID3D12DescriptorHeap>,
    bloom_srv_heap: Option<ID3D12DescriptorHeap>,
    /// ВАЖНО: реальное текущее состояние bloom_texture_a/b НАЧИНАЕТСЯ как
    /// RENDER_TARGET (см. `create_hdr_target` — все render targets в этом
    /// движке создаются именно в этом состоянии) и МЕНЯЕТСЯ каждый кадр
    /// bloom-проходом в `render_frame`. Без явного трекера пришлось бы
    /// либо угадывать состояние по номеру кадра (хрупко), либо каждый раз
    /// вставлять "safety"-барьеры с неверным StateBefore на первом кадре
    /// (что валидатор D3D12 debug layer справедливо считает ошибкой).
    /// `true` = сейчас PIXEL_SHADER_RESOURCE, `false` = сейчас RENDER_TARGET.
    bloom_a_is_srv: bool,
    bloom_b_is_srv: bool,

    /// Массив из `NUM_CASCADES` depth-таргетов (по одному на каскад) —
    /// см. подробное объяснение у `RenderTexture::create_shadow_map` в
    /// render.rs про TYPELESS-паттерн (одновременно DSV и SRV поверх
    /// одной и той же памяти). Хранятся здесь, а не в `Renderer`, по той
    /// же причине, что и bloom-текстуры выше: разрешение shadow map
    /// (фиксированное — SHADOW_MAP_RESOLUTION) НЕ зависит от размера
    /// окна, поэтому НЕ должны пересоздаваться при каждом resize (в
    /// отличие от hdr_target/depth_stencil внутри Renderer).
    ///
    /// Массив фиксированного размера (`[Option<T>; NUM_CASCADES]`), а не
    /// `Vec` — число каскадов известно на этапе компиляции (константа
    /// `NUM_CASCADES`) и не меняется в рантайме, `Vec` добавил бы только
    /// лишнее косвенное обращение к куче без реальной гибкости.
    shadow_maps: [Option<crate::render::RenderTexture>; NUM_CASCADES],
    shadow_dsv_heap: Option<ID3D12DescriptorHeap>,
    shadow_dsvs: [D3D12_CPU_DESCRIPTOR_HANDLE; NUM_CASCADES],
    /// Один SHADER_VISIBLE-хип на `NUM_CASCADES` СМЕЖНЫХ дескрипторов — SRV
    /// каждого каскада для сэмплирования в основном пиксельном шейдере (см.
    /// корневой параметр 4 = descriptor table SRV t3..t3+NUM_CASCADES-1 в
    /// `create_root_signature`). Отдельный от `renderer.srv_uav_heap` (тот
    /// используется tonemap-проходом, а shadow map читается ОСНОВНЫМ 3D
    /// draw pass'ом — разные корневые сигнатуры, разные точки бинда в
    /// кадре). Смежность ОБЯЗАТЕЛЬНА — тот же принцип, что и у HDR+bloom
    /// SRV в `renderer.srv_uav_heap` (см. `create_tonemap_root_signature`):
    /// одна descriptor table с одним диапазоном на NUM_CASCADES дескрипторов
    /// требует, чтобы все они лежали подряд в одном heap.
    shadow_srv_heap: Option<ID3D12DescriptorHeap>,
    /// GPU-адрес НАЧАЛА (индекс 0) descriptor table в `shadow_srv_heap` —
    /// шейдер видит t3=каскад0, t4=каскад1, t5=каскад2 (смежные
    /// дескрипторы, начиная с этого адреса).
    shadow_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// Отдельные root signature/PSO/шейдеры под shadow pass — ОБЩИЕ для
    /// ВСЕХ каскадов (проход рисует ТОЛЬКО глубину — нет PS вообще, нет
    /// UV/normal/color на выходе VS — идентичен для любого каскада,
    /// отличается только view-proj матрица и целевой DSV), поэтому не
    /// дублируются на каждый каскад отдельно. Не может переиспользовать
    /// `self.root_signature`/`self.pipeline_state` основного 3D-прохода
    /// (тот ожидает PS и output-цель RTVFormat = R16G16B16A16_FLOAT, а не
    /// депф-онли DSV).
    shadow_vs: Option<ShaderBlob>,
    shadow_root_signature: Option<ID3D12RootSignature>,
    shadow_pipeline_state: Option<ID3D12PipelineState>,

    /// VS occluder-прохода — см. `compile_occluder_shaders`.
    occluder_vs: Option<ShaderBlob>,
    /// Root signature occluder-прохода (второе устройство) — один CBV
    /// (view-proj), см. `create_occluder_root_signature`.
    occluder_root_signature: Option<ID3D12RootSignature>,
    /// PSO occluder-прохода (второе устройство), depth-only, инстансинг.
    occluder_pipeline_state: Option<ID3D12PipelineState>,
    /// Depth-таргет occluder-прохода на ВТОРОЙ карте (НЕ `crate::render::
    /// RenderTexture` — тот жёстко использует `crate::get_device()`
    /// (первая карта), поэтому под второе устройство отдельный
    /// минимальный тип `SecondaryDepthTarget`, см. ниже).
    occluder_depth_target: Option<SecondaryDepthTarget>,
    /// DSV-heap occluder depth-таргета (второе устройство, свой отдельный
    /// heap — heap'ы, как и всё остальное в D3D12, привязаны к device).
    occluder_dsv_heap: Option<ID3D12DescriptorHeap>,
    occluder_dsv: D3D12_CPU_DESCRIPTOR_HANDLE,
    /// Статический unit-cube (8 вершин, только позиция — occluder VS сам
    /// разворачивает куб через instance min/max, см. `compile_occluder_shaders`)
    /// на второй карте. Создаётся ОДИН раз, никогда не меняется.
    occluder_cube_vertex_buffer: Option<SecondaryBuffer>,
    occluder_cube_index_buffer: Option<SecondaryBuffer>,
    /// Per-frame перезаписываемый instance-буфер (world-space AABB min/max
    /// на occluder) на второй карте — растёт степенями двойки, тот же
    /// паттерн, что `ensure_light_buffer_capacity` (см. там подробности).
    occluder_instance_buffer: Option<SecondaryBuffer>,
    occluder_instance_capacity: usize,
    /// CBV view-proj occluder-прохода (второе устройство), перезаписывается
    /// каждый раз, когда отправляется новый occluder-проход.
    occluder_viewproj_buffer: Option<SecondaryBuffer>,
    /// Постоянные (НЕ пересоздаваемые каждый кадр) command allocator/list
    /// второй карты для occluder-прохода — в отличие от `audio.rs`
    /// (разовые операции загрузки клипа), здесь проход отправляется
    /// потенциально каждый кадр, так что переиспользуем один и тот же
    /// allocator/list, точно как основной рендер-цикл переиспользует
    /// `self.command_allocator`/`self.command_list` (Reset вместо
    /// пересоздания).
    occluder_command_allocator: Option<ID3D12CommandAllocator>,
    occluder_command_list: Option<ID3D12GraphicsCommandList>,
    /// Fence второй карты для occluder-прохода — ОПРАШИВАЕТСЯ
    /// (`GetCompletedValue()`), НИКОГДА не ждётся блокирующе
    /// (`WaitForSingleObject`) в per-frame пути, см. `poll_occluder_readback`.
    occluder_fence: Option<ID3D12Fence>,
    occluder_fence_value: u64,
    /// true, когда occluder-проход отправлен на GPU второй карты, но
    /// readback ещё не подтверждён завершённым — предотвращает повторную
    /// отправку нового прохода поверх ещё не прочитанного предыдущего
    /// (буферы переиспользуются, перезапись до readback испортила бы
    /// данные, которые GPU второй карты может как раз читать).
    occluder_pass_in_flight: bool,
    /// READBACK-буфер (CPU-читаемый) для копирования occluder depth
    /// обратно с второй карты — переиспользуется каждый раз (пересоздаётся
    /// только при росте разрешения, которого здесь не бывает — разрешение
    /// occluder depth-буфера фиксировано, см. `OCCLUDER_DEPTH_RESOLUTION`).
    occluder_readback_buffer: Option<SecondaryBuffer>,
    /// Итоговый CPU-side depth-буфер окклюдеров последнего УСПЕШНО
    /// прочитанного прохода — построчно упакованный (с учётом
    /// `RowPitch`, см. `poll_occluder_readback`) буфер `f32` глубины
    /// размером `OCCLUDER_DEPTH_RESOLUTION x OCCLUDER_DEPTH_RESOLUTION`.
    /// `None`, пока ни один проход ещё не завершился (или подсистема
    /// неактивна) — Part 2b (следующий шаг) обязан считать "не знаю,
    /// не отбрасывать" при `None`, ровно как этот код уже делает для
    /// диагностики.
    occluder_depth_cpu: Option<Vec<f32>>,
    /// ВАЖНО (как и bloom_a_is_srv/bloom_b_is_srv выше): явный трекер
    /// текущего состояния КАЖДОГО каскада по отдельности — избегаем no-op
    /// ResourceBarrier на первом кадре (создаются уже в DEPTH_WRITE, см.
    /// `create_shadow_map`) точно так же, как раньше пришлось чинить для
    /// bloom-таргетов (см. подробный комментарий про этот класс бага у
    /// bloom_a_is_srv). `true` = сейчас PIXEL_SHADER_RESOURCE (можно
    /// читать в основном PS), `false` = сейчас DEPTH_WRITE (можно писать
    /// shadow-проходом).
    shadow_maps_are_srv: [bool; NUM_CASCADES],
    /// Отдельный константный буфер под shadow-проход — ОБЩИЙ для всех
    /// каскадов (см. `constant_buffer::ShadowConstants` — одна матрица на
    /// слот, а не вся TransformConstants) и
    /// `ensure_shadow_constant_buffer_capacity`. Ёмкость теперь считается
    /// на `NUM_CASCADES` полных проходов по сцене за кадр (каждый
    /// объект рисуется в КАЖДЫЙ каскад отдельно — см. render_frame), а не
    /// один — иначе слотов не хватило бы уже на втором каскаде того же
    /// кадра. Тот же паттерн роста/удвоения на 2 back buffer'а, что и у
    /// `constant_buffer` основного прохода.
    shadow_constant_buffer: Option<Buffer>,
    shadow_constant_buffer_capacity: usize,
    /// ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-подсветка):
    /// SRV основного depth-таргета (`renderer.depth_stencil`) — нужен
    /// volumetric raymarch-проходу, чтобы восстанавливать мировую позицию
    /// каждого экранного пикселя (см. подробное обоснование у
    /// `RenderTexture::create_depth_stencil` в render.rs). Отдельный
    /// SHADER_VISIBLE-хип на 1 дескриптор, аналогично `shadow_srv_heap` —
    /// не добавляется в `renderer.srv_uav_heap`, так как читается ДРУГИМ
    /// проходом (volumetric, не tonemap composite) с другой root signature.
    depth_srv_heap: Option<ID3D12DescriptorHeap>,
    depth_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,

    /// Текущее игровое время суток в часах, [0.0, 24.0) — 0 = полночь,
    /// 12 = полдень. Продвигается в `update_day_night` со скоростью
    /// `day_night_speed` часов игрового времени за одну РЕАЛЬНУЮ секунду.
    pub time_of_day: f32,
    /// Скорость течения времени суток (игровых часов в секунду). 1.0
    /// означает "полные сутки за 24 реальные секунды" — удобно для
    /// отладки/демонстрации; для обычной игры это будет намного меньше
    /// (например, 24.0/1200.0 — полные сутки за 20 реальных минут).
    /// Настраивается через `set_day_night_speed`, по умолчанию — 0 (время
    /// стоит на месте, пока приложение явно не включит смену дня/ночи —
    /// см. подробности выбора дефолта в `AlkashEngine::new`).
    pub day_night_speed: f32,
    /// ДОБАВЛЕНО: сохранённые исходные записи `IndividualLight` вместе с
    /// их id в FirstFires (возвращённым `add_light` в
    /// `load_lights_from_alfar`) — раньше `IndividualLight` конвертировался
    /// в `GPULight` и сразу выбрасывался, поэтому move/flicker/
    /// active_from/active_to были недостижимы после загрузки .alfar (эти
    /// поля попросту нигде не сохранялись). Без этого списка
    /// `update_day_night` не смог бы ни промодулировать мерцание, ни
    /// найти, какой GPULight в FirstFires нужно обновить через
    /// `LightPlugin::update_light`.
    managed_lights: Vec<ManagedLight>,
    /// Накопленная фаза шума мерцания на каждый управляемый источник —
    /// хранится отдельно от `managed_lights` (а не как поле в
    /// ManagedLight), чтобы не путать "статические данные из .alfar" с
    /// "runtime-состоянием, которое движок сам меняет каждый кадр".
    /// Индекс совпадает с индексом в `managed_lights`.
    flicker_phase: Vec<f32>,

    /// Half-res (та же половина ширины/высоты, что и bloom-таргеты — свет,
    /// рассеянный в воздухе, по природе низкочастотный, полное разрешение
    /// не даёт заметной разницы в качестве, но заметно дороже) render
    /// target, в который raymarch-шейдер аккумулирует видимую вдоль луча
    /// камера->пиксель долю "солнечного" света (проверяя каждый шаг луча
    /// через shadow map — освещён ли он, или загорожен геометрией). Не
    /// ping-pong (в отличие от bloom_texture_a/b) — здесь нет отдельного
    /// blur-прохода, raymarch сам по себе уже даёт достаточно гладкий
    /// результат при разумном числе шагов, а half-res + bilinear-апскейл
    /// при финальном чтении в tonemap composite дополнительно сглаживает.
    volumetric_texture: Option<crate::render::RenderTexture>,
    volumetric_rtv: D3D12_CPU_DESCRIPTOR_HANDLE,
    volumetric_srv_gpu_final: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// Отдельные RTV heap (1 дескриптор — сам volumetric-таргет) и SRV heap
    /// (3 дескриптора: 0=depth, 1=shadow map, 2=volumetric-таргет для
    /// финального чтения tonemap-проходом) — volumetric raymarch-шейдеру
    /// нужны ОБА входа (depth + shadow map) одновременно в одной
    /// descriptor table, поэтому они должны быть смежными дескрипторами в
    /// ОДНОМ heap (то же требование D3D12, что уже объяснено у
    /// `create_bloom_resources` про смежность HDR/bloom SRV).
    volumetric_rtv_heap: Option<ID3D12DescriptorHeap>,
    volumetric_srv_heap: Option<ID3D12DescriptorHeap>,
    /// GPU-адрес НАЧАЛА descriptor table (depth, индекс 0) внутри
    /// `volumetric_srv_heap` — передаётся в SetGraphicsRootDescriptorTable
    /// для raymarch-прохода; шейдер видит depth как t0, shadow map как t1
    /// (смежный следующий дескриптор в том же heap).
    volumetric_srv_gpu_raymarch: D3D12_GPU_DESCRIPTOR_HANDLE,
    volumetric_vs: Option<ShaderBlob>,
    volumetric_ps: Option<ShaderBlob>,
    volumetric_root_signature: Option<ID3D12RootSignature>,
    volumetric_pipeline_state: Option<ID3D12PipelineState>,
    volumetric_constant_buffer: Option<Buffer>,
    /// Как и у bloom/shadow: явный трекер текущего resource state — та же
    /// защита от no-op ResourceBarrier на первом кадре (создаётся в
    /// RENDER_TARGET, см. `create_hdr_target`, которым переиспользуется
    /// реализация под volumetric-таргет). `true` = сейчас
    /// PIXEL_SHADER_RESOURCE, `false` = сейчас RENDER_TARGET.
    volumetric_is_srv: bool,
    /// ДОБАВЛЕНО (Фаза 8): раньше `renderer.depth_stencil` НИКОГДА не
    /// покидал DEPTH_WRITE за весь срок жизни движка (только писался и
    /// тестировался основным 3D-проходом, никогда не читался как SRV) —
    /// поэтому явного трекера состояния не требовалось. Volumetric
    /// raymarch-проходу нужно ПРОЧИТАТЬ его как SRV (см.
    /// `create_depth_srv_resources`/render_frame) — то же самое отслеживание
    /// состояния, что уже применяется к shadow_map/bloom_a/bloom_b выше,
    /// требуется и здесь: без него второй и последующие кадры не знали бы,
    /// что depth_stencil уже был возвращён в DEPTH_WRITE предыдущим кадром
    /// (тот же принцип, что и у `shadow_map_is_srv`).
    depth_stencil_is_srv: bool,

    /// ДОБАВЛЕНО (World Streaming — подключение .alworld к движку):
    /// текущий загруженный мир (метаданные — где какие чанки, размер
    /// чанка, streaming config) + рантайм-состояние стриминга (какие
    /// чанки сейчас реально загружены и какие сущности Scene им
    /// принадлежат). `None`, пока `load_world()` ни разу не вызывался —
    /// `update_world_streaming()` в этом случае безопасно ничего не
    /// делает (см. её реализацию), это НЕ ошибка — многие сцены
    /// (main.rs/main1.rs/main2.rs с одиночными кубами) вообще не
    /// используют мировой стриминг.
    pub world: Option<WorldStreamingState>,
    /// ДОБАВЛЕНО (World Streaming): кэш mesh_index placeholder-геометрии
    /// (единичный куб), используемой ТОЛЬКО как fallback, когда реальный
    /// `.altex` объекта чанка не удалось загрузить (файл отсутствует,
    /// повреждён, путь "placeholder" — см. `load_object_mesh`/
    /// `load_placeholder_mesh`).
    /// `None`, пока фолбэк ни разу не понадобился.
    world_chunk_placeholder_mesh: Option<usize>,
    /// ДОБАВЛЕНО (загрузчик .altex -> GPU Mesh): кэш "путь к .altex файлу
    /// -> список mesh_index уже загруженных GPU-мешей из него" — один и
    /// тот же .altex (например меш фонарного столба или типового здания)
    /// обычно используется МНОГИМИ объектами МНОГИХ чанков; без кэша
    /// каждое появление объекта в новом чанке заново парсило бы файл с
    /// диска и заново создавало бы идентичный GPU vertex/index buffer —
    /// расточительно и по CPU (парсинг), и по GPU-памяти (дублирующиеся
    /// буферы одной и той же геометрии). Список (а не один mesh_index),
    /// т.к. один .altex может содержать НЕСКОЛЬКО мешей (см.
    /// `AltexFile::meshes`) — здание может состоять из нескольких
    /// отдельных частей с разными материалами.
    altex_mesh_cache: std::collections::HashMap<String, Vec<usize>>,

    /// ДОБАВЛЕНО (масштабирование стриминга на пул планировщика — см.
    /// `world_streaming.rs::load_chunk_data`): кэш "путь к `.altex` файлу
    /// -> уже РАСПАРСЕННЫЙ (не GPU!) файл", РАЗДЕЛЯЕМЫЙ между всеми
    /// одновременно выполняющимися задачами загрузки чанков на разных
    /// потоках `EngineScheduler` — поэтому `Arc<Mutex<...>>`, а не
    /// голый `HashMap`, как у `altex_mesh_cache` выше (тот кэш живёт
    /// только на главном потоке и в синхронизации не нуждается). Это
    /// ДРУГОЙ кэш, чем `altex_mesh_cache`: тот хранит уже готовые GPU
    /// mesh_index (может использоваться только с главного потока, где
    /// живёт D3D12 device), этот — сырые распарсенные CPU-структуры
    /// `AltexFile` (безопасно шарятся между потоками загрузки, задача
    /// создания GPU-ресурсов из них остаётся на главном потоке, см.
    /// `load_object_mesh_from_parsed`).
    altex_parse_cache: AltexParseCache,

    /// Канал, по которому потоки пула планировщика (`EngineScheduler`,
    /// см. `request_chunk_load`) присылают готовые результаты загрузки
    /// чанков — `Sender` клонируется в каждую фоновую задачу
    /// (`mpsc::Sender` поддерживает несколько отправителей на один
    /// `Receiver`), сам `AlkashEngine` хранит один `Receiver`
    /// (`chunk_loader_rx`), опрашиваемый раз в кадр в
    /// `drain_pending_chunk_io`.
    chunk_loader_result_tx: std::sync::mpsc::Sender<ChunkLoadResult>,
    chunk_loader_rx: std::sync::mpsc::Receiver<ChunkLoadResult>,
    /// Счётчик "поколений" мира — увеличивается на 1 при КАЖДОМ вызове
    /// `load_world` (см. её реализацию). Нужен, потому что фоновая
    /// загрузка теперь может занимать НЕСКОЛЬКО кадров: если игрок (или
    /// код игры) вызовет `unload_world`/`load_world` заново, ПОКА в фоне
    /// ещё обрабатывается запрос от СТАРОГО мира, результат придёт уже
    /// ПОСЛЕ того, как `self.world` указывает на совершенно другой мир —
    /// `chunk_idx` из старого результата в лучшем случае бессмысленен, в
    /// худшем указывает на чанк НОВОГО мира с другим содержимым
    /// (индексы `Vec<ChunkDescriptor>` начинаются с 0 в любом мире).
    /// `drain_pending_chunk_io` сравнивает `result.generation` с текущим
    /// `world_generation` и молча отбрасывает результат при несовпадении
    /// — та же идея, что `queued`-флаг чанка защищает от повторной
    /// постановки в очередь, только на уровне целого мира, а не одного
    /// чанка.
    world_generation: u64,

    /// Сколько СВЕРХ NUM_CASCADES material-слотов сейчас реально вмещает
    /// `shadow_srv_heap` (растёт степенями двойки, как и
    /// `light_buffer_capacity`, — не пересоздаётся на каждую новую
    /// текстуру).
    material_srv_capacity: u32,
    /// Сколько material-слотов реально ЗАНЯТО (следующий свободный —
    /// `NUM_CASCADES + material_texture_count`).
    material_texture_count: u32,
    /// Живые GPU-ресурсы текстур — должны жить как минимум столько же,
    /// сколько дескрипторы, на них ссылающиеся, поэтому хранятся здесь
    /// (в `AlkashEngine`), а не как временные локальные переменные в
    /// месте загрузки.
    material_textures: Vec<crate::texture::Texture>,
    /// Кэш "путь к текстуре -> её SRV-индекс (уже с учётом NUM_CASCADES-
    /// смещения)" — та же идея, что и `altex_mesh_cache` выше: одна и та
    /// же текстура (например обычный кирпич/асфальт) типично
    /// переиспользуется МНОГИМИ разными .altex-мешами/материалами,
    /// повторная загрузка и повторный SRV на каждое использование были
    /// бы расточительны.
    texture_cache: std::collections::HashMap<String, u32>,
    /// ДОБАВЛЕНО: индекс SRV (с учётом NUM_CASCADES-смещения) нейтральной
    /// белой 1x1 текстуры (albedo (1,1,1,1)) — создаётся ОДИН раз лениво
    /// при первом обращении (см. `ensure_white_texture`). Используется как
    /// fallback ВЕЗДЕ, где меш не имеет собственной albedo-текстуры
    /// (`Mesh::albedo_srv_index == None`) — избавляет пиксельный шейдер от
    /// отдельной HLSL-ветки "текстуры нет вообще" (см. main() в
    /// compile_default_shaders): умножение на (1,1,1,1) не меняет
    /// освещённый вершинный цвет, что в точности воспроизводит поведение
    /// движка ДО этой задачи (когда albedo-текстур не существовало
    /// вообще, работал только вершинный цвет).
    white_texture_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): индекс SRV нейтральной
    /// "плоской" normal map (128,128,255,255 — RGB-кодировка tangent-space
    /// вектора (0,0,1), т.е. "нормаль совпадает с геометрической, карта
    /// ничего не меняет") — тот же fallback-принцип, что и
    /// `white_texture_srv_index`, но для register t7 вместо t6.
    flat_normal_srv_index: Option<u32>,
    /// ДОБАВЛЕНО (Задача #15, normal mapping): индекс SRV нейтральной
    /// metallic-roughness текстуры (dummy — реальные значения в этом
    /// случае приходят из root constants `Mesh::material_metallic`/
    /// `material_roughness`, см. `create_root_signature`; эта текстура
    /// нужна только чтобы register t8 указывал на ВАЛИДНЫЙ SRV, а не на
    /// неинициализированный слот кучи, когда у меша нет собственной MR-карты).
    neutral_mr_srv_index: Option<u32>,
}

/// Разрешение shadow map directional-света в тексселях на сторону — см.
/// подробное обоснование выбора в `RenderTexture::create_shadow_map`
/// (render.rs). Константа, а не поле — единственный источник истины,
/// используемый и при создании ресурса, и при записи
/// `TransformConstants::shadow_map_size` (шейдеру нужно знать точный
/// размер текселя для шага PCF-сэмплирования).
pub const SHADOW_MAP_RESOLUTION: u32 = 2048;

/// ДОБАВЛЕНО (каскадные тени / CSM — расширение Фазы 6): число каскадов.
/// 3 — стандартный практический компромисс между качеством и стоимостью
/// (больше каскадов даёт более плавный переход плотности текселей с
/// дистанцией, но линейно увеличивает стоимость shadow-прохода — сцена
/// рисуется в КАЖДЫЙ каскад отдельно, см. render_frame). На
/// зафиксированном минимуме железа (i3-12100F/RTX 3050 8GB) 3 полных
/// depth-only прохода по сцене за кадр — разумный бюджет, 4+ уже
/// заметно дороже без пропорционального выигрыша в качестве для города
/// умеренной плотности застройки.
pub const NUM_CASCADES: usize = 3;

/// ДОБАВЛЕНО (каскадные тени / CSM): границы каскадов в ЕДИНИЦАХ ДОЛИ
/// camera.far (не абсолютные метры — далность обзора камеры может
/// меняться, доля пересчитывается в метры на лету в
/// `compute_cascade_view_proj`). Не равномерное деление (0.33/0.66/1.0),
/// а логарифмически-смещённое к камере распределение — воспринимаемая
/// плотность текселей падает с дистанцией нелинейно (объекты вблизи
/// камеры занимают на экране НАМНОГО больше пикселей на единицу мировой
/// длины, чем далёкие), поэтому ближний каскад сознательно ýже (покрывает
/// меньшую долю дальности), а дальний — шире. Стандартная практика CSM
/// (см. например Microsoft DirectX SDK "Cascaded Shadow Maps" sample).
pub const CASCADE_SPLITS: [f32; NUM_CASCADES] = [0.08, 0.25, 1.0];

/// Разрешение occluder depth-буфера на второй карте. Сознательно НИЗКОЕ —
/// это не картинка для показа пользователю, а грубая маска "что примерно
/// закрыто" для CPU-теста видимости; чем меньше разрешение, тем дешевле
/// рендер, readback и Map/копирование каждый кадр. 256x256 достаточно для
/// консервативной оценки на уровне отдельных крупных объектов.
const OCCLUDER_DEPTH_RESOLUTION: u32 = 256;

/// Минимальный world-space радиус ограничивающей сферы объекта, чтобы он
/// рассматривался как КАНДИДАТ-окклюдер (не как то, что может быть скрыто
/// другими, а как то, что само может закрывать другие объекты). Мелкие
/// объекты (столбы, мелкий реквизит) не стоят отдельного instance-слота —
/// их вклад в реальное перекрытие кадра пренебрежимо мал, а количество
/// таких объектов в сцене обычно велико (раздувало бы instance-буфер).
const OCCLUDER_MIN_WORLD_RADIUS: f32 = 2.0;

/// Инскрайб-коэффициент: половина стороны куба, ВПИСАННОГО в сферу
/// радиуса `r`, равна `r / sqrt(3)`. Это гарантирует, что box-occluder
/// НИКОГДА не выходит за пределы реальной ограничивающей сферы объекта —
/// то есть никогда не может ошибочно "закрыть" то, что на самом деле
/// видно (главное требование корректности для occlusion culling: ложно-
/// отрицательные результаты culling'а недопустимы, ложно-положительные —
/// то есть "не отбросили то, что на самом деле скрыто" — это просто
/// упущенная оптимизация, не баг рендера).
const OCCLUDER_INSCRIBE_FACTOR: f32 = 0.57735026;

/// Выравнивает `value` вверх до ближайшего кратного 256 — требование
/// D3D12 к `RowPitch` при `CopyTextureRegion` в буфер
/// (`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`), используется в
/// `create_occluder_resources`/`submit_occluder_pass`/`poll_occluder_readback`.
fn align_to_256(value: u64) -> u64 {
    (value + 255) & !255
}

/// ДОБАВЛЕНО (физика автомобиля — вращение кузова): конвертирует
/// кватернион ориентации `(x, y, z, w)`, приходящий из физики
/// (`PhysicsBody::orientation`, реально интегрируемый в inertial), в углы
/// Эйлера `[rx, ry, rz]` в ТОЙ ЖЕ конвенции, что уже использует
/// `Transform::local_matrix()` в scene.rs: `R = Rz(rot[2]) * Ry(rot[1]) *
/// Rx(rot[0])`, точка преобразуется как `R * p` (см. `local_matrix`).
///
/// Реализация — стандартное closed-form извлечение углов из матрицы
/// вращения, построенной из кватерниона, для конвенции intrinsic ZYX
/// (в этом порядке умножения матриц: сначала строим матрицу вращения из
/// кватерниона напрямую по стандартным формулам, затем разбираем её на
/// углы тем же способом, каким её раскладывал бы `Rz*Ry*Rx`). Это
/// эквивалентно прямому выводу через arctan/arcsin из компонент матрицы,
/// без промежуточного построения полной 3x3 матрицы как отдельного типа —
/// формулы ниже как раз и есть развёрнутые компоненты `Rz*Ry*Rx`.
///
/// Вырожденный случай (gimbal lock, ry ≈ ±90°) обрабатывается отдельной
/// веткой — иначе atan2(0, 0) даёт неопределённый (хотя и не NaN, в Rust
/// atan2(0,0)=0) результат, из-за чего вращение вокруг двух совпавших
/// осей "прыгало" бы каждый кадр между произвольными комбинациями rx/rz,
/// дающими одну и ту же физическую ориентацию.
fn quaternion_to_euler_zyx(q: [f32; 4]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);

    let r20 = 2.0 * (x * z - w * y);
    let r00 = 1.0 - 2.0 * (y * y + z * z);
    let r10 = 2.0 * (x * y + w * z);
    let r21 = 2.0 * (y * z + w * x);
    let r22 = 1.0 - 2.0 * (x * x + y * y);

    let sin_ry = (-r20).clamp(-1.0, 1.0);
    let ry = sin_ry.asin();

    if sin_ry.abs() > 0.999999 {
        let r01 = 2.0 * (x * y - w * z);
        let r11 = 1.0 - 2.0 * (x * x + z * z);
        let rz = (-r01).atan2(r11);
        [0.0, ry, rz]
    } else {
        let rx = r21.atan2(r22);
        let rz = r10.atan2(r00);
        [rx, ry, rz]
    }
}