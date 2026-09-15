// src/app.rs - ПОЛНАЯ GPU ВЕРСИЯ (исправления: очередь загрузки, избежание double-borrow, оценка байтов без приватных типов)
use eframe::egui;
use egui::*;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::collections::HashMap;
use crate::gpu::GpuRenderer;
use crate::math::Vec3;
use crate::scene::{Scene, GameObject, ObjectType, MeshComponent, LightComponent, LightType, AudioSourceComponent, ScriptedEntityComponent, CameraComponent, ParticleSystemComponent};
use crate::editor::{Gizmo, CommandHistory, EditorTool};
use crate::systems::*;
use crate::assets::AssetLibrary;
use crate::ui;
use crate::material::Material;
use crate::mesh::Mesh;
use uuid::Uuid;

pub struct EditorApp {
    pub scene: Scene,
    pub history: CommandHistory,
    pub asset_library: AssetLibrary,
    pub camera_position: Vec3,
    pub camera_target: Vec3,
    pub camera_up: Vec3,
    pub camera_fov: f32,
    pub current_tool: EditorTool,
    pub gizmo: Gizmo,
    pub viewport_rect: Rect,
    pub show_hierarchy: bool,
    pub show_inspector: bool,
    pub show_console: bool,
    pub show_new_scene_dialog: bool,
    pub show_import_dialog: bool,
    pub new_scene_name: String,
    pub search_filter: String,
    pub last_mouse_pos: Option<Pos2>,
    pub left_mouse_pressed: bool,
    pub right_mouse_pressed: bool,
    pub middle_mouse_pressed: bool,
    pub status_message: String,
    pub fps: f32,
    pub frame_count: u64,
    pub last_frame_time: f64,
    pub last_update_time: f64,
    pub console_messages: VecDeque<(String, Color32)>,
    pub world_streamer: WorldStreamer,
    pub material_accel: MaterialAccelerator,
    pub shader_manager: ShaderManager,
    pub audio_system: SpatialAudioSystem,
    pub scripting: ScriptingEngine,
    pub cinematic: CinematicManager,
    pub cpu_render_limit: usize,
    pub pending_imports: Vec<PendingImport>,
    pub import_progress: f32,
    pub gpu_renderer: Option<GpuRenderer>,
    pub gpu_mesh_map: HashMap<uuid::Uuid, usize>,
    pub gpu_material_map: HashMap<uuid::Uuid, usize>,
    pub wgpu_render_state: Option<egui_wgpu::RenderState>,
    pub gpu_initialized: bool,
    pub gpu_texture_id: Option<egui::TextureId>,
    pub gpu_texture_size: (u32, u32),

    // Новые поля
    pub upload_queue: VecDeque<UploadTask>,
    pub max_upload_bytes_per_frame: usize,

    // ДОБАВЛЕНО (реальный интерактивный gizmo во вьюпорте — см.
    // editor/gizmo3d.rs и handle_gizmo_input/draw_gizmo ниже).
    pub gizmo_drag: Option<crate::editor::GizmoDrag>,
    pub gizmo_hover_axis: Option<crate::editor::GizmoAxisSel>,
    pub snap_enabled: bool,
    pub snap_translate: f32,
    pub snap_rotate_deg: f32,
    pub snap_scale: f32,

    // ДОБАВЛЕНО (браузер ассетов по всем папкам проекта — см. ui/asset_browser.rs).
    pub show_asset_browser: bool,
    pub asset_browser_root: std::path::PathBuf,
    pub asset_tree: Option<crate::ui::asset_browser::AssetNode>,

    // ДОБАВЛЕНО (редактор вершин/граней — по прямому запросу пользователя:
    // "сделай возможность редактировать фигуры, делать новые"): Tab
    // переключает Object Mode <-> Edit Mode для ОДНОГО выделенного
    // Mesh-объекта (см. toggle_edit_mode в app.rs); `edit_mesh_select_mode`
    // — что именно выделяет клик (вершину или грань); `edit_selected_*` —
    // множества выбранных индексов В ТЕКУЩЕМ редактируемом меше (индексы,
    // не Uuid — это индексы внутри `mesh.vertices`/треугольников, теряют
    // смысл при выходе из Edit Mode, поэтому очищаются при каждом входе/
    // выходе, см. toggle_edit_mode).
    pub edit_mode: bool,
    pub edit_mesh_object: Option<Uuid>,
    pub edit_mesh_select_mode: crate::editor::MeshSelectMode,
    pub edit_selected_vertices: std::collections::BTreeSet<usize>,
    pub edit_selected_faces: std::collections::BTreeSet<usize>,
    /// Отдельное от `gizmo_drag` состояние перетаскивания — gizmo в Edit
    /// Mode всегда работает как Move (перемещение вершин), независимо от
    /// `current_tool`, поэтому переиспользовать `gizmo_drag`/`apply_gizmo_delta`
    /// (жёстко завязанные на `scene.selected_ids`/`obj.transform`) было бы
    /// либо развилкой внутри уже и так плотного кода, либо риском
    /// незаметно сломать перемещение целых объектов. Раздельное состояние
    /// — раздельный, независимо проверяемый путь.
    pub mesh_gizmo_drag: Option<crate::editor::GizmoDrag>,
    pub mesh_gizmo_hover_axis: Option<crate::editor::GizmoAxisSel>,

    // ДОБАВЛЕНО (по прямому запросу пользователя: показывать все
    // анимационные точки во вьюпорте, чтобы их можно было двигать рукой) —
    // независимое от gizmo_drag/mesh_gizmo_drag состояние: маркер
    // keyframe'а перетаскивается напрямую мышью по плоскости, повёрнутой
    // лицом к камере (см. `screen_delta_to_world`/
    // `handle_keyframe_marker_input`), без оси/gizmo-хендлов — так как
    // маркеров может быть много одновременно (по одному на keyframe), а не
    // один на объект.
    pub dragging_keyframe: Option<DraggingKeyframe>,

    // ИСПРАВЛЕНО (по прямому запросу пользователя — прошлая версия
    // защищала ТОЛЬКО стартовое выравнивание выделения самого с собой, а
    // не притягивала к другим вершинам меша по ходу драга, что пользователь
    // и ожидал — "чтобы при перемещении оно залипало на высоте/плоскости
    // ЛЮБОЙ другой части меша"): во время перетаскивания вдоль оси
    // непрерывно ищем среди НЕвыделенных вершин того же меша ближайшую по
    // ЭТОЙ оси к тому месту, куда сейчас тянет мышь — если она в радиусе
    // захвата, движение "примагничивается" к её координате (вершины стоят
    // ровно на ней, а не там, где реально сейчас мышь) и остаётся там, пока
    // мышь не утащит достаточно далеко (радиус отпускания чуть больше
    // радиуса захвата — гистерезис, чтобы не дребезжало на границе).
    /// Мировые координаты (по оси текущего драга) всех НЕвыделенных вершин
    /// редактируемого меша — кандидаты для примагничивания, пересчитаны
    /// один раз в момент начала конкретного перетаскивания (не каждый кадр).
    pub mesh_gizmo_snap_candidates: Vec<f32>,
    /// Координата (по оси драга) в момент начала перетаскивания — точка
    /// отсчёта для `mesh_gizmo_snap_raw_delta` ниже.
    pub mesh_gizmo_snap_start_value: f32,
    /// Координата, которая РЕАЛЬНО сейчас отражена в позициях вершин —
    /// отличается от "сырой" желаемой позиции мыши, пока драг примагничен
    /// к какому-то кандидату (см. `mesh_gizmo_snap_target`).
    pub mesh_gizmo_snap_applied_value: f32,
    /// Накопленный НЕограниченный сдвиг мыши с начала перетаскивания (без
    /// учёта примагничивания) — "куда бы уехало выделение, если бы снапа
    /// не было вообще".
    pub mesh_gizmo_snap_raw_delta: f32,
    /// Координата кандидата, к которому драг сейчас примагничен, если есть.
    pub mesh_gizmo_snap_target: Option<f32>,
    // ИСПРАВЛЕНО (по прямому запросу пользователя: "оно отсоединяется от
    // куба, и выравнивается по центру, а должно по ближайшей грани до
    // стороны (или угла)"): раньше примагничивался ЦЕНТРОИД выделения
    // (`mesh_gizmo_snap_start_value`) — из-за этого при сдвиге вдоль оси, в
    // которой у выделения есть протяжённость (например, целая грань,
    // сдвигаемая вбок, а не по своей нормали), совпадать с кандидатом
    // заставляли СЕРЕДИНУ выделения, а не его ближний край. Нужно вместо
    // этого примагничивать ближайший к цели КРАЙ (мин. или макс. координату
    // выделения по этой оси) — тогда стыкуется реальная сторона/угол, а не
    // центр. Смещения края относительно центроида постоянны на всё время
    // перетаскивания (это чистый перенос, форма выделения не меняется), так
    // что достаточно зафиксировать их один раз в момент начала драга.
    /// `(мин. по оси координата выделения на старте) - (координата пивота
    /// на старте)` — см. комментарий выше.
    pub mesh_gizmo_snap_min_offset: f32,
    /// То же самое для максимальной координаты выделения по оси.
    pub mesh_gizmo_snap_max_offset: f32,

    // ДОБАВЛЕНО (undo/redo для mesh-редактора — по прямому запросу
    // пользователя, следующий пункт плана после снап-фичи): снимок меша,
    // сделанный в момент начала текущего перетаскивания gizmo в Edit Mode
    // (см. `EditorCommand::ModifyMesh` в `editor/history.rs`). `None`, когда
    // никакого драга не идёт. Используется только для undo/redo — не
    // путать с `mesh_gizmo_snap_*`, которые про примагничивание.
    pub mesh_edit_undo_snapshot: Option<crate::mesh::Mesh>,
    /// true, если во время текущего драга реально было хоть одно движение
    /// вершин (чтобы не засорять историю "пустыми" командами при клике без
    /// сдвига мыши).
    pub mesh_edit_history_dirty: bool,

    // ДОБАВЛЕНО (по прямому запросу пользователя: "давай делать эдитор под
    // каждый формат... чтобы они не лежали мёртвым грузом") — серия
    // отдельных, не завязанных на 3D-сцену редакторов формата, все по
    // одному паттерну (см. `ui/sound_bank_editor.rs` — первый и самый
    // подробно откомментированный).
    pub sound_bank_editor: SoundBankEditorState,
    pub route_editor: RouteEditorState,
    pub script_editor: ScriptEditorState,
    pub assembly_editor: AssemblyEditorState,
    pub car_preset_editor: CarPresetEditorState,
    pub material_library_editor: MaterialLibraryEditorState,

    // ДОБАВЛЕНО (по прямому запросу пользователя: "Discord Rich Presence
    // статус"): см. src/discord_presence.rs — тихий no-op, пока не задан
    // реальный DISCORD_CLIENT_ID.
    pub discord_presence: crate::discord_presence::DiscordPresence,
}

/// Состояние окна "🔊 Sound Bank Editor" — см. `ui/sound_bank_editor.rs`.
/// Не путать с размещением `AudioSource`-объектов в 3D-сцене (инспектор,
/// `ObjectType::AudioSource`) — это про редактирование самого `.alsnd`
/// как автономного набора данных (звук не пространственный, см. шапку
/// `converters/alsnd.rs`).
pub struct SoundBankEditorState {
    pub open: bool,
    pub bank_name: String,
    pub entries: Vec<crate::converters::alsnd::SoundEntryEdit>,
    /// Путь последнего загруженного/сохранённого файла — только для
    /// отображения в заголовке окна, ни на что не влияет.
    pub loaded_path: Option<String>,
}

impl Default for SoundBankEditorState {
    fn default() -> Self {
        Self {
            open: false,
            bank_name: "SoundBank".to_string(),
            entries: Vec::new(),
            loaded_path: None,
        }
    }
}

/// Состояние окна "🛣 Route Editor" — см. `ui/route_editor.rs`.
pub struct RouteEditorState {
    pub open: bool,
    pub routes: Vec<crate::converters::alroute::RouteEdit>,
    pub loaded_path: Option<String>,
}

impl Default for RouteEditorState {
    fn default() -> Self {
        Self { open: false, routes: Vec::new(), loaded_path: None }
    }
}

/// Состояние окна "📜 Script Registry Editor" — см. `ui/script_editor.rs`.
pub struct ScriptEditorState {
    pub open: bool,
    pub entries: Vec<crate::converters::alscript::ScriptEdit>,
    pub loaded_path: Option<String>,
}

impl Default for ScriptEditorState {
    fn default() -> Self {
        Self { open: false, entries: Vec::new(), loaded_path: None }
    }
}

/// Состояние окна "🔧 Assembly Editor" — см. `ui/assembly_editor.rs`.
pub struct AssemblyEditorState {
    pub open: bool,
    pub name: String,
    pub category: alkash3d_rs::AssemblyCategory,
    pub parts: Vec<crate::converters::alasm::PartEdit>,
    pub loaded_path: Option<String>,
}

impl Default for AssemblyEditorState {
    fn default() -> Self {
        Self { open: false, name: "Assembly".to_string(), category: alkash3d_rs::AssemblyCategory::Generic, parts: Vec::new(), loaded_path: None }
    }
}

/// Состояние окна "🚗 Car Preset Editor" — см. `ui/car_preset_editor.rs`.
pub struct CarPresetEditorState {
    pub open: bool,
    pub edit: crate::converters::alcar::CarPresetEdit,
    pub loaded_path: Option<String>,
}

impl Default for CarPresetEditorState {
    fn default() -> Self {
        Self { open: false, edit: crate::converters::alcar::CarPresetEdit::default(), loaded_path: None }
    }
}

/// Состояние окна "🎨 Material Library Editor" — см.
/// `ui/material_library_editor.rs`. Данные самой библиотеки — уже
/// существующий `AssetLibrary::materials`, здесь только UI-состояние.
pub struct MaterialLibraryEditorState {
    pub open: bool,
    pub new_material_name: String,
}

impl Default for MaterialLibraryEditorState {
    fn default() -> Self {
        Self { open: false, new_material_name: String::new() }
    }
}

pub struct PendingImport {
    pub path: String,
    pub receiver: mpsc::Receiver<Result<ImportResult, String>>,
}

#[derive(Debug)]
pub struct ImportResult {
    pub mesh_names: Vec<String>,
    pub meshes: Vec<(String, Mesh)>,
}

pub struct UploadTask {
    pub id: Uuid,
    pub name: String,
    pub mesh: Mesh,
    pub material: Material,
    pub estimated_bytes: usize,
}

/// Какой именно keyframe сейчас тащат мышью во вьюпорте — см.
/// `EditorApp::dragging_keyframe`/`handle_keyframe_marker_input`. `index` —
/// позиция в `Animation::position_track.keyframes` (единственный трек,
/// маркеры которого показываются/двигаются — Rotation/Scale двигать мышью
/// в 3D неестественно, их правят через Transform-секцию после перехода к
/// нужному времени, см. `ui/inspector.rs`).
pub struct DraggingKeyframe {
    pub object_id: Uuid,
    pub animation_name: String,
    pub index: usize,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::setup_egui_style(&cc.egui_ctx);

        let wgpu_render_state = cc.wgpu_render_state.clone();

        let mut scene = Scene::new("Untitled");
        let cube_mesh = Mesh::create_cube();
        let cube = GameObject::new("Cube", ObjectType::Mesh(MeshComponent {
            mesh: cube_mesh,
            material: Material::default(),
            visible: true,
            wireframe: false,
            solid: true,
            double_sided: false,
        }));
        scene.add_object(cube);

