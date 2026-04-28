use egui::*;

pub fn setup_egui_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(35, 35, 40);
    style.visuals.panel_fill = Color32::from_rgb(30, 30, 35);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(255, 120, 30);
    style.visuals.selection.bg_fill = Color32::from_rgb(255, 120, 30);
    ctx.set_style(style);
}