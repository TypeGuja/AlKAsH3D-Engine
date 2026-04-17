// editor/src/main.rs - ПОЛНОЦЕННЫЙ 3D РЕДАКТОР
use eframe::egui;
use egui::*;
use std::sync::Arc;
use parking_lot::Mutex;
use anyhow::Result;

mod render;
mod ui;
mod obj_converter;
mod blend_converter;

use render::{EditorRenderer, Transform};
use ui::{EditorState, SceneObject};

#[derive(Clone)]
struct Camera3D {
    position: Vec3,
    target: Vec3,
    up: Vec3,
    fov: f32,
    near: f32,
    far: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for Camera3D {
    fn default() -> Self {
        Self {
            position: Vec3::new(5.0, 5.0, 10.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov: 60.0,
            near: 0.1,
            far: 1000.0,
            yaw: -45.0,
            pitch: -30.0,
            distance: 12.0,
        }
    }
}

impl Camera3D {
    fn update_orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw += delta_x * 0.3;
        self.pitch = (self.pitch - delta_y * 0.3).clamp(-89.0, 89.0);
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();
        self.position.x = self.target.x + self.distance * yaw_rad.cos() * pitch_rad.cos();
        self.position.y = self.target.y + self.distance * pitch_rad.sin();
        self.position.z = self.target.z + self.distance * yaw_rad.sin() * pitch_rad.cos();
    }

    fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let forward = (self.target - self.position).normalized();
        let right = forward.cross(&self.up).normalized();
        let up = right.cross(&forward);
        let pan_speed = self.distance * 0.005;
        self.target += right * delta_x * pan_speed;
        self.target += up * delta_y * pan_speed;
        self.position += right * delta_x * pan_speed;
        self.position += up * delta_y * pan_speed;
    }

    fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance - delta * 2.0).clamp(2.0, 100.0);
        self.update_orbit(0.0, 0.0);
    }

    fn focus_on(&mut self, pos: Vec3) {
        self.target = pos;
        self.update_orbit(0.0, 0.0);
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum GizmoMode {
    Translate, Rotate, Scale, None,
}

struct Gizmo3D {
    mode: GizmoMode,
    dragging: bool,
}

impl Default for Gizmo3D {
    fn default() -> Self {
        Self { mode: GizmoMode::Translate, dragging: false }
    }
}

struct Grid3D {
    size: f32,
    spacing: f32,
    major_color: Color32,
    minor_color: Color32,
    axis_x_color: Color32,
    axis_z_color: Color32,
}

impl Default for Grid3D {
    fn default() -> Self {
        Self {
            size: 100.0,
            spacing: 1.0,
            major_color: Color32::from_rgb(60, 60, 70),
            minor_color: Color32::from_rgb(30, 30, 35),
            axis_x_color: Color32::from_rgb(200, 50, 50),
            axis_z_color: Color32::from_rgb(50, 50, 200),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Vec3 { x: f32, y: f32, z: f32 }

impl Vec3 {
    const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    const Y: Self = Self { x: 0.0, y: 1.0, z: 0.0 };

    fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }
    fn from_array(arr: [f32; 3]) -> Self { Self { x: arr[0], y: arr[1], z: arr[2] } }
    fn length(&self) -> f32 { (self.x * self.x + self.y * self.y + self.z * self.z).sqrt() }
    fn normalized(&self) -> Self {
        let len = self.length();
        if len > 0.0 { Self { x: self.x / len, y: self.y / len, z: self.z / len } } else { *self }
    }
    fn dot(&self, other: &Vec3) -> f32 { self.x * other.x + self.y * other.y + self.z * other.z }
    fn cross(&self, other: &Vec3) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z } }
}

impl std::ops::AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) { self.x += rhs.x; self.y += rhs.y; self.z += rhs.z; }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z } }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self { Self { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs } }
}

