use egui::*;

pub fn render_inspector(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.show_inspector { return; }

    egui::SidePanel::right("inspector")
        .default_width(300.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("🔧 Inspector");
            ui.separator();

            let selected = app.scene.selected_objects();
            if selected.len() == 1 {
                let id = selected[0].id;
                let mut obj = app.scene.get_object(id).cloned();

                if let Some(ref mut o) = obj {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut o.name);
                    });

                    ui.collapsing("Transform", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(egui::DragValue::new(&mut o.transform.position.x).speed(0.1));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Y");
                            ui.add(egui::DragValue::new(&mut o.transform.position.y).speed(0.1));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Z");
                            ui.add(egui::DragValue::new(&mut o.transform.position.z).speed(0.1));
                        });
                    });

                    match &mut o.object_type {
                        crate::scene::ObjectType::Mesh(ref mut m) => {
                            ui.checkbox(&mut m.wireframe, "Wireframe");
                            ui.checkbox(&mut m.solid, "Solid");
                        }
                        crate::scene::ObjectType::Light(ref mut l) => {
                            ui.checkbox(&mut l.enabled, "Enabled");
                            ui.add(egui::Slider::new(&mut l.intensity, 0.0..=10.0).text("Intensity"));
                        }
                        _ => {}
                    }
                }

                if let Some(o) = obj {
                    if let Some(orig) = app.scene.get_object_mut(id) {
                        *orig = o;
                    }
                }

                if ui.button("🗑 Remove").clicked() {
                    app.scene.remove_object(id);
                }
            } else {
                let label_text: String = if selected.is_empty() {
                    "No object selected".to_string()
                } else {
                    format!("{} objects selected", selected.len())
                };
                ui.label(&label_text);
            }
        });
}