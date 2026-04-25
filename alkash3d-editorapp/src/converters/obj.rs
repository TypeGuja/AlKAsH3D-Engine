//! Конвертер OBJ -> Altex

use anyhow::{Result, anyhow};
use std::path::Path;
use std::fs::File;
use std::io::Write;

#[derive(Default)]
struct AltexFile {
    meshes: Vec<AltexMesh>,
    objects: Vec<AltexObject>,
}

struct AltexMesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    name: String,
}

struct AltexObject {
    mesh_id: usize,
    transform: Transform,
    name: String,
}

#[derive(Clone)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 3],
    bitangent: [f32; 3],
    uv: [f32; 2],
    uv2: [f32; 2],
    color: [f32; 4],
}

#[derive(Clone)]
struct Transform {
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

impl AltexFile {
    fn new() -> Self {
        Self::default()
    }

    fn add_mesh(&mut self, vertices: Vec<Vertex>, indices: Vec<u32>, name: &str) -> usize {
        let id = self.meshes.len();
        self.meshes.push(AltexMesh {
            vertices,
            indices,
            name: name.to_string(),
        });
        id
    }

    fn add_object(&mut self, mesh_id: usize, transform: Transform, name: &str) {
        self.objects.push(AltexObject {
            mesh_id,
            transform,
            name: name.to_string(),
        });
    }

