// src/ui/script_editor.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя — см. `ui/sound_bank_editor.rs`
// про общий паттерн этой серии редакторов): реестр скриптов `.alscript` как
// автономный список записей, не завязанный на `ScriptedEntity`-объекты сцены.

use egui::*;
use crate::converters::alscript::ScriptEdit;
use crate::EditorApp;

fn type_label(v: u32) -> &'static str {
    match v {
        1 => "Lua (DLL)",
        2 => "Native (DLL)",
        _ => "Python (hot-reload)",
    }
}

pub fn render_script_editor(ctx: &egui::Context, app: &mut EditorApp) {
    if !app.script_editor.open {
        return;
    }

    let mut still_open = true;
    let mut do_load = false;
    let mut do_save = false;
    let mut add_entry = false;
    let mut remove_idx: Option<usize> = None;

    egui::Window::new("📜 Script Registry Editor")
        .open(&mut still_open)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            if let Some(path) = &app.script_editor.loaded_path {
                ui.small(format!("📄 {}", path));
                ui.separator();
            }

            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                if app.script_editor.entries.is_empty() {
                    ui.weak("Пока нет ни одного скрипта — нажми «➕ Добавить скрипт» ниже.");
                }
                for (i, e) in app.script_editor.entries.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label("Имя:");
                                ui.text_edit_singleline(&mut e.name);
                                if ui.button("🗑").clicked() {
                                    remove_idx = Some(i);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Тип:");
                                egui::ComboBox::from_id_salt("script_type")
                                    .selected_text(type_label(e.script_type))
                                    .show_ui(ui, |ui| {
                                        for v in 0..=2u32 {
                                            ui.selectable_value(&mut e.script_type, v, type_label(v));
                                        }
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.label(if e.script_type == 2 { "Путь к DLL:" } else { "Путь к исходнику:" });
                                ui.text_edit_singleline(&mut e.path);
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut e.hot_reloadable, "Hot-reload");
                                ui.label("Приоритет:");
                                ui.add(egui::DragValue::new(&mut e.priority).range(-100..=100));
                                ui.label("Поток:");
                                ui.add(egui::DragValue::new(&mut e.run_on_thread).range(0..=16));
                            });
                        });
                    });
                }
            });

            ui.separator();
            if ui.button("➕ Добавить скрипт").clicked() {
                add_entry = true;
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("📂 Загрузить .alscript...").clicked() {
                    do_load = true;
                }
                if ui.button("💾 Сохранить как .alscript...").clicked() {
                    do_save = true;
                }
            });
        });

    if let Some(i) = remove_idx {
        app.script_editor.entries.remove(i);
    }
    if add_entry {
        app.script_editor.entries.push(ScriptEdit::default());
    }
    if do_load {
        app.load_script_registry_dialog();
    }
    if do_save {
        app.save_script_registry_dialog();
    }
    if !still_open {
        app.script_editor.open = false;
    }
}
