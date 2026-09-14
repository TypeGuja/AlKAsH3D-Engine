// src/ui/sound_bank_editor.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя: "давай теперь делать эдитор
// под каждый формат, типо надо тебе поправить звук или сделать, выбираешь
// что создать и там нормальный эдитор... чтобы они не лежали мёртвым
// грузом"): первый из серии отдельных, не завязанных на 3D-сцену редакторов
// под конкретный формат (Assets-меню в menu_bar.rs — туда же лягут
// следующие: скрипты/машины/сборки/маршруты). Держит СВОЙ список записей
// (`EditorApp::sound_bank_editor`, `Vec<SoundEntryEdit>` из
// `converters::alsnd`) и умеет напрямую грузить/сохранять `.alsnd` —
// открывать саму 3D-сцену для этого не нужно (см. подробное объяснение "не
// пространственные данные" в шапке `converters/alsnd.rs`).

use egui::*;
use crate::converters::alsnd::SoundEntryEdit;
use crate::EditorApp;

fn format_label(v: u32) -> &'static str {
    match v {
        1 => "OGG",
        2 => "MP3",
        3 => "FLAC",
        4 => "OPUS",
        _ => "WAV",
    }
}

fn category_label(v: u32) -> &'static str {
    match v {
        1 => "Music",
        2 => "Ambient",
        3 => "Voice",
        4 => "UI",
        _ => "SFX",
    }
}

pub fn render_sound_bank_editor(ctx: &egui::Context, app: &mut EditorApp) {
    if !app.sound_bank_editor.open {
        return;
    }

    let mut still_open = true;
    let mut do_load = false;
    let mut do_save = false;
    let mut add_entry = false;
    let mut remove_idx: Option<usize> = None;

    egui::Window::new("🔊 Sound Bank Editor")
        .open(&mut still_open)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Название банка:");
                ui.text_edit_singleline(&mut app.sound_bank_editor.bank_name);
            });
            if let Some(path) = &app.sound_bank_editor.loaded_path {
                ui.small(format!("📄 {}", path));
            }
            ui.separator();

            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                if app.sound_bank_editor.entries.is_empty() {
                    ui.weak("Пока нет ни одного звука — нажми «➕ Добавить звук» ниже.");
                }
                for (i, e) in app.sound_bank_editor.entries.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label("Имя:");
                                ui.text_edit_singleline(&mut e.name);
                                if ui.button("🗑").on_hover_text("Удалить").clicked() {
                                    remove_idx = Some(i);
                                }
                            });
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("format")
                                    .selected_text(format_label(e.format))
                                    .show_ui(ui, |ui| {
                                        for v in 0..=4u32 {
                                            ui.selectable_value(&mut e.format, v, format_label(v));
                                        }
                                    });
                                egui::ComboBox::from_id_salt("category")
                                    .selected_text(category_label(e.category))
                                    .show_ui(ui, |ui| {
                                        for v in 0..=4u32 {
                                            ui.selectable_value(&mut e.category, v, category_label(v));
                                        }
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.label("Громкость:");
                                ui.add(egui::Slider::new(&mut e.volume, 0.0..=2.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Питч:");
                                ui.add(egui::Slider::new(&mut e.pitch, 0.1..=3.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Spatial blend (0=2D, 1=3D):");
                                ui.add(egui::Slider::new(&mut e.spatial_blend, 0.0..=1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Приоритет:");
                                ui.add(egui::DragValue::new(&mut e.priority).range(0..=255));
                                ui.label("Макс. копий одновременно:");
                                ui.add(egui::DragValue::new(&mut e.max_instances).range(1..=64));
                            });
                        });
                    });
                }
            });

            ui.separator();
            if ui.button("➕ Добавить звук").clicked() {
                add_entry = true;
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("📂 Загрузить .alsnd...").clicked() {
                    do_load = true;
                }
                if ui.button("💾 Сохранить как .alsnd...").clicked() {
                    do_save = true;
                }
            });
        });

    if let Some(i) = remove_idx {
        app.sound_bank_editor.entries.remove(i);
    }
    if add_entry {
        app.sound_bank_editor.entries.push(SoundEntryEdit::default());
    }
    if do_load {
        app.load_sound_bank_dialog();
    }
    if do_save {
        app.save_sound_bank_dialog();
    }
    if !still_open {
        app.sound_bank_editor.open = false;
    }
}