    fn save(&self, path: &str) -> Result<()> {
        println!("[Altex] Saving {} meshes to {}", self.meshes.len(), path);

        let mut file = File::create(path)?;

        // Заголовок файла
        writeln!(file, "ALTEХ")?;
        writeln!(file, "version 1.0")?;
        writeln!(file, "meshes {}", self.meshes.len())?;
        writeln!(file, "objects {}", self.objects.len())?;

        // Сохраняем меши
        for (idx, mesh) in self.meshes.iter().enumerate() {
            writeln!(file, "mesh {} {}", idx, mesh.name)?;
            writeln!(file, "  vertices {}", mesh.vertices.len())?;
            writeln!(file, "  indices {}", mesh.indices.len())?;

            // Сохраняем вершины
            for v in &mesh.vertices {
                writeln!(file, "    v {} {} {} | {} {} {} | {} {} {} | {} {} {} | {} {} | {} {} | {} {} {} {}",
                         v.position[0], v.position[1], v.position[2],
                         v.normal[0], v.normal[1], v.normal[2],
                         v.tangent[0], v.tangent[1], v.tangent[2],
                         v.bitangent[0], v.bitangent[1], v.bitangent[2],
                         v.uv[0], v.uv[1],
                         v.uv2[0], v.uv2[1],
                         v.color[0], v.color[1], v.color[2], v.color[3]
                )?;
            }

            // Сохраняем индексы (по 10 в строке)
            for chunk in mesh.indices.chunks(10) {
                write!(file, "    i")?;
                for idx in chunk {
                    write!(file, " {}", idx)?;
                }
                writeln!(file)?;
            }
        }

        // Сохраняем объекты
        for (idx, obj) in self.objects.iter().enumerate() {
            writeln!(file, "object {} {}", idx, obj.name)?;
            writeln!(file, "  mesh {}", obj.mesh_id)?;
            writeln!(file, "  position {} {} {}", obj.transform.position[0], obj.transform.position[1], obj.transform.position[2])?;
            writeln!(file, "  rotation {} {} {} {}", obj.transform.rotation[0], obj.transform.rotation[1], obj.transform.rotation[2], obj.transform.rotation[3])?;
            writeln!(file, "  scale {} {} {}", obj.transform.scale[0], obj.transform.scale[1], obj.transform.scale[2])?;
        }

        println!("[Altex] File saved successfully");
        Ok(())
    }
}

pub fn convert(obj_path: &str, output_path: &str) -> Result<()> {
    println!("[OBJ] Loading: {}", obj_path);

    if let Some(parent) = Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (models, materials) = tobj::load_obj(
        obj_path,
        &tobj::LoadOptions {
            single_index: true,
            triangulate: true,
            ignore_points: true,
            ignore_lines: true,
        },
    ).map_err(|e| anyhow!("Failed to load OBJ: {}", e))?;

    if models.is_empty() {
        return Err(anyhow!("No models found in OBJ file"));
    }

    let mut altex = AltexFile::new();
    let _ = materials;

    for (idx, model) in models.iter().enumerate() {
        let mesh = &model.mesh;
        let vertex_count = mesh.positions.len() / 3;

        if vertex_count == 0 {
            println!("[OBJ] Skipping empty mesh");
            continue;
        }

        let mut vertices = Vec::with_capacity(vertex_count);
        let has_normals = mesh.normals.len() >= vertex_count * 3;
        let has_texcoords = mesh.texcoords.len() >= vertex_count * 2;

        for i in 0..vertex_count {
            let position = [
                mesh.positions[i * 3],
                mesh.positions[i * 3 + 1],
                mesh.positions[i * 3 + 2],
            ];

            let normal = if has_normals {
                [
                    mesh.normals[i * 3],
                    mesh.normals[i * 3 + 1],
                    mesh.normals[i * 3 + 2],
                ]
            } else {
                [0.0, 1.0, 0.0]
            };

            let uv = if has_texcoords {
                [
                    mesh.texcoords[i * 2],
                    1.0 - mesh.texcoords[i * 2 + 1],
                ]
            } else {
                [0.0, 0.0]
            };

            let (tangent, bitangent) = calculate_tangent_bitangent(&normal, &uv);

            vertices.push(Vertex {
                position,
                normal,
                tangent: tangent,
                bitangent,
                uv,
                uv2: [0.0, 0.0],
                color: [1.0, 1.0, 1.0, 1.0],
            });
        }

        if has_texcoords && has_normals {
            recalculate_tangents(&mut vertices, &mesh.indices);
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

        let obj_name = format!("obj_{}", mesh_name);
        altex.add_object(
            mesh_id,
            Transform {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
            &obj_name,
        );
    }

    altex.save(output_path)?;
    println!("[OBJ] Saved to: {}", output_path);
    Ok(())
}

fn calculate_tangent_bitangent(normal: &[f32; 3], _uv: &[f32; 2]) -> ([f32; 3], [f32; 3]) {
    let tangent = if normal[1].abs() < 0.999 {
        let t = [normal[1], -normal[0], 0.0];
        let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
        [t[0] / len, t[1] / len, t[2] / len]
    } else {
        [1.0, 0.0, 0.0]
    };

    let bitangent = [
        normal[1] * tangent[2] - normal[2] * tangent[1],
        normal[2] * tangent[0] - normal[0] * tangent[2],
        normal[0] * tangent[1] - normal[1] * tangent[0],
    ];

    (tangent, bitangent)
}

fn recalculate_tangents(vertices: &mut [Vertex], indices: &[u32]) {
    let triangle_count = indices.len() / 3;

    for tri_idx in 0..triangle_count {
        let i0 = indices[tri_idx * 3] as usize;
        let i1 = indices[tri_idx * 3 + 1] as usize;
        let i2 = indices[tri_idx * 3 + 2] as usize;

        if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
            continue;
        }

        let p0 = vertices[i0].position;
        let p1 = vertices[i1].position;
        let p2 = vertices[i2].position;

        let uv0 = vertices[i0].uv;
        let uv1 = vertices[i1].uv;
        let uv2 = vertices[i2].uv;

        let delta_pos1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let delta_pos2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

        let delta_uv1 = [uv1[0] - uv0[0], uv1[1] - uv0[1]];
        let delta_uv2 = [uv2[0] - uv0[0], uv2[1] - uv0[1]];

        let r = 1.0 / (delta_uv1[0] * delta_uv2[1] - delta_uv1[1] * delta_uv2[0]);

        let tangent = [
            (delta_pos1[0] * delta_uv2[1] - delta_pos2[0] * delta_uv1[1]) * r,
            (delta_pos1[1] * delta_uv2[1] - delta_pos2[1] * delta_uv1[1]) * r,
            (delta_pos1[2] * delta_uv2[1] - delta_pos2[2] * delta_uv1[1]) * r,
        ];

        let bitangent = [
            (delta_pos2[0] * delta_uv1[0] - delta_pos1[0] * delta_uv2[0]) * r,
            (delta_pos2[1] * delta_uv1[0] - delta_pos1[1] * delta_uv2[0]) * r,
            (delta_pos2[2] * delta_uv1[0] - delta_pos1[2] * delta_uv2[0]) * r,
        ];

        let tan_len = (tangent[0].powi(2) + tangent[1].powi(2) + tangent[2].powi(2)).sqrt();
        let bitan_len = (bitangent[0].powi(2) + bitangent[1].powi(2) + bitangent[2].powi(2)).sqrt();

        if tan_len > 0.0 && bitan_len > 0.0 {
            let norm_tangent = [tangent[0] / tan_len, tangent[1] / tan_len, tangent[2] / tan_len];
            let norm_bitangent = [bitangent[0] / bitan_len, bitangent[1] / bitan_len, bitangent[2] / bitan_len];

            vertices[i0].tangent = norm_tangent;
            vertices[i0].bitangent = norm_bitangent;
            vertices[i1].tangent = norm_tangent;
            vertices[i1].bitangent = norm_bitangent;
            vertices[i2].tangent = norm_tangent;
            vertices[i2].bitangent = norm_bitangent;
        }
    }

    for v in vertices {
        let n = v.normal;
        let t = v.tangent;
        let dot = t[0] * n[0] + t[1] * n[1] + t[2] * n[2];
        let ortho_t = [t[0] - n[0] * dot, t[1] - n[1] * dot, t[2] - n[2] * dot];
        let len = (ortho_t[0].powi(2) + ortho_t[1].powi(2) + ortho_t[2].powi(2)).sqrt();
        if len > 0.0 {
            v.tangent = [ortho_t[0] / len, ortho_t[1] / len, ortho_t[2] / len];
        }
        v.bitangent = [
            n[1] * v.tangent[2] - n[2] * v.tangent[1],
            n[2] * v.tangent[0] - n[0] * v.tangent[2],
            n[0] * v.tangent[1] - n[1] * v.tangent[0],
        ];
    }
}