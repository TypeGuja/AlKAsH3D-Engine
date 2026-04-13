use eframe::egui;
use egui::*;
use wgpu::*;

#[derive(Default)]
struct EditorApp {
    selected_object: usize,
    objects: Vec<SceneObject>,
    play_mode: bool,
    rotation: f32,
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
                SceneObject { id: 1, name: "Cube".into(), obj_type: "mesh".into(), pos: [0.0, 0.0, 0.0], rot: [0.0, 0.0, 0.0], scl: [1.0, 1.0, 1.0] },
                SceneObject { id: 2, name: "Sphere".into(), obj_type: "mesh".into(), pos: [2.0, 0.0, 2.0], rot: [0.0, 0.0, 0.0], scl: [1.0, 1.0, 1.0] },
                SceneObject { id: 3, name: "Police Car".into(), obj_type: "car".into(), pos: [-2.0, 0.0, -2.0], rot: [0.0, 90.0, 0.0], scl: [1.0, 1.0, 1.0] },
            ],
            play_mode: false,
            rotation: 0.0,
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.rotation += 0.01;

        // Тёмная тема
        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::dark();
        ctx.set_style(style);

        // Верхнее меню
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("📄 New Scene").clicked() { self.objects.clear(); ui.close_menu(); }
                    if ui.button("❌ Exit").clicked() { std::process::exit(0); }
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
                        ui.close_menu();
                    }
                });
                ui.add_space(20.0);
                if ui.button("▶ Play").clicked() { self.play_mode = true; }
                if ui.button("⏹ Stop").clicked() { self.play_mode = false; }
            });
        });

        // Hierarchy
        egui::SidePanel::left("hierarchy").default_width(250.0).show(ctx, |ui| {
            ui.heading("📁 HIERARCHY");
            ui.separator();
            for obj in &self.objects {
                let icon = match obj.obj_type.as_str() {
                    "camera" => "📷", "light" => "☀️", "car" => "🚗", _ => "📦",
                };
                if ui.selectable_label(self.selected_object == obj.id, format!("{} {}", icon, obj.name)).clicked() {
                    self.selected_object = obj.id;
                }
            }
        });

        // Inspector
        egui::SidePanel::right("inspector").default_width(300.0).show(ctx, |ui| {
            ui.heading("📋 INSPECTOR");
            ui.separator();
            if let Some(obj) = self.objects.get_mut(self.selected_object) {
                ui.label("🔧 TRANSFORM");
                ui.horizontal(|ui| {
                    ui.label("Position:");
                    ui.add(egui::DragValue::new(&mut obj.pos[0]).speed(0.1));
                    ui.add(egui::DragValue::new(&mut obj.pos[1]).speed(0.1));
                    ui.add(egui::DragValue::new(&mut obj.pos[2]).speed(0.1));
                });
                ui.horizontal(|ui| {
                    ui.label("Rotation:");
                    ui.add(egui::DragValue::new(&mut obj.rot[0]).speed(1.0));
                    ui.add(egui::DragValue::new(&mut obj.rot[1]).speed(1.0));
                    ui.add(egui::DragValue::new(&mut obj.rot[2]).speed(1.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Scale:");
                    ui.add(egui::DragValue::new(&mut obj.scl[0]).speed(0.1));
                    ui.add(egui::DragValue::new(&mut obj.scl[1]).speed(0.1));
                    ui.add(egui::DragValue::new(&mut obj.scl[2]).speed(0.1));
                });
            }
        });

        // Status Bar
        egui::TopBottomPanel::bottom("status_bar").default_height(26.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.play_mode { ui.colored_label(Color32::GREEN, "● PLAYING"); }
                else { ui.label("🎮 Edit Mode"); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Objects: {}", self.objects.len()));
                });
            });
        });

        // ========== 3D VIEWPORT с рендерингом объектов ==========
        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();

            // Фон
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 25));

            // Рисуем сетку
            let grid_color = Color32::from_rgb(40, 40, 45);
            let center = rect.center();
            let spacing = 30.0;

            for i in -20..20 {
                let x = center.x + i as f32 * spacing;
                ui.painter().line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, grid_color));
                let y = center.y + i as f32 * spacing;
                ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, grid_color));
            }

            // Рисуем каждый объект из сцены
            for obj in &self.objects {
                // Проекция 3D -> 2D (вид сверху)
                let screen_x = center.x + obj.pos[0] * spacing;
                let screen_y = center.y - obj.pos[2] * spacing;
                let screen_pos = pos2(screen_x, screen_y);

                // Цвет в зависимости от типа
                let color = match obj.obj_type.as_str() {
                    "camera" => Color32::from_rgb(0, 200, 200),
                    "light" => Color32::from_rgb(255, 255, 100),
                    "car" => Color32::from_rgb(200, 100, 0),
                    _ => Color32::from_rgb(100, 200, 100),
                };

                // Рисуем объект
                ui.painter().circle_filled(screen_pos, 10.0, color);
                ui.painter().circle_stroke(screen_pos, 10.0, Stroke::new(2.0, Color32::WHITE));

                // Подпись
                ui.painter().text(screen_pos + Vec2::new(12.0, -5.0), Align2::LEFT_CENTER, &obj.name, FontId::proportional(11.0), Color32::LIGHT_GRAY);

                // Рисуем линию от объекта до земли
                ui.painter().line_segment([screen_pos, pos2(screen_x, center.y + 100.0)], Stroke::new(1.0, Color32::from_rgb(60, 60, 70)));
            }

            // Информация о выбранном объекте
            if let Some(obj) = self.objects.get(self.selected_object) {
                let info = format!("Selected: {}\nPosition: [{:.1}, {:.1}, {:.1}]\nType: {}",
                                   obj.name, obj.pos[0], obj.pos[1], obj.pos[2], obj.obj_type);
                ui.painter().text(rect.left_top() + Vec2::new(10.0, 10.0), Align2::LEFT_TOP, info, FontId::proportional(12.0), Color32::LIGHT_GRAY);
            }
        });

        ctx.request_repaint();
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("Alkash3D Editor - Scene Editor"),
        ..Default::default()
    };

    eframe::run_native("Alkash3D Editor", options, Box::new(|_cc| Box::new(EditorApp::new()))).unwrap();
}