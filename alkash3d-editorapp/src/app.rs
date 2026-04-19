//! Главное состояние приложения

use eframe::egui;
use egui::*;
use std::sync::Arc;
use parking_lot::Mutex;
use uuid::Uuid;

use crate::math::{Camera, Vec3, Vec2, Ray, AABB};
use crate::scene::{SceneManager, GameObject};
use crate::render::RenderEngine;
use crate::editor::{Gizmo, GizmoMode, GizmoSpace};
use crate::ui::*;
use crate::assets::AssetLibrary;

pub struct EditorApp {
    // Сцена
    pub scene_manager: SceneManager,

    // Рендерер
    pub renderer: Arc<Mutex<RenderEngine>>,

    // Камера редактора
    pub camera: Camera,

    // Редактор
    pub gizmo: Gizmo,
    pub current_tool: EditorTool,

    // Ассеты
    pub asset_library: AssetLibrary,

    // UI
    pub viewport_rect: Rect,
    pub show_grid: bool,
    pub show_stats: bool,
    pub show_gizmo: bool,

    // Навигация
    pub is_navigating: bool,
    pub last_mouse_pos: Option<Pos2>,
    pub right_mouse_pressed: bool,
    pub middle_mouse_pressed: bool,
    pub left_mouse_pressed: bool,

    // Состояние
    pub status_message: String,
    pub fps: f32,
    pub frame_count: u64,
    pub last_frame_time: f64,
    pub renderer_initialized: bool,

    // Диалоги
    pub show_import_dialog: bool,
    pub show_new_scene_dialog: bool,
    pub show_settings_dialog: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    Select,
    Move,
    Rotate,
    Scale,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Настраиваем стиль
        setup_egui_style(&cc.egui_ctx);

        let mut scene_manager = SceneManager::new();
        scene_manager.new_scene("Untitled");

        // Создаем тестовую сцену
        if let Some(scene) = &mut scene_manager.current_scene {
            // Добавляем камеру
            let mut camera_obj = GameObject::new("Main Camera");
            camera_obj.transform.translation = Vec3::new(5.0, 5.0, 10.0);
            camera_obj.transform.look_at(Vec3::ZERO, Vec3::Y);

            camera_obj.components.push(crate::scene::Component::Camera(
                crate::scene::CameraComponent::default()
            ));

            let camera_id = scene.add_object(camera_obj);
            scene.main_camera = Some(camera_id);

            // Добавляем источник света
            let mut light_obj = GameObject::new("Directional Light");
            light_obj.transform.translation = Vec3::new(5.0, 10.0, 5.0);
            light_obj.components.push(crate::scene::Component::Light(
                crate::scene::LightComponent {
                    light_type: crate::scene::LightType::Directional,
                    color: Vec3::new(1.0, 0.95, 0.9),
                    intensity: 1.0,
                    range: 100.0,
                    cast_shadows: true,
                    enabled: true,
                }
            ));
            scene.add_object(light_obj);

            // Добавляем тестовые объекты
            scene.add_object(GameObject::new("Ground").with_mesh("plane"));
            scene.add_object(GameObject::new("Cube").with_mesh("cube"));
            scene.add_object(GameObject::new("Sphere").with_mesh("sphere"));
        }

        let renderer = Arc::new(Mutex::new(RenderEngine::new(1280, 720, false)));

