// src/app.rs
use eframe::egui;
use egui::*;
use std::collections::VecDeque;

use crate::math::Vec3;
use crate::scene::{Scene, GameObject, ObjectType};
use crate::editor::{Gizmo, CommandHistory, EditorTool};
use crate::systems::*;
use crate::assets::AssetLibrary;
use crate::ui;
use crate::material::Material;

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
    pub cpu_render_limit: usize, // Лимит треугольников для CPU рендеринга
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::setup_egui_style(&cc.egui_ctx);
        let mut scene = Scene::new("Untitled");
        let cube_mesh = crate::mesh::Mesh::create_cube();
        let cube = GameObject::new("Cube", ObjectType::Mesh(crate::scene::MeshComponent {
            mesh: cube_mesh, material: Material::default(), visible: true, wireframe: false, solid: true, double_sided: false,
        }));
        scene.add_object(cube);

        let mut systems = Self {
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
            cpu_render_limit: 100000, // Рендерим до 100K треугольников на CPU
        };
        systems.console_messages.push_back(("🚀 Editor started".to_string(), Color32::GREEN));
        systems
    }

    pub fn log(&mut self, msg: &str, color: Color32) {
        self.console_messages.push_back((msg.to_string(), color));
        if self.console_messages.len() > 100 { self.console_messages.pop_front(); }
        self.status_message = msg.to_string();
    }

    pub fn orbit_camera(&mut self, delta_x: f32, delta_y: f32) {
        let dir = self.camera_position - self.camera_target;
        let radius = dir.length();
        let mut h_angle = dir.z.atan2(dir.x);
        let mut v_angle = (dir.y / radius).asin();
        h_angle += -delta_x * 0.01;
        v_angle = (v_angle + -delta_y * 0.01).clamp(-1.4, 1.4);
        self.camera_position = self.camera_target + Vec3::new(
            v_angle.cos() * h_angle.cos(),
            v_angle.sin(),
            v_angle.cos() * h_angle.sin()
        ) * radius;
    }

    pub fn pan_camera(&mut self, delta_x: f32, delta_y: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let speed = self.camera_position.length() * 0.001;
        let offset = right * (-delta_x * speed) + up * (delta_y * speed);
        self.camera_position = self.camera_position + offset;
        self.camera_target = self.camera_target + offset;
    }

    pub fn zoom_camera(&mut self, delta: f32) {
        let dir = (self.camera_target - self.camera_position).normalize();
        let distance = (self.camera_target - self.camera_position).length();
        let new_distance = (distance - delta * 0.5).clamp(2.0, 50.0);
        self.camera_position = self.camera_target - dir * new_distance;
    }

    pub fn world_to_screen(&self, world_pos: Vec3, rect: Rect) -> Option<Pos2> {
        let dir = (self.camera_target - self.camera_position).normalize();
        let right = dir.cross(self.camera_up).normalize();
        let up = right.cross(dir).normalize();
        let relative = world_pos - self.camera_position;
        let distance = relative.dot(dir);
        if distance <= 0.01 { return None; }
        let tan_fov = (self.camera_fov * std::f32::consts::PI / 180.0 / 2.0).tan();
        let scale = 1.0 / (distance * tan_fov);
        let x = relative.dot(right) * scale;
        let y = relative.dot(up) * scale;
        let center = rect.center();
        Some(Pos2::new(center.x + x * rect.width() * 0.5, center.y - y * rect.height() * 0.5))
    }

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect) {
        self.viewport_rect = rect;
        if !ui.rect_contains_pointer(rect) { return; }
        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
        let right = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let middle = ui.input(|i| i.pointer.button_down(PointerButton::Middle));
        let left = ui.input(|i| i.pointer.button_down(PointerButton::Primary));
        let shift = ui.input(|i| i.modifiers.shift);

        if right {
            if let (Some(cur), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                self.orbit_camera(cur.x - last.x, cur.y - last.y);
            }
        }
        if middle {
            if let (Some(cur), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                self.pan_camera(cur.x - last.x, cur.y - last.y);
            }
        }
        ui.input(|i| {
            if i.smooth_scroll_delta.y != 0.0 {
                self.zoom_camera(i.smooth_scroll_delta.y);
            }
        });

        if left && !self.left_mouse_pressed {
            if !shift { self.scene.selected_ids.clear(); }
            if let Some(_) = mouse_pos {
                let mut closest_id = None;
                let mut min_dist = f32::MAX;
                for (&id, _) in &self.scene.objects {
                    let p = self.scene.get_world_transform(id).position;
                    let d = (p - self.camera_position).length();
                    if d < min_dist { min_dist = d; closest_id = Some(id); }
                }
                if let Some(id) = closest_id { self.scene.select(id, true); }
            }
        }

        self.left_mouse_pressed = left;
        self.last_mouse_pos = mouse_pos;
        if let Some(obj) = self.scene.selected_objects().first() {
            self.gizmo.update(obj.transform);
            self.gizmo.visible = true;
        } else {
            self.gizmo.visible = false;
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let delta_time = (now - self.last_update_time) as f32;
        self.last_update_time = now;

        self.scene.update(delta_time);
        self.cinematic.update(delta_time);
        self.scripting.execute_scripts(delta_time);
        self.audio_system.update_listener(self.camera_position);
        self.world_streamer.update_streaming(self.camera_position);
        self.world_streamer.process_loading_queue();

        ctx.input(|i| {
            if i.key_pressed(Key::W) { self.current_tool = EditorTool::Move; }
            if i.key_pressed(Key::E) { self.current_tool = EditorTool::Rotate; }
            if i.key_pressed(Key::R) { self.current_tool = EditorTool::Scale; }
            if i.key_pressed(Key::Q) { self.current_tool = EditorTool::Select; }
            if i.key_pressed(Key::Delete) { self.scene.delete_selected(); self.scene.dirty = true; }
            if i.key_pressed(Key::Z) && i.modifiers.ctrl { self.history.undo(&mut self.scene); self.scene.dirty = true; }
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
            crate::ui::viewport::render_viewport(ui, self);
        });

        ctx.request_repaint();
    }
}