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

use crate::math::Vec3;
use crate::scene::{GameObject, LightComponent, LightType, MeshComponent, ObjectType, Scene};
use uuid::Uuid;

use alkash3d_rs::{AlcarFile, CarLight};

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

// ДОБАВЛЕНО (по прямому запросу пользователя: "делай то же самое под
// другие форматы", см. `converters/alsnd.rs` про паттерн): автономная
// редактируемая модель машины — в отличие от `CarPreset`/`export_car_preset`
// выше (готовые пресеты движка, донастраиваемые только через `set_mesh`),
// здесь можно менять КАЖДОЕ поле напрямую, см. `ui/car_preset_editor.rs`.
/// Значения по умолчанию — те же, что у `AlcarFile::new()` в движке (см.
/// `alcar_format.rs`), чтобы "Create New" в редакторе стартовал с того же
/// разумного пресета, что и голый экспорт.
#[derive(Debug, Clone)]
pub struct CarPresetEdit {
    pub brand: String,
    pub model: String,
    pub mesh_path: String,
    pub year: u32,
    pub price: u32,
    pub fuel_consumption: f32,
    pub fuel_tank: f32,
    /// Свободный числовой код категории/редкости — движок сам не
    /// навязывает enum для этих двух полей (см. `CarMetadata`).
    pub category: u32,
    pub rarity: u32,
    pub engine_power: f32,
    pub torque: f32,
    pub max_rpm: f32,
    pub idle_rpm: f32,
    pub gears: u32,
    pub final_drive: f32,
    pub weight: f32,
    pub wheel_radius: f32,
    pub suspension_stiffness: f32,
    pub suspension_damping: f32,
    pub brake_power: f32,
    pub handbrake_power: f32,
    pub steering_angle: f32,
    pub turning_radius: f32,
    pub drag_coefficient: f32,
    pub downforce: f32,
    pub top_speed: f32,
    pub acceleration_0_100: f32,
    /// Заглушка-приближение (0-4, ограничено размером `[CarLight; 4]` у
    /// движка): реальные позиции/цвета фар редактор пока не выставляет —
    /// см. `import_alcar_to_scene` ниже, там то же ограничение честно
    /// откомментировано у `add_car_lights`.
    pub headlight_count: u32,
    pub taillight_count: u32,
    pub blinker_count: u32,
}

impl Default for CarPresetEdit {
    fn default() -> Self {
        Self {
            brand: String::new(), model: String::new(), mesh_path: String::new(),
            year: 2024, price: 25000, fuel_consumption: 8.0, fuel_tank: 55.0, category: 0, rarity: 0,
            engine_power: 150.0, torque: 200.0, max_rpm: 6500.0, idle_rpm: 800.0, gears: 6,
            final_drive: 3.5, weight: 1500.0, wheel_radius: 0.33,
            suspension_stiffness: 25000.0, suspension_damping: 2000.0,
            brake_power: 8000.0, handbrake_power: 5000.0,
            steering_angle: 35.0, turning_radius: 5.5,
            drag_coefficient: 0.3, downforce: 50.0,
            top_speed: 220.0, acceleration_0_100: 8.5,
            headlight_count: 2, taillight_count: 2, blinker_count: 4,
        }
    }
}

pub fn build_alcar_file(edit: &CarPresetEdit) -> AlcarFile {
    let mut file = AlcarFile::new();
    if !edit.mesh_path.trim().is_empty() {
        file.set_mesh(edit.mesh_path.trim());
    }
    if !edit.brand.trim().is_empty() {
        file.metadata.brand_id = file.add_string(edit.brand.trim());
    }
    if !edit.model.trim().is_empty() {
        file.metadata.model_id = file.add_string(edit.model.trim());
    }
    file.metadata.year = edit.year;
    file.metadata.price = edit.price;
    file.metadata.fuel_consumption = edit.fuel_consumption;
    file.metadata.fuel_tank = edit.fuel_tank;
    file.metadata.category = edit.category;
    file.metadata.rarity = edit.rarity;

    file.physics.engine_power = edit.engine_power;
    file.physics.torque = edit.torque;
    file.physics.max_rpm = edit.max_rpm;
    file.physics.idle_rpm = edit.idle_rpm;
    file.physics.gears = edit.gears;
    file.physics.final_drive = edit.final_drive;
    file.physics.weight = edit.weight;
    file.physics.wheel_radius = edit.wheel_radius;
    file.physics.suspension_stiffness = edit.suspension_stiffness;
    file.physics.suspension_damping = edit.suspension_damping;
    file.physics.brake_power = edit.brake_power;
    file.physics.handbrake_power = edit.handbrake_power;
    file.physics.steering_angle = edit.steering_angle;
    file.physics.turning_radius = edit.turning_radius;
    file.physics.drag_coefficient = edit.drag_coefficient;
    file.physics.downforce = edit.downforce;
    file.physics.top_speed = edit.top_speed;
    file.physics.acceleration_0_100 = edit.acceleration_0_100;

    file.lights.headlight_count = edit.headlight_count.min(4);
    file.lights.taillight_count = edit.taillight_count.min(4);
    file.lights.blinker_count = edit.blinker_count.min(4);

    file
}