fn world_to_screen(pos: Vec3, camera: &Camera3D, screen_width: f32, screen_height: f32) -> Pos2 {
    let forward = (camera.target - camera.position).normalized();
    let right = forward.cross(&camera.up).normalized();
    let up = right.cross(&forward);
    let relative = pos - camera.position;
    let distance = relative.dot(&forward);

    if distance <= 0.01 {
        return Pos2::new(screen_width / 2.0, screen_height / 2.0);
    }

    let aspect = screen_width / screen_height;
    let tan_half_fov = (camera.fov * 0.5).to_radians().tan();
    let screen_x = relative.dot(&right) / (distance * tan_half_fov * aspect);
    let screen_y = -relative.dot(&up) / (distance * tan_half_fov);

    Pos2::new(
        screen_width / 2.0 + screen_x * screen_width / 2.0,
        screen_height / 2.0 + screen_y * screen_height / 2.0,
    )
}

struct EditorApp {
    state: EditorState,
    renderer: Arc<Mutex<EditorRenderer>>,
    viewport_rect: Rect,
    camera: Camera3D,
    grid: Grid3D,
    gizmo: Gizmo3D,
    is_hovering_viewport: bool,
    last_mouse_pos: Option<Pos2>,
    right_mouse_pressed: bool,
    middle_mouse_pressed: bool,
    show_stats: bool,
    status_message: String,
    available_meshes: Vec<String>,
    show_import_dialog: bool,
    import_path: String,
}

