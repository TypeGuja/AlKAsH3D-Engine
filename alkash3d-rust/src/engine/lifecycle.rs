//! Жизненный цикл движка: конструктор (`new` — все поля `AlkashEngine` с их
//! начальными значениями), `init()` (создание окна/D3D12-устройства и всех
//! пайплайнов по порядку), `update()` (тикает все подсистемы за кадр —
//! физика, скрипты, день/ночь, стриминг мира, каллинг света, аудио),
//! `shutdown()` (дожидается GPU, освобождает все GPU-ресурсы, закрывает
//! окно) и `impl Drop` (гарантирует `shutdown()` даже при panic/раннем
//! выходе).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы, финальный шаг). Перенос
//! дословный, тела методов не менялись.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::*;
use crate::math::Vec3;
use crate::camera::Camera;
use crate::constant_buffer::TransformConstants;
use crate::input::InputState;
use super::{AlkashEngine, UpdateBreakdownMs, NUM_CASCADES, NEXT_FENCE_VALUE, AltexParseCache, ChunkLoadResult, GraphicsSettings, MSAA_SAMPLES};

impl AlkashEngine {
    pub fn new(width: u32, height: u32) -> Self {
        // ИЗМЕНЕНО (масштабирование стриминга — см. развёрнутое обоснование
        // в шапке world_streaming.rs): раньше здесь запускался ОДИН
        // выделенный поток-загрузчик (`spawn_chunk_loader_thread`) со
        // своей парой каналов запрос/результат. Теперь запросы вообще не
        // передаются каналом — каждый становится задачей `scheduler`
        // (создан чуть ниже), выполняемой на одном из воркеров пула;
        // остаётся только канал РЕЗУЛЬТАТОВ (`Sender` клонируется в
        // каждую задачу, `Receiver` живёт здесь одним экземпляром).
        let (chunk_loader_result_tx, chunk_loader_rx) = std::sync::mpsc::channel::<ChunkLoadResult>();

        Self {
            scheduler: Arc::new(EngineScheduler::new()),
            update_breakdown_ms: UpdateBreakdownMs::default(),
            // ДОБАВЛЕНО (runtime-переключаемые SSAO/bloom/volumetric/
            // shadows/MSAA — по прямому запросу пользователя): значения по
            // умолчанию — полное качество, БИТ В БИТ то же поведение, что
            // было у движка до этой правки (ни один существующий бинарник,
            // который не вызывает `set_graphics_settings`, не заметит
            // разницы). `msaa_samples` вычислен здесь же из
            // `GraphicsSettings::default().msaa` (см. `MSAA_SAMPLES`) —
            // `set_graphics_settings` пересчитывает его заново, если
            // приложение вызовет её ДО `init()`.
            graphics_settings: GraphicsSettings::default(),
            msaa_samples: MSAA_SAMPLES,
            renderer: None,
            meshes: Vec::new(),
            mesh_instances: Vec::new(),
            lod_groups: std::collections::HashMap::new(),
            root_signature: None,
            pipeline_state: None,
            vs: None,
            ps: None,
            camera: Camera::new(width, height),
            constant_buffer: None,
            transform_constants: TransformConstants::new(),
            constant_buffer_capacity: 0,
            physics: None,
            lights: None,
            audio: None,
            native_scripts: std::collections::HashMap::new(),
            active_scripts: Vec::new(),
            script_frame_counter: 0,
            python_scripts: std::collections::HashMap::new(),
            physics_links: Vec::new(),
            hwnd: None,
            running: false,
            is_fullscreen: false,
            saved_window_rect: None,
            saved_window_style: None,
            width,
            height,
            clear_color: [0.05, 0.05, 0.1, 1.0],
            shutdown_in_progress: false,
            resizing_live: false,
            pending_resize: None,
            frame_fence_values: vec![0, 0],
            warm_up_mode: false,
            scene: crate::scene::Scene::new(),
            input: InputState::new(),
            light_ambient: None,
            light_global_settings: None,
            light_buffer: None,
            light_buffer_capacity: 0,
            grid_cells_buffer: None,
            grid_cells_buffer_capacity: 0,
            grid_entries_buffer: None,
            grid_entries_buffer_capacity: 0,
            tonemap_vs: None,
            tonemap_ps: None,
            tonemap_root_signature: None,
            tonemap_pipeline_state: None,
            tonemap_constant_buffer: None,
            bloom_extract_ps: None,
            bloom_blur_ps: None,
            bloom_root_signature: None,
            bloom_extract_pipeline_state: None,
            bloom_blur_pipeline_state: None,
            bloom_params_buffer: None,
            bloom_texture_a: None,
            bloom_rtv_a: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            bloom_srv_a_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            bloom_texture_b: None,
            bloom_rtv_b: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            bloom_srv_b_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            bloom_rtv_heap: None,
            bloom_srv_heap: None,
            bloom_a_is_srv: false,
            bloom_b_is_srv: false,

            shadow_maps: [None, None, None],
            shadow_dsv_heap: None,
            shadow_dsvs: [D3D12_CPU_DESCRIPTOR_HANDLE::default(); NUM_CASCADES],
            shadow_srv_heap: None,
            shadow_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            shadow_vs: None,
            shadow_root_signature: None,
            shadow_pipeline_state: None,
            occluder_vs: None,
            occluder_root_signature: None,
            occluder_pipeline_state: None,
            occluder_depth_target: None,
            occluder_dsv_heap: None,
            occluder_dsv: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            occluder_cube_vertex_buffer: None,
            occluder_cube_index_buffer: None,
            occluder_instance_buffer: None,
            occluder_instance_capacity: 0,
            occluder_viewproj_buffer: None,
            occluder_command_allocator: None,
            occluder_command_list: None,
            occluder_fence: None,
            occluder_fence_value: 0,
            occluder_pass_in_flight: false,
            occluder_readback_buffer: None,
            occluder_depth_cpu: None,
            shadow_maps_are_srv: [false; NUM_CASCADES],
            shadow_constant_buffer: None,
            shadow_constant_buffer_capacity: 0,

            time_of_day: 12.0,
            day_night_speed: 0.0,
            managed_lights: Vec::new(),
            flicker_phase: Vec::new(),

            volumetric_texture: None,
            volumetric_rtv: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            volumetric_srv_gpu_final: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            volumetric_rtv_heap: None,
            volumetric_srv_heap: None,
            volumetric_srv_gpu_raymarch: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            volumetric_vs: None,
            volumetric_ps: None,
            volumetric_root_signature: None,
            volumetric_pipeline_state: None,
            volumetric_constant_buffer: None,
            volumetric_is_srv: false,
            depth_stencil_is_srv: false,

            ssao_vs: None,
            ssao_ps: None,
            ssao_root_signature: None,
            ssao_pipeline_state: None,
            ssao_texture: None,
            ssao_rtv: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            ssao_rtv_heap: None,
            ssao_depth_srv_heap: None,
            ssao_srv_gpu_depth: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            ssao_constant_buffer: None,
            ssao_is_srv: false,
            ssao_blur_texture: None,
            ssao_blur_rtv: D3D12_CPU_DESCRIPTOR_HANDLE::default(),
            ssao_blur_rtv_heap: None,
            ssao_blur_srv_heap: None,
            ssao_blur_srv_raw_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            ssao_blur_srv_mid_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),
            ssao_blur_cb_x: None,
            ssao_blur_cb_y: None,

            depth_srv_heap: None,
            depth_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE::default(),

            world: None,
            world_chunk_placeholder_mesh: None,
            altex_mesh_cache: std::collections::HashMap::new(),
            altex_parse_cache: AltexParseCache::default(),
            chunk_loader_result_tx,
            chunk_loader_rx,
            world_generation: 0,

            material_srv_capacity: 0,
            material_texture_count: 0,
            material_textures: Vec::new(),
            texture_cache: std::collections::HashMap::new(),
            white_texture_srv_index: None,
            flat_normal_srv_index: None,
            neutral_mr_srv_index: None,
        }
    }

    /// Останавливает игровой цикл (эквивалент нажатия ESC/закрытия окна),
    /// но БЕЗ закрытия окна напрямую — реальное освобождение ресурсов и
    /// закрытие окна происходит в `shutdown()`, как и при обычном
    /// закрытии через крестик. Используй это вместо того, чтобы решать
    /// "когда выходить" внутри самого движка — это дело приложения.
    pub fn request_exit(&mut self) {
        self.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = [r, g, b, a];
    }

    /// ДОБАВЛЕНО (по прямому запросу пользователя — включение/выключение
    /// SSAO/MSAA/bloom/volumetric/shadows переменной из кода бинарника, а
    /// не хардкодом внутри движка): вызывать ДО `init()` — значения читаются
    /// при построении рендер-пайплайна (шейдеры/PSO/ресурсы) и при попытке
    /// поменять их ПОСЛЕ `init()` эффекта не будет (потребовалась бы
    /// полная пересборка пайплайна, которую этот метод не делает). Без
    /// вызова этого метода движок ведёт себя ТОЧНО как раньше — полное
    /// качество, MSAA включён (см. `GraphicsSettings::default()`).
    pub fn set_graphics_settings(&mut self, settings: GraphicsSettings) {
        self.msaa_samples = if settings.msaa { MSAA_SAMPLES } else { 1 };
        self.graphics_settings = settings;
    }

    pub fn init(&mut self) -> Result<()> {
        println!("[ENGINE] Initializing Alkash3D Engine v{}...", VERSION);

        self.create_window()?;
        println!("[ENGINE] ✓ Window created");

        unsafe {
            D3D12Device::create()?;
            println!("[ENGINE] ✓ Device created");

            CommandQueue::create()?;
            println!("[ENGINE] ✓ Command queue created");

            let hwnd = self.hwnd.unwrap();
            SwapChain::create(hwnd.0 as isize, self.width, self.height, 2)?;
            println!("[ENGINE] ✓ Swap chain created");

            CommandList::create_allocators(2)?;
            println!("[ENGINE] ✓ Command allocators created");

            let fence = create_fence()?;
            {
                let mut state = STATE.lock().unwrap();
                state.fence = Some(fence);
                state.fence_values = vec![0, 0];
            }
            println!("[ENGINE] ✓ Fence created");

            let renderer = Renderer::new(self.width, self.height, 2, self.msaa_samples)?;
            self.renderer = Some(renderer);
            println!("[ENGINE] ✓ Renderer created");
        }

        self.compile_default_shaders()?;

        self.create_root_signature()?;

        self.create_pipeline_state()?;

        self.ensure_constant_buffer_capacity(128)?;
        println!("[ENGINE] ✓ Constant buffer created");

        self.compile_tonemap_shaders()?;
        self.create_tonemap_root_signature()?;
        self.create_tonemap_pipeline_state()?;

        let tonemap_cb = Buffer::create_constant_buffer(256)?;
        let default_tonemap_params: [f32; 4] = [1.0, 1.0, 0.0, 0.0];
        let bytes = unsafe {
            std::slice::from_raw_parts(default_tonemap_params.as_ptr() as *const u8, 16)
        };
        tonemap_cb.update_constant_buffer(bytes)?;
        self.tonemap_constant_buffer = Some(tonemap_cb);
        println!("[ENGINE] ✓ Tonemap constant buffer created (exposure=1.0, bloomIntensity=1.0 по умолчанию)");

        self.compile_bloom_shaders()?;
        self.create_bloom_root_signature()?;
        self.create_bloom_pipeline_states()?;
        self.create_bloom_resources()?;

        self.compile_shadow_shaders()?;
        self.create_shadow_root_signature()?;
        self.create_shadow_pipeline_state()?;
        self.create_shadow_resources()?;

        self.compile_occluder_shaders()?;
        self.create_occluder_root_signature()?;
        self.create_occluder_pipeline_state()?;
        self.create_occluder_resources()?;

        self.create_depth_srv_resources()?;
        self.compile_volumetric_shaders()?;
        self.create_volumetric_root_signature()?;
        self.create_volumetric_pipeline_state()?;
        self.create_volumetric_resources()?;

        self.compile_ssao_shaders()?;
        self.create_ssao_root_signature()?;
        self.create_ssao_pipeline_state()?;
        self.create_ssao_resources()?;

        // ДОБАВЛЕНО (runtime-переключаемые shadows — по прямому запросу
        // пользователя): безопасно переиспользует уже существующий
        // `disable_shadows_for_diagnostics()` (тот же, что уже использует
        // `bin/benchmark.rs`/`bin/example_minimal.rs`) — он обнуляет только
        // `shadow_pipeline_state` (пропускает РЕНДЕР В shadow map), а не
        // сами текстуры shadow map, поэтому ничего не может остаться
        // висячим дескриптором в main pass, который их читает. bloom/
        // volumetric/SSAO НАМЕРЕННО не переключаются так же здесь — их
        // "diagnostics"-версии обнуляют САМИ ТЕКСТУРЫ (см. комментарий у
        // `disable_bloom_for_diagnostics`), а composite-проход БЕЗУСЛОВНО
        // читает их SRV-слоты каждый кадр — обнулить текстуру означало бы
        // оставить в этих слотах дескриптор на уже уничтоженный ресурс.
        // Для них `graphics_settings.bloom/volumetric/ssao` проверяется
        // ПРЯМО в `render_frame` (см. там) — ресурсы остаются живыми, само
        // вычисление просто пропускается.
        if !self.graphics_settings.shadows {
            self.disable_shadows_for_diagnostics();
        }

        unsafe {
            ShowWindow(self.hwnd.unwrap(), SW_SHOW);
            UpdateWindow(self.hwnd.unwrap());
        }

        self.running = true;
        println!("[ENGINE] ✓ Initialization complete");
        Ok(())
    }

    pub fn update(&mut self, dt: f32, gravity: f32, camera_pos: [f32; 3], view_proj: [f32; 16]) {
        self.scheduler.reset_budget();

        macro_rules! timed {
            ($field:ident, $body:expr) => {{
                let __start = std::time::Instant::now();
                let __result = $body;
                let __ms = __start.elapsed().as_secs_f32() * 1000.0;
                if __ms > self.update_breakdown_ms.$field {
                    self.update_breakdown_ms.$field = __ms;
                }
                __result
            }};
        }

        timed!(physics_ms, {
            if let Some(physics) = &mut self.physics {
                physics.update(dt, gravity);
            }
        });
        timed!(sync_physics_ms, self.sync_physics_transforms());

        timed!(native_scripts_ms, self.update_native_scripts(dt));

        timed!(python_scripts_ms, self.update_python_scripts(dt));

        timed!(day_night_ms, self.update_day_night(dt));

        timed!(world_streaming_ms, self.update_world_streaming(Vec3::new(camera_pos[0], camera_pos[1], camera_pos[2])));
        timed!(chunk_io_ms, self.drain_pending_chunk_io());

        timed!(light_cull_ms, {
            if let Some(lights) = &mut self.lights {
                lights.cull(camera_pos, &view_proj, dt);
            }
        });

        timed!(audio_ms, {
            if let Some(audio) = &mut self.audio {
                let forward = self.camera.target - self.camera.position;
                audio.set_listener(crate::audio::Listener {
                    position: Vec3::new(camera_pos[0], camera_pos[1], camera_pos[2]),
                    forward: if forward.length_squared() > 1e-6 { forward.normalize() } else { Vec3::new(0.0, 0.0, 1.0) },
                    up: self.camera.up,
                    velocity: Vec3::ZERO,
                });
                audio.update(dt);
            }
        });
    }

    pub fn shutdown(&mut self) {
        if self.shutdown_in_progress {
            println!("[ENGINE] Shutdown already in progress");
            return;
        }
        self.shutdown_in_progress = true;

        println!("[ENGINE] Shutting down...");
        self.running = false;

        let mut gpu_hung = false;
        {
            // .unwrap_or_else(...) вместо .unwrap(): shutdown() вызывается из
            // Drop и должен пройти до конца даже если STATE уже отравлен
            // паникой на другом пути (иначе тут случилась бы повторная
            // паника поверх текущего unwind → abort в обход GPU-hang-таймаута
            // выше и force-exit ниже).
            let state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if let (Some(queue), Some(fence)) = (&state.command_queue, &state.fence) {
                let fence_value = NEXT_FENCE_VALUE.fetch_add(1, Ordering::SeqCst);
                unsafe {
                    println!("[ENGINE] Signaling fence (value={})...", fence_value);
                    let _ = queue.Signal(fence, fence_value);
                    println!("[ENGINE] Waiting for GPU to finish...");

                    let start = std::time::Instant::now();
                    while fence.GetCompletedValue() < fence_value {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        if start.elapsed() > std::time::Duration::from_secs(5) {
                            gpu_hung = true;
                            break;
                        }
                    }
                    if gpu_hung {
                        println!("[ENGINE] WARNING: GPU timeout, forcing shutdown");
                    } else {
                        println!("[ENGINE] GPU idle");
                    }
                }
            }
        }

        // ИЗМЕНЕНО (код-ревью: fence timeout здесь раньше просто логировался,
        // а дальше shutdown() всё равно шёл release'ить все GPU-ресурсы —
        // meshes.clear(), буферы, renderer и т.д. — то есть дальше дёргать
        // уже подвисший драйвер. Это и есть задокументированный механизм,
        // из-за которого зависание кадра 2 (fence timeout) превращалось не
        // просто в падение процесса, а в зависание ВСЕЙ машины именно
        // внутри meshes.clear() (см. car_stdout.txt/engine_car_log.txt).
        // fence.GetCompletedValue() не сигнализировал за 5 реальных секунд
        // означает: либо устройство реально removed, либо GPU физически
        // завис (в т.ч. если TDR отключён — тогда Windows вообще не
        // репортует removal, device_removed_reason() ниже даст None, но
        // это НЕ значит "всё в порядке"). В обоих случаях безопаснее не
        // продолжать релизить COM-объекты (Release() на подвисшем
        // устройстве может так же зависнуть), а сразу завершить процесс:
        // shutdown() везде в этом крейте (main.rs/main1.rs/main2.rs/
        // main_car.rs, а также Drop) вызывается последним шагом прямо
        // перед выходом из main(), так что "утечка" COM-объектов тут не
        // страшнее обычного завершения процесса — ОС и так заберёт все
        // хендлы, а вот попытка "вежливо" их отпустить через драйвер —
        // именно то, что вешало машину.
        if gpu_hung {
            match crate::device_removed_reason() {
                Some(reason) => eprintln!(
                    "[ENGINE] CRITICAL: GPU hang on shutdown, device removed: {reason}"
                ),
                None => eprintln!(
                    "[ENGINE] CRITICAL: GPU hang on shutdown — fence did not signal within \
                     5s and the device was not reported removed (TDR may be disabled). \
                     Skipping GPU resource release and exiting immediately instead of \
                     risking a full-machine freeze."
                ),
            }
            std::process::exit(1);
        }

        println!("[ENGINE] Clearing meshes...");
        self.meshes.clear();
        self.mesh_instances.clear();

        println!("[ENGINE] Releasing resources...");

        self.constant_buffer = None;
        self.constant_buffer_capacity = 0;
        self.vs = None;
        self.ps = None;
        self.pipeline_state = None;
        self.root_signature = None;
        self.renderer = None;

        self.bloom_texture_a = None;
        self.bloom_texture_b = None;
        self.bloom_rtv_heap = None;
        self.bloom_srv_heap = None;
        self.bloom_extract_ps = None;
        self.bloom_blur_ps = None;
        self.bloom_root_signature = None;
        self.bloom_extract_pipeline_state = None;
        self.bloom_blur_pipeline_state = None;
        self.bloom_params_buffer = None;

        self.tonemap_vs = None;
        self.tonemap_ps = None;
        self.tonemap_root_signature = None;
        self.tonemap_pipeline_state = None;
        self.tonemap_constant_buffer = None;

        for slot in self.shadow_maps.iter_mut() {
            *slot = None;
        }
        self.shadow_dsv_heap = None;
        self.shadow_srv_heap = None;
        self.shadow_vs = None;
        self.shadow_root_signature = None;
        self.shadow_pipeline_state = None;

        self.light_buffer = None;
        self.light_buffer_capacity = 0;
        self.grid_cells_buffer = None;
        self.grid_cells_buffer_capacity = 0;
        self.grid_entries_buffer = None;
        self.grid_entries_buffer_capacity = 0;

        self.material_textures.clear();
        self.texture_cache.clear();

        {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            state.info_queue = None;
        }

        println!("[ENGINE] Resetting global state...");
        {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());

            state.fence = None;
            state.fence_values.clear();
            state.command_allocators.clear();
            state.command_list = None;

            if let Some(swap_chain) = &state.swap_chain {
                unsafe {
                    let _ = swap_chain.SetFullscreenState(false, None);
                }
            }
            state.swap_chain = None;
            state.command_queue = None;
            state.device = None;
            state.descriptor_heaps.clear();
            state.root_signature = None;
            state.current_pso = None;
            state.bound_vertex_buffers.clear();
            state.bound_index_buffer = None;
            state.scheduler = None;
        }

        unsafe {
            if let Some(hwnd) = self.hwnd {
                println!("[ENGINE] Destroying window...");
                if IsWindow(Some(hwnd)).as_bool() {
                    DestroyWindow(hwnd);
                }
                self.hwnd = None;
            }
        }

        self.shutdown_in_progress = false;
        println!("[ENGINE] Shutdown complete");
    }
}

impl Drop for AlkashEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