        let mut app = Self {
            scene,
            history: CommandHistory::new(100),
            asset_library: AssetLibrary::new(),
            camera_position: Vec3::new(5.0, 5.0, 10.0),
            camera_target: Vec3::ZERO,
            camera_up: Vec3::UP,
            camera_fov: 60.0,
            current_tool: EditorTool::Select,
            gizmo: Gizmo::default(),
            viewport_rect: Rect::NOTHING,
            show_hierarchy: true,
            show_inspector: true,
            show_console: true,
            show_new_scene_dialog: false,
            show_import_dialog: false,
            new_scene_name: String::from("New Scene"),
            search_filter: String::new(),
            last_mouse_pos: None,
            left_mouse_pressed: false,
            right_mouse_pressed: false,
            middle_mouse_pressed: false,
            status_message: String::from("Ready"),
            fps: 0.0,
            frame_count: 0,
            last_frame_time: 0.0,
            last_update_time: 0.0,
            console_messages: VecDeque::new(),
            world_streamer: WorldStreamer::new(),
            material_accel: MaterialAccelerator::new(),
            shader_manager: ShaderManager::new(),
            audio_system: SpatialAudioSystem::new(),
            scripting: ScriptingEngine::new(),
            cinematic: CinematicManager::new(),
            cpu_render_limit: 5000000,
            pending_imports: Vec::new(),
            import_progress: 0.0,
            gpu_renderer: None,
            gpu_mesh_map: HashMap::new(),
            gpu_material_map: HashMap::new(),
            wgpu_render_state,
            gpu_initialized: false,
            gpu_texture_id: None,
            gpu_texture_size: (1720, 768),

            upload_queue: VecDeque::new(),
            max_upload_bytes_per_frame: 4 * 1024 * 1024,

            gizmo_drag: None,
            gizmo_hover_axis: None,
            snap_enabled: false,
            snap_translate: 1.0,
            snap_rotate_deg: 15.0,
            snap_scale: 0.1,

            show_asset_browser: true,
            asset_browser_root: std::env::current_dir()
                .ok()
                .and_then(|d| d.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from(".")),
            asset_tree: None,

            edit_mode: false,
            edit_mesh_object: None,
            edit_mesh_select_mode: crate::editor::MeshSelectMode::Vertex,
            edit_selected_vertices: std::collections::BTreeSet::new(),
            edit_selected_faces: std::collections::BTreeSet::new(),
            mesh_gizmo_drag: None,
            mesh_gizmo_hover_axis: None,
            dragging_keyframe: None,
            mesh_gizmo_snap_candidates: Vec::new(),
            mesh_gizmo_snap_start_value: 0.0,
            mesh_gizmo_snap_applied_value: 0.0,
            mesh_gizmo_snap_raw_delta: 0.0,
            mesh_gizmo_snap_target: None,
            mesh_gizmo_snap_min_offset: 0.0,
            mesh_gizmo_snap_max_offset: 0.0,
            mesh_edit_undo_snapshot: None,
            mesh_edit_history_dirty: false,
            sound_bank_editor: SoundBankEditorState::default(),
            route_editor: RouteEditorState::default(),
            script_editor: ScriptEditorState::default(),
            assembly_editor: AssemblyEditorState::default(),
            car_preset_editor: CarPresetEditorState::default(),
            material_library_editor: MaterialLibraryEditorState::default(),
            discord_presence: crate::discord_presence::DiscordPresence::new(),
        };

