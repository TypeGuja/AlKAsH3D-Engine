use nalgebra::{Vector3, Matrix4};

#[derive(Debug, Clone)]
pub struct Frustum {
    pub planes: [Vector3<f32>; 6],
    pub distances: [f32; 6],
}

impl Frustum {
    pub fn from_view_proj(view_proj: &Matrix4<f32>) -> Self {
        let mut planes = [Vector3::zeros(); 6];
        let mut distances = [0.0; 6];

        let m = view_proj.as_slice();

        // Left
        planes[0] = Vector3::new(m[3] + m[0], m[7] + m[4], m[11] + m[8]);
        distances[0] = m[15] + m[12];

        // Right
        planes[1] = Vector3::new(m[3] - m[0], m[7] - m[4], m[11] - m[8]);
        distances[1] = m[15] - m[12];

        // Bottom
        planes[2] = Vector3::new(m[3] + m[1], m[7] + m[5], m[11] + m[9]);
        distances[2] = m[15] + m[13];

        // Top
        planes[3] = Vector3::new(m[3] - m[1], m[7] - m[5], m[11] - m[9]);
        distances[3] = m[15] - m[13];

        // Near
        planes[4] = Vector3::new(m[3] + m[2], m[7] + m[6], m[11] + m[10]);
        distances[4] = m[15] + m[14];

        // Far
        planes[5] = Vector3::new(m[3] - m[2], m[7] - m[6], m[11] - m[10]);
        distances[5] = m[15] - m[14];

        // Normalize
        for i in 0..6 {
            let len = planes[i].norm();
            if len > 0.0 {
                planes[i] /= len;
                distances[i] /= len;
            }
        }

        Self { planes, distances }
    }

    #[inline]
    pub fn test_sphere(&self, center: Vector3<f32>, radius: f32) -> bool {
        for i in 0..6 {
            let dist = self.planes[i].dot(&center) + self.distances[i];
            if dist < -radius {
                return false;
            }
        }
        true
    }
}

pub struct Culler {
    pub lod_distances: [f32; 3],
}

impl Culler {
    pub fn new(lod_distances: [f32; 3]) -> Self {
        Self { lod_distances }
    }

    #[inline]
    pub fn get_lod_level(&self, distance: f32) -> i32 {
        if distance < self.lod_distances[0] {
            0
        } else if distance < self.lod_distances[1] {
            1
        } else if distance < self.lod_distances[2] {
            2
        } else {
            -1
        }
    }
}