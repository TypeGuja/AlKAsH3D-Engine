use egui::*;

pub fn render_status_bar(ctx: &egui::Context, app: &crate::EditorApp) {
    egui::TopBottomPanel::bottom("status_bar")
        .default_height(26.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("🎯 {:?}", app.current_tool));
                ui.separator();
                ui.label(format!("📦 {} objects", app.scene.objects.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(&app.status_message);
                });
            });
        });
}