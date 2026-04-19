//! Иконки для редактора

use egui::*;

pub const ICON_CUBE: &str = "📦";
pub const ICON_SPHERE: &str = "⚪";
pub const ICON_CAMERA: &str = "📷";
pub const ICON_LIGHT: &str = "💡";
pub const ICON_MESH: &str = "🔷";
pub const ICON_EMPTY: &str = "📌";
pub const ICON_FILE: &str = "📄";
pub const ICON_FOLDER: &str = "📁";
pub const ICON_PLAY: &str = "▶";
pub const ICON_STOP: &str = "⏹";
pub const ICON_PAUSE: &str = "⏸";
pub const ICON_SAVE: &str = "💾";
pub const ICON_OPEN: &str = "📂";
pub const ICON_NEW: &str = "✨";
pub const ICON_DELETE: &str = "🗑";
pub const ICON_DUPLICATE: &str = "📋";
pub const ICON_UNDO: &str = "↩";
pub const ICON_REDO: &str = "↪";
pub const ICON_SETTINGS: &str = "⚙";
pub const ICON_SEARCH: &str = "🔍";
pub const ICON_FILTER: &str = "🔽";
pub const ICON_VISIBLE: &str = "👁";
pub const ICON_HIDDEN: &str = "👁‍🗨";
pub const ICON_LOCKED: &str = "🔒";
pub const ICON_UNLOCKED: &str = "🔓";

pub fn icon_button(ui: &mut Ui, icon: &str) -> egui::Response {
    ui.button(icon)
}

pub fn icon_selectable(ui: &mut Ui, icon: &str, selected: bool) -> egui::Response {
    ui.selectable_label(selected, icon)
}

pub fn object_icon(object_type: &str) -> &'static str {
    match object_type {
        "camera" => ICON_CAMERA,
        "light" => ICON_LIGHT,
        "mesh" => ICON_MESH,
        "empty" => ICON_EMPTY,
        _ => ICON_CUBE,
    }
}