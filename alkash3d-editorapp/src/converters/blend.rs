//! Конвертер Blender (.blend) -> Altex

use anyhow::{Result, anyhow};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;

pub fn convert(blend_path: &str, output_path: &str) -> Result<()> {
    println!("[BLEND] Converting: {}", blend_path);

    if let Some(parent) = Path::new(output_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let blender_path = find_blender()?;
    println!("[BLEND] Using Blender: {}", blender_path.display());

    let temp_dir = std::env::temp_dir().join(format!("alkash_export_{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;

    let temp_obj = temp_dir.join("export.obj");
    let _temp_mtl = temp_dir.join("export.mtl");

    let python_script = format!(
        r#"
import bpy
import sys
import os

print("=" * 50)
print("AlKAsH3D Blender Exporter")
print("Blender version:", bpy.app.version_string)
print("=" * 50)

bpy.ops.object.select_all(action='DESELECT')

mesh_objects = []
for obj in bpy.data.objects:
    if obj.type == 'MESH':
        mesh_objects.append(obj)
        obj.select_set(True)
        print(f"Found mesh: {{obj.name}}")

print(f"Total meshes: {{len(mesh_objects)}}")

if len(mesh_objects) == 0:
    print("ERROR: No mesh objects found!")
    sys.exit(1)

bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

bpy.ops.object.select_all(action='DESELECT')
for obj in mesh_objects:
    obj.select_set(True)

try:
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.mesh.quads_convert_to_tris()
    bpy.ops.object.mode_set(mode='OBJECT')
except Exception as e:
    print(f"Warning: Could not triangulate: {{e}}")

bpy.ops.object.select_all(action='DESELECT')
for obj in mesh_objects:
    obj.select_set(True)

try:
    if bpy.app.version[0] >= 4:
        bpy.ops.wm.obj_export(
            filepath=r'{}',
            export_selected_objects=True,
            export_materials=True,
            export_normals=True,
            export_uv=True,
            export_triangulated_mesh=True,
            forward_axis='NEGATIVE_Z',
            up_axis='Y'
        )
    else:
        bpy.ops.export_scene.obj(
            filepath=r'{}',
            use_selection=True,
            use_materials=True,
            use_normals=True,
            use_uvs=True,
            use_triangles=True,
            axis_forward='-Z',
            axis_up='Y'
        )
    print("EXPORT_SUCCESS")
except Exception as e:
    print(f"Export error: {{e}}")
    try:
        bpy.ops.export_scene.obj(
            filepath=r'{}',
            use_selection=True,
            use_materials=False,
            use_normals=True,
            use_uvs=True,
            use_triangles=True
        )
        print("EXPORT_SUCCESS (fallback)")
    except Exception as e2:
        print(f"Fallback export error: {{e2}}")
        sys.exit(1)

print("Script completed")
"#,
        temp_obj.to_str().unwrap().replace("\\", "\\\\"),
        temp_obj.to_str().unwrap().replace("\\", "\\\\"),
        temp_obj.to_str().unwrap().replace("\\", "\\\\"),
    );

    println!("[BLEND] Running Blender export...");

    let output = Command::new(&blender_path)
        .arg("--background")
        .arg(blend_path)
        .arg("--python-expr")
        .arg(&python_script)
        .arg("--")
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    for line in stdout.lines() {
        if !line.trim().is_empty() {
            println!("[BLEND] {}", line);
        }
    }

    if !stderr.is_empty() {
        for line in stderr.lines() {
            if !line.trim().is_empty() {
                eprintln!("[BLEND ERR] {}", line);
            }
        }
    }

    if !output.status.success() {
        return Err(anyhow!("Blender process failed with code: {:?}", output.status.code()));
    }

    if !temp_obj.exists() {
        let blend_dir = Path::new(blend_path).parent().unwrap_or(Path::new("."));
        let alt_obj = blend_dir.join("export.obj");
        if alt_obj.exists() {
            fs::copy(&alt_obj, &temp_obj)?;
        } else {
            return Err(anyhow!("OBJ file not created"));
        }
    }

    let file_size = fs::metadata(&temp_obj)?.len();
    if file_size == 0 {
        return Err(anyhow!("OBJ file is empty"));
    }

    println!("[BLEND] OBJ created ({} bytes)", file_size);

    super::obj::convert(temp_obj.to_str().unwrap(), output_path)?;

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
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let ms_store_path = format!("{}\\Blender Foundation\\Blender\\blender.exe", local_app_data);

        let possible_paths = [
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
            &ms_store_path,
        ];

        for path_str in possible_paths {
            let path = Path::new(path_str);
            if path.exists() {
                println!("[BLEND] Found Blender at: {}", path_str);
                return Ok(path.to_path_buf());
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let possible_paths = [
            "/usr/bin/blender",
            "/usr/local/bin/blender",
            "/opt/blender/blender",
            &format!("{}/blender/blender", std::env::var("HOME").unwrap_or_default()),
        ];

        for path_str in possible_paths {
            let path = Path::new(path_str);
            if path.exists() {
                return Ok(path.to_path_buf());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let possible_paths = [
            "/Applications/Blender.app/Contents/MacOS/Blender",
            &format!("{}/Applications/Blender.app/Contents/MacOS/Blender",
                     std::env::var("HOME").unwrap_or_default()),
        ];

        for path_str in possible_paths {
            let path = Path::new(path_str);
            if path.exists() {
                return Ok(path.to_path_buf());
            }
        }
    }

    Err(anyhow!("Blender not found. Please install Blender and ensure it's in PATH."))
}