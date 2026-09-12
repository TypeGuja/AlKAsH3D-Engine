// src/converters/alcar.rs
//
// Экспорт конфигурации машины в настоящий `.alcar` движка
// (alkash3d_rs::AlcarFile). Сцена эдитора не моделирует машину как
// отдельный тип объекта (нет `ObjectType::Car`) — экспорт работает с
// готовыми пресетами движка (`AlcarFile::new/create_sports_car/
// create_police_car`), которые пользователь дальше донастраивает
// (`mesh_path` и т.п.) вручную или через будущий редактор; это честная
// граница текущего охвата — см. обсуждение в сессии (сохранение/экспорт в
// реальные форматы важнее выделенных под-редакторов по типам данных).

use anyhow::{anyhow, Result};

use alkash3d_rs::AlcarFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarPreset {
    Default,
    Sports,
    Police,
}

impl CarPreset {
    pub fn label(&self) -> &'static str {
        match self {
            CarPreset::Default => "Default",
            CarPreset::Sports => "Sports Car",
            CarPreset::Police => "Police Car",
        }
    }

    fn build(&self) -> AlcarFile {
        match self {
            CarPreset::Default => AlcarFile::new(),
            CarPreset::Sports => AlcarFile::create_sports_car(),
            CarPreset::Police => AlcarFile::create_police_car(),
        }
    }
}

pub fn export_car_preset(preset: CarPreset, mesh_path: &str, path: &str) -> Result<()> {
    let mut car = preset.build();
    if !mesh_path.is_empty() {
        car.set_mesh(mesh_path);
    }
    car.save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alcar '{}': {}", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_then_load_preserves_sports_preset() {
        let path = std::env::temp_dir().join("alkash3d_editor_alcar_test.alcar");
        let path_str = path.to_string_lossy().to_string();

        export_car_preset(CarPreset::Sports, "meshes/sports.altex", &path_str).expect("export");
        let loaded = AlcarFile::load(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert!((loaded.physics.engine_power - 500.0).abs() < 1e-3);
        assert_eq!(loaded.mesh_path, "meshes/sports.altex");
    }
}
