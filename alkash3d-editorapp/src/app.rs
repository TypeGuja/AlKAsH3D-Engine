// src/app.rs - ПОЛНАЯ GPU ВЕРСИЯ (исправления: очередь загрузки, избежание double-borrow, оценка байтов без приватных типов)
use eframe::egui;
use egui::*;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::collections::HashMap;
use crate::gpu::GpuRenderer;
use crate::math::Vec3;
use crate::scene::{Scene, GameObject, ObjectType, MeshComponent, LightComponent, LightType, AudioSourceComponent, ScriptedEntityComponent};
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

        if let Some(ref mut renderer) = self.gpu_renderer {
            renderer.camera.position = self.camera_position;
            renderer.camera.target = self.camera_target;
            renderer.camera.up = self.camera_up;
            renderer.camera.fov = self.camera_fov.to_radians();
            renderer.camera.aspect = rect.width() / rect.height();

            let width = rect.width() as u32;
            let height = rect.height() as u32;

            // Рендерим сцену в offscreen текстуру
            renderer.render(&render_objects, width, height);

            // Пытаемся обновить egui текстуру (если готов readback)
            renderer.try_update_egui_texture(ui.ctx());

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
            c.x + x * rect.width() * 0.5,
            c.y - y * rect.height() * 0.5,
        ))
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
        let x = (screen.x - c.x) / (rect.width() * 0.5).max(1.0);
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

        ctx.input(|i| {
            if i.key_pressed(Key::W) { self.current_tool = EditorTool::Move; }
            if i.key_pressed(Key::E) { self.current_tool = EditorTool::Rotate; }
            if i.key_pressed(Key::R) { self.current_tool = EditorTool::Scale; }
            if i.key_pressed(Key::Q) { self.current_tool = EditorTool::Select; }

            if i.key_pressed(Key::Delete) {
                if self.gpu_renderer.is_some() {
                    for id in &self.scene.selected_ids {
                        self.gpu_mesh_map.remove(id);
                        self.gpu_material_map.remove(id);
                    }
                }
                self.scene.delete_selected();
            }

            if i.key_pressed(Key::Z) && i.modifiers.ctrl {
                self.history.undo(&mut self.scene);
            }
            if i.key_pressed(Key::Y) && i.modifiers.ctrl {
                self.history.redo(&mut self.scene);
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            let gizmo_active = self.handle_gizmo_input(ui, rect);
            self.handle_viewport_input(ui, rect, gizmo_active);

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
