// src/assets/groups.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя): два независимых, но
// хранящихся в одном файле проекта состояния UI-эдитора, которые раньше
// вообще не переживали перезапуск:
//
// 1. `AssetGroup` — именованная, сворачиваемая группа объектов сцены в
//    панели Hierarchy (см. ui/hierarchy.rs). Группа хранит имена объектов
//    (`member_names`), а НЕ их `Uuid` — каждый импорт/загрузка сцены
//    (`GameObject::new`) генерирует новый `Uuid::new_v4()`, так что Uuid не
//    переживает даже простой цикл "сохранить .alworld -> открыть заново",
//    а имя объекта в этом смысле куда стабильнее.
// 2. `LightColorGroup` — переиспользуемый "оттенок" света (цвет+
//    интенсивность) с числовым ID: светильник хранит только
//    `LightComponent::color_group` (см. scene/object_type.rs), а не сам
//    цвет заново каждый раз — назначить существующий оттенок можно через
//    выпадающий список в инспекторе (ui/inspector.rs), без диалога открытия
//    файла. Правка цвета в самой группе применяется сразу ко всем
//    светильникам с этим ID (см. `EditorApp::sync_light_color_group`).
//
// Файл хранится РЯДОМ С ПРОЕКТОМ (`EditorApp::asset_browser_root`), а не в
// глобальных настройках эдитора — группировка объектов имеет смысл только
// в контексте конкретной сцены/проекта, у разных проектов будут разные
// группы.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetGroup {
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub member_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightColorGroup {
    pub name: String,
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EditorGroupsFile {
    #[serde(default)]
    pub asset_groups: Vec<AssetGroup>,
    #[serde(default)]
    pub light_color_groups: HashMap<u32, LightColorGroup>,
    #[serde(default)]
    pub next_light_color_group_id: u32,
}

impl EditorGroupsFile {
    /// Тихо возвращает пустое состояние на любую ошибку (файла нет — первый
    /// запуск на этом проекте, повреждён — лучше стартовать с чистого листа,
    /// чем падать при открытии эдитора).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, json)
    }
}
