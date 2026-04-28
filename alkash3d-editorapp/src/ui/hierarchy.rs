use egui::*;
use uuid::Uuid;

pub fn render_hierarchy(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.show_hierarchy { return; }

    egui::SidePanel::left("hierarchy")
        .default_width(250.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("📁 Hierarchy");
            ui.separator();

            let mut to_select = None;
            let mut to_toggle = None;

            let visible_objects: Vec<(Uuid, String, bool, bool)> = app.scene.objects.iter()
                .filter(|(_, o)| app.search_filter.is_empty() ||
                    o.name.to_lowercase().contains(&app.search_filter.to_lowercase()))
                .map(|(&id, o)| (id, o.name.clone(), o.visible, app.scene.selected_ids.contains(&id)))
                .collect();

            for (id, name, vis, sel) in visible_objects {
                ui.horizontal(|ui| {
                    if ui.selectable_label(false, if vis { "👁" } else { "👁‍🗨" }).clicked() {
                        to_toggle = Some(id);
                    }
                    if ui.selectable_label(sel, &name).clicked() {
                        to_select = Some(id);
                    }
                });
            }

            if let Some(id) = to_select {
                let add = ctx.input(|i| i.modifiers.shift);
                app.scene.select(id, add);
            }
            if let Some(id) = to_toggle {
                if let Some(obj) = app.scene.get_object_mut(id) {
                    obj.visible = !obj.visible;
                }
            }
        });
}