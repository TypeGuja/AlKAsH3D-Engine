use egui::*;
use crate::EditorApp;
use crate::math::Vec3;

pub fn render_dialogs(ctx: &egui::Context, app: &mut EditorApp) {
    if app.show_new_scene_dialog {
        egui::Window::new("New Scene")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Scene name:");
                ui.text_edit_singleline(&mut app.new_scene_name);
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        app.scene = crate::scene::Scene::new(&app.new_scene_name);
                        app.history = crate::editor::CommandHistory::new(100);
                        app.show_new_scene_dialog = false;
                    }
                    if ui.button("Cancel").clicked() {
                        app.show_new_scene_dialog = false;
                    }
                });
            });
    }

    if app.show_import_dialog {
        egui::Window::new("Import Model")
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("3D Models", &["obj", "blend", "fbx", "gltf", "glb"])
                        .pick_file()
                    {
                        let path_str = path.to_string_lossy().to_string();
                        println!("[DIALOG] Selected file: {}", path_str);

                        match app.asset_library.import_model(&path_str) {
                            Ok(imported_names) => {
                                println!("[DIALOG] Successfully imported: {:?}", imported_names);
                                for name in &imported_names {
                                    if let Some(mesh) = app.asset_library.get_mesh(name) {
                                        let mesh_clone = mesh.clone();
                                        let obj = crate::scene::GameObject::new(
                                            name,
                                            crate::scene::ObjectType::Mesh(
                                                crate::scene::MeshComponent {
                                                    mesh: mesh_clone,
                                                    material: crate::material::Material {
                                                        color: [0.7, 0.7, 0.7, 1.0],
                                                        metallic: 0.0,
                                                        roughness: 0.8,
                                                        ..Default::default()
                                                    },
                                                    visible: true,
                                                    wireframe: false,
                                                    solid: true,
                                                    double_sided: false,
                                                }
                                            )
                                        );
                                        app.scene.add_object(obj);
                                        println!("[DIALOG] Added object: {}", name);
                                    } else {
                                        println!("[DIALOG] WARNING: Mesh '{}' not found after import", name);
                                    }
                                }
                                app.log(&format!("✅ Imported {} models", imported_names.len()), Color32::GREEN);
                                app.show_import_dialog = false;
                            }
                            Err(e) => {
                                println!("[DIALOG] Import error: {}", e);
                                app.log(&format!("❌ Import failed: {}", e), Color32::RED);
                            }
                        }
                    }
                }
                if ui.button("Cancel").clicked() {
                    app.show_import_dialog = false;
                }
            });
    }
}