        app.init_gpu();
        app.log("🚀 Editor started with GPU acceleration!", Color32::GREEN);
        app
    }

    pub fn log(&mut self, msg: &str, color: Color32) {
        self.console_messages.push_back((msg.to_string(), color));
        if self.console_messages.len() > 100 {
            self.console_messages.pop_front();
        }
        self.status_message = msg.to_string();
    }

    fn create_gpu_renderer(&self) -> Result<GpuRenderer, String> {
        let render_state = self.wgpu_render_state.as_ref()
            .ok_or("No wgpu render state".to_string())?;

        let format = render_state.target_format;
        let width = self.viewport_rect.width().max(1.0) as u32;
        let height = self.viewport_rect.height().max(1.0) as u32;

        // Клонируем device и queue (они не Copy, нужно явно clone)
        let device = render_state.device.clone();
        let queue = render_state.queue.clone();

        let renderer = GpuRenderer::with_device(
            device,
            queue,
            format,
            width,
            height,
            render_state.renderer.clone(),
        );

        Ok(renderer)
    }

    pub fn init_gpu(&mut self) {
        if self.gpu_initialized {
            return;
        }

        if self.wgpu_render_state.is_none() {
            self.log("⚠️ No wgpu render state - using CPU fallback", Color32::YELLOW);
            return;
        }

        self.log("🔧 GPU ready - will init on first frame", Color32::YELLOW);
        self.gpu_initialized = true;
    }

    // Обработка очереди загрузок (без удержания mutable borrow при логировании)
    fn process_upload_queue(&mut self, budget_bytes: usize) {
        if self.upload_queue.is_empty() || self.gpu_renderer.is_none() {
            return;
        }

        let mut remaining = budget_bytes;
        let mut uploaded_count = 0usize;
        let mut messages: Vec<String> = Vec::new();

        // получаем mutable borrow единственный раз, но НЕ вызываем self.log() внутри
        if let Some(renderer) = self.gpu_renderer.as_mut() {
            while remaining > 0 {
                if let Some(task) = self.upload_queue.front() {
                    if task.estimated_bytes > remaining && remaining < 1024 {
                        break;
                    }
                    let task = self.upload_queue.pop_front().unwrap();
                    remaining = remaining.saturating_sub(task.estimated_bytes);

                    let mesh_idx = renderer.add_mesh(&task.mesh);
                    let mat_idx = renderer.add_material(task.material.color, task.material.metallic, task.material.roughness);
                    self.gpu_mesh_map.insert(task.id, mesh_idx);
                    self.gpu_material_map.insert(task.id, mat_idx);

                    uploaded_count += 1;
                    messages.push(format!("Uploaded '{}' -> mesh_idx={}, mat_idx={}", task.name, mesh_idx, mat_idx));
                } else {
                    break;
                }
            }
        }

        if uploaded_count > 0 {
            for m in messages {
                // теперь безопасно логируем — borrow renderer уже отпущен
                self.log(&format!("⬆️ {}", m), Color32::from_rgb(180, 255, 180));
            }
            self.log(&format!("Uploaded {} objects this frame (budget {:.1} KB left)", uploaded_count, remaining as f32 / 1024.0), Color32::GREEN);
        }
    }

    fn render_gpu_viewport(&mut self, ui: &mut Ui, rect: Rect) {
        if self.gpu_renderer.is_none() && self.gpu_initialized {
            match self.create_gpu_renderer() {
                Ok(renderer) => {
                    for (&id, obj) in &self.scene.objects {
                        if let ObjectType::Mesh(ref m) = obj.object_type {
                            let task = UploadTask {
                                id,
                                name: obj.name.clone(),
                                mesh: m.mesh.clone(),
                                material: m.material.clone(),
                                estimated_bytes: m.mesh.vertices.len() * 36 + m.mesh.indices.len() * 4,
                            };
                            self.upload_queue.push_back(task);
                        }
                    }
                    self.gpu_renderer = Some(renderer);
                    self.log("✅ GPU renderer created!", Color32::GREEN);
                }
                Err(e) => {
                    self.log(&format!("❌ GPU init failed: {}", e), Color32::RED);
                    crate::ui::viewport::render_viewport(ui, self);
                    return;
                }
            }
        }

        self.process_upload_queue(self.max_upload_bytes_per_frame);

        let render_objects = self.get_gpu_render_objects();
        let particle_instances = self.get_gpu_particle_instances();

        if let Some(ref mut renderer) = self.gpu_renderer {
            renderer.camera.position = self.camera_position;
            renderer.camera.target = self.camera_target;
            renderer.camera.up = self.camera_up;
            renderer.camera.fov = self.camera_fov.to_radians();
            renderer.camera.aspect = rect.width() / rect.height();

            let width = rect.width() as u32;
            let height = rect.height() as u32;

            // Рендерим сцену в offscreen текстуру — зарегистрирована
            // напрямую в egui_wgpu (см. GpuRenderer::ensure_output_texture),
            // так что `get_egui_texture()` ниже сразу видит этот же кадр,
            // без промежуточного шага чтения обратно на CPU.
            renderer.render(&render_objects, &particle_instances, width, height);

            // Отображаем текстуру
            if let Some(tex_id) = renderer.get_egui_texture() {
                ui.painter().image(
                    tex_id,
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                // Первый кадр — показываем фон
                let bg = Color32::from_rgb(
                    (self.scene.ambient_color[0] * 255.0) as u8,
                    (self.scene.ambient_color[1] * 255.0) as u8,
                    (self.scene.ambient_color[2] * 255.0) as u8,
                );
                ui.painter().rect_filled(rect, 0.0, bg);
            }
        } else {
            crate::ui::viewport::render_viewport(ui, self);
            return;
        }

        // Оверлей
        for obj in self.scene.objects.values() {
            if !obj.visible { continue; }
            let world = self.scene.get_world_transform(obj.id);
            let selected = self.scene.selected_ids.contains(&obj.id);
            if matches!(obj.object_type, ObjectType::SpawnPoint) {
                self.draw_spawn_marker(ui, world.position, world.rotation.forward(), rect);
                continue;
            }
            if let Some(pos) = self.world_to_screen(world.position + Vec3::new(0.0, 1.0, 0.0), rect) {
                ui.painter().text(
                    pos, Align2::CENTER_CENTER, &obj.name,
                    FontId::proportional(10.0),
                    if selected { Color32::WHITE } else { Color32::LIGHT_GRAY },
                );
            }
        }

        self.draw_gizmo(ui, rect);
        self.draw_mesh_edit_overlay(ui, rect);
        self.draw_keyframe_markers(ui, rect);
    }

    fn get_gpu_render_objects(&self) -> Vec<(usize, [[f32; 4]; 4], usize)> {
        let mut objects = Vec::new();

        let forward = (self.camera_target - self.camera_position).normalize();

        for (&id, obj) in &self.scene.objects {
            if !obj.visible { continue; }

            if let ObjectType::Mesh(m) = &obj.object_type {
                if let Some(&mesh_idx) = self.gpu_mesh_map.get(&id) {
                    if let Some(&mat_idx) = self.gpu_material_map.get(&id) {
                        let transform = self.scene.get_world_transform(id);
                        let center = transform.position;
                        let to_obj = center - self.camera_position;

                        // ИСПРАВЛЕНО (баг: "большой импортированный OBJ-город
                        // не виден"): культ по одной только точке pivot
                        // (center) ложно прятал ЦЕЛИКОМ огромные меши, у
                        // которых pivot стоит не в центре геометрии, а
                        // где-нибудь с краю (типичная ситуация для
                        // импортированных City/архитектурных OBJ — origin
                        // часто в углу модели, а не в её центре масс). Pivot
                        // формально мог быть "за спиной" камеры (dot<=0), в
                        // то время как бОльшая часть самой геометрии — перед
                        // ней и должна рисоваться. Расширяем порог культа на
                        // радиус bounding-сферы меша в мировых единицах (с
                        // учётом scale) — объект теперь пропускается только
                        // если ВЕСЬ его bounding-объём целиком позади камеры.
                        let (bmin, bmax) = m.mesh.bounds;
                        let half_extent = (bmax - bmin) * 0.5;
                        let s = transform.scale;
                        let world_half_extent = Vec3::new(half_extent.x * s.x, half_extent.y * s.y, half_extent.z * s.z);
                        let radius = world_half_extent.length();

                        if to_obj.dot(forward) <= -radius {
                            continue;
                        }
                        let model_matrix = transform.to_matrix();
                        objects.push((mesh_idx, model_matrix, mat_idx));
                    }
                }
            }
        }

        objects
    }

    /// Живые частицы всех ParticleSystem-объектов сцены в мировых
    /// координатах, готовые для `GpuRenderer::render` — сама симуляция
    /// (эмиссия/движение/старение) уже сделана в `Scene::update`, здесь
    /// только плоский список для отрисовки.
    fn get_gpu_particle_instances(&self) -> Vec<crate::gpu::renderer::ParticleInstance> {
        let mut instances = Vec::new();
        for obj in self.scene.objects.values() {
            if !obj.visible { continue; }
            if let ObjectType::ParticleSystem(p) = &obj.object_type {
                for particle in &p.system.particles {
                    instances.push(crate::gpu::renderer::ParticleInstance {
                        position: [particle.position.x, particle.position.y, particle.position.z],
                        size: particle.size,
                        color: particle.color,
                    });
                }
            }
        }
        instances
    }

    pub fn import_model_async(&mut self, path: &str) {
        let path_owned = path.to_string();
        let (tx, rx) = mpsc::channel();
        let path_clone = path_owned.clone();

        let file_size = std::fs::metadata(&path_clone)
            .map(|m| m.len())
            .unwrap_or(0);
        let size_mb = file_size as f64 / (1024.0 * 1024.0);

        std::thread::spawn(move || {
            let mut lib = AssetLibrary::new();
            match lib.import_model(&path_clone) {
                Ok(names) => {
                    let mut meshes = Vec::new();
                    for name in &names {
                        if let Some(m) = lib.get_mesh(name) {
                            meshes.push((name.clone(), m.clone()));
                        }
                    }
                    let _ = tx.send(Ok(ImportResult {
                        mesh_names: names,
                        meshes,
                    }));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        self.pending_imports.push(PendingImport {
            path: path_owned,
            receiver: rx,
        });

        self.log(
            &format!("📥 Importing: {} ({:.1} MB)...", path, size_mb),
            Color32::YELLOW,
        );
    }

    fn check_pending_imports(&mut self) {
        if self.pending_imports.is_empty() {
            return;
        }

        let mut results = Vec::new();
        let mut completed_indices = Vec::new();

        for (i, imp) in self.pending_imports.iter().enumerate() {
            if let Ok(r) = imp.receiver.try_recv() {
                completed_indices.push(i);
                results.push((imp.path.clone(), r));
            }
        }

        for &i in completed_indices.iter().rev() {
            self.pending_imports.remove(i);
        }

        for (_path, result) in results {
            match result {
                Ok(ir) => {
                    let mut total_tris = 0;
                    for (name, mesh) in ir.meshes {
                        let tris = mesh.indices.len() / 3;
                        let verts = mesh.vertices.len();

                        if tris == 0 {
                            self.log(&format!("⚠️ Empty mesh: {}", name), Color32::YELLOW);
                        }

                        total_tris += tris;

                        self.asset_library.meshes.insert(name.clone(), mesh.clone());

                        let obj = GameObject::new(
                            &name,
                            ObjectType::Mesh(MeshComponent {
                                mesh: mesh.clone(),
                                material: Material {
                                    name: format!("{}_mat", name),
                                    color: [0.7, 0.7, 0.7, 1.0],
                                    ..Default::default()
                                },
                                visible: true,
                                wireframe: false,
                                solid: true,
                                double_sided: false,
                            }),
                        );

                        let id = obj.id;
                        self.scene.add_object(obj);

                        // Оценка байтов: 36 bytes per vertex (pos+normal+color) + 4 bytes per index
                        let bytes_est = verts * 36 + mesh.indices.len() * 4;
                        let task = UploadTask {
                            id,
                            name: name.clone(),
                            mesh: mesh.clone(),
                            material: Material {
                                name: format!("{}_mat", name),
                                color: [0.7, 0.7, 0.7, 1.0],
                                ..Default::default()
                            },
                            estimated_bytes: bytes_est.max(1024),
                        };
                        self.upload_queue.push_back(task);
                    }
                    self.log(
                        &format!("✅ Import complete: {}K tris in {} meshes (queued)", total_tris / 1000, self.upload_queue.len()),
                        Color32::GREEN,
                    );
                    self.show_import_dialog = false;
                }
                Err(e) => {
                    self.log(&format!("❌ Import error: {}", e), Color32::RED);
                }
            }
        }
    }

    pub fn orbit_camera(&mut self, dx: f32, dy: f32) {
        let dir = self.camera_position - self.camera_target;
        let r = dir.length();
        if r < 0.01 {
            return;
        }
        let mut ha = dir.z.atan2(dir.x);
        let mut va = (dir.y / r).asin();
        ha += -dx * 0.01;
        va = (va + -dy * 0.01).clamp(-1.4, 1.4);
        self.camera_position = self.camera_target
            + Vec3::new(va.cos() * ha.cos(), va.sin(), va.cos() * ha.sin()) * r;
    }

    pub fn pan_camera(&mut self, dx: f32, dy: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        // ИСПРАВЛЕНО (баг: "большой импортированный город не видно" —
        // сюда же относится): скорость панорамирования масштабировалась от
        // РАССТОЯНИЯ ДО МИРОВОГО НУЛЯ (camera_position.length()), а не от
        // текущего расстояния до цели (масштаба зума). У большой сцены
        // цель обычно далеко от (0,0,0) — если при этом камера ещё и
        // приближена к цели (маленький d), панорамирование всё равно
        // считалось "быстрым" (или наоборот "мучительно медленным", если
        // камера случайно оказалась близко к мировому нулю) независимо от
        // текущего масштаба, что делает облёт большого объекта
        // непредсказуемым. Теперь скорость зависит от того же расстояния
        // "камера-цель", что и у zoom_camera/orbit_camera ниже —
        // стандартное поведение (Blender/Maya): чем сильнее приближено,
        // тем медленнее двигается панорама в мировых единицах.
        let s = (self.camera_target - self.camera_position).length() * 0.002;
        let off = right * (-dx * s) + up * (dy * s);
        self.camera_position = self.camera_position + off;
        self.camera_target = self.camera_target + off;
    }

    pub fn zoom_camera(&mut self, delta: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let d = (self.camera_target - self.camera_position).length();
        // ИСПРАВЛЕНО (баг: "большой импортированный OBJ-город (он большой)
        // не виден"): верхняя граница зума была жёстко зашита в 50 единиц —
        // физически невозможно было отъехать от цели дальше 50м, что бы вы
        // ни делали колесом мыши. Для сцены крупнее ~100 единиц в поперечнике
        // (типичный масштаб "города") это означает, что весь объект целиком
        // не помещается в кадр ни при каком зуме — выглядит как "объект не
        // рисуется", хотя на самом деле он просто физически не может влезть
        // в область обзора при максимально доступном отдалении. Поднял
        // потолок на 4 порядка (до 500 000 — с большим запасом на реальные
        // сцены; локальная точность f32 на таких дистанциях от камеры уже
        // начинает деградировать, но это отдельная, гораздо менее острая
        // проблема, чем полная невозможность отъехать).
        let nd = (d * (1.0 - delta * 0.001)).clamp(0.15, 500_000.0);
        self.camera_position = self.camera_target - dir * nd;
    }

    // ИСПРАВЛЕНО (баг, найденный пользователем: "поставленный спавн и точка
    // света визуально уезжают при движении карты" — вторая, независимая от
    // async-readback причина): `camera_fov` — ВЕРТИКАЛЬНЫЙ угол обзора (та
    // же величина, что `gpu/camera.rs::calculate_projection_matrix` кладёт
    // в `f = 1/tan(fovY/2)` и применяет к Y напрямую, а к X — только через
    // ДЕЛЕНИЕ на `aspect`). Раньше здесь X домножался на `rect.width()`, а
    // Y — на `rect.height()`, то есть использовались РАЗНЫЕ множители для
    // двух осей одного и того же пинхол-преобразования. Алгебраически
    // корректная формула (после сокращения деления на `aspect = width/height`
    // с последующим умножением на `width/2`, как и делает GPU-проекция +
    // растеризатор) сокращается до ОДНОГО и того же `rect.height() * 0.5`
    // для ОБЕИХ осей — GPU-рендер всегда масштабирует именно по высоте,
    // ширина участвует только в отсечении по краям кадра, не в масштабе.
    // На квадратном вьюпорте (width == height) разницы не было видно, а на
    // обычном прямоугольном панели X систематически съезжал относительно
    // того, что реально рисует GPU — тем сильнее, чем дальше объект от
    // центра экрана и чем больше aspect отличается от 1.
    pub fn world_to_screen(&self, wp: Vec3, rect: Rect) -> Option<Pos2> {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let rel = wp - self.camera_position;
        let dist = rel.dot(dir);
        if dist <= 0.01 {
            return None;
        }
        let tf = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();
        let scale = 1.0 / (dist * tf);
        let x = rel.dot(right) * scale;
        let y = rel.dot(up) * scale;
        let c = rect.center();
        Some(Pos2::new(
            c.x + x * rect.height() * 0.5,
            c.y - y * rect.height() * 0.5,
        ))
    }

    /// Обратная операция к `world_to_screen` для перетаскивания точки
    /// мышью (см. `handle_keyframe_marker_input`): переводит дельту мыши в
    /// пикселях в мировую дельту на плоскости, перпендикулярной направлению
    /// камеры и проходящей через `at_world_pos` — та же проекционная
    /// математика, что у `world_to_screen`, только "в обратную сторону",
    /// так что курсор мыши весь драг остаётся ровно над точкой (в отличие
    /// от gizmo-осей здесь нет ограничения по одной оси — точка свободно
    /// скользит по экранной плоскости, как и ожидается от простого
    /// "потащить точку мышью").
    fn screen_delta_to_world(&self, mouse_delta: Vec2, at_world_pos: Vec3, rect: Rect) -> Vec3 {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let rel = at_world_pos - self.camera_position;
        let dist = rel.dot(dir).max(0.01);
        let tf = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();
        let half_h = rect.height() * 0.5;
        let dr = mouse_delta.x * dist * tf / half_h;
        let du = -mouse_delta.y * dist * tf / half_h;
        right * dr + up * du
    }

    // =====================================================================
    // ДОБАВЛЕНО (полноценный эдитор — экспорт/импорт родных форматов
    // движка): см. src/converters/{altex,alworld,alfar}.rs — там настоящие
    // alkash3d_rs::AltexFile/AlworldFile/AlfarFile, не самодельная
    // реализация. Диалоги выбора файла/папки — синхронные (rfd), тот же
    // стиль, что уже использует "Browse for Model..." в ui/dialogs.rs.
    // =====================================================================

    /// Добавляет меш-объект в текущую сцену И ставит его в очередь загрузки
    /// на GPU — общий путь для импорта .altex/.alworld, повторяет то, что
    /// `check_pending_imports()` уже делает для стороннего импорта моделей.
    pub fn add_mesh_object(&mut self, name: &str, mesh: Mesh, material: Material) -> Uuid {
        let bytes_est = (mesh.vertices.len() * 36 + mesh.indices.len() * 4).max(1024);
        let obj = GameObject::new(
            name,
            ObjectType::Mesh(MeshComponent {
                mesh: mesh.clone(),
                material: material.clone(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        );
        let id = obj.id;
        self.scene.add_object(obj);
        self.upload_queue.push_back(UploadTask {
            id,
            name: name.to_string(),
            mesh,
            material,
            estimated_bytes: bytes_est,
        });
        id
    }

    /// ДОБАВЛЕНО (GameObject-меню — как в Unity): раньше в сцену вообще
    /// НЕЛЬЗЯ было добавить Light/AudioSource/ScriptedEntity/Empty объект
    /// через UI (эти варианты `ObjectType` существовали только в коде) — а
    /// новые экспортёры .alfar/.alsnd/.alscript (см. converters/) читают
    /// именно такие объекты из сцены, то есть были практически недостижимы
    /// без ручного редактирования кода. Единая точка создания любого
    /// объекта сцены: если выделен РОВНО один объект, новый становится его
    /// ребёнком (та же семантика, что у Unity — "GameObject > Create Empty"
    /// с выделенным родителем создаёт дочерний объект), иначе — объектом
    /// верхнего уровня, размещённым в точке, куда сейчас смотрит камера
    /// (`camera_target`), чтобы новый объект сразу было видно во вьюпорте.
    pub fn spawn_object(&mut self, name: &str, object_type: ObjectType) -> Uuid {
        let mut obj = GameObject::new(name, object_type);

        let parent = if self.scene.selected_ids.len() == 1 {
            Some(self.scene.selected_ids[0])
        } else {
            None
        };
        obj.parent = parent;
        if parent.is_none() {
            obj.transform.position = self.camera_target;
        }

        let id = obj.id;
        if let ObjectType::Mesh(m) = &obj.object_type {
            let bytes_est = (m.mesh.vertices.len() * 36 + m.mesh.indices.len() * 4).max(1024);
            self.upload_queue.push_back(UploadTask {
                id,
                name: name.to_string(),
                mesh: m.mesh.clone(),
                material: m.material.clone(),
                estimated_bytes: bytes_est,
            });
        }

        self.scene.add_object(obj);
        self.scene.select(id, false);
        self.log(&format!("✅ Создан объект: {}", name), Color32::GREEN);
        id
    }

    pub fn create_primitive(&mut self, name: &str, mesh: Mesh) -> Uuid {
        self.spawn_object(
            name,
            ObjectType::Mesh(MeshComponent {
                mesh,
                material: Material::default(),
                visible: true,
                wireframe: false,
                solid: true,
                double_sided: false,
            }),
        )
    }

    pub fn create_light(&mut self, name: &str, light_type: LightType) -> Uuid {
        self.spawn_object(
            name,
            ObjectType::Light(LightComponent {
                light_type,
                color: [1.0, 0.95, 0.85],
                intensity: 2.0,
                range: 15.0,
                enabled: true,
            }),
        )
    }

    pub fn create_audio_source(&mut self) -> Uuid {
        self.spawn_object(
            "Audio Source",
            ObjectType::AudioSource(AudioSourceComponent {
                sound_name: String::new(),
                volume: 1.0,
                spatial_blend: 1.0,
                enabled: true,
            }),
        )
    }

    pub fn create_camera(&mut self) -> Uuid {
        self.spawn_object(
            "Camera",
            ObjectType::Camera(CameraComponent {
                fov: 60.0,
                near: 0.1,
                far: 1000.0,
                orthographic: false,
            }),
        )
    }

    pub fn create_particle_system(&mut self) -> Uuid {
        self.spawn_object(
            "Particle System",
            ObjectType::ParticleSystem(ParticleSystemComponent {
                system: crate::particle::ParticleSystem::new(),
                enabled: true,
            }),
        )
    }

    pub fn create_scripted_entity(&mut self) -> Uuid {
        self.spawn_object(
            "Scripted Entity",
            ObjectType::ScriptedEntity(ScriptedEntityComponent {
                script_name: String::new(),
                enabled: true,
            }),
        )
    }

    pub fn create_empty(&mut self) -> Uuid {
        self.spawn_object("Empty", ObjectType::Empty)
    }

    pub fn create_spawn_point(&mut self) -> Uuid {
        self.spawn_object("Spawn Point", ObjectType::SpawnPoint)
    }

    /// Заменяет всю сцену (используется импортом .alworld — "Open World",
    /// а не "Import", целиком меняет содержимое редактора) и переставляет
    /// все её меш-объекты в очередь GPU-загрузки. Старые GPU-буферы
    /// прежней сцены (`gpu_renderer.meshes`/`materials`) намеренно не
    /// освобождаются — `GpuRenderer` сейчас не умеет удалять отдельные
    /// записи (см. gpu/renderer.rs::add_mesh/add_material, только append) —
    /// они просто становятся недостижимыми через gpu_mesh_map/
    /// gpu_material_map до следующего перезапуска. Утечка GPU-памяти при
    /// повторных Open World в одной сессии — известное ограничение,
    /// отдельное от целей этой задачи (реальный экспорт/импорт форматов).
    pub fn replace_scene(&mut self, mut new_scene: Scene) {
        self.gpu_mesh_map.clear();
        self.gpu_material_map.clear();
        self.upload_queue.clear();

        let mut tasks = Vec::new();
        for (&id, obj) in &new_scene.objects {
            if let ObjectType::Mesh(m) = &obj.object_type {
                let bytes_est = (m.mesh.vertices.len() * 36 + m.mesh.indices.len() * 4).max(1024);
                tasks.push(UploadTask {
                    id,
                    name: obj.name.clone(),
                    mesh: m.mesh.clone(),
                    material: m.material.clone(),
                    estimated_bytes: bytes_est,
                });
            }
        }
        self.upload_queue.extend(tasks);

        new_scene.dirty = false;
        self.scene = new_scene;
        self.history = CommandHistory::new(100);
    }

    /// File > Export Selected to .altex...
    pub fn export_selected_to_altex(&mut self) {
        let Some(&id) = self.scene.selected_ids.first() else {
            self.log("⚠️ Выделите меш-объект для экспорта в .altex", Color32::YELLOW);
            return;
        };
        let Some(obj) = self.scene.get_object(id) else { return; };
        let ObjectType::Mesh(m) = &obj.object_type else {
            self.log("⚠️ Выбранный объект не является мешем", Color32::YELLOW);
            return;
        };
        let (mesh, material, name) = (m.mesh.clone(), m.material.clone(), obj.name.clone());

        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Model", &["altex"])
            .set_file_name(&format!("{}.altex", name))
            .save_file()
        {
            let path_str = path.to_string_lossy().to_string();
            match crate::converters::altex::export_mesh_to_altex(&mesh, &material, &name, &path_str) {
                Ok(()) => self.log(&format!("✅ Экспортировано в .altex: {}", path_str), Color32::GREEN),
                Err(e) => self.log(&format!("❌ Ошибка экспорта .altex: {}", e), Color32::RED),
            }
        }
    }

    /// File > Import .altex...
    pub fn import_altex_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Model", &["altex"])
            .pick_file()
        else { return; };
        self.import_altex_from_path(&path.to_string_lossy(), None);
    }

    /// Общее ядро импорта `.altex`, используемое и диалогом File > Import,
    /// и двойным кликом/drag-and-drop из браузера ассетов (см.
    /// `ui/asset_browser.rs`). `place_at` — мировая позиция для нового
    /// объекта (drag-and-drop передаёт точку сброса на плоскости земли);
    /// `None` оставляет позицию из самого `.altex` (обычно ноль).
    pub fn import_altex_from_path(&mut self, path_str: &str, place_at: Option<Vec3>) {
        match crate::converters::altex::import_altex(path_str) {
            Ok(meshes) => {
                let count = meshes.len();
                for (name, mesh, material) in meshes {
                    let id = self.add_mesh_object(&name, mesh, material);
                    if let Some(pos) = place_at {
                        if let Some(obj) = self.scene.get_object_mut(id) {
                            obj.transform.position = pos;
                        }
                    }
                }
                self.log(&format!("✅ Импортировано из .altex: {} меш(ей) из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .altex: {}", e), Color32::RED),
        }
    }

    /// File > Export Scene to .alworld...
    pub fn export_scene_to_alworld_dialog(&mut self) {
        let Some(dir) = rfd::FileDialog::new().pick_folder() else { return; };
        let dir_str = dir.to_string_lossy().to_string();

        match crate::converters::alworld::export_scene_to_alworld(&self.scene, &dir_str) {
            Ok(world_path) => self.log(&format!("✅ Мир экспортирован: {}", world_path), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alworld: {}", e), Color32::RED),
        }
    }

    /// File > Open World (.alworld)... — заменяет текущую сцену.
    pub fn import_alworld_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D World", &["alworld"])
            .pick_file()
        else { return; };
        self.import_alworld_from_path(&path.to_string_lossy());
    }

    /// Общее ядро — см. комментарий у `import_altex_from_path`.
    pub fn import_alworld_from_path(&mut self, path_str: &str) {
        let mut messages = Vec::new();
        let result = crate::converters::alworld::import_alworld_to_scene(path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(scene) => {
                let count = scene.objects.len();
                self.replace_scene(scene);
                self.log(&format!("✅ Мир загружен: {} объект(ов) из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка открытия .alworld: {}", e), Color32::RED),
        }
    }

    /// File > Export Lighting to .alfar...
    pub fn export_lighting_to_alfar_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Lighting", &["alfar"])
            .set_file_name("lighting.alfar")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alfar::export_scene_to_alfar_file(&self.scene, &path_str) {
            Ok(()) => self.log(&format!("✅ Освещение экспортировано: {}", path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alfar: {}", e), Color32::RED),
        }
    }

    /// File > Import Lighting (.alfar)... — добавляет источники света в
    /// ТЕКУЩУЮ сцену (не заменяет её, в отличие от Open World) и заменяет
    /// ambient-цвет сцены на значение из файла.
    pub fn import_alfar_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Lighting", &["alfar"])
            .pick_file()
        else { return; };
        self.import_alfar_from_path(&path.to_string_lossy());
    }

    /// Общее ядро — см. комментарий у `import_altex_from_path`.
    pub fn import_alfar_from_path(&mut self, path_str: &str) {
        let mut messages = Vec::new();
        let result = crate::converters::alfar::import_alfar_to_scene(path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(light_scene) => {
                let count = light_scene.objects.len();
                self.scene.ambient_color = light_scene.ambient_color;
                for (_, obj) in light_scene.objects {
                    self.scene.add_object(obj);
                }
                self.log(&format!("✅ Загружено источников света: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .alfar: {}", e), Color32::RED),
        }
    }

    /// File > Export > Material Library to .almat... — сохраняет ВСЮ
    /// `AssetLibrary::materials` (именованную библиотеку материалов, не
    /// материал текущего выделенного объекта — см. подробное объяснение в
    /// шапке converters/almat.rs про то, чем это отличается от материала,
    /// уже встроенного в `.altex`).
    pub fn export_materials_to_almat_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Materials", &["almat"])
            .set_file_name("materials.almat")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::almat::export_materials_to_almat_file(&self.asset_library.materials, &path_str) {
            Ok(()) => self.log(&format!("✅ Библиотека материалов экспортирована ({} шт.): {}", self.asset_library.materials.len(), path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .almat: {}", e), Color32::RED),
        }
    }

    /// File > Import Engine Format > .almat materials... — ДОБАВЛЯЕТ
    /// материалы в `AssetLibrary::materials` (совпадающие по имени —
    /// перезаписывает), не заменяет всю библиотеку целиком и не трогает
    /// текущую сцену.
    pub fn import_almat_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Materials", &["almat"])
            .pick_file()
        else { return; };
        self.import_almat_from_path(&path.to_string_lossy());
    }

    /// Общее ядро — см. комментарий у `import_altex_from_path`.
    pub fn import_almat_from_path(&mut self, path_str: &str) {
        let mut messages = Vec::new();
        let result = crate::converters::almat::import_almat_to_materials(path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(materials) => {
                let count = materials.len();
                self.asset_library.materials.extend(materials);
                self.log(&format!("✅ Загружено материалов: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .almat: {}", e), Color32::RED),
        }
    }

    /// File > Export > Lighting to .alfar... (см. выше для .altex/.alworld/.alfar)
    /// Эти три ниже — гейм-плей форматы (Tier 3): звук/машины/маршруты/
    /// скрипты/сборки. У сцены эдитора нет выделенных типов объектов для
    /// части из них (машина, маршрут, сборка) — см. комментарии в
    /// соответствующих converters/al*.rs про то, как именно они собираются
    /// из того, что в сцене реально есть (выделение/AudioSource/
    /// ScriptedEntity).

    /// File > Export > Sounds to .alsnd...
    pub fn export_sounds_to_alsnd_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Sound Bank", &["alsnd"])
            .set_file_name("scene.alsnd")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alsnd::export_scene_to_alsnd_file(&self.scene, &path_str) {
            Ok(count) => self.log(&format!("✅ Экспортировано звуков: {} -> {}", count, path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alsnd: {}", e), Color32::RED),
        }
    }

    /// File > Export > Car Preset (.alcar)
    pub fn export_car_preset_dialog(&mut self, preset: crate::converters::alcar::CarPreset) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Car", &["alcar"])
            .set_file_name("car.alcar")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alcar::export_car_preset(preset, "", &path_str) {
            Ok(()) => self.log(&format!("✅ Экспортирован пресет машины '{}': {}", preset.label(), path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alcar: {}", e), Color32::RED),
        }
    }

    /// File > Export > Selected as Route (.alroute) — открытый маршрут (loop_type=0)
    pub fn export_selection_to_alroute_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Route", &["alroute"])
            .set_file_name("route.alroute")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alroute::export_selection_to_alroute(&self.scene, "Route", 0, &path_str) {
            Ok(count) => self.log(&format!("✅ Экспортирован маршрут из {} точек: {}", count, path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alroute: {}", e), Color32::RED),
        }
    }

    /// File > Export > Scripts to .alscript...
    pub fn export_scripts_to_alscript_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Scripts", &["alscript"])
            .set_file_name("scripts.alscript")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        let mut messages = Vec::new();
        let result = crate::converters::alscript::export_scene_to_alscript_file(&self.scene, &path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(count) => self.log(&format!("✅ Экспортировано скриптов: {} -> {}", count, path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alscript: {}", e), Color32::RED),
        }
    }

    /// File > Export > Selected as Assembly (.alasm)
    pub fn export_selected_to_alasm_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Assembly", &["alasm"])
            .set_file_name("assembly.alasm")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alasm::export_selected_to_alasm(&self.scene, alkash3d_rs::AssemblyCategory::Generic, &path_str) {
            Ok(()) => self.log(&format!("✅ Экспортировано в .alasm (+ .altex геометрия рядом): {}", path_str), Color32::GREEN),
            Err(e) => self.log(&format!("❌ Ошибка экспорта .alasm: {}", e), Color32::RED),
        }
    }

    // ДОБАВЛЕНО (по прямому запросу пользователя — "по порядку сначала
    // форматы": у Tier-3 форматов раньше был только Export, без Import —
    // асимметрия с .altex/.alworld/.alfar/.almat выше). Все пять ниже
    // ДОБАВЛЯЮТ объекты в ТЕКУЩУЮ сцену (как .alfar), а не заменяют её
    // целиком (как .alworld) — общее ядро возвращает `Scene`-контейнер,
    // объекты которого просто переносятся в `self.scene.add_object`, тот
    // же паттерн, что и `import_alfar_from_path` выше.

    /// File > Import > Sounds (.alsnd)... — добавляет AudioSource-объекты.
    pub fn import_alsnd_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Sound Bank", &["alsnd"])
            .pick_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        let mut messages = Vec::new();
        let result = crate::converters::alsnd::import_alsnd_to_scene(&path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(sound_scene) => {
                let count = sound_scene.objects.len();
                for (_, obj) in sound_scene.objects {
                    self.scene.add_object(obj);
                }
                self.log(&format!("✅ Загружено звуков: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .alsnd: {}", e), Color32::RED),
        }
    }

    /// Assets > Sound Bank Editor... — открывает (или переоткрывает поверх)
    /// панель редактора, см. `ui/sound_bank_editor.rs`.
    pub fn open_sound_bank_editor(&mut self) {
        self.sound_bank_editor.open = true;
    }

    /// "📂 Загрузить .alsnd..." в самом редакторе — В ОТЛИЧИЕ от
    /// `import_alsnd_dialog` выше (которая добавляет AudioSource-объекты в
    /// 3D-сцену), заменяет содержимое панели редактора напрямую, без
    /// прохода через `Scene` — см. `converters::alsnd::load_sound_bank`.
    pub fn load_sound_bank_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Sound Bank", &["alsnd"])
            .pick_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alsnd::load_sound_bank(&path_str) {
            Ok((bank_name, entries)) => {
                let count = entries.len();
                self.sound_bank_editor.bank_name = if bank_name.is_empty() { "SoundBank".to_string() } else { bank_name };
                self.sound_bank_editor.entries = entries;
                self.sound_bank_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Загружено в редактор звуков: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка загрузки .alsnd: {}", e), Color32::RED),
        }
    }

    /// "💾 Сохранить как .alsnd..." в самом редакторе.
    pub fn save_sound_bank_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Sound Bank", &["alsnd"])
            .set_file_name("sounds.alsnd")
            .save_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        match crate::converters::alsnd::save_sound_bank(&self.sound_bank_editor.bank_name, &self.sound_bank_editor.entries, &path_str) {
            Ok(count) => {
                self.sound_bank_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Сохранено звуков: {} -> {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка сохранения .alsnd: {}", e), Color32::RED),
        }
    }

    // ========================= Route Editor (.alroute) =========================

    pub fn open_route_editor(&mut self) {
        self.route_editor.open = true;
    }

    pub fn load_route_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Route", &["alroute"]).pick_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alroute::load_routes(&path_str) {
            Ok(routes) => {
                let count = routes.len();
                self.route_editor.routes = routes;
                self.route_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Загружено маршрутов в редактор: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка загрузки .alroute: {}", e), Color32::RED),
        }
    }

    pub fn save_route_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Route", &["alroute"]).set_file_name("routes.alroute").save_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alroute::save_routes(&self.route_editor.routes, &path_str) {
            Ok(count) => {
                self.route_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Сохранено маршрутов: {} -> {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка сохранения .alroute: {}", e), Color32::RED),
        }
    }

    // ========================= Script Registry Editor (.alscript) =========================

    pub fn open_script_editor(&mut self) {
        self.script_editor.open = true;
    }

    pub fn load_script_registry_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Scripts", &["alscript"]).pick_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        let mut messages = Vec::new();
        match crate::converters::alscript::load_scripts(&path_str, &mut |m| messages.push(m)) {
            Ok(entries) => {
                let count = entries.len();
                self.script_editor.entries = entries;
                self.script_editor.loaded_path = Some(path_str.clone());
                for m in messages { self.log(&m, Color32::YELLOW); }
                self.log(&format!("✅ Загружено скриптов в редактор: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка загрузки .alscript: {}", e), Color32::RED),
        }
    }

    pub fn save_script_registry_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Scripts", &["alscript"]).set_file_name("scripts.alscript").save_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alscript::save_scripts(&self.script_editor.entries, &path_str) {
            Ok(count) => {
                self.script_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Сохранено скриптов: {} -> {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка сохранения .alscript: {}", e), Color32::RED),
        }
    }

    // ========================= Assembly Editor (.alasm) =========================

    pub fn open_assembly_editor(&mut self) {
        self.assembly_editor.open = true;
    }

    pub fn load_assembly_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Assembly", &["alasm"]).pick_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alasm::load_assembly(&path_str) {
            Ok((name, category, parts)) => {
                let count = parts.len();
                self.assembly_editor.name = if name.is_empty() { "Assembly".to_string() } else { name };
                self.assembly_editor.category = category;
                self.assembly_editor.parts = parts;
                self.assembly_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Загружено деталей в редактор: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка загрузки .alasm: {}", e), Color32::RED),
        }
    }

    pub fn save_assembly_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Assembly", &["alasm"]).set_file_name("assembly.alasm").save_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alasm::save_assembly(&self.assembly_editor.name, self.assembly_editor.category, &self.assembly_editor.parts, &path_str) {
            Ok(count) => {
                self.assembly_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Сохранено деталей: {} -> {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка сохранения .alasm: {}", e), Color32::RED),
        }
    }

    // ========================= Car Preset Editor (.alcar) =========================

    pub fn open_car_preset_editor(&mut self) {
        self.car_preset_editor.open = true;
    }

    pub fn load_car_preset_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Car", &["alcar"]).pick_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alcar::load_car_preset(&path_str) {
            Ok(edit) => {
                self.car_preset_editor.edit = edit;
                self.car_preset_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Загружен пресет машины: {}", path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка загрузки .alcar: {}", e), Color32::RED),
        }
    }

    pub fn save_car_preset_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("AlKAsH3D Car", &["alcar"]).set_file_name("car.alcar").save_file() else { return; };
        let path_str = path.to_string_lossy().to_string();
        match crate::converters::alcar::save_car_preset(&self.car_preset_editor.edit, &path_str) {
            Ok(()) => {
                self.car_preset_editor.loaded_path = Some(path_str.clone());
                self.log(&format!("✅ Сохранён пресет машины: {}", path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка сохранения .alcar: {}", e), Color32::RED),
        }
    }

    // ========================= Material Library Editor (.almat) =========================
    // Load/Save намеренно переиспользуют уже существующие
    // `import_almat_dialog`/`export_materials_to_almat_dialog` (см. выше) —
    // редактор работает прямо по `AssetLibrary::materials`, отдельная
    // пара диалогов ему не нужна.

    pub fn open_material_library_editor(&mut self) {
        self.material_library_editor.open = true;
    }

    /// File > Import > Scripts (.alscript)... — добавляет ScriptedEntity-объекты.
    pub fn import_alscript_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Scripts", &["alscript"])
            .pick_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        let mut messages = Vec::new();
        let result = crate::converters::alscript::import_alscript_to_scene(&path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(script_scene) => {
                let count = script_scene.objects.len();
                for (_, obj) in script_scene.objects {
                    self.scene.add_object(obj);
                }
                self.log(&format!("✅ Загружено скриптов: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .alscript: {}", e), Color32::RED),
        }
    }

    /// File > Import > Route (.alroute)... — добавляет корень-маршрут +
    /// цепочку дочерних Empty-точек на каждую точку маршрута.
    pub fn import_alroute_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Route", &["alroute"])
            .pick_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        let mut messages = Vec::new();
        let result = crate::converters::alroute::import_alroute_to_scene(&path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(route_scene) => {
                let count = route_scene.objects.len();
                for (_, obj) in route_scene.objects {
                    self.scene.add_object(obj);
                }
                self.log(&format!("✅ Загружено объектов маршрута: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .alroute: {}", e), Color32::RED),
        }
    }

    /// File > Import > Assembly (.alasm)... — добавляет иерархию деталей
    /// (Mesh/куб-заглушка + parent/child).
    pub fn import_alasm_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Assembly", &["alasm"])
            .pick_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        let mut messages = Vec::new();
        let result = crate::converters::alasm::import_alasm_to_scene(&path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(asm_scene) => {
                let count = asm_scene.objects.len();
                for (_, obj) in asm_scene.objects {
                    self.scene.add_object(obj);
                }
                self.log(&format!("✅ Загружено деталей сборки: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .alasm: {}", e), Color32::RED),
        }
    }

    /// File > Import > Car Preset (.alcar)... — добавляет корень-машину +
    /// кузов (если резолвится) + фары/стопы/поворотники как Light-объекты.
    pub fn import_alcar_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Car", &["alcar"])
            .pick_file()
        else { return; };
        let path_str = path.to_string_lossy().to_string();

        let mut messages = Vec::new();
        let result = crate::converters::alcar::import_alcar_to_scene(&path_str, &mut |m| messages.push(m));
        for m in messages {
            self.log(&m, Color32::YELLOW);
        }
        match result {
            Ok(car_scene) => {
                let count = car_scene.objects.len();
                for (_, obj) in car_scene.objects {
                    self.scene.add_object(obj);
                }
                self.log(&format!("✅ Загружено объектов машины: {} из {}", count, path_str), Color32::GREEN);
            }
            Err(e) => self.log(&format!("❌ Ошибка импорта .alcar: {}", e), Color32::RED),
        }
    }

    // =====================================================================
    // ДОБАВЛЕНО (реальный интерактивный gizmo во вьюпорте): `editor::Gizmo`
    // (gizmo.rs) существовал только как структура данных — ни `current_tool`
    // (кнопки тулбара Move/Rotate/Scale), ни `Gizmo::drag()`/`begin_drag()`
    // нигде не вызывались, подвинуть объект в 3D можно было только вводом
    // чисел в инспекторе. Ниже — экранно-проекционная реализация: три оси
    // рисуются как отрезки от центра выделения (среднее мировых позиций
    // всех выделенных — тот же пивот, что и у мультиредактирования в
    // инспекторе) наружу вдоль X/Y/Z, наведение/перетаскивание определяется
    // расстоянием от курсора до 2D-отрезка на экране (`world_to_screen`).
    //
    // Перетаскивание работает через ПРОЕКЦИЮ дельты мыши на экранное
    // направление оси (скаляр пикселей "вдоль" оси) — для Move это даёт
    // мировое смещение вдоль оси (пиксели * мировых-единиц-на-пиксель,
    // посчитанных из текущей длины хэндла на экране); для Rotate/Scale тот
    // же скаляр интерпретируется как угол/приращение масштаба с
    // фиксированной чувствительностью. ВАЖНО: Rotate здесь — это линейное
    // перетаскивание вдоль оси, а не классическое кольцо с угловым
    // прослеживанием курсора вокруг центра — сильно проще в реализации и
    // достаточно для точного поворота по одной оси через клавиатурный ввод
    // числа шага (Drag speed), но визуально не "крутится под курсором" так,
    // как в Unity/Blender. Честный компромисс, а не незаконченная попытка
    // сделать полноценное угловое кольцо.
    fn gizmo_pivot(&self, ids: &[Uuid]) -> Vec3 {
        if ids.is_empty() {
            return Vec3::ZERO;
        }
        let mut sum = Vec3::ZERO;
        for &id in ids {
            sum = sum + self.scene.get_world_transform(id).position;
        }
        sum * (1.0 / ids.len() as f32)
    }

    fn gizmo_handle_length(&self, pivot: Vec3) -> f32 {
        ((pivot - self.camera_position).length() * 0.2).max(0.5)
    }

    /// Возвращает true, если клик/драг этого кадра был "поглощён" gizmo
    /// (используется наведением или начатым/продолжающимся перетаскиванием)
    /// — тогда `handle_viewport_input` не должен обрабатывать тот же клик
    /// как попытку выделить объект под курсором.
    fn handle_gizmo_input(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        if self.current_tool == crate::editor::EditorTool::Select {
            self.gizmo_hover_axis = None;
            self.gizmo_drag = None;
            return false;
        }
        let ids = self.scene.selected_ids.clone();
        if ids.is_empty() {
            self.gizmo_hover_axis = None;
            self.gizmo_drag = None;
            return false;
        }

        let pivot = self.gizmo_pivot(&ids);
        let Some(origin_screen) = self.world_to_screen(pivot, rect) else {
            return false;
        };
        let handle_len = self.gizmo_handle_length(pivot);

        use crate::editor::GizmoAxisSel;
        let mut axis_screens: Vec<(GizmoAxisSel, Pos2)> = Vec::new();
        for axis in [GizmoAxisSel::X, GizmoAxisSel::Y, GizmoAxisSel::Z] {
            if let Some(tip) = self.world_to_screen(pivot + axis.world_dir() * handle_len, rect) {
                axis_screens.push((axis, tip));
            }
        }

        // Продолжение уже начатого перетаскивания — приоритетнее наведения.
        if let Some(drag) = self.gizmo_drag.clone() {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    let mouse_delta = p - drag.last_mouse;
                    if mouse_delta.length_sq() > 0.0 {
                        self.apply_gizmo_delta(&ids, drag.axis, drag.tool, mouse_delta, rect);
                    }
                    if let Some(d) = self.gizmo_drag.as_mut() {
                        d.last_mouse = p;
                    }
                }
                return true;
            } else {
                if self.snap_enabled {
                    self.apply_gizmo_snap(&ids, drag.axis, drag.tool);
                }
                self.gizmo_drag = None;
                return true;
            }
        }

        // Наведение + возможное начало перетаскивания.
        self.gizmo_hover_axis = None;
        let Some(p) = ui.input(|i| i.pointer.hover_pos()) else { return false; };
        if !rect.contains(p) {
            return false;
        }

        let mut best: Option<(GizmoAxisSel, f32)> = None;
        for (axis, tip) in &axis_screens {
            let d = distance_point_to_segment(p, origin_screen, *tip);
            if d < 10.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((*axis, d));
            }
        }
        self.gizmo_hover_axis = best.map(|(a, _)| a);

        if let Some(axis) = self.gizmo_hover_axis {
            if ui.input(|i| i.pointer.primary_pressed()) {
                self.gizmo_drag = Some(crate::editor::GizmoDrag {
                    axis,
                    tool: self.current_tool,
                    last_mouse: p,
                });
                return true;
            }
            return true; // наведение тоже "поглощает" клик, чтобы не мигало выделение под курсором
        }

        false
    }

    fn apply_gizmo_delta(
        &mut self,
        ids: &[Uuid],
        axis: crate::editor::GizmoAxisSel,
        tool: EditorTool,
        mouse_delta: Vec2,
        rect: Rect,
    ) {
        let pivot = self.gizmo_pivot(ids);
        let Some(origin_screen) = self.world_to_screen(pivot, rect) else { return; };
        let handle_len = self.gizmo_handle_length(pivot);
        let Some(tip_screen) = self.world_to_screen(pivot + axis.world_dir() * handle_len, rect) else { return; };

        let axis_screen_vec = tip_screen - origin_screen;
        let screen_len = axis_screen_vec.length();
        if screen_len < 0.5 {
            return;
        }
        let axis_screen_dir = axis_screen_vec / screen_len;
        let pixels_along_axis = mouse_delta.x * axis_screen_dir.x + mouse_delta.y * axis_screen_dir.y;
        if pixels_along_axis == 0.0 {
            return;
        }

        match tool {
            EditorTool::Move => {
                let units_per_pixel = handle_len / screen_len;
                let world_delta = axis.world_dir() * (pixels_along_axis * units_per_pixel);
                for &id in ids {
                    if let Some(obj) = self.scene.get_object_mut(id) {
                        obj.transform.position = obj.transform.position + world_delta;
                    }
                }
            }
            EditorTool::Scale => {
                let scale_delta = pixels_along_axis * 0.01;
                for &id in ids {
                    if let Some(obj) = self.scene.get_object_mut(id) {
                        let s = &mut obj.transform.scale;
                        match axis {
                            crate::editor::GizmoAxisSel::X => s.x = (s.x + scale_delta).max(0.01),
                            crate::editor::GizmoAxisSel::Y => s.y = (s.y + scale_delta).max(0.01),
                            crate::editor::GizmoAxisSel::Z => s.z = (s.z + scale_delta).max(0.01),
                        }
                    }
                }
            }
            EditorTool::Rotate => {
                let angle = pixels_along_axis * 0.01;
                let delta_rot = crate::math::Quat::from_axis_angle(axis.world_dir(), angle);
                for &id in ids {
                    if let Some(obj) = self.scene.get_object_mut(id) {
                        obj.transform.rotation = obj.transform.rotation.mul(&delta_rot);
                    }
                }
            }
            EditorTool::Select => {}
        }
    }

    /// Применяется один раз, на отпускании кнопки мыши — округляет
    /// изменённую этим перетаскиванием компоненту (для Move/Scale — ось
    /// axis; для Rotate — соответствующий угол Эйлера) до ближайшего кратного
    /// шага привязки (`snap_translate`/`snap_rotate_deg`/`snap_scale`).
    fn apply_gizmo_snap(&mut self, ids: &[Uuid], axis: crate::editor::GizmoAxisSel, tool: EditorTool) {
        use crate::editor::GizmoAxisSel;
        let step = match tool {
            EditorTool::Move => self.snap_translate,
            EditorTool::Scale => self.snap_scale,
            EditorTool::Rotate => self.snap_rotate_deg.to_radians(),
            EditorTool::Select => return,
        };
        if step <= 0.0 {
            return;
        }
        for &id in ids {
            let Some(obj) = self.scene.get_object_mut(id) else { continue; };
            match tool {
                EditorTool::Move => {
                    let p = &mut obj.transform.position;
                    match axis {
                        GizmoAxisSel::X => p.x = (p.x / step).round() * step,
                        GizmoAxisSel::Y => p.y = (p.y / step).round() * step,
                        GizmoAxisSel::Z => p.z = (p.z / step).round() * step,
                    }
                }
                EditorTool::Scale => {
                    let s = &mut obj.transform.scale;
                    match axis {
                        GizmoAxisSel::X => s.x = ((s.x / step).round() * step).max(0.01),
                        GizmoAxisSel::Y => s.y = ((s.y / step).round() * step).max(0.01),
                        GizmoAxisSel::Z => s.z = ((s.z / step).round() * step).max(0.01),
                    }
                }
                EditorTool::Rotate => {
                    let mut euler = obj.transform.rotation.to_euler();
                    match axis {
                        GizmoAxisSel::X => euler.x = (euler.x / step).round() * step,
                        GizmoAxisSel::Y => euler.y = (euler.y / step).round() * step,
                        GizmoAxisSel::Z => euler.z = (euler.z / step).round() * step,
                    }
                    obj.transform.rotation = crate::math::Quat::from_euler(euler.x, euler.y, euler.z);
                }
                EditorTool::Select => {}
            }
        }
    }

    // =====================================================================
    // ДОБАВЛЕНО (редактор вершин/граней — по прямому запросу пользователя:
    // "сделай возможность редактировать фигуры, делать новые"): Tab —
    // вход/выход из Edit Mode для одного выделенного Mesh-объекта; 1/2 —
    // режим выделения (вершины/грани); клик — выделить (Shift — добавить/
    // снять); существующий gizmo НЕ переиспользуется напрямую (см.
    // комментарий у mesh_gizmo_drag в определении EditorApp) — здесь
    // параллельный, но геометрически идентичный путь, привязанный к
    // индексам вершин меша вместо Uuid объектов сцены. Чистая геометрия
    // (move/extrude/delete) — в editor/mesh_edit.rs, тут только ввод/GPU.
    // =====================================================================

    pub fn toggle_edit_mode(&mut self) {
        if self.edit_mode {
            // Если Tab нажали прямо посреди перетаскивания — не терять его
            // из истории отмены.
            self.mesh_edit_commit_history();
            self.edit_mode = false;
            self.edit_mesh_object = None;
            self.edit_selected_vertices.clear();
            self.edit_selected_faces.clear();
            self.mesh_gizmo_drag = None;
            self.mesh_gizmo_hover_axis = None;
            self.mesh_gizmo_snap_target = None;
            self.mesh_gizmo_snap_candidates.clear();
            self.log("Edit Mode: выключен", Color32::GRAY);
            return;
        }

        if self.scene.selected_ids.len() != 1 {
            self.log("⚠️ Для Edit Mode выдели РОВНО один Mesh-объект, потом Tab", Color32::YELLOW);
            return;
        }
        let id = self.scene.selected_ids[0];
        let Some(obj) = self.scene.get_object(id) else { return; };
        if !matches!(obj.object_type, ObjectType::Mesh(_)) {
            self.log("⚠️ Edit Mode доступен только для Mesh-объектов", Color32::YELLOW);
            return;
        }

        self.edit_mode = true;
        self.edit_mesh_object = Some(id);
        self.edit_selected_vertices.clear();
        self.edit_selected_faces.clear();
        self.log("✏️ Edit Mode: 1=вершины 2=грани, клик=выделить, Shift+клик=добавить, E=экструзия (грани), Delete=удалить, Tab=выход", Color32::GREEN);
    }

    /// "Действующее" выделение вершин для gizmo/перемещения: в режиме Face
    /// это объединение вершин всех выбранных граней (двигать грань = двигать
    /// её вершины), в режиме Vertex — само `edit_selected_vertices`.
    fn effective_selected_vertices(&self) -> std::collections::BTreeSet<usize> {
        match self.edit_mesh_select_mode {
            crate::editor::MeshSelectMode::Vertex => self.edit_selected_vertices.clone(),
            crate::editor::MeshSelectMode::Face => {
                let Some(id) = self.edit_mesh_object else { return Default::default(); };
                let Some(obj) = self.scene.get_object(id) else { return Default::default(); };
                let ObjectType::Mesh(m) = &obj.object_type else { return Default::default(); };
                crate::editor::mesh_edit::vertices_of_faces(&m.mesh, &self.edit_selected_faces)
            }
        }
    }

    /// Мировой центроид текущего выделения — pivot mesh-gizmo. `None`, если
    /// нечего двигать (нет активного Edit Mode или пустое выделение).
    fn mesh_gizmo_pivot(&self) -> Option<Vec3> {
        let id = self.edit_mesh_object?;
        let world_transform = self.scene.get_world_transform(id);
        let obj = self.scene.get_object(id)?;
        let ObjectType::Mesh(m) = &obj.object_type else { return None; };
        let verts = self.effective_selected_vertices();
        if verts.is_empty() {
            return None;
        }
        let mut sum = Vec3::ZERO;
        let mut count = 0u32;
        for &i in &verts {
            if let Some(&v) = m.mesh.vertices.get(i) {
                sum = sum + world_transform.transform_point(v);
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        Some(sum * (1.0 / count as f32))
    }

    fn axis_component(v: Vec3, axis: crate::editor::GizmoAxisSel) -> f32 {
        use crate::editor::GizmoAxisSel;
        match axis {
            GizmoAxisSel::X => v.x,
            GizmoAxisSel::Y => v.y,
            GizmoAxisSel::Z => v.z,
        }
    }

    /// Кандидаты для примагничивания драга по оси `axis` — мировые
    /// координаты (по этой оси) всех вершин редактируемого меша, КРОМЕ
    /// текущего выделения (притягиваться к самому себе бессмысленно).
    /// Пересчитывается один раз в момент начала каждого перетаскивания — см.
    /// комментарий у `mesh_gizmo_snap_candidates` в определении EditorApp.
    fn compute_snap_candidates(&self, axis: crate::editor::GizmoAxisSel) -> Vec<f32> {
        let Some(id) = self.edit_mesh_object else { return Vec::new(); };
        let world_transform = self.scene.get_world_transform(id);
        let Some(obj) = self.scene.get_object(id) else { return Vec::new(); };
        let ObjectType::Mesh(m) = &obj.object_type else { return Vec::new(); };
        let selected = self.effective_selected_vertices();

        let mut out = Vec::with_capacity(m.mesh.vertices.len());
        for (i, &v) in m.mesh.vertices.iter().enumerate() {
            if selected.contains(&i) {
                continue;
            }
            let world_v = world_transform.transform_point(v);
            out.push(Self::axis_component(world_v, axis));
        }
        out
    }

    /// Мин./макс. мировая координата (по оси `axis`) вершин текущего
    /// действующего выделения — протяжённость выделения по этой оси. `None`,
    /// если выделять нечего. См. комментарий у `mesh_gizmo_snap_min_offset`.
    fn selected_axis_extent(&self, axis: crate::editor::GizmoAxisSel) -> Option<(f32, f32)> {
        let id = self.edit_mesh_object?;
        let world_transform = self.scene.get_world_transform(id);
        let obj = self.scene.get_object(id)?;
        let ObjectType::Mesh(m) = &obj.object_type else { return None; };
        let verts = self.effective_selected_vertices();
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for &i in &verts {
            if let Some(&v) = m.mesh.vertices.get(i) {
                let c = Self::axis_component(world_transform.transform_point(v), axis);
                min = min.min(c);
                max = max.max(c);
            }
        }
        if min.is_finite() && max.is_finite() {
            Some((min, max))
        } else {
            None
        }
    }

    /// Аналог `handle_gizmo_input`, но для Edit Mode — работает поверх
    /// `edit_selected_vertices`/`edit_selected_faces` вместо
    /// `scene.selected_ids`, gizmo всегда ведёт себя как Move (см.
    /// комментарий у `mesh_gizmo_drag`). Возвращает true, если клик этого
    /// кадра "поглощён" (наведение/начатое перетаскивание/клик-выделение),
    /// чтобы `handle_viewport_input` не пытался параллельно вращать камеру
    /// тем же кликом.
    fn handle_mesh_edit_input(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        if !self.edit_mode {
            self.mesh_gizmo_drag = None;
            self.mesh_gizmo_hover_axis = None;
            return false;
        }
        let Some(id) = self.edit_mesh_object else { return false; };
        if self.scene.get_object(id).is_none() {
            // Объект пропал (удалён/отменено через undo) — не застреваем в
            // Edit Mode над несуществующим объектом.
            self.toggle_edit_mode();
            return false;
        }

        // Продолжение уже начатого перетаскивания — приоритетнее наведения/клика.
        if let Some(drag) = self.mesh_gizmo_drag.clone() {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    let mouse_delta = p - drag.last_mouse;
                    if mouse_delta.length_sq() > 0.0 {
                        self.apply_mesh_gizmo_delta(drag.axis, mouse_delta, rect);
                    }
                    if let Some(d) = self.mesh_gizmo_drag.as_mut() {
                        d.last_mouse = p;
                    }
                }
                return true;
            } else {
                self.mesh_gizmo_drag = None;
                self.mesh_gizmo_snap_target = None;
                self.mesh_gizmo_snap_candidates.clear();
                self.mesh_edit_commit_history();
                return true;
            }
        }

        // Наведение + возможное начало перетаскивания на хэндле gizmo.
        self.mesh_gizmo_hover_axis = None;
        if let Some(pivot) = self.mesh_gizmo_pivot() {
            if let Some(origin_screen) = self.world_to_screen(pivot, rect) {
                let handle_len = self.gizmo_handle_length(pivot);
                use crate::editor::GizmoAxisSel;
                let mut axis_screens: Vec<(GizmoAxisSel, Pos2)> = Vec::new();
                for axis in [GizmoAxisSel::X, GizmoAxisSel::Y, GizmoAxisSel::Z] {
                    if let Some(tip) = self.world_to_screen(pivot + axis.world_dir() * handle_len, rect) {
                        axis_screens.push((axis, tip));
                    }
                }

                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    if rect.contains(p) {
                        let mut best: Option<(GizmoAxisSel, f32)> = None;
                        for (axis, tip) in &axis_screens {
                            let d = distance_point_to_segment(p, origin_screen, *tip);
                            if d < 10.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                                best = Some((*axis, d));
                            }
                        }
                        self.mesh_gizmo_hover_axis = best.map(|(a, _)| a);

                        if let Some(axis) = self.mesh_gizmo_hover_axis {
                            if ui.input(|i| i.pointer.primary_pressed()) {
                                self.mesh_gizmo_drag = Some(crate::editor::GizmoDrag {
                                    axis,
                                    tool: EditorTool::Move,
                                    last_mouse: p,
                                });
                                self.mesh_edit_snapshot_before();
                                // ДОБАВЛЕНО (примагничивание к другим вершинам
                                // меша — см. комментарий у `mesh_gizmo_snap_candidates`
                                // в определении EditorApp): список кандидатов и
                                // точка отсчёта фиксируются один раз в момент
                                // начала ИМЕННО ЭТОГО перетаскивания.
                                self.mesh_gizmo_snap_candidates = self.compute_snap_candidates(axis);
                                self.mesh_gizmo_snap_start_value = Self::axis_component(pivot, axis);
                                self.mesh_gizmo_snap_applied_value = self.mesh_gizmo_snap_start_value;
                                self.mesh_gizmo_snap_raw_delta = 0.0;
                                self.mesh_gizmo_snap_target = None;
                                let (ext_min, ext_max) = self.selected_axis_extent(axis)
                                    .unwrap_or((self.mesh_gizmo_snap_start_value, self.mesh_gizmo_snap_start_value));
                                self.mesh_gizmo_snap_min_offset = ext_min - self.mesh_gizmo_snap_start_value;
                                self.mesh_gizmo_snap_max_offset = ext_max - self.mesh_gizmo_snap_start_value;
                                return true;
                            }
                            return true;
                        }
                    }
                }
            }
        }

        // Ни наведения, ни драга на хэндле — обычный клик выделяет
        // вершину/грань под курсором (или снимает выделение при клике мимо).
        self.mesh_edit_click_select(ui, rect)
    }

    fn apply_mesh_gizmo_delta(&mut self, axis: crate::editor::GizmoAxisSel, mouse_delta: Vec2, rect: Rect) {
        let Some(pivot) = self.mesh_gizmo_pivot() else { return; };
        let Some(origin_screen) = self.world_to_screen(pivot, rect) else { return; };
        let handle_len = self.gizmo_handle_length(pivot);
        let Some(tip_screen) = self.world_to_screen(pivot + axis.world_dir() * handle_len, rect) else { return; };

        let axis_screen_vec = tip_screen - origin_screen;
        let screen_len = axis_screen_vec.length();
        if screen_len < 0.5 {
            return;
        }
        let axis_screen_dir = axis_screen_vec / screen_len;
        let pixels_along_axis = mouse_delta.x * axis_screen_dir.x + mouse_delta.y * axis_screen_dir.y;
        if pixels_along_axis == 0.0 {
            return;
        }

        let units_per_pixel = handle_len / screen_len;

        // ДОБАВЛЕНО (примагничивание к другим вершинам меша — по прямому
        // запросу пользователя: "снап к любой другой вершине меша", после
        // того как первая версия — защита только стартового выравнивания —
        // оказалась не тем, что нужно): считаем, куда бы уехало выделение
        // БЕЗ какого-либо снапа (`raw_target`), затем либо продолжаем уже
        // начатое примагничивание (пока `raw_target` не отъехал от цели
        // дальше радиуса отпускания), либо ищем среди кандидатов
        // (`mesh_gizmo_snap_candidates`, см. `compute_snap_candidates`)
        // ближайшего в радиусе захвата. Реально ПРИМЕНЯЕМ только разницу
        // между эффективной целью и уже применённым значением — пока цель
        // не меняется (держимся на кандидате), вершины стоят на месте;
        // когда меняется (нашли новый кандидат или отпустили старый),
        // скачком доезжают до неё.
        self.mesh_gizmo_snap_raw_delta += pixels_along_axis * units_per_pixel;
        let raw_target = self.mesh_gizmo_snap_start_value + self.mesh_gizmo_snap_raw_delta;

        const CAPTURE_PX: f32 = 10.0;
        const RELEASE_PX: f32 = 14.0; // чуть больше радиуса захвата — гистерезис против дребезга на границе
        let capture_world = CAPTURE_PX * units_per_pixel;
        let release_world = RELEASE_PX * units_per_pixel;

        if let Some(target) = self.mesh_gizmo_snap_target {
            if (raw_target - target).abs() >= release_world {
                self.mesh_gizmo_snap_target = None;
            }
        }
        if self.mesh_gizmo_snap_target.is_none() {
            // ИСПРАВЛЕНО (регрессия, найденная пользователем: "снап больше
            // ни на одной оси не залипает" — прошлый фикс сломал ВООБЩЕ
            // ВСЁ, не только починил X): раньше "исключение стартовой
            // позиции" использовало ТОТ ЖЕ `capture_world`, что и сам
            // поиск кандидата — но `capture_world` зависит от масштаба
            // экрана (`units_per_pixel`, см. выше), а при отдалённой
            // камере (как на скриншоте — куб небольшой в кадре) это может
            // быть ЗАМЕТНАЯ доля мировых единиц самого объекта. На
            // маленьком кубе (сторона 1) это исключало ВСЕ кандидаты
            // подряд, не только буквально совпадающие со стартом — отсюда
            // "вообще перестало залипать". Для проверки "это буквально та
            // же точка, откуда начали" нужен маленький ФИКСИРОВАННЫЙ
            // допуск, не зависящий от зума камеры — не `capture_world`.
            const SELF_MATCH_EPS: f32 = 1e-4;
            // ИСПРАВЛЕНО (см. комментарий у `mesh_gizmo_snap_min_offset`):
            // проверяем оба края выделения (мин. и макс. координату по этой
            // оси), а не только центроид (`raw_target` без поправки) — для
            // каждого края переводим "край совпал с кандидатом" в
            // равносильную цель для ПИВОТА (raw_target + offset края = край;
            // край = кандидат  =>  raw_target = кандидат - offset), чтобы
            // реально применяемый сдвиг (который двигает весь пивот) поставил
            // именно этот край точно на кандидата, а не середину выделения.
            let mut best: Option<(f32, f32)> = None; // (цель ДЛЯ ПИВОТА, расстояние)
            for &candidate in &self.mesh_gizmo_snap_candidates {
                for offset in [self.mesh_gizmo_snap_min_offset, self.mesh_gizmo_snap_max_offset] {
                    let edge_start = self.mesh_gizmo_snap_start_value + offset;
                    if (candidate - edge_start).abs() < SELF_MATCH_EPS {
                        continue;
                    }
                    let pivot_target = candidate - offset;
                    let d = (raw_target - pivot_target).abs();
                    if d < capture_world && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                        best = Some((pivot_target, d));
                    }
                }
            }
            self.mesh_gizmo_snap_target = best.map(|(v, _)| v);

            // ДОБАВЛЕНО (временная диагностика — по прямому запросу, раз
            // визуально протестировать не могу сам): показывает, что
            // реально происходит при каждой попытке примагничивания, чтобы
            // не гадать вслепую, если проблема всё ещё не решена.
            if let Some(target) = self.mesh_gizmo_snap_target {
                self.log(&format!("[snap] цель найдена: {:.4} (старт {:.4}, raw {:.4}, кандидатов {})", target, self.mesh_gizmo_snap_start_value, raw_target, self.mesh_gizmo_snap_candidates.len()), Color32::from_rgb(120, 220, 255));
            }
        }

        let effective_target = self.mesh_gizmo_snap_target.unwrap_or(raw_target);
        let delta_scalar = effective_target - self.mesh_gizmo_snap_applied_value;
        if delta_scalar == 0.0 {
            return; // держимся на кандидате — вершины не двигаются этот кадр
        }
        self.mesh_gizmo_snap_applied_value = effective_target;
        let world_delta = axis.world_dir() * delta_scalar;

        let Some(id) = self.edit_mesh_object else { return; };
        let world_transform = self.scene.get_world_transform(id);
        // Мировое смещение -> локальное пространство меша. `transform_point`
        // сначала масштабирует, потом вращает, потом сдвигает (см.
        // math/transform.rs) — для ВЕКТОРА (не точки) сдвиг не участвует,
        // так что обратное преобразование делает шаги в обратном порядке:
        // сначала отменяем вращение, потом масштаб.
        let rotated_back = world_transform.rotation.inverse().rotate(world_delta);
        let scale = world_transform.scale;
        let local_delta = Vec3::new(
            if scale.x.abs() > 1e-8 { rotated_back.x / scale.x } else { 0.0 },
            if scale.y.abs() > 1e-8 { rotated_back.y / scale.y } else { 0.0 },
            if scale.z.abs() > 1e-8 { rotated_back.z / scale.z } else { 0.0 },
        );

        let verts = self.effective_selected_vertices();
        if let Some(obj) = self.scene.get_object_mut(id) {
            if let ObjectType::Mesh(m) = &mut obj.object_type {
                crate::editor::mesh_edit::move_vertices(&mut m.mesh, &verts, local_delta);
            }
        }
        self.mesh_edit_history_dirty = true;
        self.refresh_gpu_mesh_live(id);
    }

    /// Снимок меша редактируемого объекта — вызывать ПЕРЕД началом правки
    /// (драг, экструзия, удаление), см. `mesh_edit_undo_snapshot`.
    fn mesh_edit_snapshot_before(&mut self) {
        self.mesh_edit_history_dirty = false;
        let Some(id) = self.edit_mesh_object else { self.mesh_edit_undo_snapshot = None; return; };
        let Some(obj) = self.scene.get_object(id) else { self.mesh_edit_undo_snapshot = None; return; };
        let ObjectType::Mesh(m) = &obj.object_type else { self.mesh_edit_undo_snapshot = None; return; };
        self.mesh_edit_undo_snapshot = Some(m.mesh.clone());
    }

    /// Закрывает правку, начатую `mesh_edit_snapshot_before` — если меш
    /// реально поменялся (`mesh_edit_history_dirty`), кладёт в историю ОДНУ
    /// команду `ModifyMesh` на весь жест целиком (не на каждый кадр драга).
    /// Безопасно звать даже если ничего не менялось — просто ничего не
    /// делает.
    fn mesh_edit_commit_history(&mut self) {
        let snapshot = self.mesh_edit_undo_snapshot.take();
        let dirty = self.mesh_edit_history_dirty;
        self.mesh_edit_history_dirty = false;
        if !dirty {
            return;
        }
        let Some(old_mesh) = snapshot else { return; };
        let Some(id) = self.edit_mesh_object else { return; };
        let Some(obj) = self.scene.get_object(id) else { return; };
        let ObjectType::Mesh(m) = &obj.object_type else { return; };
        self.history.push(crate::editor::EditorCommand::ModifyMesh {
            id,
            old_mesh,
            new_mesh: m.mesh.clone(),
        });
    }

    /// Обычный клик (без наведения на gizmo) в Edit Mode — выбирает
    /// ближайшую к курсору вершину/грань (в пределах пиксельного порога),
    /// либо снимает выделение при клике мимо (без Shift). Возвращает true,
    /// если клик вообще был обработан этим кадром (нажатие мыши зафиксировано,
    /// даже если ничего не попало под курсор) — тот же смысл, что у
    /// остальных `handle_*_input`, чтобы `handle_viewport_input` не начал
    /// параллельно двигать камеру.
    fn mesh_edit_click_select(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let Some(id) = self.edit_mesh_object else { return false; };
        if !ui.input(|i| i.pointer.primary_pressed()) {
            return false;
        }
        let Some(p) = ui.input(|i| i.pointer.hover_pos()) else { return false; };
        if !rect.contains(p) {
            return false;
        }
        let shift = ui.input(|i| i.modifiers.shift);
        // ИСПРАВЛЕНО (по прямому запросу пользователя: "убери отсоединение
        // грани от куба", затем "зачем ты alt вырезал? он мне нужен") —
        // Alt+клик остаётся способом выделить РОВНО один треугольник вместо
        // всей компланарной группы, но БЕЗ физического отрыва от соседей
        // (раньше Alt+клик ещё и дублировал общие вершины —
        // `detach_faces_from_neighbors` — это и убрали; сам факт выбора
        // одного треугольника отдельно от группы — нужная пользователю
        // функция, топологический разрыв меша — нет).
        let alt = ui.input(|i| i.modifiers.alt);

        let world_transform = self.scene.get_world_transform(id);
        let Some(obj) = self.scene.get_object(id) else { return false; };
        let ObjectType::Mesh(m) = &obj.object_type else { return false; };
        let mesh = &m.mesh;

        match self.edit_mesh_select_mode {
            crate::editor::MeshSelectMode::Vertex => {
                let mut best: Option<(usize, f32)> = None;
                for (i, &v) in mesh.vertices.iter().enumerate() {
                    let world_v = world_transform.transform_point(v);
                    let Some(screen) = self.world_to_screen(world_v, rect) else { continue; };
                    let d = (screen - p).length();
                    if d < 12.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                        best = Some((i, d));
                    }
                }
                if let Some((idx, _)) = best {
                    if shift {
                        if self.edit_selected_vertices.contains(&idx) {
                            self.edit_selected_vertices.remove(&idx);
                        } else {
                            self.edit_selected_vertices.insert(idx);
                        }
                    } else {
                        self.edit_selected_vertices.clear();
                        self.edit_selected_vertices.insert(idx);
                    }
                } else if !shift {
                    self.edit_selected_vertices.clear();
                }
            }
            crate::editor::MeshSelectMode::Face => {
                let count = crate::editor::mesh_edit::face_count(mesh);
                let mut best: Option<(usize, f32)> = None;
                for f in 0..count {
                    let Some(centroid) = crate::editor::mesh_edit::face_centroid(mesh, f) else { continue; };
                    let world_c = world_transform.transform_point(centroid);
                    let Some(screen) = self.world_to_screen(world_c, rect) else { continue; };
                    let d = (screen - p).length();
                    if d < 14.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                        best = Some((f, d));
                    }
                }
                if let Some((idx, _)) = best {
                    // Клик выделяет всю связную компланарную группу (см.
                    // `coplanar_face_group`) — для плоской поверхности (куб,
                    // плоскость) это ровно та "грань", что пользователь видит
                    // визуально (обычно 2 треугольника на квад). Alt+клик —
                    // осознанный выход из этого: РОВНО один треугольник под
                    // курсором, без расширения до соседей (без их отрыва).
                    let group: std::collections::BTreeSet<usize> = if alt {
                        [idx].into_iter().collect()
                    } else {
                        crate::editor::mesh_edit::coplanar_face_group(mesh, idx)
                    };
                    if shift {
                        if group.iter().all(|f| self.edit_selected_faces.contains(f)) {
                            for f in &group {
                                self.edit_selected_faces.remove(f);
                            }
                        } else {
                            self.edit_selected_faces.extend(group);
                        }
                    } else {
                        self.edit_selected_faces.clear();
                        self.edit_selected_faces.extend(group);
                    }
                } else if !shift {
                    self.edit_selected_faces.clear();
                }
            }
        }

        true
    }

    /// Дешёвое обновление GPU-меша "на месте" (см.
    /// `GpuRenderer::update_mesh_vertices`) — вызывается каждый кадр во
    /// время перетаскивания gizmo. Если число вершин почему-то разошлось
    /// (не должно происходить для чистого перемещения), откатывается на
    /// полное пересоздание, как структурные правки.
    fn refresh_gpu_mesh_live(&mut self, id: Uuid) {
        let Some(&mesh_idx) = self.gpu_mesh_map.get(&id) else { return; };
        let Some(obj) = self.scene.get_object(id) else { return; };
        let ObjectType::Mesh(m) = &obj.object_type else { return; };
        if let Some(renderer) = self.gpu_renderer.as_mut() {
            if !renderer.update_mesh_vertices(mesh_idx, &m.mesh) {
                let new_idx = renderer.add_mesh(&m.mesh);
                self.gpu_mesh_map.insert(id, new_idx);
            }
        }
    }

    /// Полное пересоздание GPU-меша — после структурных правок (экструзия/
    /// удаление), которые меняют число вершин/индексов, а не только их
    /// значения (см. `GpuRenderer::update_mesh_vertices` про то, почему
    /// именно они не могут переиспользовать существующий буфер).
    pub fn refresh_gpu_mesh_structural(&mut self, id: Uuid) {
        let Some(obj) = self.scene.get_object(id) else { return; };
        let ObjectType::Mesh(m) = &obj.object_type else { return; };
        if let Some(renderer) = self.gpu_renderer.as_mut() {
            let new_idx = renderer.add_mesh(&m.mesh);
            self.gpu_mesh_map.insert(id, new_idx);
        }
    }

    /// Вызывается после `history.undo`/`history.redo` — сам `CommandHistory`
    /// знает только про `Scene`, ничего не знает ни про GPU, ни про
    /// mesh-редактор. `id` — объект, которого коснулась отменённая/
    /// повторённая команда (см. `CommandHistory::undo`/`redo`).
    fn after_history_change(&mut self, id: Uuid) {
        // move/extrude/delete меняют число вершин меша, а undo/redo самого
        // объекта может вовсе создать/удалить его — старые индексы
        // выделения в mesh-редакторе (если это как раз редактируемый
        // объект) более не гарантированно валидны, безопаснее сбросить их,
        // чем рискнуть индексом за границей массива вершин.
        if self.edit_mesh_object == Some(id) {
            self.edit_selected_vertices.clear();
            self.edit_selected_faces.clear();
            self.mesh_gizmo_drag = None;
            self.mesh_gizmo_snap_target = None;
            self.mesh_gizmo_snap_candidates.clear();
        }
        if self.scene.get_object(id).is_some() {
            self.refresh_gpu_mesh_structural(id);
        } else {
            self.gpu_mesh_map.remove(&id);
        }
    }

    /// Клавиша E в Edit Mode (Face) — экструдирует выбранные грани вдоль их
    /// среднего нормали на дистанцию, пропорциональную размеру меша (тот же
    /// принцип, что и у `gizmo_handle_length` — фиксированное число единиц
    /// было бы то незаметным, то абсурдным в зависимости от масштаба
    /// объекта). Новое выделение после экструзии — только что созданные
    /// (выдвинутые) вершины, в режиме Vertex — гизмо сразу готов их подвинуть
    /// дальше, тот же поток действий, что в Blender (Extrude, затем Move).
    pub fn extrude_selected_faces(&mut self) {
        if !self.edit_mode {
            return;
        }
        if self.edit_mesh_select_mode != crate::editor::MeshSelectMode::Face {
            self.log("⚠️ Экструзия работает только в режиме выделения граней (клавиша 2)", Color32::YELLOW);
            return;
        }
        let Some(id) = self.edit_mesh_object else { return; };
        if self.edit_selected_faces.is_empty() {
            self.log("⚠️ Нет выделенных граней для экструзии", Color32::YELLOW);
            return;
        }

        let amount = {
            let Some(obj) = self.scene.get_object(id) else { return; };
            let ObjectType::Mesh(m) = &obj.object_type else { return; };
            let (min, max) = m.mesh.bounds;
            ((max - min).length() * 0.1).max(0.05)
        };

        self.mesh_edit_snapshot_before();
        let new_vertex_indices = {
            let Some(obj) = self.scene.get_object_mut(id) else { return; };
            let ObjectType::Mesh(m) = &mut obj.object_type else { return; };
            crate::editor::mesh_edit::extrude_faces(&mut m.mesh, &self.edit_selected_faces, amount)
        };

        if new_vertex_indices.is_empty() {
            self.mesh_edit_undo_snapshot = None;
            self.log("⚠️ Экструзия не дала результата (проверь выделение)", Color32::YELLOW);
            return;
        }

        let new_count = new_vertex_indices.len();
        self.edit_selected_faces.clear();
        self.edit_mesh_select_mode = crate::editor::MeshSelectMode::Vertex;
        self.edit_selected_vertices = new_vertex_indices;

        self.mesh_edit_history_dirty = true;
        self.mesh_edit_commit_history();
        self.refresh_gpu_mesh_structural(id);
        self.log(&format!("✅ Экструзия: {} новых вершин выделено", new_count), Color32::GREEN);
    }

    /// Клавиша Delete в Edit Mode — удаляет выбранные вершины (режим Vertex)
    /// или грани (режим Face) текущего редактируемого меша. Отдельная
    /// функция от обычного `Key::Delete` (который удаляет ЦЕЛЫЕ объекты
    /// сцены) — переключение между ними по `self.edit_mode` в обработчике
    /// клавиш (см. `update()`).
    pub fn delete_selected_mesh_elements(&mut self) {
        if !self.edit_mode {
            return;
        }
        let Some(id) = self.edit_mesh_object else { return; };

        match self.edit_mesh_select_mode {
            crate::editor::MeshSelectMode::Vertex => {
                if self.edit_selected_vertices.is_empty() {
                    return;
                }
                let selected = self.edit_selected_vertices.clone();
                self.mesh_edit_snapshot_before();
                if let Some(obj) = self.scene.get_object_mut(id) {
                    if let ObjectType::Mesh(m) = &mut obj.object_type {
                        crate::editor::mesh_edit::delete_vertices(&mut m.mesh, &selected);
                    }
                }
                self.edit_selected_vertices.clear();
                self.mesh_edit_history_dirty = true;
                self.mesh_edit_commit_history();
                self.log(&format!("🗑️ Удалено вершин: {}", selected.len()), Color32::YELLOW);
            }
            crate::editor::MeshSelectMode::Face => {
                if self.edit_selected_faces.is_empty() {
                    return;
                }
                let selected = self.edit_selected_faces.clone();
                self.mesh_edit_snapshot_before();
                if let Some(obj) = self.scene.get_object_mut(id) {
                    if let ObjectType::Mesh(m) = &mut obj.object_type {
                        crate::editor::mesh_edit::delete_faces(&mut m.mesh, &selected);
                    }
                }
                self.edit_selected_faces.clear();
                self.mesh_edit_history_dirty = true;
                self.mesh_edit_commit_history();
                self.log(&format!("🗑️ Удалено граней: {}", selected.len()), Color32::YELLOW);
            }
        }

        self.refresh_gpu_mesh_structural(id);
    }

    /// Отрисовка маркеров вершин/граней + подпись режима + gizmo Edit
    /// Mode — общая для GPU-оверлея (render_gpu_viewport) и CPU-фолбэка
    /// (ui/viewport.rs), тот же принцип, что и у `draw_spawn_marker`/
    /// `draw_gizmo`.
    pub fn draw_mesh_edit_overlay(&self, ui: &Ui, rect: Rect) {
        if !self.edit_mode {
            return;
        }
        let Some(id) = self.edit_mesh_object else { return; };
        let Some(obj) = self.scene.get_object(id) else { return; };
        let ObjectType::Mesh(m) = &obj.object_type else { return; };
        let mesh = &m.mesh;
        let world_transform = self.scene.get_world_transform(id);
        let painter = ui.painter();

        let mode_label = match self.edit_mesh_select_mode {
            crate::editor::MeshSelectMode::Vertex => "✏️ EDIT MODE — вершины (1/2 режим, E экструзия граней, Delete, Tab выход)",
            crate::editor::MeshSelectMode::Face => "✏️ EDIT MODE — грани (1/2 режим, Alt+клик — один треугольник, E экструзия, Delete, Tab выход)",
        };
        painter.text(
            rect.left_top() + egui::vec2(8.0, 8.0),
            Align2::LEFT_TOP,
            mode_label,
            FontId::proportional(13.0),
            Color32::from_rgb(255, 200, 60),
        );

        // ДОБАВЛЕНО (по прямому запросу пользователя — примагничивание к
        // другим вершинам меша во время драга, см. `mesh_gizmo_snap_target`):
        // пока активное перетаскивание реально держится на каком-то
        // кандидате — показываем это явно, иначе "вершины перестали
        // двигаться, хотя мышь ещё едет" выглядело бы как баг/лаг, а не
        // как осознанное примагничивание.
        if let (Some(drag), Some(target)) = (&self.mesh_gizmo_drag, self.mesh_gizmo_snap_target) {
            let axis_name = match drag.axis {
                crate::editor::GizmoAxisSel::X => "X",
                crate::editor::GizmoAxisSel::Y => "Y",
                crate::editor::GizmoAxisSel::Z => "Z",
            };
            painter.text(
                rect.left_top() + egui::vec2(8.0, 26.0),
                Align2::LEFT_TOP,
                format!("🔒 Примагничено: {} = {:.3} (другая вершина меша)", axis_name, target),
                FontId::proportional(12.0),
                Color32::from_rgb(120, 220, 255),
            );
        }

        match self.edit_mesh_select_mode {
            crate::editor::MeshSelectMode::Vertex => {
                for (i, &v) in mesh.vertices.iter().enumerate() {
                    let world_v = world_transform.transform_point(v);
                    let Some(p) = self.world_to_screen(world_v, rect) else { continue; };
                    let selected = self.edit_selected_vertices.contains(&i);
                    let color = if selected { Color32::from_rgb(255, 160, 40) } else { Color32::WHITE };
                    let radius = if selected { 5.0 } else { 3.5 };
                    painter.circle_filled(p, radius, color);
                    if selected {
                        painter.circle_stroke(p, radius + 2.0, Stroke::new(1.5, Color32::from_rgb(255, 220, 140)));
                    }
                }
            }
            crate::editor::MeshSelectMode::Face => {
                let count = crate::editor::mesh_edit::face_count(mesh);
                for f in 0..count {
                    let Some(centroid) = crate::editor::mesh_edit::face_centroid(mesh, f) else { continue; };
                    let world_c = world_transform.transform_point(centroid);
                    let Some(p) = self.world_to_screen(world_c, rect) else { continue; };
                    let selected = self.edit_selected_faces.contains(&f);
                    let color = if selected { Color32::from_rgb(255, 160, 40) } else { Color32::from_rgb(200, 200, 220) };
                    painter.circle_filled(p, if selected { 5.0 } else { 3.0 }, color);

                    if selected {
                        if let Some([a, b, c]) = crate::editor::mesh_edit::face_vertex_indices(mesh, f) {
                            let pa = mesh.vertices.get(a).and_then(|&v| self.world_to_screen(world_transform.transform_point(v), rect));
                            let pb = mesh.vertices.get(b).and_then(|&v| self.world_to_screen(world_transform.transform_point(v), rect));
                            let pc = mesh.vertices.get(c).and_then(|&v| self.world_to_screen(world_transform.transform_point(v), rect));
                            if let (Some(pa), Some(pb), Some(pc)) = (pa, pb, pc) {
                                let hl = Color32::from_rgb(255, 220, 140);
                                painter.line_segment([pa, pb], Stroke::new(2.0, hl));
                                painter.line_segment([pb, pc], Stroke::new(2.0, hl));
                                painter.line_segment([pc, pa], Stroke::new(2.0, hl));
                            }
                        }
                    }
                }
            }
        }

        if let Some(pivot) = self.mesh_gizmo_pivot() {
            if let Some(origin_screen) = self.world_to_screen(pivot, rect) {
                let handle_len = self.gizmo_handle_length(pivot);
                use crate::editor::GizmoAxisSel;
                for axis in [GizmoAxisSel::X, GizmoAxisSel::Y, GizmoAxisSel::Z] {
                    let Some(tip) = self.world_to_screen(pivot + axis.world_dir() * handle_len, rect) else { continue; };
                    let is_dragging_this = self.mesh_gizmo_drag.as_ref().map(|d| d.axis == axis).unwrap_or(false);
                    let is_hovered = self.mesh_gizmo_drag.is_none() && self.mesh_gizmo_hover_axis == Some(axis);
                    let active = is_dragging_this || is_hovered;
                    let color = if active { Color32::WHITE } else { axis.color() };
                    let width = if active { 4.0 } else { 2.5 };
                    painter.line_segment([origin_screen, tip], Stroke::new(width, color));
                    painter.circle_filled(tip, 5.0, color);
                }
                painter.circle_filled(origin_screen, 4.0, Color32::WHITE);
            }
        }
    }

    /// Визуальная отметка точки спавна во вьюпорте (флажок на шесте + линия
    /// направления взгляда) — общая для GPU-оверлея (render_gpu_viewport
    /// выше) и CPU-фолбэка (ui/viewport.rs), поэтому метод, а не
    /// продублированный код в обоих местах.
    pub fn draw_spawn_marker(&self, ui: &Ui, position: Vec3, forward: Vec3, rect: Rect) {
        let color = Color32::from_rgb(60, 220, 100);
        let painter = ui.painter();

        if let (Some(base), Some(top)) = (
            self.world_to_screen(position, rect),
            self.world_to_screen(position + Vec3::UP * 2.0, rect),
        ) {
            painter.line_segment([base, top], Stroke::new(2.5, color));
            painter.circle_filled(top, 5.0, color);
            painter.circle_stroke(base, 5.0, Stroke::new(1.5, color));
            painter.text(
                Pos2::new(top.x + 8.0, top.y - 4.0),
                Align2::LEFT_CENTER,
                "🚩 Spawn",
                FontId::proportional(11.0),
                color,
            );
        }

        if let (Some(p0), Some(p1)) = (
            self.world_to_screen(position, rect),
            self.world_to_screen(position + forward * 1.5, rect),
        ) {
            painter.line_segment([p0, p1], Stroke::new(2.0, color));
            painter.circle_filled(p1, 3.0, color);
        }
    }

    /// ДОБАВЛЕНО (по прямому запросу пользователя: показывать все
    /// анимационные точки во вьюпорте, чтобы их можно было двигать рукой):
    /// для каждого ВЫДЕЛЕННОГО объекта — маркер на позиции каждого
    /// keyframe'а любой его анимации с `show_keyframes == true`, плюс
    /// тонкая линия между соседними по времени keyframe'ами (чтобы был
    /// виден путь). Только `position_track` — см. комментарий у
    /// `DraggingKeyframe` про то, почему Rotation/Scale не рисуются как
    /// точки в 3D. Чистая отрисовка, ввод — в `handle_keyframe_marker_input`
    /// ниже (та же схема разделения, что у `draw_gizmo`/`handle_gizmo_input`).
    pub fn draw_keyframe_markers(&self, ui: &Ui, rect: Rect) {
        let painter = ui.painter();
        for &id in &self.scene.selected_ids {
            let Some(obj) = self.scene.get_object(id) else { continue; };
            for anim in obj.animations.values() {
                if !anim.show_keyframes { continue; }
                let points: Vec<Pos2> = anim.position_track.keyframes.iter()
                    .filter_map(|kf| self.world_to_screen(kf.value, rect))
                    .collect();
                for pair in points.windows(2) {
                    painter.line_segment([pair[0], pair[1]], Stroke::new(1.0, Color32::from_rgb(255, 170, 60)));
                }
                for (i, kf) in anim.position_track.keyframes.iter().enumerate() {
                    let Some(p) = self.world_to_screen(kf.value, rect) else { continue; };
                    let dragging_this = self.dragging_keyframe.as_ref()
                        .map(|d| d.object_id == id && d.animation_name == anim.name && d.index == i)
                        .unwrap_or(false);
                    let (radius, color) = if dragging_this {
                        (7.0, Color32::WHITE)
                    } else {
                        (5.0, Color32::from_rgb(255, 170, 60))
                    };
                    painter.circle_filled(p, radius, color);
                    painter.circle_stroke(p, radius, Stroke::new(1.0, Color32::BLACK));
                    if !kf.name.is_empty() {
                        painter.text(
                            Pos2::new(p.x, p.y - radius - 4.0),
                            Align2::CENTER_BOTTOM,
                            &kf.name,
                            FontId::proportional(11.0),
                            Color32::WHITE,
                        );
                    }
                }
            }
        }
    }

    /// Ввод для маркеров из `draw_keyframe_markers` — вызывается ДО
    /// `handle_gizmo_input` (см. eframe::App::update ниже) и "поглощает"
    /// клик/драг тем же способом (возврат true), если он попал по маркеру
    /// или маркер уже тащат: `handle_gizmo_input`/`handle_viewport_input`
    /// в этот кадр тогда не выполняются вовсе (short-circuit ||), чтобы
    /// перетаскивание точки не путалось с обычным gizmo объекта или сбросом
    /// выделения кликом по вьюпорту.
    fn handle_keyframe_marker_input(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        if let Some(drag) = &self.dragging_keyframe {
            if ui.input(|i| i.pointer.primary_down()) {
                let mouse_delta = ui.input(|i| i.pointer.delta());
                if mouse_delta.length_sq() > 0.0 {
                    let object_id = drag.object_id;
                    let animation_name = drag.animation_name.clone();
                    let index = drag.index;
                    let current_pos = self.scene.get_object(object_id)
                        .and_then(|obj| obj.animations.get(&animation_name))
                        .and_then(|anim| anim.position_track.keyframes.get(index))
                        .map(|kf| kf.value);
                    if let Some(pos) = current_pos {
                        let world_delta = self.screen_delta_to_world(mouse_delta, pos, rect);
                        if let Some(obj) = self.scene.get_object_mut(object_id) {
                            if let Some(anim) = obj.animations.get_mut(&animation_name) {
                                if let Some(kf) = anim.position_track.keyframes.get_mut(index) {
                                    kf.value = kf.value + world_delta;
                                }
                            }
                        }
                    }
                }
                return true;
            } else {
                self.dragging_keyframe = None;
                return true;
            }
        }

        let Some(p) = ui.input(|i| i.pointer.hover_pos()) else { return false; };
        if !rect.contains(p) { return false; }

        let mut best: Option<(Uuid, String, usize, f32)> = None;
        for &id in &self.scene.selected_ids {
            let Some(obj) = self.scene.get_object(id) else { continue; };
            for anim in obj.animations.values() {
                if !anim.show_keyframes { continue; }
                for (i, kf) in anim.position_track.keyframes.iter().enumerate() {
                    let Some(marker) = self.world_to_screen(kf.value, rect) else { continue; };
                    let d = (marker - p).length();
                    if d < 10.0 && best.as_ref().map(|(_, _, _, bd)| d < *bd).unwrap_or(true) {
                        best = Some((id, anim.name.clone(), i, d));
                    }
                }
            }
        }

        if let Some((object_id, animation_name, index, _)) = best {
            if ui.input(|i| i.pointer.primary_pressed()) {
                self.dragging_keyframe = Some(DraggingKeyframe { object_id, animation_name, index });
            }
            return true;
        }

        false
    }

    /// Чистая отрисовка (без чтения ввода) — состояние наведения/драга уже
    /// посчитано в `handle_gizmo_input` этим же кадром. `pub`, т.к. вызывается
    /// и из GPU-пути (render_gpu_viewport выше), и из CPU-фолбэка
    /// (ui/viewport.rs::render_viewport, другой модуль).
    pub fn draw_gizmo(&self, ui: &Ui, rect: Rect) {
        if self.current_tool == crate::editor::EditorTool::Select {
            return;
        }
        let ids = &self.scene.selected_ids;
        if ids.is_empty() {
            return;
        }
        let pivot = self.gizmo_pivot(ids);
        let Some(origin_screen) = self.world_to_screen(pivot, rect) else { return; };
        let handle_len = self.gizmo_handle_length(pivot);

        use crate::editor::GizmoAxisSel;
        let painter = ui.painter();
        for axis in [GizmoAxisSel::X, GizmoAxisSel::Y, GizmoAxisSel::Z] {
            let Some(tip) = self.world_to_screen(pivot + axis.world_dir() * handle_len, rect) else { continue; };
            let is_dragging_this = self.gizmo_drag.as_ref().map(|d| d.axis == axis).unwrap_or(false);
            let is_hovered = self.gizmo_drag.is_none() && self.gizmo_hover_axis == Some(axis);
            let active = is_dragging_this || is_hovered;
            let color = if active { Color32::WHITE } else { axis.color() };
            let width = if active { 4.0 } else { 2.5 };
            painter.line_segment([origin_screen, tip], Stroke::new(width, color));
            match self.current_tool {
                EditorTool::Rotate => { painter.circle_stroke(tip, 5.0, Stroke::new(2.0, color)); }
                EditorTool::Scale => {
                    painter.rect_filled(Rect::from_center_size(tip, egui::vec2(8.0, 8.0)), 1.0, color);
                }
                _ => { painter.circle_filled(tip, 5.0, color); }
            }
        }
        painter.circle_filled(origin_screen, 4.0, Color32::WHITE);
    }

    /// `suppress_select` — true, когда клик этого кадра уже обработал
    /// gizmo (`handle_gizmo_input`, вызывается раньше) — иначе тот же клик
    /// по хэндлу gizmo ДОПОЛНИТЕЛЬНО переключал бы выделение на "ближайший
    /// к камере объект" (см. цикл ниже), что не то поведение, которое
    /// ожидается при перетаскивании gizmo.
    /// Обратная проекция экранной точки в мировую позицию на плоскости
    /// земли (y=0) — тот же камерный базис (`dir`/`right`/`up`, тот же tan
    /// от `camera_fov`), что и `world_to_screen`, только в обратную
    /// сторону, чтобы 2D-точка курсора и 3D-луч, который она задаёт,
    /// оставались согласованы. Используется, чтобы поставить объект,
    /// перетащенный из браузера ассетов, ровно туда, куда его бросили, а
    /// не в случайное/фиксированное место. Если луч не пересекает землю
    /// перед камерой (смотрим вверх, или земля позади), откатывается на
    /// `camera_target` — на неё в любом случае сейчас смотрит пользователь.
    pub fn screen_to_ground_position(&self, screen: Pos2, rect: Rect) -> Vec3 {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let c = rect.center();
        // ИСПРАВЛЕНО (та же причина, что и у `world_to_screen` — см. её
        // комментарий): обратная проекция ОБЯЗАНА делить на тот же
        // множитель, каким прямая проекция умножает, иначе пара функций
        // перестаёт быть взаимно-обратной на неквадратном вьюпорте — объект,
        // перетащенный из браузера ассетов, приземлялся бы не под курсором,
        // а со сдвигом по X.
        let x = (screen.x - c.x) / (rect.height() * 0.5).max(1.0);
        let y = (c.y - screen.y) / (rect.height() * 0.5).max(1.0);
        let tf = (self.camera_fov.to_radians() * 0.5).tan();
        let ray_dir = (dir + right * (x * tf) + up * (y * tf)).normalize();

        if ray_dir.y.abs() > 1e-4 {
            let t = -self.camera_position.y / ray_dir.y;
            if t > 0.0 {
                return self.camera_position + ray_dir * t;
            }
        }
        self.camera_target
    }

    /// Импортирует файл ассета (по расширению) и, если задано, ставит
    /// результат в мировую позицию `place_at` — общая точка входа и для
    /// двойного клика, и для drag-and-drop из браузера ассетов
    /// (ui/asset_browser.rs).
    pub fn import_asset_path(&mut self, path: &std::path::Path, place_at: Option<Vec3>) {
        let path_str = path.to_string_lossy().to_string();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match ext.as_str() {
            "altex" => self.import_altex_from_path(&path_str, place_at),
            "obj" | "fbx" | "gltf" | "glb" | "blend" => self.import_model_async(&path_str),
            "alworld" => self.import_alworld_from_path(&path_str),
            "alfar" => self.import_alfar_from_path(&path_str),
            _ => self.log(&format!("⚠️ Не знаю, как импортировать: {}", path_str), Color32::YELLOW),
        }
    }

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect, suppress_select: bool) {
        self.viewport_rect = rect;

        // Drop зона для drag-and-drop из браузера ассетов — проверяется
        // ДАЖЕ если курсор вне `rect` в момент этого вызова не имеет
        // смысла (сброс работает только над вьюпортом), но `pointer.
        // hover_pos()` в момент отпускания кнопки уже отражает финальную
        // позицию курсора, так что достаточно простой проверки `rect.
        // contains`.
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
            if rect.contains(p) && ui.input(|i| i.pointer.any_released()) {
                if let Some(path) = egui::DragAndDrop::take_payload::<std::path::PathBuf>(ui.ctx()) {
                    let world_pos = self.screen_to_ground_position(p, rect);
                    self.import_asset_path(&path, Some(world_pos));
                }
            }
        }

        if !ui.rect_contains_pointer(rect) {
            return;
        }
        let mp = ui.input(|i| i.pointer.hover_pos());
        let left = ui.input(|i| i.pointer.button_down(PointerButton::Primary));
        let right = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let middle = ui.input(|i| i.pointer.button_down(PointerButton::Middle));
        let shift = ui.input(|i| i.modifiers.shift);

        if right {
            if let (Some(c), Some(l)) = (mp, self.last_mouse_pos) {
                self.orbit_camera(c.x - l.x, c.y - l.y);
            }
        }
        if middle || (right && shift) {
            if let (Some(c), Some(l)) = (mp, self.last_mouse_pos) {
                self.pan_camera(c.x - l.x, c.y - l.y);
            }
        }
        ui.input(|i| {
            if i.smooth_scroll_delta.y != 0.0 {
                self.zoom_camera(i.smooth_scroll_delta.y);
            }
        });

        if left && !self.left_mouse_pressed && !suppress_select {
            if !shift {
                self.scene.selected_ids.clear();
            }
            let mut cid = None;
            let mut md = f32::MAX;
            for (&id, _) in &self.scene.objects {
                let p = self.scene.get_world_transform(id).position;
                let d = (p - self.camera_position).length();
                if d < md {
                    md = d;
                    cid = Some(id);
                }
            }
            if let Some(id) = cid {
                self.scene.select(id, true);
            }
        }
        self.left_mouse_pressed = left;
        self.last_mouse_pos = mp;
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        self.last_update_time = now;
        self.check_pending_imports();
        self.scene.update(0.016);

        let discord_details = format!("Editing '{}'", self.scene.name);
        let discord_state = format!("{} objects", self.scene.objects.len());
        self.discord_presence.update(&discord_details, &discord_state);

        ctx.input(|i| {
            if i.key_pressed(Key::W) { self.current_tool = EditorTool::Move; }
            if i.key_pressed(Key::E) {
                // ДОБАВЛЕНО (редактор вершин/граней): в Edit Mode E —
                // экструзия выбранных граней, а не переключение на Rotate
                // (вне Edit Mode поведение прежнее).
                if self.edit_mode {
                    self.extrude_selected_faces();
                } else {
                    self.current_tool = EditorTool::Rotate;
                }
            }
            if i.key_pressed(Key::R) { self.current_tool = EditorTool::Scale; }
            if i.key_pressed(Key::Q) { self.current_tool = EditorTool::Select; }

            // ДОБАВЛЕНО (редактор вершин/граней): Tab — вход/выход из Edit
            // Mode; 1/2 — режим выделения (вершины/грани), только пока Edit
            // Mode активен, чтобы не перехватывать эти клавиши в остальном UI.
            if i.key_pressed(Key::Tab) {
                self.toggle_edit_mode();
            }
            if self.edit_mode {
                if i.key_pressed(Key::Num1) {
                    self.edit_mesh_select_mode = crate::editor::MeshSelectMode::Vertex;
                    self.edit_selected_faces.clear();
                }
                if i.key_pressed(Key::Num2) {
                    self.edit_mesh_select_mode = crate::editor::MeshSelectMode::Face;
                    self.edit_selected_vertices.clear();
                }
            }

            if i.key_pressed(Key::Delete) {
                // ДОБАВЛЕНО (редактор вершин/граней): в Edit Mode Delete
                // удаляет выбранные вершины/грани РЕДАКТИРУЕМОГО меша, а не
                // целые объекты сцены (прежнее поведение, сохранено для
                // Object Mode).
                if self.edit_mode {
                    self.delete_selected_mesh_elements();
                } else {
                    if self.gpu_renderer.is_some() {
                        for id in &self.scene.selected_ids {
                            self.gpu_mesh_map.remove(id);
                            self.gpu_material_map.remove(id);
                        }
                    }
                    self.scene.delete_selected();
                }
            }

            if i.key_pressed(Key::Z) && i.modifiers.ctrl {
                if let Some(id) = self.history.undo(&mut self.scene) {
                    self.after_history_change(id);
                }
            }
            if i.key_pressed(Key::Y) && i.modifiers.ctrl {
                if let Some(id) = self.history.redo(&mut self.scene) {
                    self.after_history_change(id);
                }
            }

            if i.key_pressed(Key::F5) {
                self.log(
                    &format!("GPU: {}", if self.gpu_initialized { "ACTIVE" } else { "NOT INITIALIZED" }),
                    if self.gpu_initialized { Color32::GREEN } else { Color32::YELLOW }
                );
            }
        });

        self.frame_count += 1;
        if now - self.last_frame_time > 1.0 {
            self.fps = self.frame_count as f32;
            self.frame_count = 0;
            self.last_frame_time = now;
        }

        crate::ui::menu_bar::render_menu_bar(ctx, self);
        crate::ui::hierarchy::render_hierarchy(ctx, self);
        crate::ui::inspector::render_inspector(ctx, self);
        crate::ui::console::render_console(ctx, self);
        crate::ui::status_bar::render_status_bar(ctx, self);
        crate::ui::dialogs::render_dialogs(ctx, self);
        crate::ui::asset_browser::render_asset_browser(ctx, self);
        crate::ui::sound_bank_editor::render_sound_bank_editor(ctx, self);
        crate::ui::route_editor::render_route_editor(ctx, self);
        crate::ui::script_editor::render_script_editor(ctx, self);
        crate::ui::assembly_editor::render_assembly_editor(ctx, self);
        crate::ui::car_preset_editor::render_car_preset_editor(ctx, self);
        crate::ui::material_library_editor::render_material_library_editor(ctx, self);

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            // ДОБАВЛЕНО (редактор вершин/граней): в Edit Mode клик/gizmo
            // обрабатывает вершины/грани редактируемого меша, а не объекты
            // сцены — `suppress_select` дополнительно форсируется в true в
            // Edit Mode, чтобы клик по вершине не переключал заодно
            // "выделенный объект" (см. handle_viewport_input ниже).
            let gizmo_active = if self.edit_mode {
                self.handle_mesh_edit_input(ui, rect)
            } else {
                self.handle_keyframe_marker_input(ui, rect) || self.handle_gizmo_input(ui, rect)
            };
            self.handle_viewport_input(ui, rect, gizmo_active || self.edit_mode);

            // Сначала даём шанс загрузить
            self.process_upload_queue(self.max_upload_bytes_per_frame);

            // ИСПРАВЛЕНО (главный баг GPU-рендера): условие раньше требовало
            // self.gpu_renderer.is_some(), а единственное место, где
            // gpu_renderer вообще становится Some, — это ВНУТРИ
            // render_gpu_viewport() (см. create_gpu_renderer() выше). Получался
            // замкнутый круг: чтобы вызвать render_gpu_viewport(), рендерер уже
            // должен существовать, а чтобы он появился, нужно вызвать
            // render_gpu_viewport() — то есть GPU-путь не мог включиться
            // никогда, и viewport вечно падал на CPU wireframe-fallback
            // (ui/viewport.rs), даже когда GPU был полностью готов. Теперь
            // проверяем только gpu_initialized — сама render_gpu_viewport()
            // уже умеет создать renderer при первом вызове (и корректно
            // откатывается на CPU-fallback, если create_gpu_renderer() всё же
            // вернёт Err).
            if self.gpu_initialized {
                self.render_gpu_viewport(ui, rect);
            } else {
                crate::ui::viewport::render_viewport(ui, self);
            }
        });

        ctx.request_repaint();
    }
}

