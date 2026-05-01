// src/app.rs
use eframe::egui;
use egui::*;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Arc;

use crate::math::Vec3;
use crate::scene::{Scene, GameObject, ObjectType, MeshComponent};
use crate::editor::{Gizmo, CommandHistory, EditorTool};
use crate::systems::*;
use crate::assets::AssetLibrary;
use crate::ui;
use crate::material::Material;
use crate::mesh::Mesh;
use crate::gpu;

pub struct EditorApp {
    pub scene: Scene,
    pub history: CommandHistory,
    pub asset_library: AssetLibrary,
    pub camera_position: Vec3,
    pub camera_target: Vec3,
    pub camera_up: Vec3,
    pub camera_fov: f32,
    pub use_gpu: bool,
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
    pub gpu_window: Option<Arc<winit::window::Window>>,
    pub gpu_renderer: Option<gpu::renderer::GpuRenderer>,
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

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::setup_egui_style(&cc.egui_ctx);
        let mut scene = Scene::new("Untitled");
        let cube_mesh = crate::mesh::Mesh::create_cube();
        let cube = GameObject::new("Cube", ObjectType::Mesh(MeshComponent {
            mesh: cube_mesh, material: Material::default(), visible: true, wireframe: false, solid: true, double_sided: false,
        }));
        scene.add_object(cube);

