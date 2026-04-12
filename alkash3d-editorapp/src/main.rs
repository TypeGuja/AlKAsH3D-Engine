mod converters;

use eframe::egui;
use egui::*;
use converters::*;
use rfd::FileDialog;
use std::path::Path;

#[derive(Default)]
struct EditorApp {
    selected_object: usize,
    objects: Vec<SceneObject>,
    play_mode: bool,
    status_message: String,
    import_progress: String,
}

struct SceneObject {
    id: usize,
    name: String,
    obj_type: String,
    pos: [f32; 3],
    rot: [f32; 3],
    scl: [f32; 3],
}

impl Default for SceneObject {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Object".into(),
            obj_type: "mesh".into(),
            pos: [0.0, 0.0, 0.0],
            rot: [0.0, 0.0, 0.0],
            scl: [1.0, 1.0, 1.0],
        }
    }
}

impl EditorApp {
    fn new() -> Self {
        Self {
            selected_object: 0,
            objects: vec![
                SceneObject { id: 0, name: "Camera".into(), obj_type: "camera".into(), pos: [0.0, 5.0, 10.0], rot: [0.0, 0.0, 0.0], scl: [1.0, 1.0, 1.0] },
                SceneObject { id: 1, name: "Directional Light".into(), obj_type: "light".into(), pos: [0.0, 10.0, 0.0], rot: [45.0, 0.0, 0.0], scl: [1.0, 1.0, 1.0] },
                SceneObject { id: 2, name: "Cube".into(), obj_type: "mesh".into(), pos: [0.0, 0.0, 0.0], rot: [0.0, 0.0, 0.0], scl: [1.0, 1.0, 1.0] },
                SceneObject { id: 3, name: "Sphere".into(), obj_type: "mesh".into(), pos: [2.0, 0.0, 2.0], rot: [0.0, 0.0, 0.0], scl: [1.0, 1.0, 1.0] },
                SceneObject { id: 4, name: "Police Car".into(), obj_type: "car".into(), pos: [-2.0, 0.0, -2.0], rot: [0.0, 90.0, 0.0], scl: [1.0, 1.0, 1.0] },
            ],
            play_mode: false,
            status_message: "Ready".to_string(),
            import_progress: String::new(),
        }
    }

    fn import_obj(&mut self) {
        if let Some(path) = FileDialog::new().add_filter("OBJ Files", &["obj"]).pick_file() {
            let path_str = path.to_str().unwrap();
            let output = replace_extension(path_str, "altex");
            let name = Path::new(&output).file_stem().unwrap().to_str().unwrap();
            let output_path = format!("assets/models/{}.altex", name);

            self.status_message = format!("Importing: {}...", path_str);

            match obj_to_altex(path_str, &output_path) {
                Ok(_) => {
                    self.status_message = format!("Imported: {}", path_str);
                    self.import_progress = format!("✅ Saved to: {}", output_path);

                    // Добавляем импортированный объект в сцену
                    self.objects.push(SceneObject {
                        id: self.objects.len(),
                        name: name.to_string(),
                        obj_type: "mesh".into(),
                        pos: [0.0, 0.0, 0.0],
                        rot: [0.0, 0.0, 0.0],
                        scl: [1.0, 1.0, 1.0],
                    });
                }
                Err(e) => {
                    self.status_message = format!("Error: {}", e);
                    self.import_progress = format!("❌ Failed: {}", e);
                }
            }
        }
    }

    fn import_blend(&mut self) {
        if let Some(path) = FileDialog::new().add_filter("Blender Files", &["blend"]).pick_file() {
            let path_str = path.to_str().unwrap();
            let name = Path::new(path_str).file_stem().unwrap().to_str().unwrap();
            let output_path = format!("assets/models/{}.altex", name);

            self.status_message = format!("Importing: {}...", path_str);

            match blend_to_altex(path_str, &output_path) {
                Ok(_) => {
                    self.status_message = format!("Imported: {}", path_str);
                    self.import_progress = format!("✅ Saved to: {}", output_path);

                    self.objects.push(SceneObject {
                        id: self.objects.len(),
                        name: name.to_string(),
                        obj_type: "mesh".into(),
                        pos: [0.0, 0.0, 0.0],
                        rot: [0.0, 0.0, 0.0],
                        scl: [1.0, 1.0, 1.0],
                    });
                }
                Err(e) => {
                    self.status_message = format!("Error: {}", e);
                    self.import_progress = format!("❌ Failed: {}", e);
                }
            }
        }
    }

    fn create_car(&mut self, car_type: &str) {
        let output_path = format!("assets/cars/{}.alcar", car_type);

        match create_car_from_mesh(&format!("assets/models/{}.altex", car_type), &output_path, car_type) {
            Ok(_) => {
                self.status_message = format!("Created {} car", car_type);
                self.import_progress = format!("✅ Saved to: {}", output_path);
            }
            Err(e) => {
                self.status_message = format!("Error: {}", e);
                self.import_progress = format!("❌ Failed: {}", e);
            }
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Тёмная тема
        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::dark();
        ctx.set_style(style);

        // ========== ВЕРХНЕЕ МЕНЮ ==========
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("📄 New Scene").clicked() {
                        self.objects.clear();
                        self.status_message = "New scene created".to_string();
                        ui.close_menu();
                    }
                    if ui.button("📂 Open Scene").clicked() { ui.close_menu(); }
                    if ui.button("💾 Save Scene").clicked() { ui.close_menu(); }
                    ui.separator();
                    if ui.button("❌ Exit").clicked() { std::process::exit(0); }
                });

