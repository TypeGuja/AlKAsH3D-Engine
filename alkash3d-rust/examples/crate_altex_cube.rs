// examples/create_cube_altex_fixed.rs
use alkash3d_rs::*;

fn main() {
    println!("Creating cube_fixed.altex file...");

    let mut altex = AltexFile::new();

    // Маленький куб
    let scale = 0.25;

    let vertices = vec![
        // Front face
        Vertex { position: [-0.5 * scale, -0.5 * scale, -0.5 * scale], normal: [0.0, 0.0, -1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
        Vertex { position: [ 0.5 * scale, -0.5 * scale, -0.5 * scale], normal: [0.0, 0.0, -1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
        Vertex { position: [ 0.5 * scale,  0.5 * scale, -0.5 * scale], normal: [0.0, 0.0, -1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
        Vertex { position: [-0.5 * scale,  0.5 * scale, -0.5 * scale], normal: [0.0, 0.0, -1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 0.0, 1.0] },
        // Back face
        Vertex { position: [-0.5 * scale, -0.5 * scale,  0.5 * scale], normal: [0.0, 0.0, 1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
        Vertex { position: [ 0.5 * scale, -0.5 * scale,  0.5 * scale], normal: [0.0, 0.0, 1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
        Vertex { position: [ 0.5 * scale,  0.5 * scale,  0.5 * scale], normal: [0.0, 0.0, 1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
        Vertex { position: [-0.5 * scale,  0.5 * scale,  0.5 * scale], normal: [0.0, 0.0, 1.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 0.0, 1.0] },
        // Top face
        Vertex { position: [-0.5 * scale,  0.5 * scale, -0.5 * scale], normal: [0.0, 1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
        Vertex { position: [ 0.5 * scale,  0.5 * scale, -0.5 * scale], normal: [0.0, 1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
        Vertex { position: [ 0.5 * scale,  0.5 * scale,  0.5 * scale], normal: [0.0, 1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
        Vertex { position: [-0.5 * scale,  0.5 * scale,  0.5 * scale], normal: [0.0, 1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [0.0, 0.0, 1.0, 1.0] },
        // Bottom face
        Vertex { position: [-0.5 * scale, -0.5 * scale, -0.5 * scale], normal: [0.0, -1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 0.0, 1.0] },
        Vertex { position: [ 0.5 * scale, -0.5 * scale, -0.5 * scale], normal: [0.0, -1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 0.0, 1.0] },
        Vertex { position: [ 0.5 * scale, -0.5 * scale,  0.5 * scale], normal: [0.0, -1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 0.0, 1.0] },
        Vertex { position: [-0.5 * scale, -0.5 * scale,  0.5 * scale], normal: [0.0, -1.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 0.0, 1.0] },
        // Right face
        Vertex { position: [ 0.5 * scale, -0.5 * scale, -0.5 * scale], normal: [1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 1.0, 1.0] },
        Vertex { position: [ 0.5 * scale,  0.5 * scale, -0.5 * scale], normal: [1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 1.0, 1.0] },
        Vertex { position: [ 0.5 * scale,  0.5 * scale,  0.5 * scale], normal: [1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 1.0, 1.0] },
        Vertex { position: [ 0.5 * scale, -0.5 * scale,  0.5 * scale], normal: [1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 0.0, 1.0, 1.0] },
        // Left face
        Vertex { position: [-0.5 * scale, -0.5 * scale, -0.5 * scale], normal: [-1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 1.0, 1.0] },
        Vertex { position: [-0.5 * scale,  0.5 * scale, -0.5 * scale], normal: [-1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 1.0, 1.0] },
        Vertex { position: [-0.5 * scale,  0.5 * scale,  0.5 * scale], normal: [-1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 1.0, 1.0] },
        Vertex { position: [-0.5 * scale, -0.5 * scale,  0.5 * scale], normal: [-1.0, 0.0, 0.0], tangent: [0.0; 3], bitangent: [0.0; 3], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [0.0, 1.0, 1.0, 1.0] },
    ];

    let indices: Vec<u32> = vec![
        0,1,2, 0,2,3,
        4,6,5, 4,7,6,
        8,9,10, 8,10,11,
        12,14,13, 12,15,14,
        16,17,18, 16,18,19,
        20,22,21, 20,23,22,
    ];

    let mesh_id = altex.add_mesh(vertices, indices, "Cube_Fixed");
    altex.add_object(mesh_id, Transform { position: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0, 1.0, 1.0] }, "Cube");

    altex.save("cube.altex").unwrap();
    println!("✅ Saved cube_fixed.altex");
}