//! Панели редактора

use egui::*;

pub struct InspectorPanel;

impl InspectorPanel {
    pub fn show<R>(
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R> {
        egui::SidePanel::right("inspector")
            .default_width(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Inspector");
                ui.separator();
                add_contents(ui)
            })
    }
}

pub struct HierarchyPanel;

impl HierarchyPanel {
    pub fn show<R>(
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R> {
        egui::SidePanel::left("hierarchy")
            .default_width(250.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Hierarchy");
                ui.separator();
                add_contents(ui)
            })
    }
}

pub struct AssetBrowserPanel;

impl AssetBrowserPanel {
    pub fn show<R>(
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R> {
        egui::SidePanel::left("asset_browser")
            .default_width(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Asset Browser");
                ui.separator();
                add_contents(ui)
            })
    }
}

pub struct ConsolePanel;

impl ConsolePanel {
    pub fn show<R>(
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R> {
        egui::TopBottomPanel::bottom("console")
            .default_height(200.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Console");
                ui.separator();
                add_contents(ui)
            })
    }
}