        Self {
            scene_manager,
            renderer,
            camera: Camera::new(1280.0 / 720.0),
            gizmo: Gizmo::default(),
            current_tool: EditorTool::Select,
            asset_library: AssetLibrary::new(),
            viewport_rect: Rect::NOTHING,
            show_grid: true,
            show_stats: true,
            show_gizmo: true,
            is_navigating: false,
            last_mouse_pos: None,
            right_mouse_pressed: false,
            middle_mouse_pressed: false,
            left_mouse_pressed: false,
            status_message: String::from("Ready"),
            fps: 0.0,
            frame_count: 0,
            last_frame_time: 0.0,
            renderer_initialized: false,
            show_import_dialog: false,
            show_new_scene_dialog: false,
            show_settings_dialog: false,
        }
    }

    pub fn init_renderer(&mut self, hwnd: usize) {
        if let Some(mut renderer) = self.renderer.try_lock() {
            match renderer.init(hwnd) {
                Ok(_) => {
                    self.status_message = "Renderer ready".to_string();
                    self.renderer_initialized = true;
                    println!("[Editor] Renderer initialized successfully");
                }
                Err(e) => {
                    self.status_message = format!("Renderer init failed: {}", e);
                    println!("[Editor] Renderer init failed: {}", e);
                }
            }
        }
    }

    fn handle_viewport_input(&mut self, ui: &mut Ui, rect: Rect) {
        self.viewport_rect = rect;

        let is_hovering = ui.rect_contains_pointer(rect);
        if !is_hovering {
            return;
        }

        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
        let right_pressed = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let middle_pressed = ui.input(|i| i.pointer.button_down(PointerButton::Middle));
        let left_pressed = ui.input(|i| i.pointer.button_down(PointerButton::Primary));

        // Навигация камеры
        if right_pressed {
            if let (Some(current), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                let delta = current - last;
                let target = if let Some(scene) = &self.scene_manager.current_scene {
                    scene.get_selected().first()
                        .map(|obj| obj.world_position())
                        .unwrap_or(Vec3::ZERO)
                } else {
                    Vec3::ZERO
                };
                self.camera.orbit(Vec2::new(delta.x, delta.y), target);
            }
            self.right_mouse_pressed = true;
        } else {
            self.right_mouse_pressed = false;
        }

        if middle_pressed {
            if let (Some(current), Some(last)) = (mouse_pos, self.last_mouse_pos) {
                let delta = current - last;
                self.camera.pan(Vec2::new(delta.x, delta.y));
            }
            self.middle_mouse_pressed = true;
        } else {
            self.middle_mouse_pressed = false;
        }

        ui.input(|i| {
            let scroll = i.smooth_scroll_delta.y;
            if scroll != 0.0 {
                self.camera.zoom(scroll);
            }
        });

        // Гизмо и выделение
        if let Some(pos) = mouse_pos {
            let ray = Ray::from_screen(
                Vec2::new(pos.x - rect.min.x, pos.y - rect.min.y),
                Vec2::new(rect.width(), rect.height()),
                &self.camera,
            );

            if left_pressed && !self.gizmo.dragging {
                if !self.gizmo.begin_drag(pos, ray, self.camera.transform.translation) {
                    self.select_object(ray, ui);
                }
            } else if !left_pressed && self.gizmo.dragging {
                self.gizmo.end_drag();
            }

            if self.gizmo.dragging {
                self.gizmo.drag(pos, ray);
                if let Some(scene) = &mut self.scene_manager.current_scene {
                    for obj in scene.get_selected_mut() {
                        self.gizmo.apply_transform(&mut obj.transform);
                    }
                    scene.mark_dirty();
                }
            }
        }

        self.left_mouse_pressed = left_pressed;
        self.last_mouse_pos = mouse_pos;

        // Обновляем позицию гизмо
        if let Some(scene) = &self.scene_manager.current_scene {
            if let Some(first_selected) = scene.get_selected().first() {
                self.gizmo.update(first_selected.transform);
                self.gizmo.visible = true;
            } else {
                self.gizmo.visible = false;
            }
        }

        // Обновляем курсор
        if right_pressed || middle_pressed {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if self.gizmo.dragging {
            ui.ctx().set_cursor_icon(CursorIcon::Move);
        }
    }

    fn select_object(&mut self, ray: Ray, ui: &mut Ui) {
        if let Some(scene) = &mut self.scene_manager.current_scene {
            let mut closest_dist = f32::MAX;
            let mut closest_id = None;

            // Собираем bounds объектов
            let objects_info: Vec<(Uuid, AABB)> = scene.objects.iter()
                .map(|(id, obj)| {
                    let size = obj.transform.scale;
                    let half = size * 0.5;
                    let bounds = AABB::new(
                        obj.transform.translation - half,
                        obj.transform.translation + half,
                    );
                    (*id, bounds)
                })
                .collect();

            // Проверяем пересечения
            for (id, bounds) in objects_info {
                if let Some(t) = ray.intersect_aabb(bounds.min, bounds.max) {
                    if t < closest_dist {
                        closest_dist = t;
                        closest_id = Some(id);
                    }
                }
            }

            let add = ui.input(|i| i.modifiers.shift);

            if let Some(id) = closest_id {
                scene.select(id, add);
            } else if !add {
                scene.clear_selection();
            }
        }
    }

    fn render_viewport(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();

        // Обработка ввода
        self.handle_viewport_input(ui, rect);

        // Фон
        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(25, 25, 30));

        // Отладочный текст
        if !self.renderer_initialized {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "3D Viewport\n(Initializing renderer...)",
                FontId::proportional(20.0),
                Color32::from_rgb(150, 150, 150),
            );
        }

        // Рендеринг через движок
        if self.renderer_initialized {
            if let Some(mut renderer) = self.renderer.try_lock() {
                renderer.begin_frame(&self.camera);

                if let Some(scene) = &self.scene_manager.current_scene {
                    for obj in scene.objects.values() {
                        if obj.visible {
                            if let Some(mesh) = obj.get_component::<crate::scene::MeshRendererComponent>() {
                                if mesh.visible {
                                    let transform = obj.world_transform();
                                    renderer.render_mesh(
                                        &mesh.asset_id,
                                        &transform,
                                        &self.camera,
                                        mesh.wireframe,
                                    );
                                }
                            }
                        }
                    }
                }

                renderer.end_frame();
            }
        }

        // UI поверх вьюпорта
        self.render_viewport_overlay(ui, rect);
    }

    fn render_viewport_overlay(&self, ui: &mut Ui, rect: Rect) {
        // Информация о камере
        let info = format!(
            "Camera: ({:.1}, {:.1}, {:.1}) | Tool: {:?} | Objects: {}",
            self.camera.transform.translation.x,
            self.camera.transform.translation.y,
            self.camera.transform.translation.z,
            self.current_tool,
            self.scene_manager.current_scene.as_ref().map(|s| s.objects.len()).unwrap_or(0)
        );

        ui.painter().text(
            egui::pos2(rect.min.x + 10.0, rect.min.y + 10.0),
            Align2::LEFT_TOP,
            info,
            FontId::proportional(12.0),
            Color32::LIGHT_GRAY,
        );

        // Статус
        let status_color = if self.renderer_initialized {
            Color32::from_rgb(100, 200, 100)
        } else {
            Color32::from_rgb(200, 150, 50)
        };

        ui.painter().text(
            egui::pos2(rect.min.x + 10.0, rect.max.y - 10.0),
            Align2::LEFT_BOTTOM,
            &self.status_message,
            FontId::proportional(11.0),
            status_color,
        );

        // Подсказки управления
        let hints = [
            ("RMB - Orbit", Color32::WHITE),
            ("MMB - Pan", Color32::WHITE),
            ("Scroll - Zoom", Color32::WHITE),
            ("LMB - Select", Color32::WHITE),
            ("W - Move", Color32::WHITE),
            ("E - Rotate", Color32::WHITE),
            ("R - Scale", Color32::WHITE),
            ("F - Focus", Color32::WHITE),
            ("Del - Delete", Color32::WHITE),
        ];

        let mut y_offset = 50.0;
        for (text, color) in hints {
            ui.painter().text(
                egui::pos2(rect.max.x - 120.0, rect.min.y + y_offset),
                Align2::RIGHT_TOP,
                text,
                FontId::proportional(10.0),
                color,
            );
            y_offset += 18.0;
        }

        // Индикатор гизмо
        if self.show_gizmo && self.gizmo.visible {
            let gizmo_text = match self.gizmo.mode {
                GizmoMode::Translate => "Gizmo: Translate",
                GizmoMode::Rotate => "Gizmo: Rotate",
                GizmoMode::Scale => "Gizmo: Scale",
                GizmoMode::None => "",
            };
            if !gizmo_text.is_empty() {
                ui.painter().text(
                    egui::pos2(rect.max.x - 120.0, rect.max.y - 30.0),
                    Align2::RIGHT_BOTTOM,
                    gizmo_text,
                    FontId::proportional(11.0),
                    Color32::from_rgb(255, 200, 100),
                );
            }
        }
    }

    fn render_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Scene").clicked() {
                        self.show_new_scene_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("Open Scene...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("AlKAsH3D Scene", &["alscene"])
                            .pick_file() {
                            if let Err(e) = self.scene_manager.load_scene(path.to_str().unwrap()) {
                                self.status_message = format!("Failed to load: {}", e);
                            } else {
                                self.status_message = "Scene loaded".to_string();
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("Save Scene").clicked() {
                        if let Some(scene) = &self.scene_manager.current_scene {
                            let path_to_save = scene.path.clone();
                            if let Some(path) = path_to_save {
                                if let Err(e) = self.scene_manager.save_scene(&path) {
                                    self.status_message = format!("Failed to save: {}", e);
                                } else {
                                    self.status_message = "Scene saved".to_string();
                                }
                            } else if let Some(path) = rfd::FileDialog::new()
                                .add_filter("AlKAsH3D Scene", &["alscene"])
                                .save_file() {
                                if let Err(e) = self.scene_manager.save_scene(path.to_str().unwrap()) {
                                    self.status_message = format!("Failed to save: {}", e);
                                } else {
                                    self.status_message = "Scene saved".to_string();
                                }
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Import Model...").clicked() {
                        self.show_import_dialog = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        std::process::exit(0);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Redo").clicked() {
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Duplicate").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.duplicate_selected();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Delete").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.delete_selected();
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("GameObject", |ui| {
                    if ui.button("Create Empty").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.add_object(GameObject::new("GameObject"));
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Cube").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.add_object(GameObject::new("Cube").with_mesh("cube"));
                        }
                        ui.close_menu();
                    }
                    if ui.button("Sphere").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.add_object(GameObject::new("Sphere").with_mesh("sphere"));
                        }
                        ui.close_menu();
                    }
                    if ui.button("Plane").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.add_object(GameObject::new("Plane").with_mesh("plane"));
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Light").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            scene.add_object(GameObject::new("Light")
                                .with_light(crate::scene::LightType::Point));
                        }
                        ui.close_menu();
                    }
                    if ui.button("Camera").clicked() {
                        if let Some(scene) = &mut self.scene_manager.current_scene {
                            let mut cam_obj = GameObject::new("Camera");
                            cam_obj.components.push(crate::scene::Component::Camera(
                                crate::scene::CameraComponent::default()
                            ));
                            scene.add_object(cam_obj);
                        }
                        ui.close_menu();
                    }
                });

                ui.add_space(20.0);

                ui.selectable_value(&mut self.current_tool, EditorTool::Select, "Select");
                ui.selectable_value(&mut self.current_tool, EditorTool::Move, "Move");
                ui.selectable_value(&mut self.current_tool, EditorTool::Rotate, "Rotate");
                ui.selectable_value(&mut self.current_tool, EditorTool::Scale, "Scale");

                self.gizmo.mode = match self.current_tool {
                    EditorTool::Move => GizmoMode::Translate,
                    EditorTool::Rotate => GizmoMode::Rotate,
                    EditorTool::Scale => GizmoMode::Scale,
                    _ => GizmoMode::None,
                };

                ui.separator();

                ui.selectable_value(&mut self.gizmo.space, GizmoSpace::World, "World");
                ui.selectable_value(&mut self.gizmo.space, GizmoSpace::Local, "Local");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("FPS: {:.1}", self.fps));
                });
            });
        });
    }

    fn render_hierarchy(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("hierarchy")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Hierarchy");
                ui.separator();

                if let Some(scene) = &mut self.scene_manager.current_scene {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let objects: Vec<(Uuid, String, bool)> = scene.objects.iter()
                            .map(|(id, obj)| (*id, obj.name.clone(), scene.selection.contains(id)))
                            .collect();

                        for (id, name, selected) in objects {
                            let response = ui.selectable_label(selected, &name);

                            if response.clicked() {
                                scene.select(id, ui.input(|i| i.modifiers.shift));
                            }

                            response.context_menu(|ui| {
                                if ui.button("Delete").clicked() {
                                    scene.remove_object(id);
                                    ui.close_menu();
                                }
                                if ui.button("Duplicate").clicked() {
                                    ui.close_menu();
                                }
                                if ui.button("Focus").clicked() {
                                    if let Some(obj) = scene.get_object(id) {
                                        self.camera.focus_on(obj.world_position());
                                    }
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                }
            });
    }

    fn render_inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector")
            .default_width(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Inspector");
                ui.separator();

                if let Some(scene) = &mut self.scene_manager.current_scene {
                    let selected_ids: Vec<Uuid> = scene.selection.iter().copied().collect();

                    if selected_ids.len() == 1 {
                        let id = selected_ids[0];
                        let mut components_to_remove = Vec::new();
                        let mut name_changed = false;
                        let mut transform_changed = false;

                        if let Some(obj) = scene.get_object_mut(id) {
                            let old_name = obj.name.clone();

                            ui.horizontal(|ui| {
                                ui.label("Name:");
                                ui.text_edit_singleline(&mut obj.name);
                            });
                            name_changed = old_name != obj.name;

                            ui.separator();

                            ui.collapsing("Transform", |ui| {
                                let mut transform_widget = TransformWidget::from_transform(&obj.transform);
                                if transform_widget.show(ui) {
                                    obj.transform = transform_widget.to_transform();
                                    transform_changed = true;
                                }
                            });

                            for (idx, component) in obj.components.iter().enumerate() {
                                match component {
                                    crate::scene::Component::MeshRenderer(mr) => {
                                        ui.collapsing(format!("Mesh Renderer: {}", mr.asset_id), |ui| {
                                            ui.checkbox(&mut mr.visible.clone(), "Visible");
                                            ui.checkbox(&mut mr.wireframe.clone(), "Wireframe");
                                            if ui.button("Remove Component").clicked() {
                                                components_to_remove.push(idx);
                                            }
                                        });
                                    }
                                    crate::scene::Component::Light(light) => {
                                        ui.collapsing("Light", |ui| {
                                            ui.checkbox(&mut light.enabled.clone(), "Enabled");
                                            ui.add(egui::Slider::new(&mut light.intensity.clone(), 0.0..=10.0).text("Intensity"));
                                            if ui.button("Remove Component").clicked() {
                                                components_to_remove.push(idx);
                                            }
                                        });
                                    }
                                    crate::scene::Component::Camera(cam) => {
                                        ui.collapsing("Camera", |ui| {
                                            ui.checkbox(&mut cam.is_main.clone(), "Main Camera");
                                            ui.add(egui::Slider::new(&mut cam.fov.clone(), 30.0..=120.0).text("FOV"));
                                            if ui.button("Remove Component").clicked() {
                                                components_to_remove.push(idx);
                                            }
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }

                        if name_changed || transform_changed || !components_to_remove.is_empty() {
                            if let Some(obj) = scene.get_object_mut(id) {
                                for idx in components_to_remove.into_iter().rev() {
                                    obj.components.remove(idx);
                                }
                            }
                            scene.mark_dirty();
                        }

                        ui.separator();
                        if ui.button("Add Component").clicked() {
                            // TODO: показать меню выбора компонента
                        }
                    } else if selected_ids.len() > 1 {
                        ui.label(format!("{} objects selected", selected_ids.len()));
                    } else {
                        ui.label("No object selected");
                    }
                }
            });
    }

    fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .default_height(26.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Tool: {:?}", self.current_tool));
                    ui.separator();

                    if let Some(scene) = &self.scene_manager.current_scene {
                        ui.label(format!("Objects: {}", scene.objects.len()));
                        ui.separator();

                        if scene.dirty {
                            ui.colored_label(Color32::YELLOW, "● Unsaved");
                        } else {
                            ui.colored_label(Color32::GREEN, "● Saved");
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.renderer_initialized {
                            ui.colored_label(Color32::GREEN, "● Renderer");
                        } else {
                            ui.colored_label(Color32::YELLOW, "○ Renderer");
                        }
                        ui.label(&self.status_message);
                    });
                });
            });
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(Key::W) { self.current_tool = EditorTool::Move; }
            if i.key_pressed(Key::E) { self.current_tool = EditorTool::Rotate; }
            if i.key_pressed(Key::R) { self.current_tool = EditorTool::Scale; }
            if i.key_pressed(Key::Q) { self.current_tool = EditorTool::Select; }

            if i.key_pressed(Key::A) && i.modifiers.ctrl {
                if let Some(scene) = &mut self.scene_manager.current_scene {
                    scene.select_all();
                }
            }
            if i.key_pressed(Key::D) && i.modifiers.ctrl {
                if let Some(scene) = &mut self.scene_manager.current_scene {
                    scene.duplicate_selected();
                }
            }
            if i.key_pressed(Key::Delete) {
                if let Some(scene) = &mut self.scene_manager.current_scene {
                    scene.delete_selected();
                }
            }
            if i.key_pressed(Key::F) {
                if let Some(scene) = &self.scene_manager.current_scene {
                    scene.focus_on_selection(&mut self.camera);
                }
            }
            if i.key_pressed(Key::S) && i.modifiers.ctrl {
                if let Some(scene) = &self.scene_manager.current_scene {
                    let path_to_save = scene.path.clone();
                    if let Some(path) = path_to_save {
                        if let Err(e) = self.scene_manager.save_scene(&path) {
                            self.status_message = format!("Failed to save: {}", e);
                        } else {
                            self.status_message = "Scene saved".to_string();
                        }
                    }
                }
            }
            if i.key_pressed(Key::X) { self.gizmo.space = GizmoSpace::World; }
            if i.key_pressed(Key::C) { self.gizmo.space = GizmoSpace::Local; }
        });
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Инициализируем рендерер при первом кадре
        if self.frame_count == 0 && !self.renderer_initialized {
            #[cfg(target_os = "windows")]
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(window_handle) = frame.window_handle() {
                    if let RawWindowHandle::Win32(handle) = window_handle.as_raw() {
                        self.init_renderer(handle.hwnd.get() as usize);
                    }
                }
            }
        }

        self.handle_keyboard_shortcuts(ctx);

        // Обновляем FPS
        self.frame_count += 1;
        let now = ctx.input(|i| i.time);
        if now - self.last_frame_time > 1.0 {
            self.fps = self.frame_count as f32;
            self.frame_count = 0;
            self.last_frame_time = now;
        }

        // Обновляем аспект камеры
        let screen_rect = ctx.screen_rect();
        if screen_rect.height() > 0.0 {
            self.camera.aspect = screen_rect.width() / screen_rect.height();
        }

        // Рендерим UI
        self.render_menu_bar(ctx);
        self.render_hierarchy(ctx);
        self.render_inspector(ctx);
        self.render_status_bar(ctx);

        // Центральная панель с вьюпортом
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_viewport(ui);
        });

        // Диалоги
        if self.show_import_dialog {
            egui::Window::new("Import Model")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Select model file:");

                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("3D Models", &["obj", "blend", "gltf", "fbx"])
                            .pick_file() {

                            let path_str = path.to_string_lossy().to_string();

                            if let Some(mut renderer) = self.renderer.try_lock() {
                                match renderer.load_altex(&path_str) {
                                    Ok(meshes) => {
                                        let mesh_count = meshes.len();
                                        if let Some(scene) = &mut self.scene_manager.current_scene {
                                            for mesh_name in meshes {
                                                scene.add_object(
                                                    GameObject::new(&mesh_name).with_mesh(mesh_name)
                                                );
                                            }
                                        }
                                        self.status_message = format!("Imported {} meshes", mesh_count);
                                        self.show_import_dialog = false;
                                    }
                                    Err(e) => {
                                        self.status_message = format!("Import failed: {}", e);
                                    }
                                }
                            }
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        self.show_import_dialog = false;
                    }
                });
        }

        if self.show_new_scene_dialog {
            egui::Window::new("New Scene")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Enter scene name:");
                    let mut name = String::from("New Scene");
                    ui.text_edit_singleline(&mut name);

                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            self.scene_manager.new_scene(&name);
                            self.show_new_scene_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_scene_dialog = false;
                        }
                    });
                });
        }

        ctx.request_repaint();
    }
}

fn setup_egui_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.visuals = Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(40, 40, 45);
    style.visuals.panel_fill = Color32::from_rgb(35, 35, 40);
    style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(50, 50, 55);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(70, 130, 200);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(60, 60, 65);
    style.visuals.selection.bg_fill = Color32::from_rgb(70, 130, 200);

    ctx.set_style(style);
}