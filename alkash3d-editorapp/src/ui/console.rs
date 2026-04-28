use egui::*;

pub fn render_console(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.show_console { return; }

    egui::TopBottomPanel::bottom("console")
        .default_height(150.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("💬 Console");
                if ui.button("Clear").clicked() {
                    app.console_messages.clear();
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                for (msg, col) in &app.console_messages {
                    ui.colored_label(*col, msg);
                }
            });
        });
}