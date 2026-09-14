// src/ui/route_editor.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя — см. `ui/sound_bank_editor.rs`
// про общий паттерн этой серии редакторов). В отличие от Sound Bank Editor,
// маршрут пространственный (у точек есть позиция) — но всё равно не привязан
// к текущей 3D-сцене: точки задаются числами прямо здесь, а не кликами по
// вьюпорту (для "кликнуть точки в 3D" уже есть отдельный путь —
// `export_selection_to_alroute` из выделенных объектов сцены).

use egui::*;
use crate::converters::alroute::{RouteEdit, WaypointEdit};
use crate::EditorApp;

pub fn render_route_editor(ctx: &egui::Context, app: &mut EditorApp) {
    if !app.route_editor.open {
        return;
    }

    let mut still_open = true;
    let mut do_load = false;
    let mut do_save = false;
    let mut add_route = false;
    let mut remove_route: Option<usize> = None;

    egui::Window::new("🛣 Route Editor")
        .open(&mut still_open)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            if let Some(path) = &app.route_editor.loaded_path {
                ui.small(format!("📄 {}", path));
                ui.separator();
            }

            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                if app.route_editor.routes.is_empty() {
                    ui.weak("Пока нет ни одного маршрута — нажми «➕ Добавить маршрут» ниже.");
                }
                for (ri, route) in app.route_editor.routes.iter_mut().enumerate() {
                    ui.push_id(ri, |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label("Название:");
                                ui.text_edit_singleline(&mut route.name);
                                if ui.button("🗑 Маршрут").clicked() {
                                    remove_route = Some(ri);
                                }
                            });
                            ui.horizontal(|ui| {
                                let mut looped = route.loop_type != 0;
                                if ui.checkbox(&mut looped, "Замкнутый").changed() {
                                    route.loop_type = if looped { 1 } else { 0 };
                                }
                                ui.label("Скорость ×:");
                                ui.add(egui::DragValue::new(&mut route.speed_factor).speed(0.1).range(0.01..=10.0));
                                ui.label("Задержка старта (сек):");
                                ui.add(egui::DragValue::new(&mut route.start_delay).speed(0.1).range(0.0..=600.0));
                            });

                            ui.label("Точки:");
                            let mut remove_wp: Option<usize> = None;
                            for (wi, wp) in route.waypoints.iter_mut().enumerate() {
                                ui.push_id(wi, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("#{}", wi));
                                        ui.add(egui::DragValue::new(&mut wp.position[0]).speed(0.1).prefix("X:"));
                                        ui.add(egui::DragValue::new(&mut wp.position[1]).speed(0.1).prefix("Y:"));
                                        ui.add(egui::DragValue::new(&mut wp.position[2]).speed(0.1).prefix("Z:"));
                                        ui.add(egui::DragValue::new(&mut wp.wait_time).speed(0.1).range(0.0..=600.0).prefix("ожид: "));
                                        ui.add(egui::DragValue::new(&mut wp.speed_limit).speed(0.5).range(0.0..=500.0).prefix("лимит: "));
                                        if ui.small_button("🗑").clicked() {
                                            remove_wp = Some(wi);
                                        }
                                    });
                                });
                            }
                            if let Some(wi) = remove_wp {
                                route.waypoints.remove(wi);
                            }
                            if ui.button("➕ Точка").clicked() {
                                route.waypoints.push(WaypointEdit::default());
                            }
                        });
                    });
                }
            });

            ui.separator();
            if ui.button("➕ Добавить маршрут").clicked() {
                add_route = true;
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("📂 Загрузить .alroute...").clicked() {
                    do_load = true;
                }
                if ui.button("💾 Сохранить как .alroute...").clicked() {
                    do_save = true;
                }
            });
        });

    if let Some(ri) = remove_route {
        app.route_editor.routes.remove(ri);
    }
    if add_route {
        app.route_editor.routes.push(RouteEdit::default());
    }
    if do_load {
        app.load_route_dialog();
    }
    if do_save {
        app.save_route_dialog();
    }
    if !still_open {
        app.route_editor.open = false;
    }
}
