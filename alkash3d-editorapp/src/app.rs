/// src/app.rs
use std::time::Instant;
use std::collections::{HashMap, VecDeque};
use eframe::emath::{pos2, Align2, Pos2};
use eframe::epaint::{Color32, FontId};
use egui::{Key, PointerButton, Rect, Ui};
// Используем super:: для доступа к родительскому модулю (lib.rs)
use super::ui;
use super::editor::{CommandHistory, EditorTool, Gizmo};
use super::scene::{Scene, GameObject, MeshComponent, ObjectType};
use super::math::Vec3;
use super::assets::AssetLibrary;
use super::gpu::GpuRenderer;
use super::material::Material;
use super::systems::{
    CinematicManager, MaterialAccelerator, ScriptingEngine,
    ShaderManager, SpatialAudioSystem, WorldStreamer
};
use super::mesh::Mesh;
use crate::memory::{AssetCache, FrameAllocator, ObjectPool};


pub struct PendingImport {
    pub path: String,
    pub receiver: std::sync::mpsc::Receiver<Result<ImportResult, String>>,
}

#[derive(Debug)]
pub struct ImportResult {
    pub mesh_names: Vec<String>,
    pub meshes: Vec<(String, Mesh)>,
}

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

    // Оптимизации
    pub frame_allocator: FrameAllocator,
    pub object_pool: ObjectPool<CachedTransform>,
    pub asset_cache: AssetCache<Vec<u8>>,
    pub frame_start: Instant,
    pub fps_counter: FPSCounter,
    pub visible_objects: Vec<bool>,
    pub transform_cache: HashMap<uuid::Uuid, [[f32; 4]; 4]>,
    pub bounds_cache: HashMap<uuid::Uuid, (Vec3, f32)>,
}

#[derive(Clone)]
pub struct CachedTransform {
    pub matrix: [[f32; 4]; 4],
    pub position: Vec3,
    pub bounds_center: Vec3,
    pub bounds_radius: f32,
}

pub struct FPSCounter {
    frames: u32,
    last_time: Instant,
    current_fps: f32,
}

impl FPSCounter {
    pub fn new() -> Self {
        Self {
            frames: 0,
            last_time: Instant::now(),
            current_fps: 0.0,
        }
    }