        let mut app = Self {
            scene, history: CommandHistory::new(100), asset_library: AssetLibrary::new(),
            camera_position: Vec3::new(5.0, 5.0, 10.0), camera_target: Vec3::ZERO, camera_up: Vec3::UP, camera_fov: 60.0,
            use_gpu: false,
            current_tool: EditorTool::Select, gizmo: Gizmo::default(), viewport_rect: Rect::NOTHING,
            show_hierarchy: true, show_inspector: true, show_console: true, show_new_scene_dialog: false, show_import_dialog: false,
            new_scene_name: String::from("New Scene"), search_filter: String::new(),
            last_mouse_pos: None, left_mouse_pressed: false, right_mouse_pressed: false, middle_mouse_pressed: false,
            status_message: String::from("Ready"), fps: 0.0, frame_count: 0, last_frame_time: 0.0, last_update_time: 0.0,
            console_messages: VecDeque::new(),
            world_streamer: WorldStreamer::new(), material_accel: MaterialAccelerator::new(), shader_manager: ShaderManager::new(),
            audio_system: SpatialAudioSystem::new(), scripting: ScriptingEngine::new(), cinematic: CinematicManager::new(),
            cpu_render_limit: 5000,
            pending_imports: Vec::new(),
            import_progress: 0.0,
            gpu_window: None,
            gpu_renderer: None,
        };
        app.log("🚀 Editor started", Color32::GREEN);
        app
    }

    pub fn log(&mut self, msg: &str, color: Color32) {
        self.console_messages.push_back((msg.to_string(), color));
        if self.console_messages.len() > 100 {
            self.console_messages.pop_front();
        }
        self.status_message = msg.to_string();
    }

    pub fn init_gpu(&mut self) {
        use winit::event_loop::EventLoop;

        let event_loop = EventLoop::new().unwrap();

        #[allow(deprecated)]
        let window = Arc::new(
            event_loop.create_window(
                winit::window::WindowAttributes::default()
                    .with_title("GPU Viewport")
                    .with_inner_size(winit::dpi::LogicalSize::new(800, 600))
            ).unwrap()
        );

        let mut renderer = pollster::block_on(gpu::renderer::GpuRenderer::new(window.clone())).unwrap();

        for obj in self.scene.objects.values() {
            if let ObjectType::Mesh(ref m) = obj.object_type {
                renderer.add_mesh(&m.mesh);
            }
        }

        self.gpu_window = Some(window);
        self.gpu_renderer = Some(renderer);
        self.use_gpu = true;
        self.log("✅ GPU renderer initialized!", Color32::GREEN);
    }

    pub fn import_model_async(&mut self, path: &str) {
        let path_owned = path.to_string();
        let (tx, rx) = mpsc::channel();
        let path_clone = path_owned.clone();
        std::thread::spawn(move || {
            let mut lib = AssetLibrary::new();
            match lib.import_model(&path_clone) {
                Ok(names) => {
                    let mut meshes = Vec::new();
                    for name in &names { if let Some(m) = lib.get_mesh(name) { meshes.push((name.clone(), m.clone())); } }
                    let _ = tx.send(Ok(ImportResult { mesh_names: names, meshes }));
                }
                Err(e) => { let _ = tx.send(Err(e)); }
            }
        });
        self.pending_imports.push(PendingImport { path: path_owned, receiver: rx });
        self.log(&format!("📥 Importing: {}...", path), Color32::YELLOW);
    }

    fn check_pending_imports(&mut self) {
        if self.pending_imports.is_empty() { return; }
        let mut results = Vec::new();
        let mut completed = Vec::new();
        for (i, imp) in self.pending_imports.iter().enumerate() {
            if let Ok(r) = imp.receiver.try_recv() { completed.push(i); results.push(r); }
        }
        for &i in completed.iter().rev() { self.pending_imports.remove(i); }
        for result in results {
            match result {
                Ok(ir) => {
                    let mut total_tris = 0;
                    for (name, mesh) in ir.meshes {
                        let tris = mesh.indices.len() / 3;
                        let verts = mesh.vertices.len();
                        total_tris += tris;

                        self.asset_library.meshes.insert(name.clone(), mesh.clone());

                        // Определяем режим отображения
                        let is_large = tris > 5000;

                        let obj = GameObject::new(&name, ObjectType::Mesh(MeshComponent {
                            mesh: mesh.clone(),
                            material: Material {
                                name: format!("{}_mat", name),
                                color: [0.7, 0.7, 0.7, 1.0],
                                ..Default::default()
                            },
                            visible: true,
                            wireframe: is_large,  // Большие - wireframe
                            solid: !is_large,      // Маленькие - solid
                            double_sided: false,
                        }));
                        self.scene.add_object(obj);

                        if let Some(ref mut r) = self.gpu_renderer {
                            r.add_mesh(&mesh);
                        }

                        self.log(&format!("✅ {} ({}K tris, {}K verts)", name, tris/1000, verts/1000),
                                 if is_large { Color32::YELLOW } else { Color32::GREEN });
                    }
                    self.log(&format!("✅ Import complete: {}K total tris", total_tris/1000), Color32::GREEN);
                    self.show_import_dialog = false;
                }
                Err(e) => { self.log(&format!("❌ {}", e), Color32::RED); }
            }
        }
    }

    pub fn orbit_camera(&mut self, dx: f32, dy: f32) {
        let dir = self.camera_position - self.camera_target;
        let r = dir.length(); if r < 0.01 { return; }
        let mut ha = dir.z.atan2(dir.x); let mut va = (dir.y/r).asin();
        ha += -dx * 0.01; va = (va + -dy * 0.01).clamp(-1.4, 1.4);
        self.camera_position = self.camera_target + Vec3::new(va.cos()*ha.cos(), va.sin(), va.cos()*ha.sin()) * r;
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
        let dist = rel.dot(dir); if dist <= 0.01 { return None; }
        let tf = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();
        let scale = 1.0 / (dist * tf);
        let x = rel.dot(right) * scale; let y = rel.dot(up) * scale;
        let c = rect.center();
        Some(Pos2::new(c.x + x * rect.width() * 0.5, c.y - y * rect.height() * 0.5))
    }

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect) {
        self.viewport_rect = rect;
        if !ui.rect_contains_pointer(rect) { return; }
        let mp = ui.input(|i| i.pointer.hover_pos());
        let left = ui.input(|i| i.pointer.button_down(PointerButton::Primary));
        let right = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let middle = ui.input(|i| i.pointer.button_down(PointerButton::Middle));
        let shift = ui.input(|i| i.modifiers.shift);

        if right { if let (Some(c), Some(l)) = (mp, self.last_mouse_pos) { self.orbit_camera(c.x-l.x, c.y-l.y); } }
        if middle || (right && shift) { if let (Some(c), Some(l)) = (mp, self.last_mouse_pos) { self.pan_camera(c.x-l.x, c.y-l.y); } }
        ui.input(|i| { if i.smooth_scroll_delta.y != 0.0 { self.zoom_camera(i.smooth_scroll_delta.y); } });

        if left && !self.left_mouse_pressed {
            if !shift { self.scene.selected_ids.clear(); }
            let mut cid = None; let mut md = f32::MAX;
            for (&id, _) in &self.scene.objects { let p = self.scene.get_world_transform(id).position; let d = (p-self.camera_position).length(); if d < md { md = d; cid = Some(id); } }
            if let Some(id) = cid { self.scene.select(id, true); }
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
            if i.key_pressed(Key::Delete) { self.scene.delete_selected(); }
            if i.key_pressed(Key::Z) && i.modifiers.ctrl { self.history.undo(&mut self.scene); }
            if i.key_pressed(Key::F5) { self.init_gpu(); }
        });

        self.frame_count += 1;
        if now - self.last_frame_time > 1.0 { self.fps = self.frame_count as f32; self.frame_count = 0; self.last_frame_time = now; }

        // Рендерим GPU
        if let Some(ref mut r) = self.gpu_renderer {
            r.camera.position = self.camera_position;
            r.camera.target = self.camera_target;
            r.camera.up = self.camera_up;
            let _ = r.render();
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
            crate::ui::viewport::render_viewport(ui, self);
        });
        ctx.request_repaint();
    }
}