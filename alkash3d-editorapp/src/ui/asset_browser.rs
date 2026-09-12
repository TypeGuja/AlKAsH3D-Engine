// src/ui/asset_browser.rs
//
// ДОБАВЛЕНО (браузер ассетов по всем папкам проекта — как Project-панель в
// Unity): раньше единственный способ попасть в файл, лежащий на диске, был
// диалог File > Import (нужно точно знать путь заранее) — никакого обзора
// репозитория из самого эдитора не было. Панель рекурсивно сканирует
// заданный корень (по умолчанию — родитель рабочей директории, т.е. корень
// репозитория при обычном запуске через `cargo run` из alkash3d-editorapp/)
// и показывает дерево, отфильтрованное до форматов, которые эдитор реально
// умеет открыть (см. `IMPORTABLE_EXTS`) — не совместимость "как в Unity" с
// любым типом файла, а честный список того, что произойдёт по клику.
// Двойной клик и drag-and-drop во вьюпорт оба ведут в один и тот же
// `EditorApp::import_asset_path` (см. app.rs).

use std::path::{Path, PathBuf};

use egui::*;

const IMPORTABLE_EXTS: &[&str] = &[
    "altex", "alworld", "alfar", "obj", "fbx", "gltf", "glb", "blend",
];

const SKIP_DIR_NAMES: &[&str] = &["target", ".git", "node_modules", ".idea", ".vs", "chunks", "objects"];

#[derive(Debug, Clone)]
pub struct AssetNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<AssetNode>,
}

/// Сканирует `path` рекурсивно (ограничение глубины — защита от случайно
/// выбранного корня со сложной/циклической структурой символьных ссылок).
/// Папки без единого подходящего файла в поддереве ОТБРАСЫВАЮТСЯ — в
/// репозитории с движком на Rust большинство папок (src/, target/,
/// .git/...) не содержат ни одного `.altex`/`.alworld`/... файла, и без
/// такой обрезки дерево было бы забито пустым шумом.
fn scan_dir(path: &Path, depth: u32) -> Option<AssetNode> {
    if depth > 8 {
        return None;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    if path.is_dir() {
        if SKIP_DIR_NAMES.iter().any(|s| *s == name) || name.starts_with('.') {
            return None;
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort_by(|a, b| {
            let a_is_file = a.is_file();
            let b_is_file = b.is_file();
            a_is_file.cmp(&b_is_file).then_with(|| {
                a.file_name().unwrap_or_default().to_string_lossy().to_lowercase()
                    .cmp(&b.file_name().unwrap_or_default().to_string_lossy().to_lowercase())
            })
        });

        let children: Vec<AssetNode> = entries.iter().filter_map(|p| scan_dir(p, depth + 1)).collect();
        if children.is_empty() {
            None
        } else {
            Some(AssetNode { path: path.to_path_buf(), name, is_dir: true, children })
        }
    } else {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if IMPORTABLE_EXTS.contains(&ext.as_str()) {
            Some(AssetNode { path: path.to_path_buf(), name, is_dir: false, children: Vec::new() })
        } else {
            None
        }
    }
}

fn icon_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "altex" => "📦",
        "alworld" => "🌍",
        "alfar" => "💡",
        "obj" | "fbx" | "gltf" | "glb" | "blend" => "🧊",
        _ => "📄",
    }
}

pub fn render_asset_browser(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.show_asset_browser {
        return;
    }

    egui::TopBottomPanel::bottom("asset_browser")
        .resizable(true)
        .default_height(220.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🗀 Assets");
                ui.weak(app.asset_browser_root.display().to_string());
                if ui.button("📂 Change Root...").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        app.asset_browser_root = dir;
                        app.asset_tree = None;
                    }
                }
                if ui.button("🔄 Rescan").clicked() {
                    app.asset_tree = None;
                }
                ui.weak("Двойной клик или перетащить во вьюпорт — импорт");
            });
            ui.separator();

            if app.asset_tree.is_none() {
                app.asset_tree = scan_dir(&app.asset_browser_root, 0);
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                match app.asset_tree.clone() {
                    Some(tree) => {
                        for child in &tree.children {
                            render_node(ui, app, child);
                        }
                    }
                    None => {
                        ui.weak("Ничего не найдено (папка недоступна или в ней нет .altex/.alworld/.alfar/.obj/.fbx/.gltf/.blend)");
                    }
                }
            });
        });
}

fn render_node(ui: &mut Ui, app: &mut crate::EditorApp, node: &AssetNode) {
    if node.is_dir {
        egui::CollapsingHeader::new(format!("📁 {}", node.name))
            .default_open(false)
            .show(ui, |ui| {
                for child in &node.children {
                    render_node(ui, app, child);
                }
            });
    } else {
        let id = egui::Id::new(("asset_browser_row", &node.path));
        let inner = ui.dnd_drag_source(id, node.path.clone(), |ui| {
            ui.selectable_label(false, format!("{} {}", icon_for(&node.path), node.name))
                .on_hover_text(node.path.display().to_string())
        });
        if inner.inner.double_clicked() {
            app.import_asset_path(&node.path, None);
        }
    }
}