                ui.menu_button("Import", |ui| {
                    if ui.button("📦 Import OBJ → Altex").clicked() {
                        self.import_obj();
                        ui.close_menu();
                    }
                    if ui.button("🎨 Import BLEND → Altex").clicked() {
                        self.import_blend();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("🚓 Create Police Car").clicked() {
                        self.create_car("police");
                        ui.close_menu();
                    }
                    if ui.button("🏎️ Create Sports Car").clicked() {
                        self.create_car("sports");
                        ui.close_menu();
                    }
                });

                ui.menu_button("GameObject", |ui| {
                    if ui.button("📦 Cube").clicked() {
                        self.objects.push(SceneObject {
                            id: self.objects.len(),
                            name: format!("Cube_{}", self.objects.len()),
                            obj_type: "mesh".into(),
                            pos: [0.0, 0.0, 0.0],
                            rot: [0.0, 0.0, 0.0],
                            scl: [1.0, 1.0, 1.0],
                        });
                        self.status_message = "Added Cube".to_string();
                        ui.close_menu();
                    }
                    if ui.button("⚪ Sphere").clicked() {
                        self.objects.push(SceneObject {
                            id: self.objects.len(),
                            name: format!("Sphere_{}", self.objects.len()),
                            obj_type: "mesh".into(),
                            pos: [0.0, 0.0, 0.0],
                            rot: [0.0, 0.0, 0.0],
                            scl: [1.0, 1.0, 1.0],
                        });
                        self.status_message = "Added Sphere".to_string();
                        ui.close_menu();
                    }
                    if ui.button("💡 Light").clicked() {
                        self.objects.push(SceneObject {
                            id: self.objects.len(),
                            name: format!("Light_{}", self.objects.len()),
                            obj_type: "light".into(),
                            pos: [0.0, 5.0, 0.0],
                            rot: [0.0, 0.0, 0.0],
                            scl: [1.0, 1.0, 1.0],
                        });
                        self.status_message = "Added Light".to_string();
                        ui.close_menu();
                    }
                    if ui.button("🚗 Car").clicked() {
                        self.objects.push(SceneObject {
                            id: self.objects.len(),
                            name: format!("Car_{}", self.objects.len()),
                            obj_type: "car".into(),
                            pos: [0.0, 0.0, 0.0],
                            rot: [0.0, 0.0, 0.0],
                            scl: [1.0, 1.0, 1.0],
                        });
                        self.status_message = "Added Car".to_string();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("🗑 Delete Selected").clicked() {
                        if self.selected_object < self.objects.len() {
                            self.objects.remove(self.selected_object);
                            self.selected_object = 0;
                            self.status_message = "Deleted object".to_string();
                        }
                        ui.close_menu();
                    }
                });

                ui.add_space(20.0);
                if ui.button("▶ Play").clicked() {
                    self.play_mode = true;
                    self.status_message = "Play mode ON".to_string();
                }
                if ui.button("⏹ Stop").clicked() {
                    self.play_mode = false;
                    self.status_message = "Play mode OFF".to_string();
                }
            });
        });

        // ========== HIERARCHY ПАНЕЛЬ (СПИСОК ОБЪЕКТОВ) ==========
        egui::SidePanel::left("hierarchy")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📁 HIERARCHY");
                ui.separator();

                if self.objects.is_empty() {
                    ui.label("No objects in scene");
                    ui.label("Click GameObject → Cube to add");
                } else {
                    for obj in &self.objects {
                        let icon = match obj.obj_type.as_str() {
                            "camera" => "📷",
                            "light" => "☀️",
                            "car" => "🚗",
                            _ => "📦",
                        };
                        let label = format!("{}  {}", icon, obj.name);

                        let response = ui.selectable_label(self.selected_object == obj.id, label);
                        if response.clicked() {
                            self.selected_object = obj.id;
                            self.status_message = format!("Selected: {}", obj.name);
                        }
                    }
                }
            });

        // ========== INSPECTOR ПАНЕЛЬ ==========
        egui::SidePanel::right("inspector")
            .default_width(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📋 INSPECTOR");
                ui.separator();

                if let Some(obj) = self.objects.get_mut(self.selected_object) {
                    ui.label("🔧 TRANSFORM");

                    ui.horizontal(|ui| {
                        ui.label("📍 Position:");
                        ui.add(egui::DragValue::new(&mut obj.pos[0]).speed(0.1));
                        ui.add(egui::DragValue::new(&mut obj.pos[1]).speed(0.1));
                        ui.add(egui::DragValue::new(&mut obj.pos[2]).speed(0.1));
                    });

                    ui.horizontal(|ui| {
                        ui.label("🔄 Rotation:");
                        ui.add(egui::DragValue::new(&mut obj.rot[0]).speed(1.0));
                        ui.add(egui::DragValue::new(&mut obj.rot[1]).speed(1.0));
                        ui.add(egui::DragValue::new(&mut obj.rot[2]).speed(1.0));
                    });

                    ui.horizontal(|ui| {
                        ui.label("📏 Scale:");
                        ui.add(egui::DragValue::new(&mut obj.scl[0]).speed(0.1).clamp_range(0.01..=10.0));
                        ui.add(egui::DragValue::new(&mut obj.scl[1]).speed(0.1).clamp_range(0.01..=10.0));
                        ui.add(egui::DragValue::new(&mut obj.scl[2]).speed(0.1).clamp_range(0.01..=10.0));
                    });

                    ui.separator();

                    match obj.obj_type.as_str() {
                        "camera" => {
                            ui.label("🎥 CAMERA");
                            let mut fov = 60.0;
                            ui.horizontal(|ui| { ui.label("FOV:"); ui.add(egui::DragValue::new(&mut fov).speed(1.0)); });
                        }
                        "light" => {
                            ui.label("💡 LIGHT");
                            let mut intensity = 1.0;
                            ui.horizontal(|ui| { ui.label("Intensity:"); ui.add(egui::DragValue::new(&mut intensity).speed(0.1)); });
                        }
                        "car" => {
                            ui.label("🚗 CAR");
                            let mut speed = 250.0;
                            ui.horizontal(|ui| { ui.label("Max Speed:"); ui.add(egui::DragValue::new(&mut speed).speed(1.0)); });
                        }
                        _ => {
                            ui.label("📦 MESH RENDERER");
                            ui.label("Material: Default");
                        }
                    }
                } else {
                    ui.label("No object selected");
                }
            });

        // ========== ASSET BROWSER ==========
        egui::TopBottomPanel::bottom("asset_browser")
            .default_height(130.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📦 ASSET BROWSER");
                ui.separator();

                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| { ui.label("📦"); ui.label("Cube"); });
                    ui.add_space(30.0);
                    ui.vertical(|ui| { ui.label("⚪"); ui.label("Sphere"); });
                    ui.add_space(30.0);
                    ui.vertical(|ui| { ui.label("🚗"); ui.label("Police Car"); });
                    ui.add_space(30.0);
                    ui.vertical(|ui| { ui.label("💡"); ui.label("Light"); });
                    ui.add_space(30.0);
                    ui.vertical(|ui| { ui.label("🖼️"); ui.label("Brick Texture"); });
                });

                ui.separator();
                ui.colored_label(Color32::from_rgb(100, 200, 100), &self.import_progress);
            });

        // ========== СТАТУС БАР ==========
        egui::TopBottomPanel::bottom("status_bar")
            .default_height(26.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.play_mode {
                        ui.colored_label(Color32::from_rgb(0, 200, 0), "● PLAYING");
                    } else {
                        ui.label("🎮 Edit Mode");
                    }

                    ui.separator();
                    ui.label(&self.status_message);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("Objects: {}", self.objects.len()));
                        ui.label(format!("FPS: {:.0}", ctx.input(|i| i.time) * 1000.0 % 1000.0));
                        ui.label("Scene: Untitled");
                    });
                });
            });

        // ========== ЦЕНТРАЛЬНЫЙ ВЬЮПОРТ ==========
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();

            // Тёмный фон
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 25));

            // Рисуем сетку
            let grid_color = Color32::from_rgb(40, 40, 45);
            let spacing = 30.0;
            let center = rect.center();

            for i in -20..20 {
                let x = center.x + i as f32 * spacing;
                ui.painter().line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, grid_color));

                let y = center.y + i as f32 * spacing;
                ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, grid_color));
            }

            // Текст по центру
            let mut text = String::new();
            if self.objects.is_empty() {
                text = "🎮 3D Viewport\n\nNo objects in scene\n\nClick GameObject → Cube to add".to_string();
            } else {
                text = format!("🎮 3D Viewport\n\n{} objects in scene\n\nSelected: {}",
                               self.objects.len(),
                               if self.selected_object < self.objects.len() {
                                   &self.objects[self.selected_object].name
                               } else {
                                   "None"
                               }
                );
            }

            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(14.0),
                Color32::from_rgb(100, 100, 110),
            );
        });
    }
}

fn main() {
    // Создаём папки
    std::fs::create_dir_all("assets/models").unwrap_or(());
    std::fs::create_dir_all("assets/cars").unwrap_or(());
    std::fs::create_dir_all("assets/lights").unwrap_or(());
    std::fs::create_dir_all("assets/routes").unwrap_or(());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("Alkash3D Editor"),
        ..Default::default()
    };

    eframe::run_native("Alkash3D Editor", options, Box::new(|_cc| Box::new(EditorApp::new()))).unwrap();
}