use super::Mesh;
use crate::math::Vec3;

impl Mesh {
    pub fn create_cube() -> Self {
        let vertices = vec![
            Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, -0.5, -0.5),
            Vec3::new(0.5, 0.5, -0.5), Vec3::new(-0.5, 0.5, -0.5),
            Vec3::new(-0.5, -0.5, 0.5), Vec3::new(0.5, -0.5, 0.5),
            Vec3::new(0.5, 0.5, 0.5), Vec3::new(-0.5, 0.5, 0.5),
        ];
        let indices = vec![
            0,1,2, 2,3,0, 4,5,6, 6,7,4,
            0,4,7, 7,3,0, 1,5,6, 6,2,1,
            0,1,5, 5,4,0, 3,2,6, 6,7,3,
        ];
        Self::new(vertices, indices)
    }

    pub fn create_sphere() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        let rings = 16;

        for i in 0..=rings {
            let phi = std::f32::consts::PI * i as f32 / rings as f32;
            let y = -phi.cos() * 0.5;
            let r = phi.sin() * 0.5;
            for j in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / segments as f32;
                vertices.push(Vec3::new(r * theta.cos(), y, r * theta.sin()));
            }
        }

        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + 1;
                let c = (i + 1) * (segments + 1) + j;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
            }
        }

        Self::new(vertices, indices)
    }

    pub fn create_plane() -> Self {
        let vertices = vec![
            Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, -5.0),
            Vec3::new(5.0, 0.0, 5.0), Vec3::new(-5.0, 0.0, 5.0),
        ];
        let indices = vec![0,1,2, 2,3,0];
        Self::new(vertices, indices)
    }

    pub fn create_cylinder() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;

        for i in 0..segments {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = angle.cos() * 0.5;
            let z = angle.sin() * 0.5;
            vertices.push(Vec3::new(x, -0.5, z));
            vertices.push(Vec3::new(x, 0.5, z));
        }

        for i in 0..segments {
            let next = (i + 1) % segments;
            let base = (i * 2) as u32;
            let next_base = (next * 2) as u32;
            indices.extend_from_slice(&[base, base+1, next_base, next_base, base+1, next_base+1]);
        }

        Self::new(vertices, indices)
    }

    pub fn create_cone() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;

        vertices.push(Vec3::new(0.0, 0.5, 0.0));
        for i in 0..segments {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = angle.cos() * 0.5;
            let z = angle.sin() * 0.5;
            vertices.push(Vec3::new(x, -0.5, z));
        }

        for i in 0..segments {
            let next = (i + 1) % segments;
            indices.extend_from_slice(&[0, (i+1) as u32, (next+1) as u32]);
        }

        Self::new(vertices, indices)
    }

    pub fn create_torus() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        let rings = 16;
        let r1 = 0.2;
        let r2 = 0.5;

        for i in 0..=rings {
            let phi = 2.0 * std::f32::consts::PI * i as f32 / rings as f32;
            for j in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / segments as f32;
                let x = (r2 + r1 * theta.cos()) * phi.cos();
                let y = r1 * theta.sin();
                let z = (r2 + r1 * theta.cos()) * phi.sin();
                vertices.push(Vec3::new(x, y, z));
            }
        }

        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + 1;
                let c = (i + 1) * (segments + 1) + j;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
            }
        }

        Self::new(vertices, indices)
    }
}