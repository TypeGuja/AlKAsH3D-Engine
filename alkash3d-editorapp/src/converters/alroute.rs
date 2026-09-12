// src/converters/alroute.rs
//
// Экспорт AI-маршрута из позиций выделенных объектов сцены в настоящий
// `.alroute` движка (alkash3d_rs::AlrouteFile) — порядок точек маршрута
// берётся из порядка выделения (`Scene::selected_ids`, сохраняет порядок
// клика, см. `Scene::select`), а не из порядка обхода `HashMap<Uuid,
// GameObject>` (тот недетерминирован между запусками).

use anyhow::{anyhow, Result};

use crate::scene::Scene;

use alkash3d_rs::{AlrouteFile, Waypoint};

/// `loop_type`: 0 = разомкнутый маршрут (доехать и остановиться), 1 = замкнутый
/// (вернуться к первой точке) — та же семантика, что ожидает движок в
/// `AlrouteFile::add_route` (третий параметр передаётся как есть).
pub fn export_selection_to_alroute(scene: &Scene, route_name: &str, loop_type: u32, path: &str) -> Result<usize> {
    let waypoints: Vec<Waypoint> = scene
        .selected_ids
        .iter()
        .filter_map(|id| scene.get_object(*id))
        .map(|obj| {
            let p = obj.transform.position;
            Waypoint {
                position: [p.x, p.y, p.z],
                wait_time: 0.0,
                speed_limit: 0.0, // 0.0 = без ограничения (доверяем speed_factor маршрута)
                action_id: 0xFFFF_FFFF,
            }
        })
        .collect();

    if waypoints.len() < 2 {
        return Err(anyhow!("Нужно выделить минимум 2 объекта, чтобы построить маршрут (выделено: {})", waypoints.len()));
    }

    let mut file = AlrouteFile::new();
    file.add_route(route_name, &waypoints, loop_type);
    file.save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alroute '{}': {}", path, e))?;

    Ok(waypoints.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::scene::GameObject;

    #[test]
    fn export_then_load_preserves_waypoints() {
        let mut scene = Scene::new("TestRoute");
        let mut obj1 = GameObject::new("WP1", crate::scene::ObjectType::Empty);
        obj1.transform.position = Vec3::new(1.0, 0.0, 2.0);
        let id1 = scene.add_object(obj1);
        let mut obj2 = GameObject::new("WP2", crate::scene::ObjectType::Empty);
        obj2.transform.position = Vec3::new(5.0, 0.0, 6.0);
        let id2 = scene.add_object(obj2);
        scene.select(id1, false);
        scene.select(id2, true);

        let path = std::env::temp_dir().join("alkash3d_editor_alroute_test.alroute");
        let path_str = path.to_string_lossy().to_string();

        let count = export_selection_to_alroute(&scene, "TestRoute", 0, &path_str).expect("export");
        assert_eq!(count, 2);

        let loaded = AlrouteFile::load(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.routes.len(), 1);
        assert_eq!(loaded.waypoints.len(), 2);
        assert!((loaded.waypoints[0].position[0] - 1.0).abs() < 1e-5);
        assert!((loaded.waypoints[1].position[0] - 5.0).abs() < 1e-5);
    }
}
