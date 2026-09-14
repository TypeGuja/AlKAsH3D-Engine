// src/converters/alroute.rs
//
// Экспорт AI-маршрута из позиций выделенных объектов сцены в настоящий
// `.alroute` движка (alkash3d_rs::AlrouteFile) — порядок точек маршрута
// берётся из порядка выделения (`Scene::selected_ids`, сохраняет порядок
// клика, см. `Scene::select`), а не из порядка обхода `HashMap<Uuid,
// GameObject>` (тот недетерминирован между запусками).

use anyhow::{anyhow, Result};

use crate::math::Vec3;
use crate::scene::{GameObject, ObjectType, Scene};

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

// ДОБАВЛЕНО (по прямому запросу пользователя: "делай то же самое под
// другие форматы" — после Sound Bank Editor, см. `converters/alsnd.rs`):
// автономная редактируемая модель маршрута(ов), не привязанная к
// `Scene`/выделению объектов — см. `ui/route_editor.rs`.
#[derive(Debug, Clone)]
pub struct WaypointEdit {
    pub position: [f32; 3],
    pub wait_time: f32,
    pub speed_limit: f32,
}

impl Default for WaypointEdit {
    fn default() -> Self {
        Self { position: [0.0, 0.0, 0.0], wait_time: 0.0, speed_limit: 0.0 }
    }
}

#[derive(Debug, Clone)]
pub struct RouteEdit {
    pub name: String,
    /// 0 = разомкнутый, 1 = замкнутый — см. `export_selection_to_alroute`.
    pub loop_type: u32,
    pub speed_factor: f32,
    pub start_delay: f32,
    pub waypoints: Vec<WaypointEdit>,
}

impl Default for RouteEdit {
    fn default() -> Self {
        Self { name: String::new(), loop_type: 0, speed_factor: 1.0, start_delay: 0.0, waypoints: Vec::new() }
    }
}

pub fn build_alroute_file(routes: &[RouteEdit]) -> AlrouteFile {
    let mut file = AlrouteFile::new();
    for r in routes {
        let name = r.name.trim();
        if name.is_empty() || r.waypoints.is_empty() {
            continue;
        }
        let wps: Vec<Waypoint> = r.waypoints.iter().map(|w| Waypoint {
            position: w.position,
            wait_time: w.wait_time,
            speed_limit: w.speed_limit,
            action_id: 0xFFFF_FFFF,
        }).collect();
        let idx = file.add_route(name, &wps, r.loop_type) as usize;
        if let Some(route) = file.routes.get_mut(idx) {
            route.speed_factor = r.speed_factor;
            route.start_delay = r.start_delay;
        }
    }
    file
}

pub fn save_routes(routes: &[RouteEdit], path: &str) -> Result<usize> {
    let file = build_alroute_file(routes);
    let count = file.routes.len();
    if count == 0 {
        return Err(anyhow!("Нет ни одного маршрута с именем и точками — сохранять нечего"));
    }
    file.save(path).map_err(|e| anyhow!("Не удалось сохранить .alroute '{}': {}", path, e))?;
    Ok(count)
}

pub fn load_routes(path: &str) -> Result<Vec<RouteEdit>> {
    let file = AlrouteFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alroute '{}': {}", path, e))?;
    let mut routes = Vec::with_capacity(file.routes.len());
    for r in &file.routes {
        let start = r.waypoint_start as usize;
        let count = r.waypoint_count as usize;
        let wps = file.waypoints.get(start..start.saturating_add(count)).unwrap_or(&[]);
        routes.push(RouteEdit {
            name: file.get_string(r.name_id).to_string(),
            loop_type: r.loop_type,
            speed_factor: r.speed_factor,
            start_delay: r.start_delay,
            waypoints: wps.iter().map(|w| WaypointEdit {
                position: w.position,
                wait_time: w.wait_time,
                speed_limit: w.speed_limit,
            }).collect(),
        });
    }
    Ok(routes)
}