pub fn save_car_preset(edit: &CarPresetEdit, path: &str) -> Result<()> {
    let file = build_alcar_file(edit);
    file.save(path).map_err(|e| anyhow!("Не удалось сохранить .alcar '{}': {}", path, e))
}

pub fn load_car_preset(path: &str) -> Result<CarPresetEdit> {
    let file = AlcarFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alcar '{}': {}", path, e))?;
    Ok(CarPresetEdit {
        brand: file.get_string(file.metadata.brand_id).to_string(),
        model: file.get_string(file.metadata.model_id).to_string(),
        mesh_path: file.mesh_path.clone(),
        year: file.metadata.year,
        price: file.metadata.price,
        fuel_consumption: file.metadata.fuel_consumption,
        fuel_tank: file.metadata.fuel_tank,
        category: file.metadata.category,
        rarity: file.metadata.rarity,
        engine_power: file.physics.engine_power,
        torque: file.physics.torque,
        max_rpm: file.physics.max_rpm,
        idle_rpm: file.physics.idle_rpm,
        gears: file.physics.gears,
        final_drive: file.physics.final_drive,
        weight: file.physics.weight,
        wheel_radius: file.physics.wheel_radius,
        suspension_stiffness: file.physics.suspension_stiffness,
        suspension_damping: file.physics.suspension_damping,
        brake_power: file.physics.brake_power,
        handbrake_power: file.physics.handbrake_power,
        steering_angle: file.physics.steering_angle,
        turning_radius: file.physics.turning_radius,
        drag_coefficient: file.physics.drag_coefficient,
        downforce: file.physics.downforce,
        top_speed: file.physics.top_speed,
        acceleration_0_100: file.physics.acceleration_0_100,
        headlight_count: file.lights.headlight_count,
        taillight_count: file.lights.taillight_count,
        blinker_count: file.lights.blinker_count,
    })
}

pub fn export_car_preset(preset: CarPreset, mesh_path: &str, path: &str) -> Result<()> {
    let mut car = preset.build();
    if !mesh_path.is_empty() {
        car.set_mesh(mesh_path);
    }
    car.save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alcar '{}': {}", path, e))
}

/// Импорт `.alcar` — в сцене эдитора нет типа объекта "машина" (см. шапку
/// файла), так что вместо одного объекта импорт восстанавливает то, что
/// РЕАЛЬНО можно представить существующими типами: корневой `Empty`
/// (имя — бренд+модель из строковой таблицы), дочерний `Mesh` для кузова
/// (`mesh_path`, если задан и резолвится через `import_altex`) и дочерний
/// `Light` (Spot) на каждую фару/стоп-сигнал/поворотник/маячок из
/// `CarLights`. Физика/звук/метаданные (`CarPhysics`/`CarAudio`/
/// `CarMetadata`) у сцены попросту нет куда положить — их значения не
/// теряются в самом `.alcar` (он остаётся на диске), просто эдиторное
/// представление их не показывает, честно как и было в экспорте.
pub fn import_alcar_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let file = AlcarFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alcar '{}': {}", path, e))?;

    let brand = file.get_string(file.metadata.brand_id);
    let model = file.get_string(file.metadata.model_id);
    let car_name = match (brand.is_empty(), model.is_empty()) {
        (false, false) => format!("{} {}", brand, model),
        (false, true) => brand.to_string(),
        (true, false) => model.to_string(),
        (true, true) => "Car".to_string(),
    };

    let mut scene = Scene::new("ImportedCar");
    let root_id = scene.add_object(GameObject::new(&car_name, ObjectType::Empty));

    if !file.mesh_path.is_empty() {
        match super::altex::import_altex(&file.mesh_path) {
            Ok(mut meshes) if !meshes.is_empty() => {
                let (_, mesh, material) = meshes.remove(0);
                let body_id = scene.add_object(GameObject::new("Body", ObjectType::Mesh(MeshComponent {
                    mesh,
                    material,
                    visible: true,
                    wireframe: false,
                    solid: true,
                    double_sided: false,
                })));
                if let Err(e) = scene.set_parent(body_id, Some(root_id)) {
                    log(format!("⚠️ Не удалось прикрепить кузов к '{}': {}", car_name, e));
                }
            }
            Ok(_) => log(format!("⚠️ '{}' не содержит мешей — кузов не добавлен", file.mesh_path)),
            Err(e) => log(format!("⚠️ Не удалось загрузить кузов '{}': {} — кузов не добавлен", file.mesh_path, e)),
        }
    }

    let l = &file.lights;
    add_car_lights(&mut scene, root_id, &l.headlights, l.headlight_count, "Headlight", log);
    add_car_lights(&mut scene, root_id, &l.taillights, l.taillight_count, "Taillight", log);
    add_car_lights(&mut scene, root_id, &l.blinkers, l.blinker_count, "Blinker", log);
    if l.has_siren != 0 {
        add_car_lights(&mut scene, root_id, &l.siren_lights, l.siren_light_count, "SirenLight", log);
    }

    Ok(scene)
}

