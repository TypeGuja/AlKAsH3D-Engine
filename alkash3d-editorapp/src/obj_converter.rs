// editor/src/obj_converter.rs
use alkash3d_rs::*;
use anyhow::{Result, anyhow};
use std::path::Path;

pub fn convert(obj_path: &str, output_path: &str) -> Result<()> {
    println!("[OBJ] Loading: {}", obj_path);

    if let Some(parent) = Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (models, _materials) = tobj::load_obj(
        obj_path,
        &tobj::LoadOptions {
            single_index: true,
            triangulate: true,
            ignore_points: true,
            ignore_lines: true,
        }
    ).map_err(|e| anyhow!("Failed to load OBJ: {}", e))?;

    if models.is_empty() {
        return Err(anyhow!("No models found in OBJ file"));
    }

    let mut altex = AltexFile::new();

    for (idx, model) in models.iter().enumerate() {
        let mesh = &model.mesh;
        let vertex_count = mesh.positions.len() / 3;

        if vertex_count == 0 {
            println!("[OBJ] Skipping empty mesh");
            continue;
        }

        let mut vertices = Vec::with_capacity(vertex_count);

        for i in 0..vertex_count {
            let has_normals = mesh.normals.len() > i * 3 + 2;
            let has_texcoords = mesh.texcoords.len() > i * 2 + 1;

            vertices.push(Vertex {
                position: [
                    mesh.positions[i*3],
                    mesh.positions[i*3+1],
                    mesh.positions[i*3+2],
                ],
                normal: if has_normals {
                    [mesh.normals[i*3], mesh.normals[i*3+1], mesh.normals[i*3+2]]
                } else {
                    [0.0, 1.0, 0.0]
                },
                uv: if has_texcoords {
                    [mesh.texcoords[i*2], 1.0 - mesh.texcoords[i*2+1]]
                } else {
                    [0.0, 0.0]
                },
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

        let mesh_id = altex.add_mesh(vertices.clone(), indices.clone(), &mesh_name);
        println!("[OBJ] Added mesh '{}' ({} verts, {} indices, ID: {})",
                 mesh_name, vertices.len(), indices.len(), mesh_id);

        altex.add_object(
            mesh_id,
            Transform {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
            &mesh_name
        );
    }

    altex.save(output_path)?;
    println!("[OBJ] Saved to: {}", output_path);
    Ok(())
}