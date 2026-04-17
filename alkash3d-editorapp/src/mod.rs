use alkash3d_rs::*;
use anyhow::Result;
use std::path::Path;

pub mod obj_converter;
pub mod blend_converter;

// Конвертация OBJ -> Altex
pub fn obj_to_altex(obj_path: &str, output_path: &str) -> Result<()> {
    obj_converter::convert(obj_path, output_path)
}

// Конвертация BLEND -> Altex
pub fn blend_to_altex(blend_path: &str, output_path: &str) -> Result<()> {
    blend_converter::blend_to_altex(blend_path, output_path)
}

// Создание Alcar из Altex
pub fn create_car_from_mesh(mesh_path: &str, output_path: &str, car_type: &str) -> Result<()> {
    let mut car = AlcarFile::new();
    car.set_mesh(mesh_path);

    match car_type {
        "police" => {
            car.metadata.category = 4;
            car.metadata.ai_script_id = car.add_string("police_ai.lua");
            car.lights.has_siren = 1;
            car.lights.siren_light_count = 2;
            car.physics.engine_power = 300.0;
            car.physics.top_speed = 250.0;
            car.physics.acceleration_0_100 = 5.5;
        }
        "sports" => {
            car.metadata.category = 2;
            car.physics.engine_power = 500.0;
            car.physics.gears = 7;
            car.physics.acceleration_0_100 = 3.5;
            car.physics.top_speed = 320.0;
        }
        _ => {}
    }

    car.save(output_path)?;
    Ok(())
}

// Утилита для замены расширения
pub fn replace_extension(path: &str, new_ext: &str) -> String {
    let path = Path::new(path);
    let stem = path.file_stem().unwrap().to_str().unwrap();
    let parent = path.parent().unwrap_or(Path::new(""));
    parent.join(format!("{}.{}", stem, new_ext)).to_str().unwrap().to_string()
}