// src/ui/assembly_editor.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя — см. `ui/sound_bank_editor.rs`
// про общий паттерн этой серии редакторов): дерево деталей `.alasm` как
// плоский список с полем "родитель" (индекс другой записи того же списка) —
// см. `converters::alasm::PartEdit`.

use egui::*;
use alkash3d_rs::AssemblyCategory;
use crate::converters::alasm::PartEdit;
use crate::EditorApp;

fn category_label(c: AssemblyCategory) -> &'static str {
    match c {
        AssemblyCategory::Vehicle => "Vehicle",
        AssemblyCategory::Engine => "Engine",
        AssemblyCategory::Gearbox => "Gearbox",
        AssemblyCategory::Differential => "Differential",
        AssemblyCategory::Generic => "Generic",
    }
}

fn joint_label(v: i32) -> &'static str {
    match v {
        0 => "Ball",
        1 => "Hinge",
        3 => "Slider",
        _ => "Fixed",
    }
}

pub fn render_assembly_editor(ctx: &egui::Context, app: &mut EditorApp) {
    if !app.assembly_editor.open {
        return;
    }

    let mut still_open = true;
    let mut do_load = false;
    let mut do_save = false;
    let mut add_part = false;
    let mut remove_idx: Option<usize> = None;

    egui::Window::new("🔧 Assembly Editor (.alasm)")
        .open(&mut still_open)
        .resizable(true)
        .default_width(600.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Название сборки:");
                ui.text_edit_singleline(&mut app.assembly_editor.name);
                egui::ComboBox::from_id_salt("asm_category")
                    .selected_text(category_label(app.assembly_editor.category))
                    .show_ui(ui, |ui| {
                        for c in [AssemblyCategory::Generic, AssemblyCategory::Vehicle, AssemblyCategory::Engine, AssemblyCategory::Gearbox, AssemblyCategory::Differential] {
                            ui.selectable_value(&mut app.assembly_editor.category, c, category_label(c));
                        }
                    });
            });
            if let Some(path) = &app.assembly_editor.loaded_path {
                ui.small(format!("📄 {}", path));
            }
            ui.weak("Ровно одна деталь без родителя — корень сборки.");
            ui.separator();

            let part_names: Vec<String> = app.assembly_editor.parts.iter().enumerate()
                .map(|(i, p)| format!("#{} {}", i, if p.name.is_empty() { "(без имени)" } else { &p.name }))
                .collect();

            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                if app.assembly_editor.parts.is_empty() {
                    ui.weak("Пока нет ни одной детали — нажми «➕ Добавить деталь» ниже.");
                }
                let part_count = app.assembly_editor.parts.len();
                for i in 0..part_count {
                    ui.push_id(i, |ui| {
                        ui.group(|ui| {
                            let p = &mut app.assembly_editor.parts[i];
                            ui.horizontal(|ui| {
                                ui.label("Имя:");
                                ui.text_edit_singleline(&mut p.name);
                                if ui.button("🗑").clicked() {
                                    remove_idx = Some(i);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Родитель:");
                                let cur_label = match p.parent {
                                    None => "— корень —".to_string(),
                                    Some(pi) => part_names.get(pi).cloned().unwrap_or_else(|| format!("#{}", pi)),
                                };
                                egui::ComboBox::from_id_salt("parent")
                                    .selected_text(cur_label)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut p.parent, None, "— корень —");
                                        for (j, label) in part_names.iter().enumerate() {
                                            if j == i { continue; } // нельзя быть родителем самому себе
                                            ui.selectable_value(&mut p.parent, Some(j), label);
                                        }
                                    });
                                ui.label("Meш:");
                                ui.text_edit_singleline(&mut p.mesh_path);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Позиция (отн. родителя):");
                                ui.add(egui::DragValue::new(&mut p.local_position[0]).speed(0.05).prefix("X:"));
                                ui.add(egui::DragValue::new(&mut p.local_position[1]).speed(0.05).prefix("Y:"));
                                ui.add(egui::DragValue::new(&mut p.local_position[2]).speed(0.05).prefix("Z:"));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Поворот (рад):");
                                ui.add(egui::DragValue::new(&mut p.local_rotation[0]).speed(0.02).prefix("X:"));
                                ui.add(egui::DragValue::new(&mut p.local_rotation[1]).speed(0.02).prefix("Y:"));
                                ui.add(egui::DragValue::new(&mut p.local_rotation[2]).speed(0.02).prefix("Z:"));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Масса:");
                                ui.add(egui::DragValue::new(&mut p.mass).speed(0.5).range(0.0..=100000.0));
                                ui.label("Трение:");
                                ui.add(egui::DragValue::new(&mut p.friction).speed(0.01).range(0.0..=2.0));
                                ui.label("Упругость:");
                                ui.add(egui::DragValue::new(&mut p.restitution).speed(0.01).range(0.0..=1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Соединение с родителем:");
                                egui::ComboBox::from_id_salt("joint")
                                    .selected_text(joint_label(p.joint_type))
                                    .show_ui(ui, |ui| {
                                        for v in 0..=3i32 {
                                            ui.selectable_value(&mut p.joint_type, v, joint_label(v));
                                        }
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.label("Порог разрушения (лин./угл.):");
                                ui.add(egui::DragValue::new(&mut p.break_impulse_linear).speed(1.0).range(0.0..=1_000_000.0));
                                ui.add(egui::DragValue::new(&mut p.break_impulse_angular).speed(1.0).range(0.0..=1_000_000.0));
                                ui.weak("(0 = не ломается)");
                            });
                        });
                    });
                }
            });

            ui.separator();
            if ui.button("➕ Добавить деталь").clicked() {
                add_part = true;
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("📂 Загрузить .alasm...").clicked() {
                    do_load = true;
                }
                if ui.button("💾 Сохранить как .alasm...").clicked() {
                    do_save = true;
                }
            });
        });

    if let Some(i) = remove_idx {
        app.assembly_editor.parts.remove(i);
        // Ссылки на удалённую (или сдвинутые следом) детали как на
        // родителя иначе указывали бы не туда — переиндексируем/обнуляем.
        for p in app.assembly_editor.parts.iter_mut() {
            if let Some(pi) = p.parent {
                if pi == i {
                    p.parent = None;
                } else if pi > i {
                    p.parent = Some(pi - 1);
                }
            }
        }
    }
    if add_part {
        app.assembly_editor.parts.push(PartEdit::default());
    }
    if do_load {
        app.load_assembly_dialog();
    }
    if do_save {
        app.save_assembly_dialog();
    }
    if !still_open {
        app.assembly_editor.open = false;
    }
}
