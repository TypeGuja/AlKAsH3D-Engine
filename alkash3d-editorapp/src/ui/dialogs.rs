use egui::*;
use crate::EditorApp;

pub fn render_dialogs(ctx: &egui::Context, app: &mut EditorApp) {
    // Диалог создания новой сцены
    if app.show_new_scene_dialog {
        render_new_scene_dialog(ctx, app);
    }

    // Диалог импорта модели
    if app.show_import_dialog {
        render_import_dialog(ctx, app);
    }

    // Диалог прогресса импорта
    if !app.pending_imports.is_empty() {
        render_import_progress(ctx, app);
    }
}

fn render_new_scene_dialog(ctx: &egui::Context, app: &mut EditorApp) {
    egui::Window::new("New Scene")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.heading("Create New Scene");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Scene name:");
                ui.text_edit_singleline(&mut app.new_scene_name);
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    let name = if app.new_scene_name.trim().is_empty() {
                        "Untitled".to_string()
                    } else {
                        app.new_scene_name.clone()
                    };

                    app.scene = crate::scene::Scene::new(&name);
                    app.history = crate::editor::CommandHistory::new(100);
                    app.show_new_scene_dialog = false;
                    app.log(&format!("📄 Created new scene: {}", name), Color32::GREEN);
                }

                if ui.button("Cancel").clicked() {
                    app.show_new_scene_dialog = false;
                    app.new_scene_name = "New Scene".to_string();
                }
            });
        });
}

fn render_import_dialog(ctx: &egui::Context, app: &mut EditorApp) {
    egui::Window::new("Import Model")
        .collapsible(false)
        .resizable(false)
        .min_width(350.0)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.heading("📥 Import 3D Model");
            ui.separator();

            ui.label("Select a 3D model file to import:");
            ui.add_space(5.0);

            ui.label("Supported formats:");
            ui.label("  • OBJ - Wavefront (.obj)");
            ui.label("  • BLEND - Blender (.blend)");
            ui.label("  • FBX - Autodesk FBX (.fbx)");
            ui.label("  • glTF/GLB - GL Transmission Format");

            ui.add_space(10.0);

            ui.colored_label(Color32::YELLOW, "⚠️ Large models will be imported in background");
            ui.colored_label(Color32::GRAY, "   and displayed with bounding box for performance.");

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let browse_btn = ui.button("📂 Browse for Model...");

                if browse_btn.clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("3D Models", &["obj", "blend", "fbx", "gltf", "glb"])
                        .add_filter("Wavefront OBJ", &["obj"])
                        .add_filter("Blender", &["blend"])
                        .add_filter("FBX", &["fbx"])
                        .add_filter("glTF", &["gltf", "glb"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                    {
                        let path_str = path.to_string_lossy().to_string();

                        // Проверяем размер файла
                        let file_size = std::fs::metadata(&path_str)
                            .map(|m| m.len())
                            .unwrap_or(0);

                        let size_mb = file_size as f64 / (1024.0 * 1024.0);

                        if size_mb > 50.0 {
                            app.log(
                                &format!("⚠️ Very large file ({:.1} MB)! Import may take a while...", size_mb),
                                Color32::YELLOW
                            );
                        }

                        // ЗАПУСКАЕМ АСИНХРОННЫЙ ИМПОРТ
                        app.import_model_async(&path_str);
                        app.show_import_dialog = false;
                        app.log(&format!("📥 Importing {} ({:.1} MB)...", path_str, size_mb), Color32::YELLOW);
                    }
                }

                if ui.button("Cancel").clicked() {
                    app.show_import_dialog = false;
                }
            });

            ui.add_space(5.0);
            ui.colored_label(Color32::GRAY, "Tip: Large models will show as bounding boxes for better performance");
        });
}

fn render_import_progress(ctx: &egui::Context, app: &mut EditorApp) {
    let import_count = app.pending_imports.len();
    let current_path = app.pending_imports.first()
        .map(|i| i.path.clone())
        .unwrap_or_default();

    egui::Window::new("Importing...")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::RIGHT_BOTTOM, [-20.0, -20.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Importing model...");
            });

            ui.add_space(5.0);
            ui.label(format!("File: {}", current_path));
            ui.label(format!("{} import(s) in queue", import_count));

            ui.add_space(10.0);

            // Прогресс-бар
            let progress = app.import_progress;
            let progress_bar = egui::ProgressBar::new(progress)
                .show_percentage()
                .animate(true);
            ui.add(progress_bar);
        });
}