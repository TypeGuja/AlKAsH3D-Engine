// tools/create_scene.rs (отдельный бинарник для создания тестовой сцены)
use alkash3d_rs::*;

fn main() {
    let mut scene = AltexFile::new();

    // Создаём простой куб
    let vertices = vec![
        Vertex { position: [-0.5, -0.5, -0.5], normal: [0.0, 0.0, -1.0], tangent: [1.0, 0.0, 0.0], bitangent: [0.0, 1.0, 0.0], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        Vertex { position: [ 0.5, -0.5, -0.5], normal: [0.0, 0.0, -1.0], tangent: [1.0, 0.0, 0.0], bitangent: [0.0, 1.0, 0.0], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        Vertex { position: [ 0.5,  0.5, -0.5], normal: [0.0, 0.0, -1.0], tangent: [1.0, 0.0, 0.0], bitangent: [0.0, 1.0, 0.0], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        Vertex { position: [-0.5,  0.5, -0.5], normal: [0.0, 0.0, -1.0], tangent: [1.0, 0.0, 0.0], bitangent: [0.0, 1.0, 0.0], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
    ];

    let indices = vec![0, 1, 2, 0, 2, 3];

    scene.add_mesh(vertices, indices, "Cube");
    scene.save("assets/scene.altex").unwrap();

    println!("Created scene.altex");
}