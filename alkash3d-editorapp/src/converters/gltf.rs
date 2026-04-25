//! Конвертер glTF -> OBJ с децимацией

use anyhow::{Result, anyhow};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;

pub fn convert(gltf_path: &str, output_path: &str) -> Result<()> {
    convert_with_quality(gltf_path, output_path, 0.5)
}

pub fn convert_with_quality(gltf_path: &str, output_path: &str, quality: f32) -> Result<()> {
    println!("[glTF] Converting: {} (quality: {:.0}%)", gltf_path, quality * 100.0);

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let blender_path = find_blender()?;
    println!("[glTF] Using: {}", blender_path.display());

    let quality = quality.clamp(0.05, 1.0);

    let python_script = format!(
        r#"
import bpy
import sys

print("Blender {{}}".format(bpy.app.version_string), flush=True)

# Очищаем сцену
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete()

# Импортируем glTF
bpy.ops.import_scene.gltf(filepath=r'{0}')

mesh_objects = [obj for obj in bpy.data.objects if obj.type == 'MESH']
print("Found {{}} mesh(es)".format(len(mesh_objects)), flush=True)

# Децимация
if {2} < 0.99:
    for obj in mesh_objects:
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        modifier = obj.modifiers.new(name="Decimate", type='DECIMATE')
        modifier.ratio = {2}
        try:
            bpy.ops.object.modifier_apply(modifier="Decimate")
        except:
            pass

# Выделяем все для экспорта
bpy.ops.object.select_all(action='DESELECT')
for obj in mesh_objects:
    obj.select_set(True)

bpy.ops.wm.obj_export(filepath=r'{1}')
print("SUCCESS", flush=True)
"#,
        gltf_path.replace("\\", "\\\\"),
        output_path.replace("\\", "\\\\"),
        quality
    );

    let temp_script = std::env::temp_dir().join("gltf_export.py");
    fs::write(&temp_script, &python_script)?;

    let output = Command::new(&blender_path)
        .arg("--background")
        .arg("--python")
        .arg(&temp_script)
        .output()?;

    if !output.status.success() {
        return Err(anyhow!("glTF export failed"));
    }

    println!("[glTF] Success!");
    let _ = fs::remove_file(temp_script);
    Ok(())
}

fn find_blender() -> Result<PathBuf> {
    if let Ok(path) = which::which("blender") {
        return Ok(path);
    }
    #[cfg(target_os = "windows")]
    {
        for p in ["D:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe"] {
            if Path::new(p).exists() {
                return Ok(PathBuf::from(p));
            }
        }
    }
    Err(anyhow!("Blender not found"))
}