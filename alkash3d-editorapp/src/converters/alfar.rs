// src/converters/alfar.rs
//
// Конвертация между Light-объектами сцены эдитора и настоящим форматом
// освещения движка `.alfar` (alkash3d_rs::AlfarFile, см.
// alkash3d-rust/src/alfar_format.rs).
//
// ИЗВЕСТНОЕ ОГРАНИЧЕНИЕ: `IndividualLight` несёт `direction`/`up` (мировые
// векторы), а `LightComponent` эдитора направления не хранит явно — оно
// выражается через `GameObject::transform.rotation`. При экспорте
// направление вычисляется из поворота объекта (`rotation.forward()`/`up()`),
// но при ИМПОРТЕ обратное преобразование (direction+up -> Quat) не
// делается: импортированные Point-светильники (у которых направление всё
// равно не используется на рендере) получают верные позицию/цвет/
// интенсивность, а импортированные Spot/Directional светильники получают
// identity-поворот — их направление нужно поправить в инспекторе вручную
// после импорта. Это единственная урезанная часть конвертации;
// исправление потребовало бы устойчивого "look rotation" конструктора в
// `math::Quat`, которого сейчас в эдиторе нет — лучше явно предупредить в
// логе, чем тихо подсунуть неверный, но правдоподобно выглядящий поворот.

use anyhow::{anyhow, Result};

use crate::scene::{GameObject, LightComponent, LightType as EditorLightType, ObjectType, Scene};

use alkash3d_rs::{AlfarFile, IndividualLight};

fn editor_light_type_to_u32(lt: &EditorLightType) -> (u32, f32, f32) {
    match lt {
        EditorLightType::Point => (0, 0.0, 0.0),
        EditorLightType::Spot { inner_angle, outer_angle } => (1, *inner_angle, *outer_angle),
        EditorLightType::Directional => (2, 0.0, 0.0),
    }
}

fn alfar_light_type_to_editor(light_type: u32, inner: f32, outer: f32) -> EditorLightType {
    match light_type {
        1 => EditorLightType::Spot { inner_angle: inner, outer_angle: outer },
        2 => EditorLightType::Directional,
        // Point (0) и Area (3, эдитор его не поддерживает — сводим к Point,
        // единственному типу без направленности, ближайшему по смыслу).
        _ => EditorLightType::Point,
    }
}

/// Строит `AlfarFile` из ambient-цвета сцены и всех включённых
/// Light-объектов сцены. Группы/анимации света эдитор пока не
/// редактирует — файл получает только `ambient`/`global_settings` (дефолты
/// `AlfarFile::new()`) и плоский список `lights`.
pub fn export_scene_to_alfar(scene: &Scene) -> AlfarFile {
    let mut alfar = AlfarFile::new();
    alfar.ambient.color = scene.ambient_color;

    for obj in scene.objects.values() {
        let light = match &obj.object_type {
            ObjectType::Light(l) => l,
            _ => continue,
        };

        let (light_type, spot_inner, spot_outer) = editor_light_type_to_u32(&light.light_type);
        // ИСПРАВЛЕНО (по прямому запросу пользователя: свет, добавленный
        // на выбранный объект — т.е. созданный как его РЕБЁНОК, см.
        // `EditorApp::spawn_object` — должен получать ту же высоту, что и
        // этот объект) — `obj.transform` тут ЛОКАЛЬНЫЙ (см. комментарий у
        // `GameObject::parent`), а .alfar читает мировые координаты
        // напрямую движком, без какой-либо иерархии. Раньше сюда шёл
        // именно локальный transform, так что дочерний свет с локальной
        // позицией (0,0,0) экспортировался в мировой origin вместо высоты
        // родителя. `Scene::get_world_transform` поднимается по цепочке
        // `parent` и для объекта без родителя возвращает тот же результат,
        // что и раньше.
        let world = scene.get_world_transform(obj.id);
        let forward = world.forward();
        let up = world.up();

        let record = IndividualLight {
            id: 0,        // перезаписывается AlfarFile::add_light
            name_id: 0,   // перезаписывается AlfarFile::add_light
            light_type,
            position: [world.position.x, world.position.y, world.position.z],
            direction: [forward.x, forward.y, forward.z],
            up: [up.x, up.y, up.z],
            color: light.color,
            intensity: light.intensity,
            range: light.range,
            falloff_type: 1, // Quadratic — физически правдоподобный дефолт
            falloff_custom: 0.0,
            spot_inner_angle: spot_inner,
            spot_outer_angle: spot_outer,
            casts_shadows: 1,
            shadow_bias: 0.005,
            shadow_resolution: 1024,
            flicker_enabled: 0,
            flicker_speed: 0.0,
            flicker_intensity: 0.0,
            enabled: if light.enabled { 1 } else { 0 },
            active_from: 0.0,
            active_to: 24.0,
            has_physics: 0,
            breakable: 0,
            health: 0.0,
            custom_data_offset: 0,
        };

        alfar.add_light(record, &obj.name);
    }

    alfar
}

