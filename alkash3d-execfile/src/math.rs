// math.rs
#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32, pub y: f32, pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self { Self { x, y, z } }
}

#[derive(Debug, Clone, Copy)]
pub struct Mat4 {
    pub m: [[f32; 4]; 4],
}

impl Mat4 {
    pub fn identity() -> Self {
        Self { m: [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]] }
    }

    pub fn to_array(&self) -> [f32; 16] {
        [
            self.m[0][0], self.m[0][1], self.m[0][2], self.m[0][3],
            self.m[1][0], self.m[1][1], self.m[1][2], self.m[1][3],
            self.m[2][0], self.m[2][1], self.m[2][2], self.m[2][3],
            self.m[3][0], self.m[3][1], self.m[3][2], self.m[3][3],
        ]
    }
}

// RenderObject теперь здесь
pub struct RenderObject {
    pub position: Vec3,
    pub color: [f32; 4],
}

impl RenderObject {
    pub fn new(position: Vec3, color: [f32; 4]) -> Self {
        Self { position, color }
    }
}