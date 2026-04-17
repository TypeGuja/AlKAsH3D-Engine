// editor/src/blend_converter.rs
use anyhow::{Result, anyhow};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;

pub fn blend_to_altex(blend_path: &str, output_path: &str) -> Result<()> {
    println!("[BLEND] Converting: {}", blend_path);

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let blender_path = find_blender()?;
    println!("[BLEND] Using Blender: {}", blender_path.display());

    let temp_dir = std::env::temp_dir().join(format!("alkash_export_{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;

    let temp_obj = temp_dir.join("export.obj");
    let temp_obj_str = temp_obj.to_str().unwrap().replace("\\", "\\\\");

    // Исправленный Python скрипт для Blender 5.1.1
    let python_script = format!(
        r#"
import bpy
import sys

print("Starting Blender export...")
print("Blender version:", bpy.app.version_string)

# Снимаем выделение
bpy.ops.object.select_all(action='DESELECT')

# Выделяем все меши
mesh_count = 0
for obj in bpy.data.objects:
    if obj.type == 'MESH':
        mesh_count += 1
        obj.select_set(True)
        print("Selected mesh:", obj.name)

print("Total meshes selected:", mesh_count)

if mesh_count == 0:
    print("ERROR: No mesh objects found!")
    sys.exit(1)

# Применяем трансформации
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Пробуем экспорт с правильными параметрами для Blender 5.x
try:
    # Правильные параметры для Blender 5.1.1
    bpy.ops.wm.obj_export(
        filepath=r'{}',
        export_selected_objects=True,
        export_materials=True,
        export_normals=True,
        export_uv=True,                    # export_uvs -> export_uv
        export_triangulated_mesh=True,
        forward_axis='NEGATIVE_Z',
        up_axis='Y'
    )
    print("EXPORT_SUCCESS")
    export_success = True
except Exception as e:
    print("First method failed:", str(e))

    # Пробуем узнать правильные параметры
    try:
        # Получаем список параметров оператора
        op = bpy.ops.wm.obj_export
        print("Available parameters:", dir(op))

        # Пробуем минимальный набор параметров
        bpy.ops.wm.obj_export(
            filepath=r'{}',
            export_selected_objects=True
        )
        print("EXPORT_SUCCESS (minimal params)")
        export_success = True
    except Exception as e2:
        print("Minimal params failed:", str(e2))

        # Пробуем альтернативный экспорт через колбэк
        try:
            bpy.ops.wm.obj_export('EXEC_DEFAULT', filepath=r'{}')
            print("EXPORT_SUCCESS (EXEC_DEFAULT)")
            export_success = True
        except Exception as e3:
            print("EXEC_DEFAULT failed:", str(e3))
            print("EXPORT_ERROR: All methods failed")
            sys.exit(1)

print("Script completed")
"#,
        temp_obj_str, temp_obj_str, temp_obj_str
    );

    println!("[BLEND] Running Blender export...");

    let output = Command::new(&blender_path)
        .arg("--background")
        .arg(blend_path)
        .arg("--python-expr")
        .arg(&python_script)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("[BLEND] === stdout ===");
    for line in stdout.lines() {
        println!("[BLEND] {}", line);
    }

    if !stderr.is_empty() {
        println!("[BLEND] === stderr ===");
        for line in stderr.lines() {
            println!("[BLEND] {}", line);
        }
    }

    if !output.status.success() {
        return Err(anyhow!("Blender process failed"));
    }

    if !temp_obj.exists() {
        return Err(anyhow!("OBJ file not created"));
    }

    let file_size = fs::metadata(&temp_obj)?.len();
    if file_size == 0 {
        return Err(anyhow!("OBJ file is empty"));
    }

    println!("[BLEND] OBJ created ({} bytes)", file_size);

    super::obj_converter::convert(temp_obj.to_str().unwrap(), output_path)?;

    let _ = fs::remove_dir_all(temp_dir);

    println!("[BLEND] Saved to: {}", output_path);
    Ok(())
}

fn find_blender() -> Result<PathBuf> {
    if let Ok(path) = which::which("blender") {
        println!("[BLEND] Found blender in PATH: {}", path.display());
        return Ok(path);
    }

    #[cfg(target_os = "windows")]
    {
        let paths = [
            "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Blender\\blender.exe",
            "C:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe",
            "D:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe",
            "E:\\SteamLibrary\\steamapps\\common\\Blender\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 5.1\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 5.0\\blender.exe",
            "C:\\Program Files\\Blender Foundation\\Blender 4.3\\blender.exe",
        ];

        for p in paths {
            let path = Path::new(p);
            if path.exists() {
                println!("[BLEND] Found Blender at: {}", p);
                return Ok(path.to_path_buf());
            }
        }
    }

    Err(anyhow!("Blender not found"))
}

pub fn replace_extension(path: &str, new_ext: &str) -> String {
    Path::new(path)
        .with_extension(new_ext)
        .to_string_lossy()
        .to_string()
}