/// Сохраняет `.alfar` для текущей сцены. `save()` в `AlfarFile` сейчас не
/// сериализует группы/анимации (см. комментарий у `AlfarFile::save` в
/// движке) — это ограничение самого формата на сегодня, не эдитора.
pub fn export_scene_to_alfar_file(scene: &Scene, path: &str) -> Result<()> {
    let alfar = export_scene_to_alfar(scene);
    alfar
        .save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alfar '{}': {}", path, e))
}

/// Строит `LightComponent` из одной записи `.alfar` — общий код между
/// `import_alfar_to_scene` (весь файл разом, новая сцена) и точечным
/// назначением ОДНОГО светильника уже существующему Light-объекту (см.
/// `load_lights_for_picker` ниже и кнопку "📂 Load from .alfar..." в
/// `ui/inspector.rs`).
fn individual_light_to_component(light: &IndividualLight) -> LightComponent {
    LightComponent {
        light_type: alfar_light_type_to_editor(light.light_type, light.spot_inner_angle, light.spot_outer_angle),
        color: light.color,
        intensity: light.intensity,
        range: light.range,
        enabled: light.enabled != 0,
        color_group: None,
    }
}

/// Импортирует все источники света из `.alfar` как Light-объекты новой
/// сцены (ambient сцены берётся из `AlfarFile::ambient`). Direction/up не
/// восстанавливаются как поворот объекта — см. комментарий в шапке файла.
pub fn import_alfar_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let alfar = AlfarFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alfar '{}': {}", path, e))?;

    let name = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Lighting".to_string());
    let mut scene = Scene::new(&name);
    scene.ambient_color = alfar.ambient.color;

    let mut directional_or_spot_count = 0;
    for light in &alfar.lights {
        let component = individual_light_to_component(light);
        if !matches!(component.light_type, EditorLightType::Point) {
            directional_or_spot_count += 1;
        }

        let light_name = alfar
            .strings
            .get(light.name_id as usize)
            .cloned()
            .unwrap_or_else(|| "Light".to_string());

        let mut obj = GameObject::new(&light_name, ObjectType::Light(component));
        obj.transform.position = crate::math::Vec3::new(light.position[0], light.position[1], light.position[2]);
        scene.add_object(obj);
    }

    if directional_or_spot_count > 0 {
        log(format!(
            "⚠️ {} источник(ов) света Spot/Directional импортированы с направлением по умолчанию — поправьте поворот в инспекторе (см. комментарий в converters/alfar.rs)",
            directional_or_spot_count
        ));
    }

    Ok(scene)
}

/// Читает список источников света `.alfar` для точечного выбора — НЕ
/// создаёт объекты и не трогает сцену (в отличие от `import_alfar_to_scene`
/// выше), только возвращает (имя, параметры) для каждой записи, чтобы
/// вызывающий код (инспектор) сам решил, какую применить к уже
/// существующему Light-объекту. Позиция намеренно не возвращается — при
/// точечном назначении объект остаётся там, где его разместили в сцене.
pub fn load_lights_for_picker(path: &str) -> Result<Vec<(String, LightComponent)>> {
    let alfar = AlfarFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alfar '{}': {}", path, e))?;
    let out = alfar
        .lights
        .iter()
        .map(|light| {
            let name = alfar
                .strings
                .get(light.name_id as usize)
                .cloned()
                .unwrap_or_else(|| "Light".to_string());
            (name, individual_light_to_component(light))
        })
        .collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::GameObject;

    #[test]
    fn round_trip_preserves_light_properties() {
        let mut scene = Scene::new("TestLighting");
        scene.ambient_color = [0.1, 0.15, 0.2];
        let mut obj = GameObject::new(
            "StreetLamp",
            ObjectType::Light(LightComponent {
                light_type: EditorLightType::Point,
                color: [1.0, 0.8, 0.6],
                intensity: 3.5,
                range: 25.0,
                enabled: true,
                color_group: None,
            }),
        );
        obj.transform.position = crate::math::Vec3::new(1.0, 2.0, 3.0);
        scene.add_object(obj);

        let path = std::env::temp_dir().join("alkash3d_editor_alfar_roundtrip_test.alfar");
        let path_str = path.to_string_lossy().to_string();

        export_scene_to_alfar_file(&scene, &path_str).expect("export");
        let mut logs = Vec::new();
        let imported = import_alfar_to_scene(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.objects.len(), 1, "logs: {:?}", logs);
        assert!((imported.ambient_color[0] - 0.1).abs() < 1e-5);
        let light_obj = imported.objects.values().next().unwrap();
        let ObjectType::Light(light) = &light_obj.object_type else { panic!("expected light") };
        assert!((light.intensity - 3.5).abs() < 1e-5);
        assert!((light.range - 25.0).abs() < 1e-5);
        assert!((light_obj.transform.position.x - 1.0).abs() < 1e-5);
    }
}