// CPU helpers (unchanged)...
fn render_bounding_box_gpu(
    ui: &Ui,
    mesh: &Mesh,
    transform: &crate::math::Transform,
    selected: bool,
    rect: Rect,
    app: &EditorApp,
) {
    use crate::math::Vec3;

    let (min, max) = mesh.bounds;
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];

    let transformed: Vec<Pos2> = corners
        .iter()
        .filter_map(|c| app.world_to_screen(transform.transform_point(*c), rect))
        .collect();

    if transformed.len() < 8 {
        return;
    }

    let color = if selected {
        Color32::from_rgb(255, 200, 100)
    } else {
        Color32::from_rgb(255, 255, 0)
    };

    let edges = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];

    for &(a, b) in &edges {
        ui.painter().line_segment([transformed[a], transformed[b]], (1.5, color));
    }
}

fn render_mesh_cpu(
    ui: &Ui,
    mesh: &Mesh,
    transform: &crate::math::Transform,
    selected: bool,
    rect: Rect,
    app: &EditorApp,
) {
    let color = if selected {
        Color32::from_rgb(255, 200, 100)
    } else {
        Color32::from_rgb(180, 180, 200)
    };

    let tc = mesh.indices.len() / 3;
    let step = if tc > 1000 { 2 } else { 1 };

    for i in (0..tc).step_by(step) {
        let idx = i * 3;
        if idx + 2 >= mesh.indices.len() {
            continue;
        }

        let i0 = mesh.indices[idx] as usize;
        let i1 = mesh.indices[idx + 1] as usize;
        let i2 = mesh.indices[idx + 2] as usize;

        if i0 >= mesh.vertices.len() || i1 >= mesh.vertices.len() || i2 >= mesh.vertices.len() {
            continue;
        }

        let v0 = transform.transform_point(mesh.vertices[i0]);
        let v1 = transform.transform_point(mesh.vertices[i1]);
        let v2 = transform.transform_point(mesh.vertices[i2]);

        if let (Some(p0), Some(p1), Some(p2)) = (
            app.world_to_screen(v0, rect),
            app.world_to_screen(v1, rect),
            app.world_to_screen(v2, rect)
        ) {
            ui.painter().line_segment([p0, p1], (1.0, color));
            ui.painter().line_segment([p1, p2], (1.0, color));
            ui.painter().line_segment([p2, p0], (1.0, color));
        }
    }
}
/// Кратчайшее расстояние от точки `p` до отрезка `[a, b]` на экране —
/// используется наведением/hit-тестом gizmo (`EditorApp::handle_gizmo_input`).
fn distance_point_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let ab_len2 = ab.x * ab.x + ab.y * ab.y;
    if ab_len2 < 1e-6 {
        return (p - a).length();
    }
    let t = (((p - a).x * ab.x + (p - a).y * ab.y) / ab_len2).clamp(0.0, 1.0);
    let closest = a + ab * t;
    (p - closest).length()
}
