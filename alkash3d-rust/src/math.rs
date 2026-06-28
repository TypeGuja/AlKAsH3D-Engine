// src/math.rs
//! Математика для 3D рендеринга

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Mat4 {
    pub m: [[f32; 4]; 4],
}

impl Mat4 {
    pub fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn translation(x: f32, y: f32, z: f32) -> Self {
        let mut m = Self::identity();
        m.m[0][3] = x;
        m.m[1][3] = y;
        m.m[2][3] = z;
        m
    }

    pub fn rotation_y(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            m: [
                [c, 0.0, s, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [-s, 0.0, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn rotation_x(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, c, -s, 0.0],
                [0.0, s, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn rotation_z(angle: f32) -> Self {
        let c = angle.cos();
        let s = angle.sin();
        Self {
            m: [
                [c, -s, 0.0, 0.0],
                [s, c, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn scaling(x: f32, y: f32, z: f32) -> Self {
        let mut m = Self::identity();
        m.m[0][0] = x;
        m.m[1][1] = y;
        m.m[2][2] = z;
        m
    }

    // ===== ИСПРАВЛЕННАЯ ПЕРСПЕКТИВНАЯ МАТРИЦА ДЛЯ DIRECTX =====
    pub fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov / 2.0).tan();
        Self {
            m: [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [0.0, 0.0, far / (far - near), 1.0],
                [0.0, 0.0, -near * far / (far - near), 0.0],  // ← исправлено
            ],
        }
    }

    pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Self {
        let f = {
            let mut v = [
                target[0] - eye[0],
                target[1] - eye[1],
                target[2] - eye[2],
            ];
            let len = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
            v[0] /= len; v[1] /= len; v[2] /= len;
            v
        };
        let r = {
            let mut v = [
                f[1]*up[2] - f[2]*up[1],
                f[2]*up[0] - f[0]*up[2],
                f[0]*up[1] - f[1]*up[0],
            ];
            let len = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
            v[0] /= len; v[1] /= len; v[2] /= len;
            v
        };
        let u = [
            r[1]*f[2] - r[2]*f[1],
            r[2]*f[0] - r[0]*f[2],
            r[0]*f[1] - r[1]*f[0],
        ];
        Self {
            m: [
                [r[0], r[1], r[2], -(r[0]*eye[0] + r[1]*eye[1] + r[2]*eye[2])],
                [u[0], u[1], u[2], -(u[0]*eye[0] + u[1]*eye[1] + u[2]*eye[2])],
                [-f[0], -f[1], -f[2], f[0]*eye[0] + f[1]*eye[1] + f[2]*eye[2]],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn multiply(&self, other: &Self) -> Self {
        let mut result = Self::identity();
        for i in 0..4 {
            for j in 0..4 {
                result.m[i][j] = self.m[i][0]*other.m[0][j]
                    + self.m[i][1]*other.m[1][j]
                    + self.m[i][2]*other.m[2][j]
                    + self.m[i][3]*other.m[3][j];
            }
        }
        result
    }

    pub fn transform_point(&self, p: &[f32; 3]) -> [f32; 4] {
        let mut result = [0.0; 4];
        result[0] = self.m[0][0]*p[0] + self.m[0][1]*p[1] + self.m[0][2]*p[2] + self.m[0][3];
        result[1] = self.m[1][0]*p[0] + self.m[1][1]*p[1] + self.m[1][2]*p[2] + self.m[1][3];
        result[2] = self.m[2][0]*p[0] + self.m[2][1]*p[1] + self.m[2][2]*p[2] + self.m[2][3];
        result[3] = self.m[3][0]*p[0] + self.m[3][1]*p[1] + self.m[3][2]*p[2] + self.m[3][3];
        result
    }

    pub fn to_array(&self) -> [[f32; 4]; 4] {
        self.m
    }
}