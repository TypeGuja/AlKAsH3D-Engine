// src/app.rs - ПОЛНАЯ GPU ВЕРСИЯ (исправления: очередь загрузки, избежание double-borrow, оценка байтов без приватных типов)
use eframe::egui;
use egui::*;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::collections::HashMap;
use crate::gpu::GpuRenderer;
use crate::math::Vec3;
use crate::scene::{Scene, GameObject, ObjectType, MeshComponent};
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
                Ok(mut renderer) => {
                    // очередь заполнится позже; для существующих объектов — поставим задачи в очередь
                    for (&id, obj) in &self.scene.objects {
                        if let ObjectType::Mesh(ref m) = obj.object_type {
                            let verts = m.mesh.vertices.len();
                            let inds = m.mesh.indices.len();
                            // estimate: 9 floats per vertex (pos+normal+color) => 9*4 = 36 bytes per vertex
                            let bytes = verts * 36 + inds * 4;
                            let task = UploadTask {
                                id,
                                name: obj.name.clone(),
                                mesh: m.mesh.clone(),
                                material: m.material.clone(),
                                estimated_bytes: bytes.max(1024),
                            };
                            self.upload_queue.push_back(task);
                        }
                    }
                    self.gpu_renderer = Some(renderer);
                    self.log("✅ GPU renderer created! Upload queue initialized.", Color32::GREEN);
                }
                Err(e) => {
                    self.log(&format!("❌ GPU init failed: {}", e), Color32::RED);
                    return;
                }
            }
        }

        // обработаем очередь (budget per frame)
        self.process_upload_queue(self.max_upload_bytes_per_frame);

        let render_objects = self.get_gpu_render_objects();

        if render_objects.is_empty() {
            crate::ui::viewport::render_viewport(ui, self);
            return;
        }

        if self.gpu_texture_id.is_none() {
            let size = [rect.width() as usize, rect.height() as usize];
            let handle = ui.ctx().load_texture(
                "gpu-3d",
                egui::ColorImage::new(size, Color32::BLACK),
                egui::TextureOptions::LINEAR,
            );
            self.gpu_texture_id = Some(handle.id());
        }

        if let Some(ref mut renderer) = self.gpu_renderer {
            renderer.camera.position = self.camera_position;
            renderer.camera.target = self.camera_target;
            renderer.camera.up = self.camera_up;
            renderer.camera.fov = self.camera_fov.to_radians();
            renderer.camera.aspect = rect.width() / rect.height();

            // Используем render_to_egui_texture вместо render_to_image
            if let Some(tex_id) = self.gpu_texture_id {
                renderer.render_to_egui_texture(
                    &render_objects,
                    tex_id,
                    ui.ctx(),
                    rect.width() as u32,
                    rect.height() as u32,
                );

                ui.painter().image(
                    tex_id,
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }

        // overlay/grid/names (как раньше)
        if self.scene.grid_enabled {
            let gc = Color32::from_rgb(60, 60, 70);
            for i in -20..=20 {
                for j in -20..=20 {
                    let x = i as f32; let z = j as f32;
                    if let (Some(p1), Some(p2)) = (
                        self.world_to_screen(Vec3::new(x, 0.0, z), rect),
                        self.world_to_screen(Vec3::new(x + 1.0, 0.0, z), rect)
                    ) {
                        ui.painter().line_segment([p1, p2], (1.0, gc));
                    }
                }
            }
        }

        for obj in self.scene.objects.values() {
            if !obj.visible { continue; }
            let world = self.scene.get_world_transform(obj.id);
            let selected = self.scene.selected_ids.contains(&obj.id);
            if let Some(pos) = self.world_to_screen(world.position + Vec3::new(0.0, 1.0, 0.0), rect) {
                ui.painter().text(
                    pos, Align2::CENTER_CENTER, &obj.name,
                    FontId::proportional(10.0),
                    if selected { Color32::WHITE } else { Color32::LIGHT_GRAY }
                );
            }
        }
    }

    fn get_gpu_render_objects(&self) -> Vec<(usize, [[f32; 4]; 4], usize)> {
        let mut objects = Vec::new();

        let forward = (self.camera_target - self.camera_position).normalize();

        for (&id, obj) in &self.scene.objects {
            if !obj.visible { continue; }

            if let ObjectType::Mesh(_) = obj.object_type {
                if let Some(&mesh_idx) = self.gpu_mesh_map.get(&id) {
                    if let Some(&mat_idx) = self.gpu_material_map.get(&id) {
                        let transform = self.scene.get_world_transform(id);
                        let center = transform.position;
                        let to_obj = center - self.camera_position;
                        if to_obj.dot(forward) <= 0.0 {
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
        let s = self.camera_position.length() * 0.001;
        let off = right * (-dx * s) + up * (dy * s);
        self.camera_position = self.camera_position + off;
        self.camera_target = self.camera_target + off;
    }

    pub fn zoom_camera(&mut self, delta: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let d = (self.camera_target - self.camera_position).length();
        let nd = (d * (1.0 - delta * 0.001)).clamp(2.0, 50.0);
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

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect) {
        self.viewport_rect = rect;
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

        if left && !self.left_mouse_pressed {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            self.handle_viewport_input(ui, rect);

            // Сначала даём шанс загрузить
            self.process_upload_queue(self.max_upload_bytes_per_frame);

            if self.gpu_initialized && self.gpu_renderer.is_some() {
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