/// Хелпер для `import_alcar_to_scene` — превращает до 4 `CarLight` в
/// дочерние `Light`-объекты корня машины. `cone_angle` у `CarLight` один
/// (не разделён на внутренний/внешний конус, как у `LightType::Spot`) —
/// внутренний угол берётся приближённо как 70% от внешнего.
fn add_car_lights(scene: &mut Scene, root_id: Uuid, lights: &[CarLight; 4], count: u32, prefix: &str, log: &mut dyn FnMut(String)) {
    let n = (count as usize).min(lights.len());
    if count as usize > lights.len() {
        log(format!("⚠️ {}: заявлено {} шт., но массив вмещает только {} — лишние пропущены", prefix, count, lights.len()));
    }
    for (i, l) in lights.iter().take(n).enumerate() {
        let mut obj = GameObject::new(&format!("{}_{}", prefix, i), ObjectType::Light(LightComponent {
            light_type: LightType::Spot { inner_angle: l.cone_angle * 0.7, outer_angle: l.cone_angle },
            color: l.color,
            intensity: l.intensity,
            range: l.range,
            enabled: true,
        }));
        obj.transform.position = Vec3::new(l.position[0], l.position[1], l.position[2]);
        let id = scene.add_object(obj);
        if let Err(e) = scene.set_parent(id, Some(root_id)) {
            log(format!("⚠️ Не удалось прикрепить {}_{}: {}", prefix, i, e));
        }
    }
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

    #[test]
    fn import_recreates_root_and_headlights() {
        let path = std::env::temp_dir().join("alkash3d_editor_alcar_import_test.alcar");
        let path_str = path.to_string_lossy().to_string();
        // Без mesh_path — create_sports_car() не ссылается на реальный файл
        // на диске, кузов ожидаемо не добавится (см. лог), важна только
        // структура: корень + фары.
        export_car_preset(CarPreset::Sports, "", &path_str).expect("export");

        let mut logs = Vec::new();
        let imported = import_alcar_to_scene(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        let root = imported.objects.values().find(|o| o.parent.is_none()).expect("root");
        assert!(matches!(root.object_type, ObjectType::Empty), "logs: {:?}", logs);
        let children = imported.children_of(Some(root.id));
        assert!(!children.is_empty(), "expected at least headlight children, logs: {:?}", logs);
    }

    #[test]
    fn car_preset_editor_round_trip() {
        let edit = CarPresetEdit {
            brand: "Lada".to_string(),
            model: "Niva".to_string(),
            mesh_path: "meshes/niva.altex".to_string(),
            engine_power: 90.0,
            top_speed: 140.0,
            gears: 5,
            headlight_count: 2,
            ..Default::default()
        };

        let path = std::env::temp_dir().join("alkash3d_editor_car_preset_editor_test.alcar");
        let path_str = path.to_string_lossy().to_string();
        save_car_preset(&edit, &path_str).expect("save");

        let loaded = load_car_preset(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.brand, "Lada");
        assert_eq!(loaded.model, "Niva");
        assert_eq!(loaded.mesh_path, "meshes/niva.altex");
        assert!((loaded.engine_power - 90.0).abs() < 1e-3);
        assert!((loaded.top_speed - 140.0).abs() < 1e-3);
        assert_eq!(loaded.gears, 5);
    }
}