impl EditorApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let hwnd = 0usize;

        let renderer = EditorRenderer::new(hwnd, 1280, 720)
            .unwrap_or_else(|_| {
                println!("[Editor] Creating fallback renderer");
                EditorRenderer::new(0, 1280, 720).expect("Failed to create renderer")
            });

        let mut app = Self {
            state: EditorState::new(),
            renderer: Arc::new(Mutex::new(renderer)),
            viewport_rect: Rect::NOTHING,
            camera: Camera3D::default(),
            grid: Grid3D::default(),
            gizmo: Gizmo3D::default(),
            is_hovering_viewport: false,
            last_mouse_pos: None,
            right_mouse_pressed: false,
            middle_mouse_pressed: false,
            show_stats: true,
            status_message: String::from("Ready"),
            available_meshes: Vec::new(),
            show_import_dialog: false,
            import_path: String::new(),
        };

        if let Some(mut r) = app.renderer.try_lock() {
            if let Err(e) = r.init() {
                app.status_message = format!("Renderer init failed: {}", e);
                println!("[Editor] Renderer init failed: {}", e);
            } else {
                app.available_meshes = r.get_loaded_meshes();
                app.status_message = "Renderer ready".to_string();
            }
        }

        app
    }

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect) {
        self.is_hovering_viewport = ui.rect_contains_pointer(rect);
        if !self.is_hovering_viewport { return; }

        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
        let right_pressed = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let middle_pressed = ui.input(|i| i.pointer.button_down(PointerButton::Middle));
        let left_pressed = ui.input(|i| i.pointer.button_down(PointerButton::Primary));

        if right_pressed {
            if let (Some(current), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                let delta = current - last;
                self.camera.update_orbit(delta.x, delta.y);
            }
            self.right_mouse_pressed = true;
        } else {
            self.right_mouse_pressed = false;
        }

        if middle_pressed {
            if let (Some(current), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                let delta = current - last;
                self.camera.pan(-delta.x, delta.y);
            }
            self.middle_mouse_pressed = true;
        } else {
            self.middle_mouse_pressed = false;
        }

        ui.input(|i| {
            let scroll = i.smooth_scroll_delta.y;
            if scroll != 0.0 { self.camera.zoom(scroll * 0.1); }
        });

        if left_pressed && !self.gizmo.dragging && !self.right_mouse_pressed && !self.middle_mouse_pressed {
            self.pick_object(mouse_pos);
        }

        self.last_mouse_pos = mouse_pos;

        if self.right_mouse_pressed || self.middle_mouse_pressed {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
    }

    fn pick_object(&mut self, mouse_pos: Option<Pos2>) {
        if let Some(pos) = mouse_pos {
            let mut closest_dist = 50.0f32;
            let mut closest_id = None;

            for obj in &self.state.objects {
                let screen_pos = world_to_screen(
                    Vec3::from_array(obj.position),
                    &self.camera,
                    self.viewport_rect.width(),
                    self.viewport_rect.height(),
                );

                let dist = ((screen_pos.x - pos.x).powi(2) + (screen_pos.y - pos.y).powi(2)).sqrt();
                if dist < closest_dist {
                    closest_dist = dist;
                    closest_id = Some(obj.id);
                }
            }

            self.state.selected_object = closest_id;
        }
    }

    fn render_3d_viewport(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        self.viewport_rect = rect;

        self.handle_viewport_input(ui, rect);

        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(15, 15, 20));

        self.render_grid(ui, rect);
        self.render_objects(ui, rect);

        if self.state.selected_object.is_some() {
            self.render_gizmo(ui, rect);
        }

        self.render_viewport_overlay(ui, rect);

        if self.is_hovering_viewport {
            self.render_controls_hint(ui, rect);
        }
    }

    fn render_grid(&self, ui: &mut Ui, rect: Rect) {
        let grid_size = 100.0;
        let grid_lines = 50;
        let forward = (self.camera.target - self.camera.position).normalized();

        for i in -grid_lines..=grid_lines {
            let world_x = i as f32 * self.grid.spacing;
            let world_z = i as f32 * self.grid.spacing;

            let start_x = Vec3::new(world_x, 0.0, -grid_size);
            let end_x = Vec3::new(world_x, 0.0, grid_size);
            let mid_x = Vec3::new(world_x, 0.0, 0.0);

            if (mid_x - self.camera.position).dot(&forward) > -grid_size {
                let screen_start = world_to_screen(start_x, &self.camera, rect.width(), rect.height());
                let screen_end = world_to_screen(end_x, &self.camera, rect.width(), rect.height());

                if screen_start.x.is_finite() && screen_start.y.is_finite() &&
                    screen_end.x.is_finite() && screen_end.y.is_finite() {

                    let color = if i == 0 { self.grid.axis_z_color }
                    else if i % 10 == 0 { self.grid.major_color }
                    else { self.grid.minor_color };

                    ui.painter().line_segment([screen_start, screen_end], Stroke::new(1.0, color));
                }
            }

            let start_z = Vec3::new(-grid_size, 0.0, world_z);
            let end_z = Vec3::new(grid_size, 0.0, world_z);
            let mid_z = Vec3::new(0.0, 0.0, world_z);

            if (mid_z - self.camera.position).dot(&forward) > -grid_size {
                let screen_start = world_to_screen(start_z, &self.camera, rect.width(), rect.height());
                let screen_end = world_to_screen(end_z, &self.camera, rect.width(), rect.height());

                if screen_start.x.is_finite() && screen_start.y.is_finite() &&
                    screen_end.x.is_finite() && screen_end.y.is_finite() {

                    let color = if i == 0 { self.grid.axis_x_color }
                    else if i % 10 == 0 { self.grid.major_color }
                    else { self.grid.minor_color };

                    ui.painter().line_segment([screen_start, screen_end], Stroke::new(1.0, color));
                }
            }
        }

        let origin = Vec3::ZERO;
        let y_axis = Vec3::new(0.0, 5.0, 0.0);
        let screen_origin = world_to_screen(origin, &self.camera, rect.width(), rect.height());
        let screen_y = world_to_screen(y_axis, &self.camera, rect.width(), rect.height());
        ui.painter().line_segment([screen_origin, screen_y], Stroke::new(2.0, Color32::from_rgb(50, 200, 50)));
    }

    fn render_objects(&self, ui: &mut Ui, rect: Rect) {
        for obj in &self.state.objects {
            let world_pos = Vec3::from_array(obj.position);
            let screen_pos = world_to_screen(world_pos, &self.camera, rect.width(), rect.height());

            let forward = (self.camera.target - self.camera.position).normalized();
            let relative = world_pos - self.camera.position;
            if relative.dot(&forward) <= 0.0 { continue; }

            let color = match obj.object_type.as_str() {
                "camera" => Color32::from_rgb(0, 200, 200),
                "light" => Color32::from_rgb(255, 255, 100),
                "car" => Color32::from_rgb(200, 100, 0),
                "spawn" => Color32::from_rgb(200, 50, 200),
                _ => Color32::from_rgb(100, 200, 100),
            };

            let size = 10.0 * obj.scale[0];

            if Some(obj.id) == self.state.selected_object {
                ui.painter().circle_filled(screen_pos, size + 4.0, Color32::from_rgb(255, 255, 100));
                ui.painter().circle_stroke(screen_pos, size + 6.0, Stroke::new(2.0, Color32::WHITE));
            }

            ui.painter().circle_filled(screen_pos, size, color);
            ui.painter().circle_stroke(screen_pos, size, Stroke::new(1.5, Color32::WHITE));

            ui.painter().text(
                screen_pos + Vec2::new(size + 5.0, -5.0),
                Align2::LEFT_CENTER,
                &obj.name,
                FontId::proportional(11.0),
                Color32::LIGHT_GRAY,
            );

            let icon = match obj.object_type.as_str() {
                "camera" => "📷", "light" => "☀️", "car" => "🚗", "spawn" => "🚩", _ => "📦",
            };
            ui.painter().text(
                screen_pos + Vec2::new(-5.0, -size - 15.0),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(16.0),
                Color32::WHITE,
            );
        }
    }

    fn render_gizmo(&mut self, ui: &mut Ui, rect: Rect) {
        if let Some(selected_id) = self.state.selected_object {
            if let Some(obj) = self.state.objects.iter().find(|o| o.id == selected_id) {
                let pos = Vec3::from_array(obj.position);
                let screen_pos = world_to_screen(pos, &self.camera, rect.width(), rect.height());
                let gizmo_size = 50.0;

                let axis_x_end = pos + Vec3::new(gizmo_size / 30.0, 0.0, 0.0);
                let axis_y_end = pos + Vec3::new(0.0, gizmo_size / 30.0, 0.0);
                let axis_z_end = pos + Vec3::new(0.0, 0.0, gizmo_size / 30.0);

                let screen_x = world_to_screen(axis_x_end, &self.camera, rect.width(), rect.height());
                let screen_y = world_to_screen(axis_y_end, &self.camera, rect.width(), rect.height());
                let screen_z = world_to_screen(axis_z_end, &self.camera, rect.width(), rect.height());

                ui.painter().line_segment([screen_pos, screen_x], Stroke::new(3.0, Color32::RED));
                ui.painter().line_segment([screen_pos, screen_y], Stroke::new(3.0, Color32::GREEN));
                ui.painter().line_segment([screen_pos, screen_z], Stroke::new(3.0, Color32::BLUE));

                ui.painter().circle_filled(screen_x, 6.0, Color32::RED);
                ui.painter().circle_filled(screen_y, 6.0, Color32::GREEN);
                ui.painter().circle_filled(screen_z, 6.0, Color32::BLUE);
            }
        }
    }

    fn render_viewport_overlay(&self, ui: &mut Ui, rect: Rect) {
        let info = format!(
            "Camera: pos({:.1}, {:.1}, {:.1}) | Objects: {} | Meshes: {}",
            self.camera.position.x, self.camera.position.y, self.camera.position.z,
            self.state.objects.len(),
            self.available_meshes.len()
        );

        ui.painter().text(
            rect.left_top() + Vec2::new(10.0, 10.0),
            Align2::LEFT_TOP,
            info,
            FontId::proportional(11.0),
            Color32::LIGHT_GRAY,
        );

        ui.painter().text(
            rect.left_bottom() + Vec2::new(10.0, -20.0),
            Align2::LEFT_BOTTOM,
            &self.status_message,
            FontId::proportional(11.0),
            Color32::from_rgb(100, 200, 100),
        );
    }

    fn render_controls_hint(&self, ui: &mut Ui, rect: Rect) {
        let hints = [
            ("RMB - Orbit", Color32::WHITE),
            ("MMB - Pan", Color32::WHITE),
            ("Scroll - Zoom", Color32::WHITE),
            ("LMB - Select", Color32::WHITE),
            ("F - Focus", Color32::WHITE),
            ("W/E/R - Gizmo", Color32::WHITE),
            ("Del - Delete", Color32::WHITE),
        ];

        let mut y_offset = 50.0;
        for (text, color) in hints {
            ui.painter().text(
                rect.right_top() + Vec2::new(-120.0, y_offset),
                Align2::RIGHT_TOP,
                text,
                FontId::proportional(10.0),
                color,
            );
            y_offset += 18.0;
        }
    }

    fn import_altex_dialog(&mut self, ctx: &egui::Context) {
        if self.show_import_dialog {
            egui::Window::new("Import Altex Model")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Model path:");
                    ui.text_edit_singleline(&mut self.import_path);

                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Altex Model", &["altex"])
                            .pick_file() {
                            self.import_path = path.to_string_lossy().to_string();
                        }
                    }

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() {
                            if !self.import_path.is_empty() {
                                if let Some(mut r) = self.renderer.try_lock() {
                                    match r.load_altex(&self.import_path) {
                                        Ok(meshes) => {
                                            println!("[Editor] Loaded {} meshes", meshes.len());
                                            for (i, mesh_name) in meshes.iter().enumerate() {
                                                let id = self.state.objects.len() as u32;
                                                self.state.objects.push(SceneObject {
                                                    id,
                                                    name: mesh_name.clone(),
                                                    object_type: "mesh".to_string(),
                                                    position: [i as f32 * 10.0, 0.0, 0.0],
                                                    rotation: [0.0, 0.0, 0.0],
                                                    scale: [0.1, 0.1, 0.1], // Уменьшаем масштаб для больших моделей
                                                });
                                            }
                                            self.available_meshes = r.get_loaded_meshes();
                                            self.status_message = format!("Imported {} meshes", meshes.len());
                                        }
                                        Err(e) => {
                                            self.status_message = format!("Import failed: {}", e);
                                        }
                                    }
                                }
                            }
                            self.show_import_dialog = false;
                            self.import_path.clear();
                        }

                        if ui.button("Cancel").clicked() {
                            self.show_import_dialog = false;
                            self.import_path.clear();
                        }
                    });
                });
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.input(|i| {
            if i.key_pressed(Key::F) {
                if let Some(id) = self.state.selected_object {
                    if let Some(obj) = self.state.objects.iter().find(|o| o.id == id) {
                        self.camera.focus_on(Vec3::from_array(obj.position));
                    }
                }
            }

            if i.key_pressed(Key::W) { self.gizmo.mode = GizmoMode::Translate; }
            if i.key_pressed(Key::E) { self.gizmo.mode = GizmoMode::Rotate; }
            if i.key_pressed(Key::R) { self.gizmo.mode = GizmoMode::Scale; }
            if i.key_pressed(Key::Escape) {
                self.gizmo.mode = GizmoMode::None;
                self.state.selected_object = None;
            }
            if i.key_pressed(Key::Delete) {
                if let Some(id) = self.state.selected_object {
                    self.state.objects.retain(|o| o.id != id);
                    self.state.selected_object = None;
                }
            }
        });

        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::dark();
        ctx.set_style(style);

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("📁 File", |ui| {
                    if ui.button("📄 New Scene").clicked() {
                        self.state.objects.clear();
                        self.state.scene_name = "Untitled".to_string();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("📦 Import Altex...").clicked() {
                        self.show_import_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("📂 Import OBJ...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("OBJ Model", &["obj"])
                            .pick_file() {
                            let output = path.with_extension("altex");
                            match obj_converter::convert(path.to_str().unwrap(), output.to_str().unwrap()) {
                                Ok(_) => {
                                    if let Some(mut r) = self.renderer.try_lock() {
                                        if let Ok(meshes) = r.load_altex(output.to_str().unwrap()) {
                                            for (i, mesh_name) in meshes.iter().enumerate() {
                                                let id = self.state.objects.len() as u32;
                                                self.state.objects.push(SceneObject {
                                                    id, name: mesh_name.clone(), object_type: "mesh".to_string(),
                                                    position: [i as f32 * 10.0, 0.0, 0.0],
                                                    rotation: [0.0, 0.0, 0.0],
                                                    scale: [0.1, 0.1, 0.1],
                                                });
                                            }
                                            self.available_meshes = r.get_loaded_meshes();
                                            self.status_message = "OBJ imported".to_string();
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.status_message = format!("OBJ import failed: {}", e);
                                }
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("🎨 Import Blend...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Blender File", &["blend"])
                            .pick_file() {
                            let output = path.with_extension("altex");
                            match blend_converter::blend_to_altex(path.to_str().unwrap(), output.to_str().unwrap()) {
                                Ok(_) => {
                                    if let Some(mut r) = self.renderer.try_lock() {
                                        if let Ok(meshes) = r.load_altex(output.to_str().unwrap()) {
                                            for (i, mesh_name) in meshes.iter().enumerate() {
                                                let id = self.state.objects.len() as u32;
                                                self.state.objects.push(SceneObject {
                                                    id, name: mesh_name.clone(), object_type: "mesh".to_string(),
                                                    position: [i as f32 * 10.0, 0.0, 0.0],
                                                    rotation: [0.0, 0.0, 0.0],
                                                    scale: [0.1, 0.1, 0.1],
                                                });
                                            }
                                            self.available_meshes = r.get_loaded_meshes();
                                            self.status_message = "Blend imported".to_string();
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.status_message = format!("Blend import failed: {}", e);
                                }
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("❌ Exit").clicked() { std::process::exit(0); }
                });

                ui.menu_button("🎮 GameObject", |ui| {
                    if ui.button("📦 Empty Mesh").clicked() {
                        let id = self.state.objects.len() as u32;
                        self.state.objects.push(SceneObject {
                            id, name: format!("Mesh_{}", id), object_type: "mesh".to_string(),
                            position: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], scale: [1.0, 1.0, 1.0],
                        });
                        ui.close_menu();
                    }
                    if ui.button("☀️ Light").clicked() {
                        let id = self.state.objects.len() as u32;
                        self.state.objects.push(SceneObject {
                            id, name: format!("Light_{}", id), object_type: "light".to_string(),
                            position: [0.0, 5.0, 0.0], rotation: [0.0, 0.0, 0.0], scale: [1.0, 1.0, 1.0],
                        });
                        ui.close_menu();
                    }
                    if ui.button("📷 Camera").clicked() {
                        let id = self.state.objects.len() as u32;
                        self.state.objects.push(SceneObject {
                            id, name: format!("Camera_{}", id), object_type: "camera".to_string(),
                            position: [0.0, 5.0, 10.0], rotation: [0.0, 0.0, 0.0], scale: [1.0, 1.0, 1.0],
                        });
                        ui.close_menu();
                    }
                    if ui.button("🚩 Spawn Point").clicked() {
                        let id = self.state.objects.len() as u32;
                        self.state.objects.push(SceneObject {
                            id, name: format!("Spawn_{}", id), object_type: "spawn".to_string(),
                            position: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], scale: [1.0, 1.0, 1.0],
                        });
                        ui.close_menu();
                    }
                });

                ui.add_space(20.0);

                if self.state.play_mode {
                    if ui.button("⏹ Stop").clicked() { self.state.play_mode = false; }
                    ui.colored_label(Color32::GREEN, "● PLAYING");
                } else {
                    if ui.button("▶ Play").clicked() { self.state.play_mode = true; }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("FPS: {:.1}", self.state.fps));
                });
            });
        });

        egui::SidePanel::left("hierarchy")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📁 HIERARCHY");
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut objects_to_delete = Vec::new();
                    let objects = self.state.objects.clone();

                    for obj in &objects {
                        let icon = match obj.object_type.as_str() {
                            "camera" => "📷", "light" => "☀️", "car" => "🚗", "spawn" => "🚩", _ => "📦",
                        };

                        let selected = Some(obj.id) == self.state.selected_object;
                        let response = ui.selectable_label(selected, format!("{} {}", icon, obj.name));

                        if response.clicked() {
                            self.state.selected_object = Some(obj.id);
                        }

                        if response.double_clicked() {
                            self.camera.focus_on(Vec3::from_array(obj.position));
                        }

                        response.context_menu(|ui| {
                            if ui.button("🗑️ Delete").clicked() {
                                objects_to_delete.push(obj.id);
                                ui.close_menu();
                            }
                            if ui.button("🎯 Focus").clicked() {
                                self.camera.focus_on(Vec3::from_array(obj.position));
                                ui.close_menu();
                            }
                        });
                    }

                    for id in objects_to_delete {
                        self.state.objects.retain(|o| o.id != id);
                        if self.state.selected_object == Some(id) {
                            self.state.selected_object = None;
                        }
                    }
                });

                ui.separator();
                if ui.button("➕ Add Object").clicked() {
                    let id = self.state.objects.len() as u32;
                    self.state.objects.push(SceneObject {
                        id, name: format!("Object_{}", id), object_type: "mesh".to_string(),
                        position: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], scale: [1.0, 1.0, 1.0],
                    });
                }
            });

        egui::SidePanel::right("inspector")
            .default_width(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📋 INSPECTOR");
                ui.separator();

                if let Some(selected_id) = self.state.selected_object {
                    if let Some(obj) = self.state.objects.iter_mut().find(|o| o.id == selected_id) {
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut obj.name);
                        });

                        ui.label("🔧 TRANSFORM");

                        ui.horizontal(|ui| {
                            ui.label("Position:");
                            ui.add(egui::DragValue::new(&mut obj.position[0]).speed(0.1).prefix("X "));
                            ui.add(egui::DragValue::new(&mut obj.position[1]).speed(0.1).prefix("Y "));
                            ui.add(egui::DragValue::new(&mut obj.position[2]).speed(0.1).prefix("Z "));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Rotation:");
                            ui.add(egui::DragValue::new(&mut obj.rotation[0]).speed(1.0).prefix("X "));
                            ui.add(egui::DragValue::new(&mut obj.rotation[1]).speed(1.0).prefix("Y "));
                            ui.add(egui::DragValue::new(&mut obj.rotation[2]).speed(1.0).prefix("Z "));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Scale:");
                            ui.add(egui::DragValue::new(&mut obj.scale[0]).speed(0.1).prefix("X "));
                            ui.add(egui::DragValue::new(&mut obj.scale[1]).speed(0.1).prefix("Y "));
                            ui.add(egui::DragValue::new(&mut obj.scale[2]).speed(0.1).prefix("Z "));
                        });

                        ui.separator();

                        if ui.button("🔄 Reset Transform").clicked() {
                            obj.position = [0.0, 0.0, 0.0];
                            obj.rotation = [0.0, 0.0, 0.0];
                            obj.scale = [1.0, 1.0, 1.0];
                        }
                    }
                } else {
                    ui.label("No object selected");
                }
            });

        egui::TopBottomPanel::bottom("status_bar")
            .default_height(26.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.state.play_mode {
                        ui.colored_label(Color32::GREEN, "● PLAYING");
                    } else {
                        ui.label("🎮 Edit Mode");
                    }
                    ui.separator();
                    ui.label(format!("Scene: {}", self.state.scene_name));
                    ui.add_space(20.0);
                    ui.label(format!("Gizmo: {:?}", self.gizmo.mode));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(&self.status_message);
                    });
                });
            });

        // РЕНДЕРИНГ 3D МЕШЕЙ
        if let Some(mut r) = self.renderer.try_lock() {
            r.begin_frame();

            for obj in &self.state.objects {
                if obj.object_type == "mesh" && self.available_meshes.contains(&obj.name) {
                    let transform = Transform {
                        position: obj.position,
                        rotation: [0.0, 0.0, 0.0, 1.0],
                        scale: obj.scale,
                    };
                    r.render_mesh(&obj.name, &transform);
                }
            }

            r.end_frame();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_3d_viewport(ui);
        });

        self.import_altex_dialog(ctx);

        self.state.fps = 1.0 / ctx.input(|i| i.stable_dt).max(0.001);
        ctx.request_repaint();
    }
}

fn main() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_title("AlKAsH3D Editor")
            .with_min_inner_size([1000.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "AlKAsH3D Editor",
        options,
        Box::new(|cc| Box::new(EditorApp::new(cc)))
    ).map_err(|e| anyhow::anyhow!("Failed to run editor: {}", e))
}