    pub fn update(&mut self) -> f32 {
        self.frames += 1;
        let elapsed = self.last_time.elapsed().as_secs_f32();
        if elapsed >= 1.0 {
            self.current_fps = self.frames as f32 / elapsed;
            self.frames = 0;
            self.last_time = Instant::now();
        }
        self.current_fps
    }
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
            frame_allocator: FrameAllocator::new(256),
            object_pool: ObjectPool::new(
                || CachedTransform {
                    matrix: [[0.0; 4]; 4],
                    position: Vec3::ZERO,
                    bounds_center: Vec3::ZERO,
                    bounds_radius: 0.0,
                },
                1024,
                100_000,
            ),
            asset_cache: AssetCache::new(60.0, 1024),
            frame_start: Instant::now(),
            fps_counter: FPSCounter::new(),
            visible_objects: Vec::with_capacity(10000),
            transform_cache: HashMap::with_capacity(10000),
            bounds_cache: HashMap::with_capacity(10000),
        };

        app.log("🚀 Editor started with extreme performance optimizations!", Color32::GREEN);
        app
    }

    fn render_gpu_viewport(&mut self, ui: &mut Ui, rect: Rect) {
        self.update_visibility_cache();
        let render_objects = self.get_visible_gpu_objects();

        if render_objects.is_empty() {
            super::ui::viewport::render_viewport(ui, self);
            return;
        }

        if self.gpu_texture_id.is_none() {
            let handle = ui.ctx().load_texture(
                "gpu3d",
                egui::ColorImage::new(
                    [rect.width() as usize, rect.height() as usize],
                    Color32::BLACK
                ),
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

        self.render_overlay(ui, rect);
    }

    fn update_visibility_cache(&mut self) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let near_dist = 0.1;
        let far_dist = 1000.0;
        let tan_half_fov = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();

        self.visible_objects.clear();

        let aspect = if self.viewport_rect.width() > 0.0 {
            self.viewport_rect.width() / self.viewport_rect.height().max(1.0)
        } else {
            16.0 / 9.0
        };

        let object_data: Vec<(uuid::Uuid, bool)> = self.scene.objects.iter()
            .map(|(&id, obj)| (id, obj.visible))
            .collect();

        for (id, visible) in object_data {
            if !visible {
                self.visible_objects.push(false);
                continue;
            }

            let (center, radius) = self.get_object_bounds(id);
            let rel = center - self.camera_position;
            let dist = rel.dot(dir);

            if dist < near_dist || dist > far_dist {
                self.visible_objects.push(false);
                continue;
            }

            let half_width = tan_half_fov * dist;
            let half_height = half_width / aspect;

            let dx = rel.dot(right);
            let dy = rel.dot(up);

            if dx.abs() - radius > half_width || dy.abs() - radius > half_height {
                self.visible_objects.push(false);
            } else {
                self.visible_objects.push(true);
            }
        }
    }

    fn get_object_bounds(&mut self, id: uuid::Uuid) -> (Vec3, f32) {
        if let Some(&bounds) = self.bounds_cache.get(&id) {
            return bounds;
        }

        if let Some(obj) = self.scene.get_object(id) {
            let transform = self.scene.get_world_transform(id);
            let center = transform.position;

            let radius = match &obj.object_type {
                ObjectType::Mesh(mesh_comp) => {
                    let (min, max) = mesh_comp.mesh.bounds;
                    let world_min = transform.transform_point(min);
                    let world_max = transform.transform_point(max);
                    (world_max - world_min).length() * 0.5
                }
                _ => 1.0,
            };

            let bounds = (center, radius);
            self.bounds_cache.insert(id, bounds);
            bounds
        } else {
            (Vec3::ZERO, 1.0)
        }
    }

    fn get_visible_gpu_objects(&self) -> Vec<(usize, [[f32; 4]; 4], usize)> {
        let mut objects = Vec::new();

        for (i, (&id, obj)) in self.scene.objects.iter().enumerate() {
            if i < self.visible_objects.len() && !self.visible_objects[i] {
                continue;
            }

            if !obj.visible {
                continue;
            }

            if let ObjectType::Mesh(_) = obj.object_type {
                if let Some(&mesh_idx) = self.gpu_mesh_map.get(&id) {
                    if let Some(&mat_idx) = self.gpu_material_map.get(&id) {
                        let model_matrix = if let Some(&matrix) = self.transform_cache.get(&id) {
                            matrix
                        } else {
                            let transform = self.scene.get_world_transform(id);
                            transform.to_matrix()
                        };

                        objects.push((mesh_idx, model_matrix, mat_idx));
                    }
                }
            }
        }

        objects
    }

    fn render_overlay(&self, ui: &mut Ui, rect: Rect) {
        if self.scene.grid_enabled {
            let gc = Color32::from_rgb(60, 60, 70);
            for i in -20..=20 {
                for j in -20..=20 {
                    let x = i as f32;
                    let z = j as f32;
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
            if !obj.visible {
                continue;
            }
            let world = self.scene.get_world_transform(obj.id);
            if let Some(pos) = self.world_to_screen(
                world.position + Vec3::new(0.0, 1.0, 0.0),
                rect
            ) {
                ui.painter().text(
                    pos,
                    Align2::CENTER_CENTER,
                    &obj.name,
                    FontId::proportional(10.0),
                    Color32::WHITE,
                );
            }
        }
    }

    pub fn orbit_camera(&mut self, dx: f32, dy: f32) {
        let dir = self.camera_position - self.camera_target;
        let r = dir.length();
        if r < 0.01 { return; }
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
        if dist <= 0.01 { return None; }
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
        if !ui.rect_contains_pointer(rect) { return; }
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
            if !shift { self.scene.selected_ids.clear(); }
            let mut cid = None;
            let mut md = f32::MAX;
            for (&id, _) in &self.scene.objects {
                let p = self.scene.get_world_transform(id).position;
                let d = (p - self.camera_position).length();
                if d < md { md = d; cid = Some(id); }
            }
            if let Some(id) = cid { self.scene.select(id, true); }
        }
        self.left_mouse_pressed = left;
        self.last_mouse_pos = mp;
    }

    pub fn log(&mut self, msg: &str, color: Color32) {
        self.console_messages.push_back((msg.to_string(), color));
        if self.console_messages.len() > 100 {
            self.console_messages.pop_front();
        }
        self.status_message = msg.to_string();
    }

    pub fn import_model_async(&mut self, path: &str) {
        let path_owned = path.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
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
                    let _ = tx.send(Ok(ImportResult { mesh_names: names, meshes }));
                }
                Err(e) => { let _ = tx.send(Err(e)); }
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
        if self.pending_imports.is_empty() { return; }

        let mut results = Vec::new();
        let mut completed_indices = Vec::new();

        for (i, imp) in self.pending_imports.iter().enumerate() {
            if let Ok(r) = imp.receiver.try_recv() {
                completed_indices.push(i);
                results.push(r);
            }
        }

        for &i in completed_indices.iter().rev() {
            self.pending_imports.remove(i);
        }

        for result in results {
            match result {
                Ok(ir) => {
                    let mut total_tris = 0;
                    for (name, mesh) in ir.meshes {
                        let tris = mesh.indices.len() / 3;
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

                        if let Some(ref mut renderer) = self.gpu_renderer {
                            let gpu_idx = renderer.add_mesh(&mesh);
                            self.gpu_mesh_map.insert(id, gpu_idx);
                            let mat_idx = renderer.add_material([0.7, 0.7, 0.7, 1.0], 0.0, 0.5);
                            self.gpu_material_map.insert(id, mat_idx);
                        }

                        self.transform_cache.remove(&id);
                        self.bounds_cache.remove(&id);
                    }
                    self.log(
                        &format!("✅ Import complete: {}K tris in {} meshes",
                                 total_tris / 1000, self.gpu_mesh_map.len()),
                        Color32::GREEN,
                    );
                }
                Err(e) => {
                    self.log(&format!("❌ Import error: {}", e), Color32::RED);
                }
            }
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_allocator.reset_frame();

        let now = ctx.input(|i| i.time);
        self.last_update_time = now;

        self.check_pending_imports();
        self.scene.update(0.016);
        self.fps = self.fps_counter.update();

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
                        self.transform_cache.remove(id);
                        self.bounds_cache.remove(id);
                    }
                }
                self.scene.delete_selected();
            }

            if i.key_pressed(Key::Z) && i.modifiers.ctrl {
                self.history.undo(&mut self.scene);
                self.transform_cache.clear();
                self.bounds_cache.clear();
            }
            if i.key_pressed(Key::Y) && i.modifiers.ctrl {
                self.history.redo(&mut self.scene);
                self.transform_cache.clear();
                self.bounds_cache.clear();
            }
        });

        super::ui::menu_bar::render_menu_bar(ctx, self);
        super::ui::hierarchy::render_hierarchy(ctx, self);
        super::ui::inspector::render_inspector(ctx, self);
        super::ui::console::render_console(ctx, self);
        super::ui::status_bar::render_status_bar(ctx, self);
        super::ui::dialogs::render_dialogs(ctx, self);

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            self.handle_viewport_input(ui, rect);

            if self.gpu_initialized && self.gpu_renderer.is_some() {
                self.render_gpu_viewport(ui, rect);
            } else {
                super::ui::viewport::render_viewport(ui, self);
            }
        });

        ctx.request_repaint();
    }
}
