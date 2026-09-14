// src/ui/material_library_editor.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя — см. `ui/sound_bank_editor.rs`
// про общий паттерн этой серии редакторов). Проще остальных: данные УЖЕ
// живут как автономная (не завязанная на сцену) структура —
// `AssetLibrary::materials` (`HashMap<String, Material>`) — так что здесь
// не нужна отдельная Edit-модель, редактор работает прямо по ней.
// Переименование = удалить старый ключ + вставить новый (HashMap), поэтому
// имя материала запрашивается только при СОЗДАНИИ, а не редактируется inline.

use egui::*;
use crate::material::Material;
use crate::EditorApp;

pub fn render_material_library_editor(ctx: &egui::Context, app: &mut EditorApp) {
    if !app.material_library_editor.open {
        return;
    }

    let mut still_open = true;
    let mut do_load = false;
    let mut do_save = false;
    let mut add_material = false;
    let mut remove_name: Option<String> = None;

    egui::Window::new("🎨 Material Library Editor (.almat)")
        .open(&mut still_open)
        .resizable(true)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Новый материал:");
                ui.text_edit_singleline(&mut app.material_library_editor.new_material_name);
                if ui.button("➕ Создать").clicked() {
                    add_material = true;
                }
            });
            ui.separator();

            let mut names: Vec<String> = app.asset_library.materials.keys().cloned().collect();
            names.sort();

            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                if names.is_empty() {
                    ui.weak("Библиотека пуста — создай материал выше.");
                }
                for name in &names {
                    let Some(mat) = app.asset_library.materials.get_mut(name) else { continue; };
                    ui.push_id(name, |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(name);
                                if ui.button("🗑").clicked() {
                                    remove_name = Some(name.clone());
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Цвет:");
                                ui.color_edit_button_rgba_unmultiplied(&mut mat.color);
                                ui.label("Metallic:");
                                ui.add(egui::Slider::new(&mut mat.metallic, 0.0..=1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Roughness:");
                                ui.add(egui::Slider::new(&mut mat.roughness, 0.0..=1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Emissive:");
                                let mut emissive4 = [mat.emissive[0], mat.emissive[1], mat.emissive[2], 1.0];
                                if ui.color_edit_button_rgba_unmultiplied(&mut emissive4).changed() {
                                    mat.emissive = [emissive4[0], emissive4[1], emissive4[2]];
                                }
                            });
                        });
                    });
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("📂 Загрузить .almat (добавить в библиотеку)...").clicked() {
                    do_load = true;
                }
                if ui.button("💾 Сохранить всю библиотеку как .almat...").clicked() {
                    do_save = true;
                }
            });
        });

    if let Some(name) = remove_name {
        app.asset_library.materials.remove(&name);
    }
    if add_material {
        let name = app.material_library_editor.new_material_name.trim().to_string();
        if name.is_empty() {
            app.log("⚠️ Введи имя материала", Color32::YELLOW);
        } else if app.asset_library.materials.contains_key(&name) {
            app.log(&format!("⚠️ Материал '{}' уже есть в библиотеке", name), Color32::YELLOW);
        } else {
            app.asset_library.materials.insert(name.clone(), Material { name, ..Default::default() });
            app.material_library_editor.new_material_name.clear();
        }
    }
    if do_load {
        app.import_almat_dialog();
    }
    if do_save {
        app.export_materials_to_almat_dialog();
    }
    if !still_open {
        app.material_library_editor.open = false;
    }
}
