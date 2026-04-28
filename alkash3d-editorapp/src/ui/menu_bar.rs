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
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });

            ui.separator();
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Select, "🖱");
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Move, "↔");
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Rotate, "🔄");
            ui.selectable_value(&mut app.current_tool, crate::editor::EditorTool::Scale, "⤢");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("FPS: {:.1}", app.fps));
            });
        });
    });
}