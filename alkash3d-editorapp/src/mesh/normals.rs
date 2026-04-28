use super::Mesh;
use crate::math::Vec3;

impl Mesh {
    pub fn recalculate_normals(&mut self) {
        self.normals = vec![Vec3::ZERO; self.vertices.len()];

        for i in (0..self.indices.len()).step_by(3) {
            if i + 2 >= self.indices.len() { break; }

            let i0 = self.indices[i] as usize;
            let i1 = self.indices[i + 1] as usize;
            let i2 = self.indices[i + 2] as usize;

            if i0 < self.vertices.len() && i1 < self.vertices.len() && i2 < self.vertices.len() {
                let v0 = self.vertices[i0];
                let v1 = self.vertices[i1];
                let v2 = self.vertices[i2];

                let edge1 = v1 - v0;
                let edge2 = v2 - v0;
                let normal = edge1.cross(edge2);
                let len = normal.length();

                if len > 0.0001 {
                    let normal = normal * (1.0 / len);
                    self.normals[i0] = self.normals[i0] + normal;
                    self.normals[i1] = self.normals[i1] + normal;
                    self.normals[i2] = self.normals[i2] + normal;
                }
            }
        }

        use rayon::prelude::*;
        self.normals.par_iter_mut().for_each(|normal: &mut Vec3| {
            let len = normal.length();
            if len > 0.0001 {
                *normal = *normal * (1.0 / len);
            } else {
                *normal = Vec3::UP;
            }
        });
    }
}