//! Конвертер Blender (.blend) -> OBJ с децимацией

use anyhow::{Result, anyhow};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;

pub fn convert(blend_path: &str, output_path: &str) -> Result<()> {
    convert_with_quality(blend_path, output_path, 0.5)
}

pub fn convert_with_quality(blend_path: &str, output_path: &str, quality: f32) -> Result<()> {
    println!("[BLEND] Converting: {} (quality: {:.0}%)", blend_path, quality * 100.0);

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let blender_path = find_blender()?;
    println!("[BLEND] Using: {}", blender_path.display());

    // Ограничиваем quality
    let quality = quality.clamp(0.05, 1.0);

    let python_script = if quality < 0.99 {
        // С децимацией
        format!(
            r#"
import bpy
import sys

print("Blender {{}}".format(bpy.app.version_string), flush=True)
print("Decimate ratio: {2}", flush=True)

bpy.ops.wm.open_mainfile(filepath=r'{0}')

mesh_objects = [obj for obj in bpy.data.objects if obj.type == 'MESH']
if not mesh_objects:
    print("ERROR: No meshes found", flush=True)
    sys.exit(1)

print("Found {{}} mesh(es)".format(len(mesh_objects)), flush=True)

# Применяем децимацию
for obj in mesh_objects:
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj

    # Добавляем модификатор Decimate
    modifier = obj.modifiers.new(name="Decimate", type='DECIMATE')
    modifier.ratio = {2}

    # Применяем модификатор
    try:
        bpy.ops.object.modifier_apply(modifier="Decimate")
        print("  Decimated {{}}".format(obj.name), flush=True)
    except Exception as e:
        print("  Failed to decimate {{}}: {{}}".format(obj.name, e), flush=True)

# Выделяем все для экспорта
bpy.ops.object.select_all(action='DESELECT')
for obj in mesh_objects:
    obj.select_set(True)

# Экспорт
bpy.ops.wm.obj_export(filepath=r'{1}')
print("SUCCESS", flush=True)
"#,
            blend_path.replace("\\", "\\\\"),
            output_path.replace("\\", "\\\\"),
            quality
        )
    } else {
        // Без децимации (полное качество)
        format!(
            r#"
import bpy
import sys

print("Blender {{}}".format(bpy.app.version_string), flush=True)

bpy.ops.wm.open_mainfile(filepath=r'{0}')

mesh_objects = [obj for obj in bpy.data.objects if obj.type == 'MESH']
if not mesh_objects:
    print("ERROR: No meshes found", flush=True)
    sys.exit(1)

print("Found {{}} mesh(es)".format(len(mesh_objects)), flush=True)

bpy.ops.object.select_all(action='DESELECT')
for obj in mesh_objects:
    obj.select_set(True)

bpy.ops.wm.obj_export(filepath=r'{1}')
print("SUCCESS", flush=True)
"#,
            blend_path.replace("\\", "\\\\"),
            output_path.replace("\\", "\\\\"),
        )
    };

    let temp_script = std::env::temp_dir().join("blender_export.py");
    fs::write(&temp_script, &python_script)?;

    let output = Command::new(&blender_path)
        .arg("--background")
        .arg("--python")
        .arg(&temp_script)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    if !output.status.success() {
        return Err(anyhow!("Blender export failed: {}", stdout));
    }

    if !Path::new(output_path).exists() {
        return Err(anyhow!("OBJ file not created"));
    }

    let size = fs::metadata(output_path)?.len();
    println!("[BLEND] Success! OBJ size: {} bytes", size);

    let _ = fs::remove_file(temp_script);
    Ok(())
}

fn find_blender() -> Result<PathBuf> {
    if let Ok(path) = which::which("blender") {
        return Ok(path);
    }

    #[cfg(target_os = "windows")]
    {
        for p in [
            "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Blender\\blender.exe",
            "C:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe",
            "D:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe",
            "E:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 4.3\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 4.2\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 4.1\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 4.0\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 3.6\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 3.5\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 3.4\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 3.3\\blender.exe",
        ] {
            if Path::new(p).exists() {
                return Ok(PathBuf::from(p));
            }
        }
    }

    Err(anyhow!("Blender not found"))
}