use alkash3d_rs::*;
use anyhow::Result;
use tobj;
use std::path::Path;

pub fn convert(obj_path: &str, output_path: &str) -> Result<()> {
    println!("[OBJ] Loading: {}", obj_path);

    // Создаём папку для вывода если нужно
    if let Some(parent) = Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (models, _) = tobj::load_obj(obj_path, &tobj::LoadOptions {
        single_index: true,
        triangulate: true,
        ignore_points: true,
        ignore_lines: true,
    })?;

    let mut altex = AltexFile::new();

    for (idx, model) in models.iter().enumerate() {
        let mesh = &model.mesh;
        let mut vertices = Vec::new();

        for i in 0..mesh.positions.len() / 3 {
            vertices.push(Vertex {
                position: [mesh.positions[i*3], mesh.positions[i*3+1], mesh.positions[i*3+2]],
                normal: if mesh.normals.len() > i*3 {
                    [mesh.normals[i*3], mesh.normals[i*3+1], mesh.normals[i*3+2]]
                } else { [0.0, 1.0, 0.0] },
                uv: if mesh.texcoords.len() > i*2 {
                    [mesh.texcoords[i*2], mesh.texcoords[i*2+1]]
                } else { [0.0, 0.0] },
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
                uv2: [0.0, 0.0],
                color: [1.0, 1.0, 1.0, 1.0],
            });
        }

        let indices: Vec<u32> = mesh.indices.iter().map(|&i| i as u32).collect();
        let mesh_name = if model.name.is_empty() {
            format!("mesh_{}", idx)
        } else {
            model.name.clone()
        };

        altex.add_mesh(vertices.clone(), indices, &mesh_name);
        println!("[OBJ] Added mesh '{}' ({} verts)", mesh_name, vertices.len());
    }

    altex.save(output_path)?;
    println!("[OBJ] Saved: {}", output_path);
    Ok(())
}