/// Импорт `.alroute` — зеркально экспорту выше (позиции выделенных
/// объектов -> waypoints): на каждый маршрут файла создаётся корневой
/// `Empty` (имя маршрута) и по одному дочернему `Empty` на точку (`wp_N`,
/// `Scene::set_parent`), позиция ребёнка = `Waypoint::position`. Родитель
/// стоит в мировом нуле, так что дочерняя мировая позиция после
/// `set_parent` в точности равна позиции из файла (см. `Scene::set_parent`
/// — композиция трансформов).
pub fn import_alroute_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let file = AlrouteFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alroute '{}': {}", path, e))?;
    if file.routes.is_empty() {
        return Err(anyhow!("В '{}' нет маршрутов", path));
    }

    let mut scene = Scene::new("ImportedRoutes");
    for route in &file.routes {
        let name = file.get_string(route.name_id);
        let root = GameObject::new(if name.is_empty() { "Route" } else { name }, ObjectType::Empty);
        let root_id = scene.add_object(root);

        let start = route.waypoint_start as usize;
        let count = route.waypoint_count as usize;
        let Some(wps) = file.waypoints.get(start..start.saturating_add(count)) else {
            log(format!("⚠️ Маршрут '{}': waypoint_start/count за пределами файла — пропущен", name));
            continue;
        };
        for (i, wp) in wps.iter().enumerate() {
            let mut child = GameObject::new(&format!("wp_{}", i), ObjectType::Empty);
            child.transform.position = Vec3::new(wp.position[0], wp.position[1], wp.position[2]);
            let child_id = scene.add_object(child);
            if let Err(e) = scene.set_parent(child_id, Some(root_id)) {
                log(format!("⚠️ Не удалось прикрепить wp_{} к маршруту '{}': {}", i, name, e));
            }
        }
    }
    Ok(scene)
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

    #[test]
    fn import_recreates_waypoint_hierarchy() {
        let mut scene = Scene::new("TestRoute");
        let mut obj1 = GameObject::new("WP1", crate::scene::ObjectType::Empty);
        obj1.transform.position = Vec3::new(1.0, 0.0, 2.0);
        let id1 = scene.add_object(obj1);
        let mut obj2 = GameObject::new("WP2", crate::scene::ObjectType::Empty);
        obj2.transform.position = Vec3::new(5.0, 0.0, 6.0);
        let id2 = scene.add_object(obj2);
        scene.select(id1, false);
        scene.select(id2, true);

        let path = std::env::temp_dir().join("alkash3d_editor_alroute_import_test.alroute");
        let path_str = path.to_string_lossy().to_string();
        export_selection_to_alroute(&scene, "TestRoute", 0, &path_str).expect("export");

        let mut logs = Vec::new();
        let imported = import_alroute_to_scene(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.objects.len(), 3, "1 root + 2 waypoints, logs: {:?}", logs); // root + 2 wp
        let root = imported.objects.values().find(|o| o.parent.is_none()).expect("root");
        assert_eq!(root.name, "TestRoute");
        let children = imported.children_of(Some(root.id));
        assert_eq!(children.len(), 2);
        let positions: Vec<Vec3> = children.iter().map(|id| imported.get_world_transform(*id).position).collect();
        assert!(positions.iter().any(|p| (*p - Vec3::new(1.0, 0.0, 2.0)).length() < 1e-4));
        assert!(positions.iter().any(|p| (*p - Vec3::new(5.0, 0.0, 6.0)).length() < 1e-4));
    }

    #[test]
    fn route_editor_round_trip() {
        let routes = vec![RouteEdit {
            name: "Patrol".to_string(),
            loop_type: 1,
            speed_factor: 1.5,
            start_delay: 2.0,
            waypoints: vec![
                WaypointEdit { position: [0.0, 0.0, 0.0], wait_time: 1.0, speed_limit: 10.0 },
                WaypointEdit { position: [10.0, 0.0, 5.0], wait_time: 0.0, speed_limit: 20.0 },
            ],
        }];

        let path = std::env::temp_dir().join("alkash3d_editor_route_editor_test.alroute");
        let path_str = path.to_string_lossy().to_string();
        let count = save_routes(&routes, &path_str).expect("save");
        assert_eq!(count, 1);

        let loaded = load_routes(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Patrol");
        assert_eq!(loaded[0].loop_type, 1);
        assert!((loaded[0].speed_factor - 1.5).abs() < 1e-5);
        assert_eq!(loaded[0].waypoints.len(), 2);
        assert!((loaded[0].waypoints[1].position[0] - 10.0).abs() < 1e-5);
    }
}
