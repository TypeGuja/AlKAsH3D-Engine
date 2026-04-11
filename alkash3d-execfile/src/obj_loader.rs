// obj_loader.rs
pub struct Mesh {
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn new(vertices: Vec<f32>, indices: Vec<u32>) -> Self {
        Self { vertices, indices }
    }
}

pub fn create_cube_mesh() -> Mesh {
    let vertices = vec![
        -0.5, -0.5,  0.5,  0.0, 0.0, 1.0,  0.0, 0.0,
        0.5, -0.5,  0.5,  0.0, 0.0, 1.0,  1.0, 0.0,
        0.5,  0.5,  0.5,  0.0, 0.0, 1.0,  1.0, 1.0,
        -0.5,  0.5,  0.5,  0.0, 0.0, 1.0,  0.0, 1.0,
    ];
    let indices = vec![0,1,2, 0,2,3];
    Mesh::new(vertices, indices)
}