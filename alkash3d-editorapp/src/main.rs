mod math;
mod animation;
mod mesh;
mod material;
mod particle;
mod scene;
mod editor;
mod systems;
mod ui;
mod assets;
mod converters;
mod gpu;
mod app;
mod memory;
mod discord_presence;

use app::EditorApp;

fn main() -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1024.0, 768.0])
            .with_title("AlKAsH3D Editor"),
        renderer: eframe::Renderer::Wgpu,  // ВКЛЮЧАЕМ GPU
        wgpu_options: wgpu_options_with_adapter_buffer_limit(),
        ..Default::default()
    };

    eframe::run_native(
        "AlKAsH3D Editor",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(EditorApp::new(cc)))
        }),
    ).map_err(|e| anyhow::anyhow!("Failed to run editor: {}", e))
}

/// Настройки wgpu-устройства eframe: всё как у egui по умолчанию, кроме
/// `max_buffer_size` — берём реальный предел адаптера вместо дефолтных
/// 256 МиБ. Весь меш-контент сцены живёт в ОДНОМ общем вершинном буфере
/// (`GpuRenderer::shared_vertex_buffer`, 36 байт на вершину), и с дефолтом
/// сцена крупнее ~7 млн вершин (например импортированный город) роняла
/// эдитор ошибкой валидации wgpu на `create_buffer`.
fn wgpu_options_with_adapter_buffer_limit() -> egui_wgpu::WgpuConfiguration {
    use egui_wgpu::{wgpu, WgpuSetup, WgpuSetupCreateNew};
    let default_setup = WgpuSetupCreateNew::default();
    let default_descriptor = default_setup.device_descriptor.clone();
    egui_wgpu::WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
            device_descriptor: std::sync::Arc::new(move |adapter: &wgpu::Adapter| {
                let mut desc = (*default_descriptor)(adapter);
                let adapter_limits = adapter.limits();
                desc.required_limits.max_buffer_size =
                    desc.required_limits.max_buffer_size.max(adapter_limits.max_buffer_size);
                desc
            }),
            ..default_setup
        }),
        ..Default::default()
    }
}
