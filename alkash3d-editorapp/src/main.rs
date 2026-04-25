mod app;

use app::EditorApp;
use eframe::egui;

fn main() -> anyhow::Result<()> {
    env_logger::init();
    println!("[Editor] Starting AlKAsH3D Editor...");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1024.0, 768.0])
            .with_title("AlKAsH3D Editor"),
        ..Default::default()
    };

    eframe::run_native(
        "AlKAsH3D Editor",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::new(EditorApp::new(cc))
        }),
    ).map_err(|e| anyhow::anyhow!("Failed to run editor: {}", e))
}