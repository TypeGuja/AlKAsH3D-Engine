use alkash3d_rs::*;
use anyhow::Result;
use std::process::Command;
use std::path::Path;

pub fn blend_to_altex(blend_path: &str, output_path: &str) -> Result<()> {
    println!("[BLEND] Loading: {}", blend_path);

    // Создаём папку для вывода
    if let Some(parent) = Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Временный OBJ файл
    let temp_obj = format!("{}/temp_{}.obj", std::env::temp_dir().to_str().unwrap(), std::process::id());

    // Экспорт из Blender в OBJ
    let status = Command::new("blender")
        .arg("--background")
        .arg(blend_path)
        .arg("--enable-autoexec")
        .arg("--python-expr")
        .arg(format!(
            "import bpy; \
             bpy.ops.export_scene.obj(filepath='{}', \
             use_selection=False, \
             use_materials=True, \
             use_normals=True, \
             use_uvs=True, \
             use_triangles=True)",
            temp_obj
        ))
        .status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("Blender export failed. Make sure Blender is installed and in PATH"));
    }

    // Конвертируем OBJ в Altex
    super::obj_converter::convert(&temp_obj, output_path)?;

    // Удаляем временный файл
    let _ = std::fs::remove_file(temp_obj);

    println!("[BLEND] Saved: {}", output_path);
    Ok(())
}

// Прямая конвертация BLEND -> Alcar (создаёт mesh + car)
pub fn blend_to_alcar(blend_path: &str, output_path: &str, car_type: &str) -> Result<()> {
    // Сначала конвертируем в Altex
    let altex_path = super::replace_extension(output_path, "altex");
    blend_to_altex(blend_path, &altex_path)?;

    // Создаём Alcar из меша
    let mesh_name = Path::new(&altex_path).file_stem().unwrap().to_str().unwrap();
    super::create_car_from_mesh(&altex_path, output_path, car_type)?;

    Ok(())
}