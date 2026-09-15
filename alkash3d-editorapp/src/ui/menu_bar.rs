use egui::*;

pub fn render_menu_bar(ctx: &egui::Context, app: &mut crate::EditorApp) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Scene").clicked() {
                    app.show_new_scene_dialog = true;
                    ui.close_menu();
                }
                if ui.button("Import Model...").clicked() {
                    app.show_import_dialog = true;
                    ui.close_menu();
                }

                ui.separator();
                ui.menu_button("Export", |ui| {
                    if ui.button("Selected Object to .altex...").clicked() {
                        app.export_selected_to_altex();
                        ui.close_menu();
                    }
                    if ui.button("Scene to .alworld (folder)...").clicked() {
                        app.export_scene_to_alworld_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Lighting to .alfar...").clicked() {
                        app.export_lighting_to_alfar_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Material Library to .almat...").clicked() {
                        app.export_materials_to_almat_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Sounds to .alsnd...").clicked() {
                        app.export_sounds_to_alsnd_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Scripts to .alscript...").clicked() {
                        app.export_scripts_to_alscript_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Selected as Route (.alroute)...").clicked() {
                        app.export_selection_to_alroute_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Selected as Assembly (.alasm)...").clicked() {
                        app.export_selected_to_alasm_dialog();
                        ui.close_menu();
                    }
                    ui.menu_button("Car Preset (.alcar)", |ui| {
                        if ui.button("Default").clicked() {
                            app.export_car_preset_dialog(crate::converters::alcar::CarPreset::Default);
                            ui.close_menu();
                        }
                        if ui.button("Sports Car").clicked() {
                            app.export_car_preset_dialog(crate::converters::alcar::CarPreset::Sports);
                            ui.close_menu();
                        }
                        if ui.button("Police Car").clicked() {
                            app.export_car_preset_dialog(crate::converters::alcar::CarPreset::Police);
                            ui.close_menu();
                        }
                    });
                });
                ui.menu_button("Import Engine Format", |ui| {
                    if ui.button(".altex (adds to scene)...").clicked() {
                        app.import_altex_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Open World .alworld (replaces scene)...").clicked() {
                        app.import_alworld_dialog();
                        ui.close_menu();
                    }
                    if ui.button(".alfar lighting (adds to scene)...").clicked() {
                        app.import_alfar_dialog();
                        ui.close_menu();
                    }
                    if ui.button(".almat materials (adds to library)...").clicked() {
                        app.import_almat_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(".alsnd sounds (adds to scene)...").clicked() {
                        app.import_alsnd_dialog();
                        ui.close_menu();
                    }
                    if ui.button(".alscript scripts (adds to scene)...").clicked() {
                        app.import_alscript_dialog();
                        ui.close_menu();
                    }
                    if ui.button(".alroute route (adds to scene)...").clicked() {
                        app.import_alroute_dialog();
                        ui.close_menu();
                    }
                    if ui.button(".alasm assembly (adds to scene)...").clicked() {
                        app.import_alasm_dialog();
                        ui.close_menu();
                    }
                    if ui.button(".alcar car preset (adds to scene)...").clicked() {
                        app.import_alcar_dialog();
                        ui.close_menu();
                    }
                });

                ui.separator();
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });

            // ДОБАВЛЕНО (по прямому запросу пользователя: "давай делать
            // эдитор под каждый формат... чтобы они не лежали мёртвым
            // грузом") — отдельные, не завязанные на 3D-сцену редакторы под
            // конкретный формат данных (первый — звуковой банк, см.
            // `ui/sound_bank_editor.rs`); остальные форматы получат свои
            // пункты здесь по тому же паттерну позже.
            ui.menu_button("Assets", |ui| {
                if ui.button("🔊 Sound Bank Editor...").clicked() {
                    app.open_sound_bank_editor();
                    ui.close_menu();
                }
                if ui.button("🛣 Route Editor...").clicked() {
                    app.open_route_editor();
                    ui.close_menu();
                }
                if ui.button("📜 Script Registry Editor...").clicked() {
                    app.open_script_editor();
                    ui.close_menu();
                }
                if ui.button("🔧 Assembly Editor...").clicked() {
                    app.open_assembly_editor();
                    ui.close_menu();
                }
                if ui.button("🚗 Car Preset Editor...").clicked() {
                    app.open_car_preset_editor();
                    ui.close_menu();
                }
                if ui.button("🎨 Material Library Editor...").clicked() {
                    app.open_material_library_editor();
                    ui.close_menu();
                }
            });

            // ДОБАВЛЕНО (GameObject-меню — как в Unity): единственный способ
            // добавить в сцену что-либо, кроме меша (свет/звук/скрипт/пустой
            // объект), а не только примитивы — см. EditorApp::spawn_object.
            ui.menu_button("GameObject", |ui| {
                if ui.button("Create Empty").clicked() {
                    app.create_empty();
                    ui.close_menu();
                }
                if ui.button("🚩 Create Spawn Point").clicked() {
                    app.create_spawn_point();
                    ui.close_menu();
                }
                ui.menu_button("Light", |ui| {
                    if ui.button("Point Light").clicked() {
                        app.create_light("Point Light", crate::scene::LightType::Point);
                        ui.close_menu();
                    }
                    if ui.button("Directional Light").clicked() {
                        app.create_light("Directional Light", crate::scene::LightType::Directional);
                        ui.close_menu();
                    }
                    if ui.button("Spot Light").clicked() {
                        app.create_light("Spot Light", crate::scene::LightType::Spot { inner_angle: 25.0, outer_angle: 35.0 });
                        ui.close_menu();
                    }
                });
                if ui.button("Audio Source").clicked() {
                    app.create_audio_source();
                    ui.close_menu();
                }
                if ui.button("Scripted Entity").clicked() {
                    app.create_scripted_entity();
                    ui.close_menu();
                }
                if ui.button("🎥 Camera").clicked() {
                    app.create_camera();
                    ui.close_menu();
                }
                if ui.button("✨ Particle System").clicked() {
                    app.create_particle_system();
                    ui.close_menu();
                }
                ui.menu_button("3D Object", |ui| {
                    if ui.button("Cube").clicked() {
                        app.create_primitive("Cube", crate::mesh::Mesh::create_cube());
                        ui.close_menu();
                    }
                    if ui.button("Sphere").clicked() {
                        app.create_primitive("Sphere", crate::mesh::Mesh::create_sphere());
                        ui.close_menu();
                    }
                    if ui.button("Plane").clicked() {
                        app.create_primitive("Plane", crate::mesh::Mesh::create_plane());
                        ui.close_menu();
                    }
                    if ui.button("Cylinder").clicked() {
                        app.create_primitive("Cylinder", crate::mesh::Mesh::create_cylinder());
                        ui.close_menu();
                    }
                    if ui.button("Cone").clicked() {
                        app.create_primitive("Cone", crate::mesh::Mesh::create_cone());
                        ui.close_menu();
                    }
                    if ui.button("Torus").clicked() {
                        app.create_primitive("Torus", crate::mesh::Mesh::create_torus());
                        ui.close_menu();
                    }
                });
            });

            ui.separator();
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Select, "🖱").on_hover_text("Select (Q)");
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Move, "↔").on_hover_text("Move (W)");
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Rotate, "🔄").on_hover_text("Rotate (E)");
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Scale, "⤢").on_hover_text("Scale (R)");

            // ДОБАВЛЕНО (привязка к сетке для gizmo — см. EditorApp::apply_gizmo_snap):
            // применяется на отпускании кнопки мыши после перетаскивания
            // хэндла gizmo, к той оси, которую тащили.
            ui.separator();
            ui.checkbox(&mut app.snap_enabled, "Snap");
            if app.snap_enabled {
                ui.add(egui::DragValue::new(&mut app.snap_translate).speed(0.1).range(0.001..=1000.0).prefix("move: "));
                ui.add(egui::DragValue::new(&mut app.snap_rotate_deg).speed(1.0).range(0.1..=180.0).suffix("°"));
                ui.add(egui::DragValue::new(&mut app.snap_scale).speed(0.01).range(0.001..=10.0).prefix("scale: "));
            }

            ui.separator();
            ui.checkbox(&mut app.show_asset_browser, "🗀 Assets");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("FPS: {:.1}", app.fps));
            });
        });
    });
}